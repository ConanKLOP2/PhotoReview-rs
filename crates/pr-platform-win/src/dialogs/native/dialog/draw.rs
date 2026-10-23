//! Port of `NativeDialog.cs` lines 802 to 817 (`DrawOwnerItem`) and the colouring cases of `HandleMessage` (lines 591 to 599 `WM_SETCURSOR`, 639 to 666 the
//! `WM_CTLCOLOR*` family), plus the creation of the two brushes of `InitializeDialog` (lines 981 and 982), baseline `d92d6aae`. Unit W8-18k.
//! Contract: `contracts/W8-18k.md`.
//!
//! The dark look of the dialog itself: the background and the text of every control kind come from the `WM_CTLCOLOR*` answers (the controls lost their visual
//! style, `chrome.rs`), owner-draw list boxes are painted by `ListBoxControl::draw_item`, and a busy dialog shows the wait cursor everywhere.
use std::cell::Cell;

use super::core::{ListItemText, NativeDialog};
use crate::dialogs::native::chrome::DialogColors;
use crate::dialogs::native::controls::basic::ListBoxControl;
use crate::win32::structs::{Hdc, Hgdi, Hwnd, Lparam, Lresult, Wparam};
use crate::win32::{api::user32, dialog_native};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;

const _: () = {
    assert!(std::mem::size_of::<DRAWITEMSTRUCT>() == 64);
    assert!(std::mem::offset_of!(DRAWITEMSTRUCT, CtlID) == 4);
    assert!(std::mem::offset_of!(DRAWITEMSTRUCT, itemID) == 8);
    assert!(std::mem::offset_of!(DRAWITEMSTRUCT, hwndItem) == 24);
};

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
    #[allow(dead_code)] // The 18h constructor still initializes these cells inline; the unit test verifies this constructor's contract.
    pub(super) fn new() -> Self {
        Self {
            window_brush: Cell::new(0),
            input_brush: Cell::new(0),
        }
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
    match message {
        dialog_native::WM_CTLCOLORDLG => Some(CtlColor::DialogBackground),
        dialog_native::WM_CTLCOLORBTN | dialog_native::WM_CTLCOLORMSGBOX | dialog_native::WM_CTLCOLORSCROLLBAR => {
            Some(CtlColor::Button)
        }
        dialog_native::WM_CTLCOLORSTATIC => Some(if control_is_edit {
            CtlColor::ReadOnlyEdit
        } else {
            CtlColor::Static
        }),
        dialog_native::WM_CTLCOLOREDIT | dialog_native::WM_CTLCOLORLISTBOX => Some(CtlColor::Input),
        _ => None,
    }
}

impl NativeDialog {
    /// Creates `window_brush` and `input_brush` (`create_solid_brush(DialogColors::window())` and `input()`), the two lines at the start of `InitializeDialog`
    /// (called by `initialize_dialog`, 18j, right after the icon).
    pub(super) fn create_brushes(&self) {
        self.inner
            .draw
            .window_brush
            .set(dialog_native::gdi32::create_solid_brush(DialogColors::window()));
        self.inner
            .draw
            .input_brush
            .set(dialog_native::gdi32::create_solid_brush(DialogColors::input()));
    }

    /// C# `DrawOwnerItem(drawItem)` (lines 802 to 817), the `WM_DRAWITEM` case; `true` = handled (the router answers `1`). `lparam == 0` -> `false`. `lparam` is
    /// a `*const DRAWITEMSTRUCT` (SAFETY: valid for the message). `id = ctlID`, `item_id = itemID as i32` (`-1` for an empty list or the area below the last row),
    /// `control = hwndItem`. `item = item_id >= 0 ? content.list_item_text(self, id, item_id) : ListItemText::default()`; `font = control != 0 ?
    /// WM_GETFONT(control) : 0`; `ListBoxControl::draw_item(&draw, &item.text, item.badge.as_deref(), font, dx(4), subtitle, badge_color)` where `subtitle` and
    /// `badge_color` come from the hooks only for `item_id >= 0`. All hooks run with no borrow held.
    pub(super) fn draw_owner_item(&self, lparam: Lparam) -> bool {
        if lparam == 0 {
            return false;
        }
        // SAFETY: WM_DRAWITEM supplies a valid DRAWITEMSTRUCT for the duration of this synchronous call.
        let draw = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
        let id = draw.CtlID as i32;
        let item_id = draw.itemID as i32;
        let control = draw.hwndItem.0 as Hwnd;
        let item = if item_id >= 0 {
            self.inner.content.list_item_text(self, id, item_id)
        } else {
            ListItemText::default()
        };
        let font = if control != 0 {
            // SAFETY: WM_GETFONT has no pointer arguments; control comes from this DRAWITEMSTRUCT.
            unsafe { dialog_native::user32::send_message(control, dialog_native::WM_GETFONT, 0, 0) }
        } else {
            0
        };
        let subtitle = (item_id >= 0)
            .then(|| self.inner.content.list_item_subtitle(self, id, item_id))
            .flatten();
        let badge_color = (item_id >= 0)
            .then(|| self.inner.content.list_item_badge_color(self, id, item_id))
            .flatten();
        ListBoxControl::draw_item(
            draw,
            &item.text,
            item.badge.as_deref(),
            font,
            self.dx(4),
            subtitle.as_deref(),
            badge_color,
        );
        true
    }

    /// The `WM_CTLCOLOR*` cases: `Some(brush)` is the router's return value (the OS reads it as the brush handle), `None` = not a colour message. `kind =
    /// ctl_color_kind(message, is_edit(lparam))` (the edit test only for `WM_CTLCOLORSTATIC`); then `set_text_color(wparam as Hdc, ..)` and `set_bk_color(..)` as the
    /// kind says, and the brush. `wparam` is the control's DC.
    pub(super) fn on_ctl_color(&self, message: u32, wparam: Wparam, lparam: Lparam) -> Option<Lresult> {
        let is_edit = message == dialog_native::WM_CTLCOLORSTATIC && Self::is_edit(lparam as Hwnd);
        let kind = ctl_color_kind(message, is_edit)?;
        let dc = wparam as Hdc;
        let window_brush = self.inner.draw.window_brush.get();
        let input_brush = self.inner.draw.input_brush.get();
        let brush = match kind {
            CtlColor::DialogBackground => window_brush,
            CtlColor::Button => {
                dialog_native::gdi32::set_text_color(dc, DialogColors::text());
                dialog_native::gdi32::set_bk_color(dc, DialogColors::window());
                window_brush
            }
            CtlColor::Static | CtlColor::ReadOnlyEdit => {
                let id = dialog_native::user32::get_dlg_ctrl_id(lparam as Hwnd);
                let text = self.inner.content.static_text_color(self, id);
                dialog_native::gdi32::set_text_color(dc, text);
                let (background, brush) = if kind == CtlColor::ReadOnlyEdit {
                    (DialogColors::input(), input_brush)
                } else {
                    (DialogColors::window(), window_brush)
                };
                dialog_native::gdi32::set_bk_color(dc, background);
                brush
            }
            CtlColor::Input => {
                dialog_native::gdi32::set_text_color(dc, DialogColors::text_strong());
                dialog_native::gdi32::set_bk_color(dc, DialogColors::input());
                input_brush
            }
        };
        Some(brush as Lresult)
    }

    /// The `WM_SETCURSOR` case (lines 591 to 599): `content.wait_cursor()` -> `set_cursor(load_system_cursor(IDC_WAIT))` (an `Err` keeps the cursor),
    /// `SetWindowLongPtr(hwnd, DWLP_MSGRESULT, 1)` and answer `1`; else `0` (default processing).
    pub(super) fn on_set_cursor(&self, hwnd: Hwnd) -> Lresult {
        if !self.inner.content.wait_cursor() {
            return 0;
        }
        if let Ok(cursor) = user32::load_system_cursor(IDC_WAIT) {
            user32::set_cursor(cursor);
        }
        // SAFETY: DWLP_MSGRESULT is the dialog's result slot; the WM_SETCURSOR answer is the integer 1.
        unsafe { user32::set_window_long_ptr(hwnd, dialog_native::DWLP_MSGRESULT, 1) };
        1
    }
}

#[cfg(test)]
mod tests {
    use super::DrawState;

    // C#: NativeDialog.InitializeDialog starts with no owned brushes.
    #[test]
    fn draw_state_starts_empty() {
        let state = DrawState::new();
        assert_eq!(state.window_brush.get(), 0);
        assert_eq!(state.input_brush.get(), 0);
    }
}
