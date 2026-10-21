//! Port of `PhotoReview.Shell.Win32/Hosting/ShellWindow.cs` `ProcessMessage` (lines 251-314) and the helpers it alone calls (`UpdateClientSize`,
//! `RaiseClientSizeChangedIfNeeded`, `UpdateActive`, `RaiseClosing`; baseline `d92d6aae`). Unit W8-13f.
//!
//! The order inside [`ShellWindowInner::process_message`] is the contract:
//! 1. lifecycle messages (`WM_NCCREATE`, `WM_CREATE`, `WM_DESTROY`, `WM_NCDESTROY`) are handled first and never reach the message handlers;
//! 2. the window updates its OWN state for `WM_SIZE`, `WM_DPICHANGED`, `WM_ACTIVATE` (so handlers see the new client size / dpi / active flag);
//! 3. the registered handlers run in registration order; the first `Some(result)` wins and ends the processing;
//! 4. the defaults: `WM_SIZE` invalidate + 0; `WM_DPICHANGED` 0; `WM_GETMINMAXINFO` set the minimum track size + 0; `WM_CLOSE` raise `Closing`
//!    (destroy unless cancelled) + 0; `WM_SETCURSOR` with hit-test `HTCLIENT` set our cursor + 1; `WM_ERASEBKGND` 1 (no white flash); `WM_PAINT`
//!    validate + invalidate + 0; anything else `DefWindowProc`.
//!
//! 13e and 13f were ported together (D-W8B3H-13 allows it): this file holds the final `process_message`, not the lifecycle-only first version.

use super::create::{LIST_EVENT, ShellWindowInner, apply_cursor_handle, cursor_handle};
use super::dpi::{high_word, low_word};
use crate::host::contracts::{CancelEventArgs, ClosingHandler, ShellWindowEvent, WindowEventHandler, WindowMessage};
use crate::host::icons;
use crate::host::native::{HTCLIENT, MinMaxInfo, WA_INACTIVE, validate_whole_client};
use crate::win32::api::user32;
use crate::win32::messages::{
    WM_ACTIVATE, WM_CLOSE, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_NCCREATE,
    WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_SIZE,
};
use crate::win32::structs::{Hwnd, Lparam, Lresult, Rect, Wparam};

/// The pointer in the `lparam` of `WM_DPICHANGED` / `WM_GETMINMAXINFO`; a null or misaligned value is "no pointer" (D-W8B3H-19).
fn aligned_pointer<T>(lparam: Lparam) -> Option<*mut T> {
    (lparam != 0 && lparam as usize % align_of::<T>() == 0).then_some(lparam as *mut T)
}

impl ShellWindowInner {
    /// C# `ProcessMessage(hwnd, message, wParam, lParam)`. See the module doc for the order.
    pub(super) fn process_message(&self, hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        // 1. Lifecycle: never reaches the handlers.
        match message {
            WM_NCCREATE => return self.on_nc_create(hwnd, message, wparam, lparam),
            WM_CREATE => return self.on_create(hwnd),
            WM_DESTROY => {
                self.on_destroy();
                return 0;
            }
            // SAFETY: the parameters are the ones the window procedure received for this message.
            WM_NCDESTROY => return unsafe { user32::def_window_proc(hwnd, message, wparam, lparam) },
            // 2. Own state first, so handlers see the new size / dpi / active flag.
            WM_SIZE => self.update_client_size(low_word(lparam), high_word(lparam)),
            WM_DPICHANGED => {
                let suggested = aligned_pointer::<Rect>(lparam).map(|pointer| {
                    // SAFETY: for WM_DPICHANGED `lparam` points to a RECT that is valid for the duration of the message (Win32 contract); non-null and
                    // aligned checked above; the value is copied out at once and the pointer is not kept.
                    unsafe { *pointer }
                });
                self.apply_dpi_change(low_word(wparam as isize), suggested);
                icons::apply(hwnd, self.dpi.get()); // taskbar and title bar icons at the new dpi
            }
            WM_ACTIVATE => self.update_active(low_word(wparam as isize) != WA_INACTIVE),
            _ => {}
        }

        // 3. The handler chain, in registration order; the list is cloned so a handler may add handlers (seen from the next message).
        let handlers = self.handlers.borrow().clone();
        let window_message = WindowMessage {
            hwnd,
            msg: message,
            wparam,
            lparam,
        };
        for handler in handlers {
            if let Some(result) = handler.try_handle(&window_message) {
                return result;
            }
        }

        // 4. Defaults.
        match message {
            WM_SIZE => {
                self.invalidate();
                0
            }
            WM_DPICHANGED => 0,
            WM_GETMINMAXINFO => {
                let info = aligned_pointer::<MinMaxInfo>(lparam).map(|pointer| {
                    // SAFETY: for WM_GETMINMAXINFO `lparam` points to a MINMAXINFO that is valid and writable for the duration of the message (Win32
                    // contract); non-null and aligned checked above; the reference does not outlive `apply_min_track_size`.
                    unsafe { &mut *pointer }
                });
                self.apply_min_track_size(info);
                0
            }
            WM_CLOSE => {
                self.raise_closing();
                0
            }
            WM_SETCURSOR if low_word(lparam) == HTCLIENT => {
                apply_cursor_handle(cursor_handle(self.cursor.get()));
                1
            }
            // The renderer covers the whole client area: no background erase (no white flash).
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                validate_whole_client(hwnd);
                self.invalidate();
                0
            }
            // SAFETY: the parameters are the ones the window procedure received for this message.
            _ => unsafe { user32::def_window_proc(hwnd, message, wparam, lparam) },
        }
    }

    /// C# `UpdateClientSize(widthPx, heightPx)` (`WM_SIZE`, `LowWord(lParam)` x `HighWord(lParam)`): store, then `raise_client_size_changed_if_needed`.
    pub(super) fn update_client_size(&self, width_px: i32, height_px: i32) {
        self.client_width_px.set(width_px);
        self.client_height_px.set(height_px);
        self.raise_client_size_changed_if_needed();
    }

    /// C# `RaiseClientSizeChangedIfNeeded()`: raise `ClientSizeChanged` only when `client_size_dip()` differs from `last_raised_client_size`
    /// (record equality, NaN-safe); remember the new size.
    pub(super) fn raise_client_size_changed_if_needed(&self) {
        let size = self.client_size_dip();
        if size == self.last_raised_client_size.get() {
            return;
        }
        self.last_raised_client_size.set(size);
        self.raise_event(ShellWindowEvent::ClientSizeChanged);
    }

    /// C# `UpdateActive(active)` (`WM_ACTIVATE`, `LowWord(wParam) != WA_INACTIVE`, so `WA_CLICKACTIVE` counts as active): raise `Activated` or
    /// `Deactivated` only on a change.
    pub(super) fn update_active(&self, active: bool) {
        if active == self.is_active.get() {
            return;
        }
        self.is_active.set(active);
        self.raise_event(if active {
            ShellWindowEvent::Activated
        } else {
            ShellWindowEvent::Deactivated
        });
    }

    /// C# `RaiseClosing()` (`WM_CLOSE`): a fresh `CancelEventArgs`; every `Closing` handler runs (all of them, even after one cancelled, like a C#
    /// multicast); if none set `cancel`, `DestroyWindow(hwnd)`.
    pub(super) fn raise_closing(&self) {
        let mut args = CancelEventArgs::default();
        self.closing
            .raise(LIST_EVENT, &mut |handler: &mut ClosingHandler| handler(&mut args));
        if !args.cancel {
            let _ = user32::destroy_window(self.hwnd.get());
        }
    }

    /// Raises a parameterless event (no borrow is held while the handlers run).
    pub(super) fn raise_event(&self, event: ShellWindowEvent) {
        self.events
            .raise(event, &mut |handler: &mut WindowEventHandler| handler());
    }
}
