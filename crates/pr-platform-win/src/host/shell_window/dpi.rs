//! Port of `PhotoReview.Shell.Win32/Hosting/ShellWindow.cs` `ApplyDpiChange` (lines 357-375), `ApplyMinTrackSize` (388-401), `LowWord`/`HighWord`
//! (468, 470) (baseline `d92d6aae`). Unit W8-13f.
//!
//! Per-monitor v2 DPI handling. `WM_DPICHANGED`: `LowWord(wParam)` is the new dpi, `lParam` points to the RECT the OS suggests for the window at that
//! dpi. The window stores the dpi, applies the suggested rectangle with ONE `SetWindowPos` (no z-order change, no activation), raises
//! `ClientSizeChanged` if the DIP size changed (the `WM_SIZE` that `SetWindowPos` provoked already raised it when the pixel size changed; with the same
//! pixel size and a different dpi the DIP size still changes) and ONLY THEN raises `DpiChanged`, invalidates, and re-applies the window icons for the new dpi.
//! `WM_GETMINMAXINFO`: the minimum track size is `(min_width_dip, min_height_dip)` scaled to the current dpi and grown by the window frame
//! (`AdjustWindowRectExForDpi`).

#![allow(unused_variables, dead_code)] // W8-13f stub: the porter removes this line when the last `todo!()` is gone.

use super::create::ShellWindowInner;
use crate::host::native::MinMaxInfo;
use crate::win32::structs::Rect;

/// C# `LowWord(nint)`: `(int)((ulong)value & 0xFFFF)`; sign-extension of `value` never matters (the mask comes first).
pub fn low_word(value: isize) -> i32 {
    todo!("W8-13f")
}

/// C# `HighWord(nint)`: `(int)(((ulong)value >> 16) & 0xFFFF)` (a LOGICAL shift of the 64-bit pattern, not an arithmetic one).
pub fn high_word(value: isize) -> i32 {
    todo!("W8-13f")
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
    todo!("W8-13f")
}

impl ShellWindowInner {
    /// C# `ApplyDpiChange(newDpi, suggested)`: `new_dpi > 0` stores it (0 or negative keeps the old dpi); `suggested = Some(rect)` -> `SetWindowPos(hwnd, 0,
    /// left, top, width, height, SWP_NOZORDER | SWP_NOACTIVATE)`; then `raise_client_size_changed_if_needed()`, raise `DpiChanged`, `invalidate()`.
    /// (The `icons::apply(hwnd, dpi)` call that follows in `ProcessMessage` stays in `process_message`.)
    pub(super) fn apply_dpi_change(&self, new_dpi: i32, suggested: Option<Rect>) {
        todo!("W8-13f")
    }

    /// C# `ApplyMinTrackSize(MINMAXINFO*)`: `None` (null pointer) does nothing; else `min_track_size(..)` and, when it is `Some((w, h))`, writes
    /// `ptMinTrackSize.x/y`. The `lParam` -> `&mut MinMaxInfo` conversion (the one `unsafe` of this path, with its `// SAFETY:`) is in `process_message`.
    pub(super) fn apply_min_track_size(&self, info: Option<&mut MinMaxInfo>) {
        todo!("W8-13f")
    }
}
