//! Shared fakes for the `FileActionController` test files (W8-07c owns this module; 07d, 07e and 07f reuse it).
//! Port of the C# harness `FileActionControllerMutationGapHarness` (`RecordingDialog`, `RecordingSink`, `FakeBin`, `HookFileSystem`,
//! `FixedPicker`) in the Rust shape: no disk, no real Recycle Bin. The executor is a hand-written `IFileActionExecutor`, the worker
//! is a manual queue that runs a job only when the test says so (no sleeps, no threads).
//!
//! Each test file includes it with `mod file_action_fakes;`, so a helper that one file does not use is expected.
#![allow(dead_code)] // shared by several test binaries; each one uses a subset

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_app::coordinators::file_actions::recycle::{
    FileActionController, FileActionCx, FileActionDeps, FileActionEvent, FileActionStep, TextSource,
};
use pr_app::coordinators::folder_load::state::{WorkerLane, WorkerPort};
use pr_app::coordinators::traits::{IFileActionExecutor, IFileActionSink, IPreloadController};
use pr_core::abstractions::{BatchReviewItem, DialogService, Log, RecycleBin, SkippedEntry, SystemClock};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::natural_cmp::ManagedNaturalComparer;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CatalogEntry;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, FileActionRequest, FileActionResult,
};
use pr_core::file_actions::undo::{EnglishUndoTexts, UndoService};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, RawPairMode};
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::shortcuts::ReviewAction;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::preload::scheduler::RunHandle;
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::template::NumberCulture;

/// English texts from the built-in catalog (C# ambient `Tr` with the English localizer).
pub struct EnglishTexts {
    localizer: Arc<Localizer>,
    numbers: Arc<NumberCulture>,
}

impl EnglishTexts {
    pub fn new() -> Self {
        Self {
            localizer: built_in_english_localizer(),
            numbers: Arc::new(NumberCulture::invariant()),
        }
    }
}

impl TextSource for EnglishTexts {
    fn localizer(&self) -> Arc<Localizer> {
        self.localizer.clone()
    }

    fn numbers(&self) -> Arc<NumberCulture> {
        self.numbers.clone()
    }
}

/// C# `RecordingDialog`: scripted confirmation answer, records every call, can run a hook while a confirmation is "open"
/// (the nested dispatcher loop of the C# dialog: a forwarded open switches the folder, another action becomes busy).
pub struct RecordingDialog {
    response: AtomicBool,
    pub confirmations: Mutex<Vec<(String, String)>>,
    pub errors: Mutex<Vec<(String, String)>>,
    pub on_confirm: Mutex<Option<Box<dyn FnMut() + Send>>>,
}

impl RecordingDialog {
    pub fn new(response: bool) -> Arc<Self> {
        Arc::new(Self {
            response: AtomicBool::new(response),
            confirmations: Mutex::new(Vec::new()),
            errors: Mutex::new(Vec::new()),
            on_confirm: Mutex::new(None),
        })
    }

    pub fn set_answer(&self, answer: bool) {
        self.response.store(answer, Ordering::SeqCst);
    }

    pub fn set_on_confirm(&self, hook: impl FnMut() + Send + 'static) {
        *self.on_confirm.lock().unwrap() = Some(Box::new(hook));
    }

    pub fn confirmation_count(&self) -> usize {
        self.confirmations.lock().unwrap().len()
    }

    pub fn only_confirmation(&self) -> (String, String) {
        let list = self.confirmations.lock().unwrap();
        assert_eq!(list.len(), 1, "expected exactly one confirmation, got {list:?}");
        list[0].clone()
    }

    pub fn only_error(&self) -> (String, String) {
        let list = self.errors.lock().unwrap();
        assert_eq!(list.len(), 1, "expected exactly one error dialog, got {list:?}");
        list[0].clone()
    }
}

impl DialogService for RecordingDialog {
    fn show_confirmation(&self, title: &str, message: &str) -> bool {
        self.confirmations
            .lock()
            .unwrap()
            .push((title.to_string(), message.to_string()));
        if let Some(hook) = self.on_confirm.lock().unwrap().as_mut() {
            hook();
        }
        self.response.load(Ordering::SeqCst)
    }

    fn show_message(&self, _title: &str, _message: &str) {}

    fn show_error(&self, title: &str, message: &str) {
        self.errors
            .lock()
            .unwrap()
            .push((title.to_string(), message.to_string()));
    }

    fn pick_folder(&self, _initial_folder: Option<&Path>) -> Option<PathBuf> {
        None
    }

    fn show_batch_review(&self, _items: &[BatchReviewItem]) -> bool {
        false
    }

    fn show_recovery(&self) {}

    fn show_diagnostics(&self) {}

    fn show_settings(&self) -> bool {
        false
    }

    fn show_benchmark(&self, _folder: Option<&Path>) {}

    fn show_skipped_files(&self, _entries: &[SkippedEntry]) {}
}

/// C# `RecordingSink`: an ordered log of everything the controller told the shell.
#[derive(Debug, Default)]
pub struct RecordingSink {
    pub statuses: Vec<String>,
    pub late_statuses: Vec<String>,
    pub removed: Vec<Option<String>>,
    pub evicted: Vec<String>,
    pub presented: Vec<i32>,
    pub sessions: Vec<String>,
    pub navigation_changes: usize,
    /// Makes `update_session_path` panic (C# "the bookkeeping after a successful action threw").
    pub panic_on_session: bool,
}

impl RecordingSink {
    pub fn last_status(&self) -> Option<&str> {
        self.statuses.last().map(String::as_str)
    }

    /// True when the sink saw no call at all.
    pub fn is_untouched(&self) -> bool {
        self.statuses.is_empty()
            && self.late_statuses.is_empty()
            && self.removed.is_empty()
            && self.evicted.is_empty()
            && self.presented.is_empty()
            && self.sessions.is_empty()
            && self.navigation_changes == 0
    }
}

impl IFileActionSink for RecordingSink {
    fn set_status_text(&mut self, status: &str) {
        self.statuses.push(status.to_string());
    }

    fn show_late_action_status(&mut self, status: &str) {
        self.late_statuses.push(status.to_string());
    }

    fn on_catalog_changed(&mut self, removed_path: Option<&str>) {
        self.removed.push(removed_path.map(str::to_string));
    }

    fn evict_cached_paths(&mut self, paths: &[String]) {
        self.evicted.extend(paths.iter().cloned());
    }

    fn present(&mut self, index: i32) {
        self.presented.push(index);
    }

    fn update_session_path(&mut self, current_path: &str) {
        assert!(!self.panic_on_session, "session write failed");
        self.sessions.push(current_path.to_string());
    }

    fn notify_navigation_state_changed(&mut self) {
        self.navigation_changes += 1;
    }
}

/// One call the executor received.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutorCall {
    Single(FileActionRequest),
    Group(CaptureGroupActionRequest),
}

/// What the next executor call does instead of succeeding (C# fake bins that fail, a service that throws).
#[derive(Debug, Clone)]
pub enum Scripted {
    Single(FileActionResult),
    Group(CaptureGroupActionResult),
    /// The executor panics (C# "unexpected exception").
    Panic(String),
}

/// C# `FakeBin` + `VanishingBin` folded into an `IFileActionExecutor`: nothing touches a disk or the real Recycle Bin.
/// `lacks_recycle_bin` answers from a script; every call is counted.
#[derive(Default)]
pub struct FakeExecutor {
    pub busy: AtomicBool,
    /// Every path lacks a bin (C# `FakeBin.NoBin`).
    pub no_bin: AtomicBool,
    /// Only these paths lack a bin (C# `FakeBin.NoBinFor`, compared exactly).
    pub no_bin_for: Mutex<HashSet<String>>,
    /// C# `VanishingBin(binAnswers)`: the first N `lacks_recycle_bin` queries say "has a bin", later ones say "no bin".
    pub bin_answers: Mutex<Option<usize>>,
    pub lacks_calls: AtomicUsize,
    pub calls: Mutex<Vec<ExecutorCall>>,
    pub script: Mutex<VecDeque<Scripted>>,
}

impl FakeExecutor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::SeqCst);
    }

    pub fn set_no_bin(&self, no_bin: bool) {
        self.no_bin.store(no_bin, Ordering::SeqCst);
    }

    pub fn add_no_bin_for(&self, path: &str) {
        self.no_bin_for.lock().unwrap().insert(path.to_string());
    }

    pub fn vanish_after(&self, bin_answers: usize) {
        *self.bin_answers.lock().unwrap() = Some(bin_answers);
    }

    pub fn script(&self, next: Scripted) {
        self.script.lock().unwrap().push_back(next);
    }

    pub fn recorded(&self) -> Vec<ExecutorCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl IFileActionExecutor for FakeExecutor {
    fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    fn lacks_recycle_bin(&self, path: &str) -> bool {
        let call_number = self.lacks_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(answers) = *self.bin_answers.lock().unwrap() {
            return call_number > answers;
        }
        self.no_bin.load(Ordering::SeqCst) || self.no_bin_for.lock().unwrap().contains(path)
    }

    fn execute(&self, request: &FileActionRequest, _cancel: &CancelToken) -> FileActionResult {
        self.calls.lock().unwrap().push(ExecutorCall::Single(request.clone()));
        match self.script.lock().unwrap().pop_front() {
            Some(Scripted::Single(result)) => return result,
            Some(Scripted::Panic(text)) => panic!("{text}"),
            _ => {}
        }
        FileActionResult::new(
            true,
            request.operation,
            request.source.clone(),
            request.destination.clone(),
            4,
            DateTimeUtc::MIN,
            None,
        )
    }

    fn execute_group(&self, request: &CaptureGroupActionRequest, _cancel: &CancelToken) -> CaptureGroupActionResult {
        self.calls.lock().unwrap().push(ExecutorCall::Group(request.clone()));
        match self.script.lock().unwrap().pop_front() {
            Some(Scripted::Group(result)) => return result,
            Some(Scripted::Panic(text)) => panic!("{text}"),
            _ => {}
        }
        CaptureGroupActionResult::new(true, false, request.operation, "group", None, Vec::new(), None)
    }
}

type QueuedJob = (WorkerLane, Box<dyn FnOnce() -> FileActionEvent + Send>);

/// The worker seam as a manual queue: a job runs only when the test calls [`ManualPort::run_all`], which returns the events
/// that the shell would deliver with `handle_event`.
#[derive(Default)]
pub struct ManualPort {
    jobs: Mutex<VecDeque<QueuedJob>>,
    pub lanes: Mutex<Vec<WorkerLane>>,
}

impl ManualPort {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn queued(&self) -> usize {
        self.jobs.lock().unwrap().len()
    }

    pub fn run_all(&self) -> Vec<FileActionEvent> {
        let mut events = Vec::new();
        loop {
            let next = self.jobs.lock().unwrap().pop_front();
            let Some((_, job)) = next else { break };
            events.push(job());
        }
        events
    }
}

impl WorkerPort<FileActionEvent> for ManualPort {
    fn submit(&self, lane: WorkerLane, job: Box<dyn FnOnce() -> FileActionEvent + Send>) {
        self.lanes.lock().unwrap().push(lane);
        self.jobs.lock().unwrap().push_back((lane, job));
    }
}

/// A review action profile (C# harness `MoveAction` / `RecycleAction`).
pub fn action(name: &str, operation: FileOperationType, destination: &str, confirm: bool) -> ReviewAction {
    ReviewAction {
        name: name.to_string(),
        shortcut: "Enter".to_string(),
        operation,
        destination: destination.to_string(),
        confirm,
    }
}

/// Test paths: relative, never on a disk. `Path::file_name` gives the C# `Path.GetFileName` result.
pub fn p(name: &str) -> String {
    format!("photos/{name}")
}

/// Records `error` lines (the C# `AppLog.Error` calls the tests look for).
#[derive(Default)]
pub struct RecordingLog {
    pub errors: Mutex<Vec<String>>,
}

impl Log for RecordingLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, _message: &str) {}

    fn warn(&self, _message: &str) {}

    fn error(&self, message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
        self.errors.lock().unwrap().push(message.to_string());
    }
}

/// Counts the calls the controller makes on the preload seam (C# `preloadController: null` tests never see them).
#[derive(Default)]
pub struct RecordingPreload {
    pub cancels: AtomicUsize,
    pub preload_centers: Mutex<Vec<i32>>,
}

impl IPreloadController for RecordingPreload {
    fn preload_around(&self, center: i32) -> RunHandle {
        self.preload_centers.lock().unwrap().push(center);
        RunHandle::completed()
    }

    fn try_consume_preloaded_key(&self, _key: &ImageCacheKey) -> bool {
        false
    }

    fn cancel(&self) {
        self.cancels.fetch_add(1, Ordering::SeqCst);
    }

    fn remove_preloaded_keys_for_path(&self, _normalized_path: &str) {}

    fn clear_preloaded_keys(&self) {}
}

/// A Recycle Bin that never touches anything (nothing in these tests reaches the real one).
struct InertBin;

impl RecycleBin for InertBin {
    fn send_to_recycle_bin(&self, _path: &Path) -> std::io::Result<()> {
        Ok(())
    }

    fn try_restore(&self, _path: &Path, _size: u64, _last_write: DateTimeUtc) -> std::io::Result<bool> {
        Ok(false)
    }
}

/// A real `UndoService` over a journal that is never written (nothing here executes a file action for real).
pub fn new_undo_service() -> Arc<Mutex<UndoService>> {
    let fs: Arc<dyn pr_core::abstractions::FileSystem> = Arc::new(PhysicalFileSystem);
    let paths = AppPaths::new(&std::env::temp_dir().join("pr-file-action-fakes"), None, false);
    let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), Arc::new(SystemClock)));
    Arc::new(Mutex::new(UndoService::new(
        journal,
        fs,
        Arc::new(InertBin),
        Arc::new(EnglishUndoTexts),
    )))
}

/// One controller with all its fakes. The catalog and the sink are owned here and borrowed per call (`FileActionCx`).
pub struct Env {
    pub catalog: ReviewCatalog,
    pub clock: Arc<GenerationClock>,
    pub sink: RecordingSink,
    pub dialog: Arc<RecordingDialog>,
    pub exec: Arc<FakeExecutor>,
    pub port: Arc<ManualPort>,
    pub preload: Arc<RecordingPreload>,
    pub log: Arc<RecordingLog>,
    pub undo: Option<Arc<Mutex<UndoService>>>,
    pub settings: Arc<Mutex<Arc<AppSettings>>>,
    pub controller: FileActionController,
}

pub struct EnvOptions {
    pub dialog: bool,
    pub dialog_answer: bool,
    pub file_system: bool,
    pub executor: bool,
    pub undo: bool,
}

impl Default for EnvOptions {
    fn default() -> Self {
        Self {
            dialog: true,
            dialog_answer: true,
            file_system: true,
            executor: true,
            undo: true,
        }
    }
}

impl Env {
    pub fn new(settings: AppSettings) -> Self {
        Self::with_options(settings, EnvOptions::default())
    }

    pub fn with_options(settings: AppSettings, options: EnvOptions) -> Self {
        let clock = Arc::new(GenerationClock::new(0, 0, 0));
        let dialog = RecordingDialog::new(options.dialog_answer);
        let exec = FakeExecutor::new();
        let port = ManualPort::new();
        let preload = Arc::new(RecordingPreload::default());
        let log = Arc::new(RecordingLog::default());
        let undo = options.undo.then(new_undo_service);
        let settings = Arc::new(Mutex::new(Arc::new(settings)));
        let settings_for_controller = settings.clone();
        let controller = FileActionController::new(FileActionDeps {
            clock: clock.clone(),
            file_actions: options
                .executor
                .then(|| exec.clone() as Arc<dyn IFileActionExecutor + Send + Sync>),
            undo_service: undo.clone(),
            dialog_service: options.dialog.then(|| dialog.clone() as Arc<dyn DialogService>),
            preload_controller: Some(preload.clone()),
            natural_comparer: Arc::new(ManagedNaturalComparer),
            get_settings: Box::new(move || settings_for_controller.lock().unwrap().clone()),
            file_system: options
                .file_system
                .then(|| Arc::new(PhysicalFileSystem) as Arc<dyn pr_core::abstractions::FileSystem>),
            remember_folder: None,
            worker: port.clone(),
            texts: Arc::new(EnglishTexts::new()),
            log: log.clone(),
        });
        Self {
            catalog: ReviewCatalog::new(),
            clock,
            sink: RecordingSink::default(),
            dialog,
            exec,
            port,
            preload,
            log,
            undo,
            settings,
            controller,
        }
    }

    pub fn dialog_answer(&self, answer: bool) {
        self.dialog.set_answer(answer);
    }

    pub fn set_settings(&self, settings: AppSettings) {
        *self.settings.lock().unwrap() = Arc::new(settings);
    }

    /// Plain photos, in the given order.
    pub fn load(&mut self, names: &[&str]) {
        let entries = names.iter().map(|n| CatalogEntry::new(p(n)).expect("path")).collect();
        self.catalog.reset(entries, RawPairMode::PreferJpeg, None);
    }

    /// `[before.jpg, pair.jpg + pair.cr2 (+ pair.xmp) as one capture, after.jpg]` (C# `LoadPairBetweenTwoPhotos`).
    pub fn load_pair_between_two_photos(&mut self, with_xmp: bool) {
        let entries = vec![
            CatalogEntry::new(p("zbefore.jpg")).expect("path"),
            CatalogEntry::new(p("pair.jpg")).expect("path"),
            CatalogEntry::new(p("pair.cr2")).expect("path"),
            CatalogEntry::new(p("aafter.jpg")).expect("path"),
        ];
        let sidecars = [p("pair.xmp")];
        let sidecar_paths: Option<&[String]> = if with_xmp { Some(&sidecars) } else { None };
        self.catalog.reset(entries, RawPairMode::PreferJpeg, sidecar_paths);
    }

    /// The controller's `recycle` with the catalog and the sink of this environment.
    pub fn recycle(
        &mut self,
        compare_selected: Option<&str>,
        current: Option<&str>,
    ) -> pr_app::coordinators::file_actions::recycle::FileActionStep {
        let mut cx = FileActionCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.recycle(&mut cx, compare_selected, current)
    }

    /// The controller's `run_action`.
    pub fn run_action(&mut self, index: i32, compare_selected: Option<&str>, current: Option<&str>) -> FileActionStep {
        let mut cx = FileActionCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.run_action(&mut cx, index, compare_selected, current)
    }

    /// Delivers one event to the controller.
    pub fn handle(&mut self, event: FileActionEvent) -> FileActionStep {
        let mut cx = FileActionCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.handle_event(&mut cx, event)
    }

    /// Runs every queued worker job and delivers its event; returns the last step (the one that ends the operation).
    pub fn run_worker(&mut self) -> FileActionStep {
        let mut last = FileActionStep::Pending;
        for event in self.port.run_all() {
            last = self.handle(event);
        }
        last
    }

    /// The test presenter reports that the present the controller requested finished.
    pub fn present_finished(&mut self, faulted: bool) -> FileActionStep {
        let mut cx = FileActionCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.on_present_finished(&mut cx, faulted)
    }

    /// Settings with the given action profiles.
    pub fn set_actions(&self, settings: AppSettings, actions: Vec<ReviewAction>) {
        self.set_settings(AppSettings { actions, ..settings });
    }

    /// Everything the catalog shows, in order.
    pub fn catalog_paths(&self) -> Vec<String> {
        self.catalog.paths().to_vec()
    }
}

/// English text of a key with arguments (what the C# tests compute with `Tr.*`).
pub fn en(key: &str, args: &[pr_l10n::template::LocArg]) -> String {
    built_in_english_localizer().format(key, args)
}

/// English text of an argument-less key.
pub fn text(key: &str) -> String {
    built_in_english_localizer().get(key)
}

/// A string argument.
pub fn arg(name: &str, value: &str) -> pr_l10n::template::LocArg {
    pr_l10n::template::LocArg::new(name, value.to_string())
}
