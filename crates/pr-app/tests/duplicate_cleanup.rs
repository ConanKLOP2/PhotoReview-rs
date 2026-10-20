//! Tests of `coordinators::duplicate_cleanup` (W8-07f; contract `contracts/W8-07f.md`, C# baseline `d92d6aae`).
//!
//! Harness: the worker is a manual queue (a job runs only when the test says so, no sleeps, no real threads except in the one
//! test that presses Esc from another thread while a hash blocks), the executor is `FakeExecutor` (nothing touches a disk or a
//! real Recycle Bin), the file system is the in-memory `FakeFileSystem`, the hasher is a counting fake. The catalog, the clock
//! and the sink are real / recording. Differences from the C# tests are named in the comment of the test they concern.

mod file_action_fakes;
mod support {
    pub mod preview_fakes;
}

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use file_action_fakes::{EnglishTexts, FakeExecutor, RecordingLog, Scripted, arg, en, p, single_fail, text};
use pr_app::coordinators::duplicate_cleanup::{
    CacheClearOutcome, DuplicateCleanupController, DuplicateCleanupCx, DuplicateCleanupDeps, DuplicateCleanupEvent,
    DuplicateCleanupStep, HashOutcome, RecycleOutcome,
};
use pr_app::coordinators::folder_load::state::{WorkerLane, WorkerPort};
use pr_app::coordinators::traits::{IDuplicateCleanupSink, IFileActionExecutor, IPreloadController};
use pr_app::misc::file_hash::{FileHashError, FileHasher};
use pr_core::abstractions::{BatchReviewItem, DialogService, FileSystem, SkippedEntry};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CatalogEntry;
use pr_core::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, FileActionRequest, FileActionResult,
};
use pr_core::model::{FileOperationType, RawPairMode};
use pr_core::sync::CancelToken;
use pr_imaging::cache::source_bytes::SourceBytesCache;
use pr_imaging::cache_key::{ImageCacheKey, normalize_key_path};
use pr_imaging::preload::scheduler::RunHandle;
use pr_imaging::service::{PreviewImageService, PreviewServiceConfig};
use pr_l10n::template::LocArg;
use pr_test_support::{FakeFileSystem, FsOp, TempRoot};

use file_action_fakes::ExecutorCall;
use support::preview_fakes::{FakePayloadCodec, GIB, MIB, PixelsCodec, SizedDecoder, TIMEOUT};

// ---- fakes ----------------------------------------------------------------------------------------------------------

type Job = Box<dyn FnOnce() -> DuplicateCleanupEvent + Send>;

/// The worker seam as a manual queue (like `file_action_fakes::ManualPort`, for the duplicate-cleanup events).
#[derive(Default)]
struct DupPort {
    jobs: Mutex<VecDeque<(WorkerLane, Job)>>,
    lanes: Mutex<Vec<WorkerLane>>,
}

impl DupPort {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn queued(&self) -> usize {
        self.jobs.lock().unwrap().len()
    }

    fn take_next(&self) -> Option<(WorkerLane, Job)> {
        self.jobs.lock().unwrap().pop_front()
    }

    /// Runs the oldest queued job on the calling thread and returns its event.
    fn run_next(&self) -> Option<DuplicateCleanupEvent> {
        self.take_next().map(|(_, job)| job())
    }

    fn lanes(&self) -> Vec<WorkerLane> {
        self.lanes.lock().unwrap().clone()
    }
}

impl WorkerPort<DuplicateCleanupEvent> for DupPort {
    fn submit(&self, lane: WorkerLane, job: Job) {
        self.lanes.lock().unwrap().push(lane);
        self.jobs.lock().unwrap().push_back((lane, job));
    }
}

/// C# `ReloadingSink` / `LateStatusSink` in one: `open_folder` clears the status line like the real view model.
struct DupSink {
    statuses: Vec<String>,
    late: Vec<String>,
    reloads: Vec<String>,
    current: Option<String>,
    reload_accepted: bool,
}

impl DupSink {
    fn new() -> Self {
        Self {
            statuses: Vec::new(),
            late: Vec::new(),
            reloads: Vec::new(),
            current: None,
            reload_accepted: true,
        }
    }

    fn last(&self) -> Option<&str> {
        self.statuses.last().map(String::as_str)
    }
}

impl IDuplicateCleanupSink for DupSink {
    fn set_status_text(&mut self, status: &str) {
        self.statuses.push(status.to_string());
        self.current = Some(status.to_string());
    }

    fn show_late_action_status(&mut self, status: &str) {
        self.late.push(status.to_string());
    }

    fn open_folder(&mut self, _folder: &str, _initial_path: Option<&str>) {
        self.current = None;
    }

    fn reload_folder(&mut self, folder: &str) -> bool {
        self.reloads.push(folder.to_string());
        self.current = None;
        self.reload_accepted
    }
}

/// C# `ReviewingDialog` + `RecordingDialog`: scripted review answer, a hook that runs while the review is "open".
struct DupDialog {
    review_answer: AtomicBool,
    reviews: Mutex<Vec<Vec<BatchReviewItem>>>,
    on_review: Mutex<Option<Box<dyn FnMut() + Send>>>,
    confirm_answer: AtomicBool,
    confirmations: Mutex<Vec<(String, String)>>,
    errors: Mutex<Vec<(String, String)>>,
}

impl DupDialog {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            review_answer: AtomicBool::new(true),
            reviews: Mutex::new(Vec::new()),
            on_review: Mutex::new(None),
            confirm_answer: AtomicBool::new(true),
            confirmations: Mutex::new(Vec::new()),
            errors: Mutex::new(Vec::new()),
        })
    }

    fn errors(&self) -> Vec<(String, String)> {
        self.errors.lock().unwrap().clone()
    }
}

impl DialogService for DupDialog {
    fn show_confirmation(&self, title: &str, message: &str) -> bool {
        self.confirmations
            .lock()
            .unwrap()
            .push((title.to_string(), message.to_string()));
        self.confirm_answer.load(Ordering::SeqCst)
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

    fn show_batch_review(&self, items: &[BatchReviewItem]) -> bool {
        self.reviews.lock().unwrap().push(items.to_vec());
        if let Some(hook) = self.on_review.lock().unwrap().as_mut() {
            hook();
        }
        self.review_answer.load(Ordering::SeqCst)
    }

    fn show_recovery(&self) {}

    fn show_diagnostics(&self) {}

    fn show_settings(&self) -> bool {
        false
    }

    fn show_benchmark(&self, _folder: Option<&Path>) {}

    fn show_skipped_files(&self, _entries: &[SkippedEntry]) {}
}

type HashHook = Box<dyn FnMut(usize, &CancelToken) + Send>;

/// C# `InstantHasher` / `BlockingHasher`: every call is counted; the answer is "same" unless a file name is scripted.
#[derive(Default)]
struct FakeHasher {
    calls: Mutex<Vec<String>>,
    results: Mutex<HashMap<String, Result<String, io::ErrorKind>>>,
    hook: Mutex<Option<HashHook>>,
    /// The first call blocks until its token is canceled (a hash that is still reading when Esc is pressed).
    block_first_until_canceled: AtomicBool,
    /// Answers even when the token fired (a reader that does not look at the token).
    ignore_cancellation: AtomicBool,
    started: (Mutex<usize>, Condvar),
}

impl FakeHasher {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn fail_for(&self, file_name: &str) {
        self.results
            .lock()
            .unwrap()
            .insert(file_name.to_string(), Err(io::ErrorKind::PermissionDenied));
    }

    fn set_hook(&self, hook: impl FnMut(usize, &CancelToken) + Send + 'static) {
        *self.hook.lock().unwrap() = Some(Box::new(hook));
    }

    /// Waits (bounded) until the first hash call started.
    fn wait_first_started(&self) {
        let (count, changed) = &self.started;
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut guard = count.lock().unwrap_or_else(PoisonError::into_inner);
        while *guard == 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "the first hash never started");
            guard = changed
                .wait_timeout(guard, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl FileHasher for FakeHasher {
    fn get(&self, path: &Path, cancel: &CancelToken) -> Result<String, FileHashError> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let number = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(name.clone());
            calls.len()
        };
        {
            let (count, changed) = &self.started;
            *count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            changed.notify_all();
        }
        if let Some(hook) = self.hook.lock().unwrap().as_mut() {
            hook(number, cancel);
        }
        if number == 1 && self.block_first_until_canceled.load(Ordering::SeqCst) {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !cancel.is_canceled() {
                if Instant::now() > deadline {
                    return Err(FileHashError::Io(Arc::new(io::Error::other(
                        "test guard: never canceled",
                    ))));
                }
                std::thread::yield_now();
            }
        }
        if cancel.is_canceled() && !self.ignore_cancellation.load(Ordering::SeqCst) {
            return Err(FileHashError::Canceled);
        }
        match self.results.lock().unwrap().get(&name) {
            Some(Err(kind)) => Err(FileHashError::Io(Arc::new(io::Error::from(*kind)))),
            Some(Ok(hash)) => Ok(hash.clone()),
            None => Ok("same".to_string()),
        }
    }
}

/// C# `ThrowingExecutor`: every `execute` call panics (an unexpected exception); the calls are counted.
struct ThrowingExecutor {
    calls: AtomicUsize,
}

impl IFileActionExecutor for ThrowingExecutor {
    fn is_busy(&self) -> bool {
        false
    }

    fn lacks_recycle_bin(&self, _path: &str) -> bool {
        false
    }

    fn execute(&self, _request: &FileActionRequest, _cancel: &CancelToken) -> FileActionResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("boom");
    }

    fn execute_group(&self, _request: &CaptureGroupActionRequest, _cancel: &CancelToken) -> CaptureGroupActionResult {
        panic!("boom");
    }
}

/// C# `RecordingPreload`: the keys removed for a path, and the order of the calls (for the clear-cache order test).
#[derive(Default)]
struct KeyPreload {
    removed_keys: Mutex<Vec<String>>,
    events: Arc<Mutex<Vec<String>>>,
}

impl IPreloadController for KeyPreload {
    fn preload_around(&self, _center: i32) -> RunHandle {
        RunHandle::completed()
    }

    fn try_consume_preloaded_key(&self, _key: &ImageCacheKey) -> bool {
        false
    }

    fn cancel(&self) {
        self.events.lock().unwrap().push("preload.cancel".to_string());
    }

    fn remove_preloaded_keys_for_path(&self, normalized_path: &str) {
        self.removed_keys.lock().unwrap().push(normalized_path.to_string());
    }

    fn clear_preloaded_keys(&self) {
        self.events.lock().unwrap().push("preload.clear_keys".to_string());
    }
}

// ---- the rig --------------------------------------------------------------------------------------------------------

struct Options {
    dialog: bool,
    executor: bool,
    hash_service: bool,
    preview: Option<Arc<PreviewImageService>>,
    clear_disk: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The list the preload fake logs its calls into (shared with other fakes to check an order).
    events: Option<Arc<Mutex<Vec<String>>>>,
    /// Replaces the scripted executor (C# `FileActionsOverride`).
    executor_override: Option<Arc<dyn IFileActionExecutor + Send + Sync>>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dialog: false,
            executor: true,
            hash_service: true,
            preview: None,
            clear_disk: None,
            events: None,
            executor_override: None,
        }
    }
}

struct Rig {
    catalog: ReviewCatalog,
    clock: Arc<GenerationClock>,
    sink: DupSink,
    dialog: Arc<DupDialog>,
    exec: Arc<FakeExecutor>,
    port: Arc<DupPort>,
    preload: Arc<KeyPreload>,
    log: Arc<RecordingLog>,
    hasher: Arc<FakeHasher>,
    fs: Arc<FakeFileSystem>,
    controller: DuplicateCleanupController,
}

impl Rig {
    fn new(options: Options) -> Self {
        let clock = Arc::new(GenerationClock::new(0, 0, 0));
        let dialog = DupDialog::new();
        let exec = FakeExecutor::new();
        let port = DupPort::new();
        let preload = Arc::new(KeyPreload {
            removed_keys: Mutex::new(Vec::new()),
            events: options.events.clone().unwrap_or_default(),
        });
        let log = Arc::new(RecordingLog::default());
        let hasher = FakeHasher::new();
        let fs = Arc::new(FakeFileSystem::new());
        let controller = DuplicateCleanupController::new(DuplicateCleanupDeps {
            clock: clock.clone(),
            file_actions: options.executor_override.or_else(|| {
                options
                    .executor
                    .then(|| exec.clone() as Arc<dyn IFileActionExecutor + Send + Sync>)
            }),
            hash_service: options.hash_service.then(|| hasher.clone() as Arc<dyn FileHasher>),
            file_system: fs.clone() as Arc<dyn FileSystem>,
            dialog_service: options.dialog.then(|| dialog.clone() as Arc<dyn DialogService>),
            preload_controller: Some(preload.clone()),
            thumbnail_cache: None,
            preview_service: options.preview,
            clear_disk_caches: options.clear_disk,
            worker: port.clone(),
            texts: Arc::new(EnglishTexts::new()),
            log: log.clone(),
        });
        Self {
            catalog: ReviewCatalog::new(),
            clock,
            sink: DupSink::new(),
            dialog,
            exec,
            port,
            preload,
            log,
            hasher,
            fs,
            controller,
        }
    }

    fn with_dialog() -> Self {
        Self::new(Options {
            dialog: true,
            ..Options::default()
        })
    }

    /// Plain photos of 2048 bytes each (same size: every file is hashed), in the given order.
    fn load(&mut self, names: &[&str]) {
        let entries = names
            .iter()
            .map(|name| {
                self.fs.add_file(p(name), &[0u8; 2048]);
                let mut entry = CatalogEntry::new(p(name)).expect("path");
                entry.length = Some(2048);
                entry
            })
            .collect();
        self.catalog.reset(entries, RawPairMode::PreferJpeg, None);
    }

    /// `a.jpg` is kept, `a (1).jpg` and `a (2).jpg` are the numbered copies of the batch.
    fn load_three(&mut self) {
        self.load(&["a.jpg", "a (1).jpg", "a (2).jpg"]);
    }

    fn remove_duplicates(&mut self, remove_numbered: bool) -> DuplicateCleanupStep {
        let mut cx = DuplicateCleanupCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.remove_duplicates(&mut cx, remove_numbered)
    }

    fn clear_cache(&mut self) -> DuplicateCleanupStep {
        let mut cx = DuplicateCleanupCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.clear_cache(&mut cx)
    }

    fn handle(&mut self, event: DuplicateCleanupEvent) -> DuplicateCleanupStep {
        let mut cx = DuplicateCleanupCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.handle_event(&mut cx, event)
    }

    fn reload_finished(&mut self, still_current: bool) -> DuplicateCleanupStep {
        let mut cx = DuplicateCleanupCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.controller.on_reload_finished(&mut cx, still_current)
    }

    /// Runs the oldest queued job and delivers its event.
    fn step(&mut self) -> DuplicateCleanupStep {
        let event = self.port.run_next().expect("a queued job");
        self.handle(event)
    }

    /// Runs queued jobs (each delivery may queue the next one) until none is left; returns the last step.
    fn pump(&mut self) -> DuplicateCleanupStep {
        let mut last = DuplicateCleanupStep::Pending;
        while let Some(event) = self.port.run_next() {
            last = self.handle(event);
        }
        last
    }

    /// `remove_duplicates` followed by every job it leads to (hash, then one recycle after the other).
    fn run_batch(&mut self, remove_numbered: bool) -> DuplicateCleanupStep {
        assert_eq!(self.remove_duplicates(remove_numbered), DuplicateCleanupStep::Pending);
        self.pump()
    }

    fn recycled_paths(&self) -> Vec<String> {
        self.exec
            .recorded()
            .into_iter()
            .map(|call| match call {
                ExecutorCall::Single(request) => {
                    assert_eq!(request.operation, FileOperationType::Recycle);
                    assert!(!request.allow_permanent_delete, "a batch never deletes permanently");
                    request.source
                }
                ExecutorCall::Group(_) => panic!("the batch uses single requests"),
            })
            .collect()
    }
}

fn batch_done(succeeded: i32, failures: i32) -> String {
    en(
        "status.batchDone",
        &[LocArg::new("succeeded", succeeded), LocArg::new("failures", failures)],
    )
}

fn late_recycled(done: i32, failed: i32) -> String {
    en(
        "status.lateBatchRecycled",
        &[LocArg::new("done", done), LocArg::new("failed", failed)],
    )
}

/// A preview service whose disk cache is ON (C# `diskCacheDirectory: previewDir`, `sourceBytesCache`).
fn disk_preview_service(dir: &Path, source_bytes: Option<Arc<SourceBytesCache>>) -> Arc<PreviewImageService> {
    let mut config = PreviewServiceConfig::new(
        Arc::new(pr_core::diagnostics::metrics::ReviewMetrics::new()),
        Arc::new(|| false),
        Arc::new(|| pr_imaging::decode_types::DecodeBox::new(1920, 0)),
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
    );
    config.capacity_bytes = 16 * MIB;
    config.disk_cache_directory = Some(dir.to_path_buf());
    config.disable_disk_cache_override = Some(false);
    config.decoder = Some(SizedDecoder::new());
    config.physical_memory_bytes = 32 * GIB;
    config.source_bytes_cache = source_bytes;
    match PreviewImageService::new(config) {
        Ok(service) => Arc::new(service),
        Err(error) => panic!("the preview service did not build: {error}"),
    }
}

// ---- DuplicateCleanupControllerTests ---------------------------------------------------------------------------------

// C#: DuplicateCleanupControllerTests.ClearCacheAsync_ClearsSourceBytesCacheAndPreviewDisk
#[test]
fn clear_cache_clears_source_bytes_cache_and_preview_disk() {
    let root = TempRoot::new("dup-clear-cache");
    let photo = root.join("photo.bin");
    std::fs::write(&photo, vec![0u8; 4096]).expect("photo");
    let source_bytes = Arc::new(SourceBytesCache::new(MIB, 32 * GIB, None).expect("cache"));
    source_bytes.get_or_read(&photo).expect("read");
    assert_eq!(source_bytes.count(), 1);

    let preview_dir = root.join("preview");
    std::fs::create_dir_all(&preview_dir).expect("dir");
    let preview = disk_preview_service(&preview_dir, Some(source_bytes.clone()));
    assert!(preview.wait_startup_cleanup(TIMEOUT));
    let cached = preview_dir.join("entry.pv4");
    std::fs::write(&cached, vec![0u8; 128]).expect("cached entry");

    let mut rig = Rig::new(Options {
        preview: Some(preview.clone()),
        ..Options::default()
    });
    assert_eq!(rig.clear_cache(), DuplicateCleanupStep::Pending);
    assert_eq!(rig.port.lanes(), vec![WorkerLane::CacheClear]);
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);

    assert_eq!(source_bytes.count(), 0);
    assert!(!cached.exists());
    assert_eq!(rig.sink.statuses, vec![text("status.cacheCleared")]);
    assert!(preview.shutdown_persist_workers_timeout(TIMEOUT));
    assert!(preview.wait_for_prune(TIMEOUT));
}

// C#: DuplicateCleanupControllerTests.RemoveDuplicatesAsync_FolderChangesBeforeHashing_HashesNothing_AndReportsFolderChanged
// Difference: the C# hook switches the folder during the stat pass; here the folder changes after the check started and before
// the worker reaches its first hash (same guard: the hash closure checks the generation first).
#[test]
fn folder_changes_before_hashing_hashes_nothing_and_reports_folder_changed() {
    let mut rig = Rig::new(Options::default());
    rig.load(&["a.jpg", "b.jpg", "c.jpg"]);

    assert_eq!(rig.remove_duplicates(false), DuplicateCleanupStep::Pending);
    rig.clock.next_folder();
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);

    assert_eq!(rig.hasher.calls(), 0);
    assert_eq!(
        rig.sink.statuses,
        vec![
            text("status.duplicateCheckRunning"),
            text("status.duplicateCheckCanceledFolderChanged"),
        ]
    );
    assert!(rig.exec.recorded().is_empty());
}

// C#: DuplicateCleanupControllerTests.RemoveDuplicatesAsync_BatchDoneStatus_SurvivesFolderReload
#[test]
fn batch_done_status_survives_folder_reload() {
    let mut rig = Rig::new(Options::default());
    rig.load(&["a.jpg", "a (1).jpg"]);

    // The recycle is done; the reload is requested FIRST and the status is written after it (reload clears the status line).
    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.sink.reloads, vec!["photos".to_string()]);
    assert_eq!(rig.sink.current, None);
    assert_eq!(rig.reload_finished(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.recycled_paths(), vec![p("a (1).jpg")]);
    assert_eq!(rig.sink.current, Some(batch_done(1, 0)));
    assert_eq!(
        rig.fs.count(FsOp::Delete),
        0,
        "the controller never deletes files itself"
    );
}

// C#: DuplicateCleanupControllerTests.RemoveDuplicatesAsync_FolderOpenedDuringReload_WritesNothingIntoNewFolder
#[test]
fn folder_opened_during_reload_writes_nothing_into_new_folder() {
    let mut rig = Rig::new(Options::default());
    rig.load(&["a.jpg", "a (1).jpg"]);

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.reload_finished(false), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.reloads.len(), 1);
    assert_eq!(rig.sink.current, None); // no "Batch done" written over the folder the user opened meanwhile
    assert_eq!(rig.sink.late, vec![late_recycled(1, 0)]);
}

// C#: DuplicateCleanupControllerTests.RemoveDuplicatesAsync_EvictsRecycledFilesFromPreloadKeys
#[test]
fn evicts_recycled_files_from_preload_keys() {
    let root = TempRoot::new("dup-evict");
    let preview = support::preview_fakes::build_service(
        root.path(),
        SizedDecoder::new(),
        support::preview_fakes::ServiceOptions::default(),
    );
    let mut rig = Rig::new(Options {
        preview: Some(preview),
        ..Options::default()
    });
    rig.load(&["a.jpg", "a (1).jpg"]);

    rig.run_batch(true);

    let expected = normalize_key_path(&p("a (1).jpg")).expect("normalized");
    assert_eq!(*rig.preload.removed_keys.lock().unwrap(), vec![expected]);
}

// C#: DuplicateCleanupControllerTests.RemoveDuplicates_FolderChangedDuringBatch_ReportsLateCompletion
// Difference: the C# bin hook switches the folder while the first recycle runs; here the folder changes between the first and the
// second recycle (the controller sees it at the end of the batch in both).
#[test]
fn folder_changed_during_batch_reports_late_completion() {
    let mut rig = Rig::new(Options::default());
    rig.load(&["a.jpg", "a (1).jpg", "a (2).jpg", "a (3).jpg"]);

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.step(), DuplicateCleanupStep::Pending); // hash -> first recycle submitted
    assert_eq!(rig.step(), DuplicateCleanupStep::Pending); // first recycle done
    rig.clock.next_folder(); // the user opens another folder after the first of three recycles
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);

    assert_eq!(rig.exec.recorded().len(), 3);
    assert_eq!(rig.sink.late, vec![late_recycled(3, 0)]);
    assert!(rig.sink.reloads.is_empty());
    assert!(!rig.sink.statuses.contains(&batch_done(3, 0)));
}

// C#: DuplicateCleanupControllerTests.RemoveDuplicates_OneFileFails_StatusCountsTheFailure
#[test]
fn one_file_fails_status_counts_the_failure() {
    let mut rig = Rig::new(Options::default());
    rig.load(&["a.jpg", "a (1).jpg", "a (2).jpg", "a (3).jpg"]);
    // The second of the three recycles fails (the first and the third succeed).
    rig.exec.script(Scripted::Single(file_action_fakes::single_ok(
        FileOperationType::Recycle,
        &p("a (1).jpg"),
        None,
    )));
    rig.exec.script(Scripted::Single(single_fail(
        FileOperationType::Recycle,
        &p("a (2).jpg"),
        "locked by another process",
    )));

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.reload_finished(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.last(), Some(batch_done(2, 1).as_str()));
}

// ---- DuplicateCleanupControllerReviewGapTests ---------------------------------------------------------------------------

// C#: DuplicateCleanupControllerReviewGapTests.RemoveDuplicatesAsync_ReviewConfirmedButOneFileLeftTheCatalog_RecyclesOnlyTheRemainingOne
// Difference: the C# dialog hook removes a file from the catalog while the review is open (nested dispatcher loop). Here the
// catalog is borrowed by the call and cannot change under the dialog, so the file leaves the catalog between the hash job and
// its event; the re-filter after the dialog is the same code.
#[test]
fn review_confirmed_but_one_file_left_the_catalog_recycles_only_the_remaining_one() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    let hashed = rig.port.run_next().expect("hash job");
    rig.catalog.remove(&p("a (1).jpg"));
    assert_eq!(rig.handle(hashed), DuplicateCleanupStep::Pending);

    let reviews = rig.dialog.reviews.lock().unwrap().clone();
    assert_eq!(reviews.len(), 1);
    let offered: Vec<PathBuf> = reviews[0].iter().map(|item| item.path.clone()).collect();
    assert_eq!(
        offered,
        vec![PathBuf::from(p("a (1).jpg")), PathBuf::from(p("a (2).jpg"))]
    );
    assert_eq!(
        reviews[0][0].length, None,
        "unknown size for a file that left the catalog"
    );
    assert_eq!(reviews[0][1].length, Some(2048));

    assert_eq!(rig.pump(), DuplicateCleanupStep::Pending);
    assert_eq!(rig.reload_finished(true), DuplicateCleanupStep::Done);
    assert_eq!(rig.recycled_paths(), vec![p("a (2).jpg")]);
    assert_eq!(rig.sink.last(), Some(batch_done(1, 0).as_str()));
}

// C#: DuplicateCleanupControllerReviewGapTests.RemoveDuplicatesAsync_ReviewConfirmedButEveryFileLeftTheCatalog_RecyclesNothingAndReportsBatchCanceled
// Difference: same as the test above (the catalog changes before the event instead of inside the dialog).
#[test]
fn review_confirmed_but_every_file_left_the_catalog_recycles_nothing_and_reports_batch_canceled() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    let hashed = rig.port.run_next().expect("hash job");
    rig.catalog.remove(&p("a (1).jpg"));
    rig.catalog.remove(&p("a (2).jpg"));

    assert_eq!(rig.handle(hashed), DuplicateCleanupStep::Done);

    assert!(rig.exec.recorded().is_empty());
    assert_eq!(rig.sink.last(), Some(text("status.batchCanceled").as_str()));
    assert!(rig.sink.reloads.is_empty());
    assert_eq!(rig.port.queued(), 0);
}

// ---- DuplicateCleanupControllerMutationGapTests --------------------------------------------------------------------------

// C#: DuplicateCleanupControllerMutationGapTests.RemoveDuplicatesAsync_NoFileActionService_ReturnsWithoutAnyStatus
#[test]
fn no_file_action_service_returns_without_any_status() {
    let mut rig = Rig::new(Options {
        executor: false,
        ..Options::default()
    });
    rig.load_three();

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Done);

    assert!(rig.sink.statuses.is_empty() && rig.sink.late.is_empty());
    assert_eq!(rig.port.queued(), 0);
}

// C#: DuplicateCleanupControllerMutationGapTests.RemoveDuplicatesAsync_EveryRecycleFailsAfterTheFolderChanged_ReportsTheFailureCountLate
#[test]
fn every_recycle_fails_after_the_folder_changed_reports_the_failure_count_late() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    for name in ["a (1).jpg", "a (2).jpg"] {
        rig.exec.script(Scripted::Single(single_fail(
            FileOperationType::Recycle,
            &p(name),
            "locked",
        )));
    }

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.step(), DuplicateCleanupStep::Pending); // hash
    let first = rig.port.run_next().expect("first recycle");
    rig.clock.next_folder(); // the folder changes while the first recycle runs
    assert_eq!(rig.handle(first), DuplicateCleanupStep::Pending);
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.late, vec![late_recycled(0, 2)]);
    assert!(rig.sink.reloads.is_empty());
}

// C#: DuplicateCleanupControllerMutationGapTests.RemoveDuplicatesAsync_EveryRecycleFails_DoesNotReloadTheFolderAndReportsTheFailures
#[test]
fn every_recycle_fails_does_not_reload_the_folder_and_reports_the_failures() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    for name in ["a (1).jpg", "a (2).jpg"] {
        rig.exec.script(Scripted::Single(single_fail(
            FileOperationType::Recycle,
            &p(name),
            "locked",
        )));
    }

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert!(rig.sink.reloads.is_empty());
    assert_eq!(rig.sink.last(), Some(batch_done(0, 2).as_str()));
}

// C#: DuplicateCleanupControllerMutationGapTests.RemoveDuplicatesAsync_AllRecyclesSucceedWithADialog_NeverShowsTheErrorDialog
#[test]
fn all_recycles_succeed_with_a_dialog_never_shows_the_error_dialog() {
    let mut rig = Rig::with_dialog();
    rig.load_three();

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.reload_finished(true), DuplicateCleanupStep::Done);

    assert!(rig.dialog.errors().is_empty());
    assert_eq!(rig.sink.last(), Some(batch_done(2, 0).as_str()));
    assert_eq!(rig.sink.reloads.len(), 1);
}

// C#: DuplicateCleanupControllerMutationGapTests.RemoveDuplicatesAsync_SomeRecyclesFailWithADialog_ShowsOneErrorDialogListingEachFailure
#[test]
fn some_recycles_fail_with_a_dialog_shows_one_error_dialog_listing_each_failure() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    for name in ["a (1).jpg", "a (2).jpg"] {
        rig.exec.script(Scripted::Single(single_fail(
            FileOperationType::Recycle,
            &p(name),
            "locked",
        )));
    }

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    let errors = rig.dialog.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, text("dialog.batchErrors.title"));
    // One line per failure, joined with CRLF (C# `Environment.NewLine` on Windows).
    assert_eq!(errors[0].1, "a (1).jpg: locked\r\na (2).jpg: locked");
}

// C#: DuplicateCleanupControllerMutationGapTests.RemoveDuplicatesAsync_AnUnexpectedThrowForOneFile_DoesNotAbortTheBatchAndIsReported
#[test]
fn an_unexpected_throw_for_one_file_does_not_abort_the_batch_and_is_reported() {
    let throwing = Arc::new(ThrowingExecutor {
        calls: AtomicUsize::new(0),
    });
    let mut rig = Rig::new(Options {
        dialog: true,
        executor_override: Some(throwing.clone()),
        ..Options::default()
    });
    rig.load_three();

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.last(), Some(batch_done(0, 2).as_str()));
    let errors = rig.dialog.errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].1.contains("a (1).jpg: boom"), "{:?}", errors[0].1);
    assert!(
        errors[0].1.contains("a (2).jpg: boom"),
        "the second file was still attempted"
    );
    assert!(rig.sink.reloads.is_empty());
    assert_eq!(throwing.calls.load(Ordering::SeqCst), 2);
    assert_eq!(rig.log.errors.lock().unwrap().len(), 2);
}

// C#: DuplicateCleanupControllerMutationGapTests.RemoveDuplicatesAsync_ACancellationFromTheService_IsNotSwallowedAsAFileFailure
// Difference: a Rust executor cannot throw a cancellation, so the event of the first recycle is replaced by `Canceled`.
#[test]
fn a_cancellation_from_the_service_is_not_swallowed_as_a_file_failure() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.step(), DuplicateCleanupStep::Pending); // hash -> first recycle submitted
    let ticket = match rig.port.run_next().expect("first recycle") {
        DuplicateCleanupEvent::Recycled { ticket, index, .. } => {
            assert_eq!(index, 0);
            ticket
        }
        other => panic!("expected a Recycled event, got {other:?}"),
    };

    let step = rig.handle(DuplicateCleanupEvent::Recycled {
        ticket,
        index: 0,
        outcome: RecycleOutcome::Canceled,
    });

    assert_eq!(step, DuplicateCleanupStep::Canceled);
    assert!(!rig.sink.statuses.contains(&batch_done(0, 2)));
    assert!(!rig.sink.statuses.iter().any(|s| s == &batch_done(0, 1)));
    assert_eq!(rig.port.queued(), 0, "the batch stops");
    assert!(rig.dialog.errors().is_empty());
}

// C#: DuplicateCleanupControllerMutationGapTests.ClearCacheAsync_AnUnexpectedDiskFailure_IsReportedAsFailedNotAsCleared
#[test]
fn clear_cache_an_unexpected_disk_failure_is_reported_as_failed_not_as_cleared() {
    let mut rig = Rig::new(Options {
        clear_disk: Some(Arc::new(|| panic!("disk exploded"))),
        ..Options::default()
    });

    assert_eq!(rig.clear_cache(), DuplicateCleanupStep::Pending);
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);

    let expected = en(
        "status.actionFailed",
        &[
            arg("actionName", &text("dialog.clearCache.title")),
            arg("message", "disk exploded"),
        ],
    );
    assert_eq!(rig.sink.statuses, vec![expected]);
    assert_eq!(*rig.log.errors.lock().unwrap(), vec!["Clear cache failed".to_string()]);
}

// C#: DuplicateCleanupControllerMutationGapTests.ClearCacheAsync_ACancelledDiskClear_PropagatesInsteadOfBeingReported
// Difference: a Rust disk clear cannot throw a cancellation, so the event is replaced by `Canceled`.
#[test]
fn clear_cache_a_cancelled_disk_clear_propagates_instead_of_being_reported() {
    let mut rig = Rig::new(Options::default());
    assert_eq!(rig.clear_cache(), DuplicateCleanupStep::Pending);
    let ticket = match rig.port.run_next().expect("disk clear") {
        DuplicateCleanupEvent::CacheCleared { ticket, .. } => ticket,
        other => panic!("expected a CacheCleared event, got {other:?}"),
    };

    let step = rig.handle(DuplicateCleanupEvent::CacheCleared {
        ticket,
        outcome: CacheClearOutcome::Canceled,
    });

    assert_eq!(step, DuplicateCleanupStep::Canceled);
    assert!(rig.sink.statuses.is_empty());
}

// ---- DuplicateCheckCancelTests (Q-R25) and other behavior of the contract -----------------------------------------------------

// C#: DuplicateCheckCancelTests.CancelDuplicateCheck_BeforeStart_IsNoOp / CancelDuplicateCheck_AfterCompletion_IsNoOp
// New test: `cancel_duplicate_check` is true only while a check is hashing.
#[test]
fn cancel_duplicate_check_is_true_only_while_hashing() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    assert!(!rig.controller.cancel_duplicate_check(), "nothing is running yet");

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert!(rig.controller.cancel_duplicate_check());
    assert!(
        rig.controller.cancel_duplicate_check(),
        "a repeated Esc is consumed too"
    );
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);
    assert_eq!(rig.sink.last(), Some(text("status.duplicateCheckCanceled").as_str()));
    assert!(!rig.controller.cancel_duplicate_check(), "hashing is over");

    // The next check is not poisoned by the earlier Esc.
    rig.sink.statuses.clear();
    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.reload_finished(true), DuplicateCleanupStep::Done);
    assert_eq!(rig.sink.last(), Some(batch_done(2, 0).as_str()));
    assert!(
        !rig.controller.cancel_duplicate_check(),
        "the batch is not reachable by Esc"
    );
}

// C#: DuplicateCheckCancelTests.CancelDuplicateCheck_MidHash_StopsWithoutRecycling
// Esc is pressed from the "UI" thread while the worker thread is blocked inside the first hash.
#[test]
fn cancel_duplicate_check_mid_hash_stops_without_recycling() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    rig.hasher.block_first_until_canceled.store(true, Ordering::SeqCst);

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.sink.last(), Some(text("status.duplicateCheckRunning").as_str()));
    let (lane, job) = rig.port.take_next().expect("hash job");
    assert_eq!(lane, WorkerLane::Hashing);
    let worker = std::thread::spawn(job);
    rig.hasher.wait_first_started();

    assert!(rig.controller.cancel_duplicate_check());
    let event = worker.join().expect("the worker thread finished");
    assert_eq!(rig.handle(event), DuplicateCleanupStep::Done);

    assert_eq!(rig.hasher.calls(), 1, "stopped: the remaining files were never hashed");
    assert!(rig.exec.recorded().is_empty());
    assert_eq!(rig.sink.last(), Some(text("status.duplicateCheckCanceled").as_str()));
    assert!(!rig.controller.cancel_duplicate_check());
}

// C#: DuplicateCheckCancelTests.CancelDuplicateCheck_RacesWithLastHash_DiscardsResult
#[test]
fn esc_as_the_last_hash_finishes_discards_the_result() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    // The token fires during the third (last) hash, which still answers (a reader that ignores the token).
    rig.hasher.ignore_cancellation.store(true, Ordering::SeqCst);
    rig.hasher.set_hook(|number, token| {
        if number == 3 {
            token.cancel();
        }
    });

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);

    assert!(rig.exec.recorded().is_empty());
    assert!(
        rig.dialog.reviews.lock().unwrap().is_empty(),
        "no review for a discarded result"
    );
    assert_eq!(rig.sink.last(), Some(text("status.duplicateCheckCanceled").as_str()));
}

// C#: DuplicateCheckCancelTests.RemoveDuplicatesAsync_FolderSwitchDuringHash_StillCancels
#[test]
fn folder_switch_during_hash_still_cancels_with_the_folder_changed_message() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    let clock = rig.clock.clone();
    rig.hasher.set_hook(move |number, _| {
        if number == 1 {
            clock.next_folder();
        }
    });

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);

    assert_eq!(
        rig.hasher.calls(),
        1,
        "the second same-size file is not read once the folder changed"
    );
    assert!(rig.exec.recorded().is_empty());
    assert_eq!(
        rig.sink.last(),
        Some(text("status.duplicateCheckCanceledFolderChanged").as_str())
    );
    assert!(!rig.controller.cancel_duplicate_check());
}

// C#: DuplicateCheckCancelTests.RemoveDuplicatesAsync_FreshCheckWhileHashing_CancelsPreviousSilently
// New test: `superseded_hash_run_is_silent`.
#[test]
fn superseded_hash_run_is_silent() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending); // run 1 (its job has not started)
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending); // run 2 supersedes it
    let first = rig.port.run_next().expect("job of run 1");
    let second = rig.port.run_next().expect("job of run 2");

    // The superseded run ends first and writes nothing (the newer run owns the status text).
    let statuses_before = rig.sink.statuses.len();
    assert_eq!(rig.handle(first), DuplicateCleanupStep::Pending);
    assert_eq!(rig.sink.statuses.len(), statuses_before);
    assert!(!rig.sink.statuses.contains(&text("status.duplicateCheckCanceled")));

    // Run 2 completes and recycles the two copies once.
    assert_eq!(rig.handle(second), DuplicateCleanupStep::Pending);
    assert_eq!(rig.pump(), DuplicateCleanupStep::Pending);
    assert_eq!(rig.reload_finished(true), DuplicateCleanupStep::Done);
    assert_eq!(rig.recycled_paths().len(), 2);
    assert_eq!(rig.sink.last(), Some(batch_done(2, 0).as_str()));
    assert!(!rig.sink.statuses.contains(&text("status.duplicateCheckCanceled")));
    assert!(!rig.controller.cancel_duplicate_check());
}

// C#: DuplicateCheckCancelTests.CancelDuplicateCheck_DuringBatchReview_IsFalseAndBatchProceeds
#[test]
fn esc_during_the_batch_review_is_false_and_the_batch_proceeds() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    // The dialog cannot reach the controller (it is borrowed by the call): the answer is checked right before and after.
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    let hashed = rig.port.run_next().expect("hash job");
    assert_eq!(rig.handle(hashed), DuplicateCleanupStep::Pending);
    assert!(!rig.controller.cancel_duplicate_check());
    assert_eq!(rig.dialog.reviews.lock().unwrap().len(), 1);
    assert_eq!(rig.pump(), DuplicateCleanupStep::Pending);
    assert_eq!(rig.exec.recorded().len(), 2);
}

// New test (stale folder guard): the folder changed after the hash job finished but before its result was delivered.
#[test]
fn folder_changed_between_the_hash_job_and_its_event_discards_the_list() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    let hashed = rig.port.run_next().expect("hash job");
    rig.clock.next_folder();

    assert_eq!(rig.handle(hashed), DuplicateCleanupStep::Done);

    assert_eq!(
        rig.sink.last(),
        Some(text("status.duplicateCheckCanceledFolderChanged").as_str())
    );
    assert!(rig.dialog.reviews.lock().unwrap().is_empty());
    assert!(rig.exec.recorded().is_empty());
}

// New test (C# `ThrowIfCancellationRequested` after the finder): Esc between the end of the hash job and its event wins.
#[test]
fn esc_between_the_hash_job_and_its_event_discards_the_list() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    let hashed = rig.port.run_next().expect("hash job");
    assert!(
        rig.controller.cancel_duplicate_check(),
        "the check is still registered, Esc is consumed"
    );

    assert_eq!(rig.handle(hashed), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.last(), Some(text("status.duplicateCheckCanceled").as_str()));
    assert!(rig.dialog.reviews.lock().unwrap().is_empty());
    assert!(rig.exec.recorded().is_empty());
}

// New test: a recycle event of another run or of another file index is dropped, the batch continues undisturbed.
#[test]
fn stale_recycle_events_are_ignored() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.step(), DuplicateCleanupStep::Pending); // hash -> first recycle queued
    let ok = || {
        RecycleOutcome::Result(file_action_fakes::single_ok(
            FileOperationType::Recycle,
            &p("a (2).jpg"),
            None,
        ))
    };

    let wrong_ticket = rig.handle(DuplicateCleanupEvent::Recycled {
        ticket: 99,
        index: 0,
        outcome: ok(),
    });
    let wrong_index = rig.handle(DuplicateCleanupEvent::Recycled {
        ticket: 1,
        index: 1,
        outcome: ok(),
    });
    assert_eq!(
        (wrong_ticket, wrong_index),
        (DuplicateCleanupStep::Pending, DuplicateCleanupStep::Pending)
    );
    assert_eq!(rig.port.queued(), 1, "no second job was submitted by the stale events");

    assert_eq!(rig.pump(), DuplicateCleanupStep::Pending);
    assert_eq!(rig.reload_finished(true), DuplicateCleanupStep::Done);
    assert_eq!(rig.sink.last(), Some(batch_done(2, 0).as_str()));
    assert_eq!(rig.recycled_paths().len(), 2);
}

// New test: recycle jobs are submitted one at a time, on the file-action lane, in catalog order.
#[test]
fn recycle_jobs_are_submitted_one_at_a_time() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();

    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.port.queued(), 1);
    assert_eq!(rig.step(), DuplicateCleanupStep::Pending); // hash
    assert_eq!(rig.port.queued(), 1, "only the first recycle is queued");
    assert!(rig.exec.recorded().is_empty());
    assert_eq!(rig.step(), DuplicateCleanupStep::Pending);
    assert_eq!(rig.recycled_paths(), vec![p("a (1).jpg")]);
    assert_eq!(
        rig.port.queued(),
        1,
        "the next one is submitted only after the previous event"
    );
    assert_eq!(rig.step(), DuplicateCleanupStep::Pending); // second recycle; the batch ends with a reload
    assert_eq!(rig.port.queued(), 0);
    assert_eq!(
        rig.port.lanes(),
        vec![WorkerLane::Hashing, WorkerLane::FileAction, WorkerLane::FileAction]
    );
    assert_eq!(rig.recycled_paths(), vec![p("a (1).jpg"), p("a (2).jpg")]);
}

// New test: the reload request was refused -> late status at once, nothing is written into the folder.
#[test]
fn reload_rejected_reports_late_status() {
    let mut rig = Rig::new(Options::default());
    rig.load(&["a.jpg", "a (1).jpg"]);
    rig.sink.reload_accepted = false;

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.late, vec![late_recycled(1, 0)]);
    assert!(!rig.sink.statuses.contains(&batch_done(1, 0)));
}

// New test: an event of an unknown run is dropped.
#[test]
fn stale_ticket_event_is_ignored() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    let statuses = rig.sink.statuses.len();

    let stale = rig.handle(DuplicateCleanupEvent::HashFinished {
        ticket: 999,
        outcome: HashOutcome::Found(vec![p("a (1).jpg")]),
    });
    assert_eq!(stale, DuplicateCleanupStep::Pending);
    assert_eq!(rig.sink.statuses.len(), statuses);
    assert_eq!(rig.port.queued(), 1, "nothing was started by the stale event");

    // A recycle / cache event with no batch / no clear in progress is dropped as well.
    let recycled = rig.handle(DuplicateCleanupEvent::Recycled {
        ticket: 1,
        index: 0,
        outcome: RecycleOutcome::Panicked("late".to_string()),
    });
    assert_eq!(recycled, DuplicateCleanupStep::Pending);
    let cleared = rig.handle(DuplicateCleanupEvent::CacheCleared {
        ticket: 1,
        outcome: CacheClearOutcome::Cleared,
    });
    assert_eq!(cleared, DuplicateCleanupStep::Pending);
    assert_eq!(rig.sink.statuses.len(), statuses);
    assert!(rig.exec.recorded().is_empty());
}

// New test (traps): the guards of `remove_duplicates` that end without a status.
#[test]
fn busy_executor_or_empty_catalog_ends_without_status() {
    let mut rig = Rig::new(Options::default());
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Done, "empty catalog");
    rig.load_three();
    rig.exec.set_busy(true);
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Done, "busy executor");
    assert!(rig.sink.statuses.is_empty());
    assert_eq!(rig.port.queued(), 0);
}

// New test: nothing to remove.
#[test]
fn no_duplicates_found_says_so() {
    let mut rig = Rig::with_dialog();
    rig.load(&["a.jpg", "a (1).jpg"]);
    rig.hasher
        .results
        .lock()
        .unwrap()
        .insert("a (1).jpg".to_string(), Ok("other".to_string()));

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.last(), Some(text("status.noDuplicatesFound").as_str()));
    assert!(rig.dialog.reviews.lock().unwrap().is_empty());
}

// New test (Q-R14): a file without a Recycle Bin refuses the whole batch once, up front.
#[test]
fn a_file_without_a_recycle_bin_refuses_the_batch_up_front() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    rig.exec.add_no_bin_for(&p("a (2).jpg"));

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.last(), Some(text("status.batchCanceled").as_str()));
    let errors = rig.dialog.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, text("dialog.batchErrors.title"));
    assert_eq!(
        errors[0].1,
        en("core.recycle.unsupportedDrive", &[arg("name", "a (2).jpg")])
    );
    assert!(rig.exec.recorded().is_empty());
    assert!(
        rig.dialog.reviews.lock().unwrap().is_empty(),
        "no review before the refusal"
    );
}

// New test: the review answer "No" cancels the batch.
#[test]
fn a_declined_review_reports_batch_canceled() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    rig.dialog.review_answer.store(false, Ordering::SeqCst);

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.last(), Some(text("status.batchCanceled").as_str()));
    assert!(rig.exec.recorded().is_empty());
}

// New test (R7-4): the folder switched while the review was open.
#[test]
fn folder_switched_during_the_review_recycles_nothing() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    let clock = rig.clock.clone();
    *rig.dialog.on_review.lock().unwrap() = Some(Box::new(move || {
        clock.next_folder();
    }));

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert_eq!(
        rig.sink.last(),
        Some(text("status.duplicateCheckCanceledFolderChanged").as_str())
    );
    assert!(rig.exec.recorded().is_empty());
}

// New test (R7-4): another action became busy while the review was open -> silent stop.
#[test]
fn executor_busy_after_the_review_stops_silently() {
    let mut rig = Rig::with_dialog();
    rig.load_three();
    let exec = rig.exec.clone();
    *rig.dialog.on_review.lock().unwrap() = Some(Box::new(move || exec.set_busy(true)));

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.sink.last(), Some(text("status.duplicateCheckRunning").as_str()));
    assert!(rig.exec.recorded().is_empty());
}

// New test: no hash service while a hash is needed.
#[test]
fn a_missing_hash_service_reports_the_failure() {
    let mut rig = Rig::new(Options {
        hash_service: false,
        ..Options::default()
    });
    rig.load_three();

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Done);

    assert_eq!(
        rig.sink.last(),
        Some(
            en(
                "status.duplicateCheckFailed",
                &[arg("message", &text("err.io.hashServiceMissing"))]
            )
            .as_str()
        )
    );
    assert!(rig.exec.recorded().is_empty());
}

// New test: an unreadable file is skipped by the finder, not reported as a failure of the check.
#[test]
fn an_unreadable_file_is_skipped() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    rig.hasher.fail_for("a (1).jpg");

    assert_eq!(rig.run_batch(true), DuplicateCleanupStep::Pending);
    assert_eq!(rig.reload_finished(true), DuplicateCleanupStep::Done);

    assert_eq!(rig.recycled_paths(), vec![p("a (2).jpg")]);
    assert_eq!(rig.sink.last(), Some(batch_done(1, 0).as_str()));
}

// New test: a hasher that reports `Canceled` by itself (folder and token untouched) is a failure of the check.
#[test]
fn a_spontaneous_hash_cancellation_is_reported_as_a_failure() {
    let mut rig = Rig::new(Options::default());
    rig.load_three();
    // The fake only answers Canceled for a fired token, so the worker outcome is injected.
    assert_eq!(rig.remove_duplicates(true), DuplicateCleanupStep::Pending);
    let _ = rig.port.run_next();
    let step = rig.handle(DuplicateCleanupEvent::HashFinished {
        ticket: 1,
        outcome: HashOutcome::Canceled,
    });
    assert_eq!(step, DuplicateCleanupStep::Done);
    assert_eq!(
        rig.sink.last(),
        Some(en("status.duplicateCheckFailed", &[arg("message", "operation canceled")]).as_str()),
    );
}

// New test: the review shows the known sizes.
#[test]
fn the_review_lists_the_files_with_their_catalog_sizes() {
    let mut rig = Rig::with_dialog();
    rig.load_three();

    rig.run_batch(true);

    let reviews = rig.dialog.reviews.lock().unwrap().clone();
    assert_eq!(reviews.len(), 1);
    assert_eq!(
        reviews[0],
        vec![
            BatchReviewItem {
                path: PathBuf::from(p("a (1).jpg")),
                length: Some(2048)
            },
            BatchReviewItem {
                path: PathBuf::from(p("a (2).jpg")),
                length: Some(2048)
            },
        ]
    );
}

// New test (C-05 order): confirmation, then preload cancel, RAM caches, preload keys, and the disk clear last (on the cache lane).
#[test]
fn clear_cache_clears_ram_first_and_the_disk_last() {
    let root = TempRoot::new("dup-clear-order");
    let preview = disk_preview_service(&root.join("preview"), None);
    assert!(preview.wait_startup_cleanup(TIMEOUT));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let for_listener = events.clone();
    let _subscription = preview.subscribe_cache_cleared(Arc::new(move || {
        for_listener.lock().unwrap().push("preview.clear_cache".to_string());
    }));
    let for_disk = events.clone();
    let mut rig = Rig::new(Options {
        dialog: true,
        preview: Some(preview.clone()),
        clear_disk: Some(Arc::new(move || for_disk.lock().unwrap().push("disk".to_string()))),
        events: Some(events.clone()),
        ..Options::default()
    });

    assert_eq!(rig.clear_cache(), DuplicateCleanupStep::Pending);
    assert_eq!(
        *events.lock().unwrap(),
        vec!["preload.cancel", "preview.clear_cache", "preload.clear_keys"],
        "RAM first; the disk clear has not run yet"
    );
    assert_eq!(rig.dialog.confirmations.lock().unwrap().len(), 1);
    assert_eq!(
        rig.dialog.confirmations.lock().unwrap()[0],
        (text("dialog.clearCache.title"), text("dialog.clearCache.message"))
    );
    assert!(rig.sink.statuses.is_empty(), "the status waits for the disk clear");
    assert_eq!(rig.pump(), DuplicateCleanupStep::Done);
    assert_eq!(events.lock().unwrap().last().map(String::as_str), Some("disk"));
    assert_eq!(rig.sink.statuses, vec![text("status.cacheCleared")]);
    assert!(preview.shutdown_persist_workers_timeout(TIMEOUT));
    assert!(preview.wait_for_prune(TIMEOUT));
}

// New test: a declined confirmation clears nothing.
#[test]
fn clear_cache_declined_does_nothing() {
    let mut rig = Rig::with_dialog();
    rig.dialog.confirm_answer.store(false, Ordering::SeqCst);

    assert_eq!(rig.clear_cache(), DuplicateCleanupStep::Done);

    assert_eq!(rig.port.queued(), 0);
    assert!(rig.sink.statuses.is_empty());
    assert!(rig.preload.events.lock().unwrap().is_empty());
}
