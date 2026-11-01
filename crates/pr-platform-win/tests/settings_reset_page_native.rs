//! "Reset this page" on a real Settings window: only the showing page returns to the built-in defaults (C# `ResetActivePage`, SettingsDialogTests.ResetPage_*).
#[path = "common/dialogs_dialog_fixture.rs"]
mod desktop;
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
use pr_app::presenters::settings::SettingsPresenter;
use pr_core::settings::app_settings::AppSettings;

fn lines(state: &impl std::fmt::Debug) -> Vec<String> {
    format!("{state:#?}").lines().map(str::to_owned).collect()
}

fn non_default() -> AppSettings {
    let mut s = fixture::args().current;
    s.logging_enabled = true;
    s.confirm_before_delete = true;
    s.show_zoom_indicator = true;
    s.initial_view_mode = pr_core::model::enums::InitialViewMode::Percent100;
    s.show_folder_info = true;
    s.show_info_overlay = false;
    s.mouse_wheel_action = pr_core::model::enums::MouseWheelAction::Navigate;
    s.click_to_zoom_enabled = true;
    s.preload_worker_count = 7;
    s.preload_forward_count = 9;
    s.image_cache_ram_percent = 33;
    s.use_source_bytes_cache = !s.use_source_bytes_cache;
    s.compare_hash_enabled = false;
    s.keep_zoom_across_images = true;
    s.external_editor_sends_raw_original = false;
    s.shortcuts.next = "Q".to_owned();
    s.shortcuts.zoom_in = "W".to_owned();
    s.external_editor_path = r"C:/ed.exe".to_owned();
    s.external_editors = vec![pr_core::settings::external_editors::ExternalEditor {
        id: "0123456789abcdef0123456789abcdef".to_owned(),
        name: "Ed".to_owned(),
        path: r"C:/ed.exe".to_owned(),
        arguments: "{file}".to_owned(),
    }];
    s.default_external_editor_id = "0123456789abcdef0123456789abcdef".to_owned();
    s.show_exif_info = !s.show_exif_info;
    s.toolbar_auto_hide = !s.toolbar_auto_hide;
    s.toolbar_opacity_percent = 40;
    s.info_overlay_font_size = 20.0;
    s.image_transition_ms = 500;
    s.touchpad_swipe_enabled = !s.touchpad_swipe_enabled;
    s.kinetic_pan_enabled = !s.kinetic_pan_enabled;
    s.arrow_pan_step_percent = 33;
    s.keyboard_zoom_step_percent = 44;
    s.raw_support_enabled = !s.raw_support_enabled;

    s.compare_size_enabled = false;
    s.allow_permanent_delete_without_recycle_bin = true;
    s.memory_reserve_bytes = 123 << 20;
    s.preload_backward_count = 3;
    s.show_folder_menu_items = !s.show_folder_menu_items;
    s.move_copy_reuse_last_folder = true;
    s
}

#[test]
#[ignore = "native: needs desktop"]
fn reset_page_diff_per_page() {
    // Every setting that is not at its default must be restored by the reset of SOME page, and no page may write a value that is neither the old one nor the default.
    static UNRESET: std::sync::Mutex<Vec<std::collections::HashSet<String>>> = std::sync::Mutex::new(Vec::new());
    for page in 0..7usize {
        desktop::on_sta(move || {
            let _styles = desktop::activate_visual_styles();
            let mut args = fixture::args();
            args.current = non_default();
            let options = fixture::args().presenter;
            let defaults = SettingsPresenter::new(&args.current, options).apply_defaults();
            let dialog = pr_platform_win::dialogs::settings::dialog::SettingsDialog::new(args).unwrap();
            let ready = |d: &pr_platform_win::dialogs::settings::dialog::SettingsDialog| {
                d.ensure_all_pages_created();
                d.activate_page(page as i32, false, true);
                let before = lines(&d.collect_all());
                d.reset_active_page();
                let after = lines(&d.collect_all());
                let def = lines(&defaults);
                let mut report = String::new();
                let mut unreset = std::collections::HashSet::new();
                for a in &after {
                    if !before.contains(a) && !def.contains(a) {
                        report += &format!(
                            "WRONG-VALUE {a}
"
                        );
                    }
                    if before.contains(a) && !def.contains(a) {
                        report += &format!(
                            "UNRESET {a}
"
                        );
                        unreset.insert(a.clone());
                    }
                }
                assert!(!report.contains("WRONG-VALUE"), "{report}");
                UNRESET.lock().unwrap().push(unreset);
                println!(
                    "=== page {page} ({}) ===\n{report}",
                    d.page_key(page).unwrap_or_default()
                );
                d.close();
            };
            dialog.show_modal(0, Some(&ready), None).unwrap();
        });
    }
    let all = UNRESET.lock().unwrap();
    assert_eq!(all.len(), 7);
    let never: Vec<_> = all[0].iter().filter(|l| all.iter().all(|s| s.contains(*l))).collect();
    assert!(never.is_empty(), "restored by no page: {never:?}");
}

// C#: SettingsDialogTests.ResetPage_ResetsOnlyTheShowingPage, but through a REAL click on the "Reset this page" button.
#[test]
#[ignore = "native: needs desktop"]
fn clicking_the_reset_button_resets_only_the_showing_page() {
    use pr_platform_win::dialogs::settings::{
        builder::inputs::SettingsPageBuilder,
        dialog::frame::{RESET_PAGE_ID, page_id_base},
        page_view::SettingsPageView,
        pages::{diagnostics::DiagnosticsSettingsPage, general::GeneralSettingsPage},
    };
    desktop::on_sta(|| {
        let _styles = desktop::activate_visual_styles();
        let mut args = fixture::args();
        args.current.logging_enabled = true;
        args.current.initial_view_mode = pr_core::model::enums::InitialViewMode::Percent100;
        let dialog = pr_platform_win::dialogs::settings::dialog::SettingsDialog::new(args).unwrap();
        let ready = |d: &pr_platform_win::dialogs::settings::dialog::SettingsDialog| {
            let mut diag = DiagnosticsSettingsPage::new();
            diag.declare(&mut SettingsPageBuilder::new(page_id_base(6)));
            let mut general = GeneralSettingsPage::new();
            general.declare(&mut SettingsPageBuilder::new(page_id_base(0)));
            let (logging, view_mode) = (diag.ids().logging, general.ids().view_mode);
            d.ensure_all_pages_created();
            assert!(d.is_control_checked(logging));
            assert_eq!(d.combo_selected_index(view_mode), 4);
            let button = d.native().item(RESET_PAGE_ID);
            assert_ne!(button, 0, "no Reset button");
            assert!(d.is_control_enabled(RESET_PAGE_ID), "Reset button disabled");
            // SAFETY: BM_CLICK carries no pointer arguments and the button window is live.
            unsafe { pr_platform_win::win32::dialog_native::user32::send_message(button, 0x00F5, 0, 0) };
            assert_eq!(d.combo_selected_index(view_mode), 0, "General page not reset");
            assert!(d.is_control_checked(logging), "another page was reset");
            d.activate_page(6, false, true);
            // SAFETY: as above.
            unsafe { pr_platform_win::win32::dialog_native::user32::send_message(button, 0x00F5, 0, 0) };
            assert!(!d.is_control_checked(logging), "Diagnostics page not reset");
            assert!(d.settings().logging_enabled, "the working copy changes only on Save");
            d.close();
        };
        dialog.show_modal(0, Some(&ready), None).unwrap();
    });
}
