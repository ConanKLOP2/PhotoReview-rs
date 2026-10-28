#![allow(dead_code)]
//! W9-RENAME-MUTATION-3 killers for cargo-mutants survivors in `coordinators/file_actions/undo.rs`,
//! `file_actions/actions.rs` (non-rename part) and `coordinators/presenter/rename.rs`.
//! Each test names the mutant it kills. Everything runs on fakes (FakeFileSystem, FakeRecycleBin, InertBin, TempRoot paths);
//! no test touches the real Recycle Bin.
mod file_action_fakes;

use std::path::Path;
use std::sync::{Arc, Mutex};

use file_action_fakes::{
    EnglishTexts, Env, ManualPort, RecordingLog, RecordingPreload, Scripted, action, group_failed, group_ok, member, p,
    single_ok,
};
use pr_app::coordinators::file_actions::recycle::{
    FileActionController, FileActionCx, FileActionDeps, FileActionDone, FileActionEvent, FileActionStep, UndoOutcome,
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
use pr_core::file_actions::types::{CaptureGroupActionResult, FileActionResult, UndoResult};
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

impl RenameEnv {
    /// Starts an undo, lets the real worker job run (it undoes the rename on the fake disk) and delivers `scripted`
    /// instead of its result, so the controller's handling of every `UndoResult` shape can be pinned.
    fn undo_scripted(&mut self, folder: &str, scripted: pr_core::file_actions::types::UndoResult) -> FileActionStep {
        let start = {
            let mut cx = FileActionCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            self.controller.undo_last(&mut cx, Some(folder))
        };
        assert_eq!(start, FileActionStep::Pending);
        let ticket = self
            .worker
            .run_all()
            .into_iter()
            .find_map(|event| match event {
                FileActionEvent::Undone { ticket, .. } => Some(ticket),
                _ => None,
            })
            .expect("an Undone event");
        let mut cx = FileActionCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.handle_event(
            &mut cx,
            FileActionEvent::Undone {
                ticket,
                outcome: UndoOutcome::Result(scripted),
            },
        )
    }

    /// Renames b.jpg of a.jpg/b.jpg/c.jpg to x.jpg (armed for undo); returns (old path, new path, folder).
    fn renamed_single(&mut self) -> (String, String, String) {
        let paths = self.load(&["a.jpg", "b.jpg", "c.jpg"], 1);
        assert_eq!(self.rename_done(&paths[1], "x").0, FileRenameStatus::Renamed);
        let folder = self.root.path().to_string_lossy().into_owned();
        (paths[1].clone(), self.path("x.jpg"), folder)
    }
}

fn rename_undo_result(succeeded: bool, source: &str, destination: &str, error: Option<&str>) -> UndoResult {
    UndoResult::new(
        succeeded,
        Some(FileOperationType::Move),
        source,
        Some(destination.to_owned()),
        error.map(str::to_owned),
    )
}

fn rename_undone_text(file_name: &str) -> String {
    file_action_fakes::en("status.renameUndone", &[file_action_fakes::arg("fileName", file_name)])
}

// Mutant: undo.rs:197 `on_undone` rename branch, `if !result.succeeded` -> `if result.succeeded` (status of a FAILED
// rename undo without a message is "nothing to undo"; with a message it is the message).
#[test]
fn rename_undo_failed_result_shows_nothing_to_undo_or_its_own_message() {
    let mut env = RenameEnv::new();
    let (old, new, folder) = env.renamed_single();
    env.sink.status.clear();
    let step = env.undo_scripted(&folder, rename_undo_result(false, &old, &new, None));
    assert!(
        matches!(step, FileActionStep::Done(FileActionDone::Undone(Some(_)))),
        "{step:?}"
    );
    assert_eq!(
        env.sink.status.last(),
        Some(&file_action_fakes::text("status.nothingToUndo"))
    );

    let mut env = RenameEnv::new();
    let (old, new, folder) = env.renamed_single();
    env.sink.status.clear();
    env.undo_scripted(&folder, rename_undo_result(false, &old, &new, Some("scripted failure")));
    assert_eq!(env.sink.status.last().map(String::as_str), Some("scripted failure"));
}

// Mutant: undo.rs:202 `on_undone` rename branch, `.filter(|m| !m.is_empty())` -> `.filter(|m| m.is_empty())`: a succeeded
// undo shows its non-empty note, and an EMPTY note falls back to the "rename undone" text.
#[test]
fn rename_undo_succeeded_result_shows_a_non_empty_note_and_ignores_an_empty_one() {
    let mut env = RenameEnv::new();
    let (old, new, folder) = env.renamed_single();
    env.sink.status.clear();
    env.undo_scripted(
        &folder,
        rename_undo_result(true, &old, &new, Some("one companion stayed")),
    );
    assert_eq!(env.sink.status.last().map(String::as_str), Some("one companion stayed"));

    let mut env = RenameEnv::new();
    let (old, new, folder) = env.renamed_single();
    env.sink.status.clear();
    env.undo_scripted(&folder, rename_undo_result(true, &old, &new, Some("")));
    assert_eq!(env.sink.status.last(), Some(&rename_undone_text("b.jpg")));
}

// Mutant: undo.rs:183 (parked tail of the same rename-undo status chain): same two cases when the catalog presents
// (the sink keeps nothing, so `apply_rename_undo` presents and parks the status until the present finished).
#[test]
fn rename_undo_parked_status_shows_a_non_empty_note_and_ignores_an_empty_one() {
    let mut env = RenameEnv::new();
    let (old, new, folder) = env.renamed_single();
    env.sink.keep = false;
    env.sink.status.clear();
    let step = env.undo_scripted(
        &folder,
        rename_undo_result(true, &old, &new, Some("one companion stayed")),
    );
    assert_eq!(step, FileActionStep::Pending, "the present is awaited");
    assert!(env.sink.status.is_empty(), "the status waits for the present");
    env.finish_present();
    assert_eq!(env.sink.status.last().map(String::as_str), Some("one companion stayed"));

    let mut env = RenameEnv::new();
    let (old, new, folder) = env.renamed_single();
    env.sink.keep = false;
    env.sink.status.clear();
    assert_eq!(
        env.undo_scripted(&folder, rename_undo_result(true, &old, &new, Some(""))),
        FileActionStep::Pending
    );
    env.finish_present();
    assert_eq!(env.sink.status.last(), Some(&rename_undone_text("b.jpg")));
}

// ---------------------------------------------------------------------------------------------------------------
// Non-rename undo paths (file_action_fakes::Env)
// ---------------------------------------------------------------------------------------------------------------

const FOLDER: &str = "photos";

fn undo_result(
    ok: bool,
    op: FileOperationType,
    source: &str,
    restored: Option<Vec<String>>,
    error: Option<&str>,
) -> UndoResult {
    let mut r = UndoResult::new(ok, Some(op), source, None, error.map(str::to_string));
    r.restored_paths = restored;
    r
}

fn move_env(names: &[&str]) -> Env {
    let mut env = Env::new(AppSettings::default());
    env.load(names);
    env.set_actions(
        AppSettings::default(),
        vec![
            action("MoveToSub", FileOperationType::Move, "Sorted", false),
            action("DeleteIt", FileOperationType::Recycle, "", false),
        ],
    );
    env
}

fn do_move(env: &mut Env, name: &str) {
    env.exec.script(Scripted::Single(single_ok(
        FileOperationType::Move,
        &p(name),
        Some("photos/Sorted/x.jpg"),
    )));
    env.run_action(0, None, Some(&p(name)));
    env.run_worker();
}

fn do_recycle(env: &mut Env, name: &str) {
    env.run_action(1, None, Some(&p(name)));
    env.run_worker();
}

fn undone_ok(step: &FileActionStep) -> bool {
    matches!(step, FileActionStep::Done(FileActionDone::Undone(Some(r))) if r.succeeded)
}

// Mutants: undo.rs:93 `undo_last` `next_ticket += 1` -> `-=` / `*=`. Two undos in a row get different tickets, and the
// event of the older one is dropped as stale while the newer one is handled.
#[test]
fn undo_last_gives_each_undo_its_own_ticket_and_drops_the_event_of_the_older_one() {
    let mut env = move_env(&["a.jpg"]);
    let first = env.undo_begin(Some(FOLDER));
    let second = env.undo_begin(Some(FOLDER));
    assert!(second > first, "tickets grow: {first} then {second}");
    let stale = env.undo_deliver(
        first,
        undo_result(true, FileOperationType::Move, &p("a.jpg"), None, None),
    );
    assert_eq!(stale, FileActionStep::Pending, "the older undo is ignored");
    assert!(env.controller.is_in_flight());
    assert!(env.sink.is_untouched());
    let real = env.undo_deliver(
        second,
        undo_result(
            false,
            FileOperationType::Move,
            &p("a.jpg"),
            None,
            Some("scripted failure"),
        ),
    );
    assert!(!undone_ok(&real));
    assert!(!env.controller.is_in_flight());
}

// Mutants: undo.rs:274 `on_undone` Recycle branch, `was_empty && cx.catalog.count() > 0` -> `==`, `<`. A recycled last
// photo that is undone in its folder is shown (present index 0) and the session path follows after the present.
#[test]
fn recycle_undo_into_an_empty_catalog_presents_the_restored_photo_then_updates_the_session() {
    let mut env = move_env(&["a.jpg"]);
    do_recycle(&mut env, "a.jpg");
    assert_eq!(env.catalog.count(), 0);
    let step = env.undo_with(
        undo_result(true, FileOperationType::Recycle, &p("a.jpg"), None, None),
        Some(FOLDER),
    );
    assert_eq!(step, FileActionStep::Pending, "the present is awaited");
    assert_eq!(env.sink.presented, vec![0]);
    let done = env.present_finished(false);
    assert!(undone_ok(&done));
    assert_eq!(env.sink.sessions.last(), Some(&p("a.jpg")));
}

// Mutant: undo.rs:370 `run_undo_tail` (RecyclePresented), `if !is_folder_current` -> `if is_folder_current`: when the
// folder changed while the restored photo was presented, nothing is written to the new folder's session.
#[test]
fn recycle_undo_folder_switch_during_present_does_not_write_session_or_status() {
    let mut env = move_env(&["a.jpg"]);
    do_recycle(&mut env, "a.jpg");
    assert_eq!(
        env.undo_with(
            undo_result(true, FileOperationType::Recycle, &p("a.jpg"), None, None),
            Some(FOLDER)
        ),
        FileActionStep::Pending
    );
    env.clock.next_folder();
    env.load_paths(&["other/x.jpg".to_string()]);
    env.sink = Default::default();
    let step = env.present_finished(false);
    assert!(undone_ok(&step));
    assert!(env.sink.sessions.is_empty());
    assert!(env.sink.statuses.is_empty());
}

// Mutant: undo.rs:274 `>` -> `>=`: a restored path that is not a catalog image leaves the catalog empty, so nothing is
// presented.
#[test]
fn recycle_undo_of_a_non_image_into_an_empty_catalog_does_not_present() {
    let mut env = move_env(&["a.jpg"]);
    do_recycle(&mut env, "a.jpg");
    let step = env.undo_with(
        undo_result(
            true,
            FileOperationType::Recycle,
            &p("note.txt"),
            Some(vec![p("note.txt")]),
            None,
        ),
        Some(FOLDER),
    );
    assert!(undone_ok(&step), "{step:?}");
    assert!(env.sink.presented.is_empty());
    assert_eq!(env.catalog.count(), 0);
}

// Mutant: undo.rs:236 `on_undone` failed-partial branch, `was_empty && cx.catalog.count() > 0` -> `>=`: a partly
// restored path that is not an image leaves the empty catalog empty, so nothing is presented.
#[test]
fn failed_partial_undo_of_a_non_image_into_an_empty_catalog_does_not_present() {
    let mut env = move_env(&["a.jpg"]);
    do_recycle(&mut env, "a.jpg");
    let step = env.undo_with(
        undo_result(
            false,
            FileOperationType::Recycle,
            &p("note.txt"),
            Some(vec![p("note.txt")]),
            Some("scripted failure"),
        ),
        Some(FOLDER),
    );
    assert!(!undone_ok(&step), "{step:?}");
    assert!(env.sink.presented.is_empty());
    assert_eq!(env.catalog.count(), 0);
}

// Mutant: undo.rs:277 `restored_index >= 0` -> `< 0`: after a restore into an empty catalog the photo named by the undo
// is the one that is presented, not the current index.
#[test]
fn recycle_undo_into_an_empty_catalog_presents_the_named_restored_photo() {
    let mut env = move_env(&["a.jpg"]);
    env.load(&[]);
    let step = env.undo_with(
        undo_result(
            true,
            FileOperationType::Recycle,
            &p("b.jpg"),
            Some(vec![p("a.jpg"), p("b.jpg")]),
            None,
        ),
        Some(FOLDER),
    );
    assert_eq!(step, FileActionStep::Pending);
    let index = env.catalog.index_of(&p("b.jpg"));
    assert_eq!(index, 1, "a.jpg then b.jpg: {:?}", env.catalog_paths());
    assert_eq!(env.sink.presented, vec![index]);
}

// Mutants: undo.rs:297 and 298 `&&` -> `||` in the Move branch condition: only a Move restored in this folder is
// inserted; any other operation with a source in the folder leaves the catalog alone.
#[test]
fn undo_of_a_copy_in_the_current_folder_does_not_insert_or_present_anything() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    let before = env.catalog_paths();
    let step = env.undo_with(
        undo_result(true, FileOperationType::Copy, &p("c.jpg"), None, None),
        Some(FOLDER),
    );
    assert!(undone_ok(&step), "{step:?}");
    assert_eq!(env.catalog_paths(), before);
    assert!(env.sink.presented.is_empty());
    assert!(env.sink.removed.is_empty());
}

// Mutant: undo.rs:398 `remember_move_position` `len >= MAX` -> `len < MAX`: positions are NOT cleared while the map is
// below the bound, so the older move is still restored at its remembered (review-order) position.
#[test]
fn remembered_move_positions_survive_below_the_bound() {
    // review order is DESCENDING by name; the natural name order would put the restored photo after the survivor
    let mut env = move_env(&["c.jpg", "b.jpg", "a.jpg"]);
    do_move(&mut env, "c.jpg");
    do_move(&mut env, "b.jpg");
    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
    env.undo_with(
        undo_result(true, FileOperationType::Move, &p("c.jpg"), None, None),
        Some(FOLDER),
    );
    assert_eq!(env.catalog_paths(), vec![p("c.jpg"), p("a.jpg")]);
}

// ---------------------------------------------------------------------------------------------------------------
// File action core (actions.rs, non-rename part)
// ---------------------------------------------------------------------------------------------------------------

const DONE_FALSE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: false });
const DONE_TRUE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: true });

use FileOperationType::{Copy, Move, Recycle};

/// Move (0), Recycle (1), Copy (2) profiles, the first one with a confirmation when `confirm` is set.
fn action_env(names: &[&str], confirm: bool) -> Env {
    let mut env = Env::new(AppSettings::default());
    env.load(names);
    env.set_actions(
        AppSettings::default(),
        vec![
            action("MoveToSub", Move, "Sorted", confirm),
            action("DeleteIt", Recycle, "", false),
            action("CopyIt", Copy, "Copies", false),
        ],
    );
    env
}

fn pair_action_env() -> Env {
    let mut env = Env::new(AppSettings::default());
    env.load_pair_between_two_photos(false);
    env.set_actions(
        AppSettings::default(),
        vec![
            action("MoveToSub", Move, "Sorted", false),
            action("DeleteIt", Recycle, "", false),
            action("CopyIt", Copy, "Copies", false),
        ],
    );
    env
}

fn calls(env: &Env) -> usize {
    env.exec.recorded().len()
}

// Mutant: actions.rs:672 `run_action` re-check after the confirmation, `folder changed || busy || target missing`:
// `||` -> `&&` (first operator). A folder switch ALONE during the confirmation abandons the action.
#[test]
fn confirm_then_folder_switched_alone_abandons_a_move() {
    let mut env = action_env(&["a.jpg", "b.jpg"], true);
    let clock = env.clock.clone();
    env.dialog.set_on_confirm(move || {
        clock.next_folder();
    });
    assert_eq!(env.run_action(0, None, Some(&p("a.jpg"))), DONE_FALSE);
    assert_eq!(calls(&env), 0);
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("b.jpg")]);
    assert_eq!(env.port.queued(), 0);
}

// Mutant: actions.rs:672 (second operator): an executor that became busy ALONE during the confirmation abandons it.
#[test]
fn confirm_then_executor_busy_alone_abandons_a_move() {
    let mut env = action_env(&["a.jpg", "b.jpg"], true);
    let exec = env.exec.clone();
    env.dialog.set_on_confirm(move || exec.set_busy(true));
    assert_eq!(env.run_action(0, None, Some(&p("a.jpg"))), DONE_FALSE);
    assert_eq!(calls(&env), 0);
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("b.jpg")]);
}

// Mutants: actions.rs:672 `index_of(t) < 0` -> `==` / `<=`. A confirmed Move of the FIRST catalog entry (index 0) goes
// ahead; a confirmed Move of a path that is not in the catalog is abandoned.
#[test]
fn confirmed_move_goes_ahead_for_index_zero_and_stops_for_an_unlisted_path() {
    let mut env = action_env(&["a.jpg", "b.jpg"], true);
    env.exec
        .script(Scripted::Single(single_ok(Move, &p("a.jpg"), Some("Sorted/a.jpg"))));
    assert_eq!(env.run_action(0, None, Some(&p("a.jpg"))), FileActionStep::Pending);
    env.run_worker();
    assert_eq!(calls(&env), 1);
    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]);

    let mut env = action_env(&["a.jpg", "b.jpg"], true);
    assert_eq!(env.run_action(0, None, Some(&p("zz.jpg"))), DONE_FALSE);
    assert_eq!(calls(&env), 0);
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("b.jpg")]);
}

// Mutant: actions.rs:787 `execute_core` `next_ticket += 1` -> `*=`. Two actions started in a row get different tickets; the
// event of the older one is dropped as stale.
#[test]
fn action_tickets_differ_and_the_event_of_an_older_action_is_dropped() {
    let mut env = action_env(&["a.jpg", "b.jpg"], false);
    assert_eq!(env.run_action(2, None, Some(&p("a.jpg"))), FileActionStep::Pending);
    let first = env.port.run_all();
    assert_eq!(env.run_action(2, None, Some(&p("b.jpg"))), FileActionStep::Pending);
    let second = env.port.run_all();
    assert_eq!((first.len(), second.len()), (1, 1));
    for event in first {
        assert_eq!(
            env.handle(event),
            FileActionStep::Pending,
            "the older action is ignored"
        );
    }
    assert!(env.controller.is_in_flight());
    assert!(env.sink.is_untouched());
    for event in second {
        assert_eq!(env.handle(event), DONE_TRUE);
    }
    assert!(!env.controller.is_in_flight());
}

// Mutant: actions.rs:946 `on_executed` `flight.is_remove && flight.source_index >= 0` -> `||`. A failed capture COPY
// removed nothing from the catalog, so nothing is restored and the sink is not told the catalog changed.
#[test]
fn failed_group_copy_does_not_restore_or_announce_a_catalog_change() {
    let mut env = pair_action_env();
    env.exec.script(Scripted::Group(group_failed(
        Copy,
        vec![
            member(&p("pair.jpg"), Some("Copies/pair.jpg"), false, false),
            member(&p("pair.cr2"), Some("Copies/pair.cr2"), false, false),
        ],
        "denied",
    )));
    let before = env.catalog_paths();
    env.run_action(2, None, Some(&p("pair.jpg")));
    env.run_worker();
    assert_eq!(env.catalog_paths(), before);
    assert!(env.sink.removed.is_empty(), "{:?}", env.sink.removed);
}

// Mutant: actions.rs:959 `on_executed` `is a group image && (!state_known || source_exists)` -> `||`. After a capture
// Move that failed part way, a member that already left its source (state known, no source) does NOT come back.
#[test]
fn failed_group_move_restores_only_the_members_still_on_disk() {
    let mut env = pair_action_env();
    env.exec.script(Scripted::Group(group_failed(
        Move,
        vec![
            member(&p("pair.jpg"), Some("Sorted/pair.jpg"), true, false),
            member(&p("pair.cr2"), Some("Sorted/pair.cr2"), false, false),
        ],
        "disk full",
    )));
    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();
    assert!(
        env.catalog.index_of(&p("pair.cr2")) >= 0,
        "the member still on disk returns"
    );
    assert!(
        env.catalog.index_of(&p("pair.jpg")) < 0,
        "the moved member stays out: {:?}",
        env.catalog_paths()
    );
}

// Mutant: actions.rs:1014 `report_late` group success `operation == Move && target.is_some()` -> `||`. A late capture
// COPY is silent (it has no undo hint), and a late capture Move without a destination says nothing either.
#[test]
fn late_group_copy_and_late_group_move_without_destination_show_no_status() {
    let mut env = pair_action_env();
    env.exec.script(Scripted::Group(group_ok(
        Copy,
        vec![
            member(&p("pair.jpg"), Some("Copies/pair.jpg"), true, false),
            member(&p("pair.cr2"), Some("Copies/pair.cr2"), true, false),
        ],
        Some("Copies/pair.jpg"),
    )));
    env.run_action(2, None, Some(&p("pair.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);
    assert!(env.sink.late_statuses.is_empty(), "{:?}", env.sink.late_statuses);

    let mut env = pair_action_env();
    env.exec.script(Scripted::Group(group_ok(
        Move,
        vec![
            member(&p("pair.jpg"), None, true, false),
            member(&p("pair.cr2"), None, true, false),
        ],
        None,
    )));
    env.run_action(0, None, Some(&p("pair.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);
    assert!(env.sink.late_statuses.is_empty(), "{:?}", env.sink.late_statuses);
}

// Mutant: actions.rs:1052 `report_late` `processed > 0` -> `>= 0`. A late capture Delete that failed before any member
// was deleted reports no "partially processed" line.
#[test]
fn late_failed_group_recycle_with_no_processed_member_shows_no_status() {
    let mut env = pair_action_env();
    env.exec.script(Scripted::Group(group_failed(
        Recycle,
        vec![
            member(&p("pair.jpg"), None, false, false),
            member(&p("pair.cr2"), None, false, false),
        ],
        "bin full",
    )));
    env.run_action(1, None, Some(&p("pair.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);
    assert!(env.sink.late_statuses.is_empty(), "{:?}", env.sink.late_statuses);
}

// Mutant: actions.rs:1088 `finish_success` `flight.is_remove && flight.source_index >= 0` -> `||`. A successful COPY
// remembers no review position, so a later Move-undo of the same file falls back to the name order.
#[test]
fn successful_copy_does_not_remember_a_position_for_a_later_move_undo() {
    let mut env = action_env(&["a.jpg", "b.jpg", "c.jpg"], false);
    env.exec
        .script(Scripted::Single(single_ok(Copy, &p("b.jpg"), Some("Copies/b.jpg"))));
    env.run_action(2, None, Some(&p("b.jpg")));
    assert_eq!(env.run_worker(), DONE_TRUE);
    env.load(&["a.jpg", "c.jpg"]);
    env.undo_with(undo_result(true, Move, &p("b.jpg"), None, None), Some(FOLDER));
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("b.jpg"), p("c.jpg")]);
}

// Mutant: actions.rs:1204 `executor_threw` `operation == Recycle` -> `!=`. A Delete whose executor threw shows the
// Recycle failure dialog (file name and reason); a Move whose executor threw shows none.
#[test]
fn executor_throw_shows_the_recycle_failure_dialog_only_for_a_delete() {
    let mut env = action_env(&["a.jpg", "b.jpg"], false);
    env.exec.script(Scripted::Panic("boom".to_string()));
    env.run_action(1, None, Some(&p("a.jpg")));
    env.run_worker();
    env.present_finished(false);
    let (_title, message) = env.dialog.only_error();
    assert!(message.contains("a.jpg") && message.contains("boom"), "{message}");

    let mut env = action_env(&["a.jpg", "b.jpg"], false);
    env.exec.script(Scripted::Panic("boom".to_string()));
    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();
    env.present_finished(false);
    assert!(env.dialog.errors.lock().unwrap().is_empty());
}
