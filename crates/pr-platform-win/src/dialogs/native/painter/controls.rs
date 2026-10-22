//! Port of `NativeDialogChromePainter.cs` lines 335-657 (baseline `d92d6aae`): the part of the painter that is shared by all kinds (double buffering, the
//! drawing helpers, the list view row correction) and the push button. Unit W8-18m. Contract: `contracts/W8-18m.md`.
//!
//! The pure decisions (which colour, which rectangle) are plain functions with plain inputs ([`push_button_draw`]); the GDI part only reads the window state and
//! calls them. Every GDI object created here (pens, brushes, memory DC, bitmap) is released on every path by a guard (trap T1); a font selected into the DC is
//! restored. The drawing of the other kinds is `painter::{widgets, frame}` (W8-18n) and is reached through [`draw`].
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use windows::Win32::UI::Controls::NMLVCUSTOMDRAW;

use super::PaintState;
use crate::win32::structs::{Hdc, Hwnd, Rect};

/// C# `AdjustListViewCustomDraw(customDraw, result)` (lines 340-366) on the typed structure (D-W8B5-406): reads `dwDrawStage`, `dwItemSpec` (the row),
/// `hdr.hwndFrom` (the list view), writes `uItemState` (clears `CDIS_SELECTED`), `clrText` and `clrTextBk`; the decision is
/// `geometry::adjust_custom_draw(stage, result, selected)` with `selected` = `(send_message(list, LVM_GETITEMSTATE, row, LVIS_SELECTED) as u32 & LVIS_SELECTED)
/// != 0` (the system flag is not trusted in the classic look: the C# comment saw `0x201` for an unselected row). Applies the decision and returns its `result`.
pub fn adjust_list_view_custom_draw(custom_draw: &mut NMLVCUSTOMDRAW, result: isize) -> isize {
    todo!()
}

/// C# `RepaintNow(hwnd, state)` (lines 394-415): paints over what the system control painted by itself. Nothing when the window is dead or not visible, or when
/// `get_dc` gives `0`; else `paint_to(hwnd, dc, state)` and `release_dc` on every path.
pub(super) fn repaint_now(hwnd: Hwnd, state: &PaintState) {
    todo!()
}

/// C# `PaintWindow(hwnd, state)` (lines 417-432): `begin_paint`, `paint_to(hwnd, session.hdc, state)` when a DC was given, `end_paint` on every path (a guard: also
/// when `paint_to` panics, so the window is never left between `BeginPaint` and `EndPaint`).
pub(super) fn paint_window(hwnd: Hwnd, state: &PaintState) {
    todo!()
}

/// C# `PaintTo(hwnd, hdc, state)` (lines 434-479): double buffering. `get_client_rect` fails or the client is empty (`width <= 0 || height <= 0`) -> nothing.
/// A memory DC compatible with `hdc` and a bitmap of the client size are created; when either fails everything created is released and the control is drawn
/// DIRECTLY on `hdc` (`draw(hwnd, hdc, w, h, state)`); else the bitmap is selected, `draw(.., memory, ..)`, `bit_blt(hdc, 0, 0, w, h, memory, 0, 0, SRCCOPY)`, the
/// old bitmap is selected back, the bitmap and the DC are deleted (a guard, also on panic).
pub(super) fn paint_to(hwnd: Hwnd, hdc: Hdc, state: &PaintState) {
    todo!()
}

/// C# `Draw(hwnd, hdc, width, height, state)` (lines 481-523): `scale = geometry::paint_scale(get_dpi_for_window(hwnd))`; the control's font
/// (`send_message(hwnd, WM_GETFONT)`) is selected into the DC when non-zero and selected back at the end; the whole client is filled with `Dark.Window`
/// (`DialogColors::window()`); then by `state.kind`: `PushButton` -> [`draw_push_button`]; `CheckBox` and `RadioButton` -> `widgets::draw_check_or_radio(..,
/// radio, hot)`; `GroupBox` -> `frame::draw_group_box`; `ComboBox` -> `widgets::draw_combo_box`; `Progress` -> `widgets::draw_progress`; `Trackbar` ->
/// `widgets::draw_trackbar`; `EtchedStatic` -> `frame::draw_etched`; every other kind draws nothing more.
pub(super) fn draw(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, state: &PaintState) {
    todo!()
}

// ---- drawing helpers (C# lines 525-612) --------------------------------------------------------------------------------------------------------------------------

/// C# `Fill(hdc, left, top, right, bottom, color)`: nothing when `right <= left || bottom <= top`; else a solid brush of `color` (a `COLORREF`), `fill_rect`, the
/// brush deleted.
pub(super) fn fill(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32, color: u32) {
    todo!()
}

/// The four rectangles of `Frame` in the C# order (top, bottom, left, right): `(l, t, r, t + k)`, `(l, b - k, r, b)`, `(l, t, l + k, b)`, `(r - k, t, r, b)`
/// with plain `i32` arithmetic. `pub` (not `pub(super)`) because the pure test lives in `tests/` and `pub(super)` items are invisible there (D-W8B5-420).
pub fn frame_rects(left: i32, top: i32, right: i32, bottom: i32, thickness: i32) -> [Rect; 4] {
    todo!()
}

/// C# `Frame(hdc, left, top, right, bottom, color, thickness = 1)`: `fill` of each rectangle of [`frame_rects`], in that order (an empty one draws nothing).
pub(super) fn frame(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32, color: u32, thickness: i32) {
    todo!()
}

/// C# `WindowText(hwnd)`: the window text (`""` when empty); `dialog_native::user32::get_window_text`.
pub(super) fn window_text(hwnd: Hwnd) -> String {
    todo!()
}

/// C# `DrawText(hdc, text, rect, color, flags)`: nothing for `""`; else `set_bk_mode(hdc, TRANSPARENT)`, `set_text_color(hdc, color)`, `draw_text(hdc, text,
/// &mut rect, flags)` (the rectangle is a copy: the C# passes it by value).
pub(super) fn draw_text(hdc: Hdc, text: &str, rect: Rect, color: u32, flags: u32) {
    todo!()
}

/// C# `TextWidth(hdc, text)`: `0` for `""`, else the `x` of `get_text_extent_point32` (`0` when the call fails).
pub(super) fn text_width(hdc: Hdc, text: &str) -> i32 {
    todo!()
}

/// C# `TextHeight(hdc)`: the `y` extent of the text `"Ag"` (`0` when the call fails).
pub(super) fn text_height(hdc: Hdc) -> i32 {
    todo!()
}

/// C# `UiState(hwnd)`: `geometry::ui_state_from_flags(send_message(hwnd, WM_QUERYUISTATE, 0, 0) as u32)`.
pub(super) fn ui_state(hwnd: Hwnd) -> super::geometry::UiState {
    todo!()
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
    todo!()
}

/// C# `DrawPushButton(hwnd, hdc, width, height, scale, hot)` (lines 614-657): decode the input, `push_button_draw`, then GDI: a 1 px pen of `border_key` and a
/// solid brush of `back_key` selected into the DC, `RoundRect(0, 0, width, height, radius, radius)`, the old pen and brush selected back and the new ones deleted;
/// `draw_text(window_text(hwnd), text_rect, text colour, text_flags)`; `draw_focus_rect` when `focus_rect` is `Some`.
pub(super) fn draw_push_button(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64, hot: bool) {
    todo!()
}
