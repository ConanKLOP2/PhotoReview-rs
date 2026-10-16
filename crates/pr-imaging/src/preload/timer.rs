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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    const GUARD: Duration = Duration::from_secs(10);

    /// A callback that sends the name of the thread it ran on.
    fn signalling() -> (TimerCallback, mpsc::Receiver<Option<String>>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let callback: TimerCallback = Arc::new(move || {
            let name = thread::current().name().map(str::to_owned);
            let _ = tx.lock().unwrap_or_else(PoisonError::into_inner).send(name);
        });
        (callback, rx)
    }

    /// Waits (guarded) until only `callback` itself holds it: the timer thread ended and dropped its clone.
    fn wait_released(callback: &TimerCallback) -> bool {
        let deadline = Instant::now() + GUARD;
        while Arc::strong_count(callback) > 1 {
            if Instant::now() >= deadline {
                return false;
            }
            thread::yield_now();
        }
        true
    }

    #[test]
    fn a_due_timer_calls_back_once_on_the_recheck_thread() {
        let (callback, fired) = signalling();
        let _timer = ThreadTimerFactory.create_timer(Duration::from_millis(1), Arc::clone(&callback));
        assert_eq!(fired.recv_timeout(GUARD), Ok(Some("pr-preload-recheck".to_owned())));
        assert!(wait_released(&callback));
        assert!(fired.try_recv().is_err(), "a one-shot timer fires once");
    }

    #[test]
    fn a_disposed_timer_ends_its_thread_without_calling_back() {
        let (callback, fired) = signalling();
        let timer = ThreadTimerFactory.create_timer(Duration::from_secs(3600), Arc::clone(&callback));
        timer.dispose();
        timer.dispose(); // idempotent
        assert!(wait_released(&callback), "the waiting thread ended at once");
        assert!(fired.try_recv().is_err());
    }

    #[test]
    fn dropping_the_timer_disposes_it() {
        let (callback, fired) = signalling();
        let timer = ThreadTimerFactory.create_timer(Duration::from_secs(3600), Arc::clone(&callback));
        drop(timer);
        assert!(wait_released(&callback), "the waiting thread ended at once");
        assert!(fired.try_recv().is_err());
    }

    #[test]
    fn wait_until_due_reports_due_or_disposed() {
        let state = ThreadTimerState {
            disposed: Mutex::new(false),
            wake: Condvar::new(),
        };
        assert!(state.wait_until_due(Duration::ZERO));
        assert!(state.wait_until_due(Duration::from_millis(2)));
        state.dispose();
        // On a helper thread: a dispose that did nothing must fail the test, not block it for an hour.
        let state = Arc::new(state);
        let waiter = Arc::clone(&state);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(waiter.wait_until_due(Duration::from_secs(3600)));
        });
        assert_eq!(rx.recv_timeout(GUARD), Ok(false));
    }
}
