//! W8-07a: `FolderLoadCoordinator` state and load (generation token, INV-9). Ports of the C# `FolderLoadCoordinatorTests`
//! (a partial class over several files) and `FolderLoadSkippedFilesTests` that belong to the scan, the catalog application,
//! the first present, the empty folder, the early Explorer order and the drops of stale loads (contract `contracts/W8-07a.md`).
//!
//! Test kind F: hand-written fakes, no oracle. The worker is a [`ManualPort`]: jobs queue up and the test runs them (and
//! delivers their events) when it says so, so every race is an explicit order of `run`/`deliver` calls and nothing sleeps.
//! The late Explorer order, the probe and `dispose` are W8-07b; the tests here only use them as far as the load reaches them.

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_app::coordinators::folder_load::state::{
    FolderLoadCoordinator, FolderLoadCx, FolderLoadDeps, FolderLoadError, FolderLoadEvent, FolderLoadStep,
    FolderLoadUpdate, FolderPerf, LoadPhase, WorkerLane, WorkerPort,
};
use pr_app::coordinators::traits::IFolderLoadSink;
use pr_core::abstractions::{
    AppendStream, ExplorerOrderProvider, FileStat, FileSystem, Log, ReadSeek, SkippedEntry, SkippedKind, SystemClock,
};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::explorer_types::{
    ExplorerGroupState, ExplorerOrderStatus, ExplorerQueryProgress, ExplorerViewSnapshot,
};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::LatestExplorerSnapshot;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::{ImageSortMode, RawPairMode};
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::store::SettingsStore;
use pr_core::sync::CancelToken;
use pr_core::text::path_eq_ignore_case;
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeFileSystem, FsOp, LogLevel, RecordingLog, TempRoot};

// ---------------------------------------------------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------------------------------------------------

type Job = Box<dyn FnOnce() -> FolderLoadEvent + Send>;

/// A [`WorkerPort`] that queues jobs; the test runs them in the order it wants.
#[derive(Default)]
struct ManualPort {
    jobs: Mutex<VecDeque<(WorkerLane, Job)>>,
    submitted: Mutex<Vec<WorkerLane>>,
}

impl ManualPort {
    fn take_lane(&self, lane: WorkerLane) -> Option<Job> {
        let mut jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let at = jobs.iter().position(|(l, _)| *l == lane)?;
        jobs.remove(at).map(|(_, job)| job)
    }

    fn take_first(&self) -> Option<Job> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .map(|(_, job)| job)
    }

    fn pending(&self, lane: WorkerLane) -> usize {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(l, _)| *l == lane)
            .count()
    }

    fn submitted(&self) -> Vec<WorkerLane> {
        self.submitted.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl std::fmt::Debug for ManualPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManualPort").finish_non_exhaustive()
    }
}

impl WorkerPort<FolderLoadEvent> for ManualPort {
    fn submit(&self, lane: WorkerLane, job: Job) {
        self.submitted.lock().unwrap_or_else(PoisonError::into_inner).push(lane);
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back((lane, job));
    }
}

type ListingHook = Box<dyn Fn() + Send + Sync>;

/// Wraps the shared in-memory file system. The folder listing yields files in INSERTION order (the C# fake did too, so the
/// scan order differs from every sort), reports a configurable skipped entry, counts accepted entries and per-file calls.
struct ScanFs {
    inner: FakeFileSystem,
    order: Mutex<Vec<String>>,
    listing_hook: Mutex<Option<ListingHook>>,
    fail_listing: Mutex<Option<io::ErrorKind>>,
    skipped_to_report: Mutex<Vec<SkippedEntry>>,
    subfolders: Mutex<HashMap<String, Vec<PathBuf>>>,
    fail_directories: Mutex<Option<io::ErrorKind>>,
    accepted: AtomicUsize,
    directory_calls: AtomicUsize,
    file_exists_calls: AtomicUsize,
    probe_log: Mutex<Vec<String>>,
}

impl std::fmt::Debug for ScanFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScanFs").finish_non_exhaustive()
    }
}

impl ScanFs {
    fn new() -> Self {
        Self {
            inner: FakeFileSystem::new(),
            order: Mutex::new(Vec::new()),
            listing_hook: Mutex::new(None),
            fail_listing: Mutex::new(None),
            skipped_to_report: Mutex::new(Vec::new()),
            subfolders: Mutex::new(HashMap::new()),
            fail_directories: Mutex::new(None),
            accepted: AtomicUsize::new(0),
            directory_calls: AtomicUsize::new(0),
            file_exists_calls: AtomicUsize::new(0),
            probe_log: Mutex::new(Vec::new()),
        }
    }

    fn add_file(&self, path: &str, length: usize) {
        self.inner.add_file(path, &vec![b'x'; length]);
        self.order
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_owned());
    }

    fn per_file_calls(&self) -> usize {
        self.inner.count(FsOp::Stat)
            + self.inner.count(FsOp::OpenRead)
            + self.file_exists_calls.load(Ordering::SeqCst)
            + self.probe_log.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    fn probes(&self) -> Vec<String> {
        self.probe_log.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl FileSystem for ScanFs {
    fn file_exists(&self, path: &Path) -> bool {
        self.file_exists_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.file_exists(path)
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.inner.get_file_stat(path)
    }
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.move_file(source, destination)
    }
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.copy_file(source, destination)
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        self.inner.delete(path)
    }
    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.inner.open_read_shared(path, buffer_size)
    }
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.inner.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.inner.write_all_text_atomic(path, text, durable)
    }
    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.inner.read_all_text(path)
    }
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_files(directory, pattern)
    }

    fn enumerate_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        if let Some(hook) = &*self.listing_hook.lock().unwrap_or_else(PoisonError::into_inner) {
            hook();
        }
        if let Some(kind) = *self.fail_listing.lock().unwrap_or_else(PoisonError::into_inner) {
            return Err(io::Error::new(kind, "listing failed"));
        }
        for entry in self
            .skipped_to_report
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            on_skipped(entry.clone());
        }
        let order = self.order.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let mut listed = Vec::new();
        for path in order {
            let file = Path::new(&path);
            if !file
                .parent()
                .is_some_and(|parent| path_eq_ignore_case(parent, directory))
                || !include(file)
            {
                continue;
            }
            self.accepted.fetch_add(1, Ordering::SeqCst);
            listed.push((file.to_path_buf(), self.inner.peek_stat(file)));
        }
        Ok(listed)
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        self.probe_log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_string_lossy().into_owned());
        self.inner.try_probe_readable(path)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.directory_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(kind) = *self.fail_directories.lock().unwrap_or_else(PoisonError::into_inner) {
            return Err(io::Error::new(kind, "listing failed"));
        }
        let key = directory.to_string_lossy().to_uppercase();
        if let Some(found) = self.subfolders.lock().unwrap_or_else(PoisonError::into_inner).get(&key) {
            return Ok(found.clone());
        }
        self.inner.enumerate_directories(directory)
    }
    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.inner.create_directory(path)
    }
}

type SnapshotHook = Box<dyn Fn(&str) -> ExplorerViewSnapshot + Send + Sync>;

/// Explorer provider fake: counts queries; the snapshot comes from a hook (default: native view unavailable).
#[derive(Default)]
struct FakeExplorer {
    hook: Mutex<Option<SnapshotHook>>,
    queries: AtomicUsize,
    panic_on_query: AtomicBool,
}

impl std::fmt::Debug for FakeExplorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeExplorer").finish_non_exhaustive()
    }
}

impl FakeExplorer {
    fn set_order(&self, order: &[&str]) {
        let order: Vec<String> = order.iter().map(|p| (*p).to_owned()).collect();
        let hook: SnapshotHook = Box::new(move |folder| {
            let mut snapshot = snapshot(folder, &[], ExplorerOrderStatus::Available);
            snapshot.ordered_paths = order.clone();
            snapshot
        });
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    fn set_status(&self, status: ExplorerOrderStatus) {
        let hook: SnapshotHook = Box::new(move |folder| snapshot(folder, &[], status));
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    fn queries(&self) -> usize {
        self.queries.load(Ordering::SeqCst)
    }
}

impl ExplorerOrderProvider for FakeExplorer {
    fn try_get_snapshot_progressive(
        &self,
        folder: &Path,
        _timeout: std::time::Duration,
        _progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
        _batch_size: usize,
        _cancel: &CancelToken,
    ) -> ExplorerViewSnapshot {
        self.queries.fetch_add(1, Ordering::SeqCst);
        assert!(
            !self.panic_on_query.load(Ordering::SeqCst),
            "the Explorer provider failed (test)"
        );
        let folder = folder.to_string_lossy();
        match &*self.hook.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(hook) => hook(&folder),
            None => snapshot(&folder, &[], ExplorerOrderStatus::NativeViewUnavailable),
        }
    }
}

fn snapshot(folder: &str, paths: &[&str], status: ExplorerOrderStatus) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: folder.to_owned(),
        ordered_paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        sort_columns: Vec::new(),
        group_state: ExplorerGroupState::None,
        status,
        reason: None,
        captured_utc: DateTimeUtc::now(),
    }
}

/// (generation, phase, value, detail)
type PerfMark = (i64, String, Option<i64>, String);

#[derive(Debug, Default)]
struct RecordingPerf {
    marks: Mutex<Vec<PerfMark>>,
}

impl RecordingPerf {
    fn phases(&self) -> Vec<String> {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|m| m.1.clone())
            .collect()
    }

    fn mark_of(&self, phase: &str) -> Option<(Option<i64>, String)> {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|m| m.1 == phase)
            .map(|m| (m.2, m.3.clone()))
    }
}

impl FolderPerf for RecordingPerf {
    fn mark(&self, generation: i64, phase: &str, value: Option<i64>, detail: &str) {
        self.marks.lock().unwrap_or_else(PoisonError::into_inner).push((
            generation,
            phase.to_owned(),
            value,
            detail.to_owned(),
        ));
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Call {
    ResetCaches,
    CatalogReady { folder: String, count: i32 },
    Present { index: i32, generation: i64 },
    Empty,
    EmptyWithSubfolders(i32),
    OrderApplied { count: i32, current_index: i32, kept: bool },
    FilesSkipped { folder: String, skipped: Vec<SkippedEntry> },
    UnreadableRemoved,
    Failed { folder: String, directory_not_found: bool },
}

/// Records every sink call in order, plus the sessions it received.
#[derive(Default)]
struct RecordingSink {
    calls: Vec<Call>,
    sessions: Vec<SessionState>,
    catalog_ready_at_skipped: Option<usize>,
    /// Per-file file-system calls at the moment of the first `present` (SlowStorage).
    watch: Option<Arc<ScanFs>>,
    at_first_present: Option<(usize, usize)>,
}

impl RecordingSink {
    fn presented(&self) -> Vec<(i32, i64)> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Present { index, generation } => Some((*index, *generation)),
                _ => None,
            })
            .collect()
    }

    fn count(&self, pick: impl Fn(&Call) -> bool) -> usize {
        self.calls.iter().filter(|c| pick(c)).count()
    }

    fn reset_caches(&self) -> usize {
        self.count(|c| matches!(c, Call::ResetCaches))
    }

    fn catalog_ready(&self) -> usize {
        self.count(|c| matches!(c, Call::CatalogReady { .. }))
    }

    fn failures(&self) -> Vec<(String, bool)> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Failed {
                    folder,
                    directory_not_found,
                } => Some((folder.clone(), *directory_not_found)),
                _ => None,
            })
            .collect()
    }

    fn orders_applied(&self) -> Vec<(i32, i32, bool)> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::OrderApplied {
                    count,
                    current_index,
                    kept,
                } => Some((*count, *current_index, *kept)),
                _ => None,
            })
            .collect()
    }

    fn skipped_calls(&self) -> Vec<(String, Vec<SkippedEntry>)> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::FilesSkipped { folder, skipped } => Some((folder.clone(), skipped.clone())),
                _ => None,
            })
            .collect()
    }
}

impl IFolderLoadSink for RecordingSink {
    fn reset_caches(&mut self) {
        self.calls.push(Call::ResetCaches);
    }
    fn on_catalog_ready(&mut self, folder: &str, count: i32, session: &SessionState) {
        self.sessions.push(session.clone());
        self.calls.push(Call::CatalogReady {
            folder: folder.to_owned(),
            count,
        });
    }
    fn present(&mut self, index: i32, presentation_generation: i64) {
        if self.at_first_present.is_none() {
            if let Some(fs) = &self.watch {
                self.at_first_present = Some((fs.per_file_calls(), fs.inner.count(FsOp::Stat)));
            }
        }
        self.calls.push(Call::Present {
            index,
            generation: presentation_generation,
        });
    }
    fn on_empty(&mut self, _folder: &str, session: &SessionState) {
        self.sessions.push(session.clone());
        self.calls.push(Call::Empty);
    }
    fn on_empty_with_subfolders(&mut self, _folder: &str, session: &SessionState, subfolder_count: i32) {
        self.sessions.push(session.clone());
        self.calls.push(Call::EmptyWithSubfolders(subfolder_count));
    }
    fn on_order_applied(&mut self, count: i32, current_index: i32, current_kept: bool) {
        self.calls.push(Call::OrderApplied {
            count,
            current_index,
            kept: current_kept,
        });
    }
    fn on_files_skipped(&mut self, folder: &str, skipped: &[SkippedEntry]) {
        self.catalog_ready_at_skipped = Some(self.catalog_ready());
        self.calls.push(Call::FilesSkipped {
            folder: folder.to_owned(),
            skipped: skipped.to_vec(),
        });
    }
    fn on_unreadable_removed(&mut self, _removed_paths: &[String], _current_removed: bool) {
        self.calls.push(Call::UnreadableRemoved);
    }
    fn on_failed(&mut self, folder: &str, error: &dyn Error) {
        self.calls.push(Call::Failed {
            folder: folder.to_owned(),
            // `downcast_ref` needs `'static`; the localized text identifies the variant.
            directory_not_found: error.to_string().starts_with("Folder not found"),
        });
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------------------------------

fn update_settings(store: &SettingsStore, change: impl FnOnce(&mut AppSettings)) {
    let mut settings = (*store.current()).clone();
    change(&mut settings);
    assert!(store.save(settings).is_ok(), "settings save failed");
}

struct Env {
    fs: Arc<ScanFs>,
    settings: Arc<SettingsStore>,
    sessions: Arc<SessionStore>,
    clock: Arc<GenerationClock>,
    explorer: Arc<FakeExplorer>,
    port: Arc<ManualPort>,
    log: Arc<RecordingLog>,
    perf: Arc<RecordingPerf>,
    latest: Arc<LatestExplorerSnapshot>,
    catalog: ReviewCatalog,
    sink: RecordingSink,
    coordinator: FolderLoadCoordinator,
}

impl Env {
    /// In-memory file system; the sort mode is `Name` (the C# fixture: only the Explorer-ordered modes query Explorer).
    fn new() -> Self {
        let fs = Arc::new(ScanFs::new());
        let paths = AppPaths::new(Path::new("C:\\data"), None, false);
        let env = Self::assemble(Arc::clone(&fs), Arc::clone(&fs) as Arc<dyn FileSystem>, &paths);
        update_settings(&env.settings, |s| s.image_sort_mode = Some(ImageSortMode::Name));
        env
    }

    /// Real file system under a temp folder (the IO05 tests); the settings keep their defaults.
    fn physical(root: &Path) -> Self {
        let paths = AppPaths::new(&root.join("appdata"), None, false);
        Self::assemble(Arc::new(ScanFs::new()), Arc::new(PhysicalFileSystem), &paths)
    }

    fn assemble(fake: Arc<ScanFs>, fs: Arc<dyn FileSystem>, paths: &AppPaths) -> Self {
        let log = Arc::new(RecordingLog::new());
        let settings = Arc::new(SettingsStore::new(
            paths.clone(),
            Arc::clone(&fs),
            Arc::clone(&log) as Arc<dyn Log>,
            Arc::new(SystemClock),
            Arc::new(EnglishSettingsTexts),
        ));
        let sessions = Arc::new(SessionStore::new(paths, Arc::clone(&fs), None));
        let clock = Arc::new(GenerationClock::default());
        let explorer = Arc::new(FakeExplorer::default());
        let port = Arc::new(ManualPort::default());
        let perf = Arc::new(RecordingPerf::default());
        let latest = Arc::new(LatestExplorerSnapshot::new());
        let coordinator = FolderLoadCoordinator::new(FolderLoadDeps {
            clock: Arc::clone(&clock),
            explorer_order: Arc::clone(&explorer) as Arc<dyn ExplorerOrderProvider>,
            file_system: fs,
            session_store: Arc::clone(&sessions),
            settings_store: Arc::clone(&settings),
            session_writer: None,
            latest_explorer: Some(Arc::clone(&latest)),
            worker: Arc::clone(&port) as Arc<dyn WorkerPort<FolderLoadEvent>>,
            log: Arc::clone(&log) as Arc<dyn Log>,
            perf: Some(Arc::clone(&perf) as Arc<dyn FolderPerf>),
        });
        Self {
            fs: fake,
            settings,
            sessions,
            clock,
            explorer,
            port,
            log,
            perf,
            latest,
            catalog: ReviewCatalog::new(),
            sink: RecordingSink::default(),
            coordinator,
        }
    }

    fn set_sort(&self, mode: ImageSortMode) {
        update_settings(&self.settings, |s| s.image_sort_mode = Some(mode));
    }

    fn set_raw(&self, enabled: bool, pair: RawPairMode) {
        update_settings(&self.settings, |s| {
            s.raw_support_enabled = enabled;
            s.raw_pair_mode = pair;
        });
    }

    fn add(&self, path: &str, length: usize) {
        self.fs.add_file(path, length);
    }

    fn add_dir(&self, path: &str) {
        self.fs.inner.add_dir(path);
    }

    /// Creates `a.jpg`, `b.jpg`, `c.jpg` (in that scan order) in `folder`.
    fn three_images(&self, folder: &str) -> [String; 3] {
        self.add_dir(folder);
        let paths = ["a", "b", "c"].map(|n| format!("{folder}\\{n}.jpg"));
        for (i, path) in paths.iter().enumerate() {
            self.add(path, i + 1);
        }
        paths
    }

    /// Creates `c`, `a`, `b` in that scan order (returns `[a, b, c]`).
    fn three_images_scan_order_cab(&self, folder: &str) -> [String; 3] {
        self.add_dir(folder);
        let paths = ["a", "b", "c"].map(|n| format!("{folder}\\{n}.jpg"));
        for (i, path) in [&paths[2], &paths[0], &paths[1]].into_iter().enumerate() {
            self.add(path, i + 1);
        }
        paths
    }

    fn load(&mut self, folder: &str, initial: Option<&str>) -> Result<FolderLoadUpdate, FolderLoadError> {
        let mut cx = FolderLoadCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.coordinator.load(&mut cx, folder, initial)
    }

    /// `load` for a call that must succeed.
    fn open(&mut self, folder: &str, initial: Option<&str>) -> FolderLoadUpdate {
        match self.load(folder, initial) {
            Ok(update) => update,
            Err(error) => panic!("load failed: {error}"),
        }
    }

    fn deliver(&mut self, event: FolderLoadEvent) -> FolderLoadUpdate {
        let mut cx = FolderLoadCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.coordinator.handle_event(&mut cx, event)
    }

    /// Runs the oldest queued job of `lane` WITHOUT delivering its event (the result is "in flight").
    fn run_held(&mut self, lane: WorkerLane) -> FolderLoadEvent {
        let Some(job) = self.port.take_lane(lane) else {
            panic!("no queued {lane:?} job");
        };
        job()
    }

    /// Runs the oldest queued job of `lane` and delivers its event.
    fn run(&mut self, lane: WorkerLane) -> FolderLoadUpdate {
        let event = self.run_held(lane);
        self.deliver(event)
    }

    /// Runs every queued job in submission order (including jobs queued meanwhile) and delivers each event.
    fn pump(&mut self) -> Vec<FolderLoadUpdate> {
        let mut updates = Vec::new();
        while let Some(job) = self.port.take_first() {
            let event = job();
            updates.push(self.deliver(event));
        }
        updates
    }

    fn paths(&self) -> Vec<String> {
        self.catalog.paths().to_vec()
    }

    fn current_path(&self) -> Option<String> {
        self.catalog.current().map(|e| e.path().to_owned())
    }

    fn phases(&self) -> Vec<String> {
        self.perf.phases()
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LoadAsync_NormalFolder_PopulatesCatalogAndPresentsFirstImage
#[test]
fn normal_folder_populates_catalog_and_presents_first_image() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 4);
    env.add("C:\\photos\\b.jpg", 4);

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.catalog.count(), 2);
    assert_eq!(env.sink.reset_caches(), 1);
    assert_eq!(env.sink.catalog_ready(), 1);
    // The scan takes Length/LastWriteUtc from the one listing: no per-file stat.
    assert_eq!(env.fs.inner.count(FsOp::Stat), 0);
    let presented = env.sink.presented();
    assert_eq!(presented.len(), 1);
    assert_eq!(presented[0].0, 0);
    assert!(env.sink.failures().is_empty());
    // The observable sink order of the load (contract trap list).
    assert_eq!(env.sink.calls[0], Call::ResetCaches);
    assert!(matches!(env.sink.calls[1], Call::CatalogReady { count: 2, .. }));
    assert!(matches!(env.sink.calls[2], Call::Present { .. }));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_RawPairModeGroupsImagesAndKeepsXmpOutOfExplorerAndReadabilityProbe
#[test]
fn raw_pair_mode_groups_images_and_keeps_xmp_out_of_explorer_and_probe() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 4);
    env.add("C:\\photos\\a.cr2", 3);
    env.add("C:\\photos\\a.xmp", 7);
    env.set_raw(true, RawPairMode::PreferJpeg);

    env.open("C:\\photos", None);
    env.pump();

    let entries = env.catalog.entries_snapshot();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path(), "C:\\photos\\a.jpg");
    let group = entries[0].capture_group.as_ref();
    assert_eq!(group.and_then(|g| g.xmp_path()), Some("C:\\photos\\a.xmp"));
    assert!(
        env.sink
            .presented()
            .iter()
            .all(|(index, _)| env.catalog.path_at(usize::try_from(*index).unwrap_or(0)) != "C:\\photos\\a.xmp")
    );
    // The readability probe covers the images only, never the sidecar.
    assert!(!env.fs.probes().iter().any(|p| p.ends_with("a.xmp")));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_RawSettingsChangedWhileScanning_UsesOneSnapshotForListingAndPairing
#[test]
fn raw_settings_changed_while_scanning_use_one_snapshot() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 4);
    env.add("C:\\photos\\a.cr2", 3);
    env.set_raw(true, RawPairMode::PreferJpeg);
    // Settings are saved while the background scan lists the folder (the load already decided: RAW enabled).
    let settings = Arc::clone(&env.settings);
    *env.fs.listing_hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(move || {
        update_settings(&settings, |s| {
            s.raw_support_enabled = false;
            s.raw_pair_mode = RawPairMode::Separate;
        });
    }));

    env.open("C:\\photos", None);
    env.pump();

    // The RAW was listed under the snapshot (enabled), so it is also paired under the snapshot's mode.
    let entries = env.catalog.entries_snapshot();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path(), "C:\\photos\\a.jpg");
    assert_eq!(
        entries[0].capture_group.as_ref().map(|g| g.raw_path().to_owned()),
        Some("C:\\photos\\a.cr2".to_owned())
    );
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DriveRoot_KeepsTheRootAndLoadsItsImages
#[test]
fn drive_root_keeps_the_root_and_loads_its_images() {
    let mut env = Env::new();
    env.add_dir("C:\\");
    env.add("C:\\root-photo.jpg", 4);

    env.open("C:\\", None);
    env.pump();

    assert!(env.sink.failures().is_empty());
    assert_eq!(env.paths(), strings(&["C:\\root-photo.jpg"]));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_TrailingSeparator_LoadsTheFolder
#[test]
fn trailing_separator_loads_the_folder() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 4);

    env.open("C:\\photos\\", None);
    env.pump();

    assert!(env.sink.failures().is_empty());
    assert_eq!(env.catalog.count(), 1);
    // The sink sees the folder without the trailing separator.
    assert!(matches!(&env.sink.calls[1], Call::CatalogReady { folder, .. } if folder == "C:\\photos"));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_ReadsSessionOnce_AndPassesItToSink
#[test]
fn load_reads_session_once_and_passes_it_to_sink() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 4);
    let saved = SessionState {
        current_path: Some("C:\\photos\\a.jpg".to_owned()),
        ..SessionState::new("C:\\photos")
    };
    assert!(env.sessions.save(&saved).is_ok());
    let reads_before = env.fs.inner.count(FsOp::ReadText);

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.fs.inner.count(FsOp::ReadText) - reads_before, 1);
    assert_eq!(env.sink.sessions.len(), 1);
    assert_eq!(env.sink.sessions[0].current_path.as_deref(), Some("C:\\photos\\a.jpg"));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_EmptyFolder_ReadsSessionOnce_AndPassesItToSink
#[test]
fn empty_folder_reads_session_once_and_passes_it_to_sink() {
    let mut env = Env::new();
    env.add_dir("C:\\empty");
    let saved = SessionState {
        current_path: Some("C:\\empty\\gone.jpg".to_owned()),
        ..SessionState::new("C:\\empty")
    };
    assert!(env.sessions.save(&saved).is_ok());
    let reads_before = env.fs.inner.count(FsOp::ReadText);

    env.open("C:\\empty", None);
    env.pump();

    assert_eq!(env.fs.inner.count(FsOp::ReadText) - reads_before, 1);
    // An empty folder raises on_catalog_ready(0) then on_empty: both carry the one loaded session.
    assert_eq!(env.sink.sessions.len(), 2);
    assert_eq!(env.sink.sessions[0], env.sink.sessions[1]);
    assert_eq!(
        env.sink.sessions[0].current_path.as_deref(),
        Some("C:\\empty\\gone.jpg")
    );
}

// C#: FolderLoadCoordinatorTests.LoadAsync_EmptyFolder_CallsOnEmptyAndInvalidatesNavigation
#[test]
fn empty_folder_calls_on_empty_and_invalidates_navigation() {
    let mut env = Env::new();
    env.add_dir("C:\\empty");
    let navigation_before = env.clock.current_navigation();

    env.open("C:\\empty", None);
    env.pump();

    assert_eq!(env.catalog.count(), 0);
    assert_eq!(env.sink.count(|c| *c == Call::Empty), 1);
    assert!(env.sink.presented().is_empty());
    assert!(env.clock.current_navigation() > navigation_before);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_EmptyFolderWithSubfolders_CallsOnEmptyWithSubfoldersAndCount
#[test]
fn empty_folder_with_subfolders_calls_on_empty_with_subfolders_and_count() {
    let mut env = Env::new();
    env.add_dir("C:\\empty_with_subs");
    env.fs.subfolders.lock().unwrap_or_else(PoisonError::into_inner).insert(
        "C:\\EMPTY_WITH_SUBS".to_owned(),
        vec![
            PathBuf::from("C:\\empty_with_subs\\sub1"),
            PathBuf::from("C:\\empty_with_subs\\sub2"),
        ],
    );

    env.open("C:\\empty_with_subs", None);
    env.pump();

    assert_eq!(env.catalog.count(), 0);
    assert_eq!(env.sink.count(|c| *c == Call::Empty), 0); // the more specific call is made instead
    assert_eq!(env.sink.count(|c| *c == Call::EmptyWithSubfolders(2)), 1);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_EmptyFolderWithoutSubfolders_StillCallsPlainOnEmpty
#[test]
fn empty_folder_without_subfolders_still_calls_plain_on_empty() {
    let mut env = Env::new();
    env.add_dir("C:\\empty_no_subs");

    env.open("C:\\empty_no_subs", None);
    env.pump();

    assert_eq!(env.sink.count(|c| *c == Call::Empty), 1);
    assert_eq!(env.sink.count(|c| matches!(c, Call::EmptyWithSubfolders(_))), 0);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DirectoryNotFound_CallsOnFailed
#[test]
fn directory_not_found_calls_on_failed() {
    let mut env = Env::new();

    let update = env.open("C:\\non_existent_folder", None);

    assert_eq!(env.sink.failures(), vec![("C:\\non_existent_folder".to_owned(), true)]);
    assert_eq!(update.step, FolderLoadStep::Finished);
    assert_eq!(env.coordinator.phase(), LoadPhase::Idle);
    assert_eq!(env.port.submitted(), Vec::<WorkerLane>::new());
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DirectFileOpen_KeepsTheFileAtItsSortedPosition
#[test]
fn direct_file_open_keeps_the_file_at_its_sorted_position() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");

    env.open("C:\\photos", Some(&b));
    env.pump();

    assert_eq!(env.paths(), vec![a, b.clone(), c]);
    assert_eq!(env.current_path(), Some(b));
    // The first frame is centred on the real index, so a.jpg is "before" it.
    let presented = env.sink.presented();
    assert_eq!(presented.len(), 1);
    assert_eq!(presented[0].0, 1);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DirectFileOpen_AppSortedModes_PresentTheFileAtItsSortedIndex
#[test]
fn direct_file_open_app_sorted_modes_present_the_file_at_its_sorted_index() {
    for mode in [
        ImageSortMode::NameAscending,
        ImageSortMode::NameDescending,
        ImageSortMode::Name,
        ImageSortMode::SizeAscending,
    ] {
        let mut env = Env::new();
        env.add_dir("C:\\photos");
        let (c, a, b) = ("C:\\photos\\c.jpg", "C:\\photos\\a.jpg", "C:\\photos\\b.jpg");
        env.add(c, 3);
        env.add(a, 1);
        env.add(b, 2);
        env.set_sort(mode);

        env.open("C:\\photos", Some(b));
        env.pump();

        let expected = if mode == ImageSortMode::NameDescending {
            strings(&[c, b, a])
        } else {
            strings(&[a, b, c])
        };
        assert_eq!(env.paths(), expected, "{mode:?}");
        assert_eq!(env.current_path(), Some(b.to_owned()), "{mode:?}");
        assert_eq!(
            env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(),
            vec![1],
            "{mode:?}"
        );
    }
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DirectFileOpen_NonNormalizedPath_IsFoundAtItsPosition
#[test]
fn direct_file_open_non_normalized_path_is_found_at_its_position() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");

    env.open("C:\\photos", Some("C:\\photos\\sub\\..\\c.jpg"));
    env.pump();

    assert_eq!(env.paths(), vec![a, b, c.clone()]);
    assert_eq!(env.current_path(), Some(c));
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![2]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DirectFileOpen_SnapshotAlreadyIn_AppliesOrderBeforeTheOnlyFrame
#[test]
fn direct_file_open_snapshot_already_in_applies_order_before_the_only_frame() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&c, &b, &a]);

    env.open("C:\\photos", Some(&b));
    env.pump();

    // The opened file is presented once, already at its Explorer position.
    assert_eq!(env.paths(), vec![c, b.clone(), a]);
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![1]);
    assert_eq!(env.current_path(), Some(b));
    assert_eq!(env.sink.orders_applied(), vec![(3, 2, false)]);
    assert!(!env.coordinator.pending_order());
}

// C#: FolderLoadCoordinatorTests.LoadAsync_FolderOpen_SnapshotAlreadyIn_PresentsExplorerFirstImageOnly
#[test]
fn folder_open_snapshot_already_in_presents_explorer_first_image_only() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&c, &b, &a]);

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![0]);
    assert_eq!(env.current_path(), Some(c));
    assert_eq!(env.sink.orders_applied().len(), 1);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_StartsExplorerQueryBeforeScanning
#[test]
fn explorer_query_is_submitted_before_the_scan() {
    let mut env = Env::new();
    env.three_images("C:\\photos");

    env.open("C:\\photos", None);

    // perf(startup): the query (the slowest part of a load) is queued first so it runs in parallel with the scan.
    assert_eq!(env.port.submitted(), vec![WorkerLane::Explorer, WorkerLane::Scan]);
    env.pump();
    assert_eq!(env.fs.inner.count(FsOp::Stat), 0);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_SizeSort_PreservesScannedMetadataWithSortedEntries
#[test]
fn size_sort_preserves_scanned_metadata_with_sorted_entries() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\small.jpg", 1);
    env.add("C:\\photos\\large.jpg", 5);
    env.set_sort(ImageSortMode::SizeDescending);

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.current_path(), Some("C:\\photos\\large.jpg".to_owned()));
    assert_eq!(env.catalog.current().and_then(|e| e.length), Some(5));
    assert!(env.catalog.current().and_then(|e| e.last_write_utc).is_some());
    env.catalog.set_current(1);
    assert_eq!(env.current_path(), Some("C:\\photos\\small.jpg".to_owned()));
    assert_eq!(env.catalog.current().and_then(|e| e.length), Some(1));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DefaultSort_KeepsScanOrder
#[test]
fn default_sort_keeps_scan_order() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images_scan_order_cab("C:\\photos");
    env.set_sort(ImageSortMode::Default);

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.paths(), vec![c, a, b]);
    assert_eq!(env.explorer.queries(), 0);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_AppDecidedModes_NeverQueryOrApplyExplorerOrder
#[test]
fn app_decided_modes_never_query_or_apply_explorer_order() {
    for mode in [
        ImageSortMode::Default,
        ImageSortMode::NameAscending,
        ImageSortMode::NameDescending,
    ] {
        let mut env = Env::new();
        let [a, b, c] = env.three_images_scan_order_cab("C:\\photos");
        env.set_sort(mode);
        // A valid snapshot that would reorder everything to [b, c, a] if it were ever asked for.
        env.explorer.set_order(&[&b, &c, &a]);

        env.open("C:\\photos", None);
        env.pump();

        let expected = match mode {
            ImageSortMode::Default => vec![c.clone(), a.clone(), b.clone()],
            ImageSortMode::NameAscending => vec![a.clone(), b.clone(), c.clone()],
            _ => vec![c.clone(), b.clone(), a.clone()],
        };
        assert_eq!(env.paths(), expected, "{mode:?}");
        assert_eq!(env.explorer.queries(), 0, "{mode:?}");
        assert!(env.sink.orders_applied().is_empty(), "{mode:?}");
        assert!(!env.coordinator.pending_order(), "{mode:?}");
        assert!(!env.port.submitted().contains(&WorkerLane::Explorer), "{mode:?}");
    }
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DefaultSort_InitialPathKeepsItsScanPosition
#[test]
fn default_sort_initial_path_keeps_its_scan_position() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images_scan_order_cab("C:\\photos");
    env.set_sort(ImageSortMode::Default);

    env.open("C:\\photos", Some(&a));
    env.pump();

    // Scan order c, a, b: the opened a.jpg stays second instead of becoming photo 1.
    assert_eq!(env.paths(), vec![c, a.clone(), b]);
    assert_eq!(env.current_path(), Some(a));
    assert_eq!(env.catalog.current_index(), 1);
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![1]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DefaultSort_FirstFrameNeverWaitsOnExplorer
#[test]
fn default_sort_first_frame_never_waits_on_explorer() {
    let mut env = Env::new();
    let [_, b, _] = env.three_images_scan_order_cab("C:\\photos");
    env.set_sort(ImageSortMode::Default);

    let update = env.open("C:\\photos", Some(&b));
    // Only the scan was queued: there is no Explorer job the first frame could wait for.
    assert_eq!(env.port.pending(WorkerLane::Explorer), 0);
    let updates = env.pump();

    assert_eq!(env.sink.presented().len(), 1);
    assert!(!env.coordinator.pending_order());
    assert_eq!(env.explorer.queries(), 0);
    assert_eq!(update.step, FolderLoadStep::Pending);
    assert_eq!(updates.first().map(|u| u.step), Some(FolderLoadStep::Finished));
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.GapCases.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LoadAsync_InitialPathNotInFolder_KeepsTheScanOrderAndPresentsIndexZero
#[test]
fn initial_path_not_in_folder_keeps_the_scan_order_and_presents_index_zero() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");

    env.open("C:\\photos", Some("C:\\photos\\ghost.jpg"));
    env.pump();

    assert_eq!(env.paths(), paths.to_vec());
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![0]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_InitialPathAlreadyFirst_LeavesTheOrderUntouchedAndPresentsIt
#[test]
fn initial_path_already_first_leaves_the_order_untouched_and_presents_it() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");

    env.open("C:\\photos", Some(&paths[0]));
    env.pump();

    assert_eq!(env.paths(), paths.to_vec());
    assert_eq!(env.current_path(), Some(paths[0].clone()));
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![0]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_InitialPathInTheMiddleDifferentCase_IsPresentedWhereItSits
#[test]
fn initial_path_in_the_middle_different_case_is_presented_where_it_sits() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");

    env.open("C:\\photos", Some(&paths[1].to_uppercase()));
    env.pump();

    assert_eq!(env.paths(), paths.to_vec());
    assert_eq!(env.current_path(), Some(paths[1].clone()));
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![1]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DriveRootFolder_LoadsItsImages
#[test]
fn drive_root_folder_loads_its_images() {
    let mut env = Env::new();
    env.add_dir("C:\\");
    env.add("C:\\root1.jpg", 1);
    env.add("C:\\root2.jpg", 2);

    env.open("C:\\", None);
    env.pump();

    assert!(env.sink.failures().is_empty());
    assert_eq!(env.paths(), strings(&["C:\\root1.jpg", "C:\\root2.jpg"]));
    assert_eq!(env.sink.catalog_ready(), 1);
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![0]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_EmptyFolderWhoseSubfolderListingThrows_ReportsPlainEmptyNotAFailure
#[test]
fn empty_folder_whose_subfolder_listing_fails_reports_plain_empty_not_a_failure() {
    // C# theory: IOException and UnauthorizedAccessException; the Rust listing returns io::Error of either kind.
    for kind in [io::ErrorKind::Other, io::ErrorKind::PermissionDenied] {
        let mut env = Env::new();
        env.add_dir("C:\\emptyfolder");
        *env.fs.fail_directories.lock().unwrap_or_else(PoisonError::into_inner) = Some(kind);

        env.open("C:\\emptyfolder", None);
        env.pump();

        assert!(env.sink.failures().is_empty(), "{kind:?}");
        assert_eq!(env.sink.count(|c| *c == Call::Empty), 1, "{kind:?}");
        assert_eq!(
            env.sink.count(|c| matches!(c, Call::EmptyWithSubfolders(_))),
            0,
            "{kind:?}"
        );
        assert_eq!(env.fs.directory_calls.load(Ordering::SeqCst), 1, "{kind:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.LatestExplorer.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LoadAsync_ExplorerSort_PublishesSnapshotToLatestExplorerSnapshot
#[test]
fn explorer_sort_publishes_snapshot_to_latest_explorer_snapshot() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images_scan_order_cab("C:\\photos");
    env.set_sort(ImageSortMode::Name);
    env.explorer.set_order(&[&b, &c, &a]);

    env.open("C:\\photos", None);
    env.pump();

    let latest = env.latest.current();
    assert_eq!(latest.as_ref().map(|s| s.status), Some(ExplorerOrderStatus::Available));
    assert_eq!(latest.map(|s| s.ordered_paths.clone()), Some(vec![b, c, a]));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_AppDecidedSort_ClearsLatestExplorerSnapshot
#[test]
fn app_decided_sort_clears_latest_explorer_snapshot() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images_scan_order_cab("C:\\photos");
    env.explorer.set_order(&[&b, &c, &a]);
    env.set_sort(ImageSortMode::Name);
    env.open("C:\\photos", None);
    env.pump();
    assert!(env.latest.current().is_some());

    env.set_sort(ImageSortMode::Default);
    env.open("C:\\photos", None);
    env.pump();

    assert!(env.latest.current().is_none());
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.MutationGaps.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LoadAsync_ScanSkippedAnEntry_StillLoadsTheImagesAndReportsTheSkippedEntryOnce
#[test]
fn scan_skipped_an_entry_still_loads_the_images_and_reports_it_once() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 3);
    let skipped = SkippedEntry {
        path: PathBuf::from("C:\\photos\\locked.jpg"),
        reason: "access denied".to_owned(),
        kind: SkippedKind::FileUnreadable,
    };
    env.fs
        .skipped_to_report
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(skipped.clone());

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.paths(), strings(&["C:\\photos\\a.jpg"]));
    assert_eq!(env.sink.skipped_calls(), vec![("C:\\photos".to_owned(), vec![skipped])]);
    assert!(env.sink.failures().is_empty());
    // Sink order: reset, catalog ready, skipped, then the first present.
    assert!(matches!(env.sink.calls[0], Call::ResetCaches));
    assert!(matches!(env.sink.calls[1], Call::CatalogReady { .. }));
    assert!(matches!(env.sink.calls[2], Call::FilesSkipped { .. }));
    assert!(matches!(env.sink.calls[3], Call::Present { .. }));
    assert_eq!(env.sink.catalog_ready_at_skipped, Some(1));
    assert!(
        env.log
            .contains(LogLevel::Warn, "Folder scan skipped 1 unreadable entr(y/ies)")
    );
}

// C#: FolderLoadCoordinatorTests.LoadAsync_SinkThrowsAfterTheFolderGenerationMovedOn_IsSwallowedNotReportedAsAFailure
// D-07a-4: a Rust sink does not throw. The same catch branch is reached by a scan failure after the folder generation moved on.
#[test]
fn stale_load_failure_after_the_folder_generation_moved_on_is_not_reported_as_a_failure() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    *env.fs.fail_listing.lock().unwrap_or_else(PoisonError::into_inner) = Some(io::ErrorKind::PermissionDenied);
    env.open("C:\\photos", None);
    // Another folder was opened meanwhile (the load token itself is not canceled).
    env.clock.next_folder();

    env.pump();

    assert!(env.sink.failures().is_empty());
    assert_eq!(env.sink.reset_caches(), 0);
    assert!(env.log.contains(LogLevel::Warn, "Superseded folder load failed"));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_SinkThrowsWhileTheLoadIsStillCurrent_IsReportedAsAFailure
// D-07a-4: the current-load error reaches on_failed with the normalized folder.
#[test]
fn current_load_error_reaches_on_failed() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    *env.fs.fail_listing.lock().unwrap_or_else(PoisonError::into_inner) = Some(io::ErrorKind::PermissionDenied);

    env.open("C:\\photos\\", None);
    let updates = env.pump();

    assert_eq!(env.sink.failures(), vec![("C:\\photos".to_owned(), false)]);
    assert_eq!(env.sink.catalog_ready(), 0);
    assert_eq!(updates.last().map(|u| u.step), Some(FolderLoadStep::Finished));
    assert_eq!(env.coordinator.phase(), LoadPhase::Idle);
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.PerfMarks.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LoadAsync_WithRawSupportOff_TheListingNeverAcceptsSidecarFiles
#[test]
fn raw_support_off_the_listing_never_accepts_sidecar_files() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 1);
    env.add("C:\\photos\\b.jpg", 2);
    env.add("C:\\photos\\a.xmp", 7);
    env.set_raw(false, RawPairMode::Separate);

    env.open("C:\\photos", None);
    env.pump();

    // An .xmp is a RAW sidecar and is listed only with RAW support.
    assert_eq!(env.fs.accepted.load(Ordering::SeqCst), 2);
    assert_eq!(env.catalog.count(), 2);
}

// C#: FolderLoadCoordinatorTests.PerfMarks_EarlySnapshotThatReorders_IsMarkedApplied
#[test]
fn early_snapshot_that_reorders_is_marked_applied() {
    let mut env = Env::new();
    let [a, b, _] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&b, &a, "C:\\photos\\c.jpg"]);

    env.open("C:\\photos", None);
    env.pump();

    let phases = env.phases();
    assert!(phases.contains(&"explorerApplied".to_owned()));
    assert!(!phases.contains(&"explorerIgnored".to_owned()));
}

// C#: FolderLoadCoordinatorTests.PerfMarks_EarlySnapshotThatIsUnavailable_IsMarkedFallback
#[test]
fn early_snapshot_that_is_unavailable_is_marked_fallback() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    env.explorer.set_status(ExplorerOrderStatus::NativeViewUnavailable);

    env.open("C:\\photos", None);
    env.pump();

    let phases = env.phases();
    assert!(phases.contains(&"explorerFallback".to_owned()));
    assert!(!phases.contains(&"explorerApplied".to_owned()));
}

// C#: FolderLoadCoordinatorTests.PerfMarks_TheExplorerQueryIsTracedWithItsStatusAndCount
#[test]
fn the_explorer_query_is_traced_with_its_status_and_count() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&a, &b, &c]);

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(
        env.perf.mark_of("explorerSnapshot"),
        Some((Some(3), "Available".to_owned()))
    );
}

// C#: FolderLoadCoordinatorTests.PerfMarks_AFailedExplorerQueryIsTracedWithTheExceptionType
// A Rust provider reports failure by panicking only on a bug; the job turns it into a failed handle and traces "Panic".
#[test]
fn a_failed_explorer_query_is_traced_as_a_failure() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    env.explorer.panic_on_query.store(true, Ordering::SeqCst);

    env.open("C:\\photos", None);
    let held = env.run_held(WorkerLane::Explorer);

    assert!(matches!(held, FolderLoadEvent::ExplorerFinished { snapshot: None, .. }));
    assert_eq!(env.perf.mark_of("explorerSnapshot"), Some((None, "Panic".to_owned())));
    assert!(env.latest.current().is_none());
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.SlowStorage.cs
// ---------------------------------------------------------------------------------------------------------------------

/// Builds an independent environment of `file_count` files, loads it, and returns the per-file counters at the first present
/// and the probe count after the probe ran.
fn load_and_snapshot(file_count: usize) -> (usize, usize, usize) {
    let mut env = Env::new();
    let folder = format!("C:\\nas\\folder{file_count}");
    env.add_dir(&folder);
    for i in 0..file_count {
        env.add(&format!("{folder}\\img{i:05}.jpg"), i % 7);
    }
    env.sink.watch = Some(Arc::clone(&env.fs));

    env.open(&folder, None);
    env.pump();

    let (per_file_at_first, stat_at_first) = env.sink.at_first_present.unwrap_or((usize::MAX, usize::MAX));
    (per_file_at_first, stat_at_first, env.fs.probes().len())
}

// C#: FolderLoadCoordinatorTests.Load_PerFileCallsBeforeFirstFrame_DoNotGrowWithFolderSize
#[test]
fn per_file_calls_before_first_frame_do_not_grow_with_folder_size() {
    let (small_per_file, small_stat, small_probes) = load_and_snapshot(20);
    let (large_per_file, large_stat, large_probes) = load_and_snapshot(500);

    // The only per-file call before the first present is the session file's single existence check.
    assert_eq!(small_per_file, large_per_file);
    assert!(
        large_per_file <= 2,
        "expected a small folder-size-independent constant (<=2), got {large_per_file}"
    );
    assert!(small_stat < 20);
    assert!(large_stat < 500);
    // The probe covers every file, but only after the first frame (it is a separate job).
    assert_eq!(small_probes, 20);
    assert_eq!(large_probes, 500);
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.WebpHeic.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LoadAsync_WebpHeicSwitch_DecidesWhetherWebpAndHeicFilesAreListed
#[test]
fn webp_heic_switch_decides_whether_webp_and_heic_files_are_listed() {
    for enabled in [true, false] {
        let mut env = Env::new();
        env.add_dir("C:\\photos");
        for (name, length) in [
            ("a.jpg", 1),
            ("b.webp", 2),
            ("c.HEIC", 3),
            ("d.heif", 4),
            ("e.avif", 5), // AVIF/JXL are deferred: never listed
            ("f.jxl", 6),
        ] {
            env.add(&format!("C:\\photos\\{name}"), length);
        }
        update_settings(&env.settings, |s| s.webp_heic_support_enabled = enabled);

        env.open("C:\\photos", None);
        env.pump();

        let mut listed = env.paths();
        listed.sort_by_key(|p| p.to_uppercase());
        let expected = if enabled {
            strings(&[
                "C:\\photos\\a.jpg",
                "C:\\photos\\b.webp",
                "C:\\photos\\c.HEIC",
                "C:\\photos\\d.heif",
            ])
        } else {
            strings(&["C:\\photos\\a.jpg"])
        };
        assert_eq!(listed, expected, "enabled = {enabled}");
    }
}

// C#: FolderLoadCoordinatorTests.LoadAsync_WebpHeicSwitchChangedWhileScanning_UsesTheSnapshotTakenAtLoadStart
#[test]
fn webp_heic_switch_changed_while_scanning_uses_the_snapshot_taken_at_load_start() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 1);
    env.add("C:\\photos\\b.webp", 2);
    update_settings(&env.settings, |s| s.webp_heic_support_enabled = true);
    let settings = Arc::clone(&env.settings);
    *env.fs.listing_hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Box::new(move || {
        update_settings(&settings, |s| s.webp_heic_support_enabled = false);
    }));

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.catalog.count(), 2);
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.Races.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LoadAsync_SecondFolderWhileFirstScanBlocked_FirstLoadIsDroppedSilently
#[test]
fn superseded_load_events_are_dropped_silently() {
    let mut env = Env::new();
    env.three_images("C:\\slow");
    env.add_dir("C:\\fast");
    env.add("C:\\fast\\x.jpg", 1);

    env.open("C:\\slow", None);
    // The slow scan has finished its work, but its event is still in flight when the second folder is opened.
    let slow_scan = env.run_held(WorkerLane::Scan);
    env.open("C:\\fast", None);
    env.pump();
    let late = env.deliver(slow_scan);

    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert_eq!(env.sink.catalog_ready(), 1);
    assert_eq!(env.sink.reset_caches(), 1);
    assert_eq!(env.paths(), strings(&["C:\\fast\\x.jpg"]));
    assert_eq!(env.sink.presented().len(), 1);
    assert!(env.sink.failures().is_empty());
}

// C#: FolderLoadCoordinatorTests.LoadAsync_FolderGenerationBumpedDuringScan_AppliesNothing
#[test]
fn folder_generation_bumped_during_scan_applies_nothing() {
    let mut env = Env::new();
    env.three_images("C:\\slow");
    env.open("C:\\slow", None);
    let scan = env.run_held(WorkerLane::Scan);

    env.clock.next_folder();
    let update = env.deliver(scan);

    assert_eq!(update.step, FolderLoadStep::Finished);
    assert_eq!(env.sink.reset_caches(), 0);
    assert_eq!(env.sink.catalog_ready(), 0);
    assert_eq!(env.catalog.count(), 0);
    assert!(env.sink.presented().is_empty());
    assert_eq!(env.coordinator.phase(), LoadPhase::Idle);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_RapidSwitches_OnlyTheLastIsApplied
#[test]
fn rapid_switches_only_the_last_is_applied() {
    let mut env = Env::new();
    for name in ["one", "two", "three"] {
        env.add_dir(&format!("C:\\{name}"));
        env.add(&format!("C:\\{name}\\{name}.jpg"), 1);
    }

    env.open("C:\\one", None);
    env.open("C:\\two", None);
    env.open("C:\\three", None);
    // Whatever order the scans finish in: the superseded ones first, then the last.
    let first = env.run_held(WorkerLane::Scan);
    let second = env.run_held(WorkerLane::Scan);
    let first_update = env.deliver(first);
    let second_update = env.deliver(second);
    env.pump();

    assert_eq!(first_update.step, FolderLoadStep::Ignored);
    assert_eq!(second_update.step, FolderLoadStep::Ignored);
    assert_eq!(env.sink.catalog_ready(), 1);
    assert_eq!(env.paths(), strings(&["C:\\three\\three.jpg"]));
    assert!(env.sink.failures().is_empty());
}

// C#: FolderLoadCoordinatorTests.LoadAsync_InitialPathNotInFolder_PresentsFirstImage
#[test]
fn initial_path_not_in_folder_presents_first_image() {
    let mut env = Env::new();
    let [a, _, _] = env.three_images("C:\\photos");

    env.open("C:\\photos", Some("C:\\photos\\ghost.jpg"));
    env.pump();

    assert!(env.sink.failures().is_empty());
    assert_eq!(env.current_path(), Some(a));
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![0]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_MalformedPath_ReportsFailure
#[test]
fn malformed_path_reports_failure() {
    // Rows: a folder with an invalid character, an initial path with a NUL, a folder with a NUL.
    let rows: [(&str, Option<&str>); 3] = [
        ("C:\\ph|otos", None),
        ("C:\\photos", Some("C:\\photos\\a\0.jpg")),
        ("C:\\photos\0", None),
    ];
    for (folder, initial) in rows {
        let mut env = Env::new();
        env.three_images("C:\\photos");

        env.open(folder, initial);
        env.pump();

        assert_eq!(env.sink.failures().len(), 1, "{folder:?} {initial:?}");
        assert_eq!(env.sink.catalog_ready(), 0, "{folder:?} {initial:?}");
    }
}

// C#: FolderLoadCoordinatorTests.LoadAsync_BlankFolder_IsRejectedUpFront
#[test]
fn blank_folder_is_rejected_before_any_side_effect() {
    for folder in ["", "   "] {
        let mut env = Env::new();
        let folder_before = env.clock.current_folder();

        let result = env.load(folder, None);

        assert!(matches!(result, Err(FolderLoadError::BlankFolder)), "{folder:?}");
        assert_eq!(env.sink.reset_caches(), 0);
        assert_eq!(env.clock.current_folder(), folder_before, "no generation bump");
        assert!(env.port.submitted().is_empty());
        assert!(env.sink.calls.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadCoordinatorTests.Superseded.cs
// ---------------------------------------------------------------------------------------------------------------------

// C#: FolderLoadCoordinatorTests.LoadAsync_CancelledDuringScan_StopsEnumerating
// D-07a-3: the Rust listing cannot be aborted, so a canceled scan accepts no further entry and returns `Canceled`.
#[test]
fn canceled_during_scan_accepts_no_entry_and_applies_nothing() {
    let mut env = Env::new();
    env.add_dir("C:\\big");
    for i in 0..200 {
        env.add(&format!("C:\\big\\p{i:04}.jpg"), 1);
    }
    env.add_dir("C:\\other");
    env.add("C:\\other\\x.jpg", 1);

    env.open("C:\\big", None);
    env.open("C:\\other", None); // cancels the token of the big load before its scan job starts
    let canceled = env.run_held(WorkerLane::Scan); // the big scan (queued first)
    let accepted_by_big = env.fs.accepted.load(Ordering::SeqCst);

    assert!(matches!(
        canceled,
        FolderLoadEvent::ScanFinished {
            outcome: Err(pr_app::coordinators::folder_load::state::ScanFailure::Canceled),
            ..
        }
    ));
    assert_eq!(accepted_by_big, 0, "a canceled scan keeps nothing");
    assert_eq!(env.deliver(canceled).step, FolderLoadStep::Ignored);
    env.pump();
    assert_eq!(env.paths(), strings(&["C:\\other\\x.jpg"]));
}

// ---------------------------------------------------------------------------------------------------------------------
// New tests (no C# twin)
// ---------------------------------------------------------------------------------------------------------------------

// New test
#[test]
fn load_after_dispose_returns_disposed_error() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    env.coordinator.dispose();

    let result = env.load("C:\\photos", None);

    assert!(matches!(result, Err(FolderLoadError::Disposed)));
    assert!(env.sink.calls.is_empty());
    assert!(env.port.submitted().is_empty());
}

// New test
#[test]
fn event_with_unknown_load_id_is_ignored() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    env.open("C:\\photos", None);

    let update = env.deliver(FolderLoadEvent::ScanFinished {
        load_id: 999,
        outcome: Err(pr_app::coordinators::folder_load::state::ScanFailure::Canceled),
    });

    assert_eq!(update.step, FolderLoadStep::Ignored);
    assert_eq!(env.coordinator.phase(), LoadPhase::Scanning);
    assert!(env.sink.calls.is_empty());
}

// New test
#[test]
fn invalid_initial_path_reports_failure_after_scan() {
    let mut env = Env::new();
    env.three_images("C:\\photos");

    env.open("C:\\photos", Some("C:\\photos\\a\0.jpg"));
    // Nothing is reported by the synchronous prefix: the initial path is normalized after the scan (C# line 175).
    assert!(env.sink.calls.is_empty());
    env.pump();

    // The failure carries the normalized folder, and the catalog was never touched.
    assert_eq!(env.sink.failures(), vec![("C:\\photos".to_owned(), false)]);
    assert_eq!(env.sink.catalog_ready(), 0);
    assert_eq!(env.sink.reset_caches(), 0);
    assert_eq!(env.catalog.count(), 0);
}

// New test
#[test]
fn explorer_placeholder_is_used_for_app_decided_modes() {
    let mut env = Env::new();
    env.three_images_scan_order_cab("C:\\photos");
    env.set_sort(ImageSortMode::Default);

    env.open("C:\\photos", None);

    // No Explorer job, "explorerSkipped" traced, Diagnostics shows "not queried".
    assert_eq!(env.port.submitted(), vec![WorkerLane::Scan]);
    assert!(env.phases().contains(&"explorerSkipped".to_owned()));
    assert!(env.latest.current().is_none());
    env.pump();
    // The placeholder counts as an already completed snapshot: the order is settled without a late path.
    assert_eq!(env.coordinator.phase(), LoadPhase::Idle);
    assert!(env.phases().contains(&"explorerFallback".to_owned()));
}

// New test
#[test]
fn direct_file_open_with_pending_snapshot_presents_the_file_and_holds_the_order_gate() {
    let mut env = Env::new();
    let [_, b, _] = env.three_images("C:\\photos");

    env.open("C:\\photos", Some(&b));
    // Only the scan has finished; the Explorer query is still queued (snapshot not in).
    let update = env.run(WorkerLane::Scan);

    // INV-9: the requested file is presented at once, navigation waits on the gate.
    assert_eq!(update.step, FolderLoadStep::Pending);
    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![1]);
    assert!(env.coordinator.pending_order());
    assert_eq!(env.coordinator.phase(), LoadPhase::AwaitingOrder);
    assert!(env.sink.orders_applied().is_empty());
}

// New test
#[test]
fn folder_open_with_pending_snapshot_presents_the_fallback_and_never_gates_navigation() {
    let mut env = Env::new();
    env.three_images("C:\\photos");

    env.open("C:\\photos", None);
    env.run(WorkerLane::Scan);

    assert_eq!(env.sink.presented().iter().map(|p| p.0).collect::<Vec<_>>(), vec![0]);
    assert!(!env.coordinator.pending_order());
    assert_eq!(env.coordinator.phase(), LoadPhase::AwaitingOrder);
}

// New test
#[test]
fn a_superseding_load_releases_the_pending_order_gate() {
    let mut env = Env::new();
    let [_, b, _] = env.three_images("C:\\photos");
    env.add_dir("C:\\other");
    env.add("C:\\other\\x.jpg", 1);
    env.open("C:\\photos", Some(&b));
    env.run(WorkerLane::Scan);
    assert!(env.coordinator.pending_order());

    let update = env.open("C:\\other", None);

    assert!(
        update.order_gate_released,
        "a superseded file-open load must not keep navigation gated"
    );
    assert!(!env.coordinator.pending_order());
}

// ---------------------------------------------------------------------------------------------------------------------
// FolderLoadSkippedFilesTests.cs (real file system in a temp folder)
// ---------------------------------------------------------------------------------------------------------------------

struct Physical {
    _root: TempRoot,
    folder: PathBuf,
    env: Env,
}

impl Physical {
    fn new() -> Self {
        let root = TempRoot::new("io05");
        let folder = root.path().join("photos");
        assert!(std::fs::create_dir_all(&folder).is_ok());
        let env = Env::physical(root.path());
        Self {
            _root: root,
            folder,
            env,
        }
    }

    fn touch(&self, name: &str, dir: Option<&Path>) -> PathBuf {
        let path = dir.unwrap_or(&self.folder).join(name);
        assert!(std::fs::write(&path, "img").is_ok());
        path
    }

    /// Loads the folder and runs every job, the readability probe included.
    fn load(&mut self) {
        let folder = self.folder.to_string_lossy().into_owned();
        self.env.open(&folder, None);
        self.env.pump();
    }

    fn assert_only_skipped(&self, expected: &Path, expected_catalog_count: usize) {
        let sink = &self.env.sink;
        assert_eq!(self.env.catalog.count(), expected_catalog_count);
        let expected_text = expected.to_string_lossy().to_string();
        assert!(
            !self
                .env
                .catalog
                .entries_snapshot()
                .iter()
                .any(|e| e.path().eq_ignore_ascii_case(&expected_text))
        );
        let calls = sink.skipped_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1.len(), 1);
        assert!(
            calls[0].1[0]
                .path
                .to_string_lossy()
                .eq_ignore_ascii_case(&expected_text)
        );
        assert!(!calls[0].1[0].reason.trim().is_empty());
        assert_eq!(sink.catalog_ready_at_skipped, Some(1)); // reported for this load, after its only on_catalog_ready
        assert!(sink.failures().is_empty());
    }
}

/// Denies read/list on `path` for the current user until dropped (the Rust image of the C# `FileSystemAccessRule`).
struct DenyRead {
    path: PathBuf,
    user: String,
}

impl DenyRead {
    fn new(path: &Path) -> Option<Self> {
        let user = std::env::var("USERNAME").ok()?;
        let status = std::process::Command::new("icacls")
            .arg(path)
            .args(["/deny", &format!("{user}:(RD)")])
            .output()
            .ok()?;
        status.status.success().then(|| Self {
            path: path.to_path_buf(),
            user,
        })
    }
}

impl Drop for DenyRead {
    fn drop(&mut self) {
        // Best effort: the temp folder cleanup needs the access back.
        let _ = std::process::Command::new("icacls")
            .arg(&self.path)
            .args(["/remove:d", &self.user])
            .output();
    }
}

// C#: FolderLoadSkippedFilesTests.NormalFolder_ReportsNoSkippedFiles
#[test]
fn normal_folder_reports_no_skipped_files() {
    let mut p = Physical::new();
    p.touch("a.jpg", None);
    p.touch("b.png", None);
    p.touch("notes.txt", None);

    p.load();

    assert_eq!(p.env.catalog.count(), 2);
    assert!(p.env.sink.skipped_calls().is_empty());
    assert!(p.env.sink.failures().is_empty());
}

// C#: FolderLoadSkippedFilesTests.LockedFile_IsSkippedAndReported
#[test]
fn locked_file_is_skipped_and_reported() {
    use std::os::windows::fs::OpenOptionsExt;

    let mut p = Physical::new();
    p.touch("a.jpg", None);
    let locked = p.touch("locked.jpg", None);
    p.touch("c.jpg", None);
    let hold = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&locked);
    assert!(hold.is_ok(), "could not lock the file");

    p.load();

    p.assert_only_skipped(&locked, 2);
    drop(hold);
}

// C#: FolderLoadSkippedFilesTests.AclDeniedFile_IsSkippedAndReported
#[test]
#[ignore = "native: changes a real file ACL with icacls"]
fn acl_denied_file_is_skipped_and_reported() {
    let mut p = Physical::new();
    p.touch("a.jpg", None);
    let denied = p.touch("denied.jpg", None);
    p.touch("c.jpg", None);
    let guard = DenyRead::new(&denied);
    assert!(guard.is_some(), "could not set the ACL this test needs");

    p.load();

    p.assert_only_skipped(&denied, 2);
}

// C#: FolderLoadSkippedFilesTests.UnreadableSubdirectory_DoesNotAffectFolderScanOrWarn
#[test]
#[ignore = "native: changes a real folder ACL with icacls"]
fn unreadable_subdirectory_does_not_affect_folder_scan_or_warn() {
    let mut p = Physical::new();
    p.touch("a.jpg", None);
    let sub = p.folder.join("private");
    assert!(std::fs::create_dir_all(&sub).is_ok());
    p.touch("inner.jpg", Some(&sub));
    let guard = DenyRead::new(&sub);
    assert!(guard.is_some(), "could not set the ACL this test needs");

    p.load();

    // Sub-folders are not part of the catalog (non-recursive scan): nothing is missing, so no warning.
    assert_eq!(p.env.catalog.count(), 1);
    assert!(p.env.sink.skipped_calls().is_empty());
    assert!(p.env.sink.failures().is_empty());
}

// C#: FolderLoadSkippedFilesTests.UnreadableFolderItself_FailsLoudlyInsteadOfShowingEmptyCatalog
#[test]
#[ignore = "native: changes a real folder ACL with icacls"]
fn unreadable_folder_itself_fails_loudly_instead_of_showing_empty_catalog() {
    let mut p = Physical::new();
    p.touch("a.jpg", None);
    let guard = DenyRead::new(&p.folder);
    assert!(guard.is_some(), "could not set the ACL this test needs");

    p.load();

    assert_eq!(p.env.sink.failures().len(), 1);
    assert!(p.env.sink.skipped_calls().is_empty());
}
