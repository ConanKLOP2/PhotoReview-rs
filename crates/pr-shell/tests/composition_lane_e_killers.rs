//! Lane E mutation killers for `pr_shell::composition::{contracts, graph, main_view_model, registry}` (cargo-mutants survivors of the whole-directory run).
//! New tests, no C# twin. Public API only; the registry fixture of `composition_registry.rs` is reused. Nothing here opens a window.
#[allow(dead_code)]
#[path = "common/composition_fixture.rs"]
mod composition_fixture;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use pr_app::coordinators::presenter::ImageChange;
use pr_app::coordinators::sink_callbacks::{MainViewModelSinkCallbacks, PresentationSinkFactory};
use pr_app::coordinators::traits::IPresentationSink;
use pr_app::coordinators::zoom_detail::ThreadRunner;
use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_app::services::contracts::{ClipboardService, PresentationObserver};
use pr_core::abstractions::{BatchReviewItem, DialogService, SkippedEntry};
use pr_core::diagnostics::file_log::{DEFAULT_MAX_LOG_BYTES, FileLog};
use pr_core::file_actions::journal::JournalCompactionOutcome;
use pr_core::file_actions::reconcile::ReconcileJournal;
use pr_core::model::JournalDurability;
use pr_core::model::enums::{DecoderBackend, RawFullDecode};
use pr_imaging::decode_types::PlatformImage;
use pr_l10n::localizer::{Localizer, built_in_english_localizer, create_show_keys};
use pr_shell::composition::contracts::SharedServiceOptions;
use pr_shell::composition::graph::GraphOverrides;
use pr_shell::composition::main_view_model::{CatalogFeed, FeedPreloadController, MainViewModelHost};
use pr_test_support::LogLevel;
use pr_test_support::TempRoot;

// ---- Debug impls ------------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn debug_of_shared_service_options_names_the_type_and_its_flags() {
    let options = SharedServiceOptions {
        codec: pr_imaging::pixel_memory::PixelBufferImageCodec::shared(),
        wpf_fallback_decoder: None,
        thumbnail_decoder: Arc::new(|| Arc::new(composition_fixture::FixtureDecoder)),
    };
    let text = format!("{options:?}");
    assert!(text.contains("SharedServiceOptions"), "{text}");
    assert!(text.contains("has_wpf_fallback_decoder: false"), "{text}");
}

#[test]
fn debug_of_graph_overrides_names_the_type_and_which_overrides_are_set() {
    let text = format!("{:?}", GraphOverrides::default());
    assert!(text.contains("GraphOverrides"), "{text}");
    assert!(text.contains("paths: false"), "{text}");
    let text = format!(
        "{:?}",
        GraphOverrides {
            clock: Some(Arc::new(pr_test_support::FakeClock::new())),
            ..Default::default()
        }
    );
    assert!(text.contains("clock: true"), "{text}");
}

#[test]
fn debug_of_shared_services_and_its_inputs_name_the_types() {
    let (inputs, _root, _log) = composition_fixture::shared_service_inputs();
    let text = format!("{inputs:?}");
    assert!(text.contains("SharedServiceInputs"), "{text}");
    let services = pr_shell::composition::registry::SharedServices::build(inputs);
    let text = format!("{services:?}");
    assert!(text.contains("SharedServices"), "{text}");
}

#[test]
fn debug_of_catalog_feed_feed_preload_controller_and_host_name_the_types() {
    let _app_log = APP_LOG.lock().unwrap_or_else(PoisonError::into_inner); // resolving the decoder factory writes to the process-wide AppLog
    assert!(format!("{:?}", CatalogFeed::new()).contains("CatalogFeed"));

    let (services, _root, _log) = composition_fixture::shared_services();
    let feed = CatalogFeed::new();
    let scheduler = (services.preload_scheduler_factory())(feed.entries_fn(), feed.version_fn()).expect("scheduler");
    let controller = FeedPreloadController::new(Arc::clone(&scheduler), feed);
    assert!(format!("{controller:?}").contains("FeedPreloadController"));
    scheduler.dispose();

    let host = MainViewModelHost {
        ui_scheduler: Arc::new(pr_test_support::QueuedUiScheduler::new()),
        dialog_service: Arc::new(Dialogs),
        folder_picker: None,
        clipboard: Box::new(Clipboard),
        sink_factory: Box::new(SinkFactory),
        observer: Rc::new(Observer(RefCell::new(Vec::new()))),
        wake: Arc::new(|| {}),
        preload_controller: None,
        runner: Arc::new(ThreadRunner),
    };
    assert!(format!("{host:?}").contains("MainViewModelHost"));
}

struct Dialogs;
impl DialogService for Dialogs {
    fn show_confirmation(&self, _: &str, _: &str) -> bool {
        false
    }
    fn show_message(&self, _: &str, _: &str) {}
    fn show_error(&self, _: &str, _: &str) {}
    fn pick_folder(&self, _: Option<&Path>) -> Option<PathBuf> {
        None
    }
    fn show_batch_review(&self, _: &[BatchReviewItem]) -> bool {
        false
    }
    fn show_recovery(&self) {}
    fn show_diagnostics(&self) {}
    fn show_settings(&self) -> bool {
        false
    }
    fn show_benchmark(&self, _: Option<&Path>) {}
    fn show_skipped_files(&self, _: &[SkippedEntry]) {}
}
struct Clipboard;
impl ClipboardService for Clipboard {
    fn try_set_text(&self, _: &str) -> bool {
        true
    }
}
struct Observer(RefCell<Vec<String>>);
impl PresentationObserver for Observer {
    fn on_presented(&self, path: &str) {
        self.0.borrow_mut().push(path.to_owned());
    }
}
struct Sink;
impl IPresentationSink for Sink {
    fn set_current_image(&mut self, _: Option<PlatformImage>, _: bool, _: &ImageChange<'_>) {}
    fn set_status_text(&mut self, _: &str) {}
    fn apply_initial_view_mode(&mut self) {}
    fn on_presented(&mut self, _: &str) {}
    fn trace_presented(&mut self, _: i64, _: &str, _: i64) {}
}
struct SinkFactory;
impl PresentationSinkFactory for SinkFactory {
    fn create(&self, _: MainViewModelSinkCallbacks) -> Box<dyn IPresentationSink> {
        Box::new(Sink)
    }
}

// ---- registry: the journal options ------------------------------------------------------------------------------------------------------------------------

#[test]
fn journal_durability_follows_the_settings_at_every_read() {
    // mutant: `durability` field deleted (the journal then always answers Fast).
    let (services, _root, _log) = composition_fixture::shared_services();
    let journal = services.journal();
    assert_eq!(journal.durability(), JournalDurability::Fast);
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.journal_durability = JournalDurability::PowerLossSafe;
    store.save(settings).expect("save settings");
    assert_eq!(journal.durability(), JournalDurability::PowerLossSafe);
}

#[test]
fn journal_is_wired_with_compaction_files_so_it_can_compact() {
    // mutant: `compaction_files` deleted (`try_compact` then answers NotSupported without looking at the file).
    let (services, _root, _log) = composition_fixture::shared_services();
    let outcome = services.journal().try_compact().outcome;
    assert!(
        !matches!(outcome, JournalCompactionOutcome::NotSupported),
        "{outcome:?}"
    );
}

#[test]
fn journal_diagnostics_go_to_the_injected_log() {
    // mutant: `log` deleted (the journal then falls back to the process default FileLog).
    let (services, _root, log) = composition_fixture::shared_services();
    let journal = services.journal();
    let injected: *const dyn pr_core::abstractions::Log = Arc::as_ptr(&log) as *const _;
    assert!(std::ptr::addr_eq(journal.diagnostics_log() as *const _, injected));
}

#[test]
fn journal_reconcile_asks_the_platform_recycle_bin() {
    // mutant: `recycle_bin` deleted (reconcile would then trust "source gone").
    let (services, _root, _log) = composition_fixture::shared_services();
    let journal = services.journal();
    let wired = ReconcileJournal::recycle_bin(&*journal).expect("a recycle bin is wired into the journal");
    let expected = services.recycle_bin();
    assert!(std::ptr::addr_eq(wired as *const _, Arc::as_ptr(&expected)));
}

static AMBIENT: Mutex<()> = Mutex::new(());

#[test]
fn journal_uses_the_localized_texts() {
    // mutant: `texts` deleted (the journal then uses `EnglishJournalTexts`, whose `localized` equals `english`).
    let _lock = AMBIENT.lock().unwrap_or_else(PoisonError::into_inner);
    let (services, _root, _log) = composition_fixture::shared_services();
    let journal = services.journal();
    Localizer::set_current(Arc::new(create_show_keys(&built_in_english_localizer())));
    let key = "core.journal.sourceStillExists";
    let localized = journal.texts().localized(key);
    let english = journal.texts().english(key);
    Localizer::set_current(built_in_english_localizer());
    assert_ne!(localized, english);
    assert!(english.contains("The source still exists"), "{english}");
}

// ---- registry: viewport size source -----------------------------------------------------------------------------------------------------------------------

#[test]
fn viewport_size_source_is_one_shared_instance() {
    // mutant: the accessor returns a fresh `Arc::new(Default::default())` every call.
    let (services, _root, _log) = composition_fixture::shared_services();
    let first = services.viewport_size_source();
    let second = services.viewport_size_source();
    assert!(Arc::ptr_eq(&first, &second));
    first.set_viewport_size_fn(Arc::new(|| (123.0, 45.0)));
    assert_eq!(services.viewport_size_source().viewport_size(), (123.0, 45.0));
}

// ---- registry: is_libraw_needed ---------------------------------------------------------------------------------------------------------------------------

static APP_LOG: Mutex<()> = Mutex::new(());

struct RedirectedLog {
    _guard: std::sync::MutexGuard<'static, ()>,
    root: TempRoot,
}

impl RedirectedLog {
    fn new() -> Self {
        let guard = APP_LOG.lock().unwrap_or_else(PoisonError::into_inner);
        AppLog::global().flush(); // a write queued by the previous test must not land in this test's file
        let root = TempRoot::new("lane-e-applog");
        let target: Arc<dyn AppLogTarget> =
            Arc::new(FileLog::new(root.join("app.log"), DEFAULT_MAX_LOG_BYTES).expect("absolute temp path"));
        AppLog::global().set_instance(Some(target));
        AppLog::global().set_enabled(false);
        Self { _guard: guard, root }
    }

    fn text(&self) -> String {
        AppLog::global().flush();
        std::fs::read_to_string(self.root.join("app.log")).unwrap_or_default()
    }
}

impl Drop for RedirectedLog {
    fn drop(&mut self) {
        AppLog::global().flush();
        AppLog::global().set_enabled(true);
        AppLog::global().set_instance(None);
    }
}

const LIBRAW_FORCED: &str = "LibRaw decoder backend is not available and will not be registered";

/// With logging off, a LibRaw probe that fails is written (forced error) only when RAW support is on or the LibRaw backend is selected. A machine whose probe
/// succeeds registers the backend and logs nothing, so the assertion has nothing to see there.
fn libraw_forced_error_logged(raw_support: bool, backend: Option<DecoderBackend>) -> Option<bool> {
    if pr_libraw::availability::probe().available {
        return None;
    }
    let log = RedirectedLog::new();
    let (services, _root, _plog) = composition_fixture::shared_services();
    let store = services.settings_store();
    let mut settings = (*store.current()).clone();
    settings.raw_support_enabled = raw_support;
    settings.decoder_backend = backend;
    store.save(settings).expect("save settings");
    services.decoder_factory().expect("decoder factory");
    Some(log.text().contains(LIBRAW_FORCED))
}

#[test]
fn libraw_is_needed_when_raw_support_is_on() {
    // mutants `||` -> `&&` (raw on, backend not LibRaw must still be "needed").
    if let Some(logged) = libraw_forced_error_logged(true, Some(DecoderBackend::WicDirect)) {
        assert!(logged);
    }
}

#[test]
fn libraw_is_needed_when_the_libraw_backend_is_selected() {
    if let Some(logged) = libraw_forced_error_logged(false, Some(DecoderBackend::LibRaw)) {
        assert!(logged);
    }
}

#[test]
fn libraw_is_not_needed_when_raw_support_is_off_and_another_backend_is_selected() {
    // mutant `==` -> `!=`.
    if let Some(logged) = libraw_forced_error_logged(false, Some(DecoderBackend::WicDirect)) {
        assert!(!logged);
    }
}

// ---- registry: thumbnail cache log and clock --------------------------------------------------------------------------------------------------------------

fn set_mtime(path: &Path, unix_seconds: u64) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for set_modified");
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(unix_seconds))
        .expect("set_modified");
}

/// 2026-01-01T00:00:00Z, the start of `FakeClock::new()` (the fixture clock).
const FAKE_NOW: u64 = 1_767_225_600;

#[test]
fn thumbnail_cache_startup_sweep_uses_the_injected_clock() {
    // mutant: `clock` deleted (SystemClock: a temp file last written at the fake "now" would be ten months old and swept).
    let (services, _root, _log) = composition_fixture::shared_services();
    let dir = services.paths().thumbnail_cache_dir.clone();
    std::fs::create_dir_all(&dir).expect("create dir");
    let fresh = dir.join("fresh-at-fake-now.tmp");
    std::fs::write(&fresh, b"x").expect("write");
    set_mtime(&fresh, FAKE_NOW);
    let cache = services.thumbnail_cache().expect("thumbnail cache");
    assert!(cache.wait_startup_cleanup(Duration::from_secs(10)));
    assert!(fresh.exists(), "the sweep must compare against the injected clock");
}

#[test]
fn thumbnail_cache_sweep_failures_go_to_the_injected_log() {
    // mutant: `log` deleted (NullLog). A stale read-only temp file cannot be deleted: "Delete failed" is logged by the startup sweep.
    let (services, _root, log) = composition_fixture::shared_services();
    let dir = services.paths().thumbnail_cache_dir.clone();
    std::fs::create_dir_all(&dir).expect("create dir");
    let stale = dir.join("stale-readonly.tmp");
    std::fs::write(&stale, b"x").expect("write");
    set_mtime(&stale, FAKE_NOW - 3600);
    let mut permissions = std::fs::metadata(&stale).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&stale, permissions).expect("read-only");
    let cache = services.thumbnail_cache().expect("thumbnail cache");
    assert!(cache.wait_startup_cleanup(Duration::from_secs(10)));
    let logged = log.contains(LogLevel::Error, "Delete failed");
    let mut permissions = std::fs::metadata(&stale).expect("metadata").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&stale, permissions).expect("writable again");
    assert!(logged, "{:?}", log.entries());
}

// ---- registry: preview service RAW full decode gate -------------------------------------------------------------------------------------------------------

#[test]
fn preview_service_raw_full_decode_needs_raw_support_and_on_zoom() {
    // mutants `&&` -> `||` and `==` -> `!=`. The RAW full decoder exists only when `libraw.dll` loads next to the executable; without it the gate is unreachable.
    if !pr_libraw::availability::probe().available {
        eprintln!("libraw is not available next to the test executable: the gate is not observable here");
        return;
    }
    let check = |raw_support: bool, mode: RawFullDecode| {
        let _app_log = APP_LOG.lock().unwrap_or_else(PoisonError::into_inner);
        let (services, _root, _log) = composition_fixture::shared_services();
        let store = services.settings_store();
        let mut settings = (*store.current()).clone();
        settings.raw_support_enabled = raw_support;
        settings.raw_full_decode = mode;
        store.save(settings).expect("save settings");
        services
            .preview_service()
            .expect("preview service")
            .is_raw_full_decode_request("photo.cr2")
    };
    assert!(check(true, RawFullDecode::OnZoom));
    assert!(!check(false, RawFullDecode::OnZoom));
    assert!(!check(true, RawFullDecode::Never));
}

// ---- registry: preload scheduler factory deps -------------------------------------------------------------------------------------------------------------

#[test]
fn preload_scheduler_reads_the_snapshot_version_it_was_given() {
    // mutant: `snapshot_version` deleted (the scheduler would never ask for the structural version).
    let _app_log = APP_LOG.lock().unwrap_or_else(PoisonError::into_inner); // resolving the decoder factory writes to the process-wide AppLog
    let (services, _root, _log) = composition_fixture::shared_services();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let entries = Arc::new(|| {
        Ok(Arc::<[pr_core::catalog::types::CatalogEntry]>::from(vec![
            pr_core::catalog::types::CatalogEntry::new("item.jpg")?,
        ]))
    });
    let version = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(0)
    });
    let scheduler = (services.preload_scheduler_factory())(entries, version).expect("scheduler");
    let run = scheduler.preload_around(0);
    let _ = run.wait_timeout(Duration::from_secs(3));
    scheduler.dispose();
    assert!(calls.load(Ordering::SeqCst) > 0);
}

#[test]
fn preload_scheduler_writes_to_the_injected_log() {
    // mutant: `log` deleted (NullLog). A failing version callback makes `preload_around` log "Preload kick failed; skipped".
    let _app_log = APP_LOG.lock().unwrap_or_else(PoisonError::into_inner); // resolving the decoder factory writes to the process-wide AppLog
    let (services, _root, log) = composition_fixture::shared_services();
    let entries = Arc::new(|| Ok(Arc::<[pr_core::catalog::types::CatalogEntry]>::from(Vec::new())));
    let version = Arc::new(|| Err("version failed".into()));
    let scheduler = (services.preload_scheduler_factory())(entries, version).expect("scheduler");
    let run = scheduler.preload_around(0);
    let _ = run.wait_timeout(Duration::from_secs(3));
    scheduler.dispose();
    assert!(
        log.contains(LogLevel::Error, "Preload kick failed"),
        "{:?}",
        log.entries()
    );
}
