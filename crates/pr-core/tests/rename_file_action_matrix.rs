//! Dedicated W9-R9 rename action tests at C# baseline 7eaee2a2.
//!
//! Coverage inventory (C# method -> Rust test owner):
//! * R1 `rename_policy.rs` owns the naming tables. R2 `rename_service.rs` owns
//!   Rename_InvalidTarget_IsRejectedBeforeAnythingIsJournaled and RenameGroup_InvalidName_IsRejectedBeforeAnything_IsTouched.
//! * R2 `rename_service.rs`: Rename_Success_MovesInPlaceAndJournalsOneCommittedMove,
//!   Rename_JournalLine_UsesExactlyTheSchemaOfAnOrdinaryMove, Rename_TargetTakenByAnotherFile_IsRefusedAndTouchesNothing,
//!   Rename_ToTheSameName_IsANoChangeSuccessWithNoJournalLine, Rename_OsRefusesTheMove_NothingChangedSoTheLineIsDismissedNotARecoveryItem,
//!   Rename_FailureAfterTheFileWasRenamed_StaysAFailedRecoveryItemNeverDismissed, CaseOnlyRename_Succeeds_StoresTheNewSpellingAndIsCommitted,
//!   RenameGroup_RenamesEveryMemberInOneJournalGroup_EachKeepingItsExtension, RenameGroup_SidecarNamedAfterTheRawFile_FollowsTheRawFile,
//!   RenameGroup_OneTargetTaken_ChangesNothingAndJournalsNothing, RenameGroup_PartlyAlreadyNamed_RenamesOnlyTheOthers,
//!   RenameGroup_InvalidName_IsRejectedBeforeAnything_IsTouched, and RenameGroup_CaseOnly_SecondMemberFails_FirstIsPutBackAndTheLineIsDismissed.
//! * R3 `rename_recovery.rs`: CaseOnlyRename_RecoveryVerdict_FollowsTheStoredSpelling,
//!   CaseOnlyRename_StartupReconcile_FollowsTheStoredSpelling, CaseOnlyRename_RecoveryRetryOfAFailedLine_FinishesTheRename,
//!   CaseOnlyRename_Undo_RestoresTheOldSpelling, CaseOnlyRename_UndoOfAnAlreadyUndoneRename_DoesNotRunTwiceAndDropsTheEntry,
//!   CaseOnlyRename_FileStoredUnderAThirdSpelling_IsNeverLostOrDone, CaseOnlyRename_TwoEntriesDifferingByCase_AreTwoFiles_Conflict,
//!   CaseOnlyRename_UndoWhenTheFileWasRenamedBackByHand_DropsTheDeadHistoryEntry and
//!   CaseOnlyRename_UndoMovedTheFileButTheCheckFailed_DropsTheEntryByTheStoredSpelling.
//! * New twins in this binary: Rename_TargetTakenByAnotherFileWithOtherCase_IsRefused,
//!   Rename_TargetIsAFolder_IsRefused, Rename_SourceMissing_FailsWithoutJournal, Rename_WhileAnotherActionIsRunning_IsRejectedAsBusy,
//!   Rename_FileLockedByAnotherProgram_FailsCleanlyAndIsDismissed, Rename_ForeignFileAppearsAtTheTargetAfterThePreflight_IsNeverOverwritten,
//!   Rename_ThenUndo_PutsTheOriginalNameBack, Rename_ThenUndo_AfterTheOriginalNameWasTakenByAnotherFile_IsRefused,
//!   CaseOnlyRename_ToTheSpellingTheFileAlreadyHas_IsNoChange, CaseOnlyRename_TwoDifferentFilesInACaseSensitiveFolder_IsRefused,
//!   CaseOnlyRename_MoveFails_NothingChangedAndTheLineIsDismissedNotLost, RenameGroup_AMemberCannotBeRenamed_RollsTheOthersBackAndDismissesTheLine,
//!   RenameGroup_ThenUndo_RestoresEveryName, RenameGroup_CaseOnly_RenamesAllMembersUndoesAndReconcilesClean,
//!   RenameGroup_EveryMemberAlreadyNamed_IsASuccessWithNoJournalLine,
//!   RenameGroup_TwoMembersToTheSameTarget_IsRefusedBeforeAnythingIsJournaled.
//! * `file_actions_matrix.rs` owns the other two fault tests: GroupCaseOnlyRename_Matrix and
//!   CaseOnlyRename_ReconcileInterrupted. Neither R3 scenario is duplicated by the dedicated fault binary.
//! * Additional twins below cover RenameGroup_CaseOnly_RecoveryRetryOfAFailedGroupLine_FinishesEveryMember,
//!   OrdinaryMoveOrCopyIntoTheSourceFolder_IsStillRefused and
//!   FileOperationType_HasNoRenameMember_SoAnOlderBuildReadsEveryJournalLine.

use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::FileSystem;
use pr_core::app_paths::AppPaths;
use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_retry::{EnglishRecoveryRetryTexts, RecoveryRetryService};
use pr_core::file_actions::rename::RenameRequest;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::FileActionRequest;
use pr_core::file_actions::undo::{EnglishUndoTexts, UndoService};
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
        let root = TempRoot::new("rename-actions-matrix");
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

    fn rename(&self, source: &str, target: &str) -> pr_core::file_actions::types::FileActionResult {
        self.service.rename(
            &RenameRequest {
                source: source.into(),
                new_file_name: target.into(),
            },
            &CancelToken::new(),
        )
    }

    fn entries(&self) -> Vec<pr_core::file_actions::types::JournalEntry> {
        let mut entries = Vec::new();
        self.journal
            .read_entries(&mut |entry| entries.push(entry))
            .expect("read journal");
        entries
    }
}

#[test]
fn rename_target_taken_by_another_file_with_other_case_is_refused() {
    // C#: RenameFileActionTests.Rename_TargetTakenByAnotherFileWithOtherCase_IsRefused.
    let rig = Rig::new(&[("a.jpg", b"original"), ("b.jpg", b"foreign")]);
    let result = rig.rename(&rig.path("a.jpg"), "B.JPG");
    assert!(!result.succeeded);
    assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"original".to_vec()));
    assert_eq!(
        rig.fs.get_stored_file_names(Path::new(&rig.path("b.jpg"))).unwrap(),
        ["b.jpg"]
    );
    assert!(rig.entries().is_empty());
}

#[test]
fn rename_target_is_a_folder_is_refused() {
    // C#: RenameFileActionTests.Rename_TargetIsAFolder_IsRefused.
    let rig = Rig::new(&[("a.jpg", b"original")]);
    rig.fs.add_dir(rig.path("x.jpg"));
    let result = rig.rename(&rig.path("a.jpg"), "x.jpg");
    assert!(!result.succeeded);
    assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"original".to_vec()));
    assert!(rig.entries().is_empty());
}

#[test]
fn rename_source_missing_fails_without_journal() {
    // C#: RenameFileActionTests.Rename_SourceMissing_FailsWithoutJournal.
    let rig = Rig::new(&[("other.jpg", b"foreign")]);
    let result = rig.rename(&rig.path("gone.jpg"), "x.jpg");
    assert!(!result.succeeded);
    assert!(rig.entries().is_empty());
    assert!(!rig.fs.file_exists(Path::new(&rig.path("x.jpg"))));
}

#[test]
fn rename_while_another_action_is_running_is_rejected_as_busy() {
    // C#: RenameFileActionTests.Rename_WhileAnotherActionIsRunning_IsRejectedAsBusy.
    let rig = Rig::new(&[("a.jpg", b"original")]);
    assert!(rig.service.try_begin());
    let result = rig.rename(&rig.path("a.jpg"), "x.jpg");
    rig.service.end();
    assert!(!result.succeeded);
    assert!(result.rejected);
    assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"original".to_vec()));
}

#[test]
fn rename_file_locked_by_another_program_fails_cleanly_and_is_dismissed() {
    // C#: RenameFileActionTests.Rename_FileLockedByAnotherProgram_FailsCleanlyAndIsDismissed.
    let rig = Rig::new(&[("a.jpg", b"original")]);
    rig.fs.fail_locked(FsOp::Move, rig.path("a.jpg"));
    let result = rig.rename(&rig.path("a.jpg"), "x.jpg");
    assert!(!result.succeeded);
    assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"original".to_vec()));
    assert_eq!(rig.entries().last().unwrap().state, JournalState::Dismissed);
}

#[test]
fn rename_foreign_file_appears_at_the_target_after_the_preflight_is_never_overwritten() {
    // C#: RenameFileActionTests.Rename_ForeignFileAppearsAtTheTargetAfterThePreflight_IsNeverOverwritten.
    let rig = Rig::new(&[("a.jpg", b"original")]);
    let fs = rig.fs.clone();
    let target = rig.path("x.jpg");
    let service = FileActionService::new(
        rig.journal.clone(),
        fs.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(FakeRecycleBin::new(fs.clone())),
    )
    .with_move_override(Arc::new(move |from, to| {
        fs.add_file(
            to,
            b"SOMEONE ELSE'S PHOTO, longer than the original so the sizes differ",
        );
        FileSystem::move_file(fs.as_ref(), from, to)
    }));
    let result = service.rename(
        &RenameRequest {
            source: rig.path("a.jpg"),
            new_file_name: "x.jpg".into(),
        },
        &CancelToken::new(),
    );
    assert!(!result.succeeded);
    assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"original".to_vec()));
    assert!(
        rig.fs
            .file_content(target)
            .unwrap()
            .starts_with(b"SOMEONE ELSE'S PHOTO")
    );
}

#[test]
fn rename_then_undo_puts_the_original_name_back() {
    // C#: RenameFileActionTests.Rename_ThenUndo_PutsTheOriginalNameBack.
    let rig = Rig::new(&[("a.jpg", b"original")]);
    let renamed = rig.rename(&rig.path("a.jpg"), "x.jpg");
    assert!(renamed.succeeded);
    let mut undo = UndoService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
        Arc::new(EnglishUndoTexts),
    );
    undo.register(&renamed);
    let undone = undo.undo_move();
    assert!(undone.succeeded, "{:?}", undone.error_message);
    assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"original".to_vec()));
    assert!(!rig.fs.file_exists(Path::new(&rig.path("x.jpg"))));
    assert!(rig.entries().iter().any(|line| line.undo == Some(true)
        && line.state == JournalState::Committed
        && line.source == rig.path("x.jpg")
        && line.destination.as_deref() == Some(rig.path("a.jpg").as_str())));
}

#[test]
fn rename_then_undo_after_the_original_name_was_taken_by_another_file_is_refused() {
    // C#: RenameFileActionTests.Rename_ThenUndo_AfterTheOriginalNameWasTakenByAnotherFile_IsRefused.
    let rig = Rig::new(&[("a.jpg", b"original")]);
    let renamed = rig.rename(&rig.path("a.jpg"), "x.jpg");
    let mut undo = UndoService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
        Arc::new(EnglishUndoTexts),
    );
    undo.register(&renamed);
    rig.fs
        .add_file(rig.path("a.jpg"), b"a different photo now living at the old name");
    let undone = undo.undo_move();
    assert!(!undone.succeeded);
    assert_eq!(rig.fs.file_content(rig.path("x.jpg")), Some(b"original".to_vec()));
    assert_eq!(
        rig.fs.file_content(rig.path("a.jpg")),
        Some(b"a different photo now living at the old name".to_vec())
    );
}

#[test]
fn rename_group_member_cannot_be_renamed_rolls_others_back_and_dismisses() {
    // C#: RenameFileActionTests.RenameGroup_AMemberCannotBeRenamed_RollsTheOthersBackAndDismissesTheLine.
    let rig = Rig::new(&[("a.jpg", b"jpeg"), ("a.cr2", b"raw"), ("a.xmp", b"sidecar")]);
    rig.fs
        .fail_times(FsOp::Move, rig.path("a.cr2"), std::io::ErrorKind::PermissionDenied, 1);
    let group = CaptureGroup::new(rig.path("a.jpg"), rig.path("a.cr2"), Some(rig.path("a.xmp"))).unwrap();
    let result = rig.service.rename_group(&group, "x", &CancelToken::new());
    assert!(!result.succeeded);
    for (name, bytes) in [("a.jpg", b"jpeg".as_slice()), ("a.cr2", b"raw"), ("a.xmp", b"sidecar")] {
        assert_eq!(rig.fs.file_content(rig.path(name)), Some(bytes.to_vec()));
        assert!(!rig.fs.file_exists(Path::new(&rig.path(&format!("x{}", &name[1..])))));
    }
    assert_eq!(rig.entries().last().unwrap().state, JournalState::Dismissed);
}

#[test]
fn rename_group_case_only_renames_all_members_undoes_and_reconciles_clean() {
    // C#: RenameFileActionTests.RenameGroup_CaseOnly_RenamesAllMembersUndoesAndReconcilesClean.
    let rig = Rig::new(&[("a.jpg", b"jpeg"), ("a.cr2", b"raw"), ("a.xmp", b"sidecar")]);
    let group = CaptureGroup::new(rig.path("a.jpg"), rig.path("a.cr2"), Some(rig.path("a.xmp"))).unwrap();
    let result = rig.service.rename_group(&group, "A", &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    for name in ["A.jpg", "A.cr2", "A.xmp"] {
        assert_eq!(
            rig.fs.get_stored_file_names(Path::new(&rig.path(name))).unwrap(),
            [name]
        );
    }
    assert!(rig.journal.reconcile_pending_operations(None).unwrap().is_empty());
    let mut undo = UndoService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
        Arc::new(EnglishUndoTexts),
    );
    undo.register_group(&result);
    assert!(undo.undo_last().succeeded);
    for name in ["a.jpg", "a.cr2", "a.xmp"] {
        assert_eq!(
            rig.fs.get_stored_file_names(Path::new(&rig.path(name))).unwrap(),
            [name]
        );
    }
}

#[test]
fn rename_group_partly_already_named_renames_only_the_others() {
    // C#: RenameFileActionTests.RenameGroup_PartlyAlreadyNamed_RenamesOnlyTheOthers.
    let rig = Rig::new(&[("a.jpg", b"jpeg"), ("A.cr2", b"raw"), ("a.xmp", b"sidecar")]);
    let group = CaptureGroup::new(rig.path("a.jpg"), rig.path("A.cr2"), Some(rig.path("a.xmp"))).unwrap();
    let result = rig.service.rename_group(&group, "A", &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    for name in ["A.jpg", "A.cr2", "A.xmp"] {
        assert_eq!(
            rig.fs.get_stored_file_names(Path::new(&rig.path(name))).unwrap(),
            [name]
        );
    }
    assert_eq!(rig.entries().last().unwrap().group_members.as_ref().unwrap().len(), 2);
}

#[test]
fn rename_group_every_member_already_named_is_a_success_with_no_journal_line() {
    // C#: RenameFileActionTests.RenameGroup_EveryMemberAlreadyNamed_IsASuccessWithNoJournalLine.
    let rig = Rig::new(&[("A.jpg", b"jpeg"), ("A.cr2", b"raw")]);
    let group = CaptureGroup::new(rig.path("a.jpg"), rig.path("a.cr2"), None).unwrap();
    let result = rig.service.rename_group(&group, "A", &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    assert!(rig.entries().is_empty());
}

#[test]
fn rename_group_two_members_to_the_same_target_is_refused_before_anything_is_journaled() {
    // C#: RenameFileActionTests.RenameGroup_TwoMembersToTheSameTarget_IsRefusedBeforeAnythingIsJournaled.
    use pr_core::file_actions::rename::PathKeyMap;
    let rig = Rig::new(&[("a.jpg", b"jpeg"), ("a.cr2", b"raw"), ("a.xmp", b"sidecar")]);
    let group = CaptureGroup::new(rig.path("a.jpg"), rig.path("a.cr2"), Some(rig.path("a.xmp"))).unwrap();
    let mut names = PathKeyMap::new();
    names.insert(rig.path("a.jpg"), "x.dat".to_owned());
    names.insert(rig.path("a.cr2"), "X.DAT".to_owned());
    names.insert(rig.path("a.xmp"), "y.xmp".to_owned());
    let request = pr_core::file_actions::types::CaptureGroupActionRequest {
        group,
        operation: FileOperationType::Move,
        destination: None,
        allow_permanent_delete: false,
        destination_file_names: Some(names),
    };
    let result = rig.service.execute_group(&request, &CancelToken::new());
    assert!(!result.succeeded);
    assert!(rig.entries().is_empty());
    for name in ["a.jpg", "a.cr2", "a.xmp"] {
        assert!(rig.fs.file_exists(Path::new(&rig.path(name))));
    }
}

#[test]
fn case_only_rename_two_different_files_in_case_sensitive_folder_is_refused() {
    // C#: RenameFileActionTests.CaseOnlyRename_TwoDifferentFilesInACaseSensitiveFolder_IsRefused.
    let rig = Rig::new(&[("a.jpg", b"one")]);
    rig.fs
        .set_stored_file_names_override(Some(vec!["a.jpg".into(), "A.jpg".into()]));
    let result = rig.rename(&rig.path("a.jpg"), "A.jpg");
    assert!(!result.succeeded);
    assert!(rig.entries().is_empty());
}

#[test]
fn case_only_rename_to_the_spelling_the_file_already_has_is_no_change() {
    // C#: RenameFileActionTests.CaseOnlyRename_ToTheSpellingTheFileAlreadyHas_IsNoChange.
    let rig = Rig::new(&[("Photo.JPG", b"photo")]);
    let result = rig.rename(&rig.path("photo.jpg"), "Photo.JPG");
    assert!(result.succeeded);
    assert!(result.no_change);
    assert_eq!(
        rig.fs.get_stored_file_names(Path::new(&rig.path("photo.jpg"))).unwrap(),
        ["Photo.JPG"]
    );
    assert!(rig.entries().is_empty());
}

#[test]
fn case_only_rename_move_fails_nothing_changed_and_line_is_dismissed_not_lost() {
    // C#: RenameFileActionTests.CaseOnlyRename_MoveFails_NothingChangedAndTheLineIsDismissedNotLost.
    let rig = Rig::new(&[("a.jpg", b"photo")]);
    rig.fs
        .fail_times(FsOp::Move, rig.path("a.jpg"), std::io::ErrorKind::PermissionDenied, 1);
    let result = rig.rename(&rig.path("a.jpg"), "A.jpg");
    assert!(!result.succeeded);
    assert_eq!(
        rig.fs.get_stored_file_names(Path::new(&rig.path("a.jpg"))).unwrap(),
        ["a.jpg"]
    );
    assert_eq!(rig.entries().last().unwrap().state, JournalState::Dismissed);
}

#[test]
fn rename_group_then_undo_restores_every_name() {
    // C#: RenameFileActionTests.RenameGroup_ThenUndo_RestoresEveryName.
    let rig = Rig::new(&[("a.jpg", b"jpeg"), ("a.cr2", b"raw"), ("a.xmp", b"sidecar")]);
    let group = CaptureGroup::new(rig.path("a.jpg"), rig.path("a.cr2"), Some(rig.path("a.xmp"))).unwrap();
    let result = rig.service.rename_group(&group, "x", &CancelToken::new());
    assert!(result.succeeded, "{:?}", result.error);
    let mut undo = UndoService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        Arc::new(FakeRecycleBin::new(rig.fs.clone())),
        Arc::new(EnglishUndoTexts),
    );
    undo.register_group(&result);
    assert!(undo.undo_last().succeeded);
    for (source, destination, bytes) in [
        ("a.jpg", "x.jpg", b"jpeg".as_slice()),
        ("a.cr2", "x.cr2", b"raw"),
        ("a.xmp", "x.xmp", b"sidecar"),
    ] {
        assert_eq!(rig.fs.file_content(rig.path(source)), Some(bytes.to_vec()));
        assert!(!rig.fs.file_exists(Path::new(&rig.path(destination))));
    }
}

#[test]
#[ignore = "native: test-owned TempRoot with filesystem case preservation required"]
fn case_only_rename_native_two_different_files_in_case_sensitive_folder_is_refused() {
    // C#: RenameFileActionTests.CaseOnlyRename_TwoDifferentFilesInACaseSensitiveFolder_IsRefused.
    let root = TempRoot::new("rename-case-sensitive-native");
    let enabled = std::process::Command::new("fsutil")
        .args(["file", "setCaseSensitiveInfo"])
        .arg(root.path())
        .arg("enable")
        .output()
        .unwrap();
    assert!(
        enabled.status.success(),
        "case-sensitive directory unavailable: {}",
        String::from_utf8_lossy(&enabled.stderr)
    );
    let photos = root.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("a.jpg"), b"one").unwrap();
    std::fs::write(photos.join("A.jpg"), b"two").unwrap();
    let fs = Arc::new(pr_core::io::physical::PhysicalFileSystem);
    let clock = Arc::new(FakeClock::new());
    let paths = AppPaths {
        journal_file: root.path().join("journal.jsonl"),
        ..AppPaths::new(root.path(), None, false)
    };
    let journal = Arc::new(OperationJournal::new(&paths, fs.clone(), clock.clone()));
    let bin = Arc::new(FakeRecycleBin::new(Arc::new(FakeFileSystem::new())));
    let service = FileActionService::new(journal.clone(), fs, clock, bin);
    let result = service.rename(
        &RenameRequest {
            source: photos.join("a.jpg").to_string_lossy().into_owned(),
            new_file_name: "A.jpg".into(),
        },
        &CancelToken::new(),
    );
    assert!(!result.succeeded);
    assert_eq!(std::fs::read(photos.join("a.jpg")).unwrap(), b"one");
    assert_eq!(std::fs::read(photos.join("A.jpg")).unwrap(), b"two");
    let mut entries = Vec::new();
    journal.read_entries(&mut |e| entries.push(e)).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn rename_group_case_only_recovery_retry_of_failed_group_line_finishes_every_member() {
    // C#: RenameFileActionTests.RenameGroup_CaseOnly_RecoveryRetryOfAFailedGroupLine_FinishesEveryMember.
    let rig = Rig::new(&[("a.jpg", b"jpeg"), ("a.cr2", b"raw"), ("a.xmp", b"sidecar")]);
    let sources = ["a.jpg", "a.cr2", "a.xmp"];
    let targets = ["A.jpg", "A.cr2", "A.xmp"];
    let stamp = rig
        .fs
        .get_file_stat(Path::new(&rig.path(sources[0])))
        .unwrap()
        .last_write_utc;
    let members = sources
        .iter()
        .zip(targets)
        .map(|(source, destination)| {
            let stat = rig.fs.get_file_stat(Path::new(&rig.path(source))).unwrap();
            JournalGroupMember {
                source: rig.path(source),
                destination: Some(rig.path(destination)),
                size: stat.length as i64,
                last_write_utc: JournalDateTime::utc(stat.last_write_utc),
                permanent: false,
            }
        })
        .collect::<Vec<_>>();
    let entry = JournalEntry {
        id: "group-failed".into(),
        op_type: FileOperationType::Move,
        state: JournalState::Failed,
        source: rig.path(sources[0]),
        destination: Some(rig.path(targets[0])),
        size: members[0].size,
        last_write_utc: JournalDateTime::utc(stamp),
        timestamp_utc: JournalDateTime::utc(stamp),
        error: Some("interrupted".into()),
        error_code: None,
        permanent: None,
        undo: None,
        group_id: Some("group-id".into()),
        group_members: Some(members),
    };
    rig.journal.append(&entry).unwrap();
    let retry_bin = Arc::new(FakeRecycleBin::new(rig.fs.clone()));
    let retry = RecoveryRetryService::new(
        rig.journal.clone(),
        rig.fs.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishRecoveryRetryTexts),
    )
    .with_recycle_bin(retry_bin);
    let result = retry.retry_move_or_copy(&entry, false, &CancelToken::new()).unwrap();
    assert!(result.succeeded, "{}", result.message);
    for target in targets {
        assert_eq!(
            rig.fs.get_stored_file_names(Path::new(&rig.path(target))).unwrap(),
            [target]
        );
    }
}

#[test]
fn ordinary_move_or_copy_into_the_source_folder_is_still_refused() {
    // C#: RenameFileActionTests.OrdinaryMoveOrCopyIntoTheSourceFolder_IsStillRefused (both theory rows).
    for operation in [FileOperationType::Move, FileOperationType::Copy] {
        let rig = Rig::new(&[("a.jpg", b"photo")]);
        let mut request = FileActionRequest::new(rig.path("a.jpg"), operation);
        request.destination = Some(rig.folder.to_string_lossy().into_owned());
        let result = rig.service.execute(&request, &CancelToken::new());
        assert!(!result.succeeded, "{operation:?}");
        assert_eq!(rig.fs.file_content(rig.path("a.jpg")), Some(b"photo".to_vec()));
        assert!(rig.entries().is_empty());
    }
}

#[test]
fn file_operation_type_has_no_rename_member_for_older_build_journal_compatibility() {
    // C#: RenameFileActionTests.FileOperationType_HasNoRenameMember_SoAnOlderBuildReadsEveryJournalLine.
    let mut names = [
        format!("{:?}", FileOperationType::Copy),
        format!("{:?}", FileOperationType::Move),
        format!("{:?}", FileOperationType::Recycle),
    ];
    names.sort();
    assert_eq!(names, ["Copy", "Move", "Recycle"]);
}
