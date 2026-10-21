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

use std::cell::Cell;
use std::error::Error;
use std::fmt;
use std::panic::AssertUnwindSafe;

use windows::Win32::Foundation::{E_FAIL, HGLOBAL, POINTL};
use windows::Win32::Globalization::{CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, TYMED_HGLOBAL};
use windows::Win32::System::Memory::GlobalSize;
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows_core::implement;

use crate::host::pending_panic::guarded;
use crate::win32::api::{kernel32, ole32};
use crate::win32::shell_native::{CF_HDROP, release_stg_medium};
use crate::win32::structs::{Hresult, Hwnd};

/// Size of the `DROPFILES` header (`pFiles` u32 at 0, `pt` 8 bytes at 4, `fNC` at 12, `fWide` at 16).
const DROPFILES_HEADER: usize = 20;
/// Byte offset of `DROPFILES.fWide`.
const F_WIDE_OFFSET: usize = 16;

/// C# `DropData.HDropFormat()`: `CF_HDROP`, no target device, content aspect, index -1, `TYMED_HGLOBAL`.
fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// Releases a `GetData` medium exactly once on every path (C# `finally { ReleaseStgMedium }`).
struct MediumGuard(STGMEDIUM);

impl Drop for MediumGuard {
    fn drop(&mut self) {
        // SAFETY: the medium was filled by `IDataObject::GetData` and this guard is its only owner, so it is released exactly once.
        unsafe { release_stg_medium(&mut self.0) };
    }
}

/// Unlocks a locked `HGLOBAL` on every path (C# `finally { GlobalUnlock }`).
struct LockGuard(isize);

impl Drop for LockGuard {
    fn drop(&mut self) {
        kernel32::global_unlock(self.0);
    }
}

/// C# `DropData.HasFiles(data)`: `IDataObject::QueryGetData` of `CF_HDROP` on `TYMED_HGLOBAL` (aspect content, index -1) is `S_OK`.
pub fn has_files(data: &IDataObject) -> bool {
    let format = hdrop_format();
    // SAFETY: `data` is a live `IDataObject` borrowed by the caller for the call; `format` is a local that outlives it.
    unsafe { data.QueryGetData(&format) }.0 == 0
}

/// C# `DropData.Files(data)`: the paths of the `CF_HDROP` list in data-object order; empty when the format is missing, `GetData` fails, the medium is
/// not an `HGLOBAL` (or its handle is 0), or the lock fails. The medium is always released (`ReleaseStgMedium`), the lock always undone.
pub fn files(data: &IDataObject) -> Vec<String> {
    let format = hdrop_format();
    // SAFETY: `data` is a live `IDataObject` borrowed by the caller for the call; `format` is a local that outlives it.
    let Ok(medium) = (unsafe { data.GetData(&format) }) else {
        return Vec::new();
    };
    let medium = MediumGuard(medium);
    if medium.0.tymed != TYMED_HGLOBAL.0 as u32 {
        return Vec::new();
    }
    // SAFETY: `tymed == TYMED_HGLOBAL` was checked, so `hGlobal` is the active union member.
    let handle: HGLOBAL = unsafe { medium.0.u.hGlobal };
    if handle.0.is_null() {
        return Vec::new();
    }
    let Ok(ptr) = kernel32::global_lock(handle.0 as isize) else {
        return Vec::new();
    };
    let _unlock = LockGuard(handle.0 as isize);
    // SAFETY: `handle` is a live global-memory handle (the medium owns it until the guard drops).
    let size = unsafe { GlobalSize(handle) };
    if size == 0 || ptr.is_null() {
        return Vec::new();
    }
    // SAFETY: the block is locked (`global_lock`) and `GlobalSize` bytes long; it is unlocked and released only after the slice is dropped
    // (`_unlock` and `medium` drop at the end of this function, after `parse_drop_files` returned owned strings).
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>().cast_const(), size) };
    parse_drop_files(bytes)
}

/// Decodes ANSI bytes with the system code page (`MultiByteToWideChar(CP_ACP)`), never panics.
fn decode_ansi(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    // SAFETY: sizing call (no output buffer); `bytes` is a live slice.
    let needed = unsafe { MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None) };
    let Ok(needed) = usize::try_from(needed) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    if needed == 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; needed];
    // SAFETY: the output buffer has exactly the length the first call asked for; `bytes` is a live slice.
    let written = unsafe { MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, Some(&mut wide)) };
    let written = usize::try_from(written).unwrap_or(0).min(wide.len());
    String::from_utf16_lossy(wide.get(..written).unwrap_or(&[]))
}

/// C# `DropData.Parse(byte* block)` over a slice (the Rust form is bounded by `GlobalSize`): `DROPFILES.pFiles` (`u32` at byte 0) is the offset of the
/// list, `fWide` (`i32` at byte 16) says UTF-16 or ANSI (system code page); the list ends at the double NUL. A block shorter than the 20-byte header,
/// an offset beyond the end or an item that runs past the end stops the list (hostile data must not read out of bounds; the C# pointer walk had no bound).
pub fn parse_drop_files(block: &[u8]) -> Vec<String> {
    if block.len() < DROPFILES_HEADER {
        return Vec::new();
    }
    let read4 = |at: usize| -> Option<[u8; 4]> { block.get(at..at.checked_add(4)?)?.try_into().ok() };
    let (Some(offset), Some(wide_flag)) = (read4(0), read4(F_WIDE_OFFSET)) else {
        return Vec::new();
    };
    let wide = i32::from_le_bytes(wide_flag) != 0;
    let Some(list) = usize::try_from(u32::from_le_bytes(offset))
        .ok()
        .and_then(|offset| block.get(offset..))
    else {
        return Vec::new();
    };
    let mut items = Vec::new();
    if wide {
        let units: Vec<u16> = list
            .chunks_exact(2)
            .filter_map(|pair| Some(u16::from_le_bytes([*pair.first()?, *pair.get(1)?])))
            .collect();
        let mut rest = units.as_slice();
        // An item without a NUL before the end stops the list (items before it are kept).
        while let Some(end) = rest.iter().position(|&unit| unit == 0) {
            let (item, tail) = rest.split_at(end);
            if item.is_empty() {
                break;
            }
            items.push(String::from_utf16_lossy(item));
            rest = tail.get(1..).unwrap_or(&[]);
        }
    } else {
        let mut rest = list;
        while let Some(end) = rest.iter().position(|&byte| byte == 0) {
            let (item, tail) = rest.split_at(end);
            if item.is_empty() {
                break;
            }
            items.push(decode_ansi(item));
            rest = tail.get(1..).unwrap_or(&[]);
        }
    }
    items
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

impl DropTargetObject {
    /// C# `SafeHasFiles`: a reader failure counts as "no files".
    fn safe_has_files(&self, data: Option<&IDataObject>) -> bool {
        let Some(data) = data else {
            return false;
        };
        match &self.callbacks.has_files {
            Some(reader) => reader(data).unwrap_or(false),
            None => has_files(data),
        }
    }

    /// The effect answer for the current drag state.
    fn current_effect(&self) -> DROPEFFECT {
        if self.drag_has_files.get() {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        }
    }

    /// C# `Drop` body (the COM method wraps it in the panic guard).
    fn drop_files(&self, data: Option<&IDataObject>, effect: *mut DROPEFFECT) {
        self.drag_has_files.set(false);
        write_effect(effect, DROPEFFECT_NONE);
        let files = match data {
            None => Vec::new(),
            Some(data) => {
                let read = match &self.callbacks.files {
                    Some(reader) => reader(data),
                    None => Ok(files(data)),
                };
                match read {
                    Ok(files) => files,
                    Err(error) => {
                        (self.callbacks.on_failure)(&*error);
                        return;
                    }
                }
            }
        };
        let Some(first) = files.first() else {
            return;
        };
        write_effect(effect, DROPEFFECT_COPY);
        if let Err(error) = (self.callbacks.open_path)(first) {
            (self.callbacks.on_failure)(&*error);
        }
    }
}

/// Writes the answer through OLE's `pdwEffect` (a null pointer is ignored).
fn write_effect(effect: *mut DROPEFFECT, value: DROPEFFECT) {
    if !effect.is_null() {
        // SAFETY: non-null checked; the caller (OLE) passes a live DWORD for the effect for the duration of the call.
        unsafe { effect.write(value) };
    }
}

/// Runs a COM method body under the panic boundary (D-W8B3H-4): a panic is captured for the message loop and the method answers `E_FAIL`.
fn com_guarded(body: impl FnOnce() -> windows_core::Result<()>) -> windows_core::Result<()> {
    guarded(Err(E_FAIL.into()), AssertUnwindSafe(body))
}

#[allow(non_snake_case)] // the trait methods carry the COM names
impl IDropTarget_Impl for DropTargetObject_Impl {
    /// C# `DragEnter`: `drag_has_files = data non-null && safe_has_files(data)`; `*effect = Copy` if so, else `None`; `S_OK`.
    fn DragEnter(
        &self,
        pdataobj: windows_core::Ref<IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        com_guarded(|| {
            self.drag_has_files.set(self.safe_has_files(pdataobj.as_ref()));
            write_effect(pdweffect, self.current_effect());
            Ok(())
        })
    }

    /// C# `DragOver`: `*effect = Copy` while `drag_has_files`, else `None`.
    fn DragOver(
        &self,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        com_guarded(|| {
            write_effect(pdweffect, self.current_effect());
            Ok(())
        })
    }

    /// C# `DragLeave`: `drag_has_files = false`.
    fn DragLeave(&self) -> windows_core::Result<()> {
        com_guarded(|| {
            self.drag_has_files.set(false);
            Ok(())
        })
    }

    /// C# `Drop`: `drag_has_files = false`, `*effect = None`; read the files (a reader `Err` goes to `on_failure`, effect stays `None`); no files:
    /// nothing; else `*effect = Copy` and the FIRST path is opened (an `Err` of the open goes to `on_failure`). Always `S_OK`.
    fn Drop(
        &self,
        pdataobj: windows_core::Ref<IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        com_guarded(|| {
            self.drop_files(pdataobj.as_ref(), pdweffect);
            Ok(())
        })
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
        if self.is_registered() {
            return Err(DropRegisterError::AlreadyRegistered);
        }
        // The result is ignored on purpose: `S_FALSE` (already initialised) and `RPC_E_CHANGED_MODE` are both fine for what follows.
        let _ = ole32::ole_initialize();
        ole32::register_drag_drop(hwnd, &self.object).map_err(DropRegisterError::Refused)?;
        self.hwnd.set(hwnd);
        Ok(())
    }

    /// C# `Dispose()`: `RevokeDragDrop` when registered (a failure means the window is gone: nothing to revoke); idempotent.
    pub fn dispose(&self) {
        let hwnd = self.hwnd.replace(0);
        if hwnd != 0 {
            // A failure means the window is already gone: nothing to revoke.
            let _ = ole32::revoke_drag_drop(hwnd);
        }
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
