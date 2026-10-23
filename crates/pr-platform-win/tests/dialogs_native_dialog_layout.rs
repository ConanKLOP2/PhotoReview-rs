//! Pure W8-18i layout and DPI contract tests, baseline d92d6aae.
use pr_platform_win::dialogs::native::dialog::{dpi, layout};
use pr_platform_win::win32::structs::Rect;
use pr_test_support::golden;

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

#[test]
fn dlu_matches_the_csharp_golden() {
    // C#: NativeDialog.Dx / Dy.
    let data = golden::load("dialog-layout");
    let rows: Vec<_> = data.cases.iter().filter(|row| row["kind"] == "dlu").collect();
    assert_eq!(rows.len(), 112);
    for row in rows {
        let name = golden::str_of(row, "name");
        let units = golden::i64_of(row, "units") as i32;
        let avg = golden::i64_of(row, "avg") as i32;
        let line = golden::i64_of(row, "line") as i32;
        assert_eq!(
            i64::from(dpi::scale_dlu_x(units, avg)),
            golden::i64_of(row, "dx"),
            "{name}"
        );
        assert_eq!(
            i64::from(dpi::scale_dlu_y(units, line)),
            golden::i64_of(row, "dy"),
            "{name}"
        );
    }
}

#[test]
fn button_row_matches_the_csharp_golden() {
    // C#: NativeDialog.ButtonWidth / ButtonRowWidth / PlaceButtonRow.
    let data = golden::load("dialog-layout");
    let rows: Vec<_> = data.cases.iter().filter(|row| row["kind"] == "button_row").collect();
    assert_eq!(rows.len(), 20);
    for row in rows {
        let name = golden::str_of(row, "name");
        let avg = golden::i64_of(row, "avg") as i32;
        let count = golden::i64_of(row, "count") as usize;
        let width = dpi::scale_dlu_x(50, avg);
        let gap = dpi::scale_dlu_x(6, avg);
        let total = layout::button_row_total(count, width, gap);
        assert_eq!(i64::from(total), golden::i64_of(row, "row_width"), "{name}");
        assert_eq!(i64::from(total), golden::i64_of(row, "placed"), "{name}");
        assert_eq!(
            i64::from(layout::button_width_for(&[], width, dpi::scale_dlu_x(16, avg))),
            golden::i64_of(row, "button_width"),
            "{name}"
        );
    }
}

#[test]
fn average_char_width_table() {
    // C#: NativeDialog.MeasureUnits.
    for (extent, expected) in [(0, 1), (25, 1), (26, 1), (52, 1), (311, 6), (338, 7), (400, 8)] {
        assert_eq!(dpi::average_char_width(extent), expected, "extent={extent}");
    }
}

#[test]
fn effective_dpi_table() {
    // C#: NativeDialog.DpiFor.
    for (window, system, expected) in [(0, 0, 96), (0, 144, 144), (0, 72, 96), (120, 0, 120), (96, 192, 96)] {
        assert_eq!(dpi::effective_dpi(window, system), expected);
    }
}

#[test]
fn fallback_font_height_table() {
    // C#: NativeDialog.CreateFonts fallback.
    for (input, expected) in [
        (72, -9),
        (96, -12),
        (100, -12),
        (108, -14),
        (120, -15),
        (144, -18),
        (168, -21),
        (192, -24),
        (0, 0),
    ] {
        assert_eq!(dpi::fallback_font_height(input), expected, "dpi={input}");
    }
}

#[test]
fn header_font_height_table() {
    // C#: NativeDialog.CreateFonts header.
    for (input, expected) in [(0, 0), (-9, -12), (-12, -16), (-16, -22)] {
        assert_eq!(dpi::header_font_height(input), expected);
    }
    assert_eq!(dpi::header_font_height(i32::MIN), i32::MIN.wrapping_mul(14) / 10);
}

#[test]
fn fallback_log_font_fields() {
    // C#: NativeDialog.CreateFonts fallback LOGFONT.
    let font = dpi::fallback_log_font(96);
    assert_eq!(font.lfHeight, -12);
    assert_eq!(font.lfWeight, 400);
    assert_eq!(font.lfCharSet.0, 1);
    assert_eq!(font.lfQuality.0, 5);
    let face: Vec<u16> = "Segoe UI".encode_utf16().collect();
    assert_eq!(&font.lfFaceName[..face.len()], face.as_slice());
    assert!(font.lfFaceName[face.len()..].iter().all(|unit| *unit == 0));
    assert_eq!(font.lfWidth, 0);
    assert_eq!(font.lfEscapement, 0);
    assert_eq!(font.lfOrientation, 0);
    assert_eq!(font.lfItalic, 0);
    assert_eq!(font.lfUnderline, 0);
    assert_eq!(font.lfStrikeOut, 0);
    assert_eq!(font.lfOutPrecision.0, 0);
    assert_eq!(font.lfClipPrecision.0, 0);
    assert_eq!(font.lfPitchAndFamily, 0);
}

#[test]
fn header_log_font_copies_everything_but_height_and_weight() {
    // C#: NativeDialog.CreateFonts header LOGFONT.
    let mut body = dpi::fallback_log_font(96);
    body.lfWidth = 7;
    body.lfItalic = 1;
    let header = dpi::header_log_font(&body);
    assert_eq!(header.lfHeight, -16);
    assert_eq!(header.lfWeight, 600);
    body.lfHeight = -16;
    body.lfWeight = 600;
    assert_eq!(format!("{header:?}"), format!("{body:?}"));
}

#[test]
fn button_width_for_grows_with_the_widest_text() {
    // C#: NativeDialog.ButtonWidth.
    assert_eq!(layout::button_width_for(&[10, 80, 30], 50, 16), 96);
    assert_eq!(layout::button_width_for(&[], 50, 16), 50);
    assert_eq!(layout::button_width_for(&[5], 50, 16), 50);
}

#[test]
fn button_row_positions_are_right_aligned() {
    // C#: NativeDialog.PlaceButtonRow.
    assert_eq!(layout::button_row_positions(600, 10, 3, 80, 8), [334, 422, 510]);
    assert_eq!(layout::button_row_positions(600, 10, 1, 80, 8), [510]);
    assert!(layout::button_row_positions(600, 10, 0, 80, 8).is_empty());
}

#[test]
fn button_row_total_table() {
    // C#: NativeDialog.ButtonRowWidth.
    for (count, expected) in [(3, 256), (1, 80), (0, -8)] {
        assert_eq!(layout::button_row_total(count, 80, 8), expected);
    }
}

#[test]
fn work_area_falls_back_to_the_screen() {
    // C#: NativeDialog.PlaceInitially monitor fallback.
    let monitor = rect(-1920, 0, 0, 1080);
    assert_eq!(layout::work_area(None, 1920, 1080), rect(0, 0, 1920, 1080));
    assert_eq!(layout::work_area(Some(monitor), 1920, 1080), monitor);
}

#[test]
fn initial_frame_centres_on_the_owner() {
    // C#: NativeDialog.PlaceInitially.
    assert_eq!(
        layout::initial_frame(200, 100, Some(rect(100, 100, 500, 400)), rect(0, 0, 800, 600), false),
        (200, 200, 200, 100)
    );
}

#[test]
fn initial_frame_is_clamped_to_the_work_area_right_and_bottom() {
    // C#: NativeDialog.PlaceInitially.
    assert_eq!(
        layout::initial_frame(200, 100, Some(rect(750, 550, 850, 650)), rect(0, 0, 800, 600), false),
        (600, 500, 200, 100)
    );
}

#[test]
fn initial_frame_left_edge_wins_when_the_frame_is_wider_than_the_work_area_and_not_resizable() {
    // C#: NativeDialog.PlaceInitially min then max.
    assert_eq!(
        layout::initial_frame(900, 700, None, rect(100, 50, 800, 600), false),
        (100, 50, 900, 700)
    );
}

#[test]
fn initial_frame_shrinks_a_resizable_frame_to_the_work_area() {
    // C#: NativeDialog.PlaceInitially.
    assert_eq!(
        layout::initial_frame(900, 700, None, rect(100, 50, 800, 600), true),
        (100, 50, 700, 550)
    );
}

#[test]
fn initial_frame_without_owner_centres_in_the_work_area() {
    // C#: NativeDialog.PlaceInitially.
    assert_eq!(
        layout::initial_frame(200, 100, None, rect(100, 50, 800, 600), false),
        (350, 275, 200, 100)
    );
}

#[test]
fn initial_frame_on_a_monitor_with_negative_coordinates() {
    // C#: NativeDialog.PlaceInitially.
    assert_eq!(
        layout::initial_frame(400, 200, None, rect(-1920, 0, 0, 1080), false),
        (-1160, 440, 400, 200)
    );
}
