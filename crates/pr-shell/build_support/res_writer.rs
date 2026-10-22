//! Pure writer of a Win32 `.res` resource file (the format `rc.exe` produces and `link.exe` / `lld-link` accept as an input). Used by `crates/pr-shell/build.rs` through
//! `#[path = "build_support/res_writer.rs"] mod res_writer;` and by the test `tests/build_res_writer.rs` through the same `#[path]`, so the build script is tested without running it.
//! Unit W8-20a. Contract: `contracts/W8-20a.md` (decision D-W8B6-42: no resource crate, one hand-written `.res`).
//!
//! What the exe needs (C# `PhotoReview.Shell.Win32.csproj`: `ApplicationManifest` + `ApplicationIcon`): the process manifest (`RT_MANIFEST` id 1, the `shell.manifest` of `pr-platform-win`, which
//! declares PerMonitorV2 and Common-Controls v6) and the application icon as an `RT_GROUP_ICON` with the id `32512` (`IDI_APPLICATION`, which `ShellIcons::load` reads, D-W8B3H-16) plus its
//! `RT_ICON` images. No `VERSIONINFO` (open question 3 of the contract).
//!
//! Pure: no file system, no environment, no `unsafe`. This file must compile in a build script, so it uses `std` only.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

/// `RT_ICON`.
pub const RT_ICON: u16 = 3;
/// `RT_GROUP_ICON`.
pub const RT_GROUP_ICON: u16 = 14;
/// `RT_MANIFEST`.
pub const RT_MANIFEST: u16 = 24;
/// The id of the process manifest (`CREATEPROCESS_MANIFEST_RESOURCE_ID`).
pub const PROCESS_MANIFEST_ID: u16 = 1;
/// The id of the application icon group: `IDI_APPLICATION` = 32512 (`MAKEINTRESOURCE(32512)`).
pub const APP_ICON_GROUP_ID: u16 = 32512;
/// `LANG_NEUTRAL` / `SUBLANG_NEUTRAL`: the language id of every resource written here (`0`).
pub const LANGUAGE_NEUTRAL: u16 = 0;

/// Why a `.res` could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResError {
    /// The icon bytes are not an `.ico` file: the header is not `reserved = 0, type = 1`, or `count = 0`.
    NotAnIco(String),
    /// A directory entry points outside the file, or the file is shorter than its directory says.
    Truncated,
    /// More than 65535 images (the count is a `u16`).
    TooManyImages,
}

/// Builds the bytes of a `.res` file, in this order (every resource header is 4-byte aligned and every data block is padded with zeros to a multiple of 4):
///
/// 1. The mandatory empty first resource (`DataSize` 0, `HeaderSize` 32, type id 0, name id 0, all other fields 0).
/// 2. `RT_MANIFEST` id [`PROCESS_MANIFEST_ID`], language [`LANGUAGE_NEUTRAL`], the UTF-8 bytes of `manifest_xml` exactly as given (no BOM added).
/// 3. When `ico` is `Some`: one `RT_ICON` resource per image of the `.ico` directory, ids `1..=count` in directory order, data = the image bytes (`bytesInRes` bytes at `imageOffset`, copied as
///    they are: BMP DIBs and PNG-compressed images alike), then ONE `RT_GROUP_ICON` resource with id [`APP_ICON_GROUP_ID`]: `GRPICONDIR { reserved 0, type 1, count }` followed by one 14-byte
///    `GRPICONDIRENTRY { width, height, colorCount, reserved, planes, bitCount, bytesInRes (u32), id (u16: the RT_ICON id) }` per image (the `.ico` entry with its 4-byte offset replaced by the id).
///
/// Header layout of one resource: `DataSize u32`, `HeaderSize u32`, `TYPE` as `0xFFFF` + `u16` id, `NAME` as `0xFFFF` + `u16` id, padding to 4 bytes, `DataVersion u32` 0, `MemoryFlags u16`
/// (`0x1030` = MOVEABLE | PURE | DISCARDABLE for icons and manifest), `LanguageId u16`, `Version u32` 0, `Characteristics u32` 0. All integers little endian.
pub fn build_res(manifest_xml: &str, ico: Option<&[u8]>) -> Result<Vec<u8>, ResError> {
    todo!()
}

/// Splits an `.ico` file into its images: `(width, height, colorCount, planes, bitCount, bytes)` in directory order (the `width`/`height`/`colorCount` bytes as stored, `0` = 256 / 256 colours
/// is kept as `0`). `ResError::NotAnIco` for a wrong header, `ResError::Truncated` when an entry's `[imageOffset, imageOffset + bytesInRes)` is not inside `ico`.
pub fn parse_ico(ico: &[u8]) -> Result<Vec<IcoImage>, ResError> {
    todo!()
}

/// One image of an `.ico` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcoImage {
    /// `bWidth` as stored (`0` means 256).
    pub width: u8,
    /// `bHeight` as stored (`0` means 256).
    pub height: u8,
    /// `bColorCount` as stored.
    pub color_count: u8,
    /// `wPlanes`.
    pub planes: u16,
    /// `wBitCount`.
    pub bit_count: u16,
    /// The image bytes (`dwBytesInRes` of them).
    pub data: Vec<u8>,
}
