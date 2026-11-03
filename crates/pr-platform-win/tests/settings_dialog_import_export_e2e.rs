//! Q5 (D-10): Settings Export and Import inside the REAL modal loop (real window, real command routing), with the Shell file picker faked.
//! C# `SettingsDialog.ExportSettings` / `ImportSettings` / `ImportSettingsFrom` (lines 856 to 905). The real `IFileDialog` itself needs a person (see the Q5 report).
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
use pr_platform_win::dialogs::file_dialog::{FileDialogFilter, ShellFileDialog};
use pr_platform_win::dialogs::settings::dialog::{
    SettingsDialog,
    frame::{EXPORT_ID, IMPORT_ID},
};
use pr_platform_win::win32::structs::Hresult;
use std::{cell::RefCell, rc::Rc};

/// A picker that answers every Open/Save with the current `path` (`None` = the user cancelled).
struct Picker(RefCell<Option<String>>);
impl ShellFileDialog for Picker {
    fn pick_open_file(
        &self,
        _: isize,
        _: &str,
        _: &[FileDialogFilter],
        _: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        Ok(self.0.borrow().clone())
    }
    fn pick_save_file(
        &self,
        _: isize,
        _: &str,
        _: &[FileDialogFilter],
        _: Option<&str>,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        Ok(self.0.borrow().clone())
    }
}

// C#: SettingsDialogTests export then import round trip (no direct C# test drives the modal loop; this is the Rust-side desktop proof of the two commands).
#[test]
#[ignore = "native: needs desktop"]
fn export_then_import_round_trip_and_a_repaired_import_notice_inside_the_modal_loop() {
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let root = pr_test_support::TempRoot::new("settings-import-export-modal");
        let file = root.join("PhotoReview-settings.json");
        let picker = Rc::new(Picker(RefCell::new(Some(file.to_str().unwrap().to_owned()))));
        let warnings = Rc::new(fixture::Warnings::default());
        let mut args = fixture::args();
        args.current.logging_enabled = true;
        args.options.warnings = Some(warnings.clone());
        args.options.files = Some(picker.clone());
        let dialog = SettingsDialog::new(args).unwrap();
        let ready = |d: &SettingsDialog| {
            d.ensure_all_pages_created(); // so that Import must REFRESH existing controls (a lazily created page would read the new state anyway)
            d.activate_page(4, false, true); // the shortcuts page is the one on screen when Import runs
            // Export writes the dialog's current values.
            desktop::command(d.native(), EXPORT_ID, 0);
            let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(written["LoggingEnabled"], true);
            assert!(warnings.0.borrow().is_empty(), "{:?}", warnings.0.borrow());
            // A cancelled Export writes nothing new.
            *picker.0.borrow_mut() = None;
            desktop::command(d.native(), EXPORT_ID, 0);
            // Import of a valid file refreshes the values and shows nothing.
            let mut changed = written.clone();
            changed["LoggingEnabled"] = serde_json::json!(false);
            changed["Shortcuts"]["Next"] = serde_json::json!("F9");
            std::fs::write(&file, serde_json::to_string(&changed).unwrap()).unwrap();
            *picker.0.borrow_mut() = Some(file.to_str().unwrap().to_owned());
            desktop::command(d.native(), IMPORT_ID, 0);
            assert!(!d.settings().logging_enabled);
            // The visible page controls were reloaded from the imported values (not only the presenter state).
            assert_eq!(d.control_text(5006), "F9");
            assert!(warnings.0.borrow().is_empty(), "{:?}", warnings.0.borrow());
            // Import of a file with an invalid value: repaired, and ONE notice titled "Import settings".
            std::fs::write(&file, r#"{"ImageCacheRamPercent":999,"LoggingEnabled":true}"#).unwrap();
            desktop::command(d.native(), IMPORT_ID, 0);
            assert!(d.settings().logging_enabled);
            let shown = warnings.0.borrow();
            assert_eq!(shown.len(), 1, "{shown:?}");
            assert_eq!(shown[0].0, pr_l10n::tr::dialog_import_settings_title());
            assert!(!shown[0].1.is_empty());
            drop(shown);
            // A file that is not JSON is refused with the invalid-settings title and leaves the values alone.
            std::fs::write(&file, "not json").unwrap();
            desktop::command(d.native(), IMPORT_ID, 0);
            assert!(d.settings().logging_enabled);
            assert_eq!(warnings.0.borrow().len(), 2);
            assert_eq!(warnings.0.borrow()[1].0, pr_l10n::tr::dialog_settings_invalid_title());
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
        assert!(!dialog.saved(), "Import and Export never save by themselves");
    });
}
