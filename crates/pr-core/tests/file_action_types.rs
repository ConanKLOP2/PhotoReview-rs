//! Tests for `pr_core::file_actions::types` (W1-09). Port of `PhotoReview.Core/FileActions/{FileFingerprint,
//! FileActionRequest,FileActionResult,CaptureGroupActionResult,UndoResult,JournalGroupMember}.cs` (C# baseline e441d67a).
//!
//! `CaptureGroupActionRequest::new` is not covered here: it takes a `CaptureGroup`, whose constructor belongs to W1-07
//! and is still a stub on this branch. Add its `record_*` test once W1-07 is merged.

use pr_core::abstractions::FileStat;
use pr_core::file_actions::journal_line::JournalDateTime;
use pr_core::file_actions::types::{
    CaptureGroupActionResult, CaptureGroupMemberResult, FileActionRequest, FileActionResult, JournalGroupMember,
    UndoResult, matches_moved_destination,
};
use pr_core::model::FileOperationType;
use pr_core::time::DateTimeUtc;

/// 2026-09-29T08:00:00Z in .NET ticks (Unix 1790668800 + 62135596800 seconds to 0001-01-01, times 10^7).
const STAMP_TICKS: i64 = 639_262_656_000_000_000;

fn stamp() -> DateTimeUtc {
    DateTimeUtc::from_ticks(STAMP_TICKS).expect("stamp is inside the DateTime range")
}

fn sample_member() -> JournalGroupMember {
    JournalGroupMember {
        source: r"C:\photos\a.jpg".to_owned(),
        destination: Some(r"D:\moved\a.jpg".to_owned()),
        size: 10,
        last_write_utc: JournalDateTime::utc(stamp()),
        permanent: false,
    }
}

fn sample_member_result(source: &str) -> CaptureGroupMemberResult {
    let mut member = sample_member();
    member.source = source.to_owned();
    CaptureGroupMemberResult::new(member, true, false)
}

// ---------------------------------------------------------------------------------------------------------------
// FileFingerprint.MatchesMovedDestination
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionsSmallMutationGapTests.MatchesMovedDestination_WriteTimeAtTheToleranceBoundary_IsAcceptedOnlyUpToTheTolerance
#[test]
fn matches_moved_destination_write_time_at_tolerance_boundary_is_accepted_only_up_to_tolerance() {
    // (offset in ms, extra ticks, expected)
    let rows: [(i64, i64, bool); 7] = [
        (0, 0, true),
        (0, 1, true),
        (0, -1, true),
        (2000, 0, true),  // exactly the tolerance, later: still the same file
        (-2000, 0, true), // exactly the tolerance, earlier
        (2000, 1, false), // one tick beyond
        (-2000, -1, false),
    ];
    for (offset_ms, extra_ticks, expected) in rows {
        let ticks = offset_ms * 10_000 + extra_ticks;
        let stat = FileStat {
            length: 10,
            last_write_utc: DateTimeUtc::from_ticks(STAMP_TICKS + ticks).expect("row tick value is in range"),
        };
        assert_eq!(
            matches_moved_destination(&stat, 10, stamp()),
            expected,
            "offset {offset_ms} ms, extra {extra_ticks} ticks"
        );
    }
}

// C#: FileActionsSmallMutationGapTests.MatchesMovedDestination_DifferentSizeWithinTolerance_IsRejected
#[test]
fn matches_moved_destination_different_size_within_tolerance_is_rejected() {
    let stat = FileStat {
        length: 11,
        last_write_utc: stamp(),
    };
    assert!(!matches_moved_destination(&stat, 10, stamp()));
}

// C#: n/a (Rust length conversion: a u64 above i64::MAX never matches; a naive `as i64` would make u64::MAX equal -1)
#[test]
fn matches_moved_destination_length_above_i64_max_never_matches() {
    let stat = FileStat {
        length: u64::MAX,
        last_write_utc: stamp(),
    };
    assert!(!matches_moved_destination(&stat, -1, stamp()));
}

// ---------------------------------------------------------------------------------------------------------------------
// Record defaults (C# records: `new` takes the parameters with no default; the rest get the C# default)
// ---------------------------------------------------------------------------------------------------------------------

// C#: n/a (record defaults)
#[test]
fn record_file_action_request_new_sets_documented_defaults() {
    let request = FileActionRequest::new(r"C:\photos\a.jpg", FileOperationType::Move);
    assert_eq!(request.source, r"C:\photos\a.jpg");
    assert_eq!(request.operation, FileOperationType::Move);
    assert_eq!(request.destination, None);
    assert!(!request.allow_permanent_delete);
}

// C#: n/a (record defaults)
#[test]
fn record_file_action_result_new_sets_documented_defaults() {
    let result = FileActionResult::new(
        true,
        FileOperationType::Copy,
        "src",
        Some("dst".to_owned()),
        10,
        stamp(),
        None,
    );
    assert!(result.succeeded);
    assert_eq!(result.operation, FileOperationType::Copy);
    assert_eq!(result.source, "src");
    assert_eq!(result.destination_path.as_deref(), Some("dst"));
    assert_eq!(result.size, 10);
    assert_eq!(result.last_write_utc, stamp());
    assert_eq!(result.error, None);
    assert!(!result.rejected);
    assert!(result.journal_persisted, "journal_persisted defaults to TRUE");
    assert_eq!(result.journal_error, None);
    assert!(!result.permanently_deleted);
    assert!(!result.source_removed);
}

// C#: n/a (record defaults). `Some("")` is kept as `Some("")`, never folded into `None`.
#[test]
fn record_file_action_result_keeps_empty_error_text_as_some() {
    let result = FileActionResult::new(
        false,
        FileOperationType::Move,
        "src",
        None,
        0,
        stamp(),
        Some(String::new()),
    );
    assert_eq!(result.error, Some(String::new()));
    assert_eq!(result.destination_path, None);
}

// C#: n/a (record defaults)
#[test]
fn record_capture_group_member_result_new_sets_documented_defaults() {
    let result = CaptureGroupMemberResult::new(sample_member(), true, false);
    assert_eq!(result.member, sample_member());
    assert!(result.completed);
    assert!(!result.conflict);
    assert_eq!(result.error, None);
    assert!(!result.source_exists);
    assert!(!result.destination_exists);
    assert!(result.state_known, "state_known defaults to TRUE");
}

// C#: n/a (record defaults)
#[test]
fn record_capture_group_action_result_new_sets_documented_defaults() {
    let result = CaptureGroupActionResult::new(
        true,
        false,
        FileOperationType::Recycle,
        "group-1",
        None,
        vec![sample_member_result("a")],
        None,
    );
    assert!(result.succeeded);
    assert!(!result.rejected);
    assert_eq!(result.operation, FileOperationType::Recycle);
    assert_eq!(result.group_id, "group-1");
    assert!(result.entry.is_none());
    assert_eq!(result.members.len(), 1);
    assert_eq!(result.error, None);
    assert!(result.journal_persisted, "journal_persisted defaults to TRUE");
    assert_eq!(result.journal_error, None);
    assert!(!result.permanently_deleted);
    assert_eq!(result.skipped_missing, None);
}

// C#: n/a (record defaults)
#[test]
fn record_undo_result_new_sets_documented_defaults() {
    let undo = UndoResult::new(true, Some(FileOperationType::Move), "src", Some("dst".to_owned()), None);
    assert!(undo.succeeded);
    assert_eq!(undo.operation, Some(FileOperationType::Move));
    assert_eq!(undo.source, "src");
    assert_eq!(undo.destination.as_deref(), Some("dst"));
    assert_eq!(undo.error_message, None);
    assert!(!undo.rejected);
    assert_eq!(undo.restored_paths, None);

    // `None` operation means "nothing to undo" (C# `FileOperationType?`), not a default Move.
    let nothing = UndoResult::new(false, None, "src", None, None);
    assert_eq!(nothing.operation, None);
}

// ---------------------------------------------------------------------------------------------------------------------
// Record equality and clone
// ---------------------------------------------------------------------------------------------------------------------

// C#: n/a (record equality)
#[test]
fn record_journal_group_member_equality_is_by_value() {
    let member = sample_member();
    assert_eq!(member, sample_member());

    let mut bigger = sample_member();
    bigger.size = 11;
    assert_ne!(member, bigger, "a different size is a different member");

    let mut no_destination = sample_member();
    no_destination.destination = None;
    assert_ne!(member, no_destination, "None destination differs from Some");

    let mut permanent = sample_member();
    permanent.permanent = true;
    assert_ne!(member, permanent, "a different permanent flag is a different member");

    let mut other_case = sample_member();
    other_case.source = other_case.source.to_uppercase();
    assert_ne!(member, other_case, "paths compare exactly (case-sensitive)");
}

// C#: n/a (record equality)
#[test]
fn record_file_action_request_equality_is_by_value() {
    let request = FileActionRequest::new("a", FileOperationType::Copy);
    assert_eq!(request, FileActionRequest::new("a", FileOperationType::Copy));
    assert_ne!(request, FileActionRequest::new("a", FileOperationType::Move));
    assert_ne!(request, FileActionRequest::new("A", FileOperationType::Copy));
}

// C#: n/a (record defaults: Clone is deep)
#[test]
fn record_capture_group_action_result_clone_is_deep() {
    let original = CaptureGroupActionResult::new(
        true,
        false,
        FileOperationType::Move,
        "group-1",
        None,
        vec![sample_member_result("a")],
        None,
    );
    let mut copy = original.clone();
    assert_eq!(copy, original);

    copy.members.push(sample_member_result("b"));
    copy.members[0].member.source.push_str("-changed");
    copy.group_id.push_str("-changed");

    assert_eq!(
        original.members.len(),
        1,
        "pushing into the clone must not touch the original"
    );
    assert_eq!(
        original.members[0].member.source, "a",
        "mutating a nested string must not touch the original"
    );
    assert_eq!(original.group_id, "group-1");
    assert_ne!(copy, original);
}
