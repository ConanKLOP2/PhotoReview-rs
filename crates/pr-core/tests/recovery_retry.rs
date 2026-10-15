//! Red tests for W3-06 `RecoveryRetryService` (`crates/pr-core/src/file_actions/recovery_retry.rs`).
//!
//! Ports of the C# tests listed in `contracts/W3-06.md` (baseline d92d6aae, `tests/PhotoReview.Core.Tests/FileActions/`).
//! Every test carries `// C#: <Class>.<Method>`. C# asserts on the Vietnamese `Tr` text; these tests assert the English
//! catalog of the same key (`EnglishRecoveryRetryTexts`, `EnglishJournalTexts`). The scripted disk, bin and gate live
//! in `recovery_retry_support` (fakes only: the real Recycle Bin is never touched).
//!
//! Against the stubs every test that reaches the service panics with `todo!()`; that is the red state.
//!
//! NOT PORTED (documented, see the comment at each one):
//! * `Constructor_NullValidation`: the parameters are not optional in Rust (`Arc` / typed values cannot be null).
//! * the two `...RunsMutationOffCallerThread` / `Retry_GroupRecycle_RunsTheBinCallsOffTheCallerThread` tests: R0003
//!   makes the call blocking, the caller owns the thread. Their twins assert the Rust contract (the mutation runs on
//!   the calling thread).

mod recovery_retry_support;

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{Clock, FileSystem, LiveOperationRegistry};
use pr_core::file_actions::errors::{
    self, CANCELLED_BY_USER, MOVE_SOURCE_NOT_REMOVED, PENDING_UNCONFIRMED, RETRY_VERIFY_FAILED,
    SOURCE_STILL_EXISTS_AFTER_RECOVERY,
};
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::journal::{EnglishJournalTexts, JournalTexts};
use pr_core::file_actions::journal_line::JournalDateTime;
use pr_core::file_actions::journal_line::{JournalEntry, JournalGroupMember};
use pr_core::file_actions::recovery_check::{RecoveryFileCheck, RecoveryVerdict};
use pr_core::file_actions::recovery_retry::{
    EnglishRecoveryRetryTexts, RecoveryRetryResult, RecoveryRetryService, RecoveryRetryTexts,
};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::{CancelToken, Canceled};
use pr_core::time::DateTimeUtc;
use recovery_retry_support::*;

/// The English catalog text of a journal error code (C# `Tr` text of the coded error).
fn english_code_text(code: &str) -> String {
    errors::text(code, &|key| EnglishJournalTexts.localized(key)).expect("known journal code")
}

/// One retry with a fresh, un-cancelled token (C# `RetryMoveOrCopyAsync(failed)`).
fn retry(service: &RecoveryRetryService, failed: &JournalEntry) -> RecoveryRetryResult {
    service
        .retry_move_or_copy(failed, false, &CancelToken::none())
        .expect("the retry was not cancelled")
}

/// One retry with an explicit token and confirmation flag.
fn retry_with(
    service: &RecoveryRetryService,
    failed: &JournalEntry,
    confirmed: bool,
    cancel: &CancelToken,
) -> Result<RecoveryRetryResult, Canceled> {
    service.retry_move_or_copy(failed, confirmed, cancel)
}

/// The English texts, for message checks.
fn texts() -> Arc<dyn RecoveryRetryTexts> {
    Arc::new(EnglishRecoveryRetryTexts)
}

const SOURCE: &str = r"C:\photos\a.jpg";
const DEST: &str = r"C:\photos\sub\a.jpg";
const SEED_TIMESTAMP: i64 = CSHARP_CLOCK_TICKS;

// ---------------------------------------------------------------------------------------------------------------
// RecoveryRetryServiceTests.cs
// ---------------------------------------------------------------------------------------------------------------

/// Seeds `text` at `path` and returns its fingerprint (the snapshot's size and last write), as the C# tests read it.
fn seed(rig: &Rig, path: &str, text: &str) -> (i64, DateTimeUtc) {
    rig.fs.seed_text(path, text, ts(SEED_TIMESTAMP));
    let stat = stat_of(&rig.fs, path);
    (stat.length as i64, stat.last_write_utc)
}

// C#: RecoveryRetryServiceTests.SizeMismatchMove_SourceGone_RecoveryRefusesRetryAndAllowsDismiss
// Seeded by hand (the W3-04 move-override route is covered by the file_actions_service* tests): source gone,
// destination 1 byte, so the recovery check reads DestinationChanged.
#[test]
fn size_mismatch_move_source_gone_recovery_refuses_retry_and_allows_dismiss() {
    let rig = Rig::new();
    let dest = r"D:\Backup\a.jpg";
    rig.fs.seed_text(dest, "x", ts(SEED_TIMESTAMP));
    let failed = failed_entry(
        "op-size",
        FileOperationType::Move,
        SOURCE,
        dest,
        11,
        ts(SEED_TIMESTAMP),
        ts(SEED_TIMESTAMP),
    );
    rig.append(&failed);

    let check = RecoveryFileCheck::new(rig.fs_dyn()).check(&failed);
    assert_eq!(check.verdict, RecoveryVerdict::DestinationChanged);

    let result = retry(&rig.service(), &failed);
    assert!(!result.succeeded);
    assert_eq!(result.message, texts().core_recovery_source_missing());
    assert!(!rig.fs.has_file(SOURCE));
    assert!(rig.fs.has_file(dest));
    assert_eq!(stat_of(&rig.fs, dest).length, 1);

    rig.journal
        .dismiss(std::slice::from_ref(&failed))
        .expect("dismiss appends");
    assert!(rig.failed().is_empty());
    assert!(rig.fs.has_file(dest));
}

// C#: RecoveryRetryServiceTests.RetryMove_FailedUndoMove_CommittedEntryKeepsUndoFlag
#[test]
fn retry_move_failed_undo_move_committed_entry_keeps_undo_flag() {
    let rig = Rig::new();
    let source = r"D:\sorted\a.jpg";
    let dest = r"C:\photos\a.jpg";
    let (size, last_write) = seed(&rig, source, "12345");
    let failed = JournalEntry {
        undo: Some(true),
        ..failed_entry(
            "op-undo",
            FileOperationType::Move,
            source,
            dest,
            size,
            last_write,
            ts(SEED_TIMESTAMP),
        )
    };

    let result = retry(&rig.service(), &failed);

    assert!(result.succeeded);
    assert_eq!(result.entry.as_ref().and_then(|e| e.undo), Some(true));
    let committed = rig.committed();
    assert!(
        committed
            .iter()
            .filter(|e| e.id == "op-undo")
            .all(|e| e.undo == Some(true))
    );
}

// C#: RecoveryRetryServiceTests.RetryMove_Success
#[test]
fn retry_move_success() {
    let rig = Rig::new();
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    let failed = failed_entry(
        "op-1",
        FileOperationType::Move,
        SOURCE,
        DEST,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(result.succeeded);
    assert_eq!(result.message, texts().core_recovery_succeeded());
    let entry = result.entry.expect("a committed entry");
    assert_eq!(entry.state, JournalState::Committed);
    assert!(!rig.fs.has_file(SOURCE));
    assert!(rig.fs.has_file(DEST));
}

// C#: RecoveryRetryServiceTests.RetryMove_SourceStillExistsAfterMove_FailsWithDistinctCode
#[test]
fn retry_move_source_still_exists_after_move_fails_with_distinct_code() {
    let rig = Rig::new();
    let dest = r"D:\sorted\a.jpg";
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    rig.fs.set_move_leaves_source(true);
    let failed = failed_entry(
        "op-1",
        FileOperationType::Move,
        SOURCE,
        dest,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, english_code_text(MOVE_SOURCE_NOT_REMOVED));
    let entry = result.entry.expect("a failed entry");
    assert_eq!(entry.state, JournalState::Failed);
    assert_eq!(entry.error_code.as_deref(), Some(MOVE_SOURCE_NOT_REMOVED));
    assert!(rig.fs.has_file(SOURCE));
    assert!(rig.fs.has_file(dest));
    let failed_now = rig.failed();
    assert_eq!(failed_now.len(), 1);
    assert_eq!(failed_now[0].error_code.as_deref(), Some(MOVE_SOURCE_NOT_REMOVED));
    assert!(rig.committed().is_empty());
}

// C#: RecoveryRetryServiceTests.RetryCopy_Success
#[test]
fn retry_copy_success() {
    let rig = Rig::new();
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    let failed = failed_entry(
        "op-copy",
        FileOperationType::Copy,
        SOURCE,
        DEST,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(result.succeeded);
    assert_eq!(result.message, texts().core_recovery_succeeded());
    let entry = result.entry.expect("a committed entry");
    assert_eq!(entry.state, JournalState::Committed);
    assert!(rig.fs.has_file(SOURCE));
    assert!(rig.fs.has_file(DEST));
}

// C#: RecoveryRetryServiceTests.RetryMove_CommitJournalFailure_PreservesCompletedOutcome
#[test]
fn retry_move_commit_journal_failure_preserves_completed_outcome() {
    let rig = Rig::new();
    let source = r"C:\photos\journal-failure.jpg";
    let dest = r"C:\photos\sub\journal-failure.jpg";
    let (size, last_write) = seed(&rig, source, "12345");
    // The second journal append is the Committed line.
    rig.fs.add_rule(data_rule(
        "append",
        None,
        2,
        vec![Op::Throw {
            kind: io::ErrorKind::Other,
            message: "journal unavailable".to_owned(),
        }],
    ));
    let failed = failed_entry(
        "op-journal-failure",
        FileOperationType::Move,
        source,
        dest,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(result.succeeded);
    assert!(!result.journal_persisted);
    assert_eq!(result.journal_error.as_deref(), Some("journal unavailable"));
    assert!(!rig.fs.has_file(source));
    assert!(rig.fs.has_file(dest));
}

// C#: RecoveryRetryServiceTests.RetryMove_MutationAndFailureJournalFailure_PreservesOriginalFailure
#[test]
fn retry_move_mutation_and_failure_journal_failure_preserves_original_failure() {
    let rig = Rig::new();
    let source = r"C:\photos\mutation-failure.jpg";
    let dest = r"C:\photos\sub\mutation-failure.jpg";
    let (size, last_write) = seed(&rig, source, "12345");
    // The second append is the Failed line, which also fails.
    rig.fs.add_rule(data_rule(
        "append",
        None,
        2,
        vec![Op::Throw {
            kind: io::ErrorKind::Other,
            message: "journal unavailable".to_owned(),
        }],
    ));
    rig.fs
        .on_every("move", |_fs, _call| Err(io::Error::other("move unavailable")));
    let failed = failed_entry(
        "op-mutation-journal-failure",
        FileOperationType::Move,
        source,
        dest,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(!result.journal_persisted);
    assert_eq!(result.journal_error.as_deref(), Some("journal unavailable"));
    assert!(
        result.message.contains("move unavailable"),
        "message was {:?}",
        result.message
    );
    assert!(rig.fs.has_file(source));
    assert!(!rig.fs.has_file(dest));
}

// C#: RecoveryRetryServiceTests.Retry_RejectsRecycle
#[test]
fn retry_rejects_recycle() {
    let rig = Rig::new();
    let failed = entry(
        "op-recycle",
        FileOperationType::Recycle,
        JournalState::Failed,
        SOURCE,
        None,
        100,
        ts(SEED_TIMESTAMP),
        ts(SEED_TIMESTAMP),
        None,
    );

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, texts().core_recovery_only_move_copy());
}

// C#: RecoveryRetryServiceTests.Retry_RejectsMissingDestination
#[test]
fn retry_rejects_missing_destination() {
    let rig = Rig::new();
    let failed = entry(
        "op-nodest",
        FileOperationType::Move,
        JournalState::Failed,
        SOURCE,
        None,
        100,
        ts(SEED_TIMESTAMP),
        ts(SEED_TIMESTAMP),
        None,
    );

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, texts().core_recovery_no_destination());
}

// C#: RecoveryRetryServiceTests.Retry_RejectsMissingSource
#[test]
fn retry_rejects_missing_source() {
    let rig = Rig::new();
    let failed = failed_entry(
        "op-nosrc",
        FileOperationType::Move,
        r"C:\photos\missing.jpg",
        r"C:\photos\dest.jpg",
        100,
        ts(SEED_TIMESTAMP),
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, texts().core_recovery_source_missing());
}

// C#: RecoveryRetryServiceTests.Retry_RejectsChangedSource
#[test]
fn retry_rejects_changed_source() {
    let rig = Rig::new();
    seed(&rig, SOURCE, "new content with different length");
    let failed = failed_entry(
        "op-changed",
        FileOperationType::Move,
        SOURCE,
        r"C:\photos\dest.jpg",
        5,
        ts(SEED_TIMESTAMP),
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, texts().core_recovery_source_changed());
}

// C#: RecoveryRetryServiceTests.Retry_RejectsExistingDestination
#[test]
fn retry_rejects_existing_destination() {
    let rig = Rig::new();
    let dest = r"C:\photos\dest.jpg";
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    rig.fs.seed_text(dest, "already exists", ts(SEED_TIMESTAMP));
    let failed = failed_entry(
        "op-destexists",
        FileOperationType::Move,
        SOURCE,
        dest,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, texts().core_recovery_destination_exists());
}

// C#: RecoveryRetryServiceTests.RetryMoveOrCopyAsync_RunsMutationOffCallerThread
// NOT PORTED (R0003): the call is blocking and the caller (the Recovery presenter, W8) owns the thread. The twin below
// asserts the Rust contract instead: the mutation runs on the CALLING thread.
#[test]
fn retry_move_or_copy_runs_mutation_on_the_calling_thread() {
    let rig = Rig::new();
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    let failed = failed_entry(
        "op-thread",
        FileOperationType::Copy,
        SOURCE,
        DEST,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );
    let mutation_thread = Arc::new(Mutex::new(None));
    let slot = mutation_thread.clone();
    rig.fs.on_every("copy", move |_fs, _call| {
        *slot.lock().expect("lock") = Some(std::thread::current().id());
        Ok(Next::Proceed)
    });

    let result = retry(&rig.service(), &failed);

    assert!(result.succeeded);
    assert_eq!(
        *mutation_thread.lock().expect("lock"),
        Some(std::thread::current().id())
    );
}

// C#: RecoveryRetryServiceTests.Retry_WhileFileActionInProgress_RefusesWithBusy
#[test]
fn retry_while_file_action_in_progress_refuses_with_busy() {
    // RV-C08: Recovery is a file action too (INV-4); a running Move owns the gate, so nothing is read or journaled.
    let rig = Rig::new();
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    let failed = failed_entry(
        "op-busy",
        FileOperationType::Move,
        SOURCE,
        DEST,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );
    rig.append(&failed);
    let gate = new_gate();
    let service = rig.service().with_file_action_gate(gate.clone());
    assert!(gate.try_begin()); // a Move is running
    rig.fs.clear_ops();

    let busy = retry(&service, &failed);

    assert!(!busy.succeeded);
    assert!(!busy.superseded);
    assert!(busy.entry.is_none());
    assert_eq!(busy.message, texts().core_recovery_busy());
    assert_eq!(rig.fs.count_ops("stat "), 0);
    assert!(
        rig.fs.ops().is_empty(),
        "no journal append, no move: {:?}",
        rig.fs.ops()
    );
    assert!(rig.fs.has_file(SOURCE));
    assert!(!rig.fs.has_file(DEST));
    assert!(gate.is_busy()); // the running action still owns the gate

    gate.end();
    let retried = retry(&service, &failed);
    assert!(retried.succeeded, "{}", retried.message);
    assert!(!gate.is_busy()); // the retry released the gate it took
}

// C#: RecoveryRetryServiceTests.Retry_GroupWhileFileActionInProgress_RefusesWithBusy
#[test]
fn retry_group_while_file_action_in_progress_refuses_with_busy() {
    let rig = Rig::new();
    let jpeg = r"C:\photos\a.jpg";
    let raw = r"C:\photos\a.cr2";
    let members = vec![
        member(jpeg, Some(r"C:\photos\sub\a.jpg"), 5, ts(SEED_TIMESTAMP)),
        member(raw, Some(r"C:\photos\sub\a.cr2"), 5, ts(SEED_TIMESTAMP)),
    ];
    let failed = JournalEntry {
        group_id: Some("g".to_owned()),
        group_members: Some(members),
        ..failed_entry(
            "op-group",
            FileOperationType::Move,
            jpeg,
            r"C:\photos\sub\a.jpg",
            5,
            ts(SEED_TIMESTAMP),
            ts(SEED_TIMESTAMP),
        )
    };
    let gate = new_gate();
    let service = rig.service().with_file_action_gate(gate.clone());
    assert!(gate.try_begin());
    rig.fs.clear_ops();

    let busy = retry(&service, &failed);

    assert_eq!(busy.message, texts().core_recovery_busy());
    assert_eq!(rig.fs.count_ops("stat "), 0);
    assert!(rig.fs.ops().is_empty(), "nothing read or journaled: {:?}", rig.fs.ops());
    assert!(gate.is_busy());
}

// C#: RecoveryRetryServiceTests.RetryCopy_FailsMidway_KeepsUnclaimedPartialDestinationAsConflict
// D-01 (decision c): File.Copy gives no proof of creation, so the retry's partial copy is kept and the entry becomes a
// Conflict.
#[test]
fn retry_copy_fails_midway_keeps_unclaimed_partial_destination_as_conflict() {
    let rig = Rig::new();
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    let failed = failed_entry(
        "op-copy-partial",
        FileOperationType::Copy,
        SOURCE,
        DEST,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );
    rig.append(&failed);
    rig.fs.set_copy_fails_after(Some(2));

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(rig.fs.text(SOURCE), "12345");
    assert_eq!(stat_of(&rig.fs, DEST).length, 2);
    let entry = result.entry.expect("a failed entry");
    assert_eq!(
        RecoveryFileCheck::new(rig.fs_dyn()).check(&entry).verdict,
        RecoveryVerdict::Conflict
    );
}

// C#: RecoveryRetryServiceTests.RetryGroupCopy_FailsMidway_KeepsUnclaimedPartialDestinationOfTheFailingMember
#[test]
fn retry_group_copy_fails_midway_keeps_unclaimed_partial_destination_of_the_failing_member() {
    let rig = Rig::new();
    let jpeg = r"C:\photos\a.jpg";
    let raw = r"C:\photos\a.cr2";
    let (jpeg_size, jpeg_write) = seed(&rig, jpeg, "12345");
    let (raw_size, raw_write) = seed(&rig, raw, "1234567890");
    let members = vec![
        member(jpeg, Some(r"C:\photos\sub\a.jpg"), jpeg_size, jpeg_write),
        member(raw, Some(r"C:\photos\sub\a.cr2"), raw_size, raw_write),
    ];
    let failed = JournalEntry {
        group_id: Some("g".to_owned()),
        group_members: Some(members),
        ..failed_entry(
            "op-group-copy",
            FileOperationType::Copy,
            jpeg,
            r"C:\photos\sub\a.jpg",
            jpeg_size,
            jpeg_write,
            ts(SEED_TIMESTAMP),
        )
    };
    rig.append(&failed);
    // The copy of the RAW member fails after 3 bytes; the JPEG copy is untouched.
    rig.fs.on_every("copy", move |fs, call| {
        let from = call.path.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        fs.set_copy_fails_after(if from.eq_ignore_ascii_case(raw) { Some(3) } else { None });
        Ok(Next::Proceed)
    });

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(stat_of(&rig.fs, r"C:\photos\sub\a.cr2").length, 3); // never deleted: no proof it is ours
    assert!(rig.fs.has_file(raw));
}

// C#: RecoveryRetryServiceTests.RetryCopy_FailsMidway_NeverAttemptsToDeleteThePartial_ReportsTheOriginalFailure
#[test]
fn retry_copy_fails_midway_never_attempts_to_delete_the_partial_reports_the_original_failure() {
    let rig = Rig::new();
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    let failed = failed_entry(
        "op-copy-cleanup",
        FileOperationType::Copy,
        SOURCE,
        DEST,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );
    rig.append(&failed);
    rig.fs.set_copy_fails_after(Some(2));
    let delete_attempted = Arc::new(AtomicBool::new(false));
    let flag = delete_attempted.clone();
    rig.fs.on_every("delete", move |_fs, _call| {
        flag.store(true, Ordering::SeqCst);
        Err(io::Error::other("simulated filter driver failure"))
    });

    let result = retry(&rig.service(), &failed);

    assert!(!delete_attempted.load(Ordering::SeqCst)); // no proof of creation: the partial is left alone
    assert_eq!(rig.fs.count_ops("delete "), 0);
    assert!(!result.succeeded);
    assert_eq!(result.message, DISK_FULL_DURING_COPY);
    let entry = result.entry.expect("a failed entry");
    assert_eq!(entry.state, JournalState::Failed);
}

// C#: RecoveryRetryServiceTests.RetryCopy_ForeignFileAppearsAtDestination_IsNotDeleted
#[test]
fn retry_copy_foreign_file_appears_at_destination_is_not_deleted() {
    let rig = Rig::new();
    let (size, last_write) = seed(&rig, SOURCE, "12345");
    let failed = failed_entry(
        "op-copy-foreign",
        FileOperationType::Copy,
        SOURCE,
        DEST,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );
    rig.append(&failed);
    // Someone else's shorter file appears at the destination after the pre-check.
    rig.fs.on_every("copy", |fs, _call| {
        fs.seed_text(DEST, "xy", ts(SEED_TIMESTAMP));
        Ok(Next::Proceed)
    });

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(rig.fs.text(DEST), "xy");
}

// C#: RecoveryRetryServiceTests.Constructor_NullValidation
// NOT PORTED: Arc parameters cannot be null in Rust (the C# ArgumentNullException has no twin).

// C#: RecoveryRetryServiceTests.RetryMove_FailsBecauseForeignFileAppearedAtDestination_NeverDeletesTheForeignFile
#[test]
fn retry_move_fails_because_foreign_file_appeared_at_destination_never_deletes_the_foreign_file() {
    let rig = Rig::new();
    let (size, last_write) = seed(&rig, SOURCE, "original larger photo");
    let failed = failed_entry(
        "op-move-partial",
        FileOperationType::Move,
        SOURCE,
        DEST,
        size,
        last_write,
        ts(SEED_TIMESTAMP),
    );
    rig.append(&failed);
    rig.fs.on_every("move", |fs, _call| {
        fs.seed_text(DEST, "foreign", ts(SEED_TIMESTAMP));
        Err(io::Error::other("destination already exists"))
    });

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(rig.fs.has_file(SOURCE));
    assert_eq!(rig.fs.text(DEST), "foreign");
}

// ---------------------------------------------------------------------------------------------------------------
// RecoveryRetryServiceMutationTests.cs (mutation gaps of the group retry)
// ---------------------------------------------------------------------------------------------------------------

/// 2026-10-01T08:00:00Z in .NET ticks (the C# `Stamp`).
const STAMP: i64 = 639_264_384_000_000_000;
const TICKS_PER_SECOND_RR: i64 = 10_000_000;
const JPG: &str = r"C:\photos\a.jpg";
const RAW: &str = r"C:\photos\a.cr2";
const OUT_JPG: &str = r"C:\photos\out\a.jpg";
const OUT_RAW: &str = r"C:\photos\out\a.cr2";

fn stamp() -> DateTimeUtc {
    ts(STAMP)
}

/// `STAMP` plus `seconds`.
fn stamp_plus(seconds: i64) -> DateTimeUtc {
    ts(STAMP + seconds * TICKS_PER_SECOND_RR)
}

/// The C# mutation fixture: journal clock `Stamp + 1 min`, service clock `Stamp + 2 min`.
fn mutation_rig() -> Rig {
    Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR)
}

/// The service with the default scripted bin (C# `_bin`, no restriction).
fn mutation_service(rig: &Rig) -> RecoveryRetryService {
    rig.service().with_recycle_bin(rig.bin(BinConfig::default()))
}

fn move_member(source: &str, destination: &str, size: i64, permanent: bool) -> JournalGroupMember {
    JournalGroupMember {
        permanent,
        ..member(source, Some(destination), size, stamp())
    }
}

/// C# `FailedMoveGroup()`: group `grp`, Move of `a.jpg` (4 bytes) and `a.cr2` (8 bytes), error "earlier failure".
fn failed_move_group() -> JournalEntry {
    JournalEntry {
        group_id: Some("capture".to_owned()),
        group_members: Some(vec![
            move_member(JPG, OUT_JPG, 4, false),
            move_member(RAW, OUT_RAW, 8, false),
        ]),
        ..entry(
            "grp",
            FileOperationType::Move,
            JournalState::Failed,
            JPG,
            Some(OUT_JPG),
            4,
            stamp(),
            stamp(),
            Some("earlier failure"),
        )
    }
}

/// C# `FailedRecycleGroup(undo, members)`: a Delete group (`undo` = `Some(true)` for the undo of a Delete).
fn failed_recycle_group(undo: Option<bool>, members: Vec<JournalGroupMember>) -> JournalEntry {
    let source = members[0].source.clone();
    let size = members[0].size;
    let permanent = members.iter().any(|m| m.permanent).then_some(true);
    JournalEntry {
        error_code: Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY.to_owned()),
        undo,
        permanent,
        group_id: Some("capture".to_owned()),
        group_members: Some(members),
        ..entry(
            "del",
            FileOperationType::Recycle,
            JournalState::Failed,
            &source,
            None,
            size,
            stamp(),
            stamp(),
            Some("x"),
        )
    }
}

fn seed_jpg_and_raw(rig: &Rig) {
    rig.fs.seed_text(JPG, "jpeg", stamp());
    rig.fs.seed_text(RAW, "raw data", stamp());
}

/// A Delete group whose two members are still on disk (C# `PresentDeleteGroup`).
fn present_delete_group(rig: &Rig, raw_permanent: bool) -> JournalEntry {
    seed_jpg_and_raw(rig);
    let failed = failed_recycle_group(
        None,
        vec![
            member(JPG, None, 4, stamp()),
            JournalGroupMember {
                permanent: raw_permanent,
                ..member(RAW, None, 8, stamp())
            },
        ],
    );
    rig.append(&failed);
    failed
}

/// Runs `action` inside the `nth` stat of `path` (1 = the recovery check), before the stat is read.
fn on_nth_stat(rig: &Rig, path: &str, nth: usize, action: impl Fn(&ScriptedFs) + Send + Sync + 'static) {
    let hook: HookFn = Arc::new(move |fs: &ScriptedFs, _call: &Call<'_>| {
        action(fs);
        Ok(Next::Proceed)
    });
    rig.fs.add_rule(Rule {
        on: "stat",
        path: Some(path.to_owned()),
        nth: Some(nth),
        f: hook,
    });
}

/// C# `AssertRefusedBeforeJournaling`: refused, no Prepared line, no move, no bin mutation.
fn assert_refused_before_journaling(rig: &Rig, result: &RecoveryRetryResult, expected: &str) {
    assert!(!result.succeeded);
    assert_eq!(result.message, expected);
    assert!(result.entry.is_none());
    assert_eq!(
        rig.fs.journal_lines().len(),
        1,
        "only the original Failed line: no Prepared was appended"
    );
    let ops = rig.fs.ops();
    assert!(!ops.iter().any(|o| o.starts_with("move ")), "no move: {ops:?}");
    assert!(
        !ops.iter().any(|o| o.starts_with("bin.recycle")
            || o.starts_with("bin.delete-permanently")
            || o.starts_with("bin.restore")),
        "no bin mutation: {ops:?}"
    );
}

/// The paths the bin was asked to restore, in call order (C# `_bin.RestoreCalls`).
fn bin_restore_calls(rig: &Rig) -> Vec<String> {
    rig.fs
        .ops()
        .iter()
        .filter_map(|o| o.strip_prefix("bin.restore ").map(str::to_owned))
        .collect()
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveEntryWithAnEmptyMemberList_RetriesAsASingleMove
#[test]
fn retry_move_or_copy_move_entry_with_an_empty_member_list_retries_as_a_single_move() {
    let rig = mutation_rig();
    rig.fs.seed_text(JPG, "jpeg", stamp());
    let failed = JournalEntry {
        group_members: Some(Vec::new()),
        ..entry(
            "single",
            FileOperationType::Move,
            JournalState::Failed,
            JPG,
            Some(OUT_JPG),
            4,
            stamp(),
            stamp(),
            Some("x"),
        )
    };
    rig.append(&failed);

    let result = retry(&mutation_service(&rig), &failed);

    assert!(result.succeeded, "{}", result.message);
    assert!(rig.fs.has_file(OUT_JPG));
    assert!(!rig.fs.has_file(JPG));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupWithAMemberFlaggedPermanentAndSettingOff_StillRetries
#[test]
fn retry_move_or_copy_move_group_with_a_member_flagged_permanent_and_setting_off_still_retries() {
    // `Permanent` is only meaningful for Delete: a Move member carrying it must not trip the permanent-delete refusal.
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = JournalEntry {
        group_members: Some(vec![
            move_member(JPG, OUT_JPG, 4, false),
            move_member(RAW, OUT_RAW, 8, true),
        ]),
        ..failed_move_group()
    };
    rig.append(&failed);

    let result = retry(&mutation_service(&rig), &failed);

    assert!(result.succeeded, "{}", result.message);
    assert!(rig.fs.has_file(OUT_JPG));
    assert!(rig.fs.has_file(OUT_RAW));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_UndoDeleteGroupWithAPermanentMissingMember_IsRefusedBeforeAnythingIsRestoredOrJournaled
#[test]
fn retry_move_or_copy_undo_delete_group_with_a_permanent_missing_member_is_refused_before_anything_is_restored_or_journaled()
 {
    // JPEG restorable, RAW was deleted permanently: an undo can never bring it back, so nothing may be touched.
    let rig = mutation_rig();
    let failed = failed_recycle_group(
        Some(true),
        vec![
            member(JPG, None, 4, stamp()),
            JournalGroupMember {
                permanent: true,
                ..member(RAW, None, 8, stamp())
            },
        ],
    );
    rig.append(&failed);

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
    assert!(!rig.fs.has_file(JPG));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_DeleteGroupWithPermanentMemberAndSettingOff_RefusesWithTheUnsupportedDriveMessageAndDeletesNothing
#[test]
fn retry_move_or_copy_delete_group_with_permanent_member_and_setting_off_refuses_with_the_unsupported_drive_message_and_deletes_nothing()
 {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let bin = rig.bin(BinConfig {
        no_bin: vec![RAW.to_owned()],
        ..BinConfig::default()
    });
    let failed = failed_recycle_group(
        None,
        vec![
            member(JPG, None, 4, stamp()),
            JournalGroupMember {
                permanent: true,
                ..member(RAW, None, 8, stamp())
            },
        ],
    );
    rig.append(&failed);
    let service = rig.service().with_recycle_bin(bin);

    let result = retry(&service, &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recycle_unsupported_drive("a.cr2"));
    assert!(rig.fs.has_file(JPG));
    assert!(rig.fs.has_file(RAW));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_DeleteGroupMemberVanishesAfterTheCheck_IsRefusedAndNothingRecycled
#[test]
fn retry_move_or_copy_delete_group_member_vanishes_after_the_check_is_refused_and_nothing_recycled() {
    let rig = mutation_rig();
    let failed = present_delete_group(&rig, false);
    on_nth_stat(&rig, JPG, 2, |fs| fs.remove(JPG));

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_DeleteGroupMemberResizedAfterTheCheck_IsRefusedAndNothingRecycled
#[test]
fn retry_move_or_copy_delete_group_member_resized_after_the_check_is_refused_and_nothing_recycled() {
    let rig = mutation_rig();
    let failed = present_delete_group(&rig, false);
    on_nth_stat(&rig, JPG, 2, |fs| fs.seed_text(JPG, "jpeg!", stamp()));

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
    assert!(rig.fs.has_file(RAW));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_DeleteGroupMemberTouchedAfterTheCheckWithSameSize_IsRefusedAndNothingRecycled
#[test]
fn retry_move_or_copy_delete_group_member_touched_after_the_check_with_same_size_is_refused_and_nothing_recycled() {
    let rig = mutation_rig();
    let failed = present_delete_group(&rig, false);
    on_nth_stat(&rig, JPG, 2, |fs| fs.seed_text(JPG, "jpeg", stamp_plus(30)));

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_DeleteGroupMemberJournaledPermanentButDriveNowHasABin_IsRefusedAndNeverDeletedPermanently
#[test]
fn retry_move_or_copy_delete_group_member_journaled_permanent_but_drive_now_has_a_bin_is_refused_and_never_deleted_permanently()
 {
    let rig = mutation_rig();
    let failed = present_delete_group(&rig, true);
    let service = mutation_service(&rig).with_allow_permanent_delete(Rig::allow(true));

    let result = retry(&service, &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
    assert!(rig.fs.has_file(RAW));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupSourceVanishesAfterTheCheck_IsRefusedBeforeJournaling
#[test]
fn retry_move_or_copy_move_group_source_vanishes_after_the_check_is_refused_before_journaling() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    on_nth_stat(&rig, JPG, 2, |fs| fs.remove(JPG));

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupSourceResizedAfterTheCheck_IsRefusedBeforeJournaling
#[test]
fn retry_move_or_copy_move_group_source_resized_after_the_check_is_refused_before_journaling() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    on_nth_stat(&rig, JPG, 2, |fs| fs.seed_text(JPG, "jpeg!", stamp()));

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupSourceTouchedAfterTheCheckWithSameSize_IsRefusedBeforeJournaling
#[test]
fn retry_move_or_copy_move_group_source_touched_after_the_check_with_same_size_is_refused_before_journaling() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    on_nth_stat(&rig, JPG, 2, |fs| fs.seed_text(JPG, "jpeg", stamp_plus(30)));

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupDestinationAppearsAfterTheCheck_IsRefusedBeforeJournaling
#[test]
fn retry_move_or_copy_move_group_destination_appears_after_the_check_is_refused_before_journaling() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    on_nth_stat(&rig, JPG, 2, |fs| fs.seed_text(OUT_JPG, "someone else's file", stamp()));

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, &english().core_recovery_source_changed());
    assert_eq!(stat_of(&rig.fs, OUT_JPG).length, "someone else's file".len() as u64);
}

/// C# `RetryMoveGroupWith`: the group move with `during` run inside the third stat of the JPEG source (after Prepared).
fn retry_move_group_with(rig: &Rig, during: impl Fn(&ScriptedFs) + Send + Sync + 'static) -> RecoveryRetryResult {
    seed_jpg_and_raw(rig);
    let failed = failed_move_group();
    rig.append(&failed);
    on_nth_stat(rig, JPG, 3, during);
    retry(&mutation_service(rig), &failed)
}

/// C# `AssertFailedAfterPreparedWithoutMoving`.
fn assert_failed_after_prepared_without_moving(rig: &Rig, result: &RecoveryRetryResult, expected: &str) {
    assert!(!result.succeeded);
    assert_eq!(result.message, expected);
    let entry = result.entry.as_ref().expect("the failure is journaled");
    assert_eq!(entry.state, JournalState::Failed);
    assert!(!rig.fs.ops().iter().any(|o| o.starts_with("move ")));
    assert!(!rig.fs.has_file(OUT_RAW));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupSourceVanishesAfterPrepared_FailsAsSourceChangedWithoutMoving
#[test]
fn retry_move_or_copy_move_group_source_vanishes_after_prepared_fails_as_source_changed_without_moving() {
    let rig = mutation_rig();
    let result = retry_move_group_with(&rig, |fs| fs.remove(JPG));

    assert_failed_after_prepared_without_moving(&rig, &result, &english().core_recovery_source_changed());
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupSourceResizedAfterPrepared_FailsAsSourceChangedWithoutMoving
#[test]
fn retry_move_or_copy_move_group_source_resized_after_prepared_fails_as_source_changed_without_moving() {
    let rig = mutation_rig();
    let result = retry_move_group_with(&rig, |fs| fs.seed_text(JPG, "jpeg!", stamp()));

    assert_failed_after_prepared_without_moving(&rig, &result, &english().core_recovery_source_changed());
    assert!(rig.fs.has_file(JPG));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupSourceTouchedAfterPreparedWithSameSize_FailsAsSourceChangedWithoutMoving
#[test]
fn retry_move_or_copy_move_group_source_touched_after_prepared_with_same_size_fails_as_source_changed_without_moving() {
    let rig = mutation_rig();
    let result = retry_move_group_with(&rig, |fs| fs.seed_text(JPG, "jpeg", stamp_plus(30)));

    assert_failed_after_prepared_without_moving(&rig, &result, &english().core_recovery_source_changed());
    assert!(rig.fs.has_file(JPG));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupDestinationAppearsAfterPrepared_FailsAsDestinationExistsAndNeverOverwrites
#[test]
fn retry_move_or_copy_move_group_destination_appears_after_prepared_fails_as_destination_exists_and_never_overwrites() {
    let rig = mutation_rig();
    let result = retry_move_group_with(&rig, |fs| fs.seed_text(OUT_JPG, "someone else's file", stamp()));

    assert_failed_after_prepared_without_moving(&rig, &result, &english().core_recovery_destination_exists());
    assert!(rig.fs.has_file(JPG));
    assert_eq!(stat_of(&rig.fs, OUT_JPG).length, "someone else's file".len() as u64);
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupMoveLeavesTheSourceBehind_FailsWithRetryVerifyFailed
#[test]
fn retry_move_or_copy_move_group_move_leaves_the_source_behind_fails_with_retry_verify_failed() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    rig.fs.set_move_leaves_source(true); // cross-volume move that copied but could not delete the source

    let result = retry(&mutation_service(&rig), &failed);

    assert!(!result.succeeded);
    assert_eq!(
        result.entry.as_ref().and_then(|e| e.error_code.as_deref()),
        Some(RETRY_VERIFY_FAILED)
    );
    assert!(rig.pending().is_empty());
    let failed_now = rig.failed();
    assert_eq!(failed_now.len(), 1);
    assert_eq!(failed_now[0].error_code.as_deref(), Some(RETRY_VERIFY_FAILED));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupMovedFileHasAnotherSize_FailsWithRetryVerifyFailed
#[test]
fn retry_move_or_copy_move_group_moved_file_has_another_size_fails_with_retry_verify_failed() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    // The file is replaced by a longer one right when it is moved (same write time, so the re-checks passed).
    rig.fs.on_every("move", |fs, call| {
        if call.path.is_some_and(|p| p.to_string_lossy().eq_ignore_ascii_case(JPG)) {
            fs.seed_text(JPG, "jpeg plus more", stamp());
        }
        Ok(Next::Proceed)
    });

    let result = retry(&mutation_service(&rig), &failed);

    assert!(!result.succeeded);
    assert_eq!(
        result.entry.as_ref().and_then(|e| e.error_code.as_deref()),
        Some(RETRY_VERIFY_FAILED)
    );
    assert!(!rig.fs.has_file(OUT_RAW)); // the loop stopped at the first member
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_UndoDeleteGroupBinReportsFailureEvenThoughAFileAppeared_FailsAsRestoreFailed
#[test]
fn retry_move_or_copy_undo_delete_group_bin_reports_failure_even_though_a_file_appeared_fails_as_restore_failed() {
    let rig = mutation_rig();
    let failed = failed_recycle_group(
        Some(true),
        vec![member(JPG, None, 4, stamp()), member(RAW, None, 8, stamp())],
    );
    rig.append(&failed);
    // TryRestore says "not restored" and still left a file behind.
    let bin = rig.bin(BinConfig {
        restore: RestoreMode::Synthetic {
            recreate: true,
            answer: false,
        },
        ..BinConfig::default()
    });

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_undo_recycle_restore_failed("a.jpg"));
    assert_eq!(bin_restore_calls(&rig), vec![JPG.to_owned()]); // stopped at the first failure
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_UndoDeleteGroupBinReportsSuccessButNothingCameBack_FailsAsRestoreFailed
#[test]
fn retry_move_or_copy_undo_delete_group_bin_reports_success_but_nothing_came_back_fails_as_restore_failed() {
    let rig = mutation_rig();
    let failed = failed_recycle_group(
        Some(true),
        vec![member(JPG, None, 4, stamp()), member(RAW, None, 8, stamp())],
    );
    rig.append(&failed);
    let bin = rig.bin(BinConfig {
        restore: RestoreMode::Synthetic {
            recreate: false,
            answer: true,
        },
        ..BinConfig::default()
    });

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_undo_recycle_restore_failed("a.jpg"));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_UndoDeleteGroupRestoresEveryMissingMember_Succeeds
#[test]
fn retry_move_or_copy_undo_delete_group_restores_every_missing_member_succeeds() {
    let rig = mutation_rig();
    let failed = failed_recycle_group(
        Some(true),
        vec![member(JPG, None, 4, stamp()), member(RAW, None, 8, stamp())],
    );
    rig.append(&failed);
    let bin = rig.bin(BinConfig {
        restore: RestoreMode::Synthetic {
            recreate: true,
            answer: true,
        },
        ..BinConfig::default()
    });

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(result.succeeded, "{}", result.message);
    assert_eq!(bin_restore_calls(&rig), vec![JPG.to_owned(), RAW.to_owned()]);
    assert!(rig.fs.has_file(JPG));
    assert!(rig.fs.has_file(RAW));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_GroupMoveSucceedsAndJournals_ReportsPlainSuccessWithPersistedJournal
#[test]
fn retry_move_or_copy_group_move_succeeds_and_journals_reports_plain_success_with_persisted_journal() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);

    let result = retry(&mutation_service(&rig), &failed);

    assert!(result.succeeded, "{}", result.message);
    assert_eq!(result.message, english().core_recovery_succeeded());
    assert!(result.journal_persisted);
    assert_eq!(result.journal_error, None);
    assert_eq!(result.entry.as_ref().map(|e| e.state), Some(JournalState::Committed));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_GroupMoveSucceedsButCommitAppendFails_ReportsSuccessWithUnpersistedJournal
#[test]
fn retry_move_or_copy_group_move_succeeds_but_commit_append_fails_reports_success_with_unpersisted_journal() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    // Append 1 = Prepared, append 2 = Committed.
    rig.fs.add_rule(data_rule(
        "append",
        None,
        2,
        vec![Op::Throw {
            kind: io::ErrorKind::Other,
            message: "disk full".to_owned(),
        }],
    ));

    let result = retry(&mutation_service(&rig), &failed);

    assert!(result.succeeded);
    assert_eq!(result.message, english().core_recovery_succeeded_journal_failed());
    assert!(!result.journal_persisted);
    assert_eq!(result.journal_error.as_deref(), Some("disk full"));
    assert!(rig.fs.has_file(OUT_JPG));
    assert!(rig.fs.has_file(OUT_RAW));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_GroupMoveFailsAndFailureIsJournaled_ReportsTheFailureWithPersistedJournal
#[test]
fn retry_move_or_copy_group_move_fails_and_failure_is_journaled_reports_the_failure_with_persisted_journal() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    rig.fs.on_every("move", |_fs, _call| Err(io::Error::other("boom")));

    let result = retry(&mutation_service(&rig), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, "boom");
    assert!(result.journal_persisted);
    assert_eq!(result.journal_error, None);
    assert_eq!(result.entry.as_ref().map(|e| e.state), Some(JournalState::Failed));
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_GroupMoveFailsAndTheFailureAppendFailsToo_ReportsNotJournaled
#[test]
fn retry_move_or_copy_group_move_fails_and_the_failure_append_fails_too_reports_not_journaled() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    rig.fs.on_every("move", |_fs, _call| Err(io::Error::other("boom")));
    // Append 1 = Prepared, append 2 = Failed.
    rig.fs.add_rule(data_rule(
        "append",
        None,
        2,
        vec![Op::Throw {
            kind: io::ErrorKind::Other,
            message: "disk full".to_owned(),
        }],
    ));

    let result = retry(&mutation_service(&rig), &failed);

    assert!(!result.succeeded);
    assert_eq!(
        result.message,
        english().core_recovery_completed_failure_not_journaled()
    );
    assert!(!result.journal_persisted);
    assert_eq!(result.journal_error.as_deref(), Some("disk full"));
    assert!(result.entry.is_none());
}

/// C# `RetryMoveOrCopyAsync_MoveGroupRecheckThrowsAFileSystemError_FailsWithItsMessageBeforeJournaling` (Theory rows).
fn recheck_throws_before_journaling(kind: io::ErrorKind) {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let failed = failed_move_group();
    rig.append(&failed);
    rig.fs.add_rule(data_rule(
        "stat",
        Some(JPG),
        2,
        vec![Op::Throw {
            kind,
            message: "share gone".to_owned(),
        }],
    ));

    let result = retry(&mutation_service(&rig), &failed);

    assert_refused_before_journaling(&rig, &result, "share gone");
}

// C#: RecoveryRetryServiceMutationTests.RetryMoveOrCopyAsync_MoveGroupRecheckThrowsAFileSystemError_FailsWithItsMessageBeforeJournaling (IOException)
#[test]
fn retry_move_or_copy_move_group_recheck_throws_io_error_fails_with_its_message_before_journaling() {
    recheck_throws_before_journaling(io::ErrorKind::Other);
}

// C#: ...MoveGroupRecheckThrowsAFileSystemError_FailsWithItsMessageBeforeJournaling (UnauthorizedAccessException)
#[test]
fn retry_move_or_copy_move_group_recheck_throws_unauthorized_access_fails_with_its_message_before_journaling() {
    recheck_throws_before_journaling(io::ErrorKind::PermissionDenied);
}

// C#: ...MoveGroupRecheckThrowsAFileSystemError_FailsWithItsMessageBeforeJournaling (ArgumentException)
#[test]
fn retry_move_or_copy_move_group_recheck_throws_argument_fails_with_its_message_before_journaling() {
    recheck_throws_before_journaling(io::ErrorKind::InvalidInput);
}

// C#: ...MoveGroupRecheckThrowsAFileSystemError_FailsWithItsMessageBeforeJournaling (NotSupportedException)
#[test]
fn retry_move_or_copy_move_group_recheck_throws_not_supported_fails_with_its_message_before_journaling() {
    recheck_throws_before_journaling(io::ErrorKind::Unsupported);
}

// ---------------------------------------------------------------------------------------------------------------
// RecoveryRetryRaceTests.cs (P02: two processes over one journal file)
// ---------------------------------------------------------------------------------------------------------------

/// A clock that reads a distinct instant on every read, as with real clocks in two processes (C# `SteppingClock`).
struct SteppingClock {
    now: Mutex<DateTimeUtc>,
}

impl Clock for SteppingClock {
    fn utc_now(&self) -> DateTimeUtc {
        let mut now = self.now.lock().expect("clock lock");
        *now = now.checked_add_ticks(10_000).expect("clock stays in range"); // +1 ms
        *now
    }
}

/// Two "processes": each with its own journal (own lock, own live registry) over one shared scripted disk.
struct TwoProcesses {
    fs: ScriptedFs,
    clock: Arc<SteppingClock>,
    journal_a: Arc<OperationJournal>,
    journal_b: Arc<OperationJournal>,
}

const RACE_SOURCE: &str = r"C:\photos\a.jpg";
const RACE_DEST: &str = r"D:\sorted\a.jpg";

fn two_processes() -> TwoProcesses {
    let fs = ScriptedFs::new(CancelToken::new());
    let clock = Arc::new(SteppingClock {
        now: Mutex::new(ts(639260928000000000)), // 2026-09-27T08:00:00Z
    });
    let paths = journal_paths();
    let fs_dyn: Arc<dyn FileSystem> = Arc::new(fs.clone());
    let clock_dyn: Arc<dyn Clock> = clock.clone();
    let journal_a = Arc::new(OperationJournal::new(&paths, fs_dyn.clone(), clock_dyn.clone()));
    let journal_b = Arc::new(OperationJournal::new(&paths, fs_dyn, clock_dyn));
    TwoProcesses {
        fs,
        clock,
        journal_a,
        journal_b,
    }
}

impl TwoProcesses {
    fn service(&self, journal: &Arc<OperationJournal>) -> RecoveryRetryService {
        let fs_dyn: Arc<dyn FileSystem> = Arc::new(self.fs.clone());
        let clock_dyn: Arc<dyn Clock> = self.clock.clone();
        RecoveryRetryService::new(journal.clone(), fs_dyn, clock_dyn, english())
    }

    /// The journal lines in file order (C# `History()`).
    fn history(&self) -> Vec<JournalEntry> {
        let mut out = Vec::new();
        self.journal_a
            .read_entries(&mut |entry| out.push(entry))
            .expect("journal reads back");
        out
    }

    /// Seeds `RACE_SOURCE` and journals a Failed Move of it (C# `SeedFailedMove`); returns the snapshot.
    fn seed_failed_move(&self, op_type: FileOperationType) -> JournalEntry {
        self.fs.seed_text(RACE_SOURCE, "photo-bytes", ts(SEED_TIMESTAMP));
        let stat = stat_of(&self.fs, RACE_SOURCE);
        let line = JournalEntry {
            error: Some("The process cannot access the file".to_owned()),
            ..entry(
                "op-race",
                op_type,
                JournalState::Failed,
                RACE_SOURCE,
                Some(RACE_DEST),
                stat.length as i64,
                stat.last_write_utc,
                self.clock.utc_now(),
                None,
            )
        };
        self.journal_a.append(&line).expect("seed append");
        let mut failed = self.journal_a.read_pending_and_failed_operations().expect("reads back");
        assert_eq!(failed.len(), 1, "one snapshot");
        failed.remove(0)
    }
}

/// Runs `action` once, on the first journal append or move that fires after it is armed (the re-entrancy guard of the
/// C# hooks: `if (winner is not null) return null`). The action may call the other process's service.
fn once_on(fs: &ScriptedFs, on: &'static str, action: impl Fn() + Send + Sync + 'static) {
    let started = Arc::new(AtomicBool::new(false));
    fs.add_rule(Rule {
        on,
        path: None,
        nth: None,
        f: Arc::new(move |_fs: &ScriptedFs, _call: &Call<'_>| {
            if !started.swap(true, Ordering::SeqCst) {
                action();
            }
            Ok(Next::Proceed)
        }),
    });
}

// C#: RecoveryRetryRaceTests.ConcurrentRetry_LoserPreparedAfterWinnerCommitted_DoesNotOverwriteCommittedWithFailed
#[test]
fn concurrent_retry_loser_prepared_after_winner_committed_does_not_overwrite_committed_with_failed() {
    let p = two_processes();
    let snapshot = p.seed_failed_move(FileOperationType::Move);

    // B (the loser) passes the latest-entry check; right as it opens the journal to append its Prepared, A (the winner)
    // runs its whole retry. B's Prepared then lands after A's Committed and B's Move throws because A already moved it.
    let winner: Arc<Mutex<Option<RecoveryRetryResult>>> = Arc::new(Mutex::new(None));
    let winner_slot = winner.clone();
    let service_a = p.service(&p.journal_a);
    let snapshot_for_a = snapshot.clone();
    once_on(&p.fs, "append", move || {
        let result = service_a
            .retry_move_or_copy(&snapshot_for_a, false, &CancelToken::none())
            .expect("winner not cancelled");
        *winner_slot.lock().expect("lock") = Some(result);
    });

    let loser = retry(&p.service(&p.journal_b), &snapshot);
    p.fs.clear_rules();

    let winner = winner.lock().expect("lock").take().expect("the winner ran");
    assert!(winner.succeeded);
    assert_eq!(winner.entry.as_ref().map(|e| e.state), Some(JournalState::Committed));
    assert!(!loser.succeeded);
    assert!(loser.superseded);
    assert_eq!(loser.message, english().core_recovery_already_handled());
    assert!(!p.fs.has_file(RACE_SOURCE));
    assert!(p.fs.has_file(RACE_DEST));

    // The bug: a Failed record after A's Committed made the completed Move look permanently Failed.
    let failed_now = p.journal_a.read_pending_and_failed_operations().expect("reads back");
    assert!(
        !failed_now
            .iter()
            .any(|e| e.id == "op-race" && e.state == JournalState::Failed)
    );
    assert_eq!(
        p.history().iter().map(|e| e.state).collect::<Vec<_>>(),
        vec![
            JournalState::Failed,
            JournalState::Prepared,
            JournalState::Committed,
            JournalState::Prepared
        ]
    );

    // B's dangling Prepared is resolved by the next startup reconcile from the file system: the Move did happen.
    let reconciled = p.journal_b.reconcile_pending_operations(None).expect("reconcile");
    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, JournalState::Committed);
    assert!(
        p.journal_a
            .read_pending_and_failed_operations()
            .expect("reads back")
            .is_empty()
    );
}

// C#: RecoveryRetryRaceTests.ConcurrentGroupRetry_LoserPreparedAfterWinnerCommitted_IsSupersededAndWritesNoFailedLine
#[test]
fn concurrent_group_retry_loser_prepared_after_winner_committed_is_superseded_and_writes_no_failed_line() {
    let p = two_processes();
    let names = ["a.jpg", "b.jpg"];
    let members: Vec<JournalGroupMember> = names
        .iter()
        .map(|name| {
            let source = format!(r"C:\photos\{name}");
            let dest = format!(r"D:\sorted\{name}");
            p.fs.seed_text(&source, &format!("photo-{name}"), ts(SEED_TIMESTAMP));
            let stat = stat_of(&p.fs, &source);
            member(&source, Some(&dest), stat.length as i64, stat.last_write_utc)
        })
        .collect();
    let first = members[0].clone();
    let line = JournalEntry {
        error: Some("The process cannot access the file".to_owned()),
        group_id: Some("g1".to_owned()),
        group_members: Some(members.clone()),
        ..entry(
            "op-race-g",
            FileOperationType::Move,
            JournalState::Failed,
            &first.source,
            first.destination.as_deref(),
            first.size,
            first.last_write_utc.value,
            p.clock.utc_now(),
            None,
        )
    };
    p.journal_a.append(&line).expect("seed append");
    let snapshot = p
        .journal_a
        .read_pending_and_failed_operations()
        .expect("reads back")
        .remove(0);

    let winner: Arc<Mutex<Option<RecoveryRetryResult>>> = Arc::new(Mutex::new(None));
    let winner_slot = winner.clone();
    let service_a = p.service(&p.journal_a);
    let snapshot_for_a = snapshot.clone();
    once_on(&p.fs, "append", move || {
        let result = service_a
            .retry_move_or_copy(&snapshot_for_a, false, &CancelToken::none())
            .expect("winner not cancelled");
        *winner_slot.lock().expect("lock") = Some(result);
    });

    let loser = retry(&p.service(&p.journal_b), &snapshot);
    p.fs.clear_rules();

    let winner = winner.lock().expect("lock").take().expect("the winner ran");
    assert!(winner.succeeded);
    assert!(!loser.succeeded);
    assert!(loser.superseded);
    assert_eq!(loser.message, english().core_recovery_already_handled());
    assert!(loser.entry.is_none());
    for m in &members {
        let dest = m.destination.as_deref().expect("a destination");
        assert!(p.fs.has_file(dest) && !p.fs.has_file(&m.source));
    }
    let failed_now = p.journal_a.read_pending_and_failed_operations().expect("reads back");
    assert!(
        !failed_now
            .iter()
            .any(|e| e.id == "op-race-g" && e.state == JournalState::Failed)
    );
    assert!(!p.history().iter().skip(1).any(|e| e.state == JournalState::Failed));
}

// C#: RecoveryRetryRaceTests.ConcurrentRetry_SecondStartsAfterFirstPrepared_IsRefusedAndFirstCommits
#[test]
fn concurrent_retry_second_starts_after_first_prepared_is_refused_and_first_commits() {
    let p = two_processes();
    let snapshot = p.seed_failed_move(FileOperationType::Move);

    // B has appended its Prepared; just before its Move, A retries the same snapshot.
    let late: Arc<Mutex<Option<RecoveryRetryResult>>> = Arc::new(Mutex::new(None));
    let late_slot = late.clone();
    let service_a = p.service(&p.journal_a);
    let snapshot_for_a = snapshot.clone();
    once_on(&p.fs, "move", move || {
        let result = service_a
            .retry_move_or_copy(&snapshot_for_a, false, &CancelToken::none())
            .expect("late retry not cancelled");
        *late_slot.lock().expect("lock") = Some(result);
    });

    let first = retry(&p.service(&p.journal_b), &snapshot);
    p.fs.clear_rules();

    assert!(first.succeeded);
    assert!(!first.superseded);
    let late = late.lock().expect("lock").take().expect("the late retry ran");
    assert!(!late.succeeded);
    assert!(late.superseded);
    assert!(late.entry.is_none());
    assert!(!p.fs.has_file(RACE_SOURCE));
    assert!(p.fs.has_file(RACE_DEST));
    assert_eq!(
        p.history().iter().map(|e| e.state).collect::<Vec<_>>(),
        vec![JournalState::Failed, JournalState::Prepared, JournalState::Committed]
    );
    assert!(
        p.journal_a
            .read_pending_and_failed_operations()
            .expect("reads back")
            .is_empty()
    );
}

// C#: RecoveryRetryRaceTests.Retry_StaleSnapshot_IsRefusedWithoutMutationOrJournalAppend (Theory: Committed, Failed, Dismissed)
#[test]
fn retry_stale_snapshot_is_refused_without_mutation_or_journal_append_committed() {
    retry_stale_snapshot_is_refused(JournalState::Committed);
}

// C#: ...Retry_StaleSnapshot_IsRefusedWithoutMutationOrJournalAppend (Failed)
#[test]
fn retry_stale_snapshot_is_refused_without_mutation_or_journal_append_failed() {
    retry_stale_snapshot_is_refused(JournalState::Failed);
}

// C#: ...Retry_StaleSnapshot_IsRefusedWithoutMutationOrJournalAppend (Dismissed)
#[test]
fn retry_stale_snapshot_is_refused_without_mutation_or_journal_append_dismissed() {
    retry_stale_snapshot_is_refused(JournalState::Dismissed);
}

fn retry_stale_snapshot_is_refused(newer: JournalState) {
    let p = two_processes();
    let snapshot = p.seed_failed_move(FileOperationType::Copy);
    // Another process resolved the same Id after this window took its snapshot (a Failed here stands for another
    // process's retry that failed again: its record differs from the snapshot).
    let other = JournalEntry {
        state: newer,
        timestamp_utc: JournalDateTime::utc(p.clock.utc_now()),
        error: Some("other window".to_owned()),
        ..snapshot.clone()
    };
    p.journal_b.append(&other).expect("other window appends");
    let before = p.history().len();

    let result = retry(&p.service(&p.journal_a), &snapshot);

    assert!(!result.succeeded);
    assert!(result.superseded);
    assert_eq!(result.message, english().core_recovery_already_handled());
    assert!(result.entry.is_none());
    assert!(!p.fs.has_file(RACE_DEST));
    assert!(p.fs.has_file(RACE_SOURCE));
    assert_eq!(p.history().len(), before);
}

// C#: RecoveryRetryRaceTests.Retry_IdLiveInAnotherProcess_IsRefusedWithoutMutationOrJournalAppend
#[test]
fn retry_id_live_in_another_process_is_refused_without_mutation_or_journal_append() {
    let p = two_processes();
    let snapshot = p.seed_failed_move(FileOperationType::Move);
    let before = p.history().len();
    let service_a = p.service(&p.journal_a);
    {
        // another process is executing this Id right now
        let _marker = p.journal_a.live_operations().begin(&snapshot.id);

        let result = retry(&service_a, &snapshot);

        assert!(result.superseded);
        assert!(!result.succeeded);
        assert!(p.fs.has_file(RACE_SOURCE));
        assert!(!p.fs.has_file(RACE_DEST));
        assert_eq!(p.history().len(), before);
    }

    // Marker gone: the same snapshot is still current, so the retry now runs.
    let retried = retry(&service_a, &snapshot);
    assert!(retried.succeeded);
    assert!(!retried.superseded);
}

// C#: RecoveryRetryRaceTests.Retry_CurrentSnapshot_GenuineFailure_StillJournalsFailed
#[test]
fn retry_current_snapshot_genuine_failure_still_journals_failed() {
    let p = two_processes();
    let snapshot = p.seed_failed_move(FileOperationType::Move);
    p.fs.on_every("move", |_fs, _call| Err(io::Error::other("disk unplugged")));

    let result = retry(&p.service(&p.journal_a), &snapshot);

    assert!(!result.succeeded);
    assert!(!result.superseded);
    let failed = p.journal_a.read_pending_and_failed_operations().expect("reads back");
    let failed: Vec<_> = failed.iter().filter(|e| e.state == JournalState::Failed).collect();
    assert_eq!(failed.len(), 1);
    assert_ne!(*failed[0], snapshot);
    assert_eq!(
        p.history().iter().map(|e| e.state).collect::<Vec<_>>(),
        vec![JournalState::Failed, JournalState::Prepared, JournalState::Failed]
    );
}

// ---------------------------------------------------------------------------------------------------------------
// RecoveryRetryCancelledTests.cs
// ---------------------------------------------------------------------------------------------------------------

const CANCEL_JPG: &str = r"C:\photos\a.jpg";
const CANCEL_RAW: &str = r"C:\photos\a.cr2";
const CANCEL_OUT_JPG: &str = r"C:\photos\out\a.jpg";
const CANCEL_OUT_RAW: &str = r"C:\photos\out\a.cr2";

/// C# `CancelledGroupCopy()`: a Copy group whose user cancellation is journaled.
fn cancelled_group_copy() -> JournalEntry {
    let text = english_code_text(CANCELLED_BY_USER);
    JournalEntry {
        error_code: Some(CANCELLED_BY_USER.to_owned()),
        group_id: Some("capture".to_owned()),
        group_members: Some(vec![
            member(CANCEL_JPG, Some(CANCEL_OUT_JPG), 4, stamp()),
            member(CANCEL_RAW, Some(CANCEL_OUT_RAW), 8, stamp()),
        ]),
        ..entry(
            "copy-group",
            FileOperationType::Copy,
            JournalState::Failed,
            CANCEL_JPG,
            Some(CANCEL_OUT_JPG),
            4,
            stamp(),
            stamp(),
            Some(text.as_str()),
        )
    }
}

fn cancelled_rig() -> Rig {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(CANCEL_JPG, "jpeg", stamp());
    rig.fs.seed_text(CANCEL_RAW, "raw data", stamp());
    rig
}

// C#: RecoveryRetryCancelledTests.RetryMoveOrCopyAsync_CancelledGroupCopyWithoutConfirmation_IsRefusedAndNothingIsCopied
#[test]
fn retry_move_or_copy_cancelled_group_copy_without_confirmation_is_refused_and_nothing_is_copied() {
    let rig = cancelled_rig();
    let failed = cancelled_group_copy();
    rig.append(&failed);

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_recovery_cancelled_needs_confirm());
    assert!(!rig.fs.has_file(CANCEL_OUT_JPG));
    assert!(!rig.fs.has_file(CANCEL_OUT_RAW));
    assert_eq!(rig.failed(), vec![failed.clone()]); // untouched: still the same Failed line
}

// C#: RecoveryRetryCancelledTests.RetryMoveOrCopyAsync_CancelledGroupCopyExplicitlyConfirmed_FinishesTheCopy
#[test]
fn retry_move_or_copy_cancelled_group_copy_explicitly_confirmed_finishes_the_copy() {
    let rig = cancelled_rig();
    let failed = cancelled_group_copy();
    rig.append(&failed);

    let result = retry_with(&rig.service(), &failed, true, &CancelToken::none()).expect("not cancelled");

    assert!(result.succeeded, "{}", result.message);
    assert!(rig.fs.has_file(CANCEL_OUT_JPG));
    assert!(rig.fs.has_file(CANCEL_OUT_RAW));
    assert!(rig.failed().is_empty());
}

// C#: RecoveryRetryCancelledTests.RetryMoveOrCopyAsync_CancelledSingleMoveWithoutConfirmation_IsRefused
#[test]
fn retry_move_or_copy_cancelled_single_move_without_confirmation_is_refused() {
    let rig = cancelled_rig();
    let failed = JournalEntry {
        error_code: Some(CANCELLED_BY_USER.to_owned()),
        ..entry(
            "move",
            FileOperationType::Move,
            JournalState::Failed,
            CANCEL_JPG,
            Some(CANCEL_OUT_JPG),
            4,
            stamp(),
            stamp(),
            Some("x"),
        )
    };
    rig.append(&failed);

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(rig.fs.has_file(CANCEL_JPG));
    assert!(!rig.fs.has_file(CANCEL_OUT_JPG));
}

// C#: RecoveryRetryCancelledTests.RetryMoveOrCopyAsync_NonCancelledFailure_DoesNotNeedConfirmation
#[test]
fn retry_move_or_copy_non_cancelled_failure_does_not_need_confirmation() {
    let rig = cancelled_rig();
    let failed = JournalEntry {
        error_code: None,
        error: Some("disk full".to_owned()),
        ..cancelled_group_copy()
    };
    rig.append(&failed);

    let result = retry(&rig.service(), &failed);

    assert!(result.succeeded, "{}", result.message);
}

// ---------------------------------------------------------------------------------------------------------------
// RecoveryRetryGroupGapTests.cs
// ---------------------------------------------------------------------------------------------------------------

/// C# `FailedGroup(type, errorCode)`: group `grp`, Move/Copy of `a.jpg` and `a.cr2`, error "earlier failure".
fn failed_group(op_type: FileOperationType, error_code: Option<&str>) -> JournalEntry {
    JournalEntry {
        error_code: error_code.map(str::to_owned),
        group_id: Some("capture".to_owned()),
        group_members: Some(vec![
            member(JPG, Some(OUT_JPG), 4, stamp()),
            member(RAW, Some(OUT_RAW), 8, stamp()),
        ]),
        ..entry(
            "grp",
            op_type,
            JournalState::Failed,
            JPG,
            Some(OUT_JPG),
            4,
            stamp(),
            stamp(),
            Some("earlier failure"),
        )
    }
}

fn group_gap_rig() -> Rig {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    rig
}

// C#: RecoveryRetryGroupGapTests.RetryMoveOrCopyAsync_GroupMoveCancelledAfterFirstMember_FailsCancelledAndKeepsTheCompletedMember
#[test]
fn retry_move_or_copy_group_move_cancelled_after_first_member_fails_cancelled_and_keeps_the_completed_member() {
    let rig = group_gap_rig();
    let failed = failed_group(FileOperationType::Move, None);
    rig.append(&failed);
    // the user cancels while the first member is being moved
    rig.fs.add_rule(data_rule("move", Some(JPG), 1, vec![Op::Cancel]));
    let token = rig.cancel.clone();

    let result = retry_with(&rig.service(), &failed, false, &token).expect("the cancel is journaled, not raised");

    assert!(!result.succeeded);
    assert_eq!(
        result.entry.as_ref().and_then(|e| e.error_code.as_deref()),
        Some(CANCELLED_BY_USER)
    );
    assert!(!rig.fs.has_file(JPG)); // completed member stays moved: a retry does not undo
    assert!(rig.fs.has_file(OUT_JPG));
    assert!(rig.fs.has_file(RAW)); // the second member was never touched
    assert!(!rig.fs.has_file(OUT_RAW));
    let latest = rig.failed();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].error_code.as_deref(), Some(CANCELLED_BY_USER));
    assert!(rig.pending().is_empty());
}

// C#: RecoveryRetryGroupGapTests.RetryMoveOrCopyAsync_GroupCancelledMidLoopThenConfirmedAndRetried_FinishesOnlyTheRemainingMember
#[test]
fn retry_move_or_copy_group_cancelled_mid_loop_then_confirmed_and_retried_finishes_only_the_remaining_member() {
    let rig = group_gap_rig();
    let failed = failed_group(FileOperationType::Move, None);
    rig.append(&failed);
    rig.fs.add_rule(data_rule("move", Some(JPG), 1, vec![Op::Cancel]));
    let token = rig.cancel.clone();
    let service = rig.service();
    let cancelled = retry_with(&service, &failed, false, &token).expect("journaled, not raised");
    rig.fs.clear_rules();
    let cancelled_entry = cancelled.entry.expect("the cancelled Failed line");

    // Plain retry is refused (the entry says the user cancelled); the explicit confirmation finishes the rest.
    let refused = retry(&service, &cancelled_entry);
    assert_eq!(refused.message, english().core_recovery_cancelled_needs_confirm());
    let finished = retry_with(&service, &cancelled_entry, true, &CancelToken::none()).expect("not cancelled");

    assert!(finished.succeeded, "{}", finished.message);
    assert!(rig.fs.has_file(OUT_JPG));
    assert!(rig.fs.has_file(OUT_RAW));
    assert!(rig.failed().is_empty());
}

// C#: RecoveryRetryGroupGapTests.RetryMoveOrCopyAsync_GroupCopyWithOneDestinationAlreadyComplete_CopiesOnlyTheMissingMemberAndCommits
#[test]
fn retry_move_or_copy_group_copy_with_one_destination_already_complete_copies_only_the_missing_member_and_commits() {
    let rig = group_gap_rig();
    let failed = failed_group(FileOperationType::Copy, None);
    rig.append(&failed);
    rig.fs.seed_text(OUT_RAW, "raw data", stamp()); // the first run got as far as the second member before failing

    let result = retry(&rig.service(), &failed);

    // Pinned verdict: members whose destination is already in place are left alone, the missing one is copied.
    assert!(result.succeeded, "{}", result.message);
    assert!(rig.fs.has_file(OUT_JPG));
    assert_eq!(stat_of(&rig.fs, OUT_RAW).length, 8);
    assert!(rig.failed().is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// RecoveryRetryFileCheckFaultTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: RecoveryRetryFileCheckFaultTests.RetrySingle_FileCheckThrows_ReturnsFailedResult
// C3: the C# source check throws ArgumentException. The Rust `file_exists` cannot fail: the file exists and the stat
// seam fails with InvalidInput ("simulated invalid path"), which is what the pre-check catch turns into a failed result.
#[test]
fn retry_single_file_check_throws_returns_failed_result() {
    let rig = Rig::new();
    let bad = r"C:\photos\bad.jpg";
    rig.fs.seed_text(bad, "photo", ts(SEED_TIMESTAMP));
    rig.fs.add_rule(data_rule(
        "stat",
        Some(bad),
        1,
        vec![Op::Throw {
            kind: io::ErrorKind::InvalidInput,
            message: "simulated invalid path".to_owned(),
        }],
    ));
    let failed = failed_entry(
        "id1",
        FileOperationType::Move,
        bad,
        r"C:\selected\bad.jpg",
        10,
        ts(SEED_TIMESTAMP),
        ts(SEED_TIMESTAMP),
    );

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(
        result.message.contains("simulated invalid path"),
        "message was {:?}",
        result.message
    );
}

// ---------------------------------------------------------------------------------------------------------------
// CopyOwnershipFollowupsTests.cs (W2-FA-05: the group retry refusal texts per verdict). The DuplicateFinder
// cancellation test of that file is not a retry test and is not ported here (owned by the duplicate-finder unit).
// ---------------------------------------------------------------------------------------------------------------

const OWN_OUT_JPG: &str = r"C:\photos\out\a.jpg";
const OWN_OUT_RAW: &str = r"C:\photos\out\a.cr2";

/// C# `FailedMoveGroup()` of that file: no error code.
fn own_failed_move_group() -> JournalEntry {
    JournalEntry {
        group_id: Some("capture".to_owned()),
        group_members: Some(vec![
            member(JPG, Some(OWN_OUT_JPG), 4, stamp()),
            member(RAW, Some(OWN_OUT_RAW), 8, stamp()),
        ]),
        ..entry(
            "grp",
            FileOperationType::Move,
            JournalState::Failed,
            JPG,
            Some(OWN_OUT_JPG),
            4,
            stamp(),
            stamp(),
            Some("earlier failure"),
        )
    }
}

/// C# `RetryAsync(failed)`: the verdict of the recovery check and the retry's result, over one rig.
fn own_retry(rig: &Rig, failed: &JournalEntry) -> (RecoveryRetryResult, RecoveryVerdict) {
    rig.append(failed);
    let verdict = RecoveryFileCheck::new(rig.fs_dyn()).check(failed).verdict;
    let result = retry(&rig.service(), failed);
    (result, verdict)
}

// C#: CopyOwnershipFollowupsTests.RetryMoveOrCopyAsync_GroupAlreadyFullyMoved_ReportsAlreadyHandledAndSuperseded
#[test]
fn retry_move_or_copy_group_already_fully_moved_reports_already_handled_and_superseded() {
    let rig = mutation_rig();
    rig.fs.seed_text(OWN_OUT_JPG, "jpeg", stamp());
    rig.fs.seed_text(OWN_OUT_RAW, "raw data", stamp());

    let (result, verdict) = own_retry(&rig, &own_failed_move_group());

    assert_eq!(verdict, RecoveryVerdict::AlreadyDone);
    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_recovery_already_handled());
    assert!(result.superseded);
}

// C#: CopyOwnershipFollowupsTests.RetryMoveOrCopyAsync_GroupWithSourceAndDestinationBothPresent_ReportsConflictText
#[test]
fn retry_move_or_copy_group_with_source_and_destination_both_present_reports_conflict_text() {
    let rig = mutation_rig();
    rig.fs.seed_text(JPG, "jpeg", stamp());
    rig.fs.seed_text(OWN_OUT_JPG, "jpeg", stamp());
    rig.fs.seed_text(RAW, "raw data", stamp());

    let (result, verdict) = own_retry(&rig, &own_failed_move_group());

    assert_eq!(verdict, RecoveryVerdict::Conflict);
    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_recovery_conflict());
}

// C#: CopyOwnershipFollowupsTests.RetryMoveOrCopyAsync_GroupWithDestinationNoLongerTheMovedFile_ReportsDestinationChangedText
#[test]
fn retry_move_or_copy_group_with_destination_no_longer_the_moved_file_reports_destination_changed_text() {
    let rig = mutation_rig();
    rig.fs.seed_text(OWN_OUT_JPG, "jpeg but edited afterwards", stamp()); // source gone, destination has another size
    rig.fs.seed_text(RAW, "raw data", stamp());

    let (result, verdict) = own_retry(&rig, &own_failed_move_group());

    assert_eq!(verdict, RecoveryVerdict::DestinationChanged);
    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_recovery_destination_changed());
}

// C#: CopyOwnershipFollowupsTests.RetryMoveOrCopyAsync_GroupWithChangedSource_StillReportsSourceChangedText
#[test]
fn retry_move_or_copy_group_with_changed_source_still_reports_source_changed_text() {
    let rig = mutation_rig();
    rig.fs.seed_text(JPG, "jpeg edited, different size", stamp());
    rig.fs.seed_text(RAW, "raw data", stamp());

    let (result, verdict) = own_retry(&rig, &own_failed_move_group());

    assert_eq!(verdict, RecoveryVerdict::SourceChanged);
    assert_eq!(result.message, english().core_recovery_source_changed());
}

// ---------------------------------------------------------------------------------------------------------------
// RecoveryIdentityTests.cs: the three cases owned by the retry (R15 group Recycle undo, R17 single Move/Copy). The
// Reconcile and Undo cases of that file belong to W3-03 and W3-05.
// ---------------------------------------------------------------------------------------------------------------

const IDENT_JPEG: &str = r"C:\photos\a.jpg";
const IDENT_RAW: &str = r"C:\photos\a.cr2";

/// C# `UndoRecycleGroup(state, members)`: a group Recycle undo (`Undo: true`) with `Stamp`.
fn undo_recycle_group(state: JournalState, members: Vec<JournalGroupMember>) -> JournalEntry {
    let source = members[0].source.clone();
    let size = members[0].size;
    JournalEntry {
        undo: Some(true),
        group_id: Some("g".to_owned()),
        group_members: Some(members),
        ..entry(
            "undo-recycle",
            FileOperationType::Recycle,
            state,
            &source,
            None,
            size,
            stamp(),
            stamp(),
            None,
        )
    }
}

// C#: RecoveryIdentityTests.RetryGroupRecycleUndo_ForeignFileAppearsAtPendingMemberAfterThePreCheck_FailsInsteadOfCommitting
#[test]
fn retry_group_recycle_undo_foreign_file_appears_at_pending_member_after_the_pre_check_fails_instead_of_committing() {
    let rig = mutation_rig();
    let failed = undo_recycle_group(
        JournalState::Failed,
        vec![
            member(IDENT_JPEG, None, 10, stamp()),
            member(IDENT_RAW, None, 10, stamp()),
        ],
    );
    rig.append(&failed);
    // A foreign file takes the JPEG's original path right after the Prepared line (the first append of the retry).
    let fs = rig.fs.clone();
    once_on(&rig.fs, "append", move || {
        fs.seed_text(IDENT_JPEG, "fff", ts(STAMP_OTHER))
    });

    // C# `NewRetry()` passes `_bin`: without a bin the Delete-undo retry is refused before the Prepared append.
    let result = retry(&mutation_service(&rig), &failed);

    assert!(!result.succeeded);
    assert_ne!(result.entry.as_ref().map(|e| e.state), Some(JournalState::Committed));
    assert_eq!(stat_of(&rig.fs, IDENT_JPEG).length, 3); // the foreign file is untouched
}

// C#: RecoveryIdentityTests.RetryGroupRecycleUndo_BinReportsRestoreButAForeignFileSitsAtTheMember_Fails
#[test]
fn retry_group_recycle_undo_bin_reports_restore_but_a_foreign_file_sits_at_the_member_fails() {
    let rig = mutation_rig();
    let failed = undo_recycle_group(JournalState::Failed, vec![member(IDENT_JPEG, None, 10, stamp())]);
    rig.append(&failed);
    // The bin answers "restored" and leaves a foreign 3-byte file at the member's path.
    let fs = rig.fs.clone();
    rig.fs.on_every("restore", move |_fs, call| {
        if let Some(path) = call.path {
            fs.seed_text(&path.to_string_lossy(), "fff", stamp());
        }
        Ok(Next::Proceed)
    });
    let bin = rig.bin(BinConfig {
        restore: RestoreMode::OkNoFile,
        ..BinConfig::default()
    });

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(!result.succeeded);
    assert_ne!(result.entry.as_ref().map(|e| e.state), Some(JournalState::Committed));
}

// C#: RecoveryIdentityTests.RetryMoveOrCopy_SourceReplacedAfterThePreCheck_IsRefusedAndNothingIsMutated (Theory: Move, Copy)
#[test]
fn retry_move_or_copy_source_replaced_after_the_pre_check_is_refused_and_nothing_is_mutated_move() {
    retry_move_or_copy_source_replaced_after_the_pre_check_is_refused_and_nothing_is_mutated(FileOperationType::Move);
}

// C#: ...RetryMoveOrCopy_SourceReplacedAfterThePreCheck_IsRefusedAndNothingIsMutated (Copy)
#[test]
fn retry_move_or_copy_source_replaced_after_the_pre_check_is_refused_and_nothing_is_mutated_copy() {
    retry_move_or_copy_source_replaced_after_the_pre_check_is_refused_and_nothing_is_mutated(FileOperationType::Copy);
}

fn retry_move_or_copy_source_replaced_after_the_pre_check_is_refused_and_nothing_is_mutated(
    op_type: FileOperationType,
) {
    const DESTINATION: &str = r"D:\sorted\a.jpg";
    let rig = mutation_rig();
    rig.fs.seed_text(IDENT_JPEG, "jjjjj", stamp());
    let failed = entry(
        "retry-op",
        op_type,
        JournalState::Failed,
        IDENT_JPEG,
        Some(DESTINATION),
        5,
        stamp(),
        stamp(),
        Some("Previous error"),
    );
    rig.append(&failed);
    // A different photo takes over the path right after the Prepared line.
    let fs = rig.fs.clone();
    once_on(&rig.fs, "append", move || {
        fs.seed_text(IDENT_JPEG, "rrrrrrrrr", ts(STAMP_OTHER))
    });

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(!rig.fs.has_file(DESTINATION)); // the replacement was neither moved nor copied under the old operation
    assert_eq!(stat_of(&rig.fs, IDENT_JPEG).length, 9);
    assert!(rig.committed().iter().all(|e| e.id != "retry-op"));
}

/// C# `OtherStamp` of that file: `Stamp` plus three hours.
const STAMP_OTHER: i64 = STAMP + 3 * 3600 * TICKS_PER_SECOND_RR;

// ---------------------------------------------------------------------------------------------------------------
// RecoveryRecycleGroupTests.cs (the retry part: a capture-group Delete that failed part-way). The Check_* tests of
// that file are the recovery check's (W3-07) and are not ported here.
// ---------------------------------------------------------------------------------------------------------------

const DEL_JPG: &str = r"C:\photos\a.jpg";
const DEL_RAW: &str = r"C:\photos\a.cr2";

/// C# `Member(path, size, permanent)` of a Delete group: no destination, `Stamp`.
fn delete_member(path: &str, size: i64, permanent: bool) -> JournalGroupMember {
    JournalGroupMember {
        permanent,
        ..member(path, None, size, stamp())
    }
}

/// C# `FailedGroup(members)` of a Delete: error "x", `SourceStillExistsAfterRecovery`, group `capture-a`.
fn failed_delete_group(members: Vec<JournalGroupMember>) -> JournalEntry {
    let source = members[0].source.clone();
    let size = members[0].size;
    let permanent = members.iter().any(|m| m.permanent).then_some(true);
    JournalEntry {
        error_code: Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY.to_owned()),
        permanent,
        group_id: Some("capture-a".to_owned()),
        group_members: Some(members),
        ..entry(
            "delete-group",
            FileOperationType::Recycle,
            JournalState::Failed,
            &source,
            None,
            size,
            stamp(),
            stamp(),
            Some("x"),
        )
    }
}

/// The bin calls of one kind, in call order (C# `_bin.Recycled` / `PermanentlyDeleted`), by their log prefix.
fn bin_calls(rig: &Rig, prefix: &str) -> Vec<String> {
    rig.fs
        .ops()
        .iter()
        .filter_map(|o| o.strip_prefix(prefix).map(str::to_owned))
        .collect()
}

// C#: RecoveryRecycleGroupTests.Retry_RecycledMemberStillOnDisk_JournalsTheCodedErrorSoRecoveryCanLocalizeIt
#[test]
fn retry_recycled_member_still_on_disk_journals_the_coded_error_so_recovery_can_localize_it() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp());
    let bin = rig.bin(BinConfig {
        recycle_noop: true, // the bin call "succeeds" but the file is still there
        ..BinConfig::default()
    });
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, false),
    ]);
    rig.append(&failed);

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(!result.succeeded);
    assert_eq!(
        result.entry.as_ref().and_then(|e| e.error_code.as_deref()),
        Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY) // was null: a bare IOException carried only the identifier text
    );
}

// C#: RecoveryRecycleGroupTests.Retry_PartlyRecycledGroup_RecyclesOnlyTheMembersStillPresentAndCommits
#[test]
fn retry_partly_recycled_group_recycles_only_the_members_still_present_and_commits() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp());
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, false),
    ]);
    rig.append(&failed);
    let bin = rig.bin(BinConfig::default());

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(result.succeeded, "{}", result.message);
    assert_eq!(bin_calls(&rig, "bin.recycle "), vec![DEL_RAW.to_owned()]);
    assert!(bin_calls(&rig, "bin.delete-permanently ").is_empty());
    assert_eq!(result.entry.as_ref().map(|e| e.state), Some(JournalState::Committed));
    assert!(rig.failed().is_empty());
    assert!(rig.pending().is_empty());
}

// C#: RecoveryRecycleGroupTests.Retry_GroupRecycle_RunsTheBinCallsOffTheCallerThread
// NOT PORTED (R0003): the call is blocking and the presenter owns the thread. The twin below asserts the Rust contract:
// every bin call runs on the CALLING thread.
#[test]
fn retry_group_recycle_runs_the_bin_calls_on_the_calling_thread() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_JPG, "jjjjjjjjjj", stamp());
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp());
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, false),
    ]);
    rig.append(&failed);
    let threads = Arc::new(Mutex::new(Vec::new()));
    let slot = threads.clone();
    rig.fs.on_every("recycle", move |_fs, _call| {
        slot.lock().expect("lock").push(std::thread::current().id());
        Ok(Next::Proceed)
    });
    let bin = rig.bin(BinConfig::default());

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(result.succeeded, "{}", result.message);
    let threads = threads.lock().expect("lock");
    assert_eq!(threads.len(), 2);
    assert!(threads.iter().all(|id| *id == std::thread::current().id()));
}

// C#: RecoveryRecycleGroupTests.Retry_BinFitsEachMemberButNotTheirSum_IsRefusedAndNothingIsRecycled
#[test]
fn retry_bin_fits_each_member_but_not_their_sum_is_refused_and_nothing_is_recycled() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_JPG, "jjjjjjjjjj", stamp());
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp());
    let bin = rig.bin(BinConfig {
        capacity: Some(15), // each 10-byte file fits, both together (20) do not
        ..BinConfig::default()
    });
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, false),
    ]);
    rig.append(&failed);

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_recycle_bin_cannot_hold("a.jpg"));
    assert!(bin_calls(&rig, "bin.recycle ").is_empty());
    assert!(rig.fs.has_file(DEL_JPG));
    assert!(rig.fs.has_file(DEL_RAW));
    assert_eq!(rig.failed().len(), 1); // still the same retryable item
    assert_eq!(rig.failed()[0].state, JournalState::Failed);
}

// C#: RecoveryRecycleGroupTests.Retry_BinFitsTheMembersStillPendingEvenIfNotTheWholeGroup_Succeeds
#[test]
fn retry_bin_fits_the_members_still_pending_even_if_not_the_whole_group_succeeds() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp()); // the JPEG was already recycled by the first attempt
    let bin = rig.bin(BinConfig {
        capacity: Some(15),
        ..BinConfig::default()
    });
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, false),
    ]);
    rig.append(&failed);

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(result.succeeded, "{}", result.message);
    assert_eq!(bin_calls(&rig, "bin.recycle "), vec![DEL_RAW.to_owned()]);
}

// C#: RecoveryRecycleGroupTests.Retry_PresentMemberChangedSinceJournal_IsRefusedAndNothingIsRecycled
#[test]
fn retry_present_member_changed_since_journal_is_refused_and_nothing_is_recycled() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrrr", stamp()); // 11 bytes: the journal says 10
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, false),
    ]);
    rig.append(&failed);

    let result = retry(&rig.service().with_recycle_bin(rig.bin(BinConfig::default())), &failed);

    assert!(!result.succeeded);
    assert!(bin_calls(&rig, "bin.recycle ").is_empty());
    assert!(bin_calls(&rig, "bin.delete-permanently ").is_empty());
}

// C#: RecoveryRecycleGroupTests.Retry_MemberOnDriveWithoutRecycleBinNotJournaledPermanent_IsRefusedAndNeverDeleted
#[test]
fn retry_member_on_drive_without_recycle_bin_not_journaled_permanent_is_refused_and_never_deleted() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_JPG, "jjjjjjjjjj", stamp());
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp());
    let bin = rig.bin(BinConfig {
        no_bin: vec![DEL_RAW.to_owned()],
        ..BinConfig::default()
    });
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, false),
    ]);
    rig.append(&failed);

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(!result.succeeded);
    assert!(bin_calls(&rig, "bin.recycle ").is_empty());
    assert!(bin_calls(&rig, "bin.delete-permanently ").is_empty());
    assert!(rig.fs.has_file(DEL_JPG));
    assert!(rig.fs.has_file(DEL_RAW));
}

// C#: RecoveryRecycleGroupTests.Retry_MemberJournaledPermanentOnDriveWithoutBin_IsDeletedPermanentlyOnlyThatMember
#[test]
fn retry_member_journaled_permanent_on_drive_without_bin_is_deleted_permanently_only_that_member() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_JPG, "jjjjjjjjjj", stamp());
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp());
    let bin = rig.bin(BinConfig {
        no_bin: vec![DEL_RAW.to_owned()],
        ..BinConfig::default()
    });
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, true),
    ]);
    rig.append(&failed);
    let service = rig
        .service()
        .with_recycle_bin(bin)
        .with_allow_permanent_delete(Rig::allow(true));

    let result = retry(&service, &failed);

    assert!(result.succeeded, "{}", result.message);
    assert_eq!(bin_calls(&rig, "bin.recycle "), vec![DEL_JPG.to_owned()]);
    assert_eq!(bin_calls(&rig, "bin.delete-permanently "), vec![DEL_RAW.to_owned()]);
}

// C#: RecoveryRecycleGroupTests.Retry_MemberJournaledPermanentButSettingNowOff_IsRefusedBeforeAnyMutation (Theory: false, null)
#[test]
fn retry_member_journaled_permanent_but_setting_now_off_is_refused_before_any_mutation_setting_false() {
    retry_member_journaled_permanent_but_setting_now_off(Some(false));
}

// C#: ...Retry_MemberJournaledPermanentButSettingNowOff_IsRefusedBeforeAnyMutation (null = parameter not passed)
#[test]
fn retry_member_journaled_permanent_but_setting_now_off_is_refused_before_any_mutation_setting_absent() {
    retry_member_journaled_permanent_but_setting_now_off(None);
}

fn retry_member_journaled_permanent_but_setting_now_off(setting: Option<bool>) {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_JPG, "jjjjjjjjjj", stamp());
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp());
    let bin = rig.bin(BinConfig {
        no_bin: vec![DEL_RAW.to_owned()],
        ..BinConfig::default()
    });
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, true),
    ]);
    rig.append(&failed);
    let mut service = rig.service().with_recycle_bin(bin);
    if let Some(value) = setting {
        service = service.with_allow_permanent_delete(Rig::allow(value));
    }

    let result = retry(&service, &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_recycle_unsupported_drive("a.cr2"));
    assert!(bin_calls(&rig, "bin.recycle ").is_empty());
    assert!(bin_calls(&rig, "bin.delete-permanently ").is_empty());
    assert!(rig.fs.has_file(DEL_JPG));
    assert!(rig.fs.has_file(DEL_RAW));
    assert_eq!(rig.failed().len(), 1); // still the one Failed line, nothing new journaled
}

// C#: RecoveryRecycleGroupTests.Retry_WithoutRecycleBinService_IsRefusedAndNothingChanges
#[test]
fn retry_without_recycle_bin_service_is_refused_and_nothing_changes() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(DEL_RAW, "rrrrrrrrrr", stamp());
    let failed = failed_delete_group(vec![
        delete_member(DEL_JPG, 10, false),
        delete_member(DEL_RAW, 10, false),
    ]);
    rig.append(&failed);

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(rig.fs.has_file(DEL_RAW));
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroupActionServiceTests.cs (the RecoveryRetry_* tests; the ExecuteGroupAsync_* ones are W3-04's)
// ---------------------------------------------------------------------------------------------------------------

/// C# `Stamp.AddSeconds(-1)`: the older copy of a journal line written just before the real one.
fn one_second_earlier(e: &JournalEntry) -> JournalEntry {
    JournalEntry {
        timestamp_utc: JournalDateTime::utc(ts(STAMP - TICKS_PER_SECOND_RR)),
        ..e.clone()
    }
}

// C#: CaptureGroupActionServiceTests.RecoveryRetry_GroupSkipsCompletedMembersAndRetriesRemainingMembers
#[test]
fn recovery_retry_group_skips_completed_members_and_retries_remaining_members() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    let selected_jpg = r"C:\photos\selected\a.jpg";
    let selected_raw = r"C:\photos\selected\a.cr2";
    rig.fs.seed_text(JPG, "jpeg", stamp());
    rig.fs.seed_text(RAW, "raw data", stamp());
    let members = vec![
        member(JPG, Some(selected_jpg), 4, stamp()),
        member(RAW, Some(selected_raw), 8, stamp()),
    ];
    let entry_value = JournalEntry {
        group_id: Some("group".to_owned()),
        group_members: Some(members),
        ..entry(
            "group",
            FileOperationType::Move,
            JournalState::Failed,
            JPG,
            Some(selected_jpg),
            4,
            stamp(),
            stamp(),
            None,
        )
    };
    // The first member was already moved by the first attempt.
    rig.fs.seed_dir(r"C:\photos\selected");
    rig.fs.seed_text(selected_jpg, "jpeg", stamp());
    rig.fs.remove(JPG);
    rig.append(&one_second_earlier(&entry_value));
    rig.append(&entry_value);

    let result = retry(&rig.service(), &entry_value);

    assert!(result.succeeded, "{}", result.message);
    assert!(!rig.fs.has_file(RAW));
    assert!(rig.fs.has_file(selected_raw));
    assert!(rig.fs.has_file(selected_jpg));
    let committed = rig.committed();
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].group_members.as_ref().map(Vec::len), Some(2));
}

// C#: CaptureGroupActionServiceTests.RecoveryRetry_GroupUndoMoveReversesManifestAndSkipsRestoredMember
#[test]
fn recovery_retry_group_undo_move_reverses_manifest_and_skips_restored_member() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    let selected_jpg = r"C:\photos\selected\a.jpg";
    let selected_raw = r"C:\photos\selected\a.cr2";
    rig.fs.seed_text(selected_jpg, "jpeg", stamp());
    rig.fs.seed_text(selected_raw, "raw data", stamp());
    let members = vec![
        member(selected_jpg, Some(JPG), 4, stamp()),
        member(selected_raw, Some(RAW), 8, stamp()),
    ];
    let failed = JournalEntry {
        undo: Some(true),
        group_id: Some("undo".to_owned()),
        group_members: Some(members),
        ..entry(
            "undo-group",
            FileOperationType::Move,
            JournalState::Failed,
            selected_jpg,
            Some(JPG),
            4,
            stamp(),
            stamp(),
            None,
        )
    };
    rig.append(&one_second_earlier(&failed));
    rig.append(&failed);
    rig.fs.seed_dir(r"C:\photos");

    let result = retry(&rig.service(), &failed);

    assert!(result.succeeded, "{}", result.message);
    assert!(!rig.fs.has_file(selected_jpg));
    assert!(rig.fs.has_file(JPG));
    assert!(!rig.fs.has_file(selected_raw));
    assert!(rig.fs.has_file(RAW));
    let committed = rig.committed();
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].undo, Some(true));
}

// C#: CaptureGroupActionServiceTests.RecoveryRetry_GroupUndoRecycleUsesRestoreInsteadOfRecyclingAgain
#[test]
fn recovery_retry_group_undo_recycle_uses_restore_instead_of_recycling_again() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_dir(r"C:\photos");
    let bin = rig.bin(BinConfig {
        restore: RestoreMode::Synthetic {
            recreate: true,
            answer: true,
        },
        ..BinConfig::default()
    });
    let member_value = member(JPG, None, 4, stamp());
    let failed = JournalEntry {
        undo: Some(true),
        group_id: Some("undo".to_owned()),
        group_members: Some(vec![member_value]),
        ..entry(
            "undo-recycle",
            FileOperationType::Recycle,
            JournalState::Failed,
            JPG,
            None,
            4,
            stamp(),
            stamp(),
            None,
        )
    };
    rig.append(&one_second_earlier(&failed));
    rig.append(&failed);

    let result = retry(&rig.service().with_recycle_bin(bin), &failed);

    assert!(result.succeeded, "{}", result.message);
    assert_eq!(bin_calls(&rig, "bin.restore ").len(), 1);
    assert_eq!(bin_calls(&rig, "bin.recycle ").len(), 0);
    assert!(rig.fs.has_file(JPG));
}

// C#: CaptureGroupActionServiceTests.RecoveryRetry_GroupUndoMovePreflightsAllPendingTargetsBeforeMutation
#[test]
fn recovery_retry_group_undo_move_preflights_all_pending_targets_before_mutation() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    let selected_jpg = r"C:\photos\selected\a.jpg";
    let selected_raw = r"C:\photos\selected\a.cr2";
    rig.fs.seed_text(selected_jpg, "jpeg", stamp());
    rig.fs.seed_text(selected_raw, "raw data", stamp());
    rig.fs.seed_text(RAW, "new unrelated file", stamp());
    let members = vec![
        member(selected_jpg, Some(JPG), 4, stamp()),
        member(selected_raw, Some(RAW), 8, stamp()),
    ];
    let failed = JournalEntry {
        undo: Some(true),
        group_id: Some("undo".to_owned()),
        group_members: Some(members.clone()),
        ..entry(
            "undo-conflict",
            FileOperationType::Move,
            JournalState::Failed,
            selected_jpg,
            Some(JPG),
            4,
            stamp(),
            stamp(),
            None,
        )
    };
    rig.append(&one_second_earlier(&failed));
    rig.append(&failed);

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(rig.fs.has_file(selected_jpg));
    assert!(rig.fs.has_file(selected_raw));
    assert!(!rig.fs.ops().iter().any(|o| o.starts_with("move ")));
}

// C#: CaptureGroupActionServiceTests.RecoveryRetry_GroupRejectsStaleFailedSnapshotBeforeMutation
#[test]
fn recovery_retry_group_rejects_stale_failed_snapshot_before_mutation() {
    let rig = Rig::with_clocks(STAMP + 60 * TICKS_PER_SECOND_RR, STAMP + 120 * TICKS_PER_SECOND_RR);
    rig.fs.seed_text(JPG, "jpeg", stamp());
    let member_value = member(JPG, Some(r"C:\photos\selected\a.jpg"), 4, stamp());
    let failed = JournalEntry {
        group_id: Some("stale-group".to_owned()),
        group_members: Some(vec![member_value.clone()]),
        ..entry(
            "stale-group",
            FileOperationType::Move,
            JournalState::Failed,
            &member_value.source,
            member_value.destination.as_deref(),
            member_value.size,
            member_value.last_write_utc.value,
            stamp(),
            None,
        )
    };
    rig.append(&one_second_earlier(&failed));
    rig.append(&failed);
    rig.append(&JournalEntry {
        state: JournalState::Dismissed,
        timestamp_utc: JournalDateTime::utc(ts(STAMP + TICKS_PER_SECOND_RR)),
        ..failed.clone()
    });

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(result.superseded);
    assert!(rig.fs.has_file(JPG));
    assert!(!rig.fs.has_file(r"C:\photos\selected\a.jpg"));
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionMutationAuditBTests.cs, FileActionSafetyEdgeTests.cs, CopyForeignDestinationTests.cs,
// CopyRollbackIdentityTests.cs, LiveOperationMarkerTests.cs, JournalErrorCodeTests.cs (the retry cases only)
// ---------------------------------------------------------------------------------------------------------------

/// C# `Start` of `LiveOperationMarkerTests` / `Stamp` of `FileActionSafetyEdgeTests` (2026-09-26T08:00Z).
const MARKER_START: i64 = 639_260_064_000_000_000;
/// C# `ForeignStamp` of `CopyRollbackIdentityTests` (2001-01-01T00:00Z).
const FOREIGN_STAMP: i64 = 631_139_040_000_000_000;

// C#: FileActionMutationAuditBTests.RetryGroup_PermanentMemberWhoseDriveGainedARecycleBinAfterThePreCheck_IsNotDeleted
#[test]
fn retry_group_permanent_member_whose_drive_gained_a_recycle_bin_after_the_pre_check_is_not_deleted() {
    let rig = mutation_rig();
    seed_jpg_and_raw(&rig);
    let bin = rig.bin(BinConfig {
        no_bin: vec![RAW.to_owned()],
        ..BinConfig::default()
    });
    let failed = failed_recycle_group(
        None,
        vec![
            member(JPG, None, 10, stamp()),
            JournalGroupMember {
                permanent: true,
                ..member(RAW, None, 10, stamp())
            },
        ],
    );
    rig.append(&failed);
    // The pre-checks are done once Prepared is appended: the drive "gets" a Recycle Bin right then.
    let hook_bin = bin.clone();
    once_on(&rig.fs, "append", move || {
        hook_bin.update(|config| config.no_bin.clear())
    });
    let service = rig
        .service()
        .with_recycle_bin(bin)
        .with_allow_permanent_delete(Rig::allow(true));

    let result = retry(&service, &failed);

    // The journaled "permanent" verdict no longer holds, so nothing is permanently deleted.
    assert!(!result.succeeded);
    assert!(bin_calls(&rig, "bin.delete-permanently ").is_empty());
    assert!(rig.fs.has_file(RAW));
}

/// C# `Move(id, from, to, state, undo)` of `FileActionSafetyEdgeTests`: a 5-byte Move stamped `MARKER_START`.
fn safety_move(id: &str, from: &str, to: &str, state: JournalState, undo: Option<bool>) -> JournalEntry {
    JournalEntry {
        undo,
        ..entry(
            id,
            FileOperationType::Move,
            state,
            from,
            Some(to),
            5,
            ts(MARKER_START),
            ts(MARKER_START),
            None,
        )
    }
}

// C#: FileActionSafetyEdgeTests.RetryTwice_LoserDoesNotOverrideCommit
#[test]
fn retry_twice_loser_does_not_override_commit() {
    let rig = Rig::with_clocks(MARKER_START, MARKER_START);
    rig.fs.seed_text(r"C:\photos\a.jpg", "12345", ts(MARKER_START));
    let failed = safety_move(
        "r1",
        r"C:\photos\a.jpg",
        r"C:\photos\sel\a.jpg",
        JournalState::Failed,
        None,
    );
    rig.append(&failed);
    let service = rig.service();

    let first = retry(&service, &failed);
    let second = retry(&service, &failed); // stale window: still holds the Failed entry

    assert!(first.succeeded);
    assert!(!second.succeeded);
    assert!(rig.failed().is_empty());
    assert!(rig.pending().is_empty());
}

// C#: FileActionSafetyEdgeTests.Retry_SourceChanged_IsRefused (Theory: length 6 same time; length 5 one second off)
#[test]
fn retry_source_changed_is_refused_length_differs() {
    retry_source_changed_is_refused(6, 0);
}

// C#: ...Retry_SourceChanged_IsRefused (same length, last write one second off)
#[test]
fn retry_source_changed_is_refused_last_write_differs() {
    retry_source_changed_is_refused(5, 1);
}

fn retry_source_changed_is_refused(current_length: usize, seconds_off: i64) {
    let rig = Rig::with_clocks(MARKER_START, MARKER_START);
    rig.fs.seed_text(
        r"C:\photos\a.jpg",
        &"x".repeat(current_length),
        ts(MARKER_START + seconds_off * TICKS_PER_SECOND_RR),
    );
    // journaled Size 5, MARKER_START
    let failed = safety_move(
        "chg",
        r"C:\photos\a.jpg",
        r"C:\photos\sel\a.jpg",
        JournalState::Failed,
        None,
    );
    rig.append(&failed);

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(result.message, english().core_recovery_source_changed());
    assert!(rig.fs.has_file(r"C:\photos\a.jpg"));
    assert!(!rig.fs.has_file(r"C:\photos\sel\a.jpg"));
}

// C#: CopyForeignDestinationTests.RecoveryCopy_SourceVanishesAndForeignDestinationArrives_ForeignFileSurvives
#[test]
fn recovery_copy_source_vanishes_and_foreign_destination_arrives_foreign_file_survives() {
    const FOREIGN: &str = "foreign";
    let rig = Rig::with_clocks(MARKER_START, MARKER_START);
    let source = r"C:\photos\a.jpg";
    let destination = r"D:\sel\a.jpg";
    let (size, last_write) = seed(&rig, source, "long source photo");
    let failed = failed_entry(
        "r01",
        FileOperationType::Copy,
        source,
        destination,
        size,
        last_write,
        ts(MARKER_START),
    );
    rig.append(&failed);
    // The copy finds a foreign file at the destination and the source has gone.
    rig.fs.on_every("copy", |fs, call| {
        if let Some(to) = call.to {
            fs.seed_text(&to.to_string_lossy(), FOREIGN, ts(MARKER_START));
        }
        if let Some(from) = call.path {
            fs.remove(&from.to_string_lossy());
        }
        Ok(Next::Proceed)
    });

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert!(rig.fs.has_file(destination));
    assert_eq!(rig.fs.text(destination), FOREIGN);
}

// C#: CopyRollbackIdentityTests.RecoveryCopy_ThrowsAfterProofAndForeignShorterFileReplacesDestination_ForeignFileSurvives
#[test]
fn recovery_copy_throws_after_proof_and_foreign_shorter_file_replaces_destination_foreign_file_survives() {
    let rig = Rig::with_clocks(MARKER_START, MARKER_START);
    let destination = r"D:\sel\a.jpg";
    let (size, last_write) = seed(&rig, JPG, "long source photo");
    let failed = failed_entry(
        "r01a",
        FileOperationType::Copy,
        JPG,
        destination,
        size,
        last_write,
        ts(MARKER_START),
    );
    rig.append(&failed);
    // The copy completed (proof raised), then the call fails and someone else's shorter file is at the destination.
    rig.fs.on_every("copy-after", |fs, call| {
        if let Some(to) = call.to {
            fs.seed_text(&to.to_string_lossy(), "foreign", ts(FOREIGN_STAMP));
        }
        Err(io::Error::other("simulated"))
    });

    let result = retry(&rig.service(), &failed);

    assert!(!result.succeeded);
    assert_eq!(rig.fs.text(destination), "foreign");
}

// C#: LiveOperationMarkerTests.RecoveryRetry_MarkerCoversPreparedToOutcome (Theory: copyFails false / true)
#[test]
fn recovery_retry_marker_covers_prepared_to_outcome_copy_succeeds() {
    recovery_retry_marker_covers_prepared_to_outcome(false);
}

// C#: ...RecoveryRetry_MarkerCoversPreparedToOutcome (copyFails = true)
#[test]
fn recovery_retry_marker_covers_prepared_to_outcome_copy_fails() {
    recovery_retry_marker_covers_prepared_to_outcome(true);
}

fn recovery_retry_marker_covers_prepared_to_outcome(copy_fails: bool) {
    let rig = Rig::with_clocks(MARKER_START, MARKER_START);
    let write_time = ts(MARKER_START - 86_400 * TICKS_PER_SECOND_RR);
    let source = r"C:\photos\a.jpg";
    let destination = r"C:\photos\sel\a.jpg";
    rig.fs.seed_text(source, "12345", write_time);
    let failed = JournalEntry {
        error_code: Some(PENDING_UNCONFIRMED.to_owned()),
        ..entry(
            "retry1",
            FileOperationType::Copy,
            JournalState::Failed,
            source,
            Some(destination),
            5,
            write_time,
            ts(MARKER_START - 300 * TICKS_PER_SECOND_RR),
            Some("x"),
        )
    };
    let registry: Arc<dyn LiveOperationRegistry> = rig.journal.live_operations().clone();
    let journal = rig.journal.clone();
    let during: Arc<Mutex<Option<(String, bool)>>> = Arc::new(Mutex::new(None));
    let during_slot = during.clone();
    let copy_registry = registry.clone();
    rig.fs.on_every("copy", move |_fs, _call| {
        let pending = journal.read_pending_and_failed_operations().expect("reads back");
        let prepared = pending
            .iter()
            .find(|e| e.state == JournalState::Prepared)
            .expect("the Prepared line");
        *during_slot.lock().expect("lock") = Some((prepared.id.clone(), copy_registry.is_live(&prepared.id)));
        if copy_fails {
            Err(io::Error::other("network drive went away"))
        } else {
            Ok(Next::Proceed)
        }
    });
    let live_at_append: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
    let append_slot = live_at_append.clone();
    let append_registry = registry.clone();
    rig.fs.on_every("append", move |_fs, _call| {
        append_slot
            .lock()
            .expect("lock")
            .push(append_registry.is_live("retry1"));
        Ok(Next::Proceed)
    });

    let result = retry(&rig.service(), &failed);

    assert_eq!(result.succeeded, !copy_fails, "{}", result.message);
    let (id, live) = during.lock().expect("lock").take().expect("the copy ran");
    assert_eq!(id, "retry1");
    assert!(live);
    assert!(!registry.is_live("retry1"));
    let lines = rig.fs.journal_lines();
    assert_eq!(lines.len(), 2);
    let live_at_append = live_at_append.lock().expect("lock").clone();
    assert_eq!(live_at_append, vec![true, true]); // every journal line was appended under the marker
}

// C#: JournalErrorCodeTests.CoreMessages_English (its retry assertion only)
#[test]
fn core_messages_english_retry_of_a_recycle_entry_is_only_move_copy() {
    let rig = Rig::with_clocks(CSHARP_CLOCK_TICKS, CSHARP_CLOCK_TICKS);
    rig.fs.seed_text(SOURCE, "x", ts(SEED_TIMESTAMP));
    let recycle = entry(
        "r",
        FileOperationType::Recycle,
        JournalState::Failed,
        SOURCE,
        None,
        1,
        ts(SEED_TIMESTAMP),
        ts(SEED_TIMESTAMP),
        None,
    );

    let result = retry(&rig.service(), &recycle);

    assert_eq!(
        result.message,
        "Only Move/Copy can be retried; Recycle Bin operations are not retried automatically."
    );
}

// ---------------------------------------------------------------------------------------------------------------
// The English catalog against assets/languages/en.json (the key of each text is in its trait doc line)
// ---------------------------------------------------------------------------------------------------------------

/// Every `EnglishRecoveryRetryTexts` string equals the `en.json` entry of its key (`{name}` placeholders filled alike).
#[test]
fn english_recovery_retry_texts_match_en_json() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let text =
        std::fs::read_to_string(root.join("assets").join("languages").join("en.json")).expect("en.json is readable");
    let catalog: serde_json::Value = serde_json::from_str(&text).expect("en.json parses");
    let lookup = |key: &str| {
        catalog[key]
            .as_str()
            .unwrap_or_else(|| panic!("en.json has no string for {key}"))
            .to_owned()
    };
    let t = EnglishRecoveryRetryTexts;
    let expected: Vec<(&str, String)> = vec![
        ("core.recovery.busy", t.core_recovery_busy()),
        (
            "core.recovery.cancelledNeedsConfirm",
            t.core_recovery_cancelled_needs_confirm(),
        ),
        ("core.recovery.onlyMoveCopy", t.core_recovery_only_move_copy()),
        ("core.recovery.noDestination", t.core_recovery_no_destination()),
        ("core.recovery.sourceMissing", t.core_recovery_source_missing()),
        ("core.recovery.sourceChanged", t.core_recovery_source_changed()),
        ("core.recovery.destinationExists", t.core_recovery_destination_exists()),
        ("core.recovery.conflict", t.core_recovery_conflict()),
        (
            "core.recovery.destinationChanged",
            t.core_recovery_destination_changed(),
        ),
        ("core.recovery.alreadyHandled", t.core_recovery_already_handled()),
        ("core.recovery.succeeded", t.core_recovery_succeeded()),
        (
            "core.recovery.succeededJournalFailed",
            t.core_recovery_succeeded_journal_failed(),
        ),
        (
            "core.recovery.completedFailureNotJournaled",
            t.core_recovery_completed_failure_not_journaled(),
        ),
        (
            "core.recycle.unsupportedDrive",
            t.core_recycle_unsupported_drive("{name}"),
        ),
        ("core.recycle.binCannotHold", t.core_recycle_bin_cannot_hold("{name}")),
        (
            "core.undo.recycleTargetExists",
            t.core_undo_recycle_target_exists("{fileName}"),
        ),
        (
            "core.undo.recycleRestoreFailed",
            t.core_undo_recycle_restore_failed("{fileName}"),
        ),
    ];
    // The code is called with the placeholder itself, so the template text (with `{name}` / `{fileName}`) is compared.
    let mut mismatches = Vec::new();
    for (key, ours) in &expected {
        let catalog_text = lookup(key);
        if &catalog_text != ours {
            mismatches.push(format!("{key}: catalog {catalog_text:?} vs code {ours:?}"));
        }
    }
    assert!(mismatches.is_empty(), "catalog mismatches:\n{}", mismatches.join("\n"));
}
