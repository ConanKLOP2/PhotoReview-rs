//! G18 (docs/reviews/pr-164.md L4): the native startup seams against a real window. The older `shell_startup_native.rs` tests only call the
//! functions with handle 0. DWM and placement round trips need a desktop session and are `#[ignore]`d with a reason; the timer-handle tests
//! need no desktop and assert without a fixed delay (a dropped job closes its channel, so the result is `Disconnected`, not a timeout).
#![cfg(windows)]

use pr_app::windowing::contracts::WindowPlacementData;
use pr_platform_win::shell_startup::cloak::{schedule_on_thread, try_set_cloaked};
use pr_platform_win::shell_startup::placement::{NativeWindowPlacement, WindowPlacementNative};
use std::sync::mpsc;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, IsWindowVisible, SW_SHOWNOACTIVATE, ShowWindow, WS_EX_TOOLWINDOW,
    WS_OVERLAPPEDWINDOW,
};
use windows::core::w;

/// A top-level tool window (screen coordinates in `rcNormalPosition`), destroyed on drop.
struct TestWindow(isize);

impl TestWindow {
    fn new() -> Self {
        // SAFETY: a predefined `STATIC` class, no parent, menu, instance or creation data; created and destroyed on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!("g18"),
                WS_OVERLAPPEDWINDOW,
                100,
                120,
                300,
                200,
                None,
                None,
                None,
                None,
            )
        }
        .expect("CreateWindowExW");
        Self(hwnd.0 as isize)
    }

    fn raw(&self) -> HWND {
        HWND(self.0 as *mut _)
    }

    fn cloaked(&self) -> u32 {
        let mut value = 0u32;
        // SAFETY: `value` is a live 4-byte buffer and the size passed is 4; the window is alive.
        unsafe {
            DwmGetWindowAttribute(
                self.raw(),
                DWMWINDOWATTRIBUTE(14), // DWMWA_CLOAKED
                (&mut value as *mut u32).cast(),
                4,
            )
        }
        .expect("DwmGetWindowAttribute");
        value
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: the window was created by `new` on this thread.
        let _ = unsafe { DestroyWindow(self.raw()) };
    }
}

fn data(normal: (i32, i32, i32, i32)) -> WindowPlacementData {
    WindowPlacementData {
        length: 0,
        flags: 0,
        show_command: 3,
        min_x: -1,
        min_y: -1,
        max_x: -1,
        max_y: -1,
        normal_left: normal.0,
        normal_top: normal.1,
        normal_right: normal.2,
        normal_bottom: normal.3,
    }
}

#[test]
#[ignore = "native: needs an interactive desktop with DWM"]
fn try_set_cloaked_cloaks_and_uncloaks_a_real_window() {
    let window = TestWindow::new();
    // SAFETY: the window is alive; the return value is the previous visibility.
    let _ = unsafe { ShowWindow(window.raw(), SW_SHOWNOACTIVATE) };
    assert_eq!(window.cloaked(), 0, "a fresh window is not cloaked");
    assert!(try_set_cloaked(window.0, true));
    assert_ne!(window.cloaked(), 0, "DWMWA_CLOAKED reports the cloak");
    assert!(try_set_cloaked(window.0, false));
    assert_eq!(window.cloaked(), 0, "uncloaked again");
}

#[test]
#[ignore = "native: needs an interactive desktop with DWM"]
fn try_set_cloaked_is_false_for_a_destroyed_window() {
    let window = TestWindow::new();
    let hwnd = window.0;
    drop(window);
    assert!(!try_set_cloaked(hwnd, true));
}

#[test]
#[ignore = "native: needs an interactive desktop"]
fn native_placement_set_hidden_then_get_round_trips_the_normal_rect() {
    let window = TestWindow::new();
    let native = NativeWindowPlacement;
    assert!(native.set_hidden(window.0, &data((210, 230, 810, 630))));
    let got = native.try_get(window.0).expect("GetWindowPlacement");
    assert_eq!(
        (got.normal_left, got.normal_top, got.normal_right, got.normal_bottom),
        (210, 230, 810, 630)
    );
    // GetWindowPlacement reports a hidden window that is neither minimized nor maximized as SW_SHOWNORMAL; the 3 of the input is not applied.
    assert_eq!(got.show_command, 1);
    // SAFETY: the window is alive.
    let visible = unsafe { IsWindowVisible(window.raw()) }.as_bool();
    assert!(!visible, "set_hidden must not show the window");
    assert!(got.length > 0);
}

#[test]
#[ignore = "native: needs an interactive desktop"]
fn native_placement_of_a_destroyed_window_is_none_and_set_hidden_fails() {
    let window = TestWindow::new();
    let hwnd = window.0;
    drop(window);
    let native = NativeWindowPlacement;
    assert!(native.try_get(hwnd).is_none());
    assert!(!native.set_hidden(hwnd, &data((0, 0, 10, 10))));
}

#[test]
fn schedule_on_thread_runs_the_job_once_after_the_delay() {
    let (tx, rx) = mpsc::channel();
    let _handle = schedule_on_thread(
        Duration::from_millis(1),
        Box::new(move || {
            let _ = tx.send(());
        }),
    );
    assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(()));
    // The job ran once and was dropped with its sender: no second message is possible.
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}

#[test]
fn dropping_the_handle_cancels_the_job_without_a_fixed_wait() {
    let (tx, rx) = mpsc::channel::<()>();
    let handle = schedule_on_thread(
        Duration::from_secs(3600),
        Box::new(move || {
            let _ = tx.send(());
        }),
    );
    drop(handle);
    // The cancelled thread ends and drops the job (and `tx` with it): Disconnected, never a message, and no wait for the delay.
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}
