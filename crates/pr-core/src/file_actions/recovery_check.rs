//! W9-R3 ports the FEAT-RENAME stored-spelling delta from the C# source named below at 7eaee2a2.
//! Port of `PhotoReview.Core/FileActions/RecoveryFileCheck.cs` (C# baseline d92d6aae; the verdict table of the C#
//! `docs/architecture.md`, "Recovery check"; PARITY F-17, W-04).
//!
//! Read-only "is this journal entry still valid?" check for the Recovery window. It only calls
//! [`FileSystem::directory_exists`] / [`FileSystem::try_get_file_stat`] (P-12): it never moves, writes or deletes and never touches
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

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::abstractions::FileSystem;
use crate::file_actions::case_only_rename::CaseOnlyRename;
use crate::file_actions::journal_line::{JournalEntry, JournalGroupMember};
use crate::model::{FileOperationType, JournalState};
use crate::text::dotnet_path::get_directory_name;
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
        if let Some(members) = entry.group_members.as_deref().filter(|members| !members.is_empty()) {
            let checks: Vec<RecoveryGroupMemberCheck> = members
                .iter()
                .map(|member| self.check_group_member(entry, member))
                .collect();
            let verdict = aggregate_group_verdict(entry, &checks);
            let (source, destination) = match checks.first() {
                Some(first) => (first.check.source.clone(), first.check.destination.clone()),
                // Unreachable: `members` is non-empty, so `checks` holds one check per member.
                None => (
                    RecoveryPathCheck::new(entry.source.clone(), RecoveryPathStatus::Unreadable, None, None, false),
                    None,
                ),
            };
            return RecoveryCheckResult {
                entry: entry.clone(),
                source,
                destination,
                verdict,
                group_members: Some(checks),
            };
        }
        let source = self.check_path(
            &entry.source,
            entry.size,
            entry.last_write_utc.value,
            true,
            entry.destination.as_deref(),
        );
        let destination = entry
            .destination
            .as_deref()
            .filter(|destination| !is_null_or_white_space(destination))
            .map(|destination| {
                self.check_path(
                    destination,
                    entry.size,
                    entry.last_write_utc.value,
                    false,
                    Some(&entry.source),
                )
            });
        let verdict = Self::decide(entry, &source, destination.as_ref());
        RecoveryCheckResult {
            entry: entry.clone(),
            source,
            destination,
            verdict,
            group_members: None,
        }
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
        let s = source.status;
        if entry.op_type == FileOperationType::Recycle {
            return match s {
                RecoveryPathStatus::Missing if entry.permanent == Some(true) => RecoveryVerdict::PermanentlyDeleted,
                RecoveryPathStatus::Missing => RecoveryVerdict::RecycleUnverifiable,
                RecoveryPathStatus::Unreadable => RecoveryVerdict::Unknown,
                RecoveryPathStatus::Exists | RecoveryPathStatus::Changed => RecoveryVerdict::NotRecycled,
            };
        }

        let Some(destination) = destination else {
            return RecoveryVerdict::Unknown;
        };
        let d = destination.status;

        if entry.state == JournalState::Committed {
            if d == RecoveryPathStatus::Exists {
                return RecoveryVerdict::AlreadyDone;
            }
            return if d == RecoveryPathStatus::Missing && s == RecoveryPathStatus::Missing {
                RecoveryVerdict::Lost
            } else {
                RecoveryVerdict::Unknown
            };
        }
        if !matches!(entry.state, JournalState::Prepared | JournalState::Failed) {
            return RecoveryVerdict::Unknown;
        }
        if s == RecoveryPathStatus::Unreadable || d == RecoveryPathStatus::Unreadable {
            return RecoveryVerdict::Unknown;
        }

        let source_present = matches!(s, RecoveryPathStatus::Exists | RecoveryPathStatus::Changed);
        let destination_present = matches!(d, RecoveryPathStatus::Exists | RecoveryPathStatus::Changed);

        if !destination_present {
            if !source_present {
                return RecoveryVerdict::Lost;
            }
            return if s == RecoveryPathStatus::Exists {
                RecoveryVerdict::CanRetry
            } else {
                RecoveryVerdict::SourceChanged
            };
        }
        if !source_present {
            return if d == RecoveryPathStatus::Exists {
                RecoveryVerdict::AlreadyDone
            } else {
                RecoveryVerdict::DestinationChanged
            };
        }
        if entry.op_type == FileOperationType::Copy
            && s == RecoveryPathStatus::Exists
            && d == RecoveryPathStatus::Exists
        {
            return RecoveryVerdict::AlreadyDone;
        }
        RecoveryVerdict::Conflict
    }

    /// C# `CheckGroupMember(group, member)`: `check()` of a copy of the group entry with `source`, `destination`,
    /// `size`, `last_write_utc` from the member, `permanent = Some(member.permanent)`, `group_id`, `group_members` and
    /// `undo` cleared. When `group.undo == Some(true)` and `group.op_type == Recycle` the result's entry becomes the
    /// group entry and its verdict is replaced by the "is it back at its original path" rule (source `Exists`, which
    /// means size and last-write equal to the member's => `AlreadyDone`; `Missing` => `RecycleUnverifiable` even when
    /// the member is `permanent`; `Changed`/`Unreadable` => `Unknown`).
    fn check_group_member(&self, group: &JournalEntry, member: &JournalGroupMember) -> RecoveryGroupMemberCheck {
        let single_entry = JournalEntry {
            source: member.source.clone(),
            destination: member.destination.clone(),
            size: member.size,
            last_write_utc: member.last_write_utc,
            permanent: Some(member.permanent),
            group_id: None,
            group_members: None,
            undo: None,
            ..group.clone()
        };
        let mut single = self.check(&single_entry);
        if group.undo == Some(true) && group.op_type == FileOperationType::Recycle {
            // "Is the file back at its original path?" Ticks only (C# `DateTime ==` ignores Kind).
            // `Exists` already means size and last-write (ticks only, C# `DateTime ==` ignores Kind) equal the member's
            // (`check_path` with `compare_last_write: true`), so no second comparison is needed.
            let verdict = match single.source.status {
                RecoveryPathStatus::Exists => RecoveryVerdict::AlreadyDone,
                RecoveryPathStatus::Missing => RecoveryVerdict::RecycleUnverifiable,
                RecoveryPathStatus::Changed | RecoveryPathStatus::Unreadable => RecoveryVerdict::Unknown,
            };
            single.entry = group.clone();
            single.verdict = verdict;
        }
        RecoveryGroupMemberCheck {
            member: member.clone(),
            check: single,
        }
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
        twin: Option<&str>,
    ) -> RecoveryPathCheck {
        let mut folder_exists = false;
        if let Some(folder) = get_directory_name(path).filter(|folder| !folder.is_empty()) {
            // C# `DirectoryExists` throws `ArgumentException` for a whitespace-only folder, before any stat.
            if is_null_or_white_space(&folder) {
                return RecoveryPathCheck::new(path, RecoveryPathStatus::Unreadable, None, None, false);
            }
            folder_exists = self.file_system.directory_exists(Path::new(&folder));
        }
        // P-12 (option A): no blank/NUL pre-check on `path`; the seam reports such a path as `Err` after counting the
        // stat, like the C# proxies.
        let stat = match CaseOnlyRename::stat_as(self.file_system.as_ref(), path, twin) {
            Err(_) => return RecoveryPathCheck::new(path, RecoveryPathStatus::Unreadable, None, None, folder_exists),
            Ok(None) => return RecoveryPathCheck::new(path, RecoveryPathStatus::Missing, None, None, folder_exists),
            Ok(Some(stat)) => stat,
        };
        let length = i64::try_from(stat.length);
        let changed =
            length != Ok(recorded_size) || (compare_last_write && stat.last_write_utc != recorded_last_write_utc);
        let status = if changed {
            RecoveryPathStatus::Changed
        } else {
            RecoveryPathStatus::Exists
        };
        RecoveryPathCheck::new(
            path,
            status,
            Some(length.unwrap_or(i64::MAX)),
            Some(stat.last_write_utc),
            folder_exists,
        )
    }
}

/// C# `AggregateGroupVerdict(entry, members)`. Rules in this exact order (first match wins; `verdicts` = the member
/// verdicts, `all`/`contains` as LINQ; the list is never empty):
///
/// 1. `undo == Some(true)`, Recycle, all of `{RecycleUnverifiable, AlreadyDone}` and contains `RecycleUnverifiable`
///    => `CanRetry`.
/// 2. Recycle and `undo != Some(true)`, all of `{NotRecycled, RecycleUnverifiable, PermanentlyDeleted}` and contains
///    `NotRecycled` => `CanRetry` when every `NotRecycled` member's SOURCE status is `Exists` (not `Changed`), else
///    `NotRecycled`.
/// 3. all of `{AlreadyDone, RecycleUnverifiable, PermanentlyDeleted}`: all `PermanentlyDeleted` =>
///    `PermanentlyDeleted`; contains `PermanentlyDeleted` => `PartiallyPermanentlyDeleted`; else Recycle with
///    `undo != Some(true)` => `RecycleUnverifiable`, otherwise `AlreadyDone`.
/// 4. all of `{AlreadyDone, CanRetry}` and contains `CanRetry` => `CanRetry`. (The C# code
///    has an earlier rule for the UNDO of a Move with this same condition and the same result. It is redundant: rules 1
///    to 3 each require every member verdict to be outside `CanRetry`, so none of them matches before rule 4. The
///    Rust port omits it; the truth-table test in this file's `tests` module covers every input.)
/// 5. contains `Conflict` => `Conflict`; `SourceChanged`; `DestinationChanged`; `Lost`; `NotRecycled` (in that
///    order); otherwise `Unknown`.
fn aggregate_group_verdict(entry: &JournalEntry, members: &[RecoveryGroupMemberCheck]) -> RecoveryVerdict {
    use RecoveryVerdict as V;
    let verdicts: Vec<RecoveryVerdict> = members.iter().map(RecoveryGroupMemberCheck::verdict).collect();
    let all = |pred: &dyn Fn(RecoveryVerdict) -> bool| verdicts.iter().all(|v| pred(*v));
    let contains = |wanted: RecoveryVerdict| verdicts.contains(&wanted);
    let undo = entry.undo == Some(true);

    if undo
        && entry.op_type == FileOperationType::Recycle
        && all(&|v| matches!(v, V::RecycleUnverifiable | V::AlreadyDone))
        && contains(V::RecycleUnverifiable)
    {
        return V::CanRetry;
    }
    // A non-undo Delete (Recycle) group that stopped part-way: the members still on disk can be recycled again, the
    // members already gone stay as they are. A present member that no longer matches the journal blocks the retry.
    if entry.op_type == FileOperationType::Recycle
        && !undo
        && all(&|v| matches!(v, V::NotRecycled | V::RecycleUnverifiable | V::PermanentlyDeleted))
        && contains(V::NotRecycled)
    {
        let retryable = members
            .iter()
            .filter(|member| member.verdict() == V::NotRecycled)
            .all(|member| member.source().status == RecoveryPathStatus::Exists);
        return if retryable { V::CanRetry } else { V::NotRecycled };
    }
    if all(&|v| matches!(v, V::AlreadyDone | V::RecycleUnverifiable | V::PermanentlyDeleted)) {
        // Only when EVERY file was deleted permanently is the whole capture unrecoverable.
        if all(&|v| v == V::PermanentlyDeleted) {
            return V::PermanentlyDeleted;
        }
        if contains(V::PermanentlyDeleted) {
            return V::PartiallyPermanentlyDeleted;
        }
        // An undo of a Recycle whose members are all back on disk is done; only a plain Recycle stays unverifiable.
        return if entry.op_type == FileOperationType::Recycle && !undo {
            V::RecycleUnverifiable
        } else {
            V::AlreadyDone
        };
    }
    if all(&|v| matches!(v, V::AlreadyDone | V::CanRetry)) && contains(V::CanRetry) {
        return V::CanRetry;
    }
    for verdict in [
        V::Conflict,
        V::SourceChanged,
        V::DestinationChanged,
        V::Lost,
        V::NotRecycled,
    ] {
        if contains(verdict) {
            return verdict;
        }
    }
    V::Unknown
}

/// C# `string.IsNullOrWhiteSpace` for a non-null string (`char::is_whitespace` = .NET `char.IsWhiteSpace`).
fn is_null_or_white_space(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    //! P-393b: exhaustive truth-table proof that `aggregate_group_verdict` needs no separate "undo Move" rule (the later
    //! "all of {AlreadyDone, CanRetry} and contains CanRetry" rule returns the same `CanRetry` for it). The reference
    //! below restates the documented rules independently; the flag `with_undo_move_rule` switches the old rule on so the
    //! test shows both forms agree, and the production function must equal the form without it.
    use super::*;
    use crate::file_actions::journal_line::JournalDateTime;

    use RecoveryVerdict as V;

    const VERDICTS: [RecoveryVerdict; 11] = [
        V::CanRetry,
        V::AlreadyDone,
        V::Conflict,
        V::SourceChanged,
        V::DestinationChanged,
        V::Lost,
        V::Unknown,
        V::RecycleUnverifiable,
        V::NotRecycled,
        V::PermanentlyDeleted,
        V::PartiallyPermanentlyDeleted,
    ];
    const STATUSES: [RecoveryPathStatus; 4] = [
        RecoveryPathStatus::Exists,
        RecoveryPathStatus::Missing,
        RecoveryPathStatus::Changed,
        RecoveryPathStatus::Unreadable,
    ];
    const OPS: [FileOperationType; 3] = [
        FileOperationType::Move,
        FileOperationType::Copy,
        FileOperationType::Recycle,
    ];
    const UNDOS: [Option<bool>; 3] = [None, Some(false), Some(true)];

    type Picked = (RecoveryVerdict, RecoveryPathStatus);

    fn stamp() -> DateTimeUtc {
        DateTimeUtc::from_ticks(639_264_384_000_000_000).expect("ticks in range")
    }

    fn entry(op: FileOperationType, undo: Option<bool>) -> JournalEntry {
        JournalEntry {
            id: "group".to_owned(),
            op_type: op,
            state: JournalState::Prepared,
            source: r"C:\photos\group.jpg".to_owned(),
            destination: None,
            size: 0,
            last_write_utc: JournalDateTime::utc(stamp()),
            timestamp_utc: JournalDateTime::utc(stamp()),
            error: None,
            error_code: None,
            permanent: None,
            undo,
            group_id: Some("capture".to_owned()),
            group_members: None,
        }
    }

    fn member(verdict: RecoveryVerdict, status: RecoveryPathStatus) -> RecoveryGroupMemberCheck {
        let source = RecoveryPathCheck::new(r"C:\photos\m.jpg", status, None, None, false);
        RecoveryGroupMemberCheck {
            member: JournalGroupMember {
                source: source.path.clone(),
                destination: None,
                size: 0,
                last_write_utc: JournalDateTime::utc(stamp()),
                permanent: false,
            },
            check: RecoveryCheckResult {
                entry: entry(FileOperationType::Copy, None),
                source,
                destination: None,
                verdict,
                group_members: None,
            },
        }
    }

    /// Independent restatement of the documented rules; `with_undo_move_rule` adds the old rule 1.
    fn reference(op: FileOperationType, undo: Option<bool>, members: &[Picked], with_undo_move_rule: bool) -> V {
        let undo = undo == Some(true);
        let all = |set: &[V]| members.iter().all(|(v, _)| set.contains(v));
        let has = |v: V| members.iter().any(|(m, _)| *m == v);
        if with_undo_move_rule
            && undo
            && op == FileOperationType::Move
            && all(&[V::CanRetry, V::AlreadyDone])
            && has(V::CanRetry)
        {
            return V::CanRetry;
        }
        if undo
            && op == FileOperationType::Recycle
            && all(&[V::RecycleUnverifiable, V::AlreadyDone])
            && has(V::RecycleUnverifiable)
        {
            return V::CanRetry;
        }
        if op == FileOperationType::Recycle
            && !undo
            && all(&[V::NotRecycled, V::RecycleUnverifiable, V::PermanentlyDeleted])
            && has(V::NotRecycled)
        {
            let all_present_unchanged = members
                .iter()
                .filter(|(v, _)| *v == V::NotRecycled)
                .all(|(_, s)| *s == RecoveryPathStatus::Exists);
            return if all_present_unchanged {
                V::CanRetry
            } else {
                V::NotRecycled
            };
        }
        if all(&[V::AlreadyDone, V::RecycleUnverifiable, V::PermanentlyDeleted]) {
            if all(&[V::PermanentlyDeleted]) {
                return V::PermanentlyDeleted;
            }
            if has(V::PermanentlyDeleted) {
                return V::PartiallyPermanentlyDeleted;
            }
            return if op == FileOperationType::Recycle && !undo {
                V::RecycleUnverifiable
            } else {
                V::AlreadyDone
            };
        }
        if all(&[V::AlreadyDone, V::CanRetry]) && has(V::CanRetry) {
            return V::CanRetry;
        }
        [
            V::Conflict,
            V::SourceChanged,
            V::DestinationChanged,
            V::Lost,
            V::NotRecycled,
        ]
        .into_iter()
        .find(|v| has(*v))
        .unwrap_or(V::Unknown)
    }

    #[derive(Default)]
    struct Counts {
        groups: usize,
        undo_move_rule_domain: usize,
    }

    fn check(picked: &[Picked], counts: &mut Counts) {
        let members: Vec<RecoveryGroupMemberCheck> = picked.iter().map(|(v, s)| member(*v, *s)).collect();
        for op in OPS {
            for undo in UNDOS {
                let actual = aggregate_group_verdict(&entry(op, undo), &members);
                let without = reference(op, undo, picked, false);
                let with = reference(op, undo, picked, true);
                assert_eq!(actual, without, "{op:?} undo {undo:?} members {picked:?}");
                assert_eq!(
                    with, without,
                    "the undo-Move rule changes the result: {op:?} undo {undo:?} {picked:?}"
                );
                counts.groups += 1;
                if undo == Some(true)
                    && op == FileOperationType::Move
                    && picked.iter().all(|(v, _)| matches!(v, V::CanRetry | V::AlreadyDone))
                    && picked.iter().any(|(v, _)| *v == V::CanRetry)
                {
                    counts.undo_move_rule_domain += 1;
                    assert_eq!(actual, V::CanRetry);
                }
            }
        }
    }

    #[test]
    fn aggregate_group_verdict_is_identical_with_and_without_the_undo_move_rule_for_every_member_combination() {
        // Domain: 3 ops (Move, Copy, Recycle: the only values) x 3 undo values x every ordered group of 1..=3 members over
        // the 44 (verdict, source status) pairs (11 verdicts x 4 statuses), every ordered group of 4 verdicts with
        // status Exists, and every non-empty SET of verdicts (2047) with the status of every member uniform for each of
        // the 4 statuses. The function reads the members only through `all`/`contains` of the verdict and the source
        // status of the `NotRecycled` members, so a set plus the statuses covers any longer group.
        let pairs: Vec<Picked> = VERDICTS
            .iter()
            .flat_map(|v| STATUSES.iter().map(move |s| (*v, *s)))
            .collect();
        let mut counts = Counts::default();
        for a in &pairs {
            check(&[*a], &mut counts);
            for b in &pairs {
                check(&[*a, *b], &mut counts);
                for c in &pairs {
                    check(&[*a, *b, *c], &mut counts);
                }
            }
        }
        let exists = RecoveryPathStatus::Exists;
        for a in VERDICTS {
            for b in VERDICTS {
                for c in VERDICTS {
                    for d in VERDICTS {
                        check(&[(a, exists), (b, exists), (c, exists), (d, exists)], &mut counts);
                    }
                }
            }
        }
        for mask in 1u32..(1 << VERDICTS.len()) {
            for status in STATUSES {
                let picked: Vec<Picked> = VERDICTS
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| mask & (1 << i) != 0)
                    .map(|(_, v)| (*v, status))
                    .collect();
                check(&picked, &mut counts);
            }
        }
        assert!(counts.groups > 700_000, "{}", counts.groups);
        assert!(
            counts.undo_move_rule_domain > 100,
            "the domain of the undo-Move rule was barely reached: {}",
            counts.undo_move_rule_domain
        );
    }
}
