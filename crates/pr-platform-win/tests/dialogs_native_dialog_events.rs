//! Pure routing and constant checks for W8-18j (NativeDialog.cs, baseline d92d6aae).

use pr_platform_win::dialogs::native::dialog::{
    events::{CommandRoute, LIST_VIEW_SUBCLASS_ID, class_name_is, command_route, split_command_wparam},
    focus::is_tab_stop,
    tooltip::{ITEM_TIP_SUBCLASS_ID, tip_capacity},
};

#[test]
fn command_route_table() {
    // C#: NativeDialog.HandleCommand.
    for (id, notification, expected) in [
        (1, 0, CommandRoute::Ok),
        (1, 1, CommandRoute::Other),
        (2, 0, CommandRoute::Cancel),
        (3, 0, CommandRoute::Other),
        (0, 0, CommandRoute::Other),
        (-1, 0, CommandRoute::Other),
    ] {
        assert_eq!(
            command_route(id, notification),
            expected,
            "id={id}, notification={notification}"
        );
    }
}

#[test]
fn split_command_wparam_table() {
    // C#: NativeDialog.HandleMessage WM_COMMAND.
    for (word, expected) in [
        (0x1234, (0x1234, 0)),
        (0x5678_0000, (0, 0x5678)),
        (0x5678_1234, (0x1234, 0x5678)),
        (0xffff_ffff, (0xffff, 0xffff)),
        (0x1_5678_1234, (0x1234, 0x5678)),
    ] {
        assert_eq!(split_command_wparam(word), expected, "word={word:#x}");
    }
}

#[test]
fn class_name_is_table() {
    // C#: NativeDialog.IsEdit / IsButton.
    for (actual, expected, result) in [
        ("Button", "Button", true),
        ("bUtToN", "Button", true),
        ("Butto", "Button", false),
        ("ButtonX", "Button", false),
        ("", "Button", false),
        ("", "", true),
        ("Édit", "Edit", false),
        ("Bütton", "Button", false),
    ] {
        assert_eq!(
            class_name_is(actual, expected),
            result,
            "actual={actual:?}, expected={expected:?}"
        );
    }
}

#[test]
fn is_tab_stop_table() {
    // C#: NativeDialog.FirstTabStop.
    for (style, expected) in [
        (0, false),
        (0x0001_0000, true),
        (0x0000_ffff, false),
        (0x8001_0000, true),
    ] {
        assert_eq!(is_tab_stop(style), expected, "style={style:#x}");
    }
}

#[test]
fn tip_capacity_table() {
    // C#: NativeDialog.EnsureTipBuffer.
    for (chars, expected) in [(0, 256), (1, 256), (255, 256), (256, 256), (257, 257), (1000, 1000)] {
        assert_eq!(tip_capacity(chars), expected, "chars={chars}");
    }
}

#[test]
fn subclass_ids_match_the_csharp_golden() {
    // C#: NativeDialog.ListViewSubclassId / ItemTipSubclassId.
    let golden = pr_test_support::golden::load("dialog-layout");
    let row = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "constants")
        .expect("constants row");
    assert_eq!(LIST_VIEW_SUBCLASS_ID as u64, row["list_view_subclass_id"]);
    assert_eq!(ITEM_TIP_SUBCLASS_ID as u64, row["item_tip_subclass_id"]);
}
