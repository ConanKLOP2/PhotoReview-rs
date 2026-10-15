//! Port of `PhotoReview.Imaging/Preload/NavigationPace.cs` (C# baseline d92d6aae).
//!
//! Tracks the travel direction and an EWMA of the interval between single-step navigations, so preload can
//! prioritise the side the user heads to and, during a key-held burst, start decodes far enough ahead. Thread-safe:
//! every member takes one short lock. The clock is injected as a monotonic tick source (C# `Func<long>` over
//! `Stopwatch.GetTimestamp`); the Rust tick is 100 ns ([`TICKS_PER_SECOND`], the value of `Stopwatch.Frequency` on
//! Windows), so the arithmetic matches C# bit for bit.

use std::sync::{Mutex, MutexGuard, PoisonError};
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
        let origin = std::time::Instant::now();
        Self::with_timestamp(Box::new(move || {
            // 100 ns ticks since construction; saturates instead of wrapping after ~29 000 years.
            i64::try_from(origin.elapsed().as_nanos() / 100).unwrap_or(i64::MAX)
        }))
    }

    /// C# `new NavigationPace(timestamp)`.
    pub fn with_timestamp(timestamp: TimestampFn) -> Self {
        Self {
            timestamp,
            state: Mutex::new(State {
                last_index: -1,
                last_ticks: 0,
                interval_ms: 0.0,
                samples: 0,
                direction: 1,
            }),
        }
    }

    /// `+1` when the last single step went to a higher index, `-1` for a lower one; initially `+1`.
    pub fn direction(&self) -> i32 {
        self.lock().direction
    }

    /// Records a navigation to `index`. Negative indexes and the index already recorded are ignored (idempotent).
    pub fn record(&self, index: i32) {
        if index < 0 {
            return;
        }
        let mut state = self.lock();
        if index == state.last_index {
            return;
        }
        let now = (self.timestamp)();
        if state.last_index >= 0 {
            let delta = index - state.last_index;
            // RV-I09: only a single step says which way the user travels. A jump (Home/End, a file action
            // landing elsewhere) keeps the previous direction; after Home, Next is still the review default.
            if delta.abs() == 1 {
                state.direction = delta;
            }
            let elapsed_ms = ticks_to_ms(now.wrapping_sub(state.last_ticks));
            // Only single steps held or pressed in quick succession form a burst; a jump or a pause restarts
            // the estimate. The step limit is inclusive.
            if delta.abs() == 1 && elapsed_ms <= MAX_STEP_INTERVAL_MS {
                state.interval_ms = if state.samples == 0 {
                    elapsed_ms
                } else {
                    // C# form `i + a * (e - i)`: the two forms differ in the last bit, the golden is whole-value.
                    state.interval_ms + INTERVAL_ALPHA * (elapsed_ms - state.interval_ms)
                };
                state.samples = state.samples.saturating_add(1);
            } else {
                state.interval_ms = 0.0;
                state.samples = 0;
            }
        }
        state.last_index = index;
        state.last_ticks = now;
    }

    /// About `decode_ms / interval` images ahead when that exceeds 1 (ceil, capped at [`MAX_LEAD`]), else 0; 0 outside a burst.
    pub fn get_lead(&self, decode_ms: f64) -> i32 {
        let state = self.lock();
        let interval = self.active_interval_ms(&state);
        if interval <= 0.0 || decode_ms <= 0.0 {
            return 0;
        }
        let ratio = decode_ms / interval;
        // `as` saturates and maps NaN to 0, matching C# `(int)Math.Ceiling` on .NET 9: NaN gives 0, +inf gives MAX_LEAD.
        if ratio <= 1.0 {
            0
        } else {
            MAX_LEAD.min(ratio.ceil() as i32)
        }
    }

    /// `min(100, 1.5 x interval)` during a burst when `decode_ms > interval`, else zero.
    pub fn get_viewer_start_delay(&self, decode_ms: f64) -> Duration {
        let state = self.lock();
        let interval = self.active_interval_ms(&state);
        // NaN decode is not `<= interval`, so it gets the delay (as in C#); the input is not validated.
        if interval <= 0.0 || decode_ms <= interval {
            return Duration::ZERO;
        }
        let ms = MAX_VIEWER_START_DELAY_MS.min(interval * 1.5);
        // Open question 1 (contract): the golden keeps sub-millisecond ticks (49.5 ms = 495 000 ticks), so no
        // round-to-ms. Round to the nearest 100 ns tick, as .NET `TimeSpan.FromMilliseconds` does for positive values.
        let ticks = (ms * 10_000.0).round() as u64;
        Duration::from_nanos(ticks * 100)
    }

    // Caller holds the lock (`state` is the guard's contents).
    fn active_interval_ms(&self, state: &State) -> f64 {
        if state.samples < MIN_BURST_SAMPLES || state.interval_ms <= 0.0 {
            return 0.0;
        }
        let since_last_ms = ticks_to_ms((self.timestamp)().wrapping_sub(state.last_ticks));
        // The window edge is inclusive: exactly equal is still active.
        if since_last_ms > (2.0 * state.interval_ms).max(BURST_IDLE_FLOOR_MS) {
            0.0
        } else {
            state.interval_ms
        }
    }

    /// Locks the state. A poisoned lock is recovered: the state is plain numbers, so it is always consistent.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Converts a tick delta to milliseconds. Multiplies before dividing, as C# `(now - last) * 1000.0 / Frequency`.
/// A negative delta (a clock going backwards) stays negative: it is not clamped.
fn ticks_to_ms(ticks: i64) -> f64 {
    ticks as f64 * 1000.0 / TICKS_PER_SECOND as f64
}

impl Default for NavigationPace {
    fn default() -> Self {
        Self::new()
    }
}
