//! Port of `PhotoReview.Core/FileActions/FileActionService.cs` (C# baseline d92d6aae; PARITY F-01, F-02, F-03, F-04, F-05,
//! F-06, F-21, R01, SEC-01, INV-3 (the F3 `source_removed` fact), INV-4). Contract: `contracts/W3-04.md`.
//!
//! Runs ONE journaled file action at a time (INV-4):
//!
//! * [`FileActionService::execute`]: a single Move, Copy or Recycle (C# `ExecuteAsync`);
//! * [`FileActionService::execute_group`]: one capture group (JPEG + RAW + optional XMP) as ONE manifest journal line
//!   (C# `ExecuteGroupAsync`), with compensation (Copy: delete the copies this call made; Move: put moved members back)
//!   when a member fails.
//!
//! Every action follows Prepared -> mutate -> verify -> Committed/Failed (or Dismissed when the disk is provably
//! unchanged) through W3-01's `JournalTransaction`. The journal line format is W1-14 (`journal_line`) and must stay
//! byte-identical to C#.
//!
//! The data records ([`FileActionRequest`], [`FileActionResult`], [`CaptureGroupActionRequest`],
//! [`CaptureGroupActionResult`], [`CaptureGroupMemberResult`]) and `FileFingerprint`
//! ([`super::types::matches_moved_destination`]) are W1-09 (`file_actions::types`); `RecycleBinCapacity.FirstOverflow`
//! is W1-13 ([`super::errors::first_overflow`]); `PartialDestinationCleanup` is W3-08 (`file_actions::partial_cleanup`).
//!
//! Threading (docs/adr/R0003): C# awaits `Task.Run(mutation, ct)` for every file mutation. Here every call is BLOCKING and
//! runs on the caller's thread (the App's file-action lane, never the UI thread). The `Task.Run(..., ct)` hand-off becomes
//! one explicit cancellation check right before the mutation (see the contract, "Cancellation").
// TEMPORARY (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;

use crate::abstractions::{Clock, FileStat, FileSystem, RecycleBin};
use crate::file_actions::journal::OperationJournal;
use crate::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, CaptureGroupMemberResult, FileActionRequest, FileActionResult,
    JournalGroupMember,
};
use crate::file_actions::undo::{FileActionGate, MoveOverride};
use crate::model::FileOperationType;
use crate::sync::CancelToken;
use crate::text::dotnet_path::PathError;
use crate::time::DateTimeUtc;

// ---------------------------------------------------------------------------------------------------------------
// .NET exception messages that reach the user (`ex.Message`)
// ---------------------------------------------------------------------------------------------------------------

/// `OperationCanceledException.Message` (.NET `SR.OperationCanceled`): what C# `ThrowIfCancellationRequested()` raises,
/// i.e. a cancellation noticed at the top of a group loop iteration (preflight or member loop). Also the message of a
/// `Canceled` returned by a move override or a file-system call (C# `throw new OperationCanceledException()`).
pub const OPERATION_CANCELED_MESSAGE: &str = "The operation was canceled.";

/// `TaskCanceledException.Message` (.NET `SR.TaskCanceledException`): what `await Task.Run(mutation, ct)` raises when
/// `ct` is already cancelled when the task would start, i.e. a cancellation noticed at the mutation hand-off point.
/// Pinned by the golden cases `single-*-pre-cancelled`. In a group the loop's own check comes first, so a group shows it only
/// through a race.
pub const TASK_CANCELED_MESSAGE: &str = "A task was canceled.";

// ---------------------------------------------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------------------------------------------

/// The localized texts this unit needs: C# `Tr.*` (generated from `en.json`; key in each doc line). pr-core cannot see
/// pr-l10n (D13), so the App composition root passes an implementation over the real catalogs;
/// [`EnglishFileActionTexts`] is the built-in English catalog (what C# shows until a language is set, and what the
/// oracle/golden run with). The UI-language message of a CODED journal error (C# `new JournalCodedException(code)`) is
/// NOT here: it comes from the journal's own catalog, `OperationJournal::texts().localized(key)` (W3-01), exactly like
/// the coded errors `JournalTransaction` raises, so both always speak the same language.
///
/// The forward-move test seam is W3-05's [`MoveOverride`] (`Arc<dyn Fn(&Path, &Path) -> io::Result<()>>`): the
/// composition root hands the SAME override to this service and to `UndoService`.
pub trait FileActionTexts: Send + Sync {
    /// `Tr.CoreFileActionBusy` (`core.fileAction.busy`).
    fn core_file_action_busy(&self) -> String;
    /// `Tr.CoreFileActionGroupNeedsTwoPaths` (`core.fileAction.groupNeedsTwoPaths`).
    fn core_file_action_group_needs_two_paths(&self) -> String;
    /// `Tr.CoreFileActionUnsupportedOperation(operation)` (`core.fileAction.unsupportedOperation`, placeholder
    /// `{operation}` = the enum name). Unreachable in Rust (the enum is closed) but kept for parity of the catalog.
    fn core_file_action_unsupported_operation(&self, operation: FileOperationType) -> String;
    /// `Tr.CoreFileActionNoDestination` (`core.fileAction.noDestination`).
    fn core_file_action_no_destination(&self) -> String;
    /// `Tr.CoreFileActionSameFolder` (`core.fileAction.sameFolder`).
    fn core_file_action_same_folder(&self) -> String;
    /// `Tr.CoreFileActionDestinationExists(path)` (`core.fileAction.destinationExists`, placeholder `{path}`).
    fn core_file_action_destination_exists(&self, path: &str) -> String;
    /// `Tr.CoreFileActionSourceMissing(path)` (`core.fileAction.sourceMissing`, placeholder `{path}`).
    fn core_file_action_source_missing(&self, path: &str) -> String;
    /// `Tr.CoreRecycleUnsupportedDrive(name)` (`core.recycle.unsupportedDrive`, placeholder `{name}`).
    fn core_recycle_unsupported_drive(&self, name: &str) -> String;
    /// `Tr.CoreRecycleBinCannotHold(name)` (`core.recycle.binCannotHold`, placeholder `{name}`).
    fn core_recycle_bin_cannot_hold(&self, name: &str) -> String;
    /// `Tr.CoreGroupActionRollbackFailed(message, count)` (`core.groupAction.rollbackFailed`, placeholders `{message}`,
    /// `{count}`; the number is rendered invariant, W1-11).
    fn core_group_action_rollback_failed(&self, message: &str, count: usize) -> String;
}

/// The built-in English catalog (`assets/languages/en.json` at d92d6aae). Complete: no porter work. A test compares every
/// text with the asset (see the contract, "Done when").
#[derive(Debug, Default, Clone, Copy)]
pub struct EnglishFileActionTexts;

impl FileActionTexts for EnglishFileActionTexts {
    fn core_file_action_busy(&self) -> String {
        "A previous operation is still running.".to_owned()
    }
    fn core_file_action_group_needs_two_paths(&self) -> String {
        "A grouped file action needs at least two files.".to_owned()
    }
    fn core_file_action_unsupported_operation(&self, operation: FileOperationType) -> String {
        let name = match operation {
            FileOperationType::Move => "Move",
            FileOperationType::Copy => "Copy",
            FileOperationType::Recycle => "Recycle",
        };
        format!("Operation not supported: {name}")
    }
    fn core_file_action_no_destination(&self) -> String {
        "The action has no destination folder.".to_owned()
    }
    fn core_file_action_same_folder(&self) -> String {
        "Cannot Move/Copy into the source folder itself.".to_owned()
    }
    fn core_file_action_destination_exists(&self, path: &str) -> String {
        format!("The destination already exists: {path}")
    }
    fn core_file_action_source_missing(&self, path: &str) -> String {
        format!("The source does not exist: {path}")
    }
    fn core_recycle_unsupported_drive(&self, name: &str) -> String {
        format!(
            "\"{name}\" is on a removable or network drive that has no Recycle Bin, so it was NOT deleted (it would be removed permanently). Delete it in Explorer if you really want it gone."
        )
    }
    fn core_recycle_bin_cannot_hold(&self, name: &str) -> String {
        format!(
            "\"{name}\" was NOT deleted: the Recycle Bin on this drive is turned off, is smaller than this file, or its settings could not be read, so Windows would delete the file permanently. Check the drive's settings in Recycle Bin > Properties, or delete the file in Explorer if you really want it gone."
        )
    }
    fn core_group_action_rollback_failed(&self, message: &str, count: usize) -> String {
        format!(
            "{message} In addition, {count} file(s) that were already processed could not be put back automatically; check Recovery."
        )
    }
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionService
// ---------------------------------------------------------------------------------------------------------------

/// C# `FileActionService` (`sealed class`). Thread-safe; share it as `Arc<FileActionService>`: the App, `UndoService`
/// (W3-05) and `RecoveryRetryService` (W3-06), both through W3-05's [`FileActionGate`] trait, all use the
/// SAME instance, so its gate is the one INV-4 gate of the process.
pub struct FileActionService {
    journal: Arc<OperationJournal>,
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    recycle_bin: Arc<dyn RecycleBin>,
    /// C# `_moveOverride`; `None` = `FileSystem::move_file`.
    move_override: Option<MoveOverride>,
    texts: Arc<dyn FileActionTexts>,
    /// C# `private int _inProgress` (0 = free, 1 = held). `AtomicI32` because C# uses `int` (D3); every access is
    /// `SeqCst` (C# `Interlocked.CompareExchange` / `Volatile.Read` / `Volatile.Write`).
    in_progress: AtomicI32,
}

impl fmt::Debug for FileActionService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileActionService")
            .field("journal", &self.journal)
            .field("move_override", &self.move_override.is_some())
            .field("in_progress", &self.in_progress)
            .finish_non_exhaustive()
    }
}

impl FileActionService {
    /// `new FileActionService(journal, fileSystem, clock, recycleBin)` (no move override, English texts). The C#
    /// `ArgumentNullException` guards have no Rust twin (the arguments cannot be null).
    pub fn new(
        journal: Arc<OperationJournal>,
        fs: Arc<dyn FileSystem>,
        clock: Arc<dyn Clock>,
        recycle_bin: Arc<dyn RecycleBin>,
    ) -> Self {
        Self {
            journal,
            fs,
            clock,
            recycle_bin,
            move_override: None,
            texts: Arc::new(EnglishFileActionTexts),
            in_progress: AtomicI32::new(0),
        }
    }

    /// The C# `moveOverride` constructor argument (test seam).
    pub fn with_move_override(mut self, move_override: MoveOverride) -> Self {
        self.move_override = Some(move_override);
        self
    }

    /// The UI-language catalog (C# reads the ambient `Tr.*`); default [`EnglishFileActionTexts`].
    pub fn with_texts(mut self, texts: Arc<dyn FileActionTexts>) -> Self {
        self.texts = texts;
        self
    }

    /// C# `IsBusy`: `in_progress != 0` (`SeqCst` load).
    pub fn is_busy(&self) -> bool {
        todo!("W3-04: Volatile.Read(ref _inProgress) != 0")
    }

    /// C# `TryBegin()` (INV-4): compare-exchange 0 -> 1 (`SeqCst`); `true` when THIS call took the gate.
    pub fn try_begin(&self) -> bool {
        todo!("W3-04: Interlocked.CompareExchange(ref _inProgress, 1, 0) == 0")
    }

    /// C# `End()`: unconditional `SeqCst` store of 0 (also when the gate was not held).
    pub fn end(&self) {
        todo!("W3-04: Volatile.Write(ref _inProgress, 0)")
    }

    /// C# `LacksRecycleBin(path)` (Q-R8): `!recycle_bin.can_recycle(path)`, i.e. a Recycle of `path` would be a
    /// PERMANENT delete. Callers (App) use it to decide whether the user must confirm.
    pub fn lacks_recycle_bin(&self, path: &str) -> bool {
        todo!("W3-04: !_recycleBin.CanRecycle(path)")
    }

    /// C# `ExecuteGroupAsync(request, ct)` (F-06). Never returns an error and never panics on file-system failures: every
    /// outcome is a [`CaptureGroupActionResult`] (contract "Semantics", group section, steps G1-G9).
    ///
    /// Order: gate (`try_begin`; refused => `rejected` result with `core_file_action_busy`, nothing read or journaled, the
    /// gate is NOT released by this call); group id (`crate::io::physical::new_guid_n()`, C# `Guid.NewGuid().ToString("N")`, generated BEFORE the gate check, so a
    /// rejected result carries one too); validation; preflight of every member (missing partners are skipped and
    /// reported); capacity (Recycle); destination folder; ONE Prepared manifest line; mutations in member order;
    /// `Committed`; on failure compensation, inspection, then `Failed` or `Dismissed` (fully rolled back). The gate is
    /// released on every path after the gate was taken (RAII guard calling [`Self::end`]).
    pub fn execute_group(&self, request: &CaptureGroupActionRequest, cancel: &CancelToken) -> CaptureGroupActionResult {
        todo!("W3-04: port ExecuteGroupAsync")
    }

    /// C# `ExecuteAsync(request, ct)` (F-01..F-05). Never returns an error: every outcome is a [`FileActionResult`]
    /// (contract "Semantics", single section, steps S1-S8).
    ///
    /// Gate refused => `rejected: true`, `succeeded: false`, `destination_path: None`, `size: 0`,
    /// `last_write_utc: DateTimeUtc::MIN`, `error: core_file_action_busy` (no id is generated, nothing is read).
    pub fn execute(&self, request: &FileActionRequest, cancel: &CancelToken) -> FileActionResult {
        todo!("W3-04: port ExecuteAsync")
    }

    // -----------------------------------------------------------------------------------------------------------
    // Private helpers (C# private methods). Signatures are a suggestion; the porter may change PRIVATE items.
    // -----------------------------------------------------------------------------------------------------------

    /// C# `CreateDestinationFolder` (lines 290-302): walks up from `trim_ending_directory_separator(destination_folder)`
    /// while the text is non-empty and `!directory_exists(folder)`, collecting each missing folder (deepest first; the
    /// walk uses `dotnet_path::get_directory_name`, `None` ends it), THEN calls `create_directory(destination_folder)`
    /// (the untrimmed text). Returns the folders this call brought into existence. A `create_directory` error is returned
    /// (C# exception) and nothing is returned as created.
    fn create_destination_folder(&self, destination_folder: &str) -> io::Result<Vec<String>> {
        todo!("W3-04: port CreateDestinationFolder")
    }

    /// C# `RemoveCreatedEmptyFolders` (lines 308-321): for each folder in order, `try_delete_empty_directory`; stop at the
    /// first `false`. Never fails (the Rust trait method cannot fail; see the contract, trap T9).
    fn remove_created_empty_folders(&self, created_folders: &[String]) {
        todo!("W3-04: port RemoveCreatedEmptyFolders")
    }

    /// C# `RemoveCreatedCopies` (lines 332-382): Copy compensation. `created_copies` is keyed by
    /// `crate::text::ignore_case_key(destination)` (C# `StringComparer.OrdinalIgnoreCase`). Returns the number of files
    /// that could not be cleaned up ("stuck").
    fn remove_created_copies(
        &self,
        done: &[JournalGroupMember],
        in_flight: Option<&JournalGroupMember>,
        in_flight_destination_is_ours: bool,
        created_copies: &HashMap<String, FileStat>,
    ) -> usize {
        todo!("W3-04: port RemoveCreatedCopies")
    }

    /// C# `RestoreMovedMembers` (lines 394-433): Move compensation, with the plain `FileSystem::move_file` (never the
    /// override). Candidates: the in-flight member first (`was_done = false`), then `done` in REVERSE order. Returns stuck.
    fn restore_moved_members(&self, done: &[JournalGroupMember], in_flight: Option<&JournalGroupMember>) -> usize {
        todo!("W3-04: port RestoreMovedMembers")
    }

    /// C# `VerifyGroupDestination` (lines 435-439): `destination` is `None` or its length differs from `member.size`
    /// (compare without wrapping: `i64::try_from(length) != Ok(size)`) => the coded error `VERIFY_SIZE_CHANGED`.
    fn verify_group_destination(&self, member: &JournalGroupMember, destination: Option<&FileStat>) -> io::Result<()> {
        todo!("W3-04: port VerifyGroupDestination")
    }

    /// C# `VerifyGroupMove` (lines 441-445): `file_exists(source)` => coded `MOVE_SOURCE_NOT_REMOVED`; else
    /// [`Self::verify_group_destination`] on a fresh `get_file_stat(destination)`.
    fn verify_group_move(&self, member: &JournalGroupMember) -> io::Result<()> {
        todo!("W3-04: port VerifyGroupMove")
    }

    /// C# `InspectGroupMember` (lines 447-477): the post-failure state of one member. `error` is the ORIGINAL failure's
    /// message. A stat error (P1 seam) => `completed: false, conflict: true, error: Some(err.to_string()),
    /// state_known: false` (source/destination_exists false).
    fn inspect_group_member(
        &self,
        operation: FileOperationType,
        member: &JournalGroupMember,
        error: &str,
    ) -> CaptureGroupMemberResult {
        todo!("W3-04: port InspectGroupMember")
    }

    /// C# `RemovePartialCopy` (lines 739-740): `partial_cleanup::remove_if_partial(fs, Path::new(destination),
    /// source_size, created)` (W3-08). Only called with a raised creation proof and an observed `created` stat.
    fn remove_partial_copy(&self, destination: &str, source_size: i64, created: &FileStat) {
        todo!("W3-04: port RemovePartialCopy")
    }

    /// C# `IsUntouched` (lines 744-756, RV-C02): the source stat is `Some` with `length == size` and
    /// `last_write_utc == last_write_utc` (EXACT ticks), and `destination` is `None` or `!file_exists(destination)`. A
    /// stat error (P1 seam) => `false`.
    fn is_untouched(&self, source: &str, size: i64, last_write_utc: DateTimeUtc, destination: Option<&str>) -> bool {
        todo!("W3-04: port IsUntouched")
    }

    /// C# `IsSourceGone` (lines 759-769): `!file_exists(source)` (the Rust `file_exists` cannot fail).
    fn is_source_gone(&self, source: &str) -> bool {
        todo!("W3-04: port IsSourceGone")
    }

    /// The "stat that can fail" seam (contract open question P1, shared with W3-07). C# `GetFileStat` returns `null` for
    /// a missing file and THROWS when the path cannot be inspected. Option A (recommended):
    /// `self.fs.try_get_file_stat(Path::new(path))`; option B (until the lead decides): `Ok(self.fs.get_file_stat(..))`.
    fn stat(&self, path: &str) -> io::Result<Option<FileStat>> {
        todo!("W3-04: P1 seam")
    }

    /// `new JournalCodedException(code)`: `errors::JournalCodedError::new(code, message).into_io_error()` with
    /// `message = errors::text(code, &|key| self.journal.texts().localized(key))` (UI language; `code` is always one of
    /// the `errors` constants, so `text` cannot fail: fall back to the code itself, never `unwrap`).
    /// `JournalTransaction::fail` later recognizes it (`errors::for_journal`) and stores the code + ENGLISH text.
    fn coded_error(&self, code: &'static str) -> io::Error {
        todo!("W3-04: JournalCodedException(code)")
    }
}

/// C# `IsSamePath` (lines 771-776): both sides through `get_full_path` then `trim_ending_directory_separator`, compared
/// OrdinalIgnoreCase (`crate::text::eq_ignore_case`). The inputs are already full paths here, so the `Err` arm is never
/// reached in practice; it is returned, not swallowed (C# would throw into the action's catch).
fn is_same_path(first: &str, second: &str) -> Result<bool, PathError> {
    todo!("W3-04: port IsSamePath")
}

/// .NET `Path.Combine(first, second)` (two arguments, Windows): `second` rooted (`dotnet_path::is_path_rooted`) =>
/// `second`; `first` empty => `second`; `second` empty => `first`; else `first + second` when `first` ends with `'\\'`
/// or `'/'` (NOT `':'`: `"C:" + "x"` is `"C:\x"`), otherwise `first + "\\" + second`. No normalization of any kind.
/// Candidate for `text::dotnet_path` (open question 5).
fn combine(first: &str, second: &str) -> String {
    todo!("W3-04: Path.Combine")
}

/// True when `err` is a cancellation: an `io::Error` wrapping `crate::sync::Canceled` (look through `get_ref()`, the
/// same rule as `errors::for_journal`). C# `ex is OperationCanceledException`.
fn is_canceled(err: &io::Error) -> bool {
    todo!("W3-04: ex is OperationCanceledException")
}

/// One failure of an action: the C# `catch (Exception ex)` value, split in two because Rust cannot have both halves in
/// one `io::Error` (contract trap T5):
///
/// * `error` goes to `JournalTransaction::fail` / `is_canceled`. A cancellation MUST be exactly
///   `io::Error::other(Canceled)` (or a fake's error wrapping `Canceled`): `errors::for_journal` recognizes ONLY a
///   `Canceled` directly inside the `io::Error`, and stores `CancelledByUser`. A wrapper type carrying its own text
///   would be journaled as an uncoded failure.
/// * `message` is C# `ex.Message`, used for the result's `error`, the member states and the rollback note:
///   `error.to_string()` for every non-cancellation; for a cancellation the .NET text of the place that raised it
///   ([`OPERATION_CANCELED_MESSAGE`] for `cancel.check()` and for a `Canceled` returned by a fake / the override,
///   [`TASK_CANCELED_MESSAGE`] for the `Task.Run(.., ct)` hand-off check). `Canceled`'s own Display
///   ("operation canceled") never reaches a result.
///
/// Private: the porter may reshape it, but must keep the two halves apart.
#[derive(Debug)]
struct ActionFailure {
    error: io::Error,
    message: String,
}

impl ActionFailure {
    /// The `await Task.Run(mutation, ct)` refusal: `io::Error::other(Canceled)` + [`TASK_CANCELED_MESSAGE`].
    fn hand_off_canceled() -> Self {
        todo!("W3-04: TaskCanceledException")
    }
}

impl From<io::Error> for ActionFailure {
    /// `message` = [`OPERATION_CANCELED_MESSAGE`] when [`is_canceled`], else `error.to_string()`.
    fn from(error: io::Error) -> Self {
        todo!("W3-04: ex.Message")
    }
}

impl From<crate::sync::Canceled> for ActionFailure {
    /// `cancel.check()?` (C# `ThrowIfCancellationRequested()`): `io::Error::other(Canceled)` +
    /// [`OPERATION_CANCELED_MESSAGE`].
    fn from(canceled: crate::sync::Canceled) -> Self {
        todo!("W3-04: OperationCanceledException")
    }
}

/// A `get_full_path` failure as the C# exception it stands for, turned into the action's failure: kind `InvalidInput`
/// for [`PathError::Argument`] (`ArgumentException`), `Other` for [`PathError::TooLong`] (`PathTooLongException`).
/// The message is the .NET `ex.Message`, NOT `PathError`'s Display: for `Argument` the empty-path text
/// `"The path is empty. (Parameter 'path')"` (.NET `SR.Arg_PathEmpty` + the parameter suffix; the only `Argument` case
/// this unit can reach is a source without a folder, `GetFullPath("")`), pinned by the golden case
/// `single-move-source-without-folder` (contract trap T7; if the golden shows another text, the golden wins). `TooLong`
/// carries no path, so the .NET text `"The path '{path}' is too long, or a component of the specified path is too
/// long."` cannot be rebuilt here: pass the path in or record a `Different` row (no golden case reaches it).
fn path_error_to_io(err: PathError) -> io::Error {
    todo!("W3-04: GetFullPath exception text")
}

// ---------------------------------------------------------------------------------------------------------------
// The one INV-4 gate, as the other Wave 3 services see it (complete: they only forward to the inherent methods)
// ---------------------------------------------------------------------------------------------------------------

impl FileActionGate for FileActionService {
    fn try_begin(&self) -> bool {
        FileActionService::try_begin(self)
    }
    fn end(&self) {
        FileActionService::end(self)
    }
    fn is_busy(&self) -> bool {
        FileActionService::is_busy(self)
    }
}
