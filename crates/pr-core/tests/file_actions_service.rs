//! Tests for `pr_core::file_actions::service` single-file actions (`FileActionService::execute`): Move, Copy and Recycle
//! (F-01..F-05). Port of `PhotoReview.Core.Tests/FileActions/FileActionServiceTests.cs`,
//! `FileActionServiceSingleMutationGapTests.cs`, `FileActionMutationAuditBTests.cs` (single part),
//! `FileActionSafetyEdgeTests.cs` (prepared-append part), `JournalCommitClockFailureTests.cs`, `ActionDestinationRootTests.cs`,
//! `DestinationEscapeTests.cs`, `PermanentDeleteTests.cs` (Recycle part), `FileActionFaultInjectionGapTests.cs` (the four
//! single/permission tests), `LiveOperationMarkerTests.cs` (single part) and `JournalErrorCodeTests.cs`.
//!
//! Baseline d92d6aae. Red until the W3-04 bodies land: every test that reaches `execute`, `try_begin`, `end`,
//! `execute_group` or any other unimplemented method panics with `todo!()`. Each test carries a `// C#:` comment naming the
//! C# test it ports; a Theory is one Rust test that loops over its rows. Nothing here touches the real Recycle Bin: the
//! bin is [`ScenarioBin`] over the in-memory disk.
//!
//! Open items:
//! * `TODO(W3-07)`: four C# tests assert with `RecoveryFileCheck`; until W3-07 merges they assert the disk state instead.
//! * `DIVERGENCE:` lines mark the C# behaviour the Rust port cannot reproduce by construction (see each test).

mod common_file_actions;

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common_file_actions::{
    Fixture, LAST_WRITE_TICKS, canceled_error, english_code_text, english_texts, fail_journal_append_nth, journal_path,
    last_write_plus, outside_text, single, stamp,
};
use pr_core::abstractions::FileSystem;
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors::{
    CANCELLED_BY_USER, MOVE_SOURCE_NOT_REMOVED, SOURCE_STILL_EXISTS_AFTER_RECOVERY, VERIFY_SIZE_CHANGED, catalog_key,
};
use pr_core::file_actions::journal::{EnglishJournalTexts, JournalTexts, OperationJournal, OperationJournalOptions};
use pr_core::file_actions::service::{
    EnglishFileActionTexts, FileActionService, FileActionTexts, OPERATION_CANCELED_MESSAGE, TASK_CANCELED_MESSAGE,
};
use pr_core::file_actions::undo::MoveOverride;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::CancelToken;
use pr_test_support::{FaultInjectionFileSystem, FaultKind, FaultPlan};

use FileOperationType::{Copy, Move, Recycle};

/// Source file of the single-file scenarios (`C:\photos\a.jpg`).
const SOURCE: &str = r"C:\photos\a.jpg";
/// The destination folder of the single-file scenarios (`C:\photos\sel`).
const SEL: &str = r"C:\photos\sel";

/// The journal's states of the lines written so far, in order.
fn states(fx: &Fixture) -> Vec<JournalState> {
    fx.journal_entries().into_iter().map(|e| e.state).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Pre-checks (steps S3.1-S3.10): nothing is created or journaled before them
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionServiceTests.ExecuteAsync_MissingSource_DoesNotCreateDestinationFolder (Theory: Move, Copy)
#[test]
fn execute_async_missing_source_does_not_create_destination_folder() {
    for op in [Move, Copy] {
        let fx = Fixture::new();
        let result = fx.run(&single(r"C:\photos\gone.jpg", op, Some("sel")));
        assert!(!result.succeeded, "{op:?}: a missing source must fail");
        assert!(
            !fx.dir_exists(SEL),
            "{op:?}: the destination folder must not be created"
        );
    }
}

// C#: FileActionServiceTests.ExecuteAsync_RelativeDestinationOutsideSource_FailsWithoutJournalOrMove (Theory: Move, Copy)
#[test]
fn execute_async_relative_destination_outside_source_fails_without_journal_or_move() {
    for op in [Move, Copy] {
        let fx = Fixture::new();
        fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
        let result = fx.run(&single(SOURCE, op, Some(r"..\outside")));
        assert!(!result.succeeded);
        assert_eq!(result.error.as_deref(), Some(outside_text().as_str()));
        assert!(fx.file_exists(SOURCE));
        assert!(!fx.file_exists(r"C:\outside\a.jpg"));
        assert!(
            fx.events_of(&["move", "copy", "append"]).is_empty(),
            "no journal line and no move/copy"
        );
        assert!(!fx.file_exists(&journal_path()));
        assert!(fx.journal.read_pending_and_failed_operations().unwrap().is_empty());
    }
}

// C#: FileActionServiceTests.ExecuteAsync_RelativeDestinationThroughJunctionEscapesFolder_FailsWithoutJournalOrMove (Theory)
// SEC-01: `link` is a junction inside the photo folder whose real target is outside it; the lexical destination
// `link\processed` passes ActionDestinationPolicy.Validate, so only the reparse check rejects it.
#[test]
fn execute_async_relative_destination_through_junction_escapes_folder_fails_without_journal_or_move() {
    for op in [Move, Copy] {
        let fx = Fixture::new();
        fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
        fx.hooks.add_reparse_point(r"C:\photos\link", r"C:\outside");
        let result = fx.run(&single(SOURCE, op, Some(r"link\processed")));
        assert!(!result.succeeded);
        assert_eq!(result.error.as_deref(), Some(outside_text().as_str()));
        assert!(fx.file_exists(SOURCE));
        assert!(!fx.file_exists(r"C:\outside\processed\a.jpg"));
        assert!(fx.events_of(&["move", "copy", "append"]).is_empty());
        assert!(!fx.file_exists(&journal_path()));
    }
}

// C#: FileActionServiceTests.ExecuteAsync_RelativeDestinationThroughJunctionStaysInsideFolder_Succeeds
#[test]
fn execute_async_relative_destination_through_junction_stays_inside_folder_succeeds() {
    let fx = Fixture::new();
    fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
    fx.hooks.add_reparse_point(r"C:\photos\link", r"C:\photos\real");
    let result = fx.run(&single(SOURCE, Move, Some(r"link\processed")));
    assert!(result.succeeded);
    assert!(fx.file_exists(r"C:\photos\real\processed\a.jpg"));
}

// C#: FileActionServiceTests.ExecuteAsync_DriveRelativeDestination_FailsWithoutJournalOrMove (Theory: Move, Copy)
#[test]
fn execute_async_drive_relative_destination_fails_without_journal_or_move() {
    for op in [Move, Copy] {
        let fx = Fixture::new();
        fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
        let result = fx.run(&single(SOURCE, op, Some("D:Backup")));
        assert!(!result.succeeded);
        assert_eq!(result.error.as_deref(), Some(outside_text().as_str()));
        assert!(fx.file_exists(SOURCE));
        assert!(fx.events_of(&["move", "copy", "append"]).is_empty());
        assert!(!fx.file_exists(&journal_path()));
    }
}

// C#: FileActionServiceTests.ExecuteAsync_RootedDestinationWithoutDrive_FailsWithoutJournalOrMove (Theory: Move, Copy)
#[test]
fn execute_async_rooted_destination_without_drive_fails_without_journal_or_move() {
    for op in [Move, Copy] {
        let fx = Fixture::new();
        fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
        let result = fx.run(&single(SOURCE, op, Some(r"\sel")));
        assert!(!result.succeeded);
        assert_eq!(result.error.as_deref(), Some(outside_text().as_str()));
        assert!(fx.file_exists(SOURCE));
        assert!(fx.events_of(&["move", "copy", "append"]).is_empty());
    }
}

// C#: FileActionServiceTests.ExecuteAsync_SameFolder_Fails
#[test]
fn execute_async_same_folder_fails() {
    let fx = Fixture::new();
    fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
    let result = fx.run(&single(SOURCE, Move, Some(".")));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_texts().core_file_action_same_folder().as_str())
    );
    assert!(fx.file_exists(SOURCE));
}

// C#: FileActionServiceTests.ExecuteAsync_DestinationMissing_Fails
#[test]
fn execute_async_destination_missing_fails() {
    let fx = Fixture::new();
    fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
    let result = fx.run(&single(SOURCE, Move, None));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_texts().core_file_action_no_destination().as_str())
    );
    assert!(fx.file_exists(SOURCE));
}

// C#: FileActionServiceTests.ExecuteAsync_DestinationExists_Fails
#[test]
fn execute_async_destination_exists_fails() {
    let fx = Fixture::new();
    fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
    fx.add_file_bytes(r"C:\photos\sel\a.jpg", b"foreign", last_write_plus(0));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(
            english_texts()
                .core_file_action_destination_exists(r"C:\photos\sel\a.jpg")
                .as_str()
        )
    );
    assert!(fx.file_exists(SOURCE));
    assert_eq!(fx.content(r"C:\photos\sel\a.jpg").as_deref(), Some(&b"foreign"[..]));
}

// C#: FileActionServiceTests.ExecuteAsync_SourceNotFound_FailsWithoutPreparedInJournal
#[test]
fn execute_async_source_not_found_fails_without_prepared_in_journal() {
    let fx = Fixture::new();
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.journal_lines().is_empty(), "no Prepared line for a missing source");
    assert!(!fx.dir_exists(SEL));
}

// C#: FileActionServiceTests.ExecuteAsync_RelativeDestinationInsideSource_Succeeds
#[test]
fn execute_async_relative_destination_inside_source_succeeds() {
    let fx = Fixture::new();
    fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
    let result = fx.run(&single(SOURCE, Move, Some(r"sel\deeper")));
    assert!(result.succeeded);
    assert!(fx.file_exists(r"C:\photos\sel\deeper\a.jpg"));
}

// C#: FileActionServiceTests.ExecuteAsync_AbsoluteDestinationOutsideSource_StaysAllowed
#[test]
fn execute_async_absolute_destination_outside_source_stays_allowed() {
    let fx = Fixture::new();
    fx.add_file_bytes(SOURCE, b"hello photo", last_write_plus(0));
    fx.add_dir(r"D:\");
    let result = fx.run(&single(SOURCE, Move, Some(r"D:\out")));
    assert!(result.succeeded);
    assert!(fx.file_exists(r"D:\out\a.jpg"));
}

// ---------------------------------------------------------------------------------------------------------------
// Successful actions (S4, S5)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionServiceTests.ExecuteAsync_Move_Success
#[test]
fn execute_async_move_success() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(result.succeeded);
    assert!(
        !result.source_removed,
        "a succeeded move is not a 'source removed' fact"
    );
    assert_eq!(result.destination_path.as_deref(), Some(r"C:\photos\sel\a.jpg"));
    assert_eq!(result.size, 11);
    assert_eq!(result.last_write_utc, stamp(LAST_WRITE_TICKS));
    assert!(fx.file_exists(r"C:\photos\sel\a.jpg"));
    assert!(!fx.file_exists(SOURCE));
    assert_eq!(states(&fx), vec![JournalState::Prepared, JournalState::Committed]);
}

// C#: FileActionServiceTests.ExecuteAsync_Copy_Success
#[test]
fn execute_async_copy_success() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(result.succeeded);
    assert!(fx.file_exists(SOURCE), "a copy keeps the source");
    assert!(fx.file_exists(r"C:\photos\sel\a.jpg"));
    assert_eq!(states(&fx), vec![JournalState::Prepared, JournalState::Committed]);
}

// C#: FileActionServiceTests.ExecuteAsync_Copy_SourceStays_Succeeds
#[test]
fn execute_async_copy_source_stays_succeeds() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 7);
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(result.succeeded);
    assert!(!result.source_removed);
    assert_eq!(fx.content(SOURCE).map(|c| c.len()), Some(7));
}

// C#: FileActionServiceTests.ExecuteAsync_Recycle_Success
#[test]
fn execute_async_recycle_success() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(result.succeeded);
    assert!(!result.permanently_deleted);
    assert_eq!(result.destination_path, None);
    assert_eq!(fx.bin.recycled(), vec![SOURCE.to_owned()]);
    assert!(!fx.file_exists(SOURCE));
    assert_eq!(states(&fx), vec![JournalState::Prepared, JournalState::Committed]);
}

// ---------------------------------------------------------------------------------------------------------------
// Failure paths (S7) and the source fact (INV-3, F3)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionServiceTests.ExecuteAsync_MoveLeavesSource_FailsWithDistinctCodeAndKeepsBoth
#[test]
fn execute_async_move_leaves_source_fails_with_distinct_code_and_keeps_both() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_move_leaves_source(true);
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_code_text(MOVE_SOURCE_NOT_REMOVED).as_str())
    );
    assert!(fx.file_exists(SOURCE));
    assert!(fx.file_exists(r"C:\photos\sel\a.jpg"));
    assert!(!result.source_removed, "the source is still there");
    let last = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(last.state, JournalState::Failed);
    assert_eq!(last.error_code.as_deref(), Some(MOVE_SOURCE_NOT_REMOVED));
}

// C#: FileActionServiceTests.ExecuteAsync_MoveSizeChangedSourceGone_FailedJournalAndSourceRemoved
// The override moves the file and then shortens the destination: the source is gone, so `source_removed` is true (F3).
#[test]
fn execute_async_move_size_changed_source_gone_failed_journal_and_source_removed() {
    let fx = Fixture::with_move_override(|disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            disk.move_file(source, destination)?;
            disk.add_file_at(destination, b"aaaa", stamp(LAST_WRITE_TICKS));
            Ok(())
        })
    });
    fx.add_file(SOURCE, 11);
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_code_text(VERIFY_SIZE_CHANGED).as_str())
    );
    assert!(result.source_removed);
    assert!(!fx.file_exists(SOURCE));
    let last = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(last.state, JournalState::Failed);
    assert_eq!(last.error_code.as_deref(), Some(VERIFY_SIZE_CHANGED));
}

// C#: FileActionServiceTests.ExecuteAsync_MoveLeavesSource_SourceRemovedFalse
#[test]
fn execute_async_move_leaves_source_source_removed_false() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_move_leaves_source(true);
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.source_removed);
    assert!(fx.file_exists(SOURCE));
}

// C#: FileActionServiceTests.ExecuteAsync_MoveOsFailureSourceIntact_SourceRemovedFalse
#[test]
fn execute_async_move_os_failure_source_intact_source_removed_false() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_move_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some(io::Error::other("disk on fire"))
    })));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(result.error.as_deref(), Some("disk on fire"));
    assert!(!result.source_removed);
    assert!(fx.file_exists(SOURCE));
}

// C#: FileActionServiceTests.ExecuteAsync_MoveSourceMissingUpFront_SourceRemovedFalse
#[test]
fn execute_async_move_source_missing_up_front_source_removed_false() {
    let fx = Fixture::new();
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(!result.source_removed);
}

// C#: FileActionServiceTests.ExecuteAsync_CopyFailure_SourceRemovedFalse
#[test]
fn execute_async_copy_failure_source_removed_false() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_copy_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some(io::Error::other("access denied"))
    })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(!result.source_removed);
    assert!(fx.file_exists(SOURCE));
}

// C#: FileActionServiceTests.ExecuteAsync_RecycleFailure_SourceRemovedFalse
#[test]
fn execute_async_recycle_failure_source_removed_false() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.fail_send(SOURCE, "shell said no");
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(!result.succeeded);
    assert!(!result.source_removed);
    assert!(fx.file_exists(SOURCE));
}

// C#: FileActionServiceTests.ExecuteAsync_IOErrorDuringMove_RecordsFailedInJournal
#[test]
fn execute_async_io_error_during_move_records_failed_in_journal() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_move_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some(io::Error::other("disk on fire"))
    })));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    let last = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(last.state, JournalState::Failed);
    assert_eq!(last.error.as_deref(), Some("disk on fire"));
    assert_eq!(last.error_code, None, "a raw OS failure is journaled without a code");
}

// C#: FileActionServiceTests.ExecuteAsync_MoveCompletedButCommitJournalFails_PreservesCompletedOutcome
// Golden `single-move-commit-append-fails`: append 2 (the Committed line) fails.
#[test]
fn execute_async_move_completed_but_commit_journal_fails_preserves_completed_outcome() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(result.succeeded, "the move completed, so the outcome is success");
    assert!(!result.journal_persisted);
    assert_eq!(result.journal_error.as_deref(), Some("journal unavailable"));
    assert!(fx.file_exists(r"C:\photos\sel\a.jpg"));
}

// C#: FileActionServiceTests.ExecuteAsync_WhenGateBusy_RejectsSecondAction
#[test]
fn execute_async_when_gate_busy_rejects_second_action() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    assert!(fx.service.try_begin());
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(result.rejected);
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_texts().core_file_action_busy().as_str())
    );
    assert!(fx.file_exists(SOURCE), "a rejected request reads and moves nothing");
    assert!(fx.journal_lines().is_empty());
    fx.service.end();
}

// C#: FileActionServiceTests.TryBegin_And_End_ManageBusyGate
#[test]
fn try_begin_and_end_manage_busy_gate() {
    let fx = Fixture::new();
    assert!(!fx.service.is_busy());
    assert!(fx.service.try_begin());
    assert!(fx.service.is_busy());
    assert!(!fx.service.try_begin(), "a second TryBegin while held is refused");
    fx.service.end();
    assert!(!fx.service.is_busy());
    assert!(fx.service.try_begin());
    fx.service.end();
}

// C#: FileActionServiceTests.ExecuteAsync_CancelledBeforeStart_DismissesJournalEntry (Theory: Move, Copy, Recycle)
// The hand-off check fails after Prepared; an untouched source is Dismissed. Golden `single-*-pre-cancelled`.
#[test]
fn execute_async_cancelled_before_start_dismisses_journal_entry() {
    for op in [Move, Copy, Recycle] {
        let fx = Fixture::new();
        fx.add_file(SOURCE, 11);
        let cancel = CancelToken::new();
        cancel.cancel();
        let destination = if op == Recycle { None } else { Some("sel") };
        let result = fx.run_with(&single(SOURCE, op, destination), &cancel);
        assert!(!result.succeeded, "{op:?}");
        assert_eq!(result.error.as_deref(), Some(TASK_CANCELED_MESSAGE), "{op:?}");
        assert!(fx.file_exists(SOURCE), "{op:?}: nothing was moved or deleted");
        assert_eq!(
            states(&fx),
            vec![JournalState::Prepared, JournalState::Dismissed],
            "{op:?}: T12, the cancellation happens after Prepared"
        );
    }
}

// C#: FileActionServiceTests.ExecuteAsync_MoveCancelledAfterMove_KeepsFailedEntry
#[test]
fn execute_async_move_cancelled_after_move_keeps_failed_entry() {
    let fx = Fixture::with_move_override(|disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            disk.move_file(source, destination)?;
            Err(canceled_error())
        })
    });
    fx.add_file(SOURCE, 11);
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(result.error.as_deref(), Some(OPERATION_CANCELED_MESSAGE));
    let last = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(
        last.state,
        JournalState::Failed,
        "the move happened, so the entry is not dismissed"
    );
    assert_eq!(last.error_code.as_deref(), Some(CANCELLED_BY_USER));
}

// C#: FileActionServiceTests.ExecuteAsync_CopyCancelledAfterCompleteCopy_KeepsFailedEntry
#[test]
fn execute_async_copy_cancelled_after_complete_copy_keeps_failed_entry() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            disk.copy_file(source, destination).expect("the copy itself succeeds");
            Some(canceled_error())
        })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    let last = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(last.state, JournalState::Failed);
    assert_eq!(last.error_code.as_deref(), Some(CANCELLED_BY_USER));
}

// C#: FileActionServiceTests.ExecuteAsync_CopyFailsMidway_KeepsUnclaimedPartialDestinationAndRecoveryShowsConflict
// TODO(W3-07): the C# test asserts `RecoveryFileCheck` here; until W3-07 merges only the disk state is asserted.
#[test]
fn execute_async_copy_fails_midway_keeps_unclaimed_partial_destination_and_recovery_shows_conflict() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_copy_partial_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some((4, io::Error::other("disk full")))
    })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(SOURCE));
    assert_eq!(
        fx.content(r"C:\photos\sel\a.jpg").map(|c| c.len()),
        Some(4),
        "the partial file is kept"
    );
}

// C#: FileActionServiceTests.ExecuteAsync_CopyCompletesButSourceShrankMeanwhile_PartialCleanupThrowing_StillJournalsTheFailure
// The "throwing" cleanup is a `delete` that returns Err; the failure is still journaled.
#[test]
fn execute_async_copy_completes_but_source_shrank_meanwhile_partial_cleanup_throwing_still_journals_the_failure() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            disk.copy_file(source, destination).expect("copy");
            disk.add_file_at(source, b"aaaa", stamp(LAST_WRITE_TICKS));
            Some(io::Error::other("copy reported failure"))
        })));
    fx.hooks
        .set_delete_hook(Some(Arc::new(|_: &Path| Some(io::Error::other("cannot delete")))));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    let last = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(last.state, JournalState::Failed);
}

// C#: FileActionServiceTests.ExecuteAsync_CopyFails_DestinationExistedBefore_IsNotDeleted
// A foreign file appears at the destination during the copy (race after the pre-check); it must survive.
#[test]
fn execute_async_copy_fails_destination_existed_before_is_not_deleted() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            disk.add_file_at(destination, b"foreign", stamp(LAST_WRITE_TICKS));
            Some(io::Error::from(io::ErrorKind::AlreadyExists))
        })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(fx.content(r"C:\photos\sel\a.jpg").as_deref(), Some(&b"foreign"[..]));
}

// C#: FileActionServiceTests.ExecuteAsync_CopyFailsAfterCompleteCopy_KeepsCompleteDestination
#[test]
fn execute_async_copy_fails_after_complete_copy_keeps_complete_destination() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            disk.copy_file(source, destination).expect("copy");
            Some(io::Error::other("late failure"))
        })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        fx.content(r"C:\photos\sel\a.jpg").map(|c| c.len()),
        Some(11),
        "a complete copy is never deleted"
    );
}

// C#: FileActionServiceTests.ExecuteAsync_MoveFailsBecauseForeignFileAppearedAtDestination_NeverDeletesTheForeignFile
#[test]
fn execute_async_move_fails_because_foreign_file_appeared_at_destination_never_deletes_the_foreign_file() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_move_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            disk.add_file_at(destination, b"foreign file, longer", stamp(LAST_WRITE_TICKS));
            Some(io::Error::from(io::ErrorKind::AlreadyExists))
        })));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(SOURCE));
    assert_eq!(
        fx.content(r"C:\photos\sel\a.jpg").as_deref(),
        Some(&b"foreign file, longer"[..]),
        "the foreign destination file is never deleted"
    );
}

// C#: FileActionServiceTests.ExecuteAsync_MoveFailsAfterSourceGone_KeepsTheDestination
#[test]
fn execute_async_move_fails_after_source_gone_keeps_the_destination() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_move_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            disk.move_file(source, destination).expect("the move itself succeeds");
            Some(io::Error::other("late failure"))
        })));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        result.source_removed,
        "F3: the source is provably gone after a prepared move"
    );
    assert!(
        fx.file_exists(r"C:\photos\sel\a.jpg"),
        "the moved file stays where it is"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionServiceSingleMutationGapTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionServiceSingleMutationGapTests.ExecuteAsync_RecycleCommitAppendFails_ReportsSuccessWithJournalNotPersisted
#[test]
fn execute_async_recycle_commit_append_fails_reports_success_with_journal_not_persisted() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(result.succeeded);
    assert!(!result.journal_persisted);
    assert!(result.journal_error.is_some());
}

// C#: FileActionServiceSingleMutationGapTests.ExecuteAsync_FailureAfterTheSourceVanished_ReportsSourceRemovedOnlyForAMove
#[test]
fn execute_async_failure_after_the_source_vanished_reports_source_removed_only_for_a_move() {
    // Move: the move happened, then the verify fails: source_removed = true.
    let fx = Fixture::with_move_override(|disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            disk.move_file(source, destination)?;
            Err(io::Error::other("late failure"))
        })
    });
    fx.add_file(SOURCE, 11);
    let moved = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!moved.succeeded);
    assert!(moved.source_removed);

    // Copy: the source is still there after the failure: source_removed = false.
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_copy_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some(io::Error::other("late failure"))
    })));
    let copied = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!copied.succeeded);
    assert!(!copied.source_removed);
}

// C#: FileActionServiceSingleMutationGapTests.ExecuteAsync_CancelledCopy_IsDismissedOnlyWhileTheSourceIsUntouched
// Theory: same content and stamp => Dismissed; other content or +1 s => Failed.
#[test]
fn execute_async_cancelled_copy_is_dismissed_only_while_the_source_is_untouched() {
    #[derive(Clone, Copy)]
    enum Change {
        None,
        Content,
        Stamp,
    }
    for (change, expected) in [
        (Change::None, JournalState::Dismissed),
        (Change::Content, JournalState::Failed),
        (Change::Stamp, JournalState::Failed),
    ] {
        let fx = Fixture::new();
        fx.add_file(SOURCE, 11);
        let disk = fx.disk.clone();
        fx.hooks.set_copy_hook(Some(Arc::new(move |source: &Path, _: &Path| {
            match change {
                Change::None => {}
                Change::Content => disk.add_file_at(source, b"changed!!!!", stamp(LAST_WRITE_TICKS)),
                Change::Stamp => disk.add_file_at(source, &common_file_actions::pattern_bytes(11), last_write_plus(1)),
            }
            Some(canceled_error())
        })));
        let result = fx.run(&single(SOURCE, Copy, Some("sel")));
        assert!(!result.succeeded);
        assert_eq!(fx.journal_entries().pop().map(|e| e.state), Some(expected));
    }
}

// C#: FileActionServiceSingleMutationGapTests.ExecuteAsync_CancelledCopyAndSourceCannotBeInspected_KeepsTheFailedRecoveryItemAndDoesNotThrow
// Needs the P1 stat seam (P-12): a stat that fails makes the source uninspectable, so the entry is Failed, never Dismissed.
#[test]
fn execute_async_cancelled_copy_and_source_cannot_be_inspected_keeps_the_failed_recovery_item_and_does_not_throw() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks
        .set_copy_hook(Some(Arc::new(|_: &Path, _: &Path| Some(canceled_error()))));
    fx.hooks.set_stat_hook(Some(Arc::new(|path: &Path| {
        if path.to_string_lossy().eq_ignore_ascii_case(SOURCE) {
            Some(io::Error::new(io::ErrorKind::PermissionDenied, "access denied"))
        } else {
            None
        }
    })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(fx.journal_entries().pop().map(|e| e.state), Some(JournalState::Failed));
}

// C#: FileActionServiceSingleMutationGapTests.ExecuteAsync_UnknownOperation_FailsAsUnsupportedAndWritesNoJournalLine
// NO behavioural twin: `FileOperationType` is a closed enum, so `(FileOperationType)99` does not exist in Rust. The twin
// is the catalog text only (`core.fileAction.unsupportedOperation`). This test passes without the service body.
#[test]
fn execute_async_unknown_operation_fails_as_unsupported_and_writes_no_journal_line() {
    let texts = EnglishFileActionTexts;
    assert_eq!(
        texts.core_file_action_unsupported_operation(Recycle),
        "Operation not supported: Recycle"
    );
}

// C#: FileActionServiceSingleMutationGapTests.ExecuteAsync_ClockFailsAfterTheRecycleCompleted_StillReportsTheCompletedOutcome
// NO behavioural twin: `Clock::utc_now` cannot fail. Twin: a recycle whose Committed append fails still reports success.
#[test]
fn execute_async_clock_fails_after_the_recycle_completed_still_reports_the_completed_outcome() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(result.succeeded);
    assert_eq!(result.error, None);
    assert!(!result.permanently_deleted);
}

// C#: FileActionServiceSingleMutationGapTests.ExecuteAsync_MoveFailsAndTheSourceCannotBeChecked_DoesNotThrowAndKeepsTheSource
// NO behavioural twin in this form: `FileSystem::file_exists` cannot fail (C# `File.Exists` never throws either). Twin:
// after a failed move the source is still there, so `source_removed == false`.
#[test]
fn execute_async_move_fails_and_the_source_cannot_be_checked_does_not_throw_and_keeps_the_source() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks
        .set_move_hook(Some(Arc::new(|_: &Path, _: &Path| Some(io::Error::other("no")))));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(!result.source_removed);
    assert!(fx.file_exists(SOURCE));
}

// ---------------------------------------------------------------------------------------------------------------
// JournalCommitClockFailureTests.cs, FileActionMutationAuditBTests.cs, FileActionSafetyEdgeTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalCommitClockFailureTests.ExecuteAsync_ClockThrowsStampingCommitted_SucceedsWithJournalErrorAndNoFailedLine
// NO behavioural twin: the clock is infallible. Twin: a Committed append failure => succeeded, journal_persisted == false,
// and no Failed line is written.
#[test]
fn execute_async_clock_throws_stamping_committed_succeeds_with_journal_error_and_no_failed_line() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(result.succeeded);
    assert!(!result.journal_persisted);
    assert!(fx.journal_entries().iter().all(|e| e.state != JournalState::Failed));
}

// C#: FileActionMutationAuditBTests.ExecuteAsync_Recycle_ShellReportsSuccessButFileIsStillThere_FailsAndIsNotCommitted
// The shell reports success but the file stays: SOURCE_STILL_EXISTS_AFTER_RECOVERY (B-06), no Committed line.
#[test]
fn execute_async_recycle_shell_reports_success_but_file_is_still_there_fails_and_is_not_committed() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.leave_on_send(SOURCE);
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_code_text(SOURCE_STILL_EXISTS_AFTER_RECOVERY).as_str())
    );
    assert!(states(&fx).iter().all(|s| *s != JournalState::Committed));
    assert_eq!(fx.journal_entries().pop().map(|e| e.state), Some(JournalState::Failed));
}

// C#: FileActionSafetyEdgeTests.PreparedAppendFails_NoMove_NoOrphanFailedRecord
#[test]
fn prepared_append_fails_no_move_no_orphan_failed_record() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(SOURCE));
    assert!(!fx.file_exists(r"C:\photos\sel\a.jpg"));
    assert!(
        fx.journal_entries().is_empty(),
        "no orphan Failed record when Prepared could not be written"
    );
}

// Extra (not a C# twin): a recycle refused for a drive without a bin reports the catalog text, not a raw message.
#[test]
fn execute_async_recycle_bin_refusal_message_is_the_catalog_text() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_texts().core_recycle_unsupported_drive("a.jpg").as_str())
    );
}

// ---------------------------------------------------------------------------------------------------------------
// PermanentDeleteTests.cs (single parts) and FileActionFaultInjectionGapTests.cs (four tests)
// ---------------------------------------------------------------------------------------------------------------

// C#: PermanentDeleteTests.Recycle_NoRecycleBinNotAllowed_RefusesAndKeepsFile
#[test]
fn recycle_no_recycle_bin_not_allowed_refuses_and_keeps_file() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(!result.succeeded);
    assert!(fx.file_exists(SOURCE));
    assert!(fx.bin.permanently_deleted().is_empty());
}

// C#: PermanentDeleteTests.Recycle_NoRecycleBinAllowed_DeletesPermanentlyAndJournalsTruthfully
#[test]
fn recycle_no_recycle_bin_allowed_deletes_permanently_and_journals_truthfully() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    let mut request = single(SOURCE, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run(&request);
    assert!(result.succeeded);
    assert!(result.permanently_deleted);
    assert!(!fx.file_exists(SOURCE));
    let entries = fx.journal_entries();
    assert_eq!(entries.last().and_then(|e| e.permanent), Some(true));
}

// C#: PermanentDeleteTests.Recycle_FixedDrive_UnchangedEvenWithFlag
#[test]
fn recycle_fixed_drive_unchanged_even_with_flag() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let mut request = single(SOURCE, Recycle, None);
    request.allow_permanent_delete = true;
    let result = fx.run(&request);
    assert!(result.succeeded);
    assert!(
        !result.permanently_deleted,
        "a bin on the drive: the flag does not force a permanent delete"
    );
    assert_eq!(fx.bin.recycled(), vec![SOURCE.to_owned()]);
}

// C#: PermanentDeleteTests.Recycle_BinCannotHoldFile_RefusesBeforeJournal
#[test]
fn recycle_bin_cannot_hold_file_refuses_before_journal() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_capacity(r"C:\", 10);
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_texts().core_recycle_bin_cannot_hold("a.jpg").as_str())
    );
    assert!(fx.journal_lines().is_empty(), "refused before any journal line");
    assert!(fx.file_exists(SOURCE));
}

// C#: PermanentDeleteTests.Recycle_BinFits_PassesStatSizeAndRecycles
#[test]
fn recycle_bin_fits_passes_stat_size_and_recycles() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_capacity(r"C:\", 11);
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(result.succeeded);
    assert_eq!(fx.bin.fits_calls(), vec![SOURCE.to_owned()]);
}

// C#: PermanentDeleteTests.Recycle_NoRecycleBin_DoesNotAskCapacity
#[test]
fn recycle_no_recycle_bin_does_not_ask_capacity() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    fx.bin.set_capacity(r"C:\", 0);
    let mut request = single(SOURCE, Recycle, None);
    request.allow_permanent_delete = true;
    let _ = fx.run(&request);
    assert!(
        fx.bin.fits_calls().is_empty(),
        "capacity is not asked for a permanent delete"
    );
}

// C#: FileActionFaultInjectionGapTests.CancelAfterPrepared_SingleMove_IsObserved
// The cancel token is cancelled when the move starts (golden `cancel_on`): the move is refused after Prepared.
#[test]
fn cancel_after_prepared_single_move_is_observed() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    fx.hooks.set_move_hook(Some(Arc::new(move |_: &Path, _: &Path| {
        trigger.cancel();
        None
    })));
    let result = fx.run_with(&single(SOURCE, Move, Some("sel")), &cancel);
    assert!(!result.succeeded);
    assert!(fx.journal_entries().iter().any(|e| e.state == JournalState::Prepared));
}

// C#: FileActionFaultInjectionGapTests.Unauthorized_AtTheFileOperation_IsACleanFailure
// A "not authorized" fault at the move (the C# FaultKind.FailUnauthorized) is a clean failure: the source stays.
#[test]
fn unauthorized_at_the_file_operation_is_a_clean_failure() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_move_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some(io::Error::new(io::ErrorKind::PermissionDenied, "not authorized"))
    })));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(SOURCE));
    assert_eq!(fx.journal_entries().pop().map(|e| e.state), Some(JournalState::Failed));
}

// C#: FileActionFaultInjectionGapTests.PermanentDelete_NotOptedIn_IsRefusedWithoutAnyMutation
#[test]
fn permanent_delete_not_opted_in_is_refused_without_any_mutation() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.bin.set_no_bin_roots(&[r"C:\"]);
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(!result.succeeded);
    assert!(fx.file_exists(SOURCE));
    assert!(fx.bin.permanently_deleted().is_empty());
    assert!(fx.journal_lines().is_empty());
}

// Extra: pins the fault rig of pr-test-support (P-14, the C# FaultInjectionRig the four gap tests use). Mutations are
// counted from 1: the folder (1), the Prepared append (2), the move (3). A crash at the move kills the "process".
#[test]
fn fault_rig_arms_a_crash_on_the_move_and_marks_the_process_dead() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let rig = Arc::new(FaultInjectionFileSystem::new(fx.disk.clone()));
    rig.arm(Some(FaultPlan {
        at: 3,
        kind: FaultKind::CrashBefore,
    }));
    let clock = fx.clock.clone();
    let rig_fs: Arc<dyn FileSystem> = rig.clone();
    let journal = Arc::new(OperationJournal::new(
        &AppPaths::new(Path::new(r"C:\data"), None, false),
        rig_fs.clone(),
        clock.clone(),
    ));
    let service = FileActionService::new(journal, rig_fs, clock, fx.bin.clone());
    let result = service.execute(&single(SOURCE, Move, Some("sel")), &CancelToken::none());
    assert!(!result.succeeded);
    assert!(rig.dead(), "the crash at the move kills the injected process");
    assert!(fx.file_exists(SOURCE), "the crashed move never happened");
}

// ---------------------------------------------------------------------------------------------------------------
// Texts and the journal's coded messages (JournalErrorCodeTests, EnglishFileActionTexts)
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalErrorCodeTests.FileAction_SizeChangedAfterMove_JournalsCode_ResultShowsUiLanguage
// A journal whose `localized` returns "VI:<key>": the result shows the localized text, the journal keeps the code and
// the ENGLISH text (the journal's own `english`).
#[test]
fn file_action_size_changed_after_move_journals_code_result_shows_ui_language() {
    struct Vietnamese;
    impl JournalTexts for Vietnamese {
        fn english(&self, key: &str) -> String {
            EnglishJournalTexts.english(key)
        }
        fn localized(&self, key: &str) -> String {
            format!("VI:{key}")
        }
    }
    let options = OperationJournalOptions {
        texts: Some(Arc::new(Vietnamese)),
        ..OperationJournalOptions::default()
    };
    let fx = Fixture::with_move_override_and_journal_options(
        |disk| -> MoveOverride {
            Arc::new(move |source: &Path, destination: &Path| {
                disk.move_file(source, destination)?;
                disk.add_file_at(destination, b"aaaa", stamp(LAST_WRITE_TICKS));
                Ok(())
            })
        },
        options,
    );
    fx.add_file(SOURCE, 11);
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    let key = catalog_key(VERIFY_SIZE_CHANGED).expect("known code");
    assert_eq!(result.error.as_deref(), Some(format!("VI:{key}").as_str()));
    let last = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(last.error_code.as_deref(), Some(VERIFY_SIZE_CHANGED));
    assert_eq!(
        last.error.as_deref(),
        Some(english_code_text(VERIFY_SIZE_CHANGED).as_str()),
        "the journal keeps the English text"
    );
}

// C#: JournalErrorCodeTests.FileAction_OsFailure_JournalsRawMessageWithoutCode
#[test]
fn file_action_os_failure_journals_raw_message_without_code() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_move_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some(io::Error::other("The process cannot access the file."))
    })));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    let last = fx.journal_entries().pop().expect("a Failed line");
    assert_eq!(last.error_code, None);
    assert_eq!(last.error.as_deref(), Some("The process cannot access the file."));
}

// C#: LiveOperationMarkerTests.FileAction_MoveCopy_MarkerCoversPreparedToOutcome (Move part, the marker is live until the outcome)
#[test]
fn file_action_move_copy_marker_covers_prepared_to_outcome() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(result.succeeded);
    let id = fx
        .journal_entries()
        .last()
        .map(|e| e.id.clone())
        .expect("a Committed line");
    assert!(
        !fx.journal.live_operations().is_live(&id),
        "the marker is released after the outcome"
    );
}

// C#: LiveOperationMarkerTests.FileAction_Recycle_MarkerCoversPreparedToOutcome (Recycle part)
#[test]
fn file_action_recycle_marker_covers_prepared_to_outcome() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let result = fx.run(&single(SOURCE, Recycle, None));
    assert!(result.succeeded);
    let id = fx
        .journal_entries()
        .last()
        .map(|e| e.id.clone())
        .expect("a Committed line");
    assert!(!fx.journal.live_operations().is_live(&id));
}

// C#: LiveOperationMarkerTests.FileAction_PreparedAppendFails_MarkerReleased
#[test]
fn file_action_prepared_append_fails_marker_released() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(!fx.journal.live_operations().is_live("anything"));
}

// ---------------------------------------------------------------------------------------------------------------
// ActionDestinationRootTests.cs and DestinationEscapeTests.cs (single parts)
// ---------------------------------------------------------------------------------------------------------------

// C#: ActionDestinationRootTests.ExecuteAsync_PhotoAtRootWithRelativeDestination_LandsInsideTheRoot
#[test]
fn execute_async_photo_at_root_with_relative_destination_lands_inside_the_root() {
    let fx = Fixture::new();
    fx.add_file(r"C:\a.jpg", 11);
    let result = fx.run(&single(r"C:\a.jpg", Move, Some("sel")));
    assert!(result.succeeded);
    assert!(fx.file_exists(r"C:\sel\a.jpg"));
}

// C#: ActionDestinationRootTests.ExecuteAsync_PhotoAtRootWithParentTraversal_IsRejectedAndNothingMoves
#[test]
fn execute_async_photo_at_root_with_parent_traversal_is_rejected_and_nothing_moves() {
    let fx = Fixture::new();
    fx.add_file(r"C:\a.jpg", 11);
    let result = fx.run(&single(r"C:\a.jpg", Move, Some(r"..\x")));
    assert!(!result.succeeded);
    assert_eq!(result.error.as_deref(), Some(outside_text().as_str()));
    assert!(fx.file_exists(r"C:\a.jpg"));
}

// C#: DestinationEscapeTests.HostileRelativeDestination_NeverLeavesPhotoFolder
// Every hostile spelling is refused before anything moves, and the file never leaves the photo folder.
#[test]
fn hostile_relative_destination_never_leaves_photo_folder() {
    for hostile in [r"..\..\evil", r"sub\..\..\evil", "..", r"..\", "D:x", r"\x", "a|b"] {
        let fx = Fixture::new();
        fx.add_file(SOURCE, 11);
        let result = fx.run(&single(SOURCE, Move, Some(hostile)));
        assert!(!result.succeeded, "{hostile:?} must be refused");
        assert!(fx.file_exists(SOURCE), "{hostile:?}: the source stays");
        assert!(fx.events_of(&["move", "copy"]).is_empty(), "{hostile:?}: nothing moved");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionFaultInjectionGapTests / CopyForeignDestinationTests (single parts)
// ---------------------------------------------------------------------------------------------------------------

// C#: CopyForeignDestinationTests.SingleCopy_SourceVanishesAndForeignDestinationArrives_ForeignFileSurvives
#[test]
fn single_copy_source_vanishes_and_foreign_destination_arrives_foreign_file_survives() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |source: &Path, destination: &Path| {
            disk.delete(source).expect("source vanishes");
            disk.add_file_at(destination, b"foreign", stamp(LAST_WRITE_TICKS));
            Some(io::Error::other("copy failed"))
        })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(fx.content(r"C:\photos\sel\a.jpg").as_deref(), Some(&b"foreign"[..]));
}

// C#: CopyForeignDestinationTests.SingleCopy_FailsAfterWritingPartialDestination_PartialFileIsKeptForRecovery
#[test]
fn single_copy_fails_after_writing_partial_destination_partial_file_is_kept_for_recovery() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_copy_partial_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some((4, io::Error::other("disk full")))
    })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    // TODO(W3-07): the recovery check of the kept partial file is asserted by W3-07; here only the disk state.
    assert!(fx.file_exists(r"C:\photos\sel\a.jpg"));
}

// C#: CopyForeignDestinationTests.SingleCopy_ForeignShorterFileAppearsAndCopyThrowsGenericError_ForeignFileSurvives
#[test]
fn single_copy_foreign_shorter_file_appears_and_copy_throws_generic_error_foreign_file_survives() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            disk.add_file_at(destination, b"xy", stamp(LAST_WRITE_TICKS));
            Some(io::Error::other("generic error"))
        })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(fx.content(r"C:\photos\sel\a.jpg").as_deref(), Some(&b"xy"[..]));
}

// C#: CopyRollbackIdentityTests.SingleCopy_VerificationFails_OwnIncompleteDestinationIsRemoved
#[test]
fn single_copy_verification_fails_own_incomplete_destination_is_removed() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    // The source shrinks just before the copy: the copy is 4 bytes, the verify expects the 11 the Prepared line recorded.
    let disk = fx.disk.clone();
    fx.hooks.set_copy_hook(Some(Arc::new(move |source: &Path, _: &Path| {
        disk.add_file_at(source, b"aaaa", stamp(LAST_WRITE_TICKS));
        None
    })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(
        result.error.as_deref(),
        Some(english_code_text(VERIFY_SIZE_CHANGED).as_str())
    );
    assert!(!fx.file_exists(r"C:\photos\sel\a.jpg"), "our own short copy is removed");
}

// C#: CopyRollbackIdentityTests.SingleCopy_ForeignShorterFileReplacesDestinationBeforeCleanup_ForeignFileSurvives
#[test]
fn single_copy_foreign_shorter_file_replaces_destination_before_cleanup_foreign_file_survives() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    let disk = fx.disk.clone();
    fx.hooks
        .set_copy_hook(Some(Arc::new(move |_: &Path, destination: &Path| {
            disk.add_file_at(destination, b"xy", stamp(LAST_WRITE_TICKS));
            Some(io::Error::other("generic error"))
        })));
    let result = fx.run(&single(SOURCE, Copy, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(fx.content(r"C:\photos\sel\a.jpg").as_deref(), Some(&b"xy"[..]));
}

// C#: FileActionDestinationFolderCleanupTests.Single_JournalFailsAtBegin_DestinationFolderCreatedByThisCall_IsRemoved
#[test]
fn single_journal_fails_at_begin_destination_folder_created_by_this_call_is_removed() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(
        !fx.dir_exists(SEL),
        "a folder this call created is removed when Prepared fails"
    );
}

// C#: FileActionDestinationFolderCleanupTests.Single_JournalFailsAtBegin_PreExistingEmptyDestinationFolder_IsKept
#[test]
fn single_journal_fails_at_begin_pre_existing_empty_destination_folder_is_kept() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.add_dir(SEL);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.dir_exists(SEL));
}

// C#: FileActionDestinationFolderCleanupTests.Single_JournalFailsAtBegin_PreExistingDestinationFolderWithFiles_IsKeptWithItsFiles
#[test]
fn single_journal_fails_at_begin_pre_existing_destination_folder_with_files_is_kept_with_its_files() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.add_file(r"C:\photos\sel\keep.jpg", 3);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(r"C:\photos\sel\keep.jpg"));
}

// C#: FileActionDestinationFolderCleanupTests.Single_FailureAfterPrepared_KeepsTheDestinationFolderLikeBefore
#[test]
fn single_failure_after_prepared_keeps_the_destination_folder_like_before() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks
        .set_move_hook(Some(Arc::new(|_: &Path, _: &Path| Some(io::Error::other("boom")))));
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.dir_exists(SEL), "a prepared action keeps the folder it created");
}

// C#: FileActionDestinationFolderCleanupTests.Single_JournalFailsAtBegin_CleanupThatThrows_DoesNotHideTheOriginalFailure
// Rust: `try_delete_empty_directory` cannot throw; the hook makes it return false, and the original error is reported.
#[test]
fn single_journal_fails_at_begin_cleanup_that_throws_does_not_hide_the_original_failure() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks
        .set_delete_directory_hook(Some(Arc::new(|_: &Path| Some(io::Error::other("cleanup failed")))));
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert_eq!(result.error.as_deref(), Some("journal unavailable"));
}

// ---------------------------------------------------------------------------------------------------------------
// Journal append failures and the Prepared line (golden-derived, with the journal's own numbering)
// ---------------------------------------------------------------------------------------------------------------

// Extra (golden `single-move-prepared-append-fails-*`): a failing Prepared append writes nothing and moves nothing.
#[test]
fn execute_async_prepared_append_failure_does_not_mutate_and_writes_nothing() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fail_journal_append_nth(&fx, 1, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(fx.file_exists(SOURCE));
    assert!(fx.events_of(&["move"]).is_empty());
}

// Golden `single-move-pre-cancelled-with-override-still-moves` (the Move-with-override row of the C# cancelled-before-start
// Theory): the override is awaited without a token, so a pre-cancelled move with an override still moves.
#[test]
fn execute_async_pre_cancelled_move_with_override_still_moves() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let fx = Fixture::with_move_override(move |disk| -> MoveOverride {
        Arc::new(move |source: &Path, destination: &Path| {
            counter.fetch_add(1, Ordering::SeqCst);
            disk.move_file(source, destination)
        })
    });
    fx.add_file(SOURCE, 11);
    let cancel = CancelToken::new();
    cancel.cancel();
    let result = fx.run_with(&single(SOURCE, Move, Some("sel")), &cancel);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(result.succeeded);
}

// Extra (golden `single-move-failed-append-fails`): a failing Failed append is reported in `journal_error`.
#[test]
fn execute_async_move_failed_append_fails_reports_journal_error() {
    let fx = Fixture::new();
    fx.add_file(SOURCE, 11);
    fx.hooks.set_move_hook(Some(Arc::new(|_: &Path, _: &Path| {
        Some(io::Error::other("disk on fire"))
    })));
    fail_journal_append_nth(&fx, 2, "journal unavailable");
    let result = fx.run(&single(SOURCE, Move, Some("sel")));
    assert!(!result.succeeded);
    assert!(!result.journal_persisted);
}

// ---------------------------------------------------------------------------------------------------------------
// Catalog parity: every English text of the unit equals its en.json entry (sample arguments substituted)
// ---------------------------------------------------------------------------------------------------------------

// C#: Tr.* of `en.json` for core.fileAction.* / core.recycle.* / core.groupAction.* (the catalog this unit reads).
// The English texts are complete in the stub, so this test passes without the service body; it pins the catalog.
#[test]
fn english_file_action_texts_match_en_json_catalog() {
    let path = pr_test_support::golden::repo_root()
        .join("assets")
        .join("languages")
        .join("en.json");
    let catalog: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("en.json reads")).expect("en.json parses");
    let text = |key: &str| {
        catalog[key]
            .as_str()
            .unwrap_or_else(|| panic!("en.json has {key}"))
            .to_owned()
    };
    let texts = EnglishFileActionTexts;
    assert_eq!(texts.core_file_action_busy(), text("core.fileAction.busy"));
    assert_eq!(
        texts.core_file_action_group_needs_two_paths(),
        text("core.fileAction.groupNeedsTwoPaths")
    );
    assert_eq!(
        texts.core_file_action_unsupported_operation(FileOperationType::Recycle),
        text("core.fileAction.unsupportedOperation").replace("{operation}", "Recycle")
    );
    assert_eq!(
        texts.core_file_action_no_destination(),
        text("core.fileAction.noDestination")
    );
    assert_eq!(texts.core_file_action_same_folder(), text("core.fileAction.sameFolder"));
    assert_eq!(
        texts.core_file_action_destination_exists("P"),
        text("core.fileAction.destinationExists").replace("{path}", "P")
    );
    assert_eq!(
        texts.core_file_action_source_missing("P"),
        text("core.fileAction.sourceMissing").replace("{path}", "P")
    );
    assert_eq!(
        texts.core_recycle_unsupported_drive("N"),
        text("core.recycle.unsupportedDrive").replace("{name}", "N")
    );
    assert_eq!(
        texts.core_recycle_bin_cannot_hold("N"),
        text("core.recycle.binCannotHold").replace("{name}", "N")
    );
    assert_eq!(
        texts.core_group_action_rollback_failed("M", 2),
        text("core.groupAction.rollbackFailed")
            .replace("{message}", "M")
            .replace("{count}", "2")
    );
    // The journal's own coded texts are in the same catalog and must equal the English journal texts.
    for (key, english) in pr_core::file_actions::journal::ENGLISH_JOURNAL_TEXTS {
        assert_eq!(text(key), english, "{key}");
    }
}
