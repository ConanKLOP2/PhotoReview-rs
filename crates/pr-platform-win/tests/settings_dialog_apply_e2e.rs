//! Settings Save/Apply on a real window with config isolated under TempRoot (W8-19l).
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
use pr_platform_win::dialogs::settings::dialog::{SettingsDialog, frame::APPLY_ID};

#[test]
#[ignore = "native: needs desktop"]
fn worker_post_runs_on_the_ui_thread_and_posts_after_close_are_dropped() {
    desktop::on_sta(|| {
        use pr_platform_win::dialogs::settings::{
            context::SettingsPageHost,
            dialog::{frame::WM_APP_QUEUE, host::PageHost},
        };
        let _styles = desktop::activate_visual_styles();
        let d = fixture::dialog();
        let host = PageHost::new(std::rc::Rc::downgrade(d.content()));
        let retained = std::cell::RefCell::new(None);
        let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let ui = std::thread::current().id();
        let ready = |d: &SettingsDialog| {
            let poster = host.ui_post();
            let seen = observed.clone();
            let worker_poster = poster.clone();
            std::thread::spawn(move || {
                worker_poster.post(Box::new(move |_| {
                    *seen.lock().unwrap() = Some(std::thread::current().id());
                }))
            })
            .join()
            .unwrap();
            assert!(observed.lock().unwrap().is_none());
            // SAFETY: integer-only queue wake to this test's own dialog; queued work is owned by the WindowPoster.
            unsafe {
                pr_platform_win::win32::dialog_native::user32::send_message(d.hwnd(), WM_APP_QUEUE, 0, 0);
            }
            assert_eq!(*observed.lock().unwrap(), Some(ui));
            *retained.borrow_mut() = Some(poster);
            d.close();
        };
        assert_eq!(d.show_modal(0, Some(&ready), None).unwrap(), 2);
        // Close requests end the modal loop; cleanup happens when show_modal returns.
        assert_eq!(host.hwnd(), 0);
        assert_eq!(host.control_text(1), "");
        assert_eq!(host.combo_selected_index(1), -1);
        retained
            .borrow()
            .as_ref()
            .unwrap()
            .post(Box::new(|_| panic!("post after close executed")));
    });
}

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
