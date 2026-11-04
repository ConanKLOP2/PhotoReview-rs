//! M3 (PARITY close-out follow-up, 2026-11-04): hand-written killers for the surviving cargo-mutants of
//! `viewmodels/viewer_state.rs` and `input/kinetic_pan.rs`. Expected values are derived by hand from the C# sources
//! (`ViewerState.cs`, `KineticPan.cs` at baseline d92d6aae), never from a golden file. The evidence file
//! `docs/reviews/M3-MUT-2026-11-04.md` maps each killer to its mutant.

use pr_app::input::kinetic_pan::{
    KineticScroller, PanVelocityTracker, ScrollBounds, keyboard_pan_step, keyboard_pan_step_pixels,
};

// Killer for dotnet_max (kinetic_pan.rs:56): C# `Math.Max(0, ExtentWidth - ViewportWidth)` is 0 when the image is smaller.
#[test]
fn scroll_bounds_max_is_zero_when_extent_is_smaller_than_viewport() {
    let b = ScrollBounds::new(100.0, 50.0, 400.0, 300.0);
    assert_eq!(b.max_horizontal(), 0.0);
    assert!(b.max_horizontal().is_sign_positive());
    assert_eq!(b.max_vertical(), 0.0);
    assert!(b.max_vertical().is_sign_positive());
    let c = ScrollBounds::new(500.0, 400.0, 400.0, 300.0);
    assert_eq!(c.max_horizontal(), 100.0);
    assert_eq!(c.max_vertical(), 100.0);
}

// A glide on an axis whose extent is smaller than the viewport must leave the offset at 0 (max is 0, not negative).
#[test]
fn kinetic_step_on_non_scrollable_axis_keeps_offset_zero() {
    let mut k = KineticScroller::default();
    k.add_impulse(1.0, 1.0);
    let (h, v) = k.step(16.0, 0.0, 0.0, ScrollBounds::new(100.0, 50.0, 400.0, 300.0));
    assert_eq!(h, 0.0);
    assert_eq!(v, 0.0);
    assert!(h.is_sign_positive() && v.is_sign_positive());
}

// An arrow press on a non-scrollable axis is NotScrollable (max 0 <= 0.5), the offsets come back unchanged.
#[test]
fn keyboard_step_non_scrollable_axis_returns_inputs() {
    let b = ScrollBounds::new(100.0, 50.0, 400.0, 300.0);
    let (r, h, v) = keyboard_pan_step(1, 0, 3.0, 4.0, b, 0.1);
    assert_eq!(r, pr_app::input::kinetic_pan::KeyboardPanResult::NotScrollable);
    assert_eq!((h, v), (3.0, 4.0));
}

// dotnet_max(0.0, -0.0): .NET Math.Max returns +0.0 (kills `delete !` at kinetic_pan.rs:56:35).
#[test]
fn scroll_bounds_max_negative_zero_difference_gives_positive_zero() {
    let b = ScrollBounds::new(-0.0, -0.0, 0.0, 0.0);
    assert_eq!(b.max_horizontal(), 0.0);
    assert!(b.max_horizontal().is_sign_positive());
    assert!(b.max_vertical().is_sign_positive());
}

// dotnet_min: NaN on the first side propagates (kills `||` -> `&&` at 65:19).
#[test]
fn keyboard_step_pixels_nan_on_either_side_is_nan() {
    assert!(keyboard_pan_step_pixels(f64::NAN, 5.0, 0.1).is_nan());
    assert!(keyboard_pan_step_pixels(5.0, f64::NAN, 0.1).is_nan());
}

// dotnet_min(0.0, -0.0) is -0.0 in .NET (kills `<` -> `<=` at 67:17).
#[test]
fn keyboard_step_pixels_min_of_zeros_prefers_negative_zero() {
    assert!(keyboard_pan_step_pixels(0.0, -0.0, 1.0).is_sign_negative());
    assert!(keyboard_pan_step_pixels(-0.0, 0.0, 1.0).is_sign_negative());
    assert!(keyboard_pan_step_pixels(0.0, 0.0, 1.0).is_sign_positive());
}

// dotnet_min with two different negative values returns the smaller one (kills 67:32 `&&`->`||` and 67:27 `==`->`!=`).
#[test]
fn keyboard_step_pixels_min_of_two_negatives_is_the_smaller() {
    assert_eq!(keyboard_pan_step_pixels(-3.0, -5.0, 1.0), -5.0);
    assert_eq!(keyboard_pan_step_pixels(-5.0, -3.0, 1.0), -5.0);
    assert_eq!(keyboard_pan_step_pixels(-3.0, 5.0, 1.0), -3.0);
}

// dotnet_clamp returns `value` itself when it equals a bound, so a -0.0 target stays -0.0 as in C# `Math.Clamp`
// (kills `<` -> `<=` at kinetic_pan.rs:77:14 and `>` -> `>=` at 79:21). `start(0.0, 1.0)` gives vx = -0.0.
#[test]
fn kinetic_step_negative_zero_target_is_not_replaced_by_the_bound() {
    for max_extent in [400.0, 300.0] {
        // max_horizontal is 100 (first run: -0.0 < min is false) and 0 (second run: -0.0 > max is false).
        let mut k = KineticScroller::default();
        assert!(k.start(0.0, 1.0));
        assert!(k.velocity_x().is_sign_negative() && k.velocity_x() == 0.0);
        let (h, _) = k.step(16.0, -0.0, 0.0, ScrollBounds::new(max_extent, 500.0, 300.0, 300.0));
        assert_eq!(h, 0.0);
        assert!(
            h.is_sign_negative(),
            "extent {max_extent}: a -0.0 target must stay -0.0"
        );
    }
}

// C# `PanVelocityTracker.Reset()` forgets every sample (kills `reset -> ()` at kinetic_pan.rs:110:9).
#[test]
fn tracker_reset_forgets_samples_and_velocity() {
    let mut t = PanVelocityTracker::new();
    for i in 0..5 {
        t.add(f64::from(i) * 10.0, f64::from(i) * 20.0, 0.0);
    }
    assert_eq!(t.count(), 5);
    t.reset();
    assert_eq!(t.count(), 0);
    assert_eq!(t.velocity(), (0.0, 0.0));
    t.add(100.0, 500.0, 0.0);
    assert_eq!(t.count(), 1);
    assert_eq!(t.velocity(), (0.0, 0.0));
    t.add(120.0, 540.0, 0.0);
    assert_eq!(t.velocity(), (2.0, 0.0));
}

// C# `CalculateStepZoom`: a zero step takes the "down" branch (`step > 0` is false). Kills `>` -> `>=` at viewer_state.rs:193:13.
#[test]
fn step_zoom_zero_step_uses_the_down_branch() {
    use pr_app::viewmodels::viewer_state::calculate_step_zoom;
    // At the top edge the up branch would answer None; the down branch keeps 4.0.
    assert_eq!(calculate_step_zoom(4.0, 0.0), Some(4.0));
    // At the bottom edge the down branch answers None; the up branch would answer Some(0.25).
    assert_eq!(calculate_step_zoom(0.25, 0.0), None);
}

// Every plain getter of `ViewerState` returns the stored value, with values different from the defaults and from the
// constants a `-> f64/bool/i32` mutant would return (kills the `dpi_scale`, `force_high_quality_scaling`, ... body replacements).
#[test]
fn viewer_state_getters_return_the_stored_values() {
    use pr_app::viewmodels::viewer_state::ViewerState;
    use pr_app::viewport::types::ViewerStretchMode;
    use pr_core::model::enums::ScalingQuality;
    let mut s = ViewerState::new();
    s.set_source_size(4000, 3000, true);
    assert_eq!((s.source_pixel_width(), s.source_pixel_height()), (4000, 3000));
    s.set_dpi_scale(2.5);
    assert_eq!(s.dpi_scale(), 2.5);
    s.set_max_image_width(1234.5);
    s.set_max_image_height(678.25);
    assert_eq!((s.max_image_width(), s.max_image_height()), (1234.5, 678.25));
    s.set_zoom_step(0.5);
    assert_eq!(s.zoom_step(), 0.5);
    s.set_zoom_raw(2.75);
    assert_eq!(s.zoom(), 2.75);
    s.set_stretch(ViewerStretchMode::None);
    assert_eq!(s.stretch(), ViewerStretchMode::None);
    s.set_stretch(ViewerStretchMode::Uniform);
    assert_eq!(s.stretch(), ViewerStretchMode::Uniform);
    s.set_scaling_quality(ScalingQuality::Linear);
    assert_eq!(s.scaling_quality(), ScalingQuality::Linear);
    assert!(!s.force_high_quality_scaling());
    s.set_force_high_quality_scaling(true);
    assert!(s.force_high_quality_scaling());
    s.set_force_high_quality_scaling(false);
    assert!(!s.force_high_quality_scaling());
    assert!(!s.is_fullscreen());
    s.set_fullscreen(true);
    assert!(s.is_fullscreen());
    s.set_fullscreen(false);
    assert!(!s.is_fullscreen());
}

// C# `DisplayZoomPercent` in Fit with an unknown fit zoom (0) shows 100 %, not 0 % (kills `>` -> `>=` at viewer_state.rs:340:20).
#[test]
fn display_zoom_percent_in_fit_with_unknown_fit_zoom_is_100() {
    use pr_app::viewmodels::viewer_state::ViewerState;
    let s = ViewerState::new();
    assert!(s.is_fit());
    assert_eq!(s.fit_zoom(), 0.0);
    assert_eq!(s.display_zoom_percent(), 100);
}

// C# `EffectiveZoom`: null in Fit, else Zoom (kills the `effective_zoom` body replacements at viewer_state.rs:349:9).
#[test]
fn effective_zoom_is_none_in_fit_and_the_zoom_otherwise() {
    use pr_app::viewmodels::viewer_state::ViewerState;
    let mut s = ViewerState::new();
    assert_eq!(s.effective_zoom(), None);
    s.set_zoom(2.5);
    assert_eq!(s.effective_zoom(), Some(2.5));
    s.set_zoom(1.0);
    assert_eq!(s.effective_zoom(), Some(1.0));
    s.reset_fit(800.0, 600.0);
    assert_eq!(s.effective_zoom(), None);
}

// C# `WheelZoom(0)` steps DOWN (only `delta > 0` steps up). Kills `>` -> `>=` at viewer_state.rs:553:29.
#[test]
fn wheel_zoom_zero_delta_steps_down() {
    use pr_app::viewmodels::viewer_state::ViewerState;
    let mut s = ViewerState::new();
    s.set_zoom(2.0);
    s.wheel_zoom(0);
    assert_eq!(s.zoom(), 1.75);
    s.wheel_zoom(1);
    assert_eq!(s.zoom(), 2.0);
    s.wheel_zoom(-1);
    assert_eq!(s.zoom(), 1.75);
}

// The arrow step is `step_pixels * (dx + dy)`: with |dx + dy| > 1 a division gives another distance
// (kills `*` -> `/` at kinetic_pan.rs:347:9; with unit directions both are the same).
#[test]
fn keyboard_step_scales_by_the_direction_sum() {
    use pr_app::input::kinetic_pan::KeyboardPanResult;
    let b = ScrollBounds::new(2000.0, 2000.0, 400.0, 300.0); // step pixels = min(400, 300) * 0.1 = 30
    assert_eq!(
        keyboard_pan_step(2, 0, 0.0, 0.0, b, 0.1),
        (KeyboardPanResult::Panned, 60.0, 0.0)
    );
    assert_eq!(
        keyboard_pan_step(-2, 0, 100.0, 7.0, b, 0.1),
        (KeyboardPanResult::Panned, 40.0, 7.0)
    );
    assert_eq!(
        keyboard_pan_step(0, 3, 5.0, 0.0, b, 0.1),
        (KeyboardPanResult::Panned, 5.0, 90.0)
    );
}

// C# `ApplyInitialViewMode(FitWidth/FitHeight)` merges the viewport refresh with the zoom: the result carries the zoom
// changes and `ZoomModeChanged` (kills `merge -> Default` at viewer_state.rs:690:9 and `||` -> `&&` at 692:56).
#[test]
fn apply_initial_view_mode_fit_width_and_height_report_zoom_changes_and_mode_event() {
    use pr_app::viewmodels::viewer_state::{ViewerProperty, ViewerState};
    use pr_core::model::enums::InitialViewMode;
    for (mode, zoom) in [
        (InitialViewMode::FitWidth, 1000.0 / 4000.0),
        (InitialViewMode::FitHeight, 800.0 / 3000.0),
    ] {
        let mut s = ViewerState::new();
        s.set_source_size(4000, 3000, true);
        let update = s
            .apply_initial_view_mode(mode, 1000.0, 800.0, 100, false)
            .expect("not kept");
        assert!(update.zoom_mode_changed, "{mode:?}");
        assert!(update.changes.contains(ViewerProperty::Zoom), "{mode:?}");
        assert!(update.changes.contains(ViewerProperty::Stretch), "{mode:?}");
        assert!((s.zoom() - zoom).abs() < 1e-12, "{mode:?}");
    }
}

// A display timing with a zero refresh period is unusable (`refresh_period > 0`), so Predict mode steps by the rendering
// time like `Off` (kills `>` -> `>=` at kinetic_pan.rs:451:57).
#[test]
fn glide_clock_zero_refresh_period_is_treated_as_no_timing() {
    use pr_app::input::kinetic_pan::GlideFrameClock;
    use pr_core::abstractions::DisplayTiming;
    use pr_core::model::enums::KineticGlideSmoothing;
    let zero = Some(DisplayTiming {
        last_vblank: 1_000,
        refresh_period: 0,
    });
    let mut clock = GlideFrameClock::new();
    clock.start(KineticGlideSmoothing::Predict, 10_000.0);
    assert_eq!(clock.advance(100.0, 5_000, zero), 0.0);
    assert_eq!(clock.advance(116.0, 6_000, zero), 16.0);
    assert_eq!(clock.advance(133.0, 7_000, zero), 17.0);
}

// Zoom in from Fit starts at the Fit zoom when it is known (C# `StepZoom` base). Kills `fit > 0.0` -> `fit < 0.0`
// at viewer_state.rs:725:33.
#[test]
fn zoom_in_from_fit_starts_at_the_fit_zoom() {
    use pr_app::viewmodels::viewer_state::ViewerState;
    let mut s = ViewerState::new();
    s.set_source_size(4000, 3000, true);
    s.reset_fit(1000.0, 800.0); // fit zoom = min(1000 / 4000, 800 / 3000) = 0.25
    assert_eq!(s.fit_zoom(), 0.25);
    s.zoom_in();
    assert_eq!(s.zoom(), 0.5);
}

// Repeated or backwards rendering time steps nothing; a NaN rendering time too (kills `||` -> `&&` at kinetic_pan.rs:489:39).
#[test]
fn glide_clock_non_advancing_rendering_time_returns_zero() {
    use pr_app::input::kinetic_pan::GlideFrameClock;
    let mut clock = GlideFrameClock::new();
    assert_eq!(clock.advance(100.0, 0, None), 0.0);
    assert_eq!(clock.advance(116.0, 0, None), 16.0);
    assert_eq!(clock.advance(110.0, 0, None), 0.0);
    assert_eq!(clock.advance(f64::NAN, 0, None), 0.0);
    assert_eq!(clock.advance(132.0, 0, None), 16.0);
}

// The predicted vblank is the first one at least 1 ms (`ticks_per_ms` ticks) after now: a target exactly on a vblank
// belongs to that vblank (kills `*` -> `+` at kinetic_pan.rs:507:68).
#[test]
fn glide_clock_present_lead_is_one_millisecond_of_ticks() {
    use pr_app::input::kinetic_pan::GlideFrameClock;
    use pr_core::abstractions::DisplayTiming;
    use pr_core::model::enums::KineticGlideSmoothing;
    let timing = Some(DisplayTiming {
        last_vblank: 0,
        refresh_period: 100_000,
    });
    let mut clock = GlideFrameClock::new();
    clock.start(KineticGlideSmoothing::Predict, 10_000.0);
    assert_eq!(clock.advance(0.0, 50_000, timing), 0.0); // anchors on the vblank at 100_000
    // 90_000 + 10_000 ticks is exactly the anchor vblank: grid index 0, nothing to step.
    assert_eq!(clock.advance(5.0, 90_000, timing), 0.0);
    // 190_000 + 10_000 is the next vblank: one period (100_000 ticks = 10 ms).
    assert_eq!(clock.advance(15.0, 190_000, timing), 10.0);
}
