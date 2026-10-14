//! Port of `PhotoReview.Core/Catalog/ExplorerSnapshotValidator.cs` (C# baseline d92d6aae): decides whether the file order
//! Explorer reported (a COM snapshot) may replace the catalog order (INV-7, PARITY P-01). The snapshot is untrusted:
//! the validator answers `Err(reason)` for anything odd and never fails otherwise.
//!
//! Reason texts are the language-neutral codes of [`super::explorer_reason`] (or, for a non-`Available` snapshot, the
//! snapshot's own reason / the status name).

use std::collections::{HashMap, HashSet};

use crate::catalog::explorer_reason::{DUPLICATE_ITEM, EMPTY_PATH, INCOMPLETE_SNAPSHOT, OUTSIDE_FOLDER};
use crate::catalog::explorer_types::{ExplorerOrderStatus, ExplorerViewSnapshot};
use crate::text::dotnet_path::{PathError, get_directory_name_raw, get_full_path, trim_ending_directory_separator};
use crate::text::{eq_ignore_case, ignore_case_key};

/// The extended-length UNC prefix (8 chars).
const UNC_PREFIX: &str = r"\\?\UNC\";

/// `ExplorerSnapshotValidator.TryValidate(snapshot, scannedFiles, out ordered, out reason)`.
///
/// `Ok(ordered)` = C# `true`: the scanned files in the order of the snapshot, as the CALLER'S own scanned strings (they may
/// carry a `\\?\` prefix), only the members of `scanned_files`. `Err(reason)` = C# `false` (then `ordered` is empty and
/// `reason` is non-null). Steps, in this order:
/// 1. `snapshot.status != Available` => `Err(snapshot.reason.clone().unwrap_or(status_name(status)))`. An empty-string
///    reason is kept as is (only `None` falls back). Names: `NoMatchingWindow`, `NativeViewUnavailable`, `InvalidSnapshot`,
///    `TimedOut`, `Canceled`, `Failed` (C# `Status.ToString()`; write a private `status_name`).
///    (C# `snapshot is null`, `scannedFiles is null` and `OrderedPaths is null` all end as `IncompleteSnapshot`; none of them
///    can occur in Rust where those are non-optional.)
/// 2. `folder = canonicalize_folder(&snapshot.folder)`. `expected` = map `normalize_path(scanned)` (OrdinalIgnoreCase key)
///    -> the scanned string, FIRST scanned string of a key wins. Any `PathError` in this step (blank folder, a scanned path
///    with NUL...) => `Err(OUTSIDE_FOLDER)`.
/// 3. For each `candidate` of `snapshot.ordered_paths`, in order:
///    a. empty/whitespace-only (`trim().is_empty()`) => `Err(EMPTY_PATH)`;
///    b. `path = normalize_path(candidate)`; `PathError` => `Err(OUTSIDE_FOLDER)`;
///    c. `!in_folder(&path, &folder)` => `Err(OUTSIDE_FOLDER)`. If `in_folder` fails with a `PathError` (it can: see below)
///    the WHOLE call ends with `Err(INCOMPLETE_SNAPSHOT)`;
///    d. `path` already seen (OrdinalIgnoreCase) => `Err(DUPLICATE_ITEM)` (checked for non-scanned files too, so a
///    listed `x.txt` twice is a duplicate); otherwise remember it;
///    e. `path` in `expected` => push the scanned string to the result (files not scanned are silently skipped).
/// 4. `result.len() != expected.len()` => `Err(INCOMPLETE_SNAPSHOT)`; else `Ok(result)`.
///
/// Any `PathError` that is not caught in steps 2/3b (only the one from `in_folder`) maps to `INCOMPLETE_SNAPSHOT`.
pub fn try_validate(snapshot: &ExplorerViewSnapshot, scanned_files: &[String]) -> Result<Vec<String>, String> {
    if snapshot.status != ExplorerOrderStatus::Available {
        return Err(match &snapshot.reason {
            Some(r) => r.clone(),
            None => status_name(snapshot.status).to_owned(),
        });
    }

    // Step 2: any PathError here is "outside-folder".
    let Ok(folder) = canonicalize_folder(&snapshot.folder) else {
        return Err(OUTSIDE_FOLDER.to_owned());
    };
    let mut expected: HashMap<String, &String> = HashMap::with_capacity(scanned_files.len());
    for scanned in scanned_files {
        let Ok(n) = normalize_path(scanned) else {
            return Err(OUTSIDE_FOLDER.to_owned());
        };
        expected.entry(ignore_case_key(&n)).or_insert(scanned);
    }

    let mut result: Vec<String> = Vec::with_capacity(expected.len());
    let mut seen: HashSet<String> = HashSet::with_capacity(snapshot.ordered_paths.len());
    for candidate in &snapshot.ordered_paths {
        if candidate.trim().is_empty() {
            return Err(EMPTY_PATH.to_owned());
        }
        let Ok(path) = normalize_path(candidate) else {
            return Err(OUTSIDE_FOLDER.to_owned());
        };
        match in_folder(&path, &folder) {
            Ok(true) => {}
            Ok(false) => return Err(OUTSIDE_FOLDER.to_owned()),
            // C#: the exception escapes to the outer catch.
            Err(_) => return Err(INCOMPLETE_SNAPSHOT.to_owned()),
        }
        let key = ignore_case_key(&path);
        let original = expected.get(&key).copied();
        if !seen.insert(key) {
            return Err(DUPLICATE_ITEM.to_owned());
        }
        if let Some(original) = original {
            result.push(original.clone());
        }
    }

    if result.len() != expected.len() {
        return Err(INCOMPLETE_SNAPSHOT.to_owned());
    }
    Ok(result)
}

/// `ExplorerSnapshotValidator.CanonicalizeFolder`: `trim_ending_directory_separator(&normalize_path(folder)?)`.
pub fn canonicalize_folder(folder: &str) -> Result<String, PathError> {
    let normalized = normalize_path(folder)?;
    Ok(trim_ending_directory_separator(&normalized).to_owned())
}

/// `ExplorerSnapshotValidator.NormalizePath`: `get_full_path(path)?` with the Win32 extended-length prefix removed:
/// * starts with `\\?\UNC\` (case-INsensitive) => `\\` + the rest after those 8 chars (`\\?\UNC\srv\share\a` => `\\srv\share\a`);
/// * else length >= 6, starts with `\\?\` (case-SENSITIVE) and the char at index 5 is `':'` (a drive) => the text after the
///   4-char prefix (`\\?\C:\a` => `C:\a`, `\\?\C:` => `C:`);
/// * else unchanged (`\\?\x`, `\\?\Volume{1}\a.jpg`, `C:\ab:c`).
///
/// `get_full_path` leaves `\\?\` paths untouched, so `\\?\C:\a\..\b` becomes `C:\a\..\b` (dots NOT resolved).
pub fn normalize_path(path: &str) -> Result<String, PathError> {
    let full = get_full_path(path)?;
    // C# indexes UTF-16 code units, so an astral char at index 4 occupies units 4..6 and unit 5 is a low surrogate.
    let units: Vec<u16> = full.encode_utf16().collect();
    if units.len() >= 8 {
        let prefix = String::from_utf16_lossy(&units[..8]);
        if eq_ignore_case(&prefix, UNC_PREFIX) {
            let rest = String::from_utf16_lossy(&units[8..]);
            return Ok(format!("\\\\{rest}"));
        }
    }
    let ext: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if units.len() >= 6 && units[..4] == ext && units[5] == u16::from(b':') {
        return Ok(String::from_utf16_lossy(&units[4..]));
    }
    Ok(full)
}

/// `SamePath(first, second)`: `canonicalize_folder` of both are equal (OrdinalIgnoreCase). `PathError` from either.
pub fn same_path(first: &str, second: &str) -> Result<bool, PathError> {
    let a = canonicalize_folder(first)?;
    let b = canonicalize_folder(second)?;
    Ok(eq_ignore_case(&a, &b))
}

/// `InFolder(normalizedPath, canonicalFolder)`: `directory = get_directory_name_raw(normalized_path)` (the span overload:
/// no separator normalization, `""` when there is none, e.g. for a drive root `C:\`). True when `directory` equals
/// `canonical_folder` (OrdinalIgnoreCase) or `same_path(directory, canonical_folder)?`. QUIRK: for a candidate that is a
/// root (`C:\`, `\\srv\share`) `directory` is `""` and `same_path("", ..)` fails with `PathError::Argument`
/// (`GetFullPath("")` throws), which C# lets escape to `TryValidate`'s outer catch (=> `INCOMPLETE_SNAPSHOT`, not
/// `OUTSIDE_FOLDER`).
fn in_folder(normalized_path: &str, canonical_folder: &str) -> Result<bool, PathError> {
    let directory = get_directory_name_raw(normalized_path);
    if eq_ignore_case(directory, canonical_folder) {
        return Ok(true);
    }
    same_path(directory, canonical_folder)
}

/// `snapshot.Status.ToString()` for a non-`Available` status (C# enum member names).
fn status_name(status: ExplorerOrderStatus) -> &'static str {
    match status {
        ExplorerOrderStatus::Available => "Available",
        ExplorerOrderStatus::NoMatchingWindow => "NoMatchingWindow",
        ExplorerOrderStatus::NativeViewUnavailable => "NativeViewUnavailable",
        ExplorerOrderStatus::InvalidSnapshot => "InvalidSnapshot",
        ExplorerOrderStatus::TimedOut => "TimedOut",
        ExplorerOrderStatus::Canceled => "Canceled",
        ExplorerOrderStatus::Failed => "Failed",
    }
}
