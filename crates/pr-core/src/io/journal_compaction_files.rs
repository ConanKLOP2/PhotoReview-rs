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

// TEMPORARY (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

use crate::abstractions::{JournalCompactionFiles, ReadSeek, StagedReplacement};

/// C# buffer size of both `FileStream`s (64 KiB): the deny-writers reader (`BufReader`) and the replacement writer
/// (`BufWriter`).
pub const COMPACTION_BUFFER_SIZE: usize = 64 * 1024;

/// Message of the C# `PlatformNotSupportedException` thrown by `CreateStagedReplacement` off Windows.
pub const NOT_WINDOWS_MESSAGE: &str = "Journal compaction needs the Windows rename primitive.";

// Win32 constants, same names and values as the C# `NativeMethods`.
const GENERIC_WRITE: u32 = 0x4000_0000;
const DELETE: u32 = 0x0001_0000;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_DELETE: u32 = 0x4;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
/// `FILE_INFO_BY_HANDLE_CLASS::FileRenameInfoEx`.
const FILE_RENAME_INFO_EX: i32 = 22;
const FILE_RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 0x1;
const FILE_RENAME_FLAG_POSIX_SEMANTICS: u32 = 0x2;

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
        todo!("W3-02")
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
        todo!("W3-02")
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
        todo!("W3-02")
    }

    /// C# `_stream.Flush(flushToDisk: true)`: flush the `BufWriter`, then `File::sync_all` (`FlushFileBuffers`).
    fn flush_to_disk(&mut self) -> io::Result<()> {
        todo!("W3-02")
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
        todo!("W3-02")
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
        todo!("W3-02")
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
        todo!("W3-02")
    }

    pub fn message(&self) -> &str {
        todo!("W3-02")
    }

    pub fn os_error(&self) -> &io::Error {
        todo!("W3-02")
    }

    /// `io::Error::new(self.os_error.kind(), self)`: same kind as the OS error, `to_string()` == the message.
    pub fn into_io_error(self) -> io::Error {
        todo!("W3-02")
    }
}

impl fmt::Display for CompactionFileError {
    /// The message only (no ` (os error N)` suffix: .NET `ex.Message`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W3-02")
    }
}

impl std::error::Error for CompactionFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        todo!("W3-02")
    }
}

/// The Win32 error code behind `err`: `err.raw_os_error()`, else the code of a wrapped [`CompactionFileError`]'s OS
/// error (`err.get_ref()` downcast). The Rust twin of C# `IOException.HResult & 0xFFFF` for these errors; W3-01 uses it
/// for its sharing/lock-violation test.
pub fn os_error_code(err: &io::Error) -> Option<i32> {
    todo!("W3-02")
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
    }
}
