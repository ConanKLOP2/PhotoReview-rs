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

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::errors::{self, JournalCodedError};
use super::journal::OperationJournal;
use super::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use super::transaction::JournalTransaction;
use super::types::{CaptureGroupActionResult, FileActionResult, UndoResult, matches_moved_destination};
use crate::abstractions::{Clock, FileStat, FileSystem, RecycleBin, SystemClock};
use crate::io::physical::new_guid_n;
use crate::model::{FileOperationType, JournalState};
use crate::text::dotnet_path::{get_directory_name, get_file_name};
use crate::text::{eq_ignore_case, ignore_case_key};
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
        match &self.gate {
            Some(gate) => gate.is_busy(),
            None => self.own_gate.load(Ordering::SeqCst),
        }
    }

    /// C# `TryBegin` (INV-4): the shared gate's `try_begin()`, else compare-exchange of the own flag false -> true.
    pub fn try_begin(&self) -> bool {
        match &self.gate {
            Some(gate) => gate.try_begin(),
            None => self
                .own_gate
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
        }
    }

    /// C# `End`: the shared gate's `end()`, else store false.
    pub fn end(&self) {
        match &self.gate {
            Some(gate) => gate.end(),
            None => self.own_gate.store(false, Ordering::SeqCst),
        }
    }

    /// C# `Register(FileActionResult)`: makes a successful single Move or Recycle the undoable last action. Contract,
    /// semantics 2.
    pub fn register(&mut self, result: &FileActionResult) {
        if !result.succeeded || result.rejected {
            return;
        }
        match (result.operation, result.destination_path.as_deref()) {
            (FileOperationType::Move, Some(destination)) if !destination.is_empty() => {
                self.move_history.push((result.source.clone(), destination.to_owned()));
                self.move_fingerprints.insert(
                    ignore_case_key(destination),
                    MoveFingerprint {
                        size: result.size,
                        last_write_utc: result.last_write_utc,
                    },
                );
                self.last_undo_action = Some(Arc::new(UndoActionRecord::new(
                    FileOperationType::Move,
                    result.source.clone(),
                    Some(destination.to_owned()),
                    result.size,
                    result.last_write_utc,
                    false,
                    None,
                    None,
                )));
            }
            (FileOperationType::Recycle, _) => {
                self.last_undo_action = Some(Arc::new(UndoActionRecord::new(
                    FileOperationType::Recycle,
                    result.source.clone(),
                    None,
                    result.size,
                    result.last_write_utc,
                    result.permanently_deleted,
                    None,
                    None,
                )));
            }
            _ => {}
        }
    }

    /// C# `RegisterGroup(CaptureGroupActionResult)`: makes a capture-group Move/Recycle (or the completed members of
    /// a part-way failed group Delete) the undoable last action. Contract, semantics 3.
    pub fn register_group(&mut self, result: &CaptureGroupActionResult) {
        if result.rejected {
            return;
        }
        let entry = result.entry.as_deref();
        let members: Vec<JournalGroupMember> = if result.succeeded {
            match entry.and_then(|entry| entry.group_members.as_ref()) {
                Some(members) if !members.is_empty() => members.clone(),
                _ => return,
            }
        } else if result.operation == FileOperationType::Recycle {
            // A group Delete that failed part-way: the members that already reached the bin still deserve a Ctrl+Z.
            let completed: Vec<JournalGroupMember> = result
                .members
                .iter()
                .filter(|member| member.completed)
                .map(|member| member.member.clone())
                .collect();
            if completed.is_empty() {
                return;
            }
            completed
        } else {
            return;
        };
        if !matches!(result.operation, FileOperationType::Move | FileOperationType::Recycle) {
            return;
        }
        let Some(first) = members.first() else {
            return;
        };
        let source = entry.map_or_else(|| first.source.clone(), |entry| entry.source.clone());
        // C# `result.Entry?.Destination ?? members[0].Destination`: a null entry destination falls back too.
        let destination = entry
            .and_then(|entry| entry.destination.clone())
            .or_else(|| first.destination.clone());
        let size = entry.map_or(first.size, |entry| entry.size);
        let last_write_utc = entry.map_or(first.last_write_utc.value, |entry| entry.last_write_utc.value);
        // Permanence is per member: only a capture whose EVERY member was deleted permanently has nothing to restore.
        let permanent = result.operation == FileOperationType::Recycle && members.iter().all(|member| member.permanent);
        let failed_entry = entry
            .filter(|entry| {
                !result.succeeded
                    && entry.state == JournalState::Failed
                    && entry.group_members.as_ref().is_some_and(|members| !members.is_empty())
            })
            .cloned();
        self.last_undo_action = Some(Arc::new(UndoActionRecord::new(
            result.operation,
            source,
            destination,
            size,
            last_write_utc,
            permanent,
            Some(members),
            failed_entry,
        )));
    }

    /// C# `UndoMoveAsync`: undoes the Move on TOP of the history (not necessarily the last action). Blocking; takes
    /// the gate itself. Contract, semantics 4.
    pub fn undo_move(&mut self) -> UndoResult {
        let Some(_hold) = self.begin_hold() else {
            return UndoResult {
                rejected: true,
                ..UndoResult::new(
                    false,
                    Some(FileOperationType::Move),
                    "",
                    None,
                    Some(self.texts.core_undo_busy()),
                )
            };
        };
        let Some((source, destination)) = self.move_history.pop() else {
            return UndoResult::new(
                false,
                Some(FileOperationType::Move),
                "",
                None,
                Some(self.texts.core_undo_no_move_to_undo()),
            );
        };
        self.undo_single_move(source, destination)
    }

    /// C# `UndoLastAsync`: undoes the last registered action (Move, Recycle, group Move, group Recycle). Blocking.
    /// Rejected without touching any state while the gate is held. Contract, semantics 5-9.
    pub fn undo_last(&mut self) -> UndoResult {
        if self.is_busy() {
            return UndoResult {
                rejected: true,
                ..UndoResult::new(false, None, "", None, Some(self.texts.core_undo_busy()))
            };
        }
        let Some(action) = self.last_undo_action.clone() else {
            return UndoResult::new(false, None, "", None, Some(self.texts.core_undo_nothing_to_undo()));
        };
        let is_group = action.group_members.as_ref().is_some_and(|members| !members.is_empty());
        match action.operation {
            FileOperationType::Move if is_group => self.undo_group_move(action),
            FileOperationType::Move => self.undo_move(),
            FileOperationType::Recycle if is_group => self.undo_group_recycle(action),
            FileOperationType::Recycle => {
                // Q-R8: deleted permanently on a drive without a Recycle Bin: nothing to restore, say so.
                if action.permanent {
                    self.last_undo_action = None;
                    return UndoResult::new(
                        false,
                        Some(FileOperationType::Recycle),
                        action.source.clone(),
                        None,
                        Some(self.texts.core_undo_permanently_deleted(get_file_name(&action.source))),
                    );
                }
                self.undo_single_recycle(action)
            }
            other => UndoResult::new(
                false,
                Some(other),
                action.source.clone(),
                None,
                Some(self.texts.core_undo_unsupported()),
            ),
        }
    }

    // ---- private helpers ----

    /// Takes the gate and returns its RAII release, or `None` when it is held (C# `if (!TryBegin()) return ...`).
    fn begin_hold(&self) -> Option<GateHold> {
        if !self.try_begin() {
            return None;
        }
        Some(GateHold {
            gate: self.gate.clone(),
            own_gate: Arc::clone(&self.own_gate),
        })
    }

    /// C# `SafeFileExists`: the Rust `file_exists` cannot fail, so this is a plain call (kept for traceability).
    fn safe_file_exists(&self, path: &str) -> bool {
        self.file_system.file_exists(Path::new(path))
    }

    /// C# `GetFileStat(path)` (C# returns null for a missing file).
    fn stat(&self, path: &str) -> Option<FileStat> {
        self.file_system.get_file_stat(Path::new(path))
    }

    /// C# `new JournalCodedException(code)` raised by this unit (only `errors::VERIFY_SIZE_CHANGED`):
    /// `JournalCodedError::new(code, <journal.texts().localized(catalog key of code)>).into_io_error()`.
    fn coded_error(&self, code: &str) -> io::Error {
        let texts = self.journal.texts();
        // DIVERGENCE: an unknown code (a caller bug, only constants are passed) uses the code as its message (T21).
        let message = errors::text(code, &|key| texts.localized(key)).unwrap_or_else(|_| code.to_owned());
        JournalCodedError::new(code, message).into_io_error()
    }

    /// C# `throw new IOException(message)`.
    fn plain_error(message: String) -> io::Error {
        io::Error::other(message)
    }

    /// The single-Move body of C# `UndoMoveAsync` after the gate was taken and the top entry popped (semantics 4.2+).
    fn undo_single_move(&mut self, source: String, destination: String) -> UndoResult {
        // The destination is gone (deleted/renamed outside the app): this entry can never be undone. Report it once and
        // drop it, otherwise every later Ctrl+Z hits the same dead entry.
        let mut permanently_broken = false;
        let mut move_returned = false;
        let outcome = self.try_undo_single_move(&source, &destination, &mut permanently_broken, &mut move_returned);
        match outcome {
            Ok(()) => {
                // The fingerprint no longer describes anything at Destination: drop it (no unbounded growth).
                self.move_fingerprints.remove(&ignore_case_key(&destination));
                // RV-C06: only forget the last action when it is this Move.
                let is_this_move = self.last_undo_action.as_ref().is_some_and(|last| {
                    last.operation == FileOperationType::Move
                        && last.group_members.is_none()
                        && eq_ignore_case(&last.source, &source)
                        && last
                            .destination
                            .as_deref()
                            .is_some_and(|last_destination| eq_ignore_case(last_destination, &destination))
                });
                if is_this_move {
                    self.last_undo_action = None;
                }
                UndoResult::new(true, Some(FileOperationType::Move), source, Some(destination), None)
            }
            Err(ex) => {
                // The move itself ran and the file is at Source (only the verification failed): nothing is left at
                // Destination, so the entry can never be undone again. Report once and drop it.
                if move_returned
                    && !permanently_broken
                    && self.safe_file_exists(&source)
                    && !self.safe_file_exists(&destination)
                {
                    permanently_broken = true;
                }
                if permanently_broken {
                    self.move_fingerprints.remove(&ignore_case_key(&destination));
                    let is_move_to_destination = self.last_undo_action.as_ref().is_some_and(|last| {
                        last.operation == FileOperationType::Move
                            && last
                                .destination
                                .as_deref()
                                .is_some_and(|last_destination| eq_ignore_case(last_destination, &destination))
                    });
                    if is_move_to_destination {
                        self.last_undo_action = None;
                    }
                } else {
                    self.move_history.push((source.clone(), destination.clone()));
                }
                let message = self.texts.core_undo_failed(&ex.to_string());
                UndoResult::new(
                    false,
                    Some(FileOperationType::Move),
                    source,
                    Some(destination),
                    Some(message),
                )
            }
        }
    }

    /// The C# outer `try` of `UndoMoveAsync`: preflight, fingerprint, journaled reverse move.
    fn try_undo_single_move(
        &mut self,
        source: &str,
        destination: &str,
        permanently_broken: &mut bool,
        move_returned: &mut bool,
    ) -> io::Result<()> {
        if !self.file_system.file_exists(Path::new(destination)) {
            *permanently_broken = true;
            return Err(Self::plain_error(self.texts.core_undo_source_or_destination_changed()));
        }
        if self.file_system.file_exists(Path::new(source)) {
            return Err(Self::plain_error(self.texts.core_undo_source_or_destination_changed()));
        }
        let destination_stat = self.stat(destination);
        let key = ignore_case_key(destination);
        let fingerprint = match self.move_fingerprints.get(&key) {
            Some(fingerprint) => *fingerprint,
            None => {
                // Compatibility fallback for callers that filled the public history directly.
                let committed = self.journal.read_committed_moves()?.into_iter().rev().find(|entry| {
                    entry
                        .destination
                        .as_deref()
                        .is_some_and(|entry_destination| eq_ignore_case(entry_destination, destination))
                });
                let Some(committed) = committed else {
                    return Err(Self::plain_error(self.texts.core_undo_fingerprint_missing()));
                };
                let fingerprint = MoveFingerprint {
                    size: committed.size,
                    last_write_utc: committed.last_write_utc.value,
                };
                self.move_fingerprints.insert(key, fingerprint);
                fingerprint
            }
        };
        // RV-C01: destination-side compare, tolerant of a volume that rounds the write time.
        let Some(destination_stat) = destination_stat
            .filter(|stat| matches_moved_destination(stat, fingerprint.size, fingerprint.last_write_utc))
        else {
            return Err(Self::plain_error(self.texts.core_undo_destination_changed_after_move()));
        };

        // Review r7 (INV-6): the undo is itself a journaled Move (destination -> source).
        let journal = Arc::clone(&self.journal);
        let clock = Arc::clone(&self.clock);
        let prepared = JournalEntry {
            id: new_guid_n(),
            op_type: FileOperationType::Move,
            state: JournalState::Prepared,
            source: destination.to_owned(),
            destination: Some(source.to_owned()),
            size: fingerprint.size,
            // The file as it is now (its stamp may be the destination volume's rounding of the original).
            last_write_utc: JournalDateTime::utc(destination_stat.last_write_utc),
            timestamp_utc: JournalDateTime::utc(clock.utc_now()),
            error: None,
            error_code: None,
            permanent: None,
            undo: Some(true),
            group_id: None,
            group_members: None,
        };
        let mut tx = JournalTransaction::new(&journal, &*clock, prepared, None);
        if let Err(undo_failure) =
            self.reverse_single_move(&mut tx, source, destination, destination_stat, move_returned)
        {
            let _ = tx.fail(&undo_failure);
            return Err(undo_failure);
        }
        // A failed Committed append does not undo the completed move.
        let _ = tx.commit();
        Ok(())
    }

    /// The C# inner `try` of `UndoMoveAsync`: Prepared, R18 re-check, reverse move, verification.
    fn reverse_single_move(
        &self,
        tx: &mut JournalTransaction<'_>,
        source: &str,
        destination: &str,
        destination_stat: FileStat,
        move_returned: &mut bool,
    ) -> io::Result<()> {
        tx.begin()?;
        // R18: prove again, right before the mutation, that the file at Destination is the one the preflight verified.
        let unchanged = self.stat(destination).is_some_and(|before_move| {
            before_move.length == destination_stat.length
                && before_move.last_write_utc == destination_stat.last_write_utc
        });
        if !unchanged {
            return Err(Self::plain_error(self.texts.core_undo_destination_changed_after_move()));
        }
        // RV-C07: the original folder may have been removed after the Move.
        if let Some(folder) = get_directory_name(source).filter(|folder| !folder.is_empty()) {
            self.file_system.create_directory(Path::new(&folder))?;
        }
        self.reverse_move(destination, source)?;
        *move_returned = true;
        tx.verify_moved(
            self.file_system.as_ref(),
            destination,
            source,
            errors::VERIFY_SIZE_CHANGED,
        )
    }

    /// The reverse move `(from, to)`: the move override when one was given, else `FileSystem::move_file`.
    fn reverse_move(&self, from: &str, to: &str) -> io::Result<()> {
        match &self.move_override {
            Some(move_override) => move_override(Path::new(from), Path::new(to)),
            None => self.file_system.move_file(Path::new(from), Path::new(to)),
        }
    }

    /// The single-Recycle branch of C# `UndoLastAsync` (after the busy and permanent checks).
    fn undo_single_recycle(&mut self, action: Arc<UndoActionRecord>) -> UndoResult {
        let Some(_hold) = self.begin_hold() else {
            return UndoResult {
                rejected: true,
                ..UndoResult::new(
                    false,
                    Some(FileOperationType::Recycle),
                    action.source.clone(),
                    None,
                    Some(self.texts.core_undo_busy()),
                )
            };
        };
        let file_name = get_file_name(&action.source).to_owned();
        // The shell Restore verb would replace (or prompt about) a newer file at the original path.
        if self.file_system.file_exists(Path::new(&action.source)) {
            return UndoResult::new(
                false,
                Some(FileOperationType::Recycle),
                action.source.clone(),
                None,
                Some(self.texts.core_undo_recycle_target_exists(&file_name)),
            );
        }
        // A bin that fails is not trusted either way: the proof below decides (P-13: `Err` is the C# throw).
        let restore_fault = self
            .recycle_bin
            .try_restore(
                Path::new(&action.source),
                u64::try_from(action.size).unwrap_or(0),
                action.last_write_utc,
            )
            .err();
        // R18: the bin's verdict is not the proof; the recycled file itself (size AND write time) must be back.
        let restored = self.stat(&action.source).is_some_and(|back| {
            i64::try_from(back.length) == Ok(action.size) && back.last_write_utc == action.last_write_utc
        });
        if !restored {
            let message = match restore_fault {
                None => self.texts.core_undo_recycle_restore_failed(&file_name),
                Some(fault) => self.texts.core_undo_failed(&fault.to_string()),
            };
            return UndoResult::new(
                false,
                Some(FileOperationType::Recycle),
                action.source.clone(),
                None,
                Some(message),
            );
        }
        self.last_undo_action = None;
        UndoResult::new(
            true,
            Some(FileOperationType::Recycle),
            action.source.clone(),
            None,
            None,
        )
    }

    /// C# `UndoGroupMoveAsync(action, members)`.
    fn undo_group_move(&mut self, action: Arc<UndoActionRecord>) -> UndoResult {
        let Some(_hold) = self.begin_hold() else {
            return UndoResult {
                rejected: true,
                ..UndoResult::new(
                    false,
                    Some(FileOperationType::Move),
                    action.source.clone(),
                    action.destination.clone(),
                    Some(self.texts.core_undo_busy()),
                )
            };
        };
        let members = action.group_members.clone().unwrap_or_default();
        let journal = Arc::clone(&self.journal);
        let clock = Arc::clone(&self.clock);
        let mut tx: Option<JournalTransaction<'_>> = None;
        // Original paths that are back in place (restored by this call or earlier), reported even on failure.
        let mut restored: Vec<String> = Vec::new();
        let mut restored_by_this_undo = 0usize;
        let outcome = self.try_undo_group_move(
            &action,
            &members,
            &journal,
            &*clock,
            &mut tx,
            &mut restored,
            &mut restored_by_this_undo,
        );
        match outcome {
            Ok(result) => result,
            Err(ex) => {
                let failed = tx.as_mut().and_then(|tx| tx.fail(&ex).0);
                self.remember_partial_undo(&action, restored_by_this_undo, failed);
                UndoResult {
                    restored_paths: (!restored.is_empty()).then_some(restored),
                    ..UndoResult::new(
                        false,
                        Some(FileOperationType::Move),
                        action.source.clone(),
                        action.destination.clone(),
                        Some(self.texts.core_undo_failed(&ex.to_string())),
                    )
                }
            }
        }
    }

    /// The C# `try` block of `UndoGroupMoveAsync`.
    #[allow(clippy::too_many_arguments)]
    fn try_undo_group_move<'a>(
        &mut self,
        action: &Arc<UndoActionRecord>,
        members: &[JournalGroupMember],
        journal: &'a OperationJournal,
        clock: &'a dyn Clock,
        tx: &mut Option<JournalTransaction<'a>>,
        restored: &mut Vec<String>,
        restored_by_this_undo: &mut usize,
    ) -> io::Result<UndoResult> {
        // C# `Select(..).ToArray()`: every undo member is built before the preflight (a missing destination fails first).
        let mut undo_members = Vec::with_capacity(members.len());
        for member in members {
            let Some(moved_to) = member.destination.clone() else {
                return Err(Self::plain_error(self.texts.core_undo_source_or_destination_changed()));
            };
            undo_members.push(JournalGroupMember {
                source: moved_to,
                destination: Some(member.source.clone()),
                size: member.size,
                last_write_utc: member.last_write_utc,
                permanent: false,
            });
        }
        let mut pending: Vec<JournalGroupMember> = Vec::new();
        for undo_member in &mut undo_members {
            let original = undo_member.destination.clone().unwrap_or_default();
            let source_stat = self.stat(&undo_member.source);
            let destination_stat = self.stat(&original);
            if source_stat.is_some() && destination_stat.is_some() {
                return Err(Self::plain_error(self.texts.core_undo_source_or_destination_changed()));
            }
            // Already back at the original path: only when it is the moved file itself (tolerant compare, RV-C01).
            if source_stat.is_none()
                && destination_stat.is_some_and(|stat| {
                    matches_moved_destination(&stat, undo_member.size, undo_member.last_write_utc.value)
                })
            {
                restored.push(original);
                continue;
            }
            let Some(source_stat) = source_stat.filter(|stat| {
                destination_stat.is_none()
                    && matches_moved_destination(stat, undo_member.size, undo_member.last_write_utc.value)
            }) else {
                return Err(Self::plain_error(self.texts.core_undo_destination_changed_after_move()));
            };
            // Journal the file as it is now (the stamp the destination volume stored).
            undo_member.last_write_utc = JournalDateTime::utc(source_stat.last_write_utc);
            pending.push(undo_member.clone());
        }
        if pending.is_empty() {
            // Everything is already back: drop the entry instead of failing the same way on every later Ctrl+Z.
            let resumed = self.resolve_partial_undo(action);
            self.drop_group_undo(members);
            if resumed {
                return Ok(Self::group_move_success(action, members));
            }
            return Err(Self::plain_error(self.texts.core_recovery_already_handled()));
        }
        let Some(first) = undo_members.first() else {
            return Err(Self::plain_error(self.texts.core_undo_source_or_destination_changed()));
        };
        let id = new_guid_n();
        let group_id = new_guid_n();
        let prepared = JournalEntry {
            id,
            op_type: FileOperationType::Move,
            state: JournalState::Prepared,
            source: first.source.clone(),
            destination: first.destination.clone(),
            size: first.size,
            last_write_utc: first.last_write_utc,
            timestamp_utc: JournalDateTime::utc(clock.utc_now()),
            error: None,
            error_code: None,
            permanent: None,
            undo: Some(true),
            group_id: Some(group_id),
            group_members: Some(undo_members.clone()),
        };
        let tx = tx.insert(JournalTransaction::new(journal, clock, prepared, None));
        tx.begin()?;
        // R18: re-prove every file's journaled identity before the FIRST move, and again right before each move.
        for member in &pending {
            self.require_journaled_identity(member)?;
        }
        for member in &pending {
            self.require_journaled_identity(member)?;
            let original = member.destination.clone().unwrap_or_default();
            if let Some(folder) = get_directory_name(&original).filter(|folder| !folder.is_empty()) {
                self.file_system.create_directory(Path::new(&folder))?;
            }
            self.reverse_move(&member.source, &original)?;
            // C# `FileExists(member.Source) || GetFileStat(original)?.Length != member.Size`: the existence probe runs
            // first and short-circuits, so a file still at its move destination costs no stat of the original path.
            if self.file_system.file_exists(Path::new(&member.source))
                || !self
                    .stat(&original)
                    .is_some_and(|stat| i64::try_from(stat.length) == Ok(member.size))
            {
                return Err(self.coded_error(errors::VERIFY_SIZE_CHANGED));
            }
            restored.push(original);
            *restored_by_this_undo += 1;
        }
        tx.mark_mutation_completed();
        let _ = tx.commit();
        // A retry that really restored the remaining members completes the earlier partial undo: close its Failed lines.
        let _ = self.resolve_partial_undo(action);
        self.drop_group_undo(members);
        Ok(Self::group_move_success(action, members))
    }

    /// R18 helper of the group Move: the file to move back still has the journaled size and EXACT write time.
    fn require_journaled_identity(&self, member: &JournalGroupMember) -> io::Result<()> {
        let unchanged = self.stat(&member.source).is_some_and(|now| {
            i64::try_from(now.length) == Ok(member.size) && now.last_write_utc == member.last_write_utc.value
        });
        if unchanged {
            Ok(())
        } else {
            Err(Self::plain_error(self.texts.core_undo_destination_changed_after_move()))
        }
    }

    /// The success result of a group Move undo: every member source, in member order.
    fn group_move_success(action: &UndoActionRecord, members: &[JournalGroupMember]) -> UndoResult {
        UndoResult {
            restored_paths: Some(members.iter().map(|member| member.source.clone()).collect()),
            ..UndoResult::new(
                true,
                Some(FileOperationType::Move),
                action.source.clone(),
                action.destination.clone(),
                None,
            )
        }
    }

    /// C# `UndoGroupRecycleAsync(action, members)`.
    fn undo_group_recycle(&mut self, action: Arc<UndoActionRecord>) -> UndoResult {
        if action.permanent {
            self.last_undo_action = None;
            return UndoResult::new(
                false,
                Some(FileOperationType::Recycle),
                action.source.clone(),
                None,
                Some(self.texts.core_undo_permanently_deleted(get_file_name(&action.source))),
            );
        }
        let Some(_hold) = self.begin_hold() else {
            return UndoResult {
                rejected: true,
                ..UndoResult::new(
                    false,
                    Some(FileOperationType::Recycle),
                    action.source.clone(),
                    None,
                    Some(self.texts.core_undo_busy()),
                )
            };
        };
        let members = action.group_members.clone().unwrap_or_default();
        let journal = Arc::clone(&self.journal);
        let clock = Arc::clone(&self.clock);
        let mut tx: Option<JournalTransaction<'_>> = None;
        // Original paths that are back (restored by this call or earlier); reported even when a later member fails.
        let mut restored: Vec<String> = Vec::new();
        let mut restored_by_this_undo = 0usize;
        let outcome = self.try_undo_group_recycle(
            &action,
            &members,
            &journal,
            &*clock,
            &mut tx,
            &mut restored,
            &mut restored_by_this_undo,
        );
        match outcome {
            Ok(result) => result,
            Err(ex) => {
                let failed = tx.as_mut().and_then(|tx| tx.fail(&ex).0);
                self.remember_partial_undo(&action, restored_by_this_undo, failed);
                // Members that did come back must not stay on the Failed Delete line a fresh Recovery would retry.
                if !restored.is_empty() {
                    self.settle_failed_delete_line(&action, &restored);
                }
                UndoResult {
                    restored_paths: (!restored.is_empty()).then_some(restored),
                    ..UndoResult::new(
                        false,
                        Some(FileOperationType::Recycle),
                        action.source.clone(),
                        None,
                        Some(self.texts.core_undo_failed(&ex.to_string())),
                    )
                }
            }
        }
    }

    /// The C# `try` block of `UndoGroupRecycleAsync`.
    #[allow(clippy::too_many_arguments)]
    fn try_undo_group_recycle<'a>(
        &mut self,
        action: &Arc<UndoActionRecord>,
        members: &[JournalGroupMember],
        journal: &'a OperationJournal,
        clock: &'a dyn Clock,
        tx: &mut Option<JournalTransaction<'a>>,
        restored: &mut Vec<String>,
        restored_by_this_undo: &mut usize,
    ) -> io::Result<UndoResult> {
        let mut pending: Vec<&JournalGroupMember> = Vec::new();
        let mut unrecoverable: Vec<JournalGroupMember> = Vec::new();
        for member in members {
            // A member deleted permanently cannot be restored; the members that reached the bin still can.
            if member.permanent {
                unrecoverable.push(member.clone());
                continue;
            }
            let Some(existing) = self.stat(&member.source) else {
                pending.push(member);
                continue;
            };
            // Something is at the original path: "already restored" only when it is the recycled file itself.
            if i64::try_from(existing.length) != Ok(member.size)
                || existing.last_write_utc != member.last_write_utc.value
            {
                return Err(Self::plain_error(
                    self.texts
                        .core_undo_recycle_target_exists(get_file_name(&member.source)),
                ));
            }
            restored.push(member.source.clone());
        }
        if pending.is_empty() {
            self.last_undo_action = None;
            if self.resolve_partial_undo(action) {
                self.settle_failed_delete_line(action, restored);
                return Ok(self.group_recycle_success(action, members, restored, &unrecoverable));
            }
            // The failure path settles the Failed Delete line for members the user put back by hand.
            return Err(Self::plain_error(self.texts.core_recovery_already_handled()));
        }
        let undo_members: Vec<JournalGroupMember> =
            members.iter().filter(|member| !member.permanent).cloned().collect();
        let Some(first) = undo_members.first() else {
            return Err(Self::plain_error(self.texts.core_recovery_already_handled()));
        };
        let id = new_guid_n();
        let group_id = new_guid_n();
        let prepared = JournalEntry {
            id,
            op_type: FileOperationType::Recycle,
            state: JournalState::Prepared,
            source: first.source.clone(),
            destination: None,
            size: first.size,
            last_write_utc: first.last_write_utc,
            timestamp_utc: JournalDateTime::utc(clock.utc_now()),
            error: None,
            error_code: None,
            permanent: None,
            undo: Some(true),
            group_id: Some(group_id),
            group_members: Some(undo_members.clone()),
        };
        let tx = tx.insert(JournalTransaction::new(journal, clock, prepared, None));
        tx.begin()?;
        for member in pending {
            let restore_failed = || {
                Self::plain_error(
                    self.texts
                        .core_undo_recycle_restore_failed(get_file_name(&member.source)),
                )
            };
            // P-13: an `Err` from the bin is the C# throw and fails the undo with its message.
            if !self.recycle_bin.try_restore(
                Path::new(&member.source),
                u64::try_from(member.size).unwrap_or(0),
                member.last_write_utc.value,
            )? {
                return Err(restore_failed());
            }
            // R18: the recycled file itself must be back (size and write time), not merely some file at the path.
            let back = self.stat(&member.source).is_some_and(|back| {
                i64::try_from(back.length) == Ok(member.size) && back.last_write_utc == member.last_write_utc.value
            });
            if !back {
                return Err(restore_failed());
            }
            restored.push(member.source.clone());
            *restored_by_this_undo += 1;
        }
        tx.mark_mutation_completed();
        let _ = tx.commit();
        let _ = self.resolve_partial_undo(action);
        self.settle_failed_delete_line(action, restored);
        self.last_undo_action = None;
        // Partly restorable capture: success for what came back, plus a note naming what cannot.
        Ok(self.group_recycle_success(action, members, restored, &unrecoverable))
    }

    /// The success result of a group Recycle undo: restored member sources in member order, plus the note.
    fn group_recycle_success(
        &self,
        action: &UndoActionRecord,
        members: &[JournalGroupMember],
        restored: &[String],
        unrecoverable: &[JournalGroupMember],
    ) -> UndoResult {
        UndoResult {
            restored_paths: Some(restored_in_order(members, restored)),
            ..UndoResult::new(
                true,
                Some(FileOperationType::Recycle),
                action.source.clone(),
                None,
                unrecoverable_note(self.texts.as_ref(), members, restored, unrecoverable),
            )
        }
    }

    /// C# `RememberPartialUndo(action, restoredByThisUndo, failedEntry)`.
    fn remember_partial_undo(
        &mut self,
        action: &Arc<UndoActionRecord>,
        restored_by_this_undo: usize,
        failed_entry: Option<JournalEntry>,
    ) {
        let earlier = self
            .partial_group_undo
            .take_if(|partial| Arc::ptr_eq(&partial.action, action));
        if restored_by_this_undo == 0 && earlier.is_none() {
            return;
        }
        let mut entries = earlier.map(|partial| partial.failed_entries).unwrap_or_default();
        entries.extend(failed_entry);
        self.partial_group_undo = Some(PartialGroupUndo {
            action: Arc::clone(action),
            failed_entries: entries,
        });
    }

    /// C# `ResolvePartialUndo(action)`: ALWAYS clears the remembered partial undo; `true` (after closing its Failed
    /// lines, best effort) only when it belonged to `action`.
    fn resolve_partial_undo(&mut self, action: &Arc<UndoActionRecord>) -> bool {
        let Some(partial) = self.partial_group_undo.take() else {
            return false;
        };
        if !Arc::ptr_eq(&partial.action, action) {
            return false;
        }
        for failed in &partial.failed_entries {
            let committed = JournalEntry {
                state: JournalState::Committed,
                timestamp_utc: JournalDateTime::utc(self.clock.utc_now()),
                error: None,
                error_code: None,
                ..failed.clone()
            };
            // Best effort: a journal that cannot be written only leaves the old Recovery item to be dismissed.
            let _ = self.journal.append_if_unchanged_since(failed, &[], &committed);
        }
        true
    }

    /// C# `SettleFailedDeleteLine(action, restored)` (best effort, never fails the undo).
    fn settle_failed_delete_line(&self, action: &UndoActionRecord, restored: &[String]) {
        // Every error is swallowed: the undo itself is complete (C# `IsNonCritical`).
        let _ = self.try_settle_failed_delete_line(action, restored);
    }

    /// The C# `try` block of `SettleFailedDeleteLine`.
    fn try_settle_failed_delete_line(&self, action: &UndoActionRecord, restored: &[String]) -> io::Result<()> {
        let Some(anchor) = action
            .current_failed_entry()
            .filter(|anchor| anchor.state == JournalState::Failed)
        else {
            return Ok(());
        };
        if self.try_settle_from(action, &anchor, restored)? {
            return Ok(());
        }
        // Another writer appended a line for this Id since our last one: re-anchor only on a newer Failed line over a
        // subset of this action's members.
        let Some(latest) = self.journal.read_latest_entry(&anchor.id)? else {
            return Ok(());
        };
        let Some(latest_members) = latest
            .group_members
            .as_ref()
            .filter(|members| latest.state == JournalState::Failed && !members.is_empty())
        else {
            return Ok(());
        };
        if latest == anchor {
            return Ok(());
        }
        let Some(original) = action
            .failed_entry
            .as_ref()
            .and_then(|entry| entry.group_members.as_ref())
            .filter(|members| !members.is_empty())
        else {
            return Ok(());
        };
        let subset = latest_members.iter().all(|member| {
            original
                .iter()
                .any(|original_member| eq_ignore_case(&original_member.source, &member.source))
        });
        if !subset {
            return Ok(());
        }
        self.try_settle_from(action, &latest, restored)?;
        Ok(())
    }

    /// C# `TrySettleFrom(action, anchor, restored)`.
    fn try_settle_from(
        &self,
        action: &UndoActionRecord,
        anchor: &JournalEntry,
        restored: &[String],
    ) -> io::Result<bool> {
        let Some(members) = anchor.group_members.as_ref().filter(|members| !members.is_empty()) else {
            return Ok(false);
        };
        let remaining: Vec<JournalGroupMember> = members
            .iter()
            .filter(|member| !restored.iter().any(|path| eq_ignore_case(path, &member.source)))
            .cloned()
            .collect();
        let timestamp_utc = JournalDateTime::utc(self.clock.utc_now());
        let replacement = match remaining.first() {
            None => JournalEntry {
                state: JournalState::Dismissed,
                timestamp_utc,
                error: None,
                error_code: None,
                ..anchor.clone()
            },
            Some(first) => JournalEntry {
                timestamp_utc,
                source: first.source.clone(),
                destination: first.destination.clone(),
                size: first.size,
                last_write_utc: first.last_write_utc,
                group_members: Some(remaining.clone()),
                ..anchor.clone()
            },
        };
        if !self.journal.append_if_unchanged_since(anchor, &[], &replacement)? {
            return Ok(false);
        }
        action.set_current_failed_entry(replacement);
        Ok(true)
    }

    /// C# `DropGroupUndo(members)`: forgets every member destination's fingerprint and the last action.
    fn drop_group_undo(&mut self, members: &[JournalGroupMember]) {
        for member in members {
            if let Some(destination) = &member.destination {
                self.move_fingerprints.remove(&ignore_case_key(destination));
            }
        }
        self.last_undo_action = None;
    }
}

impl UndoActionRecord {
    /// C# `new UndoActionRecord(...)` with `CurrentFailedEntry = FailedEntry`.
    #[allow(clippy::too_many_arguments)]
    fn new(
        operation: FileOperationType,
        source: String,
        destination: Option<String>,
        size: i64,
        last_write_utc: DateTimeUtc,
        permanent: bool,
        group_members: Option<Vec<JournalGroupMember>>,
        failed_entry: Option<JournalEntry>,
    ) -> Self {
        Self {
            operation,
            source,
            destination,
            size,
            last_write_utc,
            permanent,
            group_members,
            current_failed_entry: Mutex::new(failed_entry.clone()),
            failed_entry,
        }
    }
}

/// C# `RestoredInOrder(members, restored)`: the member sources that are in `restored` (OrdinalIgnoreCase), in MEMBER
/// order, each as the member spells it.
fn restored_in_order(members: &[JournalGroupMember], restored: &[String]) -> Vec<String> {
    members
        .iter()
        .filter(|member| restored.iter().any(|path| eq_ignore_case(path, &member.source)))
        .map(|member| member.source.clone())
        .collect()
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
    if unrecoverable.is_empty() {
        return None;
    }
    let names = unrecoverable
        .iter()
        .map(|member| get_file_name(&member.source))
        .collect::<Vec<_>>()
        .join(", ");
    Some(texts.core_undo_group_partially_restored(restored_in_order(members, restored).len(), members.len(), &names))
}
