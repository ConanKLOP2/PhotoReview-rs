//! Port of `PhotoReview.App.Shared/ViewModels/MainViewModel.cs` lines 565 to 854 (baseline `d92d6aae`): `RunActionAsync`,
//! `WaitForQueuedTurnAsync`, `RecycleAsync`, the file-action gate members, `UndoAsync` / `UndoCoreAsync`, the zoom / fit /
//! fullscreen / info-overlay commands, `RefreshView`, the external editor, the clipboard copies, `PickAndOpenFolderAsync`, the
//! duplicate commands, `ClearCacheAsync` and the dialog shortcuts (contract `contracts/W8-08c.md`).
//! External editor members port MainViewModel.cs lines774-880 at7eaee2a2 (W9-E5).
//! Target: `crates/pr-app/src/viewmodels/main/commands.rs` (`viewmodels::main::commands`).
//!
//! The file-action gate is held by the view model, not by the controllers (contracts W8-07c, W8-07f). D-W8-2 shape:
//! - A queued command (`RunActionAsync`, `RecycleAsync`, move/copy-to) is registered under a ticket and handed to
//!   `FileActionGate::run_queued`, whose closure only sends `MainEvent::GateTurn { ticket, turn }` into the inbox. The command then
//!   runs in `on_gate_turn` on the UI thread and keeps the [`GateTurn`] until the controller reports `Done` (INV-4: the gate
//!   covers the catalog / undo update that follows the file operation).
//! - Undo and duplicate cleanup use `run_exclusive`; their turns are kept until their last step.

use std::fmt;
use std::os::windows::process::CommandExt;
use std::path::Path;

use pr_core::abstractions::{SettingsTarget, UiAction};
use pr_core::file_actions::types::UndoResult;
use pr_core::model::{FileOperationType, ScalingQuality};
use pr_core::settings::external_editors::{ExternalEditor, ExternalEditors};
use pr_core::text::dotnet_path::{get_directory_name, get_file_name};
use pr_l10n::template::LocArg;

use super::navigation::{Deferred, OpenPurpose};
use super::state::{ChangeSet, MainEvent, MainViewModel};
use crate::coordinators::duplicate_cleanup::{DuplicateCleanupCx, DuplicateCleanupEvent, DuplicateCleanupStep};
use crate::coordinators::file_action_gate::{GateTurn, QueuedOutcome};
use crate::coordinators::file_actions::recycle::{
    FileActionController, FileActionCx, FileActionDone, FileActionEvent, FileActionStep,
};
use crate::services::contracts::ClipboardService;
use crate::viewport::fit::{ViewRefreshOutcome, ZoomDetailRefreshResult};

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

/// C# `StartExternalEditor` test seam (`Action<string, string, string>` = exePath, filePath, commandLine since W9-E5; default starts the process).
pub trait ExternalEditorLauncher {
    /// C# default: `Process.Start(new ProcessStartInfo(exePath) { UseShellExecute = false, Arguments = commandLine, WorkingDirectory = Path.GetDirectoryName(exePath) ?? "" })`.
    /// `file_path` is the file that is sent (a fake records it); `command_line` is the finished `ExternalEditors::build_arguments` text.
    fn launch(&mut self, exe_path: &str, file_path: &str, command_line: &str) -> Result<(), ExternalEditorFailure>;
}

/// The production launcher: `std::process::Command::new(exe).arg(file)` without a shell; every `io::Error` is `Expected`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessEditorLauncher;

impl ExternalEditorLauncher for ProcessEditorLauncher {
    fn launch(&mut self, exe_path: &str, _file_path: &str, command_line: &str) -> Result<(), ExternalEditorFailure> {
        // No shell. W9-E5: the command line is the finished `ExternalEditors::build_arguments` text, passed VERBATIM (`raw_arg`: std must not quote it again),
        // and the working directory is the editor's folder (C# `Path.GetDirectoryName(exePath) ?? ""`; no parent = the current directory).
        let mut command = std::process::Command::new(exe_path);
        command.raw_arg(command_line);
        if let Some(folder) = std::path::Path::new(exe_path)
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
        {
            command.current_dir(folder);
        }
        match command.spawn() {
            // C# disposes the process handle at once; the editor keeps running.
            Ok(child) => {
                drop(child);
                Ok(())
            }
            Err(error) => Err(ExternalEditorFailure::Expected {
                message: error.to_string(),
            }),
        }
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
        self.submit_queued(QueuedKind::RunAction { index })
    }

    /// C# `RecycleAsync()` (line 600): as `run_action`, `QueuedKind::Recycle`.
    pub fn recycle(&mut self) -> ChangeSet {
        self.submit_queued(QueuedKind::Recycle)
    }

    /// Registers the command under a new ticket and calls `file_action_gate.run_queued(move |turn| inbox.send(GateTurn { ticket, turn }))`
    /// (`Send + 'static`: only the inbox clone and the ticket are captured). `Rejected` removes the command; `StartedNow` has already
    /// sent the event inside the call, so the inbox is drained at once (the confirmation dialog of the C# prefix appears without a
    /// dispatcher hop); `Queued` waits for the successor post.
    pub(super) fn submit_queued(&mut self, kind: QueuedKind) -> ChangeSet {
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        // R02: the identity of the folder load the user was looking at.
        let epoch = self.folder_load_epoch;
        self.queued_file_commands.insert(
            ticket,
            QueuedFileCommand {
                kind,
                epoch,
                turn: None,
            },
        );
        let inbox = self.inbox.clone();
        let gate = self.file_action_gate.clone();
        match gate.run_queued(move |turn| inbox.send(MainEvent::GateTurn { ticket, turn })) {
            QueuedOutcome::Rejected => {
                self.queued_file_commands.remove(&ticket);
                ChangeSet::default()
            }
            QueuedOutcome::StartedNow => self.drain_inbox(),
            QueuedOutcome::Queued => ChangeSet::default(),
        }
    }

    /// Applies the events that are already in the inbox, in arrival order, until it stays empty (no presenter work: `pump` does that).
    fn drain_inbox(&mut self) -> ChangeSet {
        let mut result = ChangeSet::default();
        loop {
            let events = self.inbox.drain();
            if events.is_empty() {
                return result;
            }
            for event in events {
                result.merge(self.dispatch(event));
            }
        }
    }

    /// The turn arrived. C# `WaitForQueuedTurnAsync(epoch)` (line 590): a command whose `epoch != folder_load_epoch` is dropped (its turn
    /// finishes at once, R02); else, if the Explorer order is pending, the command is parked (`Deferred::FileTurn`, the turn stored in the
    /// record) and resumed later; else `run_queued_command`.
    pub(super) fn on_gate_turn(&mut self, ticket: u64, turn: GateTurn) -> ChangeSet {
        let Some(epoch) = self.queued_file_commands.get(&ticket).map(|record| record.epoch) else {
            turn.finish();
            return ChangeSet::default();
        };
        if epoch != self.folder_load_epoch {
            self.queued_file_commands.remove(&ticket);
            turn.finish();
            return ChangeSet::default();
        }
        if self.park_if_order_pending(Deferred::FileTurn { ticket }) {
            if let Some(record) = self.queued_file_commands.get_mut(&ticket) {
                record.turn = Some(turn);
            }
            return ChangeSet::default();
        }
        self.run_queued_command(ticket, turn)
    }

    /// A queued file command parked behind the Explorer order (`Deferred::FileTurn`) is resumed: the epoch is compared AGAIN
    /// (C# `WaitForQueuedTurnAsync` after the wait), then `run_queued_command`.
    pub(super) fn resume_file_turn(&mut self, ticket: u64) -> ChangeSet {
        let Some(record) = self.queued_file_commands.get_mut(&ticket) else {
            return ChangeSet::default();
        };
        let Some(turn) = record.turn.take() else {
            return ChangeSet::default();
        };
        if record.epoch != self.folder_load_epoch {
            self.queued_file_commands.remove(&ticket);
            turn.finish();
            return ChangeSet::default();
        }
        self.run_queued_command(ticket, turn)
    }

    /// Runs the command body with its turn (after the second epoch check that follows the Explorer wait). Builds the controller call from
    /// `compare().selected_path()` and `catalog.current().path` read NOW (the live selection when the turn comes), a `FileActionCx` over
    /// the catalog and the sink recorder, then `handle_step`.
    pub(super) fn run_queued_command(&mut self, ticket: u64, turn: GateTurn) -> ChangeSet {
        let Some(record) = self.queued_file_commands.remove(&ticket) else {
            turn.finish();
            return ChangeSet::default();
        };
        // INV-4: the gate stays held until the controller reports `Done`, i.e. until the catalog update after the file operation.
        self.active_gate_turn = Some(turn);
        let step = match record.kind {
            QueuedKind::MoveCopy {
                operation,
                force_picker,
            } => self.begin_move_copy(operation, force_picker),
            kind => {
                let compare_selected = self.presenter.compare().selected_path().map(str::to_owned);
                let current = self.catalog.current().map(|entry| entry.path().to_owned());
                let mut cx = FileActionCx {
                    catalog: &mut self.catalog,
                    sink: &mut self.sink,
                };
                match kind {
                    QueuedKind::RunAction { index } => self.file_action_controller.run_action(
                        &mut cx,
                        index,
                        compare_selected.as_deref(),
                        current.as_deref(),
                    ),
                    _ => self
                        .file_action_controller
                        .recycle(&mut cx, compare_selected.as_deref(), current.as_deref()),
                }
            }
        };
        self.handle_step(step)
    }

    /// Delivers an executor / undo result: `file_action_controller.handle_event(cx, event)` then `handle_step`.
    pub(super) fn handle_file_action_event(&mut self, event: FileActionEvent) -> ChangeSet {
        let step = {
            let mut cx = FileActionCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            self.file_action_controller.handle_event(&mut cx, event)
        };
        self.handle_step(step)
    }

    /// Applies a controller step after the sink calls were replayed: `Pending` keeps the turn in `active_gate_turn`; `Done(Finished)`
    /// finishes it; `Done(Canceled)` finishes it (the C# exception escaped, nothing restored); `Done(Undone(result))` runs the second half of
    /// `UndoCoreAsync` (`finish_undo`).
    pub(super) fn handle_step(&mut self, step: FileActionStep) -> ChangeSet {
        // T3: the sink calls the controller recorded (and the debt of a finished present) are replayed BEFORE the turn is released.
        let mut result = self.apply_sink_actions();
        match step {
            FileActionStep::Pending => {}
            FileActionStep::Done(FileActionDone::Finished { .. } | FileActionDone::Canceled) => {
                self.undo_context = None;
                self.release_active_turn();
            }
            FileActionStep::Done(FileActionDone::Undone(undone)) => match self.undo_context.take() {
                Some(context) => result.merge(self.finish_undo(context, undone)),
                None => self.release_active_turn(),
            },
        }
        result
    }

    /// Finishes the gate turn of the running file action or undo (C# `finally` of `RunQueuedAsync` / `RunExclusiveAsync`).
    fn release_active_turn(&mut self) {
        if let Some(turn) = self.active_gate_turn.take() {
            turn.finish();
        }
    }

    /// The gate became completely free (`MainEvent::GateReleased`): continues a settings reload that waited for it (W8-08d).
    pub(super) fn on_gate_released(&mut self) -> ChangeSet {
        self.settings_reloads_waiting = self.settings_reloads_waiting.saturating_sub(1);
        self.continue_settings_reload()
    }

    /// C# `IsFileActionInProgress` (line 618): `file_action_gate.is_held()`.
    pub fn is_file_action_in_progress(&self) -> bool {
        self.file_action_gate.is_held()
    }

    /// C# `WhenFileActionIdleAsync()` (line 621): `file_action_gate.when_released(waiter)`; true when the gate was already free and the
    /// waiter ran inline. The window close waits with it (R7-7).
    pub fn when_file_action_idle(&self, waiter: UiAction) -> bool {
        self.file_action_gate.when_released(waiter)
    }

    // ---- undo ----

    /// C# `UndoAsync()` (line 640): `file_action_gate.run_exclusive(..)`; nothing happens (no-op) while anything holds the gate (OC14).
    /// With the turn: `current_folder = session.folder`, `undo_epoch = folder_load_epoch`, `file_action_controller.undo_last(cx, current_folder)`,
    /// `handle_step`.
    pub fn undo(&mut self) -> ChangeSet {
        let gate = self.file_action_gate.clone();
        let mut result = ChangeSet::default();
        gate.run_exclusive(|turn| {
            let current_folder = self.current_session.as_ref().map(|session| session.folder.clone());
            self.undo_context = Some(UndoContext {
                undo_epoch: self.folder_load_epoch,
                current_folder: current_folder.clone(),
            });
            self.active_gate_turn = Some(turn);
            let step = {
                let mut cx = FileActionCx {
                    catalog: &mut self.catalog,
                    sink: &mut self.sink,
                };
                self.file_action_controller
                    .undo_last(&mut cx, current_folder.as_deref())
            };
            result = self.handle_step(step);
        });
        result
    }

    /// C# `UndoCoreAsync` lines 647 to 663, run when the controller reports `Done(Undone(result))`: when
    /// `FileActionController::restores_outside_folder(result, current_folder)`: `reload_path = reload_path_after_undo(result)`,
    /// `folder = parent directory of reload_path` (non-empty), then `open_folder_core(folder, Some(reload_path), Some(undo_epoch),
    /// OpenPurpose::UndoReopen { error_message })` and the gate turn stays held until that open finishes (`finish_open` releases it);
    /// otherwise the turn finishes now.
    pub(super) fn finish_undo(&mut self, ctx: UndoContext, result: Option<UndoResult>) -> ChangeSet {
        let UndoContext {
            undo_epoch,
            current_folder,
        } = ctx;
        if FileActionController::restores_outside_folder(result.as_ref(), current_folder.as_deref()) {
            // Open at a member that really came back: `source` (first manifest member) may be a permanently deleted one.
            let reload_path = result.as_ref().and_then(FileActionController::reload_path_after_undo);
            let folder = reload_path
                .as_deref()
                .filter(|path| !path.is_empty())
                .and_then(get_directory_name)
                .filter(|folder| !folder.is_empty());
            if let (Some(folder), Some(reload_path)) = (folder, reload_path) {
                // R09: the undo awaited; a folder the user opened meanwhile must not be replaced by the undo's source folder (the file
                // is restored on disk either way), nor get this undo's status.
                let error_message = result.and_then(|undone| undone.error_message);
                match self.open_folder_core(
                    &folder,
                    Some(&reload_path),
                    Some(undo_epoch),
                    OpenPurpose::UndoReopen { error_message },
                ) {
                    Ok(changes) => return changes,
                    Err(error) => self.log.error("The folder reopen after an undo failed", Some(&error)),
                }
            }
        }
        self.release_active_turn();
        ChangeSet::default()
    }

    /// The follow-up of `OpenPurpose::UndoReopen` (C# lines 659 to 661, run when the reopen finished or was dropped): when `epoch ==
    /// Some(folder_load_epoch)` and the undo's `error_message` is non-empty it becomes the event status (the reload wrote its own);
    /// then the undo's gate turn is released (the gate stayed held until the reopen finished).
    pub(super) fn on_undo_reopen_finished(&mut self, epoch: Option<i64>, error_message: Option<String>) -> ChangeSet {
        let mut result = ChangeSet::default();
        if epoch == Some(self.folder_load_epoch)
            && let Some(message) = error_message.filter(|message| !message.is_empty())
        {
            result.merge(self.set_status_text(&message));
        }
        self.release_active_turn();
        result
    }

    /// The follow-up of `OpenPurpose::DuplicateReload` (C# `ReloadFolderAsync` result): `duplicate_controller.on_reload_finished(cx,
    /// still_current)`, the recorded sink calls are replayed and the gate turn of the batch is released when the step is over.
    pub(super) fn on_duplicate_reload_finished(&mut self, still_current: bool) -> ChangeSet {
        let step = {
            let mut cx = DuplicateCleanupCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            self.duplicate_controller.on_reload_finished(&mut cx, still_current)
        };
        self.handle_duplicate_step(step, true)
    }

    // ---- viewer commands ----

    /// C# `ToggleFit()` (line 666): `viewer.reset_fit(0.0, 0.0)` (C# defaults), then `apply_viewer_update`.
    pub fn toggle_fit(&mut self) -> ChangeSet {
        let update = self.viewer.borrow_mut().reset_fit(0.0, 0.0);
        self.apply_viewer_update(update)
    }

    /// C# `ZoomIn()`.
    pub fn zoom_in(&mut self) -> ChangeSet {
        let update = self.viewer.borrow_mut().zoom_in();
        self.apply_viewer_update(update)
    }

    /// C# `ZoomOut()`.
    pub fn zoom_out(&mut self) -> ChangeSet {
        let update = self.viewer.borrow_mut().zoom_out();
        self.apply_viewer_update(update)
    }

    /// C# `ZoomActualSize()` (line 671): no-op without images; else `viewer.zoom_to_actual_size()`.
    pub fn zoom_actual_size(&mut self) -> ChangeSet {
        if !self.has_images() {
            return ChangeSet::default();
        }
        let update = self.viewer.borrow_mut().zoom_to_actual_size();
        self.apply_viewer_update(update)
    }

    /// C# `ToggleFullscreen()` (line 697).
    pub fn toggle_fullscreen(&mut self) -> ChangeSet {
        let update = self.viewer.borrow_mut().toggle_fullscreen();
        self.apply_viewer_update(update)
    }

    /// C# `ExitFullscreen()` (line 698).
    pub fn exit_fullscreen(&mut self) -> ChangeSet {
        let update = self.viewer.borrow_mut().exit_fullscreen();
        self.apply_viewer_update(update)
    }

    /// C# `ToggleInfoOverlay()` (line 681). `show_info_overlay` is flipped in a copy of the CURRENT settings that is installed as the
    /// override BEFORE the overlay refresh and `notify_exif_line_changed` (they must read the new value); then the copy is saved with
    /// `store.save`. Save OK: the override is cleared again when it was not installed before (the store now holds the value). Save error
    /// (C# IOException, UnauthorizedAccess, NotSupported, Argument): logged `Could not save the info overlay setting`; the override stays
    /// (`SettingsSource::commit_edit` marks it provisional) so the overlay still toggles for this session.
    pub fn toggle_info_overlay(&mut self) -> ChangeSet {
        let edit = self
            .settings
            .begin_edit(|settings| settings.show_info_overlay = !settings.show_info_overlay);
        let mut result = self.refresh_overlay();
        result.merge(self.notify_exif_line_changed());
        if let Err(error) = self.settings.commit_edit(edit) {
            // The overlay still toggles for this session; only persisting failed.
            self.log.error("Could not save the info overlay setting", Some(&error));
        }
        result
    }

    /// C# `RefreshView()` (line 706): `None` without images. Else `scaling_changed = viewer.effective_scaling_quality() != HighQuality`;
    /// `viewer.set_force_high_quality_scaling(true)`; `effective_zoom = if viewer.is_fit() { fit_zoom } else { zoom }`;
    /// `(resolution, presenter_changes) = presenter.refresh_zoom_detail(effective_zoom)`; when the log is enabled one info line
    /// `Refresh view: scalingChanged=.. resolution=.. effectiveZoom=<0.####, invariant> fit=..`; returns the outcome and the changes.
    pub fn refresh_view(&mut self) -> (Option<ViewRefreshOutcome>, ChangeSet) {
        if !self.has_images() {
            return (None, ChangeSet::default());
        }
        let (scaling_changed, is_fit, effective_zoom) = {
            let viewer = self.viewer.borrow();
            let zoom = if viewer.is_fit() {
                viewer.fit_zoom()
            } else {
                viewer.zoom()
            };
            (
                viewer.effective_scaling_quality() != ScalingQuality::HighQuality,
                viewer.is_fit(),
                zoom,
            )
        };
        let forced = self.viewer.borrow_mut().set_force_high_quality_scaling(true);
        let (resolution, presenter_changes) = self.presenter.refresh_zoom_detail(effective_zoom);
        let resolution = ZoomDetailRefreshResult::from(resolution);
        if self.log.enabled() {
            self.log.info(&format!(
                "Refresh view: scalingChanged={} resolution={resolution:?} effectiveZoom={} fit={}",
                pascal_bool(scaling_changed),
                format_zoom(effective_zoom),
                pascal_bool(is_fit),
            ));
        }
        let mut result = ChangeSet {
            viewer: forced,
            ..ChangeSet::default()
        };
        result.merge(self.finish_presenter_call(presenter_changes));
        (
            Some(ViewRefreshOutcome {
                scaling_changed,
                resolution,
            }),
            result,
        )
    }

    // ---- external editor and clipboard ----

    /// Replaces the launcher (C# `StartExternalEditor` setter).
    pub fn set_external_editor_launcher(&mut self, launcher: Box<dyn ExternalEditorLauncher>) {
        self.external_editor = launcher;
    }

    /// C# `CanOpenInExternalEditor` at 7eaee2a2: an effective editor and an open image.
    pub fn can_open_in_external_editor(&self) -> bool {
        ExternalEditors::effective_default(&self.settings.current()).is_some() && self.has_images()
    }

    /// C# `OpenInExternalEditor()`: launch the effective default; no editor opens Settings at its editor field.
    pub fn open_in_external_editor(&mut self) -> Result<ChangeSet, ExternalEditorBug> {
        let Some(editor) = ExternalEditors::effective_default(&self.settings.current()) else {
            return Ok(self.show_settings_at(SettingsTarget::ExternalEditor));
        };
        self.launch_external_editor(&editor)
    }

    /// Port of MainViewModel.LaunchExternalEditor, C# 7eaee2a2 lines805-829.
    fn launch_external_editor(&mut self, editor: &ExternalEditor) -> Result<ChangeSet, ExternalEditorBug> {
        let Some(shown) = self.current_file_path() else {
            return Ok(ChangeSet::default());
        };
        let title = self.with_ctx(|ctx| ctx.texts.get("dialog.externalEditor.failedTitle"));
        if !self.file_system.file_exists(Path::new(&shown)) {
            let message = self.with_ctx(|ctx| {
                ctx.texts.format_with(
                    "dialog.externalEditor.fileMissing",
                    &[LocArg::new("fileName", get_file_name(&shown))],
                    ctx.numbers,
                )
            });
            self.dialog_service.show_error(&title, &message);
            return Ok(ChangeSet::default());
        }
        let paired = self
            .catalog
            .find(&shown)
            .and_then(|entry| entry.capture_group.as_ref())
            .map(|group| group.raw_path());
        let path = ExternalEditors::resolve_file_to_send(
            &shown,
            paired,
            self.settings.current().external_editor_sends_raw_original,
            &|path| self.file_system.file_exists(Path::new(path)),
        );
        if editor.path.trim().is_empty() || !self.file_system.file_exists(Path::new(&editor.path)) {
            let message = self.with_ctx(|ctx| {
                ctx.texts.format_with(
                    "dialog.externalEditor.exeMissing",
                    &[LocArg::new("path", editor.path.as_str())],
                    ctx.numbers,
                )
            });
            self.dialog_service.show_error(&title, &message);
            return Ok(ChangeSet::default());
        }
        let command_line = ExternalEditors::build_arguments(Some(&editor.arguments), &path)
            .ok_or_else(|| ExternalEditorBug("Cannot build external editor arguments for an empty path".into()))?;
        match self.external_editor.launch(&editor.path, &path, &command_line) {
            Ok(()) => Ok(ChangeSet::default()),
            Err(ExternalEditorFailure::Expected { message }) => {
                self.dialog_service.show_error(&title, &message);
                Ok(ChangeSet::default())
            }
            Err(ExternalEditorFailure::Unexpected { message }) => Err(ExternalEditorBug(message)),
        }
    }

    /// Replaces the clipboard (C# `Clipboard` setter).
    pub fn set_clipboard(&mut self, clipboard: Box<dyn ClipboardService>) {
        self.clipboard = clipboard;
    }

    /// C# `CanCopyCurrentFilePath` (line 762): `has_images && current_file_path().is_some()`.
    pub fn can_copy_current_file_path(&self) -> bool {
        self.has_images() && self.current_file_path().is_some()
    }

    /// C# `CurrentFilePath()` (line 764): `compare().selected_path() or else presenter.current_presented_path() or else catalog.current().path`,
    /// `None` when empty.
    pub(super) fn current_file_path(&self) -> Option<String> {
        self.presenter
            .compare()
            .selected_path()
            .or_else(|| self.presenter.current_presented_path())
            .or_else(|| self.catalog.current().map(|entry| entry.path()))
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
    }

    /// C# `CopyFileName()` (line 771): file name with extension to the clipboard; the event status becomes `status.copiedFileName`
    /// on success or `status.clipboardCopyFailed` on failure. No path -> nothing.
    pub fn copy_file_name(&mut self) -> ChangeSet {
        let Some(path) = self.current_file_path() else {
            return ChangeSet::default();
        };
        let name = get_file_name(&path).to_owned();
        let text = if self.clipboard.try_set_text(&name) {
            self.with_ctx(|ctx| {
                ctx.texts.format_with(
                    "status.copiedFileName",
                    &[LocArg::new("fileName", name.as_str())],
                    ctx.numbers,
                )
            })
        } else {
            self.with_ctx(|ctx| ctx.texts.get("status.clipboardCopyFailed"))
        };
        self.set_status_text(&text)
    }

    /// C# `CopyFullPathname()` (line 780): as `copy_file_name` with the full path and `status.copiedFullPath {path}`.
    pub fn copy_full_pathname(&mut self) -> ChangeSet {
        let Some(path) = self.current_file_path() else {
            return ChangeSet::default();
        };
        let text = if self.clipboard.try_set_text(&path) {
            self.with_ctx(|ctx| {
                ctx.texts.format_with(
                    "status.copiedFullPath",
                    &[LocArg::new("path", path.as_str())],
                    ctx.numbers,
                )
            })
        } else {
            self.with_ctx(|ctx| ctx.texts.get("status.clipboardCopyFailed"))
        };
        self.set_status_text(&text)
    }

    // ---- folder picker, duplicates, cache ----

    /// The folder the dialogs start in (C# `_currentSession?.Folder ?? Path.GetDirectoryName(_catalog.Current.Path)`): the session folder when
    /// a session exists (an EMPTY one stays empty, the C# null-coalescing operator tests for null only), else the catalog folder.
    fn start_folder(&self) -> Option<String> {
        match &self.current_session {
            Some(session) => Some(session.folder.clone()),
            None => self
                .catalog
                .current()
                .and_then(|entry| get_directory_name(entry.path())),
        }
    }

    /// C# `PickAndOpenFolderAsync()` (line 790): `dialog.pick_folder(start)` with `start = session.folder or else parent of the catalog current
    /// path`; a non-blank selection -> `open_folder(selection, None)`.
    pub fn pick_and_open_folder(&mut self) -> ChangeSet {
        let start = self.start_folder();
        let selected = self.dialog_service.pick_folder(start.as_deref().map(Path::new));
        let Some(selected) = selected.map(|path| path.to_string_lossy().into_owned()) else {
            return ChangeSet::default();
        };
        if selected.trim().is_empty() {
            return ChangeSet::default();
        }
        match self.open_folder(&selected, None) {
            Ok(changes) => changes,
            Err(error) => {
                self.log.error("Could not open the picked folder", Some(&error));
                ChangeSet::default()
            }
        }
    }

    /// C# `RemoveDuplicatesAsync(removeNumbered)` (line 804): `file_action_gate.run_exclusive`; with the turn
    /// `duplicate_controller.remove_duplicates(cx, remove_numbered)`; `Pending` keeps the turn in `duplicate_gate_turn` until the
    /// controller reports `Done` / `Canceled`. No-op while anything holds the gate (R2-F-20).
    pub fn remove_duplicates(&mut self, remove_numbered: bool) -> ChangeSet {
        let gate = self.file_action_gate.clone();
        let mut result = ChangeSet::default();
        gate.run_exclusive(|turn| {
            self.duplicate_gate_turn = Some(turn);
            let step = {
                let mut cx = DuplicateCleanupCx {
                    catalog: &mut self.catalog,
                    sink: &mut self.sink,
                };
                self.duplicate_controller.remove_duplicates(&mut cx, remove_numbered)
            };
            result = self.handle_duplicate_step(step, true);
        });
        result
    }

    /// Delivers a hash / recycle / cache-clear result: `duplicate_controller.handle_event(cx, event)`, replay, release the gate turn on
    /// `Done` / `Canceled`.
    pub(super) fn handle_duplicate_event(&mut self, event: DuplicateCleanupEvent) -> ChangeSet {
        // "Clear cache" is not under the gate: its completion must not release the turn of a duplicate check that runs meanwhile.
        let releases_turn = !matches!(event, DuplicateCleanupEvent::CacheCleared { .. });
        let step = {
            let mut cx = DuplicateCleanupCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            self.duplicate_controller.handle_event(&mut cx, event)
        };
        self.handle_duplicate_step(step, releases_turn)
    }

    /// Replays what a duplicate-cleanup step recorded, then releases the gate turn when the step is over and `releases_turn`.
    fn handle_duplicate_step(&mut self, step: DuplicateCleanupStep, releases_turn: bool) -> ChangeSet {
        let result = self.apply_sink_actions();
        if releases_turn && step != DuplicateCleanupStep::Pending {
            if let Some(turn) = self.duplicate_gate_turn.take() {
                turn.finish();
            }
        }
        result
    }

    /// C# `CancelDuplicateCheck()` (line 812): true while a check is hashing (Esc is consumed).
    pub fn cancel_duplicate_check(&self) -> bool {
        self.duplicate_controller.cancel_duplicate_check()
    }

    /// C# `DeferCloseForFileAction()` (line 818): false when no file action holds the gate; else cancels a hashing duplicate check and
    /// returns true (the window close waits with `when_file_action_idle`).
    pub fn defer_close_for_file_action(&self) -> bool {
        if !self.is_file_action_in_progress() {
            return false;
        }
        self.cancel_duplicate_check();
        true
    }

    /// C# `ClearCacheAsync()` (line 828): `duplicate_controller.clear_cache(cx)` (NOT under the gate, as C#).
    pub fn clear_cache(&mut self) -> ChangeSet {
        let step = {
            let mut cx = DuplicateCleanupCx {
                catalog: &mut self.catalog,
                sink: &mut self.sink,
            };
            self.duplicate_controller.clear_cache(&mut cx)
        };
        self.handle_duplicate_step(step, false)
    }

    // ---- dialogs ----

    /// C# `ShowRecovery()`.
    pub fn show_recovery(&self) {
        self.dialog_service.show_recovery();
    }

    /// C# `ShowDiagnostics()`.
    pub fn show_diagnostics(&self) {
        self.dialog_service.show_diagnostics();
    }

    /// C# `ShowSkippedFiles()`: `dialog.show_skipped_files(skipped_entries)`.
    pub fn show_skipped_files(&self) {
        self.dialog_service.show_skipped_files(&self.skipped_entries);
    }

    /// C# `ShowBenchmark()` (line 849): `dialog.show_benchmark(session.folder or else parent of the catalog current path)`.
    pub fn show_benchmark(&self) {
        let folder = self.start_folder();
        self.dialog_service.show_benchmark(folder.as_deref().map(Path::new));
    }
}

/// C# `bool.ToString()`: `True` / `False`.
fn pascal_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

/// C# `double.ToString("0.####", InvariantCulture)`: at most four decimals, no trailing zeros.
fn format_zoom(value: f64) -> String {
    let text = format!("{value:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" || trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

impl From<crate::coordinators::zoom_detail::ZoomDetailRefreshResult> for ZoomDetailRefreshResult {
    /// The presenter returns the loader's enum, `ViewRefreshOutcome` carries the `viewport::fit` one (open question O1 of the contract:
    /// two definitions with the same variants).
    fn from(value: crate::coordinators::zoom_detail::ZoomDetailRefreshResult) -> Self {
        use crate::coordinators::zoom_detail::ZoomDetailRefreshResult as Loader;
        match value {
            Loader::AlreadyFullResolution => Self::AlreadyFullResolution,
            Loader::PreviewSufficient => Self::PreviewSufficient,
            Loader::ShowedOriginal => Self::ShowedOriginal,
            Loader::LoadingOriginal => Self::LoadingOriginal,
            Loader::RetryingOriginal => Self::RetryingOriginal,
        }
    }
}

impl MainViewModel {
    /// W9-E5, C# `OpenInExternalEditor(string editorId)`: sends the photo to the declared editor with this id (ordinal) among `ExternalEditors::effective`;
    /// an unknown id does nothing. Same `Result` contract as `open_in_external_editor` (`Err` only for an `Unexpected` launch failure).
    pub fn open_in_external_editor_by_id(&mut self, editor_id: &str) -> Result<ChangeSet, ExternalEditorBug> {
        match ExternalEditors::effective(&self.settings.current())
            .into_iter()
            .find(|editor| editor.id == editor_id)
        {
            Some(editor) => self.launch_external_editor(&editor),
            None => Ok(ChangeSet::default()),
        }
    }

    /// W9-E5, C# `AddExternalEditor()` ("Add more editors..."): see the contract (dialog prompt, legacy materialisation, 32 cap, fresh id, `reconcile`, save).
    pub fn add_external_editor(&mut self) -> ChangeSet {
        let initial = ExternalEditors::effective_default(&self.settings.current())
            .filter(|editor| !editor.path.trim().is_empty())
            .and_then(|editor| get_directory_name(&editor.path));
        let Some(mut added) = self
            .dialog_service
            .prompt_add_external_editor(initial.as_deref().map(Path::new))
        else {
            return ChangeSet::default();
        };
        if added.path.trim().is_empty() {
            return ChangeSet::default();
        }
        let mut settings = (*self.settings.current()).clone();
        Self::materialize_legacy_editor(&mut settings);
        if settings.external_editors.len() >= ExternalEditors::MAX_EDITORS {
            return ChangeSet::default();
        }
        added.id = ExternalEditor::new_id();
        settings.external_editors.push(added);
        ExternalEditors::reconcile(&mut settings);
        self.save_external_editor_settings(settings);
        ChangeSet::default()
    }

    /// W9-E5, C# `ChooseDefaultExternalEditor()` ("Set default editor..."): see the contract.
    pub fn choose_default_external_editor(&mut self) -> ChangeSet {
        let mut settings = (*self.settings.current()).clone();
        if Self::materialize_legacy_editor(&mut settings) {
            // C# materializes the live legacy list before the modal, even on Cancel; no save yet.
            self.settings.begin_edit(|current| *current = settings.clone());
        }
        if settings.external_editors.is_empty() {
            return ChangeSet::default();
        }
        let default = ExternalEditors::default(&settings);
        let chosen = self.dialog_service.prompt_default_external_editor(
            &settings.external_editors,
            default.as_ref().map(|editor| editor.id.as_str()),
        );
        let Some(chosen) = chosen.filter(|id| ExternalEditors::find(&settings, Some(id)).is_some()) else {
            return ChangeSet::default();
        };
        settings.default_external_editor_id = chosen;
        ExternalEditors::reconcile(&mut settings);
        self.save_external_editor_settings(settings);
        ChangeSet::default()
    }

    fn materialize_legacy_editor(settings: &mut pr_core::settings::app_settings::AppSettings) -> bool {
        if !settings.external_editors.is_empty() || settings.external_editor_path.trim().is_empty() {
            return false;
        }
        ExternalEditors::reconcile(settings);
        true
    }

    fn save_external_editor_settings(&self, settings: pr_core::settings::app_settings::AppSettings) {
        let edit = self.settings.begin_edit(|current| *current = settings);
        if let Err(error) = self.settings.commit_edit(edit) {
            self.log
                .error("Could not save the external editors setting", Some(&error));
        }
    }
}
