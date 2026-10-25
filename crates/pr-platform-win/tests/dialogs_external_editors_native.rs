//! W9-E2 native message tests from ExternalEditorsUiTests (7eaee2a2).
#[path = "common/w9_e2_capture.rs"]
mod capture;
#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;
use fixture::{command, item, on_sta, set_text, show_on_owner, text_of};
use pr_core::settings::external_editors::ExternalEditor;
use pr_platform_win::dialogs::{
    action_profiles::DialogWarningSink,
    file_dialog::{FileDialogFilter, ShellFileDialog},
    native::{
        controls::basic::ListBoxControl,
        dialog::{ID_CANCEL, ID_OK},
        external_editors_dialog as ed,
    },
};
use pr_platform_win::win32::structs::{Hresult, Hwnd};
use std::{cell::RefCell, rc::Rc};

struct Files;
impl ShellFileDialog for Files {
    fn pick_open_file(
        &self,
        _: Hwnd,
        _: &str,
        _: &[FileDialogFilter],
        _: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        Ok(Some(r"C:\Apps\Canon DPP\dpp.exe".to_owned()))
    }
    fn pick_save_file(
        &self,
        _: Hwnd,
        _: &str,
        _: &[FileDialogFilter],
        _: Option<&str>,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<Option<String>, Hresult> {
        panic!("unexpected save picker")
    }
}
#[derive(Default)]
struct Warnings(RefCell<Vec<(String, String)>>);
impl DialogWarningSink for Warnings {
    fn show_warning(&self, _: Hwnd, title: &str, text: &str) {
        self.0.borrow_mut().push((title.to_owned(), text.to_owned()));
    }
}
fn sample() -> Vec<ExternalEditor> {
    [
        ("lr", "Lightroom", r"C:\Adobe\Lightroom\lightroom.exe"),
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
    .collect()
}

// C#: ExternalEditorsUiTests.EditorsDialog_AddRemoveSetDefaultAndEdit_ProduceTheExpectedResult.
#[test]
#[ignore = "native: needs desktop"]
fn editors_dialog_add_remove_set_default_and_edit_produce_the_expected_result() {
    on_sta(|| {
        let original = sample();
        let dialog = ed::ExternalEditorsDialog::new(
            &original,
            Some("tp"),
            Some(Rc::new(Files)),
            Some(Rc::new(Warnings::default())),
            Some(Rc::new(|_| Some("Canon DPP".to_owned()))),
            None,
        );
        assert_eq!(
            show_on_owner(dialog.native(), |d, _| {
                command(d, ed::ADD_ID, 0);
                assert_eq!(text_of(item(d, ed::NAME_ID)), "Canon DPP");
                assert_eq!(text_of(item(d, ed::PATH_ID)), r"C:\Apps\Canon DPP\dpp.exe");
                set_text(d, ed::ARGUMENTS_ID, "  -edit {file}  ");
                command(d, ed::SET_DEFAULT_ID, 0);
                assert!(dialog.row_text(3).badge.is_some());
                assert!(dialog.row_text(1).badge.is_none());
                ListBoxControl::select_single(item(d, ed::LIST_ID), 0);
                command(d, ed::LIST_ID, 1);
                command(d, ed::REMOVE_ID, 0);
                command(d, ID_OK, 0);
            })
            .unwrap(),
            ID_OK
        );
        let edit = dialog.outcome();
        assert_eq!(edit.editors.len(), 3);
        assert_eq!(edit.editors[0].id, "tp");
        assert_eq!(edit.editors[1].id, "dxo");
        assert_eq!(edit.editors[2].name, "Canon DPP");
        assert_eq!(edit.editors[2].arguments, "  -edit {file}  ");
        assert_eq!(edit.default_id, edit.editors[2].id);
        assert_eq!(original, sample(), "working copy leaves original untouched");
    });
}

// C#: ExternalEditorsUiTests.EditorsDialog_RemovingTheDefault_MovesTheDefaultToTheFirstRemaining_AndABlankPathBlocksApply.
#[test]
#[ignore = "native: needs desktop"]
fn editors_dialog_removing_the_default_moves_it_to_first_remaining_and_blank_path_blocks_apply() {
    on_sta(|| {
        let warnings = Rc::new(Warnings::default());
        let dialog = ed::ExternalEditorsDialog::new(&sample(), Some("lr"), None, Some(warnings.clone()), None, None);
        assert_eq!(
            show_on_owner(dialog.native(), |d, _| {
                command(d, ed::REMOVE_ID, 0);
                set_text(d, ed::PATH_ID, "   ");
                command(d, ID_OK, 0);
                assert_eq!(warnings.0.borrow().len(), 1);
                assert!(!d.is_closed());
                set_text(d, ed::PATH_ID, r"C:\Topaz\topaz.exe");
                command(d, ID_OK, 0);
            })
            .unwrap(),
            ID_OK
        );
        let edit = dialog.outcome();
        assert_eq!(
            edit.editors.iter().map(|editor| editor.id.as_str()).collect::<Vec<_>>(),
            ["tp", "dxo"]
        );
        assert_eq!(edit.default_id, "tp");
    });
}

// C#: ExternalEditorsUiTests.EditorsDialog_Cancel_ReturnsCancel.
#[test]
#[ignore = "native: needs desktop"]
fn editors_dialog_cancel_returns_cancel() {
    on_sta(|| {
        let original = sample();
        let dialog = ed::ExternalEditorsDialog::new(&original, Some("tp"), None, None, None, None);
        assert_eq!(
            show_on_owner(dialog.native(), |d, _| {
                command(d, ID_CANCEL, 0);
            })
            .unwrap(),
            ID_CANCEL
        );
        assert_eq!(original, sample());
    });
}

// C#: ExternalEditorsUiTests.EditorsDialog_PaintsListAndDetails, four DPI/language rows.
#[test]
#[ignore = "native: real desktop pixels at 96/144 DPI, en/vi"]
fn editors_dialog_paints_list_and_details() {
    on_sta(|| {
        use pr_l10n::{catalog::LanguageCatalog, localizer::Localizer};
        use pr_platform_win::win32::{dialog_native, messages};
        use std::sync::Arc;
        struct Restore(Arc<Localizer>);
        impl Drop for Restore {
            fn drop(&mut self) {
                Localizer::set_current(self.0.clone());
            }
        }
        let _restore = Restore(Localizer::current());
        let mut warnings = Vec::new();
        let english = LanguageCatalog::try_parse(
            include_str!("../../../assets/languages/en.json"),
            "en.json",
            &mut warnings,
        )
        .unwrap();
        let vietnamese = LanguageCatalog::try_parse(
            include_str!("../../../assets/languages/vi.json"),
            "vi.json",
            &mut warnings,
        )
        .unwrap();
        let measured = std::cell::Cell::new(0);
        for (language, overlays) in [("en", Vec::new()), ("vi", vec![vietnamese])] {
            Localizer::set_current(Arc::new(Localizer::create(&english, &overlays, &warnings)));
            for dpi in [96u32, 144] {
                let _styles = fixture::activate_visual_styles();
                let dialog = ed::ExternalEditorsDialog::new(&sample(), Some("tp"), None, None, None, None);
                show_on_owner(dialog.native(), |d, _| {
                    let frame = fixture::window_rect(d.hwnd());
                    let current = d.dpi() as i32;
                    let suggested = windows::Win32::Foundation::RECT {
                        left: frame.left,
                        top: frame.top,
                        right: frame.left + (frame.right - frame.left) * dpi as i32 / current,
                        bottom: frame.top + (frame.bottom - frame.top) * dpi as i32 / current,
                    };
                    // SAFETY: WM_DPICHANGED synchronously reads the live suggested RECT;
                    // the packed DPI has the same nonzero X and Y components.
                    unsafe {
                        dialog_native::user32::send_message(
                            d.hwnd(),
                            messages::WM_DPICHANGED,
                            ((dpi << 16) | dpi) as usize,
                            (&suggested as *const windows::Win32::Foundation::RECT) as isize,
                        );
                    }
                    assert_eq!(d.dpi(), dpi);
                    fixture::drain(d);
                    let shot = capture::Shot::capture(d.hwnd());
                    for id in [
                        ed::LIST_ID,
                        ed::NAME_ID,
                        ed::PATH_ID,
                        ed::ARGUMENTS_ID,
                        ed::SET_DEFAULT_ID,
                    ] {
                        shot.assert_not_flat(item(d, id), &format!("control {id} {language} {dpi}"));
                        measured.set(measured.get() + 1);
                    }
                    command(d, ID_CANCEL, 0);
                })
                .unwrap();
            }
        }
        assert_eq!(measured.get(), 20);
    });
}
