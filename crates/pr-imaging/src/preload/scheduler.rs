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
//! the log, a hook or the prefetch; the only callbacks made under the gate are the entries snapshot (documented: it
//! must not call the scheduler) and `OneShotTimerFactory::create_timer` (must not call back synchronously).
//!
//! `MSG_FAILED_DURING_DISPOSAL` has no Rust path: a run never "faults" (its panics are caught and logged as
//! `MSG_SCHEDULER_FAILED`), so `dispose` has nothing to report (contract open question 4).

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use pr_core::abstractions::{Log, MemoryProbe, NullLog};
use pr_core::catalog::types::{CatalogEntry, is_raw_path};
use pr_core::diagnostics::diag_options::PRELOAD_WORKERS_VAR;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::diagnostics::perf::{self, PhotoReviewPerf};
use pr_core::sync::CancelToken;

use crate::cache_key::ImageCacheKey; // W4-01a
use crate::decode_types::DecodeBox; // W4-01b
use crate::decoder_traits::DecodeError; // W4-01c
use crate::navigation_pace::NavigationPace; // W4-06a
use crate::preload_backoff::{PreloadBusyBackoff, PreviewSizeSampler}; // W4-06b
use crate::preload_order::{PreloadOptions, PreloadWindow}; // W4-05b

use super::policy::{
    self, DEFAULT_DISPOSE_DRAIN_TIMEOUT, MAX_RESUME_CHECKS, MSG_DRAIN_TIMEOUT, MSG_KICK_FAILED, MSG_RECHECK_FAILED,
    MSG_SCHEDULER_FAILED, PreloadOutcome, ResumeVerdict, Shape,
};
use super::queue::{Candidate, PassInputs, QueueContext, QueueStep, RunQueue};
use super::target::{KeyError, PreloadTarget};
use super::timer::{OneShotTimer, OneShotTimerFactory, ThreadTimerFactory};
use super::worker::{Finished, Job, Lifetime, RunEvents, Wakeup, WorkerEnd, WorkerPool, lock, panic_message};

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
        Self {
            state: Arc::new((Mutex::new(true), Condvar::new())),
        }
    }

    /// A handle that completes when [`RunHandle::complete`] is called.
    fn pending() -> Self {
        Self {
            state: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// `true` once the run finished, including draining every worker it started.
    pub fn is_complete(&self) -> bool {
        *lock(&self.state.0)
    }

    /// Blocks until complete. Never call on a thread the run needs (tests call it with a guard via `wait_timeout`).
    pub fn wait(&self) {
        let (done, changed) = &*self.state;
        let mut done = lock(done);
        while !*done {
            done = changed.wait(done).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Blocks until complete or `timeout` elapsed; `true` when complete (C# `Task.WaitAsync(timeout)` / `WaitAll`).
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let deadline = Instant::now().checked_add(timeout);
        let (done, changed) = &*self.state;
        let mut done = lock(done);
        loop {
            if *done {
                return true;
            }
            let remaining = match deadline {
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                None => Duration::MAX,
            };
            if remaining.is_zero() {
                return false;
            }
            done = changed
                .wait_timeout(done, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Same run (C# `ReferenceEquals` of the two tasks).
    pub fn same_run(&self, other: &RunHandle) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    /// Marks complete and wakes waiters (idempotent).
    pub(crate) fn complete(&self) {
        let (done, changed) = &*self.state;
        *lock(done) = true;
        changed.notify_all();
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
struct GateState {
    /// C# `_preloadCts` (the current lifetime).
    current: Lifetime,
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

/// A caught panic (C# the exception a fake throws), logged as the error of the catch site that caught it.
#[derive(Debug)]
struct PanicError(String);

impl fmt::Display for PanicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for PanicError {}

/// Runs `body`, turning a panic into an error (the C# catch sites: kick, loop, item, timer callback).
fn catch<T>(body: impl FnOnce() -> Result<T, CallbackError>) -> Result<T, CallbackError> {
    panic::catch_unwind(AssertUnwindSafe(body))
        .unwrap_or_else(|payload| Err(Box::new(PanicError(panic_message(payload.as_ref())))))
}

/// The default processor count (C# `Environment.ProcessorCount`).
fn default_processor_count() -> i32 {
    thread::available_parallelism().map_or(1, |n| i32::try_from(n.get()).unwrap_or(i32::MAX))
}

/// The default monotonic millisecond clock.
fn instant_clock() -> MillisecondClock {
    let start = Instant::now();
    Arc::new(move || i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX))
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
        let options = deps.options.unwrap_or_default();
        let env = deps
            .env_lookup
            .unwrap_or_else(|| Arc::new(|name: &str| std::env::var(name).ok()));
        let worker_count =
            policy::parse_diag_workers(env(PRELOAD_WORKERS_VAR).as_deref()).unwrap_or(options.worker_count);
        let processor_count = deps.processor_count.unwrap_or_else(default_processor_count);
        let first = Lifetime {
            id: 1,
            token: CancelToken::new(),
        };
        let gate = GateState {
            current: first.clone(),
            next_lifetime_id: 2,
            lifetimes: vec![first],
            lifetime_runs: Vec::new(),
            scheduler_run: None,
            scheduler_lifetime_id: None,
            run_id: 0,
            run_exiting: false,
            snapshot_version: 0,
            disposed: false,
            resume_timer: None,
            resume_generation: 0,
            resume_checks: 0,
            resumes_for_tests: 0,
            resumed_run_for_tests: None,
        };
        let shared = Shared {
            target,
            metrics,
            snapshot_entries,
            snapshot_version: deps.snapshot_version,
            options,
            memory_probe,
            log: deps.log.unwrap_or_else(|| Arc::new(NullLog)),
            prefetch_source_bytes: deps.prefetch_source_bytes,
            pace: deps.pace.unwrap_or_else(|| Arc::new(NavigationPace::new())),
            sizes: PreviewSizeSampler::default(),
            busy_backoff: PreloadBusyBackoff::new(),
            viewer_busy_worker_limit: policy::viewer_busy_worker_limit(processor_count),
            worker_count,
            timers: deps.timers.unwrap_or_else(|| Arc::new(ThreadTimerFactory)),
            clock_ms: deps.clock_ms.unwrap_or_else(instant_clock),
            preload_center: AtomicI32::new(0),
            priority_version: AtomicI64::new(0),
            preloaded_keys: Mutex::new(PreloadedKeys::default()),
            gate: Mutex::new(gate),
            events: RunEvents::default(),
            pool: WorkerPool::new(worker_count),
            seams: Mutex::new(Seams::default()),
        };
        Self {
            shared: Arc::new(shared),
        }
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
        let env_worker_count = || {
            let value = match &deps.env_lookup {
                Some(lookup) => lookup(PRELOAD_WORKERS_VAR),
                None => std::env::var(PRELOAD_WORKERS_VAR).ok(),
            };
            policy::parse_diag_workers(value.as_deref())
        };
        let worker_count = thresholds
            .worker_count_override
            .or_else(env_worker_count)
            .unwrap_or(policy::CONVENIENCE_DEFAULT_WORKER_COUNT);
        let options = PreloadOptions {
            worker_count,
            memory_load_limit: thresholds.memory_load_limit,
            full_folder_threshold_bytes: thresholds.full_folder_ram_threshold_bytes,
            window: thresholds.window.unwrap_or(PreloadWindow::DEFAULT),
            ..PreloadOptions::default()
        };
        let deps = PreloadSchedulerDeps {
            options: Some(options),
            pace: None,
            ..deps
        };
        Self::new(target, metrics, snapshot_entries, memory_probe, deps)
    }

    /// The effective worker count (C# private `_workerCount`, read by reflection in `PreloadSchedulerGapTests`).
    pub fn worker_count(&self) -> i32 {
        self.shared.worker_count
    }

    /// C# `Cancel()`: under the gate, unless disposed: cancel the current lifetime, retire the memory re-check (reset
    /// its budget); then wake every run, complete waiting jobs of that lifetime as cancelled, emit perf
    /// `preload_cancel("cancel")`. The next `preload_around` starts a fresh lifetime.
    pub fn cancel(&self) {
        let shared = &self.shared;
        let cancelled = {
            let mut gate = lock(&shared.gate);
            if gate.disposed {
                return;
            }
            gate.current.token.cancel();
            Shared::cancel_resume_check_locked(&mut gate, true); // folder change/file action: no re-check survives it
            gate.current.id
        };
        shared.events.wake();
        shared.pool.cancel_waiting(cancelled);
        let perf_log = PhotoReviewPerf::log();
        if perf_log.is_enabled() {
            perf_log.preload_cancel("cancel");
        }
    }

    /// C# `ClearPreloadedKeys()`: drop every warmed key (folder reload / cache clear, PARITY C-05/C-20) and the busy
    /// back-off history.
    pub fn clear_preloaded_keys(&self) {
        lock(&self.shared.preloaded_keys).keys.clear();
        self.shared.busy_backoff.clear();
    }

    /// C# `RemovePreloadedKeysForPath(normalizedPath)`: remove the warmed keys whose `path()` equals `normalized_path`
    /// ORDINALLY (it is already the full, upper-invariant path of a key).
    pub fn remove_preloaded_keys_for_path(&self, normalized_path: &str) {
        lock(&self.shared.preloaded_keys)
            .keys
            .retain(|key| key.path() != normalized_path);
    }

    /// C# `IsIdle`: no run yet, or the newest run is complete (a paused run with a pending re-check counts as idle).
    pub fn is_idle(&self) -> bool {
        lock(&self.shared.gate)
            .scheduler_run
            .as_ref()
            .is_none_or(RunHandle::is_complete)
    }

    /// C# `TryConsumePreloadedKey(key)`: `true` when preload warmed `key`; consumes it (the viewer records a preload hit).
    pub fn try_consume_preloaded_key(&self, key: &ImageCacheKey) -> bool {
        lock(&self.shared.preloaded_keys).keys.remove(key)
    }

    /// C# `NotifyNavigation(index)`, at the START of every navigation. No-op with 0 workers (not even the pace). Records
    /// the pace; under the gate (unless disposed): centre = index, version += 1, retire the re-check (reset budget);
    /// wake; when the lead is now > 0 (a burst) also `preload_around(index)` (its handle is dropped).
    pub fn notify_navigation(&self, index: i32) {
        let shared = &self.shared;
        if shared.worker_count == 0 {
            return;
        }
        shared.pace.record(index);
        {
            let mut gate = lock(&shared.gate);
            if gate.disposed {
                return;
            }
            shared.preload_center.store(index, Ordering::SeqCst);
            shared.priority_version.fetch_add(1, Ordering::SeqCst);
            // A navigation re-kicks preload itself (here during a burst, else preload_around after present).
            Shared::cancel_resume_check_locked(&mut gate, true);
        }
        shared.events.wake();
        if shared.current_shape().lead > 0 {
            let _ = self.preload_around(index);
        }
    }

    /// C# `GetViewerDecodeDelay()`: zero with 0 workers, else `pace.get_viewer_start_delay(metrics.decode_ms_ewma)`.
    pub fn get_viewer_decode_delay(&self) -> Duration {
        if self.shared.worker_count == 0 {
            Duration::ZERO
        } else {
            self.shared
                .pace
                .get_viewer_start_delay(self.shared.metrics.decode_milliseconds_ewma())
        }
    }

    /// C# `PreloadAroundAsync(center)`: start or join the run that preloads around `center`. Never fails: a failing
    /// snapshot callback (or a panic in the kick) is logged as `"Preload kick failed; skipped"` and a completed handle is
    /// returned. Runs the run's first steps on the calling thread (see the module doc and contract semantics 3-5).
    pub fn preload_around(&self, center: i32) -> RunHandle {
        match catch(|| self.shared.preload_around_core(center, None)) {
            Ok(handle) => handle,
            Err(error) => {
                self.shared.log.error(MSG_KICK_FAILED, Some(error.as_ref()));
                RunHandle::completed()
            }
        }
    }

    /// C# `Dispose()`: idempotent. Under the gate: disposed, cancel the current lifetime, retire the re-check, copy the
    /// lifetime runs; wake; complete waiting jobs as cancelled; wait for every lifetime run up to the drain timeout
    /// (default 3 s). In time: stop the pool (join). Too late: Warn `MSG_DRAIN_TIMEOUT`, record the late-disposal
    /// handle that stops the pool once every lifetime run completed, and return. Afterwards `preload_around`,
    /// `notify_navigation` and `cancel` are no-ops; the warmed-key methods, `is_idle` and `get_viewer_decode_delay` keep
    /// working (C# has no disposed check there).
    pub fn dispose(&self) {
        let shared = &self.shared;
        let (runs, lifetimes) = {
            let mut gate = lock(&shared.gate);
            if gate.disposed {
                return;
            }
            gate.disposed = true;
            gate.current.token.cancel();
            Shared::cancel_resume_check_locked(&mut gate, true);
            let runs: Vec<RunHandle> = gate.lifetime_runs.iter().map(|(_, run)| run.clone()).collect();
            let lifetimes: Vec<u64> = gate.lifetimes.iter().map(|lifetime| lifetime.id).collect();
            (runs, lifetimes)
        };
        shared.events.wake();
        for lifetime in lifetimes {
            shared.pool.cancel_waiting(lifetime);
        }

        let timeout = lock(&shared.seams)
            .dispose_drain_timeout
            .unwrap_or(DEFAULT_DISPOSE_DRAIN_TIMEOUT);
        let deadline = Instant::now().checked_add(timeout);
        let drained = runs.iter().all(|run| {
            let remaining = match deadline {
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                None => Duration::MAX,
            };
            run.wait_timeout(remaining)
        });
        if drained {
            shared.pool.shutdown(true);
            return;
        }

        // A decode that cannot be cancelled is still running: do not hold the caller for it. The pool is stopped once
        // the last lifetime run has finished.
        shared.log.warn(MSG_DRAIN_TIMEOUT);
        let late = RunHandle::pending();
        lock(&shared.seams).late_disposal = Some(late.clone());
        let owner = Arc::clone(shared);
        let finisher = late.clone();
        let spawned = thread::Builder::new()
            .name("pr-preload-late-dispose".to_owned())
            .spawn(move || {
                for run in &runs {
                    run.wait();
                }
                owner.pool.shutdown(true);
                finisher.complete();
            });
        if spawned.is_err() {
            // No thread for the late disposal: let the pool threads end on their own once their queues are empty.
            shared.pool.shutdown(false);
            late.complete();
        }
    }

    // ---- test seams (C# `internal`; `pub` + `#[doc(hidden)]` so `tests/` can reach them) ----------------------------

    /// C# `ResumedRunForTests`: the run the last successful memory re-check started (a completed handle when superseded).
    #[doc(hidden)]
    pub fn resumed_run_for_tests(&self) -> Option<RunHandle> {
        lock(&self.shared.gate).resumed_run_for_tests.clone()
    }

    /// C# `ResumesForTests`: memory re-checks that actually kicked a resumed run.
    #[doc(hidden)]
    pub fn resumes_for_tests(&self) -> i32 {
        lock(&self.shared.gate).resumes_for_tests
    }

    /// C# `BeforeSchedulerExitForTests` (RV-I12): runs on the loop's exit path, after its final version check, before the
    /// run completes (on whatever thread the run is on).
    #[doc(hidden)]
    pub fn set_before_scheduler_exit_for_tests(&self, hook: Option<TestHook>) {
        lock(&self.shared.seams).before_scheduler_exit = hook;
    }

    /// C# `PreloadedKeyCountForTests` (RV-I15).
    #[doc(hidden)]
    pub fn preloaded_key_count_for_tests(&self) -> usize {
        lock(&self.shared.preloaded_keys).keys.len()
    }

    /// C# `BusyNoted`: called with the path right AFTER a busy outcome was recorded in the back-off.
    #[doc(hidden)]
    pub fn set_busy_noted_for_tests(&self, hook: Option<PathHook>) {
        lock(&self.shared.seams).busy_noted = hook;
    }

    /// NEW seam (no C# twin): called with the path on the thread that DISPATCHES a candidate to the worker pool. Ports the
    /// C# "PreloadThread == caller" assertions (see contract, test notes).
    #[doc(hidden)]
    pub fn set_dispatched_for_tests(&self, hook: Option<PathHook>) {
        lock(&self.shared.seams).dispatched_on = hook;
    }

    /// C# `TrackedLifetimeCounts`: (lifetimes, lifetime runs) still tracked for dispose; must stay bounded.
    #[doc(hidden)]
    pub fn tracked_lifetime_counts(&self) -> (usize, usize) {
        let gate = lock(&self.shared.gate);
        (gate.lifetimes.len(), gate.lifetime_runs.len())
    }

    /// C# `DisposeDrainTimeout` setter (default [`super::policy::DEFAULT_DISPOSE_DRAIN_TIMEOUT`]).
    #[doc(hidden)]
    pub fn set_dispose_drain_timeout(&self, timeout: Duration) {
        lock(&self.shared.seams).dispose_drain_timeout = Some(timeout);
    }

    /// C# `LateDisposalForTests`: `None` when dispose drained in time; else completes after the blocked work finished and
    /// the pool was stopped.
    #[doc(hidden)]
    pub fn late_disposal_for_tests(&self) -> Option<RunHandle> {
        lock(&self.shared.seams).late_disposal.clone()
    }
}

impl Drop for PreloadScheduler {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// What a kick decided under the gate.
enum Kick {
    /// Nothing to do (disposed, or a stale resume): a completed handle.
    Completed,
    /// The newest run is live and belongs to the current lifetime: return its handle.
    Join(RunHandle),
    /// Start a new run.
    Start {
        entries: Arc<[CatalogEntry]>,
        run_id: u64,
        lifetime: Lifetime,
        handle: RunHandle,
    },
}

impl Shared {
    /// C# `PreloadAroundCore(center, resumeGeneration)`: see contract semantics 3. `resume_generation` is `Some` only
    /// from the memory re-check.
    fn preload_around_core(
        self: &Arc<Self>,
        center: i32,
        resume_generation: Option<i64>,
    ) -> Result<RunHandle, CallbackError> {
        // D10: 0 workers = no preload at all: no lifetime, no version, no run.
        if self.worker_count == 0 {
            return Ok(RunHandle::completed());
        }
        // No-op when notify_navigation already recorded this index (the normal App path).
        if resume_generation.is_none() {
            self.pace.record(center);
        }
        // RV-I13: read before the snapshot is taken, so a change in between only costs one extra restart.
        let snapshot_version = match &self.snapshot_version {
            Some(version) => version()?,
            None => 0,
        };
        let mut retired: Option<u64> = None;
        let kick = self.kick_locked(center, resume_generation, snapshot_version, &mut retired);
        if let Some(lifetime) = retired {
            self.pool.cancel_waiting(lifetime);
        }
        match kick? {
            Kick::Completed => Ok(RunHandle::completed()),
            Kick::Join(handle) => Ok(handle),
            Kick::Start {
                entries,
                run_id,
                lifetime,
                handle,
            } => {
                let queue = RunQueue::new(entries, self.worker_count);
                self.run(queue, run_id, lifetime, handle.clone());
                Ok(handle)
            }
        }
    }

    /// The gate part of [`Shared::preload_around_core`]. `retired` receives the lifetime an RV-I13 retire cancelled
    /// (its waiting jobs are cancelled by the caller, outside the gate).
    fn kick_locked(
        &self,
        center: i32,
        resume_generation: Option<i64>,
        snapshot_version: i32,
        retired: &mut Option<u64>,
    ) -> Result<Kick, CallbackError> {
        let mut gate = lock(&self.gate);
        if gate.disposed {
            return Ok(Kick::Completed);
        }
        if let Some(generation) = resume_generation {
            if generation != gate.resume_generation || gate.current.token.is_canceled() {
                return Ok(Kick::Completed);
            }
        }
        Self::cancel_resume_check_locked(&mut gate, resume_generation.is_none());
        if resume_generation.is_some() {
            gate.resumes_for_tests += 1;
        }
        let current_id = gate.current.id;
        if snapshot_version != gate.snapshot_version
            && gate.scheduler_run.as_ref().is_some_and(|run| !run.is_complete())
            && gate.scheduler_lifetime_id == Some(current_id)
            && !gate.current.token.is_canceled()
        {
            // RV-I13: the entries changed without a cancel (e.g. an undo re-inserted a file): the running loop iterates
            // a stale snapshot whose indices now name other files. Retire it exactly like cancel() does.
            gate.current.token.cancel();
            self.events.wake();
            *retired = Some(current_id);
        }
        if gate.current.token.is_canceled() {
            Self::prune_finished_lifetimes(&mut gate);
            let lifetime = Lifetime {
                id: gate.next_lifetime_id,
                token: CancelToken::new(),
            };
            gate.next_lifetime_id += 1;
            gate.current = lifetime.clone();
            gate.lifetimes.push(lifetime);
        }
        let lifetime = gate.current.clone();
        self.preload_center.store(center, Ordering::SeqCst);
        self.priority_version.fetch_add(1, Ordering::SeqCst);
        if let Some(run) = &gate.scheduler_run {
            if !run.is_complete() && gate.scheduler_lifetime_id == Some(lifetime.id) && !gate.run_exiting {
                let run = run.clone();
                drop(gate);
                self.events.wake();
                return Ok(Kick::Join(run));
            }
        }
        gate.scheduler_lifetime_id = Some(lifetime.id);
        gate.lifetime_runs.retain(|(_, run)| !run.is_complete());
        gate.run_id += 1;
        let run_id = gate.run_id;
        gate.run_exiting = false;
        gate.snapshot_version = snapshot_version;
        // Under the gate as in C# (a joining kick never pays for a snapshot copy); the callback must not call back.
        let entries = (self.snapshot_entries)()?;
        let handle = RunHandle::pending();
        gate.scheduler_run = Some(handle.clone());
        gate.lifetime_runs.push((lifetime.id, handle.clone()));
        Ok(Kick::Start {
            entries,
            run_id,
            lifetime,
            handle,
        })
    }

    /// C# `RunPreloadSchedulerAsync`: the run loop, see contract semantics 4-5. Starts on the caller's thread and moves
    /// itself to a run thread at the first yield point.
    fn run(self: &Arc<Self>, queue: RunQueue, run_id: u64, lifetime: Lifetime, handle: RunHandle) {
        self.events.register(run_id);
        let mut run = RunLoop {
            shared: Arc::clone(self),
            queue,
            run_id,
            lifetime,
            handle,
            running: HashMap::new(),
            next_worker_id: 0,
            phase: Phase::Top,
            seen_wake: 0,
            limit: 0,
            finished: false,
        };
        if let Drive::Done = run.drive(true) {
            return;
        }
        let spawned = thread::Builder::new().name("pr-preload-run".to_owned()).spawn(move || {
            let mut run = run;
            run.drive(false);
        });
        if let Err(error) = spawned {
            // The closure (and the run) was dropped: its drop guard drained and completed it on this thread.
            self.log.error(MSG_SCHEDULER_FAILED, Some(&error));
        }
    }

    /// C# `PreloadOneAsync` checks done the moment the item holds its slot: already cached (another task decoded it
    /// while this one waited) => `Cached`; no longer wanted => `Superseded`; `None` = go on with the decode.
    fn short_circuit(&self, candidate: &Candidate, slot: i32, enqueued_at: i64) -> Option<PreloadOutcome> {
        let perf_log = PhotoReviewPerf::log();
        let perf_on = perf_log.is_enabled();
        let started = Instant::now();
        let outcome = if self.target.try_get_cached_preview(&candidate.key) {
            Some((PreloadOutcome::Cached, "skipped"))
        } else {
            let center = self.preload_center.load(Ordering::SeqCst);
            let shape = self.current_shape();
            if policy::is_still_wanted(candidate.index, center, shape) {
                None
            } else {
                Some((PreloadOutcome::Superseded, "superseded"))
            }
        };
        let (outcome, kind) = outcome?;
        if perf_on {
            let queue_wait_ms = perf::ms(enqueued_at);
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            perf_log.preload_item(slot, &perf::path_id(&candidate.path), queue_wait_ms, kind, elapsed);
        }
        Some(outcome)
    }

    /// C# `PreloadOneAsync` body after the checks (prefetch, decode, fresh key, warmed keys, size sample, error arms).
    /// Runs on a pool thread.
    fn preload_one(&self, candidate: &Candidate, slot: i32, enqueued_at: i64, cancel: &CancelToken) -> WorkerEnd {
        let perf_log = PhotoReviewPerf::log();
        let perf_on = perf_log.is_enabled();
        let started = Instant::now();
        let path = candidate.path.as_str();
        let perf_item = |kind: &str| {
            if perf_on {
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                perf_log.preload_item(slot, &perf::path_id(path), perf::ms(enqueued_at), kind, elapsed);
            }
        };
        let attempt = panic::catch_unwind(AssertUnwindSafe(|| self.decode_item(candidate, cancel, perf_on)))
            .unwrap_or_else(|payload| Err(ItemError::Panic(PanicError(panic_message(payload.as_ref())))));
        match attempt {
            Ok((hit, source_read)) => {
                perf_item(if source_read {
                    "decoded"
                } else if hit {
                    "hit"
                } else {
                    "miss"
                });
                WorkerEnd::Done(if hit {
                    PreloadOutcome::Cached
                } else {
                    PreloadOutcome::Failed
                })
            }
            Err(ItemError::Decode(error)) => self.decode_failed(candidate, error, cancel, perf_item),
            Err(ItemError::Key(error)) => {
                self.log.error(&policy::format_preload_failed(path), Some(&error));
                WorkerEnd::Done(PreloadOutcome::Failed)
            }
            Err(ItemError::Panic(error)) => {
                self.log.error(&policy::format_preload_failed(path), Some(&error));
                WorkerEnd::Done(PreloadOutcome::Failed)
            }
        }
    }

    /// The decode of one item: `Ok((hit, source_read))`.
    fn decode_item(
        &self,
        candidate: &Candidate,
        cancel: &CancelToken,
        perf_on: bool,
    ) -> Result<(bool, bool), ItemError> {
        let path = candidate.path.as_str();
        // A preview already in the disk cache decodes from there: prefetching the source would be a read nobody uses.
        if !is_raw_path(path) {
            if let Some(prefetch) = &self.prefetch_source_bytes {
                if !self.target.has_disk_cached_preview(&candidate.key) {
                    prefetch(path, cancel).map_err(ItemError::Decode)?;
                }
            }
        }
        // snapshot() copies the per-path open table: only pay for it when tracing.
        let reads_before = if perf_on {
            self.metrics.snapshot().source_reads
        } else {
            0
        };
        self.target.preload(path, cancel).map_err(ItemError::Decode)?;
        // INV-1: the identity the decode actually stored can differ from the pre-decode key (the source changed).
        let fresh = self
            .target
            .get_current_cache_key_for_path(path)
            .map_err(ItemError::Key)?;
        let hit = self.target.try_get_cached_preview(&fresh);
        if hit {
            let mut warmed = lock(&self.preloaded_keys);
            // RV-I15: a new box makes every key of the old box unmatchable: drop them.
            if fresh.target_box() != warmed.decode_box {
                warmed.keys.clear();
                warmed.decode_box = fresh.target_box();
            }
            warmed.keys.insert(fresh.clone());
        }
        if hit {
            if let Some(bytes) = self.target.cached_preview_bytes(&fresh) {
                self.sizes.record(fresh.target_box(), bytes);
            }
        }
        let source_read = perf_on && self.metrics.snapshot().source_reads > reads_before;
        if hit {
            self.busy_backoff.record_success(path);
        }
        Ok((hit, source_read))
    }

    /// The decode error arms, one per C# catch, in C# order.
    fn decode_failed(
        &self,
        candidate: &Candidate,
        error: DecodeError,
        cancel: &CancelToken,
        perf_item: impl Fn(&str),
    ) -> WorkerEnd {
        let path = candidate.path.as_str();
        match error.kind() {
            DecodeError::Busy { .. } => {
                // Not a failure: the decoder's bounded queue was full. The first busy of a path is worth one Info line;
                // repeats (C# Debug.WriteLine, debugger only) are not logged.
                if self.busy_backoff.note_busy(path, &candidate.key) == 1 {
                    self.log.info(&policy::format_deferred_busy(path));
                }
                let hook = lock(&self.seams).busy_noted.clone();
                if let Some(hook) = hook {
                    if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| hook(path))) {
                        return WorkerEnd::Faulted(panic_message(payload.as_ref()));
                    }
                }
                perf_item("busy");
                WorkerEnd::Done(PreloadOutcome::Busy)
            }
            kind if kind.is_io_exception() => {
                self.log.error(&policy::format_preload_failed(path), Some(&error));
                WorkerEnd::Done(PreloadOutcome::Failed)
            }
            DecodeError::Canceled if !cancel.is_canceled() => {
                // RV-I14: cancelled by someone else's token: only this item failed (not retried this lifetime).
                self.log.warn(&policy::format_foreign_cancel(path, &error.to_string()));
                WorkerEnd::Done(PreloadOutcome::Failed)
            }
            DecodeError::Canceled => WorkerEnd::Canceled,
            // Q-FMT-WEBP-HEIC: the router already logged the install guidance once for the whole format.
            DecodeError::MissingCodec { .. } => WorkerEnd::Done(PreloadOutcome::Failed),
            _ => {
                // A corrupt/unsupported file must skip only itself.
                self.log.error(&policy::format_preload_failed(path), Some(&error));
                WorkerEnd::Done(PreloadOutcome::Failed)
            }
        }
    }

    /// C# `TryExitRun(runId, seenVersion, paused, ct)` (RV-I12), under the gate.
    fn try_exit_run(self: &Arc<Self>, run_id: u64, seen_version: i64, paused: bool, cancel: &CancelToken) -> bool {
        let mut gate = lock(&self.gate);
        if !paused && !cancel.is_canceled() && self.priority_version.load(Ordering::SeqCst) != seen_version {
            return false;
        }
        if run_id == gate.run_id {
            gate.run_exiting = true;
            // The newest run stops for memory headroom: arm a low-frequency re-check.
            if paused && !cancel.is_canceled() && !gate.disposed {
                self.arm_resume_check_locked(&mut gate);
            }
        }
        true
    }

    /// C# `MarkRunExiting(runId)`.
    fn mark_run_exiting(&self, run_id: u64) {
        let mut gate = lock(&self.gate);
        if run_id == gate.run_id {
            gate.run_exiting = true;
        }
    }

    /// C# `PruneFinishedLifetimes()`; caller holds the gate.
    fn prune_finished_lifetimes(gate: &mut GateState) {
        gate.lifetime_runs.retain(|(_, run)| !run.is_complete());
        let current = gate.current.id;
        let runs = &gate.lifetime_runs;
        gate.lifetimes
            .retain(|lifetime| lifetime.id == current || runs.iter().any(|(id, _)| *id == lifetime.id));
    }

    /// C# `ArmResumeCheckLocked()`; caller holds the gate. Only creates a timer (no probe/target/log call under the gate).
    fn arm_resume_check_locked(self: &Arc<Self>, gate: &mut GateState) {
        if let Some(timer) = gate.resume_timer.take() {
            timer.dispose();
        }
        if gate.resume_checks >= MAX_RESUME_CHECKS {
            return; // budget spent: wait for the next navigation
        }
        let delay = policy::resume_check_delay(gate.resume_checks);
        gate.resume_checks += 1;
        gate.resume_generation = gate.resume_generation.wrapping_add(1);
        let generation = gate.resume_generation;
        let weak = Arc::downgrade(self);
        let callback = Arc::new(move || {
            if let Some(shared) = weak.upgrade() {
                shared.on_resume_check(generation);
            }
        });
        gate.resume_timer = Some(self.timers.create_timer(delay, callback));
    }

    /// C# `CancelResumeCheckLocked(resetChecks)`; caller holds the gate.
    fn cancel_resume_check_locked(gate: &mut GateState, reset_checks: bool) {
        gate.resume_generation = gate.resume_generation.wrapping_add(1);
        if let Some(timer) = gate.resume_timer.take() {
            timer.dispose();
        }
        if reset_checks {
            gate.resume_checks = 0;
        }
    }

    /// C# `OnResumeCheck(generation)`: the timer callback (never panics out: everything is caught and logged).
    fn on_resume_check(self: &Arc<Self>, generation: i64) {
        let result = catch(|| {
            {
                let mut gate = lock(&self.gate);
                if gate.disposed || generation != gate.resume_generation {
                    return Ok(()); // superseded by a kick/navigation/cancel/dispose
                }
                if let Some(timer) = gate.resume_timer.take() {
                    timer.dispose();
                }
            }
            match policy::evaluate_resume(&self.options, &*self.target, &*self.memory_probe) {
                ResumeVerdict::Resume => {
                    let center = self.preload_center.load(Ordering::SeqCst);
                    if self.log.enabled() {
                        self.log.info(&policy::format_resumed_log(center));
                    }
                    let run = self.preload_around_core(center, Some(generation))?;
                    lock(&self.gate).resumed_run_for_tests = Some(run);
                }
                ResumeVerdict::CacheFull => {
                    if self.log.enabled() {
                        self.log
                            .info(&policy::format_stays_paused_log(self.target.cache_bytes()));
                    }
                }
                ResumeVerdict::NotYet => {
                    let mut gate = lock(&self.gate);
                    if !gate.disposed && generation == gate.resume_generation {
                        self.arm_resume_check_locked(&mut gate);
                    }
                }
            }
            Ok(())
        });
        if let Err(error) = result {
            self.log.error(MSG_RECHECK_FAILED, Some(error.as_ref()));
        }
    }

    /// C# `CurrentShape()`: `(pace.direction(), pace.get_lead(metrics.decode_milliseconds_ewma()))`.
    fn current_shape(&self) -> Shape {
        Shape {
            direction: self.pace.direction(),
            lead: self.pace.get_lead(self.metrics.decode_milliseconds_ewma()),
        }
    }

    /// The queue's borrowed collaborators.
    fn queue_context(&self) -> QueueContext<'_> {
        QueueContext {
            target: &*self.target,
            memory_probe: &*self.memory_probe,
            sizes: &self.sizes,
            busy_backoff: &self.busy_backoff,
            log: &*self.log,
            options: &self.options,
            clock_ms: &*self.clock_ms,
            perf_enabled: PhotoReviewPerf::log().is_enabled(),
        }
    }
}

/// Why one item's decode did not complete (one variant per C# catch family).
enum ItemError {
    Decode(DecodeError),
    Key(KeyError),
    Panic(PanicError),
}

/// Where the run loop resumes (the loop can stop at a hand-off point on the caller's thread and continue on the run
/// thread exactly there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Top of a pass (`while (!ct.IsCancellationRequested)`).
    Top,
    /// The inner candidate loop; `check_cancel` = resume after a batch yield (C# `Task.Delay(1, ct)`).
    Inner { check_cancel: bool },
    /// After the inner loop: one more pass, exit, or wait.
    AfterInner,
    /// Waiting for a worker or a wake.
    Wait,
    /// Waiting for every running worker, then complete.
    Drain,
}

/// Result of [`RunLoop::drive`].
enum Drive {
    /// The run must continue off the caller's thread.
    HandOff,
    /// The run completed.
    Done,
}

/// How the inner candidate loop ended.
enum InnerEnd {
    Break,
    Canceled,
    HandOff { check_cancel: bool },
}

/// One run (C# the locals of `RunPreloadSchedulerAsync`).
struct RunLoop {
    shared: Arc<Shared>,
    queue: RunQueue,
    run_id: u64,
    lifetime: Lifetime,
    handle: RunHandle,
    /// Worker id => path (C# `running`).
    running: HashMap<u64, String>,
    next_worker_id: u64,
    phase: Phase,
    seen_wake: u64,
    limit: i32,
    finished: bool,
}

impl RunLoop {
    /// Runs the loop until it completes or (on the caller's thread) reaches a hand-off point.
    fn drive(&mut self, on_caller: bool) -> Drive {
        loop {
            match self.phase {
                Phase::Top => {
                    if self.lifetime.token.is_canceled() {
                        self.phase = Phase::Drain;
                        continue;
                    }
                    if self.absorb_reports() {
                        continue;
                    }
                    match catch(|| self.top()) {
                        Ok(()) => self.phase = Phase::Inner { check_cancel: false },
                        Err(error) => self.fail(error.as_ref()),
                    }
                }
                Phase::Inner { check_cancel } => {
                    if check_cancel && self.lifetime.token.is_canceled() {
                        self.phase = Phase::Drain;
                        continue;
                    }
                    match catch(|| self.inner(on_caller)) {
                        Ok(InnerEnd::Break) => self.phase = Phase::AfterInner,
                        Ok(InnerEnd::Canceled) => self.phase = Phase::Drain,
                        Ok(InnerEnd::HandOff { check_cancel }) => {
                            self.phase = Phase::Inner { check_cancel };
                            return Drive::HandOff;
                        }
                        Err(error) => self.fail(error.as_ref()),
                    }
                }
                Phase::AfterInner => {
                    if let Err(error) = catch(|| self.after_inner()) {
                        self.fail(error.as_ref());
                    }
                }
                Phase::Wait => {
                    if on_caller {
                        return Drive::HandOff;
                    }
                    match self.shared.events.wait(self.run_id, self.seen_wake) {
                        Wakeup::Woken => self.phase = Phase::Top, // woken by a navigation: re-prioritize
                        Wakeup::Finished(finished) => self.apply(finished),
                    }
                }
                Phase::Drain => {
                    if on_caller && !self.running.is_empty() {
                        return Drive::HandOff;
                    }
                    self.drain();
                    self.finish();
                    return Drive::Done;
                }
            }
        }
    }

    /// Loop head: wake sequence, version, shape, centre (in this order), order refresh, worker limit.
    fn top(&mut self) -> Result<(), CallbackError> {
        let shared = &*self.shared;
        // Captured before the version is read: a navigation after this point moves the sequence, so the wait below can
        // never miss it.
        self.seen_wake = shared.events.wake_seq();
        let version = shared.priority_version.load(Ordering::SeqCst);
        let shape = shared.current_shape();
        let center = shared.preload_center.load(Ordering::SeqCst);
        let context = shared.queue_context();
        self.queue
            .refresh(&context, PassInputs { version, center, shape })
            .map_err(|error| Box::new(error) as CallbackError)?;
        self.limit = policy::worker_limit(
            shared.worker_count,
            shared.target.active_viewer_decodes(),
            shared.metrics.decode_milliseconds_ewma(),
            shared.viewer_busy_worker_limit,
        );
        Ok(())
    }

    /// The inner candidate loop (C# `while (running.Count < limit && order.MoveNext())`).
    fn inner(&mut self, on_caller: bool) -> Result<InnerEnd, CallbackError> {
        let shared = Arc::clone(&self.shared);
        while i32::try_from(self.running.len()).unwrap_or(i32::MAX) < self.limit {
            let context = shared.queue_context();
            let step = self
                .queue
                .next(&context, on_caller, &self.lifetime.token)
                .map_err(|error| Box::new(error) as CallbackError)?;
            match step {
                QueueStep::Exhausted | QueueStep::FillLimitStop | QueueStep::Paused => return Ok(InnerEnd::Break),
                QueueStep::Canceled => return Ok(InnerEnd::Canceled),
                QueueStep::LeaveCallerThread => {
                    if on_caller {
                        return Ok(InnerEnd::HandOff { check_cancel: false });
                    }
                }
                QueueStep::Skipped => {}
                QueueStep::Dispatch(candidate) => {
                    let batch_yield = candidate.batch_yield;
                    self.dispatch(candidate)?;
                    if batch_yield {
                        if shared.log.enabled() {
                            let memory = shared.memory_probe.get_snapshot();
                            shared.log.info(&policy::format_progress_log(
                                self.queue.queued_count(),
                                self.running.len(),
                                shared.target.cache_count(),
                                shared.target.cache_bytes(),
                                memory,
                            ));
                        }
                        // C# `await Task.Delay(1, ct)`: off the caller's thread from here; a cancellation ends the run.
                        if on_caller {
                            return Ok(InnerEnd::HandOff { check_cancel: true });
                        }
                        if self.lifetime.token.is_canceled() {
                            return Ok(InnerEnd::Canceled);
                        }
                        thread::yield_now();
                    }
                }
            }
        }
        Ok(InnerEnd::Break)
    }

    /// Hands one candidate to the worker pool (contract W4/W5).
    fn dispatch(&mut self, candidate: Candidate) -> Result<(), CallbackError> {
        let shared = Arc::clone(&self.shared);
        let hook = lock(&shared.seams).dispatched_on.clone();
        if let Some(hook) = hook {
            hook(&candidate.path);
        }
        let enqueued_at = if PhotoReviewPerf::log().is_enabled() {
            perf::timestamp()
        } else {
            0
        };
        let worker_id = self.next_worker_id;
        self.next_worker_id += 1;
        match shared.pool.try_reserve() {
            Some(reservation) => {
                // A free slot: the two short-circuit checks run inline (C# a synchronously completed item).
                let slot = reservation.slot;
                match catch(|| Ok(shared.short_circuit(&candidate, slot, enqueued_at))) {
                    Ok(Some(outcome)) => {
                        shared.pool.release(reservation);
                        self.queue.on_finished(&candidate.path, outcome);
                    }
                    Ok(None) => {
                        self.running.insert(worker_id, candidate.path.clone());
                        let job = self.job(worker_id, candidate, enqueued_at, false);
                        shared.pool.start_reserved(reservation, job);
                    }
                    Err(error) => {
                        shared.pool.release(reservation);
                        return Err(error);
                    }
                }
            }
            None => {
                self.running.insert(worker_id, candidate.path.clone());
                let job = self.job(worker_id, candidate, enqueued_at, true);
                shared.pool.enqueue(job);
            }
        }
        Ok(())
    }

    /// The pool job of one item. `checks` = run the short-circuit checks first (the item waited for its slot).
    fn job(&self, worker_id: u64, candidate: Candidate, enqueued_at: i64, checks: bool) -> Job {
        let shared = Arc::clone(&self.shared);
        let token = self.lifetime.token.clone();
        let path = candidate.path.clone();
        let run = Box::new(move |slot: i32| {
            // D04 perf (W8): preload work is not tied to a navigation.
            let perf_on = PhotoReviewPerf::log().is_enabled();
            let previous = PhotoReviewPerf::nav_context();
            if perf_on {
                PhotoReviewPerf::set_nav_context(-1);
            }
            let end = if checks {
                match panic::catch_unwind(AssertUnwindSafe(|| shared.short_circuit(&candidate, slot, enqueued_at))) {
                    Ok(Some(outcome)) => WorkerEnd::Done(outcome),
                    Ok(None) => shared.preload_one(&candidate, slot, enqueued_at, &token),
                    Err(payload) => WorkerEnd::Faulted(panic_message(payload.as_ref())),
                }
            } else {
                shared.preload_one(&candidate, slot, enqueued_at, &token)
            };
            if perf_on {
                PhotoReviewPerf::set_nav_context(previous);
            }
            end
        });
        let reporter = Arc::clone(&self.shared);
        let run_id = self.run_id;
        let report = Box::new(move |end: WorkerEnd| {
            reporter.events.report(run_id, Finished { worker_id, path, end });
        });
        Job::new(self.lifetime.id, run, report)
    }

    /// After the inner loop (C# lines 642-653): one more pass for a fresh calibration, exit, or wait.
    fn after_inner(&mut self) -> Result<(), CallbackError> {
        let shared = Arc::clone(&self.shared);
        let paused = self.queue.is_paused();
        // Q-R17: the window may run out right as its decodes finish calibrating the estimate: one more pass.
        if !paused && self.running.is_empty() && self.queue.calibration_changed(&shared.sizes) {
            self.phase = Phase::Top;
            return Ok(());
        }
        if paused || self.running.is_empty() {
            // RV-I12: a navigation that landed after this pass read the version takes another pass instead of exiting.
            if !shared.try_exit_run(self.run_id, self.queue.seen_version(), paused, &self.lifetime.token) {
                self.phase = Phase::Top;
                return Ok(());
            }
            let hook = lock(&shared.seams).before_scheduler_exit.clone();
            self.phase = Phase::Drain;
            if let Some(hook) = hook {
                hook();
            }
            return Ok(());
        }
        self.phase = Phase::Wait;
        Ok(())
    }

    /// Applies every report that already arrived before the pass reads its inputs, so a worker that finished while the
    /// run was busy leaves the queued set before the next order pass examines its path (C# items that complete
    /// synchronously never stay queued). `true` when a report ended the loop (cancelled or faulted worker).
    fn absorb_reports(&mut self) -> bool {
        while let Some(finished) = self.shared.events.try_take(self.run_id) {
            self.apply(finished);
            if self.phase != Phase::Top {
                return true;
            }
        }
        false
    }

    /// Applies a worker's report (contract semantics 7).
    fn apply(&mut self, finished: Finished) {
        self.running.remove(&finished.worker_id);
        match finished.end {
            WorkerEnd::Done(outcome) => {
                self.queue.on_finished(&finished.path, outcome);
                self.phase = Phase::Top;
            }
            WorkerEnd::Canceled => self.phase = Phase::Drain,
            WorkerEnd::Faulted(message) => self.fail(&PanicError(message)),
        }
    }

    /// C# `catch (Exception ex)`: the next navigation starts a new run; log; drain.
    fn fail(&mut self, error: &(dyn Error + 'static)) {
        self.shared.mark_run_exiting(self.run_id);
        self.shared.log.error(MSG_SCHEDULER_FAILED, Some(error));
        self.phase = Phase::Drain;
    }

    /// C# `DrainWorkersAsync(running.Keys)`: wait for every started worker (wakes are ignored).
    fn drain(&mut self) {
        while !self.running.is_empty() {
            match self.shared.events.wait_finished(self.run_id) {
                Some(finished) => {
                    self.running.remove(&finished.worker_id);
                }
                None => break,
            }
        }
        while self.shared.events.try_take(self.run_id).is_some() {}
    }

    /// The run is over: forget its report queue and complete its handle (after the drain).
    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.shared.events.remove_run(self.run_id);
        self.handle.complete();
    }
}

impl Drop for RunLoop {
    fn drop(&mut self) {
        // A run that never reached its end (its thread could not be started): never leave the handle pending.
        if !self.finished {
            self.shared.mark_run_exiting(self.run_id);
            self.drain();
            self.finish();
        }
    }
}
