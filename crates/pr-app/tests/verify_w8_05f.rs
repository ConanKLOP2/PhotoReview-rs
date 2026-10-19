//! Adversarial verification tests for W8-05f (kept out of the commit). Each kills a mutant of the private .NET
//! Math helpers that survived the unit's own tests (signed-zero and negative-operand rules).
use pr_app::input::kinetic_pan::{KineticScroller, ScrollBounds, keyboard_pan_step_pixels};

const NEG_ZERO_BITS: u64 = 0x8000_0000_0000_0000;

// C#: Math.Max(0, -0.0) is +0 (IEEE: +0 beats -0). Kills `delete !` in dotnet_max.
#[test]
fn max_horizontal_of_negative_zero_slack_is_positive_zero() {
    let b = ScrollBounds::new(-0.0, -0.0, 0.0, 0.0);
    assert_eq!(b.max_horizontal().to_bits(), 0);
    assert_eq!(b.max_vertical().to_bits(), 0);
}

// C#: Math.Min(+0, -0) is -0 and Math.Min(-0, +0) is -0. Kills `<` -> `<=` in dotnet_min.
#[test]
fn step_pixels_min_of_signed_zeros_is_negative_zero() {
    assert_eq!(keyboard_pan_step_pixels(0.0, -0.0, 1.0).to_bits(), NEG_ZERO_BITS);
    assert_eq!(keyboard_pan_step_pixels(-0.0, 0.0, 1.0).to_bits(), NEG_ZERO_BITS);
}

// C#: Math.Min(-1, -2) is -2. Kills `&&` -> `||` and `==` -> `!=` in dotnet_min.
#[test]
fn step_pixels_min_of_two_negatives_is_the_smaller() {
    assert_eq!(keyboard_pan_step_pixels(-1.0, -2.0, 1.0), -2.0);
    assert_eq!(keyboard_pan_step_pixels(-2.0, -1.0, 1.0), -2.0);
    assert_eq!(keyboard_pan_step_pixels(3.0, -2.0, 1.0), -2.0);
}

// C#: Math.Clamp(-0.0, 0, max) returns the value (-0.0 < 0 is false). Kills `<` -> `<=` in dotnet_clamp.
#[test]
fn step_keeps_a_negative_zero_offset_when_the_velocity_is_negative_zero() {
    let mut s = KineticScroller::default();
    assert!(s.start(0.0, 1.0)); // vx = -(0) * 0.65 = -0
    assert_eq!(s.velocity_x().to_bits(), NEG_ZERO_BITS);
    let (h, _v) = s.step(16.0, -0.0, 0.0, ScrollBounds::new(1000.0, 1000.0, 100.0, 100.0));
    assert_eq!(h.to_bits(), NEG_ZERO_BITS);
}

use pr_app::input::kinetic_pan::GlideFrameClock;
use pr_core::abstractions::DisplayTiming;
use pr_core::model::enums::KineticGlideSmoothing;

const TICKS_PER_MS: f64 = 10_000.0;

fn timing(last_vblank: i64, refresh_period: i64) -> Option<DisplayTiming> {
    Some(DisplayTiming {
        last_vblank,
        refresh_period,
    })
}

// C#: `timing is { RefreshPeriod: > 0 }` -- a zero period is not usable timing (steps like Off). Kills `>` -> `>=` at the usable filter.
#[test]
fn zero_refresh_period_is_treated_as_no_timing() {
    let mut c = GlideFrameClock::new();
    c.start(KineticGlideSmoothing::Predict, TICKS_PER_MS);
    assert_eq!(c.advance(0.0, 0, timing(0, 0)), 0.0);
    assert_eq!(c.advance(16.0, 160_000, timing(0, 0)), 16.0);
    assert_eq!(c.advance(32.0, 320_000, timing(0, 0)), 16.0);
}

// C#: `if (!(renderingElapsed > 0)) return 0;` -- a rendering time that goes backwards steps nothing (Off and Predict).
// Kills `||` -> `&&` on the NaN / non-positive guard.
#[test]
fn backwards_rendering_time_steps_nothing_and_does_not_move_the_anchor() {
    for mode in [KineticGlideSmoothing::Off, KineticGlideSmoothing::Predict] {
        let mut c = GlideFrameClock::new();
        c.start(mode, TICKS_PER_MS);
        assert_eq!(c.advance(100.0, 0, None), 0.0);
        assert_eq!(c.advance(50.0, 0, None), 0.0, "{mode:?}: backwards time");
        // last_rendering stayed at 100, so 116 is a 16 ms step.
        assert_eq!(c.advance(116.0, 0, None), 16.0, "{mode:?}: after backwards time");
    }
}

// C#: NextVBlank target = now + (long)(PresentLeadMs * ticksPerMs): the lead is a PRODUCT. Kills `*` -> `+`.
#[test]
fn present_lead_is_one_ms_of_ticks_exactly() {
    let period = 100_000; // 10 ms at 10 000 ticks/ms
    let mut c = GlideFrameClock::new();
    c.start(KineticGlideSmoothing::Predict, TICKS_PER_MS);
    assert_eq!(c.advance(0.0, 0, timing(0, period)), 0.0); // anchor_vblank = 100 000
    // now + 10 000 == 100 000 exactly: still the anchored vblank, so no step.
    assert_eq!(c.advance(16.0, 90_000, timing(0, period)), 0.0);
    // one tick later the next vblank (200 000) is the target: one period = 10 ms.
    assert_eq!(c.advance(17.0, 90_001, timing(0, period)), 10.0);
}

// C#: Math.Clamp(-0.0, 0, 0) returns the value (-0.0 > 0 is false). Kills `>` -> `>=` in dotnet_clamp (upper bound).
#[test]
fn step_keeps_a_negative_zero_offset_when_the_scroll_range_is_zero() {
    let mut s = KineticScroller::default();
    assert!(s.start(0.0, 1.0));
    let (h, _v) = s.step(16.0, -0.0, 0.0, ScrollBounds::new(100.0, 1000.0, 100.0, 100.0));
    assert_eq!(h.to_bits(), NEG_ZERO_BITS);
}

// C#: Math.Max(0, ExtentWidth - ViewportWidth) clamps a negative slack to +0 (image smaller than the viewport).
// Kills `||` -> `&&` and `>` -> `==` in dotnet_max.
#[test]
fn max_scroll_is_zero_when_the_extent_is_smaller_than_the_viewport() {
    let b = ScrollBounds::new(50.0, 40.0, 100.0, 100.0);
    assert_eq!(b.max_horizontal().to_bits(), 0);
    assert_eq!(b.max_vertical().to_bits(), 0);
}
