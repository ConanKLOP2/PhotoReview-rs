//! Port of `PhotoReview.Shell.Win32/Input/InputNative.cs` (99 lines, baseline `d92d6aae`). Unit W8-14a. Contract: `contracts/W8-14a.md`.
//!
//! The user32 reads that W8-11b did not declare (`GetKeyState` is `win32::settings_native::get_key_state`, `GetCursorPos` is
//! `win32::comctl::user32::get_cursor_pos`, `GetSystemMetricsForDpi` is `win32::api::user32::get_system_metrics_for_dpi`) and the pure bit helpers.
//! `InputNative.WheelDelta` is NOT ported: it duplicates `pr_app::input::wheel_message::delta`.
use pr_app::input::primitives::{KeyModifiers, PointD};
use pr_app::input::touchpad_gestures::WheelDeviceHint;

use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::GetMessageTime;

use crate::win32::api::user32;
use crate::win32::comctl::user32 as comctl_user32;
use crate::win32::messages::{SM_CXDOUBLECLK, SM_CXDRAG, SM_CYDOUBLECLK, SM_CYDRAG};
use crate::win32::settings_native::get_key_state;
use crate::win32::structs::Lparam;

/// `IMDT_TOUCHPAD`: the `INPUT_MESSAGE_DEVICE_TYPE` of a precision touchpad.
const IMDT_TOUCHPAD: i32 = 0x10;

/// C# `VkShift`.
pub const VK_SHIFT: i32 = 0x10;
/// C# `VkControl`.
pub const VK_CONTROL: i32 = 0x11;
/// C# `VkMenu` (Alt).
pub const VK_MENU: i32 = 0x12;
/// C# `VkLShift`.
pub const VK_LSHIFT: i32 = 0xA0;
/// C# `VkRShift`.
pub const VK_RSHIFT: i32 = 0xA1;
/// C# `VkLWin`.
pub const VK_LWIN: i32 = 0x5B;
/// C# `VkRWin`.
pub const VK_RWIN: i32 = 0x5C;
/// C# `MkLButton`: key flag in `LOWORD(wParam)` of the mouse messages.
pub const MK_LBUTTON: i32 = 0x0001;
/// C# `MkControl`.
pub const MK_CONTROL: i32 = 0x0008;
/// C# `KeyRepeatBit`: bit 30 of the `lParam` of `WM_KEYDOWN` (the key was already down: auto-repeat).
pub const KEY_REPEAT_BIT: i64 = 0x4000_0000;
/// C# `KeyExtendedBit`: bit 24 of `lParam` (extended key).
pub const KEY_EXTENDED_BIT: i64 = 0x0100_0000;
/// C# `ScanCodeRightShift`: `MapVirtualKey(0x36, MAPVK_VSC_TO_VK_EX)` is `VK_RSHIFT`.
pub const SCAN_CODE_RIGHT_SHIFT: i32 = 0x36;

/// C# `ReadModifiers()` (`Keyboard.Modifiers`): `GetKeyState` of Ctrl, Alt, Shift and both Windows keys at the time of the call; a key is down when
/// its state is negative.
pub fn read_modifiers() -> KeyModifiers {
    let ctrl = get_key_state(VK_CONTROL);
    let alt = get_key_state(VK_MENU);
    let shift = get_key_state(VK_SHIFT);
    let left_win = get_key_state(VK_LWIN);
    // C# `GetKeyState(VkLWin) < 0 || GetKeyState(VkRWin) < 0`: the right key is read only when the left one is not down.
    let right_win = if left_win < 0 { 0 } else { get_key_state(VK_RWIN) };
    modifiers_from_key_states(ctrl, alt, shift, left_win, right_win)
}

/// The pure half of `read_modifiers`: the five `GetKeyState` results (`< 0` = down) to flags. Control, Alt (`VK_MENU`), Shift, Windows (either key).
pub fn modifiers_from_key_states(ctrl: i16, alt: i16, shift: i16, left_win: i16, right_win: i16) -> KeyModifiers {
    let mut modifiers = KeyModifiers::NONE;
    if ctrl < 0 {
        modifiers = modifiers | KeyModifiers::CONTROL;
    }
    if alt < 0 {
        modifiers = modifiers | KeyModifiers::ALT;
    }
    if shift < 0 {
        modifiers = modifiers | KeyModifiers::SHIFT;
    }
    if left_win < 0 || right_win < 0 {
        modifiers = modifiers | KeyModifiers::WINDOWS;
    }
    modifiers
}

/// C# `GetMessageTime()`: the time stamp (ms since boot, wrapping `i32`) of the message being dispatched.
pub fn message_time() -> i32 {
    // SAFETY: no arguments, reads process-global (thread message queue) state.
    unsafe { GetMessageTime() }
}

/// C# `GetDoubleClickTime()` as `i32` (ms).
pub fn double_click_time_ms() -> i32 {
    // SAFETY: no arguments, reads process-global state.
    let ms = unsafe { GetDoubleClickTime() };
    i32::try_from(ms).unwrap_or(i32::MAX)
}

/// C# `Environment.TickCount`: `GetTickCount64() as i32` (truncating), the same clock domain as [`message_time`].
pub fn tick_count() -> i32 {
    // SAFETY: no arguments, reads process-global state.
    let ms = unsafe { GetTickCount64() };
    // Same clock and the same 32-bit wrap as `GetMessageTime` (D-W8B4I-15).
    ms as u32 as i32
}

/// C# `CursorScreenPixel()`: `GetCursorPos` as a `PointD` (screen pixels), `None` when the call fails.
pub fn cursor_screen_pixel() -> Option<PointD> {
    comctl_user32::get_cursor_pos().map(|point| PointD {
        x: f64::from(point.x),
        y: f64::from(point.y),
    })
}

/// C# `DragMetricsPixels(dpi)`: `(SM_CXDRAG, SM_CYDRAG)` in pixels for `dpi` (`GetSystemMetricsForDpi`).
pub fn drag_metrics_pixels(dpi: u32) -> (i32, i32) {
    (
        user32::get_system_metrics_for_dpi(SM_CXDRAG, dpi),
        user32::get_system_metrics_for_dpi(SM_CYDRAG, dpi),
    )
}

/// C# `DoubleClickSizePixels(dpi)`: `(SM_CXDOUBLECLK, SM_CYDOUBLECLK)` in pixels for `dpi`.
pub fn double_click_size_pixels(dpi: u32) -> (i32, i32) {
    (
        user32::get_system_metrics_for_dpi(SM_CXDOUBLECLK, dpi),
        user32::get_system_metrics_for_dpi(SM_CYDOUBLECLK, dpi),
    )
}

/// C# `SignedLowWord(value)`: `LOWORD` as a signed 16-bit value (mouse coordinates are negative while the image holds the capture).
pub fn signed_low_word(value: isize) -> i32 {
    // Mask first (T2): the whole value is never narrowed before the mask.
    i32::from(((value as usize) & 0xFFFF) as u16 as i16)
}

/// C# `SignedHighWord(value)`.
pub fn signed_high_word(value: isize) -> i32 {
    i32::from((((value as usize) >> 16) & 0xFFFF) as u16 as i16)
}

/// C# `LowWord(value)`: `LOWORD` unsigned.
pub fn low_word(value: isize) -> i32 {
    ((value as usize) & 0xFFFF) as i32
}

/// C# `DpiOf(dpiScale)`: `max(1, round_ties_even(scale * 96))` as `u32`.
pub fn dpi_of(dpi_scale: f64) -> u32 {
    // `as` maps NaN to 0 (C# x64 gives `int.MinValue`); both end at 1.
    ((dpi_scale * 96.0).round_ties_even() as i32).max(1) as u32
}

/// C# `WheelMessageSource.Current()`: `GetCurrentInputMessageSource`; `Touchpad` when the device type is `IMDT_TOUCHPAD` (0x10), else `Unknown`.
/// Valid only while the wheel message is being dispatched.
pub fn current_wheel_device() -> WheelDeviceHint {
    match user32::get_current_input_message_source() {
        Some(source) if source.deviceType.0 == IMDT_TOUCHPAD => WheelDeviceHint::Touchpad,
        _ => WheelDeviceHint::Unknown,
    }
}

/// The pure half of the key decode: bit 24 of `lparam` (extended key).
pub fn is_extended_key(lparam: Lparam) -> bool {
    (lparam as i64 & KEY_EXTENDED_BIT) != 0
}

/// Bit 30 of `lparam` (auto-repeat).
pub fn is_key_repeat(lparam: Lparam) -> bool {
    (lparam as i64 & KEY_REPEAT_BIT) != 0
}
