//! Port of `PhotoReview.Core/Catalog/DragDropInputService.cs` (C# baseline d92d6aae): turns the paths dropped on the main
//! window into "open this folder" / "open this image's folder at this image" (PARITY K-39).
//!
//! Differences forced by the Rust layering (no behaviour change):
//! * C# calls `Directory.Exists` / `File.Exists` directly; Rust takes `&dyn FileSystem` (the production caller passes the
//!   `PhysicalFileSystem`, tests pass a `FakeFileSystem`).
//! * C# returns the localized warning text (`Tr.CoreDragDrop*`). `pr-core` cannot depend on the localizer, so the result
//!   carries a language-neutral [`DragDropWarning`]; the window maps it to text with [`DragDropWarning::catalog_key`].

use std::path::Path;

use crate::abstractions::FileSystem;
use crate::catalog::types::is_supported_with;
use crate::text::dotnet_path::{get_directory_name, get_full_path, trim_ending_directory_separator};
use crate::text::eq_ignore_case;

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
        match self {
            Self::NoValidInput => "core.dragDrop.noValidInput",
            Self::OnlyFirstFolder => "core.dragDrop.onlyFirstFolder",
            Self::NoSupportedImages => "core.dragDrop.noSupportedImages",
            Self::FolderUnknown => "core.dragDrop.folderUnknown",
            Self::MixedFolders => "core.dragDrop.mixedFolders",
            Self::UnsupportedIgnored => "core.dragDrop.unsupportedIgnored",
        }
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
        self.kind != DragDropInputKind::Invalid && self.folder_path.is_some()
    }
}

/// `Invalid` result with no folder and no image.
fn invalid(ignored: usize, warning: DragDropWarning) -> DragDropInputResult {
    DragDropInputResult {
        kind: DragDropInputKind::Invalid,
        folder_path: None,
        initial_image_path: None,
        ignored_path_count: ignored,
        warning: Some(warning),
    }
}

/// The full path of `path` when the file system reports a directory (`want_dir`) or a file at it. Existence is checked on
/// the full path, the way .NET `Directory.Exists` / `File.Exists` resolve their argument. A `get_full_path` failure means
/// "does not exist": C# would let the exception escape here, but only a fake file system can fail after an `Exists` hit.
fn existing_full_path(path: &str, want_dir: bool, file_system: &dyn FileSystem) -> Option<String> {
    let full = get_full_path(path).ok()?;
    let target = Path::new(&full);
    let exists = if want_dir {
        file_system.directory_exists(target)
    } else {
        file_system.file_exists(target)
    };
    exists.then_some(full)
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
    let valid: Vec<&str> = paths
        .unwrap_or(&[])
        .iter()
        .map(String::as_str)
        .filter(|path| !path.trim().is_empty())
        .collect();
    if valid.is_empty() {
        return invalid(0, DragDropWarning::NoValidInput);
    }

    // C# `FirstOrDefault(Directory.Exists)` is lazy: `find_map` stops at the first existing folder too.
    let folder = valid
        .iter()
        .copied()
        .find_map(|path| existing_full_path(path, true, file_system).map(|full| (path, full)));
    if let Some((folder_raw, folder_full)) = folder {
        let ignored = valid
            .iter()
            .copied()
            .filter(|&path| !is_same_folder(path, folder_raw))
            .count();
        return DragDropInputResult {
            kind: DragDropInputKind::Folder,
            folder_path: Some(folder_full),
            initial_image_path: None,
            ignored_path_count: ignored,
            warning: (ignored > 0).then_some(DragDropWarning::OnlyFirstFolder),
        };
    }

    // (raw path, full path) of every existing, supported image, in input order.
    let images: Vec<(&str, String)> = valid
        .iter()
        .copied()
        .filter_map(|path| {
            let full = existing_full_path(path, false, file_system)?;
            is_supported_with(path, raw_enabled, webp_heic_enabled).then_some((path, full))
        })
        .collect();
    let Some((_, initial)) = images.first() else {
        return invalid(valid.len(), DragDropWarning::NoSupportedImages);
    };
    let Some(folder) = get_directory_name(initial) else {
        return invalid(valid.len(), DragDropWarning::FolderUnknown);
    };

    let accepted = images
        .iter()
        .filter(|(_, full)| get_directory_name(full).is_some_and(|dir| eq_ignore_case(&dir, &folder)))
        .count();
    let ignored = valid.len().saturating_sub(accepted);
    let warning = if accepted != images.len() {
        Some(DragDropWarning::MixedFolders)
    } else if ignored > 0 {
        Some(DragDropWarning::UnsupportedIgnored)
    } else {
        None
    };
    DragDropInputResult {
        kind: DragDropInputKind::Image,
        folder_path: Some(folder),
        initial_image_path: Some(initial.clone()),
        ignored_path_count: ignored,
        warning,
    }
}

/// `IsSameFolder(path, folder)`: equal text (OrdinalIgnoreCase), or `trim_ending_directory_separator(get_full_path(path))`
/// equals the same of `folder` (OrdinalIgnoreCase). A `get_full_path` failure => false (C# catches `ArgumentException`,
/// `NotSupportedException`, `PathTooLongException` here).
fn is_same_folder(path: &str, folder: &str) -> bool {
    if eq_ignore_case(path, folder) {
        return true;
    }
    let (Ok(path_full), Ok(folder_full)) = (get_full_path(path), get_full_path(folder)) else {
        return false;
    };
    eq_ignore_case(
        trim_ending_directory_separator(&path_full),
        trim_ending_directory_separator(&folder_full),
    )
}
