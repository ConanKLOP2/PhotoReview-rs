//! Test helper (feature `test-support`): posts input messages to a window that lives on ANOTHER thread (or process) of the same desktop. The Rust twin of what the C# integration tests did with
//! `User32.PostMessage`/`SendMessage` from the test thread (`ShellRun.RequestClose`, `InputHarness.SendMouse`, `ShellProcessTests.CloseWindow`). Units W8-20a and W8-20b.
//! Contract: `contracts/W8-20a.md` (`close_main_window_of_process`, `main_window_icons_of_process`), `contracts/W8-20b.md` (the rest).
//!
//! Only `PostMessageW`: the caller never blocks on the UI thread. No `SetForegroundWindow`, no `SendInput` (C# N-8: tests never steal the user's focus or move the real cursor). Every function
//! returns `false` when the OS refused (stale handle, other desktop) and never panics. All `unsafe` is here, each block with a `// SAFETY:` comment.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use crate::win32::structs::Hwnd;

/// `WM_KEYDOWN` (`down`) or `WM_KEYUP` for `virtual_key`: `lParam` = repeat count 1, the scan code from `MapVirtualKeyW(vk, MAPVK_VK_TO_VSC)` in bits 16..23, extended-key bit 24 for
/// the arrow, Home/End/PageUp/PageDown/Insert/Delete keys, previous-state bit 30 and transition bit 31 set on key-up. Modifier keys are NOT simulated (the shell reads `GetKeyState`).
pub fn post_key(hwnd: Hwnd, virtual_key: u16, down: bool) -> bool {
    todo!()
}

/// `WM_MOUSEWHEEL`: `wParam` = `delta` (120 per notch) in the high word, `lParam` = the SCREEN position of the client point `(client_x, client_y)` (`ClientToScreen`, then `x` low word, `y`
/// high word; the shell converts back), as the OS posts it.
pub fn post_mouse_wheel(hwnd: Hwnd, client_x: i32, client_y: i32, delta: i16) -> bool {
    todo!()
}

/// `WM_MOUSEMOVE` at the client point (pixels), no button flags.
pub fn post_mouse_move(hwnd: Hwnd, client_x: i32, client_y: i32) -> bool {
    todo!()
}

/// `WM_LBUTTONDOWN` (`down`, `wParam` = `MK_LBUTTON`) or `WM_LBUTTONUP` (`wParam` 0) at the client point (pixels).
pub fn post_left_button(hwnd: Hwnd, client_x: i32, client_y: i32, down: bool) -> bool {
    todo!()
}

/// `WM_CLOSE`: what the title-bar close button posts.
pub fn post_close(hwnd: Hwnd) -> bool {
    todo!()
}

/// .NET `Process.CloseMainWindow()`: enumerates the top-level windows (`EnumWindows`), takes the first VISIBLE one with no owner whose `GetWindowThreadProcessId` process id is `pid`, and posts
/// `WM_CLOSE` to it. `false` when the process has no such window yet (the caller retries on its own bound, no sleep inside).
pub fn close_main_window_of_process(pid: u32) -> bool {
    todo!()
}

/// Unit W8-20a (`exe_process`): finds the main window of process `pid` exactly as [`close_main_window_of_process`] does and asks it for its icons with `SendMessageTimeoutW(WM_GETICON,
/// ICON_BIG / ICON_SMALL, SMTO_ABORTIFHUNG, 5000 ms)` (called from the TEST thread, which is not the window's thread, so the message is delivered and answered normally). Returns
/// `Some((big_is_set, small_is_set))` (a non-zero icon handle each); `None` when the process has no such window yet or the call timed out (the caller retries on its own bound).
pub fn main_window_icons_of_process(pid: u32) -> Option<(bool, bool)> {
    todo!()
}

/// `WM_MOUSEMOVE` at the client point with the left button reported as held (`wParam = MK_LBUTTON` when `left_down`): a thumb drag between a `post_left_button(down)` and a `post_left_button(up)`.
/// Slice S3 of W8-20b.
pub fn post_mouse_move_with_buttons(hwnd: Hwnd, client_x: i32, client_y: i32, left_down: bool) -> bool {
    todo!()
}

/// `WM_COMMAND` with `wParam = command_id` (`IDOK` 1, `IDCANCEL` 2, a control id) and `lParam = 0` posted to a dialog. Slice S5 of W8-20b.
pub fn post_command(dialog: Hwnd, command_id: u16) -> bool {
    todo!()
}

/// The first VISIBLE window of the thread that owns `owner` whose `GetWindow(GW_OWNER)` is `owner` (`EnumThreadWindows` of the thread from `GetWindowThreadProcessId(owner)`); `None` when there is none.
/// Slice S5 of W8-20b.
pub fn first_window_owned_by(owner: Hwnd) -> Option<Hwnd> {
    todo!()
}

/// `IsWindowEnabled` (a modal dialog disables its owner). Slice S5 of W8-20b.
pub fn is_window_enabled(hwnd: Hwnd) -> bool {
    todo!()
}

/// `SetDlgItemTextW(dialog, item_id, text)`; `false` when the OS refused. Slice S5 of W8-20b.
pub fn set_dialog_item_text(dialog: Hwnd, item_id: i32, text: &str) -> bool {
    todo!()
}
