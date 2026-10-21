//! Port of `PhotoReview.Shell.Win32/Hosting/SwapChainFrameClock.cs` (100 lines, baseline `d92d6aae`). Unit W8-13b.
//!
//! The main C-05 clock: ticks come from the swap chain's frame-latency waitable object (`IDXGISwapChain2::GetFrameLatencyWaitableObject`, reached
//! through `RenderSurface::frame_latency_wait_handle()` of W8-12d). The handle belongs to the swap chain; this clock NEVER closes it. The object
//! signals when the swap chain can take a new frame, that is "once per frame about to be presented" when the `Frame` handler presents on every tick.
//! - A signal that arrives within half a period of the previous tick (frame latency > 1, first frames) is deferred to the end of the period.
//! - A handler that does not present (nothing to draw) makes the object stop signalling: a fallback deadline of two periods keeps the clock running.
//!
//! Dependency on W8-12d (contract `W8-12d`, branch `wave8/b3-contracts-render-a`): only the native smoke test needs a real surface; the clock takes
//! the handle as an `isize` and every other test uses a semaphore handle or none (D-W8B3H-5).

use std::sync::Arc;

use pr_core::abstractions::DisplayClock;

use crate::host::error::HostError;
use crate::host::frame_clock::base::{
    ClockEnv, ClockPolicy, FrameClockBase, TimestampFn, next_grid_point, qpc_frequency,
};

/// State of C# `SwapChainFrameClock`.
#[derive(Debug)]
pub struct SwapChainPolicy {
    wait_handle: isize,
    fixed_period: Option<i64>,
    period: i64,
    /// QPC ticks of the latest tick; `0` = none yet.
    last_tick: i64,
    armed_at: i64,
    deferred: bool,
}

impl SwapChainPolicy {
    /// `wait_handle == 0` is `InvalidArgument { param: "frame_latency_wait_handle", message: "A swap chain frame latency waitable handle is required." }`;
    /// `fixed_period <= 0` is `ArgumentOutOfRange { param: "fixed_period" }`. `period = fixed_period` or `frequency / 60`.
    pub fn new(wait_handle: isize, fixed_period: Option<i64>) -> Result<Self, HostError> {
        if wait_handle == 0 {
            return Err(HostError::InvalidArgument {
                param: "frame_latency_wait_handle",
                message: "A swap chain frame latency waitable handle is required.",
            });
        }
        if fixed_period.is_some_and(|period| period <= 0) {
            return Err(HostError::ArgumentOutOfRange { param: "fixed_period" });
        }
        Ok(Self {
            wait_handle,
            fixed_period,
            period: fixed_period.unwrap_or(qpc_frequency() / 60),
            last_tick: 0,
            armed_at: 0,
            deferred: false,
        })
    }

    /// C# `_period` (follows the display's refresh period unless a fixed period was given).
    pub fn period(&self) -> i64 {
        self.period
    }

    /// C# `_lastTick`: QPC ticks of the latest tick, `0` = none yet.
    pub fn last_tick(&self) -> i64 {
        self.last_tick
    }

    /// C# `_deferred`.
    pub fn deferred(&self) -> bool {
        self.deferred
    }
}

impl ClockPolicy for SwapChainPolicy {
    fn wait_handle(&self) -> isize {
        self.wait_handle
    }

    /// C# `DeadlineTimestamp`: not armed -> `i64::MAX`; deferred -> `last_tick + period`; else `max(last_tick, armed_at) + 2 * period`.
    /// Saturating additions (a huge fixed period must not wrap, T7).
    fn deadline_timestamp(&self, armed: bool) -> i64 {
        if !armed {
            return i64::MAX;
        }
        if self.deferred {
            self.last_tick.saturating_add(self.period)
        } else {
            self.last_tick
                .max(self.armed_at)
                .saturating_add(self.period.saturating_mul(2))
        }
    }

    /// C# `ShouldTick` (lines 73-99). `signaled` and `last_tick != 0` and `now - last_tick < period / 2` -> `deferred = true`, `None`. Not signaled and
    /// `now < deadline_timestamp(true)` -> `None`. Otherwise `frame = now`; with no fixed period and a known `refresh_period > 0`: `period = refresh_period`,
    /// `frame = next_grid_point(timing, now)`. Then `deferred = false`, `last_tick = now`, `Some(frame)`.
    fn should_tick(&mut self, env: &ClockEnv<'_>, now: i64, signaled: bool) -> Result<Option<i64>, HostError> {
        let mut frame = now;
        if signaled {
            if self.last_tick != 0 && now.wrapping_sub(self.last_tick) < self.period / 2 {
                self.deferred = true;
                return Ok(None);
            }
        } else if now < self.deadline_timestamp(true) {
            return Ok(None);
        }
        if self.fixed_period.is_none()
            && let Some(timing) = env.current_timing().filter(|timing| timing.refresh_period > 0)
        {
            self.period = timing.refresh_period;
            frame = next_grid_point(timing, now);
        }
        self.deferred = false;
        self.last_tick = now;
        Ok(Some(frame))
    }

    /// C# `OnArmedChanged`: armed -> `armed_at = now`, `deferred = false`. Disarmed -> nothing (the handle is not ours to cancel).
    fn on_armed_changed(&mut self, _env: &ClockEnv<'_>, armed: bool, now: i64) -> Result<(), HostError> {
        if armed {
            self.armed_at = now;
            self.deferred = false;
        }
        Ok(())
    }
}

/// C# `SwapChainFrameClock`.
pub type SwapChainFrameClock = FrameClockBase<SwapChainPolicy>;

impl SwapChainFrameClock {
    /// C# public constructor `SwapChainFrameClock(frameLatencyWaitHandle, displayClock, window)`.
    pub fn create(
        frame_latency_wait_handle: isize,
        display_clock: Option<Arc<dyn DisplayClock>>,
        window: isize,
    ) -> Result<Self, HostError> {
        Self::with_seams(frame_latency_wait_handle, display_clock, window, None, None)
    }

    /// C# internal constructor with the test seams.
    pub fn with_seams(
        frame_latency_wait_handle: isize,
        display_clock: Option<Arc<dyn DisplayClock>>,
        window: isize,
        timestamp: Option<TimestampFn>,
        fixed_period: Option<i64>,
    ) -> Result<Self, HostError> {
        let policy = SwapChainPolicy::new(frame_latency_wait_handle, fixed_period)?;
        Ok(FrameClockBase::new(policy, display_clock, window, timestamp))
    }
}
