//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeControls.cs` lines 1-252 (baseline `d92d6aae`): `CommonControlsInit`, `ComboBoxControl` and
//! `ListBoxControl`. Unit W8-18f. Contract: `contracts/W8-18f.md`. The list view, progress, trackbar and check box wrappers are W8-18g (`list_tree`, `custom`).
//!
//! Every function wraps one or two messages of a standard window class and takes the control `Hwnd` as its first argument (C# static classes; here unit structs
//! with associated functions). `unsafe` here: `win32::dialog_native::user32::send_message` for the messages that carry a pointer (`CB_ADDSTRING`,
//! `CB_GETLBTEXT`, `LB_ADDSTRING`, `LB_GETTEXT`); every site has its `// SAFETY:`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::sync::atomic::AtomicBool;

use crate::win32::control_constants::{
    CBS_DROPDOWNLIST, CBS_HASSTRINGS, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_PROGRESS_CLASS, ICC_STANDARD_CLASSES,
    ICC_WIN95_CLASSES,
};
use crate::win32::dialog_native::{WS_TABSTOP, WS_VSCROLL};
use crate::win32::structs::{Hgdi, Hwnd, Lparam, Lresult, Point, Rect, Wparam};

/// The class set `CommonControlsInit::ensure` asks for: `ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES | ICC_PROGRESS_CLASS | ICC_WIN95_CLASSES |
/// ICC_STANDARD_CLASSES` (listview, progress, trackbar, tooltip, standard).
pub const COMMON_CONTROL_CLASSES: u32 =
    ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES | ICC_PROGRESS_CLASS | ICC_WIN95_CLASSES | ICC_STANDARD_CLASSES;

/// A message whose `wParam` and `lParam` are plain values (no pointer): the helper of every integer-only call in `controls::*`. Shared with `list_tree` and
/// `custom` (W8-18g uses it after W8-18f merges, plan edge 18g -> 18f).
///
/// SAFETY of the helper (written once in the body): callers in this module tree pass only messages of the standard classes whose parameters are values.
pub(super) fn send_int(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
    todo!()
}

/// C# `CommonControlsInit`: registers the common-control classes once per process, before a template that names them is created.
#[derive(Debug, Clone, Copy)]
pub struct CommonControlsInit;

impl CommonControlsInit {
    /// C# `Ensure()`: the first call runs `common_controls::init_common_controls_ex(COMMON_CONTROL_CLASSES)` and ignores its result; every later call (any
    /// thread) does nothing, also when the first call failed (C# `Interlocked.Exchange(ref s_done, 1)`: the flag is set BEFORE the call).
    pub fn ensure() {
        todo!()
    }

    /// The once-only core of `ensure` (pure with a fake `init`): returns `true` and runs `init(COMMON_CONTROL_CLASSES)` exactly when `done` was `false`.
    pub fn ensure_once(done: &AtomicBool, init: impl FnOnce(u32) -> bool) -> bool {
        todo!()
    }
}

/// C# `ComboBoxControl` (`CBS_DROPDOWNLIST`: pick one item of a drop-down list).
#[derive(Debug, Clone, Copy)]
pub struct ComboBoxControl;

impl ComboBoxControl {
    /// C# `DefaultStyle` = `CBS_DROPDOWNLIST | CBS_HASSTRINGS | WS_TABSTOP | WS_VSCROLL`.
    pub const DEFAULT_STYLE: u32 = CBS_DROPDOWNLIST | CBS_HASSTRINGS | WS_TABSTOP | WS_VSCROLL;

    /// C# `ListWindow(combo)`: the drop-down list window (its own list box, which also needs the dark theme); `0` for `combo == 0` or when it cannot be read.
    /// Forwards to `chrome::combo_list_window`.
    pub fn list_window(combo: Hwnd) -> Hwnd {
        todo!()
    }

    /// C# `Add(combo, text)`: `CB_ADDSTRING`.
    pub fn add(combo: Hwnd, text: &str) {
        todo!()
    }

    /// C# `Clear(combo)`: `CB_RESETCONTENT`.
    pub fn clear(combo: Hwnd) {
        todo!()
    }

    /// C# `Count(combo)`: `CB_GETCOUNT`.
    pub fn count(combo: Hwnd) -> i32 {
        todo!()
    }

    /// C# `SelectedIndex(combo)`: `CB_GETCURSEL` (`-1` = none, `CB_ERR`).
    pub fn selected_index(combo: Hwnd) -> i32 {
        todo!()
    }

    /// C# `Select(combo, index)`: `CB_SETCURSEL` (`-1` clears the selection).
    pub fn select(combo: Hwnd, index: i32) {
        todo!()
    }

    /// C# `IsDropped(combo)`: `CB_GETDROPPEDSTATE != 0`.
    pub fn is_dropped(combo: Hwnd) -> bool {
        todo!()
    }

    /// C# `Drop(combo, show)`: `CB_SHOWDROPDOWN` with `1` or `0`.
    pub fn drop(combo: Hwnd, show: bool) {
        todo!()
    }

    /// C# `TextAt(combo, index)`: `CB_GETLBTEXTLEN`; `<= 0` gives `""`; else a buffer of `length + 1` UTF-16 units, `CB_GETLBTEXT`, and the first `length`
    /// units as the string (the length reported BEFORE the copy, as C#).
    pub fn text_at(combo: Hwnd, index: i32) -> String {
        todo!()
    }
}

/// Where a list row puts its parts (the pure geometry of C# `ListBoxControl.DrawItem`, see `row_layout`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowLayout {
    /// The pill of the badge (right edge); `None` without a badge.
    pub badge: Option<Rect>,
    /// The title text rectangle (its bottom is the middle line when there is a subtitle).
    pub text: Rect,
    /// The subtitle rectangle (lower half); `None` without a subtitle.
    pub subtitle: Option<Rect>,
}

/// The pure geometry of `ListBoxControl::draw_item`. `badge_extent` is `Some(extent of the badge text)` when a non-empty badge is drawn (a failed measure
/// is `(0, 0)`, as C# whose result is ignored). Arithmetic as C#: `right = rect.right - pad_x`; with a badge: `height = rect.bottom - rect.top`,
/// `badge_height = min(height - 4, extent.y + 2)`, `badge_width = extent.x + pad_x`, `badge_left = right - badge_width`, `badge_top = rect.top + (height -
/// badge_height) / 2`, badge rect `(badge_left, badge_top, right, badge_top + badge_height)`, then `right = badge_left - pad_x`. Text rect: `(rect.left +
/// pad_x, rect.top, max(rect.left + pad_x, right), rect.bottom)`. With a subtitle: `middle = rect.top + (rect.bottom - rect.top) / 2`, subtitle rect `(text.left,
/// middle, text.right, rect.bottom)` and `text.bottom = middle`.
pub fn row_layout(rect: Rect, badge_extent: Option<Point>, pad_x: i32, has_subtitle: bool) -> RowLayout {
    todo!()
}

/// C# `ListBoxControl`: a list box (single or multiple selection), optionally owner-drawn with a badge on the right.
#[derive(Debug, Clone, Copy)]
pub struct ListBoxControl;

impl ListBoxControl {
    /// C# `Count(list)`: `LB_GETCOUNT`.
    pub fn count(list: Hwnd) -> i32 {
        todo!()
    }

    /// C# `Add(list, text)`: `LB_ADDSTRING`, the index of the new row.
    pub fn add(list: Hwnd, text: &str) -> i32 {
        todo!()
    }

    /// C# `Clear(list)`: `LB_RESETCONTENT`.
    pub fn clear(list: Hwnd) {
        todo!()
    }

    /// C# `SelectedIndex(list)`: `LB_GETCURSEL` of a single-selection list, `-1` when none.
    pub fn selected_index(list: Hwnd) -> i32 {
        todo!()
    }

    /// C# `SelectSingle(list, index)`: `LB_SETCURSEL`.
    pub fn select_single(list: Hwnd, index: i32) {
        todo!()
    }

    /// C# `IsSelected(list, index)`: `LB_GETSEL > 0`.
    pub fn is_selected(list: Hwnd, index: i32) -> bool {
        todo!()
    }

    /// C# `SetSelected(list, index, selected)`: `LB_SETSEL` with `wParam = 1/0`, `lParam = index` (multiple-selection list).
    pub fn set_selected(list: Hwnd, index: i32, selected: bool) {
        todo!()
    }

    /// C# `SelectAll(list, selected)`: only when `count > 0`: `LB_SELITEMRANGE` with `wParam = 1/0` and `lParam = (int)((ushort)(count - 1) << 16)` sign-extended
    /// (first = 0, last = `count - 1` truncated to 16 bits; for a last index of `0x8000` or more the C# `int` is negative and so is `lParam`).
    pub fn select_all(list: Hwnd, selected: bool) {
        todo!()
    }

    /// The `lParam` of `select_all` (pure): `(((count - 1) as u16 as u32) << 16) as i32 as isize`.
    pub fn select_range_lparam(count: i32) -> isize {
        todo!()
    }

    /// C# `SelectedIndices(list)`: every `i` in `0..count` with `is_selected`, ascending.
    pub fn selected_indices(list: Hwnd) -> Vec<i32> {
        todo!()
    }

    /// C# `TextAt(list, index)`: like `ComboBoxControl::text_at` with `LB_GETTEXTLEN` / `LB_GETTEXT`.
    pub fn text_at(list: Hwnd, index: i32) -> String {
        todo!()
    }

    /// C# `ItemFromPoint(list, x, y)`: `LB_ITEMFROMPOINT` with `lParam = (y << 16) | (x & 0xFFFF)` (zero-extended `uint`); the result is `packed = (uint)
    /// result`; `-1` when `packed >> 16 != 0` (outside the item area), else `packed & 0xFFFF`.
    pub fn item_from_point(list: Hwnd, x: i32, y: i32) -> i32 {
        todo!()
    }

    /// The pure decoding of the `LB_ITEMFROMPOINT` answer (see `item_from_point`).
    pub fn decode_item_from_point(packed: u32) -> i32 {
        todo!()
    }

    /// The pure encoding of the `lParam` of `LB_ITEMFROMPOINT`: `((y as u32) << 16) | ((x as u32) & 0xFFFF)` as a zero-extended `isize`.
    pub fn encode_item_from_point(x: i32, y: i32) -> isize {
        todo!()
    }

    /// C# `DrawItem(byte* drawItem, text, badge, font, padX, subtitle, badgeColor)`: paints one owner-draw row of the list: background (selected: `Dark.Selection`,
    /// else `DialogColors::input()`), text on the left, badge pill on the right, focus rectangle. The caller (the dialog's `WM_DRAWITEM`, W8-18k) turns `lParam`
    /// into the `&DRAWITEMSTRUCT` under its own SAFETY.
    ///
    /// Order (C#): `selected = itemState & ODS_SELECTED`; fill `rcItem` with a solid brush of the background colour (create, `FillRect`, delete); when
    /// `itemID as i32 < 0` (an empty list or the area below the last row, the field is a `UINT` holding `-1`): only the focus rectangle when `ODS_FOCUS`, then
    /// return. Else: select `font` when non-zero, `SetBkMode(TRANSPARENT)`; a non-empty `badge`: measure it (`GetTextExtentPoint32`, a failure is `(0, 0)`), pen
    /// `PS_SOLID` 1 px of `badge_color` or `Dark.BorderStrong`, fill brush `Dark.Panel` when `badge_color` is given else `Dark.Surface`, `RoundRect` with the
    /// ellipse size = badge height, restore and delete pen and brush, text colour `badge_color` or `DialogColors::text_secondary()`, `DrawText` centred
    /// (`DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX`); the subtitle (colour `Dark.TextMuted`, `DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX |
    /// DT_END_ELLIPSIS`) before the title; the title (colour `DialogColors::text_strong()`, same flags); restore the font; the focus rectangle when `ODS_FOCUS`.
    #[allow(clippy::too_many_arguments)] // mirrors the C# signature
    pub fn draw_item(
        draw: &windows::Win32::UI::Controls::DRAWITEMSTRUCT,
        text: &str,
        badge: Option<&str>,
        font: Hgdi,
        pad_x: i32,
        subtitle: Option<&str>,
        badge_color: Option<u32>,
    ) {
        todo!()
    }
}
