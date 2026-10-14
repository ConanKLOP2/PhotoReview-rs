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
use std::cmp::Ordering;
use std::io;
use std::path::Path;

use crate::abstractions::{FileSystem, Log, NaturalComparer};
use crate::catalog::natural_cmp::build_natural_key;
use crate::catalog::types::CatalogEntry;
use crate::model::ImageSortMode;
use crate::text::dotnet_path::{
    PathError, get_directory_name, get_file_name, get_full_path, trim_ending_directory_separator,
};
use crate::text::{cmp_ignore_case, eq_ignore_case};

/// Size sort request: the per-item size keys and the direction.
struct SizeKeys<'a> {
    keys: &'a [i64],
    descending: bool,
}

/// Stable order (as indices into `names`) of items by optional size key, then by name (natural).
///
/// `None` comparer (C# `ManagedNaturalComparer`): the natural key of each name is built once (C# `NameKey`) and the
/// comparison is the three steps of `compare_natural`. `Some(c)`: the generic path through the comparer.
fn order_by(names: &[&str], size: Option<SizeKeys<'_>>, comparer: Option<&dyn NaturalComparer>) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..names.len()).collect();
    let natural_keys: Vec<String> = match comparer {
        None => names.iter().map(|name| build_natural_key(name)).collect(),
        Some(_) => Vec::new(),
    };
    let name_cmp = |a: usize, b: usize| -> Ordering {
        match comparer {
            Some(c) => c.compare(Some(names[a]), Some(names[b])),
            None => cmp_ignore_case(&natural_keys[a], &natural_keys[b])
                .then_with(|| cmp_ignore_case(names[a], names[b]))
                .then_with(|| names[a].cmp(names[b])),
        }
    };
    // `Vec::sort_by` is stable (LINQ `OrderBy`/`ThenBy`).
    match size {
        None => indices.sort_by(|&a, &b| name_cmp(a, b)),
        Some(SizeKeys { keys, descending }) => indices.sort_by(|&a, &b| {
            let by_size = if descending {
                keys[b].cmp(&keys[a])
            } else {
                keys[a].cmp(&keys[b])
            };
            by_size.then_with(|| name_cmp(a, b))
        }),
    }
    indices
}

/// Shared body of the two sort entry points. `size_of` is only called for the two size modes, once per item in
/// input order.
fn sort_items<T: Clone>(
    items: &[T],
    mode: ImageSortMode,
    comparer: Option<&dyn NaturalComparer>,
    path_of: impl Fn(&T) -> &str,
    mut size_of: impl FnMut(&T) -> i64,
) -> Vec<T> {
    if mode == ImageSortMode::Default {
        return items.to_vec();
    }
    let names: Vec<&str> = items.iter().map(|item| get_file_name(path_of(item))).collect();
    let sizes: Option<(Vec<i64>, bool)> = match mode {
        ImageSortMode::SizeDescending => Some((items.iter().map(&mut size_of).collect(), true)),
        ImageSortMode::SizeAscending => Some((items.iter().map(&mut size_of).collect(), false)),
        _ => None,
    };
    let size_keys = sizes.as_ref().map(|(keys, descending)| SizeKeys {
        keys,
        descending: *descending,
    });
    let order = order_by(&names, size_keys, comparer);
    let mut sorted: Vec<T> = order.into_iter().map(|i| items[i].clone()).collect();
    if mode == ImageSortMode::NameDescending {
        sorted.reverse();
    }
    sorted
}

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
    sort_items(
        entries,
        mode,
        natural_comparer,
        |entry| entry.path(),
        |entry| entry.length.unwrap_or(-1),
    )
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
    let size_of = |path: &String| -> i64 {
        match file_system.get_file_stat(Path::new(path)) {
            Some(stat) => i64::try_from(stat.length).unwrap_or(i64::MAX),
            None => {
                if let Some(log) = log {
                    let err = io::Error::new(io::ErrorKind::NotFound, format!("Could not find file '{path}'."));
                    log.error(&format!("Failed to read file size for {path}"), Some(&err));
                }
                -1
            }
        }
    };
    sort_items(files, mode, natural_comparer, |path| path.as_str(), size_of)
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
    // `ToUpperInvariant` quirk: U+017F maps to `S`, U+0131 stays; nothing else becomes ASCII.
    let upper: String = mode
        .unwrap_or("")
        .chars()
        .map(|c| if c == '\u{17F}' { 'S' } else { c.to_ascii_uppercase() })
        .collect();
    match upper.as_str() {
        "SIZE" | "SIZEDESCENDING" => ImageSortMode::SizeDescending,
        "SIZEASCENDING" => ImageSortMode::SizeAscending,
        "DEFAULT" => ImageSortMode::Default,
        "NAMEASCENDING" => ImageSortMode::NameAscending,
        "NAMEDESCENDING" => ImageSortMode::NameDescending,
        _ => ImageSortMode::Name,
    }
}

/// `ImageSortService.Sort(files, string? mode, ...)` = `sort_paths(files, parse_sort_mode(mode), ...)`.
pub fn sort_paths_by_mode_name(
    files: &[String],
    mode: Option<&str>,
    natural_comparer: Option<&dyn NaturalComparer>,
    file_system: &dyn FileSystem,
    log: Option<&dyn Log>,
) -> Vec<String> {
    sort_paths(files, parse_sort_mode(mode), natural_comparer, file_system, log)
}

/// The pure half of `SiblingFolderService.GetSorted`. `full_current` is the already-resolved
/// `Path.TrimEndingDirectorySeparator(Path.GetFullPath(currentFolder))`; `listed` are the full paths of the parent's
/// visible (not Hidden/System) sub-folders in the order the file system returned them.
/// 1. If no listed path equals `full_current` (OrdinalIgnoreCase) append `full_current` at the END (the folder the user
///    deliberately opened may itself be Hidden/System and must stay in the list).
/// 2. Stable-sort by [`crate::text::dotnet_path::get_file_name`] with `ManagedNaturalComparer`
///    (`compare_natural`; `Album 1` < `Album 2` < `Album 10`).
pub fn sort_sibling_folders(full_current: &str, listed: Vec<String>) -> Vec<String> {
    let mut folders = listed;
    if !folders.iter().any(|folder| eq_ignore_case(folder, full_current)) {
        folders.push(full_current.to_owned());
    }
    let names: Vec<&str> = folders.iter().map(|folder| get_file_name(folder)).collect();
    let order = order_by(&names, None, None);
    order.into_iter().map(|i| folders[i].clone()).collect()
}

/// The impure half: the full paths (`parent` + `\` + name, in file-system enumeration order) of the directories
/// directly inside `parent`, skipping every entry whose attributes include Hidden (0x2) or System (0x4). Non-recursive.
/// Directory symlinks and junctions count as directories (their attributes have the DIRECTORY bit). Any error from
/// opening the listing or from reading an entry is returned (`IgnoreInaccessible = false` in C#).
/// Implementation: `std::fs::read_dir`, then per entry `entry.metadata()?.file_attributes()` under `#[cfg(windows)]`
/// (`std::os::windows::fs::MetadataExt`; pr-core already has `cfg(windows)` code in `abstractions/source_reader.rs`).
pub fn list_visible_directories(parent: &Path) -> io::Result<Vec<String>> {
    let mut result = Vec::new();
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if is_visible_directory(&metadata) {
            result.push(entry.path().to_string_lossy().into_owned());
        }
    }
    Ok(result)
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
    let full = get_full_path(current_folder)?;
    let full_current = trim_ending_directory_separator(&full);
    let Some(parent) = get_directory_name(full_current) else {
        return Ok(Vec::new());
    };
    match list_visible_directories(Path::new(&parent)) {
        Ok(listed) => Ok(sort_sibling_folders(full_current, listed)),
        Err(err) => {
            if let Some(log) = log {
                log.error(&format!("Failed to enumerate sibling folders of {parent}"), Some(&err));
            }
            Ok(Vec::new())
        }
    }
}

/// Directory (attribute 0x10, so directory symlinks and junctions count) that is neither Hidden (0x2) nor System (0x4).
#[cfg(windows)]
fn is_visible_directory(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    let attrs = metadata.file_attributes();
    attrs & 0x10 != 0 && attrs & (0x2 | 0x4) == 0
}

#[cfg(not(windows))]
fn is_visible_directory(metadata: &std::fs::Metadata) -> bool {
    metadata.is_dir()
}
