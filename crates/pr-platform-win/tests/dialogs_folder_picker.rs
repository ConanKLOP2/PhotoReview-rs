#[rustfmt::skip]
mod red_tests {

use std::cell::RefCell;
use std::rc::Rc;

use pr_platform_win::dialogs::file_dialog::{OPEN_OPTIONS, SAVE_OPTIONS};
use pr_platform_win::dialogs::folder_dialog::{
    FolderPicker, ShellFolderDialog, UnwiredShellFolderDialog, FOLDER_OPTIONS,
    HRESULT_CANCELLED,
};
use pr_platform_win::win32::structs::{Hresult, Hwnd};

struct RecordingDialog {
    calls: RefCell<Vec<(Hwnd, String, Option<String>)>>,
}

impl ShellFolderDialog for RecordingDialog {
    fn show(
        &self,
        owner: Hwnd,
        title: &str,
        initial_folder: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        self.calls.borrow_mut().push((owner, title.to_owned(), initial_folder.map(str::to_owned)));
        Ok(Some("picked".to_owned()))
    }
}

// C#: FolderPicker.UsableInitialFolder
#[test]
fn usable_initial_folder_matches_the_golden_for_every_case() {
    let root = pr_test_support::TempRoot::new("dialogs-folder-picker");
    let photos = root.path().join("photos");
    std::fs::create_dir_all(photos.join("sub")).unwrap();
    std::fs::write(photos.join("a.jpg"), []).unwrap();
    let mut input: serde_json::Value = serde_json::from_str(include_str!("../../../tests/golden-inputs/folder-picker-policy.json")).unwrap();
    let mut golden: serde_json::Value = serde_json::from_str(include_str!("../../../tests/golden/folder-picker-policy.json")).unwrap();
    replace_root(&mut input, &root.path().to_string_lossy());
    replace_root(&mut golden, &root.path().to_string_lossy());
    let cases = input["usable"].as_array().unwrap();
    let expected_cases: Vec<_> = golden["cases"].as_array().unwrap().iter()
        .filter(|case| case["kind"] == "usable")
        .collect();
    assert_eq!(cases.len(), expected_cases.len());
    for (case, expected) in cases.iter().zip(expected_cases) {
        let id = case["id"].as_str().unwrap();
        let value = case["folder"].as_str().map(str::to_owned);
        let actual = FolderPicker::usable_initial_folder(value.as_deref());
        let expected_value = match expected["result"].as_str() {
            Some("same") => expected["folder"].as_str().map(str::to_owned),
            _ => None,
        };
        assert_eq!(actual, expected_value, "case {id}");
    }
}

fn replace_root(value: &mut serde_json::Value, root: &str) {
    match value {
        serde_json::Value::String(text) => *text = text.replace("$ROOT", root),
        serde_json::Value::Array(values) => values.iter_mut().for_each(|value| replace_root(value, root)),
        serde_json::Value::Object(values) => values.values_mut().for_each(|value| replace_root(value, root)),
        _ => {}
    }
}

// C#: FolderPicker.Pick
#[test]
fn pick_forwards_owner_title_and_the_usable_initial_folder() {
    let dialog = Rc::new(RecordingDialog { calls: RefCell::new(Vec::new()) });
    let owner = Hwnd::default();
    let picker = FolderPicker::new(dialog.clone(), Box::new(move || owner));
    let root = pr_test_support::TempRoot::new("dialogs-folder-pick");
    picker.pick("Choose", Some(root.path().to_str().unwrap())).unwrap();
    assert_eq!(dialog.calls.borrow()[0], (owner, "Choose".to_owned(), Some(root.path().to_str().unwrap().to_owned())));
}

// C#: FolderPicker.Pick
#[test]
fn pick_calls_the_owner_provider_on_every_call() {
    let dialog = Rc::new(RecordingDialog { calls: RefCell::new(Vec::new()) });
    let owners = Rc::new(RefCell::new(0));
    let owner_count = owners.clone();
    let picker = FolderPicker::new(dialog.clone(), Box::new(move || { *owner_count.borrow_mut() += 1; Hwnd::default() }));
    picker.pick("one", None).unwrap();
    picker.pick("two", None).unwrap();
    assert_eq!(*owners.borrow(), 2);
    assert_eq!(dialog.calls.borrow()[1].0, Hwnd::default());
}

// C#: UnwiredShellFolderDialog.Show
#[test]
fn unwired_dialog_answers_cancelled() {
    assert_eq!(UnwiredShellFolderDialog.show(Hwnd::default(), "", None).unwrap(), None);
}

// C#: NativeShellFolderDialog.FolderOptions
#[test]
fn folder_options_equal_the_golden_file_dialog_options() {
    assert_eq!(FOLDER_OPTIONS.0, 2144);
}

// C#: NativeShellFileDialog.OpenOptions and SaveOptions
#[test]
fn open_and_save_options_equal_the_golden_values() {
    assert_eq!(OPEN_OPTIONS.0, 6208);
    assert_eq!(SAVE_OPTIONS.0, 34882);
}

// C#: NativeShellFolderDialog.HResultCancelled
#[test]
fn hresult_cancelled_is_0x800704c7() {
    assert_eq!(HRESULT_CANCELLED, 0x800704c7u32 as i32);
}
}
