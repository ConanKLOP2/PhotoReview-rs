//! Port of `PhotoReview.Shell.Win32/Viewing/FrameStats.cs` (104 lines, WP-21, baseline d92d6aae): frame and `EndDrawAndPresent`
//! times of the last `CAPACITY` drawn frames (ring), plus the count of blocked presents and up to `SLOW_CAPACITY` slow-frame
//! breakdowns. Always on (two timestamps per frame). UI thread only (C# comment), so no internal locking.
//!
//! Time units: C# converts `Stopwatch` ticks with `Stopwatch.Frequency`. The Rust type takes the tick frequency explicitly
//! (`new(ticks_per_second)`); production passes the QueryPerformanceFrequency value (W8-13 host), tests and the golden pass a
//! fixed one. The math is identical: `ms = ticks * 1000.0 / ticks_per_second`.

/// C# `FrameStats.Capacity`: ring size (frames kept for percentiles).
pub const CAPACITY: usize = 2048;

/// C# `FrameStats.BlockedThresholdMs`: a present or end-draw at or above this many ms counts as blocking the UI thread.
pub const BLOCKED_THRESHOLD_MS: f64 = 8.0;

/// C# `RecordSlow` bound: at most this many slow-frame breakdowns are kept (the first ones, never the last).
pub const SLOW_CAPACITY: usize = 64;

/// C# `ToMs(long ticks)` with an explicit frequency: `ticks * 1000.0 / ticks_per_second`.
pub fn ticks_to_ms(ticks: i64, ticks_per_second: i64) -> f64 {
    // Same operation order as C# (`ticks * 1000.0 / Stopwatch.Frequency`); precomputing `1000.0 / freq` rounds differently (T7).
    ticks as f64 * 1000.0 / ticks_per_second as f64
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
        Self {
            ticks_per_second,
            frame_samples_ms: vec![0.0; CAPACITY],
            present_samples_ms: vec![0.0; CAPACITY],
            next: 0,
            count: 0,
            total: 0,
            blocked_presents: 0,
            slow: Vec::new(),
        }
    }

    /// C# `Count`: frames currently held (at most `CAPACITY`).
    pub fn count(&self) -> usize {
        self.count
    }

    /// C# `Total`: every frame ever recorded since the last `reset` (not bounded by `CAPACITY`).
    pub fn total(&self) -> i64 {
        self.total
    }

    /// C# `BlockedPresents`: frames whose present time is at or above `BLOCKED_THRESHOLD_MS`.
    pub fn blocked_presents(&self) -> i64 {
        self.blocked_presents
    }

    /// C# `Slow`: the kept slow-frame breakdowns, in the order recorded.
    pub fn slow(&self) -> &[SlowFrame] {
        &self.slow
    }

    /// C# `IsSlow(endDrawMs, presentMs)`: true when either time is at or above `BLOCKED_THRESHOLD_MS`.
    pub fn is_slow(end_draw_ms: f64, present_ms: f64) -> bool {
        present_ms >= BLOCKED_THRESHOLD_MS || end_draw_ms >= BLOCKED_THRESHOLD_MS
    }

    /// C# `RecordSlow`: keeps the breakdown only when `Slow.Count < SLOW_CAPACITY` and `is_slow(end_draw_ms, present_ms)`.
    pub fn record_slow(&mut self, frame_ms: f64, draw_ms: f64, end_draw_ms: f64, present_ms: f64, note: &str) {
        if self.slow.len() < SLOW_CAPACITY && Self::is_slow(end_draw_ms, present_ms) {
            self.slow.push(SlowFrame {
                frame_ms,
                draw_ms,
                end_draw_ms,
                present_ms,
                note: note.to_owned(),
            });
        }
    }

    /// C# `Record(long frameTicks, long presentTicks)`: one frame in the ring, `total` + 1, `blocked_presents` + 1 when the
    /// present time is at or above the threshold.
    pub fn record(&mut self, frame_ticks: i64, present_ticks: i64) {
        let frame_ms = ticks_to_ms(frame_ticks, self.ticks_per_second);
        let present_ms = ticks_to_ms(present_ticks, self.ticks_per_second);

        self.frame_samples_ms[self.next] = frame_ms;
        self.present_samples_ms[self.next] = present_ms;
        self.next = (self.next + 1) % CAPACITY;
        self.count = (self.count + 1).min(CAPACITY);
        self.total = self.total.saturating_add(1);
        if present_ms >= BLOCKED_THRESHOLD_MS {
            self.blocked_presents = self.blocked_presents.saturating_add(1);
        }
    }

    /// C# `Reset()`: empties the ring, the counters and NOT the slow list (C# `Reset` leaves `Slow` alone; test pins it).
    pub fn reset(&mut self) {
        self.next = 0;
        self.count = 0;
        self.total = 0;
        self.blocked_presents = 0;
    }

    /// C# `FrameMs(percentile)` (0..=100): nearest-rank percentile of the held frame times; `0.0` when empty.
    pub fn frame_ms(&self, percentile: f64) -> f64 {
        percentile_of(&self.frame_samples_ms, self.count, percentile)
    }

    /// C# `PresentMs(percentile)`: nearest-rank percentile of the held present times; `0.0` when empty.
    pub fn present_ms(&self, percentile: f64) -> f64 {
        percentile_of(&self.present_samples_ms, self.count, percentile)
    }

    /// C# `MaxPresentMs()`: largest held present time, starting from `0.0` like `Math.Max` (so negatives give `0.0`).
    pub fn max_present_ms(&self) -> f64 {
        self.present_samples_ms[..self.count]
            .iter()
            .copied()
            .fold(0.0, f64::max)
    }
}

/// Nearest-rank percentile over the first `count` samples (the held ones; the ring is filled from index 0 until it wraps).
/// `rank = ceil(clamp(p, 0, 100) / 100 * count) - 1`, clamped to `[0, count - 1]`; `0.0` when `count` is 0.
fn percentile_of(samples: &[f64], count: usize, percentile: f64) -> f64 {
    if count == 0 {
        return 0.0;
    }
    let mut sorted: Vec<f64> = samples[..count].to_vec();
    // Samples come from integer ticks, so they are always finite (T17): `total_cmp` is plain ascending order here.
    sorted.sort_by(f64::total_cmp);
    let raw = (percentile.clamp(0.0, 100.0) / 100.0 * count as f64).ceil();
    // A NaN percentile gives a NaN `raw`: rank 0 (the smallest sample), as the C# `Math.Clamp` path would with a NaN too.
    let rank = if raw.is_nan() {
        0
    } else {
        (raw as usize).saturating_sub(1).min(count - 1)
    };
    sorted[rank]
}
