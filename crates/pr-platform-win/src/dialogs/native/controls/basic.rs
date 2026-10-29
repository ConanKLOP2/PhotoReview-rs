//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeControls.cs` lines 1-252 (baseline `d92d6aae`): `CommonControlsInit`, `ComboBoxControl` and
//! `ListBoxControl`. Unit W8-18f. Contract: `contracts/W8-18f.md`. The list view, progress, trackbar and check box wrappers are W8-18g (`list_tree`, `custom`).
//!
//! Every function wraps one or two messages of a standard window class and takes the control `Hwnd` as its first argument (C# static classes; here unit structs
//! with associated functions). `unsafe` here: `win32::dialog_native::user32::send_message` for the messages that carry a pointer (`CB_ADDSTRING`,
//! `CB_GETLBTEXT`, `LB_ADDSTRING`, `LB_GETTEXT`); every site has its `// SAFETY:`.
use std::sync::atomic::{AtomicBool, Ordering};

use crate::win32::control_constants::{
    CBS_DROPDOWNLIST, CBS_HASSTRINGS, ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_PROGRESS_CLASS, ICC_STANDARD_CLASSES,
    ICC_WIN95_CLASSES,
};
use crate::win32::dialog_native::{
    DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, WS_TABSTOP, WS_VSCROLL,
};
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
    // SAFETY: callers pass only messages whose wParam/lParam are plain values (this module tree).
    unsafe { crate::win32::dialog_native::user32::send_message(hwnd, message, wparam, lparam) }
}

/// C# `CommonControlsInit`: registers the common-control classes once per process, before a template that names them is created.
#[derive(Debug, Clone, Copy)]
pub struct CommonControlsInit;

impl CommonControlsInit {
    /// C# `Ensure()`: the first call runs `common_controls::init_common_controls_ex(COMMON_CONTROL_CLASSES)` and ignores its result; every later call (any
    /// thread) does nothing, also when the first call failed (C# `Interlocked.Exchange(ref s_done, 1)`: the flag is set BEFORE the call).
    pub fn ensure() {
        static DONE: AtomicBool = AtomicBool::new(false);
        Self::ensure_once(&DONE, crate::win32::comctl::common_controls::init_common_controls_ex);
    }

    /// The once-only core of `ensure` (pure with a fake `init`): returns `true` and runs `init(COMMON_CONTROL_CLASSES)` exactly when `done` was `false`.
    pub fn ensure_once(done: &AtomicBool, init: impl FnOnce(u32) -> bool) -> bool {
        if done.swap(true, Ordering::SeqCst) {
            false
        } else {
            let _ = init(COMMON_CONTROL_CLASSES);
            true
        }
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
        crate::dialogs::native::chrome::combo_list_window(combo)
    }

    /// C# `Add(combo, text)`: `CB_ADDSTRING`.
    pub fn add(combo: Hwnd, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: pointer to a NUL-terminated UTF-16 buffer, alive for the synchronous call.
        unsafe {
            crate::win32::dialog_native::user32::send_message(
                combo,
                crate::win32::control_constants::CB_ADDSTRING,
                0,
                wide.as_ptr() as isize,
            );
        }
    }

    /// C# `Clear(combo)`: `CB_RESETCONTENT`.
    pub fn clear(combo: Hwnd) {
        let _ = send_int(combo, crate::win32::control_constants::CB_RESETCONTENT, 0, 0);
    }

    /// C# `Count(combo)`: `CB_GETCOUNT`.
    pub fn count(combo: Hwnd) -> i32 {
        send_int(combo, crate::win32::control_constants::CB_GETCOUNT, 0, 0) as i32
    }

    /// C# `SelectedIndex(combo)`: `CB_GETCURSEL` (`-1` = none, `CB_ERR`).
    pub fn selected_index(combo: Hwnd) -> i32 {
        send_int(combo, crate::win32::control_constants::CB_GETCURSEL, 0, 0) as i32
    }

    /// C# `Select(combo, index)`: `CB_SETCURSEL` (`-1` clears the selection).
    pub fn select(combo: Hwnd, index: i32) {
        let _ = send_int(combo, crate::win32::control_constants::CB_SETCURSEL, index as usize, 0);
    }

    /// C# `IsDropped(combo)`: `CB_GETDROPPEDSTATE != 0`.
    pub fn is_dropped(combo: Hwnd) -> bool {
        send_int(combo, crate::win32::control_constants::CB_GETDROPPEDSTATE, 0, 0) != 0
    }

    /// C# `Drop(combo, show)`: `CB_SHOWDROPDOWN` with `1` or `0`.
    pub fn drop(combo: Hwnd, show: bool) {
        let _ = send_int(
            combo,
            crate::win32::control_constants::CB_SHOWDROPDOWN,
            usize::from(show),
            0,
        );
    }

    /// C# `TextAt(combo, index)`: `CB_GETLBTEXTLEN`; `<= 0` gives `""`; else a buffer of `length + 1` UTF-16 units, `CB_GETLBTEXT`, and the first `length`
    /// units as the string (the length reported BEFORE the copy, as C#).
    pub fn text_at(combo: Hwnd, index: i32) -> String {
        let length = send_int(
            combo,
            crate::win32::control_constants::CB_GETLBTEXTLEN,
            index as usize,
            0,
        ) as i32;
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        // SAFETY: pointer to a length+1 unit buffer, alive for the synchronous call; the control writes at most length+1 units.
        unsafe {
            crate::win32::dialog_native::user32::send_message(
                combo,
                crate::win32::control_constants::CB_GETLBTEXT,
                index as usize,
                buffer.as_mut_ptr() as isize,
            );
        }
        String::from_utf16_lossy(&buffer[..length as usize])
    }
}

/// `DrawText` flags of a list row's badge (C# `NativeControls.cs` `DtSingleLine | DtVCenter | DtCenter | DtNoPrefix`). `DT_NOPREFIX` keeps a `&` literal: without it
/// the settings navigation showed "Mouse & zoom" as "Mouse _zoom" (the `&` is a mnemonic prefix that underlines the next character).
pub const BADGE_TEXT_FLAGS: u32 = DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX;
/// `DrawText` flags of a list row's title and subtitle (C# `DtSingleLine | DtVCenter | DtNoPrefix | DtEndEllipsis`).
pub const ROW_TEXT_FLAGS: u32 = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS;

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
    let mut right = rect.right.saturating_sub(pad_x);
    let badge = badge_extent.map(|extent| {
        let height = rect.bottom.saturating_sub(rect.top);
        let badge_height = height.saturating_sub(4).min(extent.y.saturating_add(2));
        let badge_width = extent.x.saturating_add(pad_x);
        let left = right.saturating_sub(badge_width);
        let top = rect.top.saturating_add(height.saturating_sub(badge_height) / 2);
        let bottom = top.saturating_add(badge_height);
        right = left.saturating_sub(pad_x);
        Rect {
            left,
            top,
            right: rect.right.saturating_sub(pad_x),
            bottom,
        }
    });
    let left = rect.left.saturating_add(pad_x);
    let mut text = Rect {
        left,
        top: rect.top,
        right: left.max(right),
        bottom: rect.bottom,
    };
    let subtitle = if has_subtitle {
        let middle = rect.top.saturating_add(rect.bottom.saturating_sub(rect.top) / 2);
        let sub = Rect {
            left: text.left,
            top: middle,
            right: text.right,
            bottom: rect.bottom,
        };
        text.bottom = middle;
        Some(sub)
    } else {
        None
    };
    RowLayout { badge, text, subtitle }
}

/// C# `ListBoxControl`: a list box (single or multiple selection), optionally owner-drawn with a badge on the right.
#[derive(Debug, Clone, Copy)]
pub struct ListBoxControl;

impl ListBoxControl {
    /// C# `Count(list)`: `LB_GETCOUNT`.
    pub fn count(list: Hwnd) -> i32 {
        send_int(list, crate::win32::control_constants::LB_GETCOUNT, 0, 0) as i32
    }

    /// C# `Add(list, text)`: `LB_ADDSTRING`, the index of the new row.
    pub fn add(list: Hwnd, text: &str) -> i32 {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: pointer to a NUL-terminated UTF-16 buffer, alive for the synchronous call.
        unsafe { crate::win32::dialog_native::user32::send_message(list, 0x0180, 0, wide.as_ptr() as isize) as i32 }
    }

    /// C# `Clear(list)`: `LB_RESETCONTENT`.
    pub fn clear(list: Hwnd) {
        let _ = send_int(list, 0x0184, 0, 0);
    }

    /// C# `SelectedIndex(list)`: `LB_GETCURSEL` of a single-selection list, `-1` when none.
    pub fn selected_index(list: Hwnd) -> i32 {
        send_int(list, crate::win32::control_constants::LB_GETCURSEL, 0, 0) as i32
    }

    /// C# `SelectSingle(list, index)`: `LB_SETCURSEL`.
    pub fn select_single(list: Hwnd, index: i32) {
        let _ = send_int(list, crate::win32::control_constants::LB_SETCURSEL, index as usize, 0);
    }

    /// C# `IsSelected(list, index)`: `LB_GETSEL > 0`.
    pub fn is_selected(list: Hwnd, index: i32) -> bool {
        send_int(list, crate::win32::control_constants::LB_GETSEL, index as usize, 0) > 0
    }

    /// C# `SetSelected(list, index, selected)`: `LB_SETSEL` with `wParam = 1/0`, `lParam = index` (multiple-selection list).
    pub fn set_selected(list: Hwnd, index: i32, selected: bool) {
        let _ = send_int(
            list,
            crate::win32::control_constants::LB_SETSEL,
            usize::from(selected),
            index as isize,
        );
    }

    /// C# `SelectAll(list, selected)`: only when `count > 0`: `LB_SELITEMRANGE` with `wParam = 1/0` and `lParam = (int)((ushort)(count - 1) << 16)` sign-extended
    /// (first = 0, last = `count - 1` truncated to 16 bits; for a last index of `0x8000` or more the C# `int` is negative and so is `lParam`).
    pub fn select_all(list: Hwnd, selected: bool) {
        let count = Self::count(list);
        if count > 0 {
            let _ = send_int(
                list,
                crate::win32::control_constants::LB_SELITEMRANGE,
                usize::from(selected),
                Self::select_range_lparam(count),
            );
        }
    }

    /// The `lParam` of `select_all` (pure): `(((count - 1) as u16 as u32) << 16) as i32 as isize`.
    pub fn select_range_lparam(count: i32) -> isize {
        ((((count - 1) as u16 as u32) << 16) as i32) as isize
    }

    /// C# `SelectedIndices(list)`: every `i` in `0..count` with `is_selected`, ascending.
    pub fn selected_indices(list: Hwnd) -> Vec<i32> {
        (0..Self::count(list)).filter(|&i| Self::is_selected(list, i)).collect()
    }

    /// C# `TextAt(list, index)`: like `ComboBoxControl::text_at` with `LB_GETTEXTLEN` / `LB_GETTEXT`.
    pub fn text_at(list: Hwnd, index: i32) -> String {
        let length = send_int(list, crate::win32::control_constants::LB_GETTEXTLEN, index as usize, 0) as i32;
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        // SAFETY: pointer to a length+1 unit buffer, alive for the synchronous call; the control writes at most length+1 units.
        unsafe {
            crate::win32::dialog_native::user32::send_message(
                list,
                crate::win32::control_constants::LB_GETTEXT,
                index as usize,
                buffer.as_mut_ptr() as isize,
            );
        }
        String::from_utf16_lossy(&buffer[..length as usize])
    }

    /// C# `ItemFromPoint(list, x, y)`: `LB_ITEMFROMPOINT` with `lParam = (y << 16) | (x & 0xFFFF)` (zero-extended `uint`); the result is `packed = (uint)
    /// result`; `-1` when `packed >> 16 != 0` (outside the item area), else `packed & 0xFFFF`.
    pub fn item_from_point(list: Hwnd, x: i32, y: i32) -> i32 {
        Self::decode_item_from_point(send_int(
            list,
            crate::win32::control_constants::LB_ITEMFROMPOINT,
            0,
            Self::encode_item_from_point(x, y),
        ) as u32)
    }

    /// The pure decoding of the `LB_ITEMFROMPOINT` answer (see `item_from_point`).
    pub fn decode_item_from_point(packed: u32) -> i32 {
        if packed >> 16 != 0 {
            -1
        } else {
            (packed & 0xffff) as i32
        }
    }

    /// The pure encoding of the `lParam` of `LB_ITEMFROMPOINT`: `((y as u32) << 16) | ((x as u32) & 0xFFFF)` as a zero-extended `isize`.
    pub fn encode_item_from_point(x: i32, y: i32) -> isize {
        ((((y as u32) << 16) | ((x as u32) & 0xffff)) as usize) as isize
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
        use crate::dialogs::native::chrome::DialogColors;
        use crate::win32::{
            comctl::{gdi32 as cc_gdi32, user32 as cc_user32},
            control_constants as cc,
            dialog_native::gdi32,
        };
        let rect = draw.rcItem;
        let state = draw.itemState.0;
        let selected = state & cc::ODS_SELECTED != 0;
        let back = if selected {
            DialogColors::from_key("Dark.Selection").unwrap_or(0)
        } else {
            DialogColors::input()
        };
        let brush = gdi32::create_solid_brush(back);
        let _ = cc_user32::fill_rect(draw.hDC.0 as isize, &rect, brush);
        gdi32::delete_object(brush);
        if (draw.itemID as i32) < 0 {
            if state & cc::ODS_FOCUS != 0 {
                let _ = cc_user32::draw_focus_rect(draw.hDC.0 as isize, &rect);
            }
            return;
        }
        let old_font = if font != 0 {
            Some(gdi32::select_object(draw.hDC.0 as isize, font))
        } else {
            None
        };
        let _ = cc_gdi32::set_bk_mode(draw.hDC.0 as isize, 1);
        let nonempty_badge = badge.filter(|s| !s.is_empty());
        let mut badge_extent = None;
        if let Some(value) = nonempty_badge {
            let extent = gdi32::get_text_extent_point32(draw.hDC.0 as isize, value).unwrap_or(Point { x: 0, y: 0 });
            badge_extent = Some(extent);
        }
        let layout = row_layout(rect, badge_extent, pad_x, subtitle.is_some_and(|s| !s.is_empty()));
        if let (Some(value), Some(badge_rect)) = (nonempty_badge, layout.badge) {
            let color = badge_color.unwrap_or_else(|| DialogColors::from_key("Dark.BorderStrong").unwrap_or(0));
            let pen = cc_gdi32::create_pen(0, 1, color);
            let fill = gdi32::create_solid_brush(
                DialogColors::from_key(if badge_color.is_some() {
                    "Dark.Panel"
                } else {
                    "Dark.Surface"
                })
                .unwrap_or(0),
            );
            let old_pen = gdi32::select_object(draw.hDC.0 as isize, pen);
            let old_brush = gdi32::select_object(draw.hDC.0 as isize, fill);
            let _ = cc_gdi32::round_rect(
                draw.hDC.0 as isize,
                badge_rect.left,
                badge_rect.top,
                badge_rect.right,
                badge_rect.bottom,
                badge_rect.bottom - badge_rect.top,
                badge_rect.bottom - badge_rect.top,
            );
            let _ = gdi32::select_object(draw.hDC.0 as isize, old_pen);
            let _ = gdi32::select_object(draw.hDC.0 as isize, old_brush);
            gdi32::delete_object(pen);
            gdi32::delete_object(fill);
            let _ = gdi32::set_text_color(
                draw.hDC.0 as isize,
                badge_color.unwrap_or_else(DialogColors::text_secondary),
            );
            let mut badge_rect = badge_rect;
            let _ = crate::win32::dialog_native::user32::draw_text(
                draw.hDC.0 as isize,
                value,
                &mut badge_rect,
                BADGE_TEXT_FLAGS,
            );
        }
        if let Some(value) = subtitle.filter(|s| !s.is_empty()) {
            if let Some(sub_rect) = layout.subtitle {
                let _ = gdi32::set_text_color(
                    draw.hDC.0 as isize,
                    DialogColors::from_key("Dark.TextMuted").unwrap_or(0),
                );
                let mut sub_rect = sub_rect;
                let _ = crate::win32::dialog_native::user32::draw_text(
                    draw.hDC.0 as isize,
                    value,
                    &mut sub_rect,
                    ROW_TEXT_FLAGS,
                );
            }
        }
        let _ = gdi32::set_text_color(draw.hDC.0 as isize, DialogColors::text_strong());
        let mut text_rect = layout.text;
        let _ =
            crate::win32::dialog_native::user32::draw_text(draw.hDC.0 as isize, text, &mut text_rect, ROW_TEXT_FLAGS);
        if let Some(old) = old_font {
            let _ = gdi32::select_object(draw.hDC.0 as isize, old);
        }
        if state & cc::ODS_FOCUS != 0 {
            let _ = cc_user32::draw_focus_rect(draw.hDC.0 as isize, &rect);
        }
    }
}
