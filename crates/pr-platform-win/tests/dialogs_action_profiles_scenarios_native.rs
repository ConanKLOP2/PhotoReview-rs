//! Lane st-dialog-scenarios: the C# `ActionProfilesDialogTests` scenarios that had no Rust twin yet (Enter in the name box, every cancel
//! path, Import x3, Export x2). Each opens the dialog on a hidden owner, drives it from the `ready` callback (so nothing blocks) and
//! the file dialog is a fake: no Explorer, no real picker, files live in a `TempRoot`.

#[path = "common/dialogs_dialog_fixture.rs"]
mod dialogs_dialog_fixture;

use dialogs_dialog_fixture::{
    SC_CLOSE, VK_ESCAPE, VK_RETURN, activate_visual_styles, command, drain, item, key, on_sta, set_text, show_on_owner,
    text_of,
};
use pr_app::presenters::action_list_json::serialize_for_export;
use pr_core::model::enums::FileOperationType;
use pr_core::settings::shortcuts::ReviewAction;
use pr_platform_win::dialogs::action_profiles::{
    ActionProfilesDialog, CONFIRM_ID, DialogWarningSink, EXPORT_ID, IMPORT_ID, LIST_ID, NAME_ID, OPERATION_ID,
    SHORTCUT_ID,
};
use pr_platform_win::dialogs::file_dialog::{FileDialogFilter, ShellFileDialog};
use pr_platform_win::dialogs::native::controls::basic::{ComboBoxControl, ListBoxControl};
use pr_platform_win::dialogs::native::controls::custom::ButtonStateControl;
use pr_platform_win::dialogs::native::dialog::{ID_CANCEL, ID_OK};
use pr_platform_win::win32::structs::{Hresult, Hwnd};
use pr_platform_win::win32::{dialog_native, messages};
use pr_test_support::TempRoot;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct FakeFiles {
    open_result: Option<String>,
    save_result: Option<String>,
    calls: RefCell<Vec<String>>,
    filters: RefCell<Vec<String>>,
}

impl ShellFileDialog for FakeFiles {
    fn pick_open_file(
        &self,
        _owner: Hwnd,
        title: &str,
        filters: &[FileDialogFilter],
        _initial_folder: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        self.calls.borrow_mut().push(format!("open:{title}"));
        self.filters
            .borrow_mut()
            .extend(filters.iter().map(|f| format!("{}|{}", f.name, f.pattern)));
        Ok(self.open_result.clone())
    }

    fn pick_save_file(
        &self,
        _owner: Hwnd,
        title: &str,
        filters: &[FileDialogFilter],
        default_extension: Option<&str>,
        suggested_file_name: Option<&str>,
        _initial_folder: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        self.calls.borrow_mut().push(format!(
            "save:{title}:{}:{}",
            default_extension.unwrap_or(""),
            suggested_file_name.unwrap_or("")
        ));
        self.filters
            .borrow_mut()
            .extend(filters.iter().map(|f| format!("{}|{}", f.name, f.pattern)));
        Ok(self.save_result.clone())
    }
}

#[derive(Default)]
struct RecordingWarnings(RefCell<Vec<(String, String)>>);
impl DialogWarningSink for RecordingWarnings {
    fn show_warning(&self, _owner: Hwnd, title: &str, message: &str) {
        self.0.borrow_mut().push((title.to_owned(), message.to_owned()));
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

fn list_names(d: &pr_platform_win::dialogs::native::dialog::NativeDialog) -> Vec<String> {
    let list = item(d, LIST_ID);
    (0..ListBoxControl::count(list))
        .map(|i| ListBoxControl::text_at(list, i))
        .collect()
}

// C#: ActionProfilesDialogTests.Enter_InTheNameBox_AppliesThroughTheDialogManager
#[test]
#[ignore = "native: needs desktop"]
fn enter_in_the_name_box_applies_through_the_dialog_manager() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let dialog = ActionProfilesDialog::new(&sample(), None, None, None);
        let result = show_on_owner(dialog.native(), |d, _| {
            let name = item(d, NAME_ID);
            assert!(key(d, name, VK_RETURN));
            drain(d);
        })
        .expect("show action profiles dialog");
        assert_eq!(result, ID_OK);
    });
}

// C#: ActionProfilesDialogTests.EveryCancelPath_ReturnsCancel_WithoutWarnings (button, escape, close, syscommand)
#[test]
#[ignore = "native: needs desktop"]
fn every_cancel_path_returns_cancel_without_warnings() {
    for how in ["button", "escape", "close", "syscommand"] {
        on_sta(move || {
            let _styles = activate_visual_styles();
            let warnings = Rc::new(RecordingWarnings::default());
            let dialog = ActionProfilesDialog::new(&sample(), None, Some(warnings.clone()), None);
            let result = show_on_owner(dialog.native(), |d, _| {
                set_text(d, NAME_ID, "changed but cancelled");
                match how {
                    "button" => {
                        command(d, ID_CANCEL, 0);
                    }
                    "escape" => {
                        key(d, item(d, NAME_ID), VK_ESCAPE);
                        drain(d);
                    }
                    "close" => {
                        // SAFETY: WM_CLOSE has no pointer arguments and the dialog is live.
                        unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_CLOSE, 0, 0) };
                    }
                    _ => {
                        // SAFETY: WM_SYSCOMMAND/SC_CLOSE has no pointer arguments and the dialog is live.
                        unsafe { dialog_native::user32::send_message(d.hwnd(), messages::WM_SYSCOMMAND, SC_CLOSE, 0) };
                    }
                }
            })
            .expect("show action profiles dialog");
            assert_eq!(result, ID_CANCEL, "{how}");
            assert!(warnings.0.borrow().is_empty(), "{how}");
        });
    }
}

// C#: ActionProfilesDialogTests.Import_ReplacesTheListAndOpensTheFirstRow
#[test]
#[ignore = "native: needs desktop"]
fn import_replaces_the_list_and_opens_the_first_row() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let root = TempRoot::new("ap-import");
        let path = root.join("in.json");
        let mut imported = ReviewAction::new("Imported A");
        imported.shortcut = "F10".to_owned();
        imported.operation = FileOperationType::Copy;
        imported.destination = "A".to_owned();
        imported.confirm = true;
        std::fs::write(&path, serialize_for_export(&[imported])).unwrap();
        let files = Rc::new(FakeFiles {
            open_result: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        });
        let dialog = ActionProfilesDialog::new(&sample(), Some(files.clone()), None, None);
        show_on_owner(dialog.native(), |d, _| {
            command(d, IMPORT_ID, 0);
            assert_eq!(list_names(d), ["Imported A"]);
            assert_eq!(ListBoxControl::selected_index(item(d, LIST_ID)), 0);
            assert_eq!(text_of(item(d, SHORTCUT_ID)), "F10");
            assert_eq!(ComboBoxControl::selected_index(item(d, OPERATION_ID)), 1);
            assert!(ButtonStateControl::is_checked(item(d, CONFIRM_ID)));
            d.close();
        })
        .expect("show action profiles dialog");
        assert_eq!(
            *files.calls.borrow(),
            [format!("open:{}", pr_l10n::tr::action_profiles_import())]
        );
        assert!(files.filters.borrow().iter().any(|f| f.ends_with("*.json")));
    });
}

// C#: ActionProfilesDialogTests.Import_OfAnInvalidFile_WarnsAndKeepsTheList
#[test]
#[ignore = "native: needs desktop"]
fn import_of_an_invalid_file_warns_and_keeps_the_list() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let root = TempRoot::new("ap-import-bad");
        let path = root.join("bad.json");
        std::fs::write(&path, "{ not json").unwrap();
        let files = Rc::new(FakeFiles {
            open_result: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        });
        let warnings = Rc::new(RecordingWarnings::default());
        let dialog = ActionProfilesDialog::new(&sample(), Some(files), Some(warnings.clone()), None);
        show_on_owner(dialog.native(), |d, _| {
            command(d, IMPORT_ID, 0);
            assert_eq!(list_names(d), ["Keep", "Copy out", "Trash"]);
            d.close();
        })
        .expect("show action profiles dialog");
        let shown = warnings.0.borrow();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].0, pr_l10n::tr::dialog_import_actions_failed_title());
        assert_eq!(
            shown[0].1,
            pr_l10n::localizer::Localizer::current().get("dialog.importActions.invalid.message")
        );
    });
}

// C#: ActionProfilesDialogTests.Import_Cancelled_ChangesNothing
#[test]
#[ignore = "native: needs desktop"]
fn import_cancelled_changes_nothing() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let files = Rc::new(FakeFiles::default());
        let warnings = Rc::new(RecordingWarnings::default());
        let dialog = ActionProfilesDialog::new(&sample(), Some(files.clone()), Some(warnings.clone()), None);
        show_on_owner(dialog.native(), |d, _| {
            command(d, IMPORT_ID, 0);
            assert_eq!(list_names(d), ["Keep", "Copy out", "Trash"]);
            d.close();
        })
        .expect("show action profiles dialog");
        assert!(warnings.0.borrow().is_empty());
        assert_eq!(files.calls.borrow().len(), 1);
    });
}

// C#: ActionProfilesDialogTests.Export_WritesTheCurrentEdits_ToTheChosenFile
#[test]
#[ignore = "native: needs desktop"]
fn export_writes_the_current_edits_to_the_chosen_file() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let root = TempRoot::new("ap-export");
        let path = root.join("out.json");
        let files = Rc::new(FakeFiles {
            save_result: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        });
        let dialog = ActionProfilesDialog::new(&sample(), Some(files.clone()), None, None);
        show_on_owner(dialog.native(), |d, _| {
            set_text(d, NAME_ID, "Exported name"); // not yet saved to the row: Export saves it first
            command(d, EXPORT_ID, 0);
            d.close();
        })
        .expect("show action profiles dialog");
        assert!(std::fs::read_to_string(&path).unwrap().contains("Exported name"));
        assert_eq!(
            *files.calls.borrow(),
            [format!(
                "save:{}:json:photoreview-actions.json",
                pr_l10n::tr::action_profiles_export()
            )]
        );
    });
}

// C#: ActionProfilesDialogTests.Export_ToAnUnwritableTarget_WarnsWithTheReason
#[test]
#[ignore = "native: needs desktop"]
fn export_to_an_unwritable_target_warns_with_the_reason() {
    on_sta(|| {
        let _styles = activate_visual_styles();
        let root = TempRoot::new("ap-export-bad");
        // A directory: writing a file onto it fails.
        let files = Rc::new(FakeFiles {
            save_result: Some(root.path().to_string_lossy().into_owned()),
            ..Default::default()
        });
        let warnings = Rc::new(RecordingWarnings::default());
        let dialog = ActionProfilesDialog::new(&sample(), Some(files), Some(warnings.clone()), None);
        show_on_owner(dialog.native(), |d, _| {
            command(d, EXPORT_ID, 0);
            d.close();
        })
        .expect("show action profiles dialog");
        let shown = warnings.0.borrow();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].0, pr_l10n::tr::dialog_export_actions_failed_title());
        assert!(!shown[0].1.trim().is_empty());
    });
}
