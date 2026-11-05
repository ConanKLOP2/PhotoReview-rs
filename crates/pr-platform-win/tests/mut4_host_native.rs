//! MUT4 (2026-11-06): killers for the survivors of `host/native.rs` (`HWND_MESSAGE`, `get_capture`, `validate_whole_client`,
//! `window_from_point`). A tiny `BUTTON` popup window of this test thread stands in for the OS object; nothing outside it is touched.
//! The tests that need a window are `#[ignore = "native: ..."]` (a window station; the visible one needs an unlocked desktop);
//! run them with `-- --include-ignored --test-threads=1`.
#![cfg(windows)]

use pr_platform_win::host::native::{HWND_MESSAGE, get_capture, validate_whole_client, window_from_point};
use pr_platform_win::win32::structs::Point;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{GetUpdateRect, InvalidateRect};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SW_SHOWNOACTIVATE, ShowWindow, WINDOW_EX_STYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

/// A `BUTTON` popup, destroyed on drop. `visible` windows are topmost and never activated.
struct TestWindow(isize);

impl TestWindow {
    fn new(visible: bool) -> Self {
        let (style, ex) = if visible {
            (WS_POPUP, WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW)
        } else {
            (WS_POPUP, WINDOW_EX_STYLE(0))
        };
        // SAFETY: the predefined `BUTTON` class (a `STATIC` answers HTTRANSPARENT: `WindowFromPoint` skips it) with no parent, menu, instance or creation data; created and destroyed on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                ex,
                w!("BUTTON"),
                w!("mut4"),
                style,
                200,
                200,
                64,
                64,
                None,
                None,
                None,
                None,
            )
        }
        .expect("create the BUTTON popup");
        if visible {
            // SAFETY: the window was just created on this thread.
            let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        }
        Self(hwnd.0 as isize)
    }

    fn has_update_region(&self) -> bool {
        // SAFETY: the window is alive; a null rectangle only asks whether an update region exists.
        unsafe { GetUpdateRect(self.hwnd(), None, false) }.as_bool()
    }

    fn hwnd(&self) -> HWND {
        HWND(self.0 as *mut _)
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: the window was created by `new` on this thread.
        let _ = unsafe { DestroyWindow(self.hwnd()) };
    }
}

// native.rs:27:32 `-3` -> `3`: HWND_MESSAGE is the C# `HwndMessage` (-3), the parent value of a message-only window. The value is a single constant,
// so the assertion covers the whole domain.
#[test]
fn hwnd_message_is_minus_three() {
    assert_eq!(HWND_MESSAGE, -3);
}

// native.rs:56:5 `validate_whole_client -> true`: a handle that is not a window is reported as failure by the OS.
#[test]
fn validate_whole_client_of_a_bad_handle_is_false() {
    assert!(!validate_whole_client(0x0FFF_FFFE));
}

// native.rs:56:5 `validate_whole_client -> false`, and the call itself: a real window with a pending update region is validated.
#[test]
#[ignore = "native: needs a window station"]
fn validate_whole_client_clears_the_update_region_and_reports_success() {
    let window = TestWindow::new(true);
    // SAFETY: the window is alive; a null rectangle invalidates the whole client area.
    assert!(unsafe { InvalidateRect(Some(window.hwnd()), None, false) }.as_bool());
    assert!(window.has_update_region(), "the window starts invalid");

    assert!(validate_whole_client(window.0));

    assert!(!window.has_update_region(), "validated");
}

// native.rs:50:5 `get_capture -> 0`: with the mouse captured by our window, the function returns that window; without capture, 0.
#[test]
#[ignore = "native: needs a window station"]
fn get_capture_returns_the_capturing_window_else_zero() {
    let window = TestWindow::new(false);
    // SAFETY: releasing capture of this thread has no arguments.
    let _ = unsafe { ReleaseCapture() };
    assert_eq!(get_capture(), 0);

    // SAFETY: the window is alive and belongs to this thread.
    let _ = unsafe { SetCapture(window.hwnd()) };
    let seen = get_capture();
    // SAFETY: as above.
    let _ = unsafe { ReleaseCapture() };
    assert_eq!(seen, window.0);
    assert_eq!(get_capture(), 0);
}

// native.rs:62:5 `window_from_point -> 0`: a visible topmost popup at (200,200) 64x64 is the window under (232,232).
#[test]
#[ignore = "native: needs an unlocked desktop"]
fn window_from_point_returns_the_visible_window_under_the_point() {
    let window = TestWindow::new(true);
    assert_eq!(window_from_point(Point { x: 232, y: 232 }), window.0);
}
