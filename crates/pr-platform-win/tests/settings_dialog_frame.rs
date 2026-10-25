use pr_platform_win::dialogs::settings::dialog::create::{is_group_box_style, page_control_style};
use pr_platform_win::dialogs::settings::dialog::frame::{
    APPLY_ID, CANCEL_ID, DEFAULTS_ID, EXPORT_ID, HEADER_ID, IMPORT_ID, NAV_ID, PAGE_ID_STRIDE, PAGE_TITLE_ID,
    RESET_PAGE_ID, SAVE_ID, SCROLL_ID, VERSION_ID, VIEWPORT_BASE, page_id_base, viewport_id,
};
use pr_platform_win::dialogs::settings::dialog::nav::{DialogKeyAction, VK_TAB, classify_dialog_key, page_of};
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

#[test]
fn every_frame_case_matches_the_csharp_golden() {
    use pr_platform_win::dialogs::native::dialog::core::DialogContent;
    use pr_platform_win::dialogs::settings::dialog::frame::*;
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/settings-dialog-frame.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 9);
    for row in cases {
        match row["kind"].as_str().unwrap() {
            "frame" => {
                let mut args = fixture::args();
                args.options.version = Some("9.9.9+oracle".to_owned());
                let d = SettingsDialog::new(args).unwrap();
                let c = d.content().as_ref();
                assert_eq!(DialogContent::title(c), row["title"].as_str().unwrap());
                assert_eq!(DialogContent::resizable(c), row["resizable"].as_bool().unwrap());
                assert_eq!(
                    serde_json::json!(DialogContent::header_control_ids(c)),
                    row["header_ids"]
                );
                assert_eq!(
                    DialogContent::initial_focus_id(c),
                    row["initial_focus_id"].as_i64().unwrap() as i32
                );
                assert_eq!(
                    DialogContent::default_button_id(c),
                    row["default_button_id"].as_i64().unwrap() as i32
                );
                let items:Vec<_>=DialogContent::items(c).iter().map(|i|serde_json::json!({"class":format!("{:?}",i.class),"id":i.id,"text":i.text,"style":i.style,"ex_style":i.ex_style})).collect();
                assert_eq!(items, *row["items"].as_array().unwrap());
                assert_eq!(
                    serde_json::json!((0..d.page_count()).map(|i| d.page_key(i).unwrap()).collect::<Vec<_>>()),
                    row["page_keys"]
                );
                let expected_counts = row["page_item_counts"].as_array().unwrap().clone();
                assert_eq!(
                    serde_json::json!((0..d.page_count()).map(|i| d.page_items(i).len()).collect::<Vec<_>>()),
                    serde_json::Value::Array(expected_counts)
                );
                assert_eq!(d.active_page_index(), row["active_page_index"].as_i64().unwrap() as i32);
                assert_eq!(d.active_page_key(), row["active_page_key"].as_str().unwrap());
                assert_eq!(d.saved(), row["saved"].as_bool().unwrap());
                // C# runtime page_types are descriptive class names; Rust trait objects expose page keys and declarations above.
            }
            "constants" => {
                let actual = serde_json::json!({"name":"constants","kind":"constants","header_id":HEADER_ID,"nav_id":NAV_ID,"version_id":VERSION_ID,"page_title_id":PAGE_TITLE_ID,"scroll_id":SCROLL_ID,"viewport_base":VIEWPORT_BASE,"apply_id":APPLY_ID,"defaults_id":DEFAULTS_ID,"reset_page_id":RESET_PAGE_ID,"export_id":EXPORT_ID,"import_id":IMPORT_ID,"page_id_stride":PAGE_ID_STRIDE,"save_id":SAVE_ID,"cancel_id":CANCEL_ID,"wm_app_language":WM_APP_LANGUAGE,"wm_app_queue":WM_APP_QUEUE,"forward_all_subclass_id":FORWARD_ALL_SUBCLASS_ID,"wheel_subclass_id":WHEEL_SUBCLASS_ID,"key_box_subclass_id":KEY_BOX_SUBCLASS_ID});
                assert_eq!(&actual, row);
            }
            "page_math" => {
                for (i, index) in row["indices"].as_array().unwrap().iter().enumerate() {
                    let index = index.as_i64().unwrap() as i32;
                    assert_eq!(page_id_base(index), row["page_id_base"][i].as_i64().unwrap() as i32);
                    assert_eq!(viewport_id(index), row["viewport_id"][i].as_i64().unwrap() as i32);
                }
            }
            "page_of" => {
                for (i, id) in row["ids"].as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        page_of(id.as_i64().unwrap() as i32),
                        row["results"][i].as_i64().unwrap() as i32
                    );
                }
            }
            "dialog_key" => {
                let mut args = fixture::args();
                let control = row["control"].as_bool().unwrap();
                args.options.control_key_down = Some(std::rc::Rc::new(move || control));
                let d = SettingsDialog::new(args).unwrap();
                let message = pr_platform_win::win32::structs::Msg {
                    message: match row["message"].as_str().unwrap() {
                        "keydown" => 0x100,
                        "keyup" => 0x101,
                        "syskeydown" => 0x104,
                        other => panic!("unknown key message {other}"),
                    },
                    wParam: windows::Win32::Foundation::WPARAM(row["vk"].as_u64().unwrap() as usize),
                    ..Default::default()
                };
                assert_eq!(d.handle_key_for_test(&message), row["handled"].as_bool().unwrap());
                assert_eq!(d.active_page_index(), row["active_page_index"].as_i64().unwrap() as i32);
                assert_eq!(d.active_page_key(), row["active_page_key"].as_str().unwrap());
            }
            other => panic!("unhandled frame kind {other}"),
        }
    }
}

#[test]
fn constants_match_frame_contract() {
    assert_eq!(
        [HEADER_ID, NAV_ID, VERSION_ID, PAGE_TITLE_ID, SCROLL_ID],
        [100, 101, 102, 103, 104]
    );
    assert_eq!(
        [DEFAULTS_ID, RESET_PAGE_ID, EXPORT_ID, IMPORT_ID, CANCEL_ID, APPLY_ID],
        [121, 122, 123, 124, 2, 120]
    );
    assert_eq!(SAVE_ID, 1);
    assert_eq!(VIEWPORT_BASE, 110);
    assert_eq!(PAGE_ID_STRIDE, 1000);
}

#[test]
fn page_math_matches_contract() {
    for index in -1..=7 {
        assert_eq!(viewport_id(index), 110_i32.wrapping_add(index));
        assert_eq!(page_id_base(index), 1000_i32.wrapping_mul(index.wrapping_add(1)));
    }
    assert_eq!(viewport_id(100), 210);
    assert_eq!(page_id_base(100), 101_000);
}

#[test]
fn page_of_matches_id_ranges() {
    for (id, expected) in [
        (0, -1),
        (999, -1),
        (1000, 0),
        (1001, 0),
        (1999, 0),
        (2000, 1),
        (2999, 1),
        (7001, 6),
        (8000, 7),
    ] {
        assert_eq!(page_of(id), expected, "id {id}");
    }
}

#[test]
fn ctrl_tab_cycles_and_plain_tab_is_left_to_dialog_manager() {
    assert_eq!(
        classify_dialog_key(0x0100, VK_TAB as usize, true, false, false),
        DialogKeyAction::CyclePage { forward: true }
    );
    assert_eq!(
        classify_dialog_key(0x0100, VK_TAB as usize, true, true, false),
        DialogKeyAction::CyclePage { forward: false }
    );
    assert_eq!(
        classify_dialog_key(0x0100, VK_TAB as usize, false, false, false),
        DialogKeyAction::NotHandled
    );
    assert_eq!(
        classify_dialog_key(0x0104, VK_TAB as usize, true, false, false),
        DialogKeyAction::NotHandled
    );
}

#[test]
fn classify_dialog_key_handles_capture_box_keys_only() {
    assert_eq!(
        classify_dialog_key(0x0100, 0x0D, false, false, true),
        DialogKeyAction::CaptureKey
    );
    assert_eq!(
        classify_dialog_key(0x0104, 0x25, false, false, true),
        DialogKeyAction::CaptureKey
    );
    assert_eq!(
        classify_dialog_key(0x0101, 0x0D, false, false, true),
        DialogKeyAction::NotHandled
    );
    assert_eq!(
        classify_dialog_key(0x0100, 0x0D, false, false, false),
        DialogKeyAction::NotHandled
    );
}

#[test]
fn group_box_style_uses_low_nibble_and_page_style_adds_clip_siblings_once() {
    assert!(is_group_box_style(0xFFFF_FFF7));
    assert!(!is_group_box_style(3));
    assert!(!is_group_box_style(4));
    assert!(!is_group_box_style(0));
    assert_eq!(page_control_style(0x0400_0000 | 3), 0x0400_0003);
    assert_eq!(page_control_style(3), 0x0400_0003);
}
