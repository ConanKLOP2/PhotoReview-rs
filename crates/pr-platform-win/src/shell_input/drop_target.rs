//! Port of `PhotoReview.Shell.Win32/Input/DropTarget.cs` (249 lines, baseline `d92d6aae`): `DropData` and `ShellDropTarget`. Unit W8-14d.
//! Contract: `contracts/W8-14d.md`.
//!
//! WP-22: drag-and-drop of files onto the window (WPF `AllowDrop` + `Window_PreviewDragOver`/`Window_Drop`) with a real OLE `IDropTarget`: a
//! drag that carries files shows the Copy effect, anything else None; a drop opens the FIRST file. The COM object is a `#[implement(IDropTarget)]`
//! struct (the C# `[GeneratedComClass]`); `RegisterDragDrop` AddRefs it until `RevokeDragDrop` (`dispose`). The Shell COM interfaces themselves are the
//! `windows` crate's (D-W8B3-10); the signatures below are the ones pinned by the W8-11e recorder test.
//!
//! Panic rule: a panic must not unwind out of a COM method (Rust aborts). Every method runs under `host::pending_panic::guarded` and answers
//! `E_FAIL`; the message loop resumes the panic after `DispatchMessage` (D-W8B3H-4).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::Cell;
use std::error::Error;
use std::fmt;

use windows::Win32::Foundation::POINTL;
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT, IDropTarget, IDropTarget_Impl};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows_core::implement;

use crate::win32::structs::{Hresult, Hwnd};

/// C# `DropData.HasFiles(data)`: `IDataObject::QueryGetData` of `CF_HDROP` on `TYMED_HGLOBAL` (aspect content, index -1) is `S_OK`.
pub fn has_files(data: &IDataObject) -> bool {
    todo!()
}

/// C# `DropData.Files(data)`: the paths of the `CF_HDROP` list in data-object order; empty when the format is missing, `GetData` fails, the medium is
/// not an `HGLOBAL` (or its handle is 0), or the lock fails. The medium is always released (`ReleaseStgMedium`), the lock always undone.
pub fn files(data: &IDataObject) -> Vec<String> {
    todo!()
}

/// C# `DropData.Parse(byte* block)` over a slice (the Rust form is bounded by `GlobalSize`): `DROPFILES.pFiles` (`u32` at byte 0) is the offset of the
/// list, `fWide` (`i32` at byte 16) says UTF-16 or ANSI (system code page); the list ends at the double NUL. A block shorter than the 20-byte header,
/// an offset beyond the end or an item that runs past the end stops the list (hostile data must not read out of bounds; the C# pointer walk had no bound).
pub fn parse_drop_files(block: &[u8]) -> Vec<String> {
    todo!()
}

/// A reader of the file list that can fail (the C# `Func<IDataObject, IReadOnlyList<string>>` that may throw `COMException`/`InvalidOperationException`).
pub type FilesReader = Box<dyn Fn(&IDataObject) -> Result<Vec<String>, Box<dyn Error>>>;
/// A reader of "does the drag carry files" that can fail; a failure counts as `false` (C# `SafeHasFiles`).
pub type HasFilesReader = Box<dyn Fn(&IDataObject) -> Result<bool, Box<dyn Error>>>;
/// Opens one dropped path. Called inline from `Drop` on the UI thread inside OLE's callback: it must return quickly (the main window posts the real
/// work). An `Err` is reported through [`DropCallbacks::on_failure`] (C# `OpenFirstAsync` catch).
pub type OpenPath = Box<dyn Fn(&str) -> Result<(), Box<dyn Error>>>;
/// Reports a failure (C# `Action<Exception> onFailure`).
pub type OnFailure = Box<dyn Fn(&dyn Error)>;

/// The constructor arguments of `ShellDropTarget` (C# `openPath`, `onFailure`, `files`, `hasFiles`).
pub struct DropCallbacks {
    /// C# `_openPath`.
    pub open_path: OpenPath,
    /// C# `_onFailure`.
    pub on_failure: OnFailure,
    /// C# `_files`; `None` = [`files`] wrapped in `Ok`.
    pub files: Option<FilesReader>,
    /// C# `_hasFiles`; `None` = [`has_files`] wrapped in `Ok`.
    pub has_files: Option<HasFilesReader>,
}

impl fmt::Debug for DropCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DropCallbacks").finish_non_exhaustive()
    }
}

/// Why [`ShellDropTarget::register`] did not register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropRegisterError {
    /// C# `InvalidOperationException("The drop target is already registered.")`.
    AlreadyRegistered,
    /// `RegisterDragDrop` refused (C# `Register` returned `false`); drag and drop is simply unavailable.
    Refused(Hresult),
}

impl fmt::Display for DropRegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRegistered => write!(f, "The drop target is already registered."),
            Self::Refused(hresult) => write!(f, "RegisterDragDrop failed: {hresult}"),
        }
    }
}

impl Error for DropRegisterError {}

/// The COM object. `drag_has_files` is C# `_dragHasFiles`; the callbacks are called without any borrow held (they may re-enter the window).
#[implement(IDropTarget)]
struct DropTargetObject {
    drag_has_files: Cell<bool>,
    callbacks: DropCallbacks,
}

#[allow(non_snake_case)] // the trait methods carry the COM names
impl IDropTarget_Impl for DropTargetObject_Impl {
    /// C# `DragEnter`: `drag_has_files = data non-null && safe_has_files(data)`; `*effect = Copy` if so, else `None`; `S_OK`.
    fn DragEnter(
        &self,
        pdataobj: windows_core::Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        todo!()
    }

    /// C# `DragOver`: `*effect = Copy` while `drag_has_files`, else `None`.
    fn DragOver(
        &self,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        todo!()
    }

    /// C# `DragLeave`: `drag_has_files = false`.
    fn DragLeave(&self) -> windows_core::Result<()> {
        todo!()
    }

    /// C# `Drop`: `drag_has_files = false`, `*effect = None`; read the files (a reader `Err` goes to `on_failure`, effect stays `None`); no files:
    /// nothing; else `*effect = Copy` and the FIRST path is opened (an `Err` of the open goes to `on_failure`). Always `S_OK`.
    fn Drop(
        &self,
        pdataobj: windows_core::Ref<IDataObject>,
        grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        todo!()
    }
}

/// C# `ShellDropTarget`: registration of the COM object on a window handle.
pub struct ShellDropTarget {
    object: IDropTarget,
    hwnd: Cell<Hwnd>,
}

impl ShellDropTarget {
    /// C# constructor: builds the COM object (not registered yet).
    pub fn new(callbacks: DropCallbacks) -> Self {
        let object: IDropTarget = DropTargetObject {
            drag_has_files: Cell::new(false),
            callbacks,
        }
        .into();
        Self {
            object,
            hwnd: Cell::new(0),
        }
    }

    /// C# `Register(hwnd)`: `OleInitialize` on this (STA) thread (result ignored: `S_FALSE`, `RPC_E_CHANGED_MODE`), then `RegisterDragDrop(hwnd, object)`.
    /// Never calls `OleUninitialize` (the thread's OLE lives to the end of the process; see the C# comment).
    pub fn register(&self, hwnd: Hwnd) -> Result<(), DropRegisterError> {
        todo!()
    }

    /// C# `Dispose()`: `RevokeDragDrop` when registered (a failure means the window is gone: nothing to revoke); idempotent.
    pub fn dispose(&self) {
        todo!()
    }

    /// True between a successful `register` and `dispose`.
    pub fn is_registered(&self) -> bool {
        self.hwnd.get() != 0
    }

    /// The COM interface (tests call `Drop` through it; the OS gets its own reference from `RegisterDragDrop`).
    pub fn interface(&self) -> &IDropTarget {
        &self.object
    }
}

impl Drop for ShellDropTarget {
    /// Revokes the registration if it is still active: OLE holds a reference to the object (and so to the callbacks) until `RevokeDragDrop`.
    fn drop(&mut self) {
        self.dispose();
    }
}

impl fmt::Debug for ShellDropTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellDropTarget")
            .field("hwnd", &self.hwnd.get())
            .finish_non_exhaustive()
    }
}
