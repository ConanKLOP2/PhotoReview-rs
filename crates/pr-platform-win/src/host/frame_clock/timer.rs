//! Port of `PhotoReview.Shell.Win32/Hosting/TimerFrameClock.cs` (149 lines, baseline `d92d6aae`). Unit W8-13a.
//!
//! The fallback C-05 clock (no waitable swap chain: WARP, RDP, before the surface exists, tests): a high-resolution waitable timer that is
//! armed once per frame. Period = the monitor's refresh period when `DisplayClock` knows it, else 60 Hz; frames anchor to the vblank grid when
//! known. A late timer drops the missed frames (no burst), so there is never more than one tick per period. Not armed = timer cancelled = no
//! wake-up. The timer is `gfx::extras::WaitableTimer` (D-W8B3G-4; the host copy `PendingKernel32` is not ported).

#![allow(unused_variables, dead_code)] // W8-13a stub: the porter removes this line when the last `todo!()` is gone.

use std::sync::Arc;

use pr_core::abstractions::{DisplayClock, DisplayTiming};

use super::base::{ClockEnv, ClockPolicy, FrameClockBase, TimestampFn};
use crate::gfx::extras::WaitableTimer;
use crate::host::error::HostError;

/// C# `EarlyTolerance`: `Stopwatch.Frequency / 500` (2 ms). A signal earlier than the scheduled frame by at most this still counts.
pub fn early_tolerance() -> i64 {
    todo!("W8-13a")
}

/// C# `ArmTimer` arithmetic: the relative due time in 100 ns units, negative (= relative) and at least `-1`:
/// `-max(1, (due - now) * 10_000_000 / frequency)`. The product is computed in `i128` and clamped, so a huge `due - now` cannot wrap (T7).
pub fn relative_due_100ns(due: i64, now: i64, frequency: i64) -> i64 {
    todo!("W8-13a")
}

/// Result of C# `Schedule`'s decision part, pure so it can be tested without a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextDue {
    /// The QPC time of the next frame.
    pub due: i64,
    /// The period to keep: `timing.refresh_period` when there is no fixed period and the timing is known, else the old period.
    pub period: i64,
}

/// C# `Schedule` (lines 109-138) minus the timer call. Rules, in this order:
/// 1. `fixed_period.is_none()` and `timing.refresh_period > 0`: the period becomes `refresh_period`.
/// 2. no fixed period, known timing, and (`!has_last_frame` or `now - last_frame >= period`): `due = now` (idle display: draw at once, gate-3 perf rule).
/// 3. no fixed period, known timing: `earliest = if !has_last_frame { now } else { max(now, last_frame + period / 2) }`, `due = next_grid_point(timing, earliest)`.
/// 4. otherwise: `due = if !has_last_frame { now } else { max(now, last_frame + period) }`.
pub fn compute_next_due(
    fixed_period: Option<i64>,
    period: i64,
    has_last_frame: bool,
    last_frame: i64,
    now: i64,
    timing: Option<DisplayTiming>,
) -> NextDue {
    todo!("W8-13a")
}

/// State of C# `TimerFrameClock`.
#[derive(Debug)]
pub struct TimerPolicy {
    timer: WaitableTimer,
    frequency: i64,
    fixed_period: Option<i64>,
    period: i64,
    /// QPC ticks of the scheduled frame (valid while `scheduled`).
    due: i64,
    scheduled: bool,
    last_frame: i64,
    has_last_frame: bool,
}

impl TimerPolicy {
    /// Creates the timer (high resolution, falling back to a normal one, both failing is `Err`) and the initial period
    /// (`fixed_period` or `frequency / 60`). `fixed_period <= 0` is `ArgumentOutOfRange { param: "fixed_period" }`.
    pub fn new(fixed_period: Option<i64>) -> Result<Self, HostError> {
        todo!("W8-13a")
    }

    /// The timer (test access: a test waits on it to prove `cancel`; C# tests read `WaitHandle`).
    pub fn timer(&self) -> &WaitableTimer {
        todo!("W8-13a")
    }

    /// C# private `Schedule(now)`: `compute_next_due`, store, `arm_timer`.
    fn schedule(&mut self, env: &ClockEnv<'_>, now: i64) -> Result<(), HostError> {
        todo!("W8-13a")
    }

    /// C# private `ArmTimer(due, now)`: `timer.set_due_100ns(relative_due_100ns(..))`; a failure is `HostError::Gfx`.
    fn arm_timer(&self, due: i64, now: i64) -> Result<(), HostError> {
        todo!("W8-13a")
    }
}

impl ClockPolicy for TimerPolicy {
    fn wait_handle(&self) -> isize {
        todo!("W8-13a")
    }

    /// C# `ShouldTick` (lines 76-98): `frame = due`; not signaled or not scheduled -> `None`; `now < due - early_tolerance` -> re-arm for `due`, `None`;
    /// `now - due >= period` -> `frame = now`; `scheduled = false`; `Some(frame)`.
    fn should_tick(&mut self, env: &ClockEnv<'_>, now: i64, signaled: bool) -> Result<Option<i64>, HostError> {
        todo!("W8-13a")
    }

    /// C# `OnArmedChanged`: armed -> `schedule(now)`; disarmed -> `scheduled = false`, `timer.cancel()` (a cancel failure is ignored: the C# ignored it).
    fn on_armed_changed(&mut self, env: &ClockEnv<'_>, armed: bool, now: i64) -> Result<(), HostError> {
        todo!("W8-13a")
    }

    /// C# `OnTicked`: `last_frame = frame_timestamp`, `has_last_frame = true`, `schedule(now)`.
    fn on_ticked(&mut self, env: &ClockEnv<'_>, frame_timestamp: i64, now: i64) -> Result<(), HostError> {
        todo!("W8-13a")
    }

    /// C# `DisposeCore`: close the timer handle now (the `WaitableTimer` field closes it on drop as well).
    fn dispose_core(&mut self) {
        todo!("W8-13a")
    }
}

/// C# `TimerFrameClock`.
pub type TimerFrameClock = FrameClockBase<TimerPolicy>;

impl TimerFrameClock {
    /// C# public constructor `TimerFrameClock(displayClock, window)`: real QPC, period from the display (60 Hz fallback).
    pub fn create(display_clock: Option<Arc<dyn DisplayClock>>, window: isize) -> Result<Self, HostError> {
        todo!("W8-13a")
    }

    /// C# internal constructor with the test seams (`timestamp`, `fixedPeriod`).
    pub fn with_seams(
        display_clock: Option<Arc<dyn DisplayClock>>,
        window: isize,
        timestamp: Option<TimestampFn>,
        fixed_period: Option<i64>,
    ) -> Result<Self, HostError> {
        todo!("W8-13a")
    }

    /// C# `ScheduledFrame`: the scheduled frame's QPC ticks, `None` when nothing is scheduled.
    pub fn scheduled_frame(&self) -> Option<i64> {
        todo!("W8-13a")
    }

    /// C# `Period`.
    pub fn period(&self) -> i64 {
        todo!("W8-13a")
    }
}
