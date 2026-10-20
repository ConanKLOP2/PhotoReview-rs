//! Port of `PhotoReview.App.Shared/Coordinators/FileActionController.cs` lines 68 to 116 (`RunActionAsync`, the action
//! profiles) and 289 to 527 plus 592 to 617 (the second half of `ExecuteFileActionCoreAsync` and `ReportLateCompletion`)
//! (contract `contracts/W8-07d.md`). Target: `coordinators::file_actions::actions`.
//!
//! The C# core is one `async` method with six `await` points. Here it is cut at those points: `execute_core` runs up to the
//! executor call (INV-3: the next photo is requested BEFORE the I/O), the worker runs the executor, `handle_event` runs the
//! result handling, and the `await presentTask` points become a parked continuation ([`ParkedTail`]) that
//! `on_present_finished` resumes (RV-A09, R08).
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, FileActionRequest, FileActionResult,
};
use pr_core::model::FileOperationType;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{get_directory_name, get_file_name};
use pr_core::text::eq_ignore_case;
use pr_l10n::keys;
use pr_l10n::template::LocArg;

use super::move_copy::MoveCopyTail;
use super::recycle::{
    CorePrep, ExecOutcome, FileActionController, FileActionCx, FileActionDone, FileActionEvent, FileActionStep,
    PreparedAction, PresentState,
};
use super::undo::UndoTail;
use crate::coordinators::folder_load::state::WorkerLane;

/// What the core remembers between the executor call and its result (the C# locals that live across `await`).
#[derive(Debug)]
pub(super) struct InFlightAction {
    pub ticket: u64,
    pub action_name: String,
    pub operation: FileOperationType,
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

/// The recycle-failure error dialog a failure tail shows: `(source, reason)` of `report_recycle_failure`.
type RecycleFailure = Option<(String, Option<String>)>;

/// A continuation of the core that C# runs after `await presentTask`. Each variant is one `await` site of the C# core; every
/// one carries the `folder_gen` the action ran in (R08: nothing is written into a folder opened since).
#[derive(Debug)]
pub(super) enum CoreTail {
    /// Lines 397 to 403: a partner file vanished; status `StatusGroupPartnerMissing(..)` (prefixed by `AllImagesProcessed + " "`
    /// when a remove left the catalog empty, decided AFTER the present), only while the folder is still current. Result `Finished{true}`.
    PartnerMissing {
        folder_gen: i64,
        file_name: String,
        missing: String,
        is_remove: bool,
        /// Move-to / Copy-to: what runs after this tail ended the core (C# code after `ExecuteFileActionCoreAsync` returned true).
        then: Option<MoveCopyTail>,
    },
    /// Lines 420 to 428: Move failed verification after the source was removed: if still current, `update_session_path`
    /// then status `StatusMoveUnverified(fileName)`. Result `Finished{false}`.
    UnverifiedMove { folder_gen: i64, source: String },
    /// Lines 457 to 473: failure after a (maybe faulted) present: if still current, status `ActionFailed(name, failure)` and,
    /// for a Recycle that completed no member, the `DialogRecycleFailed*` error dialog. Result `Finished{false}`.
    Failure {
        folder_gen: i64,
        status: String,
        recycle_failure: RecycleFailure,
    },
    /// Lines 485 to 496: the action succeeded but its bookkeeping threw: if still current, status `ActionCompleted(name)`.
    /// Result `Finished{true}` while current, else `Finished{false}`.
    CompletedAfterBookkeepingFailure { folder_gen: i64, status: String },
    /// Lines 510 to 521: the executor threw (the entry was restored): if still current, status `ActionFailed(name, text)`.
    ExecutorThrew {
        folder_gen: i64,
        status: String,
        recycle_failure: RecycleFailure,
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

/// The text of a caught panic (the Rust image of `UserFacingError.Describe(ex)` for an unexpected exception).
pub(super) fn describe_panic(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unexpected error".to_string()
    }
}

fn done(succeeded: bool) -> FileActionStep {
    FileActionStep::Done(FileActionDone::Finished { succeeded })
}

/// How the success branch ended: synchronously (the core returned true) or with the step of a tail.
enum SuccessEnd {
    Sync,
    Step(FileActionStep),
}

/// The result of the executor in the shape the post-I/O code needs.
enum IoResult {
    Single(FileActionResult),
    Group(CaptureGroupActionResult),
}

impl IoResult {
    fn succeeded(&self) -> bool {
        match self {
            IoResult::Single(r) => r.succeeded,
            IoResult::Group(r) => r.succeeded,
        }
    }
}

impl FileActionController {
    fn status_of(&self, key: &str, args: &[LocArg]) -> String {
        self.deps.texts.format(key, args)
    }

    /// Runs `f` on the undo service when there is one (a poisoned lock is recovered, R0003). Never call the sink inside `f`.
    pub(super) fn with_undo<R>(&self, f: impl FnOnce(&mut pr_core::file_actions::undo::UndoService) -> R) -> Option<R> {
        let undo = self.deps.undo_service.as_ref()?;
        let mut guard = undo.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Some(f(&mut guard))
    }

    /// C# `RunActionAsync(index, compareSelectedPath, currentPath)`. Returns `Done(Finished{false})` (C# `return`) when:
    /// the catalog is empty; `index` is outside the settings' action list; a Recycle that `refuse_recycle_without_bin`; the
    /// generic confirmation (`action.confirm`, a dialog service, no permanent and no group prompt coming) is declined or the
    /// state changed during it; the source is empty; a Move/Copy has a blank destination (status `StatusActionNoDestination(name)`).
    /// Otherwise the core runs (`Recycle` ignores the destination).
    ///
    /// The C# `Enum.IsDefined(action.Operation)` arm (status `StatusActionInvalidOperation`) does not exist here:
    /// `FileOperationType` is a closed Rust enum and the settings loader maps an unknown value to Move (S-18).
    pub fn run_action(
        &mut self,
        cx: &mut FileActionCx<'_>,
        index: i32,
        compare_selected_path: Option<&str>,
        current_path: Option<&str>,
    ) -> FileActionStep {
        if cx.catalog.count() == 0 {
            return done(false);
        }
        // Read at use time: Settings > Save replaces the AppSettings instance (R2-F-03).
        let settings = (self.deps.get_settings)();
        let Some(action) = usize::try_from(index).ok().and_then(|i| settings.actions.get(i)) else {
            return done(false);
        };

        let target = compare_selected_path.or(current_path);
        // A Delete that will be refused anyway (no Recycle Bin, permanent delete off) is refused before any confirmation.
        if action.operation == FileOperationType::Recycle && self.refuse_recycle_without_bin(cx, target) {
            return done(false);
        }

        // Q-R8: a permanent delete gets its own explicit confirmation (in the core step), which replaces the generic one.
        let permanent_prompt = action.operation == FileOperationType::Recycle && self.will_ask_permanent_delete(target);
        // A capture that gets its own confirmation in the core step must not be asked twice.
        let group_prompt = self.will_ask_group_confirmation(cx, action.operation, compare_selected_path, target);
        if action.confirm
            && !permanent_prompt
            && !group_prompt
            && let Some(dialog) = self.deps.dialog_service.clone()
        {
            // R7-4: the dialog runs a nested loop (a forwarded open can switch the folder meanwhile): re-check afterwards.
            let folder_before = self.deps.clock.current_folder();
            let ok = dialog.show_confirmation(
                &self.deps.texts.get(keys::DIALOG_CONFIRM_ACTION_TITLE),
                &self.status_of(
                    keys::DIALOG_CONFIRM_ACTION_MESSAGE,
                    &[LocArg::new("actionName", action.name.clone())],
                ),
            );
            if !ok {
                return done(false);
            }
            if self.deps.clock.current_folder() != folder_before
                || self.deps.file_actions.as_ref().is_some_and(|f| f.is_busy())
                || target.is_some_and(|t| cx.catalog.index_of(t) < 0)
            {
                return done(false);
            }
        }

        if action.operation == FileOperationType::Recycle {
            return match self.prepare_core(
                cx,
                &action.name,
                FileOperationType::Recycle,
                None,
                compare_selected_path,
                current_path,
            ) {
                CorePrep::Refused => done(false),
                CorePrep::Ready(prepared) => self.execute_core(cx, prepared, AfterCore::Nothing),
            };
        }

        if target.is_none_or(str::is_empty) {
            return done(false);
        }
        if action.destination.trim().is_empty() {
            cx.sink.set_status_text(&self.status_of(
                keys::STATUS_ACTION_NO_DESTINATION,
                &[LocArg::new("actionName", action.name.clone())],
            ));
            return done(false);
        }

        match self.prepare_core(
            cx,
            &action.name,
            action.operation,
            Some(&action.destination),
            compare_selected_path,
            current_path,
        ) {
            CorePrep::Refused => done(false),
            CorePrep::Ready(prepared) => self.execute_core(cx, prepared, AfterCore::Nothing),
        }
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
        let Some(executor) = self.deps.file_actions.clone() else {
            return done(false);
        };
        let PreparedAction {
            action_name,
            operation,
            destination,
            source,
            group,
            allow_permanent,
        } = prepared;
        let source_index = cx.catalog.index_of(&source);
        let is_remove = matches!(operation, FileOperationType::Move | FileOperationType::Recycle);
        // Only a Move/Recycle takes the file out of the catalog and re-presents the next photo (which also restarts preload).
        // A Copy changes nothing on screen, so it must not invalidate the in-flight present or the preload window.
        if is_remove {
            self.deps.clock.stop_for_action();
            if let Some(preload) = &self.deps.preload_controller {
                preload.cancel();
            }
        }

        let folder_gen = self.deps.clock.current_folder();
        self.last_action_folder_gen = folder_gen;
        // Neighbour that precedes the source in the review order: Undo of a Move puts the photo back after it.
        let previous_path = if is_remove && source_index > 0 {
            usize::try_from(source_index - 1)
                .ok()
                .map(|i| cx.catalog.path_at(i).to_string())
        } else {
            None
        };

        if is_remove {
            let next_index = cx.catalog.remove(&source);
            cx.sink.on_catalog_changed(Some(&source));
            // A capture leaves the catalog as a whole: its partner members' cached previews go with it.
            if let Some(group) = &group {
                let partners: Vec<String> = group
                    .paths()
                    .iter()
                    .filter(|p| !eq_ignore_case(p, &source))
                    .cloned()
                    .collect();
                cx.sink.evict_cached_paths(&partners);
            }
            // INV-3: present the next photo BEFORE the file operation completes, without waiting.
            if next_index >= 0 {
                cx.sink.present(next_index);
                self.present = PresentState::Running;
            } else {
                cx.sink
                    .set_status_text(&self.deps.texts.get(keys::STATUS_ALL_IMAGES_PROCESSED));
            }
        }

        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.in_flight = Some(InFlightAction {
            ticket,
            action_name,
            operation,
            source: source.clone(),
            source_index,
            previous_path,
            group: group.clone(),
            folder_gen,
            is_remove,
            after,
        });

        let job = Box::new(move || {
            let run = catch_unwind(AssertUnwindSafe(|| {
                let cancel = CancelToken::none();
                match group {
                    None => {
                        let request = FileActionRequest {
                            source,
                            operation,
                            destination,
                            allow_permanent_delete: allow_permanent,
                        };
                        ExecOutcome::Single(executor.execute(&request, &cancel))
                    }
                    Some(group) => {
                        let request = CaptureGroupActionRequest {
                            group,
                            operation,
                            destination,
                            allow_permanent_delete: allow_permanent,
                        };
                        ExecOutcome::Group(executor.execute_group(&request, &cancel))
                    }
                }
            }));
            let outcome = run.unwrap_or_else(|payload| ExecOutcome::Panicked(describe_panic(payload.as_ref())));
            FileActionEvent::Executed { ticket, outcome }
        });
        self.deps.worker.submit(WorkerLane::FileAction, job);
        FileActionStep::Pending
    }

    /// Delivers a worker result (UI thread). A ticket other than the in-flight one is dropped (returns `Pending`).
    /// `Executed` -> `on_executed` (this file); `Undone` -> `on_undone` (`undo.rs`).
    pub fn handle_event(&mut self, cx: &mut FileActionCx<'_>, event: FileActionEvent) -> FileActionStep {
        match event {
            FileActionEvent::Executed { ticket, outcome } => self.on_executed(cx, ticket, outcome),
            FileActionEvent::Undone { ticket, outcome } => self.on_undone(cx, ticket, outcome),
        }
    }

    /// The shell reports that the `sink.present(..)` this controller requested has finished (`faulted` = the present failed;
    /// a fault is logged, never shown). Resumes the parked continuation when there is one; otherwise only records the state
    /// and returns `Pending` (nothing for the caller to release: its `Done` already came, or the action is still running).
    pub fn on_present_finished(&mut self, cx: &mut FileActionCx<'_>, faulted: bool) -> FileActionStep {
        self.present = PresentState::Finished { faulted };
        if faulted {
            self.deps.log.error("Present after failed file action threw", None);
        }
        match self.parked.take() {
            Some(ParkedTail::Core(tail)) => self.run_core_tail(cx, tail),
            Some(ParkedTail::Undo(tail)) => self.run_undo_tail(cx, tail),
            None => FileActionStep::Pending,
        }
    }

    /// `await presentTask` (C#): run the tail now when the present is over or was never requested, else park it.
    fn await_present(&mut self, cx: &mut FileActionCx<'_>, tail: CoreTail) -> FileActionStep {
        if self.present == PresentState::Running {
            self.parked = Some(ParkedTail::Core(tail));
            FileActionStep::Pending
        } else {
            self.run_core_tail(cx, tail)
        }
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
        outcome: ExecOutcome,
    ) -> FileActionStep {
        if !self.in_flight.as_ref().is_some_and(|f| f.ticket == ticket) {
            return FileActionStep::Pending; // INV-9: a result of an older operation
        }
        let Some(flight) = self.in_flight.take() else {
            return FileActionStep::Pending;
        };
        let result = match outcome {
            // A cancelled executor is not a failure: no restore, no status (C# lets the exception escape; `finally` still runs).
            ExecOutcome::Canceled => {
                cx.sink.notify_navigation_state_changed();
                return FileActionStep::Done(FileActionDone::Canceled);
            }
            ExecOutcome::Panicked(text) => return self.executor_threw(cx, flight, &text, false),
            ExecOutcome::Single(r) => IoResult::Single(r),
            ExecOutcome::Group(r) => IoResult::Group(r),
        };
        let succeeded = result.succeeded();
        let operation = flight.operation;
        let source_removed = match &result {
            IoResult::Group(g) => {
                g.members.iter().any(|m| m.completed)
                    && matches!(operation, FileOperationType::Move | FileOperationType::Recycle)
            }
            IoResult::Single(s) => s.source_removed,
        };

        // Stale Folder Guard: the user switched folder while the I/O ran. The new folder's catalog, session path, current image
        // and gate state must not be touched. APP-03 (Q-R25 B): a SUCCESSFUL action is real on disk, so it is registered for Undo
        // and the user is told. On failure nothing changed on disk (except an unverified Move): no undo, no INV-5 restore.
        if !self.deps.clock.is_folder_current(flight.folder_gen) {
            self.report_late(cx, &flight, &result, source_removed);
            cx.sink.notify_navigation_state_changed();
            return done(false);
        }

        if succeeded {
            // A throw in the bookkeeping must not be treated as a failed action: the file operation is already real.
            let attempt = catch_unwind(AssertUnwindSafe(|| self.finish_success(cx, &flight, &result)));
            return match attempt {
                Ok(SuccessEnd::Step(step)) => step,
                // The core returned true: Move-to / Copy-to continues with its own final steps (outside the `try`, like C#).
                Ok(SuccessEnd::Sync) => match flight.after {
                    AfterCore::MoveCopyTo(tail) => self.move_copy_after_core(cx, tail),
                    AfterCore::Nothing => done(true),
                },
                Err(payload) => self.executor_threw(cx, flight, &describe_panic(payload.as_ref()), true),
            };
        }

        // F3: a Move that failed verification (size differs) after the source was already removed. It must NOT go back into
        // the catalog; no Undo is registered.
        if operation == FileOperationType::Move && source_removed && matches!(result, IoResult::Single(_)) {
            let tail = CoreTail::UnverifiedMove {
                folder_gen: flight.folder_gen,
                source: flight.source.clone(),
            };
            return self.await_present(cx, tail);
        }

        // A Delete that failed part-way: the members already in the Recycle Bin get a Ctrl+Z (INV-6), BEFORE the catalog restore.
        if let IoResult::Group(group_result) = &result {
            self.with_undo(|u| u.register_group(group_result));
        }

        // INV-5: on failure the source goes back into the catalog.
        if flight.is_remove && flight.source_index >= 0 {
            match (&result, &flight.group) {
                (IoResult::Group(group_result), Some(group)) => {
                    // No member state at all: nothing changed on disk, the whole capture returns as one grouped entry. Otherwise
                    // exactly the image members still on disk return (a member whose state is unknown also returns).
                    let available: Vec<String> = if group_result.members.is_empty() {
                        group.image_paths().to_vec()
                    } else {
                        group_result
                            .members
                            .iter()
                            .filter(|m| {
                                group.image_paths().iter().any(|p| eq_ignore_case(p, &m.member.source))
                                    && (!m.state_known || m.source_exists)
                            })
                            .map(|m| m.member.source.clone())
                            .collect()
                    };
                    let raw = (self.deps.get_settings)().raw_support_enabled;
                    cx.catalog
                        .restore_members(&available, flight.source_index, Some(group), raw);
                    cx.sink.on_catalog_changed(None);
                }
                _ => {
                    cx.catalog.restore(&flight.source, flight.source_index);
                }
            }
        }

        let (failure, completed_any) = match &result {
            IoResult::Group(g) => (g.error.clone(), g.members.iter().any(|m| m.completed)),
            IoResult::Single(s) => (s.error.clone(), false),
        };
        let status = self.status_of(
            keys::STATUS_ACTION_FAILED,
            &[
                LocArg::new("actionName", flight.action_name.clone()),
                LocArg::new("message", failure.clone().unwrap_or_default()),
            ],
        );
        // Only when nothing at all was deleted: a capture whose members partly reached the Recycle Bin keeps its status text.
        let recycle_failure =
            (operation == FileOperationType::Recycle && !completed_any).then(|| (flight.source.clone(), failure));
        self.await_present(
            cx,
            CoreTail::Failure {
                folder_gen: flight.folder_gen,
                status,
                recycle_failure,
            },
        )
    }

    /// The late-completion branch of the core (lines 340 to 384). Writes no catalog change, no session path.
    fn report_late(
        &mut self,
        cx: &mut FileActionCx<'_>,
        flight: &InFlightAction,
        result: &IoResult,
        source_removed: bool,
    ) {
        let operation = flight.operation;
        let name = get_file_name(&flight.source).to_string();
        match result {
            IoResult::Group(group_result) => {
                self.with_undo(|u| u.register_group(group_result)); // also the completed members of a part-way failed Delete
                if group_result.succeeded {
                    let target = group_result.entry.as_ref().and_then(|e| e.destination.clone());
                    if operation == FileOperationType::Move && target.is_some() {
                        let folder = target.as_deref().and_then(get_directory_name).unwrap_or_default();
                        cx.sink.show_late_action_status(&self.status_of(
                            keys::STATUS_LATE_MOVE_UNDOABLE,
                            &[LocArg::new("fileName", name.clone()), LocArg::new("folder", folder)],
                        ));
                    } else if operation == FileOperationType::Recycle {
                        // `permanently_deleted` is true when ANY member went for good: say exactly which ones.
                        let permanent_names: Vec<String> = group_result
                            .members
                            .iter()
                            .filter(|m| m.completed && m.member.permanent)
                            .map(|m| get_file_name(&m.member.source).to_string())
                            .collect();
                        let recycled_any = group_result.members.iter().any(|m| m.completed && !m.member.permanent);
                        let text = if !permanent_names.is_empty() && recycled_any {
                            self.status_of(
                                keys::STATUS_LATE_DELETED_PARTLY_PERMANENTLY,
                                &[
                                    LocArg::new("fileName", name.clone()),
                                    LocArg::new("permanentFiles", permanent_names.join(", ")),
                                ],
                            )
                        } else if !permanent_names.is_empty() {
                            self.status_of(
                                keys::STATUS_LATE_DELETED_PERMANENTLY,
                                &[LocArg::new("fileName", name.clone())],
                            )
                        } else {
                            self.status_of(
                                keys::STATUS_LATE_RECYCLE_UNDOABLE,
                                &[LocArg::new("fileName", name.clone())],
                            )
                        };
                        cx.sink.show_late_action_status(&text);
                    }
                }
                let processed = group_result.members.iter().filter(|m| m.completed).count();
                if !group_result.succeeded && operation == FileOperationType::Recycle && processed > 0 {
                    // A Delete that failed part-way after the folder switch: the user is not looking at this folder any more.
                    cx.sink.show_late_action_status(&self.status_of(
                        keys::STATUS_LATE_GROUP_PARTIALLY_PROCESSED,
                        &[
                            LocArg::new("fileName", name),
                            LocArg::new("done", processed as i64),
                            LocArg::new("total", group_result.members.len() as i64),
                        ],
                    ));
                }
            }
            IoResult::Single(single) => {
                if single.succeeded {
                    self.report_late_completion(cx, single);
                } else if operation == FileOperationType::Move && source_removed {
                    // D-04: a Move whose destination could not be verified after the source was removed.
                    cx.sink.show_late_action_status(
                        &self.status_of(keys::STATUS_MOVE_UNVERIFIED, &[LocArg::new("fileName", name)]),
                    );
                }
            }
        }
    }

    /// The success branch (lines 386 to 410). A tail may park (`SuccessEnd::Step`).
    fn finish_success(&mut self, cx: &mut FileActionCx<'_>, flight: &InFlightAction, result: &IoResult) -> SuccessEnd {
        match result {
            IoResult::Group(g) => {
                self.with_undo(|u| u.register_group(g));
            }
            IoResult::Single(s) => {
                self.with_undo(|u| u.register(s));
            }
        }
        // Move AND Recycle remember where the entry sat: a partly failed undo puts the members back there.
        if flight.is_remove && flight.source_index >= 0 {
            self.remember_move_position(&flight.source, flight.previous_path.as_deref(), flight.group.as_ref());
        }
        let session = cx
            .catalog
            .current()
            .map_or_else(|| flight.source.clone(), |e| e.path().to_string());
        cx.sink.update_session_path(&session);

        let skipped = match result {
            IoResult::Group(g) => g.skipped_missing.as_deref().filter(|s| !s.is_empty()),
            IoResult::Single(_) => None,
        };
        if let Some(skipped) = skipped {
            // A partner file vanished outside the app: the action covered the remaining files. Say so after the presenter
            // (which writes its own status when it finishes), only while this folder is still the open one.
            let missing = skipped.iter().map(|p| get_file_name(p)).collect::<Vec<_>>().join(", ");
            let then = match &flight.after {
                AfterCore::MoveCopyTo(tail) => Some(tail.clone()),
                AfterCore::Nothing => None,
            };
            return SuccessEnd::Step(self.await_present(
                cx,
                CoreTail::PartnerMissing {
                    folder_gen: flight.folder_gen,
                    file_name: get_file_name(&flight.source).to_string(),
                    missing,
                    is_remove: flight.is_remove,
                    then,
                },
            ));
        }
        if cx.catalog.count() == 0 {
            let text = if flight.is_remove {
                self.deps.texts.get(keys::STATUS_ALL_IMAGES_PROCESSED)
            } else {
                self.status_of(
                    keys::STATUS_ACTION_COMPLETED,
                    &[LocArg::new("actionName", flight.action_name.clone())],
                )
            };
            cx.sink.set_status_text(&text);
        } else if flight.operation == FileOperationType::Copy {
            let destination = match result {
                IoResult::Group(g) => g.entry.as_ref().and_then(|e| e.destination.clone()),
                IoResult::Single(s) => s.destination_path.clone(),
            }
            .unwrap_or_default();
            cx.sink.set_status_text(&self.status_of(
                keys::STATUS_COPIED_TO,
                &[LocArg::new("fileName", get_file_name(&destination).to_string())],
            ));
        }
        cx.sink.notify_navigation_state_changed();
        SuccessEnd::Sync
    }

    /// The `catch` of the C# core: the executor threw (`bookkeeping == false`) or the bookkeeping after a success threw.
    fn executor_threw(
        &mut self,
        cx: &mut FileActionCx<'_>,
        flight: InFlightAction,
        text: &str,
        bookkeeping: bool,
    ) -> FileActionStep {
        let operation = flight.operation;
        if bookkeeping {
            // The action itself succeeded on disk; only the bookkeeping after it threw. Restoring the entry would show a file
            // that is gone and ActionFailed would be wrong: log it and leave the state as-is.
            self.deps.log.error(
                &format!(
                    "File action {operation:?} succeeded but its post-processing threw: {}: {text}",
                    flight.source
                ),
                None,
            );
            let status = self.status_of(
                keys::STATUS_ACTION_COMPLETED,
                &[LocArg::new("actionName", flight.action_name.clone())],
            );
            return self.await_present(
                cx,
                CoreTail::CompletedAfterBookkeepingFailure {
                    folder_gen: flight.folder_gen,
                    status,
                },
            );
        }

        self.deps.log.error(
            &format!("File action {operation:?} threw: {}: {text}", flight.source),
            None,
        );
        // The entry already left the catalog before the executor ran: restore it (same as INV-5), but only into the folder the
        // action ran in.
        if flight.is_remove && flight.source_index >= 0 && self.deps.clock.is_folder_current(flight.folder_gen) {
            match &flight.group {
                None => {
                    cx.catalog.restore(&flight.source, flight.source_index);
                }
                Some(group) => {
                    let raw = (self.deps.get_settings)().raw_support_enabled;
                    cx.catalog
                        .restore_members(group.image_paths(), flight.source_index, Some(group), raw);
                    cx.sink.on_catalog_changed(None);
                }
            }
        }
        let status = self.status_of(
            keys::STATUS_ACTION_FAILED,
            &[
                LocArg::new("actionName", flight.action_name.clone()),
                LocArg::new("message", text.to_string()),
            ],
        );
        let recycle_failure =
            (operation == FileOperationType::Recycle).then(|| (flight.source.clone(), Some(text.to_string())));
        self.await_present(
            cx,
            CoreTail::ExecutorThrew {
                folder_gen: flight.folder_gen,
                status,
                recycle_failure,
            },
        )
    }

    /// Runs a parked core continuation (after the present finished or when it had already finished). Every tail first checks
    /// `clock.is_folder_current(folder_gen)` (R08) before it writes a session path or a status into the (maybe new) folder.
    /// The `finally` of the C# core (`notify_navigation_state_changed`) runs when the tail ends.
    pub(super) fn run_core_tail(&mut self, cx: &mut FileActionCx<'_>, tail: CoreTail) -> FileActionStep {
        // The final status of Move-to / Copy-to runs after the core's own `finally`, which already happened.
        let tail = match tail {
            CoreTail::MoveCopyFinal(move_copy) => return self.run_move_copy_final(cx, &move_copy, true),
            other => other,
        };
        let mut follow_up: Option<MoveCopyTail> = None;
        let step = match tail {
            CoreTail::PartnerMissing {
                folder_gen,
                file_name,
                missing,
                is_remove,
                then,
            } => {
                follow_up = then;
                if self.deps.clock.is_folder_current(folder_gen) {
                    let missing_text = self.status_of(
                        keys::STATUS_GROUP_PARTNER_MISSING,
                        &[LocArg::new("fileName", file_name), LocArg::new("missing", missing)],
                    );
                    self.last_partner_missing_warning = Some(missing_text.clone());
                    let status = if is_remove && cx.catalog.count() == 0 {
                        format!(
                            "{} {missing_text}",
                            self.deps.texts.get(keys::STATUS_ALL_IMAGES_PROCESSED)
                        )
                    } else {
                        missing_text
                    };
                    cx.sink.set_status_text(&status);
                }
                done(true)
            }
            CoreTail::UnverifiedMove { folder_gen, source } => {
                // R08: the present awaited; a folder opened meanwhile must not get this old Move's session path or status.
                if self.deps.clock.is_folder_current(folder_gen) {
                    let session = cx
                        .catalog
                        .current()
                        .map_or_else(|| source.clone(), |e| e.path().to_string());
                    cx.sink.update_session_path(&session);
                    cx.sink.set_status_text(&self.status_of(
                        keys::STATUS_MOVE_UNVERIFIED,
                        &[LocArg::new("fileName", get_file_name(&source).to_string())],
                    ));
                }
                done(false)
            }
            CoreTail::Failure {
                folder_gen,
                status,
                recycle_failure,
            }
            | CoreTail::ExecutorThrew {
                folder_gen,
                status,
                recycle_failure,
            } => {
                if self.deps.clock.is_folder_current(folder_gen) {
                    cx.sink.set_status_text(&status);
                    if let Some((source, reason)) = recycle_failure {
                        self.report_recycle_failure(&source, reason.as_deref());
                    }
                }
                done(false)
            }
            CoreTail::CompletedAfterBookkeepingFailure { folder_gen, status } => {
                if self.deps.clock.is_folder_current(folder_gen) {
                    cx.sink.set_status_text(&status);
                    done(true)
                } else {
                    done(false)
                }
            }
            CoreTail::MoveCopyFinal(_) => done(true), // handled before the match
        };
        cx.sink.notify_navigation_state_changed();
        match follow_up {
            Some(tail) if step == done(true) => self.move_copy_after_core(cx, tail),
            _ => step,
        }
    }

    /// C# `ReportLateCompletion(result)` (lines 599 to 617): a single-file Move/Recycle that finished after the folder changed.
    /// `rejected` results are ignored; otherwise `undo.register(result)` and `sink.show_late_action_status` with
    /// `StatusLateMoveUndoable(fileName, directory of destination)` (Move with a destination), `StatusLateDeletedPermanently`
    /// (permanent Recycle) or `StatusLateRecycleUndoable`. Copy is silent.
    pub(super) fn report_late_completion(&mut self, cx: &mut FileActionCx<'_>, result: &FileActionResult) {
        if result.rejected {
            return;
        }
        self.with_undo(|u| u.register(result));

        let file_name = get_file_name(&result.source).to_string();
        match result.operation {
            FileOperationType::Move => {
                if let Some(destination) = result.destination_path.as_deref().filter(|d| !d.is_empty()) {
                    let folder = get_directory_name(destination).unwrap_or_default();
                    cx.sink.show_late_action_status(&self.status_of(
                        keys::STATUS_LATE_MOVE_UNDOABLE,
                        &[LocArg::new("fileName", file_name), LocArg::new("folder", folder)],
                    ));
                }
            }
            FileOperationType::Recycle if result.permanently_deleted => {
                cx.sink.show_late_action_status(&self.status_of(
                    keys::STATUS_LATE_DELETED_PERMANENTLY,
                    &[LocArg::new("fileName", file_name)],
                ));
            }
            FileOperationType::Recycle => {
                cx.sink.show_late_action_status(&self.status_of(
                    keys::STATUS_LATE_RECYCLE_UNDOABLE,
                    &[LocArg::new("fileName", file_name)],
                ));
            }
            FileOperationType::Copy => {}
        }
    }
}
