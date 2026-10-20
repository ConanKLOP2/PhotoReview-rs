//! Port of `PhotoReview.App.Shared/ViewModels/MainViewModel.cs` lines 565 to 854 (baseline `d92d6aae`): `RunActionAsync`,
//! `WaitForQueuedTurnAsync`, `RecycleAsync`, the file-action gate members, `UndoAsync` / `UndoCoreAsync`, the zoom / fit /
//! fullscreen / info-overlay commands, `RefreshView`, the external editor, the clipboard copies, `PickAndOpenFolderAsync`, the
//! duplicate commands, `ClearCacheAsync` and the dialog shortcuts (contract `contracts/W8-08c.md`).
//! Target: `crates/pr-app/src/viewmodels/main/commands.rs` (`viewmodels::main::commands`).
//!
//! The file-action gate is held by the view model, not by the controllers (contracts W8-07c, W8-07f). D-W8-2 shape:
//! - A queued command (`RunActionAsync`, `RecycleAsync`, move/copy-to) is registered under a ticket and handed to
//!   `FileActionGate::run_queued`, whose closure only sends `MainEvent::GateTurn { ticket, turn }` into the inbox. The command then
//!   runs in `on_gate_turn` on the UI thread and keeps the [`GateTurn`] until the controller reports `Done` (INV-4: the gate
//!   covers the catalog / undo update that follows the file operation).
//! - Undo and duplicate cleanup use `run_exclusive`; their turns are kept until their last step.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use std::fmt;
use std::path::Path;

use pr_core::abstractions::UiAction;
use pr_core::model::FileOperationType;

use super::state::{ChangeSet, MainEvent, MainViewModel};
use crate::coordinators::duplicate_cleanup::DuplicateCleanupEvent;
use crate::coordinators::file_action_gate::{GateTurn, QueuedOutcome};
use crate::coordinators::file_actions::recycle::{FileActionEvent, FileActionStep};
use crate::services::contracts::ClipboardService;
use crate::viewport::fit::ViewRefreshOutcome;

// ---- seams -------------------------------------------------------------------------------------------------------------------

/// Why an external-editor launch did not happen. C# catches `Win32Exception`, `InvalidOperationException` and `FileNotFoundException`
/// (shown in an error dialog) and lets every other exception escape (`OpenInExternalEditor_WhenTheLaunchThrowsSomethingUnexpected_DoesNotHideIt`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalEditorFailure {
    /// An expected failure: the text is shown in the error dialog `dialog.externalEditor.failedTitle`.
    Expected { message: String },
    /// A bug (C# `ArgumentException` in the test): no dialog; `open_in_external_editor` returns it as `Err`.
    Unexpected { message: String },
}

/// C# `StartExternalEditor` test seam (`Action<string, string>`, default starts the process).
pub trait ExternalEditorLauncher {
    /// C# `Process.Start(new ProcessStartInfo(exePath) { UseShellExecute = false, ArgumentList = { filePath } })`.
    fn launch(&mut self, exe_path: &str, file_path: &str) -> Result<(), ExternalEditorFailure>;
}

/// The production launcher: `std::process::Command::new(exe).arg(file)` without a shell; every `io::Error` is `Expected`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessEditorLauncher;

impl ExternalEditorLauncher for ProcessEditorLauncher {
    fn launch(&mut self, exe_path: &str, file_path: &str) -> Result<(), ExternalEditorFailure> {
        todo!()
    }
}

/// An `Unexpected` launch failure that C# would let escape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEditorBug(pub String);

impl fmt::Display for ExternalEditorBug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExternalEditorBug {}

// ---- queued file commands ------------------------------------------------------------------------------------------------------

/// Which controller entry point a queued command calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum QueuedKind {
    /// `FileActionController::run_action(index, compare_selected, current)`.
    RunAction { index: i32 },
    /// `FileActionController::recycle(compare_selected, current)`.
    Recycle,
    /// `FileActionController::move_or_copy_to_folder(picker, operation, force_picker, get_source)` (W8-08e submits it).
    MoveCopy {
        operation: FileOperationType,
        force_picker: bool,
    },
}

/// A command waiting for, or holding, its gate turn.
#[derive(Debug)]
pub(super) struct QueuedFileCommand {
    pub kind: QueuedKind,
    /// C# `epoch = _folderLoadEpoch` captured at submission (R02).
    pub epoch: i64,
    /// Set while the command is parked behind a pending Explorer order.
    pub turn: Option<GateTurn>,
}

/// What `UndoCoreAsync` remembers across the undo's `await` (C# locals `currentFolder`, `undoEpoch`).
#[derive(Debug, Clone)]
pub(super) struct UndoContext {
    pub undo_epoch: i64,
    pub current_folder: Option<String>,
}

impl MainViewModel {
    // ---- queued file commands ----

    /// C# `RunActionAsync(index)` (line 568): registers `QueuedKind::RunAction { index }` with the current epoch and submits it with
    /// `submit_queued`. A rejected submission (an exclusive holder owns the gate) drops the command silently (C# task result false).
    pub fn run_action(&mut self, index: i32) -> ChangeSet {
        todo!()
    }

    /// C# `RecycleAsync()` (line 600): as `run_action`, `QueuedKind::Recycle`.
    pub fn recycle(&mut self) -> ChangeSet {
        todo!()
    }

    /// Registers the command under a new ticket and calls `file_action_gate.run_queued(move |turn| inbox.send(GateTurn { ticket, turn }))`
    /// (`Send + 'static`: only the inbox clone and the ticket are captured). `Rejected` removes the command; `StartedNow` has already
    /// sent the event inside the call, so the inbox is drained at once (the confirmation dialog of the C# prefix appears without a
    /// dispatcher hop); `Queued` waits for the successor post.
    pub(super) fn submit_queued(&mut self, kind: QueuedKind) -> ChangeSet {
        todo!()
    }

    /// The turn arrived. C# `WaitForQueuedTurnAsync(epoch)` (line 590): a command whose `epoch != folder_load_epoch` is dropped (its turn
    /// finishes at once, R02); else, if the Explorer order is pending, the command is parked (`Deferred::FileTurn`, the turn stored in the
    /// record) and resumed later; else `run_queued_command`.
    pub(super) fn on_gate_turn(&mut self, ticket: u64, turn: GateTurn) -> ChangeSet {
        todo!()
    }

    /// Runs the command body with its turn (after the second epoch check that follows the Explorer wait). Builds the controller call from
    /// `compare().selected_path()` and `catalog.current().path()` read NOW (the live selection when the turn comes), a `FileActionCx` over
    /// the catalog and the sink recorder, then `handle_step`.
    pub(super) fn run_queued_command(&mut self, ticket: u64, turn: GateTurn) -> ChangeSet {
        todo!()
    }

    /// Delivers an executor / undo result: `file_action_controller.handle_event(cx, event)` then `handle_step`.
    pub(super) fn handle_file_action_event(&mut self, event: FileActionEvent) -> ChangeSet {
        todo!()
    }

    /// Applies a controller step after the sink calls were replayed: `Pending` keeps the turn in `active_gate_turn`; `Done(Finished)`
    /// finishes it; `Done(Canceled)` finishes it (the C# exception escaped, nothing restored); `Done(Undone(result))` runs the second half of
    /// `UndoCoreAsync` (`finish_undo`).
    pub(super) fn handle_step(&mut self, step: FileActionStep) -> ChangeSet {
        todo!()
    }

    /// The gate became completely free (`MainEvent::GateReleased`): continues a settings reload that waited for it (W8-08d).
    pub(super) fn on_gate_released(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `IsFileActionInProgress` (line 618): `file_action_gate.is_held()`.
    pub fn is_file_action_in_progress(&self) -> bool {
        todo!()
    }

    /// C# `WhenFileActionIdleAsync()` (line 621): `file_action_gate.when_released(waiter)`; true when the gate was already free and the
    /// waiter ran inline. The window close waits with it (R7-7).
    pub fn when_file_action_idle(&self, waiter: UiAction) -> bool {
        todo!()
    }

    // ---- undo ----

    /// C# `UndoAsync()` (line 640): `file_action_gate.run_exclusive(..)`; nothing happens (no-op) while anything holds the gate (OC14).
    /// With the turn: `current_folder = session.folder`, `undo_epoch = folder_load_epoch`, `file_action_controller.undo_last(cx, current_folder)`,
    /// `handle_step`.
    pub fn undo(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `UndoCoreAsync` lines 647 to 663, run when the controller reports `Done(Undone(result))`: when
    /// `FileActionController::restores_outside_folder(result, current_folder)`: `reload_path = reload_path_after_undo(result)`,
    /// `folder = parent directory of reload_path` (non-empty), then `open_folder_core(folder, Some(reload_path), Some(undo_epoch),
    /// OpenPurpose::UndoReopen { error_message })` and the gate turn stays held until that open finishes (`finish_open` releases it);
    /// otherwise the turn finishes now.
    pub(super) fn finish_undo(
        &mut self,
        ctx: UndoContext,
        result: Option<pr_core::file_actions::types::UndoResult>,
    ) -> ChangeSet {
        todo!()
    }

    /// The follow-up of `OpenPurpose::UndoReopen` (C# lines 659 to 661, run when the reopen finished or was dropped): when `epoch ==
    /// Some(folder_load_epoch)` and the undo's `error_message` is non-empty it becomes the event status (the reload wrote its own);
    /// then the undo's gate turn is released (the gate stayed held until the reopen finished).
    pub(super) fn on_undo_reopen_finished(&mut self, epoch: Option<i64>, error_message: Option<String>) -> ChangeSet {
        todo!()
    }

    /// The follow-up of `OpenPurpose::DuplicateReload` (C# `ReloadFolderAsync` result): `duplicate_controller.on_reload_finished(cx,
    /// still_current)`, the recorded sink calls are replayed and the gate turn of the batch is released when the step is over.
    pub(super) fn on_duplicate_reload_finished(&mut self, still_current: bool) -> ChangeSet {
        todo!()
    }

    /// A queued file command parked behind the Explorer order (`Deferred::FileTurn`) is resumed: the epoch is compared AGAIN
    /// (C# `WaitForQueuedTurnAsync` after the wait), then `run_queued_command`.
    pub(super) fn resume_file_turn(&mut self, ticket: u64) -> ChangeSet {
        todo!()
    }

    // ---- viewer commands ----

    /// C# `ToggleFit()` (line 666): `viewer.reset_fit(0.0, 0.0)` (C# defaults), then `apply_viewer_update`.
    pub fn toggle_fit(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `ZoomIn()`.
    pub fn zoom_in(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `ZoomOut()`.
    pub fn zoom_out(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `ZoomActualSize()` (line 671): no-op without images; else `viewer.zoom_to_actual_size()`.
    pub fn zoom_actual_size(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `ToggleFullscreen()` (line 697).
    pub fn toggle_fullscreen(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `ExitFullscreen()` (line 698).
    pub fn exit_fullscreen(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `ToggleInfoOverlay()` (line 681). `show_info_overlay` is flipped in a copy of the CURRENT settings that is installed as the
    /// override BEFORE the overlay refresh and `notify_exif_line_changed` (they must read the new value); then the copy is saved with
    /// `store.save`. Save OK: the override is cleared again when it was not installed before (the store now holds the value). Save error
    /// (C# IOException, UnauthorizedAccess, NotSupported, Argument): logged `Could not save the info overlay setting`; the override stays
    /// (`SettingsSource::commit_edit` marks it provisional) so the overlay still toggles for this session.
    pub fn toggle_info_overlay(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `RefreshView()` (line 706): `None` without images. Else `scaling_changed = viewer.effective_scaling_quality() != HighQuality`;
    /// `viewer.set_force_high_quality_scaling(true)`; `effective_zoom = if viewer.is_fit() { fit_zoom } else { zoom }`;
    /// `(resolution, presenter_changes) = presenter.refresh_zoom_detail(effective_zoom)`; when the log is enabled one info line
    /// `Refresh view: scalingChanged=.. resolution=.. effectiveZoom=<0.####, invariant> fit=..`; returns the outcome and the changes.
    pub fn refresh_view(&mut self) -> (Option<ViewRefreshOutcome>, ChangeSet) {
        todo!()
    }

    // ---- external editor and clipboard ----

    /// Replaces the launcher (C# `StartExternalEditor` setter).
    pub fn set_external_editor_launcher(&mut self, launcher: Box<dyn ExternalEditorLauncher>) {
        todo!()
    }

    /// C# `CanOpenInExternalEditor` (line 730): the configured path is not blank and `has_images`.
    pub fn can_open_in_external_editor(&self) -> bool {
        todo!()
    }

    /// C# `OpenInExternalEditor()` (line 733). No editor configured: `show_settings_at(SettingsTarget::ExternalEditor)` (W8-08d) and stop.
    /// `path = compare().selected_path() or else presenter.current_presented_path() or else catalog.current().path`; empty -> nothing. A missing file
    /// -> `dialog.show_error(dialog.externalEditor.failedTitle, dialog.externalEditor.fileMissing {fileName})`. `launcher.launch(editor, path)`:
    /// `Expected` -> the same error dialog with the failure text; `Unexpected` -> `Err(ExternalEditorBug)`, no dialog.
    pub fn open_in_external_editor(&mut self) -> Result<ChangeSet, ExternalEditorBug> {
        todo!()
    }

    /// Replaces the clipboard (C# `Clipboard` setter).
    pub fn set_clipboard(&mut self, clipboard: Box<dyn ClipboardService>) {
        todo!()
    }

    /// C# `CanCopyCurrentFilePath` (line 762): `has_images && current_file_path().is_some()`.
    pub fn can_copy_current_file_path(&self) -> bool {
        todo!()
    }

    /// C# `CurrentFilePath()` (line 764): `compare().selected_path() or else presenter.current_presented_path() or else catalog.current().path`,
    /// `None` when empty.
    pub(super) fn current_file_path(&self) -> Option<String> {
        todo!()
    }

    /// C# `CopyFileName()` (line 771): file name with extension to the clipboard; the event status becomes `status.copiedFileName`
    /// on success or `status.clipboardCopyFailed` on failure. No path -> nothing.
    pub fn copy_file_name(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `CopyFullPathname()` (line 780): as `copy_file_name` with the full path and `status.copiedFullPath {path}`.
    pub fn copy_full_pathname(&mut self) -> ChangeSet {
        todo!()
    }

    // ---- folder picker, duplicates, cache ----

    /// C# `PickAndOpenFolderAsync()` (line 790): `dialog.pick_folder(start)` with `start = session.folder or else parent of the catalog current
    /// path`; a non-blank selection -> `open_folder(selection, None)`.
    pub fn pick_and_open_folder(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `RemoveDuplicatesAsync(removeNumbered)` (line 804): `file_action_gate.run_exclusive`; with the turn
    /// `duplicate_controller.remove_duplicates(cx, remove_numbered)`; `Pending` keeps the turn in `duplicate_gate_turn` until the
    /// controller reports `Done` / `Canceled`. No-op while anything holds the gate (R2-F-20).
    pub fn remove_duplicates(&mut self, remove_numbered: bool) -> ChangeSet {
        todo!()
    }

    /// Delivers a hash / recycle / cache-clear result: `duplicate_controller.handle_event(cx, event)`, replay, release the gate turn on
    /// `Done` / `Canceled`.
    pub(super) fn handle_duplicate_event(&mut self, event: DuplicateCleanupEvent) -> ChangeSet {
        todo!()
    }

    /// C# `CancelDuplicateCheck()` (line 812): true while a check is hashing (Esc is consumed).
    pub fn cancel_duplicate_check(&self) -> bool {
        todo!()
    }

    /// C# `DeferCloseForFileAction()` (line 818): false when no file action holds the gate; else cancels a hashing duplicate check and
    /// returns true (the window close waits with `when_file_action_idle`).
    pub fn defer_close_for_file_action(&self) -> bool {
        todo!()
    }

    /// C# `ClearCacheAsync()` (line 828): `duplicate_controller.clear_cache(cx)` (NOT under the gate, as C#).
    pub fn clear_cache(&mut self) -> ChangeSet {
        todo!()
    }

    // ---- dialogs ----

    /// C# `ShowRecovery()`.
    pub fn show_recovery(&self) {
        todo!()
    }

    /// C# `ShowDiagnostics()`.
    pub fn show_diagnostics(&self) {
        todo!()
    }

    /// C# `ShowSkippedFiles()`: `dialog.show_skipped_files(skipped_entries)`.
    pub fn show_skipped_files(&self) {
        todo!()
    }

    /// C# `ShowBenchmark()` (line 849): `dialog.show_benchmark(session.folder or else parent of the catalog current path)`.
    pub fn show_benchmark(&self) {
        todo!()
    }
}
