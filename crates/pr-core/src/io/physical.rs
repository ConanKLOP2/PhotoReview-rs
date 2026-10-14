//! Port of `PhotoReview.Core/IO/PhysicalFileSystem.cs` (C# baseline d92d6aae): the real-disk [`FileSystem`], with the
//! Windows semantics the file-action safety story rests on (never overwrite on move/copy, create-new copy with an
//! ownership proof, atomic replace through a temp file, shared reads that never block other apps, reparse-point
//! resolution). Every behaviour below is pinned by the goldens `physical-fs-ops`, `physical-fs-pattern`,
//! `physical-fs-read-text` and `physical-fs-resolve` (contracts/W2-14.md).
//!
//! .NET quirks this port reproduces on purpose (do not "fix" them): `File.Delete` fails on read-only files and succeeds on
//! a missing file; `File.Move` of a directory is "file not found"; `File.Exists` is false for a trailing separator;
//! `File.ReadAllText` shares only `Read` (fails while another handle has the file open for writing); directory listings
//! use DOS wildcard rules; `Directory.EnumerateDirectories` includes junctions.
// STUB (contract writer): the bodies are `todo!()`; the porter removes this allow together with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::abstractions::{CopyCreationProof, FileStat, FileSystem, ReadSeek, SkippedEntry};
use crate::time::DateTimeUtc;

/// C# `ReplaceAttempts`: the final rename of an atomic write is tried at most 8 times.
pub const REPLACE_ATTEMPTS: u32 = 8;

/// The file system of the machine. Stateless: every call goes to the OS.
#[derive(Debug, Default, Clone, Copy)]
pub struct PhysicalFileSystem;

/// One directory entry as a listing delivers it (C# `FileInfo` from `DirectoryInfo.EnumerateFiles`): full path, length and
/// last write time taken from the directory entry, no per-file open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    /// `Path::new(directory)` made absolute, joined with the entry name (C# `FileInfo.FullName`).
    pub full_name: PathBuf,
    pub length: u64,
    pub last_write_utc: DateTimeUtc,
}

/// Test seam (C# `openListing` of the internal `EnumerateFilesWithStat`): opens a directory listing. `Err` from the opener
/// is a failure before the first entry; an `Err` item is a failure after `n` entries.
pub type OpenListing<'a> = &'a dyn Fn(&Path) -> io::Result<Box<dyn Iterator<Item = io::Result<ListedFile>>>>;

impl PhysicalFileSystem {
    /// C# internal `EnumerateFilesWithStat(directory, include, onSkipped, openListing)`: the logic of the trait method
    /// `enumerate_files_with_stat` over an injected listing. An `Err` item after at least one entry was read keeps what was
    /// read and reports one `SkippedEntry { path: directory, reason: err.to_string(), kind: ListingInterrupted }`; an `Err`
    /// before the first entry (opener or first item) is returned. Any other failure kind is not special-cased.
    pub fn enumerate_files_with_stat_from(
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
        open_listing: OpenListing<'_>,
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        todo!()
    }
}

/// C# `ReplaceWithRetry(move, sleep)` (internal): runs `move_op`; a transient failure (`PermissionDenied`, or raw OS error
/// 32 sharing violation, or 33 lock violation) is retried after `sleep(attempt * 3)` milliseconds, at most
/// [`REPLACE_ATTEMPTS`] attempts in total (sleeps 3, 6, 9 ... 21 = at most 84 ms); any other error, and the last attempt's
/// error, is returned at once without sleeping.
pub fn replace_with_retry(move_op: impl FnMut() -> io::Result<()>, sleep: impl FnMut(u32)) -> io::Result<()> {
    todo!()
}

/// C# `IsSwallowedAsDestinationExists` (internal): the failures of the no-overwrite copy that mean "the destination
/// already exists" (`TryCopyNew` returns false): exactly [`errors::is_destination_exists`](crate::file_actions::errors::is_destination_exists).
pub fn is_swallowed_as_destination_exists(err: &io::Error) -> bool {
    todo!()
}

/// `FileSystemName.MatchesWin32Expression` semantics of `Directory.EnumerateFiles(dir, pattern)` (legacy overload):
/// DOS wildcards, case-insensitive with [`text::ordinal_upper`](crate::text::ordinal_upper). See the contract for the
/// translation and the matcher; pinned by golden `physical-fs-pattern`.
pub fn matches_win32_expression(expression: &str, name: &str) -> bool {
    todo!()
}

/// `File.ReadAllText(path)` decoding: a UTF-8 / UTF-16 LE / UTF-16 BE / UTF-32 LE / UTF-32 BE byte-order mark selects the
/// encoding and is dropped; otherwise UTF-8. Invalid input becomes U+FFFD, never an error. Pinned by golden
/// `physical-fs-read-text`.
pub fn decode_text_like_dotnet(bytes: &[u8]) -> String {
    todo!()
}

/// `Guid.NewGuid().ToString("N")`: 32 lowercase hex digits, unique per call across threads and processes (it names the
/// temp file of an atomic write). Not required to be cryptographically random.
pub fn new_guid_n() -> String {
    todo!()
}

impl FileSystem for PhysicalFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        todo!()
    }

    fn directory_exists(&self, path: &Path) -> bool {
        todo!()
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        todo!()
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        todo!()
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        todo!()
    }

    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        todo!()
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        todo!()
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        todo!()
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn Write + Send>> {
        todo!()
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        todo!()
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        todo!()
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        todo!()
    }

    fn enumerate_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        todo!()
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        todo!()
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        todo!()
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        todo!()
    }

    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        todo!()
    }

    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        todo!()
    }
}
