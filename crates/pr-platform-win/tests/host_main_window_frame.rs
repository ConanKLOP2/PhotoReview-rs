//! Tests of W8-13i (N): the frame loop of `MainWindow` (`present`, `render_now`, first-present events) and the scroll-bar glue. Ports of the window-level
//! twins of `ShellStartupTests` / `ShellWindowingTests` / `ShellScrollBarEndToEndTests` (baseline `d92d6aae`) plus the new tests of the contract.
//!
//! The pure tests need no window. The native tests are `#[ignore = "native: needs desktop"]`: a real UI thread with a `Win32UiDispatcher` and a hidden
//! (never shown, never activated) shell window with a WARP surface (an offscreen one, or a swap chain on the hidden window for the present seam).
//! Every wait is bounded; no sleeps.
//!
//! NOTE for the lead: a `MainWindow` is built with 13j's `ViewingState` (composer, pacer, stats), so the native tests run once 13j is merged.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/main_window_frame_fixture.rs"]
mod main_window_frame_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use main_window_fixture::{
    BuildOptions, WarpHook, build, built, finish, new_order, run, start_ui, take_built, wait_until,
};
use main_window_frame_fixture::{
    HookFeature, WindowSurfaceHook, inject_device_loss, inject_end_draw_failure, inject_occlusion, render_until,
};
use pr_app::services::render::PresentResult;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::main_window::core::MainWindowError;
use pr_platform_win::host::main_window::frame::{FrameState, duration_ms, slow_frame_note};
use pr_platform_win::shell_viewing::pacer::PresentMode;
use pr_test_support::LogLevel;

// ---- pure ---------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn frame_state_default_is_empty() {
    let state = FrameState::default();
    let text = format!("{state:?}");
    assert!(text.contains("first_image_drawn: false"), "{text}");
}

// C#: the diagnostic string of `Present` (`$"mode={mode} result={result} img={w}x{h} fade={active}"`), Rust spelling of the enums and the bool.
#[test]
fn present_note_text_for_slow_frames() {
    assert_eq!(
        slow_frame_note(
            PresentMode::Immediate,
            PresentResult::Presented,
            Some((640, 480)),
            Some(false)
        ),
        "mode=Immediate result=Presented img=640x480 fade=false"
    );
    assert_eq!(
        slow_frame_note(
            PresentMode::VSync,
            PresentResult::DeviceRecreated,
            Some((1, 2)),
            Some(true)
        ),
        "mode=VSync result=DeviceRecreated img=1x2 fade=true"
    );
    // No image / no composer: C# prints null as an empty string.
    assert_eq!(
        slow_frame_note(PresentMode::Immediate, PresentResult::Occluded, None, Some(false)),
        "mode=Immediate result=Occluded img=x fade=false"
    );
    assert_eq!(
        slow_frame_note(PresentMode::Immediate, PresentResult::Failed, None, None),
        "mode=Immediate result=Failed img=x fade="
    );
}

// T3: the surface reports `Duration`s; the frame statistics take milliseconds (`as_secs_f64() * 1000.0`).
#[test]
fn duration_converts_to_milliseconds() {
    assert_eq!(duration_ms(std::time::Duration::ZERO), 0.0);
    assert_eq!(duration_ms(std::time::Duration::from_millis(8)), 8.0);
    assert_eq!(duration_ms(std::time::Duration::from_micros(2500)), 2.5);
    assert_eq!(duration_ms(std::time::Duration::from_secs(2)), 2000.0);
}

// ---- native: first present --------------------------------------------------------------------------------------------------------------------

// C#: ShellStartupTests.Startup_WithJpeg_OpensWindowThenDrawsFirstImageThenClosesCleanly (the canvas half: the first frame lifts the cloak, marks `firstPresent`)
#[test]
#[ignore = "native: needs desktop"]
fn first_frame_marks_first_present_and_lifts_the_cloak() {
    let ui = start_ui();
    let (before, after, marks, cloak, presented, image_flag) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let before = (b.window.first_present_done(), b.cloak.first_presents.get());
        b.window.render_now();
        let after = (b.window.first_present_done(), b.cloak.first_presents.get());
        // A second frame does not repeat the first-present work.
        b.window.render_now();
        (
            before,
            after,
            b.marks.borrow().iter().filter(|m| *m == "firstPresent").count(),
            b.cloak.first_presents.get(),
            b.hub.presented.borrow().clone(),
            b.window.first_image_presented(),
        )
    });
    assert_eq!(before, (false, 0));
    assert_eq!(after, (true, 1));
    assert_eq!(marks, 1, "the mark is recorded once");
    assert_eq!(cloak, 1);
    assert_eq!(
        presented,
        [false, false],
        "every presented frame is reported to the hub, none contained an image"
    );
    assert!(!image_flag, "a canvas frame is not the first image");
    finish(ui);
}

#[test]
#[ignore = "native: needs desktop"]
fn first_present_events_raise_in_subscription_order_and_allow_unsubscribe() {
    let ui = start_ui();
    let seen = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let order = new_order();
        let first = {
            let order = Rc::clone(&order);
            b.window
                .subscribe_first_presented(Box::new(move || order.borrow_mut().push("first".to_owned())))
        };
        let removed = {
            let order = Rc::clone(&order);
            b.window
                .subscribe_first_presented(Box::new(move || order.borrow_mut().push("removed".to_owned())))
        };
        let image = {
            let order = Rc::clone(&order);
            b.window
                .subscribe_first_image_presented(Box::new(move || order.borrow_mut().push("image".to_owned())))
        };
        {
            let order = Rc::clone(&order);
            b.window
                .subscribe_first_presented(Box::new(move || order.borrow_mut().push("second".to_owned())));
        }
        assert_ne!(first, image, "the two lists use disjoint ids");
        b.window.unsubscribe_first_present(removed);
        b.window.unsubscribe_first_present(removed); // an unknown id is a no-op
        b.window.render_now();
        b.window.render_now();
        let seen = order.borrow().clone();
        drop(b);
        seen
    });
    assert_eq!(
        seen,
        ["first", "second"],
        "FirstPresented once, in order; FirstImagePresented not on a canvas frame"
    );
    finish(ui);
}

// ---- native: guards -----------------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore = "native: needs desktop"]
fn frame_before_attach_is_a_no_op() {
    let ui = start_ui();
    let (done, presented, cloak, errors) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window.render_now(); // no surface yet
        (
            b.window.first_present_done(),
            b.hub.presented.borrow().len(),
            b.cloak.first_presents.get(),
            b.log.contains(LogLevel::Error, "Frame failed"),
        )
    });
    assert!(!done);
    assert_eq!(presented, 0);
    assert_eq!(cloak, 0);
    assert!(!errors);
    finish(ui);
}

// T2: a hook that asks for a frame from inside a frame gets nothing (a nested BeginDraw would be an error anyway).
#[test]
#[ignore = "native: needs desktop"]
fn render_now_from_inside_a_frame_returns_at_once() {
    let ui = start_ui();
    let (calls, presented, errors) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let feature = HookFeature::new("hook", new_order());
        *feature.hook.borrow_mut() = Some(Box::new(|| built().window.render_now()));
        b.window.add_feature(feature.clone());
        b.window.render_now();
        (
            feature.overlay_calls.get(),
            b.hub.presented.borrow().len(),
            b.log.contains(LogLevel::Error, "Frame failed"),
        )
    });
    assert_eq!(calls, 1, "the nested request did not draw a second frame");
    assert_eq!(presented, 1);
    assert!(!errors);
    finish(ui);
}

// ---- native: draw order and borrows -------------------------------------------------------------------------------------------------------------

// New: the overlay hooks run after the content, in registration order, once per frame; T1: the surface is busy while they draw.
#[test]
#[ignore = "native: needs desktop"]
fn draw_order_is_clear_content_bars_overlay_and_the_surface_is_busy_while_drawing() {
    let ui = start_ui();
    let (log, probes) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let log = new_order();
        let probes = Rc::new(RefCell::new(Vec::new()));
        for name in ["a", "b"] {
            let feature = HookFeature::new(name, Rc::clone(&log));
            let probes = Rc::clone(&probes);
            *feature.hook.borrow_mut() = Some(Box::new(move || {
                let window = &built().window;
                // `with_surface` must say "busy" (None), not panic; the surface still exists.
                probes
                    .borrow_mut()
                    .push((window.with_surface(|_| ()).is_none(), window.has_surface()));
            }));
            b.window.add_feature(feature);
        }
        b.window.render_now();
        let log_now = log.borrow().clone();
        let probes_now = probes.borrow().clone();
        (log_now, probes_now)
    });
    assert_eq!(log, ["a:overlay", "b:overlay"]);
    assert_eq!(probes, [(true, true), (true, true)]);
    finish(ui);
}

// C#: ShellScrollBarEndToEndTests.ResizingTheWindow_ReflowsTheBars (the layout half): a layout posted by `invalidate_layout` is applied by the next frame
// BEFORE it draws.
#[test]
#[ignore = "native: needs desktop"]
fn layout_runs_before_draw_in_the_same_frame() {
    let ui = start_ui();
    let (updates_before, updates_in_overlay) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let updates = Rc::new(Cell::new(0_u32));
        let counter = Rc::clone(&updates);
        b.window
            .viewport()
            .subscribe_layout_updated(Rc::new(move || counter.set(counter.get() + 1)));
        b.window.viewport().invalidate_layout();
        assert!(
            b.window.viewport().is_layout_posted(),
            "invalidate_layout posts a layout pass"
        );
        let updates_before = updates.get();
        let feature = HookFeature::new("probe", new_order());
        let in_overlay = Rc::new(Cell::new(0_u32));
        let (probe, seen) = (Rc::clone(&in_overlay), Rc::clone(&updates));
        *feature.hook.borrow_mut() = Some(Box::new(move || probe.set(seen.get())));
        b.window.add_feature(feature);
        b.window.render_now();
        (updates_before, in_overlay.get())
    });
    // The posted pass cannot have run (the UI thread was busy in this closure): the layout seen by the draw segment is the frame's own `update_layout`.
    assert_eq!(updates_before, 0);
    assert!(updates_in_overlay >= 1, "the layout ran before the draw segment");
    finish(ui);
}

// ---- native: faults ---------------------------------------------------------------------------------------------------------------------------

// C#: a `BeginDraw` that fails ("Frame failed") ends the frame without `OnFramePresented`.
#[test]
#[ignore = "native: needs desktop"]
fn a_fault_from_begin_draw_is_logged_and_skips_on_frame_presented() {
    let ui = start_ui();
    let (logged, done, presented, overlay_calls, recovered) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let feature = HookFeature::new("f", new_order());
        b.window.add_feature(feature.clone());
        // A frame left open: the window's BeginDraw is refused ("BeginDraw was already called").
        b.window
            .with_surface(|surface| {
                surface.begin_draw_context().expect("begin");
            })
            .expect("surface");
        b.window.render_now();
        let logged = b.log.contains(LogLevel::Error, "Frame failed");
        let done = b.window.first_present_done();
        let presented = b.hub.presented.borrow().len();
        let overlay_calls = feature.overlay_calls.get();
        // Close the open frame; the next frame works.
        b.window
            .with_surface(|surface| {
                surface.end_draw_and_present().expect("end");
            })
            .expect("surface");
        b.window.render_now();
        (logged, done, presented, overlay_calls, b.window.first_present_done())
    });
    assert!(logged);
    assert!(!done, "no first-present work after a failed frame");
    assert_eq!(presented, 0);
    assert_eq!(overlay_calls, 0);
    assert!(recovered, "the next frame presents");
    finish(ui);
}

// C#: ShellStartupTests.DeviceLost_DuringPresent_RecreatesTheDeviceAndRedrawsTheImage (the window half: the lost device is recreated, the frame counts and
// the surface keeps working).
#[test]
#[ignore = "native: needs desktop"]
fn device_lost_during_present_recreates_the_device_and_the_frame_counts() {
    let ui = start_ui();
    let (presented, errors) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        inject_device_loss(&b.window);
        b.window.render_now();
        let after_loss = b.hub.presented.borrow().len();
        b.window.render_now();
        (
            (after_loss, b.hub.presented.borrow().len()),
            b.log.contains(LogLevel::Error, "Frame failed"),
        )
    });
    assert_eq!(
        presented,
        (1, 2),
        "DeviceRecreated counts as presented and the next frame draws again"
    );
    assert!(!errors);
    finish(ui);
}

// C#: ShellStartupTests.DeviceLost_RequestsItsOwnRedraw_EvenWhenTheOsSendsNoPaint (no WM_PAINT is sent: the render event still rises after the loss).
#[test]
#[ignore = "native: needs desktop"]
fn device_lost_requests_its_own_redraw_even_when_the_os_sends_no_paint() {
    let ui = start_ui();
    let seen = Arc::new(AtomicU32::new(0));
    let published = Arc::clone(&seen);
    run(&ui, move || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        b.shell.subscribe_render(Box::new(move |_| {
            published.fetch_add(1, Ordering::SeqCst);
        }));
        // Settle: one requested frame is delivered at the next tick, so nothing is pending when the test injects the loss.
        b.shell.invalidate();
    });
    let watcher = Arc::clone(&seen);
    wait_until(&ui, "the settling frame", move || watcher.load(Ordering::SeqCst) > 0);
    seen.store(0, Ordering::SeqCst);
    run(&ui, || {
        let b = built();
        inject_device_loss(&b.window);
        b.window.render_now();
    });
    let watcher = Arc::clone(&seen);
    wait_until(&ui, "a render event after the device loss", move || {
        watcher.load(Ordering::SeqCst) > 0
    });
    finish(ui);
}

// End-draw failure: the frame is dropped (not presented), the window asks for another one.
#[test]
#[ignore = "native: needs desktop"]
fn failed_end_draw_is_not_counted_as_presented() {
    let ui = start_ui();
    let (presented, done) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        inject_end_draw_failure(&b.window);
        b.window.render_now();
        (b.hub.presented.borrow().len(), b.window.first_present_done())
    });
    assert_eq!(presented, 0);
    assert!(!done);
    finish(ui);
}

// New (T7): an occluded present was drawn and submitted; it counts so the cloak lifts and the startup marks advance.
#[test]
#[ignore = "native: needs desktop"]
fn occluded_frame_counts_as_presented() {
    let ui = start_ui();
    let (done, presented, cloak) = run(&ui, || {
        let b = build(BuildOptions::default());
        b.window
            .attach_renderer(Some(&WindowSurfaceHook))
            .expect("attach (window surface)");
        inject_occlusion(&b.window);
        let mut frames = 0;
        // `FramePacer` may defer a frame, the first one is never deferred.
        let reached = render_until(
            || {
                b.window.render_now();
                frames += 1;
            },
            || b.window.first_present_done(),
            8,
        );
        assert!(reached, "an occluded frame counted after {frames} tries");
        (
            b.window.first_present_done(),
            b.hub.presented.borrow().len(),
            b.cloak.first_presents.get(),
        )
    });
    assert!(done);
    assert!(presented >= 1);
    assert_eq!(cloak, 1);
    finish(ui);
}

// C#: ShellWindowingTests.DpiChange_WithTheSamePixelSize_StillResizesTheRenderSurfaceScaleOnTheNextFrame
#[test]
#[ignore = "native: needs desktop"]
fn dpi_change_with_the_same_pixel_size_still_resizes_the_surface_scale_on_the_next_frame() {
    let ui = start_ui();
    let (before, after, window_scale) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let window_scale = b.shell.dpi_scale();
        // The surface believes another scale; the pixel size is the window's.
        b.window
            .with_surface(|surface| surface.resize(w, h, window_scale * 2.0).expect("resize"))
            .expect("surface");
        let before = b.window.with_surface(|surface| surface.dpi_scale()).expect("surface");
        let reached = render_until(
            || b.window.render_now(),
            || b.window.with_surface(|surface| surface.dpi_scale()) == Some(window_scale),
            8,
        );
        assert!(reached, "the surface scale follows the window's");
        (
            before,
            b.window.with_surface(|surface| surface.dpi_scale()).expect("surface"),
            window_scale,
        )
    });
    assert!((before - window_scale * 2.0).abs() < f64::EPSILON);
    assert!((after - window_scale).abs() < f64::EPSILON);
    finish(ui);
}

// ---- native: scroll-bar glue --------------------------------------------------------------------------------------------------------------------

// C#: `ShellMainWindow.ScrollBars` getter (the constructor registers the bars; the error text is the registration check of the getter).
#[test]
#[ignore = "native: needs desktop"]
fn scroll_bars_are_registered_by_the_constructor() {
    let ui = start_ui();
    let (registered, error_text) = run(&ui, || {
        let b = build(BuildOptions::default());
        let bars = b.window.scroll_bars();
        let text = MainWindowError::FeatureNotRegistered { feature: "Scroll bars" }.to_string();
        (bars.is_ok(), text)
    });
    assert!(registered);
    assert_eq!(error_text, "Scroll bars are not registered.");
    finish(ui);
}

// New: the bars are in the window's handler chain only until `Closed`; they are disposed once and do not keep the window alive (16g: the cycle
// handlers -> bars -> viewport -> window).
#[test]
#[ignore = "native: needs desktop (green on a desktop 2026-11-01, G1; stays ignored like the other hidden-window tests of this file)"]
fn closed_disposes_the_bars_once_and_unsubscribes() {
    let ui = start_ui();
    let (while_open, after_close, still_reachable, drawn_after, window_gone, bars_gone) = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let bars = b.window.scroll_bars().expect("registered");
        let weak_bars = Rc::downgrade(&bars);
        let weak_shell = Rc::downgrade(&b.shell);
        // field + handler chain + this local
        let while_open = Rc::strong_count(&bars);
        // The native window is destroyed: `Closed` runs the glue.
        b.shell.dispose();
        let after_close = Rc::strong_count(&bars);
        let still_reachable = b.window.scroll_bars().is_ok();
        // After the dispose the bars draw nothing and count nothing.
        let drawn_before = bars.drawn_frames();
        b.window.render_now();
        let drawn_after = bars.drawn_frames() - drawn_before;
        drop(bars);
        drop(b);
        drop(take_built());
        shell_window_fixture::dispose_window();
        (
            while_open,
            after_close,
            still_reachable,
            drawn_after,
            weak_shell.upgrade().is_none(),
            weak_bars.upgrade().is_none(),
        )
    });
    assert_eq!(while_open, 3, "field, window handler chain and the local");
    assert_eq!(after_close, 2, "the handler chain let go at Closed");
    assert!(still_reachable, "the bars stay in the field, as in C#");
    assert_eq!(drawn_after, 0);
    assert!(window_gone, "the native window object is not kept alive by the bars");
    assert!(bars_gone, "the bars are freed with the main window");
    finish(ui);
}

// C#: ShellScrollBarEndToEndTests.BarsAreAbsent_InFitMode (the glue half): in fit mode a frame draws no bar.
#[test]
#[ignore = "native: needs desktop"]
fn bars_are_absent_in_fit_mode() {
    let ui = start_ui();
    let drawn = run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        b.window.render_now();
        b.window.render_now();
        b.window.scroll_bars().expect("registered").drawn_frames()
    });
    assert_eq!(drawn, 0);
    finish(ui);
}
