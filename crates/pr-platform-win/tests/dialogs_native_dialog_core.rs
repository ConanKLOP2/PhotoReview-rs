//! Pure contract tests for W8-18h, port of NativeDialog.cs at d92d6aae.

use std::rc::Rc;

use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::dialog::core::{
    DialogContent, DialogError, ID_CANCEL, ID_OK, NativeDialog, WS_DEFAULT_DIALOG,
};
use pr_platform_win::dialogs::native::dialog::modal::{dialog_ex_style, dialog_style};
use pr_platform_win::dialogs::native::template::{DialogControlClass, DialogItem, TemplateError};
use pr_platform_win::win32::structs::Msg;

struct MinimalContent;

impl DialogContent for MinimalContent {
    fn title(&self) -> String {
        "test".into()
    }
    fn items(&self) -> Vec<DialogItem> {
        Vec::new()
    }
    fn preferred_client_size(&self, _: &NativeDialog) -> (i32, i32) {
        (200, 100)
    }
    fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
}

fn dialog() -> NativeDialog {
    NativeDialog::new(Rc::new(MinimalContent))
}

#[test]
fn constants_match_the_csharp_golden() {
    // C#: NativeDialog.IdOk, IdCancel, WsDefaultDialog (SizeMinimized is checked in core.rs).
    let golden = pr_test_support::golden::load("dialog-layout");
    let rows: Vec<_> = golden.cases.iter().filter(|c| c["kind"] == "constants").collect();
    assert_eq!(rows.len(), 1);
    let row = rows[0];
    assert_eq!(i64::from(ID_OK), row["id_ok"]);
    assert_eq!(i64::from(ID_CANCEL), row["id_cancel"]);
    assert_eq!(u64::from(WS_DEFAULT_DIALOG), row["ws_default_dialog"]);
}

#[test]
fn dialog_style_matches_the_csharp_golden_for_four_dialog_types() {
    // C#: NativeDialog.DialogStyle / DialogExStyle in ListDialog, DiagnosticsDialog, BenchmarkDialog, ZoomPromptDialog.
    let golden = pr_test_support::golden::load("dialog-layout");
    let mut seen = Vec::new();
    for row in golden.cases.iter().filter(|c| c["kind"] == "style") {
        let name = row["type"].as_str().expect("dialog type");
        let (resizable, frame, extra) = match name {
            "ListDialog" => (true, true, 0),
            "DiagnosticsDialog" | "ZoomPromptDialog" => (false, true, 0),
            "BenchmarkDialog" => (true, false, 0x0002_0000 | 0x0001_0000),
            other => panic!("unexpected style row: {other}"),
        };
        assert_eq!(u64::from(dialog_style(resizable, frame, extra)), row["style"], "{name}");
        assert_eq!(u64::from(dialog_ex_style(frame)), row["ex_style"], "{name}");
        seen.push(name);
    }
    assert_eq!(seen.len(), 4);
}

#[test]
fn dialog_style_flags_are_independent() {
    // C#: NativeDialog.DialogStyle.
    for resizable in [false, true] {
        for frame in [false, true] {
            for extra in [0, 0x0003_0000] {
                let expected = WS_DEFAULT_DIALOG
                    | 0x10
                    | if frame { 0x80 } else { 0 }
                    | if resizable { 0x0004_0000 } else { 0 }
                    | extra;
                assert_eq!(dialog_style(resizable, frame, extra), expected);
            }
        }
    }
}

#[test]
fn dialog_ex_style_has_control_parent_always() {
    // C#: NativeDialog.DialogExStyle.
    assert_eq!(dialog_ex_style(false), 0x1_0000);
    assert_eq!(dialog_ex_style(true), 0x1_0001);
}

#[test]
fn dialog_error_display_texts() {
    // C#: NativeDialog.ShowModal invalid operation and CreateDialogIndirectParam failure.
    assert_eq!(
        DialogError::AlreadyShown.to_string(),
        "A NativeDialog instance can be shown once."
    );
    assert_eq!(
        DialogError::Template(TemplateError::TooManyControls { count: 65_536 }).to_string(),
        "Too many controls for a dialog template."
    );
    assert_eq!(
        DialogError::CreateFailed(pr_platform_win::win32::structs::Win32Error(87)).to_string(),
        "CreateDialogIndirectParam failed (Win32 error 87)."
    );
    let host = pr_platform_win::host::error::HostError::InvalidOperation {
        message: "wrong thread",
    };
    assert_eq!(DialogError::Loop(host).to_string(), "wrong thread");
}

#[test]
fn template_error_converts_with_question_mark() {
    // C#: DialogTemplate.Build throws ArgumentException, mapped to DialogError::Template.
    fn convert() -> Result<(), DialogError> {
        Err(TemplateError::TooManyControls { count: 65_536 })?
    }
    assert!(matches!(
        convert(),
        Err(DialogError::Template(TemplateError::TooManyControls { count: 65_536 }))
    ));
}

#[test]
fn content_defaults_are_the_csharp_defaults() {
    // C#: NativeDialog virtual members, lines 90-183 and 519-524.
    let content = MinimalContent;
    let dialog = dialog();
    assert!(!content.resizable());
    assert!(content.header_control_ids().is_empty());
    assert_eq!(content.initial_focus_id(), 0);
    assert_eq!(content.default_button_id(), ID_OK);
    assert_eq!(content.minimum_client_size(&dialog), (200, 100));
    assert!(content.on_ok(&dialog));
    assert!(!content.on_cancel(&dialog));
    assert!(!content.on_command(&dialog, 9, 2));
    assert_eq!(content.on_notify(&dialog, 9, 2, 0), None);
    assert!(content.can_close_by_user());
    assert!(!content.wait_cursor());
    assert_eq!(content.static_text_color(&dialog, 9), DialogColors::text());
    assert_eq!(content.list_item_subtitle(&dialog, 9, 0), None);
    assert_eq!(content.list_item_badge_color(&dialog, 9, 0), None);
    assert_eq!(content.extra_window_style(), 0);
    assert!(content.uses_modal_frame());
    assert_eq!(content.on_window_message(&dialog, 0x8001, 0, 0), None);
    assert!(!content.on_dialog_key(&dialog, &Msg::default()));
}

#[test]
fn a_new_dialog_has_no_window() {
    // C#: NativeDialog constructor and field initializers.
    let dialog = dialog();
    assert_eq!(dialog.hwnd(), 0);
    assert!(!dialog.is_closed());
    assert_eq!(dialog.dpi(), 96);
    assert_eq!(dialog.body_font(), 0);
    assert_eq!(dialog.self_ref_data(), 0);
}

#[test]
fn end_modal_before_show_is_idempotent_and_the_first_result_wins() {
    // C#: NativeDialog.EndModal.
    let dialog = dialog();
    dialog.end_modal(ID_OK);
    dialog.end_modal(ID_CANCEL);
    assert!(dialog.is_closed());
    // A later desktop test verifies the retained result through ShowModal.
}

#[test]
fn close_before_show_marks_the_dialog_closed() {
    // C#: NativeDialog.Close and EndModal.
    let dialog = dialog();
    dialog.close();
    assert!(dialog.is_closed());
    assert_eq!(dialog.hwnd(), 0);
}

#[test]
fn too_many_controls_is_a_template_error_and_the_second_show_is_rejected() {
    // C#: NativeDialog.ShowModal + DialogTemplate.Build.
    struct Many;
    impl DialogContent for Many {
        fn title(&self) -> String {
            String::new()
        }
        fn items(&self) -> Vec<DialogItem> {
            vec![DialogItem::new(DialogControlClass::Button, 1, "", 0); 65_536]
        }
        fn preferred_client_size(&self, _: &NativeDialog) -> (i32, i32) {
            (100, 100)
        }
        fn layout_controls(&self, _: &NativeDialog, _: i32, _: i32) {}
    }
    let dialog = NativeDialog::new(Rc::new(Many));
    assert!(matches!(
        dialog.show_modal(0, None, None),
        Err(DialogError::Template(TemplateError::TooManyControls { count: 65_536 }))
    ));
    assert_eq!(dialog.hwnd(), 0);
    assert!(matches!(
        dialog.show_modal(0, None, None),
        Err(DialogError::AlreadyShown)
    ));
}
