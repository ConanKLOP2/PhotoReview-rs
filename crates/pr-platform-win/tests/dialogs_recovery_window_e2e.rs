//! Native RecoveryDialog view checks; the other C# native scenarios remain tracked in PARITY.
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/dialogs_recovery_fixture.rs"]
mod fixture;
use pr_app::presenters::recovery_window::RecoveryChecker;
use pr_core::file_actions::journal::DismissOutcome;
use pr_core::file_actions::journal_line::JournalEntry;
use pr_core::file_actions::recovery_check::{
    RecoveryCheckResult, RecoveryPathCheck, RecoveryPathStatus, RecoveryVerdict,
};
use pr_platform_win::dialogs::native::controls::basic::ComboBoxControl;
use pr_platform_win::dialogs::native::controls::basic::ListBoxControl;
use pr_platform_win::dialogs::recovery::{RecoveryDialog, RecoveryDialogSeams, layout::*};
use std::sync::Arc;

struct CheckedRows;
impl RecoveryChecker for CheckedRows {
    fn check(&self, entry: &JournalEntry) -> Result<RecoveryCheckResult, String> {
        Ok(RecoveryCheckResult {
            entry: entry.clone(),
            source: RecoveryPathCheck::new(
                entry.source.clone(),
                RecoveryPathStatus::Exists,
                Some(entry.size),
                None,
                true,
            ),
            destination: None,
            group_members: None,
            verdict: if entry.id == "e1" {
                RecoveryVerdict::AlreadyDone
            } else {
                RecoveryVerdict::CanRetry
            },
        })
    }
}

// C#: RecoveryDialogTests.Filter_KeepsTheSelection_OfRowsThatStayVisible
#[test]
#[ignore = "native: needs desktop"]
fn filter_carries_selection_by_identity_when_visible_positions_differ_from_presenter_indices() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let runner = Arc::new(fixture::QueuedRunner::default());
        let mut options = fixture::options();
        options.checker = Arc::new(CheckedRows);
        let mut services = fixture::services();
        services.runner = runner.clone();
        let d = RecoveryDialog::new(
            vec![fixture::entry("e1"), fixture::entry("e2"), fixture::entry("e3")],
            options,
            services,
            RecoveryDialogSeams {
                messages: Some(fixture::FakeMessages::yes()),
                auto_check: false,
                ..Default::default()
            },
            None,
        );
        d.show_modeless(owner, None, None).unwrap();
        d.start_checks();
        runner.run_all();
        d.pump_completions();
        ComboBoxControl::select(d.native().item(FILTER_ID), 1);
        desktop::command(d.native(), FILTER_ID, 1);
        assert_eq!(d.visible_rows(), vec![1, 2]);
        d.select_rows(&[0]);
        assert_eq!(d.selected_rows(), vec![1]);
        ComboBoxControl::select(d.native().item(FILTER_ID), 0);
        desktop::command(d.native(), FILTER_ID, 1);
        assert_eq!(d.selected_rows(), vec![1]);
        ComboBoxControl::select(d.native().item(FILTER_ID), 1);
        desktop::command(d.native(), FILTER_ID, 1);
        assert_eq!(d.selected_rows(), vec![1]);
        d.close();
        desktop::destroy_owner(owner);
    });
}

// C#: RecoveryDialogTests.Opens_WithTheRowsBeforeTheChecks_AsCheckingNeutralBadges
#[test]
#[ignore = "native: needs desktop"]
fn opens_with_rows_and_summary_before_checks() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let dialog = RecoveryDialog::new(
            vec![fixture::entry("e1"), fixture::entry("e2"), fixture::entry("e3")],
            fixture::options(),
            fixture::services(),
            RecoveryDialogSeams {
                messages: Some(fixture::FakeMessages::yes()),
                auto_check: false,
                ..Default::default()
            },
            None,
        );
        dialog.show_modeless(owner, None, None).unwrap();
        assert_eq!(ListBoxControl::count(dialog.native().item(LIST_ID)), 3);
        assert_eq!(dialog.visible_rows(), vec![0, 1, 2]);
        let summary = dialog.with_presenter(|p| p.summary_text(&pr_l10n::localizer::built_in_english_localizer()));
        assert_eq!(dialog.native().get_item_text(SUMMARY_ID), summary);
        assert!(!dialog.row_view(0).text.is_empty());
        assert!(!dialog.row_view(0).badge.unwrap().is_empty());
        assert!(desktop::is_visible(dialog.native(), DETAILS_EMPTY_ID));
        dialog.close();
        desktop::destroy_owner(owner);
    });
}

// C#: RecoveryDialogTests.EmptyJournal_ShowsTheEmptySummary_AndNoRows
#[test]
#[ignore = "native: needs desktop"]
fn empty_journal_shows_its_summary_and_no_rows() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let dialog = RecoveryDialog::new(
            Vec::new(),
            fixture::options(),
            fixture::services(),
            RecoveryDialogSeams {
                messages: Some(fixture::FakeMessages::yes()),
                auto_check: false,
                ..Default::default()
            },
            None,
        );
        dialog.show_modeless(owner, None, None).unwrap();
        assert_eq!(ListBoxControl::count(dialog.native().item(LIST_ID)), 0);
        assert!(!dialog.native().get_item_text(SUMMARY_ID).is_empty());
        assert!(!dialog.is_enabled(RETRY_ID));
        assert!(!dialog.is_enabled(CLEAR_ALL_ID));
        dialog.close();
        desktop::destroy_owner(owner);
    });
}

// C#: RecoveryDialogTests.ClearSelected_RemovesRowsWithoutSelectingNeighbours
#[test]
#[ignore = "native: needs desktop"]
fn removing_selected_entry_does_not_select_its_neighbour_and_refreshes_buttons() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let owner = desktop::create_owner();
        let mut options = fixture::options();
        options.dismiss = Some(Box::new(|entries| {
            Ok(DismissOutcome {
                dismissed: entries.to_vec(),
                skipped: vec![],
            })
        }));
        let dialog = RecoveryDialog::new(
            vec![fixture::entry("e1"), fixture::entry("e2"), fixture::entry("e3")],
            options,
            fixture::services(),
            RecoveryDialogSeams {
                messages: Some(fixture::FakeMessages::yes()),
                auto_check: false,
                ..Default::default()
            },
            None,
        );
        dialog.show_modeless(owner, None, None).unwrap();
        dialog.select_rows(&[1]);
        assert!(dialog.is_enabled(CLEAR_SELECTED_ID));
        desktop::command(dialog.native(), CLEAR_SELECTED_ID, 0);
        assert_eq!(
            dialog.with_presenter(|p| p.rows().iter().map(|r| r.entry().id.clone()).collect::<Vec<_>>()),
            vec!["e1", "e3"]
        );
        assert_eq!(ListBoxControl::count(dialog.native().item(LIST_ID)), 2);
        assert!(dialog.selected_rows().is_empty());
        assert!(!dialog.is_enabled(CLEAR_SELECTED_ID));
        assert!(desktop::is_visible(dialog.native(), DETAILS_EMPTY_ID));
        dialog.close();
        desktop::destroy_owner(owner);
    });
}
