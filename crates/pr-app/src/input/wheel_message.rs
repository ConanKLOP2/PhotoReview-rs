//! Port of the pure decoding part of `PhotoReview.App.Shared/Input/WheelMessageSource.cs` (Q-TOUCHPAD-REFRESH).
//!
//! The Win32 call `GetCurrentInputMessageSource` (`Current()` in C#) is NOT here: it needs `unsafe` and the `windows` crate.
//! It lives in `pr_platform_win` (W8-14a) and returns a plain `bool` (touchpad or not). The mapping to `WheelDeviceHint`
//! happens there, so this pure module does not depend on `input::touchpad_gestures` (W8-05e, which owns `WheelDeviceHint`).
//! Baseline `d92d6aae`.

use crate::input::primitives::PointD;

/// `WM_MOUSEHWHEEL` (`0x020E`): the horizontal wheel message (sideways touchpad swipe, tilted wheel).
pub const WM_MOUSE_HWHEEL: u32 = 0x020E;

/// C# `WheelMessageSource.Delta(IntPtr wParam)`: the signed wheel delta in the high word of `wParam`
/// (`unchecked((short)((wParam >> 16) & 0xFFFF))`). The input is the `IntPtr` value as `i64` (`ToInt64`).
pub fn delta(w_param: i64) -> i32 {
    // Mask after shifting: an arithmetic shift of a sign-extended value keeps high bits, which the mask removes.
    i32::from(((w_param >> 16) & 0xFFFF) as u16 as i16)
}

/// C# `WheelMessageSource.ScreenPoint(IntPtr lParam)`: the cursor position packed in `lParam`. X is the low word and Y the
/// high word, each read as a signed 16-bit value (`unchecked((short)...)`), so monitors left of or above the primary have
/// negative coordinates. The result is in physical screen pixels, stored as `PointD` for the shared type only.
pub fn screen_point(l_param: i64) -> PointD {
    let x = (l_param & 0xFFFF) as u16 as i16;
    let y = ((l_param >> 16) & 0xFFFF) as u16 as i16;
    PointD {
        x: f64::from(x),
        y: f64::from(y),
    }
}
