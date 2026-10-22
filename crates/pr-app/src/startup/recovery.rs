//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 567-594 (`RecoverJournalAsync`, baseline `d92d6aae`): the start-up recovery prompt (C-27, W-11). Unit W8-17e.
//! Contract: `contracts/W8-17e.md`.
//!
//! INV-6 / ADR 0003: pending journal operations are reconciled AFTER the instance lock and AFTER the window is shown, on a worker, so the first image is not slowed.
//! Entries that reconcile had to mark Failed are reported ONCE, with an offer to open the Recovery window (W-04). Nothing is replayed or deleted here. The reconcile itself is
//! `pr_core::file_actions::startup_recovery::spawn` (W3-03, merged); the thread hop and the window are `pr_shell::startup::recovery`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_core::abstractions::DialogService;
use pr_core::file_actions::journal_line::JournalEntry;
use pr_l10n::localizer::Localizer;

/// What the prompt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPromptOutcome {
    /// Nothing was Failed: no box.
    NothingFailed,
    /// The box was shown and answered No.
    Declined,
    /// The box was shown, answered Yes, and the Recovery window was requested.
    OpenedRecovery,
}

/// `(title, message)` of the confirmation: `Tr.DialogStartupRecoveryFailedTitle` = `localizer.get("dialog.startupRecoveryFailed.title")` and
/// `Tr.DialogStartupRecoveryFailedMessage(count)` = `localizer.format_plural("dialog.startupRecoveryFailed.message", count as i64, &[LocArg::new("count", count)])` (keys `.one` / `.other`).
pub fn recovery_prompt_texts(localizer: &Localizer, failed_count: usize) -> (String, String) {
    todo!()
}

/// C# lines 579-588 after the reconcile finished: `failed` empty -> `NothingFailed` (return); else `dialogs.show_confirmation(title, message)` ONCE; `true` -> `show_recovery()` then
/// `OpenedRecovery`; `false` -> `Declined`. UI thread. A panic of `dialogs` is the caller's (the C# `catch (Exception) -> AppLog.Error("Startup journal recovery failed", ex)` is
/// `pr_shell::startup::recovery`, which wraps this call).
pub fn prompt_for_failed_entries(
    failed: &[JournalEntry],
    localizer: &Localizer,
    dialogs: &dyn DialogService,
    show_recovery: &mut dyn FnMut(),
) -> RecoveryPromptOutcome {
    todo!()
}
