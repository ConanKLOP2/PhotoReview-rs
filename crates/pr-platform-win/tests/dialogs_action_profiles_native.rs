//! Desktop checks for the W8-18q action profiles dialog. Other C# native
//! scenarios remain open in the parity row until they have real Rust twins.

#[path = "common/dialogs_dialog_fixture.rs"]
mod dialogs_dialog_fixture;

use dialogs_dialog_fixture::{activate_visual_styles, command, item, on_sta, set_text, show_on_owner, text_of};
use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::ReviewAction;
use pr_platform_win::dialogs::action_profiles::DialogWarningSink;
use pr_platform_win::dialogs::action_profiles::{
    ADD_ID, APPLY_ID, ActionProfilesDialog, CONFIRM_ID, DESTINATION_ID, LIST_ID, NAME_ID, OPERATION_ID, REMOVE_ID,
    SHORTCUT_ID,
};
use pr_platform_win::dialogs::native::controls::basic::{ComboBoxControl, ListBoxControl};
use pr_platform_win::dialogs::native::controls::custom::ButtonStateControl;
use pr_platform_win::dialogs::native::dialog::{ID_CANCEL, ID_OK};
use pr_platform_win::win32::structs::Hwnd;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct RecordingWarnings(RefCell<Vec<(Hwnd, String, String)>>);

impl DialogWarningSink for RecordingWarnings {
    fn show_warning(&self, owner: Hwnd, title: &str, message: &str) {
        self.0.borrow_mut().push((owner, title.to_owned(), message.to_owned()));
    }
}

fn sample() -> Vec<ReviewAction> {
    let mut keep = ReviewAction::new("Keep");
    keep.shortcut = "F6".to_owned();
    keep.destination = "Keep".to_owned();
    let mut copy = ReviewAction::new("Copy out");
    copy.shortcut = "F7".to_owned();
    copy.operation = FileOperationType::Copy;
    copy.destination = "Out".to_owned();
    copy.confirm = true;
    let mut trash = ReviewAction::new("Trash");
    trash.shortcut = "F8".to_owned();
    trash.operation = FileOperationType::Recycle;
    trash.destination = "x".to_owned();
    vec![keep, copy, trash]
}

// C#: ActionProfilesDialogTests.Opens_WithTheListAndTheFirstRowsFields
#[test]
#[ignore = "native: needs desktop"]
fn opens_with_the_list_and_the_first_rows_fields() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&sample(), None, None, None);
        let result = show_on_owner(dialog.native(), |d, _| {
            let list = item(d, LIST_ID);
            assert_eq!(ListBoxControl::count(list), 3);
            assert_eq!(ListBoxControl::text_at(list, 0), "Keep");
            assert_eq!(ListBoxControl::text_at(list, 1), "Copy out");
            assert_eq!(ListBoxControl::text_at(list, 2), "Trash");
            assert_eq!(ListBoxControl::selected_index(list), 0);
            assert_eq!(text_of(item(d, NAME_ID)), "Keep");
            assert_eq!(text_of(item(d, SHORTCUT_ID)), "F6");
            assert_eq!(text_of(item(d, DESTINATION_ID)), "Keep");
            assert_eq!(ComboBoxControl::selected_index(item(d, OPERATION_ID)), 0);
            assert_eq!(ComboBoxControl::count(item(d, OPERATION_ID)), 3);
            assert!(!ButtonStateControl::is_checked(item(d, CONFIRM_ID)));
            d.close();
        })
        .expect("show action profiles dialog");
        assert_eq!(result, ID_CANCEL);
    });
}

// C#: ActionProfilesDialogTests.EmptyList_ShowsEmptyFields_AndNothingIsSelected
#[test]
#[ignore = "native: needs desktop"]
fn empty_list_shows_empty_fields_and_nothing_is_selected() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&[], None, None, None);
        let result = show_on_owner(dialog.native(), |d, _| {
            assert_eq!(ListBoxControl::count(item(d, LIST_ID)), 0);
            assert_eq!(ListBoxControl::selected_index(item(d, LIST_ID)), -1);
            assert_eq!(text_of(item(d, NAME_ID)), "");
            assert_eq!(text_of(item(d, SHORTCUT_ID)), "");
            d.close();
        })
        .expect("show empty action profiles dialog");
        assert_eq!(result, ID_CANCEL);
    });
}

// C#: ActionProfilesDialogTests.TheListShowsTheShortcutAsABadge
#[test]
#[ignore = "native: needs desktop"]
fn the_list_shows_the_shortcut_as_a_badge() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&sample(), None, None, None);
        let result = show_on_owner(dialog.native(), |d, _| {
            let first = dialog.row_text(0);
            assert_eq!(first.text, "Keep");
            assert_eq!(first.badge.as_deref(), Some("F6"));
            let last = dialog.row_text(2);
            assert_eq!(last.text, "Trash");
            assert_eq!(last.badge.as_deref(), Some("F8"));
            d.close();
        })
        .expect("show action profiles dialog");
        assert_eq!(result, ID_CANCEL);
    });
}

// C#: ActionProfilesDialogTests.SelectingAnotherRow_SavesThePreviousOne_RefreshesTheList_AndShowsTheNewFields
#[test]
#[ignore = "native: needs desktop"]
fn selecting_another_row_saves_previous_refreshes_list_and_shows_new_fields() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&sample(), None, None, None);
        let result = show_on_owner(dialog.native(), |d, _| {
            set_text(d, NAME_ID, "  Keep it  ");
            set_text(d, SHORTCUT_ID, " F9 ");
            ComboBoxControl::select(item(d, OPERATION_ID), 1);
            ListBoxControl::select_single(item(d, LIST_ID), 1);
            command(d, LIST_ID, 1);
            assert_eq!(ListBoxControl::text_at(item(d, LIST_ID), 0), "Keep it");
            assert_eq!(text_of(item(d, NAME_ID)), "Copy out");
            assert_eq!(text_of(item(d, SHORTCUT_ID)), "F7");
            assert_eq!(ComboBoxControl::selected_index(item(d, OPERATION_ID)), 1);
            assert!(ButtonStateControl::is_checked(item(d, CONFIRM_ID)));
            d.close();
        })
        .expect("show action profiles dialog");
        assert_eq!(result, ID_CANCEL);
        let first = &dialog.actions()[0];
        assert_eq!(first.name, "Keep it");
        assert_eq!(first.shortcut, "F9");
        assert_eq!(first.operation, FileOperationType::Copy);
    });
}

// C#: ActionProfilesDialogTests.OperationAndConfirm_SurviveASelectionRoundTrip
#[test]
#[ignore = "native: needs desktop"]
fn operation_and_confirm_survive_a_selection_round_trip() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&sample(), None, None, None);
        show_on_owner(dialog.native(), |d, _| {
            ComboBoxControl::select(item(d, OPERATION_ID), 2);
            ButtonStateControl::set_checked(item(d, CONFIRM_ID), true);
            ListBoxControl::select_single(item(d, LIST_ID), 2);
            command(d, LIST_ID, 1);
            ListBoxControl::select_single(item(d, LIST_ID), 0);
            command(d, LIST_ID, 1);
            assert_eq!(ComboBoxControl::selected_index(item(d, OPERATION_ID)), 2);
            assert!(ButtonStateControl::is_checked(item(d, CONFIRM_ID)));
            d.close();
        })
        .expect("show action profiles dialog");
    });
}

// C#: ActionProfilesDialogTests.Add_AppendsANewActionWithTheNextFreeShortcut_AndOpensIt
#[test]
#[ignore = "native: needs desktop"]
fn add_appends_a_new_action_with_the_next_free_shortcut_and_opens_it() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&sample(), None, None, None);
        show_on_owner(dialog.native(), |d, _| {
            set_text(d, NAME_ID, "Edited");
            command(d, ADD_ID, 0);
            assert_eq!(ListBoxControl::count(item(d, LIST_ID)), 4);
            assert_eq!(ListBoxControl::text_at(item(d, LIST_ID), 0), "Edited");
            assert_eq!(ListBoxControl::selected_index(item(d, LIST_ID)), 3);
            assert_eq!(text_of(item(d, SHORTCUT_ID)), "F9");
            assert_eq!(text_of(item(d, NAME_ID)), ListBoxControl::text_at(item(d, LIST_ID), 3));
            assert_eq!(text_of(item(d, DESTINATION_ID)), "Output");
            d.close();
        })
        .expect("show action profiles dialog");
        assert_eq!(dialog.actions()[0].name, "Edited");
    });
}

// C#: ActionProfilesDialogTests.Remove_SelectsTheNextRow_AndRemovingTheLastOneClearsTheFields
#[test]
#[ignore = "native: needs desktop"]
fn remove_selects_the_next_row_and_removing_the_last_one_clears_fields() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&sample(), None, None, None);
        show_on_owner(dialog.native(), |d, _| {
            ListBoxControl::select_single(item(d, LIST_ID), 1);
            command(d, LIST_ID, 1);
            command(d, REMOVE_ID, 0);
            assert_eq!(ListBoxControl::count(item(d, LIST_ID)), 2);
            assert_eq!(text_of(item(d, NAME_ID)), "Trash");
            command(d, REMOVE_ID, 0);
            assert_eq!(ListBoxControl::count(item(d, LIST_ID)), 1);
            assert_eq!(text_of(item(d, NAME_ID)), "Keep");
            command(d, REMOVE_ID, 0);
            assert_eq!(ListBoxControl::count(item(d, LIST_ID)), 0);
            assert_eq!(text_of(item(d, NAME_ID)), "");
            assert_eq!(text_of(item(d, SHORTCUT_ID)), "");
            assert_eq!(text_of(item(d, DESTINATION_ID)), "");
            assert!(!ButtonStateControl::is_checked(item(d, CONFIRM_ID)));
            command(d, REMOVE_ID, 0);
            d.close();
        })
        .expect("show action profiles dialog");
    });
}

// C#: ActionProfilesDialogTests.Apply_WithAValidList_ClosesWithOk_AndKeepsTheEdits
#[test]
#[ignore = "native: needs desktop"]
fn apply_valid_list_closes_with_ok_and_keeps_edits() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&sample(), None, None, None);
        let result = show_on_owner(dialog.native(), |d, _| {
            set_text(d, NAME_ID, "Renamed");
            ComboBoxControl::select(item(d, OPERATION_ID), 2);
            command(d, APPLY_ID, 0);
        })
        .expect("show action profiles dialog");
        assert_eq!(result, ID_OK);
        assert_eq!(dialog.actions()[0].name, "Renamed");
        assert_eq!(dialog.actions()[0].operation, FileOperationType::Recycle);
    });
}

// C#: ActionProfilesDialogTests.Apply_WithABlankName_WarnsAndKeepsTheDialogOpen
#[test]
#[ignore = "native: needs desktop"]
fn apply_blank_name_warns_and_keeps_the_dialog_open() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let warnings = Rc::new(RecordingWarnings::default());
        let dialog = ActionProfilesDialog::new(&sample(), None, Some(warnings.clone()), None);
        let result = show_on_owner(dialog.native(), |d, _| {
            set_text(d, NAME_ID, "   ");
            command(d, APPLY_ID, 0);
            assert!(!d.is_closed());
            let shown = warnings.0.borrow();
            assert_eq!(shown.len(), 1);
            assert_eq!(shown[0].0, d.hwnd());
            assert_eq!(shown[0].1, pr_l10n::tr::dialog_action_profiles_invalid_title());
            assert_eq!(shown[0].2, pr_l10n::tr::dialog_action_profiles_invalid_message());
            d.close();
        })
        .expect("show action profiles dialog");
        assert_eq!(result, ID_CANCEL);
    });
}

// C#: ActionProfilesDialogTests.Apply_WithDuplicateShortcuts_Warns
#[test]
#[ignore = "native: needs desktop"]
fn apply_duplicate_shortcuts_warns() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let warnings = Rc::new(RecordingWarnings::default());
        let dialog = ActionProfilesDialog::new(&sample(), None, Some(warnings.clone()), None);
        show_on_owner(dialog.native(), |d, _| {
            set_text(d, SHORTCUT_ID, "F7");
            command(d, APPLY_ID, 0);
            assert!(!d.is_closed());
            assert_eq!(warnings.0.borrow().len(), 1);
            d.close();
        })
        .expect("show action profiles dialog");
    });
}

// C#: ActionProfilesDialogTests.Apply_WithADestinationOutsideThePhotoFolder_ReportsThePresentersReason
#[test]
#[ignore = "native: needs desktop"]
fn apply_destination_outside_photo_folder_reports_presenter_reason() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let warnings = Rc::new(RecordingWarnings::default());
        let dialog = ActionProfilesDialog::new(&sample(), None, Some(warnings.clone()), None);
        show_on_owner(dialog.native(), |d, _| {
            set_text(d, DESTINATION_ID, "..\\elsewhere");
            command(d, APPLY_ID, 0);
            assert!(!d.is_closed());
            let shown = warnings.0.borrow();
            assert_eq!(shown.len(), 1);
            assert_eq!(shown[0].0, d.hwnd());
            assert_eq!(
                shown[0].2,
                pr_l10n::tr::action_profiles_error_destination_escapes("Keep")
            );
            d.close();
        })
        .expect("show action profiles dialog");
    });
}
