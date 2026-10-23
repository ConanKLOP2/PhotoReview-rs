//! Port of `NativeDialog.cs` lines 1030 to 1053 (`DpiFor`, `ApplyFonts`), 1076 to 1145 (`DeleteIfSet`, `CreateFonts`, `MeasureUnits`, `Dx`, `Dy`,
//! `LineHeight`) and 1433 to 1458 (`OnDpiChanged`) plus the `WM_DPICHANGED` case of `HandleMessage` (lines 635 to 637), baseline `d92d6aae`. Unit W8-18i. Contract: `contracts/W8-18i.md`.
//!
//! Per-monitor DPI: the dialog fonts come from `SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS)` at the dialog's DPI, the dialog-unit size is measured from
//! the body font (4 dlu = one average character, 8 dlu = one line), and a `WM_DPICHANGED` recreates the fonts and runs the layout again. The system never
//! scales the dialog for us.
use std::cell::Cell;
use std::collections::HashSet;

use super::core::NativeDialog;
use crate::host::icons;
use crate::win32::structs::{Hgdi, Hwnd, Lparam, Rect, Wparam};
use crate::win32::{api::user32, dialog_native};

/// C# `s_alphabet`: the 52 letters whose width gives the average character width.
pub const ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Font, DPI and unit state (C# `_dpi`, `_font`, `_headerFont`, `_avgCharWidth`, `_lineHeight`).
pub(super) struct MetricsState {
    /// C# `_dpi`, starts at `96`.
    pub(super) dpi: Cell<u32>,
    /// C# `_font`: the body font (an `HFONT` owned by the dialog, deleted by `cleanup`).
    pub(super) font: Cell<Hgdi>,
    /// C# `_headerFont`: the larger bold font, `0` when no control uses it.
    pub(super) header_font: Cell<Hgdi>,
    /// C# `_avgCharWidth`, starts at `6`.
    pub(super) avg_char_width: Cell<i32>,
    /// C# `_lineHeight`, starts at `15`.
    pub(super) line_height: Cell<i32>,
}

impl MetricsState {
    /// `dpi = 96`, fonts `0`, `avg_char_width = 6`, `line_height = 15`.
    #[allow(dead_code)] // The 18h constructor still initializes fields inline; a later slice will use this constructor.
    pub(super) fn new() -> Self {
        Self {
            dpi: Cell::new(96),
            font: Cell::new(0),
            header_font: Cell::new(0),
            avg_char_width: Cell::new(6),
            line_height: Cell::new(15),
        }
    }
}

/// C# `Dx` arithmetic (pure): `(units * avg_char_width + 2) / 4`, `i32` wrapping like the C# unchecked `int`, `/` truncates toward zero.
pub fn scale_dlu_x(units: i32, avg_char_width: i32) -> i32 {
    units.wrapping_mul(avg_char_width).wrapping_add(2) / 4
}

/// C# `Dy` arithmetic (pure): `(units * line_height + 4) / 8`, same arithmetic rules.
pub fn scale_dlu_y(units: i32, line_height: i32) -> i32 {
    units.wrapping_mul(line_height).wrapping_add(4) / 8
}

/// C# `MeasureUnits` arithmetic (pure): `max(1, (extent_x / 26 + 1) / 2)` where `extent_x` is the width of [`ALPHABET`] (truncating division).
pub fn average_char_width(extent_x: i32) -> i32 {
    (extent_x / 26).wrapping_add(1).wrapping_div(2).max(1)
}

/// C# `DpiFor` arithmetic (pure): `window_dpi` when non-zero, else `max(system_dpi, 96)`.
pub fn effective_dpi(window_dpi: u32, system_dpi: u32) -> u32 {
    if window_dpi != 0 {
        window_dpi
    } else {
        system_dpi.max(96)
    }
}

/// C# fallback font height (pure): `-(int)Math.Round(9.0 * dpi / 72.0)` with banker's rounding (`round_ties_even`), e.g. `96 -> -12`, `100 -> -12`,
/// `108 -> -14`, `144 -> -18`, `192 -> -24`.
pub fn fallback_font_height(dpi: u32) -> i32 {
    -((9.0_f64 * f64::from(dpi) / 72.0).round_ties_even() as i32)
}

/// C# header font height (pure): `body_height * 14 / 10` with `i32` wrapping multiplication and truncating division (the height is negative: "by character").
pub fn header_font_height(body_height: i32) -> i32 {
    body_height.wrapping_mul(14) / 10
}

/// The fallback `LOGFONTW` of `CreateFonts` (pure): all fields zero except `lfHeight = fallback_font_height(dpi)`, `lfWeight = 400`, `lfCharSet = 1`
/// (`DEFAULT_CHARSET`), `lfQuality = 5` (`CLEARTYPE_QUALITY`), `lfFaceName = "Segoe UI"` (UTF-16, NUL padded).
pub fn fallback_log_font(dpi: u32) -> windows::Win32::Graphics::Gdi::LOGFONTW {
    let mut font = windows::Win32::Graphics::Gdi::LOGFONTW {
        lfHeight: fallback_font_height(dpi),
        lfWeight: 400,
        lfCharSet: windows::Win32::Graphics::Gdi::FONT_CHARSET(1),
        lfQuality: windows::Win32::Graphics::Gdi::FONT_QUALITY(5),
        ..Default::default()
    };
    for (slot, unit) in font.lfFaceName.iter_mut().zip("Segoe UI".encode_utf16()) {
        *slot = unit;
    }
    font
}

/// The header variant of a body font (pure): a copy with `lfHeight = header_font_height(lfHeight)` and `lfWeight = 600`.
pub fn header_log_font(body: &windows::Win32::Graphics::Gdi::LOGFONTW) -> windows::Win32::Graphics::Gdi::LOGFONTW {
    let mut font = *body;
    font.lfHeight = header_font_height(body.lfHeight);
    font.lfWeight = 600;
    font
}

impl NativeDialog {
    /// C# `Dx(dialogUnits)`: `scale_dlu_x(units, avg_char_width)`.
    pub fn dx(&self, dialog_units: i32) -> i32 {
        scale_dlu_x(dialog_units, self.inner.metrics.avg_char_width.get())
    }

    /// C# `Dy(dialogUnits)`: `scale_dlu_y(units, line_height)`.
    pub fn dy(&self, dialog_units: i32) -> i32 {
        scale_dlu_y(dialog_units, self.inner.metrics.line_height.get())
    }

    /// C# `LineHeight`: the height of one text line in px.
    pub fn line_height(&self) -> i32 {
        self.inner.metrics.line_height.get()
    }

    /// C# `CreateFonts(dpi, header, out body, out headerFont)` (`internal static`, lines 1084 to 1112): `(body, header)` font handles, `header = 0` when not
    /// asked for. The message font is `dialog_native::user32::non_client_metrics_for_dpi(dpi)` -> `lfMessageFont`; `Err` or `lfHeight == 0` -> `fallback_log_font(dpi)`.
    /// `body = create_font_indirect(&log_font)` (`0` on failure, not an error); when `header`: `create_font_indirect(&header_log_font(&log_font))`.
    pub(super) fn create_fonts(dpi: u32, header: bool) -> (Hgdi, Hgdi) {
        let font = dialog_native::user32::non_client_metrics_for_dpi(dpi)
            .ok()
            .map(|metrics| metrics.lfMessageFont)
            .filter(|font| font.lfHeight != 0)
            .unwrap_or_else(|| fallback_log_font(dpi));
        let body = dialog_native::gdi32::create_font_indirect(&font);
        let header_font = if header {
            dialog_native::gdi32::create_font_indirect(&header_log_font(&font))
        } else {
            0
        };
        (body, header_font)
    }

    /// C# `DpiFor(hwnd)`: `effective_dpi(hwnd != 0 ? get_dpi_for_window(hwnd) : 0, get_dpi_for_system())`.
    #[allow(dead_code)] // Dialog creation in the later 18j slice obtains its initial DPI here.
    pub(super) fn dpi_for(hwnd: Hwnd) -> u32 {
        effective_dpi(
            if hwnd == 0 { 0 } else { user32::get_dpi_for_window(hwnd) },
            user32::get_dpi_for_system(),
        )
    }

    /// C# `ApplyFonts()` (lines 1037 to 1053): recreates the fonts for `metrics.dpi`, gives the body font to the dialog (`WM_SETFONT`) and to every registered
    /// child, except the controls of `content.header_control_ids()`, which get the header font when it exists; THEN deletes the previous fonts (a font in use is
    /// never deleted); then `measure_units()`.
    pub(super) fn apply_fonts(&self) {
        let old_font = self.inner.metrics.font.get();
        let old_header = self.inner.metrics.header_font.get();
        let headers: HashSet<i32> = self.inner.content.header_control_ids().into_iter().collect();
        let (font, header) = Self::create_fonts(self.dpi(), !headers.is_empty());
        self.inner.metrics.font.set(font);
        self.inner.metrics.header_font.set(header);
        let hwnd = self.hwnd();
        // SAFETY: WM_SETFONT carries only a font handle and an integer flag.
        unsafe {
            dialog_native::user32::send_message(hwnd, 0x0030, font as usize, 0);
        }
        let children = self.inner.controls.children.borrow().clone();
        for child in children {
            let id = dialog_native::user32::get_dlg_ctrl_id(child);
            let selected = if headers.contains(&id) && header != 0 {
                header
            } else {
                font
            };
            // SAFETY: WM_SETFONT carries only a font handle and an integer flag.
            unsafe {
                dialog_native::user32::send_message(child, 0x0030, selected as usize, 0);
            }
        }
        if old_font != 0 {
            let _ = dialog_native::gdi32::delete_object(old_font);
        }
        if old_header != 0 {
            let _ = dialog_native::gdi32::delete_object(old_header);
        }
        self.measure_units();
    }

    /// C# `MeasureUnits()` (lines 1115 to 1136): `get_dc(hwnd)` (`0` -> return, units unchanged), select the body font, `get_text_extent_point32(ALPHABET)`;
    /// on success `avg_char_width = average_char_width(x)` and `line_height = max(1, y)`; reselect the old font; `release_dc`.
    pub(super) fn measure_units(&self) {
        let hwnd = self.hwnd();
        let hdc = dialog_native::user32::get_dc(hwnd);
        if hdc == 0 {
            return;
        }
        let old = dialog_native::gdi32::select_object(hdc, self.body_font());
        if let Some(extent) = dialog_native::gdi32::get_text_extent_point32(hdc, ALPHABET) {
            self.inner.metrics.avg_char_width.set(average_char_width(extent.x));
            self.inner.metrics.line_height.set(extent.y.max(1));
        }
        let _ = dialog_native::gdi32::select_object(hdc, old);
        let _ = dialog_native::user32::release_dc(hwnd, hdc);
    }

    /// C# `OnDpiChanged(newDpi, suggested)` (lines 1433 to 1458). `new_dpi == 0 || new_dpi == dpi || hwnd == 0` -> nothing. Else `dpi = new_dpi`;
    /// `host::icons::apply(hwnd, new_dpi)`; `apply_fonts()`; resizable -> `set_window_pos(hwnd, 0, s.left, s.top, s.right - s.left, s.bottom - s.top,
    /// SWP_NOZORDER | SWP_NOACTIVATE)` (the system's suggestion, already scaled); not resizable -> `size = preferred_client_size`, `frame =
    /// window_size_for_client(size)`, `set_window_pos(hwnd, 0, s.left, s.top, frame.0, frame.1, SWP_NOZORDER | SWP_NOACTIVATE)`; then `run_layout()`.
    /// `suggested = None` (a null `lParam`, impossible from the OS) skips the `set_window_pos` only.
    pub(super) fn on_dpi_changed(&self, new_dpi: u32, suggested: Option<Rect>) {
        let hwnd = self.hwnd();
        if new_dpi == 0 || new_dpi == self.dpi() || hwnd == 0 {
            return;
        }
        self.inner.metrics.dpi.set(new_dpi);
        let _ = icons::apply(hwnd, new_dpi);
        self.apply_fonts();
        if let Some(rect) = suggested {
            let (w, h) = if self.inner.content.resizable() {
                (rect.right.wrapping_sub(rect.left), rect.bottom.wrapping_sub(rect.top))
            } else {
                let (w, h) = self.inner.content.preferred_client_size(self);
                self.window_size_for_client(w, h)
            };
            let _ = user32::set_window_pos(hwnd, 0, rect.left, rect.top, w, h, 0x0004 | 0x0010);
        }
        self.run_layout();
    }

    /// The `WM_DPICHANGED` entry of the router: `new_dpi = (wparam & 0xFFFF) as u32`; `lparam` -> `Option<Rect>` (a null pointer is `None`) under a SAFETY
    /// comment (`WM_DPICHANGED` `lParam` points to a `RECT` valid for the message); calls `on_dpi_changed`.
    pub(super) fn on_dpi_changed_message(&self, wparam: Wparam, lparam: Lparam) {
        let suggested = if lparam == 0 {
            None
        } else {
            // SAFETY: WM_DPICHANGED supplies a RECT valid for the duration of the message; copy it once.
            Some(unsafe { *(lparam as *const Rect) })
        };
        self.on_dpi_changed((wparam & 0xffff) as u32, suggested);
    }
}
