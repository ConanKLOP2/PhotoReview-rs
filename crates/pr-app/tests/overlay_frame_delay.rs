//! Red tests (W8-15b, test porter) for `pr_app::overlay::frame_delay`: a one-shot delay counted in frames.
//!
//! Port of `PhotoReview.Shell.Tests/Overlay/OverlayPartsTests.cs`, class `FrameDelayTimerTests` (5 facts, lines 9 to 104),
//! plus the golden `tests/golden/overlay-frame-delay.json` (10 `script` cases). Every test here fails through `todo!()`
//! until the porter fills the bodies of `overlay::frame_delay`.
//!
//! Time convention: the C# fake clock ticks in milliseconds (`ticks_per_second = 1000`). `TimeSpan` delays are .NET tick
//! counts (100 ns), so 1 ms is `10_000` ticks.

use pr_app::input::frame_clock::FrameTick;
use pr_app::overlay::frame_delay::{FrameDelayError, FrameDelayTimer};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// One .NET `TimeSpan` millisecond in ticks (100 ns units).
const TICKS_PER_MS: i64 = 10_000;

/// The C# fake clock frame: only the timestamp matters to the timer.
fn tick(timestamp: i64) -> FrameTick {
    FrameTick {
        timestamp,
        rendering_time_ms: 0.0,
        timing: None,
    }
}

/// A timer in the C# test units (1000 ticks per second), unwrapped.
fn timer() -> FrameDelayTimer {
    FrameDelayTimer::new(1000).expect("1000 ticks per second is valid")
}

// C#: FrameDelayTimerTests.Fires_OnTheFirstFrameAtOrAfterTheDeadline_ExactlyOnce
#[test]
fn fires_on_the_first_frame_at_or_after_the_deadline_exactly_once() {
    let mut timer = timer();
    let started = timer.start(100, 500 * TICKS_PER_MS).expect("start");
    assert!(started.newly_running);
    assert!(timer.is_running());

    assert!(!timer.on_frame(&tick(599)), "599 is before the deadline 600");
    assert!(timer.on_frame(&tick(600)), "600 is the deadline");
    assert!(!timer.on_frame(&tick(900)), "fires once only");
    assert!(!timer.is_running());
}

// C#: FrameDelayTimerTests.Start_WhileRunning_ReplacesTheDeadline
#[test]
fn start_while_running_replaces_the_deadline() {
    let mut timer = timer();
    timer.start(0, 500 * TICKS_PER_MS).expect("first start");
    let restarted = timer.start(400, 500 * TICKS_PER_MS).expect("second start");
    assert!(!restarted.newly_running, "already running: only the deadline changes");

    assert!(!timer.on_frame(&tick(500)), "old deadline 500 is replaced by 900");
    assert!(timer.on_frame(&tick(900)));
    assert!(!timer.is_running());
}

// C#: FrameDelayTimerTests.Stop_CancelsAndReleasesTheClock_AndStartRequestsAFrame
// The "start requests a frame" half is `FrameDelayStart` in the Rust return value (the host requests the frame).
#[test]
fn stop_cancels_and_releases_the_clock_and_start_requests_a_frame() {
    let mut timer = timer();
    let started = timer.start(0, 10 * TICKS_PER_MS).expect("start");
    assert!(started.newly_running, "the host subscribes to the clock now");
    assert!(timer.is_running());

    timer.stop();
    assert!(!timer.is_running(), "the host unsubscribes after stop");
    assert!(!timer.on_frame(&tick(1_000_000)), "a stopped timer never fires");
    timer.stop(); // idempotent
    assert!(!timer.is_running());
}

// C#: FrameDelayTimerTests.CallbackMayRestartTheTimer
// Host logic: the elapsed action runs after `on_frame` returns `true`, and it may call `start` again.
#[test]
fn callback_may_restart_the_timer() {
    let mut timer = timer();
    timer.start(0, 100 * TICKS_PER_MS).expect("start");
    let mut fired = 0;
    for t in (100..=400).step_by(100) {
        if timer.on_frame(&tick(t)) {
            fired += 1;
            if fired < 3 {
                timer.start(t, 100 * TICKS_PER_MS).expect("restart from the callback");
            }
        }
    }

    assert_eq!(fired, 3);
}

// C#: FrameDelayTimerTests.NegativeDelayFiresOnTheNextFrame_DisposedTimerRefusesToStart
// The `ArgumentOutOfRangeException` half: `FrameDelayTimer::new(0)` is `InvalidTicksPerSecond(0)`.
#[test]
fn negative_delay_fires_on_the_next_frame_disposed_timer_refuses_to_start() {
    let mut timer = timer();
    timer.start(0, -5 * TICKS_PER_MS).expect("negative delay is accepted");
    assert!(timer.on_frame(&tick(0)), "a negative delay fires on the next frame");

    timer.dispose();
    assert_eq!(
        timer.start(0, TICKS_PER_MS),
        Err(FrameDelayError::Disposed),
        "a disposed timer refuses to start"
    );
    assert_eq!(
        FrameDelayTimer::new(0).err(),
        Some(FrameDelayError::InvalidTicksPerSecond(0)),
        "zero ticks per second is rejected"
    );
}

// new: a timer that was never started never fires and reports not running.
#[test]
fn on_frame_when_not_running_is_false() {
    let mut timer = timer();

    assert!(!timer.is_running());
    assert!(!timer.on_frame(&tick(10_000)));
    assert!(!timer.is_running());
}

// new: a huge delay and a huge `now` must saturate or wrap without panicking (contract Semantics 7: deadline wraps like C# unchecked).
#[test]
fn start_with_huge_delay_saturates_without_panic() {
    let mut timer = timer();

    let result = timer.start(i64::MAX, i64::MAX);

    assert!(result.is_ok(), "start must not fail or panic on extreme inputs");
    assert!(timer.is_running());
}

// new: dispose is idempotent and leaves the timer stopped.
#[test]
fn dispose_is_idempotent() {
    let mut timer = timer();
    timer.start(0, 10 * TICKS_PER_MS).expect("start");

    timer.dispose();
    timer.dispose();

    assert!(!timer.is_running());
    assert!(!timer.on_frame(&tick(1_000_000)));
}

// new: only the first start reports `newly_running`; later starts while running do not.
#[test]
fn start_returns_newly_running_only_on_the_first_start() {
    let mut timer = timer();

    let first = timer.start(0, 10 * TICKS_PER_MS).expect("first start");
    let second = timer.start(1, 10 * TICKS_PER_MS).expect("second start");
    timer.stop();
    let third = timer.start(2, 10 * TICKS_PER_MS).expect("start after stop");

    assert!(first.newly_running);
    assert!(!second.newly_running);
    assert!(third.newly_running, "stopped, so starting again subscribes again");
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/overlay-frame-delay.json (`oracle-cs overlay-frame-delay`, C# d92d6aae).
// ---------------------------------------------------------------------------------------------------------------------

fn run_script_case(case: &Value) {
    let name = str_of(case, "name");
    let mut timer = FrameDelayTimer::new(i64_of(case, "ticks_per_second")).unwrap_or_else(|e| panic!("{name}: {e}"));
    let ops = case["ops"].as_array().expect("ops");
    let results = case["results"].as_array().expect("results");
    assert_eq!(ops.len(), results.len(), "{name}: ops and results differ in length");

    let mut fired: i64 = 0;
    for (index, (op, expected)) in ops.iter().zip(results).enumerate() {
        let step = format!("{name} op #{index} ({})", str_of(op, "op"));
        let mut fired_now = false;
        let mut request_frame = false;
        let threw = expected.get("threw").and_then(Value::as_str);

        match str_of(op, "op") {
            "start" => {
                let outcome = timer.start(i64_of(op, "now"), i64_of(op, "delay_ticks"));
                // Every successful start asks the host for a frame (contract Semantics 7); a refused start requests nothing.
                request_frame = outcome.is_ok();
                if let Err(error) = outcome {
                    assert_eq!(error, FrameDelayError::Disposed, "{step}");
                    assert_eq!(threw, Some("ObjectDisposedException"), "{step}: unexpected error");
                }
            }
            "tick" => {
                if timer.on_frame(&tick(i64_of(op, "timestamp"))) {
                    fired += 1;
                    fired_now = true;
                }
            }
            "stop" => timer.stop(),
            "dispose" => timer.dispose(),
            other => panic!("{name}: unknown op {other}"),
        }

        assert_eq!(timer.is_running(), bool_of(expected, "running"), "{step}: running");
        assert_eq!(fired, i64_of(expected, "fired"), "{step}: fired (cumulative)");
        assert_eq!(fired_now, bool_of(expected, "fired_now"), "{step}: fired_now");
        assert_eq!(
            request_frame,
            bool_of(expected, "request_frame"),
            "{step}: request_frame"
        );
    }
}

#[test]
fn golden_overlay_frame_delay() {
    let golden = golden::load("overlay-frame-delay");
    assert_eq!(golden.generator, "oracle-cs overlay-frame-delay");
    assert_eq!(golden.cases.len(), 10, "overlay-frame-delay golden has 10 cases");
    for case in &golden.cases {
        assert_eq!(
            str_of(case, "kind"),
            "script",
            "every frame-delay golden case is a script"
        );
        run_script_case(case);
    }
}
