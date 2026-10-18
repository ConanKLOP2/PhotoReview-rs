//! Containment checks shared by every CLI mode that writes reports or owns a cache folder.
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/ToolPathGuard.cs` (C# baseline d92d6aae, 107 lines). Paths are compared after
//! resolving existing symlinks/junctions (`FileSystem::resolve_real_path`), so a junction that points into the photo folder
//! cannot hide a write target from a lexical prefix test (T-B-03, T-B-13). Violations are `InvalidOperationException`
//! (`ToolError::invalid_operation`), which every mode maps to exit code 2.

use std::io::ErrorKind;
use std::path::Path;

use pr_core::abstractions::FileSystem;
use pr_core::identity::{APP_DIR_NAME, CSHARP_APP_DIR_NAME};
use pr_core::text::dotnet_path::{self, PathError};
use pr_core::text::{eq_ignore_case, ignore_case_key};

use super::backend::ToolError;

/// Marker a tool drops into a cache folder it owns; a non-empty `--cache-dir` without it is refused.
pub const CACHE_MARKER: &str = ".photoreview-benchmark-cache";

/// Text of the marker file (written by [`claim_cache_directory`]).
pub const CACHE_MARKER_TEXT: &str =
    "created by the PhotoReview benchmark CLI; its cache and thumbnails folders are pruned\n";

/// `Path.GetFullPath` with its failures as the .NET exceptions the callers see: `ArgumentException` (NUL checked before
/// the blank rule) or `PathTooLongException`.
pub(crate) fn get_full_path(path: &str) -> Result<String, ToolError> {
    dotnet_path::get_full_path(path).map_err(|error| match error {
        PathError::Argument if path.contains('\0') => ToolError::argument("Null character in path. (Parameter 'path')"),
        PathError::Argument => ToolError::argument("The path is empty. (Parameter 'path')"),
        PathError::TooLong => ToolError::new(
            "PathTooLongException",
            "The path is too long, or a component of the specified path is too long.",
        ),
    })
}

/// The absolute path with every existing reparse point resolved and no trailing separator
/// (`Path.TrimEndingDirectorySeparator(fs.ResolveRealPath(path))`). A failing resolve is an `IOException`.
pub fn real(fs: &dyn FileSystem, path: &str) -> Result<String, ToolError> {
    // `ArgumentException.ThrowIfNullOrWhiteSpace(path)` of `ResolveRealPath`, then the NUL check of `Path.GetFullPath`.
    if path.trim().is_empty() {
        return Err(ToolError::argument(
            "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'path')",
        ));
    }
    if path.contains('\0') {
        return Err(ToolError::argument("Null character in path. (Parameter 'path')"));
    }
    match fs.resolve_real_path(Path::new(path)) {
        Ok(resolved) => {
            let text = resolved.to_string_lossy();
            Ok(dotnet_path::trim_ending_directory_separator(&text).to_owned())
        }
        Err(error) => Err(map_resolve_error(&error, path)),
    }
}

/// A failed `resolve_real_path`: `InvalidInput` (a blank or NUL-bearing path) is an `ArgumentException`, everything else an
/// `IOException` family member through [`ToolError::from_io`].
pub(crate) fn map_resolve_error(error: &std::io::Error, path: &str) -> ToolError {
    if error.kind() == ErrorKind::InvalidInput {
        ToolError::argument(error.to_string())
    } else {
        ToolError::from_io(error, path, "DirectoryNotFoundException")
    }
}

/// True when `path` equals `root` or lies below it (both through [`real`], OrdinalIgnoreCase via `pr_core::text`). The prefix is
/// `root` plus one backslash, so `C:\a\photos-reports` is NOT under `C:\a\photos` (golden `sibling-with-same-prefix`).
pub fn is_same_or_under(fs: &dyn FileSystem, path: &str, root: &str) -> Result<bool, ToolError> {
    let p = real(fs, path)?;
    let r = real(fs, root)?;
    if eq_ignore_case(&p, &r) {
        return Ok(true);
    }
    let prefix = if r.ends_with('\\') { r } else { format!("{r}\\") };
    Ok(ignore_case_key(&p).starts_with(&ignore_case_key(&prefix)))
}

/// True when either path equals, contains or lies inside the other.
pub fn overlaps(fs: &dyn FileSystem, a: &str, b: &str) -> Result<bool, ToolError> {
    Ok(is_same_or_under(fs, a, b)? || is_same_or_under(fs, b, a)?)
}

/// A report directory must not equal, sit inside or contain the photo folder. Error text:
/// `output directory '{output_directory}' must not equal, contain or sit inside the photo folder '{photo_folder}'`.
pub fn ensure_output_directory(
    fs: &dyn FileSystem,
    output_directory: &str,
    photo_folder: &str,
) -> Result<(), ToolError> {
    if overlaps(fs, output_directory, photo_folder)? {
        return Err(ToolError::invalid_operation(format!(
            "output directory '{output_directory}' must not equal, contain or sit inside the photo folder '{photo_folder}'"
        )));
    }
    Ok(())
}

/// A report file must not live in the photo folder or below it: `get_directory_name(get_full_path(output_file))` (falling back to
/// the full path itself) is tested with [`is_same_or_under`]. Error text:
/// `output file '{output_file}' must not be inside the photo folder '{photo_folder}'`.
pub fn ensure_output_file(fs: &dyn FileSystem, output_file: &str, photo_folder: &str) -> Result<(), ToolError> {
    let full = get_full_path(output_file)?;
    let directory = dotnet_path::get_directory_name(&full).unwrap_or(full);
    if is_same_or_under(fs, &directory, photo_folder)? {
        return Err(ToolError::invalid_operation(format!(
            "output file '{output_file}' must not be inside the photo folder '{photo_folder}'"
        )));
    }
    Ok(())
}

/// `--cache-dir` folders are pruned and cleaned, so the directory must be disposable: it must not overlap any of
/// `must_not_overlap` (`(path, display name)`, checked in order; error `--cache-dir '{cache_dir}' must not equal, contain or sit
/// inside the {name} '{path}'`) and must be missing, empty, or carry [`CACHE_MARKER`] (otherwise error `--cache-dir '{cache_dir}' is
/// not empty and was not created by this tool (no .photoreview-benchmark-cache marker); its cache folders would be pruned. Use an
/// empty folder`). "Empty" = no file and no directory entry (`enumerate_files(dir, "*")` + `enumerate_directories(dir)`).
pub fn ensure_cache_directory(
    fs: &dyn FileSystem,
    cache_dir: &str,
    must_not_overlap: &[(&str, &str)],
) -> Result<(), ToolError> {
    for (path, name) in must_not_overlap {
        if overlaps(fs, cache_dir, path)? {
            return Err(ToolError::invalid_operation(format!(
                "--cache-dir '{cache_dir}' must not equal, contain or sit inside the {name} '{path}'"
            )));
        }
    }
    let dir = Path::new(cache_dir);
    if fs.directory_exists(dir) && !fs.file_exists(&dir.join(CACHE_MARKER)) && has_entries(fs, dir, cache_dir)? {
        return Err(ToolError::invalid_operation(format!(
            "--cache-dir '{cache_dir}' is not empty and was not created by this tool (no {CACHE_MARKER} marker); its cache folders would be pruned. Use an empty folder"
        )));
    }
    Ok(())
}

/// `Directory.EnumerateFileSystemEntries(dir).Any()`.
fn has_entries(fs: &dyn FileSystem, dir: &Path, display: &str) -> Result<bool, ToolError> {
    let io_error = |error: std::io::Error| ToolError::from_io(&error, display, "DirectoryNotFoundException");
    if !fs.enumerate_files(dir, "*").map_err(io_error)?.is_empty() {
        return Ok(true);
    }
    Ok(!fs.enumerate_directories(dir).map_err(io_error)?.is_empty())
}

/// Creates the cache folder and its marker file (idempotent: an existing marker is left alone), UTF-8 without BOM.
///
/// Errors follow `Directory.CreateDirectory` / `File.WriteAllText`: an empty path is
/// `ArgumentException("The value cannot be an empty string. (Parameter 'path')")` BEFORE anything is created (it must never
/// resolve to the current directory), a blank or NUL-bearing one the `Path.GetFullPath` `ArgumentException`s, and a file in
/// the way is `IOException("Cannot create '<that file>' because a file or directory with the same name already exists.")`.
/// IO messages name the full path.
pub fn claim_cache_directory(cache_dir: &str) -> Result<(), ToolError> {
    if cache_dir.is_empty() {
        return Err(ToolError::argument(
            "The value cannot be an empty string. (Parameter 'path')",
        ));
    }
    let full = get_full_path(cache_dir)?;
    let io_error = |error: std::io::Error, path: &str| ToolError::from_io(&error, path, "DirectoryNotFoundException");
    if let Err(error) = std::fs::create_dir_all(&full) {
        // `CreateDirectory` names the existing FILE that blocks the path (the leaf itself or an ancestor).
        let blocker = Path::new(&full).ancestors().find(|ancestor| ancestor.is_file());
        return Err(match blocker {
            Some(file) => ToolError::from_io(
                &std::io::Error::from(ErrorKind::AlreadyExists),
                &file.to_string_lossy(),
                "DirectoryNotFoundException",
            ),
            None => io_error(error, &full),
        });
    }
    let marker = Path::new(&full).join(CACHE_MARKER);
    if !marker.is_file() {
        let marker_text = marker.to_string_lossy().into_owned();
        std::fs::write(&marker, CACHE_MARKER_TEXT).map_err(|error| io_error(error, &marker_text))?;
    }
    Ok(())
}

/// Writes a report file without ever writing THROUGH its final path (R13): the text goes to a sibling temp file
/// `{full}.{first 8 of guid N}.tmp`, which then replaces the leaf (`std::fs::rename` over an existing file; a leaf that is a
/// symlink or hard link to a photo is replaced by a regular file and the photo's bytes stay untouched). UTF-8, no BOM, text
/// written as given (no newline conversion). The temp file is deleted when it still exists after a failure.
///
/// Errors follow .NET: the temp write maps like `File.WriteAllTextAsync` (an `IOException` without a more specific type reads
/// `{system message} : '{temp path}'.`), the replace like `File.Move(overwrite: true)`: an existing directory, a read-only leaf
/// and a leaf another handle holds open with `FILE_SHARE_DELETE` (which POSIX-style `rename` would silently replace) are
/// `UnauthorizedAccessException("Access to the path is denied.")`, with no path in the text.
pub fn write_report_file(path: &str, content: &str) -> Result<(), ToolError> {
    let full = get_full_path(path)?;
    let guid = pr_core::io::physical::new_guid_n();
    let short: String = guid.chars().take(8).collect();
    let temp = format!("{full}.{short}.tmp");
    let result = std::fs::write(&temp, content.as_bytes())
        .map_err(|error| map_write_error(&error, &temp))
        .and_then(|()| replace_leaf(&temp, &full));
    if Path::new(&temp).is_file() {
        // Best effort, like the C# `finally`; the original error (if any) is what the caller needs.
        let _ = std::fs::remove_file(&temp);
    }
    result
}

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PATH_NOT_FOUND: i32 = 3;
const ERROR_BAD_NETPATH: i32 = 53;
const ERROR_BAD_NET_NAME: i32 = 67;

/// `new IOException("{system message} : '{path}'.")`, what .NET builds for an error code without a dedicated exception type.
fn io_exception_with_path(error: &std::io::Error, path: &str) -> ToolError {
    ToolError::new(
        "IOException",
        format!("{} : '{path}'.", super::backend::os_error_description(error)),
    )
}

/// Failures of writing the temp file, typed like `File.WriteAllTextAsync` (`Win32Marshal.GetExceptionForWin32Error`).
pub(crate) fn map_write_error(error: &std::io::Error, path: &str) -> ToolError {
    match error.raw_os_error() {
        Some(ERROR_FILE_NOT_FOUND) => ToolError::from_io(error, path, "FileNotFoundException"),
        Some(ERROR_PATH_NOT_FOUND) => ToolError::from_io(error, path, "DirectoryNotFoundException"),
        Some(ERROR_BAD_NETPATH | ERROR_BAD_NET_NAME) => io_exception_with_path(error, path),
        // Sharing and lock violations have their own text.
        Some(32 | 33) => ToolError::from_io(error, path, "DirectoryNotFoundException"),
        _ => match error.kind() {
            ErrorKind::NotFound | ErrorKind::PermissionDenied | ErrorKind::AlreadyExists => {
                ToolError::from_io(error, path, "DirectoryNotFoundException")
            }
            _ => io_exception_with_path(error, path),
        },
    }
}

/// `File.Move(temp, full, overwrite: true)`.
fn replace_leaf(temp: &str, full: &str) -> Result<(), ToolError> {
    if leaf_refuses_replacement(full) {
        return Err(access_denied_without_path());
    }
    std::fs::rename(temp, full).map_err(|error| {
        if error.kind() == ErrorKind::PermissionDenied {
            access_denied_without_path()
        } else {
            ToolError::from_io(&error, full, "FileNotFoundException")
        }
    })
}

/// `File.Move` reports `Access to the path is denied.` WITHOUT the path.
fn access_denied_without_path() -> ToolError {
    ToolError::new("UnauthorizedAccessException", "Access to the path is denied.")
}

/// True when `MoveFileEx(MOVEFILE_REPLACE_EXISTING)` would answer `ERROR_ACCESS_DENIED` for the existing `full`: a directory (also a
/// directory link), a read-only file, or a file another handle holds open WITH `FILE_SHARE_DELETE` (Rust's `rename` has POSIX
/// semantics and would replace it). A leaf held open without `FILE_SHARE_DELETE` is left to `rename`, which fails with a
/// sharing violation like .NET does.
#[cfg(windows)]
fn leaf_refuses_replacement(full: &str) -> bool {
    use std::fs::OpenOptions;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const DELETE: u32 = 0x1_0000;
    const FILE_SHARE_ALL: u32 = 0x7;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const ERROR_SHARING_VIOLATION: i32 = 32;

    let Ok(metadata) = std::fs::symlink_metadata(full) else {
        return false;
    };
    if metadata.file_attributes() & (FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return true;
    }
    let open = |share: u32| {
        OpenOptions::new()
            .access_mode(DELETE)
            .share_mode(share)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(full)
    };
    // Opening for DELETE while allowing every share succeeds only when no other handle denies delete sharing; a second open that
    // allows nothing then fails exactly when some other handle exists.
    match open(FILE_SHARE_ALL) {
        Ok(probe) => {
            drop(probe);
            matches!(open(0), Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION))
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn leaf_refuses_replacement(full: &str) -> bool {
    std::fs::symlink_metadata(full).is_ok_and(|metadata| metadata.is_dir() || metadata.permissions().readonly())
}

/// `Path.Combine(Environment.GetFolderPath(LocalApplicationData), name)`.
fn local_app_data_child(name: &str) -> String {
    combine_with_base(std::env::var_os("LOCALAPPDATA").as_deref(), name)
}

/// `Path.Combine(base, name)` where an unset or empty `base` leaves just `name`.
fn combine_with_base(base: Option<&std::ffi::OsStr>, name: &str) -> String {
    match base {
        Some(base) if !base.is_empty() => {
            let base = base.to_string_lossy();
            if dotnet_path::ends_in_directory_separator(&base) {
                format!("{base}{name}")
            } else {
                format!("{base}\\{name}")
            }
        }
        _ => name.to_owned(),
    }
}

/// The Rust app's data folder `%LOCALAPPDATA%\PhotoReviewRs` (`pr_core::identity::APP_DIR_NAME`). No tool-owned folder may overlap it.
pub fn app_data_folder() -> String {
    local_app_data_child(APP_DIR_NAME)
}

/// The C# app's data folder `%LOCALAPPDATA%\PhotoReview` (`ToolPathGuard.AppDataFolder` in C#). Also protected (R0002: never touch it).
pub fn csharp_app_data_folder() -> String {
    local_app_data_child(CSHARP_APP_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::io::{Error, ErrorKind};

    use super::*;

    #[test]
    fn combine_with_base_follows_path_combine() {
        assert_eq!(combine_with_base(None, "PhotoReview"), "PhotoReview");
        assert_eq!(combine_with_base(Some(OsStr::new("")), "PhotoReview"), "PhotoReview");
        assert_eq!(
            combine_with_base(Some(OsStr::new("C:\\L")), "PhotoReview"),
            "C:\\L\\PhotoReview"
        );
        assert_eq!(
            combine_with_base(Some(OsStr::new("C:\\L\\")), "PhotoReview"),
            "C:\\L\\PhotoReview"
        );
    }

    #[test]
    fn app_data_folders_sit_under_local_app_data() {
        let Some(base) = std::env::var_os("LOCALAPPDATA").filter(|b| !b.is_empty()) else {
            return;
        };
        let base = base.to_string_lossy().trim_end_matches('\\').to_owned();
        assert_eq!(app_data_folder(), format!("{base}\\{APP_DIR_NAME}"));
        assert_eq!(csharp_app_data_folder(), format!("{base}\\{CSHARP_APP_DIR_NAME}"));
    }

    #[test]
    fn resolve_errors_map_by_kind() {
        let invalid = map_resolve_error(&Error::new(ErrorKind::InvalidInput, "bad path"), "p");
        assert_eq!(
            (invalid.type_name.as_str(), invalid.message.as_str()),
            ("ArgumentException", "bad path")
        );
        let denied = map_resolve_error(&Error::from(ErrorKind::PermissionDenied), "p");
        assert_eq!(denied.type_name, "UnauthorizedAccessException");
        let other = map_resolve_error(&Error::from_raw_os_error(1921), "p");
        assert_eq!(other.type_name, "IOException");
    }

    // Kind-only mapping of `File.WriteAllTextAsync` failures (the `_` arm of `map_write_error`): access denied, an existing
    // name and a code-less NotFound keep their dedicated .NET texts; an unknown kind is the generic IOException with suffix.
    #[test]
    fn write_errors_map_by_kind_like_dotnet() {
        let denied = map_write_error(&Error::from_raw_os_error(5), "p");
        assert_eq!(
            (denied.type_name.as_str(), denied.message.as_str()),
            ("UnauthorizedAccessException", "Access to the path 'p' is denied.")
        );
        let exists = map_write_error(&Error::from_raw_os_error(183), "p");
        assert_eq!(
            (exists.type_name.as_str(), exists.message.as_str()),
            (
                "IOException",
                "Cannot create 'p' because a file or directory with the same name already exists."
            )
        );
        let not_found = map_write_error(&Error::from(ErrorKind::NotFound), "p");
        assert_eq!(
            (not_found.type_name.as_str(), not_found.message.as_str()),
            ("DirectoryNotFoundException", "Could not find a part of the path 'p'.")
        );
        let other = map_write_error(&Error::from(ErrorKind::TimedOut), "p");
        assert_eq!(other.type_name, "IOException");
        assert!(other.message.ends_with(" : 'p'."), "{}", other.message);
    }

    #[test]
    fn write_errors_map_like_dotnet() {
        let missing_file = map_write_error(&Error::from_raw_os_error(2), "p");
        assert_eq!(missing_file.type_name, "FileNotFoundException");
        let missing_dir = map_write_error(&Error::from_raw_os_error(3), "p");
        assert_eq!(missing_dir.type_name, "DirectoryNotFoundException");
        let share = map_write_error(&Error::from_raw_os_error(67), "p");
        assert_eq!(share.type_name, "IOException");
        assert_eq!(share.message, "The network name cannot be found. : 'p'.");
        let syntax = map_write_error(&Error::from_raw_os_error(123), "p");
        assert_eq!(syntax.type_name, "IOException");
        assert_eq!(
            syntax.message,
            "The filename, directory name, or volume label syntax is incorrect. : 'p'."
        );
        let sharing = map_write_error(&Error::from_raw_os_error(32), "p");
        assert!(
            sharing.message.starts_with("The process cannot access the file 'p'"),
            "{}",
            sharing.message
        );
    }
}
