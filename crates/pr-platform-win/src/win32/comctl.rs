//! Port of `PhotoReview.Shell.Interop/Win32/{CommonControlsNative, Comctl32}.cs` (baseline `d92d6aae`; 280 + 39 lines, the constants class `ControlConstants`
//! of the first file is `control_constants.rs`, `ActivationContext.cs` is `activation_context.rs`): the user32/gdi32/comctl32 calls the native dialog controls
//! need (combo box info, focus rectangle, class name, rounded rectangle, pens, common-control initialisation, window subclassing) and `TaskDialogIndirect`.
//!
//! Unit W8-11c. `TaskDialogIndirect` exists only in comctl32 v6, so it is bound at run time with `GetProcAddress` on the first use (D-W8B3-8): a static import
//! would stop every test executable without a manifest from starting (checked on this machine: `System32\comctl32.dll`, the v5.82 that a manifest-less process
//! loads, has 119 named exports and no `TaskDialogIndirect`; it does export `InitCommonControlsEx`, `SetWindowSubclass`, `RemoveWindowSubclass` and
//! `DefSubclassProc` by name, so those four are imported statically through the `windows` crate). `GetWindowSubclass` is also v6-only (the C# comment) and is not ported.
use core::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};

use super::structs::{Hdc, Hgdi, Hwnd, Lparam, Lresult, Point, Rect, TaskDialogConfig, Wparam};

pub const TDCBF_OK_BUTTON: u32 = 1; // TdcbfOkButton
pub const TDCBF_YES_BUTTON: u32 = 2; // TdcbfYesButton
pub const TDCBF_NO_BUTTON: u32 = 4; // TdcbfNoButton
pub const TDCBF_CANCEL_BUTTON: u32 = 8; // TdcbfCancelButton
pub const TDCBF_RETRY_BUTTON: u32 = 0x0010; // TdcbfRetryButton
pub const TDCBF_CLOSE_BUTTON: u32 = 0x0020; // TdcbfCloseButton
pub const TDF_ENABLE_HYPERLINKS: u32 = 1; // TdfEnableHyperlinks
pub const TDF_USE_HICON_MAIN: u32 = 2; // TdfUseHIconMain
pub const TDF_USE_HICON_FOOTER: u32 = 4; // TdfUseHIconFooter
pub const TDF_ALLOW_DIALOG_CANCELLATION: u32 = 8; // TdfAllowDialogCancellation
pub const TDF_USE_COMMAND_LINKS: u32 = 0x0010; // TdfUseCommandLinks
pub const TDF_EXPANDED_BY_DEFAULT: u32 = 0x0080; // TdfExpandedByDefault
pub const TDF_VERIFICATION_FLAG_CHECKED: u32 = 0x0100; // TdfVerificationFlagChecked
pub const TDF_POSITION_RELATIVE_TO_WINDOW: u32 = 0x1000; // TdfPositionRelativeToWindow
pub const TDF_RTL_LAYOUT: u32 = 0x2000; // TdfRtlLayout
pub const TDF_NO_DEFAULT_RADIO_BUTTON: u32 = 0x4000; // TdfNoDefaultRadioButton
pub const TDF_CAN_BE_MINIMIZED: u32 = 0x8000; // TdfCanBeMinimized
pub const TDF_SIZE_TO_CONTENT: u32 = 0x01000000; // TdfSizeToContent
pub const TD_WARNING_ICON: isize = -1; // TdWarningIcon
pub const TD_ERROR_ICON: isize = -2; // TdErrorIcon
pub const TD_INFORMATION_ICON: isize = -3; // TdInformationIcon
pub const TD_SHIELD_ICON: isize = -4; // TdShieldIcon

/// The Common-Controls v6 + PerMonitorV2 manifest of the shell (`app.manifest` of `PhotoReview.Shell.Win32`, identity renamed to `PhotoReviewRs`). The app
/// exe embeds it at link time (W8-20a decides how: linker `/MANIFESTINPUT`); the native tests activate it through `activation_context`.
pub const SHELL_MANIFEST: &str = include_str!("shell.manifest");

/// The subclass procedure: C# `delegate* unmanaged<nint, uint, nint, nint, nuint, nuint, nint>` (`hwnd, message, wParam, lParam, subclassId, refData`).
pub type SubclassProc = unsafe extern "system" fn(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    id: usize,
    ref_data: usize,
) -> Lresult;

/// The three outputs of `TaskDialogIndirect` (C# `int* button`, `int* radioButton`, `int* verificationChecked`; the C# caller may pass null for the
/// ones it does not need, the wrapper always asks for all three).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskDialogResult {
    /// The id of the button the user chose (`IDOK`, `IDCANCEL`, `IDYES`, `IDNO`, a custom id...).
    pub button: i32,
    /// The id of the selected radio button (`0` when the dialog has none).
    pub radio_button: i32,
    /// Whether the verification check box was checked.
    pub verification_checked: bool,
}

/// `user32.dll` additions of `CommonControlsNative.cs` (the `User32` partial).
pub mod user32 {
    use super::*;
    use windows::Win32::Graphics::Gdi::{DrawFocusRect, FillRect, HBRUSH, HDC};
    use windows::Win32::UI::Controls::{COMBOBOXINFO, GetComboBoxInfo};
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetCursorPos, GetParent};

    /// C# `GetCursorPos(Point*)`: `None` when the call fails.
    pub fn get_cursor_pos() -> Option<Point> {
        let mut point = Point::default();
        // SAFETY: `point` is a live local POINT that the OS fills.
        unsafe { GetCursorPos(&mut point) }.ok().map(|()| point)
    }

    /// C# `GetComboBoxInfo(combo, void* info)` with a `COMBOBOXINFO` of `cbSize = 64` (`COMBOBOXINFO_SIZE`): `None` when the call fails. The windows-rs
    /// struct replaces the raw 64-byte buffer; `hwndList` is at offset 56 (`COMBOBOXINFO_HWNDLIST_OFFSET`).
    pub fn get_combo_box_info(combo: Hwnd) -> Option<windows::Win32::UI::Controls::COMBOBOXINFO> {
        let mut info = COMBOBOXINFO {
            cbSize: size_of::<COMBOBOXINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is a local COMBOBOXINFO with `cbSize` set; the OS writes at most `cbSize` bytes.
        unsafe { GetComboBoxInfo(HWND(combo as *mut c_void), &mut info) }
            .ok()
            .map(|()| info)
    }

    /// C# `DrawFocusRect`.
    pub fn draw_focus_rect(hdc: Hdc, rect: &Rect) -> bool {
        // SAFETY: `rect` is a live `&RECT` that the OS only reads; the DC is a handle value the OS validates.
        unsafe { DrawFocusRect(HDC(hdc as *mut c_void), rect) }.as_bool()
    }

    /// C# `FillRect(hdc, rect, brush)`: non-zero on success.
    pub fn fill_rect(hdc: Hdc, rect: &Rect, brush: Hgdi) -> i32 {
        // SAFETY: `rect` is a live `&RECT` that the OS only reads; the DC and brush are handle values the OS validates.
        unsafe { FillRect(HDC(hdc as *mut c_void), rect, HBRUSH(brush as *mut c_void)) }
    }

    /// C# `GetParent`: `0` when there is no parent (or on error).
    pub fn get_parent(hwnd: Hwnd) -> Hwnd {
        // SAFETY: a handle value; the OS validates it (an error maps to `0`, like C#).
        unsafe { GetParent(HWND(hwnd as *mut c_void)) }.map_or(0, |parent| parent.0 as isize)
    }

    /// C# `GetClassName(hwnd, char* buffer, maxCount)`: the class name, `None` on failure. The wrapper uses a 256 unit buffer (the longest class name is 256
    /// characters including the NUL).
    pub fn get_class_name(hwnd: Hwnd) -> Option<String> {
        let mut buffer = [0u16; 256];
        // SAFETY: the buffer is a local `[u16; 256]` and the slice length passed is 256.
        let length = unsafe { GetClassNameW(HWND(hwnd as *mut c_void), &mut buffer) };
        let length = usize::try_from(length).ok().filter(|&n| n > 0)?;
        buffer.get(..length).map(String::from_utf16_lossy)
    }
}

/// `gdi32.dll` additions of `CommonControlsNative.cs` (the `Gdi32` partial).
pub mod gdi32 {
    use super::*;
    use windows::Win32::Graphics::Gdi::{BACKGROUND_MODE, CreatePen, HDC, PEN_STYLE, RoundRect, SetBkMode};

    /// C# `SetBkMode(hdc, mode)` (`TRANSPARENT` = 1, `OPAQUE` = 2): the previous mode, `0` on failure.
    pub fn set_bk_mode(hdc: Hdc, mode: i32) -> i32 {
        // SAFETY: a DC handle value the OS validates and a mode integer.
        unsafe { SetBkMode(HDC(hdc as *mut c_void), BACKGROUND_MODE(mode as u32)) }
    }

    /// C# `RoundRect(hdc, l, t, r, b, ellipseWidth, ellipseHeight)`.
    pub fn round_rect(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32, width: i32, height: i32) -> bool {
        // SAFETY: a DC handle value the OS validates and plain integers.
        unsafe { RoundRect(HDC(hdc as *mut c_void), left, top, right, bottom, width, height) }.as_bool()
    }

    /// C# `CreatePen(style, width, colorRef)`: `0` on failure. The caller deletes it with `DeleteObject` (W8-11d `gdi32::delete_object`).
    pub fn create_pen(style: i32, width: i32, color_ref: u32) -> Hgdi {
        // SAFETY: plain integers; the caller owns (and deletes) the returned pen.
        unsafe { CreatePen(PEN_STYLE(style), width, COLORREF(color_ref)) }.0 as isize
    }
}

/// C# `CommonControls` (comctl32.dll).
pub mod common_controls {
    use super::*;
    use windows::Win32::UI::Controls::{INITCOMMONCONTROLSEX, INITCOMMONCONTROLSEX_ICC, InitCommonControlsEx};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};

    /// C# `InitCommonControlsEx(uint* controls)` with `{ dwSize = 8, dwICC = classes }` built by the wrapper (the C# callers build a `uint[2]`); `classes` is a
    /// combination of the `ICC_*` constants of `control_constants`. `true` on success.
    pub fn init_common_controls_ex(classes: u32) -> bool {
        let init = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: INITCOMMONCONTROLSEX_ICC(classes),
        };
        // SAFETY: `init` is a live local INITCOMMONCONTROLSEX with `dwSize` set; the OS only reads it.
        unsafe { InitCommonControlsEx(&init) }.as_bool()
    }

    /// C# `SetWindowSubclass(hwnd, proc, id, refData)`: `true` on success. The subclass is identified by `(proc, id)`.
    ///
    /// # Safety
    /// `proc` must stay valid until `remove_window_subclass` or `WM_NCDESTROY`, and `ref_data` is passed to it as is.
    pub unsafe fn set_window_subclass(hwnd: Hwnd, proc: SubclassProc, id: usize, ref_data: usize) -> bool {
        // SAFETY: `SubclassProc` and the windows-rs `SUBCLASSPROC` target have the same `extern "system"` ABI: `Hwnd` is a pointer-sized integer
        // where `HWND` is a pointer, `Wparam`/`Lparam`/`Lresult` have the size of `WPARAM`/`LPARAM`/`LRESULT`.
        let native: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT = unsafe {
            core::mem::transmute::<
                SubclassProc,
                unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT,
            >(proc)
        };
        // SAFETY: upheld by the caller of this function (`proc` stays valid until removal or `WM_NCDESTROY`); the handle is a value the OS validates.
        unsafe { SetWindowSubclass(HWND(hwnd as *mut c_void), Some(native), id, ref_data) }.as_bool()
    }

    /// C# `RemoveWindowSubclass(hwnd, proc, id)`: `true` when a subclass was removed.
    pub fn remove_window_subclass(hwnd: Hwnd, proc: SubclassProc, id: usize) -> bool {
        // SAFETY: same ABI as in `set_window_subclass`; the pair `(proc, id)` is only compared by the OS, never called here.
        let native: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT = unsafe {
            core::mem::transmute::<
                SubclassProc,
                unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT,
            >(proc)
        };
        // SAFETY: a handle value the OS validates; the function pointer is only used as an identity.
        unsafe { RemoveWindowSubclass(HWND(hwnd as *mut c_void), Some(native), id) }.as_bool()
    }

    /// C# `DefSubclassProc`: calls the next procedure in the subclass chain.
    ///
    /// # Safety
    /// Call it only from a subclass procedure with the message parameters that procedure received.
    pub unsafe fn def_subclass_proc(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        // SAFETY: upheld by the caller of this function (called from a subclass procedure with its own message parameters).
        unsafe { DefSubclassProc(HWND(hwnd as *mut c_void), message, WPARAM(wparam), LPARAM(lparam)) }.0
    }
}

/// C# `Comctl32.TaskDialogIndirect` (comctl32 v6 only, resolved at run time: module docs).
pub mod task_dialog {
    use super::*;
    use crate::win32::structs::{Hresult, to_utf16z};
    use std::sync::OnceLock;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{BOOL, HRESULT, PCWSTR, s};

    /// `E_NOINTERFACE` (0x80004002) as the signed `HRESULT` value: what `task_dialog_indirect` returns when comctl32 v6 is not active.
    const E_NOINTERFACE: i32 = 0x8000_4002_u32 as i32;

    /// The export of comctl32 v6: `HRESULT TaskDialogIndirect(const TASKDIALOGCONFIG*, int*, int*, BOOL*)`.
    type TaskDialogIndirectFn =
        unsafe extern "system" fn(*const TaskDialogConfig, *mut i32, *mut i32, *mut BOOL) -> HRESULT;

    /// Set on the first successful lookup only (D-W8B3-8): a lookup that failed before an activation context existed must not poison a later one.
    static RESOLVED: OnceLock<TaskDialogIndirectFn> = OnceLock::new();

    /// Resolves `TaskDialogIndirect` under the current thread's activation context.
    fn resolve() -> Option<TaskDialogIndirectFn> {
        if let Some(function) = RESOLVED.get() {
            return Some(*function);
        }
        let name = to_utf16z("comctl32.dll");
        // SAFETY: `name` is a NUL-terminated UTF-16 buffer in a local that lives past the call.
        let module = unsafe { LoadLibraryW(PCWSTR(name.as_ptr())) }.ok()?;
        // SAFETY: `module` is the handle just returned; the export name is a NUL-terminated ASCII literal.
        let address = unsafe { GetProcAddress(module, s!("TaskDialogIndirect")) }?;
        // SAFETY: the export has exactly this documented signature in comctl32 v6.
        let function =
            unsafe { core::mem::transmute::<unsafe extern "system" fn() -> isize, TaskDialogIndirectFn>(address) };
        Some(*RESOLVED.get_or_init(|| function))
    }

    /// `true` when `TaskDialogIndirect` can be resolved now, that is when comctl32 v6 is the one `LoadLibraryW("comctl32.dll")` returns for the current thread
    /// (the exe manifest, or an `ActivationScope` on this thread). Only successful lookups are cached: a failure outside an activation scope must not stop a later
    /// call inside one. New in Rust (D-W8B3-8): C# finds out by an exception.
    pub fn is_available() -> bool {
        resolve().is_some()
    }

    /// C# `TaskDialogIndirect(config, &button, &radio, &verification)`: `Err(hr)` for a failing `HRESULT` (for example `E_INVALIDARG` when the struct is not
    /// 1-byte packed or `cbSize` is wrong, and `E_NOINTERFACE`-like failures when comctl32 v6 is not active). Blocks (modal message loop) until the dialog closes.
    ///
    /// # Safety
    /// Every pointer in `config` (`pszWindowTitle`, `pszMainInstruction`, `pszContent`, `pButtons`, `pRadioButtons`, ..., `pfCallback`, `lpCallbackData`) is valid
    /// for the whole call; a `MAKEINTRESOURCE` icon (`TD_*_ICON`) is the 16-bit value `(icon as u16) as usize`, not the sign-extended `isize`.
    pub unsafe fn task_dialog_indirect(config: &TaskDialogConfig) -> Result<TaskDialogResult, Hresult> {
        let Some(function) = resolve() else {
            return Err(Hresult(E_NOINTERFACE));
        };
        let (mut button, mut radio, mut verification) = (0i32, 0i32, BOOL(0));
        // SAFETY: upheld by the caller of this function for the pointers inside `config`; `config` is a live packed struct and the three out pointers are locals.
        let hr = unsafe { function(config, &mut button, &mut radio, &mut verification) };
        if hr.0 < 0 {
            Err(Hresult(hr.0))
        } else {
            Ok(TaskDialogResult {
                button,
                radio_button: radio,
                verification_checked: verification.as_bool(),
            })
        }
    }
}
