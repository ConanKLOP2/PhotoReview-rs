//! Settings Save/Apply on a real window with config isolated under TempRoot (W8-19l).
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
use pr_platform_win::dialogs::settings::dialog::{SettingsDialog, frame::APPLY_ID};

// C#: Apply_SavesAndStaysOpen_AndASubsequentCancelStillReportsSaved;
// AMandatoryBoxEmptied_IsRefusedAtSave_WithTheInvalidShortcutMessage.
#[test]
#[ignore = "native: needs desktop"]
fn invalid_shortcut_writes_nothing_then_apply_persists_and_cancel_keeps_saved_state() {
    desktop::on_sta(|| {
        use pr_core::{
            abstractions::{NullLog, SystemClock},
            app_paths::AppPaths,
            io::physical::PhysicalFileSystem,
            settings::{app_settings::EnglishSettingsTexts, store::SettingsStore},
        };
        use std::{rc::Rc, sync::Arc};
        let _styles = desktop::activate_visual_styles();
        let root = pr_test_support::TempRoot::new("settings-native-save");
        let paths = AppPaths::new(root.path(), None, false);
        let store = Arc::new(SettingsStore::new(
            paths.clone(),
            Arc::new(PhysicalFileSystem),
            Arc::new(NullLog),
            Arc::new(SystemClock),
            Arc::new(EnglishSettingsTexts),
        ));
        let warnings = Rc::new(fixture::Warnings::default());
        let mut args = fixture::args();
        args.current.logging_enabled = true;
        args.presenter.store = Some(store);
        args.options.warnings = Some(warnings.clone());
        let dialog = SettingsDialog::new(args).unwrap();
        let ready = |d: &SettingsDialog| {
            d.activate_page(4, false, true);
            d.set_control_text(5006, "");
            assert!(!d.try_save_all());
            assert!(!d.saved());
            assert!(!paths.config_file.exists());
            assert_eq!(warnings.0.borrow().len(), 1);
            assert_eq!(warnings.0.borrow()[0].0, pr_l10n::tr::dialog_settings_invalid_title());
            assert!(!warnings.0.borrow()[0].1.is_empty());
            assert!(!d.is_closed());
            d.set_control_text(5006, "Right");
            desktop::command(d.native(), APPLY_ID, 0);
            assert!(d.saved());
            assert!(!d.is_closed());
            let saved: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&paths.config_file).unwrap()).unwrap();
            assert_eq!(saved["LoggingEnabled"], true); // Unvisited General page survives Save.
            assert_eq!(saved["Shortcuts"]["Next"], "Right");
            d.close();
        };
        assert_eq!(dialog.show_modal(0, Some(&ready), None).unwrap(), 2);
        assert!(dialog.saved());
        assert!(paths.config_file.is_file());
    });
}
