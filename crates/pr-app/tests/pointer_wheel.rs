// Each integration binary uses a different subset of the shared fixture.
#[allow(dead_code)]
#[path = "common/pointer_fixture.rs"]
mod pointer_fixture;

use pointer_fixture::{Fixture, point};
use pr_app::input::pointer::PointerStep;
use pr_app::input::touchpad_gestures::{WheelDeviceHint, WheelInput};
use pr_core::model::enums::{KeyboardZoomAnchor, MouseWheelAction};

fn wheel(f: &mut Fixture, delta: i32, ctrl: bool) {
    f.run(|c, ctx| c.on_wheel_vertical(ctx, delta, ctrl, point(400.0, 300.0)));
}
fn touchpad(f: &mut Fixture, delta: i32, horizontal: bool, ctrl: bool, time: i32) {
    f.run(|c, ctx| c.on_wheel(ctx, WheelInput::new(delta, horizontal, ctrl, time), point(400.0, 300.0)));
}

// C#: PointerInputControllerTests.Wheel_ZoomMode_ZoomsAndScrollsToKeepThePointUnderTheCursor
#[test]
fn wheel_zoom_mode_zooms_and_scrolls_to_keep_the_point_under_the_cursor() {
    let mut f = Fixture::new();
    wheel(&mut f, 120, false);
    assert!(!f.viewer.is_fit());
    assert_eq!(f.yields, 1);
    assert_eq!(f.surface.scrolls.len(), 1);
    assert_eq!(f.commands.next + f.commands.previous, 0);
}

// C#: PointerInputControllerTests.Wheel_NavigateMode_OneNotchDownGoesToTheNextImage_CtrlZoomsInstead
#[test]
fn wheel_navigate_mode_one_notch_down_goes_to_the_next_image_ctrl_zooms_instead() {
    let mut f = Fixture::new();
    f.settings.mouse_wheel_action = MouseWheelAction::Navigate;
    wheel(&mut f, -120, false);
    assert_eq!(f.commands.next, 1);
    assert!(f.viewer.is_fit());
    wheel(&mut f, 120, false);
    assert_eq!(f.commands.previous, 1);
    wheel(&mut f, -120, true);
    assert_eq!(f.commands.next, 1);
    assert!(!f.viewer.is_fit());
}

// C#: PointerInputControllerTests.Wheel_NavigateMode_WithoutImages_DoesNotNavigate
#[test]
fn wheel_navigate_mode_without_images_does_not_navigate() {
    let mut f = Fixture::new();
    f.settings.mouse_wheel_action = MouseWheelAction::Navigate;
    f.commands.has_images = false;
    wheel(&mut f, -120, false);
    assert_eq!(f.commands.next, 0);
}

// C#: PointerInputControllerTests.Touchpad_AtFit_AVerticalSwipeChangesImagesByDistance_WithoutZooming
#[test]
fn touchpad_at_fit_a_vertical_swipe_changes_images_by_distance_without_zooming() {
    let mut f = Fixture::new();
    f.settings.touchpad_swipe_distance_per_image = 200;
    for i in 0..72 {
        touchpad(&mut f, -10, false, false, 10_008 + i * 8);
    }
    assert_eq!(f.commands.next, 4);
    assert_eq!(f.commands.previous, 0);
    assert!(f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.Touchpad_AtFit_SwipingUpGoesBack
#[test]
fn touchpad_at_fit_swiping_up_goes_back() {
    let mut f = Fixture::new();
    for i in 0..10 {
        touchpad(&mut f, 10, false, false, 10_008 + i * 8);
    }
    assert_eq!(f.commands.previous, 1);
    assert_eq!(f.commands.next, 0);
}

// C#: PointerInputControllerTests.Touchpad_AtFit_TheMouseWheelModeDoesNotMatter
#[test]
fn touchpad_at_fit_the_mouse_wheel_mode_does_not_matter() {
    let mut f = Fixture::new();
    f.settings.mouse_wheel_action = MouseWheelAction::Navigate;
    f.settings.touchpad_swipe_distance_per_image = 600;
    for i in 0..24 {
        touchpad(&mut f, -10, false, false, 10_008 + i * 8);
    }
    assert_eq!(f.commands.next, 0);
}

// C#: PointerInputControllerTests.Touchpad_AtFit_ASidewaysSwipeIsIgnored
#[test]
fn touchpad_at_fit_a_sideways_swipe_is_ignored() {
    let mut f = Fixture::new();
    for i in 0..50 {
        touchpad(&mut f, -10, true, false, 10_008 + i * 8);
    }
    assert_eq!(f.commands.next + f.commands.previous, 0);
    assert!(f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.Touchpad_Zoomed_PansBothAxes_AndNeverNavigatesOrZooms
#[test]
fn touchpad_zoomed_pans_both_axes_and_never_navigates_or_zooms() {
    let mut f = Fixture::new();
    f.zoomed_with_source(4000, 3000);
    f.surface.vertical = 400.0;
    let zoom = f.viewer.zoom();
    touchpad(&mut f, -30, false, false, 10_008);
    assert_eq!(f.surface.scrolls.last(), Some(&(500.0, 430.0)));
    touchpad(&mut f, 45, true, false, 10_016);
    assert_eq!(f.surface.scrolls.last(), Some(&(545.0, 430.0)));
    touchpad(&mut f, 20, false, false, 10_024);
    assert_eq!(f.surface.scrolls.last(), Some(&(545.0, 410.0)));
    assert_eq!(f.commands.next + f.commands.previous, 0);
    assert_eq!(f.viewer.zoom(), zoom);
}

// C#: PointerInputControllerTests.Touchpad_Zoomed_PanIsClampedToTheScrollableRange
#[test]
fn touchpad_zoomed_pan_is_clamped_to_the_scrollable_range() {
    let mut f = Fixture::new();
    f.zoomed_with_source(4000, 3000);
    f.surface.vertical = 400.0;
    touchpad(&mut f, 5000, false, false, 10_008);
    assert_eq!(f.surface.scrolls.last(), Some(&(500.0, 0.0)));
}

// C#: PointerInputControllerTests.Touchpad_Pinch_CtrlStillZoomsAtTheCursor
#[test]
fn touchpad_pinch_ctrl_still_zooms_at_the_cursor() {
    let mut f = Fixture::new();
    touchpad(&mut f, -7, false, true, 10_008);
    assert!(!f.viewer.is_fit());
    assert_eq!(f.commands.next + f.commands.previous, 0);
}

// C#: PointerInputControllerTests.MouseWheel_WholeNotches_KeepTheConfiguredZoom
#[test]
fn mouse_wheel_whole_notches_keep_the_configured_zoom() {
    let mut f = Fixture::new();
    touchpad(&mut f, -120, false, false, 5000);
    assert!(!f.viewer.is_fit());
    assert_eq!(f.commands.next + f.commands.previous, 0);
}

// C#: PointerInputControllerTests.MouseWheel_TiltSideways_IsIgnored
#[test]
fn mouse_wheel_tilt_sideways_is_ignored() {
    let mut f = Fixture::new();
    touchpad(&mut f, 120, true, false, 5000);
    assert!(f.viewer.is_fit());
    assert_eq!(f.commands.next + f.commands.previous, 0);
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.Touchpad_Disabled_APartialDeltaBehavesLikeBefore_ZoomMode
#[test]
fn touchpad_disabled_a_partial_delta_behaves_like_before_zoom_mode() {
    let mut f = Fixture::new();
    f.settings.touchpad_swipe_enabled = false;
    touchpad(&mut f, -10, false, false, 10_008);
    assert!(!f.viewer.is_fit());
    assert_eq!(f.commands.next, 0);
}

// C#: PointerInputControllerTests.Touchpad_Disabled_NavigateModeUsesWholeNotches
#[test]
fn touchpad_disabled_navigate_mode_uses_whole_notches() {
    let mut f = Fixture::new();
    f.settings.touchpad_swipe_enabled = false;
    f.settings.mouse_wheel_action = MouseWheelAction::Navigate;
    for i in 0..12 {
        touchpad(&mut f, -10, false, false, 10_008 + i * 8);
    }
    assert_eq!(f.commands.next, 1);
}

// C#: PointerInputControllerTests.Touchpad_TheOsHintMakesAWholeNotchASwipe
#[test]
fn touchpad_the_os_hint_makes_a_whole_notch_a_swipe() {
    let mut f = Fixture::new();
    f.run(|c, ctx| {
        c.on_wheel(
            ctx,
            WheelInput::new(-120, false, false, 5000).with_hint(WheelDeviceHint::Touchpad),
            point(400.0, 300.0),
        )
    });
    assert_eq!(f.commands.next, 1);
    assert!(f.viewer.is_fit());
}

// C#: PointerInputControllerTests.Touchpad_ImageChangesFromTheSwipe_DoNotRestartTheSwipeDistance
#[test]
fn touchpad_image_changes_from_the_swipe_do_not_restart_the_swipe_distance() {
    let mut f = Fixture::new();
    for i in 0..10 {
        touchpad(&mut f, -10, false, false, 10_008 + i * 8);
    }
    assert_eq!(f.commands.next, 1);
    f.controller.on_current_index_changed(&mut f.surface, 1);
    for i in 10..29 {
        touchpad(&mut f, -10, false, false, 10_008 + i * 8);
    }
    assert_eq!(f.commands.next, 1);
    touchpad(&mut f, -10, false, false, 10_240);
    assert_eq!(f.commands.next, 2);
}

// C#: no direct twin; W8-05g T4 short-circuit of touchpad classification.
#[test]
fn on_wheel_does_not_feed_the_touchpad_classifier_when_the_feature_is_off() {
    let mut f = Fixture::new();
    f.settings.touchpad_swipe_enabled = false;
    touchpad(&mut f, -10, false, false, 1000);
    f.settings.touchpad_swipe_enabled = true;
    touchpad(&mut f, -120, false, false, 1008);
    assert_eq!(f.commands.next, 0);
    assert!(!f.viewer.is_fit());
}

// C#: PointerInputControllerTests.ZoomInAsync_WithoutImages_DoesNothing
#[test]
fn zoom_in_without_images_does_nothing() {
    let mut f = Fixture::new();
    f.commands.has_images = false;
    f.run(|c, ctx| c.zoom_in(ctx));
    assert!(f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.ZoomOutAsync_WithoutImages_DoesNothing
#[test]
fn zoom_out_without_images_does_nothing() {
    let mut f = Fixture::new();
    f.commands.has_images = false;
    f.run(|c, ctx| c.zoom_out(ctx));
    assert!(f.viewer.is_fit());
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.ZoomInAsync_IncreasesZoom_ZoomOutAsync_DecreasesItBack
#[test]
fn zoom_in_increases_zoom_zoom_out_decreases_it_back() {
    let mut f = Fixture::new();
    f.run(|c, ctx| c.zoom_in(ctx));
    let up = f.viewer.zoom();
    f.run(|c, ctx| c.zoom_out(ctx));
    assert!(up > f.viewer.zoom());
}

fn check_keyboard_anchor(
    anchor: KeyboardZoomAnchor,
    pointer: Option<pr_app::input::primitives::PointD>,
    expected: pr_app::input::primitives::PointD,
    action: fn(
        &mut pr_app::input::pointer::PointerInputController,
        &mut pr_app::input::pointer::PointerContext<'_>,
    ) -> PointerStep,
) {
    let mut f = Fixture::new();
    f.settings.keyboard_zoom_anchor = Some(anchor);
    f.surface.pointer = pointer;
    f.run(action);
    assert_eq!(
        f.surface.to_image_element_calls.borrow().last().copied(),
        Some(expected)
    );
}

// C#: PointerInputControllerTests.ZoomInAsync_PointerOverTheViewport_ZoomsAtThePointer_NotTheCentre
#[test]
fn zoom_in_pointer_over_the_viewport_zooms_at_the_pointer_not_the_centre() {
    check_keyboard_anchor(
        KeyboardZoomAnchor::Pointer,
        Some(point(100.0, 200.0)),
        point(100.0, 200.0),
        |c, ctx| c.zoom_in(ctx),
    );
}
// C#: PointerInputControllerTests.ZoomInAsync_PointerOutsideTheViewport_FallsBackToTheCentre
#[test]
fn zoom_in_pointer_outside_the_viewport_falls_back_to_the_centre() {
    check_keyboard_anchor(KeyboardZoomAnchor::Pointer, None, point(400.0, 300.0), |c, ctx| {
        c.zoom_in(ctx)
    });
}
// C#: PointerInputControllerTests.ZoomInAsync_ViewportCentreSetting_IgnoresThePointer_EvenWhenItIsOverTheViewport
#[test]
fn zoom_in_viewport_centre_setting_ignores_the_pointer_even_when_it_is_over_the_viewport() {
    check_keyboard_anchor(
        KeyboardZoomAnchor::ViewportCentre,
        Some(point(100.0, 200.0)),
        point(400.0, 300.0),
        |c, ctx| c.zoom_in(ctx),
    );
}
// C#: PointerInputControllerTests.ZoomOutAsync_PointerOverTheViewport_ZoomsAtThePointer_NotTheCentre
#[test]
fn zoom_out_pointer_over_the_viewport_zooms_at_the_pointer_not_the_centre() {
    check_keyboard_anchor(
        KeyboardZoomAnchor::Pointer,
        Some(point(100.0, 200.0)),
        point(100.0, 200.0),
        |c, ctx| c.zoom_out(ctx),
    );
}
// C#: PointerInputControllerTests.ZoomActualSizeAsync_PointerOverTheViewport_ZoomsAtThePointer_NotTheCentre
#[test]
fn zoom_actual_size_pointer_over_the_viewport_zooms_at_the_pointer_not_the_centre() {
    check_keyboard_anchor(
        KeyboardZoomAnchor::Pointer,
        Some(point(100.0, 200.0)),
        point(100.0, 200.0),
        |c, ctx| c.zoom_actual_size(ctx),
    );
}

// C#: no direct twin; W8-05g OnCurrentIndexChanged preserves a partial wheel gesture at the same index.
#[test]
fn on_current_index_changed_same_index_keeps_the_wheel_accumulator() {
    let mut f = Fixture::new();
    f.settings.mouse_wheel_action = MouseWheelAction::Navigate;
    f.controller.on_current_index_changed(&mut f.surface, 0);
    wheel(&mut f, -60, false);
    assert_eq!(f.commands.next, 0);
    f.controller.on_current_index_changed(&mut f.surface, 0);
    wheel(&mut f, -60, false);
    assert_eq!(f.commands.next, 1);
}
