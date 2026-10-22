//! Port of `NativeDialogChromePainter.cs` lines 751-775, 899-990 (baseline `d92d6aae`): the group box, the etched lines, the list view header filler and the flat
//! border painted over edit boxes, list boxes, list views and progress bars at `WM_NCPAINT`. Unit W8-18n. Contract: `contracts/W8-18n.md`.
//!
//! Pure core functions (`border_thickness`, `border_layers`, `group_box_layout`, `etched_shape`, `header_filler_rects`) and thin GDI functions around them.

use super::controls::{draw_text, fill, frame, text_height, text_width, ui_state, window_text};
use super::geometry::{ChromePaintKind, color, keys, scaled};
use crate::dialogs::native::chrome::DialogColors;
use crate::win32::api::user32::{
    get_client_rect, get_dpi_for_window, get_system_metrics_for_dpi, get_window_long_ptr, get_window_rect,
};
use crate::win32::dialog_chrome::{
    DT_HIDEPREFIX, HDM_GETITEMCOUNT, HDM_GETITEMRECT, LVM_GETBKCOLOR, SM_CXBORDER, SM_CXEDGE, SS_ETCHEDHORZ,
    SS_ETCHEDVERT, SS_TYPEMASK, WS_BORDER, WS_DLGFRAME, WS_EX_CLIENTEDGE, WS_EX_STATICEDGE,
};
use crate::win32::dialog_native::user32::{is_window_enabled, send_message};
use crate::win32::dialog_native::{DT_END_ELLIPSIS, DT_SINGLELINE};
use crate::win32::messages::{GWL_EXSTYLE, GWL_STYLE};
use crate::win32::structs::{Hdc, Hwnd, Rect};

/// The most flat-border layers `border_layers` returns (the real metrics are single digits; the cap keeps a silly input from allocating without bound).
const MAX_BORDER_LAYERS: i32 = 1024;

// ---- border thickness and flat border -------------------------------------------------------------------------------------------------------------------------------

/// The pure core of `BorderThickness(hwnd)` (lines 939-960): `(edge, client_edge)`. `dpi = max(96, dpi)`; `ex_style & WS_EX_CLIENTEDGE (0x200)` -> `(max(2,
/// metric(SM_CXEDGE (45), dpi)), true)`; else `ex_style & WS_EX_STATICEDGE (0x20000)` -> `(max(1, metric(SM_CXBORDER (5), dpi)), false)`; else `style & WS_BORDER
/// (0x800000) != 0 && style & WS_DLGFRAME (0x400000) != WS_DLGFRAME` -> `(max(1, metric(SM_CXBORDER, dpi)), false)`; else `(0, false)`. `metric` is
/// `GetSystemMetricsForDpi`, passed in so the function is pure.
pub fn border_thickness(style: u32, ex_style: u32, dpi: u32, metric: &dyn Fn(i32, u32) -> i32) -> (i32, bool) {
    let dpi = dpi.max(96);
    if ex_style & WS_EX_CLIENTEDGE != 0 {
        return (metric(SM_CXEDGE, dpi).max(2), true);
    }
    if ex_style & WS_EX_STATICEDGE != 0 {
        return (metric(SM_CXBORDER, dpi).max(1), false);
    }
    if style & WS_BORDER != 0 && style & WS_DLGFRAME != WS_DLGFRAME {
        return (metric(SM_CXBORDER, dpi).max(1), false);
    }
    (0, false)
}

/// C# `BorderThickness(hwnd)`: `border_thickness(GWL_STYLE, GWL_EXSTYLE, get_dpi_for_window(hwnd), &get_system_metrics_for_dpi)`. `pub(super)` for `widgets`.
pub(super) fn border_thickness_of(hwnd: Hwnd) -> (i32, bool) {
    let style = get_window_long_ptr(hwnd, GWL_STYLE) as u32;
    let ex_style = get_window_long_ptr(hwnd, GWL_EXSTYLE) as u32;
    border_thickness(style, ex_style, get_dpi_for_window(hwnd), &get_system_metrics_for_dpi)
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
    let _ = (width, height); // every layer is a frame of the window size inset by `inset`: the size is only needed by the caller
    if edge <= 0 {
        return Vec::new();
    }
    let mut layers = vec![BorderLayer { inset: 0, outer: true }];
    if client_edge && edge > 1 {
        layers.extend((1..edge.min(MAX_BORDER_LAYERS)).map(|inset| BorderLayer { inset, outer: false }));
    }
    layers
}

/// C# `PaintBorder(hwnd, hdc, kind)`: `border_thickness_of`; `edge <= 0` -> nothing; `get_window_rect` fails -> nothing; layers over `border_layers(width, height,
/// ..)`; outer layer `Dark.BorderStrong`; inner layers `back` = `LVM_GETBKCOLOR` of the control (a `COLORREF`) for `ListViewBorder`, `Dark.Surface` for `Progress`,
/// `Dark.Input` otherwise. `frame` with thickness 1 per layer.
pub(super) fn paint_border(hwnd: Hwnd, hdc: Hdc, kind: ChromePaintKind) {
    let (edge, client_edge) = border_thickness_of(hwnd);
    if edge <= 0 {
        return;
    }
    let Ok(window) = get_window_rect(hwnd) else {
        return;
    };
    let width = window.right.wrapping_sub(window.left);
    let height = window.bottom.wrapping_sub(window.top);
    let layers = border_layers(width, height, edge, client_edge);
    // The inner colour is only needed (and asked from the control) when there are inner layers.
    let back = layers.iter().any(|layer| !layer.outer).then(|| match kind {
        ChromePaintKind::ListViewBorder => {
            // SAFETY: `LVM_GETBKCOLOR` takes no parameters; the answer is a `COLORREF`.
            let answer = unsafe { send_message(hwnd, LVM_GETBKCOLOR, 0, 0) };
            answer as u32
        }
        ChromePaintKind::Progress => color(keys::SURFACE),
        _ => color(keys::INPUT),
    });
    for layer in layers {
        let inset = layer.inset;
        let layer_color = if layer.outer {
            color(keys::BORDER_STRONG)
        } else {
            back.unwrap_or_else(|| color(keys::INPUT))
        };
        frame(hdc, inset, inset, width - inset, height - inset, layer_color, 1);
    }
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
    let top = text_height / 2;
    let pad = scaled(scale, 3);
    let left = scaled(scale, 8);
    let shown = (width - left - pad * 2).max(0).min(text_width);
    let label = has_text.then(|| GroupBoxLabel {
        patch: Rect {
            left: left - pad,
            top: 0,
            right: left + shown + pad,
            bottom: text_height,
        },
        text_rect: Rect {
            left,
            top: 0,
            right: left + shown,
            bottom: text_height,
        },
        text_flags: DT_SINGLELINE | DT_END_ELLIPSIS | if hide_accel { DT_HIDEPREFIX } else { 0 },
    });
    GroupBoxLayout {
        frame: Rect {
            left: 0,
            top,
            right: width,
            bottom: height,
        },
        label,
    }
}

/// C# `DrawGroupBox(hwnd, hdc, width, height, scale)`: reads the text, `text_height(hdc)`, `text_width`, `ui_state`; `frame` the outline, then the patch and the
/// text (`Dark.TextStrong` enabled, `Dark.TextMuted` disabled).
pub(super) fn draw_group_box(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32, scale: f64) {
    let enabled = is_window_enabled(hwnd);
    let text = window_text(hwnd);
    let ui = ui_state(hwnd);
    let layout = group_box_layout(
        width,
        height,
        scale,
        text_height(hdc),
        text_width(hdc, &text),
        !text.is_empty(),
        ui.hide_accel,
    );
    let outline = layout.frame;
    frame(
        hdc,
        outline.left,
        outline.top,
        outline.right,
        outline.bottom,
        color(keys::BORDER_STRONG),
        1,
    );
    if let Some(label) = layout.label {
        // The patch hides the frame behind the caption.
        fill(
            hdc,
            label.patch.left,
            label.patch.top,
            label.patch.right,
            label.patch.bottom,
            DialogColors::window(),
        );
        let text_key = if enabled { keys::TEXT_STRONG } else { keys::TEXT_MUTED };
        draw_text(hdc, &text, label.text_rect, color(text_key), label.text_flags);
    }
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
    match style_type {
        SS_ETCHEDHORZ => EtchedShape::Line(Rect {
            left: 0,
            top: height / 2,
            right: width,
            bottom: height / 2 + 1,
        }),
        SS_ETCHEDVERT => EtchedShape::Line(Rect {
            left: width / 2,
            top: 0,
            right: width / 2 + 1,
            bottom: height,
        }),
        _ => EtchedShape::Frame(Rect {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        }),
    }
}

/// C# `DrawEtched(hwnd, hdc, width, height)`.
pub(super) fn draw_etched(hwnd: Hwnd, hdc: Hdc, width: i32, height: i32) {
    let style_type = (get_window_long_ptr(hwnd, GWL_STYLE) as u32) & SS_TYPEMASK;
    let line = color(keys::BORDER);
    match etched_shape(style_type, width, height) {
        EtchedShape::Line(rect) => fill(hdc, rect.left, rect.top, rect.right, rect.bottom, line),
        EtchedShape::Frame(rect) => frame(hdc, rect.left, rect.top, rect.right, rect.bottom, line, 1),
    }
}

// ---- header filler ----------------------------------------------------------------------------------------------------------------------------------------------------

/// The pure part of `PaintHeaderFiller` (lines 917-934): `None` when `last_item.right >= client.right` (no free area); else `[surface, line]`: `surface =
/// (last_item.right, client.top, client.right, client.bottom)` in `Dark.Surface` and `line = (last_item.right, client.bottom - 1, client.right, client.bottom)` in
/// `Dark.BorderSubtle`.
pub fn header_filler_rects(client: Rect, last_item: Rect) -> Option<[Rect; 2]> {
    if last_item.right >= client.right {
        return None;
    }
    Some([
        Rect {
            left: last_item.right,
            top: client.top,
            right: client.right,
            bottom: client.bottom,
        },
        Rect {
            left: last_item.right,
            top: client.bottom.wrapping_sub(1),
            right: client.right,
            bottom: client.bottom,
        },
    ])
}

/// C# `PaintHeaderFiller(header, hdc)`: `count = HDM_GETITEMCOUNT`; `count <= 0` or `get_client_rect` fails -> nothing; `last = HDM_GETITEMRECT(count - 1)` (a
/// pointer to a local `RECT`, SAFETY comment; the message answers `0` on failure -> nothing); `header_filler_rects`; fill the two rectangles.
pub(super) fn paint_header_filler(header: Hwnd, hdc: Hdc) {
    // SAFETY: `HDM_GETITEMCOUNT` takes no parameters.
    let count = unsafe { send_message(header, HDM_GETITEMCOUNT, 0, 0) } as i32;
    if count <= 0 {
        return;
    }
    let Ok(client) = get_client_rect(header) else {
        return;
    };
    let mut last = Rect::default();
    let item = usize::try_from(count - 1).unwrap_or(0);
    // SAFETY: the pointer is to a live local `RECT`; the header fills at most `sizeof(RECT)` bytes during the synchronous call.
    let found = unsafe { send_message(header, HDM_GETITEMRECT, item, std::ptr::from_mut(&mut last) as isize) };
    if found == 0 {
        return;
    }
    if let Some([surface, line]) = header_filler_rects(client, last) {
        fill(
            hdc,
            surface.left,
            surface.top,
            surface.right,
            surface.bottom,
            color(keys::SURFACE),
        );
        fill(
            hdc,
            line.left,
            line.top,
            line.right,
            line.bottom,
            color(keys::BORDER_SUBTLE),
        );
    }
}
