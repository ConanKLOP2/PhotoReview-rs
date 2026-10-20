//! Tests of `ImagePresenter::present` and the first half of `PresentCoreAsync` (W8-07i): new navigation generation, initial stat, R7-1 key refresh, RAM lookup,
//! thumbnail-vs-preview race, present the preview, preload kick (C# `ImagePresenter.cs` lines 222 to 511, baseline d92d6aae). Contract: `contracts/W8-07i.md`,
//! kind F (fakes, no oracle, no golden).
//!
//! The C# `async` waits become `pump` calls on the UI thread; every wait is bounded and gate-driven, no fixed sleeps. The tests that need the tail of a present
//! (W8-07j) or the missing-item removal (W8-07k) carried an `#[ignore]` until those units landed; none is left.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::io;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use pr_app::coordinators::presenter::state::{ImageChange, PresentError, PresentRequest, PresenterChanges};
use pr_app::coordinators::zoom_detail::same_platform_image;
use pr_app::viewmodels::format::status;
use pr_core::catalog::types::CatalogEntry;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::LoadingMode;
use pr_core::model::enums::RawPairMode;
use pr_core::sync::CancelToken;
use pr_imaging::cache::thumbnails::{EmbeddedThumbnailReaderFn, ThumbnailError};
use pr_imaging::service::PreviewImage;
use support::present_helpers::{
    FaultingGatedDecoder, GatedDecoder, ImmediateDecoder, PerPathGatedDecoder, RecordingDecoder, ScriptedFs,
    captured_log, images_shown, present, present_request, present_with, presented_paths, pump_one, start, statuses,
};
use support::presenter_fakes::{
    CountingRunner, FixtureOptions, GatedScheduler, PresenterFixture, build_thumbnail_cache_with,
};
use support::preview_fakes::{Gate, SizedDecoder, lock, opaque_platform_image, sized_image};

// ---- helpers ---------------------------------------------------------------------------------------------------------------

fn fixture(names: &[&str]) -> PresenterFixture {
    PresenterFixture::new(SizedDecoder::new(), names)
}

/// A preview image of `width x height` pixels (not downscaled) with its own platform identity.
fn image(width: i32, height: i32) -> PreviewImage {
    sized_image(width, height, width, height, false)
}

fn file_len(path: &str) -> i64 {
    i64::try_from(std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)).unwrap_or(0)
}

/// The stat the physical file system gives right now.
fn real_stat(path: &str) -> pr_core::abstractions::FileStat {
    use pr_core::abstractions::FileSystem;
    PhysicalFileSystem
        .get_file_stat(std::path::Path::new(path))
        .unwrap_or_else(|| panic!("{path} has no stat"))
}

/// `CatalogEntry` of `path` with the metadata the folder scan would have recorded.
fn scanned_entry(path: &str) -> CatalogEntry {
    let stat = real_stat(path);
    CatalogEntry::new(path)
        .unwrap_or_else(|error| panic!("entry: {error:?}"))
        .with_metadata(i64::try_from(stat.length).unwrap_or(0), stat.last_write_utc, None, None)
}

fn set_mtime(path: &str, when: SystemTime) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap_or_else(|error| panic!("open {path}: {error}"));
    file.set_modified(when)
        .unwrap_or_else(|error| panic!("set_modified {path}: {error}"));
}

fn scan_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_600) // 2026-01-01
}

fn shown_image_is(f: &PresenterFixture, expected: &PreviewImage) -> bool {
    f.presenter
        .current_image()
        .is_some_and(|shown| same_platform_image(shown, expected.platform_image()))
}

fn sink_image_is(f: &PresenterFixture, index: usize, expected: &PreviewImage) -> bool {
    lock(&f.sink_log)
        .images
        .get(index)
        .and_then(|call| call.image.as_ref())
        .is_some_and(|shown| same_platform_image(shown, expected.platform_image()))
}

/// A thumbnail reader that finds `thumbnail` at once.
fn reader_returning(thumbnail: PreviewImage) -> EmbeddedThumbnailReaderFn {
    Arc::new(move |_path: &str, _cancel: &CancelToken| Ok(Some(Arc::clone(&thumbnail))))
}

/// A thumbnail reader that blocks until `gate` opens, then finds nothing (it can never win a race while the gate is closed).
fn reader_blocked_on(gate: Arc<Gate>) -> EmbeddedThumbnailReaderFn {
    Arc::new(move |_path: &str, _cancel: &CancelToken| {
        gate.wait();
        Ok(None)
    })
}

fn options_with(
    thumbnails: Option<Arc<pr_imaging::cache::thumbnails::ThumbnailCache>>,
    file_system: Option<Arc<dyn pr_core::abstractions::FileSystem>>,
) -> FixtureOptions {
    FixtureOptions {
        thumbnails,
        file_system,
        ..FixtureOptions::default()
    }
}

/// A fixture over `decoder` whose embedded-thumbnail reader is `reader`.
fn fixture_with_reader(
    decoder: Arc<dyn pr_imaging::decoder_traits::ImageDecoder>,
    names: &[&str],
    reader: EmbeddedThumbnailReaderFn,
) -> PresenterFixture {
    fixture_with_reader_and_runner(decoder, names, reader, None)
}

/// [`fixture_with_reader`] with the background runner of the test's choice (`None` = the thread runner).
fn fixture_with_reader_and_runner(
    decoder: Arc<dyn pr_imaging::decoder_traits::ImageDecoder>,
    names: &[&str],
    reader: EmbeddedThumbnailReaderFn,
    runner: Option<Arc<dyn pr_app::coordinators::zoom_detail::BackgroundRunner>>,
) -> PresenterFixture {
    // The thumbnail folder lives next to (not inside) the fixture's own temp root, so build the cache from a scratch root of its own.
    let scratch = pr_test_support::TempRoot::new("presenter-thumbs");
    let cache = build_thumbnail_cache_with(&scratch.join("thumbs"), reader);
    let mut options = options_with(Some(cache), None);
    options.runner = runner;
    let mut f = PresenterFixture::with_options(decoder, names, options);
    // Keep the scratch folder alive as long as the fixture (leaked on purpose: it is a few empty directories under the OS temp folder).
    std::mem::forget(scratch);
    f.settings.loading_mode = Some(LoadingMode::Preview);
    f
}

fn fixture_with_fs(names: &[&str], fs: &Arc<ScriptedFs>) -> PresenterFixture {
    let file_system: Arc<dyn pr_core::abstractions::FileSystem> =
        Arc::clone(fs) as Arc<dyn pr_core::abstractions::FileSystem>;
    PresenterFixture::with_options(SizedDecoder::new(), names, options_with(None, Some(file_system)))
}

// ---- [self]: the synchronous start --------------------------------------------------------------------------------------

// C#: ImagePresenterTests.Adversarial.PresentAsync_IndexOutOfRange_IsANoOp (-1, 0, 1, int.MinValue, int.MaxValue)
#[test]
fn present_index_out_of_range_is_a_no_op() {
    for index in [-1, 0, 1, i32::MIN, i32::MAX] {
        let mut f = fixture(&[]); // empty catalog
        let changes = present_request(&mut f, PresentRequest::new(index))
            .unwrap_or_else(|error| panic!("present {index}: {error}"));
        f.pump_until_idle();

        assert_eq!(changes, PresenterChanges::NONE, "index {index}");
        assert!(f.presenter.current_image().is_none(), "index {index}");
        assert_eq!(lock(&f.sink_log).call_count(), 0, "index {index}: the sink was called");
        assert!(f.preload.notified().is_empty(), "index {index}");
        assert_eq!(f.clock.current_navigation(), 0, "index {index}: a token was taken");
    }
}

// New test: the range check comes before the override check and before any state change (contract note 1), also with a non-empty catalog.
#[test]
fn present_out_of_range_with_an_override_is_a_no_op_and_not_an_error() {
    let mut f = fixture(&["only.png"]);
    let request = PresentRequest {
        index: 5,
        allow_compare: true,
        path_override: Some("not-in-the-catalog.png".to_owned()),
        include_capture_group_in_compare: false,
    };

    let result = present_request(&mut f, request);

    assert_eq!(result, Ok(PresenterChanges::NONE));
    assert_eq!(f.clock.current_navigation(), 0);
    assert_eq!(f.catalog.current_index(), 0, "set_current must not run");
    assert!(f.preload.notified().is_empty());
}

// New test: C# `ArgumentException("The presentation path must belong to the selected catalog entry.")` is `PresentError::PathNotInEntry`, before any state change.
#[test]
fn present_path_override_that_does_not_belong_to_the_entry_is_an_error_without_side_effects() {
    let mut f = fixture(&["a.png", "b.png"]);
    let other_entry = f.path("b.png");
    let request = PresentRequest {
        index: 0,
        allow_compare: false,
        path_override: Some(other_entry),
        include_capture_group_in_compare: false,
    };

    let result = present_request(&mut f, request);

    assert_eq!(result, Err(PresentError::PathNotInEntry));
    assert_eq!(f.clock.current_navigation(), 0, "no token is taken");
    assert!(f.preload.notified().is_empty());
    assert!(f.presenter.current_presented_path().is_none());
    assert_eq!(lock(&f.sink_log).call_count(), 0);
    assert!(f.presenter.is_idle());
}

// New test: the order of the synchronous part (contract note 2): the catalog position, the token, the preload notification and the cleared info are all in place
// BEFORE the stat comes back; the stat itself runs on the stat worker.
#[test]
fn present_sets_current_notifies_preload_and_clears_the_info_before_the_stat_returns() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["a.png", "b.png"], &fs);
    let second = f.path("b.png");
    fs.hold(&second);
    let leftover = f.path("a.png");
    f.presenter.compare_mut().select(Some(leftover)); // a selection left over from a compare view

    start(&mut f, 1);

    assert_eq!(f.catalog.current_index(), 1, "INV-3: set_current before any wait");
    assert_eq!(f.clock.current_navigation(), 1);
    assert_eq!(f.preload.notified(), vec![1]);
    assert_eq!(f.presenter.current_presented_path(), Some(second.as_str()));
    assert!(f.presenter.current_photo_info().is_none());
    assert!(!f.presenter.is_compare_visible());
    assert!(
        f.presenter.compare().selected_path().is_none(),
        "compare.select(None) runs before the stat"
    );
    assert_eq!(lock(&f.sink_log).call_count(), 0, "nothing is shown before the stat");

    // Supersede the present and let its held stat finish: the late result must change nothing.
    f.clock.next_navigation();
    fs.release_all();
    f.pump_until_idle();
    assert_eq!(lock(&f.sink_log).call_count(), 0);
}

// ---- [self]: INV-1 -----------------------------------------------------------------------------------------------------------

// C#: ImagePresenterTests.PresentAsync_TokenSuperseded_AbortsUiUpdates_PreservingInv1
#[test]
fn present_token_superseded_aborts_ui_updates_preserving_inv1() {
    let mut f = fixture(&["slow1.jpg", "fast2.jpg"]);

    start(&mut f, 0);
    f.clock.next_navigation(); // interleaved immediate navigation
    f.pump_until_idle();

    assert_eq!(f.clock.current_navigation(), 2);
    assert_eq!(
        lock(&f.sink_log).call_count(),
        0,
        "the stale present must not touch the sink"
    );
    assert!(f.presenter.current_image().is_none());
}

// C#: ImagePresenterTests.Adversarial.PresentAsync_TokenSuperseded_ShowsNothingFromTheStaleNavigation
#[test]
fn present_token_superseded_shows_nothing_from_the_stale_navigation() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["stale.png"]);

    start(&mut f, 0);
    f.clock.next_navigation(); // a newer navigation took over before the decode came back
    decoder.release();
    f.pump_until_idle();

    assert!(presented_paths(&f).is_empty());
    assert!(f.presenter.current_image().is_none());
    assert!(
        statuses(&f).is_empty(),
        "no loading and no error status: {:?}",
        statuses(&f)
    );
}

// New test: the same, but superseded AFTER the viewer decode started (the stat continuation was current): the preview event is the one that is dropped.
#[test]
fn present_token_superseded_after_the_decode_started_shows_nothing_from_the_stale_navigation() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["stale.png"]);
    // Fast mode: no thumbnail event, so the PREVIEW event is the one that meets the superseded token.
    f.settings.loading_mode = Some(LoadingMode::Fast);

    start(&mut f, 0);
    pump_one(&mut f); // the stat came back: the loading status shows and the viewer decode is requested
    assert!(decoder.wait_started(1));
    let before = statuses(&f);
    assert_eq!(before.len(), 1, "only the loading status: {before:?}");
    f.clock.next_navigation();
    decoder.release();
    f.pump_until_idle();

    assert!(presented_paths(&f).is_empty());
    assert_eq!(images_shown(&f), 0);
    assert_eq!(statuses(&f), before, "no status after the supersede");
    assert!(f.presenter.current_image().is_none());
}

// C#: ImagePresenterTests.PresentAsync_SupersededNavigation_ObservesLaterPreviewFault
// Rust has no unobserved task exception: the twin checks that a superseded present whose preview fails later shows nothing and leaves no in-flight preview.
#[test]
fn present_superseded_navigation_with_a_later_preview_failure_changes_nothing() {
    let decoder = FaultingGatedDecoder::new("marker-r2-f29");
    let read_started = Gate::new();
    let proceed = Gate::new();
    let reader: EmbeddedThumbnailReaderFn = {
        let (read_started, proceed) = (Arc::clone(&read_started), Arc::clone(&proceed));
        Arc::new(move |_path: &str, _cancel: &CancelToken| {
            read_started.open();
            proceed.wait();
            Ok(Some(sized_image(160, 160, 160, 160, false)))
        })
    };
    let mut f = fixture_with_reader(Arc::clone(&decoder) as _, &["fault-after-supersede.png"], reader);
    let path = f.path("fault-after-supersede.png");

    start(&mut f, 0);
    pump_one(&mut f); // the stat: the preview and the thumbnail are requested
    assert!(read_started.wait());
    f.clock.next_navigation(); // the thumbnail wins the race and, while it is being read, a newer navigation supersedes this one
    proceed.open();
    pump_one(&mut f); // the thumbnail event: stale, dropped
    assert!(presented_paths(&f).is_empty(), "it really was superseded");
    assert_eq!(images_shown(&f), 0);

    decoder.release_and_throw();
    assert!(decoder.wait_threw());
    let key = f
        .service
        .get_current_cache_key(&path)
        .unwrap_or_else(|error| panic!("cache key: {error}"));
    // The failed preview's event arrives; the in-flight entry is removed before the waiter is woken.
    f.pump_until(|f| !f.service.has_inflight_preview(&key));
    f.pump_until_idle();

    assert!(presented_paths(&f).is_empty());
    assert_eq!(images_shown(&f), 0);
    assert!(
        statuses(&f).iter().all(|status| !status.contains("marker-r2-f29")),
        "a stale failure must not be reported: {:?}",
        statuses(&f)
    );
}

// New test (contract note 2, T3): the next present cancels the previous present's viewer decode. A decode that waits in its start delay (a key-held burst) is dropped
// before it starts, so the decoder only ever sees the current navigation's file.
#[test]
fn present_cancels_the_previous_viewer_decode_when_the_next_navigation_starts() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["superseded.png", "current.png"]);
    let superseded = f.path("superseded.png");

    f.preload.set_viewer_decode_delay(Duration::from_secs(30)); // burst: the first navigation waits before decoding
    start(&mut f, 0);
    pump_one(&mut f); // its stat came back: the viewer decode is requested and sits in its start delay
    f.preload.set_viewer_decode_delay(Duration::ZERO);
    let key = f
        .service
        .get_current_cache_key(&superseded)
        .unwrap_or_else(|error| panic!("key: {error}"));
    assert!(f.service.has_inflight_preview(&key));

    start(&mut f, 1); // cancels the first present's viewer token
    // The dropped decode ends without ever starting, and the second present's stat comes back and starts its own decode (both need the pump).
    f.pump_until(|f| !f.service.has_inflight_preview(&key) && decoder.started() >= 1);
    assert_eq!(decoder.started(), 1, "the superseded decode must not start");

    f.clock.next_navigation();
    decoder.release();
    f.pump_until_idle();
    assert!(presented_paths(&f).is_empty());
}

// ---- [self]: stat errors -------------------------------------------------------------------------------------------------

// C#: ImagePresenterTests.StatErrors.PresentAsync_WhenStatFailsWithIoError_KeepsPhotoInCatalogAndReportsError
#[test]
fn present_stat_io_error_keeps_photo_in_catalog_and_reports_error() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["flaky.png", "other.png"], &fs);
    let flaky = f.path("flaky.png");
    let other = f.path("other.png");
    fs.fail_with(&flaky, || io::Error::other("share hiccup"));

    present(&mut f, 0);

    assert_eq!(&*f.catalog.paths(), &[flaky.clone(), other][..]);
    assert_eq!(f.catalog.current_index(), 0);
    let last = statuses(&f).last().cloned().unwrap_or_default();
    assert!(last.contains("share hiccup"), "{last}");
    assert!(f.presenter.status_needs_attention());
    assert!(last.contains("flaky.png"), "{last}");
}

// C#: ImagePresenterTests.StatErrors.PresentAsync_WhenStatFailsWithIoError_ClearsThePreviouslyShownImage
#[test]
fn present_stat_io_error_clears_the_previously_shown_image() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["good.png", "flaky.png"], &fs);
    let flaky = f.path("flaky.png");
    fs.fail_with(&flaky, || io::Error::other("share hiccup"));
    f.presenter.sink_mut().set_current_image(
        Some(opaque_platform_image()),
        false,
        &ImageChange {
            presented_path: None,
            original_width: 1,
            original_height: 1,
            is_same_source_swap: false,
        },
    );

    present(&mut f, 1);

    // The error status names flaky.png; the previous photo must not stay visible under it.
    assert!(lock(&f.sink_log).images.last().is_some_and(|call| call.image.is_none()));
    assert!(f.presenter.current_image().is_none());
}

// C#: ImagePresenterTests.StatErrors.PresentAsync_WhenStatFailsWithIoError_ClearsThePreviouslyShownComparePair
#[test]
fn present_stat_io_error_clears_the_previously_shown_compare_pair() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["photo.jpg", "photo (1).jpg", "flaky.png"], &fs);
    let (p1, p2, flaky) = (f.path("photo.jpg"), f.path("photo (1).jpg"), f.path("flaky.png"));
    fs.fail_with(&flaky, || io::Error::other("share hiccup"));

    present(&mut f, 0);
    assert!(f.presenter.compare().is_visible(), "precondition: a pair is on screen");

    present(&mut f, 2);

    // The error status names flaky.png; the old A/B pair must not stay visible under it, and the catalog is untouched.
    let compare = f.presenter.compare();
    assert!(!compare.is_visible());
    assert!(compare.left_path().is_none());
    assert!(compare.right_path().is_none());
    assert!(compare.left_image().is_none());
    assert!(compare.right_image().is_none());
    assert!(!f.presenter.is_compare_visible());
    assert_eq!(&*f.catalog.paths(), &[p1, p2, flaky][..]);
}

// ---- [self]: the stat result (R7-1, RAM lookup, initial status) ---------------------------------------------------------------

// New test (contract note 4): a catalog entry that no longer matches the stat is refreshed BEFORE the key is built, so the preview decoded is keyed by the file as it is now.
#[test]
fn present_stat_mismatch_refreshes_the_catalog_entry_before_the_key_is_built() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["edited.png"]);
    let path = f.path("edited.png");
    let stale = scanned_entry(&path);
    f.catalog.reset(vec![stale.clone()], RawPairMode::Separate, None);
    std::fs::write(&path, [1u8; 9]).unwrap_or_else(|error| panic!("write: {error}"));
    set_mtime(&path, scan_time() + Duration::from_secs(3600));
    let new_stat = real_stat(&path);

    start(&mut f, 0);
    pump_one(&mut f); // the stat came back

    let entry = f.catalog.find(&path).cloned().unwrap_or_else(|| panic!("entry"));
    assert_eq!(entry.length, Some(9));
    assert_eq!(entry.last_write_utc, Some(new_stat.last_write_utc));
    // The decode was requested for the NEW version: its in-flight key is the key of the refreshed entry.
    assert!(decoder.wait_started(1));
    let key = f
        .service
        .get_current_cache_key_for_entry(&entry)
        .unwrap_or_else(|error| panic!("key: {error}"));
    assert!(f.service.has_inflight_preview(&key));

    f.clock.next_navigation();
    decoder.release();
    f.pump_until_idle();
}

// New test (contract note 4, C# `MutationGaps.PresentAsync_CaptureMemberOverride_*`): a presentation of a capture-group member (path override) never touches
// the representative's catalog metadata, not even when the member's stat differs from it.
#[test]
fn present_capture_member_override_leaves_the_catalog_metadata_alone_at_the_initial_stat() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["rep.png", "member.png"]);
    let (rep, member) = (f.path("rep.png"), f.path("member.png"));
    std::fs::write(&member, [7u8; 11]).unwrap_or_else(|error| panic!("write: {error}"));
    let group = pr_core::catalog::types::CaptureGroup::new(&rep, &member, None)
        .unwrap_or_else(|error| panic!("group: {error:?}"));
    let mut entry = scanned_entry(&rep);
    entry.capture_group = Some(group);
    f.catalog.reset(vec![entry.clone()], RawPairMode::Separate, None);

    let request = PresentRequest {
        index: 0,
        allow_compare: false,
        path_override: Some(member.clone()),
        include_capture_group_in_compare: false,
    };
    present_request(&mut f, request).unwrap_or_else(|error| panic!("present: {error}"));
    pump_one(&mut f);

    let after = f.catalog.find(&rep).cloned().unwrap_or_else(|| panic!("entry"));
    assert_eq!(after.length, entry.length);
    assert_eq!(after.last_write_utc, entry.last_write_utc);
    assert_eq!(f.presenter.current_presented_path(), Some(member.as_str()));
    // The member's own stat differs from the representative's metadata, yet nothing is evicted for it either (note 4: `path_override.is_none()`).
    assert!(f.preload.removed_for_path().is_empty());

    f.clock.next_navigation();
    decoder.release();
    f.pump_until_idle();
}

// New test (contract note 5): a cold start shows the loading status WITH attention, `last_present_started_from_ram` is false, no preload hit is counted, and the
// photo information stays empty until the image arrives.
#[test]
fn present_cold_start_shows_the_loading_status_with_attention() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["cold.png"]);
    let path = f.path("cold.png");

    start(&mut f, 0);
    pump_one(&mut f);

    let expected = status::loading(0, 1, file_len(&path), &f.format.ctx());
    assert_eq!(statuses(&f), vec![expected.clone()]);
    assert_eq!(f.presenter.status_text(), expected);
    assert!(f.presenter.status_needs_attention());
    assert!(!f.presenter.last_present_started_from_ram());
    assert!(f.presenter.current_photo_info().is_none());
    assert_eq!(f.metrics.snapshot().preload_hits, 0);
    assert!(
        f.preload.preload_around_calls().is_empty(),
        "the kick comes after the preview is presented"
    );

    f.clock.next_navigation();
    decoder.release();
    f.pump_until_idle();
}

// New test (contract note 5, RAM-only accounting): a warmed preload key is consumed and counted ONLY on a RAM hit; a cold start leaves it alone.
#[test]
fn present_cold_start_does_not_consume_a_warmed_preload_key() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["warm.png"]);
    let path = f.path("warm.png");
    let key = f
        .service
        .get_current_cache_key(&path)
        .unwrap_or_else(|error| panic!("key: {error}"));
    f.preload.warm(key);

    start(&mut f, 0);
    pump_one(&mut f); // the stat: nothing in RAM

    assert!(f.preload.consumed().is_empty());
    assert_eq!(f.metrics.snapshot().preload_hits, 0);

    f.clock.next_navigation();
    decoder.release();
    f.pump_until_idle();
}

// New test (contract note 2, T6): `last_present_started_from_ram` is reset at the START of every accepted present, so it is false while the stat is pending,
// not only after the lookup.
#[test]
fn present_resets_last_present_started_from_ram_before_the_stat() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["warm.png", "cold.png"], &fs);
    let (warm, cold) = (f.path("warm.png"), f.path("cold.png"));
    f.service
        .get_preview_path(&warm)
        .wait()
        .unwrap_or_else(|error| panic!("pre-warm: {error}"));
    present(&mut f, 0);
    assert!(f.presenter.last_present_started_from_ram());

    fs.hold(&cold);
    start(&mut f, 1);

    assert!(fs.wait_started(&cold));
    assert!(
        !f.presenter.last_present_started_from_ram(),
        "the previous present's true must not outlive the start of the next one"
    );
    f.clock.next_navigation();
    fs.release_all();
    f.pump_until_idle();
}

// New test (contract note 6): a thumbnail that ends CANCELLED while its navigation is current is a failure of the present, not "no thumbnail".
#[test]
fn present_thumbnail_cancellation_while_current_is_a_failure() {
    let decoder = GatedDecoder::new(image(100, 100));
    let reader: EmbeddedThumbnailReaderFn =
        Arc::new(|_path: &str, _cancel: &CancelToken| Err(ThumbnailError::Canceled));
    let mut f = fixture_with_reader(Arc::clone(&decoder) as _, &["thumb-cancel.png"], reader);

    start(&mut f, 0);
    pump_one(&mut f); // the stat
    pump_one(&mut f); // the cancelled thumbnail (the preview is gated)

    assert!(f.presenter.status_needs_attention());
    assert!(
        statuses(&f)
            .last()
            .is_some_and(|status| status.contains("thumb-cancel.png"))
    );
    assert!(presented_paths(&f).is_empty());
    decoder.release();
    f.pump_until_idle();
    assert!(
        presented_paths(&f).is_empty(),
        "the failed present ended; the late preview is dropped"
    );
}

// New test (contract note 6, I-09): the thumbnail wins, so it is shown as a placeholder (sized by its own original size when no decode told us the size), the initial view
// mode is applied, and the status becomes "loading full resolution" with attention; the crossfade flag is false for the first image of a folder.
#[test]
fn present_thumbnail_first_shows_a_placeholder_and_the_loading_full_res_status() {
    let thumbnail = sized_image(160, 120, 640, 480, true);
    let decoder = GatedDecoder::new(image(1920, 1080));
    let mut f = fixture_with_reader(
        Arc::clone(&decoder) as _,
        &["thumb.png"],
        reader_returning(Arc::clone(&thumbnail)),
    );
    let path = f.path("thumb.png");

    start(&mut f, 0);
    pump_one(&mut f); // the stat: the preview decode is requested and the thumbnail is requested
    pump_one(&mut f); // the thumbnail event (the preview is gated)

    assert!(shown_image_is(&f, &thumbnail));
    assert_eq!(
        (
            f.presenter.current_original_width(),
            f.presenter.current_original_height()
        ),
        (640, 480)
    );
    assert_eq!(lock(&f.sink_log).initial_view_mode_count, 1);
    let log = lock(&f.sink_log);
    assert_eq!(log.images.len(), 1);
    assert!(!log.images[0].is_file_change);
    assert_eq!(log.images[0].presented_path.as_deref(), Some(path.as_str()));
    drop(log);
    let loading_full_res = status::loading_full_res(0, 1, file_len(&path), &f.format.ctx());
    assert_eq!(statuses(&f).last(), Some(&loading_full_res));
    assert!(f.presenter.status_needs_attention());
    assert!(presented_paths(&f).is_empty(), "the thumbnail is not 'presented'");

    f.clock.next_navigation();
    decoder.release();
    f.pump_until_idle();
}

// New test (contract note 6, I-04): the thumbnail is requested in Preview mode only. The presenter hands one job to its runner for the preview wait, and one more
// for the thumbnail wait in Preview mode; in Fast mode the thumbnail reader is never asked.
#[test]
fn present_asks_for_a_thumbnail_in_preview_mode_only() {
    for (mode, expected_jobs) in [(LoadingMode::Preview, 2), (LoadingMode::Fast, 1)] {
        let decoder = GatedDecoder::new(image(100, 100));
        let runner = CountingRunner::new();
        let mut f = fixture_with_reader_and_runner(
            Arc::clone(&decoder) as _,
            &["mode.png"],
            Arc::new(|_path: &str, _cancel: &CancelToken| Ok(None)),
            Some(Arc::clone(&runner) as _),
        );
        f.settings.loading_mode = Some(mode);
        let before = runner.jobs();

        start(&mut f, 0);
        pump_one(&mut f); // the stat came back: the preview (and maybe the thumbnail) is requested

        assert_eq!(runner.jobs() - before, expected_jobs, "{mode:?}");
        assert!(decoder.wait_started(1), "{mode:?}: the preview decode did start");

        f.clock.next_navigation();
        decoder.release();
        f.pump_until_idle();
    }
}

// ---- [needs 07j]: out-of-order and stale continuations --------------------------------------------------------------------

// C#: ReentrantSupersede.PresentAsync_SupersededByReentrantPresentBeforeViewerDecodeStarts_ReadsTheCapturedTokenNotTheDisposedSource
// Rust cannot re-enter from the sink: the second present is called AFTER the first stat event is queued and BEFORE pump.
#[test]
fn present_superseded_between_stat_and_viewer_decode_uses_the_captured_token() {
    let log = captured_log();
    let mut f = fixture(&["first.png", "second.png"]);
    let (first, second) = (f.path("first.png"), f.path("second.png"));

    start(&mut f, 0);
    assert!(
        f.presenter.wait_for_event(support::preview_fakes::TIMEOUT),
        "the first stat event is queued"
    );
    start(&mut f, 1); // supersedes the first present: its decode token is cancelled, its stat event is stale
    f.pump_until_idle();

    assert_eq!(presented_paths(&f), vec![second.clone()]);
    for line in log.lines_with("ShowImage failed") {
        assert!(!line.contains(&first) && !line.contains(&second), "{line}");
    }
    for line in log.lines_with("ObjectDisposedException") {
        assert!(!line.contains(&first) && !line.contains(&second), "{line}");
    }
}

// C#: StatOffUiThread.PresentAsync_WhileStatIsHeld_DoesNotBlockTheCallingThread
#[test]
fn present_returns_while_the_stat_is_held_and_the_stat_never_runs_on_the_calling_thread() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["slow-share.png"], &fs);
    let photo = f.path("slow-share.png");
    fs.hold(&photo);

    let caller = std::thread::current().id();
    fs.set_caller_thread(Some(caller));
    start(&mut f, 0);
    fs.set_caller_thread(None);

    // The call came back although the stat has not finished: the calling (UI) thread was never blocked on it.
    assert!(fs.wait_started(&photo));
    assert!(!f.presenter.is_idle(), "the present is still waiting for its stat");
    assert!(presented_paths(&f).is_empty());
    assert!(fs.stats_on_caller_thread().is_empty());

    fs.release(&photo);
    f.pump_until_idle();

    assert_eq!(presented_paths(&f), vec![photo.clone()]);
    assert!(fs.stats_on_caller_thread().is_empty());
    assert!(
        fs.stats().iter().all(|(_, thread)| *thread != caller),
        "a stat ran on the calling thread"
    );
}

// C#: StatOffUiThread.PresentAsync_FileRewrittenWhileStatPending_NeverShowsTheStaleRamPreview
#[test]
fn present_file_rewritten_while_stat_pending_never_shows_the_stale_ram_preview() {
    let fs = ScriptedFs::new();
    let decoder = SizedDecoder::new();
    decoder.set_original(1, 1);
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &["edited-on-share.png"],
        options_with(None, Some(Arc::clone(&fs) as _)),
    );
    let path = f.path("edited-on-share.png");
    set_mtime(&path, scan_time());
    f.catalog.reset(vec![scanned_entry(&path)], RawPairMode::Separate, None);
    present(&mut f, 0);
    let stale = f
        .presenter
        .current_image()
        .cloned()
        .unwrap_or_else(|| panic!("an image is shown"));
    assert_eq!(f.presenter.current_original_width(), 1);
    lock(&f.sink_log).images.clear();
    lock(&f.sink_log).presented_paths.clear();

    fs.hold(&path);
    start(&mut f, 0);
    assert!(fs.wait_started(&path));
    // Edited in another app while the (slow) stat is in flight; the catalog still has the scan's metadata.
    std::fs::write(&path, [9u8; 12]).unwrap_or_else(|error| panic!("write: {error}"));
    set_mtime(&path, scan_time() + Duration::from_secs(3600));
    decoder.set_original(3, 2);
    fs.release(&path);
    f.pump_until_idle();

    assert_eq!(f.presenter.current_original_width(), 3);
    assert_eq!(f.presenter.current_original_height(), 2);
    assert!(
        lock(&f.sink_log).images.iter().all(|call| call
            .image
            .as_ref()
            .is_none_or(|shown| !same_platform_image(shown, &stale))),
        "the stale RAM preview was shown"
    );
    assert_eq!(f.catalog.find(&path).and_then(|entry| entry.length), Some(12));
}

// C#: StatOffUiThread.PresentAsync_FileDeletedWhileStatPending_SkipsItWithoutShowingItsCachedPreview
#[test]
fn present_file_deleted_while_stat_pending_skips_it_without_showing_its_cached_preview() {
    let fs = ScriptedFs::new();
    let decoder = SizedDecoder::new();
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &["doomed.png", "survivor.png"],
        options_with(None, Some(Arc::clone(&fs) as _)),
    );
    let (doomed, survivor) = (f.path("doomed.png"), f.path("survivor.png"));
    decoder.set_original(2, 2);
    present(&mut f, 0);
    let doomed_image = f
        .presenter
        .current_image()
        .cloned()
        .unwrap_or_else(|| panic!("an image is shown"));
    present(&mut f, 1);
    lock(&f.sink_log).images.clear();
    lock(&f.sink_log).presented_paths.clear();

    fs.hold(&doomed);
    start(&mut f, 0);
    assert!(fs.wait_started(&doomed));
    std::fs::remove_file(&doomed).unwrap_or_else(|error| panic!("delete: {error}"));
    fs.release(&doomed);
    f.pump_until_idle();

    assert_eq!(&*f.catalog.paths(), &[survivor.clone()][..]);
    assert_eq!(presented_paths(&f), vec![survivor]);
    assert!(
        lock(&f.sink_log).images.iter().all(|call| call
            .image
            .as_ref()
            .is_none_or(|shown| !same_platform_image(shown, &doomed_image))),
        "the deleted file's cached preview was shown"
    );
    assert_eq!(f.presenter.current_original_width(), 2);
}

// C#: StatOffUiThread.PresentAsync_SupersededWhileStatPending_StaleStatErrorIsIgnored
#[test]
fn present_superseded_while_stat_pending_stale_stat_error_is_ignored() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["flaky-share.png", "next.png"], &fs);
    let (flaky, next) = (f.path("flaky-share.png"), f.path("next.png"));
    fs.fail_with(&flaky, || io::Error::other("share hiccup"));
    fs.hold(&flaky);

    start(&mut f, 0);
    assert!(fs.wait_started(&flaky));
    start(&mut f, 1); // the user moved on while the share was slow
    fs.release(&flaky);
    f.pump_until_idle();

    assert_eq!(presented_paths(&f), vec![next.clone()]);
    assert!(f.presenter.current_image().is_some());
    assert!(lock(&f.sink_log).images.iter().all(|call| call.image.is_some()));
    assert!(statuses(&f).iter().all(|status| !status.contains("share hiccup")));
    assert_eq!(&*f.catalog.paths(), &[flaky, next][..]);
    assert_eq!(f.catalog.current_index(), 1);
}

// C#: StatOffUiThread.PresentAsync_SupersededWhileStatPending_SuccessfulStatResumesWithoutTouchingDisposedSource
#[test]
fn present_superseded_while_stat_pending_successful_stat_resumes_quietly() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["first.png", "second.png"], &fs);
    let (first, second) = (f.path("first.png"), f.path("second.png"));
    let log = captured_log();
    fs.hold(&first);

    start(&mut f, 0);
    assert!(fs.wait_started(&first));
    start(&mut f, 1); // replaces the first navigation's decode token while its stat is pending
    fs.release(&first); // the first stat now resumes with a Found result, after its token was cancelled
    f.pump_until_idle();

    assert_eq!(presented_paths(&f), vec![second]);
    assert_eq!(f.catalog.current_index(), 1);
    assert!(
        log.lines_with("ShowImage failed")
            .iter()
            .all(|line| !line.contains(&first))
    );
}

// C#: StatOffUiThread.PresentAsync_BurstBehindSlowStat_SkipsTheSupersededStats
#[test]
fn present_burst_behind_slow_stat_skips_the_superseded_stats() {
    let fs = ScriptedFs::new();
    let mut f = fixture_with_fs(&["slow.png", "skipped.png", "last.png"], &fs);
    let (slow, skipped, last) = (f.path("slow.png"), f.path("skipped.png"), f.path("last.png"));
    fs.hold(&slow);

    start(&mut f, 0);
    assert!(fs.wait_started(&slow));
    start(&mut f, 1); // queued behind the held stat, then superseded
    start(&mut f, 2);
    fs.release(&slow);
    f.pump_until_idle();

    assert_eq!(presented_paths(&f), vec![last]);
    assert_eq!(fs.stat_count(&skipped), 0, "a superseded stat must not touch the disk");
    assert_eq!(f.catalog.current_index(), 2);
}

// ---- [needs 07j]: the deferred preload kick ---------------------------------------------------------------------------------

// C#: DeferredPreloadKick.PresentAsync_DeferNextPreloadKick_KicksAfterTheYieldOnceThenGoesBackToSynchronous
// The scheduler's actions are released by index: 0 = the deferred kick, 1 = the post-frame yield of the single-image branch.
#[test]
fn present_defer_next_preload_kick_kicks_after_the_yield_once_then_goes_back_to_synchronous() {
    let scheduler = GatedScheduler::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["first.png", "second.png"],
        FixtureOptions {
            scheduler: Some(Arc::clone(&scheduler) as _),
            ..FixtureOptions::default()
        },
    );
    f.presenter.set_defer_next_preload_kick(true);

    start(&mut f, 0);
    f.pump_until(|_| scheduler.posted_count() >= 2); // deferred kick + post-frame bookkeeping
    assert!(f.preload.preload_around_calls().is_empty());
    assert!(
        !f.presenter.defer_next_preload_kick(),
        "consumed when the kick was scheduled"
    );

    assert!(scheduler.release(0));
    f.pump();
    assert_eq!(f.preload.preload_around_calls(), vec![0]);
    assert!(scheduler.release(1));
    f.pump_until_idle();
    assert_eq!(f.preload.preload_around_calls(), vec![0], "exactly one kick");

    start(&mut f, 1);
    f.pump_until(|_| scheduler.posted_count() >= 3); // only the post-frame yield: the kick did not wait for one
    assert_eq!(f.preload.preload_around_calls(), vec![0, 1]);
    assert!(scheduler.release(2));
    f.pump_until_idle();
}

// C#: DeferredPreloadKick.PresentAsync_DeferNextPreloadKick_NavigationSupersededBeforeTheYield_DropsTheKick
#[test]
fn present_defer_next_preload_kick_navigation_superseded_before_the_yield_drops_the_kick() {
    let scheduler = GatedScheduler::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["only.png"],
        FixtureOptions {
            scheduler: Some(Arc::clone(&scheduler) as _),
            ..FixtureOptions::default()
        },
    );
    f.presenter.set_defer_next_preload_kick(true);

    start(&mut f, 0);
    f.pump_until(|_| scheduler.posted_count() >= 2);
    f.clock.next_navigation(); // the user moved on: another presentation owns the kick now

    scheduler.release_all();
    f.pump_until_idle();

    assert!(f.preload.preload_around_calls().is_empty());
    assert!(!f.presenter.defer_next_preload_kick());
}

// C#: DeferredPreloadKick.PresentAsync_WithoutDeferFlag_KicksBeforeAnyYield
#[test]
fn present_without_defer_flag_kicks_before_any_yield() {
    let scheduler = GatedScheduler::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["plain.png"],
        FixtureOptions {
            scheduler: Some(Arc::clone(&scheduler) as _),
            ..FixtureOptions::default()
        },
    );

    start(&mut f, 0);
    f.pump_until(|_| scheduler.posted_count() >= 1); // the post-frame yield: the kick is already behind us

    assert_eq!(f.preload.preload_around_calls(), vec![0]);
    assert!(scheduler.release(0));
    f.pump_until_idle();
    assert_eq!(f.preload.preload_around_calls(), vec![0]);
}

// C#: DeferredPreloadKick.PresentAsync_DeferFlagWithoutScheduler_KicksSynchronously
#[test]
fn present_defer_flag_without_scheduler_kicks_synchronously() {
    let mut f = fixture(&["noscheduler.png"]);
    f.presenter.set_defer_next_preload_kick(true);

    present(&mut f, 0);

    assert_eq!(f.preload.preload_around_calls(), vec![0]);
    assert!(
        f.presenter.defer_next_preload_kick(),
        "without a scheduler the flag is not consumed"
    );
}

// ---- [needs 07j]: R7-1, RAM and preload accounting --------------------------------------------------------------------------

// C#: ImagePresenterTests.PresentAsync_WhenFileRewrittenAfterScan_PresentsNewContentAndRefreshesCatalog (R7-1)
#[test]
fn present_file_rewritten_after_scan_presents_new_content_and_refreshes_catalog() {
    let decoder = SizedDecoder::new();
    decoder.set_original(1, 1);
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["edited.png"]);
    let path = f.path("edited.png");
    set_mtime(&path, scan_time());
    f.catalog.reset(vec![scanned_entry(&path)], RawPairMode::Separate, None);

    present(&mut f, 0);
    assert_eq!(f.presenter.current_original_width(), 1);

    // Edited in another app: different size and mtime, catalog still holds the scan's metadata.
    std::fs::write(&path, [3u8; 17]).unwrap_or_else(|error| panic!("write: {error}"));
    set_mtime(&path, scan_time() + Duration::from_secs(3600));
    decoder.set_original(3, 2);
    let edited = real_stat(&path);

    present(&mut f, 0);

    assert_eq!(f.presenter.current_original_width(), 3);
    assert_eq!(f.presenter.current_original_height(), 2);
    let entry = f.catalog.find(&path).cloned().unwrap_or_else(|| panic!("entry"));
    assert_eq!(entry.length, Some(i64::try_from(edited.length).unwrap_or(0)));
    assert_eq!(entry.last_write_utc, Some(edited.last_write_utc));
    // The old version's RAM entry is gone: only the new key is cached for this path.
    assert_eq!(f.service.cache_count(), 1);
}

// C#: ImagePresenterTests.PresentAsync_WhenRamHit_DirectlyPresentsCachedImageAndConsumesPreloadedKey
#[test]
fn present_when_ram_hit_directly_presents_cached_image_and_consumes_preloaded_key() {
    let mut f = fixture(&["ram1.jpg"]);
    let path = f.path("ram1.jpg");
    // Pre-warm the preview in the service.
    f.service
        .get_preview_path(&path)
        .wait()
        .unwrap_or_else(|error| panic!("pre-warm: {error}"));
    let key = f
        .service
        .get_current_cache_key(&path)
        .unwrap_or_else(|error| panic!("key: {error}"));
    f.preload.warm(key);

    present(&mut f, 0);

    assert_eq!(f.metrics.snapshot().preload_hits, 1);
    assert_eq!(f.preload.consumed().len(), 1);
    assert_eq!(
        f.metrics.snapshot().cache_hits,
        1,
        "the RAM hit is counted once, by the presenter"
    );
    assert!(presented_paths(&f).contains(&path));
    assert!(f.presenter.current_image().is_some());
    assert!(f.presenter.last_present_started_from_ram());
}

// C#: ImagePresenterTests.PresentAsync_LastPresentStartedFromRam_TracksRamHit
#[test]
fn present_last_present_started_from_ram_tracks_ram_hit() {
    let mut f = fixture(&["state1.jpg", "state2.jpg"]);
    let (p1, p2) = (f.path("state1.jpg"), f.path("state2.jpg"));
    f.service
        .get_preview_path(&p2)
        .wait()
        .unwrap_or_else(|error| panic!("pre-warm: {error}"));

    present(&mut f, 0); // cold: shows the loading status first
    assert!(!f.presenter.last_present_started_from_ram());

    present(&mut f, 1); // warm: shown straight from RAM
    assert!(f.presenter.last_present_started_from_ram());

    f.presenter
        .evict_cached_path(&p1)
        .unwrap_or_else(|error| panic!("evict: {error}"));
    present(&mut f, 0); // cold again: the previous present's true must not leak
    assert!(!f.presenter.last_present_started_from_ram());
}

// C#: ImagePresenterTests.PresentAsync_PreloadController_PreloadAroundTriggered
#[test]
fn present_preload_controller_preload_around_triggered() {
    let mut f = fixture(&["img1.jpg", "img2.jpg"]);

    present(&mut f, 0);

    assert!(f.preload.preload_around_calls().contains(&0));
}

// C#: ImagePresenterTests.PresentAsync_NotifiesPreloadOfEachNavigation
#[test]
fn present_notifies_preload_of_each_navigation() {
    let mut f = fixture(&["nav1.jpg", "nav2.jpg"]);

    present(&mut f, 0);
    present(&mut f, 1);

    assert_eq!(f.preload.notified(), vec![0, 1]);
    assert_eq!(f.preload.preload_around_calls(), vec![0, 1]);
}

// ---- [needs 07j]: the thumbnail / preview race ----------------------------------------------------------------------------

// C#: ImagePresenterTests.PresentAsync_WhenPreviewFasterThanThumbnail_ShowsPreviewDirectlyAndSkipsThumbnail
#[test]
fn present_when_preview_faster_than_thumbnail_shows_preview_directly_and_skips_thumbnail() {
    let preview = image(1920, 1080);
    // A thumbnail reader that never completes during this test: it cannot possibly win, so if the preview is ever shown it is because the race logic picked it.
    let never = Gate::new();
    let mut f = fixture_with_reader(
        Arc::new(ImmediateDecoder {
            image: Arc::clone(&preview),
        }),
        &["preview-wins.jpg"],
        reader_blocked_on(Arc::clone(&never)),
    );

    present(&mut f, 0); // can only finish if the present never waits for the thumbnail

    assert!(shown_image_is(&f, &preview));
    assert_eq!(images_shown(&f), 1, "no thumbnail was ever shown");
    never.open();
}

// C#: ImagePresenterTests.PresentAsync_WhenThumbnailFasterThanPreview_ShowsThumbnailThenReplacesWithPreview
#[test]
fn present_when_thumbnail_faster_than_preview_shows_thumbnail_then_replaces_with_preview() {
    let thumbnail = image(160, 160);
    let preview = image(1920, 1080);
    let decoder = GatedDecoder::new(Arc::clone(&preview));
    let mut f = fixture_with_reader(
        Arc::clone(&decoder) as _,
        &["thumbnail-wins.jpg"],
        reader_returning(Arc::clone(&thumbnail)),
    );

    start(&mut f, 0);
    pump_one(&mut f); // the stat
    pump_one(&mut f); // the thumbnail wins: the decode is gated
    assert!(shown_image_is(&f, &thumbnail));

    decoder.release(); // now let the preview finish; the present must replace the thumbnail with it
    f.pump_until_idle();

    assert!(shown_image_is(&f, &preview));
}

// C#: ImagePresenterTests.PresentAsync_ThumbnailReplacedByPreview_SinkNeverRequestsTransitionForTheUpgrade
#[test]
fn present_thumbnail_replaced_by_preview_sink_never_requests_transition_for_the_upgrade() {
    let thumbnail = image(160, 160);
    let preview = image(1920, 1080);
    let decoder = GatedDecoder::new(Arc::clone(&preview));
    let mut f = fixture_with_reader(
        Arc::clone(&decoder) as _,
        &["crossfade-same-file.jpg"],
        reader_returning(Arc::clone(&thumbnail)),
    );

    start(&mut f, 0);
    pump_one(&mut f);
    pump_one(&mut f);
    decoder.release();
    f.pump_until_idle();

    assert_eq!(lock(&f.sink_log).images.len(), 2);
    assert!(sink_image_is(&f, 0, &thumbnail));
    assert!(sink_image_is(&f, 1, &preview));
    // The SECOND call (the preview replacing the thumbnail for the SAME file) must never request a transition.
    assert!(!lock(&f.sink_log).file_change_flags()[1]);
}

// C#: ImagePresenterTests.PresentAsync_NavigatingToADifferentFile_RequestsTransitionOnlyOnceForTheFirstBitmap
#[test]
fn present_navigating_to_a_different_file_requests_transition_only_once_for_the_first_bitmap() {
    let mut f = fixture(&["crossfade-first.jpg", "crossfade-second.jpg"]);

    // LoadingMode.Preview + not in RAM + no embedded thumbnail: a single set_current_image call per navigation, straight to the preview.
    present(&mut f, 0);
    assert_eq!(lock(&f.sink_log).images.len(), 1);
    assert!(!lock(&f.sink_log).file_change_flags()[0]); // first image after opening the folder: nothing to fade from

    present(&mut f, 1);
    assert_eq!(lock(&f.sink_log).images.len(), 2);
    assert!(lock(&f.sink_log).file_change_flags()[1]); // a real navigation to a different file
}

// C#: ImagePresenterTests.PresentAsync_WhenThumbnailFaultsBeforePreview_StillPresentsThePreview
#[test]
fn present_when_thumbnail_faults_before_preview_still_presents_the_preview() {
    let preview = image(1920, 1080);
    let decoder = GatedDecoder::new(Arc::clone(&preview)); // the preview cannot win the race before the thumbnail faults
    let faulted = Gate::new();
    let reader: EmbeddedThumbnailReaderFn = {
        let faulted = Arc::clone(&faulted);
        Arc::new(move |_path: &str, _cancel: &CancelToken| {
            faulted.open();
            Err(ThumbnailError::Reader(Arc::new(io::Error::other(
                "damaged thumbnail metadata",
            ))))
        })
    };
    let mut f = fixture_with_reader(Arc::clone(&decoder) as _, &["thumbnail-faults.jpg"], reader);
    let path = f.path("thumbnail-faults.jpg");

    start(&mut f, 0);
    pump_one(&mut f); // the stat
    assert!(faulted.wait());
    pump_one(&mut f); // the faulted thumbnail: treated as "no thumbnail"
    decoder.release();
    f.pump_until_idle();

    assert!(shown_image_is(&f, &preview));
    assert!(presented_paths(&f).contains(&path));
}

// ---- [needs 07j]: superseded decode, AR16, bursts -------------------------------------------------------------------------

// C#: ImagePresenterTests.PresentAsync_SupersededNavigation_DropsPendingViewerDecode
#[test]
fn present_superseded_navigation_drops_pending_viewer_decode() {
    let decoder = RecordingDecoder::new();
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["superseded.jpg", "current.jpg"]);
    let (f1, f2) = (f.path("superseded.jpg"), f.path("current.jpg"));

    f.preload.set_viewer_decode_delay(Duration::from_secs(30)); // burst: the first navigation waits before decoding
    start(&mut f, 0);
    pump_one(&mut f); // its stat came back: the viewer decode is requested and sits in its start delay
    f.preload.set_viewer_decode_delay(Duration::ZERO);
    start(&mut f, 1);
    f.pump_until_idle();
    let key1 = f
        .service
        .get_current_cache_key(&f1)
        .unwrap_or_else(|error| panic!("key: {error}"));
    f.pump_until(|f| !f.service.has_inflight_preview(&key1)); // the cancelled decode's event arrives

    assert_eq!(decoder.paths().len(), 1);
    assert!(decoder.paths()[0].eq_ignore_ascii_case(&f2), "{:?}", decoder.paths());
    assert!(presented_paths(&f).contains(&f2));
    assert!(!presented_paths(&f).contains(&f1));
    assert!(statuses(&f).iter().all(|status| !status.contains("superseded.jpg")));
}

// C#: ImagePresenterTests.PresentAsync_CatalogPrunedDuringDecode_PreloadUsesTheCurrentIndex (AR16)
#[test]
fn present_catalog_pruned_during_decode_preload_uses_the_current_index() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(
        Arc::clone(&decoder) as _,
        &["pruned0.jpg", "pruned1.jpg", "pruned2.jpg"],
    );
    let (p0, p2) = (f.path("pruned0.jpg"), f.path("pruned2.jpg"));

    start(&mut f, 2);
    pump_one(&mut f); // the stat; the decode is requested
    f.catalog.remove_paths(&[p0]); // the background readability probe keeps the current path and shifts its index
    decoder.release();
    f.pump_until_idle();

    assert_eq!(f.catalog.current().map(CatalogEntry::path), Some(p2.as_str()));
    assert_eq!(f.preload.preload_around_calls(), vec![1]);
}

// C#: Adversarial.PresentAsync_BurstCompletingInRandomOrder_ShowsOnlyTheLastNavigation (seeds 1 to 5)
fn burst_completing_in_random_order(seed: u64) {
    const COUNT: usize = 8;
    let names: Vec<String> = (0..COUNT).map(|i| format!("burst{i}.png")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let decoder = PerPathGatedDecoder::new();
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &name_refs);
    let paths: Vec<String> = names.iter().map(|name| f.path(name)).collect();

    start(&mut f, 0);
    pump_one(&mut f);
    // The first decode has really started (so it cannot be dropped as "not yet started") before the burst supersedes it; the rest are cancelled while pending.
    assert!(decoder.wait_started(&paths[0]));
    for index in 1..COUNT {
        start(&mut f, i32::try_from(index).unwrap_or(0));
    }

    // Finish the decodes in a seeded random order (xorshift), the last navigation somewhere in the middle.
    let mut order: Vec<usize> = (0..COUNT).collect();
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for position in (1..COUNT).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        order.swap(position, usize::try_from(state % (position as u64 + 1)).unwrap_or(0));
    }
    for index in order {
        decoder.open(&paths[index]);
    }
    f.pump_until_idle();

    assert!(
        decoder.timed_out().is_empty(),
        "APP-T23: a decode gave up on its gate: {:?}",
        decoder.timed_out()
    );
    assert_eq!(presented_paths(&f), vec![paths[COUNT - 1].clone()]);
    assert_eq!(f.catalog.current_index(), i32::try_from(COUNT - 1).unwrap_or(0));
    assert_eq!(images_shown(&f), 1);
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.file_name.clone()),
        Some(names[COUNT - 1].clone())
    );
}

#[test]
fn present_burst_completing_in_random_order_shows_only_the_last_navigation_seed_1() {
    burst_completing_in_random_order(1);
}

#[test]
fn present_burst_completing_in_random_order_shows_only_the_last_navigation_seed_2() {
    burst_completing_in_random_order(2);
}

#[test]
fn present_burst_completing_in_random_order_shows_only_the_last_navigation_seed_3() {
    burst_completing_in_random_order(3);
}

#[test]
fn present_burst_completing_in_random_order_shows_only_the_last_navigation_seed_4() {
    burst_completing_in_random_order(4);
}

#[test]
fn present_burst_completing_in_random_order_shows_only_the_last_navigation_seed_5() {
    burst_completing_in_random_order(5);
}

// ---- [needs 07j]: capture-group member override ----------------------------------------------------------------------------

// C#: MutationGaps.PresentAsync_CaptureMemberOverride_DoesNotOverwriteTheRepresentativeMetadata
#[test]
fn present_capture_member_override_does_not_overwrite_the_representative_metadata() {
    let decoder = SizedDecoder::new();
    decoder.set_original(5, 4);
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["rep.png", "member.png"]);
    let (rep, member) = (f.path("rep.png"), f.path("member.png"));
    std::fs::write(&member, [5u8; 20]).unwrap_or_else(|error| panic!("write: {error}"));
    let rep_stat = real_stat(&rep);
    assert_ne!(rep_stat.length, 20);
    let group = pr_core::catalog::types::CaptureGroup::new(&rep, &member, None)
        .unwrap_or_else(|error| panic!("group: {error:?}"));
    let mut entry = scanned_entry(&rep);
    entry.capture_group = Some(group);
    f.catalog.reset(vec![entry], RawPairMode::Separate, None);

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: false,
            path_override: Some(member),
            include_capture_group_in_compare: false,
        },
    );

    let after = f.catalog.find(&rep).cloned().unwrap_or_else(|| panic!("entry"));
    assert_eq!(after.length, Some(i64::try_from(rep_stat.length).unwrap_or(0)));
    assert_eq!(after.last_write_utc, Some(rep_stat.last_write_utc));
    assert_eq!(f.presenter.current_original_width(), 5);
}

// C#: ImagePresenterTests.PresentAsync_CaptureGroupMemberOverride_DoesNotOverwriteRepresentativeMetadata
#[test]
fn present_capture_group_member_override_does_not_overwrite_representative_metadata() {
    let mut f = fixture(&["capture-rep.jpg", "capture-member.png"]);
    let (jpeg, member) = (f.path("capture-rep.jpg"), f.path("capture-member.png"));
    std::fs::write(&member, [3u8; 30]).unwrap_or_else(|error| panic!("write: {error}"));
    assert_ne!(file_len(&jpeg), file_len(&member));
    let group = pr_core::catalog::types::CaptureGroup::new(&jpeg, &member, None)
        .unwrap_or_else(|error| panic!("group: {error:?}"));
    let mut entry = CatalogEntry::new(jpeg.as_str()).unwrap_or_else(|error| panic!("entry: {error:?}"));
    entry.capture_group = Some(group);
    f.catalog.reset(vec![entry], RawPairMode::Separate, None);

    present(&mut f, 0);
    let rep_length = f.catalog.find(&jpeg).and_then(|entry| entry.length);
    assert_eq!(rep_length, Some(file_len(&jpeg)));

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: false,
            path_override: Some(member.clone()),
            include_capture_group_in_compare: false,
        },
    );

    assert_eq!(f.presenter.current_presented_path(), Some(member.as_str()));
    assert_eq!(f.catalog.find(&jpeg).and_then(|entry| entry.length), rep_length);
}
