//! Port of `PhotoReview.Imaging/Caching/AtomicCacheFile.cs` (C# baseline d92d6aae): the shared "temp file, write,
//! atomic rename" primitive behind the disk cache (`DiskCacheStore`, W4-09) and the preview cache file
//! (`PreviewCacheFile`, W4-08b). PARITY: see contracts/drafts-w4/W4-08a.
//!
//! Target path: `crates/pr-imaging/src/cache/atomic_file.rs` (lead adds `pub mod atomic_file;` to
//! `crates/pr-imaging/src/cache/mod.rs`, and `pub mod cache;` to `lib.rs`).
//!
//! This is a disposable cache write, not a durability-critical journal: NO write-through, NO fsync. The only
//! guarantee is that a crash (or a cancelled/failed write) never leaves a half-written file at the cache path.
//! Uses `std::fs` directly (the `FileSystem` trait has no create-new stream; C# also bypasses `IFileSystem` here).

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use pr_core::abstractions::Log;
use pr_core::sync::{CancelToken, Canceled};

/// Size of the write buffer in front of the temp file (C# `FileStream` bufferSize `64 * 1024`).
pub const WRITE_BUFFER_SIZE: usize = 64 * 1024;

/// Suffix of every temp file this module creates; matches `DiskCacheStore.TempFilePattern` (`*.tmp`), which is how
/// leftovers of a crash are found and swept (W4-09).
pub const TEMP_FILE_SUFFIX: &str = ".tmp";

/// Why [`write_atomic`] did not publish the file. C# threw `OperationCanceledException` or the I/O / payload exception.
#[derive(Debug)]
pub enum AtomicWriteError {
    /// The token was cancelled before the write started, or by the payload before the rename (C# `OperationCanceledException`).
    Canceled,
    /// Directory creation, temp creation (including "already exists"), the payload's own error, flush or rename failed.
    Io(io::Error),
}

impl fmt::Display for AtomicWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtomicWriteError::Canceled => f.write_str("operation canceled"),
            AtomicWriteError::Io(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl std::error::Error for AtomicWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AtomicWriteError::Canceled => None,
            AtomicWriteError::Io(e) => Some(e),
        }
    }
}

impl From<io::Error> for AtomicWriteError {
    fn from(e: io::Error) -> Self {
        AtomicWriteError::Io(e)
    }
}

impl From<Canceled> for AtomicWriteError {
    fn from(_: Canceled) -> Self {
        AtomicWriteError::Canceled
    }
}

/// `cache_path + "." + guid_n + ".tmp"` (pure; C# `cachePath + "." + Guid.NewGuid().ToString("N") + ".tmp"`).
/// `guid_n` is 32 lowercase hex digits (`pr_core::io::physical::new_guid_n()`).
pub fn temporary_path_for(cache_path: &str, guid_n: &str) -> String {
    format!("{cache_path}.{guid_n}{TEMP_FILE_SUFFIX}")
}

/// C# `AtomicCacheFile.WriteAsync(cachePath, writePayload, log, cancellationToken)`: writes the payload to a fresh
/// temp file next to `cache_path` and renames it over `cache_path` (replacing an existing file).
///
/// Blocking (R0003: `Task` becomes a blocking call; the caller runs it on its persist worker).
/// Order of effects (do not reorder):
///  1. `cancel.check()` (nothing created, not even the directory).
///  2. create the parent directory of `cache_path` (all missing levels).
///  3. create the temp file with create-new + no sharing (an existing temp path is an `AlreadyExists` error and the
///     existing file is left untouched).
///  4. run `write_payload` on a [`WRITE_BUFFER_SIZE`] buffered writer over it, flush, close the handle (drop the
///     writer before any cleanup, also when the payload failed: Windows cannot delete an open share-none file).
///  5. `cancel.check()` again (a superseded write, e.g. Clear Cache, must not publish).
///  6. rename the temp file over `cache_path` with overwrite.
///  7. always (success or failure) delete the temp file, but ONLY if step 3 created it (best effort, errors go to `log`).
pub fn write_atomic(
    cache_path: &Path,
    write_payload: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    log: Option<&dyn Log>,
    cancel: &CancelToken,
) -> Result<(), AtomicWriteError> {
    write_atomic_with_temp(cache_path, write_payload, log, None, cancel)
}

/// C# test seam `WriteAsync(..., temporaryPathOverride, ...)`: `temporary_path_override` pins the temp file path
/// (production callers pass `None` and get [`temporary_path_for`] with a fresh GUID).
pub fn write_atomic_with_temp(
    cache_path: &Path,
    write_payload: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    log: Option<&dyn Log>,
    temporary_path_override: Option<&Path>,
    cancel: &CancelToken,
) -> Result<(), AtomicWriteError> {
    write_atomic_impl(cache_path, write_payload, log, temporary_path_override, cancel, None)
}

/// Marker for whatever a [`CommitGate`] keeps alive across the rename (a lock guard).
pub trait CommitHold {}
impl<T> CommitHold for T {}

/// Rust-only commit gate (ST-IMAGING-INV M1 / G26): called right before the rename. `None` aborts the write as
/// [`AtomicWriteError::Canceled`] (the temp file is deleted); `Some(hold)` keeps `hold` alive until the rename is done,
/// so the check and the rename are atomic against whatever the hold locks.
pub type CommitGate<'a> = &'a dyn Fn() -> Option<Box<dyn CommitHold + 'a>>;

/// [`write_atomic`] plus a [`CommitGate`] consulted after the payload is written and the cancel token re-checked.
pub fn write_atomic_gated(
    cache_path: &Path,
    write_payload: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    log: Option<&dyn Log>,
    cancel: &CancelToken,
    gate: CommitGate<'_>,
) -> Result<(), AtomicWriteError> {
    write_atomic_impl(cache_path, write_payload, log, None, cancel, Some(gate))
}

fn write_atomic_impl(
    cache_path: &Path,
    write_payload: impl FnOnce(&mut dyn Write) -> io::Result<()>,
    log: Option<&dyn Log>,
    temporary_path_override: Option<&Path>,
    cancel: &CancelToken,
    gate: Option<CommitGate<'_>>,
) -> Result<(), AtomicWriteError> {
    cancel.check()?;

    // C# `Path.GetDirectoryName(cachePath)!`: no folder part (or a root path) is an `ArgumentException`.
    let directory = match directory_name(cache_path) {
        Some(directory) => directory,
        None => {
            return Err(AtomicWriteError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cache path has no directory part",
            )));
        }
    };
    fs::create_dir_all(directory)?;

    let generated;
    let temp_path: &Path = match temporary_path_override {
        Some(p) => p,
        None => {
            let name = temporary_path_for(&cache_path.to_string_lossy(), &pr_core::io::physical::new_guid_n());
            generated = std::path::PathBuf::from(name);
            &generated
        }
    };

    // Create-new: from here on this call owns the temp file (R07). A failed open leaves a pre-existing file alone.
    let file = open_create_new(temp_path)?;

    // Cleanup guard: runs on every exit, including a panic unwinding out of the payload (C# `finally`). The writer
    // below is dropped (handle closed) before it, on every path. The original error wins over a cleanup failure.
    let _cleanup = TempCleanup { path: temp_path, log };

    let mut writer = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);
    write_payload(&mut writer)?;
    writer.flush()?;
    // Close the handle before the rename (Windows cannot rename an open share-none file).
    drop(writer);
    // A superseded write (e.g. Clear Cache) must not publish.
    cancel.check()?;
    let _hold = match gate {
        Some(gate) => Some(gate().ok_or(AtomicWriteError::Canceled)?),
        None => None,
    };
    rename_over(temp_path, cache_path)?;
    Ok(())
}

/// Deletes the temp file this call created when dropped (best effort, errors go to the log).
struct TempCleanup<'a> {
    path: &'a Path,
    log: Option<&'a dyn Log>,
}

impl Drop for TempCleanup<'_> {
    fn drop(&mut self) {
        try_delete(self.path, self.log);
    }
}

/// C# `Path.GetDirectoryName` for the cache path: everything before the last separator, so `root\sub\` gives
/// `root\sub` (std's `parent` would give `root`). `None` for no folder part or a root.
fn directory_name(path: &Path) -> Option<&Path> {
    let text = path.to_str();
    let trailing = match text {
        Some(t) => t.ends_with(['\\', '/']),
        None => false,
    };
    if trailing {
        let trimmed = Path::new(text?.trim_end_matches(['\\', '/']));
        // A root (`C:\`, `\`) trims to a path without a final component: C# returns null.
        trimmed.file_name()?;
        return Some(trimmed);
    }
    path.parent().filter(|p| !p.as_os_str().is_empty())
}

/// Plain paths of 260+ UTF-16 units need the verbatim prefix for `MoveFileExW` (`C:\x` -> `\\?\C:\x`, `\\server\share\x`
/// -> `\\?\UNC\server\share\x`, what .NET's path normalisation does). Shorter paths and `\\?\`, `\\.\` or other forms
/// pass through unchanged.
#[cfg(windows)]
fn extend_long_path(mut units: Vec<u16>) -> Vec<u16> {
    const MAX_SHORT_PATH: usize = 260;
    let bs = u16::from(b'\\');
    let drive_rooted = units.len() >= 3 && units[1] == u16::from(b':') && units[2] == bs;
    // `\\server\share...` but not `\\?\...`, `\\.\...` or `\\\...`.
    let plain_unc = units.len() >= 3
        && units[0] == bs
        && units[1] == bs
        && units[2] != bs
        && units[2] != u16::from(b'?')
        && units[2] != u16::from(b'.');
    if units.len() >= MAX_SHORT_PATH && drive_rooted {
        let mut extended: Vec<u16> = r"\\?\".encode_utf16().collect();
        extended.append(&mut units);
        extended
    } else if units.len() >= MAX_SHORT_PATH && plain_unc {
        let mut extended: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
        extended.extend_from_slice(&units[2..]);
        extended
    } else {
        units
    }
}

#[cfg(all(test, windows))]
mod long_path_tests {
    use super::extend_long_path;

    fn pad(prefix: &str, total: usize) -> Vec<u16> {
        let mut text = prefix.to_string();
        while text.encode_utf16().count() < total {
            text.push('x');
        }
        text.encode_utf16().collect()
    }

    fn text(units: &[u16]) -> String {
        String::from_utf16(units).expect("utf16")
    }

    #[test]
    fn drive_path_gets_prefix_from_260_units() {
        let at_limit = pad(r"C:\a\", 260);
        let expected = format!(r"\\?\{}", text(&at_limit));
        assert_eq!(text(&extend_long_path(at_limit)), expected);
    }

    #[test]
    fn drive_path_below_260_units_is_unchanged() {
        let short = pad(r"C:\a\", 259);
        assert_eq!(extend_long_path(short.clone()), short);
    }

    #[test]
    fn unc_path_gets_unc_prefix_from_260_units() {
        let at_limit = pad(r"\\server\share\a\", 260);
        let expected = format!(r"\\?\UNC\{}", &text(&at_limit)[2..]);
        assert_eq!(text(&extend_long_path(at_limit)), expected);
    }

    #[test]
    fn unc_path_below_260_units_is_unchanged() {
        let short = pad(r"\\server\share\a\", 259);
        assert_eq!(extend_long_path(short.clone()), short);
    }

    #[test]
    fn verbatim_device_and_triple_backslash_paths_are_unchanged() {
        for prefix in [
            r"\\?\C:\a\",
            r"\\?\UNC\server\share\",
            r"\\.\C:\a\",
            r"\\\server\share\",
        ] {
            let long = pad(prefix, 300);
            assert_eq!(extend_long_path(long.clone()), long, "{prefix}");
        }
    }

    #[test]
    fn other_long_forms_are_unchanged() {
        for prefix in [r"C:a\", r"\", r"a\"] {
            let long = pad(prefix, 300);
            assert_eq!(extend_long_path(long.clone()), long, "{prefix}");
        }
    }

    #[test]
    fn minimal_unc_and_degenerate_inputs() {
        assert!(extend_long_path(Vec::new()).is_empty());
        let two: Vec<u16> = r"\\".encode_utf16().collect();
        assert_eq!(extend_long_path(two.clone()), two);
        let long_unc_one_char_server = pad(r"\\s", 300);
        let expected = format!(r"\\?\UNC\{}", &text(&long_unc_one_char_server)[2..]);
        assert_eq!(text(&extend_long_path(long_unc_one_char_server)), expected);
    }
}

/// `File.Move(temp, target, overwrite: true)`: `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`, which is denied while another
/// handle has the target open (even with delete sharing). std's `fs::rename` uses POSIX replace semantics on current
/// toolchains and would silently succeed there.
#[cfg(windows)]
fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    /// UTF-16 for `MoveFileExW`. Like .NET's `Path.GetFullPath`, the path is made absolute first (a relative path is
    /// resolved against the current directory, also when only its absolute form is long) and a plain path of 260+
    /// units gets the verbatim prefix. `\\?\` and `\\.\` paths pass through `absolute` unchanged.
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        if path.as_os_str().encode_wide().any(|unit| unit == 0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL"));
        }
        let absolute = std::path::absolute(path)?;
        let mut units = extend_long_path(absolute.as_os_str().encode_wide().collect());
        units.push(0);
        Ok(units)
    }

    let source = wide(from)?;
    let destination = wide(to)?;
    // SAFETY: both buffers are NUL-terminated UTF-16 strings without an interior NUL that outlive the call;
    // MoveFileExW only reads them.
    let ok = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), MOVEFILE_REPLACE_EXISTING) };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn rename_over(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn open_create_new(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path)
}

#[cfg(not(windows))]
fn open_create_new(path: &Path) -> io::Result<fs::File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// C# `DiskCacheStore.TryDelete(path, log)` as used by the cleanup: `false` when the file does not exist; deletes it
/// and returns `!exists` afterwards; an I/O or permission error is logged as `"Delete failed: {path}"` with the error
/// (`log.error`) and (nothing is returned). W4-09 owns the public twin; if it lands first this private copy is replaced by
/// a call to it (no behaviour change).
fn try_delete(path: &Path, log: Option<&dyn Log>) {
    if !path.exists() {
        return;
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) => {
            if let Some(log) = log {
                log.error(&format!("Delete failed: {}", path.display()), Some(&e));
            }
        }
    }
}
