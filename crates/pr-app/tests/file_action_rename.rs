//! FEAT-RENAME controller and undo parity checks (W9-R5).
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
    move_observations: Arc<Mutex<Vec<(i64, i64, usize)>>>,
}

impl RenameEnv {
    fn new() -> Self {
        let root = TempRoot::new("file-action-rename");
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
        let move_observations = Arc::new(Mutex::new(Vec::new()));
        let move_override = |label: &'static str| -> MoveOverride {
            let observations = move_observations.clone();
            let move_clock = clock.clone();
            let move_preload = preload.clone();
            let move_fs = fs_dyn.clone();
            let log = action_log.clone();
            Arc::new(move |source, destination| {
                observations.lock().expect("move observations").push((
                    move_clock.current_navigation(),
                    move_clock.current_folder(),
                    move_preload.cancels.load(std::sync::atomic::Ordering::SeqCst),
                ));
                log.lock().expect("ordered log").push(label.into());
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
            UndoService::new(journal, fs_dyn.clone(), bin, Arc::new(EnglishUndoTexts))
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
            Arc::new(FileActionServiceExecutor::new(service));
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
            log: Arc::new(RecordingLog::default()),
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
            move_observations,
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

    fn rename_done(&mut self, source: &str, name: &str) -> (FileRenameStatus, Option<String>) {
        let started = self.rename(source, name);
        let completed = if started == FileActionStep::Pending {
            self.deliver_worker()
        } else {
            started
        };
        done_rename(completed)
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

    fn undo(&mut self, folder: &str) -> FileActionStep {
        let start = {
            let mut cx = FileActionCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            self.controller.undo_last(&mut cx, Some(folder))
        };
        assert_eq!(start, FileActionStep::Pending);
        self.deliver_worker()
    }

    fn finish_present(&mut self) -> FileActionStep {
        let mut cx = FileActionCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.on_present_finished(&mut cx, false)
    }
}

fn done_rename(step: FileActionStep) -> (FileRenameStatus, Option<String>) {
    match step {
        FileActionStep::Done(FileActionDone::Renamed(result)) => (result.status, result.new_path),
        other => panic!("expected completed rename, got {other:?}"),
    }
}

// C#: FileActionControllerRenameTests.RenameAsync_EffectsRunInTheDocumentedOrder_ClockAndPreloadStoppedBeforeTheIo
#[test]
fn rename_async_effects_run_in_the_documented_order_clock_and_preload_stopped_before_the_io() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let source = paths[1].clone();
    let metadata = env.catalog.current().expect("shown entry").clone();
    let nav = env.clock.current_navigation();
    let folder = env.clock.current_folder();
    assert_eq!(env.rename(&source, "x"), FileActionStep::Pending);
    assert!(env.clock.current_navigation() > nav);
    assert_eq!(env.preload.cancels.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(env.worker.queued(), 1);
    assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
    let mut events = env.worker.run_all();
    assert_eq!(events.len(), 1);
    // The worker cannot borrow the UI catalog: it still has the old listing after the move,
    // until the event below applies the re-key before notifying the sink.
    assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
    let completed = {
        let mut cx = FileActionCx {
            catalog: &mut env.catalog,
            sink: &mut env.sink,
        };
        env.controller
            .handle_event(&mut cx, events.pop().expect("rename event"))
    };
    assert_eq!(
        completed,
        FileActionStep::Done(FileActionDone::Renamed(
            pr_app::coordinators::rename::FileRenameResult {
                status: FileRenameStatus::Renamed,
                new_path: Some(env.path("x.jpg")),
                problem: pr_core::file_actions::rename::RenameNameProblem::None,
                invalid_character: None,
                error: None,
            }
        ))
    );
    assert_eq!(env.catalog.paths().get(1), Some(&env.path("x.jpg")));
    let io = env.move_observations.lock().expect("I/O observations");
    assert_eq!(io.len(), 1);
    assert!(io[0].0 > nav, "navigation stopped before the actual move");
    assert!(io[0].1 > folder, "folder generation stopped before the actual move");
    assert_eq!(io[0].2, 1, "preload canceled before the actual move");
    assert_eq!(
        *env.sink.action_log.lock().expect("ordered log"),
        ["Cancel", "IO", "OnFilesRenamed", "Preload:1"]
    );
    assert_eq!(env.sink.effects, ["renamed", "status", "nav"]);
    assert_eq!(env.sink.maps[0].get(&source), Some(&env.path("x.jpg")));
    assert_eq!(
        env.sink.status.last().expect("status"),
        "Renamed b.jpg to x.jpg. Press Ctrl+Z to undo."
    );
    assert_eq!(*env.preload.preload_centers.lock().expect("preload"), [1]);
    let renamed = env.catalog.current().expect("re-keyed entry");
    assert_eq!(env.catalog.current_index(), 1);
    assert_eq!(
        (renamed.length, renamed.last_write_utc, renamed.width, renamed.height),
        (
            metadata.length,
            metadata.last_write_utc,
            metadata.width,
            metadata.height
        )
    );
}

// C#: FileActionControllerRenameTests.RenameAsync_SameName_IsNoChangeAndTouchesNothing
#[test]
fn rename_async_same_name_is_no_change_and_touches_nothing() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let generation = env.clock.current_navigation();
    let folder = env.clock.current_folder();
    let journal_path = AppPaths::new(&env.root.join("appdata"), None, false).journal_file;
    let journal_before = env.fs.file_content(&journal_path);
    assert_eq!(
        done_rename(env.rename(&paths[1], "b")),
        (FileRenameStatus::NoChange, Some(paths[1].clone()))
    );
    assert_eq!(env.clock.current_navigation(), generation);
    assert_eq!(env.clock.current_folder(), folder);
    assert_eq!(env.preload.cancels.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(env.worker.queued(), 0);
    assert!(env.sink.maps.is_empty());
    assert!(env.sink.action_log.lock().expect("ordered log").is_empty());
    assert!(env.move_observations.lock().expect("I/O observations").is_empty());
    assert_eq!(env.fs.file_content(&journal_path), journal_before);
    assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
    assert_eq!(env.sink.status.last().expect("status"), "b.jpg already has that name.");
}

// C#: FileActionControllerRenameTests.RenameAsync_FolderSwitchedDuringTheIo_RenameStaysUndoableAndTheNewFoldersStateIsUntouched
#[test]
fn rename_async_folder_switched_during_the_io_rename_stays_undoable_and_the_new_folders_state_is_untouched() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert_eq!(env.rename(&paths[1], "x"), FileActionStep::Pending);
    env.clock.next_folder();
    assert!(matches!(
        env.deliver_worker(),
        FileActionStep::Done(FileActionDone::Renamed(_))
    ));
    assert!(env.exists(&env.path("x.jpg")));
    assert!(!env.exists(&paths[1]));
    assert_eq!(env.catalog.paths().to_vec(), paths);
    assert!(env.sink.maps.is_empty());
    assert_eq!(env.sink.late.len(), 1);
    assert_eq!(env.sink.late[0], "Renamed b.jpg to x.jpg. Press Ctrl+Z to undo.");
    assert!(env.sink.status.is_empty());
    assert!(env.sink.presented.is_empty());
    assert!(
        env.undo
            .lock()
            .expect("undo lock")
            .last_undo_action_identity()
            .is_some()
    );
}

// C#: FileActionControllerRenameTests.RenameAsync_CaptureGroup_RespellsEveryMemberAndRegistersTheGroupForUndo
#[test]
fn rename_async_capture_group_respells_every_member_and_registers_the_group_for_undo() {
    let mut env = RenameEnv::new();
    let before = env.make("zbefore.jpg");
    let jpeg = env.make("shot.jpg");
    let raw = env.make("shot.cr2");
    let xmp = env.make("shot.xmp");
    let after = env.make("aafter.jpg");
    let entries = [&before, &jpeg, &raw, &after]
        .iter()
        .map(|p| CatalogEntry::new((*p).clone()).expect("entry"))
        .collect();
    env.catalog
        .reset(entries, RawPairMode::PreferJpeg, Some(std::slice::from_ref(&xmp)));
    let index = env.catalog.index_of(&jpeg);
    env.catalog.set_current(index);
    let (status, new_path) = env.rename_done(&jpeg, "pic");
    assert_eq!(status, FileRenameStatus::Renamed);
    assert_eq!(new_path, Some(env.path("pic.jpg")));
    assert!(env.exists(&env.path("pic.jpg")));
    assert!(env.exists(&env.path("pic.cr2")));
    assert!(env.exists(&env.path("pic.xmp")));
    assert!(!env.exists(&jpeg) && !env.exists(&raw) && !env.exists(&xmp));
    assert_eq!(env.catalog.current_index(), index);
    let map = &env.sink.maps[0];
    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&jpeg), Some(&env.path("pic.jpg")));
    assert_eq!(map.get(&raw), Some(&env.path("pic.cr2")));
    assert_eq!(map.get(&xmp), Some(&env.path("pic.xmp")));
    assert!(env.undo.lock().expect("undo").last_undo_action_identity().is_some());
    assert!(env.sink.presented.is_empty());
    assert_eq!(env.catalog.paths().to_vec(), [before, env.path("pic.jpg"), after]);
    let renamed_group = env
        .catalog
        .current()
        .and_then(|entry| entry.capture_group.as_ref())
        .expect("renamed capture group");
    assert_eq!(renamed_group.jpeg_path(), env.path("pic.jpg"));
    assert_eq!(renamed_group.raw_path(), env.path("pic.cr2"));
    assert_eq!(renamed_group.xmp_path(), Some(env.path("pic.xmp").as_str()));
}

// C#: FileActionControllerRenameUndoTests.UndoLast_AfterSingleRename_RekeysBackInPlaceWithoutInsertingOrPresenting
#[test]
fn undo_last_after_single_rename_rekeys_back_in_place_without_inserting_or_presenting() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert_eq!(env.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
    let current = env.path("x.jpg");
    let folder = env.root.path().to_string_lossy().into_owned();
    assert!(matches!(
        env.undo(&folder),
        FileActionStep::Done(FileActionDone::Undone(Some(_)))
    ));
    assert_eq!(env.catalog.paths().to_vec(), paths);
    assert_eq!(env.catalog.current_index(), 1);
    assert!(env.exists(&paths[1]));
    assert!(!env.exists(&current));
    assert!(env.sink.presented.is_empty());
    assert_eq!(env.sink.maps.last().and_then(|m| m.get(&current)), Some(&paths[1]));
}

// C#: FileActionControllerRenameUndoTests.UndoLast_ClockAndPreloadAreStoppedBeforeTheUndoIo
#[test]
fn undo_last_clock_and_preload_are_stopped_before_the_undo_io() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert_eq!(env.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
    let navigation = env.clock.current_navigation();
    env.move_observations.lock().expect("move observations").clear();
    let folder = env.root.path().to_string_lossy().into_owned();
    let _ = env.undo(&folder);
    assert!(env.clock.current_navigation() > navigation);
    assert_eq!(env.preload.cancels.load(std::sync::atomic::Ordering::SeqCst), 2);
    let observations = env.move_observations.lock().expect("move observations");
    let (io_navigation, _, cancel_count) = observations.last().expect("undo move observed");
    assert!(*io_navigation > navigation);
    assert_eq!(*cancel_count, 2);
}

// C#: FileActionControllerRenameUndoTests.UndoLast_SinkCannotKeepShownImage_PresentsTheCurrentIndex
#[test]
fn undo_last_sink_cannot_keep_shown_image_presents_the_current_index() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert_eq!(env.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
    env.sink.keep = false;
    let folder = env.root.path().to_string_lossy().into_owned();
    env.sink.effects.clear();
    assert_eq!(env.undo(&folder), FileActionStep::Pending);
    assert_eq!(env.sink.presented, [1]);
    assert_eq!(env.sink.effects, ["renamed", "present"]);
    assert_eq!(env.catalog.paths().as_ref(), paths.as_slice());
    assert!(
        matches!(env.finish_present(), FileActionStep::Done(FileActionDone::Undone(Some(result))) if result.succeeded && result.source == paths[1])
    );
    assert_eq!(env.sink.effects, ["renamed", "present", "status", "nav"]);
    assert_eq!(
        env.sink.status.last().expect("undo status"),
        "Rename undone: the file is b.jpg again."
    );
}

// C#: FileActionControllerRenameUndoTests.UndoLast_AfterCaptureGroupRename_UsesTheRememberedNamesForEveryMember
#[test]
fn undo_last_after_capture_group_rename_uses_remembered_names_for_every_member() {
    let mut env = RenameEnv::new();
    let before = env.make("zbefore.jpg");
    let jpeg = env.make("shot.jpg");
    let raw = env.make("shot.cr2");
    let xmp = env.make("shot.xmp");
    let after = env.make("aafter.jpg");
    let entries = [&before, &jpeg, &raw, &after]
        .iter()
        .map(|p| CatalogEntry::new((*p).clone()).expect("entry"))
        .collect();
    env.catalog
        .reset(entries, RawPairMode::PreferJpeg, Some(std::slice::from_ref(&xmp)));
    env.catalog.set_current(env.catalog.index_of(&jpeg));
    assert_eq!(env.rename_done(&jpeg, "pic").0, FileRenameStatus::Renamed);
    let folder = env.root.path().to_string_lossy().into_owned();
    let _ = env.undo(&folder);
    assert_eq!(env.catalog.paths().to_vec(), [before, jpeg.clone(), after]);
    assert!(env.exists(&raw) && env.exists(&xmp));
    let restored_group = env
        .catalog
        .current()
        .and_then(|entry| entry.capture_group.as_ref())
        .expect("restored capture group");
    assert_eq!(restored_group.jpeg_path(), jpeg);
    assert_eq!(restored_group.raw_path(), raw);
    assert_eq!(restored_group.xmp_path(), Some(xmp.as_str()));
    let undo_map = env.sink.maps.last().expect("rename undo map");
    assert_eq!(undo_map.get(&env.path("pic.jpg")), Some(&jpeg));
    assert_eq!(undo_map.get(&env.path("pic.cr2")), Some(&raw));
    assert_eq!(undo_map.get(&env.path("pic.xmp")), Some(&xmp));
}

// C#: FileActionControllerRenameUndoTests.UndoLast_AfterAMoveRegisteredAfterTheRename_IsTheMovesUndoAndTheRenameStays
#[test]
fn undo_last_after_a_move_registered_after_the_rename_is_the_moves_undo_and_the_rename_stays() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert_eq!(env.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
    let before = env
        .undo
        .lock()
        .expect("undo lock")
        .last_undo_action_identity()
        .expect("rename slot");
    let renamed_path = env.path("x.jpg");
    let mut cx = FileActionCx {
        catalog: &mut env.catalog,
        sink: &mut env.sink,
    };
    assert_eq!(
        env.controller.run_action(&mut cx, 0, None, Some(&renamed_path)),
        FileActionStep::Pending
    );
    let _ = env.deliver_worker();
    let after = env
        .undo
        .lock()
        .expect("undo lock")
        .last_undo_action_identity()
        .expect("move slot");
    assert_ne!(before, after);
    let folder = env.root.path().to_string_lossy().into_owned();
    let _ = env.undo(&folder);
    assert!(env.exists(&env.path("x.jpg")));
    assert!(!env.exists(&paths[1]));
    let second = env.undo(&folder);
    assert!(matches!(second, FileActionStep::Done(FileActionDone::Undone(Some(result))) if !result.succeeded));
    assert!(env.exists(&env.path("x.jpg")));
    assert!(!env.exists(&paths[1]));
}

// C#: FileActionControllerRenameUndoTests.UndoLast_RenameAfterAMove_ReplacesTheMovesUndo
#[test]
fn undo_last_rename_after_a_move_replaces_the_moves_undo() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let mut cx = FileActionCx {
        catalog: &mut env.catalog,
        sink: &mut env.sink,
    };
    assert_eq!(
        env.controller.run_action(&mut cx, 0, None, Some(&paths[0])),
        FileActionStep::Pending
    );
    let _ = env.deliver_worker();
    let move_slot = env
        .undo
        .lock()
        .expect("undo lock")
        .last_undo_action_identity()
        .expect("move slot");
    assert_eq!(env.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
    let rename_slot = env
        .undo
        .lock()
        .expect("undo lock")
        .last_undo_action_identity()
        .expect("rename slot");
    assert_ne!(move_slot, rename_slot);
    let folder = env.root.path().to_string_lossy().into_owned();
    let _ = env.undo(&folder);
    assert!(env.exists(&paths[1]));
    assert!(!env.exists(&env.path("x.jpg")));
    let second = env.undo(&folder);
    assert!(matches!(second, FileActionStep::Done(FileActionDone::Undone(Some(result))) if !result.succeeded));
}

// C#: FileActionControllerRenameUndoTests.UndoLast_UndoIoFails_ReArmsWithAnEmptyMapAndShowsTheError
#[test]
fn undo_last_undo_io_fails_rearms_with_an_empty_map_and_shows_the_error() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert_eq!(env.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
    // A missing destination makes the core undo report a failure while preserving catalog state.
    env.fs
        .delete(Path::new(&env.path("x.jpg")))
        .expect("remove only fake fixture");
    let folder = env.root.path().to_string_lossy().into_owned();
    let step = env.undo(&folder);
    assert!(matches!(step, FileActionStep::Done(FileActionDone::Undone(Some(result))) if !result.succeeded));
    assert!(env.sink.maps.last().is_some_and(PathKeyMap::is_empty));
    assert_eq!(env.catalog.index_of(&env.path("x.jpg")), 1);
}

// C#: FileActionControllerRenameUndoTests.UndoLast_RenameMadeInAnotherFolder_FallsThroughToTheGenericPath
#[test]
fn undo_last_rename_made_in_another_folder_falls_through_to_the_generic_path() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert_eq!(env.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
    let renamed = env.path("x.jpg");
    let other = env.root.join("elsewhere");
    env.fs.add_dir(&other);
    let other_folder = other.to_string_lossy().into_owned();
    assert!(matches!(
        env.undo(&other_folder),
        FileActionStep::Done(FileActionDone::Undone(Some(_)))
    ));
    assert!(env.exists(&paths[1]));
    assert!(env.catalog.index_of(&renamed) >= 0);
    assert_eq!(env.sink.maps.len(), 1); // rename completion only; generic restore leaves this folder's listing alone
}

#[test]
fn undo_identity_distinguishes_equal_registrations_and_retains_the_old_record() {
    // Integration regression for C# ReferenceEquals(LastUndoAction, _lastRenameUndoAction).
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg"], 0);
    let renamed = env.rename_done(&paths[0], "x").1.expect("renamed path");
    let stat = env.fs.get_file_stat(Path::new(&renamed)).expect("renamed file");
    let result = pr_core::file_actions::types::FileActionResult::new(
        true,
        FileOperationType::Move,
        paths[0].clone(),
        Some(renamed),
        i64::try_from(stat.length).expect("small fixture"),
        stat.last_write_utc,
        None,
    );
    let mut undo = env.undo.lock().expect("undo");
    undo.register(&result);
    let first = undo.last_undo_action_identity().expect("first record");
    assert_eq!(first, first.clone());
    undo.register(&result);
    let second = undo.last_undo_action_identity().expect("equal-valued new record");
    assert_ne!(first, second, "same values must not imply the same Undo slot");
    assert!(undo.undo_last().succeeded);
    assert!(undo.last_undo_action_identity().is_none());
    assert_eq!(
        first,
        first.clone(),
        "retained token keeps its original identity after clearing"
    );
    assert_ne!(first, second);
}

#[test]
fn rename_present_completed_after_folder_switch_keeps_the_new_folder_status() {
    // Integration regression for RenameAsync's post-present R08 guard and finally notification.
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg"], 0);
    env.sink.keep = false;
    assert_eq!(env.rename(&paths[0], "x"), FileActionStep::Pending);
    assert_eq!(env.deliver_worker(), FileActionStep::Pending);
    assert_eq!(env.catalog.paths().as_ref(), &[env.path("x.jpg")]);
    env.clock.next_folder();
    let new_paths = env.load(&["elsewhere/new.jpg"], 0);
    env.sink.status.push("new folder status".into());
    let nav = env.sink.nav;
    assert_eq!(
        done_rename(env.finish_present()),
        (FileRenameStatus::Renamed, Some(env.path("x.jpg")))
    );
    assert_eq!(env.catalog.paths().as_ref(), new_paths.as_slice());
    assert_eq!(env.sink.status.last().expect("status"), "new folder status");
    assert_eq!(env.sink.nav, nav + 1);
}

#[test]
fn rename_undo_present_completed_after_folder_switch_returns_undo_without_new_folder_effects() {
    // Integration regression for TryHandleRenameUndoAsync's stale present continuation.
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg"], 0);
    assert_eq!(env.rename_done(&paths[0], "x").0, FileRenameStatus::Renamed);
    env.sink.keep = false;
    let folder = env.root.path().to_string_lossy().into_owned();
    assert_eq!(env.undo(&folder), FileActionStep::Pending);
    env.clock.next_folder();
    let new_paths = env.load(&["elsewhere/new.jpg"], 0);
    env.sink.status.push("new folder status".into());
    let nav = env.sink.nav;
    assert!(
        matches!(env.finish_present(), FileActionStep::Done(FileActionDone::Undone(Some(result))) if result.succeeded)
    );
    assert_eq!(env.catalog.paths().as_ref(), new_paths.as_slice());
    assert_eq!(env.sink.status.last().expect("status"), "new folder status");
    assert_eq!(env.sink.nav, nav);
}
