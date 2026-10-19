//! Port of `PhotoReview.App.Shared/Coordinators/FileActionController.cs` lines 68 to 116 (`RunActionAsync`, the action
//! profiles) and 289 to 527 plus 592 to 617 (the second half of `ExecuteFileActionCoreAsync` and `ReportLateCompletion`)
//! (contract `contracts/W8-07d.md`). Target: `coordinators::file_actions::actions`.
//!
//! The C# core is one `async` method with six `await` points. Here it is cut at those points: `execute_core` runs up to the
//! executor call (INV-3: the next photo is requested BEFORE the I/O), the worker runs the executor, `handle_event` runs the
//! result handling, and the `await presentTask` points become a parked continuation ([`ParkedTail`]) that
//! `on_present_finished` resumes (RV-A09, R08).
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::types::FileActionResult;
use pr_core::model::FileOperationType;

use super::move_copy::MoveCopyTail;
use super::recycle::{FileActionController, FileActionCx, FileActionEvent, FileActionStep, PreparedAction};
use super::undo::UndoTail;

/// What the core remembers between the executor call and its result (the C# locals that live across `await`).
#[derive(Debug)]
pub(super) struct InFlightAction {
    pub ticket: u64,
    pub action_name: String,
    pub operation: FileOperationType,
    pub destination: Option<String>,
    pub source: String,
    /// C# `sourceIndex` (catalog index of `source` before the removal; -1 when it was not listed).
    pub source_index: i32,
    /// C# `previousPath`: the review-order neighbour before the source (Move/Recycle only).
    pub previous_path: Option<String>,
    pub group: Option<CaptureGroup>,
    /// C# `folderGen`, read AFTER `stop_for_action` (so the stale-folder guard compares with the folder the action ran in).
    pub folder_gen: i64,
    /// Move or Recycle (the entry leaves the catalog).
    pub is_remove: bool,
    pub after: AfterCore,
}

/// What runs when the core finishes (the C# caller of `ExecuteFileActionCoreAsync`).
#[derive(Debug)]
pub(super) enum AfterCore {
    /// `RunActionAsync` / `RecycleAsync`: the bool is the entry point result.
    Nothing,
    /// `MoveOrCopyToFolderAsync` (W8-07e): remember the folder, wait for the present, write the final status.
    MoveCopyTo(MoveCopyTail),
}

/// A continuation of the core that C# runs after `await presentTask`. Each variant is one `await` site of the C# core.
#[derive(Debug)]
pub(super) enum CoreTail {
    /// Lines 397 to 403: a partner file vanished; status `StatusGroupPartnerMissing(..)` (prefixed by `AllImagesProcessed + " "`
    /// when a remove left the catalog empty), only while the folder is still current. Result `Finished{succeeded: true}`.
    PartnerMissing { message: String },
    /// Lines 420 to 428: Move failed verification after the source was removed: if still current, `update_session_path`
    /// then status `StatusMoveUnverified(fileName)`. Result `Finished{false}`.
    UnverifiedMove { file_name: String },
    /// Lines 457 to 473: failure after a (maybe faulted) present: if still current, status `ActionFailed(name, failure)` and,
    /// for a Recycle that completed no member, the `DialogRecycleFailed*` error dialog. Result `Finished{false}`.
    Failure {
        status: String,
        recycle_failure_reason: Option<String>,
    },
    /// Lines 485 to 496: the action succeeded but its bookkeeping threw: if still current, status `ActionCompleted(name)`.
    /// Result `Finished{true}` while current, else `Finished{false}`.
    CompletedAfterBookkeepingFailure { status: String },
    /// Lines 510 to 521: the executor threw (the entry was restored): if still current, status `ActionFailed(name, text)`.
    ExecutorThrew {
        status: String,
        recycle_failure_reason: Option<String>,
    },
    /// `MoveOrCopyToFolderAsync` lines 799 to 813 (W8-07e), run when the core returned true and a present is outstanding.
    MoveCopyFinal(MoveCopyTail),
}

/// The continuation parked while a present runs (at most one at a time: the caller's gate serializes operations).
#[derive(Debug)]
pub(super) enum ParkedTail {
    Core(CoreTail),
    /// Undo awaits the present at three places (W8-07d `undo.rs`).
    Undo(UndoTail),
}

impl FileActionController {
    /// C# `RunActionAsync(index, compareSelectedPath, currentPath)`. Returns `Done(Finished{false})` (C# `return`) when:
    /// the catalog is empty; `index` is outside the settings' action list; and (after the status
    /// `StatusActionInvalidOperation(name)`) the operation is not one of Move/Copy/Recycle; a Recycle that
    /// `refuse_recycle_without_bin`; the generic confirmation (`action.confirm`, a dialog service, no permanent and no group
    /// prompt coming) is declined or the state changed during it; the source is empty; a Move/Copy has a blank destination
    /// (status `StatusActionNoDestination(name)`). Otherwise the core runs (`Recycle` ignores the destination).
    pub fn run_action(
        &mut self,
        cx: &mut FileActionCx<'_>,
        index: i32,
        compare_selected_path: Option<&str>,
        current_path: Option<&str>,
    ) -> FileActionStep {
        todo!()
    }

    /// C# `ExecuteFileActionCoreAsync` from `sourceIndex` on (lines 278 to 330) after `prepare_core` returned `Ready`:
    /// 1. `isRemove` -> `clock.stop_for_action()` and `preload.cancel()` (a Copy does neither).
    /// 2. `folder_gen = clock.current_folder()`; remember it as `last_action_folder_gen`.
    /// 3. `previous_path` = the catalog neighbour before the source (remove only).
    /// 4. remove only: `next = catalog.remove(source)`, `sink.on_catalog_changed(Some(source))`, evict the other members of a
    ///    capture (`sink.evict_cached_paths`), then `sink.present(next)` (state `Running`) or, when nothing is left, status
    ///    `AllImagesProcessed` (INV-3: BEFORE the I/O).
    /// 5. submit the executor call on `WorkerLane::FileAction` with `CancelToken::none()`; returns `Pending`.
    pub(super) fn execute_core(
        &mut self,
        cx: &mut FileActionCx<'_>,
        prepared: PreparedAction,
        after: AfterCore,
    ) -> FileActionStep {
        todo!()
    }

    /// Delivers a worker result (UI thread). A ticket other than the in-flight one is dropped (returns `Pending`).
    /// `Executed` -> `on_executed` (this file); `Undone` -> `on_undone` (`undo.rs`).
    pub fn handle_event(&mut self, cx: &mut FileActionCx<'_>, event: FileActionEvent) -> FileActionStep {
        todo!()
    }

    /// The shell reports that the `sink.present(..)` this controller requested has finished (`faulted` = the present failed;
    /// a fault is logged, never shown). Resumes the parked continuation when there is one; otherwise only records the state.
    pub fn on_present_finished(&mut self, cx: &mut FileActionCx<'_>, faulted: bool) -> FileActionStep {
        todo!()
    }

    /// C# lines 321 to 522, the part after `await _fileActions.Execute..`: the stale-folder guard (late completion: register the
    /// result for undo, `ReportLateCompletion`/group texts, no catalog change, `Done(false)`); success (register undo, remember
    /// the move position, `update_session_path`, partner-missing status, `AllImagesProcessed`/`ActionCompleted`/`CopiedTo`);
    /// unverified Move (F3); failure (INV-5: `catalog.restore` / `restore_members`, `sink.on_catalog_changed(None)` for a group);
    /// executor panic (restore only when the folder is still current and the action had not succeeded);
    /// `finally`: `sink.notify_navigation_state_changed()` on every path that ends the core.
    pub(super) fn on_executed(
        &mut self,
        cx: &mut FileActionCx<'_>,
        ticket: u64,
        outcome: super::recycle::ExecOutcome,
    ) -> FileActionStep {
        todo!()
    }

    /// Runs a parked core continuation (after the present finished or when it had already finished). Every tail first checks
    /// `clock.is_folder_current(folder_gen)` (R08) before it writes a session path or a status into the (maybe new) folder.
    pub(super) fn run_core_tail(&mut self, cx: &mut FileActionCx<'_>, tail: CoreTail) -> FileActionStep {
        todo!()
    }

    /// C# `ReportLateCompletion(result)` (lines 599 to 617): a single-file Move/Recycle that finished after the folder changed.
    /// `rejected` results are ignored; otherwise `undo.register(result)` and `sink.show_late_action_status` with
    /// `StatusLateMoveUndoable(fileName, directory of destination)` (Move with a destination), `StatusLateDeletedPermanently`
    /// (permanent Recycle) or `StatusLateRecycleUndoable`. Copy is silent.
    pub(super) fn report_late_completion(&mut self, cx: &mut FileActionCx<'_>, result: &FileActionResult) {
        todo!()
    }
}
