//! W8-17e start-up recovery prompt (C-27, W-11), pure half. Contract: `contracts/W8-17e.md`.
use pr_app::startup::recovery::*;
use pr_core::abstractions::{BatchReviewItem, DialogService, SkippedEntry};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_l10n::localizer::built_in_english_localizer;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

struct Dialogs {
    answer: bool,
    asked: RefCell<Vec<(String, String)>>,
    errors: Cell<usize>,
}

impl Dialogs {
    fn new(answer: bool) -> Self {
        Self {
            answer,
            asked: RefCell::new(Vec::new()),
            errors: Cell::new(0),
        }
    }
}

impl DialogService for Dialogs {
    fn show_confirmation(&self, title: &str, message: &str) -> bool {
        self.asked.borrow_mut().push((title.to_owned(), message.to_owned()));
        self.answer
    }
    fn show_message(&self, _title: &str, _message: &str) {}
    fn show_error(&self, _title: &str, _message: &str) {
        self.errors.set(self.errors.get() + 1);
    }
    fn pick_folder(&self, _initial_folder: Option<&Path>) -> Option<PathBuf> {
        None
    }
    fn show_batch_review(&self, _items: &[BatchReviewItem]) -> bool {
        false
    }
    fn show_recovery(&self) {}
    fn show_diagnostics(&self) {}
    fn show_settings(&self) -> bool {
        false
    }
    fn show_benchmark(&self, _folder: Option<&Path>) {}
    fn show_skipped_files(&self, _entries: &[SkippedEntry]) {}
}

fn failed_entry(id: &str) -> JournalEntry {
    let when = JournalDateTime::utc(DateTimeUtc::from_ticks(639_000_000_000_000_000).unwrap());
    JournalEntry {
        id: id.to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Failed,
        source: format!("source-{id}.jpg"),
        destination: None,
        size: 1,
        last_write_utc: when,
        timestamp_utc: when,
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

#[test]
fn recovery_failed_operations_prompt_with_count_and_open_recovery_when_confirmed() {
    // C#: AppJournalForwardBranchTests.RecoverJournalAsync_FailedOperations_PromptWithCountAndOpenRecoveryWhenConfirmed.
    let localizer = built_in_english_localizer();
    let dialogs = Dialogs::new(true);
    let mut opened = 0;
    let failed = [failed_entry("a"), failed_entry("b"), failed_entry("c")];
    let outcome = prompt_for_failed_entries(&failed, &localizer, &dialogs, &mut || opened += 1);
    assert_eq!(outcome, RecoveryPromptOutcome::OpenedRecovery);
    assert_eq!(opened, 1);
    let asked = dialogs.asked.borrow();
    assert_eq!(asked.len(), 1, "one box at most");
    assert_eq!(asked[0], recovery_prompt_texts(&localizer, 3));
    assert!(asked[0].1.contains('3'));
}

#[test]
fn recovery_declined_prompt_does_not_open_recovery() {
    // C#: AppJournalForwardBranchTests.RecoverJournalAsync_FailedOperations_DeclinedPromptDoesNotOpenRecovery.
    let localizer = built_in_english_localizer();
    let dialogs = Dialogs::new(false);
    let mut opened = 0;
    let outcome = prompt_for_failed_entries(&[failed_entry("a")], &localizer, &dialogs, &mut || opened += 1);
    assert_eq!(outcome, RecoveryPromptOutcome::Declined);
    assert_eq!(opened, 0);
    assert_eq!(dialogs.asked.borrow().len(), 1);
}

#[test]
fn recovery_nothing_failed_shows_no_prompt() {
    // C#: AppJournalForwardBranchTests.RecoverJournalAsync_NothingFailed_ShowsNoPrompt.
    let localizer = built_in_english_localizer();
    let dialogs = Dialogs::new(true);
    let mut opened = 0;
    let outcome = prompt_for_failed_entries(&[], &localizer, &dialogs, &mut || opened += 1);
    assert_eq!(outcome, RecoveryPromptOutcome::NothingFailed);
    assert_eq!(opened, 0);
    assert!(dialogs.asked.borrow().is_empty());
    assert_eq!(dialogs.errors.get(), 0);
}

#[test]
fn recovery_prompt_texts_use_the_singular_and_plural_forms() {
    // new: keys `.one` / `.other` of `dialog.startupRecoveryFailed.message`.
    let localizer = built_in_english_localizer();
    let (title, one) = recovery_prompt_texts(&localizer, 1);
    let (_, many) = recovery_prompt_texts(&localizer, 5);
    assert_eq!(title, "Interrupted file operations");
    assert!(one.starts_with("1 file operation was interrupted"), "{one}");
    assert!(many.starts_with("5 file operations were interrupted"), "{many}");
}
