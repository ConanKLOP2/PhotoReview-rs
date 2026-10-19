//! Port of `PhotoReview.App.Shared/Coordinators/FileActionController.cs` lines 529 to 590 (remembered Move positions,
//! `InsertRestoredMove`, `SortedInsertIndex`) and 619 to 721 (`UndoLastAsync`) (contract `contracts/W8-07d.md`).
//! Target: `coordinators::file_actions::undo`.
//!
//! `UndoService::undo_last` blocks (file moves), so it runs on `WorkerLane::FileAction`; the result handling and the three
//! `await _sink.PresentAsync(..)` sites of the C# method run on the UI thread, the awaits become parked continuations
//! ([`UndoTail`]) resumed by `on_present_finished`.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]
// Stub only (contract text in doc comments, boxed seams): the porter fixes the doc layout / introduces type aliases and removes this line.
#![allow(clippy::enum_variant_names)]

use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::types::UndoResult;

use super::recycle::{FileActionController, FileActionCx, FileActionStep, UndoOutcome};

/// C# `MaxRememberedMovePositions`: when the map reaches this size it is cleared before the next insert.
pub const MAX_REMEMBERED_MOVE_POSITIONS: usize = 256;

/// C# `MovePosition(PreviousPath, Group)`: where a moved/recycled photo sat. `previous_path == None` means "it was first".
#[derive(Debug, Clone, PartialEq)]
pub struct MovePosition {
    pub previous_path: Option<String>,
    pub group: Option<CaptureGroup>,
}

/// What an undo remembers while the worker runs it.
#[derive(Debug)]
pub(super) struct InFlightUndo {
    pub ticket: u64,
    /// C# `folderGen`, read BEFORE `undo_last` started.
    pub folder_gen: i64,
    /// The `currentFolder` argument.
    pub current_folder: Option<String>,
}

/// The continuation after an undo's present, one per C# `await _sink.PresentAsync` site. Each first checks
/// `is_folder_current(folder_gen)` and returns the result without writing a session path or status when the folder changed.
#[derive(Debug)]
pub(super) enum UndoTail {
    /// Lines 644 to 654: a failed undo restored members into an EMPTY catalog and presented the restored entry.
    FailedPartialPresented { result: UndoResult },
    /// Lines 689 to 697 and 718 to 720: a Recycle undone in its own folder into an empty catalog.
    RecyclePresented { result: UndoResult },
    /// Lines 709 to 720: a Move undone in its own folder.
    MovePresented { result: UndoResult },
}

impl FileActionController {
    /// C# `UndoLastAsync(currentFolder)`. `None` undo service -> `Done(Undone(None))`. Otherwise `folder_gen =
    /// clock.current_folder()`, submit `undo_service.lock().undo_last()` on `WorkerLane::FileAction`, return `Pending`
    /// (a poisoned lock is recovered with `PoisonError::into_inner`, R0003).
    pub fn undo_last(&mut self, cx: &mut FileActionCx<'_>, current_folder: Option<&str>) -> FileActionStep {
        todo!()
    }

    /// C# lines 627 to 720 for one `Undone` event: failed undo (partial restore into the catalog, status
    /// `error_message` or `StatusNothingToUndo`), stale folder (return the result untouched), restores outside the folder
    /// (`take_move_position`; session path + `on_catalog_changed(None)` only when the SOURCE is in the folder), Recycle undone in
    /// place (`insert_restored_move`, present only when the catalog was empty, else `preload.preload_around`), Move undone in
    /// place (`insert_restored_move`, present the restored entry); finally `error_message` as status and
    /// `sink.notify_navigation_state_changed()`. A cancelled undo ends `Done(Canceled)` without touching anything.
    pub(super) fn on_undone(&mut self, cx: &mut FileActionCx<'_>, ticket: u64, outcome: UndoOutcome) -> FileActionStep {
        todo!()
    }

    /// Resumes a parked undo continuation.
    pub(super) fn run_undo_tail(&mut self, cx: &mut FileActionCx<'_>, tail: UndoTail) -> FileActionStep {
        todo!()
    }

    /// C# `RememberMovePosition(source, previousPath, group)`: clears the map when it holds `MAX_REMEMBERED_MOVE_POSITIONS`
    /// entries, then stores the position under `ignore_case_key(source)` and, for a capture, under EVERY member path.
    pub(super) fn remember_move_position(
        &mut self,
        source: &str,
        previous_path: Option<&str>,
        group: Option<&CaptureGroup>,
    ) {
        todo!()
    }

    /// C# `InsertRestoredMove(source, restoredPaths, keepPosition)`: index 0 when the remembered predecessor is `None`; the index
    /// after the predecessor when it is still listed; else `sorted_insert_index(source)`. `restored_paths` non-empty ->
    /// `catalog.restore_members(paths, index, position.group, settings.raw_support_enabled)`, else `catalog.restore(source, index)`.
    /// `keep_position` peeks instead of taking the remembered position (a partly failed undo is retried).
    pub(super) fn insert_restored_move(
        &mut self,
        cx: &mut FileActionCx<'_>,
        source: &str,
        restored_paths: Option<&[String]>,
        keep_position: bool,
    ) {
        todo!()
    }

    /// C# `PeekMovePosition`.
    pub(super) fn peek_move_position(&self, source: &str) -> Option<MovePosition> {
        todo!()
    }

    /// C# `TakeMovePosition`: removes the entry and, for a capture, the entries of every member path.
    pub(super) fn take_move_position(&mut self, source: &str) -> Option<MovePosition> {
        todo!()
    }

    /// C# `SortedInsertIndex(path)`: the first catalog index whose file name is NOT less than the file name of `path`
    /// under `natural_comparer.compare(Some(a), Some(b))` (file names via `get_file_name`).
    pub(super) fn sorted_insert_index(&self, cx: &FileActionCx<'_>, path: &str) -> i32 {
        todo!()
    }
}
