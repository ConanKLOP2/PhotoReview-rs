//! Port of `PhotoReview.Shell.Interop/Win32/StartupAdditions.cs` (baseline `d92d6aae`, 27 lines): the four user32 calls the shell start-up needs and
//! `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2` (`windef.h`: -4).
//!
//! Unit W8-11e.
#![allow(unused_variables, dead_code)] // stub: removed by the W8-11e porter when the last `todo!()` is gone.

use super::structs::{Hwnd, Win32Error};

/// C# `User32.DpiAwarenessContextPerMonitorAwareV2` = `-4` (a pseudo handle, `isize`).
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;

/// C# `SetProcessDpiAwarenessContext(value)`: `Err(ERROR_ACCESS_DENIED = 5)` when the awareness was already set (by the manifest or an earlier call); the shell
/// treats that as "already fine" (W8-17). Must be called before the first window is created.
pub fn set_process_dpi_awareness_context(value: isize) -> Result<(), Win32Error> {
    todo!()
}

/// C# `AreDpiAwarenessContextsEqual(first, second)`.
pub fn are_dpi_awareness_contexts_equal(first: isize, second: isize) -> bool {
    todo!()
}

/// C# `IsIconic`.
pub fn is_iconic(hwnd: Hwnd) -> bool {
    todo!()
}

/// C# `SetForegroundWindow`: `false` when the OS refuses (foreground lock); that is not an error the shell reports.
pub fn set_foreground_window(hwnd: Hwnd) -> bool {
    todo!()
}
