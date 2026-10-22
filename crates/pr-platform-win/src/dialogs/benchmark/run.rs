//! Port of `BenchmarkDialog.cs` lines 398 to 564 (`OnCommand`, `OnCancel`, `Browse`, `Run`, `Observe`, `StartExplorer`'s call site, the `IBenchmarkView`
//! implementation and `Enable`), baseline `d92d6aae`, plus the run driver that replaces the C# `async` presenter (D-W8B5-505). Unit W8-18s. Contract:
//! `contracts/W8-18s.md`.
//!
//! The C# `BenchmarkPresenter` awaits a scan, one session per profile and a save. `pr_app::presenters::benchmark` (W8-10i) is a step machine instead: each press of
//! Run / Quick check gives a [`BenchmarkStep`] (a `Send` job), the job runs on a worker, and its outcome comes back to the UI thread to be fed to the matching `on_*`
//! method, which returns the next step. Here:
//! - a job is handed to a [`BackgroundRunner`] by [`dispatch_step`]; its outcome (and every progress message of a profile job) is sent to a
//!   `CompletionInbox<BenchmarkEvent>` whose wake-up posts [`WM_BENCHMARK_WAKE`] to the dialog window ([`post_wake`]);
//! - the dialog procedure receives the message and `BenchmarkContent::drain_inbox` applies the events in arrival order ([`apply_event`]), dispatching each next step;
//! - the presenter writes to a [`DialogView`] that applies `set_status`, `set_progress` and `set_running` to the controls at once and only RECORDS `refresh_rows`
//!   (`rows_dirty`); the grid is rebuilt after the presenter call returns, because the rebuild reads the presenter (D-W8B5-504).
//!
//! A window closed in the middle of a run cancels the token; the job still ends through its own cancel path and disposes its session on the worker; its late event finds
//! no window to wake and is dropped (no Fail row, nothing applied; D-W8B5-506).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::Cell;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicIsize;

use pr_app::coordinators::zoom_detail::{BackgroundRunner, CompletionInbox};
use pr_app::presenters::benchmark::{
    BenchmarkPresenter, BenchmarkProfileOutcome, BenchmarkProgressMessage, BenchmarkScanOutcome, BenchmarkStep,
    BenchmarkView,
};
use pr_app::presenters::settings_field_map::FieldContext;

use super::state::BenchmarkContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::dialogs::zoom_prompt::DialogMeasure;
use crate::win32::structs::Hwnd;

/// What a worker sends to the UI thread. `Send` (checked by a compile-time assertion in the test file).
// One event per job outcome, moved straight into `apply_event`; boxing the report variant would only add an allocation (same justification as `BenchmarkProfileOutcome`).
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum BenchmarkEvent {
    /// A progress message of the profile job in flight (C# `owner._ui.Post(() => owner.ApplyProgress(..))`).
    Progress(BenchmarkProgressMessage),
    /// The folder scan finished.
    Scan(BenchmarkScanOutcome),
    /// A profile finished (its session is already disposed).
    Profile(BenchmarkProfileOutcome),
    /// The report save finished: the summary path or the error text.
    Save(Result<String, String>),
}

/// The run driver state (the C# fields it replaces: `_ui`, the `Task` of `LastRunTask`).
pub struct RunState {
    /// Worker-to-UI mailbox; the wake function posts `WM_BENCHMARK_WAKE` to the window in `hwnd`.
    pub(super) inbox: CompletionInbox<BenchmarkEvent>,
    /// Runs the jobs.
    pub(super) runner: Arc<dyn BackgroundRunner>,
    /// The dialog window, `0` before `WM_INITDIALOG` and after the close; read by the wake function on any thread.
    pub(super) hwnd: Arc<AtomicIsize>,
    /// C# `CollectionChanged`: the grid must be rebuilt (set by [`DialogView::refresh_rows`], cleared by `flush_view_effects`).
    pub(super) rows_dirty: Cell<bool>,
}

impl fmt::Debug for RunState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunState").finish_non_exhaustive()
    }
}

impl RunState {
    /// `hwnd = 0`; `inbox = CompletionInbox::new(wake)` where `wake` is a closure over a clone of `hwnd` that calls [`post_wake`] with its current value; `runner =
    /// runner or Arc::new(ThreadRunner)`; `rows_dirty = false`.
    pub fn new(runner: Option<Arc<dyn BackgroundRunner>>) -> Self {
        todo!()
    }
}

/// Posts `WM_BENCHMARK_WAKE` (wparam and lparam 0) to `hwnd`; `0` does nothing; a failing `PostMessage` (the window is gone) is ignored. Callable from any thread
/// (one SAFETY-commented `user32::post_message` call: the message carries no pointer).
pub fn post_wake(hwnd: Hwnd) {
    todo!()
}

/// Hands the next job to the runner (C# `Observe(Task)` for the part that starts work): `Idle` does nothing; `Scan(job)` runs `inbox.send(Scan(job.run()))` on the
/// runner (name `"benchmark-scan"`); `Profile(job)` runs `job.run(&|m| inbox.send(Progress(m)))` and then `inbox.send(Profile(outcome))` (name `"benchmark-profile"`);
/// `Save(job)` runs `inbox.send(Save(job.run()))` (name `"benchmark-save"`). The jobs catch their own panics (W8-10i), so no closure here can unwind.
pub fn dispatch_step(runner: &dyn BackgroundRunner, inbox: &CompletionInbox<BenchmarkEvent>, step: BenchmarkStep) {
    todo!()
}

/// Feeds one event to the presenter and returns the next step: `Progress(m)` -> `on_progress(m, view, ctx)` and `Idle`; `Scan(o)` -> `on_scan_finished(o, view, ctx)`;
/// `Profile(o)` -> `on_profile_finished(o, view, ctx)`; `Save(r)` -> `on_save_finished(r, view, ctx)`. Pure over fakes.
pub fn apply_event(
    presenter: &mut BenchmarkPresenter,
    event: BenchmarkEvent,
    view: &mut dyn BenchmarkView,
    ctx: &FieldContext<'_>,
) -> BenchmarkStep {
    todo!()
}

/// C# `Enable` calls of `SetRunning`, pure and in the C# order: `[(RUN_ID, !running), (QUICK_CHECK_ID, !running), (CANCEL_RUN_ID, running), (BROWSE_ID, !running),
/// (PROFILES_ID, !running), (LIMIT_ID, !running), (FOLDER_ID, !running)]`.
pub fn enabled_flags(running: bool) -> [(i32, bool); 7] {
    todo!()
}

/// C# focus rescue of `SetRunning`: when the control that had the focus is now disabled, the focus goes to Cancel (`running`) or to Run (not running); otherwise
/// `None` (the focus stays). `focus_is_disabled` is `focus != 0 && !IsWindowEnabled(focus)` read AFTER the `Enable` calls.
pub fn focus_target(running: bool, focus_is_disabled: bool) -> Option<i32> {
    todo!()
}

/// The height the status line needs for `text`, pure: `0` when `text` is empty, else `m.measure_text_height(text, max(1, client_width - 2 * margin))`. `SetStatus` re-runs
/// the layout only when this differs from the height of the last layout.
pub fn status_line_height(m: &dyn DialogMeasure, text: &str, client_width: i32, margin: i32) -> i32 {
    todo!()
}

/// The view the presenter writes to (C# `IBenchmarkView`). Borrows the dialog and its content; NEVER touches the presenter.
pub(super) struct DialogView<'a> {
    pub(super) dialog: &'a NativeDialog,
    pub(super) content: &'a BenchmarkContent,
}

impl BenchmarkView for DialogView<'_> {
    /// C# `SetStatus(text)`: nothing when the window is gone (`hwnd == 0`); `set_item_text(STATUS_ID, text)`; `height = status_line_height(dialog, text,
    /// current_client_size().0, layout::margin(dialog))`; `run_layout()` only when `height != layout.status_height` (progress arrives often; a full layout erases and
    /// repaints the window).
    fn set_status(&mut self, text: &str) {
        todo!()
    }

    /// C# `SetProgress(fraction)`: `ProgressControl::set_fraction(item(PROGRESS_ID), fraction)` when the window exists.
    fn set_progress(&mut self, fraction: f64) {
        todo!()
    }

    /// C# `SetRunning(running)`: nothing without a window. `focus = get_focus()`; `enable_window(item(id), enabled)` for each pair of `enabled_flags(running)` whose
    /// control exists; then `focus_target(running, focus != 0 && !is_window_enabled(focus))` -> `set_focus(item(target))`.
    fn set_running(&mut self, running: bool) {
        todo!()
    }

    /// C# `RefreshRows()` (and the `CollectionChanged` event): records `rows_dirty = true`; the grid is rebuilt by `flush_view_effects` after the presenter call.
    fn refresh_rows(&mut self) {
        todo!()
    }

    /// C# `OpenInExplorer(arguments)`: `(content.open_in_explorer)(arguments)`.
    fn open_in_explorer(&mut self, arguments: &str) {
        todo!()
    }
}

impl BenchmarkContent {
    /// C# `OnCommand(id, notification)`: `notification != BN_CLICKED` -> `false`. `BROWSE_ID` -> `browse`; `SELECT_ALL_ID` / `SELECT_NONE_ID` ->
    /// `ListBoxControl::select_all(item(PROFILES_ID), true / false)`; `RUN_ID` -> `run_click`; `QUICK_CHECK_ID` -> `quick_check_click`; `CANCEL_RUN_ID` -> `presenter.cancel()`;
    /// `OPEN_REPORT_ID` -> `presenter.open_report(view)`; `OPEN_FOLDER_ID` -> `presenter.open_result_folder(view)`; `CLOSE_ID` -> `dialog.close()`; each returns `true`.
    /// Anything else `false`. Every presenter call that gets a view is followed by `flush_view_effects`.
    pub(super) fn command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        todo!()
    }

    /// C# `OnCancel()` (Esc): not running -> `false` (the base closes the window); running -> `presenter.cancel()` and `true` (the window stays).
    pub(super) fn cancel_requested(&self, dialog: &NativeDialog) -> bool {
        todo!()
    }

    /// C# `Browse()`: `current = get_item_text(FOLDER_ID)`; `initial = Some(current)` only when `Path::new(&current).is_dir()` (C# `Directory.Exists`), else `None`;
    /// `folder = (pick_folder)(dialog.hwnd(), &tr::benchmark_pick_folder_title(), initial)`; a non-empty answer replaces the folder box text.
    pub(super) fn browse(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `Run()`: the selected indices of the profile list (`ListBoxControl::selected_indices`) map to profiles (`profile_items()[i].profile.clone()`, an index out of
    /// range is skipped); folder and limit texts are read BEFORE the borrow; `with_field_context(|ctx| presenter.borrow_mut().run_selected(&profiles, &folder,
    /// Some(&limit), &mut view, ctx))`; borrow released; `flush_view_effects`; `dispatch_step(runner, inbox, step)`.
    pub(super) fn run_click(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// C# `QuickCheckId` branch: the same with `quick_check(&folder, Some(&limit), &mut view, ctx)`; the selection is not read.
    pub(super) fn quick_check_click(&self, dialog: &NativeDialog) {
        todo!()
    }

    /// Applies every queued event in arrival order (the `WM_BENCHMARK_WAKE` handler): for each event `with_field_context` + `presenter.borrow_mut()` +
    /// [`apply_event`] with a [`DialogView`]; the borrow is released; the returned step is dispatched; after the last event `flush_view_effects`. The events are
    /// taken out of the inbox first (`drain()`); a presenter that is already borrowed (cannot happen: no presenter call pumps messages) makes `try_borrow_mut` fail:
    /// that event is logged with `AppLog` and dropped (never a panic, R0003).
    pub(super) fn drain_inbox(&self, dialog: &NativeDialog) {
        todo!()
    }
}
