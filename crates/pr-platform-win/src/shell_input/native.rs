//! Port of `PhotoReview.Shell.Win32/Input/InputNative.cs` (99 lines, baseline `d92d6aae`). Unit W8-14a. Contract: `contracts/W8-14a.md`.
//!
//! The user32 reads that W8-11b did not declare (`GetKeyState` is `win32::settings_native::get_key_state`, `GetCursorPos` is
//! `win32::comctl::user32::get_cursor_pos`, `GetSystemMetricsForDpi` is `win32::api::user32::get_system_metrics_for_dpi`) and the pure bit helpers.
//! `InputNative.WheelDelta` is NOT ported: it duplicates `pr_app::input::wheel_message::delta`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::input::primitives::{KeyModifiers, PointD};
use pr_app::input::touchpad_gestures::WheelDeviceHint;

use crate::win32::structs::Lparam;

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
    todo!()
}

/// The pure half of `read_modifiers`: the five `GetKeyState` results (`< 0` = down) to flags. Control, Alt (`VK_MENU`), Shift, Windows (either key).
pub fn modifiers_from_key_states(ctrl: i16, alt: i16, shift: i16, left_win: i16, right_win: i16) -> KeyModifiers {
    todo!()
}

/// C# `GetMessageTime()`: the time stamp (ms since boot, wrapping `i32`) of the message being dispatched.
pub fn message_time() -> i32 {
    todo!()
}

/// C# `GetDoubleClickTime()` as `i32` (ms).
pub fn double_click_time_ms() -> i32 {
    todo!()
}

/// C# `Environment.TickCount`: `GetTickCount64() as i32` (truncating), the same clock domain as [`message_time`].
pub fn tick_count() -> i32 {
    todo!()
}

/// C# `CursorScreenPixel()`: `GetCursorPos` as a `PointD` (screen pixels), `None` when the call fails.
pub fn cursor_screen_pixel() -> Option<PointD> {
    todo!()
}

/// C# `DragMetricsPixels(dpi)`: `(SM_CXDRAG, SM_CYDRAG)` in pixels for `dpi` (`GetSystemMetricsForDpi`).
pub fn drag_metrics_pixels(dpi: u32) -> (i32, i32) {
    todo!()
}

/// C# `DoubleClickSizePixels(dpi)`: `(SM_CXDOUBLECLK, SM_CYDOUBLECLK)` in pixels for `dpi`.
pub fn double_click_size_pixels(dpi: u32) -> (i32, i32) {
    todo!()
}

/// C# `SignedLowWord(value)`: `LOWORD` as a signed 16-bit value (mouse coordinates are negative while the image holds the capture).
pub fn signed_low_word(value: isize) -> i32 {
    todo!()
}

/// C# `SignedHighWord(value)`.
pub fn signed_high_word(value: isize) -> i32 {
    todo!()
}

/// C# `LowWord(value)`: `LOWORD` unsigned.
pub fn low_word(value: isize) -> i32 {
    todo!()
}

/// C# `DpiOf(dpiScale)`: `max(1, round_ties_even(scale * 96))` as `u32`.
pub fn dpi_of(dpi_scale: f64) -> u32 {
    todo!()
}

/// C# `WheelMessageSource.Current()`: `GetCurrentInputMessageSource`; `Touchpad` when the device type is `IMDT_TOUCHPAD` (0x10), else `Unknown`.
/// Valid only while the wheel message is being dispatched.
pub fn current_wheel_device() -> WheelDeviceHint {
    todo!()
}

/// The pure half of the key decode: bit 24 of `lparam` (extended key).
pub fn is_extended_key(lparam: Lparam) -> bool {
    todo!()
}

/// Bit 30 of `lparam` (auto-repeat).
pub fn is_key_repeat(lparam: Lparam) -> bool {
    todo!()
}
