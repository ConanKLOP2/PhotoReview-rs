//! Port of the `VBlankEstimator` class in `PhotoReview.Core/Abstractions/IDisplayClock.cs` (C# baseline e441d67a).
//! (`DisplayTiming` and the `DisplayClock` trait of the same C# file are already in
//! [`pr_core::abstractions`](crate::abstractions).)
//!
//! Estimates a display's vblank grid from observed vblank timestamps (QPC ticks taken right after a vblank wait
//! returned: each is the true vblank plus a non-negative wake-up delay, and a busy observer can miss vblanks). Pure,
//! any refresh rate. The period is the observed span divided by the number of refreshes in it; the phase is the
//! earliest projection of the recent samples onto the latest vblank, which discards late wake-ups.
//!
//! Arithmetic contract (docs/MULTI-AGENT-PLAN.md section 7, items 3 and 4), pinned by `tests/golden/vblank.json`:
//! every `long` operation of the C# code is UNCHECKED (wraps), so use `wrapping_*`; `Math.Round` is ties-to-even
//! (`f64::round_ties_even`); `(long)double` saturates and maps NaN to 0 (that is Rust `as i64`); `Math.Min` and
//! `Math.Max` propagate NaN and order `-0.0` below `0.0` (Rust `f64::min`/`max` do neither).

use crate::abstractions::DisplayTiming;

/// Samples kept (about half a second at 60 Hz).
pub const CAPACITY: usize = 32;

/// Samples needed before a timing is reported.
pub const MIN_SAMPLES: usize = 4;

/// C# `VBlankEstimator`: a ring of the last [`CAPACITY`] increasing timestamps and the grid estimated from them.
#[derive(Debug, Clone)]
pub struct VBlankEstimator {
    samples: [i64; CAPACITY],
    count: usize,
    next: usize,
    current: Option<DisplayTiming>,
}

impl Default for VBlankEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl VBlankEstimator {
    /// An empty estimator (`Count == 0`, `Current == None`).
    pub const fn new() -> Self {
        Self {
            samples: [0; CAPACITY],
            count: 0,
            next: 0,
            current: None,
        }
    }

    /// C# `Count`: samples currently held (at most [`CAPACITY`]; drops to 5 after a refresh-rate change).
    pub fn count(&self) -> usize {
        self.count
    }

    /// C# `Current`: the estimated grid after the last [`add`](Self::add), or `None` with fewer than [`MIN_SAMPLES`]
    /// samples (and after [`reset`](Self::reset)).
    pub fn current(&self) -> Option<DisplayTiming> {
        self.current
    }

    /// C# `Reset()`: forget every sample and the current timing.
    pub fn reset(&mut self) {
        self.count = 0;
        self.next = 0;
        self.current = None;
    }

    /// C# `Add(long timestamp)`: records a vblank observed at `timestamp` (QPC ticks). A timestamp that is not
    /// greater than the most recent sample is ignored (nothing changes, not even `current`). Otherwise the sample
    /// is stored (the oldest is overwritten when full), a refresh-rate change is detected and old samples dropped,
    /// and `current` is recomputed.
    pub fn add(&mut self, timestamp: i64) {
        if self.count > 0 && timestamp <= self.samples[(self.next + CAPACITY - 1) % CAPACITY] {
            return;
        }
        self.samples[self.next] = timestamp;
        self.next = (self.next + 1) % CAPACITY;
        if self.count < CAPACITY {
            self.count += 1;
        }
        self.drop_samples_before_a_rate_change();
        self.current = self.compute();
    }

    /// C# `Sample(int index)`: `index` 0 is the OLDEST held sample.
    fn sample(&self, index: usize) -> i64 {
        self.samples[(self.next + CAPACITY - self.count + index) % CAPACITY]
    }

    /// C# `Compute()`: the grid from the held samples, or `None` with too few samples or a non-positive baseline.
    fn compute(&self) -> Option<DisplayTiming> {
        if self.count < MIN_SAMPLES {
            return None;
        }
        let n = self.count - 1;
        let mut intervals = [0_i64; CAPACITY];
        for (i, interval) in intervals.iter_mut().enumerate().take(n) {
            *interval = self.sample(i + 1).wrapping_sub(self.sample(i));
        }
        // Lower quartile: most intervals are one refresh; a missed vblank gives ~2 refreshes, a late wake-up
        // followed by an on-time one gives slightly less than one, so the lower quartile is a safe base.
        let mut sorted = intervals;
        sorted[..n].sort_unstable();
        let baseline = sorted[n / 4] as f64;
        if baseline <= 0.0 {
            return None;
        }

        let mut counts = [0_i64; CAPACITY];
        let mut refreshes: i64 = 0;
        for i in 0..n {
            counts[i] = 1_i64.max((intervals[i] as f64 / baseline).round_ties_even() as i64);
            refreshes = refreshes.wrapping_add(counts[i]);
        }
        let period = self.sample(self.count - 1).wrapping_sub(self.sample(0)) as f64 / refreshes as f64;

        // Project every sample onto the latest vblank; the earliest projection has the least wake-up delay.
        let mut latest = f64::MAX;
        let mut ahead: i64 = 0;
        for i in (0..self.count).rev() {
            latest = math_min(latest, self.sample(i) as f64 + (ahead as f64) * period);
            if i > 0 {
                ahead = ahead.wrapping_add(counts[i - 1]);
            }
        }
        Some(DisplayTiming {
            last_vblank: latest.round_ties_even() as i64,
            refresh_period: period.round_ties_even() as i64,
        })
    }

    /// C# `DropSamplesBeforeARateChange()`: a refresh-rate change shows as several consecutive intervals that fit
    /// neither the old period nor a whole multiple of it; keep only the samples after the change.
    fn drop_samples_before_a_rate_change(&mut self) {
        const RECENT: usize = 4;
        if self.count < RECENT + MIN_SAMPLES {
            return;
        }
        let older_span = self.sample(self.count - RECENT - 1).wrapping_sub(self.sample(0));
        let older_intervals = self.count - RECENT - 1;
        let older = older_span as f64 / older_intervals as f64;
        for i in (self.count - RECENT)..self.count {
            let ratio = self.sample(i).wrapping_sub(self.sample(i - 1)) as f64 / older;
            let nearest = math_max(1.0, ratio.round_ties_even());
            if (ratio - nearest).abs() < 0.15 * nearest {
                return;
            }
        }
        let keep = RECENT + 1;
        let mut kept = [0_i64; CAPACITY];
        for (k, slot) in kept.iter_mut().enumerate().take(keep) {
            *slot = self.sample(self.count - keep + k);
        }
        // C# Reset() also clears Current; add() recomputes it straight after.
        self.reset();
        for &sample in kept.iter().take(keep) {
            self.samples[self.next] = sample;
            self.next = (self.next + 1) % CAPACITY;
            self.count += 1;
        }
    }
}

/// C# `Math.Min(double, double)`: NaN in either operand gives NaN; `-0.0` orders below `0.0`.
fn math_min(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if b.is_nan() {
        return b;
    }
    if a == b {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}

/// C# `Math.Max(double, double)`: NaN in either operand gives NaN; `-0.0` orders below `0.0`.
fn math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if b.is_nan() {
        return b;
    }
    if a == b {
        return if a.is_sign_negative() { b } else { a };
    }
    if a > b { a } else { b }
}
