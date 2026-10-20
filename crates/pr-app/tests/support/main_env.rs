//! The whole-window fixture of the W8-08b..08e tests: a real `MainViewModel` over real coordinators, a real presenter over a fake
//! decoder, real (tiny) files under a `TempRoot`, and test doubles for everything that blocks or is native.
//!
//! Included with
//! `mod file_action_fakes; mod support { pub mod main_env; pub mod preview_fakes; pub mod presenter_fakes; pub mod present_helpers; }`.
//!
//! Determinism (AGENTS.md "Tests"): every job the view model's coordinators send to a worker goes into the [`ManualRunner`] queue and
//! runs only when the test says so ([`MainEnv::settle`] runs them all; [`ManualRunner::run_tagged`] runs one lane), so a test can hold
//! the Explorer query back while the scan finishes. The presenter's own decode/stat work runs on real threads and is waited for with
//! the presenter's inbox condvar (bounded by [`TIMEOUT`]), never by a sleep. The gate scheduler is a queue the test drains.

// Each test binary uses a different subset of this fixture.
#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use pr_app::coordinators::duplicate_cleanup::{
    DuplicateCleanupController, DuplicateCleanupDeps, DuplicateCleanupEvent,
};
use pr_app::coordinators::file_action_gate::FileActionGate;
use pr_app::coordinators::file_actions::recycle::{FileActionController, FileActionDeps, TextSource};
use pr_app::coordinators::folder_load::state::{FolderLoadCoordinator, FolderLoadDeps, WorkerLane, WorkerPort};
use pr_app::coordinators::presenter::state::{ImageChange, ImagePresenter, ImagePresenterDeps};
use pr_app::coordinators::sibling_navigator::SiblingFolderNavigator;
use pr_app::coordinators::traits::{
    FileActionServiceExecutor, IFileActionExecutor, IFolderPicker, IPreloadController, IPresentationSink,
};
use pr_app::coordinators::zoom_detail::{BackgroundJob, BackgroundRunner, CompletionInbox};
use pr_app::misc::file_hash::FileHasher;
use pr_app::viewmodels::compare::CompareViewModel;
use pr_app::viewmodels::format::context::FormatContext;
use pr_app::viewmodels::main::{
    ChangeSet, FormatContextSource, MainEvent, MainViewModel, MainViewModelDeps, PresentationBridge, SettingsSource,
    ViewerHandle, remember_move_copy_folder,
};
use pr_app::viewmodels::viewer_state::ViewerState;
use pr_core::abstractions::{
    BatchReviewItem, DialogService, ExplorerOrderProvider, FileSystem, Log, RecycleBin, SettingsTarget, SkippedEntry,
    SystemClock,
};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::explorer_types::{
    ExplorerGroupState, ExplorerOrderStatus, ExplorerQueryProgress, ExplorerViewSnapshot,
};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::natural_cmp::ManagedNaturalComparer;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::undo::{EnglishUndoTexts, FileActionGate as ServiceGate, MoveOverride, UndoService};
use pr_core::instance::ownership::{FolderOpenDecision, FolderOwnership, FolderOwnershipError};
use pr_core::io::navigation_stat::NavigationStatWorker;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{FileOperationType, ImageSortMode, LoadingMode};
use pr_core::session::store::SessionStore;
use pr_core::session::writer::{Completion, DebounceTimer, RunningTimer, SessionWriter, TimerError, TimerOutcome};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::shortcuts::ReviewAction;
use pr_core::settings::store::SettingsStore;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::decode_types::{DecodeBox, PlatformImage};
use pr_imaging::decoder_traits::ImageDecoder;
use pr_imaging::preload::scheduler::RunHandle;
use pr_imaging::service::{PreviewImageService, PreviewServiceConfig};
use pr_test_support::{QueuedUiScheduler, RecordingLog, TempRoot};

use crate::file_action_fakes::EnglishTexts;

use super::presenter_fakes::{FakeHasher, FormatFixture, build_thumbnail_cache};
use super::preview_fakes::{FakePayloadCodec, GIB, MIB, PixelsCodec, SizedDecoder, TIMEOUT, lock};

// ---- worker queue -----------------------------------------------------------------------------------------------------------

/// A [`BackgroundRunner`] that queues every job (tagged with the name it was submitted under, or the worker lane) until the test runs it.
#[derive(Default)]
pub struct ManualRunner {
    jobs: Mutex<VecDeque<(String, BackgroundJob)>>,
}

impl std::fmt::Debug for ManualRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManualRunner")
            .field("pending", &self.pending())
            .finish()
    }
}

impl ManualRunner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn push(&self, tag: String, job: BackgroundJob) {
        lock(&self.jobs).push_back((tag, job));
    }

    pub fn pending(&self) -> usize {
        lock(&self.jobs).len()
    }

    /// Number of queued jobs with this tag (`"Scan"`, `"Explorer"`, `"Probe"`, `"FileAction"`, `"Hashing"`, `"CacheClear"`, or a runner name).
    pub fn count_of(&self, tag: &str) -> usize {
        lock(&self.jobs).iter().filter(|(t, _)| t == tag).count()
    }

    pub fn tags(&self) -> Vec<String> {
        lock(&self.jobs).iter().map(|(t, _)| t.clone()).collect()
    }

    /// Runs the OLDEST queued job with this tag on the calling thread; false when there is none.
    pub fn run_tagged(&self, tag: &str) -> bool {
        let job = {
            let mut jobs = lock(&self.jobs);
            let at = jobs.iter().position(|(t, _)| t == tag);
            at.and_then(|at| jobs.remove(at)).map(|(_, job)| job)
        };
        match job {
            Some(job) => {
                job();
                true
            }
            None => false,
        }
    }

    /// Runs the queued job number `index` (0 = oldest) on the calling thread.
    pub fn run_at(&self, index: usize) -> bool {
        let job = lock(&self.jobs).remove(index).map(|(_, job)| job);
        match job {
            Some(job) => {
                job();
                true
            }
            None => false,
        }
    }

    /// Index of the oldest queued job whose tag is not in `hold`.
    pub fn position_not_in(&self, hold: &[&str]) -> Option<usize> {
        lock(&self.jobs)
            .iter()
            .position(|(tag, _)| !hold.contains(&tag.as_str()))
    }

    /// Runs every queued job, oldest first, including the ones they queue. Returns how many ran.
    pub fn run_all(&self) -> usize {
        let mut ran = 0;
        loop {
            let job = lock(&self.jobs).pop_front().map(|(_, job)| job);
            let Some(job) = job else { return ran };
            job();
            ran += 1;
        }
    }
}

impl BackgroundRunner for ManualRunner {
    fn run(&self, name: &'static str, job: BackgroundJob) {
        self.push(name.to_owned(), job);
    }
}

/// The `WorkerPort` of one coordinator: queues the job under its lane name and, when it runs, sends the wrapped event into the
/// view model's inbox (what `MainWorkerPort` does in production, but lane-aware so a test can hold one lane back).
pub struct LanePort<E> {
    runner: Arc<ManualRunner>,
    inbox: CompletionInbox<MainEvent>,
    wrap: fn(E) -> MainEvent,
}

impl<E> LanePort<E> {
    pub fn new(runner: Arc<ManualRunner>, inbox: CompletionInbox<MainEvent>, wrap: fn(E) -> MainEvent) -> Self {
        Self { runner, inbox, wrap }
    }
}

impl<E: Send + 'static> WorkerPort<E> for LanePort<E> {
    fn submit(&self, lane: WorkerLane, job: Box<dyn FnOnce() -> E + Send>) {
        let inbox = self.inbox.clone();
        let wrap = self.wrap;
        self.runner
            .push(format!("{lane:?}"), Box::new(move || inbox.send(wrap(job()))));
    }
}

// ---- presentation sink ------------------------------------------------------------------------------------------------------

/// What the sink saw (for the tests that count presentations).
#[derive(Debug, Default)]
pub struct SinkSeen {
    pub images: usize,
    pub statuses: Vec<String>,
    pub presented: Vec<String>,
}

/// Production's `WpfPresentationSink` wiring in miniature: the image and status callbacks feed the [`PresentationBridge`].
pub struct BridgeSink {
    bridge: PresentationBridge,
    seen: Arc<Mutex<SinkSeen>>,
}

impl IPresentationSink for BridgeSink {
    fn set_current_image(&mut self, image: Option<PlatformImage>, is_file_change: bool, change: &ImageChange<'_>) {
        if image.is_some() {
            lock(&self.seen).images += 1;
        }
        self.bridge.on_set_current_image(change, is_file_change);
    }

    fn take_viewer_zoom_change(&mut self) -> Option<Option<f64>> {
        self.bridge.take_viewer_zoom_change()
    }

    fn set_status_text(&mut self, status: &str) {
        lock(&self.seen).statuses.push(status.to_owned());
        self.bridge.on_set_status_text();
    }

    fn apply_initial_view_mode(&mut self) {}

    fn on_presented(&mut self, path: &str) {
        lock(&self.seen).presented.push(path.to_owned());
    }

    fn trace_presented(&mut self, _token: i64, _kind: &str, _assigned_timestamp: i64) {}
}

// ---- format -----------------------------------------------------------------------------------------------------------------

/// The texts for the view model (English unless the test starts in Vietnamese or switches); local time = UTC (the tests compare
/// invariant text). The language can be switched while the view model lives (ADR 0006).
pub struct TestFormat {
    fixture: std::sync::RwLock<FormatFixture>,
}

impl TestFormat {
    /// The live language switch of the tests.
    pub fn use_language(&self, vietnamese: bool) {
        let fixture = if vietnamese {
            FormatFixture::vietnamese()
        } else {
            FormatFixture::new()
        };
        *self.fixture.write().unwrap_or_else(std::sync::PoisonError::into_inner) = fixture;
    }
}

impl FormatContextSource for TestFormat {
    fn with_context(&self, run: &mut dyn FnMut(&FormatContext<'_>)) {
        run(&self
            .fixture
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ctx());
    }

    fn to_local_time(&self, utc: DateTimeUtc) -> DateTimeUtc {
        utc
    }
}

// ---- explorer ---------------------------------------------------------------------------------------------------------------

/// Explorer provider: `NativeViewUnavailable` unless the test gave an order.
#[derive(Default)]
pub struct FakeExplorer {
    order: Mutex<Option<Vec<String>>>,
    queries: AtomicUsize,
}

impl std::fmt::Debug for FakeExplorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeExplorer").finish_non_exhaustive()
    }
}

impl FakeExplorer {
    pub fn set_order(&self, order: Vec<String>) {
        *lock(&self.order) = Some(order);
    }

    pub fn queries(&self) -> usize {
        self.queries.load(Ordering::SeqCst)
    }
}

impl ExplorerOrderProvider for FakeExplorer {
    fn try_get_snapshot_progressive(
        &self,
        folder: &Path,
        _timeout: Duration,
        _progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
        _batch_size: usize,
        _cancel: &CancelToken,
    ) -> ExplorerViewSnapshot {
        self.queries.fetch_add(1, Ordering::SeqCst);
        let order = lock(&self.order).clone();
        ExplorerViewSnapshot {
            folder: folder.to_string_lossy().into_owned(),
            ordered_paths: order.clone().unwrap_or_default(),
            sort_columns: Vec::new(),
            group_state: ExplorerGroupState::None,
            status: if order.is_some() {
                ExplorerOrderStatus::Available
            } else {
                ExplorerOrderStatus::NativeViewUnavailable
            },
            reason: None,
            captured_utc: DateTimeUtc::now(),
        }
    }
}

// ---- dialogs and picker -----------------------------------------------------------------------------------------------------

type SettingsHook = Box<dyn FnMut(SettingsTarget) -> bool + Send>;

/// A scripted [`DialogService`] that records every call.
pub struct TestDialog {
    pub confirm_answer: AtomicBool,
    /// Runs while a confirmation is "open" (the nested loop of the C# dialog): it may change what the shared clock says.
    pub on_confirm: Mutex<Option<Box<dyn FnMut() + Send>>>,
    pub confirmations: Mutex<Vec<(String, String)>>,
    pub errors: Mutex<Vec<(String, String)>>,
    pub messages: Mutex<Vec<(String, String)>>,
    pub picked: Mutex<Option<PathBuf>>,
    pub pick_initials: Mutex<Vec<Option<PathBuf>>>,
    pub on_settings: Mutex<Option<SettingsHook>>,
    pub settings_targets: Mutex<Vec<SettingsTarget>>,
    pub batch_review_answer: AtomicBool,
    /// Runs while the batch review is open (the nested loop of the C# dialog).
    pub on_batch_review: Mutex<Option<Box<dyn FnMut() + Send>>>,
    pub batch_reviews: Mutex<Vec<Vec<BatchReviewItem>>>,
    pub benchmark_folders: Mutex<Vec<Option<PathBuf>>>,
    pub skipped_shown: Mutex<Vec<Vec<SkippedEntry>>>,
    pub recovery_shown: AtomicUsize,
    pub diagnostics_shown: AtomicUsize,
}

impl std::fmt::Debug for TestDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestDialog").finish_non_exhaustive()
    }
}

impl TestDialog {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            confirm_answer: AtomicBool::new(true),
            on_confirm: Mutex::new(None),
            confirmations: Mutex::new(Vec::new()),
            errors: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
            picked: Mutex::new(None),
            pick_initials: Mutex::new(Vec::new()),
            on_settings: Mutex::new(None),
            settings_targets: Mutex::new(Vec::new()),
            batch_review_answer: AtomicBool::new(true),
            on_batch_review: Mutex::new(None),
            batch_reviews: Mutex::new(Vec::new()),
            benchmark_folders: Mutex::new(Vec::new()),
            skipped_shown: Mutex::new(Vec::new()),
            recovery_shown: AtomicUsize::new(0),
            diagnostics_shown: AtomicUsize::new(0),
        })
    }

    /// What the settings dialog does when the user opens it: `hook` may change the store and returns "saved".
    pub fn on_settings(&self, hook: impl FnMut(SettingsTarget) -> bool + Send + 'static) {
        *lock(&self.on_settings) = Some(Box::new(hook));
    }
}

impl DialogService for TestDialog {
    fn show_confirmation(&self, title: &str, message: &str) -> bool {
        lock(&self.confirmations).push((title.to_owned(), message.to_owned()));
        if let Some(hook) = lock(&self.on_confirm).as_mut() {
            hook();
        }
        self.confirm_answer.load(Ordering::SeqCst)
    }

    fn show_message(&self, title: &str, message: &str) {
        lock(&self.messages).push((title.to_owned(), message.to_owned()));
    }

    fn show_error(&self, title: &str, message: &str) {
        lock(&self.errors).push((title.to_owned(), message.to_owned()));
    }

    fn pick_folder(&self, initial_folder: Option<&Path>) -> Option<PathBuf> {
        lock(&self.pick_initials).push(initial_folder.map(Path::to_path_buf));
        lock(&self.picked).clone()
    }

    fn show_batch_review(&self, items: &[BatchReviewItem]) -> bool {
        lock(&self.batch_reviews).push(items.to_vec());
        if let Some(hook) = lock(&self.on_batch_review).as_mut() {
            hook();
        }
        self.batch_review_answer.load(Ordering::SeqCst)
    }

    fn show_recovery(&self) {
        self.recovery_shown.fetch_add(1, Ordering::SeqCst);
    }

    fn show_diagnostics(&self) {
        self.diagnostics_shown.fetch_add(1, Ordering::SeqCst);
    }

    fn show_settings(&self) -> bool {
        self.show_settings_at(SettingsTarget::Default)
    }

    fn show_settings_at(&self, target: SettingsTarget) -> bool {
        lock(&self.settings_targets).push(target);
        match lock(&self.on_settings).as_mut() {
            Some(hook) => hook(target),
            None => false,
        }
    }

    fn show_benchmark(&self, folder: Option<&Path>) {
        lock(&self.benchmark_folders).push(folder.map(Path::to_path_buf));
    }

    fn show_skipped_files(&self, entries: &[SkippedEntry]) {
        lock(&self.skipped_shown).push(entries.to_vec());
    }
}

/// `(title, initial folder)` of one picker call.
pub type PickerCall = (String, Option<String>);

/// The Move-to / Copy-to folder picker: answers a fixed folder and records the folder it was asked to start in.
#[derive(Clone, Default)]
pub struct TestPicker {
    pub answer: Arc<Mutex<Option<String>>>,
    pub calls: Arc<Mutex<Vec<PickerCall>>>,
}

impl IFolderPicker for TestPicker {
    fn pick_folder(&mut self, title: &str, initial_folder: Option<&str>) -> Option<String> {
        lock(&self.calls).push((title.to_owned(), initial_folder.map(str::to_owned)));
        lock(&self.answer).clone()
    }
}

// ---- instance ownership -----------------------------------------------------------------------------------------------------

/// C# `RecordingOwnership`: scripted decisions per folder (default `Proceed`), every call recorded, the thread of `before_open` noted.
#[derive(Default)]
pub struct RecordingOwnership {
    decisions: Mutex<HashMap<String, FolderOpenDecision>>,
    pub before_opens: Mutex<Vec<String>>,
    pub before_threads: Mutex<Vec<ThreadId>>,
    pub initial_paths: Mutex<Vec<Option<String>>>,
    /// When set, `before_open` fails with this error (the negotiation broke).
    pub fail_with: Mutex<Option<FolderOwnershipError>>,
    pub shown: Mutex<Vec<String>>,
    pub after_opens: Mutex<Vec<(String, Option<String>)>>,
}

impl RecordingOwnership {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn decide(&self, folder: &str, decision: FolderOpenDecision) {
        lock(&self.decisions).insert(folder.to_owned(), decision);
    }

    pub fn before_opens(&self) -> Vec<String> {
        lock(&self.before_opens).clone()
    }

    pub fn shown(&self) -> Vec<String> {
        lock(&self.shown).clone()
    }

    pub fn after_opens(&self) -> Vec<(String, Option<String>)> {
        lock(&self.after_opens).clone()
    }
}

impl FolderOwnership for RecordingOwnership {
    fn before_open(
        &self,
        folder: &str,
        initial_path: Option<&str>,
        _cancel: &CancelToken,
    ) -> Result<FolderOpenDecision, FolderOwnershipError> {
        lock(&self.before_opens).push(folder.to_owned());
        lock(&self.initial_paths).push(initial_path.map(str::to_owned));
        lock(&self.before_threads).push(std::thread::current().id());
        if let Some(error) = *lock(&self.fail_with) {
            return Err(error);
        }
        Ok(lock(&self.decisions)
            .get(folder)
            .copied()
            .unwrap_or(FolderOpenDecision::Proceed))
    }

    fn on_folder_shown(&self, folder: &str) {
        lock(&self.shown).push(folder.to_owned());
    }

    fn after_open(&self, folder: &str, shown_folder: Option<&str>) {
        lock(&self.after_opens).push((folder.to_owned(), shown_folder.map(str::to_owned)));
    }
}

/// A debounce timer that never fires: a started timer ends only when it is cancelled (by `flush` or `dispose`), so the session is
/// written exactly when a test flushes it and never concurrently with the test's own reads (no timer thread racing a reopen).
#[derive(Debug, Default)]
pub struct NeverFiringTimer;

struct WaitForCancel {
    cancel: CancelToken,
}

impl RunningTimer for WaitForCancel {
    fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError> {
        while !self.cancel.is_canceled() {
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(TimerOutcome::Canceled)
    }
}

impl DebounceTimer for NeverFiringTimer {
    fn start(&self, _span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        Ok(Box::new(WaitForCancel { cancel: cancel.clone() }))
    }
}

// ---- preload controller -----------------------------------------------------------------------------------------------------

/// The preload seam with an ORDERED event log (`cancel`, `preload:<i>`, `clear`, `evict:<path>`): the tests assert that `cancel` comes
/// before `preload_around` (R2-F-01). `notify_navigation` goes to its own list (the presenter calls it on every navigation).
#[derive(Default)]
pub struct EventPreload {
    events: Mutex<Vec<String>>,
    notified: Mutex<Vec<i32>>,
}

impl std::fmt::Debug for EventPreload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventPreload").finish_non_exhaustive()
    }
}

impl EventPreload {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn events(&self) -> Vec<String> {
        lock(&self.events).clone()
    }

    pub fn clear_events(&self) {
        lock(&self.events).clear();
    }

    pub fn notified(&self) -> Vec<i32> {
        lock(&self.notified).clone()
    }

    pub fn count_of(&self, event: &str) -> usize {
        lock(&self.events).iter().filter(|recorded| *recorded == event).count()
    }

    pub fn preload_centers(&self) -> Vec<i32> {
        lock(&self.events)
            .iter()
            .filter_map(|event| event.strip_prefix("preload:").and_then(|center| center.parse().ok()))
            .collect()
    }
}

impl IPreloadController for EventPreload {
    fn preload_around(&self, center: i32) -> RunHandle {
        lock(&self.events).push(format!("preload:{center}"));
        RunHandle::completed()
    }

    fn try_consume_preloaded_key(&self, _key: &ImageCacheKey) -> bool {
        false
    }

    fn cancel(&self) {
        lock(&self.events).push("cancel".to_owned());
    }

    fn remove_preloaded_keys_for_path(&self, normalized_path: &str) {
        lock(&self.events).push(format!("evict:{normalized_path}"));
    }

    fn clear_preloaded_keys(&self) {
        lock(&self.events).push("clear".to_owned());
    }

    fn notify_navigation(&self, index: i32) {
        lock(&self.notified).push(index);
    }
}

// ---- recycle bin ------------------------------------------------------------------------------------------------------------

/// C# `FakeRecycleBin`: "recycling" moves the file into a folder of the test root (nothing outside it is touched, AGENTS.md);
/// `no_bin` simulates a removable / network drive, `fail_send` a locked file.
pub struct TestBin {
    dir: PathBuf,
    pub no_bin: AtomicBool,
    pub fail_send: AtomicBool,
    pub recycled: Mutex<Vec<String>>,
    /// Paths whose recycling fails (a locked file), compared ignoring case.
    failing: Mutex<std::collections::HashSet<String>>,
    /// Paths on a "drive without a Recycle Bin" (the others keep their bin), compared ignoring case.
    no_bin_paths: Mutex<std::collections::HashSet<String>>,
    pub permanently_deleted: Mutex<Vec<String>>,
    pub restore_calls: AtomicUsize,
    stored: Mutex<HashMap<String, PathBuf>>,
    counter: AtomicUsize,
}

impl std::fmt::Debug for TestBin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestBin").finish_non_exhaustive()
    }
}

impl TestBin {
    pub fn new(dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            dir,
            no_bin: AtomicBool::new(false),
            fail_send: AtomicBool::new(false),
            recycled: Mutex::new(Vec::new()),
            failing: Mutex::new(std::collections::HashSet::new()),
            no_bin_paths: Mutex::new(std::collections::HashSet::new()),
            permanently_deleted: Mutex::new(Vec::new()),
            restore_calls: AtomicUsize::new(0),
            stored: Mutex::new(HashMap::new()),
            counter: AtomicUsize::new(0),
        })
    }

    /// Makes `path` count as lying on a drive without a Recycle Bin.
    pub fn no_bin_for(&self, path: &str) {
        lock(&self.no_bin_paths).insert(path.to_lowercase());
    }

    fn lacks_bin(&self, path: &Path) -> bool {
        self.no_bin.load(Ordering::SeqCst) || lock(&self.no_bin_paths).contains(&Self::key(path))
    }

    /// Makes the recycling of `path` fail with an I/O error.
    pub fn fail_for(&self, path: &str) {
        lock(&self.failing).insert(path.to_lowercase());
    }

    pub fn recycled(&self) -> Vec<String> {
        lock(&self.recycled).clone()
    }

    pub fn permanently_deleted(&self) -> Vec<String> {
        lock(&self.permanently_deleted).clone()
    }

    pub fn restore_calls(&self) -> usize {
        self.restore_calls.load(Ordering::SeqCst)
    }

    fn key(path: &Path) -> String {
        path.to_string_lossy().to_lowercase()
    }
}

impl RecycleBin for TestBin {
    fn send_to_recycle_bin(&self, path: &Path) -> std::io::Result<()> {
        if self.lacks_bin(path) {
            return Err(std::io::Error::other("no recycle bin"));
        }
        if self.fail_send.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("locked by another process"));
        }
        if lock(&self.failing).contains(&Self::key(path)) {
            return Err(std::io::Error::other("locked by another process"));
        }
        std::fs::create_dir_all(&self.dir)?;
        let number = self.counter.fetch_add(1, Ordering::SeqCst);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = self.dir.join(format!("{number}_{name}"));
        std::fs::rename(path, &target)?;
        lock(&self.recycled).push(path.to_string_lossy().into_owned());
        lock(&self.stored).insert(Self::key(path), target);
        Ok(())
    }

    fn can_recycle(&self, path: &Path) -> bool {
        !self.lacks_bin(path)
    }

    fn delete_permanently(&self, path: &Path) -> std::io::Result<()> {
        if !self.lacks_bin(path) {
            return Err(std::io::Error::other(
                "a drive with a Recycle Bin must never be deleted from permanently",
            ));
        }
        lock(&self.permanently_deleted).push(path.to_string_lossy().into_owned());
        std::fs::remove_file(path)
    }

    fn try_restore(
        &self,
        original_path: &Path,
        _expected_size: u64,
        _expected_last_write_utc: DateTimeUtc,
    ) -> std::io::Result<bool> {
        self.restore_calls.fetch_add(1, Ordering::SeqCst);
        let stored = lock(&self.stored).remove(&Self::key(original_path));
        match stored {
            Some(target) => {
                std::fs::rename(target, original_path)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

// ---- the environment --------------------------------------------------------------------------------------------------------

/// Builds a catalog from the photos folder of the environment.
pub type CatalogBuilder = Box<dyn FnOnce(&Path) -> ReviewCatalog>;

/// Optional parts of a [`MainEnv`].
pub struct EnvOptions {
    pub decoder: Arc<dyn ImageDecoder>,
    pub explorer: Arc<FakeExplorer>,
    pub settings: Box<dyn FnOnce(&mut AppSettings)>,
    /// A catalog filled before the view model exists (the C# tests call `_catalog.Reset` on the object they pass in).
    pub catalog: Option<ReviewCatalog>,
    /// Builds the catalog from the photos folder of the environment (the files must exist under it, so the folder is given).
    pub catalog_builder: Option<CatalogBuilder>,
    /// A scripted executor instead of the real `FileActionService` over the test root.
    pub executor: Option<Arc<dyn IFileActionExecutor + Send + Sync>>,
    /// C# `moveOverride`: replaces the physical move of the service AND of the undo service (fail / observe a move).
    pub move_override: Option<MoveOverride>,
    pub reset_caches: Option<Box<dyn FnMut()>>,
    pub picker: Option<TestPicker>,
    pub session_debounce: Option<Duration>,
    /// Start with the Vietnamese texts.
    pub vietnamese: bool,
    /// The RAW full decoder of the preview service (its use follows the live `raw_full_decode` setting).
    pub raw_full_decoder: Option<Arc<dyn ImageDecoder>>,
}

impl Default for EnvOptions {
    fn default() -> Self {
        Self {
            decoder: SizedDecoder::new(),
            explorer: Arc::new(FakeExplorer::default()),
            settings: Box::new(|_| {}),
            catalog: None,
            catalog_builder: None,
            executor: None,
            move_override: None,
            reset_caches: None,
            picker: None,
            session_debounce: None,
            vietnamese: false,
            raw_full_decoder: None,
        }
    }
}

/// A `MainViewModel` with all its collaborators; the test thread is the UI thread.
pub struct MainEnv {
    pub vm: MainViewModel,
    pub photos: PathBuf,
    pub paths: AppPaths,
    pub runner: Arc<ManualRunner>,
    pub scheduler: Arc<QueuedUiScheduler>,
    pub inbox: CompletionInbox<MainEvent>,
    pub clock: Arc<GenerationClock>,
    pub settings: SettingsSource,
    pub store: Arc<SettingsStore>,
    pub sessions: Arc<SessionStore>,
    pub session_writer: Arc<SessionWriter>,
    pub preload: Arc<EventPreload>,
    pub hasher: Arc<FakeHasher>,
    pub dialog: Arc<TestDialog>,
    pub explorer: Arc<FakeExplorer>,
    pub service: Arc<FileActionService>,
    pub bin: Arc<TestBin>,
    pub log: Arc<RecordingLog>,
    pub gate: FileActionGate,
    pub sink_seen: Arc<Mutex<SinkSeen>>,
    pub viewer: ViewerHandle,
    pub format: Arc<TestFormat>,
    pub bridge: PresentationBridge,
    pub resets: Arc<AtomicUsize>,
    pub picker: TestPicker,
    pub metrics: Arc<ReviewMetrics>,
    pub undo: Arc<Mutex<UndoService>>,
    pub preview_service: Arc<PreviewImageService>,
    /// Merged result of every `settle` / `pump` of this env.
    pub changes: ChangeSet,
    /// LAST on purpose: fields drop in declaration order, and dropping the session writer flushes into this folder (a root removed
    /// first would be re-created by that flush and leak a stale session that a later run with the same name would load).
    pub root: TempRoot,
}

impl std::fmt::Debug for MainEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainEnv").finish_non_exhaustive()
    }
}

/// The RAW full decoder of the service and the live "is it enabled" check.
type RawFullDecode = (Option<Arc<dyn ImageDecoder>>, Arc<dyn Fn() -> bool + Send + Sync>);

/// A preview service whose "original loading mode" follows the live settings (C# `() => _settings.LoadingMode == Original`).
fn build_live_service(
    dir: &Path,
    decoder: Arc<dyn ImageDecoder>,
    original_mode: Arc<dyn Fn() -> bool + Send + Sync>,
    raw_full: RawFullDecode,
    metrics: Arc<ReviewMetrics>,
) -> Arc<PreviewImageService> {
    let mut config = PreviewServiceConfig::new(
        metrics,
        original_mode,
        Arc::new(|| DecodeBox::new(1920, 1080)),
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
    );
    config.capacity_bytes = 64 * MIB;
    config.disk_cache_directory = Some(dir.to_path_buf());
    config.disable_disk_cache_override = Some(true);
    config.decoder = Some(decoder);
    config.physical_memory_bytes = 32 * GIB;
    config.raw_full_decoder = raw_full.0;
    config.is_raw_full_decode_enabled = Some(raw_full.1);
    match PreviewImageService::new(config) {
        Ok(service) => Arc::new(service),
        Err(error) => panic!("the preview service did not build: {error}"),
    }
}

/// The English text of a catalog key with its placeholders filled.
pub fn en_with(key: &str, args: &[pr_l10n::template::LocArg]) -> String {
    let fixture = FormatFixture::new();
    let ctx = fixture.ctx();
    ctx.texts.format_with(key, args, ctx.numbers)
}

/// The three profiles of the C# file-action fixtures: Move to `Sorted`, Copy to `Backup`, Recycle with confirmation.
pub fn default_actions() -> Vec<ReviewAction> {
    let action = |name: &str, operation: FileOperationType, destination: &str, confirm: bool| ReviewAction {
        name: name.to_owned(),
        operation,
        destination: destination.to_owned(),
        confirm,
        ..ReviewAction::new(name)
    };
    vec![
        action("MoveToSub", FileOperationType::Move, "Sorted", false),
        action("CopyToBackup", FileOperationType::Copy, "Backup", false),
        action("ConfirmRecycle", FileOperationType::Recycle, "", true),
    ]
}

/// Writes the settings through the store (`SettingsStore::save`), like the Settings dialog does.
pub fn update_settings(store: &SettingsStore, change: impl FnOnce(&mut AppSettings)) {
    let mut settings = (*store.current()).clone();
    change(&mut settings);
    assert!(store.save(settings).is_ok(), "settings save failed");
}

impl MainEnv {
    pub fn new() -> Self {
        Self::with_options(EnvOptions::default())
    }

    pub fn with_options(options: EnvOptions) -> Self {
        // Unique per environment even when the process id and the counter repeat across runs (a leftover folder of a crashed run).
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let root = TempRoot::new(&format!("main-vm-{stamp}"));
        let photos = root.join("photos");
        std::fs::create_dir_all(&photos).unwrap_or_else(|error| panic!("create photos: {error}"));
        let paths = AppPaths::new(&root.join("appdata"), None, false);
        let fs: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
        let log = Arc::new(RecordingLog::new());
        let log_dyn: Arc<dyn Log> = log.clone();

        let store = Arc::new(SettingsStore::new(
            paths.clone(),
            Arc::clone(&fs),
            Arc::clone(&log_dyn),
            Arc::new(SystemClock),
            Arc::new(EnglishSettingsTexts),
        ));
        update_settings(&store, |settings| {
            settings.loading_mode = Some(LoadingMode::Preview);
            settings.image_sort_mode = Some(ImageSortMode::Name);
            settings.actions = default_actions();
        });
        update_settings(&store, options.settings);
        let settings = SettingsSource::new(Arc::clone(&store));
        let sessions = Arc::new(SessionStore::new(&paths, Arc::clone(&fs), None));
        let session_writer = Arc::new(
            SessionWriter::new(
                Arc::clone(&sessions),
                Some(Arc::clone(&log_dyn)),
                options.session_debounce,
                Some(Arc::new(NeverFiringTimer)),
            )
            .unwrap_or_else(|error| panic!("session writer: {error:?}")),
        );

        // A flush writes on the calling thread (the bounded save worker of production would outlive the flush under CPU load).
        session_writer.set_save_spawner_for_tests(Some(Box::new(|work| {
            work();
            Ok(Completion::completed())
        })));

        let runner = ManualRunner::new();
        let scheduler = Arc::new(QueuedUiScheduler::new());
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_counter = Arc::clone(&wakes);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            wake_counter.fetch_add(1, Ordering::SeqCst);
        });
        let inbox: CompletionInbox<MainEvent> = CompletionInbox::new(Arc::clone(&wake));
        let clock = Arc::new(GenerationClock::default());
        let metrics = Arc::new(ReviewMetrics::new());
        let format = FormatFixture::new();
        let test_format = Arc::new(TestFormat {
            fixture: std::sync::RwLock::new(if options.vietnamese {
                FormatFixture::vietnamese()
            } else {
                FormatFixture::new()
            }),
        });
        let preload = EventPreload::new();
        let preload_dyn: Arc<dyn IPreloadController> = preload.clone();
        let viewer = ViewerHandle::new(ViewerState::new());
        let bridge = PresentationBridge::new(viewer.clone());
        let sink_seen = Arc::new(Mutex::new(SinkSeen::default()));
        let fake_hasher = FakeHasher::new("HASH");
        let hasher: Arc<dyn FileHasher> = fake_hasher.clone();

        let original_mode_settings = settings.clone();
        let raw_full_settings = settings.clone();
        let preview_service = build_live_service(
            &root.join("cache"),
            options.decoder,
            Arc::new(move || original_mode_settings.current().loading_mode_or_default() == LoadingMode::Original),
            (
                options.raw_full_decoder,
                Arc::new(move || raw_full_settings.current().raw_full_decode == pr_core::model::RawFullDecode::OnZoom),
            ),
            Arc::clone(&metrics),
        );
        let thumbnails = build_thumbnail_cache(&root.join("thumbs"));
        let presenter = ImagePresenter::new(
            ImagePresenterDeps {
                clock: Arc::clone(&clock),
                preview_service: Arc::clone(&preview_service),
                thumbnail_cache: Arc::clone(&thumbnails),
                preload: Arc::clone(&preload_dyn),
                hasher: Arc::clone(&hasher),
                metrics: Arc::clone(&metrics),
                session_store: Some(Arc::clone(&sessions)),
                session_writer: Some(Arc::clone(&session_writer)),
                sink: Box::new(BridgeSink {
                    bridge: bridge.clone(),
                    seen: Arc::clone(&sink_seen),
                }),
                file_system: Some(Arc::clone(&fs)),
                on_presented_hook: None,
                ui_scheduler: None,
                busy_retry_delay: Some(Arc::new(|_delay: Duration, _cancel: &CancelToken| true)),
                runner: None,
                stat_worker: Some(NavigationStatWorker::new()),
                wake: Arc::clone(&wake),
                indicator_delay: None,
            },
            CompareViewModel::new(&format.ctx()),
        );

        let explorer = options.explorer;
        let folder_coordinator = FolderLoadCoordinator::new(FolderLoadDeps {
            clock: Arc::clone(&clock),
            explorer_order: Arc::clone(&explorer) as Arc<dyn ExplorerOrderProvider>,
            file_system: Arc::clone(&fs),
            session_store: Arc::clone(&sessions),
            settings_store: Arc::clone(&store),
            session_writer: Some(Arc::clone(&session_writer)),
            latest_explorer: None,
            worker: Arc::new(LanePort::new(Arc::clone(&runner), inbox.clone(), MainEvent::FolderLoad)),
            log: Arc::clone(&log_dyn),
            perf: None,
        });

        let texts: Arc<dyn TextSource> = Arc::new(EnglishTexts::new());
        let dialog = TestDialog::new();
        let dialog_dyn: Arc<dyn DialogService> = dialog.clone();
        let bin = TestBin::new(root.join("bin"));
        let journal = Arc::new(OperationJournal::new(&paths, Arc::clone(&fs), Arc::new(SystemClock)));
        let service = {
            let mut service = FileActionService::new(
                Arc::clone(&journal),
                Arc::clone(&fs),
                Arc::new(SystemClock),
                bin.clone() as Arc<dyn RecycleBin>,
            );
            if let Some(over) = options.move_override.clone() {
                service = service.with_move_override(over);
            }
            Arc::new(service)
        };
        let executor: Arc<dyn IFileActionExecutor + Send + Sync> = options
            .executor
            .clone()
            .unwrap_or_else(|| Arc::new(FileActionServiceExecutor::new(Arc::clone(&service))));
        let undo = {
            let mut undo = UndoService::new(
                Arc::clone(&journal),
                Arc::clone(&fs),
                bin.clone() as Arc<dyn RecycleBin>,
                Arc::new(EnglishUndoTexts),
            )
            .with_gate(Arc::clone(&service) as Arc<dyn ServiceGate>);
            if let Some(over) = options.move_override.clone() {
                undo = undo.with_move_override(over);
            }
            Arc::new(Mutex::new(undo))
        };
        let settings_for_controller = settings.clone();
        let settings_for_remember = settings.clone();
        let log_for_remember = Arc::clone(&log_dyn);
        let file_action_controller = FileActionController::new(FileActionDeps {
            clock: Arc::clone(&clock),
            file_actions: Some(Arc::clone(&executor)),
            undo_service: Some(Arc::clone(&undo)),
            dialog_service: Some(Arc::clone(&dialog_dyn)),
            preload_controller: Some(Arc::clone(&preload_dyn)),
            natural_comparer: Arc::new(ManagedNaturalComparer),
            get_settings: Box::new(move || settings_for_controller.current()),
            file_system: Some(Arc::clone(&fs)),
            remember_folder: Some(Box::new(move |operation: FileOperationType, folder: &str| {
                remember_move_copy_folder(&settings_for_remember, log_for_remember.as_ref(), operation, folder);
            })),
            worker: Arc::new(LanePort::new(Arc::clone(&runner), inbox.clone(), MainEvent::FileAction)),
            texts: Arc::clone(&texts),
            log: Arc::clone(&log_dyn),
        });
        let duplicate_controller = DuplicateCleanupController::new(DuplicateCleanupDeps {
            clock: Arc::clone(&clock),
            file_actions: Some(Arc::clone(&executor)),
            hash_service: Some(Arc::clone(&hasher)),
            file_system: Arc::clone(&fs),
            dialog_service: Some(Arc::clone(&dialog_dyn)),
            preload_controller: Some(Arc::clone(&preload_dyn)),
            thumbnail_cache: Some(Arc::clone(&thumbnails)),
            preview_service: Some(Arc::clone(&preview_service)),
            clear_disk_caches: None,
            worker: Arc::new(LanePort::<DuplicateCleanupEvent>::new(
                Arc::clone(&runner),
                inbox.clone(),
                MainEvent::Duplicate,
            )),
            texts: Arc::clone(&texts),
            log: Arc::clone(&log_dyn),
        });

        let raw_settings = settings.clone();
        let webp_settings = settings.clone();
        let sibling_navigator = Arc::new(SiblingFolderNavigator::new(
            Arc::clone(&clock),
            Arc::clone(&fs),
            Some(Arc::new(move || raw_settings.current().raw_support_enabled)),
            Some(Arc::new(move || webp_settings.current().webp_heic_support_enabled)),
        ));

        let gate = FileActionGate::new(scheduler.clone());
        let resets = Arc::new(AtomicUsize::new(0));
        let reset_counter = Arc::clone(&resets);
        let mut user_reset = options.reset_caches;
        let picker = options.picker.unwrap_or_default();

        let vm = MainViewModel::new(MainViewModelDeps {
            catalog: match options.catalog_builder {
                Some(builder) => builder(&photos),
                None => options.catalog.unwrap_or_default(),
            },
            clock: Arc::clone(&clock),
            folder_coordinator,
            presenter,
            viewer: viewer.clone(),
            bridge: bridge.clone(),
            settings: settings.clone(),
            file_system: Arc::clone(&fs),
            file_action_controller,
            file_action_gate: gate.clone(),
            sibling_navigator,
            duplicate_controller,
            dialog_service: Arc::clone(&dialog_dyn),
            session_writer: Arc::clone(&session_writer),
            undo_service: Arc::clone(&undo),
            preview_service: Arc::clone(&preview_service),
            preload_controller: Some(Arc::clone(&preload_dyn)),
            reset_caches_action: Some(Box::new(move || {
                reset_counter.fetch_add(1, Ordering::SeqCst);
                if let Some(action) = user_reset.as_mut() {
                    action();
                }
            })),
            metrics: Arc::clone(&metrics),
            folder_picker: Some(Box::new(picker.clone())),
            clipboard: None,
            external_editor: None,
            texts,
            format: Arc::clone(&test_format) as Arc<dyn FormatContextSource>,
            inbox: inbox.clone(),
            runner: runner.clone(),
            log: log_dyn,
        });

        Self {
            vm,
            photos,
            paths,
            runner,
            scheduler,
            inbox,
            clock,
            settings,
            store,
            sessions,
            session_writer,
            preload,
            hasher: fake_hasher,
            dialog,
            explorer,
            service,
            bin,
            log,
            gate,
            sink_seen,
            viewer,
            format: test_format,
            bridge,
            resets,
            picker,
            metrics,
            undo,
            preview_service,
            changes: ChangeSet::default(),
            root,
        }
    }

    // ---- files ----

    /// Creates `photos/<name>` (a folder) and returns its path.
    pub fn folder(&self, name: &str) -> String {
        let path = self.photos.join(name);
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create {name}: {error}"));
        path.to_string_lossy().into_owned()
    }

    /// Writes a tiny file (the decoder is a fake) and returns its path.
    pub fn add_image(&self, folder: &str, name: &str) -> String {
        let path = Path::new(folder).join(name);
        std::fs::write(&path, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap_or_else(|error| panic!("write {name}: {error}"));
        path.to_string_lossy().into_owned()
    }

    /// A folder `photos/<name>` holding the given image files; returns `(folder, paths)`.
    pub fn images(&self, name: &str, files: &[&str]) -> (String, Vec<String>) {
        let folder = self.folder(name);
        let paths = files.iter().map(|file| self.add_image(&folder, file)).collect();
        (folder, paths)
    }

    // ---- driving ----

    /// Runs every queued job, pumps the view model, and waits (bounded) for the presenter's worker threads, until nothing is outstanding.
    pub fn settle(&mut self) -> ChangeSet {
        self.settle_until("the view model to become idle", |_| false)
    }

    /// Like [`settle`](Self::settle) but also returns as soon as `done` holds (for states that are never idle, e.g. a command parked
    /// behind the Explorer order). The queued jobs of `skip_tags` stay queued (a held-back lane).
    pub fn settle_until(&mut self, what: &str, done: impl FnMut(&mut MainEnv) -> bool) -> ChangeSet {
        self.settle_holding(what, &[], done)
    }

    pub fn settle_holding(
        &mut self,
        what: &str,
        hold: &[&str],
        mut done: impl FnMut(&mut MainEnv) -> bool,
    ) -> ChangeSet {
        let deadline = Instant::now() + TIMEOUT;
        let mut all = ChangeSet::default();
        loop {
            let mut progressed = false;
            // Run the queued jobs that are not held back, oldest first (a job may queue more).
            while let Some(index) = self.runner.position_not_in(hold) {
                if self.runner.run_at(index) {
                    progressed = true;
                }
            }
            if self.scheduler.run_pending() > 0 {
                progressed = true;
            }
            let changes = self.vm.pump();
            all.merge(changes.clone());
            self.changes.merge(changes);
            if done(self) {
                return all;
            }
            if progressed || self.runner.position_not_in(hold).is_some() {
                continue;
            }
            let settled = if hold.is_empty() {
                self.vm.is_idle() && self.runner.pending() == 0 && self.scheduler.pending() == 0
            } else {
                self.vm.presenter().is_idle() && self.inbox.is_empty()
            };
            if settled {
                return all;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            self.vm.presenter().wait_for_event(Duration::from_millis(20));
        }
    }

    /// One `pump` (events that are already in the inbox / the presenter), merged into [`Self::changes`].
    pub fn pump(&mut self) -> ChangeSet {
        let changes = self.vm.pump();
        self.changes.merge(changes.clone());
        changes
    }

    /// `open_folder` + `settle`.
    pub fn open(&mut self, folder: &str) -> ChangeSet {
        let mut changes = self
            .vm
            .open_folder(folder, None)
            .unwrap_or_else(|error| panic!("open {folder}: {error}"));
        changes.merge(self.settle());
        changes
    }

    /// `open_folder(folder, Some(initial))` + `settle`.
    pub fn open_at(&mut self, folder: &str, initial: &str) -> ChangeSet {
        let mut changes = self
            .vm
            .open_folder(folder, Some(initial))
            .unwrap_or_else(|error| panic!("open {folder}: {error}"));
        changes.merge(self.settle());
        changes
    }

    pub fn update_settings(&self, change: impl FnOnce(&mut AppSettings)) {
        update_settings(&self.store, change);
    }

    /// File names of the catalog, in order.
    pub fn names(&self) -> Vec<String> {
        self.vm
            .catalog()
            .paths()
            .iter()
            .map(|path| {
                Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The text of a language-catalog key (English).
    pub fn text(&self, key: &str) -> String {
        FormatFixture::new().ctx().texts.get(key)
    }

    /// `ctx.texts.format_with(key, args)`.
    pub fn text_with(&self, key: &str, args: &[pr_l10n::template::LocArg]) -> String {
        en_with(key, args)
    }

    /// The directory-name part of a path as the UI shows it.
    pub fn leaf(path: &str) -> String {
        Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

impl Default for MainEnv {
    fn default() -> Self {
        Self::new()
    }
}
