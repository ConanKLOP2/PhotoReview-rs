//! Native W9-E3 checks, confined to test windows and a TempRoot file.
#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;
use fixture::{command, item, on_sta, show_on_owner, text_of};
use pr_platform_win::dialogs::native::{
    dialog::{ID_CANCEL, ID_OK, NativeDialog},
    list_dialog::{LIST_ID, ListDialog, ListDialogContent},
    text_prompt_dialog::{EDIT_ID, TextPromptDialog},
};
use pr_platform_win::win32::{control_constants as cc, dialog_native};
use std::rc::Rc;

// C#: ExternalEditorsUiTests.TextPrompt_BlankIsRefused_ATrimmedNameIsReturned.
#[test]
#[ignore = "native: needs desktop"]
fn text_prompt_blank_is_refused_a_trimmed_name_is_returned() {
    on_sta(|| {
        let prompt = TextPromptDialog::new("Editor name", "Name", " initial ", None);
        let result = show_on_owner(prompt.native(), |dialog, _| {
            assert_eq!(text_of(item(dialog, EDIT_ID)), " initial ");
            // SAFETY: EM_GETSEL with null output pointers returns packed scalar selection indices.
            let selection = unsafe { dialog_native::user32::send_message(item(dialog, EDIT_ID), 0x00b0, 0, 0) };
            assert_eq!(selection as u32, 9 << 16, "all initial text is selected");
            dialog.set_item_text(EDIT_ID, " \t ");
            command(dialog, ID_OK, 0);
            assert!(!dialog.is_closed());
            assert_eq!(prompt.value(), None);
            dialog.set_item_text(EDIT_ID, " Retouch ");
            command(dialog, ID_OK, 0);
        })
        .unwrap();
        assert_eq!(result, ID_OK);
        assert_eq!(prompt.value().as_deref(), Some("Retouch"));
    });
}

// C#: TextPromptDialog.Value stays null after Cancel.
#[test]
#[ignore = "native: needs desktop"]
fn text_prompt_cancel_keeps_value_none() {
    on_sta(|| {
        let prompt = TextPromptDialog::new("Editor name", "Name", "initial", None);
        assert_eq!(
            show_on_owner(prompt.native(), |dialog, _| {
                command(dialog, ID_CANCEL, 0);
            })
            .unwrap(),
            ID_CANCEL
        );
        assert_eq!(prompt.value(), None);
    });
}

// C#: ExternalEditorsUiTests.ChoiceList_PreselectsTheCurrent_AndReportsTheChosenRow.
#[test]
#[ignore = "native: needs desktop"]
fn choice_list_preselects_the_current_and_reports_the_chosen_row() {
    on_sta(|| {
        let localizer = pr_l10n::localizer::Localizer::current();
        let content = Rc::new(ListDialog::new(ListDialogContent::for_choice(
            "Choose",
            "Editor",
            &["First".to_owned(), "Second".to_owned()],
            1,
            &localizer,
        )));
        let native = NativeDialog::new(content.clone());
        assert_eq!(content.selected_index(), -1);
        assert_eq!(
            show_on_owner(&native, |dialog, _| {
                let list = item(dialog, LIST_ID);
                // SAFETY: list is live; GETCURSEL/SETCURSEL carry scalar indices only.
                unsafe {
                    assert_eq!(dialog_native::user32::send_message(list, cc::LB_GETCURSEL, 0, 0), 1);
                    dialog_native::user32::send_message(list, cc::LB_SETCURSEL, usize::MAX, 0);
                }
                command(dialog, ID_OK, 0);
                assert!(!dialog.is_closed());
                assert_eq!(content.selected_index(), -1);
                // SAFETY: selecting valid row zero in this live two-row list; no pointers.
                unsafe {
                    dialog_native::user32::send_message(list, cc::LB_SETCURSEL, 0, 0);
                }
                command(dialog, ID_OK, 0);
            })
            .unwrap(),
            ID_OK
        );
        assert_eq!(content.selected_index(), 0);
    });
}

// C#: OnOk preserves legacy no-selection confirmation when InitialSelectedIndex is negative;
// an invalid nonnegative initial index still requires a selection.
#[test]
#[ignore = "native: needs desktop"]
fn choice_list_invalid_preselection_requires_a_row_and_negative_index_keeps_legacy_confirm() {
    on_sta(|| {
        for selected in [-1, 99] {
            let localizer = pr_l10n::localizer::Localizer::current();
            let content = Rc::new(ListDialog::new(ListDialogContent::for_choice(
                "Choose",
                "Editor",
                &["Only".to_owned()],
                selected,
                &localizer,
            )));
            let native = NativeDialog::new(content.clone());
            let result = show_on_owner(&native, |dialog, _| {
                command(dialog, ID_OK, 0);
                if selected >= 0 {
                    assert!(!dialog.is_closed());
                    command(dialog, ID_CANCEL, 0);
                }
            })
            .unwrap();
            assert_eq!(result, if selected < 0 { ID_OK } else { ID_CANCEL });
            assert_eq!(content.selected_index(), -1);
        }
    });
}

// Source-derived N: only version resources are read; no executable is loaded as code.
#[test]
#[ignore = "native: Windows executable version resources"]
fn product_name_missing_blank_non_pe_are_none_and_notepad_has_a_name() {
    use pr_platform_win::dialogs::native::external_editor_product_name::suggest;
    let root = pr_test_support::TempRoot::new("w9-e3-product-name");
    assert_eq!(suggest(""), None);
    assert_eq!(suggest(" \t"), None);
    assert_eq!(suggest(&root.path().join("missing.exe").to_string_lossy()), None);
    let file = root.path().join("not-pe.exe");
    std::fs::write(&file, b"not an executable").unwrap();
    assert_eq!(suggest(&file.to_string_lossy()), None);
    let notepad = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap())
        .join("System32")
        .join("notepad.exe");
    assert!(suggest(&notepad.to_string_lossy()).is_some_and(|name| !name.trim().is_empty()));
}
