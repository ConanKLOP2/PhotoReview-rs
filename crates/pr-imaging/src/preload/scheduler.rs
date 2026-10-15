//! Port of `PhotoReview.Imaging/Preload/PreloadScheduler.cs` (C# baseline d92d6aae, 966 lines, 57 KB): the scheduler
//! shell. It owns the cancellation lifetimes, the run threads, the worker slots, the wake-up on navigation, the
//! memory-pause auto-resume, the warmed-key set and disposal. WHAT to preload next is decided by
//! [`super::queue::RunQueue`] (W4-15a); the pure rules and log texts are in [`super::policy`] (W4-15a). Contract:
//! `contracts/W4-15b.md` (draft `contracts/drafts-w4/W4-15b/contract.md`). PARITY I-11, P-16, INV-1 (fresh key after
//! decode), C-05/C-20 (cleared keys).
//!
//! Target path: `crates/pr-imaging/src/preload/scheduler.rs`.
//!
//! Threads (R0003, no async runtime): `preload_around` runs the run's first steps on the CALLER's thread (C# the
//! synchronous prefix of `RunPreloadSchedulerAsync`: order build, window candidates, first `workers` dispatches) and
//! moves the rest to one named run thread (`pr-preload-run`) at the first point where C# would yield. Decodes run on
//! the worker pool (`worker.rs`), never on the caller's thread. No lock is held while calling the target, the probe,
//! the log, a snapshot callback or the prefetch.

#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicI32, AtomicI64};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use pr_core::abstractions::{Log, MemoryProbe};
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::sync::CancelToken;

use crate::cache_key::ImageCacheKey; // W4-01a
use crate::decode_types::DecodeBox; // W4-01b
use crate::decoder_traits::DecodeError; // W4-01c
use crate::navigation_pace::NavigationPace; // W4-06a
use crate::preload_backoff::{PreloadBusyBackoff, PreviewSizeSampler}; // W4-06b
use crate::preload_order::{PreloadOptions, PreloadWindow}; // W4-05b

use super::policy::{PreloadOutcome, Shape};
use super::queue::{Candidate, RunQueue};
use super::target::PreloadTarget;
use super::timer::{OneShotTimer, OneShotTimerFactory};
use super::worker::{Lifetime, RunEvents, WorkerPool};

/// Error type of the injected callbacks (C# a callback that throws).
pub type CallbackError = Box<dyn Error + Send + Sync>;

/// C# `Func<CatalogEntry[]> snapshotEntries`: the current catalog entries, in catalog order. Called on the caller's
/// thread when a run starts. `Err` (C# a throw while the catalog is being replaced) is logged as
/// `"Preload kick failed; skipped"` and the kick does nothing (I6). The `Arc` identity is the snapshot identity of the
/// whole-folder estimate cache (W4-07a).
pub type EntriesSnapshotFn = Arc<dyn Fn() -> Result<Arc<[CatalogEntry]>, CallbackError> + Send + Sync>;

/// C# `Func<int>? snapshotVersion` (RV-I13, `ReviewCatalog::structural_version`): cheap change counter of the list behind
/// the entries callback. `Err` behaves like a throwing entries callback (I6).
pub type SnapshotVersionFn = Arc<dyn Fn() -> Result<i32, CallbackError> + Send + Sync>;

/// C# `Func<string, CancellationToken, Task>? prefetchSourceBytes` awaited: reads the whole source file into the
/// source-bytes cache (W4-10a) before the decode. BLOCKING, called on a worker thread. Errors go through the same arms
/// as the decode's (`Busy`, `Canceled`, `MissingCodec`, I/O, other).
pub type PrefetchSourceBytesFn = Arc<dyn Fn(&str, &CancelToken) -> Result<(), DecodeError> + Send + Sync>;

/// Environment lookup (C# `Environment.GetEnvironmentVariable`); `None` = unset. Tests inject one instead of mutating
/// the process environment.
pub type EnvLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Monotonic millisecond clock of the headroom cache (C# `Environment.TickCount64`).
pub type MillisecondClock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// A test seam action (C# `Action`).
pub type TestHook = Arc<dyn Fn() + Send + Sync>;

/// A test seam action with the path (C# `Action<string>`).
pub type PathHook = Arc<dyn Fn(&str) + Send + Sync>;

/// The optional constructor arguments of the main C# constructor (all `None` = the C# defaults).
#[derive(Default, Clone)]
pub struct PreloadSchedulerDeps {
    /// C# `options ?? new PreloadOptions()`.
    pub options: Option<PreloadOptions>,
    /// C# `log ?? NullLog.Instance`.
    pub log: Option<Arc<dyn Log>>,
    /// C# `prefetchSourceBytes` (no prefetch when `None`).
    pub prefetch_source_bytes: Option<PrefetchSourceBytesFn>,
    /// C# `pace ?? new NavigationPace()`.
    pub pace: Option<Arc<NavigationPace>>,
    /// C# `snapshotVersion` (`None` = the list never changes: version 0).
    pub snapshot_version: Option<SnapshotVersionFn>,
    /// C# `timeProvider ?? TimeProvider.System` (`None` = [`super::timer::ThreadTimerFactory`]).
    pub timers: Option<Arc<dyn OneShotTimerFactory>>,
    /// `None` = `std::env::var` (D4: the variable is `PHOTOREVIEW_RS_DIAG_PRELOAD_WORKERS`,
    /// `pr_core::diagnostics::diag_options::PRELOAD_WORKERS_VAR`).
    pub env_lookup: Option<EnvLookup>,
    /// `None` = `std::thread::available_parallelism()` (C# `Environment.ProcessorCount`; 1 when unknown).
    pub processor_count: Option<i32>,
    /// `None` = an `Instant`-based clock.
    pub clock_ms: Option<MillisecondClock>,
}

impl fmt::Debug for PreloadSchedulerDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreloadSchedulerDeps")
            .field("options", &self.options)
            .field("processor_count", &self.processor_count)
            .finish_non_exhaustive()
    }
}

/// The arguments of the C# convenience constructor (`fullFolderRamThresholdBytes, memoryLoadLimit, ...,
/// workerCountOverride, ..., window`). They become `PreloadOptions { worker_count: override ?? env ?? 8,
/// memory_load_limit, reserve_bytes: default, full_folder_threshold_bytes, window: window ?? DEFAULT }`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThresholdOptions {
    pub full_folder_ram_threshold_bytes: i64,
    pub memory_load_limit: f64,
    pub worker_count_override: Option<i32>,
    pub window: Option<PreloadWindow>,
}

/// Completion of one scheduler run (C# the `Task` returned by `PreloadAroundAsync`). Clones share the run; a kick that
/// JOINS a running run returns a clone of the same handle ([`RunHandle::same_run`] is C# `Assert.Same`).
#[derive(Clone)]
pub struct RunHandle {
    state: Arc<(Mutex<bool>, Condvar)>,
}

impl fmt::Debug for RunHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunHandle").finish_non_exhaustive()
    }
}

impl RunHandle {
    /// An already completed handle (C# `Task.CompletedTask`; every no-op kick returns a NEW one).
    pub fn completed() -> Self {
        todo!("W4-15b")
    }

    /// `true` once the run finished, including draining every worker it started.
    pub fn is_complete(&self) -> bool {
        todo!("W4-15b")
    }

    /// Blocks until complete. Never call on a thread the run needs (tests call it with a guard via `wait_timeout`).
    pub fn wait(&self) {
        todo!("W4-15b")
    }

    /// Blocks until complete or `timeout` elapsed; `true` when complete (C# `Task.WaitAsync(timeout)` / `WaitAll`).
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        todo!("W4-15b")
    }

    /// Same run (C# `ReferenceEquals` of the two tasks).
    pub fn same_run(&self, other: &RunHandle) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    /// Marks complete and wakes waiters (idempotent).
    pub(crate) fn complete(&self) {
        todo!("W4-15b")
    }
}

/// Owns background preload (C# `public sealed class PreloadScheduler : IDisposable`). `Drop` = `dispose()`.
///
/// Thread-safe: every method takes `&self` and may be called from any thread; the App calls `notify_navigation`,
/// `preload_around`, `cancel` and `dispose` from the UI thread.
pub struct PreloadScheduler {
    shared: Arc<Shared>,
}

impl fmt::Debug for PreloadScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreloadScheduler").finish_non_exhaustive()
    }
}

/// Everything the run threads, workers and the re-check timer share (C# the scheduler's fields).
struct Shared {
    target: Arc<dyn PreloadTarget>,
    metrics: Arc<ReviewMetrics>,
    snapshot_entries: EntriesSnapshotFn,
    snapshot_version: Option<SnapshotVersionFn>,
    options: PreloadOptions,
    memory_probe: Arc<dyn MemoryProbe>,
    log: Arc<dyn Log>,
    prefetch_source_bytes: Option<PrefetchSourceBytesFn>,
    pace: Arc<NavigationPace>,
    sizes: PreviewSizeSampler,
    busy_backoff: PreloadBusyBackoff,
    viewer_busy_worker_limit: i32,
    /// C# `_workerCount = DiagOptionsWorkers() ?? options.WorkerCount` (0 = preload disabled).
    worker_count: i32,
    timers: Arc<dyn OneShotTimerFactory>,
    clock_ms: MillisecondClock,
    /// C# `_preloadCenter` (Volatile).
    preload_center: AtomicI32,
    /// C# `_preloadPriorityVersion` (Interlocked; starts at 0, wrapping increments).
    priority_version: AtomicI64,
    /// C# `_preloadedKeys` + `_preloadedKeysBox`, under one lock (C# `_preloadedKeysGate`).
    preloaded_keys: Mutex<PreloadedKeys>,
    /// C# `_preloadCtsGate` and the fields it guards.
    gate: Mutex<GateState>,
    events: RunEvents,
    pool: WorkerPool,
    /// Test seams (C# internal properties).
    seams: Mutex<Seams>,
}

/// C# `_preloadedKeys` / `_preloadedKeysBox` (RV-I15).
#[derive(Debug, Default)]
struct PreloadedKeys {
    keys: HashSet<ImageCacheKey>,
    decode_box: DecodeBox,
}

/// The fields C# guards with `_preloadCtsGate`.
#[derive(Default)]
struct GateState {
    /// C# `_preloadCts` (the current lifetime).
    current: Option<Lifetime>,
    next_lifetime_id: u64,
    /// C# `_preloadLifetimes`.
    lifetimes: Vec<Lifetime>,
    /// C# `_preloadLifetimeTasks`: (lifetime id, run handle).
    lifetime_runs: Vec<(u64, RunHandle)>,
    /// C# `_preloadSchedulerTask` / `_preloadSchedulerCts`.
    scheduler_run: Option<RunHandle>,
    scheduler_lifetime_id: Option<u64>,
    /// C# `_schedulerRunId`, `_schedulerRunExiting`, `_schedulerSnapshotVersion` (RV-I12/RV-I13).
    run_id: u64,
    run_exiting: bool,
    snapshot_version: i32,
    disposed: bool,
    /// C# `_resumeTimer`, `_resumeGeneration`, `_resumeChecks`.
    resume_timer: Option<Box<dyn OneShotTimer>>,
    resume_generation: i64,
    resume_checks: i32,
    /// C# test seams `ResumesForTests`, `ResumedRunForTests`.
    resumes_for_tests: i32,
    resumed_run_for_tests: Option<RunHandle>,
}

/// C# internal test seams (settable before the first pass).
#[derive(Default)]
struct Seams {
    before_scheduler_exit: Option<TestHook>,
    busy_noted: Option<PathHook>,
    dispatched_on: Option<PathHook>,
    dispose_drain_timeout: Option<Duration>,
    late_disposal: Option<RunHandle>,
}

impl PreloadScheduler {
    /// C# main constructor. `worker_count = parse_diag_workers(env(PRELOAD_WORKERS_VAR)) ?? options.worker_count` (the
    /// diagnostic variable WINS over the option); slots `max(1, worker_count)`; the first lifetime is created and
    /// tracked; `viewer_busy_worker_limit = policy::viewer_busy_worker_limit(processor_count)`. No thread starts.
    /// (C# `ArgumentNullException`s for null target/metrics/entries/probe have no Rust twin: the types forbid them.)
    pub fn new(
        target: Arc<dyn PreloadTarget>,
        metrics: Arc<ReviewMetrics>,
        snapshot_entries: EntriesSnapshotFn,
        memory_probe: Arc<dyn MemoryProbe>,
        deps: PreloadSchedulerDeps,
    ) -> Self {
        todo!("W4-15b: see the doc comment")
    }

    /// C# convenience constructor: `PreloadOptions { worker_count: thresholds.worker_count_override ??
    /// parse_diag_workers(env) ?? 8, memory_load_limit, full_folder_threshold_bytes, window: window ?? DEFAULT, reserve:
    /// default }` then [`new`](Self::new) with `deps.options` REPLACED by those options and `deps.pace` IGNORED (C#
    /// passes `pace: null`). Note: `new` re-reads the environment, so the variable still wins over the override. The
    /// Rust-only seams `timers`, `env_lookup`, `processor_count` and `clock_ms` are honoured (C# has no parameter for
    /// them here: `TimeProvider.System`, the process environment and clock).
    pub fn with_thresholds(
        target: Arc<dyn PreloadTarget>,
        metrics: Arc<ReviewMetrics>,
        snapshot_entries: EntriesSnapshotFn,
        memory_probe: Arc<dyn MemoryProbe>,
        thresholds: ThresholdOptions,
        deps: PreloadSchedulerDeps,
    ) -> Self {
        todo!("W4-15b: see the doc comment")
    }

    /// The effective worker count (C# private `_workerCount`, read by reflection in `PreloadSchedulerGapTests`).
    pub fn worker_count(&self) -> i32 {
        todo!("W4-15b")
    }

    /// C# `Cancel()`: under the gate, unless disposed: cancel the current lifetime, retire the memory re-check (reset
    /// its budget); then wake every run, complete waiting jobs of that lifetime as cancelled, emit perf
    /// `preload_cancel("cancel")`. The next `preload_around` starts a fresh lifetime.
    pub fn cancel(&self) {
        todo!("W4-15b")
    }

    /// C# `ClearPreloadedKeys()`: drop every warmed key (folder reload / cache clear, PARITY C-05/C-20) and the busy
    /// back-off history.
    pub fn clear_preloaded_keys(&self) {
        todo!("W4-15b")
    }

    /// C# `RemovePreloadedKeysForPath(normalizedPath)`: remove the warmed keys whose `path()` equals `normalized_path`
    /// ORDINALLY (it is already the full, upper-invariant path of a key).
    pub fn remove_preloaded_keys_for_path(&self, normalized_path: &str) {
        todo!("W4-15b")
    }

    /// C# `IsIdle`: no run yet, or the newest run is complete (a paused run with a pending re-check counts as idle).
    pub fn is_idle(&self) -> bool {
        todo!("W4-15b")
    }

    /// C# `TryConsumePreloadedKey(key)`: `true` when preload warmed `key`; consumes it (the viewer records a preload hit).
    pub fn try_consume_preloaded_key(&self, key: &ImageCacheKey) -> bool {
        todo!("W4-15b")
    }

    /// C# `NotifyNavigation(index)`, at the START of every navigation. No-op with 0 workers (not even the pace). Records
    /// the pace; under the gate (unless disposed): centre = index, version += 1, retire the re-check (reset budget);
    /// wake; when the lead is now > 0 (a burst) also `preload_around(index)` (its handle is dropped).
    pub fn notify_navigation(&self, index: i32) {
        todo!("W4-15b")
    }

    /// C# `GetViewerDecodeDelay()`: zero with 0 workers, else `pace.get_viewer_start_delay(metrics.decode_ms_ewma)`.
    pub fn get_viewer_decode_delay(&self) -> Duration {
        todo!("W4-15b")
    }

    /// C# `PreloadAroundAsync(center)`: start or join the run that preloads around `center`. Never fails: a failing
    /// snapshot callback (or a panic in the kick) is logged as `"Preload kick failed; skipped"` and a completed handle is
    /// returned. Runs the run's first steps on the calling thread (see the module doc and contract semantics 3-5).
    pub fn preload_around(&self, center: i32) -> RunHandle {
        todo!("W4-15b: PreloadAroundCore(center, resume_generation: None) inside the catch")
    }

    /// C# `Dispose()`: idempotent. Under the gate: disposed, cancel the current lifetime, retire the re-check, copy the
    /// lifetime runs; wake; complete waiting jobs as cancelled; wait for every lifetime run up to the drain timeout
    /// (default 3 s). In time: stop the pool (join). Too late: Warn `MSG_DRAIN_TIMEOUT`, record the late-disposal
    /// handle that stops the pool once every lifetime run completed, and return. Afterwards `preload_around`,
    /// `notify_navigation` and `cancel` are no-ops; the warmed-key methods, `is_idle` and `get_viewer_decode_delay` keep
    /// working (C# has no disposed check there).
    pub fn dispose(&self) {
        todo!("W4-15b")
    }

    // ---- test seams (C# `internal`; `pub` + `#[doc(hidden)]` so `tests/` can reach them) ----------------------------

    /// C# `ResumedRunForTests`: the run the last successful memory re-check started (a completed handle when superseded).
    #[doc(hidden)]
    pub fn resumed_run_for_tests(&self) -> Option<RunHandle> {
        todo!("W4-15b")
    }

    /// C# `ResumesForTests`: memory re-checks that actually kicked a resumed run.
    #[doc(hidden)]
    pub fn resumes_for_tests(&self) -> i32 {
        todo!("W4-15b")
    }

    /// C# `BeforeSchedulerExitForTests` (RV-I12): runs on the loop's exit path, after its final version check, before the
    /// run completes (on whatever thread the run is on).
    #[doc(hidden)]
    pub fn set_before_scheduler_exit_for_tests(&self, hook: Option<TestHook>) {
        todo!("W4-15b")
    }

    /// C# `PreloadedKeyCountForTests` (RV-I15).
    #[doc(hidden)]
    pub fn preloaded_key_count_for_tests(&self) -> usize {
        todo!("W4-15b")
    }

    /// C# `BusyNoted`: called with the path right AFTER a busy outcome was recorded in the back-off.
    #[doc(hidden)]
    pub fn set_busy_noted_for_tests(&self, hook: Option<PathHook>) {
        todo!("W4-15b")
    }

    /// NEW seam (no C# twin): called with the path on the thread that DISPATCHES a candidate to the worker pool. Ports the
    /// C# "PreloadThread == caller" assertions (see contract, test notes).
    #[doc(hidden)]
    pub fn set_dispatched_for_tests(&self, hook: Option<PathHook>) {
        todo!("W4-15b")
    }

    /// C# `TrackedLifetimeCounts`: (lifetimes, lifetime runs) still tracked for dispose; must stay bounded.
    #[doc(hidden)]
    pub fn tracked_lifetime_counts(&self) -> (usize, usize) {
        todo!("W4-15b")
    }

    /// C# `DisposeDrainTimeout` setter (default [`super::policy::DEFAULT_DISPOSE_DRAIN_TIMEOUT`]).
    #[doc(hidden)]
    pub fn set_dispose_drain_timeout(&self, timeout: Duration) {
        todo!("W4-15b")
    }

    /// C# `LateDisposalForTests`: `None` when dispose drained in time; else completes after the blocked work finished and
    /// the pool was stopped.
    #[doc(hidden)]
    pub fn late_disposal_for_tests(&self) -> Option<RunHandle> {
        todo!("W4-15b")
    }
}

impl Drop for PreloadScheduler {
    fn drop(&mut self) {
        todo!("W4-15b: self.dispose()")
    }
}

impl Shared {
    /// C# `PreloadAroundCore(center, resumeGeneration)`: see contract semantics 3. `resume_generation` is `Some` only
    /// from the memory re-check.
    fn preload_around_core(
        self: &Arc<Self>,
        center: i32,
        resume_generation: Option<i64>,
    ) -> Result<RunHandle, CallbackError> {
        todo!("W4-15b")
    }

    /// C# `RunPreloadSchedulerAsync`: the run loop, see contract semantics 4-5. Starts on the caller's thread and moves
    /// itself to a run thread at the first yield point.
    fn run(self: &Arc<Self>, queue: RunQueue, run_id: u64, lifetime: Lifetime, handle: RunHandle) {
        todo!("W4-15b")
    }

    /// C# `PreloadOneAsync` body after the slot was taken (cache re-check, still-wanted, prefetch, decode, fresh key,
    /// warmed keys, size sample, error arms). Runs on a pool thread except the two short-circuit checks (semantics 6).
    fn preload_one(&self, candidate: &Candidate, slot: i32, enqueued_at: i64, cancel: &CancelToken) -> PreloadOutcome {
        todo!("W4-15b")
    }

    /// C# `TryExitRun(runId, seenVersion, paused, ct)` (RV-I12), under the gate.
    fn try_exit_run(self: &Arc<Self>, run_id: u64, seen_version: i64, paused: bool, cancel: &CancelToken) -> bool {
        todo!("W4-15b")
    }

    /// C# `MarkRunExiting(runId)`.
    fn mark_run_exiting(&self, run_id: u64) {
        todo!("W4-15b")
    }

    /// C# `PruneFinishedLifetimes()`; caller holds the gate.
    fn prune_finished_lifetimes(gate: &mut GateState) {
        todo!("W4-15b")
    }

    /// C# `ArmResumeCheckLocked()`; caller holds the gate. Only creates a timer (no probe/target/log call under the gate).
    fn arm_resume_check_locked(self: &Arc<Self>, gate: &mut GateState) {
        todo!("W4-15b")
    }

    /// C# `CancelResumeCheckLocked(resetChecks)`; caller holds the gate.
    fn cancel_resume_check_locked(gate: &mut GateState, reset_checks: bool) {
        todo!("W4-15b")
    }

    /// C# `OnResumeCheck(generation)`: the timer callback (never panics out: everything is caught and logged).
    fn on_resume_check(self: &Arc<Self>, generation: i64) {
        todo!("W4-15b")
    }

    /// C# `CurrentShape()`: `(pace.direction(), pace.get_lead(metrics.decode_milliseconds_ewma()))`.
    fn current_shape(&self) -> Shape {
        todo!("W4-15b")
    }
}
