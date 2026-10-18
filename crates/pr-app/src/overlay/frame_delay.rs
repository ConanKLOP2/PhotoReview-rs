//! Port of `PhotoReview.Shell.Win32/Overlay/FrameDelayTimer.cs` (WP-23): a one-shot delay counted in frames, replacing the
//! `DispatcherTimer` of the auto-hide timers. Baseline `d92d6aae`. Owned by W8-15b; pure (no clock, no `windows`, no `unsafe`).
//!
//! Same host contract as [`crate::overlay::animator::Animator`]: the host owns the single frame subscription, forwards each
//! `FrameTick` to [`FrameDelayTimer::on_frame`], subscribes when [`FrameDelayTimer::start`] reports `newly_running`, unsubscribes
//! whenever [`FrameDelayTimer::is_running`] turns false, and always calls `request_frame` after `start`.
//!
//! The deadline is in the timestamp's own units (Stopwatch ticks in the host, injected by the caller).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::frame_clock::FrameTick;

/// What `start` changed. `request_frame` is always required after `start` (C# `_clock.RequestFrame()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameDelayStart {
    /// The timer was not running before this call: the host must subscribe to its frame clock now (C# `_clock.Frame += OnFrame`).
    /// False when it was already running (only the deadline changed).
    pub newly_running: bool,
}

/// Construction and use errors (C# `ArgumentOutOfRangeException` / `ObjectDisposedException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDelayError {
    /// `ticks_per_second <= 0` (C# `ArgumentOutOfRangeException`, only reachable when the C# argument is given explicitly).
    InvalidTicksPerSecond(i64),
    /// `start` after `dispose` (C# `ObjectDisposedException`).
    Disposed,
}

impl std::fmt::Display for FrameDelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl std::error::Error for FrameDelayError {}

/// C# `FrameDelayTimer`. Invariant: `deadline` is meaningful only while `running`.
#[derive(Debug)]
pub struct FrameDelayTimer {
    ticks_per_second: i64,
    deadline: i64,
    running: bool,
    disposed: bool,
}

impl FrameDelayTimer {
    /// C# constructor. `ticks_per_second` is the frequency of the timestamps the host passes to `start` and `on_frame`
    /// (C# `Stopwatch.Frequency` by default; the port has no default). `<= 0` is `InvalidTicksPerSecond`.
    pub fn new(ticks_per_second: i64) -> Result<Self, FrameDelayError> {
        todo!()
    }

    /// C# `IsRunning`.
    pub fn is_running(&self) -> bool {
        todo!()
    }

    /// C# `Start(TimeSpan delay)` with the timestamp passed in (C# `_timestamp()`). `now_timestamp` is the current timestamp
    /// in the timer's units; `delay_ticks` is a .NET `TimeSpan` tick count (100 ns; `Duration::as_nanos() / 100` in the host).
    /// Rules: `Disposed` when disposed. Otherwise `secs = delay_ticks as f64 / 1e7` (TimeSpan.TotalSeconds), `ticks =
    /// (max(0, secs) * ticks_per_second as f64).ceil() as i64` (C# `(long)Math.Ceiling`, saturating cast), `deadline =
    /// now_timestamp.wrapping_add(ticks)`. A running timer keeps running (new deadline replaces the old one); otherwise it
    /// starts (`newly_running = true`). Always requests a frame (the host does that from the returned value).
    pub fn start(&mut self, now_timestamp: i64, delay_ticks: i64) -> Result<FrameDelayStart, FrameDelayError> {
        todo!()
    }

    /// C# `Stop()`: idempotent. Afterwards `is_running()` is false and the host unsubscribes.
    pub fn stop(&mut self) {
        todo!()
    }

    /// C# `OnFrame(sender, e)` for one frame. Returns `false` (no effect) when not running or when
    /// `tick.timestamp < deadline`. Otherwise the timer stops (`is_running()` becomes false) and the function returns `true`:
    /// the host then runs the elapsed action. The action may call `start` again (C# `CallbackMayRestartTheTimer`), which
    /// the host must handle by re-subscribing after the call.
    pub fn on_frame(&mut self, tick: &FrameTick) -> bool {
        todo!()
    }

    /// C# `Dispose()`: stops the timer; later `start` returns `Disposed`. Idempotent.
    pub fn dispose(&mut self) {
        todo!()
    }
}
