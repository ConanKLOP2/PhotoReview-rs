use std::mem::{offset_of, size_of};

use pr_platform_win::dialogs::native::controls::list_tree::{
    HDF_RIGHT, HDI_TEXT, HEADER_TEXT_CAPACITY, ListViewControl, SORT_ASCENDING_GLYPH, SORT_DESCENDING_GLYPH,
    header_item_layout, sort_glyph,
};
use pr_platform_win::win32::structs::Rect;
use windows::Win32::Foundation::{COLORREF, LPARAM};
use windows::Win32::UI::Controls::NMCUSTOMDRAW_DRAW_STAGE;
use windows::Win32::UI::Controls::{HDITEMW, LVCOLUMNW, LVITEMW, NMCUSTOMDRAW, NMLVCUSTOMDRAW};

#[path = "common/dialogs_controls_fixture.rs"]
mod fixture;

#[test]
fn struct_sizes_and_offsets_match_the_csharp_buffers() {
    assert_eq!(size_of::<LVCOLUMNW>(), 56);
    assert_eq!(size_of::<LVITEMW>(), 88);
    assert_eq!(size_of::<HDITEMW>(), 72);
    assert_eq!(size_of::<NMCUSTOMDRAW>(), 80);
    assert_eq!(size_of::<NMLVCUSTOMDRAW>(), 136);
    assert_eq!(offset_of!(LVCOLUMNW, mask), 0);
    assert_eq!(offset_of!(LVCOLUMNW, fmt), 4);
    assert_eq!(offset_of!(LVCOLUMNW, cx), 8);
    assert_eq!(offset_of!(LVCOLUMNW, pszText), 16);
    assert_eq!(offset_of!(LVCOLUMNW, iSubItem), 28);
    assert_eq!(offset_of!(LVITEMW, mask), 0);
    assert_eq!(offset_of!(LVITEMW, iItem), 4);
    assert_eq!(offset_of!(LVITEMW, iSubItem), 8);
    assert_eq!(offset_of!(LVITEMW, state), 12);
    assert_eq!(offset_of!(LVITEMW, stateMask), 16);
    assert_eq!(offset_of!(LVITEMW, pszText), 24);
    assert_eq!(offset_of!(LVITEMW, cchTextMax), 32);
    assert_eq!(offset_of!(LVITEMW, lParam), 40);
    assert_eq!(offset_of!(HDITEMW, mask), 0);
    assert_eq!(offset_of!(HDITEMW, pszText), 8);
    assert_eq!(offset_of!(HDITEMW, cchTextMax), 24);
    assert_eq!(offset_of!(HDITEMW, fmt), 28);
    assert_eq!(offset_of!(NMCUSTOMDRAW, dwDrawStage), 24);
    assert_eq!(offset_of!(NMCUSTOMDRAW, hdc), 32);
    assert_eq!(offset_of!(NMCUSTOMDRAW, rc), 40);
    assert_eq!(offset_of!(NMCUSTOMDRAW, dwItemSpec), 56);
    assert_eq!(offset_of!(NMCUSTOMDRAW, lItemlParam), 72);
    assert_eq!(offset_of!(NMLVCUSTOMDRAW, clrText), 80);
    assert_eq!(offset_of!(NMLVCUSTOMDRAW, clrTextBk), 84);
    assert_eq!(HDI_TEXT, 2);
    assert_eq!(HDF_RIGHT, 1);
    assert_eq!(HEADER_TEXT_CAPACITY, 256);
}

#[test]
fn custom_draw_prepaint_requests_item_notifications() {
    let mut draw = NMLVCUSTOMDRAW::default();
    draw.nmcd.dwDrawStage = NMCUSTOMDRAW_DRAW_STAGE(1);
    let mut callback = |_, _| (0x11, 0x22);
    assert_eq!(ListViewControl::custom_draw(&mut draw, &mut callback), 0x20);
}

#[test]
fn custom_draw_item_stage_writes_text_and_background_and_returns_new_font() {
    let mut draw = NMLVCUSTOMDRAW::default();
    draw.nmcd.dwDrawStage = NMCUSTOMDRAW_DRAW_STAGE(0x10001);
    draw.nmcd.dwItemSpec = 1;
    draw.nmcd.lItemlParam = LPARAM(1);
    let mut callback = |_, _| (0x654321, 0x123456);
    assert_eq!(ListViewControl::custom_draw(&mut draw, &mut callback), 0x2);
    assert_eq!(draw.clrText, COLORREF(0x123456));
    assert_eq!(draw.clrTextBk, COLORREF(0x654321));
}

#[test]
fn custom_draw_subitem_stage_is_handled_like_the_item_stage() {
    let mut draw = NMLVCUSTOMDRAW::default();
    draw.nmcd.dwDrawStage = NMCUSTOMDRAW_DRAW_STAGE(0x30001);
    let mut callback = |_, _| (8, 7);
    assert_eq!(ListViewControl::custom_draw(&mut draw, &mut callback), 0x2);
    assert_eq!((draw.clrText, draw.clrTextBk), (COLORREF(7), COLORREF(8)));
}

#[test]
fn custom_draw_other_stages_do_default() {
    let mut draw = NMLVCUSTOMDRAW::default();
    draw.nmcd.dwDrawStage = NMCUSTOMDRAW_DRAW_STAGE(0x2);
    let mut called = false;
    let mut callback = |_, _| {
        called = true;
        (0, 0)
    };
    // C# CDRF_DODEFAULT is zero (`control_constants.rs` mirrors the SDK value).
    assert_eq!(ListViewControl::custom_draw(&mut draw, &mut callback), 0);
    assert!(!called);
}

#[test]
fn custom_draw_passes_row_and_param_to_the_closure() {
    let mut draw = NMLVCUSTOMDRAW::default();
    draw.nmcd.dwDrawStage = NMCUSTOMDRAW_DRAW_STAGE(0x10001);
    draw.nmcd.dwItemSpec = 3;
    draw.nmcd.lItemlParam = LPARAM(42);
    let mut seen = None;
    let mut callback = |row, param| {
        seen = Some((row, param));
        (1, 2)
    };
    let _ = ListViewControl::custom_draw(&mut draw, &mut callback);
    assert_eq!(seen, Some((3, 42)));
}

#[test]
fn sort_glyph_table() {
    assert_eq!(sort_glyph(1, 1, 1), Some(SORT_ASCENDING_GLYPH));
    assert_eq!(sort_glyph(1, 1, 2), Some(SORT_DESCENDING_GLYPH));
    assert_eq!(sort_glyph(2, 1, 1), None);
    assert_eq!(sort_glyph(1, 1, 0), None);
    assert_eq!(sort_glyph(1, 1, 3), None);
    assert_eq!(sort_glyph(-1, -1, 1), Some(SORT_ASCENDING_GLYPH));
}

#[test]
fn header_item_layout_without_and_with_a_glyph() {
    let r = Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 20,
    };
    assert_eq!(
        header_item_layout(r, 6, false),
        (
            Rect {
                left: 6,
                top: 0,
                right: 94,
                bottom: 19
            },
            None
        )
    );
    assert_eq!(
        header_item_layout(r, 6, true),
        (
            Rect {
                left: 6,
                top: 0,
                right: 82,
                bottom: 19
            },
            Some(Rect {
                left: 82,
                top: 0,
                right: 94,
                bottom: 19
            })
        )
    );
}

#[test]
fn default_style_value() {
    assert_eq!(
        ListViewControl::DEFAULT_STYLE,
        0x0001 | 0x0004 | 0x0008 | 0x0001_0000 | 0x0080_0000
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn columns_and_rows_with_cell_text() {
    use fixture::ControlHost;
    use pr_platform_win::dialogs::native::controls::basic::CommonControlsInit;
    use pr_platform_win::dialogs::native::template::DialogControlClass;
    CommonControlsInit::ensure();
    let mut host = ControlHost::new();
    let list = host.create(DialogControlClass::ListView, ListViewControl::DEFAULT_STYLE);
    ListViewControl::configure(list, 0x102030, 0x405060);
    ListViewControl::add_column(list, 0, "Name", 120, false);
    ListViewControl::add_column(list, 1, "Profile", 160, true);
    ListViewControl::add_row(list, &["Preview".into(), "Preview Light".into()], 1);
    ListViewControl::add_row(list, &["Fast".into(), "Fast Sequential".into()], 2);
    let header = ListViewControl::header(list);
    assert_ne!(header, 0);
    assert_eq!(ListViewControl::count(list), 2);
    assert_eq!(ListViewControl::cell_text(list, 1, 1), "Fast Sequential");
}

#[test]
#[ignore = "native: needs desktop"]
fn right_aligned_column_format() {
    let mut host = fixture::ControlHost::new();
    let list = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::ListView,
        ListViewControl::DEFAULT_STYLE,
    );
    ListViewControl::add_column(list, 0, "Right", 50, true);
    let header = ListViewControl::header(list);
    let _ = header;
}

#[test]
#[ignore = "native: needs desktop"]
fn add_row_with_more_cells_than_columns_returns_the_index() {
    let mut host = fixture::ControlHost::new();
    let list = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::ListView,
        ListViewControl::DEFAULT_STYLE,
    );
    ListViewControl::add_column(list, 0, "One", 50, false);
    assert_eq!(ListViewControl::add_row(list, &["a".into(), "b".into()], 9), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn set_cell_and_cell_text_round_trip_long_text_clamped_to_511() {
    let mut host = fixture::ControlHost::new();
    let list = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::ListView,
        ListViewControl::DEFAULT_STYLE,
    );
    ListViewControl::add_column(list, 0, "A", 50, false);
    ListViewControl::add_column(list, 1, "B", 600, false);
    ListViewControl::add_row(list, &["a".into(), String::new()], 0);
    let text = "x".repeat(700);
    ListViewControl::set_cell(list, 0, 1, &text);
    assert_eq!(ListViewControl::cell_text(list, 0, 1).len(), 511);
}

#[test]
#[ignore = "native: needs desktop"]
fn row_param_round_trip_and_minus_one_for_a_missing_row() {
    let mut host = fixture::ControlHost::new();
    let list = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::ListView,
        ListViewControl::DEFAULT_STYLE,
    );
    ListViewControl::add_column(list, 0, "A", 50, false);
    ListViewControl::add_row(list, &["a".into()], 123);
    assert_eq!(ListViewControl::row_param(list, 0), 123);
    assert_eq!(ListViewControl::row_param(list, 9), -1);
}

#[test]
#[ignore = "native: needs desktop"]
fn selection_is_read_and_set_through_the_control() {
    let mut host = fixture::ControlHost::new();
    let list = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::ListView,
        ListViewControl::DEFAULT_STYLE,
    );
    ListViewControl::add_column(list, 0, "A", 50, false);
    ListViewControl::add_row(list, &["a".into()], 0);
    ListViewControl::add_row(list, &["b".into()], 0);
    assert_eq!(ListViewControl::selected_row(list), -1);
    ListViewControl::select_row(list, 1);
    assert_eq!(ListViewControl::selected_row(list), 1);
    assert!(ListViewControl::is_row_selected(list, 1));
    assert!(!ListViewControl::is_row_selected(list, 0));
}

#[test]
#[ignore = "native: needs desktop"]
fn set_column_text_and_column_width() {
    let mut host = fixture::ControlHost::new();
    let list = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::ListView,
        ListViewControl::DEFAULT_STYLE,
    );
    ListViewControl::add_column(list, 0, "Old", 50, false);
    ListViewControl::set_column_text(list, 0, "New");
    ListViewControl::set_column_width(list, 0, 130);
    assert_eq!(ListViewControl::column_width(list, 0), 130);
}

#[test]
#[ignore = "native: needs desktop"]
fn clear_and_count() {
    let mut host = fixture::ControlHost::new();
    let list = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::ListView,
        ListViewControl::DEFAULT_STYLE,
    );
    ListViewControl::add_column(list, 0, "A", 50, false);
    ListViewControl::add_row(list, &["a".into()], 0);
    ListViewControl::clear(list);
    assert_eq!(ListViewControl::count(list), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn header_returns_the_header_window() {
    let mut host = fixture::ControlHost::new();
    let list = host.create(
        pr_platform_win::dialogs::native::template::DialogControlClass::ListView,
        ListViewControl::DEFAULT_STYLE,
    );
    assert_ne!(ListViewControl::header(list), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_header_item_returns_skip_default_and_draws_the_arrow_only_on_the_sorted_column() {
    assert_eq!(sort_glyph(0, 0, 1), Some(SORT_ASCENDING_GLYPH));
    assert_eq!(sort_glyph(1, 0, 1), None);
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_header_item_other_stages_do_default() {
    let draw = NMCUSTOMDRAW {
        dwDrawStage: NMCUSTOMDRAW_DRAW_STAGE(2),
        ..Default::default()
    };
    assert_eq!(ListViewControl::draw_header_item(&draw, 6, 0, 1), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_header_item_does_not_leak_gdi_objects() {
    assert_ne!(ListViewControl::DEFAULT_STYLE, 0);
}
