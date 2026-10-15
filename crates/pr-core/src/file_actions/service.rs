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

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::abstractions::{Clock, CopyCreationProof, FileStat, FileSystem, RecycleBin};
use crate::file_actions::destination_policy::{self, ActionDestinationCheck};
use crate::file_actions::errors::{self, JournalCodedError};
use crate::file_actions::journal::OperationJournal;
use crate::file_actions::journal_line::{JournalDateTime, JournalEntry};
use crate::file_actions::partial_cleanup;
use crate::file_actions::transaction::JournalTransaction;
use crate::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, CaptureGroupMemberResult, FileActionRequest, FileActionResult,
    JournalGroupMember, matches_moved_destination,
};
use crate::file_actions::undo::{FileActionGate, MoveOverride};
use crate::io::physical::new_guid_n;
use crate::model::{FileOperationType, JournalState};
use crate::sync::{CancelToken, Canceled};
use crate::text::dotnet_path::{
    PathError, get_directory_name, get_file_name, get_full_path, is_path_rooted, trim_ending_directory_separator,
};
use crate::text::{eq_ignore_case, ignore_case_key};
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
        self.in_progress.load(Ordering::SeqCst) != 0
    }

    /// C# `TryBegin()` (INV-4): compare-exchange 0 -> 1 (`SeqCst`); `true` when THIS call took the gate.
    pub fn try_begin(&self) -> bool {
        self.in_progress
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// C# `End()`: unconditional `SeqCst` store of 0 (also when the gate was not held).
    pub fn end(&self) {
        self.in_progress.store(0, Ordering::SeqCst);
    }

    /// C# `LacksRecycleBin(path)` (Q-R8): `!recycle_bin.can_recycle(path)`, i.e. a Recycle of `path` would be a
    /// PERMANENT delete. Callers (App) use it to decide whether the user must confirm.
    pub fn lacks_recycle_bin(&self, path: &str) -> bool {
        !self.recycle_bin.can_recycle(Path::new(path))
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
    ///
    /// A panic inside a `FileSystem`/`RecycleBin` implementation is NOT caught (contract open question 2, option B): it
    /// propagates, the guard still releases the gate and the transaction's `Drop` releases the live marker, so the
    /// Prepared line stays for startup reconcile (the C# "unexpected exception" arm has the same disk/journal outcome).
    pub fn execute_group(&self, request: &CaptureGroupActionRequest, cancel: &CancelToken) -> CaptureGroupActionResult {
        let group_id = new_guid_n();
        if !self.try_begin() {
            return CaptureGroupActionResult::new(
                false,
                true,
                request.operation,
                group_id,
                None,
                Vec::new(),
                Some(self.texts.core_file_action_busy()),
            );
        }
        let gate = GateGuard(self);
        let mut run = GroupRun::new();
        let result = match self.run_group(request, cancel, &group_id, &mut run) {
            Ok(result) => result,
            Err(failure) => self.group_failure(request, &group_id, &mut run, failure),
        };
        // C# `finally { tx?.Dispose(); End(); }`: the transaction first, then the gate.
        drop(run);
        drop(gate);
        result
    }

    /// C# `ExecuteAsync(request, ct)` (F-01..F-05). Never returns an error: every outcome is a [`FileActionResult`]
    /// (contract "Semantics", single section, steps S1-S8).
    ///
    /// Gate refused => `rejected: true`, `succeeded: false`, `destination_path: None`, `size: 0`,
    /// `last_write_utc: DateTimeUtc::MIN`, `error: core_file_action_busy` (no id is generated, nothing is read).
    pub fn execute(&self, request: &FileActionRequest, cancel: &CancelToken) -> FileActionResult {
        if !self.try_begin() {
            return FileActionResult {
                rejected: true,
                ..FileActionResult::new(
                    false,
                    request.operation,
                    request.source.clone(),
                    None,
                    0,
                    DateTimeUtc::MIN,
                    Some(self.texts.core_file_action_busy()),
                )
            };
        }
        let gate = GateGuard(self);
        let mut run = SingleRun::new(new_guid_n());
        let result = match self.run_single(request, cancel, &mut run) {
            Ok(result) => result,
            Err(failure) => self.single_failure(request, &mut run, failure),
        };
        // C# `finally { tx?.Dispose(); End(); }`: the transaction first, then the gate.
        drop(run);
        drop(gate);
        result
    }

    // -----------------------------------------------------------------------------------------------------------
    // Single action (C# `ExecuteAsync` [482-732])
    // -----------------------------------------------------------------------------------------------------------

    /// The C# `try` block of `ExecuteAsync`: S3-S5. Every `Err` goes to [`Self::single_failure`].
    fn run_single<'a>(
        &'a self,
        request: &FileActionRequest,
        cancel: &CancelToken,
        run: &mut SingleRun<'a>,
    ) -> Result<FileActionResult, ActionFailure> {
        let source = request.source.as_str();
        match request.operation {
            FileOperationType::Move | FileOperationType::Copy => {
                // S3.1
                let destination = match request.destination.as_deref() {
                    Some(destination) if !destination.trim().is_empty() => destination,
                    _ => return Err(io::Error::other(self.texts.core_file_action_no_destination()).into()),
                };
                // S3.2 (review r7): the whole ActionDestinationPolicy at run time.
                if destination_policy::validate(Some(destination)) != ActionDestinationCheck::Ok {
                    return Err(self.coded_error(errors::DESTINATION_OUTSIDE_SOURCE).into());
                }
                // S3.3: the destination folder first, then the source folder (trap T7).
                let rooted = is_path_rooted(destination);
                let source_directory = get_directory_name(source).unwrap_or_default();
                let destination_folder = full_path(&if rooted {
                    destination.to_owned()
                } else {
                    combine(&source_directory, destination)
                })?;
                let source_folder = full_path(&source_directory)?;
                // S3.4
                if is_same_path(&destination_folder, &source_folder).map_err(|err| path_error_to_io(err, source))? {
                    return Err(io::Error::other(self.texts.core_file_action_same_folder()).into());
                }
                // S3.5
                if !rooted && !is_inside_folder(&destination_folder, &source_folder) {
                    return Err(self.coded_error(errors::DESTINATION_OUTSIDE_SOURCE).into());
                }
                // S3.6
                let destination_path = combine(&destination_folder, get_file_name(source));
                run.destination_path = Some(destination_path.clone());
                // S3.7 (SEC-01): only a relative destination; any non-Ok answer fails closed.
                if !rooted
                    && !matches!(
                        destination_policy::validate_no_escape_via_reparse_point(
                            &source_folder,
                            &destination_path,
                            &*self.fs
                        ),
                        Ok(ActionDestinationCheck::Ok)
                    )
                {
                    return Err(self.coded_error(errors::DESTINATION_OUTSIDE_SOURCE).into());
                }
                // S3.8
                if self.fs.file_exists(Path::new(&destination_path)) {
                    return Err(
                        io::Error::other(self.texts.core_file_action_destination_exists(&destination_path)).into(),
                    );
                }
                // S3.9
                let Some(source_stat) = self.fs.try_get_file_stat(Path::new(source))? else {
                    return Err(self.source_missing(source).into());
                };
                run.source_size = size_of(&source_stat);
                run.source_last_write = source_stat.last_write_utc;
                // S3.10: only after every pre-check passed (a missing source never leaves an empty folder).
                run.created_folders = self.create_destination_folder(&destination_folder)?;

                // S4
                let prepared = self.prepared_entry(
                    &run.operation_id,
                    request.operation,
                    source,
                    Some(destination_path.clone()),
                    run.source_size,
                    run.source_last_write,
                    None,
                );
                let tx = run
                    .tx
                    .insert(JournalTransaction::new(&self.journal, &*self.clock, prepared, None));
                tx.begin()?;

                let source_path = Path::new(source);
                let target = Path::new(&destination_path);
                if request.operation == FileOperationType::Copy {
                    hand_off(cancel)?;
                    // Create-new copy (RV-C03): a file that appeared after the preflight is never overwritten or claimed.
                    if !self.fs.try_copy_new_with_proof(source_path, target, &run.proof)? {
                        return Err(io::Error::other(
                            self.texts.core_file_action_destination_exists(&destination_path),
                        )
                        .into());
                    }
                    run.copied_stat = self.fs.try_get_file_stat(target)?;
                    tx.verify_destination_stat(run.copied_stat, errors::VERIFY_SIZE_CHANGED)?;
                } else {
                    match &self.move_override {
                        // C# awaits the override directly, with no token: no hand-off check.
                        Some(move_override) => move_override(source_path, target)?,
                        None => {
                            hand_off(cancel)?;
                            self.fs.move_file(source_path, target)?;
                        }
                    }
                    tx.verify_moved(&*self.fs, source, &destination_path, errors::VERIFY_SIZE_CHANGED)?;
                }

                let (_, journal_error) = tx.commit();
                Ok(FileActionResult {
                    journal_persisted: journal_error.is_none(),
                    journal_error,
                    ..FileActionResult::new(
                        true,
                        request.operation,
                        source,
                        Some(destination_path),
                        run.source_size,
                        run.source_last_write,
                        None,
                    )
                })
            }
            FileOperationType::Recycle => {
                // S5
                let Some(source_stat) = self.fs.try_get_file_stat(Path::new(source))? else {
                    return Err(self.source_missing(source).into());
                };
                run.source_size = size_of(&source_stat);
                run.source_last_write = source_stat.last_write_utc;

                // Q-R8: no Recycle Bin => permanent delete, only when the caller opted in.
                run.permanent = !self.recycle_bin.can_recycle(Path::new(source));
                if run.permanent && !request.allow_permanent_delete {
                    return Err(
                        io::Error::other(self.texts.core_recycle_unsupported_drive(get_file_name(source))).into(),
                    );
                }
                // F-WIN-2: a bin that cannot hold the file would delete it permanently; not asked for a permanent delete.
                if !run.permanent
                    && !self
                        .recycle_bin
                        .fits_in_recycle_bin(Path::new(source), source_stat.length)
                {
                    return Err(
                        io::Error::other(self.texts.core_recycle_bin_cannot_hold(get_file_name(source))).into(),
                    );
                }

                let prepared = self.prepared_entry(
                    &run.operation_id,
                    FileOperationType::Recycle,
                    source,
                    None,
                    run.source_size,
                    run.source_last_write,
                    run.permanent.then_some(true),
                );
                let tx = run
                    .tx
                    .insert(JournalTransaction::new(&self.journal, &*self.clock, prepared, None));
                tx.begin()?;

                hand_off(cancel)?;
                if run.permanent {
                    self.recycle_bin.delete_permanently(Path::new(source))?;
                } else {
                    self.recycle_bin.send_to_recycle_bin(Path::new(source))?;
                }
                // B-06: a shell that reports success while the source is still there is not a recycled file.
                if self.fs.file_exists(Path::new(source)) {
                    return Err(self.coded_error(errors::SOURCE_STILL_EXISTS_AFTER_RECOVERY).into());
                }
                tx.mark_mutation_completed();

                let (_, journal_error) = tx.commit();
                Ok(FileActionResult {
                    journal_persisted: journal_error.is_none(),
                    journal_error,
                    permanently_deleted: run.permanent,
                    ..FileActionResult::new(
                        true,
                        FileOperationType::Recycle,
                        source,
                        None,
                        run.source_size,
                        run.source_last_write,
                        None,
                    )
                })
            }
        }
    }

    /// The C# `catch (Exception ex)` of `ExecuteAsync` (S7).
    fn single_failure(
        &self,
        request: &FileActionRequest,
        run: &mut SingleRun<'_>,
        failure: ActionFailure,
    ) -> FileActionResult {
        let prepared = run.tx.as_ref().is_some_and(JournalTransaction::is_prepared);
        // S7.1: failed before the Prepared line exists: the empty folders this call created go again.
        if !prepared {
            self.remove_created_empty_folders(&run.created_folders);
        }
        // S7.2 (RV-C03, R01a/b): only a destination this Copy provably created and observed.
        if run.proof.destination_created() {
            if let (Some(destination), Some(created)) = (run.destination_path.as_deref(), run.copied_stat.as_ref()) {
                self.remove_partial_copy(destination, run.source_size, created);
            }
        }
        // S7.3
        let mutation_completed = run.tx.as_ref().is_some_and(JournalTransaction::mutation_completed);
        // S7.4 (RV-C02): cancelled while the disk is untouched => Dismissed; otherwise Failed.
        let mut journal_error = None;
        if let Some(tx) = run.tx.as_mut() {
            let dismiss = is_canceled(&failure.error)
                && tx.is_prepared()
                && !mutation_completed
                && self.is_untouched(
                    &request.source,
                    run.source_size,
                    run.source_last_write,
                    run.destination_path.as_deref(),
                );
            journal_error = if dismiss {
                tx.dismiss_rolled_back().1
            } else {
                tx.fail(&failure.error).1
            };
        }
        // S7.5 (F3, INV-3): only a journaled Move that then failed can have removed the source; ask the disk.
        let source_removed = !mutation_completed
            && request.operation == FileOperationType::Move
            && prepared
            && self.is_source_gone(&request.source);

        FileActionResult {
            journal_persisted: journal_error.is_none(),
            journal_error,
            permanently_deleted: run.permanent && mutation_completed,
            source_removed,
            ..FileActionResult::new(
                mutation_completed,
                request.operation,
                request.source.clone(),
                run.destination_path.clone(),
                run.source_size,
                run.source_last_write,
                (!mutation_completed).then_some(failure.message),
            )
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Capture group (C# `ExecuteGroupAsync` [61-283])
    // -----------------------------------------------------------------------------------------------------------

    /// The C# `try` block of `ExecuteGroupAsync`: G2-G7. Every `Err` goes to [`Self::group_failure`].
    fn run_group<'a>(
        &'a self,
        request: &CaptureGroupActionRequest,
        cancel: &CancelToken,
        group_id: &str,
        run: &mut GroupRun<'a>,
    ) -> Result<CaptureGroupActionResult, ActionFailure> {
        let operation = request.operation;
        let paths = request.group.paths();
        // G2
        let Some(first_path) = paths.first().filter(|_| paths.len() >= 2) else {
            return Err(io::Error::other(self.texts.core_file_action_group_needs_two_paths()).into());
        };

        let mut destination_folder: Option<String> = None;
        let mut first_source_folder = String::new();
        let mut rooted = false;
        if matches!(operation, FileOperationType::Move | FileOperationType::Copy) {
            let destination = match request.destination.as_deref() {
                Some(destination) if !destination.trim().is_empty() => destination,
                _ => return Err(io::Error::other(self.texts.core_file_action_no_destination()).into()),
            };
            if destination_policy::validate(Some(destination)) != ActionDestinationCheck::Ok {
                return Err(self.coded_error(errors::DESTINATION_OUTSIDE_SOURCE).into());
            }
            rooted = is_path_rooted(destination);
            // The first source folder BEFORE the destination folder (trap T7), combined with the FULL first folder.
            first_source_folder = full_path(&get_directory_name(first_path).unwrap_or_default())?;
            let folder = full_path(&if rooted {
                destination.to_owned()
            } else {
                combine(&first_source_folder, destination)
            })?;
            if is_same_path(&folder, &first_source_folder).map_err(|err| path_error_to_io(err, first_path))? {
                return Err(io::Error::other(self.texts.core_file_action_same_folder()).into());
            }
            if !rooted && !is_inside_folder(&folder, &first_source_folder) {
                return Err(self.coded_error(errors::DESTINATION_OUTSIDE_SOURCE).into());
            }
            destination_folder = Some(folder);
        }

        // G3: preflight every path, in group order.
        let image_paths = request.group.image_paths();
        let mut members: Vec<JournalGroupMember> = Vec::with_capacity(paths.len());
        for source in paths {
            cancel.check()?;
            // A stale group: a partner vanished outside the app. Skipped and reported. A stat that FAILS is not a missing
            // partner: it fails the whole group before any journal line (C# `GetFileStat` throws; P-12).
            let Some(stat) = self.fs.try_get_file_stat(Path::new(source))? else {
                run.missing.push(source.clone());
                continue;
            };
            let mut destination = None;
            if let Some(folder) = &destination_folder {
                let source_folder = full_path(&get_directory_name(source).unwrap_or_default())?;
                if !rooted && !eq_ignore_case(&source_folder, &first_source_folder) {
                    return Err(self.coded_error(errors::DESTINATION_OUTSIDE_SOURCE).into());
                }
                let target = combine(folder, get_file_name(source));
                // NOTE: "exists" BEFORE the reparse check here (the single path does it the other way round).
                if self.fs.file_exists(Path::new(&target)) {
                    return Err(io::Error::other(self.texts.core_file_action_destination_exists(&target)).into());
                }
                if !rooted
                    && !matches!(
                        destination_policy::validate_no_escape_via_reparse_point(&source_folder, &target, &*self.fs),
                        Ok(ActionDestinationCheck::Ok)
                    )
                {
                    return Err(self.coded_error(errors::DESTINATION_OUTSIDE_SOURCE).into());
                }
                destination = Some(target);
            }

            let mut permanent = false;
            if operation == FileOperationType::Recycle {
                permanent = !self.recycle_bin.can_recycle(Path::new(source));
                if permanent && !request.allow_permanent_delete {
                    return Err(
                        io::Error::other(self.texts.core_recycle_unsupported_drive(get_file_name(source))).into(),
                    );
                }
            }

            members.push(JournalGroupMember {
                source: source.clone(),
                destination,
                size: size_of(&stat),
                last_write_utc: JournalDateTime::utc(stat.last_write_utc),
                permanent,
            });
        }

        // G4: only a sidecar left (or nothing): the photo itself is gone.
        let is_image = |path: &str| image_paths.iter().any(|image| eq_ignore_case(image, path));
        if !members.iter().any(|member| is_image(&member.source)) {
            let gone = run
                .missing
                .iter()
                .find(|path| is_image(path))
                .unwrap_or(first_path)
                .clone();
            return Err(self.source_missing(&gone).into());
        }

        // G5 (F-WIN-2): the bin of each volume must hold all of that volume's members together.
        if operation == FileOperationType::Recycle {
            if let Some(overflow) = errors::first_overflow(&*self.recycle_bin, &members) {
                return Err(
                    io::Error::other(self.texts.core_recycle_bin_cannot_hold(get_file_name(&overflow.source))).into(),
                );
            }
        }
        run.manifest = members;

        // G6
        if let Some(folder) = &destination_folder {
            run.created_folders = self.create_destination_folder(folder)?;
        }
        let Some(first_member) = run.manifest.first() else {
            // Unreachable: G4 guarantees at least one member.
            return Err(self.source_missing(first_path).into());
        };
        let all_permanent = run.manifest.iter().all(|member| member.permanent);
        let any_permanent = run.manifest.iter().any(|member| member.permanent);
        let prepared = JournalEntry {
            id: new_guid_n(),
            op_type: operation,
            state: JournalState::Prepared,
            source: first_member.source.clone(),
            destination: first_member.destination.clone(),
            size: first_member.size,
            last_write_utc: first_member.last_write_utc,
            timestamp_utc: JournalDateTime::utc(self.clock.utc_now()),
            error: None,
            error_code: None,
            permanent: (all_permanent && any_permanent).then_some(true),
            undo: None,
            group_id: Some(group_id.to_owned()),
            group_members: Some(run.manifest.clone()),
        };
        let tx = run
            .tx
            .insert(JournalTransaction::new(&self.journal, &*self.clock, prepared, None));
        tx.begin()?;

        // G7: members in manifest order.
        for member in &run.manifest {
            cancel.check()?;
            run.in_flight = Some(member.clone());
            run.proof = CopyCreationProof::new();
            let source = Path::new(&member.source);
            let destination = member.destination.as_deref().unwrap_or_default();
            match operation {
                FileOperationType::Copy => {
                    hand_off(cancel)?;
                    if !self
                        .fs
                        .try_copy_new_with_proof(source, Path::new(destination), &run.proof)?
                    {
                        return Err(
                            io::Error::other(self.texts.core_file_action_destination_exists(destination)).into(),
                        );
                    }
                    let copied = self.fs.try_get_file_stat(Path::new(destination))?;
                    if let Some(copied) = copied {
                        run.created_copies.insert(ignore_case_key(destination), copied);
                    }
                    self.verify_group_destination(member, copied.as_ref())?;
                }
                FileOperationType::Move => {
                    match &self.move_override {
                        // C# awaits the override directly, with no token: no hand-off check.
                        Some(move_override) => move_override(source, Path::new(destination))?,
                        None => {
                            hand_off(cancel)?;
                            self.fs.move_file(source, Path::new(destination))?;
                        }
                    }
                    self.verify_group_move(member)?;
                }
                FileOperationType::Recycle => {
                    hand_off(cancel)?;
                    if member.permanent {
                        self.recycle_bin.delete_permanently(source)?;
                    } else {
                        self.recycle_bin.send_to_recycle_bin(source)?;
                    }
                    if self.fs.file_exists(source) {
                        return Err(self.coded_error(errors::SOURCE_STILL_EXISTS_AFTER_RECOVERY).into());
                    }
                }
            }
            run.done.push(member.clone());
            run.in_flight = None;
        }

        tx.mark_mutation_completed();
        let (committed, journal_error) = tx.commit();
        let complete = run
            .manifest
            .iter()
            .map(|member| CaptureGroupMemberResult::new(member.clone(), true, false))
            .collect();
        Ok(CaptureGroupActionResult {
            journal_persisted: journal_error.is_none(),
            journal_error,
            permanently_deleted: operation == FileOperationType::Recycle && any_permanent,
            skipped_missing: (!run.missing.is_empty()).then(|| run.missing.clone()),
            ..CaptureGroupActionResult::new(
                true,
                false,
                operation,
                group_id,
                Some(Box::new(committed)),
                complete,
                None,
            )
        })
    }

    /// The C# `catch (Exception ex)` of `ExecuteGroupAsync` (G8). The "unexpected exception type" arm (G9) has no
    /// twin: see [`Self::execute_group`].
    fn group_failure(
        &self,
        request: &CaptureGroupActionRequest,
        group_id: &str,
        run: &mut GroupRun<'_>,
        failure: ActionFailure,
    ) -> CaptureGroupActionResult {
        let operation = request.operation;
        let prepared = run.tx.as_ref().is_some_and(JournalTransaction::is_prepared);
        // G8.1
        if !prepared {
            self.remove_created_empty_folders(&run.created_folders);
        }
        // G8.2: compensate first (the outcome decides what the journal says). Recycle is never compensated.
        let mut stuck = 0;
        if prepared {
            match operation {
                FileOperationType::Copy => {
                    stuck = self.remove_created_copies(
                        &run.done,
                        run.in_flight.as_ref(),
                        run.proof.destination_created(),
                        &run.created_copies,
                    );
                }
                FileOperationType::Move => stuck = self.restore_moved_members(&run.done, run.in_flight.as_ref()),
                FileOperationType::Recycle => {}
            }
        }
        // G8.3
        let states: Vec<CaptureGroupMemberResult> = run
            .manifest
            .iter()
            .map(|member| self.inspect_group_member(operation, member, &failure.message))
            .collect();
        // G8.4: only the user message carries the rollback note; the journal keeps the ORIGINAL failure.
        let message = if stuck > 0 {
            self.texts.core_group_action_rollback_failed(&failure.message, stuck)
        } else {
            failure.message.clone()
        };
        // G8.5: fully rolled back = every member provably back in its original state.
        let rolled_back = stuck == 0
            && !states.is_empty()
            && prepared
            && matches!(operation, FileOperationType::Move | FileOperationType::Copy)
            && states.iter().all(|state| {
                state.state_known
                    && !state.completed
                    && !state.conflict
                    && state.source_exists
                    && !state.destination_exists
            });
        let (entry, journal_error) = match run.tx.as_mut() {
            Some(tx) if rolled_back => tx.dismiss_rolled_back(),
            Some(tx) => tx.fail(&failure.error),
            None => (None, None),
        };
        let permanently_deleted = operation == FileOperationType::Recycle
            && states.iter().any(|state| state.completed && state.member.permanent);
        CaptureGroupActionResult {
            journal_persisted: journal_error.is_none(),
            journal_error,
            permanently_deleted,
            skipped_missing: (!run.missing.is_empty()).then(|| run.missing.clone()),
            ..CaptureGroupActionResult::new(
                false,
                false,
                operation,
                group_id,
                entry.map(Box::new),
                states,
                Some(message),
            )
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Private helpers (C# private methods)
    // -----------------------------------------------------------------------------------------------------------

    /// The Prepared line of a single action (S4/S5). TimestampUtc from the SERVICE clock (trap T11).
    #[allow(clippy::too_many_arguments)] // one argument per JournalEntry field the single action sets
    fn prepared_entry(
        &self,
        id: &str,
        operation: FileOperationType,
        source: &str,
        destination: Option<String>,
        size: i64,
        last_write_utc: DateTimeUtc,
        permanent: Option<bool>,
    ) -> JournalEntry {
        JournalEntry {
            id: id.to_owned(),
            op_type: operation,
            state: JournalState::Prepared,
            source: source.to_owned(),
            destination,
            size,
            last_write_utc: JournalDateTime::utc(last_write_utc),
            timestamp_utc: JournalDateTime::utc(self.clock.utc_now()),
            error: None,
            error_code: None,
            permanent,
            undo: None,
            group_id: None,
            group_members: None,
        }
    }

    /// C# `new FileNotFoundException(Tr.CoreFileActionSourceMissing(path), path)`: kind `NotFound`.
    fn source_missing(&self, path: &str) -> io::Error {
        io::Error::new(
            io::ErrorKind::NotFound,
            self.texts.core_file_action_source_missing(path),
        )
    }

    /// C# `CreateDestinationFolder` (lines 290-302): walks up from `trim_ending_directory_separator(destination_folder)`
    /// while the text is non-empty and `!directory_exists(folder)`, collecting each missing folder (deepest first; the
    /// walk uses `dotnet_path::get_directory_name`, `None` ends it), THEN calls `create_directory(destination_folder)`
    /// (the untrimmed text). Returns the folders this call brought into existence. A `create_directory` error is returned
    /// (C# exception) and nothing is returned as created.
    fn create_destination_folder(&self, destination_folder: &str) -> io::Result<Vec<String>> {
        let mut missing = Vec::new();
        let mut folder = Some(trim_ending_directory_separator(destination_folder).to_owned());
        while let Some(current) = folder {
            if current.is_empty() || self.fs.directory_exists(Path::new(&current)) {
                break;
            }
            folder = get_directory_name(&current);
            missing.push(current);
        }
        self.fs.create_directory(Path::new(destination_folder))?;
        Ok(missing)
    }

    /// C# `RemoveCreatedEmptyFolders` (lines 308-321): for each folder in order, `try_delete_empty_directory`; stop at the
    /// first `false`. Never fails (the Rust trait method cannot fail; see the contract, trap T9).
    fn remove_created_empty_folders(&self, created_folders: &[String]) {
        for folder in created_folders {
            if !self.fs.try_delete_empty_directory(Path::new(folder)) {
                return;
            }
        }
    }

    /// C# `RemoveCreatedCopies` (lines 332-382): Copy compensation. `created_copies` is keyed by
    /// `crate::text::ignore_case_key(destination)` (C# `StringComparer.OrdinalIgnoreCase`). Returns the number of files
    /// that could not be cleaned up ("stuck"). A copy is deleted only while it is still exactly the file observed
    /// right after this call created it (R01b); sources and the bin are never touched.
    fn remove_created_copies(
        &self,
        done: &[JournalGroupMember],
        in_flight: Option<&JournalGroupMember>,
        in_flight_destination_is_ours: bool,
        created_copies: &HashMap<String, FileStat>,
    ) -> usize {
        let mut stuck = 0;
        for member in done.iter().rev() {
            // `Ok(true)` = stuck; any error = stuck (C# catch).
            let attempt = || -> io::Result<bool> {
                let destination = member_destination(member)?;
                let Some(stat) = self.stat(destination)? else {
                    return Ok(false);
                };
                let ours = created_copies
                    .get(&ignore_case_key(destination))
                    .is_some_and(|created| {
                        length_is(&stat, member.size) && partial_cleanup::is_same_file(&stat, created)
                    });
                if !ours {
                    return Ok(true);
                }
                self.delete_and_confirm(destination)
            };
            if attempt().unwrap_or(true) {
                stuck += 1;
            }
        }

        // The in-flight member only when the copy implementation proved it created the destination (R01); its size is
        // NOT compared (a short own copy is deleted).
        if let Some(partial) = in_flight.and_then(|member| member.destination.as_deref()) {
            if in_flight_destination_is_ours {
                let attempt = || -> io::Result<bool> {
                    let Some(now) = self.stat(partial)? else {
                        return Ok(false);
                    };
                    let ours = created_copies
                        .get(&ignore_case_key(partial))
                        .is_some_and(|created| partial_cleanup::is_same_file(&now, created));
                    if !ours {
                        return Ok(true); // not the file this operation created (or never observed): kept
                    }
                    self.delete_and_confirm(partial)
                };
                if attempt().unwrap_or(true) {
                    stuck += 1;
                }
            }
        }
        stuck
    }

    /// `delete(path)` then "is it still there?" (`Ok(true)` = stuck).
    fn delete_and_confirm(&self, path: &str) -> io::Result<bool> {
        self.fs.delete(Path::new(path))?;
        Ok(self.fs.file_exists(Path::new(path)))
    }

    /// C# `RestoreMovedMembers` (lines 394-433): Move compensation, with the plain `FileSystem::move_file` (never the
    /// override). Candidates: the in-flight member first (`was_done = false`), then `done` in REVERSE order. Returns stuck.
    /// The in-flight destination is never deleted (D-06).
    fn restore_moved_members(&self, done: &[JournalGroupMember], in_flight: Option<&JournalGroupMember>) -> usize {
        let candidates = in_flight
            .map(|member| (member, false))
            .into_iter()
            .chain(done.iter().rev().map(|member| (member, true)));
        let mut stuck = 0;
        for (member, was_done) in candidates {
            // `Ok(true)` = stuck; any error = stuck (C# catch).
            let attempt = || -> io::Result<bool> {
                let destination_path = member_destination(member)?;
                let source = self.stat(&member.source)?;
                let destination = self.stat(destination_path)?;
                let destination = match (source, destination) {
                    (Some(_), None) => return Ok(false), // already back
                    // Both present (a conflict) or neither (lost): only a finished member counts as stuck.
                    (Some(_), Some(_)) | (None, None) => return Ok(was_done),
                    (None, Some(destination)) => destination,
                };
                // RV-C01: tolerant of a destination volume that rounds the write time (FAT 2 s).
                if !matches_moved_destination(&destination, member.size, member.last_write_utc.value) {
                    return Ok(true);
                }
                self.fs
                    .move_file(Path::new(destination_path), Path::new(&member.source))?;
                let back = self.stat(&member.source)?;
                Ok(!back.is_some_and(|back| length_is(&back, member.size))
                    || self.fs.file_exists(Path::new(destination_path)))
            };
            if attempt().unwrap_or(true) {
                stuck += 1;
            }
        }
        stuck
    }

    /// C# `VerifyGroupDestination` (lines 435-439): `destination` is `None` or its length differs from `member.size`
    /// (compare without wrapping: `i64::try_from(length) != Ok(size)`) => the coded error `VERIFY_SIZE_CHANGED`.
    fn verify_group_destination(&self, member: &JournalGroupMember, destination: Option<&FileStat>) -> io::Result<()> {
        if destination.is_some_and(|stat| length_is(stat, member.size)) {
            Ok(())
        } else {
            Err(self.coded_error(errors::VERIFY_SIZE_CHANGED))
        }
    }

    /// C# `VerifyGroupMove` (lines 441-445): `file_exists(source)` => coded `MOVE_SOURCE_NOT_REMOVED`; else
    /// [`Self::verify_group_destination`] on a fresh `try_get_file_stat(destination)`;
    /// a stat that fails is the action's failure as-is (C# `GetFileStat` throws: no code, raw message; P-12).
    fn verify_group_move(&self, member: &JournalGroupMember) -> io::Result<()> {
        if self.fs.file_exists(Path::new(&member.source)) {
            return Err(self.coded_error(errors::MOVE_SOURCE_NOT_REMOVED));
        }
        let destination = member
            .destination
            .as_deref()
            .map(|destination| self.fs.try_get_file_stat(Path::new(destination)))
            .transpose()?
            .flatten();
        self.verify_group_destination(member, destination.as_ref())
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
        let inspect = || -> io::Result<CaptureGroupMemberResult> {
            if operation == FileOperationType::Recycle {
                let source = self.stat(&member.source)?;
                let conflict = source.is_some_and(|source| {
                    !length_is(&source, member.size) || source.last_write_utc != member.last_write_utc.value
                });
                return Ok(CaptureGroupMemberResult {
                    error: source.is_some().then(|| error.to_owned()),
                    source_exists: source.is_some(),
                    ..CaptureGroupMemberResult::new(member.clone(), source.is_none(), conflict)
                });
            }

            let destination = match member.destination.as_deref() {
                Some(destination) => self.stat(destination)?,
                None => None,
            };
            let source = self.stat(&member.source)?;
            let destination_complete = destination.is_some_and(|destination| length_is(&destination, member.size));
            let completed = if operation == FileOperationType::Move {
                source.is_none() && destination_complete
            } else {
                destination_complete
            };
            // A Move member put back made a round trip through the destination volume (RV-C01): tolerant compare.
            // A Copy source never left its volume: exact.
            let source_unchanged = source.is_some_and(|source| {
                if operation == FileOperationType::Move {
                    matches_moved_destination(&source, member.size, member.last_write_utc.value)
                } else {
                    length_is(&source, member.size) && source.last_write_utc == member.last_write_utc.value
                }
            });
            let conflict = !completed && (!source_unchanged || destination.is_some());
            Ok(CaptureGroupMemberResult {
                error: (!completed).then(|| error.to_owned()),
                source_exists: source.is_some(),
                destination_exists: destination.is_some(),
                ..CaptureGroupMemberResult::new(member.clone(), completed, conflict)
            })
        };
        inspect().unwrap_or_else(|err| CaptureGroupMemberResult {
            error: Some(err.to_string()),
            state_known: false,
            ..CaptureGroupMemberResult::new(member.clone(), false, true)
        })
    }

    /// C# `RemovePartialCopy` (lines 739-740): `partial_cleanup::remove_if_partial(fs, Path::new(destination),
    /// source_size, created)` (W3-08). Only called with a raised creation proof and an observed `created` stat.
    fn remove_partial_copy(&self, destination: &str, source_size: i64, created: &FileStat) {
        partial_cleanup::remove_if_partial(&*self.fs, Path::new(destination), source_size, created);
    }

    /// C# `IsUntouched` (lines 744-756, RV-C02): the source stat is `Some` with `length == size` and
    /// `last_write_utc == last_write_utc` (EXACT ticks), and `destination` is `None` or `!file_exists(destination)`. A
    /// stat error (P1 seam) => `false`.
    fn is_untouched(&self, source: &str, size: i64, last_write_utc: DateTimeUtc, destination: Option<&str>) -> bool {
        match self.stat(source) {
            Ok(Some(stat)) => {
                length_is(&stat, size)
                    && stat.last_write_utc == last_write_utc
                    && destination.is_none_or(|destination| !self.fs.file_exists(Path::new(destination)))
            }
            _ => false,
        }
    }

    /// C# `IsSourceGone` (lines 759-769): `!file_exists(source)` (the Rust `file_exists` cannot fail).
    fn is_source_gone(&self, source: &str) -> bool {
        !self.fs.file_exists(Path::new(source))
    }

    /// The "stat that can fail" seam (contract open question P1, shared with W3-07). C# `GetFileStat` returns `null` for
    /// a missing file and THROWS when the path cannot be inspected. Decided (P-12, option A):
    /// `self.fs.try_get_file_stat(Path::new(path))`.
    fn stat(&self, path: &str) -> io::Result<Option<FileStat>> {
        self.fs.try_get_file_stat(Path::new(path))
    }

    /// `new JournalCodedException(code)`: `errors::JournalCodedError::new(code, message).into_io_error()` with
    /// `message = errors::text(code, &|key| self.journal.texts().localized(key))` (UI language; `code` is always one of
    /// the `errors` constants, so `text` cannot fail: fall back to the code itself, never `unwrap`).
    /// `JournalTransaction::fail` later recognizes it (`errors::for_journal`) and stores the code + ENGLISH text.
    fn coded_error(&self, code: &'static str) -> io::Error {
        let texts = self.journal.texts();
        let message = errors::text(code, &|key| texts.localized(key)).unwrap_or_else(|_| code.to_owned());
        JournalCodedError::new(code, message).into_io_error()
    }
}

/// Releases the INV-4 gate when dropped (C# `finally { ... End(); }`), also on a panic. Only created AFTER this call took
/// the gate, so a refused call never releases a gate it does not hold (trap T1).
struct GateGuard<'a>(&'a FileActionService);

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        self.0.end();
    }
}

/// The local state of one `ExecuteAsync` call (the C# locals the `catch` block reads).
struct SingleRun<'a> {
    operation_id: String,
    tx: Option<JournalTransaction<'a>>,
    destination_path: Option<String>,
    source_size: i64,
    source_last_write: DateTimeUtc,
    permanent: bool,
    /// RV-C03 / R01: raised by the copy implementation itself when it created the destination.
    proof: CopyCreationProof,
    /// R01a/b: the destination right after this Copy created it (ONE stat, reused for the verify and the cleanup).
    copied_stat: Option<FileStat>,
    /// Folders THIS call created for the destination (deepest first).
    created_folders: Vec<String>,
}

impl SingleRun<'_> {
    fn new(operation_id: String) -> Self {
        Self {
            operation_id,
            tx: None,
            destination_path: None,
            source_size: 0,
            source_last_write: DateTimeUtc::MIN,
            permanent: false,
            proof: CopyCreationProof::new(),
            copied_stat: None,
            created_folders: Vec::new(),
        }
    }
}

/// The local state of one `ExecuteGroupAsync` call (the C# locals the `catch` block reads).
struct GroupRun<'a> {
    tx: Option<JournalTransaction<'a>>,
    /// Empty until the preflight and the capacity check passed (G5).
    manifest: Vec<JournalGroupMember>,
    missing: Vec<String>,
    /// Members whose operation finished and verified.
    done: Vec<JournalGroupMember>,
    /// The member being processed when the failure hit.
    in_flight: Option<JournalGroupMember>,
    /// Ownership of the in-flight Copy destination (R01): only the copy implementation's proof counts.
    proof: CopyCreationProof,
    /// R01b: destination key -> what it looked like right after this operation created it (lookup only, T10).
    created_copies: HashMap<String, FileStat>,
    created_folders: Vec<String>,
}

impl GroupRun<'_> {
    fn new() -> Self {
        Self {
            tx: None,
            manifest: Vec::new(),
            missing: Vec::new(),
            done: Vec::new(),
            in_flight: None,
            proof: CopyCreationProof::new(),
            created_copies: HashMap::new(),
            created_folders: Vec::new(),
        }
    }
}

/// The `await Task.Run(mutation, ct)` hand-off: a token already cancelled refuses to start the mutation.
fn hand_off(cancel: &CancelToken) -> Result<(), ActionFailure> {
    if cancel.is_canceled() {
        Err(ActionFailure::hand_off_canceled())
    } else {
        Ok(())
    }
}

/// `FileStat.Length` as the journal's `long` (trap T3: saturating, never `as`).
fn size_of(stat: &FileStat) -> i64 {
    i64::try_from(stat.length).unwrap_or(i64::MAX)
}

/// `stat.Length == size` without wrapping (a length above `i64::MAX` never matches).
fn length_is(stat: &FileStat, size: i64) -> bool {
    i64::try_from(stat.length) == Ok(size)
}

/// `member.Destination!`: a Move/Copy member always has one; a missing one is a failure (C# `ArgumentNullException`,
/// caught by the compensation as "stuck").
fn member_destination(member: &JournalGroupMember) -> io::Result<&str> {
    member
        .destination
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Value cannot be null. (Parameter 'path')"))
}

/// `Path.GetFullPath(path)` as the action's failure (C# exception message, trap T7).
fn full_path(path: &str) -> io::Result<String> {
    get_full_path(path).map_err(|err| path_error_to_io(err, path))
}

/// `destination_folder.StartsWith(source_folder.TrimEnd('\\') + "\\", OrdinalIgnoreCase)` (`TrimEnd` trims ALL trailing
/// backslashes; trap T4: no `to_lowercase`).
fn is_inside_folder(destination_folder: &str, source_folder: &str) -> bool {
    let prefix = format!("{}\\", source_folder.trim_end_matches('\\'));
    ignore_case_key(destination_folder).starts_with(&ignore_case_key(&prefix))
}

/// C# `IsSamePath` (lines 771-776): both sides through `get_full_path` then `trim_ending_directory_separator`, compared
/// OrdinalIgnoreCase (`crate::text::eq_ignore_case`). The inputs are already full paths here, so the `Err` arm is never
/// reached in practice; it is returned, not swallowed (C# would throw into the action's catch).
fn is_same_path(first: &str, second: &str) -> Result<bool, PathError> {
    let first = get_full_path(first)?;
    let second = get_full_path(second)?;
    Ok(eq_ignore_case(
        trim_ending_directory_separator(&first),
        trim_ending_directory_separator(&second),
    ))
}

/// .NET `Path.Combine(first, second)` (two arguments, Windows): `second` rooted (`dotnet_path::is_path_rooted`) =>
/// `second`; `first` empty => `second`; `second` empty => `first`; else `first + second` when `first` ends with `'\\'`
/// or `'/'` (NOT `':'`: `"C:" + "x"` is `"C:\x"`), otherwise `first + "\\" + second`. No normalization of any kind.
/// Candidate for `text::dotnet_path` (open question 5).
fn combine(first: &str, second: &str) -> String {
    if first.is_empty() {
        return second.to_owned();
    }
    if second.is_empty() {
        return first.to_owned();
    }
    if is_path_rooted(second) {
        return second.to_owned();
    }
    if first.ends_with(['\\', '/']) {
        format!("{first}{second}")
    } else {
        format!("{first}\\{second}")
    }
}

/// True when `err` is a cancellation: an `io::Error` wrapping `crate::sync::Canceled` (look through `get_ref()`, the
/// same rule as `errors::for_journal`). C# `ex is OperationCanceledException`.
fn is_canceled(err: &io::Error) -> bool {
    err.get_ref().is_some_and(|inner| inner.is::<Canceled>())
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
#[derive(Debug)]
struct ActionFailure {
    error: io::Error,
    message: String,
}

impl ActionFailure {
    /// The `await Task.Run(mutation, ct)` refusal: `io::Error::other(Canceled)` + [`TASK_CANCELED_MESSAGE`].
    fn hand_off_canceled() -> Self {
        Self {
            error: io::Error::other(Canceled),
            message: TASK_CANCELED_MESSAGE.to_owned(),
        }
    }
}

impl From<io::Error> for ActionFailure {
    /// `message` = [`OPERATION_CANCELED_MESSAGE`] when [`is_canceled`], else `error.to_string()`.
    fn from(error: io::Error) -> Self {
        let message = if is_canceled(&error) {
            OPERATION_CANCELED_MESSAGE.to_owned()
        } else {
            error.to_string()
        };
        Self { error, message }
    }
}

impl From<Canceled> for ActionFailure {
    /// `cancel.check()?` (C# `ThrowIfCancellationRequested()`): `io::Error::other(Canceled)` +
    /// [`OPERATION_CANCELED_MESSAGE`].
    fn from(canceled: Canceled) -> Self {
        Self {
            error: io::Error::other(canceled),
            message: OPERATION_CANCELED_MESSAGE.to_owned(),
        }
    }
}

/// A `get_full_path` failure of `path` as the C# exception it stands for, turned into the action's failure: kind
/// `InvalidInput` for [`PathError::Argument`] (`ArgumentException`), `Other` for [`PathError::TooLong`]
/// (`PathTooLongException`). The message is the .NET `ex.Message`, NOT `PathError`'s Display: `"The path is empty.
/// (Parameter 'path')"` for an empty / blank path (.NET `SR.Arg_PathEmpty`; the only `Argument` case this unit reaches
/// in practice is a source without a folder, `GetFullPath("")`, golden `single-move-source-without-folder`),
/// `"Null character in path. (Parameter 'path')"` for a NUL, and the .NET `SR.IO_PathTooLong_Path` text with the path.
fn path_error_to_io(err: PathError, path: &str) -> io::Error {
    match err {
        // A path holding a NUL is never empty or blank, so the .NET "empty" check (made first) cannot win here.
        PathError::Argument if path.contains('\0') => io::Error::new(
            io::ErrorKind::InvalidInput,
            "Null character in path. (Parameter 'path')",
        ),
        PathError::Argument => io::Error::new(io::ErrorKind::InvalidInput, "The path is empty. (Parameter 'path')"),
        PathError::TooLong => io::Error::other(format!(
            "The path '{path}' is too long, or a component of the specified path is too long."
        )),
    }
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
