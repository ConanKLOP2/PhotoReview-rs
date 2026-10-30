//! Focused mutation guards for NativeDialogChromePainter at C# baseline 7eaee2a2.
//! Pure inputs only: no HWND, DC, user data, clock or filesystem is needed.

use pr_platform_win::dialogs::native::chrome::DialogColors;
use pr_platform_win::dialogs::native::dialog::events::{
    CommandRoute, class_name_is, command_route, split_command_wparam,
};
use pr_platform_win::dialogs::native::painter::geometry::{adjust_custom_draw, color, keys};

#[test]
fn selected_item_already_requesting_new_font_preserves_all_result_flags() {
    // C#: NativeDialogChromePainter.AdjustListViewCustomDraw: result | CdrfNewFont.
    // Mutant geometry.rs:413:28, replace | with ^: an existing NEWFONT must not be toggled off.
    for stage in [0x10001, 0x30001] {
        for result in [2, 34, 6, 0x402, -1] {
            let adjusted = adjust_custom_draw(stage, result, || true);
            assert_eq!(adjusted.result, result, "stage={stage:#x}, result={result:#x}");
            assert!(adjusted.clear_selected_flag);
            assert_eq!(adjusted.text_color, Some(color(keys::TEXT_STRONG)));
            assert_eq!(adjusted.back_color, Some(color(keys::SELECTION)));
        }
    }
}

#[test]
fn unrelated_stage_bits_do_not_query_selection_or_change_result() {
    // C#: NativeDialogChromePainter.AdjustListViewCustomDraw: ITEM and low-word PREPAINT are both required.
    // Mutants geometry.rs:411:14/40, replace & with | or ^; :411:31/54, replace && with ||.
    for stage in [0, 0x20001, 0x10000, 0x10002, 0x10003, 0x30002, 0xffff_0001] {
        // The final stage has ITEM set and low word 1, so it is a valid item prepaint.
        if stage == 0xffff_0001 {
            let adjusted = adjust_custom_draw(stage, 2, || false);
            assert_eq!(adjusted.result, 2);
            assert!(!adjusted.clear_selected_flag);
            assert_eq!((adjusted.text_color, adjusted.back_color), (None, None));
        } else {
            let adjusted = adjust_custom_draw(stage, 0x402, || panic!("selection queried for {stage:#x}"));
            assert_eq!(adjusted.result, 0x402);
            assert!(!adjusted.clear_selected_flag);
            assert_eq!((adjusted.text_color, adjusted.back_color), (None, None));
        }
    }
}

#[test]
fn command_route_requires_clicked_notification_for_both_builtin_buttons() {
    // C#: NativeDialog.HandleCommand: only BN_CLICKED on IdOk / IdCancel takes a built-in route.
    // Mutants dialog/events.rs:58:21/26/32 and :60:28/33/39, == -> != and && -> ||.
    for (id, notification, expected) in [
        (1, 0, CommandRoute::Ok),
        (2, 0, CommandRoute::Cancel),
        (1, 1, CommandRoute::Other),
        (2, 1, CommandRoute::Other),
        (1, -1, CommandRoute::Other),
        (2, -1, CommandRoute::Other),
        (0, 0, CommandRoute::Other),
        (3, 0, CommandRoute::Other),
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
fn command_wparam_extracts_unsigned_words_and_discards_machine_high_bits() {
    // C#: NativeDialog.HandleMessage WM_COMMAND: LOWORD id and HIWORD notification.
    // Mutants dialog/events.rs:69:5 tuple replacements; :69:14/48 & -> |/^; :69:41 >> -> <<.
    for (raw, expected) in [
        (0, (0, 0)),
        (0x5678_1234, (0x1234, 0x5678)),
        (0xdead_beef_5678_1234, (0x1234, 0x5678)),
        (usize::MAX, (0xffff, 0xffff)),
        (0xffff_0000, (0, 0xffff)),
        (0x0000_ffff, (0xffff, 0)),
    ] {
        assert_eq!(split_command_wparam(raw), expected, "raw={raw:#x}");
    }
}

#[test]
fn class_name_matching_requires_the_complete_ascii_name() {
    // C#: NativeDialog.IsButton / IsEdit: complete class names, OrdinalIgnoreCase.
    // Mutants dialog/events.rs:74:5, replace class_name_is with true / false.
    for (actual, expected, matches) in [
        ("bUtToN", "Button", true),
        ("EDIT", "Edit", true),
        ("ButtonExtra", "Button", false),
        ("Butto", "Button", false),
        ("", "Edit", false),
    ] {
        assert_eq!(class_name_is(actual, expected), matches);
    }
}

#[test]
fn color_ref_preserves_overlapping_bits_of_unclamped_channels() {
    // C#: DialogColors.ToColorRef does not clamp: 2*255=510; 512*255=130560.
    // Mutants native/chrome.rs:88:20/40, replace | with ^: these channels overlap after shifting.
    assert_eq!(DialogColors::to_color_ref(2.0, 1.0, 0.0), 0x0000_fffe);
    // ^ binds more tightly than |: the right-hand mutant needs green/blue overlap.
    assert_eq!(DialogColors::to_color_ref(0.0, 512.0, 1.0), 0x01ff_0000);
}

#[test]
fn excessive_control_error_keeps_the_csharp_message() {
    // C#: DialogTemplate.Build throws ArgumentException("Too many controls for a dialog template.").
    // Mutant native/template.rs:88:9, replace TemplateError::fmt with Ok(Default::default()).
    let error = pr_platform_win::dialogs::native::template::TemplateError::TooManyControls { count: 65536 };
    assert_eq!(error.to_string(), "Too many controls for a dialog template.");
}

#[test]
fn extended_style_builder_keeps_the_requested_style_and_item_fields() {
    // C#: DialogItem with ExStyle used by DialogTemplate.Build for WS_EX_CLIENTEDGE.
    // Mutant native/template.rs:75:16, delete field ex_style (filter spill-over in the focused proof).
    use pr_platform_win::dialogs::native::template::{DialogControlClass, DialogItem};
    let item = DialogItem::new(DialogControlClass::Edit, 31, "value", 0x5001_0080).with_ex_style(0x200);
    assert_eq!(item.ex_style, 0x200);
    assert_eq!(item.class, DialogControlClass::Edit);
    assert_eq!(item.id, 31);
    assert_eq!(item.text, "value");
    assert_eq!(item.style, 0x5001_0080);
}

#[test]
fn dialog_prepaint_preserves_existing_notify_flag_and_respects_skip_default() {
    // C#: NativeDialogChromePainter.AdjustListViewCustomDraw: preserve flags; SKIPDEFAULT suppresses item requests.
    // Mutants geometry.rs:399:32/51 (& -> |/^, == -> !=), :400:20 (| -> &/^).
    for (result, expected) in [(0, 32), (32, 32), (34, 34), (4, 4), (36, 36), (-1, -1)] {
        let adjusted = adjust_custom_draw(1, result, || panic!("dialog prepaint queried selection"));
        assert_eq!(adjusted.result, expected);
        assert!(!adjusted.clear_selected_flag);
        assert_eq!((adjusted.text_color, adjusted.back_color), (None, None));
    }
}

#[test]
fn checkbox_focus_width_subtracts_the_text_origin_before_clamping() {
    // C#: DrawCheckOrRadio uses textRect.Right - textRect.Left before Math.Min.
    // Mutant painter/widgets.rs:230:38, replace - with +: addition overflows for a representable wide rectangle.
    // Original arithmetic remains in range; the literal focus rectangle is independent of the implementation.
    use pr_platform_win::dialogs::native::painter::widgets::{CheckInput, check_layout};
    let layout = check_layout(CheckInput {
        width: i32::MAX,
        height: 30,
        scale: 1.0,
        radio: false,
        enabled: true,
        hot: false,
        checked: false,
        focused: true,
        multiline: false,
        left_text: false,
        text_height: 12,
        text_width: 12,
        hide_focus: false,
        hide_accel: false,
    });
    let focus = layout.focus_rect.unwrap();
    assert_eq!((focus.left, focus.top, focus.right, focus.bottom), (18, 8, 33, 22));
}

#[test]
fn missing_palette_key_remains_visible_magenta() {
    // C#: Key delegates to DialogColors.FromKey; Rust's documented missing-key sentinel is magenta.
    // Mutants painter/geometry.rs:375:5, replace color with 0 / 1.
    assert_eq!(color("CX.Painter.Unknown.Key"), 0x00ff_00ff);
}
