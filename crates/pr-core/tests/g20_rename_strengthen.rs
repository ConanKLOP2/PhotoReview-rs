//! G20 Rename part: stricter twins of three W9-R2 tests (plan items tp-pr-core T1, T2, T3 in
//! `docs/reviews/test-strengthening-plan.md`). The weaker originals in `rename_service.rs` and
//! `rename_file_action_matrix.rs` stay; these add the assertions the C# tests make and the originals leave out
//! (both files' bytes, no mutating file-system call, every journal field, unchanged size and write time).
//! Tests only, no production change.

use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::FileSystem;
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::journal_line::JournalEntry;
use pr_core::file_actions::rename::RenameRequest;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::FileActionResult;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin, FsOp, TempRoot};

struct Rig {
    _root: TempRoot,
    folder: std::path::PathBuf,
    fs: Arc<FakeFileSystem>,
    journal: Arc<OperationJournal>,
    service: FileActionService,
}

impl Rig {
    fn new(names: &[(&str, &[u8])]) -> Self {
        let root = TempRoot::new("g20-rename-strengthen");
        let folder = root.path().join("photos");
        std::fs::create_dir_all(&folder).expect("create test-owned folder");
        let fs = Arc::new(FakeFileSystem::new());
        for (name, bytes) in names {
            fs.add_file(folder.join(name), bytes);
        }
        let clock = Arc::new(FakeClock::new());
        let paths = AppPaths {
            journal_file: root.path().join("journal.jsonl"),
            ..AppPaths::new(root.path(), None, false)
        };
        let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
        let bin = Arc::new(FakeRecycleBin::new(fs.clone()));
        let service = FileActionService::new(journal.clone(), fs.clone(), clock, bin);
        Self {
            _root: root,
            folder,
            fs,
            journal,
            service,
        }
    }

    fn path(&self, name: &str) -> String {
        self.folder.join(name).to_string_lossy().into_owned()
    }

    fn rename(&self, source: &str, target: &str) -> FileActionResult {
        self.service.rename(
            &RenameRequest {
                source: source.into(),
                new_file_name: target.into(),
            },
            &CancelToken::new(),
        )
    }

    fn entries(&self) -> Vec<JournalEntry> {
        let mut entries = Vec::new();
        self.journal
            .read_entries(&mut |entry| entries.push(entry))
            .expect("read journal");
        entries
    }

    fn mutating_calls(&self) -> usize {
        [
            FsOp::Move,
            FsOp::Copy,
            FsOp::Delete,
            FsOp::WriteAtomic,
            FsOp::OpenAppend,
            FsOp::CreateDirectory,
        ]
        .into_iter()
        .map(|op| self.fs.count(op))
        .sum()
    }
}

#[test]
fn rename_target_taken_by_another_file_is_refused_and_touches_nothing_strict() {
    // C#: RenameFileActionTests.Rename_TargetTakenByAnotherFile_IsRefusedAndTouchesNothing (tp-pr-core T1).
    let rig = Rig::new(&[("a.jpg", b"original"), ("b.jpg", b"foreign")]);
    rig.fs.reset_counts();

    let result = rig.rename(&rig.path("a.jpg"), "b.jpg");

    assert!(!result.succeeded);
    assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"original".to_vec()));
    assert_eq!(rig.fs.file_content(rig.path("b.jpg")), Some(b"foreign".to_vec()));
    assert!(rig.entries().is_empty(), "a refused rename journals nothing");
    assert_eq!(
        rig.mutating_calls(),
        0,
        "a refused rename makes no mutating file-system call"
    );
}

#[test]
fn rename_target_taken_by_another_file_with_other_case_keeps_the_stored_spelling_and_bytes() {
    // C#: RenameFileActionTests.Rename_TargetTakenByAnotherFileWithOtherCase_IsRefused (tp-pr-core T2).
    let rig = Rig::new(&[("a.jpg", b"original"), ("b.jpg", b"foreign")]);
    rig.fs.reset_counts();

    let result = rig.rename(&rig.path("a.jpg"), "B.JPG");

    assert!(!result.succeeded);
    assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"original".to_vec()));
    assert_eq!(rig.fs.file_content(rig.path("b.jpg")), Some(b"foreign".to_vec()));
    assert_eq!(
        rig.fs.get_stored_file_names(Path::new(&rig.path("b.jpg"))).unwrap(),
        ["b.jpg"],
        "the foreign file keeps its own spelling"
    );
    assert!(rig.entries().is_empty());
    assert_eq!(rig.mutating_calls(), 0);
}

#[test]
fn rename_success_moves_in_place_and_journals_one_committed_move_strict() {
    // C#: RenameFileActionTests.Rename_Success_MovesInPlaceAndJournalsOneCommittedMove (tp-pr-core T3).
    let rig = Rig::new(&[("a.jpg", b"original")]);
    let source = rig.path("a.jpg");
    let destination = rig.path("x.jpg");
    let before = rig.fs.peek_stat(&source).expect("source stat");

    let result = rig.rename(&source, "x.jpg");

    assert!(result.succeeded, "{:?}", result.error);
    assert!(!result.no_change);
    assert_eq!(result.operation, FileOperationType::Move);
    assert_eq!(result.destination_path.as_deref(), Some(destination.as_str()));
    assert!(!rig.fs.file_exists(Path::new(&source)));
    assert_eq!(rig.fs.file_content(&destination), Some(b"original".to_vec()));
    assert_eq!(
        rig.fs.peek_stat(&destination).expect("destination stat"),
        before,
        "a rename never changes size or write time"
    );
    let entries = rig.entries();
    // C# `LatestOnly`: the journal holds a Prepared line then a Committed line of ONE operation (same id).
    let states: Vec<JournalState> = entries.iter().map(|e| e.state).collect();
    assert_eq!(states, [JournalState::Prepared, JournalState::Committed]);
    assert!(entries.iter().all(|e| e.id == entries[0].id), "one operation, one id");
    let entry = &entries[1];
    assert_eq!(entry.op_type, FileOperationType::Move);
    assert_eq!(entry.state, JournalState::Committed);
    assert_eq!(entry.source, source);
    assert_eq!(entry.destination.as_deref(), Some(destination.as_str()));
    assert!(entry.group_members.is_none());
    assert!(entry.undo.is_none());
}
