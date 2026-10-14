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

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::journal_line::{self, JournalEntry};
use super::journal_rules::{
    STARTUP_COMMITTED_MOVE_LIMIT, is_stale_reconcile_after_committed, try_parse_tail_committed_move,
};
use crate::model::JournalState;

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
    if snapshot.is_empty() {
        return CompactionPlan::default();
    }

    // Pass 1: line boundaries (each range includes its own '\n') and recognized entries (one parse per line).
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut entries: Vec<Option<JournalEntry>> = Vec::new();
    let mut blank: Vec<bool> = Vec::new();
    let mut position = 0;
    while position < snapshot.len() {
        let length = match snapshot[position..].iter().position(|&b| b == b'\n') {
            Some(newline) => newline + 1,
            None => snapshot.len() - position,
        };
        let range = position..position + length;
        let content = line_content(&snapshot[range.clone()]);
        let is_blank = is_blank_content(content);
        // C#: `OperationJournal.ReadEntries` splits at a lone '\r' too, so a line holding one is never recognized here.
        let entry = if is_blank || content.contains(&b'\r') {
            None
        } else {
            journal_line::try_parse_utf8(content)
        };
        ranges.push(range);
        entries.push(entry);
        blank.push(is_blank);
        position += length;
    }

    // Effective latest line per Id (ordinal key), with the FA-01 rule of the journal readers.
    let mut latest: HashMap<&str, (usize, &JournalEntry)> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let Some(entry) = entry else { continue };
        let previous = latest.get(entry.id.as_str()).map(|&(_, previous)| previous);
        if is_stale_reconcile_after_committed(entry, previous) {
            continue;
        }
        latest.insert(entry.id.as_str(), (i, entry));
    }

    // The committed-Move lines the tail reader of `ReadCommittedMoves` returns (same predicate, newest first).
    let mut committed_move_window: HashSet<usize> = HashSet::new();
    for i in (0..ranges.len()).rev() {
        if committed_move_window.len() >= STARTUP_COMMITTED_MOVE_LIMIT {
            break;
        }
        if try_parse_tail_committed_move(line_content(&snapshot[ranges[i].clone()])).is_some() {
            committed_move_window.insert(i);
        }
    }

    // W2-FA-04: the LAST members-carrying line of an Id whose effective latest line carries no members (and is not
    // Dismissed) is the downgrade repair's evidence: kept.
    let mut last_members_line: HashMap<&str, usize> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        match entry {
            Some(entry) if has_members(entry) => {
                last_members_line.insert(entry.id.as_str(), i);
            }
            _ => {}
        }
    }
    let mut evidence: HashSet<usize> = HashSet::new();
    for (id, index) in last_members_line {
        match latest.get(id) {
            Some(&(_, latest_entry)) if !has_members(latest_entry) && latest_entry.state != JournalState::Dismissed => {
                evidence.insert(index);
            }
            _ => {}
        }
    }

    let is_droppable = |i: usize| -> bool {
        if blank[i] {
            return true;
        }
        let Some(entry) = &entries[i] else {
            return false; // unparseable here: kept verbatim
        };
        let Some(&(latest_index, latest_entry)) = latest.get(entry.id.as_str()) else {
            return false;
        };
        matches!(latest_entry.state, JournalState::Committed | JournalState::Dismissed)
            && i < latest_index
            && !committed_move_window.contains(&i)
            && !evidence.contains(&i)
    };

    // Output order comes from this index loop only (never from a map).
    let mut kept = Vec::with_capacity(snapshot.len());
    let mut dropped_lines = 0;
    for (i, range) in ranges.iter().enumerate() {
        if is_droppable(i) {
            dropped_lines += 1;
            continue;
        }
        kept.extend_from_slice(&snapshot[range.clone()]);
    }
    CompactionPlan {
        kept,
        dropped_lines,
        kept_lines: ranges.len() - dropped_lines,
    }
}

/// C# `GroupMembers is { Count: > 0 }`.
fn has_members(entry: &JournalEntry) -> bool {
    entry.group_members.as_ref().is_some_and(|members| !members.is_empty())
}

/// C# `content.Trim(" \t\r"u8).IsEmpty`: the content holds ONLY those three bytes (`\x0C`, `\x0B`, NBSP, NUL are not
/// blank).
fn is_blank_content(content: &[u8]) -> bool {
    content.iter().all(|b| matches!(b, b' ' | b'\t' | b'\r'))
}

/// C# `JournalCompactionPlan.Content`: a line without its terminator, as both journal readers see it: strip ONE trailing
/// `\n`, then ONE trailing `\r` (so `"x\r\r\n"` -> `"x\r"`, `"x\r"` without `\n` -> `"x"`). The same stripping as the
/// W3-01 tail reader (`ParseCommittedMoves` splits at `\n`, then strips one `\r`).
pub fn line_content(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}
