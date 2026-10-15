//! Worker slots and run events of the preload scheduler: the Rust twin of C# `SemaphoreSlim _preloadSlots`, the
//! `Task<PreloadOutcome>` set a run waits on (`Task.WhenAny(running.Keys.Append(wake))`) and the `_wake`
//! `TaskCompletionSource` (C# baseline d92d6aae, `Preload/PreloadScheduler.cs`). Contract: `contracts/W4-15b.md`
//! (draft `contracts/drafts-w4/W4-15b/contract.md`).
//!
//! Target path: `crates/pr-imaging/src/preload/worker.rs` (private module: `mod worker;` in `preload/mod.rs`).
//!
//! The item types and method set below are the stub's proposal; the porter may reshape PRIVATE items as long as the
//! rules W1..W8 of the contract hold (they are what the C# tests observe).

#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Condvar, Mutex};
use std::thread::JoinHandle;

use pr_core::sync::CancelToken;

use super::policy::PreloadOutcome;

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
    /// C# `Volatile.Read(ref _wake).Task`, captured at the top of each pass BEFORE the version is read.
    pub(crate) fn wake_seq(&self) -> u64 {
        todo!("W4-15b")
    }

    /// C# `WakeScheduler()`: bump the sequence and wake every waiting run.
    pub(crate) fn wake(&self) {
        todo!("W4-15b")
    }

    /// A worker of `run_id` finished: queue the report and wake waiters.
    pub(crate) fn report(&self, run_id: u64, finished: Finished) {
        todo!("W4-15b")
    }

    /// Blocks the run thread until a worker of `run_id` reported or the wake sequence differs from `seen_wake`.
    pub(crate) fn wait(&self, run_id: u64, seen_wake: u64) -> Wakeup {
        todo!("W4-15b: Condvar wait loop, PoisonError::into_inner")
    }

    /// Takes one report of `run_id` without blocking (drain, inline completions).
    pub(crate) fn try_take(&self, run_id: u64) -> Option<Finished> {
        todo!("W4-15b")
    }

    /// Forget a finished run's queue.
    pub(crate) fn remove_run(&self, run_id: u64) {
        todo!("W4-15b")
    }
}

/// A unit of work for a pool thread. `run(slot)` executes the item with slot number `slot` (C# `_workerCount - 1 -
/// _preloadSlots.CurrentCount`, perf only) and reports by itself; `cancel_now()` reports `WorkerEnd::Canceled` without
/// running (a waiting job whose lifetime was cancelled).
pub(crate) struct Job {
    pub(crate) lifetime_id: u64,
    pub(crate) token: CancelToken,
    pub(crate) run: Box<dyn FnOnce(i32) + Send>,
    pub(crate) cancel_now: Box<dyn FnOnce() + Send>,
}

impl fmt::Debug for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field("lifetime_id", &self.lifetime_id)
            .finish_non_exhaustive()
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
    shutting_down: bool,
}

impl fmt::Debug for WorkerPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerPool")
            .field("slots", &self.slots)
            .finish_non_exhaustive()
    }
}

impl WorkerPool {
    /// `slots = max(1, worker_count)` (C# `new SemaphoreSlim(Math.Max(1, n), Math.Max(1, n))`); no thread yet.
    pub(crate) fn new(worker_count: i32) -> Self {
        todo!("W4-15b")
    }

    /// A free slot when one is free AND no job is waiting (FIFO fairness); `None` otherwise. Never blocks.
    pub(crate) fn try_reserve(&self) -> Option<SlotReservation> {
        todo!("W4-15b")
    }

    /// Hands `job` with its reserved slot to a pool thread (starting threads lazily up to `slots`).
    pub(crate) fn start_reserved(&self, reservation: SlotReservation, job: Job) {
        todo!("W4-15b")
    }

    /// Gives a reserved slot back without running anything (after the inline short-circuit checks, contract W4) and hands
    /// it to the first waiting job, if any.
    pub(crate) fn release(&self, reservation: SlotReservation) {
        todo!("W4-15b")
    }

    /// Queues `job` until a slot frees (FIFO). The pool thread that takes it calls `job.run(slot)`.
    pub(crate) fn enqueue(&self, job: Job) {
        todo!("W4-15b")
    }

    /// C# `WaitAsync(ct)` throwing for a cancelled lifetime: every WAITING job of `lifetime_id` is removed and
    /// `cancel_now()` is called for it, on the calling thread, after the pool lock is released.
    pub(crate) fn cancel_waiting(&self, lifetime_id: u64) {
        todo!("W4-15b")
    }

    /// Stops the pool threads once the queues are empty; `join` waits for them (only when no job can still be running).
    pub(crate) fn shutdown(&self, join: bool) {
        todo!("W4-15b")
    }
}
