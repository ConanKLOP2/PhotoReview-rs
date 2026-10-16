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

    /// Takes the oldest report of `run_id` without blocking, but only when it is a `Done` one: a cancelled or faulted
    /// worker stays queued for the wait, which ends the loop.
    pub(crate) fn try_take_done(&self, run_id: u64) -> Option<Finished> {
        let mut state = lock(&self.state);
        let queue = state.finished.get_mut(&run_id)?;
        if matches!(
            queue.front(),
            Some(Finished {
                end: WorkerEnd::Done(_),
                ..
            })
        ) {
            queue.pop_front()
        } else {
            None
        }
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
    /// The lifetime token (C# the `CancellationToken` passed to `WaitAsync`): a job whose token is cancelled never gets
    /// a slot (W3).
    cancel: CancelToken,
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
    pub(crate) fn new(lifetime: &Lifetime, run: JobRun, report: JobReport) -> Self {
        Self {
            lifetime_id: lifetime.id,
            cancel: lifetime.token.clone(),
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
    /// C# `_workerCount - 1 - _preloadSlots.CurrentCount` read right after the slot was taken (perf only).
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
    /// Test seam: no pool thread can be started (the jobs then run on the thread that queued them).
    spawn_disabled: bool,
}

#[derive(Default)]
struct PoolState {
    /// C# `_preloadSlots.CurrentCount`.
    free: i32,
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
        Self::build(worker_count, false)
    }

    fn build(worker_count: i32, spawn_disabled: bool) -> Self {
        let slots = worker_count.max(1);
        let state = PoolState {
            free: slots,
            ..PoolState::default()
        };
        Self {
            inner: Arc::new(PoolInner {
                slots,
                state: Mutex::new(state),
                available: Condvar::new(),
                spawn_disabled,
            }),
        }
    }

    /// A free slot when one is free AND no job is waiting (FIFO fairness); `None` otherwise. Never blocks.
    pub(crate) fn try_reserve(&self) -> Option<SlotReservation> {
        let mut state = lock(&self.inner.state);
        if !state.waiting.is_empty() || state.shutting_down || state.free == 0 {
            return None;
        }
        Some(SlotReservation {
            slot: state.take_slot(self.inner.slots),
        })
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
        let _ = reservation.slot;
        let mut state = lock(&self.inner.state);
        state.free += 1;
        let cancelled = state.promote_waiting(self.inner.slots);
        let inline = self.inner.ensure_thread(&mut state);
        drop(state);
        self.inner.available.notify_all();
        drop(cancelled); // each dropped job reports Canceled
        if inline {
            self.inner.run_ready_inline();
        }
    }

    /// Queues `job` until a slot frees (FIFO). The pool thread that takes it calls `job.run(slot)`. A job whose lifetime
    /// is already cancelled (C# `WaitAsync(ct)` with a cancelled token) or that arrives after `shutdown` never waits: it
    /// reports `Canceled` at once (W3).
    pub(crate) fn enqueue(&self, job: Job) {
        let mut state = lock(&self.inner.state);
        if state.shutting_down || job.cancel.is_canceled() {
            drop(state);
            drop(job); // reports Canceled
            return;
        }
        state.waiting.push_back(job);
        let cancelled = state.promote_waiting(self.inner.slots);
        let inline = self.inner.ensure_thread(&mut state);
        drop(state);
        self.inner.available.notify_all();
        drop(cancelled);
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
    /// Takes one free slot and returns its C# slot number (`_workerCount - 1 - CurrentCount` after the take).
    fn take_slot(&mut self, slots: i32) -> i32 {
        self.free -= 1;
        slots - 1 - self.free
    }

    /// Pairs free slots with waiting jobs, FIFO. A waiting job whose lifetime was cancelled meanwhile never takes a slot
    /// (C# the cancellation callback removes the waiter): it is returned so the caller drops it (reports `Canceled`)
    /// after releasing the pool lock.
    fn promote_waiting(&mut self, slots: i32) -> Vec<Job> {
        let mut cancelled = Vec::new();
        while self.free > 0 {
            let Some(job) = self.waiting.pop_front() else { break };
            if job.cancel.is_canceled() {
                cancelled.push(job);
                continue;
            }
            let slot = self.take_slot(slots);
            self.ready.push_back((slot, job));
        }
        cancelled
    }
}

impl PoolInner {
    /// Starts one more pool thread when ready jobs outnumber idle threads. Returns `true` when no pool thread exists and
    /// none could be started: the caller then runs the ready jobs itself (never leaves a job without a thread).
    fn ensure_thread(self: &Arc<Self>, state: &mut PoolState) -> bool {
        if state.ready.len() <= state.idle || state.threads.len() >= usize::try_from(self.slots).unwrap_or(1) {
            return false;
        }
        if self.spawn_disabled {
            return state.threads.is_empty();
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
        state.free += 1;
        let cancelled = state.promote_waiting(self.slots);
        drop(state);
        self.available.notify_all();
        drop(cancelled);
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::time::{Duration, Instant};

    use super::*;

    const GUARD: Duration = Duration::from_secs(10);

    /// A one-shot latch (Mutex<bool> + Condvar).
    #[derive(Default)]
    struct Latch {
        open: Mutex<bool>,
        changed: Condvar,
    }

    impl Latch {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn open(&self) {
            *lock(&self.open) = true;
            self.changed.notify_all();
        }

        fn wait(&self) -> bool {
            let deadline = Instant::now() + GUARD;
            let mut open = lock(&self.open);
            while !*open {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return false;
                }
                open = self
                    .changed
                    .wait_timeout(open, remaining)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
            true
        }
    }

    fn lifetime(id: u64) -> Lifetime {
        Lifetime {
            id,
            token: CancelToken::new(),
        }
    }

    /// A job that records its slot, waits on `gate` (when given) and ends `Done(Cached)`; it reports `(name, end)`.
    fn job(
        lifetime: &Lifetime,
        name: &'static str,
        gate: Option<Arc<Latch>>,
        started: Sender<(&'static str, i32)>,
        reports: Sender<(&'static str, WorkerEnd)>,
    ) -> Job {
        Job::new(
            lifetime,
            Box::new(move |slot| {
                let _ = started.send((name, slot));
                if let Some(gate) = gate {
                    gate.wait();
                }
                WorkerEnd::Done(PreloadOutcome::Cached)
            }),
            Box::new(move |end| {
                let _ = reports.send((name, end));
            }),
        )
    }

    fn channels<T>() -> (Sender<T>, Receiver<T>) {
        mpsc::channel()
    }

    fn wait_idle(pool: &WorkerPool, idle: usize) {
        let deadline = Instant::now() + GUARD;
        while lock(&pool.inner.state).idle != idle {
            assert!(Instant::now() < deadline, "pool threads never became idle");
            thread::yield_now();
        }
    }

    #[test]
    fn panic_message_reads_str_and_string_payloads() {
        assert_eq!(panic_message(&"boom"), "boom");
        assert_eq!(panic_message(&String::from("bang")), "bang");
        assert_eq!(panic_message(&42_i32), "a preload component panicked");
    }

    #[test]
    fn wake_seq_counts_every_wake() {
        let events = RunEvents::default();
        assert_eq!(events.wake_seq(), 0);
        events.wake();
        events.wake();
        events.wake();
        assert_eq!(events.wake_seq(), 3);
    }

    #[test]
    fn wait_blocks_until_a_report_and_prefers_it_over_a_wake() {
        let events = Arc::new(RunEvents::default());
        events.register(7);
        let seen = events.wake_seq();
        let (tx, rx) = channels();
        let waiter = Arc::clone(&events);
        let handle = thread::spawn(move || {
            let _ = tx.send(waiter.wait(7, seen));
        });
        // Nothing reported, no wake: the waiter must still be blocked.
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout)
        );
        let finished = Finished {
            worker_id: 1,
            path: "a".to_owned(),
            end: WorkerEnd::Done(PreloadOutcome::Cached),
        };
        events.report(7, finished.clone());
        assert_eq!(rx.recv_timeout(GUARD), Ok(Wakeup::Finished(finished)));
        handle.join().unwrap_or_default();

        // A report already queued wins over a pending wake.
        let queued = Finished {
            worker_id: 2,
            path: "b".to_owned(),
            end: WorkerEnd::Canceled,
        };
        events.report(7, queued.clone());
        events.wake();
        assert_eq!(events.wait(7, seen), Wakeup::Finished(queued));
        // Then the wake itself; on a helper thread so that a wait which misses it fails instead of blocking.
        let (tx, rx) = channels();
        let waiter = Arc::clone(&events);
        thread::spawn(move || {
            let _ = tx.send(waiter.wait(7, seen));
        });
        assert_eq!(rx.recv_timeout(GUARD), Ok(Wakeup::Woken));
    }

    #[test]
    fn removing_a_run_wakes_its_waiter_and_drops_late_reports() {
        let events = Arc::new(RunEvents::default());
        events.register(3);
        let seen = events.wake_seq();
        let (tx, rx) = channels();
        let waiter = Arc::clone(&events);
        let handle = thread::spawn(move || {
            let _ = tx.send(waiter.wait(3, seen));
        });
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        );
        events.remove_run(3);
        assert_eq!(rx.recv_timeout(GUARD), Ok(Wakeup::Woken));
        handle.join().unwrap_or_default();
        events.report(
            3,
            Finished {
                worker_id: 1,
                path: "late".to_owned(),
                end: WorkerEnd::Canceled,
            },
        );
        assert_eq!(events.try_take(3), None);
        assert_eq!(events.wait_finished(3), None);
    }

    #[test]
    fn try_take_done_leaves_a_failed_or_cancelled_report_queued() {
        let events = RunEvents::default();
        events.register(1);
        let done = Finished {
            worker_id: 1,
            path: "a".to_owned(),
            end: WorkerEnd::Done(PreloadOutcome::Failed),
        };
        let faulted = Finished {
            worker_id: 2,
            path: "b".to_owned(),
            end: WorkerEnd::Faulted("x".to_owned()),
        };
        events.report(1, done.clone());
        events.report(1, faulted.clone());
        assert_eq!(events.try_take_done(1), Some(done));
        assert_eq!(events.try_take_done(1), None);
        assert_eq!(events.try_take(1), Some(faulted));
    }

    #[test]
    fn a_dropped_job_reports_canceled_exactly_once() {
        let (reports_tx, reports) = channels();
        let (started_tx, _started) = channels();
        drop(job(&lifetime(1), "dropped", None, started_tx, reports_tx));
        assert_eq!(reports.try_recv(), Ok(("dropped", WorkerEnd::Canceled)));
        assert!(reports.try_recv().is_err());
    }

    #[test]
    fn a_free_slot_is_reserved_and_given_back() {
        let pool = WorkerPool::new(1);
        let reservation = pool.try_reserve().expect("a fresh pool has a free slot");
        assert_eq!(reservation.slot, 0);
        assert!(pool.try_reserve().is_none(), "the only slot is taken");
        pool.release(reservation);
        assert!(pool.try_reserve().is_some(), "the released slot is free again");
    }

    #[test]
    fn no_slot_is_reserved_after_shutdown() {
        let pool = WorkerPool::new(2);
        pool.shutdown(true);
        assert!(pool.try_reserve().is_none(), "no slot after shutdown");
    }

    #[test]
    fn a_waiting_job_blocks_new_reservations() {
        let pool = WorkerPool::new(2);
        let gate = Latch::new();
        let (started_tx, started) = channels();
        let (reports_tx, reports) = channels();
        let a = pool.try_reserve().expect("slot a");
        pool.start_reserved(
            a,
            job(
                &lifetime(1),
                "a",
                Some(Arc::clone(&gate)),
                started_tx.clone(),
                reports_tx.clone(),
            ),
        );
        let b = pool.try_reserve().expect("slot b");
        pool.start_reserved(
            b,
            job(
                &lifetime(1),
                "b",
                Some(Arc::clone(&gate)),
                started_tx.clone(),
                reports_tx.clone(),
            ),
        );
        pool.enqueue(job(&lifetime(1), "c", None, started_tx, reports_tx));
        assert!(pool.try_reserve().is_none(), "every slot is taken and a job waits");
        gate.open();
        let mut names: Vec<&str> = (0..3)
            .filter_map(|_| reports.recv_timeout(GUARD).ok())
            .map(|(name, _)| name)
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["a", "b", "c"]);
        let mut seen: Vec<&str> = started.try_iter().map(|(name, _)| name).collect();
        seen.sort_unstable();
        assert_eq!(seen, ["a", "b", "c"]);
    }

    #[test]
    fn slot_numbers_follow_the_csharp_formula() {
        // C# slot = _workerCount - 1 - CurrentCount right after the take: the number of slots in use minus one.
        let pool = WorkerPool::new(2);
        let first = pool.try_reserve().expect("first");
        let second = pool.try_reserve().expect("second");
        assert_eq!((first.slot, second.slot), (0, 1));
        pool.release(first);
        // `second` still holds a slot: the next take is the second slot in use again.
        let third = pool.try_reserve().expect("third");
        assert_eq!(third.slot, 1);
        pool.release(second);
        pool.release(third);
        assert_eq!(pool.try_reserve().map(|r| r.slot), Some(0));
    }

    #[test]
    fn a_promoted_waiter_gets_the_slot_number_of_its_take() {
        let pool = WorkerPool::new(2);
        let gate = Latch::new();
        let (started_tx, started) = channels();
        let (reports_tx, reports) = channels();
        let a = pool.try_reserve().expect("a");
        let b = pool.try_reserve().expect("b");
        pool.start_reserved(
            b,
            job(
                &lifetime(1),
                "b",
                Some(Arc::clone(&gate)),
                started_tx.clone(),
                reports_tx.clone(),
            ),
        );
        pool.enqueue(job(&lifetime(1), "c", None, started_tx, reports_tx));
        pool.release(a);
        let mut slots: Vec<(&str, i32)> = (0..2).filter_map(|_| started.recv_timeout(GUARD).ok()).collect();
        slots.sort_unstable();
        assert_eq!(slots, [("b", 1), ("c", 1)]);
        gate.open();
        assert_eq!(reports.iter().take(2).count(), 2);
    }

    #[test]
    fn cancel_waiting_ends_only_the_jobs_of_that_lifetime() {
        let pool = WorkerPool::new(1);
        let held = pool.try_reserve().expect("slot");
        let old = lifetime(1);
        let live = lifetime(2);
        let (started_tx, started) = channels();
        let (reports_tx, reports) = channels();
        pool.enqueue(job(&old, "old-1", None, started_tx.clone(), reports_tx.clone()));
        pool.enqueue(job(&live, "live", None, started_tx.clone(), reports_tx.clone()));
        pool.enqueue(job(&old, "old-2", None, started_tx, reports_tx));
        pool.cancel_waiting(old.id);
        assert_eq!(reports.try_recv(), Ok(("old-1", WorkerEnd::Canceled)));
        assert_eq!(reports.try_recv(), Ok(("old-2", WorkerEnd::Canceled)));
        assert!(reports.try_recv().is_err(), "the live job still waits");
        pool.release(held);
        assert_eq!(started.recv_timeout(GUARD).map(|(name, _)| name), Ok("live"));
        assert_eq!(
            reports.recv_timeout(GUARD),
            Ok(("live", WorkerEnd::Done(PreloadOutcome::Cached)))
        );
        assert!(started.try_recv().is_err(), "a cancelled job never ran");
    }

    #[test]
    fn a_job_of_a_cancelled_lifetime_never_waits_nor_takes_a_slot() {
        let pool = WorkerPool::new(1);
        let held = pool.try_reserve().expect("slot");
        let cancelled = lifetime(1);
        let (started_tx, started) = channels();
        let (reports_tx, reports) = channels();
        // Queued while live, cancelled before a slot frees (the token flips before cancel_waiting runs).
        pool.enqueue(job(
            &cancelled,
            "late-cancel",
            None,
            started_tx.clone(),
            reports_tx.clone(),
        ));
        cancelled.token.cancel();
        // Enqueued after the cancel: ends at once.
        pool.enqueue(job(&cancelled, "after-cancel", None, started_tx, reports_tx));
        assert_eq!(reports.try_recv(), Ok(("after-cancel", WorkerEnd::Canceled)));
        pool.release(held);
        assert_eq!(reports.recv_timeout(GUARD), Ok(("late-cancel", WorkerEnd::Canceled)));
        assert!(started.try_recv().is_err(), "neither job ran");
        assert!(pool.try_reserve().is_some(), "the slot stayed free");
    }

    #[test]
    fn jobs_after_shutdown_report_canceled_without_running() {
        let pool = WorkerPool::new(2);
        pool.shutdown(true);
        let (started_tx, started) = channels();
        let (reports_tx, reports) = channels();
        pool.enqueue(job(&lifetime(1), "late", None, started_tx, reports_tx));
        assert_eq!(reports.try_recv(), Ok(("late", WorkerEnd::Canceled)));
        assert!(started.try_recv().is_err());
    }

    #[test]
    fn shutdown_with_join_waits_for_the_running_job() {
        let pool = Arc::new(WorkerPool::new(1));
        let gate = Latch::new();
        let (started_tx, started) = channels();
        let (reports_tx, reports) = channels();
        let returned = Arc::new(Mutex::new(false));
        let seen_by_job = Arc::new(Mutex::new(None));
        let observer = Arc::clone(&returned);
        let seen = Arc::clone(&seen_by_job);
        let job_gate = Arc::clone(&gate);
        let blocking = Job::new(
            &lifetime(1),
            Box::new(move |_| {
                let _ = started_tx.send(("blocking", 0));
                job_gate.wait();
                *lock(&seen) = Some(*lock(&observer));
                WorkerEnd::Done(PreloadOutcome::Cached)
            }),
            Box::new(move |end| {
                let _ = reports_tx.send(("blocking", end));
            }),
        );
        let reservation = pool.try_reserve().expect("slot");
        pool.start_reserved(reservation, blocking);
        assert!(started.recv_timeout(GUARD).is_ok());
        let stopper = Arc::clone(&pool);
        let flag = Arc::clone(&returned);
        let shutdown = thread::spawn(move || {
            stopper.shutdown(true);
            *lock(&flag) = true;
        });
        // A joining shutdown cannot return while the job runs; give a wrong one time to return early.
        let deadline = Instant::now() + Duration::from_millis(200);
        while !shutdown.is_finished() && Instant::now() < deadline {
            thread::yield_now();
        }
        gate.open();
        shutdown.join().unwrap_or_default();
        assert_eq!(
            *lock(&seen_by_job),
            Some(false),
            "shutdown returned before the running job ended"
        );
        assert!(reports.recv_timeout(GUARD).is_ok());
        assert_eq!(Arc::strong_count(&pool.inner), 1, "every pool thread was joined");
    }

    #[test]
    fn dropping_the_pool_cancels_waiting_jobs() {
        let pool = WorkerPool::new(1);
        let gate = Latch::new();
        let (started_tx, _started) = channels();
        let (reports_tx, reports) = channels();
        let held = pool.try_reserve().expect("slot");
        pool.start_reserved(
            held,
            job(
                &lifetime(1),
                "running",
                Some(Arc::clone(&gate)),
                started_tx.clone(),
                reports_tx.clone(),
            ),
        );
        pool.enqueue(job(&lifetime(1), "waiting", None, started_tx, reports_tx));
        drop(pool);
        assert_eq!(reports.recv_timeout(GUARD), Ok(("waiting", WorkerEnd::Canceled)));
        gate.open();
        assert_eq!(
            reports.recv_timeout(GUARD),
            Ok(("running", WorkerEnd::Done(PreloadOutcome::Cached)))
        );
    }

    #[test]
    fn an_idle_thread_is_reused_and_a_busy_one_gets_a_sibling() {
        let pool = WorkerPool::new(2);
        let (started_tx, started) = channels();
        let (reports_tx, reports) = channels();
        pool.enqueue(job(
            &lifetime(1),
            "warm-up",
            None,
            started_tx.clone(),
            reports_tx.clone(),
        ));
        assert!(reports.recv_timeout(GUARD).is_ok());
        wait_idle(&pool, 1);
        let gate = Latch::new();
        pool.enqueue(job(
            &lifetime(1),
            "a",
            Some(Arc::clone(&gate)),
            started_tx.clone(),
            reports_tx.clone(),
        ));
        assert_eq!(
            lock(&pool.inner.state).threads.len(),
            1,
            "the idle thread takes the job: no new thread"
        );
        assert_eq!(started.recv_timeout(GUARD).map(|(name, _)| name), Ok("warm-up"));
        assert_eq!(started.recv_timeout(GUARD).map(|(name, _)| name), Ok("a"));
        // "a" blocks the only thread: the next job needs a second one.
        pool.enqueue(job(&lifetime(1), "b", None, started_tx, reports_tx));
        assert_eq!(started.recv_timeout(GUARD).map(|(name, _)| name), Ok("b"));
        assert_eq!(lock(&pool.inner.state).threads.len(), 2);
        gate.open();
        assert_eq!(reports.iter().take(2).count(), 2);
    }

    #[test]
    fn without_pool_threads_the_jobs_run_on_the_queuing_thread() {
        let pool = WorkerPool::build(1, true);
        let me = thread::current().id();
        let (reports_tx, reports) = channels();
        let ran_on = Arc::new(Mutex::new(None));
        let record = Arc::clone(&ran_on);
        let reservation = pool.try_reserve().expect("slot");
        pool.start_reserved(
            reservation,
            Job::new(
                &lifetime(1),
                Box::new(move |_| {
                    *lock(&record) = Some(thread::current().id());
                    WorkerEnd::Done(PreloadOutcome::Cached)
                }),
                Box::new(move |end| {
                    let _ = reports_tx.send(end);
                }),
            ),
        );
        assert_eq!(*lock(&ran_on), Some(me));
        assert_eq!(reports.try_recv(), Ok(WorkerEnd::Done(PreloadOutcome::Cached)));
        assert!(pool.try_reserve().is_some(), "the slot came back");
    }

    #[test]
    fn a_panicking_job_reports_faulted_with_its_message() {
        let pool = WorkerPool::new(1);
        let (reports_tx, reports) = channels();
        pool.enqueue(Job::new(
            &lifetime(1),
            Box::new(|_| panic!("check exploded")),
            Box::new(move |end| {
                let _ = reports_tx.send(end);
            }),
        ));
        assert_eq!(
            reports.recv_timeout(GUARD),
            Ok(WorkerEnd::Faulted("check exploded".to_owned()))
        );
    }
}
