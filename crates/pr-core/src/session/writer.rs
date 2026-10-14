//! Port of `PhotoReview.Core/Session/SessionWriter.cs` (C# baseline d92d6aae).
//!
//! Debounces [`SessionStore::save`]: [`SessionWriter::update`] only records the newest state per folder and schedules ONE
//! write after the debounce window (500 ms), so a burst of navigation costs one atomic write instead of one per image.
//! [`SessionWriter::flush`] writes everything pending now (folder change, shutdown). Threads, not async
//! (docs/adr/R0003-concurrency.md): a short-lived timer thread per debounce window, a dedicated save thread per bounded
//! flush, `Mutex` + `Condvar` instead of `lock`/`Monitor`/`SemaphoreSlim`, [`CancelToken`] instead of
//! `CancellationTokenSource`, and an injectable [`DebounceTimer`] instead of the `Func<TimeSpan, CancellationToken, Task>`
//! delay, so tests fire the timer by hand (no sleeping).
//!
//! Safety story to preserve (ADR 0007 section 2: a session write is best effort, but never silently stale):
//! a newer snapshot always wins over an older batch that is still waiting for the writer lock (per-folder version
//! counters); `flush` and `dispose` never hang behind a stuck disk write (bounded waits of 2 s); nothing is written after
//! `dispose`; a failing write never loses the rest of the batch.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::store::{BlankFolder, SessionState, SessionStore};
use crate::abstractions::{Log, panic_message};
use crate::sync::CancelToken;
use crate::text::ignore_case_key;

/// C# `SessionWriter.DefaultDebounce`: 500 ms.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(500);

/// C# `ShutdownWait` / `BoundedWaitForTests` default: the longest `flush()` and `dispose()` wait for a write: 2 s.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// Longest accepted debounce: `Task.Delay`'s own limit, `uint.MaxValue - 1` milliseconds (C# `MaxDebounceMilliseconds`).
pub const MAX_DEBOUNCE_MILLISECONDS: u64 = 4_294_967_294;

/// The debounce passed to [`SessionWriter::new`] was zero or longer than [`MAX_DEBOUNCE_MILLISECONDS`]
/// (C# `ArgumentOutOfRangeException`). A zero debounce would run the write inside `update` on the caller's thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidDebounce(pub Duration);

impl fmt::Display for InvalidDebounce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The debounce must be positive and no longer than Task.Delay supports (got {:?}).",
            self.0
        )
    }
}

impl std::error::Error for InvalidDebounce {}

/// How a running debounce timer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerOutcome {
    /// The full span elapsed (C#: the delay task completed): the debounced write may run.
    Elapsed,
    /// The token was cancelled first (C#: `OperationCanceledException`): the writer was flushed or disposed; do not write.
    Canceled,
}

/// A timer that failed to start or broke while waiting (C# `_delay` throwing).
pub type TimerError = Box<dyn std::error::Error + Send + Sync>;

/// The debounce clock (C# `delay` constructor argument `Func<TimeSpan, CancellationToken, Task>`).
///
/// Two steps, like the C# call + await: `start` runs SYNCHRONOUSLY inside `SessionWriter::update` (under the writer's gate),
/// so a test timer can count requests and capture the spans right after `update` returns; the returned [`RunningTimer`]
/// is waited on a short-lived timer thread.
pub trait DebounceTimer: Send + Sync {
    /// Starts a timer of `span` that ends early when `cancel` is cancelled; returns at once. `Err` = the delay could not be
    /// created (C#: `_delay` throws synchronously): the writer logs "Session write timer failed", forgets the schedule and
    /// keeps the pending state for the next `update`, `flush` or `dispose`.
    fn start(&self, span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError>;
}

/// One started timer.
pub trait RunningTimer: Send {
    /// Blocks the timer thread until the span elapsed or the token was cancelled. `Err` = the timer broke while waiting
    /// (C#: the delay task faulted): handled like a failed `start`.
    fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError>;
}

/// Production timer: `wait` sleeps in slices of at most 10 ms and checks the token between slices
/// (a [`CancelToken`] has no wake-up; 10 ms latency on cancel is fine for a 500 ms debounce).
#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadTimer;

impl DebounceTimer for ThreadTimer {
    fn start(&self, span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        Ok(Box::new(ThreadTimerRun {
            started: Instant::now(),
            span,
            cancel: cancel.clone(),
        }))
    }
}

/// Longest sleep slice of [`ThreadTimer`] between two looks at the token.
const TIMER_SLICE: Duration = Duration::from_millis(10);

/// One running [`ThreadTimer`].
struct ThreadTimerRun {
    started: Instant,
    span: Duration,
    cancel: CancelToken,
}

impl RunningTimer for ThreadTimerRun {
    fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError> {
        loop {
            if self.cancel.is_canceled() {
                return Ok(TimerOutcome::Canceled);
            }
            // Measured from the start (no `Instant + span`): a span of ~49 days must not overflow.
            let elapsed = self.started.elapsed();
            if elapsed >= self.span {
                return Ok(TimerOutcome::Elapsed);
            }
            thread::sleep((self.span - elapsed).min(TIMER_SLICE));
        }
    }
}

/// A one-shot completion: C# `Task` that only has to be waited for. Cloning shares the same flag.
#[derive(Debug, Clone, Default)]
pub struct Completion {
    state: Arc<(Mutex<bool>, Condvar)>,
}

impl Completion {
    /// A completion that is not done yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// A completion that is already done (C# `Task.CompletedTask`).
    pub fn completed() -> Self {
        let completion = Self::new();
        completion.complete();
        completion
    }

    fn flag(&self) -> MutexGuard<'_, bool> {
        self.state.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Marks it done and wakes every waiter. Idempotent.
    pub fn complete(&self) {
        let mut done = self.flag();
        *done = true;
        drop(done);
        self.state.1.notify_all();
    }

    /// `true` once [`complete`](Self::complete) ran.
    pub fn is_complete(&self) -> bool {
        *self.flag()
    }

    /// Blocks until done.
    pub fn wait(&self) {
        let mut done = self.flag();
        while !*done {
            done = self.state.1.wait(done).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Blocks until done or `timeout` elapsed; `true` when done.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let started = Instant::now();
        let mut done = self.flag();
        while !*done {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return false;
            }
            done = self
                .state
                .1
                .wait_timeout(done, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

/// Completes the wrapped [`Completion`] when dropped (also while unwinding), so no waiter can hang.
struct CompleteOnDrop(Completion);

impl Drop for CompleteOnDrop {
    fn drop(&mut self) {
        self.0.complete();
    }
}

/// A panic caught around a store call or a write, reported to the log as an error.
#[derive(Debug)]
struct PanicError(String);

impl fmt::Display for PanicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for PanicError {}

/// Test seam (C# `StartSaveForTests`): runs the given work on its own thread and returns its completion, or an error when
/// the thread cannot be started. The default spawns a thread named `PhotoReview.SessionSave`.
pub type SaveSpawner = Box<dyn Fn(Box<dyn FnOnce() + Send + 'static>) -> io::Result<Completion> + Send + Sync>;

/// Test seam: a hook run on the writer's behalf.
pub type Hook = Box<dyn Fn() + Send + Sync>;

/// Binary semaphore with a timed acquire (C# `SemaphoreSlim(1, 1)`); the save thread releases what the flusher acquired.
#[derive(Debug, Default)]
struct WriteLock {
    held: Mutex<bool>,
    released: Condvar,
}

impl WriteLock {
    fn held(&self) -> MutexGuard<'_, bool> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// C# `_writeLock.Wait()`.
    fn acquire(&self) {
        let mut held = self.held();
        while *held {
            held = self.released.wait(held).unwrap_or_else(PoisonError::into_inner);
        }
        *held = true;
    }

    /// C# `_writeLock.Wait(timeout)`: `true` when acquired.
    fn acquire_timeout(&self, timeout: Duration) -> bool {
        let started = Instant::now();
        let mut held = self.held();
        while *held {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return false;
            }
            held = self
                .released
                .wait_timeout(held, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *held = true;
        true
    }

    /// C# `_writeLock.Release()`; may run on another thread than the one that acquired.
    fn release(&self) {
        *self.held() = false;
        self.released.notify_all();
    }
}

/// Releases the writer lock when dropped (C# `finally { _writeLock.Release(); }`).
struct ReleaseOnDrop<'a>(&'a WriteLock);

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// Shared form of a [`SaveSpawner`].
type SharedSaveSpawner = Arc<dyn Fn(Box<dyn FnOnce() + Send + 'static>) -> io::Result<Completion> + Send + Sync>;

/// Test hooks, all optional.
#[derive(Default)]
struct Hooks {
    after_drain: Option<Arc<dyn Fn() + Send + Sync>>,
    before_shutdown_wait: Option<Arc<dyn Fn() + Send + Sync>>,
    save_spawner: Option<SharedSaveSpawner>,
    bounded_wait: Option<Duration>,
}

/// Everything guarded by the gate (C# `lock (_gate)`).
#[derive(Default)]
struct GateState {
    disposed: bool,
    /// Newest snapshot per folder key (see `key_of`), in first-insertion order. At most a handful of entries.
    pending: Vec<(String, SessionState)>,
    /// Folder key (OrdinalIgnoreCase) -> version of the newest snapshot. Never cleared.
    versions: HashMap<String, i64>,
    /// The scheduled debounce window, if any: (id, token). `None` = no write is scheduled.
    timer: Option<(u64, CancelToken)>,
    next_timer_id: u64,
    /// The most recently scheduled timer thread's completion (C# `_lastRun`).
    last_run: Completion,
    /// Batches drained by a writer that are not written yet.
    in_flight: u32,
}

struct Shared {
    store: Arc<SessionStore>,
    log: Option<Arc<dyn Log>>,
    timer: Arc<dyn DebounceTimer>,
    debounce: Duration,
    gate: Mutex<GateState>,
    /// Signalled (`notify_all`) whenever `in_flight` drops.
    in_flight_changed: Condvar,
    write_lock: WriteLock,
    hooks: Mutex<Hooks>,
}

/// Debounced, thread-safe session persistence. All methods take `&self`; share as `Arc<SessionWriter>`.
pub struct SessionWriter {
    shared: Arc<Shared>,
}

impl fmt::Debug for SessionWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionWriter")
            .field("debounce", &self.shared.debounce)
            .finish_non_exhaustive()
    }
}

/// A drained batch: (folder key, snapshot, version of that snapshot), in pending order.
type Batch = Arc<Vec<(String, SessionState, i64)>>;

/// C# `KeyOf` + the `OrdinalIgnoreCase` dictionary comparer: the session-file identity of `folder` (canonical folder, or
/// the raw string when it cannot be canonicalised), folded so `C:\a\`, `C:\a` and `C:\A\` are one key.
fn key_of(folder: &str) -> String {
    match SessionStore::canonical_folder(folder) {
        Ok(canonical) => ignore_case_key(&canonical),
        Err(_) => ignore_case_key(folder),
    }
}

/// The default bounded save worker: a thread named `PhotoReview.SessionSave` (C# `TaskCreationOptions.LongRunning`).
fn spawn_save_thread(work: Box<dyn FnOnce() + Send + 'static>) -> io::Result<Completion> {
    let completion = Completion::new();
    let done = CompleteOnDrop(completion.clone());
    thread::Builder::new()
        .name("PhotoReview.SessionSave".to_owned())
        .spawn(move || {
            let _done = done;
            work();
        })?;
    Ok(completion)
}

/// Decrements `in_flight` and wakes `dispose` when a drained batch is done (also on early return and while unwinding).
struct InFlightGuard<'a>(&'a Shared);

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        let mut gate = self.0.gate();
        gate.in_flight = gate.in_flight.saturating_sub(1);
        drop(gate);
        self.0.in_flight_changed.notify_all();
    }
}

impl Shared {
    fn gate(&self) -> MutexGuard<'_, GateState> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn hooks(&self) -> MutexGuard<'_, Hooks> {
        self.hooks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn log_error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        if let Some(log) = &self.log {
            log.error(message, error);
        }
    }

    /// C# `Update(state)`.
    fn update(self: &Arc<Self>, state: &SessionState) -> Result<(), BlankFolder> {
        if state.folder.trim().is_empty() {
            return Err(BlankFolder);
        }
        let snapshot = state.clone();
        let mut gate = self.gate();
        if gate.disposed {
            return Ok(());
        }
        let key = key_of(&snapshot.folder);
        let version = gate.versions.entry(key.clone()).or_insert(0);
        *version = version.wrapping_add(1);
        match gate.pending.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = snapshot,
            None => gate.pending.push((key, snapshot)),
        }
        if gate.timer.is_some() {
            return Ok(()); // a write is already scheduled; it will pick up the latest state
        }

        // Scheduled under the gate, as C# runs the synchronous part of RunTimerAsync inside the lock.
        let id = gate.next_timer_id;
        gate.next_timer_id = gate.next_timer_id.wrapping_add(1);
        let token = CancelToken::new();
        gate.timer = Some((id, token.clone()));
        let completion = Completion::new();
        gate.last_run = completion.clone();
        let failure: Option<TimerError> = match self.timer.start(self.debounce, &token) {
            Err(error) => Some(error),
            Ok(running) => {
                let shared = Arc::clone(self);
                let done = CompleteOnDrop(completion.clone());
                let spawned = thread::Builder::new()
                    .name("PhotoReview.SessionTimer".to_owned())
                    .spawn(move || {
                        let _done = done;
                        shared.run_timer(id, running);
                    });
                spawned.err().map(|error| -> TimerError { Box::new(error) })
            }
        };
        if let Some(error) = failure {
            // A broken timer must not leave the schedule set (no later update would ever schedule again): forget it; the
            // pending state stays queued for the next update's timer, flush or dispose.
            gate.timer = None;
            completion.complete();
            drop(gate);
            self.log_error("Session write timer failed", Some(&*error));
        }
        Ok(())
    }

    /// C# `RunTimerAsync` (timer thread).
    fn run_timer(self: &Arc<Self>, id: u64, running: Box<dyn RunningTimer>) {
        match running.wait() {
            Ok(TimerOutcome::Elapsed) => {}
            Ok(TimerOutcome::Canceled) => return,
            Err(error) => {
                self.log_error("Session write timer failed", Some(&*error));
                let mut gate = self.gate();
                if gate.timer.as_ref().is_some_and(|(current, _)| *current == id) {
                    gate.timer = None;
                }
                return;
            }
        }
        {
            let mut gate = self.gate();
            if !gate.timer.as_ref().is_some_and(|(current, _)| *current == id) {
                return; // flushed (or replaced) while we were waiting
            }
            gate.timer = None;
        }
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| self.write_pending(false))) {
            let error = PanicError(panic_message(&*payload));
            self.log_error("Session write failed", Some(&error));
        }
    }

    /// C# `Flush(bounded: true)`.
    fn flush(self: &Arc<Self>) {
        let scheduled = self.gate().timer.take();
        if let Some((_, token)) = scheduled {
            token.cancel();
        }
        self.write_pending(true);
    }

    /// C# `WritePending(bounded)`.
    fn write_pending(self: &Arc<Self>, bounded: bool) {
        let batch: Vec<(String, SessionState, i64)> = {
            let mut gate = self.gate();
            let pending = std::mem::take(&mut gate.pending);
            let batch: Vec<_> = pending
                .into_iter()
                .map(|(key, state)| {
                    let version = gate.versions.get(&key).copied().unwrap_or(0);
                    (key, state, version)
                })
                .collect();
            if !batch.is_empty() {
                gate.in_flight += 1;
            }
            batch
        };
        if batch.is_empty() {
            return;
        }
        let _in_flight = InFlightGuard(self);
        let after_drain = self.hooks().after_drain.clone();
        if let Some(hook) = after_drain {
            hook();
        }
        self.write_batch(Arc::new(batch), bounded);
    }

    /// C# `WriteBatch(batch, bounded)`.
    fn write_batch(self: &Arc<Self>, batch: Batch, bounded: bool) {
        if batch.is_empty() {
            return;
        }
        if !bounded {
            self.write_lock.acquire();
            let _release = ReleaseOnDrop(&self.write_lock);
            self.save_batch(&batch);
            return;
        }

        // One budget for the lock wait AND the save itself: acquiring the free lock says nothing about how long a
        // synchronous save takes (a stalled disk/NAS), and a UI-thread flush or the shutdown must not hang behind it.
        let budget = Instant::now();
        let (wait, spawner) = {
            let hooks = self.hooks();
            (hooks.bounded_wait.unwrap_or(SHUTDOWN_WAIT), hooks.save_spawner.clone())
        };
        if !self.write_lock.acquire_timeout(wait) {
            self.log_error("Session write skipped: writer busy", None);
            self.requeue_skipped(&batch);
            return;
        }
        // The save runs on its own thread, which owns the lock until it is done. If it outlives the budget the caller
        // moves on and the write completes later (ADR 0007 section 2: best effort); the state is not requeued.
        let shared = Arc::clone(self);
        let work_batch = Arc::clone(&batch);
        let work: Box<dyn FnOnce() + Send + 'static> = Box::new(move || {
            let _release = ReleaseOnDrop(&shared.write_lock);
            shared.save_batch(&work_batch);
        });
        let started = match spawner {
            Some(spawner) => spawner(work),
            None => spawn_save_thread(work),
        };
        let completion = match started {
            Ok(completion) => completion,
            Err(error) => {
                // D-07: the worker's own guard never ran, so release the lock here or every later write blocks on it
                // forever; the state goes back for the next flush.
                self.write_lock.release();
                self.log_error(
                    "Session write skipped: the save worker could not be started",
                    Some(&error),
                );
                self.requeue_skipped(&batch);
                return;
            }
        };
        let remaining = wait.saturating_sub(budget.elapsed());
        if !completion.wait_timeout(remaining) {
            self.log_error(
                "Session write may be lost at shutdown: the save is still running after the wait budget",
                None,
            );
        }
    }

    /// C# `SaveBatch(batch)`: runs with the writer lock held (timer thread or save thread).
    fn save_batch(&self, batch: &[(String, SessionState, i64)]) {
        for (key, state, version) in batch {
            // A newer snapshot may have arrived while this batch was waiting for the writer lock: never overwrite it.
            if self.gate().versions.get(key) != Some(version) {
                continue;
            }
            // Best effort: a failed write (any error, a panic included) must not drop the rest of the batch.
            match catch_unwind(AssertUnwindSafe(|| self.store.save(state))) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => self.log_error(&format!("Session write failed: {}", state.folder), Some(&error)),
                Err(payload) => {
                    let error = PanicError(panic_message(&*payload));
                    self.log_error(&format!("Session write failed: {}", state.folder), Some(&error));
                }
            }
        }
    }

    /// C# `RequeueSkipped`: a bounded wait that gave up must not lose the state: put it back (unless a newer snapshot
    /// arrived) for the next flush.
    fn requeue_skipped(&self, batch: &[(String, SessionState, i64)]) {
        let mut gate = self.gate();
        if gate.disposed {
            return;
        }
        for (key, state, version) in batch {
            if gate.versions.get(key) == Some(version) && !gate.pending.iter().any(|(k, _)| k == key) {
                gate.pending.push((key.clone(), state.clone()));
            }
        }
    }

    /// C# `Dispose()`.
    fn dispose(self: &Arc<Self>) {
        {
            let mut gate = self.gate();
            if gate.disposed {
                return;
            }
            gate.disposed = true;
        }
        let started = Instant::now();
        self.flush();
        self.wait_for_in_flight(SHUTDOWN_WAIT.saturating_sub(started.elapsed()));
    }

    /// C# `WaitForInFlightWrites`: the timer thread may have drained the pending states and not written them yet, so
    /// `flush` saw an empty batch: wait (bounded) for it so the process does not exit under the last write (R2-A-04).
    fn wait_for_in_flight(&self, timeout: Duration) {
        let hook = self.hooks().before_shutdown_wait.clone();
        let mut gate = self.gate();
        if gate.in_flight == 0 {
            return;
        }
        if let Some(hook) = hook {
            hook(); // under the gate, as C#: the hook only signals
        }
        let started = Instant::now();
        while gate.in_flight > 0 {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                drop(gate);
                self.log_error(
                    "Session write may be lost at shutdown: a write was still in flight",
                    None,
                );
                return;
            }
            gate = self
                .in_flight_changed
                .wait_timeout(gate, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl SessionWriter {
    /// C# `new SessionWriter(store, log, debounce, delay)`. `debounce` default [`DEFAULT_DEBOUNCE`], `timer` default
    /// [`ThreadTimer`]. `Err` when `debounce` is zero or longer than [`MAX_DEBOUNCE_MILLISECONDS`].
    pub fn new(
        store: Arc<SessionStore>,
        log: Option<Arc<dyn Log>>,
        debounce: Option<Duration>,
        timer: Option<Arc<dyn DebounceTimer>>,
    ) -> Result<Self, InvalidDebounce> {
        let debounce = debounce.unwrap_or(DEFAULT_DEBOUNCE);
        // Zero would run the write inside update on the caller's thread; an over-long value makes Task.Delay throw in C#.
        if debounce.is_zero() || debounce.as_millis() > u128::from(MAX_DEBOUNCE_MILLISECONDS) {
            return Err(InvalidDebounce(debounce));
        }
        Ok(Self {
            shared: Arc::new(Shared {
                store,
                log,
                timer: timer.unwrap_or_else(|| Arc::new(ThreadTimer)),
                debounce,
                gate: Mutex::new(GateState {
                    last_run: Completion::completed(),
                    ..GateState::default()
                }),
                in_flight_changed: Condvar::new(),
                write_lock: WriteLock::default(),
                hooks: Mutex::new(Hooks::default()),
            }),
        })
    }

    /// C# `Update(state)`: records a COPY of `state` (later mutation by the caller is not observed) as the newest snapshot
    /// of its folder and schedules the debounced write if none is scheduled. Ignored after [`dispose`](Self::dispose).
    /// `Err` only for a blank `state.folder`.
    pub fn update(&self, state: &SessionState) -> Result<(), BlankFolder> {
        self.shared.update(state)
    }

    /// C# `Flush()`: cancels the scheduled write and writes every pending state now, waiting at most
    /// [`SHUTDOWN_WAIT`] (or the test override) for the writer lock and the save; never hangs behind a stuck disk write.
    pub fn flush(&self) {
        self.shared.flush();
    }

    /// C# `FlushAsync()`: runs [`flush`](Self::flush) on its own thread (named `PhotoReview.SessionFlush`); join the handle to wait.
    /// `Err` only when the OS cannot start a thread.
    pub fn flush_async(&self) -> io::Result<JoinHandle<()>> {
        // The thread holds the shared state, not a `SessionWriter`: dropping it can never run `dispose`.
        let shared = Arc::clone(&self.shared);
        thread::Builder::new()
            .name("PhotoReview.SessionFlush".to_owned())
            .spawn(move || shared.flush())
    }

    /// C# `WhenIdleAsync()`: blocks until the most recently scheduled debounced write (timer thread) has finished.
    pub fn when_idle(&self) {
        let last_run = self.shared.gate().last_run.clone();
        last_run.wait();
    }

    /// C# `Dispose()`: marks the writer disposed (further `update`s are ignored), flushes (bounded), then waits for a
    /// batch some timer thread already drained but has not written, so that the total time stays within
    /// [`SHUTDOWN_WAIT`] counting the time the flush took. A second call does nothing.
    pub fn dispose(&self) {
        self.shared.dispose();
    }

    /// C# `BoundedWaitForTests`: how long a bounded flush waits for the writer lock and for the save. Default
    /// [`SHUTDOWN_WAIT`].
    #[doc(hidden)]
    pub fn set_bounded_wait_for_tests(&self, wait: Duration) {
        self.shared.hooks().bounded_wait = Some(wait);
    }

    /// C# `AfterDrainForTests`: runs after a NON-empty batch was drained and before the writer lock is taken. Runs outside
    /// the gate, so the hook may call `update`/`flush` (C# test `Flush_StaleBatchDrainedBeforeNewerUpdate...` does).
    #[doc(hidden)]
    pub fn set_after_drain_for_tests(&self, hook: Option<Hook>) {
        self.shared.hooks().after_drain = hook.map(Arc::from);
    }

    /// C# `BeforeShutdownWaitForTests`: runs in `dispose` when it is about to wait for an in-flight write, WHILE HOLDING
    /// the gate (as in C#): the hook must only signal, never call writer methods.
    #[doc(hidden)]
    pub fn set_before_shutdown_wait_for_tests(&self, hook: Option<Hook>) {
        self.shared.hooks().before_shutdown_wait = hook.map(Arc::from);
    }

    /// C# `StartSaveForTests`: replaces how the bounded save worker is started; `None` restores the default.
    #[doc(hidden)]
    pub fn set_save_spawner_for_tests(&self, spawner: Option<SaveSpawner>) {
        self.shared.hooks().save_spawner = spawner.map(|s| -> SharedSaveSpawner { Arc::from(s) });
    }
}

impl Drop for SessionWriter {
    /// C# `IDisposable`: dropping the writer disposes it (flushes, bounded).
    fn drop(&mut self) {
        self.dispose();
    }
}
