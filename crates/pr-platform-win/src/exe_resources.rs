//! Test helper (feature `test-support`): reads the Win32 resources of a built exe WITHOUT running it, the Rust twin of the `ReadEmbeddedManifest` helper of the C# `ShellAssemblyTests`
//! (`LoadLibraryEx(.., LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE)`, `FindResource`, `LoadResource`, `SizeofResource`). Unit W8-20a. Contract: `contracts/W8-20a.md`.
//!
//! Not part of the production exe; `pr-shell`'s `tests/exe_resources.rs` uses it on `env!("CARGO_BIN_EXE_PhotoReviewRs")`. All `unsafe` of the exe checks is here (AGENTS.md).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

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
        todo!()
    }

    /// The `RT_MANIFEST` resource 1 as UTF-8 text (a UTF-8 BOM is stripped); `None` when the exe has no manifest resource.
    pub fn manifest_xml(&self) -> Option<String> {
        todo!()
    }

    /// The images of the `RT_GROUP_ICON` group with numeric id `group_id` (parsed from `GRPICONDIR`: `idReserved` 0, `idType` 1, `idCount`, then 14-byte entries); `None` when the group does
    /// not exist or its header is malformed.
    pub fn group_icon_images(&self, group_id: u16) -> Option<Vec<GroupIconImage>> {
        todo!()
    }
}

impl Drop for ExeResources {
    /// `FreeLibrary(module)`.
    fn drop(&mut self) {
        todo!()
    }
}
