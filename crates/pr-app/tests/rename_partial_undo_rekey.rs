#![allow(dead_code)]
//! W9-rename-undo-parity (PINNED behaviour): a PARTIAL failed undo of a renamed capture group followed by a successful
//! retry. The re-armed undo keeps its identity (Rust `Arc::ptr_eq`, C# `ReferenceEquals`), so the retry IS handled as a
//! rename undo in both ports; the files come back on disk. But the sidecar re-key map is `{pic.xmp -> shot.xmp}` and
//! `ReviewCatalog.RenameEntries` locates entries only through `IndexOf(key)`, which indexes the entry path and
//! `CaptureGroup.ImagePaths` (jpeg, raw) and never the xmp. No entry is found, `RenameEntries` returns false and
//! `TryHandleRenameUndoAsync` clears the map (`undone.Clear()`), so the catalog keeps the stale sidecar spelling in
//! BOTH the C# app (ReviewCatalog.cs lines 553-590, FileActionController.cs lines 756-766 at 7eaee2a2) and this port.
//! This file pins that identical behaviour; it is not an endorsement of it. Fakes only.
mod file_action_fakes;

use std::path::Path;
use std::sync::{Arc, Mutex};

use file_action_fakes::{EnglishTexts, ManualPort, RecordingLog, RecordingPreload};
use pr_app::coordinators::file_actions::recycle::{
    FileActionController, FileActionCx, FileActionDeps, FileActionDone, FileActionStep,
};
use pr_app::coordinators::rename::FileRenameStatus;
use pr_app::coordinators::traits::{
    FileActionServiceExecutor, IFileActionExecutor, IFileActionSink, IPreloadController,
};
use pr_core::abstractions::{Clock, FileSystem, RecycleBin};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::natural_cmp::ManagedNaturalComparer;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CatalogEntry;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::rename::PathKeyMap;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::{CaptureGroupActionResult, FileActionResult};
use pr_core::file_actions::undo::{EnglishUndoTexts, FileActionGate, MoveOverride, UndoService};
use pr_core::model::{FileOperationType, RawPairMode};
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::shortcuts::ReviewAction;
use pr_core::sync::CancelToken;
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

type SingleTweak = Box<dyn Fn(&str) -> FileActionResult + Send>;
type GroupTweak = Box<dyn Fn() -> CaptureGroupActionResult + Send>;

/// Scripted replacement of the worker result of a rename (the real service is not run when a tweak is armed).
#[derive(Default)]
struct Tweaks {
    panic_rename: std::sync::atomic::AtomicBool,
    single: Mutex<Option<SingleTweak>>,
    group: Mutex<Option<GroupTweak>>,
}

struct TweakExec {
    inner: FileActionServiceExecutor,
    tweaks: Arc<Tweaks>,
}

impl IFileActionExecutor for TweakExec {
    fn is_busy(&self) -> bool {
        self.inner.is_busy()
    }
    fn lacks_recycle_bin(&self, path: &str) -> bool {
        self.inner.lacks_recycle_bin(path)
    }
    fn execute(
        &self,
        request: &pr_core::file_actions::types::FileActionRequest,
        cancel: &CancelToken,
    ) -> FileActionResult {
        self.inner.execute(request, cancel)
    }
    fn execute_group(
        &self,
        request: &pr_core::file_actions::types::CaptureGroupActionRequest,
        cancel: &CancelToken,
    ) -> CaptureGroupActionResult {
        self.inner.execute_group(request, cancel)
    }
    fn rename(&self, request: &pr_core::file_actions::rename::RenameRequest, cancel: &CancelToken) -> FileActionResult {
        if self.tweaks.panic_rename.load(std::sync::atomic::Ordering::SeqCst) {
            panic!("scripted rename executor panic");
        }
        if let Some(tweak) = self.tweaks.single.lock().expect("tweak").as_ref() {
            return tweak(&request.source);
        }
        self.inner.rename(request, cancel)
    }
    fn rename_group(
        &self,
        group: &pr_core::catalog::types::CaptureGroup,
        typed_stem: &str,
        cancel: &CancelToken,
    ) -> CaptureGroupActionResult {
        if let Some(tweak) = self.tweaks.group.lock().expect("tweak").as_ref() {
            return tweak();
        }
        self.inner.rename_group(group, typed_stem, cancel)
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
    tweaks: Arc<Tweaks>,
    undo_fail_at: Arc<std::sync::atomic::AtomicUsize>,
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
        let undo_moves = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let undo_fail_at = Arc::new(std::sync::atomic::AtomicUsize::new(usize::MAX));
        let journal = Arc::new(OperationJournal::new(&app, fs_dyn.clone(), fake_clock.clone()));
        let bin: Arc<dyn RecycleBin> = Arc::new(FakeRecycleBin::new(fs.clone()));
        let service = Arc::new(
            FileActionService::new(journal.clone(), fs_dyn.clone(), fake_clock.clone(), bin.clone())
                .with_move_override(move_override("IO")),
        );
        let undo = Arc::new(Mutex::new(
            UndoService::new(journal, fs_dyn.clone(), bin, Arc::new(EnglishUndoTexts))
                .with_move_override({
                    let inner = move_override("UndoIO");
                    let calls = undo_moves.clone();
                    let fail_at = undo_fail_at.clone();
                    Arc::new(move |source: &Path, destination: &Path| {
                        let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if call == fail_at.load(std::sync::atomic::Ordering::SeqCst) {
                            return Err(std::io::Error::other("scripted undo move failure"));
                        }
                        inner(source, destination)
                    })
                })
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
        let tweaks = Arc::new(Tweaks::default());
        let executor: Arc<dyn pr_app::coordinators::traits::IFileActionExecutor + Send + Sync> = Arc::new(TweakExec {
            inner: FileActionServiceExecutor::new(service),
            tweaks: tweaks.clone(),
        });
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
            tweaks,
            undo_fail_at,
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

fn scripted_single(source: &str, succeeded: bool, destination: Option<String>) -> FileActionResult {
    FileActionResult::new(
        succeeded,
        FileOperationType::Move,
        source,
        destination,
        0,
        pr_core::time::DateTimeUtc::MIN,
        None,
    )
}

fn arm_single(env: &RenameEnv, build: impl Fn(&str) -> FileActionResult + Send + 'static) {
    *env.tweaks.single.lock().expect("tweak") = Some(Box::new(build));
}

fn identity_registered(env: &RenameEnv) -> bool {
    env.undo
        .lock()
        .expect("undo lock")
        .last_undo_action_identity()
        .is_some()
}

fn load_capture_group(env: &mut RenameEnv) -> (String, String, String, String) {
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
    (before, jpeg, raw, xmp)
}

fn xmp_of_current(env: &RenameEnv) -> Option<String> {
    env.catalog
        .current()
        .and_then(|entry| entry.capture_group.as_ref())
        .and_then(|group| group.xmp_path().map(str::to_owned))
}

fn jpeg_of_current(env: &RenameEnv) -> Option<String> {
    env.catalog.current().map(|entry| entry.path().to_owned())
}

// C#: no direct twin (FileActionControllerRenameUndoTests covers the single-file undo). Behaviour comes from
// FileActionController.UndoLastAsync (lines 880-894), TryHandleRenameUndoAsync (726-780), ReviewCatalog.RenameEntries
// (553-590) and UndoService.UndoGroupMoveAsync, which keeps the same `_lastUndoAction` object after a partial failure.
#[test]
fn rename_group_partial_undo_then_retry_keeps_the_stale_sidecar_key_like_csharp() {
    let mut env = RenameEnv::new();
    let (_before, jpeg, raw, xmp) = load_capture_group(&mut env);
    let folder = env.path("");
    assert_eq!(env.rename_done(&jpeg, "pic").0, FileRenameStatus::Renamed);
    assert_eq!(
        xmp_of_current(&env),
        Some(env.path("pic.xmp")),
        "renamed sidecar is keyed by its new name"
    );

    // First undo: the third member move (the sidecar) fails after jpg and cr2 are back.
    env.undo_fail_at.store(2, std::sync::atomic::Ordering::SeqCst);
    let first = env.undo(&folder);
    let FileActionStep::Done(FileActionDone::Undone(Some(first))) = first else {
        panic!("expected a finished undo, got {first:?}");
    };
    assert!(!first.succeeded, "the scripted failure makes the first undo fail");
    assert!(env.exists(&jpeg) && env.exists(&raw), "jpg and cr2 are back");
    assert!(
        !env.exists(&xmp) && env.exists(&env.path("pic.xmp")),
        "the sidecar is still renamed"
    );
    assert_eq!(
        jpeg_of_current(&env),
        Some(jpeg.clone()),
        "the restored members are re-keyed by the first undo"
    );
    assert_eq!(
        env.sink.maps.last().map(PathKeyMap::len),
        Some(2),
        "first undo re-keys jpg and cr2 only"
    );

    // Second undo succeeds and puts the sidecar back on disk.
    env.undo_fail_at.store(usize::MAX, std::sync::atomic::Ordering::SeqCst);
    let second = env.undo(&folder);
    let FileActionStep::Done(FileActionDone::Undone(Some(second))) = second else {
        panic!("expected a finished undo, got {second:?}");
    };
    assert!(second.succeeded, "{second:?}");
    assert!(
        env.exists(&xmp) && !env.exists(&env.path("pic.xmp")),
        "the sidecar is back on disk"
    );

    // Pinned (same as C#): no catalog entry lists the xmp, RenameEntries returns false, the map is cleared.
    assert!(
        env.sink.maps.last().is_some_and(PathKeyMap::is_empty),
        "the sink hears an empty re-key map on the retry"
    );
    assert_eq!(
        xmp_of_current(&env),
        Some(env.path("pic.xmp")),
        "PINNED C# parity: the capture group keeps the stale sidecar key until the folder is reloaded"
    );
    assert_eq!(
        jpeg_of_current(&env),
        Some(jpeg),
        "the representative stays under its restored name"
    );
}
