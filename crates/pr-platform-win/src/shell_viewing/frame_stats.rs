//! Port of `PhotoReview.Shell.Win32/Viewing/FrameStats.cs` (104 lines, WP-21, baseline d92d6aae): frame and `EndDrawAndPresent`
//! times of the last `CAPACITY` drawn frames (ring), plus the count of blocked presents and up to `SLOW_CAPACITY` slow-frame
//! breakdowns. Always on (two timestamps per frame). UI thread only (C# comment), so no internal locking.
//!
//! Time units: C# converts `Stopwatch` ticks with `Stopwatch.Frequency`. The Rust type takes the tick frequency explicitly
//! (`new(ticks_per_second)`); production passes the QueryPerformanceFrequency value (W8-13 host), tests and the golden pass a
//! fixed one. The math is identical: `ms = ticks * 1000.0 / ticks_per_second`.
//!
//! Stub: bodies are `todo!()`; signatures are final (contract W8-16a, Public API).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

/// C# `FrameStats.Capacity`: ring size (frames kept for percentiles).
pub const CAPACITY: usize = 2048;

/// C# `FrameStats.BlockedThresholdMs`: a present or end-draw at or above this many ms counts as blocking the UI thread.
pub const BLOCKED_THRESHOLD_MS: f64 = 8.0;

/// C# `RecordSlow` bound: at most this many slow-frame breakdowns are kept (the first ones, never the last).
pub const SLOW_CAPACITY: usize = 64;

/// C# `ToMs(long ticks)` with an explicit frequency: `ticks * 1000.0 / ticks_per_second`.
pub fn ticks_to_ms(ticks: i64, ticks_per_second: i64) -> f64 {
    todo!()
}

/// One entry of C# `FrameStats.Slow`: `(FrameMs, DrawMs, EndDrawMs, PresentMs, Note)`.
#[derive(Debug, Clone, PartialEq)]
pub struct SlowFrame {
    pub frame_ms: f64,
    pub draw_ms: f64,
    pub end_draw_ms: f64,
    pub present_ms: f64,
    pub note: String,
}

/// C# `FrameStats` (`internal sealed class`). Samples are `f64` milliseconds; counters are `i64` (C# `long`).
#[derive(Debug)]
pub struct FrameStats {
    ticks_per_second: i64,
    frame_samples_ms: Vec<f64>,
    present_samples_ms: Vec<f64>,
    next: usize,
    count: usize,
    total: i64,
    blocked_presents: i64,
    slow: Vec<SlowFrame>,
}

impl FrameStats {
    /// `new FrameStats()` with the tick frequency of the clock that produces the ticks.
    pub fn new(ticks_per_second: i64) -> Self {
        todo!()
    }

    /// C# `Count`: frames currently held (at most `CAPACITY`).
    pub fn count(&self) -> usize {
        todo!()
    }

    /// C# `Total`: every frame ever recorded since the last `reset` (not bounded by `CAPACITY`).
    pub fn total(&self) -> i64 {
        todo!()
    }

    /// C# `BlockedPresents`: frames whose present time is at or above `BLOCKED_THRESHOLD_MS`.
    pub fn blocked_presents(&self) -> i64 {
        todo!()
    }

    /// C# `Slow`: the kept slow-frame breakdowns, in the order recorded.
    pub fn slow(&self) -> &[SlowFrame] {
        todo!()
    }

    /// C# `IsSlow(endDrawMs, presentMs)`: true when either time is at or above `BLOCKED_THRESHOLD_MS`.
    pub fn is_slow(end_draw_ms: f64, present_ms: f64) -> bool {
        todo!()
    }

    /// C# `RecordSlow`: keeps the breakdown only when `Slow.Count < SLOW_CAPACITY` and `is_slow(end_draw_ms, present_ms)`.
    pub fn record_slow(&mut self, frame_ms: f64, draw_ms: f64, end_draw_ms: f64, present_ms: f64, note: &str) {
        todo!()
    }

    /// C# `Record(long frameTicks, long presentTicks)`: one frame in the ring, `total` + 1, `blocked_presents` + 1 when the
    /// present time is at or above the threshold.
    pub fn record(&mut self, frame_ticks: i64, present_ticks: i64) {
        todo!()
    }

    /// C# `Reset()`: empties the ring, the counters and NOT the slow list (C# `Reset` leaves `Slow` alone; test pins it).
    pub fn reset(&mut self) {
        todo!()
    }

    /// C# `FrameMs(percentile)` (0..=100): nearest-rank percentile of the held frame times; `0.0` when empty.
    pub fn frame_ms(&self, percentile: f64) -> f64 {
        todo!()
    }

    /// C# `PresentMs(percentile)`: nearest-rank percentile of the held present times; `0.0` when empty.
    pub fn present_ms(&self, percentile: f64) -> f64 {
        todo!()
    }

    /// C# `MaxPresentMs()`: largest held present time; `0.0` when empty.
    pub fn max_present_ms(&self) -> f64 {
        todo!()
    }
}
