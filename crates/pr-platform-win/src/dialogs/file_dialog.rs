//! Port of `PhotoReview.Shell.Win32/Dialogs/NativeShellFileDialog.cs` (181 lines, baseline `d92d6aae`). Unit W8-18a. Contract: `contracts/W8-18a.md`.
//!
//! WP-35 G1 seam: open-file and save-file dialogs (`IFileOpenDialog` / `IFileSaveDialog`), for Import/Export Settings and the JSON action list. Modal on the STA
//! UI thread of the owner window. Cancel is `Ok(None)`; a real failure is `Err(Hresult)` (the C# `COMException`).
#![allow(unused_variables, dead_code)] // stub: removed by the porter with the last `todo!()`

use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOREADONLYRETURN, FOS_OVERWRITEPROMPT,
    FOS_PATHMUSTEXIST, IFileDialog,
};

use crate::win32::structs::{Hresult, Hwnd};

/// C# `FileDialogFilter(Name, Pattern)`: a display name and a pattern (`*.json`, several patterns separated by `;`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDialogFilter {
    pub name: String,
    pub pattern: String,
}

/// C# `NativeShellFileDialog.OpenOptions`.
pub const OPEN_OPTIONS: FILEOPENDIALOGOPTIONS =
    FILEOPENDIALOGOPTIONS(FOS_FILEMUSTEXIST.0 | FOS_PATHMUSTEXIST.0 | FOS_FORCEFILESYSTEM.0);

/// C# `NativeShellFileDialog.SaveOptions`.
pub const SAVE_OPTIONS: FILEOPENDIALOGOPTIONS =
    FILEOPENDIALOGOPTIONS(FOS_OVERWRITEPROMPT.0 | FOS_PATHMUSTEXIST.0 | FOS_FORCEFILESYSTEM.0 | FOS_NOREADONLYRETURN.0);

/// C# `IShellFileDialog`.
pub trait ShellFileDialog {
    /// Picks an existing file; `Ok(None)` when cancelled.
    fn pick_open_file(
        &self,
        owner: Hwnd,
        title: &str,
        filters: &[FileDialogFilter],
        initial_folder: Option<&str>,
    ) -> Result<Option<String>, Hresult>;

    /// Picks a path to save to (asks before overwriting); `Ok(None)` when cancelled.
    fn pick_save_file(
        &self,
        owner: Hwnd,
        title: &str,
        filters: &[FileDialogFilter],
        default_extension: Option<&str>,
        suggested_file_name: Option<&str>,
        initial_folder: Option<&str>,
    ) -> Result<Option<String>, Hresult>;
}

/// The UTF-16 text of the filter strings, owned by Rust (the C# `Marshal.StringToCoTaskMemUni` + `FreeSpecs` pair has no Rust twin: dropping this value is the free).
/// `IFileDialog::SetFileTypes` keeps the pointers until the dialog is destroyed, so the value must outlive `Show`; the `Vec<u16>` heap blocks do not move when the
/// struct is moved.
pub struct FilterBuffers {
    names: Vec<Vec<u16>>,
    patterns: Vec<Vec<u16>>,
    specs: Vec<COMDLG_FILTERSPEC>,
}

impl FilterBuffers {
    /// The `COMDLG_FILTERSPEC` array that was passed to `SetFileTypes` (empty when there were no filters).
    pub fn specs(&self) -> &[COMDLG_FILTERSPEC] {
        &self.specs
    }
}

/// The test hook of the file dialogs: called with the configured dialog right before `Show`.
pub type FileBeforeShow = Box<dyn Fn(&IFileDialog)>;

/// C# `NativeShellFileDialog`.
pub struct NativeShellFileDialog {
    before_show: Option<FileBeforeShow>,
}

impl NativeShellFileDialog {
    /// C# `NativeShellFileDialog()`.
    pub fn new() -> Self {
        todo!("W8-18a")
    }

    /// C# internal ctor with `Action<IFileDialog>? beforeShow`: called after `configure`, right before `Show`.
    pub fn with_before_show(before_show: FileBeforeShow) -> Self {
        todo!("W8-18a")
    }

    /// C# `Configure`: `GetOptions`, `SetOptions(current | options)`; `SetTitle` when `title` is not empty; with filters `SetFileTypes` + `SetFileTypeIndex(1)`;
    /// `SetDefaultExtension(ext.trim_start_matches('.'))` when not empty; `SetFileName` when not empty; `SetFolder` (via `create_item_from_path`) when `initial_folder`
    /// is not empty. Returns the filter buffers the caller keeps alive until the dialog has closed.
    pub fn configure(
        dialog: &IFileDialog,
        title: &str,
        filters: &[FileDialogFilter],
        initial_folder: Option<&str>,
        options: FILEOPENDIALOGOPTIONS,
        default_extension: Option<&str>,
        suggested_file_name: Option<&str>,
    ) -> Result<FilterBuffers, Hresult> {
        todo!("W8-18a")
    }
}

impl Default for NativeShellFileDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellFileDialog for NativeShellFileDialog {
    fn pick_open_file(
        &self,
        owner: Hwnd,
        title: &str,
        filters: &[FileDialogFilter],
        initial_folder: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        todo!("W8-18a")
    }

    fn pick_save_file(
        &self,
        owner: Hwnd,
        title: &str,
        filters: &[FileDialogFilter],
        default_extension: Option<&str>,
        suggested_file_name: Option<&str>,
        initial_folder: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        todo!("W8-18a")
    }
}

impl std::fmt::Debug for FilterBuffers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterBuffers").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for NativeShellFileDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeShellFileDialog").finish_non_exhaustive()
    }
}
