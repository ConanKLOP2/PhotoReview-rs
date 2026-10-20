//! Tests of the second half of `PresentCoreAsync` (W8-07j): the branch decision, the compare branch (previews, sizes, hashes, the partner failure), the single-image
//! branch (original dimensions, the refresh stat), the session save and the metric (step 7), and the three `catch` clauses (C# `ImagePresenter.cs` lines 500 to 712,
//! `CompareViewModel.LoadAsync` lines 166 to 255, baseline d92d6aae). Contract: `contracts/W8-07j.md`, kind F (fakes, no oracle, no golden).
//!
//! The C# `async` waits become `pump` calls on the UI thread; every wait is bounded and gate-driven, no fixed sleeps. Tests that must change something "while the
//! decode runs" do it between two pumps, from the test thread that owns the catalog.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use pr_app::coordinators::presenter::state::{PresentRequest, PresenterChanges};
use pr_app::coordinators::zoom_detail::same_platform_image;
use pr_app::misc::file_hash::{FileHashError, FileHasher};
use pr_app::viewmodels::format::status::{self, CompareStatus};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::types::{CaptureGroup, CatalogEntry};
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::LoadingMode;
use pr_core::model::enums::RawPairMode;
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::session::writer::SessionWriter;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::service::{DecodeThreadKind, PreviewImage, PreviewImageService};
use pr_l10n::template::LocArg;
use pr_test_support::TempRoot;
use support::present_helpers::{
    FaultingGatedDecoder, GatedDecoder, GatedInfoDecoder, HoldFirstFailureDecoder, HoldRunner, MemberScriptDecoder,
    NamedScriptDecoder, NeverFiringTimer, RecordingSpawner, RewritingDecoder, ScriptedFs, ThrowingInfoDecoder,
    images_shown, present, present_with, presented_paths, pump_one, start, statuses,
};
use support::presenter_fakes::{FixtureOptions, FormatFixture, GatedScheduler, PresenterFixture};
use support::preview_fakes::{ServiceOptions, SizedDecoder, build_service_full, lock, sized_image};

// ---- helpers ---------------------------------------------------------------------------------------------------------------

fn fixture(names: &[&str]) -> PresenterFixture {
    PresenterFixture::new(SizedDecoder::new(), names)
}

fn image(width: i32, height: i32) -> PreviewImage {
    sized_image(width, height, width, height, false)
}

fn file_len(path: &str) -> i64 {
    i64::try_from(std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)).unwrap_or(0)
}

fn real_stat(path: &str) -> pr_core::abstractions::FileStat {
    use pr_core::abstractions::FileSystem;
    PhysicalFileSystem
        .get_file_stat(Path::new(path))
        .unwrap_or_else(|| panic!("{path} has no stat"))
}

fn scanned_entry(path: &str) -> CatalogEntry {
    let stat = real_stat(path);
    CatalogEntry::new(path)
        .unwrap_or_else(|error| panic!("entry: {error:?}"))
        .with_metadata(i64::try_from(stat.length).unwrap_or(0), stat.last_write_utc, None, None)
}

/// A catalog entry of `jpeg` that is a capture group with `member`.
fn group_entry(jpeg: &str, member: &str) -> CatalogEntry {
    let group = CaptureGroup::new(jpeg, member, None).unwrap_or_else(|error| panic!("group: {error:?}"));
    let mut entry = CatalogEntry::new(jpeg).unwrap_or_else(|error| panic!("entry: {error:?}"));
    entry.capture_group = Some(group);
    entry
}

fn override_request(index: i32, path: &str) -> PresentRequest {
    PresentRequest {
        index,
        allow_compare: false,
        path_override: Some(path.to_owned()),
        include_capture_group_in_compare: false,
    }
}

/// The text a failed image shows ahead of the file name (taken from the catalog, not typed here).
fn image_error_prefix(f: &PresenterFixture) -> String {
    let marker = status::image_error("\u{1}", "\u{2}", &f.format.ctx());
    marker.split('\u{1}').next().unwrap_or_default().to_owned()
}

fn no_error_status(f: &PresenterFixture) -> bool {
    let prefix = image_error_prefix(f);
    statuses(f).iter().all(|status| !status.starts_with(&prefix))
}

/// A preview service over `decoder` that the test owns (metrics and thread spawner of its own), with its scratch folder.
struct ServiceRig {
    _scratch: TempRoot,
    service: Arc<PreviewImageService>,
    metrics: Arc<ReviewMetrics>,
    spawner: Arc<RecordingSpawner>,
}

fn service_rig(decoder: Arc<dyn ImageDecoder>) -> ServiceRig {
    let scratch = TempRoot::new("presenter-service");
    let metrics = Arc::new(ReviewMetrics::new());
    let spawner = RecordingSpawner::new();
    let service = build_service_full(
        &scratch.join("cache"),
        decoder,
        ServiceOptions::default(),
        Arc::clone(&metrics),
        Some(Arc::clone(&spawner) as _),
    );
    ServiceRig {
        _scratch: scratch,
        service,
        metrics,
        spawner,
    }
}

fn fixture_with_service(rig: &ServiceRig, names: &[&str], mut options: FixtureOptions) -> PresenterFixture {
    options.prebuilt_service = Some(Arc::clone(&rig.service));
    PresenterFixture::with_options(SizedDecoder::new(), names, options)
}

/// A session store over a scratch data root (C# `SessionStore(new AppPaths(_tempDir), ..)`).
struct SessionRig {
    _data: TempRoot,
    store: Arc<SessionStore>,
}

fn session_rig() -> SessionRig {
    let data = TempRoot::new("presenter-sessions");
    let paths = AppPaths::new(data.path(), None, false);
    let store = Arc::new(SessionStore::new(&paths, Arc::new(PhysicalFileSystem), None));
    SessionRig { _data: data, store }
}

fn with_session(f: &mut PresenterFixture) -> String {
    let folder = f.root.path().to_string_lossy().into_owned();
    f.session = Some(SessionState {
        folder: folder.clone(),
        current_path: Some(String::new()),
        ..SessionState::default()
    });
    folder
}

fn saved_current_path(rig: &SessionRig, folder: &str) -> Option<String> {
    rig.store.load(folder).ok().and_then(|state| state.current_path)
}

/// What a [`ScriptHasher`] answers for a file name.
type HashScript = Box<dyn Fn(&str) -> Result<String, FileHashError> + Send + Sync>;

/// A hasher the test scripts per file name.
struct ScriptHasher {
    answer: HashScript,
    calls: AtomicUsize,
}

impl std::fmt::Debug for ScriptHasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptHasher").finish_non_exhaustive()
    }
}

impl ScriptHasher {
    fn new(answer: impl Fn(&str) -> Result<String, FileHashError> + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            answer: Box::new(answer),
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl FileHasher for ScriptHasher {
    fn get(&self, path: &Path, _cancel: &CancelToken) -> Result<String, FileHashError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        (self.answer)(&name)
    }
}

// ---- [self]: the single-image branch -------------------------------------------------------------------------------------

// C#: ImagePresenterTests.PresentAsync_WhenSingleImage_DisplaysFormattedStatusWithDimensions
#[test]
fn present_single_image_displays_formatted_status_with_dimensions() {
    let mut f = fixture(&["single.jpg"]);
    let path = f.path("single.jpg");

    present(&mut f, 0);

    assert!(!f.presenter.is_compare_visible());
    assert_eq!(presented_paths(&f), vec![path.clone()]);
    let expected = status::with_dimensions(0, 1, file_len(&path), 6000, 4000, "single.jpg", &f.format.ctx());
    assert_eq!(f.presenter.status_text(), expected);
    assert!(f.presenter.status_text().contains("1/1"));
    assert!(f.presenter.status_text().contains("single.jpg"));
    assert!(!f.presenter.status_needs_attention());
    assert!(lock(&f.sink_log).initial_view_mode_count >= 1);
    assert_eq!(f.metrics.snapshot().presented_images, 1);
}

// New test (contract note 4): the status of the single-image branch is set only after the frame yielded; until then the loading status stands, and the
// session is not saved before it.
#[test]
fn present_single_image_waits_for_the_frame_before_the_status_bookkeeping() {
    let scheduler = GatedScheduler::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["frame.png"],
        FixtureOptions {
            scheduler: Some(Arc::clone(&scheduler) as _),
            ..FixtureOptions::default()
        },
    );
    let path = f.path("frame.png");

    start(&mut f, 0);
    f.pump_until(|_| scheduler.posted_count() >= 1); // the post-frame yield
    let loading = status::loading(0, 1, file_len(&path), &f.format.ctx());
    assert_eq!(f.presenter.status_text(), loading, "the frame has not rendered yet");
    assert_eq!(images_shown(&f), 1, "the image itself is already assigned");

    assert!(scheduler.release(0));
    f.pump_until_idle();
    let ready = status::with_dimensions(0, 1, file_len(&path), 6000, 4000, "frame.png", &f.format.ctx());
    assert_eq!(f.presenter.status_text(), ready);
}

// New test (contract note 4, INV-1 after the frame yield): a navigation superseded while the frame is pending does no bookkeeping: no dimension lookup, no refresh stat.
#[test]
fn present_single_image_superseded_during_the_frame_yield_does_no_bookkeeping() {
    let scheduler = GatedScheduler::new();
    let fs = ScriptedFs::new();
    let decoder = GatedInfoDecoder::new(image(100, 100));
    let rig = service_rig(Arc::clone(&decoder) as _);
    let service = Arc::clone(&rig.service);
    let mut f = fixture_with_service(
        &rig,
        &["frame.png"],
        FixtureOptions {
            scheduler: Some(Arc::clone(&scheduler) as _),
            file_system: Some(Arc::clone(&fs) as _),
            // The dimensions the decode seeded are evicted right after the image is shown, so a live continuation would have to ReadInfo.
            on_presented_hook: Some(Box::new(move |_path: &str| service.clear_cache())),
            ..FixtureOptions::default()
        },
    );
    let path = f.path("frame.png");
    start(&mut f, 0);
    f.pump_until(|_| scheduler.posted_count() >= 1);
    let before = statuses(&f);
    f.clock.next_navigation();

    scheduler.release_all();
    f.pump_until_idle();

    assert_eq!(statuses(&f), before, "no status after the supersede");
    assert_eq!(f.metrics.snapshot().presented_images, 0);
    assert!(!decoder.info_started(), "no dimension lookup for a superseded present");
    assert_eq!(
        fs.stat_count(&path),
        1,
        "only the initial stat: no refresh stat for a superseded present"
    );
}

// New test (contract note 4, Original mode): no lookup, the size is the preview's own pixel size.
#[test]
fn present_original_mode_status_uses_the_preview_pixel_size_without_a_lookup() {
    let decoder = GatedDecoder::new(image(800, 600));
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &["orig.png"],
        FixtureOptions {
            service: ServiceOptions {
                original_loading_mode: true,
                ..ServiceOptions::default()
            },
            ..FixtureOptions::default()
        },
    );
    f.settings.loading_mode = Some(LoadingMode::Original);
    let path = f.path("orig.png");
    decoder.release();

    present(&mut f, 0);

    let expected = status::with_dimensions(0, 1, file_len(&path), 800, 600, "orig.png", &f.format.ctx());
    assert_eq!(f.presenter.status_text(), expected);
}

// C#: ImagePresenterTests.PresentAsync_WhenSessionActive_UpdatesAndSavesSessionCurrentPath
#[test]
fn present_session_active_updates_and_saves_session_current_path() {
    let rig = session_rig();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["item1.jpg"],
        FixtureOptions {
            session_store: Some(Arc::clone(&rig.store)),
            ..FixtureOptions::default()
        },
    );
    let folder = with_session(&mut f);
    let path = f.path("item1.jpg");

    present(&mut f, 0);

    assert_eq!(
        f.session.as_ref().and_then(|s| s.current_path.clone()),
        Some(path.clone())
    );
    assert_eq!(saved_current_path(&rig, &folder), Some(path));
}

// C#: ImagePresenterTests.PresentAsync_WithSessionWriter_DefersSessionWriteUntilFlush
#[test]
fn present_with_session_writer_defers_session_write_until_flush() {
    let rig = session_rig();
    let writer = Arc::new(
        SessionWriter::new(Arc::clone(&rig.store), None, None, Some(Arc::new(NeverFiringTimer)))
            .unwrap_or_else(|error| panic!("writer: {error:?}")),
    );
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["item1.jpg", "item2.jpg"],
        FixtureOptions {
            session_store: Some(Arc::clone(&rig.store)),
            session_writer: Some(Arc::clone(&writer)),
            ..FixtureOptions::default()
        },
    );
    let folder = with_session(&mut f);
    let second = f.path("item2.jpg");

    present(&mut f, 0);
    present(&mut f, 1);

    assert_eq!(saved_current_path(&rig, &folder).unwrap_or_default(), "");
    writer.flush();
    assert_eq!(saved_current_path(&rig, &folder), Some(second));
}

// New test (contract note 5): without a live session, or without a store, nothing is saved and the present still completes with its metric.
#[test]
fn present_without_a_session_or_a_store_saves_nothing_and_still_records_the_metric() {
    let rig = session_rig();
    let mut without_session = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["a.jpg"],
        FixtureOptions {
            session_store: Some(Arc::clone(&rig.store)),
            ..FixtureOptions::default()
        },
    );
    present(&mut without_session, 0);
    assert_eq!(without_session.metrics.snapshot().presented_images, 1);

    let mut without_store = fixture(&["b.jpg"]);
    let folder = with_session(&mut without_store);
    present(&mut without_store, 0);
    assert_eq!(without_store.metrics.snapshot().presented_images, 1);
    assert_eq!(
        without_store.session.as_ref().and_then(|s| s.current_path.clone()),
        Some(String::new()),
        "the live session is not touched without a store"
    );
    assert_eq!(saved_current_path(&rig, &folder), None);
}

// New test (contract note 5): a session with a blank folder makes the store refuse the save (C# `ArgumentException`): the generic failure branch reports it.
#[test]
fn present_session_with_a_blank_folder_is_reported_as_a_failure() {
    let rig = session_rig();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["item.jpg"],
        FixtureOptions {
            session_store: Some(Arc::clone(&rig.store)),
            ..FixtureOptions::default()
        },
    );
    f.session = Some(SessionState::default()); // blank folder

    present(&mut f, 0);

    assert!(f.presenter.status_needs_attention());
    assert!(!no_error_status(&f));
}

// C#: StatErrors.PresentAsync_PostPresentStatFails_StatusIsReadyAndSessionSaved(bool statThrows) (RV-A04)
fn post_present_stat_fails(stat_throws: bool) {
    let rig = session_rig();
    let fs = ScriptedFs::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["shown.png"],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            session_store: Some(Arc::clone(&rig.store)),
            ..FixtureOptions::default()
        },
    );
    let file = f.path("shown.png");
    let folder = with_session(&mut f);
    // The first stat (before the decode) is the real one; the refresh stat after the image is shown vanishes or throws.
    let real = real_stat(&file);
    fs.script(&file, move |call| {
        if call == 1 {
            Ok(Some(real))
        } else if stat_throws {
            Err(io::Error::other("share hiccup"))
        } else {
            Ok(None)
        }
    });

    present(&mut f, 0);

    let loading = status::loading(0, 1, file_len(&file), &f.format.ctx());
    let shown = statuses(&f);
    assert!(!shown.is_empty());
    assert_ne!(shown.last(), Some(&loading));
    assert!(shown.last().is_some_and(|status| status.contains("shown.png")));
    assert!(
        !f.presenter.status_needs_attention(),
        "the RV-A04 status is not attention"
    );
    assert_eq!(
        f.session.as_ref().and_then(|s| s.current_path.clone()),
        Some(file.clone())
    );
    assert_eq!(saved_current_path(&rig, &folder), Some(file.clone()));
    assert_eq!(&*f.catalog.paths(), &[file][..], "not removed under the user");
}

#[test]
fn present_post_present_stat_missing_status_is_ready_and_session_saved() {
    post_present_stat_fails(false);
}

#[test]
fn present_post_present_stat_throws_status_is_ready_and_session_saved() {
    post_present_stat_fails(true);
}

// C#: StatRefresh.Present_WhenOnlyTheLengthChangedWhileShowing_TheCatalogEntryTakesTheNewLength
// C#: StatRefresh.Present_WhenOnlyTheModifiedTimeChangedWhileShowing_TheCatalogEntryTakesTheNewTime
fn present_whose_refresh_stat_differs(
    refreshed: impl Fn(pr_core::abstractions::FileStat) -> pr_core::abstractions::FileStat + Send + Sync + 'static,
) -> (CatalogEntry, pr_core::abstractions::FileStat) {
    let fs = ScriptedFs::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["refresh.png"],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            ..FixtureOptions::default()
        },
    );
    let path = f.path("refresh.png");
    let real = real_stat(&path);
    // The first stat (before the decode) is the real one; the refresh stat after the image is shown reports a change.
    fs.script(&path, move |call| {
        Ok(Some(if call == 1 { real } else { refreshed(real) }))
    });

    present(&mut f, 0);

    assert!(fs.stat_count(&path) >= 2, "the post-present refresh stat did not run");
    let entry = f.catalog.find(&path).cloned().unwrap_or_else(|| panic!("entry"));
    (entry, real)
}

#[test]
fn present_when_only_the_length_changed_while_showing_the_catalog_entry_takes_the_new_length() {
    let (entry, real) = present_whose_refresh_stat_differs(|real| pr_core::abstractions::FileStat {
        length: real.length + 7,
        last_write_utc: real.last_write_utc,
    });

    assert_eq!(entry.length, Some(i64::try_from(real.length).unwrap_or(0) + 7));
}

#[test]
fn present_when_only_the_modified_time_changed_while_showing_the_catalog_entry_takes_the_new_time() {
    let shifted = DateTimeUtc::from_system_time(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .unwrap_or_else(|| panic!("a time"));
    let (entry, _) = present_whose_refresh_stat_differs(move |real| pr_core::abstractions::FileStat {
        length: real.length,
        last_write_utc: shifted,
    });

    assert_eq!(entry.last_write_utc, Some(shifted));
}

// ---- [self]: compare ---------------------------------------------------------------------------------------------------------

// C#: ImagePresenterTests.PresentAsync_WhenComparePairExists_ActivatesCompareViewModel
#[test]
fn present_when_compare_pair_exists_activates_compare_view_model() {
    let mut f = fixture(&["photo.jpg", "photo (1).jpg"]);
    let (f1, f2) = (f.path("photo.jpg"), f.path("photo (1).jpg"));
    f.settings.compare_size_enabled = true;
    f.settings.compare_hash_enabled = false;

    present(&mut f, 0);

    assert!(f.presenter.is_compare_visible());
    let compare = f.presenter.compare();
    assert!(compare.is_visible());
    assert_eq!(compare.left_path(), Some(f1.as_str()));
    assert_eq!(compare.right_path(), Some(f2.as_str()));
    assert_eq!(compare.selected_path(), Some(f1.as_str()));
    assert!(compare.left_image().is_some() && compare.right_image().is_some());
    // The status is the compare status with the ZERO-based index (the formatter prints it 1-based), the size suffixes and the "hash off" text.
    let ctx = f.format.ctx();
    let suffix = status::compare_size_suffix(4, &ctx);
    let off = status::compare_hash_text(None, &ctx);
    let expected = status::compare(
        &CompareStatus {
            index: 0,
            count: 2,
            left_name: "photo.jpg",
            left_size: &suffix,
            right_name: "photo (1).jpg",
            right_size: &suffix,
            hash_text: &off,
        },
        &ctx,
    );
    assert_eq!(f.presenter.status_text(), expected);
    assert!(!f.presenter.status_needs_attention());
    assert_eq!(compare.left_size_text(), suffix);
    assert_eq!(compare.hash_text(), off);
    assert!(
        f.presenter.current_image().is_none(),
        "two images: the main image is cleared"
    );
    assert!(f.presenter.current_photo_info().is_none());
    assert_eq!(f.metrics.snapshot().presented_images, 1);
}

// C#: ImagePresenterTests.PresentAsync_WhenCompareIsSuppressed_ClearsCompareAndKeepsMainImage
#[test]
fn present_when_compare_is_suppressed_clears_compare_and_keeps_main_image() {
    let mut f = fixture(&["photo.jpg", "photo (1).jpg"]);

    present(&mut f, 0);
    assert!(f.presenter.compare().is_visible());

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: false,
            path_override: None,
            include_capture_group_in_compare: false,
        },
    );

    assert!(!f.presenter.compare().is_visible());
    assert!(f.presenter.compare().left_path().is_none());
    assert!(f.presenter.current_image().is_some());
}

// C#: ImagePresenterTests.PresentAsync_CaptureGroup_CompareShowsBothMembersAndCanPresentAlternateMember
#[test]
fn present_capture_group_compare_shows_both_members_and_can_present_alternate_member() {
    let mut f = fixture(&["capture.jpg", "capture-member.jpg"]);
    let (jpeg, member) = (f.path("capture.jpg"), f.path("capture-member.jpg"));
    f.catalog
        .reset(vec![group_entry(&jpeg, &member)], RawPairMode::Separate, None);

    assert_eq!(f.presenter.has_compare_pair(&f.catalog, &jpeg), Ok(true));
    present(&mut f, 0);

    assert!(!f.presenter.is_compare_visible());

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: true,
            path_override: None,
            include_capture_group_in_compare: true,
        },
    );

    assert!(f.presenter.is_compare_visible());
    assert_eq!(f.presenter.compare().left_path(), Some(jpeg.as_str()));
    assert_eq!(f.presenter.compare().right_path(), Some(member.as_str()));

    present_with(&mut f, override_request(0, &member));

    assert!(!f.presenter.is_compare_visible());
    assert_eq!(f.presenter.current_presented_path(), Some(member.as_str()));
    assert_eq!(
        f.presenter.current_photo_info().map(|info| info.file_name.clone()),
        Some("capture-member.jpg".to_owned())
    );
    assert_eq!(f.catalog.count(), 1);
}

// New test (contract note 2): the compare view model selects the PRESENTED path, so the right member is selected when it is the override.
#[test]
fn present_compare_selects_the_presented_member() {
    let mut f = fixture(&["capture.jpg", "capture-member.jpg"]);
    let (jpeg, member) = (f.path("capture.jpg"), f.path("capture-member.jpg"));
    f.catalog
        .reset(vec![group_entry(&jpeg, &member)], RawPairMode::Separate, None);

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: true,
            path_override: Some(member.clone()),
            include_capture_group_in_compare: true,
        },
    );

    let compare = f.presenter.compare();
    assert!(compare.is_visible());
    assert_eq!(compare.selected_path(), Some(member.as_str()));
    assert!(compare.is_right_selected());
}

// New test (contract note 1): while compare is open EVERY presentation compares the capture pair, even without the flag (Next / Previous / undo).
#[test]
fn present_while_compare_is_open_compares_the_capture_pair_without_the_flag() {
    let mut f = fixture(&["capture.jpg", "capture-member.jpg"]);
    let (jpeg, member) = (f.path("capture.jpg"), f.path("capture-member.jpg"));
    f.catalog
        .reset(vec![group_entry(&jpeg, &member)], RawPairMode::Separate, None);
    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: true,
            path_override: None,
            include_capture_group_in_compare: true,
        },
    );
    assert!(f.presenter.is_compare_visible());

    present(&mut f, 0); // a plain present, the compare is open

    assert!(f.presenter.is_compare_visible());
    assert_eq!(f.presenter.compare().right_path(), Some(member.as_str()));
}

// New test (contract note 2): with the size and hash switches on, the sizes are formatted and the hashes compared (ordinal ignore case).
#[test]
fn present_compare_with_sizes_and_equal_hashes_shows_the_same_hash_text() {
    let hasher = ScriptHasher::new(|name| Ok(if name == "photo.jpg" { "ABC123" } else { "abc123" }.to_owned()));
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["photo.jpg", "photo (1).jpg"],
        FixtureOptions {
            hasher: Some(Arc::clone(&hasher) as _),
            ..FixtureOptions::default()
        },
    );
    f.settings.compare_size_enabled = true;
    f.settings.compare_hash_enabled = true;

    present(&mut f, 0);

    assert_eq!(hasher.calls(), 2);
    let ctx = f.format.ctx();
    assert_eq!(
        f.presenter.compare().hash_text(),
        status::compare_hash_text(Some(true), &ctx)
    );
    assert!(
        f.presenter
            .status_text()
            .contains(&status::compare_hash_text(Some(true), &ctx))
    );
}

#[test]
fn present_compare_with_different_hashes_shows_the_different_hash_text() {
    let hasher = ScriptHasher::new(|name| Ok(name.to_owned()));
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["photo.jpg", "photo (1).jpg"],
        FixtureOptions {
            hasher: Some(Arc::clone(&hasher) as _),
            ..FixtureOptions::default()
        },
    );
    f.settings.compare_hash_enabled = true;

    present(&mut f, 0);

    let ctx = f.format.ctx();
    assert_eq!(
        f.presenter.compare().hash_text(),
        status::compare_hash_text(Some(false), &ctx)
    );
}

// New test (contract note 2, T3): exactly one RAW member (a JPEG+RAW capture) never hashes: the hash text is "different" without reading any file.
#[test]
fn present_compare_with_exactly_one_raw_member_skips_both_hashes() {
    let hasher = ScriptHasher::new(|_| Ok("SAME".to_owned()));
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["pair.jpg", "pair.cr3"],
        FixtureOptions {
            hasher: Some(Arc::clone(&hasher) as _),
            ..FixtureOptions::default()
        },
    );
    let (jpeg, raw) = (f.path("pair.jpg"), f.path("pair.cr3"));
    f.catalog
        .reset(vec![group_entry(&jpeg, &raw)], RawPairMode::Separate, None);
    f.settings.compare_hash_enabled = true;

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: true,
            path_override: None,
            include_capture_group_in_compare: true,
        },
    );

    assert_eq!(hasher.calls(), 0, "no file is hashed for a JPEG+RAW pair");
    assert!(f.presenter.is_compare_visible());
    let ctx = f.format.ctx();
    assert_eq!(
        f.presenter.compare().hash_text(),
        status::compare_hash_text(Some(false), &ctx)
    );
}

// New test (contract note 2, T4): a hash that fails (not a cancellation) is logged and shown as "unknown"; the loaded previews stay.
#[test]
fn present_compare_hash_failure_is_unknown_and_keeps_the_previews() {
    let hasher = ScriptHasher::new(|name| {
        if name == "photo.jpg" {
            Err(FileHashError::Io(Arc::new(io::Error::other("unreadable"))))
        } else {
            Ok("H".to_owned())
        }
    });
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["photo.jpg", "photo (1).jpg"],
        FixtureOptions {
            hasher: Some(Arc::clone(&hasher) as _),
            ..FixtureOptions::default()
        },
    );
    f.settings.compare_hash_enabled = true;

    present(&mut f, 0);

    let ctx = f.format.ctx();
    assert!(f.presenter.compare().is_visible());
    assert!(f.presenter.compare().left_image().is_some());
    assert_eq!(f.presenter.compare().hash_text(), status::compare_hash_unknown(&ctx));
    assert!(!f.presenter.status_needs_attention());
}

// New test (contract note 2, T4): a CANCELLED hash is not "unknown": the whole present takes the failure path.
#[test]
fn present_compare_hash_canceled_is_a_failure_not_unknown() {
    let hasher = ScriptHasher::new(|_| Err(FileHashError::Canceled));
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["photo.jpg", "photo (1).jpg"],
        FixtureOptions {
            hasher: Some(Arc::clone(&hasher) as _),
            ..FixtureOptions::default()
        },
    );
    f.settings.compare_hash_enabled = true;

    present(&mut f, 0);

    let ctx = f.format.ctx();
    assert!(f.presenter.status_needs_attention());
    assert!(!no_error_status(&f));
    assert!(
        !f.presenter.compare().is_visible(),
        "the failure branch cleared the compare"
    );
    assert_ne!(f.presenter.compare().hash_text(), status::compare_hash_unknown(&ctx));
}

// New test (contract note 2, INV-1 after the previews): a compare superseded while its previews decode shows nothing and sets no status.
#[test]
fn present_compare_superseded_while_the_previews_decode_shows_nothing() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &["photo.jpg", "photo (1).jpg"],
        FixtureOptions::default(),
    );

    start(&mut f, 0);
    pump_one(&mut f); // stat
    decoder.release(); // the current image decodes
    // The preview event: the compare branch starts and both partner previews are requested (the left one is the RAM-cached current image, so the
    // second decode that enters the decoder is the right partner's).
    f.pump_until(|_| decoder.started() >= 2);
    let before = statuses(&f);
    f.clock.next_navigation();
    decoder.release(); // the right partner decodes
    f.pump_until_idle();

    assert_eq!(statuses(&f), before, "no compare status after the supersede");
    assert!(f.presenter.compare().right_image().is_none());
}

// C#: Adversarial.PresentAsync_ComparePartnerMissing_KeepsCurrentImageAndClearsCompare
#[test]
fn present_compare_partner_missing_keeps_current_image_and_clears_compare() {
    let mut f = fixture(&["photo.png", "photo (1).png"]); // the partner is listed in the catalog but gone from disk
    let (current, partner) = (f.path("photo.png"), f.path("photo (1).png"));
    std::fs::remove_file(&partner).unwrap_or_else(|error| panic!("delete: {error}"));

    present(&mut f, 0);

    assert!(f.catalog.paths().contains(&current));
    assert_eq!(f.catalog.count(), 2, "a failed partner never removes anything");
    assert!(f.presenter.current_image().is_some());
    assert!(!f.presenter.compare().is_visible());
    assert!(f.presenter.compare().left_path().is_none());
    assert!(f.presenter.status_text().contains("photo (1).png"));
    assert!(f.presenter.status_needs_attention());
    assert!(f.presenter.current_photo_info().is_some());
    assert_eq!(
        f.metrics.snapshot().presented_images,
        0,
        "step 7 does not run on a partner failure"
    );
}

// C#: Adversarial.PresentAsync_ComparePartnerMissing_StillArmsZoomDetailForCurrentImage
#[test]
fn present_compare_partner_missing_still_arms_zoom_detail_for_current_image() {
    // A downscaled preview (a real preview would be, for anything bigger than the decode box): the preview's own pixels cannot satisfy a 2x zoom, so the zoom
    // loader must have a target armed to decode the original.
    let downscaled = sized_image(100, 100, 2000, 2000, true);
    let decoder = GatedDecoder::new(Arc::clone(&downscaled));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["zoomed.png", "zoomed (1).png"]);
    let partner = f.path("zoomed (1).png");
    std::fs::remove_file(&partner).unwrap_or_else(|error| panic!("delete: {error}"));

    decoder.release(); // the current image's own decode
    present(&mut f, 0);
    // Sanity: this really is the compare-partner-failure path.
    assert!(!f.presenter.compare().is_visible());
    assert!(f.presenter.status_text().contains("zoomed (1).png"));

    f.presenter.set_viewer_zoom(Some(2.0)); // zoom in well past what the 100px preview can show

    // Without the fix, `on_preview_presented` was never called on this path, so no target was armed and this zoom was a silent no-op.
    assert!(f.presenter.zoom_detail().pending_load());
    decoder.release(); // the full-resolution decode
    f.pump_until(|f| f.presenter.zoom_detail().held_original().is_some());
    let held = f
        .presenter
        .zoom_detail()
        .held_original()
        .cloned()
        .unwrap_or_else(|| panic!("held"));
    assert!(same_platform_image(held.platform_image(), downscaled.platform_image()));
}

// New test (contract note 2, O4): the LEFT error wins when both partners fail. The presented-hook clears the RAM cache, so the current image (the left
// partner) has to be decoded again for the compare; its second decode and the right partner's first both fail, with different errors.
#[test]
fn present_compare_left_error_wins_when_both_partners_fail() {
    let decoder = NamedScriptDecoder::new(image(100, 100), |name, call| match (name, call) {
        ("left-wins.png", 2) => Err(DecodeError::InvalidData {
            message: "left-error".to_owned(),
        }),
        ("left-wins-member.png", 1) => Err(DecodeError::InvalidData {
            message: "right-error".to_owned(),
        }),
        _ => Ok(()),
    });
    let rig = service_rig(Arc::clone(&decoder) as _);
    let service = Arc::clone(&rig.service);
    let mut f = fixture_with_service(
        &rig,
        &["left-wins.png", "left-wins-member.png"],
        FixtureOptions {
            on_presented_hook: Some(Box::new(move |_path: &str| service.clear_cache())),
            ..FixtureOptions::default()
        },
    );
    let (left, right) = (f.path("left-wins.png"), f.path("left-wins-member.png"));
    f.catalog
        .reset(vec![group_entry(&left, &right)], RawPairMode::Separate, None);

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: true,
            path_override: None,
            include_capture_group_in_compare: true,
        },
    );

    assert!(
        !f.presenter.compare().is_visible(),
        "the compare was cleared by the partner failure"
    );
    assert!(
        f.presenter.current_image().is_some(),
        "the healthy current image is shown alone"
    );
    assert!(f.presenter.status_needs_attention());
    let status = f.presenter.status_text();
    assert!(status.contains("left-error"), "{status}");
    assert!(!status.contains("right-error"), "{status}");
    assert!(
        status.contains("left-wins-member.png"),
        "the status names the partner: {status}"
    );
}

// ---- [self]: compare busy retry and the dimension lookup ----------------------------------------------------------------------

/// C# `CreateComparePresenter`: a capture group of a JPEG and a PNG member whose decode is busy for the first `busy_count` calls.
fn busy_rig(
    busy_count: usize,
) -> (
    PresenterFixture,
    Arc<MemberScriptDecoder>,
    Arc<RecordingSpawner>,
    Arc<AtomicUsize>,
    String,
    String,
    ServiceRig,
) {
    let decoder = MemberScriptDecoder::new("busy-pair-member.png", image(100, 100), move |call| {
        if call <= busy_count {
            Err(DecodeError::busy())
        } else {
            Ok(())
        }
    });
    let rig = service_rig(Arc::clone(&decoder) as _);
    let delays = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&delays);
    let mut f = fixture_with_service(
        &rig,
        &["busy-pair.jpg", "busy-pair-member.png"],
        FixtureOptions {
            busy_retry_delay: Some(Arc::new(move |_delay, _cancel| {
                counter.fetch_add(1, Ordering::SeqCst);
                true
            })),
            ..FixtureOptions::default()
        },
    );
    let (jpeg, partner) = (f.path("busy-pair.jpg"), f.path("busy-pair-member.png"));
    f.catalog
        .reset(vec![group_entry(&jpeg, &partner)], RawPairMode::Separate, None);
    let spawner = Arc::clone(&rig.spawner);
    (f, decoder, spawner, delays, jpeg, partner, rig)
}

// C#: CompareBusyAndDimensions.PresentAsync_ComparePartnerBusyOnce_RetriesAtViewerPriorityAndShowsThePartner
#[test]
fn present_compare_partner_busy_once_retries_at_viewer_priority_and_shows_the_partner() {
    let (mut f, decoder, spawner, delays, jpeg, partner, _rig) = busy_rig(1);

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: true,
            path_override: None,
            include_capture_group_in_compare: true,
        },
    );

    let compare = f.presenter.compare();
    assert!(compare.is_visible());
    assert_eq!(compare.left_path(), Some(jpeg.as_str()));
    assert_eq!(compare.right_path(), Some(partner.as_str()));
    assert!(compare.right_image().is_some());
    assert!(!f.presenter.status_needs_attention());
    assert_eq!(delays.load(Ordering::SeqCst), 1);
    assert_eq!(decoder.member_calls(), 2);
    // The viewer lane, not the preload lane: no decode thread of the preload kind was ever started.
    assert_eq!(spawner.count(DecodeThreadKind::Preview), 0, "{:?}", spawner.kinds());
    assert!(spawner.count(DecodeThreadKind::Viewer) >= 3, "{:?}", spawner.kinds());
}

// C#: CompareBusyAndDimensions.PresentAsync_ComparePartnerBusyTwiceInARow_ShowsTheErrorAfterOneBoundedRetry
#[test]
fn present_compare_partner_busy_twice_in_a_row_shows_the_error_after_one_bounded_retry() {
    let (mut f, decoder, _spawner, delays, jpeg, partner, _rig) = busy_rig(2);

    present_with(
        &mut f,
        PresentRequest {
            index: 0,
            allow_compare: true,
            path_override: None,
            include_capture_group_in_compare: true,
        },
    );

    assert!(!f.presenter.compare().is_visible());
    assert!(f.presenter.current_image().is_some(), "the healthy current image stays");
    assert!(f.presenter.status_needs_attention());
    assert!(f.presenter.status_text().contains("busy-pair-member.png"));
    assert_eq!(delays.load(Ordering::SeqCst), 1);
    assert_eq!(decoder.member_calls(), 2);
    assert_eq!(f.presenter.current_presented_path(), Some(jpeg.as_str()));
    let _ = partner;
}

/// A fixture whose service reads the header through `decoder` and whose presented hook clears the cache (so the status lookup must `ReadInfo`).
fn dimension_rig(decoder: Arc<ThrowingInfoDecoder>, name: &str) -> (PresenterFixture, String) {
    let rig = service_rig(decoder as _);
    let service = Arc::clone(&rig.service);
    let mut f = fixture_with_service(
        &rig,
        &[name],
        FixtureOptions {
            // The dimensions the decode seeded are evicted right after the image is shown, so the status lookup must ReadInfo.
            on_presented_hook: Some(Box::new(move |_path: &str| service.clear_cache())),
            ..FixtureOptions::default()
        },
    );
    let path = f.path(name);
    std::mem::forget(rig); // the scratch folder stays for the process (a few empty directories)
    f.settings.loading_mode = Some(LoadingMode::Preview);
    (f, path)
}

// C#: CompareBusyAndDimensions.PresentAsync_DimensionLookupThrowsInvalidData_KeepsTheImageAndShowsTheStatusWithoutDimensions
#[test]
fn present_dimension_lookup_throws_invalid_data_keeps_the_image_and_shows_the_status_without_dimensions() {
    let decoder = ThrowingInfoDecoder::new(image(100, 100), || DecodeError::InvalidData {
        message: "no sensor size and no preview size".to_owned(),
    });
    let (mut f, path) = dimension_rig(decoder, "unknown-size.png");

    present(&mut f, 0);

    assert!(f.presenter.current_image().is_some());
    assert!(!f.presenter.status_needs_attention());
    let expected = status::ready(0, 1, file_len(&path), "unknown-size.png", &f.format.ctx());
    assert_eq!(f.presenter.status_text(), expected); // no "WxH"
    assert!(f.presenter.current_photo_info().is_some());
}

// C#: CompareBusyAndDimensions.PresentAsync_DimensionLookupThrowsUnexpectedBug_IsNotHiddenAsReadyStatus
#[test]
fn present_dimension_lookup_throws_unexpected_bug_is_not_hidden_as_ready_status() {
    let decoder = ThrowingInfoDecoder::new(image(100, 100), || DecodeError::InvalidArgument {
        message: "decoder bug".to_owned(),
    });
    let (mut f, path) = dimension_rig(decoder, "buggy-size.png");

    present(&mut f, 0);

    let ready = status::ready(0, 1, file_len(&path), "buggy-size.png", &f.format.ctx());
    assert_ne!(f.presenter.status_text(), ready);
    assert!(f.presenter.status_needs_attention());
}

// C#: MutationGaps.PresentAsync_DimensionLookupFailsWithAnExpectedHeaderReadFailure_KeepsTheImageWithoutDimensions(string)
fn expected_dimension_failure(make: impl Fn() -> DecodeError + Send + Sync + 'static, label: &str) {
    let decoder = ThrowingInfoDecoder::new(image(100, 100), make);
    let (mut f, path) = dimension_rig(decoder, "dims.png");

    present(&mut f, 0);

    assert!(f.presenter.current_image().is_some(), "{label}");
    assert!(!f.presenter.status_needs_attention(), "{label}");
    let expected = status::ready(0, 1, file_len(&path), "dims.png", &f.format.ctx());
    assert_eq!(f.presenter.status_text(), expected, "{label}");
}

#[test]
fn present_dimension_lookup_fails_with_an_expected_header_read_failure_io() {
    expected_dimension_failure(|| DecodeError::Io(io::Error::other("io")), "io");
}

#[test]
fn present_dimension_lookup_fails_with_an_expected_header_read_failure_permission_denied() {
    expected_dimension_failure(
        || DecodeError::Io(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        "permission denied",
    );
}

#[test]
fn present_dimension_lookup_fails_with_an_expected_header_read_failure_not_supported() {
    expected_dimension_failure(
        || DecodeError::NotSupported {
            message: "nope".to_owned(),
            user_text: None,
            inner_hresult: None,
        },
        "not supported",
    );
}

#[test]
fn present_dimension_lookup_fails_with_an_expected_header_read_failure_format() {
    expected_dimension_failure(
        || DecodeError::Format {
            message: "format".to_owned(),
        },
        "format",
    );
}

#[test]
fn present_dimension_lookup_fails_with_an_expected_header_read_failure_invalid_operation() {
    expected_dimension_failure(
        || DecodeError::InvalidOperation {
            message: "invalid".to_owned(),
        },
        "invalid operation",
    );
}

#[test]
fn present_dimension_lookup_fails_with_an_expected_header_read_failure_invalid_data() {
    expected_dimension_failure(
        || DecodeError::InvalidData {
            message: "data".to_owned(),
        },
        "invalid data",
    );
}

#[test]
fn present_dimension_lookup_fails_with_an_expected_header_read_failure_com() {
    expected_dimension_failure(
        || DecodeError::Com {
            hresult: -2_147_467_259,
            message: "external".to_owned(),
        },
        "com",
    );
}

#[test]
fn present_dimension_lookup_fails_with_an_expected_header_read_failure_busy() {
    expected_dimension_failure(DecodeError::busy, "busy");
}

// ---- [self]: failures ----------------------------------------------------------------------------------------------------------

// C#: ImagePresenterTests.PresentAsync_WhenSourceChangesDuringDecode_ShowsLocalizedImageError
#[test]
fn present_when_source_changes_during_decode_shows_localized_image_error() {
    let decoder = RewritingDecoder::new();
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["racing.png"]);
    f.format = FormatFixture::vietnamese();
    let path = f.path("racing.png");
    decoder.set_path(&path);
    // Another program rewrites the file while the decoder is running: the key taken before the decode never matches afterwards.
    f.catalog.reset(vec![scanned_entry(&path)], RawPairMode::Separate, None);

    present(&mut f, 0);

    assert_eq!(decoder.decode_calls(), 1);
    let ctx = f.format.ctx();
    let sentence = ctx.texts.format_with(
        "err.io.sourceChangedDuringDecode",
        &[LocArg::new("path", path.as_str())],
        ctx.numbers,
    );
    let expected = status::image_error("racing.png", &sentence, &ctx);
    assert_eq!(f.presenter.status_text(), expected);
    assert!(f.presenter.status_needs_attention());
    assert!(!f.presenter.status_text().contains("changed during decode"));
    assert!(!f.presenter.status_text().contains("changed while"));
}

// C#: ImagePresenterTests.PresentAsync_CurrentEntryLeavesCatalogWhileDecoding_NeverPreloadsAroundMinusOne
#[test]
fn present_current_entry_leaves_catalog_while_decoding_never_preloads_around_minus_one() {
    let decoder = GatedDecoder::new(image(100, 100));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["vanishes-during-decode.jpg"]);

    start(&mut f, 0);
    pump_one(&mut f); // the stat came back: the loading status was set, the decode is requested
    f.catalog.reset(Vec::new(), RawPairMode::Separate, None); // the catalog is emptied between the status event and the pump
    decoder.release();
    f.pump_until_idle();

    assert!(!f.preload.preload_around_calls().contains(&-1));
}

// C#: DecodeFailure.PresentAsync_WhenDifferentEntryFailsToDecode_ClearsPreviousImage
#[test]
fn present_when_different_entry_fails_to_decode_clears_previous_image() {
    let decoder = MemberScriptDecoder::new("bad.png", image(100, 100), |_| {
        Err(DecodeError::InvalidData {
            message: "not an image".to_owned(),
        })
    });
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["good.png", "bad.png"]);
    let (good, bad) = (f.path("good.png"), f.path("bad.png"));

    present(&mut f, 0);
    assert!(f.presenter.current_image().is_some());

    present(&mut f, 1);

    assert!(f.presenter.current_image().is_none());
    assert!(lock(&f.sink_log).images.last().is_some_and(|call| call.image.is_none()));
    assert_eq!(&*f.catalog.paths(), &[good, bad][..]);
    assert!(f.presenter.status_text().contains("bad.png"));
    assert!(f.presenter.status_needs_attention());
}

// New test (contract note 6, T1): a decode failure that arrives AFTER the navigation was superseded is logged only: no status, no image, no catalog change.
#[test]
fn present_superseded_decode_failure_changes_nothing() {
    let decoder = FaultingGatedDecoder::new("marker-stale-failure");
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["only.png"]);
    let path = f.path("only.png");
    // Fast mode: no thumbnail event, so the failed PREVIEW event is the one that reaches the superseded navigation.
    f.settings.loading_mode = Some(LoadingMode::Fast);

    start(&mut f, 0);
    pump_one(&mut f);
    let before = statuses(&f);
    f.clock.next_navigation(); // the user moved on
    decoder.release_and_throw();
    assert!(decoder.wait_threw());
    pump_one(&mut f); // the failed preview's event
    f.pump_until_idle();

    assert_eq!(statuses(&f), before);
    assert_eq!(images_shown(&f), 0);
    assert_eq!(&*f.catalog.paths(), &[path][..]);
}

// New test (contract note 6 (2)): a stale FileNotFound failure of a SUPERSEDED navigation requests no recheck stat (the file system is not touched for it).
#[test]
fn present_superseded_not_found_failure_requests_no_recheck_stat() {
    let decoder = FaultingGatedDecoder::with_failure("stale-not-found", |_| DecodeError::FileNotFound {
        message: "stale".to_owned(),
    });
    let fs = ScriptedFs::new();
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &["only.png"],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            ..FixtureOptions::default()
        },
    );
    f.settings.loading_mode = Some(LoadingMode::Fast);
    let path = f.path("only.png");

    start(&mut f, 0);
    pump_one(&mut f);
    f.clock.next_navigation();
    decoder.release_and_throw();
    assert!(decoder.wait_threw());
    pump_one(&mut f); // the failed preview's event
    f.pump_until_idle();

    assert_eq!(fs.stat_count(&path), 1, "only the initial stat");
    assert_eq!(&*f.catalog.paths(), &[path][..]);
}

// New test (contract note 6 (3)): a decoder cancellation while the navigation is still current is an ordinary failure (not a silent supersede).
#[test]
fn present_cancellation_while_current_is_reported_as_a_failure() {
    let decoder = MemberScriptDecoder::new("cancelled.png", image(100, 100), |_| Err(DecodeError::Canceled));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["cancelled.png"]);

    present(&mut f, 0);

    assert!(f.presenter.status_needs_attention());
    assert!(!no_error_status(&f));
}

// C#: MutationGaps.PresentAsync_StaleNotFoundKeepsFailingWhileTheFileExists_RetriesExactlyThreeTimesThenReportsTheError
#[test]
fn present_stale_not_found_keeps_failing_while_the_file_exists_retries_exactly_three_times_then_reports_the_error() {
    let decoder = MemberScriptDecoder::new("restored.png", image(100, 100), |_| {
        Err(DecodeError::FileNotFound {
            message: "stale".to_owned(),
        })
    });
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["first.png", "restored.png"]);
    let (first, restored) = (f.path("first.png"), f.path("restored.png"));

    present(&mut f, 1);

    assert_eq!(
        decoder.member_calls(),
        4,
        "the first decode plus MaxStaleNotFoundRetries (3) re-presents"
    );
    assert!(f.presenter.status_needs_attention());
    assert_eq!(&*f.catalog.paths(), &[first, restored][..]);
}

// New test (contract note 6 (2)): the persisted stale-file failure is reported like an unreadable file: the photo stays in the catalog, the previous image and
// compare pair are cleared (Delete / Move must not act on an unseen file), the status has attention.
#[test]
fn present_stale_not_found_persisted_failure_clears_the_previous_image_and_keeps_the_entry() {
    let decoder = MemberScriptDecoder::new("restored.png", image(100, 100), |_| {
        Err(DecodeError::FileNotFound {
            message: "stale".to_owned(),
        })
    });
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["first.png", "restored.png"]);
    let (first, restored) = (f.path("first.png"), f.path("restored.png"));
    present(&mut f, 0);
    assert!(f.presenter.current_image().is_some());

    present(&mut f, 1);

    assert!(
        f.presenter.current_image().is_none(),
        "the previous photo must not stay under the error status"
    );
    assert!(lock(&f.sink_log).images.last().is_some_and(|call| call.image.is_none()));
    assert!(f.presenter.current_photo_info().is_none());
    assert!(f.presenter.status_needs_attention());
    assert!(f.presenter.status_text().contains("restored.png"));
    assert_eq!(&*f.catalog.paths(), &[first, restored][..]);
}

// C#: StaleInflightFailure.PresentAsync_StaleNotFoundKeepsFailingWhileTheFileExists_KeepsTheEntry
#[test]
fn present_stale_not_found_keeps_failing_while_the_file_exists_keeps_the_entry() {
    let decoder = MemberScriptDecoder::new("restored.png", image(100, 100), |_| {
        Err(DecodeError::FileNotFound {
            message: "stale".to_owned(),
        })
    });
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["first.png", "restored.png"]);
    let (first, restored) = (f.path("first.png"), f.path("restored.png"));

    present(&mut f, 1);

    assert!(Path::new(&restored).exists());
    assert_eq!(&*f.catalog.paths(), &[first, restored][..]);
    assert!(decoder.member_calls() >= 2, "it was retried, not given up on at once");
}

// C#: StaleInflightFailure.PresentAsync_StaleNotFoundRetryThenGenericFailure_KeepsThePresentedPathOnTheMemberOnScreen
#[test]
fn present_stale_not_found_retry_then_generic_failure_keeps_the_presented_path_on_the_member_on_screen() {
    let decoder = MemberScriptDecoder::new("pair-shown-member.png", image(100, 100), |call| {
        if call == 1 {
            Err(DecodeError::FileNotFound {
                message: "stale".to_owned(),
            })
        } else {
            Err(DecodeError::InvalidOperation {
                message: "broken member".to_owned(),
            })
        }
    });
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["pair-shown.jpg", "pair-shown-member.png"]);
    let (jpeg, member) = (f.path("pair-shown.jpg"), f.path("pair-shown-member.png"));
    f.catalog
        .reset(vec![group_entry(&jpeg, &member)], RawPairMode::Separate, None);
    present(&mut f, 0);
    assert_eq!(f.presenter.current_presented_path(), Some(jpeg.as_str()));

    present_with(&mut f, override_request(0, &member));

    assert_eq!(decoder.member_calls(), 2);
    assert_eq!(
        f.presenter.current_presented_path(),
        Some(jpeg.as_str()),
        "the JPEG is still what is displayed"
    );
    assert!(
        f.presenter.current_image().is_some(),
        "a failed member switch keeps the image of the same entry"
    );
    assert!(f.presenter.status_needs_attention());
}

// C#: StaleInflightFailure.PresentAsync_JoinedInflightDecodeFailedWhileFileWasAway_KeepsRestoredPhotoAndPresentsIt (OC14)
#[test]
fn present_joined_inflight_decode_failed_while_file_was_away_keeps_restored_photo_and_presents_it() {
    let reader = HoldFirstFailureDecoder::new("moved.png", image(100, 100));
    let rig = service_rig(Arc::clone(&reader) as _);
    let mut f = fixture_with_service(&rig, &["first.png", "moved.png"], FixtureOptions::default());
    let (first, moved) = (f.path("first.png"), f.path("moved.png"));
    let away = f.path("away.png");

    // A preload / earlier present of 'moved' is in flight when the user Moves it away: its open fails, but the failure is only reported later (it stays the
    // shared in-flight decode for that key until then).
    let key = rig
        .service
        .get_current_cache_key(&moved)
        .unwrap_or_else(|error| panic!("key: {error}"));
    std::fs::rename(&moved, &away).unwrap_or_else(|error| panic!("move away: {error}"));
    let inflight = rig.service.get_preview(&moved, &key);
    assert!(reader.wait_failed(), "the in-flight decode's failed open");

    // Undo: the file is back (a Move keeps length and mtime, so the cache key is identical) and is presented.
    std::fs::rename(&away, &moved).unwrap_or_else(|error| panic!("move back: {error}"));
    start(&mut f, 1);
    pump_one(&mut f); // the stat: the viewer request joins the in-flight decode
    assert!(
        rig.metrics.snapshot().inflight_joins >= 1,
        "the presentation joined the in-flight decode"
    );

    reader.release();
    assert!(inflight.wait().is_err(), "the shared decode failed with FileNotFound");
    f.pump_until_idle();

    assert!(Path::new(&moved).exists());
    assert_eq!(&*f.catalog.paths(), &[first, moved][..]);
    assert_eq!(f.catalog.current_index(), 1);
    assert!(f.presenter.current_image().is_some());
}

// ---- [self]: AR16 and the other index-shifting cases -------------------------------------------------------------------------------

// C#: MutationGaps.PresentAsync_EarlierEntryRemovedWhileDecoding_PreloadAndStatusUseTheShiftedIndex (AR16)
// The catalog is owned by the test thread, so the earlier entry is removed between the preview event and the pump instead of inside the decoder.
#[test]
fn present_earlier_entry_removed_while_decoding_preload_and_status_use_the_shifted_index() {
    let decoder = GatedDecoder::new(image(1, 1));
    let mut f = PresenterFixture::new(Arc::clone(&decoder) as _, &["earlier.png", "shown.png"]);
    let (earlier, shown) = (f.path("earlier.png"), f.path("shown.png"));

    start(&mut f, 1);
    pump_one(&mut f); // the stat; the decode is requested
    f.catalog.remove(&earlier);
    decoder.release();
    f.pump_until_idle();

    assert_eq!(&*f.catalog.paths(), &[shown.clone()][..]);
    assert_eq!(f.preload.preload_around_calls(), vec![0]);
    let expected = status::with_dimensions(0, 1, file_len(&shown), 1, 1, "shown.png", &f.format.ctx());
    assert_eq!(f.presenter.status_text(), expected);
}

// C#: Adversarial.PresentAsync_SingleImage_IndexPastEnd_KeepsShownImage
#[test]
fn present_single_image_index_past_end_keeps_shown_image() {
    let mut f = fixture(&["only.png"]);
    present(&mut f, 0);
    let shown = f
        .presenter
        .current_image()
        .cloned()
        .unwrap_or_else(|| panic!("an image is shown"));
    let navigation = f.clock.current_navigation();

    present(&mut f, 1);
    present(&mut f, -1);

    assert!(
        f.presenter
            .current_image()
            .is_some_and(|current| same_platform_image(current, &shown))
    );
    assert_eq!(f.clock.current_navigation(), navigation);
    assert_eq!(f.catalog.current_index(), 0);
}

// C#: Adversarial.PresentAsync_RepeatedOnSameIndex_StaysConsistent
#[test]
fn present_repeated_on_same_index_stays_consistent() {
    let mut f = fixture(&["a.png", "b.png"]);
    let second = f.path("b.png");

    for _ in 0..25 {
        present(&mut f, 1);
    }

    assert_eq!(f.catalog.current_index(), 1);
    assert_eq!(f.catalog.count(), 2);
    assert!(f.presenter.current_image().is_some());
    assert_eq!(presented_paths(&f), vec![second; 25]);
    assert!(no_error_status(&f));
}

// ---- un-ignored in this unit: W8-07h / W8-07i tests that needed the tail of a present -----------------------------------------------

// (they live in presenter_state.rs and presenter_present_a.rs; this module only carries the 07j-specific ones)

#[test]
fn the_changes_of_a_completed_present_name_what_moved() {
    // New test: the shell repaints from `PresenterChanges` (D-W8-1): a completed single-image present reports the image, status, path and photo info.
    let mut f = fixture(&["a.png"]);
    start(&mut f, 0);
    let changes = f.pump_until_idle();
    for bit in [
        PresenterChanges::CURRENT_IMAGE,
        PresenterChanges::STATUS,
        PresenterChanges::PHOTO_INFO,
    ] {
        assert!(changes.contains(bit), "{bit:?} missing from {changes:?}");
    }
}

// ---- [self]: INV-1 after every wait (T1) ------------------------------------------------------------------------------------------

// New test (contract note 4, T1): a navigation superseded while the original-dimension lookup runs on its worker does no bookkeeping afterwards: no refresh stat
// is even requested.
#[test]
fn present_superseded_during_the_dimension_lookup_does_no_bookkeeping() {
    let decoder = GatedInfoDecoder::new(image(100, 100));
    let rig = service_rig(Arc::clone(&decoder) as _);
    let service = Arc::clone(&rig.service);
    let fs = ScriptedFs::new();
    let mut f = fixture_with_service(
        &rig,
        &["dims.png"],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            // The dimensions the decode seeded are evicted right after the image is shown, so the status lookup must ReadInfo.
            on_presented_hook: Some(Box::new(move |_path: &str| service.clear_cache())),
            ..FixtureOptions::default()
        },
    );
    let path = f.path("dims.png");

    start(&mut f, 0);
    f.pump_until(|_| decoder.info_started()); // the image is shown; the lookup runs on its worker
    let before = statuses(&f);
    f.clock.next_navigation();
    decoder.release_info();
    f.pump_until_idle();

    assert_eq!(statuses(&f), before, "no status after the supersede");
    assert_eq!(f.metrics.snapshot().presented_images, 0);
    assert_eq!(
        fs.stat_count(&path),
        1,
        "only the initial stat: no refresh stat for a superseded present"
    );
}

// New test (contract note 4, T1): a navigation superseded while the refresh stat runs does no bookkeeping afterwards (no status, no catalog update, no session).
#[test]
fn present_superseded_during_the_refresh_stat_does_no_bookkeeping() {
    let rig = session_rig();
    let fs = ScriptedFs::new();
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["refresh.png"],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            session_store: Some(Arc::clone(&rig.store)),
            ..FixtureOptions::default()
        },
    );
    let path = f.path("refresh.png");
    let folder = with_session(&mut f);
    let real = real_stat(&path);
    let refresh_seen = Arc::new(AtomicBool::new(false));
    let proceed = support::preview_fakes::Gate::new();
    {
        let (seen, proceed) = (Arc::clone(&refresh_seen), Arc::clone(&proceed));
        fs.script(&path, move |call| {
            if call >= 2 {
                seen.store(true, Ordering::SeqCst);
                proceed.wait();
            }
            Ok(Some(real))
        });
    }

    start(&mut f, 0);
    f.pump_until(|_| refresh_seen.load(Ordering::SeqCst)); // the image is shown; the refresh stat is held on the stat worker
    let before = statuses(&f);
    f.clock.next_navigation();
    proceed.open();
    f.pump_until_idle();

    assert_eq!(statuses(&f), before, "no status after the supersede");
    assert_eq!(f.metrics.snapshot().presented_images, 0);
    assert_eq!(saved_current_path(&rig, &folder), None, "no session save");
    assert_eq!(
        f.session.as_ref().and_then(|s| s.current_path.clone()),
        Some(String::new())
    );
}

/// A compare fixture (two numbered files) whose worker jobs named in `hold` wait for the test.
fn held_compare_fixture(
    hold: &[&'static str],
    hasher: Option<Arc<dyn FileHasher>>,
) -> (PresenterFixture, Arc<HoldRunner>) {
    let runner = HoldRunner::new(hold);
    let mut f = PresenterFixture::with_options(
        SizedDecoder::new(),
        &["photo.jpg", "photo (1).jpg"],
        FixtureOptions {
            runner: Some(Arc::clone(&runner) as _),
            hasher,
            ..FixtureOptions::default()
        },
    );
    f.settings.compare_size_enabled = true;
    f.settings.compare_hash_enabled = true;
    (f, runner)
}

// New test (contract note 2, T1): a compare superseded while its sizes are computed shows no size text and no compare status.
#[test]
fn present_compare_superseded_while_the_sizes_are_computed_shows_nothing() {
    let (mut f, runner) = held_compare_fixture(&["PhotoReviewRs.CompareSizes"], None);

    start(&mut f, 0);
    f.pump_until(|_| runner.held_count("PhotoReviewRs.CompareSizes") == 1); // both previews are in: the sizes are requested
    let before = statuses(&f);
    f.clock.next_navigation();
    assert_eq!(runner.release("PhotoReviewRs.CompareSizes"), 1);
    f.pump_until_idle();

    assert_eq!(statuses(&f), before);
    assert_eq!(f.presenter.compare().left_size_text(), "");
    assert_eq!(f.metrics.snapshot().presented_images, 0);
}

// New test (contract note 2, T1): a compare superseded while its hashes are computed shows no hash text and no compare status.
#[test]
fn present_compare_superseded_while_the_hashes_are_computed_shows_nothing() {
    let hasher = ScriptHasher::new(|_| Ok("SAME".to_owned()));
    let (mut f, runner) = held_compare_fixture(&["PhotoReviewRs.CompareHash"], Some(Arc::clone(&hasher) as _));

    start(&mut f, 0);
    f.pump_until(|_| runner.held_count("PhotoReviewRs.CompareHash") == 2); // sizes are in: both hashes are requested
    let before = statuses(&f);
    f.clock.next_navigation();
    assert_eq!(runner.release("PhotoReviewRs.CompareHash"), 2);
    f.pump_until_idle();

    assert_eq!(statuses(&f), before);
    let ctx = f.format.ctx();
    assert_eq!(
        f.presenter.compare().hash_text(),
        status::compare_hash_text(None, &ctx),
        "the hash text was not applied"
    );
    assert_eq!(f.metrics.snapshot().presented_images, 0);
}

// New test (contract note 6 (2), T6): a recheck stat that is superseded ends quietly: the file is neither re-presented nor removed.
#[test]
fn present_superseded_during_the_stale_not_found_recheck_changes_nothing() {
    let fs = ScriptedFs::new();
    let decoder = MemberScriptDecoder::new("restored.png", image(100, 100), |_| {
        Err(DecodeError::FileNotFound {
            message: "stale".to_owned(),
        })
    });
    let mut f = PresenterFixture::with_options(
        Arc::clone(&decoder) as _,
        &["restored.png"],
        FixtureOptions {
            file_system: Some(Arc::clone(&fs) as _),
            ..FixtureOptions::default()
        },
    );
    let path = f.path("restored.png");
    let recheck_seen = Arc::new(AtomicBool::new(false));
    let proceed = support::preview_fakes::Gate::new();
    {
        let (seen, proceed) = (Arc::clone(&recheck_seen), Arc::clone(&proceed));
        let real = real_stat(&path);
        fs.script(&path, move |call| {
            if call >= 2 {
                seen.store(true, Ordering::SeqCst);
                proceed.wait();
            }
            Ok(Some(real))
        });
    }

    start(&mut f, 0);
    f.pump_until(|_| recheck_seen.load(Ordering::SeqCst)); // the decode failed with FileNotFound: the recheck stat is held
    f.clock.next_navigation();
    proceed.open();
    f.pump_until_idle();

    assert_eq!(decoder.member_calls(), 1, "no re-present");
    assert_eq!(&*f.catalog.paths(), &[path][..], "nothing removed");
}
