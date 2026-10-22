//! Port of `PhotoReview.Shell.Win32/Dialogs/TaskDialogs.cs` (114 lines, baseline `d92d6aae`). Unit W8-18b (first of three files). Contract: `contracts/W8-18b.md`.
//!
//! Message boxes of the shell: `TaskDialogIndirect` (comctl32 v6 through the manifest, `win32::comctl::task_dialog`) with a Yes/No question, an information box and
//! an error box. The nested modal loop is the dialog's own, on the calling (UI) thread. `TaskDialogPort` is the seam the tests replace with a fake.
use core::ffi::c_void;
use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{TASKDIALOG_COMMON_BUTTON_FLAGS, TASKDIALOG_FLAGS, TASKDIALOG_NOTIFICATIONS};
use windows::core::{HRESULT, PCWSTR};

use crate::host::pending_panic;
use crate::win32::comctl::{self, TDF_ALLOW_DIALOG_CANCELLATION};
use crate::win32::messages::IDCANCEL;
use crate::win32::structs::{Hwnd, new_task_dialog_config, to_utf16z};

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
    match kind {
        TaskDialogKind::Confirmation => comctl::TDCBF_YES_BUTTON | comctl::TDCBF_NO_BUTTON,
        TaskDialogKind::Information | TaskDialogKind::Error => comctl::TDCBF_OK_BUTTON,
    }
}

/// The main icon of a kind as the 16-bit `MAKEINTRESOURCE` value: `Information` -> `TD_INFORMATION_ICON` (`0xFFFD`), `Error` -> `TD_WARNING_ICON` (`0xFFFF`),
/// `Confirmation` -> `0` (no icon). The C# `nint` constants `-3` / `-1` are NOT passed as they are (D-W8B3-2: see `win32::comctl::task_dialog::task_dialog_indirect`).
pub fn main_icon(kind: TaskDialogKind) -> u16 {
    match kind {
        TaskDialogKind::Confirmation => 0,
        TaskDialogKind::Information => 0xFFFD,
        TaskDialogKind::Error => 0xFFFF,
    }
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
        Self {
            on_created: None,
            last_failure: RefCell::new(None),
        }
    }

    /// C# internal ctor `NativeTaskDialogPort(Action<nint>? onCreated)`: the hook receives the dialog's window as soon as it is created (a test closes it with `PostMessage`).
    pub fn with_on_created(on_created: Box<dyn Fn(Hwnd)>) -> Self {
        Self {
            on_created: Some(on_created),
            last_failure: RefCell::new(None),
        }
    }

    /// C# `LastFailure`: why the last `show` could not show a dialog (`None` after a normal one): `"HRESULT 0x{hr:08X}"` or `"TaskDialogIndirect is missing (comctl32 v5)"`.
    pub fn last_failure(&self) -> Option<String> {
        self.last_failure.borrow().clone()
    }
}

impl Default for NativeTaskDialogPort {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskDialogPort for NativeTaskDialogPort {
    fn show(&self, owner: Hwnd, title: &str, message: &str, kind: TaskDialogKind) -> i32 {
        *self.last_failure.borrow_mut() = None;
        if !comctl::task_dialog::is_available() {
            *self.last_failure.borrow_mut() = Some("TaskDialogIndirect is missing (comctl32 v5)".to_owned());
            return IDCANCEL;
        }

        let title_buffer = to_utf16z(title);
        let message_buffer = to_utf16z(message);
        let mut config = new_task_dialog_config();
        config.hwndParent = HWND(owner as *mut c_void);
        config.dwFlags = TASKDIALOG_FLAGS(TDF_ALLOW_DIALOG_CANCELLATION as i32);
        config.dwCommonButtons = TASKDIALOG_COMMON_BUTTON_FLAGS(common_buttons(kind) as i32);
        config.pszWindowTitle = PCWSTR(title_buffer.as_ptr());
        config.pszContent = PCWSTR(message_buffer.as_ptr());
        config.Anonymous1.pszMainIcon = PCWSTR(main_icon(kind) as usize as *const u16);
        config.pfCallback = Some(on_callback);
        config.lpCallbackData = (self as *const Self) as isize;

        // SAFETY: every pointer in `config` (title, content, callback, callback data) points into locals or into `self`, all alive until the call returns; the icon is the 16-bit resource value.
        let result = unsafe { comctl::task_dialog::task_dialog_indirect(&config) };
        pending_panic::rethrow_pending();
        match result {
            Ok(result) => result.button,
            Err(error) => {
                *self.last_failure.borrow_mut() = Some(error.to_string());
                IDCANCEL
            }
        }
    }
}

unsafe extern "system" fn on_callback(
    hwnd: HWND,
    notification: TASKDIALOG_NOTIFICATIONS,
    _wparam: WPARAM,
    _lparam: LPARAM,
    data: isize,
) -> HRESULT {
    if notification.0 == TDN_CREATED as i32 && data != 0 {
        // SAFETY: `data` is the address of the NativeTaskDialogPort that `show` passed; `show` is still on the stack (the call is synchronous), so the reference is valid for the duration of the callback; only shared access.
        let port = unsafe { &*(data as *const NativeTaskDialogPort) };
        let _ = pending_panic::guarded(
            HRESULT(0),
            AssertUnwindSafe(|| {
                if let Some(hook) = &port.on_created {
                    hook(hwnd.0 as isize);
                }
                HRESULT(0)
            }),
        );
    }
    HRESULT(0)
}

impl std::fmt::Debug for NativeTaskDialogPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTaskDialogPort").finish_non_exhaustive()
    }
}
