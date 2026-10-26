use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_core::abstractions::FileSystem;
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{RecoveryFileCheck, RecoveryPathStatus, RecoveryVerdict};
use pr_core::file_actions::recovery_retry::{EnglishRecoveryRetryTexts, RecoveryRetryService};
use pr_core::file_actions::rename::RenameRequest;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::undo::{EnglishUndoTexts, UndoService};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin, FsOp, TempRoot};

const STAMP: i64 = 639_029_198_450_000_000;

struct Rig {
    _root: TempRoot,
    photos: PathBuf,
    fs: Arc<FakeFileSystem>,
    journal: Arc<OperationJournal>,
    clock: Arc<FakeClock>,
    service: FileActionService,
    undo: UndoService,
}

impl Rig {
    fn new(names: &[&str]) -> Self {
        let root = TempRoot::new("rename-recovery");
        let photos = root.path().join("photos");
        std::fs::create_dir_all(&photos).expect("temp photo folder");
        let fs = Arc::new(FakeFileSystem::new());
        let stamp = DateTimeUtc::from_ticks(STAMP).expect("valid test stamp");
        for name in names {
            fs.add_file_at(photos.join(name), b"photo", stamp);
        }
        let clock = Arc::new(FakeClock::new());
        let paths = AppPaths {
            journal_file: root.path().join("journal.jsonl"),
            ..AppPaths::new(root.path(), None, false)
        };
        let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
        let bin = Arc::new(FakeRecycleBin::new(fs.clone()));
        let service = FileActionService::new(journal.clone(), fs.clone(), clock.clone(), bin.clone());
        let undo =
            UndoService::new(journal.clone(), fs.clone(), bin, Arc::new(EnglishUndoTexts)).with_clock(clock.clone());
        Self {
            _root: root,
            photos,
            fs,
            journal,
            clock,
            service,
            undo,
        }
    }
    fn path(&self, name: &str) -> String {
        self.photos.join(name).to_string_lossy().into_owned()
    }
    fn prepared(&self, source: &str, destination: &str, state: JournalState) -> JournalEntry {
        let stat = self.fs.get_file_stat(Path::new(source)).expect("source fingerprint");
        let stamp = DateTimeUtc::from_ticks(STAMP).expect("valid stamp");
        JournalEntry {
            id: "rename-1".into(),
            op_type: FileOperationType::Move,
            state,
            source: source.into(),
            destination: Some(destination.into()),
            size: stat.length as i64,
            last_write_utc: JournalDateTime::utc(stamp),
            timestamp_utc: JournalDateTime::utc(stamp),
            error: Some("interrupted".into()),
            error_code: None,
            permanent: None,
            undo: None,
            group_id: None,
            group_members: None,
        }
    }
    fn stored(&self, path: &str) -> Vec<String> {
        self.fs.get_stored_file_names(Path::new(path)).expect("stored name")
    }
}

#[test]
fn case_only_rename_recovery_verdict_follows_the_stored_spelling() {
    // C#: RenameFileActionTests.CaseOnlyRename_RecoveryVerdict_FollowsTheStoredSpelling (each theory row).
    let cases = [
        ("a.jpg", RecoveryVerdict::CanRetry),
        ("A.jpg", RecoveryVerdict::AlreadyDone),
        ("a.JPG", RecoveryVerdict::Conflict),
    ];
    for (stored, expected) in cases {
        let rig = Rig::new(&[stored]);
        let entry = rig.prepared(&rig.path("a.jpg"), &rig.path("A.jpg"), JournalState::Failed);
        let checked = RecoveryFileCheck::new(rig.fs.clone()).check(&entry);
        assert_eq!(checked.verdict, expected, "stored={stored}");
        match stored {
            "a.jpg" => {
                assert_eq!(checked.source.status, RecoveryPathStatus::Exists);
                assert_eq!(
                    checked.destination.as_ref().expect("destination").status,
                    RecoveryPathStatus::Missing
                );
            }
            "A.jpg" => {
                assert_eq!(checked.source.status, RecoveryPathStatus::Missing);
                assert_eq!(
                    checked.destination.as_ref().expect("destination").status,
                    RecoveryPathStatus::Exists
                );
            }
            _ => {
                assert_eq!(checked.source.status, RecoveryPathStatus::Changed);
                assert_eq!(
                    checked.destination.as_ref().expect("destination").status,
                    RecoveryPathStatus::Changed
                );
            }
        }
    }
}

#[test]
fn case_only_rename_file_stored_under_a_third_spelling_is_never_lost_or_done() {
    // C#: RenameFileActionTests.CaseOnlyRename_FileStoredUnderAThirdSpelling_IsNeverLostOrDone.
    let rig = Rig::new(&["a.jpg"]);
    rig.fs.set_stored_file_names_override(Some(vec!["a.JPG".into()]));
    let entry = rig.prepared(&rig.path("a.jpg"), &rig.path("A.jpg"), JournalState::Prepared);
    assert_eq!(
        RecoveryFileCheck::new(rig.fs.clone()).check(&entry).verdict,
        RecoveryVerdict::Conflict
    );
    let committed = JournalEntry {
        state: JournalState::Committed,
        ..entry
    };
    assert_eq!(
        RecoveryFileCheck::new(rig.fs.clone()).check(&committed).verdict,
        RecoveryVerdict::Unknown
    );
}

#[test]
fn case_only_rename_two_entries_differing_by_case_are_two_files_conflict() {
    // C#: RenameFaultInjectionTests.CaseOnlyRename_TwoEntriesDifferingByCase_AreTwoFiles_Conflict.
    let rig = Rig::new(&["a.jpg"]);
    rig.fs
        .set_stored_file_names_override(Some(vec!["a.jpg".into(), "A.jpg".into()]));
    let entry = rig.prepared(&rig.path("a.jpg"), &rig.path("A.jpg"), JournalState::Prepared);
    assert_eq!(
        RecoveryFileCheck::new(rig.fs.clone()).check(&entry).verdict,
        RecoveryVerdict::Conflict
    );
    let committed = JournalEntry {
        state: JournalState::Committed,
        ..entry
    };
    assert_eq!(
        RecoveryFileCheck::new(rig.fs.clone()).check(&committed).verdict,
        RecoveryVerdict::Unknown
    );
}

#[test]
fn case_only_rename_startup_reconcile_follows_the_stored_spelling() {
    // C#: RenameFileActionTests.CaseOnlyRename_StartupReconcile_FollowsTheStoredSpelling (each theory row).
    for stored in ["a.jpg", "A.jpg", "a.JPG"] {
        let rig = Rig::new(&[stored]);
        let entry = rig.prepared(&rig.path("a.jpg"), &rig.path("A.jpg"), JournalState::Prepared);
        rig.journal.append(&entry).expect("append prepared");
        let result = rig
            .journal
            .reconcile_pending_operations(Some(DateTimeUtc::from_ticks(STAMP + 1).expect("threshold")))
            .expect("reconcile");
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].state,
            if stored == "A.jpg" {
                JournalState::Committed
            } else {
                JournalState::Failed
            }
        );
    }
}

#[test]
fn case_only_rename_undo_restores_the_old_spelling() {
    // C#: RenameFileActionTests.CaseOnlyRename_Undo_RestoresTheOldSpelling.
    let mut rig = Rig::new(&["a.jpg"]);
    let source = rig.path("a.jpg");
    let before = rig.fs.get_file_stat(Path::new(&source)).expect("before");
    let renamed = rig.service.rename(
        &RenameRequest {
            source: source.clone(),
            new_file_name: "A.jpg".into(),
        },
        &CancelToken::new(),
    );
    assert!(renamed.succeeded, "{:?}", renamed.error);
    assert_eq!(rig.stored(&source), vec!["A.jpg"]);
    assert_eq!(
        rig.fs.get_file_stat(Path::new(&source)).expect("renamed").length,
        before.length
    );
    rig.undo.register(&renamed);
    let undone = rig.undo.undo_last();
    assert!(undone.succeeded, "{:?}", undone.error_message);
    assert_eq!(rig.stored(&source), vec!["a.jpg"]);
}

// C# fault-matrix scenarios are explicit test functions; the local fake rig uses TempRoot and records durable journal state.
#[test]
fn case_only_rename_reconcile_interrupted() {
    // C#: RenameFaultInjectionTests.CaseOnlyRename_ReconcileInterrupted.
    let rig = Rig::new(&["A.jpg"]);
    let entry = rig.prepared(&rig.path("a.jpg"), &rig.path("A.jpg"), JournalState::Prepared);
    rig.journal.append(&entry).expect("prepared");
    rig.fs
        .move_file(Path::new(&rig.path("a.jpg")), Path::new(&rig.path("A.jpg")))
        .expect("interrupted mutation");
    let outcomes = rig
        .journal
        .reconcile_pending_operations(Some(DateTimeUtc::from_ticks(STAMP + 1).expect("threshold")))
        .expect("reconcile");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].state, JournalState::Committed);
}

#[test]
fn group_case_only_rename_matrix_preserves_all_fingerprints_and_spelling() {
    // C#: RenameFaultInjectionTests.GroupCaseOnlyRename_Matrix.
    let rig = Rig::new(&["a.jpg", "a.cr2", "a.xmp"]);
    let group =
        pr_core::catalog::types::CaptureGroup::new(rig.path("a.jpg"), rig.path("a.cr2"), Some(rig.path("a.xmp")))
            .expect("group");
    let result = rig.service.rename_group(&group, "A", &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    for name in ["A.jpg", "A.cr2", "A.xmp"] {
        assert_eq!(rig.stored(&rig.path(name)), vec![name]);
    }
    assert_eq!(result.members.len(), 3);
    let mut entries = Vec::new();
    rig.journal
        .read_entries(&mut |entry| entries.push(entry))
        .expect("journal read");
    let committed = entries.last().expect("committed group line");
    assert_eq!(committed.state, JournalState::Committed);
    let members = committed.group_members.as_ref().expect("journal group members");
    for (member, target) in members.iter().zip(["A.jpg", "A.cr2", "A.xmp"]) {
        assert_eq!(
            Path::new(&member.destination.as_ref().expect("destination"))
                .file_name()
                .unwrap(),
            target
        );
        let current = rig
            .fs
            .get_file_stat(Path::new(&rig.path(target)))
            .expect("destination fingerprint");
        assert_eq!(i64::try_from(current.length).expect("size"), member.size);
        assert_eq!(current.last_write_utc, member.last_write_utc.value);
    }
}

#[test]
fn group_case_only_rename_matrix_faults_and_restart_preserve_each_member() {
    // C#: RenameFaultInjectionTests.GroupCaseOnlyRename_Matrix.
    // Matrix phases: complete transaction, failure before the first mutation, and failure between group members.
    for failed_member in [None, Some("a.jpg"), Some("a.cr2")] {
        let rig = Rig::new(&["a.jpg", "a.cr2", "a.xmp"]);
        if let Some(name) = failed_member {
            rig.fs
                .fail(FsOp::Move, rig.photos.join(name), std::io::ErrorKind::PermissionDenied);
        }
        let group =
            pr_core::catalog::types::CaptureGroup::new(rig.path("a.jpg"), rig.path("a.cr2"), Some(rig.path("a.xmp")))
                .expect("group");
        let result = rig.service.rename_group(&group, "A", &CancelToken::new());
        if failed_member.is_none() {
            assert!(result.succeeded, "{:?}", result.error);
            for target in ["A.jpg", "A.cr2", "A.xmp"] {
                assert_eq!(rig.stored(&rig.path(target)), vec![target]);
            }
            let mut entries = Vec::new();
            rig.journal
                .read_entries(&mut |entry| entries.push(entry))
                .expect("journal read");
            assert_eq!(entries.last().expect("outcome").state, JournalState::Committed);
        } else {
            assert!(!result.succeeded);
            for source in ["a.jpg", "a.cr2", "a.xmp"] {
                assert_eq!(rig.stored(&rig.path(source)), vec![source]);
            }
            let mut entries = Vec::new();
            rig.journal
                .read_entries(&mut |entry| entries.push(entry))
                .expect("journal read");
            assert_eq!(entries.last().expect("outcome").state, JournalState::Dismissed);
        }
        for name in ["a.jpg", "a.cr2", "a.xmp"] {
            assert_eq!(
                rig.fs
                    .get_file_stat(Path::new(&rig.path(name)))
                    .expect("member retained")
                    .length,
                5
            );
        }
    }
    let rig = Rig::new(&["a.jpg", "a.cr2", "a.xmp"]);
    let stamp = DateTimeUtc::from_ticks(STAMP).expect("stamp");
    let sources = ["a.jpg", "a.cr2", "a.xmp"];
    let targets = ["A.jpg", "A.cr2", "A.xmp"];
    let members = sources
        .iter()
        .zip(targets)
        .map(|(source, target)| JournalGroupMember {
            source: rig.path(source),
            destination: Some(rig.path(target)),
            size: 5,
            last_write_utc: JournalDateTime::utc(stamp),
            permanent: false,
        })
        .collect::<Vec<_>>();
    let mut entry = rig.prepared(
        &members[0].source,
        members[0].destination.as_deref().expect("dst"),
        JournalState::Prepared,
    );
    entry.group_id = Some("group-1".into());
    entry.group_members = Some(members);
    rig.journal.append(&entry).expect("prepared group");
    rig.fs
        .move_file(Path::new(&rig.path("a.jpg")), Path::new(&rig.path("A.jpg")))
        .expect("first member interrupted");
    let reconciled = rig
        .journal
        .reconcile_pending_operations(Some(DateTimeUtc::from_ticks(STAMP + 1).expect("threshold")))
        .expect("restart reconcile");
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, JournalState::Failed);
    assert_eq!(rig.stored(&rig.path("A.jpg")), vec!["A.jpg"]);
    assert_eq!(rig.stored(&rig.path("a.cr2")), vec!["a.cr2"]);
    assert_eq!(rig.stored(&rig.path("a.xmp")), vec!["a.xmp"]);
    let retry = RecoveryRetryService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(EnglishRecoveryRetryTexts),
    );
    let retried = retry
        .retry_move_or_copy(&reconciled[0], false, &CancelToken::new())
        .expect("group retry");
    assert!(retried.succeeded, "{}", retried.message);
    for target in ["A.jpg", "A.cr2", "A.xmp"] {
        assert_eq!(rig.stored(&rig.path(target)), vec![target]);
    }
}

#[test]
fn case_only_rename_recovery_retry_of_failed_line_finishes_the_rename() {
    // C#: RenameFileActionTests.CaseOnlyRename_RecoveryRetryOfAFailedLine_FinishesTheRename.
    let rig = Rig::new(&["a.jpg"]);
    let entry = rig.prepared(&rig.path("a.jpg"), &rig.path("A.jpg"), JournalState::Failed);
    rig.journal.append(&entry).expect("failed entry");
    let retry = RecoveryRetryService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        rig.clock.clone(),
        Arc::new(EnglishRecoveryRetryTexts),
    );
    let result = retry
        .retry_move_or_copy(&entry, false, &CancelToken::new())
        .expect("retry");
    assert!(result.succeeded, "{}", result.message);
    assert_eq!(rig.stored(&entry.source), vec!["A.jpg"]);
    let mut entries = Vec::new();
    rig.journal
        .read_entries(&mut |line| entries.push(line))
        .expect("journal read");
    assert_eq!(entries.last().expect("retry outcome").state, JournalState::Committed);
}

#[test]
fn case_only_rename_undo_of_an_already_undone_rename_does_not_run_twice_and_drops_the_entry() {
    // C#: RenameFileActionTests.CaseOnlyRename_UndoOfAnAlreadyUndoneRename_DoesNotRunTwiceAndDropsTheEntry.
    let mut rig = Rig::new(&["a.jpg"]);
    let renamed = rig.service.rename(
        &RenameRequest {
            source: rig.path("a.jpg"),
            new_file_name: "A.jpg".into(),
        },
        &CancelToken::new(),
    );
    rig.undo.register(&renamed);
    rig.fs
        .move_file(Path::new(&rig.path("A.jpg")), Path::new(&rig.path("a.jpg")))
        .expect("manual restore");
    let undone = rig.undo.undo_last();
    assert!(!undone.succeeded);
    assert_eq!(rig.stored(&rig.path("a.jpg")), vec!["a.jpg"]);
    assert!(!rig.undo.has_last_undo_action());
}

#[test]
fn case_only_rename_undo_when_the_file_was_renamed_back_by_hand_drops_the_dead_history_entry() {
    // C#: RenameFileActionTests.CaseOnlyRename_UndoWhenTheFileWasRenamedBackByHand_DropsTheDeadHistoryEntry.
    let mut rig = Rig::new(&["a.jpg"]);
    let renamed = rig.service.rename(
        &RenameRequest {
            source: rig.path("a.jpg"),
            new_file_name: "A.jpg".into(),
        },
        &CancelToken::new(),
    );
    rig.undo.register(&renamed);
    rig.fs
        .move_file(Path::new(&rig.path("A.jpg")), Path::new(&rig.path("a.jpg")))
        .expect("manual restore");
    assert!(!rig.undo.undo_last().succeeded);
    assert!(rig.undo.move_history().is_empty());
}

#[test]
fn case_only_rename_undo_moved_the_file_but_the_check_failed_drops_the_entry_by_the_stored_spelling() {
    // C#: RenameFileActionTests.CaseOnlyRename_UndoMovedTheFileButTheCheckFailed_DropsTheEntryByTheStoredSpelling.
    let rig = Rig::new(&["a.jpg"]);
    let renamed = rig.service.rename(
        &RenameRequest {
            source: rig.path("a.jpg"),
            new_file_name: "A.jpg".into(),
        },
        &CancelToken::new(),
    );
    let fs = rig.fs.clone();
    let mut undo = UndoService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
        Arc::new(EnglishUndoTexts),
    )
    .with_clock(rig.clock.clone())
    .with_move_override(Arc::new(move |from, to| {
        FileSystem::move_file(fs.as_ref(), from, to)?;
        fs.add_file(to, b"changed size");
        Ok(())
    }));
    undo.register(&renamed);
    assert!(!undo.undo_last().succeeded);
    assert_eq!(rig.stored(&rig.path("a.jpg")), vec!["a.jpg"]);
    assert_eq!(
        rig.fs.file_content(rig.photos.join("a.jpg")).expect("returned file"),
        b"changed size"
    );
    assert!(undo.move_history().is_empty());
}

#[test]
#[ignore = "native: case-preserving filesystem required"]
fn case_only_rename_native_move_undo_reconcile_uses_temp_root() {
    // C#: RenameFileActionTests.CaseOnlyRename_Undo_RestoresTheOldSpelling; startup reconcile uses the same native pair probes.
    let root = TempRoot::new("rename-recovery-native");
    let photos = root.path().join("photos");
    std::fs::create_dir_all(&photos).expect("create temp folder");
    let source = photos.join("a.jpg");
    std::fs::write(&source, b"photo").expect("create temp file");
    let fs = Arc::new(pr_core::io::physical::PhysicalFileSystem);
    let clock = Arc::new(FakeClock::new());
    let paths = AppPaths {
        journal_file: root.path().join("journal.jsonl"),
        ..AppPaths::new(root.path(), None, false)
    };
    let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
    let fake_bin_fs = Arc::new(FakeFileSystem::new());
    let bin = Arc::new(FakeRecycleBin::new(fake_bin_fs));
    let service = FileActionService::new(journal.clone(), fs.clone(), clock.clone(), bin.clone());
    let mut undo =
        UndoService::new(journal.clone(), fs.clone(), bin, Arc::new(EnglishUndoTexts)).with_clock(clock.clone());
    let source_text = source.to_string_lossy().into_owned();
    let before = fs.get_file_stat(&source).expect("source stat");
    let renamed = service.rename(
        &RenameRequest {
            source: source_text.clone(),
            new_file_name: "A.jpg".into(),
        },
        &CancelToken::new(),
    );
    assert!(renamed.succeeded, "{:?}", renamed.error);
    let stored = || {
        std::fs::read_dir(&photos)
            .expect("list")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(stored(), vec!["A.jpg"]);
    assert_eq!(fs.get_file_stat(&source).expect("renamed stat").length, before.length);
    undo.register(&renamed);
    assert!(undo.undo_last().succeeded);
    assert_eq!(stored(), vec!["a.jpg"]);
    assert_eq!(fs.get_file_stat(&source).expect("restored stat").length, before.length);
    let pending = JournalEntry {
        id: "native-reconcile".into(),
        op_type: FileOperationType::Move,
        state: JournalState::Prepared,
        source: source_text.clone(),
        destination: Some(photos.join("A.jpg").to_string_lossy().into_owned()),
        size: i64::try_from(before.length).expect("native size"),
        last_write_utc: JournalDateTime::utc(before.last_write_utc),
        timestamp_utc: JournalDateTime::utc(DateTimeUtc::from_ticks(STAMP).expect("old prepared stamp")),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    };
    journal.append(&pending).expect("prepared reconcile line");
    fs.move_file(&source, &photos.join("A.jpg"))
        .expect("interrupted native rename");
    let reconciled = journal
        .reconcile_pending_operations(Some(DateTimeUtc::from_ticks(STAMP + 2).expect("threshold")))
        .expect("native reconcile");
    assert_eq!(
        reconciled.last().expect("reconciled line").state,
        JournalState::Committed
    );
    assert_eq!(stored(), vec!["A.jpg"]);
    assert_eq!(
        fs.get_file_stat(&source).expect("reconciled stat").length,
        before.length
    );
}
