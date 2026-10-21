//! Port of `PhotoReview.Shell.Win32/Hosting/ShellWindow.cs` `ApplyDpiChange` (lines 357-375), `ApplyMinTrackSize` (388-401), `LowWord`/`HighWord`
//! (468, 470) (baseline `d92d6aae`). Unit W8-13f.
//!
//! Per-monitor v2 DPI handling. `WM_DPICHANGED`: `LowWord(wParam)` is the new dpi, `lParam` points to the RECT the OS suggests for the window at that
//! dpi. The window stores the dpi, applies the suggested rectangle with ONE `SetWindowPos` (no z-order change, no activation), raises
//! `ClientSizeChanged` if the DIP size changed (the `WM_SIZE` that `SetWindowPos` provoked already raised it when the pixel size changed; with the same
//! pixel size and a different dpi the DIP size still changes) and ONLY THEN raises `DpiChanged`, invalidates, and re-applies the window icons for the new dpi.
//! `WM_GETMINMAXINFO`: the minimum track size is `(min_width_dip, min_height_dip)` scaled to the current dpi and grown by the window frame
//! (`AdjustWindowRectExForDpi`).

use super::create::{ShellWindowInner, dpi_scale_of, to_pixels};
use crate::host::contracts::{ShellWindowEvent, new_rect, rect_height, rect_width};
use crate::host::native::MinMaxInfo;
use crate::win32::api::user32;
use crate::win32::messages::{SWP_NOACTIVATE, SWP_NOZORDER, WS_OVERLAPPEDWINDOW};
use crate::win32::structs::Rect;

/// C# `LowWord(nint)`: `(int)((ulong)value & 0xFFFF)`; sign-extension of `value` never matters (the mask comes first).
pub fn low_word(value: isize) -> i32 {
    (value as u64 & 0xFFFF) as i32
}

/// C# `HighWord(nint)`: `(int)(((ulong)value >> 16) & 0xFFFF)` (a LOGICAL shift of the 64-bit pattern, not an arithmetic one).
pub fn high_word(value: isize) -> i32 {
    ((value as u64 >> 16) & 0xFFFF) as i32
}

/// The pure part of C# `ApplyMinTrackSize`: the frame-grown rectangle size of `(min_width_dip, min_height_dip)` at `scale`. `grow` stands for
/// `AdjustWindowRectExForDpi(&rect, WS_OVERLAPPEDWINDOW, false, 0, dpi)` (it receives the client rect and returns the window rect, `None` on failure).
/// Returns `None` when `grow` fails: `MinTrackSize` is then left unchanged.
pub fn min_track_size(
    min_width_dip: f64,
    min_height_dip: f64,
    scale: f64,
    grow: &dyn Fn(Rect) -> Option<Rect>,
) -> Option<(i32, i32)> {
    let client = new_rect(0, 0, to_pixels(min_width_dip, scale), to_pixels(min_height_dip, scale));
    grow(client).map(|frame| (rect_width(&frame), rect_height(&frame)))
}

impl ShellWindowInner {
    /// C# `ApplyDpiChange(newDpi, suggested)`: `new_dpi > 0` stores it (0 or negative keeps the old dpi); `suggested = Some(rect)` -> `SetWindowPos(hwnd, 0,
    /// left, top, width, height, SWP_NOZORDER | SWP_NOACTIVATE)`; then `raise_client_size_changed_if_needed()`, raise `DpiChanged`, `invalidate()`.
    /// (The `icons::apply(hwnd, dpi)` call that follows in `ProcessMessage` stays in `process_message`.)
    pub(super) fn apply_dpi_change(&self, new_dpi: i32, suggested: Option<Rect>) {
        if new_dpi > 0 {
            // Stored BEFORE SetWindowPos: the WM_SIZE it provokes computes DIPs with the new scale.
            self.dpi.set(new_dpi as u32);
        }
        if let Some(rect) = suggested {
            let _ = user32::set_window_pos(
                self.hwnd.get(),
                0,
                rect.left,
                rect.top,
                rect_width(&rect),
                rect_height(&rect),
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        // WM_SIZE already raised it when the pixel size changed; with the same pixels and another dpi the DIP size still changes.
        self.raise_client_size_changed_if_needed();
        self.raise_event(ShellWindowEvent::DpiChanged);
        self.invalidate();
    }

    /// C# `ApplyMinTrackSize(MINMAXINFO*)`: `None` (null pointer) does nothing; else `min_track_size(..)` and, when it is `Some((w, h))`, writes
    /// `ptMinTrackSize.x/y`. The `lParam` -> `&mut MinMaxInfo` conversion (the one `unsafe` of this path, with its `// SAFETY:`) is in `process_message`.
    pub(super) fn apply_min_track_size(&self, info: Option<&mut MinMaxInfo>) {
        let Some(info) = info else {
            return;
        };
        let dpi = self.dpi.get();
        let size = min_track_size(
            self.options.min_width_dip,
            self.options.min_height_dip,
            dpi_scale_of(dpi),
            &|mut rect| {
                user32::adjust_window_rect_ex_for_dpi(&mut rect, WS_OVERLAPPEDWINDOW, false, 0, dpi)
                    .ok()
                    .map(|()| rect)
            },
        );
        if let Some((width, height)) = size {
            info.ptMinTrackSize.x = width;
            info.ptMinTrackSize.y = height;
        }
    }
}
