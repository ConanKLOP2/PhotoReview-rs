//! Port of `PhotoReview.App.Shared/Coordinators/FileActionController.cs` lines 529 to 590 (remembered Move positions,
//! `InsertRestoredMove`, `SortedInsertIndex`) and 619 to 721 (`UndoLastAsync`) (contract `contracts/W8-07d.md`).
//! Target: `coordinators::file_actions::undo`.
//!
//! `UndoService::undo_last` blocks (file moves), so it runs on `WorkerLane::FileAction`; the result handling and the three
//! `await _sink.PresentAsync(..)` sites of the C# method run on the UI thread, the awaits become parked continuations
//! ([`UndoTail`]) resumed by `on_present_finished`.
//! W9-R5 adds C# `7eaee2a2` rename-Undo detection (878-897) and its same-folder re-key continuation (725-782).
// The three variants all end in `Presented` because each names the present they wait for.
#![allow(clippy::enum_variant_names)]

use std::cmp::{Ordering, max};
use std::panic::{AssertUnwindSafe, catch_unwind};

use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::types::UndoResult;
use pr_core::model::FileOperationType;
use pr_core::text::dotnet_path::get_file_name;
use pr_core::text::ignore_case_key;
use pr_l10n::keys;

use super::actions::{ParkedTail, describe_panic};
use super::recycle::{
    FileActionController, FileActionCx, FileActionDone, FileActionEvent, FileActionStep, PresentState, UndoOutcome,
};
use crate::coordinators::folder_load::state::WorkerLane;

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
    pub rename_identity: Option<pr_core::file_actions::undo::UndoActionIdentity>,
}

/// The continuation after an undo's present, one per C# `await _sink.PresentAsync` site. Each first checks
/// `is_folder_current(folder_gen)` and returns the result without writing a session path or status when the folder changed.
#[derive(Debug)]
pub(super) enum UndoTail {
    /// Lines 644 to 654: a failed undo restored members into an EMPTY catalog and presented the restored entry.
    FailedPartialPresented { folder_gen: i64, result: UndoResult },
    /// Lines 689 to 697 and 718 to 720: a Recycle undone in its own folder into an empty catalog.
    RecyclePresented { folder_gen: i64, result: UndoResult },
    /// Lines 709 to 720: a Move undone in its own folder.
    MovePresented { folder_gen: i64, result: UndoResult },
}

fn undone(result: UndoResult) -> FileActionStep {
    FileActionStep::Done(FileActionDone::Undone(Some(result)))
}

impl FileActionController {
    /// C# `UndoLastAsync(currentFolder)`. `None` undo service -> `Done(Undone(None))`. Otherwise `folder_gen =
    /// clock.current_folder()`, submit `undo_service.lock().undo_last()` on `WorkerLane::FileAction`, return `Pending`
    /// (a poisoned lock is recovered with `PoisonError::into_inner`, R0003).
    pub fn undo_last(&mut self, _cx: &mut FileActionCx<'_>, current_folder: Option<&str>) -> FileActionStep {
        let Some(undo) = self.deps.undo_service.clone() else {
            return FileActionStep::Done(FileActionDone::Undone(None));
        };
        let rename_identity = {
            let guard = undo.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let current = guard.last_undo_action_identity();
            let is_rename = self
                .last_rename_undo_action
                .as_ref()
                .zip(current.as_ref())
                .is_some_and(|(a, b)| a == b);
            if is_rename && !guard.is_busy() {
                self.deps.clock.stop_for_action();
                if let Some(preload) = &self.deps.preload_controller {
                    preload.cancel();
                }
                current
            } else {
                None
            }
        };
        let folder_gen = self.deps.clock.current_folder();
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.undo_in_flight = Some(InFlightUndo {
            ticket,
            folder_gen,
            current_folder: current_folder.map(str::to_string),
            rename_identity,
        });
        let job = Box::new(move || {
            let run = catch_unwind(AssertUnwindSafe(|| {
                let mut guard = undo.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.undo_last()
            }));
            let outcome = match run {
                Ok(result) => UndoOutcome::Result(result),
                Err(payload) => UndoOutcome::Panicked(describe_panic(payload.as_ref())),
            };
            FileActionEvent::Undone { ticket, outcome }
        });
        self.deps.worker.submit(WorkerLane::FileAction, job);
        FileActionStep::Pending
    }

    /// C# lines 627 to 720 for one `Undone` event: failed undo (partial restore into the catalog, status
    /// `error_message` or `StatusNothingToUndo`), stale folder (return the result untouched), restores outside the folder
    /// (`take_move_position`; session path + `on_catalog_changed(None)` only when the SOURCE is in the folder), Recycle undone in
    /// place (`insert_restored_move`, present only when the catalog was empty, else `preload.preload_around`), Move undone in
    /// place (`insert_restored_move`, present the restored entry); finally `error_message` as status and
    /// `sink.notify_navigation_state_changed()`. A cancelled undo ends `Done(Canceled)` without touching anything.
    pub(super) fn on_undone(&mut self, cx: &mut FileActionCx<'_>, ticket: u64, outcome: UndoOutcome) -> FileActionStep {
        if !self.undo_in_flight.as_ref().is_some_and(|f| f.ticket == ticket) {
            return FileActionStep::Pending; // INV-9: a result of an older operation
        }
        let Some(flight) = self.undo_in_flight.take() else {
            return FileActionStep::Pending;
        };
        let result = match outcome {
            UndoOutcome::Result(result) => result,
            UndoOutcome::Canceled => return FileActionStep::Done(FileActionDone::Canceled),
            UndoOutcome::Panicked(text) => {
                self.deps.log.error(&format!("Undo threw: {text}"), None);
                UndoResult::new(false, None, "", None, Some(text))
            }
        };
        let folder_gen = flight.folder_gen;
        let current_folder = flight.current_folder.as_deref();

        let rename_undo = flight.rename_identity.is_some();
        if rename_undo && !self.deps.clock.is_folder_current(folder_gen) {
            return undone(result);
        }
        let in_folder = !result.source.is_empty() && Self::is_in_folder(&result.source, current_folder);
        // C# TryHandleRenameUndoAsync returns false for a successful restore outside this folder;
        // continue through the generic path below so its move-position/session handling still runs.
        if rename_undo && (in_folder || !result.succeeded) {
            let mut renames = pr_core::file_actions::rename::PathKeyMap::new();
            if result.succeeded && result.restored_paths.as_ref().is_none_or(Vec::is_empty) {
                if let (Some(new_path), source) = (result.destination.as_deref(), result.source.as_str()) {
                    if !new_path.is_empty() && !source.is_empty() {
                        renames.insert(new_path.to_owned(), source.to_owned());
                    }
                }
            } else if let Some(restored) = result.restored_paths.as_ref() {
                for old_path in restored {
                    if let Some(new_path) = self.renamed_to.get(old_path) {
                        renames.insert(new_path.clone(), old_path.clone());
                    }
                }
            }
            let _ = self.apply_rename_undo(cx, &renames, in_folder);
            if self.present == PresentState::Running {
                self.parked = Some(ParkedTail::Rename(super::actions::RenameTail {
                    folder_gen,
                    late_result: None,
                    done: Some(FileActionDone::Undone(Some(result.clone()))),
                    result: crate::coordinators::rename::FileRenameResult {
                        status: if result.succeeded {
                            crate::coordinators::rename::FileRenameStatus::Renamed
                        } else {
                            crate::coordinators::rename::FileRenameStatus::Failed
                        },
                        new_path: Some(result.source.clone()),
                        problem: pr_core::file_actions::rename::RenameNameProblem::None,
                        invalid_character: None,
                        error: result.error_message.clone(),
                    },
                    status_text: Some(if !result.succeeded {
                        result
                            .error_message
                            .clone()
                            .unwrap_or_else(|| self.deps.texts.get(keys::STATUS_NOTHING_TO_UNDO))
                    } else if let Some(message) = result.error_message.clone().filter(|m| !m.is_empty()) {
                        message
                    } else {
                        self.deps.texts.format(
                            "status.renameUndone",
                            &[pr_l10n::template::LocArg::new(
                                "fileName",
                                get_file_name(&result.source).to_owned(),
                            )],
                        )
                    }),
                }));
                return FileActionStep::Pending;
            }
            let text = if !result.succeeded {
                result
                    .error_message
                    .clone()
                    .unwrap_or_else(|| self.deps.texts.get(keys::STATUS_NOTHING_TO_UNDO))
            } else if let Some(message) = result.error_message.clone().filter(|m| !m.is_empty()) {
                message
            } else {
                self.deps.texts.format(
                    "status.renameUndone",
                    &[pr_l10n::template::LocArg::new(
                        "fileName",
                        get_file_name(&result.source).to_owned(),
                    )],
                )
            };
            cx.sink.set_status_text(&text);
            cx.sink.notify_navigation_state_changed();
            return undone(result);
        }

        if !result.succeeded {
            // A capture undo that failed half way still put some members back: show them now instead of at the next reload.
            if self.deps.clock.is_folder_current(folder_gen)
                && let Some(partial) = result.restored_paths.as_ref().filter(|p| !p.is_empty())
                && !result.source.is_empty()
                && Self::is_in_folder(&result.source, current_folder)
            {
                let in_folder: Vec<String> = partial
                    .iter()
                    .filter(|p| Self::is_in_folder(p, current_folder))
                    .cloned()
                    .collect();
                if !in_folder.is_empty() {
                    // Move and Recycle both go back to their remembered review position. When the catalog was empty nothing is
                    // on screen: present the restored entry, otherwise keep showing the current photo.
                    let was_empty = cx.catalog.count() == 0;
                    self.insert_restored_move(cx, &result.source, Some(&in_folder), true);
                    cx.sink.on_catalog_changed(None);
                    if was_empty && cx.catalog.count() > 0 {
                        cx.sink.present(max(cx.catalog.current_index(), 0));
                        self.present = PresentState::Running;
                        self.parked = Some(ParkedTail::Undo(UndoTail::FailedPartialPresented {
                            folder_gen,
                            result,
                        }));
                        return FileActionStep::Pending;
                    }
                    self.failed_partial_session(cx, &result);
                }
            }
            return self.failed_status(cx, folder_gen, result);
        }

        if !self.deps.clock.is_folder_current(folder_gen) {
            return undone(result);
        }

        // R7-2: a Move/Recycle made in another folder is restored there, not into this folder's catalog; the caller opens that
        // folder at the restored file (see `restores_outside_folder`).
        if Self::restores_outside_folder(Some(&result), current_folder) {
            // The caller reloads the folder, which rebuilds the catalog from disk and presents the restored photo once.
            self.take_move_position(&result.source);
            if Self::is_in_folder(&result.source, current_folder) {
                cx.sink.on_catalog_changed(None);
                cx.sink.update_session_path(&result.source);
            }
        } else if result.operation == Some(FileOperationType::Recycle)
            && !result.source.is_empty()
            && Self::is_in_folder(&result.source, current_folder)
        {
            // A Recycle undone in the folder it was made in: the photo goes back to its remembered review position without
            // reloading, and the photo on screen stays the one the user is reviewing. Only an empty catalog presents.
            let restored = self.in_folder_paths(&result, current_folder);
            let was_empty = cx.catalog.count() == 0;
            self.insert_restored_move(cx, &result.source, restored.as_deref(), false);
            cx.sink.on_catalog_changed(None);
            if was_empty && cx.catalog.count() > 0 {
                let reload = Self::reload_path_after_undo(&result).unwrap_or_else(|| result.source.clone());
                let restored_index = cx.catalog.index_of(&reload);
                cx.sink.present(if restored_index >= 0 {
                    restored_index
                } else {
                    max(cx.catalog.current_index(), 0)
                });
                self.present = PresentState::Running;
                self.parked = Some(ParkedTail::Undo(UndoTail::RecyclePresented { folder_gen, result }));
                return FileActionStep::Pending;
            }
            // The neighbour window shifted by one: warm the restored photo (fire and forget, the handle is dropped).
            if let Some(preload) = &self.deps.preload_controller {
                preload.catalog_changed(cx.catalog);
                drop(preload.preload_around(max(cx.catalog.current_index(), 0)));
            }
            let session = cx
                .catalog
                .current()
                .map_or_else(|| result.source.clone(), |e| e.path().to_string());
            cx.sink.update_session_path(&session);
        } else if result.operation == Some(FileOperationType::Move)
            && !result.source.is_empty()
            && Self::is_in_folder(&result.source, current_folder)
        {
            let restored = self.in_folder_paths(&result, current_folder);
            self.insert_restored_move(cx, &result.source, restored.as_deref(), false);
            cx.sink.on_catalog_changed(None);
            let index = cx.catalog.index_of(&result.source);
            if index >= 0 {
                cx.sink.present(index);
                self.present = PresentState::Running;
                self.parked = Some(ParkedTail::Undo(UndoTail::MovePresented { folder_gen, result }));
                return FileActionStep::Pending;
            }
            cx.sink.update_session_path(&result.source);
        }

        self.finish_undo(cx, result)
    }

    /// The restored paths of a capture undo that are in the current folder (`None` when the result lists none).
    fn in_folder_paths(&self, result: &UndoResult, current_folder: Option<&str>) -> Option<Vec<String>> {
        result.restored_paths.as_ref().filter(|p| !p.is_empty()).map(|paths| {
            paths
                .iter()
                .filter(|p| Self::is_in_folder(p, current_folder))
                .cloned()
                .collect()
        })
    }

    /// `UpdateSessionPath(current ?? source)` + `NotifyNavigationStateChanged` of the failed-undo branch.
    fn failed_partial_session(&mut self, cx: &mut FileActionCx<'_>, result: &UndoResult) {
        let session = cx
            .catalog
            .current()
            .map_or_else(|| result.source.clone(), |e| e.path().to_string());
        cx.sink.update_session_path(&session);
        cx.sink.notify_navigation_state_changed();
    }

    /// The end of the failed-undo branch: a status about the old folder must not overwrite the new folder's line.
    fn failed_status(&mut self, cx: &mut FileActionCx<'_>, folder_gen: i64, result: UndoResult) -> FileActionStep {
        if self.deps.clock.is_folder_current(folder_gen) {
            let text = match result.error_message.as_deref() {
                Some(message) => message.to_string(),
                None => self.deps.texts.get(keys::STATUS_NOTHING_TO_UNDO),
            };
            cx.sink.set_status_text(&text);
        }
        undone(result)
    }

    /// The common end of a succeeded undo: a capture restored only in part succeeds with a note, then the navigation refresh.
    fn finish_undo(&mut self, cx: &mut FileActionCx<'_>, result: UndoResult) -> FileActionStep {
        if let Some(message) = result.error_message.as_deref().filter(|m| !m.is_empty()) {
            cx.sink.set_status_text(message);
        }
        cx.sink.notify_navigation_state_changed();
        undone(result)
    }

    /// Resumes a parked undo continuation.
    pub(super) fn run_undo_tail(&mut self, cx: &mut FileActionCx<'_>, tail: UndoTail) -> FileActionStep {
        match tail {
            UndoTail::FailedPartialPresented { folder_gen, result } => {
                if !self.deps.clock.is_folder_current(folder_gen) {
                    return undone(result);
                }
                self.failed_partial_session(cx, &result);
                self.failed_status(cx, folder_gen, result)
            }
            UndoTail::RecyclePresented { folder_gen, result } => {
                // The present awaited: the folder may have changed meanwhile.
                if !self.deps.clock.is_folder_current(folder_gen) {
                    return undone(result);
                }
                let session = cx
                    .catalog
                    .current()
                    .map_or_else(|| result.source.clone(), |e| e.path().to_string());
                cx.sink.update_session_path(&session);
                self.finish_undo(cx, result)
            }
            UndoTail::MovePresented { folder_gen, result } => {
                if !self.deps.clock.is_folder_current(folder_gen) {
                    return undone(result);
                }
                cx.sink.update_session_path(&result.source);
                self.finish_undo(cx, result)
            }
        }
    }

    /// C# `RememberMovePosition(source, previousPath, group)`: clears the map when it holds `MAX_REMEMBERED_MOVE_POSITIONS`
    /// entries, then stores the position under `ignore_case_key(source)` and, for a capture, under EVERY member path.
    pub(super) fn remember_move_position(
        &mut self,
        source: &str,
        previous_path: Option<&str>,
        group: Option<&CaptureGroup>,
    ) {
        if self.move_positions.len() >= MAX_REMEMBERED_MOVE_POSITIONS {
            self.move_positions.clear();
        }
        let position = MovePosition {
            previous_path: previous_path.map(str::to_string),
            group: group.cloned(),
        };
        self.move_positions.insert(ignore_case_key(source), position.clone());
        if let Some(group) = group {
            for path in group.paths() {
                self.move_positions.insert(ignore_case_key(path), position.clone());
            }
        }
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
        let position = if keep_position {
            self.peek_move_position(source)
        } else {
            self.take_move_position(source)
        };
        let index = match &position {
            Some(p) if p.previous_path.is_none() => 0,
            Some(MovePosition {
                previous_path: Some(previous),
                ..
            }) if cx.catalog.index_of(previous) >= 0 => cx.catalog.index_of(previous) + 1,
            _ => self.sorted_insert_index(cx, source),
        };
        match restored_paths.filter(|p| !p.is_empty()) {
            Some(paths) => {
                let raw = (self.deps.get_settings)().raw_support_enabled;
                cx.catalog
                    .restore_members(paths, index, position.as_ref().and_then(|p| p.group.as_ref()), raw);
            }
            None => {
                cx.catalog.restore(source, index);
            }
        }
    }

    /// C# `PeekMovePosition`.
    pub(super) fn peek_move_position(&self, source: &str) -> Option<MovePosition> {
        self.move_positions.get(&ignore_case_key(source)).cloned()
    }

    /// C# `TakeMovePosition`: removes the entry and, for a capture, the entries of every member path.
    pub(super) fn take_move_position(&mut self, source: &str) -> Option<MovePosition> {
        let position = self.move_positions.remove(&ignore_case_key(source))?;
        if let Some(group) = &position.group {
            for path in group.paths() {
                self.move_positions.remove(&ignore_case_key(path));
            }
        }
        Some(position)
    }

    /// C# `SortedInsertIndex(path)`: the first catalog index whose file name is NOT less than the file name of `path`
    /// under `natural_comparer.compare(Some(a), Some(b))` (file names via `get_file_name`).
    pub(super) fn sorted_insert_index(&self, cx: &FileActionCx<'_>, path: &str) -> i32 {
        let wanted = get_file_name(path);
        let mut index = 0;
        while index < cx.catalog.count()
            && self
                .deps
                .natural_comparer
                .compare(Some(get_file_name(cx.catalog.path_at(index))), Some(wanted))
                == Ordering::Less
        {
            index += 1;
        }
        i32::try_from(index).unwrap_or(i32::MAX)
    }
}
