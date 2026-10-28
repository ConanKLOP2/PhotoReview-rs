#![allow(dead_code)]
//! W9-RENAME-MUTATION-2 killers: tests added for cargo-mutants survivors of the rename coordinator path
//! (`file_actions/actions.rs` rename functions, `viewmodels/main/rename.rs`, `presenters/rename_prompt.rs`).
//! Each test names the mutant it kills. Everything runs on fakes (FakeFileSystem, FakeRecycleBin, TempRoot paths).
mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

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

// Mutant: actions.rs `on_renamed` 325:73 `&& -> ||` (Panicked path warms preload for a stale folder).
#[test]
fn on_renamed_executor_panic_after_folder_switch_does_not_warm_preload_of_the_new_folder() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    env.tweaks.panic_rename.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(env.rename(&paths[1], "x"), FileActionStep::Pending);
    env.clock.next_folder();
    let step = env.deliver_worker();
    assert!(matches!(
        step,
        FileActionStep::Done(FileActionDone::Renamed(ref r)) if r.status == FileRenameStatus::Failed
    ));
    assert!(
        env.preload.preload_centers.lock().expect("centers").is_empty(),
        "a stale folder must not start preloading"
    );
    assert!(env.sink.status.is_empty(), "a stale folder gets no status text");
    assert_eq!(env.catalog.paths().to_vec(), paths);
}

// Mutant: actions.rs `on_renamed` 325:95 `> -> >=` (Panicked path with an empty catalog warms preload at index 0),
// plus the positive control for the `&&` (current folder and non-empty catalog warm around the current index).
#[test]
fn on_renamed_executor_panic_warms_preload_only_for_a_non_empty_current_catalog() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    env.tweaks.panic_rename.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(env.rename(&paths[1], "x"), FileActionStep::Pending);
    assert!(matches!(
        env.deliver_worker(),
        FileActionStep::Done(FileActionDone::Renamed(_))
    ));
    assert_eq!(env.preload.preload_centers.lock().expect("centers").as_slice(), [1]);

    let mut empty = RenameEnv::new();
    let paths = empty.load(&["a.jpg", "b.jpg"], 0);
    empty
        .tweaks
        .panic_rename
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(empty.rename(&paths[0], "x"), FileActionStep::Pending);
    empty.catalog.reset(Vec::new(), RawPairMode::PreferJpeg, None);
    assert!(matches!(
        empty.deliver_worker(),
        FileActionStep::Done(FileActionDone::Renamed(_))
    ));
    assert!(empty.preload.preload_centers.lock().expect("centers").is_empty());
    assert!(empty.sink.presented.is_empty());
}

// Mutant: actions.rs `on_renamed` 362:26 `&& -> ||` (stale folder, success without any renamed path is registered
// for undo and announced as a rename).
#[test]
fn on_renamed_stale_folder_success_without_renamed_paths_is_no_change_and_not_registered() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    arm_single(&env, |source| {
        let mut result = scripted_single(source, true, Some(source.to_owned()));
        result.no_change = true;
        result
    });
    assert_eq!(env.rename(&paths[1], "x"), FileActionStep::Pending);
    env.clock.next_folder();
    let step = env.deliver_worker();
    let FileActionStep::Done(FileActionDone::Renamed(result)) = step else {
        panic!("expected a completed rename");
    };
    assert_eq!(result.status, FileRenameStatus::NoChange);
    assert_eq!(result.new_path, None);
    assert!(env.sink.late.is_empty(), "nothing was renamed, so no late status");
    assert!(!identity_registered(&env), "a no-change result is not undoable");
}

// Mutant: actions.rs `on_renamed` 420:22 `&& -> ||` (a FAILED rename that left the file under a new name is
// registered for undo): the catalog follows the file, but no undo is armed for a failure.
#[test]
fn on_renamed_failed_result_with_a_moved_source_rekeys_the_catalog_but_registers_no_undo() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    let moved = env.path("x.jpg");
    arm_single(&env, {
        let moved = moved.clone();
        move |source| {
            let mut result = scripted_single(source, false, Some(moved.clone()));
            result.source_removed = true;
            result.error = Some("size differs".into());
            result
        }
    });
    let (status, new_path) = env.rename_done(&paths[1], "x");
    assert_eq!(status, FileRenameStatus::Failed);
    assert_eq!(new_path, None);
    assert_eq!(env.sink.maps.len(), 1);
    assert_eq!(env.sink.maps[0].get(&paths[1]), Some(&moved));
    assert_eq!(env.catalog.index_of(&moved), 1, "the catalog follows the file on disk");
    assert!(!identity_registered(&env), "a failed rename must not arm undo");
}

// Mutants: actions.rs `on_renamed` 426:13 `|| -> &&` and 420:22 (second half: a success without renamed paths
// is NoChange, shows the unchanged status and registers no undo).
#[test]
fn on_renamed_current_folder_success_flagged_no_change_reports_no_change_and_registers_no_undo() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    arm_single(&env, |source| {
        let mut result = scripted_single(source, true, Some(source.to_owned()));
        result.no_change = true;
        result
    });
    let (status, new_path) = env.rename_done(&paths[1], "x");
    assert_eq!(status, FileRenameStatus::NoChange);
    assert_eq!(new_path, Some(paths[1].clone()));
    assert!(env.sink.status.last().is_some_and(|text| text.contains("b.jpg")));
    assert!(!identity_registered(&env));
    assert!(env.sink.maps.last().is_some_and(PathKeyMap::is_empty));
}

// Mutant: actions.rs `on_renamed` 426:98 `delete !` (`&& !conflict`): a group rename that succeeds without
// re-spelling any member is NoChange (the conflict-free branch).
#[test]
fn on_renamed_group_success_without_renamed_members_is_no_change() {
    let mut env = RenameEnv::new();
    let (_before, jpeg, _raw, _xmp) = load_capture_group(&mut env);
    *env.tweaks.group.lock().expect("tweak") = Some(Box::new(|| {
        CaptureGroupActionResult::new(true, false, FileOperationType::Move, "group", None, Vec::new(), None)
    }));
    let (status, new_path) = env.rename_done(&jpeg, "pic");
    assert_eq!(status, FileRenameStatus::NoChange);
    assert_eq!(new_path, Some(jpeg));
    assert!(!identity_registered(&env));
}

// Mutant: actions.rs `on_renamed` 454:31 `> -> >=` (an empty catalog must not present or park the tail).
#[test]
fn on_renamed_with_an_empty_catalog_completes_without_presenting() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg"], 0);
    assert_eq!(env.rename(&paths[0], "x"), FileActionStep::Pending);
    env.catalog.reset(Vec::new(), RawPairMode::PreferJpeg, None);
    env.sink.keep = false;
    let step = env.deliver_worker();
    assert!(
        matches!(step, FileActionStep::Done(FileActionDone::Renamed(_))),
        "an empty catalog has nothing to present: {step:?}"
    );
    assert!(env.sink.presented.is_empty());
    assert!(env.preload.preload_centers.lock().expect("centers").is_empty());
}

// Mutant: actions.rs `apply_rename_undo` 597:31 `> -> >=` (an empty catalog must not present or preload after undo).
#[test]
fn apply_rename_undo_with_an_empty_catalog_does_not_present() {
    let mut env = RenameEnv::new();
    let paths = env.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
    assert_eq!(env.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
    env.catalog.reset(Vec::new(), RawPairMode::PreferJpeg, None);
    env.sink.keep = false;
    let centers_before = env.preload.preload_centers.lock().expect("centers").len();
    let folder = env.root.path().to_string_lossy().into_owned();
    let _ = env.undo(&folder);
    assert!(env.sink.presented.is_empty(), "nothing to present in an empty catalog");
    assert_eq!(
        env.preload.preload_centers.lock().expect("centers").len(),
        centers_before
    );
}

// Mutant: actions.rs `apply_rename_undo` 583:31 `&& -> ||` (a failed partial undo reported for ANOTHER folder
// reaches the sink but must not re-key this catalog).
#[test]
fn apply_rename_undo_partial_group_undo_in_another_folder_does_not_rekey_this_catalog() {
    let mut env = RenameEnv::new();
    let (_before, jpeg, _raw, _xmp) = load_capture_group(&mut env);
    assert_eq!(env.rename_done(&jpeg, "pic").0, FileRenameStatus::Renamed);
    let listing = env.catalog.paths().to_vec();
    let other_folder = env.path("elsewhere");
    env.undo_fail_at.store(2, std::sync::atomic::Ordering::SeqCst);
    let step = env.undo(&other_folder);
    assert!(
        matches!(step, FileActionStep::Done(FileActionDone::Undone(_))),
        "{step:?}"
    );
    let map = env.sink.maps.last().expect("undo map");
    assert_eq!(
        map.get(&env.path("pic.jpg")),
        Some(&jpeg),
        "the sink hears about the restored members"
    );
    assert_eq!(
        env.catalog.paths().to_vec(),
        listing,
        "this folder's catalog keeps the new names"
    );
}

// Mutant: presenters/rename_prompt.rs `<impl Debug for PreparedRenamePrompt>::fmt -> Ok(Default::default())`.
#[test]
fn prepared_rename_prompt_debug_names_the_type_and_the_source() {
    let check: pr_app::presenters::rename_prompt::RenameCheck =
        Arc::new(|typed| pr_core::file_actions::rename::RenameNamePolicy::check(Some(typed), "C:/r/a.jpg", false));
    let prompt = pr_app::presenters::rename_prompt::PreparedRenamePrompt::new("C:/r/a.jpg".to_owned(), check);
    let text = format!("{prompt:?}");
    assert!(text.contains("PreparedRenamePrompt"), "{text}");
    assert!(text.contains("a.jpg"), "{text}");
}

// Mutant: presenters/rename_prompt.rs `<impl Debug for RenamePromptPresenter>::fmt -> Ok(Default::default())`.
#[test]
fn rename_prompt_presenter_debug_names_the_type_and_the_source() {
    let check: pr_app::presenters::rename_prompt::RenameCheck =
        Arc::new(|typed| pr_core::file_actions::rename::RenameNamePolicy::check(Some(typed), "C:/r/b.jpg", false));
    let presenter = pr_app::presenters::rename_prompt::RenamePromptPresenter::new("C:/r/b.jpg".to_owned(), check, None)
        .expect("non-blank source");
    let text = format!("{presenter:?}");
    assert!(text.contains("RenamePromptPresenter"), "{text}");
    assert!(text.contains("b.jpg"), "{text}");
}

// Mutant: viewmodels/main/rename.rs `MainViewModel::on_files_renamed -> Default::default()` (the public wrapper must
// return the change set of the re-key, not an empty one).
#[test]
fn on_files_renamed_returns_the_catalog_changed_change_set_of_the_rekey() {
    let mut env = support::main_env::MainEnv::new();
    let (folder, files) = env.images("killer_rekey", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    let mut renames = PathKeyMap::new();
    renames.insert(
        files[0].clone(),
        Path::new(&folder).join("z.jpg").to_string_lossy().into_owned(),
    );
    let changes = env.vm.on_files_renamed(&renames);
    assert!(changes.catalog_changed, "a non-empty re-key reports CatalogChanged");
}

// Mutant: viewmodels/main/rename.rs:60 `delete !` in `apply_files_renamed` (an empty map must not report
// CatalogChanged).
#[test]
fn on_files_renamed_with_an_empty_map_reports_no_catalog_change() {
    let mut env = support::main_env::MainEnv::new();
    let (folder, files) = env.images("killer_rekey_empty", &["a.jpg", "b.jpg"]);
    env.open_at(&folder, &files[0]);
    let changes = env.vm.on_files_renamed(&PathKeyMap::new());
    assert!(!changes.catalog_changed, "an empty re-key changes nothing");
}
