//! Port of `PhotoReview.Shell.Win32/Input/ShellInputEnvironment.cs` (30 lines, baseline `d92d6aae`). Unit W8-14a. Contract: `contracts/W8-14a.md`.
//!
//! Everything `ShellInput` reads from the system besides the message itself (modifier keys, message time stamp, tick count, wheel device,
//! double-click thresholds). The default is the real Win32; tests (the G-INPUT replay with synthetic `SendMessage`, where `GetKeyState`,
//! `GetMessageTime` and `GetCurrentInputMessageSource` do not describe the fake message) replace single members. DPI-dependent functions take the
//! window's whole DPI.

use std::fmt;

use pr_app::input::primitives::KeyModifiers;
use pr_app::input::touchpad_gestures::WheelDeviceHint;

use super::native;

/// C# `ShellInputEnvironment` (`init`-only `Func<...>` properties). Fields are public so a test builds one with struct-update syntax.
pub struct ShellInputEnvironment {
    /// C# `Modifiers` (`Keyboard.Modifiers`; keys only, the mouse uses the `MK_*` flags of the message).
    pub modifiers: Box<dyn Fn() -> KeyModifiers>,
    /// C# `MessageTime` (`GetMessageTime`).
    pub message_time: Box<dyn Fn() -> i32>,
    /// C# `TickCount` (`Environment.TickCount`; `WM_MOUSEHWHEEL` has no message time in the WPF version).
    pub tick_count: Box<dyn Fn() -> i32>,
    /// C# `WheelDevice` (`WheelMessageSource.Current`): valid only while the message is dispatched.
    pub wheel_device: Box<dyn Fn() -> WheelDeviceHint>,
    /// C# `DoubleClickTimeMs`.
    pub double_click_time_ms: Box<dyn Fn() -> i32>,
    /// C# `DoubleClickSizePixels`: `SM_CXDOUBLECLK x SM_CYDOUBLECLK` for a whole DPI.
    pub double_click_size_pixels: Box<dyn Fn(u32) -> (i32, i32)>,
}

impl Default for ShellInputEnvironment {
    /// The real Win32 reads of [`native`].
    fn default() -> Self {
        Self {
            modifiers: Box::new(native::read_modifiers),
            message_time: Box::new(native::message_time),
            tick_count: Box::new(native::tick_count),
            wheel_device: Box::new(native::current_wheel_device),
            double_click_time_ms: Box::new(native::double_click_time_ms),
            double_click_size_pixels: Box::new(native::double_click_size_pixels),
        }
    }
}

impl fmt::Debug for ShellInputEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellInputEnvironment").finish_non_exhaustive()
    }
}
