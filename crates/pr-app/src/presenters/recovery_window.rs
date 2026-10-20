//! Port of `PhotoReview.App.Shared/Presenters/RecoveryWindowPresenter.cs` (C# baseline `d92d6aae`, 332 lines): the Recovery
//! window's behaviour without a window (WP-35 G0) -- the rows, the filter, the details model, the live read-only check, Clear
//! (dismiss) and Retry. INV-5 on screen: Restore/Dismiss decisions happen here; the file work is `RecoveryRetryService` and
//! `OperationJournal::dismiss` behind the injected delegates. Contract: `contracts/W8-10g.md`.
//!
//! D-W8-2: the two C# `async Task` methods (`RunChecksAsync`, `ExecuteRetryAsync`) are split in three steps so the UI thread
//! never blocks and no future exists: `begin_*` (UI thread: state change, returns a `Send` job) -> `job.run(..)` (worker thread,
//! blocking) -> the result comes back as a message the host posts to the UI thread with `UiScheduler::post`, FIFO, and feeds to
//! `on_check_message` / `finish_retry` (UI thread). The C# cancellation token per run is the INV-9 generation token: every message
//! carries the token of the run that produced it and is dropped when that token was cancelled.
//! D-W8-1: no `ResultChanged` event; the presenter tells the view with `RecoveryCheckView::row_changed`.
//! Texts: `RecoveryTexts` of `presenters::recovery_presentation` (B1, W8-10f) carries the localizer; no ambient `Tr`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use pr_core::file_actions::errors;
use pr_core::file_actions::journal::DismissOutcome;
use pr_core::file_actions::journal_line::JournalEntry;
use pr_core::file_actions::recovery_check::{RecoveryCheckResult, RecoveryFileCheck};
use pr_core::file_actions::recovery_retry::{AllowPermanentDelete, RecoveryRetryResult};
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::get_file_name;
use pr_l10n::keys;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

use super::recovery_filter::{RecoveryFilter, RecoveryTone};
use super::recovery_presentation::{
    RecoveryGroupMemberView, RecoveryPathView, RecoveryTexts, allows_retry, matches, verdict_tone,
};
use crate::misc::app_log::AppLog;

/// The text of a caught panic payload (`&str` / `String`), else a fixed fallback.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "The operation panicked.".to_owned()
    }
}

/// C# `RecoveryRowModel(entry)`: one list row without a UI type. `check` stays `None` ("checking") until the background check
/// reports. The C# `ResultChanged` event is replaced by `RecoveryCheckView::row_changed` (D-W8-1).
#[derive(Debug, Clone)]
pub struct RecoveryRowModel {
    entry: JournalEntry,
    check: Option<RecoveryCheckResult>,
}

impl RecoveryRowModel {
    /// A row with no result yet.
    pub fn new(entry: JournalEntry) -> Self {
        Self { entry, check: None }
    }

    /// C# `Entry`.
    pub fn entry(&self) -> &JournalEntry {
        &self.entry
    }

    /// C# `Check`: `None` until the check reported.
    pub fn check(&self) -> Option<&RecoveryCheckResult> {
        self.check.as_ref()
    }

    /// C# `SetResult(result)`: stores the result (`None` = back to "checking"). Always counts as a change (C# raises the event
    /// even for an equal value); the caller tells the view.
    pub fn set_result(&mut self, result: Option<RecoveryCheckResult>) {
        self.check = result;
    }

    /// C# `Title`: with non-empty `entry.group_members`, `recovery.group.title` with `{fileName}` = `get_file_name(entry.source)`
    /// and `{count}` = member count; else the file name alone (`pr_core::text::dotnet_path::get_file_name`).
    pub fn title(&self, texts: &RecoveryTexts<'_>) -> String {
        let name = get_file_name(&self.entry.source);
        match self.entry.group_members.as_deref() {
            Some(members) if !members.is_empty() => texts.loc.format(
                keys::RECOVERY_GROUP_TITLE,
                &[
                    LocArg::new("fileName", name),
                    LocArg::new("count", members.len() as i64),
                ],
            ),
            _ => name.to_owned(),
        }
    }

    /// C# `Subtitle`: `recovery.row.subtitle` with `{state}` = `texts.state_text(entry.state)` and `{operation}` =
    /// `texts.operation_text(entry.op_type)`.
    pub fn subtitle(&self, texts: &RecoveryTexts<'_>) -> String {
        texts.loc.format(
            keys::RECOVERY_ROW_SUBTITLE,
            &[
                LocArg::new("state", texts.state_text(self.entry.state)),
                LocArg::new("operation", texts.operation_text(self.entry.op_type)),
            ],
        )
    }

    /// C# `BadgeText`: `recovery.row.checking` while unchecked, else `texts.verdict_text(verdict)`.
    pub fn badge_text(&self, texts: &RecoveryTexts<'_>) -> String {
        match &self.check {
            None => texts.loc.format(keys::RECOVERY_ROW_CHECKING, &[]),
            Some(result) => texts.verdict_text(result.verdict),
        }
    }

    /// C# `BadgeTone`: `Neutral` while unchecked, else `recovery_presentation::verdict_tone(verdict)`.
    pub fn badge_tone(&self) -> RecoveryTone {
        self.check
            .as_ref()
            .map_or(RecoveryTone::Neutral, |r| verdict_tone(r.verdict))
    }
}

/// C# `RecoveryDetails`: everything the details panel shows for the selected row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDetails {
    pub error_text: String,
    pub error_visible: bool,
    pub verdict_text: String,
    pub verdict_tone: RecoveryTone,
    pub explain_text: String,
    pub action_text: String,
    pub source: Option<RecoveryPathView>,
    pub source_visible: bool,
    pub destination: Option<RecoveryPathView>,
    pub destination_visible: bool,
    pub no_destination_visible: bool,
    pub group_heading: Option<String>,
    pub group_members: Option<Vec<RecoveryGroupMemberView>>,
}

/// C# `RecoveryRetryNotice(Title, Message, IsWarning, CloseWindow, Recheck)`: the outcome of Retry as the dialog must show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryRetryNotice {
    pub title: String,
    pub message: String,
    pub is_warning: bool,
    /// Success: the window closes.
    pub close_window: bool,
    /// A refused retry re-runs the live check.
    pub recheck: bool,
}

/// C# `RecoveryDismissResult(Error, SkippedCount)`: an error to show (nothing removed) or the count of rows skipped as stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDismissResult {
    pub error: Option<String>,
    pub skipped_count: usize,
}

/// C# tuple `(Retry, ClearSelected, ClearAll)` of `ButtonStates`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryButtonStates {
    pub retry: bool,
    pub clear_selected: bool,
    pub clear_all: bool,
}

/// C# `IRecoveryCheckView`: the few things the live check needs from a view. UI-thread object (no `Send`).
pub trait RecoveryCheckView {
    /// The progress / summary line next to the Re-check button.
    fn set_check_status(&mut self, text: &str);
    /// The Re-check button.
    fn set_recheck_enabled(&mut self, enabled: bool);
    /// A run finished normally: rebuild the visible list (the filter may now match differently).
    fn check_completed(&mut self);
    /// NEW (replaces the C# `RecoveryRowModel.ResultChanged` event): the result of row `row_index` (index in `rows()` NOW) changed,
    /// including the reset to "checking" at the start of a run.
    fn row_changed(&mut self, row_index: usize);
}

/// C# `Func<JournalEntry, Task<RecoveryRetryResult>>`: BLOCKING (D-W8-2), called on a worker thread. `Err` is the C# exception
/// message. Production: a closure over `RecoveryRetryService::retry_move_or_copy(failed, confirmed_finish_cancelled, cancel)`.
pub type RetryFn = Arc<dyn Fn(&JournalEntry) -> Result<RecoveryRetryResult, String> + Send + Sync>;

/// C# `Func<IReadOnlyList<JournalEntry>, DismissOutcome>`: called synchronously on the UI thread (C# does the same). `Err` is the
/// exception message. Production: a closure over `OperationJournal::dismiss`.
pub type DismissFn = Box<dyn Fn(&[JournalEntry]) -> Result<DismissOutcome, String>>;

/// C# `RecoveryFileCheck` seam. `Err` stands for a C# exception (access denied on a share, ...). Never writes.
pub trait RecoveryChecker: Send + Sync {
    fn check(&self, entry: &JournalEntry) -> Result<RecoveryCheckResult, String>;
}

/// Production checker: `RecoveryFileCheck::check` never fails, so this is always `Ok`.
impl RecoveryChecker for RecoveryFileCheck {
    fn check(&self, entry: &JournalEntry) -> Result<RecoveryCheckResult, String> {
        Ok(RecoveryFileCheck::check(self, entry))
    }
}

/// The constructor arguments of C# `RecoveryWindowPresenter(entries, retry, dismiss, fileSystem, allowPermanentDelete)`.
/// The C# `IFileSystem` becomes the checker (`Arc::new(RecoveryFileCheck::new(fs))` in production).
pub struct RecoveryWindowOptions {
    /// `None` = the Retry button is never enabled.
    pub retry: Option<RetryFn>,
    /// `None` = the Clear buttons are never enabled.
    pub dismiss: Option<DismissFn>,
    pub checker: Arc<dyn RecoveryChecker>,
    /// Read at the moment the confirmation is asked (C# `Func<bool>`); `None` = false.
    pub allow_permanent_delete: Option<AllowPermanentDelete>,
    /// C# `DateTime.Now.ToString("T", CurrentCulture)` (the long-time pattern) for the "done" line. Not in
    /// `RecoveryTimeFormatter` (B1, which has only the "g" pattern): production binds it to the Windows locale.
    pub current_time_text: Arc<dyn Fn() -> String + Send + Sync>,
}

impl fmt::Debug for RecoveryWindowOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryWindowOptions")
            .field("has_retry", &self.retry.is_some())
            .field("has_dismiss", &self.dismiss.is_some())
            .finish_non_exhaustive()
    }
}

/// What a worker reports for one live-check run, in order: zero or more `Result`, then exactly one of `Finished` / `Failed`, or
/// nothing after the first cancelled row (C# `ThrowIfCancellationRequested` leaves silently).
#[derive(Debug, Clone, PartialEq, Eq)]
// Short-lived message moved once per row; boxing would change the public contract shape for no gain.
#[allow(clippy::large_enum_variant)]
pub enum RecoveryCheckEvent {
    /// One row finished. `id` is the journal entry Id (the row may have been cleared meanwhile; its index is NOT stable).
    Result { id: String, result: RecoveryCheckResult },
    /// The loop ended normally.
    Finished,
    /// The checker failed (`Err` or a panic); the message is logged.
    Failed { message: String },
}

/// An event plus the cancel token of the run that produced it (INV-9 generation guard).
#[derive(Debug, Clone)]
pub struct RecoveryCheckMessage {
    pub event: RecoveryCheckEvent,
    token: CancelToken,
}

/// A started live check, ready for a worker. `Send`: owns the entries snapshot, an `Arc` checker and the token.
pub struct RecoveryCheckRun {
    entries: Vec<JournalEntry>,
    checker: Arc<dyn RecoveryChecker>,
    token: CancelToken,
}

impl fmt::Debug for RecoveryCheckRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryCheckRun")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl RecoveryCheckRun {
    /// BLOCKING, worker thread. For each entry in snapshot order: when the token is cancelled return WITHOUT posting anything; else
    /// `checker.check(entry)` (a panic is caught and treated like `Err`) and `post(Result {..})`; on `Err(message)` post
    /// `Failed { message }` and stop. After the last entry post `Finished`. Read-only: it never calls retry or dismiss.
    pub fn run(self, post: &dyn Fn(RecoveryCheckMessage)) {
        for entry in &self.entries {
            if self.token.is_canceled() {
                return;
            }
            let checker = &self.checker;
            let outcome = match catch_unwind(AssertUnwindSafe(|| checker.check(entry))) {
                Ok(result) => result,
                Err(payload) => Err(panic_message(payload.as_ref())),
            };
            match outcome {
                Ok(result) => post(RecoveryCheckMessage {
                    event: RecoveryCheckEvent::Result {
                        id: entry.id.clone(),
                        result,
                    },
                    token: self.token.clone(),
                }),
                Err(message) => {
                    post(RecoveryCheckMessage {
                        event: RecoveryCheckEvent::Failed { message },
                        token: self.token.clone(),
                    });
                    return;
                }
            }
        }
        post(RecoveryCheckMessage {
            event: RecoveryCheckEvent::Finished,
            token: self.token.clone(),
        });
    }
}

/// A started retry, ready for a worker. `Send`.
pub struct RecoveryRetryJob {
    entry: JournalEntry,
    retry: RetryFn,
}

impl fmt::Debug for RecoveryRetryJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryRetryJob")
            .field("id", &self.entry.id)
            .finish_non_exhaustive()
    }
}

impl RecoveryRetryJob {
    /// BLOCKING, worker thread: `retry(&entry)`; a panic of the delegate is caught and becomes `Err(message)`.
    pub fn run(self) -> Result<RecoveryRetryResult, String> {
        let retry = &self.retry;
        let entry = &self.entry;
        match catch_unwind(AssertUnwindSafe(|| retry(entry))) {
            Ok(outcome) => outcome,
            Err(payload) => Err(panic_message(payload.as_ref())),
        }
    }
}

/// C# `RecoveryWindowPresenter`. UI-thread object, except the two jobs it hands out.
pub struct RecoveryWindowPresenter {
    rows: Vec<RecoveryRowModel>,
    retry: Option<RetryFn>,
    dismiss: Option<DismissFn>,
    checker: Arc<dyn RecoveryChecker>,
    allow_permanent_delete: Option<AllowPermanentDelete>,
    current_time_text: Arc<dyn Fn() -> String + Send + Sync>,
    /// C# `_cancelCheck`: the token of the latest run.
    cancel_check: Option<CancelToken>,
    is_retrying: bool,
    /// Progress of the current run (C# locals `total` and `done`).
    check_total: usize,
    check_done: usize,
}

impl fmt::Debug for RecoveryWindowPresenter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecoveryWindowPresenter")
            .field("rows", &self.rows.len())
            .field("is_retrying", &self.is_retrying)
            .finish_non_exhaustive()
    }
}

impl RecoveryWindowPresenter {
    /// C# constructor: one row per entry, in order.
    pub fn new(entries: Vec<JournalEntry>, options: RecoveryWindowOptions) -> Self {
        Self {
            rows: entries.into_iter().map(RecoveryRowModel::new).collect(),
            retry: options.retry,
            dismiss: options.dismiss,
            checker: options.checker,
            allow_permanent_delete: options.allow_permanent_delete,
            current_time_text: options.current_time_text,
            cancel_check: None,
            is_retrying: false,
            check_total: 0,
            check_done: 0,
        }
    }

    /// `recovery.check.progress` with `{done}` / `{total}`.
    fn progress_text(texts: &RecoveryTexts<'_>, done: usize, total: usize) -> String {
        texts.loc.format(
            keys::RECOVERY_CHECK_PROGRESS,
            &[LocArg::new("done", done as i64), LocArg::new("total", total as i64)],
        )
    }

    // ---- List ----

    /// C# `Rows`: the rows still in the journal snapshot, in journal order (the filter is applied by `shown`).
    pub fn rows(&self) -> &[RecoveryRowModel] {
        &self.rows
    }

    /// C# `IsRetrying`: true while a retry runs; Retry/Clear are disabled and the window refuses to close.
    pub fn is_retrying(&self) -> bool {
        self.is_retrying
    }

    /// C# `CanClose`.
    pub fn can_close(&self) -> bool {
        !self.is_retrying
    }

    /// C# `Shown(filter)`: indices into `rows()` of the rows the filter lets through, in journal order
    /// (`recovery_presentation::matches(filter, row.check().map(|c| c.verdict))`; an unchecked row matches `All` only).
    pub fn shown(&self, filter: RecoveryFilter) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches(filter, row.check().map(|c| c.verdict)))
            .map(|(index, _)| index)
            .collect()
    }

    /// C# `SummaryText`: no rows => `recovery.summary.empty`; else the plural `recovery.summary.count` (`format_plural` with
    /// `{count}`; keys `.one` / `.other` in `en.json`).
    pub fn summary_text(&self, loc: &Localizer) -> String {
        if self.rows.is_empty() {
            loc.format(keys::RECOVERY_SUMMARY_EMPTY, &[])
        } else {
            let count = self.rows.len() as i64;
            loc.format_plural("recovery.summary.count", count, &[LocArg::new("count", count)])
        }
    }

    /// C# `FilterEmptyVisible(shownCount)`: `rows.len() > 0 && shown_count == 0` (an EMPTY list is not "nothing matches").
    pub fn filter_empty_visible(&self, shown_count: usize) -> bool {
        !self.rows.is_empty() && shown_count == 0
    }

    /// C# `ButtonStates(singleSelected, selectedCount)`. All three are false while retrying. `retry` also needs a retry delegate and
    /// `allows_retry(single.check())` (false for an unchecked row and for `None`); `clear_selected` needs a dismiss delegate and
    /// `selected_count > 0`; `clear_all` needs a dismiss delegate and at least one row.
    pub fn button_states(&self, single_selected: Option<usize>, selected_count: usize) -> RecoveryButtonStates {
        let busy = self.is_retrying;
        let single = single_selected
            .and_then(|i| self.rows.get(i))
            .and_then(RecoveryRowModel::check);
        RecoveryButtonStates {
            retry: !busy && self.retry.is_some() && allows_retry(single),
            clear_selected: !busy && self.dismiss.is_some() && selected_count > 0,
            clear_all: !busy && self.dismiss.is_some() && !self.rows.is_empty(),
        }
    }

    // ---- Details ----

    /// C# static `Details(row)`. Unchecked row: source view from the journal (`texts.journal_text`, neutral tone, empty now/status),
    /// destination view when `entry.destination` is `Some`, verdict text `recovery.row.checking`, explain text
    /// `recovery.detail.checking`, empty action text, `source_visible` true, `no_destination_visible` = destination is `None`, no group.
    /// Checked row: group members (non-empty `group_members` of the result) become `RecoveryGroupMemberView { status:
    /// texts.group_status_text(verdict, member, source, destination), source: member.source, destination: member.destination or
    /// recovery.detail.noDestination }` with heading `recovery.group.members` ({count}); a group hides BOTH path panels and the "no
    /// destination" text; a plain entry shows `texts.path_view(recovery.detail.source, result.source, entry, false)` and, when the
    /// result has a destination, `path_view(recovery.detail.destination, dest, entry, result.destination_folder_missing())`.
    /// `error_text` = `recovery.detail.error` ({error}) when `errors::describe(error_code, error, localize)` is `Some`.
    pub fn details(row: &RecoveryRowModel, texts: &RecoveryTexts<'_>) -> RecoveryDetails {
        let entry = row.entry();
        let error = errors::describe(entry.error_code.as_deref(), entry.error.as_deref(), &|key| {
            texts.loc.format(key, &[])
        });
        let error_visible = error.is_some();
        let error_text = error.map_or_else(String::new, |e| {
            texts
                .loc
                .format(keys::RECOVERY_DETAIL_ERROR, &[LocArg::new("error", e)])
        });
        let source_title = texts.loc.format(keys::RECOVERY_DETAIL_SOURCE, &[]);
        let destination_title = texts.loc.format(keys::RECOVERY_DETAIL_DESTINATION, &[]);

        let Some(result) = row.check() else {
            let unchecked = |title: &str, path: &str| RecoveryPathView {
                title: title.to_owned(),
                path: path.to_owned(),
                status_text: String::new(),
                status_tone: RecoveryTone::Neutral,
                now_text: String::new(),
                journal_text: texts.journal_text(entry),
                note: None,
            };
            let destination = entry.destination.as_deref().map(|d| unchecked(&destination_title, d));
            return RecoveryDetails {
                error_text,
                error_visible,
                verdict_text: texts.loc.format(keys::RECOVERY_ROW_CHECKING, &[]),
                verdict_tone: RecoveryTone::Neutral,
                explain_text: texts.loc.format(keys::RECOVERY_DETAIL_CHECKING, &[]),
                action_text: String::new(),
                source: Some(unchecked(&source_title, &entry.source)),
                source_visible: true,
                destination_visible: destination.is_some(),
                no_destination_visible: destination.is_none(),
                destination,
                group_heading: None,
                group_members: None,
            };
        };

        let mut members = None;
        let mut heading = None;
        if let Some(group) = result.group_members.as_deref().filter(|g| !g.is_empty()) {
            members = Some(
                group
                    .iter()
                    .map(|item| RecoveryGroupMemberView {
                        status: texts.group_status_text(
                            item.verdict(),
                            &item.member,
                            item.source(),
                            item.destination(),
                        ),
                        source: item.member.source.clone(),
                        destination: item
                            .member
                            .destination
                            .clone()
                            .unwrap_or_else(|| texts.loc.format(keys::RECOVERY_DETAIL_NO_DESTINATION, &[])),
                    })
                    .collect::<Vec<_>>(),
            );
            heading = Some(texts.loc.format(
                keys::RECOVERY_GROUP_MEMBERS,
                &[LocArg::new("count", group.len() as i64)],
            ));
        }
        let is_group = members.is_some();
        let (source, destination) = if is_group {
            (None, None)
        } else {
            (
                Some(texts.path_view(&source_title, &result.source, entry, false)),
                result
                    .destination
                    .as_ref()
                    .map(|d| texts.path_view(&destination_title, d, entry, result.destination_folder_missing())),
            )
        };
        RecoveryDetails {
            error_text,
            error_visible,
            verdict_text: texts.verdict_text(result.verdict),
            verdict_tone: verdict_tone(result.verdict),
            explain_text: texts.explain_text(result.verdict),
            action_text: texts.action_text(result.verdict),
            source,
            source_visible: !is_group,
            destination_visible: destination.is_some(),
            no_destination_visible: !is_group && destination.is_none(),
            destination,
            group_heading: heading,
            group_members: members,
        }
    }

    // ---- Live read-only check ----

    /// Step 1 of C# `RunChecksAsync(view)`, UI thread. Cancels the previous run (`cancel_check()`), stores a fresh token, resets EVERY
    /// row to "checking" (`set_result(None)` + `view.row_changed(i)` for each, before anything else). No rows: `view.set_check_status("")`
    /// and `None`. Else `view.set_recheck_enabled(false)`, `check_done = 0`, `check_total = rows.len()`,
    /// `view.set_check_status(recovery.check.progress {done: 0, total})` and the run for a worker.
    pub fn begin_checks(
        &mut self,
        view: &mut dyn RecoveryCheckView,
        texts: &RecoveryTexts<'_>,
    ) -> Option<RecoveryCheckRun> {
        self.cancel_check();
        let token = CancelToken::new();
        self.cancel_check = Some(token.clone());
        for index in 0..self.rows.len() {
            self.rows[index].set_result(None);
            view.row_changed(index);
        }
        let total = self.rows.len();
        self.check_total = total;
        self.check_done = 0;
        if total == 0 {
            view.set_check_status("");
            return None;
        }
        view.set_recheck_enabled(false);
        view.set_check_status(&Self::progress_text(texts, 0, total));
        Some(RecoveryCheckRun {
            entries: self.rows.iter().map(|r| r.entry().clone()).collect(),
            checker: Arc::clone(&self.checker),
            token,
        })
    }

    /// Step 3, UI thread, for every message the worker posted, in order. A message whose token is cancelled is dropped (nothing
    /// touches the view or a row): the two C# races are "worker failure after cancel" and "result posted before the cancel but
    /// delivered after". Otherwise: `Result` => the row with that Id (ordinal) gets the result and `view.row_changed(its index)` (a
    /// cleared row is skipped but still counts), `check_done += 1`, status `recovery.check.progress {done, total}`; `Finished` =>
    /// `set_recheck_enabled(true)`, status `recovery.check.done {total, time = current_time_text()}`, `view.check_completed()`;
    /// `Failed` => log `"Recovery live check failed"`, `set_recheck_enabled(true)`, status `""`.
    pub fn on_check_message(
        &mut self,
        message: RecoveryCheckMessage,
        view: &mut dyn RecoveryCheckView,
        texts: &RecoveryTexts<'_>,
    ) {
        // A cancelled run changes nothing: not a row, not the status line, not the button (INV-9 generation guard).
        if message.token.is_canceled() {
            return;
        }
        match message.event {
            RecoveryCheckEvent::Result { id, result } => {
                // Ordinal Id compare; a row cleared meanwhile is skipped but still counts towards the progress.
                if let Some(index) = self.rows.iter().position(|row| row.entry().id == id) {
                    self.rows[index].set_result(Some(result));
                    view.row_changed(index);
                }
                self.check_done += 1;
                view.set_check_status(&Self::progress_text(texts, self.check_done, self.check_total));
            }
            RecoveryCheckEvent::Finished => {
                view.set_recheck_enabled(true);
                let text = texts.loc.format(
                    keys::RECOVERY_CHECK_DONE,
                    &[
                        LocArg::new("total", self.check_total as i64),
                        LocArg::new("time", (self.current_time_text)()),
                    ],
                );
                view.set_check_status(&text);
                view.check_completed();
            }
            RecoveryCheckEvent::Failed { message } => {
                let error = std::io::Error::other(message);
                AppLog::global().error("Recovery live check failed", Some(&error));
                view.set_recheck_enabled(true);
                view.set_check_status("");
            }
        }
    }

    /// Test and tool convenience: `begin_checks`, then `run` with an inline `post` that delivers each message at once.
    pub fn run_checks_blocking(&mut self, view: &mut dyn RecoveryCheckView, texts: &RecoveryTexts<'_>) {
        if let Some(run) = self.begin_checks(view, texts) {
            // Single thread: run the worker part to the end, then deliver its messages in order (what the host's FIFO post does).
            let queue = RefCell::new(Vec::new());
            run.run(&|message| queue.borrow_mut().push(message));
            for message in queue.into_inner() {
                self.on_check_message(message, view, texts);
            }
        }
    }

    /// C# `CancelCheck()`: the window closed (or a new run starts): cancel the latest token. Harmless when none exists.
    pub fn cancel_check(&mut self) {
        if let Some(token) = &self.cancel_check {
            token.cancel();
        }
    }

    // ---- Clear (dismiss) ----

    /// C# `DismissPrompt(rows)`: `None` when there is no dismiss delegate or `row_indices` is empty, else
    /// `recovery.dismiss.confirm.message` with `{count}` = `row_indices.len()`.
    pub fn dismiss_prompt(&self, row_indices: &[usize], loc: &Localizer) -> Option<String> {
        if self.dismiss.is_none() || row_indices.is_empty() {
            return None;
        }
        Some(loc.format(
            keys::RECOVERY_DISMISS_CONFIRM_MESSAGE,
            &[LocArg::new("count", row_indices.len() as i64)],
        ))
    }

    /// C# `Dismiss(rows)`, after the user confirmed. No delegate or no rows => `{ error: None, skipped_count: 0 }`, nothing called.
    /// Else the delegate gets the entries of the rows in the order given; `Err(message)` => `{ error: Some(message), skipped_count: 0 }`
    /// and NO row removed; `Ok(outcome)` => EVERY requested row is removed (by Id, ordinal: the dismissed ones and the ones the
    /// journal reported stale, because a row whose Id was handled elsewhere must not stay looking current) and the result is
    /// `{ error: None, skipped_count: outcome.skipped.len() }`.
    pub fn dismiss(&mut self, row_indices: &[usize]) -> RecoveryDismissResult {
        let Some(dismiss) = &self.dismiss else {
            return RecoveryDismissResult {
                error: None,
                skipped_count: 0,
            };
        };
        let entries: Vec<JournalEntry> = row_indices
            .iter()
            .filter_map(|&i| self.rows.get(i))
            .map(|row| row.entry().clone())
            .collect();
        if entries.is_empty() {
            return RecoveryDismissResult {
                error: None,
                skipped_count: 0,
            };
        }
        match dismiss(&entries) {
            // A failing delegate is no evidence of dismissal: every row stays.
            Err(message) => RecoveryDismissResult {
                error: Some(message),
                skipped_count: 0,
            },
            Ok(outcome) => {
                // Dismissed and stale rows both go: a row whose Id was handled elsewhere must not stay looking current.
                let requested: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
                self.rows.retain(|row| !requested.contains(row.entry().id.as_str()));
                RecoveryDismissResult {
                    error: None,
                    skipped_count: outcome.skipped.len(),
                }
            }
        }
    }

    // ---- Retry ----

    /// C# `CanStartRetry(row)`: a retry delegate exists, a row is selected and no retry runs.
    pub fn can_start_retry(&self, row: Option<usize>) -> bool {
        self.retry.is_some() && row.is_some_and(|i| i < self.rows.len()) && !self.is_retrying
    }

    /// C# `RetryConfirmText(row)`: `texts.retry_confirm_text(entry, check, allow_permanent_delete() == true)`, the setting read NOW.
    pub fn retry_confirm_text(&self, row_index: usize, texts: &RecoveryTexts<'_>) -> String {
        let Some(row) = self.rows.get(row_index) else {
            return String::new();
        };
        let allow = self.allow_permanent_delete.as_ref().is_some_and(|f| f());
        texts.retry_confirm_text(row.entry(), row.check(), allow)
    }

    /// Step 1 of C# `ExecuteRetryAsync(entry, setBusy, rowsChanged)`, UI thread. No retry delegate => `Err("Retry is not
    /// available.")` and nothing changes. Else `is_retrying = true`, `set_busy(true)` and the job for a worker.
    pub fn begin_retry(
        &mut self,
        entry: &JournalEntry,
        set_busy: &mut dyn FnMut(bool),
    ) -> Result<RecoveryRetryJob, String> {
        let Some(retry) = &self.retry else {
            return Err("Retry is not available.".to_owned());
        };
        self.is_retrying = true;
        set_busy(true);
        Ok(RecoveryRetryJob {
            entry: entry.clone(),
            retry: Arc::clone(retry),
        })
    }

    /// Step 3, UI thread, with the worker's outcome. ALWAYS first (C# `finally`, also for `Err`): `is_retrying = false`,
    /// `set_busy(false)`. Then `Err(message)` is returned unchanged (rows untouched); `Ok(result)` with `result.superseded` removes the
    /// rows whose Id equals `entry_id` (ordinal) and calls `rows_changed()` once; the result is returned.
    pub fn finish_retry(
        &mut self,
        entry_id: &str,
        outcome: Result<RecoveryRetryResult, String>,
        set_busy: &mut dyn FnMut(bool),
        rows_changed: &mut dyn FnMut(),
    ) -> Result<RecoveryRetryResult, String> {
        // C# `finally`: first, on every path.
        self.is_retrying = false;
        set_busy(false);
        let result = outcome?;
        if result.superseded {
            self.rows.retain(|row| row.entry().id != entry_id);
            rows_changed();
        }
        Ok(result)
    }

    /// C# static `DescribeRetry(result)`. Superseded => title `recovery.retry.stale.title`, `result.message`, no warning, no close, no
    /// recheck. Else `journal_warning = succeeded && !journal_persisted`; title `dialog.retryDoneJournalFailed.title` (journal
    /// warning) / `dialog.retrySucceeded.title` (success) / `dialog.retryRejected.title`; `is_warning = !(succeeded && !journal_warning)`;
    /// `close_window = succeeded`; `recheck = !succeeded`.
    pub fn describe_retry(result: &RecoveryRetryResult, loc: &Localizer) -> RecoveryRetryNotice {
        if result.superseded {
            return RecoveryRetryNotice {
                title: loc.format(keys::RECOVERY_RETRY_STALE_TITLE, &[]),
                message: result.message.clone(),
                is_warning: false,
                close_window: false,
                recheck: false,
            };
        }
        let journal_warning = result.succeeded && !result.journal_persisted;
        let key = if result.succeeded {
            if journal_warning {
                keys::DIALOG_RETRY_DONE_JOURNAL_FAILED_TITLE
            } else {
                keys::DIALOG_RETRY_SUCCEEDED_TITLE
            }
        } else {
            keys::DIALOG_RETRY_REJECTED_TITLE
        };
        RecoveryRetryNotice {
            title: loc.format(key, &[]),
            message: result.message.clone(),
            is_warning: !(result.succeeded && !journal_warning),
            close_window: result.succeeded,
            recheck: !result.succeeded,
        }
    }
}
