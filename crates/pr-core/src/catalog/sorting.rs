//! Port of `PhotoReview.Core/Catalog/ImageSortService.cs` and `SiblingFolderService.cs` (C# baseline d92d6aae).
//!
//! * [`sort_entries`] / [`sort_paths`] / [`parse_sort_mode`]: the fallback folder sort (PARITY P-02, S-05). Name sorts
//!   use [`crate::catalog::natural_cmp`]; size sorts use the file size, then the same name order. All sorts are STABLE
//!   (LINQ `OrderBy`), `NameDescending` is the exact reverse of the ascending result (ties reversed too).
//! * [`get_sorted_siblings`]: the sibling folders of the open folder in natural name order (PARITY K-05, K-06).
//!
//! C# `SiblingFolderService` calls `Directory.EnumerateDirectories` directly (it is on the architecture test's
//! file-system-boundary allowlist, there is no `IFileSystem` seam), so the Rust port lists with `std::fs` too and keeps
//! the sort/merge step in a pure function ([`sort_sibling_folders`]) that tests can drive without a disk.
// Temporary (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::io;
use std::path::Path;

use crate::abstractions::{FileSystem, Log, NaturalComparer};
use crate::catalog::types::CatalogEntry;
use crate::model::ImageSortMode;
use crate::text::dotnet_path::PathError;

/// `ImageSortService.SortEntries(entries, mode, naturalComparer)`.
///
/// `natural_comparer`: `None` = C# `null`, i.e. `ManagedNaturalComparer.Instance`. Result: a NEW list holding clones
/// of the entries (metadata and capture group stay attached).
/// * `Default`: the input order, duplicates included (a plain copy).
/// * `NameDescending`: the ascending name sort, reversed.
/// * `SizeDescending`: by `entry.length.unwrap_or(-1)` DESCENDING, then by file name ascending.
/// * `SizeAscending`: by `entry.length.unwrap_or(-1)` ascending, then by file name ascending.
/// * `Name` and `NameAscending` (the C# `_` arm): by file name ascending.
///
/// File name = [`crate::text::dotnet_path::get_file_name`] of the entry path, compared with the comparer (ties keep
/// the input order). With `natural_comparer == None` compute each name's `build_natural_key` ONCE (C# `NameKey`) and
/// compare with the three steps of `compare_natural` (key OrdinalIgnoreCase, name OrdinalIgnoreCase, name Ordinal);
/// the result must equal what the generic path gives with a comparer that behaves like `ManagedNaturalComparer`.
pub fn sort_entries(
    entries: &[CatalogEntry],
    mode: ImageSortMode,
    natural_comparer: Option<&dyn NaturalComparer>,
) -> Vec<CatalogEntry> {
    todo!()
}

/// `ImageSortService.Sort(files, mode, naturalComparer, log)` (the `ImageSortMode` overload): same orders as
/// [`sort_entries`], but the size of each file comes from `file_system.get_file_stat(Path::new(path))`.
/// A file whose stat is `None` (missing, a directory, unreadable) has size `-1` (sorts as the smallest) and is logged
/// ONCE per call, in input order, with `log.error(&format!("Failed to read file size for {path}"), Some(&err))` where
/// `err` is an `io::Error` of kind `NotFound` (C# logs the `FileNotFoundException`); `log == None` logs nothing.
/// Sizes are read once per file, before sorting, only for the two size modes (never for `Default` or the name modes).
/// `u64` sizes above `i64::MAX` saturate to `i64::MAX`.
pub fn sort_paths(
    files: &[String],
    mode: ImageSortMode,
    natural_comparer: Option<&dyn NaturalComparer>,
    file_system: &dyn FileSystem,
    log: Option<&dyn Log>,
) -> Vec<String> {
    todo!()
}

/// The `string? mode` switch of `ImageSortService.Sort(files, mode: string?, ...)`: `mode?.ToUpperInvariant()`:
/// `"SIZE"` or `"SIZEDESCENDING"` => `SizeDescending`; `"SIZEASCENDING"` => `SizeAscending`; `"DEFAULT"` => `Default`;
/// `"NAMEASCENDING"` => `NameAscending`; `"NAMEDESCENDING"` => `NameDescending`; ANYTHING else (including `None`,
/// `""`, `"Name"`, `"bogus"`) => `ImageSortMode::Name`. No trimming. QUIRK (verified with .NET 10): `ToUpperInvariant`
/// maps U+017F LATIN SMALL LETTER LONG S to `S` (`"\u{17F}ize"` is `SIZE`) but leaves U+0131 DOTLESS I alone
/// (`"s\u{131}ze"` is NOT `SIZE`, it falls to the default arm), and no other non-ASCII char uppercases to ASCII. Rust's
/// `str::to_uppercase` would turn `ı` into `I` (wrong): upper-case with
/// `chars().map(|c| if c == '\u{17F}' { 'S' } else { c.to_ascii_uppercase() })` and compare with the ASCII names.
pub fn parse_sort_mode(mode: Option<&str>) -> ImageSortMode {
    todo!()
}

/// `ImageSortService.Sort(files, string? mode, ...)` = `sort_paths(files, parse_sort_mode(mode), ...)`.
pub fn sort_paths_by_mode_name(
    files: &[String],
    mode: Option<&str>,
    natural_comparer: Option<&dyn NaturalComparer>,
    file_system: &dyn FileSystem,
    log: Option<&dyn Log>,
) -> Vec<String> {
    todo!()
}

/// The pure half of `SiblingFolderService.GetSorted`. `full_current` is the already-resolved
/// `Path.TrimEndingDirectorySeparator(Path.GetFullPath(currentFolder))`; `listed` are the full paths of the parent's
/// visible (not Hidden/System) sub-folders in the order the file system returned them.
/// 1. If no listed path equals `full_current` (OrdinalIgnoreCase) append `full_current` at the END (the folder the user
///    deliberately opened may itself be Hidden/System and must stay in the list).
/// 2. Stable-sort by [`crate::text::dotnet_path::get_file_name`] with `ManagedNaturalComparer`
///    (`compare_natural`; `Album 1` < `Album 2` < `Album 10`).
pub fn sort_sibling_folders(full_current: &str, listed: Vec<String>) -> Vec<String> {
    todo!()
}

/// The impure half: the full paths (`parent` + `\` + name, in file-system enumeration order) of the directories
/// directly inside `parent`, skipping every entry whose attributes include Hidden (0x2) or System (0x4). Non-recursive.
/// Directory symlinks and junctions count as directories (their attributes have the DIRECTORY bit). Any error from
/// opening the listing or from reading an entry is returned (`IgnoreInaccessible = false` in C#).
/// Implementation: `std::fs::read_dir`, then per entry `entry.metadata()?.file_attributes()` under `#[cfg(windows)]`
/// (`std::os::windows::fs::MetadataExt`; pr-core already has `cfg(windows)` code in `abstractions/source_reader.rs`).
pub fn list_visible_directories(parent: &Path) -> io::Result<Vec<String>> {
    todo!()
}

/// `SiblingFolderService.GetSorted(currentFolder, log)`.
/// * `full_current = trim_ending_directory_separator(&get_full_path(current_folder)?)`; a `PathError` is returned as
///   `Err` (C# lets `GetFullPath`'s exception escape; it is outside the `try`).
/// * `parent = Directory.GetParent(full_current)` = `get_directory_name(full_current)`; `None` (a drive root or a UNC
///   share root) => `Ok(vec![])`.
/// * Listing failure (any `io::Error`, e.g. the parent does not exist) => log
///   `log.error(&format!("Failed to enumerate sibling folders of {parent}"), Some(&err))` and return `Ok(vec![])`.
/// * Otherwise `Ok(sort_sibling_folders(&full_current, list_visible_directories(parent)?))`.
pub fn get_sorted_siblings(current_folder: &str, log: Option<&dyn Log>) -> Result<Vec<String>, PathError> {
    todo!()
}
