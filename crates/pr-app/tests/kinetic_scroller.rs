//! Red tests (W8-05f) for `pr_app::input::kinetic_pan::KineticScroller` and the `ImpulseVelocity` helper.
//!
//! Port of `KineticPanTests.cs` (`Start_*`, glide `Step_*`, `Stop_*`, `AddImpulse_*`, `ImpulseVelocity_*`),
//! `KineticPanMutationGapTests.cs` (`Start_*`, `Step_*` mutation gaps) and `KineticScrollerGapTests.cs`.
//! The bodies are `todo!()`, so every test fails until the port lands.

use pr_app::input::kinetic_pan::{
    KINETIC_MAX_FRAME_MS, KINETIC_MAX_VELOCITY, KINETIC_POINTER_RELEASE_SPEED_FACTOR, KINETIC_START_VELOCITY,
    KINETIC_STOP_VELOCITY, KINETIC_TIME_CONSTANT_MS, KineticScroller, ScrollBounds, keyboard_pan_impulse_velocity,
};

/// xUnit `Assert.Equal(expected, actual, decimals)`: both values rounded to `decimals` must be equal.
fn assert_decimals(actual: f64, expected: f64, decimals: i32, what: &str) {
    let tolerance = 0.5 * 10f64.powi(-decimals);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected} (to {decimals} decimals), got {actual}"
    );
}

fn large() -> ScrollBounds {
    ScrollBounds::new(10_000.0, 10_000.0, 800.0, 600.0)
}

fn gap_bounds() -> ScrollBounds {
    ScrollBounds::new(100_000.0, 100_000.0, 1_000.0, 1_000.0)
}

// ---- Start ----

// C#: KineticPanTests.Start_BelowThreshold_DoesNotGlide
#[test]
fn start_below_threshold_does_not_glide() {
    let mut scroller = KineticScroller::default();

    assert!(!scroller.start(0.05, 0.05));
    assert!(!scroller.is_active());
}

// C#: KineticPanTests.Start_ScrollVelocityIsOppositeThePointerAndCapped
#[test]
fn start_scroll_velocity_is_opposite_the_pointer_and_capped() {
    let mut scroller = KineticScroller::default();
    let factor = KINETIC_POINTER_RELEASE_SPEED_FACTOR;

    assert!(scroller.start(2.0, -100.0));

    assert_decimals(scroller.velocity_x(), -2.0 * factor, 6, "vx");
    assert_decimals(scroller.velocity_y(), KINETIC_MAX_VELOCITY, 6, "vy (capped)");

    assert!(scroller.start(100.0, 1.0));
    assert_decimals(scroller.velocity_x(), -KINETIC_MAX_VELOCITY, 6, "vx (capped)");
    assert_decimals(scroller.velocity_y(), -factor, 6, "vy");
}

// C#: KineticPanTests.Start_DampsTheRawPointerVelocityByPointerReleaseSpeedFactor
#[test]
fn start_damps_the_raw_pointer_velocity_by_pointer_release_speed_factor() {
    let mut scroller = KineticScroller::default();
    let raw_velocity = 1.2; // realistic flick, DIP/ms, well under MaxVelocity so the factor is visible

    assert!(scroller.start(raw_velocity, 0.0));

    assert_decimals(
        scroller.velocity_x(),
        -raw_velocity * KINETIC_POINTER_RELEASE_SPEED_FACTOR,
        9,
        "vx",
    );
    assert!(
        (0.5..=0.7).contains(&KINETIC_POINTER_RELEASE_SPEED_FACTOR),
        "the documented, chosen range is 0.5 to 0.7"
    );
}

// C#: KineticPanTests.Start_RealisticFlick_StillStartsAGlideAfterDamping
#[test]
fn start_realistic_flick_still_starts_a_glide_after_damping() {
    let mut scroller = KineticScroller::default();
    // A moderate flick (well above StartVelocity even after damping) still starts gliding.
    let raw_velocity = 0.5;

    assert!(scroller.start(raw_velocity, 0.0));
    assert!(scroller.is_active());
    assert!(scroller.velocity_x().abs() >= KINETIC_START_VELOCITY);
}

// C#: KineticPanMutationGapTests.Start_ReleaseSpeedExactlyTheStartVelocity_Glides
#[test]
fn start_release_speed_exactly_the_start_velocity_glides() {
    let mut scroller = KineticScroller::default();

    // -pointer * PointerReleaseSpeedFactor (0.65) is exactly 0.1 for this value in IEEE double arithmetic.
    assert!(scroller.start(-0.153_846_153_846_153_85, 0.0));
    assert!(scroller.is_active());
    assert_eq!(scroller.velocity_x(), KINETIC_START_VELOCITY);
}

// C#: KineticPanMutationGapTests.Start_DiagonalReleaseWhoseComponentsAreEachBelowTheThresholdButWhoseSpeedIsNot_Glides
#[test]
fn start_diagonal_release_whose_components_are_each_below_the_threshold_but_whose_speed_is_not_glides() {
    let mut scroller = KineticScroller::default();
    let component = -0.08 / KINETIC_POINTER_RELEASE_SPEED_FACTOR;

    assert!(scroller.start(component, component));
    assert!(scroller.is_active());
}

// C#: KineticPanTests.AddImpulse_IsNotDampedByPointerReleaseSpeedFactor
#[test]
fn add_impulse_is_not_damped_by_pointer_release_speed_factor() {
    // Regression guard (Q-R40): AddImpulse (keyboard arrow-key panning) is not damped by the mouse-release factor.
    let mut scroller = KineticScroller::default();
    let velocity_x = 1.5;
    let velocity_y = -0.5;

    scroller.add_impulse(velocity_x, velocity_y);

    assert_decimals(scroller.velocity_x(), velocity_x, 9, "vx");
    assert_decimals(scroller.velocity_y(), velocity_y, 9, "vy");
}

// ---- glide Step ----

// C#: KineticPanTests.Step_MovesWithVelocityAndDecaysExponentially
#[test]
fn step_moves_with_velocity_and_decays_exponentially() {
    let mut scroller = KineticScroller::default();
    scroller.start(-1.0, 0.0); // pointer moved left -> content scrolls right, damped
    let v0 = KINETIC_POINTER_RELEASE_SPEED_FACTOR; // effective scroll velocity after Start's damping

    let (h, v) = scroller.step(16.0, 1000.0, 500.0, large());

    let tau = KINETIC_TIME_CONSTANT_MS;
    assert_decimals(h, 1000.0 + v0 * tau * (1.0 - (-16.0 / tau).exp()), 6, "h");
    assert_decimals(v, 500.0, 6, "v");
    assert_decimals(scroller.velocity_x(), v0 * (-16.0 / tau).exp(), 6, "vx after the frame");
}

// C#: KineticPanTests.Step_DistanceIsIndependentOfFrameRate
#[test]
fn step_distance_is_independent_of_frame_rate() {
    let mut at60 = KineticScroller::default();
    let mut at144 = KineticScroller::default();
    at60.start(-3.0, 0.0);
    at144.start(-3.0, 0.0);
    let mut h60 = 1000.0;
    let mut h144 = 1000.0;

    for _ in 0..60 {
        h60 = at60.step(1000.0 / 60.0, h60, 0.0, large()).0;
    }
    for _ in 0..144 {
        h144 = at144.step(1000.0 / 144.0, h144, 0.0, large()).0;
    }

    assert_decimals(h60, h144, 3, "distance after one second at 60 and at 144 Hz");
}

// C#: KineticPanTests.Step_GlideDecelerates_ThenStopsNearTheTotalDistance
#[test]
fn step_glide_decelerates_then_stops_near_the_total_distance() {
    let mut scroller = KineticScroller::default();
    scroller.start(-2.0, 0.0);
    let mut h = 1000.0;
    let mut last_step = f64::MAX;
    let mut frames = 0;

    while scroller.is_active() && frames < 1000 {
        let next = scroller.step(16.0, h, 0.0, large()).0;
        let step = next - h;
        assert!(step <= last_step + 1e-9, "the glide must never speed up");
        last_step = step;
        h = next;
        frames += 1;
    }

    assert!(!scroller.is_active());
    assert!((10..=200).contains(&frames), "frames {frames} outside 10..=200");
    // Total distance of an unbounded exponential glide is v0 * tau; stopping at StopVelocity leaves at most StopVelocity * tau.
    let full = 2.0 * KINETIC_POINTER_RELEASE_SPEED_FACTOR * KINETIC_TIME_CONSTANT_MS;
    let low = full - KINETIC_STOP_VELOCITY * KINETIC_TIME_CONSTANT_MS - 1e-6;
    let distance = h - 1000.0;
    assert!(
        (low..=full).contains(&distance),
        "distance {distance} outside {low}..={full}"
    );
}

// C#: KineticPanTests.Step_ReachingAnEdge_StopsThatAxisOnly
#[test]
fn step_reaching_an_edge_stops_that_axis_only() {
    let bounds = ScrollBounds::new(2000.0, 2000.0, 800.0, 600.0); // max offsets 1200 x 1400
    let mut scroller = KineticScroller::default();
    scroller.start(-8.0, -1.0); // content scrolls right fast and down slowly

    let (h, v) = scroller.step(100.0, 1190.0, 100.0, bounds);

    assert_decimals(h, 1200.0, 6, "h clamped at the right edge");
    assert_eq!(scroller.velocity_x(), 0.0, "that axis stopped");
    assert!(v > 100.0);
    assert!(scroller.velocity_y() > 0.0, "the other axis keeps gliding");
    assert!(scroller.is_active());
}

// C#: KineticPanTests.Step_AtTheEdgeAlready_StopsTheGlide
#[test]
fn step_at_the_edge_already_stops_the_glide() {
    let mut scroller = KineticScroller::default();
    scroller.start(1.0, 0.0); // content scrolls left, but it is already at 0

    let (h, _) = scroller.step(16.0, 0.0, 0.0, large());

    assert_eq!(h, 0.0);
    assert!(!scroller.is_active());
}

// C#: KineticPanTests.Step_LongFrameGap_IsCapped
#[test]
fn step_long_frame_gap_is_capped() {
    let mut scroller = KineticScroller::default();
    scroller.start(-1.0, 0.0);
    let mut capped = KineticScroller::default();
    capped.start(-1.0, 0.0);

    let after_gap = scroller.step(5000.0, 0.0, 0.0, large()).0;
    let after_max = capped.step(KINETIC_MAX_FRAME_MS, 0.0, 0.0, large()).0;

    assert_decimals(after_gap, after_max, 6, "h after a 5 s gap");
}

// C#: KineticPanTests.Stop_EndsTheGlideImmediately
#[test]
fn stop_ends_the_glide_immediately() {
    let mut scroller = KineticScroller::default();
    scroller.start(-3.0, -3.0);

    scroller.stop();
    let (h, v) = scroller.step(16.0, 50.0, 60.0, large());

    assert!(!scroller.is_active());
    assert_eq!((h, v), (50.0, 60.0));
}

// C#: KineticPanMutationGapTests.Step_VerticalVelocity_DecaysByTheFrictionFactor
#[test]
fn step_vertical_velocity_decays_by_the_friction_factor() {
    let mut scroller = KineticScroller::default();
    scroller.add_impulse(0.0, 1.0);
    let decay = (-16.0 / KINETIC_TIME_CONSTANT_MS).exp();

    scroller.step(16.0, 0.0, 0.0, large());

    assert_decimals(scroller.velocity_y(), decay, 9, "vy");
}

// C#: KineticPanMutationGapTests.Step_HorizontalSpeedExactlyTheStopVelocityAfterAZeroLengthFrame_KeepsGliding
#[test]
fn step_horizontal_speed_exactly_the_stop_velocity_after_a_zero_length_frame_keeps_gliding() {
    let mut scroller = KineticScroller::default();
    scroller.add_impulse(KINETIC_STOP_VELOCITY, 0.0);

    scroller.step(0.0, 0.0, 0.0, large());

    assert!(scroller.is_active());
    assert_eq!(scroller.velocity_x(), KINETIC_STOP_VELOCITY);
}

// C#: KineticPanMutationGapTests.Step_VerticalSpeedExactlyTheStopVelocityAfterAZeroLengthFrame_KeepsGliding
#[test]
fn step_vertical_speed_exactly_the_stop_velocity_after_a_zero_length_frame_keeps_gliding() {
    let mut scroller = KineticScroller::default();
    scroller.add_impulse(0.0, KINETIC_STOP_VELOCITY);

    scroller.step(0.0, 0.0, 0.0, large());

    assert!(scroller.is_active());
    assert_eq!(scroller.velocity_y(), KINETIC_STOP_VELOCITY);
}

// ---- KineticScrollerGapTests ----

// C#: KineticScrollerGapTests.Step_NonFiniteOrNegativeElapsed_DoesNotMoveOrDecayTheGlide (theory: NaN, +Inf, -Inf, -50)
#[test]
fn step_non_finite_or_negative_elapsed_does_not_move_or_decay_the_glide() {
    for elapsed_ms in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -50.0] {
        let mut scroller = KineticScroller::default();
        scroller.start(-2.0, 1.0);
        let (vx, vy) = (scroller.velocity_x(), scroller.velocity_y());

        let (h, v) = scroller.step(elapsed_ms, 500.0, 400.0, gap_bounds());

        // +Infinity becomes 0 like NaN (Finite()), so the offset never jumps.
        assert_eq!((h, v), (500.0, 400.0), "elapsed {elapsed_ms}");
        assert_decimals(scroller.velocity_x(), vx, 9, "vx");
        assert_decimals(scroller.velocity_y(), vy, 9, "vy");
        assert!(scroller.is_active(), "elapsed {elapsed_ms}");
    }
}

// C#: KineticScrollerGapTests.Step_NonFiniteStartOffset_IsTreatedAsZero
#[test]
fn step_non_finite_start_offset_is_treated_as_zero() {
    let mut scroller = KineticScroller::default();
    scroller.start(-2.0, 0.0);

    let (h, _) = scroller.step(16.0, f64::NAN, 0.0, gap_bounds());

    assert!(h.is_finite());
    assert!(h > 0.0);
}

// ---- AddImpulse ----

// C#: KineticPanTests.AddImpulse_WhileIdle_StartsAGlideAtThatVelocity
#[test]
fn add_impulse_while_idle_starts_a_glide_at_that_velocity() {
    let mut scroller = KineticScroller::default();

    scroller.add_impulse(1.5, -0.5);

    assert!(scroller.is_active());
    assert_decimals(scroller.velocity_x(), 1.5, 6, "vx");
    assert_decimals(scroller.velocity_y(), -0.5, 6, "vy");
}

// C#: KineticPanTests.AddImpulse_WhileGliding_AccumulatesOntoTheCurrentVelocity
#[test]
fn add_impulse_while_gliding_accumulates_onto_the_current_velocity() {
    let mut scroller = KineticScroller::default();
    scroller.add_impulse(1.0, 0.2);

    scroller.add_impulse(0.5, 0.1);

    assert_decimals(scroller.velocity_x(), 1.5, 6, "vx");
    assert_decimals(scroller.velocity_y(), 0.3, 6, "vy");
}

// C#: KineticPanTests.AddImpulse_OppositeDirection_ReducesOrReversesTheVelocity
#[test]
fn add_impulse_opposite_direction_reduces_or_reverses_the_velocity() {
    let mut scroller = KineticScroller::default();
    scroller.add_impulse(2.0, 0.0);

    scroller.add_impulse(-0.5, 0.0);
    assert_decimals(scroller.velocity_x(), 1.5, 6, "vx after the reduction");

    scroller.add_impulse(-3.0, 0.0);
    assert_decimals(scroller.velocity_x(), -1.5, 6, "vx after the reversal");
    assert!(scroller.is_active(), "still gliding, just the other way");
}

// C#: KineticPanTests.AddImpulse_ClampsEachAxisToMaxVelocity
#[test]
fn add_impulse_clamps_each_axis_to_max_velocity() {
    let mut scroller = KineticScroller::default();

    scroller.add_impulse(KINETIC_MAX_VELOCITY * 10.0, -KINETIC_MAX_VELOCITY * 10.0);

    assert_decimals(scroller.velocity_x(), KINETIC_MAX_VELOCITY, 6, "vx");
    assert_decimals(scroller.velocity_y(), -KINETIC_MAX_VELOCITY, 6, "vy");
}

// C#: KineticPanTests.AddImpulse_NonFiniteComponent_IsIgnored_OtherAxisStillApplies
#[test]
fn add_impulse_non_finite_component_is_ignored_other_axis_still_applies() {
    let mut scroller = KineticScroller::default();

    scroller.add_impulse(f64::NAN, 1.0);
    assert_decimals(scroller.velocity_x(), 0.0, 6, "vx after NaN");
    assert_decimals(scroller.velocity_y(), 1.0, 6, "vy after NaN");

    scroller.add_impulse(f64::INFINITY, f64::NEG_INFINITY);
    assert_decimals(scroller.velocity_x(), 0.0, 6, "vx after infinities");
    assert_decimals(scroller.velocity_y(), 1.0, 6, "vy after infinities");
}

// ---- ImpulseVelocity (KeyboardPan.ImpulseVelocity, sized for the kinetic glide) ----

// C#: KineticPanTests.ImpulseVelocity_SignMatchesTheStepDirection
#[test]
fn impulse_velocity_sign_matches_the_step_direction() {
    let (vx, vy) = keyboard_pan_impulse_velocity(-80.0, 60.0);

    assert!(vx < 0.0);
    assert!(vy > 0.0);
}

// C#: KineticPanTests.ImpulseVelocity_DrivenToRest_TravelsTheStepDistance_WithinOneDip
#[test]
fn impulse_velocity_driven_to_rest_travels_the_step_distance_within_one_dip() {
    let step = 80.0;
    let (vx, _) = keyboard_pan_impulse_velocity(step, 0.0);
    let mut scroller = KineticScroller::default();
    scroller.add_impulse(vx, 0.0);
    let mut h = 0.0;

    let mut frames = 0;
    while scroller.is_active() && frames < 10_000 {
        h = scroller.step(16.7, h, 0.0, large()).0;
        frames += 1;
    }

    assert!(
        (1..10_000).contains(&frames),
        "it must stop on its own, frames {frames}"
    );
    assert!((h - step).abs() <= 1.0, "travelled {h}, expected {step} within one DIP");
}
