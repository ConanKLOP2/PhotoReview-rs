//! Port of `NativeDialog.cs` lines 819 to 972 (`SetTooltip`, `SetItemTooltips`, `EnsureTooltip`, `AddTool`, `TooltipToolCount`, `ItemTooltipText`, `ProvideTipText`,
//! `EnsureTipBuffer`, `ItemTipSubclassProc`, `OnItemTipMouseMove`), baseline `d92d6aae`. Unit W8-18j. Contract: `contracts/W8-18j.md`.
//!
//! One tooltip window per dialog, created on first use, without a visual style so the dark colours apply. A fixed-text tool (`set_tooltip`) hands the text to the
//! control; a per-row tool (`set_item_tooltips`) is ONE tool for a whole list box whose text is asked for with `TTN_GETDISPINFO` (`provide_tip_text` answers from
//! the row under the cursor), and the list box is subclassed so the tip pops again when the cursor moves to another row.
//!
//! `unsafe` here: the `TTTOOLINFOW` and `NMTTDISPINFOW` structures, the text buffer whose pointer the tooltip keeps until the next request, and the subclass
//! procedure. Layout asserts: `TTTOOLINFOW` `cbSize` is `TTTOOLINFOW_V2_SIZE` (64, the C# `ToolInfoSize`, even though the SDK struct is 72 bytes on x64);
//! `NMTTDISPINFOW.lpszText` is at offset 24 (D-W8B5-212).
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use super::core::NativeDialog;
use crate::dialogs::native::chrome::DialogColors;
use crate::dialogs::native::controls::basic::ListBoxControl;
use crate::host::pending_panic;
use crate::shell_input::native::{signed_high_word, signed_low_word};
use crate::win32::structs::{Hwnd, Lparam, Lresult, Wparam};
use crate::win32::{
    api::{kernel32, user32},
    comctl::common_controls,
    control_constants as cc, dialog_native, messages,
};
use windows::Win32::UI::Controls::{NMTTDISPINFOW, TTTOOLINFOW};
use windows::core::PWSTR;

/// C# `ItemTipSubclassId`: ASCII `"WT35"`.
pub const ITEM_TIP_SUBCLASS_ID: usize = 0x5754_3335;

/// C# `Func<int, string?>`: the tooltip text of a row index; `None` or `""` = no tooltip.
pub type ItemTipProvider = Rc<dyn Fn(i32) -> Option<String>>;

/// Tooltip state (C# `_tooltip`, `_tipBuffer`, `_tipCapacity`, `_itemTips`, `_lastTipItem`).
pub(super) struct TipState {
    /// C# `_tooltip`: the tooltip window (`0` until `ensure_tooltip`).
    pub(super) tooltip: Cell<Hwnd>,
    /// C# `_tipBuffer` + `_tipCapacity`: the UTF-16 text handed to the tooltip (`lpszText`). Its capacity is `tip_capacity(..)`; it must not be reallocated or
    /// freed while the tooltip may still read the pointer (until the next `TTN_GETDISPINFO` or `cleanup`).
    pub(super) tip_buffer: RefCell<Vec<u16>>,
    /// C# `_itemTips`: per list box the provider, in insertion order (C# `Dictionary` insertion order; `cleanup` walks it).
    pub(super) item_tips: RefCell<Vec<(Hwnd, ItemTipProvider)>>,
    /// C# `_lastTipItem`: the row the cursor was last over, per list box (`-1` initially).
    pub(super) last_tip_item: RefCell<HashMap<Hwnd, i32>>,
}

impl TipState {
    /// Everything empty, `tooltip = 0`.
    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self {
            tooltip: Cell::new(0),
            tip_buffer: RefCell::new(Vec::new()),
            item_tips: RefCell::new(Vec::new()),
            last_tip_item: RefCell::new(HashMap::new()),
        }
    }
}

/// C# `EnsureTipBuffer` capacity rule (pure): `max(chars, 256)` UTF-16 units.
pub fn tip_capacity(chars: usize) -> usize {
    chars.max(256)
}

impl NativeDialog {
    /// C# `SetTooltip(id, text)` (lines 824 to 836): a fixed tip for control `id`. Absent control or no tooltip window -> nothing. Else `add_tool(control,
    /// text pointer)`; the text is read by the tooltip during `TTM_ADDTOOL` (it copies it), so a local buffer is enough.
    pub fn set_tooltip(&self, id: i32, text: &str) {
        let control = self.item(id);
        if control == 0 || self.ensure_tooltip() == 0 {
            return;
        }
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        self.add_tool(control, wide.as_ptr() as isize);
    }

    /// C# `SetItemTooltips(id, textOf)` (lines 842 to 854): the per-row tip of list box `id`. Absent control or no tooltip window -> nothing. Else
    /// `item_tips[list] = provider` (replace in place when present, else push), `last_tip_item[list] = -1`, `add_tool(list, LPSTR_TEXTCALLBACKW)`,
    /// `set_window_subclass(list, item_tip_subclass_proc, ITEM_TIP_SUBCLASS_ID, self_ref_data())`.
    pub fn set_item_tooltips(&self, id: i32, text_of: ItemTipProvider) {
        let list = self.item(id);
        if list == 0 || self.ensure_tooltip() == 0 {
            return;
        }
        {
            let mut tips = self.inner.tips.item_tips.borrow_mut();
            if let Some((_, provider)) = tips.iter_mut().find(|(hwnd, _)| *hwnd == list) {
                *provider = text_of;
            } else {
                tips.push((list, text_of));
            }
        }
        self.inner.tips.last_tip_item.borrow_mut().insert(list, -1);
        self.add_tool(list, cc::LPSTR_TEXTCALLBACKW);
        // SAFETY: the subclass is removed by cleanup or WM_NCDESTROY before the dialog cookie is freed.
        let _ = unsafe {
            common_controls::set_window_subclass(
                list,
                item_tip_subclass_proc,
                ITEM_TIP_SUBCLASS_ID,
                self.self_ref_data(),
            )
        };
    }

    /// C# `EnsureTooltip()` (lines 856 to 879): returns the tooltip window, creating it on first use: `create_window_ex(WS_EX_TOPMOST, TOOLTIPS_CLASS, "",
    /// WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX, CW_USEDEFAULT x4, parent = hwnd, 0, module, 0)`; on success `set_window_theme(tooltip, "", "")` (no visual
    /// style: the colour messages are ignored under a theme), `TTM_SETTIPBKCOLOR` = `DialogColors::from_key("Dark.Tooltip")` (missing key -> `0`),
    /// `TTM_SETTIPTEXTCOLOR` = `text_strong()`, `TTM_SETMAXTIPWIDTH` = `dx(240)`. `0` when creation fails (nothing is retried until the next call).
    pub(super) fn ensure_tooltip(&self) -> Hwnd {
        let existing = self.inner.tips.tooltip.get();
        if existing != 0 {
            return existing;
        }
        // SAFETY: literal class and title, integer styles, and no creation parameter; the dialog owns the popup.
        let tip = unsafe {
            user32::create_window_ex(
                8,
                "tooltips_class32",
                "",
                0x8000_0000 | cc::TTS_ALWAYSTIP | cc::TTS_NOPREFIX,
                i32::MIN,
                i32::MIN,
                i32::MIN,
                i32::MIN,
                self.hwnd(),
                0,
                kernel32::get_module_handle().unwrap_or(0),
                0,
            )
        }
        .unwrap_or(0);
        if tip == 0 {
            return 0;
        }
        self.inner.tips.tooltip.set(tip);
        let _ = dialog_native::uxtheme::set_window_theme(tip, Some(""), Some(""));
        // SAFETY: these tooltip messages carry only integer colours or dimensions, no pointers.
        unsafe {
            dialog_native::user32::send_message(
                tip,
                cc::TTM_SETTIPBKCOLOR,
                DialogColors::from_key("Dark.Tooltip").unwrap_or(0) as usize,
                0,
            );
            dialog_native::user32::send_message(tip, cc::TTM_SETTIPTEXTCOLOR, DialogColors::text_strong() as usize, 0);
            dialog_native::user32::send_message(tip, cc::TTM_SETMAXTIPWIDTH, 0, self.dx(240) as isize);
        }
        tip
    }

    /// C# `AddTool(control, text)` (lines 881 to 891): `TTM_ADDTOOL` with a zeroed `TTTOOLINFOW` of `cbSize = 64`, `uFlags = TTF_IDISHWND | TTF_SUBCLASS`,
    /// `hwnd = dialog`, `uId = control`, `lpszText = text` (a pointer, or `LPSTR_TEXTCALLBACKW`).
    pub(super) fn add_tool(&self, control: Hwnd, text: isize) {
        let tip = self.inner.tips.tooltip.get();
        if tip == 0 {
            return;
        }
        let tool = TTTOOLINFOW {
            cbSize: cc::TTTOOLINFOW_V2_SIZE as u32,
            uFlags: windows::Win32::UI::Controls::TOOLTIP_FLAGS(cc::TTF_IDISHWND | cc::TTF_SUBCLASS),
            hwnd: windows::Win32::Foundation::HWND(self.hwnd() as *mut _),
            uId: control as usize,
            lpszText: PWSTR(text as *mut u16),
            ..Default::default()
        };
        // SAFETY: tool and the text it points to stay live during synchronous TTM_ADDTOOL; the tooltip copies fixed text.
        unsafe {
            dialog_native::user32::send_message(tip, cc::TTM_ADDTOOL, 0, &tool as *const _ as isize);
        }
    }

    /// C# `TooltipToolCount` (`internal`, test hook): `0` without a tooltip window, else `TTM_GETTOOLCOUNT`.
    pub fn tooltip_tool_count(&self) -> i32 {
        let tip = self.inner.tips.tooltip.get();
        if tip == 0 {
            return 0;
        }
        // SAFETY: TTM_GETTOOLCOUNT carries integer-only parameters.
        unsafe { dialog_native::user32::send_message(tip, cc::TTM_GETTOOLCOUNT, 0, 0) as i32 }
    }

    /// C# `ItemTooltipText(id, index)` (`internal`, test hook): the provider's text for that row of list box `id`; `None` when `id` has no provider.
    pub fn item_tooltip_text(&self, id: i32, index: i32) -> Option<String> {
        let list = self.item(id);
        let provider = self
            .inner
            .tips
            .item_tips
            .borrow()
            .iter()
            .find(|(hwnd, _)| *hwnd == list)
            .map(|(_, provider)| Rc::clone(provider));
        provider.and_then(|provider| provider(index))
    }

    /// C# `ProvideTipText(header)` (lines 900 to 922), the `TTN_GETDISPINFO` answer. `header` is the `lParam` (a `*mut NMTTDISPINFOW`). `control = idFrom` (the
    /// control window, `TTF_IDISHWND`). When `control` has a provider and `get_cursor_pos` succeeds: `screen_to_client(control, &mut point)`; `index =
    /// ListBoxControl::item_from_point(control, point.x, point.y)`; `last_tip_item[control] = index`; `text = index >= 0 ? provider(index) : None`. `text`
    /// defaults to `""`. Then `tip_buffer` is resized to at least `tip_capacity(len + 1)`, the text and a terminating `0` are copied in, and `lpszText` (offset 24 of
    /// the structure) is set to the buffer pointer. The provider runs with no borrow held.
    pub(super) fn provide_tip_text(&self, header: Lparam) {
        if header == 0 {
            return;
        }
        debug_assert_eq!(std::mem::offset_of!(NMTTDISPINFOW, lpszText), 24);
        // SAFETY: TTN_GETDISPINFO lParam points to a mutable NMTTDISPINFOW valid for this call.
        let info = unsafe { &mut *(header as *mut NMTTDISPINFOW) };
        let control = info.hdr.idFrom as Hwnd;
        let provider = self
            .inner
            .tips
            .item_tips
            .borrow()
            .iter()
            .find(|(hwnd, _)| *hwnd == control)
            .map(|(_, provider)| Rc::clone(provider));
        let mut text = None;
        if let (Some(provider), Some(mut point)) = (provider, crate::win32::comctl::user32::get_cursor_pos()) {
            let _ = dialog_native::user32::screen_to_client(control, &mut point);
            let index = ListBoxControl::item_from_point(control, point.x, point.y);
            self.inner.tips.last_tip_item.borrow_mut().insert(control, index);
            if index >= 0 {
                text = provider(index);
            }
        }
        let wide: Vec<u16> = text.unwrap_or_default().encode_utf16().collect();
        let needed = wide.len().wrapping_add(1);
        let mut buffer = self.inner.tips.tip_buffer.borrow_mut();
        if buffer.len() < needed {
            buffer.resize(tip_capacity(needed), 0);
        }
        if buffer.is_empty() {
            buffer.resize(tip_capacity(needed), 0);
        }
        buffer[..wide.len()].copy_from_slice(&wide);
        buffer[wide.len()] = 0;
        // SAFETY: this buffer is private to the dialog and is replaced only here; the tooltip reads it before the next request.
        info.lpszText = PWSTR(buffer.as_mut_ptr());
    }

    /// C# `OnItemTipMouseMove(list, x, y)` (lines 961 to 972): `index = item_from_point(list, x, y)`; when `last_tip_item[list]` exists and differs: store it and,
    /// when the tooltip exists, `TTM_POP` (the tip is shown again with the text of the new row).
    pub(super) fn on_item_tip_mouse_move(&self, list: Hwnd, x: i32, y: i32) {
        let index = ListBoxControl::item_from_point(list, x, y);
        let changed = {
            let mut last = self.inner.tips.last_tip_item.borrow_mut();
            if let Some(previous) = last.get_mut(&list) {
                if *previous != index {
                    *previous = index;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        let tip = self.inner.tips.tooltip.get();
        if changed && tip != 0 {
            // SAFETY: TTM_POP carries no pointer parameters.
            unsafe {
                dialog_native::user32::send_message(tip, cc::TTM_POP, 0, 0);
            }
        }
    }
}

/// C# `ItemTipSubclassProc` (lines 938 to 959), installed by `set_item_tooltips`. Under `pending_panic::guarded` (D-W8B5-207). `WM_MOUSEMOVE` with a live dialog
/// from `ref_data` -> `on_item_tip_mouse_move(hwnd, x, y)` with `x = shell_input::native::signed_low_word(lparam)`, `y = signed_high_word(lparam)` (signed words:
/// a cursor left of the control is negative); `WM_NCDESTROY` -> `remove_window_subclass(hwnd, item_tip_subclass_proc, id)`; always ends with `def_subclass_proc`.
///
/// # Safety
/// Only the OS calls it, as a subclass procedure installed by `set_item_tooltips`, with `ref_data` = the cookie of a live dialog.
pub(super) unsafe extern "system" fn item_tip_subclass_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    id: usize,
    ref_data: usize,
) -> Lresult {
    // SAFETY: the subclass receives a live dialog cookie; cleanup removes this procedure before freeing it.
    pending_panic::guarded(
        (),
        AssertUnwindSafe(|| {
            if message == messages::WM_MOUSEMOVE {
                // SAFETY: ref_data is this live dialog's cookie until the subclass is removed.
                if let Some(dialog) = unsafe { NativeDialog::from_cookie(ref_data as isize) } {
                    dialog.on_item_tip_mouse_move(hwnd, signed_low_word(lparam), signed_high_word(lparam));
                }
            }
            if message == messages::WM_NCDESTROY {
                let _ = common_controls::remove_window_subclass(hwnd, item_tip_subclass_proc, id);
            }
        }),
    );
    // SAFETY: forwarding unchanged parameters from this subclass callback to the next procedure.
    unsafe { common_controls::def_subclass_proc(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::{ITEM_TIP_SUBCLASS_ID, TipState, item_tip_subclass_proc};

    #[test]
    fn tip_state_starts_empty() {
        // C#: NativeDialog._tooltip / _tipBuffer / _itemTips / _lastTipItem initial state.
        let state = TipState::new();
        assert_eq!(state.tooltip.get(), 0);
        assert!(state.tip_buffer.borrow().is_empty());
        assert!(state.item_tips.borrow().is_empty());
        assert!(state.last_tip_item.borrow().is_empty());
    }

    #[test]
    #[ignore = "native: needs desktop"]
    fn item_tip_subclass_can_be_queried_after_set_item_tooltips() {
        // C#: NativeDialog.SetItemTooltips / ItemTipSubclassProc.
        use crate::dialogs::native::dialog::core::{DialogContent, NativeDialog};
        use crate::dialogs::native::template::DialogItem;
        use crate::win32::api::{kernel32, user32};
        use crate::win32::comctl::common_controls;
        use crate::win32::structs::Hwnd;

        struct Content;
        impl DialogContent for Content {
            fn title(&self) -> String {
                String::new()
            }
            fn items(&self) -> Vec<DialogItem> {
                Vec::new()
            }
            fn preferred_client_size(&self, _: &NativeDialog) -> (i32, i32) {
                (100, 100)
            }
            fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
        }
        struct Window(Hwnd);
        impl Drop for Window {
            fn drop(&mut self) {
                if self.0 != 0 {
                    let _ = user32::destroy_window(self.0);
                }
            }
        }
        let instance = kernel32::get_module_handle().unwrap_or(0);
        // SAFETY: literal standard class/title and integer parameters; Window destroys the result.
        let parent = Window(
            unsafe { user32::create_window_ex(0, "Static", "", 0, 0, 0, 200, 200, 0, 0, instance, 0) }
                .expect("create test parent"),
        );
        // SAFETY: literal standard class/title and integer parameters; the parent owns the child.
        let list = unsafe {
            user32::create_window_ex(0, "ListBox", "", 0x5000_0000, 0, 0, 100, 100, parent.0, 13, instance, 0)
        }
        .expect("create test list");
        let dialog = NativeDialog::new(std::rc::Rc::new(Content));
        dialog.inner.modal.hwnd.set(parent.0);
        assert!(!common_controls::remove_window_subclass(
            list,
            item_tip_subclass_proc,
            ITEM_TIP_SUBCLASS_ID
        ));
        dialog.set_item_tooltips(13, std::rc::Rc::new(|_| Some("tip".into())));
        assert!(common_controls::remove_window_subclass(
            list,
            item_tip_subclass_proc,
            ITEM_TIP_SUBCLASS_ID
        ));
    }
}
