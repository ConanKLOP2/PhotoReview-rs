//! W8-05h, part 2: press / move / release, the kinetic glide and arrow-key panning. Twins of the `PointerInputControllerTests`
//! methods named in `contracts/W8-05h.md`. The glide frames of the C# tests (`handler(null, new FrameArgs(t))`) are
//! `on_kinetic_frame(ctx, Some(t_ms * 10_000))` while `is_kinetic_hooked()`; no test waits on time.

// Each integration binary uses a different subset of the shared fixture.
#[allow(dead_code)]
#[path = "common/pointer_fixture.rs"]
mod pointer_fixture;

use pointer_fixture::{FakeCommands, FakeSurface, Fixture, point};
use pr_app::input::key_id::KeyId;
use pr_app::input::operation_version::ViewportOperationVersion;
use pr_app::input::pointer::{PointerContext, PointerInputController, PointerRelease, PointerStep};
use pr_app::input::primitives::{PointD, PointerButton, SizeD};
use pr_app::input::surface::{ImageSurface, RenderFrameHandler, RenderHookId};
use pr_app::viewmodels::viewer_state::ViewerState;
use pr_core::abstractions::{DisplayTiming, NullLog};
use pr_core::diagnostics::perf::STOPWATCH_FREQUENCY;
use pr_core::model::enums::{KeyboardZoomAnchor, KineticGlideSmoothing, MouseWheelAction};
use pr_core::settings::app_settings::AppSettings;

/// The C# test class constructor: the settings every test starts from.
fn fixture() -> Fixture {
    let mut f = Fixture::new();
    f.settings.click_to_zoom_enabled = true;
    f.settings.click_zoom_percent = 200;
    f.settings.kinetic_pan_enabled = true;
    f
}

/// C# `ZoomInSoTheImageCanPan()`.
fn zoom_in_so_the_image_can_pan(f: &mut Fixture) {
    f.viewer.set_zoom(2.0);
    f.surface.extent_width = 4000.0;
    f.surface.extent_height = 3000.0;
    f.surface.horizontal = 500.0;
    f.surface.vertical = 400.0;
}

fn press(f: &mut Fixture, p: PointD, timestamp: i32) -> bool {
    let mut taken = false;
    f.run(|c, ctx| {
        taken = c.on_image_press(ctx, PointerButton::Left, 1, p, timestamp);
        PointerStep::Done
    });
    taken
}

fn move_to(f: &mut Fixture, p: PointD, timestamp: i32, left_down: bool) -> bool {
    let mut handled = false;
    f.run(|c, ctx| {
        handled = c.on_image_move(ctx, left_down, p, timestamp);
        PointerStep::Done
    });
    handled
}

fn release(f: &mut Fixture, p: PointD, timestamp: i32) -> PointerRelease {
    let mut result = None;
    f.run(|c, ctx| {
        result = Some(c.on_image_release(ctx, p, timestamp));
        PointerStep::Done
    });
    result.expect("the closure ran")
}

fn arrow(f: &mut Fixture, key: KeyId, is_repeat: bool) -> bool {
    let mut used = false;
    f.run(|c, ctx| {
        used = c.try_pan_by_arrow(ctx, key, is_repeat);
        PointerStep::Done
    });
    used
}

fn frame(f: &mut Fixture, ms: i64) {
    f.run(|c, ctx| {
        c.on_kinetic_frame(ctx, Some(ms * 10_000));
        PointerStep::Done
    });
}

/// C# `Drag()` with a chosen first timestamp: a fast horizontal drag (2 DIP/ms) released while still moving.
fn drag_from(f: &mut Fixture, base: i32) {
    assert!(press(f, point(400.0, 300.0), base));
    for t in (10..=60).step_by(10) {
        move_to(f, point(400.0 - 2.0 * f64::from(t), 300.0), base.wrapping_add(t), true);
    }
    assert!(release(f, point(270.0, 300.0), base.wrapping_add(65)).handled);
}

fn drag(f: &mut Fixture) {
    drag_from(f, 1000);
}

/// C# `StartGlide()`.
fn start_glide(f: &mut Fixture) {
    zoom_in_so_the_image_can_pan(f);
    drag(f);
    assert!(f.controller.is_kinetic_hooked());
}

/// C# `DriveGlideToRest()`: frames every 16 ms until the glide unhooks itself.
fn drive_glide_to_rest(f: &mut Fixture) {
    let mut t = 0;
    while t < 20_000 && f.controller.is_kinetic_hooked() {
        frame(f, t);
        t += 16;
    }
    assert!(!f.controller.is_kinetic_hooked());
}

// ---- pan ----

// C#: PointerInputControllerTests.PressMoveRelease_Pans_CapturesAndReleases_AndTheReleaseIsNotAClick
#[test]
fn press_move_release_pans_captures_and_releases_and_the_release_is_not_a_click() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);

    assert!(press(&mut f, point(100.0, 100.0), 1000));
    assert_eq!(f.surface.captures, 1);
    assert!(f.surface.pan_cursor);

    assert!(move_to(&mut f, point(120.0, 110.0), 1010, true));
    assert_eq!(f.surface.scrolls.last(), Some(&(480.0, 390.0))); // offsets 500/400 minus the pointer delta

    let zoom = f.viewer.zoom();
    let result = release(&mut f, point(120.0, 110.0), 1500); // rested: no glide
    assert!(result.handled);
    assert!(matches!(result.step, PointerStep::Done));
    assert_eq!(f.viewer.zoom(), zoom);
    assert_eq!(f.commands.fits, 0);
    assert_eq!(f.surface.releases, 1);
    assert!(!f.surface.pan_cursor);
    assert_eq!(f.surface.hooks, 0);
}

// C#: PointerInputControllerTests.Move_BelowTheDragThreshold_IsNotHandled_AndTheReleaseIsAClick
#[test]
fn move_below_the_drag_threshold_is_not_handled_and_the_release_is_a_click() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    assert!(press(&mut f, point(100.0, 100.0), 0));

    assert!(!move_to(&mut f, point(101.0, 101.0), 5, true));
    let result = release(&mut f, point(101.0, 101.0), 10);
    assert!(result.handled);
    assert!(matches!(result.step, PointerStep::Done)); // a click at the click zoom (200 %) goes back to Fit, no render yield
    assert_eq!(f.commands.fits, 1);
}

// C#: PointerInputControllerTests.Move_WithoutATrackedPress_OrWithTheButtonUp_DoesNothing
#[test]
fn move_without_a_tracked_press_or_with_the_button_up_does_nothing() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    assert!(!move_to(&mut f, point(200.0, 200.0), 0, true));

    assert!(press(&mut f, point(100.0, 100.0), 0));
    assert!(!move_to(&mut f, point(200.0, 200.0), 5, false));
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.LostCapture_EndsThePan
#[test]
fn lost_capture_ends_the_pan() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    assert!(press(&mut f, point(100.0, 100.0), 0));

    f.controller.on_lost_capture(&mut f.surface);

    assert!(!move_to(&mut f, point(200.0, 200.0), 5, true));
    assert!(!f.surface.pan_cursor);
}

// C#: PointerInputControllerTests.MutationGaps.Press_ZoomedImageWithClickToZoomOff_IsTakenOnlyWhenTheExtentExceedsTheViewportByMoreThanHalfADip
#[test]
fn press_zoomed_image_with_click_to_zoom_off_is_taken_only_when_the_extent_exceeds_the_viewport_by_more_than_half_a_dip()
 {
    for (extent_width, extent_height, expected_taken) in [
        (800.0, 600.0, false),
        (800.5, 600.0, false),
        (800.0, 600.5, false),
        (800.6, 600.0, true),
        (800.0, 600.6, true),
        (900.0, 600.0, true),
        (800.0, 700.0, true),
    ] {
        let mut f = fixture();
        f.settings.click_to_zoom_enabled = false;
        f.viewer.set_zoom(2.0);
        f.surface.extent_width = extent_width;
        f.surface.extent_height = extent_height;

        let taken = press(&mut f, point(400.0, 300.0), 1000);

        assert_eq!(taken, expected_taken, "extent {extent_width} x {extent_height}");
        assert_eq!(
            f.surface.pan_cursor, expected_taken,
            "extent {extent_width} x {extent_height}"
        );
    }
}

// C#: PointerInputControllerTests.MutationGaps.Press_FitImageWithAnOversizedExtentAndClickToZoomOff_IsNotTaken
#[test]
fn press_fit_image_with_an_oversized_extent_and_click_to_zoom_off_is_not_taken() {
    let mut f = fixture();
    f.settings.click_to_zoom_enabled = false;
    assert!(f.viewer.is_fit());
    f.surface.extent_width = 4000.0;
    f.surface.extent_height = 3000.0;

    assert!(!press(&mut f, point(400.0, 300.0), 1000));
    assert!(!f.surface.pan_cursor);
    assert_eq!(f.surface.captures, 0);
}

// New test: the drag threshold is per axis and inclusive (4 DIP in the fixture surface), measured from the START point.
#[test]
fn drag_threshold_is_inclusive_per_axis_and_measured_from_the_start_point() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    assert!(press(&mut f, point(100.0, 100.0), 0));
    assert!(!move_to(&mut f, point(103.0, 100.0), 1, true)); // 3 < 4
    assert!(move_to(&mut f, point(104.0, 100.0), 2, true)); // 4 >= 4, total from the start
    assert!(move_to(&mut f, point(100.0, 100.0), 3, true)); // sticky once crossed
}

// New test: a non-left button that passes the other checks is tracked like a left press (C# only compares it for the double click).
#[test]
fn a_non_left_press_is_tracked_and_only_the_left_double_click_fits() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    let mut taken = false;
    f.run(|c, ctx| {
        taken = c.on_image_press(ctx, PointerButton::Right, 2, point(10.0, 10.0), 0);
        PointerStep::Done
    });
    assert!(taken);
    assert_eq!(f.commands.fits, 0);
    assert_eq!(f.surface.captures, 1);
}

// ---- kinetic glide ----

// C#: PointerInputControllerTests.FastRelease_StartsAGlide_ThatStepsOnNewFramesAndUnhooksWhenItStops
#[test]
fn fast_release_starts_a_glide_that_steps_on_new_frames_and_unhooks_when_it_stops() {
    let mut f = fixture();
    start_glide(&mut f);
    assert_eq!(f.surface.hooks, 1);

    let scrolls = f.surface.scrolls.len();
    frame(&mut f, 0); // first frame only records the time
    frame(&mut f, 0); // same frame again: no step
    assert_eq!(f.surface.scrolls.len(), scrolls);
    frame(&mut f, 16);
    assert_eq!(f.surface.scrolls.len(), scrolls + 1);

    let mut t = 32;
    while t < 20_000 && f.controller.is_kinetic_hooked() {
        let before = f.surface.scrolls.len();
        frame(&mut f, t);
        if !f.controller.is_kinetic_hooked() {
            // The glide unhooks in the very frame whose step ended it, not on a later frame.
            assert_eq!(f.surface.scrolls.len(), before + 1);
        }
        t += 16;
    }

    assert!(!f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 1);
    assert_eq!(f.surface.hooks, 1);
}

// C#: PointerInputControllerTests.PressDuringAGlide_StopsIt_AndThatPressIsNeverAClick
#[test]
fn press_during_a_glide_stops_it_and_that_press_is_never_a_click() {
    let mut f = fixture();
    start_glide(&mut f);
    f.settings.click_to_zoom_enabled = true;

    f.controller.on_window_preview_mouse_down(&mut f.surface);
    assert!(!f.controller.is_kinetic_hooked());
    assert!(press(&mut f, point(300.0, 300.0), 5000));
    assert!(!release(&mut f, point(300.0, 300.0), 5010).handled);

    assert_eq!(f.commands.fits, 0);
    assert!((f.viewer.zoom() - 2.0).abs() < 1e-6);
}

// C#: PointerInputControllerTests.GlideStoppedByAnUnrelatedPress_ThenASeparateImageClick_StillTriggersClickToZoom
#[test]
fn glide_stopped_by_an_unrelated_press_then_a_separate_image_click_still_triggers_click_to_zoom() {
    let mut f = fixture();
    start_glide(&mut f);

    // Press A: e.g. a toolbar button. Only the window-level tunnel fires.
    f.controller.on_window_preview_mouse_down(&mut f.surface);
    assert!(!f.controller.is_kinetic_hooked());

    // Press B: window tunnel first (overwrites the stale flag), then the image press, then the click.
    f.controller.on_window_preview_mouse_down(&mut f.surface);
    assert!(press(&mut f, point(300.0, 300.0), 6000));
    assert!(release(&mut f, point(300.0, 300.0), 6010).handled);

    assert_eq!(f.commands.fits, 1); // already at the click zoom -> back to Fit, not swallowed
}

// C#: PointerInputControllerTests.Navigation_StopsAGlide_ButTheSameIndexDoesNot
#[test]
fn navigation_stops_a_glide_but_the_same_index_does_not() {
    let mut f = fixture();
    f.controller.on_current_index_changed(&mut f.surface, 3);
    start_glide(&mut f);

    f.controller.on_current_index_changed(&mut f.surface, 3);
    assert!(f.controller.is_kinetic_hooked());

    f.controller.on_current_index_changed(&mut f.surface, 4);
    assert!(!f.controller.is_kinetic_hooked());
}

// C#: PointerInputControllerTests.WindowClosed_UnhooksARunningGlide
#[test]
fn window_closed_unhooks_a_running_glide() {
    let mut f = fixture();
    start_glide(&mut f);
    f.controller.on_window_closed(&mut f.surface);
    assert!(!f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 1);
}

// C#: PointerInputControllerTests.WindowClosed_UnhooksARunningGlide (the second half of OnWindowClosed: CancelPan)
#[test]
fn window_closed_also_ends_a_tracked_press() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    assert!(press(&mut f, point(100.0, 100.0), 0));
    f.controller.on_window_closed(&mut f.surface);
    assert_eq!(f.surface.releases, 1);
    assert!(!f.surface.pan_cursor);
    assert!(!move_to(&mut f, point(200.0, 200.0), 5, true));
}

// C#: PointerInputControllerTests.Frame_AfterTheWindowUnloaded_StopsTheGlide
#[test]
fn frame_after_the_window_unloaded_stops_the_glide() {
    let mut f = fixture();
    start_glide(&mut f);
    f.surface.loaded = false;
    let scrolls = f.surface.scrolls.len();
    frame(&mut f, 0);
    assert!(!f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 1);
    assert_eq!(f.surface.scrolls.len(), scrolls);
}

// C#: PointerInputControllerTests.Release_AfterADrag_WithKineticPanDisabled_DoesNotGlide
#[test]
fn release_after_a_drag_with_kinetic_pan_disabled_does_not_glide() {
    let mut f = fixture();
    f.settings.kinetic_pan_enabled = false;
    zoom_in_so_the_image_can_pan(&mut f);
    drag(&mut f);
    assert_eq!(f.surface.hooks, 0);
    assert!(!f.controller.is_kinetic_hooked());
}

// C#: PointerInputControllerTests.Wheel_StopsARunningGlide
#[test]
fn wheel_stops_a_running_glide() {
    let mut f = fixture();
    start_glide(&mut f);
    f.settings.mouse_wheel_action = MouseWheelAction::Navigate;
    f.run(|c, ctx| c.on_wheel_vertical(ctx, 30, false, point(10.0, 10.0))); // a partial notch: no outcome at all
    assert_eq!(f.surface.unhooks, 1);
    assert!(!f.controller.is_kinetic_hooked());
}

// C#: PointerInputControllerTests.ZoomInAsync_StopsARunningGlide
#[test]
fn zoom_in_stops_a_running_glide() {
    let mut f = fixture();
    start_glide(&mut f);
    f.run(|c, ctx| c.zoom_in(ctx));
    assert!(!f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 1);
}

// New test (T1): the press stops the glide even when the window-level flag is already set (bit-or, not `||`).
#[test]
fn press_stops_the_glide_even_when_the_flag_is_already_set() {
    let mut f = fixture();
    start_glide(&mut f);
    f.controller.on_window_preview_mouse_down(&mut f.surface); // glide stopped, flag set
    assert!(!f.controller.is_kinetic_hooked());
    assert!(arrow(&mut f, KeyId::RIGHT, false)); // a new glide (arrow impulse) is running again
    assert!(f.controller.is_kinetic_hooked());

    assert!(press(&mut f, point(400.0, 300.0), 1200));

    assert!(!f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.hooks, f.surface.unhooks);
    assert!(!release(&mut f, point(400.0, 300.0), 1210).handled); // and that press is not a click
}

// New test (T2): the stale window-level flag is cleared by the image press, so the next press is a click again.
#[test]
fn the_stopped_glide_flag_is_cleared_by_the_image_press() {
    let mut f = fixture();
    start_glide(&mut f);
    f.controller.on_window_preview_mouse_down(&mut f.surface);
    assert!(press(&mut f, point(300.0, 300.0), 5000));
    assert!(!release(&mut f, point(300.0, 300.0), 5010).handled);
    // A later press without a window tunnel: the flag was cleared, so it is a click (back to Fit).
    assert!(press(&mut f, point(300.0, 300.0), 6000));
    assert!(release(&mut f, point(300.0, 300.0), 6010).handled);
    assert_eq!(f.commands.fits, 1);
}

// New test: a frame without a rendering time changes nothing.
#[test]
fn kinetic_frame_without_rendering_time_changes_nothing() {
    let mut f = fixture();
    start_glide(&mut f);
    frame(&mut f, 0);
    frame(&mut f, 16);
    let scrolls = f.surface.scrolls.len();
    let offset = f.surface.horizontal;
    f.run(|c, ctx| {
        c.on_kinetic_frame(ctx, None);
        PointerStep::Done
    });
    assert_eq!(f.surface.scrolls.len(), scrolls);
    assert_eq!(f.surface.horizontal, offset);
    assert!(f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 0);
    // The glide is intact: the next real frame still steps.
    frame(&mut f, 32);
    assert_eq!(f.surface.scrolls.len(), scrolls + 1);
}

// New test (T5): the velocity sample time wraps on `i32` overflow instead of panicking, and the flick still glides.
#[test]
fn velocity_sample_time_wraps_on_timestamp_overflow() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    drag_from(&mut f, i32::MAX - 30); // the timestamps cross i32::MAX during the drag
    assert!(f.controller.is_kinetic_hooked());
    let before = f.surface.horizontal;
    frame(&mut f, 0);
    frame(&mut f, 16);
    assert!(f.surface.horizontal > before, "a leftward flick keeps scrolling right");
}

// New test (T7): an impulse added mid-glide keeps the glide clock's anchoring (no restart, so the next frame still steps).
#[test]
fn impulse_mid_glide_keeps_the_glide_clock_anchoring() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    assert!(arrow(&mut f, KeyId::RIGHT, false));
    frame(&mut f, 0); // the clock records its first frame
    frame(&mut f, 16); // and steps
    let scrolls = f.surface.scrolls.len();
    assert_eq!(scrolls, 1);

    assert!(arrow(&mut f, KeyId::RIGHT, true)); // auto-repeat: an impulse onto the running glide
    frame(&mut f, 32);

    assert_eq!(
        f.surface.scrolls.len(),
        scrolls + 1,
        "a restarted clock would only record this frame"
    );
}

// New test: a fresh impulse after the glide ended restarts the clock (the first frame only records the time).
#[test]
fn impulse_on_an_idle_glide_restarts_the_glide_clock() {
    let mut f = fixture();
    zoom_in_so_the_image_can_pan(&mut f);
    assert!(arrow(&mut f, KeyId::RIGHT, false));
    drive_glide_to_rest(&mut f);
    let scrolls = f.surface.scrolls.len();
    assert!(arrow(&mut f, KeyId::RIGHT, false));
    frame(&mut f, 100_000);
    assert_eq!(
        f.surface.scrolls.len(),
        scrolls,
        "first frame of a fresh glide only records the time"
    );
    frame(&mut f, 100_016);
    assert_eq!(f.surface.scrolls.len(), scrolls + 1);
}

// ---- arrow keys ----

// C#: PointerInputControllerTests.Arrow_ZoomedImage_PansTenPercentOfTheShortSide_WithoutNavigating
#[test]
fn arrow_zoomed_image_pans_ten_percent_of_the_short_side_without_navigating() {
    let mut f = fixture();
    f.settings.kinetic_pan_enabled = false;
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    f.surface.horizontal = 100.0;
    f.surface.vertical = 100.0;

    assert!(arrow(&mut f, KeyId::RIGHT, false));
    assert_eq!(f.surface.scrolls.last(), Some(&(160.0, 100.0))); // 10 % of the 600-high short side, both axes
    assert!(arrow(&mut f, KeyId::DOWN, false));
    assert_eq!(f.surface.scrolls.last(), Some(&(160.0, 160.0)));
    assert!(arrow(&mut f, KeyId::LEFT, true));
    assert_eq!(f.surface.scrolls.last(), Some(&(100.0, 160.0)));
    assert!(arrow(&mut f, KeyId::UP, true));
    assert_eq!(f.surface.scrolls.last(), Some(&(100.0, 100.0)));
    assert_eq!(f.commands.next + f.commands.previous, 0);
}

// C#: PointerInputControllerTests.Arrow_StepFollowsTheArrowPanStepPercentSetting
#[test]
fn arrow_step_follows_the_arrow_pan_step_percent_setting() {
    let mut f = fixture();
    f.settings.kinetic_pan_enabled = false;
    f.settings.arrow_pan_step_percent = 25;
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    f.surface.horizontal = 100.0;
    f.surface.vertical = 100.0;

    assert!(arrow(&mut f, KeyId::RIGHT, false));
    assert_eq!(f.surface.scrolls.last(), Some(&(250.0, 100.0))); // 25 % of the 600 short side
    assert!(arrow(&mut f, KeyId::DOWN, false));
    assert_eq!(f.surface.scrolls.last(), Some(&(250.0, 250.0))); // the same 150 DIP vertically
}

// C#: PointerInputControllerTests.Arrow_StepIsClampedToTheEdge
#[test]
fn arrow_step_is_clamped_to_the_edge() {
    let mut f = fixture();
    f.settings.kinetic_pan_enabled = false;
    f.surface.extent_width = 2000.0;
    f.surface.horizontal = 1170.0; // max 1200

    assert!(arrow(&mut f, KeyId::RIGHT, false));
    assert_eq!(f.surface.scrolls.last(), Some(&(1200.0, 0.0)));
}

// C#: PointerInputControllerTests.Arrow_AtFit_IsNotUsed_SoLeftRightStillNavigate
#[test]
fn arrow_at_fit_is_not_used_so_left_right_still_navigate() {
    let mut f = fixture();
    assert!(!arrow(&mut f, KeyId::RIGHT, false));
    assert!(!arrow(&mut f, KeyId::DOWN, true));
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.Arrow_LegacySetting_AtTheEdge_RepeatIsSwallowed_FreshPressFallsThroughToNavigation
#[test]
fn arrow_legacy_setting_at_the_edge_repeat_is_swallowed_fresh_press_falls_through_to_navigation() {
    let mut f = fixture();
    f.settings.kinetic_pan_enabled = false;
    f.settings.arrow_key_navigates_at_zoom_edge = true;
    f.surface.extent_width = 2000.0;
    f.surface.horizontal = 1200.0;

    assert!(arrow(&mut f, KeyId::RIGHT, true));
    assert!(!arrow(&mut f, KeyId::RIGHT, false));
    assert!(f.surface.scrolls.is_empty());
    // The other direction still pans.
    assert!(arrow(&mut f, KeyId::LEFT, false));
    assert_eq!(f.surface.scrolls.last(), Some(&(1140.0, 0.0)));
}

// C#: PointerInputControllerTests.Arrow_LegacySetting_OnlyScrollableAxisPans_OtherKeysAndNoImageAreIgnored
#[test]
fn arrow_legacy_setting_only_scrollable_axis_pans_other_keys_and_no_image_are_ignored() {
    let mut f = fixture();
    f.settings.arrow_key_navigates_at_zoom_edge = true;
    f.surface.extent_width = 2000.0; // wide image: vertical fits

    assert!(!arrow(&mut f, KeyId::UP, false));
    assert!(!arrow(&mut f, KeyId::A, false));
    f.commands.has_images = false;
    assert!(!arrow(&mut f, KeyId::RIGHT, false));
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.Arrow_Default_OtherKeysAndNoImageAreIgnored
#[test]
fn arrow_default_other_keys_and_no_image_are_ignored() {
    let mut f = fixture();
    f.surface.extent_width = 2000.0;
    assert!(!arrow(&mut f, KeyId::A, false));
    f.commands.has_images = false;
    assert!(!arrow(&mut f, KeyId::RIGHT, false));
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.Arrow_Default_FreshPressAtTheEdge_IsConsumed_NoScrollNoNavigation
#[test]
fn arrow_default_fresh_press_at_the_edge_is_consumed_no_scroll_no_navigation() {
    let mut f = fixture();
    assert!(!f.settings.arrow_key_navigates_at_zoom_edge);
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    f.surface.horizontal = 1200.0;
    f.surface.vertical = 900.0;

    assert!(arrow(&mut f, KeyId::RIGHT, false));
    assert!(arrow(&mut f, KeyId::RIGHT, true));
    assert!(arrow(&mut f, KeyId::DOWN, false));
    f.surface.horizontal = 0.0;
    f.surface.vertical = 0.0;
    assert!(arrow(&mut f, KeyId::LEFT, false));
    assert!(arrow(&mut f, KeyId::UP, false));
    assert!(f.surface.scrolls.is_empty());
    assert_eq!(f.commands.next + f.commands.previous, 0);
}

// C#: PointerInputControllerTests.Arrow_Default_OnAnAxisThatCannotScroll_IsConsumedAsANoOp
#[test]
fn arrow_default_on_an_axis_that_cannot_scroll_is_consumed_as_a_no_op() {
    let mut f = fixture();
    f.surface.extent_height = 1500.0; // tall image zoomed only vertically
    f.surface.vertical = 100.0;

    assert!(arrow(&mut f, KeyId::LEFT, false));
    assert!(arrow(&mut f, KeyId::RIGHT, false));
    assert!(f.surface.scrolls.is_empty());
    assert_eq!(f.commands.next + f.commands.previous, 0);
}

// C#: PointerInputControllerTests.Arrow_Default_AtFit_FallsThrough
#[test]
fn arrow_default_at_fit_falls_through() {
    let mut f = fixture();
    assert!(!arrow(&mut f, KeyId::LEFT, false));
    assert!(!arrow(&mut f, KeyId::RIGHT, false));
    assert!(!arrow(&mut f, KeyId::UP, false));
    assert!(f.surface.scrolls.is_empty());
}

// C#: PointerInputControllerTests.Arrow_KineticPanEnabled_Panned_StartsAGlide_InsteadOfAnInstantScrollTo
#[test]
fn arrow_kinetic_pan_enabled_panned_starts_a_glide_instead_of_an_instant_scroll_to() {
    let mut f = fixture();
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    f.surface.horizontal = 100.0;
    f.surface.vertical = 100.0;

    assert!(arrow(&mut f, KeyId::RIGHT, false));

    assert_eq!(f.surface.hooks, 1);
    assert!(f.controller.is_kinetic_hooked());
    assert!(f.surface.scrolls.is_empty()); // no instant ScrollTo: only the glide's own frame steps move it
    assert_eq!(f.commands.next + f.commands.previous, 0);
}

// C#: PointerInputControllerTests.Arrow_KineticPanEnabled_DrivenToRest_TravelsAboutTheConfiguredStep
#[test]
fn arrow_kinetic_pan_enabled_driven_to_rest_travels_about_the_configured_step() {
    let mut f = fixture();
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    f.surface.horizontal = 100.0;
    f.surface.vertical = 100.0;
    let before = f.surface.horizontal;

    assert!(arrow(&mut f, KeyId::RIGHT, false));
    drive_glide_to_rest(&mut f); // the glide stopped on its own

    // The step at ArrowPanStepPercent = 10 % of the 600 short side is 60 DIP (matches the instant-mode test).
    assert!(
        (f.surface.horizontal - (before + 60.0)).abs() <= 1.0,
        "got {}",
        f.surface.horizontal
    );
}

// C#: PointerInputControllerTests.Arrow_KineticPanEnabled_AutoRepeat_AddsToTheRunningGlide_WithoutRehooking
#[test]
fn arrow_kinetic_pan_enabled_auto_repeat_adds_to_the_running_glide_without_rehooking() {
    let mut f = fixture();
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    f.surface.horizontal = 100.0;

    assert!(arrow(&mut f, KeyId::RIGHT, false));
    assert_eq!(f.surface.hooks, 1);
    assert!(f.controller.is_kinetic_hooked());

    assert!(arrow(&mut f, KeyId::RIGHT, true));

    assert_eq!(f.surface.hooks, 1); // still hooked once, not re-hooked
    assert!(f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 0);
}

// C#: PointerInputControllerTests.Arrow_KineticPanEnabled_AtTheEdge_ConsumedAsANoOp_DoesNotStartAGlide
#[test]
fn arrow_kinetic_pan_enabled_at_the_edge_consumed_as_a_no_op_does_not_start_a_glide() {
    let mut f = fixture();
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    f.surface.horizontal = 1200.0; // already at the max

    assert!(arrow(&mut f, KeyId::RIGHT, false));

    assert_eq!(f.surface.hooks, 0);
    assert!(!f.controller.is_kinetic_hooked());
}

// C#: PointerInputControllerTests.Arrow_Instant_HorizontalAndVerticalStepsAreTheSamePixels_ShortSideTimesPercent
#[test]
fn arrow_instant_horizontal_and_vertical_steps_are_the_same_pixels_short_side_times_percent() {
    for (viewport_w, viewport_h) in [(800.0, 600.0), (600.0, 800.0)] {
        let mut f = fixture();
        f.settings.kinetic_pan_enabled = false;
        f.settings.arrow_pan_step_percent = 17;
        f.surface.viewport_width = viewport_w;
        f.surface.viewport_height = viewport_h;
        f.surface.extent_width = 5000.0;
        f.surface.extent_height = 5000.0;
        f.surface.horizontal = 1000.0;
        f.surface.vertical = 1000.0;
        let expected: f64 = 600.0 * 0.17;

        assert!(arrow(&mut f, KeyId::RIGHT, false));
        let horizontal_step = f.surface.scrolls.last().expect("scrolled").0 - 1000.0;
        assert!(arrow(&mut f, KeyId::DOWN, false));
        let vertical_step = f.surface.scrolls.last().expect("scrolled").1 - 1000.0;

        assert!(
            (horizontal_step - expected).abs() < 1e-9,
            "{viewport_w}x{viewport_h}: {horizontal_step}"
        );
        assert!(
            (vertical_step - expected).abs() < 1e-9,
            "{viewport_w}x{viewport_h}: {vertical_step}"
        );
    }
}

// C#: PointerInputControllerTests.Arrow_Kinetic_HorizontalAndVerticalGlidesTravelTheSamePixels
#[test]
fn arrow_kinetic_horizontal_and_vertical_glides_travel_the_same_pixels() {
    let mut f = fixture();
    f.settings.arrow_pan_step_percent = 17;
    f.surface.extent_width = 5000.0;
    f.surface.extent_height = 5000.0;
    f.surface.horizontal = 1000.0;
    f.surface.vertical = 1000.0;

    assert!(arrow(&mut f, KeyId::RIGHT, false));
    drive_glide_to_rest(&mut f);
    let horizontal_travel = f.surface.horizontal - 1000.0;
    assert!(arrow(&mut f, KeyId::DOWN, false));
    drive_glide_to_rest(&mut f);
    let vertical_travel = f.surface.vertical - 1000.0;

    let expected: f64 = 600.0 * 0.17; // short side of the 800x600 viewport
    assert!((horizontal_travel - expected).abs() <= 1.0, "{horizontal_travel}");
    assert!((vertical_travel - expected).abs() <= 1.0, "{vertical_travel}");
}

fn glide_distance_of_one_down_arrow(start_offset: f64) -> f64 {
    let mut f = fixture();
    f.viewer.set_zoom(2.0);
    f.surface.extent_width = 800.0;
    f.surface.extent_height = 4000.0;
    f.surface.vertical = start_offset;
    assert!(arrow(&mut f, KeyId::DOWN, false));
    let mut t = 0;
    while t < 400 && f.controller.is_kinetic_hooked() {
        frame(&mut f, t);
        t += 16;
    }
    f.surface.vertical - start_offset
}

// C#: PointerInputControllerTests.MutationGaps.Arrow_KineticGlide_TravelsTheSameDistanceFromAnyStartingOffset
#[test]
fn arrow_kinetic_glide_travels_the_same_distance_from_any_starting_offset() {
    let from_top = glide_distance_of_one_down_arrow(0.0);
    let from_middle = glide_distance_of_one_down_arrow(1000.0);
    assert!(from_top > 0.0);
    assert!((from_top - from_middle).abs() < 1e-6, "{from_top} vs {from_middle}");
}

// New test: a non-arrow key and an unloaded surface give false without touching the surface.
#[test]
fn non_arrow_key_and_unloaded_surface_return_false_without_touching_the_surface() {
    let mut f = fixture();
    f.surface.extent_width = 2000.0;
    f.surface.extent_height = 1500.0;
    assert!(!arrow(&mut f, KeyId::A, false));
    f.surface.loaded = false;
    assert!(!arrow(&mut f, KeyId::RIGHT, false));
    assert!(f.surface.scrolls.is_empty());
    assert_eq!(f.surface.hooks, 0);
    assert_eq!(f.surface.unhooks, 0);
}

// New test: the instant arrow pan stops a running glide first.
#[test]
fn instant_arrow_pan_stops_a_running_glide() {
    let mut f = fixture();
    start_glide(&mut f);
    f.settings.kinetic_pan_enabled = false;
    assert!(arrow(&mut f, KeyId::RIGHT, false));
    assert!(!f.controller.is_kinetic_hooked());
    assert_eq!(f.surface.unhooks, 1);
}

// ---- rig with a scriptable monitor clock and an event log (display-timing reads, call order) ----

/// A `FakeSurface` plus a settable timestamp and display timing, counting `display_timing` reads and logging the order of
/// `release_mouse_capture` and `scroll_to`.
struct TimedSurface {
    inner: FakeSurface,
    now: i64,
    timing: Option<DisplayTiming>,
    reads: usize,
    events: Vec<&'static str>,
}

impl ImageSurface for TimedSurface {
    fn is_loaded(&self) -> bool {
        self.inner.is_loaded()
    }
    fn horizontal_offset(&self) -> f64 {
        self.inner.horizontal_offset()
    }
    fn vertical_offset(&self) -> f64 {
        self.inner.vertical_offset()
    }
    fn viewport_width(&self) -> f64 {
        self.inner.viewport_width()
    }
    fn viewport_height(&self) -> f64 {
        self.inner.viewport_height()
    }
    fn extent_width(&self) -> f64 {
        self.inner.extent_width()
    }
    fn extent_height(&self) -> f64 {
        self.inner.extent_height()
    }
    fn drag_threshold(&self) -> (f64, f64) {
        self.inner.drag_threshold()
    }
    fn scroll_to(&mut self, horizontal: f64, vertical: f64) {
        self.events.push("scroll_to");
        self.inner.scroll_to(horizontal, vertical);
    }
    fn update_layout(&mut self) {
        self.inner.update_layout();
    }
    fn yield_to_render(&mut self, continuation: Box<dyn FnOnce()>) {
        self.inner.yield_to_render(continuation);
    }
    fn image_origin(&self) -> PointD {
        self.inner.image_origin()
    }
    fn to_image_element(&self, surface_point: PointD) -> PointD {
        self.inner.to_image_element(surface_point)
    }
    fn image_actual_width(&self) -> f64 {
        self.inner.image_actual_width()
    }
    fn image_actual_height(&self) -> f64 {
        self.inner.image_actual_height()
    }
    fn source_size(&self) -> Option<SizeD> {
        self.inner.source_size()
    }
    fn capture_mouse(&mut self) {
        self.inner.capture_mouse();
    }
    fn release_mouse_capture(&mut self) {
        self.events.push("release_mouse_capture");
        self.inner.release_mouse_capture();
    }
    fn set_pan_cursor(&mut self, panning: bool) {
        self.inner.set_pan_cursor(panning);
    }
    fn hook_render_frame(&mut self, handler: RenderFrameHandler) -> RenderHookId {
        self.inner.hook_render_frame(handler)
    }
    fn unhook_render_frame(&mut self, id: RenderHookId) {
        self.inner.unhook_render_frame(id);
    }
    fn timestamp(&self) -> i64 {
        self.now
    }
    fn display_timing(&mut self) -> Option<DisplayTiming> {
        self.reads += 1;
        self.timing
    }
    fn pointer_position(&self) -> Option<PointD> {
        self.inner.pointer_position()
    }
}

struct Rig {
    controller: PointerInputController,
    surface: TimedSurface,
    viewer: ViewerState,
    settings: AppSettings,
    versions: ViewportOperationVersion,
    commands: FakeCommands,
}

impl Rig {
    fn new(smoothing: KineticGlideSmoothing) -> Self {
        let mut viewer = ViewerState::new();
        viewer.reset_fit(800.0, 600.0);
        let settings = AppSettings {
            mouse_wheel_action: MouseWheelAction::Zoom,
            keyboard_zoom_anchor: Some(KeyboardZoomAnchor::Pointer),
            click_to_zoom_enabled: true,
            click_zoom_percent: 200,
            kinetic_pan_enabled: true,
            kinetic_glide_smoothing: Some(smoothing),
            ..AppSettings::default()
        };
        Self {
            // C# `TicksPerMs = Stopwatch.Frequency / 1000.0`.
            controller: PointerInputController::new(STOPWATCH_FREQUENCY as f64 / 1000.0),
            surface: TimedSurface {
                inner: FakeSurface::default(),
                now: 0,
                timing: None,
                reads: 0,
                events: Vec::new(),
            },
            viewer,
            settings,
            versions: ViewportOperationVersion::new(),
            commands: FakeCommands {
                has_images: true,
                ..Default::default()
            },
        }
    }

    fn with<R>(&mut self, action: impl FnOnce(&mut PointerInputController, &mut PointerContext<'_>) -> R) -> R {
        let mut ctx = PointerContext {
            surface: &mut self.surface,
            viewer: &mut self.viewer,
            settings: &self.settings,
            versions: &mut self.versions,
            commands: &mut self.commands,
            log: &NullLog,
        };
        action(&mut self.controller, &mut ctx)
    }

    fn zoom_in_so_the_image_can_pan(&mut self) {
        self.viewer.set_zoom(2.0);
        self.surface.inner.extent_width = 4000.0;
        self.surface.inner.extent_height = 3000.0;
        self.surface.inner.horizontal = 500.0;
        self.surface.inner.vertical = 400.0;
    }

    fn press(&mut self, x: f64, y: f64, timestamp: i32) -> bool {
        self.with(|c, ctx| c.on_image_press(ctx, PointerButton::Left, 1, point(x, y), timestamp))
    }

    fn drag(&mut self) {
        assert!(self.press(400.0, 300.0, 1000));
        for t in (10..=60).step_by(10) {
            self.with(|c, ctx| c.on_image_move(ctx, true, point(400.0 - 2.0 * f64::from(t), 300.0), 1000 + t));
        }
        let released = self.with(|c, ctx| c.on_image_release(ctx, point(270.0, 300.0), 1065));
        assert!(released.handled);
    }

    fn start_glide(&mut self) {
        self.zoom_in_so_the_image_can_pan();
        self.drag();
        assert!(self.controller.is_kinetic_hooked());
    }

    fn frame(&mut self, ticks: i64) {
        self.with(|c, ctx| c.on_kinetic_frame(ctx, Some(ticks)));
    }
}

// C#: PointerInputControllerTests.Predict_StepsToTheMonitorRefresh_LikeOffWithRefreshAlignedFrameTimes
#[test]
fn predict_steps_to_the_monitor_refresh_like_off_with_refresh_aligned_frame_times() {
    // Callback times in refreshes: several per refresh, a missed one, one just before a refresh.
    let callbacks = [
        0.1, 0.3, 0.6, 1.2, 1.5, 3.4, 3.5, 4.05, 4.9, 6.2, 7.7, 7.8, 9.1, 12.4, 13.3,
    ];
    for hz in [60.0_f64, 75.0, 144.0, 240.0] {
        let period = STOPWATCH_FREQUENCY as f64 / hz;
        let period_ms = 1000.0 / hz;

        let mut predict = Rig::new(KineticGlideSmoothing::Predict);
        predict.surface.timing = Some(DisplayTiming {
            last_vblank: 0,
            refresh_period: period.round() as i64,
        });
        predict.start_glide();
        let mut predicted = Vec::new();
        for (i, callback) in callbacks.iter().enumerate() {
            if !predict.controller.is_kinetic_hooked() {
                break;
            }
            predict.surface.now = (callback * period) as i64;
            predict.frame((i as i64 + 1) * 10_000); // RenderingTime says 1 ms per callback
            predicted.push((predict.surface.inner.horizontal, predict.surface.inner.vertical));
        }

        let mut off = Rig::new(KineticGlideSmoothing::Off);
        off.start_glide();
        let mut reference = Vec::new();
        for callback in callbacks {
            if !off.controller.is_kinetic_hooked() {
                break;
            }
            // The refresh each callback is shown on: the first vblank at least the present lead after it.
            let lead = pr_app::input::kinetic_pan::GLIDE_PRESENT_LEAD_MS / period_ms;
            let refresh = (callback + lead).ceil();
            off.frame((refresh * period_ms * 10_000.0).round() as i64);
            reference.push((off.surface.inner.horizontal, off.surface.inner.vertical));
        }

        assert_eq!(reference.len(), predicted.len(), "{hz} Hz");
        for (i, (o, p)) in reference.iter().zip(&predicted).enumerate() {
            // 0.01 DIP: TimeSpan keeps 100 ns, so the Off reference's frame times are rounded slightly.
            assert!((o.0 - p.0).abs() <= 0.01, "{hz} Hz, callback {i}: {o:?} vs {p:?}");
            assert!((o.1 - p.1).abs() <= 0.01, "{hz} Hz, callback {i}: {o:?} vs {p:?}");
        }
        assert_ne!(predict.surface.inner.horizontal, 500.0, "{hz} Hz: it did glide");
    }
}

// C#: PointerInputControllerTests.Predict_ReadsTheMonitorTimingDuringTheDrag_OffNeverDoes
#[test]
fn predict_reads_the_monitor_timing_during_the_drag_off_never_does() {
    let mut predict = Rig::new(KineticGlideSmoothing::Predict);
    predict.zoom_in_so_the_image_can_pan();
    assert!(predict.press(400.0, 300.0, 1000));
    assert!(predict.surface.reads > 0, "The press did not start the monitor clock.");

    let mut off = Rig::new(KineticGlideSmoothing::Off);
    off.surface.timing = Some(DisplayTiming {
        last_vblank: 0,
        refresh_period: 1000,
    });
    off.start_glide();
    let mut t = 0;
    while t < 2000 && off.controller.is_kinetic_hooked() {
        off.frame(t * 10_000);
        t += 16;
    }
    assert_eq!(off.surface.reads, 0);
}

// New test: the frame reads the display timing only when the clock needs it (Predict yes, Off never, unloaded never).
#[test]
fn kinetic_frame_reads_display_timing_only_when_the_clock_needs_it() {
    let mut predict = Rig::new(KineticGlideSmoothing::Predict);
    predict.surface.timing = Some(DisplayTiming {
        last_vblank: 0,
        refresh_period: 166_667,
    });
    predict.start_glide();
    let after_drag = predict.surface.reads;
    assert!(after_drag > 0);
    predict.frame(10_000);
    assert_eq!(predict.surface.reads, after_drag + 1);
    // A frame without a rendering time returns before the clock is consulted.
    predict.with(|c, ctx| c.on_kinetic_frame(ctx, None));
    assert_eq!(predict.surface.reads, after_drag + 1);
    // An unloaded surface stops the glide before the clock is consulted.
    predict.surface.inner.loaded = false;
    predict.frame(20_000);
    assert_eq!(predict.surface.reads, after_drag + 1);
    assert!(!predict.controller.is_kinetic_hooked());
}

// New test (T3): the release cancels the pan (capture released) BEFORE the click-zoom pass scrolls.
#[test]
fn release_cancels_the_pan_before_the_click_zoom_runs() {
    let mut rig = Rig::new(KineticGlideSmoothing::Off);
    assert!(rig.press(100.0, 100.0, 1));
    assert!(rig.surface.events.is_empty());

    let result = rig.with(|c, ctx| c.on_image_release(ctx, point(100.0, 100.0), 2));

    assert!(result.handled);
    assert!(matches!(result.step, PointerStep::AwaitRender(_)));
    assert_eq!(rig.surface.events, ["release_mouse_capture", "scroll_to"]);
}
