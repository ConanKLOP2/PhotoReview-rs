//! Red tests (W8-05e, test porter) for `pr_app::input::mouse_gestures`. Port of the C# tests at baseline `d92d6aae`:
//! `tests/PhotoReview.App.Tests/Input/MouseGesturesTests.cs` (18 tests), `MouseGesturesMutationGapTests.cs` (2) and
//! `MiddleClickResolverTests.cs` (4 of 5 names, see the skips). Every `#[test]` here fails through `todo!()` until the bodies land.
//! Theories are one `#[test]` with a loop over the C# rows. Each test carries a `// C#:` comment naming the C# test.
//!
//! Documented skips (not ported here):
//! - `MouseGesturesTests.ZoomToPoint_*` (4 tests): `MainWindowHelpers.CalculateZoomToPointOffsets` is owned by W8-04e (`misc::window_helpers`).
//! - `MiddleClickResolverTests.Resolve_NoneAndUndefined_DoNothing`, the `(MiddleClickAction)99` and `(-1)` rows: a Rust
//!   `MiddleClickAction` is a closed enum (contract "Traps" 11). Only the `None` row is ported.
//! - `WheelMessageSource_*`: W8-05a (`input::wheel_message`).
//!
//! Golden-driven checks of the same functions live in `golden_gestures.rs`.

use pr_app::input::mouse_gestures::{
    ClickZoomTarget, PointerReleaseAction, SAME_ZOOM_TOLERANCE, WheelGestureInterpreter, WheelOutcomeKind,
    classify_release, click_zoom_factor, decide_click_zoom, is_fit_double_click, resolve_keyboard_zoom_anchor,
    resolve_middle_click,
};
use pr_app::input::primitives::PointD;
use pr_app::input::review_command::ReviewCommand;
use pr_core::model::enums::{KeyboardZoomAnchor, MiddleClickAction, MouseWheelAction};
use pr_core::settings::app_settings::AppSettings;

fn wheel() -> WheelGestureInterpreter {
    WheelGestureInterpreter::new()
}

/// C# `Assert.Equal(expected, actual, decimals)`: equal when rounded to `decimals` places (within half a unit of that place).
fn assert_close(actual: f64, expected: f64, decimals: i32) {
    let tolerance = 0.5 * 10f64.powi(-decimals);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected} (to {decimals} decimals)"
    );
}

// ---- wheel ----

#[test]
fn wheel_zoom_mode_every_event_zooms() {
    // C#: MouseGesturesTests.Wheel_ZoomMode_EveryEventZooms (theory, rows 120, -120, 15)
    for delta in [120, -120, 15] {
        let mut wheel = wheel();
        assert_eq!(
            wheel.handle(delta, false, MouseWheelAction::Zoom),
            WheelOutcomeKind::Zoom,
            "delta {delta}"
        );
    }
}

#[test]
fn wheel_ctrl_always_zooms() {
    // C#: MouseGesturesTests.Wheel_CtrlAlwaysZooms (theory, modes Zoom and Navigate)
    for mode in [MouseWheelAction::Zoom, MouseWheelAction::Navigate] {
        let mut wheel = wheel();
        assert_eq!(wheel.handle(-120, true, mode), WheelOutcomeKind::Zoom, "mode {mode:?}");
    }
}

#[test]
fn wheel_navigate_mode_down_is_next_up_is_previous() {
    // C#: MouseGesturesTests.Wheel_NavigateMode_DownIsNextUpIsPrevious
    let mut wheel = wheel();

    assert_eq!(
        wheel.handle(-120, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::Next
    );
    assert_eq!(
        wheel.handle(120, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::Previous
    );
}

#[test]
fn wheel_navigate_mode_high_resolution_deltas_accumulate_to_one_notch() {
    // C#: MouseGesturesTests.Wheel_NavigateMode_HighResolutionDeltasAccumulateToOneNotch
    let mut wheel = wheel();

    for _ in 0..3 {
        assert_eq!(
            wheel.handle(-30, false, MouseWheelAction::Navigate),
            WheelOutcomeKind::None
        );
    }
    assert_eq!(
        wheel.handle(-30, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::Next
    );
    assert_eq!(
        wheel.handle(-30, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );
}

#[test]
fn wheel_navigate_mode_reversing_drops_the_partial_notch() {
    // C#: MouseGesturesTests.Wheel_NavigateMode_ReversingDropsThePartialNotch
    let mut wheel = wheel();

    assert_eq!(
        wheel.handle(-100, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );
    assert_eq!(
        wheel.handle(40, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );
    assert_eq!(
        wheel.handle(80, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::Previous
    );
}

#[test]
fn wheel_navigate_mode_coarse_event_moves_one_image_and_drops_the_excess() {
    // C#: MouseGesturesTests.Wheel_NavigateMode_CoarseEventMovesOneImageAndDropsTheExcess
    let mut wheel = wheel();

    assert_eq!(
        wheel.handle(-360, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::Next
    );
    assert_eq!(
        wheel.handle(-60, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );
}

#[test]
fn wheel_ctrl_zoom_clears_a_partial_navigation_notch() {
    // C#: MouseGesturesTests.Wheel_CtrlZoomClearsAPartialNavigationNotch
    let mut wheel = wheel();
    wheel.handle(-100, false, MouseWheelAction::Navigate);

    wheel.handle(-120, true, MouseWheelAction::Navigate);

    assert_eq!(
        wheel.handle(-40, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );
}

// ---- click vs drag ----

#[test]
fn release_without_drag_is_a_click_zoom() {
    // C#: MouseGesturesTests.Release_WithoutDrag_IsAClickZoom
    assert_eq!(
        classify_release(false, false, false, true, true),
        PointerReleaseAction::ClickZoom
    );
    assert_eq!(
        classify_release(false, true, false, true, true),
        PointerReleaseAction::ClickZoom
    );
}

#[test]
fn release_after_drag_pan_never_toggles_zoom() {
    // C#: MouseGesturesTests.Release_AfterDragPan_NeverTogglesZoom (theory: kinetic true -> StartKinetic, false -> None)
    assert_eq!(
        classify_release(true, true, false, true, true),
        PointerReleaseAction::StartKinetic
    );
    assert_eq!(
        classify_release(true, true, false, true, false),
        PointerReleaseAction::None
    );
}

#[test]
fn release_drag_without_pan_does_nothing() {
    // C#: MouseGesturesTests.Release_DragWithoutPan_DoesNothing
    // In Fit nothing scrolls: a drag is neither a click nor a glide.
    assert_eq!(
        classify_release(true, false, false, true, true),
        PointerReleaseAction::None
    );
}

#[test]
fn release_press_that_stopped_a_glide_is_not_a_click() {
    // C#: MouseGesturesTests.Release_PressThatStoppedAGlide_IsNotAClick
    assert_eq!(
        classify_release(false, true, true, true, true),
        PointerReleaseAction::None
    );
}

#[test]
fn release_click_to_zoom_disabled_is_not_a_click() {
    // C#: MouseGesturesTests.Release_ClickToZoomDisabled_IsNotAClick
    assert_eq!(
        classify_release(false, false, false, false, true),
        PointerReleaseAction::None
    );
}

#[test]
fn double_click_press_is_fit() {
    // C#: MouseGesturesTests.DoubleClickPress_IsFit (theory: 1 -> false, 2 -> true, 3 -> true)
    for (click_count, expected) in [(1, false), (2, true), (3, true)] {
        assert_eq!(is_fit_double_click(click_count), expected, "click count {click_count}");
    }
}

// ---- click-to-zoom target ----

#[test]
fn click_zoom_toggles_between_fit_and_the_click_zoom() {
    // C#: MouseGesturesTests.ClickZoom_TogglesBetweenFitAndTheClickZoom (theory, six rows)
    let rows = [
        (true, 1.0, 1.0, ClickZoomTarget::ClickZoom),
        (false, 2.0, 1.0, ClickZoomTarget::ClickZoom),
        (false, 0.5, 1.0, ClickZoomTarget::ClickZoom),
        (false, 1.0, 1.0, ClickZoomTarget::Fit),
        (false, 2.0, 2.0, ClickZoomTarget::Fit),
        (false, 1.0005, 1.0, ClickZoomTarget::Fit),
    ];
    for (is_fit, zoom, click_zoom, expected) in rows {
        assert_eq!(
            decide_click_zoom(is_fit, zoom, click_zoom),
            expected,
            "is_fit {is_fit}, zoom {zoom}, click zoom {click_zoom}"
        );
    }
}

#[test]
fn click_zoom_factor_is_percent_of_source_pixels_clamped() {
    // C#: MouseGesturesTests.ClickZoomFactor_IsPercentOfSourcePixelsClamped (theory, 6 decimals)
    for (percent, expected) in [(100, 1.0), (250, 2.5), (5, 0.10), (2000, 8.0)] {
        assert_close(click_zoom_factor(percent), expected, 6);
    }
}

// ---- zoom to point (keyboard anchor part; the ZoomToPoint_* tests are W8-04e) ----

#[test]
fn resolve_keyboard_zoom_anchor_pointer_mode_over_the_viewport_uses_the_pointer() {
    // C#: MouseGesturesTests.ResolveKeyboardZoomAnchor_PointerMode_OverTheViewport_UsesThePointer
    let anchor = resolve_keyboard_zoom_anchor(
        KeyboardZoomAnchor::Pointer,
        Some(PointD { x: 120.0, y: 40.0 }),
        800.0,
        600.0,
    );

    assert_eq!(anchor, PointD { x: 120.0, y: 40.0 });
}

#[test]
fn resolve_keyboard_zoom_anchor_pointer_mode_outside_the_viewport_uses_the_centre() {
    // C#: MouseGesturesTests.ResolveKeyboardZoomAnchor_PointerMode_OutsideTheViewport_UsesTheCentre
    let anchor = resolve_keyboard_zoom_anchor(KeyboardZoomAnchor::Pointer, None, 800.0, 600.0);

    assert_eq!(anchor, PointD { x: 400.0, y: 300.0 });
}

#[test]
fn resolve_keyboard_zoom_anchor_viewport_centre_mode_ignores_the_pointer_even_when_it_is_over_the_viewport() {
    // C#: MouseGesturesTests.ResolveKeyboardZoomAnchor_ViewportCentreMode_IgnoresThePointer_EvenWhenItIsOverTheViewport
    let anchor = resolve_keyboard_zoom_anchor(
        KeyboardZoomAnchor::ViewportCentre,
        Some(PointD { x: 120.0, y: 40.0 }),
        800.0,
        600.0,
    );

    assert_eq!(anchor, PointD { x: 400.0, y: 300.0 });
}

// ---- mutation gap tests ----

#[test]
fn decide_click_zoom_zoom_difference_exactly_the_tolerance_zooms_in_instead_of_returning_to_fit() {
    // C#: MouseGesturesMutationGapTests.DecideClickZoom_ZoomDifferenceExactlyTheTolerance_ZoomsInInsteadOfReturningToFit
    // |0.001 - 0| == SameZoomTolerance: not "the same zoom".
    assert_eq!(
        decide_click_zoom(false, SAME_ZOOM_TOLERANCE, 0.0),
        ClickZoomTarget::ClickZoom
    );
}

#[test]
fn decide_click_zoom_zoom_difference_just_inside_the_tolerance_returns_to_fit() {
    // C#: MouseGesturesMutationGapTests.DecideClickZoom_ZoomDifferenceJustInsideTheTolerance_ReturnsToFit
    assert_eq!(decide_click_zoom(false, 2.0005, 2.0), ClickZoomTarget::Fit);
}

// ---- middle click ----

#[test]
fn resolve_middle_click_each_action_maps_to_its_command() {
    // C#: MiddleClickResolverTests.Resolve_EachAction_MapsToItsCommand (theory, 10 rows)
    let rows = [
        (MiddleClickAction::ClickZoom, ReviewCommand::CLICK_ZOOM),
        (MiddleClickAction::ActualSize, ReviewCommand::ZOOM_ACTUAL_SIZE),
        (MiddleClickAction::Fit, ReviewCommand::TOGGLE_FIT),
        (MiddleClickAction::FitWidth, ReviewCommand::FIT_WIDTH),
        (MiddleClickAction::FitWidth2, ReviewCommand::FIT_WIDTH2),
        (MiddleClickAction::PreviousImage, ReviewCommand::PREVIOUS),
        (MiddleClickAction::NextImage, ReviewCommand::NEXT),
        (MiddleClickAction::PreviousFolder, ReviewCommand::PREVIOUS_FOLDER),
        (MiddleClickAction::NextFolder, ReviewCommand::NEXT_FOLDER),
        (MiddleClickAction::OpenFolder, ReviewCommand::OPEN_FOLDER),
    ];
    for (action, command) in rows {
        assert_eq!(resolve_middle_click(action), Some(command), "{action:?}");
    }
}

#[test]
fn resolve_none_does_nothing() {
    // C#: MiddleClickResolverTests.Resolve_NoneAndUndefined_DoNothing (only the `None` row is portable, contract Traps 11)
    assert_eq!(resolve_middle_click(MiddleClickAction::None), None);
}

#[test]
fn resolve_every_defined_action_except_none_resolves_to_a_command() {
    // C#: MiddleClickResolverTests.Resolve_EveryDefinedActionExceptNone_ResolvesToACommand
    // Rust has no `Enum.GetValues`: the ten non-None variants are listed explicitly.
    let defined = [
        MiddleClickAction::ClickZoom,
        MiddleClickAction::ActualSize,
        MiddleClickAction::Fit,
        MiddleClickAction::FitWidth,
        MiddleClickAction::FitWidth2,
        MiddleClickAction::PreviousImage,
        MiddleClickAction::NextImage,
        MiddleClickAction::PreviousFolder,
        MiddleClickAction::NextFolder,
        MiddleClickAction::OpenFolder,
    ];
    for action in defined {
        assert!(
            resolve_middle_click(action).is_some(),
            "{action:?} resolves to no command"
        );
    }
}

#[test]
fn default_setting_is_fullsize() {
    // C#: MiddleClickResolverTests.DefaultSetting_IsFullsize
    let action = AppSettings::default().middle_click_action_or_default();

    assert_eq!(action, MiddleClickAction::ActualSize);
    assert_eq!(resolve_middle_click(action), Some(ReviewCommand::ZOOM_ACTUAL_SIZE));
}

// ---- new: hand-written extra tests (contract "Done when") ----

#[test]
fn wheel_handle_zero_delta_in_zoom_mode_is_none_without_touching_the_sum() {
    // new: trap 9 and Semantics 1(a). A zero delta in Zoom mode clears the partial navigation sum (the clear runs before the
    // zero check) and returns None. In Navigate mode a zero delta keeps the sum: -100 then 0 then -20 is one Next.
    let mut zoom_clears = wheel();
    assert_eq!(
        zoom_clears.handle(-100, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );
    assert_eq!(
        zoom_clears.handle(0, false, MouseWheelAction::Zoom),
        WheelOutcomeKind::None
    );
    assert_eq!(
        zoom_clears.handle(-20, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );

    let mut navigate_keeps = wheel();
    assert_eq!(
        navigate_keeps.handle(-100, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );
    assert_eq!(
        navigate_keeps.handle(0, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::None
    );
    assert_eq!(
        navigate_keeps.handle(-20, false, MouseWheelAction::Navigate),
        WheelOutcomeKind::Next
    );
}
