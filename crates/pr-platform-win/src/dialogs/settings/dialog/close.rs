//! Port of `SettingsDialog.cs` lines 440 to 462 (`OnClosed`), baseline `d92d6aae`. Unit W8-19m. Contract: `contracts/W8-19m.md`.
//!
//! What ends with the window: the language subscription, the update check, the pages, the key box brush, and (Rust only) the `Rc` cycle, the subclass registry entry, the
//! poster and the queues.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;

impl SettingsContent {
    /// C# `OnClosed()`, then the Rust-only cleanup, in this order. Runs once, right before the window is destroyed (also after a captured panic), under
    /// `pending_panic::guarded` by the caller; it must not panic itself.
    /// 1. drop the language `subscription` (C# `CurrentChanged -= handler`);
    /// 2. `presenter.borrow().cancel_update_check()` (an answer that arrives later is dropped by the poster, INV-9 spirit);
    /// 3. `on_closed()` of every page, in order (a page that is borrowed is skipped);
    /// 4. delete the key box brush (`delete_object`) and set it to `0`;
    /// 5. `poster.set_hwnd(0)` (clears the cross-thread queue), clear `local_queue`, `placed` and `page_combos` (their handles die with the window), `attached = false`;
    /// 6. `forget_subclass_key()` (after this the subclass procedures of the children pass messages on);
    /// 7. `link.take()` (breaks the `NativeDialog` <-> content cycle). Nothing may use `dialog` after this call returns, except the base class destroying the window.
    pub(super) fn on_closed(&self, dialog: &NativeDialog) {
        todo!()
    }
}
