//! One-shot timer seam of the preload memory re-check (C# `TimeProvider.CreateTimer(callback, null, due,
//! Timeout.InfiniteTimeSpan)` + `ITimer.Dispose`, used by `PreloadScheduler.ArmResumeCheckLocked`, C# baseline d92d6aae).
//! Contract: `contracts/W4-15b.md` (draft `contracts/drafts-w4/W4-15b/contract.md`).
//!
//! Target path: `crates/pr-imaging/src/preload/timer.rs`.
//!
//! Tests inject a manual factory whose timers record their due time and fire only when the test says so (the C#
//! `ManualTime`/`ManualTimer` fakes); production uses [`ThreadTimerFactory`]. See contract open question 2 about
//! hoisting this seam into `pr_core` if W4-14 needs one too.

#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

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
        todo!("W4-15b: spawn the waiting thread; no sleep polling (Condvar::wait_timeout until due or disposed)")
    }
}

/// The handle of one [`ThreadTimerFactory`] timer.
pub struct ThreadTimer {
    state: Arc<ThreadTimerState>,
}

/// Shared between the handle and the waiting thread: `disposed` flag under a mutex plus the condition variable.
struct ThreadTimerState {
    disposed: std::sync::Mutex<bool>,
    wake: std::sync::Condvar,
}

impl fmt::Debug for ThreadTimer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThreadTimer").finish_non_exhaustive()
    }
}

impl OneShotTimer for ThreadTimer {
    fn dispose(&self) {
        todo!("W4-15b: set disposed under the lock (PoisonError::into_inner), notify_all")
    }
}

impl Drop for ThreadTimer {
    fn drop(&mut self) {
        todo!("W4-15b: same as dispose (the scheduler drops a replaced timer)")
    }
}
