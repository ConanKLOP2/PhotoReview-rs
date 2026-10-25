use pr_platform_win::dialogs::settings::dialog::create::{is_group_box_style, page_control_style};
use pr_platform_win::dialogs::settings::dialog::frame::{
    APPLY_ID, CANCEL_ID, DEFAULTS_ID, EXPORT_ID, HEADER_ID, IMPORT_ID, NAV_ID, PAGE_ID_STRIDE, PAGE_TITLE_ID,
    RESET_PAGE_ID, SAVE_ID, SCROLL_ID, VERSION_ID, VIEWPORT_BASE, page_id_base, viewport_id,
};
use pr_platform_win::dialogs::settings::dialog::nav::{DialogKeyAction, VK_TAB, classify_dialog_key, page_of};

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
