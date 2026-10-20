//! Shared fakes and harness of the `FolderLoadCoordinator` test binaries (W8-07a/07b): a queued worker port, a recording
//! sink, an insertion-ordered in-memory file system and an Explorer provider fake.
// Each test binary uses a subset of this module (items and re-exported imports).
#![allow(dead_code, unused_imports)]

pub use std::collections::{HashMap, VecDeque};
pub use std::error::Error;
pub use std::io;
pub use std::path::{Path, PathBuf};
pub use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
pub use std::sync::{Arc, Mutex, PoisonError};

pub use pr_app::coordinators::folder_load::state::{
    FolderLoadCoordinator, FolderLoadCx, FolderLoadDeps, FolderLoadError, FolderLoadEvent, FolderLoadStep,
    FolderLoadUpdate, FolderPerf, LoadPhase, WorkerLane, WorkerPort,
};
pub use pr_app::coordinators::traits::IFolderLoadSink;
pub use pr_core::abstractions::{
    AppendStream, ExplorerOrderProvider, FileStat, FileSystem, Log, ReadSeek, SkippedEntry, SkippedKind, SystemClock,
};
pub use pr_core::app_paths::AppPaths;
pub use pr_core::catalog::explorer_types::{
    ExplorerGroupState, ExplorerOrderStatus, ExplorerQueryProgress, ExplorerViewSnapshot,
};
pub use pr_core::catalog::generation::GenerationClock;
pub use pr_core::catalog::review_catalog::ReviewCatalog;
pub use pr_core::catalog::types::LatestExplorerSnapshot;
pub use pr_core::io::physical::PhysicalFileSystem;
pub use pr_core::model::{ImageSortMode, RawPairMode};
pub use pr_core::session::store::{SessionState, SessionStore};
pub use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
pub use pr_core::settings::store::SettingsStore;
pub use pr_core::sync::CancelToken;
pub use pr_core::text::path_eq_ignore_case;
pub use pr_core::time::DateTimeUtc;
pub use pr_test_support::{FakeFileSystem, FsOp, LogLevel, RecordingLog, TempRoot};

// ---------------------------------------------------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------------------------------------------------

pub type Job = Box<dyn FnOnce() -> FolderLoadEvent + Send>;

/// A [`WorkerPort`] that queues jobs; the test runs them in the order it wants.
#[derive(Default)]
pub struct ManualPort {
    pub jobs: Mutex<VecDeque<(WorkerLane, Job)>>,
    pub submitted: Mutex<Vec<WorkerLane>>,
}

impl ManualPort {
    pub fn take_lane(&self, lane: WorkerLane) -> Option<Job> {
        let mut jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        let at = jobs.iter().position(|(l, _)| *l == lane)?;
        jobs.remove(at).map(|(_, job)| job)
    }

    pub fn take_first(&self) -> Option<Job> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .map(|(_, job)| job)
    }

    pub fn pending(&self, lane: WorkerLane) -> usize {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(l, _)| *l == lane)
            .count()
    }

    pub fn submitted(&self) -> Vec<WorkerLane> {
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

pub type ProbeHook = Arc<dyn Fn(&str) + Send + Sync>;

pub type ListingHook = Box<dyn Fn() + Send + Sync>;

/// Wraps the shared in-memory file system. The folder listing yields files in INSERTION order (the C# fake did too, so the
/// scan order differs from every sort), reports a configurable skipped entry, counts accepted entries and per-file calls.
pub struct ScanFs {
    pub inner: FakeFileSystem,
    pub order: Mutex<Vec<String>>,
    pub listing_hook: Mutex<Option<ListingHook>>,
    pub fail_listing: Mutex<Option<io::ErrorKind>>,
    pub skipped_to_report: Mutex<Vec<SkippedEntry>>,
    pub subfolders: Mutex<HashMap<String, Vec<PathBuf>>>,
    pub fail_directories: Mutex<Option<io::ErrorKind>>,
    pub accepted: AtomicUsize,
    pub directory_calls: AtomicUsize,
    pub file_exists_calls: AtomicUsize,
    pub probe_log: Mutex<Vec<String>>,
    /// Runs on the probe worker thread before each probe (used to hold a probe back).
    pub probe_hook: Mutex<Option<ProbeHook>>,
}

impl std::fmt::Debug for ScanFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScanFs").finish_non_exhaustive()
    }
}

impl ScanFs {
    pub fn new() -> Self {
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
            probe_hook: Mutex::new(None),
        }
    }

    pub fn add_file(&self, path: &str, length: usize) {
        self.inner.add_file(path, &vec![b'x'; length]);
        self.order
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_owned());
    }

    pub fn per_file_calls(&self) -> usize {
        self.inner.count(FsOp::Stat)
            + self.inner.count(FsOp::OpenRead)
            + self.file_exists_calls.load(Ordering::SeqCst)
            + self.probe_log.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn probes(&self) -> Vec<String> {
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
        // Clone out of the lock: several probe threads may be inside the hook at once.
        let hook = self.probe_hook.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(hook) = hook {
            hook(&path.to_string_lossy());
        }
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

pub type SnapshotHook = Box<dyn Fn(&str) -> ExplorerViewSnapshot + Send + Sync>;

/// Explorer provider fake: counts queries; the snapshot comes from a hook (default: native view unavailable).
#[derive(Default)]
pub struct FakeExplorer {
    pub hook: Mutex<Option<SnapshotHook>>,
    pub queries: AtomicUsize,
    pub panic_on_query: AtomicBool,
}

impl std::fmt::Debug for FakeExplorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeExplorer").finish_non_exhaustive()
    }
}

impl FakeExplorer {
    pub fn set_order(&self, order: &[&str]) {
        let order: Vec<String> = order.iter().map(|p| (*p).to_owned()).collect();
        let hook: SnapshotHook = Box::new(move |folder| {
            let mut snapshot = snapshot(folder, &[], ExplorerOrderStatus::Available);
            snapshot.ordered_paths = order.clone();
            snapshot
        });
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    pub fn set_status(&self, status: ExplorerOrderStatus) {
        let hook: SnapshotHook = Box::new(move |folder| snapshot(folder, &[], status));
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    pub fn queries(&self) -> usize {
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

pub fn snapshot(folder: &str, paths: &[&str], status: ExplorerOrderStatus) -> ExplorerViewSnapshot {
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
pub type PerfMark = (i64, String, Option<i64>, String);

#[derive(Debug, Default)]
pub struct RecordingPerf {
    pub marks: Mutex<Vec<PerfMark>>,
}

impl RecordingPerf {
    pub fn phases(&self) -> Vec<String> {
        self.marks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|m| m.1.clone())
            .collect()
    }

    pub fn mark_of(&self, phase: &str) -> Option<(Option<i64>, String)> {
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
pub enum Call {
    ResetCaches,
    CatalogReady {
        folder: String,
        count: i32,
    },
    Present {
        index: i32,
        generation: i64,
    },
    Empty,
    EmptyWithSubfolders(i32),
    OrderApplied {
        count: i32,
        current_index: i32,
        kept: bool,
    },
    FilesSkipped {
        folder: String,
        skipped: Vec<SkippedEntry>,
    },
    UnreadableRemoved {
        removed: Vec<String>,
        current_removed: bool,
    },
    Failed {
        folder: String,
        directory_not_found: bool,
    },
}

/// Records every sink call in order, plus the sessions it received.
#[derive(Default)]
pub struct RecordingSink {
    pub calls: Vec<Call>,
    pub sessions: Vec<SessionState>,
    pub catalog_ready_at_skipped: Option<usize>,
    /// Per-file file-system calls at the moment of the first `present` (SlowStorage).
    pub watch: Option<Arc<ScanFs>>,
    pub at_first_present: Option<(usize, usize)>,
}

impl RecordingSink {
    pub fn presented(&self) -> Vec<(i32, i64)> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Present { index, generation } => Some((*index, *generation)),
                _ => None,
            })
            .collect()
    }

    pub fn count(&self, pick: impl Fn(&Call) -> bool) -> usize {
        self.calls.iter().filter(|c| pick(c)).count()
    }

    pub fn reset_caches(&self) -> usize {
        self.count(|c| matches!(c, Call::ResetCaches))
    }

    pub fn catalog_ready(&self) -> usize {
        self.count(|c| matches!(c, Call::CatalogReady { .. }))
    }

    pub fn removals(&self) -> Vec<(Vec<String>, bool)> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::UnreadableRemoved {
                    removed,
                    current_removed,
                } => Some((removed.clone(), *current_removed)),
                _ => None,
            })
            .collect()
    }

    pub fn failures(&self) -> Vec<(String, bool)> {
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

    pub fn orders_applied(&self) -> Vec<(i32, i32, bool)> {
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

    pub fn skipped_calls(&self) -> Vec<(String, Vec<SkippedEntry>)> {
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
    fn on_unreadable_removed(&mut self, removed_paths: &[String], current_removed: bool) {
        self.calls.push(Call::UnreadableRemoved {
            removed: removed_paths.to_vec(),
            current_removed,
        });
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

pub fn update_settings(store: &SettingsStore, change: impl FnOnce(&mut AppSettings)) {
    let mut settings = (*store.current()).clone();
    change(&mut settings);
    assert!(store.save(settings).is_ok(), "settings save failed");
}

pub struct Env {
    pub fs: Arc<ScanFs>,
    pub settings: Arc<SettingsStore>,
    pub sessions: Arc<SessionStore>,
    pub clock: Arc<GenerationClock>,
    pub explorer: Arc<FakeExplorer>,
    pub port: Arc<ManualPort>,
    pub log: Arc<RecordingLog>,
    pub perf: Arc<RecordingPerf>,
    pub latest: Arc<LatestExplorerSnapshot>,
    pub catalog: ReviewCatalog,
    pub sink: RecordingSink,
    pub coordinator: FolderLoadCoordinator,
}

impl Env {
    /// In-memory file system; the sort mode is `Name` (the C# fixture: only the Explorer-ordered modes query Explorer).
    pub fn new() -> Self {
        let fs = Arc::new(ScanFs::new());
        let paths = AppPaths::new(Path::new("C:\\data"), None, false);
        let env = Self::assemble(Arc::clone(&fs), Arc::clone(&fs) as Arc<dyn FileSystem>, &paths);
        update_settings(&env.settings, |s| s.image_sort_mode = Some(ImageSortMode::Name));
        env
    }

    /// Real file system under a temp folder (the IO05 tests); the settings keep their defaults.
    pub fn physical(root: &Path) -> Self {
        let paths = AppPaths::new(&root.join("appdata"), None, false);
        Self::assemble(Arc::new(ScanFs::new()), Arc::new(PhysicalFileSystem), &paths)
    }

    pub fn assemble(fake: Arc<ScanFs>, fs: Arc<dyn FileSystem>, paths: &AppPaths) -> Self {
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

    pub fn set_sort(&self, mode: ImageSortMode) {
        update_settings(&self.settings, |s| s.image_sort_mode = Some(mode));
    }

    pub fn set_raw(&self, enabled: bool, pair: RawPairMode) {
        update_settings(&self.settings, |s| {
            s.raw_support_enabled = enabled;
            s.raw_pair_mode = pair;
        });
    }

    pub fn add(&self, path: &str, length: usize) {
        self.fs.add_file(path, length);
    }

    pub fn add_dir(&self, path: &str) {
        self.fs.inner.add_dir(path);
    }

    /// Creates `a.jpg`, `b.jpg`, `c.jpg` (in that scan order) in `folder`.
    pub fn three_images(&self, folder: &str) -> [String; 3] {
        self.add_dir(folder);
        let paths = ["a", "b", "c"].map(|n| format!("{folder}\\{n}.jpg"));
        for (i, path) in paths.iter().enumerate() {
            self.add(path, i + 1);
        }
        paths
    }

    /// Creates `c`, `a`, `b` in that scan order (returns `[a, b, c]`).
    pub fn three_images_scan_order_cab(&self, folder: &str) -> [String; 3] {
        self.add_dir(folder);
        let paths = ["a", "b", "c"].map(|n| format!("{folder}\\{n}.jpg"));
        for (i, path) in [&paths[2], &paths[0], &paths[1]].into_iter().enumerate() {
            self.add(path, i + 1);
        }
        paths
    }

    pub fn load(&mut self, folder: &str, initial: Option<&str>) -> Result<FolderLoadUpdate, FolderLoadError> {
        let mut cx = FolderLoadCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.coordinator.load(&mut cx, folder, initial)
    }

    /// `load` for a call that must succeed.
    pub fn open(&mut self, folder: &str, initial: Option<&str>) -> FolderLoadUpdate {
        match self.load(folder, initial) {
            Ok(update) => update,
            Err(error) => panic!("load failed: {error}"),
        }
    }

    pub fn deliver(&mut self, event: FolderLoadEvent) -> FolderLoadUpdate {
        let mut cx = FolderLoadCx {
            catalog: &mut self.catalog,
            sink: &mut self.sink,
        };
        self.coordinator.handle_event(&mut cx, event)
    }

    /// Runs the oldest queued job of `lane` WITHOUT delivering its event (the result is "in flight").
    pub fn run_held(&mut self, lane: WorkerLane) -> FolderLoadEvent {
        let Some(job) = self.port.take_lane(lane) else {
            panic!("no queued {lane:?} job");
        };
        job()
    }

    /// Runs the oldest queued job of `lane` and delivers its event.
    pub fn run(&mut self, lane: WorkerLane) -> FolderLoadUpdate {
        let event = self.run_held(lane);
        self.deliver(event)
    }

    /// Runs every queued job in submission order (including jobs queued meanwhile) and delivers each event.
    pub fn pump(&mut self) -> Vec<FolderLoadUpdate> {
        let mut updates = Vec::new();
        while let Some(job) = self.port.take_first() {
            let event = job();
            updates.push(self.deliver(event));
        }
        updates
    }

    pub fn paths(&self) -> Vec<String> {
        self.catalog.paths().to_vec()
    }

    pub fn current_path(&self) -> Option<String> {
        self.catalog.current().map(|e| e.path().to_owned())
    }

    pub fn phases(&self) -> Vec<String> {
        self.perf.phases()
    }
}

pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}
