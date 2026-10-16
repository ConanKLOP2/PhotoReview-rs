//! One-shot timer seam of the preload memory re-check (C# `TimeProvider.CreateTimer(callback, null, due,
//! Timeout.InfiniteTimeSpan)` + `ITimer.Dispose`, used by `PreloadScheduler.ArmResumeCheckLocked`, C# baseline d92d6aae).
//! Contract: `contracts/W4-15b.md` (draft `contracts/drafts-w4/W4-15b/contract.md`).
//!
//! Target path: `crates/pr-imaging/src/preload/timer.rs`.
//!
//! Tests inject a manual factory whose timers record their due time and fire only when the test says so (the C#
//! `ManualTime`/`ManualTimer` fakes); production uses [`ThreadTimerFactory`]. See contract open question 2 about
//! hoisting this seam into `pr_core` if W4-14 needs one too.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// The callback a timer runs once when due. It may also run after `dispose` returned when it had already started
/// (the scheduler guards with a generation number, as C# does).
pub type TimerCallback = Arc<dyn Fn() + Send + Sync>;

/// Creates one-shot timers (C# `TimeProvider`).
pub trait OneShotTimerFactory: Send + Sync {
    /// Arms a timer that calls `callback` once, `due` from now, on a thread that is NOT the caller's and never the UI
    /// thread. Must return without calling `callback` and without blocking: the scheduler calls it while holding its
    /// gate. A thread-local perf navigation context of the caller must NOT flow into the callback (C#
    /// `ExecutionContext.SuppressFlow`).
    fn create_timer(&self, due: Duration, callback: TimerCallback) -> Box<dyn OneShotTimer>;
}

/// An armed timer (C# `ITimer`).
pub trait OneShotTimer: Send {
    /// C# `ITimer.Dispose()`: after this returns the callback will not START (one already running may finish).
    /// Idempotent; also called from `Drop` by implementations that own a thread.
    fn dispose(&self);
}

/// Production factory: one short-lived named thread (`pr-preload-recheck`) per armed timer that waits on a condition
/// variable with the due time as timeout; `dispose` sets a flag and notifies, so a disposed timer's thread exits at
/// once without calling back. At most one timer is armed per scheduler at a time.
#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadTimerFactory;

impl OneShotTimerFactory for ThreadTimerFactory {
    fn create_timer(&self, due: Duration, callback: TimerCallback) -> Box<dyn OneShotTimer> {
        let state = Arc::new(ThreadTimerState {
            disposed: Mutex::new(false),
            wake: Condvar::new(),
        });
        let waiter = Arc::clone(&state);
        // A thread that cannot be started leaves a timer that never fires: the next navigation re-kicks preload anyway.
        let _ = thread::Builder::new()
            .name("pr-preload-recheck".to_owned())
            .spawn(move || {
                if waiter.wait_until_due(due) {
                    callback();
                }
            });
        Box::new(ThreadTimer { state })
    }
}

/// The handle of one [`ThreadTimerFactory`] timer.
pub struct ThreadTimer {
    state: Arc<ThreadTimerState>,
}

/// Shared between the handle and the waiting thread: `disposed` flag under a mutex plus the condition variable.
struct ThreadTimerState {
    disposed: Mutex<bool>,
    wake: Condvar,
}

impl ThreadTimerState {
    /// Waits until `due` elapsed (`true`) or the timer was disposed (`false`). No polling: one timed condition wait.
    fn wait_until_due(&self, due: Duration) -> bool {
        let deadline = Instant::now().checked_add(due);
        let mut disposed = self.disposed.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if *disposed {
                return false;
            }
            let remaining = match deadline {
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                None => Duration::MAX,
            };
            if remaining.is_zero() {
                return true;
            }
            disposed = self
                .wake
                .wait_timeout(disposed, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn dispose(&self) {
        *self.disposed.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.wake.notify_all();
    }
}

impl fmt::Debug for ThreadTimer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThreadTimer").finish_non_exhaustive()
    }
}

impl OneShotTimer for ThreadTimer {
    fn dispose(&self) {
        self.state.dispose();
    }
}

impl Drop for ThreadTimer {
    fn drop(&mut self) {
        self.state.dispose();
    }
}
