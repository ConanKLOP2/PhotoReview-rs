//! Port of `PhotoReview.Core/IO/PhysicalJournalCompactionFiles.cs` (C# baseline d92d6aae, 124 lines): the Windows
//! implementation of [`JournalCompactionFiles`]. The replacement is renamed with
//! `SetFileInformationByHandle(FileRenameInfoEx, REPLACE_IF_EXISTS | POSIX_SEMANTICS)`: one atomic NTFS rename that,
//! unlike `MoveFileEx`, succeeds while the target is open with delete sharing (the compaction lock and the journal
//! readers) and, unlike `ReplaceFile`, has no multi-step failure mode that can leave the target name missing. Needs
//! Windows 10 1709+ on NTFS; anywhere else the rename fails, the target is untouched and compaction is skipped.
//!
//! Creation and the deny-writers open use std `OpenOptions` with `OpenOptionsExt::{access_mode, share_mode,
//! custom_flags, attributes}` (no FFI needed). Only `SetFileInformationByHandle` is declared in a private
//! `extern "system"` block (kernel32 is already linked by std; no new crate), like `io::physical`'s `win32` module; the
//! .NET path/delete quirks (`open_like_file_stream`, `DeleteFileW`) are reused from `io::physical`, not re-written.
//! Contract: `contracts/W3-02.md`.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::abstractions::{JournalCompactionFiles, ReadSeek, StagedReplacement};
use crate::text::dotnet_path::{self, PathError};

/// C# buffer size of both `FileStream`s (64 KiB): the deny-writers reader (`BufReader`) and the replacement writer
/// (`BufWriter`).
pub const COMPACTION_BUFFER_SIZE: usize = 64 * 1024;

/// Message of the C# `PlatformNotSupportedException` thrown by `CreateStagedReplacement` off Windows.
pub const NOT_WINDOWS_MESSAGE: &str = "Journal compaction needs the Windows rename primitive.";

// Win32 constants, same names and values as the C# `NativeMethods`.
#[cfg_attr(not(windows), allow(dead_code))]
const GENERIC_WRITE: u32 = 0x4000_0000;
#[cfg_attr(not(windows), allow(dead_code))]
const DELETE: u32 = 0x0001_0000;
#[cfg_attr(not(windows), allow(dead_code))]
const FILE_SHARE_READ: u32 = 0x1;
#[cfg_attr(not(windows), allow(dead_code))]
const FILE_SHARE_DELETE: u32 = 0x4;
#[cfg_attr(not(windows), allow(dead_code))]
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
#[cfg_attr(not(windows), allow(dead_code))]
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
/// `FILE_INFO_BY_HANDLE_CLASS::FileRenameInfoEx`.
#[cfg_attr(not(windows), allow(dead_code))]
const FILE_RENAME_INFO_EX: i32 = 22;
#[cfg_attr(not(windows), allow(dead_code))]
const FILE_RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 0x1;
#[cfg_attr(not(windows), allow(dead_code))]
const FILE_RENAME_FLAG_POSIX_SEMANTICS: u32 = 0x2;

/// `ERROR_FILENAME_EXCED_RANGE`: what `PathTooLongException` maps to (as in `io::physical`).
const ERROR_FILENAME_EXCED_RANGE: i32 = 206;
/// `ERROR_PATH_NOT_FOUND`.
const ERROR_PATH_NOT_FOUND: i32 = 3;
/// `ERROR_ACCESS_DENIED`.
const ERROR_ACCESS_DENIED: i32 = 5;
/// .NET `PathInternal.MaxShortPath`: a path of this many UTF-16 units or more needs the `\\?\` form.
const MAX_SHORT_PATH: usize = 260;

/// The real Windows primitives. Stateless.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhysicalJournalCompactionFiles;

impl JournalCompactionFiles for PhysicalJournalCompactionFiles {
    /// C# `OpenReadDenyWriters(path)`: `ArgumentException.ThrowIfNullOrWhiteSpace(path)` -> `InvalidInput` (same text as
    /// `io::physical`'s blank-path error); then `new FileStream(path, Open, Read, FileShare.Read | FileShare.Delete,
    /// 64 KiB, SequentialScan)`: `read(true)`, `share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)`,
    /// `custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)`, opened the way `FileStream` resolves a path (`Path.GetFullPath`, extended
    /// prefix when needed: reuse `io::physical`'s `open_like_file_stream`, see contract open question 1), wrapped in a
    /// `BufReader::with_capacity(COMPACTION_BUFFER_SIZE, ..)`. The OS error is returned UNWRAPPED (raw code kept): W3-01's
    /// `TryCompact` maps a sharing (32) or lock (33) violation to `Busy`.
    fn open_read_deny_writers(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        if is_blank(path) {
            return Err(blank_path_error());
        }
        let file = open_deny_writers(path)?;
        Ok(Box::new(BufReader::with_capacity(COMPACTION_BUFFER_SIZE, file)))
    }

    /// C# `CreateStagedReplacement(tempPath)`:
    /// 1. blank `temp_path` -> `InvalidInput` (C# `ArgumentException`);
    /// 2. not Windows -> `io::Error::new(ErrorKind::Unsupported, NOT_WINDOWS_MESSAGE)` (C# `PlatformNotSupportedException`,
    ///    which `TryCompact` reports as `Failed`);
    /// 3. `full = Path.GetFullPath(temp_path)` (`dotnet_path::get_full_path`);
    /// 4. `CreateFileW(full, GENERIC_WRITE | DELETE, FILE_SHARE_READ, null, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, null)`:
    ///    `OpenOptions::new().write(true).create_new(true)` + `access_mode(GENERIC_WRITE | DELETE)` +
    ///    `share_mode(FILE_SHARE_READ)` + `attributes(FILE_ATTRIBUTE_NORMAL)`; an existing file fails (80 / 183);
    /// 5. on failure: [`CompactionFileError`] with message `"Could not create the journal compaction file " + full` and the
    ///    OS error as its source, converted with [`CompactionFileError::into_io_error`];
    /// 6. `Ok(Box::new(PhysicalStagedReplacement { path: full, writer: Some(BufWriter::with_capacity(64 KiB, file)),
    ///    replaced: false }))`.
    fn create_staged_replacement(&self, temp_path: &Path) -> io::Result<Box<dyn StagedReplacement>> {
        if is_blank(temp_path) {
            return Err(blank_path_error());
        }
        if !cfg!(windows) {
            return Err(io::Error::new(io::ErrorKind::Unsupported, NOT_WINDOWS_MESSAGE));
        }
        let full = full_path(temp_path)?;
        let file = create_new_staged(&full).map_err(|err| {
            CompactionFileError::new(format!("Could not create the journal compaction file {full}"), err)
                .into_io_error()
        })?;
        Ok(Box::new(PhysicalStagedReplacement {
            path: PathBuf::from(full),
            writer: Some(BufWriter::with_capacity(COMPACTION_BUFFER_SIZE, file)),
            replaced: false,
        }))
    }
}

/// C# `PhysicalJournalCompactionFiles.StagedReplacement`. Dropping it without a successful
/// [`replace_atomically`](StagedReplacement::replace_atomically) deletes the file (C# `Dispose`).
#[derive(Debug)]
pub struct PhysicalStagedReplacement {
    /// The FULL path the file was created at (C# `fullPath`): what `Drop` deletes.
    path: PathBuf,
    /// `None` only while dropping.
    writer: Option<BufWriter<File>>,
    /// Set after a successful rename; the file then IS the journal and must never be deleted.
    replaced: bool,
}

impl StagedReplacement for PhysicalStagedReplacement {
    /// C# `_stream.Write(bytes)`: buffered (`write_all` on the `BufWriter`).
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer_mut()?.write_all(bytes)
    }

    /// C# `_stream.Flush(flushToDisk: true)`: flush the `BufWriter`, then `File::sync_all` (`FlushFileBuffers`).
    fn flush_to_disk(&mut self) -> io::Result<()> {
        let writer = self.writer_mut()?;
        writer.flush()?;
        writer.get_ref().sync_all()
    }

    /// C# `ReplaceAtomically(destination)`, in order:
    /// 1. blank `destination` -> `InvalidInput`;
    /// 2. [`flush_to_disk`](StagedReplacement::flush_to_disk) (an error propagates unwrapped);
    /// 3. `name = Path.GetFullPath(destination)` (no `\\?\` prefix is added: C# passes the plain full path);
    /// 4. build `FILE_RENAME_INFO` in a zeroed, 8-byte aligned buffer (`Vec<u64>`): `Flags` (u32) at 0 =
    ///    `REPLACE_IF_EXISTS | POSIX_SEMANTICS`; `RootDirectory` (HANDLE, null) at `size_of::<usize>()`; `FileNameLength`
    ///    (u32, BYTES, no terminator) at `2 * size_of::<usize>()`; the UTF-16 units of `name` at `2 * size_of::<usize>() + 4`;
    ///    total size = name offset + name bytes + 2 (a zero terminator), passed as the buffer size;
    /// 5. `SetFileInformationByHandle(handle, FILE_RENAME_INFO_EX, buffer, size)` with the writer's raw handle
    ///    (`AsRawHandle`); on `0` -> [`CompactionFileError`] `"Could not replace " + name + " with the compacted journal"`
    ///    with `io::Error::last_os_error()` as source (sharing violation 32 when an appender holds the target);
    /// 6. success -> `replaced = true`.
    ///
    /// Non-Windows: `Unsupported` (unreachable: creation already failed).
    fn replace_atomically(&mut self, destination: &Path) -> io::Result<()> {
        if is_blank(destination) {
            return Err(blank_path_error());
        }
        self.flush_to_disk()?;
        let name = full_path(destination)?;
        let file = self.writer_mut()?.get_ref();
        rename_by_handle(file, &name).map_err(|err| {
            CompactionFileError::new(format!("Could not replace {name} with the compacted journal"), err)
                .into_io_error()
        })?;
        self.replaced = true;
        Ok(())
    }
}

impl Drop for PhysicalStagedReplacement {
    /// C# `Dispose`: flush and close the file, IGNORING a flush error (`catch (IOException)`); then, unless replaced,
    /// delete `path` ignoring every error (a missing file, still open elsewhere, access denied: "the next compaction
    /// removes stale replacement files"). The handle MUST be closed before the delete (it shares Read only). Never panics.
    /// Delete like .NET `File.Delete` = `DeleteFileW` on the full path (`io::physical`'s `win32::delete_file(&wide(..))`,
    /// made `pub(crate)` by the lead, contract open question 1), NOT `std::fs::remove_file`: std removes READ-ONLY files on
    /// Windows (see the `io::physical` module doc), while `DeleteFileW` fails with access denied and the C# test
    /// `StagedReplacementDispose_LeftoverTempThatCannotBeDeleted_DoesNotThrow` requires the read-only file to be LEFT.
    fn drop(&mut self) {
        if let Some(mut writer) = self.writer.take() {
            // C# `catch (IOException)`: a failed final flush of a replacement that is deleted below anyway. The buffer
            // is then discarded (`into_parts`) so `BufWriter`'s own drop does not retry it.
            let _ = writer.flush();
            let (file, _unwritten) = writer.into_parts();
            drop(file); // closed BEFORE the delete: the handle shares Read only
        }
        if self.replaced {
            return; // the file IS the journal now
        }
        // C# `File.Delete` inside `catch (IOException or UnauthorizedAccessException)`: left behind on any error, the
        // next compaction removes stale replacement files.
        let _ = delete_file(&self.path);
    }
}

/// An `IOException` that C# builds with its own message and an HRESULT: `Display` is exactly the message (what
/// `TryCompact` stores as `JournalCompactionResult.Error`), and the OS error stays reachable through
/// [`std::error::Error::source`] and [`os_error_code`] (C# `ex.HResult` = `0x8007_0000 | code`).
#[derive(Debug)]
pub struct CompactionFileError {
    message: String,
    os_error: io::Error,
}

impl CompactionFileError {
    pub fn new(message: impl Into<String>, os_error: io::Error) -> Self {
        Self {
            message: message.into(),
            os_error,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn os_error(&self) -> &io::Error {
        &self.os_error
    }

    /// `io::Error::new(self.os_error.kind(), self)`: same kind as the OS error, `to_string()` == the message.
    pub fn into_io_error(self) -> io::Error {
        io::Error::new(self.os_error.kind(), self)
    }
}

impl fmt::Display for CompactionFileError {
    /// The message only (no ` (os error N)` suffix: .NET `ex.Message`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CompactionFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.os_error)
    }
}

/// The Win32 error code behind `err`: `err.raw_os_error()`, else the code of a wrapped [`CompactionFileError`]'s OS
/// error (`err.get_ref()` downcast). The Rust twin of C# `IOException.HResult & 0xFFFF` for these errors; W3-01 uses it
/// for its sharing/lock-violation test.
pub fn os_error_code(err: &io::Error) -> Option<i32> {
    err.raw_os_error().or_else(|| {
        err.get_ref()
            .and_then(|inner| inner.downcast_ref::<CompactionFileError>())
            .and_then(|wrapped| wrapped.os_error.raw_os_error())
    })
}

impl PhysicalStagedReplacement {
    /// The open writer. `None` only inside `Drop`, so every caller sees `Some`; the error branch exists only to avoid a
    /// panic path (C# `ObjectDisposedException` cannot happen in Rust: `Drop` consumes the value).
    fn writer_mut(&mut self) -> io::Result<&mut BufWriter<File>> {
        self.writer
            .as_mut()
            .ok_or_else(|| io::Error::other("The staged journal replacement is already closed."))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// .NET path handling. The same quirks as the private helpers of `io::physical` (`is_blank`, `blank_path_error`,
// `full_path`, `open_like_file_stream`, `win32::{wide, delete_file}`). Contract open question 1 recommended making those
// `pub(crate)`; `io/physical.rs` (not a W3-02 file) does not expose them yet, so this file carries the small copies the
// contract names as the alternative. Same behaviour: switch to the shared helpers once they are visible.
// ---------------------------------------------------------------------------------------------------------------

/// `ArgumentException.ThrowIfNullOrWhiteSpace`.
fn is_blank(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().trim().is_empty()
}

fn blank_path_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "The value cannot be an empty string or composed entirely of whitespace.",
    )
}

/// `Path.GetFullPath` as an `io::Result` (`ArgumentException` -> `InvalidInput`, `PathTooLongException` -> raw 206).
fn full_path(path: &Path) -> io::Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The path is not valid Unicode."))?;
    dotnet_path::get_full_path(text).map_err(|err| match err {
        PathError::Argument => io::Error::new(io::ErrorKind::InvalidInput, err.to_string()),
        PathError::TooLong => io::Error::from_raw_os_error(ERROR_FILENAME_EXCED_RANGE),
    })
}

/// `PathInternal.IsDevice`: `\\?\`, `\??\` or `\\.\` (either separator).
#[cfg_attr(not(windows), allow(dead_code))]
fn is_device(path: &str) -> bool {
    let b = path.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    b.len() >= 4
        && ((sep(b[0]) && sep(b[1]) && (b[2] == b'?' || b[2] == b'.') && sep(b[3]))
            || (b[0] == b'\\' && b[1] == b'?' && b[2] == b'?' && b[3] == b'\\'))
}

/// `PathInternal.EnsureExtendedPrefix`: `\\?\` (or `\\?\UNC\`) in front of a fully qualified, non-device path.
#[cfg_attr(not(windows), allow(dead_code))]
fn ensure_extended_prefix(path: &str) -> String {
    if !dotnet_path::is_path_fully_qualified(path) || is_device(path) {
        return path.to_owned();
    }
    match path.strip_prefix(r"\\") {
        Some(unc) => format!(r"\\?\UNC\{unc}"),
        None => format!(r"\\?\{path}"),
    }
}

/// `PathInternal.EnsureExtendedPrefixIfNeeded` (what `FileStream` opens): the extended form when the path is 260 UTF-16
/// units or longer, or ends with a period or a space.
#[cfg_attr(not(windows), allow(dead_code))]
fn ensure_extended_prefix_if_needed(path: &str) -> String {
    if path.encode_utf16().count() >= MAX_SHORT_PATH || path.ends_with(['.', ' ']) {
        ensure_extended_prefix(path)
    } else {
        path.to_owned()
    }
}

/// C# `new FileStream(path, Open, Read, FileShare.Read | FileShare.Delete, 64 KiB, SequentialScan)`: the full path,
/// extended when needed, opened for reading with an explicit share mode (std's default shares ReadWrite|Delete).
/// `SafeFileHandle.Open` reports "path not found" on a bare root (`C:\`) as access denied (raw 5), exactly like
/// `io::physical::open_like_file_stream`.
#[cfg(windows)]
fn open_deny_writers(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    let full = full_path(path)?;
    let opened = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(ensure_extended_prefix_if_needed(&full));
    opened.map_err(|err| map_root_path_not_found(err, &full))
}

/// .NET `FileSystem.GetExceptionForWin32Error` as `SafeFileHandle.Open` applies it: ERROR_PATH_NOT_FOUND on a path that
/// is only a root becomes ERROR_ACCESS_DENIED (`UnauthorizedAccessException`, HResult 0x80070005).
#[cfg_attr(not(windows), allow(dead_code))]
fn map_root_path_not_found(err: io::Error, full: &str) -> io::Error {
    if err.raw_os_error() == Some(ERROR_PATH_NOT_FOUND) && full.len() == dotnet_path::root_length(full) {
        io::Error::from_raw_os_error(ERROR_ACCESS_DENIED)
    } else {
        err
    }
}

/// Off Windows `FileStream` has no share modes: a plain read open (contract open question 5, untested).
#[cfg(not(windows))]
fn open_deny_writers(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).open(full_path(path)?)
}

/// C# `CreateFileW(full, GENERIC_WRITE | DELETE, FILE_SHARE_READ, null, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, null)` on
/// the plain full path (std adds `\\?\` itself only for long paths: contract open question 4).
#[cfg(windows)]
fn create_new_staged(full: &str) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .access_mode(GENERIC_WRITE | DELETE)
        .share_mode(FILE_SHARE_READ)
        .attributes(FILE_ATTRIBUTE_NORMAL)
        .open(full)
}

#[cfg(not(windows))]
fn create_new_staged(_: &str) -> io::Result<File> {
    Err(io::Error::new(io::ErrorKind::Unsupported, NOT_WINDOWS_MESSAGE))
}

/// The `FILE_RENAME_INFO` bytes C# writes with `Marshal.Write*`: `Flags` (u32) at 0, `RootDirectory` (null HANDLE) at
/// `size_of::<usize>()` (the u32 union is padded to pointer alignment), `FileNameLength` (u32, BYTES, no terminator) at
/// `2 * size_of::<usize>()`, the UTF-16 units of `name` right after it, then one zero unit. Built with safe byte copies
/// and returned in a zeroed `u64` buffer (8-byte aligned, as the struct requires) with the byte size passed to the API.
#[cfg_attr(not(windows), allow(dead_code))]
fn rename_info(name: &str) -> io::Result<(Vec<u64>, u32)> {
    let root_offset = size_of::<usize>();
    let length_offset = root_offset + size_of::<usize>();
    let name_offset = length_offset + size_of::<u32>();
    let units: Vec<u16> = name.encode_utf16().collect();
    let name_bytes = units.len() * size_of::<u16>();
    let size = name_offset + name_bytes + size_of::<u16>();
    let too_long = || io::Error::from_raw_os_error(ERROR_FILENAME_EXCED_RANGE);
    let name_length = u32::try_from(name_bytes).map_err(|_| too_long())?;
    let size_u32 = u32::try_from(size).map_err(|_| too_long())?;

    let flags = FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS;
    let mut bytes = vec![0u8; size.div_ceil(8) * 8];
    bytes[..4].copy_from_slice(&flags.to_ne_bytes());
    // RootDirectory stays null (zero bytes).
    bytes[length_offset..name_offset].copy_from_slice(&name_length.to_ne_bytes());
    for (k, unit) in units.iter().enumerate() {
        let at = name_offset + k * 2;
        bytes[at..at + 2].copy_from_slice(&unit.to_ne_bytes());
    }
    let aligned = bytes
        .chunks_exact(8)
        .map(|chunk| {
            let mut word = [0u8; 8];
            word.copy_from_slice(chunk);
            u64::from_ne_bytes(word)
        })
        .collect();
    Ok((aligned, size_u32))
}

/// `SetFileInformationByHandle(handle, FileRenameInfoEx, info, size)`; `0` -> the last OS error.
#[cfg(windows)]
fn rename_by_handle(file: &File, name: &str) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    let (info, size) = rename_info(name)?;
    // SAFETY: `file` is an open handle borrowed for the whole synchronous call; `info` is an 8-byte aligned buffer of at
    // least `size` bytes holding a well-formed FILE_RENAME_INFO (built by `rename_info`), alive until the call returns;
    // the API only reads it.
    let ok = unsafe {
        win32::SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FILE_RENAME_INFO_EX,
            info.as_ptr().cast(),
            size,
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn rename_by_handle(_: &File, _: &str) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, NOT_WINDOWS_MESSAGE))
}

/// .NET `File.Delete` = `DeleteFileW` on the full path, through `EnsureExtendedPrefixIfNeeded` (`\\?\` form when 260
/// units or longer; the trailing period/space branch never fires: `path` comes from `get_full_path`, which trimmed them).
/// NOT `fs::remove_file`, which clears the read-only attribute first (trap T6).
#[cfg(windows)]
fn delete_file(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let text = ensure_extended_prefix_if_needed(&path.to_string_lossy());
    let wide: Vec<u16> = std::ffi::OsStr::new(&text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call; DeleteFileW only reads it.
    let ok = unsafe { win32::DeleteFileW(wide.as_ptr()) };
    if ok != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn delete_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// `BOOL SetFileInformationByHandle(HANDLE, FILE_INFO_BY_HANDLE_CLASS, LPVOID, DWORD)`.
        pub(super) fn SetFileInformationByHandle(
            file: *mut c_void,
            information_class: i32,
            information: *const c_void,
            buffer_size: u32,
        ) -> i32;
        /// `BOOL DeleteFileW(LPCWSTR)`.
        pub(super) fn DeleteFileW(path: *const u16) -> i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_info_layout_matches_the_csharp_marshal_writes() {
        let (info, size) = rename_info("C:\\a").expect("short name");
        let p = size_of::<usize>();
        assert_eq!(size as usize, 2 * p + 4 + 8 + 2);
        let bytes: Vec<u8> = info.iter().flat_map(|w| w.to_ne_bytes()).collect();
        assert_eq!(&bytes[..4], &3u32.to_ne_bytes());
        assert!(bytes[4..2 * p].iter().all(|&b| b == 0));
        assert_eq!(&bytes[2 * p..2 * p + 4], &8u32.to_ne_bytes());
        let name: Vec<u16> = bytes[2 * p + 4..2 * p + 12]
            .chunks_exact(2)
            .map(|c| u16::from_ne_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(String::from_utf16_lossy(&name), "C:\\a");
        assert_eq!(&bytes[2 * p + 12..2 * p + 14], &[0, 0]);
    }

    #[test]
    fn compaction_file_error_displays_the_message_and_keeps_the_os_code() {
        let err = CompactionFileError::new("Could not x", io::Error::from_raw_os_error(80)).into_io_error();
        assert_eq!(err.to_string(), "Could not x");
        assert_eq!(err.kind(), io::Error::from_raw_os_error(80).kind());
        assert_eq!(os_error_code(&err), Some(80));
        assert_eq!(os_error_code(&io::Error::from_raw_os_error(32)), Some(32));
        assert_eq!(os_error_code(&io::Error::other("plain")), None);
    }

    #[test]
    fn blank_paths_are_invalid_input() {
        let files = PhysicalJournalCompactionFiles;
        let err = files.open_read_deny_writers(Path::new("  ")).err().expect("blank");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        let err = files.create_staged_replacement(Path::new("")).err().expect("blank");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    /// The blank check runs before `Path.GetFullPath` (C# `ThrowIfNullOrWhiteSpace` first): the message is the
    /// `ArgumentException` text, not the `GetFullPath` one.
    #[test]
    fn blank_paths_report_the_whitespace_argument_message() {
        let expected = "The value cannot be an empty string or composed entirely of whitespace.";
        let files = PhysicalJournalCompactionFiles;
        for blank in ["", " ", " \t "] {
            let err = files.open_read_deny_writers(Path::new(blank)).err().expect("blank");
            assert_eq!(err.to_string(), expected, "open {blank:?}");
            let err = files.create_staged_replacement(Path::new(blank)).err().expect("blank");
            assert_eq!(err.to_string(), expected, "create {blank:?}");
        }
        assert!(is_blank(Path::new(" \t\r\n")));
        assert!(!is_blank(Path::new(" a ")));
    }

    #[test]
    fn compaction_file_error_exposes_its_message_and_os_error_source() {
        let error = CompactionFileError::new(
            "Could not replace C:\\j with the compacted journal",
            io::Error::from_raw_os_error(32),
        );
        assert_eq!(error.message(), "Could not replace C:\\j with the compacted journal");
        assert_eq!(error.os_error().raw_os_error(), Some(32));
        let source = std::error::Error::source(&error).expect("the OS error is the source");
        let source = source.downcast_ref::<io::Error>().expect("an io::Error");
        assert_eq!(source.raw_os_error(), Some(32));
    }

    #[test]
    fn is_device_recognises_the_three_device_prefixes_with_either_separator() {
        for device in [
            r"\\?\C:\a",
            r"\\.\C:\a",
            r"\??\C:\a",
            r"//?/C:/a",
            r"//./C:/a",
            r"\/?\C:\a",
            r"/\.\x",
        ] {
            assert!(is_device(device), "{device}");
        }
        for plain in [
            r"C:\a",
            r"\\server\share",
            r"\\?",
            r"\\x\a",
            r"\?.\a",
            r"/??\a",
            r"\??/a",
            r"\?\\a",
            r"\\?x",
            "",
        ] {
            assert!(!is_device(plain), "{plain}");
        }
    }

    #[test]
    fn ensure_extended_prefix_prefixes_only_fully_qualified_non_device_paths() {
        assert_eq!(ensure_extended_prefix(r"C:\a\b"), r"\\?\C:\a\b");
        assert_eq!(ensure_extended_prefix(r"\\server\share\f"), r"\\?\UNC\server\share\f");
        assert_eq!(ensure_extended_prefix(r"\\?\C:\a"), r"\\?\C:\a");
        assert_eq!(ensure_extended_prefix(r"\\.\C:\a"), r"\\.\C:\a");
        assert_eq!(ensure_extended_prefix(r"a\b"), r"a\b");
        assert_eq!(ensure_extended_prefix(r"C:a"), r"C:a");
    }

    #[test]
    fn ensure_extended_prefix_if_needed_switches_at_260_units_or_a_trailing_period_or_space() {
        let base = r"C:\";
        let at = |units: usize| format!("{base}{}", "a".repeat(units - base.len()));
        let short = at(MAX_SHORT_PATH - 1);
        assert_eq!(ensure_extended_prefix_if_needed(&short), short);
        let long = at(MAX_SHORT_PATH);
        assert_eq!(ensure_extended_prefix_if_needed(&long), format!(r"\\?\{long}"));
        // UTF-16 units, not bytes: 203 units (403 UTF-8 bytes) stay short.
        let wide_chars = format!("{base}{}", "\u{e9}".repeat(200));
        assert_eq!(ensure_extended_prefix_if_needed(&wide_chars), wide_chars);
        assert_eq!(ensure_extended_prefix_if_needed(r"C:\a."), r"\\?\C:\a.");
        assert_eq!(ensure_extended_prefix_if_needed(r"C:\a "), r"\\?\C:\a ");
        assert_eq!(ensure_extended_prefix_if_needed(r"C:\a"), r"C:\a");
    }

    #[test]
    fn path_not_found_on_a_bare_root_becomes_access_denied() {
        let code = |err: io::Error, full: &str| map_root_path_not_found(err, full).raw_os_error();
        assert_eq!(code(io::Error::from_raw_os_error(3), r"C:\"), Some(5));
        assert_eq!(code(io::Error::from_raw_os_error(3), r"\\server\share"), Some(5));
        assert_eq!(code(io::Error::from_raw_os_error(3), r"C:\missing\x"), Some(3));
        assert_eq!(code(io::Error::from_raw_os_error(2), r"C:\"), Some(2));
        assert_eq!(code(io::Error::from_raw_os_error(32), r"C:\a"), Some(32));
    }
}
