//! Attended Win32 checks for W8-15f. Each window lives and is destroyed on its own UI thread.

#[path = "common/overlay_fakes.rs"]
mod overlay_fakes;

use std::cell::Cell;
use std::ffi::c_void;
use std::rc::Rc;

use overlay_fakes::{FakeOverlayOs, RecordingInput};
use pr_platform_win::host::contracts::{ShellWindowApi, WindowMessage, WindowMessageHandler};
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::frame_clock::timer::TimerFrameClock;
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::shell_overlay::messages::OverlayMessageHandler;
use pr_platform_win::shell_overlay::native::{OverlayOs, VK_SHIFT, WM_MOUSELEAVE, Win32OverlayOs, key_state_is_down};
use pr_platform_win::win32::messages::{WM_LBUTTONDOWN, WM_LBUTTONUP};
use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage,
};

fn on_ui_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f).join().unwrap();
}

fn hidden_window() -> ShellWindow {
    let clock: Rc<dyn HostFrameClock> = Rc::new(TimerFrameClock::create(None, 0).unwrap());
    ShellWindow::create(ShellWindowOptions::default(), clock).unwrap()
}

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

struct CountHandler(Rc<Cell<usize>>);

impl WindowMessageHandler for CountHandler {
    fn try_handle(&self, message: &WindowMessage) -> Option<isize> {
        if message.msg == WM_LBUTTONDOWN || message.msg == WM_MOUSELEAVE {
            self.0.set(self.0.get() + 1);
        }
        None
    }
}

// new: client origin is exactly the native ClientToScreen result for a hidden shell window
#[test]
#[ignore = "native: needs desktop"]
fn client_origin_on_screen_matches_client_to_screen_of_a_hidden_window() {
    on_ui_thread(|| {
        let window = hidden_window();
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: point is a live local; the hidden window handle belongs to this UI thread.
        assert!(unsafe { ClientToScreen(hwnd(window.hwnd()), &mut point) }.as_bool());
        assert_eq!(
            Win32OverlayOs.client_origin_on_screen(window.hwnd()),
            (point.x, point.y)
        );
        window.dispose();
    });
}

// new: a stale HWND has the C# zero-origin fallback
#[test]
#[ignore = "native: needs desktop"]
fn client_origin_of_a_destroyed_window_is_zero_zero() {
    on_ui_thread(|| {
        let window = hidden_window();
        let handle = window.hwnd();
        window.dispose();
        assert_eq!(Win32OverlayOs.client_origin_on_screen(handle), (0, 0));
    });
}

// new: a one-shot TME_LEAVE on a hidden window produces WM_MOUSELEAVE without moving the cursor
#[test]
#[ignore = "native: needs desktop"]
fn request_mouse_leave_posts_wm_mouseleave_when_the_cursor_is_elsewhere() {
    on_ui_thread(|| {
        let window = hidden_window();
        let seen = Rc::new(Cell::new(0));
        window.add_message_handler(Rc::new(CountHandler(seen.clone())));
        Win32OverlayOs.request_mouse_leave(window.hwnd());
        // Bounded, immediate queue drain: no sleep or polling for elapsed time.
        for _ in 0..128 {
            let mut msg = MSG::default();
            // SAFETY: msg is a live local and PM_REMOVE removes one message from this thread's queue.
            let present = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool();
            if !present {
                break;
            }
            // SAFETY: msg was returned by PeekMessageW and is valid for the synchronous dispatch.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        assert!(seen.get() > 0, "hidden window did not receive WM_MOUSELEAVE");
        window.dispose();
    });
}

// new: Shift state reads the same high bit as the C# GetKeyState helper
#[test]
#[ignore = "native: needs desktop"]
fn is_key_down_reads_the_shift_state_without_throwing() {
    on_ui_thread(|| {
        let expected = key_state_is_down(pr_platform_win::win32::settings_native::get_key_state(VK_SHIFT));
        assert_eq!(Win32OverlayOs.is_key_down(VK_SHIFT), expected);
    });
}

// new: the first handler owns an accepted press before later handlers see it
#[test]
#[ignore = "native: needs desktop"]
fn real_shell_window_with_the_handler_first_swallows_a_press_over_the_overlay() {
    on_ui_thread(|| {
        let window = Rc::new(hidden_window());
        let input = Rc::new(RecordingInput::default());
        input.accept_down.set(true);
        input.accept_up.set(true);
        let later = Rc::new(Cell::new(0));
        window.add_message_handler(Rc::new(CountHandler(later.clone())));
        let handler = Rc::new(OverlayMessageHandler::new(
            input.clone(),
            window.clone(),
            None,
            Rc::new(FakeOverlayOs::default()),
        ));
        window.insert_message_handler_first(handler);
        let scale = window.dpi_scale();
        let x = (12.0 * scale).round() as i16;
        let y = (18.0 * scale).round() as i16;
        let lp = ((y as u16 as isize) << 16) | (x as u16 as isize);
        // SAFETY: the HWND is a live hidden window on this thread; these messages carry only integer coordinates.
        unsafe {
            SendMessageW(hwnd(window.hwnd()), WM_LBUTTONDOWN, None, Some(LPARAM(lp)));
        }
        assert_eq!(later.get(), 0);
        // SAFETY: the matching release carries only integer coordinates and completes before window disposal.
        unsafe {
            SendMessageW(hwnd(window.hwnd()), WM_LBUTTONUP, None, Some(LPARAM(lp)));
        }
        let points = input.points.borrow();
        assert_eq!(points.len(), 2);
        assert!((points[0].x - 12.0).abs() <= 1.0 / scale);
        assert!((points[0].y - 18.0).abs() <= 1.0 / scale);
        assert_eq!(points[0], points[1]);
        window.dispose();
    });
}
