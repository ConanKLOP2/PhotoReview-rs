//! Port of `PhotoReview.Shell.Win32/Input/{KeyboardMessageHandler, PointerMessageHandler, WheelMessageHandler}.cs` (60 + 50 + 31 lines, baseline
//! `d92d6aae`). Unit W8-14a. Contract: `contracts/W8-14a.md`.
//!
//! Three `WindowMessageHandler`s that only DECODE messages (virtual key and scan code to `KeyId`, `lParam` words to signed client pixels, `MK_*`
//! flags, screen to client DIP for the wheel) and call a port trait. The C# handlers hold the concrete `ShellInput`; the Rust handlers hold a port
//! trait implemented by `dispatch::ShellInput` (W8-14b), which breaks the 14a <-> 14b cycle and lets the decoding be tested with a recording fake.
//! Handlers are `&self` and re-entrant (D-W8B3H-6): their small state is in `Cell`s.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use pr_app::input::key_id::KeyId;
use pr_app::input::primitives::{PointD, PointerButton};

use pr_app::input::key_mapping;
use pr_app::input::wheel_message;

use super::native::{
    MK_CONTROL, MK_LBUTTON, SCAN_CODE_RIGHT_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_SHIFT, is_extended_key, is_key_repeat,
    signed_high_word, signed_low_word,
};
use crate::host::contracts::{WindowMessage, WindowMessageHandler};
use crate::win32::messages::{
    WM_CAPTURECHANGED, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK,
    WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_SYSCHAR,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN,
};
use crate::win32::structs::{Lparam, Lresult};

/// C# `XButton1`: the high word of `wParam` of `WM_XBUTTONDOWN` for the first extra button.
const XBUTTON1: i32 = 1;

/// What `KeyboardMessageHandler` needs from `ShellInput` (C# `HandleKeyDown` and `Environment.MessageTime()`).
pub trait KeyInputPort {
    /// C# `HandleKeyDown(key, isSystemKey, isRepeat, messageTime)`: true = handled (no `DefWindowProc`).
    fn handle_key_down(&self, key: KeyId, is_system_key: bool, is_repeat: bool, message_time: i32) -> bool;
    /// C# `Environment.MessageTime()`.
    fn message_time(&self) -> i32;
}

/// What `PointerMessageHandler` needs from `ShellInput`. `x`, `y` are signed client pixels.
pub trait PointerInputPort {
    /// C# `HandlePointerDown(button, x, y)`: true = handled.
    fn handle_pointer_down(&self, button: PointerButton, pixel_x: i32, pixel_y: i32) -> bool;
    /// C# `HandlePointerMove(x, y, leftButtonPressed)`.
    fn handle_pointer_move(&self, pixel_x: i32, pixel_y: i32, left_button_pressed: bool);
    /// C# `HandleLeftButtonUp(x, y)`.
    fn handle_left_button_up(&self, pixel_x: i32, pixel_y: i32);
    /// C# `HandleCaptureLost()`.
    fn handle_capture_lost(&self);
}

/// What `WheelMessageHandler` needs from `ShellInput`.
pub trait WheelInputPort {
    /// C# `ScreenToClientDip(screenPixel)`.
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD;
    /// C# `HandleVerticalWheel(delta, ctrl, clientDip)`: true = handled.
    fn handle_vertical_wheel(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool;
    /// C# `HandleHorizontalWheel(delta, ctrl, clientDip)`.
    fn handle_horizontal_wheel(&self, delta: i32, ctrl: bool, client_dip: PointD) -> bool;
}

/// C# `KeyboardMessageHandler`: `WM_KEYDOWN`/`WM_SYSKEYDOWN` to `handle_key_down`; the matching `WM_KEYUP`/`WM_SYSKEYUP` and the `WM_CHAR`/`WM_SYSCHAR`
/// of a handled key are swallowed (no beep, no stray character in `DefWindowProc`); an unhandled key goes on (Alt+F4, Alt+Space).
pub struct KeyboardMessageHandler {
    input: Rc<dyn KeyInputPort>,
    last_key_handled: Cell<bool>,
    handled_virtual_key: Cell<i32>,
}

impl KeyboardMessageHandler {
    /// C# primary constructor; `_lastKeyHandled = false`, `_handledVirtualKey = -1`.
    pub fn new(input: Rc<dyn KeyInputPort>) -> Self {
        Self {
            input,
            last_key_handled: Cell::new(false),
            handled_virtual_key: Cell::new(-1),
        }
    }

    /// C# `ResolveVirtualKey(virtualKey, lParam)`: `VK_SHIFT` is shared by both Shift keys; scan code `0x36` (bits 16..23 of `lparam`) is the right
    /// one (`VK_RSHIFT`), any other scan code is `VK_LSHIFT`. Other keys are returned unchanged.
    pub fn resolve_virtual_key(virtual_key: i32, lparam: Lparam) -> i32 {
        if virtual_key != VK_SHIFT {
            return virtual_key;
        }
        let scan_code = (((lparam as usize) >> 16) & 0xFF) as i32;
        if scan_code == SCAN_CODE_RIGHT_SHIFT {
            VK_RSHIFT
        } else {
            VK_LSHIFT
        }
    }
}

impl fmt::Debug for KeyboardMessageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyboardMessageHandler")
            .field("last_key_handled", &self.last_key_handled.get())
            .field("handled_virtual_key", &self.handled_virtual_key.get())
            .finish_non_exhaustive()
    }
}

impl WindowMessageHandler for KeyboardMessageHandler {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        match message.msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let raw = message.wparam as i32;
                let virtual_key = Self::resolve_virtual_key(raw, message.lparam);
                let key = key_mapping::from_virtual_key(virtual_key, is_extended_key(message.lparam));
                // No cell is borrowed across the port call (T1): the port can re-enter this chain.
                let handled = self.input.handle_key_down(
                    key,
                    message.msg == WM_SYSKEYDOWN,
                    is_key_repeat(message.lparam),
                    self.input.message_time(),
                );
                self.last_key_handled.set(handled);
                // The RAW wparam (`VK_SHIFT`), because the key-up message carries the raw one.
                self.handled_virtual_key.set(if handled { raw } else { -1 });
                handled.then_some(0)
            }
            WM_KEYUP | WM_SYSKEYUP => {
                if self.handled_virtual_key.get() == message.wparam as i32 {
                    self.handled_virtual_key.set(-1);
                    Some(0)
                } else {
                    None
                }
            }
            WM_CHAR | WM_SYSCHAR => self.last_key_handled.get().then_some(0),
            _ => None,
        }
    }
}

/// C# `PointerMessageHandler`: `WM_*BUTTONDOWN`/`DBLCLK`, `WM_MOUSEMOVE`, `WM_LBUTTONUP`, `WM_CAPTURECHANGED`. Left, middle and right buttons return
/// `handle_pointer_down`'s answer; the X buttons, move, left-up and capture-lost always return `None` (the window goes on).
pub struct PointerMessageHandler {
    input: Rc<dyn PointerInputPort>,
}

impl PointerMessageHandler {
    /// C# primary constructor.
    pub fn new(input: Rc<dyn PointerInputPort>) -> Self {
        Self { input }
    }
}

impl fmt::Debug for PointerMessageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointerMessageHandler").finish_non_exhaustive()
    }
}

impl WindowMessageHandler for PointerMessageHandler {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        let x = signed_low_word(message.lparam);
        let y = signed_high_word(message.lparam);
        match message.msg {
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => self.input.handle_pointer_down(PointerButton::Left, x, y).then_some(0),
            WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => {
                self.input.handle_pointer_down(PointerButton::Middle, x, y).then_some(0)
            }
            WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => {
                self.input.handle_pointer_down(PointerButton::Right, x, y).then_some(0)
            }
            WM_XBUTTONDOWN | WM_XBUTTONDBLCLK => {
                let which = signed_high_word(message.wparam as isize);
                let button = if which == XBUTTON1 {
                    PointerButton::XButton1
                } else {
                    PointerButton::XButton2
                };
                // C#: the answer is ignored, the window goes on.
                self.input.handle_pointer_down(button, x, y);
                None
            }
            WM_MOUSEMOVE => {
                self.input
                    .handle_pointer_move(x, y, (message.wparam as i32 & MK_LBUTTON) != 0);
                None
            }
            WM_LBUTTONUP => {
                self.input.handle_left_button_up(x, y);
                None
            }
            WM_CAPTURECHANGED => {
                self.input.handle_capture_lost();
                None
            }
            _ => None,
        }
    }
}

/// C# `WheelMessageHandler`: `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL`. `lparam` is the SCREEN pixel position, the delta is the signed high word of `wparam`
/// (`pr_app::input::wheel_message::{screen_point, delta}`), Ctrl is `MK_CONTROL` in the low word. Handled gives result `0`.
pub struct WheelMessageHandler {
    input: Rc<dyn WheelInputPort>,
}

impl WheelMessageHandler {
    /// C# primary constructor.
    pub fn new(input: Rc<dyn WheelInputPort>) -> Self {
        Self { input }
    }
}

impl fmt::Debug for WheelMessageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WheelMessageHandler").finish_non_exhaustive()
    }
}

impl WindowMessageHandler for WheelMessageHandler {
    fn try_handle(&self, message: &WindowMessage) -> Option<Lresult> {
        if message.msg != WM_MOUSEWHEEL && message.msg != WM_MOUSEHWHEEL {
            return None;
        }
        // Same bits as `InputNative.SignedLowWord/HighWord/WheelDelta` (pure helpers of W8-05a).
        let screen = wheel_message::screen_point(message.lparam as i64);
        let client = self.input.screen_to_client_dip(screen);
        let delta = wheel_message::delta(message.wparam as i64);
        let ctrl = (message.wparam as i32 & MK_CONTROL) != 0;
        let handled = if message.msg == WM_MOUSEWHEEL {
            self.input.handle_vertical_wheel(delta, ctrl, client)
        } else {
            self.input.handle_horizontal_wheel(delta, ctrl, client)
        };
        handled.then_some(0)
    }
}
