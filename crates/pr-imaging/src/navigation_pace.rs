//! Port of `PhotoReview.Imaging/Preload/NavigationPace.cs` (C# baseline d92d6aae).
//!
//! Tracks the travel direction and an EWMA of the interval between single-step navigations, so preload can
//! prioritise the side the user heads to and, during a key-held burst, start decodes far enough ahead. Thread-safe:
//! every member takes one short lock. The clock is injected as a monotonic tick source (C# `Func<long>` over
//! `Stopwatch.GetTimestamp`); the Rust tick is 100 ns ([`TICKS_PER_SECOND`], the value of `Stopwatch.Frequency` on
//! Windows), so the arithmetic matches C# bit for bit.

#![allow(unused_variables, dead_code)] // D11: remove with the last `todo!()`.

use std::sync::Mutex;
use std::time::Duration;

/// Ticks per second of the timestamp source (C# `Stopwatch.Frequency`, 10 000 000 on Windows).
/// When merged this is `pr_core::time::TICKS_PER_SECOND`; kept local so the stub compiles alone.
pub const TICKS_PER_SECOND: i64 = 10_000_000;

/// Weight of the newest inter-key interval in the EWMA.
pub const INTERVAL_ALPHA: f64 = 0.3;
/// A gap longer than this ends a run of steps (the EWMA restarts from the next interval). Inclusive limit.
pub const MAX_STEP_INTERVAL_MS: f64 = 500.0;
/// Consecutive short intervals needed before a run counts as a burst (a double-tap is not one).
pub const MIN_BURST_SAMPLES: i32 = 2;
/// Upper bound on how many images a burst may skip ahead.
pub const MAX_LEAD: i32 = 24;
/// A burst is over once no key arrived for `max(2 x interval, this floor)`.
pub const BURST_IDLE_FLOOR_MS: f64 = 150.0;
/// Upper bound on the viewer-decode start delay during a burst.
pub const MAX_VIEWER_START_DELAY_MS: f64 = 100.0;

/// Monotonic tick source; tests pass a fake one.
pub type TimestampFn = Box<dyn Fn() -> i64 + Send + Sync>;

#[derive(Debug)]
struct State {
    last_index: i32,
    last_ticks: i64,
    interval_ms: f64,
    samples: i32,
    direction: i32,
}

/// Navigation direction and key-rate tracker (C# `NavigationPace`).
pub struct NavigationPace {
    timestamp: TimestampFn,
    state: Mutex<State>,
}

impl std::fmt::Debug for NavigationPace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavigationPace").finish_non_exhaustive()
    }
}

impl NavigationPace {
    /// Uses a monotonic `std::time::Instant` based source in [`TICKS_PER_SECOND`] ticks.
    pub fn new() -> Self {
        todo!("W4-06a: default timestamp = Instant elapsed since construction, in 100 ns ticks")
    }

    /// C# `new NavigationPace(timestamp)`.
    pub fn with_timestamp(timestamp: TimestampFn) -> Self {
        todo!("W4-06a")
    }

    /// `+1` when the last single step went to a higher index, `-1` for a lower one; initially `+1`.
    pub fn direction(&self) -> i32 {
        todo!("W4-06a")
    }

    /// Records a navigation to `index`. Negative indexes and the index already recorded are ignored (idempotent).
    pub fn record(&self, index: i32) {
        todo!("W4-06a: see contract semantics 2-4")
    }

    /// About `decode_ms / interval` images ahead when that exceeds 1 (ceil, capped at [`MAX_LEAD`]), else 0; 0 outside a burst.
    pub fn get_lead(&self, decode_ms: f64) -> i32 {
        todo!("W4-06a")
    }

    /// `min(100, 1.5 x interval)` during a burst when `decode_ms > interval`, else zero.
    pub fn get_viewer_start_delay(&self, decode_ms: f64) -> Duration {
        todo!("W4-06a: see contract open question 1 (TimeSpan.FromMilliseconds rounding)")
    }

    // Caller holds the lock.
    fn active_interval_ms(&self, state: &State) -> f64 {
        todo!("W4-06a")
    }
}

impl Default for NavigationPace {
    fn default() -> Self {
        Self::new()
    }
}
