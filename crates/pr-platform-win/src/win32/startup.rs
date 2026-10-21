//! Port of `PhotoReview.Shell.Interop/Win32/StartupAdditions.cs` (baseline `d92d6aae`, 27 lines): the four user32 calls the shell start-up needs and
//! `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2` (`windef.h`: -4).
//!
//! Unit W8-11e.

use core::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow};

use super::structs::{Hwnd, Win32Error};

/// C# `User32.DpiAwarenessContextPerMonitorAwareV2` = `-4` (a pseudo handle, `isize`).
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

/// C# `SetProcessDpiAwarenessContext(value)`: `Err(ERROR_ACCESS_DENIED = 5)` when the awareness was already set (by the manifest or an earlier call); the shell
/// treats that as "already fine" (W8-17). Must be called before the first window is created.
pub fn set_process_dpi_awareness_context(value: isize) -> Result<(), Win32Error> {
    // SAFETY: a pseudo handle value (`-4`...) that the OS interprets; it is never dereferenced.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT(value as *mut c_void)) }.map_err(|e| {
        let code = e.code().0 as u32;
        // `HRESULT_FROM_WIN32(GetLastError())`: the low 16 bits are the Win32 error code.
        Win32Error(if code >> 16 == 0x8007 { code & 0xFFFF } else { code })
    })
}

/// C# `AreDpiAwarenessContextsEqual(first, second)`.
pub fn are_dpi_awareness_contexts_equal(first: isize, second: isize) -> bool {
    // SAFETY: pseudo handle values that the OS compares by meaning; they are never dereferenced.
    unsafe {
        AreDpiAwarenessContextsEqual(
            DPI_AWARENESS_CONTEXT(first as *mut c_void),
            DPI_AWARENESS_CONTEXT(second as *mut c_void),
        )
    }
    .as_bool()
}

/// C# `IsIconic`.
pub fn is_iconic(hwnd: Hwnd) -> bool {
    // SAFETY: a handle value; the OS validates it.
    unsafe { IsIconic(HWND(hwnd as *mut c_void)) }.as_bool()
}

/// C# `SetForegroundWindow`: `false` when the OS refuses (foreground lock); that is not an error the shell reports.
pub fn set_foreground_window(hwnd: Hwnd) -> bool {
    // SAFETY: a handle value; the OS validates it and refuses (returns FALSE) when the foreground lock applies.
    unsafe { SetForegroundWindow(HWND(hwnd as *mut c_void)) }.as_bool()
}
