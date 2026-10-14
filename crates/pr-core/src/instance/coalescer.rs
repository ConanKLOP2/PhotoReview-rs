//! Port of `PhotoReview.Core/Instance/ForwardedOpenCoalescer.cs` (C# baseline d92d6aae, 80 lines).
//!
//! Explorer starts N processes for an N-file selection and they all forward within a short burst. Requests that
//! arrive while a window is open are merged, and the running instance opens the FIRST forwarded path exactly once
//! when the window closes. A request with no path (plain "bring to front") still triggers one activation.
//!
//! `submit` never runs or waits for the open callback on the caller's thread (RV-P01): the pipe loop calls it and must
//! not be held up by the UI thread. The window runs on a worker thread (`std::thread`, R0003; no async runtime).
//!
//! Unit W2-19. Contract: `contracts/W2-19.md`.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::abstractions::{Log, NullLog};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Carries a message (a delay failure or a panic payload) to `Log::error`, which takes an `Error`.
#[derive(Debug)]
struct MessageError(String);

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MessageError {}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_owned()
    }
}

/// Opens `Some(first path)`, or `None` for a path-less activation. C# `Action<string?>`.
pub type OpenCallback = Arc<dyn Fn(Option<String>) + Send + Sync>;

/// Runs a unit of work on some thread. The default spawns a named `std::thread`; tests inject a closure that runs the
/// work inline (the C# tests rely on an already-complete delay running the whole flush inside `Submit`).
pub type Spawner = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

/// A cancellation signal a blocked window can wait on (a [`CancelToken`](crate::sync::CancelToken) is only a flag; the
/// window needs to be woken by `dispose`, so it uses a condition variable).
#[derive(Debug, Clone, Default)]
pub struct WindowCancel {
    state: Arc<(Mutex<bool>, Condvar)>,
}

impl WindowCancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels and wakes every waiter. Idempotent.
    pub fn cancel(&self) {
        let (flag, cv) = &*self.state;
        *lock(flag) = true;
        cv.notify_all();
    }

    pub fn is_canceled(&self) -> bool {
        *lock(&self.state.0)
    }

    /// Blocks until canceled.
    pub fn wait_canceled(&self) {
        let (flag, cv) = &*self.state;
        let mut guard = lock(flag);
        while !*guard {
            guard = cv.wait(guard).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Blocks up to `timeout`; true when canceled (before or during the wait).
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let (flag, cv) = &*self.state;
        let guard = lock(flag);
        let (guard, _) = cv
            .wait_timeout_while(guard, timeout, |canceled| !*canceled)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

/// Why the window delay did not simply elapse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelayFailure {
    /// The delay was canceled (C# `OperationCanceledException`): the flush ends silently, nothing is opened and the
    /// pending state is NOT reset.
    Canceled,
    /// The delay failed otherwise (C# any other exception): logged as `Forwarded-open window failed`, then the open
    /// still happens.
    Failed(String),
}

/// The wait that defines the burst window. Default: `cancel.wait_timeout(window)` -> `Err(Canceled)` when it returns
/// true. C# `Func<TimeSpan, CancellationToken, Task>`.
pub type DelayFn = Arc<dyn Fn(Duration, &WindowCancel) -> Result<(), DelayFailure> + Send + Sync>;

struct State {
    pending: bool,
    disposed: bool,
    first: Option<String>,
}

/// C# `ForwardedOpenCoalescer`.
pub struct ForwardedOpenCoalescer {
    open: OpenCallback,
    window: Duration,
    log: Arc<dyn Log>,
    delay: DelayFn,
    spawner: Option<Spawner>,
    cancel: WindowCancel,
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for ForwardedOpenCoalescer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardedOpenCoalescer")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

impl ForwardedOpenCoalescer {
    /// `log == None` -> `NullLog`; `delay == None` -> the default window wait; `spawner == None` -> a named thread.
    pub fn new(
        open: OpenCallback,
        window: Duration,
        log: Option<Arc<dyn Log>>,
        delay: Option<DelayFn>,
        spawner: Option<Spawner>,
    ) -> Self {
        let default_delay: DelayFn = Arc::new(|window, cancel| {
            if cancel.wait_timeout(window) {
                Err(DelayFailure::Canceled)
            } else {
                Ok(())
            }
        });
        Self {
            open,
            window,
            log: log.unwrap_or_else(|| Arc::new(NullLog)),
            delay: delay.unwrap_or(default_delay),
            spawner,
            cancel: WindowCancel::new(),
            state: Arc::new(Mutex::new(State {
                pending: false,
                disposed: false,
                first: None,
            })),
        }
    }

    /// Queues or merges a forwarded request (C# `Submit`). No effect after [`dispose`](Self::dispose). The first
    /// request of a burst (nothing pending) stores `paths.first()` and starts the window on a worker; later
    /// requests only fill the stored path when it is still `None` (`_first ??= first`).
    pub fn submit(&self, paths: &[String]) {
        let incoming = paths.first().cloned();
        {
            let mut state = lock(&self.state);
            if state.disposed {
                return;
            }
            if state.pending {
                state.first = state.first.take().or(incoming);
                return;
            }
            state.pending = true;
            state.first = incoming;
        }

        let open = Arc::clone(&self.open);
        let window = self.window;
        let log = Arc::clone(&self.log);
        let delay = Arc::clone(&self.delay);
        let cancel = self.cancel.clone();
        let state = Arc::clone(&self.state);
        let flush = move || flush_after_window(&open, window, log.as_ref(), &delay, &cancel, &state);

        match &self.spawner {
            Some(spawner) => spawner(Box::new(flush)),
            None => {
                let spawned = std::thread::Builder::new()
                    .name("forwarded-open-window".to_owned())
                    .spawn(flush);
                if let Err(e) = spawned {
                    // No C# counterpart: let the next submit retry.
                    lock(&self.state).pending = false;
                    self.log.error(
                        "Forwarded-open window could not start",
                        Some(&MessageError(e.to_string())),
                    );
                }
            }
        }
    }

    /// Marks disposed, cancels the window (wakes a waiting worker). Idempotent; also runs on drop. Never joins the
    /// worker: the open callback may drop the coalescer.
    pub fn dispose(&self) {
        lock(&self.state).disposed = true;
        self.cancel.cancel();
    }
}

fn flush_after_window(
    open: &OpenCallback,
    window: Duration,
    log: &dyn Log,
    delay: &DelayFn,
    cancel: &WindowCancel,
    state: &Mutex<State>,
) {
    // C# catches any non-cancel exception from the delay; a panic is the Rust equivalent.
    let outcome = catch_unwind(AssertUnwindSafe(|| delay(window, cancel)))
        .unwrap_or_else(|payload| Err(DelayFailure::Failed(panic_message(payload.as_ref()))));
    match outcome {
        Ok(()) => {}
        // Canceled without dispose leaves `pending` set, like C#.
        Err(DelayFailure::Canceled) => return,
        Err(DelayFailure::Failed(message)) => {
            log.error("Forwarded-open window failed", Some(&MessageError(message)));
        }
    }
    let first = {
        let mut state = lock(state);
        let first = state.first.take();
        state.pending = false;
        if state.disposed {
            return;
        }
        first
    };
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| open(first))) {
        log.error(
            "Forwarded open failed",
            Some(&MessageError(panic_message(payload.as_ref()))),
        );
    }
}

impl Drop for ForwardedOpenCoalescer {
    fn drop(&mut self) {
        self.dispose();
    }
}
