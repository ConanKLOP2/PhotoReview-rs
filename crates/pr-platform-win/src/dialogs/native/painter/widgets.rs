//! Port of `NativeDialogChromePainter.cs` lines 659-897 (baseline `d92d6aae`): the check box, radio button, combo box (closed part), progress bar and trackbar.
//! Unit W8-18n. Contract: `contracts/W8-18n.md`.
//!
//! Each drawing has a pure layout function (data in, data out, tested without a window) and a GDI function that reads the control, calls it and draws the result
//! with the helpers of `painter::controls`. The plan names this file `title_bar`; the C# painter has no title bar code (D-W8B5-401).

use super::controls::{draw_text, fill, frame, text_height, text_width, ui_state, window_text};
use super::frame::border_thickness_of;
use super::geometry::{color, keys, scaled};
use crate::win32::api::user32::get_window_long_ptr;
use crate::win32::comctl::gdi32::create_pen;
use crate::win32::comctl::user32::draw_focus_rect;
use crate::win32::control_constants::{
    BM_GETCHECK, BST_UNCHECKED, CB_GETCURSEL, CB_GETDROPPEDSTATE, CB_GETLBTEXT, CB_GETLBTEXTLEN, PBM_GETPOS,
    TBS_AUTOTICKS, TBS_NOTICKS,
};
use crate::win32::dialog_chrome::{
    BM_GETSTATE, BS_LEFTTEXT, BS_MULTILINE, BST_FOCUS, DT_HIDEPREFIX, PBM_GETRANGE, TBM_GETCHANNELRECT, TBM_GETNUMTICS,
    TBM_GETPTICS, TBM_GETRANGEMAX, TBM_GETRANGEMIN, TBM_GETTHUMBRECT, ellipse, polyline,
};
use crate::win32::dialog_native::gdi32::{create_solid_brush, delete_object, select_object};
use crate::win32::dialog_native::user32::{get_focus, is_window_enabled, send_message};
use crate::win32::dialog_native::{DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK};
use crate::win32::messages::GWL_STYLE;
use crate::win32::structs::{Hdc, Hgdi, Hwnd, Point, Rect};

// ---- guards (trap T1: every pen, brush and selected object is restored and deleted on every path) ------------------------------------------------------------

/// Deletes a GDI object when dropped; `0` is ignored.
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

/// Runs `draw` with a solid pen of `width` and `pen_color` selected; the old pen is selected back and the new one deleted afterwards (also on a panic).
fn with_pen(hdc: Hdc, width: i32, pen_color: u32, draw: impl FnOnce()) {
    let pen = Owned(create_pen(0, width, pen_color));
    let _old = Reselect {
        hdc,
        old: select_object(hdc, pen.0),
    };
    draw();
}

/// Runs `draw` with a 1 px pen and a solid brush selected, restored and deleted afterwards (C# pen, brush, `Select` both, draw, `Select` back, delete both).
fn with_pen_and_brush(hdc: Hdc, pen_color: u32, brush_color: u32, draw: impl FnOnce()) {
    let pen = Owned(create_pen(0, 1, pen_color));
    let brush = Owned(create_solid_brush(brush_color));
    let _old_pen = Reselect {
        hdc,
        old: select_object(hdc, pen.0),
    };
    let _old_brush = Reselect {
        hdc,
        old: select_object(hdc, brush.0),
    };
    draw();
}

/// `Math.Round(value)` of the C# (to even) as an `i32` (saturating).
fn round_even(value: f64) -> i32 {
    value.round_ties_even() as i32
}

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
    let CheckInput {
        width,
        height,
        scale,
        radio,
        enabled,
        hot,
        checked,
        focused,
        multiline,
        left_text,
        text_height,
        text_width,
        hide_focus,
        hide_accel,
    } = input;
    let box_size = scaled(scale, 13);
    let box_left = if left_text { width - box_size } else { 0 };
    // The C# truncating division of a negative value is clamped by `Math.Max(0, ..)`.
    let box_top = if multiline {
        ((text_height - box_size) / 2).max(0)
    } else {
        ((height - box_size) / 2).max(0)
    };
    let border_key = if !enabled {
        keys::TEXT_FAINT
    } else if checked {
        keys::ACCENT
    } else if hot {
        keys::TEXT_MUTED
    } else {
        keys::BORDER_STRONG
    };
    let fill_key = if !radio && checked && enabled {
        keys::ACCENT
    } else {
        keys::INPUT
    };
    let box_f = f64::from(box_size);
    let dot = (radio && checked).then(|| {
        let key = if enabled { keys::ACCENT } else { keys::TEXT_FAINT };
        (key, round_even(box_f * 0.28).max(3))
    });
    let mark = (!radio && checked).then(|| {
        let key = if enabled { keys::TEXT_STRONG } else { keys::TEXT_FAINT };
        let points = [
            Point {
                x: box_left + round_even(box_f * 0.22),
                y: box_top + round_even(box_f * 0.52),
            },
            Point {
                x: box_left + round_even(box_f * 0.42),
                y: box_top + round_even(box_f * 0.72),
            },
            Point {
                x: box_left + round_even(box_f * 0.80),
                y: box_top + round_even(box_f * 0.28),
            },
        ];
        (key, scaled(scale, 2), points)
    });
    let gap = scaled(scale, 6);
    let text_rect = if left_text {
        Rect {
            left: 0,
            top: 0,
            right: (box_left - gap).max(0),
            bottom: height,
        }
    } else {
        Rect {
            left: box_size + gap,
            top: 0,
            right: width,
            bottom: height,
        }
    };
    let mut text_flags = if multiline {
        DT_WORDBREAK
    } else {
        DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS
    };
    if hide_accel {
        text_flags |= DT_HIDEPREFIX;
    }
    let focus_rect = (focused && !hide_focus && enabled).then(|| {
        let shown = (text_rect.right - text_rect.left).min(text_width);
        let text_top = if multiline {
            0
        } else {
            ((height - text_height) / 2).max(0)
        };
        Rect {
            left: text_rect.left - 1,
            top: (text_top - 1).max(0),
            right: width.min(text_rect.left + shown + 2),
            bottom: height.min(text_top + text_height + 1),
        }
    });
    CheckLayout {
        box_size,
        box_left,
        box_top,
        border_key,
        fill_key,
        dot,
        mark,
        text_rect,
        text_flags,
        text_key: if enabled { keys::TEXT_STRONG } else { keys::TEXT_MUTED },
        focus_rect,
    }
}

/// C# `DrawCheckOrRadio(hwnd, hdc, width, height, scale, radio, hot)`: decode, [`check_layout`], then GDI. Radio: a 1 px pen of `border_key` and a brush of
/// `fill_key`, `ellipse(box)`, restored and deleted; the dot as a filled ellipse (pen and brush of the dot key) inset by `inset`. Check box: `fill` the box with
/// `fill_key`, `frame` it with `border_key`, the mark as `polyline` with a pen of the mark width. Then the text and the focus rectangle.
pub(super) fn draw_check_or_radio(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64, radio: bool, hot: bool) {
    let style = get_window_long_ptr(hwnd, GWL_STYLE) as u32;
    let enabled = is_window_enabled(hwnd);
    // SAFETY: `BM_GETSTATE` and `BM_GETCHECK` take no parameters.
    let button_state = unsafe { send_message(hwnd, BM_GETSTATE, 0, 0) } as u32;
    // SAFETY: as above.
    let checked = unsafe { send_message(hwnd, BM_GETCHECK, 0, 0) } != BST_UNCHECKED as isize;
    let text = window_text(hwnd);
    let ui = ui_state(hwnd);
    let layout = check_layout(CheckInput {
        width,
        height,
        scale,
        radio,
        enabled,
        hot,
        checked,
        focused: button_state & BST_FOCUS != 0,
        multiline: style & BS_MULTILINE != 0,
        left_text: style & BS_LEFTTEXT != 0,
        text_height: text_height(hdc),
        text_width: text_width(hdc, &text),
        hide_focus: ui.hide_focus,
        hide_accel: ui.hide_accel,
    });
    let (left, top, size) = (layout.box_left, layout.box_top, layout.box_size);
    if radio {
        with_pen_and_brush(hdc, color(layout.border_key), color(layout.fill_key), || {
            ellipse(hdc, left, top, left + size, top + size);
        });
        if let Some((key, inset)) = layout.dot {
            with_pen_and_brush(hdc, color(key), color(key), || {
                ellipse(hdc, left + inset, top + inset, left + size - inset, top + size - inset);
            });
        }
    } else {
        fill(hdc, left, top, left + size, top + size, color(layout.fill_key));
        frame(hdc, left, top, left + size, top + size, color(layout.border_key), 1);
        if let Some((key, pen_width, points)) = layout.mark {
            with_pen(hdc, pen_width, color(key), || {
                polyline(hdc, &points);
            });
        }
    }
    draw_text(hdc, &text, layout.text_rect, color(layout.text_key), layout.text_flags);
    if let Some(focus) = layout.focus_rect {
        draw_focus_rect(hdc, &focus);
    }
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
    let ComboInput {
        width,
        height,
        enabled,
        focused,
        dropped,
        hot,
        hide_focus,
    } = input;
    let border_key = if !enabled {
        keys::BORDER
    } else if focused || dropped {
        keys::ACCENT
    } else if hot {
        keys::TEXT_MUTED
    } else {
        keys::BORDER_STRONG
    };
    let arrow_width = height.max(scaled(scale, 18));
    let left = scaled(scale, 6);
    let text_rect = Rect {
        left,
        top: 1,
        right: left.max(width - arrow_width),
        bottom: height - 1,
    };
    let cx = width - arrow_width / 2 - 1;
    let cy = height / 2;
    let half = scaled(scale, 4);
    ComboLayout {
        border_key,
        arrow_width,
        text_rect,
        text_flags: DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS,
        text_key: if enabled { keys::TEXT_STRONG } else { keys::TEXT_MUTED },
        arrow_pen_width: if scaled(scale, 2) > 1 { 2 } else { 1 },
        arrow_key: if enabled {
            keys::TEXT_SECONDARY
        } else {
            keys::TEXT_FAINT
        },
        arrow_points: [
            Point {
                x: cx - half,
                y: cy - half / 2,
            },
            Point {
                x: cx,
                y: cy + half / 2 + 1,
            },
            Point {
                x: cx + half,
                y: cy - half / 2,
            },
        ],
        focus_rect: (focused && !dropped && !hide_focus).then_some(Rect {
            left: 3,
            top: 3,
            right: 3.max(width - arrow_width),
            bottom: height - 3,
        }),
    }
}

/// C# `DrawComboBox(hwnd, hdc, width, height, scale, hot)`: fill the whole client with `Dark.Input`, `frame` it with `border_key`; when `CB_GETCURSEL >= 0` draw the
/// text `ComboBoxControl::text_at(hwnd, index)` in `text_rect`; the chevron as a `polyline` with a pen of `arrow_pen_width` and `arrow_key`; the focus rectangle.
pub(super) fn draw_combo_box(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64, hot: bool) {
    let enabled = is_window_enabled(hwnd);
    let focused = get_focus() == hwnd;
    // SAFETY: `CB_GETDROPPEDSTATE` and `CB_GETCURSEL` take no parameters.
    let dropped = unsafe { send_message(hwnd, CB_GETDROPPEDSTATE, 0, 0) } != 0;
    let ui = ui_state(hwnd);
    let layout = combo_layout(
        ComboInput {
            width,
            height,
            enabled,
            focused,
            dropped,
            hot,
            hide_focus: ui.hide_focus,
        },
        scale,
    );
    fill(hdc, 0, 0, width, height, color(keys::INPUT));
    frame(hdc, 0, 0, width, height, color(layout.border_key), 1);
    // SAFETY: as above.
    let index = unsafe { send_message(hwnd, CB_GETCURSEL, 0, 0) } as i32;
    if index >= 0 {
        let text = combo_item_text(hwnd, index);
        draw_text(hdc, &text, layout.text_rect, color(layout.text_key), layout.text_flags);
    }
    with_pen(hdc, layout.arrow_pen_width, color(layout.arrow_key), || {
        polyline(hdc, &layout.arrow_points);
    });
    if let Some(focus) = layout.focus_rect {
        draw_focus_rect(hdc, &focus);
    }
}

// ---- progress -----------------------------------------------------------------------------------------------------------------------------------------------------

/// The pure part of the fill of `DrawProgress` (lines 829-834): `None` when `high <= low`; else `fraction = clamp((position - low) as f64 / (high - low) as f64,
/// 0.0, 1.0)` (the subtractions wrap like the C# unchecked `int`; the clamp is `f64::clamp(0.0, 1.0)`: constant ordered bounds, it cannot panic) and the width in pixels
/// `round_ties_even(width.wrapping_sub(inset.wrapping_mul(2)) as f64 * fraction) as i32` (saturating cast; every argument may be an `i32` extreme).
pub fn progress_fill_width(width: i32, inset: i32, low: i32, high: i32, position: i32) -> Option<i32> {
    if high <= low {
        return None;
    }
    // The subtractions wrap like the C# unchecked `int`; `clamp(0.0, 1.0)` with constant ordered bounds cannot panic.
    let fraction = (f64::from(position.wrapping_sub(low)) / f64::from(high.wrapping_sub(low))).clamp(0.0, 1.0);
    Some(round_even(
        f64::from(width.wrapping_sub(inset.wrapping_mul(2))) * fraction,
    ))
}

/// C# `DrawProgress(hwnd, hdc, width, height)` (lines 816-835): fill with `Dark.Surface`; `framed = frame::border_thickness_of(hwnd).0 > 0` (the outer border is
/// painted at `WM_NCPAINT`); `inset = framed ? 0 : 1`; when not framed `frame` the client with `Dark.BorderStrong`; `low = PBM_GETRANGE(wParam 1)`, `high =
/// PBM_GETRANGE(wParam 0)`, `position = PBM_GETPOS`; when `progress_fill_width` is `Some(fill)`: `fill` the rectangle `(inset, inset, inset + fill, height -
/// inset)` with `Dark.Accent`.
pub(super) fn draw_progress(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32) {
    fill(hdc, 0, 0, width, height, color(keys::SURFACE));
    // The outer border of a bordered progress bar is painted at `WM_NCPAINT` (`frame::paint_border`).
    let framed = border_thickness_of(hwnd).0 > 0;
    let inset = i32::from(!framed);
    if !framed {
        frame(hdc, 0, 0, width, height, color(keys::BORDER_STRONG), 1);
    }
    // SAFETY: `PBM_GETRANGE` takes a flag in `wparam` and an optional (here null) range pointer; `PBM_GETPOS` takes no parameters.
    let (low, high, position) = unsafe {
        (
            send_message(hwnd, PBM_GETRANGE, 1, 0) as i32,
            send_message(hwnd, PBM_GETRANGE, 0, 0) as i32,
            send_message(hwnd, PBM_GETPOS, 0, 0) as i32,
        )
    };
    if let Some(filled) = progress_fill_width(width, inset, low, high, position) {
        fill(hdc, inset, inset, inset + filled, height - inset, color(keys::ACCENT));
    }
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
    let thickness = scaled(input.scale, 4);
    let channel = input.channel;
    let thumb = input.thumb;
    let enabled = input.enabled;
    let mid = channel.top.wrapping_add(channel.bottom) / 2;
    let line_top = mid.wrapping_sub(thickness / 2);
    let line_bottom = line_top.wrapping_add(thickness);
    let thumb_center = thumb.left.wrapping_add(thumb.right) / 2;
    // `max(left).min(right)`, never `clamp`: it panics when `left > right`.
    let filled_right = thumb_center.max(channel.left).min(channel.right);
    let tick_key = if enabled { keys::TEXT_MUTED } else { keys::TEXT_FAINT };
    let mut ticks = Vec::new();
    if !input.no_ticks && input.tick_count >= 2 && input.range_max > input.range_min {
        let top = thumb.bottom.wrapping_add(scaled(input.scale, 1));
        let bottom = input.height.min(top.wrapping_add(scaled(input.scale, 4)));
        for x in tick_xs(
            channel.left,
            channel.right,
            input.tick_count,
            input.range_min,
            input.range_max,
            input.auto_ticks,
            input.inner_ticks.as_deref(),
            input.scale,
        ) {
            ticks.push(Rect {
                left: x,
                top,
                right: x.wrapping_add(1),
                bottom,
            });
        }
    }
    TrackbarLayout {
        channel: KeyedRect {
            rect: Rect {
                left: channel.left,
                top: line_top,
                right: channel.right,
                bottom: line_bottom,
            },
            key: if enabled { keys::BORDER_STRONG } else { keys::BORDER },
        },
        filled: KeyedRect {
            rect: Rect {
                left: channel.left,
                top: line_top,
                right: filled_right,
                bottom: line_bottom,
            },
            key: if enabled { keys::ACCENT } else { keys::TEXT_FAINT },
        },
        ticks,
        tick_key,
        thumb: KeyedRect {
            rect: thumb,
            key: if !enabled {
                keys::TEXT_FAINT
            } else if input.hot {
                keys::ACCENT_HOVER
            } else {
                keys::ACCENT
            },
        },
        thumb_border_key: if enabled { keys::TEXT_SOFT } else { keys::BORDER },
    }
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
    if count < 2 || max <= min {
        return Vec::new();
    }
    let span = channel_right.wrapping_sub(channel_left).wrapping_sub(1).max(1);
    let min_gap = scaled(scale, 6);
    let gaps = f64::from(count - 1);
    // `(int)Math.Ceiling(..)` is a saturating cast; the step is at least 1 and a `usize` for `step_by`.
    let step = ((f64::from(min_gap) / (f64::from(span) / gaps).max(0.01)).ceil() as i32).max(1);
    let stride = usize::try_from(step).unwrap_or(1).max(1);
    let range = f64::from(max.wrapping_sub(min));
    let mut xs = Vec::new();
    for i in (0..count).step_by(stride) {
        let value = if auto {
            min.wrapping_add(round_even(f64::from(i) * range / gaps))
        } else if i == 0 {
            min
        } else if i == count - 1 {
            max
        } else {
            // A missing or too short table gives `min` (the C# reads the pointer without a bound; the caller copies at most `count - 2` values).
            inner
                .and_then(|table| usize::try_from(i - 1).ok().and_then(|index| table.get(index)))
                .copied()
                .unwrap_or(min)
        };
        xs.push(channel_left.wrapping_add(round_even(f64::from(value.wrapping_sub(min)) * f64::from(span) / range)));
    }
    if (count - 1) % step != 0 {
        // The last tick is always drawn.
        xs.push(channel_left.wrapping_add(span));
    }
    xs
}

/// C# `DrawTrackbar(hwnd, hdc, width, height, scale, hot)`: reads `TBM_GETCHANNELRECT` and `TBM_GETTHUMBRECT` (a pointer to a local `RECT`, SAFETY comment),
/// the style, `TBM_GETNUMTICS`, `TBM_GETRANGEMIN/MAX`, and for non-auto ticks `TBM_GETPTICS` (a pointer to `count - 2` `i32` owned by the control: copied at once,
/// `None` for a null pointer, SAFETY comment); `trackbar_layout`; fills the three rectangles, the ticks, the thumb; `frame` of the thumb with `thumb_border_key`;
/// the focus rectangle `(0, 0, width, height)` with `draw_focus_rect` when `get_focus() == hwnd && !ui_state.hide_focus`.
pub(super) fn draw_trackbar(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64, hot: bool) {
    let enabled = is_window_enabled(hwnd);
    let style = get_window_long_ptr(hwnd, GWL_STYLE) as u32;
    let mut channel = Rect::default();
    let mut thumb = Rect::default();
    // SAFETY: the pointer is to a live local `RECT`; the control fills at most `sizeof(RECT)` bytes during the synchronous call.
    unsafe {
        send_message(hwnd, TBM_GETCHANNELRECT, 0, std::ptr::from_mut(&mut channel) as isize);
    }
    // SAFETY: as above.
    unsafe {
        send_message(hwnd, TBM_GETTHUMBRECT, 0, std::ptr::from_mut(&mut thumb) as isize);
    }
    let no_ticks = style & TBS_NOTICKS != 0;
    let auto_ticks = style & TBS_AUTOTICKS != 0;
    // SAFETY: the three messages take no parameters.
    let (tick_count, range_min, range_max) = unsafe {
        (
            send_message(hwnd, TBM_GETNUMTICS, 0, 0) as i32,
            send_message(hwnd, TBM_GETRANGEMIN, 0, 0) as i32,
            send_message(hwnd, TBM_GETRANGEMAX, 0, 0) as i32,
        )
    };
    // TBS_AUTOTICKS: the system keeps no array of positions (TBM_GETPTICS answers garbage), the ticks are evenly spaced; otherwise the inner positions are copied
    // at once (the pointer belongs to the control and is invalid after the next change).
    let inner_ticks = if !no_ticks && !auto_ticks && tick_count >= 2 && range_max > range_min {
        // SAFETY: `TBM_GETPTICS` takes no parameters.
        let pointer = unsafe { send_message(hwnd, TBM_GETPTICS, 0, 0) } as *const i32;
        if pointer.is_null() {
            None
        } else {
            let inner = usize::try_from(tick_count - 2).unwrap_or(0);
            // SAFETY: the answer is a pointer into the control's own array of `count - 2` `i32`, valid until the control changes; it is copied here, before any
            // other message is sent, and at most `count - 2` values are read.
            Some(
                (0..inner)
                    .map(|index| unsafe { pointer.add(index).read_unaligned() })
                    .collect::<Vec<i32>>(),
            )
        }
    } else {
        None
    };
    let layout = trackbar_layout(&TrackbarInput {
        channel,
        thumb,
        height,
        scale,
        enabled,
        hot,
        no_ticks,
        auto_ticks,
        tick_count,
        range_min,
        range_max,
        inner_ticks,
    });
    let paint = |keyed: &KeyedRect| {
        fill(
            hdc,
            keyed.rect.left,
            keyed.rect.top,
            keyed.rect.right,
            keyed.rect.bottom,
            color(keyed.key),
        );
    };
    paint(&layout.channel);
    paint(&layout.filled);
    for tick in &layout.ticks {
        fill(
            hdc,
            tick.left,
            tick.top,
            tick.right,
            tick.bottom,
            color(layout.tick_key),
        );
    }
    paint(&layout.thumb);
    let outline = layout.thumb.rect;
    frame(
        hdc,
        outline.left,
        outline.top,
        outline.right,
        outline.bottom,
        color(layout.thumb_border_key),
        1,
    );
    if get_focus() == hwnd && !ui_state(hwnd).hide_focus {
        draw_focus_rect(
            hdc,
            &Rect {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
        );
    }
}

/// The text of item `index` of a combo box: the same as `ComboBoxControl::text_at` (W8-18f) — `CB_GETLBTEXTLEN` (`<= 0` gives `""`), then `CB_GETLBTEXT` into a
/// buffer of `length + 1` units and the first `length` units. Kept local so this unit does not depend on the merge order of W8-18f.
fn combo_item_text(combo: Hwnd, index: i32) -> String {
    let Ok(item) = usize::try_from(index) else {
        return String::new();
    };
    // SAFETY: `CB_GETLBTEXTLEN` takes the item index in `wparam` and no pointer.
    let length = unsafe { send_message(combo, CB_GETLBTEXTLEN, item, 0) };
    let Some(length) = usize::try_from(length).ok().filter(|&n| n > 0) else {
        return String::new();
    };
    let mut buffer = vec![0u16; length.saturating_add(1)];
    // SAFETY: `lparam` points to a local buffer of `length + 1` UTF-16 units, the size the control reported for this item; the control writes at most that many
    // units, NUL included, during the synchronous call.
    unsafe {
        send_message(combo, CB_GETLBTEXT, item, buffer.as_mut_ptr() as isize);
    }
    String::from_utf16_lossy(buffer.get(..length).unwrap_or(&[]))
}
