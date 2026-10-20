//! Tests of `crates/pr-app/src/coordinators/traits.rs` (contract `contracts/W8-06d.md`, "Test plan"): the C# default members
//! of the sink and seam traits, and the two forwarding classes (`FileActionServiceExecutor`, `into_move_override`).
//!
//! C# source: `PhotoReview.App.Shared/Coordinators/` (baseline d92d6aae). The C# tests of these files are not in this file: the
//! default members have no C# test of their own, so every test here is `// new:` (derived from the C# default bodies), except
//! the forwarder tests, which are `// new:` too (the C# `FileActionServiceExecutor` and `IMoveOverride` have no unit test here).
//!
//! Red status: the default-member tests are green on the stub (the defaults are real code). The executor and move-override
//! tests are red on `todo!()` until the forwarders are implemented.
//!
//! Harness is test-local: a `RecordingSink` per sink trait (the call trace as a `Vec<String>`), a `FakeMoveOverride` that
//! records both paths and returns a scripted `io::Result`, and a real `FileActionService` on fakes (`pr_test_support`).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pr_app::coordinators::traits::{
    FileActionServiceExecutor, IDuplicateCleanupSink, IFileActionExecutor, IFileActionSink, IFolderLoadSink,
    IMoveOverride, IPreloadController, into_move_override,
};
use pr_core::abstractions::SkippedEntry;
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::service::FileActionService;
use pr_core::session::store::SessionState;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::preload::scheduler::RunHandle;
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin};

/// Locks ignoring poison (a panicking fake must not wedge the test that reports it).
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- IDuplicateCleanupSink -------------------------------------------------------------------------------------------

/// Records the calls of `IDuplicateCleanupSink`; the defaults under test are not overridden.
#[derive(Default)]
struct DuplicateSink {
    log: Vec<String>,
}

impl IDuplicateCleanupSink for DuplicateSink {
    fn set_status_text(&mut self, status: &str) {
        self.log.push(format!("status:{status}"));
    }

    fn open_folder(&mut self, folder: &str, initial_path: Option<&str>) {
        self.log.push(format!("open:{folder}:{initial_path:?}"));
    }
}

#[test]
fn duplicate_cleanup_show_late_action_status_default_forwards_to_set_status() {
    // new: C# default `ShowLateActionStatus => SetStatusText` (RV-A10)
    let mut sink = DuplicateSink::default();

    sink.show_late_action_status("late");

    assert_eq!(vec!["status:late".to_owned()], sink.log);
}

#[test]
fn duplicate_cleanup_reload_folder_default_opens_without_initial_path_and_returns_true() {
    // new: C# default `ReloadFolderAsync(folder)` = `OpenFolderAsync(folder)` then `true` (D-06d-4)
    let mut sink = DuplicateSink::default();

    let reloaded = sink.reload_folder(r"C:\dups");

    assert!(reloaded);
    assert_eq!(vec![r"open:C:\dups:None".to_owned()], sink.log);
}

// ---- IFolderLoadSink -------------------------------------------------------------------------------------------------

/// Records the calls of `IFolderLoadSink`. `on_empty_with_subfolders` and `on_files_skipped` keep their defaults.
#[derive(Default)]
struct FolderSink {
    log: Vec<String>,
}

impl IFolderLoadSink for FolderSink {
    fn reset_caches(&mut self) {
        self.log.push("reset".to_owned());
    }

    fn on_catalog_ready(&mut self, folder: &str, count: i32, _session: &SessionState) {
        self.log.push(format!("ready:{folder}:{count}"));
    }

    fn present(&mut self, index: i32, presentation_generation: i64) {
        self.log.push(format!("present:{index}:{presentation_generation}"));
    }

    fn on_empty(&mut self, folder: &str, _session: &SessionState) {
        self.log.push(format!("empty:{folder}"));
    }

    fn on_order_applied(&mut self, count: i32, current_index: i32, current_kept: bool) {
        self.log.push(format!("order:{count}:{current_index}:{current_kept}"));
    }

    fn on_unreadable_removed(&mut self, removed_paths: &[String], current_removed: bool) {
        self.log
            .push(format!("unreadable:{}:{current_removed}", removed_paths.len()));
    }

    fn on_failed(&mut self, folder: &str, error: &dyn std::error::Error) {
        self.log.push(format!("failed:{folder}:{error}"));
    }
}

#[test]
fn folder_load_on_empty_with_subfolders_default_forwards_to_on_empty() {
    // new: C# Q-R47 default `OnEmptyWithSubfolders` forwards to `OnEmpty`; the subfolder count is not passed on
    let mut sink = FolderSink::default();
    let session = SessionState::default();

    sink.on_empty_with_subfolders(r"C:\empty", &session, 3);

    assert_eq!(vec![r"empty:C:\empty".to_owned()], sink.log);
}

#[test]
fn folder_load_on_files_skipped_default_is_empty() {
    // new: C# default `OnFilesSkipped` is an empty body (IO05): nothing is recorded, nothing else is called
    let mut sink = FolderSink::default();
    let skipped: [SkippedEntry; 0] = [];

    sink.on_files_skipped(r"C:\folder", &skipped);

    assert!(sink.log.is_empty());
}

// ---- IFileActionSink -------------------------------------------------------------------------------------------------

/// Records the calls of `IFileActionSink`. `evict_cached_paths` keeps its default (empty).
#[derive(Default)]
struct FileActionSink {
    log: Vec<String>,
}

impl IFileActionSink for FileActionSink {
    fn set_status_text(&mut self, status: &str) {
        self.log.push(format!("status:{status}"));
    }

    fn show_late_action_status(&mut self, status: &str) {
        self.log.push(format!("late:{status}"));
    }

    fn on_catalog_changed(&mut self, removed_path: Option<&str>) {
        self.log.push(format!("changed:{removed_path:?}"));
    }

    fn present(&mut self, index: i32) {
        self.log.push(format!("present:{index}"));
    }

    fn update_session_path(&mut self, current_path: &str) {
        self.log.push(format!("session:{current_path}"));
    }

    fn notify_navigation_state_changed(&mut self) {
        self.log.push("nav".to_owned());
    }
}

#[test]
fn file_action_sink_evict_cached_paths_default_is_empty() {
    // new: C# default `EvictCachedPaths` is an empty body; an implementation that caches nothing keeps it
    let mut sink = FileActionSink::default();

    sink.evict_cached_paths(&[r"C:\a\IMG_0001.jpg".to_owned()]);

    assert!(sink.log.is_empty());
}

// ---- IPreloadController defaults -------------------------------------------------------------------------------------

/// A bare `IPreloadController`: the required members record nothing; only the three defaults are read by the test.
struct BarePreload;

impl IPreloadController for BarePreload {
    fn preload_around(&self, _center: i32) -> RunHandle {
        RunHandle::completed()
    }

    fn try_consume_preloaded_key(&self, _key: &ImageCacheKey) -> bool {
        false
    }

    fn cancel(&self) {}

    fn remove_preloaded_keys_for_path(&self, _normalized_path: &str) {}

    fn clear_preloaded_keys(&self) {}
}

#[test]
fn preload_controller_defaults_are_idle_no_op_navigation_and_zero_decode_delay() {
    // new: C# defaults `IsIdle => true`, `NotifyNavigation(index)` (empty), `GetViewerDecodeDelay()` (zero)
    let controller = BarePreload;

    assert!(controller.is_idle());
    controller.notify_navigation(7);
    assert_eq!(Duration::ZERO, controller.get_viewer_decode_delay());
}

// ---- IMoveOverride and the into_move_override helper -----------------------------------------------------------------

/// Records both paths of every `move_file` call and returns the scripted result (`Ok` unless a failure is set).
struct FakeMoveOverride {
    calls: Mutex<Vec<(PathBuf, PathBuf)>>,
    failure: Mutex<Option<io::ErrorKind>>,
}

impl FakeMoveOverride {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            failure: Mutex::new(None),
        })
    }

    fn fail_with(&self, kind: io::ErrorKind) {
        *lock(&self.failure) = Some(kind);
    }

    fn calls(&self) -> Vec<(PathBuf, PathBuf)> {
        lock(&self.calls).clone()
    }
}

impl IMoveOverride for FakeMoveOverride {
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        lock(&self.calls).push((source.to_path_buf(), destination.to_path_buf()));
        match *lock(&self.failure) {
            Some(kind) => Err(io::Error::new(kind, "scripted move failure")),
            None => Ok(()),
        }
    }
}

#[test]
fn into_move_override_forwards_both_paths_and_the_result() {
    // new: the closure calls `move_file(source, destination)` and returns its `io::Result` unchanged (contract, semantics 5)
    let fake = FakeMoveOverride::new();
    let over = into_move_override(fake.clone());
    let source = Path::new(r"C:\photos\IMG_0001.jpg");
    let destination = Path::new(r"D:\archive\IMG_0001.jpg");

    assert!(over(source, destination).is_ok());
    assert_eq!(vec![(source.to_path_buf(), destination.to_path_buf())], fake.calls());

    fake.fail_with(io::ErrorKind::PermissionDenied);
    let error = over(source, destination).expect_err("the scripted failure is returned");
    assert_eq!(io::ErrorKind::PermissionDenied, error.kind());
    assert_eq!(2, fake.calls().len());
}

// ---- FileActionServiceExecutor -----------------------------------------------------------------------------------------

#[test]
fn file_action_service_executor_forwards_busy_and_recycle_bin_checks() {
    // new: forwarding only (the service behaviour is W3-04 / W8-07c). `is_busy` and `lacks_recycle_bin` reach the real service.
    let fs = Arc::new(FakeFileSystem::new());
    let clock = Arc::new(FakeClock::new());
    let paths = AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false);
    let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
    let bin = Arc::new(FakeRecycleBin::new(fs.clone()));
    bin.set_can_recycle(false);
    let service = Arc::new(FileActionService::new(journal, fs.clone(), clock.clone(), bin.clone()));
    let executor = FileActionServiceExecutor::new(Arc::clone(&service));

    assert!(!executor.is_busy());
    assert!(service.try_begin());
    assert!(executor.is_busy(), "the executor reports the service's gate");
    service.end();
    assert!(!executor.is_busy());

    assert!(executor.lacks_recycle_bin(r"C:\photos\IMG_0001.jpg"));
    bin.set_can_recycle(true);
    assert!(!executor.lacks_recycle_bin(r"C:\photos\IMG_0001.jpg"));
}
