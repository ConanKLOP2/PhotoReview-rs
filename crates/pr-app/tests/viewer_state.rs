//! Red tests (W8-06a, test porter) for `pr_app::viewmodels::viewer_state` (C# `ViewModels/ViewerState.cs`, baseline `d92d6aae`).
//!
//! Section A of `contracts/W8-06a.md`: the `ViewerState` part of `tests/PhotoReview.App.Tests/ViewModels/ViewerStateTests.cs`,
//! plus the rule tests marked `// new:` in "Tests (porter)". Each test names its C# twin in a `// C#:` comment. Assertions on
//! `PropertyChanged` become assertions on the returned `ViewerChanges`. Theories are loops over tables; the failing case is
//! printed in the assertion message.
//!
//! Not here (other units, contract "NOT here"): the AppSettings round trip (W2-01); the `MainWindowHelpers` tests and the
//! `FitWidthAnchor` tests (W8-04e); the click-level rule (`menus::zoom_rules`, W8-01d, already ported); `FitViewControllerTests`
//! (W8-01d). The C# "ClickLevelFromEffectiveZoom" tests are twins of `menus_zoom_rules.rs` and are not repeated here.
//!
//! Every test fails through `todo!()` until the bodies land.

use pr_app::viewmodels::viewer_state::{
    ACTUAL_SIZE_ZOOM, MAX_STEP_ZOOM, MIN_STEP_ZOOM, ViewerChanges, ViewerProperty, ViewerState, ViewerUpdate,
    calculate_display_size, calculate_step_zoom,
};
use pr_app::viewport::fit::FitViewer;
use pr_app::viewport::types::ViewerStretchMode;
use pr_core::model::enums::{InitialViewMode, ScalingQuality};
use pr_core::settings::app_settings::DEFAULT_CLICK_ZOOM_PERCENT;

/// Tolerance of xunit's `Assert.Equal(expected, actual, 6)` (6 decimal places), used by the twins that pass a precision.
fn assert_close(actual: f64, expected: f64, context: &str) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "{context}: actual {actual}, expected {expected}"
    );
}

// C#: ViewerStateTests.ScalingQuality_DefaultsToHighQualityAndNotifiesOnChange
#[test]
fn scaling_quality_defaults_to_high_quality_and_reports_change() {
    let mut state = ViewerState::new();
    assert_eq!(state.scaling_quality(), ScalingQuality::HighQuality);

    let changes = state.set_scaling_quality(ScalingQuality::Linear);

    assert!(changes.contains(ViewerProperty::ScalingQuality));
}

// C#: ViewerStateTests.DisplayZoomPercent_IsNotifiedWhenTheFitViewportBoundChanges
#[test]
fn display_zoom_percent_is_reported_when_the_fit_viewport_bound_changes() {
    let mut state = ViewerState::new();
    state.set_source_size(4000, 3000, true);
    state.reset_fit(1000.0, 750.0);
    state.zoom_to_actual_size();

    let update = state.reset_fit(1000.0, 750.0);

    assert_eq!(state.display_zoom_percent(), 25);
    assert!(update.changes.contains(ViewerProperty::DisplayZoomPercent));

    // window resized while in Fit
    let update = state.update_viewport(2000.0, 1500.0, false);

    assert_eq!(state.display_zoom_percent(), 50);
    assert!(update.changes.contains(ViewerProperty::DisplayZoomPercent));
}

// C#: ViewerStateTests.DisplayZoomPercent_IsNotifiedWhenOnlyOneViewportBoundChanges
// Theory rows: (4000x1000, 2000x750 -> 50: wide image, width bound), (1000x4000, 1000x1500 -> 38: tall image, height bound).
#[test]
fn display_zoom_percent_reported_when_only_one_bound_changes() {
    let cases = [(4000, 1000, 2000.0, 750.0, 50), (1000, 4000, 1000.0, 1500.0, 38)];
    for (source_width, source_height, width, height, expected) in cases {
        let mut state = ViewerState::new();
        state.set_source_size(source_width, source_height, true);
        state.reset_fit(1000.0, 750.0);

        let update = state.update_viewport(width, height, false);

        assert_eq!(
            state.display_zoom_percent(),
            expected,
            "source {source_width}x{source_height}, viewport {width}x{height}"
        );
        assert!(
            update.changes.contains(ViewerProperty::DisplayZoomPercent),
            "source {source_width}x{source_height}, viewport {width}x{height}"
        );
    }
}

// C#: ViewerStateTests.DefaultState_IsFitAndUniform
// Trap T1: the enum's `#[default]` is `None`, the C# default is `Uniform`; `new()` must set it.
#[test]
fn default_state_is_fit_and_uniform() {
    let state = ViewerState::new();

    assert_eq!(state.zoom(), 1.0);
    assert_eq!(state.stretch(), ViewerStretchMode::Uniform);
    assert!(state.is_fit());
    assert!(!state.is_fullscreen());
}

// C#: ViewerStateTests.SetZoom_ChangesModeToNone_ClampsBetweenMinAndMax
#[test]
fn set_zoom_changes_mode_to_none_and_clamps() {
    let mut state = ViewerState::new();

    let update = state.set_zoom(2.0);

    assert_eq!(state.zoom(), 2.0);
    assert_eq!(state.stretch(), ViewerStretchMode::None);
    assert!(!state.is_fit());
    assert_eq!(state.max_image_width(), f64::INFINITY);
    assert_eq!(state.max_image_height(), f64::INFINITY);
    assert!(update.zoom_mode_changed);

    // feat/mouse-zoom: the absolute range is 10 %..800 % (click-to-zoom range); stepping stays 25 %..400 %.
    state.set_zoom(0.05);
    assert_eq!(state.zoom(), 0.10);

    state.set_zoom(10.0);
    assert_eq!(state.zoom(), 8.0);
}

// C#: ViewerStateTests.CalculateStepZoom_StepsWithinTheSteppingRange
#[test]
fn calculate_step_zoom_steps_within_the_stepping_range() {
    let cases = [
        (1.0, 0.25, 1.25),
        (3.9, 0.25, 4.0), // up: capped at the stepping max
        (1.0, -0.25, 0.75),
        (0.3, -0.25, 0.25), // down: floored at the stepping min
        (0.10, 0.25, 0.35), // a 10 % click zoom can step up
        (8.0, -0.25, 4.0),  // from an 800 % click zoom one step down lands on 400 %
        (5.5, -0.25, 4.0),
    ];
    for (current, step, expected) in cases {
        let next = calculate_step_zoom(current, step)
            .unwrap_or_else(|| panic!("current {current}, step {step}: expected a step, got none"));
        assert_close(next, expected, &format!("current {current}, step {step}"));
    }
}

// C#: ViewerStateTests.CalculateStepZoom_NeverMovesAgainstTheStepDirection
#[test]
fn calculate_step_zoom_never_moves_against_the_step_direction() {
    let cases = [
        (4.0, 0.25),   // at the stepping max
        (8.0, 0.25),   // above it: zooming in must not shrink to 400 %
        (0.25, -0.25), // at the stepping min
        (0.10, -0.25), // below it: zooming out must not enlarge to 25 %
    ];
    for (current, step) in cases {
        assert_eq!(
            calculate_step_zoom(current, step),
            None,
            "current {current}, step {step}"
        );
    }
}

// C#: ViewerStateTests.ZoomIn_AboveStepRange_KeepsClickZoom
#[test]
fn zoom_in_above_step_range_keeps_click_zoom() {
    let mut state = ViewerState::new();
    state.set_zoom(8.0);

    state.zoom_in();
    assert_eq!(state.zoom(), 8.0);

    state.zoom_out();
    assert_eq!(state.zoom(), 4.0);
}

// C#: ViewerStateTests.ZoomIn_And_ZoomOut_StepCorrectly
#[test]
fn zoom_in_and_zoom_out_step_correctly() {
    let mut state = ViewerState::new();

    state.zoom_in();
    assert_eq!(state.zoom(), 1.25);

    state.zoom_out();
    assert_eq!(state.zoom(), 1.0);

    // zooming out repeatedly clamps at 0.25
    for _ in 0..10 {
        state.zoom_out();
    }
    assert_eq!(state.zoom(), 0.25);

    // zooming in repeatedly clamps at 4.0
    for _ in 0..20 {
        state.zoom_in();
    }
    assert_eq!(state.zoom(), 4.0);
}

// C#: ViewerStateTests.WheelZoom_PositiveAndNegative_StepsCorrectly
#[test]
fn wheel_zoom_positive_and_negative_steps_correctly() {
    let mut state = ViewerState::new();

    state.wheel_zoom(120);
    assert_eq!(state.zoom(), 1.25);

    state.wheel_zoom(-120);
    assert_eq!(state.zoom(), 1.0);
}

// C#: ViewerStateTests.ResetFit_WithViewportRestoresViewportLimits
#[test]
fn reset_fit_with_viewport_restores_viewport_limits() {
    let mut state = ViewerState::new();
    state.set_zoom(3.0);

    state.reset_fit(1280.0, 720.0);

    assert!(state.is_fit());
    assert_eq!(state.max_image_width(), 1280.0);
    assert_eq!(state.max_image_height(), 720.0);
}

// C#: ViewerStateTests.ResetFit_RestoresUniformAndCalculatesViewport
#[test]
fn reset_fit_restores_uniform_and_calculates_viewport() {
    let mut state = ViewerState::new();
    state.set_zoom(3.0);

    state.reset_fit(1920.0, 1080.0);

    assert_eq!(state.zoom(), 1.0);
    assert_eq!(state.stretch(), ViewerStretchMode::Uniform);
    assert!(state.is_fit());
    assert_eq!(state.max_image_width(), 1920.0);
    assert_eq!(state.max_image_height(), 1080.0);
}

// C#: ViewerStateTests.UpdateViewport_OnlyWhenUniform
#[test]
fn update_viewport_only_when_uniform() {
    let mut state = ViewerState::new();
    state.reset_fit(1000.0, 800.0);

    // Uniform: the viewport size is taken.
    state.update_viewport(1200.0, 900.0, false);
    assert_eq!(state.max_image_width(), 1200.0);
    assert_eq!(state.max_image_height(), 900.0);

    // Free zoom (Stretch == None) clears the limits.
    state.set_zoom(2.0);
    assert_eq!(state.max_image_width(), f64::INFINITY);

    // Stretch == None: UpdateViewport does nothing.
    state.update_viewport(1600.0, 1200.0, false);
    assert_eq!(state.max_image_width(), f64::INFINITY);
    assert_eq!(state.max_image_height(), f64::INFINITY);
}

// C#: ViewerStateTests.ApplyInitialViewMode_ConfiguresStateCorrectly
// Theory rows: Fit (1.0, Uniform), Percent100 (1.0, None), Percent200 (2.0, None), Percent400 (legacy, behaves as 200 %).
#[test]
fn apply_initial_view_mode_configures_state_correctly() {
    let cases = [
        (InitialViewMode::Fit, 1.0, ViewerStretchMode::Uniform),
        (InitialViewMode::Percent100, 1.0, ViewerStretchMode::None),
        (InitialViewMode::Percent200, 2.0, ViewerStretchMode::None),
        (InitialViewMode::Percent400, 2.0, ViewerStretchMode::None),
    ];
    for (mode, expected_zoom, expected_stretch) in cases {
        let mut state = ViewerState::new();

        let applied = state.apply_initial_view_mode(mode, 1920.0, 1080.0, DEFAULT_CLICK_ZOOM_PERCENT, false);

        assert!(applied.is_some(), "{mode:?}: expected Some");
        assert_eq!(state.zoom(), expected_zoom, "{mode:?} zoom");
        assert_eq!(state.stretch(), expected_stretch, "{mode:?} stretch");
    }
}

// C#: ViewerStateTests.ApplyInitialViewMode_FitWidth_ZoomsToFitWidthAfterRefreshingViewport
#[test]
fn apply_initial_view_mode_fit_width_zooms_after_refreshing_viewport() {
    let mut state = ViewerState::new();
    state.set_source_size(2000, 4000, false); // tall portrait: width-fit != height-fit
    state.set_dpi_scale(1.0);

    let applied = state.apply_initial_view_mode(
        InitialViewMode::FitWidth,
        1000.0,
        2000.0,
        DEFAULT_CLICK_ZOOM_PERCENT,
        false,
    );

    assert!(applied.is_some());
    assert_close(state.zoom(), 0.5, "1000 / 2000 source width");
    assert_eq!(state.stretch(), ViewerStretchMode::None);
}

// C#: ViewerStateTests.SwapSourceSize_AfterFitWidthOnPreviousImageAndSizeChange_KeepsZoomInsteadOfRefittingAgainstStaleViewport
#[test]
fn swap_after_fit_width_on_previous_image_and_size_change_keeps_zoom() {
    let mut state = ViewerState::new();
    state.set_dpi_scale(1.0);
    state.set_source_size(2000, 4000, false);
    state.apply_initial_view_mode(
        InitialViewMode::FitWidth,
        1000.0,
        2000.0,
        DEFAULT_CLICK_ZOOM_PERCENT,
        false,
    );
    assert_close(state.zoom(), 0.5, "fit width on the first image");

    // navigate to B (KeepZoomAcrossImages: zoom survives, ApplyInitialViewMode never runs)
    state.set_source_size(3000, 2000, false);
    // B's RAW original arrives
    state.swap_source_size(3010, 2007, &mut |_: &ViewerState| {});

    assert_close(state.zoom(), 0.5, "not 1000 / 3010");
}

// C#: ViewerStateTests.SwapSourceSize_AfterFitWidthOnPreviousImageAndSameSizedNewImage_KeepsZoom
#[test]
fn swap_after_fit_width_and_same_sized_new_image_keeps_zoom() {
    let mut state = ViewerState::new();
    state.set_dpi_scale(1.0);
    state.set_source_size(2000, 4000, false);
    state.apply_initial_view_mode(
        InitialViewMode::FitWidth,
        1000.0,
        2000.0,
        DEFAULT_CLICK_ZOOM_PERCENT,
        false,
    );

    // same dimensions, different image
    state.set_source_size(2000, 4000, true);
    state.swap_source_size(2010, 4020, &mut |_: &ViewerState| {});

    assert_close(state.zoom(), 0.5, "new image of the same size forgets the axis");
}

// C#: ViewerStateTests.SwapSourceSize_SameImageAfterFitWidth_StillRefitsAndSameSizeSetKeepsTheAxis
#[test]
fn swap_same_image_after_fit_width_refits_and_keeps_axis() {
    let mut state = ViewerState::new();
    state.set_dpi_scale(1.0);
    state.set_source_size(2000, 4000, false);
    state.apply_initial_view_mode(
        InitialViewMode::FitWidth,
        1000.0,
        2000.0,
        DEFAULT_CLICK_ZOOM_PERCENT,
        false,
    );

    // thumbnail -> preview of the same image: no new image
    state.set_source_size(2000, 4000, false);
    state.swap_source_size(2010, 4020, &mut |_: &ViewerState| {});

    // the fitted width still fills the viewport
    assert_close(state.zoom(), 1000.0 / 2010.0, "fitted width refits");
}

// C#: ViewerStateTests.SwapSourceSize_FitWidthThenViewportGrows_RefitsAgainstTheCurrentViewport
#[test]
fn swap_fit_width_then_viewport_grows_refits_against_current_viewport() {
    let mut state = ViewerState::new();
    state.set_dpi_scale(1.0);
    state.set_source_size(2000, 4000, false);
    state.apply_initial_view_mode(
        InitialViewMode::FitWidth,
        1000.0,
        2000.0,
        DEFAULT_CLICK_ZOOM_PERCENT,
        false,
    );

    // the window grew; zoom is not refit here (Stretch is None) but the axis viewport must follow
    state.update_viewport(1900.0, 2000.0, false);
    // same image, bigger decode
    state.swap_source_size(2010, 4020, &mut |_: &ViewerState| {});

    // not 1000 / 2010 (stale)
    assert_close(state.zoom(), 1900.0 / 2010.0, "refit against the grown viewport");
}

// C#: ViewerStateTests.SwapSourceSize_FitHeightThenViewportGrows_RefitsAgainstTheCurrentViewport
#[test]
fn swap_fit_height_then_viewport_grows_refits_against_current_viewport() {
    let mut state = ViewerState::new();
    state.set_dpi_scale(1.0);
    state.set_source_size(4000, 2000, false);
    state.apply_initial_view_mode(
        InitialViewMode::FitHeight,
        2000.0,
        1000.0,
        DEFAULT_CLICK_ZOOM_PERCENT,
        false,
    );

    state.update_viewport(2000.0, 1700.0, false);
    state.swap_source_size(4020, 2010, &mut |_: &ViewerState| {});

    assert_close(state.zoom(), 1700.0 / 2010.0, "refit height against the grown viewport");
}

// C#: ViewerStateTests.ApplyInitialViewMode_FitHeight_ZoomsToFitHeightAfterRefreshingViewport
#[test]
fn apply_initial_view_mode_fit_height_zooms_after_refreshing_viewport() {
    let mut state = ViewerState::new();
    state.set_source_size(2000, 4000, false);
    state.set_dpi_scale(1.0);

    let applied = state.apply_initial_view_mode(
        InitialViewMode::FitHeight,
        1000.0,
        2000.0,
        DEFAULT_CLICK_ZOOM_PERCENT,
        false,
    );

    assert!(applied.is_some());
    assert_close(state.zoom(), 0.5, "2000 / 4000 source height");
    assert_eq!(state.stretch(), ViewerStretchMode::None);
}

// C#: ViewerStateTests.ApplyInitialViewMode_ClickZoomLevel_ZoomsToConfiguredPercent
#[test]
fn apply_initial_view_mode_click_zoom_level_zooms_to_configured_percent() {
    let mut state = ViewerState::new();

    let applied = state.apply_initial_view_mode(InitialViewMode::ClickZoomLevel, 1920.0, 1080.0, 150, false);

    assert!(applied.is_some());
    assert_close(state.zoom(), 1.5, "150 percent click level");
}

// C#: ViewerStateTests.ApplyInitialViewMode_KeepZoomAcrossImages_IsANoOpAndReturnsFalse
#[test]
fn apply_initial_view_mode_keep_zoom_is_a_no_op_and_returns_none() {
    let mut state = ViewerState::new();
    state.set_zoom(3.0);

    let applied = state.apply_initial_view_mode(InitialViewMode::Fit, 1920.0, 1080.0, DEFAULT_CLICK_ZOOM_PERCENT, true);

    assert_eq!(applied, None);
    // untouched: still the zoom from before the image change
    assert_eq!(state.zoom(), 3.0);
    assert!(!state.is_fit());
}

// C#: ViewerStateTests.FitWidthZoom_FillsViewportWidthAtDpiScale
// Theory rows: (1000x500, 100 % -> 1.0: width alone decides), (1000x500, 200 % -> 2.0: twice the zoom at DPI 2.0).
#[test]
fn fit_width_zoom_fills_viewport_width_at_dpi_scale() {
    let cases = [(1000, 500, 100, 1.0), (1000, 500, 200, 2.0)];
    for (source_width, source_height, dpi_percent, expected_zoom) in cases {
        let mut state = ViewerState::new();
        state.set_source_size(source_width, source_height, false);
        state.set_dpi_scale(f64::from(dpi_percent) / 100.0);
        // viewport DIP width == source width at 100 % DPI
        state.update_viewport(f64::from(source_width), 10000.0, true);

        assert_close(
            state.fit_width_zoom(),
            expected_zoom,
            &format!("source {source_width}x{source_height} at {dpi_percent} percent"),
        );
    }
}

// C#: ViewerStateTests.FitWidthZoom_UnknownSourceOrViewport_IsZero
#[test]
fn fit_width_zoom_unknown_source_or_viewport_is_zero() {
    let mut state = ViewerState::new();
    assert_eq!(state.fit_width_zoom(), 0.0);

    state.set_source_size(1000, 1000, false);
    // viewport still unknown (Infinity)
    assert_eq!(state.fit_width_zoom(), 0.0);
}

// C#: ViewerStateTests.FitHeightZoom_FillsViewportHeightAtDpiScale
// Theory rows: (500x1000, 100 % -> 1.0), (500x1000, 200 % -> 2.0).
#[test]
fn fit_height_zoom_fills_viewport_height_at_dpi_scale() {
    let cases = [(500, 1000, 100, 1.0), (500, 1000, 200, 2.0)];
    for (source_width, source_height, dpi_percent, expected_zoom) in cases {
        let mut state = ViewerState::new();
        state.set_source_size(source_width, source_height, false);
        state.set_dpi_scale(f64::from(dpi_percent) / 100.0);
        state.update_viewport(10000.0, f64::from(source_height), true);

        assert_close(
            state.fit_height_zoom(),
            expected_zoom,
            &format!("source {source_width}x{source_height} at {dpi_percent} percent"),
        );
    }
}

// C#: ViewerStateTests.FitHeightZoom_UnknownSourceOrViewport_IsZero
#[test]
fn fit_height_zoom_unknown_source_or_viewport_is_zero() {
    let state = ViewerState::new();
    assert_eq!(state.fit_height_zoom(), 0.0);
}

// C#: ViewerStateTests.ZoomToFitWidth_UnknownSize_DoesNotChangeZoom
#[test]
fn zoom_to_fit_width_unknown_size_does_not_change_zoom() {
    let mut state = ViewerState::new();
    state.set_zoom(2.0);

    // FitWidthZoom is 0 (no source size): must not clobber the current zoom
    state.zoom_to_fit_width();

    assert_eq!(state.zoom(), 2.0);
}

// C#: ViewerStateTests.ZoomToFitHeight_UnknownSize_DoesNotChangeZoom
#[test]
fn zoom_to_fit_height_unknown_size_does_not_change_zoom() {
    let mut state = ViewerState::new();
    state.set_zoom(2.0);

    state.zoom_to_fit_height();

    assert_eq!(state.zoom(), 2.0);
}

// C#: ViewerStateTests.Fullscreen_ToggleAndExit
#[test]
fn fullscreen_toggle_and_exit() {
    let mut state = ViewerState::new();
    assert!(!state.is_fullscreen());

    state.toggle_fullscreen();
    assert!(state.is_fullscreen());

    state.toggle_fullscreen();
    assert!(!state.is_fullscreen());

    state.toggle_fullscreen();
    assert!(state.is_fullscreen());

    state.exit_fullscreen();
    assert!(!state.is_fullscreen());
}

// ---- rule tests of "Tests (porter)" (new, no C# twin of their own) ----

// new: change_set_names_are_csharp_names_in_all_order (contract D-06a-1, `ViewerChanges::names` order)
#[test]
fn change_set_names_are_csharp_names_in_all_order() {
    let mut changes = ViewerChanges::EMPTY;
    assert!(changes.is_empty());

    changes.insert(ViewerProperty::IsFit);
    changes.insert(ViewerProperty::Zoom);

    assert_eq!(changes.names(), vec!["Zoom", "IsFit"]);
    assert!(changes.contains(ViewerProperty::IsFit));
    assert!(!changes.contains(ViewerProperty::Stretch));

    let csharp: Vec<&str> = ViewerProperty::ALL
        .iter()
        .map(|property| property.csharp_name())
        .collect();
    assert_eq!(
        csharp,
        [
            "Zoom",
            "Stretch",
            "SourcePixelWidth",
            "SourcePixelHeight",
            "DpiScale",
            "IsFullscreen",
            "ScalingQuality",
            "ForceHighQualityScaling",
            "MaxImageWidth",
            "MaxImageHeight",
            "IsFit",
            "ImageWidth",
            "ImageHeight",
            "DisplayZoomPercent",
            "EffectiveScalingQuality",
        ]
    );

    let mut every = ViewerChanges::EMPTY;
    for property in ViewerProperty::ALL {
        every.insert(property);
    }
    assert_eq!(every.names(), csharp);
}

// new: setting_equal_value_returns_no_change (D-W8-1 change-set rule; traps T13 and T14)
#[test]
fn setting_equal_value_returns_no_change() {
    let mut state = ViewerState::new();

    // 1.0 is the stored zoom of a fresh state
    assert!(state.set_zoom_raw(1.0).is_empty());
    // Stretch is already Uniform
    assert!(state.set_stretch(ViewerStretchMode::Uniform).is_empty());

    // NaN is stored as NaN the first time
    let first = state.set_zoom_raw(f64::NAN);
    assert!(first.contains(ViewerProperty::Zoom));

    // NaN equals NaN (`f64_same`), so the second store reports nothing
    assert!(state.set_zoom_raw(f64::NAN).is_empty());
}

// new: set_source_size_negative_is_clamped_to_zero_and_changes_nothing_when_already_zero (trap T15)
#[test]
fn set_source_size_negative_is_clamped_to_zero_and_changes_nothing_when_already_zero() {
    let mut state = ViewerState::new();

    let update = state.set_source_size(-5, -3, false);

    assert!(update.changes.is_empty(), "0 x 0 is already the stored size");
    assert_eq!(state.source_pixel_width(), 0);
    assert_eq!(state.source_pixel_height(), 0);

    // a -5 change from 2000 is a change to 0 (clamped, then compared)
    state.set_source_size(2000, 1000, false);
    let update = state.set_source_size(-5, 1000, false);

    assert_eq!(state.source_pixel_width(), 0);
    assert!(update.changes.contains(ViewerProperty::SourcePixelWidth));
}

// new: swap_source_size_callback_runs_once_when_zoomed_and_never_otherwise (contract section 3 and trap T5)
#[test]
fn swap_source_size_callback_runs_once_when_zoomed_and_never_otherwise() {
    // zoomed: announced exactly once
    let mut zoomed = ViewerState::new();
    zoomed.set_source_size(50, 50, false);
    zoomed.set_zoom(2.0);
    let mut zoomed_calls = 0;
    zoomed.swap_source_size(60, 60, &mut |_: &ViewerState| zoomed_calls += 1);
    assert_eq!(zoomed_calls, 1, "zoomed swap announces once");

    // the same size again: no change, no announcement
    let mut same_calls = 0;
    zoomed.swap_source_size(60, 60, &mut |_: &ViewerState| same_calls += 1);
    assert_eq!(same_calls, 0, "same size after clamping: no callback");

    // in Fit: never announced
    let mut fit = ViewerState::new();
    fit.set_source_size(50, 50, false);
    let mut fit_calls = 0;
    fit.swap_source_size(60, 60, &mut |_: &ViewerState| fit_calls += 1);
    assert_eq!(fit_calls, 0, "Fit swap never announces");
}

// new: set_zoom_nan_is_a_full_no_op (trap T13)
#[test]
fn set_zoom_nan_is_a_full_no_op() {
    let mut state = ViewerState::new();
    state.set_source_size(1000, 800, false);
    let before = state.clone();

    let update = state.set_zoom(f64::NAN);

    assert_eq!(update, ViewerUpdate::default());
    assert_eq!(state, before, "no Stretch, limit, axis or flag change");
}

// new: take_pending_zoom_mode_changed_reports_and_clears_the_fit_driver_flag (D-06a-4, OQ-1)
#[test]
fn take_pending_zoom_mode_changed_reports_and_clears_the_fit_driver_flag() {
    let mut state = ViewerState::new();

    // no other method touches the flag
    state.set_zoom(2.0);
    assert!(!state.take_pending_zoom_mode_changed());

    FitViewer::reset_fit(&mut state, 800.0, 600.0);

    assert!(state.take_pending_zoom_mode_changed());
    assert!(!state.take_pending_zoom_mode_changed(), "the read clears the flag");
}

// new: fit_viewer_reset_fit_matches_inherent_reset_fit (D-06a-4)
#[test]
fn fit_viewer_reset_fit_matches_inherent_reset_fit() {
    let mut via_inherent = ViewerState::new();
    via_inherent.set_source_size(4000, 3000, true);
    let mut via_seam = via_inherent.clone();

    via_inherent.reset_fit(1000.0, 750.0);
    FitViewer::reset_fit(&mut via_seam, 1000.0, 750.0);

    assert!(via_seam.take_pending_zoom_mode_changed());
    assert_eq!(via_inherent, via_seam, "the seam leaves the same state, flag drained");
}

// new: math_min_max_propagate_nan_like_csharp (trap T3): `Math.Min`/`Math.Max` return NaN when either operand is NaN.
// Observed through `calculate_step_zoom`: a NaN current or step reaches math_min/math_max and comes back as Some(NaN).
#[test]
fn math_min_max_propagate_nan_like_csharp() {
    let up = calculate_step_zoom(f64::NAN, 0.25);
    assert!(up.is_some_and(f64::is_nan), "step up from NaN: {up:?}");

    let down = calculate_step_zoom(f64::NAN, -0.25);
    assert!(down.is_some_and(f64::is_nan), "step down from NaN: {down:?}");

    let nan_step = calculate_step_zoom(1.0, f64::NAN);
    assert!(
        nan_step.is_some_and(f64::is_nan),
        "a NaN step takes the else branch: {nan_step:?}"
    );
}

// new: display_zoom_percent_ties_to_even (trap T9, D-06a-7): C# Math.Round(double) rounds 12.5 -> 12 and 62.5 -> 62.
#[test]
fn display_zoom_percent_ties_to_even() {
    let cases = [(0.125, 12), (0.375, 38), (0.625, 62)];
    for (zoom, expected) in cases {
        let mut state = ViewerState::new();
        state.set_zoom_raw(zoom);

        assert_eq!(state.display_zoom_percent(), expected, "zoom {zoom}");
    }
}

// new: the stepping and display constants keep their C# values (contract "Public API")
#[test]
fn constants_keep_their_csharp_values() {
    assert_eq!(MAX_STEP_ZOOM, 4.0);
    assert_eq!(MIN_STEP_ZOOM, 0.25);
    assert_eq!(ACTUAL_SIZE_ZOOM, 1.0);
    // the display helper: 1000 x 800 at zoom 1 and 100 % DPI keeps its size
    assert_eq!(calculate_display_size(1000, 800, 1.0, 1.0), (1000.0, 800.0));
}

// new: the `ViewerStretchMode` default is not the state default (trap T1), so a fresh state is Uniform, not None.
#[test]
fn fresh_state_is_uniform_not_the_enum_default() {
    assert_eq!(ViewerStretchMode::default(), ViewerStretchMode::None);
    assert_eq!(ViewerState::new().stretch(), ViewerStretchMode::Uniform);
}

// new: IsFit tolerance is strict (trap T2): |zoom - 1| < 0.001, so 1.0009 is Fit and 1.001 is not.
#[test]
fn is_fit_tolerance_is_strict() {
    let mut inside = ViewerState::new();
    inside.set_zoom_raw(1.0009);
    assert!(inside.is_fit(), "1.0009 is within 0.001 of 1.0");

    let mut boundary = ViewerState::new();
    // 1.001 - 1.0 is 0.00099999999999988987 in IEEE doubles (below 0.001), so C# also reports Fit for 1.001; use a value
    // that is really outside the tolerance.
    boundary.set_zoom_raw(1.0011);
    assert!(!boundary.is_fit(), "1.0011 is outside 0.001");
}
