//! Tests of `pr_platform_win::shell_viewing::viewport_controller` (W8-16h): the port of `PhotoReview.Shell.Win32/Viewing/ViewportController.cs`
//! (baseline `d92d6aae`). Ported from `tests/PhotoReview.Shell.Tests/Viewport/ViewportControllerTests.cs` (23 of the 24 tests; `Constructor_RejectsNulls`
//! is unrepresentable in Rust), plus the new tests listed in `contracts/W8-16h.md`. Test kind F: fake window, fake render poster, fake frame clock,
//! the real `ViewportState` and layout engine. No sleeps, no native calls.

#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use pr_app::input::primitives::PointD;
use pr_app::input::surface::RenderHookId;
use pr_app::viewport::layout;
use pr_app::viewport::state::LayoutEngine;
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput};
use pr_platform_win::host::contracts::{PointD as HostPointD, ShellCursor, ShellWindowEvent, SizeD};
use pr_platform_win::shell_viewing::viewport_controller::{
    MAX_LAYOUT_ITERATIONS, ViewportContent, ViewportController, ViewportControllerOptions, ViewportLayoutEngine,
    normalized_dpi,
};
use viewing_fakes::{FakeDisplayClock, FakeHostClock, FakeRenderPoster, FakeWindow};

/// C# fixture: `_content` (None, 3000x2000, unbounded, bitmap 750x500, not Fit) and the `_fitUpdates` counter.
struct Rig {
    window: Rc<FakeWindow>,
    poster: Rc<FakeRenderPoster>,
    clock: Rc<FakeHostClock>,
    content: Rc<Cell<ViewportContent>>,
    fit_updates: Rc<Cell<u32>>,
}

fn default_content() -> ViewportContent {
    ViewportContent {
        stretch: ViewerStretchMode::None,
        image_width: 3000.0,
        image_height: 2000.0,
        max_image_width: f64::INFINITY,
        max_image_height: f64::INFINITY,
        bitmap_width: 750.0,
        bitmap_height: 500.0,
        is_fit: false,
    }
}

impl Rig {
    fn new() -> Rig {
        Rig {
            window: FakeWindow::new(),
            poster: FakeRenderPoster::new(),
            clock: FakeHostClock::new(),
            content: Rc::new(Cell::new(default_content())),
            fit_updates: Rc::new(Cell::new(0)),
        }
    }

    fn create(&self) -> Rc<ViewportController> {
        self.create_with(ViewportControllerOptions::default())
    }

    fn create_with(&self, options: ViewportControllerOptions) -> Rc<ViewportController> {
        let fit_updates = Rc::clone(&self.fit_updates);
        self.create_full(options, Rc::new(move || fit_updates.set(fit_updates.get() + 1)))
    }

    fn create_full(&self, options: ViewportControllerOptions, update_fit_size: Rc<dyn Fn()>) -> Rc<ViewportController> {
        let content = Rc::clone(&self.content);
        ViewportController::new(
            self.window.clone(),
            self.poster.clone(),
            self.clock.clone(),
            Rc::new(move || content.get()),
            update_fit_size,
            options,
        )
    }

    fn set_content(&self, change: impl FnOnce(&mut ViewportContent)) {
        let mut content = self.content.get();
        change(&mut content);
        self.content.set(content);
    }
}

// C#: ViewportControllerTests.Construction_PostsOneLayoutAtRenderPriority_ThatReadsTheWindowAndTheContent
#[test]
fn construction_posts_one_layout_at_render_priority_that_reads_the_window_and_the_content() {
    let rig = Rig::new();
    let controller = rig.create();
    assert_eq!(rig.poster.posts(), 1);
    assert!(controller.is_layout_posted());
    assert_eq!(controller.extent_width(), 0.0);

    rig.poster.drain();

    assert!(!controller.is_layout_posted());
    assert_eq!(controller.extent_width(), 3000.0);
    assert_eq!(controller.viewport_width(), 990.0); // 1000 client - vertical bar 10
    assert_eq!(controller.viewport_height(), 590.0);
    assert_eq!(rig.window.invalidations.get(), 1);
}

// C#: ViewportControllerTests.InvalidateLayout_IsCoalescedUntilThePostedLayoutRuns
#[test]
fn invalidate_layout_is_coalesced_until_the_posted_layout_runs() {
    let rig = Rig::new();
    let controller = rig.create();
    controller.invalidate_layout();
    controller.scroll_to(10.0, 10.0);
    controller.invalidate_layout();
    assert_eq!(rig.poster.posts(), 1);
    rig.poster.drain();
    controller.invalidate_layout();
    assert_eq!(rig.poster.posts(), 2);
}

// C#: ViewportControllerTests.ScrollTo_IsAppliedByTheNextLayout_Clamped_AndRedraws
#[test]
fn scroll_to_is_applied_by_the_next_layout_clamped_and_redraws() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.poster.drain();
    let updates = Rc::new(Cell::new(0));
    let counter = Rc::clone(&updates);
    controller.subscribe_layout_updated(Rc::new(move || counter.set(counter.get() + 1)));

    controller.scroll_to(5000.0, 100.0);
    assert_eq!(controller.horizontal_offset(), 0.0);
    rig.poster.drain();

    assert_eq!(controller.horizontal_offset(), 2010.0); // 3000 - 990
    assert_eq!(controller.vertical_offset(), 100.0);
    assert_eq!(updates.get(), 1);
    assert_eq!(rig.window.invalidations.get(), 2);
}

// C#: ViewportControllerTests.ALayoutThatChangesNothing_DoesNotRedraw
#[test]
fn a_layout_that_changes_nothing_does_not_redraw() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.poster.drain();
    controller.scroll_to(0.0, 0.0);
    rig.poster.drain();
    assert_eq!(rig.window.invalidations.get(), 1);
}

// C#: ViewportControllerTests.YieldToRenderAsync_CompletesAfterTheLayoutPostedBeforeIt
#[test]
fn yield_to_render_completes_after_the_layout_posted_before_it() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.poster.drain();
    controller.scroll_to(300.0, 200.0);
    let seen: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let probe = Rc::downgrade(&controller);
    let slot = Rc::clone(&seen);
    controller.yield_to_render(Box::new(move || {
        slot.set(probe.upgrade().map(|c| c.horizontal_offset()));
    }));
    assert_eq!(seen.get(), None);

    rig.poster.drain();

    assert_eq!(seen.get(), Some(300.0));
}

// new: the continuation is never run inline, even with nothing queued, and runs behind earlier jobs in FIFO order
#[test]
fn yield_to_render_runs_after_the_layout_job_and_never_inline() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.poster.drain();
    assert_eq!(rig.poster.pending(), 0);
    let order = Rc::new(RefCell::new(Vec::<&'static str>::new()));
    let log = Rc::clone(&order);
    controller.yield_to_render(Box::new(move || log.borrow_mut().push("first")));
    assert!(order.borrow().is_empty());
    assert_eq!(rig.poster.pending(), 1);

    controller.scroll_to(1.0, 1.0); // layout job queued behind "first"
    let log = Rc::clone(&order);
    controller.yield_to_render(Box::new(move || log.borrow_mut().push("second")));
    assert_eq!(rig.poster.pending(), 3);
    rig.poster.drain();

    assert_eq!(*order.borrow(), vec!["first", "second"]);
    assert_eq!(controller.horizontal_offset(), 1.0);
}

// C#: ViewportControllerTests.UpdateLayout_IsSynchronous_AndRereadsTheContent
#[test]
fn update_layout_is_synchronous_and_rereads_the_content() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.set_content(|c| {
        c.image_width = 500.0;
        c.image_height = 400.0;
    });
    controller.update_layout();
    assert_eq!(controller.image_actual_width(), 500.0);
    let origin = controller.image_origin();
    assert_eq!((origin.x, origin.y), (250.0, 100.0)); // centred in 1000x600
    let element = controller.to_image_element(PointD { x: 300.0, y: 160.0 });
    assert_eq!((element.x, element.y), (50.0, 60.0));
}

// C#: ViewportControllerTests.ClientResize_UpdatesTheFitSize_ThenLaysOutAtTheNewSize
#[test]
fn client_resize_updates_the_fit_size_then_lays_out_at_the_new_size() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.poster.drain();
    rig.window.resize(1600.0, 900.0);
    assert_eq!(rig.fit_updates.get(), 1);
    rig.poster.drain();
    assert_eq!(controller.viewport_width(), 1590.0);
    assert_eq!(controller.viewport_size(), (1600.0, 900.0));
}

// C#: ViewportControllerTests.FitImageChangingSize_RunsUpdateFitSize_LikeMainImageSizeChanged_ButNotWhenZoomed
#[test]
fn fit_image_changing_size_runs_update_fit_size_like_main_image_size_changed_but_not_when_zoomed() {
    let rig = Rig::new();
    rig.content.set(ViewportContent {
        stretch: ViewerStretchMode::Uniform,
        image_width: f64::NAN,
        image_height: f64::NAN,
        max_image_width: 1000.0,
        max_image_height: 600.0,
        bitmap_width: 1500.0,
        bitmap_height: 1000.0,
        is_fit: true,
    });
    let controller = rig.create();
    rig.poster.drain();
    assert_eq!(rig.fit_updates.get(), 1); // 0x0 -> 900x600

    rig.set_content(|c| {
        c.bitmap_width = 1000.0;
        c.bitmap_height = 1000.0;
    });
    controller.update_layout();
    assert_eq!(rig.fit_updates.get(), 2);

    rig.content.set(ViewportContent {
        stretch: ViewerStretchMode::None,
        image_width: 2000.0,
        image_height: 2000.0,
        max_image_width: f64::INFINITY,
        max_image_height: f64::INFINITY,
        bitmap_width: 1000.0,
        bitmap_height: 1000.0,
        is_fit: false,
    });
    controller.update_layout();
    assert_eq!(rig.fit_updates.get(), 2);
}

// C#: ViewportControllerTests.FitSizeHook_FeedsBackIntoTheSameLayout_UntilStable
#[test]
fn fit_size_hook_feeds_back_into_the_same_layout_until_stable() {
    // update_fit_size changes the max image size (like ViewerState.UpdateViewport) -> the pass runs again inside the same update_layout.
    let rig = Rig::new();
    rig.content.set(ViewportContent {
        stretch: ViewerStretchMode::Uniform,
        image_width: f64::NAN,
        image_height: f64::NAN,
        max_image_width: f64::INFINITY,
        max_image_height: f64::INFINITY,
        bitmap_width: 1500.0,
        bitmap_height: 1000.0,
        is_fit: true,
    });
    let content = Rc::clone(&rig.content);
    let controller = rig.create_full(
        ViewportControllerOptions::default(),
        Rc::new(move || {
            let mut c = content.get();
            c.max_image_width = 1000.0;
            c.max_image_height = 600.0;
            content.set(c);
        }),
    );
    controller.update_layout();
    assert!((controller.image_actual_width() - 900.0).abs() < 1e-9);
    assert!(!controller.layout().vertical_bar_visible);
}

// C#: ViewportControllerTests.DragThreshold_IsTheSystemMetricInDips (Theory, 8 rows)
#[test]
fn drag_threshold_is_the_system_metric_in_dips() {
    let rows: [(f64, f64); 8] = [
        (1.0, 4.0),
        (1.5, 4.0 / 1.5),
        (2.0, 2.0),
        (0.0, 4.0),
        (f64::NAN, 4.0),
        (17.0, 4.0),
        (0.25, 16.0),
        (16.0, 0.25),
    ];
    for (dpi, expected) in rows {
        let rig = Rig::new();
        rig.window.dpi_scale.set(dpi);
        let controller = rig.create();
        let (h, v) = controller.drag_threshold();
        assert!((h - expected).abs() < 1e-9, "dpi {dpi}: horizontal {h}");
        assert!((v - expected).abs() < 1e-9, "dpi {dpi}: vertical {v}");
    }
}

// C#: ViewportControllerTests.DragThreshold_UsesBothAxesOfTheMetric
#[test]
fn drag_threshold_uses_both_axes_of_the_metric() {
    let rig = Rig::new();
    rig.window.dpi_scale.set(1.25);
    let controller = rig.create_with(ViewportControllerOptions {
        drag_metrics_pixels: Box::new(|| (5, 10)),
        ..ViewportControllerOptions::default()
    });
    assert_eq!(controller.drag_threshold(), (4.0, 8.0));
}

// C#: ViewportControllerTests.PointerPosition_IsTheCursorInViewportDips_OnlyInsideTheViewport (Theory, 7 rows)
#[test]
fn pointer_position_is_the_cursor_in_viewport_dips_only_inside_the_viewport() {
    let rows: [(f64, f64, bool); 7] = [
        (10.0, 10.0, true),
        (0.0, 0.0, true),
        (990.0, 590.0, true), // the viewport edge (bars subtracted) counts as inside
        (990.5, 10.0, false),
        (10.0, 590.5, false),
        (-0.5, 10.0, false),
        (10.0, -0.5, false),
    ];
    for (x, y, inside) in rows {
        let rig = Rig::new();
        rig.window.dpi_scale.set(2.0);
        rig.window.client_origin_pixel.set(HostPointD { x: 100.0, y: 40.0 });
        let controller = rig.create_with(ViewportControllerOptions {
            cursor_screen_pixel: Box::new(move || {
                Some(PointD {
                    x: x * 2.0 + 100.0,
                    y: y * 2.0 + 40.0,
                })
            }),
            ..ViewportControllerOptions::default()
        });
        rig.poster.drain();
        let position = controller.pointer_position().map(|p| (p.x, p.y));
        let expected = if inside { Some((x, y)) } else { None };
        assert_eq!(position, expected, "cursor ({x}, {y})");
    }
}

// C#: ViewportControllerTests.PointerPosition_UnknownCursor_IsNull
#[test]
fn pointer_position_unknown_cursor_is_none() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.poster.drain();
    assert!(controller.pointer_position().is_none());
}

// C#: ViewportControllerTests.MouseCaptureAndCursor_GoToTheWindow_ReleaseOnlyWhenHeld
#[test]
fn mouse_capture_and_cursor_go_to_the_window_release_only_when_held() {
    let rig = Rig::new();
    let controller = rig.create();
    controller.release_mouse_capture();
    assert_eq!(rig.window.releases.get(), 0);
    controller.capture_mouse();
    assert!(rig.window.has_capture.get());
    controller.release_mouse_capture();
    assert!(!rig.window.has_capture.get());
    assert_eq!(rig.window.releases.get(), 1);

    controller.set_pan_cursor(true);
    assert_eq!(rig.window.cursor.get(), Some(ShellCursor::SizeAll));
    controller.set_pan_cursor(false);
    assert_eq!(rig.window.cursor.get(), Some(ShellCursor::Arrow));
}

// C#: ViewportControllerTests.RenderFrames_ReachHookedHandlers_WithTheirRenderingTime_UntilUnhooked
#[test]
fn render_frames_reach_hooked_handlers_with_their_rendering_time_until_unhooked() {
    let rig = Rig::new();
    let controller = rig.create();
    let times = Rc::new(RefCell::new(Vec::<Option<i64>>::new()));
    let sink = Rc::clone(&times);

    let id = controller.hook_render_frame(Box::new(move |ticks| sink.borrow_mut().push(ticks)));
    assert_eq!(rig.clock.frame_requests(), 1);
    rig.clock.tick(16.5);
    controller.unhook_render_frame(id);
    rig.clock.tick(33.0);

    assert_eq!(*times.borrow(), vec![Some(165_000)]);
    assert_eq!(rig.clock.subscriber_count(), 0);
}

// new: 16.5 ms is 165_000 TimeSpan ticks (100 ns), not QPC ticks (T4); the cast saturates
#[test]
fn rendering_time_ticks_for_16_5_ms_is_165_000() {
    let rig = Rig::new();
    let controller = rig.create();
    let times = Rc::new(RefCell::new(Vec::<Option<i64>>::new()));
    let sink = Rc::clone(&times);
    controller.hook_render_frame(Box::new(move |ticks| sink.borrow_mut().push(ticks)));
    rig.clock.tick(16.5);
    rig.clock.tick(0.0);
    rig.clock.tick(1.0e300);
    assert_eq!(*times.borrow(), vec![Some(165_000), Some(0), Some(i64::MAX)]);
}

// C#: ViewportControllerTests.HookingTwice_CallsTwice_AndUnhookRemovesOneAtATime_UnknownHandlersAreIgnored
#[test]
fn hooking_twice_calls_twice_and_unhook_removes_one_at_a_time_unknown_ids_are_ignored() {
    let rig = Rig::new();
    let controller = rig.create();
    let calls = Rc::new(Cell::new(0));
    let (first, second) = (Rc::clone(&calls), Rc::clone(&calls));
    let id1 = controller.hook_render_frame(Box::new(move |_| first.set(first.get() + 1)));
    let id2 = controller.hook_render_frame(Box::new(move |_| second.set(second.get() + 1)));
    assert_ne!(id1, id2);
    controller.unhook_render_frame(RenderHookId(9999));
    rig.clock.tick(1.0);
    assert_eq!(calls.get(), 2);
    controller.unhook_render_frame(id1);
    controller.unhook_render_frame(id1); // already removed: ignored
    rig.clock.tick(2.0);
    assert_eq!(calls.get(), 3);
    assert_eq!(rig.clock.subscriber_count(), 1);
}

// new: a hook may unhook itself and hook another one from inside its own delivery (no double borrow)
#[test]
fn hook_closure_may_unhook_itself() {
    let rig = Rig::new();
    let controller = rig.create();
    let own_id: Rc<Cell<Option<RenderHookId>>> = Rc::new(Cell::new(None));
    let calls = Rc::new(Cell::new(0));
    let late_calls = Rc::new(Cell::new(0));
    let (weak, slot, counter, late) = (
        Rc::downgrade(&controller),
        Rc::clone(&own_id),
        Rc::clone(&calls),
        Rc::clone(&late_calls),
    );
    let id = controller.hook_render_frame(Box::new(move |_| {
        counter.set(counter.get() + 1);
        if let (Some(controller), Some(id)) = (weak.upgrade(), slot.get()) {
            controller.unhook_render_frame(id);
            let late = Rc::clone(&late);
            controller.hook_render_frame(Box::new(move |_| late.set(late.get() + 1)));
        }
    }));
    own_id.set(Some(id));

    rig.clock.tick(1.0);
    rig.clock.tick(2.0);

    assert_eq!(calls.get(), 1);
    assert_eq!(late_calls.get(), 1); // hooked during the first tick, delivered from the second
    assert_eq!(rig.clock.subscriber_count(), 1);
}

// C#: ViewportControllerTests.Dispose_UnhooksEverything_StopsLayout_AndReportsNotLoaded
#[test]
fn dispose_unhooks_everything_stops_layout_and_reports_not_loaded() {
    let rig = Rig::new();
    let controller = rig.create();
    controller.hook_render_frame(Box::new(|_| {}));
    assert!(controller.is_loaded());
    controller.dispose();
    controller.dispose();

    assert_eq!(rig.clock.subscriber_count(), 0);
    assert!(!controller.is_loaded());
    rig.poster.drain(); // the job posted before dispose does nothing
    assert_eq!(rig.window.invalidations.get(), 0);
    controller.invalidate_layout();
    assert_eq!(rig.poster.posts(), 1);
    rig.window.resize(500.0, 500.0);
    assert_eq!(rig.fit_updates.get(), 0);
    assert_eq!(rig.window.subscriber_count(ShellWindowEvent::ClientSizeChanged), 0);
}

// new: a layout job posted before dispose runs later and does nothing, and clears the posted flag
#[test]
fn job_posted_before_dispose_does_nothing() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.content.set(default_content());
    controller.dispose();
    assert_eq!(rig.poster.pending(), 1);
    rig.poster.drain();
    assert_eq!(controller.extent_width(), 0.0);
    assert_eq!(rig.window.invalidations.get(), 0);
    assert!(!controller.is_layout_posted());
}

// C#: ViewportControllerTests.IsLoaded_FollowsTheWindow
#[test]
fn is_loaded_follows_the_window() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.window.loaded.set(false);
    assert!(!controller.is_loaded());
    rig.window.loaded.set(true);
    assert!(controller.is_loaded());
}

// C#: ViewportControllerTests.SourceSize_IsTheBitmap_OrNullWithoutOne
#[test]
fn source_size_is_the_bitmap_or_none_without_one() {
    let rig = Rig::new();
    let controller = rig.create();
    let size = controller.source_size().unwrap();
    assert_eq!((size.width, size.height), (750.0, 500.0));
    rig.set_content(|c| c.bitmap_width = 0.0);
    assert!(controller.source_size().is_none());
    rig.set_content(|c| {
        c.bitmap_width = 10.0;
        c.bitmap_height = 0.0;
    });
    assert!(controller.source_size().is_none());
    rig.set_content(|c| {
        c.bitmap_width = 0.0;
        c.bitmap_height = 10.0;
    });
    assert!(controller.source_size().is_none());
}

// C#: ViewportControllerTests.ScrollHome_ReturnsToTheOrigin
#[test]
fn scroll_home_returns_to_the_origin() {
    let rig = Rig::new();
    let controller = rig.create();
    controller.scroll_to(400.0, 300.0);
    rig.poster.drain();
    assert_eq!(
        (controller.horizontal_offset(), controller.vertical_offset()),
        (400.0, 300.0)
    );
    controller.scroll_home();
    assert!(controller.is_layout_posted());
    rig.poster.drain();
    assert_eq!(
        (controller.horizontal_offset(), controller.vertical_offset()),
        (0.0, 0.0)
    );
}

// C#: ViewportControllerTests.ViewportSize_ReadsAnInvalidClientAsZero
#[test]
fn viewport_size_reads_an_invalid_client_as_zero() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.window.client_size.set(SizeD {
        width: -3.0,
        height: f64::NAN,
    });
    assert_eq!(controller.viewport_size(), (0.0, 0.0));
    rig.window.client_size.set(SizeD {
        width: f64::INFINITY,
        height: 0.0,
    });
    assert_eq!(controller.viewport_size(), (0.0, 0.0));
    rig.window.client_size.set(SizeD {
        width: 5.0,
        height: 7.0,
    });
    assert_eq!(controller.viewport_size(), (5.0, 7.0));
}

// C#: ViewportControllerTests.TimestampAndDisplayTiming_ComeFromTheOptions
#[test]
fn timestamp_and_display_timing_come_from_the_options() {
    let rig = Rig::new();
    let display = Arc::new(FakeDisplayClock::default());
    let controller = rig.create_with(ViewportControllerOptions {
        timestamp: Box::new(|| 1234),
        display_clock: Some(display.clone()),
        ..ViewportControllerOptions::default()
    });
    assert_eq!(controller.timestamp(), 1234);
    let timing = controller.display_timing().unwrap();
    assert_eq!((timing.last_vblank, timing.refresh_period), (5, 6));
    assert_eq!(display.last_window(), Some(rig.window.hwnd));
    let no_clock = Rig::new().create();
    assert!(no_clock.display_timing().is_none());
    assert!(no_clock.timestamp() > 0);
}

// C#: ViewportControllerTests.EmptyContent_IsAnUnboundedFitWithoutABitmap
#[test]
fn empty_content_is_an_unbounded_fit_without_a_bitmap() {
    let empty = ViewportContent::empty();
    assert_eq!(empty.stretch, ViewerStretchMode::Uniform);
    assert!(empty.image_width.is_nan() && empty.image_height.is_nan());
    assert_eq!(empty.max_image_width, f64::INFINITY);
    assert_eq!(empty.max_image_height, f64::INFINITY);
    assert_eq!((empty.bitmap_width, empty.bitmap_height), (0.0, 0.0));
    assert!(empty.is_fit);

    let rig = Rig::new();
    rig.content.set(empty);
    let controller = rig.create();
    rig.poster.drain();
    assert_eq!(controller.image_actual_width(), 0.0);
    assert!(!controller.layout().horizontal_bar_visible);
    assert!(controller.source_size().is_none());
}

// new: the unit struct delegates to the layout functions
#[test]
fn engine_matches_viewport_layout_functions() {
    let engine = ViewportLayoutEngine;
    let inputs = [
        ViewportInput {
            client_width: 1000.0,
            client_height: 600.0,
            scroll_bar_thickness: 10.0,
            image_width: 3000.0,
            image_height: 2000.0,
            max_image_width: f64::INFINITY,
            max_image_height: f64::INFINITY,
            bitmap_width: 750.0,
            bitmap_height: 500.0,
            ..ViewportInput::default()
        },
        ViewportInput {
            client_width: 800.0,
            client_height: 400.0,
            scroll_bar_thickness: 10.0,
            scroll_bars: ScrollBarPolicy::Overlay,
            stretch: ViewerStretchMode::Uniform,
            image_width: f64::NAN,
            image_height: f64::NAN,
            max_image_width: 500.0,
            max_image_height: 300.0,
            bitmap_width: 1500.0,
            bitmap_height: 1000.0,
        },
        ViewportInput::default(),
    ];
    for input in inputs {
        let computed = engine.compute(&input);
        assert_eq!(computed, layout::compute(&input));
        for (h, v) in [(0.0, 0.0), (5000.0, 100.0), (-3.0, f64::NAN)] {
            let (eh, ev) = engine.clamp_offset(&computed, h, v);
            let (xh, xv) = layout::clamp_offset(&computed, h, v);
            assert!(eh == xh || (eh.is_nan() && xh.is_nan()));
            assert!(ev == xv || (ev.is_nan() && xv.is_nan()));
        }
    }
}

// new: the closed interval 0.25..=16 (T6)
#[test]
fn normalized_dpi_table() {
    let rows: [(f64, f64); 11] = [
        (0.0, 1.0),
        (f64::NAN, 1.0),
        (-1.0, 1.0),
        (0.25, 0.25),
        (0.2499, 1.0),
        (16.0, 16.0),
        (16.0001, 1.0),
        (1.5, 1.5),
        (f64::INFINITY, 1.0),
        (f64::NEG_INFINITY, 1.0),
        (1.0, 1.0),
    ];
    for (input, expected) in rows {
        assert_eq!(normalized_dpi(input), expected, "normalized_dpi({input})");
    }
}

// new: the options defaults (the WPF values at 96 DPI) and the single scroll bar thickness source (D-W8B4X-7)
#[test]
fn options_defaults_and_scroll_bar_thickness_accessor() {
    let options = ViewportControllerOptions::default();
    assert_eq!(options.scroll_bar_thickness, 10.0);
    assert_eq!(options.scroll_bars, ScrollBarPolicy::Auto);
    assert_eq!((options.drag_metrics_pixels)(), (4, 4));
    assert!((options.cursor_screen_pixel)().is_none());
    assert!(options.display_clock.is_none());
    assert!((options.timestamp)() > 0);
    assert!(format!("{options:?}").contains("scroll_bar_thickness"));

    let rig = Rig::new();
    assert_eq!(rig.create().scroll_bar_thickness(), 10.0);
    assert!(format!("{:?}", rig.create()).contains("ViewportController"));
    let thick = rig.create_with(ViewportControllerOptions {
        scroll_bar_thickness: 14.0,
        ..ViewportControllerOptions::default()
    });
    assert_eq!(thick.scroll_bar_thickness(), 14.0);
    rig.poster.drain();
    assert_eq!(thick.viewport_width(), 986.0); // the layout uses the same thickness
}

// new: the scroll bar policy of the options reaches the layout input
#[test]
fn scroll_bar_policy_reaches_the_layout() {
    let rig = Rig::new();
    let controller = rig.create_with(ViewportControllerOptions {
        scroll_bars: ScrollBarPolicy::Overlay,
        ..ViewportControllerOptions::default()
    });
    rig.poster.drain();
    assert_eq!(controller.viewport_width(), 1000.0); // overlay bars do not subtract
}

// new (T4/INV): a content callback that changes the Fit size on every call stops after MAX_LAYOUT_ITERATIONS passes
#[test]
fn layout_loop_is_capped_at_eight_iterations() {
    assert_eq!(MAX_LAYOUT_ITERATIONS, 8);
    let rig = Rig::new();
    let calls = Rc::new(Cell::new(0u32));
    let counter = Rc::clone(&calls);
    let fit_updates = Rc::clone(&rig.fit_updates);
    let controller = ViewportController::new(
        rig.window.clone(),
        rig.poster.clone(),
        rig.clock.clone(),
        Rc::new(move || {
            counter.set(counter.get() + 1);
            ViewportContent {
                stretch: ViewerStretchMode::None,
                image_width: 100.0 + f64::from(counter.get()),
                image_height: 50.0,
                max_image_width: f64::INFINITY,
                max_image_height: f64::INFINITY,
                bitmap_width: 750.0,
                bitmap_height: 500.0,
                is_fit: true,
            }
        }),
        Rc::new(move || fit_updates.set(fit_updates.get() + 1)),
        ViewportControllerOptions::default(),
    );
    rig.poster.drain();
    assert_eq!(calls.get(), 8);
    assert_eq!(rig.fit_updates.get(), 8);
    assert_eq!(rig.window.invalidations.get(), 1); // one redraw for the whole pass
    assert!((controller.image_actual_width() - 108.0).abs() < 1e-9);
}

// new: each side of the Fit size triggers update_fit_size on its own (C# tuple `!=`), and a fixed size does not
#[test]
fn fit_size_change_on_one_side_only_runs_update_fit_size() {
    for (vary_width, vary_height) in [(true, false), (false, true)] {
        let rig = Rig::new();
        let calls = Rc::new(Cell::new(0u32));
        let counter = Rc::clone(&calls);
        let fit_updates = Rc::clone(&rig.fit_updates);
        let controller = ViewportController::new(
            rig.window.clone(),
            rig.poster.clone(),
            rig.clock.clone(),
            Rc::new(move || {
                // Only the first read differs from the following ones when nothing varies; the size converges after 2 passes.
                counter.set(counter.get() + 1);
                let step = f64::from(counter.get().min(2));
                ViewportContent {
                    stretch: ViewerStretchMode::None,
                    image_width: 100.0 + if vary_width { step } else { 0.0 },
                    image_height: 50.0 + if vary_height { step } else { 0.0 },
                    max_image_width: f64::INFINITY,
                    max_image_height: f64::INFINITY,
                    bitmap_width: 750.0,
                    bitmap_height: 500.0,
                    is_fit: true,
                }
            }),
            Rc::new(move || fit_updates.set(fit_updates.get() + 1)),
            ViewportControllerOptions::default(),
        );
        rig.poster.drain();
        // pass 1: 0x0 -> first size (update), pass 2: size grows by 1 on the varying side (update), pass 3: no layout change (break)
        assert_eq!(
            rig.fit_updates.get(),
            2,
            "vary_width={vary_width} vary_height={vary_height}"
        );
        assert!(controller.image_actual_width() > 0.0);
    }
}

// new (T1): the fit callback may call back into the controller (invalidate, scroll_to, getters) without a double borrow
#[test]
fn update_fit_size_may_reenter_the_controller() {
    let rig = Rig::new();
    rig.content.set(ViewportContent {
        stretch: ViewerStretchMode::Uniform,
        image_width: f64::NAN,
        image_height: f64::NAN,
        max_image_width: 1000.0,
        max_image_height: 600.0,
        bitmap_width: 1500.0,
        bitmap_height: 1000.0,
        is_fit: true,
    });
    let holder: Rc<RefCell<Weak<ViewportController>>> = Rc::new(RefCell::new(Weak::new()));
    let seen = Rc::new(Cell::new(0.0f64));
    let (reenter, observed, fit_updates) = (Rc::clone(&holder), Rc::clone(&seen), Rc::clone(&rig.fit_updates));
    let controller = rig.create_full(
        ViewportControllerOptions::default(),
        Rc::new(move || {
            fit_updates.set(fit_updates.get() + 1);
            if let Some(controller) = reenter.borrow().upgrade() {
                controller.invalidate_layout();
                controller.scroll_to(10.0, 20.0);
                observed.set(controller.image_actual_width() + controller.viewport_size().0);
                let _ = controller.layout();
            }
        }),
    );
    *holder.borrow_mut() = Rc::downgrade(&controller);
    // The construction job runs now, with the holder set.
    rig.poster.drain();
    assert!(rig.fit_updates.get() >= 1);
    assert!(seen.get() > 0.0);
    // The invalidation made inside the pass posted a new job (T3: the posted flag is cleared before the pass), which drained too.
    assert!(!controller.is_layout_posted());
    assert_eq!(rig.poster.pending(), 0);
}

// new (T3): an invalidate_layout made inside a posted pass posts a new job
#[test]
fn invalidate_inside_the_posted_pass_posts_a_new_job() {
    let rig = Rig::new();
    rig.content.set(ViewportContent {
        stretch: ViewerStretchMode::Uniform,
        image_width: f64::NAN,
        image_height: f64::NAN,
        max_image_width: 1000.0,
        max_image_height: 600.0,
        bitmap_width: 1500.0,
        bitmap_height: 1000.0,
        is_fit: true,
    });
    let holder: Rc<RefCell<Weak<ViewportController>>> = Rc::new(RefCell::new(Weak::new()));
    let reenter = Rc::clone(&holder);
    let controller = rig.create_full(
        ViewportControllerOptions::default(),
        Rc::new(move || {
            if let Some(controller) = reenter.borrow().upgrade() {
                controller.invalidate_layout();
            }
        }),
    );
    *holder.borrow_mut() = Rc::downgrade(&controller);
    assert!(rig.poster.run_one()); // the construction job; the fit callback invalidates inside the pass
    assert!(controller.is_layout_posted());
    assert_eq!(rig.poster.pending(), 1);
    assert_eq!(rig.poster.posts(), 2);
    rig.poster.drain();
    assert!(!controller.is_layout_posted());
}

// new (T1): a LayoutUpdated handler may call update_layout (a nested pass with its own cap) and scroll_to; the chain terminates
#[test]
fn layout_updated_handler_may_call_update_layout() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.poster.drain();
    let calls = Rc::new(Cell::new(0u32));
    let (weak, counter) = (Rc::downgrade(&controller), Rc::clone(&calls));
    controller.subscribe_layout_updated(Rc::new(move || {
        counter.set(counter.get() + 1);
        if counter.get() == 1
            && let Some(controller) = weak.upgrade()
        {
            controller.scroll_to(50.0, 60.0);
            controller.update_layout(); // nested pass: changes the offsets, raises LayoutUpdated again
        }
    }));

    controller.scroll_to(100.0, 0.0);
    rig.poster.drain();

    assert_eq!(calls.get(), 2);
    assert_eq!(
        (controller.horizontal_offset(), controller.vertical_offset()),
        (50.0, 60.0)
    );
    assert_eq!(rig.window.invalidations.get(), 3);
}

// new: unsubscribe_layout_updated stops the notifications; an unknown id is ignored
#[test]
fn unsubscribe_layout_updated_stops_the_notifications() {
    let rig = Rig::new();
    let controller = rig.create();
    rig.poster.drain();
    let calls = Rc::new([Cell::new(0u32), Cell::new(0), Cell::new(0)]);
    let ids: Vec<u64> = (0..3)
        .map(|index| {
            let counter = Rc::clone(&calls);
            controller.subscribe_layout_updated(Rc::new(move || {
                counter[index].set(counter[index].get() + 1);
            }))
        })
        .collect();
    assert!(ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2]);
    controller.unsubscribe_layout_updated(ids.iter().max().unwrap() + 100); // unknown id: ignored
    controller.scroll_to(10.0, 0.0);
    rig.poster.drain();
    assert_eq!([calls[0].get(), calls[1].get(), calls[2].get()], [1, 1, 1]);

    controller.unsubscribe_layout_updated(ids[1]);
    controller.scroll_to(20.0, 0.0);
    rig.poster.drain();
    assert_eq!([calls[0].get(), calls[1].get(), calls[2].get()], [2, 1, 2]);
}

// new (T8): no strong cycle; dropping the owner disposes the controller (window and clock subscriptions gone, a queued job is a no-op)
#[test]
fn controller_is_freed_after_the_owner_drops_it() {
    let rig = Rig::new();
    let controller = rig.create();
    controller.hook_render_frame(Box::new(|_| {}));
    let weak = Rc::downgrade(&controller);
    assert_eq!(Rc::strong_count(&controller), 1);
    assert_eq!(rig.window.subscriber_count(ShellWindowEvent::ClientSizeChanged), 1);
    assert_eq!(rig.clock.subscriber_count(), 1);

    drop(controller);

    assert!(weak.upgrade().is_none());
    assert_eq!(rig.window.subscriber_count(ShellWindowEvent::ClientSizeChanged), 0);
    assert_eq!(rig.clock.subscriber_count(), 0);
    rig.poster.drain(); // the job queued at construction finds no controller
    assert_eq!(rig.window.invalidations.get(), 0);
    assert_eq!(Rc::strong_count(&rig.window), 1);
}
