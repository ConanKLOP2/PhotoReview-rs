//! Port of `NativeDialogChromePainter.cs` lines 751-775, 899-990 (baseline `d92d6aae`): the group box, the etched lines, the list view header filler and the flat
//! border painted over edit boxes, list boxes, list views and progress bars at `WM_NCPAINT`. Unit W8-18n. Contract: `contracts/W8-18n.md`.
//!
//! Pure core functions (`border_thickness`, `border_layers`, `group_box_layout`, `etched_shape`, `header_filler_rects`) and thin GDI functions around them.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use super::geometry::ChromePaintKind;
use crate::win32::structs::{Hdc, Hwnd, Rect};

// ---- border thickness and flat border -------------------------------------------------------------------------------------------------------------------------------

/// The pure core of `BorderThickness(hwnd)` (lines 939-960): `(edge, client_edge)`. `dpi = max(96, dpi)`; `ex_style & WS_EX_CLIENTEDGE (0x200)` -> `(max(2,
/// metric(SM_CXEDGE (45), dpi)), true)`; else `ex_style & WS_EX_STATICEDGE (0x20000)` -> `(max(1, metric(SM_CXBORDER (5), dpi)), false)`; else `style & WS_BORDER
/// (0x800000) != 0 && style & WS_DLGFRAME (0x400000) != WS_DLGFRAME` -> `(max(1, metric(SM_CXBORDER, dpi)), false)`; else `(0, false)`. `metric` is
/// `GetSystemMetricsForDpi`, passed in so the function is pure.
pub fn border_thickness(style: u32, ex_style: u32, dpi: u32, metric: &dyn Fn(i32, u32) -> i32) -> (i32, bool) {
    todo!()
}

/// C# `BorderThickness(hwnd)`: `border_thickness(GWL_STYLE, GWL_EXSTYLE, get_dpi_for_window(hwnd), &get_system_metrics_for_dpi)`. `pub(super)` for `widgets`.
pub(super) fn border_thickness_of(hwnd: Hwnd) -> (i32, bool) {
    todo!()
}

/// One layer of the flat border: `inset` pixels from the window edge, `outer` for the strong outline (inset 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderLayer {
    pub inset: i32,
    /// `true` only for the first layer (colour `Dark.BorderStrong`); the inner layers use the control's own background colour.
    pub outer: bool,
}

/// The layers of `PaintBorder` (lines 962-990) for a window of `width x height` (window coordinates, `GetWindowDC`): `edge <= 0` -> empty; else the outer layer
/// (inset 0); and, only when `client_edge && edge > 1`, the inner layers `inset = 1 .. edge - 1`. Each layer is a 1 px frame `(inset, inset, width - inset,
/// height - inset)`.
pub fn border_layers(width: i32, height: i32, edge: i32, client_edge: bool) -> Vec<BorderLayer> {
    todo!()
}

/// C# `PaintBorder(hwnd, hdc, kind)`: `border_thickness_of`; `edge <= 0` -> nothing; `get_window_rect` fails -> nothing; layers over `border_layers(width, height,
/// ..)`; outer layer `Dark.BorderStrong`; inner layers `back` = `LVM_GETBKCOLOR` of the control (a `COLORREF`) for `ListViewBorder`, `Dark.Surface` for `Progress`,
/// `Dark.Input` otherwise. `frame` with thickness 1 per layer.
pub(super) fn paint_border(hwnd: Hwnd, hdc: Hdc, kind: ChromePaintKind) {
    todo!()
}

// ---- group box ------------------------------------------------------------------------------------------------------------------------------------------------------

/// What `DrawGroupBox` paints, as data.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupBoxLayout {
    /// The 1 px outline `(0, text_height / 2, width, height)` in `Dark.BorderStrong`.
    pub frame: Rect,
    /// `None` for an empty caption: only the frame is drawn.
    pub label: Option<GroupBoxLabel>,
}

/// The caption of a group box.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupBoxLabel {
    /// The patch that hides the frame behind the caption, filled with `Dark.Window`: `(left - pad, 0, left + shown + pad, text_height)`.
    pub patch: Rect,
    /// The text rectangle `(left, 0, left + shown, text_height)`.
    pub text_rect: Rect,
    /// `DT_SINGLELINE | DT_END_ELLIPSIS`, `| DT_HIDEPREFIX` when the accelerators are hidden.
    pub text_flags: u32,
}

/// The pure part of `DrawGroupBox` (lines 751-775): `pad = S(3)`, `left = S(8)`, `shown = min(max(0, width - left - 2 * pad), text_width)`; `frame` as above;
/// `label` is `None` when `has_text` is false.
pub fn group_box_layout(
    width: i32,
    height: i32,
    scale: f64,
    text_height: i32,
    text_width: i32,
    has_text: bool,
    hide_accel: bool,
) -> GroupBoxLayout {
    todo!()
}

/// C# `DrawGroupBox(hwnd, hdc, width, height, scale)`: reads the text, `text_height(hdc)`, `text_width`, `ui_state`; `frame` the outline, then the patch and the
/// text (`Dark.TextStrong` enabled, `Dark.TextMuted` disabled).
pub(super) fn draw_group_box(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64) {
    todo!()
}

// ---- etched line ----------------------------------------------------------------------------------------------------------------------------------------------------

/// What `DrawEtched` paints (always in `Dark.Border`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EtchedShape {
    /// A filled 1 px line (`SS_ETCHEDHORZ` / `SS_ETCHEDVERT`).
    Line(Rect),
    /// A 1 px frame (`SS_ETCHEDFRAME` and any other type).
    Frame(Rect),
}

/// The pure part of `DrawEtched` (lines 899-915); `style_type = style & SS_TYPEMASK`. `SS_ETCHEDHORZ (0x10)` -> `Line(0, height / 2, width, height / 2 + 1)`;
/// `SS_ETCHEDVERT (0x11)` -> `Line(width / 2, 0, width / 2 + 1, height)`; anything else -> `Frame(0, 0, width, height)`.
pub fn etched_shape(style_type: u32, width: i32, height: i32) -> EtchedShape {
    todo!()
}

/// C# `DrawEtched(hwnd, hdc, width, height)`.
pub(super) fn draw_etched(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32) {
    todo!()
}

// ---- header filler ----------------------------------------------------------------------------------------------------------------------------------------------------

/// The pure part of `PaintHeaderFiller` (lines 917-934): `None` when `last_item.right >= client.right` (no free area); else `[surface, line]`: `surface =
/// (last_item.right, client.top, client.right, client.bottom)` in `Dark.Surface` and `line = (last_item.right, client.bottom - 1, client.right, client.bottom)` in
/// `Dark.BorderSubtle`.
pub fn header_filler_rects(client: Rect, last_item: Rect) -> Option<[Rect; 2]> {
    todo!()
}

/// C# `PaintHeaderFiller(header, hdc)`: `count = HDM_GETITEMCOUNT`; `count <= 0` or `get_client_rect` fails -> nothing; `last = HDM_GETITEMRECT(count - 1)` (a
/// pointer to a local `RECT`, SAFETY comment; the message answers `0` on failure -> nothing); `header_filler_rects`; fill the two rectangles.
pub(super) fn paint_header_filler(header: Hwnd, hdc: Hdc) {
    todo!()
}
