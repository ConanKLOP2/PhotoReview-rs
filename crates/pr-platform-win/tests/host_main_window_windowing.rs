//! Tests of W8-13k: `ShellFullscreen`, `apply_title_bar`, the real `Win32WindowOps` / `Win32FullscreenNative` / `Win32MonitorLayout` and the window glue
//! (`apply_fullscreen`, `WM_DISPLAYCHANGE`, `DpiChanged`). Ports of `ShellFullscreenTests` (3), `TitleBarTests` (6) and the window-level twins of
//! `ShellWindowingTests` (baseline `d92d6aae`), plus the hand-written tests of `contracts/W8-13k.md`.
//!
//! The F tests (fakes, no window) always run. The N tests are `#[ignore = "native: needs desktop"]`: they use hidden (never shown, never activated) windows only;
//! no test shows a window, F11 included. The tests that build a whole `MainWindow` also run the constructor of 13i/13j.
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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use main_window_fixture::{BuildOptions, build, built, dispatcher, finish, run, start_ui, take_built, wait_until};
use pr_app::windowing::contracts::WindowShowState::{Maximized, Minimized, Normal};
use pr_app::windowing::fullscreen::{FullscreenController, WindowOps};
use pr_app::windowing::monitor_layout::{FixedMonitorLayout, MonitorLayout, ScreenRect};
use pr_core::abstractions::{UiDispatcher, UiPriority};
use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent};
use pr_platform_win::host::main_window::core::MainWindowOptions;
use pr_platform_win::host::main_window::windowing::{
    CAPTION_COLOR_BGR, DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY, DwmWindowAttributes, FullscreenNative, ShellFullscreen,
    ShellWindowingSeams, Win32DwmWindowAttributes, Win32FullscreenNative, Win32MonitorLayout, Win32WindowOps,
    apply_title_bar,
};
use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::messages::{GWL_STYLE, WM_DISPLAYCHANGE};
use shell_window_fixture::{create_window, dispose_window, send, send_dpi_changed, window_rect};
use windowing_fixture::{FakeDwm, FakeNative, FakeOps, HWND, MONITOR, MOVE, NativeCall, Op, STYLE, WINDOW, WORK};

/// `STYLE` without the chrome bits: what enter sets.
const STYLE_FULLSCREEN: isize = 0x1600_0000;
/// `WS_MINIMIZE`, `WS_MAXIMIZE` (what `IsIconic` / `IsZoomed` read, so a HIDDEN window can be put in those states).
const WS_MINIMIZE: isize = 0x2000_0000;
const WS_MAXIMIZE: isize = 0x0100_0000;

fn shell_fullscreen(ops: &Rc<FakeOps>, native: &Rc<FakeNative>, works: Vec<ScreenRect>) -> Rc<ShellFullscreen> {
    let controller = FullscreenController::new(Box::new(FixedMonitorLayout::new(works)));
    let ops: Rc<dyn WindowOps> = ops.clone();
    let native: Rc<dyn FullscreenNative> = native.clone();
    Rc::new(ShellFullscreen::new(controller, ops, native))
}

fn baseline() -> (Rc<FakeOps>, Rc<FakeNative>, Rc<ShellFullscreen>) {
    let ops = Rc::new(FakeOps::baseline());
    let native = Rc::new(FakeNative::new(Normal));
    let fullscreen = shell_fullscreen(&ops, &native, vec![WORK]);
    (ops, native, fullscreen)
}

// ---- F: ShellFullscreenTests ---------------------------------------------------------------------------------------------------------------------

// C#: NotFullscreen_HasNoRestoreStateAndIgnoresRefitAndExit
#[test]
fn not_fullscreen_has_no_restore_state_and_ignores_refit_and_exit() {
    let (ops, native, fullscreen) = baseline();
    assert!(!fullscreen.is_active());
    assert_eq!(fullscreen.restore_state(), None);
    assert_eq!(fullscreen.restore_bounds(), None);
    assert!(!fullscreen.refit(HWND));
    assert!(!fullscreen.apply(HWND, false));
    assert!(ops.ops().is_empty() && native.calls().is_empty(), "nothing touched");
}

// C#: WithoutAHandle_NothingHappens
#[test]
fn without_a_handle_nothing_happens() {
    let (ops, native, fullscreen) = baseline();
    assert!(!fullscreen.apply(0, true));
    assert!(!fullscreen.is_active());
    assert!(!fullscreen.refit(0));
    assert!(ops.ops().is_empty() && native.calls().is_empty());
}

// C#: Reset_DropsTheFullscreenState
#[test]
fn reset_drops_the_fullscreen_state() {
    let (_ops, _native, fullscreen) = baseline();
    assert!(fullscreen.apply(HWND, true));
    assert!(fullscreen.is_active());
    fullscreen.reset();
    assert!(!fullscreen.is_active());
    assert_eq!(fullscreen.restore_state(), None);
    assert_eq!(fullscreen.restore_bounds(), None);
}

// New: the documented order of the controller's calls; `active` is assigned after them.
#[test]
fn apply_enter_then_exit_calls_ops_in_the_documented_order() {
    let (ops, native, fullscreen) = baseline();
    assert!(fullscreen.apply(HWND, true));
    assert_eq!(
        ops.ops(),
        vec![Op::SetStyle(STYLE_FULLSCREEN), Op::SetPos(MONITOR, MOVE)],
        "enter: chrome off, then ONE SetWindowPos onto the monitor"
    );
    assert!(native.calls().is_empty(), "the monitor was known: no maximise fallback");
    ops.log.borrow_mut().clear();
    assert!(fullscreen.apply(HWND, false));
    assert_eq!(
        ops.ops(),
        vec![Op::SetStyle(STYLE), Op::SetPos(WINDOW, MOVE)],
        "exit: the style back, then ONE SetWindowPos to the exact rect"
    );
    assert!(native.calls().is_empty());
    assert!(!fullscreen.is_active());
}

// T2: a second enter must not overwrite the state from before the first one.
#[test]
fn second_enter_does_not_overwrite_the_state_before_the_first() {
    let (ops, native, fullscreen) = baseline();
    native.state.set(Maximized);
    assert!(fullscreen.apply(HWND, true));
    assert_eq!(fullscreen.restore_state(), Some(Maximized));
    assert_eq!(fullscreen.restore_bounds(), Some((100, 100, 700, 500)));
    // The window is now the fullscreen one; a second enter would record that.
    native.state.set(Normal);
    let calls_before = ops.ops().len();
    assert!(!fullscreen.apply(HWND, true), "idempotent: false");
    assert_eq!(ops.ops().len(), calls_before, "nothing touched by the second enter");
    assert_eq!(fullscreen.restore_state(), Some(Maximized));
    assert_eq!(fullscreen.restore_bounds(), Some((100, 100, 700, 500)));
}

#[test]
fn exit_when_not_fullscreen_is_a_no_op() {
    let (ops, native, fullscreen) = baseline();
    assert!(!fullscreen.apply(HWND, false));
    assert!(ops.ops().is_empty() && native.calls().is_empty());
    assert!(fullscreen.apply(HWND, true));
    assert!(fullscreen.apply(HWND, false));
    let calls = ops.ops().len();
    assert!(!fullscreen.apply(HWND, false), "a repeated exit is a no-op");
    assert_eq!(ops.ops().len(), calls);
}

// F11 on a minimised window: the controller restores it first (T8), `show` is not called again.
#[test]
fn enter_while_minimized_restores_first_and_counts_as_normal() {
    let mut fake = FakeOps::baseline();
    fake.iconic = true;
    let ops = Rc::new(fake);
    let native = Rc::new(FakeNative::new(Minimized));
    let fullscreen = shell_fullscreen(&ops, &native, vec![WORK]);
    assert!(fullscreen.apply(HWND, true));
    assert_eq!(ops.ops()[0], Op::ShowNormal, "restored before the move");
    assert!(native.calls().is_empty());
    assert_eq!(fullscreen.restore_state(), Some(Normal));
}

#[test]
fn enter_with_no_monitor_info_shows_maximized_and_exit_restores_the_state() {
    let mut fake = FakeOps::baseline();
    fake.monitor = None;
    let ops = Rc::new(fake);
    let native = Rc::new(FakeNative::new(Maximized));
    let fullscreen = shell_fullscreen(&ops, &native, vec![WORK]);
    assert!(fullscreen.apply(HWND, true));
    assert_eq!(native.calls(), vec![NativeCall::Show(Maximized)], "the OS maximises");
    assert!(
        !fullscreen.refit(HWND),
        "the maximise fallback keeps the window covering the monitor"
    );
    native.calls.borrow_mut().clear();
    assert!(fullscreen.apply(HWND, false));
    assert!(!fullscreen.is_active());
    assert_eq!(
        native.calls(),
        vec![NativeCall::Show(Maximized)],
        "back to the state before F11"
    );
}

#[test]
fn refit_moves_only_when_active_and_the_window_differs_from_the_monitor() {
    let (_ops, native, fullscreen) = baseline();
    assert!(!fullscreen.refit(HWND), "not active");
    assert!(fullscreen.apply(HWND, true));
    native.window.set(Some(MONITOR));
    assert!(!fullscreen.refit(HWND), "window == monitor");
    native.window.set(Some(WINDOW));
    assert!(fullscreen.refit(HWND), "window != monitor");
    assert_eq!(native.calls(), vec![NativeCall::MoveTo(MONITOR)]);
    native.window.set(None);
    assert!(!fullscreen.refit(HWND), "no window rect");
    native.window.set(Some(WINDOW));
    native.monitor.set(None);
    assert!(!fullscreen.refit(HWND), "no monitor rect");
    assert_eq!(native.calls().len(), 1, "no extra move");
}

#[test]
fn refit_in_the_maximise_fallback_is_false() {
    let mut fake = FakeOps::baseline();
    fake.monitor = None;
    let ops = Rc::new(fake);
    let native = Rc::new(FakeNative::new(Normal));
    native.window.set(Some(WINDOW));
    let fullscreen = shell_fullscreen(&ops, &native, vec![WORK]);
    assert!(fullscreen.apply(HWND, true));
    native.calls.borrow_mut().clear();
    assert!(!fullscreen.refit(HWND));
    assert!(native.calls().is_empty());
}

// D-W8B4M-10: `SetWindowPos` inside enter/exit re-enters through the window procedure; a nested refit must not panic and must do nothing.
#[test]
fn refit_during_enter_or_exit_is_a_no_op() {
    let ops = Rc::new(FakeOps::baseline());
    let native = Rc::new(FakeNative::new(Normal));
    native.window.set(Some(WINDOW)); // differs from the monitor: a refit would move
    let fullscreen = shell_fullscreen(&ops, &native, vec![WORK]);
    let results: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let hook_results = Rc::clone(&results);
    let hook_target = Rc::clone(&fullscreen);
    *ops.on_set_pos.borrow_mut() = Some(Box::new(move || {
        let refit = hook_target.refit(HWND);
        hook_results.borrow_mut().push(refit);
        // The accessors are re-entrant too.
        let _ = (
            hook_target.is_active(),
            hook_target.restore_state(),
            hook_target.restore_bounds(),
        );
    }));
    assert!(fullscreen.apply(HWND, true));
    assert!(fullscreen.apply(HWND, false));
    assert_eq!(
        *results.borrow(),
        vec![false, false],
        "one nested refit per SetWindowPos, both refused"
    );
    assert!(native.calls().is_empty(), "no nested move");
    *ops.on_set_pos.borrow_mut() = None; // break the Rc cycle
}

// A nested apply from a handler must not panic either.
#[test]
fn apply_during_enter_is_refused_without_panicking() {
    let ops = Rc::new(FakeOps::baseline());
    let native = Rc::new(FakeNative::new(Normal));
    let fullscreen = shell_fullscreen(&ops, &native, vec![WORK]);
    let nested = Rc::new(Cell::new(None));
    let (hook_nested, hook_target) = (Rc::clone(&nested), Rc::clone(&fullscreen));
    *ops.on_set_pos.borrow_mut() = Some(Box::new(move || hook_nested.set(Some(hook_target.apply(HWND, false)))));
    assert!(fullscreen.apply(HWND, true));
    assert_eq!(nested.get(), Some(false));
    assert!(fullscreen.is_active());
    *ops.on_set_pos.borrow_mut() = None;
}

#[test]
fn restore_state_and_bounds_exist_only_while_active() {
    let (_ops, native, fullscreen) = baseline();
    native.state.set(Maximized);
    assert_eq!((fullscreen.restore_state(), fullscreen.restore_bounds()), (None, None));
    fullscreen.apply(HWND, true);
    assert_eq!(fullscreen.restore_state(), Some(Maximized));
    assert_eq!(fullscreen.restore_bounds(), Some((100, 100, 700, 500)));
    fullscreen.apply(HWND, false);
    assert_eq!((fullscreen.restore_state(), fullscreen.restore_bounds()), (None, None));
}

// ---- F: TitleBarTests ----------------------------------------------------------------------------------------------------------------------------

// C#: Apply_SetsImmersiveDarkModeAndTheDarkWindowCaptionColour
#[test]
fn apply_sets_immersive_dark_mode_and_the_dark_window_caption_colour() {
    let dwm = FakeDwm::new(|_| true);
    assert!(apply_title_bar(0x42, Some(&dwm)));
    assert_eq!(dwm.calls(), vec![(0x42, 20, 1), (0x42, 35, 0x0017_1717)]);
}

// C#: Apply_FallsBackToTheLegacyDarkModeAttributeOnOlderWindows10
#[test]
fn apply_falls_back_to_the_legacy_dark_mode_attribute_on_older_windows_10() {
    let dwm = FakeDwm::new(|attribute| attribute == 19);
    assert!(apply_title_bar(0x42, Some(&dwm)));
    assert_eq!(dwm.calls(), vec![(0x42, 20, 1), (0x42, 19, 1), (0x42, 35, 0x0017_1717)]);
}

// C#: Apply_WhenNothingIsSupported_ReturnsFalseAndDoesNotThrow
#[test]
fn apply_when_nothing_is_supported_returns_false_and_does_not_panic() {
    let dwm = FakeDwm::new(|_| false);
    assert!(!apply_title_bar(0x42, Some(&dwm)));
    assert_eq!(dwm.calls().len(), 3, "20, 19 and the caption colour are all tried");
}

// C#: Apply_WithoutAHandle_DoesNothing
#[test]
fn apply_without_a_handle_does_nothing() {
    let dwm = FakeDwm::new(|_| true);
    assert!(!apply_title_bar(0, Some(&dwm)));
    assert!(dwm.calls().is_empty());
}

// C#: Apply_WhenDwmapiIsMissing_IsSwallowed (the real seam reports `false`; the fake that "throws" has no Rust form)
#[test]
fn apply_when_dwmapi_is_missing_is_swallowed() {
    let dwm = FakeDwm::new(|_| false);
    assert!(!apply_title_bar(1, Some(&dwm)));
}

// The five outcomes; the caption colour is best effort and never changes the result.
#[test]
fn apply_title_bar_sequence_table() {
    // (accepted attributes, expected result, expected attribute sequence)
    let cases: [(&[u32], bool, Vec<u32>); 5] = [
        (&[20, 35], true, vec![20, 35]),
        (&[20], true, vec![20, 35]),
        (&[19, 35], true, vec![20, 19, 35]),
        (&[35], false, vec![20, 19, 35]),
        (&[], false, vec![20, 19, 35]),
    ];
    for (accepted, result, attributes) in cases {
        let accepted: Vec<u32> = accepted.to_vec();
        let dwm = FakeDwm::new(move |attribute| accepted.contains(&attribute));
        assert_eq!(apply_title_bar(9, Some(&dwm)), result, "{attributes:?}");
        let seen: Vec<u32> = dwm.calls().iter().map(|(_, attribute, _)| *attribute).collect();
        assert_eq!(seen, attributes);
    }
}

#[test]
fn caption_color_is_dark_window_bgr() {
    assert_eq!(CAPTION_COLOR_BGR, 0x0017_1717);
    assert_eq!(DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY, 19);
}

#[test]
fn windowing_seams_default_is_all_real() {
    let seams = ShellWindowingSeams::default();
    assert!(seams.native.is_none() && seams.ops.is_none() && seams.monitor_layout.is_none() && seams.dwm.is_none());
}

// ---- N: real Win32 (hidden message-only style windows; no MainWindow) ---------------------------------------------------------------------------

// C#: Apply_OnARealWindowHandleThatDoesNotExist_NeverThrows
#[test]
#[ignore = "native: needs desktop"]
fn real_dwm_title_bar_never_throws_on_a_bogus_handle() {
    assert!(
        !apply_title_bar(0x00FF_FFFF, None),
        "DWM refuses a window that does not exist"
    );
    assert!(!Win32DwmWindowAttributes.set(0x00FF_FFFF, 20, 1));
}

#[test]
#[ignore = "native: needs desktop"]
fn win32_monitor_layout_returns_one_work_area_per_monitor_and_the_nearest_monitor() {
    let layout = Win32MonitorLayout;
    let works = layout.work_areas();
    let monitors = user32::enum_display_monitors(None).expect("enumeration");
    assert_eq!(works.len(), monitors.len(), "one work area per monitor");
    assert!(!works.is_empty());
    for (item, work) in monitors.iter().zip(&works) {
        assert_eq!(
            layout.try_get_work(item.monitor),
            Some(*work),
            "enumeration order is kept"
        );
        assert!(work.width() > 0 && work.height() > 0);
    }
    assert_eq!(layout.try_get_work(0), None, "a null monitor has no work area");
    let ui = start_ui();
    let (nearest, handles) = run(&ui, || {
        let window = create_window(Default::default());
        let nearest = Win32MonitorLayout.monitor_of(window.hwnd());
        let handles: Vec<isize> = user32::enum_display_monitors(None)
            .expect("enumeration")
            .iter()
            .map(|item| item.monitor)
            .collect();
        dispose_window();
        (nearest, handles)
    });
    assert!(handles.contains(&nearest), "{nearest} in {handles:?}");
    ui.dispose().expect("ui ends");
}

#[test]
#[ignore = "native: needs desktop"]
fn win32_window_ops_style_roundtrip_on_a_hidden_window() {
    let ui = start_ui();
    run(&ui, || {
        let window = create_window(Default::default());
        let hwnd = window.hwnd();
        let ops = Win32WindowOps;
        let original = ops.window_style(hwnd);
        assert_eq!(original, user32::get_window_long_ptr(hwnd, GWL_STYLE));
        assert_ne!(original, 0);
        let changed = original ^ 0x0001_0000; // WS_MAXIMIZEBOX: a harmless bit
        ops.set_window_style(hwnd, changed);
        assert_eq!(ops.window_style(hwnd), changed);
        ops.set_window_style(hwnd, original);
        assert_eq!(ops.window_style(hwnd), original);
        assert!(!user32::is_window_visible(hwnd), "never shown");
        dispose_window();
    });
    ui.dispose().expect("ui ends");
}

#[test]
#[ignore = "native: needs desktop"]
fn win32_window_ops_nearest_monitor_matches_monitor_from_window() {
    let ui = start_ui();
    run(&ui, || {
        let window = create_window(Default::default());
        let hwnd = window.hwnd();
        let ops = Win32WindowOps;
        let snapshot = ops.nearest_monitor(hwnd).expect("a monitor");
        assert_eq!(snapshot.handle, Win32MonitorLayout.monitor_of(hwnd));
        assert_eq!(Some(snapshot.work), Win32MonitorLayout.try_get_work(snapshot.handle));
        assert_eq!(Win32FullscreenNative.try_get_monitor_rect(hwnd), Some(snapshot.monitor));
        assert!(ops.normal_position(hwnd).expect("placement").width() > 0);
        let rect = ops.window_rect(hwnd).expect("window rect");
        assert_eq!(Some(rect), Win32FullscreenNative.try_get_window_rect(hwnd));
        assert_eq!(ops.window_rect(0), None, "no window: no rect");
        assert!(!user32::is_window_visible(hwnd), "never shown");
        dispose_window();
    });
    ui.dispose().expect("ui ends");
}

#[test]
#[ignore = "native: needs desktop"]
fn win32_fullscreen_native_current_state_normal_minimized_maximized() {
    let ui = start_ui();
    run(&ui, || {
        let window = create_window(Default::default());
        let hwnd = window.hwnd();
        let (ops, native) = (Win32WindowOps, Win32FullscreenNative);
        let original = ops.window_style(hwnd);
        assert_eq!(native.current_state(hwnd), Normal);
        // `IsIconic` / `IsZoomed` read the style bits, so a HIDDEN window can be put in those states without showing it.
        ops.set_window_style(hwnd, original | WS_MAXIMIZE);
        assert_eq!(native.current_state(hwnd), Maximized);
        ops.set_window_style(hwnd, original | WS_MINIMIZE);
        assert_eq!(native.current_state(hwnd), Minimized);
        assert!(ops.is_iconic(hwnd));
        ops.set_window_style(hwnd, original);
        assert_eq!(native.current_state(hwnd), Normal);
        assert!(!user32::is_window_visible(hwnd), "never shown");
        dispose_window();
    });
    ui.dispose().expect("ui ends");
}

// The real ops + real native on a hidden window: F11 covers the whole monitor, exit puts the exact rect back (twin of F11_OnANormalWindow_...).
#[test]
#[ignore = "native: needs desktop"]
fn real_shell_fullscreen_covers_the_monitor_and_exit_restores_the_exact_rect() {
    let ui = start_ui();
    run(&ui, || {
        let window = create_window(Default::default());
        let hwnd = window.hwnd();
        let controller = FullscreenController::new(Box::new(Win32MonitorLayout));
        let fullscreen = ShellFullscreen::new(controller, Rc::new(Win32WindowOps), Rc::new(Win32FullscreenNative));
        let before = window_rect(hwnd);
        let monitor = Win32FullscreenNative.try_get_monitor_rect(hwnd).expect("monitor");
        let style = Win32WindowOps.window_style(hwnd);
        assert!(fullscreen.apply(hwnd, true));
        let now = window_rect(hwnd);
        assert_eq!(
            (now.left, now.top, now.right, now.bottom),
            (monitor.left, monitor.top, monitor.right, monitor.bottom)
        );
        assert_ne!(Win32WindowOps.window_style(hwnd), style, "chrome removed");
        assert!(!fullscreen.refit(hwnd), "already covering the monitor");
        assert!(!fullscreen.apply(hwnd, true), "twice: no-op");
        assert!(fullscreen.restore_bounds().is_some());
        assert!(fullscreen.apply(hwnd, false));
        let after = window_rect(hwnd);
        assert_eq!(
            (after.left, after.top, after.right, after.bottom),
            (before.left, before.top, before.right, before.bottom)
        );
        assert_eq!(Win32WindowOps.window_style(hwnd), style, "style back");
        assert!(!user32::is_window_visible(hwnd), "never shown");
        // A window moved off the monitor is brought back by refit.
        assert!(fullscreen.apply(hwnd, true));
        Win32FullscreenNative.move_to(hwnd, ScreenRect::new(10, 10, 300, 300));
        assert!(fullscreen.refit(hwnd));
        let refit = window_rect(hwnd);
        assert_eq!(
            (refit.left, refit.top, refit.right, refit.bottom),
            (monitor.left, monitor.top, monitor.right, monitor.bottom)
        );
        assert!(fullscreen.apply(hwnd, false));
        dispose_window();
    });
    ui.dispose().expect("ui ends");
}

// ---- N: the window glue (a whole MainWindow: also runs the constructor of 13i/13j) -------------------------------------------------------------

thread_local! {
    /// The `FakeNative` of the test on this UI thread (`Rc` is not `Send`, so it lives here instead of crossing into the `run` closures).
    static NATIVE_PROBE: RefCell<Option<Rc<FakeNative>>> = const { RefCell::new(None) };
}

fn probe_calls() -> Vec<NativeCall> {
    NATIVE_PROBE.with(|slot| slot.borrow().as_ref().expect("probe").calls())
}

fn options_with(seams: ShellWindowingSeams) -> BuildOptions {
    BuildOptions {
        options: MainWindowOptions {
            activate: false,
            prefer_warp: true,
            windowing: Some(seams),
            ..MainWindowOptions::default()
        },
        ..BuildOptions::default()
    }
}

/// Seams over fakes: the window never really moves; `native.window` decides what `refit` sees.
fn fake_seams(window: Option<ScreenRect>) -> ShellWindowingSeams {
    let native = Rc::new(FakeNative::new(Normal));
    native.window.set(window);
    NATIVE_PROBE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&native)));
    ShellWindowingSeams {
        native: Some(native),
        ops: Some(Rc::new(FakeOps::baseline())),
        ..ShellWindowingSeams::default()
    }
}

/// Waits until a `Render`-priority sentinel posted NOW has run (every earlier Render job has run).
fn drain_render(ui: &ui_thread::UiThread) {
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    run(ui, move || {
        dispatcher().post_with_priority(Box::new(move || flag.store(true, Ordering::SeqCst)), UiPriority::Render);
    });
    wait_until(ui, "the render-priority sentinel", move || done.load(Ordering::SeqCst));
}

// C#: TitleBar_IsMadeDarkWithTheDarkWindowCaptionColourBeforeTheWindowIsRevealed
#[test]
#[ignore = "native: needs desktop"]
fn title_bar_is_made_dark_before_the_window_is_revealed() {
    let ui = start_ui();
    let (calls, hwnd, visible) = run(&ui, || {
        let dwm = Rc::new(FakeDwm::new(|_| true));
        let seams = ShellWindowingSeams {
            dwm: Some(dwm.clone()),
            ..ShellWindowingSeams::default()
        };
        let b = build(options_with(seams));
        (dwm.calls(), b.shell.hwnd(), user32::is_window_visible(b.shell.hwnd()))
    });
    assert_eq!(calls, vec![(hwnd, 20, 1), (hwnd, 35, CAPTION_COLOR_BGR)]);
    assert!(!visible, "the window was never revealed");
    finish(ui);
}

// C#: F11_OnANormalWindow_... and F11_Twice_And_ExitWhenNotFullscreen_AreNoOps_... (real ops, hidden window)
#[test]
#[ignore = "native: needs desktop"]
fn f11_on_a_normal_window_covers_the_monitor_and_exit_restores_the_exact_rect() {
    let ui = start_ui();
    run(&ui, || {
        let b = build(BuildOptions::default());
        let hwnd = b.shell.hwnd();
        let before = window_rect(hwnd);
        let monitor = Win32FullscreenNative.try_get_monitor_rect(hwnd).expect("monitor");
        assert!(!b.window.apply_fullscreen(false), "exit when not fullscreen: no-op");
        assert!(b.window.apply_fullscreen(true));
        let now = window_rect(hwnd);
        assert_eq!(
            (now.left, now.top, now.right, now.bottom),
            (monitor.left, monitor.top, monitor.right, monitor.bottom)
        );
        let fullscreen = b.window.fullscreen().expect("registered");
        assert!(fullscreen.is_active());
        let bounds = fullscreen.restore_bounds();
        assert!(!b.window.apply_fullscreen(true), "twice: no-op");
        assert_eq!(
            fullscreen.restore_bounds(),
            bounds,
            "the state before the first F11 is kept"
        );
        assert!(b.window.apply_fullscreen(false));
        let after = window_rect(hwnd);
        assert_eq!(
            (after.left, after.top, after.right, after.bottom),
            (before.left, before.top, before.right, before.bottom)
        );
        assert!(!user32::is_window_visible(hwnd), "never shown");
    });
    finish(ui);
}

// C#: F11Transition_IsOneResizeStep_NoIntermediateClientSize
#[test]
#[ignore = "native: needs desktop"]
fn f11_transition_is_one_resize_step() {
    let ui = start_ui();
    run(&ui, || {
        let b = build(BuildOptions::default());
        let sizes = Rc::new(Cell::new(0u32));
        let counter = Rc::clone(&sizes);
        b.shell.subscribe(
            ShellWindowEvent::ClientSizeChanged,
            Box::new(move || counter.set(counter.get() + 1)),
        );
        assert!(b.window.apply_fullscreen(true));
        assert_eq!(sizes.get(), 1, "enter: the client size changes exactly once");
        sizes.set(0);
        assert!(b.window.apply_fullscreen(false));
        assert_eq!(sizes.get(), 1, "exit: the client size changes exactly once");
    });
    finish(ui);
}

// The viewer's IsFullscreen reaches the window through apply_changes (step 5): the F11 key path of the view model.
#[test]
#[ignore = "native: needs desktop"]
fn viewer_fullscreen_change_applies_f11_through_apply_changes() {
    let ui = start_ui();
    run(&ui, || {
        let b = build(BuildOptions::default());
        let changes = b.window.view_model().borrow_mut().toggle_fullscreen();
        b.window.apply_changes(&changes);
        assert!(b.window.fullscreen().expect("registered").is_active());
        let changes = b.window.view_model().borrow_mut().exit_fullscreen();
        b.window.apply_changes(&changes);
        assert!(!b.window.fullscreen().expect("registered").is_active());
    });
    finish(ui);
}

// C#: DisplayChangeWhileNotFullscreen_DoesNotMoveTheWindow
#[test]
#[ignore = "native: needs desktop"]
fn display_change_while_not_fullscreen_does_not_move_the_window() {
    let ui = start_ui();
    let hwnd = run(&ui, || {
        let b = build(options_with(fake_seams(Some(WINDOW))));
        b.shell.hwnd()
    });
    run(&ui, move || {
        send(hwnd, WM_DISPLAYCHANGE, 32, 0);
    });
    drain_render(&ui);
    assert!(run(&ui, probe_calls).is_empty());
    finish(ui);
}

// C#: MonitorVanishesWhileFullscreen_TheWindowIsRefittedOntoTheRemainingMonitor (WM_DISPLAYCHANGE posts ONE refit at Render priority)
#[test]
#[ignore = "native: needs desktop"]
fn display_change_in_f11_posts_one_render_priority_refit() {
    let ui = start_ui();
    let hwnd = run(&ui, || {
        let b = build(options_with(fake_seams(Some(WINDOW)))); // the window no longer covers the monitor
        assert!(b.window.apply_fullscreen(true));
        b.shell.hwnd()
    });
    run(&ui, move || {
        send(hwnd, WM_DISPLAYCHANGE, 32, 0);
        assert!(probe_calls().is_empty(), "posted, not inline");
    });
    drain_render(&ui);
    assert_eq!(
        run(&ui, probe_calls),
        vec![NativeCall::MoveTo(MONITOR)],
        "ONE refit onto the monitor"
    );
    finish(ui);
}

// T7: the window closes between the post and the run: the job does nothing and does not panic.
#[test]
#[ignore = "native: needs desktop"]
fn display_change_job_after_the_window_closed_does_nothing() {
    let ui = start_ui();
    let hwnd = run(&ui, || {
        let b = build(options_with(fake_seams(Some(WINDOW))));
        assert!(b.window.apply_fullscreen(true));
        b.shell.hwnd()
    });
    run(&ui, move || {
        send(hwnd, WM_DISPLAYCHANGE, 32, 0);
        built().shell.close();
    });
    drain_render(&ui);
    assert!(run(&ui, probe_calls).is_empty());
    finish(ui);
}

// C#: DpiChangedWhileFullscreen_TheWindowCoversTheMonitorAgainAndTheViewerScaleFollows
#[test]
#[ignore = "native: needs desktop"]
fn dpi_changed_while_fullscreen_covers_the_monitor_again_and_the_viewer_scale_follows() {
    let ui = start_ui();
    run(&ui, || {
        let b = build(options_with(fake_seams(Some(WINDOW))));
        let hwnd = b.shell.hwnd();
        assert!(b.window.apply_fullscreen(true));
        send_dpi_changed(hwnd, 144, window_rect(hwnd));
        assert_eq!(
            probe_calls(),
            vec![NativeCall::MoveTo(MONITOR)],
            "refit before the core reads the client size"
        );
        let scale = b.window.view_model().borrow().viewer().borrow().dpi_scale();
        assert!((scale - 1.5).abs() < 1e-9, "{scale}");
    });
    finish(ui);
}

// C#: ExitFullscreen_AfterTheOriginalMonitorWasUnplugged_PutsTheNormalWindowOntoARemainingWorkArea (seam `monitor_layout`)
#[test]
#[ignore = "native: needs desktop"]
fn exit_fullscreen_after_the_original_monitor_was_unplugged_uses_a_remaining_work_area() {
    let ui = start_ui();
    run(&ui, || {
        let ops = Rc::new(FakeOps::baseline());
        let remaining = ScreenRect::new(3000, 0, 4920, 1040);
        let seams = ShellWindowingSeams {
            ops: Some(ops.clone()),
            native: Some(Rc::new(FakeNative::new(Normal))),
            monitor_layout: Some(Arc::new(FixedMonitorLayout::new(vec![remaining]))),
            ..ShellWindowingSeams::default()
        };
        let b = build(options_with(seams));
        assert!(b.window.apply_fullscreen(true));
        ops.log.borrow_mut().clear();
        assert!(b.window.apply_fullscreen(false));
        let last = ops.ops().into_iter().rev().find_map(|op| match op {
            Op::SetPos(rect, _) => Some(rect),
            _ => None,
        });
        let rect = last.expect("one SetWindowPos on exit");
        assert!(
            rect.left >= remaining.left && rect.right <= remaining.right,
            "{rect:?} inside {remaining:?}"
        );
    });
    finish(ui);
}

// The window disposes cleanly: reset on dispose, no refit afterwards.
#[test]
#[ignore = "native: needs desktop"]
fn dispose_resets_the_fullscreen_state() {
    let ui = start_ui();
    run(&ui, || {
        let b = build(BuildOptions::default());
        let fullscreen = b.window.fullscreen().expect("registered");
        assert!(b.window.apply_fullscreen(true));
        assert!(fullscreen.is_active());
        drop(b);
        drop(take_built()); // the window is disposed when its owner is dropped
        assert!(!fullscreen.is_active(), "reset by dispose_windowing");
        assert_eq!(fullscreen.restore_state(), None);
    });
    finish(ui);
}
