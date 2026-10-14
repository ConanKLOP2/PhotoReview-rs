//! Port of `PhotoReview.Core/FileActions/UndoService.cs` (C# baseline d92d6aae, 693 lines). `UndoResult.cs` (15 lines)
//! is already ported as [`super::types::UndoResult`] (W1-09) and is only re-used here. Contract:
//! `contracts/W3-05.md` (PARITY F-10, INV-4 for Undo, INV-6 for the undo's own journal lines).
//!
//! One level of Undo for the current session (P03: nothing is reloaded from the journal at start):
//! * a single Move ([`UndoService::undo_move`]: a stack of `(source, destination)` plus a fingerprint map),
//! * a single Recycle (shell restore, proven by size AND write time at the original path, R18),
//! * a capture-group Move or Recycle (partial restore, retry, settling the Failed line of a part-way group Delete).
//!
//! Every undo that mutates files is itself journaled like any other operation (INV-6, review r7): Prepared before the
//! mutation, Committed/Failed after it, `Undo: true`, through `JournalTransaction` (W3-01). Undo shares the one
//! file-action gate with `FileActionService` (INV-4): it never runs while another file action holds it.
//!
//! Concurrency (docs/adr/R0003): the C# `async Task` methods are blocking here. The caller (App
//! `FileActionController`, W8) runs `undo_last` on the file-action lane, never on the UI thread; the C# `Task.Run`
//! hops inside the service have no Rust twin (the whole call already runs off the UI thread). The undo state is
//! `&mut self`: the C# rule "UI thread only" becomes "one owner" (the App keeps the service in a `Mutex`); only the
//! gate methods take `&self` and are thread-safe.
//!
//! Localization: C# reads `Tr.*` from the ambient localizer; `pr-core` cannot see `pr-l10n`, so the texts come from an
//! injected [`UndoTexts`] (same pattern as `settings::app_settings::SettingsTexts`, D13). The message of a coded
//! journal error this unit raises itself (`VerifySizeChanged`) comes from the journal's own
//! `OperationJournal::texts().localized(..)` (W3-01), exactly like the coded errors the transaction raises.

// TEMPORARY (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::journal::OperationJournal;
use super::journal_line::{JournalEntry, JournalGroupMember};
use super::types::{CaptureGroupActionResult, FileActionResult, UndoResult};
use crate::abstractions::{Clock, FileSystem, RecycleBin, SystemClock};
use crate::model::FileOperationType;
use crate::time::DateTimeUtc;

// ---------------------------------------------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------------------------------------------

/// The one file-action gate (INV-4) that `FileActionService` (W3-04) owns. C# passes the whole `FileActionService?`
/// and calls only `TryBegin`/`End`/`IsBusy` on it; Rust takes only that part so this unit does not depend on W3-04.
/// W3-04 implements it for `FileActionService` with exactly the C# semantics: `try_begin` = compare-exchange
/// 0 -> 1 (SeqCst), `end` = store 0, `is_busy` = load != 0.
pub trait FileActionGate: Send + Sync {
    /// C# `FileActionService.TryBegin`: takes the gate; `false` when it is already held.
    fn try_begin(&self) -> bool;
    /// C# `FileActionService.End`: releases the gate (unconditionally, like `Volatile.Write(ref _inProgress, 0)`).
    fn end(&self);
    /// C# `FileActionService.IsBusy`.
    fn is_busy(&self) -> bool;
}

/// C# `Func<string, string, Task>? moveOverride`: replaces `FileSystem::move_file` for the reverse move(s) of an undo,
/// called as `(from, to)` = (where the Move put the file, its original path). Production passes none (C#
/// `IMoveOverride` is a test seam of the App composition root). Blocking: the C# delegate's `Task` is awaited at
/// once. An `Arc` so the composition root can hand the SAME override to `FileActionService` (W3-04).
pub type MoveOverride = Arc<dyn Fn(&Path, &Path) -> io::Result<()> + Send + Sync>;

/// The localized texts this unit needs: C# `Tr.*` (generated from `en.json`; key in each doc line). An implementation
/// over the real catalogs comes from the App composition root; [`EnglishUndoTexts`] is the built-in English catalog,
/// which is what C# uses until a language is set and what the oracle/golden run with.
pub trait UndoTexts: Send + Sync {
    /// `Tr.CoreUndoBusy` (`core.undo.busy`).
    fn core_undo_busy(&self) -> String;
    /// `Tr.CoreUndoNoMoveToUndo` (`core.undo.noMoveToUndo`).
    fn core_undo_no_move_to_undo(&self) -> String;
    /// `Tr.CoreUndoNothingToUndo` (`core.undo.nothingToUndo`).
    fn core_undo_nothing_to_undo(&self) -> String;
    /// `Tr.CoreUndoSourceOrDestinationChanged` (`core.undo.sourceOrDestinationChanged`).
    fn core_undo_source_or_destination_changed(&self) -> String;
    /// `Tr.CoreUndoFingerprintMissing` (`core.undo.fingerprintMissing`).
    fn core_undo_fingerprint_missing(&self) -> String;
    /// `Tr.CoreUndoDestinationChangedAfterMove` (`core.undo.destinationChangedAfterMove`).
    fn core_undo_destination_changed_after_move(&self) -> String;
    /// `Tr.CoreUndoFailed(message)` (`core.undo.failed`, placeholder `{message}`).
    fn core_undo_failed(&self, message: &str) -> String;
    /// `Tr.CoreUndoPermanentlyDeleted(fileName)` (`core.undo.permanentlyDeleted`, placeholder `{fileName}`).
    fn core_undo_permanently_deleted(&self, file_name: &str) -> String;
    /// `Tr.CoreUndoRecycleTargetExists(fileName)` (`core.undo.recycleTargetExists`).
    fn core_undo_recycle_target_exists(&self, file_name: &str) -> String;
    /// `Tr.CoreUndoRecycleRestoreFailed(fileName)` (`core.undo.recycleRestoreFailed`).
    fn core_undo_recycle_restore_failed(&self, file_name: &str) -> String;
    /// `Tr.CoreUndoUnsupported` (`core.undo.unsupported`).
    fn core_undo_unsupported(&self) -> String;
    /// `Tr.CoreUndoGroupPartiallyRestored(restored, total, names)` (`core.undo.groupPartiallyRestored`; the two
    /// numbers are rendered with the invariant culture, W1-11).
    fn core_undo_group_partially_restored(&self, restored: usize, total: usize, names: &str) -> String;
    /// `Tr.CoreRecoveryAlreadyHandled` (`core.recovery.alreadyHandled`).
    fn core_recovery_already_handled(&self) -> String;
}

/// The built-in English catalog (`en.json` at d92d6aae). Complete: no porter work. A test compares every text with
/// `assets/languages/en.json` (see the contract, "C# tests to port").
#[derive(Debug, Default, Clone, Copy)]
pub struct EnglishUndoTexts;

impl UndoTexts for EnglishUndoTexts {
    fn core_undo_busy(&self) -> String {
        "Another operation is in progress.".to_owned()
    }
    fn core_undo_no_move_to_undo(&self) -> String {
        "There is no Move to undo.".to_owned()
    }
    fn core_undo_nothing_to_undo(&self) -> String {
        "There is no recent Move/Delete to undo.".to_owned()
    }
    fn core_undo_source_or_destination_changed(&self) -> String {
        "The source or destination has changed.".to_owned()
    }
    fn core_undo_fingerprint_missing(&self) -> String {
        "The Move fingerprint was not found in the journal.".to_owned()
    }
    fn core_undo_destination_changed_after_move(&self) -> String {
        "The destination file changed after the Move; it will not be undone automatically.".to_owned()
    }
    fn core_undo_failed(&self, message: &str) -> String {
        format!("Could not undo: {message}")
    }
    fn core_undo_permanently_deleted(&self, file_name: &str) -> String {
        format!("\"{file_name}\" was deleted permanently (its drive has no Recycle Bin), so it cannot be restored.")
    }
    fn core_undo_recycle_target_exists(&self, file_name: &str) -> String {
        format!("A file named {file_name} already exists at the original location; restoring would replace it.")
    }
    fn core_undo_recycle_restore_failed(&self, file_name: &str) -> String {
        format!("Could not restore from the Recycle Bin: {file_name}")
    }
    fn core_undo_unsupported(&self) -> String {
        "Undo is not supported for this operation.".to_owned()
    }
    fn core_undo_group_partially_restored(&self, restored: usize, total: usize, names: &str) -> String {
        format!("Restored {restored} of {total} files. Could not restore (deleted permanently): {names}")
    }
    fn core_recovery_already_handled(&self) -> String {
        "This entry was already handled by another PhotoReview window, or is being retried there right now. Nothing \
         was changed; the entry has been removed from this list."
            .to_owned()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Private state records (complete; no porter changes needed)
// ---------------------------------------------------------------------------------------------------------------

/// C# `(long Size, DateTime LastWriteUtc)` value of `_moveFingerprints`: the identity the Move committed for its
/// destination. Compared with [`super::types::matches_moved_destination`]; `size` is journaled as the undo's `Size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MoveFingerprint {
    size: i64,
    last_write_utc: DateTimeUtc,
}

/// C# `private sealed record UndoActionRecord`. Held in an `Arc` because C# compares it BY REFERENCE
/// (`ReferenceEquals(partial.Action, action)`) and mutates `CurrentFailedEntry` on the object that may still be
/// `_lastUndoAction`: use `Arc::ptr_eq` for identity and the `Mutex` for that one mutable field. (`ptr_eq` cannot
/// see a recycled address: a `PartialGroupUndo` keeps its action alive.)
#[derive(Debug)]
struct UndoActionRecord {
    operation: FileOperationType,
    source: String,
    destination: Option<String>,
    size: i64,
    /// C# `DateTime`: compared by ticks and handed to `RecycleBin::try_restore`; never journaled from here.
    last_write_utc: DateTimeUtc,
    /// Single Recycle: `FileActionResult::permanently_deleted`. Group: Recycle AND every member permanent.
    permanent: bool,
    /// `Some` (non-empty) only for a group action.
    group_members: Option<Vec<JournalGroupMember>>,
    /// The Failed Recovery line of a part-way group Delete (C# `FailedEntry`), never changed.
    failed_entry: Option<JournalEntry>,
    /// C# `CurrentFailedEntry { get; set; } = FailedEntry`: the latest Failed line this action put in the journal for
    /// its Id; replaced by every successful settle append.
    current_failed_entry: Mutex<Option<JournalEntry>>,
}

impl UndoActionRecord {
    fn current_failed_entry(&self) -> Option<JournalEntry> {
        self.current_failed_entry
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_current_failed_entry(&self, entry: JournalEntry) {
        *self.current_failed_entry.lock().unwrap_or_else(PoisonError::into_inner) = Some(entry);
    }
}

/// C# `private sealed record PartialGroupUndo(object Action, IReadOnlyList<JournalEntry> FailedEntries)`: a group
/// undo that restored members by its own mutations and then failed. `failed_entries` keeps every Failed undo line it
/// wrote, oldest first, so a later successful retry closes ALL of them.
#[derive(Debug)]
struct PartialGroupUndo {
    action: Arc<UndoActionRecord>,
    failed_entries: Vec<JournalEntry>,
}

/// Holds the gate taken by [`UndoService::try_begin`] and releases it on drop (C# `finally { End(); }`), so a panic
/// inside the undo (a buggy `FileSystem`) cannot leave the gate locked (C# test
/// `UndoRecycle_FileSystemThrows_GateIsReleased`). Built from clones so it does not borrow the service.
struct GateHold {
    gate: Option<Arc<dyn FileActionGate>>,
    own_gate: Arc<AtomicBool>,
}

impl Drop for GateHold {
    fn drop(&mut self) {
        match &self.gate {
            Some(gate) => gate.end(),
            None => self.own_gate.store(false, Ordering::SeqCst),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// UndoService
// ---------------------------------------------------------------------------------------------------------------

/// C# `UndoService`. See the module docs.
pub struct UndoService {
    journal: Arc<OperationJournal>,
    file_system: Arc<dyn FileSystem>,
    recycle_bin: Arc<dyn RecycleBin>,
    /// C# `_fileActionService` (only its gate is used). `None`: the service's own gate below.
    gate: Option<Arc<dyn FileActionGate>>,
    move_override: Option<MoveOverride>,
    clock: Arc<dyn Clock>,
    texts: Arc<dyn UndoTexts>,
    /// C# `_internalInProgress` (used only when `gate` is `None`).
    own_gate: Arc<AtomicBool>,
    /// C# `Stack<(string Source, string Destination)> _moveHistory`: the TOP is the LAST element.
    move_history: Vec<(String, String)>,
    /// C# `Dictionary<string, (long, DateTime)>(StringComparer.OrdinalIgnoreCase)`: keyed by
    /// `crate::text::ignore_case_key(destination)`. Its order never leaks, so a `HashMap` is fine.
    move_fingerprints: HashMap<String, MoveFingerprint>,
    /// C# `_lastUndoAction`.
    last_undo_action: Option<Arc<UndoActionRecord>>,
    /// C# `_partialGroupUndo`.
    partial_group_undo: Option<PartialGroupUndo>,
}

impl fmt::Debug for UndoService {
    /// The undo state only (the injected services have no `Debug`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UndoService")
            .field("shared_gate", &self.gate.is_some())
            .field("move_override", &self.move_override.is_some())
            .field("move_history", &self.move_history)
            .field("move_fingerprints", &self.move_fingerprints.len())
            .field("last_undo_action", &self.last_undo_action)
            .field("partial_group_undo", &self.partial_group_undo)
            .finish_non_exhaustive()
    }
}

// The App keeps the service in a `Mutex` shared by the UI thread (register) and the file-action lane (undo).
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<UndoService>();
};

impl UndoService {
    /// C# `new UndoService(journal, fileSystem, recycleBin)`: no file-action service (own gate), no move override,
    /// `SystemClock`. `texts` replaces the ambient `Tr` (pass `Arc::new(EnglishUndoTexts)` in tests).
    pub fn new(
        journal: Arc<OperationJournal>,
        file_system: Arc<dyn FileSystem>,
        recycle_bin: Arc<dyn RecycleBin>,
        texts: Arc<dyn UndoTexts>,
    ) -> Self {
        Self {
            journal,
            file_system,
            recycle_bin,
            gate: None,
            move_override: None,
            clock: Arc::new(SystemClock),
            texts,
            own_gate: Arc::new(AtomicBool::new(false)),
            move_history: Vec::new(),
            move_fingerprints: HashMap::new(),
            last_undo_action: None,
            partial_group_undo: None,
        }
    }

    /// C# `fileActionService:` argument: share `FileActionService`'s gate (INV-4) instead of the own one.
    pub fn with_gate(mut self, gate: Arc<dyn FileActionGate>) -> Self {
        self.gate = Some(gate);
        self
    }

    /// C# `moveOverride:` argument (test seam).
    pub fn with_move_override(mut self, move_override: MoveOverride) -> Self {
        self.move_override = Some(move_override);
        self
    }

    /// C# `clock:` argument (default `SystemClock`). Stamps the `TimestampUtc` of every journal line this service
    /// writes (Prepared, and the Committed/Failed/Dismissed lines it builds itself).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// C# `internal int MoveFingerprintCount` (test seam, Q-R small-findings #2): number of cached move fingerprints.
    pub fn move_fingerprint_count(&self) -> usize {
        self.move_fingerprints.len()
    }

    /// C# `MoveHistory` (read side), OLDEST FIRST: `last()` is the C# `Peek()`. (C# enumerates a `Stack` top first;
    /// a test that compares the whole history reverses one side.)
    pub fn move_history(&self) -> &[(String, String)] {
        &self.move_history
    }

    /// C# `MoveHistory.Push(...)` from a caller (compatibility path: a pushed entry has no fingerprint and falls back
    /// to the journal, see `undo_move`). Push = `push`, top = last element.
    pub fn move_history_mut(&mut self) -> &mut Vec<(String, String)> {
        &mut self.move_history
    }

    /// C# `LastUndoAction is not null` (the C# property is an opaque `object?`, only ever tested for null).
    pub fn has_last_undo_action(&self) -> bool {
        self.last_undo_action.is_some()
    }

    /// C# `IsBusy`: the shared gate's `is_busy()` when one was given, else the own flag (SeqCst load).
    pub fn is_busy(&self) -> bool {
        todo!("W3-05")
    }

    /// C# `TryBegin` (INV-4): the shared gate's `try_begin()`, else compare-exchange of the own flag false -> true.
    pub fn try_begin(&self) -> bool {
        todo!("W3-05")
    }

    /// C# `End`: the shared gate's `end()`, else store false.
    pub fn end(&self) {
        todo!("W3-05")
    }

    /// C# `Register(FileActionResult)`: makes a successful single Move or Recycle the undoable last action. Contract,
    /// semantics 2.
    pub fn register(&mut self, result: &FileActionResult) {
        todo!("W3-05")
    }

    /// C# `RegisterGroup(CaptureGroupActionResult)`: makes a capture-group Move/Recycle (or the completed members of
    /// a part-way failed group Delete) the undoable last action. Contract, semantics 3.
    pub fn register_group(&mut self, result: &CaptureGroupActionResult) {
        todo!("W3-05")
    }

    /// C# `UndoMoveAsync`: undoes the Move on TOP of the history (not necessarily the last action). Blocking; takes
    /// the gate itself. Contract, semantics 4.
    pub fn undo_move(&mut self) -> UndoResult {
        todo!("W3-05")
    }

    /// C# `UndoLastAsync`: undoes the last registered action (Move, Recycle, group Move, group Recycle). Blocking.
    /// Rejected without touching any state while the gate is held. Contract, semantics 5-9.
    pub fn undo_last(&mut self) -> UndoResult {
        todo!("W3-05")
    }

    // ---- private helpers (C# private methods; the porter may restructure them, the public API is fixed) ----

    /// Takes the gate and returns its RAII release, or `None` when it is held (C# `if (!TryBegin()) return ...`).
    fn begin_hold(&self) -> Option<GateHold> {
        todo!("W3-05")
    }

    /// C# `SafeFileExists`: the Rust `file_exists` cannot fail, so this is a plain call (kept for traceability).
    fn safe_file_exists(&self, path: &str) -> bool {
        todo!("W3-05")
    }

    /// C# `new JournalCodedException(code)` raised by this unit (only `errors::VERIFY_SIZE_CHANGED`):
    /// `JournalCodedError::new(code, <journal.texts().localized(catalog key of code)>).into_io_error()`.
    fn coded_error(&self, code: &str) -> io::Error {
        todo!("W3-05")
    }

    /// The single-Move body of C# `UndoMoveAsync` after the gate was taken and the top entry popped (semantics 4.2+).
    fn undo_single_move(&mut self, source: String, destination: String) -> UndoResult {
        todo!("W3-05")
    }

    /// The single-Recycle branch of C# `UndoLastAsync` (after the busy and permanent checks).
    fn undo_single_recycle(&mut self, action: Arc<UndoActionRecord>) -> UndoResult {
        todo!("W3-05")
    }

    /// C# `UndoGroupMoveAsync(action, members)`.
    fn undo_group_move(&mut self, action: Arc<UndoActionRecord>) -> UndoResult {
        todo!("W3-05")
    }

    /// C# `UndoGroupRecycleAsync(action, members)`.
    fn undo_group_recycle(&mut self, action: Arc<UndoActionRecord>) -> UndoResult {
        todo!("W3-05")
    }

    /// C# `RememberPartialUndo(action, restoredByThisUndo, failedEntry)`.
    fn remember_partial_undo(
        &mut self,
        action: &Arc<UndoActionRecord>,
        restored_by_this_undo: usize,
        failed_entry: Option<JournalEntry>,
    ) {
        todo!("W3-05")
    }

    /// C# `ResolvePartialUndo(action)`: ALWAYS clears the remembered partial undo; `true` (after closing its Failed
    /// lines, best effort) only when it belonged to `action`.
    fn resolve_partial_undo(&mut self, action: &Arc<UndoActionRecord>) -> bool {
        todo!("W3-05")
    }

    /// C# `SettleFailedDeleteLine(action, restored)` (best effort, never fails the undo).
    fn settle_failed_delete_line(&self, action: &UndoActionRecord, restored: &[String]) {
        todo!("W3-05")
    }

    /// C# `TrySettleFrom(action, anchor, restored)`.
    fn try_settle_from(
        &self,
        action: &UndoActionRecord,
        anchor: &JournalEntry,
        restored: &[String],
    ) -> io::Result<bool> {
        todo!("W3-05")
    }

    /// C# `DropGroupUndo(members)`: forgets every member destination's fingerprint and the last action.
    fn drop_group_undo(&mut self, members: &[JournalGroupMember]) {
        todo!("W3-05")
    }
}

/// C# `RestoredInOrder(members, restored)`: the member sources that are in `restored` (OrdinalIgnoreCase), in MEMBER
/// order, each as the member spells it.
fn restored_in_order(members: &[JournalGroupMember], restored: &[String]) -> Vec<String> {
    todo!("W3-05")
}

/// C# `UnrecoverableNote(members, restored, unrecoverable)`: `None` when nothing is unrecoverable, else
/// `core_undo_group_partially_restored(restored_in_order(..).len(), members.len(), <file names of the unrecoverable
/// members joined by ", ">)`.
fn unrecoverable_note(
    texts: &dyn UndoTexts,
    members: &[JournalGroupMember],
    restored: &[String],
    unrecoverable: &[JournalGroupMember],
) -> Option<String> {
    todo!("W3-05")
}
