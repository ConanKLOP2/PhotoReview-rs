//! `VBlankEstimator` against the C# tests `VBlankEstimatorTests` (synthetic vblank observations, 10 000 ticks per ms)
//! and against the golden file `tests/golden/vblank.json` (every operation, state after each one).

use pr_core::abstractions::DisplayTiming;
use pr_core::display::vblank_estimator::{CAPACITY, MIN_SAMPLES, VBlankEstimator};
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

const TICKS_PER_MS: f64 = 10_000.0;

/// C# test helper `Period(hz) = 1000 * TicksPerMs / hz` (a double).
fn period(hz: f64) -> f64 {
    1000.0 * TICKS_PER_MS / hz
}

/// C# `(long)Math.Round(x)`: ties to even, then truncation.
fn round_ties_even_i64(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// xUnit `Assert.Equal(expected, actual, tolerance)` for doubles: absolute tolerance.
fn assert_close(expected: f64, actual: f64, tolerance: f64) {
    assert!(
        (expected - actual).abs() <= tolerance,
        "expected {expected} +- {tolerance}, got {actual}"
    );
}

/// xUnit `Assert.InRange(actual, low, high)` for doubles.
fn assert_in_range(actual: f64, low: f64, high: f64) {
    assert!(low <= actual && actual <= high, "{actual} not in [{low}, {high}]");
}

/// C# `TheoryData<double>` RefreshRates.
const REFRESH_RATES: [f64; 4] = [60.0, 75.0, 144.0, 240.0];

// C#: VBlankEstimatorTests.ExactObservations_GiveThePeriodAndTheLatestVBlank
#[test]
fn exact_observations_give_the_period_and_the_latest_vblank() {
    for hz in REFRESH_RATES {
        let mut estimator = VBlankEstimator::new();
        let origin: i64 = 5_000_000;
        for i in 0..20 {
            estimator.add(origin + round_ties_even_i64(f64::from(i) * period(hz)));
        }

        let timing = estimator.current().expect("20 exact samples give a timing");
        assert_close(period(hz), timing.refresh_period as f64, 1.0);
        assert_close(origin as f64 + 19.0 * period(hz), timing.last_vblank as f64, 1.0);
    }
}

// C#: VBlankEstimatorTests.LateWakeUpsAndMissedVBlanks_DoNotShiftTheGrid
#[test]
fn late_wake_ups_and_missed_vblanks_do_not_shift_the_grid() {
    // Wake-up delays of 0.05-1.2 ms; vblanks 7, 8 and 21 are missed (the observer was busy).
    let delays_ms = [0.3, 0.05, 1.2, 0.6, 0.1, 0.9, 0.2, 0.7, 0.4, 1.0, 0.15, 0.5];
    for hz in REFRESH_RATES {
        let mut estimator = VBlankEstimator::new();
        let origin: i64 = 1_000_000;
        let mut last: i64 = 0;
        for i in 0..30_usize {
            if matches!(i, 7 | 8 | 21) {
                continue;
            }
            last = origin + round_ties_even_i64(i as f64 * period(hz));
            let delay = delays_ms[i % delays_ms.len()];
            estimator.add(last + (delay * TICKS_PER_MS) as i64);
        }

        let timing = estimator.current().expect("late samples still give a timing");
        assert_in_range(timing.refresh_period as f64, period(hz) * 0.995, period(hz) * 1.005);
        // The phase is within the smallest wake-up delay seen, not the average one.
        let phase = (timing.last_vblank - last) as f64;
        assert_in_range(phase, -0.1 * TICKS_PER_MS, 0.2 * TICKS_PER_MS);
    }
}

// C#: VBlankEstimatorTests.FewerThanTheMinimumSamples_GiveNoTiming
#[test]
fn fewer_than_the_minimum_samples_give_no_timing() {
    let mut estimator = VBlankEstimator::new();
    for i in 0..(MIN_SAMPLES as i64 - 1) {
        estimator.add(1000 + i * 166_667);
    }
    assert_eq!(estimator.current(), None);

    estimator.add(1000 + (MIN_SAMPLES as i64 - 1) * 166_667);
    assert!(estimator.current().is_some());

    estimator.reset();
    assert_eq!(estimator.current(), None);
}

// C#: VBlankEstimatorTests.ARefreshRateChange_IsFollowedWithinAFewRefreshes
#[test]
fn a_refresh_rate_change_is_followed_within_a_few_refreshes() {
    let cases = [(60.0, 75.0), (75.0, 60.0), (144.0, 240.0)];
    for (before, after) in cases {
        let mut estimator = VBlankEstimator::new();
        let mut t = 0.0_f64;
        for _ in 0..CAPACITY {
            t += period(before);
            estimator.add(t as i64);
        }
        for _ in 0..8 {
            t += period(after);
            estimator.add(t as i64);
        }

        let timing: DisplayTiming = estimator.current().expect("rate change keeps a timing");
        assert_in_range(timing.refresh_period as f64, period(after) * 0.99, period(after) * 1.01);
        assert_close(t, timing.last_vblank as f64, 10.0);
    }
}

// C#: VBlankEstimatorTests.AnIntegerRatioRateChange_IsFollowedWithinTheSampleWindow
#[test]
fn an_integer_ratio_rate_change_is_followed_within_the_sample_window() {
    let mut estimator = VBlankEstimator::new();
    let mut t = 0.0_f64;
    for _ in 0..CAPACITY {
        t += period(240.0);
        estimator.add(t as i64);
    }
    for _ in 0..CAPACITY {
        t += period(60.0);
        estimator.add(t as i64);
    }

    let timing = estimator.current().expect("rate change keeps a timing");
    assert_in_range(timing.refresh_period as f64, period(60.0) * 0.99, period(60.0) * 1.01);
}

// C#: VBlankEstimatorTests.NonIncreasingTimestamps_AreIgnored
#[test]
fn non_increasing_timestamps_are_ignored() {
    let mut estimator = VBlankEstimator::new();
    for i in 1..=6_i64 {
        estimator.add(i * 100_000);
    }
    estimator.add(300_000);

    assert_eq!(estimator.count(), 6);
    assert_eq!(estimator.current().map(|t| t.refresh_period), Some(100_000));
}

// C#: VBlankEstimator (golden oracle `oracle-cs vblank`, one trace entry per operation).
// DIVERGENCE: none. Operations: a JSON integer or a decimal string (beyond 2^53) is Add, the string "reset" is Reset.
#[test]
fn vblank_golden_replays_every_case_after_every_operation() {
    let g = golden::load("vblank");
    for case in &g.cases {
        let name = str_of(case, "name");
        let ops = case["ops"].as_array().unwrap_or_else(|| panic!("{name}: no ops array"));
        let trace = case["trace"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: no trace array"));
        assert_eq!(ops.len(), trace.len(), "{name}: ops and trace differ in length");

        let mut estimator = VBlankEstimator::new();
        for (step, (op, expect)) in ops.iter().zip(trace).enumerate() {
            match op {
                Value::String(s) if s.as_str() == "reset" => estimator.reset(),
                Value::String(s) => {
                    let timestamp: i64 = s
                        .parse()
                        .unwrap_or_else(|_| panic!("{name} step {step}: bad timestamp {s:?}"));
                    estimator.add(timestamp);
                }
                other => {
                    let timestamp = other
                        .as_i64()
                        .unwrap_or_else(|| panic!("{name} step {step}: bad op {other}"));
                    estimator.add(timestamp);
                }
            }

            let want_count = expect["count"]
                .as_u64()
                .unwrap_or_else(|| panic!("{name} step {step}: no count"));
            assert_eq!(estimator.count() as u64, want_count, "{name} step {step}: count");

            let got = estimator.current();
            if expect["last"].is_null() {
                assert_eq!(got, None, "{name} step {step}: expected no timing");
            } else {
                let timing = got.unwrap_or_else(|| panic!("{name} step {step}: expected a timing"));
                assert_eq!(timing.last_vblank, i64_of(expect, "last"), "{name} step {step}: last");
                assert_eq!(
                    timing.refresh_period,
                    i64_of(expect, "period"),
                    "{name} step {step}: period"
                );
            }
        }
    }
}

// Kills the `f64::min` mutant of `math_min`: `refreshes` wraps to 0, so `period` is -inf and the projection
// of the newest sample is `0 * -inf = NaN`, which .NET `Math.Min` propagates (`(long)NaN == 0`).
#[test]
fn wrapped_refresh_count_makes_the_projection_nan() {
    let mut estimator = VBlankEstimator::new();
    // Intervals 1, 1, i64::MAX, i64::MAX - 1: counts 1, 1, i64::MAX (saturated), i64::MAX (saturated) sum to 0.
    for t in [i64::MIN, i64::MIN + 1, i64::MIN + 2, 1, i64::MAX] {
        estimator.add(t);
    }
    assert_eq!(estimator.count(), 5);
    let timing = estimator.current().expect("baseline is 1, so a timing is reported");
    assert_eq!(timing.last_vblank, 0);
    assert_eq!(timing.refresh_period, i64::MIN);
}
