//! Port of `PhotoReview.Core/Catalog/DragDropInputService.cs` (C# baseline d92d6aae): turns the paths dropped on the main
//! window into "open this folder" / "open this image's folder at this image" (PARITY K-39).
//!
//! Differences forced by the Rust layering (no behaviour change):
//! * C# calls `Directory.Exists` / `File.Exists` directly; Rust takes `&dyn FileSystem` (the production caller passes the
//!   `PhysicalFileSystem`, tests pass a `FakeFileSystem`).
//! * C# returns the localized warning text (`Tr.CoreDragDrop*`). `pr-core` cannot depend on the localizer, so the result
//!   carries a language-neutral [`DragDropWarning`]; the window maps it to text with [`DragDropWarning::catalog_key`].
// Temporary (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use crate::abstractions::FileSystem;

/// `DragDropInputKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragDropInputKind {
    Invalid,
    Folder,
    Image,
}

/// The warning of a [`DragDropInputResult`] (C# `string? Warning`, one `Tr.CoreDragDrop*` text each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragDropWarning {
    /// `Tr.CoreDragDropNoValidInput`: nothing usable was dropped.
    NoValidInput,
    /// `Tr.CoreDragDropOnlyFirstFolder`: more than the first folder was dropped.
    OnlyFirstFolder,
    /// `Tr.CoreDragDropNoSupportedImages`: no dropped file is a supported, existing image.
    NoSupportedImages,
    /// `Tr.CoreDragDropFolderUnknown`: the first image has no parent folder.
    FolderUnknown,
    /// `Tr.CoreDragDropMixedFolders`: images from several folders, only the first image's folder is opened.
    MixedFolders,
    /// `Tr.CoreDragDropUnsupportedIgnored`: some dropped paths were skipped.
    UnsupportedIgnored,
}

impl DragDropWarning {
    /// The language-catalog key of the text (assets/languages/en.json): `NoValidInput` => `"core.dragDrop.noValidInput"`,
    /// `OnlyFirstFolder` => `"core.dragDrop.onlyFirstFolder"`, `NoSupportedImages` => `"core.dragDrop.noSupportedImages"`,
    /// `FolderUnknown` => `"core.dragDrop.folderUnknown"`, `MixedFolders` => `"core.dragDrop.mixedFolders"`,
    /// `UnsupportedIgnored` => `"core.dragDrop.unsupportedIgnored"`.
    pub fn catalog_key(self) -> &'static str {
        todo!()
    }
}

/// `DragDropInputResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragDropInputResult {
    pub kind: DragDropInputKind,
    pub folder_path: Option<String>,
    pub initial_image_path: Option<String>,
    /// Dropped paths that were not used (C# `int IgnoredPathCount`).
    pub ignored_path_count: usize,
    pub warning: Option<DragDropWarning>,
}

impl DragDropInputResult {
    /// `IsValid`: `kind != Invalid && folder_path.is_some()`.
    pub fn is_valid(&self) -> bool {
        todo!()
    }
}

/// `DragDropInputService.Parse(paths, rawEnabled, webpHeicEnabled)`. `paths == None` is C# `null`.
/// 1. `valid` = the non-blank paths (`trim().is_empty()` filtered out), input order. None => `Invalid`, no folder, no image,
///    ignored 0, warning `NoValidInput`.
/// 2. Folder drop: the FIRST valid path for which `file_system.directory_exists` is true. Then `ignored` = number of valid
///    paths that are not [`is_same_folder`] as it (the folder itself, dropped twice, recased or with a trailing separator,
///    counts as the same); result `Folder`, `folder_path = get_full_path(folder)` (NOT trimmed: a trailing separator
///    stays), no image, ignored, warning `OnlyFirstFolder` iff ignored > 0.
/// 3. Image drop: `images` = valid paths for which `file_system.file_exists` is true AND
///    `types::is_supported_with(path, raw_enabled, webp_heic_enabled)`. None => `Invalid`, ignored = `valid.len()`, warning
///    `NoSupportedImages`.
/// 4. `initial = get_full_path(images[0])`; `folder = get_directory_name(initial)`; `None` => `Invalid`, ignored =
///    `valid.len()`, warning `FolderUnknown`.
/// 5. `accepted` = images whose `get_directory_name(get_full_path(p))` equals `folder` (OrdinalIgnoreCase);
///    `ignored = valid.len() - accepted.len()` (missing files, unsupported files and images of other folders all count);
///    warning: `MixedFolders` if `accepted.len() != images.len()`, else `UnsupportedIgnored` if ignored > 0, else none.
///    Result `Image`, `folder_path = Some(folder)`, `initial_image_path = Some(initial)`.
///
/// A path for which `get_full_path` fails (impossible with a real file system, possible with a fake) is treated as "does
/// not exist" (DIVERGENCE from C#, where the exception would escape); record it as a comment in the test.
pub fn parse(
    paths: Option<&[String]>,
    raw_enabled: bool,
    webp_heic_enabled: bool,
    file_system: &dyn FileSystem,
) -> DragDropInputResult {
    todo!()
}

/// `IsSameFolder(path, folder)`: equal text (OrdinalIgnoreCase), or `trim_ending_directory_separator(get_full_path(path))`
/// equals the same of `folder` (OrdinalIgnoreCase). A `get_full_path` failure => false (C# catches `ArgumentException`,
/// `NotSupportedException`, `PathTooLongException` here).
fn is_same_folder(path: &str, folder: &str) -> bool {
    todo!()
}
