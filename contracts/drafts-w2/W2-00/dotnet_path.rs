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
// Temporary (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

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
    todo!("c == DIRECTORY_SEPARATOR || c == ALT_DIRECTORY_SEPARATOR")
}

/// `Path.GetInvalidPathChars()` on Windows (.NET 10): `'|'` and U+0000..=U+001F. NOT `"`, `<`, `>` (older .NET
/// Framework listed them), NOT `*` and `?`. Verified with the oracle (`dotnet-path` command, field
/// `invalid_path_chars`).
pub fn is_invalid_path_char(c: char) -> bool {
    todo!("c == '|' || (c as u32) < 0x20")
}

/// `PathInternal.GetRootLength(path)`: the length in BYTES of `path`'s root (so `&path[..root_length(path)]` is the
/// raw, un-normalized root); `0` when there is none. Same rules as [`crate::file_actions::errors::get_path_root`]
/// (device paths `\\?\`, `\\.\`, `\??\`; `\\?\UNC\server\share`; UNC `\\server\share`; `\foo` => 1; `C:` => 2;
/// `C:\` => 3), without the separator normalization and without the "only spaces => None" rule.
pub fn root_length(path: &str) -> usize {
    todo!()
}

/// `Path.GetFullPath(path)`.
///
/// Errors: [`PathError::Argument`] when `path` is empty or consists only of `' '` characters
/// (`PathInternal.IsEffectivelyEmpty`; a path of tabs or NBSP is NOT empty and resolves against the current
/// directory), or contains `'\0'`; [`PathError::TooLong`] when the OS rejects the length.
/// Behaviour (verified with .NET 10, see golden `dotnet-path`): `/` becomes `\`, runs of separators collapse
/// (`C:/a//b` => `C:\a\b`), `.` and `..` segments are resolved (`C:\a\..\..\..` => `C:\`), trailing dots and spaces of
/// the last segment are removed (`C:\a. ` => `C:\a`), a trailing separator is kept (`C:\a\b\` => `C:\a\b\`), a path
/// that starts with `\\?\` (extended-length) is returned UNCHANGED (no `..` resolution, no slash conversion), a
/// `\\.\` device path IS normalized, `<`, `>`, `|`, `*`, `?` do not make it fail. A relative path (and `\x`, `C:x`)
/// is resolved against the current directory of the process / of that drive. Suggested implementation:
/// `std::path::absolute` after the two pre-checks (it calls `GetFullPathNameW` like .NET and leaves `\\?\` paths
/// alone); the golden decides any disagreement.
pub fn get_full_path(path: &str) -> Result<String, PathError> {
    todo!()
}

/// `Path.GetDirectoryName(string)`: `None` for an empty path, an only-spaces path, or a root (`C:\`, `\\srv\share`,
/// `\\?\C:\`); otherwise the part before the last separator, with the root kept and with trailing separators removed
/// (`C:\a\\b.jpg` => `C:\a`), and the result NORMALIZED like [`get_path_root`](crate::file_actions::errors::get_path_root)
/// does for the root: `/` becomes `\`, doubled separators collapse (`C:/a/b.jpg` => `C:\a`). Returns `Some("")` for a
/// bare file name (`a.jpg`). `C:a` => `Some("C:")`, `C:\a` => `Some("C:\")`, `\a` => `Some("\")`.
/// Algorithm (`Path.GetDirectoryNameOffset`): `root = root_length(path)`; if `path.len() <= root` => `None`; scan back
/// from the end to the previous separator (not below `root`), then drop remaining separators before it (not below
/// `root`); then normalize separators of `path[..end]` (`PathInternal.NormalizeDirectorySeparators`: convert `/` to
/// `\`, collapse runs of separators, but a leading `\\` UNC prefix is kept as two).
pub fn get_directory_name(path: &str) -> Option<String> {
    todo!()
}

/// `Path.GetDirectoryName(ReadOnlySpan<char>)`: as [`get_directory_name`] but WITHOUT separator normalization and
/// returning `""` instead of null (`C:\` => `""`). `ExplorerSnapshotValidator.InFolder` uses this overload.
pub fn get_directory_name_raw(path: &str) -> &str {
    todo!()
}

/// `Path.GetFileName(string)`: the text after the last separator that is not inside the root; `""` when the path
/// ends in a separator or is a bare root. A `':'` only counts as a separator through the root (`C:b.jpg` =>
/// `b.jpg`, `C:\a\b:c.jpg` => `b:c.jpg`, `a:b.jpg` => `b.jpg`). Algorithm: `root = root_length(path)`;
/// `i` = byte index of the last `'\\'` or `'/'`; result = `path[(if i < root { root } else { i + 1 })..]`
/// (`i` absent counts as below `root`).
pub fn get_file_name(path: &str) -> &str {
    todo!()
}

/// `Path.GetFileNameWithoutExtension(string)`: [`get_file_name`] with everything from its LAST `'.'` removed (`a (1).JPG`
/// => `a (1)`, `.jpg` => `""`, `a.` => `a`, `a..jpg` => `a.`, `a.b.c` => `a.b`, no dot => unchanged).
pub fn get_file_name_without_extension(path: &str) -> &str {
    todo!()
}

/// `Path.EndsInDirectorySeparator`: the last char is `'\\'` or `'/'`; `""` is false.
pub fn ends_in_directory_separator(path: &str) -> bool {
    todo!()
}

/// `Path.TrimEndingDirectorySeparator`: removes exactly ONE trailing separator, unless `path` is a root
/// (`path.len() == root_length(path)`): `C:\a\` => `C:\a`, `C:\a\\` => `C:\a\`, `C:\` stays, `\\srv\share\` =>
/// `\\srv\share`, `\\srv\` stays (it is the root), `\\?\C:\` stays, `/` stays, `a/` => `a`.
pub fn trim_ending_directory_separator(path: &str) -> &str {
    todo!()
}

/// `Path.IsPathRooted`: `path` starts with a separator, or its first char is an ASCII letter and its SECOND char is
/// `':'` (`a:b`, `C:`, `E:Backup` are rooted; `1:\a` is NOT, the drive char must be a letter). `""` and one-char
/// non-separators are false.
pub fn is_path_rooted(path: &str) -> bool {
    todo!()
}

/// `Path.IsPathFullyQualified` (`!PathInternal.IsPartiallyQualified`). Partially qualified (=> false) when: the path
/// is shorter than 2 chars; or it starts with a separator and its second char is neither `'?'` nor a separator
/// (`\a`, `/a`); or it does not start with a separator and is NOT of the form `<ASCII letter>:<separator>...`
/// (needs at least 3 chars: `C:a`, `C:`, `1:\a`, `a:b`, `..\x` are partial). Everything else is fully qualified:
/// `C:\a`, `E:\a|b`, `\\srv\share`, `\\`, `//x/y`, `\\?\C:`, `\??\C:\a`, `\\.\D:\x`.
pub fn is_path_fully_qualified(path: &str) -> bool {
    todo!()
}
