//! MUT9 (2026-11-06): killers for the survivors of `dialogs/native/painter/widgets.rs` (check box, radio, combo box, progress bar, trackbar).
//!
//! Pure tests (no window) pin the layout functions; the native tests paint real offscreen child windows with `PrintWindow` (the fixture of the other
//! painter tests; the owner is shown far outside every monitor, nothing flashes) and read pixels. Where the drawing reads a control state that cannot be
//! produced reliably by a real control (`BM_GETSTATE` focus bit, trackbar rectangles and tick table, `WM_QUERYUISTATE`) a subclass installed AFTER the painter
//! answers those messages itself and logs every message it sees, so a mutated condition is detected by the pixels or by the messages that were (not) sent.
//! Evidence: `docs/reviews/P9-PAINTER-WIDGETS-MUT-2026-11-06.md`.
#![cfg(windows)]

mod common {
    pub mod dialogs_painter_fixture;
}

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use common::dialogs_painter_fixture::{OwnerWindow, Shot, child, client_on_screen, on_sta, rgb};
use pr_platform_win::dialogs::native::painter::NativeDialogChromePainter as Painter;
use pr_platform_win::dialogs::native::painter::geometry::{ChromePaintKind as K, keys, paint_scale, scaled};
use pr_platform_win::dialogs::native::painter::widgets::{
    CheckInput, ComboInput, TrackbarInput, check_layout, combo_layout, tick_xs, trackbar_layout,
};
use pr_platform_win::dialogs::native::template::DialogControlClass as C;
use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::comctl::common_controls::{def_subclass_proc, set_window_subclass};
use pr_platform_win::win32::control_constants::{
    CB_GETLBTEXT, CB_GETLBTEXTLEN, PBM_SETPOS, PBM_SETRANGE32, TBM_GETRANGEMAX, TBM_GETRANGEMIN, TBS_AUTOTICKS,
    TBS_NOTICKS,
};
use pr_platform_win::win32::dialog_chrome::{
    BM_GETSTATE, TBM_GETCHANNELRECT, TBM_GETNUMTICS, TBM_GETPTICS, TBM_GETTHUMBRECT, WM_QUERYUISTATE,
};
use pr_platform_win::win32::dialog_native::user32::{get_focus, send_message, set_focus};
use pr_platform_win::win32::dialog_native::{DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK};
use pr_platform_win::win32::structs::{Hwnd, Rect};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_PROTECTION_FLAGS, PAGE_READWRITE, VirtualAlloc,
    VirtualFree, VirtualProtect,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowLongPtrW, SetWindowPos,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn run(test: impl FnOnce() + Send + 'static) {
    on_sta(move || {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        test();
    });
}

fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
    Rect {
        left,
        top,
        right,
        bottom,
    }
}

// ---- pure ------------------------------------------------------------------------------------------------------------------------------------------------

fn check_input() -> CheckInput {
    CheckInput {
        width: 120,
        height: 20,
        scale: 1.0,
        radio: false,
        enabled: true,
        hot: false,
        checked: false,
        focused: true,
        multiline: false,
        left_text: false,
        text_height: 15,
        text_width: 60,
        hide_focus: false,
        hide_accel: false,
    }
}

/// `widgets.rs` 230:38 (`text_rect.right - text_rect.left` -> `+`). For every input without overflow the two expressions give the same focus rectangle
/// (`left >= 0`, so `min(right + left, w) >= min(right - left, w)` and the result is clamped to `width` either way); the difference is the overflow of
/// `right + left`, which panics in this profile (overflow checks on). The widest control that the unmutated code accepts is `i32::MAX - 2`: its
/// `left + shown + 2` is `i32::MAX`, while the mutant adds `right + left` first and overflows.
#[test]
fn check_layout_focus_rect_of_the_widest_control_does_not_overflow() {
    let layout = check_layout(CheckInput {
        width: i32::MAX - 2,
        text_width: i32::MAX,
        ..check_input()
    });
    let focus = layout.focus_rect.expect("focused and enabled");
    assert_eq!(focus.left, 18);
    assert_eq!(focus.right, i32::MAX - 2, "clamped to the width");
}

/// `widgets.rs` 224:20, 224:36 and 386:35, 386:48, 386:62 (`|` -> `^` in the constant flag expressions). `a | b == a ^ b` exactly when `a & b == 0`; the operands
/// are constants, so the whole domain is these four values: they share no bit, and the flag words are the sums.
#[test]
fn the_text_flag_constants_share_no_bit_so_or_and_xor_agree() {
    let singles = [DT_VCENTER, DT_SINGLELINE, DT_END_ELLIPSIS, DT_NOPREFIX];
    for (i, a) in singles.iter().enumerate() {
        for b in &singles[i + 1..] {
            assert_eq!(a & b, 0, "{a:#x} and {b:#x} overlap");
        }
    }
    let check = check_layout(CheckInput {
        multiline: false,
        hide_accel: false,
        ..check_input()
    });
    assert_eq!(check.text_flags, DT_VCENTER + DT_SINGLELINE + DT_END_ELLIPSIS);
    let combo = combo_layout(
        ComboInput {
            width: 100,
            height: 20,
            enabled: true,
            focused: false,
            dropped: false,
            hot: false,
            hide_focus: true,
        },
        1.0,
    );
    assert_eq!(
        combo.text_flags,
        DT_SINGLELINE + DT_VCENTER + DT_NOPREFIX + DT_END_ELLIPSIS
    );
    assert_ne!(
        DT_WORDBREAK & (DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS),
        DT_WORDBREAK
    );
}

/// `widgets.rs` 388:46 (`scaled(scale, 2) > 1` -> `>=`): the pen is 1 px wide when the scaled width is 1, which happens for a scale below 0.75.
#[test]
fn combo_arrow_pen_is_one_pixel_wide_when_two_scales_to_one() {
    let input = ComboInput {
        width: 100,
        height: 20,
        enabled: true,
        focused: false,
        dropped: false,
        hot: false,
        hide_focus: true,
    };
    for (scale, pen) in [(0.25, 1), (0.5, 1), (0.74, 1), (0.75, 2), (1.0, 2), (1.5, 2), (3.0, 2)] {
        assert_eq!(scaled(scale, 2) > 1, pen == 2, "scaled(scale, 2) at {scale}");
        assert_eq!(combo_layout(input, scale).arrow_pen_width, pen, "scale {scale}");
    }
}

/// `widgets.rs` 641:56 (`span / gaps` -> `span * gaps`, a TIMEOUT in the P9 run A: the step becomes 1 and a count of `i32::MAX` loops 2^31 times). Bounded killer:
/// count 1000 over a 10 px span has `span / gaps = 0.01001` (floored to 0.01 by the max), a step of 600 and 3 ticks; the mutant has a step of 1 and 1000 ticks.
#[test]
fn tick_xs_step_follows_the_span_per_gap_with_a_bounded_count() {
    let xs = tick_xs(0, 11, 1000, 0, 1000, true, None, 1.0);
    assert_eq!(xs, [0, 6, 10], "i = 0, 600 and the always drawn last tick");
    let xs = tick_xs(0, 17, 5, 0, 4, true, None, 1.0);
    assert_eq!(xs, [0, 8, 16], "step 2 over count 5");
}

/// `widgets.rs` 661:15 (`(count - 1) % step` -> `count / 1 % step`): with count 5 and step 2 the last loop tick (i = 4) already is the last tick, so no extra tick is
/// appended; the mutant appends a duplicate because `5 % 2 != 0`.
#[test]
fn tick_xs_does_not_repeat_the_last_tick_when_the_step_reaches_it() {
    let xs = tick_xs(0, 17, 5, 0, 4, true, None, 1.0);
    assert_eq!(xs.len(), 3, "{xs:?}");
    assert_eq!(xs.last(), Some(&16));
    // And when the step skips it, exactly one extra tick is appended (count 6, step 2: i = 0, 2, 4 and then 5 -> span).
    let xs = tick_xs(0, 17, 6, 0, 5, true, None, 1.0);
    assert_eq!(xs, [0, 6, 13, 16]);
}

/// `widgets.rs` 562:68 (`range_max > range_min` -> `>=` in `trackbar_layout`). Equivalent: when `range_max == range_min` the branch is entered but
/// `tick_xs` returns an empty list at its first statement (`max <= min`) for every count, mode, table and scale, so `ticks` stays empty exactly as in the
/// original, where the branch is skipped; for `range_max < range_min` both skip. No other input class differs.
#[test]
fn trackbar_layout_with_equal_range_ends_has_no_ticks_for_every_mode() {
    for auto_ticks in [false, true] {
        for count in [-3, 0, 1, 2, 3, 6, 1000, i32::MAX] {
            for scale in [0.25, 1.0, 2.5] {
                for inner in [None, Some(vec![1, 2, 3])] {
                    for min in [i32::MIN, -5, 0, 7, i32::MAX] {
                        let layout = trackbar_layout(&TrackbarInput {
                            channel: r(10, 14, 190, 18),
                            thumb: r(80, 6, 92, 26),
                            height: 40,
                            scale,
                            enabled: true,
                            hot: false,
                            no_ticks: false,
                            auto_ticks,
                            tick_count: count,
                            range_min: min,
                            range_max: min,
                            inner_ticks: inner.clone(),
                        });
                        assert!(layout.ticks.is_empty(), "count {count} min {min}");
                    }
                }
                assert!(tick_xs(10, 190, count, 5, 5, auto_ticks, Some(&[1]), scale).is_empty());
            }
        }
    }
}

// ---- native helpers --------------------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Reply {
    Value(isize),
    Rect(Rect),
}

/// A subclass installed after the painter: answers the listed messages and logs every message it receives.
struct Fake {
    replies: Mutex<Vec<(u32, Reply)>>,
    seen: Mutex<Vec<u32>>,
}

impl Fake {
    fn set(&self, message: u32, reply: Reply) {
        let mut replies = self.replies.lock().expect("replies");
        replies.retain(|(m, _)| *m != message);
        replies.push((message, reply));
    }

    fn count(&self, message: u32) -> usize {
        self.seen
            .lock()
            .expect("seen")
            .iter()
            .filter(|&&m| m == message)
            .count()
    }

    fn forget(&self) {
        self.seen.lock().expect("seen").clear();
    }
}

// SAFETY: a subclass procedure called by the OS with the parameters of the message; `data` is the leaked `Fake` of `install`.
unsafe extern "system" fn fake_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    data: usize,
) -> isize {
    // SAFETY: `data` is the address of the leaked `Fake` installed by `install`.
    let fake = unsafe { &*(data as *const Fake) };
    fake.seen.lock().expect("seen").push(message);
    let reply = fake
        .replies
        .lock()
        .expect("replies")
        .iter()
        .find(|(m, _)| *m == message)
        .map(|(_, reply)| *reply);
    match reply {
        Some(Reply::Value(value)) => value,
        Some(Reply::Rect(rect)) => {
            // SAFETY: the painter passes the address of a live local `RECT` for the rectangle messages (`TBM_GETCHANNELRECT`, `TBM_GETTHUMBRECT`).
            unsafe { *(lparam as *mut Rect) = rect };
            1
        }
        // SAFETY: called from a subclass procedure with its own message parameters.
        None => unsafe { def_subclass_proc(hwnd, message, wparam, lparam) },
    }
}

fn install(hwnd: Hwnd, replies: &[(u32, Reply)]) -> &'static Fake {
    let fake: &'static Fake = Box::leak(Box::new(Fake {
        replies: Mutex::new(replies.to_vec()),
        seen: Mutex::new(Vec::new()),
    }));
    // SAFETY: `fake_proc` lives for the process and `fake` is leaked, so the reference data outlives the window.
    let attached = unsafe { set_window_subclass(hwnd, fake_proc, 0x4D55_5439, std::ptr::from_ref(fake) as usize) };
    assert!(attached, "subclass the control");
    fake
}

/// The client pixels of `hwnd` in `shot`, row by row.
fn client_pixels(shot: &Shot, hwnd: Hwnd) -> (Vec<u32>, i32, i32) {
    let client = client_on_screen(hwnd);
    let (width, height) = (client.right - client.left, client.bottom - client.top);
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            pixels.push(shot.at(client.left + x, client.top + y));
        }
    }
    (pixels, width, height)
}

fn scale_of(hwnd: Hwnd) -> f64 {
    paint_scale(user32::get_dpi_for_window(hwnd))
}

const BS_AUTOCHECKBOX: u32 = 3;
const BS_AUTORADIOBUTTON: u32 = 9;
const BS_LEFTTEXT: u32 = 0x20;
const BM_SETCHECK: u32 = 0x00F1;
const BST_FOCUS_BIT: isize = 8;
const UISF_HIDEFOCUS_BIT: isize = 1;
const CBS_DROPDOWNLIST: u32 = 3;
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;
const PBS_SMOOTH: u32 = 1;
const WS_BORDER: u32 = 0x0080_0000;

// ---- check box -------------------------------------------------------------------------------------------------------------------------------------

fn check_layout_of(
    hwnd: Hwnd,
    radio: bool,
    checked: bool,
) -> pr_platform_win::dialogs::native::painter::widgets::CheckLayout {
    let client = client_on_screen(hwnd);
    check_layout(CheckInput {
        width: client.right - client.left,
        height: client.bottom - client.top,
        scale: scale_of(hwnd),
        radio,
        enabled: true,
        hot: false,
        checked,
        focused: false,
        multiline: false,
        left_text: user32::get_window_long_ptr(hwnd, -16) as u32 & BS_LEFTTEXT != 0,
        text_height: 15,
        text_width: 0,
        hide_focus: true,
        hide_accel: true,
    })
}

/// `widgets.rs` 297:47 (`fill` bottom `top + size`), 298:36 and 298:48 (`frame` right and bottom): the unchecked, enabled, cold check box (no text) is exactly a
/// `BORDER_STRONG` ring around an `INPUT` interior on the window colour. The whole client is compared, with the box on the left and on the right (`BS_LEFTTEXT`).
#[test]
#[ignore = "native: needs desktop"]
fn the_unchecked_check_box_is_a_border_ring_around_the_input_fill() {
    run(|| {
        for style in [BS_AUTOCHECKBOX, BS_AUTOCHECKBOX | BS_LEFTTEXT] {
            let owner = OwnerWindow::dark();
            let check = child(&owner, C::Button, style, "", 150, 24);
            assert!(Painter::attach(check, K::CheckBox));
            let layout = check_layout_of(check, false, false);
            let shot = Shot::capture(owner.hwnd);
            let (pixels, width, height) = client_pixels(&shot, check);
            let (left, top, size) = (layout.box_left, layout.box_top, layout.box_size);
            assert!(left + size <= width && top + size <= height);
            for y in 0..height {
                for x in 0..width {
                    let inside = x >= left && x < left + size && y >= top && y < top + size;
                    let edge = inside && (x == left || x == left + size - 1 || y == top || y == top + size - 1);
                    let expected = if edge {
                        rgb(keys::BORDER_STRONG)
                    } else if inside {
                        rgb(keys::INPUT)
                    } else {
                        rgb(keys::WINDOW)
                    };
                    assert_eq!(
                        pixels[(y * width + x) as usize],
                        expected,
                        "style {style:#x} pixel ({x}, {y}), box at ({left}, {top}) size {size}"
                    );
                }
            }
        }
    });
}

/// The extent of the run of pixels around `(cx, cy)` along a row (`horizontal`) or a column that satisfy `keep`.
fn run_around(
    pixels: &[u32],
    width: i32,
    height: i32,
    cx: i32,
    cy: i32,
    horizontal: bool,
    keep: impl Fn(u32) -> bool,
) -> (i32, i32) {
    let at = |x: i32, y: i32| pixels[(y * width + x) as usize];
    let (mut lo, mut hi) = if horizontal { (cx, cx) } else { (cy, cy) };
    let probe = |v: i32| if horizontal { at(v, cy) } else { at(cx, v) };
    let limit = if horizontal { width } else { height };
    assert!(keep(probe(lo)), "the centre pixel is part of the run");
    while lo > 0 && keep(probe(lo - 1)) {
        lo -= 1;
    }
    while hi + 1 < limit && keep(probe(hi + 1)) {
        hi += 1;
    }
    (lo, hi)
}

/// `widgets.rs` 289:54 (circle bottom `top + size`) and 293:35, 293:48, 293:69, 293:82, 293:89 (the dot ellipse `left + inset`, `top + inset`, `left + size - inset`,
/// `top + size - inset`): the extents of the circle and of the dot along the middle row and the middle column are the layout's box and inset, for a box at the
/// left edge (where `left * inset` is 0) and at the right edge (`BS_LEFTTEXT`).
#[test]
#[ignore = "native: needs desktop"]
fn the_checked_radio_circle_and_dot_span_the_layout_extents() {
    run(|| {
        for style in [BS_AUTORADIOBUTTON, BS_AUTORADIOBUTTON | BS_LEFTTEXT] {
            let owner = OwnerWindow::dark();
            let radio = child(&owner, C::Button, style, "", 150, 24);
            assert!(Painter::attach(radio, K::RadioButton));
            // SAFETY: integer parameters only.
            unsafe { send_message(radio, BM_SETCHECK, 1, 0) };
            let layout = check_layout_of(radio, true, true);
            let (dot_key, inset) = layout.dot.expect("a checked radio has a dot");
            let shot = Shot::capture(owner.hwnd);
            let (pixels, width, height) = client_pixels(&shot, radio);
            let (left, top, size) = (layout.box_left, layout.box_top, layout.box_size);
            let (cx, cy) = (left + size / 2, top + size / 2);
            let background = rgb(keys::WINDOW);
            // The circle: everything that is not the window colour.
            let horizontal = run_around(&pixels, width, height, cx, cy, true, |c| c != background);
            let vertical = run_around(&pixels, width, height, cx, cy, false, |c| c != background);
            assert_eq!(horizontal, (left, left + size - 1), "circle, style {style:#x}");
            assert_eq!(vertical, (top, top + size - 1), "circle, style {style:#x}");
            // The dot: the accent run around the centre (the INPUT ring separates it from the accent outline).
            let accent = rgb(dot_key);
            let horizontal = run_around(&pixels, width, height, cx, cy, true, |c| c == accent);
            let vertical = run_around(&pixels, width, height, cx, cy, false, |c| c == accent);
            assert_eq!(
                horizontal,
                (left + inset, left + size - inset - 1),
                "dot, style {style:#x}"
            );
            assert_eq!(vertical, (top + inset, top + size - inset - 1), "dot, style {style:#x}");
        }
    });
}

/// `widgets.rs` 278:31 (`&` -> `|`, `^`) and 278:43 (`!=` -> `==`): `focused = BM_GETSTATE & BST_FOCUS != 0`. The control answers `BM_GETSTATE` with `0` and with
/// exactly `BST_FOCUS` and `WM_QUERYUISTATE` with "show the focus cues": the picture with state 0 is the picture without cues, the picture with `BST_FOCUS`
/// differs from it (a focus rectangle), for the check box and the radio button.
#[test]
#[ignore = "native: needs desktop"]
fn the_focus_rectangle_follows_the_focus_bit_of_the_button_state() {
    run(|| {
        for (style, kind) in [(BS_AUTOCHECKBOX, K::CheckBox), (BS_AUTORADIOBUTTON, K::RadioButton)] {
            let owner = OwnerWindow::dark();
            let button = child(&owner, C::Button, style, "Check", 150, 24);
            assert!(Painter::attach(button, kind));
            let fake = install(
                button,
                &[
                    (BM_GETSTATE, Reply::Value(0)),
                    (WM_QUERYUISTATE, Reply::Value(UISF_HIDEFOCUS_BIT)),
                ],
            );
            let hidden = client_pixels(&Shot::capture(owner.hwnd), button).0;
            fake.set(WM_QUERYUISTATE, Reply::Value(0));
            let unfocused = client_pixels(&Shot::capture(owner.hwnd), button).0;
            assert_eq!(unfocused, hidden, "state 0 draws no focus rectangle (style {style:#x})");
            fake.set(BM_GETSTATE, Reply::Value(BST_FOCUS_BIT));
            let focused = client_pixels(&Shot::capture(owner.hwnd), button).0;
            assert_ne!(focused, hidden, "BST_FOCUS draws a focus rectangle (style {style:#x})");
            fake.set(WM_QUERYUISTATE, Reply::Value(UISF_HIDEFOCUS_BIT));
            let focused_but_hidden = client_pixels(&Shot::capture(owner.hwnd), button).0;
            assert_eq!(focused_but_hidden, hidden, "the cues are hidden (style {style:#x})");
        }
    });
}

// ---- combo box -------------------------------------------------------------------------------------------------------------------------------------

fn combo_with(owner: &OwnerWindow, items: &[&str], select: usize) -> Hwnd {
    let combo = child(owner, C::ComboBox, CBS_DROPDOWNLIST, "", 160, 120);
    for item in items {
        let text: Vec<u16> = item.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `text` is a NUL-terminated UTF-16 local that lives past the synchronous call; the control copies it.
        unsafe { send_message(combo, CB_ADDSTRING, 0, text.as_ptr() as isize) };
    }
    // SAFETY: integer parameters only (`usize::MAX` = -1 = no selection).
    unsafe { send_message(combo, CB_SETCURSEL, select, 0) };
    assert!(Painter::attach(combo, K::ComboBox));
    combo
}

/// The text area of the closed combo (inside the frame, left of the arrow button) as pixels.
fn combo_text_area(shot: &Shot, combo: Hwnd) -> Vec<u32> {
    let (pixels, width, height) = client_pixels(shot, combo);
    let layout = combo_layout(
        ComboInput {
            width,
            height,
            enabled: true,
            focused: false,
            dropped: false,
            hot: false,
            hide_focus: true,
        },
        scale_of(combo),
    );
    let mut area = Vec::new();
    for y in 2..height - 2 {
        for x in layout.text_rect.left..layout.text_rect.right {
            area.push(pixels[(y * width + x) as usize]);
        }
    }
    area
}

/// `widgets.rs` 441:14 (`index >= 0` -> `< 0`) and 778:5 (`combo_item_text` -> `String::new()` / `"xyzzy"`), 783:67 (`n > 0` -> `==`, `<`): the selected item is
/// drawn (the text area holds non-`INPUT` pixels), nothing is drawn without a selection, and two different item texts give two different pictures.
#[test]
#[ignore = "native: needs desktop"]
fn the_closed_combo_draws_the_text_of_the_selected_item() {
    run(|| {
        let owner = OwnerWindow::dark();
        let alpha = combo_with(&owner, &["Alpha", "Other"], 0);
        let beta = combo_with(&owner, &["Beta", "Other"], 0);
        let none = combo_with(&owner, &["Alpha", "Beta"], usize::MAX);
        let shot = Shot::capture(owner.hwnd);
        let input = rgb(keys::INPUT);
        let alpha_area = combo_text_area(&shot, alpha);
        let beta_area = combo_text_area(&shot, beta);
        let none_area = combo_text_area(&shot, none);
        assert!(alpha_area.iter().any(|&c| c != input), "Alpha is drawn");
        assert!(beta_area.iter().any(|&c| c != input), "Beta is drawn");
        assert!(none_area.iter().all(|&c| c == input), "no selection draws no text");
        assert_ne!(alpha_area, beta_area, "the drawn text is the text of the item");
        assert!(
            alpha_area.contains(&rgb(keys::TEXT_STRONG)) || alpha_area.iter().any(|&c| c != input),
            "the text colour is used"
        );
    });
}

/// `widgets.rs` 783:67 (`n > 0` -> `>=`): for a selected item with an EMPTY text the length is 0 and the original returns at once; the mutant would go on to
/// send `CB_GETLBTEXT`. The control logs the messages: `CB_GETLBTEXTLEN` is asked, `CB_GETLBTEXT` is not.
#[test]
#[ignore = "native: needs desktop"]
fn an_empty_item_text_is_not_fetched_with_cb_getlbtext() {
    run(|| {
        let owner = OwnerWindow::dark();
        let combo = combo_with(&owner, &[""], 0);
        let fake = install(combo, &[]);
        let shot = Shot::capture(owner.hwnd);
        assert!(
            fake.count(CB_GETLBTEXTLEN) > 0,
            "the length of the selected item is asked"
        );
        assert_eq!(fake.count(CB_GETLBTEXT), 0, "an empty item is not fetched");
        assert!(combo_text_area(&shot, combo).iter().all(|&c| c == rgb(keys::INPUT)));
        // A non-empty item IS fetched.
        let owner = OwnerWindow::dark();
        let combo = combo_with(&owner, &["Alpha"], 0);
        let fake = install(combo, &[]);
        let _ = Shot::capture(owner.hwnd);
        assert!(fake.count(CB_GETLBTEXT) > 0);
    });
}

/// The common-controls v5 progress bar (this test process has no v6 manifest) is created with `WS_EX_STATICEDGE`; this removes every edge style so that
/// `border_thickness_of` is 0 and the client is the whole window.
fn strip_edges(hwnd: Hwnd) {
    let edges = 0x0000_0200 | 0x0002_0000 | 0x0000_0100; // WS_EX_CLIENTEDGE | WS_EX_STATICEDGE | WS_EX_WINDOWEDGE
    // SAFETY: `hwnd` is a live window of this thread; plain integer arguments.
    unsafe {
        let handle = HWND(hwnd as *mut c_void);
        let ex = GetWindowLongPtrW(handle, GWL_EXSTYLE);
        SetWindowLongPtrW(handle, GWL_EXSTYLE, ex & !edges);
        let _ = SetWindowPos(
            handle,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

// ---- progress --------------------------------------------------------------------------------------------------------------------------------------

/// `widgets.rs` 476:46 (`> 0` -> `==`, `>=`), 477:27 and 478:8 (`!`), 490:56 (`height - inset` -> `+`): an unbordered progress bar has a 1 px `BORDER_STRONG`
/// frame at the client edge and the accent fill is inset by 1 on every side (the frame survives on the bottom row).
#[test]
#[ignore = "native: needs desktop"]
fn the_unbordered_progress_bar_keeps_its_one_pixel_frame_around_the_fill() {
    run(|| {
        let owner = OwnerWindow::dark();
        let progress = child(&owner, C::Progress, PBS_SMOOTH, "", 200, 16);
        strip_edges(progress);
        assert!(Painter::attach(progress, K::Progress));
        // SAFETY: integer parameters only.
        unsafe {
            send_message(progress, PBM_SETRANGE32, 0, 100);
            send_message(progress, PBM_SETPOS, 50, 0);
        }
        let shot = Shot::capture(owner.hwnd);
        let (pixels, width, height) = client_pixels(&shot, progress);
        assert_eq!((width, height), (200, 16));
        let at = |x: i32, y: i32| pixels[(y * width + x) as usize];
        let (strong, accent, surface) = (rgb(keys::BORDER_STRONG), rgb(keys::ACCENT), rgb(keys::SURFACE));
        // fill width = round_ties_even((200 - 2) * 0.5) = 99: columns 1..100, rows 1..15.
        assert_eq!(at(0, 8), strong, "left frame");
        assert_eq!(at(0, 0), strong, "corner");
        assert_eq!(at(1, 8), accent, "first filled column");
        assert_eq!(at(99, 8), accent, "last filled column");
        assert_eq!(at(100, 8), surface, "first column after the fill");
        assert_eq!(at(50, 0), strong, "top frame");
        assert_eq!(at(50, 1), accent, "first filled row");
        assert_eq!(at(50, 14), accent, "last filled row");
        assert_eq!(at(50, 15), strong, "bottom frame survives under the fill");
        assert_eq!(at(199, 8), strong, "right frame");
    });
}

/// `widgets.rs` 476:46 (`> 0` -> `<`): a progress bar with `WS_BORDER` has its frame painted at `WM_NCPAINT`, so the client is not framed again and the fill
/// starts at column 0.
#[test]
#[ignore = "native: needs desktop"]
fn the_bordered_progress_bar_paints_no_second_frame_in_its_client() {
    run(|| {
        let owner = OwnerWindow::dark();
        let progress = child(&owner, C::Progress, PBS_SMOOTH | WS_BORDER, "", 200, 16);
        assert!(Painter::attach(progress, K::Progress));
        // SAFETY: integer parameters only.
        unsafe {
            send_message(progress, PBM_SETRANGE32, 0, 100);
            send_message(progress, PBM_SETPOS, 50, 0);
        }
        let shot = Shot::capture(owner.hwnd);
        let (pixels, width, height) = client_pixels(&shot, progress);
        assert!(
            width < 200 && height < 16,
            "the border is outside the client: {width} x {height}"
        );
        let at = |x: i32, y: i32| pixels[(y * width + x) as usize];
        let (accent, surface) = (rgb(keys::ACCENT), rgb(keys::SURFACE));
        assert_eq!(at(0, 0), accent, "the fill starts at the corner");
        assert_eq!(at(0, height - 1), accent, "and reaches the bottom row");
        assert_eq!(at(width / 4, height / 2), accent);
        assert_eq!(at(width - 1, height / 2), surface, "the rest is the surface, no frame");
        assert_eq!(at(width - 1, 0), surface);
    });
}

// ---- trackbar --------------------------------------------------------------------------------------------------------------------------------------

/// Two pages from `VirtualAlloc`; the second is `PAGE_NOACCESS`; `values` end exactly at the page boundary. A read past the end faults.
struct GuardedTable {
    base: *mut c_void,
    table: *const i32,
}

static GUARD_BASE: AtomicUsize = AtomicUsize::new(0);
static GUARD_TOUCHED: AtomicBool = AtomicBool::new(false);
const PAGE: usize = 0x1000;

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut ExceptionRecord,
    address: *mut c_void,
    number_parameters: u32,
    information: [usize; 15],
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut c_void,
}

#[link(name = "kernel32")]
// SAFETY: the two kernel32 functions are declared with their documented signatures (handler pointer, `ULONG`, `PVOID`).
unsafe extern "system" {
    fn AddVectoredExceptionHandler(
        first: u32,
        handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32,
    ) -> *mut c_void;
    fn RemoveVectoredExceptionHandler(handle: *mut c_void) -> u32;
}

/// On a read of the guard page: remember it, make the page readable and resume the instruction (`EXCEPTION_CONTINUE_EXECUTION`, -1).
unsafe extern "system" fn guard_handler(info: *mut ExceptionPointers) -> i32 {
    // SAFETY: Windows passes valid exception pointers to a vectored handler.
    let record = unsafe { &*(*info).record };
    let base = GUARD_BASE.load(Ordering::SeqCst);
    if record.code == 0xC000_0005 && record.number_parameters >= 2 && base != 0 {
        let address = record.information[1];
        if address >= base && address < base + PAGE {
            GUARD_TOUCHED.store(true, Ordering::SeqCst);
            let mut old = PAGE_PROTECTION_FLAGS(0);
            // SAFETY: the page belongs to the test's own allocation.
            let _ = unsafe { VirtualProtect(base as *const c_void, PAGE, PAGE_READWRITE, &mut old) };
            return -1;
        }
    }
    0
}

impl GuardedTable {
    fn new(values: &[i32]) -> Self {
        assert!(values.len() * 4 <= PAGE);
        // SAFETY: a fresh anonymous two-page allocation owned by this value.
        let base = unsafe { VirtualAlloc(None, 2 * PAGE, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        assert!(!base.is_null());
        let guard = base as usize + PAGE;
        let table = (guard - values.len() * 4) as *mut i32;
        for (index, value) in values.iter().enumerate() {
            // SAFETY: inside the first (readable, writable) page.
            unsafe { table.add(index).write(*value) };
        }
        let mut old = PAGE_PROTECTION_FLAGS(0);
        // SAFETY: the second page of the allocation.
        unsafe { VirtualProtect(guard as *const c_void, PAGE, PAGE_NOACCESS, &mut old) }
            .expect("protect the guard page");
        GUARD_TOUCHED.store(false, Ordering::SeqCst);
        GUARD_BASE.store(guard, Ordering::SeqCst);
        Self {
            base,
            table: table.cast_const(),
        }
    }

    fn touched(&self) -> bool {
        GUARD_TOUCHED.load(Ordering::SeqCst)
    }
}

impl Drop for GuardedTable {
    fn drop(&mut self) {
        GUARD_BASE.store(0, Ordering::SeqCst);
        // SAFETY: releases the allocation made in `new`.
        let _ = unsafe { VirtualFree(self.base, 0, MEM_RELEASE) };
    }
}

const CHANNEL: Rect = Rect {
    left: 10,
    top: 14,
    right: 190,
    bottom: 18,
};
const THUMB: Rect = Rect {
    left: 80,
    top: 6,
    right: 92,
    bottom: 26,
};

struct Bar {
    owner: OwnerWindow,
    hwnd: Hwnd,
    fake: &'static Fake,
}

fn fake_trackbar(style: u32, count: i32, min: i32, max: i32, table: Option<&GuardedTable>) -> Bar {
    let owner = OwnerWindow::dark();
    let hwnd = child(&owner, C::Trackbar, style, "", 200, 40);
    assert!(Painter::attach(hwnd, K::Trackbar));
    let mut replies = vec![
        (TBM_GETCHANNELRECT, Reply::Rect(CHANNEL)),
        (TBM_GETTHUMBRECT, Reply::Rect(THUMB)),
        (TBM_GETNUMTICS, Reply::Value(count as isize)),
        (TBM_GETRANGEMIN, Reply::Value(min as isize)),
        (TBM_GETRANGEMAX, Reply::Value(max as isize)),
        (WM_QUERYUISTATE, Reply::Value(UISF_HIDEFOCUS_BIT)),
    ];
    if let Some(table) = table {
        replies.push((TBM_GETPTICS, Reply::Value(table.table as isize)));
    }
    let fake = install(hwnd, &replies);
    Bar { owner, hwnd, fake }
}

/// The x positions of the `TEXT_MUTED` tick pixels on the first row under the thumb.
fn tick_columns(bar: &Bar) -> (BTreeSet<i32>, Vec<u32>) {
    let shot = Shot::capture(bar.owner.hwnd);
    let (pixels, width, _height) = client_pixels(&shot, bar.hwnd);
    let row = THUMB.bottom + scaled(scale_of(bar.hwnd), 1);
    let columns = (0..width)
        .filter(|&x| pixels[(row * width + x) as usize] == rgb(keys::TEXT_MUTED))
        .collect();
    (columns, pixels)
}

fn expected_columns(style: u32, count: i32, min: i32, max: i32, inner: Option<Vec<i32>>, hwnd: Hwnd) -> BTreeSet<i32> {
    let client = client_on_screen(hwnd);
    let layout = trackbar_layout(&TrackbarInput {
        channel: CHANNEL,
        thumb: THUMB,
        height: client.bottom - client.top,
        scale: scale_of(hwnd),
        enabled: true,
        hot: false,
        no_ticks: style & TBS_NOTICKS != 0,
        auto_ticks: style & TBS_AUTOTICKS != 0,
        tick_count: count,
        range_min: min,
        range_max: max,
        inner_ticks: inner,
    });
    layout.ticks.iter().map(|tick| tick.left).collect()
}

/// `widgets.rs` 686:28, 686:44 (`style & TBS_AUTOTICKS != 0`), 697 (the condition that reads the tick table) and 703:52 (`tick_count - 2`): a trackbar WITHOUT
/// `TBS_AUTOTICKS` draws ticks at the positions of its table (here 1, 2, 7, 9 of 0..10, not evenly spaced); with `TBS_AUTOTICKS` it draws evenly spaced ticks
/// and ignores the table. The table sits against a no-access page: reading more than `count - 2` values faults (the handler records it instead of crashing).
#[test]
#[ignore = "native: needs desktop"]
fn custom_ticks_are_drawn_at_the_table_positions_and_auto_ticks_are_even() {
    run(|| {
        let handler = {
            // SAFETY: `guard_handler` is a plain function; it is removed at the end of the test.
            unsafe { AddVectoredExceptionHandler(1, guard_handler) }
        };
        assert!(!handler.is_null());
        let table = GuardedTable::new(&[1, 2, 7, 9]);
        // custom
        let bar = fake_trackbar(0, 6, 0, 10, Some(&table));
        let (columns, _) = tick_columns(&bar);
        let custom = expected_columns(0, 6, 0, 10, Some(vec![1, 2, 7, 9]), bar.hwnd);
        let even = expected_columns(TBS_AUTOTICKS, 6, 0, 10, None, bar.hwnd);
        assert_ne!(custom, even, "the probe needs unevenly spaced ticks");
        assert_eq!(columns, custom, "custom ticks at the table positions");
        assert!(!table.touched(), "no read past the count - 2 values of the table");
        drop(bar);
        // auto
        let bar = fake_trackbar(TBS_AUTOTICKS, 6, 0, 10, Some(&table));
        let (columns, _) = tick_columns(&bar);
        let auto = expected_columns(TBS_AUTOTICKS, 6, 0, 10, Some(vec![1, 2, 7, 9]), bar.hwnd);
        assert_eq!(columns, auto, "auto ticks are evenly spaced");
        assert_eq!(columns, even);
        assert_eq!(bar.fake.count(TBM_GETPTICS), 0, "auto ticks do not read the table");
        drop(bar);
        // SAFETY: the handle returned by `AddVectoredExceptionHandler` above.
        let _ = unsafe { RemoveVectoredExceptionHandler(handler) };
    });
}

/// `widgets.rs` 697:26, 697:36, 697:39, 697:51, 697:65, 697:70, 697:83: the table is requested (`TBM_GETPTICS`) exactly when the ticks are shown, not automatic,
/// there are at least two and `max > min`. The control logs the messages, so every operand of the condition is separated from the others.
#[test]
#[ignore = "native: needs desktop"]
fn the_tick_table_is_requested_only_for_custom_visible_ticks_with_a_range() {
    run(|| {
        let table = GuardedTable::new(&[1, 2, 7, 9]);
        let asked = |style: u32, count: i32, min: i32, max: i32| {
            let bar = fake_trackbar(style, count, min, max, Some(&table));
            let _ = Shot::capture(bar.owner.hwnd);
            bar.fake.count(TBM_GETPTICS)
        };
        assert!(asked(0, 6, 0, 10) > 0, "custom ticks with a range");
        assert!(asked(0, 2, 0, 10) > 0, "count 2 is enough (two ends only)");
        assert_eq!(asked(TBS_NOTICKS, 6, 0, 10), 0, "TBS_NOTICKS");
        assert_eq!(asked(TBS_AUTOTICKS, 6, 0, 10), 0, "TBS_AUTOTICKS");
        assert_eq!(asked(0, 6, 5, 5), 0, "max == min");
        assert_eq!(asked(0, 6, 5, 4), 0, "max < min");
        assert_eq!(asked(0, 1, 0, 10), 0, "count 1");
        assert_eq!(asked(0, 0, 0, 10), 0, "count 0");
        assert_eq!(asked(TBS_NOTICKS | TBS_AUTOTICKS, 6, 0, 10), 0, "both flags");
    });
}

/// `widgets.rs` 703:52 (`tick_count - 2` -> `/ 2`): five inner ticks are all used (a halved count would leave the last ones at the range minimum).
#[test]
#[ignore = "native: needs desktop"]
fn every_inner_tick_of_the_table_is_read() {
    run(|| {
        let table = GuardedTable::new(&[1, 2, 3, 7, 9]);
        let bar = fake_trackbar(0, 7, 0, 10, Some(&table));
        let (columns, _) = tick_columns(&bar);
        let expected = expected_columns(0, 7, 0, 10, Some(vec![1, 2, 3, 7, 9]), bar.hwnd);
        assert_eq!(columns, expected);
        assert!(!table.touched());
    });
}

/// `widgets.rs` 762:20 (`==` -> `!=`), 762:28 (`&&` -> `||`), 762:31 (`!`): the trackbar's focus rectangle is drawn exactly when the control has the focus
/// AND the focus cues are visible. The cues come from `WM_QUERYUISTATE` (answered by the control), the focus is the real one of the STA thread.
#[test]
#[ignore = "native: needs desktop"]
fn the_trackbar_focus_rectangle_needs_the_focus_and_visible_cues() {
    run(|| {
        let bar = fake_trackbar(TBS_AUTOTICKS, 6, 0, 10, None);
        // A second focusable window to move the focus away.
        let other = child(&bar.owner, C::Button, BS_AUTOCHECKBOX, "x", 20, 20);
        let image = |bar: &Bar| client_pixels(&Shot::capture(bar.owner.hwnd), bar.hwnd).0;
        let _ = set_focus(other);
        assert_eq!(get_focus(), other, "the offscreen owner takes the focus");
        bar.fake.set(WM_QUERYUISTATE, Reply::Value(UISF_HIDEFOCUS_BIT));
        let baseline = image(&bar);
        bar.fake.set(WM_QUERYUISTATE, Reply::Value(0));
        assert_eq!(image(&bar), baseline, "no focus: cues visible, no rectangle");
        let _ = set_focus(bar.hwnd);
        assert_eq!(get_focus(), bar.hwnd, "the trackbar takes the focus");
        bar.fake.set(WM_QUERYUISTATE, Reply::Value(UISF_HIDEFOCUS_BIT));
        assert_eq!(image(&bar), baseline, "focus with hidden cues: no rectangle");
        bar.fake.set(WM_QUERYUISTATE, Reply::Value(0));
        assert_ne!(image(&bar), baseline, "focus with visible cues: the rectangle");
        bar.fake.forget();
    });
}
