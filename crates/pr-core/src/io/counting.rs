//! Port of `PhotoReview.Core/IO/CountingFileSystem.cs` (C# baseline d92d6aae).
//!
//! Decorates a [`FileSystem`] and counts the metadata queries (`file_exists`, `directory_exists`, `get_file_stat`) into
//! `ReviewMetrics.StatCount` (through [`StatCounter`]). Everything else is forwarded untouched, and EVERY trait method is
//! overridden, including the ones with default bodies (`try_copy_new*`, `enumerate_*_with_stat`, `try_probe_readable`,
//! `try_delete_empty_directory`, `resolve_real_path`): a wrapper that left a default in place would silently replace the
//! inner file system's atomic create-new copy by "exists-check then copy" (C# `FileSystemWrapperForwardingTests`).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::abstractions::{AppendStream, CopyCreationProof, FileStat, FileSystem, ReadSeek, SkippedEntry};

/// Counts metadata queries (C# `ReviewMetrics.RecordStat`). `ReviewMetrics` (W2-17) implements it; any `Fn()` closure does
/// too, so tests can pass a counter.
pub trait StatCounter: Send + Sync {
    /// One metadata query is about to be forwarded (counted before the call, also when it then fails).
    fn record_stat(&self);
}

impl<F: Fn() + Send + Sync> StatCounter for F {
    fn record_stat(&self) {
        self();
    }
}

/// See the module docs.
pub struct CountingFileSystem {
    inner: Arc<dyn FileSystem>,
    metrics: Arc<dyn StatCounter>,
}

impl fmt::Debug for CountingFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CountingFileSystem").finish_non_exhaustive()
    }
}

impl CountingFileSystem {
    /// C# `new CountingFileSystem(inner, metrics)`.
    pub fn new(inner: Arc<dyn FileSystem>, metrics: Arc<dyn StatCounter>) -> Self {
        Self { inner, metrics }
    }
}

impl FileSystem for CountingFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        self.metrics.record_stat();
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.metrics.record_stat();
        self.inner.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.metrics.record_stat();
        self.inner.get_file_stat(path)
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.move_file(source, destination)
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.copy_file(source, destination)
    }

    fn try_copy_new(&self, source: &Path, destination: &Path) -> io::Result<bool> {
        self.inner.try_copy_new(source, destination)
    }

    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        self.inner.try_copy_new_with_proof(source, destination, proof)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        self.inner.delete(path)
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.inner.open_read_shared(path, buffer_size)
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.inner.open_append(path, durable)
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.inner.write_all_text_atomic(path, text, durable)
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.inner.read_all_text(path)
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_files(directory, pattern)
    }

    fn enumerate_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        self.inner.enumerate_files_with_stat(directory, include, on_skipped)
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        self.inner.try_probe_readable(path)
    }

    fn enumerate_readable_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        self.inner
            .enumerate_readable_files_with_stat(directory, include, on_skipped)
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_directories(directory)
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.inner.create_directory(path)
    }

    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        self.inner.try_delete_empty_directory(path)
    }

    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.resolve_real_path(path)
    }
}
