//! Tests for `pr_core::file_actions::service` capture-group actions (`FileActionService::execute_group`, F-06) and the
//! journal-level rules the service must keep (gate, durability, cleanup). Port of
//! `PhotoReview.Core.Tests/FileActions/FileActionServiceGroupMutationGapTests.cs`, `CaptureGroupActionRollbackTests.cs`,
//! `CaptureGroupActionGapTests.cs`, `CaptureGroupActionServiceTests.cs` (the three preflight/manifest tests),
//! `CaptureGroupSidecarAndGateTests.cs`, `CopyForeignDestinationTests.cs` (group part), `CopyRollbackIdentityTests.cs`
//! (group part), `FileActionDestinationFolderCleanupTests.cs` (group part), `FileActionMutationAuditBTests.cs` (group part),
//! `PermanentDeleteTests.cs` (group part), `RecycleBinCapacityPermanentMemberTests.cs`,
//! `FileActionFaultInjectionGapTests.cs` (group cancel), `JournalConcurrencyTests.cs`, `JournalDurabilityTests.cs`.
//!
//! Baseline d92d6aae. Red until the W3-04 bodies land (every test that reaches the service panics with `todo!()`).
//! Each test carries a `// C#:` comment naming the C# test it ports. Nothing touches the real Recycle Bin ([`ScenarioBin`]).
//!
//! Open items:
//! * `TODO(W3-07)`: the C# `RecoveryFileCheck` assertions are replaced by disk-state assertions until W3-07 merges.
//! * `TODO(W3-05)`: the two `UndoLastAsync_*` tests need the real `UndoService` and go green only after W3-05 merges.
//! * `DIVERGENCE:` marks the C# behaviour the Rust port reproduces differently by construction (no thread hop, no
//!   `catch (Exception)` arm for a panic; see each test).

mod common_file_actions;

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::time::Duration;

/// Hang guard for every blocking hand-off between test threads (AGENTS.md: never run tests unbounded).
const HAND_OFF: Duration = Duration::from_secs(10);

use common_file_actions::{
    Fixture, LAST_WRITE_TICKS, english_code_text, english_texts, fail_journal_append_nth, group, journal_path,
    last_write_plus, stamp,
};
use pr_core::abstractions::FileSystem;
use pr_core::file_actions::errors::{
    CANCELLED_BY_USER, MOVE_SOURCE_NOT_REMOVED, SOURCE_STILL_EXISTS_AFTER_RECOVERY, VERIFY_SIZE_CHANGED,
};
use pr_core::file_actions::journal::OperationJournalOptions;
use pr_core::file_actions::service::{FileActionService, FileActionTexts, OPERATION_CANCELED_MESSAGE};
use pr_core::file_actions::types::CaptureGroupActionResult;
use pr_core::file_actions::undo::{MoveOverride, UndoService};
use pr_core::model::{FileOperationType, JournalDurability, JournalState};
use pr_core::sync::CancelToken;
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};

use FileOperationType::{Copy, Move, Recycle};

/// The JPEG of the group scenarios.
const JPEG: &str = r"C:\photos\IMG_0001.JPG";
/// The RAW of the group scenarios.
const RAW: &str = r"C:\photos\IMG_0001.CR3";
/// The XMP sidecar of the group scenarios.
const XMP: &str = r"C:\photos\IMG_0001.xmp";
/// The destination folder of the group scenarios.
const SEL: &str = r"C:\photos\sel";

/// Adds JPEG (40 bytes), RAW (60 bytes) and XMP (5 bytes), all stamped [`LAST_WRITE_TICKS`] (golden group layout).
fn add_triple(fx: &Fixture) {
    fx.add_file(JPEG, 40);
    fx.add_file(RAW, 60);
    fx.add_file(XMP, 5);
}

/// Adds JPEG (40 bytes) and RAW (60 bytes).
fn add_pair(fx: &Fixture) {
    fx.add_file(JPEG, 40);
    fx.add_file(RAW, 60);
}

/// The journal's final state.
fn last_state(fx: &Fixture) -> Option<JournalState> {
    fx.journal_entries().pop().map(|e| e.state)
}

/// The rollback note the user sees when `stuck` members could not be put back (C# `Tr.CoreGroupActionRollbackFailed`).
fn rollback_failed_text(message: &str, stuck: usize) -> String {
    english_texts().core_group_action_rollback_failed(message, stuck)
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionServiceGroupMutationGapTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveOrCopyOnDriveWithoutRecycleBin_SucceedsAndNothingIsPermanent (Theory: Move, Copy)
#[test]
fn execute_group_move_or_copy_on_drive_without_recycle_bin_succeeds_and_nothing_is_permanent() {
    for op in [Move, Copy] {
        let fx = Fixture::new();
        add_pair(&fx);
        fx.bin.set_no_bin_roots(&[r"C:\"]);
        let result = fx.run_group(&group(JPEG, RAW, None, op, Some("sel")));
        assert!(result.succeeded, "{op:?}");
        assert!(!result.permanently_deleted, "{op:?}");
    }
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_RecycleEveryMemberOnDriveWithoutBin_DeletesPermanentlyAndFlagsEntryAndMembers
#[test]
fn execute_group_recycle_every_member_on_drive_without_bin_deletes_permanently_and_flags_entry_and_members() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    let mut request = group(JPEG, RAW, None, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run_group(&request);
    assert!(result.succeeded);
    assert!(result.permanently_deleted);
    assert_eq!(result.entry.as_ref().and_then(|e| e.permanent), Some(true));
    assert!(result.members.iter().all(|m| m.member.permanent));
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_RecycleWithOneMemberOnDriveWithoutBin_FlagsPermanentDeletionButNotTheWholeEntry
#[test]
fn execute_group_recycle_with_one_member_on_drive_without_bin_flags_permanent_deletion_but_not_the_whole_entry() {
    let fx = Fixture::new();
    add_triple(&fx);
    // Only the sidecar sits on a folder without a bin.
    fx.bin.set_no_bin_roots(&[XMP]);
    let mut request = group(JPEG, RAW, Some(XMP), Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run_group(&request);
    assert!(result.succeeded);
    assert!(result.permanently_deleted, "one member was deleted permanently");
    assert_eq!(
        result.entry.as_ref().and_then(|e| e.permanent),
        None,
        "not every member is permanent"
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_RecycleOnDriveWithoutBinAndNoOptIn_IsRefusedBeforeAnythingHappens
#[test]
fn execute_group_recycle_on_drive_without_bin_and_no_opt_in_is_refused_before_anything_happens() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(!result.succeeded);
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW));
    assert!(fx.journal_lines().is_empty(), "refused before the Prepared line");
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_RecycleWithBin_IsNotFlaggedPermanent
#[test]
fn execute_group_recycle_with_bin_is_not_flagged_permanent() {
    let fx = Fixture::new();
    add_pair(&fx);
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(result.succeeded);
    assert!(!result.permanently_deleted);
    assert_eq!(fx.bin.recycled().len(), 2);
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_PermanentDeleteLeavesTheFileBehind_FailsAsSourceStillExists
#[test]
fn execute_group_permanent_delete_leaves_the_file_behind_fails_as_source_still_exists() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    fx.bin.leave_on_permanent(RAW);
    let mut request = group(JPEG, RAW, None, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run_group(&request);
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_code_text(SOURCE_STILL_EXISTS_AFTER_RECOVERY).as_str())
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_CommitAppendFails_ReportsSuccessWithJournalNotPersisted
#[test]
fn execute_group_commit_append_fails_reports_success_with_journal_not_persisted() {
    let fx = Fixture::new();
    add_pair(&fx);
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(result.succeeded);
    assert!(!result.journal_persisted);
    assert!(result.journal_error.is_some());
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_RolledBackOutcomeAppendFails_ReportsJournalNotPersisted
#[test]
fn execute_group_rolled_back_outcome_append_fails_reports_journal_not_persisted() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if source.to_string_lossy().eq_ignore_ascii_case(RAW) {
            Some(io::Error::other("disk on fire"))
        } else {
            None
        }
    })));
    // Prepared is append 1, the rolled-back Dismissed line is append 2.
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(!result.journal_persisted);
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_RecycleSecondMemberFails_KeepsOriginalErrorAndReportsNoPermanentDeletion
#[test]
fn execute_group_recycle_second_member_fails_keeps_original_error_and_reports_no_permanent_deletion() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.fail_send(RAW, "boom");
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some("boom"),
        "recycle is never compensated: the original error is kept"
    );
    assert!(!result.permanently_deleted);
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_RecycleFirstMemberFails_IsAFailedRecoveryItemNotADismissedRollback
#[test]
fn execute_group_recycle_first_member_fails_is_a_failed_recovery_item_not_a_dismissed_rollback() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.fail_send(JPEG, "shell said no");
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(!result.succeeded);
    assert_eq!(last_state(&fx), Some(JournalState::Failed));
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_PermanentDeleteSecondMemberFails_ReportsTheCompletedPermanentDeletion
#[test]
fn execute_group_permanent_delete_second_member_fails_reports_the_completed_permanent_deletion() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    fx.bin.fail_permanent(RAW, "device not ready");
    let mut request = group(JPEG, RAW, None, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run_group(&request);
    assert!(!result.succeeded);
    assert!(result.permanently_deleted, "the first member was permanently deleted");
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_PermanentDeleteFirstMemberFails_ReportsNoPermanentDeletion
#[test]
fn execute_group_permanent_delete_first_member_fails_reports_no_permanent_deletion() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    fx.bin.fail_permanent(JPEG, "device not ready");
    let mut request = group(JPEG, RAW, None, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run_group(&request);
    assert!(!result.succeeded);
    assert!(!result.permanently_deleted);
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveOfCompleteGroup_ReportsNoSkippedMissing
#[test]
fn execute_group_move_of_complete_group_reports_no_skipped_missing() {
    let fx = Fixture::new();
    add_triple(&fx);
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(result.succeeded);
    assert_eq!(result.skipped_missing, None, "None, never Some(empty)");
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveFailsAndIsRolledBack_ReportsSkippedMissingOnlyWhenAPartnerVanished
#[test]
fn execute_group_move_fails_and_is_rolled_back_reports_skipped_missing_only_when_a_partner_vanished() {
    let fx = Fixture::new();
    add_triple(&fx);
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if source.to_string_lossy().eq_ignore_ascii_case(XMP) {
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        result.skipped_missing, None,
        "no partner vanished, so nothing is reported as skipped"
    );
    assert!(
        fx.file_exists(JPEG) && fx.file_exists(RAW) && fx.file_exists(XMP),
        "rolled back"
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_CompensationThrowsUnexpectedType_ReportsSkippedMissingOnlyWhenAPartnerVanished
// Open question 2, option B (recommended, P-level): a panic inside a file-system call is not a recoverable state. The
// panic propagates out of the service; the RAII guard releases the gate; the Prepared line stays for reconcile.
// Theory over `sidecarMissing`: the RAW forward move fails, the rollback move of the moved JPEG blows up (C#
// `InvalidOperationException`, Rust: a panic). `skipped_missing` cannot be observed (no result comes back under B); what
// is pinned is the disk/journal outcome, for both variants.
#[test]
fn execute_group_compensation_throws_unexpected_type_reports_skipped_missing_only_when_a_partner_vanished() {
    for sidecar_missing in [true, false] {
        let fx = Fixture::new();
        add_pair(&fx);
        if !sidecar_missing {
            fx.add_file(XMP, 5);
        }
        fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
            if source.to_string_lossy().eq_ignore_ascii_case(RAW) {
                return Some(io::Error::other("simulated"));
            }
            if source
                .to_string_lossy()
                .eq_ignore_ascii_case(r"C:\photos\sel\IMG_0001.JPG")
            {
                panic!("simulated unexpected fault"); // the rollback of the first member blows up
            }
            None
        })));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")))
        }));
        assert!(
            outcome.is_err(),
            "B: the panic propagates to the caller ({sidecar_missing})"
        );
        assert!(!fx.service.is_busy(), "the RAII guard released the gate");
        assert_eq!(
            last_state(&fx),
            Some(JournalState::Prepared),
            "the Prepared line stays for startup reconcile"
        );
        assert!(
            fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"),
            "the JPEG really is still moved"
        );
        assert!(fx.file_exists(RAW));
    }
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_CopyRollbackFindsTheCopyChanged_LeavesItAndCountsItAsStuck
#[test]
fn execute_group_copy_rollback_finds_the_copy_changed_leaves_it_and_counts_it_as_stuck() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            // The second member fails; before that, the first copy is replaced by another file of another size.
            if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.add_file_at(
                    Path::new(r"C:\photos\sel\IMG_0001.JPG"),
                    b"changed!",
                    stamp(LAST_WRITE_TICKS),
                );
                return Some(io::Error::other("copy failed"));
            }
            None
        })));
    let result = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"),
        "a changed copy is never deleted"
    );
    assert_eq!(
        result.error.as_deref(),
        Some(rollback_failed_text("copy failed", 1).as_str())
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_CopyRollbackCannotDeleteACompletedCopy_CountsItAsStuckWithoutEscaping
#[test]
fn execute_group_copy_rollback_cannot_delete_a_completed_copy_counts_it_as_stuck_without_escaping() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_copy_hook(Some(Arc::new(|_: &Path, destination: &Path| {
        if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
            Some(io::Error::other("copy failed"))
        } else {
            None
        }
    })));
    fx.hooks
        .set_delete_hook(Some(Arc::new(|_: &Path| Some(io::Error::other("in use")))));
    let result = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"),
        "the completed copy stays, counted as stuck"
    );
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|e| e.contains("could not be put back"))
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_CopyLeavesPartialFile_RollbackNeverTriesToDeleteIt
#[test]
fn execute_group_copy_leaves_partial_file_rollback_never_tries_to_delete_it() {
    let fx = Fixture::new();
    add_pair(&fx);
    let deletes = Arc::new(AtomicUsize::new(0));
    let counter = deletes.clone();
    fx.hooks.set_delete_hook(Some(Arc::new(move |_: &Path| {
        counter.fetch_add(1, Ordering::SeqCst);
        None
    })));
    fx.hooks
        .set_copy_partial_hook(Some(Arc::new(|_: &Path, destination: &Path| {
            if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
                Some((4, io::Error::other("disk full")))
            } else {
                None
            }
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert_eq!(
        deletes.load(Ordering::SeqCst),
        1,
        "only the first, completed copy is deleted; the partial RAW is never touched"
    );
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.CR3"),
        "the partial file is kept for recovery"
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_CopyRollbackDeletedTheCopyButCannotConfirmIt_IsNotDismissedAsRolledBack
#[test]
fn execute_group_copy_rollback_deleted_the_copy_but_cannot_confirm_it_is_not_dismissed_as_rolled_back() {
    const MOVED_JPEG: &str = r"C:\photos\sel\IMG_0001.JPG";
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_copy_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if source.to_string_lossy().eq_ignore_ascii_case(RAW) {
            Some(io::Error::other("simulated"))
        } else {
            None
        }
    })));
    // The rollback deletes the first copy, then the existence check that confirms it cannot answer (C# FileExists
    // throws; the Rust `file_exists` cannot fail, so the hook answers "still there"): stuck, although the disk is clean.
    let deleted = Arc::new(AtomicBool::new(false));
    let flag = deleted.clone();
    fx.hooks.set_delete_hook(Some(Arc::new(move |path: &Path| {
        if path.to_string_lossy().eq_ignore_ascii_case(MOVED_JPEG) {
            flag.store(true, Ordering::SeqCst);
        }
        None
    })));
    fx.faulty.set_exists_hook(Some(Arc::new(move |path: &Path| {
        (deleted.load(Ordering::SeqCst) && path.to_string_lossy().eq_ignore_ascii_case(MOVED_JPEG)).then_some(true)
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(!fx.file_exists(MOVED_JPEG));
    assert_eq!(
        result.error.as_deref(),
        Some(rollback_failed_text("simulated", 1).as_str())
    );
    assert_eq!(
        result.entry.as_ref().map(|e| e.state),
        Some(JournalState::Failed),
        "an unconfirmed rollback stays a Recovery item"
    );
    assert_eq!(last_state(&fx), Some(JournalState::Failed));
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_CopyDestinationMissingOrWrongSizeAfterCopy_FailsVerificationWithTheCodedError
#[test]
fn execute_group_copy_destination_missing_or_wrong_size_after_copy_fails_verification_with_the_coded_error() {
    const MOVED_JPEG: &str = r"C:\photos\sel\IMG_0001.JPG";
    // Theory: the copied JPEG looks missing (false) or 10 bytes longer (true) right after the copy (C# `StatFilter`).
    for wrong_length in [false, true] {
        let fx = Fixture::new();
        add_pair(&fx);
        fx.faulty
            .set_stat_filter(Some(Arc::new(move |path: &Path, stat| match stat {
                Some(stat) if path.to_string_lossy().eq_ignore_ascii_case(MOVED_JPEG) => {
                    wrong_length.then_some(pr_core::abstractions::FileStat {
                        length: stat.length + 10,
                        ..stat
                    })
                }
                other => other,
            })));
        let result = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
        assert!(!result.succeeded, "{wrong_length}");
        assert_eq!(
            result.error.as_deref(),
            Some(english_code_text(VERIFY_SIZE_CHANGED).as_str()),
            "{wrong_length}"
        );
        // Through the filter the disk then looks unchanged (no copy observable at the destination, sources intact), so
        // the group counts as fully rolled back: Dismissed, no error code (same C# rule, G8.5).
        assert_eq!(last_state(&fx), Some(JournalState::Dismissed), "{wrong_length}");
        // R01b: a copy whose size is wrong is still the observed file and is removed again; a destination that could
        // not be observed right after the copy has no identity to prove, so it is kept.
        assert_eq!(fx.file_exists(MOVED_JPEG), !wrong_length, "{wrong_length}");
    }
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveFailsAndTheMovedMemberIsAlreadyBackAtItsSource_IsFullyRolledBackWithoutRollbackNote
#[test]
fn execute_group_move_fails_and_the_moved_member_is_already_back_at_its_source_is_fully_rolled_back_without_rollback_note()
 {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_move_hook(Some(Arc::new(move |source: &Path, _destination: &Path| {
            if source.to_string_lossy().ends_with("IMG_0001.CR3") {
                // The RAW move fails, but the JPEG has already been moved back by "someone else".
                disk.move_file(Path::new(r"C:\photos\sel\IMG_0001.JPG"), Path::new(JPEG))
                    .expect("back");
                return Some(io::Error::other("boom"));
            }
            None
        })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some("boom"),
        "no rollback note: nothing is stuck"
    );
    assert_eq!(last_state(&fx), Some(JournalState::Dismissed));
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveReportsDestinationExistsForForeignFile_LeavesItAndAddsNoRollbackNote
#[test]
fn execute_group_move_reports_destination_exists_for_foreign_file_leaves_it_and_adds_no_rollback_note() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.add_file(r"C:\photos\sel\IMG_0001.CR3", 9);
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(
            english_texts()
                .core_file_action_destination_exists(r"C:\photos\sel\IMG_0001.CR3")
                .as_str()
        )
    );
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.CR3"),
        "the foreign file is left alone"
    );
    assert!(
        fx.file_exists(JPEG),
        "the first member was not moved: the preflight refuses the group"
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveRollbackCannotConfirmTheRestoredFile_CountsItAsStuck
#[test]
fn execute_group_move_rollback_cannot_confirm_the_restored_file_counts_it_as_stuck() {
    // Theory `hidden`: after the rollback move the restored file looks missing (true) or 10 bytes longer (false).
    for hidden in [true, false] {
        let fx = Fixture::new();
        add_pair(&fx);
        fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
            if source.to_string_lossy().eq_ignore_ascii_case(RAW) {
                Some(io::Error::other("simulated"))
            } else {
                None
            }
        })));
        let moved_back = Arc::new(AtomicBool::new(false));
        let flag = moved_back.clone();
        fx.faulty.set_after_move(Some(Arc::new(move |source: &Path, _: &Path| {
            if source
                .to_string_lossy()
                .eq_ignore_ascii_case(r"C:\photos\sel\IMG_0001.JPG")
            {
                flag.store(true, Ordering::SeqCst); // the rollback move of the first member just ran
            }
        })));
        fx.faulty
            .set_stat_filter(Some(Arc::new(move |path: &Path, stat| match stat {
                Some(stat)
                    if moved_back.load(Ordering::SeqCst) && path.to_string_lossy().eq_ignore_ascii_case(JPEG) =>
                {
                    (!hidden).then_some(pr_core::abstractions::FileStat {
                        length: stat.length + 10,
                        ..stat
                    })
                }
                other => other,
            })));
        let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
        assert!(!result.succeeded, "{hidden}");
        assert_eq!(
            result.error.as_deref(),
            Some(rollback_failed_text("simulated", 1).as_str()),
            "{hidden}"
        );
        assert_eq!(
            fx.journal_entries()
                .iter()
                .filter(|e| e.state == JournalState::Failed)
                .count(),
            1,
            "{hidden}"
        );
    }
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveRollbackMoveThrows_CountsTheMemberAsStuckWithoutEscaping
#[test]
fn execute_group_move_rollback_move_throws_counts_the_member_as_stuck_without_escaping() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        // Forward moves succeed; the compensating move back (from the sel folder) throws.
        if source.to_string_lossy().contains(r"\sel\") {
            Some(io::Error::other("cannot move back"))
        } else if source.to_string_lossy().ends_with("IMG_0001.CR3") {
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|e| e.contains("could not be put back"))
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveSourceKeptByTheMove_FailsAsMoveSourceNotRemoved
#[test]
fn execute_group_move_source_kept_by_the_move_fails_as_move_source_not_removed() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_move_leaves_source(true);
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|e| e.contains(&english_code_text(MOVE_SOURCE_NOT_REMOVED)))
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveRollbackBlockedByTouchedDestination_ReportsTheMovedMemberAsCompletedNotConflict
// The moved JPEG's destination is touched (stamp +10 s) before the RAW fails: rollback is blocked, and the inspection
// of the JPEG still reports it as completed (source gone, size right), not as a conflict.
#[test]
fn execute_group_move_rollback_blocked_by_touched_destination_reports_the_moved_member_as_completed_not_conflict() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks.set_move_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.CR3") {
            disk.set_last_write(Path::new(r"C:\photos\sel\IMG_0001.JPG"), last_write_plus(10));
            return Some(io::Error::other("boom"));
        }
        None
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    let jpeg = result
        .members
        .iter()
        .find(|m| m.member.source == JPEG)
        .expect("JPEG member");
    assert!(jpeg.completed);
    assert!(!jpeg.conflict);
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|e| e.contains("could not be put back"))
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveLeavesBothFilesOfAMember_ReportsItAsNotCompletedConflict
#[test]
fn execute_group_move_leaves_both_files_of_a_member_reports_it_as_not_completed_conflict() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_move_leaves_source(true);
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    let first = &result.members[0];
    assert!(!first.completed);
    assert!(first.conflict);
    assert!(first.source_exists && first.destination_exists);
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_MoveFailsWithSourceEditedMeanwhile_ReportsTheMemberAsConflict
#[test]
fn execute_group_move_fails_with_source_edited_meanwhile_reports_the_member_as_conflict() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks.set_move_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.JPG") {
            disk.add_file_at(source, &[0u8; 41], stamp(LAST_WRITE_TICKS));
            return Some(io::Error::other("boom"));
        }
        None
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        result.members[0].conflict,
        "the source changed: the member is a conflict"
    );
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_CopyFailsWithSourceChangedMeanwhile_ReportsTheMemberAsConflict
#[test]
fn execute_group_copy_fails_with_source_changed_meanwhile_reports_the_member_as_conflict() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks.set_copy_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.JPG") {
            disk.add_file_at(source, &[0u8; 41], stamp(LAST_WRITE_TICKS));
            return Some(io::Error::other("boom"));
        }
        None
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(result.members[0].conflict);
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_RecycleSecondMemberFails_ClassifiesTheFailedMemberBySourceIdentity
#[test]
fn execute_group_recycle_second_member_fails_classifies_the_failed_member_by_source_identity() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.fail_send(RAW, "boom");
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(!result.succeeded);
    let raw = result
        .members
        .iter()
        .find(|m| m.member.source == RAW)
        .expect("RAW member");
    assert!(raw.state_known);
    assert_eq!(raw.error.as_deref(), Some("boom"));
}

// C#: FileActionServiceGroupMutationGapTests.ExecuteGroupAsync_InspectingAMemberFails_MarksOnlyThatMemberUnknownAndStillReturns
// Theory over the exception kinds; the Rust error text is the injected io::Error's to_string(), not "fault".
#[test]
fn execute_group_inspecting_a_member_fails_marks_only_that_member_unknown_and_still_returns() {
    // C# kinds: io, unauthorized, argument, notsupported.
    for kind in [
        io::ErrorKind::Other,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput,
        io::ErrorKind::Unsupported,
    ] {
        let fx = Fixture::new();
        add_pair(&fx);
        let failed = Arc::new(AtomicBool::new(false));
        let flag = failed.clone();
        fx.bin.set_send_hook(Some(Arc::new(move |path: &Path| {
            if !path.to_string_lossy().eq_ignore_ascii_case(RAW) {
                return None;
            }
            flag.store(true, Ordering::SeqCst);
            Some(io::Error::other("boom"))
        })));
        // The inspection after the failure cannot stat the RAW.
        fx.hooks.set_stat_hook(Some(Arc::new(move |path: &Path| {
            if failed.load(Ordering::SeqCst) && path.to_string_lossy().eq_ignore_ascii_case(RAW) {
                Some(io::Error::new(kind, "inspection failed"))
            } else {
                None
            }
        })));
        let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
        assert!(!result.succeeded, "{kind:?}");
        assert_eq!(result.error.as_deref(), Some("boom"), "{kind:?}");
        let jpeg = result
            .members
            .iter()
            .find(|m| m.member.source == JPEG)
            .expect("JPEG member");
        assert!(jpeg.state_known, "{kind:?}");
        assert!(jpeg.completed, "{kind:?}");
        let raw = result
            .members
            .iter()
            .find(|m| m.member.source == RAW)
            .expect("RAW member");
        assert!(!raw.state_known, "{kind:?}");
        assert!(raw.conflict, "{kind:?}");
        assert!(!raw.completed, "{kind:?}");
        assert_eq!(raw.error.as_deref(), Some("inspection failed"), "{kind:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroupActionRollbackTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveSecondMemberFails_RollsFirstMemberBackAndJournalsFailure
#[test]
fn execute_group_move_second_member_fails_rolls_first_member_back_and_journals_failure() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.CR3") {
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        fx.file_exists(JPEG) && fx.file_exists(RAW),
        "the first member is back at its source"
    );
    assert_eq!(last_state(&fx), Some(JournalState::Dismissed));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveFailsWithShorterFileAtDestination_FileIsNeverDeletedAndStaysForRecovery
#[test]
fn execute_group_move_fails_with_shorter_file_at_destination_file_is_never_deleted_and_stays_for_recovery() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_move_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            if source.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.add_file_at(destination, b"xy", stamp(LAST_WRITE_TICKS));
                return Some(io::Error::other("boom"));
            }
            None
        })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.CR3"),
        "a shorter file at the destination is never deleted"
    );
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveFailsWithForeignLongerFileAtDestination_ForeignFileIsNeverDeleted
#[test]
fn execute_group_move_fails_with_foreign_longer_file_at_destination_foreign_file_is_never_deleted() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_move_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            if source.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.add_file_at(destination, &[b'f'; 99], last_write_plus(500));
                return Some(io::Error::other("boom"));
            }
            None
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert_eq!(fx.content(r"C:\photos\sel\IMG_0001.CR3").map(|c| c.len()), Some(99));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_ForeignFileAppearsBeforeMoveAndMoveReportsDestinationExists_ForeignFileSurvivesAndIsConflict
#[test]
fn execute_group_foreign_file_appears_before_move_and_move_reports_destination_exists_foreign_file_survives_and_is_conflict()
 {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_move_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            if source.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.add_file_at(destination, b"foreign", stamp(LAST_WRITE_TICKS));
                return Some(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            None
        })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        fx.content(r"C:\photos\sel\IMG_0001.CR3").as_deref(),
        Some(&b"foreign"[..])
    );
    let raw = result
        .members
        .iter()
        .find(|m| m.member.source == RAW)
        .expect("RAW member");
    assert!(raw.conflict);
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveFailsPartialDestinationButSourceChanged_PartialIsLeftAlone
#[test]
fn execute_group_move_fails_partial_destination_but_source_changed_partial_is_left_alone() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_move_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            if source.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.add_file_at(source, &[0u8; 61], stamp(LAST_WRITE_TICKS));
                disk.add_file_at(destination, b"partial", stamp(LAST_WRITE_TICKS));
                return Some(io::Error::other("boom"));
            }
            None
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.CR3"),
        "the partial is left alone"
    );
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveFailsMidway_FatDestination_CompensationRestoresMovedMembers
// Golden `group-move-rollback-on-fat-rounded-stamp`: the FAT-rounded stamp of the moved file still matches (+-2 s).
#[test]
fn execute_group_move_fails_midway_fat_destination_compensation_restores_moved_members() {
    let fx = Fixture::new();
    add_triple(&fx);
    // FAT rounds the destination stamp down to an even second (golden `move_round_2s`).
    fx.hooks
        .set_stamp_on_move(Some(Arc::new(|_: &Path, stamp_utc: DateTimeUtc| {
            let even = 2 * TICKS_PER_SECOND;
            stamp(stamp_utc.ticks() - stamp_utc.ticks() % even)
        })));
    fx.hooks.set_move_hook(Some(Arc::new(|_: &Path, destination: &Path| {
        if destination.to_string_lossy().ends_with("IMG_0001.xmp") {
            return Some(io::Error::other("boom"));
        }
        None
    })));
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW) && fx.file_exists(XMP));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveFailsAfterFirstMember_RollbackDoesNotUseTheForwardMoveOverride
#[test]
fn execute_group_move_fails_after_first_member_rollback_does_not_use_the_forward_move_override() {
    let override_calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let calls = override_calls.clone();
    let fx = Fixture::with_move_override(move |disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            calls.lock().expect("calls").push(source.to_string_lossy().into_owned());
            if source.to_string_lossy().eq_ignore_ascii_case(RAW) {
                return Err(io::Error::other("simulated second-member failure"));
            }
            disk.move_file(source, destination)
        })
    });
    add_pair(&fx);
    let _ = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    // The rollback is a plain file-system move, not a second override call.
    assert_eq!(
        *override_calls.lock().expect("calls"),
        vec![JPEG.to_owned(), RAW.to_owned()]
    );
    assert!(fx.file_exists(JPEG));
    assert_eq!(
        fx.events_of(&["move"]).len(),
        1,
        "the rollback went through FileSystem::move_file"
    );
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveRollbackBlockedByChangedDestination_KeepsMovedFileAndReportsIt
#[test]
fn execute_group_move_rollback_blocked_by_changed_destination_keeps_moved_file_and_reports_it() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks.set_move_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.CR3") {
            disk.add_file_at(
                Path::new(r"C:\photos\sel\IMG_0001.JPG"),
                &[b'z'; 3],
                stamp(LAST_WRITE_TICKS),
            );
            return Some(io::Error::other("boom"));
        }
        None
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"),
        "the changed moved file is kept"
    );
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|e| e.contains("could not be put back"))
    );
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveRollbackIncomplete_JournalsOriginalCodedFailureNotLocalizedRollbackText
#[test]
fn execute_group_move_rollback_incomplete_journals_original_coded_failure_not_localized_rollback_text() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_move_leaves_source(true);
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    let entry = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(
        entry.error_code.as_deref(),
        Some(MOVE_SOURCE_NOT_REMOVED),
        "the original code is journaled"
    );
    assert!(
        entry
            .error
            .as_deref()
            .is_some_and(|e| !e.contains("could not be put back"))
    );
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveRollback_NeverOverwritesFileThatAppearedAtSource
#[test]
fn execute_group_move_rollback_never_overwrites_file_that_appeared_at_source() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks.set_move_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.CR3") {
            disk.add_file_at(Path::new(JPEG), b"new file at the source", stamp(LAST_WRITE_TICKS));
            return Some(io::Error::other("boom"));
        }
        None
    })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert_eq!(fx.content(JPEG).as_deref(), Some(&b"new file at the source"[..]));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveCancelledAfterFirstMember_RollsBack
#[test]
fn execute_group_move_cancelled_after_first_member_rolls_back() {
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    // The override moves, then the user cancels while the first member is done (C# `cts.Cancel()` in the override).
    let fx = Fixture::with_move_override(move |disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            disk.move_file(source, destination)?;
            trigger.cancel();
            Ok(())
        })
    });
    add_pair(&fx);
    let result = fx.run_group_with(&group(JPEG, RAW, None, Move, Some("sel")), &cancel);
    assert!(!result.succeeded);
    assert_eq!(result.error.as_deref(), Some(OPERATION_CANCELED_MESSAGE));
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW), "rolled back");
    assert!(!fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"));
    assert_eq!(result.entry.as_ref().map(|e| e.state), Some(JournalState::Dismissed));
    assert_eq!(last_state(&fx), Some(JournalState::Dismissed));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_MoveFailsButBothCopiesRemain_StaysARetryableFailedItem
#[test]
fn execute_group_move_fails_but_both_copies_remain_stays_a_retryable_failed_item() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.CR3") {
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    fx.hooks.set_move_leaves_source(false);
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(result.members.iter().all(|m| !m.completed));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_CopySecondMemberFails_DeletesOnlyTheCopiesItCreatedAndAllowsRerun
#[test]
fn execute_group_copy_second_member_fails_deletes_only_the_copies_it_created_and_allows_rerun() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.add_file(r"C:\photos\sel\IMG_0001.JPG", 3); // a foreign file: never deleted
    fx.hooks.set_copy_hook(Some(Arc::new(|_: &Path, destination: &Path| {
        if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(JPEG), "the source is kept");
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"),
        "the pre-existing destination file is kept"
    );
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_CopyCancelledAndRollbackLeavesACopy_JournalsTheCancellationCode
#[test]
fn execute_group_copy_cancelled_and_rollback_leaves_a_copy_journals_the_cancellation_code() {
    let fx = Fixture::new();
    add_pair(&fx);
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    // The user cancels while the first member is being copied; the rollback cannot remove that first copy.
    fx.hooks.set_copy_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        if source.to_string_lossy().eq_ignore_ascii_case(JPEG) {
            trigger.cancel();
        }
        None
    })));
    fx.hooks.set_delete_hook(Some(Arc::new(|path: &Path| {
        path.to_string_lossy()
            .eq_ignore_ascii_case(r"C:\photos\sel\IMG_0001.JPG")
            .then(|| io::Error::other("simulated undeletable copy"))
    })));
    let result = fx.run_group_with(&group(JPEG, RAW, None, Copy, Some("sel")), &cancel);
    assert!(!result.succeeded);
    assert!(
        fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"),
        "left behind: it needs Recovery"
    );
    assert_eq!(
        result.error.as_deref(),
        Some(rollback_failed_text(OPERATION_CANCELED_MESSAGE, 1).as_str())
    );
    let failed: Vec<_> = fx
        .journal_entries()
        .into_iter()
        .filter(|e| e.state == JournalState::Failed)
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].error_code.as_deref(), Some(CANCELLED_BY_USER));
    assert_eq!(
        failed[0].error.as_deref(),
        Some(english_code_text(CANCELLED_BY_USER).as_str())
    );
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_CopyCancelledMidWay_RemovesOnlyTheCopiesThisOperationMade
#[test]
fn execute_group_copy_cancelled_mid_way_removes_only_the_copies_this_operation_made() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.add_file(r"C:\photos\sel\other.jpg", 2);
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    fx.hooks.set_copy_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        if source.to_string_lossy().eq_ignore_ascii_case(JPEG) {
            trigger.cancel(); // the user cancels while the first member is being copied
        }
        None
    })));
    let result = fx.run_group_with(&group(JPEG, RAW, None, Copy, Some("sel")), &cancel);
    assert!(!result.succeeded);
    assert!(
        fx.file_exists(r"C:\photos\sel\other.jpg"),
        "a file this operation did not make is kept"
    );
    assert!(
        !fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"),
        "the copy this operation made is removed"
    );
    assert!(!fx.file_exists(r"C:\photos\sel\IMG_0001.CR3"));
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW));
    assert_eq!(result.entry.as_ref().map(|e| e.state), Some(JournalState::Dismissed));
    assert_eq!(last_state(&fx), Some(JournalState::Dismissed));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_CopyLeavesPartialFile_PartialFileIsKeptAndRecoveryShowsConflict
// TODO(W3-07): the RecoveryFileCheck assertion is replaced by the disk state until W3-07 merges.
#[test]
fn execute_group_copy_leaves_partial_file_partial_file_is_kept_and_recovery_shows_conflict() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks
        .set_copy_partial_hook(Some(Arc::new(|_: &Path, destination: &Path| {
            if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
                Some((4, io::Error::other("disk full")))
            } else {
                None
            }
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert_eq!(fx.content(r"C:\photos\sel\IMG_0001.CR3").map(|c| c.len()), Some(4));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_CopyDestinationAppearsBetweenCheckAndCopy_ForeignFileIsNeverDeleted
#[test]
fn execute_group_copy_destination_appears_between_check_and_copy_foreign_file_is_never_deleted() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.add_file_at(destination, b"foreign", stamp(LAST_WRITE_TICKS));
                return Some(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            None
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert_eq!(
        fx.content(r"C:\photos\sel\IMG_0001.CR3").as_deref(),
        Some(&b"foreign"[..])
    );
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_CopyDestinationAppearsAfterPreflight_ForeignFileIsNeverDeleted
#[test]
fn execute_group_copy_destination_appears_after_preflight_foreign_file_is_never_deleted() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.add_file_at(destination, &[b'f'; 80], stamp(LAST_WRITE_TICKS));
                return Some(io::Error::other("boom"));
            }
            None
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert_eq!(fx.content(r"C:\photos\sel\IMG_0001.CR3").map(|c| c.len()), Some(80));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_RecycleBinThatFitsEachMemberButNotTheGroup_IsRefusedBeforeAnyMutation
#[test]
fn execute_group_recycle_bin_that_fits_each_member_but_not_the_group_is_refused_before_any_mutation() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.set_capacity(r"C:\", 70);
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(!result.succeeded);
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW));
    assert!(fx.bin.recycled().is_empty());
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_RecycleBinThatFitsTheWholeGroup_Succeeds
#[test]
fn execute_group_recycle_bin_that_fits_the_whole_group_succeeds() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.set_capacity(r"C:\", 100);
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(result.succeeded);
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_PartnerVanishedExternally_ActsOnRemainingMembersAndReportsTheMissingOne
#[test]
fn execute_group_partner_vanished_externally_acts_on_remaining_members_and_reports_the_missing_one() {
    let fx = Fixture::new();
    add_triple(&fx);
    fx.disk.delete(Path::new(XMP)).expect("the sidecar vanished");
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(result.succeeded);
    assert_eq!(result.skipped_missing, Some(vec![XMP.to_owned()]));
    assert!(fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"));
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_OnlySidecarLeft_IsRefusedAsSourceMissing
#[test]
fn execute_group_only_sidecar_left_is_refused_as_source_missing() {
    let fx = Fixture::new();
    fx.add_file(XMP, 5);
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_texts().core_file_action_source_missing(JPEG).as_str())
    );
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroupActionGapTests.cs and RecycleBinCapacityPermanentMemberTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: CaptureGroupActionGapTests.ExecuteGroupAsync_RecycleBinFitsEachVolumeButNotTheSumAcrossVolumes_Succeeds
#[test]
fn execute_group_recycle_bin_fits_each_volume_but_not_the_sum_across_volumes_succeeds() {
    let fx = Fixture::new();
    fx.add_file(JPEG, 40);
    fx.add_file(r"D:\photos\IMG_0001.CR3", 60);
    fx.add_dir(r"D:\");
    fx.bin.set_capacity(r"C:\", 45);
    fx.bin.set_capacity(r"D:\", 65);
    let result = fx.run_group(&group(JPEG, r"D:\photos\IMG_0001.CR3", None, Recycle, None));
    assert!(result.succeeded, "each volume fits its own member");
}

// C#: CaptureGroupActionGapTests.ExecuteGroupAsync_RecycleBinOverflowsOnTheSecondVolume_IsRefusedBeforeAnyMutation
#[test]
fn execute_group_recycle_bin_overflows_on_the_second_volume_is_refused_before_any_mutation() {
    let fx = Fixture::new();
    fx.add_file(JPEG, 40);
    fx.add_file(r"D:\photos\IMG_0001.CR3", 60);
    fx.bin.set_capacity(r"C:\", 45);
    fx.bin.set_capacity(r"D:\", 10);
    let result = fx.run_group(&group(JPEG, r"D:\photos\IMG_0001.CR3", None, Recycle, None));
    assert!(!result.succeeded);
    assert!(fx.bin.recycled().is_empty());
}

// C#: CaptureGroupActionGapTests.ExecuteGroupAsync_RecycleBinOverflowsOnTheFirstVolumeOnly_IsRefusedBeforeAnyMutation
#[test]
fn execute_group_recycle_bin_overflows_on_the_first_volume_only_is_refused_before_any_mutation() {
    let fx = Fixture::new();
    fx.add_file(JPEG, 40);
    fx.add_file(r"D:\photos\IMG_0001.CR3", 60);
    fx.bin.set_capacity(r"C:\", 10);
    fx.bin.set_capacity(r"D:\", 100);
    let result = fx.run_group(&group(JPEG, r"D:\photos\IMG_0001.CR3", None, Recycle, None));
    assert!(!result.succeeded);
    assert!(fx.bin.recycled().is_empty());
}

// RemoveCreatedCopies, in-flight branch: the first member's own copy (proof raised, observed right after the copy) fails
// verification and then cannot be deleted: it is counted as stuck (rollback note with 1) and the entry stays Failed.
// Rust-only mutant killer for the in-flight `stuck += 1`.
#[test]
fn execute_group_copy_in_flight_own_copy_that_cannot_be_deleted_counts_as_stuck() {
    const MOVED_JPEG: &str = r"C:\photos\sel\IMG_0001.JPG";
    let fx = Fixture::new();
    add_pair(&fx);
    // The copied JPEG looks 10 bytes longer than the source: verification fails with VerifySizeChanged.
    fx.faulty.set_stat_filter(Some(Arc::new(|path: &Path, stat| match stat {
        Some(stat) if path.to_string_lossy().eq_ignore_ascii_case(MOVED_JPEG) => {
            Some(pr_core::abstractions::FileStat {
                length: stat.length + 10,
                ..stat
            })
        }
        other => other,
    })));
    fx.hooks.set_delete_hook(Some(Arc::new(|path: &Path| {
        path.to_string_lossy()
            .eq_ignore_ascii_case(MOVED_JPEG)
            .then(|| io::Error::other("in use"))
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(MOVED_JPEG), "the undeletable own copy stays");
    assert_eq!(
        result.error.as_deref(),
        Some(rollback_failed_text(&english_code_text(VERIFY_SIZE_CHANGED), 1).as_str())
    );
    assert_eq!(last_state(&fx), Some(JournalState::Failed));
}

// InspectGroupMember, Recycle: the failed member is a conflict when its source changed in length OR in write time
// (each alone is enough; same file => no conflict). Rust-only mutant killer for the `||`.
#[test]
fn execute_group_recycle_failed_member_is_conflict_when_length_or_stamp_alone_changed() {
    for (bytes, stamp_offset, conflict) in [(61usize, 0i64, true), (60, 1, true), (60, 0, false)] {
        let fx = Fixture::new();
        add_pair(&fx);
        let disk = fx.disk.clone();
        fx.bin.set_send_hook(Some(Arc::new(move |path: &Path| {
            if !path.to_string_lossy().eq_ignore_ascii_case(RAW) {
                return None;
            }
            disk.add_file_at(
                Path::new(RAW),
                &common_file_actions::pattern_bytes(bytes),
                last_write_plus(stamp_offset),
            );
            Some(io::Error::other("boom"))
        })));
        let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
        assert!(!result.succeeded);
        let raw = result
            .members
            .iter()
            .find(|m| m.member.source == RAW)
            .expect("RAW member");
        assert_eq!(raw.conflict, conflict, "{bytes} bytes, +{stamp_offset} s");
        assert!(!raw.completed && raw.source_exists && raw.state_known);
    }
}

// G2 (C# `string.IsNullOrWhiteSpace(request.Destination)`): a whitespace-only destination is "no destination", refused
// before anything is read or journaled. Rust-only mutant killer (golden covers the single path).
#[test]
fn execute_group_whitespace_destination_is_no_destination() {
    for op in [Move, Copy] {
        let fx = Fixture::new();
        add_pair(&fx);
        let result = fx.run_group(&group(JPEG, RAW, None, op, Some(" \t ")));
        assert!(!result.succeeded, "{op:?}");
        assert_eq!(
            result.error.as_deref(),
            Some(english_texts().core_file_action_no_destination().as_str()),
            "{op:?}"
        );
        assert!(fx.journal_lines().is_empty(), "{op:?}");
    }
}

// C#: CaptureGroupActionGapTests.ExecuteGroupAsync_MoveWithTheSamePathListedTwice_FailsAndLeavesTheFileInPlace
#[test]
fn execute_group_move_with_the_same_path_listed_twice_fails_and_leaves_the_file_in_place() {
    let fx = Fixture::new();
    fx.add_file(JPEG, 40);
    let result = fx.run_group(&group(JPEG, JPEG, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(JPEG));
}

// C#: RecycleBinCapacityPermanentMemberTests.ExecuteGroupAsync_PermanentMemberLargerThanTheBin_DoesNotCountAgainstTheBinCapacity
#[test]
fn execute_group_permanent_member_larger_than_the_bin_does_not_count_against_the_bin_capacity() {
    let fx = Fixture::new();
    add_pair(&fx);
    // Only the RAW has no bin (C# `MixedBin.CanRecycle(path) => path != Raw`): the RAW path itself is the "root".
    fx.bin.set_no_bin_roots(&[RAW]);
    fx.bin.set_capacity(r"C:\", 45); // fits the 40-byte JPEG; 40 + 60 would not fit
    let mut request = group(JPEG, RAW, None, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run_group(&request);
    assert!(
        result.succeeded,
        "RAW (60 bytes) is permanent and is not counted against the 45-byte bin: {:?}",
        result.error
    );
    assert_eq!(fx.bin.recycled(), vec![JPEG.to_owned()]);
    assert_eq!(fx.bin.permanently_deleted(), vec![RAW.to_owned()]);
    assert_eq!(
        fx.bin.fits_calls(),
        vec![JPEG.to_owned()],
        "only the bin-bound members are measured"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroupActionServiceTests.cs (the three tests this unit owns)
// ---------------------------------------------------------------------------------------------------------------

// C#: CaptureGroupActionServiceTests.ExecuteGroupAsync_PreflightsEverySourceBeforeJournalOrMutation
#[test]
fn execute_group_preflights_every_source_before_journal_or_mutation() {
    let fx = Fixture::new();
    add_triple(&fx);
    fx.add_dir(SEL);
    fx.add_file(r"C:\photos\sel\IMG_0001.CR3", 9); // the second member's destination exists: found in the preflight
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(result.members.is_empty());
    assert!(fx.journal_lines().is_empty(), "nothing is journaled");
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW) && fx.file_exists(XMP));
    assert!(!fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"));
    assert!(fx.events_of(&["move", "copy"]).is_empty());
}

// C#: CaptureGroupActionServiceTests.ExecuteGroupAsync_PreparedJournalAppendFails_DoesNotMutateAnyMember
#[test]
fn execute_group_prepared_journal_append_fails_does_not_mutate_any_member() {
    let fx = Fixture::new();
    add_triple(&fx);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW) && fx.file_exists(XMP));
    assert!(fx.events_of(&["move"]).is_empty());
}

// C#: CaptureGroupActionServiceTests.ExecuteGroupAsync_PreparesManifestBeforeMovingAndRollsBackFirstMemberOnFailure
#[test]
fn execute_group_prepares_manifest_before_moving_and_rolls_back_first_member_on_failure() {
    let fx = Fixture::new();
    add_triple(&fx);
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.xmp") {
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(!result.succeeded);
    let first = fx
        .journal_entries()
        .into_iter()
        .next()
        .expect("the Prepared manifest line");
    assert_eq!(first.state, JournalState::Prepared);
    assert_eq!(first.group_members.as_ref().map(Vec::len), Some(3));
    assert!(fx.file_exists(JPEG), "the first member was rolled back");
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroupSidecarAndGateTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: CaptureGroupSidecarAndGateTests.ExecuteGroupAsync_MoveWithXmpSidecar_MovesAllThreeAndJournalsThreeMembers
#[test]
fn execute_group_move_with_xmp_sidecar_moves_all_three_and_journals_three_members() {
    let fx = Fixture::new();
    add_triple(&fx);
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(result.succeeded);
    assert_eq!(result.members.len(), 3);
    let last = fx.journal_entries().pop().expect("a Committed manifest line");
    assert_eq!(last.group_members.as_ref().map(Vec::len), Some(3));
    assert!(fx.file_exists(r"C:\photos\sel\IMG_0001.xmp"));
}

// C#: CaptureGroupSidecarAndGateTests.ExecuteGroupAsync_CopyWithXmpSidecar_CopiesAllThreeAndKeepsTheSources
#[test]
fn execute_group_copy_with_xmp_sidecar_copies_all_three_and_keeps_the_sources() {
    let fx = Fixture::new();
    add_triple(&fx);
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Copy, Some("sel")));
    assert!(result.succeeded);
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW) && fx.file_exists(XMP));
    assert!(fx.file_exists(r"C:\photos\sel\IMG_0001.xmp"));
}

// C#: CaptureGroupSidecarAndGateTests.ExecuteGroupAsync_RecycleCancelledAfterFirstMember_LeavesAFailedEntryWithTheTrueMemberStates
#[test]
fn execute_group_recycle_cancelled_after_first_member_leaves_a_failed_entry_with_the_true_member_states() {
    let fx = Fixture::new();
    add_triple(&fx);
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    fx.bin.set_send_hook(Some(Arc::new(move |path: &Path| {
        if path.to_string_lossy().ends_with("IMG_0001.JPG") {
            trigger.cancel();
        }
        None
    })));
    let result = fx.run_group_with(&group(JPEG, RAW, Some(XMP), Recycle, None), &cancel);
    assert!(!result.succeeded);
    assert_eq!(
        last_state(&fx),
        Some(JournalState::Failed),
        "recycle is not compensated: Failed, not Dismissed"
    );
    assert_eq!(
        result.members.iter().filter(|m| m.completed).count(),
        1,
        "the JPEG really went to the bin"
    );
}

// C#: CaptureGroupSidecarAndGateTests.ExecuteGroupAsync_SecondCallWhileFirstRuns_IsRejectedWithoutJournalLineAndGateIsReleasedOnce
// Two threads: the first call blocks inside the move override until the second call has been refused.
#[test]
fn execute_group_second_call_while_first_runs_is_rejected_without_journal_line_and_gate_is_released_once() {
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let entered_tx = Mutex::new(entered_tx);
    let release_rx = Mutex::new(release_rx);
    let first_move = AtomicBool::new(true);
    let fx = Fixture::with_move_override(move |disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            // Only the first member's move blocks; the second member of the same call runs through.
            if first_move.swap(false, Ordering::SeqCst) {
                entered_tx.lock().expect("sender").send(()).expect("test is listening");
                release_rx
                    .lock()
                    .expect("receiver")
                    .recv_timeout(HAND_OFF)
                    .expect("test releases the move");
            }
            disk.move_file(source, destination)
        })
    });
    add_pair(&fx);
    let request = group(JPEG, RAW, None, Move, Some("sel"));
    let second_request = request.clone();
    let (first, second) = std::thread::scope(|scope| {
        let handle = scope.spawn(|| fx.run_group(&request));
        entered_rx
            .recv_timeout(HAND_OFF)
            .expect("the first call entered the move (a panic before the move fails the test)");
        let second = fx.run_group(&second_request);
        release_tx.send(()).expect("first call is waiting");
        (handle.join().expect("first call finished"), second)
    });
    assert!(second.rejected, "the second call is refused while the first runs");
    assert!(first.succeeded);
    assert_eq!(fx.journal_entries().len(), 2, "only the first call wrote journal lines");
    assert!(!fx.service.is_busy(), "the gate is released once");
}

// C#: CaptureGroupSidecarAndGateTests.UndoLastAsync_AfterGroupMoveWithXmp_RestoresAllThree
// TODO(W3-05): needs the real UndoService (W3-05). Red until that unit merges; the group move side is what this asserts.
#[test]
fn undo_last_async_after_group_move_with_xmp_restores_all_three() {
    let fx = Fixture::new();
    add_triple(&fx);
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(result.succeeded);
    let mut undo = UndoService::new(
        fx.journal.clone(),
        fx.hooks.clone(),
        fx.bin.clone(),
        Arc::new(pr_core::file_actions::undo::EnglishUndoTexts),
    );
    undo.register_group(&result);
    let outcome = undo.undo_last();
    assert!(outcome.succeeded);
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW) && fx.file_exists(XMP));
}

// C#: CaptureGroupSidecarAndGateTests.UndoLastAsync_WhileAnotherActionRuns_IsRejectedAndKeepsTheUndoAction
// TODO(W3-05): needs the real UndoService. The gate is shared: while the service is busy, undo is refused.
#[test]
fn undo_last_async_while_another_action_runs_is_rejected_and_keeps_the_undo_action() {
    let fx = Fixture::new();
    add_triple(&fx);
    let result = fx.run_group(&group(JPEG, RAW, Some(XMP), Move, Some("sel")));
    assert!(result.succeeded);
    let shared: Arc<FileActionService> = Arc::new(fx.service);
    let mut undo = UndoService::new(
        fx.journal.clone(),
        fx.hooks.clone(),
        fx.bin.clone(),
        Arc::new(pr_core::file_actions::undo::EnglishUndoTexts),
    )
    .with_gate(shared.clone());
    undo.register_group(&result);
    assert!(shared.try_begin());
    let outcome = undo.undo_last();
    shared.end();
    assert!(outcome.rejected);
    assert!(undo.has_last_undo_action(), "the undo action is kept");
}

// ---------------------------------------------------------------------------------------------------------------
// CopyForeignDestinationTests.cs (group part), CopyRollbackIdentityTests.cs (group part)
// ---------------------------------------------------------------------------------------------------------------

// C#: CopyForeignDestinationTests.GroupCopy_SourceVanishesAndForeignDestinationArrives_ForeignFileSurvives
#[test]
fn group_copy_source_vanishes_and_foreign_destination_arrives_foreign_file_survives() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.delete(source).expect("source vanishes");
                disk.add_file_at(destination, b"foreign", stamp(LAST_WRITE_TICKS));
                return Some(io::Error::other("copy failed"));
            }
            None
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert_eq!(
        fx.content(r"C:\photos\sel\IMG_0001.CR3").as_deref(),
        Some(&b"foreign"[..])
    );
}

// C#: CopyRollbackIdentityTests.GroupCopy_SameSizeForeignFileReplacesCompletedCopy_ForeignFileSurvives
#[test]
fn group_copy_same_size_foreign_file_replaces_completed_copy_foreign_file_survives() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
                // Replace the completed JPEG copy with a foreign file of the SAME size.
                disk.add_file_at(
                    Path::new(r"C:\photos\sel\IMG_0001.JPG"),
                    &[b'x'; 40],
                    last_write_plus(7),
                );
                return Some(io::Error::other("copy failed"));
            }
            None
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert_eq!(
        fx.content(r"C:\photos\sel\IMG_0001.JPG").map(|c| c.len()),
        Some(40),
        "the foreign same-size file is never deleted"
    );
}

// C#: CopyRollbackIdentityTests.GroupCopy_NothingReplaced_RollbackRemovesItsOwnCopies
#[test]
fn group_copy_nothing_replaced_rollback_removes_its_own_copies() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_copy_hook(Some(Arc::new(|_: &Path, destination: &Path| {
        if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
            Some(io::Error::other("copy failed"))
        } else {
            None
        }
    })));
    let result = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        !fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"),
        "our own completed copy is removed"
    );
}

// C#: CopyRollbackIdentityTests.GroupCopy_ForeignFileReplacesInFlightMemberBeforeRollback_ForeignFileSurvives
#[test]
fn group_copy_foreign_file_replaces_in_flight_member_before_rollback_foreign_file_survives() {
    let fx = Fixture::new();
    add_pair(&fx);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            if destination.to_string_lossy().ends_with("IMG_0001.CR3") {
                disk.add_file_at(destination, b"foreign", stamp(LAST_WRITE_TICKS + 50_000_000));
                return Some(io::Error::other("copy failed"));
            }
            None
        })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Copy, Some("sel")));
    assert_eq!(
        fx.content(r"C:\photos\sel\IMG_0001.CR3").as_deref(),
        Some(&b"foreign"[..])
    );
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionDestinationFolderCleanupTests.cs (group part)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionDestinationFolderCleanupTests.Group_JournalFailsAtBegin_DestinationFolderCreatedByThisCall_IsRemoved
#[test]
fn group_journal_fails_at_begin_destination_folder_created_by_this_call_is_removed() {
    let fx = Fixture::new();
    add_pair(&fx);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(!fx.dir_exists(SEL));
}

// C#: FileActionDestinationFolderCleanupTests.Group_JournalFailsAtBegin_MissingParentsCreatedByThisCall_AreRemovedToo
#[test]
fn group_journal_fails_at_begin_missing_parents_created_by_this_call_are_removed_too() {
    let fx = Fixture::new();
    add_pair(&fx);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some(r"sel\deeper")));
    assert!(!result.succeeded);
    assert!(!fx.dir_exists(r"C:\photos\sel\deeper"));
    assert!(!fx.dir_exists(SEL), "the parent this call created is removed too");
}

// C#: FileActionDestinationFolderCleanupTests.Group_JournalFailsAtBegin_PreExistingEmptyDestinationFolder_IsKept
#[test]
fn group_journal_fails_at_begin_pre_existing_empty_destination_folder_is_kept() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.add_dir(SEL);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let _ = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(fx.dir_exists(SEL));
}

// C#: FileActionDestinationFolderCleanupTests.Group_JournalFailsAtBegin_PreExistingDestinationFolderWithFiles_IsKeptWithItsFiles
#[test]
fn group_journal_fails_at_begin_pre_existing_destination_folder_with_files_is_kept_with_its_files() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.add_file(r"C:\photos\sel\keep.jpg", 3);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let _ = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(fx.file_exists(r"C:\photos\sel\keep.jpg"));
}

// C#: FileActionDestinationFolderCleanupTests.Group_FailureAfterPrepared_KeepsTheDestinationFolderLikeBefore
#[test]
fn group_failure_after_prepared_keeps_the_destination_folder_like_before() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks.set_move_hook(Some(Arc::new(|source: &Path, _: &Path| {
        if source.to_string_lossy().ends_with("IMG_0001.CR3") {
            Some(io::Error::other("boom"))
        } else {
            None
        }
    })));
    let _ = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(fx.dir_exists(SEL));
}

// C#: FileActionDestinationFolderCleanupTests.Group_JournalFailsAtBegin_CleanupThatThrows_DoesNotHideTheOriginalFailure
#[test]
fn group_journal_fails_at_begin_cleanup_that_throws_does_not_hide_the_original_failure() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks
        .set_delete_directory_hook(Some(Arc::new(|_: &Path| Some(io::Error::other("cleanup failed")))));
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert_eq!(result.error.as_deref(), Some("journal unavailable"));
}

// C#: FileActionMutationAuditBTests.ExecuteGroupAsync_Recycle_ShellReportsSuccessButFileIsStillThere_FailsAndIsNotCommitted
#[test]
fn execute_group_recycle_shell_reports_success_but_file_is_still_there_fails_and_is_not_committed() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.leave_on_send(RAW);
    let result = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_code_text(SOURCE_STILL_EXISTS_AFTER_RECOVERY).as_str())
    );
    assert!(fx.journal_entries().iter().all(|e| e.state != JournalState::Committed));
}

// C#: FileActionFaultInjectionGapTests.CancelAfterPrepared_GroupMove_IsObservedAndRolledBack
#[test]
fn cancel_after_prepared_group_move_is_observed_and_rolled_back() {
    let fx = Fixture::new();
    add_pair(&fx);
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    // Cancelled right after the Prepared line (C# `FaultKind.CancelAfter` on the first journal append).
    let journal = journal_path();
    fx.hooks.set_open_append_hook(Some(Arc::new(move |path: &Path| {
        if path.to_string_lossy().eq_ignore_ascii_case(&journal) {
            trigger.cancel();
        }
        None
    })));
    let result = fx.run_group_with(&group(JPEG, RAW, None, Move, Some("sel")), &cancel);
    assert!(cancel.is_canceled());
    assert!(!result.succeeded);
    assert_eq!(result.error.as_deref(), Some(OPERATION_CANCELED_MESSAGE));
    assert!(fx.events_of(&["move"]).is_empty(), "nothing moved");
    assert!(fx.file_exists(JPEG) && fx.file_exists(RAW));
    assert!(!fx.file_exists(r"C:\photos\sel\IMG_0001.JPG"));
    assert!(
        !matches!(last_state(&fx), Some(JournalState::Prepared | JournalState::Committed)),
        "neither Prepared nor Committed is the latest state"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// JournalConcurrencyTests.cs (ManySimultaneousActions_ExactlyOneRuns), JournalDurabilityTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalConcurrencyTests.ManySimultaneousActions_ExactlyOneRuns
// 8 threads start together (Barrier); the one move that gets the gate blocks until the 7 others were rejected.
#[test]
fn many_simultaneous_actions_exactly_one_runs() {
    const THREADS: usize = 8;
    let runs = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let (rejected_tx, rejected_rx) = mpsc::channel::<()>();
    let rejected_rx = Mutex::new(rejected_rx);
    let counter = runs.clone();
    let fx = Fixture::with_move_override(move |disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            // Counts winning ACTIONS: only the first member's move of the winner waits for the losers.
            if counter.fetch_add(1, Ordering::SeqCst) > 0 {
                return disk.move_file(source, destination);
            }
            for _ in 0..(THREADS - 1) {
                rejected_rx
                    .lock()
                    .expect("receiver")
                    .recv_timeout(HAND_OFF)
                    .expect("every loser is refused while the winner runs");
            }
            disk.move_file(source, destination)
        })
    });
    add_pair(&fx);
    let barrier = Barrier::new(THREADS);
    let results: Vec<CaptureGroupActionResult> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let rejected_tx = rejected_tx.clone();
                let rejected = rejected.clone();
                let barrier = &barrier;
                let fx = &fx;
                scope.spawn(move || {
                    barrier.wait();
                    let result = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
                    if result.rejected {
                        rejected.fetch_add(1, Ordering::SeqCst);
                        rejected_tx.send(()).expect("the winner listens");
                    }
                    result
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("thread finished"))
            .collect()
    });
    assert_eq!(
        runs.load(Ordering::SeqCst),
        2,
        "exactly one action ran: the two moves of its JPEG + RAW"
    );
    assert_eq!(
        results.iter().filter(|r| r.succeeded).count(),
        1,
        "exactly one action ran"
    );
    assert_eq!(rejected.load(Ordering::SeqCst), THREADS - 1);
    assert_eq!(results.iter().filter(|r| r.rejected).count(), THREADS - 1);
}

// C#: JournalDurabilityTests.Move_OpensStreamPerMode_AndOrdersPreparedMutationCommitted
// The durable flag of each append and the order of the move among them (HookFileSystem events).
#[test]
fn move_opens_stream_per_mode_and_orders_prepared_mutation_committed() {
    let options = OperationJournalOptions {
        durability: Some(Arc::new(|| JournalDurability::PowerLossSafe)),
        ..OperationJournalOptions::default()
    };
    let fx = Fixture::with_journal_options(options);
    fx.add_file(JPEG, 40);
    let result = fx.run(&common_file_actions::single(JPEG, Move, Some("sel")));
    assert!(result.succeeded);
    let events = fx.hooks.events();
    let kinds: Vec<(&str, bool)> = events.iter().map(|e| (e.kind, e.durable)).collect();
    assert_eq!(
        kinds,
        vec![("append", true), ("move", false), ("append", true)],
        "append(durable) < move < append(durable)"
    );
}

// C#: JournalDurabilityTests.Recycle_FastMode_WritesNonDurableRecords
#[test]
fn recycle_fast_mode_writes_non_durable_records() {
    let options = OperationJournalOptions {
        durability: Some(Arc::new(|| JournalDurability::Fast)),
        ..OperationJournalOptions::default()
    };
    let fx = Fixture::with_journal_options(options);
    fx.add_file(JPEG, 40);
    let _ = fx.run(&common_file_actions::single(JPEG, Recycle, None));
    assert!(
        fx.hooks
            .events()
            .iter()
            .filter(|e| e.kind == "append")
            .all(|e| !e.durable)
    );
}

// C#: JournalDurabilityTests.ModeSwitch_AppliesToNextWrite_WithoutRecreatingJournal
#[test]
fn mode_switch_applies_to_next_write_without_recreating_journal() {
    let fast = Arc::new(AtomicBool::new(true));
    let switch = fast.clone();
    let options = OperationJournalOptions {
        durability: Some(Arc::new(move || {
            if switch.load(Ordering::SeqCst) {
                JournalDurability::Fast
            } else {
                JournalDurability::PowerLossSafe
            }
        })),
        ..OperationJournalOptions::default()
    };
    let fx = Fixture::with_journal_options(options);
    fx.add_file(JPEG, 40);
    fx.add_file(RAW, 60);
    let _ = fx.run(&common_file_actions::single(JPEG, Move, Some("sel")));
    fast.store(false, Ordering::SeqCst);
    let _ = fx.run(&common_file_actions::single(RAW, Move, Some("sel")));
    let durable: Vec<bool> = fx
        .hooks
        .events()
        .iter()
        .filter(|e| e.kind == "append")
        .map(|e| e.durable)
        .collect();
    assert_eq!(durable.first(), Some(&false));
    assert_eq!(durable.last(), Some(&true), "the next write uses the new mode");
    assert!(fx.file_exists(&journal_path()), "the journal was not recreated");
}

// C#: JournalDurabilityTests.SafeMode_PreparedAndMutationRunOffCallerThread_FastModeKeepsPreparedInline
// DIVERGENCE: no thread hop. Rust runs the whole action on the caller's thread (docs/adr/R0003); only the ORDER part
// (append(durable) < move < append(durable)) is ported. The thread-identity half has no Rust twin.
#[test]
fn safe_mode_prepared_and_mutation_order_only_no_thread_hop() {
    let options = OperationJournalOptions {
        durability: Some(Arc::new(|| JournalDurability::PowerLossSafe)),
        ..OperationJournalOptions::default()
    };
    let fx = Fixture::with_journal_options(options);
    fx.add_file(JPEG, 40);
    let _ = fx.run(&common_file_actions::single(JPEG, Move, Some("sel")));
    let order: Vec<&str> = fx.hooks.events().iter().map(|e| e.kind).collect();
    assert_eq!(order, vec!["append", "move", "append"]);
}

// Extra: the gate is released after a refusal that happens after TryBegin took it (INV-4, trap T1).
#[test]
fn group_gate_is_released_after_every_path_including_refusal() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    // Refused in the preflight (no opt-in for a permanent delete), after TryBegin took the gate.
    let refused = fx.run_group(&group(JPEG, RAW, None, Recycle, None));
    assert!(!refused.succeeded);
    assert!(
        !fx.service.is_busy(),
        "released after a refusal that happened after the gate was taken"
    );
    let ok = fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")));
    assert!(ok.succeeded);
}

// C#: CaptureGroupActionRollbackTests.ExecuteGroupAsync_CompensationThrowsUnexpectedExceptionType_DoesNotEscapeAndLeavesPreparedForReconcile
// Open question 2, option B: the panic is raised inside the compensating move (a file-system panic), not in the forward
// move. It propagates out of the service, the gate is released by the RAII guard, and the Prepared line stays.
#[test]
fn compensation_throws_unexpected_exception_type_does_not_escape_and_leaves_prepared_for_reconcile() {
    let fx = Fixture::new();
    add_pair(&fx);
    fx.hooks
        .set_move_hook(Some(Arc::new(|source: &Path, destination: &Path| {
            // The RAW forward move fails; the compensating move of the JPEG (sel -> source) panics.
            if source.to_string_lossy().ends_with("IMG_0001.CR3") {
                return Some(io::Error::other("boom"));
            }
            if destination.to_string_lossy().eq_ignore_ascii_case(JPEG) {
                panic!("file system panicked during compensation");
            }
            None
        })));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fx.run_group(&group(JPEG, RAW, None, Move, Some("sel")))
    }));
    assert!(
        outcome.is_err(),
        "B: the panic propagates to the caller, nothing is swallowed"
    );
    assert!(fx.journal_entries().iter().all(|e| e.state != JournalState::Committed));
    assert_eq!(
        last_state(&fx),
        Some(JournalState::Prepared),
        "the Prepared line stays for startup reconcile"
    );
}
