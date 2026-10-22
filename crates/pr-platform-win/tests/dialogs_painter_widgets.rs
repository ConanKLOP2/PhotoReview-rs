//! Unit W8-18n, part 1 (contract `contracts/W8-18n.md`): check box, radio button, combo box, progress bar and trackbar of the chrome painter,
//! `painter::widgets`. Pure layout tests run everywhere (worked examples hand-computed from the C# `DrawCheckOrRadio`, `DrawComboBox`, `DrawProgress`,
//! `DrawTrackbar`); the native pixel tests are `#[ignore = "native: needs desktop"]` and measure real offscreen windows with `PrintWindow`.

mod common {
    pub mod dialogs_painter_fixture;
}

use std::collections::BTreeSet;
use std::sync::Mutex;

use common::dialogs_painter_fixture::{OwnerWindow, Shot, child, client_on_screen, on_sta, rgb};
use pr_platform_win::dialogs::native::painter::NativeDialogChromePainter as Painter;
use pr_platform_win::dialogs::native::painter::geometry::{ChromePaintKind as K, keys};
use pr_platform_win::dialogs::native::painter::widgets::{
    CheckInput, ComboInput, KeyedRect, TrackbarInput, check_layout, combo_layout, progress_fill_width, tick_xs,
    trackbar_layout,
};
use pr_platform_win::dialogs::native::template::DialogControlClass as C;
use pr_platform_win::win32::dialog_native::user32::{get_gui_resources, send_message};
use pr_platform_win::win32::structs::{Hwnd, Point, Rect};

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

fn p(x: i32, y: i32) -> Point {
    Point { x, y }
}

// ---- pure: check box and radio -------------------------------------------------------------------------------------------------------------------------

fn check_input() -> CheckInput {
    CheckInput {
        width: 120,
        height: 20,
        scale: 1.0,
        radio: false,
        enabled: true,
        hot: false,
        checked: false,
        focused: false,
        multiline: false,
        left_text: false,
        text_height: 15,
        text_width: 60,
        hide_focus: false,
        hide_accel: false,
    }
}

#[test]
fn check_layout_worked_examples() {
    // C#: NativeDialogChromePainter.DrawCheckOrRadio, hand-computed.
    // (a) a checked, focused check box
    let a = check_layout(CheckInput {
        checked: true,
        focused: true,
        ..check_input()
    });
    assert_eq!((a.box_size, a.box_left, a.box_top), (13, 0, 3));
    assert_eq!((a.border_key, a.fill_key), (keys::ACCENT, keys::ACCENT));
    assert_eq!(a.mark, Some((keys::TEXT_STRONG, 2, [p(3, 10), p(5, 12), p(10, 7)])));
    assert_eq!(a.dot, None);
    assert_eq!(a.text_rect, r(19, 0, 120, 20));
    assert_eq!(a.text_flags, 0x8024);
    assert_eq!(a.text_key, keys::TEXT_STRONG);
    assert_eq!(a.focus_rect, Some(r(18, 1, 81, 18)));

    // (b) a checked, hot radio button at 150 %
    let b = check_layout(CheckInput {
        radio: true,
        checked: true,
        hot: true,
        scale: 1.5,
        text_height: 22,
        ..check_input()
    });
    assert_eq!((b.box_size, b.box_left, b.box_top), (20, 0, 0));
    assert_eq!((b.border_key, b.fill_key), (keys::ACCENT, keys::INPUT));
    assert_eq!(b.dot, Some((keys::ACCENT, 6)), "round_ties_even(5.6) = 6");
    assert_eq!(b.mark, None);
    assert_eq!(b.text_rect, r(29, 0, 120, 20));

    // (c) left text, multiline
    let c = check_layout(CheckInput {
        height: 40,
        multiline: true,
        left_text: true,
        ..check_input()
    });
    assert_eq!((c.box_left, c.box_top), (107, 1));
    assert_eq!(c.text_rect, r(0, 0, 101, 40));
    assert_eq!(c.text_flags, 0x10);
    assert_eq!((c.border_key, c.fill_key), (keys::BORDER_STRONG, keys::INPUT));

    // (d) disabled and checked
    let d = check_layout(CheckInput {
        enabled: false,
        checked: true,
        focused: true,
        ..check_input()
    });
    assert_eq!(d.border_key, keys::TEXT_FAINT);
    assert_eq!(
        d.fill_key,
        keys::INPUT,
        "the fill is the accent only when checked AND enabled"
    );
    assert_eq!(d.mark, Some((keys::TEXT_FAINT, 2, [p(3, 10), p(5, 12), p(10, 7)])));
    assert_eq!(d.text_key, keys::TEXT_MUTED);
    assert_eq!(d.focus_rect, None);
}

#[test]
fn check_layout_key_priority_table() {
    // (e) border key: disabled, then checked, then hot, else strong; all 16 combinations of enabled / checked / hot / radio.
    for bits in 0..16u32 {
        let (enabled, checked, hot, radio) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0);
        let layout = check_layout(CheckInput {
            enabled,
            checked,
            hot,
            radio,
            ..check_input()
        });
        let border = if !enabled {
            "Dark.TextFaint"
        } else if checked {
            "Dark.Accent"
        } else if hot {
            "Dark.TextMuted"
        } else {
            "Dark.BorderStrong"
        };
        assert_eq!(layout.border_key, border, "border {bits:04b}");
        let fill = if !radio && checked && enabled {
            "Dark.Accent"
        } else {
            "Dark.Input"
        };
        assert_eq!(layout.fill_key, fill, "fill {bits:04b}");
        assert_eq!(layout.dot.is_some(), radio && checked, "dot {bits:04b}");
        assert_eq!(layout.mark.is_some(), !radio && checked, "mark {bits:04b}");
        if let Some((key, _)) = layout.dot {
            assert_eq!(key, if enabled { "Dark.Accent" } else { "Dark.TextFaint" });
        }
        assert_eq!(
            layout.text_key,
            if enabled { "Dark.TextStrong" } else { "Dark.TextMuted" }
        );
    }
}

#[test]
fn check_layout_clamps_negative_offsets_and_flags() {
    // The C# truncating division of a negative value is clamped by Math.Max(0, ..).
    let short = check_layout(CheckInput {
        height: 5,
        ..check_input()
    });
    assert_eq!(short.box_top, 0);
    let tiny = check_layout(CheckInput {
        width: 5,
        left_text: true,
        ..check_input()
    });
    assert_eq!(tiny.box_left, -8);
    assert_eq!(tiny.text_rect.right, 0, "max(0, box_left - gap)");
    // hide_accel adds DT_HIDEPREFIX, hide_focus removes the focus rectangle.
    let hidden = check_layout(CheckInput {
        hide_accel: true,
        hide_focus: true,
        focused: true,
        ..check_input()
    });
    assert_eq!(hidden.text_flags, 0x8024 | 0x0010_0000);
    assert_eq!(hidden.focus_rect, None);
    let multi = check_layout(CheckInput {
        multiline: true,
        hide_accel: true,
        ..check_input()
    });
    assert_eq!(multi.text_flags, 0x10 | 0x0010_0000);
    // A multiline focus rectangle starts at the top (text_top = 0).
    let focus = check_layout(CheckInput {
        multiline: true,
        focused: true,
        ..check_input()
    });
    assert_eq!(focus.focus_rect, Some(r(18, 0, 81, 16)));
    // The text rectangle never gets a negative focus width: a long text is cut at the right edge.
    let long = check_layout(CheckInput {
        focused: true,
        text_width: 10_000,
        ..check_input()
    });
    assert_eq!(long.focus_rect, Some(r(18, 1, 120, 18)));
}

// ---- pure: combo box -----------------------------------------------------------------------------------------------------------------------------------

fn combo_input() -> ComboInput {
    ComboInput {
        width: 200,
        height: 24,
        enabled: true,
        focused: false,
        dropped: false,
        hot: false,
        hide_focus: false,
    }
}

#[test]
fn combo_layout_worked_examples() {
    let a = combo_layout(
        ComboInput {
            focused: true,
            ..combo_input()
        },
        1.0,
    );
    assert_eq!(a.arrow_width, 24);
    assert_eq!(a.text_rect, r(6, 1, 176, 23));
    assert_eq!(a.text_flags, 0x8824);
    assert_eq!(a.arrow_points, [p(183, 10), p(187, 15), p(191, 10)]);
    assert_eq!(a.arrow_pen_width, 2);
    assert_eq!(a.focus_rect, Some(r(3, 3, 176, 21)));
    assert_eq!(a.border_key, keys::ACCENT);
    assert_eq!((a.text_key, a.arrow_key), (keys::TEXT_STRONG, keys::TEXT_SECONDARY));

    let b = combo_layout(
        ComboInput {
            height: 36,
            ..combo_input()
        },
        1.5,
    );
    assert_eq!(b.arrow_width, 36);
    assert_eq!(b.text_rect, r(9, 1, 164, 35));
    assert_eq!(b.arrow_points, [p(175, 15), p(181, 22), p(187, 15)]);
    assert_eq!(b.focus_rect, None);
    assert_eq!(b.border_key, keys::BORDER_STRONG);
}

#[test]
fn combo_layout_focus_rule() {
    for bits in 0..32u32 {
        let (enabled, focused, dropped, hot, hide_focus) = (
            bits & 1 != 0,
            bits & 2 != 0,
            bits & 4 != 0,
            bits & 8 != 0,
            bits & 16 != 0,
        );
        let layout = combo_layout(
            ComboInput {
                enabled,
                focused,
                dropped,
                hot,
                hide_focus,
                ..combo_input()
            },
            1.0,
        );
        // C#: `focused && !dropped && !UiState.HideFocus` (the disabled state does not matter for the rectangle).
        assert_eq!(
            layout.focus_rect.is_some(),
            focused && !dropped && !hide_focus,
            "focus {bits:05b}"
        );
        let border = if !enabled {
            "Dark.Border"
        } else if focused || dropped {
            "Dark.Accent"
        } else if hot {
            "Dark.TextMuted"
        } else {
            "Dark.BorderStrong"
        };
        assert_eq!(layout.border_key, border, "border {bits:05b}");
        assert_eq!(
            layout.text_key,
            if enabled { "Dark.TextStrong" } else { "Dark.TextMuted" }
        );
        assert_eq!(
            layout.arrow_key,
            if enabled {
                "Dark.TextSecondary"
            } else {
                "Dark.TextFaint"
            }
        );
    }
    // A narrow combo: the text and focus rectangles never invert.
    let narrow = combo_layout(
        ComboInput {
            width: 10,
            focused: true,
            ..combo_input()
        },
        1.0,
    );
    assert_eq!(narrow.text_rect.right, 6, "max(S(6), width - arrow_width)");
    assert_eq!(
        narrow.focus_rect.map(|f| f.right),
        Some(3),
        "max(3, width - arrow_width)"
    );
}

// ---- pure: progress ------------------------------------------------------------------------------------------------------------------------------------

#[test]
fn progress_fill_width_examples() {
    assert_eq!(progress_fill_width(200, 0, 0, 100, 40), Some(80));
    assert_eq!(
        progress_fill_width(101, 1, 0, 100, 50),
        Some(50),
        "99 * 0.5 = 49.5 rounds to even 50"
    );
    assert_eq!(progress_fill_width(200, 0, 0, 0, 5), None);
    assert_eq!(
        progress_fill_width(200, 1, 10, 20, 25),
        Some(198),
        "fraction clamped to 1"
    );
    assert_eq!(
        progress_fill_width(200, 1, 0, 100, -5),
        Some(0),
        "fraction clamped to 0"
    );
    assert_eq!(progress_fill_width(200, 0, 5, 4, 5), None, "high < low");
    // Ties go to even: 0.5 -> 0, 1.5 -> 2, 2.5 -> 2.
    assert_eq!(progress_fill_width(1, 0, 0, 2, 1), Some(0));
    assert_eq!(progress_fill_width(3, 0, 0, 2, 1), Some(2));
    assert_eq!(progress_fill_width(5, 0, 0, 2, 1), Some(2));
}

#[test]
fn progress_fill_width_never_panics() {
    let values = [i32::MIN, -1, 0, 1, i32::MAX];
    let mut count = 0;
    for width in values {
        for inset in values {
            for low in values {
                for high in values {
                    for position in values {
                        let _ = progress_fill_width(width, inset, low, high, position);
                        count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(count, 3125);
}

// ---- pure: tick marks ----------------------------------------------------------------------------------------------------------------------------------

#[test]
fn tick_xs_examples() {
    assert_eq!(
        tick_xs(10, 210, 11, 0, 10, true, None, 1.0),
        [10, 30, 50, 70, 90, 110, 129, 149, 169, 189, 209]
    );
    assert_eq!(
        tick_xs(10, 110, 101, 0, 100, true, None, 1.0),
        [10, 17, 24, 31, 38, 45, 52, 59, 65, 72, 79, 86, 93, 100, 107, 109],
        "step 7, plus the last tick at channel_left + span"
    );
    assert_eq!(
        tick_xs(10, 110, 5, 0, 100, false, Some(&[10, 50, 90]), 1.0),
        [10, 20, 60, 99, 109]
    );
    assert_eq!(
        tick_xs(0, 100, 4, 0, 3, false, None, 1.0),
        [0, 0, 0, 99],
        "a missing inner table gives min"
    );
}

#[test]
fn tick_xs_degenerate_inputs() {
    assert!(tick_xs(0, 100, 1, 0, 10, true, None, 1.0).is_empty(), "count 1");
    assert!(tick_xs(0, 100, 0, 0, 10, true, None, 1.0).is_empty(), "count 0");
    assert!(tick_xs(0, 100, -5, 0, 10, true, None, 1.0).is_empty(), "negative count");
    assert!(tick_xs(0, 100, 5, 10, 10, true, None, 1.0).is_empty(), "max == min");
    assert!(tick_xs(0, 100, 5, 10, 0, true, None, 1.0).is_empty(), "max < min");
    // An inner table that is too short gives min for the missing entries.
    assert_eq!(tick_xs(0, 100, 5, 0, 100, false, Some(&[50]), 1.0), [0, 50, 0, 0, 99]);
    // Extremes never panic and finish quickly (the step grows with the count).
    let values = [i32::MIN, -1, 0, 1, 1000, i32::MAX];
    for left in values {
        for right in values {
            for count in [2, 3, 1000] {
                for min in [i32::MIN, 0, 5] {
                    for max in [i32::MAX, 10] {
                        let _ = tick_xs(left, right, count, min, max, true, None, 1.0);
                        let _ = tick_xs(left, right, count, min, max, false, Some(&[1, 2, 3]), 1.5);
                    }
                }
            }
        }
    }
    // A huge count with a tiny span: the step grows (600), so the loop is a few million iterations at most.
    assert!(!tick_xs(0, 2, i32::MAX, 0, 10, true, None, 1.0).is_empty());
}

// ---- pure: trackbar ------------------------------------------------------------------------------------------------------------------------------------

fn trackbar_input() -> TrackbarInput {
    TrackbarInput {
        channel: r(10, 12, 210, 20),
        thumb: r(90, 5, 102, 27),
        height: 40,
        scale: 1.0,
        enabled: true,
        hot: false,
        no_ticks: true,
        auto_ticks: true,
        tick_count: 0,
        range_min: 0,
        range_max: 0,
        inner_ticks: None,
    }
}

#[test]
fn trackbar_layout_example_and_disabled_hot_variants() {
    let layout = trackbar_layout(&trackbar_input());
    assert_eq!(
        layout.channel,
        KeyedRect {
            rect: r(10, 14, 210, 18),
            key: keys::BORDER_STRONG
        }
    );
    assert_eq!(
        layout.filled,
        KeyedRect {
            rect: r(10, 14, 96, 18),
            key: keys::ACCENT
        }
    );
    assert_eq!(layout.thumb.key, keys::ACCENT);
    assert_eq!(layout.thumb.rect, r(90, 5, 102, 27));
    assert_eq!(layout.thumb_border_key, keys::TEXT_SOFT);
    assert!(layout.ticks.is_empty());

    let hot = trackbar_layout(&TrackbarInput {
        hot: true,
        ..trackbar_input()
    });
    assert_eq!(hot.thumb.key, keys::ACCENT_HOVER);
    let disabled = trackbar_layout(&TrackbarInput {
        enabled: false,
        hot: true,
        no_ticks: false,
        tick_count: 3,
        range_max: 2,
        ..trackbar_input()
    });
    assert_eq!(disabled.channel.key, keys::BORDER);
    assert_eq!(disabled.filled.key, keys::TEXT_FAINT);
    assert_eq!(disabled.thumb.key, keys::TEXT_FAINT, "disabled wins over hot");
    assert_eq!(disabled.thumb_border_key, keys::BORDER);
    assert_eq!(disabled.tick_key, keys::TEXT_FAINT);
    assert_eq!(trackbar_layout(&trackbar_input()).tick_key, keys::TEXT_MUTED);
}

#[test]
fn trackbar_layout_ticks_are_one_pixel_below_the_thumb() {
    // top = thumb.bottom + S(1), bottom = min(height, top + S(4)), width 1.
    let layout = trackbar_layout(&TrackbarInput {
        no_ticks: false,
        tick_count: 3,
        range_max: 2,
        ..trackbar_input()
    });
    assert_eq!(
        layout.ticks,
        [r(10, 28, 11, 32), r(110, 28, 111, 32), r(209, 28, 210, 32)]
    );
    // The bottom is limited by the client height.
    let clipped = trackbar_layout(&TrackbarInput {
        no_ticks: false,
        tick_count: 2,
        range_max: 1,
        height: 30,
        ..trackbar_input()
    });
    assert_eq!(clipped.ticks, [r(10, 28, 11, 30), r(209, 28, 210, 30)]);
    // No ticks without TBS_NOTICKS cleared, with count < 2 or an empty range.
    for (count, min, max) in [(1, 0, 10), (0, 0, 10), (5, 3, 3), (5, 4, 3)] {
        let none = trackbar_layout(&TrackbarInput {
            no_ticks: false,
            tick_count: count,
            range_min: min,
            range_max: max,
            ..trackbar_input()
        });
        assert!(none.ticks.is_empty(), "{count} {min} {max}");
    }
    let hidden = trackbar_layout(&TrackbarInput {
        no_ticks: true,
        tick_count: 5,
        range_max: 4,
        ..trackbar_input()
    });
    assert!(hidden.ticks.is_empty());
}

#[test]
fn trackbar_layout_thumb_outside_the_channel_does_not_panic() {
    // A thumb centre left of the channel start gives an empty filled rectangle, a centre right of the end is clamped to the channel end.
    let left = trackbar_layout(&TrackbarInput {
        thumb: r(-50, 5, -38, 27),
        ..trackbar_input()
    });
    assert_eq!(left.filled.rect, r(10, 14, 10, 18));
    let right = trackbar_layout(&TrackbarInput {
        thumb: r(500, 5, 512, 27),
        ..trackbar_input()
    });
    assert_eq!(right.filled.rect, r(10, 14, 210, 18));
    // An inverted channel (left > right) must not panic either (a clamp would).
    let inverted = trackbar_layout(&TrackbarInput {
        channel: r(210, 12, 10, 20),
        ..trackbar_input()
    });
    assert_eq!(inverted.filled.rect.left, 210);
    let values = [i32::MIN, -1, 0, 1, i32::MAX];
    for a in values {
        for b in values {
            let _ = trackbar_layout(&TrackbarInput {
                channel: r(a, b, b, a),
                thumb: r(b, a, a, b),
                height: a,
                no_ticks: false,
                tick_count: 3,
                range_min: a,
                range_max: b,
                ..trackbar_input()
            });
        }
    }
}

// ---- native ----------------------------------------------------------------------------------------------------------------------------------------------

const BS_AUTOCHECKBOX: u32 = 3;
const BS_AUTORADIOBUTTON: u32 = 9;
const BM_SETCHECK: u32 = 0x00F1;
const PBS_SMOOTH: u32 = 1;
const PBM_SETRANGE32: u32 = 0x0406;
const PBM_SETPOS: u32 = 0x0402;
const TBS_AUTOTICKS: u32 = 1;
const TBM_SETPOS: u32 = 0x0405;
const TBM_SETRANGE: u32 = 0x0406;
const CBS_DROPDOWNLIST: u32 = 3;
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const GR_GDIOBJECTS: u32 = 0;

fn run(test: impl FnOnce() + Send + 'static) {
    on_sta(move || {
        let _serial = serial();
        test();
    });
}

fn pixel(shot: &Shot, hwnd: Hwnd, dx: i32, dy: i32) -> u32 {
    let client = client_on_screen(hwnd);
    shot.at(client.left + dx, client.top + dy)
}

fn distinct_colours(shot: &Shot, hwnd: Hwnd) -> BTreeSet<u32> {
    let client = client_on_screen(hwnd);
    let mut colours = BTreeSet::new();
    for y in client.top..client.bottom {
        for x in client.left..client.right {
            colours.insert(shot.at(x, y));
        }
    }
    colours
}

#[test]
#[ignore = "native: needs desktop"]
fn check_and_radio_draw_the_checked_state_with_the_accent_colour() {
    // C#: NativeDialogChromeTests.CheckBoxAndRadio_DrawTheirCheckedStateWithTheAccentColour
    run(|| {
        let owner = OwnerWindow::dark();
        let check = child(&owner, C::Button, BS_AUTOCHECKBOX, "Check", 150, 24);
        let radio = child(&owner, C::Button, BS_AUTORADIOBUTTON, "Radio", 150, 24);
        assert!(Painter::attach(check, K::CheckBox));
        assert!(Painter::attach(radio, K::RadioButton));
        // The box is 13 px at (0, 5): column 1 is inside the border and left of the mark; the radio centre is (6, 11).
        let check_probe = |shot: &Shot| pixel(shot, check, 1, 11);
        let radio_probe = |shot: &Shot| pixel(shot, radio, 6, 11);
        let shot = Shot::capture(owner.hwnd);
        assert_eq!(check_probe(&shot), rgb(keys::INPUT), "unchecked box");
        assert_eq!(radio_probe(&shot), rgb(keys::INPUT), "unchecked radio");
        assert_eq!(pixel(&shot, check, 0, 11), rgb(keys::BORDER_STRONG), "unchecked border");
        // SAFETY: integer parameters only.
        unsafe {
            send_message(check, BM_SETCHECK, 1, 0);
            send_message(radio, BM_SETCHECK, 1, 0);
        }
        let shot = Shot::capture(owner.hwnd);
        assert_eq!(check_probe(&shot), rgb(keys::ACCENT), "checked box");
        assert_eq!(radio_probe(&shot), rgb(keys::ACCENT), "checked radio dot");
        assert_eq!(pixel(&shot, check, 0, 11), rgb(keys::ACCENT), "checked border");
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn progress_fills_exactly_the_fraction_in_the_accent_colour() {
    // C#: NativeDialogChromeTests.Progress_FillsExactlyTheFractionInTheAccentColour
    run(|| {
        let owner = OwnerWindow::dark();
        let progress = child(&owner, C::Progress, PBS_SMOOTH, "", 200, 16);
        assert!(Painter::attach(progress, K::Progress));
        // SAFETY: integer parameters only.
        unsafe {
            send_message(progress, PBM_SETRANGE32, 0, 100);
            send_message(progress, PBM_SETPOS, 40, 0);
        }
        let shot = Shot::capture(owner.hwnd);
        let client = client_on_screen(progress);
        let width = client.right - client.left;
        let row = client.top + (client.bottom - client.top) / 2;
        let filled = (client.left..client.right)
            .filter(|&x| shot.at(x, row) == rgb(keys::ACCENT))
            .count() as f64;
        let fraction = filled / f64::from(width);
        assert!(
            (0.37..=0.43).contains(&fraction),
            "filled {filled} of {width} = {fraction:.3}"
        );
        assert_eq!(
            pixel(&shot, progress, width - 3, 8),
            rgb(keys::SURFACE),
            "the rest is the surface colour"
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn combo_closed_look_uses_the_input_fill_and_accent_border_when_focused() {
    run(|| {
        let owner = OwnerWindow::dark();
        let combo = child(&owner, C::ComboBox, CBS_DROPDOWNLIST, "", 160, 120);
        let text: Vec<u16> = "Item\0".encode_utf16().collect();
        // SAFETY: `text` is a NUL-terminated UTF-16 local that lives past the synchronous call; the control copies it.
        unsafe {
            send_message(combo, CB_ADDSTRING, 0, text.as_ptr() as isize);
            send_message(combo, CB_SETCURSEL, 0, 0);
        }
        assert!(Painter::attach(combo, K::ComboBox));
        let shot = Shot::capture(owner.hwnd);
        assert_eq!(pixel(&shot, combo, 2, 10), rgb(keys::INPUT), "closed fill");
        assert_eq!(pixel(&shot, combo, 0, 10), rgb(keys::BORDER_STRONG), "unfocused border");
        let _ = pr_platform_win::win32::dialog_native::user32::set_focus(combo);
        if pr_platform_win::win32::dialog_native::user32::get_focus() == combo {
            let shot = Shot::capture(owner.hwnd);
            assert_eq!(pixel(&shot, combo, 0, 10), rgb(keys::ACCENT), "focused border");
        } else {
            eprintln!("note: the offscreen owner could not take the focus; the focused border was not measured");
        }
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn trackbar_draws_the_channel_thumb_and_ticks() {
    run(|| {
        let owner = OwnerWindow::dark();
        let trackbar = child(&owner, C::Trackbar, TBS_AUTOTICKS, "", 200, 40);
        assert!(Painter::attach(trackbar, K::Trackbar));
        // SAFETY: integer parameters only (the range is MAKELONG(0, 10)).
        unsafe {
            send_message(trackbar, TBM_SETRANGE, 1, 10 << 16);
            send_message(trackbar, TBM_SETPOS, 1, 5);
        }
        let shot = Shot::capture(owner.hwnd);
        let colours = distinct_colours(&shot, trackbar);
        for (name, key) in [
            ("channel", keys::BORDER_STRONG),
            ("filled channel / thumb", keys::ACCENT),
            ("ticks", keys::TEXT_MUTED),
            ("thumb outline", keys::TEXT_SOFT),
        ] {
            assert!(colours.contains(&rgb(key)), "{name} ({key}) is drawn");
        }
        assert!(!colours.contains(&0x00FF_00FF), "nothing paints the magenta marker");
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn widgets_do_not_leak_gdi_objects_over_50_paints() {
    run(|| {
        let owner = OwnerWindow::dark();
        let check = child(&owner, C::Button, BS_AUTOCHECKBOX, "Check", 100, 24);
        let radio = child(&owner, C::Button, BS_AUTORADIOBUTTON, "Radio", 100, 24);
        let combo = child(&owner, C::ComboBox, CBS_DROPDOWNLIST, "", 100, 100);
        let progress = child(&owner, C::Progress, PBS_SMOOTH, "", 100, 16);
        let trackbar = child(&owner, C::Trackbar, TBS_AUTOTICKS, "", 100, 40);
        for (hwnd, kind) in [
            (check, K::CheckBox),
            (radio, K::RadioButton),
            (combo, K::ComboBox),
            (progress, K::Progress),
            (trackbar, K::Trackbar),
        ] {
            assert!(Painter::attach(hwnd, kind));
        }
        // SAFETY: integer parameters only.
        unsafe {
            send_message(check, BM_SETCHECK, 1, 0);
            send_message(radio, BM_SETCHECK, 1, 0);
            send_message(progress, PBM_SETRANGE32, 0, 100);
            send_message(progress, PBM_SETPOS, 50, 0);
        }
        let process = pr_platform_win::win32::dialog_native::kernel32::get_current_process();
        let _ = Shot::capture(owner.hwnd); // warm-up paint
        let before = get_gui_resources(process, GR_GDIOBJECTS);
        for _ in 0..50 {
            let _ = Shot::capture(owner.hwnd);
        }
        assert_eq!(
            get_gui_resources(process, GR_GDIOBJECTS),
            before,
            "no GDI object leaks over 50 paints of every widget"
        );
    });
}
