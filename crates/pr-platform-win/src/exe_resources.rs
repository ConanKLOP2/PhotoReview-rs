//! Test helper (feature `test-support`): reads the Win32 resources of a built exe WITHOUT running it, the Rust twin of the `ReadEmbeddedManifest` helper of the C# `ShellAssemblyTests`
//! (`LoadLibraryEx(.., LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE)`, `FindResource`, `LoadResource`, `SizeofResource`). Unit W8-20a. Contract: `contracts/W8-20a.md`.
//!
//! Not part of the production exe; `pr-shell`'s `tests/exe_resources.rs` uses it on `env!("CARGO_BIN_EXE_PhotoReviewRs")`. All `unsafe` of the exe checks is here (AGENTS.md).
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::*;
use windows::core::PCWSTR;

use std::io;
use std::path::Path;

/// `RT_GROUP_ICON` resource type id.
pub const RT_GROUP_ICON: u16 = 14;
/// `RT_MANIFEST` resource type id.
pub const RT_MANIFEST: u16 = 24;
/// `CREATEPROCESS_MANIFEST_RESOURCE_ID`: the id of the process manifest.
pub const CREATEPROCESS_MANIFEST_RESOURCE_ID: u16 = 1;

/// One image of an icon group (`GRPICONDIRENTRY`): the stored width and height in pixels (`0` in the file means 256, already mapped here) and the bit depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupIconImage {
    /// Width in pixels (1..=256).
    pub width: u32,
    /// Height in pixels (1..=256).
    pub height: u32,
    /// Bits per pixel as stored.
    pub bit_count: u16,
}

/// A module loaded as a data file (never executed). `Drop` is `FreeLibrary`.
#[derive(Debug)]
pub struct ExeResources {
    module: isize,
}

impl ExeResources {
    /// `LoadLibraryExW(path, 0, LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE)`; `Err(io::Error::last_os_error())` when it fails (message names the path).
    pub fn open(path: &Path) -> io::Result<ExeResources> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if wide[..wide.len() - 1].contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in exe path"));
        }
        // SAFETY: The path is NUL-terminated; data-file flags prevent executing the module.
        let module = unsafe {
            LoadLibraryExW(
                PCWSTR(wide.as_ptr()),
                None,
                LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
            )
        }
        .map_err(|error| io::Error::other(format!("{}: {error}", path.display())))?;
        Ok(Self {
            module: module.0 as isize,
        })
    }

    /// The `RT_MANIFEST` resource 1 as UTF-8 text (a UTF-8 BOM is stripped); `None` when the exe has no manifest resource.
    pub fn manifest_xml(&self) -> Option<String> {
        let bytes = self.resource(RT_MANIFEST, CREATEPROCESS_MANIFEST_RESOURCE_ID)?;
        String::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes).to_vec()).ok()
    }

    /// The images of the `RT_GROUP_ICON` group with numeric id `group_id` (parsed from `GRPICONDIR`: `idReserved` 0, `idType` 1, `idCount`, then 14-byte entries); `None` when the group does
    /// not exist or its header is malformed.
    pub fn group_icon_images(&self, group_id: u16) -> Option<Vec<GroupIconImage>> {
        let bytes = self.resource(RT_GROUP_ICON, group_id)?;
        parse_group_icon(&bytes)
    }
}

impl Drop for ExeResources {
    /// `FreeLibrary(module)`.
    fn drop(&mut self) {
        // SAFETY: This module was loaded by open and is released exactly once.
        let _ = unsafe { FreeLibrary(HMODULE(self.module as *mut _)) };
    }
}

impl ExeResources {
    fn resource(&self, kind: u16, id: u16) -> Option<Vec<u8>> {
        let module = HMODULE(self.module as *mut _);
        // SAFETY: Numeric resource identifiers use MAKEINTRESOURCE encoding. The module remains loaded throughout the copy.
        unsafe {
            let resource = FindResourceW(
                Some(module),
                PCWSTR(id as usize as *const u16),
                PCWSTR(kind as usize as *const u16),
            );
            if resource.0.is_null() {
                return None;
            }
            let size = SizeofResource(Some(module), resource) as usize;
            if size == 0 {
                return None;
            }
            let loaded = LoadResource(Some(module), resource).ok()?;
            let data = LockResource(loaded);
            if data.is_null() {
                return None;
            }
            Some(std::slice::from_raw_parts(data as *const u8, size).to_vec())
        }
    }
}

fn parse_group_icon(bytes: &[u8]) -> Option<Vec<GroupIconImage>> {
    let word = |offset: usize| Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?));
    if word(0)? != 0 || word(2)? != 1 {
        return None;
    }
    let count = usize::from(word(4)?);
    if count == 0 || bytes.len() < 6 + count * 14 {
        return None;
    }
    (0..count)
        .map(|index| {
            let offset = 6 + index * 14;
            let dimension = |value: u8| if value == 0 { 256 } else { u32::from(value) };
            Some(GroupIconImage {
                width: dimension(bytes[offset]),
                height: dimension(bytes[offset + 1]),
                bit_count: word(offset + 6)?,
            })
        })
        .collect()
}
