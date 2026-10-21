//! Tests of `pr_platform_win::shell_viewing::scrollbars::{input, paint}` (W8-16g): the mouse handling, the page repeat and the painting of
//! `PhotoReview.Shell.Win32/Viewing/ScrollBars.cs` (baseline `d92d6aae`). Ported from `ShellScrollBarsTests.cs` (the part that needs messages and
//! painting), plus the new tests of `contracts/W8-16g.md` and the tests that kill the 16f mutation survivors (`to_dip`, `is_over_page_region`, the
//! `dragging || paging` guard of `hovered_thumb_axis`, `is_dragging`, `is_paging`, `drawn_frames`).
//! Test kind F: fake window with a handler chain, real `ViewportController` over a fake poster and frame clock, recording draw context. No sleeps.
//! Messages go through `ScrollWindow::dispatch` as packed pixel `lparam` values and the queue is drained after each (C# `ScrollBarRig.Mouse`).

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;
#[path = "common/viewing_scrollbars_fixture.rs"]
mod viewing_scrollbars_fixture;

use std::rc::Rc;

use pr_app::input::primitives::RectD;
use pr_app::viewing::scrollbar_geometry::{self, ScrollAxis};
use pr_platform_win::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use pr_platform_win::shell_viewing::scrollbar_native::{WM_CANCEL_MODE, WM_MOUSE_LEAVE};
use pr_platform_win::shell_viewing::scrollbars::{ScrollBarColors, ShellScrollBars};
use pr_platform_win::win32::messages::{
    WM_CAPTURECHANGED, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
};
use viewing_scrollbars_fixture::{RigConfig, ScrollBarRig};

const VERTICAL_TRAVEL: f64 = 586.0 - 172.87;
const HORIZONTAL_THUMB: f64 = 986.0 * 990.0 / 3000.0;
const HORIZONTAL_TRAVEL: f64 = 986.0 - HORIZONTAL_THUMB;

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} +/- {tolerance}, got {actual}"
    );
}

fn fit_rig() -> ScrollBarRig {
    ScrollBarRig::with(RigConfig {
        actual_size: false,
        ..RigConfig::default()
    })
}

fn color_of_vertical(draw: &viewing_scrollbars_fixture::FillRecorder) -> pr_app::viewport::render_geometry::ColorF {
    draw.rounded[0].2
}

// ---- drag ----

// C#: ShellScrollBarsTests.DraggingTheVerticalThumb_ScrollsProportionally_AndTheThumbStaysUnderTheGrabPoint
#[test]
fn dragging_the_vertical_thumb_scrolls_proportionally_and_the_thumb_stays_under_the_grab_point() {
    let rig = ScrollBarRig::new();

    assert_eq!(rig.down(995.0, 50.0), Some(0)); // 48 DIP below the thumb top
    assert!(rig.bars().is_dragging());
    assert!(rig.window.has_pointer_capture());
    assert_eq!(rig.viewport.vertical_offset(), 0.0);

    rig.move_to(995.0, 150.0);
    close(rig.viewport.vertical_offset(), 100.0 * 1410.0 / VERTICAL_TRAVEL, 1e-6);
    close(rig.layout().vertical.thumb.y, 102.0, 1e-6); // 150 - 48
    assert_eq!(rig.viewport.horizontal_offset(), 0.0);

    rig.move_to(995.0, 50.0 + 413.0); // near the end of the track
    close(rig.viewport.vertical_offset(), 413.0 * 1410.0 / VERTICAL_TRAVEL, 1e-6);
}

// C#: ShellScrollBarsTests.DraggingBeyondTheTrack_Clamps_AndComingBackFollowsTheCursorAgain
#[test]
fn dragging_beyond_the_track_clamps_and_coming_back_follows_the_cursor_again() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);

    rig.move_to(995.0, 5000.0);
    close(rig.viewport.vertical_offset(), 1410.0, 1e-9);
    rig.move_to(-300.0, -400.0); // outside the window (negative coordinates while captured)
    close(rig.viewport.vertical_offset(), 0.0, 1e-9);
    rig.move_to(300.0, 100.0); // any x: only the vertical coordinate decides
    close(
        rig.viewport.vertical_offset(),
        (100.0 - 48.0 - 2.0) * 1410.0 / VERTICAL_TRAVEL,
        1e-6,
    );
}

// C#: ShellScrollBarsTests.ReleasingTheButton_EndsTheDrag_ReleasesCapture_AndKeepsTheOffset
#[test]
fn releasing_the_button_ends_the_drag_releases_capture_and_keeps_the_offset() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    rig.move_to(995.0, 150.0);
    let offset = rig.viewport.vertical_offset();

    assert_eq!(rig.up(995.0, 150.0), Some(0));
    assert!(!rig.bars().is_dragging());
    assert!(!rig.window.has_pointer_capture());
    assert_eq!(rig.window.inner.releases.get(), 1);
    rig.move_to(995.0, 400.0);
    assert_eq!(rig.viewport.vertical_offset(), offset);
}

// C#: ShellScrollBarsTests.DraggingTheHorizontalThumb_ScrollsOnlyTheHorizontalOffset
#[test]
fn dragging_the_horizontal_thumb_scrolls_only_the_horizontal_offset() {
    let rig = ScrollBarRig::new();
    rig.viewport.scroll_to(0.0, 500.0);
    rig.viewport.update_layout();
    close(rig.layout().horizontal.thumb.x, 2.0, 1e-9);

    rig.down(100.0, 595.0); // 98 DIP after the thumb's left edge
    rig.move_to(300.0, 595.0);

    close(
        rig.viewport.horizontal_offset(),
        (300.0 - 98.0 - 2.0) * 2010.0 / HORIZONTAL_TRAVEL,
        1e-6,
    );
    close(rig.viewport.vertical_offset(), 500.0, 1e-9);
}

// C#: ShellScrollBarsTests.DraggingTheWholeTravel_ReachesExactlyTheMaximumOffset
#[test]
fn dragging_the_whole_travel_reaches_exactly_the_maximum_offset() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    rig.move_to(995.0, 50.0 + 413.13);
    rig.move_to(995.0, 600.0);
    close(rig.viewport.vertical_offset(), 1410.0, 1e-9);
    close(rig.layout().vertical.thumb.bottom(), 588.0, 1e-6);
}

// C#: ShellScrollBarsTests.TheDragFollowsWhateverLayoutChangesDuringIt_NoStaleThumbGeometry
#[test]
fn the_drag_follows_whatever_layout_changes_during_it() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    rig.move_to(995.0, 100.0);
    rig.resize(1000.0, 900.0);
    let bar = rig.layout().vertical;
    rig.move_to(995.0, 300.0);
    let expected = scrollbar_geometry::offset_for_thumb_start(&bar, 300.0 - 48.0);
    close(rig.viewport.vertical_offset(), expected, 1e-6);
}

// New: the layout stops showing the bar mid-drag (Fit): the move does nothing, the drag ends with the release.
#[test]
fn a_bar_that_disappears_mid_drag_is_left_alone() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    rig.set_zoom(0.2); // 600x400 fits: no bar
    assert!(!rig.layout().any_visible());
    let before = rig.viewport.vertical_offset();
    rig.move_to(995.0, 300.0);
    assert_eq!(rig.viewport.vertical_offset(), before);
    assert_eq!(rig.up(995.0, 300.0), Some(0));
    assert!(!rig.bars().is_dragging());
}

// ---- paging ----

// C#: ShellScrollBarsTests.ClickingTheTrackBelowTheThumb_PagesDownByOneViewport
#[test]
fn clicking_the_track_below_the_thumb_pages_down_by_one_viewport() {
    let rig = ScrollBarRig::new();
    assert_eq!(rig.down(995.0, 300.0), Some(0));
    close(rig.viewport.vertical_offset(), 590.0, 1e-9);
    assert_eq!(rig.viewport.horizontal_offset(), 0.0);
    assert!(rig.bars().is_paging());
    assert!(!rig.bars().is_dragging());
    assert!(rig.window.has_pointer_capture());
}

// C#: ShellScrollBarsTests.ClickingTheTrackAboveTheThumb_PagesUp_AndTheHorizontalTrackPagesByTheViewportWidth
#[test]
fn clicking_the_track_above_the_thumb_pages_up_and_the_horizontal_track_pages_by_the_viewport_width() {
    let rig = ScrollBarRig::new();
    rig.viewport.scroll_to(1000.0, 1000.0);
    rig.viewport.update_layout();
    let thumb = rig.layout().vertical.thumb;
    assert!(thumb.y > 20.0);

    rig.down(995.0, thumb.y - 10.0);
    rig.up(995.0, thumb.y - 10.0);
    close(rig.viewport.vertical_offset(), 1000.0 - 590.0, 1e-9);

    let h_thumb = rig.layout().horizontal.thumb;
    rig.down(h_thumb.right() + 20.0, 595.0);
    close(rig.viewport.horizontal_offset(), 1000.0 + 990.0, 1e-9);
    rig.up(h_thumb.right() + 20.0, 595.0);
    rig.down(3.0, 595.0); // the start of the horizontal track, before the thumb
    close(rig.viewport.horizontal_offset(), 1000.0, 1e-9);
}

// C#: ShellScrollBarsTests.HoldingOnTheTrack_RepeatsAfterTheKeyboardDelay_ThenEveryInterval_UntilTheThumbReachesTheCursor
#[test]
fn holding_on_the_track_repeats_after_the_keyboard_delay_then_every_interval_until_the_thumb_reaches_the_cursor() {
    let rig = ScrollBarRig::new(); // repeat timing (500, 50), 1000 ticks per second
    rig.now.set(0);
    rig.down(995.0, 580.0);
    close(rig.viewport.vertical_offset(), 590.0, 1e-9);

    rig.tick_at(499);
    close(rig.viewport.vertical_offset(), 590.0, 1e-9); // the delay has not passed
    rig.tick_at(500);
    close(rig.viewport.vertical_offset(), 1180.0, 1e-9);
    rig.tick_at(549);
    close(rig.viewport.vertical_offset(), 1180.0, 1e-9); // the interval has not passed
    rig.tick_at(550);
    close(rig.viewport.vertical_offset(), 1410.0, 1e-9); // clamped at the end
    let thumb = rig.layout().vertical.thumb;
    assert!(
        580.0 >= thumb.y && 580.0 < thumb.bottom(),
        "the thumb must have reached the cursor"
    );

    rig.tick_at(600);
    rig.tick_at(5000);
    close(rig.viewport.vertical_offset(), 1410.0, 1e-9);
}

// C#: ShellScrollBarsTests.TheRepeatStopsWhenTheThumbArrivesUnderTheCursor_BeforeTheEndOfTheTrack
#[test]
fn the_repeat_stops_when_the_thumb_arrives_under_the_cursor_before_the_end_of_the_track() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 500.0); // after page 1 the thumb is at y 174.9..347.7: the cursor is still below it
    rig.tick_at(500); // page 2: the thumb 347.7..520.6 contains y = 500
    close(rig.viewport.vertical_offset(), 1180.0, 1e-9);
    rig.tick_at(550);
    rig.tick_at(2000);
    close(rig.viewport.vertical_offset(), 1180.0, 1e-9);
}

// C#: ShellScrollBarsTests.ReleasingStopsTheRepeat_AndUnhooksTheFrameClock
#[test]
fn releasing_stops_the_repeat_and_unhooks_the_frame_clock() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 580.0);
    assert!(rig.clock.subscriber_count() > 0);

    rig.up(995.0, 580.0);
    assert!(!rig.bars().is_paging());
    assert!(!rig.window.has_pointer_capture());
    assert_eq!(rig.clock.subscriber_count(), 0);
    rig.tick_at(5000);
    close(rig.viewport.vertical_offset(), 590.0, 1e-9);
}

// C#: ShellScrollBarsTests.LeavingTheRegionWhileHolding_SuspendsTheRepeat_AndComingBackRestartsTheDelay
#[test]
fn leaving_the_region_while_holding_suspends_the_repeat_and_coming_back_restarts_the_delay() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 580.0);
    rig.move_to(500.0, 300.0); // out of the track (button still held, capture kept)
    rig.tick_at(1000);
    close(rig.viewport.vertical_offset(), 590.0, 1e-9);

    rig.now.set(2000);
    rig.move_to(995.0, 580.0); // back in: the delay counts again from here
    rig.tick_at(2499);
    close(rig.viewport.vertical_offset(), 590.0, 1e-9);
    rig.tick_at(2500);
    close(rig.viewport.vertical_offset(), 1180.0, 1e-9);
}

// New (kills `is_over_page_region` mutants of 16f): paging UP keeps repeating while the pointer stays on the track BEFORE the thumb.
#[test]
fn holding_above_the_thumb_pages_up_again_after_the_delay() {
    let rig = ScrollBarRig::new();
    rig.viewport.scroll_to(0.0, 1000.0);
    rig.viewport.update_layout();
    rig.down(995.0, 50.0); // TrackBefore: the thumb is at about y 292
    close(rig.viewport.vertical_offset(), 410.0, 1e-9);
    rig.tick_at(500);
    close(rig.viewport.vertical_offset(), 0.0, 1e-9);
}

// New (kills `&&` -> `||` and the axis comparison in `is_over_page_region`): the pointer on the HORIZONTAL track-after is not the vertical page region.
#[test]
fn the_other_axis_track_is_not_the_page_region() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 580.0); // vertical TrackAfter
    close(rig.viewport.vertical_offset(), 590.0, 1e-9);
    rig.move_to(600.0, 595.0); // horizontal TrackAfter: same part kind, other axis
    rig.tick_at(1000);
    close(rig.viewport.vertical_offset(), 590.0, 1e-9);
    assert_eq!(rig.viewport.horizontal_offset(), 0.0);
    // and back on the vertical track: the delay restarts (not an immediate page)
    rig.now.set(2000);
    rig.move_to(995.0, 580.0);
    rig.tick_at(2499);
    close(rig.viewport.vertical_offset(), 590.0, 1e-9);
    rig.tick_at(2500);
    close(rig.viewport.vertical_offset(), 1180.0, 1e-9);
}

// New (kills the `==` -> `!=` mutant): the same part kind on the right axis is the region; the wanted part of the OTHER direction is not.
#[test]
fn the_track_on_the_other_side_of_the_thumb_is_not_the_page_region() {
    let rig = ScrollBarRig::new();
    rig.viewport.scroll_to(0.0, 705.0);
    rig.viewport.update_layout();
    rig.down(995.0, 580.0); // TrackAfter, pages down to 1295
    rig.move_to(995.0, 10.0); // TrackBefore of the same axis
    rig.tick_at(1000);
    close(rig.viewport.vertical_offset(), 1295.0, 1e-9);
}

// ---- swallowing ----

// C#: ShellScrollBarsTests.PressesOnTheBarBackgroundAndCorner_AreSwallowed_NeverPanningTheImageUnderneath (the bars half; the no-bars half is W8-14c)
#[test]
fn presses_on_the_bar_background_and_corner_are_swallowed_never_panning_the_image_underneath() {
    let rig = ScrollBarRig::new();
    assert_eq!(rig.down(990.0, 300.0), Some(0)); // the track's left margin
    assert!(!rig.window.has_pointer_capture());
    assert!(!rig.bars().is_dragging() && !rig.bars().is_paging());
    assert_eq!(rig.up(990.0, 300.0), None);

    assert_eq!(rig.down(995.0, 595.0), Some(0)); // the corner
    assert!(!rig.window.has_pointer_capture());
    assert_eq!(rig.viewport.vertical_offset(), 0.0);
    assert_eq!(rig.viewport.horizontal_offset(), 0.0);
}

// C#: ShellScrollBarsTests.PressesOutsideTheBars_PassToTheImage (the bars half)
#[test]
fn presses_outside_the_bars_pass_to_the_image() {
    let rig = ScrollBarRig::new();
    assert_eq!(rig.down(500.0, 300.0), None);
    assert!(!rig.window.has_pointer_capture());
    assert!(!rig.bars().is_dragging());
    assert_eq!(rig.presses.get(), 0);
}

// C#: ShellScrollBarsTests.RightAndMiddleButtons_AreNotTakenByTheBars
#[test]
fn right_and_middle_buttons_are_not_taken_by_the_bars() {
    let rig = ScrollBarRig::new();
    assert_eq!(rig.mouse(WM_RBUTTONDOWN, 995.0, 300.0), None);
    assert_eq!(rig.mouse(WM_MBUTTONDOWN, 995.0, 300.0), None);
    assert_eq!(rig.presses.get(), 0);
    assert!(!rig.bars().is_dragging() && !rig.bars().is_paging());
}

// C#: ShellScrollBarsTests.TheWheelOverABar_StillGoesToTheInputLayer (reduced: the bars answer `None`; the full path needs ShellInput, W8-14c)
#[test]
fn the_wheel_over_a_bar_is_not_taken_by_the_bars() {
    let rig = ScrollBarRig::new();
    let wparam = (120_usize << 16) | 0x0008;
    assert_eq!(
        rig.window.dispatch(WM_MOUSEWHEEL, wparam, ScrollBarRig::pack(995, 300)),
        None
    );
    assert!(!rig.bars().is_dragging() && !rig.bars().is_paging());
    assert_eq!(rig.presses.get(), 0);
}

// C#: ShellScrollBarsTests.EveryPressOnABar_StopsKineticGlideLikeWindowPreviewMouseDown
#[test]
fn every_press_on_a_bar_stops_kinetic_glide() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    rig.up(995.0, 50.0);
    rig.down(995.0, 300.0);
    rig.up(995.0, 300.0);
    rig.down(990.0, 300.0);
    rig.up(990.0, 300.0);
    assert_eq!(rig.presses.get(), 3);
    rig.down(500.0, 300.0); // outside the bars: not the bars' business
    assert_eq!(rig.presses.get(), 3);
}

// C#: ShellScrollBarsTests.WhenTheImageFits_NothingIsDrawnOrHandled (the draw and press half)
#[test]
fn when_the_image_fits_nothing_is_drawn_or_handled() {
    let rig = fit_rig();
    assert!(!rig.layout().any_visible());
    let draw = rig.draw();
    assert!(draw.rects.is_empty());
    assert!(draw.rounded.is_empty());
    assert_eq!(rig.bars().drawn_frames(), 0);

    assert_eq!(rig.down(995.0, 300.0), None);
    assert_eq!(rig.presses.get(), 0);
    assert!(!rig.bars().is_paging());
}

// ---- compare panel ----

// C#: ShellScrollBarsTests.ComparePanel_CoversTheBars_ExceptTheStripsOutsideItsMargin (the clip half)
#[test]
fn compare_panel_covers_the_bars_except_the_strips_outside_its_margin() {
    let rig = ScrollBarRig::new();
    rig.compare_visible.set(true);

    let draw = rig.draw();
    assert_eq!(
        draw.clips,
        vec![rect(992.0, 0.0, 8.0, 600.0), rect(0.0, 592.0, 1000.0, 8.0)]
    );
    assert_eq!(draw.clip_depth, 0);
    // The bars are drawn once per strip: push, vertical (bar, thumb), horizontal (bar, thumb), corner, pop.
    assert_eq!(
        draw.order,
        vec![
            "push", "fill", "round", "fill", "round", "fill", "pop", "push", "fill", "round", "fill", "round", "fill",
            "pop"
        ]
    );
    assert_eq!(rig.bars().drawn_frames(), 1);
}

// New (T7): `pop_clip` runs for every strip even when it fails; the error is returned after both strips.
#[test]
fn a_failing_pop_clip_still_pops_every_strip_and_the_error_is_returned() {
    let rig = ScrollBarRig::new();
    rig.compare_visible.set(true);
    let mut recorder = viewing_scrollbars_fixture::FillRecorder {
        pop_failures: true,
        ..Default::default()
    };
    let result = rig.bars().draw(&mut recorder);
    assert!(result.is_err());
    assert_eq!(recorder.pops, 2);
    assert_eq!(recorder.clips.len(), 2);
}

// ---- painting ----

// C#: ShellScrollBarsTests.DrawsTrackBackgrounds_RoundedThumbs_AndTheCorner
#[test]
fn draws_track_backgrounds_rounded_thumbs_and_the_corner() {
    let rig = ScrollBarRig::new();
    let draw = rig.draw();

    assert_eq!(
        draw.rects,
        vec![
            (rect(990.0, 0.0, 10.0, 590.0), ScrollBarColors::TRACK),
            (rect(0.0, 590.0, 990.0, 10.0), ScrollBarColors::TRACK),
            (rect(990.0, 590.0, 10.0, 10.0), ScrollBarColors::TRACK),
        ]
    );
    assert_eq!(draw.rounded.len(), 2);
    for (_, radius, color) in &draw.rounded {
        assert_eq!(*radius, 4.0);
        assert_eq!(*color, ScrollBarColors::THUMB);
    }
    assert_eq!(draw.rounded[0].0, rig.layout().vertical.thumb);
    assert_eq!(draw.rounded[1].0, rig.layout().horizontal.thumb);
    assert_eq!(draw.order, vec!["fill", "round", "fill", "round", "fill"]);
    assert_eq!(rig.bars().drawn_frames(), 1);
}

// C#: ShellScrollBarsTests.OnlyTheBarThatIsShown_IsDrawn_AndNoCornerForASingleBar
#[test]
fn only_the_bar_that_is_shown_is_drawn_and_no_corner_for_a_single_bar() {
    let rig = ScrollBarRig::with(RigConfig {
        image_width: 900,
        image_height: 2000,
        ..RigConfig::default()
    });
    let draw = rig.draw();
    assert_eq!(draw.rects.len(), 1);
    assert_eq!(draw.rects[0].0, rect(990.0, 0.0, 10.0, 600.0));
    assert_eq!(draw.rounded.len(), 1);
}

// New (kills `drawn_frames` mutants): the counter counts the frames in which a bar was drawn, nothing else.
#[test]
fn drawn_frames_counts_the_frames_with_a_visible_bar() {
    let rig = ScrollBarRig::new();
    assert_eq!(rig.bars().drawn_frames(), 0);
    rig.draw();
    rig.draw();
    assert_eq!(rig.bars().drawn_frames(), 2);
    rig.set_zoom(0.2); // fits: no bar
    rig.draw();
    assert_eq!(rig.bars().drawn_frames(), 2);
}

// ---- hover / pressed ----

// C#: ShellScrollBarsTests.ThumbColor_FollowsHoverAndPressedStates_PerAxis
#[test]
fn thumb_color_follows_hover_and_pressed_states_per_axis() {
    let rig = ScrollBarRig::new();

    rig.move_to(995.0, 50.0);
    let hover = rig.draw();
    assert_eq!(hover.rounded[0].2, ScrollBarColors::THUMB_HOVER);
    assert_eq!(hover.rounded[1].2, ScrollBarColors::THUMB); // the horizontal bar is not hovered

    rig.move_to(995.0, 300.0); // on the track: the thumb is not hovered
    assert_eq!(color_of_vertical(&rig.draw()), ScrollBarColors::THUMB);

    rig.move_to(100.0, 595.0);
    let horizontal_hover = rig.draw();
    assert_eq!(horizontal_hover.rounded[0].2, ScrollBarColors::THUMB);
    assert_eq!(horizontal_hover.rounded[1].2, ScrollBarColors::THUMB_HOVER);

    rig.down(100.0, 595.0);
    let pressed = rig.draw();
    assert_eq!(pressed.rounded[1].2, ScrollBarColors::THUMB_PRESSED);
    assert_eq!(pressed.rounded[0].2, ScrollBarColors::THUMB);

    rig.up(100.0, 595.0);
    assert_eq!(rig.draw().rounded[1].2, ScrollBarColors::THUMB_HOVER); // the cursor is still on the thumb
}

// C#: ShellScrollBarsTests.WhileTheTrackIsHeld_AThumbThatArrivesUnderTheCursorIsNotHovered_UntilTheButtonIsReleased
#[test]
fn while_the_track_is_held_a_thumb_that_arrives_under_the_cursor_is_not_hovered_until_the_button_is_released() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 500.0);
    rig.tick_at(500); // page 2: the thumb reaches the cursor
    let thumb = rig.layout().vertical.thumb;
    assert!(500.0 >= thumb.y && 500.0 < thumb.bottom());

    // Kills the `dragging || paging` guard mutants of `hovered_thumb_axis` (paging half).
    assert_eq!(rig.bars().hovered_thumb_axis(), None);
    assert_eq!(color_of_vertical(&rig.draw()), ScrollBarColors::THUMB);
    rig.up(995.0, 500.0);
    assert_eq!(rig.bars().hovered_thumb_axis(), Some(ScrollAxis::Vertical));
    assert_eq!(color_of_vertical(&rig.draw()), ScrollBarColors::THUMB_HOVER);
}

// New (kills the `dragging || paging` guard mutants of `hovered_thumb_axis`, drag half): the pointer is on the dragged thumb but nothing is hovered.
#[test]
fn hover_is_none_while_dragging_even_over_the_thumb() {
    let rig = ScrollBarRig::new();
    rig.move_to(995.0, 50.0);
    assert_eq!(rig.bars().hovered_thumb_axis(), Some(ScrollAxis::Vertical));
    rig.down(995.0, 50.0);
    assert!(rig.bars().is_dragging());
    assert_eq!(rig.bars().hovered_thumb_axis(), None);
    rig.move_to(995.0, 52.0); // the thumb moved by 2 DIP: the pointer is still on it
    assert_eq!(rig.bars().hovered_thumb_axis(), None);
    rig.up(995.0, 52.0);
    assert_eq!(rig.bars().hovered_thumb_axis(), Some(ScrollAxis::Vertical));
}

// C#: ShellScrollBarsTests.HoverIsCleared_WhenTheMouseLeavesTheWindow_AndTrackingIsRequestedOncePerEntry
#[test]
fn hover_is_cleared_when_the_mouse_leaves_the_window_and_tracking_is_requested_once_per_entry() {
    let rig = ScrollBarRig::new();
    rig.move_to(995.0, 50.0);
    rig.move_to(995.0, 60.0);
    assert_eq!(*rig.tracked_leave.borrow(), vec![rig.window.hwnd()]);
    assert_eq!(color_of_vertical(&rig.draw()), ScrollBarColors::THUMB_HOVER);

    assert_eq!(rig.window.dispatch(WM_MOUSE_LEAVE, 0, 0), None);
    assert_eq!(color_of_vertical(&rig.draw()), ScrollBarColors::THUMB);

    rig.move_to(995.0, 50.0);
    assert_eq!(rig.tracked_leave.borrow().len(), 2);
}

// New: invalidation happens exactly when the hovered thumb changes (and on leave with a hover).
#[test]
fn the_window_is_invalidated_only_when_the_hovered_thumb_changes() {
    let rig = ScrollBarRig::new();
    let count = || rig.window.inner.invalidations.get();
    let start = count();
    rig.move_to(500.0, 300.0); // nowhere
    assert_eq!(count(), start);
    rig.move_to(995.0, 50.0); // onto the thumb
    assert_eq!(count(), start + 1);
    rig.move_to(995.0, 60.0); // still on it
    assert_eq!(count(), start + 1);
    rig.window.dispatch(WM_MOUSE_LEAVE, 0, 0); // leaving with a hover
    assert_eq!(count(), start + 2);
    rig.window.dispatch(WM_MOUSE_LEAVE, 0, 0); // leaving without
    assert_eq!(count(), start + 2);
}

// ---- system messages ----

// C#: ShellScrollBarsTests.LosingCapture_EndsTheDrag
#[test]
fn losing_capture_ends_the_drag() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    assert_eq!(rig.window.dispatch(WM_CAPTURECHANGED, 0, 0), None);
    assert!(!rig.bars().is_dragging());
    assert_eq!(rig.window.inner.releases.get(), 0); // the capture is already gone: not released again
    rig.move_to(995.0, 300.0);
    assert_eq!(rig.viewport.vertical_offset(), 0.0);
}

// C#: ShellScrollBarsTests.CancelMode_EndsTheInteraction_AndReleasesTheCapture
#[test]
fn cancel_mode_ends_the_interaction_and_releases_the_capture() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    assert_eq!(rig.window.dispatch(WM_CANCEL_MODE, 0, 0), None);
    assert!(!rig.bars().is_dragging());
    assert!(!rig.window.has_pointer_capture());
    assert_eq!(rig.window.inner.releases.get(), 1);
}

// C#: ShellScrollBarsTests.ButtonUpWithoutAnInteraction_IsNotTaken
#[test]
fn button_up_without_an_interaction_is_not_taken() {
    let rig = ScrollBarRig::new();
    assert_eq!(rig.up(995.0, 50.0), None);
    assert!(!rig.bars().is_dragging() && !rig.bars().is_paging());
}

// New: the capture is released only while the window still holds it.
#[test]
fn the_capture_is_not_released_when_the_window_no_longer_holds_it() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    rig.window.inner.has_capture.set(false); // someone else took it without telling us
    assert_eq!(rig.up(995.0, 50.0), Some(0));
    assert_eq!(rig.window.inner.releases.get(), 0);
}

// ---- DPI 150 (kills the `to_dip` survivors of 16f) ----

// C#: ShellScrollBarsTests.Dpi150_PixelCoordinatesAreConvertedToDip_ForPressAndDrag
// Kills `to_dip`: `/` -> `%`, `/` -> `*` (x and y) and the constant result. At DPI 1.0 the three are indistinguishable.
#[test]
fn dpi150_pixel_coordinates_are_converted_to_dip_for_press_and_drag() {
    let rig = ScrollBarRig::with(RigConfig {
        dpi_scale: 1.5,
        ..RigConfig::default()
    });
    let bar = rig.layout().vertical;
    assert!(bar.visible);
    close(bar.bar.x, 990.0, 1e-9);
    let start_offset = rig.viewport.vertical_offset();
    let grab_pixel = ((bar.thumb.y + 20.0) * 1.5).round();
    let move_pixel = ((bar.thumb.y + 110.0) * 1.5).round();

    assert_eq!(rig.down(996.0 * 1.5, grab_pixel), Some(0));
    assert!(rig.bars().is_dragging());
    rig.move_to(996.0 * 1.5, move_pixel);

    let grab = grab_pixel / 1.5 - bar.thumb.y;
    let expected = scrollbar_geometry::offset_for_thumb_start(&bar, move_pixel / 1.5 - grab);
    close(rig.viewport.vertical_offset(), expected, 1e-6);
    assert!(rig.viewport.vertical_offset() > start_offset);
}

// New (kills the same `to_dip` mutants through the horizontal axis and a hit test): the point is converted on BOTH coordinates.
#[test]
fn dpi150_a_press_on_the_horizontal_thumb_hits_and_drags_in_dip() {
    let rig = ScrollBarRig::with(RigConfig {
        dpi_scale: 1.5,
        ..RigConfig::default()
    });
    let bar = rig.layout().horizontal;
    assert!(bar.visible && bar.thumb_visible);
    let y_pixel = ((bar.bar.y + 5.0) * 1.5).round();
    let x_pixel = ((bar.thumb.x + 10.0) * 1.5).round();
    assert_eq!(rig.down(x_pixel, y_pixel), Some(0));
    assert!(rig.bars().is_dragging());
    rig.move_to(x_pixel + 60.0, y_pixel);
    // 60 px = 40 DIP of thumb travel
    let grab = x_pixel / 1.5 - bar.thumb.x;
    let expected = scrollbar_geometry::offset_for_thumb_start(&bar, (x_pixel + 60.0) / 1.5 - grab);
    assert!(expected > 0.0);
    close(rig.viewport.horizontal_offset(), expected, 1e-6);
    assert_eq!(rig.viewport.vertical_offset(), 0.0);
}

// ---- contract extras ----

// New: `lparam` words are signed 16-bit; a zero-extended and a sign-extended (64-bit) negative value give the same point (T3).
#[test]
fn negative_words_zero_and_sign_extended_give_the_same_point() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 50.0);
    rig.window.dispatch(WM_MOUSEMOVE, 0, ScrollBarRig::pack(-300, -400));
    close(rig.viewport.vertical_offset(), 0.0, 1e-9);
    rig.window.dispatch(WM_MOUSEMOVE, 0, ScrollBarRig::pack(995, 5000));
    close(rig.viewport.vertical_offset(), 1410.0, 1e-9);
    // the same -300,-400 as a sign-extended isize
    let sign_extended = (((-400_i32 as u16 as u32) << 16) | (-300_i32 as u16 as u32)) as i32 as isize;
    assert!(sign_extended < 0);
    rig.window.dispatch(WM_MOUSEMOVE, 0, sign_extended);
    close(rig.viewport.vertical_offset(), 0.0, 1e-9);
    // isize::MIN: both words are 0 -> the point (0, 0) is not a bar: no panic, the drag follows y = 0
    rig.window.dispatch(WM_MOUSEMOVE, 0, isize::MIN);
    close(rig.viewport.vertical_offset(), 0.0, 1e-9);
}

// New: a double click is a press.
#[test]
fn dblclk_is_a_press() {
    let rig = ScrollBarRig::new();
    let outcome = rig.window.dispatch(WM_LBUTTONDBLCLK, 0, ScrollBarRig::pack(995, 50));
    assert_eq!(outcome, Some(0));
    assert!(rig.bars().is_dragging());
    assert_eq!(rig.presses.get(), 1);
}

// New: a press ends a previous interaction whose release was lost (one hook, the new interaction replaces it).
#[test]
fn press_ends_a_previous_interaction_first() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 580.0);
    assert!(rig.bars().is_paging());
    assert_eq!(rig.clock.subscriber_count(), 1);

    let thumb = rig.layout().vertical.thumb;
    rig.down(995.0, thumb.y + 5.0);
    assert!(rig.bars().is_dragging());
    assert!(!rig.bars().is_paging());
    assert_eq!(rig.clock.subscriber_count(), 0);
}

// New: no `track_mouse_leave` request without a window handle (C# `Hwnd == 0`); it is asked for once the window has one.
#[test]
fn move_without_hwnd_does_not_track() {
    let rig = ScrollBarRig::new();
    rig.window.hwnd_override.set(Some(0));
    rig.move_to(995.0, 50.0);
    assert!(rig.tracked_leave.borrow().is_empty());
    rig.window.hwnd_override.set(Some(0x77));
    rig.move_to(995.0, 51.0);
    assert_eq!(*rig.tracked_leave.borrow(), vec![0x77]);
}

// New (T5): one hook while paging, however often the press is repeated, and released on every exit path.
#[test]
fn frame_hook_is_registered_once_and_released_on_every_exit_path() {
    let page = |rig: &ScrollBarRig| {
        rig.down(995.0, 580.0);
        assert_eq!(rig.clock.subscriber_count(), 1);
    };

    let rig = ScrollBarRig::new();
    page(&rig);
    rig.up(995.0, 580.0);
    assert_eq!(rig.clock.subscriber_count(), 0);

    let rig = ScrollBarRig::new();
    page(&rig);
    rig.window.dispatch(WM_CAPTURECHANGED, 0, 0);
    assert_eq!(rig.clock.subscriber_count(), 0);

    let rig = ScrollBarRig::new();
    page(&rig);
    rig.window.dispatch(WM_CANCEL_MODE, 0, 0);
    assert_eq!(rig.clock.subscriber_count(), 0);

    let rig = ScrollBarRig::new();
    page(&rig);
    rig.bars().dispose();
    assert_eq!(rig.clock.subscriber_count(), 0);

    // a second press on the track after a release hooks again, exactly once
    let rig = ScrollBarRig::new();
    page(&rig);
    rig.up(995.0, 580.0);
    page(&rig);
}

// New: the frame hook delivered while not paging unhooks itself (C# `OnFrame`).
#[test]
fn on_frame_when_not_paging_unhooks() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 580.0);
    // end the paging without the bars noticing the hook (the flag only), the next frame unhooks
    rig.window.dispatch(WM_CAPTURECHANGED, 0, 0);
    assert_eq!(rig.clock.subscriber_count(), 0);
    rig.tick_at(1000);
    assert_eq!(rig.clock.subscriber_count(), 0);
}

// New: bars whose window is gone have no layout, take no message, draw nothing and do not panic.
#[test]
fn bars_whose_window_is_gone_do_nothing() {
    let rig = ScrollBarRig::new();
    let bars = {
        let window = viewing_scrollbars_fixture::ScrollWindow::new(viewing_fakes::FakeWindow::new());
        let api: Rc<dyn ShellWindowApi> = window;
        ShellScrollBars::new(&api, Rc::clone(&rig.viewport), Rc::new(|| false), None, None)
    }; // the window is dropped here; the bars hold it weakly
    assert!(!bars.current_layout().any_visible());
    let press = WindowMessage {
        hwnd: 1,
        msg: WM_LBUTTONDOWN,
        wparam: 1,
        lparam: ScrollBarRig::pack(995, 50),
    };
    assert_eq!(bars.try_handle(&press), None);
    let mut recorder = viewing_scrollbars_fixture::FillRecorder::default();
    bars.draw(&mut recorder).expect("draw");
    assert!(recorder.order.is_empty());
    assert_eq!(bars.drawn_frames(), 0);
    bars.dispose();
}

// New: `dispose` clears the interaction and unhooks, does not release the capture, and is idempotent.
#[test]
fn dispose_unhooks_without_releasing_capture() {
    let rig = ScrollBarRig::new();
    rig.down(995.0, 580.0);
    assert_eq!(rig.clock.subscriber_count(), 1);
    rig.bars().dispose();
    assert!(!rig.bars().is_paging());
    assert_eq!(rig.clock.subscriber_count(), 0);
    assert!(rig.window.has_pointer_capture());
    assert_eq!(rig.window.inner.releases.get(), 0);
    rig.bars().dispose(); // idempotent
    rig.down(995.0, 50.0);
    rig.bars().dispose();
    assert!(!rig.bars().is_dragging());
}

// New: dropping the last reference is a dispose.
#[test]
fn drop_is_dispose() {
    let mut rig = ScrollBarRig::new();
    rig.down(995.0, 580.0);
    assert_eq!(rig.clock.subscriber_count(), 1);
    rig.window.clear_handlers();
    let bars = rig.bars.take().expect("bars");
    assert_eq!(Rc::strong_count(&bars), 1);
    drop(bars);
    assert_eq!(rig.clock.subscriber_count(), 0);
}

// New (T2, open question 5): a window whose `capture_pointer` re-dispatches `WM_CAPTURECHANGED` does not panic (no borrow is held across the
// call). With the C# order (flags first, then capture) the nested message ends the drag at once: that is the observed behaviour, and the
// real OS sends the message only to the window that LOSES the capture.
#[test]
fn a_reentrant_capture_changed_during_capture_pointer_is_survived() {
    let rig = ScrollBarRig::new();
    let window = Rc::downgrade(&rig.window);
    *rig.window.on_capture.borrow_mut() = Some(Rc::new(move || {
        if let Some(window) = window.upgrade() {
            window.dispatch(WM_CAPTURECHANGED, 0, 0);
        }
    }));
    assert_eq!(rig.down(995.0, 50.0), Some(0));
    assert!(!rig.bars().is_dragging());
    assert_eq!(rig.presses.get(), 1);
    *rig.window.on_capture.borrow_mut() = None;
}

// The handler can also be used directly (not through the chain).
#[test]
fn try_handle_ignores_unknown_messages() {
    let rig = ScrollBarRig::new();
    let message = WindowMessage {
        hwnd: 1,
        msg: 0x0111,
        wparam: 0,
        lparam: ScrollBarRig::pack(995, 50),
    };
    assert_eq!(rig.bars().try_handle(&message), None);
    assert!(!rig.bars().is_dragging());
}
