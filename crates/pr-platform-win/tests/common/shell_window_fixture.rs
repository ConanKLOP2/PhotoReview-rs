//! Shared helpers of the shell-window tests (W8-13e creates it, W8-13f extends it): the Rust twin of the private helper methods and handler classes of
//! `tests/PhotoReview.Shell.Integration.Tests/Hosting/ShellWindowTests.cs` (baseline `d92d6aae`).
//!
//! A `ShellWindow` is `!Send`, so it lives in a thread-local of the UI thread ([`create_window`] / [`window`]) and tests reach it with
//! `ui.invoke(|| ...)`. The test crate that includes this file must also declare `#[path = "common/ui_thread.rs"] mod ui_thread;` at its root.
//! No window is ever shown or activated by these helpers; windows are destroyed by [`dispose_window`] (or by the thread-local destructor).
#![allow(dead_code)] // shared by several test crates, each of which uses a different part

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pr_platform_win::host::contracts::{ShellWindowApi, SizeD, WindowMessage, WindowMessageHandler};
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::host::frame_clock::timer::TimerFrameClock;
use pr_platform_win::host::native::MinMaxInfo;
use pr_platform_win::host::pending_panic::rethrow_pending;
use pr_platform_win::host::shell_window::{ShellWindow, ShellWindowOptions};
use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::dialog_native::user32 as dialog_user32;
use pr_platform_win::win32::messages::{WM_APP, WM_DPICHANGED, WM_GETMINMAXINFO, WS_OVERLAPPEDWINDOW};
use pr_platform_win::win32::structs::{Hwnd, Lresult, Point, Rect};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::ClientToScreen;

use crate::ui_thread::{BOUND, UiThread, ui_loop};

/// C# `TestMessage` = `WM_APP + 60`.
pub const TEST_MESSAGE: u32 = WM_APP + 60;

/// A log that handlers on the UI thread append to and the test thread reads.
pub type Log = Arc<Mutex<Vec<String>>>;

pub fn new_log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

pub fn log_snapshot(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

thread_local! {
    static WINDOW: RefCell<Option<Rc<ShellWindow>>> = const { RefCell::new(None) };
    static CLOCK: RefCell<Option<Rc<TimerFrameClock>>> = const { RefCell::new(None) };
}

/// C# `CreateWindow(ui, options)`: a `TimerFrameClock` registered with the loop of this UI thread and a window on it. UI thread only.
pub fn create_window(options: ShellWindowOptions) -> Rc<ShellWindow> {
    let clock = Rc::new(TimerFrameClock::create(None, 0).expect("timer clock"));
    ui_loop().add_frame_clock(Rc::clone(&clock) as Rc<dyn HostFrameClock>);
    let window =
        Rc::new(ShellWindow::create(options, Rc::clone(&clock) as Rc<dyn HostFrameClock>).expect("shell window"));
    CLOCK.with(|slot| *slot.borrow_mut() = Some(clock));
    WINDOW.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&window)));
    window
}

/// The window made by [`create_window`] on this UI thread.
pub fn window() -> Rc<ShellWindow> {
    WINDOW
        .with(|slot| slot.borrow().clone())
        .expect("create_window() first")
}

/// The clock of [`window`].
pub fn clock() -> Rc<TimerFrameClock> {
    CLOCK.with(|slot| slot.borrow().clone()).expect("create_window() first")
}

/// `window.dispose()` and forget it. UI thread only.
pub fn dispose_window() {
    if let Some(window) = WINDOW.with(|slot| slot.borrow_mut().take()) {
        window.dispose();
    }
}

/// C# `WaitUntilIdleAsync`: waits (bounded, no sleep) until the clock is not armed, that is no frame is waiting to be drawn. Each probe is a round
/// trip through the UI thread's loop, which keeps serving timers in between.
pub fn wait_until_idle(ui: &UiThread) {
    let deadline = Instant::now() + BOUND;
    while ui.invoke(|| clock().is_armed()) {
        assert!(Instant::now() < deadline, "frame clock stayed armed");
        std::thread::yield_now();
    }
}

/// `SendMessage` with integer parameters only (no pointers).
pub fn send(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> Lresult {
    // SAFETY: the callers pass integers that the message does not interpret as pointers; the window procedure runs synchronously on this thread.
    unsafe { dialog_user32::send_message(hwnd, message, wparam, lparam) }
}

/// Calls the window procedure of the shell window class directly (what the OS would do). `SendMessage` itself refuses some malformed messages (a
/// `WM_DPICHANGED` with a null `lParam` never reaches the procedure), so the null-pointer guards are exercised through this entry point.
pub fn call_window_proc(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> Lresult {
    // SAFETY: called on the thread that owns `hwnd`; the tests pass null or integer `lParam`s only, never a dangling pointer.
    unsafe {
        pr_platform_win::host::wndproc::shell_window_proc(
            HWND(hwnd as *mut core::ffi::c_void),
            message,
            windows::Win32::Foundation::WPARAM(wparam),
            windows::Win32::Foundation::LPARAM(lparam),
        )
    }
    .0
}

/// C# `SendDpiChanged`: `wParam = (dpi << 16) | dpi`, `lParam = &rect`; a panic captured in the window procedure reaches the caller.
pub fn send_dpi_changed(hwnd: Hwnd, dpi: i32, suggested: Rect) {
    let rect = suggested;
    // SAFETY: `lParam` points to a live local RECT for the duration of the synchronous call, as WM_DPICHANGED defines.
    let _result = unsafe {
        dialog_user32::send_message(
            hwnd,
            WM_DPICHANGED,
            ((dpi << 16) | dpi) as usize,
            &rect as *const Rect as isize,
        )
    };
    rethrow_pending();
}

/// C# `SendGetMinMaxInfo`.
pub fn send_get_min_max_info(hwnd: Hwnd) -> Point {
    let mut info = MinMaxInfo::default();
    // SAFETY: `lParam` points to a live local MINMAXINFO for the duration of the synchronous call, as WM_GETMINMAXINFO defines.
    let _result =
        unsafe { dialog_user32::send_message(hwnd, WM_GETMINMAXINFO, 0, &mut info as *mut MinMaxInfo as isize) };
    info.ptMinTrackSize
}

/// C# `ExpectedFrameSize`: the client size grown by the overlapped-window frame at `dpi`.
pub fn expected_frame_size(client_width: i32, client_height: i32, dpi: u32) -> Point {
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: client_width,
        bottom: client_height,
    };
    user32::adjust_window_rect_ex_for_dpi(&mut rect, WS_OVERLAPPEDWINDOW, false, 0, dpi)
        .expect("AdjustWindowRectExForDpi");
    Point {
        x: rect.right - rect.left,
        y: rect.bottom - rect.top,
    }
}

/// C# `WindowRect`.
pub fn window_rect(hwnd: Hwnd) -> Rect {
    user32::get_window_rect(hwnd).expect("GetWindowRect")
}

/// C# `ClientOrigin`: the client (0, 0) in screen coordinates.
pub fn client_origin(hwnd: Hwnd) -> Point {
    let mut point = Point::default();
    // SAFETY: `point` is a live local POINT that the OS updates in place.
    assert!(unsafe { ClientToScreen(HWND(hwnd as *mut core::ffi::c_void), &mut point) }.as_bool());
    point
}

/// C# `WindowText`.
pub fn window_text(hwnd: Hwnd) -> String {
    dialog_user32::get_window_text(hwnd)
}

/// C# `Handler(name, calls, handles, result)`: answers only [`TEST_MESSAGE`]; logs its name; handles it or not.
pub struct Handler {
    pub name: &'static str,
    pub calls: Log,
    pub handles: bool,
    pub result: Lresult,
}

impl WindowMessageHandler for Handler {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        if message.msg != TEST_MESSAGE {
            return None;
        }
        self.calls.lock().unwrap().push(self.name.to_owned());
        self.handles.then_some(self.result)
    }
}

/// (client pixels of the window, pixels in the `lParam`) at one `WM_SIZE`.
pub type PixelPair = ((i32, i32), (i32, i32));

/// C# `SizeProbe`: records the window's client size (DIP and pixels) and the size in the `lParam` at every `WM_SIZE`; handles nothing.
pub struct SizeProbe {
    pub window: Weak<ShellWindow>,
    pub seen: Arc<Mutex<Vec<SizeD>>>,
    pub seen_pixels: Arc<Mutex<Vec<PixelPair>>>,
}

impl WindowMessageHandler for SizeProbe {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        if message.msg == pr_platform_win::win32::messages::WM_SIZE {
            if let Some(window) = self.window.upgrade() {
                self.seen.lock().unwrap().push(window.client_size_dip());
                let from_message = (
                    pr_platform_win::host::shell_window::dpi::low_word(message.lparam),
                    pr_platform_win::host::shell_window::dpi::high_word(message.lparam),
                );
                self.seen_pixels
                    .lock()
                    .unwrap()
                    .push((window.client_size_pixels(), from_message));
            }
        }
        None
    }
}

/// C# `ThrowingHandler`: panics with `"from WndProc"` at [`TEST_MESSAGE`].
pub struct ThrowingHandler;

impl WindowMessageHandler for ThrowingHandler {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        if message.msg == TEST_MESSAGE {
            panic!("from WndProc");
        }
        None
    }
}

/// The text of a panic payload (`&str` or `String`).
pub fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-text panic payload>".to_owned())
}
