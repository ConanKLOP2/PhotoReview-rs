//! Lane `st-mainwin-seams2`: native killers for the main-window survivors that the earlier rounds left open
//! (`X-main-window-windowing`, `X-main-window-lifecycle`, `X-host-shell-window-messages`).
//!
//! - `DisplayChangeHandler` (`WM_DISPLAYCHANGE` observer): reached directly through the accessor `MainWindow::display_change_handler()` (the only production
//!   change of the lane, no behaviour change): returns `None` for EVERY message, posts a refit for `WM_DISPLAYCHANGE` only, `Debug` names the type;
//! - `on_viewer_fullscreen_changed` guard (`windowing_disposed || is_destroyed`): a viewer `IsFullscreen` change that reaches a destroyed window touches nothing;
//! - `update_fit_size_if_alive`: the posted `Render` job of a `CurrentImage` change in Fit mode really refits the decode box;
//! - the F11 placement pair (`SavePlacement` hands `restore_state()` AND `restore_bounds()` of the F11 state to the placement port);
//! - `WM_DPICHANGED` in the shell window procedure: the dpi is the LOW word of `wParam`, handlers see the change already applied, the default result is 0,
//!   the change invalidates (one frame) even when the rectangle does not change.
//!
//! Every test is `#[ignore = "native: needs desktop"]`: hidden windows (never shown or activated), fake windowing seams, no sleeps (every wait is bounded or a
//! direct call). Run with `--include-ignored --test-threads=1`.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
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
#[path = "common/windowing_fixture.rs"]
mod windowing_fixture;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use main_window_fixture::{BuildOptions, build, dispatcher, finish, run, start_ui, take_built, wait_until};
use pr_app::viewmodels::main::{ChangeSet, MainProperty};
use pr_app::windowing::contracts::WindowShowState::{Maximized, Normal};
use pr_core::abstractions::{UiDispatcher, UiPriority};
use pr_imaging::decode_types::DecodeBox;
use pr_platform_win::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use pr_platform_win::host::main_window::core::MainWindowOptions;
use pr_platform_win::host::main_window::lifecycle::target_decode_box_for;
use pr_platform_win::host::main_window::windowing::ShellWindowingSeams;
use pr_platform_win::win32::messages::{WM_ACTIVATE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SIZE};
use pr_platform_win::win32::structs::Lresult;
use shell_window_fixture::{create_window, dispose_window, send_dpi_changed, wait_until_idle, window, window_rect};
use ui_thread::UiThread;
use windowing_fixture::{FakeNative, FakeOps, MONITOR, NativeCall, WINDOW};

// ---- helpers --------------------------------------------------------------------------------------------------------------------------------

/// Fake windowing seams over a window that does NOT cover the monitor (`refit` has something to do) and the fakes themselves, so a test can read them.
fn seams(state: pr_app::windowing::contracts::WindowShowState) -> (ShellWindowingSeams, Rc<FakeOps>, Rc<FakeNative>) {
    let ops = Rc::new(FakeOps::baseline());
    let native = Rc::new(FakeNative::new(state));
    native.window.set(Some(WINDOW));
    let seams = ShellWindowingSeams {
        native: Some(native.clone()),
        ops: Some(ops.clone()),
        ..ShellWindowingSeams::default()
    };
    (seams, ops, native)
}

fn options(seams: ShellWindowingSeams, placement_file: Option<PathBuf>) -> BuildOptions {
    BuildOptions {
        options: MainWindowOptions {
            activate: false,
            prefer_warp: true,
            windowing: Some(seams),
            placement_file,
            ..MainWindowOptions::default()
        },
        ..BuildOptions::default()
    }
}

/// Waits until a `Render`-priority sentinel posted NOW has run (every earlier Render job has run).
fn drain_render(ui: &UiThread) {
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    run(ui, move || {
        dispatcher().post_with_priority(Box::new(move || flag.store(true, Ordering::SeqCst)), UiPriority::Render);
    });
    wait_until(ui, "the render-priority sentinel", move || done.load(Ordering::SeqCst));
}

fn message(hwnd: isize, msg: u32) -> WindowMessage {
    WindowMessage {
        hwnd,
        msg,
        wparam: 32,
        lparam: 0,
    }
}

// ---- DisplayChangeHandler -------------------------------------------------------------------------------------------------------------------

// Kills `DisplayChangeHandler::try_handle` `==` -> `!=` (a refit for every other message), `-> Some(..)`, and `<DisplayChangeHandler as Debug>::fmt -> Ok(Default)`.
#[test]
#[ignore = "native: needs desktop"]
fn display_change_handler_never_handles_and_refits_for_wm_displaychange_only() {
    let ui = start_ui();
    let (hwnd, debug) = run(&ui, || {
        let (seams, _ops, native) = seams(Normal);
        NATIVE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&native)));
        let b = build(options(seams, None));
        assert!(b.window.apply_fullscreen(true));
        native.calls.borrow_mut().clear();
        let handler = b
            .window
            .display_change_handler()
            .expect("registered by register_windowing");
        let hwnd = b.shell.hwnd();
        // Neighbours of 0x7E on both sides, the dpi message, size and activation, and 0: none of them is a display change and none is handled.
        for msg in [
            0,
            WM_DISPLAYCHANGE - 1,
            WM_DISPLAYCHANGE + 1,
            WM_SIZE,
            WM_DPICHANGED,
            WM_ACTIVATE,
        ] {
            assert!(
                handler.try_handle(&message(hwnd, msg)).is_none(),
                "message {msg:#x} is not handled"
            );
        }
        (hwnd, format!("{handler:?}"))
    });
    assert!(
        debug.contains("DisplayChangeHandler"),
        "Debug names the handler: {debug}"
    );
    drain_render(&ui);
    assert!(
        run(&ui, native_calls).is_empty(),
        "no message but WM_DISPLAYCHANGE posts a refit"
    );

    // WM_DISPLAYCHANGE itself: still `None` (default processing runs), and ONE refit is posted, not run inline.
    run(&ui, move || {
        let handler = main_window_fixture::built()
            .window
            .display_change_handler()
            .expect("registered");
        assert!(handler.try_handle(&message(hwnd, WM_DISPLAYCHANGE)).is_none());
        assert!(native_calls().is_empty(), "posted, not inline");
    });
    drain_render(&ui);
    assert_eq!(
        run(&ui, native_calls),
        vec![NativeCall::MoveTo(MONITOR)],
        "ONE refit onto the monitor"
    );
    finish(ui);
}

thread_local! {
    static NATIVE: std::cell::RefCell<Option<Rc<FakeNative>>> = const { std::cell::RefCell::new(None) };
}

fn native_calls() -> Vec<NativeCall> {
    NATIVE.with(|slot| slot.borrow().as_ref().expect("probe").calls())
}

// The handler holds only a Weak: once the window is gone a display change does nothing and does not panic (C# `DisplayChangeHandler(owner)` null owner).
#[test]
#[ignore = "native: needs desktop"]
fn display_change_handler_after_the_window_is_gone_does_nothing() {
    let ui = start_ui();
    run(&ui, || {
        let (seams, _ops, native) = seams(Normal);
        NATIVE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&native)));
        let b = build(options(seams, None));
        assert!(b.window.apply_fullscreen(true));
        let handler = b.window.display_change_handler().expect("registered");
        let hwnd = b.shell.hwnd();
        drop(b);
        drop(take_built());
        assert!(handler.try_handle(&message(hwnd, WM_DISPLAYCHANGE)).is_none());
    });
    drain_render(&ui);
    assert!(
        run(&ui, native_calls).is_empty(),
        "nothing is refitted for a window that is gone"
    );
    finish(ui);
}

// ---- on_viewer_fullscreen_changed -----------------------------------------------------------------------------------------------------------

// Kills `on_viewer_fullscreen_changed`: `windowing_disposed || is_destroyed` -> `&&`. The shell window is destroyed (here by a plain close) while the main window is not
// disposed: the viewer's `IsFullscreen` change reaches `on_viewer_fullscreen_changed` through `apply_changes` and must stop at the destroyed-window guard.
#[test]
#[ignore = "native: needs desktop"]
fn a_viewer_fullscreen_change_on_a_destroyed_window_touches_nothing() {
    let ui = start_ui();
    run(&ui, || {
        let (seams, ops, native) = seams(Normal);
        let b = build(options(seams, None));
        let fullscreen = b.window.fullscreen().expect("registered");
        b.shell.close();
        assert!(b.shell.is_destroyed(), "the plain close destroyed the window");
        assert!(!fullscreen.is_active());
        ops.log.borrow_mut().clear();
        native.calls.borrow_mut().clear();

        let changes = b.window.view_model().borrow_mut().toggle_fullscreen();
        assert!(
            b.window.view_model().borrow().viewer().borrow().is_fullscreen(),
            "the viewer now wants F11"
        );
        b.window.apply_changes(&changes);
        assert!(!fullscreen.is_active(), "no F11 on a destroyed window");
        assert!(ops.ops().is_empty(), "no user32 call: {:?}", ops.ops());
        assert!(native.calls().is_empty(), "no native call: {:?}", native.calls());
    });
    finish(ui);
}

// ---- update_fit_size_if_alive ---------------------------------------------------------------------------------------------------------------

// Kills `update_fit_size_if_alive -> ()`: a `CurrentImage` change in Fit mode posts ONE `Render` job that refits (decode box from the live viewport and dpi).
#[test]
#[ignore = "native: needs desktop"]
fn a_current_image_change_in_fit_posts_a_render_job_that_refits_the_decode_box() {
    let ui = start_ui();
    let sentinel = DecodeBox::new(1, 1);
    run(&ui, move || {
        let b = build(BuildOptions::default());
        assert!(
            b.window.view_model().borrow().viewer().borrow().is_fit(),
            "a new viewer is in Fit"
        );
        b.size_source.set_target_decode_box(sentinel);
        let mut changes = ChangeSet::default();
        changes.main.insert(MainProperty::CurrentImage);
        b.window.apply_changes(&changes);
        assert_eq!(
            b.size_source.target_decode_box(),
            sentinel,
            "posted at Render priority, not run inline"
        );
    });
    drain_render(&ui);
    run(&ui, move || {
        let b = main_window_fixture::built();
        let (width, height) = b.window.viewport().viewport_size();
        let expected = target_decode_box_for(width, height, b.shell.dpi_scale()).expect("a valid dpi");
        assert_ne!(expected, sentinel);
        assert_eq!(
            b.size_source.target_decode_box(),
            expected,
            "the job refitted the decode box to the viewport"
        );
    });
    finish(ui);
}

// Control: a `CurrentImage` change when the viewer is NOT in Fit posts no refit (the guard of `fan_out_changes`).
#[test]
#[ignore = "native: needs desktop"]
fn a_current_image_change_outside_fit_does_not_refit() {
    let ui = start_ui();
    let sentinel = DecodeBox::new(1, 1);
    run(&ui, move || {
        let b = build(BuildOptions::default());
        let update = b
            .window
            .view_model()
            .borrow()
            .viewer()
            .borrow_mut()
            .zoom_to_actual_size();
        b.window.apply_viewer_changes(update.changes);
        assert!(!b.window.view_model().borrow().viewer().borrow().is_fit());
        b.size_source.set_target_decode_box(sentinel);
        let mut changes = ChangeSet::default();
        changes.main.insert(MainProperty::CurrentImage);
        b.window.apply_changes(&changes);
    });
    drain_render(&ui);
    run(&ui, move || {
        assert_eq!(main_window_fixture::built().size_source.target_decode_box(), sentinel);
    });
    finish(ui);
}

// ---- the F11 placement pair -----------------------------------------------------------------------------------------------------------------

/// `(restore_state, restore_bounds)` the placement port received, and the bounds the F11 state held just before the close.
type PlacementPair = (
    Option<pr_app::windowing::contracts::WindowShowState>,
    Option<(i32, i32, i32, i32)>,
    Option<(i32, i32, i32, i32)>,
);

/// Closes the window (`WM_CLOSE`) and returns the one `(restore_state, restore_bounds)` pair that the placement port received.
fn closed_placement_pair(
    state: pr_app::windowing::contracts::WindowShowState,
    fullscreen_at_close: bool,
) -> PlacementPair {
    let ui = start_ui();
    let result = run(&ui, move || {
        let (seams, _ops, _native) = seams(state);
        let b = build(options(seams, Some(PathBuf::from("placement-for-tests.json"))));
        let fullscreen = b.window.fullscreen().expect("registered");
        assert!(b.window.apply_fullscreen(true));
        if !fullscreen_at_close {
            assert!(b.window.apply_fullscreen(false));
        }
        let bounds_in_f11 = fullscreen.restore_bounds();
        b.shell.close();
        assert!(b.shell.is_destroyed());
        let saves = b.placement.saves.borrow().clone();
        assert_eq!(saves.len(), 1, "one placement save on close: {saves:?}");
        assert_eq!(saves[0].file, PathBuf::from("placement-for-tests.json"));
        (saves[0].restore_state, saves[0].restore_bounds, bounds_in_f11)
    });
    finish(ui);
    result
}

// C# `SavePlacement`: closing in F11 saves the state and the Normal rect from BEFORE F11 (R7-10), not the monitor-sized F11 window.
#[test]
#[ignore = "native: needs desktop"]
fn closing_in_f11_saves_the_state_and_the_rect_from_before_f11() {
    let (state, bounds, in_f11) = closed_placement_pair(Normal, true);
    assert_eq!(state, Some(Normal), "restore_state");
    let bounds = bounds.expect("restore_bounds");
    assert_eq!(
        (bounds.0, bounds.1),
        (WINDOW.left, WINDOW.top),
        "the rect from before F11: {bounds:?}"
    );
    assert_eq!(Some(bounds), in_f11, "the pair is the one the F11 state holds");
    assert_ne!(
        (bounds.2, bounds.3),
        (MONITOR.right, MONITOR.bottom),
        "not the monitor rect"
    );
}

// The same with a maximised window: the state saved is Maximized (the pair is two independent values).
#[test]
#[ignore = "native: needs desktop"]
fn closing_in_f11_from_a_maximised_window_saves_maximized_and_the_normal_rect() {
    let (state, bounds, in_f11) = closed_placement_pair(Maximized, true);
    assert_eq!(state, Some(Maximized), "restore_state");
    let bounds = bounds.expect("restore_bounds");
    assert_eq!((bounds.0, bounds.1), (WINDOW.left, WINDOW.top), "{bounds:?}");
    assert_eq!(Some(bounds), in_f11);
}

// After F11 was left, the pair is gone: the port gets `(None, None)` and the real code saves the live placement itself.
#[test]
#[ignore = "native: needs desktop"]
fn closing_after_leaving_f11_saves_no_f11_pair() {
    let (state, bounds, in_f11) = closed_placement_pair(Normal, false);
    assert_eq!(state, None);
    assert_eq!(bounds, None);
    assert_eq!(in_f11, None);
}

// ---- WM_DPICHANGED in the shell window procedure --------------------------------------------------------------------------------------------

/// A handler that records what it sees at `WM_DPICHANGED` and answers with `result`.
struct DpiProbe {
    result: Option<Lresult>,
    seen: Arc<Mutex<Vec<(f64, pr_platform_win::win32::structs::Rect, usize)>>>,
}

impl WindowMessageHandler for DpiProbe {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        if message.msg != WM_DPICHANGED {
            return None;
        }
        let w = window();
        self.seen
            .lock()
            .unwrap()
            .push((w.dpi_scale(), window_rect(w.hwnd()), message.wparam));
        self.result
    }
}

fn start_window() -> (UiThread, isize) {
    let ui = UiThread::start();
    let hwnd = ui.invoke(|| create_window(Default::default()).hwnd());
    (ui, hwnd)
}

fn stop(ui: &UiThread) {
    ui.invoke(dispose_window);
}

// The new dpi is the LOW word of `wParam` (the high word is the y dpi, equal to it in practice): kills `low_word` -> `high_word` in the `WM_DPICHANGED` arm.
#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_takes_the_new_dpi_from_the_low_word_of_wparam() {
    let (ui, hwnd) = start_window();
    let (first, second) = ui.invoke(move || {
        let rect = window_rect(hwnd);
        let send = |low: usize, high: usize| {
            // SAFETY: `lParam` points to a live local RECT for the duration of the synchronous call, as WM_DPICHANGED defines.
            unsafe {
                pr_platform_win::win32::dialog_native::user32::send_message(
                    hwnd,
                    WM_DPICHANGED,
                    (high << 16) | low,
                    &rect as *const pr_platform_win::win32::structs::Rect as isize,
                )
            };
            window().dpi_scale()
        };
        (send(144, 96), send(96, 192))
    });
    assert_eq!(first, 1.5, "low 144, high 96");
    assert_eq!(second, 1.0, "low 96, high 192");
    stop(&ui);
}

// Handlers run AFTER the window applied the change (dpi and suggested rect), see `wParam` unchanged; the default result is 0 and a handler result wins.
#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_handlers_see_the_change_applied_and_may_override_the_default_result() {
    let (ui, hwnd) = start_window();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let suggested = pr_platform_win::host::contracts::new_rect(60, 70, 60 + 700, 70 + 500);
    let probe_seen = Arc::clone(&seen);
    let (default_result, overridden) = ui.invoke(move || {
        let w = window();
        w.add_message_handler(Rc::new(DpiProbe {
            result: None,
            seen: Arc::clone(&probe_seen),
        }));
        w.add_message_handler(Rc::new(OverrideAt { result: 77 }));
        shell_window_fixture::send_dpi_changed(hwnd, 144, suggested);
        // `send_dpi_changed` drops the result; read the default one through a direct send.
        let rect = window_rect(hwnd);
        // SAFETY: `lParam` points to a live local RECT for the duration of the synchronous call.
        let default_result = unsafe {
            pr_platform_win::win32::dialog_native::user32::send_message(
                hwnd,
                WM_DPICHANGED,
                (144 << 16) | 144,
                &rect as *const pr_platform_win::win32::structs::Rect as isize,
            )
        };
        // Arm the overriding handler: its answer must win over the default 0.
        arm_override(true);
        // SAFETY: as above.
        let overridden = unsafe {
            pr_platform_win::win32::dialog_native::user32::send_message(
                hwnd,
                WM_DPICHANGED,
                (144 << 16) | 144,
                &rect as *const pr_platform_win::win32::structs::Rect as isize,
            )
        };
        (default_result, overridden)
    });
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].0, 1.5, "the handler sees the new dpi");
    assert_eq!(
        seen[0].1, suggested,
        "the handler sees the suggested rect already applied"
    );
    assert_eq!(seen[0].2, (144 << 16) | 144, "wParam is passed on unchanged");
    assert_eq!(default_result, 0, "the default result of WM_DPICHANGED is 0");
    assert_eq!(overridden, 77, "the first handler that answers wins");
    stop(&ui);
}

thread_local! {
    static OVERRIDE_ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn arm_override(value: bool) {
    OVERRIDE_ARMED.with(|cell| cell.set(value));
}

/// Answers `WM_DPICHANGED` with `result` once armed.
struct OverrideAt {
    result: Lresult,
}

impl WindowMessageHandler for OverrideAt {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        (message.msg == WM_DPICHANGED && OVERRIDE_ARMED.with(std::cell::Cell::get)).then_some(self.result)
    }
}

// A dpi change with an UNCHANGED rectangle (so no WM_SIZE, which would invalidate by itself) still invalidates: exactly one frame follows.
#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_invalidates_even_when_the_rectangle_does_not_change() {
    let (ui, hwnd) = start_window();
    wait_until_idle(&ui);
    let before = ui.invoke(|| window().render_count());
    ui.invoke(move || send_dpi_changed(hwnd, 192, window_rect(hwnd)));
    wait_until_idle(&ui);
    let after = ui.invoke(|| window().render_count());
    assert_eq!(after - before, 1, "one frame requested by the dpi change");
    stop(&ui);
}
