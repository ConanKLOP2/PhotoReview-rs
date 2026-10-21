//! Port of `PhotoReview.Shell.Win32/Hosting/ShellIcons.cs` (68 lines, baseline `d92d6aae`). Unit W8-13b.
//!
//! The app icon for the shell windows, loaded from the exe's `RT_GROUP_ICON` resource (id 32512, which is where the C# compiler put
//! `ApplicationIcon`) with `LoadImage(LR_SHARED)`: the system owns the handle, nothing to destroy. The window class gets the big and small icon for
//! the SYSTEM dpi; every window gets `WM_SETICON` for ITS dpi (taskbar, Alt-Tab, title bar) and again on `WM_DPICHANGED`. A missing resource sets
//! nothing (the window keeps the default icon): this module never fails and never panics.
//!
//! The Rust exe must embed that resource (W8-20a, build script; see the contract, Q3). The C# fallback to the test assembly's module
//! (`Marshal.GetHINSTANCE(typeof(ShellIcons).Module)`) has no Rust twin: a `cargo test` executable has no icon, so the tests use the seams below.

#![allow(unused_variables, dead_code)] // W8-13b stub: the porter removes this line when the last `todo!()` is gone.

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
    todo!("W8-13b")
}

/// Seam of [`load`]: the same with an explicit module (`0` = the system, where `RESOURCE_ID` is the stock application icon, so a test can prove the
/// `LoadImage` path without an exe resource).
pub fn load_from(instance: Hinstance, cx: i32, cy: i32) -> isize {
    todo!("W8-13b")
}

/// C# `LoadBig(dpi)`: `load(metric(SM_CXICON, dpi), metric(SM_CYICON, dpi))`. `dpi == 0` means the system metric (the window class is registered
/// before any window has a dpi).
pub fn load_big(dpi: u32) -> isize {
    todo!("W8-13b")
}

/// C# `LoadSmall(dpi)`.
pub fn load_small(dpi: u32) -> isize {
    todo!("W8-13b")
}

/// C# `Apply(hwnd, dpi)`: `false` for `hwnd == 0`; loads the big and small icon for `dpi` and `WM_SETICON`s the ones that loaded
/// (`ICON_BIG` = 1, `ICON_SMALL` = 0); `true` when at least one icon was found. Uses `load_big`/`load_small`.
pub fn apply(hwnd: Hwnd, dpi: u32) -> bool {
    todo!("W8-13b")
}

/// Seam of [`apply`] for tests: `loader(cx, cy)` replaces `load`.
pub fn apply_with(hwnd: Hwnd, dpi: u32, loader: &dyn Fn(i32, i32) -> isize) -> bool {
    todo!("W8-13b")
}

/// C# private `Metric(index, dpi)`: `dpi == 0` gives `GetSystemMetrics(index)`, else `GetSystemMetricsForDpi(index, dpi)`.
pub fn metric(index: i32, dpi: u32) -> i32 {
    todo!("W8-13b")
}
