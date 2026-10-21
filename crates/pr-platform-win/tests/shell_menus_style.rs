//! Tests of W8-14e `shell_menus::{mapping, owner_draw_style}` (C# `NativeMenuMappingTests`, `MenuOwnerDrawTests`, baseline `d92d6aae`). Pure: no OS call.

use pr_app::menus::model::{MenuItemKind, MenuItemModel};
use pr_app::overlay::palette;
use pr_platform_win::shell_menus::mapping::{display_text, id_for_command, item_flags};
use pr_platform_win::shell_menus::owner_draw_style::{
    ODS_CHECKED, ODS_DISABLED, ODS_GRAYED, ODS_SELECTED, background, color_ref, item_background, item_height,
    item_text, item_width, left_gutter, right_pad, scale, separator, separator_height,
};
use pr_platform_win::win32::messages::{MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_STRING};

// ---------------------------------------------------------------------------------------------------------------------
// mapping
// ---------------------------------------------------------------------------------------------------------------------

fn item(text: &str, shortcut: Option<&str>, enabled: bool, checked: bool) -> MenuItemModel {
    MenuItemModel {
        id: "X".to_owned(),
        kind: MenuItemKind::Command,
        text: text.to_owned(),
        shortcut_text: shortcut.map(str::to_owned),
        is_enabled: enabled,
        is_checked: checked,
        children: Vec::new(),
        automation_name: None,
    }
}

// C#: DisplayText_WithShortcut_AppendsTabAndShortcut
#[test]
fn display_text_with_shortcut_appends_tab_and_shortcut() {
    assert_eq!(
        display_text(&item("Next folder", Some("PageDown"), true, false)),
        "Next folder\tPageDown"
    );
}

// C#: DisplayText_WithoutShortcut_IsJustTheText
#[test]
fn display_text_without_shortcut_is_just_the_text() {
    assert_eq!(
        display_text(&item("Settings\u{2026}", None, true, false)),
        "Settings\u{2026}"
    );
}

// new: an empty shortcut is no shortcut (no dangling TAB)
#[test]
fn display_text_with_an_empty_shortcut_is_just_the_text() {
    assert_eq!(display_text(&item("Undo", Some(""), true, false)), "Undo");
}

// C#: DisplayText_Ampersand_IsDoubledSoItIsNotAMnemonic
#[test]
fn display_text_ampersand_is_doubled_so_it_is_not_a_mnemonic() {
    assert_eq!(display_text(&item("Copy & paste", None, true, false)), "Copy && paste");
}

// new
#[test]
fn display_text_doubles_ampersands_in_the_shortcut_too() {
    assert_eq!(display_text(&item("A&B", Some("Ctrl+&"), true, false)), "A&&B\tCtrl+&&");
    assert_eq!(display_text(&item("&&", Some("&&"), true, false)), "&&&&\t&&&&");
}

// C#: ItemFlags_EnabledUnchecked_IsPlainString
#[test]
fn item_flags_enabled_unchecked_is_plain_string() {
    assert_eq!(item_flags(&item("a", None, true, false)), MF_STRING);
}

// C#: ItemFlags_DisabledAndChecked_SetsGrayedAndChecked
#[test]
fn item_flags_disabled_and_checked_sets_grayed_and_checked() {
    assert_eq!(item_flags(&item("a", None, false, true)), MF_GRAYED | MF_CHECKED);
}

// new
#[test]
fn item_flags_each_flag_alone() {
    assert_eq!(item_flags(&item("a", None, false, false)), MF_GRAYED);
    assert_eq!(item_flags(&item("a", None, true, true)), MF_CHECKED);
}

// new
#[test]
fn item_flags_never_set_mf_disabled() {
    for enabled in [true, false] {
        for checked in [true, false] {
            let flags = item_flags(&item("a", None, enabled, checked));
            assert_eq!(flags & MF_DISABLED, 0, "enabled {enabled} checked {checked}");
        }
    }
    // the C# values: MF_STRING 0, MF_GRAYED 1, MF_CHECKED 8
    assert_eq!((MF_STRING, MF_GRAYED, MF_CHECKED), (0, 1, 8));
}

// C#: IdForCommand_MapsOneBasedCommandToId
#[test]
fn id_for_command_maps_one_based_command_to_id() {
    let ids = vec!["first".to_owned(), "second".to_owned()];
    assert_eq!(id_for_command(&ids, 0), None);
    assert_eq!(id_for_command(&ids, 1), Some("first"));
    assert_eq!(id_for_command(&ids, 2), Some("second"));
    assert_eq!(id_for_command(&ids, 3), None);
    assert_eq!(id_for_command(&ids, -1), None);
}

// new
#[test]
fn id_for_command_boundaries() {
    let empty: Vec<String> = Vec::new();
    assert_eq!(id_for_command(&empty, 1), None);
    assert_eq!(id_for_command(&empty, 0), None);
    let ids: Vec<String> = (1..=5).map(|n| format!("id{n}")).collect();
    assert_eq!(id_for_command(&ids, 0), None);
    assert_eq!(id_for_command(&ids, 1), Some("id1"));
    assert_eq!(id_for_command(&ids, 5), Some("id5"));
    assert_eq!(id_for_command(&ids, 6), None);
    assert_eq!(id_for_command(&ids, i32::MIN), None);
    assert_eq!(id_for_command(&ids, i32::MAX), None);
    assert_eq!(id_for_command(&ids, -5), None);
}

// ---------------------------------------------------------------------------------------------------------------------
// owner_draw_style
// ---------------------------------------------------------------------------------------------------------------------

fn luma(color_ref: u32) -> f64 {
    let r = f64::from(color_ref & 0xFF);
    let g = f64::from((color_ref >> 8) & 0xFF);
    let b = f64::from((color_ref >> 16) & 0xFF);
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0
}

// C#: Background_IsDark_AndNormalTextIsLightWithStrongContrast
#[test]
fn background_is_dark_and_normal_text_is_light_with_strong_contrast() {
    assert!(luma(background()) < 0.2);
    assert!(luma(item_text(0)) > 0.8);
    assert!(luma(item_text(0)) - luma(item_background(0)) > 0.6);
}

// C#: ColorRef_IsBlueGreenRedOrder
#[test]
fn color_ref_is_blue_green_red_order() {
    // Dark.Accent = #3A7BD5: R=3A G=7B B=D5 => 0xD57B3A.
    assert_eq!(color_ref("Dark.Accent"), 0x00D5_7B3A);
}

// C#: SelectedItem_UsesALighterBackgroundThanTheMenu_AndKeepsTextReadable
#[test]
fn selected_item_uses_a_lighter_background_than_the_menu_and_keeps_text_readable() {
    let hover = item_background(ODS_SELECTED);

    assert_ne!(background(), hover);
    assert!(luma(hover) > luma(background()));
    assert!(luma(item_text(ODS_SELECTED)) - luma(hover) > 0.4);
}

// C#: DisabledItem_IsDimmerThanNormalButStillVisibleOnTheBackground (theory)
#[test]
fn disabled_item_is_dimmer_than_normal_but_still_visible_on_the_background() {
    for state in [ODS_GRAYED, ODS_DISABLED, ODS_GRAYED | ODS_SELECTED] {
        let text = luma(item_text(state));

        assert!(text < luma(item_text(0)), "state {state}");
        assert!(text - luma(item_background(state)) > 0.15, "state {state}");
    }
}

// C#: Separator_IsVisibleAgainstTheBackground
#[test]
fn separator_is_visible_against_the_background() {
    assert!(luma(separator()) > luma(background()));
}

// C#: Scale_FollowsDpi_AndNeverReturnsLessThanOnePixel (theory)
#[test]
fn scale_follows_dpi_and_never_returns_less_than_one_pixel() {
    for (px96, dpi, expected) in [
        (28, 96, 28),
        (28, 120, 35),
        (28, 144, 42),
        (28, 192, 56),
        (28, 0, 28),
        (0, 96, 1),
    ] {
        assert_eq!(scale(px96, dpi), expected, "{px96} at {dpi}");
    }
}

// new
#[test]
fn scale_rounds_ties_to_even_and_floors_at_one() {
    // dpi 48 = scale 0.5 exactly: .5 results round to the even neighbour (C# Math.Round default)
    assert_eq!(scale(1, 48), 1); // 0.5 -> 0 -> floor 1
    assert_eq!(scale(3, 48), 2); // 1.5 -> 2
    assert_eq!(scale(5, 48), 2); // 2.5 -> 2 (away from zero would be 3)
    assert_eq!(scale(7, 48), 4); // 3.5 -> 4
    assert_eq!(scale(9, 48), 4); // 4.5 -> 4
    assert_eq!(scale(-5, 96), 1); // negative sizes floor at one
    assert_eq!(scale(10, 96), 10);
    assert_eq!(scale(10, 1), 1); // 0.104 -> 0 -> 1
    assert_eq!(scale(1000, 0), 1000); // dpi 0 counts as 96
}

// C#: ItemHeight_IsTextPlusPadding_WithATouchFriendlyFloor (theory)
#[test]
fn item_height_is_text_plus_padding_with_a_touch_friendly_floor() {
    for (text_height, dpi, expected) in [(16, 96, 28), (24, 96, 34), (16, 192, 56)] {
        assert_eq!(item_height(text_height, dpi), expected, "{text_height} at {dpi}");
    }
}

// new: the floor and the padding meet at a text height of 18
#[test]
fn item_height_floor_boundary() {
    assert_eq!(item_height(17, 96), 28);
    assert_eq!(item_height(18, 96), 28);
    assert_eq!(item_height(19, 96), 29);
    assert_eq!(item_height(0, 96), 28);
    assert_eq!(item_height(30, 192), 56); // floor 2 * 28 beats 30 + 20
    assert_eq!(item_height(40, 192), 40 + 20);
}

// C#: ItemWidth_AddsGutterShortcutGapAndArrowPad_OnlyWhenThereIsAShortcut
#[test]
fn item_width_adds_gutter_shortcut_gap_and_arrow_pad_only_when_there_is_a_shortcut() {
    assert_eq!(item_width(100, 0, 96), 32 + 100 + 32);
    assert_eq!(item_width(100, 40, 96), 32 + 100 + 32 + 40 + 32);
    assert_eq!(item_width(200, 0, 192), 64 + 200 + 64);
    // a negative shortcut width counts as none
    assert_eq!(item_width(100, -5, 96), 32 + 100 + 32);
    assert_eq!(item_width(100, 1, 96), 32 + 100 + 32 + 1 + 32);
}

// C#: SeparatorHeight_IsShorterThanAnItem
#[test]
fn separator_height_is_shorter_than_an_item() {
    assert_eq!(separator_height(96), 9);
    assert!(separator_height(96) < item_height(0, 96));
    assert_eq!(separator_height(192), 18);
}

// new
#[test]
fn gutter_and_pad_scale_with_the_dpi() {
    assert_eq!((left_gutter(96), right_pad(96)), (32, 16));
    assert_eq!((left_gutter(192), right_pad(192)), (64, 32));
    assert_eq!((left_gutter(0), right_pad(0)), (32, 16));
}

// new: exact COLORREF values of the five keys the menu uses (0x00BBGGRR; the palette greys have equal channels)
#[test]
fn color_ref_table_for_the_five_dark_keys() {
    assert_eq!(color_ref("Dark.Panel"), 0x001E_1E1E);
    assert_eq!(color_ref("Dark.Border"), 0x003C_3C3C);
    assert_eq!(color_ref("Dark.ButtonPressed"), 0x0044_4444);
    assert_eq!(color_ref("Dark.TextFaint"), 0x0077_7777);
    assert_eq!(color_ref("Dark.Text"), 0x00E5_E5E5);

    assert_eq!(background(), 0x001E_1E1E);
    assert_eq!(separator(), 0x003C_3C3C);
    assert_eq!(item_background(0), 0x001E_1E1E);
    assert_eq!(item_background(ODS_SELECTED), 0x0044_4444);
    assert_eq!(item_background(ODS_SELECTED | ODS_CHECKED), 0x0044_4444);
    assert_eq!(item_background(ODS_CHECKED), 0x001E_1E1E);
    assert_eq!(item_text(0), 0x00E5_E5E5);
    assert_eq!(item_text(ODS_SELECTED), 0x00E5_E5E5);
    assert_eq!(item_text(ODS_CHECKED), 0x00E5_E5E5);
    assert_eq!(item_text(ODS_GRAYED), 0x0077_7777);
    assert_eq!(item_text(ODS_DISABLED), 0x0077_7777);
    assert_eq!(item_text(ODS_GRAYED | ODS_DISABLED), 0x0077_7777);
}

// new: every key of the palette converts as (round(f32 * 255) per channel), blue in the high byte; the alpha is dropped
#[test]
fn color_ref_converts_every_palette_key() {
    for (key, argb) in palette::TABLE {
        let r = (argb >> 16) & 0xFF;
        let g = (argb >> 8) & 0xFF;
        let b = argb & 0xFF;
        assert_eq!(color_ref(key), r | (g << 8) | (b << 16), "{key}");
    }
}

// new: an unknown key is 0, never a panic
#[test]
fn color_ref_of_an_unknown_key_is_zero() {
    assert_eq!(color_ref("Dark.NoSuchKey"), 0);
    assert_eq!(color_ref(""), 0);
    assert_eq!(color_ref("dark.panel"), 0); // ordinal, case-sensitive
}

// new: the ODS values are the DRAWITEMSTRUCT.itemState bits
#[test]
fn ods_flags_are_the_win32_bits() {
    assert_eq!((ODS_SELECTED, ODS_GRAYED, ODS_DISABLED, ODS_CHECKED), (1, 2, 4, 8));
}
