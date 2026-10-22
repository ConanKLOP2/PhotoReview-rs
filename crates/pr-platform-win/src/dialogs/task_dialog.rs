//! Port of `PhotoReview.Shell.Win32/Dialogs/TaskDialogs.cs` (114 lines, baseline `d92d6aae`). Unit W8-18b (first of three files). Contract: `contracts/W8-18b.md`.
//!
//! Message boxes of the shell: `TaskDialogIndirect` (comctl32 v6 through the manifest, `win32::comctl::task_dialog`) with a Yes/No question, an information box and
//! an error box. The nested modal loop is the dialog's own, on the calling (UI) thread. `TaskDialogPort` is the seam the tests replace with a fake.
#![allow(unused_variables, dead_code)] // stub: removed by the porter with the last `todo!()`

use std::cell::RefCell;

use crate::win32::structs::Hwnd;

/// C# `TaskDialogKind`: decides the buttons and the icon (the same as `WpfDialogService`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDialogKind {
    /// Yes and No, no icon.
    Confirmation,
    /// OK and the information icon.
    Information,
    /// OK and the WARNING icon (the WPF `ShowError` used `MessageBoxImage.Warning`).
    Error,
}

/// C# `TdnCreated`: the `TDN_CREATED` notification (the dialog exists, is not shown yet).
pub const TDN_CREATED: u32 = 0;

/// The `dwCommonButtons` of a kind: `Confirmation` = `TDCBF_YES_BUTTON | TDCBF_NO_BUTTON`, the other two `TDCBF_OK_BUTTON`.
pub fn common_buttons(kind: TaskDialogKind) -> u32 {
    todo!("W8-18b")
}

/// The main icon of a kind as the 16-bit `MAKEINTRESOURCE` value: `Information` -> `TD_INFORMATION_ICON` (`0xFFFD`), `Error` -> `TD_WARNING_ICON` (`0xFFFF`),
/// `Confirmation` -> `0` (no icon). The C# `nint` constants `-3` / `-1` are NOT passed as they are (D-W8B3-2: see `win32::comctl::task_dialog::task_dialog_indirect`).
pub fn main_icon(kind: TaskDialogKind) -> u16 {
    todo!("W8-18b")
}

/// C# `ITaskDialogPort`: `Show` returns the id of the pressed button (`IDYES`, `IDNO`, `IDOK`, `IDCANCEL`); closing with X, Esc or Alt+F4 returns `IDCANCEL`.
pub trait TaskDialogPort {
    fn show(&self, owner: Hwnd, title: &str, message: &str, kind: TaskDialogKind) -> i32;
}

/// C# `NativeTaskDialogPort`.
pub struct NativeTaskDialogPort {
    on_created: Option<Box<dyn Fn(Hwnd)>>,
    last_failure: RefCell<Option<String>>,
}

impl NativeTaskDialogPort {
    /// C# `NativeTaskDialogPort()`.
    pub fn new() -> Self {
        todo!("W8-18b")
    }

    /// C# internal ctor `NativeTaskDialogPort(Action<nint>? onCreated)`: the hook receives the dialog's window as soon as it is created (a test closes it with `PostMessage`).
    pub fn with_on_created(on_created: Box<dyn Fn(Hwnd)>) -> Self {
        todo!("W8-18b")
    }

    /// C# `LastFailure`: why the last `show` could not show a dialog (`None` after a normal one): `"HRESULT 0x{hr:08X}"` or `"TaskDialogIndirect is missing (comctl32 v5)"`.
    pub fn last_failure(&self) -> Option<String> {
        todo!("W8-18b")
    }
}

impl Default for NativeTaskDialogPort {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskDialogPort for NativeTaskDialogPort {
    fn show(&self, owner: Hwnd, title: &str, message: &str, kind: TaskDialogKind) -> i32 {
        todo!(
            "W8-18b: build the TASKDIALOGCONFIG, TDF_ALLOW_DIALOG_CANCELLATION, callback, task_dialog_indirect; failure -> IDCANCEL + last_failure"
        )
    }
}

impl std::fmt::Debug for NativeTaskDialogPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTaskDialogPort").finish_non_exhaustive()
    }
}
