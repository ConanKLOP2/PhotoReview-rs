//! Port of `NativeDialog.cs` lines 427 to 518 (`Cleanup`, `DeleteObject`) and the `WM_CLOSE` case of `HandleMessage` (lines 583 to 589), baseline `d92d6aae`.
//! Unit W8-18k. Contract: `contracts/W8-18k.md`.
//!
//! Closing: every exit of a dialog (normal, from outside, a panic) ends in `cleanup`, once. It re-enables the owner BEFORE destroying the dialog (so the
//! activation returns to the owner), releases the subclasses, destroys the window, deletes the GDI objects, frees the cookie and finally tells the opener.
use std::cell::Cell;
use std::panic::AssertUnwindSafe;

use super::core::{ID_CANCEL, NativeDialog, free_cookie};
use super::{events, tooltip};
use crate::host::pending_panic;
use crate::win32::structs::{Hgdi, Lresult};
use crate::win32::{api::user32, comctl, dialog_native};

/// C# `DeleteObject(ref handle)` (pure bookkeeping around one OS call): a non-zero handle is deleted (`delete_object`) and the cell is reset to `0`.
pub(super) fn delete_and_clear(handle: &Cell<Hgdi>) {
    let object = handle.get();
    if object != 0 {
        let _ = dialog_native::gdi32::delete_object(object);
        handle.set(0);
    }
}

/// Step 10 of `cleanup` (pure apart from the callback): runs the `closed` callback under `pending_panic::guarded`, so a panic in it is captured (first one wins)
/// instead of unwinding out of `cleanup`, which may itself run inside `CleanupGuard::drop` during unwinding (a panic there aborts the process).
pub(super) fn run_closed_callback(callback: Option<Box<dyn FnOnce()>>) {
    if let Some(callback) = callback {
        pending_panic::guarded((), AssertUnwindSafe(callback));
    }
}

impl NativeDialog {
    /// C# `Cleanup()` (lines 427 to 509). Idempotent (`cleaned_up`). Order, observable and required:
    /// 1. `cleaned_up = true`, `done = true`.
    /// 2. the modeless filter is removed from the loop (`remove_message_filter(filter_id.take())`).
    /// 3. `content.on_closed(self)` under `pending_panic::guarded` (a panic is resumed by the caller of `show_*` through `rethrow_pending`; the C# kept it in `s_pending`).
    /// 4. each list view of `events.list_views` that is still a window: `remove_window_subclass(list, list_view_subclass_proc, LIST_VIEW_SUBCLASS_ID)`; the list is cleared.
    /// 5. each list box of `tips.item_tips` that is still a window: `remove_window_subclass(list, item_tip_subclass_proc, ITEM_TIP_SUBCLASS_ID)`; `item_tips` and
    ///    `last_tip_item` are cleared.
    /// 6. `owner_disabled && is_window(owner)` -> `enable_window(owner, true)`; `owner_disabled = false`.
    /// 7. `hwnd != 0`: when `is_window(hwnd)`: `SetWindowLongPtr(hwnd, DWLP_USER, 0)` (later messages find no dialog) then `destroy_window(hwnd)` (error ignored);
    ///    `hwnd = 0`.
    /// 8. `delete_and_clear` for `font`, `header_font`, `window_brush`, `input_brush`; `controls.children` cleared; `tips.tooltip = 0`; `tip_buffer` emptied and its
    ///    allocation released.
    /// 9. the cookie is freed (`free_cookie(self_cookie)`, then `self_cookie = 0`): safe even from inside the dialog procedure, which holds its own `Rc`.
    /// 10. `closed_callback.take()` runs under `pending_panic::guarded` (`run_closed_callback`).
    ///
    /// DIVERGENCE (P-67, owner-confirmed): the C# `_closedCallback?.Invoke()` is unguarded, but .NET has no abort on a second exception. Here `CleanupGuard::drop`
    /// runs `cleanup` while unwinding, where a panic in the callback would abort the process, so it is captured like `on_closed` and resumed by `rethrow_pending`.
    ///
    /// `child_hosts`, `sorts` and `laid_out` are NOT reset (as the C#).
    pub(super) fn cleanup(&self) {
        let state = &self.inner.modal;
        if state.cleaned_up.replace(true) {
            return;
        }
        state.done.set(true);

        if let Some(id) = state.filter_id.take() {
            let message_loop = state.message_loop.borrow().clone();
            if let Some(message_loop) = message_loop {
                message_loop.remove_message_filter(id);
            }
        }

        pending_panic::guarded((), AssertUnwindSafe(|| self.inner.content.on_closed(self)));

        let list_views = std::mem::take(&mut *self.inner.events.list_views.borrow_mut());
        for list in list_views {
            if dialog_native::user32::is_window(list) {
                let _ = comctl::common_controls::remove_window_subclass(
                    list,
                    events::list_view_subclass_proc,
                    events::LIST_VIEW_SUBCLASS_ID,
                );
            }
        }

        let item_tips = std::mem::take(&mut *self.inner.tips.item_tips.borrow_mut());
        for (list, _) in &item_tips {
            if dialog_native::user32::is_window(*list) {
                let _ = comctl::common_controls::remove_window_subclass(
                    *list,
                    tooltip::item_tip_subclass_proc,
                    tooltip::ITEM_TIP_SUBCLASS_ID,
                );
            }
        }
        self.inner.tips.last_tip_item.borrow_mut().clear();
        drop(item_tips);

        if state.owner_disabled.get() && dialog_native::user32::is_window(state.owner.get()) {
            dialog_native::user32::enable_window(state.owner.get(), true);
        }
        state.owner_disabled.set(false);

        let hwnd = state.hwnd.get();
        if hwnd != 0 {
            if dialog_native::user32::is_window(hwnd) {
                // SAFETY: DWLP_USER is this dialog's cookie slot; clearing it stops later messages from finding the dialog.
                unsafe { user32::set_window_long_ptr(hwnd, dialog_native::DWLP_USER, 0) };
                let _ = user32::destroy_window(hwnd);
            }
            state.hwnd.set(0);
        }

        delete_and_clear(&self.inner.metrics.font);
        delete_and_clear(&self.inner.metrics.header_font);
        delete_and_clear(&self.inner.draw.window_brush);
        delete_and_clear(&self.inner.draw.input_brush);
        self.inner.controls.children.borrow_mut().clear();
        self.inner.tips.tooltip.set(0);
        let tip_buffer = std::mem::take(&mut *self.inner.tips.tip_buffer.borrow_mut());
        drop(tip_buffer);

        let cookie = state.self_cookie.replace(0);
        // SAFETY: self_cookie came from make_cookie and replace(0) ensures it is freed at most once after subclass and window removal.
        unsafe { free_cookie(cookie) };

        let callback = state.closed_callback.borrow_mut().take();
        run_closed_callback(callback);
    }

    /// The `WM_CLOSE` case (lines 583 to 589): `content.can_close_by_user()` -> `end_modal(ID_CANCEL)`; the router answers `1` either way. `close()` from outside
    /// does not ask the hook.
    pub(super) fn on_close_request(&self) -> Lresult {
        if self.inner.content.can_close_by_user() {
            self.end_modal(ID_CANCEL);
        }
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{delete_and_clear, run_closed_callback};
    use crate::host::pending_panic;
    use crate::win32::dialog_native;
    use std::cell::Cell;
    use std::rc::Rc;

    // P-67 (a): the closed callback runs once; no callback is a no-op.
    #[test]
    fn run_closed_callback_runs_the_callback_and_accepts_none() {
        run_closed_callback(None);
        let ran = Rc::new(Cell::new(0));
        let seen = ran.clone();
        run_closed_callback(Some(Box::new(move || seen.set(seen.get() + 1))));
        assert_eq!(ran.get(), 1);
        assert!(!pending_panic::has_pending());
    }

    // P-67 (a): a panicking callback does not unwind out of the helper (the unguarded form would abort inside CleanupGuard::drop while unwinding); the payload is
    // captured and `rethrow_pending` resumes it. The abort itself cannot be observed in-process, so this tests the extracted helper (indirect).
    #[test]
    fn run_closed_callback_captures_a_panic_instead_of_unwinding() {
        let outcome = std::panic::catch_unwind(|| {
            run_closed_callback(Some(Box::new(|| std::panic::panic_any("closed callback boom"))));
        });
        assert!(outcome.is_ok(), "the helper must not unwind");
        assert!(pending_panic::has_pending());
        let resumed = std::panic::catch_unwind(pending_panic::rethrow_pending).expect_err("pending panic is resumed");
        assert_eq!(resumed.downcast_ref::<&str>(), Some(&"closed callback boom"));
        assert!(!pending_panic::has_pending());
    }

    // C#: NativeDialog.DeleteObject with an empty handle.
    #[test]
    fn delete_and_clear_resets_a_zero_handle_without_an_os_call() {
        let handle = Cell::new(0);
        delete_and_clear(&handle);
        assert_eq!(handle.get(), 0);
    }

    // C#: NativeDialog.DeleteObject releases one solid brush and zeroes the slot.
    #[test]
    fn delete_and_clear_deletes_a_real_brush() {
        let brush = dialog_native::gdi32::create_solid_brush(0x0012_3456);
        assert_ne!(brush, 0);
        let handle = Cell::new(brush);
        delete_and_clear(&handle);
        assert_eq!(handle.get(), 0);
        // SAFETY: flush this thread's GDI batch before checking that the handle is gone.
        let _ = unsafe { windows::Win32::Graphics::Gdi::GdiFlush() };
        // SAFETY: brush is a former GDI handle; GetObjectType returns zero for a deleted object.
        let kind = unsafe {
            windows::Win32::Graphics::Gdi::GetObjectType(windows::Win32::Graphics::Gdi::HGDIOBJ(brush as *mut _))
        };
        assert_eq!(kind, 0);
    }
}
