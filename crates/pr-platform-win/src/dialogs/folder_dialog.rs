//! Port of `PhotoReview.Shell.Win32/Dialogs/NativeShellFolderDialog.cs` (96 lines) and `FolderPicker.cs` (44 lines, baseline `d92d6aae`). Unit W8-18a.
//! Contract: `contracts/W8-18a.md`. The WPF `WpfFolderPicker` (23 lines) is retired.
//!
//! The native folder picker: `IFileOpenDialog` with `FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST`, modal on the STA UI thread of the owner window
//! (the dialog pumps its own messages). Cancel is `Ok(None)`; a real failure is `Err(Hresult)` (the C# `COMException`). `FolderPicker` holds the decision (the initial
//! folder is used only when it exists) so it is tested without a dialog.
#![allow(unused_variables, dead_code)] // stub: removed by the porter with the last `todo!()`

use std::rc::Rc;

use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, IFileOpenDialog,
};

use crate::win32::shell_native::HRESULT_ERROR_CANCELLED;
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
    fn show(&self, owner: Hwnd, title: &str, initial_folder: Option<&str>) -> Result<Option<String>, Hresult> {
        todo!("W8-18a: Ok(None)")
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
        todo!("W8-18a")
    }

    /// C# internal ctor `NativeShellFolderDialog(Action<IFileOpenDialog>? beforeShow)`: the test hook called after `configure` and right before `Show`.
    pub fn with_before_show(before_show: FolderBeforeShow) -> Self {
        todo!("W8-18a")
    }

    /// C# `Configure(dialog, title, initialFolder)`: `GetOptions`, `SetOptions(current | FOLDER_OPTIONS)`, `SetTitle` when `title` is not empty, `SetFolder` when
    /// `initial_folder` is not empty (via `shell_native::create_item_from_path`).
    pub fn configure(dialog: &IFileOpenDialog, title: &str, initial_folder: Option<&str>) -> Result<(), Hresult> {
        todo!("W8-18a")
    }
}

impl Default for NativeShellFolderDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellFolderDialog for NativeShellFolderDialog {
    fn show(&self, owner: Hwnd, title: &str, initial_folder: Option<&str>) -> Result<Option<String>, Hresult> {
        todo!(
            "W8-18a: create_file_open_dialog, configure, before_show, Show(owner), cancelled -> Ok(None), GetResult -> SIGDN_FILESYSPATH"
        )
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
        todo!("W8-18a")
    }

    /// C# `Pick(title, initialFolder)`: `dialog.show(owner(), title, usable_initial_folder(initial_folder))`.
    pub fn pick(&self, title: &str, initial_folder: Option<&str>) -> Result<Option<String>, Hresult> {
        todo!("W8-18a")
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
