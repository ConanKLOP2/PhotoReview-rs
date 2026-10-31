//! Port of SettingsShortcutsPageTests and SettingsDialog control-accessor rules (W8-19k).
use std::cell::Cell;
use std::rc::{Rc, Weak};

use pr_app::input::key_mapping::from_virtual_key;
use pr_app::services::shortcut_key_name::try_parse;
use pr_platform_win::dialogs::settings::context::{ActionProfilesUnavailable, SettingsPageHost};
use pr_platform_win::dialogs::settings::dialog::{dirty::*, host::*, keybox::*};
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;

#[test]
fn loading_guard_restores_nested_state_and_restores_after_panic() {
    use pr_platform_win::dialogs::native::dialog::core::DialogContent;
    use pr_platform_win::dialogs::settings::dialog::state::LoadingGuard;
    let d = fixture::dialog();
    let notification = || DialogContent::on_command(d.content().as_ref(), d.native(), 999, 99);
    assert!(!notification());
    {
        let _outer = LoadingGuard::new(d.content());
        assert!(notification());
        {
            let _inner = LoadingGuard::new(d.content());
            assert!(notification());
        }
        assert!(notification(), "inner guard cleared the outer loading state");
    }
    assert!(!notification());
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = LoadingGuard::new(d.content());
        assert!(notification());
        panic!("simulated page-load failure");
    }));
    assert!(panic.is_err());
    assert!(!notification(), "page notifications still swallowed after panic");
}

#[test]
fn action_json_visibility_and_key_box_rows_match_the_csharp_golden() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/settings-dialog-state.json")).unwrap();
    let mut covered = 0;
    for row in golden["cases"].as_array().unwrap() {
        let kind = row["kind"].as_str().unwrap();
        if kind == "key_capture" {
            continue;
        }
        let mut args = fixture::args();
        if row["settings"]["ActionsText"] == "[]" {
            args.current.actions.clear();
        }
        let dialog = pr_platform_win::dialogs::settings::dialog::SettingsDialog::new(args).unwrap();
        match kind {
            "actions_json" => {
                let mut actual = Vec::new();
                for op in row["ops"].as_array().unwrap() {
                    let value = match op["op"].as_str().unwrap() {
                        "get" => dialog.actions_json(),
                        "refresh" => dialog.refresh_actions_json_from_pages(),
                        "set" => {
                            dialog.set_actions_json(op["json"].as_str().unwrap(), "");
                            dialog.actions_json()
                        }
                        other => panic!("unknown actions op {other}"),
                    };
                    actual.push(serde_json::Value::String(value));
                }
                assert_eq!(actual, *row["values"].as_array().unwrap(), "{}", row["name"]);
            }
            "visibility" => {
                let groups: Vec<_> = (0..dialog.page_count())
                    .flat_map(|i| dialog.page_items(i))
                    .filter(|item| {
                        item.class == pr_platform_win::dialogs::native::template::DialogControlClass::Button
                            && item.style & 15 == 7
                    })
                    .map(|item| item.id)
                    .collect();
                assert!(groups.len() >= 2);
                let mut actual = Vec::new();
                for op in row["ops"].as_array().unwrap() {
                    let group = op["group"].as_u64().map(|i| groups[i as usize]);
                    let id = group.unwrap_or_else(|| op["id"].as_i64().unwrap() as i32);
                    match op["op"].as_str().unwrap() {
                        "set" | "set_group" => dialog.set_control_visible(id, op["visible"].as_bool().unwrap()),
                        "is" | "is_group" => actual.push(serde_json::Value::Bool(dialog.is_control_visible(id))),
                        other => panic!("unknown visibility op {other}"),
                    }
                }
                assert_eq!(actual, *row["values"].as_array().unwrap());
            }
            "key_boxes" => {
                let ids: Vec<_> = (0..dialog.page_count())
                    .flat_map(|i| dialog.page_items(i))
                    .filter(|item| dialog.content().is_key_box_id(item.id))
                    .map(|item| serde_json::Value::from(item.id))
                    .collect();
                assert_eq!(ids, *row["key_box_ids"].as_array().unwrap());
                let actual: Vec<_> = row["probes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| serde_json::Value::Bool(dialog.content().is_key_box_id(id.as_i64().unwrap() as i32)))
                    .collect();
                assert_eq!(actual, *row["results"].as_array().unwrap());
            }
            other => panic!("unhandled golden kind {other}"),
        }
        covered += 1;
    }
    assert_eq!(covered, 4);
}

#[test]
fn key_capture_matches_the_golden_for_every_virtual_key() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/golden/settings-dialog-state.json")).unwrap();
    let rows: Vec<_> = golden["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "key_capture")
        .collect();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let extended = row["extended"].as_bool().unwrap();
        let texts = row["texts"].as_array().unwrap();
        assert_eq!(texts.len(), 256);
        for (vk, expected) in texts.iter().enumerate() {
            assert_eq!(
                key_box_text(vk as i32, extended).as_deref(),
                expected.as_str(),
                "vk={vk}, extended={extended}"
            );
        }
    }
}

// C#: EveryCapturedKey_RoundTripsThroughTheShortcutParser_ToTheSameKey.
#[test]
fn every_captured_key_round_trips_through_the_shortcut_parser_to_the_same_key() {
    for extended in [false, true] {
        let mut captured = 0;
        for vk in 1..256 {
            if let Some(text) = key_box_text(vk, extended) {
                assert_eq!(
                    try_parse(Some(&text)),
                    Some(from_virtual_key(vk, extended)),
                    "vk={vk}, {text}"
                );
                captured += 1;
            }
        }
        assert!(captured >= 150, "only {captured} keys captured");
    }
}

#[test]
fn ime_resolution_masks_the_message_and_only_queries_the_ime_for_process_key() {
    assert_eq!(resolve_virtual_key(0x1234_0041, || panic!("IME queried for A")), 65);
    assert_eq!(resolve_virtual_key(0x1234_00e5, || 0), 229);
    assert_eq!(resolve_virtual_key(229, || 229), 229);
    assert_eq!(resolve_virtual_key(229, || 65), 65);
}

#[test]
fn character_and_edit_messages_are_swallowed_but_copy_and_navigation_are_not() {
    for message in [
        0x102, 0x106, 0x103, 0x107, 0x286, 0x10d, 0x10f, 0x302, 0x300, 0x303, 0x304,
    ] {
        assert!(is_swallowed_message(message), "message={message:#x}");
    }
    for message in [0, 0x100, 0x104, 0x101, 0x301, 0x82] {
        assert!(!is_swallowed_message(message), "message={message:#x}");
    }
}

#[test]
fn control_accessor_decisions_keep_button_text_and_range_boundaries() {
    assert_eq!(control_text_for(true, "A&B && C"), "A&&B &&&& C");
    assert_eq!(control_text_for(false, "A&B && C"), "A&B && C");
    for (count, selected, expected) in [(0, 3, -1), (0, -1, -1), (1, -1, 0), (3, -1, 0), (3, 1, 1), (3, 99, 2)] {
        assert_eq!(combo_selection(count, selected), expected);
    }
    for (min, max, expected) in [(0, 0, 1), (20, 10, 1), (0, 9, 1), (10, 30, 2), (-100, 100, 20)] {
        assert_eq!(slider_page_size(min, max), expected);
    }
}

#[test]
fn page_host_after_the_content_is_dropped_answers_neutral_values() {
    let host = PageHost::new(Weak::new());
    assert_eq!(host.hwnd(), 0);
    assert!(host.files().is_none());
    assert!(host.decoder_factory().is_none());
    assert!(host.version().is_none());
    assert_eq!(host.open_url("https://example.invalid"), Err("detached".to_owned()));
    assert_eq!(host.start_explorer("unused"), Some("detached".to_owned()));
    assert_eq!(host.show_action_profiles(0, &[]), Err(ActionProfilesUnavailable));
    assert_eq!(host.control_text(1), "");
    assert!(!host.is_control_checked(1));
    assert_eq!(host.combo_selected_index(1), -1);
    assert_eq!(host.slider_value(1), 0);
    assert!(!host.is_control_enabled(1));
    assert!(host.current_shortcut_texts().is_empty());
    assert_eq!(host.actions_json(), "");
    assert_eq!(host.refresh_actions_json_from_pages(), "");
    assert!(Rc::ptr_eq(&host.presenter(), &host.presenter()));
    host.set_control_text(1, "text");
    host.set_control_checked(1, true);
    host.set_combo_selected_index(1, 0);
    host.set_combo_items(1, &["item".to_owned()], 0);
    host.set_slider_value(1, 20);
    host.set_slider_range(1, 0, 100);
    let queried = Rc::new(Cell::new(false));
    let observed = queried.clone();
    host.set_control_tooltip(
        1,
        Rc::new(move || {
            observed.set(true);
            "tip".to_owned()
        }),
    );
    assert!(!queried.get());
    host.set_control_enabled(1, true);
    host.set_control_visible(1, true);
    host.focus_control(1);
    host.relayout_active_page();
    host.warn("title", "message");
    host.set_actions_json("[]", "source");
    host.ui_post()
        .post(Box::new(|_| panic!("detached poster executed action")));
}

#[test]
fn empty_open_url_is_rejected_without_entering_the_shell() {
    for url in ["", " ", "\r\n\t"] {
        assert_eq!(default_open_url(url), Err("empty url".to_owned()));
    }
}

#[test]
fn open_url_accepts_only_http_and_https_without_entering_the_shell() {
    // ST-WIN32 L11: Rust-only hardening; these are rejected before ShellExecuteExW, so the test never starts a process.
    for url in [
        r"C:\Windows\System32\notepad.exe",
        "file:///C:/Windows/notepad.exe",
        "javascript:alert(1)",
        "ms-settings:",
        "ftp://example.invalid/a",
        "http://",
        "https:/x",
        "https://example.invalid/a b",
        "https://example.invalid/a\"b",
        "https://example.invalid/a\tb",
        "https://example.invalid/a\x01b",
        "https://example.invalid/a\u{7f}",
    ] {
        assert_eq!(default_open_url(url), Err("unsupported url scheme".to_owned()), "{url}");
        assert!(!is_web_url(url), "{url}");
    }
    for url in [
        "http://example.invalid/",
        "HTTPS://example.invalid/a?b=c",
        "  https://example.invalid  ",
    ] {
        assert!(is_web_url(url), "{url}");
    }
}
