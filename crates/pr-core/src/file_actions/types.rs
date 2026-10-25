//! Port of `PhotoReview.Core/FileActions/{FileFingerprint,FileActionRequest,FileActionResult,
//! CaptureGroupActionRequest,CaptureGroupActionResult,UndoResult,JournalGroupMember}.cs` (C# baseline e441d67a).
//! `CopyCreationProof` and `SkippedEntry` already live in [`crate::abstractions`].
//!
//! These are plain data records. C# records have positional parameters with defaults; Rust has no defaults, so each
//! record has a `new(..)` taking the parameters that have NO default in C#, filling the others with the C# default.
//! Set the remaining fields afterwards (all fields are `pub`) or use struct-update syntax:
//! `FileActionResult { rejected: true, ..FileActionResult::new(..) }`.
//!
//! Paths are `String`s like the C# `string` (they are journaled and compared as text). Equality is the C# record
//! equality: every field, strings compared exactly (case-sensitive), which is what `#[derive(PartialEq)]` gives.

use crate::abstractions::FileStat;
use crate::catalog::types::CaptureGroup;
use crate::model::FileOperationType;
use crate::time::{DateTimeUtc, TICKS_PER_SECOND};

// Defined by W1-14 (journal line format); re-exported here because C# kept it next to these records.
pub use super::journal_line::{JournalEntry, JournalGroupMember};

// ---------------------------------------------------------------------------------------------------------------
// FileFingerprint (C# `internal static class`)
// ---------------------------------------------------------------------------------------------------------------

/// `FileFingerprint.Tolerance` in ticks: the coarsest rounding of a Windows volume (FAT/FAT32 2 s); exFAT (10 ms)
/// and NTFS fall well inside.
pub const FINGERPRINT_TOLERANCE_TICKS: i64 = 2 * TICKS_PER_SECOND;

/// `FileFingerprint.MatchesMovedDestination` (RV-D1, option A): identity check for a file that was MOVED to another
/// place and is compared there against the stamp the source had. The destination volume may store the last-write
/// time with less precision, so equality of the write time is too strict.
///
/// True when `stat.length == size` (exact; a `stat.length` above `i64::MAX` is a mismatch) AND the absolute
/// difference between `stat.last_write_utc` and `recorded_utc`, in ticks, is `<= FINGERPRINT_TOLERANCE_TICKS`
/// (inclusive: exactly 2 s apart still matches, 2 s + 1 tick does not; earlier and later are treated alike).
pub fn matches_moved_destination(stat: &FileStat, size: i64, recorded_utc: DateTimeUtc) -> bool {
    let same_length = matches!(i64::try_from(stat.length), Ok(length) if length == size);
    let tolerance = FINGERPRINT_TOLERANCE_TICKS.unsigned_abs();
    same_length && stat.last_write_utc.ticks().abs_diff(recorded_utc.ticks()) <= tolerance
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionRequest / FileActionResult
// ---------------------------------------------------------------------------------------------------------------

/// A Move, Copy or Recycle of one file. `allow_permanent_delete` (Q-R8) is true only after the user enabled the
/// setting and confirmed: it lets Recycle delete permanently on a drive without a Recycle Bin (default false =
/// refuse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileActionRequest {
    pub source: String,
    pub operation: FileOperationType,
    pub destination: Option<String>,
    pub allow_permanent_delete: bool,
}

impl FileActionRequest {
    /// `destination` = None, `allow_permanent_delete` = false.
    pub fn new(source: impl Into<String>, operation: FileOperationType) -> Self {
        Self {
            source: source.into(),
            operation,
            destination: None,
            allow_permanent_delete: false,
        }
    }
}

/// Result of one file action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileActionResult {
    pub succeeded: bool,
    pub operation: FileOperationType,
    pub source: String,
    pub destination_path: Option<String>,
    pub size: i64,
    pub last_write_utc: DateTimeUtc,
    pub error: Option<String>,
    /// Refused before anything was journaled or touched. Default false.
    pub rejected: bool,
    /// False when the journal line could not be written. Default TRUE.
    pub journal_persisted: bool,
    pub journal_error: Option<String>,
    /// A Recycle that deleted the file permanently (Q-R8). Default false.
    pub permanently_deleted: bool,
    /// F3: a FAILED Move whose source no longer exists on disk (e.g. the size differed after the move, so the result
    /// is unverified). The file is at `destination_path`; the caller must not put `source` back into the review
    /// catalog. Always computed from the file system after the failure, never assumed; false for Copy/Recycle and
    /// successes. Default false.
    pub source_removed: bool,
    /// FEAT-RENAME: source already has the requested exact spelling; no journal line was written.
    pub no_change: bool,
}

impl FileActionResult {
    /// Defaults: `rejected` false, `journal_persisted` TRUE, `journal_error` None, `permanently_deleted` false,
    /// `source_removed` false.
    pub fn new(
        succeeded: bool,
        operation: FileOperationType,
        source: impl Into<String>,
        destination_path: Option<String>,
        size: i64,
        last_write_utc: DateTimeUtc,
        error: Option<String>,
    ) -> Self {
        Self {
            succeeded,
            operation,
            source: source.into(),
            destination_path,
            size,
            last_write_utc,
            error,
            rejected: false,
            journal_persisted: true,
            journal_error: None,
            permanently_deleted: false,
            source_removed: false,
            no_change: false,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// CaptureGroupActionRequest / CaptureGroupActionResult / CaptureGroupMemberResult
// ---------------------------------------------------------------------------------------------------------------

/// A Move, Copy or Recycle to apply to every member of one capture group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureGroupActionRequest {
    pub group: CaptureGroup,
    pub operation: FileOperationType,
    pub destination: Option<String>,
    pub allow_permanent_delete: bool,
    /// FEAT-RENAME: optional source-path to complete target-file-name map.
    pub destination_file_names: Option<super::rename::PathKeyMap>,
}

impl CaptureGroupActionRequest {
    /// `destination` = None, `allow_permanent_delete` = false.
    pub fn new(group: CaptureGroup, operation: FileOperationType) -> Self {
        Self {
            group,
            operation,
            destination: None,
            allow_permanent_delete: false,
            destination_file_names: None,
        }
    }
}

/// Post-action state for one image or sidecar in a capture group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureGroupMemberResult {
    pub member: JournalGroupMember,
    pub completed: bool,
    pub conflict: bool,
    pub error: Option<String>,
    /// Default false.
    pub source_exists: bool,
    /// Default false.
    pub destination_exists: bool,
    /// False when the file system could not be asked. Default TRUE.
    pub state_known: bool,
}

impl CaptureGroupMemberResult {
    /// Defaults: `error` None, `source_exists` false, `destination_exists` false, `state_known` TRUE.
    pub fn new(member: JournalGroupMember, completed: bool, conflict: bool) -> Self {
        Self {
            member,
            completed,
            conflict,
            error: None,
            source_exists: false,
            destination_exists: false,
            state_known: true,
        }
    }
}

/// Result of one journaled capture-group action.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureGroupActionResult {
    pub succeeded: bool,
    pub rejected: bool,
    pub operation: FileOperationType,
    pub group_id: String,
    /// The manifest journal line, when one was written. Boxed: `JournalEntry` is large.
    pub entry: Option<Box<JournalEntry>>,
    pub members: Vec<CaptureGroupMemberResult>,
    pub error: Option<String>,
    /// Default TRUE.
    pub journal_persisted: bool,
    pub journal_error: Option<String>,
    /// Default false.
    pub permanently_deleted: bool,
    /// Partner files that were missing and skipped. Default None.
    pub skipped_missing: Option<Vec<String>>,
}

impl CaptureGroupActionResult {
    /// Defaults: `journal_persisted` TRUE, `journal_error` None, `permanently_deleted` false, `skipped_missing` None.
    pub fn new(
        succeeded: bool,
        rejected: bool,
        operation: FileOperationType,
        group_id: impl Into<String>,
        entry: Option<Box<JournalEntry>>,
        members: Vec<CaptureGroupMemberResult>,
        error: Option<String>,
    ) -> Self {
        Self {
            succeeded,
            rejected,
            operation,
            group_id: group_id.into(),
            entry,
            members,
            error,
            journal_persisted: true,
            journal_error: None,
            permanently_deleted: false,
            skipped_missing: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// UndoResult
// ---------------------------------------------------------------------------------------------------------------

/// Result of undoing a file action (Move or Recycle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoResult {
    pub succeeded: bool,
    /// `None` when there was nothing to undo (C# `FileOperationType?`).
    pub operation: Option<FileOperationType>,
    pub source: String,
    pub destination: Option<String>,
    pub error_message: Option<String>,
    /// Default false.
    pub rejected: bool,
    /// Group undo: the paths that were restored. Default None.
    pub restored_paths: Option<Vec<String>>,
}

impl UndoResult {
    /// Defaults: `rejected` false, `restored_paths` None.
    pub fn new(
        succeeded: bool,
        operation: Option<FileOperationType>,
        source: impl Into<String>,
        destination: Option<String>,
        error_message: Option<String>,
    ) -> Self {
        Self {
            succeeded,
            operation,
            source: source.into(),
            destination,
            error_message,
            rejected: false,
            restored_paths: None,
        }
    }
}
