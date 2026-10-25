//! Port of `RecoveryDialog.cs`: the buttons and commands (`UpdateButtons`, `Enable`, `IsEnabled`, `OnCommand`, lines 544-626), the background check
//! (`StartChecks`, `IRecoveryCheckView`, lines 628-666), `Confirm` / `Notify` (792-796) and, new in Rust, the completion pump that replaces the C# `await`
//! continuations (D-W8B5-602). Unit W8-18u. Contract: `contracts/W8-18u.md`.
//!
//! The presenter calls its view (`RecoveryCheckView`) from INSIDE a `&mut` borrow of the presenter, so the view here only RECORDS [`CheckEffect`]s; the
//! effects are applied after the borrow ended, in the recorded order (D-W8B5-603). The same rule holds for the `set_busy` / `rows_changed` callbacks of
//! `finish_retry` (18v).
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize};

use pr_app::coordinators::zoom_detail::{CompletionInbox, WakeFn};
use pr_app::presenters::recovery_window::{RecoveryCheckMessage, RecoveryCheckView};
use pr_core::file_actions::recovery_retry::RecoveryRetryResult;

use super::RecoveryContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::win32::structs::{Lparam, Lresult, Wparam};

// The completions cross threads through the inbox (`CompletionInbox<T>` needs `T: Send`); the compiler checks it here, not at the first `send` of a porter.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<RecoveryCompletion>();
};

/// What a worker thread hands back to the UI thread through the inbox, in arrival order.
pub enum RecoveryCompletion {
    /// One message of a background check run (`RecoveryCheckRun::run` posts them).
    Check(RecoveryCheckMessage),
    /// The outcome of a retry job: the id of the entry that was retried and the worker result (`Err` = the C# exception message).
    Retry {
        /// `JournalEntry::id` of the retried row.
        entry_id: String,
        /// `RecoveryRetryJob::run()`.
        outcome: Result<RecoveryRetryResult, String>,
    },
}

impl std::fmt::Debug for RecoveryCompletion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryCompletion").finish_non_exhaustive()
    }
}

/// State of the actions slice.
pub struct ActionsState {
    /// Worker-to-UI mailbox (`send` on workers, `drain` on the UI thread); its wake function is [`post_wake`] on `wake_target`.
    pub(super) inbox: CompletionInbox<RecoveryCompletion>,
    /// Completions drained from the inbox and not handled yet, FIFO. `pump` pops ONE at a time, so a nested pump (a message box inside a handler runs a
    /// nested loop that dispatches the wake message again) continues the same queue and the order is kept (D-W8B5-602).
    pub(super) ready: RefCell<VecDeque<RecoveryCompletion>>,
    /// The dialog window the wake message goes to; `0` before `on_initialized` and after `on_closed` (written on the UI thread, read on workers).
    pub(super) wake_target: Arc<AtomicIsize>,
    /// Set by `on_closed`; a retry job that finishes later does not post, it logs (`restore` rule 6).
    pub(super) closed: Arc<AtomicBool>,
    /// C# `LastCheckTask` as a counter: checks launched.
    pub(super) check_runs: Cell<u32>,
}

impl std::fmt::Debug for ActionsState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionsState").finish_non_exhaustive()
    }
}

impl ActionsState {
    /// Empty inbox whose wake function is `post_wake(&wake_target)`.
    pub fn new() -> Self {
        let wake_target = Arc::new(AtomicIsize::new(0));
        let target = Arc::clone(&wake_target);
        let wake: WakeFn = Arc::new(move || post_wake(&target));
        Self {
            inbox: CompletionInbox::new(wake),
            ready: RefCell::new(VecDeque::new()),
            wake_target,
            closed: Arc::new(AtomicBool::new(false)),
            check_runs: Cell::new(0),
        }
    }
}

impl Default for ActionsState {
    fn default() -> Self {
        Self::new()
    }
}

/// One thing the presenter told its view during a check call (the four `RecoveryCheckView` methods).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckEffect {
    /// `row_changed(index)`: the result of that presenter row changed (also the reset to "checking").
    RowChanged(usize),
    /// `set_check_status(text)`.
    CheckStatus(String),
    /// `set_recheck_enabled(enabled)`.
    RecheckEnabled(bool),
    /// `check_completed()`.
    Completed,
}

/// A [`RecoveryCheckView`] that only records (see the module doc).
#[derive(Debug, Default)]
pub struct CheckEffects {
    /// The recorded effects in call order.
    pub effects: Vec<CheckEffect>,
}

impl RecoveryCheckView for CheckEffects {
    fn set_check_status(&mut self, text: &str) {
        self.effects.push(CheckEffect::CheckStatus(text.to_owned()));
    }
    fn set_recheck_enabled(&mut self, enabled: bool) {
        self.effects.push(CheckEffect::RecheckEnabled(enabled));
    }
    fn check_completed(&mut self) {
        self.effects.push(CheckEffect::Completed);
    }
    fn row_changed(&mut self, row_index: usize) {
        self.effects.push(CheckEffect::RowChanged(row_index));
    }
}

/// What a `WM_COMMAND` means (pure mapping of `OnCommand`'s two `switch` statements).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAction {
    /// `FILTER_ID` + `CBN_SELCHANGE`.
    FilterChanged,
    /// `LIST_ID` + `LBN_SELCHANGE`.
    SelectionChanged,
    /// `RECHECK_ID` + `BN_CLICKED`.
    Recheck,
    /// `CLEAR_SELECTED_ID` + `BN_CLICKED`.
    ClearSelected,
    /// `CLEAR_ALL_ID` + `BN_CLICKED`.
    ClearAll,
    /// `RETRY_ID` + `BN_CLICKED`.
    Retry,
    /// `COPY_SOURCE_ID` + `BN_CLICKED`.
    CopySource,
    /// `SHOW_SOURCE_ID` + `BN_CLICKED`.
    ShowSource,
    /// `COPY_DESTINATION_ID` + `BN_CLICKED`.
    CopyDestination,
    /// `SHOW_DESTINATION_ID` + `BN_CLICKED`.
    ShowDestination,
}

/// Pure core of C# `OnCommand` (lines 580-626): `None` = not handled (`false`). The combo box and the list react to their selection-change notification
/// only; every button to `BN_CLICKED` (0) only.
pub fn command_action(id: i32, notification: i32) -> Option<CommandAction> {
    use super::layout::*;
    Some(match (id, notification) {
        (FILTER_ID, 1) => CommandAction::FilterChanged,
        (LIST_ID, 1) => CommandAction::SelectionChanged,
        (RECHECK_ID, 0) => CommandAction::Recheck,
        (CLEAR_SELECTED_ID, 0) => CommandAction::ClearSelected,
        (CLEAR_ALL_ID, 0) => CommandAction::ClearAll,
        (RETRY_ID, 0) => CommandAction::Retry,
        (COPY_SOURCE_ID, 0) => CommandAction::CopySource,
        (SHOW_SOURCE_ID, 0) => CommandAction::ShowSource,
        (COPY_DESTINATION_ID, 0) => CommandAction::CopyDestination,
        (SHOW_DESTINATION_ID, 0) => CommandAction::ShowDestination,
        _ => return None,
    })
}

/// C# `OnCommand(id, notification)`: runs the action of [`command_action`] and returns `true`: `FilterChanged` -> `list::refresh_entries`; `SelectionChanged` ->
/// `list::on_selection_changed`; `Recheck` -> `start_checks`; `ClearSelected` -> `dismiss::dismiss(selected_rows)`; `ClearAll` -> `dismiss::dismiss(0..rows)`;
/// `Retry` -> `restore::retry`; the four path buttons -> `path_panel::{copy, show_in_explorer}` with the stored path of that panel.
pub fn on_command(content: &RecoveryContent, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
    let Some(action) = command_action(id, notification) else {
        return false;
    };
    match action {
        CommandAction::FilterChanged => super::list::refresh_entries(content, dialog),
        CommandAction::SelectionChanged => super::list::on_selection_changed(content, dialog),
        CommandAction::Recheck => start_checks(content, dialog),
        CommandAction::ClearSelected => {
            super::dismiss::dismiss(content, dialog, super::list::selected_rows(content, dialog))
        }
        CommandAction::ClearAll => {
            let count = content.presenter.borrow().rows().len();
            super::dismiss::dismiss(content, dialog, (0..count).collect());
        }
        CommandAction::Retry => super::restore::retry(content, dialog),
        CommandAction::CopySource => super::path_panel::copy(content, &content.details.source_path.borrow()),
        CommandAction::ShowSource => {
            super::path_panel::show_in_explorer(content, dialog, &content.details.source_path.borrow())
        }
        CommandAction::CopyDestination => super::path_panel::copy(content, &content.details.destination_path.borrow()),
        CommandAction::ShowDestination => {
            super::path_panel::show_in_explorer(content, dialog, &content.details.destination_path.borrow())
        }
    }
    true
}

/// C# `UpdateButtons()` (lines 546-564): nothing when the list window is `0`; `states = presenter.button_states(single, selected_count)` (short borrow);
/// `enable` Retry, Clear selected, Clear all; and when the control that HAD the focus (read BEFORE the enabling) is now disabled, focus the list.
pub fn update_buttons(content: &RecoveryContent, dialog: &NativeDialog) {
    use super::layout::*;
    if dialog.item(LIST_ID) == 0 {
        return;
    }
    let focus = crate::win32::dialog_native::user32::get_focus();
    let selected = super::list::selected_rows(content, dialog);
    let single = super::list::single_selected(content, dialog);
    let states = { content.presenter.borrow().button_states(single, selected.len()) };
    enable(dialog, RETRY_ID, states.retry);
    enable(dialog, CLEAR_SELECTED_ID, states.clear_selected);
    enable(dialog, CLEAR_ALL_ID, states.clear_all);
    if focus != 0 && !crate::win32::dialog_native::user32::is_window_enabled(focus) {
        crate::win32::dialog_native::user32::set_focus(dialog.item(LIST_ID));
    }
}

/// C# `Enable(id, enabled)`: `EnableWindow(item(id), enabled)` when the control exists.
pub fn enable(dialog: &NativeDialog, id: i32, enabled: bool) {
    let hwnd = dialog.item(id);
    if hwnd != 0 {
        crate::win32::dialog_native::user32::enable_window(hwnd, enabled);
    }
}

/// C# `IsEnabled(id)`: the control exists and `IsWindowEnabled`.
pub fn is_enabled(dialog: &NativeDialog, id: i32) -> bool {
    let hwnd = dialog.item(id);
    hwnd != 0 && crate::win32::dialog_native::user32::is_window_enabled(hwnd)
}

/// C# `StartChecks()` (lines 631-635) with the D-W8-2 split: under one short `borrow_mut`, `presenter.begin_checks(&mut CheckEffects, &texts)`; release; apply the
/// recorded effects; when a run came back: `check_runs += 1` and `runner.run("recovery-check", job)` where the job is `run.run(&|m| inbox.send(Check(m)))`.
pub fn start_checks(content: &RecoveryContent, dialog: &NativeDialog) {
    let (run, effects) = {
        let mut effects = CheckEffects::default();
        let run = content
            .presenter
            .borrow_mut()
            .begin_checks(&mut effects, &content.texts.texts());
        (run, effects.effects)
    };
    apply_check_effects(content, dialog, effects);
    if let Some(run) = run {
        content
            .actions
            .check_runs
            .set(content.actions.check_runs.get().wrapping_add(1));
        let inbox = content.actions.inbox.clone();
        content.runner.run(
            "recovery-check",
            Box::new(move || run.run(&|m| inbox.send(RecoveryCompletion::Check(m)))),
        );
    }
}

/// Applies recorded effects in order (the C# `IRecoveryCheckView` bodies and `OnRowResultChanged`): `RowChanged(i)` -> `list::on_row_result_changed(i)` (always:
/// the C# `ResultChanged` handler repaints the list whether or not the row is visible; the function itself compares `i` with the single selected row); `CheckStatus(t)` -> `set_item_text(CHECK_STATUS_ID, t)` when the window exists; `RecheckEnabled(e)` -> read the
/// focus, `enable(RECHECK_ID, e)`, and when `!e` and the focus WAS the Re-check button, focus the list; `Completed` -> `list::refresh_entries` when the window
/// exists.
pub fn apply_check_effects(content: &RecoveryContent, dialog: &NativeDialog, effects: Vec<CheckEffect>) {
    for effect in effects {
        match effect {
            CheckEffect::RowChanged(i) => super::list::on_row_result_changed(content, dialog, i),
            CheckEffect::CheckStatus(t) => {
                if dialog.hwnd() != 0 {
                    dialog.set_item_text(super::layout::CHECK_STATUS_ID, &t)
                }
            }
            CheckEffect::RecheckEnabled(e) => {
                if dialog.hwnd() != 0 {
                    let f = crate::win32::dialog_native::user32::get_focus();
                    enable(dialog, super::layout::RECHECK_ID, e);
                    if !e && f == dialog.item(super::layout::RECHECK_ID) {
                        crate::win32::dialog_native::user32::set_focus(dialog.item(super::layout::LIST_ID));
                    }
                }
            }
            CheckEffect::Completed => {
                if dialog.hwnd() != 0 {
                    super::list::refresh_entries(content, dialog)
                }
            }
        }
    }
}

/// Handles ONE completion: `Check(m)` -> `presenter.on_check_message(m, &mut CheckEffects, &texts)` under a short borrow, then `apply_check_effects`;
/// `Retry { entry_id, outcome }` -> `restore::on_retry_finished`.
pub fn handle_completion(content: &RecoveryContent, dialog: &NativeDialog, completion: RecoveryCompletion) {
    match completion {
        RecoveryCompletion::Check(m) => {
            let effects = {
                let mut e = CheckEffects::default();
                content
                    .presenter
                    .borrow_mut()
                    .on_check_message(m, &mut e, &content.texts.texts());
                e.effects
            };
            apply_check_effects(content, dialog, effects)
        }
        RecoveryCompletion::Retry { entry_id, outcome } => {
            super::restore::on_retry_finished(content, dialog, entry_id, outcome)
        }
    }
}

/// The pump: moves `inbox.drain()` to the back of `ready`, then loops `pop_front` + `handle_completion` until `ready` is empty. No `RefCell` borrow of `ready`
/// is held while a completion is handled (a handler may open a message box whose nested loop calls `pump` again). Does nothing after `on_closed`.
pub fn pump(content: &RecoveryContent, dialog: &NativeDialog) {
    if content.actions.closed.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    content.actions.ready.borrow_mut().extend(content.actions.inbox.drain());
    loop {
        let next = content.actions.ready.borrow_mut().pop_front();
        let Some(c) = next else { break };
        handle_completion(content, dialog, c);
    }
}

/// `OnWindowMessage` of the content: `WM_RECOVERY_COMPLETIONS` -> `pump`, `Some(0)`; anything else `None`.
pub fn on_window_message(
    content: &RecoveryContent,
    dialog: &NativeDialog,
    message: u32,
    _wparam: Wparam,
    _lparam: Lparam,
) -> Option<Lresult> {
    if message == super::WM_RECOVERY_COMPLETIONS {
        pump(content, dialog);
        Some(0)
    } else {
        None
    }
}

/// The wake function of the inbox, callable from ANY thread: loads `target`; when it is not `0` posts `WM_RECOVERY_COMPLETIONS` to that window with
/// `PostMessage` (thread-safe; `win32::api::post_message` is an `unsafe fn`: ONE call with a `// SAFETY:` comment, the message carries no pointer). A failed post
/// is ignored (the window is gone; `on_closed` drains the inbox).
pub fn post_wake(target: &AtomicIsize) {
    let hwnd = target.load(std::sync::atomic::Ordering::Acquire);
    if hwnd != 0 {
        // SAFETY: PostMessage parameters contain no pointers and are valid scalar message data.
        let _ = unsafe { crate::win32::api::user32::post_message(hwnd, super::WM_RECOVERY_COMPLETIONS, 0, 0) };
    }
}

/// C# `Confirm(title, message)`: `messages.show(dialog.hwnd(), title, message, Confirmation) == IDYES`. A confirmation closed without an answer (X, Esc) is
/// `IDCANCEL`, which is NOT a yes.
pub fn confirm(content: &RecoveryContent, dialog: &NativeDialog, title: &str, message: &str) -> bool {
    content.messages.show(
        dialog.hwnd(),
        title,
        message,
        crate::dialogs::task_dialog::TaskDialogKind::Confirmation,
    ) == crate::win32::messages::IDYES
}

/// C# `Notify(title, message, warning)`: `messages.show(dialog.hwnd(), title, message, if warning { Error } else { Information })`, the answer ignored.
pub fn notify(content: &RecoveryContent, dialog: &NativeDialog, title: &str, message: &str, warning: bool) {
    let kind = if warning {
        crate::dialogs::task_dialog::TaskDialogKind::Error
    } else {
        crate::dialogs::task_dialog::TaskDialogKind::Information
    };
    let _ = content.messages.show(dialog.hwnd(), title, message, kind);
}
