//! Containment checks shared by every CLI mode that writes reports or owns a cache folder.
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/ToolPathGuard.cs` (C# baseline d92d6aae, 107 lines). Paths are compared after
//! resolving existing symlinks/junctions (`FileSystem::resolve_real_path`), so a junction that points into the photo folder
//! cannot hide a write target from a lexical prefix test (T-B-03, T-B-13). Violations are `InvalidOperationException`
//! (`ToolError::invalid_operation`), which every mode maps to exit code 2.

use pr_core::abstractions::FileSystem;

use super::backend::ToolError;

/// Marker a tool drops into a cache folder it owns; a non-empty `--cache-dir` without it is refused.
pub const CACHE_MARKER: &str = ".photoreview-benchmark-cache";

/// Text of the marker file (written by [`claim_cache_directory`]).
pub const CACHE_MARKER_TEXT: &str =
    "created by the PhotoReview benchmark CLI; its cache and thumbnails folders are pruned\n";

/// The absolute path with every existing reparse point resolved and no trailing separator
/// (`Path.TrimEndingDirectorySeparator(fs.ResolveRealPath(path))`). A failing resolve is an `IOException`.
pub fn real(fs: &dyn FileSystem, path: &str) -> Result<String, ToolError> {
    todo!("W7-03d")
}

/// True when `path` equals `root` or lies below it (both through [`real`], OrdinalIgnoreCase via `pr_core::text`). The prefix is
/// `root` plus one backslash, so `C:\a\photos-reports` is NOT under `C:\a\photos` (golden `sibling-with-same-prefix`).
pub fn is_same_or_under(fs: &dyn FileSystem, path: &str, root: &str) -> Result<bool, ToolError> {
    todo!("W7-03d")
}

/// True when either path equals, contains or lies inside the other.
pub fn overlaps(fs: &dyn FileSystem, a: &str, b: &str) -> Result<bool, ToolError> {
    todo!("W7-03d")
}

/// A report directory must not equal, sit inside or contain the photo folder. Error text:
/// `output directory '{output_directory}' must not equal, contain or sit inside the photo folder '{photo_folder}'`.
pub fn ensure_output_directory(
    fs: &dyn FileSystem,
    output_directory: &str,
    photo_folder: &str,
) -> Result<(), ToolError> {
    todo!("W7-03d")
}

/// A report file must not live in the photo folder or below it: `get_directory_name(get_full_path(output_file))` (falling back to
/// the full path itself) is tested with [`is_same_or_under`]. Error text:
/// `output file '{output_file}' must not be inside the photo folder '{photo_folder}'`.
pub fn ensure_output_file(fs: &dyn FileSystem, output_file: &str, photo_folder: &str) -> Result<(), ToolError> {
    todo!("W7-03d")
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
    todo!("W7-03d")
}

/// Creates the cache folder and its marker file (idempotent: an existing marker is left alone), UTF-8 without BOM.
pub fn claim_cache_directory(cache_dir: &str) -> Result<(), ToolError> {
    todo!("W7-03d")
}

/// Writes a report file without ever writing THROUGH its final path (R13): the text goes to a sibling temp file
/// `{full}.{first 8 of guid N}.tmp`, which then replaces the leaf (`std::fs::rename` over an existing file; a leaf that is a
/// symlink or hard link to a photo is replaced by a regular file and the photo's bytes stay untouched). UTF-8, no BOM, text
/// written as given (no newline conversion). The temp file is deleted when it still exists after a failure. I/O errors become
/// `ToolError::from_io`.
pub fn write_report_file(path: &str, content: &str) -> Result<(), ToolError> {
    todo!("W7-03d")
}

/// The Rust app's data folder `%LOCALAPPDATA%\PhotoReviewRs` (`pr_core::identity::APP_DIR_NAME`). No tool-owned folder may overlap it.
pub fn app_data_folder() -> String {
    todo!("W7-03d")
}

/// The C# app's data folder `%LOCALAPPDATA%\PhotoReview` (`ToolPathGuard.AppDataFolder` in C#). Also protected (R0002: never touch it).
pub fn csharp_app_data_folder() -> String {
    todo!("W7-03d")
}
