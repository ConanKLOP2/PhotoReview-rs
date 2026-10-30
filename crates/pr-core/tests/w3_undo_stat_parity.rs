//! Regression tests for the three Recycle stat sites in UndoService (C# baseline 7eaee2a2).
//! In-memory fakes only: no user files, config or real Recycle Bin.
mod undo_support;

use pr_core::abstractions::{AppendStream, FileStat, FileSystem, ReadSeek};
use pr_core::file_actions::undo::{EnglishUndoTexts, UndoService, UndoTexts};
use pr_core::model::{FileOperationType, JournalState};
use pr_test_support::FakeFileSystem;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use undo_support::*;

struct StatFaultFs {
    disk: Arc<FakeFileSystem>,
    path: String,
    fail_on: usize,
    probes: AtomicUsize,
    enabled: AtomicBool,
    kind: io::ErrorKind,
}
impl FileSystem for StatFaultFs {
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.try_get_file_stat(path).ok().flatten()
    }
    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        if path == Path::new(&self.path)
            && self.probes.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_on
            && self.enabled.load(Ordering::SeqCst)
        {
            return Err(io::Error::new(self.kind, "stat inspection failed"));
        }
        self.disk.try_get_file_stat(path)
    }
    fn file_exists(&self, path: &Path) -> bool {
        self.disk.file_exists(path)
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.disk.directory_exists(path)
    }
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.disk.move_file(source, destination)
    }
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.disk.copy_file(source, destination)
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        self.disk.delete(path)
    }
    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.disk.open_read_shared(path, buffer_size)
    }
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.disk.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.disk.write_all_text_atomic(path, text, durable)
    }
    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.disk.read_all_text(path)
    }
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.disk.enumerate_files(directory, pattern)
    }
    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.disk.enumerate_directories(directory)
    }
    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.disk.create_directory(path)
    }
}

fn run_stat_failure(group: bool, post_restore: bool, bin_fault: bool, kind: io::ErrorKind) {
    let f = Fixture::new();
    if bin_fault {
        f.bin
            .set_default(BinMode::RestoreThenThrow("bin restore failed".to_owned()));
    }
    // Paths are keys in FakeFileSystem, never real disk paths.
    let first = "photos/first.jpg";
    let fault = "photos/fault.raw";
    let fs = Arc::new(StatFaultFs {
        disk: f.disk.clone(),
        path: fault.to_owned(),
        fail_on: if group && post_restore { 2 } else { 1 },
        probes: AtomicUsize::new(0),
        enabled: AtomicBool::new(true),
        kind,
    });
    let mut undo = UndoService::new(f.journal.clone(), fs.clone(), f.bin.clone(), Arc::new(EnglishUndoTexts))
        .with_clock(f.clock.clone());
    if group {
        undo.register_group(&group_result(
            FileOperationType::Recycle,
            "deleted",
            &[
                member(first, None, 4, write_time(), false),
                member(fault, None, 8, write_time(), false),
            ],
        ));
    } else {
        undo.register(&single_result(
            true,
            FileOperationType::Recycle,
            fault,
            None,
            8,
            write_time(),
        ));
    }
    let identity = undo.last_undo_action_identity();
    let result = undo.undo_last();
    assert!(!result.succeeded);
    assert!(!result.rejected);
    assert_eq!(
        result.error_message,
        Some(EnglishUndoTexts.core_undo_failed("stat inspection failed"))
    );
    assert_eq!(
        undo.last_undo_action_identity(),
        identity,
        "inspection failure retains the retryable action"
    );
    assert!(!undo.is_busy(), "INV-4: gate released on stat error");
    let mut lines = Vec::new();
    f.journal.read_entries(&mut |line| lines.push(line)).unwrap();
    if group && post_restore {
        assert_eq!(f.bin.restore_calls(), 2);
        assert_eq!(result.restored_paths, Some(vec![first.to_owned()]));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].state, JournalState::Prepared);
        assert_eq!(lines[1].state, JournalState::Failed);
        assert_eq!(lines[0].id, lines[1].id);
        assert_eq!(lines[0].undo, Some(true));
        assert_eq!(lines[1].error.as_deref(), Some("stat inspection failed"));
        assert!(lines.iter().all(|line| line.state != JournalState::Committed));
    } else {
        assert_eq!(f.bin.restore_calls(), usize::from(!group));
        assert!(result.restored_paths.is_none());
        assert!(
            lines.is_empty(),
            "pre-check faults and single Recycle never journal an undo"
        );
    }
    // A subsequent call proves gate release and preserves safe retry behaviour.
    fs.enabled.store(false, Ordering::SeqCst);
    let retry = undo.undo_last();
    assert!(!retry.rejected);
    if group && post_restore {
        assert!(
            retry.succeeded,
            "partial undo resolves once all identities can be inspected"
        );
        assert!(!undo.has_last_undo_action());
    } else if group {
        assert!(retry.succeeded);
    } else {
        assert!(
            !retry.succeeded,
            "an existing file is protected from another shell restore"
        );
        assert_eq!(f.bin.restore_calls(), 1);
    }
}

// C#: UndoService.UndoLastAsync lines 373-387; no C# stat-fault twin exists.
// Guards undo.rs:810: replacing the fallible stat with get_file_stat must fail this test.
#[test]
fn undo_last_recycle_post_restore_stat_error_is_reported_and_releases_gate() {
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::InvalidInput] {
        run_stat_failure(false, true, false, kind);
    }
}

// C#: UndoService.UndoGroupRecycleAsync pre-check and catch; no C# stat-fault twin exists.
// Guards undo.rs:1112: replacing the fallible stat with get_file_stat must fail this test.
#[test]
fn undo_last_group_recycle_pre_restore_stat_error_does_not_restore_or_journal() {
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::InvalidInput] {
        run_stat_failure(true, false, false, kind);
    }
}

// C#: UndoService.UndoGroupRecycleAsync post-restore verification and catch.
// Guards undo.rs:1177: replacing the fallible stat with get_file_stat must fail this test.
#[test]
fn undo_last_group_recycle_post_restore_stat_error_journals_failed_and_retains_partial_undo() {
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::InvalidInput] {
        run_stat_failure(true, true, false, kind);
    }
}

// C#: UndoLastAsync line 373 runs outside the inner bin-error catch (lines 362-369).
// A stat error takes precedence over the earlier saved bin error.
#[test]
fn undo_last_recycle_stat_error_takes_precedence_over_bin_restore_error() {
    run_stat_failure(false, true, true, io::ErrorKind::PermissionDenied);
}
