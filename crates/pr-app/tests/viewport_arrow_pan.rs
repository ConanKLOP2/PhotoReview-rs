//! Red tests (W8-01a, test porter) for `pr_app::viewport::arrow_pan` (C# `Viewport/ArrowPanRule.cs`, baseline `d92d6aae`).
//! The golden file `arrow-pan` is checked in `viewport_golden.rs`; these tests pin the same rules by hand.
//!
//! No C# test exists for `ArrowPanRule` (contract "C# tests to port": none). Every test below is NEW (F), written from the
//! doc comments and the contract semantics (notes 7 and 8, traps T2, T5, T7, T8). Every test fails through `todo!()`.

use pr_app::input::primitives::RectD;
use pr_app::viewport::arrow_pan::{ArrowPanOutcome, DEFAULT_STEP_FRACTION, step, step_length};
use pr_app::viewport::types::ViewportLayout;

/// An 800 x 600 viewport over a 1600 x 1200 image: the horizontal and vertical maxima are 800 and 600.
fn layout_800_by_600() -> ViewportLayout {
    ViewportLayout {
        viewport_width: 800.0,
        viewport_height: 600.0,
        extent_width: 1600.0,
        extent_height: 1200.0,
        horizontal_bar_visible: true,
        vertical_bar_visible: true,
        image_rect: RectD {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 1200.0,
        },
        max_horizontal_offset: 800.0,
        max_vertical_offset: 600.0,
    }
}

// New test: the step is the shorter side times the fraction: min(800, 600) * 0.1 = 60 DIP.
#[test]
fn step_length_is_shorter_side_times_fraction() {
    assert!((step_length(800.0, 600.0, 0.1) - 60.0).abs() < 1e-9);
    assert!(
        (step_length(300.0, 900.0, 0.1) - 30.0).abs() < 1e-9,
        "portrait: the width is the shorter side"
    );
    assert_eq!(DEFAULT_STEP_FRACTION, 0.1);
}

// New test: a NaN side propagates as in C# `Math.Min` (contract trap T2), so the length is 0, not `600 * 0.1`.
#[test]
fn step_length_nan_side_is_zero() {
    assert_eq!(step_length(f64::NAN, 600.0, 0.1), 0.0);
    assert_eq!(step_length(800.0, f64::NAN, 0.1), 0.0);
}

// New test: a fraction that is NaN or infinite is not finite, so the length is 0 (C# returns 0 unless finite and > 0).
#[test]
fn step_length_non_finite_fraction_is_zero() {
    assert_eq!(step_length(800.0, 600.0, f64::NAN), 0.0);
    assert_eq!(step_length(800.0, 600.0, f64::INFINITY), 0.0);
}

// New test: a NaN maximum is not caught by the `max <= 0.5` test (the comparison is false), so the press pans.
// Trap T2 again: the `f64::clamp` form would panic here; the C# `Math.Clamp` order returns the value.
#[test]
fn step_nan_max_is_not_scrollable_check_so_pan_happens() {
    let mut layout = layout_800_by_600();
    layout.max_horizontal_offset = f64::NAN;

    let (outcome, horizontal, vertical) = step(1, 0, 60.0, 0.0, &layout, 0.1);
    assert_eq!(outcome, ArrowPanOutcome::Panned);
    assert!((horizontal - 120.0).abs() < 1e-9, "60 + one 60 DIP step");
    assert_eq!(vertical, 0.0);
}

// New test: a move of exactly 0.5 DIP pans; the "at edge" test is strict (`|target - current| < 0.5`, contract note 7).
#[test]
fn step_exact_half_pixel_pans() {
    let layout = ViewportLayout {
        viewport_width: 5.0,
        viewport_height: 5.0,
        extent_width: 10.0,
        extent_height: 10.0,
        horizontal_bar_visible: true,
        vertical_bar_visible: true,
        image_rect: RectD {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        },
        max_horizontal_offset: 0.6,
        max_vertical_offset: 0.6,
    };

    let (outcome, horizontal, vertical) = step(1, 0, 0.0, 0.0, &layout, 0.1);
    assert_eq!(outcome, ArrowPanOutcome::Panned);
    assert_eq!(horizontal, 0.5);
    assert_eq!(vertical, 0.0);
}

// New test: a diagonal press uses the horizontal axis only, with the step doubled: (1, 1) moves 2 * 60 = 120 DIP on x.
#[test]
fn step_diagonal_uses_horizontal_axis_twice() {
    let layout = layout_800_by_600();

    let (outcome, horizontal, vertical) = step(1, 1, 0.0, 0.0, &layout, 0.1);
    assert_eq!(outcome, ArrowPanOutcome::Panned);
    assert!((horizontal - 120.0).abs() < 1e-9);
    assert_eq!(vertical, 0.0, "the vertical offset is not touched when dx != 0");
}
