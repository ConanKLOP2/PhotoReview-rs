//! Port of `PhotoReview.Core/IO/NavigationStatWorker.cs` (C# baseline d92d6aae, Q-R29 option C).
//!
//! Runs the navigation path's file-metadata calls (the per-navigation stat that decides missing / unreadable / changed)
//! on ONE dedicated background thread, so a slow share (NAS / wifi: 2-30 ms per metadata call, or a stalled SMB request)
//! never blocks the UI thread. A dedicated thread rather than a pool: the whole-folder preload keeps up to 8 pool threads
//! busy with blocking decodes, and the current image's stat must not queue behind them.
//!
//! Work items run in FIFO order. An item whose [`CancelToken`] is already cancelled when it is dequeued (the navigation
//! was superseded while it waited) is dropped without running, so a key-held burst never pays for stale stats. The thread
//! starts on first use, is a background thread for the process lifetime and owns nothing that needs a shutdown.
//!
//! Mapping from C# (docs/adr/R0003-concurrency.md): `Task<T>` becomes [`StatTask`] (a one-shot result slot that any
//! thread can block on); there are no continuations, so the C# "continuation never runs on the worker thread" property
//! is trivially true and needs no Rust code.
use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use crate::sync::CancelToken;

/// Why a [`StatTask`] has no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatError {
    /// The token was cancelled before the item ran (C# task `IsCanceled`): the work never ran.
    Canceled,
    /// The work panicked (C#: the task faulted with the work's exception); the message is the panic text.
    Panicked(String),
}

impl fmt::Display for StatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canceled => f.write_str("the navigation stat was canceled"),
            Self::Panicked(message) => write!(f, "the navigation stat panicked: {message}"),
        }
    }
}

impl std::error::Error for StatError {}

/// Locks, recovering from poisoning (a user panic hook might poison; the data stays consistent).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Result slot shared by a [`StatTask`] and its queued job: the outcome (once known) and the wake-up for waiters.
type Slot<T> = (Mutex<Option<Result<T, StatError>>>, Condvar);

/// The result slot of one queued item (C# `Task<T>`). Created by [`NavigationStatWorker::run`].
pub struct StatTask<T> {
    slot: Arc<Slot<T>>,
}

impl<T> fmt::Debug for StatTask<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatTask").finish_non_exhaustive()
    }
}

impl<T> StatTask<T> {
    /// `true` once the item ran, panicked or was canceled (C# `IsCompleted`). Does not consume the result.
    pub fn is_completed(&self) -> bool {
        lock(&self.slot.0).is_some()
    }

    /// `true` when the task completed as canceled (C# `IsCanceled`). Does not consume the result.
    pub fn is_canceled(&self) -> bool {
        matches!(&*lock(&self.slot.0), Some(Err(StatError::Canceled)))
    }

    /// Blocks until completed or `timeout` elapsed; `true` when completed. Never call it on the UI thread.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let guard = lock(&self.slot.0);
        let (guard, _) = self
            .slot
            .1
            .wait_timeout_while(guard, timeout, |r| r.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.is_some()
    }

    /// Blocks until completed and returns the outcome. Never call it on the UI thread.
    pub fn wait(self) -> Result<T, StatError> {
        let guard = lock(&self.slot.0);
        let mut guard = self
            .slot
            .1
            .wait_while(guard, |r| r.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match guard.take() {
            Some(result) => result,
            None => Err(StatError::Canceled), // unreachable: wait_while only returns once the slot is filled
        }
    }
}

/// A queued item: the type-erased closure that completes its own task.
type Job = Box<dyn FnOnce() + Send + 'static>;

/// Shared state of the worker (queue + wake-up), private.
#[derive(Default)]
struct WorkerState {
    queue: Mutex<VecDeque<Job>>,
    wake: Condvar,
    /// Set once the thread was started (under the queue lock).
    started: Mutex<bool>,
}

/// See the module docs. `Clone` shares the same queue and thread.
#[derive(Clone)]
pub struct NavigationStatWorker {
    state: Arc<WorkerState>,
}

impl fmt::Debug for NavigationStatWorker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigationStatWorker").finish_non_exhaustive()
    }
}

impl Default for NavigationStatWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl NavigationStatWorker {
    /// A worker with its own queue; the thread starts on the first [`run`](Self::run).
    pub fn new() -> Self {
        Self {
            state: Arc::new(WorkerState::default()),
        }
    }

    /// C# `NavigationStatWorker.Shared`: the process-wide worker used by the image presenter.
    pub fn shared() -> &'static NavigationStatWorker {
        static SHARED: OnceLock<NavigationStatWorker> = OnceLock::new();
        SHARED.get_or_init(NavigationStatWorker::new)
    }

    /// C# `RunAsync(work, cancellationToken)`: queues `work` for the worker thread and returns its task at once.
    /// * `token` already cancelled: the task is completed as [`StatError::Canceled`] BEFORE this returns (it does not
    ///   queue behind a busy item) and `work` is dropped without running.
    /// * `token` cancelled while the item waits: the worker completes the task as canceled when it dequeues the item
    ///   and does not run `work`.
    /// * `work` panics: the task completes as [`StatError::Panicked`]; the worker thread survives and takes the next item.
    pub fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
        token: &CancelToken,
    ) -> StatTask<T> {
        let slot: Arc<Slot<T>> = Arc::new((Mutex::new(None), Condvar::new()));
        let task = StatTask {
            slot: Arc::clone(&slot),
        };
        if token.is_canceled() {
            // Completed before returning; never touches the queue or starts the thread. `work` is dropped unrun.
            fulfil(&slot, Err(StatError::Canceled));
            return task;
        }

        let token = token.clone();
        let job: Job = Box::new(move || {
            if token.is_canceled() {
                fulfil(&slot, Err(StatError::Canceled));
                return;
            }
            let result = match catch_unwind(AssertUnwindSafe(work)) {
                Ok(value) => Ok(value),
                Err(payload) => Err(StatError::Panicked(panic_text(payload.as_ref()))),
            };
            fulfil(&slot, result);
        });

        let start_thread = {
            let mut queue = lock(&self.state.queue);
            queue.push_back(job);
            let mut started = lock(&self.state.started);
            let first = !*started;
            *started = true;
            first
        };
        if start_thread {
            let state = Arc::clone(&self.state);
            let spawned = std::thread::Builder::new()
                .name("PhotoReview navigation stat".to_owned())
                // C# runs this thread at AboveNormal; the App layer may raise it.
                .spawn(move || worker_loop(&state));
            if spawned.is_err() {
                // Could not start a thread: allow a later `run` to retry; the queued item stays queued.
                *lock(&self.state.started) = false;
            }
        } else {
            self.state.wake.notify_one();
        }
        task
    }
}

/// Stores the outcome once (first completion wins) and wakes every waiter.
fn fulfil<T>(slot: &Slot<T>, result: Result<T, StatError>) {
    let mut guard = lock(&slot.0);
    if guard.is_none() {
        *guard = Some(result);
    }
    drop(guard);
    slot.1.notify_all();
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_owned()
    }
}

/// The worker thread: FIFO, no lock held while a job runs. Lock order is always queue then started.
fn worker_loop(state: &WorkerState) {
    loop {
        let job = {
            let mut queue = lock(&state.queue);
            loop {
                if let Some(job) = queue.pop_front() {
                    break job;
                }
                queue = state
                    .wake
                    .wait(queue)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        job();
    }
}
