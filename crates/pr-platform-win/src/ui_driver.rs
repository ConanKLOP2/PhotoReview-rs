//! Test helper (feature `test-support`): posts input messages to a window that lives on ANOTHER thread (or process) of the same desktop. The Rust twin of what the C# integration tests did with
//! `User32.PostMessage`/`SendMessage` from the test thread (`ShellRun.RequestClose`, `InputHarness.SendMouse`, `ShellProcessTests.CloseWindow`). Units W8-20a and W8-20b.
//! Contract: `contracts/W8-20a.md` (`close_main_window_of_process`, `main_window_icons_of_process`), `contracts/W8-20b.md` (the rest).
//!
//! Only `PostMessageW`: the caller never blocks on the UI thread. No `SetForegroundWindow`, no `SendInput` (C# N-8: tests never steal the user's focus or move the real cursor). Every function
//! returns `false` when the OS refused (stale handle, other desktop) and never panics. All `unsafe` is here, each block with a `// SAFETY:` comment.
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::{IsWindowEnabled, MAPVK_VK_TO_VSC, MapVirtualKeyW};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR};

fn native(hwnd: Hwnd) -> HWND {
    HWND(hwnd as *mut _)
}
fn point(x: i32, y: i32) -> LPARAM {
    LPARAM(((x as u16 as u32) | ((y as u16 as u32) << 16)) as isize)
}
fn post(hwnd: Hwnd, message: u32, w: usize, l: LPARAM) -> bool {
    if hwnd == 0 {
        return false;
    }
    // SAFETY: Posting copies scalar message data; Windows validates the handle.
    unsafe { PostMessageW(Some(native(hwnd)), message, WPARAM(w), l).is_ok() }
}

struct Search {
    pid: u32,
    owner: Hwnd,
    found: Hwnd,
}
unsafe extern "system" fn find_window(hwnd: HWND, param: LPARAM) -> BOOL {
    // SAFETY: EnumWindows/EnumThreadWindows invoke this synchronously with our live Search pointer.
    let search = unsafe { &mut *(param.0 as *mut Search) };
    let mut pid = 0;
    // SAFETY: The enumerated HWND is inspected only; pid points to live stack storage.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
        if IsWindowVisible(hwnd).as_bool() && owner.0 as isize == search.owner && (search.pid == 0 || search.pid == pid)
        {
            search.found = hwnd.0 as isize;
            return BOOL(0);
        }
    }
    BOOL(1)
}
fn main_window(pid: u32) -> Option<Hwnd> {
    let mut search = Search {
        pid,
        owner: 0,
        found: 0,
    };
    // SAFETY: Search outlives the synchronous enumeration and callback.
    let _ = unsafe { EnumWindows(Some(find_window), LPARAM(&mut search as *mut _ as isize)) };
    (search.found != 0).then_some(search.found)
}

use crate::win32::structs::Hwnd;

/// `WM_KEYDOWN` (`down`) or `WM_KEYUP` for `virtual_key`: `lParam` = repeat count 1, the scan code from `MapVirtualKeyW(vk, MAPVK_VK_TO_VSC)` in bits 16..23, extended-key bit 24 for
/// the arrow, Home/End/PageUp/PageDown/Insert/Delete keys, previous-state bit 30 and transition bit 31 set on key-up. Modifier keys are NOT simulated (the shell reads `GetKeyState`).
pub fn post_key(hwnd: Hwnd, virtual_key: u16, down: bool) -> bool {
    // SAFETY: MapVirtualKeyW reads no caller memory.
    let scan = unsafe { MapVirtualKeyW(u32::from(virtual_key), MAPVK_VK_TO_VSC) } & 0xff;
    let extended = matches!(virtual_key, 0x21..=0x28 | 0x2d | 0x2e);
    let bits = 1 | (scan << 16) | (u32::from(extended) << 24) | if down { 0 } else { 0xc0000000 };
    post(
        hwnd,
        if down { WM_KEYDOWN } else { WM_KEYUP },
        usize::from(virtual_key),
        LPARAM(bits as isize),
    )
}

/// `WM_MOUSEWHEEL`: `wParam` = `delta` (120 per notch) in the high word, `lParam` = the SCREEN position of the client point `(client_x, client_y)` (`ClientToScreen`, then `x` low word, `y`
/// high word; the shell converts back), as the OS posts it.
pub fn post_mouse_wheel(hwnd: Hwnd, client_x: i32, client_y: i32, delta: i16) -> bool {
    let mut screen = POINT {
        x: client_x,
        y: client_y,
    };
    // SAFETY: screen is writable and lives through this call; Windows validates hwnd.
    if !unsafe { ClientToScreen(native(hwnd), &mut screen) }.as_bool() {
        return false;
    }
    post(
        hwnd,
        WM_MOUSEWHEEL,
        (delta as u16 as usize) << 16,
        point(screen.x, screen.y),
    )
}

/// `WM_MOUSEMOVE` at the client point (pixels), no button flags.
pub fn post_mouse_move(hwnd: Hwnd, client_x: i32, client_y: i32) -> bool {
    post_mouse_move_with_buttons(hwnd, client_x, client_y, false)
}

/// `WM_LBUTTONDOWN` (`down`, `wParam` = `MK_LBUTTON`) or `WM_LBUTTONUP` (`wParam` 0) at the client point (pixels).
pub fn post_left_button(hwnd: Hwnd, client_x: i32, client_y: i32, down: bool) -> bool {
    post(
        hwnd,
        if down { WM_LBUTTONDOWN } else { WM_LBUTTONUP },
        usize::from(down),
        point(client_x, client_y),
    )
}

/// `WM_CLOSE`: what the title-bar close button posts.
pub fn post_close(hwnd: Hwnd) -> bool {
    post(hwnd, WM_CLOSE, 0, LPARAM(0))
}

/// .NET `Process.CloseMainWindow()`: enumerates the top-level windows (`EnumWindows`), takes the first VISIBLE one with no owner whose `GetWindowThreadProcessId` process id is `pid`, and posts
/// `WM_CLOSE` to it. `false` when the process has no such window yet (the caller retries on its own bound, no sleep inside).
pub fn close_main_window_of_process(pid: u32) -> bool {
    main_window(pid).is_some_and(post_close)
}

/// Unit W8-20a (`exe_process`): finds the main window of process `pid` exactly as [`close_main_window_of_process`] does and asks it for its icons with `SendMessageTimeoutW(WM_GETICON,
/// ICON_BIG / ICON_SMALL, SMTO_ABORTIFHUNG, 5000 ms)` (called from the TEST thread, which is not the window's thread, so the message is delivered and answered normally). Returns
/// `Some((big_is_set, small_is_set))` (a non-zero icon handle each); `None` when the process has no such window yet or the call timed out (the caller retries on its own bound).
pub fn main_window_icons_of_process(pid: u32) -> Option<(bool, bool)> {
    let hwnd = native(main_window(pid)?);
    let mut icons = [0usize; 2];
    for (slot, kind) in icons.iter_mut().zip([ICON_BIG, ICON_SMALL]) {
        // SAFETY: WM_GETICON returns a handle, with no borrowed message payload. slot lives through the bounded call.
        if unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_GETICON,
                WPARAM(kind as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                5000,
                Some(slot),
            )
        }
        .0 == 0
        {
            return None;
        }
    }
    Some((icons[0] != 0, icons[1] != 0))
}

/// `WM_MOUSEMOVE` at the client point with the left button reported as held (`wParam = MK_LBUTTON` when `left_down`): a thumb drag between a `post_left_button(down)` and a `post_left_button(up)`.
/// Slice S3 of W8-20b.
pub fn post_mouse_move_with_buttons(hwnd: Hwnd, client_x: i32, client_y: i32, left_down: bool) -> bool {
    post(hwnd, WM_MOUSEMOVE, usize::from(left_down), point(client_x, client_y))
}

/// `WM_COMMAND` with `wParam = command_id` (`IDOK` 1, `IDCANCEL` 2, a control id) and `lParam = 0` posted to a dialog. Slice S5 of W8-20b.
pub fn post_command(dialog: Hwnd, command_id: u16) -> bool {
    post(dialog, WM_COMMAND, usize::from(command_id), LPARAM(0))
}

/// The first VISIBLE window of the thread that owns `owner` whose `GetWindow(GW_OWNER)` is `owner` (`EnumThreadWindows` of the thread from `GetWindowThreadProcessId(owner)`); `None` when there is none.
/// Slice S5 of W8-20b.
pub fn first_window_owned_by(owner: Hwnd) -> Option<Hwnd> {
    // SAFETY: Only queries the owner thread; no output pointer is needed.
    let thread = unsafe { GetWindowThreadProcessId(native(owner), None) };
    if thread == 0 {
        return None;
    }
    let mut search = Search {
        pid: 0,
        owner,
        found: 0,
    };
    // SAFETY: Search outlives the synchronous enumeration and callback.
    let _ = unsafe { EnumThreadWindows(thread, Some(find_window), LPARAM(&mut search as *mut _ as isize)) };
    (search.found != 0).then_some(search.found)
}

/// `IsWindowEnabled` (a modal dialog disables its owner). Slice S5 of W8-20b.
pub fn is_window_enabled(hwnd: Hwnd) -> bool {
    // SAFETY: Query only; Windows validates the handle.
    unsafe { IsWindowEnabled(native(hwnd)).as_bool() }
}

/// `SetDlgItemTextW(dialog, item_id, text)`; `false` when the OS refused. Slice S5 of W8-20b.
pub fn set_dialog_item_text(dialog: Hwnd, item_id: i32, text: &str) -> bool {
    if text.contains('\0') {
        return false;
    }
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: wide is NUL-terminated and lives through the synchronous copy into the dialog control.
    unsafe { SetDlgItemTextW(native(dialog), item_id, PCWSTR(wide.as_ptr())).is_ok() }
}
