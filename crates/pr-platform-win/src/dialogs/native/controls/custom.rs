//! Port of `PhotoReview.Shell.Win32/Dialogs/Native/NativeControls.cs` lines 500-552 (baseline `d92d6aae`): `ProgressControl`, `TrackbarControl` and
//! `ButtonStateControl`. Unit W8-18g. Contract: `contracts/W8-18g.md`. (The plan names this file `custom`.)
//!
//! Thin message wrappers for `msctls_progress32`, `msctls_trackbar32` and the check box / radio button state. The integer-only messages go through
//! `basic::send_int` (W8-18f); no pointer crosses the boundary here.
use crate::win32::control_constants::{PBS_SMOOTH, TBS_AUTOTICKS};
use crate::win32::dialog_native::WS_TABSTOP;
use crate::win32::structs::Hwnd;

/// C# `ProgressControl` (`msctls_progress32`, `PBS_SMOOTH`).
#[derive(Debug, Clone, Copy)]
pub struct ProgressControl;

impl ProgressControl {
    /// C# `DefaultStyle` = `PBS_SMOOTH`.
    pub const DEFAULT_STYLE: u32 = PBS_SMOOTH;

    /// C# `Scale`: the range is `0..=1000`; `set_fraction` takes `0.0..=1.0`.
    pub const SCALE: i32 = 1000;

    /// C# `Configure(progress)`: `PBM_SETRANGE32(0, SCALE)`. No colour is set (the dark theme draws the bar; `PBM_SETBARCOLOR` draws a glaring 3-D frame on a
    /// dark background, found in the GD2 screenshot).
    pub fn configure(progress: Hwnd) {
        let _ = super::basic::send_int(
            progress,
            crate::win32::control_constants::PBM_SETRANGE32,
            0,
            Self::SCALE as isize,
        );
    }

    /// C# `SetFraction(progress, fraction)`: `PBM_SETPOS(position_for_fraction(fraction))`.
    pub fn set_fraction(progress: Hwnd, fraction: f64) {
        let _ = super::basic::send_int(
            progress,
            crate::win32::control_constants::PBM_SETPOS,
            Self::position_for_fraction(fraction) as usize,
            0,
        );
    }

    /// The pure part of `set_fraction`: NaN is `0`; otherwise clamp to `0.0..=1.0`, multiply by `SCALE` and round half to even (`Math.Round`).
    pub fn position_for_fraction(fraction: f64) -> i32 {
        if fraction.is_nan() {
            0
        } else {
            (fraction.clamp(0.0, 1.0) * Self::SCALE as f64).round_ties_even() as i32
        }
    }

    /// C# `Fraction(progress)`: `PBM_GETPOS as f64 / SCALE as f64`.
    pub fn fraction(progress: Hwnd) -> f64 {
        super::basic::send_int(progress, crate::win32::control_constants::PBM_GETPOS, 0, 0) as f64 / Self::SCALE as f64
    }
}

/// C# `TrackbarControl` (`msctls_trackbar32`, a horizontal slider).
#[derive(Debug, Clone, Copy)]
pub struct TrackbarControl;

impl TrackbarControl {
    /// C# `DefaultStyle` = `TBS_AUTOTICKS | WS_TABSTOP`.
    pub const DEFAULT_STYLE: u32 = TBS_AUTOTICKS | WS_TABSTOP;

    /// C# `Configure(trackbar, minimum, maximum, tickFrequency, lineSize, pageSize)`: `TBM_SETRANGE(1, range_lparam(minimum, maximum))`,
    /// `TBM_SETTICFREQ(tickFrequency)`, `TBM_SETLINESIZE(0, lineSize)`, `TBM_SETPAGESIZE(0, pageSize)`, in this order.
    pub fn configure(trackbar: Hwnd, minimum: i32, maximum: i32, tick_frequency: i32, line_size: i32, page_size: i32) {
        let _ = super::basic::send_int(
            trackbar,
            crate::win32::control_constants::TBM_SETRANGE,
            1,
            Self::range_lparam(minimum, maximum),
        );
        let _ = super::basic::send_int(
            trackbar,
            crate::win32::control_constants::TBM_SETTICFREQ,
            tick_frequency as usize,
            0,
        );
        let _ = super::basic::send_int(
            trackbar,
            crate::win32::control_constants::TBM_SETLINESIZE,
            0,
            line_size as isize,
        );
        let _ = super::basic::send_int(
            trackbar,
            crate::win32::control_constants::TBM_SETPAGESIZE,
            0,
            page_size as isize,
        );
    }

    /// The `lParam` of `TBM_SETRANGE` (pure): `((maximum as u32) << 16) | (minimum as u16 as u32)`, zero-extended (the C# `(nint)(uint)`).
    pub fn range_lparam(minimum: i32, maximum: i32) -> isize {
        (((maximum as u32) << 16) | (minimum as u16 as u32)) as isize
    }

    /// C# `Position(trackbar)`: `TBM_GETPOS`.
    pub fn position(trackbar: Hwnd) -> i32 {
        super::basic::send_int(trackbar, crate::win32::control_constants::TBM_GETPOS, 0, 0) as i32
    }

    /// C# `SetPosition(trackbar, position)`: `TBM_SETPOS(1, position)` (redraw).
    pub fn set_position(trackbar: Hwnd, position: i32) {
        let _ = super::basic::send_int(
            trackbar,
            crate::win32::control_constants::TBM_SETPOS,
            1,
            position as isize,
        );
    }
}

/// C# `ButtonStateControl` (`BS_AUTOCHECKBOX` / `BS_AUTORADIOBUTTON`).
#[derive(Debug, Clone, Copy)]
pub struct ButtonStateControl;

impl ButtonStateControl {
    /// C# `IsChecked(button)`: `BM_GETCHECK == BST_CHECKED` (an indeterminate state is not checked).
    pub fn is_checked(button: Hwnd) -> bool {
        super::basic::send_int(button, crate::win32::control_constants::BM_GETCHECK, 0, 0)
            == crate::win32::control_constants::BST_CHECKED as isize
    }

    /// C# `SetChecked(button, value)`: `BM_SETCHECK(BST_CHECKED or BST_UNCHECKED)`.
    pub fn set_checked(button: Hwnd, value: bool) {
        let state = if value {
            crate::win32::control_constants::BST_CHECKED
        } else {
            crate::win32::control_constants::BST_UNCHECKED
        };
        let _ = super::basic::send_int(button, crate::win32::control_constants::BM_SETCHECK, state as usize, 0);
    }
}
