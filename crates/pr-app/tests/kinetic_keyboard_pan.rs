//! Red tests (W8-05f) for the arrow-key pan step: `pr_app::input::kinetic_pan::keyboard_pan_*`.
//!
//! Port of `KineticPanTests.cs` (`ConsumesKey_*`, `IsZoomed_*`, `StepPixels_*`, `Step_*`) and
//! `KineticPanMutationGapTests.cs` (`KeyboardPanStep_*`). C# `KeyboardPan.Step` takes an optional `stepFraction`
//! (default `0.1`), so the Rust calls pass `KEYBOARD_STEP_FRACTION` where C# omits it. The bodies are `todo!()`,
//! so every test fails until the port lands.

use pr_app::input::kinetic_pan::{
    KEYBOARD_STEP_FRACTION, KeyboardPanResult, ScrollBounds, keyboard_pan_consumes_key, keyboard_pan_impulse_velocity,
    keyboard_pan_is_zoomed, keyboard_pan_step, keyboard_pan_step_pixels,
};

/// xUnit `Assert.Equal(expected, actual, decimals)`: both values rounded to `decimals` must be equal.
fn assert_decimals(actual: f64, expected: f64, decimals: i32, what: &str) {
    let tolerance = 0.5 * 10f64.powi(-decimals);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected} (to {decimals} decimals), got {actual}"
    );
}

const FIT: ScrollBounds = ScrollBounds::new(800.0, 600.0, 800.0, 600.0);
const BOTH_AXES: ScrollBounds = ScrollBounds::new(2000.0, 1500.0, 800.0, 600.0);
const HORIZONTAL_ONLY: ScrollBounds = ScrollBounds::new(2000.0, 600.0, 800.0, 600.0);
const VERTICAL_ONLY: ScrollBounds = ScrollBounds::new(800.0, 1500.0, 800.0, 600.0);
/// The bounds the C# `Step` tests share: 2000 x 1500 extent in an 800 x 600 viewport (max 1200 x 900).
const STEP_BOUNDS: ScrollBounds = ScrollBounds::new(2000.0, 1500.0, 800.0, 600.0);

/// C# `Named` helper: the zoom case by name (`Fit`, `both`, `horizontal`, `vertical`).
fn named(name: &str) -> ScrollBounds {
    match name {
        "both" => BOTH_AXES,
        "horizontal" => HORIZONTAL_ONLY,
        "vertical" => VERTICAL_ONLY,
        _ => FIT,
    }
}

const ZOOMED_CASES: [&str; 3] = ["both", "horizontal", "vertical"];

// ---- ConsumesKey ----

// C#: KineticPanTests.ConsumesKey_Default_ZoomedConsumesEveryResult (theory: both, horizontal, vertical)
#[test]
fn consumes_key_default_zoomed_consumes_every_result() {
    for zoom in ZOOMED_CASES {
        for result in [
            KeyboardPanResult::Panned,
            KeyboardPanResult::AtEdge,
            KeyboardPanResult::NotScrollable,
        ] {
            for repeat in [false, true] {
                assert!(
                    keyboard_pan_consumes_key(result, repeat, false, named(zoom)),
                    "zoom {zoom}, result {result:?}, repeat {repeat}"
                );
            }
        }
    }
}

// C#: KineticPanTests.ConsumesKey_Default_AtFitFallsThrough
#[test]
fn consumes_key_default_at_fit_falls_through() {
    for repeat in [false, true] {
        assert!(!keyboard_pan_consumes_key(
            KeyboardPanResult::NotScrollable,
            repeat,
            false,
            FIT
        ));
    }
}

// C#: KineticPanTests.ConsumesKey_Legacy_MatchesThePreviousBehaviour (theory: both, horizontal, vertical)
#[test]
fn consumes_key_legacy_matches_the_previous_behaviour() {
    for zoom in ZOOMED_CASES {
        let bounds = named(zoom);
        assert!(
            keyboard_pan_consumes_key(KeyboardPanResult::Panned, false, true, bounds),
            "zoom {zoom}"
        );
        assert!(
            keyboard_pan_consumes_key(KeyboardPanResult::Panned, true, true, bounds),
            "zoom {zoom}"
        );
        assert!(
            !keyboard_pan_consumes_key(KeyboardPanResult::AtEdge, false, true, bounds),
            "zoom {zoom}"
        );
        assert!(
            keyboard_pan_consumes_key(KeyboardPanResult::AtEdge, true, true, bounds),
            "zoom {zoom}"
        );
        assert!(
            !keyboard_pan_consumes_key(KeyboardPanResult::NotScrollable, false, true, bounds),
            "zoom {zoom}"
        );
        assert!(
            !keyboard_pan_consumes_key(KeyboardPanResult::NotScrollable, true, true, bounds),
            "zoom {zoom}"
        );
    }
}

// C#: KineticPanTests.IsZoomed_TrueOnEitherAxis_FalseAtFit
#[test]
fn is_zoomed_true_on_either_axis_false_at_fit() {
    assert!(!keyboard_pan_is_zoomed(FIT));
    assert!(keyboard_pan_is_zoomed(HORIZONTAL_ONLY));
    assert!(keyboard_pan_is_zoomed(VERTICAL_ONLY));
    assert!(keyboard_pan_is_zoomed(BOTH_AXES));
}

// ---- StepPixels ----

// C#: KineticPanTests.StepPixels_IsTheShortSideTimesFraction (4 rows)
#[test]
fn step_pixels_is_the_short_side_times_fraction() {
    let rows = [
        (1920.0, 1080.0, 0.17, 183.6),
        (1080.0, 1920.0, 0.17, 183.6),
        (800.0, 800.0, 0.5, 400.0),
        (800.0, 600.0, 1.0, 600.0),
    ];
    for (w, h, fraction, expected) in rows {
        assert_decimals(keyboard_pan_step_pixels(w, h, fraction), expected, 9, "step pixels");
    }
}

// ---- Step ----

// C#: KineticPanTests.Step_BothAxes_MoveTheSamePixels (2 rows: landscape, portrait)
#[test]
fn step_both_axes_move_the_same_pixels() {
    for (w, h) in [(1920.0, 1080.0), (1080.0, 1920.0)] {
        let bounds = ScrollBounds::new(9000.0, 9000.0, w, h);

        let (_, right_h, _) = keyboard_pan_step(1, 0, 3000.0, 3000.0, bounds, 0.17);
        let (_, _, down_v) = keyboard_pan_step(0, 1, 3000.0, 3000.0, bounds, 0.17);

        assert_decimals(right_h - 3000.0, down_v - 3000.0, 9, "horizontal vs vertical step");
        assert_decimals(
            right_h - 3000.0,
            w.min(h) * 0.17,
            9,
            "step is the short side times the fraction",
        );
    }
}

// C#: KineticPanTests.ImpulseVelocity_OfEqualSteps_IsEqual_SoKineticGlidesTravelTheSame
#[test]
fn impulse_velocity_of_equal_steps_is_equal_so_kinetic_glides_travel_the_same() {
    let bounds = ScrollBounds::new(9000.0, 9000.0, 1920.0, 1080.0);
    let (_, right_h, _) = keyboard_pan_step(1, 0, 3000.0, 3000.0, bounds, 0.17);
    let (_, _, down_v) = keyboard_pan_step(0, 1, 3000.0, 3000.0, bounds, 0.17);

    let (vx, _) = keyboard_pan_impulse_velocity(right_h - 3000.0, 0.0);
    let (_, vy) = keyboard_pan_impulse_velocity(0.0, down_v - 3000.0);

    assert_decimals(vx, vy, 12, "impulse velocity of equal steps");
}

// C#: KineticPanTests.Step_Horizontal_Positive_UsesDefaultStepFraction
#[test]
fn step_horizontal_positive_uses_default_step_fraction() {
    let (result, h, v) = keyboard_pan_step(1, 0, 400.0, 300.0, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    // step = min(800, 600) * StepFraction * (dx + dy) = 600 * 0.1 * 1 = 60
    assert_eq!(result, KeyboardPanResult::Panned);
    assert_decimals(h, 400.0 + 60.0, 9, "h");
    assert_decimals(v, 300.0, 9, "v");
}

// C#: KineticPanTests.Step_Horizontal_Negative_MovesLeft
#[test]
fn step_horizontal_negative_moves_left() {
    let (result, h, v) = keyboard_pan_step(-1, 0, 400.0, 300.0, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    // step = min(800, 600) * StepFraction * (dx + dy) = 600 * 0.1 * (-1) = -60
    assert_eq!(result, KeyboardPanResult::Panned);
    assert_decimals(h, 400.0 - 60.0, 9, "h");
    assert_decimals(v, 300.0, 9, "v");
}

// C#: KineticPanTests.Step_Vertical_Positive_UsesTheSameShortSideStep
#[test]
fn step_vertical_positive_uses_the_same_short_side_step() {
    let (result, h, v) = keyboard_pan_step(0, 1, 400.0, 300.0, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    // step = min(800, 600) * StepFraction * (dx + dy) = 600 * 0.1 * 1 = 60
    assert_eq!(result, KeyboardPanResult::Panned);
    assert_decimals(h, 400.0, 9, "h");
    assert_decimals(v, 300.0 + 60.0, 9, "v");
}

// C#: KineticPanTests.Step_Vertical_Negative_MovesUp
#[test]
fn step_vertical_negative_moves_up() {
    let (result, h, v) = keyboard_pan_step(0, -1, 400.0, 300.0, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    // step = ViewportHeight * StepFraction * (dx + dy) = 600 * 0.1 * (-1) = -60
    assert_eq!(result, KeyboardPanResult::Panned);
    assert_decimals(h, 400.0, 9, "h");
    assert_decimals(v, 300.0 - 60.0, 9, "v");
}

// C#: KineticPanTests.Step_CustomStepFraction_0_01_MovesOnePercent
#[test]
fn step_custom_step_fraction_0_01_moves_one_percent() {
    let (result, h, v) = keyboard_pan_step(1, 0, 400.0, 300.0, STEP_BOUNDS, 0.01);

    // step = min(800, 600) * 0.01 * 1 = 6
    assert_eq!(result, KeyboardPanResult::Panned);
    assert_decimals(h, 400.0 + 6.0, 9, "h");
    assert_decimals(v, 300.0, 9, "v");
}

// C#: KineticPanTests.Step_CustomStepFraction_1_0_MovesTheShortSide
#[test]
fn step_custom_step_fraction_1_0_moves_the_short_side() {
    let (result, h, v) = keyboard_pan_step(1, 0, 400.0, 300.0, STEP_BOUNDS, 1.0);

    // step = min(800, 600) * 1.0 * 1 = 600
    assert_eq!(result, KeyboardPanResult::Panned);
    assert_decimals(h, 400.0 + 600.0, 9, "h");
    assert_decimals(v, 300.0, 9, "v");
}

// C#: KineticPanTests.Step_AtRightEdge_ReturnsAtEdge
#[test]
fn step_at_right_edge_returns_at_edge() {
    let horizontal = STEP_BOUNDS.max_horizontal(); // 2000 - 800 = 1200
    let vertical = 300.0;

    let (result, h, v) = keyboard_pan_step(1, 0, horizontal, vertical, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    assert_eq!(result, KeyboardPanResult::AtEdge);
    assert_eq!(h, horizontal);
    assert_eq!(v, vertical);
}

// C#: KineticPanTests.Step_NearRightEdge_ClampsToBoundary
#[test]
fn step_near_right_edge_clamps_to_boundary() {
    let max_h = STEP_BOUNDS.max_horizontal(); // 1200
    let horizontal = max_h - 50.0; // 1150
    let vertical = 300.0;

    let (result, h, v) = keyboard_pan_step(1, 0, horizontal, vertical, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    // step = 800 * 0.1 * 1 = 80 is wanted, but the target is clamped to max.
    assert_eq!(result, KeyboardPanResult::Panned);
    assert_eq!(h, max_h);
    assert_eq!(v, vertical);
}

// C#: KineticPanTests.Step_AtBottomEdge_ReturnsAtEdge
#[test]
fn step_at_bottom_edge_returns_at_edge() {
    let horizontal = 400.0;
    let vertical = STEP_BOUNDS.max_vertical(); // 1500 - 600 = 900

    let (result, h, v) = keyboard_pan_step(0, 1, horizontal, vertical, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    assert_eq!(result, KeyboardPanResult::AtEdge);
    assert_eq!(h, horizontal);
    assert_eq!(v, vertical);
}

// C#: KineticPanTests.Step_NearBottomEdge_ClampsToBoundary
#[test]
fn step_near_bottom_edge_clamps_to_boundary() {
    let horizontal = 400.0;
    let max_v = STEP_BOUNDS.max_vertical(); // 900
    let vertical = max_v - 30.0; // 870

    let (result, h, v) = keyboard_pan_step(0, 1, horizontal, vertical, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    // step = 600 * 0.1 * 1 = 60 is wanted, but the target is clamped to max.
    assert_eq!(result, KeyboardPanResult::Panned);
    assert_eq!(h, horizontal);
    assert_eq!(v, max_v);
}

// C#: KineticPanTests.Step_AtLeftEdge_ReturnsAtEdge
#[test]
fn step_at_left_edge_returns_at_edge() {
    let horizontal = 0.0;
    let vertical = 300.0;

    let (result, h, v) = keyboard_pan_step(-1, 0, horizontal, vertical, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    assert_eq!(result, KeyboardPanResult::AtEdge);
    assert_eq!(h, horizontal);
    assert_eq!(v, vertical);
}

// C#: KineticPanTests.Step_NotScrollable_MaxIsSmall
#[test]
fn step_not_scrollable_max_is_small() {
    // MaxHorizontal = 100 - 800 = -700 -> Math.Max(0, -700) = 0, which is <= 0.5 (slack).
    let bounds = ScrollBounds::new(100.0, 1500.0, 800.0, 600.0);
    let horizontal = 0.0;
    let vertical = 300.0;

    let (result, h, v) = keyboard_pan_step(1, 0, horizontal, vertical, bounds, KEYBOARD_STEP_FRACTION);

    assert_eq!(result, KeyboardPanResult::NotScrollable);
    assert_eq!(h, horizontal);
    assert_eq!(v, vertical);
}

// C#: KineticPanTests.Step_HorizontalAxis_UsesHorizontalDirection
#[test]
fn step_horizontal_axis_uses_horizontal_direction() {
    let horizontal = 400.0;
    let vertical = 300.0;

    let (_, h, v) = keyboard_pan_step(1, 0, horizontal, vertical, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    // Only horizontal should change; vertical stays the same.
    assert_ne!(h, horizontal);
    assert_eq!(v, vertical);
}

// C#: KineticPanTests.Step_VerticalAxis_UsesVerticalDirection
#[test]
fn step_vertical_axis_uses_vertical_direction() {
    let horizontal = 400.0;
    let vertical = 300.0;

    let (_, h, v) = keyboard_pan_step(0, 1, horizontal, vertical, STEP_BOUNDS, KEYBOARD_STEP_FRACTION);

    // Only vertical should change; horizontal stays the same.
    assert_eq!(h, horizontal);
    assert_ne!(v, vertical);
}

// ---- KineticPanMutationGapTests: keyboard-pan edge slack ----

// C#: KineticPanMutationGapTests.KeyboardPanStep_ScrollRangeExactlyTheSlack_IsNotScrollable
#[test]
fn keyboard_pan_step_scroll_range_exactly_the_slack_is_not_scrollable() {
    let bounds = ScrollBounds::new(800.5, 600.5, 800.0, 600.0);

    assert_eq!(
        keyboard_pan_step(1, 0, 0.0, 0.0, bounds, KEYBOARD_STEP_FRACTION).0,
        KeyboardPanResult::NotScrollable
    );
    assert_eq!(
        keyboard_pan_step(0, 1, 0.0, 0.0, bounds, KEYBOARD_STEP_FRACTION).0,
        KeyboardPanResult::NotScrollable
    );
    assert!(!keyboard_pan_is_zoomed(bounds));
}

// C#: KineticPanMutationGapTests.KeyboardPanStep_ScrollRangeJustAboveTheSlack_IsZoomedAndScrollable
#[test]
fn keyboard_pan_step_scroll_range_just_above_the_slack_is_zoomed_and_scrollable() {
    let horizontal_range = ScrollBounds::new(800.6, 600.0, 800.0, 600.0);
    let vertical_range = ScrollBounds::new(800.0, 600.6, 800.0, 600.0);

    assert!(keyboard_pan_is_zoomed(horizontal_range));
    assert!(keyboard_pan_is_zoomed(vertical_range));
}

// C#: KineticPanMutationGapTests.KeyboardPanStep_StepOfExactlyTheSlack_StillPans
#[test]
fn keyboard_pan_step_step_of_exactly_the_slack_still_pans() {
    let bounds = ScrollBounds::new(105.0, 600.0, 5.0, 600.0); // 10 % of a 5 DIP viewport = 0.5 DIP

    let (result, horizontal, _) = keyboard_pan_step(1, 0, 0.0, 0.0, bounds, KEYBOARD_STEP_FRACTION);

    assert_eq!(result, KeyboardPanResult::Panned);
    assert_decimals(horizontal, 0.5, 9, "h");
}
