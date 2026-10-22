//! Unit W8-18l, pure part (contract `contracts/W8-18l.md`): the classification, the predicates, the message table, the scale and colour helpers and the
//! custom-draw decision of `painter::geometry`, compared with the oracle golden `painter-kinds` (C# `NativeDialogChromePainter`) and with hand-written rows.
//! No window is created here; the native twins are in `dialogs_painter_attach.rs`.

use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::painter::geometry::{
    ChromePaintKind as K, ControlMessageAction as A, MISSING_COLOR, adjust_custom_draw, color, control_message_action,
    has_flat_border, is_full_paint, keys, kind_for_style, paint_scale, repaints_after_system_paint, scaled,
    tracks_hover, ui_state_from_flags,
};
use pr_platform_win::dialogs::native::painter::{CONTROL_PROP, CONTROL_SUBCLASS_ID, DIALOG_PROP, DIALOG_SUBCLASS_ID};
use pr_platform_win::dialogs::native::template::DialogControlClass as C;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};

const ALL_KINDS: [K; 13] = [
    K::None,
    K::PushButton,
    K::CheckBox,
    K::RadioButton,
    K::GroupBox,
    K::ComboBox,
    K::Progress,
    K::Trackbar,
    K::EtchedStatic,
    K::EditBorder,
    K::ListBoxBorder,
    K::ListViewBorder,
    K::HeaderFiller,
];

fn class_of(name: &str) -> C {
    match name {
        "Button" => C::Button,
        "Edit" => C::Edit,
        "Static" => C::Static,
        "ListBox" => C::ListBox,
        "ScrollBar" => C::ScrollBar,
        "ComboBox" => C::ComboBox,
        "ListView" => C::ListView,
        "Progress" => C::Progress,
        "Trackbar" => C::Trackbar,
        other => panic!("unknown class {other}"),
    }
}

fn kind_of(value: i64) -> K {
    K::from_value(i32::try_from(value).expect("kind value fits")).unwrap_or_else(|| panic!("kind {value}"))
}

// ---- golden --------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn golden_painter_kinds_matches_csharp() {
    // C#: NativeDialogChromePainter.KindFor / IsFullPaint / HasFlatBorder / TracksHover / RepaintsAfterSystemPaint / S / Key / AdjustListViewCustomDraw / constants
    let golden = golden::load("painter-kinds");
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for case in &golden.cases {
        let name = str_of(case, "name");
        let kind = str_of(case, "kind");
        *counts.entry(kind.to_owned()).or_default() += 1;
        match kind {
            "kind_for" => {
                // The oracle reports the style the real window had (`style_read`): that is what `KindFor` saw.
                let style = u32::try_from(i64_of(case, "style_read")).expect("style_read is a u32");
                let actual = kind_for_style(class_of(str_of(case, "class")), style);
                assert_eq!(i64::from(actual.value()), i64_of(case, "value"), "{name}");
            }
            "predicates" => {
                let k = kind_of(i64_of(case, "paint_kind"));
                assert_eq!(
                    is_full_paint(k),
                    bool_of(case, "is_full_paint"),
                    "{name}: is_full_paint"
                );
                assert_eq!(
                    has_flat_border(k),
                    bool_of(case, "has_flat_border"),
                    "{name}: has_flat_border"
                );
                assert_eq!(tracks_hover(k), bool_of(case, "tracks_hover"), "{name}: tracks_hover");
            }
            "repaints" => {
                let k = kind_of(i64_of(case, "paint_kind"));
                let message = u32::try_from(i64_of(case, "message")).expect("message is a u32");
                assert_eq!(
                    repaints_after_system_paint(k, message),
                    bool_of(case, "value"),
                    "{name}"
                );
            }
            "scaled" => {
                let scale = case["scale"].as_f64().expect("scale");
                let px = i32::try_from(i64_of(case, "px")).expect("px");
                assert_eq!(i64::from(scaled(scale, px)), i64_of(case, "value"), "{name}");
            }
            "key_color" => {
                let key = str_of(case, "key");
                if bool_of(case, "found") {
                    assert_eq!(i64::from(color(key)), i64_of(case, "value"), "{name}");
                } else {
                    // The C# throws KeyNotFoundException; Rust answers the magenta (D-W8B5-405).
                    assert_eq!(color(key), MISSING_COLOR, "{name}");
                }
            }
            "custom_draw" => {
                // hwndFrom = 0 in the oracle: the row is never selected.
                let stage = u32::try_from(i64_of(case, "stage")).expect("stage");
                let result_in = isize::try_from(i64_of(case, "result_in")).expect("result_in");
                let adjustment = adjust_custom_draw(stage, result_in, || false);
                assert_eq!(adjustment.result as i64, i64_of(case, "result"), "{name}: result");
                let item_state_in = u32::try_from(i64_of(case, "item_state_in")).expect("item_state_in");
                let item_state = if adjustment.clear_selected_flag {
                    item_state_in & !1
                } else {
                    item_state_in
                };
                assert_eq!(i64::from(item_state), i64_of(case, "item_state"), "{name}: item_state");
                assert_eq!(
                    i64::from(adjustment.text_color.unwrap_or(0)),
                    i64_of(case, "text_color"),
                    "{name}: text_color"
                );
                assert_eq!(
                    i64::from(adjustment.back_color.unwrap_or(0)),
                    i64_of(case, "back_color"),
                    "{name}: back_color"
                );
            }
            "constants" => {
                assert_eq!(CONTROL_SUBCLASS_ID as i64, i64_of(case, "control_subclass_id"));
                assert_eq!(DIALOG_SUBCLASS_ID as i64, i64_of(case, "dialog_subclass_id"));
                // The Rust prefix is `PhotoReviewRs.` instead of `PhotoReview.` (D-W8B5-206).
                assert_eq!(
                    CONTROL_PROP.replacen("PhotoReviewRs.", "PhotoReview.", 1),
                    str_of(case, "control_prop")
                );
                assert_eq!(
                    DIALOG_PROP.replacen("PhotoReviewRs.", "PhotoReview.", 1),
                    str_of(case, "dialog_prop")
                );
                let kinds = case["kinds"].as_object().expect("kinds object");
                assert_eq!(kinds.len(), ALL_KINDS.len());
                for (kind_name, value) in kinds {
                    let value = value.as_i64().expect("kind value");
                    let expected = kind_of(value);
                    assert_eq!(format!("{expected:?}"), *kind_name);
                    assert_eq!(i64::from(expected.value()), value);
                }
            }
            other => panic!("{name}: unknown golden kind {other}"),
        }
    }
    // The oracle reported 503 rows: 73 kind_for, 13 predicates, 297 repaints, 81 scaled, 18 key_color, 20 custom_draw, 1 constants.
    assert_eq!(golden.cases.len(), 503);
    assert_eq!(counts["kind_for"], 73);
    assert_eq!(counts["predicates"], 13);
    assert_eq!(counts["repaints"], 297);
    assert_eq!(counts["scaled"], 81);
    assert_eq!(counts["key_color"], 18);
    assert_eq!(counts["custom_draw"], 20);
    assert_eq!(counts["constants"], 1);
}

// ---- classification ----------------------------------------------------------------------------------------------------------------------------

#[test]
fn kind_for_style_follows_the_class_and_the_style() {
    // C#: NativeDialogChromeTests.KindFor_FollowsTheClassAndTheStyle (19 InlineData rows; the style here is the one the window would report).
    const BS_PUSHLIKE: u32 = 0x1000;
    const BS_ICON: u32 = 0x40;
    const BS_BITMAP: u32 = 0x80;
    let rows: [(C, u32, K); 19] = [
        (C::Button, 0, K::PushButton),
        (C::Button, 1, K::PushButton),
        (C::Button, 3, K::CheckBox),
        (C::Button, 9, K::RadioButton),
        (C::Button, 7, K::GroupBox),
        (C::Button, 3 | BS_PUSHLIKE, K::PushButton),
        (C::Button, 9 | BS_PUSHLIKE, K::PushButton),
        (C::Button, BS_ICON, K::None),
        (C::Button, BS_BITMAP | 1, K::None),
        (C::Button, 0x0B, K::None),
        (C::ComboBox, 3, K::ComboBox),
        (C::ComboBox, 2, K::None),
        (C::ComboBox, 3 | 0x10, K::None),
        (C::Edit, 0, K::EditBorder),
        (C::ListBox, 0, K::ListBoxBorder),
        (C::ListView, 0, K::ListViewBorder),
        (C::Progress, 0, K::Progress),
        (C::Trackbar, 0, K::Trackbar),
        (C::ScrollBar, 0, K::None),
    ];
    for (class, style, expected) in rows {
        assert_eq!(kind_for_style(class, style), expected, "{class:?} {style:#x}");
    }
}

#[test]
fn kind_for_style_button_types_match_the_c_switch() {
    // 0 and 1 push; 2, 3, 5, 6 check (push when push-like); 4 and 9 radio (push when push-like); 7 group; the rest none.
    let expected = [
        (0, K::PushButton, K::PushButton),
        (1, K::PushButton, K::PushButton),
        (2, K::CheckBox, K::PushButton),
        (3, K::CheckBox, K::PushButton),
        (4, K::RadioButton, K::PushButton),
        (5, K::CheckBox, K::PushButton),
        (6, K::CheckBox, K::PushButton),
        (7, K::GroupBox, K::GroupBox),
        (8, K::None, K::None),
        (9, K::RadioButton, K::PushButton),
        (10, K::None, K::None),
        (11, K::None, K::None),
        (12, K::None, K::None),
        (13, K::None, K::None),
        (14, K::None, K::None),
        (15, K::None, K::None),
    ];
    for (ty, plain, push_like) in expected {
        assert_eq!(kind_for_style(C::Button, ty), plain, "type {ty}");
        assert_eq!(kind_for_style(C::Button, ty | 0x1000), push_like, "type {ty} push-like");
        // BS_ICON and BS_BITMAP win over everything.
        assert_eq!(kind_for_style(C::Button, ty | 0x40), K::None, "type {ty} icon");
        assert_eq!(kind_for_style(C::Button, ty | 0x80), K::None, "type {ty} bitmap");
    }
}

#[test]
fn kind_for_style_other_classes() {
    // Combo: only the drop-down list (type 3) that is not owner drawn.
    assert_eq!(kind_for_style(C::ComboBox, 0x203), K::ComboBox);
    assert_eq!(kind_for_style(C::ComboBox, 0x13), K::None);
    assert_eq!(kind_for_style(C::ComboBox, 0x23), K::None);
    assert_eq!(kind_for_style(C::ComboBox, 0x33), K::None);
    assert_eq!(kind_for_style(C::ComboBox, 1), K::None);
    // Progress: marquee is excluded. Trackbar: vertical is excluded.
    assert_eq!(kind_for_style(C::Progress, 9), K::None);
    assert_eq!(kind_for_style(C::Progress, 1), K::Progress);
    assert_eq!(kind_for_style(C::Trackbar, 2), K::None);
    assert_eq!(kind_for_style(C::Trackbar, 0x100 | 1), K::Trackbar);
    // Static: only the three etched types.
    for (style, kind) in [
        (0x10, K::EtchedStatic),
        (0x11, K::EtchedStatic),
        (0x12, K::EtchedStatic),
        (0x13, K::None),
        (0x0E, K::None),
        (0, K::None),
        (0x200 | 0x10, K::EtchedStatic),
        (0x80, K::None),
    ] {
        assert_eq!(kind_for_style(C::Static, style), kind, "static {style:#x}");
    }
}

#[test]
fn chrome_paint_kind_from_value_round_trips() {
    for (value, kind) in (0..).zip(ALL_KINDS) {
        assert_eq!(kind.value(), value);
        assert_eq!(K::from_value(value), Some(kind));
    }
    assert_eq!(K::from_value(-1), None);
    assert_eq!(K::from_value(13), None);
    assert_eq!(K::from_value(i32::MAX), None);
    assert_eq!(K::from_value(i32::MIN), None);
}

#[test]
fn predicates_partition_the_kinds() {
    // C#: IsFullPaint = 1..=8, HasFlatBorder = 9..=11 or Progress, TracksHover = push, check, radio, combo, trackbar.
    for k in ALL_KINDS {
        assert_eq!(is_full_paint(k), (1..=8).contains(&k.value()), "{k:?} full");
        assert_eq!(
            has_flat_border(k),
            (9..=11).contains(&k.value()) || k == K::Progress,
            "{k:?} flat"
        );
        assert_eq!(
            tracks_hover(k),
            matches!(
                k,
                K::PushButton | K::CheckBox | K::RadioButton | K::ComboBox | K::Trackbar
            ),
            "{k:?} hover"
        );
    }
    assert!(!is_full_paint(K::HeaderFiller) && !has_flat_border(K::HeaderFiller) && !tracks_hover(K::HeaderFiller));
    assert!(!is_full_paint(K::None) && !has_flat_border(K::None) && !tracks_hover(K::None));
}

#[test]
fn repaints_after_system_paint_lists() {
    // C#: RepaintsAfterSystemPaint
    let combo = [
        0x7, 0x8, 0xA, 0x30, 0x1F, 0x100, 0x102, 0x201, 0x202, 0x203, 0x20A, 0x14B, 0x14D, 0x14E, 0x14F,
    ];
    let button = [0x7, 0x8, 0xA, 0xF1, 0xF3, 0xF4];
    for message in 0..0x400u32 {
        assert_eq!(
            repaints_after_system_paint(K::ComboBox, message),
            combo.contains(&message),
            "combo {message:#x}"
        );
        for k in [K::PushButton, K::CheckBox, K::RadioButton] {
            assert_eq!(
                repaints_after_system_paint(k, message),
                button.contains(&message),
                "{k:?} {message:#x}"
            );
        }
        for k in [
            K::None,
            K::GroupBox,
            K::Progress,
            K::Trackbar,
            K::EtchedStatic,
            K::EditBorder,
            K::ListBoxBorder,
            K::ListViewBorder,
            K::HeaderFiller,
        ] {
            assert!(!repaints_after_system_paint(k, message), "{k:?} {message:#x}");
        }
    }
}

// ---- the message table -------------------------------------------------------------------------------------------------------------------------

const WM_ERASEBKGND: u32 = 0x14;
const WM_PAINT: u32 = 0x0F;
const WM_PRINTCLIENT: u32 = 0x318;
const WM_MOUSEMOVE: u32 = 0x200;
const WM_MOUSELEAVE: u32 = 0x2A3;
const WM_NCPAINT: u32 = 0x85;
const WM_PRINT: u32 = 0x317;
const WM_UPDATEUISTATE: u32 = 0x128;
const WM_NCDESTROY: u32 = 0x82;

/// The C# `switch` written a second time, independently, as a plain `if` ladder (first match wins).
fn reference_action(kind: K, message: u32, lparam: isize) -> A {
    let full = (1..=8).contains(&kind.value());
    let hover = matches!(
        kind,
        K::PushButton | K::CheckBox | K::RadioButton | K::ComboBox | K::Trackbar
    );
    let flat = (9..=11).contains(&kind.value()) || kind == K::Progress;
    if message == WM_ERASEBKGND && full {
        A::EraseBackgroundHandled
    } else if message == WM_PAINT && full {
        A::PaintWindow
    } else if message == WM_PRINTCLIENT && full {
        A::PrintClient
    } else if message == WM_MOUSEMOVE && hover {
        A::TrackHover
    } else if message == WM_MOUSELEAVE && hover {
        A::LeaveHover
    } else if message == WM_NCPAINT && flat {
        A::PaintNonClientBorder
    } else if message == WM_PRINT && flat && (lparam as u32) & 2 != 0 {
        A::PrintNonClientBorder
    } else if message == WM_PAINT && kind == K::HeaderFiller {
        A::PaintHeaderFiller
    } else if message == WM_PRINTCLIENT && kind == K::HeaderFiller {
        A::PrintHeaderFiller
    } else if message == WM_UPDATEUISTATE && full {
        A::InvalidateAfterDefault
    } else if repaints_after_system_paint(kind, message) {
        A::RepaintAfterDefault
    } else if message == WM_NCDESTROY {
        A::Destroy
    } else {
        A::PassThrough
    }
}

#[test]
fn message_table_matches_the_c_switch() {
    // C#: ControlProc `switch (message)`
    let messages = [
        0, 0x7, 0x8, 0xA, 0xF, 0x14, 0x1F, 0x30, 0x82, 0x85, 0xF1, 0xF3, 0xF4, 0x100, 0x102, 0x128, 0x14B, 0x14D,
        0x14E, 0x14F, 0x200, 0x201, 0x202, 0x203, 0x20A, 0x2A3, 0x317, 0x318, 0x84, 0x1234,
    ];
    let lparams: [isize; 5] = [0, 1, 2, 3, 0x1_0000_0002];
    for kind in ALL_KINDS {
        for message in messages {
            for lparam in lparams {
                assert_eq!(
                    control_message_action(kind, message, lparam),
                    reference_action(kind, message, lparam),
                    "{kind:?} {message:#x} {lparam:#x}"
                );
            }
        }
    }
}

#[test]
fn message_table_named_rows() {
    // The consequences listed in the contract.
    assert_eq!(
        control_message_action(K::PushButton, WM_ERASEBKGND, 0),
        A::EraseBackgroundHandled
    );
    assert_eq!(control_message_action(K::EtchedStatic, WM_PAINT, 0), A::PaintWindow);
    assert_eq!(control_message_action(K::GroupBox, WM_PRINTCLIENT, 0), A::PrintClient);
    assert_eq!(control_message_action(K::Trackbar, WM_MOUSEMOVE, 0), A::TrackHover);
    assert_eq!(control_message_action(K::ComboBox, WM_MOUSELEAVE, 0), A::LeaveHover);
    // A progress bar is both full-paint (WM_PAINT) and flat-border (WM_NCPAINT).
    assert_eq!(control_message_action(K::Progress, WM_PAINT, 0), A::PaintWindow);
    assert_eq!(
        control_message_action(K::Progress, WM_NCPAINT, 0),
        A::PaintNonClientBorder
    );
    // The border kinds do not paint the client.
    assert_eq!(control_message_action(K::EditBorder, WM_PAINT, 0), A::PassThrough);
    assert_eq!(control_message_action(K::EditBorder, WM_ERASEBKGND, 0), A::PassThrough);
    assert_eq!(
        control_message_action(K::ListViewBorder, WM_NCPAINT, 0),
        A::PaintNonClientBorder
    );
    // WM_PRINT only with PRF_NONCLIENT in the LOW 32 bits of lParam.
    assert_eq!(control_message_action(K::EditBorder, WM_PRINT, 0), A::PassThrough);
    assert_eq!(control_message_action(K::EditBorder, WM_PRINT, 1), A::PassThrough);
    assert_eq!(
        control_message_action(K::EditBorder, WM_PRINT, 2),
        A::PrintNonClientBorder
    );
    assert_eq!(
        control_message_action(K::EditBorder, WM_PRINT, 3),
        A::PrintNonClientBorder
    );
    assert_eq!(
        control_message_action(K::EditBorder, WM_PRINT, 0x1_0000_0002),
        A::PrintNonClientBorder
    );
    assert_eq!(
        control_message_action(K::EditBorder, WM_PRINT, 0x1_0000_0000),
        A::PassThrough
    );
    // The header filler ignores WM_ERASEBKGND and WM_NCPAINT.
    assert_eq!(
        control_message_action(K::HeaderFiller, WM_ERASEBKGND, 0),
        A::PassThrough
    );
    assert_eq!(control_message_action(K::HeaderFiller, WM_NCPAINT, 0), A::PassThrough);
    assert_eq!(
        control_message_action(K::HeaderFiller, WM_PAINT, 0),
        A::PaintHeaderFiller
    );
    assert_eq!(
        control_message_action(K::HeaderFiller, WM_PRINTCLIENT, 0),
        A::PrintHeaderFiller
    );
    assert_eq!(
        control_message_action(K::CheckBox, WM_UPDATEUISTATE, 0),
        A::InvalidateAfterDefault
    );
    assert_eq!(
        control_message_action(K::EditBorder, WM_UPDATEUISTATE, 0),
        A::PassThrough
    );
    // A combo box repaints after focus and selection changes, but not for its other messages.
    assert_eq!(control_message_action(K::ComboBox, 0x7, 0), A::RepaintAfterDefault);
    assert_eq!(control_message_action(K::ComboBox, 0x14E, 0), A::RepaintAfterDefault);
    assert_eq!(control_message_action(K::ComboBox, 0x84, 0), A::PassThrough);
    // Every kind destroys at WM_NCDESTROY.
    for kind in ALL_KINDS {
        assert_eq!(control_message_action(kind, WM_NCDESTROY, 0), A::Destroy, "{kind:?}");
    }
}

// ---- scale, colours, custom draw -------------------------------------------------------------------------------------------------------------

#[test]
fn scaled_rounds_to_even_and_never_below_one() {
    // C#: S(scale, value) = Math.Max(1, (int)Math.Round(value * scale))
    assert_eq!(scaled(1.0, 0), 1);
    assert_eq!(scaled(1.0, -5), 1);
    assert_eq!(scaled(1.0, 13), 13);
    assert_eq!(scaled(1.5, 3), 4); // 4.5 -> 4 (to even)
    assert_eq!(scaled(1.5, 5), 8); // 7.5 -> 8
    assert_eq!(scaled(1.25, 6), 8); // 7.5 -> 8
    assert_eq!(scaled(1.25, 2), 2); // 2.5 -> 2
    assert_eq!(scaled(0.5, 3), 2); // 1.5 -> 2
    assert_eq!(scaled(2.0, 6), 12);
    assert_eq!(scaled(1.0, i32::MAX), i32::MAX);
    assert_eq!(scaled(1e12, i32::MAX), i32::MAX); // saturating
    assert_eq!(scaled(f64::NAN, 5), 1);
}

#[test]
fn paint_scale_never_below_one() {
    // C#: Math.Max(1.0, GetDpiForWindow(hwnd) / 96.0)
    assert_eq!(paint_scale(0), 1.0);
    assert_eq!(paint_scale(95), 1.0);
    assert_eq!(paint_scale(96), 1.0);
    assert_eq!(paint_scale(120), 1.25);
    assert_eq!(paint_scale(144), 1.5);
    assert_eq!(paint_scale(192), 2.0);
}

#[test]
fn ui_state_flags() {
    // C#: UiState(hwnd): (state & UisfHideFocus) != 0, (state & UisfHideAccel) != 0
    for raw in 0..8u32 {
        let state = ui_state_from_flags(raw);
        assert_eq!(state.hide_focus, raw & 1 != 0, "{raw}");
        assert_eq!(state.hide_accel, raw & 2 != 0, "{raw}");
    }
    assert_eq!(ui_state_from_flags(0xFFFF_FFFC), ui_state_from_flags(0));
}

#[test]
fn keys_all_exist_in_the_palette() {
    assert_eq!(keys::ALL.len(), 17);
    for key in keys::ALL {
        assert!(
            pr_app::overlay::palette::get(key).is_some(),
            "{key} is not in the palette"
        );
        assert_ne!(color(key), MISSING_COLOR, "{key}");
        assert_eq!(Some(color(key)), DialogColors::from_key(key), "{key}");
    }
    let unique: std::collections::BTreeSet<_> = keys::ALL.iter().collect();
    assert_eq!(unique.len(), 17);
}

#[test]
fn color_of_unknown_key_is_magenta() {
    assert_eq!(MISSING_COLOR, 0x00FF_00FF);
    assert_eq!(color("Dark.Nope"), MISSING_COLOR);
    assert_eq!(color(""), MISSING_COLOR);
}

#[test]
fn adjust_custom_draw_selected_row_gets_selection_colours() {
    // C#: AdjustListViewCustomDraw, the selected branch (the oracle cannot reach it: hwndFrom = 0).
    for stage in [0x10001u32, 0x30001] {
        let adjustment = adjust_custom_draw(stage, 0, || true);
        assert!(adjustment.clear_selected_flag, "{stage:#x}");
        assert_eq!(adjustment.text_color, Some(color(keys::TEXT_STRONG)), "{stage:#x}");
        assert_eq!(adjustment.back_color, Some(color(keys::SELECTION)), "{stage:#x}");
        assert_eq!(adjustment.result, 2, "{stage:#x}");
    }
    // The flags of the dialog's own answer are kept.
    assert_eq!(adjust_custom_draw(0x10001, 32, || true).result, 32 | 2);
    assert_eq!(adjust_custom_draw(0x10001, 4, || true).result, 4 | 2);
    // A row that is not selected is left alone.
    let plain = adjust_custom_draw(0x10001, 32, || false);
    assert_eq!(
        (
            plain.result,
            plain.clear_selected_flag,
            plain.text_color,
            plain.back_color
        ),
        (32, false, None, None)
    );
}

#[test]
fn adjust_custom_draw_asks_for_the_selection_only_for_item_prepaint() {
    // The closure must not run for other stages (C# queries LVM_GETITEMSTATE only in the item branch).
    for stage in [0u32, 1, 2, 4, 0x10000, 0x10002, 0x10004, 0x20001, 0x2, 0x8000_0001] {
        let adjustment = adjust_custom_draw(stage, 3, || panic!("selected() must not run for stage {stage:#x}"));
        assert!(!adjustment.clear_selected_flag, "{stage:#x}");
        assert_eq!(adjustment.text_color, None);
        assert_eq!(adjustment.back_color, None);
        if stage == 1 {
            assert_eq!(adjustment.result, 3 | 32);
        } else {
            assert_eq!(adjustment.result, 3, "{stage:#x}");
        }
    }
    // Item pre-paint asks exactly once.
    let mut asked = 0;
    let _ = adjust_custom_draw(0x10001, 0, || {
        asked += 1;
        false
    });
    assert_eq!(asked, 1);
}

#[test]
fn adjust_custom_draw_prepaint_requests_item_notifications_unless_skip_default() {
    // C#: stage == CddsPrePaint -> (result & CdrfSkipDefault) == 0 ? result | CdrfNotifyItemDraw : result
    assert_eq!(adjust_custom_draw(1, 0, || false).result, 32);
    assert_eq!(adjust_custom_draw(1, 2, || false).result, 34);
    assert_eq!(adjust_custom_draw(1, 4, || false).result, 4);
    assert_eq!(adjust_custom_draw(1, 6, || false).result, 6);
    assert_eq!(adjust_custom_draw(1, 32, || false).result, 32);
    assert_eq!(adjust_custom_draw(1, 36, || false).result, 36);
}
