//! In-memory Recycle Bin over a [`FakeFileSystem`]. Never touches the real bin (AGENTS.md, C# 2026-09-24 incident).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::abstractions::{FileSystem, RecycleBin};
use pr_core::text::path_eq_ignore_case;
use pr_core::time::DateTimeUtc;

use crate::FakeFileSystem;

#[derive(Debug, Default)]
struct State {
    /// (original path, content, last write) of every recycled file, oldest first.
    binned: Vec<(PathBuf, Vec<u8>, DateTimeUtc)>,
    permanently_deleted: Vec<PathBuf>,
    can_recycle: bool,
    fits: bool,
}

/// Recycles by removing the file from the fake file system and keeping its bytes so `try_restore` can put it back.
#[derive(Debug)]
pub struct FakeRecycleBin {
    fs: Arc<FakeFileSystem>,
    state: Mutex<State>,
}

impl FakeRecycleBin {
    pub fn new(fs: Arc<FakeFileSystem>) -> Self {
        Self {
            fs,
            state: Mutex::new(State {
                can_recycle: true,
                fits: true,
                ..State::default()
            }),
        }
    }

    /// Makes `can_recycle` answer `value` for every path.
    pub fn set_can_recycle(&self, value: bool) {
        self.lock().can_recycle = value;
    }

    /// Makes `fits_in_recycle_bin` answer `value` for every path.
    pub fn set_fits(&self, value: bool) {
        self.lock().fits = value;
    }

    /// Original paths of the files currently in the bin, oldest first.
    pub fn binned_paths(&self) -> Vec<PathBuf> {
        self.lock().binned.iter().map(|(p, _, _)| p.clone()).collect()
    }

    pub fn permanently_deleted(&self) -> Vec<PathBuf> {
        self.lock().permanently_deleted.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl RecycleBin for FakeRecycleBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        let (content, stat) = self.fs.take_file(path)?;
        self.lock()
            .binned
            .push((path.to_path_buf(), content, stat.last_write_utc));
        Ok(())
    }

    fn can_recycle(&self, _path: &Path) -> bool {
        self.lock().can_recycle
    }

    fn delete_permanently(&self, path: &Path) -> io::Result<()> {
        self.fs.delete(path)?;
        self.lock().permanently_deleted.push(path.to_path_buf());
        Ok(())
    }

    /// Restores the most recently binned file with this path, size and last write time; `false` when none matches or
    /// the original path is occupied again.
    fn try_restore(&self, original_path: &Path, expected_size: u64, expected_last_write_utc: DateTimeUtc) -> bool {
        if self.fs.file_exists(original_path) {
            return false;
        }
        let mut state = self.lock();
        let found = state.binned.iter().rposition(|(p, content, written)| {
            path_eq_ignore_case(p, original_path)
                && content.len() as u64 == expected_size
                && *written == expected_last_write_utc
        });
        let Some(index) = found else { return false };
        let (_, content, written) = state.binned.remove(index);
        self.fs.add_file_at(original_path, &content, written);
        true
    }

    fn fits_in_recycle_bin(&self, _path: &Path, _file_size: u64) -> bool {
        self.lock().fits
    }
}
