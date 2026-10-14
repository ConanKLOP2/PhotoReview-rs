//! In-memory Recycle Bin over a [`FakeFileSystem`]. Never touches the real bin (AGENTS.md, C# 2026-09-24 incident).
//!
//! It also carries the hooks of the C# test bins (`FakeRecycleBin.SendHook` of `FileActionServiceGroupMutationGapTests`,
//! the throwing `TryRestore` of the undo tests): each hook runs at the START of its call, outside any lock, and an error it
//! returns is the call's result (P-14).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::{FileSystem, RecycleBin};
use pr_core::text::path_eq_ignore_case;
use pr_core::time::DateTimeUtc;

use crate::FakeFileSystem;

/// A bin hook: `Some(err)` makes the call fail with `err` before it has any effect.
pub type BinHook = Arc<dyn Fn(&Path) -> Option<io::Error> + Send + Sync>;

#[derive(Default)]
struct State {
    /// (original path, content, last write) of every recycled file, oldest first.
    binned: Vec<(PathBuf, Vec<u8>, DateTimeUtc)>,
    permanently_deleted: Vec<PathBuf>,
    restore_calls: Vec<PathBuf>,
    can_recycle: bool,
    fits: bool,
    send_hook: Option<BinHook>,
    delete_permanently_hook: Option<BinHook>,
    restore_hook: Option<BinHook>,
}

/// Recycles by removing the file from the fake file system and keeping its bytes so `try_restore` can put it back.
pub struct FakeRecycleBin {
    fs: Arc<FakeFileSystem>,
    state: Mutex<State>,
}

impl fmt::Debug for FakeRecycleBin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeRecycleBin")
            .field("binned", &self.binned_paths())
            .finish_non_exhaustive()
    }
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

    /// C# `SendHook`: runs at the start of every `send_to_recycle_bin`.
    pub fn set_send_hook(&self, hook: Option<BinHook>) {
        self.lock().send_hook = hook;
    }

    /// Runs at the start of every `delete_permanently`.
    pub fn set_delete_permanently_hook(&self, hook: Option<BinHook>) {
        self.lock().delete_permanently_hook = hook;
    }

    /// Runs at the start of every `try_restore` (after the call is recorded): `Some(err)` = the shell failed (P-13).
    pub fn set_restore_hook(&self, hook: Option<BinHook>) {
        self.lock().restore_hook = hook;
    }

    /// Original paths of the files currently in the bin, oldest first.
    pub fn binned_paths(&self) -> Vec<PathBuf> {
        self.lock().binned.iter().map(|(p, _, _)| p.clone()).collect()
    }

    pub fn permanently_deleted(&self) -> Vec<PathBuf> {
        self.lock().permanently_deleted.clone()
    }

    /// Every path `try_restore` was asked for, in call order (failed calls included).
    pub fn restore_calls(&self) -> Vec<PathBuf> {
        self.lock().restore_calls.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs the hook picked by `pick` without holding the lock (a hook may call back into this bin).
    fn run_hook(&self, pick: impl Fn(&State) -> Option<BinHook>, path: &Path) -> io::Result<()> {
        let hook = pick(&self.lock());
        match hook.and_then(|hook| hook(path)) {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

impl RecycleBin for FakeRecycleBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        self.run_hook(|s| s.send_hook.clone(), path)?;
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
        self.run_hook(|s| s.delete_permanently_hook.clone(), path)?;
        self.fs.delete(path)?;
        self.lock().permanently_deleted.push(path.to_path_buf());
        Ok(())
    }

    /// Restores the most recently binned file with this path, size and last write time; `Ok(false)` when none matches
    /// or the original path is occupied again; the restore hook's error otherwise.
    fn try_restore(
        &self,
        original_path: &Path,
        expected_size: u64,
        expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        self.lock().restore_calls.push(original_path.to_path_buf());
        self.run_hook(|s| s.restore_hook.clone(), original_path)?;
        if self.fs.file_exists(original_path) {
            return Ok(false);
        }
        let mut state = self.lock();
        let found = state.binned.iter().rposition(|(p, content, written)| {
            path_eq_ignore_case(p, original_path)
                && content.len() as u64 == expected_size
                && *written == expected_last_write_utc
        });
        let Some(index) = found else { return Ok(false) };
        let (_, content, written) = state.binned.remove(index);
        drop(state);
        self.fs.add_file_at(original_path, &content, written);
        Ok(true)
    }

    fn fits_in_recycle_bin(&self, _path: &Path, _file_size: u64) -> bool {
        self.lock().fits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn setup() -> (Arc<FakeFileSystem>, FakeRecycleBin, DateTimeUtc) {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file(r"C:\a\x.jpg", b"123");
        let written = fs.peek_stat(r"C:\a\x.jpg").map(|s| s.last_write_utc).unwrap();
        let bin = FakeRecycleBin::new(Arc::clone(&fs));
        (fs, bin, written)
    }

    #[test]
    fn try_restore_reports_restored_not_found_and_failure_apart() {
        // P-13: Ok(true) restored, Ok(false) nothing matching, Err the shell failed (INV-5: never "not found").
        let (fs, bin, written) = setup();
        bin.send_to_recycle_bin(&p(r"C:\a\x.jpg")).unwrap();
        assert!(!fs.file_exists(&p(r"C:\a\x.jpg")));

        assert!(!bin.try_restore(&p(r"C:\a\x.jpg"), 4, written).unwrap(), "other size");
        bin.set_restore_hook(Some(Arc::new(|_| Some(io::Error::other("shell failed")))));
        let err = bin.try_restore(&p(r"C:\a\x.jpg"), 3, written).unwrap_err();
        assert_eq!(err.to_string(), "shell failed");
        assert_eq!(
            bin.binned_paths().len(),
            1,
            "a failed restore leaves the item in the bin"
        );

        bin.set_restore_hook(None);
        assert!(bin.try_restore(&p(r"C:\A\X.JPG"), 3, written).unwrap());
        assert_eq!(fs.file_content(r"C:\a\x.jpg").as_deref(), Some(&b"123"[..]));
        assert!(
            !bin.try_restore(&p(r"C:\a\x.jpg"), 3, written).unwrap(),
            "occupied again"
        );
        assert_eq!(bin.restore_calls().len(), 4);
    }

    #[test]
    fn send_hook_fails_before_any_effect() {
        let (fs, bin, _) = setup();
        bin.set_send_hook(Some(Arc::new(|path| {
            path.ends_with("x.jpg").then(|| io::Error::other("boom"))
        })));
        assert_eq!(
            bin.send_to_recycle_bin(&p(r"C:\a\x.jpg")).unwrap_err().to_string(),
            "boom"
        );
        assert!(fs.file_exists(&p(r"C:\a\x.jpg")));
        assert!(bin.binned_paths().is_empty());
    }

    #[test]
    fn delete_permanently_hook_fails_before_any_effect() {
        let (fs, bin, _) = setup();
        bin.set_delete_permanently_hook(Some(Arc::new(|_| {
            Some(io::Error::from(io::ErrorKind::PermissionDenied))
        })));
        assert!(bin.delete_permanently(&p(r"C:\a\x.jpg")).is_err());
        assert!(fs.file_exists(&p(r"C:\a\x.jpg")));
        assert!(bin.permanently_deleted().is_empty());
    }
}
