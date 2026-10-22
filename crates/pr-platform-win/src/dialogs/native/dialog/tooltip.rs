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
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::core::NativeDialog;
use crate::win32::structs::{Hwnd, Lparam, Lresult, Wparam};

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
    pub(super) fn new() -> Self {
        todo!()
    }
}

/// C# `EnsureTipBuffer` capacity rule (pure): `max(chars, 256)` UTF-16 units.
pub fn tip_capacity(chars: usize) -> usize {
    todo!()
}

impl NativeDialog {
    /// C# `SetTooltip(id, text)` (lines 824 to 836): a fixed tip for control `id`. Absent control or no tooltip window -> nothing. Else `add_tool(control,
    /// text pointer)`; the text is read by the tooltip during `TTM_ADDTOOL` (it copies it), so a local buffer is enough.
    pub fn set_tooltip(&self, id: i32, text: &str) {
        todo!()
    }

    /// C# `SetItemTooltips(id, textOf)` (lines 842 to 854): the per-row tip of list box `id`. Absent control or no tooltip window -> nothing. Else
    /// `item_tips[list] = provider` (replace in place when present, else push), `last_tip_item[list] = -1`, `add_tool(list, LPSTR_TEXTCALLBACKW)`,
    /// `set_window_subclass(list, item_tip_subclass_proc, ITEM_TIP_SUBCLASS_ID, self_ref_data())`.
    pub fn set_item_tooltips(&self, id: i32, text_of: ItemTipProvider) {
        todo!()
    }

    /// C# `EnsureTooltip()` (lines 856 to 879): returns the tooltip window, creating it on first use: `create_window_ex(WS_EX_TOPMOST, TOOLTIPS_CLASS, "",
    /// WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX, CW_USEDEFAULT x4, parent = hwnd, 0, module, 0)`; on success `set_window_theme(tooltip, "", "")` (no visual
    /// style: the colour messages are ignored under a theme), `TTM_SETTIPBKCOLOR` = `DialogColors::from_key("Dark.Tooltip")` (missing key -> `0`),
    /// `TTM_SETTIPTEXTCOLOR` = `text_strong()`, `TTM_SETMAXTIPWIDTH` = `dx(240)`. `0` when creation fails (nothing is retried until the next call).
    pub(super) fn ensure_tooltip(&self) -> Hwnd {
        todo!()
    }

    /// C# `AddTool(control, text)` (lines 881 to 891): `TTM_ADDTOOL` with a zeroed `TTTOOLINFOW` of `cbSize = 64`, `uFlags = TTF_IDISHWND | TTF_SUBCLASS`,
    /// `hwnd = dialog`, `uId = control`, `lpszText = text` (a pointer, or `LPSTR_TEXTCALLBACKW`).
    pub(super) fn add_tool(&self, control: Hwnd, text: isize) {
        todo!()
    }

    /// C# `TooltipToolCount` (`internal`, test hook): `0` without a tooltip window, else `TTM_GETTOOLCOUNT`.
    pub fn tooltip_tool_count(&self) -> i32 {
        todo!()
    }

    /// C# `ItemTooltipText(id, index)` (`internal`, test hook): the provider's text for that row of list box `id`; `None` when `id` has no provider.
    pub fn item_tooltip_text(&self, id: i32, index: i32) -> Option<String> {
        todo!()
    }

    /// C# `ProvideTipText(header)` (lines 900 to 922), the `TTN_GETDISPINFO` answer. `header` is the `lParam` (a `*mut NMTTDISPINFOW`). `control = idFrom` (the
    /// control window, `TTF_IDISHWND`). When `control` has a provider and `get_cursor_pos` succeeds: `screen_to_client(control, &mut point)`; `index =
    /// ListBoxControl::item_from_point(control, point.x, point.y)`; `last_tip_item[control] = index`; `text = index >= 0 ? provider(index) : None`. `text`
    /// defaults to `""`. Then `tip_buffer` is resized to at least `tip_capacity(len + 1)`, the text and a terminating `0` are copied in, and `lpszText` (offset 24 of
    /// the structure) is set to the buffer pointer. The provider runs with no borrow held.
    pub(super) fn provide_tip_text(&self, header: Lparam) {
        todo!()
    }

    /// C# `OnItemTipMouseMove(list, x, y)` (lines 961 to 972): `index = item_from_point(list, x, y)`; when `last_tip_item[list]` exists and differs: store it and,
    /// when the tooltip exists, `TTM_POP` (the tip is shown again with the text of the new row).
    pub(super) fn on_item_tip_mouse_move(&self, list: Hwnd, x: i32, y: i32) {
        todo!()
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
    todo!()
}
