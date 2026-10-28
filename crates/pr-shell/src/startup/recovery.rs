//! Port of the thread hop of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` `RecoverJournalAsync` (lines 571-594, baseline `d92d6aae`) and its call site (lines 383-386). Unit W8-17e.
//! Contract: `contracts/W8-17e.md`.
//!
//! INV-6 / ADR 0003: the reconcile runs AFTER the instance lock and AFTER the window is shown, on a worker (so the first image is not slowed), and BEFORE the message loop takes its first input (the
//! hop result is queued behind the work already posted). The prompt text and decision are `pr_app::startup::recovery`; this file starts the worker and delivers the result to the UI thread.

use std::rc::Rc;

use pr_app::misc::app_log::AppLog;
use pr_app::startup::crash::{UnhandledError, describe_payload};
use pr_app::startup::recovery::prompt_for_failed_entries;
use pr_app::startup::ui_hop::{HopLost, ui_hop};
use pr_core::file_actions::journal_line::JournalEntry;
use pr_core::file_actions::startup_recovery;
use pr_l10n::localizer::Localizer;

use super::state::StartupState;

impl StartupState {
    /// C# `if (!_launch.Smoke) { _ = RecoverJournalAsync(services, main); }`: does nothing for smoke. Otherwise, on the UI thread: `utc_now` is read NOW (`startup_recovery::spawn` does that) and the
    /// reconcile starts on its own thread; a waiter thread (`PhotoReview.JournalRecoveryWait`) `wait()`s for it and sends the `Vec<JournalEntry>` through a `ui_hop`. With
    /// `options.journal_recovery` set (tests) the worker runs that closure instead (`FnOnce(journal, clock, log) -> Vec<JournalEntry>`, on the worker). The hop continuation (UI thread) runs
    /// `catch_unwind(AssertUnwindSafe(|| recovery::prompt_for_failed_entries(&failed, &Localizer::current(), &*services.dialog_service, &mut || { view_model.borrow().show_recovery(); })))`: Yes opens Recovery through a SHARED borrow of the view model (`&self` `show_recovery`, D-W8B6-27: the nested loop of the dialog must still be able to paint), NEVER through `update_view_model` (that holds `borrow_mut` for the whole modal loop). The continuation captures a `Weak` of the state and of the window slot, never a strong `Rc<MainWindow>` (a hop whose job never runs would keep the window alive past the shutdown);
    /// a panic is logged `AppLog::error("Startup journal recovery failed", Some(&UnhandledError(..)))` and swallowed (C# `catch (Exception) when not OOM`), a `HopLost` too (same message, text
    /// "The background step ended without a result."). Never fails the start-up.
    pub(super) fn start_journal_recovery(self: &Rc<Self>) {
        if self.launch.smoke {
            return;
        }
        let (journal, clock, log) = {
            let graph = self.graph.borrow();
            let Some(graph) = graph.as_ref() else {
                return;
            };
            (graph.journal.clone(), graph.clock.clone(), graph.log.clone())
        };
        let Some(services) = self.services.borrow().clone() else {
            return;
        };
        let custom = self.options.borrow_mut().journal_recovery.take();
        let weak_state = Rc::downgrade(self);
        let weak_slot = Rc::downgrade(&self.main);
        let sender = ui_hop::<Vec<JournalEntry>>(self.ui.clone(), move |result| {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let failed = result?;
                if failed.is_empty() || weak_state.upgrade().is_none() {
                    return Ok(());
                }
                // A shared borrow of the view model: the nested loop of the dialog must still be able to paint.
                let window = weak_slot.upgrade().and_then(|slot| slot.borrow().clone());
                let mut show_recovery = || {
                    if let Some(window) = &window {
                        window.view_model().borrow().show_recovery();
                    }
                };
                prompt_for_failed_entries(&failed, &Localizer::current(), &*services.dialog_service, &mut show_recovery);
                Ok::<(), HopLost>(())
            }));
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(lost)) => {
                    AppLog::global().error("Startup journal recovery failed", Some(&UnhandledError(lost.to_string())));
                }
                Err(payload) => {
                    AppLog::global().error(
                        "Startup journal recovery failed",
                        Some(&UnhandledError(describe_payload(payload.as_ref()))),
                    );
                }
            }
        });
        // `utc_now` is read NOW (UI thread) by `spawn`; with a custom reconcile (tests) the closure runs on its own worker.
        let handle = custom
            .is_none()
            .then(|| startup_recovery::spawn(journal.clone(), &*clock, Some(log.clone())));
        let _waiter = std::thread::Builder::new()
            .name("PhotoReview.JournalRecoveryWait".into())
            .spawn(move || {
                let failed = match (handle, custom) {
                    (Some(handle), _) => handle.wait(),
                    (None, Some(custom)) => custom(journal, clock, log),
                    (None, None) => Vec::new(),
                };
                sender.send(failed);
            });
    }
}
