//! Lane `st-painter-tests` (ST-GAPMAP2 section 6, task 9; PARITY rows X-chrome-painter-controls and X-chrome-painter-frame): the three gaps those rows still list.
//!
//! 1. The 144 dpi variant of the painter geometry (W8-18m open question 2). A real 144 dpi window cannot be made on an arbitrary machine (the window dpi is the
//!    dpi of its monitor), so the whole 144 dpi chain `paint_scale(144)` -> `scaled` -> `push_button_draw` / `group_box_layout` is pinned as pure data, with the
//!    worked values of the C# rounding (`Math.Round`, to even).
//! 2. `Fill` with an empty rectangle draws nothing (C# `if (right <= left || bottom <= top) return;`). `fill` is `pub(super)` and there is no recording DC, so the
//!    pure tests classify the rectangles the public geometry hands to `fill` with the same predicate, and the native test paints a control whose client area is
//!    empty and checks that no pixel outside it changed.
//! 3. `SS_ETCHEDHORZ` / `SS_ETCHEDVERT` fixtures with a NON-empty client area. The system makes a freshly created etched line window 2 px thin with an empty client
//!    area, so `dialogs_painter_frame.rs` could only measure `SS_ETCHEDFRAME`; here the control is resized after creation and the painted line is measured.
//!
//! Pure tests run everywhere; native tests are `#[ignore = "native: needs desktop"]` (hidden/offscreen window + `PrintWindow`, like the sibling files).

mod common {
    pub mod dialogs_painter_fixture;
}

use std::sync::Mutex;

use common::dialogs_painter_fixture::{OwnerWindow, Shot, child, child_at, client_on_screen, on_sta, rgb, window_rect};
use pr_platform_win::dialogs::native::painter::NativeDialogChromePainter as Painter;
use pr_platform_win::dialogs::native::painter::controls::{PushButtonInput, frame_rects, push_button_draw};
use pr_platform_win::dialogs::native::painter::frame::{EtchedShape, etched_shape, group_box_layout};
use pr_platform_win::dialogs::native::painter::geometry::{ChromePaintKind as K, keys, paint_scale, scaled};
use pr_platform_win::dialogs::native::template::DialogControlClass as C;
use pr_platform_win::win32::dialog_native::user32::move_window;
use pr_platform_win::win32::structs::{Hwnd, Rect};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

/// The predicate of C# `Fill` (and of `painter::controls::fill`): an empty or inverted rectangle draws nothing.
fn fill_skips(rect: Rect) -> bool {
    rect.right <= rect.left || rect.bottom <= rect.top
}

// ---- pure: 144 dpi ---------------------------------------------------------------------------------------------------------------------------------

#[test]
fn paint_scale_at_144_dpi_is_one_and_a_half_and_never_below_one() {
    // C#: Math.Max(1.0, GetDpiForWindow(hwnd) / 96.0)
    assert_eq!(paint_scale(144), 1.5);
    assert_eq!(paint_scale(96), 1.0);
    assert_eq!(paint_scale(120), 1.25);
    assert_eq!(paint_scale(192), 2.0);
    assert_eq!(paint_scale(72), 1.0, "below 96 dpi clamps to 1.0");
    assert_eq!(paint_scale(0), 1.0, "a dead window answers 0 dpi");
}

#[test]
fn scaled_at_144_dpi_rounds_half_to_even_and_never_returns_less_than_one() {
    // C#: Math.Max(1, (int)Math.Round(value * scale)); Math.Round(4.5) = 4, Math.Round(1.5) = 2, Math.Round(7.5) = 8
    let scale = paint_scale(144);
    let table = [
        (0, 1), // max(1, 0)
        (1, 2), // 1.5 -> 2
        (2, 3),
        (3, 4), // 4.5 -> 4 (ties to even), NOT 5
        (4, 6),
        (5, 8), // 7.5 -> 8
        (6, 9),
        (7, 10), // 10.5 -> 10
        (8, 12),
        (-4, 1), // a negative value clamps to 1
    ];
    for (value, expected) in table {
        assert_eq!(scaled(scale, value), expected, "S({value}) at 144 dpi");
    }
}

fn enabled() -> PushButtonInput {
    PushButtonInput {
        enabled: true,
        ..Default::default()
    }
}

#[test]
fn push_button_geometry_at_144_dpi() {
    // C#: DrawPushButton with scale 1.5 on a 150 x 36 px button (a 100 x 24 px button at 96 dpi).
    let scale = paint_scale(144);
    let rest = push_button_draw(enabled(), 150, 36, scale);
    assert_eq!(rest.radius, 9, "S(6)");
    assert_eq!(rest.text_rect, r(6, 0, 144, 36), "S(4) = 6 on both sides");
    assert_eq!(rest.focus_rect, None);

    let focused = push_button_draw(
        PushButtonInput {
            focused: true,
            ..enabled()
        },
        150,
        36,
        scale,
    );
    assert_eq!(
        focused.focus_rect,
        Some(r(4, 4, 146, 32)),
        "S(3) = round_ties_even(4.5) = 4, so the focus inset is 4, not 5"
    );

    let pressed = push_button_draw(
        PushButtonInput {
            pressed: true,
            ..enabled()
        },
        150,
        36,
        scale,
    );
    assert_eq!(
        pressed.text_rect,
        r(7, 1, 145, 37),
        "the pressed shift is one device pixel, NOT scaled"
    );
}

#[test]
fn push_button_inset_never_exceeds_the_width_it_is_cut_from_at_144_dpi() {
    // A button narrower than twice the inset gives an inverted text rectangle (right < left); the draw code passes it to DrawText as is. Pin the numbers.
    let scale = paint_scale(144);
    let narrow = push_button_draw(enabled(), 10, 36, scale);
    assert_eq!(narrow.text_rect, r(6, 0, 4, 36));
    assert!(
        fill_skips(narrow.text_rect),
        "an inverted text rectangle is empty for GDI fills"
    );
}

#[test]
fn group_box_layout_at_144_dpi() {
    // C#: DrawGroupBox with scale 1.5: pad = S(3) = 4, left = S(8) = 12, frame top = text_height / 2 (NOT scaled; the font already is).
    let scale = paint_scale(144);
    let layout = group_box_layout(300, 100, scale, 22, 60, true, false);
    assert_eq!(layout.frame, r(0, 11, 300, 100));
    let label = layout.label.expect("label");
    assert_eq!(
        label.patch,
        r(8, 0, 12 + 60 + 4, 22),
        "(left - pad, 0, left + shown + pad, text_height)"
    );
    assert_eq!(label.text_rect, r(12, 0, 72, 22));

    // The shown width is cut to width - left - 2 * pad = 30 - 12 - 8 = 10.
    let narrow = group_box_layout(30, 100, scale, 22, 80, true, false)
        .label
        .expect("label");
    assert_eq!(narrow.text_rect, r(12, 0, 22, 22));

    // Narrower than the padding: shown is 0 and never negative.
    let tiny = group_box_layout(15, 100, scale, 22, 80, true, false)
        .label
        .expect("label");
    assert_eq!(tiny.text_rect, r(12, 0, 12, 22));
    assert!(fill_skips(tiny.text_rect));
}

#[test]
fn etched_lines_are_one_device_pixel_thick_at_every_dpi() {
    // C#: DrawEtched fills a 1 px line; the thickness is not multiplied by the scale. Heights a 144 dpi dialog would give (8 dlu -> 12 px).
    assert_eq!(etched_shape(0x10, 150, 12), EtchedShape::Line(r(0, 6, 150, 7)));
    assert_eq!(etched_shape(0x11, 12, 150), EtchedShape::Line(r(6, 0, 7, 150)));
    // Odd sizes: the integer division floors.
    assert_eq!(etched_shape(0x10, 150, 13), EtchedShape::Line(r(0, 6, 150, 7)));
    assert_eq!(etched_shape(0x11, 13, 150), EtchedShape::Line(r(6, 0, 7, 150)));
}

// ---- pure: empty fill rectangle -------------------------------------------------------------------------------------------------------------------

#[test]
fn fill_predicate_matches_the_documented_csharp_condition() {
    assert!(fill_skips(r(0, 0, 0, 5)), "zero width");
    assert!(fill_skips(r(0, 0, 5, 0)), "zero height");
    assert!(fill_skips(r(5, 0, 2, 5)), "inverted horizontally");
    assert!(fill_skips(r(0, 5, 5, 2)), "inverted vertically");
    assert!(!fill_skips(r(0, 0, 1, 1)), "1 x 1 is drawn");
    assert!(
        !fill_skips(r(-3, -3, -2, -2)),
        "negative coordinates are fine when the size is positive"
    );
}

#[test]
fn frame_of_an_empty_client_hands_only_empty_rectangles_to_fill() {
    // C# Frame(hdc, 0, 0, 0, 0, color, 1): all four strips are empty, so nothing is painted.
    for rect in frame_rects(0, 0, 0, 0, 1) {
        assert!(fill_skips(rect), "{rect:?} must be skipped by Fill");
    }
    // A zero thickness is skipped as well, whatever the client.
    for rect in frame_rects(0, 0, 100, 40, 0) {
        assert!(fill_skips(rect), "thickness 0: {rect:?} must be skipped");
    }
    // A negative thickness inverts every strip.
    for rect in frame_rects(0, 0, 100, 40, -2) {
        assert!(fill_skips(rect), "thickness -2: {rect:?} must be skipped");
    }
    // 1 x 1 client: all four strips are one pixel and drawn (they overlap on the same pixel).
    for rect in frame_rects(0, 0, 1, 1, 1) {
        assert!(!fill_skips(rect), "1 x 1 client: {rect:?} is drawn");
    }
}

#[test]
fn etched_shape_of_an_empty_client_hands_an_empty_rectangle_to_fill() {
    // The line variants skip when the long side is empty.
    match etched_shape(0x10, 0, 20) {
        EtchedShape::Line(rect) => assert!(fill_skips(rect), "horizontal line of width 0"),
        other => panic!("expected a line, got {other:?}"),
    }
    match etched_shape(0x11, 20, 0) {
        EtchedShape::Line(rect) => assert!(fill_skips(rect), "vertical line of height 0"),
        other => panic!("expected a line, got {other:?}"),
    }
    // The short side being 0 does NOT make the line empty: (0, 0, w, 1) is one row, painted outside the client and clipped by the DC.
    match etched_shape(0x10, 20, 0) {
        EtchedShape::Line(rect) => {
            assert_eq!(rect, r(0, 0, 20, 1));
            assert!(!fill_skips(rect));
        }
        other => panic!("expected a line, got {other:?}"),
    }
    match etched_shape(0x11, 0, 20) {
        EtchedShape::Line(rect) => {
            assert_eq!(rect, r(0, 0, 1, 20));
            assert!(!fill_skips(rect));
        }
        other => panic!("expected a line, got {other:?}"),
    }
    // The frame variant of a 0 x 0 client: the four strips are all empty.
    match etched_shape(0x12, 0, 0) {
        EtchedShape::Frame(rect) => {
            assert_eq!(rect, r(0, 0, 0, 0));
            for strip in frame_rects(rect.left, rect.top, rect.right, rect.bottom, 1) {
                assert!(fill_skips(strip));
            }
        }
        other => panic!("expected a frame, got {other:?}"),
    }
}

// ---- native ------------------------------------------------------------------------------------------------------------------------------------

const SS_ETCHEDHORZ: u32 = 0x10;
const SS_ETCHEDVERT: u32 = 0x11;
const SS_ETCHEDFRAME: u32 = 0x12;

fn run(test: impl FnOnce() + Send + 'static) {
    on_sta(move || {
        let _serial = serial();
        test();
    });
}

fn client_pixel(shot: &Shot, hwnd: Hwnd, dx: i32, dy: i32) -> u32 {
    let client = client_on_screen(hwnd);
    shot.at(client.left + dx, client.top + dy)
}

fn client_size(hwnd: Hwnd) -> (i32, i32) {
    let client = client_on_screen(hwnd);
    (client.right - client.left, client.bottom - client.top)
}

#[test]
#[ignore = "native: needs desktop"]
fn etched_horizontal_line_with_a_non_empty_client_paints_the_border_line_at_half_height() {
    // C#: DrawEtched, SS_ETCHEDHORZ -> Fill(0, height / 2, width, height / 2 + 1, Dark.Border).
    run(|| {
        let owner = OwnerWindow::dark();
        let line = child_at(&owner, C::Static, SS_ETCHEDHORZ, "", 8, 8, 120, 2);
        // The system keeps a freshly created etched line 2 px thin with an empty client; resize it so the client is real.
        move_window(line, 8, 8, 120, 12, true).expect("resize the etched line");
        let (width, height) = client_size(line);
        assert!(
            width > 0 && height > 0,
            "the fixture must have a non-empty client area, got {width} x {height}"
        );
        assert!(Painter::attach(line, K::EtchedStatic));
        let shot = Shot::capture(owner.hwnd);
        assert_eq!(
            client_pixel(&shot, line, width / 2, height / 2),
            rgb(keys::BORDER),
            "the line row"
        );
        assert_ne!(
            client_pixel(&shot, line, width / 2, height / 2 + 1),
            rgb(keys::BORDER),
            "the row below the line is not part of it"
        );
        assert_eq!(
            client_pixel(&shot, line, width / 2, 0),
            rgb(keys::WINDOW),
            "above the line is the window fill"
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn etched_vertical_line_with_a_non_empty_client_paints_the_border_line_at_half_width() {
    // C#: DrawEtched, SS_ETCHEDVERT -> Fill(width / 2, 0, width / 2 + 1, height, Dark.Border).
    run(|| {
        let owner = OwnerWindow::dark();
        let line = child_at(&owner, C::Static, SS_ETCHEDVERT, "", 8, 8, 2, 120);
        move_window(line, 8, 8, 12, 120, true).expect("resize the etched line");
        let (width, height) = client_size(line);
        assert!(
            width > 0 && height > 0,
            "the fixture must have a non-empty client area, got {width} x {height}"
        );
        assert!(Painter::attach(line, K::EtchedStatic));
        let shot = Shot::capture(owner.hwnd);
        assert_eq!(
            client_pixel(&shot, line, width / 2, height / 2),
            rgb(keys::BORDER),
            "the line column"
        );
        assert_ne!(
            client_pixel(&shot, line, width / 2 + 1, height / 2),
            rgb(keys::BORDER),
            "the column right of the line is not part of it"
        );
        assert_eq!(
            client_pixel(&shot, line, 0, height / 2),
            rgb(keys::WINDOW),
            "left of the line is the window fill"
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn painting_a_control_with_an_empty_client_changes_no_pixel_outside_it() {
    // C#: Fill returns early for an empty rectangle; the painter must not touch the owner window around a zero-size etched frame / static.
    run(|| {
        let owner = OwnerWindow::dark();
        let before = child(&owner, C::Static, SS_ETCHEDFRAME, "", 40, 20);
        let empty = child_at(&owner, C::Static, SS_ETCHEDFRAME, "", 100, 60, 0, 0);
        let (width, height) = client_size(empty);
        assert!(
            width <= 0 || height <= 0,
            "the fixture client must be empty, got {width} x {height}"
        );
        assert!(Painter::attach(before, K::EtchedStatic));
        assert!(Painter::attach(empty, K::EtchedStatic));
        let shot = Shot::capture(owner.hwnd);
        let around = window_rect(empty);
        for (dx, dy) in [(-1, -1), (-1, 0), (0, -1), (1, 0), (0, 1), (2, 2), (-3, 3)] {
            assert_eq!(
                shot.at(around.left + dx, around.top + dy),
                rgb(keys::WINDOW),
                "pixel ({dx}, {dy}) next to the empty control keeps the owner fill"
            );
        }
    });
}
