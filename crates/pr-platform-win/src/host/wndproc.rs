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

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;

use crate::host::pending_panic::guarded;
use crate::win32::api::user32;
use crate::win32::messages::{GWLP_USERDATA, WM_NCCREATE, WM_NCDESTROY};
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
    // A `Box<Rc<dyn ..>>` is a thin pointer (the `Rc<dyn>` itself is the fat one), so it fits `lpCreateParams` / `GWLP_USERDATA`.
    Box::into_raw(Box::new(target)) as isize
}

/// Releases a cookie from [`make_cookie`] (C# `GCHandle.Free`).
///
/// # Safety
/// `cookie` came from `make_cookie` and has not been freed; no `shell_window_proc` call that read it is still running (the trampoline clones the
/// `Rc` before it calls the target, so a target may free its own cookie from `on_native_destroyed`).
pub unsafe fn free_cookie(cookie: isize) {
    if cookie == 0 {
        return;
    }
    // SAFETY: per the `# Safety` contract the cookie came from `make_cookie` (`Box::into_raw` of a `Box<Rc<dyn WindowTarget>>`) and is freed once.
    drop(unsafe { Box::from_raw(cookie as *mut Rc<dyn WindowTarget>) });
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
    // Nothing may unwind out of this frame: a panic is captured, the OS gets 0 and the owner resumes it (D-W8B3H-4).
    guarded(
        LRESULT(0),
        AssertUnwindSafe(|| {
            let handle = hwnd.0 as Hwnd;
            let (wparam, lparam) = (wparam.0 as Wparam, lparam.0 as Lparam);
            if message == WM_NCCREATE {
                match creation_cookie(lparam) {
                    // No owner: returning 0 from WM_NCCREATE makes CreateWindowEx fail.
                    0 => return LRESULT(0),
                    cookie => {
                        // SAFETY: the cookie is the value our `make_cookie` produced; the slot is cleared in WM_NCDESTROY before the cookie can be freed.
                        let _previous = unsafe { user32::set_window_long_ptr(handle, GWLP_USERDATA, cookie) };
                    }
                }
            }
            let cookie = user32::get_window_long_ptr(handle, GWLP_USERDATA);
            if cookie == 0 {
                // Before WM_NCCREATE (WM_GETMINMAXINFO...) and after WM_NCDESTROY.
                // SAFETY: the parameters are the ones this procedure received.
                return LRESULT(unsafe { user32::def_window_proc(handle, message, wparam, lparam) });
            }
            // SAFETY: a non-zero slot holds a live `Box<Rc<dyn WindowTarget>>` (see above); clone the `Rc` first, the target may free its own cookie.
            let target: Rc<dyn WindowTarget> = unsafe { (*(cookie as *const Rc<dyn WindowTarget>)).clone() };
            if message != WM_NCDESTROY {
                return LRESULT(target.process_message(handle, message, wparam, lparam));
            }
            // The window is gone after this message: clear the slot and notify even when processing panicked.
            let result = catch_unwind(AssertUnwindSafe(|| {
                target.process_message(handle, message, wparam, lparam)
            }));
            // SAFETY: clearing the slot (0) is what the procedure expects; the window is being destroyed.
            let _previous = unsafe { user32::set_window_long_ptr(handle, GWLP_USERDATA, 0) };
            target.on_native_destroyed();
            match result {
                Ok(value) => LRESULT(value),
                Err(payload) => resume_unwind(payload),
            }
        }),
    )
}

/// `CREATESTRUCTW::lpCreateParams` of a `WM_NCCREATE` `lparam`; `0` for a null or misaligned pointer.
fn creation_cookie(lparam: Lparam) -> isize {
    if lparam == 0 || lparam as usize % align_of::<CREATESTRUCTW>() != 0 {
        return 0;
    }
    // SAFETY: for WM_NCCREATE `lparam` points to a CREATESTRUCTW that is valid for the duration of the call (Win32 contract); null and alignment checked above.
    unsafe { (*(lparam as *const CREATESTRUCTW)).lpCreateParams as isize }
}
