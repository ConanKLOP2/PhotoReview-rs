//! Port of `PhotoReview.Shell.Win32/Dialogs/NativeShellFileDialog.cs` (181 lines, baseline `d92d6aae`). Unit W8-18a. Contract: `contracts/W8-18a.md`.
//!
//! WP-35 G1 seam: open-file and save-file dialogs (`IFileOpenDialog` / `IFileSaveDialog`), for Import/Export Settings and the JSON action list. Modal on the STA
//! UI thread of the owner window. Cancel is `Ok(None)`; a real failure is `Err(Hresult)` (the C# `COMException`).
use std::ffi::c_void;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOREADONLYRETURN, FOS_OVERWRITEPROMPT,
    FOS_PATHMUSTEXIST, IFileDialog, SIGDN_FILESYSPATH,
};

use windows::Win32::Foundation::HWND;
use windows::core::PCWSTR;

use crate::win32::shell_native::{
    HRESULT_ERROR_CANCELLED, create_file_open_dialog, create_file_save_dialog, create_item_from_path,
    take_co_task_string,
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

struct FileDialogRequest<'a> {
    owner: Hwnd,
    title: &'a str,
    filters: &'a [FileDialogFilter],
    initial_folder: Option<&'a str>,
    options: FILEOPENDIALOGOPTIONS,
    default_extension: Option<&'a str>,
    suggested_file_name: Option<&'a str>,
}

impl NativeShellFileDialog {
    /// C# `NativeShellFileDialog()`.
    pub fn new() -> Self {
        Self { before_show: None }
    }

    /// C# internal ctor with `Action<IFileDialog>? beforeShow`: called after `configure`, right before `Show`.
    pub fn with_before_show(before_show: FileBeforeShow) -> Self {
        Self {
            before_show: Some(before_show),
        }
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
        // SAFETY: `dialog` is a live COM interface reference owned by the caller.
        let current = unsafe { dialog.GetOptions() }.map_err(|error| Hresult(error.code().0))?;
        // SAFETY: `dialog` remains live and `options` is a valid bitmask.
        unsafe { dialog.SetOptions(current | options) }.map_err(|error| Hresult(error.code().0))?;
        if !title.is_empty() {
            let title = title.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
            // SAFETY: `title` is NUL-terminated UTF-16 and stays alive for this call.
            unsafe { dialog.SetTitle(PCWSTR(title.as_ptr())) }.map_err(|error| Hresult(error.code().0))?;
        }

        let mut buffers = FilterBuffers {
            names: Vec::with_capacity(filters.len()),
            patterns: Vec::with_capacity(filters.len()),
            specs: Vec::with_capacity(filters.len()),
        };
        for filter in filters {
            buffers
                .names
                .push(filter.name.encode_utf16().chain(std::iter::once(0)).collect());
            buffers
                .patterns
                .push(filter.pattern.encode_utf16().chain(std::iter::once(0)).collect());
        }
        for (name, pattern) in buffers.names.iter().zip(&buffers.patterns) {
            buffers.specs.push(COMDLG_FILTERSPEC {
                pszName: PCWSTR(name.as_ptr()),
                pszSpec: PCWSTR(pattern.as_ptr()),
            });
        }
        if !buffers.specs.is_empty() {
            // SAFETY: specs point into buffers owned by `buffers`, which outlives Show.
            unsafe { dialog.SetFileTypes(&buffers.specs) }.map_err(|error| Hresult(error.code().0))?;
            // SAFETY: `dialog` is live and index 1 selects the first supplied filter.
            unsafe { dialog.SetFileTypeIndex(1) }.map_err(|error| Hresult(error.code().0))?;
        }
        if let Some(extension) = default_extension.filter(|extension| !extension.is_empty()) {
            let extension = extension.trim_start_matches('.');
            let extension = extension.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
            // SAFETY: `extension` is NUL-terminated UTF-16 and stays alive for this call.
            unsafe { dialog.SetDefaultExtension(PCWSTR(extension.as_ptr())) }
                .map_err(|error| Hresult(error.code().0))?;
        }
        if let Some(file_name) = suggested_file_name.filter(|file_name| !file_name.is_empty()) {
            let file_name = file_name.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
            // SAFETY: `file_name` is NUL-terminated UTF-16 and stays alive for this call.
            unsafe { dialog.SetFileName(PCWSTR(file_name.as_ptr())) }.map_err(|error| Hresult(error.code().0))?;
        }
        if let Some(folder) = initial_folder.filter(|folder| !folder.is_empty()) {
            let item = create_item_from_path(folder)?;
            // SAFETY: `item` is a live shell item for the duration of this COM call.
            unsafe { dialog.SetFolder(&item) }.map_err(|error| Hresult(error.code().0))?;
        }
        Ok(buffers)
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
        let dialog = create_file_open_dialog()?;
        self.run(
            &dialog,
            FileDialogRequest {
                owner,
                title,
                filters,
                initial_folder,
                options: OPEN_OPTIONS,
                default_extension: None,
                suggested_file_name: None,
            },
        )
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
        let dialog = create_file_save_dialog()?;
        self.run(
            &dialog,
            FileDialogRequest {
                owner,
                title,
                filters,
                initial_folder,
                options: SAVE_OPTIONS,
                default_extension,
                suggested_file_name,
            },
        )
    }
}

impl NativeShellFileDialog {
    fn run(&self, dialog: &IFileDialog, request: FileDialogRequest<'_>) -> Result<Option<String>, Hresult> {
        let _buffers = Self::configure(
            dialog,
            request.title,
            request.filters,
            request.initial_folder,
            request.options,
            request.default_extension,
            request.suggested_file_name,
        )?;
        if let Some(before_show) = &self.before_show {
            before_show(dialog);
        }
        let owner = if request.owner == 0 {
            None
        } else {
            Some(HWND(request.owner as *mut c_void))
        };
        // SAFETY: `dialog` is live and `owner` is null or a caller-supplied native window handle.
        if let Err(error) = unsafe { dialog.Show(owner) } {
            let code = error.code().0;
            return if code == HRESULT_ERROR_CANCELLED {
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
