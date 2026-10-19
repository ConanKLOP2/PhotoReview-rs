//! Port of `PhotoReview.App.Shared/Coordinators/FileActionController.cs` lines 1 to 290: fields, constructor, `RecycleAsync`,
//! the no-recycle-bin refusal, the permanent-delete and group prompts, and the pre-I/O half of `ExecuteFileActionCoreAsync`
//! (contract `contracts/W8-07c.md`). Target: `coordinators::file_actions::recycle`.
//!
//! D-W8-2: every C# `async Task` entry point returns a [`FileActionStep`]: `Done` when it ended synchronously, `Pending` while
//! the file operation runs on `WorkerLane::FileAction`; the worker result comes back as a [`FileActionEvent`] to
//! `handle_event` (`actions.rs`). The catalog and the sink are borrowed per call in a [`FileActionCx`] (D-W8-1). Modal dialogs
//! (`DialogService`, `IFolderPicker`) stay synchronous, exactly like the C# nested dispatcher loop, and the code re-checks
//! its state after each one (R7-4).
//!
//! The file-action GATE (`FileActionGate`, W8-06e) is held by the CALLER (the view model), not by this controller: the C# class
//! never mentions it. `IFileActionExecutor::is_busy` is the service's own gate (INV-4).
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]
// Stub only (contract text in doc comments, boxed seams): the porter fixes the doc layout / introduces type aliases and removes this line.
#![allow(clippy::type_complexity)]

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{DialogService, FileSystem, Log, NaturalComparer};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::types::{CaptureGroupActionResult, FileActionResult, UndoResult};
use pr_core::file_actions::undo::UndoService;
use pr_core::model::FileOperationType;
use pr_core::settings::app_settings::AppSettings;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::{LocArg, NumberCulture};

use super::actions::{InFlightAction, ParkedTail};
use super::undo::{InFlightUndo, MovePosition};
use crate::coordinators::folder_load::state::{WorkerLane, WorkerPort};
use crate::coordinators::traits::{IFileActionExecutor, IFileActionSink, IPreloadController};

/// Where user-visible sentences come from (D-W8-4). The production implementation reads the CURRENT localizer on each call
/// (C# ambient `Tr`), so a language switch applies at once; tests give a fixed English `Localizer`.
pub trait TextSource: Send + Sync {
    fn localizer(&self) -> Arc<Localizer>;
    fn numbers(&self) -> Arc<NumberCulture>;

    /// `localizer().format_with(key, args, &numbers())`; keys are the `pr_l10n::keys` of the C# `Tr.*` members.
    fn format(&self, key: &str, args: &[LocArg]) -> String {
        todo!()
    }

    /// `localizer().get(key)`.
    fn get(&self, key: &str) -> String {
        todo!()
    }
}

/// What every `FileActionController` call needs from the shell, borrowed per call.
pub struct FileActionCx<'a> {
    pub catalog: &'a mut ReviewCatalog,
    pub sink: &'a mut dyn IFileActionSink,
}

impl fmt::Debug for FileActionCx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileActionCx").finish_non_exhaustive()
    }
}

/// How an entry point finished (the C# task result).
#[derive(Debug, Clone, PartialEq)]
pub enum FileActionDone {
    /// C# `Task` / `Task<bool>` of the file-action entry points. `succeeded` = the C# core returned true (the file operation
    /// succeeded and the folder is still the current one); `false` for every refusal, failure and late completion.
    Finished { succeeded: bool },
    /// C# `Task<UndoResult?>` of `UndoLastAsync`: `None` when there is no undo service.
    Undone(Option<UndoResult>),
    /// The executor/undo reported cancellation (C# `OperationCanceledException` escapes to the caller): nothing was restored.
    Canceled,
}

/// Result of an entry point or of an event. The caller releases the file-action gate on `Done`.
#[derive(Debug, Clone, PartialEq)]
pub enum FileActionStep {
    /// The operation is over.
    Done(FileActionDone),
    /// A worker job or a present is still outstanding: wait for `handle_event` / `on_present_finished`.
    Pending,
}

/// What a worker job returns; delivered with `handle_event` on the UI thread. `ticket` identifies the operation (INV-9): an
/// event with another ticket than the in-flight one is dropped.
#[derive(Debug)]
pub enum FileActionEvent {
    /// `ExecuteAsync` / `ExecuteGroupAsync` finished.
    Executed { ticket: u64, outcome: ExecOutcome },
    /// `UndoService::undo_last` finished.
    Undone { ticket: u64, outcome: UndoOutcome },
}

/// The outcome of the executor call. `Panicked` is the Rust image of the C# "unexpected exception" (a service bug or a test
/// fake that throws): the worker port turns a panic into it (R0003). Text = `UserFacingError.Describe`.
#[derive(Debug)]
pub enum ExecOutcome {
    Single(FileActionResult),
    Group(CaptureGroupActionResult),
    /// C# `OperationCanceledException`.
    Canceled,
    Panicked(String),
}

/// The outcome of `UndoService::undo_last`.
#[derive(Debug)]
pub enum UndoOutcome {
    Result(UndoResult),
    Canceled,
    Panicked(String),
}

/// The status of the next-photo `present` the last action started (C# `Task? presentTask` / `_lastPresentTask`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentState {
    /// No present was requested (C# `null`).
    None,
    /// `sink.present` was called; the shell has not reported `on_present_finished` yet.
    Running,
    /// Finished. `faulted` = the C# task faulted (logged, never replaces the action's own error text).
    Finished { faulted: bool },
}

/// The collaborators of the C# constructor. `catalog` and `sink` arrive per call (see [`FileActionCx`]).
pub struct FileActionDeps {
    pub clock: Arc<GenerationClock>,
    /// C# `FileActionService? fileActionService` wrapped in `FileActionServiceExecutor`. `None` disables every file action.
    pub file_actions: Option<Arc<dyn IFileActionExecutor + Send + Sync>>,
    /// C# `UndoService? undoService`, shared with the file-action lane (`UndoService` is `Send`; its methods take `&mut self`).
    pub undo_service: Option<Arc<Mutex<UndoService>>>,
    pub dialog_service: Option<Arc<dyn DialogService>>,
    pub preload_controller: Option<Arc<dyn IPreloadController>>,
    pub natural_comparer: Arc<dyn NaturalComparer>,
    /// C# `Func<AppSettings> getSettings`, read at USE time (Settings > Save replaces the instance, R2-F-03).
    pub get_settings: Box<dyn Fn() -> Arc<AppSettings>>,
    /// C# `IFileSystem? fileSystem`: used to check a picked/remembered folder and required to act on a capture group.
    pub file_system: Option<Arc<dyn FileSystem>>,
    /// C# `Action<FileOperationType, string>? rememberFolder`.
    pub remember_folder: Option<Box<dyn FnMut(FileOperationType, &str)>>,
    pub worker: Arc<dyn WorkerPort<FileActionEvent>>,
    pub texts: Arc<dyn TextSource>,
    pub log: Arc<dyn Log>,
}

impl fmt::Debug for FileActionDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileActionDeps").finish_non_exhaustive()
    }
}

/// Result of the pre-I/O half of the core (lines 207 to 276).
#[derive(Debug)]
pub(super) enum CorePrep {
    /// C# `return false` before anything changed (busy, no source, prompt declined, state changed, group changed).
    Refused,
    /// All checks passed; `actions.rs` continues with the catalog removal and the executor call.
    Ready(PreparedAction),
}

/// The values the C# core computed before it touched the catalog.
#[derive(Debug)]
pub(super) struct PreparedAction {
    pub action_name: String,
    pub operation: FileOperationType,
    pub destination: Option<String>,
    pub source: String,
    /// The capture group of `source` after the dialogs (equal to the one before them, else the prep is `Refused`).
    pub group: Option<CaptureGroup>,
    /// C# `allowPermanent` (true only after an explicit permanent-delete confirmation).
    pub allow_permanent: bool,
}

/// C# `FileActionController`. Owned by the UI thread; not `Send`.
pub struct FileActionController {
    pub(super) deps: FileActionDeps,
    /// C# `_lastPartnerMissingWarning`.
    pub(super) last_partner_missing_warning: Option<String>,
    /// C# `_lastPresentTask`.
    pub(super) present: PresentState,
    /// C# `_lastActionFolderGen`.
    pub(super) last_action_folder_gen: i64,
    /// C# `_movePositions`, keyed by `ignore_case_key(path)`.
    pub(super) move_positions: HashMap<String, MovePosition>,
    pub(super) in_flight: Option<InFlightAction>,
    pub(super) undo_in_flight: Option<InFlightUndo>,
    /// The continuation waiting for the present to finish (C# `await presentTask`).
    pub(super) parked: Option<ParkedTail>,
    pub(super) next_ticket: u64,
}

impl fmt::Debug for FileActionController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileActionController")
            .field("present", &self.present)
            .field("in_flight", &self.in_flight.is_some())
            .field("undo_in_flight", &self.undo_in_flight.is_some())
            .field("remembered_move_positions", &self.move_positions.len())
            .finish_non_exhaustive()
    }
}

impl FileActionController {
    /// C# constructor. Stores the collaborators; starts nothing. Both `fileSystem` and the `Directory.Exists` fallback are
    /// folded into `deps.file_system` (the fallback is `std::path::Path::is_dir`, used only when `file_system` is `None`).
    pub fn new(deps: FileActionDeps) -> Self {
        todo!()
    }

    /// C# `FileActionsOverride` test seam (`internal`): replaces the executor. `#[doc(hidden)]`.
    #[doc(hidden)]
    pub fn set_file_actions_override(&mut self, executor: Option<Arc<dyn IFileActionExecutor + Send + Sync>>) {
        todo!()
    }

    /// True while an action or an undo is between its start and its `Done` (the caller's gate normally prevents a second one).
    pub fn is_in_flight(&self) -> bool {
        todo!()
    }

    /// C# `RecycleAsync(compareSelectedPath, currentPath)`. Order: `refuse_recycle_without_bin(source)` first; then, when the
    /// source is not a capture, "confirm before delete" is on, a dialog service exists and no permanent-delete prompt will
    /// follow: `show_confirmation(DialogConfirmActionTitle, DialogConfirmActionMessage(ActionRecycleName))`; declined, or the
    /// folder generation moved, or the executor is busy, or the source left the catalog during the dialog -> `Done(false)`.
    /// Then the shared core with `FileOperationType::Recycle` and the localized action name.
    pub fn recycle(
        &mut self,
        cx: &mut FileActionCx<'_>,
        compare_selected_path: Option<&str>,
        current_path: Option<&str>,
    ) -> FileActionStep {
        todo!()
    }

    /// C# `WillAskGroupConfirmation(operation, compareSelectedPath, source)`. Mirror the prompt condition of the core: keep in step.
    pub(super) fn will_ask_group_confirmation(
        &self,
        cx: &FileActionCx<'_>,
        operation: FileOperationType,
        compare_selected_path: Option<&str>,
        source: Option<&str>,
    ) -> bool {
        todo!()
    }

    /// C# `RefuseRecycleWithoutBin(source)`: true when the Delete was refused (status `CoreRecycleUnsupportedDrive(fileName)`,
    /// error dialog `DialogRecycleNoBin*`, `sink.notify_navigation_state_changed()`); nothing else changed. Checks EVERY member
    /// of a capture (`catalog.find(source).capture_group.paths()`), names the first member without a bin.
    pub(super) fn refuse_recycle_without_bin(&self, cx: &mut FileActionCx<'_>, source: Option<&str>) -> bool {
        todo!()
    }

    /// C# `ReportRecycleFailure(source, reason)`: error dialog `DialogRecycleFailed*` (reason `""` when `None`).
    pub(super) fn report_recycle_failure(&self, source: &str, reason: Option<&str>) {
        todo!()
    }

    /// C# `WillAskPermanentDelete(source)`: non-empty source, `allow_permanent_delete_without_recycle_bin` on, executor present,
    /// `lacks_recycle_bin(source)`.
    pub(super) fn will_ask_permanent_delete(&self, source: Option<&str>) -> bool {
        todo!()
    }

    /// C# `ExecuteFileActionCoreAsync` lines 207 to 276 (everything before `sourceIndex`): resets
    /// `last_partner_missing_warning` and `present`; refuses on empty catalog, no executor, busy executor, empty source, capture
    /// group without a file system; permanent-delete prompt (single file); group prompt (title/message rules of lines 247 to
    /// 267); each dialog is followed by the R7-4 re-check (folder generation, busy, source still in the catalog). Finally the
    /// group must equal the group seen before the dialogs, else status `StatusGroupChangedDuringConfirm(fileName)` and `Refused`.
    pub(super) fn prepare_core(
        &mut self,
        cx: &mut FileActionCx<'_>,
        action_name: &str,
        operation: FileOperationType,
        destination: Option<&str>,
        compare_selected_path: Option<&str>,
        current_path: Option<&str>,
    ) -> CorePrep {
        todo!()
    }
}
