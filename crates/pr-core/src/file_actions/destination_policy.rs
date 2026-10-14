//! Port of `PhotoReview.Core/FileActions/ActionDestinationPolicy.cs` (C# baseline d92d6aae, includes the R32 drive-root
//! fix): where a Move/Copy destination may point (PARITY F-05, C-22; SEC-01, RV-S06, R32).
//!
//! * [`validate`]: lexical check of the destination STRING (blank, invalid characters, `..` escape).
//! * [`validate_no_escape_via_reparse_point`]: SEC-01, resolves symlinks/junctions with
//!   [`FileSystem::resolve_real_path`] and fails CLOSED.
//! * [`validate_picked_folder`]: "Move to... / Copy to..." folder-picker result.
use std::path::Path;

use crate::abstractions::FileSystem;
use crate::catalog::types::ArgumentError;
use crate::text::dotnet_path::{
    PathError, ends_in_directory_separator, get_full_path, is_invalid_path_char, is_path_fully_qualified,
    is_path_rooted, trim_ending_directory_separator,
};
use crate::text::{eq_ignore_case, ignore_case_key};

/// `ActionDestinationCheck`: result of [`validate`] and [`validate_no_escape_via_reparse_point`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionDestinationCheck {
    Ok,
    Empty,
    InvalidChars,
    EscapesSourceFolder,
}

/// `PickedFolderCheck`: result of [`validate_picked_folder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickedFolderCheck {
    Ok,
    Empty,
    NotAbsolute,
    SameAsPhotoFolder,
    Missing,
}

/// `ActionDestinationPolicy.Validate(destination)`. `None` is C# `null`. Rules, in this order:
/// 1. `None`, empty or whitespace-only (`trim().is_empty()`) => `Empty`.
/// 2. `body` = `destination` without a leading `\\?\` (exact, case-sensitive prefix; the extended-length prefix
///    legitimately contains `?`). Any char of `body` that is `'*'`, `'?'` or in `dotnet_path::is_invalid_path_char`
///    (`'|'` and U+0000..=U+001F) => `InvalidChars`. `"`, `<`, `>` are NOT invalid (do not add them).
/// 3. `!is_path_fully_qualified(destination) && destination.contains(':')` => `InvalidChars` (drive-relative `a:b`,
///    `C:x`, `E:Backup`).
/// 4. `is_path_rooted(destination) && !is_path_fully_qualified(destination)` => `InvalidChars` (`\photos`, `/photos`:
///    rooted but no drive).
/// 5. `is_path_rooted(destination)` (here fully qualified: `D:\Backup`, `\\server\share\x`, `\\?\D:\Backup`,
///    `\\.\D:\Backup`) => `Ok` (an absolute destination is an explicit user choice; no `..` check).
/// 6. Split on `'\\'` and `'/'`; any segment whose `trim()` (Unicode whitespace both ends) is exactly `".."` =>
///    `EscapesSourceFolder` (`.. \x`, `a\ .. \x`, ` ..` all escape). Otherwise `Ok`.
pub fn validate(destination: Option<&str>) -> ActionDestinationCheck {
    let Some(destination) = destination else {
        return ActionDestinationCheck::Empty;
    };
    if destination.trim().is_empty() {
        return ActionDestinationCheck::Empty;
    }
    let body = destination.strip_prefix(r"\\?\").unwrap_or(destination);
    if body.chars().any(|c| c == '*' || c == '?' || is_invalid_path_char(c)) {
        return ActionDestinationCheck::InvalidChars;
    }
    let fully_qualified = is_path_fully_qualified(destination);
    if !fully_qualified && destination.contains(':') {
        return ActionDestinationCheck::InvalidChars;
    }
    let rooted = is_path_rooted(destination);
    if rooted && !fully_qualified {
        return ActionDestinationCheck::InvalidChars;
    }
    if rooted {
        return ActionDestinationCheck::Ok;
    }
    if destination.split(['\\', '/']).any(|segment| segment.trim() == "..") {
        return ActionDestinationCheck::EscapesSourceFolder;
    }
    ActionDestinationCheck::Ok
}

/// `ActionDestinationPolicy.ValidateNoEscapeViaReparsePoint(sourceFolder, destinationFolderOrPath, fileSystem)`.
/// Only for a RELATIVE destination (an absolute one may leave the photo folder).
/// * `source_folder` or `destination_folder_or_path` empty/whitespace-only => `Err(ArgumentError::BlankPath)` with
///   `param_name` `"sourceFolder"` / `"destinationFolderOrPath"` (checked in that order).
/// * `real_source` = `trim_ending_directory_separator(&file_system.resolve_real_path(Path::new(source))?)` and the same for
///   the destination (paths converted with `to_string_lossy`). ANY `io::Error` from either call (a link loop, access
///   denied, a path the OS rejects) => `Ok(EscapesSourceFolder)` (RV-S06: fail CLOSED, never an `Err`, never "inside").
///   (C# only catches `IOException`/`UnauthorizedAccessException`; an `ArgumentException` for a NUL char would escape,
///   but `validate` already rejects control characters before this is called, so Rust treats every `io::Error` alike.)
/// * `real_destination` equals `real_source` (OrdinalIgnoreCase) => `Ok(Ok)`.
/// * `prefix` = `real_source` if it already ends in a separator (a drive root `C:\`, R32), else `real_source + "\\"`.
///   `real_destination` starts with `prefix` (OrdinalIgnoreCase) => `Ok(Ok)`, else `Ok(EscapesSourceFolder)`
///   (`C:\photos2\x` is NOT inside `C:\photos`; `\\server\share2\sel` is not inside `\\server\share`).
pub fn validate_no_escape_via_reparse_point(
    source_folder: &str,
    destination_folder_or_path: &str,
    file_system: &dyn FileSystem,
) -> Result<ActionDestinationCheck, ArgumentError> {
    if source_folder.trim().is_empty() {
        return Err(ArgumentError::BlankPath {
            param_name: "sourceFolder",
        });
    }
    if destination_folder_or_path.trim().is_empty() {
        return Err(ArgumentError::BlankPath {
            param_name: "destinationFolderOrPath",
        });
    }
    // RV-S06: any resolve failure means containment cannot be proven => fail closed.
    let resolve = |p: &str| -> Option<String> {
        let real = file_system.resolve_real_path(Path::new(p)).ok()?;
        let text = real.to_string_lossy();
        Some(trim_ending_directory_separator(&text).to_owned())
    };
    let Some(real_source) = resolve(source_folder) else {
        return Ok(ActionDestinationCheck::EscapesSourceFolder);
    };
    let Some(real_destination) = resolve(destination_folder_or_path) else {
        return Ok(ActionDestinationCheck::EscapesSourceFolder);
    };
    if eq_ignore_case(&real_destination, &real_source) {
        return Ok(ActionDestinationCheck::Ok);
    }
    // R32: a drive root keeps its separator after trimming.
    let prefix = if ends_in_directory_separator(&real_source) {
        real_source
    } else {
        format!("{real_source}\\")
    };
    if ignore_case_key(&real_destination).starts_with(&ignore_case_key(&prefix)) {
        Ok(ActionDestinationCheck::Ok)
    } else {
        Ok(ActionDestinationCheck::EscapesSourceFolder)
    }
}

/// `ActionDestinationPolicy.ValidatePickedFolder(destination, photoFolder, directoryExists)`.
/// 1. `None`/empty/whitespace-only `destination` => `Empty`.
/// 2. `validate(Some(destination)) != Ok` or `!is_path_fully_qualified(destination)` => `NotAbsolute` (relative `Best`,
///    `..\Other`, rooted-without-drive `\Photos`, `E:Backup`, and `E:\a|b` all land here).
/// 3. `full` = `trim_ending_directory_separator(&get_full_path(destination)?)`, `photo` = the same for `photo_folder`.
///    Equal (OrdinalIgnoreCase) => `SameAsPhotoFolder` (`d:\photos\TRIP\`, `D:\Photos\Trip\Best\..`).
/// 4. `directory_exists(&full)` (called with the NORMALIZED text: `E:\Backup\` is asked as `E:\Backup`; a drive root keeps
///    its separator: `E:\`) => `Ok` or `Missing`.
///
/// `get_full_path` can fail only for paths the C# also throws on (too long): the error is returned, not mapped.
pub fn validate_picked_folder(
    destination: Option<&str>,
    photo_folder: &str,
    directory_exists: &dyn Fn(&str) -> bool,
) -> Result<PickedFolderCheck, PathError> {
    let Some(destination) = destination else {
        return Ok(PickedFolderCheck::Empty);
    };
    if destination.trim().is_empty() {
        return Ok(PickedFolderCheck::Empty);
    }
    if validate(Some(destination)) != ActionDestinationCheck::Ok || !is_path_fully_qualified(destination) {
        return Ok(PickedFolderCheck::NotAbsolute);
    }
    let full_path = get_full_path(destination)?;
    let full = trim_ending_directory_separator(&full_path);
    let photo_path = get_full_path(photo_folder)?;
    let photo = trim_ending_directory_separator(&photo_path);
    if eq_ignore_case(full, photo) {
        return Ok(PickedFolderCheck::SameAsPhotoFolder);
    }
    Ok(if directory_exists(full) {
        PickedFolderCheck::Ok
    } else {
        PickedFolderCheck::Missing
    })
}
