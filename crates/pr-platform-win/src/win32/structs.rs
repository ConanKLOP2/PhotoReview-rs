//! Port of `PhotoReview.Shell.Interop/Win32/Structs.cs` (137 lines, baseline `d92d6aae`) plus the structs the other Interop files declare
//! (`ScrollInfo` in `SettingsDialogNative.cs`, `ActivationContext.ActCtx`, `FormatEtc`/`StgMedium`/`PointL`/`ComdlgFilterSpec` in `Shell/*.cs`).
//!
//! Unit W8-11a. The C# structs exist because the C# project has no Windows SDK headers; the `windows` crate generates the same x64 layouts, so
//! each C# struct becomes a type alias of the `windows` type under the C# name (D-W8B3-3) and the layout is pinned by compile-time asserts here
//! and by `tests/golden/win32-constants.json` (kind `layout`, 16 types). This file also owns what every other Win32 unit shares: the handle
//! aliases, `Win32Error`/`Hresult`, the window-procedure type and the UTF-16 helpers.

use core::fmt;

use windows::Win32::Foundation::{GetLastError, POINT, POINTL, RECT};
use windows::Win32::Graphics::Gdi::{MONITORINFO, MONITORINFOEXW};
use windows::Win32::System::ApplicationInstallationAndServicing::ACTCTXW;
use windows::Win32::System::Com::{FORMATETC, STGMEDIUM};
use windows::Win32::UI::Controls::{TASKDIALOG_BUTTON, TASKDIALOGCONFIG};
use windows::Win32::UI::Input::INPUT_MESSAGE_SOURCE;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::WindowsAndMessaging::{MSG, SCROLLINFO, WINDOWPLACEMENT, WNDCLASSEXW};

// ---- handles and message parameter types (C# `nint`/`nuint`; D-W8B3-3) ----
// Plain integers on purpose: `pr_app` and the hosts pass window handles around without the `windows` crate (D-W8-16e-2). The aliases carry no type
// safety; the OS validates a stale or wrong handle and fails the call (that is never undefined behaviour in the wrappers of this module).

/// `HWND`.
pub type Hwnd = isize;
/// `HDC`.
pub type Hdc = isize;
/// `HMENU`.
pub type Hmenu = isize;
/// `HMONITOR`.
pub type Hmonitor = isize;
/// `HINSTANCE`/`HMODULE`.
pub type Hinstance = isize;
/// Any GDI object handle: `HBRUSH`, `HPEN`, `HFONT`, `HBITMAP`, `HGDIOBJ`.
pub type Hgdi = isize;
/// `HCURSOR`.
pub type Hcursor = isize;
/// `WPARAM` (C# declares it `nint`; the SDK type is unsigned: values are the same bits).
pub type Wparam = usize;
/// `LPARAM`.
pub type Lparam = isize;
/// `LRESULT`.
pub type Lresult = isize;

/// The window procedure: C# `delegate* unmanaged<nint, uint, nint, nint, nint>` in `WndClassEx.WndProc`. Also the dialog procedure (`DLGPROC`,
/// `INT_PTR` result) of `CreateDialogIndirectParam`. Callers define `unsafe extern "system" fn` items; a panic must never unwind out of one.
pub type WndProc = unsafe extern "system" fn(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult;

// ---- errors ----

/// A Win32 error code: the value of `GetLastError()` read straight after a failed call (C# `Marshal.GetLastPInvokeError()` after a `SetLastError = true`
/// P/Invoke). `Display`: `Win32 error {code} (0x{code:08X})`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Win32Error(pub u32);

impl Win32Error {
    /// `GetLastError()` of the calling thread. Wrappers call this immediately after the failing call and before anything else that could change it.
    pub fn last() -> Self {
        // SAFETY: `GetLastError` only reads the calling thread's last-error value; no pointers are involved.
        Self(unsafe { GetLastError() }.0)
    }
}

impl fmt::Display for Win32Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Win32 error {} (0x{:08X})", self.0, self.0)
    }
}

impl std::error::Error for Win32Error {}

/// A failed `HRESULT` (negative `i32`), the error of the wrappers whose C# declaration returns `int`. `Display`: `HRESULT 0x{value as u32:08X}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hresult(pub i32);

impl Hresult {
    /// C# `hr >= 0`.
    pub fn succeeded(self) -> bool {
        self.0 >= 0
    }
}

impl fmt::Display for Hresult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HRESULT 0x{:08X}", self.0 as u32)
    }
}

impl std::error::Error for Hresult {}

// ---- struct aliases (C# name = windows-rs type) ----

/// `RECT` (C# `Rect`).
pub type Rect = RECT;
/// `POINT` (C# `Point`). Also used as `SIZE` by `GetTextExtentPoint32` (`X` = cx, `Y` = cy), like the C# code does.
pub type Point = POINT;
/// `WNDCLASSEXW` (C# `WndClassEx`); `lpfnWndProc` is `Option<WNDPROC>`.
pub type WndClassEx = WNDCLASSEXW;
/// `MSG` (C# `Msg`).
pub type Msg = MSG;
/// `MONITORINFO` (C# `MonitorInfo`).
pub type MonitorInfo = MONITORINFO;
/// `MONITORINFOEXW` (C# `MonitorInfoEx`).
pub type MonitorInfoEx = MONITORINFOEXW;
/// `WINDOWPLACEMENT` (C# `WindowPlacement`).
pub type WindowPlacement = WINDOWPLACEMENT;
/// `INPUT_MESSAGE_SOURCE` (C# `InputMessageSource`).
pub type InputMessageSource = INPUT_MESSAGE_SOURCE;
/// `TASKDIALOG_BUTTON`, 1-byte packed (C# `TaskDialogButton`, `Pack = 1`).
pub type TaskDialogButton = TASKDIALOG_BUTTON;
/// `TASKDIALOGCONFIG`, 1-byte packed (C# `TaskDialogConfig`, `Pack = 1`).
pub type TaskDialogConfig = TASKDIALOGCONFIG;
/// `SCROLLINFO` (C# `ScrollInfo`, declared in `SettingsDialogNative.cs`).
pub type ScrollInfo = SCROLLINFO;
/// `ACTCTXW` (C# `ActivationContext.ActCtx`).
pub type ActCtx = ACTCTXW;
/// `FORMATETC` (C# `Shell.FormatEtc`).
pub type FormatEtc = FORMATETC;
/// `STGMEDIUM` (C# `Shell.StgMedium`).
pub type StgMedium = STGMEDIUM;
/// `POINTL` (C# `Shell.PointL`).
pub type PointL = POINTL;
/// `COMDLG_FILTERSPEC` (C# `Shell.ComdlgFilterSpec`).
pub type ComdlgFilterSpec = COMDLG_FILTERSPEC;

// ---- layout pins: the x64 sizes and offsets of `InteropSignatureTests` (the golden checks all 16 types at run time) ----
const _: () = {
    assert!(size_of::<Rect>() == 16);
    assert!(size_of::<Point>() == 8);
    assert!(size_of::<WndClassEx>() == 80);
    assert!(core::mem::offset_of!(WndClassEx, lpfnWndProc) == 8);
    assert!(core::mem::offset_of!(WndClassEx, lpszClassName) == 64);
    assert!(core::mem::offset_of!(WndClassEx, hIconSm) == 72);
    assert!(size_of::<Msg>() == 48);
    assert!(core::mem::offset_of!(Msg, time) == 32);
    assert!(core::mem::offset_of!(Msg, pt) == 36);
    assert!(size_of::<MonitorInfo>() == 40);
    assert!(size_of::<MonitorInfoEx>() == 104);
    assert!(size_of::<WindowPlacement>() == 44);
    assert!(size_of::<InputMessageSource>() == 8);
    assert!(size_of::<TaskDialogButton>() == 12);
    assert!(size_of::<TaskDialogConfig>() == 160);
    assert!(core::mem::offset_of!(TaskDialogConfig, hwndParent) == 4);
    assert!(core::mem::offset_of!(TaskDialogConfig, cButtons) == 60);
    assert!(core::mem::offset_of!(TaskDialogConfig, pButtons) == 64);
    assert!(core::mem::offset_of!(TaskDialogConfig, pfCallback) == 140);
    assert!(core::mem::offset_of!(TaskDialogConfig, cxWidth) == 156);
    assert!(size_of::<ScrollInfo>() == 28);
    assert!(size_of::<ActCtx>() == 56);
    assert!(size_of::<FormatEtc>() == 32);
    assert!(size_of::<StgMedium>() == 24);
    assert!(size_of::<PointL>() == 8);
    assert!(size_of::<ComdlgFilterSpec>() == 16);
};

// ---- constructors that set the size field the C# comments demand ("cbSize must equal sizeof before the call") ----

/// `WNDCLASSEXW` zeroed with `cbSize = 80`.
pub fn new_wnd_class_ex() -> WndClassEx {
    WndClassEx {
        cbSize: size_of::<WndClassEx>() as u32,
        ..Default::default()
    }
}

/// `MONITORINFO` zeroed with `cbSize = 40`.
pub fn new_monitor_info() -> MonitorInfo {
    MonitorInfo {
        cbSize: size_of::<MonitorInfo>() as u32,
        ..Default::default()
    }
}

/// `MONITORINFOEXW` zeroed with `cbSize = 104` (not 40: the C# comment "cbSize must be sizeof(MonitorInfoEx)" and the C# test `MonitorInfoEx_Size_Is104`).
pub fn new_monitor_info_ex() -> MonitorInfoEx {
    let mut info = MonitorInfoEx::default();
    info.monitorInfo.cbSize = size_of::<MonitorInfoEx>() as u32;
    info
}

/// `WINDOWPLACEMENT` zeroed with `length = 44`.
pub fn new_window_placement() -> WindowPlacement {
    WindowPlacement {
        length: size_of::<WindowPlacement>() as u32,
        ..Default::default()
    }
}

/// `SCROLLINFO` zeroed with `cbSize = 28`.
pub fn new_scroll_info() -> ScrollInfo {
    ScrollInfo {
        cbSize: size_of::<ScrollInfo>() as u32,
        ..Default::default()
    }
}

/// `TASKDIALOGCONFIG` zeroed with `cbSize = 160`.
pub fn new_task_dialog_config() -> TaskDialogConfig {
    TaskDialogConfig {
        cbSize: size_of::<TaskDialogConfig>() as u32,
        ..Default::default()
    }
}

/// `ACTCTXW` zeroed with `cbSize = 56`.
pub fn new_act_ctx() -> ActCtx {
    ActCtx {
        cbSize: size_of::<ActCtx>() as u32,
        ..Default::default()
    }
}

// ---- UTF-16 helpers shared by the wrappers (C# `StringMarshalling.Utf16` and `fixed (char*)`) ----

/// `text` as UTF-16 code units followed by one NUL. An embedded NUL stays in the buffer (the OS then sees a shorter string, like C#): a title or
/// path passed through here is silently cut at the first NUL, so callers that build the text from untrusted input must reject NUL themselves (ST-WIN32 L11).
pub fn to_utf16z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(core::iter::once(0)).collect()
}

/// UTF-16 code units up to the first NUL (or the whole slice) to a `String`; unpaired surrogates become U+FFFD (C# `new string(char*)` would keep them).
pub fn utf16_to_string(units: &[u16]) -> String {
    let end = units.iter().position(|&unit| unit == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}
