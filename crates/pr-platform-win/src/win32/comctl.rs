//! Port of `PhotoReview.Shell.Interop/Win32/{CommonControlsNative, Comctl32}.cs` (baseline `d92d6aae`; 280 + 39 lines, the constants class `ControlConstants`
//! of the first file is `control_constants.rs`, `ActivationContext.cs` is `activation_context.rs`): the user32/gdi32/comctl32 calls the native dialog controls
//! need (combo box info, focus rectangle, class name, rounded rectangle, pens, common-control initialisation, window subclassing) and `TaskDialogIndirect`.
//!
//! Unit W8-11c. `TaskDialogIndirect` exists only in comctl32 v6, so it is bound at run time with `GetProcAddress` on the first use (D-W8B3-8): a static import
//! would stop every test executable without a manifest from starting (checked on this machine: `System32\comctl32.dll`, the v5.82 that a manifest-less process
//! loads, has 119 named exports and no `TaskDialogIndirect`; it does export `InitCommonControlsEx`, `SetWindowSubclass`, `RemoveWindowSubclass` and
//! `DefSubclassProc` by name, so those four are imported statically through the `windows` crate). `GetWindowSubclass` is also v6-only (the C# comment) and is not ported.
#![allow(unused_variables, dead_code)] // stub: removed by the W8-11c porter when the last `todo!()` is gone.

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

    /// C# `GetCursorPos(Point*)`: `None` when the call fails.
    pub fn get_cursor_pos() -> Option<Point> {
        todo!()
    }

    /// C# `GetComboBoxInfo(combo, void* info)` with a `COMBOBOXINFO` of `cbSize = 64` (`COMBOBOXINFO_SIZE`): `None` when the call fails. The windows-rs
    /// struct replaces the raw 64-byte buffer; `hwndList` is at offset 56 (`COMBOBOXINFO_HWNDLIST_OFFSET`).
    pub fn get_combo_box_info(combo: Hwnd) -> Option<windows::Win32::UI::Controls::COMBOBOXINFO> {
        todo!()
    }

    /// C# `DrawFocusRect`.
    pub fn draw_focus_rect(hdc: Hdc, rect: &Rect) -> bool {
        todo!()
    }

    /// C# `FillRect(hdc, rect, brush)`: non-zero on success.
    pub fn fill_rect(hdc: Hdc, rect: &Rect, brush: Hgdi) -> i32 {
        todo!()
    }

    /// C# `GetParent`: `0` when there is no parent (or on error).
    pub fn get_parent(hwnd: Hwnd) -> Hwnd {
        todo!()
    }

    /// C# `GetClassName(hwnd, char* buffer, maxCount)`: the class name, `None` on failure. The wrapper uses a 256 unit buffer (the longest class name is 256
    /// characters including the NUL).
    pub fn get_class_name(hwnd: Hwnd) -> Option<String> {
        todo!()
    }
}

/// `gdi32.dll` additions of `CommonControlsNative.cs` (the `Gdi32` partial).
pub mod gdi32 {
    use super::*;

    /// C# `SetBkMode(hdc, mode)` (`TRANSPARENT` = 1, `OPAQUE` = 2): the previous mode, `0` on failure.
    pub fn set_bk_mode(hdc: Hdc, mode: i32) -> i32 {
        todo!()
    }

    /// C# `RoundRect(hdc, l, t, r, b, ellipseWidth, ellipseHeight)`.
    pub fn round_rect(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32, width: i32, height: i32) -> bool {
        todo!()
    }

    /// C# `CreatePen(style, width, colorRef)`: `0` on failure. The caller deletes it with `DeleteObject` (W8-11d `gdi32::delete_object`).
    pub fn create_pen(style: i32, width: i32, color_ref: u32) -> Hgdi {
        todo!()
    }
}

/// C# `CommonControls` (comctl32.dll).
pub mod common_controls {
    use super::*;

    /// C# `InitCommonControlsEx(uint* controls)` with `{ dwSize = 8, dwICC = classes }` built by the wrapper (the C# callers build a `uint[2]`); `classes` is a
    /// combination of the `ICC_*` constants of `control_constants`. `true` on success.
    pub fn init_common_controls_ex(classes: u32) -> bool {
        todo!()
    }

    /// C# `SetWindowSubclass(hwnd, proc, id, refData)`: `true` on success. The subclass is identified by `(proc, id)`.
    ///
    /// # Safety
    /// `proc` must stay valid until `remove_window_subclass` or `WM_NCDESTROY`, and `ref_data` is passed to it as is.
    pub unsafe fn set_window_subclass(hwnd: Hwnd, proc: SubclassProc, id: usize, ref_data: usize) -> bool {
        todo!()
    }

    /// C# `RemoveWindowSubclass(hwnd, proc, id)`: `true` when a subclass was removed.
    pub fn remove_window_subclass(hwnd: Hwnd, proc: SubclassProc, id: usize) -> bool {
        todo!()
    }

    /// C# `DefSubclassProc`: calls the next procedure in the subclass chain.
    ///
    /// # Safety
    /// Call it only from a subclass procedure with the message parameters that procedure received.
    pub unsafe fn def_subclass_proc(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
        todo!()
    }
}

/// C# `Comctl32.TaskDialogIndirect` (comctl32 v6 only, resolved at run time: module docs).
pub mod task_dialog {
    use super::*;
    use crate::win32::structs::Hresult;

    /// `true` when `TaskDialogIndirect` can be resolved now, that is when comctl32 v6 is the one `LoadLibraryW("comctl32.dll")` returns for the current thread
    /// (the exe manifest, or an `ActivationScope` on this thread). Only successful lookups are cached: a failure outside an activation scope must not stop a later
    /// call inside one. New in Rust (D-W8B3-8): C# finds out by an exception.
    pub fn is_available() -> bool {
        todo!()
    }

    /// C# `TaskDialogIndirect(config, &button, &radio, &verification)`: `Err(hr)` for a failing `HRESULT` (for example `E_INVALIDARG` when the struct is not
    /// 1-byte packed or `cbSize` is wrong, and `E_NOINTERFACE`-like failures when comctl32 v6 is not active). Blocks (modal message loop) until the dialog closes.
    ///
    /// # Safety
    /// Every pointer in `config` (`pszWindowTitle`, `pszMainInstruction`, `pszContent`, `pButtons`, `pRadioButtons`, ..., `pfCallback`, `lpCallbackData`) is valid
    /// for the whole call; a `MAKEINTRESOURCE` icon (`TD_*_ICON`) is the 16-bit value `(icon as u16) as usize`, not the sign-extended `isize`.
    pub unsafe fn task_dialog_indirect(config: &TaskDialogConfig) -> Result<TaskDialogResult, Hresult> {
        todo!()
    }
}
