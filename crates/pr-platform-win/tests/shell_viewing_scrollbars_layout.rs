//! Tests of `pr_platform_win::shell_viewing::scrollbars::{state, layout}` (W8-16f): the declaration, the options and colours, and the read-only geometry
//! queries of `PhotoReview.Shell.Win32/Viewing/ScrollBars.cs` (baseline `d92d6aae`). Ported from the part of
//! `tests/PhotoReview.Shell.Tests/Viewing/ShellScrollBarsTests.cs` that needs no message handling (8 tests), plus the new tests of `contracts/W8-16f.md`.
//! Test kind F: fake window, real `ViewportController` over a fake render poster and frame clock, real `ViewerState`. No sleeps, no native calls.
//! The tests that need the mouse (drag, paging, press swallowing, hover colours, painting, DPI 150) are W8-16g's.

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;
#[path = "common/viewing_scrollbars_fixture.rs"]
mod viewing_scrollbars_fixture;

use std::rc::Rc;

use pr_app::input::primitives::{PointD, RectD};
use pr_app::overlay::palette;
use pr_app::viewing::scrollbar_geometry::{ScrollAxis, ScrollBarLayout, ScrollBarMetrics, ScrollBarPart};
use pr_app::viewport::render_geometry::ColorF;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::frame_clock::base::qpc_frequency;
use pr_platform_win::shell_viewing::scrollbars::{ScrollBarColors, ScrollBarOptions, ShellScrollBars};
use pr_platform_win::shell_viewing::viewport_controller::ViewportController;
use viewing_fakes::{FakeHostClock, FakeRenderPoster, FakeWindow};
use viewing_scrollbars_fixture::{RigConfig, ScrollBarRig, ScrollWindow};

fn pt(x: f64, y: f64) -> PointD {
    PointD { x, y }
}

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

// C#: ShellScrollBarsTests.RigIsAsDocumented_BothBarsOverAnActualSizeImage
#[test]
fn rig_is_as_documented_both_bars_over_an_actual_size_image() {
    let rig = ScrollBarRig::new();
    let layout = rig.layout();
    assert_eq!(layout.vertical.bar, rect(990.0, 0.0, 10.0, 590.0));
    close(layout.vertical.thumb.height, 172.87, 1e-6);
    close(layout.horizontal.thumb.width, 986.0 * 990.0 / 3000.0, 1e-6);
    close(layout.vertical.max_offset, 1410.0, 1e-9);
    close(layout.horizontal.max_offset, 2010.0, 1e-9);
    assert!(layout.has_corner);
    assert_eq!(layout.horizontal.bar, rect(0.0, 590.0, 990.0, 10.0));
}

// C#: ShellScrollBarsTests.WhenTheImageFits_NothingIsDrawnOrHandled (the layout half; the draw and press halves are W8-16g)
#[test]
fn when_the_image_fits_no_bar_is_visible_and_nothing_is_hit() {
    let rig = fit_rig();
    let layout = rig.layout();
    assert!(!layout.any_visible());
    assert_eq!(layout, ScrollBarLayout::none());
    assert_eq!(rig.bars().hit_test(pt(995.0, 300.0)), ScrollBarPart::None);
}

// C#: ShellScrollBarsTests.TheBarsAppearAndDisappearWithTheImageSize_WhenZooming
#[test]
fn the_bars_appear_and_disappear_with_the_image_size_when_zooming() {
    let rig = ScrollBarRig::new();
    assert!(rig.layout().any_visible());
    rig.set_zoom(0.2); // 600x400: fits
    assert!(!rig.layout().any_visible());
    assert_eq!(rig.viewport.viewport_width(), 1000.0); // no bar takes space any more

    rig.set_zoom(1.0);
    assert!(rig.layout().any_visible());
    assert_eq!(rig.viewport.viewport_width(), 990.0);
}

// C#: ShellScrollBarsTests.ZoomingIn_ShrinksTheThumb_InProportionToTheViewportOverTheExtent
#[test]
fn zooming_in_shrinks_the_thumb_in_proportion_to_the_viewport_over_the_extent() {
    let rig = ScrollBarRig::new();
    let before = rig.layout().vertical.thumb.height;
    rig.set_zoom(2.0); // 6000x4000
    let after = rig.layout().vertical;
    close(after.thumb.height, 586.0 * 590.0 / 4000.0, 1e-6);
    assert!(after.thumb.height < before);
}

// C#: ShellScrollBarsTests.ComparePanel_CoversTheBars_ExceptTheStripsOutsideItsMargin (the hit-test half; the clip strips are W8-16g)
#[test]
fn compare_panel_covers_the_bars_except_the_strips_outside_its_margin() {
    let rig = ScrollBarRig::new();
    rig.compare_visible.set(true);
    let bars = rig.bars();

    assert_eq!(bars.hit_test(pt(991.0, 300.0)), ScrollBarPart::None); // under the panel (it reaches x = 992)
    assert_eq!(bars.hit_test(pt(994.0, 300.0)), ScrollBarPart::TrackAfter);
    assert_eq!(bars.hit_test(pt(500.0, 591.0)), ScrollBarPart::None); // the panel reaches y = 592
    assert_eq!(bars.hit_test(pt(500.0, 595.0)), ScrollBarPart::TrackAfter);
    // The half-open edge: x = 992 is OUTSIDE the panel (a closed `RectD::contains` would hide it), x = 991.999 is inside.
    assert_eq!(bars.hit_test(pt(992.0, 300.0)), ScrollBarPart::TrackAfter);
    assert_eq!(bars.hit_test(pt(991.999, 300.0)), ScrollBarPart::None);
    assert_eq!(bars.hit_test(pt(500.0, 592.0)), ScrollBarPart::TrackAfter);
    assert_eq!(bars.hit_test(pt(500.0, 591.999)), ScrollBarPart::None);
}

// C#: ShellScrollBarsTests.Resize_ReflowsTheBars_AndRepositionsTheThumb
#[test]
fn resize_reflows_the_bars_and_repositions_the_thumb() {
    let rig = ScrollBarRig::new();
    rig.resize(1200.0, 700.0);

    let layout = rig.layout();
    assert_eq!(layout.vertical.bar, rect(1190.0, 0.0, 10.0, 690.0));
    close(layout.vertical.thumb.height, 686.0 * 690.0 / 2000.0, 1e-6);
    assert_eq!(layout.horizontal.bar, rect(0.0, 690.0, 1190.0, 10.0));
    assert_eq!(rig.viewport.viewport_width(), 1190.0);
}

// C#: ShellScrollBarsTests.Colors_AreTheThumbColorsOfDarkScrollBarsXaml
#[test]
fn colors_are_the_thumb_colors_of_dark_scroll_bars_xaml() {
    let grey = |channel: u8| ColorF {
        r: f32::from(channel) / 255.0,
        g: f32::from(channel) / 255.0,
        b: f32::from(channel) / 255.0,
        a: 1.0,
    };
    assert_eq!(ScrollBarColors::THUMB, grey(0x4A));
    assert_eq!(ScrollBarColors::THUMB_HOVER, grey(0x6A));
    assert_eq!(ScrollBarColors::THUMB_PRESSED, grey(0x7A));
    assert_eq!(ScrollBarColors::TRACK, grey(0x16)); // Dark.ScrollTrack
}

// C#: ShellScrollBarsTests.HoverFollowsTheThumbWhenTheLayoutMovesItUnderAStationaryCursor (through `set_pointer_for_tests`)
#[test]
fn hover_follows_the_thumb_when_the_layout_moves_it_under_a_stationary_cursor() {
    let rig = ScrollBarRig::new();
    let bars = rig.bars();
    bars.set_pointer_for_tests(pt(995.0, 300.0), true);
    assert_eq!(bars.hovered_thumb_axis(), None); // on the track, the thumb is at y 2..174.87

    rig.viewport.scroll_to(0.0, 705.0); // the thumb slides down around y = 300
    rig.viewport.update_layout();
    assert_eq!(bars.hovered_thumb_axis(), Some(ScrollAxis::Vertical));

    rig.viewport.scroll_to(0.0, 0.0); // and back
    rig.viewport.update_layout();
    assert_eq!(bars.hovered_thumb_axis(), None);
}

#[test]
fn track_colour_equals_the_palette_entry() {
    assert_eq!(ScrollBarColors::TRACK, palette::get("Dark.ScrollTrack").unwrap());
    assert_ne!(ScrollBarColors::THUMB, ScrollBarColors::THUMB_HOVER);
    assert_ne!(ScrollBarColors::THUMB_HOVER, ScrollBarColors::THUMB_PRESSED);
}

#[test]
fn hit_test_classifies_every_part_of_the_bars() {
    let rig = ScrollBarRig::new();
    let bars = rig.bars();
    assert_eq!(bars.hit_test(pt(995.0, 50.0)), ScrollBarPart::Thumb);
    assert_eq!(bars.hit_test(pt(100.0, 595.0)), ScrollBarPart::Thumb);
    assert_eq!(bars.hit_test(pt(995.0, 300.0)), ScrollBarPart::TrackAfter);
    assert_eq!(bars.hit_test(pt(500.0, 595.0)), ScrollBarPart::TrackAfter);
    assert_eq!(bars.hit_test(pt(990.5, 300.0)), ScrollBarPart::Background); // inside the bar, outside the track margin
    assert_eq!(bars.hit_test(pt(995.0, 595.0)), ScrollBarPart::Corner);
    // Scrolled to the end, the track before the thumb is the page-up part.
    rig.viewport.scroll_to(1000.0, 1410.0);
    rig.viewport.update_layout();
    assert_eq!(bars.hit_test(pt(995.0, 50.0)), ScrollBarPart::TrackBefore);
    assert_eq!(bars.hit_test(pt(50.0, 595.0)), ScrollBarPart::TrackBefore);
}

#[test]
fn hit_test_outside_every_bar_is_none() {
    let rig = ScrollBarRig::new();
    let bars = rig.bars();
    assert_eq!(bars.hit_test(pt(500.0, 300.0)), ScrollBarPart::None);
    assert_eq!(bars.hit_test(pt(-5.0, -5.0)), ScrollBarPart::None);
    assert_eq!(bars.hit_test(pt(1000.0, 300.0)), ScrollBarPart::None); // the right edge is half-open
    assert_eq!(bars.hit_test(pt(989.9, 300.0)), ScrollBarPart::None);
    assert_eq!(bars.hit_test(pt(500.0, 589.9)), ScrollBarPart::None);
}

#[test]
fn hit_test_with_compare_hides_only_inside_the_panel() {
    let rig = ScrollBarRig::new();
    let bars = rig.bars();
    let probes = [
        pt(995.0, 50.0),
        pt(990.5, 300.0),
        pt(991.0, 300.0),
        pt(995.0, 595.0),
        pt(100.0, 595.0),
        pt(500.0, 300.0),
    ];
    let without: Vec<ScrollBarPart> = probes.iter().map(|p| bars.hit_test(*p)).collect();
    assert_eq!(
        without,
        [
            ScrollBarPart::Thumb,
            ScrollBarPart::Background,
            ScrollBarPart::TrackAfter,
            ScrollBarPart::Corner,
            ScrollBarPart::Thumb,
            ScrollBarPart::None
        ]
    );
    rig.compare_visible.set(true);
    let with: Vec<ScrollBarPart> = probes.iter().map(|p| bars.hit_test(*p)).collect();
    assert_eq!(
        with,
        [
            ScrollBarPart::Thumb, // x = 995 is outside the panel (it ends at 992)
            ScrollBarPart::None,
            ScrollBarPart::None,
            ScrollBarPart::Corner, // outside the panel on both axes
            ScrollBarPart::Thumb,  // y = 595 is below the panel (it ends at 592)
            ScrollBarPart::None
        ]
    );
    // The flag is read on every query.
    rig.compare_visible.set(false);
    assert_eq!(bars.hit_test(pt(991.0, 300.0)), ScrollBarPart::TrackAfter);
}

#[test]
fn hovered_thumb_axis_is_none_when_the_pointer_is_outside_the_window() {
    let rig = ScrollBarRig::new();
    let bars = rig.bars();
    assert_eq!(bars.hovered_thumb_axis(), None); // fresh: the pointer was never inside
    bars.set_pointer_for_tests(pt(995.0, 50.0), false);
    assert_eq!(bars.hovered_thumb_axis(), None);
    bars.set_pointer_for_tests(pt(995.0, 50.0), true);
    assert_eq!(bars.hovered_thumb_axis(), Some(ScrollAxis::Vertical));
    bars.set_pointer_for_tests(pt(100.0, 595.0), true);
    assert_eq!(bars.hovered_thumb_axis(), Some(ScrollAxis::Horizontal));
    bars.set_pointer_for_tests(pt(995.0, 300.0), true); // the track, not the thumb
    assert_eq!(bars.hovered_thumb_axis(), None);
    bars.set_pointer_for_tests(pt(500.0, 300.0), true); // the image
    assert_eq!(bars.hovered_thumb_axis(), None);
    bars.set_pointer_for_tests(pt(995.0, 595.0), true); // the corner
    assert_eq!(bars.hovered_thumb_axis(), None);
}

#[test]
fn hovered_thumb_axis_respects_the_compare_panel() {
    let rig = ScrollBarRig::new();
    let bars = rig.bars();
    bars.set_pointer_for_tests(pt(991.5, 50.0), true); // on the thumb, under the panel when compare shows
    assert_eq!(bars.hovered_thumb_axis(), Some(ScrollAxis::Vertical));
    rig.compare_visible.set(true);
    assert_eq!(bars.hovered_thumb_axis(), None);
    bars.set_pointer_for_tests(pt(995.0, 50.0), true); // outside the panel
    assert_eq!(bars.hovered_thumb_axis(), Some(ScrollAxis::Vertical));
}

#[test]
fn current_layout_is_recomputed_on_every_call() {
    let rig = ScrollBarRig::new();
    let first = rig.layout();
    rig.viewport.scroll_to(0.0, 705.0);
    rig.viewport.update_layout();
    let second = rig.layout();
    assert_ne!(first.vertical.thumb, second.vertical.thumb);
    assert_eq!(first.vertical.bar, second.vertical.bar);
    close(second.vertical.offset, 705.0, 1e-9);
}

#[test]
fn current_layout_is_none_after_the_window_is_dropped() {
    // The viewport is built over ANOTHER window, so dropping the bars' window frees it (the controller holds its own window strongly).
    let rig = ScrollBarRig::with(RigConfig {
        with_scroll_bars: false,
        ..RigConfig::default()
    });
    let window = ScrollWindow::new(FakeWindow::new());
    let api: Rc<dyn ShellWindowApi> = window.clone();
    let bars = ShellScrollBars::new(&api, Rc::clone(&rig.viewport), Rc::new(|| false), None, None);
    drop(api);
    assert_eq!(
        Rc::strong_count(&window),
        1,
        "the bars hold the window by Weak (D-W8B4V-7)"
    );
    assert_eq!(Rc::strong_count(&bars), 2); // the test + the window's handler list
    assert!(bars.current_layout().any_visible());
    assert_eq!(window.handler_count(), 1);

    drop(window);

    assert_eq!(
        Rc::strong_count(&bars),
        1,
        "the bars are freed with the window's handler list"
    );
    assert_eq!(bars.current_layout(), ScrollBarLayout::none());
    assert_eq!(bars.hit_test(pt(995.0, 50.0)), ScrollBarPart::None);
    bars.set_pointer_for_tests(pt(995.0, 50.0), true);
    assert_eq!(bars.hovered_thumb_axis(), None);
}

#[test]
fn options_default_matches_the_xaml_values() {
    let options = ScrollBarOptions::default();
    assert_eq!(options.metrics, ScrollBarMetrics::default());
    assert_eq!(options.metrics.thickness, 10.0);
    assert_eq!(options.metrics.track_margin_along, 2.0);
    assert_eq!(options.metrics.track_margin_across, 1.0);
    assert_eq!(options.metrics.min_thumb_length, 8.0);
    assert_eq!(options.metrics.thumb_radius, 4.0);
    assert_eq!(options.ticks_per_second, qpc_frequency());
    assert!(options.ticks_per_second > 0);
    // The single thickness source (D-W8B4X-7): the controller's default equals the bars' default.
    let rig = ScrollBarRig::new();
    assert_eq!(rig.viewport.scroll_bar_thickness(), options.metrics.thickness);
    // Debug does not need the closures.
    assert!(format!("{options:?}").contains("ticks_per_second"));
    let cloned = options.clone();
    assert_eq!(cloned.ticks_per_second, options.ticks_per_second);
}

#[test]
fn new_registers_exactly_one_message_handler_and_starts_idle() {
    let rig = ScrollBarRig::with(RigConfig {
        with_scroll_bars: false,
        ..RigConfig::default()
    });
    assert_eq!(rig.window.handler_count(), 0);
    let api: Rc<dyn ShellWindowApi> = rig.window.clone();
    let bars = ShellScrollBars::new(&api, Rc::clone(&rig.viewport), Rc::new(|| false), None, None);
    assert_eq!(rig.window.handler_count(), 1);
    assert!(!bars.is_dragging());
    assert!(!bars.is_paging());
    assert_eq!(bars.drawn_frames(), 0);
    let debug = format!("{bars:?}");
    assert!(debug.contains("ShellScrollBars") && debug.contains("dragging") && debug.contains("drawn_frames"));
    assert_eq!(bars.hovered_thumb_axis(), None);
}

#[test]
fn the_default_options_are_used_when_none_are_given_and_the_metrics_drive_the_layout() {
    // `Some(options)` with a thicker bar changes the geometry: the metrics come from the options, not from a constant.
    let rig = ScrollBarRig::with(RigConfig {
        with_scroll_bars: false,
        ..RigConfig::default()
    });
    let api: Rc<dyn ShellWindowApi> = rig.window.clone();
    let wide = ShellScrollBars::new(
        &api,
        Rc::clone(&rig.viewport),
        Rc::new(|| false),
        None,
        Some(ScrollBarOptions {
            metrics: ScrollBarMetrics {
                thickness: 20.0,
                ..ScrollBarMetrics::default()
            },
            ..ScrollBarOptions::default()
        }),
    );
    let default = ShellScrollBars::new(&api, Rc::clone(&rig.viewport), Rc::new(|| false), None, None);
    assert_eq!(wide.current_layout().vertical.bar.width, 20.0);
    assert_eq!(default.current_layout().vertical.bar.width, 10.0);
}

#[test]
fn the_clock_and_poster_fakes_are_not_needed_by_the_queries() {
    // The queries read only the controller's last layout: a rig whose queued layout pass never ran has no bars yet.
    let window = ScrollWindow::new(FakeWindow::new());
    let poster = FakeRenderPoster::new();
    let api: Rc<dyn ShellWindowApi> = window.clone();
    let viewport = ViewportController::new(
        Rc::clone(&api),
        poster.clone(),
        FakeHostClock::new(),
        Rc::new(pr_platform_win::shell_viewing::viewport_controller::ViewportContent::empty),
        Rc::new(|| {}),
        pr_platform_win::shell_viewing::viewport_controller::ViewportControllerOptions::default(),
    );
    let bars = ShellScrollBars::new(&api, viewport, Rc::new(|| false), None, None);
    assert!(!bars.current_layout().any_visible());
    assert_eq!(bars.hit_test(pt(995.0, 300.0)), ScrollBarPart::None);
}
