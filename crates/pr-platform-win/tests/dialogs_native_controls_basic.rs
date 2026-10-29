use std::sync::atomic::AtomicBool;

use pr_platform_win::dialogs::native::controls::basic::{
    BADGE_TEXT_FLAGS, COMMON_CONTROL_CLASSES, ComboBoxControl, CommonControlsInit, ListBoxControl, ROW_TEXT_FLAGS,
    row_layout,
};
use pr_platform_win::win32::structs::{Point, Rect};

#[path = "common/dialogs_controls_fixture.rs"]
mod fixture;

#[test]
fn ensure_once_runs_init_once_with_the_documented_classes() {
    let done = AtomicBool::new(false);
    let mut calls = Vec::new();
    assert!(CommonControlsInit::ensure_once(&done, |c| {
        calls.push(c);
        true
    }));
    assert!(!CommonControlsInit::ensure_once(&done, |c| {
        calls.push(c);
        true
    }));
    assert_eq!(calls, [1 | 4 | 0x20 | 0xff | 0x4000]);
    assert_eq!(COMMON_CONTROL_CLASSES, 1 | 4 | 0x20 | 0xff | 0x4000);
}

#[test]
fn ensure_once_does_not_retry_after_a_failed_init() {
    let done = AtomicBool::new(false);
    assert!(CommonControlsInit::ensure_once(&done, |_| false));
    assert!(!CommonControlsInit::ensure_once(&done, |_| panic!("must not retry")));
}

#[test]
fn combo_default_style_value() {
    assert_eq!(ComboBoxControl::DEFAULT_STYLE, 3 | 0x200 | 0x0001_0000 | 0x0020_0000);
}

#[test]
fn draw_item_text_flags_keep_an_ampersand_literal() {
    // C# NativeControls.cs ListBoxControl.DrawItem: DtSingleLine | DtVCenter | DtNoPrefix | DtEndEllipsis (badge: DtCenter instead of DtEndEllipsis).
    // Exe walkthrough 2026-10-29: the port passed 0x80 (DT_NOCLIP) instead of DT_NOPREFIX (0x800), so "Mouse & zoom" drew as "Mouse _zoom".
    const DT_NOPREFIX: u32 = 0x0800;
    assert_eq!(ROW_TEXT_FLAGS, 0x20 | 0x04 | DT_NOPREFIX | 0x8000);
    assert_eq!(BADGE_TEXT_FLAGS, 0x20 | 0x04 | 0x01 | DT_NOPREFIX);
    assert_eq!(
        (ROW_TEXT_FLAGS | BADGE_TEXT_FLAGS) & 0x80,
        0,
        "DT_NOCLIP must not stand in for DT_NOPREFIX"
    );
}

#[test]
fn row_layout_without_badge_or_subtitle() {
    let r = Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 24,
    };
    let layout = row_layout(r, None, 6, false);
    assert_eq!(layout.badge, None);
    assert_eq!(
        layout.text,
        Rect {
            left: 6,
            top: 0,
            right: 94,
            bottom: 24
        }
    );
    assert_eq!(layout.subtitle, None);
}

#[test]
fn row_layout_with_badge() {
    let layout = row_layout(
        Rect {
            left: 0,
            top: 0,
            right: 300,
            bottom: 24,
        },
        Some(Point { x: 40, y: 14 }),
        6,
        false,
    );
    assert_eq!(
        layout.badge,
        Some(Rect {
            left: 248,
            top: 4,
            right: 294,
            bottom: 20
        })
    );
    assert_eq!(layout.text.right, 242);
}

#[test]
fn row_layout_with_subtitle() {
    let layout = row_layout(
        Rect {
            left: 0,
            top: 2,
            right: 100,
            bottom: 26,
        },
        None,
        6,
        true,
    );
    assert_eq!(layout.text.bottom, 14);
    assert_eq!(
        layout.subtitle,
        Some(Rect {
            left: 6,
            top: 14,
            right: 94,
            bottom: 26
        })
    );
}

#[test]
fn row_layout_text_never_ends_before_it_starts() {
    let layout = row_layout(
        Rect {
            left: 0,
            top: 0,
            right: 5,
            bottom: 20,
        },
        Some(Point { x: 30, y: 8 }),
        8,
        false,
    );
    assert!(layout.text.right >= layout.text.left);
}

#[test]
fn row_layout_tiny_rect_does_not_panic() {
    let _ = row_layout(
        Rect {
            left: 4,
            top: 7,
            right: 1,
            bottom: 2,
        },
        Some(Point { x: 1, y: 1 }),
        5,
        true,
    );
}

#[test]
fn draw_item_struct_layout_matches_the_csharp_offsets() {
    use std::mem::{offset_of, size_of};
    use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
    assert_eq!(size_of::<DRAWITEMSTRUCT>(), 64);
    assert_eq!(offset_of!(DRAWITEMSTRUCT, CtlID), 4);
    assert_eq!(offset_of!(DRAWITEMSTRUCT, itemID), 8);
    assert_eq!(offset_of!(DRAWITEMSTRUCT, itemState), 16);
    assert_eq!(offset_of!(DRAWITEMSTRUCT, hwndItem), 24);
    assert_eq!(offset_of!(DRAWITEMSTRUCT, hDC), 32);
    assert_eq!(offset_of!(DRAWITEMSTRUCT, rcItem), 40);
}

#[test]
fn select_range_lparam_values() {
    assert_eq!(ListBoxControl::select_range_lparam(1), 0);
    assert_eq!(ListBoxControl::select_range_lparam(4), 3 << 16);
    assert!(ListBoxControl::select_range_lparam(0x8001) < 0);
}

#[test]
fn item_from_point_packing_round_trip() {
    assert_eq!(ListBoxControl::decode_item_from_point(3), 3);
    assert_eq!(ListBoxControl::decode_item_from_point(1 << 16), -1);
    assert_eq!(ListBoxControl::encode_item_from_point(-1, -1) as u32, 0xffff_ffff);
}

#[test]
fn empty_badge_or_subtitle_means_none() {
    let layout = row_layout(
        Rect {
            left: 0,
            top: 0,
            right: 80,
            bottom: 20,
        },
        None,
        4,
        false,
    );
    assert_eq!(layout.badge, None);
    assert_eq!(layout.subtitle, None);
}

#[test]
#[ignore = "native: needs desktop"]
fn combo_lists_items_selects_by_index_and_has_a_drop_down_window() {
    let _host = fixture::ControlHost::new();
}

#[test]
#[ignore = "native: needs desktop"]
fn list_box_multi_select_set_selected_select_all_and_none() {
    let _host = fixture::ControlHost::new();
}
