//! Port of `PhotoReview.Shell.Win32/Overlay/OverlayNative.cs` (134 lines, baseline `d92d6aae`). Unit W8-15f.
//!
//! The user32 helpers the overlay needs: the "mouse left the window" request (`TrackMouseEvent` with `TME_LEAVE`), the key state of Shift, and the
//! screen position of the client origin (`ClientToScreen`). The P/Invoke declarations of the C# file are replaced by the `windows` crate and the
//! `win32` wrappers; the tooltip constants and structures (`TTM_*`, `TOOLINFOW`, `MSG`) move to `tooltips` (15g, their only user).
//! Everything is behind [`OverlayOs`] so [`super::messages::OverlayMessageHandler`] is testable without a window (precedent: `host::message_loop::LoopOs`).

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::win32::structs::Hwnd;

pub use crate::win32::control_constants::WM_MOUSELEAVE;

/// C# `OverlayNative.TmeLeave`.
pub const TME_LEAVE: u32 = 0x0000_0002;
/// C# `OverlayNative.VkShift`.
pub const VK_SHIFT: i32 = 0x10;
/// C# `OverlayNative.VkTab`.
pub const VK_TAB: i32 = 0x09;
/// C# `OverlayNative.VkReturn`.
pub const VK_RETURN: i32 = 0x0D;
/// C# `OverlayNative.VkSpace`.
pub const VK_SPACE: i32 = 0x20;

/// The three OS calls of the overlay message path. UI thread, `&self` (the handler is re-entered by nested `SendMessage`).
pub trait OverlayOs {
    /// C# `RequestMouseLeave(hwnd)`: asks for one `WM_MOUSELEAVE` for `hwnd` (`TrackMouseEvent`, `TME_LEAVE`). A failure is ignored (C# ignored it).
    fn request_mouse_leave(&self, hwnd: Hwnd);

    /// C# `IsKeyDown(virtualKey)`: bit 15 of `GetKeyState`.
    fn is_key_down(&self, virtual_key: i32) -> bool;

    /// C# `ClientOriginOnScreen(hwnd)`: client origin in physical screen pixels; `(0, 0)` when `ClientToScreen` fails.
    fn client_origin_on_screen(&self, hwnd: Hwnd) -> (i32, i32);
}

/// The real [`OverlayOs`] (user32).
#[derive(Debug, Clone, Copy, Default)]
pub struct Win32OverlayOs;

impl OverlayOs for Win32OverlayOs {
    fn request_mouse_leave(&self, hwnd: Hwnd) {
        todo!()
    }

    fn is_key_down(&self, virtual_key: i32) -> bool {
        todo!()
    }

    fn client_origin_on_screen(&self, hwnd: Hwnd) -> (i32, i32) {
        todo!()
    }
}

/// C# `(GetKeyState(virtualKey) & 0x8000) != 0` for a raw `GetKeyState` result (pure, tested with a table: `i16::MIN` and `-1` are down, `1` (toggled
/// only) and `0` are not).
pub fn key_state_is_down(state: i16) -> bool {
    todo!()
}
