//! Replaces `PhotoReview.Shell.Win32/Hosting/PendingInterop/PendingNativeMethods.cs` (102 lines, baseline `d92d6aae`; plan row W8-13b: "retired
//! (windows-rs)"). Unit W8-13b.
//!
//! What the C# file declared and `win32::api` (W8-11b..11d) does NOT wrap: `GetCapture`, `ValidateRect`, `WindowFromPoint`, `LoadImage`, and the
//! constants/structs the host needs (`HTCLIENT`, `WA_INACTIVE`, `IDC_WAIT`, `IDC_SIZEALL`, `WM_SETICON`, `MINMAXINFO`, `CREATESTRUCTW`). The four calls
//! go straight to the `windows` crate here (no new C# P/Invoke is ported); everything else the host uses comes from `win32::{api, messages,
//! dialog_native, comctl}` (see the contract's table). The functions are safe: none lets an integer argument be dereferenced by the callee.

use std::ffi::c_void;

use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::UI::Input::KeyboardAndMouse::GetCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, GDI_IMAGE_TYPE, IMAGE_FLAGS, LoadImageW, MINMAXINFO, WindowFromPoint,
};
use windows::core::PCWSTR;

use crate::win32::structs::{Hinstance, Hwnd, Point};

/// `CREATESTRUCTW` (`lParam` of `WM_NCCREATE`/`WM_CREATE`; C# `PendingCreateStruct`).
pub type CreateStruct = CREATESTRUCTW;
/// `MINMAXINFO` (`lParam` of `WM_GETMINMAXINFO`; C# `PendingMinMaxInfo`).
pub type MinMaxInfo = MINMAXINFO;

/// C# `HwndMessage` = `HWND_MESSAGE` (-3): parent of a message-only window.
pub const HWND_MESSAGE: Hwnd = -3;
/// C# `HtClient`.
pub const HTCLIENT: i32 = 1;
/// C# `WaInactive`.
pub const WA_INACTIVE: i32 = 0;
/// C# `IdcWait`.
pub const IDC_WAIT: i32 = 32514;
/// C# `IdcSizeAll`.
pub const IDC_SIZEALL: i32 = 32646;
/// `WM_SETICON` (0x0080); not in `win32::messages`.
pub const WM_SETICON: u32 = 0x0080;
/// `ICON_SMALL`.
pub const ICON_SMALL: usize = 0;
/// `ICON_BIG`.
pub const ICON_BIG: usize = 1;
/// `IMAGE_ICON`.
pub const IMAGE_ICON: u32 = 1;
/// `LR_SHARED`.
pub const LR_SHARED: u32 = 0x8000;

/// `GetCapture()`: the window that has the mouse capture on this thread, `0` for none.
pub fn get_capture() -> Hwnd {
    // SAFETY: no arguments; reads the calling thread's capture window.
    unsafe { GetCapture() }.0 as isize
}

/// `ValidateRect(hwnd, NULL)`: marks the whole client area valid (C# `PendingUser32.ValidateRect(hwnd, null)`).
pub fn validate_whole_client(hwnd: Hwnd) -> bool {
    // SAFETY: a null rectangle pointer means "the whole client area"; the window handle is validated by the OS.
    unsafe { ValidateRect(Some(HWND(hwnd as *mut c_void)), None) }.as_bool()
}

/// `WindowFromPoint(point)`: the window under a screen point, `0` for none.
pub fn window_from_point(point: Point) -> Hwnd {
    // SAFETY: the point is passed by value; no pointer argument.
    unsafe { WindowFromPoint(point) }.0 as isize
}

/// `LoadImageW(instance, MAKEINTRESOURCE(resource_id), IMAGE_ICON, cx, cy, LR_SHARED)`: the shared icon handle, `0` when it cannot be loaded
/// (the error is not needed: callers fall back to the default icon).
pub fn load_shared_icon(instance: Hinstance, resource_id: i32, cx: i32, cy: i32) -> isize {
    // SAFETY: the "name" is `MAKEINTRESOURCE(resource_id)`, an integer atom in a pointer that the OS never dereferences as text; the instance is
    // a handle value the OS validates (0 = the system module); no memory is read or written by us.
    unsafe {
        LoadImageW(
            Some(HINSTANCE(instance as *mut c_void)),
            PCWSTR(resource_id as usize as *const u16),
            GDI_IMAGE_TYPE(IMAGE_ICON),
            cx,
            cy,
            IMAGE_FLAGS(LR_SHARED),
        )
    }
    .map_or(0, |handle| handle.0 as isize)
}
