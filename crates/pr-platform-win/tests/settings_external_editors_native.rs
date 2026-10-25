//! W9-E4 Settings UI twins of ExternalEditorsUiTests at C# 7eaee2a2.
#[path = "common/w9_e2_capture.rs"]
mod capture;
#[path = "common/dialogs_dialog_fixture.rs"]
mod native;
#[path = "common/settings_dialog_fixture.rs"]
mod settings_fixture;
use pr_core::settings::{
    app_settings::{AppSettings, EnglishSettingsTexts},
    external_editors::ExternalEditor,
};
use pr_l10n::{localizer::Localizer, tr};
use pr_platform_win::dialogs::{
    native::{controls::custom::ButtonStateControl, external_editors_dialog::ExternalEditorsEdit},
    settings::{
        builder::inputs::SettingsPageBuilder,
        dialog::{SettingsDialog, frame::page_id_base},
        page_view::SettingsPageView,
        pages::files::{FilesEditorIds, FilesIds, FilesSettingsPage},
    },
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

fn control(dialog: &pr_platform_win::dialogs::native::dialog::NativeDialog, id: i32) -> isize {
    let control = dialog.item(id);
    assert_ne!(control, 0, "missing nested Settings control {id}");
    control
}

fn sample() -> AppSettings {
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    settings.external_editors = [
        ("lr", "Lightroom", r"C:\LR\lightroom.exe"),
        ("tp", "Topaz AI", r"C:\Topaz\topaz.exe"),
        ("dxo", "DxO Lab", r"C:\DxO\dxo.exe"),
    ]
    .into_iter()
    .map(|(id, name, path)| ExternalEditor {
        id: id.to_owned(),
        name: name.to_owned(),
        path: path.to_owned(),
        arguments: ExternalEditor::DEFAULT_ARGUMENTS.to_owned(),
    })
    .collect();
    settings.default_external_editor_id = "tp".to_owned();
    settings.external_editor_path = r"C:\Topaz\topaz.exe".to_owned();
    settings
}
fn ids() -> (FilesIds, FilesEditorIds) {
    let mut page = FilesSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(page_id_base(5));
    page.declare(&mut builder);
    (page.ids(), page.editor_ids())
}

// C#: FilesPage_ManageEditors_Apply_UpdatesThePathBoxTheSummaryAndTheSavedSettings.
#[test]
#[ignore = "native: needs desktop"]
fn files_page_manage_editors_apply_updates_path_summary_and_saved_settings() {
    native::on_sta(|| {
        let (ids, editors) = ids();
        let shown = Rc::new(RefCell::new(Vec::new()));
        let record = shown.clone();
        let mut args = settings_fixture::args();
        args.current = sample();
        args.options.show_external_editors = Some(Rc::new(move |owner, list, default| {
            assert_ne!(owner, 0);
            record.borrow_mut().push((list.to_vec(), default.map(str::to_owned)));
            let mut list = list.to_vec();
            list.push(ExternalEditor {
                id: "new".to_owned(),
                name: "Capture One".to_owned(),
                path: r"C:\CO\co.exe".to_owned(),
                arguments: ExternalEditor::DEFAULT_ARGUMENTS.to_owned(),
            });
            Ok(Some(ExternalEditorsEdit {
                editors: list,
                default_id: "new".to_owned(),
            }))
        }));
        let dialog = SettingsDialog::new(args).unwrap();
        native::show_on_owner(dialog.native(), |d, _| {
            dialog.activate_page(5, false, true);
            assert_eq!(native::text_of(control(d, ids.editor_path)), r"C:\Topaz\topaz.exe");
            let before = native::text_of(control(d, editors.summary));
            assert!(before.contains('3') && before.contains("Topaz AI"));
            native::command(d, editors.manage, 0);
            assert_eq!(native::text_of(control(d, ids.editor_path)), r"C:\CO\co.exe");
            assert!(native::text_of(control(d, editors.summary)).contains("Capture One"));
            assert!(dialog.try_save_all());
            d.close();
        })
        .unwrap();
        assert_eq!(shown.borrow().len(), 1);
        assert_eq!(shown.borrow()[0].0.len(), 3);
        assert_eq!(shown.borrow()[0].1.as_deref(), Some("tp"));
        let settings = dialog.settings();
        assert_eq!(
            settings
                .external_editors
                .iter()
                .map(|editor| editor.id.as_str())
                .collect::<Vec<_>>(),
            ["lr", "tp", "dxo", "new"]
        );
        assert_eq!(settings.default_external_editor_id, "new");
        assert_eq!(settings.external_editor_path, r"C:\CO\co.exe");
    });
}

// C#: FilesPage_ManageEditors_Cancel_ChangesNothing_AndTheRawSwitchRoundTrips.
#[test]
#[ignore = "native: needs desktop"]
fn files_page_manage_editors_cancel_changes_nothing_and_raw_switch_round_trips() {
    native::on_sta(|| {
        let (ids, editors) = ids();
        let mut args = settings_fixture::args();
        args.current = sample();
        args.options.show_external_editors = Some(Rc::new(|_, _, _| Ok(None)));
        let dialog = SettingsDialog::new(args).unwrap();
        native::show_on_owner(dialog.native(), |d, _| {
            dialog.activate_page(5, false, true);
            assert!(ButtonStateControl::is_checked(control(d, editors.send_raw)));
            native::command(d, editors.manage, 0);
            assert_eq!(native::text_of(control(d, ids.editor_path)), r"C:\Topaz\topaz.exe");
            ButtonStateControl::set_checked(control(d, editors.send_raw), false);
            assert!(dialog.try_save_all());
            d.close();
        })
        .unwrap();
        let settings = dialog.settings();
        assert_eq!(settings.external_editors, sample().external_editors);
        assert_eq!(settings.default_external_editor_id, "tp");
        assert!(!settings.external_editor_sends_raw_original);
    });
}

// C#: FilesPage_NoEditorsDeclared_SummarySaysSo_AndTheDefaultPathBoxStillCreatesTheFirstEditor.
#[test]
#[ignore = "native: needs desktop"]
fn files_page_no_editors_summary_says_so_and_path_box_creates_first_editor() {
    native::on_sta(|| {
        let (ids, editors) = ids();
        let dialog = SettingsDialog::new(settings_fixture::args()).unwrap();
        native::show_on_owner(dialog.native(), |d, _| {
            dialog.activate_page(5, false, true);
            assert_eq!(
                native::text_of(control(d, editors.summary)),
                tr::settings_external_editors_summary_none()
            );
            dialog.set_control_text(ids.editor_path, r"C:\Tools\Affinity\Photo.exe");
            assert!(dialog.try_save_all());
            d.close();
        })
        .unwrap();
        let settings = dialog.settings();
        assert_eq!(settings.external_editors.len(), 1);
        assert_eq!(settings.external_editors[0].name, "Photo");
        assert_eq!(settings.external_editors[0].id, settings.default_external_editor_id);
    });
}

// Source-derived: absent nested editor callback reports the specified warning and preserves the list.
#[test]
#[ignore = "native: needs desktop"]
fn files_page_unavailable_editor_reports_warning_and_keeps_list() {
    native::on_sta(|| {
        let (_, editors) = ids();
        let warnings = Rc::new(settings_fixture::Warnings::default());
        let mut args = settings_fixture::args();
        args.current = sample();
        args.options.warnings = Some(warnings.clone());
        let dialog = SettingsDialog::new(args).unwrap();
        native::show_on_owner(dialog.native(), |d, _| {
            dialog.activate_page(5, false, true);
            native::command(d, editors.manage, 0);
            assert_eq!(warnings.0.borrow().len(), 1);
            assert_eq!(
                warnings.0.borrow()[0].0,
                Localizer::current().get("dialog.openEditorFailed.title")
            );
            assert_eq!(dialog.collect_all().external_editors, sample().external_editors);
            d.close();
        })
        .unwrap();
    });
}

// Source-derived: typed path is folded before opening the modal seam; it holds no presenter borrow.
#[test]
#[ignore = "native: needs desktop"]
fn files_page_manager_receives_typed_path_without_holding_presenter_borrow() {
    native::on_sta(|| {
        let (ids, editors) = ids();
        let shared = Rc::new(RefCell::new(
            None::<pr_platform_win::dialogs::settings::context::SharedSettingsPresenter>,
        ));
        let probe = shared.clone();
        let calls = Rc::new(Cell::new(0));
        let called = calls.clone();
        let mut args = settings_fixture::args();
        args.current = sample();
        args.options.show_external_editors = Some(Rc::new(move |_, list, default| {
            called.set(called.get() + 1);
            assert_eq!(default, Some("tp"));
            assert_eq!(list[1].path, r"C:\New\typed.exe");
            let presenter = probe.borrow().as_ref().unwrap().clone();
            assert!(
                presenter.try_borrow_mut().is_ok(),
                "presenter borrowed across nested editor callback"
            );
            Ok(None)
        }));
        let dialog = SettingsDialog::new(args).unwrap();
        *shared.borrow_mut() = Some(dialog.presenter());
        native::show_on_owner(dialog.native(), |d, _| {
            dialog.activate_page(5, false, true);
            dialog.set_control_text(ids.editor_path, r"C:\New\typed.exe");
            native::command(d, editors.manage, 0);
            assert_eq!(calls.get(), 1);
            // Cancel keeps the underlying list; the typed path still applies when Settings is saved.
            assert_eq!(dialog.collect_all().external_editors[1].path, r"C:\Topaz\topaz.exe");
            assert_eq!(dialog.control_text(ids.editor_path), r"C:\New\typed.exe");
            assert!(dialog.try_save_all());
            assert_eq!(dialog.settings().external_editors[1].path, r"C:\New\typed.exe");
            d.close();
        })
        .unwrap();
    });
}

struct RestoreLanguage(Arc<Localizer>);
impl Drop for RestoreLanguage {
    fn drop(&mut self) {
        Localizer::set_current(self.0.clone());
    }
}
fn languages() -> [Arc<Localizer>; 2] {
    let mut warnings = Vec::new();
    let en = pr_l10n::catalog::LanguageCatalog::try_parse(
        include_str!("../../../assets/languages/en.json"),
        "en.json",
        &mut warnings,
    )
    .unwrap();
    let vi = pr_l10n::catalog::LanguageCatalog::try_parse(
        include_str!("../../../assets/languages/vi.json"),
        "vi.json",
        &mut warnings,
    )
    .unwrap();
    [
        Arc::new(Localizer::create(&en, &[], &warnings)),
        Arc::new(Localizer::create(&en, &[vi], &warnings)),
    ]
}

// C#: FilesPage_EditorGroup_PaintsTheManageButtonAndTheSummary, four DPI/language rows.
#[test]
#[ignore = "native: real desktop pixels at96/144DPI en/vi"]
fn files_page_editor_group_paints_manage_summary_and_raw_switch() {
    native::on_sta(|| {
        use pr_platform_win::win32::{dialog_native, messages};
        let _restore = RestoreLanguage(Localizer::current());
        let measured = Cell::new(0);
        for (language, localizer) in ["en", "vi"].into_iter().zip(languages()) {
            Localizer::set_current(localizer);
            for dpi in [96u32, 144] {
                let _styles = native::activate_visual_styles();
                let (_, editors) = ids();
                let mut args = settings_fixture::args();
                args.current = sample();
                let dialog = SettingsDialog::new(args).unwrap();
                native::show_on_owner(dialog.native(), |d, _| {
                    dialog.ensure_all_pages_created();
                    let frame = native::window_rect(d.hwnd());
                    let current = d.dpi() as i32;
                    let suggested = windows::Win32::Foundation::RECT {
                        left: frame.left,
                        top: frame.top,
                        right: frame.left + (frame.right - frame.left) * dpi as i32 / current,
                        bottom: frame.top + (frame.bottom - frame.top) * dpi as i32 / current,
                    };
                    // SAFETY: synchronous WM_DPICHANGED reads this live suggested RECT and the nonzero packed DPI.
                    unsafe {
                        dialog_native::user32::send_message(
                            d.hwnd(),
                            messages::WM_DPICHANGED,
                            ((dpi << 16) | dpi) as usize,
                            (&suggested as *const windows::Win32::Foundation::RECT) as isize,
                        );
                    }
                    assert_eq!(d.dpi(), dpi);
                    dialog.activate_page(5, false, true);
                    for id in [editors.manage, editors.summary, editors.send_raw] {
                        dialog.ensure_visible(id);
                        native::drain(d);
                    }
                    let shot = capture::Shot::capture(d.hwnd());
                    for id in [editors.manage, editors.summary, editors.send_raw] {
                        assert!(dialog.is_control_visible(id));
                        shot.assert_not_flat(control(d, id), &format!("Files control{id} {language} {dpi}"));
                        measured.set(measured.get() + 1);
                    }
                    d.close();
                })
                .unwrap();
            }
        }
        assert_eq!(measured.get(), 12);
    });
}

// C#: FilesSettingsPage.OnLanguageChanged refreshes its dynamic summary as well as static labels.
#[test]
#[ignore = "native: needs desktop"]
fn files_page_summary_updates_on_language_change() {
    native::on_sta(|| {
        let _restore = RestoreLanguage(Localizer::current());
        let [en, vi] = languages();
        Localizer::set_current(en);
        let (_, editors) = ids();
        let mut args = settings_fixture::args();
        args.current = sample();
        let dialog = SettingsDialog::new(args).unwrap();
        native::show_on_owner(dialog.native(), |d, _| {
            dialog.activate_page(5, false, true);
            let before = native::text_of(control(d, editors.summary));
            Localizer::set_current(vi.clone());
            native::drain(d);
            let after = native::text_of(control(d, editors.summary));
            assert_eq!(after, tr::settings_external_editors_summary(3i64, "Topaz AI"));
            assert_ne!(before, after);
            d.close();
        })
        .unwrap();
    });
}

// C#: SettingsFilesPageTests.JournalRadios_Tab_VisitsOnlyTheCheckedRadio_AndTheArrowKeysMoveTheChoice.
#[test]
#[ignore = "native: needs desktop"]
fn journal_radios_tab_from_send_raw_visits_checked_radio_and_arrows_change_choice() {
    native::on_sta(|| {
        use pr_platform_win::win32::dialog_native::user32;
        let (ids, editors) = ids();
        let mut page = FilesSettingsPage::new();
        page.declare(&mut SettingsPageBuilder::new(page_id_base(5)));
        let radios = page.journal_radios().to_vec();
        assert_eq!(radios.len(), 2);
        let dialog = SettingsDialog::new(settings_fixture::args()).unwrap();
        native::show_on_owner(dialog.native(), |d, _| {
            dialog.activate_page(5, false, true);
            let send_raw = control(d, editors.send_raw);
            let first = control(d, radios[0]);
            let second = control(d, radios[1]);
            user32::set_focus(send_raw);
            assert!(native::key(d, send_raw, native::VK_TAB));
            native::drain(d);
            assert_eq!(user32::get_focus(), first);
            assert!(native::key(d, first, native::VK_TAB));
            native::drain(d);
            assert_eq!(user32::get_focus(), control(d, ids.reuse_last_folder));
            user32::set_focus(first);
            assert!(native::key(d, first, 0x28)); // VK_DOWN
            native::drain(d);
            assert_eq!(user32::get_focus(), second);
            assert!(ButtonStateControl::is_checked(second));
            assert!(!ButtonStateControl::is_checked(first));
            assert!(dialog.collect_all().journal_safe);
            user32::set_focus(send_raw);
            assert!(native::key(d, send_raw, native::VK_TAB));
            native::drain(d);
            assert_eq!(user32::get_focus(), second);
            assert!(native::key(d, second, 0x26)); // VK_UP
            native::drain(d);
            assert!(ButtonStateControl::is_checked(first));
            assert!(!dialog.collect_all().journal_safe);
            d.close();
        })
        .unwrap();
    });
}
