//! Red tests (W8-05f) for `pr_app::input::kinetic_pan::GlideFrameClock`.
//!
//! Port of `GlideFrameClockTests.cs` and the `GlideFrameClock` tests in `KineticPanMutationGapTests.cs`.
//! Time base: 10 000 ticks per ms (a 10 MHz QPC), so a 75 Hz refresh is 133 333 ticks. The C# helpers `Period`
//! and `At` use `Math.Round` (ties-to-even, trap 2), so the port uses `f64::round_ties_even`.
//! The bodies are `todo!()`, so every test fails until the port lands.

use pr_app::input::kinetic_pan::{GLIDE_PRESENT_LEAD_MS, GlideFrameClock};
use pr_core::abstractions::DisplayTiming;
use pr_core::model::enums::KineticGlideSmoothing;

const TICKS_PER_MS: f64 = 10_000.0;
const REFRESH_RATES: [f64; 4] = [60.0, 75.0, 144.0, 240.0];

/// xUnit `Assert.Equal(expected, actual, decimals)`: both values rounded to `decimals` must be equal.
fn assert_decimals(actual: f64, expected: f64, decimals: i32, what: &str) {
    let tolerance = 0.5 * 10f64.powi(-decimals);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected} (to {decimals} decimals), got {actual}"
    );
}

/// C# `Period(hz)`: one refresh in ticks, `(long)Math.Round(1000 * TicksPerMs / hz)` (ties to even).
fn period(hz: f64) -> i64 {
    (1000.0 * TICKS_PER_MS / hz).round_ties_even() as i64
}

/// C# `At(refreshes, hz)`: a time in ticks, `(long)Math.Round(refreshes * Period(hz))` (ties to even).
fn at(refreshes: f64, hz: f64) -> i64 {
    (refreshes * period(hz) as f64).round_ties_even() as i64
}

/// C# `ExpectedRefresh`: the callback time (in refreshes) plus the present lead, rounded up.
fn expected_refresh(refreshes: f64, hz: f64) -> i64 {
    (refreshes + GLIDE_PRESENT_LEAD_MS * hz / 1000.0).ceil() as i64
}

/// One refresh of `hz` in milliseconds (`Period(hz) / TicksPerMs`).
fn period_ms(hz: f64) -> f64 {
    period(hz) as f64 / TICKS_PER_MS
}

// C#: GlideFrameClockTests.Predict_EveryStepIsAWholeNumberOfRefreshes_AndTheTotalIsTheRefreshesPassed (theory: 60, 75, 144, 240 Hz)
#[test]
fn predict_every_step_is_a_whole_number_of_refreshes_and_the_total_is_the_refreshes_passed() {
    for hz in REFRESH_RATES {
        let timing = DisplayTiming {
            last_vblank: 0,
            refresh_period: period(hz),
        };
        let mut clock = GlideFrameClock::new();
        clock.start(KineticGlideSmoothing::Predict, TICKS_PER_MS);
        let callbacks = [0.2, 0.5, 0.9, 1.3, 1.4, 1.6, 4.3, 4.4, 5.2, 7.9, 8.1, 11.6];
        let periods = period_ms(hz);

        assert_eq!(
            clock.advance(1.0, at(callbacks[0], hz), Some(timing)),
            0.0,
            "{hz} Hz: anchors"
        );
        let mut total = 0.0;
        for i in 1..callbacks.len() {
            // RenderingTime is ignored once anchored.
            let step = clock.advance((1 + i) as f64, at(callbacks[i], hz), Some(timing));
            let expected =
                (expected_refresh(callbacks[i], hz) - expected_refresh(callbacks[i - 1], hz)) as f64 * periods;
            assert_decimals(step, expected, 6, &format!("{hz} Hz: step {i}"));
            total += step;
        }
        let whole = (expected_refresh(callbacks[callbacks.len() - 1], hz) - expected_refresh(callbacks[0], hz)) as f64;
        assert_decimals(total, whole * periods, 6, &format!("{hz} Hz: total"));
    }
}

// C#: GlideFrameClockTests.Predict_ALateFrameJumpsToTheRefreshItIsSeenOn (theory: 60, 75, 144, 240 Hz)
#[test]
fn predict_a_late_frame_jumps_to_the_refresh_it_is_seen_on() {
    for hz in REFRESH_RATES {
        let timing = DisplayTiming {
            last_vblank: 0,
            refresh_period: period(hz),
        };
        let mut clock = GlideFrameClock::new();
        clock.start(KineticGlideSmoothing::Predict, TICKS_PER_MS);
        let periods = period_ms(hz);

        clock.advance(0.0, at(0.1, hz), Some(timing));
        assert_decimals(
            clock.advance(0.0, at(1.1, hz), Some(timing)),
            periods,
            6,
            &format!("{hz} Hz: on time"),
        );
        assert_decimals(
            clock.advance(0.0, at(4.1, hz), Some(timing)),
            3.0 * periods,
            6,
            &format!("{hz} Hz: two refreshes missed"),
        );
        assert_eq!(
            clock.advance(0.0, at(4.15, hz), Some(timing)),
            0.0,
            "{hz} Hz: the same refresh again moves nothing"
        );
    }
}

// C#: GlideFrameClockTests.Predict_UsesTheLatestVBlankEstimate_NotTheOneItStartedWith (theory: 60, 75, 144, 240 Hz)
#[test]
fn predict_uses_the_latest_vblank_estimate_not_the_one_it_started_with() {
    for hz in REFRESH_RATES {
        let period_ticks = period(hz);
        let mut clock = GlideFrameClock::new();
        clock.start(KineticGlideSmoothing::Predict, TICKS_PER_MS);

        clock.advance(
            0.0,
            at(0.1, hz),
            Some(DisplayTiming {
                last_vblank: 0,
                refresh_period: period_ticks,
            }),
        );
        // The estimate moved on (a later LastVBlank on the same grid): still one refresh per refresh.
        assert_decimals(
            clock.advance(
                0.0,
                at(1.1, hz),
                Some(DisplayTiming {
                    last_vblank: at(1.0, hz),
                    refresh_period: period_ticks,
                }),
            ),
            period_ms(hz),
            6,
            &format!("{hz} Hz: after one refresh"),
        );
        assert_decimals(
            clock.advance(
                0.0,
                at(2.1, hz),
                Some(DisplayTiming {
                    last_vblank: at(2.0, hz),
                    refresh_period: period_ticks,
                }),
            ),
            period_ms(hz),
            6,
            &format!("{hz} Hz: after two refreshes"),
        );
    }
}

// C#: GlideFrameClockTests.Predict_WithoutTimingYet_StepsByRenderingTime_ThenLocksToTheRefreshGrid
#[test]
fn predict_without_timing_yet_steps_by_rendering_time_then_locks_to_the_refresh_grid() {
    let mut clock = GlideFrameClock::new();
    clock.start(KineticGlideSmoothing::Predict, TICKS_PER_MS);

    assert_eq!(clock.advance(100.0, 0, None), 0.0);
    assert_decimals(
        clock.advance(107.0, at(0.4, 60.0), None),
        7.0,
        6,
        "like Off, no timing yet",
    );
    // Anchors here, like Off.
    assert_decimals(
        clock.advance(
            116.0,
            at(0.9, 60.0),
            Some(DisplayTiming {
                last_vblank: 0,
                refresh_period: period(60.0),
            }),
        ),
        9.0,
        6,
        "anchors on the first timed callback",
    );
    assert_decimals(
        clock.advance(
            117.0,
            at(1.9, 60.0),
            Some(DisplayTiming {
                last_vblank: 0,
                refresh_period: period(60.0),
            }),
        ),
        period_ms(60.0),
        6,
        "then one refresh per refresh",
    );
}

// C#: GlideFrameClockTests.Predict_TimingLostThenRecoveredAtTheSamePeriod_DoesNotCountTheLostIntervalTwice
// APP-P02: timing lost while anchored and back at the same period must not replay the frames already stepped by RenderingTime.
#[test]
fn predict_timing_lost_then_recovered_at_the_same_period_does_not_count_the_lost_interval_twice() {
    let mut clock = GlideFrameClock::new();
    clock.start(KineticGlideSmoothing::Predict, TICKS_PER_MS);
    let timing = DisplayTiming {
        last_vblank: 0,
        refresh_period: period(60.0),
    };
    let periods = period_ms(60.0);

    clock.advance(0.0, at(0.1, 60.0), Some(timing)); // anchors
    assert_decimals(
        clock.advance(16.0, at(1.1, 60.0), Some(timing)),
        periods,
        6,
        "one refresh on the grid",
    );
    assert_decimals(
        clock.advance(33.0, at(2.1, 60.0), None),
        17.0,
        6,
        "timing lost: by RenderingTime",
    );
    assert_decimals(
        clock.advance(50.0, at(3.1, 60.0), None),
        17.0,
        6,
        "still lost: by RenderingTime",
    );
    assert_decimals(
        clock.advance(66.0, at(4.1, 60.0), Some(timing)),
        16.0,
        6,
        "same period again: re-anchors on this frame",
    );
    assert_decimals(
        clock.advance(67.0, at(5.1, 60.0), Some(timing)),
        periods,
        6,
        "then one refresh per refresh",
    );
}

// C#: GlideFrameClockTests.Predict_MonitorChangeFrom60To75Hz_ReanchorsOnTheNewGrid
#[test]
fn predict_monitor_change_from_60_to_75_hz_reanchors_on_the_new_grid() {
    let mut clock = GlideFrameClock::new();
    clock.start(KineticGlideSmoothing::Predict, TICKS_PER_MS);
    let sixty = DisplayTiming {
        last_vblank: 0,
        refresh_period: period(60.0),
    };
    clock.advance(0.0, at(0.1, 60.0), Some(sixty));
    clock.advance(16.0, at(1.1, 60.0), Some(sixty));

    // The window is now on a 75 Hz monitor with another phase: one frame by RenderingTime, then 75 Hz refreshes.
    let start = at(2.1, 60.0);
    let seventy_five = DisplayTiming {
        last_vblank: start - 30_000,
        refresh_period: period(75.0),
    };
    assert_decimals(
        clock.advance(30.0, start, Some(seventy_five)),
        14.0,
        6,
        "one frame by RenderingTime",
    );
    assert_decimals(
        clock.advance(31.0, start + period(75.0), Some(seventy_five)),
        period_ms(75.0),
        6,
        "one 75 Hz refresh",
    );
    assert_decimals(
        clock.advance(32.0, start + 3 * period(75.0), Some(seventy_five)),
        2.0 * period_ms(75.0),
        6,
        "two 75 Hz refreshes",
    );
}

// C#: GlideFrameClockTests.Off_StepsByRenderingTime_AndIgnoresTheTiming
#[test]
fn off_steps_by_rendering_time_and_ignores_the_timing() {
    let mut clock = GlideFrameClock::new();
    clock.start(KineticGlideSmoothing::Off, TICKS_PER_MS);
    let timing = DisplayTiming {
        last_vblank: 0,
        refresh_period: period(60.0),
    };

    assert!(!clock.needs_display_timing());
    assert_eq!(clock.advance(100.0, 0, Some(timing)), 0.0);
    assert_eq!(
        clock.advance(100.0, at(3.0, 60.0), Some(timing)),
        0.0,
        "repeated RenderingTime is not a new frame"
    );
    assert_decimals(
        clock.advance(105.0, at(9.0, 60.0), Some(timing)),
        5.0,
        6,
        "a new rendering time steps by its delta",
    );
}

// C#: GlideFrameClockTests.UndefinedMode_BehavesLikeOff
// Not ported (D-W8-5f-1): an undefined enum value cannot be built in Rust. See `kinetic_pan.rs` module doc.

// ---- KineticPanMutationGapTests: GlideFrameClock ----

// C#: KineticPanMutationGapTests.Start_ZeroTicksPerMillisecond_FallsBackToOffSmoothing
#[test]
fn start_zero_ticks_per_millisecond_falls_back_to_off_smoothing() {
    let mut clock = GlideFrameClock::new();

    clock.start(KineticGlideSmoothing::Predict, 0.0);

    assert!(!clock.needs_display_timing());
}

// C#: KineticPanMutationGapTests.Advance_RefreshPeriodChangedByOnePercent_KeepsTheAnchoredGridInsteadOfReAnchoring
#[test]
fn advance_refresh_period_changed_by_one_percent_keeps_the_anchored_grid_instead_of_re_anchoring() {
    let ticks_per_ms = 1000.0;
    let mut clock = GlideFrameClock::new();
    clock.start(KineticGlideSmoothing::Predict, ticks_per_ms);
    assert_eq!(
        clock.advance(
            0.0,
            0,
            Some(DisplayTiming {
                last_vblank: 0,
                refresh_period: 16_667
            })
        ),
        0.0
    ); // anchors the 16667-tick grid

    let advanced = clock.advance(
        16.0,
        16_667,
        Some(DisplayTiming {
            last_vblank: 0,
            refresh_period: 16_833,
        }),
    ); // +1 %: within the 2 % tolerance

    // One refresh of the NEW period, not RenderingTime's 16 ms.
    assert_decimals(advanced, 16.833, 6, "one refresh of the new period");
}
