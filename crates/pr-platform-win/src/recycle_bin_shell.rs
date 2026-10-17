//! COM adapter for the virtual Windows Recycle Bin.
//!
//! Every interface is created, used, and dropped on the STA worker that opens this session. This module never
//! modifies the bin except by invoking the restore verb on the single entry selected by `recycle_restore`.

use std::io;

use pr_core::time::DateTimeUtc;
use windows::Win32::Foundation::{FILETIME, PROPERTYKEY};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED, IBindCtx,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, BHID_SFUIObject, CMINVOKECOMMANDINFO, FOLDERID_RecycleBinFolder, IContextMenu,
    IEnumShellItems, IShellItem, IShellItem2, SHGetKnownFolderItem, SIGDN_PARENTRELATIVE,
};
use windows::core::{PCSTR, PWSTR};

use crate::recycle_candidate::RecycleCandidate;
use crate::recycle_restore::{
    RecycleRestoreEntry, RecycleRestoreItemError, RecycleRestoreItemFailureKind, RecycleRestoreSession,
};

const PKEY_RECYCLE_DELETED_FROM: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0x9b174b33_40ff_11d2_a27e_00c04fc30871),
    pid: 2,
};
const PKEY_DATE_MODIFIED: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0xb725f130_47ef_101a_a5f1_02608c9eebac),
    pid: 14,
};
const PKEY_SIZE: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0xb725f130_47ef_101a_a5f1_02608c9eebac),
    pid: 12,
};
const RESTORE_VERB: &[u8] = b"undelete\0";

struct ComApartment;

impl ComApartment {
    fn initialize() -> io::Result<Self> {
        // SAFETY: `restore_on_worker` creates a fresh dedicated thread; this is its only COM initialization call.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_ok() {
            Ok(Self)
        } else {
            Err(com_error("COM STA initialization failed", result.0))
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: this guard is constructed only after CoInitializeEx succeeded on this thread.
        unsafe { CoUninitialize() };
    }
}

struct ComRecycleBinSession {
    folder: IShellItem,
    _apartment: ComApartment,
}

pub(super) fn open_session() -> io::Result<Option<Box<dyn RecycleRestoreSession>>> {
    let apartment = ComApartment::initialize()?;
    // SAFETY: FOLDERID_RecycleBinFolder is a valid known-folder id; the returned interface is owned by this session.
    let folder = unsafe { SHGetKnownFolderItem::<IShellItem>(&FOLDERID_RecycleBinFolder, Default::default(), None) }
        .map_err(|error| com_error("Could not open the Recycle Bin shell folder", error.code().0))?;
    Ok(Some(Box::new(ComRecycleBinSession {
        _apartment: apartment,
        folder,
    })))
}

impl RecycleRestoreSession for ComRecycleBinSession {
    fn entries(&mut self) -> io::Result<Vec<Box<dyn RecycleRestoreEntry>>> {
        // SAFETY: the folder is a live IShellItem created in this thread; EnumItems returns an owned COM interface.
        let enumerator: IEnumShellItems = unsafe {
            self.folder
                .BindToHandler(None::<&IBindCtx>, &BHID_EnumItems)
        }
        .map_err(|error| com_error("Could not enumerate Recycle Bin items", error.code().0))?;

        let mut entries: Vec<Box<dyn RecycleRestoreEntry>> = Vec::new();
        loop {
            let mut next: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0_u32;
            // SAFETY: `next` has one writable slot, matching celt=1; fetched points to a live u32.
            let status = unsafe { enumerator.Next(&mut next, Some(&mut fetched)) };
            if let Err(error) = status {
                return Err(com_error("Could not read a Recycle Bin item", error.code().0));
            }
            if fetched == 0 || next[0].is_none() {
                break;
            }
            let item = next[0]
                .take()
                .ok_or_else(|| io::Error::other("Shell enumerator returned an empty item"))?;
            let item: IShellItem2 = item
                .cast()
                .map_err(|error| com_error("Recycle Bin item does not support IShellItem2", error.code().0))?;
            entries.push(Box::new(ComRecycleBinEntry { item }));
        }
        Ok(entries)
    }
}

struct ComRecycleBinEntry {
    item: IShellItem2,
}

impl RecycleRestoreEntry for ComRecycleBinEntry {
    fn candidate(&self) -> Result<Option<RecycleCandidate>, RecycleRestoreItemError> {
        // System.Recycle.DeletedFrom is optional for Shell items.
        let deleted_from = match get_shell_string(&self.item, &PKEY_RECYCLE_DELETED_FROM) {
            Ok(value) => value,
            Err(_) => None,
        };

        // SAFETY: `item` is a live IShellItem2; GetDisplayName allocates a CoTaskMem string, released below.
        let display_name = unsafe { self.item.GetDisplayName(SIGDN_PARENTRELATIVE) }
            .map_err(|error| per_item_com_error("Could not read Recycle Bin item name", error.code().0))?;
        let name = take_pwstr(display_name)
            .map_err(|error| per_item_error(RecycleRestoreItemFailureKind::Format, error))?;

        // SAFETY: `item` is a live IShellItem2 and the property key is the documented System.Size key.
        let size = match unsafe { self.item.GetUInt64(&PKEY_SIZE) } {
            Ok(size) => size,
            Err(_) => return Ok(None),
        };
        let Ok(size) = i64::try_from(size) else {
            return Ok(None);
        };

        // SAFETY: `item` is a live IShellItem2 and the property key is the documented System.DateModified key.
        let FILETIME { dwLowDateTime, dwHighDateTime } = match unsafe { self.item.GetFileTime(&PKEY_DATE_MODIFIED) } {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let filetime = (u64::from(dwHighDateTime) << 32) | u64::from(dwLowDateTime);
        if filetime == 0 {
            return Ok(None);
        }
        let Some(modified_utc) = DateTimeUtc::from_filetime(filetime) else {
            return Ok(None);
        };
        Ok(Some(RecycleCandidate::new(
            deleted_from,
            name,
            size,
            modified_utc,
            None,
        )))
    }

    fn invoke_restore(&self) -> io::Result<()> {
        // SAFETY: this live Shell item binds to its own context menu; the returned COM reference is owned locally.
        let menu: IContextMenu = unsafe {
            self.item
                .BindToHandler(None::<&IBindCtx>, &BHID_SFUIObject)
        }
        .map_err(|error| com_error("Could not open Recycle Bin item context menu", error.code().0))?;

        let command = CMINVOKECOMMANDINFO {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
            hwnd: Default::default(),
            lpVerb: PCSTR(RESTORE_VERB.as_ptr()),
            lpParameters: PCSTR::null(),
            lpDirectory: PCSTR::null(),
            nShow: 1,
            ..Default::default()
        };
        // SAFETY: command and its NUL-terminated canonical verb live through the synchronous InvokeCommand call.
        unsafe { menu.InvokeCommand(&command) }
            .map_err(|error| com_error("Could not invoke the Recycle Bin restore command", error.code().0))
    }
}

fn get_shell_string(item: &IShellItem2, key: &PROPERTYKEY) -> io::Result<Option<String>> {
    // SAFETY: item and property key are valid; a returned PWSTR is owned and freed by `take_pwstr`.
    let value = unsafe { item.GetString(key) }
        .map_err(|error| com_error("Could not read Recycle Bin string property", error.code().0))?;
    take_pwstr(value).map(Some)
}

fn take_pwstr(value: PWSTR) -> io::Result<String> {
    if value.is_null() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Shell returned a null string"));
    }
    // SAFETY: Shell GetDisplayName/GetString return a NUL-terminated CoTaskMem string; conversion happens before free.
    let text = unsafe { value.to_string() }.map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Shell string is invalid UTF-16"));
    // SAFETY: both APIs transfer the PWSTR allocation to the caller using the COM task allocator.
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    text
}

fn per_item_com_error(message: &str, hresult: i32) -> RecycleRestoreItemError {
    per_item_error(RecycleRestoreItemFailureKind::Com, com_error(message, hresult))
}

fn per_item_error(kind: RecycleRestoreItemFailureKind, source: io::Error) -> RecycleRestoreItemError {
    RecycleRestoreItemError { kind, source }
}

fn com_error(message: &str, hresult: i32) -> io::Error {
    io::Error::other(format!("{message} (HRESULT {hresult:#010x})"))
}
