//! W9-R10: the remaining `FileActionControllerRenameTests` of C# 7eaee2a2 (the R5 twins in `file_action_rename.rs` already
//! cover Effects order, SameName, FolderSwitchedDuringTheIo and CaptureGroup respell, so those four are not repeated).
mod file_action_fakes;

use std::path::Path;
use std::sync::{Arc, Mutex};

use file_action_fakes::{EnglishTexts, ManualPort, RecordingLog, RecordingPreload};
use pr_app::coordinators::file_actions::recycle::{
    FileActionController, FileActionCx, FileActionDeps, FileActionDone, FileActionStep,
};
use pr_app::coordinators::rename::FileRenameStatus;
use pr_app::coordinators::traits::{FileActionServiceExecutor, IFileActionSink, IPreloadController};
use pr_core::abstractions::{Clock, FileSystem, RecycleBin};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::natural_cmp::ManagedNaturalComparer;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CatalogEntry;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::undo::{EnglishUndoTexts, FileActionGate, MoveOverride, UndoService};
use pr_core::model::{FileOperationType, RawPairMode};
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::shortcuts::ReviewAction;
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin, TempRoot};

struct RenamePreload {
    inner: RecordingPreload,
    log: Arc<Mutex<Vec<String>>>,
}

impl std::ops::Deref for RenamePreload {
    type Target = RecordingPreload;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl IPreloadController for RenamePreload {
    fn preload_around(&self, center: i32) -> pr_imaging::preload::scheduler::RunHandle {
        self.log.lock().expect("ordered log").push(format!("Preload:{center}"));
        self.inner.preload_around(center)
    }
    fn try_consume_preloaded_key(&self, key: &pr_imaging::cache_key::ImageCacheKey) -> bool {
        self.inner.try_consume_preloaded_key(key)
    }
    fn cancel(&self) {
        self.log.lock().expect("ordered log").push("Cancel".into());
        self.inner.cancel();
    }
    fn remove_preloaded_keys_for_path(&self, path: &str) {
        self.inner.remove_preloaded_keys_for_path(path);
    }
    fn clear_preloaded_keys(&self) {
        self.inner.clear_preloaded_keys();
    }
}

#[derive(Default)]
struct Sink {
    keep: bool,
    maps: Vec<PathKeyMap>,
    presented: Vec<i32>,
    late: Vec<String>,
    status: Vec<String>,
    removed: Vec<Option<String>>,
    sessions: Vec<String>,
    nav: usize,
    effects: Vec<&'static str>,
    action_log: Arc<Mutex<Vec<String>>>,
}

impl IFileActionSink for Sink {
    fn set_status_text(&mut self, value: &str) {
        self.effects.push("status");
        self.status.push(value.to_owned());
    }
    fn show_late_action_status(&mut self, value: &str) {
        self.late.push(value.to_owned());
    }
    fn on_catalog_changed(&mut self, value: Option<&str>) {
        self.removed.push(value.map(str::to_owned));
    }
    fn present(&mut self, index: i32) {
        self.effects.push("present");
        self.presented.push(index);
    }
    fn update_session_path(&mut self, value: &str) {
        self.sessions.push(value.to_owned());
    }
    fn notify_navigation_state_changed(&mut self) {
        self.effects.push("nav");
        self.nav += 1;
    }
    fn on_files_renamed(&mut self, paths: &PathKeyMap) -> bool {
        self.action_log
            .lock()
            .expect("ordered log")
            .push("OnFilesRenamed".into());
        self.effects.push("renamed");
        self.maps.push(paths.clone());
        self.keep
    }
}

struct RenameEnv {
    root: TempRoot,
    fs: Arc<FakeFileSystem>,
    catalog: ReviewCatalog,
    clock: Arc<GenerationClock>,
    fake_clock: Arc<FakeClock>,
    sink: Sink,
    preload: Arc<RenamePreload>,
    worker: Arc<ManualPort>,
    controller: FileActionController,
    undo: Arc<Mutex<UndoService>>,
    service: Arc<FileActionService>,
    journal: Arc<OperationJournal>,
    log: Arc<RecordingLog>,
    hook: IoHook,
    app_paths: AppPaths,
}

/// Runs at the rename I/O with the 1-based call number; an `Err` is the I/O failure (C# `RenameHook`).
type IoHook = Arc<Mutex<Option<Box<dyn FnMut(usize) -> std::io::Result<()> + Send>>>>;

impl RenameEnv {
    fn new() -> Self {
        Self::with_executor(None)
    }

    fn with_executor(
        executor_override: Option<Arc<dyn pr_app::coordinators::traits::IFileActionExecutor + Send + Sync>>,
    ) -> Self {
        let root = TempRoot::new("rename-controller-matrix");
        let app = AppPaths::new(&root.join("appdata"), None, false);
        let fake_clock = Arc::new(FakeClock::new());
        let fs = Arc::new(FakeFileSystem::with_clock(fake_clock.clone()));
        let fs_dyn: Arc<dyn FileSystem> = fs.clone();
        let clock = Arc::new(GenerationClock::default());
        let action_log = Arc::new(Mutex::new(Vec::new()));
        let preload = Arc::new(RenamePreload {
            inner: RecordingPreload::default(),
            log: action_log.clone(),
        });
        let hook: IoHook = Arc::new(Mutex::new(None));
        let io_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let move_override = |label: &'static str| -> MoveOverride {
            let hook = hook.clone();
            let io_calls = io_calls.clone();
            let move_clock = clock.clone();
            let move_preload = preload.clone();
            let move_fs = fs_dyn.clone();
            let log = action_log.clone();
            Arc::new(move |source, destination| {
                let _ = (&move_clock, &move_preload);
                log.lock().expect("ordered log").push(label.into());
                let call = io_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if label == "IO" {
                    if let Some(hook) = hook.lock().expect("io hook").as_mut() {
                        hook(call)?;
                    }
                }
                move_fs.move_file(source, destination)
            })
        };
        let journal = Arc::new(OperationJournal::new(&app, fs_dyn.clone(), fake_clock.clone()));
        let bin: Arc<dyn RecycleBin> = Arc::new(FakeRecycleBin::new(fs.clone()));
        let service = Arc::new(
            FileActionService::new(journal.clone(), fs_dyn.clone(), fake_clock.clone(), bin.clone())
                .with_move_override(move_override("IO")),
        );
        let undo = Arc::new(Mutex::new(
            UndoService::new(journal.clone(), fs_dyn.clone(), bin, Arc::new(EnglishUndoTexts))
                .with_move_override(move_override("UndoIO"))
                .with_clock(fake_clock.clone())
                .with_gate(service.clone() as Arc<dyn FileActionGate>),
        ));
        fs.add_dir(root.join("Sorted"));
        let worker = ManualPort::new();
        let actions = vec![ReviewAction {
            name: "Move".into(),
            shortcut: "Enter".into(),
            operation: FileOperationType::Move,
            destination: root.join("Sorted").to_string_lossy().into_owned(),
            confirm: false,
        }];
        let settings = Arc::new(AppSettings {
            actions,
            ..AppSettings::default()
        });
        let settings_for_controller = settings.clone();
        let executor: Arc<dyn pr_app::coordinators::traits::IFileActionExecutor + Send + Sync> =
            executor_override.unwrap_or_else(|| Arc::new(FileActionServiceExecutor::new(service.clone())));
        let log = Arc::new(RecordingLog::default());
        let controller = FileActionController::new(FileActionDeps {
            clock: clock.clone(),
            file_actions: Some(executor),
            undo_service: Some(undo.clone()),
            dialog_service: None,
            preload_controller: Some(preload.clone()),
            natural_comparer: Arc::new(ManagedNaturalComparer),
            get_settings: Box::new(move || settings_for_controller.clone()),
            file_system: Some(fs.clone()),
            remember_folder: None,
            worker: worker.clone(),
            texts: Arc::new(EnglishTexts::new()),
            log: log.clone(),
        });
        Self {
            root,
            fs,
            catalog: ReviewCatalog::new(),
            clock,
            fake_clock,
            sink: Sink {
                keep: true,
                action_log,
                ..Sink::default()
            },
            preload,
            worker,
            controller,
            undo,
            service,
            journal,
            log,
            hook,
            app_paths: app,
        }
    }

    fn path(&self, name: &str) -> String {
        self.root.join(name).to_string_lossy().into_owned()
    }

    fn make(&self, name: &str) -> String {
        let path = self.path(name);
        self.fs.add_file_at(&path, b"rename fixture", self.fake_clock.utc_now());
        path
    }

    fn exists(&self, name: &str) -> bool {
        self.fs.file_exists(Path::new(name))
    }

    fn load(&mut self, names: &[&str], current: usize) -> Vec<String> {
        let paths: Vec<_> = names.iter().map(|name| self.make(name)).collect();
        let entries = paths
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let ordinal = i64::try_from(index + 1).expect("small fixture");
                let stamp = pr_core::time::DateTimeUtc::from_ticks(637_135_310_450_000_000 + ordinal * 864_000_000_000)
                    .expect("stamp");
                CatalogEntry::new(path.clone()).expect("valid temp path").with_metadata(
                    1111 * ordinal,
                    stamp,
                    Some(10 + 20 * index as i32),
                    Some(20 + 20 * index as i32),
                )
            })
            .collect();
        self.catalog.reset(entries, RawPairMode::PreferJpeg, None);
        self.catalog.set_current(i32::try_from(current).expect("small index"));
        paths
    }

    fn rename(&mut self, source: &str, name: &str) -> FileActionStep {
        let mut cx = FileActionCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.rename(&mut cx, None, Some(source), name)
    }

    fn deliver_worker(&mut self) -> FileActionStep {
        let mut result = FileActionStep::Pending;
        for event in self.worker.run_all() {
            let mut cx = FileActionCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            result = self.controller.handle_event(&mut cx, event);
        }
        result
    }

    fn finish_present(&mut self) -> FileActionStep {
        let mut cx = FileActionCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.on_present_finished(&mut cx, false)
    }
}

/// `FileRenameResult` of a finished rename (panics on any other step: the test then shows what came back).
fn finished(step: FileActionStep) -> pr_app::coordinators::rename::FileRenameResult {
    match step {
        FileActionStep::Done(FileActionDone::Renamed(result)) => result,
        other => panic!("expected a finished rename, got {other:?}"),
    }
}

/// Executor that answers every call with a panic (the Rust shape of a C# executor that throws).
struct ThrowingRenameExecutor {
    message: &'static str,
}

impl pr_app::coordinators::traits::IFileActionExecutor for ThrowingRenameExecutor {
    fn is_busy(&self) -> bool {
        false
    }
    fn lacks_recycle_bin(&self, _path: &str) -> bool {
        false
    }
    fn execute(
        &self,
        _request: &pr_core::file_actions::types::FileActionRequest,
        _cancel: &pr_core::sync::CancelToken,
    ) -> pr_core::file_actions::types::FileActionResult {
        panic!("{}", self.message)
    }
    fn execute_group(
        &self,
        _request: &pr_core::file_actions::types::CaptureGroupActionRequest,
        _cancel: &pr_core::sync::CancelToken,
    ) -> pr_core::file_actions::types::CaptureGroupActionResult {
        panic!("{}", self.message)
    }
    fn rename(
        &self,
        _request: &pr_core::file_actions::rename::RenameRequest,
        _cancel: &pr_core::sync::CancelToken,
    ) -> pr_core::file_actions::types::FileActionResult {
        panic!("{}", self.message)
    }
    fn rename_group(
        &self,
        _group: &pr_core::catalog::types::CaptureGroup,
        _typed_stem: &str,
        _cancel: &pr_core::sync::CancelToken,
    ) -> pr_core::file_actions::types::CaptureGroupActionResult {
        panic!("{}", self.message)
    }
}

impl RenameEnv {
    fn journal_text(&self) -> String {
        self.fs
            .file_content(&self.app_paths.journal_file)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    }

    fn journal_line_count(&self) -> usize {
        self.journal_text().lines().count()
    }

    fn io_count(&self) -> usize {
        self.sink
            .action_log
            .lock()
            .expect("ordered log")
            .iter()
            .filter(|line| line.as_str() == "IO")
            .count()
    }

    /// Runs a rename to its end, also finishing the present the sink asked for (when it cannot keep the shown image).
    fn rename_to_end(&mut self, source: &str, name: &str) -> pr_app::coordinators::rename::FileRenameResult {
        let started = self.rename(source, name);
        let mut step = if started == FileActionStep::Pending {
            self.deliver_worker()
        } else {
            started
        };
        if step == FileActionStep::Pending {
            step = self.finish_present();
        }
        finished(step)
    }

    fn pending_or_failed(&self) -> usize {
        self.journal
            .read_pending_and_failed_operations()
            .expect("read the journal")
            .len()
    }
}

// C#: FileActionControllerRenameTests.RenameAsync_SingleMidList_KeepsIndexMetadataAndRekeysCatalogInPlace
#[test]
fn rename_async_single_mid_list_keeps_index_metadata_and_rekeys_catalog_in_place() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let before = env.catalog.current().expect("shown entry").clone();
    let result = env.rename_to_end(&paths[1], "renamed");
    let renamed = env.path("renamed.jpg");
    assert_eq!(result.status, FileRenameStatus::Renamed);
    assert_eq!(result.new_path, Some(renamed.clone()));
    assert_eq!(
        env.catalog.paths().to_vec(),
        [paths[0].clone(), renamed.clone(), paths[2].clone()]
    );
    assert_eq!(env.catalog.current_index(), 1);
    assert_eq!(env.catalog.count(), 3);
    let entry = env.catalog.current().expect("re-keyed entry");
    assert_eq!(
        entry.length,
        Some(2222),
        "not re-listed: the metadata the listing had is still there"
    );
    assert_eq!(entry.last_write_utc, before.last_write_utc);
    assert_eq!((entry.width, entry.height), (Some(30), Some(40)));
    assert!(env.exists(&renamed));
    assert!(!env.exists(&paths[1]));
    assert_eq!(env.sink.maps.len(), 1);
    assert_eq!(env.sink.maps[0].len(), 1);
    assert_eq!(env.sink.maps[0].get(&paths[1]), Some(&renamed));
    assert_eq!(
        env.sink.status.last().expect("status"),
        "Renamed b.jpg to renamed.jpg. Press Ctrl+Z to undo."
    );
}

// C#: FileActionControllerRenameTests.RenameAsync_SinkKeepsShownImage_NeverPresents
#[test]
fn rename_async_sink_keeps_shown_image_never_presents() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    env.sink.keep = true;
    env.rename_to_end(&paths[1], "x");
    assert!(env.sink.presented.is_empty());
    assert_eq!(*env.preload.preload_centers.lock().expect("preload"), [1]);
}

// C#: FileActionControllerRenameTests.RenameAsync_SinkCannotKeepShownImage_PresentsTheCurrentIndexOnce
#[test]
fn rename_async_sink_cannot_keep_shown_image_presents_the_current_index_once() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    env.sink.keep = false;
    assert_eq!(env.rename(&paths[1], "x"), FileActionStep::Pending);
    assert_eq!(
        env.deliver_worker(),
        FileActionStep::Pending,
        "the tail waits for the present"
    );
    assert_eq!(env.sink.presented, [1]);
    assert_eq!(finished(env.finish_present()).status, FileRenameStatus::Renamed);
    assert_eq!(env.sink.presented, [1], "presented exactly once");
    assert!(
        env.preload.preload_centers.lock().expect("preload").is_empty(),
        "the present warms the neighbours itself"
    );
}

// C#: FileActionControllerRenameTests.RenameAsync_InvalidName_IsRefusedWithItsProblemAndNothingTouched (4 InlineData rows)
#[test]
fn rename_async_invalid_name_is_refused_with_its_problem_and_nothing_touched() {
    use pr_core::file_actions::rename::RenameNameProblem;
    let rows = [
        ("", RenameNameProblem::Empty),
        ("a?b", RenameNameProblem::InvalidCharacter),
        ("CON", RenameNameProblem::ReservedDeviceName),
        ("name.", RenameNameProblem::TrailingDotOrSpace),
    ];
    for (typed, problem) in rows {
        let mut env = RenameEnv::new();
        let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
        let navigation = env.clock.current_navigation();
        let result = finished(env.rename(&paths[1], typed));
        assert_eq!(result.status, FileRenameStatus::Invalid, "{typed:?}");
        assert_eq!(result.problem, problem, "{typed:?}");
        assert_eq!(env.clock.current_navigation(), navigation, "{typed:?}");
        assert_eq!(env.preload.cancels.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(env.worker.queued(), 0);
        assert!(env.sink.action_log.lock().expect("ordered log").is_empty());
        assert_eq!(env.journal_line_count(), 0);
        assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
        assert!(env.exists(&paths[1]));
    }
}

// C#: FileActionControllerRenameTests.RenameAsync_InvalidCharacter_ReportsTheOffendingCharacter
#[test]
fn rename_async_invalid_character_reports_the_offending_character() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let result = finished(env.rename(&paths[1], "a?b"));
    assert_eq!(result.invalid_character, Some(u16::from(b'?')));
}

// C#: FileActionControllerRenameTests.RenameAsync_TargetTakenByAnotherFile_FailsKeepsEverythingAndDismissesTheJournalLine
#[test]
fn rename_async_target_taken_by_another_file_fails_keeps_everything_and_dismisses_the_journal_line() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let result = env.rename_to_end(&paths[1], "c");
    assert_eq!(result.status, FileRenameStatus::Failed);
    let error = result.error.clone().expect("the failure text");
    assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
    assert_eq!(env.catalog.current_index(), 1);
    assert!(env.exists(&paths[1]) && env.exists(&paths[2]));
    // The failed attempt re-armed what StopForAction stopped: an EMPTY map, nothing presented.
    assert_eq!(env.sink.maps.len(), 1);
    assert!(env.sink.maps[0].is_empty());
    assert!(env.sink.presented.is_empty());
    assert_eq!(
        env.sink.status.last().expect("status"),
        &format!("Could not rename b.jpg: {error}")
    );
    assert_eq!(env.pending_or_failed(), 0, "not a Recovery 'Failed' item");
}

// C#: FileActionControllerRenameTests.RenameAsync_TargetTaken_IsRefusedBeforeAnythingIsJournaled
#[test]
fn rename_async_target_taken_is_refused_before_anything_is_journaled() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    env.make("taken.jpg");
    env.rename_to_end(&paths[1], "taken");
    assert_eq!(env.journal_line_count(), 0);
    assert_eq!(env.io_count(), 0, "the file was never touched");
}

// C#: FileActionControllerRenameTests.RenameAsync_IoFailsAfterPreparedWithTheSourceUntouched_JournalLineIsDismissedNotAFailedRecoveryItem
#[test]
fn rename_async_io_fails_after_prepared_with_the_source_untouched_journal_line_is_dismissed_not_a_failed_recovery_item()
{
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    *env.hook.lock().expect("io hook") = Some(Box::new(|_| Err(std::io::Error::other("share hiccup"))));
    let result = env.rename_to_end(&paths[1], "x");
    assert_eq!(result.status, FileRenameStatus::Failed);
    let text = env.journal_text();
    assert!(text.contains("Dismissed"), "{text}");
    assert!(!text.contains("\"Failed\""), "{text}");
    assert_eq!(env.pending_or_failed(), 0);
    assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
    assert!(env.undo.lock().expect("undo").last_undo_action_identity().is_none());
    assert_eq!(env.sink.maps.len(), 1);
    assert!(env.sink.maps[0].is_empty());
}

// C#: FileActionControllerRenameTests.RenameAsync_ExecutorThrows_FailsKeepsCatalogAndReArmsWithoutEscaping
#[test]
fn rename_async_executor_throws_fails_keeps_catalog_and_re_arms_without_escaping() {
    let mut env = RenameEnv::with_executor(Some(Arc::new(ThrowingRenameExecutor { message: "boom" })));
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let result = env.rename_to_end(&paths[1], "x");
    assert_eq!(result.status, FileRenameStatus::Failed);
    assert_eq!(result.error.as_deref(), Some("boom"));
    assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
    assert_eq!(env.sink.maps.len(), 1);
    assert!(env.sink.maps[0].is_empty());
    assert_eq!(env.sink.status.last().expect("status"), "Could not rename b.jpg: boom");
    assert!(
        env.log
            .errors
            .lock()
            .expect("log")
            .iter()
            .any(|line| line.contains("Rename threw")),
        "the unexpected throw is logged"
    );
}

// C#: FileActionControllerRenameTests.RenameAsync_ExecutorThrowsAndSinkCannotKeep_PresentsAgain
#[test]
fn rename_async_executor_throws_and_sink_cannot_keep_presents_again() {
    let mut env = RenameEnv::with_executor(Some(Arc::new(ThrowingRenameExecutor { message: "disk" })));
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    env.sink.keep = false;
    let result = env.rename_to_end(&paths[1], "x");
    assert_eq!(result.status, FileRenameStatus::Failed);
    assert_eq!(env.sink.presented, [1]);
}

// C#: FileActionControllerRenameTests.RenameAsync_GateBusy_ReturnsBusyAndChangesNothing
#[test]
fn rename_async_gate_busy_returns_busy_and_changes_nothing() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert!(env.service.try_begin());
    let navigation = env.clock.current_navigation();
    let result = finished(env.rename(&paths[1], "x"));
    assert_eq!(result.status, FileRenameStatus::Busy);
    assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
    assert_eq!(env.clock.current_navigation(), navigation);
    assert_eq!(env.worker.queued(), 0);
    assert!(env.sink.action_log.lock().expect("ordered log").is_empty());
    assert!(env.exists(&paths[1]));
    env.service.end();
}

// C#: FileActionControllerRenameTests.RenameAsync_EmptyCatalogOrUnknownPath_IsNothingToRename
#[test]
fn rename_async_empty_catalog_or_unknown_path_is_nothing_to_rename() {
    let mut env = RenameEnv::new();
    let nope = env.path("nope.jpg");
    let empty = finished(env.rename(&nope, "x"));
    env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let unlisted = env.path("unlisted.jpg");
    let unknown = finished(env.rename(&unlisted, "x"));
    let no_path = {
        let mut cx = FileActionCx {
            catalog: &mut env.catalog,
            sink: &mut env.sink,
        };
        finished(env.controller.rename(&mut cx, None, None, "x"))
    };
    assert_eq!(empty.status, FileRenameStatus::NothingToRename);
    assert_eq!(unknown.status, FileRenameStatus::NothingToRename);
    assert_eq!(no_path.status, FileRenameStatus::NothingToRename);
    assert!(env.sink.action_log.lock().expect("ordered log").is_empty());
}

// C#: FileActionControllerRenameTests.RenameAsync_CompareSelection_RenamesTheSelectedFileNotTheCurrent
#[test]
fn rename_async_compare_selection_renames_the_selected_file_not_the_current() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let started = {
        let mut cx = FileActionCx {
            catalog: &mut env.catalog,
            sink: &mut env.sink,
        };
        env.controller.rename(&mut cx, Some(&paths[2]), Some(&paths[1]), "x")
    };
    assert_eq!(started, FileActionStep::Pending);
    let result = finished(env.deliver_worker());
    assert_eq!(result.status, FileRenameStatus::Renamed);
    assert_eq!(
        env.catalog.paths().to_vec(),
        [paths[0].clone(), paths[1].clone(), env.path("x.jpg")]
    );
    assert!(env.exists(&paths[1]));
}

// C#: FileActionControllerRenameTests.RenameAsync_CaseOnly_RenamesTheEntryAndTheFileSpelling
#[test]
fn rename_async_case_only_renames_the_entry_and_the_file_spelling() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let result = env.rename_to_end(&paths[1], "B");
    let upper = env.path("B.jpg");
    assert_eq!(result.status, FileRenameStatus::Renamed);
    assert_eq!(
        env.catalog.paths().to_vec(),
        [paths[0].clone(), upper.clone(), paths[2].clone()]
    );
    let spelled: Vec<String> = env
        .fs
        .all_files()
        .iter()
        .filter_map(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .collect();
    assert!(spelled.iter().any(|name| name == "B.jpg"), "{spelled:?}");
    assert!(!spelled.iter().any(|name| name == "b.jpg"), "{spelled:?}");
    assert_eq!(env.sink.maps[0].get(&paths[1]), Some(&upper));
    assert!(env.undo.lock().expect("undo").last_undo_action_identity().is_some());
}

// C#: FileActionControllerRenameTests.RenameAsync_CaptureGroupFailsAtTheSecondMember_RollsBackAndKeepsTheCatalog
#[test]
fn rename_async_capture_group_fails_at_the_second_member_rolls_back_and_keeps_the_catalog() {
    let mut env = RenameEnv::new();
    let before = env.make("zbefore.jpg");
    let jpeg = env.make("shot.jpg");
    let raw = env.make("shot.cr2");
    let after = env.make("aafter.jpg");
    let entries = [&before, &jpeg, &raw, &after]
        .iter()
        .map(|p| CatalogEntry::new((*p).clone()).expect("entry"))
        .collect();
    env.catalog.reset(entries, RawPairMode::PreferJpeg, None);
    let index = env.catalog.index_of(&jpeg);
    env.catalog.set_current(index);
    *env.hook.lock().expect("io hook") = Some(Box::new(|call| {
        if call == 2 {
            Err(std::io::Error::other("second member refused"))
        } else {
            Ok(())
        }
    }));
    let result = env.rename_to_end(&jpeg, "pic");
    assert_eq!(result.status, FileRenameStatus::Failed);
    assert_eq!(env.catalog.paths().to_vec(), [before, jpeg.clone(), after]);
    let group = env
        .catalog
        .current()
        .and_then(|entry| entry.capture_group.as_ref())
        .expect("the group is still there");
    assert_eq!(group.raw_path(), raw);
    assert!(env.exists(&jpeg), "the first member must have been rolled back");
    assert!(env.exists(&raw));
    assert!(!env.exists(&env.path("pic.jpg")));
    assert_eq!(env.sink.maps.len(), 1);
    assert!(env.sink.maps[0].is_empty(), "nothing stayed renamed: pure re-arm");
    assert!(env.undo.lock().expect("undo").last_undo_action_identity().is_none());
}

// C#: FileActionControllerRenameTests.RenameAsync_CatalogAlreadyListsTheNewName_DropsTheOldEntryAndPresentsTheCurrentOne
#[test]
fn rename_async_catalog_already_lists_the_new_name_drops_the_old_entry_and_presents_the_current_one() {
    let mut env = RenameEnv::new();
    let a = env.make("a.jpg");
    let b = env.make("b.jpg");
    let stale = env.path("x.jpg"); // listed by a stale scan but not on disk: the rename itself succeeds
    let entries = [&a, &b, &stale]
        .iter()
        .map(|p| CatalogEntry::new((*p).clone()).expect("entry"))
        .collect();
    env.catalog.reset(entries, RawPairMode::Separate, None);
    env.catalog.set_current(1);
    let result = env.rename_to_end(&b, "x");
    assert_eq!(result.status, FileRenameStatus::Renamed);
    assert_eq!(env.catalog.paths().to_vec(), [a, stale]);
    assert!(env.sink.removed.contains(&Some(b.clone())));
    assert!(
        !env.sink.presented.is_empty(),
        "listing conflict: present again even though the sink would keep the image"
    );
    assert_eq!(env.sink.maps.len(), 1);
    assert!(env.sink.maps[0].is_empty());
}
