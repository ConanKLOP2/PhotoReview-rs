//! Port of the thread hop of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` `RecoverJournalAsync` (lines 571-594, baseline `d92d6aae`) and its call site (lines 383-386). Unit W8-17e.
//! Contract: `contracts/W8-17e.md`.
//!
//! INV-6 / ADR 0003: the reconcile runs AFTER the instance lock and AFTER the window is shown, on a worker (so the first image is not slowed), and BEFORE the message loop takes its first input (the
//! hop result is queued behind the work already posted). The prompt text and decision are `pr_app::startup::recovery`; this file starts the worker and delivers the result to the UI thread.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::rc::Rc;

use super::state::StartupState;

impl StartupState {
    /// C# `if (!_launch.Smoke) { _ = RecoverJournalAsync(services, main); }`: does nothing for smoke. Otherwise, on the UI thread: `utc_now` is read NOW (`startup_recovery::spawn` does that) and the
    /// reconcile starts on its own thread; a waiter thread (`PhotoReview.JournalRecoveryWait`) `wait()`s for it and sends the `Vec<JournalEntry>` through a `ui_hop`. With
    /// `options.journal_recovery` set (tests) the worker runs that closure instead (`FnOnce(journal, clock, log) -> Vec<JournalEntry>`, on the worker). The hop continuation (UI thread) runs
    /// `catch_unwind(AssertUnwindSafe(|| recovery::prompt_for_failed_entries(&failed, &Localizer::current(), &*services.dialog_service, &mut || { view_model.borrow().show_recovery(); })))`: Yes opens Recovery through a SHARED borrow of the view model (`&self` `show_recovery`, D-W8B6-27: the nested loop of the dialog must still be able to paint), NEVER through `update_view_model` (that holds `borrow_mut` for the whole modal loop). The continuation captures a `Weak` of the state and of the window slot, never a strong `Rc<MainWindow>` (a hop whose job never runs would keep the window alive past the shutdown);
    /// a panic is logged `AppLog::error("Startup journal recovery failed", Some(&UnhandledError(..)))` and swallowed (C# `catch (Exception) when not OOM`), a `HopLost` too (same message, text
    /// "The background step ended without a result."). Never fails the start-up.
    pub(super) fn start_journal_recovery(self: &Rc<Self>) {
        todo!()
    }
}
