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
    match fs.resolve_real_path(Path::new(path)) {
        Ok(resolved) => {
            let text = resolved.to_string_lossy();
            Ok(dotnet_path::trim_ending_directory_separator(&text).to_owned())
        }
        Err(error) if error.kind() == ErrorKind::InvalidInput => Err(ToolError::argument(error.to_string())),
        Err(error) => Err(ToolError::from_io(&error, path, "DirectoryNotFoundException")),
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
pub fn claim_cache_directory(cache_dir: &str) -> Result<(), ToolError> {
    let io_error = |error: std::io::Error, path: &str| ToolError::from_io(&error, path, "DirectoryNotFoundException");
    std::fs::create_dir_all(cache_dir).map_err(|error| io_error(error, cache_dir))?;
    let marker = Path::new(cache_dir).join(CACHE_MARKER);
    if !marker.is_file() {
        let marker_text = marker.to_string_lossy().into_owned();
        std::fs::write(&marker, CACHE_MARKER_TEXT).map_err(|error| io_error(error, &marker_text))?;
    }
    Ok(())
}

/// Writes a report file without ever writing THROUGH its final path (R13): the text goes to a sibling temp file
/// `{full}.{first 8 of guid N}.tmp`, which then replaces the leaf (`std::fs::rename` over an existing file; a leaf that is a
/// symlink or hard link to a photo is replaced by a regular file and the photo's bytes stay untouched). UTF-8, no BOM, text
/// written as given (no newline conversion). The temp file is deleted when it still exists after a failure. I/O errors become
/// `ToolError::from_io`.
pub fn write_report_file(path: &str, content: &str) -> Result<(), ToolError> {
    let full = get_full_path(path)?;
    let guid = pr_core::io::physical::new_guid_n();
    let short: String = guid.chars().take(8).collect();
    let temp = format!("{full}.{short}.tmp");
    let result = std::fs::write(&temp, content.as_bytes())
        .map_err(|error| ToolError::from_io(&error, &temp, "DirectoryNotFoundException"))
        .and_then(|()| {
            std::fs::rename(&temp, &full).map_err(|error| ToolError::from_io(&error, &full, "FileNotFoundException"))
        });
    if Path::new(&temp).is_file() {
        // Best effort, like the C# `finally`; the original error (if any) is what the caller needs.
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// `Path.Combine(Environment.GetFolderPath(LocalApplicationData), name)`.
fn local_app_data_child(name: &str) -> String {
    match std::env::var_os("LOCALAPPDATA") {
        Some(base) if !base.is_empty() => {
            let base = base.to_string_lossy().into_owned();
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
