//! Port of `PhotoReview.Shell.Win32/Hosting/ShellIcons.cs` (68 lines, baseline `d92d6aae`). Unit W8-13b.
//!
//! The app icon for the shell windows, loaded from the exe's `RT_GROUP_ICON` resource (id 32512, which is where the C# compiler put
//! `ApplicationIcon`) with `LoadImage(LR_SHARED)`: the system owns the handle, nothing to destroy. The window class gets the big and small icon for
//! the SYSTEM dpi; every window gets `WM_SETICON` for ITS dpi (taskbar, Alt-Tab, title bar) and again on `WM_DPICHANGED`. A missing resource sets
//! nothing (the window keeps the default icon): this module never fails and never panics.
//!
//! The Rust exe must embed that resource (W8-20a, build script; see the contract, Q3). The C# fallback to the test assembly's module
//! (`Marshal.GetHINSTANCE(typeof(ShellIcons).Module)`) has no Rust twin: a `cargo test` executable has no icon, so the tests use the seams below.

use crate::host::native;
use crate::win32::api::{kernel32, user32};
use crate::win32::dialog_native::user32 as dialog_user32;
use crate::win32::structs::{Hinstance, Hwnd};

/// C# `ResourceId`: `RT_GROUP_ICON` id of the main icon (32512 = `IDI_APPLICATION`).
pub const RESOURCE_ID: i32 = 32512;
/// C# `SmCxIcon`.
pub const SM_CXICON: i32 = 11;
/// C# `SmCyIcon`.
pub const SM_CYICON: i32 = 12;
/// C# `SmCxSmIcon`.
pub const SM_CXSMICON: i32 = 49;
/// C# `SmCySmIcon`.
pub const SM_CYSMICON: i32 = 50;

/// C# `Load(cx, cy)`: `cx <= 0 || cy <= 0` gives `0` without calling the OS; else `LoadImage(exe module, RESOURCE_ID, IMAGE_ICON, cx, cy,
/// LR_SHARED)`; `0` when the resource is missing.
pub fn load(cx: i32, cy: i32) -> isize {
    if cx <= 0 || cy <= 0 {
        return 0;
    }
    // A failing `GetModuleHandle` cannot happen for the running exe; `0` would load from the system module, so give up instead.
    match kernel32::get_module_handle() {
        Ok(instance) => load_from(instance, cx, cy),
        Err(_) => 0,
    }
}

/// Seam of [`load`]: the same with an explicit module (`0` = the system, where `RESOURCE_ID` is the stock application icon, so a test can prove the
/// `LoadImage` path without an exe resource).
pub fn load_from(instance: Hinstance, cx: i32, cy: i32) -> isize {
    if cx <= 0 || cy <= 0 {
        return 0;
    }
    native::load_shared_icon(instance, RESOURCE_ID, cx, cy)
}

/// C# `LoadBig(dpi)`: `load(metric(SM_CXICON, dpi), metric(SM_CYICON, dpi))`. `dpi == 0` means the system metric (the window class is registered
/// before any window has a dpi).
pub fn load_big(dpi: u32) -> isize {
    load(metric(SM_CXICON, dpi), metric(SM_CYICON, dpi))
}

/// C# `LoadSmall(dpi)`.
pub fn load_small(dpi: u32) -> isize {
    load(metric(SM_CXSMICON, dpi), metric(SM_CYSMICON, dpi))
}

/// C# `Apply(hwnd, dpi)`: `false` for `hwnd == 0`; loads the big and small icon for `dpi` and `WM_SETICON`s the ones that loaded
/// (`ICON_BIG` = 1, `ICON_SMALL` = 0); `true` when at least one icon was found. Uses `load_big`/`load_small`.
pub fn apply(hwnd: Hwnd, dpi: u32) -> bool {
    apply_with(hwnd, dpi, &load)
}

/// Seam of [`apply`] for tests: `loader(cx, cy)` replaces `load`.
pub fn apply_with(hwnd: Hwnd, dpi: u32, loader: &dyn Fn(i32, i32) -> isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    let big = loader(metric(SM_CXICON, dpi), metric(SM_CYICON, dpi));
    let small = loader(metric(SM_CXSMICON, dpi), metric(SM_CYSMICON, dpi));
    if big != 0 {
        set_icon(hwnd, native::ICON_BIG, big);
    }
    if small != 0 {
        set_icon(hwnd, native::ICON_SMALL, small);
    }
    big != 0 || small != 0
}

/// `SendMessage(hwnd, WM_SETICON, kind, icon)`; the result (the previous icon) is not needed.
fn set_icon(hwnd: Hwnd, kind: usize, icon: isize) {
    // SAFETY: WM_SETICON takes an HICON in lParam, a handle the OS validates; no memory is dereferenced.
    let _previous = unsafe { dialog_user32::send_message(hwnd, native::WM_SETICON, kind, icon) };
}

/// C# private `Metric(index, dpi)`: `dpi == 0` gives `GetSystemMetrics(index)`, else `GetSystemMetricsForDpi(index, dpi)`.
pub fn metric(index: i32, dpi: u32) -> i32 {
    if dpi == 0 {
        user32::get_system_metrics(index)
    } else {
        user32::get_system_metrics_for_dpi(index, dpi)
    }
}
