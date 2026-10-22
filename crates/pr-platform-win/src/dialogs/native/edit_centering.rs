//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/EditTextCentering.cs` (167 lines, baseline `d92d6aae`). Unit W8-18e. Contract: `contracts/W8-18e.md`.
//!
//! A single-line EDIT draws its text at the TOP of its client area and ignores `EM_SETRECT` (multiline only), so a box taller than one text line (the number and
//! key boxes, about 44 px at 150 %) had its text stuck to the top edge. The box keeps its full height; its non-client area is enlarged on top (`WM_NCCALCSIZE`)
//! by the amount that moves the digits and capital letters (cap height = ascent minus internal leading) to the vertical middle, and the extra band is painted
//! with the control's own background (`WM_NCPAINT`). The caret, the selection and the hit test follow the shrunken client area by themselves.
//!
//! `unsafe` here: the subclass procedure (`extern "system"`, `win32::comctl::common_controls::{set_window_subclass, def_subclass_proc}`) and the two raw
//! pointers of `WM_NCCALCSIZE`. The procedure runs under `host::pending_panic::guarded` (D-W8B5-207).
use crate::win32::structs::{Hwnd, Rect};

/// The subclass id, ASCII `"EDCT"` (C# `SubclassId`).
pub const SUBCLASS_ID: usize = 0x4544_4354;

/// The window property that marks an edit as attached (C# `"PhotoReview.EditCentering"`; the Rust prefix is `PhotoReviewRs.`, D-W8B5-206).
pub const PROP: &str = "PhotoReviewRs.EditCentering";

/// `WM_CTLCOLORSTATIC` (C# private `WmCtlColorStatic`): the answer for a read-only edit.
pub const WM_CTL_COLOR_STATIC: u32 = 0x0138;

/// `ES_READONLY` (C# private `EsReadOnly`).
pub const ES_READ_ONLY: u32 = 0x0800;

/// C# `Attach(edit)`: subclass `edit` (idempotent) and make its frame recalculate. `false` when `edit == 0`, the window does not exist, or the subclass could
/// not be installed. Order: not attached yet (`get_prop(edit, PROP) == 0`) -> `set_window_subclass(edit, subclass_proc, SUBCLASS_ID, 0)`, `false` on failure
/// (no property set), then `set_prop(edit, PROP, 1)`; then ALWAYS `refresh(edit)` and `true` (also when it was attached before).
pub fn attach(edit: Hwnd) -> bool {
    use crate::win32::comctl::common_controls::set_window_subclass;
    use crate::win32::dialog_chrome::{get_prop, set_prop};
    use crate::win32::dialog_native::user32::is_window;

    if edit == 0 || !is_window(edit) {
        return false;
    }
    if get_prop(edit, PROP) == 0 {
        // SAFETY: `subclass_proc` is a process-lifetime function item and removes itself on WM_NCDESTROY.
        if !unsafe { set_window_subclass(edit, subclass_proc, SUBCLASS_ID, 0) } {
            return false;
        }
        set_prop(edit, PROP, 1);
    }
    refresh(edit);
    true
}

/// C# `Refresh(edit)`: recompute the frame (the font or the size changed): `SetWindowPos(edit, 0, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER |
/// SWP_NOACTIVATE | SWP_FRAMECHANGED)`; the result is ignored.
pub fn refresh(edit: Hwnd) {
    use crate::win32::api::user32::set_window_pos;
    use crate::win32::messages::{SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER};
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
    let _ = set_window_pos(edit, 0, 0, 0, 0, 0, flags);
}

/// C# `TopPadding(edit, clientHeight)`: the rows the client top moves down so that the cap-height ink is centred in a client area `client_height` high.
/// OS part: the control's font (`WM_GETFONT`), a DC (`GetDC(edit)`), `SelectObject(font)` when the font is non-zero, `GetTextMetrics`, restore the old
/// object, `ReleaseDC`; any failure (`GetDC` returns 0, `GetTextMetrics` fails) gives `0`. Then `padding_for_metrics`.
pub fn top_padding(edit: Hwnd, client_height: i32) -> i32 {
    use crate::win32::dialog_native::{gdi32, user32};
    const WM_GETFONT: u32 = 0x0031;
    // SAFETY: WM_GETFONT carries scalar values and returns the control's font handle.
    let font = unsafe { user32::send_message(edit, WM_GETFONT, 0, 0) as Hwnd };
    let dc = user32::get_dc(edit);
    if dc == 0 {
        return 0;
    }
    let old = if font != 0 { gdi32::select_object(dc, font) } else { 0 };
    let metrics = gdi32::get_text_metrics(dc);
    if old != 0 {
        let _ = gdi32::select_object(dc, old);
    }
    let _ = user32::release_dc(edit, dc);
    let Some(metrics) = metrics else {
        return 0;
    };
    padding_for_metrics(
        client_height,
        metrics.tmHeight,
        metrics.tmAscent,
        metrics.tmInternalLeading,
    )
}

/// The arithmetic of `TopPadding` (pure): `cap = max(1, ascent - internal_leading)`; `padding = (client_height - cap) / 2 - 1 - internal_leading` (integer
/// division, truncating); `limit = client_height - (height + 2)`; `clamp(padding, 0, max(0, limit))`. `height`, `ascent`, `internal_leading` are
/// `TEXTMETRIC.tmHeight`, `tmAscent` and `tmInternalLeading`. The system draws the line at client top + 1, so the ink starts `1 + internal_leading` below the
/// client top; after moving the top down by `p` the gap above the ink is `p + 1 + internal_leading`, centred when it equals `(client_height - cap) / 2`. The
/// shrunken client must still hold one full line plus the 1 px the system leaves above it. Uses saturating arithmetic where C# wraps (T3).
pub fn padding_for_metrics(client_height: i32, height: i32, ascent: i32, internal_leading: i32) -> i32 {
    let cap = ascent.saturating_sub(internal_leading).max(1);
    let padding = (client_height.saturating_sub(cap) / 2)
        .saturating_sub(1)
        .saturating_sub(internal_leading);
    let limit = client_height.saturating_sub(height.saturating_add(2)).max(0);
    padding.clamp(0, limit)
}

/// The rectangle of the band that `WM_NCPAINT` fills, in window coordinates (pure part of C# `PaintBand`): `edge = (window_width - client_width) / 2` (the
/// border is symmetric), `client_top = window_height - client_height - edge` (the bottom edge is not enlarged), `None` when `client_top <= edge`, else
/// `Rect { left: edge, top: edge, right: window_width - edge, bottom: client_top }`.
pub fn band_rect(window_width: i32, window_height: i32, client_width: i32, client_height: i32) -> Option<Rect> {
    let edge = window_width.saturating_sub(client_width) / 2;
    let client_top = window_height.saturating_sub(client_height).saturating_sub(edge);
    (client_top > edge).then_some(Rect {
        left: edge,
        top: edge,
        right: window_width.saturating_sub(edge),
        bottom: client_top,
    })
}

/// C# `PaintBand(hwnd)`: `GetWindowRect` and `GetClientRect` (either failing returns); `band_rect` (`None` returns); `GetWindowDC`; the style
/// (`GWL_STYLE`) decides the message, `WM_CTLCOLORSTATIC` when `ES_READONLY` is set, else `WM_CTLCOLOREDIT`; the brush is the answer of the first ancestor
/// (walking `GetParent` upwards while the brush is 0) to `SendMessage(parent, message, hdc, hwnd)` (the page viewport may sit between the box and the dialog);
/// when non-zero `FillRect(hdc, band, brush)` (the brush belongs to the dialog: never deleted here); `ReleaseDC(hwnd, hdc)` on every path after `GetWindowDC`.
fn paint_band(hwnd: Hwnd) {
    use crate::win32::api::user32::{get_client_rect, get_window_long_ptr, get_window_rect};
    use crate::win32::dialog_chrome::get_window_dc;
    use crate::win32::dialog_native::user32;
    use crate::win32::messages::GWL_STYLE;
    let (Ok(window), Ok(client)) = (get_window_rect(hwnd), get_client_rect(hwnd)) else {
        return;
    };
    let Some(band) = band_rect(
        window.right - window.left,
        window.bottom - window.top,
        client.right - client.left,
        client.bottom - client.top,
    ) else {
        return;
    };
    let dc = get_window_dc(hwnd);
    if dc == 0 {
        return;
    }
    let style = get_window_long_ptr(hwnd, GWL_STYLE) as u32;
    let msg = if style & ES_READ_ONLY != 0 {
        WM_CTL_COLOR_STATIC
    } else {
        0x0133
    };
    let mut parent = crate::win32::comctl::user32::get_parent(hwnd);
    let mut brush = 0;
    while parent != 0 && brush == 0 {
        // SAFETY: WM_CTLCOLOR* transports only HDC/HWND scalar handles synchronously.
        brush = unsafe { user32::send_message(parent, msg, dc as usize, hwnd) as isize };
        parent = crate::win32::comctl::user32::get_parent(parent);
    }
    if brush != 0 {
        let _ = crate::win32::comctl::user32::fill_rect(dc, &band, brush);
    }
    let _ = user32::release_dc(hwnd, dc);
}

/// The subclass procedure (C# `[UnmanagedCallersOnly] Proc`). `WM_NCCALCSIZE`: `result = DefSubclassProc`; then, if `lParam` is non-null and aligned, the
/// first `RECT` at `lParam` is the client area in both forms (`wParam != 0`: `NCCALCSIZE_PARAMS.rgrc[0]`; `wParam == 0`: the `RECT` itself) and, when
/// `bottom > top`, `top += top_padding(hwnd, bottom - top)`; returns `result`. `WM_NCPAINT`: `DefSubclassProc`, then `paint_band(hwnd)`, returns the default
/// result. `WM_NCDESTROY`: `remove_window_subclass(hwnd, subclass_proc, id)` and `remove_prop(hwnd, PROP)`, then falls through to `DefSubclassProc`. Every
/// other message: `DefSubclassProc`. The switch runs under `pending_panic::guarded(None, ..)` (D-W8B5-207) and yields `Some(result)` only for the two early
/// returns; `None` (other messages, `WM_NCDESTROY`, or a captured panic) falls through to the final `DefSubclassProc` OUTSIDE the guard, as the C# `catch` does.
unsafe extern "system" fn subclass_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    id: usize,
    _ref_data: usize,
) -> isize {
    use crate::win32::comctl::common_controls::{def_subclass_proc, remove_window_subclass};
    use crate::win32::dialog_chrome::remove_prop;
    const WM_NCPAINT: u32 = 0x0085;
    const WM_NCDESTROY: u32 = 0x0082;
    use crate::host::pending_panic::guarded;
    use crate::win32::messages::WM_NCCALCSIZE;
    let handled = guarded(None, || {
        match message {
            WM_NCCALCSIZE => {
                // SAFETY: the synchronous system message supplies an aligned RECT at lParam in both NCCALCSIZE forms.
                if lparam != 0 && (lparam as usize) % std::mem::align_of::<Rect>() == 0 {
                    // SAFETY: the non-null, aligned pointer is the RECT supplied for this synchronous message.
                    let rect = unsafe { &mut *(lparam as *mut Rect) };
                    if rect.bottom > rect.top {
                        let padding = top_padding(hwnd, rect.bottom.saturating_sub(rect.top));
                        rect.top = rect.top.saturating_add(padding);
                    }
                }
                // SAFETY: forwarded unchanged from the subclass callback.
                Some(unsafe { def_subclass_proc(hwnd, message, wparam, lparam) })
            }
            WM_NCPAINT => {
                // SAFETY: forwarded unchanged from the subclass callback.
                let result = unsafe { def_subclass_proc(hwnd, message, wparam, lparam) };
                paint_band(hwnd);
                Some(result)
            }
            WM_NCDESTROY => {
                // SAFETY: same callback and id used at installation.
                remove_window_subclass(hwnd, subclass_proc, id);
                remove_prop(hwnd, PROP);
                None
            }
            _ => None,
        }
    });
    handled.unwrap_or_else(|| {
        // SAFETY: forwarded unchanged from the subclass callback after panic capture.
        unsafe { def_subclass_proc(hwnd, message, wparam, lparam) }
    })
}
