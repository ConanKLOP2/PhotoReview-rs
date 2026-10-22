//! Port of `RecoveryDialog.cs` lines 668-700: Clear selected / Clear all (the Dismiss decision, INV-5 on screen). `Dismiss(rows)`. Unit W8-18v.
//! Contract: `contracts/W8-18v.md`. The journal write is the `DismissFn` the caller gave to the presenter (`OperationJournal::dismiss`); this file asks and reports.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::presenters::recovery_window::RecoveryDismissResult;

use super::RecoveryContent;
use crate::dialogs::native::dialog::NativeDialog;

/// What follows a dismissal once the presenter answered (C# lines 684-699), pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DismissFollowUp {
    /// `result.error` is `Some`: show it as a warning (`RECOVERY_DISMISS_FAILED_TITLE`); nothing was removed, the list is NOT refreshed.
    ShowError(String),
    /// No error and the window is gone (`Hwnd == 0`): stop (no refresh, no notice).
    Stop,
    /// No error, window alive: refresh the list; when `skipped > 0` also show the information box `RECOVERY_DISMISS_STALE_TITLE` /
    /// `RECOVERY_DISMISS_STALE_MESSAGE({skipped})`.
    Refresh {
        /// `result.skipped_count`: the rows the journal reported as already resolved elsewhere (they are removed from the list too, and SAID so).
        skipped: usize,
    },
}

/// Pure: the decision above. `has_window` = `dialog.hwnd() != 0` (C# `Hwnd == 0` check; NOT `Alive`).
pub fn follow_up(result: &RecoveryDismissResult, has_window: bool) -> DismissFollowUp {
    todo!()
}

/// C# `Dismiss(List<RecoveryRowModel> rows)` with the rows as presenter indices, in this order: `prompt = presenter.dismiss_prompt(&rows, localizer)` (short
/// borrow), `None` -> return (no delegate, no question: nothing selected or no dismiss delegate); `actions::confirm(RECOVERY_DISMISS_CONFIRM_TITLE, prompt)`,
/// not a yes -> return (NO borrow across the question); `result = presenter.dismiss(&rows)` under a short `borrow_mut` (the journal write runs inside it, on the
/// UI thread, as in C#); then `follow_up(&result, dialog.hwnd() != 0)`.
pub fn dismiss(content: &RecoveryContent, dialog: &NativeDialog, rows: Vec<usize>) {
    todo!()
}
