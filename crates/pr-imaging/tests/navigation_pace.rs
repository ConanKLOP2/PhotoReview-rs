//! Port tests for `pr_imaging::navigation_pace` (C# `PhotoReview.Imaging/Preload/NavigationPace.cs`, baseline d92d6aae).
//!
//! Ports the class `NavigationPaceTests` of `tests/PhotoReview.Imaging.Tests/BurstPreloadTests.cs` and every test of
//! `NavigationPaceMutationTests.cs`, plus the golden `tests/golden/navigation-pace.json`, replayed from its inputs
//! `tests/golden-inputs/navigation-pace.json`. The scheduler-level tests (`BurstPreloadSchedulerTests`,
//! `PreloadSchedulerMutationTests`) and the order tests (`PreloadOrderDirectionTests`) belong to other units.
//!
//! Red pass (W4-06a): every test fails through the stub's `todo!()` bodies. The fake clock is an `Arc<AtomicI64>`
//! shared with the pace, so a test advances time after the pace was built.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use pr_imaging::navigation_pace::{
    BURST_IDLE_FLOOR_MS, MAX_LEAD, MAX_STEP_INTERVAL_MS, MAX_VIEWER_START_DELAY_MS, NavigationPace, TICKS_PER_SECOND,
};
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

/// C# `Stopwatch.Frequency` of the fake clock: the golden's `frequency` is the same value.
const FREQUENCY: f64 = TICKS_PER_SECOND as f64;

/// Fake monotonic clock (C# `_now` in `NavigationPaceTests`, `FakeClock` in the mutation tests).
#[derive(Clone)]
struct FakeClock {
    ticks: Arc<AtomicI64>,
}

impl FakeClock {
    /// C# `private long _now = 1_000_000;`
    fn new() -> Self {
        Self {
            ticks: Arc::new(AtomicI64::new(1_000_000)),
        }
    }

    /// C# `new NavigationPace(() => _now)`.
    fn pace(&self) -> NavigationPace {
        let ticks = Arc::clone(&self.ticks);
        NavigationPace::with_timestamp(Box::new(move || ticks.load(Ordering::SeqCst)))
    }

    /// C# `Advance(ms) => _now += (long)(ms * Frequency / 1000)`: the cast truncates toward zero.
    fn advance(&self, ms: f64) {
        self.ticks.fetch_add((ms * FREQUENCY / 1000.0) as i64, Ordering::SeqCst);
    }

    /// C# `FakeClock.AdvanceMs` of `NavigationPaceMutationTests`: `Math.Round` (to even) of the tick count.
    fn advance_rounded(&self, ms: f64) {
        self.ticks
            .fetch_add((ms * FREQUENCY / 1000.0).round_ties_even() as i64, Ordering::SeqCst);
    }
}

/// C# `Steps(pace, from, count, step, intervalMs)` of `NavigationPaceTests`: records `from`, then `count` moves of
/// `step`, each one `interval_ms` after the previous record.
fn steps(clock: &FakeClock, pace: &NavigationPace, from: i32, count: i32, step: i32, interval_ms: f64) {
    pace.record(from);
    for i in 1..=count {
        clock.advance(interval_ms);
        pace.record(from + i * step);
    }
}

/// C# `Steps(params double[] gapsMs)` of `NavigationPaceMutationTests`: records index 0, then one single step per
/// gap, each after that many milliseconds on the rounded clock.
fn steps_with_gaps(gaps_ms: &[f64]) -> (NavigationPace, FakeClock) {
    let clock = FakeClock::new();
    let pace = clock.pace();
    pace.record(0);
    for (index, gap) in (1..).zip(gaps_ms) {
        clock.advance_rounded(*gap);
        pace.record(index);
    }
    (pace, clock)
}

// ---------------------------------------------------------------------------------------------------------------------
// NavigationPaceTests (BurstPreloadTests.cs)
// ---------------------------------------------------------------------------------------------------------------------

// C#: NavigationPaceTests.Direction_FollowsLastMove
#[test]
fn direction_follows_last_move() {
    let clock = FakeClock::new();
    let pace = clock.pace();
    steps(&clock, &pace, 10, 3, 1, 600.0);
    assert_eq!(pace.direction(), 1);

    clock.advance(600.0);
    pace.record(12);
    assert_eq!(pace.direction(), -1);

    pace.record(12); // post-present kick for the same navigation: no-op
    assert_eq!(pace.direction(), -1);
}

// C#: NavigationPaceTests.Jump_KeepsPreviousDirection (RV-I09)
#[test]
fn jump_keeps_previous_direction() {
    let clock = FakeClock::new();
    let pace = clock.pace();
    pace.record(500);
    pace.record(0); // Home: forward stays the review default after a jump
    assert_eq!(pace.direction(), 1);

    pace.record(1);
    assert_eq!(pace.direction(), 1);

    let back_clock = FakeClock::new();
    let back = back_clock.pace();
    back.record(5);
    back.record(4);
    assert_eq!(back.direction(), -1);
    back.record(400); // End after a Prev: the backward travel direction is kept
    assert_eq!(back.direction(), -1);
}

// C#: NavigationPaceTests.Lead_IsRateTimesDecode
#[test]
fn lead_is_rate_times_decode() {
    let clock = FakeClock::new();
    let pace = clock.pace();
    pace.record(0);
    clock.advance(33.0);
    pace.record(1);
    assert_eq!(pace.get_lead(350.0), 0); // one interval is a double-tap, not a burst

    clock.advance(33.0);
    pace.record(2);
    assert_eq!(pace.get_lead(350.0), 11); // ceil(350 / 33)
    assert_eq!(pace.get_lead(20.0), 0); // decode faster than keys: nothing to skip
    assert_eq!(pace.get_lead(10_000.0), MAX_LEAD);
}

// C#: NavigationPaceTests.Lead_ExpiresWhenBurstStops
#[test]
fn lead_expires_when_burst_stops() {
    let clock = FakeClock::new();
    let pace = clock.pace();
    steps(&clock, &pace, 0, 10, 1, 33.0);
    assert!(pace.get_lead(350.0) > 0);

    clock.advance(BURST_IDLE_FLOOR_MS + 1.0);
    assert_eq!(pace.get_lead(350.0), 0);
}

// C#: NavigationPaceTests.JumpOrPause_ResetsEstimate
#[test]
fn jump_or_pause_resets_estimate() {
    let clock = FakeClock::new();
    let pace = clock.pace();
    steps(&clock, &pace, 0, 10, 1, 33.0);
    clock.advance(33.0);
    pace.record(500); // Home/End-like jump
    assert_eq!(pace.get_lead(350.0), 0);

    steps(&clock, &pace, 500, 1, 1, 33.0);
    assert_eq!(pace.get_lead(350.0), 0); // needs a fresh run of short steps

    let paused_clock = FakeClock::new();
    let paused = paused_clock.pace();
    steps(&paused_clock, &paused, 0, 10, 1, 33.0);
    paused_clock.advance(MAX_STEP_INTERVAL_MS + 1.0);
    paused.record(11);
    assert_eq!(paused.get_lead(350.0), 0);
}

// C#: NavigationPaceTests.ViewerStartDelay
#[test]
fn viewer_start_delay() {
    let clock = FakeClock::new();
    let pace = clock.pace();
    assert_eq!(pace.get_viewer_start_delay(350.0), Duration::ZERO);

    steps(&clock, &pace, 0, 10, 1, 33.0);
    // C# TimeSpan.FromMilliseconds(33 * 1.5) = 49.5 ms
    assert_eq!(pace.get_viewer_start_delay(350.0), Duration::from_nanos(49_500_000));

    let slow_clock = FakeClock::new();
    let slow = slow_clock.pace();
    steps(&slow_clock, &slow, 0, 10, 1, 200.0);
    assert_eq!(MAX_VIEWER_START_DELAY_MS, 100.0);
    assert_eq!(slow.get_viewer_start_delay(350.0), Duration::from_millis(100));
}

// ---------------------------------------------------------------------------------------------------------------------
// NavigationPaceMutationTests.cs (HotPath category; the same fake clock, rounded)
// ---------------------------------------------------------------------------------------------------------------------

// C#: NavigationPaceMutationTests.Record_StepAtExactlyMaxInterval_IsStillABurst
#[test]
fn record_step_at_exactly_max_interval_is_still_a_burst() {
    let (pace, _) = steps_with_gaps(&[MAX_STEP_INTERVAL_MS, MAX_STEP_INTERVAL_MS]);
    assert_eq!(pace.get_lead(1000.0), 2); // interval 500 -> 1000 / 500

    let (slow, _) = steps_with_gaps(&[MAX_STEP_INTERVAL_MS + 1.0, MAX_STEP_INTERVAL_MS + 1.0]);
    assert_eq!(slow.get_lead(1000.0), 0);
}

// C#: NavigationPaceMutationTests.Record_EwmaArithmetic
#[test]
fn record_ewma_arithmetic() {
    let (one, _) = steps_with_gaps(&[100.0]);
    assert_eq!(one.get_lead(300.0), 0); // a single interval: not a burst yet

    let (steady, _) = steps_with_gaps(&[100.0, 100.0]);
    assert_eq!(steady.get_lead(300.0), 3); // interval exactly 100 -> ceil(3.0)

    // 100, then 200: 100 + 0.3 * (200 - 100) = 130 -> 300 / 130 = 2.3 -> 3
    let (blended, _) = steps_with_gaps(&[100.0, 200.0]);
    assert_eq!(blended.get_lead(300.0), 3);
}

// C#: NavigationPaceMutationTests.GetLead_DecodeEqualsInterval_IsZero
#[test]
fn get_lead_decode_equals_interval_is_zero() {
    let (pace, _) = steps_with_gaps(&[100.0, 100.0]);

    assert_eq!(pace.get_lead(100.0), 0);
    assert_eq!(pace.get_lead(50.0), 0);
    assert_eq!(pace.get_lead(101.0), 2);
    assert_eq!(pace.get_lead(0.0), 0);
    assert_eq!(pace.get_lead(100_000.0), MAX_LEAD);
}

// C#: NavigationPaceMutationTests.GetViewerStartDelay_Boundaries
#[test]
fn get_viewer_start_delay_boundaries() {
    let (pace, _) = steps_with_gaps(&[40.0, 40.0]);

    assert_eq!(pace.get_viewer_start_delay(100.0), Duration::from_millis(60));
    assert_eq!(pace.get_viewer_start_delay(40.0), Duration::ZERO); // exactly the interval
    assert_eq!(pace.get_viewer_start_delay(30.0), Duration::ZERO);
    assert_eq!(pace.get_viewer_start_delay(0.0), Duration::ZERO);

    let (slow, _) = steps_with_gaps(&[100.0, 100.0]);
    assert_eq!(slow.get_viewer_start_delay(500.0), Duration::from_millis(100));

    let idle = FakeClock::new().pace();
    assert_eq!(idle.get_viewer_start_delay(500.0), Duration::ZERO); // no burst at all
}

// C#: NavigationPaceMutationTests.ActiveWindow_IsTwiceTheInterval
#[test]
fn active_window_is_twice_the_interval() {
    let (pace, clock) = steps_with_gaps(&[100.0, 100.0]); // window = max(200, 150) = 200 ms

    clock.advance_rounded(50.0);
    assert_eq!(pace.get_lead(300.0), 3);
    clock.advance_rounded(130.0); // 180 ms since the last key: inside 2 x interval, past the 150 ms floor
    assert_eq!(pace.get_lead(300.0), 3);
    clock.advance_rounded(20.0); // exactly 200 ms: the window edge is inclusive
    assert_eq!(pace.get_lead(300.0), 3);
    clock.advance_rounded(1.0);
    assert_eq!(pace.get_lead(300.0), 0);
    assert_eq!(pace.get_viewer_start_delay(300.0), Duration::ZERO);
}

// C#: NavigationPaceMutationTests.ActiveWindow_FloorForFastBursts
#[test]
fn active_window_floor_for_fast_bursts() {
    let (pace, clock) = steps_with_gaps(&[20.0, 20.0]); // 2 x interval = 40 ms, so the 150 ms floor applies

    clock.advance_rounded(150.0);
    assert_eq!(pace.get_lead(60.0), 3);
    clock.advance_rounded(1.0);
    assert_eq!(pace.get_lead(60.0), 0);
}

// C#: NavigationPaceMutationTests.Record_JumpOrPause_RestartsTheEstimate
#[test]
fn record_jump_or_pause_restarts_the_estimate() {
    let (pace, clock) = steps_with_gaps(&[100.0, 100.0]);
    assert_eq!(pace.get_lead(300.0), 3);

    clock.advance_rounded(100.0);
    pace.record(50); // a jump, not a step
    assert_eq!(pace.get_lead(300.0), 0);

    clock.advance_rounded(100.0);
    pace.record(51);
    clock.advance_rounded(100.0);
    pace.record(52);
    assert_eq!(pace.get_lead(300.0), 3); // a fresh burst from the new position
}

// ---------------------------------------------------------------------------------------------------------------------
// Rust-only
// ---------------------------------------------------------------------------------------------------------------------

/// `NavigationPace::new()` (and `Default`) uses the monotonic `Instant` clock: a fresh pace moves forward and is calm.
#[test]
fn default_pace_starts_forward_and_calm() {
    let pace = NavigationPace::new();
    assert_eq!(pace.direction(), 1);
    assert_eq!(pace.get_lead(350.0), 0);
    assert_eq!(pace.get_viewer_start_delay(350.0), Duration::ZERO);

    let via_default = NavigationPace::default();
    assert_eq!(via_default.direction(), 1);
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/navigation-pace.json (outputs) replayed from tests/golden-inputs/navigation-pace.json (ops)
// ---------------------------------------------------------------------------------------------------------------------

/// One result in the golden's shape: `direction` / `lead` carry an integer, `delay` carries milliseconds and ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Outcome {
    Direction(i64),
    Lead(i64),
    Delay { ms: f64, ticks: i64 },
}

/// `decode_ms` of an op: a JSON number, or the strings `NaN`, `Infinity`, `-Infinity`.
fn decode_of(op: &Value) -> f64 {
    match &op["decode_ms"] {
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => panic!("unknown decode_ms {other:?}"),
        },
        number => number
            .as_f64()
            .unwrap_or_else(|| panic!("decode_ms is not a number: {number}")),
    }
}

/// An integer field of an op, as `i32`.
fn i32_of(op: &Value, key: &str) -> i32 {
    let value = op[key]
        .as_i64()
        .unwrap_or_else(|| panic!("op has no integer '{key}': {op}"));
    i32::try_from(value).unwrap_or_else(|_| panic!("'{key}' out of i32 range: {value}"))
}

/// A numeric field of an op, as `f64`.
fn f64_of(op: &Value, key: &str) -> f64 {
    op[key]
        .as_f64()
        .unwrap_or_else(|| panic!("op has no number '{key}': {op}"))
}

/// Runs the ops of one golden input case and returns its results in order.
fn run_ops(ops: &[Value]) -> Vec<Outcome> {
    let clock = FakeClock::new();
    let pace = clock.pace();
    let mut results = Vec::new();
    for op in ops {
        match op["op"].as_str().expect("op kind") {
            "record" => pace.record(i32_of(op, "index")),
            "advance" => clock.advance(f64_of(op, "ms")),
            "steps" => steps(
                &clock,
                &pace,
                i32_of(op, "from"),
                i32_of(op, "count"),
                i32_of(op, "step"),
                f64_of(op, "interval_ms"),
            ),
            "direction" => results.push(Outcome::Direction(i64::from(pace.direction()))),
            "lead" => results.push(Outcome::Lead(i64::from(pace.get_lead(decode_of(op))))),
            "delay" => {
                let delay = pace.get_viewer_start_delay(decode_of(op));
                let ticks = i64::try_from(delay.as_nanos() / 100).expect("delay fits in i64 ticks");
                results.push(Outcome::Delay {
                    ms: delay.as_secs_f64() * 1000.0,
                    ticks,
                });
            }
            other => panic!("unknown golden op {other:?}"),
        }
    }
    results
}

/// The golden's expected result for one entry of `results`.
fn expected_of(value: &Value) -> Outcome {
    match value["op"].as_str().expect("result op") {
        "direction" => Outcome::Direction(value["value"].as_i64().expect("direction value")),
        "lead" => Outcome::Lead(value["value"].as_i64().expect("lead value")),
        "delay" => Outcome::Delay {
            ms: value["ms"].as_f64().expect("delay ms"),
            ticks: value["ticks"].as_i64().expect("delay ticks"),
        },
        other => panic!("unknown result op {other:?}"),
    }
}

fn assert_outcome(actual: Outcome, expected: Outcome, context: &str) {
    match (actual, expected) {
        (
            Outcome::Delay { ms, ticks },
            Outcome::Delay {
                ms: want_ms,
                ticks: want_ticks,
            },
        ) => {
            // Whole value: the ticks are exact; `ms` is the same value in milliseconds.
            assert_eq!(ticks, want_ticks, "{context}: delay ticks");
            assert!((ms - want_ms).abs() < 1e-9, "{context}: delay ms {ms} != {want_ms}");
        }
        _ => assert_eq!(actual, expected, "{context}"),
    }
}

// Golden: every case of tests/golden/navigation-pace.json, in order, with every result compared whole.
#[test]
fn navigation_pace_golden_matches_csharp() {
    let expected = golden::load("navigation-pace");
    let inputs_path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("navigation-pace.json");
    let inputs_text =
        std::fs::read_to_string(&inputs_path).unwrap_or_else(|e| panic!("read {}: {e}", inputs_path.display()));
    let inputs: Value = serde_json::from_str(&inputs_text).expect("parse navigation-pace inputs");
    let input_cases = inputs["cases"].as_array().expect("inputs have cases");

    assert_eq!(input_cases.len(), expected.cases.len(), "input and golden case counts");
    for (input, case) in input_cases.iter().zip(&expected.cases) {
        let name = str_of(case, "name");
        assert_eq!(str_of(input, "name"), name, "case order");
        assert_eq!(i64_of(case, "frequency"), TICKS_PER_SECOND, "{name}: frequency");

        let ops = input["ops"].as_array().expect("case ops");
        let actual = run_ops(ops);
        let wanted: Vec<Outcome> = case["results"]
            .as_array()
            .expect("case results")
            .iter()
            .map(expected_of)
            .collect();

        assert_eq!(actual.len(), wanted.len(), "{name}: result count");
        for (index, (got, want)) in actual.into_iter().zip(wanted).enumerate() {
            assert_outcome(got, want, &format!("{name} result {index}"));
        }
    }
}
