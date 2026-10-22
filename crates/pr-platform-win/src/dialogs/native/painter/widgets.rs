//! Port of `NativeDialogChromePainter.cs` lines 659-897 (baseline `d92d6aae`): the check box, radio button, combo box (closed part), progress bar and trackbar.
//! Unit W8-18n. Contract: `contracts/W8-18n.md`.
//!
//! Each drawing has a pure layout function (data in, data out, tested without a window) and a GDI function that reads the control, calls it and draws the result
//! with the helpers of `painter::controls`. The plan names this file `title_bar`; the C# painter has no title bar code (D-W8B5-401).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use crate::win32::structs::{Hdc, Hwnd, Point, Rect};

// ---- check box and radio button -----------------------------------------------------------------------------------------------------------------------------------

/// The inputs of the layout of a check box or radio button (decoded by [`draw_check_or_radio`] from the style, `BM_GETSTATE`, `BM_GETCHECK`, `IsWindowEnabled`,
/// `WM_QUERYUISTATE`, the DC font metrics and `PaintState::hot`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CheckInput {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    pub radio: bool,
    pub enabled: bool,
    pub hot: bool,
    /// `BM_GETCHECK != BST_UNCHECKED` (checked or indeterminate).
    pub checked: bool,
    /// `BM_GETSTATE & BST_FOCUS`.
    pub focused: bool,
    /// `style & BS_MULTILINE`.
    pub multiline: bool,
    /// `style & BS_LEFTTEXT`: the box is at the right, the text at its left.
    pub left_text: bool,
    /// `TextHeight(hdc)`.
    pub text_height: i32,
    /// `TextWidth(hdc, text)`.
    pub text_width: i32,
    pub hide_focus: bool,
    pub hide_accel: bool,
}

/// What `DrawCheckOrRadio` paints, as data.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckLayout {
    /// The box side, `S(13)`.
    pub box_size: i32,
    pub box_left: i32,
    pub box_top: i32,
    /// The outline key of the box (or of the circle).
    pub border_key: &'static str,
    /// Radio: the inner circle colour (`Dark.Input`). Check box: the box fill (`Dark.Accent` when checked and enabled, else `Dark.Input`).
    pub fill_key: &'static str,
    /// Radio only, when checked: the dot key (`Dark.Accent` enabled, `Dark.TextFaint` disabled) and its inset (`max(3, round_ties_even(box * 0.28))`).
    pub dot: Option<(&'static str, i32)>,
    /// Check box only, when checked: the pen key (`Dark.TextStrong` enabled, `Dark.TextFaint` disabled), the pen width `S(2)` and the three points of the mark.
    pub mark: Option<(&'static str, i32, [Point; 3])>,
    pub text_rect: Rect,
    pub text_flags: u32,
    /// `Dark.TextStrong` enabled, `Dark.TextMuted` disabled.
    pub text_key: &'static str,
    /// `None` unless `focused && !hide_focus && enabled`.
    pub focus_rect: Option<Rect>,
}

/// The pure part of `DrawCheckOrRadio` (lines 659-749). `box = S(13)`; `box_left = left_text ? width - box : 0`; `box_top = multiline ? max(0, (text_height - box)
/// / 2) : max(0, (height - box) / 2)` (truncating division). `border_key`: `!enabled` -> `Dark.TextFaint`; `checked` -> `Dark.Accent`; `hot` -> `Dark.TextMuted`;
/// else `Dark.BorderStrong`. `fill_key` and `dot` as the field docs. Check mark points: `(box_left + round(box * 0.22), box_top + round(box * 0.52))`, `(box_left +
/// round(box * 0.42), box_top + round(box * 0.72))`, `(box_left + round(box * 0.80), box_top + round(box * 0.28))`, `round` = `f64::round_ties_even`. `gap =
/// S(6)`; `text_rect = left_text ? { 0, 0, max(0, box_left - gap), height } : { box + gap, 0, width, height }`; `text_flags = multiline ? DT_WORDBREAK :
/// DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS`, `| DT_HIDEPREFIX` when `hide_accel`. Focus rectangle: `shown = min(text_rect.right - text_rect.left, text_width)`;
/// `text_top = multiline ? 0 : max(0, (height - text_height) / 2)`; `{ left: text_rect.left - 1, top: max(0, text_top - 1), right: min(width, text_rect.left +
/// shown + 2), bottom: min(height, text_top + text_height + 1) }`.
pub fn check_layout(input: CheckInput) -> CheckLayout {
    todo!()
}

/// C# `DrawCheckOrRadio(hwnd, hdc, width, height, scale, radio, hot)`: decode, [`check_layout`], then GDI. Radio: a 1 px pen of `border_key` and a brush of
/// `fill_key`, `ellipse(box)`, restored and deleted; the dot as a filled ellipse (pen and brush of the dot key) inset by `inset`. Check box: `fill` the box with
/// `fill_key`, `frame` it with `border_key`, the mark as `polyline` with a pen of the mark width. Then the text and the focus rectangle.
pub(super) fn draw_check_or_radio(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64, radio: bool, hot: bool) {
    todo!()
}

// ---- combo box ----------------------------------------------------------------------------------------------------------------------------------------------------

/// The inputs of the closed-combo layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComboInput {
    pub width: i32,
    pub height: i32,
    pub enabled: bool,
    /// `GetFocus() == hwnd`.
    pub focused: bool,
    /// `CB_GETDROPPEDSTATE != 0`.
    pub dropped: bool,
    pub hot: bool,
    pub hide_focus: bool,
}

/// What `DrawComboBox` paints, as data. The scale is an argument of [`combo_layout`].
#[derive(Debug, Clone, PartialEq)]
pub struct ComboLayout {
    /// `!enabled` -> `Dark.Border`; `focused || dropped` -> `Dark.Accent`; `hot` -> `Dark.TextMuted`; else `Dark.BorderStrong`.
    pub border_key: &'static str,
    /// The arrow button width, `max(height, S(18))`.
    pub arrow_width: i32,
    /// The text rectangle `{ S(6), 1, max(S(6), width - arrow_width), height - 1 }`.
    pub text_rect: Rect,
    /// `DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS`.
    pub text_flags: u32,
    /// `Dark.TextStrong` enabled, `Dark.TextMuted` disabled.
    pub text_key: &'static str,
    /// The pen width of the arrow, `2` when `S(2) > 1` (always, for `scale >= 1`), else `1`.
    pub arrow_pen_width: i32,
    /// `Dark.TextSecondary` enabled, `Dark.TextFaint` disabled.
    pub arrow_key: &'static str,
    /// The three points of the chevron: with `cx = width - arrow_width / 2 - 1`, `cy = height / 2`, `half = S(4)`: `(cx - half, cy - half / 2)`, `(cx, cy +
    /// half / 2 + 1)`, `(cx + half, cy - half / 2)`.
    pub arrow_points: [Point; 3],
    /// `{ 3, 3, max(3, width - arrow_width), height - 3 }` when `focused && !dropped && !hide_focus`, else `None`.
    pub focus_rect: Option<Rect>,
}

/// The pure part of `DrawComboBox` (lines 777-814).
pub fn combo_layout(input: ComboInput, scale: f64) -> ComboLayout {
    todo!()
}

/// C# `DrawComboBox(hwnd, hdc, width, height, scale, hot)`: fill the whole client with `Dark.Input`, `frame` it with `border_key`; when `CB_GETCURSEL >= 0` draw the
/// text `ComboBoxControl::text_at(hwnd, index)` in `text_rect`; the chevron as a `polyline` with a pen of `arrow_pen_width` and `arrow_key`; the focus rectangle.
pub(super) fn draw_combo_box(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64, hot: bool) {
    todo!()
}

// ---- progress -----------------------------------------------------------------------------------------------------------------------------------------------------

/// The pure part of the fill of `DrawProgress` (lines 829-834): `None` when `high <= low`; else `fraction = clamp((position - low) as f64 / (high - low) as f64,
/// 0.0, 1.0)` (the subtractions wrap like the C# unchecked `int`; the clamp is `max(0.0).min(1.0)`, a NaN-safe form that cannot panic) and the width in pixels
/// `round_ties_even(width.wrapping_sub(inset.wrapping_mul(2)) as f64 * fraction) as i32` (saturating cast; every argument may be an `i32` extreme).
pub fn progress_fill_width(width: i32, inset: i32, low: i32, high: i32, position: i32) -> Option<i32> {
    todo!()
}

/// C# `DrawProgress(hwnd, hdc, width, height)` (lines 816-835): fill with `Dark.Surface`; `framed = frame::border_thickness_of(hwnd).0 > 0` (the outer border is
/// painted at `WM_NCPAINT`); `inset = framed ? 0 : 1`; when not framed `frame` the client with `Dark.BorderStrong`; `low = PBM_GETRANGE(wParam 1)`, `high =
/// PBM_GETRANGE(wParam 0)`, `position = PBM_GETPOS`; when `progress_fill_width` is `Some(fill)`: `fill` the rectangle `(inset, inset, inset + fill, height -
/// inset)` with `Dark.Accent`.
pub(super) fn draw_progress(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32) {
    todo!()
}

// ---- trackbar -----------------------------------------------------------------------------------------------------------------------------------------------------

/// The inputs of the trackbar layout (decoded by [`draw_trackbar`]).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackbarInput {
    /// `TBM_GETCHANNELRECT` (client coordinates).
    pub channel: Rect,
    /// `TBM_GETTHUMBRECT`.
    pub thumb: Rect,
    /// The client height.
    pub height: i32,
    pub scale: f64,
    pub enabled: bool,
    pub hot: bool,
    /// `style & TBS_NOTICKS != 0`.
    pub no_ticks: bool,
    /// `style & TBS_AUTOTICKS != 0`.
    pub auto_ticks: bool,
    /// `TBM_GETNUMTICS` (includes both ends).
    pub tick_count: i32,
    /// `TBM_GETRANGEMIN` and `TBM_GETRANGEMAX`.
    pub range_min: i32,
    pub range_max: i32,
    /// The positions of the INNER ticks (`tick_count - 2` values copied from `TBM_GETPTICS`), `None` for auto ticks or when the pointer was null.
    pub inner_ticks: Option<Vec<i32>>,
}

/// A rectangle with the palette key it is filled with.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyedRect {
    pub rect: Rect,
    pub key: &'static str,
}

/// What `DrawTrackbar` paints, as data, in painting order.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackbarLayout {
    /// The whole channel line (`Dark.BorderStrong` enabled, `Dark.Border` disabled).
    pub channel: KeyedRect,
    /// The part left of the thumb centre (`Dark.Accent` enabled, `Dark.TextFaint` disabled).
    pub filled: KeyedRect,
    /// The 1 px wide tick rectangles; empty without ticks.
    pub ticks: Vec<Rect>,
    /// `Dark.TextMuted` enabled, `Dark.TextFaint` disabled.
    pub tick_key: &'static str,
    /// The thumb fill (`!enabled` -> `Dark.TextFaint`; `hot` -> `Dark.AccentHover`; else `Dark.Accent`).
    pub thumb: KeyedRect,
    /// The thumb outline (`Dark.Border` disabled, `Dark.TextSoft` enabled).
    pub thumb_border_key: &'static str,
}

/// The pure part of `DrawTrackbar` (lines 837-890). `thickness = S(4)`; `mid = (channel.top + channel.bottom) / 2`; `line_top = mid - thickness / 2`; channel
/// rectangle `(channel.left, line_top, channel.right, line_top + thickness)`; `thumb_center = (thumb.left + thumb.right) / 2`; filled rectangle `(channel.left,
/// line_top, clamp(thumb_center, channel.left, channel.right), line_top + thickness)` where the clamp is `max(left).min(right)` (never `clamp`: it panics when
/// `left > right`). Ticks: see [`tick_xs`]; each tick is `(x, top, x + 1, bottom)` with `top = thumb.bottom + S(1)` and `bottom = min(height, top + S(4))`.
pub fn trackbar_layout(input: &TrackbarInput) -> TrackbarLayout {
    todo!()
}

/// The x positions of the tick marks (C# lines 854-884), empty unless `!no_ticks && count >= 2 && max > min`. `span = max(1, channel_right - channel_left - 1)`;
/// `min_gap = S(6)`; `step = max(1, ceil(min_gap as f64 / max(0.01, span as f64 / (count - 1) as f64)) as i32)`; for `i` in `0, step, 2 * step, .. < count`: `value =
/// auto ? min + round_ties_even(i as f64 * (max - min) as f64 / (count - 1) as f64) as i32 : i == 0 ? min : i == count - 1 ? max : inner.get(i - 1) or min`;
/// `x = channel_left + round_ties_even((value - min) as f64 * span as f64 / (max - min) as f64) as i32`. After the loop, when `(count - 1) % step != 0`, one more
/// tick at `channel_left + span` (the last tick is always drawn). The subtractions wrap like the C# unchecked `int`. Total: an `inner` shorter than needed gives `min`.
#[allow(clippy::too_many_arguments)] // mirrors the C# loop inputs 1:1; every argument is a value read from the control
pub fn tick_xs(
    channel_left: i32,
    channel_right: i32,
    count: i32,
    min: i32,
    max: i32,
    auto: bool,
    inner: Option<&[i32]>,
    scale: f64,
) -> Vec<i32> {
    todo!()
}

/// C# `DrawTrackbar(hwnd, hdc, width, height, scale, hot)`: reads `TBM_GETCHANNELRECT` and `TBM_GETTHUMBRECT` (a pointer to a local `RECT`, SAFETY comment),
/// the style, `TBM_GETNUMTICS`, `TBM_GETRANGEMIN/MAX`, and for non-auto ticks `TBM_GETPTICS` (a pointer to `count - 2` `i32` owned by the control: copied at once,
/// `None` for a null pointer, SAFETY comment); `trackbar_layout`; fills the three rectangles, the ticks, the thumb; `frame` of the thumb with `thumb_border_key`;
/// the focus rectangle `(0, 0, width, height)` with `draw_focus_rect` when `get_focus() == hwnd && !ui_state.hide_focus`.
pub(super) fn draw_trackbar(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64, hot: bool) {
    todo!()
}
