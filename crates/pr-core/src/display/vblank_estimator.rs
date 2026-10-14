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
        let _ = &mut self.samples;
        todo!("port VBlankEstimator.Reset")
    }

    /// C# `Add(long timestamp)`: records a vblank observed at `timestamp` (QPC ticks). A timestamp that is not
    /// greater than the most recent sample is ignored (nothing changes, not even `current`). Otherwise the sample
    /// is stored (the oldest is overwritten when full), a refresh-rate change is detected and old samples dropped,
    /// and `current` is recomputed.
    pub fn add(&mut self, timestamp: i64) {
        let _ = (timestamp, &mut self.next);
        todo!("port VBlankEstimator.Add")
    }
}
