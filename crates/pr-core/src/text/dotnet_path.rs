//! .NET `System.IO.Path` on Windows, as the C# catalog and file-action code uses it (shared helper, proposed unit
//! W2-00; contracts/W2-00.md). C# baseline d92d6aae calls these in `ImageSortService`, `SiblingFolderService`,
//! `CaptureGroupBuilder`, `ComparePairService`, `DuplicateFinder`, `ActionDestinationPolicy`,
//! `ExplorerSnapshotValidator`, `DragDropInputService` and `ReviewCatalog`.
//!
//! Every function works on `&str` (D1: paths in records are `String`) and reproduces the .NET result, not the result
//! of `std::path::Path` (whose `parent()`/`file_name()` differ on `C:`, `//`, `\\?\` and trailing separators).
//! Behaviour is pinned by `tests/golden/dotnet-path.json` (oracle command `dotnet-path`).
//! Already ported elsewhere and NOT repeated here: `Path.GetExtension` ([`crate::catalog::types::get_extension`]) and
//! `Path.GetPathRoot` ([`crate::file_actions::errors::get_path_root`]).
use std::fmt;

/// `Path.GetFullPath` rejected its argument. Both C# exception types are `catch`ed by the same `when (ex is
/// ArgumentException or NotSupportedException or PathTooLongException)` filters in the callers, so they are one value
/// here per kind of cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// `ArgumentException` (and `ArgumentNullException`): the path is empty, only `' '` (U+0020) characters, or contains
    /// a NUL character.
    Argument,
    /// `PathTooLongException` (an `IOException`): the resolved path is too long for the OS.
    TooLong,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::Argument => f.write_str("The path is empty, only spaces, or contains a null character."),
            PathError::TooLong => f.write_str("The path is too long."),
        }
    }
}

impl std::error::Error for PathError {}

/// `Path.DirectorySeparatorChar`.
pub const DIRECTORY_SEPARATOR: char = '\\';
/// `Path.AltDirectorySeparatorChar`.
pub const ALT_DIRECTORY_SEPARATOR: char = '/';

/// `PathInternal.IsDirectorySeparator`: `'\\'` or `'/'`.
pub fn is_directory_separator(c: char) -> bool {
    c == DIRECTORY_SEPARATOR || c == ALT_DIRECTORY_SEPARATOR
}

/// `Path.GetInvalidPathChars()` on Windows (.NET 10): `'|'` and U+0000..=U+001F. NOT `"`, `<`, `>`, `*`, `?`.
pub fn is_invalid_path_char(c: char) -> bool {
    c == '|' || (c as u32) < 0x20
}

/// `PathInternal.GetRootLength(path)`: the length in BYTES of `path`'s root; `0` when there is none. Same rules as
/// [`crate::file_actions::errors::get_path_root`] without separator normalization and without the "only spaces" rule.
pub fn root_length(path: &str) -> usize {
    let b = path.as_bytes();
    let len = b.len();
    let sep_at = |i: usize| b.get(i).is_some_and(|&c| c == b'\\' || c == b'/');
    let explicit_device =
        b.first() == Some(&b'\\') && b.get(1) == Some(&b'?') && b.get(2) == Some(&b'?') && b.get(3) == Some(&b'\\');
    let general_device = sep_at(0) && sep_at(1) && b.get(2).is_some_and(|&c| c == b'.' || c == b'?') && sep_at(3);
    let device = explicit_device || general_device;
    let device_unc = device && len >= 8 && b[4..7] == *b"UNC" && sep_at(7);

    if (!device || device_unc) && sep_at(0) {
        if device_unc || sep_at(1) {
            // UNC: the second separator after the prefix ends the share name.
            let mut i = if device_unc { 8 } else { 2 };
            let mut separators_left = 2_u32;
            while i < len {
                if !sep_at(i) {
                    i += 1;
                    continue;
                }
                separators_left -= 1;
                if separators_left == 0 {
                    break;
                }
                i += 1;
            }
            i
        } else {
            1
        }
    } else if device {
        let mut i = 4;
        while i < len && !sep_at(i) {
            i += 1;
        }
        if i < len && i > 4 {
            i += 1;
        }
        i
    } else if len >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        if len > 2 && sep_at(2) { 3 } else { 2 }
    } else {
        0
    }
}

/// `Path.GetFullPath(path)`.
///
/// Errors: [`PathError::Argument`] when `path` is empty, only `' '` characters, or contains `'\0'`;
/// [`PathError::TooLong`] when the OS rejects the path. Otherwise `std::path::absolute` (`GetFullPathNameW`, like .NET;
/// `\\?\` paths are returned unchanged).
pub fn get_full_path(path: &str) -> Result<String, PathError> {
    if path.chars().all(|c| c == ' ') || path.contains('\0') {
        return Err(PathError::Argument);
    }
    match std::path::absolute(path) {
        Ok(p) => Ok(p.to_string_lossy().into_owned()),
        Err(_) => Err(PathError::TooLong),
    }
}

/// `Path.GetDirectoryName(string)`: `None` for an empty / only-spaces path or a root; `Some("")` for a bare name.
/// The result has its separators normalized (`C:/a/b.jpg` => `C:\a`).
pub fn get_directory_name(path: &str) -> Option<String> {
    if path.chars().all(|c| c == ' ') {
        return None;
    }
    let end = directory_name_end(path)?;
    Some(normalize_directory_separators(&path[..end]))
}

/// `Path.GetDirectoryName(ReadOnlySpan<char>)`: as [`get_directory_name`] without normalization, `""` instead of
/// null.
pub fn get_directory_name_raw(path: &str) -> &str {
    match directory_name_end(path) {
        Some(end) => &path[..end],
        None => "",
    }
}

/// `Path.GetFileName(string)`: the text after the last separator that is not inside the root.
pub fn get_file_name(path: &str) -> &str {
    let root = root_length(path);
    match path.rfind(['\\', '/']) {
        Some(i) if i >= root => &path[i + 1..],
        _ => &path[root..],
    }
}

/// `Path.GetFileNameWithoutExtension(string)`: [`get_file_name`] cut at its LAST `'.'`.
pub fn get_file_name_without_extension(path: &str) -> &str {
    let name = get_file_name(path);
    match name.rfind('.') {
        Some(i) => &name[..i],
        None => name,
    }
}

/// `Path.EndsInDirectorySeparator`: the last char is `'\\'` or `'/'`; `""` is false.
pub fn ends_in_directory_separator(path: &str) -> bool {
    path.chars().next_back().is_some_and(is_directory_separator)
}

/// `Path.TrimEndingDirectorySeparator`: removes exactly ONE trailing separator, never from a root.
pub fn trim_ending_directory_separator(path: &str) -> &str {
    if ends_in_directory_separator(path) && path.len() != root_length(path) {
        &path[..path.len() - 1]
    } else {
        path
    }
}

/// `Path.IsPathRooted`: starts with a separator, or an ASCII letter followed by `':'`.
pub fn is_path_rooted(path: &str) -> bool {
    let b = path.as_bytes();
    matches!(b.first(), Some(b'\\' | b'/')) || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// `Path.IsPathFullyQualified` (`!PathInternal.IsPartiallyQualified`).
pub fn is_path_fully_qualified(path: &str) -> bool {
    let b = path.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    if b.len() < 2 {
        return false;
    }
    if sep(b[0]) {
        return b[1] == b'?' || sep(b[1]);
    }
    b.len() >= 3 && b[1] == b':' && sep(b[2]) && b[0].is_ascii_alphabetic()
}

/// `Path.GetDirectoryNameOffset`: byte end of the directory part, `None` when `path` is empty or a root.
fn directory_name_end(path: &str) -> Option<usize> {
    let b = path.as_bytes();
    let root = root_length(path);
    let mut end = b.len();
    if end <= root {
        return None;
    }
    let is_sep = |c: u8| c == b'\\' || c == b'/';
    while end > root {
        end -= 1;
        if is_sep(b[end]) {
            break;
        }
    }
    while end > root && is_sep(b[end - 1]) {
        end -= 1;
    }
    Some(end)
}

/// `PathInternal.NormalizeDirectorySeparators`: `/` becomes `\`, runs of separators collapse to one.
fn normalize_directory_separators(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let chars: Vec<char> = path.chars().collect();
    let mut start = 0;
    if chars.first().is_some_and(|&c| is_directory_separator(c)) {
        start = 1;
        out.push(DIRECTORY_SEPARATOR);
    }
    for i in start..chars.len() {
        let mut c = chars[i];
        if is_directory_separator(c) {
            if chars.get(i + 1).is_some_and(|&n| is_directory_separator(n)) {
                continue;
            }
            c = DIRECTORY_SEPARATOR;
        }
        out.push(c);
    }
    out
}
