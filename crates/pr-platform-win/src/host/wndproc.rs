//! Port of `PhotoReview.Shell.Win32/Hosting/WndProcThunk.cs` (104 lines, baseline `d92d6aae`). Unit W8-13e.
//!
//! The native entry point of every shell window: `extern "system" fn` (no delegate marshalling), the `GWLP_USERDATA` trampoline and the FFI panic guard.
//! - **Trampoline.** `CreateWindowEx` gets a cookie in `lpCreateParams` ([`make_cookie`]: a thin pointer to a `Box<Rc<dyn WindowTarget>>`; C# used a
//!   `GCHandle`). `WM_NCCREATE` stores it in `GWLP_USERDATA`; every later message finds the target through it; `WM_NCDESTROY` clears it and tells the target
//!   the native window is gone ([`WindowTarget::on_native_destroyed`]), which frees the cookie. A message that arrives with no cookie (before
//!   `WM_NCCREATE`: `WM_GETMINMAXINFO`, `WM_NCCREATE` itself for a failed creation) goes to `DefWindowProc`.
//! - **Panic guard.** A Rust panic must not unwind out of this function (the process would abort without a report). The body runs under
//!   `pending_panic::guarded(LRESULT(0), ..)`: the panic is captured, the OS gets 0 (no default processing after an error: `DefWindowProc(WM_CLOSE)`
//!   would destroy a window whose `Closing` handler had not decided yet; 0 for `WM_NCCREATE` cancels the creation), and the owner resumes it after
//!   the native call returns (`pending_panic::rethrow_pending`: `MessageLoop` after `DispatchMessage`, `ShellWindow` after `CreateWindowEx`,
//!   `SendMessage`, `DestroyWindow`).
//!
//! The dispatcher's procedure is `ui_dispatcher::dispatcher_wnd_proc` (D-W8B3H-11).
//!
//! The procedure uses the `windows` parameter types, not the `isize` aliases of `win32::structs` (`WndProc`), so it fits `WNDCLASSEXW::lpfnWndProc`
//! without a cast; inside it converts to `Hwnd`/`Wparam`/`Lparam`.

#![allow(unused_variables, dead_code)] // W8-13e stub: the porter removes this line when the last `todo!()` is gone.

use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

use crate::win32::structs::{Hwnd, Lparam, Lresult, Wparam};

pub use crate::host::pending_panic::rethrow_pending;

/// What the trampoline calls (C# `ShellWindow.ProcessMessage` and `OnNativeDestroyed`). Implemented by `shell_window::create::ShellWindowInner`; the
/// tests of this file implement it with a recording fake, so the trampoline is tested without a shell window (D-W8B3H-12).
pub trait WindowTarget {
    /// C# `ProcessMessage(hwnd, message, wParam, lParam)`. Called for every message from `WM_NCCREATE` to `WM_NCDESTROY` inclusive, re-entrantly
    /// (a nested `SendMessage` from inside it comes back here), hence `&self`.
    fn process_message(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
    /// C# `OnNativeDestroyed()`: `WM_NCDESTROY` was processed; the native window no longer exists.
    fn on_native_destroyed(&self);
}

/// Boxes `target` and returns the thin pointer to pass as `lpCreateParams` (never 0). Pair with exactly one [`free_cookie`].
pub fn make_cookie(target: Rc<dyn WindowTarget>) -> isize {
    todo!("W8-13e")
}

/// Releases a cookie from [`make_cookie`] (C# `GCHandle.Free`).
///
/// # Safety
/// `cookie` came from `make_cookie` and has not been freed; no `shell_window_proc` call that read it is still running (the trampoline clones the
/// `Rc` before it calls the target, so a target may free its own cookie from `on_native_destroyed`).
pub unsafe fn free_cookie(cookie: isize) {
    todo!("W8-13e")
}

/// C# `WndProcThunk.ShellWindowWndProc`, registered in the `PhotoReviewRs.Shell.Window` class (see `create::ensure_class_registered`).
/// Algorithm (C# lines 43-82):
/// 1. `WM_NCCREATE`: `lparam` is a `CREATESTRUCTW*`; its `lpCreateParams` is the cookie. A zero cookie returns 0 (cancel creation). Otherwise
///    `SetWindowLongPtr(GWLP_USERDATA, cookie)`.
/// 2. other messages: `GetWindowLongPtr(GWLP_USERDATA)`; 0 -> `DefWindowProc`.
/// 3. `target.process_message(..)`; after `WM_NCDESTROY`: `SetWindowLongPtr(GWLP_USERDATA, 0)` then `target.on_native_destroyed()`.
/// 4. a panic anywhere above: captured, result 0.
///
/// # Safety
/// Only the OS calls it, as a `WNDPROC`: `hwnd` is a window of class `PhotoReviewRs.Shell.Window` created on this thread, and for `WM_NCCREATE`
/// `lparam` points to a valid `CREATESTRUCTW` for the duration of the call.
pub unsafe extern "system" fn shell_window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    todo!("W8-13e")
}
