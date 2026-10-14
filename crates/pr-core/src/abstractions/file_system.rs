//! Port of `IFileSystem.cs`, `CopyCreationProof.cs` and `SkippedEntry.cs`.
//!
//! Errors are `io::Error`; the `io::ErrorKind`/`raw_os_error` carry what the C# exception type carried
//! (`FileNotFoundException` → `NotFound`, `UnauthorizedAccessException` → `PermissionDenied`, sharing violation →
//! raw OS error 32). Default method bodies reproduce the C# default interface methods exactly.

use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::time::DateTimeUtc;

/// Size and last write time (UTC) of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub length: u64,
    pub last_write_utc: DateTimeUtc,
}

/// A readable, seekable byte source (C# `Stream` opened for reading).
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// Why a scan entry was skipped (language-neutral; the window maps it to catalog text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkippedKind {
    /// The file could not be opened for reading (locked, access denied, I/O error).
    #[default]
    FileUnreadable,
    /// The directory listing itself failed part-way; the rest of the folder was not scanned.
    ListingInterrupted,
}

/// A file or directory a folder scan could not read and skipped instead of failing the whole load (ADR 0007 §3).
/// `reason` is the OS message kept verbatim for logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedEntry {
    pub path: PathBuf,
    pub reason: String,
    pub kind: SkippedKind,
}

/// Proof, raised by a [`FileSystem::try_copy_new_with_proof`] implementation, that THIS call created the destination.
/// It stays false when the destination already existed or the call failed before creating anything. A caller may
/// clean up a destination after a failed copy only when this is true.
#[derive(Debug, Default)]
pub struct CopyCreationProof {
    destination_created: AtomicBool,
}

impl CopyCreationProof {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn destination_created(&self) -> bool {
        self.destination_created.load(Ordering::SeqCst)
    }

    /// Called by the implementation at (or right after) the moment it created the destination.
    pub fn mark_created(&self) {
        self.destination_created.store(true, Ordering::SeqCst);
    }
}

/// The file-system operations PhotoReview needs.
pub trait FileSystem: Send + Sync {
    fn file_exists(&self, path: &Path) -> bool;
    fn directory_exists(&self, path: &Path) -> bool;
    /// `None` when the file does not exist (or cannot be stat'ed).
    fn get_file_stat(&self, path: &Path) -> Option<FileStat>;
    /// Move/rename; fails if `destination` exists (C# `File.Move` without overwrite).
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()>;
    /// Copy; fails if `destination` exists (C# `File.Copy` without overwrite).
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()>;

    /// Copy without overwrite with "create new" semantics: `Ok(false)` (nothing touched) when `destination` already
    /// exists, `Ok(true)` when this call created it.
    fn try_copy_new(&self, source: &Path, destination: &Path) -> io::Result<bool> {
        self.try_copy_new_with_proof(source, destination, &CopyCreationProof::new())
    }

    /// As [`try_copy_new`](Self::try_copy_new), reporting ownership through `proof`: implementations call
    /// [`CopyCreationProof::mark_created`] only when this call created the destination (even if the copy then fails
    /// part-way). Default (not atomic; wrappers and fakes): existence check, then copy, then mark.
    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        if self.file_exists(destination) {
            return Ok(false);
        }
        self.copy_file(source, destination)?;
        proof.mark_created();
        Ok(true)
    }

    /// Permanent delete. Deleting a missing file succeeds (C# `File.Delete`).
    fn delete(&self, path: &Path) -> io::Result<()>;
    /// Read with share ReadWrite | Delete and SequentialScan (INV-8). C# default buffer: 64 KiB.
    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>>;
    /// Append; `durable` adds write-through (ADR 0007: false = "fast" mode, data reaches only the OS cache on flush).
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn Write + Send>>;
    /// Atomic replace through a temp file. `durable`: write-through + flush to disk (Settings); false: still atomic,
    /// no fsync (Session, ADR 0007 §2). C# default: durable.
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()>;
    fn read_all_text(&self, path: &Path) -> io::Result<String>;
    /// Files directly in `directory` matching a .NET search pattern (`*`, `?`; C# default `"*"`).
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>>;

    /// Fast listing for opening a folder (AR16): files passing `include` with their stat, no readability probe.
    /// A listing error after the first entry is reported through `on_skipped`
    /// ([`SkippedKind::ListingInterrupted`]) and the part already read is kept; an error before the first entry is
    /// returned (ADR 0007 §3). Default (in-memory fakes): enumerate + stat each, then filter.
    fn enumerate_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        let _ = on_skipped;
        Ok(self
            .enumerate_files(directory, "*")?
            .into_iter()
            .map(|path| {
                let stat = self.get_file_stat(&path);
                (path, stat)
            })
            .filter(|(path, _)| include(path))
            .collect())
    }

    /// Whether a file can be opened for reading (one open/close, no bytes read). `Err` carries the technical reason
    /// (locked, access denied, I/O error; ADR 0007 §3). Default: readable (in-memory fakes).
    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        let _ = path;
        Ok(())
    }

    /// As [`enumerate_files_with_stat`](Self::enumerate_files_with_stat) but only readable files: a file whose probe
    /// fails is skipped and reported through `on_skipped` instead of failing the scan.
    fn enumerate_readable_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        let listed = self.enumerate_files_with_stat(directory, include, on_skipped)?;
        let mut readable = Vec::with_capacity(listed.len());
        for (path, stat) in listed {
            match self.try_probe_readable(&path) {
                Ok(()) => readable.push((path, stat)),
                Err(reason) => on_skipped(SkippedEntry {
                    path,
                    reason,
                    kind: SkippedKind::FileUnreadable,
                }),
            }
        }
        Ok(readable)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>>;
    /// Creates the directory and any missing parents.
    fn create_directory(&self, path: &Path) -> io::Result<()>;

    /// Deletes `path` ONLY when it exists and is empty (never recursive, never removes content): cleans up an empty
    /// destination folder that a failed action itself created. `true` when deleted; `false` when missing, not empty
    /// or not deletable (ordinary I/O errors are not surfaced). Default: `false` (unsupported, cleans nothing).
    /// C# d92d6aae `TryDeleteEmptyDirectory`.
    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        let _ = path;
        false
    }

    /// SEC-01: absolute path after resolving every existing reparse point (symlink/junction) along it, including the
    /// last segment. Segments that do not exist are kept as they are. Default (fakes without reparse points): the
    /// absolute path unchanged.
    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        std::path::absolute(path)
    }
}
