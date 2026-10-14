//! Port of `PhotoReview.Core/IO/CountingFileSystem.cs` (C# baseline d92d6aae).
//!
//! Decorates a [`FileSystem`] and counts the metadata queries (`file_exists`, `directory_exists`, `get_file_stat`) into
//! `ReviewMetrics.StatCount` (through [`StatCounter`]). Everything else is forwarded untouched, and EVERY trait method is
//! overridden, including the ones with default bodies (`try_copy_new*`, `enumerate_*_with_stat`, `try_probe_readable`,
//! `try_delete_empty_directory`, `resolve_real_path`): a wrapper that left a default in place would silently replace the
//! inner file system's atomic create-new copy by "exists-check then copy" (C# `FileSystemWrapperForwardingTests`).
// STUB (contract writer): the bodies are `todo!()`; the porter removes this allow together with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::abstractions::{CopyCreationProof, FileStat, FileSystem, ReadSeek, SkippedEntry};

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
        todo!()
    }
}

impl FileSystem for CountingFileSystem {
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

    fn try_copy_new(&self, source: &Path, destination: &Path) -> io::Result<bool> {
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

    fn enumerate_readable_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
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
