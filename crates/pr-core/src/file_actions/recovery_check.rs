//! Port of `PhotoReview.Core/FileActions/RecoveryFileCheck.cs` (C# baseline d92d6aae; the verdict table of the C#
//! `docs/architecture.md`, "Recovery check"; PARITY F-17, W-04).
//!
//! Read-only "is this journal entry still valid?" check for the Recovery window. It only calls
//! [`FileSystem::directory_exists`] / [`FileSystem::get_file_stat`]: it never moves, writes or deletes and never touches
//! the (append-only) journal. A path that cannot be inspected is [`RecoveryPathStatus::Unreadable`].
//!
//! * Path status: `Missing` = no file; `Changed` = size differs from the journal (the SOURCE also when its last-write
//!   differs); `Exists` otherwise. The destination last-write is never compared (copies across volumes re-stamp it).
//! * Verdict for Move/Copy in Prepared/Failed state (S = source, D = destination): see [`RecoveryFileCheck::decide`].
//! * Capture groups (`group_members` non-empty): each member is checked as a single entry and the verdicts are folded
//!   by the rules of `aggregate_group_verdict`; an UNDO of a Recycle group is judged per member by whether the file is
//!   back at its original path.
//!
//! Types mirror the C# records; the Recovery presenter (App wave) builds [`RecoveryPathCheck`] /
//! [`RecoveryCheckResult`] by hand in its tests, so every field is `pub`.
//!
//! STUB: signatures and data types are final, bodies are `todo!()`.
#![allow(unused_variables, dead_code)] // D11: only while this stub has `todo!()` bodies; the porter removes it.

use std::fmt;
use std::sync::Arc;

use crate::abstractions::FileSystem;
use crate::file_actions::journal_line::{JournalEntry, JournalGroupMember};
use crate::time::DateTimeUtc;

/// Live state of one path named by a journal entry, compared with what the journal recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryPathStatus {
    /// The file exists and matches the journal (source: size and last-write; destination: size).
    Exists,
    /// No file at the path.
    Missing,
    /// The file exists but its size (or, for the source, last-write time) differs from the journal.
    Changed,
    /// The path could not be inspected (access denied, I/O error, invalid path).
    Unreadable,
}

/// Overall conclusion for one entry. Declaration order is the C# order (do not reorder: the numeric value may be shown
/// in diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryVerdict {
    CanRetry,
    AlreadyDone,
    Conflict,
    SourceChanged,
    DestinationChanged,
    Lost,
    Unknown,
    RecycleUnverifiable,
    NotRecycled,
    /// Q-R8: a Recycle that deleted the file permanently (drive without a Recycle Bin); it cannot be restored.
    PermanentlyDeleted,
    /// A Delete of a capture group where some files were deleted permanently and the others went to the Recycle Bin:
    /// only part of the capture is unrecoverable.
    PartiallyPermanentlyDeleted,
}

/// Result of checking one path. Size/time are the current values (`None` when missing or unreadable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryPathCheck {
    pub path: String,
    pub status: RecoveryPathStatus,
    /// C# `long?`: the stat length, saturated at `i64::MAX` (a `u64` length above that cannot occur in C#).
    pub current_size: Option<i64>,
    /// C# `DateTime?` (always Kind Utc here).
    pub current_last_write_utc: Option<DateTimeUtc>,
    /// The parent folder of `path` exists (`false` when the path has no folder part or the folder check failed).
    pub folder_exists: bool,
}

impl RecoveryPathCheck {
    pub fn new(
        path: impl Into<String>,
        status: RecoveryPathStatus,
        current_size: Option<i64>,
        current_last_write_utc: Option<DateTimeUtc>,
        folder_exists: bool,
    ) -> Self {
        Self {
            path: path.into(),
            status,
            current_size,
            current_last_write_utc,
            folder_exists,
        }
    }
}

/// Result for one journal entry. `destination` is `None` when the entry has none (Recycle, or a blank destination).
/// For a group, `source`/`destination` are those of the FIRST member and `group_members` holds one check per member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryCheckResult {
    pub entry: JournalEntry,
    pub source: RecoveryPathCheck,
    pub destination: Option<RecoveryPathCheck>,
    pub verdict: RecoveryVerdict,
    /// `None` for a single entry. A group result always has at least one member.
    pub group_members: Option<Vec<RecoveryGroupMemberCheck>>,
}

impl RecoveryCheckResult {
    /// C# `DestinationFolderMissing`: the destination is missing and its parent folder no longer exists either
    /// (`destination` is `Some` with status `Missing` and `folder_exists == false`).
    pub fn destination_folder_missing(&self) -> bool {
        matches!(
            &self.destination,
            Some(d) if d.status == RecoveryPathStatus::Missing && !d.folder_exists
        )
    }

    /// C# `IsGroup`: `group_members` is `Some` with at least one member (`Some(vec![])` is NOT a group).
    pub fn is_group(&self) -> bool {
        matches!(&self.group_members, Some(members) if !members.is_empty())
    }
}

/// One member of a capture-group result: the journal member and its own check (a full [`RecoveryCheckResult`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryGroupMemberCheck {
    pub member: JournalGroupMember,
    pub check: RecoveryCheckResult,
}

impl RecoveryGroupMemberCheck {
    /// C# `Source` (= `check.source`).
    pub fn source(&self) -> &RecoveryPathCheck {
        &self.check.source
    }

    /// C# `Destination` (= `check.destination`).
    pub fn destination(&self) -> Option<&RecoveryPathCheck> {
        self.check.destination.as_ref()
    }

    /// C# `Verdict` (= `check.verdict`).
    pub fn verdict(&self) -> RecoveryVerdict {
        self.check.verdict
    }
}

/// C# `RecoveryFileCheck`.
pub struct RecoveryFileCheck {
    file_system: Arc<dyn FileSystem>,
}

impl fmt::Debug for RecoveryFileCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryFileCheck").finish_non_exhaustive()
    }
}

impl RecoveryFileCheck {
    pub fn new(file_system: Arc<dyn FileSystem>) -> Self {
        Self { file_system }
    }

    /// C# `Check(entry)`. Never fails and never mutates: every problem with a path becomes
    /// [`RecoveryPathStatus::Unreadable`] and the verdict `Unknown`.
    ///
    /// * `group_members` `Some` and non-empty: one `check_group_member` per member (in order, each checked
    ///   exactly once, eagerly), then `aggregate_group_verdict`; the result carries the FIRST member's
    ///   source/destination.
    /// * Otherwise: source = `check_path(entry.source, entry.size, entry.last_write_utc, compare_last_write: true)`;
    ///   destination = `None` when `entry.destination` is `None`/empty/whitespace, else
    ///   `check_path(destination, entry.size, entry.last_write_utc, compare_last_write: false)`; verdict =
    ///   [`decide`](Self::decide).
    pub fn check(&self, entry: &JournalEntry) -> RecoveryCheckResult {
        todo!("W3-07: port RecoveryFileCheck.Check")
    }

    /// C# `internal static Decide`: the verdict of one non-group entry from the two path checks. Exposed `pub` so the
    /// 720-row table golden (`recovery-decide`) can drive it without a file system.
    ///
    /// 1. `Recycle`: source `Missing` => `PermanentlyDeleted` when `entry.permanent == Some(true)` else
    ///    `RecycleUnverifiable`; `Unreadable` => `Unknown`; `Exists`/`Changed` => `NotRecycled`. The destination is
    ///    ignored.
    /// 2. No destination => `Unknown`.
    /// 3. `Committed`: D `Exists` => `AlreadyDone`; D `Missing` and S `Missing` => `Lost`; anything else `Unknown`.
    /// 4. State not Prepared/Failed (Dismissed) => `Unknown`.
    /// 5. S or D `Unreadable` => `Unknown`.
    /// 6. present = `Exists` or `Changed`. D absent: S absent => `Lost`; S `Exists` => `CanRetry`; S `Changed` =>
    ///    `SourceChanged`. D present, S absent: D `Exists` => `AlreadyDone`, D `Changed` => `DestinationChanged`.
    ///    Both present: Copy with S `Exists` and D `Exists` => `AlreadyDone`; everything else `Conflict`.
    pub fn decide(
        entry: &JournalEntry,
        source: &RecoveryPathCheck,
        destination: Option<&RecoveryPathCheck>,
    ) -> RecoveryVerdict {
        todo!("W3-07: port RecoveryFileCheck.Decide")
    }

    /// C# `CheckGroupMember(group, member)`: `check()` of a copy of the group entry with `source`, `destination`,
    /// `size`, `last_write_utc` from the member, `permanent = Some(member.permanent)`, `group_id`, `group_members` and
    /// `undo` cleared. When `group.undo == Some(true)` and `group.op_type == Recycle` the result's entry becomes the
    /// group entry and its verdict is replaced by the "is it back at its original path" rule (source `Exists` with
    /// size and last-write equal to the member => `AlreadyDone`; `Exists` otherwise => `Conflict`; `Missing` =>
    /// `RecycleUnverifiable` even when the member is `permanent`; `Changed`/`Unreadable` => `Unknown`).
    fn check_group_member(&self, group: &JournalEntry, member: &JournalGroupMember) -> RecoveryGroupMemberCheck {
        todo!("W3-07: port RecoveryFileCheck.CheckGroupMember")
    }

    /// C# `CheckPath`. Order of calls: folder first (`get_directory_name(path)`, non-empty => `directory_exists`),
    /// (a whitespace-only folder part is `Unreadable` before any stat), then the stat through the private seam. Under
    /// P1 option A there is NO blank/NUL pre-check on `path`: the seam reports it as `Err` after counting the stat.
    fn check_path(
        &self,
        path: &str,
        recorded_size: i64,
        recorded_last_write_utc: DateTimeUtc,
        compare_last_write: bool,
    ) -> RecoveryPathCheck {
        todo!("W3-07: port RecoveryFileCheck.CheckPath")
    }
}

/// C# `AggregateGroupVerdict(entry, members)`. Rules in this exact order (first match wins; `verdicts` = the member
/// verdicts, `all`/`contains` as LINQ; the list is never empty):
///
/// 1. `undo == Some(true)`, Move, all of `{CanRetry, AlreadyDone}` and contains `CanRetry` => `CanRetry`.
/// 2. `undo == Some(true)`, Recycle, all of `{RecycleUnverifiable, AlreadyDone}` and contains `RecycleUnverifiable`
///    => `CanRetry`.
/// 3. Recycle and `undo != Some(true)`, all of `{NotRecycled, RecycleUnverifiable, PermanentlyDeleted}` and contains
///    `NotRecycled` => `CanRetry` when every `NotRecycled` member's SOURCE status is `Exists` (not `Changed`), else
///    `NotRecycled`.
/// 4. all of `{AlreadyDone, RecycleUnverifiable, PermanentlyDeleted}`: all `PermanentlyDeleted` =>
///    `PermanentlyDeleted`; contains `PermanentlyDeleted` => `PartiallyPermanentlyDeleted`; else Recycle with
///    `undo != Some(true)` => `RecycleUnverifiable`, otherwise `AlreadyDone`.
/// 5. all of `{AlreadyDone, CanRetry}` and contains `CanRetry` => `CanRetry`.
/// 6. contains `Conflict` => `Conflict`; `SourceChanged`; `DestinationChanged`; `Lost`; `NotRecycled` (in that
///    order); otherwise `Unknown`.
fn aggregate_group_verdict(entry: &JournalEntry, members: &[RecoveryGroupMemberCheck]) -> RecoveryVerdict {
    todo!("W3-07: port RecoveryFileCheck.AggregateGroupVerdict")
}
