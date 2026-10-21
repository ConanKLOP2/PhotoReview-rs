//! Port of `PhotoReview.Shell.Interop/Win32/KeyCaptureNative.cs` (baseline `d92d6aae`, 20 lines): the two `imm32.dll` calls the key capture box of the Shortcuts
//! page needs, to switch the IME off for that box and to read the real virtual key when an IME reports `VK_PROCESSKEY` (0xE5).
//!
//! Unit W8-11e. `windows::Win32::UI::Input::Ime::{ImmAssociateContext, ImmGetVirtualKey}`.
#![allow(unused_variables, dead_code)] // stub: removed by the W8-11e porter when the last `todo!()` is gone.

use super::structs::Hwnd;

pub const VK_PROCESSKEY: i32 = 229; // VkProcessKey

/// C# `ImmAssociateContext(hwnd, inputContext)`: the previous `HIMC` (`0` for none or on failure). `input_context == 0` detaches the IME from the window, so the key
/// capture box receives no composed text; the caller keeps the returned handle to restore it. The OS rejects a handle that belongs to another thread.
pub fn imm_associate_context(hwnd: Hwnd, input_context: isize) -> isize {
    todo!()
}

/// C# `ImmGetVirtualKey(hwnd)`: the real virtual key of the key an IME swallowed (`WM_KEYDOWN` reported `VK_PROCESSKEY`); `0` when there is none (the C# caller then keeps `VK_PROCESSKEY`).
pub fn imm_get_virtual_key(hwnd: Hwnd) -> u32 {
    todo!()
}
