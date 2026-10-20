//! Repair round 1 (W8-06a): tests that kill the mutants the verifier found alive in `viewer_state.rs`.
//! Expected values derived by hand from C# `ViewerState.cs` at d92d6aae.

use pr_app::viewmodels::viewer_state::{ViewerChanges, ViewerState, calculate_step_zoom};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn fitted(sw: i32, sh: i32, vw: f64, vh: f64) -> ViewerState {
    let mut s = ViewerState::new();
    s.set_source_size(sw, sh, true);
    s.reset_fit(vw, vh);
    s
}

#[test]
fn fit_height_axis_swap_with_dpi() {
    let mut s = fitted(4000, 3000, 1000.0, 800.0);
    s.set_dpi_scale(2.0);
    s.zoom_to_fit_height();
    assert!(close(s.zoom(), 800.0 * 2.0 / 3000.0));
    s.swap_source_size(4000, 2000, &mut |_| {});
    assert!(close(s.zoom(), 800.0 * 2.0 / 2000.0));
}

#[test]
fn kill_is_empty() {
    assert!(ViewerChanges::EMPTY.is_empty());
    assert!(!ViewerState::new().set_fullscreen(true).is_empty());
}

#[test]
fn kill_step_down_from_exactly_max_step() {
    assert_eq!(calculate_step_zoom(4.0, -0.25), Some(3.75));
}

#[test]
fn kill_overflowing_fit_is_unknown() {
    let mut s = ViewerState::new();
    s.set_source_size(1, 1, true);
    s.set_max_image_width(1e308);
    s.set_max_image_height(1e308);
    s.set_dpi_scale(16.0);
    assert_eq!(s.fit_zoom(), 0.0);
    assert_eq!(s.fit_width_zoom(), 0.0);
    assert_eq!(s.fit_height_zoom(), 0.0);
}

#[test]
fn kill_single_dimension_change_clears_axis() {
    for (nw, nh) in [(4000, 2000), (3000, 3000)] {
        let mut s = fitted(4000, 3000, 1000.0, 800.0);
        s.zoom_to_fit_width();
        s.set_source_size(nw, nh, false);
        s.swap_source_size(2000, 1500, &mut |_| {});
        assert!(close(s.zoom(), 0.25), "{nw}x{nh}");
    }
}

#[test]
fn kill_fit_width_swap_with_dpi() {
    let mut s = fitted(4000, 3000, 1000.0, 800.0);
    s.set_dpi_scale(2.0);
    s.zoom_to_fit_width();
    s.swap_source_size(2000, 3000, &mut |_| {});
    assert!(close(s.zoom(), 1000.0 * 2.0 / 2000.0));
}
