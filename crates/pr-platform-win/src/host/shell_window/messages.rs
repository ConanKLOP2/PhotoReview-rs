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

/// Clears the flag on drop (also when a `Closing` handler panics).
struct ResetFlag<'a>(&'a std::cell::Cell<bool>);

impl Drop for ResetFlag<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
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
            _ => unsafe { (self.default_proc.get())(hwnd, message, wparam, lparam) },
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
        // A `WM_CLOSE` that arrives while a `Closing` handler still runs (it pumps messages, e.g. a confirmation box) is ignored: `EventList::raise` skips
        // the running handler, so the nested call would see `cancel == false` and destroy the window under the outer handler.
        if self.closing_in_progress.replace(true) {
            return;
        }
        let _reset = ResetFlag(&self.closing_in_progress);
        let mut args = CancelEventArgs::default();
        self.closing
            .raise(LIST_EVENT, &mut |handler: &mut ClosingHandler| handler(&mut args));
        drop(_reset);
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

#[cfg(test)]
mod tests {
    use super::super::create::ShellWindowOptions;
    use super::*;
    use crate::host::error::HostError;
    use crate::host::frame_clock::base::{FrameHandler, FrameHandlerId, HostFrameClock};
    use std::cell::Cell;
    use std::rc::Rc;

    const SENTINEL: Lresult = 0x5EED;

    thread_local! {
        static DEFAULT_CALLS: Cell<u32> = const { Cell::new(0) };
    }

    /// Stands for `DefWindowProc`: counts the call and answers a value no default arm gives.
    unsafe fn counting_default(_hwnd: Hwnd, _message: u32, _wparam: Wparam, _lparam: Lparam) -> Lresult {
        DEFAULT_CALLS.with(|calls| calls.set(calls.get() + 1));
        SENTINEL
    }

    /// A clock that records nothing: `invalidate` only needs `subscribe`.
    struct NullClock;

    impl HostFrameClock for NullClock {
        fn subscribe(&self, _handler: FrameHandler) -> FrameHandlerId {
            1
        }
        fn unsubscribe(&self, _id: FrameHandlerId) {}
        fn request_frame(&self) {}
        fn is_armed(&self) -> bool {
            false
        }
        fn tick_count(&self) -> i64 {
            0
        }
        fn wait_handle(&self) -> isize {
            0
        }
        fn deadline_timestamp(&self) -> i64 {
            0
        }
        fn ticks_until_deadline(&self) -> i64 {
            i64::MAX
        }
        fn on_signaled(&self) -> Result<bool, HostError> {
            Ok(false)
        }
        fn on_deadline(&self) -> Result<bool, HostError> {
            Ok(false)
        }
        fn dispose(&self) {}
    }

    fn inner() -> Rc<ShellWindowInner> {
        let inner = Rc::new_cyclic(|weak| {
            ShellWindowInner::new(ShellWindowOptions::default(), Rc::new(NullClock), weak.clone())
        });
        inner.default_proc.set(counting_default);
        DEFAULT_CALLS.with(|calls| calls.set(0));
        inner
    }

    // messages.rs 88:13 (P-397): the default `WM_DPICHANGED => 0` arm answers 0 itself; without the arm the message would go to the default window
    // procedure (here the counting stand-in answers SENTINEL).
    #[test]
    fn default_wm_dpichanged_answers_zero_without_the_default_window_procedure() {
        let inner = inner();
        let result = inner.process_message(0, WM_DPICHANGED, 144, 0);
        assert_eq!(result, 0, "WM_DPICHANGED is answered with 0 by its own arm");
        assert_eq!(
            DEFAULT_CALLS.with(Cell::get),
            0,
            "the default window procedure is not reached"
        );
        assert_eq!(inner.dpi.get(), 144, "the window still took the new dpi");
    }

    // Control: a message without a default arm does reach the default window procedure, so the test above can tell the two apart.
    #[test]
    fn message_without_a_default_arm_reaches_the_default_window_procedure() {
        let inner = inner();
        let result = inner.process_message(0, 0, 0, 0); // WM_NULL
        assert_eq!(result, SENTINEL);
        assert_eq!(DEFAULT_CALLS.with(Cell::get), 1);
    }
}
