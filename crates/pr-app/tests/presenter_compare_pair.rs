//! Tests of the compare-pair lookup and the compare preview load (W8-07k): `HasComparePair` / `GetComparePair` with its structural-version cache, and
//! `LoadComparePreviewAsync` (C# `ImagePresenter.cs` lines 772 to 791 and 865 to 884, baseline d92d6aae). Contract: `contracts/W8-07k.md`, kind F.
//!
//! The pair rules themselves (numbered pairs, tie order) are W2-06's; these tests pin what the presenter adds: the capture-group pair, the version-keyed cache
//! and the busy retry of the preview load.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pr_app::coordinators::presenter::compare_pair::{BUSY_RETRY_DELAY, BusyRetryDelay, load_compare_preview_blocking};
use pr_app::coordinators::presenter::state::PresentRequest;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::{CaptureGroup, CatalogEntry};
use pr_core::model::enums::RawPairMode;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::service::PreviewError;
use pr_test_support::TempRoot;
use support::present_helpers::{MemberScriptDecoder, present, present_with};
use support::presenter_fakes::PresenterFixture;
use support::preview_fakes::{ServiceOptions, SizedDecoder, build_service_with_metrics, sized_image};

fn fixture(names: &[&str]) -> PresenterFixture {
    PresenterFixture::new(SizedDecoder::new(), names)
}

fn group_entry(jpeg: &str, member: &str) -> CatalogEntry {
    let group = CaptureGroup::new(jpeg, member, None).unwrap_or_else(|error| panic!("group: {error:?}"));
    let mut entry = CatalogEntry::new(jpeg).unwrap_or_else(|error| panic!("entry: {error:?}"));
    entry.capture_group = Some(group);
    entry
}

fn catalog_of(paths: &[&str]) -> ReviewCatalog {
    let mut catalog = ReviewCatalog::new();
    let entries = paths
        .iter()
        .map(|path| CatalogEntry::new(*path).unwrap_or_else(|error| panic!("entry: {error:?}")))
        .collect();
    catalog.reset(entries, RawPairMode::Separate, None);
    catalog
}

fn compare_request(index: i32, include_capture_group: bool) -> PresentRequest {
    PresentRequest {
        index,
        allow_compare: true,
        path_override: None,
        include_capture_group_in_compare: include_capture_group,
    }
}

// ---- HasComparePair / GetComparePair ----------------------------------------------------------------------------------------------

// C#: ImagePresenterTests.PresentAsync_CaptureGroup_CompareShowsBothMembersAndCanPresentAlternateMember (the `HasComparePair` assertion)
#[test]
fn has_compare_pair_capture_group_is_true() {
    let mut f = fixture(&["capture.jpg", "capture-member.jpg"]);
    let (jpeg, member) = (f.path("capture.jpg"), f.path("capture-member.jpg"));
    f.catalog
        .reset(vec![group_entry(&jpeg, &member)], RawPairMode::Separate, None);

    assert_eq!(f.presenter.has_compare_pair(&f.catalog, &jpeg), Ok(true));
    // The group is found by ANY member path.
    assert_eq!(f.presenter.has_compare_pair(&f.catalog, &member), Ok(true));
}

// New test: a numbered pair (the C# `ComparePairService` rule) is a pair for both of its files; an unrelated file is not.
#[test]
fn has_compare_pair_numbered_files_are_a_pair_and_a_lone_file_is_not() {
    let mut f = fixture(&["photo.jpg", "photo (1).jpg", "lone.jpg"]);
    let (a, b, lone) = (f.path("photo.jpg"), f.path("photo (1).jpg"), f.path("lone.jpg"));

    assert_eq!(f.presenter.has_compare_pair(&f.catalog, &a), Ok(true));
    assert_eq!(f.presenter.has_compare_pair(&f.catalog, &b), Ok(true));
    assert_eq!(f.presenter.has_compare_pair(&f.catalog, &lone), Ok(false));
}

// New test (contract note 4): the capture-group pair is always JPEG first, RAW second, whichever member is presented.
#[test]
fn get_compare_pair_capture_group_is_jpeg_then_raw() {
    let mut f = fixture(&["shot.jpg", "shot-member.png"]);
    let (jpeg, member) = (f.path("shot.jpg"), f.path("shot-member.png"));
    f.catalog
        .reset(vec![group_entry(&jpeg, &member)], RawPairMode::Separate, None);

    present_with(
        &mut f,
        PresentRequest {
            path_override: Some(member.clone()),
            ..compare_request(0, true)
        },
    );

    assert_eq!(f.presenter.compare().left_path(), Some(jpeg.as_str()));
    assert_eq!(f.presenter.compare().right_path(), Some(member.as_str()));
}

// New test (contract note 4): without the capture-group flag (and with the compare closed) the version-keyed numbered index is used, even for a group entry.
#[test]
fn get_compare_pair_uses_the_index_when_the_capture_group_is_not_included() {
    let mut f = fixture(&["photo.jpg", "photo.cr3", "photo (1).jpg"]);
    let (jpeg, raw, numbered) = (f.path("photo.jpg"), f.path("photo.cr3"), f.path("photo (1).jpg"));
    f.catalog.reset(
        vec![
            group_entry(&jpeg, &raw),
            CatalogEntry::new(numbered.as_str()).unwrap_or_else(|error| panic!("entry: {error:?}")),
        ],
        RawPairMode::Separate,
        None,
    );

    present_with(&mut f, compare_request(0, false));
    assert_eq!(f.presenter.compare().left_path(), Some(jpeg.as_str()));
    assert_eq!(
        f.presenter.compare().right_path(),
        Some(numbered.as_str()),
        "the numbered pair, not the capture group"
    );

    // Close the compare, then ask for the capture group explicitly.
    f.presenter.compare_mut().clear(&f.format.ctx());
    present_with(&mut f, compare_request(0, true));
    assert_eq!(f.presenter.compare().right_path(), Some(raw.as_str()));
}

// New test (contract note 4): the index is rebuilt only when the structural version changed. Two catalogs with the same version share one cached index, so the
// second one's own pair is not found until its version differs.
#[test]
fn get_compare_pair_rebuilds_the_index_only_when_the_structural_version_changes() {
    let mut f = fixture(&[]);
    let first = catalog_of(&["C:\\p\\photo.jpg", "C:\\p\\photo (1).jpg"]);
    let mut second = catalog_of(&["C:\\q\\x.jpg", "C:\\q\\x (1).jpg"]);
    assert_eq!(first.structural_version(), second.structural_version(), "precondition");

    assert_eq!(f.presenter.has_compare_pair(&first, "C:\\p\\photo.jpg"), Ok(true));
    // Same version: the cached index (built from `first`) answers, so `second`'s pair is unknown.
    assert_eq!(f.presenter.has_compare_pair(&second, "C:\\q\\x.jpg"), Ok(false));
    assert_eq!(
        f.presenter.has_compare_pair(&first, "C:\\p\\photo.jpg"),
        Ok(true),
        "still cached"
    );

    // A structural change of `second` (a restore) makes its version differ: the index is rebuilt from it.
    assert!(second.restore("C:\\q\\y.jpg", 2));
    assert_ne!(first.structural_version(), second.structural_version());
    assert_eq!(f.presenter.has_compare_pair(&second, "C:\\q\\x.jpg"), Ok(true));
}

// New test (contract note 4, T5): the lookup is OrdinalIgnoreCase (the index's own lookup, never a lower-cased copy).
#[test]
fn get_compare_pair_lookup_is_ordinal_ignore_case() {
    let mut f = fixture(&[]);
    let catalog = catalog_of(&["C:\\p\\Photo.jpg", "C:\\p\\Photo (1).jpg"]);

    assert_eq!(f.presenter.has_compare_pair(&catalog, "C:\\P\\PHOTO.JPG"), Ok(true));
    assert_eq!(f.presenter.has_compare_pair(&catalog, "c:\\p\\photo (1).jpg"), Ok(true));
}

// New test (open question O2 of the contract): a catalog path `GetFullPath` rejects is an error, not a panic and not `false`.
#[test]
fn get_compare_pair_invalid_catalog_path_is_an_error() {
    let mut f = fixture(&[]);
    let catalog = catalog_of(&["C:\\p\\ok.jpg", "C:\\p\\bad\0name.jpg"]);

    assert_eq!(
        f.presenter.has_compare_pair(&catalog, "C:\\p\\ok.jpg"),
        Err(PathError::Argument)
    );
}

// ---- LoadComparePreview -----------------------------------------------------------------------------------------------------------

/// A service whose decoder is busy for the first `busy_calls` decodes of `shot.png`, over a real file in a scratch folder.
fn busy_service(
    busy_calls: usize,
    other: Option<DecodeError>,
) -> (
    TempRoot,
    Arc<pr_imaging::service::PreviewImageService>,
    Arc<MemberScriptDecoder>,
    String,
) {
    let root = TempRoot::new("compare-load");
    let path = root.join("shot.png");
    std::fs::write(&path, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap_or_else(|error| panic!("write: {error}"));
    let other = std::sync::Mutex::new(other);
    let decoder = MemberScriptDecoder::new("shot.png", sized_image(100, 100, 100, 100, false), move |call| {
        if call <= busy_calls {
            return Err(DecodeError::busy());
        }
        match other.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    });
    let service = build_service_with_metrics(
        &root.join("cache"),
        Arc::clone(&decoder) as _,
        ServiceOptions::default(),
        Arc::new(pr_core::diagnostics::metrics::ReviewMetrics::new()),
    );
    (root, service, decoder, path.to_string_lossy().into_owned())
}

fn counting_delay(result: bool) -> (BusyRetryDelay, Arc<AtomicUsize>, Arc<std::sync::Mutex<Vec<Duration>>>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let spans = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (counter, recorded) = (Arc::clone(&calls), Arc::clone(&spans));
    let delay: BusyRetryDelay = Arc::new(move |span: Duration, _cancel: &CancelToken| {
        counter.fetch_add(1, Ordering::SeqCst);
        recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(span);
        result
    });
    (delay, calls, spans)
}

// New test (`LoadComparePreviewAsync` has no isolated C# test; the presenter tests of 07j cover it end to end)
#[test]
fn load_compare_preview_retries_busy_once_after_the_delay() {
    let (_root, service, decoder, path) = busy_service(1, None);
    let (delay, calls, spans) = counting_delay(true);

    let image = load_compare_preview_blocking(&service, &path, &CancelToken::none(), &delay);

    assert!(image.is_ok(), "{:?}", image.err());
    assert_eq!(decoder.member_calls(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *spans.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![BUSY_RETRY_DELAY]
    );
    assert_eq!(BUSY_RETRY_DELAY, Duration::from_millis(250));
}

#[test]
fn load_compare_preview_second_busy_is_returned() {
    let (_root, service, decoder, path) = busy_service(2, None);
    let (delay, calls, _spans) = counting_delay(true);

    let result = load_compare_preview_blocking(&service, &path, &CancelToken::none(), &delay);

    assert!(result.as_ref().is_err_and(PreviewError::is_busy), "{:?}", result.err());
    assert_eq!(decoder.member_calls(), 2, "one bounded retry");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn load_compare_preview_other_error_is_not_retried() {
    let (_root, service, decoder, path) = busy_service(
        0,
        Some(DecodeError::InvalidData {
            message: "not an image".to_owned(),
        }),
    );
    let (delay, calls, _spans) = counting_delay(true);

    let result = load_compare_preview_blocking(&service, &path, &CancelToken::none(), &delay);

    assert!(result.is_err());
    assert_eq!(decoder.member_calls(), 1);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "no pause for an error that is not busy"
    );
}

#[test]
fn load_compare_preview_cancelled_delay_is_canceled() {
    let (_root, service, decoder, path) = busy_service(1, None);
    let (delay, calls, _spans) = counting_delay(false); // the navigation token fired during the pause

    let result = load_compare_preview_blocking(&service, &path, &CancelToken::none(), &delay);

    assert!(matches!(result, Err(PreviewError::Canceled)), "{:?}", result.err());
    assert_eq!(decoder.member_calls(), 1, "no second attempt after a cancelled pause");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// New test (contract note 8): a file that is gone has no cache key; that is returned as `Key`, never retried.
#[test]
fn load_compare_preview_missing_file_is_a_key_error() {
    let (root, service, decoder, path) = busy_service(0, None);
    std::fs::remove_file(&path).unwrap_or_else(|error| panic!("delete: {error}"));
    let (delay, calls, _spans) = counting_delay(true);

    let result = load_compare_preview_blocking(&service, &path, &CancelToken::none(), &delay);

    assert!(matches!(result, Err(PreviewError::Key(_))), "{:?}", result.err());
    assert_eq!(decoder.member_calls(), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(root);
}

// New test (contract note 8): the default retry delay waits in slices and reports a cancellation at once.
#[test]
fn default_busy_retry_delay_reports_cancellation_and_elapsed() {
    let delay = pr_app::coordinators::presenter::compare_pair::default_busy_retry_delay();
    let cancelled = CancelToken::new();
    cancelled.cancel();

    assert!(
        !delay(Duration::from_secs(30), &cancelled),
        "a cancelled token ends the wait at once"
    );
    assert!(delay(Duration::ZERO, &CancelToken::none()), "a zero delay has elapsed");
    assert!(
        delay(Duration::from_millis(30), &CancelToken::none()),
        "a short delay elapses"
    );
}

// New test: a presenter built WITHOUT a busy-retry delay uses the default one (it used to be a stub that panicked).
#[test]
fn a_presenter_without_a_busy_retry_delay_presents_a_compare_pair() {
    use pr_app::coordinators::presenter::state::{ImagePresenter, ImagePresenterDeps};
    let mut f = fixture(&["photo.jpg", "photo (1).jpg"]);
    // Rebuild the presenter of the fixture with `busy_retry_delay: None` (the fixture itself injects an instant one).
    let deps = ImagePresenterDeps {
        clock: Arc::clone(&f.clock),
        preview_service: Arc::clone(&f.service),
        thumbnail_cache: support::presenter_fakes::build_thumbnail_cache(&f.root.join("thumbs2")),
        preload: Arc::clone(&f.preload) as _,
        hasher: support::presenter_fakes::FakeHasher::new("H"),
        metrics: Arc::clone(&f.metrics),
        session_store: None,
        session_writer: None,
        sink: Box::new(support::presenter_fakes::RecordingSink::new().0),
        file_system: None,
        on_presented_hook: None,
        ui_scheduler: None,
        busy_retry_delay: None,
        runner: None,
        stat_worker: Some(pr_core::io::navigation_stat::NavigationStatWorker::new()),
        wake: Arc::new(|| {}),
        indicator_delay: None,
    };
    f.presenter = ImagePresenter::new(
        deps,
        pr_app::viewmodels::compare::CompareViewModel::new(&f.format.ctx()),
    );

    present(&mut f, 0);

    assert!(f.presenter.is_compare_visible());
}
