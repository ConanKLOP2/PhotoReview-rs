//! Red tests (W8-05f) for `pr_app::input::kinetic_pan::PanVelocityTracker`.
//!
//! Port of `tests/PhotoReview.App.Tests/Input/KineticPanTests.cs` (`Velocity_*`) and
//! `KineticPanMutationGapTests.cs` (`GetVelocity_*`). The bodies are `todo!()`, so every test fails until the port lands.

use pr_app::input::kinetic_pan::{PAN_MIN_SPAN_MS, PAN_TRACKER_CAPACITY, PanVelocityTracker};

/// xUnit `Assert.Equal(expected, actual, decimals)`: both values rounded to `decimals` must be equal.
fn assert_decimals(actual: f64, expected: f64, decimals: i32, what: &str) {
    let tolerance = 0.5 * 10f64.powi(-decimals);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected} (to {decimals} decimals), got {actual}"
    );
}

// C#: KineticPanTests.Velocity_UniformDrag_IsTheDragSpeed
#[test]
fn velocity_uniform_drag_is_the_drag_speed() {
    let mut tracker = PanVelocityTracker::new();
    for i in 0..=20 {
        let t = f64::from(i) * 10.0;
        tracker.add(t, t * 2.0, -t * 0.5);
    }

    let (x, y) = tracker.velocity();

    assert_decimals(x, 2.0, 6, "vx");
    assert_decimals(y, -0.5, 6, "vy");
}

// C#: KineticPanTests.Velocity_UsesOnlyTheLastWindowBeforeRelease
#[test]
fn velocity_uses_only_the_last_window_before_release() {
    let mut tracker = PanVelocityTracker::new();
    // Fast early movement, then slow for the last 100 ms: only the slow part counts.
    for i in 0..=10 {
        let t = f64::from(i) * 10.0;
        tracker.add(t, t * 10.0, 0.0);
    }
    for i in 11..=20 {
        let t = f64::from(i) * 10.0;
        tracker.add(t, 1000.0 + (t - 100.0) * 1.0, 0.0);
    }

    let (x, _) = tracker.velocity();

    assert_decimals(x, 1.0, 6, "vx");
}

// C#: KineticPanTests.Velocity_PointerRestedLongerThanTheWindowBeforeRelease_IsZero
#[test]
fn velocity_pointer_rested_longer_than_the_window_before_release_is_zero() {
    let mut tracker = PanVelocityTracker::new();
    for i in 0..=10 {
        let t = f64::from(i) * 10.0;
        tracker.add(t, t * 3.0, 0.0);
    }
    tracker.add(300.0, 300.0, 0.0); // release sample, same position, 200 ms later

    assert_eq!(tracker.velocity(), (0.0, 0.0));
}

// C#: KineticPanTests.Velocity_ShortPauseBeforeRelease_DilutesTheSpeed
#[test]
fn velocity_short_pause_before_release_dilutes_the_speed() {
    let mut tracker = PanVelocityTracker::new();
    for i in 0..=6 {
        let t = f64::from(i) * 10.0;
        tracker.add(t, t * 2.0, 0.0);
    }
    // Rested 20 ms before release: 120 DIP over the 80 ms window instead of 2 DIP/ms.
    tracker.add(80.0, 120.0, 0.0);

    let (x, _) = tracker.velocity();

    assert_decimals(x, 120.0 / 80.0, 6, "vx");
    assert!(x < 2.0, "a pause must lower the speed below 2 DIP/ms, got {x}");
}

// C#: KineticPanTests.Velocity_TooFewOrTooCloseSamples_IsZero
#[test]
fn velocity_too_few_or_too_close_samples_is_zero() {
    let mut tracker = PanVelocityTracker::new();
    assert_eq!(tracker.velocity(), (0.0, 0.0));
    tracker.add(0.0, 0.0, 0.0);
    tracker.add(3.0, 30.0, 0.0);
    assert_eq!(tracker.velocity(), (0.0, 0.0));
}

// C#: KineticPanTests.Velocity_RingBufferWrapsWithoutLosingTheNewestSamples
#[test]
fn velocity_ring_buffer_wraps_without_losing_the_newest_samples() {
    let mut tracker = PanVelocityTracker::new();
    for i in 0..(PAN_TRACKER_CAPACITY * 3) {
        let t = i as f64;
        tracker.add(t * 5.0, t * 5.0 * 1.5, 0.0);
    }

    assert_eq!(tracker.count(), PAN_TRACKER_CAPACITY);
    assert_decimals(tracker.velocity().0, 1.5, 6, "vx");
}

// C#: KineticPanMutationGapTests.GetVelocity_ExactlyTwoSamples_MeasuresTheirVelocity
#[test]
fn get_velocity_exactly_two_samples_measures_their_velocity() {
    let mut tracker = PanVelocityTracker::new();
    tracker.add(0.0, 100.0, 50.0);
    tracker.add(20.0, 140.0, 50.0);

    let (x, y) = tracker.velocity();

    assert_decimals(x, 2.0, 9, "vx");
    assert_decimals(y, 0.0, 9, "vy");
}

// C#: KineticPanMutationGapTests.GetVelocity_SpanExactlyTheMinimum_StillMeasuresAVelocity
#[test]
fn get_velocity_span_exactly_the_minimum_still_measures_a_velocity() {
    let mut tracker = PanVelocityTracker::new();
    tracker.add(0.0, 0.0, 0.0);
    tracker.add(PAN_MIN_SPAN_MS, 16.0, 8.0);

    let (x, y) = tracker.velocity();

    assert_decimals(x, 2.0, 9, "vx");
    assert_decimals(y, 1.0, 9, "vy");
}

// C#: KineticPanMutationGapTests.GetVelocity_SpanJustBelowTheMinimum_IsZero
#[test]
fn get_velocity_span_just_below_the_minimum_is_zero() {
    let mut tracker = PanVelocityTracker::new();
    tracker.add(0.0, 0.0, 0.0);
    tracker.add(PAN_MIN_SPAN_MS - 0.5, 16.0, 8.0);

    assert_eq!(tracker.velocity(), (0.0, 0.0));
}

// C#: KineticPanMutationGapTests.GetVelocity_FullRingWithEverySampleInsideTheWindow_UsesTheOldestOne
#[test]
fn get_velocity_full_ring_with_every_sample_inside_the_window_uses_the_oldest_one() {
    let mut tracker = PanVelocityTracker::new();
    for i in 0..PAN_TRACKER_CAPACITY {
        let t = i as f64;
        tracker.add(t, t * 3.0, 0.0); // 3 DIP per ms, all within 31 ms
    }

    let (x, _) = tracker.velocity();

    assert_decimals(x, 3.0, 9, "vx");
}
