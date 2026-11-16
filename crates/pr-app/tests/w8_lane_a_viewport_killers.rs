//! Boundary killers for `pr_app::misc::viewport_convergence::is_stable_viewport`.
//!
//! C# `ViewportConvergence.IsStableViewport` uses strict `Math.Abs(a - b) < Epsilon` with `Epsilon = 0.5`, so a difference of
//! exactly 0.5 DIP is NOT stable. All values below are binary-exact (multiples of 0.25), so `abs(diff)` is exactly 0.5, with
//! no rounding. The existing `viewport_convergence.rs` tests only use differences far from the boundary.

use pr_app::misc::viewport_convergence::{EPSILON, is_stable_viewport};
use pr_app::viewport::types::{ViewerStretchMode, ViewportSnapshot};

fn base() -> ViewportSnapshot {
    ViewportSnapshot {
        zoom: 1.0,
        stretch: ViewerStretchMode::Uniform,
        max_image_width: 800.0,
        max_image_height: 600.0,
        actual_image_width: 800.0,
        actual_image_height: 600.0,
        extent_width: 800.0,
        extent_height: 600.0,
        viewport_width: 800.0,
        viewport_height: 600.0,
        horizontal_offset: 0.0,
        vertical_offset: 0.0,
        horizontal_scroll_bar_visible: false,
        vertical_scroll_bar_visible: false,
    }
}

#[derive(Clone, Copy, Debug)]
enum Axis {
    ViewportWidth,
    ViewportHeight,
    ExtentWidth,
    ExtentHeight,
}

fn shifted(axis: Axis, delta: f64) -> ViewportSnapshot {
    let mut s = base();
    match axis {
        Axis::ViewportWidth => s.viewport_width += delta,
        Axis::ViewportHeight => s.viewport_height += delta,
        Axis::ExtentWidth => s.extent_width += delta,
        Axis::ExtentHeight => s.extent_height += delta,
    }
    s
}

const AXES: [Axis; 4] = [
    Axis::ViewportWidth,
    Axis::ViewportHeight,
    Axis::ExtentWidth,
    Axis::ExtentHeight,
];

#[test]
fn epsilon_is_half_a_dip() {
    assert_eq!(EPSILON, 0.5);
}

fn assert_exact_epsilon_unstable(axis: Axis) {
    for delta in [0.5, -0.5] {
        assert!(
            !is_stable_viewport(&base(), &shifted(axis, delta)),
            "axis {axis:?} delta {delta}: exactly 0.5 must be unstable (strict <)"
        );
        assert!(
            !is_stable_viewport(&shifted(axis, delta), &base()),
            "axis {axis:?} delta {delta} (swapped): exactly 0.5 must be unstable (strict <)"
        );
    }
}

#[test]
fn exact_epsilon_viewport_width_is_unstable() {
    assert_exact_epsilon_unstable(Axis::ViewportWidth);
}

#[test]
fn exact_epsilon_viewport_height_is_unstable() {
    assert_exact_epsilon_unstable(Axis::ViewportHeight);
}

#[test]
fn exact_epsilon_extent_width_is_unstable() {
    assert_exact_epsilon_unstable(Axis::ExtentWidth);
}

#[test]
fn exact_epsilon_extent_height_is_unstable() {
    assert_exact_epsilon_unstable(Axis::ExtentHeight);
}

#[test]
fn difference_just_below_epsilon_is_stable_on_every_axis_both_directions() {
    for axis in AXES {
        for delta in [0.25, -0.25] {
            assert!(
                is_stable_viewport(&base(), &shifted(axis, delta)),
                "axis {axis:?} delta {delta}: 0.25 < 0.5 must be stable"
            );
        }
    }
}
