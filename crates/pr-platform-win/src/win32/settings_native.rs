//! Port of `PhotoReview.Shell.Interop/Win32/SettingsDialogNative.cs` (baseline `d92d6aae`, 61 lines): the user32 calls of the scrolling pages of the Settings window
//! (`SetParent` to move the template controls into the scrolling viewport, `SetScrollInfo`/`GetScrollInfo` for the page's own vertical scroll bar, `GetKeyState` for
//! Ctrl+Tab), the `SCROLLINFO` struct (`structs::ScrollInfo`) and `ScrollConstants`.
//!
//! Unit W8-11e.

use core::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::SetScrollInfo;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::WindowsAndMessaging::{GetScrollInfo, SCROLLBAR_CONSTANTS, SetParent};

use super::structs::{Hwnd, ScrollInfo};

pub const SB_VERT: i32 = 1; // SbVert
pub const SB_CTL: i32 = 2; // SbCtl
pub const SIF_RANGE: u32 = 1; // SifRange
pub const SIF_PAGE: u32 = 2; // SifPage
pub const SIF_POS: u32 = 4; // SifPos
pub const SIF_DISABLENOSCROLL: u32 = 8; // SifDisableNoScroll
pub const SIF_TRACKPOS: u32 = 0x0010; // SifTrackPos
pub const SB_LINEUP: i32 = 0; // SbLineUp
pub const SB_LINEDOWN: i32 = 1; // SbLineDown
pub const SB_PAGEUP: i32 = 2; // SbPageUp
pub const SB_PAGEDOWN: i32 = 3; // SbPageDown
pub const SB_THUMBPOSITION: i32 = 4; // SbThumbPosition
pub const SB_THUMBTRACK: i32 = 5; // SbThumbTrack
pub const SB_TOP: i32 = 6; // SbTop
pub const SB_BOTTOM: i32 = 7; // SbBottom
pub const SB_ENDSCROLL: i32 = 8; // SbEndScroll
pub const SBS_VERT: u32 = 1; // SbsVert
pub const WS_CLIPCHILDREN: u32 = 0x02000000; // WsClipChildren
pub const WS_CLIPSIBLINGS: u32 = 0x04000000; // WsClipSiblings

/// C# `SetParent(child, newParent)`: the previous parent (`0` on failure). The caller also changes `WS_CHILD`/`WS_POPUP` itself when the style requires it.
pub fn set_parent(child: Hwnd, new_parent: Hwnd) -> Hwnd {
    // SAFETY: handle values; the OS validates both windows (NULL is an `Err` here, `0` like the C# result).
    unsafe { SetParent(HWND(child as *mut c_void), Some(HWND(new_parent as *mut c_void))) }
        .map(|h| h.0 as isize)
        .unwrap_or(0)
}

/// C# `SetScrollInfo(hwnd, bar, ScrollInfo*, redraw)`: the current scroll position. The wrapper sets `info.cbSize` (28) on a copy.
pub fn set_scroll_info(hwnd: Hwnd, bar: i32, info: &ScrollInfo, redraw: bool) -> i32 {
    let mut copy = *info;
    copy.cbSize = size_of::<ScrollInfo>() as u32; // 28
    // SAFETY: the pointer is to a local `SCROLLINFO` copy with `cbSize` set; the OS only reads it.
    unsafe { SetScrollInfo(HWND(hwnd as *mut c_void), SCROLLBAR_CONSTANTS(bar), &copy, redraw) }
}

/// C# `GetScrollInfo(hwnd, bar, ScrollInfo*)`: fills the fields selected by `info.fMask`; the wrapper sets `cbSize`. `true` on success.
pub fn get_scroll_info(hwnd: Hwnd, bar: i32, info: &mut ScrollInfo) -> bool {
    info.cbSize = size_of::<ScrollInfo>() as u32; // 28
    // SAFETY: the pointer is to a live `&mut SCROLLINFO` with `cbSize` set; the OS fills the fields chosen by `fMask` and does not keep the pointer.
    unsafe { GetScrollInfo(HWND(hwnd as *mut c_void), SCROLLBAR_CONSTANTS(bar), info) }.is_ok()
}

/// C# `GetKeyState(virtualKey)`: the high bit (a negative `i16`) is "down", the low bit "toggled".
pub fn get_key_state(virtual_key: i32) -> i16 {
    // SAFETY: a plain integer argument.
    unsafe { GetKeyState(virtual_key) }
}
