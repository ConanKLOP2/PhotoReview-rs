//! Port of `PhotoReview.Shell.Win32/Dialogs/NativeShellFolderDialog.cs` (96 lines) and `FolderPicker.cs` (44 lines, baseline `d92d6aae`). Unit W8-18a.
//! Contract: `contracts/W8-18a.md`. The WPF `WpfFolderPicker` (23 lines) is retired.
//!
//! The native folder picker: `IFileOpenDialog` with `FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST`, modal on the STA UI thread of the owner window
//! (the dialog pumps its own messages). Cancel is `Ok(None)`; a real failure is `Err(Hresult)` (the C# `COMException`). `FolderPicker` holds the decision (the initial
//! folder is used only when it exists) so it is tested without a dialog.
use std::{ffi::c_void, rc::Rc};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, IFileOpenDialog, SIGDN_FILESYSPATH,
};
use windows::core::PCWSTR;

use crate::win32::shell_native::{
    HRESULT_ERROR_CANCELLED, create_file_open_dialog, create_item_from_path, take_co_task_string,
};
use crate::win32::structs::{Hresult, Hwnd};

/// C# `NativeShellFolderDialog.HResultCancelled` (`HRESULT_FROM_WIN32(ERROR_CANCELLED)`): the same value as `shell_native::HRESULT_ERROR_CANCELLED`.
pub const HRESULT_CANCELLED: i32 = HRESULT_ERROR_CANCELLED;

/// C# `NativeShellFolderDialog.FolderOptions`.
pub const FOLDER_OPTIONS: FILEOPENDIALOGOPTIONS =
    FILEOPENDIALOGOPTIONS(FOS_PICKFOLDERS.0 | FOS_FORCEFILESYSTEM.0 | FOS_PATHMUSTEXIST.0);

/// C# `IShellFolderDialog`: the seam of the folder dialog.
pub trait ShellFolderDialog {
    /// Shows the modal dialog. `Ok(Some(path))` = the chosen folder, `Ok(None)` = cancelled, `Err` = a real COM failure.
    fn show(&self, owner: Hwnd, title: &str, initial_folder: Option<&str>) -> Result<Option<String>, Hresult>;
}

/// C# `UnwiredShellFolderDialog`: shows nothing and answers "cancelled".
#[derive(Debug, Clone, Copy, Default)]
pub struct UnwiredShellFolderDialog;

impl ShellFolderDialog for UnwiredShellFolderDialog {
    fn show(&self, _owner: Hwnd, _title: &str, _initial_folder: Option<&str>) -> Result<Option<String>, Hresult> {
        Ok(None)
    }
}

/// The test hook of the folder dialog: called with the configured dialog right before `Show`.
pub type FolderBeforeShow = Box<dyn Fn(&IFileOpenDialog)>;

/// C# `NativeShellFolderDialog`.
pub struct NativeShellFolderDialog {
    before_show: Option<FolderBeforeShow>,
}

impl NativeShellFolderDialog {
    /// C# `NativeShellFolderDialog()`.
    pub fn new() -> Self {
        Self { before_show: None }
    }

    /// C# internal ctor `NativeShellFolderDialog(Action<IFileOpenDialog>? beforeShow)`: the test hook called after `configure` and right before `Show`.
    pub fn with_before_show(before_show: FolderBeforeShow) -> Self {
        Self {
            before_show: Some(before_show),
        }
    }

    /// C# `Configure(dialog, title, initialFolder)`: `GetOptions`, `SetOptions(current | FOLDER_OPTIONS)`, `SetTitle` when `title` is not empty, `SetFolder` when
    /// `initial_folder` is not empty (via `shell_native::create_item_from_path`).
    pub fn configure(dialog: &IFileOpenDialog, title: &str, initial_folder: Option<&str>) -> Result<(), Hresult> {
        // SAFETY: `dialog` is a live COM interface reference owned by the caller.
        let current = unsafe { dialog.GetOptions() }.map_err(|error| Hresult(error.code().0))?;
        // SAFETY: `dialog` remains live and the options value is a valid bitmask.
        unsafe { dialog.SetOptions(current | FOLDER_OPTIONS) }.map_err(|error| Hresult(error.code().0))?;
        if !title.is_empty() {
            let title = title.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
            // SAFETY: `title` is NUL-terminated UTF-16 and stays alive for this call.
            unsafe { dialog.SetTitle(PCWSTR(title.as_ptr())) }.map_err(|error| Hresult(error.code().0))?;
        }
        if let Some(folder) = initial_folder.filter(|folder| !folder.is_empty()) {
            let item = create_item_from_path(folder)?;
            // SAFETY: `item` is a live shell item for the duration of this COM call.
            unsafe { dialog.SetFolder(&item) }.map_err(|error| Hresult(error.code().0))?;
        }
        Ok(())
    }
}

impl Default for NativeShellFolderDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellFolderDialog for NativeShellFolderDialog {
    fn show(&self, owner: Hwnd, title: &str, initial_folder: Option<&str>) -> Result<Option<String>, Hresult> {
        let dialog = create_file_open_dialog()?;
        Self::configure(&dialog, title, initial_folder)?;
        if let Some(before_show) = &self.before_show {
            before_show(&dialog);
        }
        let owner = if owner == 0 {
            None
        } else {
            Some(HWND(owner as *mut c_void))
        };
        // SAFETY: `dialog` is live and `owner` is null or a caller-supplied native window handle.
        if let Err(error) = unsafe { dialog.Show(owner) } {
            let code = error.code().0;
            return if code == HRESULT_CANCELLED {
                Ok(None)
            } else {
                Err(Hresult(code))
            };
        }
        // SAFETY: `dialog` remains live after Show and GetResult returns an owned shell interface.
        let item = unsafe { dialog.GetResult() }.map_err(|error| Hresult(error.code().0))?;
        // SAFETY: `item` is live; the returned PWSTR is owned by the shell allocator.
        let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.map_err(|error| Hresult(error.code().0))?;
        // SAFETY: The shell allocated this display-name PWSTR; take_co_task_string reads and frees it.
        Ok(unsafe { take_co_task_string(name.0) })
    }
}

/// C# `FolderPicker`: the decision (a usable initial folder) in front of an `IShellFolderDialog`.
pub struct FolderPicker {
    dialog: Rc<dyn ShellFolderDialog>,
    owner: Box<dyn Fn() -> Hwnd>,
}

impl FolderPicker {
    /// C# `FolderPicker(IShellFolderDialog dialog, Func<nint> ownerProvider)`.
    pub fn new(dialog: Rc<dyn ShellFolderDialog>, owner: Box<dyn Fn() -> Hwnd>) -> Self {
        Self { dialog, owner }
    }

    /// C# `UsableInitialFolder`: the folder only when it is not empty and exists on disk (`Directory.Exists`), else `None`. The returned text is the input, unchanged.
    pub fn usable_initial_folder(initial_folder: Option<&str>) -> Option<String> {
        initial_folder
            .filter(|folder| !folder.is_empty())
            .filter(|folder| std::fs::metadata(folder).is_ok_and(|metadata| metadata.is_dir()))
            .map(str::to_owned)
    }

    /// C# `Pick(title, initialFolder)`: `dialog.show(owner(), title, usable_initial_folder(initial_folder))`.
    pub fn pick(&self, title: &str, initial_folder: Option<&str>) -> Result<Option<String>, Hresult> {
        let usable = Self::usable_initial_folder(initial_folder);
        self.dialog.show((self.owner)(), title, usable.as_deref())
    }
}

impl std::fmt::Debug for NativeShellFolderDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeShellFolderDialog").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for FolderPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FolderPicker").finish_non_exhaustive()
    }
}
