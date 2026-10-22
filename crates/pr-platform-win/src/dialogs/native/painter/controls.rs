//! Port of `NativeDialogChromePainter.cs` lines 335-657 (baseline `d92d6aae`): the part of the painter that is shared by all kinds (double buffering, the
//! drawing helpers, the list view row correction) and the push button. Unit W8-18m. Contract: `contracts/W8-18m.md`.
//!
//! The pure decisions (which colour, which rectangle) are plain functions with plain inputs ([`push_button_draw`]); the GDI part only reads the window state and
//! calls them. Every GDI object created here (pens, brushes, memory DC, bitmap) is released on every path by a guard (trap T1); a font selected into the DC is
//! restored. The drawing of the other kinds is `painter::{widgets, frame}` (W8-18n) and is reached through [`draw`].

use windows::Win32::Foundation::COLORREF;
use windows::Win32::UI::Controls::{NMCUSTOMDRAW_DRAW_STATE_FLAGS, NMLVCUSTOMDRAW};

use super::geometry::{
    ChromePaintKind, UiState, adjust_custom_draw, color, keys, paint_scale, scaled, ui_state_from_flags,
};
use super::{PaintState, frame as frame_module, widgets};
use crate::dialogs::native::chrome::DialogColors;
use crate::win32::api::user32::{get_client_rect, get_dpi_for_window, get_window_long_ptr, is_window_visible};
use crate::win32::comctl::gdi32::{create_pen, round_rect, set_bk_mode};
use crate::win32::comctl::user32::{draw_focus_rect, fill_rect};
use crate::win32::control_constants::{BM_GETCHECK, BST_CHECKED, CDIS_SELECTED, LVIS_SELECTED, LVM_GETITEMSTATE};
use crate::win32::dialog_chrome::{
    BM_GETSTATE, BS_MULTILINE, BS_PUSHLIKE, BS_TYPEMASK, BST_FOCUS, BST_PUSHED, DT_HIDEPREFIX, PaintSession, SRCCOPY,
    WM_QUERYUISTATE, begin_paint, bit_blt, create_compatible_bitmap, create_compatible_dc, delete_dc, end_paint,
};
use crate::win32::dialog_native::gdi32::{
    create_solid_brush, delete_object, get_text_extent_point32, select_object, set_text_color,
};
use crate::win32::dialog_native::user32::{
    draw_text as os_draw_text, get_dc, get_window_text, is_window, is_window_enabled, release_dc, send_message,
};
use crate::win32::dialog_native::{
    DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, TRANSPARENT, WM_GETFONT,
};
use crate::win32::messages::GWL_STYLE;
use crate::win32::structs::{Hdc, Hgdi, Hwnd, Rect};

// ---- guards (trap T1: every GDI object is released on every path, also when a drawing panics) -------------------------------------------------------------------

/// Deletes a GDI object (pen, brush, bitmap) when dropped; `0` is ignored.
struct Owned(Hgdi);

impl Drop for Owned {
    fn drop(&mut self) {
        if self.0 != 0 {
            delete_object(self.0);
        }
    }
}

/// Selects `old` back into `hdc` when dropped (`0` = nothing was selected).
struct Reselect {
    hdc: Hdc,
    old: Hgdi,
}

impl Drop for Reselect {
    fn drop(&mut self) {
        if self.old != 0 {
            select_object(self.hdc, self.old);
        }
    }
}

/// The memory DC and bitmap of [`paint_to`]: the old bitmap is selected back, the bitmap and the DC are deleted on drop.
struct MemoryBuffer {
    dc: Hdc,
    bitmap: Hgdi,
    old: Hgdi,
}

impl Drop for MemoryBuffer {
    fn drop(&mut self) {
        select_object(self.dc, self.old);
        delete_object(self.bitmap);
        delete_dc(self.dc);
    }
}

/// `BeginPaint` session: `EndPaint` on drop.
struct PaintGuard<'a> {
    hwnd: Hwnd,
    session: &'a PaintSession,
}

impl Drop for PaintGuard<'_> {
    fn drop(&mut self) {
        end_paint(self.hwnd, self.session);
    }
}

/// Releases a window DC on drop.
struct DcGuard {
    hwnd: Hwnd,
    hdc: Hdc,
}

impl Drop for DcGuard {
    fn drop(&mut self) {
        release_dc(self.hwnd, self.hdc);
    }
}

/// C# `AdjustListViewCustomDraw(customDraw, result)` (lines 340-366) on the typed structure (D-W8B5-406): reads `dwDrawStage`, `dwItemSpec` (the row),
/// `hdr.hwndFrom` (the list view), writes `uItemState` (clears `CDIS_SELECTED`), `clrText` and `clrTextBk`; the decision is
/// `geometry::adjust_custom_draw(stage, result, selected)` with `selected` = `(send_message(list, LVM_GETITEMSTATE, row, LVIS_SELECTED) as u32 & LVIS_SELECTED)
/// != 0` (the system flag is not trusted in the classic look: the C# comment saw `0x201` for an unselected row). Applies the decision and returns its `result`.
pub fn adjust_list_view_custom_draw(custom_draw: &mut NMLVCUSTOMDRAW, result: isize) -> isize {
    let stage = custom_draw.nmcd.dwDrawStage.0;
    let row = custom_draw.nmcd.dwItemSpec;
    let list = custom_draw.nmcd.hdr.hwndFrom.0 as isize;
    let adjustment = adjust_custom_draw(stage, result, || {
        // The flag CDIS_SELECTED of the notification is not trusted in the classic look (the C# saw 0x201 for an unselected row): ask the list view.
        // `hwndFrom == 0` is simply "not selected".
        list != 0 && {
            // SAFETY: `LVM_GETITEMSTATE` takes the row in `wparam` and the state mask in `lparam`: values only.
            let state = unsafe { send_message(list, LVM_GETITEMSTATE, row, LVIS_SELECTED as isize) };
            (state as u32) & LVIS_SELECTED != 0
        }
    });
    if adjustment.clear_selected_flag {
        custom_draw.nmcd.uItemState = NMCUSTOMDRAW_DRAW_STATE_FLAGS(custom_draw.nmcd.uItemState.0 & !CDIS_SELECTED);
    }
    if let Some(text) = adjustment.text_color {
        custom_draw.clrText = COLORREF(text);
    }
    if let Some(back) = adjustment.back_color {
        custom_draw.clrTextBk = COLORREF(back);
    }
    adjustment.result
}

/// C# `RepaintNow(hwnd, state)` (lines 394-415): paints over what the system control painted by itself. Nothing when the window is dead or not visible, or when
/// `get_dc` gives `0`; else `paint_to(hwnd, dc, state)` and `release_dc` on every path.
pub(super) fn repaint_now(hwnd: Hwnd, state: &PaintState) {
    if !is_window(hwnd) || !is_window_visible(hwnd) {
        return;
    }
    let hdc = get_dc(hwnd);
    if hdc == 0 {
        return;
    }
    let _release = DcGuard { hwnd, hdc };
    paint_to(hwnd, hdc, state);
}

/// C# `PaintWindow(hwnd, state)` (lines 417-432): `begin_paint`, `paint_to(hwnd, session.hdc, state)` when a DC was given, `end_paint` on every path (a guard: also
/// when `paint_to` panics, so the window is never left between `BeginPaint` and `EndPaint`).
pub(super) fn paint_window(hwnd: Hwnd, state: &PaintState) {
    // `begin_paint` answers `None` when there is no display context: nothing to paint and no session to end.
    let Some(session) = begin_paint(hwnd) else {
        return;
    };
    let _end = PaintGuard {
        hwnd,
        session: &session,
    };
    paint_to(hwnd, session.hdc, state);
}

/// C# `PaintTo(hwnd, hdc, state)` (lines 434-479): double buffering. `get_client_rect` fails or the client is empty (`width <= 0 || height <= 0`) -> nothing.
/// A memory DC compatible with `hdc` and a bitmap of the client size are created; when either fails everything created is released and the control is drawn
/// DIRECTLY on `hdc` (`draw(hwnd, hdc, w, h, state)`); else the bitmap is selected, `draw(.., memory, ..)`, `bit_blt(hdc, 0, 0, w, h, memory, 0, 0, SRCCOPY)`, the
/// old bitmap is selected back, the bitmap and the DC are deleted (a guard, also on panic).
pub(super) fn paint_to(hwnd: Hwnd, hdc: Hdc, state: &PaintState) {
    let Ok(client) = get_client_rect(hwnd) else {
        return;
    };
    let width = client.right.wrapping_sub(client.left);
    let height = client.bottom.wrapping_sub(client.top);
    if width <= 0 || height <= 0 {
        return;
    }
    let memory = create_compatible_dc(hdc);
    let bitmap = if memory != 0 {
        create_compatible_bitmap(hdc, width, height)
    } else {
        0
    };
    if memory == 0 || bitmap == 0 {
        // Without a buffer the control is drawn straight onto the target (flicker is better than a missing control).
        if bitmap != 0 {
            delete_object(bitmap);
        }
        if memory != 0 {
            delete_dc(memory);
        }
        draw(hwnd, hdc, width, height, state);
        return;
    }
    let buffer = MemoryBuffer {
        dc: memory,
        bitmap,
        old: select_object(memory, bitmap),
    };
    draw(hwnd, buffer.dc, width, height, state);
    bit_blt(hdc, 0, 0, width, height, buffer.dc, 0, 0, SRCCOPY);
}

/// C# `Draw(hwnd, hdc, width, height, state)` (lines 481-523): `scale = geometry::paint_scale(get_dpi_for_window(hwnd))`; the control's font
/// (`send_message(hwnd, WM_GETFONT)`) is selected into the DC when non-zero and selected back at the end; the whole client is filled with `Dark.Window`
/// (`DialogColors::window()`); then by `state.kind`: `PushButton` -> [`draw_push_button`]; `CheckBox` and `RadioButton` -> `widgets::draw_check_or_radio(..,
/// radio, hot)`; `GroupBox` -> `frame::draw_group_box`; `ComboBox` -> `widgets::draw_combo_box`; `Progress` -> `widgets::draw_progress`; `Trackbar` ->
/// `widgets::draw_trackbar`; `EtchedStatic` -> `frame::draw_etched`; every other kind draws nothing more.
pub(super) fn draw(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, state: &PaintState) {
    let scale = paint_scale(get_dpi_for_window(hwnd));
    // SAFETY: `WM_GETFONT` takes no parameters; the answer is the font handle of the control (or 0).
    let font = unsafe { send_message(hwnd, WM_GETFONT, 0, 0) };
    let _font = (font != 0).then(|| Reselect {
        hdc,
        old: select_object(hdc, font),
    });
    fill(hdc, 0, 0, width, height, DialogColors::window());
    let hot = state.hot.get();
    match state.kind {
        ChromePaintKind::PushButton => draw_push_button(hwnd, hdc, width, height, scale, hot),
        ChromePaintKind::CheckBox | ChromePaintKind::RadioButton => {
            widgets::draw_check_or_radio(
                hwnd,
                hdc,
                width,
                height,
                scale,
                state.kind == ChromePaintKind::RadioButton,
                hot,
            );
        }
        ChromePaintKind::GroupBox => frame_module::draw_group_box(hwnd, hdc, width, height, scale),
        ChromePaintKind::ComboBox => widgets::draw_combo_box(hwnd, hdc, width, height, scale, hot),
        ChromePaintKind::Progress => widgets::draw_progress(hwnd, hdc, width, height),
        ChromePaintKind::Trackbar => widgets::draw_trackbar(hwnd, hdc, width, height, scale, hot),
        ChromePaintKind::EtchedStatic => frame_module::draw_etched(hwnd, hdc, width, height),
        _ => {}
    }
}

// ---- drawing helpers (C# lines 525-612) --------------------------------------------------------------------------------------------------------------------------

/// C# `Fill(hdc, left, top, right, bottom, color)`: nothing when `right <= left || bottom <= top`; else a solid brush of `color` (a `COLORREF`), `fill_rect`, the
/// brush deleted.
pub(super) fn fill(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32, color: u32) {
    if right <= left || bottom <= top {
        return;
    }
    let rect = Rect {
        left,
        top,
        right,
        bottom,
    };
    let brush = Owned(create_solid_brush(color));
    fill_rect(hdc, &rect, brush.0);
}

/// The four rectangles of `Frame` in the C# order (top, bottom, left, right): `(l, t, r, t + k)`, `(l, b - k, r, b)`, `(l, t, l + k, b)`, `(r - k, t, r, b)`
/// with plain `i32` arithmetic. `pub` (not `pub(super)`) because the pure test lives in `tests/` and `pub(super)` items are invisible there (D-W8B5-420).
pub fn frame_rects(left: i32, top: i32, right: i32, bottom: i32, thickness: i32) -> [Rect; 4] {
    [
        Rect {
            left,
            top,
            right,
            bottom: top + thickness,
        },
        Rect {
            left,
            top: bottom - thickness,
            right,
            bottom,
        },
        Rect {
            left,
            top,
            right: left + thickness,
            bottom,
        },
        Rect {
            left: right - thickness,
            top,
            right,
            bottom,
        },
    ]
}

/// C# `Frame(hdc, left, top, right, bottom, color, thickness = 1)`: `fill` of each rectangle of [`frame_rects`], in that order (an empty one draws nothing).
pub(super) fn frame(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32, color: u32, thickness: i32) {
    for rect in frame_rects(left, top, right, bottom, thickness) {
        fill(hdc, rect.left, rect.top, rect.right, rect.bottom, color);
    }
}

/// C# `WindowText(hwnd)`: the window text (`""` when empty); `dialog_native::user32::get_window_text`.
pub(super) fn window_text(hwnd: Hwnd) -> String {
    get_window_text(hwnd)
}

/// C# `DrawText(hdc, text, rect, color, flags)`: nothing for `""`; else `set_bk_mode(hdc, TRANSPARENT)`, `set_text_color(hdc, color)`, `draw_text(hdc, text,
/// &mut rect, flags)` (the rectangle is a copy: the C# passes it by value).
pub(super) fn draw_text(hdc: Hdc, text: &str, rect: Rect, color: u32, flags: u32) {
    if text.is_empty() {
        return;
    }
    let mut rect = rect; // the C# passes the rectangle by value
    set_bk_mode(hdc, TRANSPARENT);
    set_text_color(hdc, color);
    os_draw_text(hdc, text, &mut rect, flags);
}

/// C# `TextWidth(hdc, text)`: `0` for `""`, else the `x` of `get_text_extent_point32` (`0` when the call fails).
pub(super) fn text_width(hdc: Hdc, text: &str) -> i32 {
    if text.is_empty() {
        return 0;
    }
    get_text_extent_point32(hdc, text).map_or(0, |extent| extent.x)
}

/// C# `TextHeight(hdc)`: the `y` extent of the text `"Ag"` (`0` when the call fails).
pub(super) fn text_height(hdc: Hdc) -> i32 {
    get_text_extent_point32(hdc, "Ag").map_or(0, |extent| extent.y)
}

/// C# `UiState(hwnd)`: `geometry::ui_state_from_flags(send_message(hwnd, WM_QUERYUISTATE, 0, 0) as u32)`.
pub(super) fn ui_state(hwnd: Hwnd) -> UiState {
    // SAFETY: `WM_QUERYUISTATE` takes no parameters.
    let raw = unsafe { send_message(hwnd, WM_QUERYUISTATE, 0, 0) };
    ui_state_from_flags(raw as u32)
}

// ---- push button (C# lines 614-657) ------------------------------------------------------------------------------------------------------------------------------

/// The state of a push button as the drawing needs it (decoded by [`draw_push_button`] from `BM_GETSTATE`, the style, `IsWindowEnabled`, `BM_GETCHECK`,
/// `WM_QUERYUISTATE` and `PaintState::hot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PushButtonInput {
    pub enabled: bool,
    /// `BM_GETSTATE & BST_PUSHED`.
    pub pressed: bool,
    /// `BM_GETSTATE & BST_FOCUS`.
    pub focused: bool,
    /// `(style & BS_TYPEMASK) == BS_DEFPUSHBUTTON (1)`.
    pub is_default: bool,
    /// `style & BS_PUSHLIKE != 0 && BM_GETCHECK == BST_CHECKED`: a toggle button that is on.
    pub latched: bool,
    pub hot: bool,
    /// `style & BS_MULTILINE != 0`.
    pub multiline: bool,
    pub hide_accel: bool,
    pub hide_focus: bool,
}

/// Everything `DrawPushButton` paints, as data.
#[derive(Debug, Clone, PartialEq)]
pub struct PushButtonDraw {
    /// The fill key (`Dark.Button`, `Dark.ButtonPressed` or `Dark.ButtonHover`).
    pub back_key: &'static str,
    /// The 1 px outline key.
    pub border_key: &'static str,
    /// The text key (`Dark.TextStrong` enabled, `Dark.TextMuted` disabled).
    pub text_key: &'static str,
    /// Corner diameter passed to `RoundRect` (`S(6)`).
    pub radius: i32,
    /// The text rectangle (shifted by one pixel when pressed).
    pub text_rect: Rect,
    /// `DrawText` flags.
    pub text_flags: u32,
    /// The focus rectangle; `None` unless `focused && !hide_focus && enabled`.
    pub focus_rect: Option<Rect>,
}

/// The pure part of `DrawPushButton`. `back_key`: `!enabled` -> `Dark.Button`; `pressed || latched` -> `Dark.ButtonPressed`; `hot` -> `Dark.ButtonHover`; else
/// `Dark.Button`. `border_key`: `!enabled` -> `Dark.Border`; `is_default` -> `Dark.TextSoft`; `focused` -> `Dark.Accent`; else `Dark.BorderStrong`. `radius =
/// scaled(scale, 6)`. `offset = pressed ? 1 : 0`; `text_rect = { left: S(4) + offset, top: offset, right: width - S(4) + offset, bottom: height + offset }`.
/// `text_flags`: multiline `DT_CENTER | DT_VCENTER | DT_WORDBREAK`, else `DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS`; `| DT_HIDEPREFIX` when
/// `hide_accel`. `focus_rect = { S(3), S(3), width - S(3), height - S(3) }` when `focused && !hide_focus && enabled`.
pub fn push_button_draw(input: PushButtonInput, width: i32, height: i32, scale: f64) -> PushButtonDraw {
    let PushButtonInput {
        enabled,
        pressed,
        focused,
        is_default,
        latched,
        hot,
        multiline,
        hide_accel,
        hide_focus,
    } = input;
    let back_key = if !enabled {
        keys::BUTTON
    } else if pressed || latched {
        keys::BUTTON_PRESSED
    } else if hot {
        keys::BUTTON_HOVER
    } else {
        keys::BUTTON
    };
    let border_key = if !enabled {
        keys::BORDER
    } else if is_default {
        keys::TEXT_SOFT
    } else if focused {
        keys::ACCENT
    } else {
        keys::BORDER_STRONG
    };
    let text_key = if enabled { keys::TEXT_STRONG } else { keys::TEXT_MUTED };
    let offset = i32::from(pressed);
    let inset = scaled(scale, 4);
    let text_rect = Rect {
        left: inset + offset,
        top: offset,
        right: width - inset + offset,
        bottom: height + offset,
    };
    let mut text_flags = if multiline {
        DT_CENTER | DT_VCENTER | DT_WORDBREAK
    } else {
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS
    };
    if hide_accel {
        text_flags |= DT_HIDEPREFIX;
    }
    let focus = scaled(scale, 3);
    let focus_rect = (focused && !hide_focus && enabled).then_some(Rect {
        left: focus,
        top: focus,
        right: width - focus,
        bottom: height - focus,
    });
    PushButtonDraw {
        back_key,
        border_key,
        text_key,
        radius: scaled(scale, 6),
        text_rect,
        text_flags,
        focus_rect,
    }
}

/// C# `DrawPushButton(hwnd, hdc, width, height, scale, hot)` (lines 614-657): decode the input, `push_button_draw`, then GDI: a 1 px pen of `border_key` and a
/// solid brush of `back_key` selected into the DC, `RoundRect(0, 0, width, height, radius, radius)`, the old pen and brush selected back and the new ones deleted;
/// `draw_text(window_text(hwnd), text_rect, text colour, text_flags)`; `draw_focus_rect` when `focus_rect` is `Some`.
pub(super) fn draw_push_button(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64, hot: bool) {
    let style = get_window_long_ptr(hwnd, GWL_STYLE) as u32;
    let enabled = is_window_enabled(hwnd);
    // SAFETY: `BM_GETSTATE` and `BM_GETCHECK` take no parameters.
    let button_state = unsafe { send_message(hwnd, BM_GETSTATE, 0, 0) } as u32;
    let pressed = button_state & BST_PUSHED != 0;
    let focused = button_state & BST_FOCUS != 0;
    let is_default = style & BS_TYPEMASK == 1;
    let latched = style & BS_PUSHLIKE != 0 && {
        // SAFETY: as above.
        unsafe { send_message(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED as isize }
    };
    let ui = ui_state(hwnd);
    let draw = push_button_draw(
        PushButtonInput {
            enabled,
            pressed,
            focused,
            is_default,
            latched,
            hot,
            multiline: style & BS_MULTILINE != 0,
            hide_accel: ui.hide_accel,
            hide_focus: ui.hide_focus,
        },
        width,
        height,
        scale,
    );
    {
        let pen = Owned(create_pen(0, 1, color(draw.border_key)));
        let brush = Owned(create_solid_brush(color(draw.back_key)));
        let _old_pen = Reselect {
            hdc,
            old: select_object(hdc, pen.0),
        };
        let _old_brush = Reselect {
            hdc,
            old: select_object(hdc, brush.0),
        };
        round_rect(hdc, 0, 0, width, height, draw.radius, draw.radius);
    }
    draw_text(
        hdc,
        &window_text(hwnd),
        draw.text_rect,
        color(draw.text_key),
        draw.text_flags,
    );
    if let Some(focus) = draw.focus_rect {
        draw_focus_rect(hdc, &focus);
    }
}
