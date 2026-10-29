//! Tests of W8-14b: the pointer and wheel half of `ShellInput` (C# `PointerInputTests`, baseline `d92d6aae`): mouse regions, press / move / release, wheel, glide.
//!
//! Test kind F: the pure controllers of `pr_app` run for real, the window, the host, the frame clock and the dispatcher are fakes
//! (`common/shell_input_dispatch_fixture.rs`, the Rust twin of `InputTestSupport.cs`); the viewport is the REAL `ViewportController`.
#![cfg(windows)]

#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::Cell;
use std::rc::Rc;

use fixture::{Rig, RigOptions};
use pr_app::input::key_id::KeyId;
use pr_app::input::primitives::PointD;
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_app::input::touchpad_gestures::WheelDeviceHint;
use pr_core::model::enums::{MiddleClickAction, MouseWheelAction};
use pr_core::settings::app_settings::AppSettings;
use pr_platform_win::host::contracts::{ShellCursor, ShellWindowApi};
use pr_platform_win::shell_input::native::MK_LBUTTON;
use pr_platform_win::win32::messages::{
    WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};

const MKL: usize = MK_LBUTTON as usize;

fn actual_size(rig: &Rig) {
    rig.key_down(KeyId::D1, false, false);
    rig.drain();
}

fn assert_close(expected: f64, actual: f64, tolerance: f64, what: &str) {
    assert!(
        (expected - actual).abs() <= tolerance,
        "{what}: expected {expected}, actual {actual}"
    );
}

fn no_kinetic() -> Rig {
    Rig::configured(|s| s.kinetic_pan_enabled = false)
}

// ---- press / click-to-zoom ----

// C#: PointerInputTests.LeftPress_OutsideTheImageRectangle_IsNotHandled_AndDoesNotCapture
#[test]
fn left_press_outside_the_image_rectangle_is_not_handled_and_does_not_capture() {
    let rig = Rig::configured(|s| s.click_to_zoom_enabled = true);
    rig.fit();

    let (handled, _) = rig.mouse(WM_LBUTTONDOWN, 20, 300, 0); // left margin (the Fit image starts at x = 50)

    assert!(!handled);
    assert!(!rig.window.captured.get());
}

// C#: PointerInputTests.ClickToZoom_PressReleaseOnTheImage_ZoomsToTheClickLevel_AndAClickAtThatLevelReturnsToFit
#[test]
fn click_to_zoom_press_release_on_the_image_zooms_to_the_click_level_and_a_click_at_that_level_returns_to_fit() {
    let rig = Rig::configured(|s| {
        s.click_to_zoom_enabled = true;
        s.click_zoom_percent = 200;
    });
    rig.fit();

    assert!(rig.mouse(WM_LBUTTONDOWN, 500, 300, 0).0);
    assert!(rig.window.captured.get());
    rig.mouse(WM_LBUTTONUP, 500, 300, 0);
    assert!(!rig.window.captured.get());
    rig.drain();

    assert!(!rig.viewer.borrow().is_fit());
    assert_close(2.0, rig.viewer.borrow().zoom(), 1e-9, "zoom after the click");

    rig.message_time.set(1_000_000); // the second press is far away: not a double click
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.mouse(WM_LBUTTONUP, 500, 300, 0);
    rig.drain();
    assert!(rig.viewer.borrow().is_fit());
}

// C#: PointerInputTests.ClickToZoomDisabled_AtFit_DoesNothing
#[test]
fn click_to_zoom_disabled_at_fit_does_nothing() {
    let rig = Rig::new();
    rig.fit();

    assert!(!rig.mouse(WM_LBUTTONDOWN, 500, 300, 0).0);
    rig.mouse(WM_LBUTTONUP, 500, 300, 0);

    assert!(rig.viewer.borrow().is_fit());
}

// C#: PointerInputTests.DoubleClick_ReturnsToFit_EvenWhenZoomed_AndTheSecondReleaseIsNotAClick
#[test]
fn double_click_returns_to_fit_even_when_zoomed_and_the_second_release_is_not_a_click() {
    let rig = Rig::configured(|s| s.click_to_zoom_enabled = true);
    actual_size(&rig);
    assert!(!rig.viewer.borrow().is_fit());

    rig.message_time.set(5000);
    rig.mouse(WM_LBUTTONDOWN, 400, 300, 0);
    rig.mouse(WM_LBUTTONUP, 400, 300, 0);
    rig.message_time.set(5100);
    rig.mouse(WM_LBUTTONDOWN, 401, 300, 0);
    rig.mouse(WM_LBUTTONUP, 401, 300, 0);
    rig.drain();

    assert!(rig.viewer.borrow().is_fit());
}

// C#: PointerInputTests.TwoClicksTooFarApart_InTime_AreNotADoubleClick
#[test]
fn two_clicks_too_far_apart_in_time_are_not_a_double_click() {
    let rig = Rig::configured(|s| s.click_to_zoom_enabled = true);
    actual_size(&rig);

    rig.message_time.set(5000);
    rig.mouse(WM_LBUTTONDOWN, 400, 300, 0);
    rig.mouse(WM_LBUTTONUP, 400, 300, 0);
    rig.message_time.set(5000 + 501);
    rig.mouse(WM_LBUTTONDOWN, 400, 300, 0);
    rig.mouse(WM_LBUTTONUP, 400, 300, 0);
    rig.drain();

    // zoomed, no drag: click-to-zoom returns to Fit only at the click level; 100 % != the default click level
    assert!(!rig.viewer.borrow().is_fit());
}

// ---- drag pan / capture ----

// C#: PointerInputTests.Drag_PansTheImage_ByThePointerDelta_AndKeepsTrackingOutsideTheWindow
#[test]
fn drag_pans_the_image_by_the_pointer_delta_and_keeps_tracking_outside_the_window() {
    let rig = no_kinetic();
    actual_size(&rig);
    let (x0, y0) = rig.offsets();

    assert!(rig.mouse(WM_LBUTTONDOWN, 500, 300, 0).0);
    assert_eq!(rig.window.cursor.get(), Some(ShellCursor::SizeAll));
    rig.mouse(WM_MOUSEMOVE, 440, 270, MKL);
    rig.drain();
    assert_close(
        x0 + 60.0,
        rig.viewport.horizontal_offset(),
        1e-6,
        "x after the first move",
    );
    assert_close(
        y0 + 30.0,
        rig.viewport.vertical_offset(),
        1e-6,
        "y after the first move",
    );

    // outside the window: the signed negative coordinate is still tracked thanks to the capture
    rig.mouse(WM_MOUSEMOVE, -20, 270, MKL);
    rig.drain();
    assert_close(
        x0 + 60.0 + 460.0,
        rig.viewport.horizontal_offset(),
        1e-6,
        "x outside the window",
    );

    rig.mouse(WM_LBUTTONUP, -20, 270, 0);
    assert!(!rig.window.captured.get());
    assert_eq!(rig.window.cursor.get(), Some(ShellCursor::Arrow));
}

// C#: PointerInputTests.Drag_ContinuesOutsideTheImageRectangle_WhileTheImageHoldsTheCapture
#[test]
fn drag_continues_outside_the_image_rectangle_while_the_image_holds_the_capture() {
    let rig = no_kinetic();
    actual_size(&rig);
    rig.viewport.scroll_to(0.0, 0.0); // the image starts exactly at the left edge: x < 0 is OUTSIDE the image rectangle
    rig.drain();
    assert_eq!(0.0, rig.viewport.image_origin().x);

    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.mouse(WM_MOUSEMOVE, -20, 300, MKL); // outside the image but capturing: still pans
    rig.drain();
    assert_close(
        520.0,
        rig.viewport.horizontal_offset(),
        1e-6,
        "x after the outside move",
    );

    rig.mouse(WM_LBUTTONUP, -600, 300, 0); // offset 520: the image starts at x = -520, so -600 is outside; the release must still release the capture
    assert!(!rig.window.captured.get());

    rig.message_time.set(100_000); // not a double click of the previous press
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.window.release_pointer(); // capture lost without WM_CAPTURECHANGED: no more tracking outside the image
    rig.mouse(WM_MOUSEMOVE, -600, 300, MKL); // offset 520 -> the image starts at x = -520: -600 is outside
    rig.drain();
    assert_close(
        520.0,
        rig.viewport.horizontal_offset(),
        1e-6,
        "x after the move without capture",
    );
}

// C#: PointerInputTests.Move_WithoutCapture_OverTheMargin_IsIgnored_ButOverTheImageIsTracked
#[test]
fn move_without_capture_over_the_margin_is_ignored_but_over_the_image_is_tracked() {
    let rig = no_kinetic();
    actual_size(&rig);
    let x0 = rig.viewport.horizontal_offset();
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.window.release_pointer(); // e.g. the system took the capture away
    rig.send(WM_CAPTURECHANGED, 0, 0);

    rig.mouse(WM_MOUSEMOVE, 400, 300, MKL);
    rig.drain();

    assert_eq!(x0, rig.viewport.horizontal_offset()); // the pan was cancelled by the capture loss
}

// C#: PointerInputTests.CaptureChanged_CancelsThePan
#[test]
fn capture_changed_cancels_the_pan() {
    let rig = no_kinetic();
    actual_size(&rig);
    let x0 = rig.viewport.horizontal_offset();
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.mouse(WM_MOUSEMOVE, 450, 300, MKL);
    rig.drain();
    let panned = rig.viewport.horizontal_offset();
    assert!(panned > x0);

    rig.window.release_pointer();
    rig.send(WM_CAPTURECHANGED, 0, 0);
    rig.mouse(WM_MOUSEMOVE, 300, 300, MKL);
    rig.drain();

    assert_eq!(panned, rig.viewport.horizontal_offset());
}

// C#: PointerInputTests.Move_WithoutTheLeftButton_DoesNotPan
#[test]
fn move_without_the_left_button_does_not_pan() {
    let rig = no_kinetic();
    actual_size(&rig);
    let x0 = rig.viewport.horizontal_offset();
    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0);
    rig.mouse(WM_MOUSEMOVE, 400, 300, 0);
    rig.drain();
    assert_eq!(x0, rig.viewport.horizontal_offset());
}

// C#: PointerInputTests.PixelCoordinates_AreConvertedToDipsByTheWindowDpi
#[test]
fn pixel_coordinates_are_converted_to_dips_by_the_window_dpi() {
    for (dpi, pixel_x, pixel_y, dip_x, dip_y) in [
        (1.0, 100, 60, 100.0, 60.0),
        (1.5, 150, 90, 100.0, 60.0),
        (2.0, 200, 120, 100.0, 60.0),
    ] {
        let rig = Rig::with(RigOptions {
            dpi_scale: dpi,
            ..RigOptions::default()
        });
        rig.mouse(WM_MOUSEMOVE, pixel_x, pixel_y, 0);
        assert_eq!(
            rig.input.last_pointer_position(),
            Some(PointD { x: dip_x, y: dip_y }),
            "dpi {dpi}"
        );
    }
}

// C#: PointerInputTests.ThePointerActivityEvent_FiresOnMove
#[test]
fn the_pointer_activity_event_fires_on_move() {
    let rig = Rig::new();
    let count = Rc::new(Cell::new(0));
    let counter = Rc::clone(&count);
    rig.input
        .subscribe_pointer_activity(Rc::new(move || counter.set(counter.get() + 1)));
    rig.mouse(WM_MOUSEMOVE, 10, 10, 0);
    rig.mouse(WM_MOUSEMOVE, 11, 10, 0);
    assert_eq!(2, count.get());
}

// ---- overlay / compare ----

// C#: PointerInputTests.Overlay_SwallowsPressAndWheel_ButStillStopsAGlide
#[test]
fn overlay_swallows_press_and_wheel_but_still_stops_a_glide() {
    let rig = Rig::configured(|s| {
        s.click_to_zoom_enabled = true;
        s.kinetic_pan_enabled = true;
    });
    actual_size(&rig);
    rig.input
        .set_overlay_hit_test(Some(Rc::new(|p: PointD| p.x < 100.0 && p.y < 60.0)));
    rig.key_down(KeyId::RIGHT, false, false); // starts a glide
    assert_eq!(1, rig.clock.subscriber_count());

    assert!(!rig.mouse(WM_LBUTTONDOWN, 50, 30, 0).0);
    assert_eq!(0, rig.clock.subscriber_count());
    assert!(!rig.window.captured.get());
    assert!(!rig.wheel(WM_MOUSEWHEEL, 120, 50, 30, false).0);
    assert!(rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, false).0);
}

// C#: PointerInputTests.CompareVisible_TakesPressAndWheelAway_FromTheImage
#[test]
fn compare_visible_takes_press_and_wheel_away_from_the_image() {
    let rig = Rig::configured(|s| s.click_to_zoom_enabled = true);
    rig.fit();
    rig.host.is_compare_visible.set(true);

    assert!(!rig.mouse(WM_LBUTTONDOWN, 500, 300, 0).0);
    assert!(!rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, false).0);
    assert!(!rig.wheel(WM_MOUSEHWHEEL, 120, 500, 300, false).0);
    assert!(!rig.mouse(WM_MBUTTONDOWN, 500, 300, 0).0);
    assert!(rig.viewer.borrow().is_fit());
}

// C#: ShellViewingTests.Compare_TogglesShowsBothImages_AndClicksSelectTheCellUnderThePoint (the click half) / WPF MainWindow.CompareLeft_Click + CompareRight_Click.
// The Win32 baseline never wired the press to TrySelectCompareAt, so "click to select" did nothing in the exe (walkthrough 3).
#[test]
fn compare_visible_left_press_on_a_cell_selects_that_cell() {
    let rig = Rig::configured(|s| s.click_to_zoom_enabled = true);
    rig.fit();
    rig.host.is_compare_visible.set(true);

    rig.mouse(WM_LBUTTONDOWN, 250, 300, 0);
    rig.mouse(WM_LBUTTONDOWN, 750, 300, 0);

    assert_eq!(vec![true, false], *rig.host.compare_selections.borrow());
    assert!(rig.viewer.borrow().is_fit(), "the press never reaches the image");
}

// C#: same test, the miss half: the margin outside the panel selects nothing; so do the other buttons and a press with the compare view hidden.
#[test]
fn compare_visible_press_outside_a_cell_or_with_another_button_selects_nothing() {
    let rig = Rig::configured(|s| s.click_to_zoom_enabled = true);
    rig.fit();
    rig.host.is_compare_visible.set(true);

    rig.mouse(WM_LBUTTONDOWN, 2, 2, 0);
    rig.mouse(WM_MBUTTONDOWN, 250, 300, 0);
    assert!(rig.host.compare_selections.borrow().is_empty());

    rig.host.is_compare_visible.set(false);
    rig.mouse(WM_LBUTTONDOWN, 250, 300, 0);
    assert!(rig.host.compare_selections.borrow().is_empty());
}

// ---- middle button ----

// C#: PointerInputTests.MiddleClick_RunsTheConfiguredAction
#[test]
fn middle_click_runs_the_configured_action() {
    for (action, expected) in [
        (MiddleClickAction::NextImage, ReviewCommandType::Next),
        (MiddleClickAction::PreviousImage, ReviewCommandType::Previous),
        (MiddleClickAction::NextFolder, ReviewCommandType::NextFolder),
        (MiddleClickAction::PreviousFolder, ReviewCommandType::PreviousFolder),
        (MiddleClickAction::OpenFolder, ReviewCommandType::OpenFolder),
    ] {
        let rig = Rig::configured(move |s| s.middle_click_action = Some(action));
        rig.fit();

        assert!(rig.mouse(WM_MBUTTONDOWN, 500, 300, 0).0, "{action:?}");

        assert_eq!(
            vec![ReviewCommand::new(expected)],
            rig.host.commands_snapshot(),
            "{action:?}"
        );
    }
}

// C#: PointerInputTests.MiddleClick_ViewportActions_RunOnTheController
#[test]
fn middle_click_viewport_actions_run_on_the_controller() {
    let rig = Rig::configured(|s| s.middle_click_action = Some(MiddleClickAction::ActualSize));
    rig.fit();
    rig.mouse(WM_MBUTTONDOWN, 500, 300, 0);
    rig.drain();
    assert_close(1.0, rig.viewer.borrow().zoom(), 1e-9, "zoom after the middle click");
    assert!(rig.host.commands_snapshot().is_empty());
}

// C#: PointerInputTests.MiddleClick_WorksOverTheMargin_NotJustTheImage
#[test]
fn middle_click_works_over_the_margin_not_just_the_image() {
    let rig = Rig::configured(|s| s.middle_click_action = Some(MiddleClickAction::NextImage));
    rig.fit();
    assert!(rig.mouse(WM_MBUTTONDOWN, 10, 300, 0).0);
    assert_eq!(vec![ReviewCommand::NEXT], rig.host.commands_snapshot());
}

// C#: PointerInputTests.MiddleClick_OnAScrollBar_Overlay_NoImages_OrNone_DoesNotRunAnything
#[test]
fn middle_click_on_a_scroll_bar_overlay_no_images_or_none_does_not_run_anything() {
    let rig = Rig::configured(|s| s.middle_click_action = Some(MiddleClickAction::NextImage));
    actual_size(&rig); // two scroll bars: viewport 990x590
    assert!(!rig.mouse(WM_MBUTTONDOWN, 995, 300, 0).0); // vertical bar
    assert!(!rig.mouse(WM_MBUTTONDOWN, 300, 595, 0).0); // horizontal bar
    rig.input
        .set_overlay_hit_test(Some(Rc::new(|p: PointD| p.x < 100.0 && p.y < 60.0)));
    assert!(!rig.mouse(WM_MBUTTONDOWN, 50, 30, 0).0);
    assert!(rig.host.commands_snapshot().is_empty());

    rig.host.has_images.set(false);
    assert!(rig.mouse(WM_MBUTTONDOWN, 500, 300, 0).0); // swallowed, but no images: no command
    assert!(rig.host.commands_snapshot().is_empty());

    rig.host.has_images.set(true);
    rig.update_settings(|s: &mut AppSettings| s.middle_click_action = Some(MiddleClickAction::None));
    assert!(rig.mouse(WM_MBUTTONDOWN, 500, 300, 0).0);
    assert!(rig.host.commands_snapshot().is_empty());
}

// C#: PointerInputTests.MiddleClick_RepeatedWithinDoubleClickTime_RunsOnce
#[test]
fn middle_click_repeated_within_double_click_time_runs_once() {
    let rig = Rig::configured(|s| s.middle_click_action = Some(MiddleClickAction::NextImage));
    rig.message_time.set(1000);
    rig.mouse(WM_MBUTTONDOWN, 500, 300, 0);
    rig.message_time.set(1100);
    assert!(rig.mouse(WM_MBUTTONDOWN, 500, 300, 0).0);
    assert_eq!(vec![ReviewCommand::NEXT], rig.host.commands_snapshot());
    rig.message_time.set(5000);
    rig.mouse(WM_MBUTTONDOWN, 500, 300, 0);
    assert_eq!(2, rig.host.commands_snapshot().len());
}

// ---- wheel ----

// C#: PointerInputTests.Wheel_ZoomsAtThePointer_KeepingThePointUnderTheCursor
#[test]
fn wheel_zooms_at_the_pointer_keeping_the_point_under_the_cursor() {
    let rig = Rig::new();
    rig.fit();
    let before = rig.viewport.to_image_element(PointD { x: 300.0, y: 200.0 });
    let fraction = (
        before.x / rig.viewport.image_actual_width(),
        before.y / rig.viewport.image_actual_height(),
    );

    assert!(rig.wheel(WM_MOUSEWHEEL, 120, 300, 200, false).0);
    rig.drain();

    assert!(!rig.viewer.borrow().is_fit());
    assert_close(0.4, rig.viewer.borrow().zoom(), 1e-9, "zoom"); // Fit 0.3 + a 10 % step
    let after = rig.viewport.to_image_element(PointD { x: 300.0, y: 200.0 });
    assert_close(
        fraction.0,
        after.x / rig.viewport.image_actual_width(),
        1e-3,
        "x fraction",
    );
    assert_close(
        fraction.1,
        after.y / rig.viewport.image_actual_height(),
        1e-3,
        "y fraction",
    );
}

// C#: PointerInputTests.Wheel_Navigate_UsesTheMessageTimeForTheGestureGrouping
#[test]
fn wheel_navigate_uses_the_message_time_for_the_gesture_grouping() {
    let rig = Rig::configured(|s| s.mouse_wheel_action = MouseWheelAction::Navigate);
    rig.fit();
    rig.message_time.set(100);
    rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, false);
    rig.message_time.set(100 + 5000);
    rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, false);
    assert_eq!(2, rig.host.previouses.get());
}

// ---- touchpad ----

fn zoomed_rig(configure: impl Fn(&mut AppSettings) + 'static) -> Rig {
    let rig = Rig::configured(move |s| {
        s.kinetic_pan_enabled = false;
        configure(s);
    });
    actual_size(&rig);
    rig.viewport.scroll_to(500.0, 500.0);
    rig.drain();
    rig
}

// C#: PointerInputTests.TouchpadVerticalSwipe_PansAZoomedImage_OneDipPerDeltaUnit
#[test]
fn touchpad_vertical_swipe_pans_a_zoomed_image_one_dip_per_delta_unit() {
    for (delta, x, y) in [(30, 500.0, 470.0), (-45, 500.0, 545.0)] {
        let rig = zoomed_rig(|_| {});
        rig.message_time.set(100);

        rig.wheel(WM_MOUSEWHEEL, delta, 500, 300, false);
        rig.drain();

        assert_close(x, rig.viewport.horizontal_offset(), 1e-6, &format!("x, delta {delta}"));
        assert_close(y, rig.viewport.vertical_offset(), 1e-6, &format!("y, delta {delta}"));
    }
}

// C#: PointerInputTests.TouchpadHorizontalSwipe_PansRight_WithPositiveDelta
#[test]
fn touchpad_horizontal_swipe_pans_right_with_positive_delta() {
    let rig = zoomed_rig(|_| {});
    rig.message_time.set(100);
    rig.tick_count.set(100);

    assert!(rig.wheel(WM_MOUSEHWHEEL, 40, 500, 300, false).0);
    rig.drain();

    assert_close(540.0, rig.viewport.horizontal_offset(), 1e-6, "x");
    assert_close(500.0, rig.viewport.vertical_offset(), 1e-6, "y");
}

// C#: PointerInputTests.DeviceHint_MakesAnExactNotchATouchpadMessage
#[test]
fn device_hint_makes_an_exact_notch_a_touchpad_message() {
    let rig = zoomed_rig(|_| {});
    rig.message_time.set(100);

    rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, false); // device Unknown + exactly one notch: a mouse wheel -> zoom
    rig.drain();
    assert!(rig.viewer.borrow().zoom() > 1.0);

    let rig2 = zoomed_rig(|_| {});
    rig2.message_time.set(100);
    rig2.wheel_device.set(WheelDeviceHint::Touchpad);
    rig2.wheel(WM_MOUSEWHEEL, 120, 500, 300, false);
    rig2.drain();
    assert_close(1.0, rig2.viewer.borrow().zoom(), 1e-9, "zoom with the touchpad hint");
    assert_close(380.0, rig2.viewport.vertical_offset(), 1e-6, "y with the touchpad hint");
}

// C#: PointerInputTests.TouchpadGesture_GapBoundary_DecidesWhetherANotchIsStillTheSwipe
#[test]
fn touchpad_gesture_gap_boundary_decides_whether_a_notch_is_still_the_swipe() {
    for (gap, still_touchpad) in [(250, true), (251, false)] {
        let rig = zoomed_rig(|_| {});
        rig.message_time.set(1000);
        rig.wheel(WM_MOUSEWHEEL, 30, 500, 300, false); // certainly a touchpad
        rig.drain();
        let offset_after_first = rig.viewport.vertical_offset();

        rig.message_time.set(1000 + gap);
        rig.wheel(WM_MOUSEWHEEL, 120, 500, 300, false);
        rig.drain();

        if still_touchpad {
            assert_close(
                offset_after_first - 120.0,
                rig.viewport.vertical_offset(),
                1e-6,
                &format!("y, gap {gap}"),
            );
            assert_close(1.0, rig.viewer.borrow().zoom(), 1e-9, &format!("zoom, gap {gap}"));
        } else {
            assert!(rig.viewer.borrow().zoom() > 1.0, "gap {gap}");
        }
    }
}

// C#: PointerInputTests.HorizontalWheel_UsesTheTickCount_NotTheMessageTime
#[test]
fn horizontal_wheel_uses_the_tick_count_not_the_message_time() {
    let rig = zoomed_rig(|_| {});
    rig.message_time.set(5);
    rig.tick_count.set(1000);
    rig.wheel(WM_MOUSEHWHEEL, 30, 500, 300, false); // touchpad (30 is not a notch)
    rig.drain();
    let x = rig.viewport.horizontal_offset();
    assert_close(530.0, x, 1e-6, "x after the swipe");

    // 5000 ms apart by the tick count but the same message time: the right source -> outside the gesture -> the horizontal notch is ignored.
    rig.tick_count.set(6000);
    rig.wheel(WM_MOUSEHWHEEL, 120, 500, 300, false);
    rig.drain();
    assert_close(x, rig.viewport.horizontal_offset(), 1e-6, "x after the notch");
}

// C#: PointerInputTests.TouchpadSwipeAtFit_ChangesImage_AfterHalfTheConfiguredDistance
#[test]
fn touchpad_swipe_at_fit_changes_image_after_half_the_configured_distance() {
    let rig = Rig::configured(|s| {
        s.touchpad_swipe_enabled = true;
        s.touchpad_swipe_distance_per_image = 600;
    });
    rig.fit();
    rig.message_time.set(100);
    rig.wheel_device.set(WheelDeviceHint::Touchpad);

    rig.wheel(WM_MOUSEWHEEL, -290, 500, 300, false);
    assert_eq!(0, rig.host.nexts.get()); // 290 < 300
    rig.message_time.set(116);
    rig.wheel(WM_MOUSEWHEEL, -20, 500, 300, false); // total 310 >= 300
    assert_eq!(1, rig.host.nexts.get());
}

// C#: PointerInputTests.TouchpadSwipe_Disabled_FallsBackToTheMouseWheelBehaviour
#[test]
fn touchpad_swipe_disabled_falls_back_to_the_mouse_wheel_behaviour() {
    let rig = Rig::configured(|s| s.touchpad_swipe_enabled = false);
    rig.fit();
    rig.message_time.set(100);
    rig.wheel_device.set(WheelDeviceHint::Touchpad);
    rig.wheel(WM_MOUSEWHEEL, 30, 500, 300, false);
    rig.drain();
    assert!(!rig.viewer.borrow().is_fit()); // zoom (the default MouseWheelAction), not navigation
}

// C#: PointerInputTests.HorizontalWheel_OutsideTheViewport_IsNotHandled_InsideIsHandled
#[test]
fn horizontal_wheel_outside_the_viewport_is_not_handled_inside_is_handled() {
    let rig = zoomed_rig(|_| {});
    assert!(!rig.wheel(WM_MOUSEHWHEEL, 40, 995, 300, false).0); // over the vertical scroll bar
    assert!(!rig.wheel(WM_MOUSEHWHEEL, 40, 300, 595, false).0);
    assert!(!rig.wheel(WM_MOUSEHWHEEL, 40, -1, 300, false).0);
    assert!(rig.wheel(WM_MOUSEHWHEEL, 40, 989, 589, false).0);
    rig.window.loaded.set(false);
    assert!(!rig.wheel(WM_MOUSEHWHEEL, 40, 300, 300, false).0);
}

// ---- kinetic glide from the mouse ----

// C#: PointerInputTests.Flick_StartsAGlide_ThatMovesTheImageEachFrame_AndAPressStopsIt
#[test]
fn flick_starts_a_glide_that_moves_the_image_each_frame_and_a_press_stops_it() {
    let rig = Rig::configured(|s| s.kinetic_pan_enabled = true);
    actual_size(&rig);
    rig.message_time.set(1000);
    rig.mouse(WM_LBUTTONDOWN, 800, 300, 0);
    for i in 1..=6 {
        rig.message_time.set(1000 + i * 8);
        rig.mouse(WM_MOUSEMOVE, 800 - i * 40, 300, MKL);
    }

    rig.message_time.set(1000 + 56);
    rig.mouse(WM_LBUTTONUP, 800 - 240, 300, 0);
    assert_eq!(1, rig.clock.subscriber_count());

    let mut offsets = Vec::new();
    rig.timestamp.set(0);
    for i in 0..8 {
        rig.clock.tick(f64::from(i) * 16.0);
        rig.timestamp.set(rig.timestamp.get() + 160_000);
        rig.drain();
        offsets.push(rig.viewport.horizontal_offset());
    }

    assert!(offsets[offsets.len() - 1] > offsets[0], "{offsets:?}");
    assert!(offsets.windows(2).all(|w| w[1] >= w[0]), "{offsets:?}");

    rig.mouse(WM_LBUTTONDOWN, 500, 300, 0); // a press stops the glide (and that press is not a click)
    assert_eq!(0, rig.clock.subscriber_count());
}
