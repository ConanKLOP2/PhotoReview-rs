//! Worker slots and run events of the preload scheduler: the Rust twin of C# `SemaphoreSlim _preloadSlots`, the
//! `Task<PreloadOutcome>` set a run waits on (`Task.WhenAny(running.Keys.Append(wake))`) and the `_wake`
//! `TaskCompletionSource` (C# baseline d92d6aae, `Preload/PreloadScheduler.cs`). Contract: `contracts/W4-15b.md`
//! (draft `contracts/drafts-w4/W4-15b/contract.md`).
//!
//! Target path: `crates/pr-imaging/src/preload/worker.rs` (private module: `mod worker;` in `preload/mod.rs`).
//!
//! Rules W1..W8 of the contract hold here: one slot per running item shared by every lifetime (W1), FIFO waiters (W2),
//! waiting items of a cancelled lifetime end at once without a slot (W3), and every item reports exactly once (W7: a
//! job that is dropped without running reports [`WorkerEnd::Canceled`]). A pool thread gives its slot back BEFORE the
//! item reports, as the C# `finally { _preloadSlots.Release(); }` runs before the item's task completes.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use pr_core::sync::CancelToken;

use super::policy::PreloadOutcome;

/// Locks `mutex`, recovering a poisoned lock (no code runs under these guards that can leave the state half-written).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The text of a caught panic payload (`&str` / `String` payloads; anything else gets a generic text).
pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a preload component panicked".to_owned()
    }
}

/// One cancellation lifetime (C# one `CancellationTokenSource` in `_preloadLifetimes`). `id` gives the identity C#
/// gets from `ReferenceEquals` on the CTS.
#[derive(Debug, Clone)]
pub(crate) struct Lifetime {
    pub(crate) id: u64,
    pub(crate) token: CancelToken,
}

/// How one worker ended (C# the state of its `Task<PreloadOutcome>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkerEnd {
    /// Ran to completion with an outcome (C# `RanToCompletion`).
    Done(PreloadOutcome),
    /// The lifetime was cancelled while waiting for a slot, or the target/prefetch reported `Canceled` while the
    /// lifetime token IS cancelled (C# `Canceled` task): the run drains and ends.
    Canceled,
    /// An unexpected failure outside the item's own catch arms, e.g. a panic in the cache re-check before the decode
    /// (C# `Faulted` task): the run ends as "Preload scheduler failed". The text is the panic message.
    Faulted(String),
}

/// A worker's report to its run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finished {
    pub(crate) worker_id: u64,
    pub(crate) path: String,
    pub(crate) end: WorkerEnd,
}

/// Scheduler-wide events (one mutex + one condition variable): finished workers per run, and the wake sequence that
/// every navigation / cancel / dispose bumps (C# `WakeScheduler`: completes the shared `_wake` and replaces it).
#[derive(Default)]
pub(crate) struct RunEvents {
    state: Mutex<EventState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct EventState {
    wake_seq: u64,
    finished: HashMap<u64, VecDeque<Finished>>,
}

impl fmt::Debug for RunEvents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunEvents").finish_non_exhaustive()
    }
}

/// What woke a waiting run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wakeup {
    /// A worker of this run finished (preferred over a wake that is also pending: C# `WhenAny` lists the workers first).
    Finished(Finished),
    /// The wake sequence moved past the value captured at the start of the pass.
    Woken,
}

impl RunEvents {
    /// Opens the report queue of a new run (reports of an unknown run are dropped, so a run that ended never leaks
    /// a late report).
    pub(crate) fn register(&self, run_id: u64) {
        lock(&self.state).finished.entry(run_id).or_default();
    }

    /// C# `Volatile.Read(ref _wake).Task`, captured at the top of each pass BEFORE the version is read.
    pub(crate) fn wake_seq(&self) -> u64 {
        lock(&self.state).wake_seq
    }

    /// C# `WakeScheduler()`: bump the sequence and wake every waiting run.
    pub(crate) fn wake(&self) {
        let mut state = lock(&self.state);
        state.wake_seq = state.wake_seq.wrapping_add(1);
        drop(state);
        self.changed.notify_all();
    }

    /// A worker of `run_id` finished: queue the report and wake waiters.
    pub(crate) fn report(&self, run_id: u64, finished: Finished) {
        let mut state = lock(&self.state);
        if let Some(queue) = state.finished.get_mut(&run_id) {
            queue.push_back(finished);
        }
        drop(state);
        self.changed.notify_all();
    }

    /// Blocks the run thread until a worker of `run_id` reported or the wake sequence differs from `seen_wake`.
    pub(crate) fn wait(&self, run_id: u64, seen_wake: u64) -> Wakeup {
        let mut state = lock(&self.state);
        loop {
            if let Some(finished) = state.finished.get_mut(&run_id).and_then(VecDeque::pop_front) {
                return Wakeup::Finished(finished);
            }
            if state.wake_seq != seen_wake || !state.finished.contains_key(&run_id) {
                return Wakeup::Woken;
            }
            state = self.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Blocks until a worker of `run_id` reported, ignoring wakes (the drain). `None` when the run is not registered.
    pub(crate) fn wait_finished(&self, run_id: u64) -> Option<Finished> {
        let mut state = lock(&self.state);
        loop {
            if let Some(finished) = state.finished.get_mut(&run_id)?.pop_front() {
                return Some(finished);
            }
            state = self.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Takes one report of `run_id` without blocking.
    pub(crate) fn try_take(&self, run_id: u64) -> Option<Finished> {
        lock(&self.state)
            .finished
            .get_mut(&run_id)
            .and_then(VecDeque::pop_front)
    }

    /// Forget a finished run's queue.
    pub(crate) fn remove_run(&self, run_id: u64) {
        lock(&self.state).finished.remove(&run_id);
        self.changed.notify_all();
    }
}

/// The body of a job: runs the item with its slot number and returns how it ended.
pub(crate) type JobRun = Box<dyn FnOnce(i32) -> WorkerEnd + Send>;
/// Delivers a job's end to its run.
pub(crate) type JobReport = Box<dyn FnOnce(WorkerEnd) + Send>;

/// A unit of work for a pool thread. `run(slot)` executes the item with slot number `slot` (C# `_workerCount - 1 -
/// _preloadSlots.CurrentCount`, perf only); the pool then releases the slot and calls `report` with the end. A job that
/// never runs (its lifetime was cancelled while it waited, or the pool stopped) reports `WorkerEnd::Canceled`, also
/// when it is simply dropped (W7).
pub(crate) struct Job {
    pub(crate) lifetime_id: u64,
    run: Option<JobRun>,
    report: Option<JobReport>,
}

impl fmt::Debug for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field("lifetime_id", &self.lifetime_id)
            .finish_non_exhaustive()
    }
}

impl Job {
    pub(crate) fn new(lifetime_id: u64, run: JobRun, report: JobReport) -> Self {
        Self {
            lifetime_id,
            run: Some(run),
            report: Some(report),
        }
    }

    /// Runs the body (a panic escaping it becomes `Faulted`) and hands back the reporter, so the caller can release
    /// the slot before the item reports.
    fn execute(mut self, slot: i32) -> (WorkerEnd, Option<JobReport>) {
        let end = match self.run.take() {
            Some(run) => panic::catch_unwind(AssertUnwindSafe(move || run(slot)))
                .unwrap_or_else(|payload| WorkerEnd::Faulted(panic_message(payload.as_ref()))),
            None => WorkerEnd::Canceled,
        };
        (end, self.report.take())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(report) = self.report.take() {
            report(WorkerEnd::Canceled);
        }
    }
}

/// A slot taken without waiting (C# `WaitAsync` completing synchronously). It must be consumed exactly once: by
/// [`WorkerPool::start_reserved`] (a pool thread runs the job) or by [`WorkerPool::release`] (the inline short-circuit
/// checks decided the outcome, C# `_preloadSlots.Release()` in the `finally`).
#[derive(Debug)]
pub(crate) struct SlotReservation {
    pub(crate) slot: i32,
}

/// `max(1, worker_count)` slots shared by every lifetime of one scheduler (C# `_preloadSlots`), served by the same
/// number of named pool threads (`pr-preload-<n>`) started lazily. Waiting jobs are served FIFO.
pub(crate) struct WorkerPool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    slots: i32,
    state: Mutex<PoolState>,
    available: Condvar,
}

#[derive(Default)]
struct PoolState {
    free_slots: Vec<i32>,
    waiting: VecDeque<Job>,
    ready: VecDeque<(i32, Job)>,
    threads: Vec<JoinHandle<()>>,
    idle: usize,
    shutting_down: bool,
}

impl fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerPool")
            .field("slots", &self.inner.slots)
            .finish_non_exhaustive()
    }
}

impl WorkerPool {
    /// `slots = max(1, worker_count)` (C# `new SemaphoreSlim(Math.Max(1, n), Math.Max(1, n))`); no thread yet.
    pub(crate) fn new(worker_count: i32) -> Self {
        let slots = worker_count.max(1);
        let state = PoolState {
            free_slots: (0..slots).rev().collect(),
            ..PoolState::default()
        };
        Self {
            inner: Arc::new(PoolInner {
                slots,
                state: Mutex::new(state),
                available: Condvar::new(),
            }),
        }
    }

    /// A free slot when one is free AND no job is waiting (FIFO fairness); `None` otherwise. Never blocks.
    pub(crate) fn try_reserve(&self) -> Option<SlotReservation> {
        let mut state = lock(&self.inner.state);
        if !state.waiting.is_empty() || state.shutting_down {
            return None;
        }
        state.free_slots.pop().map(|slot| SlotReservation { slot })
    }

    /// Hands `job` with its reserved slot to a pool thread (starting threads lazily up to `slots`).
    pub(crate) fn start_reserved(&self, reservation: SlotReservation, job: Job) {
        let mut state = lock(&self.inner.state);
        state.ready.push_back((reservation.slot, job));
        let inline = self.inner.ensure_thread(&mut state);
        drop(state);
        self.inner.available.notify_all();
        if inline {
            self.inner.run_ready_inline();
        }
    }

    /// Gives a reserved slot back without running anything (after the inline short-circuit checks, contract W4) and hands
    /// it to the first waiting job, if any.
    pub(crate) fn release(&self, reservation: SlotReservation) {
        let mut state = lock(&self.inner.state);
        state.free_slots.push(reservation.slot);
        state.promote_waiting();
        let inline = self.inner.ensure_thread(&mut state);
        drop(state);
        self.inner.available.notify_all();
        if inline {
            self.inner.run_ready_inline();
        }
    }

    /// Queues `job` until a slot frees (FIFO). The pool thread that takes it calls `job.run(slot)`.
    pub(crate) fn enqueue(&self, job: Job) {
        let mut state = lock(&self.inner.state);
        if state.shutting_down {
            drop(state);
            drop(job); // reports Canceled
            return;
        }
        state.waiting.push_back(job);
        state.promote_waiting();
        let inline = self.inner.ensure_thread(&mut state);
        drop(state);
        self.inner.available.notify_all();
        if inline {
            self.inner.run_ready_inline();
        }
    }

    /// C# `WaitAsync(ct)` throwing for a cancelled lifetime: every WAITING job of `lifetime_id` is removed and reports
    /// `Canceled`, on the calling thread, after the pool lock is released.
    pub(crate) fn cancel_waiting(&self, lifetime_id: u64) {
        let mut state = lock(&self.inner.state);
        let mut cancelled = Vec::new();
        let mut kept = VecDeque::with_capacity(state.waiting.len());
        while let Some(job) = state.waiting.pop_front() {
            if job.lifetime_id == lifetime_id {
                cancelled.push(job);
            } else {
                kept.push_back(job);
            }
        }
        state.waiting = kept;
        drop(state);
        drop(cancelled); // each dropped job reports Canceled
    }

    /// Stops the pool threads once the queues are empty; `join` waits for them (only when no job can still be running).
    /// Jobs still waiting for a slot report `Canceled`. Never joins the calling thread itself.
    pub(crate) fn shutdown(&self, join: bool) {
        let mut state = lock(&self.inner.state);
        state.shutting_down = true;
        let waiting: Vec<Job> = state.waiting.drain(..).collect();
        let threads: Vec<JoinHandle<()>> = if join {
            std::mem::take(&mut state.threads)
        } else {
            Vec::new()
        };
        drop(state);
        self.inner.available.notify_all();
        drop(waiting);
        let me = thread::current().id();
        for handle in threads {
            if handle.thread().id() != me {
                // A pool thread only ends by returning (job panics are caught), so the join result carries nothing.
                let _ = handle.join();
            }
        }
    }
}

impl PoolState {
    /// Pairs free slots with waiting jobs, FIFO.
    fn promote_waiting(&mut self) {
        while !self.waiting.is_empty() {
            let Some(slot) = self.free_slots.pop() else { break };
            if let Some(job) = self.waiting.pop_front() {
                self.ready.push_back((slot, job));
            }
        }
    }
}

impl PoolInner {
    /// Starts one more pool thread when ready jobs outnumber idle threads. Returns `true` when no pool thread exists and
    /// none could be started: the caller then runs the ready jobs itself (never leaves a job without a thread).
    fn ensure_thread(self: &Arc<Self>, state: &mut PoolState) -> bool {
        if state.ready.len() <= state.idle || state.threads.len() >= usize::try_from(self.slots).unwrap_or(1) {
            return false;
        }
        let inner = Arc::clone(self);
        let name = format!("pr-preload-{}", state.threads.len());
        match thread::Builder::new().name(name).spawn(move || inner.worker_loop()) {
            Ok(handle) => {
                state.threads.push(handle);
                false
            }
            Err(_) => state.threads.is_empty(),
        }
    }

    /// Fallback when no pool thread could be started: run every ready job on the calling thread.
    fn run_ready_inline(self: &Arc<Self>) {
        loop {
            let next = lock(&self.state).ready.pop_front();
            let Some((slot, job)) = next else { break };
            self.run_job(slot, job);
        }
    }

    /// Runs one job, gives its slot back (promoting the next waiter), then lets it report.
    fn run_job(&self, slot: i32, job: Job) {
        let (end, report) = job.execute(slot);
        let mut state = lock(&self.state);
        state.free_slots.push(slot);
        state.promote_waiting();
        drop(state);
        self.available.notify_all();
        if let Some(report) = report {
            report(end);
        }
    }

    fn worker_loop(self: Arc<Self>) {
        let mut state = lock(&self.state);
        loop {
            if let Some((slot, job)) = state.ready.pop_front() {
                drop(state);
                self.run_job(slot, job);
                state = lock(&self.state);
            } else if state.shutting_down {
                return;
            } else {
                state.idle += 1;
                state = self.available.wait(state).unwrap_or_else(PoisonError::into_inner);
                state.idle -= 1;
            }
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // The scheduler's shared state going away stops the threads (they hold the inner state only); waiting jobs
        // report Canceled.
        self.shutdown(false);
    }
}
