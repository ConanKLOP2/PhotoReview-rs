//! Port of `NativeDialog.cs` lines 189 to 425 (`ShowModal`, `ShowModeless`, `Activate`, `Close`, `EndModal`, `ModelessFilter`, `RunLoop`,
//! `ClaimDialogMessage`) and 519 to 533 (`ExtraWindowStyle`, `UsesModalFrame`, `DialogStyle`, `DialogExStyle`), baseline `d92d6aae`. Unit W8-18h.
//! Contract: `contracts/W8-18h.md`.
//!
//! Modal means: the owner is disabled, a private nested loop runs until `done`, and `WM_QUIT` is POSTED AGAIN and the dialog closes (lesson #486: a nested
//! loop never swallows `WM_QUIT`). With the shell's `MessageLoop` the nested loop is `MessageLoop::run_nested` (high-priority work, frame clocks and the message
//! filters keep running); without one it is a plain `GetMessage` loop (tests).
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

use super::core::{DialogError, DialogInner, ID_CANCEL, NativeDialog, WS_DEFAULT_DIALOG, dialog_proc, make_cookie};
use crate::dialogs::native::{controls::basic::CommonControlsInit, template};
use crate::host::message_loop::{MessageFilter, MessageFilterId, MessageLoop, RawMessageFilter};
use crate::host::pending_panic;
use crate::win32::api::user32::MessageOutcome;
use crate::win32::structs::{Hwnd, Msg};
use crate::win32::{api::user32, dialog_native, messages, startup};

/// Lifecycle state (C# `_used`, `_owner`, `_ownerDisabled`, `_done`, `_modeless`, `_cleanedUp`, `_result`, `_hwnd`, `_laidOut`, `_loop`, `_filter`,
/// `_closedCallback`, and the `GCHandle _self` as `self_cookie`).
#[allow(dead_code)] // W8-18i and W8-18k consume laid_out and cleaned_up after their stubs are ported.
pub(super) struct ModalState {
    /// C# `_hwnd`.
    pub(super) hwnd: Cell<Hwnd>,
    /// C# `_owner`: `0` when none or not a window.
    pub(super) owner: Cell<Hwnd>,
    /// C# `_used`.
    pub(super) used: Cell<bool>,
    /// C# `_ownerDisabled`.
    pub(super) owner_disabled: Cell<bool>,
    /// C# `_laidOut`: set by `initialize_dialog` after the first layout.
    pub(super) laid_out: Cell<bool>,
    /// C# `_done`.
    pub(super) done: Cell<bool>,
    /// C# `_modeless`.
    pub(super) modeless: Cell<bool>,
    /// C# `_cleanedUp`.
    pub(super) cleaned_up: Cell<bool>,
    /// C# `_result`; starts as `ID_CANCEL`.
    pub(super) result: Cell<i32>,
    /// C# `_self` (`GCHandle`): the cookie, `0` when not allocated or already freed.
    pub(super) self_cookie: Cell<isize>,
    /// C# `_loop` (modeless only).
    pub(super) message_loop: RefCell<Option<Rc<MessageLoop>>>,
    /// C# `_filter`: the registration in `message_loop` (modeless with a loop).
    pub(super) filter_id: Cell<Option<MessageFilterId>>,
    /// C# `_closedCallback`.
    pub(super) closed_callback: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl ModalState {
    /// All flags `false`, handles `0`, `result = ID_CANCEL`.
    pub(super) fn new() -> Self {
        Self {
            hwnd: Cell::new(0),
            owner: Cell::new(0),
            used: Cell::new(false),
            owner_disabled: Cell::new(false),
            laid_out: Cell::new(false),
            done: Cell::new(false),
            modeless: Cell::new(false),
            cleaned_up: Cell::new(false),
            result: Cell::new(ID_CANCEL),
            self_cookie: Cell::new(0),
            message_loop: RefCell::new(None),
            filter_id: Cell::new(None),
            closed_callback: RefCell::new(None),
        }
    }
}

/// C# `WsDefaultDialog | (UsesModalFrame ? DsModalFrame : 0) | DsNoFailCreate | (Resizable ? WS_THICKFRAME : 0) | ExtraWindowStyle` (pure).
/// `DS_MODALFRAME` = `0x80`, `DS_NOFAILCREATE` = `0x10`, `WS_THICKFRAME` = `0x40000`.
pub fn dialog_style(resizable: bool, uses_modal_frame: bool, extra_window_style: u32) -> u32 {
    WS_DEFAULT_DIALOG
        | if uses_modal_frame { 0x80 } else { 0 }
        | 0x10
        | if resizable { 0x40000 } else { 0 }
        | extra_window_style
}

/// C# `(UsesModalFrame ? WsExDlgModalFrame : 0) | WsExControlParent` (pure): `WS_EX_DLGMODALFRAME` = `1`, `WS_EX_CONTROLPARENT` = `0x10000`.
pub fn dialog_ex_style(uses_modal_frame: bool) -> u32 {
    (if uses_modal_frame { 1 } else { 0 }) | 0x10000
}

struct CleanupGuard<'a> {
    dialog: &'a NativeDialog,
    armed: bool,
}
impl Drop for CleanupGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.dialog.cleanup();
        }
    }
}

impl NativeDialog {
    fn prepare_window(&self, owner: Hwnd) -> Result<Vec<u8>, DialogError> {
        let state = &self.inner.modal;
        if state.used.replace(true) {
            return Err(DialogError::AlreadyShown);
        }
        state
            .owner
            .set(if owner != 0 && dialog_native::user32::is_window(owner) {
                owner
            } else {
                0
            });
        CommonControlsInit::ensure();
        let content = &self.inner.content;
        let template = template::build(
            &content.title(),
            dialog_style(
                content.resizable(),
                content.uses_modal_frame(),
                content.extra_window_style(),
            ),
            dialog_ex_style(content.uses_modal_frame()),
            &content.items(),
        )?;
        state.self_cookie.set(make_cookie(&self.inner));
        Ok(template)
    }

    fn create_window(&self, template: &[u8]) -> Result<(), DialogError> {
        let module = crate::win32::api::kernel32::get_module_handle().unwrap_or(0);
        // SAFETY: template was built by template::build; dialog_proc captures all panics before returning through Win32.
        let created = unsafe {
            dialog_native::user32::create_dialog_indirect_param(
                module,
                template,
                self.inner.modal.owner.get(),
                dialog_proc,
                self.inner.modal.self_cookie.get(),
            )
        };
        pending_panic::rethrow_pending();
        match created {
            Ok(hwnd) => {
                self.inner.modal.hwnd.set(hwnd);
                Ok(())
            }
            Err(error) => {
                self.inner.modal.hwnd.set(0);
                Err(DialogError::CreateFailed(error))
            }
        }
    }
    /// C# `ShowModal(owner, ready, loop)` (lines 194 to 248): shows the dialog modally on `owner` (`0` = no owner) and runs the loop until it closes.
    /// Returns `ID_OK` or `ID_CANCEL` (Cancel, X, Esc, Alt+F4, `WM_QUIT`, `close()`). `ready` runs after the window is shown and the owner disabled, before the
    /// loop. `message_loop` = the shell loop (`None` = a plain `GetMessage` loop). Steps, in order:
    /// 1. `used` already set -> `Err(AlreadyShown)` (nothing else happens); else set it.
    /// 2. `owner` = the argument when `owner != 0 && is_window(owner)`, else `0`.
    /// 3. `CommonControlsInit::ensure()`.
    /// 4. `template::build(title, dialog_style(..), dialog_ex_style(..), &content.items())` -> `Err(Template)` (before anything is allocated, `used` stays set).
    /// 5. `cookie = make_cookie(&inner)` -> `self_cookie`. From here a `CleanupGuard` (a local whose `Drop` calls `cleanup()`, the C# `finally`) runs `cleanup`
    ///    on every exit, also while a panic unwinds out of `ready` or of `resume_unwind`.
    /// 6. `create_dialog_indirect_param(module, &template, owner, dialog_proc, cookie)` (`WM_INITDIALOG` runs inside it and sets `hwnd`); then
    ///    `pending_panic::rethrow_pending()`; then `Err(e)` -> `hwnd = 0` and `Err(CreateFailed(e))`, `Ok(h)` -> `hwnd = h`.
    /// 7. When not `done`: when `owner != 0 && is_window_enabled(owner)` -> `enable_window(owner, false)` and `owner_disabled = true`; `show_window(hwnd,
    ///    SW_SHOW)`; `set_foreground_window(hwnd)`; `ready(self)`; when still not `done` -> `run_loop(message_loop)`.
    /// 8. The guard cleans up, then `rethrow_pending()`, then `Ok(result)`.
    pub fn show_modal(
        &self,
        owner: Hwnd,
        ready: Option<&dyn Fn(&NativeDialog)>,
        message_loop: Option<&Rc<MessageLoop>>,
    ) -> Result<i32, DialogError> {
        let template = self.prepare_window(owner)?;
        let guard = CleanupGuard {
            dialog: self,
            armed: true,
        };
        self.create_window(&template)?;
        let state = &self.inner.modal;
        if !state.done.get() {
            let owner = state.owner.get();
            if owner != 0 && dialog_native::user32::is_window_enabled(owner) {
                dialog_native::user32::enable_window(owner, false);
                state.owner_disabled.set(true);
            }
            user32::show_window(self.hwnd(), messages::SW_SHOW);
            startup::set_foreground_window(self.hwnd());
            if let Some(ready) = ready {
                ready(self);
            }
            if !state.done.get() {
                self.run_loop(message_loop)?;
            }
        }
        drop(guard);
        pending_panic::rethrow_pending();
        Ok(state.result.get())
    }

    /// C# `ShowModeless(owner, loop, ready, closed)` (lines 250 to 308): shows the dialog without blocking and without disabling the owner; returns as soon
    /// as the window is visible. The window cleans itself up when it closes (X, a button, `close()`); `closed` runs exactly once after the window was destroyed.
    /// Steps 1 to 6 as `show_modal`, with `closed_callback = closed`, and on ANY failure after step 5 `cleanup()` runs and the error is returned (the C#
    /// `catch { Cleanup(); throw; }`); then: `done` already (the dialog closed inside `WM_INITDIALOG`) -> `cleanup()` and `Ok(())`; else `modeless = true`,
    /// `message_loop` stored, and with a loop the filter `ModelessFilter` is registered (`add_message_filter`, id kept in `filter_id`) so Tab, Esc and Enter reach
    /// `IsDialogMessage`; `show_window(hwnd, SW_SHOW)`; `set_foreground_window(hwnd)`; `ready(self)` inside `catch_unwind`: a panic -> `end_modal(ID_CANCEL)`, then
    /// `resume_unwind` (C# `catch { EndModal(IdCancel); throw; }`).
    pub fn show_modeless(
        &self,
        owner: Hwnd,
        message_loop: Option<Rc<MessageLoop>>,
        ready: Option<&dyn Fn(&NativeDialog)>,
        closed: Option<Box<dyn FnOnce()>>,
    ) -> Result<(), DialogError> {
        let template = self.prepare_window(owner)?;
        *self.inner.modal.closed_callback.borrow_mut() = closed;
        let mut guard = CleanupGuard {
            dialog: self,
            armed: true,
        };
        self.create_window(&template)?;
        let state = &self.inner.modal;
        if state.done.get() {
            return Ok(());
        }
        state.modeless.set(true);
        if let Some(message_loop) = message_loop {
            let filter = Rc::new(ModelessFilter {
                dialog: Rc::downgrade(&self.inner),
            });
            let id = message_loop.add_message_filter(filter);
            state.filter_id.set(Some(id));
            *state.message_loop.borrow_mut() = Some(message_loop);
        }
        guard.armed = false;
        user32::show_window(self.hwnd(), messages::SW_SHOW);
        startup::set_foreground_window(self.hwnd());
        if let Some(ready) = ready {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| ready(self))) {
                self.end_modal(ID_CANCEL);
                resume_unwind(payload);
            }
        }
        Ok(())
    }

    /// C# `Activate()`: brings the window to the front (restores it when minimized). No-op when `hwnd == 0` or the window is gone. Order: `is_iconic` ->
    /// `show_window(hwnd, SW_RESTORE)`; then `set_foreground_window(hwnd)`.
    pub fn activate(&self) {
        let hwnd = self.hwnd();
        if hwnd == 0 || !dialog_native::user32::is_window(hwnd) {
            return;
        }
        if startup::is_iconic(hwnd) {
            user32::show_window(hwnd, messages::SW_RESTORE);
        }
        startup::set_foreground_window(hwnd);
    }

    /// C# `Close()`: `end_modal(ID_CANCEL)`. Idempotent, UI thread.
    pub fn close(&self) {
        self.end_modal(ID_CANCEL);
    }

    /// C# `EndModal(result)`: `done` already -> nothing. Else `result`, `done = true`; modeless -> `cleanup()` at once (no private loop); modal and `hwnd != 0` ->
    /// `post_message(hwnd, WM_NULL, 0, 0)` (wakes the `GetMessage` of the nested loop; the error is ignored).
    pub fn end_modal(&self, result: i32) {
        let state = &self.inner.modal;
        if state.done.replace(true) {
            return;
        }
        state.result.set(result);
        if state.modeless.get() {
            self.cleanup();
        } else if state.hwnd.get() != 0 {
            // SAFETY: WM_NULL carries no pointers in its parameters; the OS validates hwnd.
            let _ = unsafe { user32::post_message(state.hwnd.get(), messages::WM_NULL, 0, 0) };
        }
    }

    /// C# `RunLoop(loop)` (lines 370 to 412). With a loop: `loop.run_nested(is_done, claim)`; `Ok(false)` (`WM_QUIT`, already re-posted by the loop) ->
    /// `result = ID_CANCEL`, `done = true`; `Err(e)` -> `Err(Loop(e))`. Without a loop: `while !done { get_message }`: `Quit` -> `post_quit_message(wParam)`,
    /// `result = ID_CANCEL`, `done = true`, break; `Failed` -> the same without the repost; `Message` -> when `hwnd != 0 && (content.on_dialog_key(..) ||
    /// is_dialog_message(hwnd, msg))` continue; else `translate_message`, `dispatch_message`.
    pub(super) fn run_loop(&self, message_loop: Option<&Rc<MessageLoop>>) -> Result<(), DialogError> {
        if let Some(message_loop) = message_loop {
            let mut is_done = || self.is_closed();
            let mut claim = |message: &mut Msg| self.claim_dialog_message(message);
            if !message_loop.run_nested(&mut is_done, Some(&mut claim))? {
                self.inner.modal.result.set(ID_CANCEL);
                self.inner.modal.done.set(true);
            }
            return Ok(());
        }
        while !self.is_closed() {
            let mut message = Msg::default();
            match user32::get_message(&mut message, 0, 0, 0) {
                MessageOutcome::Quit => {
                    user32::post_quit_message(message.wParam.0 as i32);
                    self.inner.modal.result.set(ID_CANCEL);
                    self.inner.modal.done.set(true);
                    break;
                }
                MessageOutcome::Failed(_) => {
                    self.inner.modal.result.set(ID_CANCEL);
                    self.inner.modal.done.set(true);
                    break;
                }
                MessageOutcome::Message => {
                    if self.claim_dialog_message(&mut message) {
                        continue;
                    }
                    user32::translate_message(&message);
                    // SAFETY: message came from GetMessage on this thread and its parameters are valid for dispatch.
                    unsafe { user32::dispatch_message(&message) };
                }
            }
        }
        Ok(())
    }

    /// C# `ClaimDialogMessage(ref Msg)`: `true` when the message was consumed: `hwnd != 0 && content.on_dialog_key(..)`, else `hwnd != 0 &&
    /// is_dialog_message(hwnd, msg)`. Passed to `run_nested` as the claim.
    pub(super) fn claim_dialog_message(&self, message: &mut Msg) -> bool {
        let hwnd = self.hwnd();
        if hwnd == 0 {
            return false;
        }
        if self.inner.content.on_dialog_key(self, message) {
            return true;
        }
        // SAFETY: message came from the thread queue or MessageLoop's nested queue processing.
        unsafe { dialog_native::user32::is_dialog_message(hwnd, message) }
    }
}

/// C# `ModelessFilter`: a raw message filter of the shell loop that gives Tab, Esc, Enter and the accelerators of a modeless dialog to `IsDialogMessage`.
/// Holds a `Weak` (the loop holds the filter, D-W8B3H-18).
pub(super) struct ModelessFilter {
    pub(super) dialog: Weak<DialogInner>,
}

impl MessageFilter for ModelessFilter {
    /// C# `PreFilterMessage` = `false`.
    fn pre_filter_message(&self, _message: &crate::host::contracts::WindowMessage) -> bool {
        false
    }

    /// Returns `Some(self)`: the loop calls `pre_filter_raw_message` instead.
    fn as_raw(&self) -> Option<&dyn RawMessageFilter> {
        Some(self)
    }
}

impl RawMessageFilter for ModelessFilter {
    /// C# `PreFilterRawMessage(in Msg)`: the dialog is gone or `hwnd == 0` -> `false`; else `is_dialog_message(hwnd, &mut copy_of_message)` (the OS may want a
    /// mutable `MSG`; the copy is discarded).
    fn pre_filter_raw_message(&self, message: &Msg) -> bool {
        let Some(inner) = self.dialog.upgrade() else {
            return false;
        };
        let hwnd = inner.modal.hwnd.get();
        if hwnd == 0 {
            return false;
        }
        let mut copy = *message;
        // SAFETY: message was supplied by MessageLoop from the thread queue; the OS can inspect and dispatch this copy.
        unsafe { dialog_native::user32::is_dialog_message(hwnd, &mut copy) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::native::dialog::core::ID_CANCEL;
    use crate::host::contracts::WindowMessage;

    #[test]
    fn modal_state_starts_cancelled() {
        // C#: NativeDialog field initializers.
        let state = ModalState::new();
        assert_eq!(state.result.get(), ID_CANCEL);
        assert_eq!(state.hwnd.get(), 0);
        assert_eq!(state.owner.get(), 0);
        assert_eq!(state.self_cookie.get(), 0);
        for flag in [
            state.used.get(),
            state.owner_disabled.get(),
            state.laid_out.get(),
            state.done.get(),
            state.modeless.get(),
            state.cleaned_up.get(),
        ] {
            assert!(!flag);
        }
        assert!(state.message_loop.borrow().is_none());
        assert!(state.filter_id.get().is_none());
        assert!(state.closed_callback.borrow().is_none());
    }

    #[test]
    fn modeless_filter_without_a_dialog_returns_false() {
        // C#: NativeDialog.ModelessFilter.PreFilterRawMessage.
        let filter = ModelessFilter { dialog: Weak::new() };
        assert!(!filter.pre_filter_raw_message(&Msg::default()));
    }

    #[test]
    fn modeless_filter_is_a_raw_filter() {
        // C#: NativeDialog.ModelessFilter.PreFilterMessage / PreFilterRawMessage.
        let filter = ModelessFilter { dialog: Weak::new() };
        let _ = std::any::TypeId::of::<WindowMessage>();
        assert!(filter.as_raw().is_some());
    }
}
