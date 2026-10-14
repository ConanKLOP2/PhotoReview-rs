//! The journal's shared line rules (C# baseline d92d6aae, `PhotoReview.Core/FileActions/OperationJournal.cs` lines 115,
//! 384-402 and 429-438): `StartupCommittedMoveLimit`, `TryParseTailCommittedMove`, `IsRecycleOrCopyLine`,
//! `IsReconcileCode`, `IsStaleReconcileAfterCommitted`. ONE copy shared by the journal readers (`file_actions::journal`,
//! W3-01) and the compaction plan (`file_actions::compaction`, W3-02): the plan must apply exactly the rules the readers
//! apply (C# keeps them in `OperationJournal` and the plan calls them there). Contract: `contracts/W3-02.md`.
//!
//! COMPLETE in the stub (no `todo!()`): they only use the merged W1-13 / W1-14 / W1-01 items, so W3-01 and W3-02 can
//! both be ported against them in parallel.

use crate::file_actions::errors;
use crate::file_actions::journal_line::{self, JournalEntry};
use crate::model::{FileOperationType, JournalState};

/// C# `internal const int StartupCommittedMoveLimit = 200`: the number of committed Moves the tail reader of
/// `ReadCommittedMoves` returns for a journal of at least 1 MiB, and the size of the compaction plan's window.
pub const STARTUP_COMMITTED_MOVE_LIMIT: usize = 200;

/// C# `internal static TryParseTailCommittedMove(ReadOnlySpan<byte> line)` (CORE-08): the line's committed Move, or
/// `None`. `line` has no terminator (one trailing `'\r'` already stripped by the caller). An empty line, or a line
/// containing the exact bytes `"Type":"Recycle"` or `"Type":"Copy"`, is skipped without parsing (a Move line never
/// contains them: quotes inside string values are escaped). Otherwise the W1-14 byte parser decides (same acceptance as
/// every reader: a quarantined group line is `None` here too).
pub fn try_parse_tail_committed_move(line: &[u8]) -> Option<JournalEntry> {
    if line.is_empty() || is_recycle_or_copy_line(line) {
        return None;
    }
    journal_line::try_parse_utf8(line)
        .filter(|entry| entry.op_type == FileOperationType::Move && entry.state == JournalState::Committed)
}

/// C# `IsRecycleOrCopyLine`: byte search for `"Type":"Recycle"` or `"Type":"Copy"` (exact bytes, no space, case-sensitive).
pub fn is_recycle_or_copy_line(line: &[u8]) -> bool {
    contains_bytes(line, br#""Type":"Recycle""#) || contains_bytes(line, br#""Type":"Copy""#)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

/// C# `internal static IsReconcileCode(code)`: `PendingUnconfirmed` or `SourceStillExistsAfterRecovery` (exact, ordinal).
pub fn is_reconcile_code(code: Option<&str>) -> bool {
    matches!(
        code,
        Some(errors::PENDING_UNCONFIRMED | errors::SOURCE_STILL_EXISTS_AFTER_RECOVERY)
    )
}

/// C# `IsStaleReconcileAfterCommitted(entry, previous)` (FA-01): a reconcile-coded Failed whose previous effective
/// line is Committed. ONE rule for every "latest line of an Id" reader (`ComputeLatestEntries`, `AppendIfLatestIs`,
/// `Dismiss`, the compaction plan).
pub fn is_stale_reconcile_after_committed(entry: &JournalEntry, previous: Option<&JournalEntry>) -> bool {
    entry.state == JournalState::Failed
        && is_reconcile_code(entry.error_code.as_deref())
        && previous.is_some_and(|p| p.state == JournalState::Committed)
}
