//! Red tests (W8-06a, test porter) for `pr_app::viewmodels::viewer_state`: sections B and C of `contracts/W8-06a.md`.
//!
//! Section B: all of `tests/PhotoReview.App.Tests/ViewModels/ViewerStateMutationGapTests.cs` (Stryker boundary and guard pins:
//! zero and unknown sizes, viewport readings at or below 1 DIP or non-finite, the DPI plausibility range, and which readings
//! must not touch the Fit-axis memory). Section C: all of `ViewerStateRefreshScalingTests.cs` (`EffectiveScalingQuality`).
//! Names keep the C# method name in snake_case with the `mutation_gap_` prefix for section B.
//!
//! The C# `Fitted(...)` helper is `fitted` here: `set_source_size` then `update_viewport`. Setter properties (`state.MaxImageWidth =
//! x`) are the raw setters `set_max_image_width` and friends, which do not clamp or touch the Fit axis (contract D-06a-3).
//!
//! Every test fails through `todo!()` until the bodies land.

use pr_app::viewmodels::viewer_state::{
    MAX_STEP_ZOOM, MIN_STEP_ZOOM, ViewerProperty, ViewerState, calculate_display_size, calculate_step_zoom,
};
use pr_core::model::enums::ScalingQuality;

/// C# `ViewerStateMutationGapTests.Fitted(...)`: a state with a source size and a viewport reading (not forced).
fn fitted(source_width: i32, source_height: i32, viewport_width: f64, viewport_height: f64) -> ViewerState {
    let mut state = ViewerState::new();
    state.set_source_size(source_width, source_height, false);
    state.update_viewport(viewport_width, viewport_height, false);
    state
}

/// Tolerance of xunit's `Assert.Equal(expected, actual, digits)`: `digits` decimal places.
fn assert_close(actual: f64, expected: f64, tolerance: f64, context: &str) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{context}: actual {actual}, expected {expected}"
    );
}

// ---- B. ViewerStateMutationGapTests ----

// C#: ViewerStateMutationGapTests.ImageSize_WithSourceDimensionOfZero_StaysAutoNotAZeroSizedElement
// Theory rows: (0 x 100), (100 x 0), (0 x 0).
#[test]
fn mutation_gap_image_size_with_source_dimension_of_zero_stays_auto() {
    let cases = [(0, 100), (100, 0), (0, 0)];
    for (width, height) in cases {
        let mut state = ViewerState::new();
        state.set_zoom_raw(2.0);
        state.set_source_size(width, height, false);

        assert!(state.image_width().is_nan(), "{width}x{height}: ImageWidth");
        assert!(state.image_height().is_nan(), "{width}x{height}: ImageHeight");
    }
}

// C#: ViewerStateMutationGapTests.FitZoom_WithUnknownSourceAxis_IsUnknownEvenIfTheOtherAxisIsKnown
#[test]
fn mutation_gap_fit_zoom_with_unknown_source_axis_is_unknown() {
    let cases = [(0, 100), (100, 0)];
    for (width, height) in cases {
        let state = fitted(width, height, 800.0, 600.0);

        assert_eq!(state.fit_zoom(), 0.0, "{width}x{height}");
    }
}

// C#: ViewerStateMutationGapTests.FitZoom_WithUnusableViewportLimits_IsUnknown
// Rows: exactly 1 DIP is not usable; infinite, NaN and below-1 readings are not usable.
#[test]
fn mutation_gap_fit_zoom_with_unusable_viewport_limits_is_unknown() {
    let cases = [
        (1.0, 600.0),
        (800.0, 1.0),
        (f64::INFINITY, 600.0),
        (800.0, f64::INFINITY),
        (f64::NAN, 600.0),
        (800.0, f64::NAN),
        (0.5, 600.0),
        (800.0, 0.5),
    ];
    for (max_width, max_height) in cases {
        let mut state = ViewerState::new();
        state.set_source_size(1000, 1000, false);
        state.set_max_image_width(max_width);
        state.set_max_image_height(max_height);

        assert_eq!(state.fit_zoom(), 0.0, "max {max_width} x {max_height}");
    }
}

// C#: ViewerStateMutationGapTests.FitWidthZoom_WithViewportWidthOfExactlyOne_IsUnknown
#[test]
fn mutation_gap_fit_width_zoom_with_viewport_width_of_exactly_one_is_unknown() {
    let mut state = ViewerState::new();
    state.set_source_size(1000, 1000, false);
    state.set_max_image_width(1.0);

    assert_eq!(state.fit_width_zoom(), 0.0);
}

// C#: ViewerStateMutationGapTests.FitHeightZoom_WithViewportHeightOfExactlyOne_IsUnknown
#[test]
fn mutation_gap_fit_height_zoom_with_viewport_height_of_exactly_one_is_unknown() {
    let mut state = ViewerState::new();
    state.set_source_size(1000, 1000, false);
    state.set_max_image_height(1.0);

    assert_eq!(state.fit_height_zoom(), 0.0);
}

// C#: ViewerStateMutationGapTests.FitWidthZoom_WithInfiniteWidthButFiniteHeight_IsUnknownAndHeightZoomStillWorks
#[test]
fn mutation_gap_fit_width_zoom_with_infinite_width_but_finite_height() {
    let mut state = ViewerState::new();
    state.set_source_size(1000, 500, false);
    state.set_max_image_height(250.0);

    assert_eq!(state.fit_width_zoom(), 0.0);
    assert_close(state.fit_height_zoom(), 0.5, 1e-6, "250 / 500");
}

// C#: ViewerStateMutationGapTests.DpiScale_AtTheEdgesOfThePlausibleRange_IsUsedAsReported
// Rows: 0.25 (lowest plausible scale is honoured, not replaced by 1.0) and 16.0 (highest).
#[test]
fn mutation_gap_dpi_scale_at_the_edges_of_the_plausible_range_is_used_as_reported() {
    let cases = [0.25, 16.0];
    for dpi in cases {
        let mut state = ViewerState::new();
        state.set_source_size(1000, 1000, false);
        state.update_viewport(500.0, 500.0, false);
        state.set_dpi_scale(dpi);

        assert_close(state.fit_zoom(), 0.5 * dpi, 1e-9, &format!("dpi {dpi}: fit zoom"));

        let (width, height) = calculate_display_size(1000, 1000, 1.0, dpi);
        assert_close(width, 1000.0 / dpi, 1e-9, &format!("dpi {dpi}: display width"));
        assert_close(height, 1000.0 / dpi, 1e-9, &format!("dpi {dpi}: display height"));
    }
}

// C#: ViewerStateMutationGapTests.DpiScale_JustOutsideThePlausibleRange_ReadsAs100Percent
// Rows: 0.24 and 16.01 are read as 1.0, so the fit zoom is 0.5.
#[test]
fn mutation_gap_dpi_scale_just_outside_the_plausible_range_reads_as_100_percent() {
    let cases = [0.24, 16.01];
    for dpi in cases {
        let mut state = ViewerState::new();
        state.set_source_size(1000, 1000, false);
        state.update_viewport(500.0, 500.0, false);
        state.set_dpi_scale(dpi);

        assert_close(state.fit_zoom(), 0.5, 1e-9, &format!("dpi {dpi} reads as 100 percent"));
    }
}

// C#: ViewerStateMutationGapTests.SwapSourceSize_WhenOnlyTheHeightChanges_TakesTheNewSize
#[test]
fn mutation_gap_swap_when_only_the_height_changes_takes_the_new_size() {
    let mut state = ViewerState::new();
    state.set_source_size(100, 100, false);
    state.set_zoom(2.0);

    state.swap_source_size(100, 120, &mut |_: &ViewerState| {});

    assert_eq!(state.source_pixel_width(), 100);
    assert_eq!(state.source_pixel_height(), 120);
}

// C#: ViewerStateMutationGapTests.SwapSourceSize_WhenOnlyTheWidthChanges_TakesTheNewSize
#[test]
fn mutation_gap_swap_when_only_the_width_changes_takes_the_new_size() {
    let mut state = ViewerState::new();
    state.set_source_size(100, 100, false);
    state.set_zoom(2.0);

    state.swap_source_size(120, 100, &mut |_: &ViewerState| {});

    assert_eq!(state.source_pixel_width(), 120);
    assert_eq!(state.source_pixel_height(), 100);
}

// C#: ViewerStateMutationGapTests.SwapSourceSize_WithAnUnknownSizeOnEitherSide_JustSetsTheSizeWithoutAnchoringTheScroll
// Rows: (old 0 x 50 -> 100 x 100: the shown size was unknown), (old 50 x 0), (replacement 0 x 100), (replacement 100 x 0).
#[test]
fn mutation_gap_swap_with_an_unknown_size_on_either_side_just_sets_the_size() {
    let cases = [(0, 50, 100, 100), (50, 0, 100, 100), (50, 50, 0, 100), (50, 50, 100, 0)];
    for (old_width, old_height, new_width, new_height) in cases {
        let mut state = ViewerState::new();
        state.set_source_size(old_width, old_height, false);
        state.set_zoom(2.0);
        let mut swapping = 0;

        state.swap_source_size(new_width, new_height, &mut |_: &ViewerState| swapping += 1);

        assert_eq!(
            swapping, 0,
            "{old_width}x{old_height} -> {new_width}x{new_height}: no announcement"
        );
        assert_eq!(state.source_pixel_width(), new_width);
        assert_eq!(state.source_pixel_height(), new_height);
    }
}

// C#: ViewerStateMutationGapTests.SwapSourceSize_InFit_JustSetsTheSizeWithoutAnchoringTheScroll
#[test]
fn mutation_gap_swap_in_fit_just_sets_the_size_without_anchoring() {
    let mut state = fitted(50, 50, 800.0, 600.0);
    let mut swapping = 0;

    state.swap_source_size(60, 60, &mut |_: &ViewerState| swapping += 1);

    assert_eq!(swapping, 0);
    assert_eq!(state.source_pixel_width(), 60);
}

// C#: ViewerStateMutationGapTests.SwapSourceSize_WhileZoomed_AnnouncesTheSwapBeforeTheSizeChanges
// Trap T5: the listener reads the OLD width (50) and the state then holds the new one (60).
#[test]
fn mutation_gap_swap_while_zoomed_announces_the_swap_before_the_size_changes() {
    let mut state = ViewerState::new();
    state.set_source_size(50, 50, false);
    state.set_zoom(2.0);
    let mut width_at_event = -1;

    state.swap_source_size(60, 60, &mut |seen: &ViewerState| {
        width_at_event = seen.source_pixel_width()
    });

    assert_eq!(width_at_event, 50);
    assert_eq!(state.source_pixel_width(), 60);
}

// C#: ViewerStateMutationGapTests.UpdateViewport_WhileFitWidth_IgnoresAnUnusableViewportWhenRememberingTheFilledDimension
// The Fit-width zoom remembers the 1000 DIP viewport width it filled; an unusable reading must not overwrite that memory.
// Rows: (1.0, 700), (1200, 1.0), (+inf, 700), (1200, +inf), (NaN, 700), (1200, NaN).
#[test]
fn mutation_gap_update_viewport_while_fit_width_ignores_an_unusable_reading_when_remembering() {
    let cases = [
        (1.0, 700.0),
        (1200.0, 1.0),
        (f64::INFINITY, 700.0),
        (1200.0, f64::INFINITY),
        (f64::NAN, 700.0),
        (1200.0, f64::NAN),
    ];
    for (viewport_width, viewport_height) in cases {
        let mut state = fitted(2000, 1000, 1000.0, 500.0);
        state.zoom_to_fit_width(); // zoom 0.5, remembers a 1000 DIP viewport width

        state.update_viewport(viewport_width, viewport_height, false);
        state.swap_source_size(2400, 1200, &mut |_: &ViewerState| {});

        assert_close(
            state.zoom(),
            1000.0 / 2400.0,
            1e-9,
            &format!("reading {viewport_width} x {viewport_height}"),
        );
    }
}

// C#: ViewerStateMutationGapTests.UpdateViewport_WhileFitWidth_RemembersTheNewUsableViewportWidth
#[test]
fn mutation_gap_update_viewport_while_fit_width_remembers_the_new_usable_viewport_width() {
    let mut state = fitted(2000, 1000, 1000.0, 500.0);
    state.zoom_to_fit_width();

    state.update_viewport(1200.0, 700.0, false);
    state.swap_source_size(2400, 1200, &mut |_: &ViewerState| {});

    assert_close(
        state.zoom(),
        1200.0 / 2400.0,
        1e-9,
        "usable 1200 DIP width is remembered",
    );
}

// C#: ViewerStateMutationGapTests.UpdateViewport_Forced_KeepsTheLimitsWhenTheReadingIsNotUsable
// Trap T7: only readings > 1 and finite write the limits.
#[test]
fn mutation_gap_update_viewport_forced_keeps_the_limits_when_the_reading_is_not_usable() {
    let cases = [
        (1.0, 600.0),
        (800.0, 1.0),
        (f64::INFINITY, 600.0),
        (800.0, f64::INFINITY),
        (f64::NAN, 600.0),
        (800.0, f64::NAN),
    ];
    for (viewport_width, viewport_height) in cases {
        let mut state = ViewerState::new();
        state.update_viewport(1000.0, 500.0, true);

        state.update_viewport(viewport_width, viewport_height, true);

        assert_eq!(
            state.max_image_width(),
            1000.0,
            "reading {viewport_width} x {viewport_height}"
        );
        assert_eq!(
            state.max_image_height(),
            500.0,
            "reading {viewport_width} x {viewport_height}"
        );
    }
}

// C#: ViewerStateMutationGapTests.CalculateStepZoom_AtTheTopEdgeTolerance_DoesNothing
#[test]
fn mutation_gap_calculate_step_zoom_at_the_top_edge_tolerance_does_nothing() {
    assert_eq!(calculate_step_zoom(MAX_STEP_ZOOM - 0.0005, 0.25), None);
}

// C#: ViewerStateMutationGapTests.CalculateStepZoom_AtTheBottomEdgeTolerance_DoesNothing
#[test]
fn mutation_gap_calculate_step_zoom_at_the_bottom_edge_tolerance_does_nothing() {
    assert_eq!(calculate_step_zoom(MIN_STEP_ZOOM + 0.0005, -0.25), None);
}

// ---- C. ViewerStateRefreshScalingTests (Q-TOUCHPAD-REFRESH: `EffectiveScalingQuality`) ----

// C#: ViewerStateRefreshScalingTests.EffectiveScalingQuality_FollowsTheSettingUnlessARefreshForcedHighQuality
// Rows: (HighQuality, not forced -> HighQuality), (Linear, not forced -> Linear), (Linear, forced -> HighQuality),
// (HighQuality, forced -> HighQuality).
#[test]
fn effective_scaling_quality_follows_setting_unless_refresh_forced_high_quality() {
    let cases = [
        (ScalingQuality::HighQuality, false, ScalingQuality::HighQuality),
        (ScalingQuality::Linear, false, ScalingQuality::Linear),
        (ScalingQuality::Linear, true, ScalingQuality::HighQuality),
        (ScalingQuality::HighQuality, true, ScalingQuality::HighQuality),
    ];
    for (setting, forced, expected) in cases {
        let mut viewer = ViewerState::new();
        viewer.set_scaling_quality(setting);
        viewer.set_force_high_quality_scaling(forced);

        assert_eq!(
            viewer.effective_scaling_quality(),
            expected,
            "setting {setting:?}, forced {forced}"
        );
    }
}

// C#: ViewerStateRefreshScalingTests.EffectiveScalingQuality_IsNotifiedWhenEitherInputChanges
// The C# count of 2 becomes "both calls contain it".
#[test]
fn effective_scaling_quality_is_reported_when_either_input_changes() {
    let mut viewer = ViewerState::new();

    let by_setting = viewer.set_scaling_quality(ScalingQuality::Linear);
    let by_force = viewer.set_force_high_quality_scaling(true);

    assert!(by_setting.contains(ViewerProperty::EffectiveScalingQuality));
    assert!(by_force.contains(ViewerProperty::EffectiveScalingQuality));
}
