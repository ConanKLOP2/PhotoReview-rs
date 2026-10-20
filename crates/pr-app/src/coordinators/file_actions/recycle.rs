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
// Fields and helpers here (`report_recycle_failure`, `will_ask_group_confirmation`, `present`, `move_positions`, ...) are read only by
// the sibling files of W8-07d/07e, which are still stubs on this branch; remove this allow when they land.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{DialogService, FileSystem, Log, NaturalComparer};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::types::{CaptureGroupActionResult, FileActionResult, UndoResult};
use pr_core::file_actions::undo::UndoService;
use pr_core::model::FileOperationType;
use pr_core::settings::app_settings::AppSettings;
use pr_l10n::keys;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::{LocArg, NumberCulture};

use super::actions::{AfterCore, InFlightAction, ParkedTail};
use super::undo::{InFlightUndo, MovePosition};
use crate::coordinators::folder_load::state::WorkerPort;
use crate::coordinators::traits::{IFileActionExecutor, IFileActionSink, IPreloadController};

/// Where user-visible sentences come from (D-W8-4). The production implementation reads the CURRENT localizer on each call
/// (C# ambient `Tr`), so a language switch applies at once; tests give a fixed English `Localizer`.
pub trait TextSource: Send + Sync {
    fn localizer(&self) -> Arc<Localizer>;
    fn numbers(&self) -> Arc<NumberCulture>;

    /// `localizer().format_with(key, args, &numbers())`; keys are the `pr_l10n::keys` of the C# `Tr.*` members.
    fn format(&self, key: &str, args: &[LocArg]) -> String {
        self.localizer().format_with(key, args, &self.numbers())
    }

    /// `localizer().get(key)`.
    fn get(&self, key: &str) -> String {
        self.localizer().get(key)
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

type SettingsFn = Box<dyn Fn() -> Arc<AppSettings>>;
type RememberFolderFn = Box<dyn FnMut(FileOperationType, &str)>;

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
    pub get_settings: SettingsFn,
    /// C# `IFileSystem? fileSystem`: used to check a picked/remembered folder and required to act on a capture group.
    pub file_system: Option<Arc<dyn FileSystem>>,
    /// C# `Action<FileOperationType, string>? rememberFolder`.
    pub remember_folder: Option<RememberFolderFn>,
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

/// C# `Path.GetFileName`: the last path component (empty when the path ends in a separator).
fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl FileActionController {
    /// C# constructor. Stores the collaborators; starts nothing. Both `fileSystem` and the `Directory.Exists` fallback are
    /// folded into `deps.file_system` (the fallback is `std::path::Path::is_dir`, used only when `file_system` is `None`).
    pub fn new(deps: FileActionDeps) -> Self {
        Self {
            deps,
            last_partner_missing_warning: None,
            present: PresentState::None,
            last_action_folder_gen: 0,
            move_positions: HashMap::new(),
            in_flight: None,
            undo_in_flight: None,
            parked: None,
            next_ticket: 1,
        }
    }

    /// C# `FileActionsOverride` test seam (`internal`): replaces the executor. `#[doc(hidden)]`.
    #[doc(hidden)]
    pub fn set_file_actions_override(&mut self, executor: Option<Arc<dyn IFileActionExecutor + Send + Sync>>) {
        self.deps.file_actions = executor;
    }

    /// True while an action or an undo is between its start and its `Done` (the caller's gate normally prevents a second one).
    pub fn is_in_flight(&self) -> bool {
        self.in_flight.is_some() || self.undo_in_flight.is_some()
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
        let refused = FileActionStep::Done(FileActionDone::Finished { succeeded: false });
        // Q-R44: general "confirm before delete", off by default. Skipped when the permanent-delete prompt (Q-R8) will already ask.
        let source = compare_selected_path.or(current_path);
        if self.refuse_recycle_without_bin(cx, source) {
            return refused; // before "confirm before delete": the answer would not matter
        }
        let selected_group = source
            .and_then(|s| cx.catalog.find(s))
            .and_then(|e| e.capture_group.clone());
        let permanent_prompt = selected_group.is_none() && self.will_ask_permanent_delete(source);
        if selected_group.is_none()
            && (self.deps.get_settings)().confirm_before_delete
            && !permanent_prompt
            && let Some(dialog) = self.deps.dialog_service.clone()
        {
            let folder_before = self.deps.clock.current_folder();
            let recycle_name = self.deps.texts.get(keys::ACTION_RECYCLE_NAME);
            let ok = dialog.show_confirmation(
                &self.deps.texts.get(keys::DIALOG_CONFIRM_ACTION_TITLE),
                &self.deps.texts.format(
                    keys::DIALOG_CONFIRM_ACTION_MESSAGE,
                    &[LocArg::new("actionName", recycle_name)],
                ),
            );
            if !ok {
                return refused;
            }
            // R7-4: the dialog ran a nested loop; the folder may have changed, another action may have started.
            if self.deps.clock.current_folder() != folder_before
                || self.deps.file_actions.as_ref().is_some_and(|f| f.is_busy())
                || source.is_some_and(|s| cx.catalog.index_of(s) < 0)
            {
                return refused;
            }
        }

        let action_name = self.deps.texts.get(keys::ACTION_RECYCLE_NAME);
        match self.prepare_core(
            cx,
            &action_name,
            FileOperationType::Recycle,
            None,
            compare_selected_path,
            current_path,
        ) {
            CorePrep::Refused => refused,
            CorePrep::Ready(prepared) => self.execute_core(cx, prepared, AfterCore::Nothing),
        }
    }

    /// C# `WillAskGroupConfirmation(operation, compareSelectedPath, source)`. Mirror the prompt condition of the core: keep in step.
    pub(super) fn will_ask_group_confirmation(
        &self,
        cx: &FileActionCx<'_>,
        operation: FileOperationType,
        compare_selected_path: Option<&str>,
        source: Option<&str>,
    ) -> bool {
        let Some(source) = source.filter(|s| !s.is_empty()) else {
            return false;
        };
        let Some(executor) = self.deps.file_actions.as_ref() else {
            return false;
        };
        if self.deps.file_system.is_none() {
            return false;
        }
        let Some(group) = cx.catalog.find(source).and_then(|e| e.capture_group.as_ref()) else {
            return false;
        };
        // C# `compareSelectedPath is not null && operation is Recycle or Move` is `is (Recycle or Move)`.
        if compare_selected_path.is_some() && matches!(operation, FileOperationType::Recycle | FileOperationType::Move)
        {
            return true;
        }
        if operation != FileOperationType::Recycle {
            return false;
        }
        let settings = (self.deps.get_settings)();
        settings.confirm_before_delete
            || (settings.allow_permanent_delete_without_recycle_bin
                && group.paths().iter().any(|p| executor.lacks_recycle_bin(p)))
    }

    /// C# `RefuseRecycleWithoutBin(source)`: true when the Delete was refused (status `CoreRecycleUnsupportedDrive(fileName)`,
    /// error dialog `DialogRecycleNoBin*`, `sink.notify_navigation_state_changed()`); nothing else changed. Checks EVERY member
    /// of a capture (`catalog.find(source).capture_group.paths()`), names the first member without a bin.
    pub(super) fn refuse_recycle_without_bin(&self, cx: &mut FileActionCx<'_>, source: Option<&str>) -> bool {
        let Some(source) = source.filter(|s| !s.is_empty()) else {
            return false;
        };
        let Some(executor) = self.deps.file_actions.as_ref() else {
            return false;
        };
        if cx.catalog.count() == 0 {
            return false;
        }
        if (self.deps.get_settings)().allow_permanent_delete_without_recycle_bin {
            return false; // the permanent-delete prompt handles it
        }
        let paths: Vec<String> = match cx.catalog.find(source).and_then(|e| e.capture_group.as_ref()) {
            Some(group) => group.paths().to_vec(),
            None => vec![source.to_string()],
        };
        let Some(without_bin) = paths.iter().find(|p| executor.lacks_recycle_bin(p)) else {
            return false;
        };

        let file_name = file_name_of(without_bin);
        let texts = &self.deps.texts;
        cx.sink.set_status_text(&texts.format(
            keys::CORE_RECYCLE_UNSUPPORTED_DRIVE,
            &[LocArg::new("name", file_name.clone())],
        ));
        if let Some(dialog) = &self.deps.dialog_service {
            dialog.show_error(
                &texts.get(keys::DIALOG_RECYCLE_NO_BIN_TITLE),
                &texts.format(
                    keys::DIALOG_RECYCLE_NO_BIN_MESSAGE,
                    &[
                        LocArg::new("fileName", file_name),
                        LocArg::new("settingName", texts.get(keys::SETTINGS_ALLOW_PERMANENT_DELETE_LABEL)),
                        LocArg::new("settingsPage", texts.get(keys::SETTINGS_NAV_FILES)),
                    ],
                ),
            );
        }
        cx.sink.notify_navigation_state_changed();
        true
    }

    /// C# `ReportRecycleFailure(source, reason)`: error dialog `DialogRecycleFailed*` (reason `""` when `None`).
    pub(super) fn report_recycle_failure(&self, source: &str, reason: Option<&str>) {
        if let Some(dialog) = &self.deps.dialog_service {
            dialog.show_error(
                &self.deps.texts.get(keys::DIALOG_RECYCLE_FAILED_TITLE),
                &self.deps.texts.format(
                    keys::DIALOG_RECYCLE_FAILED_MESSAGE,
                    &[
                        LocArg::new("fileName", file_name_of(source)),
                        LocArg::new("message", reason.unwrap_or_default()),
                    ],
                ),
            );
        }
    }

    /// C# `WillAskPermanentDelete(source)`: non-empty source, `allow_permanent_delete_without_recycle_bin` on, executor present,
    /// `lacks_recycle_bin(source)`.
    pub(super) fn will_ask_permanent_delete(&self, source: Option<&str>) -> bool {
        let Some(source) = source.filter(|s| !s.is_empty()) else {
            return false;
        };
        (self.deps.get_settings)().allow_permanent_delete_without_recycle_bin
            && self
                .deps
                .file_actions
                .as_ref()
                .is_some_and(|f| f.lacks_recycle_bin(source))
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
        self.last_partner_missing_warning = None;
        self.present = PresentState::None;
        if cx.catalog.count() == 0 {
            return CorePrep::Refused;
        }
        let Some(executor) = self.deps.file_actions.clone() else {
            return CorePrep::Refused;
        };
        // INV-4: the service's own gate is busy.
        if executor.is_busy() {
            return CorePrep::Refused;
        }
        let Some(source) = compare_selected_path.or(current_path).filter(|s| !s.is_empty()) else {
            return CorePrep::Refused;
        };
        let selected_group = cx.catalog.find(source).and_then(|e| e.capture_group.clone());
        if selected_group.is_some() && self.deps.file_system.is_none() {
            return CorePrep::Refused;
        }
        let texts = self.deps.texts.clone();
        let source_name = file_name_of(source);

        // After a dialog (nested loop): the folder may have changed, another action may have started, the photo may be gone.
        let state_changed = |this: &Self, cx: &FileActionCx<'_>, folder_before: i64| {
            this.deps.clock.current_folder() != folder_before || executor.is_busy() || cx.catalog.index_of(source) < 0
        };

        // Q-R8: permanent delete only with the setting ON and an explicit confirmation every time. Without a dialog
        // service nothing can be confirmed, so nothing is deleted.
        let mut allow_permanent = false;
        if operation == FileOperationType::Recycle
            && selected_group.is_none()
            && self.will_ask_permanent_delete(Some(source))
        {
            let folder_before = self.deps.clock.current_folder();
            let Some(dialog) = self.deps.dialog_service.clone() else {
                return CorePrep::Refused;
            };
            if !dialog.show_confirmation(
                &texts.get(keys::DIALOG_CONFIRM_PERMANENT_DELETE_TITLE),
                &texts.format(
                    keys::DIALOG_CONFIRM_PERMANENT_DELETE_MESSAGE,
                    &[LocArg::new("fileName", source_name.clone())],
                ),
            ) {
                return CorePrep::Refused;
            }
            if state_changed(self, cx, folder_before) {
                return CorePrep::Refused;
            }
            allow_permanent = true;
        }

        // Same semantics as the single-file path: the permanent prompt, and with it `allow_permanent`, exist only when the
        // setting is ON and a member really lacks a Recycle Bin (read at use time, R2-F-03).
        let permanent_group_paths: Vec<String> = match &selected_group {
            Some(group)
                if operation == FileOperationType::Recycle
                    && (self.deps.get_settings)().allow_permanent_delete_without_recycle_bin =>
            {
                group
                    .paths()
                    .iter()
                    .filter(|p| executor.lacks_recycle_bin(p))
                    .cloned()
                    .collect()
            }
            _ => Vec::new(),
        };
        // Q-RAW-COMPARE-GROUP: Delete/Move act on the WHOLE capture, so the user is always asked.
        let from_compare_group = compare_selected_path.is_some()
            && selected_group.is_some()
            && matches!(operation, FileOperationType::Recycle | FileOperationType::Move);
        if let Some(group) = &selected_group
            && ((operation == FileOperationType::Recycle
                && ((self.deps.get_settings)().confirm_before_delete || !permanent_group_paths.is_empty()))
                || from_compare_group)
        {
            let folder_before = self.deps.clock.current_folder();
            let member_names = group
                .paths()
                .iter()
                .map(|p| file_name_of(p))
                .collect::<Vec<_>>()
                .join(", ");
            let permanent_members = operation == FileOperationType::Recycle && !permanent_group_paths.is_empty();
            let prompt = if permanent_members {
                texts.format(
                    keys::DIALOG_CONFIRM_GROUP_PERMANENT_DELETE_MESSAGE,
                    &[
                        LocArg::new("permanentCount", permanent_group_paths.len() as i64),
                        LocArg::new("count", group.paths().len() as i64),
                        LocArg::new("fileName", source_name.clone()),
                    ],
                )
            } else if from_compare_group {
                let key = if operation == FileOperationType::Move {
                    keys::DIALOG_CONFIRM_COMPARE_GROUP_MOVE_MESSAGE
                } else {
                    keys::DIALOG_CONFIRM_COMPARE_GROUP_RECYCLE_MESSAGE
                };
                texts.format(
                    key,
                    &[
                        LocArg::new("fileName", source_name.clone()),
                        LocArg::new("files", member_names),
                    ],
                )
            } else {
                texts.format(
                    keys::DIALOG_CONFIRM_GROUP_RECYCLE_MESSAGE,
                    &[
                        LocArg::new("count", group.paths().len() as i64),
                        LocArg::new("fileName", source_name.clone()),
                    ],
                )
            };
            let title = texts.get(if permanent_members {
                keys::DIALOG_CONFIRM_PERMANENT_DELETE_TITLE
            } else {
                keys::DIALOG_CONFIRM_ACTION_TITLE
            });
            let Some(dialog) = self.deps.dialog_service.clone() else {
                return CorePrep::Refused;
            };
            if !dialog.show_confirmation(&title, &prompt) {
                return CorePrep::Refused;
            }
            if state_changed(self, cx, folder_before) {
                return CorePrep::Refused;
            }
            // Only a prompt that NAMED permanent members grants permanent delete; a generic group prompt never does.
            allow_permanent = permanent_members;
        }

        // The confirmations ran nested loops: the entry may have been degraded to a standalone survivor or regrouped.
        // Never act on files that are no longer shown together.
        let group = cx.catalog.find(source).and_then(|e| e.capture_group.clone());
        if group != selected_group {
            cx.sink.set_status_text(&texts.format(
                keys::STATUS_GROUP_CHANGED_DURING_CONFIRM,
                &[LocArg::new("fileName", source_name)],
            ));
            return CorePrep::Refused;
        }

        CorePrep::Ready(PreparedAction {
            action_name: action_name.to_string(),
            operation,
            destination: destination.map(str::to_string),
            source: source.to_string(),
            group,
            allow_permanent,
        })
    }
}
