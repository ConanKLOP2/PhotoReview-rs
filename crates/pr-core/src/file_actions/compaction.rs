//! Port of `PhotoReview.Core/FileActions/JournalCompactionPlan.cs` (C# baseline d92d6aae, 126 lines): the pure
//! selection step of journal compaction (`OperationJournal::try_compact`, W3-01). Which lines of a journal snapshot can be
//! dropped without changing what ANY reader of the journal observes. No I/O. Contract: `contracts/W3-02.md`.
//!
//! **Rule.** A line is dropped only when it is (a) blank, or (b) a recognized entry
//! ([`journal_line::try_parse_utf8`]) of an Id whose effective latest entry L (latest-entry resolution with the FA-01
//! rule, [`is_stale_reconcile_after_committed`]) is Committed or Dismissed, that lies strictly BEFORE L, that is
//! not one of the last [`STARTUP_COMMITTED_MOVE_LIMIT`] committed-Move lines the tail reader returns, and that is not the
//! Id's group-repair evidence line (W2-FA-04). Everything else is kept byte for byte in its original order: every entry
//! of an Id that is still Prepared or Failed, L itself and every line of its Id after L, and every line this build cannot
//! parse (malformed, an enum value from a newer build, a quarantined group line).
//!
//! **Why readers cannot tell** (C# doc comment, summarized): only entries before a Committed/Dismissed L of the same Id
//! disappear; L is always assigned when read (FA-01 only ever skips a Failed), the last line of an Id is unchanged,
//! `AppendIfUnchangedSince` answers the same because L can never equal a Prepared entry, the 200-line committed-Move
//! window of `ReadCommittedMoves` is kept, and the downgrade repair's evidence line is kept.
//!
//! The kept bytes are the ORIGINAL bytes of each kept line including its own terminator (`\r\n`, `\n`, or none for a
//! final partial line): the on-disk format is never re-serialized (AGENTS.md: journal format byte-identical to C#).
//!
//! The line predicates are `file_actions::journal_rules` (this unit's file, complete in the stub), ONE copy shared with
//! the journal readers (`file_actions::journal`, W3-01): the plan must apply exactly the rules the readers apply.

// TEMPORARY (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code, unused_imports)]

use super::journal_line::{self, JournalEntry};
use super::journal_rules::{
    STARTUP_COMMITTED_MOVE_LIMIT, is_stale_reconcile_after_committed, try_parse_tail_committed_move,
};

/// C# `JournalCompactionPlan.Result(byte[] Kept, int DroppedLines, int KeptLines)`.
///
/// `dropped_lines + kept_lines` is the number of lines of the snapshot (blank lines count). C# uses `int`; a snapshot is
/// one .NET array (< 2^31 bytes), so the counts always fit and `usize` changes nothing observable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompactionPlan {
    /// The kept lines, concatenated in their original order with their original terminators.
    pub kept: Vec<u8>,
    pub dropped_lines: usize,
    pub kept_lines: usize,
}

/// C# `JournalCompactionPlan.Build(ReadOnlySpan<byte> snapshot)`.
///
/// `snapshot` normally ends right after a `\n` (the caller cuts a partial last line off), but a final line without a
/// terminator is handled like any other line (its bytes are kept or dropped as a whole). An empty snapshot gives
/// `CompactionPlan::default()`.
///
/// Passes (all over the SAME line list; see contract W3-02 "Semantics notes" for the traps):
/// 1. Split at every `\n` (only `\n`; a line's length includes its `\n`). For each line, `content` =
///    [`line_content`]. `blank` = `content` trimmed of the bytes `b' '`, `b'\t'`, `b'\r'` at both ends is empty (ONLY
///    those three bytes: `\x0C`, `\x0B`, NBSP are not blank). The recognized entry is `None` when blank OR when `content`
///    contains a `\r` byte anywhere, else `journal_line::try_parse_utf8(content)`.
/// 2. Effective latest entry per Id (ordinal `String` key; a `HashMap` is fine, its order never leaks): for each
///    recognized entry in file order, skip it when `is_stale_reconcile_after_committed(entry, current latest)`, else it
///    becomes the Id's latest `(index, entry)`.
/// 3. Committed-Move window: walk ALL lines from the last to the first and collect the indices whose `content` passes
///    `try_parse_tail_committed_move`, stopping once [`STARTUP_COMMITTED_MOVE_LIMIT`] are collected. This uses the raw
///    `content`, NOT the recognized entry: a committed-Move line with an inner `\r` takes a window slot although it is
///    never droppable (golden `window-inner-cr-line-takes-a-slot`).
/// 4. Evidence (W2-FA-04): for each Id, the index of its LAST recognized entry whose `group_members` is `Some(non-empty)`;
///    that index is evidence when the Id's latest entry has no members (`None` or `Some(empty)`) and is not Dismissed.
/// 5. Line i is droppable when blank, or when it has a recognized entry whose Id's latest `(L_index, L)` has
///    `L.state` Committed or Dismissed, `i < L_index`, i is not in the window and not evidence. Unrecognized non-blank
///    lines are always kept.
pub fn build(snapshot: &[u8]) -> CompactionPlan {
    todo!("W3-02")
}

/// C# `JournalCompactionPlan.Content`: a line without its terminator, as both journal readers see it: strip ONE trailing
/// `\n`, then ONE trailing `\r` (so `"x\r\r\n"` -> `"x\r"`, `"x\r"` without `\n` -> `"x"`). The same stripping as the
/// W3-01 tail reader (`ParseCommittedMoves` splits at `\n`, then strips one `\r`).
pub fn line_content(line: &[u8]) -> &[u8] {
    todo!("W3-02")
}
