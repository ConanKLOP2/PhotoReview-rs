//! Port of `PhotoReview.Shell.Interop/Shell/{IDataObject, IDropTarget, IFileOpenDialog}.cs` (baseline `d92d6aae`; 81 + 38 + 222 lines): the Vista file dialogs
//! (`IFileOpenDialog`/`IFileSaveDialog`, `IShellItem`, `IShellItemArray`), the OLE `IDataObject` and `IDropTarget`, the shell GUIDs/flags and the `ShellNative` creation
//! helpers.
//!
//! Unit W8-11e. The COM interfaces and enums come from the `windows` crate (D-W8B3-10): the C# hand-written vtables (`[GeneratedComInterface]`, `ReservedNN` slot
//! fillers, `ComInterop.Check/Wrap/Release`) have no Rust counterpart; `contracts/W8-11e.md` holds the audit table (C# member -> windows-rs item). What is ported here
//! is `ShellNative` and the few constants the Rust callers still spell out. `ComInterop` itself belongs to W8-11f.
//!
//! COM must be initialised on the calling thread (the shell UI thread is an STA); every function returns the failing `HRESULT` as `Hresult`.
#![allow(unused_variables, dead_code)] // stub: removed by the W8-11e porter when the last `todo!()` is gone.

use windows::Win32::System::Com::{IDataObject, STGMEDIUM};
use windows::Win32::UI::Shell::{IFileOpenDialog, IFileSaveDialog, IShellItem};

use super::structs::Hresult;

pub const CF_HDROP: u16 = 0x000F; // HDrop
pub const CF_UNICODETEXT: u16 = 0x000D; // UnicodeText
pub const DVASPECT_CONTENT: u32 = 1; // Content
pub const DATADIR_GET: u32 = 1; // Get
pub const DATADIR_SET: u32 = 2; // Set
pub const DV_E_FORMATETC: i32 = -2147221404; // DvEFormatEtc
pub const DROPEFFECT_NONE: i32 = 0; // DragDropSEffectNone

/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)` = `0x800704C7`: what `IModalWindow::Show` returns when the user cancels (C# `IModalWindow.Show` doc).
pub const HRESULT_ERROR_CANCELLED: i32 = -2147023673;

/// C# `ShellNative.CreateItemFromPath(path)`: `SHCreateItemFromParsingName(path, null, IID_IShellItem)`.
pub fn create_item_from_path(path: &str) -> Result<IShellItem, Hresult> {
    todo!()
}

/// C# `ShellNative.CreateFileOpenDialog()`: `CoCreateInstance(CLSID_FileOpenDialog, null, CLSCTX_INPROC_SERVER, IID_IFileOpenDialog)`.
pub fn create_file_open_dialog() -> Result<IFileOpenDialog, Hresult> {
    todo!()
}

/// C# `ShellNative.CreateFileSaveDialog()`: the same for `CLSID_FileSaveDialog`.
pub fn create_file_save_dialog() -> Result<IFileSaveDialog, Hresult> {
    todo!()
}

/// C# `ShellNative.CreateEmptyDataObject()`: `SHCreateDataObject(null, 0, null, null, IID_IDataObject)`, the shell's empty data object (any `SetData`/`GetData`).
pub fn create_empty_data_object() -> Result<IDataObject, Hresult> {
    todo!()
}

/// C# `ShellNative.TakeCoTaskString(pwstr)`: reads a `PWSTR` the shell allocated with `CoTaskMemAlloc` (`IShellItem::GetDisplayName`, `IFileDialog::GetFileName`),
/// then frees it. `None` for a null pointer.
///
/// # Safety
/// `pwstr` is null or a NUL-terminated UTF-16 string allocated with `CoTaskMemAlloc` that nobody else frees; it is invalid after this call.
pub unsafe fn take_co_task_string(pwstr: *mut u16) -> Option<String> {
    todo!()
}

/// C# `OleNative.ReleaseStgMedium(StgMedium*)`: frees what `IDataObject::GetData` returned (after this the medium's handle is invalid).
///
/// # Safety
/// `medium` is a medium filled by `GetData` (or built by the caller with a release method that matches its `tymed`) and is released exactly once.
pub unsafe fn release_stg_medium(medium: &mut STGMEDIUM) {
    todo!()
}
