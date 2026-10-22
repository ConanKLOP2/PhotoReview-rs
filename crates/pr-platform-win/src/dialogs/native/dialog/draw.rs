//! Port of `NativeDialog.cs` lines 802 to 817 (`DrawOwnerItem`) and the colouring cases of `HandleMessage` (lines 591 to 599 `WM_SETCURSOR`, 639 to 666 the
//! `WM_CTLCOLOR*` family), plus the creation of the two brushes of `InitializeDialog` (lines 981 and 982), baseline `d92d6aae`. Unit W8-18k.
//! Contract: `contracts/W8-18k.md`.
//!
//! The dark look of the dialog itself: the background and the text of every control kind come from the `WM_CTLCOLOR*` answers (the controls lost their visual
//! style, `chrome.rs`), owner-draw list boxes are painted by `ListBoxControl::draw_item`, and a busy dialog shows the wait cursor everywhere.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::Cell;

use super::core::NativeDialog;
use crate::win32::structs::{Hgdi, Hwnd, Lparam, Lresult, Wparam};

/// C# `IdcWait` = `IDC_WAIT` (`32514`).
pub const IDC_WAIT: i32 = 32514;

/// The two solid brushes (C# `_windowBrush`, `_inputBrush`); created at `WM_INITDIALOG`, deleted by `cleanup`.
pub(super) struct DrawState {
    /// C# `_windowBrush`: `DialogColors::window()`.
    pub(super) window_brush: Cell<Hgdi>,
    /// C# `_inputBrush`: `DialogColors::input()`.
    pub(super) input_brush: Cell<Hgdi>,
}

impl DrawState {
    /// Both `0`.
    pub(super) fn new() -> Self {
        todo!()
    }
}

/// The colours and the brush a `WM_CTLCOLOR*` message asks for (pure decision of `HandleMessage` lines 275 to 302).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtlColor {
    /// `WM_CTLCOLORDLG`: only the brush (the window brush); no colour call.
    DialogBackground,
    /// `WM_CTLCOLORBTN`, `WM_CTLCOLORMSGBOX`, `WM_CTLCOLORSCROLLBAR`: text `DialogColors::text()`, background `window()`, window brush.
    Button,
    /// `WM_CTLCOLORSTATIC` of a STATIC: text `content.static_text_color(id)`, background `window()`, window brush.
    Static,
    /// `WM_CTLCOLORSTATIC` of a read-only EDIT: text `content.static_text_color(id)`, background `input()`, INPUT brush.
    ReadOnlyEdit,
    /// `WM_CTLCOLOREDIT`, `WM_CTLCOLORLISTBOX`: text `text_strong()`, background `input()`, input brush.
    Input,
}

/// Which colouring a message gets (pure): `WM_CTLCOLORDLG` (`0x0136`) -> `DialogBackground`; `0x0135`, `0x0132`, `0x0137` -> `Button`; `0x0138` -> `ReadOnlyEdit`
/// when `control_is_edit` else `Static`; `0x0133`, `0x0134` -> `Input`; any other message -> `None`.
pub fn ctl_color_kind(message: u32, control_is_edit: bool) -> Option<CtlColor> {
    todo!()
}

impl NativeDialog {
    /// Creates `window_brush` and `input_brush` (`create_solid_brush(DialogColors::window())` and `input()`), the two lines at the start of `InitializeDialog`
    /// (called by `initialize_dialog`, 18j, right after the icon).
    pub(super) fn create_brushes(&self) {
        todo!()
    }

    /// C# `DrawOwnerItem(drawItem)` (lines 802 to 817), the `WM_DRAWITEM` case; `true` = handled (the router answers `1`). `lparam == 0` -> `false`. `lparam` is
    /// a `*const DRAWITEMSTRUCT` (SAFETY: valid for the message). `id = ctlID`, `item_id = itemID as i32` (`-1` for an empty list or the area below the last row),
    /// `control = hwndItem`. `item = item_id >= 0 ? content.list_item_text(self, id, item_id) : ListItemText::default()`; `font = control != 0 ?
    /// WM_GETFONT(control) : 0`; `ListBoxControl::draw_item(&draw, &item.text, item.badge.as_deref(), font, dx(4), subtitle, badge_color)` where `subtitle` and
    /// `badge_color` come from the hooks only for `item_id >= 0`. All hooks run with no borrow held.
    pub(super) fn draw_owner_item(&self, lparam: Lparam) -> bool {
        todo!()
    }

    /// The `WM_CTLCOLOR*` cases: `Some(brush)` is the router's return value (the OS reads it as the brush handle), `None` = not a colour message. `kind =
    /// ctl_color_kind(message, is_edit(lparam))` (the edit test only for `WM_CTLCOLORSTATIC`); then `set_text_color(wparam as Hdc, ..)` and `set_bk_color(..)` as the
    /// kind says, and the brush. `wparam` is the control's DC.
    pub(super) fn on_ctl_color(&self, message: u32, wparam: Wparam, lparam: Lparam) -> Option<Lresult> {
        todo!()
    }

    /// The `WM_SETCURSOR` case (lines 591 to 599): `content.wait_cursor()` -> `set_cursor(load_system_cursor(IDC_WAIT))` (an `Err` keeps the cursor),
    /// `SetWindowLongPtr(hwnd, DWLP_MSGRESULT, 1)` and answer `1`; else `0` (default processing).
    pub(super) fn on_set_cursor(&self, hwnd: Hwnd) -> Lresult {
        todo!()
    }
}
