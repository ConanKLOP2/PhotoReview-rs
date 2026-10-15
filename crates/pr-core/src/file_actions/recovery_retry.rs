//! Port of `PhotoReview.Core/FileActions/RecoveryRetryService.cs` (C# baseline d92d6aae; PARITY F-18, INV-4, INV-6).
//! Contract: `contracts/W3-06.md`.
//!
//! Re-runs a Failed (or Prepared) journal entry from the Recovery window: a single Move/Copy, or a capture group
//! (Move/Copy members, a Delete = Recycle group, or the undo of a Delete group). Every retry:
//!
//! * takes the app's file-action gate like any other file action (INV-4, RV-C08) and refuses with
//!   [`RecoveryRetryTexts::core_recovery_busy`] while it is held, before anything is read;
//! * refuses an entry the user cancelled (`error_code == CancelledByUser`) unless the caller passes
//!   `confirmed_finish_cancelled`;
//! * re-checks the files against the journaled fingerprint before journaling anything, then journals `Prepared` under
//!   the SAME Id, mutates, verifies and journals `Committed` or `Failed` through a retry-mode [`JournalTransaction`]
//!   (P02: the Prepared/Failed appends are conditional on nobody else having written that Id since the snapshot;
//!   otherwise the result is `superseded` and nothing is appended);
//! * never overwrites or deletes a destination (create-new copy, RV-C03 / R01a), never deletes permanently unless the
//!   member was journaled Permanent AND the setting is on now AND the drive still has no bin (Q-R8), and checks the
//!   Recycle Bin capacity of the members still pending (F-WIN-2).
//!
//! Threading (R0003): C# `RetryMoveOrCopyAsync` hops to the thread pool for the mutation (CORE-06). Here the call is
//! blocking; the caller (Recovery presenter, App wave W8) runs it on the file-action lane, never on the UI thread.
//! The C# `await Task.Run(..., ct)` hand-off becomes ONE explicit `cancel.check()?` at the same point (see
//! [`RecoveryRetryService::retry_move_or_copy`]).
//!
//! The order and count of file-system calls is the C# one (contract "Semantics"): one stat per C# `GetFileStat`, no
//! caching across steps.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;

use crate::abstractions::{Clock, FileStat, FileSystem, RecycleBin};
use crate::file_actions::errors;
use crate::file_actions::journal::OperationJournal;
use crate::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use crate::file_actions::recovery_check::{
    RecoveryFileCheck, RecoveryGroupMemberCheck, RecoveryPathStatus, RecoveryVerdict,
};
use crate::file_actions::transaction::JournalTransaction;
use crate::file_actions::undo::FileActionGate;
use crate::model::{FileOperationType, JournalState};
use crate::sync::{CancelToken, Canceled};
use crate::text::dotnet_path::{get_directory_name, get_file_name};

/// Releases the file-action gate taken by [`RecoveryRetryService::retry_move_or_copy`] on every exit (C# `finally`),
/// `Err` and panic included.
struct GateGuard<'a>(&'a dyn FileActionGate);

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        self.0.end();
    }
}

fn is_move_or_copy(op_type: FileOperationType) -> bool {
    matches!(op_type, FileOperationType::Move | FileOperationType::Copy)
}

/// A stat length as the journal's `i64`: `None` above `i64::MAX` (never matches a journaled size).
fn length_as_i64(stat: &FileStat) -> Option<i64> {
    i64::try_from(stat.length).ok()
}

/// .NET's `OperationCanceledException.Message` (`SR.OperationCanceled`). C# reports `ex.Message` when a group retry is
/// cancelled mid-loop; [`Canceled`]'s own `Display` is different ("operation canceled"), so a failure that carries a
/// `Canceled` is reported with this text in [`RecoveryRetryResult::message`] (the journal gets the coded
/// `CancelledByUser` English text either way, through `errors::for_journal`).
pub const OPERATION_CANCELED_MESSAGE: &str = "The operation was canceled.";

/// C# `Func<bool>? allowPermanentDelete`: the CURRENT value of the "permanent delete without Recycle Bin" setting.
pub type AllowPermanentDelete = Arc<dyn Fn() -> bool + Send + Sync>;

/// C# `RecoveryRetryResult` (positional record with three defaulted members).
///
/// * `superseded` (P02): the retried entry was already handled by another PhotoReview window (another process sharing
///   the journal appended a newer record for its Id, or is executing it right now). Nothing was mutated or journaled by
///   this retry; the caller drops/refreshes its snapshot (like `DismissOutcome::skipped`).
/// * `journal_persisted == false`: the outcome line could not be appended; `journal_error` holds that append's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryRetryResult {
    pub succeeded: bool,
    pub message: String,
    pub entry: Option<JournalEntry>,
    /// C# default `true`.
    pub journal_persisted: bool,
    /// C# default `null`.
    pub journal_error: Option<String>,
    /// C# default `false`.
    pub superseded: bool,
}

impl RecoveryRetryResult {
    /// C# `new(succeeded, message, entry)`: the three defaulted members keep their C# defaults
    /// (`journal_persisted: true`, `journal_error: None`, `superseded: false`).
    pub fn new(succeeded: bool, message: impl Into<String>, entry: Option<JournalEntry>) -> Self {
        Self {
            succeeded,
            message: message.into(),
            entry,
            journal_persisted: true,
            journal_error: None,
            superseded: false,
        }
    }

    /// C# `new(succeeded, message, entry, JournalPersisted: false, JournalError: error)`.
    pub fn not_journaled(
        succeeded: bool,
        message: impl Into<String>,
        entry: Option<JournalEntry>,
        journal_error: impl Into<String>,
    ) -> Self {
        Self {
            journal_persisted: false,
            journal_error: Some(journal_error.into()),
            ..Self::new(succeeded, message, entry)
        }
    }
}

/// The localized strings the retry needs. C# reads them from the ambient `Tr.*` (`Localizer.Current`); `pr-core`
/// cannot see `pr-l10n`, so the App composition root passes an implementation over the real catalogs (same pattern as
/// `settings::app_settings::SettingsTexts`, D13). [`EnglishRecoveryRetryTexts`] is the built-in English catalog.
///
/// The message of a coded error (`JournalCodedException(code)`) is NOT here: it comes from the journal's own
/// `JournalTexts::localized` (W3-01), exactly like the coded errors the transaction raises.
pub trait RecoveryRetryTexts: Send + Sync {
    /// `Tr.CoreRecoveryBusy` (`core.recovery.busy`).
    fn core_recovery_busy(&self) -> String;
    /// `Tr.CoreRecoveryCancelledNeedsConfirm` (`core.recovery.cancelledNeedsConfirm`).
    fn core_recovery_cancelled_needs_confirm(&self) -> String;
    /// `Tr.CoreRecoveryOnlyMoveCopy` (`core.recovery.onlyMoveCopy`).
    fn core_recovery_only_move_copy(&self) -> String;
    /// `Tr.CoreRecoveryNoDestination` (`core.recovery.noDestination`).
    fn core_recovery_no_destination(&self) -> String;
    /// `Tr.CoreRecoverySourceMissing` (`core.recovery.sourceMissing`).
    fn core_recovery_source_missing(&self) -> String;
    /// `Tr.CoreRecoverySourceChanged` (`core.recovery.sourceChanged`).
    fn core_recovery_source_changed(&self) -> String;
    /// `Tr.CoreRecoveryDestinationExists` (`core.recovery.destinationExists`).
    fn core_recovery_destination_exists(&self) -> String;
    /// `Tr.CoreRecoveryConflict` (`core.recovery.conflict`).
    fn core_recovery_conflict(&self) -> String;
    /// `Tr.CoreRecoveryDestinationChanged` (`core.recovery.destinationChanged`).
    fn core_recovery_destination_changed(&self) -> String;
    /// `Tr.CoreRecoveryAlreadyHandled` (`core.recovery.alreadyHandled`).
    fn core_recovery_already_handled(&self) -> String;
    /// `Tr.CoreRecoverySucceeded` (`core.recovery.succeeded`).
    fn core_recovery_succeeded(&self) -> String;
    /// `Tr.CoreRecoverySucceededJournalFailed` (`core.recovery.succeededJournalFailed`).
    fn core_recovery_succeeded_journal_failed(&self) -> String;
    /// `Tr.CoreRecoveryCompletedFailureNotJournaled` (`core.recovery.completedFailureNotJournaled`).
    fn core_recovery_completed_failure_not_journaled(&self) -> String;
    /// `Tr.CoreRecycleUnsupportedDrive(name)` (`core.recycle.unsupportedDrive`, placeholder `{name}`).
    fn core_recycle_unsupported_drive(&self, name: &str) -> String;
    /// `Tr.CoreRecycleBinCannotHold(name)` (`core.recycle.binCannotHold`, placeholder `{name}`).
    fn core_recycle_bin_cannot_hold(&self, name: &str) -> String;
    /// `Tr.CoreUndoRecycleTargetExists(fileName)` (`core.undo.recycleTargetExists`, placeholder `{fileName}`).
    fn core_undo_recycle_target_exists(&self, file_name: &str) -> String;
    /// `Tr.CoreUndoRecycleRestoreFailed(fileName)` (`core.undo.recycleRestoreFailed`, placeholder `{fileName}`).
    fn core_undo_recycle_restore_failed(&self, file_name: &str) -> String;
}

/// The built-in English catalog (`assets/languages/en.json` at d92d6aae, checked by the lead). Complete: no porter
/// work; the test porter adds one test that compares every text with the asset (key in each trait doc line).
#[derive(Debug, Default, Clone, Copy)]
pub struct EnglishRecoveryRetryTexts;

impl RecoveryRetryTexts for EnglishRecoveryRetryTexts {
    fn core_recovery_busy(&self) -> String {
        "A file action is still running. Wait for it to finish, then retry.".to_owned()
    }
    fn core_recovery_cancelled_needs_confirm(&self) -> String {
        "You cancelled this operation earlier. Retrying would finish it, so it needs an explicit confirmation in the \
         Recovery window. Nothing was changed."
            .to_owned()
    }
    fn core_recovery_only_move_copy(&self) -> String {
        "Only Move/Copy can be retried; Recycle Bin operations are not retried automatically.".to_owned()
    }
    fn core_recovery_no_destination(&self) -> String {
        "The operation has no destination.".to_owned()
    }
    fn core_recovery_source_missing(&self) -> String {
        "The source no longer exists.".to_owned()
    }
    fn core_recovery_source_changed(&self) -> String {
        "The source has changed; retry refused to protect your data.".to_owned()
    }
    fn core_recovery_destination_exists(&self) -> String {
        "The destination already exists; it will not be overwritten.".to_owned()
    }
    fn core_recovery_conflict(&self) -> String {
        "Files exist at both the source and the destination; retry refused so that nothing is overwritten. Resolve \
         the duplicate by hand."
            .to_owned()
    }
    fn core_recovery_destination_changed(&self) -> String {
        "The file at the destination is no longer the one that was moved; retry refused to protect your data."
            .to_owned()
    }
    fn core_recovery_already_handled(&self) -> String {
        "This entry was already handled by another PhotoReview window, or is being retried there right now. Nothing \
         was changed; the entry has been removed from this list."
            .to_owned()
    }
    fn core_recovery_succeeded(&self) -> String {
        "Retry succeeded.".to_owned()
    }
    fn core_recovery_succeeded_journal_failed(&self) -> String {
        "Retry finished, but the journal could not be written.".to_owned()
    }
    fn core_recovery_completed_failure_not_journaled(&self) -> String {
        "The operation finished, but its failure could not be written to the journal.".to_owned()
    }
    fn core_recycle_unsupported_drive(&self, name: &str) -> String {
        format!(
            "\"{name}\" is on a removable or network drive that has no Recycle Bin, so it was NOT deleted (it would \
             be removed permanently). Delete it in Explorer if you really want it gone."
        )
    }
    fn core_recycle_bin_cannot_hold(&self, name: &str) -> String {
        format!(
            "\"{name}\" was NOT deleted: the Recycle Bin on this drive is turned off, is smaller than this file, or \
             its settings could not be read, so Windows would delete the file permanently. Check the drive's \
             settings in Recycle Bin > Properties, or delete the file in Explorer if you really want it gone."
        )
    }
    fn core_undo_recycle_target_exists(&self, file_name: &str) -> String {
        format!("A file named {file_name} already exists at the original location; restoring would replace it.")
    }
    fn core_undo_recycle_restore_failed(&self, file_name: &str) -> String {
        format!("Could not restore from the Recycle Bin: {file_name}")
    }
}

/// C# `RecoveryRetryService`. Cheap to share behind an `Arc`; every method takes `&self` (C#: no mutable state).
///
/// The C# constructor null checks (`ArgumentNullException` for journal / file system / clock) have no Rust twin: the
/// parameters are not optional. The three optional C# parameters are the `with_*` builders (absent = C# `null`).
pub struct RecoveryRetryService {
    journal: Arc<OperationJournal>,
    file_system: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    /// C# `IRecycleBin? recycleBin = null`: without it a Delete-group retry is refused (`only_move_copy` text).
    recycle_bin: Option<Arc<dyn RecycleBin>>,
    /// C# `Func<bool>? allowPermanentDelete = null`: read at retry time; `None` = not allowed (Q-R8).
    allow_permanent_delete: Option<AllowPermanentDelete>,
    /// C# `FileActionService? fileActionGate = null` (only `TryBegin`/`End` are used): `None` = no gate (tests).
    file_action_gate: Option<Arc<dyn FileActionGate>>,
    texts: Arc<dyn RecoveryRetryTexts>,
}

impl fmt::Debug for RecoveryRetryService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryRetryService")
            .field("has_recycle_bin", &self.recycle_bin.is_some())
            .field("has_allow_permanent_delete", &self.allow_permanent_delete.is_some())
            .field("has_file_action_gate", &self.file_action_gate.is_some())
            .finish_non_exhaustive()
    }
}

impl RecoveryRetryService {
    /// C# `new RecoveryRetryService(journal, fileSystem, clock)`, plus the UI texts the C# code takes from `Tr`
    /// (tests pass `Arc::new(EnglishRecoveryRetryTexts)`). `clock` stamps the Prepared/Committed/Failed lines (it is
    /// the clock handed to the transaction, NOT the journal's own clock).
    pub fn new(
        journal: Arc<OperationJournal>,
        file_system: Arc<dyn FileSystem>,
        clock: Arc<dyn Clock>,
        texts: Arc<dyn RecoveryRetryTexts>,
    ) -> Self {
        Self {
            journal,
            file_system,
            clock,
            recycle_bin: None,
            allow_permanent_delete: None,
            file_action_gate: None,
            texts,
        }
    }

    /// C# `recycleBin` argument.
    pub fn with_recycle_bin(mut self, recycle_bin: Arc<dyn RecycleBin>) -> Self {
        self.recycle_bin = Some(recycle_bin);
        self
    }

    /// C# `allowPermanentDelete` argument: called at retry time, exactly once per Delete-group (non-undo) retry that
    /// reaches the permanent-delete rule, BEFORE looking for a Permanent member (C# `?.Invoke()` is the third operand
    /// of an `&&` chain; contract semantics G6). Never called for Move/Copy groups, undo groups or single entries.
    pub fn with_allow_permanent_delete(mut self, allow_permanent_delete: AllowPermanentDelete) -> Self {
        self.allow_permanent_delete = Some(allow_permanent_delete);
        self
    }

    /// C# `fileActionGate` argument (RV-C08): the app's INV-4 gate (W3-04 implements [`FileActionGate`] for
    /// `FileActionService`).
    pub fn with_file_action_gate(mut self, gate: Arc<dyn FileActionGate>) -> Self {
        self.file_action_gate = Some(gate);
        self
    }

    /// C# `RetryMoveOrCopyAsync(failed, confirmedFinishCancelled, ct)`. Blocking (R0003).
    ///
    /// Order (contract semantics E): gate (`try_begin`; refused => busy result, NOTHING read or journaled, the gate
    /// is NOT released because this call does not own it); then [`Self::retry_core`]; the gate taken here is released
    /// on every path (RAII guard calling `end`, also on `Err` and on panic).
    ///
    /// `Err(Canceled)` reproduces the C# `TaskCanceledException` that escapes `await Task.Run(..., ct)` when `cancel`
    /// is already cancelled at the hand-off: AFTER every pre-check passed and BEFORE the live-marker check / Prepared
    /// append (single) or the transaction (group). A refusal decided earlier is returned as `Ok` even when cancelled.
    /// A cancellation noticed inside the group loop is NOT an `Err`: it is a journaled failure (`CancelledByUser`).
    pub fn retry_move_or_copy(
        &self,
        failed: &JournalEntry,
        confirmed_finish_cancelled: bool,
        cancel: &CancelToken,
    ) -> Result<RecoveryRetryResult, Canceled> {
        // RV-C08 (INV-4): refused before anything is read or journaled; the gate is not ours, so it is not released.
        let _guard = match &self.file_action_gate {
            Some(gate) => {
                if !gate.try_begin() {
                    return Ok(RecoveryRetryResult::new(false, self.texts.core_recovery_busy(), None));
                }
                Some(GateGuard(gate.as_ref()))
            }
            None => None,
        };
        self.retry_core(failed, confirmed_finish_cancelled, cancel)
    }

    /// C# `RetryCoreAsync`: cancelled-needs-confirm, group routing (`group_members` is `Some` AND non-empty), single
    /// Move/Copy validation and pre-checks, `prepared`, then the hand-off check and [`Self::execute_retry`].
    fn retry_core(
        &self,
        failed: &JournalEntry,
        confirmed_finish_cancelled: bool,
        cancel: &CancelToken,
    ) -> Result<RecoveryRetryResult, Canceled> {
        let texts = &*self.texts;
        if !confirmed_finish_cancelled && failed.error_code.as_deref() == Some(errors::CANCELLED_BY_USER) {
            return Ok(RecoveryRetryResult::new(
                false,
                texts.core_recovery_cancelled_needs_confirm(),
                None,
            ));
        }
        if failed.group_members.as_ref().is_some_and(|members| !members.is_empty()) {
            return self.retry_group(failed, cancel);
        }
        if !is_move_or_copy(failed.op_type) {
            return Ok(RecoveryRetryResult::new(
                false,
                texts.core_recovery_only_move_copy(),
                None,
            ));
        }
        let destination = match failed.destination.as_deref() {
            Some(destination) if !is_null_or_white_space(Some(destination)) => destination,
            _ => {
                return Ok(RecoveryRetryResult::new(
                    false,
                    texts.core_recovery_no_destination(),
                    None,
                ));
            }
        };
        let source_stat = match self.precheck_single(failed, destination) {
            Ok(Ok(stat)) => stat,
            Ok(Err(message)) => return Ok(RecoveryRetryResult::new(false, message, None)),
            // C# catch filter around the pre-checks: a path the file system rejects is a failed retry, not a crash.
            Err(err) => return Ok(RecoveryRetryResult::new(false, err.to_string(), None)),
        };

        // Keeps Undo / Permanent / the group fields: dropping Undo turns a retried undo-Move into a redo after a restart.
        let prepared = JournalEntry {
            state: JournalState::Prepared,
            error: None,
            error_code: None,
            size: length_as_i64(&source_stat).unwrap_or(i64::MAX),
            last_write_utc: JournalDateTime::utc(source_stat.last_write_utc),
            timestamp_utc: JournalDateTime::utc(self.clock.utc_now()),
            ..failed.clone()
        };

        // The C# `await Task.Run(..., ct)` hand-off.
        cancel.check()?;
        Ok(self.execute_retry(failed, prepared))
    }

    /// The single pre-check block of `RetryCoreAsync` (inside its `try`): `Ok(Ok(stat))` = the source stat to journal;
    /// `Ok(Err(message))` = refusal (source missing / changed, destination exists); `Err(e)` = the stat seam failed
    /// (C# catch filter: the result message is `e.to_string()`).
    fn precheck_single(&self, failed: &JournalEntry, destination: &str) -> io::Result<Result<FileStat, String>> {
        let texts = &*self.texts;
        let fs = &*self.file_system;
        if !fs.file_exists(Path::new(&failed.source)) {
            return Ok(Err(texts.core_recovery_source_missing()));
        }
        let stat = match self.stat(&failed.source)? {
            Some(stat) if same_fingerprint(&stat, failed.size, &failed.last_write_utc) => stat,
            _ => return Ok(Err(texts.core_recovery_source_changed())),
        };
        if fs.file_exists(Path::new(destination)) {
            return Ok(Err(texts.core_recovery_destination_exists()));
        }
        Ok(Ok(stat))
    }

    /// C# `RetryGroupAsync`: type check, live check, `RecoveryFileCheck`, pending members, refusals, per-member
    /// pre-check, capacity, `prepared`, hand-off check, [`Self::execute_group_retry`].
    fn retry_group(&self, failed: &JournalEntry, cancel: &CancelToken) -> Result<RecoveryRetryResult, Canceled> {
        let texts = &*self.texts;
        let is_recycle = failed.op_type == FileOperationType::Recycle;
        let is_undo = failed.undo == Some(true);
        if !is_move_or_copy(failed.op_type) && !is_recycle {
            return Ok(RecoveryRetryResult::new(
                false,
                texts.core_recovery_only_move_copy(),
                None,
            ));
        }
        if self.journal.live_operations().is_live(&failed.id) {
            return Ok(self.already_handled());
        }

        let check = RecoveryFileCheck::new(Arc::clone(&self.file_system)).check(failed);
        let members = match (check.verdict, check.group_members.as_deref()) {
            (RecoveryVerdict::CanRetry, Some(members)) => members,
            (verdict, _) => {
                return Ok(match verdict {
                    RecoveryVerdict::AlreadyDone => self.already_handled(),
                    RecoveryVerdict::Conflict => RecoveryRetryResult::new(false, texts.core_recovery_conflict(), None),
                    RecoveryVerdict::DestinationChanged => {
                        RecoveryRetryResult::new(false, texts.core_recovery_destination_changed(), None)
                    }
                    _ => RecoveryRetryResult::new(false, texts.core_recovery_source_changed(), None),
                });
            }
        };

        let pending = select_pending(failed, members);
        if pending.is_empty() {
            return Ok(self.already_handled());
        }
        let recycle_bin = self.recycle_bin.as_deref();
        if is_recycle && recycle_bin.is_none() {
            return Ok(RecoveryRetryResult::new(
                false,
                texts.core_recovery_only_move_copy(),
                None,
            ));
        }
        // Same rule as the first run (FileActionService): permanent deletion needs the setting NOW; refused before
        // anything is mutated. The setting is read once, before looking for a Permanent member.
        if is_recycle && !is_undo && !self.allow_permanent_delete.as_ref().is_some_and(|allow| allow()) {
            if let Some(item) = pending.iter().find(|item| item.member.permanent) {
                let name = get_file_name(&item.member.source);
                return Ok(RecoveryRetryResult::new(
                    false,
                    texts.core_recycle_unsupported_drive(name),
                    None,
                ));
            }
        }
        match self.precheck_group_members(failed, &pending) {
            Ok(None) => {}
            Ok(Some(message)) => return Ok(RecoveryRetryResult::new(false, message, None)),
            Err(err) => return Ok(RecoveryRetryResult::new(false, err.to_string(), None)),
        }

        // F-WIN-2: a volume's bin must hold its pending members TOGETHER, not just each one alone.
        if let (true, false, Some(bin)) = (is_recycle, is_undo, recycle_bin) {
            let members: Vec<JournalGroupMember> = pending.iter().map(|item| item.member.clone()).collect();
            if let Some(overflow) = errors::first_overflow(bin, &members) {
                let name = get_file_name(&overflow.source);
                return Ok(RecoveryRetryResult::new(
                    false,
                    texts.core_recycle_bin_cannot_hold(name),
                    None,
                ));
            }
        }

        // Size and last write are kept: the group fingerprint is per member.
        let prepared = JournalEntry {
            state: JournalState::Prepared,
            error: None,
            error_code: None,
            timestamp_utc: JournalDateTime::utc(self.clock.utc_now()),
            ..failed.clone()
        };

        // The C# `await Task.Run(..., ct)` hand-off.
        cancel.check()?;
        Ok(self.execute_group_retry(failed, prepared, &pending, cancel))
    }

    /// The per-member pre-check loop of `RetryGroupAsync` (inside its `try`): `Ok(None)` = every pending member still
    /// matches; `Ok(Some(message))` = refusal (always `core_recovery_source_changed`); `Err(e)` = the stat seam failed
    /// (result message `e.to_string()`). Stops at the first refusal or error.
    fn precheck_group_members(
        &self,
        failed: &JournalEntry,
        pending: &[RecoveryGroupMemberCheck],
    ) -> io::Result<Option<String>> {
        let source_changed = || Ok(Some(self.texts.core_recovery_source_changed()));
        let fs = &*self.file_system;
        let is_recycle = failed.op_type == FileOperationType::Recycle;
        let is_undo = failed.undo == Some(true);
        for item in pending {
            let member = &item.member;
            if is_recycle && is_undo {
                // No file-system call for an undo member.
                if member.permanent {
                    return source_changed();
                }
            } else if is_recycle {
                let Some(bin) = self.recycle_bin.as_deref() else {
                    return source_changed(); // unreachable: refused by the caller
                };
                let source = Path::new(&member.source);
                let unchanged = self
                    .stat(&member.source)?
                    .is_some_and(|stat| same_fingerprint(&stat, member.size, &member.last_write_utc));
                // Permanent but the drive has a bin now: never delete permanently. Not permanent but no bin: the
                // shell would delete it permanently.
                if !unchanged
                    || member.permanent && bin.can_recycle(source)
                    || !member.permanent && !bin.can_recycle(source)
                {
                    return source_changed();
                }
            } else {
                let unchanged = self
                    .stat(&member.source)?
                    .is_some_and(|stat| same_fingerprint(&stat, member.size, &member.last_write_utc));
                if !unchanged {
                    return source_changed();
                }
                match member.destination.as_deref() {
                    Some(destination)
                        if !is_null_or_white_space(Some(destination)) && !fs.file_exists(Path::new(destination)) => {}
                    _ => return source_changed(),
                }
            }
        }
        Ok(None)
    }

    /// C# `ExecuteGroupRetry`: one retry-mode transaction around the per-member mutations; maps the outcome.
    fn execute_group_retry(
        &self,
        failed: &JournalEntry,
        prepared: JournalEntry,
        pending: &[RecoveryGroupMemberCheck],
        cancel: &CancelToken,
    ) -> RecoveryRetryResult {
        let mut tx = JournalTransaction::new(&self.journal, &*self.clock, prepared, Some(failed.clone()));
        match self.run_group_members(&mut tx, failed, pending, cancel) {
            Ok(false) => self.already_handled(),
            Ok(true) => self.committed_result(&mut tx),
            Err(err) => {
                let (entry, fail_error) = tx.fail(&err);
                if tx.superseded() {
                    return self.already_handled();
                }
                match fail_error {
                    // A retry never undoes the members it already completed.
                    None => RecoveryRetryResult::new(tx.mutation_completed(), Self::failure_message(&err), entry),
                    Some(fail_error) => RecoveryRetryResult::not_journaled(
                        tx.mutation_completed(),
                        self.texts.core_recovery_completed_failure_not_journaled(),
                        entry,
                        fail_error,
                    ),
                }
            }
        }
    }

    /// The body of the C# group `try` block (`TryBegin`, members loop, `MarkMutationCompleted`), as a `Result` so that
    /// every failure reaches the single catch arm in [`Self::execute_group_retry`]. `Ok(false)` = `TryBegin` said
    /// superseded. A `Canceled` is carried as `io::Error::other(Canceled)` so that `errors::for_journal` codes it
    /// `CancelledByUser`.
    fn run_group_members(
        &self,
        tx: &mut JournalTransaction<'_>,
        failed: &JournalEntry,
        pending: &[RecoveryGroupMemberCheck],
        cancel: &CancelToken,
    ) -> io::Result<bool> {
        if !tx.try_begin()? {
            return Ok(false);
        }
        let texts = &*self.texts;
        let fs = &*self.file_system;
        let is_recycle = failed.op_type == FileOperationType::Recycle;
        let is_undo = failed.undo == Some(true);
        for item in pending {
            if cancel.check().is_err() {
                return Err(io::Error::other(Canceled));
            }
            let member = &item.member;
            let source = Path::new(&member.source);
            let name = get_file_name(&member.source);
            if is_recycle {
                let Some(bin) = self.recycle_bin.as_deref() else {
                    // Unreachable: refused before the transaction.
                    return Err(Self::plain_error(texts.core_recovery_only_move_copy()));
                };
                if is_undo {
                    // R15: this member was Missing at the pre-check (it is in the bin). Whatever sits at its path now
                    // is not the recycled file: never count it as restored, never ask the shell to replace it.
                    if fs.file_exists(source) {
                        return Err(Self::plain_error(texts.core_undo_recycle_target_exists(name)));
                    }
                    if member.permanent {
                        return Err(Self::plain_error(texts.core_recovery_source_changed()));
                    }
                    // DIVERGENCE: a negative journaled size (hand-edited journal) cannot be passed to the bin (`u64`);
                    // it counts as "restore failed" without asking the bin (contract open question 4).
                    let restored = match u64::try_from(member.size) {
                        Ok(size) => bin.try_restore(source, size, member.last_write_utc.value)?,
                        Err(_) => false,
                    };
                    if !restored || !self.is_restored_file(member)? {
                        return Err(Self::plain_error(texts.core_undo_recycle_restore_failed(name)));
                    }
                } else if member.permanent {
                    // Q-R8: only on a drive that still has no bin.
                    if bin.can_recycle(source) {
                        return Err(Self::plain_error(texts.core_recovery_source_changed()));
                    }
                    bin.delete_permanently(source)?;
                } else {
                    bin.send_to_recycle_bin(source)?;
                }
                if !is_undo && fs.file_exists(source) {
                    return Err(self.coded_error(errors::SOURCE_STILL_EXISTS_AFTER_RECOVERY));
                }
                if is_undo && !self.is_restored_file(member)? {
                    return Err(Self::plain_error(texts.core_undo_recycle_restore_failed(name)));
                }
            } else {
                let unchanged = self
                    .stat(&member.source)?
                    .is_some_and(|stat| same_fingerprint(&stat, member.size, &member.last_write_utc));
                if !unchanged {
                    return Err(Self::plain_error(texts.core_recovery_source_changed()));
                }
                let destination = match member.destination.as_deref() {
                    Some(destination)
                        if !is_null_or_white_space(Some(destination)) && !fs.file_exists(Path::new(destination)) =>
                    {
                        destination
                    }
                    _ => return Err(Self::plain_error(texts.core_recovery_destination_exists())),
                };
                // IsNullOrWhiteSpace here (IsNullOrEmpty on the single path).
                if let Some(folder) = get_directory_name(destination) {
                    if !is_null_or_white_space(Some(&folder)) {
                        fs.create_directory(Path::new(&folder))?;
                    }
                }
                if failed.op_type == FileOperationType::Copy {
                    self.copy_new_or_keep_leftover(&member.source, destination)?;
                    let copied = self.stat(destination)?;
                    if copied.as_ref().and_then(length_as_i64) != Some(member.size) {
                        return Err(self.coded_error(errors::RETRY_VERIFY_FAILED));
                    }
                } else {
                    fs.move_file(source, Path::new(destination))?;
                    // NOT `verify_moved`: the group path reports RetryVerifyFailed for both checks.
                    if fs.file_exists(source)
                        || self.stat(destination)?.as_ref().and_then(length_as_i64) != Some(member.size)
                    {
                        return Err(self.coded_error(errors::RETRY_VERIFY_FAILED));
                    }
                }
            }
        }
        tx.mark_mutation_completed();
        Ok(true)
    }

    /// C# `ExecuteRetry` (single Move/Copy): live check, transaction, outcome mapping.
    fn execute_retry(&self, failed: &JournalEntry, prepared: JournalEntry) -> RecoveryRetryResult {
        // P02 / Q-R27: another process is executing this Id right now.
        if self.journal.live_operations().is_live(&failed.id) {
            return self.already_handled();
        }
        let destination = failed.destination.as_deref().unwrap_or_default(); // validated non-blank by the caller
        let mut tx = JournalTransaction::new(&self.journal, &*self.clock, prepared, Some(failed.clone()));
        match self.run_single(&mut tx, failed, destination) {
            Ok(false) => self.already_handled(),
            Ok(true) => self.committed_result(&mut tx),
            Err(err) => {
                let (entry, fail_error) = tx.fail(&err);
                if tx.superseded() {
                    return self.already_handled();
                }
                match fail_error {
                    // A journaled failure is reported as a failure, even when the mutation completed.
                    None => RecoveryRetryResult::new(false, Self::failure_message(&err), entry),
                    Some(fail_error) => {
                        let completed = tx.mutation_completed();
                        let message = if completed {
                            self.texts.core_recovery_completed_failure_not_journaled()
                        } else {
                            Self::failure_message(&err)
                        };
                        RecoveryRetryResult::not_journaled(completed, message, entry, fail_error)
                    }
                }
            }
        }
    }

    /// The body of the C# single `try` block: `TryBegin` (`Ok(false)` = superseded), R17 re-stat, create folder,
    /// mutate, verify.
    fn run_single(
        &self,
        tx: &mut JournalTransaction<'_>,
        failed: &JournalEntry,
        destination: &str,
    ) -> io::Result<bool> {
        if !tx.try_begin()? {
            return Ok(false);
        }
        let fs = &*self.file_system;
        let source = failed.source.as_str();
        // R17: a different file may have taken the source path since the pre-checks. `prepared` carries the stat the
        // pre-check read, so a replacement is never moved/copied under the old operation.
        let prepared = tx.prepared();
        let unchanged = self
            .stat(source)?
            .is_some_and(|stat| same_fingerprint(&stat, prepared.size, &prepared.last_write_utc));
        if !unchanged {
            return Err(Self::plain_error(self.texts.core_recovery_source_changed()));
        }
        // IsNullOrEmpty here: a whitespace folder IS created.
        if let Some(folder) = get_directory_name(destination) {
            if !folder.is_empty() {
                fs.create_directory(Path::new(&folder))?;
            }
        }
        if failed.op_type == FileOperationType::Copy {
            self.copy_new_or_keep_leftover(source, destination)?;
            tx.verify_destination(fs, destination, errors::RETRY_VERIFY_FAILED)?;
        } else {
            fs.move_file(Path::new(source), Path::new(destination))?;
            tx.verify_moved(fs, source, destination, errors::RETRY_VERIFY_FAILED)?;
        }
        Ok(true)
    }

    /// C# `CopyNewOrKeepLeftover` (RV-C03, R01a): `try_copy_new` returning `false` => an `io::Error` whose message is
    /// `core_recovery_destination_exists`; an `Err` from the copy propagates unchanged. NEVER deletes the destination.
    fn copy_new_or_keep_leftover(&self, source: &str, destination: &str) -> io::Result<()> {
        if self
            .file_system
            .try_copy_new(Path::new(source), Path::new(destination))?
        {
            Ok(())
        } else {
            Err(Self::plain_error(self.texts.core_recovery_destination_exists()))
        }
    }

    /// C# `IsRestoredFile` (R15/R18): a stat of `member.source` exists and matches the member's fingerprint
    /// ([`same_fingerprint`]). `Err` = the stat seam failed (C#: `GetFileStat` threw inside the loop => failure).
    fn is_restored_file(&self, member: &JournalGroupMember) -> io::Result<bool> {
        Ok(self
            .stat(&member.source)?
            .is_some_and(|stat| same_fingerprint(&stat, member.size, &member.last_write_utc)))
    }

    /// Every stat of this unit goes through here (C# `_fileSystem.GetFileStat(path)`, which may THROW). With the W3-07
    /// seam (decided: P-12, option A): `self.file_system.try_get_file_stat(Path::new(path))`. Exactly one file-system
    /// stat call per invocation.
    fn stat(&self, path: &str) -> io::Result<Option<FileStat>> {
        self.file_system.try_get_file_stat(Path::new(path))
    }

    /// C# `AlreadyHandled()`: `succeeded: false`, `core_recovery_already_handled`, no entry, `superseded: true`
    /// (`journal_persisted` keeps its default `true`).
    fn already_handled(&self) -> RecoveryRetryResult {
        RecoveryRetryResult {
            superseded: true,
            ..RecoveryRetryResult::new(false, self.texts.core_recovery_already_handled(), None)
        }
    }

    /// The success arm shared by the single and group paths (C# `tx.Commit(out commitError)` and its mapping).
    fn committed_result(&self, tx: &mut JournalTransaction<'_>) -> RecoveryRetryResult {
        let (committed, commit_error) = tx.commit();
        match commit_error {
            None => RecoveryRetryResult::new(true, self.texts.core_recovery_succeeded(), Some(committed)),
            Some(commit_error) => RecoveryRetryResult::not_journaled(
                true,
                self.texts.core_recovery_succeeded_journal_failed(),
                Some(committed),
                commit_error,
            ),
        }
    }

    /// `new JournalCodedException(code)`: `errors::JournalCodedError::new(code, errors::text(code, ui))` turned into an
    /// `io::Error` (`into_io_error`), where `ui` = `|key| self.journal.texts().localized(key)`. `code` is always one of
    /// the `errors` constants here, so `text` cannot fail (map the impossible `Err` to the code itself, no panic).
    fn coded_error(&self, code: &'static str) -> io::Error {
        let journal_texts = self.journal.texts();
        let message = errors::text(code, &|key| journal_texts.localized(key)).unwrap_or_else(|_| code.to_owned());
        errors::JournalCodedError::new(code, message).into_io_error()
    }

    /// C# `ex.Message` of a failure: [`OPERATION_CANCELED_MESSAGE`] when `err` carries a [`Canceled`] (look through
    /// `io::Error::get_ref`), otherwise `err.to_string()` (D5: free text).
    fn failure_message(err: &io::Error) -> String {
        if err
            .get_ref()
            .is_some_and(|inner| inner.downcast_ref::<Canceled>().is_some())
        {
            OPERATION_CANCELED_MESSAGE.to_owned()
        } else {
            err.to_string()
        }
    }

    /// `new IOException(message)`: `io::Error::other(message)` (Display = `message`, no code).
    fn plain_error(message: String) -> io::Error {
        io::Error::other(message)
    }
}

/// The members a group retry still has to process (C# `pending` in `RetryGroupAsync`), in member order:
///
/// * Move/Copy: members whose own verdict is `CanRetry`;
/// * Recycle with `undo == Some(true)` (undo of a Delete): members whose SOURCE status is `Missing` (restore them);
/// * Recycle otherwise (Delete): members whose verdict is `NotRecycled` (still on disk); those already recycled or
///   deleted permanently are left alone.
///
/// Pure (no I/O): `members` is `RecoveryCheckResult::group_members` of the `RecoveryFileCheck::check` result.
pub fn select_pending(failed: &JournalEntry, members: &[RecoveryGroupMemberCheck]) -> Vec<RecoveryGroupMemberCheck> {
    let is_recycle = failed.op_type == FileOperationType::Recycle;
    let is_undo = failed.undo == Some(true);
    members
        .iter()
        .filter(|member| {
            if !is_recycle {
                member.verdict() == RecoveryVerdict::CanRetry
            } else if is_undo {
                member.source().status == RecoveryPathStatus::Missing
            } else {
                member.verdict() == RecoveryVerdict::NotRecycled
            }
        })
        .cloned()
        .collect()
}

/// The journaled fingerprint still matches a stat: `length == size` (as `i64`; a `u64` length above `i64::MAX`
/// never matches) AND `last_write_utc == recorded.value` (ticks only: the `unspecified` flag is ignored, like .NET
/// `DateTime ==`, which ignores `Kind`).
pub fn same_fingerprint(stat: &FileStat, size: i64, recorded: &JournalDateTime) -> bool {
    length_as_i64(stat) == Some(size) && stat.last_write_utc == recorded.value
}

/// C# `string.IsNullOrWhiteSpace(value)` (`char::is_whitespace` = .NET `char.IsWhiteSpace` for every BMP char;
/// the W3-07 contract uses the same rule).
pub fn is_null_or_white_space(value: Option<&str>) -> bool {
    value.is_none_or(|value| value.chars().all(char::is_whitespace))
}
