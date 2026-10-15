//! Tests for `pr_core::file_actions::undo` (`UndoService`: one-level session Undo of a single Move, a single Recycle, a capture-group
//! Move or Recycle, and the settling of a part-way failed group Delete). Port of the C# tests of `tests/PhotoReview.Core.Tests/
//! FileActions/` at baseline d92d6aae: `UndoServiceTests.cs`, `UndoServiceGroupTests.cs`, `UndoServiceMutationGapTests.cs`,
//! `UndoServiceSafetyTests.cs`, and the undo tests of `FileActionMutationAuditBTests.cs`, `RecoveryIdentityTests.cs`,
//! `FileActionSafetyEdgeTests.cs`, `JournalErrorCodeTests.cs`, `LiveOperationMarkerTests.cs`, `PermanentDeleteTests.cs`.
//!
//! Every test carries a `// C#: <Class>.<Method>` comment. Vietnamese message asserts of the C# tests compare with the English
//! text of the same key ([`EnglishUndoTexts`]). Red until the W3-05 bodies land: each test that reaches `UndoService` panics with
//! `todo!()`. Nothing here touches the real Recycle Bin: the bin is [`undo_support::ScriptedBin`] over the in-memory disk.
//!
//! Documented deviations from the C# tests (they are ports, not skips):
//! * `DIVERGENCE (R0003)`: the three C# thread tests (`*OffTheCallerThread*`) keep the order and restored-paths asserts and drop
//!   the thread-id asserts (the Rust API is blocking; the caller already runs off the UI thread).
//! * `DIVERGENCE (R0003)`: `UndoLastAsync_DoesNotResumeOnCapturedContext` keeps only its functional part (no SynchronizationContext
//!   in Rust): a Move undo then a Recycle undo both succeed. `// R0003: blocking API, nothing to resume`.
//! * `DIVERGENCE`: any `io::Error` is swallowed by the settle path; there is no "unexpected exception" class in Rust.
//! * `TODO(lead)`: the last three lines of `UndoLastAsync_FailedGroupDeleteWhereTheRecycledMemberWasRestoredAndEdited_*`
//!   (`RecoveryRetryService`, W3-06) are not ported yet: W3-06 is not merged. Marked `TODO(lead)` in that test.
//! * `DIVERGENCE (Q1, P-13)`: the bin's shell error is an `io::Error` (`RecycleBin::try_restore` returns `io::Result<bool>`), so
//!   the C# throwing-bin tests port as they are.

mod undo_support;

use std::collections::HashSet;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{FileSystem, InProcessLiveOperationRegistry, LiveMarker, LiveOperationRegistry};
use pr_core::file_actions::errors::{MOVE_SOURCE_NOT_REMOVED, PENDING_UNCONFIRMED};
use pr_core::file_actions::journal_line::{self, JournalEntry, JournalGroupMember};
use pr_core::file_actions::types::{CaptureGroupActionResult, CaptureGroupMemberResult, FileActionResult, UndoResult};
use pr_core::file_actions::undo::{EnglishUndoTexts, FileActionGate, MoveOverride, UndoService, UndoTexts};
use pr_core::file_actions::{
    journal::OperationJournal,
    recovery_check::{RecoveryFileCheck, RecoveryVerdict},
};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::Canceled;
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, FakeFileSystem};
use undo_support::*;

// ---------------------------------------------------------------------------------------------------------------
// Shared constants and helpers (C# fields of the test classes)
// ---------------------------------------------------------------------------------------------------------------

const JPEG: &str = r"C:\photos\a.jpg";
const RAW: &str = r"C:\photos\a.cr2";
const XMP: &str = r"C:\photos\a.xmp";
const MOVED_JPEG: &str = r"C:\photos\selected\a.jpg";
const MOVED_RAW: &str = r"C:\photos\selected\a.cr2";
const FAT_SOURCE: &str = r"C:\photos\fat.jpg";
const FAT_DESTINATION: &str = r"F:\sorted\fat.jpg";
const FAT_JPEG: &str = r"F:\selected\a.jpg";
const FAT_RAW: &str = r"F:\selected\a.cr2";

/// C# `Stamp` of the group and mutation-gap tests: `2026-09-29T08:00:00Z`.
fn group_stamp() -> DateTimeUtc {
    utc(2026, 9, 29, 8, 0, 0)
}

/// The English texts (what C# uses until a language is set).
fn texts() -> EnglishUndoTexts {
    EnglishUndoTexts
}

/// The error message of a result, `""` when there is none.
fn msg(result: &UndoResult) -> String {
    result.error_message.clone().unwrap_or_default()
}

/// C# `Assert.True(result.Succeeded, result.ErrorMessage)`.
fn assert_succeeded(result: &UndoResult) {
    assert!(result.succeeded, "undo failed: {:?}", result.error_message);
}

/// The undo service with the fixture's clock injected (C# `clock: clock`).
fn undo_for(f: &Fixture) -> UndoService {
    f.undo_with_clock(f.clock.clone())
}

/// A file moved by hand (C# `_fs.Move`): no hook fires, the stamp is kept.
fn by_hand_move(f: &Fixture, from: &str, to: &str) {
    FileSystem::move_file(f.disk.as_ref(), Path::new(from), Path::new(to)).expect("by-hand move");
}

/// A file deleted by hand (C# `_fs.Delete`).
fn by_hand_delete(f: &Fixture, path: &str) {
    FileSystem::delete(f.disk.as_ref(), Path::new(path)).expect("by-hand delete");
}

/// The journal lines of `read_committed_moves` whose `undo` flag is set (C# `Assert.Single(.., item => item.Undo == true)`).
fn undo_committed_lines(journal: &OperationJournal) -> Vec<JournalEntry> {
    read_committed_moves(journal)
        .into_iter()
        .filter(|line| line.undo == Some(true))
        .collect()
}

/// The group members of a line (C# `GroupMembers`), empty when the line has none.
fn members_of(line: &JournalEntry) -> Vec<JournalGroupMember> {
    line.group_members.clone().unwrap_or_default()
}

/// The sources of the members, in order.
fn sources_of(members: &[JournalGroupMember]) -> Vec<String> {
    members.iter().map(|m| m.source.clone()).collect()
}

/// The Failed line with `id` (C# `Assert.Single(ReadFailedOperations(), entry => entry.Id == id)`).
fn failed_with_id(journal: &OperationJournal, id: &str) -> Vec<JournalEntry> {
    read_failed_operations(journal)
        .into_iter()
        .filter(|line| line.id == id)
        .collect()
}

/// The count of journal lines mentioning `needle` (C# `DeleteGroupLines`).
fn journal_lines_mentioning(f: &Fixture, needle: &str) -> usize {
    f.journal_text().lines().filter(|line| line.contains(needle)).count()
}

/// A raw Move line of the journal, committed, with `timestamp == stamp`.
fn committed_move(id: &str, source: &str, destination: &str, size: i64, stamp: DateTimeUtc) -> JournalEntry {
    entry(
        id,
        FileOperationType::Move,
        JournalState::Committed,
        source,
        Some(destination),
        size,
        stamp,
    )
}

/// A single Recycle result (C# `new FileActionResult(true, Recycle, source, null, size, stamp, null)`).
fn recycled(source: &str, size: i64, stamp: DateTimeUtc) -> FileActionResult {
    single_result(true, FileOperationType::Recycle, source, None, size, stamp)
}

/// A group result with `permanently_deleted` set when any member is permanent (C# `Result(...)` helper of the group tests).
fn group_result_of(operation: FileOperationType, members: &[JournalGroupMember]) -> CaptureGroupActionResult {
    let mut result = group_result(operation, "g", members);
    result.permanently_deleted = members.iter().any(|m| m.permanent);
    result
}

/// `RegisterFailedDelete` of the group tests: appends the Failed line `delete-group` to the journal and registers the failed
/// group Delete with `completed` as the completed members.
fn register_failed_delete(
    f: &Fixture,
    undo: &mut UndoService,
    all: &[JournalGroupMember],
    completed: &[JournalGroupMember],
) -> JournalEntry {
    let line = JournalEntry {
        error: Some("boom".to_owned()),
        group_id: Some("g".to_owned()),
        group_members: Some(all.to_vec()),
        ..entry(
            "delete-group",
            FileOperationType::Recycle,
            JournalState::Failed,
            &all[0].source,
            None,
            all[0].size,
            group_stamp(),
        )
    };
    f.append(&line);
    let members = all
        .iter()
        .map(|m| {
            let done = completed.contains(m);
            let mut result = CaptureGroupMemberResult::new(m.clone(), done, false);
            result.error = (!done).then(|| "boom".to_owned());
            result.source_exists = !done;
            result
        })
        .collect();
    undo.register_group(&CaptureGroupActionResult::new(
        false,
        false,
        FileOperationType::Recycle,
        "g",
        Some(Box::new(line.clone())),
        members,
        Some("boom".to_owned()),
    ));
    line
}

/// Builds the state of a single Move to a FAT volume by hand (C# `MoveToFatAsync`, which uses `FileActionService.ExecuteAsync`):
/// the file at `F:\sorted\fat.jpg` carries the 2 s-rounded stamp, the registered Move keeps the odd source stamp.
fn move_to_fat(f: &Fixture, undo: &mut UndoService) -> FileActionResult {
    let odd = plus_ms(utc(2026, 9, 19, 9, 0, 1), 735);
    f.add_bytes(FAT_DESTINATION, b"image-content", round_to_fat(odd));
    assert_ne!(
        f.stamp_of(FAT_DESTINATION),
        odd,
        "precondition: the stamp really was rounded"
    );
    let moved = single_result(
        true,
        FileOperationType::Move,
        FAT_SOURCE,
        Some(FAT_DESTINATION),
        13,
        odd,
    );
    undo.register(&moved);
    moved
}

// ---------------------------------------------------------------------------------------------------------------
// UndoServiceTests.cs (27 facts)
// ---------------------------------------------------------------------------------------------------------------

// C#: UndoServiceTests.UndoMoveAsync_Success
#[test]
fn undo_move_async_success() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(destination, b"image-content", write);

    f.append(&committed_move("1", source, destination, 13, write));
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        write,
    ));

    assert!(!undo.move_history().is_empty());
    assert_eq!(undo.move_history().len(), 1);

    let result = undo.undo_move();

    assert!(result.succeeded);
    assert_eq!(result.operation, Some(FileOperationType::Move));
    assert_eq!(result.source, source);
    assert_eq!(result.destination.as_deref(), Some(destination));
    assert_eq!(result.error_message, None);
    assert!(f.exists(source));
    assert!(!f.exists(destination));
    assert!(undo.move_history().is_empty());
}

// C#: UndoServiceTests.UndoLastAsync_GroupMoveRestoresEveryMemberAsOneJournaledUndo
#[test]
fn undo_last_async_group_move_restores_every_member_as_one_journaled_undo() {
    let f = Fixture::new();
    let mut undo = undo_for(&f);
    let stamp = plus_secs(clock_now(), -60);
    let members = [
        member(JPEG, Some(MOVED_JPEG), 4, stamp, false),
        member(RAW, Some(MOVED_RAW), 8, stamp, false),
    ];
    f.add_bytes(MOVED_JPEG, b"jpeg", stamp);
    f.add_bytes(MOVED_RAW, b"raw data", stamp);
    let line = JournalEntry {
        group_id: Some("group".to_owned()),
        group_members: Some(members.to_vec()),
        ..entry(
            "group",
            FileOperationType::Move,
            JournalState::Committed,
            &members[0].source,
            members[0].destination.as_deref(),
            members[0].size,
            stamp,
        )
    };
    undo.register_group(&CaptureGroupActionResult::new(
        true,
        false,
        FileOperationType::Move,
        "group",
        Some(Box::new(line)),
        members.iter().map(completed).collect(),
        None,
    ));

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert_eq!(result.restored_paths, Some(sources_of(&members)));
    assert!(f.exists(JPEG));
    assert!(f.exists(RAW));
    let undo_line = undo_committed_lines(&f.journal);
    assert_eq!(undo_line.len(), 1);
    let undo_members = members_of(&undo_line[0]);
    assert_eq!(undo_members.len(), 2);
    assert!(
        undo_members
            .iter()
            .all(|m| f.exists(m.destination.as_deref().unwrap_or("")))
    );
}

// C#: UndoServiceTests.UndoLastAsync_GroupRecyclePartialRestore_RemainsRecoverableAndUndoable
#[test]
fn undo_last_async_group_recycle_partial_restore_remains_recoverable_and_undoable() {
    let f = Fixture::new();
    let stamp = plus_secs(clock_now(), -60);
    let members = [member(JPEG, None, 4, stamp, false), member(RAW, None, 8, stamp, false)];
    let line = JournalEntry {
        group_id: Some("recycled-group".to_owned()),
        group_members: Some(members.to_vec()),
        ..entry(
            "recycled-group",
            FileOperationType::Recycle,
            JournalState::Committed,
            JPEG,
            None,
            4,
            stamp,
        )
    };
    // The bin restores only the first member (C# PartialRestoreRecycleBin).
    f.bin.set_default(BinMode::Fail);
    f.bin.set_mode_for(JPEG, BinMode::Restore);
    let mut undo = f.undo();
    undo.register_group(&CaptureGroupActionResult::new(
        true,
        false,
        FileOperationType::Recycle,
        "recycled-group",
        Some(Box::new(line)),
        members.iter().map(completed).collect(),
        None,
    ));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(f.exists(JPEG));
    assert!(!f.exists(RAW));
    assert!(undo.has_last_undo_action());
    let failed = read_failed_operations(&f.journal);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].undo, Some(true));
    assert_eq!(members_of(&failed[0]).len(), 2);
    assert_eq!(
        sources_of(&members_of(&failed[0])),
        vec![JPEG.to_owned(), RAW.to_owned()]
    );
}

// C#: UndoServiceTests.UndoMoveAsync_RepeatedRegisterAndUndo_DoesNotLeakFingerprints
#[test]
fn undo_move_async_repeated_register_and_undo_does_not_leak_fingerprints() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let write = write_time();

    for i in 0..10 {
        let source = format!(r"C:\photos\cycle-{i}.jpg");
        let destination = format!(r"C:\photos\sorted\cycle-{i}.jpg");
        f.add_bytes(&destination, b"image-content", write);
        undo.register(&single_result(
            true,
            FileOperationType::Move,
            &source,
            Some(&destination),
            13,
            write,
        ));

        let result = undo.undo_move();
        assert!(result.succeeded);
        assert_eq!(undo.move_fingerprint_count(), 0);
    }
}

// C#: UndoServiceTests.UndoMoveAsync_JournalsReverseMove_PreparedBeforeMutationThenCommitted
#[test]
fn undo_move_async_journals_reverse_move_prepared_before_mutation_then_committed() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(destination, b"image-content", write);
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        write,
    ));
    let pending_at_mutation: Arc<Mutex<Option<Vec<JournalEntry>>>> = Arc::new(Mutex::new(None));
    let captured = pending_at_mutation.clone();
    let journal = f.journal.clone();
    f.fs.on_move(move |_, _| {
        *captured.lock().unwrap_or_else(|e| e.into_inner()) = Some(read_pending_operations(&journal));
        None
    });

    let result = undo.undo_move();

    assert!(result.succeeded);
    // A crash during the move would leave exactly this pending entry for startup reconcile / Recovery.
    let pending_list = pending_at_mutation
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .expect("the move hook ran");
    assert_eq!(pending_list.len(), 1);
    let pending = &pending_list[0];
    assert_eq!(pending.op_type, FileOperationType::Move);
    assert_eq!(pending.source, destination);
    assert_eq!(pending.destination.as_deref(), Some(source));
    assert_eq!(pending.size, 13);
    assert_eq!(pending.undo, Some(true));
    assert!(read_pending_operations(&f.journal).is_empty());
    let committed = read_committed_moves(&f.journal);
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].id, pending.id);
    assert_eq!(committed[0].undo, Some(true));
}

// C#: UndoServiceTests.UndoMoveAsync_MoveThrows_JournalsFailed
#[test]
fn undo_move_async_move_throws_journals_failed() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(destination, b"image-content", write);
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        write,
    ));
    f.fs.on_move(|_, _| Some(io::Error::other("locked")));

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert!(!undo.move_history().is_empty());
    let failed = read_failed_operations(&f.journal);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].source, destination);
    assert_eq!(failed[0].undo, Some(true));
}

// C#: UndoServiceTests.UndoMoveAsync_MoveLeavesSource_FailsWithDistinctCode
#[test]
fn undo_move_async_move_leaves_source_fails_with_distinct_code() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"D:\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(destination, b"image-content", write);
    f.disk.add_dir(r"C:\photos");
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        write,
    ));
    f.fs.set_move_leaves_source(true);

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert!(f.exists(destination));
    let failed = read_failed_operations(&f.journal);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].error_code.as_deref(), Some(MOVE_SOURCE_NOT_REMOVED));
}

// C#: UndoServiceTests.Restart_AfterUndo_HasNothingToUndo
#[test]
fn restart_after_undo_has_nothing_to_undo() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(destination, b"image-content", write);
    f.append(&committed_move("1", source, destination, 13, write));
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        write,
    ));
    assert!(undo.undo_move().succeeded);

    // Simulates a restart: a brand-new UndoService over the same (now Committed-undo-laden) journal.
    let restarted = f.undo();

    assert!(restarted.move_history().is_empty());
}

// C#: UndoServiceTests.RegisterMoreThanStartupTail_UndoLatestUsesRegisteredFingerprint
#[test]
fn register_more_than_startup_tail_undo_latest_uses_registered_fingerprint() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let write = plus_secs(clock_now(), -60);
    for i in 0..250 {
        let source = format!(r"C:\photos\source-{i}.jpg");
        let destination = format!(r"C:\photos\sorted\source-{i}.jpg");
        let size = 7 + i64::from(i);
        f.add(&destination, usize::try_from(size).expect("small size"), write);
        f.append(&committed_move(
            &format!("move-{i}"),
            &source,
            &destination,
            size,
            write,
        ));
        undo.register(&single_result(
            true,
            FileOperationType::Move,
            &source,
            Some(&destination),
            size,
            write,
        ));
    }

    assert_eq!(undo.move_history().len(), 250);
    let result = undo.undo_move();

    assert!(result.succeeded);
    assert_eq!(result.source, r"C:\photos\source-249.jpg");
    assert_eq!(undo.move_history().len(), 249);
}

// C#: UndoServiceTests.UndoMoveAsync_DestinationModified_FailsAndRestoresStack
#[test]
fn undo_move_async_destination_modified_fails_and_restores_stack() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(destination, b"modified-longer-content", write);
    f.append(&committed_move("1", source, destination, 10, write));
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        10,
        write,
    ));

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert!(msg(&result).contains(&texts().core_undo_destination_changed_after_move()));
    assert!(!undo.move_history().is_empty());
    assert!(!f.exists(source));
    assert!(f.exists(destination));
}

// C#: UndoServiceTests.UndoMoveAsync_SourceAlreadyExists_FailsAndRestoresStack
#[test]
fn undo_move_async_source_already_exists_fails_and_restores_stack() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(source, b"source-already-there", write);
    f.add_bytes(destination, b"dest", write);
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        4,
        write,
    ));

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert!(msg(&result).contains(&texts().core_undo_source_or_destination_changed()));
    assert!(!undo.move_history().is_empty());
}

// C#: UndoServiceTests.UndoMoveAsync_DestinationDeletedExternally_ReportsOnceAndDropsEntry
#[test]
fn undo_move_async_destination_deleted_externally_reports_once_and_drops_entry() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let write = write_time();
    let old_source = r"C:\photos\old.jpg";
    let old_destination = r"C:\photos\sorted\old.jpg";
    let broken_source = r"C:\photos\broken.jpg";
    let broken_destination = r"C:\photos\sorted\broken.jpg";
    f.add_bytes(old_destination, b"old!", write);
    f.add_bytes(broken_destination, b"brok", write);
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        old_source,
        Some(old_destination),
        4,
        write,
    ));
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        broken_source,
        Some(broken_destination),
        4,
        write,
    ));
    by_hand_delete(&f, broken_destination);

    let first = undo.undo_last();

    assert!(!first.succeeded);
    assert_eq!(undo.move_history().len(), 1);
    assert!(!undo.has_last_undo_action());

    let second = undo.undo_move();

    assert!(second.succeeded);
    assert_eq!(second.source, old_source);
    assert!(f.exists(old_source));
    assert!(undo.move_history().is_empty());
}

// C#: UndoServiceTests.UndoMoveAsync_SourceOccupied_KeepsEntryUntilSourceFreed
#[test]
fn undo_move_async_source_occupied_keeps_entry_until_source_freed() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(source, b"blocker", write);
    f.add_bytes(destination, b"dest", write);
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        4,
        write,
    ));

    let blocked = undo.undo_last();
    assert!(!blocked.succeeded);
    assert!(undo.has_last_undo_action());

    by_hand_delete(&f, source);
    let retry = undo.undo_last();

    assert!(retry.succeeded);
}

// C#: UndoServiceTests.UndoMoveAsync_CaseInsensitiveDestinationMatch_P12
#[test]
fn undo_move_async_case_insensitive_destination_match_p12() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\Photos\Photo1.JPG";
    let destination_in_journal = r"C:\Photos\Sorted\Photo1.JPG";
    let destination_in_move = r"c:\photos\sorted\photo1.jpg";
    let write = write_time();
    f.add_bytes(destination_in_move, b"test-data", write);
    f.append(&committed_move("1", source, destination_in_journal, 9, write));
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination_in_move),
        9,
        write,
    ));

    let result = undo.undo_move();

    assert!(result.succeeded);
    assert!(f.exists(source));
}

// C#: UndoServiceTests.UndoMoveAsync_EmptyHistory_ReturnsError
#[test]
fn undo_move_async_empty_history_returns_error() {
    let f = Fixture::new();
    let mut undo = f.undo();

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert_eq!(result.error_message, Some(texts().core_undo_no_move_to_undo()));
}

// C#: UndoServiceTests.Register_Recycle_SetsLastAction_UndoLastRestores
#[test]
fn register_recycle_sets_last_action_undo_last_restores() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\deleted.jpg";
    let write = write_time();

    undo.register(&single_result(
        true,
        FileOperationType::Recycle,
        source,
        None,
        100,
        write,
    ));

    assert!(undo.has_last_undo_action());
    assert!(undo.move_history().is_empty());

    let result = undo.undo_last();

    assert!(result.succeeded);
    assert_eq!(result.operation, Some(FileOperationType::Recycle));
    assert_eq!(result.source, source);
    assert_eq!(f.bin.calls().last().map(String::as_str), Some(source));
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceTests.UndoLastAsync_Recycle_FailsWhenRestoreReturnsFalse
#[test]
fn undo_last_async_recycle_fails_when_restore_returns_false() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\deleted.jpg";
    f.bin.set_default(BinMode::Fail);

    undo.register(&single_result(
        true,
        FileOperationType::Recycle,
        source,
        None,
        100,
        write_time(),
    ));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(msg(&result).contains(&texts().core_undo_recycle_restore_failed("deleted.jpg")));
}

// C#: UndoServiceTests.UndoLastAsync_Recycle_RestoreReportsFailureButFileIsBack_Succeeds (C5)
#[test]
fn undo_last_async_recycle_restore_reports_failure_but_file_is_back_succeeds() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\deleted.jpg";
    // The bin puts the file back and still answers false (C# OnTryRestore adds the file, TryRestoreResult = false).
    f.bin.set_default(BinMode::RestoreReportFalse);
    undo.register(&single_result(
        true,
        FileOperationType::Recycle,
        source,
        None,
        100,
        write_time(),
    ));

    let result = undo.undo_last();

    assert!(result.succeeded);
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceTests.UndoLastAsync_Recycle_SourceOccupied_DoesNotRestore
#[test]
fn undo_last_async_recycle_source_occupied_does_not_restore() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\deleted.jpg";
    f.add_bytes(source, b"a newer file", write_time());
    undo.register(&single_result(
        true,
        FileOperationType::Recycle,
        source,
        None,
        100,
        write_time(),
    ));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(f.bin.calls().is_empty());
    assert!(undo.has_last_undo_action());
    assert!(msg(&result).contains("deleted.jpg"));
}

// C#: UndoServiceTests.UndoLastAsync_NoLastAction_ReturnsError
#[test]
fn undo_last_async_no_last_action_returns_error() {
    let f = Fixture::new();
    let mut undo = f.undo();

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(result.error_message, Some(texts().core_undo_nothing_to_undo()));
}

// C#: UndoServiceTests.UndoMoveAsync_WhenGateBusy_ReturnsRejected
#[test]
fn undo_move_async_when_gate_busy_returns_rejected() {
    let f = Fixture::new();
    let gate = TestGate::new();
    let mut undo = f.undo().with_gate(gate.clone());
    assert!(gate.try_begin());
    assert!(undo.is_busy());

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert!(result.rejected);
    assert!(msg(&result).contains(&texts().core_undo_busy()));

    gate.end();
    assert!(!undo.is_busy());
}

// C#: UndoServiceTests.UndoLastAsync_DoesNotResumeOnCapturedContext
// R0003: blocking API, nothing to resume. Only the functional part is ported: a Move undo then a Recycle undo both succeed.
#[test]
fn undo_last_async_does_not_resume_on_captured_context() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let recycled_path = r"C:\photos\deleted.jpg";
    let write = write_time();
    f.add_bytes(destination, b"image-content", write);

    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        write,
    ));
    let move_result = undo.undo_last();
    undo.register(&recycled(recycled_path, 100, write));
    let recycle_result = undo.undo_last();

    assert!(move_result.succeeded);
    assert!(recycle_result.succeeded);
}

// RV-C01: a FAT destination volume stores write times rounded to 2 s, so a moved file's stamp there is never exactly the
// stamp the source had (the fingerprint Undo compares against).

// C#: UndoServiceTests.UndoMoveAsync_DestinationOnFatRoundedStamp_RestoresFile
#[test]
fn undo_move_async_destination_on_fat_rounded_stamp_restores_file() {
    let f = Fixture::new();
    let mut undo = f.undo();
    move_to_fat(&f, &mut undo);

    let result = undo.undo_move();

    assert!(result.succeeded, "{:?}", result.error_message);
    assert!(f.exists(FAT_SOURCE));
    assert!(!f.exists(FAT_DESTINATION));
}

// C#: UndoServiceTests.UndoMoveAsync_DestinationSizeChanged_Refuses
#[test]
fn undo_move_async_destination_size_changed_refuses() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let odd = plus_ms(utc(2026, 9, 19, 9, 0, 1), 735);
    move_to_fat(&f, &mut undo);
    f.add_bytes(FAT_DESTINATION, b"image-content-edited", round_to_fat(odd));

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert!(!f.exists(FAT_SOURCE));
    assert!(f.exists(FAT_DESTINATION));
    assert!(!undo.move_history().is_empty());
}

// C#: UndoServiceTests.UndoMoveAsync_DestinationStampMovedBy3Seconds_Refuses
#[test]
fn undo_move_async_destination_stamp_moved_by_3_seconds_refuses() {
    let f = Fixture::new();
    let mut undo = f.undo();
    let odd = plus_ms(utc(2026, 9, 19, 9, 0, 1), 735);
    move_to_fat(&f, &mut undo);
    // Same size, but written 3 s after the recorded stamp: more than any volume rounds, so it is another file.
    f.add_bytes(FAT_DESTINATION, b"image-CONTENT", plus_secs(odd, 3));

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert!(!f.exists(FAT_SOURCE));
    assert!(f.exists(FAT_DESTINATION));
    assert!(!undo.move_history().is_empty());
}

// C#: UndoServiceTests.UndoMoveAsync_CalledDirectlyWhileLastActionIsRecycle_KeepsRecycleUndo
#[test]
fn undo_move_async_called_directly_while_last_action_is_recycle_keeps_recycle_undo() {
    // RV-C06: UndoMoveAsync pops the Move history; it must not forget a later, unrelated Recycle.
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\photo1.jpg";
    let destination = r"C:\photos\sorted\photo1.jpg";
    let recycled_path = r"C:\photos\deleted.jpg";
    let write = write_time();
    f.add_bytes(destination, b"image-content", write);
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        write,
    ));
    undo.register(&recycled(recycled_path, 100, write));

    let move_result = undo.undo_move();
    assert!(move_result.succeeded, "{:?}", move_result.error_message);

    assert!(undo.has_last_undo_action());
    let recycle_result = undo.undo_last();
    assert!(recycle_result.succeeded, "{:?}", recycle_result.error_message);
    assert_eq!(recycle_result.operation, Some(FileOperationType::Recycle));
    assert_eq!(f.bin.calls().last().map(String::as_str), Some(recycled_path));
}

// C#: UndoServiceTests.UndoMoveAsync_SourceFolderDeletedAfterMove_RecreatesFolderAndRestores
#[test]
fn undo_move_async_source_folder_deleted_after_move_recreates_folder_and_restores() {
    // RV-C07: the original folder was removed after the Move (it became empty); the group undo recreates it, so must this.
    let f = Fixture::new();
    let mut undo = f.undo();
    let source = r"C:\photos\day1\photo1.jpg";
    let destination = r"D:\sorted\photo1.jpg";
    let odd = plus_ms(utc(2026, 9, 19, 9, 0, 1), 735);
    f.add_bytes(destination, b"image-content", odd);
    assert!(!f.disk.directory_exists(Path::new(r"C:\photos\day1")));
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        source,
        Some(destination),
        13,
        odd,
    ));

    let result = undo.undo_move();

    assert!(result.succeeded, "{:?}", result.error_message);
    assert!(f.exists(source));
    assert!(!f.exists(destination));
}

// ---------------------------------------------------------------------------------------------------------------
// UndoServiceGroupTests.cs (35 facts)
// ---------------------------------------------------------------------------------------------------------------

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycle_RestoresOffTheCallerThreadInOrder
// R0003: blocking API. The thread-id asserts are dropped; the restore order is kept.
#[test]
fn undo_last_async_group_recycle_restores_off_the_caller_thread_in_order() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let result = undo.undo_last();

    assert!(result.succeeded, "{:?}", result.error_message);
    assert_eq!(f.bin.calls(), vec![JPEG.to_owned(), RAW.to_owned()]);
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecyclePartialRestoreOffTheCallerThread_StillReportsRestoredPaths
// R0003: blocking API. The thread-id asserts are dropped; the restored-paths and order asserts are kept.
#[test]
fn undo_last_async_group_recycle_partial_restore_off_the_caller_thread_still_reports_restored_paths() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.bin.set_mode_for(RAW, BinMode::Fail);
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(result.restored_paths, Some(vec![JPEG.to_owned()]));
    assert_eq!(f.bin.calls(), vec![JPEG.to_owned(), RAW.to_owned()]);
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupMove_MovesBackOffTheCallerThread
// R0003: blocking API. The thread-id asserts are dropped; the order of the moves is kept.
#[test]
fn undo_last_async_group_move_moves_back_off_the_caller_thread() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    f.add_bytes(MOVED_RAW, b"raw data", s);
    let members = [
        member(JPEG, Some(MOVED_JPEG), 4, s, false),
        member(RAW, Some(MOVED_RAW), 8, s, false),
    ];
    undo.register_group(&group_result_of(FileOperationType::Move, &members));

    let result = undo.undo_last();

    assert!(result.succeeded, "{:?}", result.error_message);
    assert_eq!(f.fs.moved_sources(), vec![MOVED_JPEG.to_owned(), MOVED_RAW.to_owned()]);
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupMoveAlreadyRestoredExternally_ClearsTheActionSoItDoesNotRepeat
#[test]
fn undo_last_async_group_move_already_restored_externally_clears_the_action_so_it_does_not_repeat() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.add_bytes(JPEG, b"jpeg", s);
    f.add_bytes(RAW, b"raw data", s);
    let members = [
        member(JPEG, Some(MOVED_JPEG), 4, s, false),
        member(RAW, Some(MOVED_RAW), 8, s, false),
    ];
    undo.register_group(&group_result_of(FileOperationType::Move, &members));

    let first = undo.undo_last();
    let second = undo.undo_last();

    assert!(!first.succeeded);
    assert!(!undo.has_last_undo_action());
    assert_eq!(second.error_message, Some(texts().core_undo_nothing_to_undo()));
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleEverythingAlreadyBack_ClearsTheActionSoItDoesNotRepeat
#[test]
fn undo_last_async_group_recycle_everything_already_back_clears_the_action_so_it_does_not_repeat() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.add_bytes(JPEG, b"jpeg", s);
    f.add_bytes(RAW, b"raw data", s);
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let first = undo.undo_last();
    let second = undo.undo_last();

    assert!(!first.succeeded);
    assert!(!undo.has_last_undo_action());
    assert_eq!(second.error_message, Some(texts().core_undo_nothing_to_undo()));
    assert_eq!(f.bin.restore_calls(), 0);
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleUnrelatedFileAtOriginalPath_FailsInsteadOfReportingSuccess
#[test]
fn undo_last_async_group_recycle_unrelated_file_at_original_path_fails_instead_of_reporting_success() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    // Same path, not the recycled photo: a different, newer file.
    f.add_bytes(JPEG, b"a different, newer file", plus_hours(s, 5));
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(msg(&result).contains(&texts().core_undo_recycle_target_exists("a.jpg")));
    // Nothing was restored on top of / next to the unrelated file.
    assert_eq!(f.bin.restore_calls(), 0);
    assert!(!f.exists(RAW));
    assert!(undo.has_last_undo_action());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleMemberWithMatchingIdentityAlreadyBack_IsSkippedAndRestPending
#[test]
fn undo_last_async_group_recycle_member_with_matching_identity_already_back_is_skipped_and_rest_pending() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    // Restored earlier by hand: same size and time.
    f.add_bytes(JPEG, b"jpeg", s);
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert_eq!(f.bin.restore_calls(), 1);
    assert!(f.exists(RAW));
    assert_eq!(result.restored_paths, Some(vec![JPEG.to_owned(), RAW.to_owned()]));
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupMoveOriginalHasSameLengthButOtherTimestamp_IsNotTreatedAsRestored
#[test]
fn undo_last_async_group_move_original_has_same_length_but_other_timestamp_is_not_treated_as_restored() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    // Same length as the moved one, different write time.
    f.add_bytes(JPEG, b"JPEG", plus_hours(s, 3));
    f.add_bytes(MOVED_RAW, b"raw data", s);
    let members = [
        member(JPEG, Some(MOVED_JPEG), 4, s, false),
        member(RAW, Some(MOVED_RAW), 8, s, false),
    ];
    undo.register_group(&group_result_of(FileOperationType::Move, &members));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(f.exists(MOVED_RAW));
    assert!(!f.exists(RAW));
    assert!(f.fs.moved_sources().is_empty());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupMoveFailsMidWay_ReturnsTheAlreadyRestoredPaths
#[test]
fn undo_last_async_group_move_fails_mid_way_returns_the_already_restored_paths() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    f.add_bytes(MOVED_RAW, b"raw data", s);
    f.fs.fail_move_of(MOVED_RAW, "simulated undo failure");
    let members = [
        member(JPEG, Some(MOVED_JPEG), 4, s, false),
        member(RAW, Some(MOVED_RAW), 8, s, false),
    ];
    undo.register_group(&group_result_of(FileOperationType::Move, &members));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(result.restored_paths, Some(vec![JPEG.to_owned()]));
    assert!(f.exists(JPEG));
    // Still retryable: the next attempt skips the restored member.
    assert!(undo.has_last_undo_action());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleFailsMidWay_ReturnsTheAlreadyRestoredPaths
#[test]
fn undo_last_async_group_recycle_fails_mid_way_returns_the_already_restored_paths() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.bin.set_mode_for(RAW, BinMode::Fail);
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(result.restored_paths, Some(vec![JPEG.to_owned()]));
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleWithOnePermanentMember_RestoresTheRecyclableOnesAndReportsTheOther
#[test]
fn undo_last_async_group_recycle_with_one_permanent_member_restores_the_recyclable_ones_and_reports_the_other() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let members = [
        member(JPEG, None, 4, s, false),
        member(RAW, None, 8, s, false),
        member(XMP, None, 3, s, true),
    ];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert_eq!(result.restored_paths, Some(vec![JPEG.to_owned(), RAW.to_owned()]));
    assert_eq!(f.bin.restore_calls(), 2);
    assert!(!f.exists(XMP));
    assert_eq!(
        result.error_message,
        Some(texts().core_undo_group_partially_restored(2, 3, "a.xmp"))
    );
    assert!(!undo.has_last_undo_action());
    assert!(read_pending_and_failed_operations(&f.journal).is_empty());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupMoveRetryAfterPartialUndoWhenPartnerCameBackMeanwhile_SucceedsIdempotentlyWithAllPaths
#[test]
fn undo_last_async_group_move_retry_after_partial_undo_when_partner_came_back_meanwhile_succeeds_idempotently() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    f.add_bytes(MOVED_RAW, b"raw data", s);
    f.fs.fail_move_of(MOVED_RAW, "simulated undo failure");
    let members = [
        member(JPEG, Some(MOVED_JPEG), 4, s, false),
        member(RAW, Some(MOVED_RAW), 8, s, false),
    ];
    undo.register_group(&group_result_of(FileOperationType::Move, &members));
    let partial = undo.undo_last();
    assert!(!partial.succeeded);
    assert_eq!(read_failed_operations(&f.journal).len(), 1);
    f.fs.reset_step();
    // E.g. restored through the Recovery window in the meantime.
    by_hand_move(&f, MOVED_RAW, RAW);

    let retry = undo.undo_last();

    assert_succeeded(&retry);
    assert_eq!(retry.restored_paths, Some(vec![JPEG.to_owned(), RAW.to_owned()]));
    assert!(!undo.has_last_undo_action());
    // The earlier Failed Recovery item is closed.
    assert!(read_failed_operations(&f.journal).is_empty());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleRetryAfterPartialUndoWhenPartnerCameBackMeanwhile_SucceedsIdempotentlyWithAllPaths
#[test]
fn undo_last_async_group_recycle_retry_after_partial_undo_when_partner_came_back_meanwhile_succeeds_idempotently() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.bin.set_mode_for(RAW, BinMode::Fail);
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));
    let partial = undo.undo_last();
    assert!(!partial.succeeded);
    assert_eq!(read_failed_operations(&f.journal).len(), 1);
    f.bin.clear_modes();
    // Restored through the Recovery window in the meantime.
    f.add_bytes(RAW, b"raw data", s);
    let restore_calls_before = f.bin.restore_calls();

    let retry = undo.undo_last();

    assert_succeeded(&retry);
    assert_eq!(retry.restored_paths, Some(vec![JPEG.to_owned(), RAW.to_owned()]));
    assert_eq!(f.bin.restore_calls(), restore_calls_before);
    assert!(!undo.has_last_undo_action());
    assert!(read_failed_operations(&f.journal).is_empty());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupMoveRetryThatRestoresRemainingMembers_ClosesTheEarlierFailedLine
#[test]
fn undo_last_async_group_move_retry_that_restores_remaining_members_closes_the_earlier_failed_line() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    f.add_bytes(MOVED_RAW, b"raw data", s);
    f.fs.fail_move_of(MOVED_RAW, "simulated undo failure");
    let members = [
        member(JPEG, Some(MOVED_JPEG), 4, s, false),
        member(RAW, Some(MOVED_RAW), 8, s, false),
    ];
    undo.register_group(&group_result_of(FileOperationType::Move, &members));
    assert!(!undo.undo_last().succeeded);
    assert_eq!(read_failed_operations(&f.journal).len(), 1);
    f.fs.reset_step();

    let retry = undo.undo_last();

    assert_succeeded(&retry);
    // Otherwise it looks retryable and would move fresh files.
    assert!(read_failed_operations(&f.journal).is_empty());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleRetryThatRestoresRemainingMembers_ClosesTheEarlierFailedLine
#[test]
fn undo_last_async_group_recycle_retry_that_restores_remaining_members_closes_the_earlier_failed_line() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.bin.set_mode_for(RAW, BinMode::Fail);
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));
    assert!(!undo.undo_last().succeeded);
    assert_eq!(read_failed_operations(&f.journal).len(), 1);
    f.bin.clear_modes();

    let retry = undo.undo_last();

    assert_succeeded(&retry);
    assert!(read_failed_operations(&f.journal).is_empty());
}

// C#: UndoServiceGroupTests.RegisterGroup_RecycleFailedPartWay_RegistersOnlyTheCompletedNonPermanentlyDeletedMembers
#[test]
fn register_group_recycle_failed_part_way_registers_only_the_completed_non_permanently_deleted_members() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let jpeg = member(JPEG, None, 4, s, false);
    let raw = member(RAW, None, 8, s, false);
    let xmp = member(XMP, None, 3, s, true);
    let mut raw_failed = CaptureGroupMemberResult::new(raw.clone(), false, false);
    raw_failed.error = Some("boom".to_owned());
    raw_failed.source_exists = true;
    let failed = CaptureGroupActionResult::new(
        false,
        false,
        FileOperationType::Recycle,
        "g",
        None,
        vec![completed(&jpeg), completed(&xmp), raw_failed],
        Some("boom".to_owned()),
    );

    undo.register_group(&failed);

    assert!(undo.has_last_undo_action());
    // The raw never left the disk.
    f.add(RAW, 8, s);
    let result = undo.undo_last();
    assert_succeeded(&result);
    assert_eq!(f.bin.restore_calls(), 1, "only the JPEG");
    assert!(f.exists(JPEG));
    assert!(!f.exists(XMP));
}

/// The two members of the failed-delete tests: a JPEG and a RAW that were both recycled.
fn jpeg_and_raw() -> (JournalGroupMember, JournalGroupMember) {
    let s = group_stamp();
    (member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false))
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteWhereEveryMemberWasRecycled_ClosesTheFailedLineSoRecoveryOffersNoRetry
#[test]
fn undo_last_async_failed_group_delete_where_every_member_was_recycled_closes_the_failed_line_so_recovery_offers_no_retry()
 {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let (jpeg, raw) = jpeg_and_raw();
    register_failed_delete(&f, &mut undo, &[jpeg.clone(), raw.clone()], &[jpeg, raw]);

    let result = undo.undo_last();

    assert_succeeded(&result);
    // Otherwise Retry would recycle the files the user just restored.
    assert!(read_failed_operations(&f.journal).is_empty());
    assert!(read_pending_and_failed_operations(&f.journal).is_empty());
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeletePartWay_RestoredMemberLeavesTheFailedLineAndTheOtherStaysRetryable (*W3-07)
#[test]
fn undo_last_async_failed_group_delete_part_way_restored_member_leaves_the_failed_line_and_the_other_stays_retryable() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let (jpeg, raw) = jpeg_and_raw();
    // The JPEG reached the bin, the RAW never left the disk.
    register_failed_delete(&f, &mut undo, &[jpeg.clone(), raw.clone()], &[jpeg]);
    f.add(RAW, 8, s);

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert!(f.exists(JPEG));
    let still_failed = read_failed_operations(&f.journal);
    assert_eq!(still_failed.len(), 1);
    assert_eq!(members_of(&still_failed[0]), vec![raw.clone()]);
    // Recovery would retry only the RAW; the restored JPEG is no longer part of the retry.
    let check = RecoveryFileCheck::new(f.fs.clone()).check(&still_failed[0]);
    assert_eq!(check.verdict, RecoveryVerdict::CanRetry);
    let checked: Vec<String> = check
        .group_members
        .unwrap_or_default()
        .iter()
        .map(|item| item.member.source.clone())
        .collect();
    assert_eq!(checked, vec![RAW.to_owned()]);
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteWithPermanentMember_OnlyTheRestoredMembersLeaveTheFailedLine
#[test]
fn undo_last_async_failed_group_delete_with_permanent_member_only_the_restored_members_leave_the_failed_line() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let (jpeg, raw) = jpeg_and_raw();
    let xmp = member(XMP, None, 3, s, true);
    register_failed_delete(
        &f,
        &mut undo,
        &[jpeg.clone(), raw.clone(), xmp.clone()],
        &[jpeg, xmp.clone()],
    );
    f.add(RAW, 8, s);

    let result = undo.undo_last();

    assert_succeeded(&result);
    let still_failed = read_failed_operations(&f.journal);
    assert_eq!(still_failed.len(), 1);
    assert_eq!(members_of(&still_failed[0]), vec![raw, xmp]);
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteUndoFailsAfterRestoringSomeMembers_RestoredMemberLeavesTheFailedLine
#[test]
fn undo_last_async_failed_group_delete_undo_fails_after_restoring_some_members_restored_member_leaves_the_failed_line()
{
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let (jpeg, raw) = jpeg_and_raw();
    // Both reached the bin; restoring the JPEG works, the RAW then fails.
    register_failed_delete(
        &f,
        &mut undo,
        &[jpeg.clone(), raw.clone()],
        &[jpeg.clone(), raw.clone()],
    );
    f.bin.set_mode_for(RAW, BinMode::Fail);

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(f.exists(JPEG));
    // A fresh Recovery window must not offer a retry that would recycle the JPEG the undo just restored.
    let still_failed = failed_with_id(&f.journal, "delete-group");
    assert_eq!(still_failed.len(), 1);
    assert_eq!(members_of(&still_failed[0]), vec![raw]);
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteWhereTheRecycledMemberWasRestoredByHand_SettlesTheFailedLineAndReportsAlreadyHandled
#[test]
fn undo_last_async_failed_group_delete_where_the_recycled_member_was_restored_by_hand_settles_the_failed_line_and_reports_already_handled()
 {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let (jpeg, raw) = jpeg_and_raw();
    // The JPEG reached the bin, the RAW never left the disk.
    register_failed_delete(&f, &mut undo, &[jpeg.clone(), raw.clone()], &[jpeg]);
    // The user restored the JPEG from the bin by hand.
    f.add_bytes(JPEG, b"jpeg", s);
    f.add_bytes(RAW, b"raw data", s);

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(msg(&result).contains(&texts().core_recovery_already_handled()));
    assert_eq!(f.bin.restore_calls(), 0);
    // A retry would recycle only the RAW, never the restored JPEG.
    let still_failed = failed_with_id(&f.journal, "delete-group");
    assert_eq!(still_failed.len(), 1);
    assert_eq!(members_of(&still_failed[0]), vec![raw]);
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteSettleAppendThrowsUnexpectedException_UndoStillReportsSuccess
// DIVERGENCE: Rust has no "unexpected exception" class; any io::Error of the settle append is swallowed.
#[test]
fn undo_last_async_failed_group_delete_settle_append_throws_unexpected_exception_undo_still_reports_success() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let (jpeg, raw) = jpeg_and_raw();
    register_failed_delete(&f, &mut undo, &[jpeg.clone(), raw.clone()], &[jpeg, raw]);
    // The undo's own Prepared and Committed lines are appended first; the settle is the third append.
    let appends = Arc::new(AtomicUsize::new(0));
    let counter = appends.clone();
    f.fs.on_open_append(move |_| {
        let nth = counter.fetch_add(1, Ordering::SeqCst) + 1;
        (nth >= 3).then(|| io::Error::other("simulated journal failure"))
    });

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert!(f.exists(JPEG));
    assert!(f.exists(RAW));
    f.fs.reset_step();
    // No Failed line after the undo's Committed.
    assert!(
        read_failed_operations(&f.journal)
            .iter()
            .all(|line| line.undo != Some(true))
    );
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteWhereTheRecycledMemberWasRestoredAndEdited_RestoresNothingAndRecoveryOffersNoRetry (*W3-07, **W3-06)
#[test]
fn undo_last_async_failed_group_delete_where_the_recycled_member_was_restored_and_edited_restores_nothing_and_recovery_offers_no_retry()
 {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let (jpeg, raw) = jpeg_and_raw();
    let line = register_failed_delete(&f, &mut undo, &[jpeg.clone(), raw.clone()], &[jpeg]);
    // Put back by hand, then edited: same size, other write time.
    f.add_bytes(JPEG, b"jpeg", plus_hours(s, 2));
    f.add_bytes(RAW, b"raw data", s);

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(f.bin.restore_calls(), 0);
    // The line is untouched: nothing was undone.
    assert_eq!(read_failed_operations(&f.journal), vec![line.clone()]);
    // Recovery must not offer a retry that would recycle the edited JPEG.
    let check = RecoveryFileCheck::new(f.fs.clone()).check(&line);
    assert_ne!(check.verdict, RecoveryVerdict::CanRetry);
    // TODO(lead): add the RecoveryRetryService retry + `Assert.False(retry.Succeeded)` + `Assert.True(FileExists(Jpeg))`
    // (C# l.459-461, `new RecoveryRetryService(_journal, _fs, new FixedClock(), _bin).RetryMoveOrCopyAsync(entry)`) after
    // W3-06 merges. W3-06 is not merged yet: `RecoveryRetryService` is still `todo!()`.
}

// C#: UndoServiceGroupTests.RegisterGroup_FailedWithNothingCompletedOrNotRecycle_RegistersNothing
#[test]
fn register_group_failed_with_nothing_completed_or_not_recycle_registers_nothing() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let jpeg = member(JPEG, None, 4, s, false);
    let mut not_completed = CaptureGroupMemberResult::new(jpeg, false, false);
    not_completed.error = Some("x".to_owned());
    not_completed.source_exists = true;
    undo.register_group(&CaptureGroupActionResult::new(
        false,
        false,
        FileOperationType::Recycle,
        "g",
        None,
        vec![not_completed],
        Some("x".to_owned()),
    ));
    undo.register_group(&CaptureGroupActionResult::new(
        false,
        false,
        FileOperationType::Move,
        "g",
        None,
        vec![completed(&member(JPEG, Some(MOVED_JPEG), 4, s, false))],
        Some("x".to_owned()),
    ));

    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleNothingEverRestoredByThisUndo_StillReportsAlreadyHandled
#[test]
fn undo_last_async_group_recycle_nothing_ever_restored_by_this_undo_still_reports_already_handled() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    f.add_bytes(JPEG, b"jpeg", s);
    f.add_bytes(RAW, b"raw data", s);
    let members = [member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(msg(&result).contains(&texts().core_recovery_already_handled()));
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceGroupTests.UndoLastAsync_GroupRecycleAllPermanent_SaysNothingCanBeRestoredAndClears
#[test]
fn undo_last_async_group_recycle_all_permanent_says_nothing_can_be_restored_and_clears() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let members = [member(JPEG, None, 4, s, true), member(RAW, None, 8, s, true)];
    undo.register_group(&group_result_of(FileOperationType::Recycle, &members));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(
        result.error_message,
        Some(texts().core_undo_permanently_deleted("a.jpg"))
    );
    assert!(!undo.has_last_undo_action());
    assert_eq!(f.bin.restore_calls(), 0);
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteUndoFailsThenRetrySucceeds_LeavesNoRetryableFailedLine
#[test]
fn undo_last_async_failed_group_delete_undo_fails_then_retry_succeeds_leaves_no_retryable_failed_line() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let (jpeg, raw) = jpeg_and_raw();
    let xmp = member(XMP, None, 3, s, false);
    // All three reached the bin; the JPEG comes back, the RAW fails.
    register_failed_delete(
        &f,
        &mut undo,
        &[jpeg.clone(), raw.clone(), xmp.clone()],
        &[jpeg.clone(), raw.clone(), xmp.clone()],
    );
    f.bin.set_mode_for(RAW, BinMode::Fail);

    let first = undo.undo_last();

    assert!(!first.succeeded);
    let still_failed = failed_with_id(&f.journal, "delete-group");
    assert_eq!(still_failed.len(), 1);
    assert_eq!(members_of(&still_failed[0]), vec![raw, xmp]);

    f.bin.clear_modes();
    let second = undo.undo_last();

    assert_succeeded(&second);
    assert!(f.exists(JPEG));
    assert!(f.exists(RAW));
    assert!(f.exists(XMP));
    // Every member is back: a fresh Recovery window must not offer "retry delete" on live files.
    assert!(failed_with_id(&f.journal, "delete-group").is_empty());
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteNewerForeignLineAppearedBeforeSettle_SettleSkipsAndKeepsTheNewerLine
#[test]
fn undo_last_async_failed_group_delete_newer_foreign_line_appeared_before_settle_settle_skips_and_keeps_the_newer_line()
{
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let (jpeg, raw) = jpeg_and_raw();
    let xmp = member(XMP, None, 3, s, false);
    register_failed_delete(
        &f,
        &mut undo,
        &[jpeg.clone(), raw.clone(), xmp.clone()],
        &[jpeg, raw, xmp],
    );
    f.bin.set_mode_for(RAW, BinMode::Fail);
    let _ = undo.undo_last();
    // Another writer (a second process sharing the journal) appends a newer line for the same Id, about other members.
    let foreign = member(r"C:\photos\other.jpg", None, 5, s, false);
    let newer = JournalEntry {
        error: Some("other writer".to_owned()),
        group_id: Some("g".to_owned()),
        group_members: Some(vec![foreign.clone()]),
        ..entry(
            "delete-group",
            FileOperationType::Recycle,
            JournalState::Failed,
            &foreign.source,
            None,
            5,
            s,
        )
    };
    f.append(&newer);
    f.bin.clear_modes();

    let second = undo.undo_last();

    assert_succeeded(&second);
    // Not overwritten.
    assert_eq!(failed_with_id(&f.journal, "delete-group"), vec![newer]);
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteNewerLineIsASubsetOfTheMembers_SettleReanchorsOnItAndClosesIt
#[test]
fn undo_last_async_failed_group_delete_newer_line_is_a_subset_of_the_members_settle_reanchors_on_it_and_closes_it() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let (jpeg, raw) = jpeg_and_raw();
    let xmp = member(XMP, None, 3, s, false);
    register_failed_delete(
        &f,
        &mut undo,
        &[jpeg.clone(), raw.clone(), xmp.clone()],
        &[jpeg, raw, xmp.clone()],
    );
    f.bin.set_mode_for(RAW, BinMode::Fail);
    let _ = undo.undo_last();
    let newer = JournalEntry {
        error: Some("other writer".to_owned()),
        group_id: Some("g".to_owned()),
        group_members: Some(vec![xmp.clone()]),
        ..entry(
            "delete-group",
            FileOperationType::Recycle,
            JournalState::Failed,
            &xmp.source,
            None,
            3,
            s,
        )
    };
    f.append(&newer);
    f.bin.clear_modes();

    let second = undo.undo_last();

    assert_succeeded(&second);
    assert!(failed_with_id(&f.journal, "delete-group").is_empty());
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteSettleAppendCancelled_UndoStillSucceedsAndClearsTheAction
#[test]
fn undo_last_async_failed_group_delete_settle_append_cancelled_undo_still_succeeds_and_clears_the_action() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let (jpeg, raw) = jpeg_and_raw();
    register_failed_delete(&f, &mut undo, &[jpeg.clone(), raw.clone()], &[jpeg, raw]);
    // Prepared, Committed, then the settle.
    let appends = Arc::new(AtomicUsize::new(0));
    let counter = appends.clone();
    f.fs.on_open_append(move |_| {
        let nth = counter.fetch_add(1, Ordering::SeqCst) + 1;
        (nth >= 3).then(|| io::Error::other(Canceled))
    });

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert!(!undo.has_last_undo_action());
    f.fs.reset_step();
    // No tx.Fail after the Commit.
    assert!(
        read_failed_operations(&f.journal)
            .iter()
            .all(|line| line.undo != Some(true))
    );
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteCatchPathSettleCancelled_ReturnsTheFailedResultInsteadOfThrowing
#[test]
fn undo_last_async_failed_group_delete_catch_path_settle_cancelled_returns_the_failed_result_instead_of_throwing() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let (jpeg, raw) = jpeg_and_raw();
    register_failed_delete(&f, &mut undo, &[jpeg.clone(), raw.clone()], &[jpeg, raw]);
    f.bin.set_mode_for(RAW, BinMode::Fail);
    // Prepared, the undo's own Failed line, then the catch-path settle.
    let appends = Arc::new(AtomicUsize::new(0));
    let counter = appends.clone();
    f.fs.on_open_append(move |_| {
        let nth = counter.fetch_add(1, Ordering::SeqCst) + 1;
        (nth >= 3).then(|| io::Error::other(Canceled))
    });

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(result.restored_paths, Some(vec![JPEG.to_owned()]));
}

// C#: UndoServiceGroupTests.UndoLastAsync_FailedGroupDeleteStaleReconcileFailedAfterForeignCommit_SettleAppendsNoLine
#[test]
fn undo_last_async_failed_group_delete_stale_reconcile_failed_after_foreign_commit_settle_appends_no_line() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    let s = group_stamp();
    let (jpeg, raw) = jpeg_and_raw();
    let xmp = member(XMP, None, 3, s, false);
    register_failed_delete(
        &f,
        &mut undo,
        &[jpeg.clone(), raw.clone(), xmp.clone()],
        &[jpeg, raw, xmp.clone()],
    );
    f.bin.set_mode_for(RAW, BinMode::Fail);
    let _ = undo.undo_last();
    // Another process retried the delete (Committed) and a stale reconcile Failed landed after it.
    let retry = JournalEntry {
        group_id: Some("g".to_owned()),
        group_members: Some(vec![xmp.clone()]),
        ..entry(
            "delete-group",
            FileOperationType::Recycle,
            JournalState::Committed,
            &xmp.source,
            None,
            3,
            s,
        )
    };
    f.append(&retry);
    f.append(&JournalEntry {
        state: JournalState::Failed,
        error: Some("stale".to_owned()),
        error_code: Some(PENDING_UNCONFIRMED.to_owned()),
        ..retry.clone()
    });
    f.bin.clear_modes();
    let before = journal_lines_mentioning(&f, "delete-group");

    let second = undo.undo_last();

    assert_succeeded(&second);
    // The settle must not re-anchor on the stale Failed line.
    assert_eq!(before, journal_lines_mentioning(&f, "delete-group"));
}

/// C# `UndoGroupMoveAsync` FAT test state: the members of a group Move to `F:\selected` built by hand, with the odd stamp
/// `Stamp + 1735 ms` as their source stamp and the 2 s-rounded stamp on the FAT files (C# `MoveGroupToFatAsync`).
fn move_group_to_fat(f: &Fixture, undo: &mut UndoService) {
    let odd = plus_ms(group_stamp(), 1735);
    f.add_bytes(FAT_JPEG, b"jpeg", round_to_fat(odd));
    f.add_bytes(FAT_RAW, b"raw data", round_to_fat(odd));
    assert_ne!(f.stamp_of(FAT_JPEG), odd, "precondition: the stamp really was rounded");
    let members = [
        member(JPEG, Some(FAT_JPEG), 4, odd, false),
        member(RAW, Some(FAT_RAW), 8, odd, false),
    ];
    undo.register_group(&group_result_of(FileOperationType::Move, &members));
}

// C#: UndoServiceGroupTests.UndoGroupMoveAsync_DestinationOnFatRoundedStamp_RestoresAllMembers
#[test]
fn undo_group_move_async_destination_on_fat_rounded_stamp_restores_all_members() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    move_group_to_fat(&f, &mut undo);

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert!(f.exists(JPEG));
    assert!(f.exists(RAW));
    assert!(!f.exists(FAT_JPEG));
    assert!(!f.exists(FAT_RAW));
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceGroupTests.UndoGroupMoveAsync_MemberMovedBackByHandFromFat_CountsAsAlreadyRestored
#[test]
fn undo_group_move_async_member_moved_back_by_hand_from_fat_counts_as_already_restored() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    move_group_to_fat(&f, &mut undo);
    // The user put the JPEG back by hand: it keeps the FAT-rounded stamp.
    by_hand_move(&f, FAT_JPEG, JPEG);

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert!(f.exists(JPEG));
    assert!(f.exists(RAW));
    assert!(!f.exists(FAT_RAW));
}

// C#: UndoServiceGroupTests.UndoGroupMoveAsync_FatDestinationStampMovedBy3Seconds_Refuses
#[test]
fn undo_group_move_async_fat_destination_stamp_moved_by_3_seconds_refuses() {
    let f = Fixture::at(group_stamp());
    let mut undo = undo_for(&f);
    move_group_to_fat(&f, &mut undo);
    let odd = plus_ms(group_stamp(), 1735);
    // Same size, written later: another file.
    f.add_bytes(FAT_RAW, b"RAW DATA", plus_secs(odd, 3));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(f.exists(FAT_JPEG));
    assert!(f.exists(FAT_RAW));
    assert!(!f.exists(JPEG));
    assert!(!f.exists(RAW));
}

// ---------------------------------------------------------------------------------------------------------------
// UndoServiceMutationGapTests.cs (23 facts, incl. the Theory with rows None / "")
// ---------------------------------------------------------------------------------------------------------------

const ENTRY_SOURCE: &str = r"C:\photos\entry.jpg";
const ENTRY_DESTINATION: &str = r"C:\photos\selected\entry.jpg";

/// C# `Moved(succeeded, rejected, destination, operation)`: a single result of `JPEG` with size 4 at `group_stamp()`.
fn moved_result(
    succeeded: bool,
    rejected: bool,
    destination: Option<&str>,
    operation: FileOperationType,
) -> FileActionResult {
    let error = if succeeded { None } else { Some("failed".to_owned()) };
    let mut result = FileActionResult::new(
        succeeded,
        operation,
        JPEG,
        destination.map(str::to_owned),
        4,
        group_stamp(),
        error,
    );
    result.rejected = rejected;
    result
}

/// C# `GroupResult(operation, members, entrySource, entryDestination)`: a completed group whose entry is `Committed`.
fn gap_group_result(
    operation: FileOperationType,
    members: &[JournalGroupMember],
    entry_source: Option<&str>,
    entry_destination: Option<&str>,
) -> CaptureGroupActionResult {
    let s = group_stamp();
    let line = JournalEntry {
        group_id: Some("g".to_owned()),
        group_members: Some(members.to_vec()),
        ..entry(
            "g",
            operation,
            JournalState::Committed,
            entry_source.unwrap_or(&members[0].source),
            entry_destination.or(members[0].destination.as_deref()),
            members[0].size,
            s,
        )
    };
    CaptureGroupActionResult::new(
        true,
        false,
        operation,
        "g",
        Some(Box::new(line)),
        members.iter().map(completed).collect(),
        None,
    )
}

// C#: UndoServiceMutationGapTests.TryBegin_WithoutFileActionService_GateFollowsBeginAndEnd
#[test]
fn try_begin_without_file_action_service_gate_follows_begin_and_end() {
    let f = Fixture::at(group_stamp());
    let undo = f.undo();

    assert!(!undo.is_busy());
    assert!(undo.try_begin());
    assert!(undo.is_busy());
    assert!(!undo.try_begin(), "already taken");
    undo.end();
    assert!(!undo.is_busy());
    assert!(undo.try_begin(), "released by end");
}

// C#: UndoServiceMutationGapTests.Register_FailedMoveWithDestination_IsNotUndoable
#[test]
fn register_failed_move_with_destination_is_not_undoable() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();

    undo.register(&moved_result(false, false, Some(MOVED_JPEG), FileOperationType::Move));

    assert!(undo.move_history().is_empty());
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceMutationGapTests.Register_RejectedMoveReportedAsSucceeded_IsNotUndoable
#[test]
fn register_rejected_move_reported_as_succeeded_is_not_undoable() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();

    undo.register(&moved_result(true, true, Some(MOVED_JPEG), FileOperationType::Move));

    assert!(undo.move_history().is_empty());
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceMutationGapTests.Register_FailedRecycle_IsNotUndoable
#[test]
fn register_failed_recycle_is_not_undoable() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();

    undo.register(&moved_result(false, false, None, FileOperationType::Recycle));

    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceMutationGapTests.Register_SucceededCopyWithDestination_IsNotUndoable
#[test]
fn register_succeeded_copy_with_destination_is_not_undoable() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();

    undo.register(&moved_result(true, false, Some(MOVED_JPEG), FileOperationType::Copy));

    assert!(undo.move_history().is_empty());
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceMutationGapTests.Register_SucceededMoveWithoutDestinationPath_IsNotUndoable (Theory: None, "")
#[test]
fn register_succeeded_move_without_destination_path_is_not_undoable() {
    for destination in [None, Some("")] {
        let f = Fixture::at(group_stamp());
        let mut undo = f.undo();

        undo.register(&moved_result(true, false, destination, FileOperationType::Move));

        assert!(undo.move_history().is_empty(), "destination {destination:?}");
        assert!(!undo.has_last_undo_action(), "destination {destination:?}");
    }
}

// C#: UndoServiceMutationGapTests.Register_SucceededMove_BecomesTheUndoableLastAction
#[test]
fn register_succeeded_move_becomes_the_undoable_last_action() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();

    undo.register(&moved_result(true, false, Some(MOVED_JPEG), FileOperationType::Move));

    assert_eq!(undo.move_history().len(), 1);
    assert!(undo.has_last_undo_action());
}

// C#: UndoServiceMutationGapTests.UndoMoveAsync_AfterSuccess_ForgetsTheMoveAsLastAction
#[test]
fn undo_move_async_after_success_forgets_the_move_as_last_action() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    f.add_bytes(MOVED_JPEG, b"jpeg", group_stamp());
    undo.register(&moved_result(true, false, Some(MOVED_JPEG), FileOperationType::Move));

    let result = undo.undo_move();

    assert_succeeded(&result);
    // The undone Move is no longer something Ctrl+Z could undo again.
    assert!(!undo.has_last_undo_action());
    assert!(f.exists(JPEG));
}

// C#: UndoServiceMutationGapTests.UndoMoveAsync_CalledDirectlyWhileARecycleIsTheLastAction_KeepsTheRecycleUndoable
#[test]
fn undo_move_async_called_directly_while_a_recycle_is_the_last_action_keeps_the_recycle_undoable() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    f.add_bytes(MOVED_JPEG, b"jpeg", group_stamp());
    undo.register(&moved_result(true, false, Some(MOVED_JPEG), FileOperationType::Move));
    // The latest action.
    undo.register(&recycled(r"C:\photos\b.jpg", 4, group_stamp()));

    // Undoes the earlier Move only.
    let result = undo.undo_move();

    assert_succeeded(&result);
    assert!(undo.has_last_undo_action());
}

// C#: UndoServiceMutationGapTests.UndoMoveAsync_UsesTheMoveOverrideForTheReverseMove
#[test]
fn undo_move_async_uses_the_move_override_for_the_reverse_move() {
    let f = Fixture::at(group_stamp());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut undo = f.undo().with_move_override(f.recording_override(calls.clone()));
    f.add_bytes(MOVED_JPEG, b"jpeg", group_stamp());
    undo.register(&moved_result(true, false, Some(MOVED_JPEG), FileOperationType::Move));

    let result = undo.undo_move();

    assert_succeeded(&result);
    assert_eq!(
        *calls.lock().unwrap_or_else(|e| e.into_inner()),
        vec![(MOVED_JPEG.to_owned(), JPEG.to_owned())]
    );
    assert!(f.exists(JPEG));
}

// C#: UndoServiceMutationGapTests.UndoMoveAsync_UsesTheInjectedClockForItsJournalLines
#[test]
fn undo_move_async_uses_the_injected_clock_for_its_journal_lines() {
    let f = Fixture::at(group_stamp());
    let later = plus_hours(group_stamp(), 3);
    let mut undo = f.undo_with_clock(Arc::new(FakeClock::at(later)));
    f.add_bytes(MOVED_JPEG, b"jpeg", group_stamp());
    undo.register(&moved_result(true, false, Some(MOVED_JPEG), FileOperationType::Move));
    f.fs.on_move(|source, _| (source == Path::new(MOVED_JPEG)).then(|| io::Error::other("simulated")));

    let result = undo.undo_move();

    assert!(!result.succeeded);
    let failed = read_failed_operations(&f.journal);
    assert_eq!(failed.len(), 1);
    // The undo's own clock stamps its lines, not the journal's clock.
    assert_eq!(failed[0].timestamp_utc.value, later);
}

// C#: UndoServiceMutationGapTests.UndoMoveAsync_HistoryFilledDirectlyAndMoveJournaled_UsesTheJournalFingerprint
// Rust: the committed Move line is appended by hand (C# makes it with FileActionService.ExecuteAsync), the file is moved by hand.
#[test]
fn undo_move_async_history_filled_directly_and_move_journaled_uses_the_journal_fingerprint() {
    let f = Fixture::at(group_stamp());
    let s = group_stamp();
    f.add(JPEG, 4, s);
    // C# ExecuteAsync creates the destination folder; by hand here.
    f.disk.add_dir(r"C:\photos\selected");
    by_hand_move(&f, JPEG, MOVED_JPEG);
    f.append(&committed_move("moved", JPEG, MOVED_JPEG, 4, s));
    let mut undo = f.undo_with_clock(Arc::new(FakeClock::at(plus_hours(s, 3))));
    // Nothing registered: the history is filled directly.
    undo.move_history_mut().push((JPEG.to_owned(), MOVED_JPEG.to_owned()));

    let result = undo.undo_move();

    assert_succeeded(&result);
    assert!(f.exists(JPEG));
    assert!(!f.exists(MOVED_JPEG));
}

// C#: UndoServiceMutationGapTests.UndoMoveAsync_HistoryFilledDirectlyWithoutAJournalLine_FailsAsFingerprintMissingAndKeepsTheEntry
#[test]
fn undo_move_async_history_filled_directly_without_a_journal_line_fails_as_fingerprint_missing_and_keeps_the_entry() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    f.add_bytes(MOVED_JPEG, b"jpeg", group_stamp());
    undo.move_history_mut().push((JPEG.to_owned(), MOVED_JPEG.to_owned()));

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert_eq!(
        result.error_message,
        Some(texts().core_undo_failed(&texts().core_undo_fingerprint_missing()))
    );
    // Pushed back: nothing was proven, nothing was dropped.
    assert_eq!(undo.move_history().len(), 1);
    assert!(f.exists(MOVED_JPEG));
    assert!(!f.exists(JPEG));
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_RecycleWhileAnotherOperationRuns_IsRejectedAndRestoresNothing
#[test]
fn undo_last_async_recycle_while_another_operation_runs_is_rejected_and_restores_nothing() {
    let f = Fixture::at(group_stamp());
    let gate = TestGate::new();
    let mut undo = f.undo().with_gate(gate.clone());
    undo.register(&single_result(
        true,
        FileOperationType::Recycle,
        JPEG,
        None,
        4,
        group_stamp(),
    ));
    // A file action is in flight.
    assert!(gate.try_begin());

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(result.rejected);
    assert_eq!(result.error_message, Some(texts().core_undo_busy()));
    assert_eq!(f.bin.restore_calls(), 0);
    // Still undoable once the gate is free.
    assert!(undo.has_last_undo_action());
    gate.end();
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_GroupMoveWhenTheMovedFileIsGone_FailsAndDoesNotCountItAsRestored
#[test]
fn undo_last_async_group_move_when_the_moved_file_is_gone_fails_and_does_not_count_it_as_restored() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[member(JPEG, Some(MOVED_JPEG), 4, group_stamp(), false)],
        None,
        None,
    ));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(
        result.error_message,
        Some(texts().core_undo_failed(&texts().core_undo_destination_changed_after_move()))
    );
    assert_eq!(result.restored_paths, None);
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_GroupMoveWhenADifferentFileSitsAtTheOriginalPath_FailsAndLeavesThatFileAlone
#[test]
fn undo_last_async_group_move_when_a_different_file_sits_at_the_original_path_fails_and_leaves_that_file_alone() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    // Not the moved file: another length.
    f.add_bytes(JPEG, b"someone else's file", group_stamp());
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[member(JPEG, Some(MOVED_JPEG), 4, group_stamp(), false)],
        None,
        None,
    ));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(
        result.error_message,
        Some(texts().core_undo_failed(&texts().core_undo_destination_changed_after_move()))
    );
    assert_eq!(result.restored_paths, None);
    assert_eq!(f.size_of(JPEG), 19);
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_GroupMoveWhoseSourceIsKeptByTheMove_FailsVerificationInsteadOfClaimingSuccess
#[test]
fn undo_last_async_group_move_whose_source_is_kept_by_the_move_fails_verification_instead_of_claiming_success() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    f.add_bytes(MOVED_JPEG, b"jpeg", group_stamp());
    // The reverse move copies but cannot remove the file at the move destination.
    f.fs.set_move_leaves_source(true);
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[member(JPEG, Some(MOVED_JPEG), 4, group_stamp(), false)],
        None,
        None,
    ));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    // C# `JournalCodedException(VerifySizeChanged).Message`: the journal's localized text of that code.
    let verify_text = f.journal.texts().localized("core.fileAction.verifyFailedSizeChanged");
    assert_eq!(result.error_message, Some(texts().core_undo_failed(&verify_text)));
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_GroupMoveFailedWithoutRestoringAnything_DoesNotRememberAPartialUndo
#[test]
fn undo_last_async_group_move_failed_without_restoring_anything_does_not_remember_a_partial_undo() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    // Not the moved RAW: the pre-scan fails before anything moves.
    f.add_bytes(MOVED_RAW, b"raw edited elsewhere", s);
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[
            member(JPEG, Some(MOVED_JPEG), 4, s, false),
            member(RAW, Some(MOVED_RAW), 8, s, false),
        ],
        None,
        None,
    ));
    let first = undo.undo_last();
    assert!(!first.succeeded);
    assert!(!f.exists(JPEG), "nothing was restored by this undo");

    // The user then puts everything back by hand.
    f.add_bytes(MOVED_RAW, b"raw data", s);
    by_hand_move(&f, MOVED_JPEG, JPEG);
    by_hand_move(&f, MOVED_RAW, RAW);
    let second = undo.undo_last();

    // Nothing was restored by the earlier attempt, so this is "already handled", not a completed partial undo.
    assert!(!second.succeeded);
    assert_eq!(
        second.error_message,
        Some(texts().core_undo_failed(&texts().core_recovery_already_handled()))
    );
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_GroupMoveFailsTwiceAfterPartialRestore_RetryClosesBothFailedUndoLines
#[test]
fn undo_last_async_group_move_fails_twice_after_partial_restore_retry_closes_both_failed_undo_lines() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    f.add_bytes(MOVED_RAW, b"raw data", s);
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[
            member(JPEG, Some(MOVED_JPEG), 4, s, false),
            member(RAW, Some(MOVED_RAW), 8, s, false),
        ],
        None,
        None,
    ));
    f.fs.fail_move_of(MOVED_RAW, "raw busy");

    // JPEG restored, RAW fails: first Failed line.
    let first = undo.undo_last();
    // JPEG already back, RAW fails again: second Failed line.
    let second = undo.undo_last();
    assert!(!first.succeeded);
    assert!(!second.succeeded);
    assert_eq!(read_failed_operations(&f.journal).len(), 2);

    f.fs.reset_step();
    let third = undo.undo_last();

    assert_succeeded(&third);
    // Both earlier Failed lines were closed, not just the latest.
    assert!(read_failed_operations(&f.journal).is_empty());
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_GroupMoveSuccess_DropsTheFingerprintOfEveryMemberDestination
#[test]
fn undo_last_async_group_move_success_drops_the_fingerprint_of_every_member_destination() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    f.add_bytes(MOVED_JPEG, b"jpeg", group_stamp());
    // A single Move registered earlier left a fingerprint for the same destination.
    undo.register(&moved_result(true, false, Some(MOVED_JPEG), FileOperationType::Move));
    assert_eq!(undo.move_fingerprint_count(), 1);
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[member(JPEG, Some(MOVED_JPEG), 4, group_stamp(), false)],
        None,
        None,
    ));

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert_eq!(undo.move_fingerprint_count(), 0);
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_GroupMove_ReportsTheJournalEntrySourceAndDestinationWhenPresent
#[test]
fn undo_last_async_group_move_reports_the_journal_entry_source_and_destination_when_present() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    f.add_bytes(MOVED_JPEG, b"jpeg", group_stamp());
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[member(JPEG, Some(MOVED_JPEG), 4, group_stamp(), false)],
        Some(ENTRY_SOURCE),
        Some(ENTRY_DESTINATION),
    ));

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert_eq!(result.source, ENTRY_SOURCE);
    assert_eq!(result.destination.as_deref(), Some(ENTRY_DESTINATION));
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_GroupRecycleOfNormalMembers_RestoresAllWithoutAnyNote
#[test]
fn undo_last_async_group_recycle_of_normal_members_restores_all_without_any_note() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    undo.register_group(&gap_group_result(
        FileOperationType::Recycle,
        &[member(JPEG, None, 4, s, false), member(RAW, None, 8, s, false)],
        None,
        None,
    ));

    let result = undo.undo_last();

    assert_succeeded(&result);
    // No "partially restored" warning when nothing was permanent.
    assert_eq!(result.error_message, None);
    assert_eq!(result.restored_paths, Some(vec![JPEG.to_owned(), RAW.to_owned()]));
}

// C#: UndoServiceMutationGapTests.UndoLastAsync_FailedGroupDeleteWhereNothingCanBeRestored_ReportsNoRestoredPathsAndLeavesTheFailedLineAlone
#[test]
fn undo_last_async_failed_group_delete_where_nothing_can_be_restored_reports_no_restored_paths_and_leaves_the_failed_line_alone()
 {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    let jpeg = member(JPEG, None, 4, s, false);
    let raw = member(RAW, None, 8, s, false);
    let failed_line = JournalEntry {
        error: Some("boom".to_owned()),
        group_id: Some("g".to_owned()),
        group_members: Some(vec![jpeg.clone(), raw.clone()]),
        ..entry(
            "delete-group",
            FileOperationType::Recycle,
            JournalState::Failed,
            JPEG,
            None,
            4,
            s,
        )
    };
    f.append(&failed_line);
    let mut raw_failed = CaptureGroupMemberResult::new(raw.clone(), false, false);
    raw_failed.error = Some("boom".to_owned());
    raw_failed.source_exists = true;
    undo.register_group(&CaptureGroupActionResult::new(
        false,
        false,
        FileOperationType::Recycle,
        "g",
        Some(Box::new(failed_line)),
        vec![completed(&jpeg), raw_failed],
        Some("boom".to_owned()),
    ));
    // The one recycled member cannot be restored.
    f.bin.set_mode_for(JPEG, BinMode::Fail);

    let result = undo.undo_last();

    assert!(!result.succeeded);
    // Nothing came back: null, not an empty list.
    assert_eq!(result.restored_paths, None);
    // Nothing was restored, so the Failed delete line is not rewritten (a rewrite would carry the undo's later stamp).
    let delete = failed_with_id(&f.journal, "delete-group");
    assert_eq!(delete.len(), 1);
    assert_eq!(delete[0].timestamp_utc.value, s);
    assert_eq!(members_of(&delete[0]), vec![jpeg, raw]);
}

// ---------------------------------------------------------------------------------------------------------------
// UndoServiceSafetyTests.cs (5 facts)
// ---------------------------------------------------------------------------------------------------------------

// C#: UndoServiceSafetyTests.UndoLast_Recycle_BinThrows_FailsTruthfullyAndStaysRetryable
// The bin's shell error is an io::Error (RecycleBin::try_restore returns io::Result<bool>, P-13): the C# throw ports as Err.
#[test]
fn undo_last_recycle_bin_throws_fails_truthfully_and_stays_retryable() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    f.bin.set_default(BinMode::Throw("bin exploded".to_owned()));
    undo.register(&recycled(JPEG, 4, group_stamp()));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(!result.rejected);
    assert!(msg(&result).contains("bin exploded"));
    assert!(undo.has_last_undo_action());
    assert!(!undo.is_busy());

    f.bin.set_default(BinMode::Restore);
    let retry = undo.undo_last();

    assert!(retry.succeeded);
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceSafetyTests.UndoLast_Recycle_BinRestoresThenThrows_Succeeds
#[test]
fn undo_last_recycle_bin_restores_then_throws_succeeds() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    f.bin.set_default(BinMode::RestoreThenThrow("bin exploded".to_owned()));
    undo.register(&recycled(JPEG, 4, group_stamp()));

    let result = undo.undo_last();

    // The file is the proof.
    assert!(result.succeeded);
    assert!(!undo.has_last_undo_action());
}

// C#: UndoServiceSafetyTests.UndoLast_WhileBusy_IsRejectedAndKeepsState
#[test]
fn undo_last_while_busy_is_rejected_and_keeps_state() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    undo.register(&recycled(JPEG, 4, group_stamp()));
    assert!(undo.try_begin());

    let result = undo.undo_last();

    assert!(result.rejected);
    assert!(!result.succeeded);
    assert!(undo.has_last_undo_action());
    undo.end();
}

// C#: UndoServiceSafetyTests.UndoLast_WhileBusy_PermanentOrEmpty_IsRejectedNotMutated
#[test]
fn undo_last_while_busy_permanent_or_empty_is_rejected_not_mutated() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    assert!(undo.try_begin());

    // Empty history: busy is reported before anything else is read.
    assert!(undo.undo_last().rejected);
    let mut permanent = recycled(JPEG, 4, group_stamp());
    permanent.permanently_deleted = true;
    undo.register(&permanent);
    assert!(undo.undo_last().rejected);
    assert!(undo.has_last_undo_action());
    undo.end();
}

// C#: UndoServiceSafetyTests.UndoMove_VerifyThrowsAfterFileReturned_DropsEntry
#[test]
fn undo_move_verify_throws_after_file_returned_drops_entry() {
    let f = Fixture::at(group_stamp());
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"abcd", s);
    // The move override moves the file back, then rewrites it with 8 bytes: VerifyMoved throws after the file returned.
    let disk = f.disk.clone();
    let over: MoveOverride = Arc::new(move |from: &Path, to: &Path| -> io::Result<()> {
        FileSystem::move_file(disk.as_ref(), from, to)?;
        disk.add_file_at(to, b"abcdefgh", s);
        Ok(())
    });
    let mut undo = f.undo().with_move_override(over);
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        JPEG,
        Some(MOVED_JPEG),
        4,
        s,
    ));

    let first = undo.undo_last();

    assert!(!first.succeeded);
    assert!(undo.move_history().is_empty());
    assert!(!undo.has_last_undo_action());
    let second = undo.undo_move();
    assert_eq!(second.error_message, Some(texts().core_undo_no_move_to_undo()));
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionMutationAuditBTests.cs: the undo tests of the file (5 facts)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionMutationAuditBTests.UndoMove_Group_SecondFileReplacedAfterPreflight_NothingIsRestored (M21b)
#[test]
fn undo_move_group_second_file_replaced_after_preflight_nothing_is_restored() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    // Arrange: a Move of two files, both at their destination, undo registered.
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    f.add_bytes(MOVED_RAW, b"raw!", s);
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[
            member(JPEG, Some(MOVED_JPEG), 4, s, false),
            member(RAW, Some(MOVED_RAW), 4, s, false),
        ],
        None,
        None,
    ));
    // The preflight passed; a foreign file (same size, other write time) takes the second path just as Prepared is journaled.
    let replaced = Arc::new(AtomicBool::new(false));
    let flag = replaced.clone();
    let later = plus_hours(s, 2);
    f.fs.plant_at_next_append(Action::Run(Arc::new(move |disk: &FakeFileSystem| {
        flag.store(true, Ordering::SeqCst);
        disk.add_file_at(MOVED_RAW, b"evil", later);
    })));

    let result = undo.undo_last();

    // The whole undo fails BEFORE the first move; no partial capture, the foreign file is untouched.
    assert!(replaced.load(Ordering::SeqCst));
    assert!(!result.succeeded);
    assert!(!f.exists(JPEG));
    assert!(f.exists(MOVED_JPEG));
    assert!(!f.exists(RAW));
    assert_eq!(f.stamp_of(MOVED_RAW), later);
}

// C#: FileActionMutationAuditBTests.UndoMove_Group_SecondFileReplacedAfterTheFirstMove_ForeignFileIsNeverRestored (M21c)
#[test]
fn undo_move_group_second_file_replaced_after_the_first_move_foreign_file_is_never_restored() {
    let f = Fixture::at(group_stamp());
    let s = group_stamp();
    // The preflight and the up-front pass succeed; the replacement happens while the first file is being moved.
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    f.add_bytes(MOVED_RAW, b"raw!", s);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let disk = f.disk.clone();
    let later = plus_hours(s, 2);
    let over: MoveOverride = Arc::new(move |from: &Path, to: &Path| -> io::Result<()> {
        let call = counter.fetch_add(1, Ordering::SeqCst) + 1;
        FileSystem::move_file(disk.as_ref(), from, to)?;
        if call == 1 {
            disk.add_file_at(MOVED_RAW, b"evil", later);
        }
        Ok(())
    });
    let mut undo = f.undo().with_move_override(over);
    undo.register_group(&gap_group_result(
        FileOperationType::Move,
        &[
            member(JPEG, Some(MOVED_JPEG), 4, s, false),
            member(RAW, Some(MOVED_RAW), 4, s, false),
        ],
        None,
        None,
    ));

    let result = undo.undo_last();

    // The foreign file stays where it is, nothing is restored under this undo's name.
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!result.succeeded);
    assert!(!f.exists(RAW));
    assert_eq!(f.stamp_of(MOVED_RAW), later);
}

// C#: FileActionMutationAuditBTests.UndoRecycle_Single_BinPutsBackSameSizeFileWithOtherWriteTime_IsNotAnUndoAndStaysRetryable (M18)
#[test]
fn undo_recycle_single_bin_puts_back_same_size_file_with_other_write_time_is_not_an_undo_and_stays_retryable() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    // The bin "restores" a DIFFERENT file of the same size: the recycled file (size AND write time) is not at the path.
    f.bin.set_default(BinMode::RestoreOtherStamp);
    undo.register(&recycled(JPEG, 4, s));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(f.bin.restore_calls(), 1);
    // Still retryable.
    assert!(undo.has_last_undo_action());
}

// C#: FileActionMutationAuditBTests.UndoRecycle_Single_BinPutsBackTheRecycledFile_IsASuccessfulUndo (M18)
#[test]
fn undo_recycle_single_bin_puts_back_the_recycled_file_is_a_successful_undo() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    undo.register(&recycled(JPEG, 4, s));

    let result = undo.undo_last();

    assert_succeeded(&result);
    assert!(!undo.has_last_undo_action());
}

// C#: FileActionMutationAuditBTests.UndoRecycle_Group_BinPutsBackSameSizeFileWithOtherWriteTime_IsNotAnUndo (M18b)
#[test]
fn undo_recycle_group_bin_puts_back_same_size_file_with_other_write_time_is_not_an_undo() {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    f.bin.set_default(BinMode::RestoreOtherStamp);
    undo.register_group(&gap_group_result(
        FileOperationType::Recycle,
        &[member(JPEG, None, 4, s, false), member(RAW, None, 4, s, false)],
        None,
        None,
    ));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert!(result.restored_paths.as_ref().is_none_or(Vec::is_empty));
    assert!(undo.has_last_undo_action());
}

// ---------------------------------------------------------------------------------------------------------------
// RecoveryIdentityTests.cs: the R18 undo tests (4 facts; the Theories loop over their rows)
// ---------------------------------------------------------------------------------------------------------------

// C#: RecoveryIdentityTests.UndoMove_DestinationReplacedAfterThePreflight_IsRefusedAndTheReplacementIsNotRestored
#[test]
fn undo_move_destination_replaced_after_the_preflight_is_refused_and_the_replacement_is_not_restored() {
    let f = Fixture::at(group_stamp());
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"jjjj", s);
    let mut undo = f.undo();
    undo.register(&single_result(
        true,
        FileOperationType::Move,
        JPEG,
        Some(MOVED_JPEG),
        4,
        s,
    ));
    // Replaced just as the Prepared line is journaled: 9 bytes, other write time.
    f.fs.plant_at_next_append(Action::Add {
        path: MOVED_JPEG.to_owned(),
        size: 9,
        stamp: plus_hours(s, 3),
    });

    let result = undo.undo_move();

    assert!(!result.succeeded);
    assert!(!f.exists(JPEG));
    assert_eq!(f.size_of(MOVED_JPEG), 9);
    assert!(
        read_committed_moves(&f.journal)
            .iter()
            .all(|line| line.undo != Some(true))
    );
}

// C#: RecoveryIdentityTests.UndoGroupMove_MemberReplacedAfterThePreflight_NoReplacementIsRestoredOrCommitted
#[test]
fn undo_group_move_member_replaced_after_the_preflight_no_replacement_is_restored_or_committed() {
    let f = Fixture::at(group_stamp());
    let s = group_stamp();
    f.add_bytes(MOVED_JPEG, b"jpeg", s);
    f.add_bytes(MOVED_RAW, b"raw data", s);
    let mut undo = f.undo();
    let members = [
        member(JPEG, Some(MOVED_JPEG), 4, s, false),
        member(RAW, Some(MOVED_RAW), 8, s, false),
    ];
    let line = JournalEntry {
        group_id: Some("g".to_owned()),
        group_members: Some(members.to_vec()),
        ..entry(
            "g",
            FileOperationType::Move,
            JournalState::Committed,
            JPEG,
            Some(MOVED_JPEG),
            4,
            s,
        )
    };
    undo.register_group(&CaptureGroupActionResult::new(
        true,
        false,
        FileOperationType::Move,
        "g",
        Some(Box::new(line)),
        members.iter().map(completed).collect(),
        None,
    ));
    // Same size, other write time, planted at the Prepared line.
    f.fs.plant_at_next_append(Action::Add {
        path: MOVED_RAW.to_owned(),
        size: 8,
        stamp: plus_hours(s, 3),
    });

    let result = undo.undo_last();

    assert!(!result.succeeded);
    // The replacement never reached the original path.
    assert!(!f.exists(RAW));
    assert_eq!(f.stamp_of(MOVED_RAW), plus_hours(s, 3));
    assert!(
        read_committed_moves(&f.journal)
            .iter()
            .all(|line| line.undo != Some(true))
    );
}

// C#: RecoveryIdentityTests.UndoRecycle_BinReportsRestoreWithoutTheRecycledFileAtThePath_IsAFailure (Theory: wrong size true/false)
#[test]
fn undo_recycle_bin_reports_restore_without_the_recycled_file_at_the_path_is_a_failure() {
    for plant_wrong_size in [false, true] {
        let f = Fixture::at(group_stamp());
        let mut undo = f.undo();
        undo.register(&recycled(JPEG, 10, group_stamp()));
        let disk = f.disk.clone();
        let plant: PlantFn = Arc::new(move |path: &Path, _size: u64, stamp: DateTimeUtc| {
            if plant_wrong_size {
                disk.add_file_at(path, b"fff", stamp);
            }
        });
        f.bin.plant_instead_of_restore(Some(plant));

        let result = undo.undo_last();

        assert!(!result.succeeded, "plant_wrong_size={plant_wrong_size}");
        // Still undoable, not forgotten.
        assert!(undo.has_last_undo_action(), "plant_wrong_size={plant_wrong_size}");
    }
}

// C#: RecoveryIdentityTests.UndoGroupRecycle_BinReportsRestoreButAWrongSizeFileSitsAtTheMember_IsAFailureWithNothingReportedRestored
#[test]
fn undo_group_recycle_bin_reports_restore_but_a_wrong_size_file_sits_at_the_member_is_a_failure_with_nothing_reported_restored()
 {
    let f = Fixture::at(group_stamp());
    let mut undo = f.undo();
    let s = group_stamp();
    let members = [member(JPEG, None, 10, s, false)];
    undo.register_group(&gap_group_result(FileOperationType::Recycle, &members, None, None));
    let disk = f.disk.clone();
    let plant: PlantFn = Arc::new(move |path: &Path, _size: u64, stamp: DateTimeUtc| {
        disk.add_file_at(path, b"fff", stamp);
    });
    f.bin.plant_instead_of_restore(Some(plant));

    let result = undo.undo_last();

    assert!(!result.succeeded);
    assert_eq!(result.restored_paths, None);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionSafetyEdgeTests.cs (1 undo fact)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionSafetyEdgeTests.UndoRecycle_FileSystemThrows_GateIsReleased
// C# `FileExists` throws IOException; the Rust `file_exists` cannot fail, so the fake panics instead (`set_probe_panics`).
#[test]
fn undo_recycle_file_system_throws_gate_is_released() {
    let f = Fixture::at(group_stamp());
    let gate = TestGate::new();
    let mut undo = f.undo().with_gate(gate.clone());
    undo.register(&recycled(JPEG, 5, group_stamp()));
    f.fs.set_probe_panics(true);

    let outcome = catch_unwind(AssertUnwindSafe(|| undo.undo_last()));

    let payload = outcome.expect_err("a throwing probe must propagate out of undo_last");
    let text = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        text.contains("simulated probe failure"),
        "panicked for another reason: {text}"
    );
    assert!(
        !gate.is_busy(),
        "a throwing probe left the file-action gate locked for the rest of the session"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// JournalErrorCodeTests.cs / en.json (2 facts)
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalErrorCodeTests.UndoMessages_English
#[test]
fn undo_messages_english() {
    let f = Fixture::new();
    let mut undo = f.undo();

    let nothing = undo.undo_last();
    let no_move = undo.undo_move();

    assert_eq!(
        nothing.error_message.as_deref(),
        Some("There is no recent Move/Delete to undo.")
    );
    assert_eq!(no_move.error_message.as_deref(), Some("There is no Move to undo."));
}

// Port-specific (contract "Test fakes" / "C# tests to port"): every English undo text equals its en.json template.
#[test]
fn english_undo_texts_match_en_json() {
    let path = pr_test_support::golden::repo_root()
        .join("assets")
        .join("languages")
        .join("en.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("en.json parses");
    let key = |name: &str| -> String {
        json[name]
            .as_str()
            .unwrap_or_else(|| panic!("en.json has no string key {name}"))
            .to_owned()
    };
    let t = EnglishUndoTexts;

    assert_eq!(t.core_undo_busy(), key("core.undo.busy"));
    assert_eq!(t.core_undo_no_move_to_undo(), key("core.undo.noMoveToUndo"));
    assert_eq!(t.core_undo_nothing_to_undo(), key("core.undo.nothingToUndo"));
    assert_eq!(
        t.core_undo_source_or_destination_changed(),
        key("core.undo.sourceOrDestinationChanged")
    );
    assert_eq!(t.core_undo_fingerprint_missing(), key("core.undo.fingerprintMissing"));
    assert_eq!(
        t.core_undo_destination_changed_after_move(),
        key("core.undo.destinationChangedAfterMove")
    );
    assert_eq!(
        t.core_undo_failed("M"),
        key("core.undo.failed").replace("{message}", "M")
    );
    assert_eq!(
        t.core_undo_permanently_deleted("F"),
        key("core.undo.permanentlyDeleted").replace("{fileName}", "F")
    );
    assert_eq!(
        t.core_undo_recycle_target_exists("F"),
        key("core.undo.recycleTargetExists").replace("{fileName}", "F")
    );
    assert_eq!(
        t.core_undo_recycle_restore_failed("F"),
        key("core.undo.recycleRestoreFailed").replace("{fileName}", "F")
    );
    assert_eq!(t.core_undo_unsupported(), key("core.undo.unsupported"));
    assert_eq!(
        t.core_undo_group_partially_restored(2, 3, "N"),
        key("core.undo.groupPartiallyRestored")
            .replace("{restored}", "2")
            .replace("{total}", "3")
            .replace("{names}", "N")
    );
    assert_eq!(t.core_recovery_already_handled(), key("core.recovery.alreadyHandled"));
}

// ---------------------------------------------------------------------------------------------------------------
// LiveOperationMarkerTests.cs (1 undo fact, Theory: move fails true/false)
// ---------------------------------------------------------------------------------------------------------------

/// A live-operation registry that also records every id it was asked to mark (C# `SnapshotRegistry`).
struct RecordingRegistry {
    inner: InProcessLiveOperationRegistry,
    begun: Mutex<HashSet<String>>,
}

impl RecordingRegistry {
    fn new() -> Self {
        Self {
            inner: InProcessLiveOperationRegistry::new(),
            begun: Mutex::new(HashSet::new()),
        }
    }

    /// The ids that were begun and are live right now.
    fn live_now(&self) -> HashSet<String> {
        let begun = self.begun.lock().unwrap_or_else(|e| e.into_inner());
        begun.iter().filter(|id| self.inner.is_live(id)).cloned().collect()
    }
}

impl LiveOperationRegistry for RecordingRegistry {
    fn begin(&self, operation_id: &str) -> LiveMarker {
        self.begun
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(operation_id.to_owned());
        self.inner.begin(operation_id)
    }

    fn is_live(&self, operation_id: &str) -> bool {
        self.inner.is_live(operation_id)
    }
}

// C#: LiveOperationMarkerTests.Undo_Move_MarkerCoversPreparedToOutcome (Q-R27; Theory moveFails false/true)
#[test]
fn undo_move_marker_covers_prepared_to_outcome() {
    for move_fails in [false, true] {
        let registry = Arc::new(RecordingRegistry::new());
        let f = Fixture::with_live_registry(clock_now(), registry.clone());
        let live_at_append: Arc<Mutex<Vec<HashSet<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let (snapshot_registry, appended) = (registry.clone(), live_at_append.clone());
        f.fs.on_open_append(move |_| {
            appended
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(snapshot_registry.live_now());
            None
        });
        let source = r"C:\photos\a.jpg";
        let destination = r"C:\photos\sel\a.jpg";
        let write = write_time();
        f.add_bytes(destination, b"12345", write);
        let mut undo = undo_for(&f);
        undo.register(&single_result(
            true,
            FileOperationType::Move,
            source,
            Some(destination),
            5,
            write,
        ));

        let during: Arc<Mutex<Option<(String, bool)>>> = Arc::new(Mutex::new(None));
        let slot = during.clone();
        let journal = f.journal.clone();
        let live_registry = registry.clone();
        f.fs.on_move(move |_, _| {
            let pending = read_pending_operations(&journal);
            assert_eq!(pending.len(), 1, "one pending undo line during the move");
            let id = pending[0].id.clone();
            let live = live_registry.is_live(&id);
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, live));
            move_fails.then(|| io::Error::other("locked"))
        });

        let result = undo.undo_move();

        assert_eq!(result.succeeded, !move_fails, "move_fails={move_fails}");
        let (id, live) = during
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .expect("the move hook ran");
        assert!(live, "the undo's Id is live during the move");
        assert!(!registry.is_live(&id), "the marker is released after the outcome");
        // Every appended line was written while its marker was live.
        let lines: Vec<JournalEntry> = f
            .journal_text()
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| journal_line::try_parse(line).expect("journal line parses"))
            .collect();
        let at_append = live_at_append.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(lines.len(), 2, "Prepared and the outcome");
        assert_eq!(at_append.len(), lines.len());
        for (index, line) in lines.iter().enumerate() {
            assert!(
                at_append[index].contains(&line.id),
                "journal line {index} ({:?}) was appended while its operation had no live marker",
                line.state
            );
        }
        let expected_state = if move_fails {
            JournalState::Failed
        } else {
            JournalState::Committed
        };
        assert_eq!(lines[1].state, expected_state);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// PermanentDeleteTests.cs (2 undo facts)
// ---------------------------------------------------------------------------------------------------------------

const PHOTO: &str = r"E:\usb\a.jpg";

// C#: PermanentDeleteTests.Undo_AfterPermanentDelete_SaysCannotRestore
// C# builds the result with FileActionService.ExecuteAsync (W3-04): built by hand here (permanently_deleted: true).
#[test]
fn undo_after_permanent_delete_says_cannot_restore() {
    let stamp = utc(2026, 9, 25, 10, 0, 0);
    let f = Fixture::at(stamp);
    let mut undo = f.undo();
    let mut result = single_result(true, FileOperationType::Recycle, PHOTO, None, 10, stamp);
    result.permanently_deleted = true;
    undo.register(&result);

    let undo_result = undo.undo_last();

    assert!(!undo_result.succeeded);
    assert_eq!(
        undo_result.error_message,
        Some(texts().core_undo_permanently_deleted("a.jpg"))
    );
    assert_eq!(f.bin.restore_calls(), 0);
}

// C#: PermanentDeleteTests.Undo_AfterNormalRecycle_StillRestores
// C# builds the result with FileActionService.ExecuteAsync (W3-04): built by hand here (permanently_deleted: false).
#[test]
fn undo_after_normal_recycle_still_restores() {
    let stamp = utc(2026, 9, 25, 10, 0, 0);
    let f = Fixture::at(stamp);
    let mut undo = f.undo();
    undo.register(&single_result(true, FileOperationType::Recycle, PHOTO, None, 10, stamp));

    let undo_result = undo.undo_last();

    assert!(undo_result.succeeded, "{:?}", undo_result.error_message);
    assert_eq!(f.bin.restore_calls(), 1);
}
