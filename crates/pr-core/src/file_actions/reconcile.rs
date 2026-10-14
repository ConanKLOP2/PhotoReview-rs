//! Port of the startup-reconcile half of `PhotoReview.Core/FileActions/OperationJournal.cs` (C# baseline d92d6aae):
//! `ReconcilePendingOperations` (lines 457-488), `ReconcileOne` (490-520), the ADR 0003 "older builds" downgrade guard
//! `RepairGroupLinesSettledByOlderBuild` + `SettledByOlderBuildText` (522-578), `TryReconcileAppend` (583-594),
//! `AppendIfLatestIs` (596-612), `IsRecordedDestination` (619-620), `IsGroupMemberCompleted` (622-647),
//! `AppendIfStillPending` (653-666), `IsExecuting` (841) and `WithOutcome` (844-853). INV-6, PARITY F-11, F-14, F-15.
//! Contract: `contracts/W3-03.md`.
//!
//! The rest of `OperationJournal` (append, reads, Dismiss, compaction) is W3-01 (`file_actions::journal`). This module
//! reaches the journal only through [`ReconcileJournal`], which `OperationJournal` implements, so the reconcile rules
//! live here and the journal file handling there. `OperationJournal::reconcile_pending_operations` (W3-01) is a one-line
//! delegate to [`reconcile_pending_operations`].
//!
//! **Rules** (contract W3-03, "Semantics notes"):
//! * Only the LATEST line of an Id counts (FA-01: a reconcile-coded `Failed` right after a `Committed` is not the
//!   latest, W3-02 `journal_rules::is_stale_reconcile_after_committed`).
//! * A latest `Prepared` entry stamped before `prepared_before_utc` and without a live marker is judged from the files:
//!   Move = source gone AND destination is the recorded file; Copy = destination is the recorded file; Recycle = source
//!   gone; a group = every member completed. The verdict is appended only when the Id's raw last line is STILL
//!   `Prepared` and the Id is still not live (re-read under the journal gate).
//! * Downgrade guard (ADR 0003, "older builds"): an Id whose latest line has no members although an earlier line had
//!   them was settled by an older build. The members are restored on a new line; a `Committed` verdict is re-checked
//!   against every member and becomes `Failed` with [`SETTLED_BY_OLDER_BUILD_TEXT`] (no code) when one is incomplete.
//!   `Dismissed` is left alone. Idempotent: the repaired line carries the members, so the next run finds nothing.
//! * One entry that cannot be judged or appended never stops the others: it stays as it is and the next start retries.

// TEMPORARY (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::collections::HashMap;
use std::io;

use crate::abstractions::{Clock, FileStat, FileSystem, LiveOperationRegistry, Log};
use crate::file_actions::journal_line::{JournalEntry, JournalGroupMember};
use crate::file_actions::journal_rules::is_stale_reconcile_after_committed;
use crate::model::JournalState;
use crate::time::DateTimeUtc;

/// C# `OperationJournal.SettledByOlderBuildText` (internal const): the English `Error` of a group line an older build
/// settled as Committed although not every member is in place (ADR 0003 downgrade guard). Deliberately written WITHOUT an
/// `ErrorCode`: a reconcile code after a Committed line is ignored by every reader (FA-01), and this verdict follows
/// exactly such a line. Persisted text: never change it (the journal format is byte-identical to C#).
///
/// ONE definition, here (W3-01's draft stub has none; if W3-01 needs the name it re-exports this one).
pub const SETTLED_BY_OLDER_BUILD_TEXT: &str = "This capture group was settled by an older PhotoReview build that only tracks its first file; not every file was found in its expected place.";

/// What the reconcile needs from the journal. Implemented by `OperationJournal` (W3-01, in `journal.rs`: it needs the
/// private fields and the gate). Tests use the real journal over `FakeFileSystem` (a fake journal would re-implement the
/// very file rules under test).
///
/// All methods are blocking. The journal gate is reentrant (W3-01 `ReentrantGate`, like C# `lock`), but `decide`
/// callbacks never call back into the journal anyway.
pub trait ReconcileJournal {
    /// The journal's file system (C# `_fileSystem`): every file check of the reconcile goes through it.
    fn file_system(&self) -> &dyn FileSystem;
    /// The journal's clock (C# `_clock`): the `TimestampUtc` of every outcome line. NOT the startup clock.
    fn clock(&self) -> &dyn Clock;
    /// C# `LiveOperations` (Q-R27): an Id whose marker is live is executing somewhere and is never judged.
    fn live_operations(&self) -> &dyn LiveOperationRegistry;
    /// C# `JournalErrors.EnglishText(code)`: the invariant English text stored next to a reconcile code (the journal's
    /// English catalog, `JournalTexts::english(errors::catalog_key(code))`; never the UI language).
    fn english_text(&self, code: &str) -> String;
    /// C# `ReadEntries`: under the journal gate, every line `journal_line::try_parse` accepts, in file order (UTF-8, BOM
    /// skipped, `\r\n` / `\n` / `\r` line ends like `StreamReader.ReadLine`). A missing journal file is `Ok` with no call.
    /// Open/read errors are returned (C#: the exception propagates).
    fn read_entries(&self, handle: &mut dyn FnMut(JournalEntry)) -> io::Result<()>;
    /// The compare-and-append primitive under C# `AppendIfStillPending` / `AppendIfLatestIs`: under ONE hold of the
    /// journal gate, collect the entries whose `id` equals `id` (ordinal) in file order with the same reader as
    /// [`read_entries`](Self::read_entries), call `decide` with them, and when it returns `Some(entry)` append `entry`
    /// exactly like `OperationJournal::append` (directory creation, torn-tail newline, unrepresentable-path refusal,
    /// sharing-violation retries, durability) and return `Ok(true)`. `None` appends nothing: `Ok(false)`. A missing
    /// journal gives `decide(&[])`. Read and append errors are returned as `Err` (nothing appended).
    fn append_if(&self, id: &str, decide: &mut dyn FnMut(&[JournalEntry]) -> Option<JournalEntry>) -> io::Result<bool>;
}

/// C# `OperationJournal.ReconcilePendingOperations(DateTime? preparedBeforeUtc)`.
///
/// Returns the outcome lines this call appended whose verdict the caller must know, in append order: first the
/// downgrade-guard lines whose state CHANGED (only Committed -> Failed), in the order the Ids first appear in the journal
/// WITH members; then one line per reconciled Prepared entry, in the order the Ids first appear in the journal. Repair
/// lines that only restore the members (state unchanged) are appended but not returned.
///
/// `prepared_before_utc`: only Prepared entries whose `timestamp_utc.value` is strictly before it are judged (startup
/// passes the moment it began, so this process's own in-flight actions are never judged). `None` judges every one.
///
/// `diagnostics` receives the per-entry warnings C# writes to `FileLog.Default` (W3-01 passes its configured log, by
/// default `FileLog::default_instance()`):
/// * `"Journal reconcile: skipped {id}: {message}"` when judging one Prepared entry panicked (C#: a file check threw);
/// * `"Journal reconcile: skipped group repair of {id}: {message}"` when repairing one group panicked;
/// * `"Journal reconcile: could not append outcome for {id}: {error}"` when an outcome append (or its re-read) failed.
///
/// `{message}` of a panic is `abstractions::panic_message(payload)`; `{error}` is the `io::Error` display text.
///
/// `Err` only when the initial full read of the journal fails (C#: the exception escapes; `startup_recovery`
/// catches it). Everything after that read is per entry and never fails the call. An `Err` of `append_if` (its re-read
/// or its append; C# catches `IOException` and `UnauthorizedAccessException`, Rust: every `io::Error`) is the
/// "could not append outcome" warning; a PANIC anywhere in one entry's judgement or append is the "skipped" warning
/// (C#: any other exception, caught by the per-entry `catch (Exception) when not OutOfMemory`).
pub fn reconcile_pending_operations(
    journal: &dyn ReconcileJournal,
    prepared_before_utc: Option<DateTimeUtc>,
    diagnostics: &dyn Log,
) -> io::Result<Vec<JournalEntry>> {
    todo!()
}

// ---------------------------------------------------------------------------------------------------------------
// Private helpers (names and shapes are a suggestion; only the public items above are the contract).
// ---------------------------------------------------------------------------------------------------------------

/// The `(GroupId, GroupMembers)` of the LAST line of an Id that carried a non-empty member list.
type GroupHistory = (Option<String>, Vec<JournalGroupMember>);

/// An insertion-ordered map keyed by Id (C# `Dictionary<string, T>` with `StringComparer.Ordinal`, which enumerates in
/// first-insertion order because nothing is ever removed). Re-assigning an existing key keeps its position.
#[derive(Debug)]
struct OrderedById<T> {
    order: Vec<String>,
    by_id: HashMap<String, T>,
}

impl<T> Default for OrderedById<T> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            by_id: HashMap::new(),
        }
    }
}

impl<T> OrderedById<T> {
    fn get(&self, id: &str) -> Option<&T> {
        self.by_id.get(id)
    }

    /// Inserts or replaces; a new key goes to the end, an existing one keeps its position.
    fn set(&mut self, id: &str, value: T) {
        todo!()
    }

    /// Values in first-insertion order of their keys.
    fn values(&self) -> impl Iterator<Item = &T> {
        self.order.iter().filter_map(|id| self.by_id.get(id))
    }

    fn keys(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }
}

/// C# `ComputeLatestEntries(groupHistory)`: one pass over [`ReconcileJournal::read_entries`]. For every entry, in file
/// order: (1) when `group_members` is `Some` and NON-EMPTY, `history[id] = (group_id, members)` (the LAST such line
/// wins, position of the first); (2) unless `is_stale_reconcile_after_committed(entry, latest.get(id))`,
/// `latest[id] = entry`. Step 1 runs before the stale check, so a stale line still records its members.
fn compute_latest_entries(
    journal: &dyn ReconcileJournal,
) -> io::Result<(OrderedById<JournalEntry>, OrderedById<GroupHistory>)> {
    todo!()
}

/// C# `RepairGroupLinesSettledByOlderBuild` + its local `RepairOne`, for every Id of `history` in order. Each Id runs
/// inside `catch_unwind`; a panic logs `"Journal reconcile: skipped group repair of {id}: {message}"` to `diagnostics`
/// and leaves `latest` for that Id as it was. Returns the repair lines whose state CHANGED (in order).
///
/// `RepairOne(id)`: return when `latest[id]` has non-empty members or is `Dismissed`. `restored` = `latest[id]` with
/// `group_id`/`group_members` from `history[id]`. `Prepared` -> `latest[id] = restored`, return (judged by the main loop).
/// Live -> return. `outcome` = `restored`, or, when `restored` is `Committed` and NOT every member is completed,
/// `restored` with `state = Failed`, `timestamp_utc = JournalDateTime::utc(journal.clock().utc_now())`,
/// `error = Some(SETTLED_BY_OLDER_BUILD_TEXT)`, `error_code = None`. Append through [`append_if_latest_is`]`(latest[id],
/// outcome)` wrapped in [`try_reconcile_append`]; on `true`: `latest[id] = outcome`, and the result gets `outcome`
/// when its state differs from `restored`'s.
fn repair_group_lines_settled_by_older_build(
    journal: &dyn ReconcileJournal,
    latest: &mut OrderedById<JournalEntry>,
    history: &OrderedById<GroupHistory>,
    diagnostics: &dyn Log,
) -> Vec<JournalEntry> {
    todo!()
}

/// C# `ReconcileOne`. Runs inside `catch_unwind` (caller); a panic logs `"Journal reconcile: skipped {id}: {message}"`.
/// Live -> nothing. Group (non-empty members): all members completed -> Committed, else Failed with
/// `SourceStillExistsAfterRecovery` (Recycle) / `PendingUnconfirmed` (Move, Copy). Single Recycle: `file_exists(source)`
/// -> Failed (`SourceStillExistsAfterRecovery`) else Committed. Single Move/Copy: `file_exists(source)` FIRST, then the
/// destination stat only when `destination` is `Some` (C# `is not null`; W1-14 `try_parse` already turned a blank or
/// whitespace-only entry-level `Destination` into `None`, so `Some` is always a usable path here); Move =
/// `!source_exists && recorded`, Copy = `recorded`; failure code `PendingUnconfirmed`. Each verdict goes through
/// `try_reconcile_append(append_if_still_pending)`; `true` -> `Some(outcome)` (the caller appends it to its result).
fn reconcile_one(
    journal: &dyn ReconcileJournal,
    pending: &JournalEntry,
    diagnostics: &dyn Log,
) -> Option<JournalEntry> {
    todo!()
}

/// C# `TryReconcileAppend`: `Ok(b)` -> `b`; `Err(e)` -> warn `"Journal reconcile: could not append outcome for {id}: {e}"`
/// and `false`.
fn try_reconcile_append(append: impl FnOnce() -> io::Result<bool>, id: &str, diagnostics: &dyn Log) -> bool {
    todo!()
}

/// C# `AppendIfStillPending` (FA-01 + Q-R27): through [`ReconcileJournal::append_if`], append `outcome` only when the RAW
/// last line of its Id (no FA-01 skipping here) is `Prepared` and, checked after that read, the Id is not live.
fn append_if_still_pending(journal: &dyn ReconcileJournal, outcome: &JournalEntry) -> io::Result<bool> {
    todo!()
}

/// The effective latest line of ONE Id's lines (file order) under FA-01: the same fold as [`compute_latest_entries`]
/// and C# `AppendIfLatestIs` (`current` advances unless the line is a stale reconcile verdict after `current`).
/// Complete in the stub.
fn effective_latest(lines: &[JournalEntry]) -> Option<&JournalEntry> {
    lines.iter().fold(None, |current, entry| {
        if is_stale_reconcile_after_committed(entry, current) {
            current
        } else {
            Some(entry)
        }
    })
}

/// C# `AppendIfLatestIs`: append `outcome` only when [`effective_latest`] of `expected.id`'s lines equals `expected`
/// (value equality, `JournalEntry: PartialEq`: timestamps compare by ticks only). No live check (the caller did it).
fn append_if_latest_is(
    journal: &dyn ReconcileJournal,
    expected: &JournalEntry,
    outcome: &JournalEntry,
) -> io::Result<bool> {
    todo!()
}

/// C# `IsRecordedDestination` (R01c): `destination` is `Some` and `types::matches_moved_destination(stat, size,
/// last_write_utc)` (exact size, write time within the 2 s FAT tolerance, inclusive).
fn is_recorded_destination(destination: Option<FileStat>, size: i64, last_write_utc: DateTimeUtc) -> bool {
    todo!()
}

/// C# `IsGroupMemberCompleted`. Recycle: not undo (`group.undo != Some(true)`) -> `!file_exists(member.source)`; undo ->
/// `get_file_stat(member.source)` has EXACTLY `member.size` and `member.last_write_utc` ticks (no tolerance, R15).
/// Move/Copy: destination stat only when `member.destination` is not null/empty/whitespace (C# `IsNullOrWhiteSpace`),
/// THEN `file_exists(member.source)`; Move = `!source_exists && recorded`, Copy = `recorded`.
fn is_group_member_completed(
    journal: &dyn ReconcileJournal,
    group: &JournalEntry,
    member: &JournalGroupMember,
) -> bool {
    todo!()
}

/// C# `WithOutcome`: a copy of `pending` with `state`, `timestamp_utc = JournalDateTime::utc(journal.clock().utc_now())`,
/// and, only when `state == Failed`, `error = Some(journal.english_text(failure_code))`, `error_code =
/// Some(failure_code)`; otherwise both `None`. Every other field (members, group id, permanent, undo) is kept.
fn with_outcome(
    journal: &dyn ReconcileJournal,
    pending: &JournalEntry,
    state: JournalState,
    failure_code: &str,
) -> JournalEntry {
    todo!()
}
