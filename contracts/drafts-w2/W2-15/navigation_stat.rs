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
// STUB (contract writer): the bodies are `todo!()`; the porter removes this allow together with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
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
        todo!()
    }

    /// `true` when the task completed as canceled (C# `IsCanceled`). Does not consume the result.
    pub fn is_canceled(&self) -> bool {
        todo!()
    }

    /// Blocks until completed or `timeout` elapsed; `true` when completed. Never call it on the UI thread.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        todo!()
    }

    /// Blocks until completed and returns the outcome. Never call it on the UI thread.
    pub fn wait(self) -> Result<T, StatError> {
        todo!()
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
        todo!()
    }
}
