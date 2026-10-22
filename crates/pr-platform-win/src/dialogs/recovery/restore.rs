//! Port of `RecoveryDialog.cs` lines 702-788 and 792-796: Retry (the Restore decision, INV-5 on screen). `RetryAsync`, `SetBusy`, `OnRowsChangedByPresenter`,
//! `Alive`, the wait cursor. Unit W8-18v. Contract: `contracts/W8-18v.md`.
//!
//! The C# `async Task RetryAsync()` is split in two UI-thread steps around ONE worker job (D-W8B5-602): [`retry`] (start: conditions, question, `begin_retry`,
//! worker launch) and [`on_retry_finished`] (the continuation, called by the pump). The file work is NEVER here: it runs in the `RetryFn` the caller gave to the
//! presenter (`RecoveryRetryService`); this file only asks the user and reports.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::Cell;

use pr_app::presenters::recovery_window::RecoveryRetryNotice;
use pr_core::file_actions::recovery_retry::RecoveryRetryResult;

use super::RecoveryContent;
use crate::dialogs::native::dialog::NativeDialog;

/// C# `CursorArrow` (`IDC_ARROW`).
pub const CURSOR_ARROW: i32 = 32512;
/// C# `CursorWait` (`IDC_WAIT`).
pub const CURSOR_WAIT: i32 = 32514;

/// State of the retry slice.
#[derive(Debug)]
pub struct RestoreState {
    /// C# `_busy`: a retry runs (wait cursor, Retry and Clear locked).
    pub(super) busy: Cell<bool>,
    /// C# `LastRetryTask` as a counter: retries launched by the Retry button.
    pub(super) retry_runs: Cell<u32>,
}

impl RestoreState {
    /// Idle.
    pub fn new() -> Self {
        Self {
            busy: Cell::new(false),
            retry_runs: Cell::new(0),
        }
    }
}

impl Default for RestoreState {
    fn default() -> Self {
        Self::new()
    }
}

/// What the dialog does after the retry notice was shown (C# lines 759-766).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterNotice {
    /// `notice.close_window`: `dialog.close()`.
    Close,
    /// `notice.recheck` (and not `close_window`): `actions::start_checks`.
    Recheck,
    /// Neither (a superseded entry): nothing.
    Nothing,
}

/// Pure: `Close` if `notice.close_window`, else `Recheck` if `notice.recheck`, else `Nothing` (the C# `if / else if`).
pub fn after_notice(notice: &RecoveryRetryNotice) -> AfterNotice {
    todo!()
}

/// The cursor id `SetBusy` selects: `CURSOR_WAIT` when busy, else `CURSOR_ARROW`.
pub fn cursor_id(busy: bool) -> i32 {
    todo!()
}

/// C# `Alive`: `dialog.hwnd() != 0 && !dialog.is_closed()`.
pub fn is_alive(dialog: &NativeDialog) -> bool {
    todo!()
}

/// C# `IsBusy` / `WaitCursor`.
pub fn is_busy(content: &RecoveryContent) -> bool {
    todo!()
}

/// C# `SetBusy(busy)` (lines 769-780), called AFTER the presenter call returned (never inside its callback, D-W8B5-603): `busy` flag; nothing more when the
/// window is `0`; `actions::update_buttons`; `win32::api::set_cursor(load_system_cursor(cursor_id(busy)))` (an `Err` of the load is ignored, nothing to free) so the cursor changes at once, not at the next `WM_SETCURSOR`.
pub fn apply_busy(content: &RecoveryContent, dialog: &NativeDialog, busy: bool) {
    todo!()
}

/// C# `OnRowsChangedByPresenter()`: `list::refresh_entries` when `is_alive`.
pub fn on_rows_changed(content: &RecoveryContent, dialog: &NativeDialog) {
    todo!()
}

/// First half of C# `RetryAsync()` (lines 711-727), the `RetryId` click, in this order:
/// 1. `row = list::single_selected`; `!presenter.can_start_retry(row)` -> return (no question, no delegate).
/// 2. `entry = rows[row].entry().clone()`; `message = presenter.retry_confirm_text(row, &texts)` (the "allow permanent delete" setting is read NOW).
/// 3. `actions::confirm(DIALOG_CONFIRM_RETRY_TITLE, message)`; not a yes -> return. NO presenter borrow is held across this call. C# parity: there is NO
///    `is_alive` check between the question and step 4 (a window closed during the question still starts the retry the user just confirmed; its result is
///    then only logged, rule 6).
/// 4. `job = presenter.begin_retry(&entry, &mut |b| pending.push(b))` under a short borrow (the callback only pushes); release; `apply_busy` for every pushed
///    value, in order. `Err(message)` (impossible after step 1) -> `Notify(DIALOG_RETRY_REJECTED_TITLE, message, warning)` while alive.
/// 5. `retry_runs += 1`; `runner.run("recovery-retry", job)` where the job is: `outcome = job.run()`; when the `closed` flag is set log it (rule 6) and stop;
///    else `inbox.send(Retry { entry_id, outcome })`.
pub fn retry(content: &RecoveryContent, dialog: &NativeDialog) {
    todo!()
}

/// Second half of C# `RetryAsync()` (lines 729-767), called by the pump with the worker outcome. Order: `finish_retry(entry_id, outcome, set_busy, rows_changed)`
/// under one short `borrow_mut` with callbacks that only record (busy flags, "rows changed"); release; `apply_busy` per recorded flag; when "rows changed" and
/// alive, `on_rows_changed`. Then by result:
/// - `Err(message)`: alive -> `Notify(DIALOG_RETRY_REJECTED_TITLE, message, warning = true)`; not alive -> `AppLog::error("Recovery retry failed after the window
///   closed", message)`. Return.
/// - `Ok(result)` and not alive: `AppLog::info("Recovery retry finished after the window closed: " + result.message)`. Return.
/// - `Ok(result)`: `notice = RecoveryWindowPresenter::describe_retry(&result, localizer)`; `Notify(notice.title, notice.message, notice.is_warning)`; when no longer
///   alive return (the window closed while the notice was up: no re-check on a dead window); else `after_notice(&notice)`: `Close` -> `dialog.close()`, `Recheck` ->
///   `actions::start_checks`.
pub fn on_retry_finished(
    content: &RecoveryContent,
    dialog: &NativeDialog,
    entry_id: String,
    outcome: Result<RecoveryRetryResult, String>,
) {
    todo!()
}
