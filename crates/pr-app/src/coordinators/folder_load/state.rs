//! Port of `PhotoReview.App.Shared/Coordinators/FolderLoadCoordinator.cs` (544 lines): lines 1 to 59 (fields, constructor) and
//! the shared types of the sync form (contract `contracts/W8-07a.md`). Target: `coordinators::folder_load::state`.
//!
//! D-W8-2: the C# `async Task LoadAsync` becomes a state machine on the UI thread. `load` (07a) does the synchronous prefix and
//! submits the scan and the Explorer query to a [`WorkerPort`]; each completion comes back as a [`FolderLoadEvent`] that the
//! shell passes to `handle_event` on the UI thread. The catalog and the sink are not owned: every call receives them in a
//! [`FolderLoadCx`] (no `Rc<RefCell>` graph, D-W8-1).
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use pr_core::abstractions::{ExplorerOrderProvider, FileSystem, Log, SkippedEntry};
use pr_core::catalog::explorer_types::ExplorerViewSnapshot;
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::{CatalogEntry, ExplorerQueryHandle, LatestExplorerSnapshot};
use pr_core::model::RawPairMode;
use pr_core::session::store::SessionStore;
use pr_core::session::writer::SessionWriter;
use pr_core::settings::store::SettingsStore;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;

use crate::coordinators::traits::IFolderLoadSink;

/// C# `FolderLoadCoordinator.ExplorerQueryTimeout`: how long a load waits for Explorer's view order before falling back.
pub const EXPLORER_QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// The `batchSize` argument of `TryGetSnapshotProgressiveAsync` in `LoadAsync` (C# literal `16`).
pub const EXPLORER_BATCH_SIZE: usize = 16;

/// C# `FolderLoadCoordinator.ProbeParallelism`: at most this many files are probed at once.
pub const PROBE_PARALLELISM: usize = 8;

/// At most this many skipped entries are written to the log line (C# `skipped.Take(20)`).
pub const LOGGED_SKIPPED_LIMIT: usize = 20;

/// The thread a unit of blocking work runs on (R0003 lanes). The port maps a lane to a thread; the coordinator never spawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkerLane {
    /// Folder scan + sort (C# `Task.Run` in `LoadAsync`).
    Scan,
    /// The Explorer COM query (its own STA thread).
    Explorer,
    /// The readability probe (C# `StartReadabilityProbe`).
    Probe,
    /// One file action or undo (C# `Task.Run(() => ExecuteAsync(..))` / the service's own thread).
    FileAction,
    /// Duplicate hashing (`DuplicateFinder.FindAsync`).
    Hashing,
    /// Disk-cache clearing (C# `Task.Run(ClearDiskCaches)`).
    CacheClear,
}

/// Where coordinators submit blocking work (D-W8-2). The implementation runs `job` on the thread of `lane` inside
/// `catch_unwind` (R0003), then delivers the returned event to the coordinator on the UI thread (`UiDispatcher::post`).
/// Tests implement it with a queue that runs jobs and delivers events when the test says so.
/// A new local trait: no equivalent exists in `pr_core` or `coordinators::traits` (see contract, open question O2).
pub trait WorkerPort<E>: Send + Sync {
    /// Queues `job`. Returns at once; never runs `job` on the calling (UI) thread unless the implementation is a test fake.
    fn submit(&self, lane: WorkerLane, job: Box<dyn FnOnce() -> E + Send>);
}

/// Optional perf marks of a load (C# private struct `FolderPerf`, a no-op without a listener). `value`/`detail` map to the
/// C# `Mark(phase, value, detail)` overload; `None` is `Mark(phase)`. Called from the UI thread AND from the scan job.
pub trait FolderPerf: Send + Sync {
    fn mark(&self, generation: i64, phase: &str, value: Option<i64>, detail: &str);
}

/// What every `FolderLoadCoordinator` call needs from the shell. Borrowed per call, so the coordinator owns no UI object.
pub struct FolderLoadCx<'a> {
    pub catalog: &'a mut ReviewCatalog,
    pub sink: &'a mut dyn IFolderLoadSink,
}

impl fmt::Debug for FolderLoadCx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FolderLoadCx").finish_non_exhaustive()
    }
}

/// Failures of a load (C# exceptions caught by `LoadAsync`, delivered to `IFolderLoadSink::on_failed`), plus the two
/// precondition errors the C# method throws to its caller.
#[derive(Debug)]
pub enum FolderLoadError {
    /// C# `ArgumentException.ThrowIfNullOrWhiteSpace(folder)`: returned by `load`, no side effect happened.
    BlankFolder,
    /// C# `ObjectDisposedException.ThrowIf(_disposed, this)`: returned by `load`, the sink is not touched.
    Disposed,
    /// C# `DirectoryNotFoundException(Tr.StatusFolderNotFound(folder))`. `Display` is the localized text.
    DirectoryNotFound { folder: String },
    /// C# `Path.GetFullPath` threw (`ArgumentException`, `PathTooLongException`) for the folder or the initial path.
    InvalidPath(PathError),
    /// The directory listing failed before its first entry (`FileSystem::enumerate_files_with_stat` returned `Err`).
    Io(std::io::Error),
}

impl fmt::Display for FolderLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // C# ArgumentException.ThrowIfNullOrWhiteSpace message.
            Self::BlankFolder => f.write_str("The folder path cannot be empty or composed entirely of whitespace."),
            // C# ObjectDisposedException message.
            Self::Disposed => f.write_str("Cannot access a disposed object. Object name: 'FolderLoadCoordinator'."),
            Self::DirectoryNotFound { folder } => f.write_str(&pr_l10n::tr::status_folder_not_found(folder.as_str())),
            Self::InvalidPath(error) => fmt::Display::fmt(error, f),
            Self::Io(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl Error for FolderLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPath(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// What the scan job returns (C# tuple `(scannedFiles, entries, sidecarPaths)` plus the `skipped` list and the settings
/// snapshot the job used). All owned and `Send`: the job never touches the catalog.
#[derive(Debug)]
pub struct ScanOutput {
    /// Paths of the supported images in scan order (C# `scannedFiles`), the argument of `ExplorerSnapshotValidator`.
    pub scanned_files: Vec<String>,
    /// Sorted entries, each with its own stat metadata (C# `entries`).
    pub entries: Vec<CatalogEntry>,
    /// `.xmp` paths found while RAW support is on (C# `sidecarPaths`).
    pub sidecar_paths: Vec<String>,
    /// Entries the listing could not read (C# `skipped`, in report order).
    pub skipped: Vec<SkippedEntry>,
    /// The RAW pair mode of the snapshot taken at load start (`Separate` when RAW is off).
    pub raw_pair_mode: RawPairMode,
}

/// Why a scan job ended without output.
#[derive(Debug)]
pub enum ScanFailure {
    /// The load token fired (C# `OperationCanceledException`).
    Canceled,
    /// The listing failed (C# exception from `EnumerateFilesWithStat`).
    Failed(FolderLoadError),
}

/// Results that come back to the UI thread. `load_id` identifies the load (C# object identity of its `CancellationTokenSource`);
/// an event whose `load_id` is not the active one is dropped (INV-9) without touching the catalog or the sink.
#[derive(Debug)]
pub enum FolderLoadEvent {
    /// The scan + sort job finished.
    ScanFinished {
        load_id: u64,
        outcome: Result<ScanOutput, ScanFailure>,
    },
    /// The Explorer query finished. `None` = it failed or was canceled (C# faulted/canceled task).
    ExplorerFinished {
        load_id: u64,
        snapshot: Option<Arc<ExplorerViewSnapshot>>,
    },
    /// The readability probe finished (W8-07b). `Err(None)` = canceled; `Err(Some(text))` = failed with that technical text.
    ProbeFinished {
        load_id: u64,
        outcome: Result<Vec<SkippedEntry>, Option<String>>,
    },
}

/// Where a load is (the Rust image of "which `await` the C# method is parked on").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPhase {
    /// No load running (never started, or the last one finished).
    Idle,
    /// Waiting for the scan job (C# `await Task.Run(scan)`).
    Scanning,
    /// Catalog applied and the first image requested; waiting for the Explorer snapshot (C# `await explorerTask`, second one).
    AwaitingOrder,
}

/// Whether the load that the call belongs to is over (C# `LoadAsync` task completed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FolderLoadStep {
    /// The load continues; wait for the next event.
    #[default]
    Pending,
    /// The load completed (applied, ignored, failed, canceled or superseded): C# `LoadAsync` returned.
    Finished,
    /// The event belonged to an older load, or the coordinator is disposed: nothing happened.
    Ignored,
}

/// What a call changed about the load lifecycle. Callers use `order_gate_released` to resume navigation that waited on
/// `pending_order()` (C# `await PendingOrder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FolderLoadUpdate {
    pub step: FolderLoadStep,
    /// True when this call released the pending Explorer-order gate (C# `_pendingOrder.TrySetResult()`).
    pub order_gate_released: bool,
}

/// The collaborators of the C# constructor. `catalog` and `sink` are NOT here (they arrive per call, see [`FolderLoadCx`]).
/// Every non-optional C# argument has the `ArgumentNullException` check; Rust `Arc`s cannot be null, so there is no check.
pub struct FolderLoadDeps {
    pub clock: Arc<GenerationClock>,
    pub explorer_order: Arc<dyn ExplorerOrderProvider>,
    pub file_system: Arc<dyn FileSystem>,
    pub session_store: Arc<SessionStore>,
    pub settings_store: Arc<SettingsStore>,
    /// C# `SessionWriter? sessionWriter = null`.
    pub session_writer: Option<Arc<SessionWriter>>,
    /// C# `LatestExplorerSnapshot? latestExplorer = null`.
    pub latest_explorer: Option<Arc<LatestExplorerSnapshot>>,
    pub worker: Arc<dyn WorkerPort<FolderLoadEvent>>,
    pub log: Arc<dyn Log>,
    /// `None` = no perf marks.
    pub perf: Option<Arc<dyn FolderPerf>>,
}

impl fmt::Debug for FolderLoadDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FolderLoadDeps").finish_non_exhaustive()
    }
}

/// Explorer query of the active load (the Rust image of C# `Task<ExplorerViewSnapshot> explorerTask`).
#[derive(Debug)]
pub(super) enum ExplorerSlot {
    /// App-decided sort mode: the query was never started; holds the placeholder `NativeViewUnavailable` snapshot
    /// ("Explorer order not used by this sort mode"). Counts as already completed.
    Unavailable(Arc<ExplorerViewSnapshot>),
    /// Query running. `handle` is the cell also published to `LatestExplorerSnapshot`.
    Pending(ExplorerQueryHandle),
    /// Query completed with a snapshot (any status).
    Done(Arc<ExplorerViewSnapshot>),
    /// Query failed or was canceled.
    Failed,
}

/// State of the readability probe of the active load (W8-07b).
#[derive(Debug)]
pub(super) enum ProbeState {
    /// Not started (no image in the folder, or the load ended before the first image was requested).
    None,
    /// Job submitted, result not in yet. `published` = the load finished and this probe is the current one.
    Running { published: bool },
    /// Result in, load not finished yet: applied by `finish_load` (C# applies in the `finally`).
    Ready(Result<Vec<SkippedEntry>, Option<String>>),
}

/// Everything one load remembers (the locals of the C# `LoadAsync` that live across an `await`).
#[derive(Debug)]
pub(super) struct ActiveLoad {
    pub load_id: u64,
    /// The folder as normalized by `get_full_path` + `trim_ending_directory_separator` (the argument until then).
    pub folder: String,
    pub initial_path: Option<String>,
    pub token: CancelToken,
    /// C# `loadGeneration` (`GenerationClock::next_folder`).
    pub load_generation: i64,
    pub phase: LoadPhase,
    pub explorer: ExplorerSlot,
    /// C# `skipped` (listing).
    pub skipped: Vec<SkippedEntry>,
    pub scanned_files: Arc<[String]>,
    /// C# `interactionGeneration` captured right after `OnCatalogReady`.
    pub interaction_generation: i64,
    /// C# `presentationGeneration` read after the first present/empty branch.
    pub presentation_generation: i64,
    /// This load owns the pending-order gate (file open whose snapshot was not in at catalog time).
    pub owns_order_gate: bool,
    pub probe: ProbeState,
}

/// C# `FolderLoadCoordinator`. Owned by the UI thread; not `Send`.
pub struct FolderLoadCoordinator {
    pub(super) deps: FolderLoadDeps,
    /// C# `_loadCts` + the locals of the running load. `None` when idle.
    pub(super) active: Option<ActiveLoad>,
    /// C# `_pendingOrder != null && !completed`.
    pub(super) order_gate_open: bool,
    pub(super) next_load_id: u64,
    pub(super) disposed: bool,
}

impl fmt::Debug for FolderLoadCoordinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FolderLoadCoordinator")
            .field("phase", &self.active.as_ref().map(|a| a.phase))
            .field("order_gate_open", &self.order_gate_open)
            .field("disposed", &self.disposed)
            .finish_non_exhaustive()
    }
}

impl FolderLoadCoordinator {
    /// C# constructor: stores the collaborators, starts no work, calls nothing on them.
    pub fn new(deps: FolderLoadDeps) -> Self {
        Self {
            deps,
            active: None,
            order_gate_open: false,
            next_load_id: 0,
            disposed: false,
        }
    }

    /// Which `await` the active load is parked on; `Idle` when none.
    pub fn phase(&self) -> LoadPhase {
        self.active.as_ref().map_or(LoadPhase::Idle, |active| active.phase)
    }

    /// C# `Dispose` was called.
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }
}
