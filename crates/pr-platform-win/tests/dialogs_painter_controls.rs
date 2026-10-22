//! Unit W8-18m (contract `contracts/W8-18m.md`): the shared drawing of the chrome painter and the push button, `painter::controls`.
//! Pure tests run everywhere; the native tests are `#[ignore = "native: needs desktop"]`, measure real hidden windows with `PrintWindow` and paint push
//! buttons only (the other kinds belong to W8-18n). Every native test of this file takes `SERIAL` (the GDI handle counts are per process).
//!
//! The comctl32 v5 and v6 legs are separate tests whose names sort v5 first: run with `--test-threads=1` the classic leg really runs before the manifest is
//! activated in the process.

mod common {
    pub mod dialogs_painter_fixture;
}

use std::sync::Mutex;

use common::dialogs_painter_fixture::{OwnerWindow, Shot, analyse, child, client_on_screen, on_sta, rgb};
use pr_platform_win::dialogs::native::painter::NativeDialogChromePainter as Painter;
use pr_platform_win::dialogs::native::painter::controls::{
    PushButtonDraw, PushButtonInput, adjust_list_view_custom_draw, frame_rects, push_button_draw,
};
use pr_platform_win::dialogs::native::painter::geometry::{ChromePaintKind as K, color, keys};
use pr_platform_win::dialogs::native::template::DialogControlClass as C;
use pr_platform_win::win32::activation_context::ActivationScope;
use pr_platform_win::win32::comctl::SHELL_MANIFEST;
use pr_platform_win::win32::dialog_native::user32::{enable_window, get_gui_resources, send_message};
use pr_platform_win::win32::structs::{Hwnd, Rect};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_FLAGS, LIST_VIEW_ITEM_STATE_FLAGS, LVCOLUMNW, LVCOLUMNW_MASK, LVITEMW, NMCUSTOMDRAW_DRAW_STAGE,
    NMCUSTOMDRAW_DRAW_STATE_FLAGS, NMLVCUSTOMDRAW,
};
use windows::core::PWSTR;

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

const DT_ENABLED_SINGLE: u32 = 0x8025; // DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS
const DT_ENABLED_MULTI: u32 = 0x15; // DT_CENTER | DT_VCENTER | DT_WORDBREAK
const DT_HIDEPREFIX: u32 = 0x0010_0000;

fn input(flags: &[&str]) -> PushButtonInput {
    let has = |name: &str| flags.contains(&name);
    PushButtonInput {
        enabled: has("enabled"),
        pressed: has("pressed"),
        focused: has("focused"),
        is_default: has("default"),
        latched: has("latched"),
        hot: has("hot"),
        multiline: has("multiline"),
        hide_accel: has("hide_accel"),
        hide_focus: has("hide_focus"),
    }
}

// ---- pure: push_button_draw ----------------------------------------------------------------------------------------------------------------------------

#[test]
fn push_button_draw_worked_examples() {
    // C#: NativeDialogChromePainter.DrawPushButton, hand-computed.
    let rest = push_button_draw(input(&["enabled"]), 100, 24, 1.0);
    assert_eq!(
        rest,
        PushButtonDraw {
            back_key: keys::BUTTON,
            border_key: keys::BORDER_STRONG,
            text_key: keys::TEXT_STRONG,
            radius: 6,
            text_rect: r(4, 0, 96, 24),
            text_flags: DT_ENABLED_SINGLE,
            focus_rect: None,
        }
    );
    let busy = push_button_draw(input(&["enabled", "pressed", "focused", "default"]), 100, 24, 1.0);
    assert_eq!(busy.back_key, keys::BUTTON_PRESSED);
    assert_eq!(busy.border_key, keys::TEXT_SOFT, "default wins over focused");
    assert_eq!(busy.text_rect, r(5, 1, 97, 25));
    assert_eq!(busy.focus_rect, Some(r(3, 3, 97, 21)));
    let scaled = push_button_draw(input(&["enabled", "focused"]), 100, 24, 1.5);
    assert_eq!(scaled.radius, 9);
    assert_eq!(scaled.text_rect, r(6, 0, 94, 24));
    assert_eq!(
        scaled.focus_rect,
        Some(r(4, 4, 96, 20)),
        "S(3) = round_ties_even(4.5) = 4"
    );
    assert_eq!(scaled.border_key, keys::ACCENT);
    let disabled = push_button_draw(input(&["hot", "pressed", "focused"]), 100, 24, 1.0);
    assert_eq!(disabled.back_key, keys::BUTTON, "disabled ignores hot and pressed");
    assert_eq!(disabled.border_key, keys::BORDER);
    assert_eq!(disabled.text_key, keys::TEXT_MUTED);
    assert_eq!(disabled.focus_rect, None, "no focus rectangle on a disabled button");
}

#[test]
fn push_button_draw_key_table() {
    // All 2^6 combinations against an independent ladder (C#: the three ternaries of DrawPushButton).
    for bits in 0..64u32 {
        let enabled = bits & 1 != 0;
        let pressed = bits & 2 != 0;
        let latched = bits & 4 != 0;
        let hot = bits & 8 != 0;
        let is_default = bits & 16 != 0;
        let focused = bits & 32 != 0;
        let draw = push_button_draw(
            PushButtonInput {
                enabled,
                pressed,
                focused,
                is_default,
                latched,
                hot,
                ..Default::default()
            },
            100,
            24,
            1.0,
        );
        let back = if !enabled {
            "Dark.Button"
        } else if pressed || latched {
            "Dark.ButtonPressed"
        } else if hot {
            "Dark.ButtonHover"
        } else {
            "Dark.Button"
        };
        let border = if !enabled {
            "Dark.Border"
        } else if is_default {
            "Dark.TextSoft"
        } else if focused {
            "Dark.Accent"
        } else {
            "Dark.BorderStrong"
        };
        assert_eq!(draw.back_key, back, "back {bits:06b}");
        assert_eq!(draw.border_key, border, "border {bits:06b}");
        assert_eq!(
            draw.text_key,
            if enabled { "Dark.TextStrong" } else { "Dark.TextMuted" },
            "text {bits:06b}"
        );
        assert_eq!(draw.focus_rect.is_some(), focused && enabled, "focus {bits:06b}");
        // `latched` alone never shifts the text; only `pressed` does.
        assert_eq!(draw.text_rect.top, i32::from(pressed), "offset {bits:06b}");
    }
}

#[test]
fn push_button_draw_rectangles_scale_1_0_1_25_1_5_2_0() {
    // (scale, radius S(6), text inset S(4), focus inset S(3))
    for (scale, radius, inset, focus) in [(1.0, 6, 4, 3), (1.25, 8, 5, 4), (1.5, 9, 6, 4), (2.0, 12, 8, 6)] {
        let plain = push_button_draw(input(&["enabled", "focused"]), 100, 24, scale);
        assert_eq!(plain.radius, radius, "radius at {scale}");
        assert_eq!(plain.text_rect, r(inset, 0, 100 - inset, 24), "text at {scale}");
        assert_eq!(
            plain.focus_rect,
            Some(r(focus, focus, 100 - focus, 24 - focus)),
            "focus at {scale}"
        );
        let pressed = push_button_draw(input(&["enabled", "pressed", "focused"]), 100, 24, scale);
        assert_eq!(
            pressed.text_rect,
            r(inset + 1, 1, 100 - inset + 1, 25),
            "pressed text at {scale}"
        );
        // The focus rectangle does not move when pressed.
        assert_eq!(pressed.focus_rect, plain.focus_rect);
    }
}

#[test]
fn push_button_draw_multiline_and_hide_accel_flags() {
    assert_eq!(
        push_button_draw(input(&["enabled"]), 100, 24, 1.0).text_flags,
        DT_ENABLED_SINGLE
    );
    assert_eq!(
        push_button_draw(input(&["enabled", "multiline"]), 100, 24, 1.0).text_flags,
        DT_ENABLED_MULTI
    );
    assert_eq!(
        push_button_draw(input(&["enabled", "hide_accel"]), 100, 24, 1.0).text_flags,
        DT_ENABLED_SINGLE | DT_HIDEPREFIX
    );
    assert_eq!(
        push_button_draw(input(&["enabled", "multiline", "hide_accel"]), 100, 24, 1.0).text_flags,
        DT_ENABLED_MULTI | DT_HIDEPREFIX
    );
    // hide_focus removes the focus rectangle only.
    let hidden = push_button_draw(input(&["enabled", "focused", "hide_focus"]), 100, 24, 1.0);
    assert_eq!(hidden.focus_rect, None);
    assert_eq!(hidden.border_key, keys::ACCENT, "the focused border stays");
}

#[test]
fn push_button_draw_never_panics_on_degenerate_sizes() {
    for (w, h) in [(0, 0), (1, 1), (-5, -5), (i32::MAX / 4, i32::MAX / 4)] {
        let _ = push_button_draw(input(&["enabled", "pressed", "focused"]), w, h, 1.0);
        let _ = push_button_draw(input(&["enabled"]), w, h, 2.0);
    }
}

// ---- pure: frame_rects ---------------------------------------------------------------------------------------------------------------------------------

#[test]
fn frame_rects_arithmetic() {
    // C#: Frame(hdc, l, t, r, b, color, thickness): top, bottom, left, right.
    assert_eq!(
        frame_rects(0, 0, 100, 24, 1),
        [r(0, 0, 100, 1), r(0, 23, 100, 24), r(0, 0, 1, 24), r(99, 0, 100, 24)]
    );
    assert_eq!(
        frame_rects(5, 6, 50, 40, 2),
        [r(5, 6, 50, 8), r(5, 38, 50, 40), r(5, 6, 7, 40), r(48, 6, 50, 40)]
    );
    // A thickness larger than half the box: the rectangles overlap; nothing panics.
    let thick = frame_rects(0, 0, 10, 10, 8);
    assert_eq!(
        thick,
        [r(0, 0, 10, 8), r(0, 2, 10, 10), r(0, 0, 8, 10), r(2, 0, 10, 10)]
    );
    // Thickness 0 gives empty (degenerate) rectangles.
    assert_eq!(
        frame_rects(0, 0, 10, 10, 0),
        [r(0, 0, 10, 0), r(0, 10, 10, 10), r(0, 0, 0, 10), r(10, 0, 10, 10)]
    );
}

// ---- pure: adjust_list_view_custom_draw -------------------------------------------------------------------------------------------------------------

fn custom_draw(stage: u32, item_state: u32) -> NMLVCUSTOMDRAW {
    let mut nm = NMLVCUSTOMDRAW::default();
    nm.nmcd.dwDrawStage = NMCUSTOMDRAW_DRAW_STAGE(stage);
    nm.nmcd.uItemState = NMCUSTOMDRAW_DRAW_STATE_FLAGS(item_state);
    nm
}

#[test]
fn adjust_list_view_custom_draw_without_a_list_view() {
    // Twin of the oracle `custom_draw` rows: hwndFrom = 0, so LVM_GETITEMSTATE is not asked and the row counts as not selected.
    let mut nm = custom_draw(1, 0x201);
    assert_eq!(adjust_list_view_custom_draw(&mut nm, 0), 32);
    assert_eq!((nm.nmcd.uItemState.0, nm.clrText.0, nm.clrTextBk.0), (0x201, 0, 0));
    let mut nm = custom_draw(1, 0x201);
    assert_eq!(
        adjust_list_view_custom_draw(&mut nm, 4),
        4,
        "CDRF_SKIPDEFAULT is kept as is"
    );
    let mut nm = custom_draw(1, 0x201);
    assert_eq!(adjust_list_view_custom_draw(&mut nm, 2), 34);
    for stage in [0x10001u32, 0x30001, 0x10002, 0x2, 0x4, 0x10000, 0x20001] {
        let mut nm = custom_draw(stage, 0x201);
        assert_eq!(adjust_list_view_custom_draw(&mut nm, 32), 32, "{stage:#x}");
        assert_eq!(
            (nm.nmcd.uItemState.0, nm.clrText.0, nm.clrTextBk.0),
            (0x201, 0, 0),
            "{stage:#x}"
        );
    }
}

// ---- native ----------------------------------------------------------------------------------------------------------------------------------------------

const WM_MOUSEMOVE: u32 = 0x0200;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_PRINTCLIENT: u32 = 0x0318;
const WM_SETFONT: u32 = 0x0030;
const BM_SETSTATE: u32 = 0x00F3;
const BM_SETCHECK: u32 = 0x00F1;
const BS_DEFPUSHBUTTON: u32 = 1;
const BS_AUTOCHECKBOX: u32 = 3;
const BS_PUSHLIKE: u32 = 0x1000;
const GR_GDIOBJECTS: u32 = 0;

fn run(v6: bool, test: impl FnOnce() + Send + 'static) {
    on_sta(move || {
        let _serial = serial();
        let _activation = if v6 {
            Some(ActivationScope::from_manifest_text(SHELL_MANIFEST).expect("activate comctl32 v6"))
        } else {
            None
        };
        test();
    });
}

/// The colour of the pixel at (dx, dy) of the client area of `hwnd`, measured with `PrintWindow`.
fn pixel(hwnd: Hwnd, dx: i32, dy: i32) -> u32 {
    let shot = Shot::capture(hwnd);
    let client = client_on_screen(hwnd);
    shot.at(client.left + dx, client.top + dy)
}

fn push_button(owner: &OwnerWindow, style: u32, text: &str) -> Hwnd {
    let button = child(owner, C::Button, style, text, 120, 30);
    assert!(Painter::attach(button, K::PushButton));
    button
}

fn fill_follows_the_state() {
    let owner = OwnerWindow::dark();
    let button = push_button(&owner, 0, "OK");
    // Inside the rounded rectangle, left of the centred text.
    let probe = |hwnd: Hwnd| pixel(hwnd, 8, 15);
    assert_eq!(probe(button), rgb(keys::BUTTON), "rest");
    // SAFETY: integer parameters only.
    unsafe { send_message(button, WM_MOUSEMOVE, 0, 0x000F_0008) };
    assert_eq!(probe(button), rgb(keys::BUTTON_HOVER), "hover");
    // SAFETY: integer parameters only.
    unsafe { send_message(button, WM_MOUSELEAVE, 0, 0) };
    assert_eq!(probe(button), rgb(keys::BUTTON), "left again");
    // SAFETY: integer parameters only.
    unsafe { send_message(button, BM_SETSTATE, 1, 0) };
    assert_eq!(probe(button), rgb(keys::BUTTON_PRESSED), "pressed");
    // SAFETY: integer parameters only.
    unsafe { send_message(button, BM_SETSTATE, 0, 0) };
    assert_eq!(probe(button), rgb(keys::BUTTON), "released");
    enable_window(button, false);
    assert_eq!(probe(button), rgb(keys::BUTTON), "disabled");
    let (_, _, contrast, _) = analyse(&Shot::capture(button), button);
    assert!(contrast >= 4.5, "disabled text contrast {contrast:.2}");
}

#[test]
#[ignore = "native: needs desktop"]
fn push_button_fill_follows_the_state_rest_hover_pressed_disabled_v5_classic() {
    // C#: NativeDialogChromeTests.PushButton_FillFollowsTheState_Rest_Hover_Pressed_Disabled (without comctl32 v6)
    run(false, fill_follows_the_state);
}

#[test]
#[ignore = "native: needs desktop"]
fn push_button_fill_follows_the_state_rest_hover_pressed_disabled_v6_manifest() {
    // C#: NativeDialogChromeTests.PushButton_FillFollowsTheState_Rest_Hover_Pressed_Disabled (with comctl32 v6)
    run(true, fill_follows_the_state);
}

#[test]
#[ignore = "native: needs desktop"]
fn default_button_has_the_text_soft_border() {
    run(false, || {
        let owner = OwnerWindow::dark();
        let button = push_button(&owner, BS_DEFPUSHBUTTON, "OK");
        // The top edge, in the middle of the button: the 1 px outline of the rounded rectangle.
        assert_eq!(pixel(button, 60, 0), rgb(keys::TEXT_SOFT));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn toggle_push_like_button_is_pressed_looking_when_checked() {
    run(false, || {
        let owner = OwnerWindow::dark();
        let toggle = push_button(&owner, BS_AUTOCHECKBOX | BS_PUSHLIKE, "Toggle");
        assert_eq!(pixel(toggle, 8, 15), rgb(keys::BUTTON), "unchecked");
        // SAFETY: integer parameters only.
        unsafe { send_message(toggle, BM_SETCHECK, 1, 0) };
        assert_eq!(pixel(toggle, 8, 15), rgb(keys::BUTTON_PRESSED), "checked = latched");
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn hover_sets_and_clears_with_mouse_move_and_leave() {
    run(false, || {
        let owner = OwnerWindow::dark();
        let button = push_button(&owner, 0, "OK");
        assert_eq!(pixel(button, 8, 15), rgb(keys::BUTTON));
        // SAFETY: integer parameters only.
        unsafe { send_message(button, WM_MOUSEMOVE, 0, 0x000F_0008) };
        assert_eq!(pixel(button, 8, 15), rgb(keys::BUTTON_HOVER));
        // A second move while hot changes nothing.
        // SAFETY: integer parameters only.
        unsafe { send_message(button, WM_MOUSEMOVE, 0, 0x000F_0009) };
        assert_eq!(pixel(button, 8, 15), rgb(keys::BUTTON_HOVER));
        // SAFETY: integer parameters only.
        unsafe { send_message(button, WM_MOUSELEAVE, 0, 0) };
        assert_eq!(pixel(button, 8, 15), rgb(keys::BUTTON));
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn paint_to_draws_an_invisible_control_into_a_memory_dc() {
    // C#: PaintTo double buffering. The owner is never shown, so the control is invisible; WM_PRINTCLIENT into a memory DC must still paint it.
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetPixel, HGDIOBJ, ReleaseDC,
        SelectObject,
    };
    run(false, || {
        let owner = OwnerWindow::dark();
        let button = push_button(&owner, 0, "OK");
        // SAFETY: the screen DC is released, the memory DC and the bitmap are deleted before returning; the message carries the memory DC as a value.
        let probe = unsafe {
            let screen = GetDC(None);
            let memory = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, 120, 30);
            let old = SelectObject(memory, HGDIOBJ(bitmap.0));
            let answer = send_message(button, WM_PRINTCLIENT, memory.0 as usize, 0);
            let colour = GetPixel(memory, 8, 15).0;
            SelectObject(memory, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            (answer, colour)
        };
        assert_eq!(probe.0, 0, "WM_PRINTCLIENT answers 0");
        assert_eq!(
            probe.1,
            color(keys::BUTTON),
            "the button was painted into the memory DC"
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn fifty_paints_do_not_leak_gdi_objects() {
    run(false, || {
        let owner = OwnerWindow::dark();
        let button = push_button(&owner, BS_DEFPUSHBUTTON, "OK");
        let process = pr_platform_win::win32::dialog_native::kernel32::get_current_process();
        let _ = Shot::capture(button); // warm-up paint
        let before = get_gui_resources(process, GR_GDIOBJECTS);
        for _ in 0..50 {
            let _ = Shot::capture(button);
        }
        assert_eq!(
            get_gui_resources(process, GR_GDIOBJECTS),
            before,
            "no GDI object leaks over 50 paints"
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn font_is_selected_back_after_draw() {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DEFAULT_GUI_FONT, DeleteDC, DeleteObject, GetCurrentObject, GetDC,
        GetStockObject, HGDIOBJ, OBJ_FONT, ReleaseDC, SelectObject,
    };
    run(false, || {
        let owner = OwnerWindow::dark();
        let button = push_button(&owner, 0, "OK");
        // SAFETY: the stock font handle is a value; the screen and memory DCs and the bitmap are released before returning.
        unsafe {
            let font = GetStockObject(DEFAULT_GUI_FONT);
            send_message(button, WM_SETFONT, font.0 as usize, 0);
            let screen = GetDC(None);
            let memory = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, 120, 30);
            let old = SelectObject(memory, HGDIOBJ(bitmap.0));
            let font_before = GetCurrentObject(memory, OBJ_FONT);
            send_message(button, WM_PRINTCLIENT, memory.0 as usize, 0);
            let font_after = GetCurrentObject(memory, OBJ_FONT);
            assert_eq!(font_before, font_after, "the control font is selected back");
            SelectObject(memory, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
        }
    });
}

// ---- native: the selected list view row ---------------------------------------------------------------------------------------------------------------

const LVM_SETBKCOLOR: u32 = 0x1001;
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1036;
const LVM_SETTEXTCOLOR: u32 = 0x1024;
const LVM_SETTEXTBKCOLOR: u32 = 0x1026;
const LVM_INSERTCOLUMNW: u32 = 0x1061;
const LVM_INSERTITEMW: u32 = 0x104D;
const LVM_SETITEMSTATE: u32 = 0x102B;
const LVM_GETITEMRECT: u32 = 0x100E;
const LVM_SETCOLUMNWIDTH: u32 = 0x101E;
const LVS_REPORT: u32 = 1;
const LVS_SHOWSELALWAYS: u32 = 8;
const LVIS_SELECTED: u32 = 2;
const WM_NOTIFY: u32 = 0x004E;

/// A report-mode list view configured like `ListViewControl::configure(lv, Dark.Input, Dark.TextStrong)`, with `rows` rows.
fn list_view(owner: &OwnerWindow, rows: i32) -> Hwnd {
    let lv = child(owner, C::ListView, LVS_REPORT | LVS_SHOWSELALWAYS, "", 300, 200);
    // SAFETY: integer parameters only.
    unsafe {
        // LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER, as `ListViewControl::configure`: the selection colour fills the whole row.
        send_message(lv, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, 0x20);
        send_message(lv, LVM_SETBKCOLOR, 0, color(keys::INPUT) as isize);
        send_message(lv, LVM_SETTEXTBKCOLOR, 0, color(keys::INPUT) as isize);
        send_message(lv, LVM_SETTEXTCOLOR, 0, color(keys::TEXT_STRONG) as isize);
    }
    let mut title: Vec<u16> = "Column\0".encode_utf16().collect();
    let column = LVCOLUMNW {
        mask: LVCOLUMNW_MASK(0x4 | 0x2), // LVCF_TEXT | LVCF_WIDTH
        cx: 280,
        pszText: PWSTR(title.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: `column` and `title` are live locals for the synchronous call; the control copies the text.
    unsafe { send_message(lv, LVM_INSERTCOLUMNW, 0, std::ptr::from_ref(&column) as isize) };
    // SAFETY: integer parameters only.
    unsafe { send_message(lv, LVM_SETCOLUMNWIDTH, 0, 280) };
    for row in 0..rows {
        let mut text: Vec<u16> = format!("Row {row}\0").encode_utf16().collect();
        let item = LVITEMW {
            mask: LIST_VIEW_ITEM_FLAGS(1), // LVIF_TEXT
            iItem: row,
            pszText: PWSTR(text.as_mut_ptr()),
            ..Default::default()
        };
        // SAFETY: `item` and `text` are live locals for the synchronous call; the control copies the text.
        unsafe { send_message(lv, LVM_INSERTITEMW, 0, std::ptr::from_ref(&item) as isize) };
    }
    lv
}

fn select_row(lv: Hwnd, row: usize) {
    let item = LVITEMW {
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED),
        state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED),
        ..Default::default()
    };
    // SAFETY: `item` is a live local for the synchronous call.
    unsafe { send_message(lv, LVM_SETITEMSTATE, row, std::ptr::from_ref(&item) as isize) };
}

fn row_rect(lv: Hwnd, row: usize) -> Rect {
    let mut rect = Rect::default(); // left = LVIR_BOUNDS (0)
    // SAFETY: `rect` is a live local RECT for the synchronous call.
    let ok = unsafe { send_message(lv, LVM_GETITEMRECT, row, std::ptr::from_mut(&mut rect) as isize) };
    assert_ne!(ok, 0, "LVM_GETITEMRECT");
    rect
}

fn selected_row_uses_the_selection_colour() {
    let owner = OwnerWindow::dark();
    assert!(Painter::attach_dialog(owner.hwnd));
    let combo = child(&owner, C::ComboBox, 3, "", 100, 200);
    let lv = list_view(&owner, 5);
    select_row(lv, 2);
    let rect = row_rect(lv, 2);
    let other = row_rect(lv, 3);
    for focus_on_list in [false, true] {
        // The focus first on the combo box (the list view is unfocused: the system would paint a light grey), then on the list view. The windows are hidden,
        // so a real focus change may be refused; WM_SETFOCUS / WM_KILLFOCUS reach the controls either way.
        let target = if focus_on_list { lv } else { combo };
        let _ = pr_platform_win::win32::dialog_native::user32::set_focus(target);
        // SAFETY: integer parameters only.
        unsafe {
            if focus_on_list {
                send_message(lv, 0x0007, 0, 0);
            } else {
                send_message(lv, 0x0008, 0, 0);
            }
        }
        let shot = Shot::capture(lv);
        let client = client_on_screen(lv);
        let at =
            |dx: i32, rect: &Rect| shot.at(client.left + rect.left + dx, client.top + (rect.top + rect.bottom) / 2);
        assert_eq!(
            at(200, &rect),
            rgb(keys::SELECTION),
            "selected row (focus on list: {focus_on_list})"
        );
        assert_ne!(
            at(200, &other),
            rgb(keys::SELECTION),
            "unselected row (focus on list: {focus_on_list})"
        );
        let (_, _, contrast, _) = analyse(&shot, lv);
        assert!(
            contrast >= 4.5,
            "ink contrast {contrast:.2} (focus on list: {focus_on_list})"
        );
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn selected_list_view_row_uses_the_selection_colour_focused_or_not_v5_classic() {
    // C#: NativeDialogChromeTests.SelectedListViewRow_UsesTheSelectionColour_FocusedOrNot (without comctl32 v6)
    run(false, selected_row_uses_the_selection_colour);
}

#[test]
#[ignore = "native: needs desktop"]
fn selected_list_view_row_uses_the_selection_colour_focused_or_not_v6_manifest() {
    // C#: NativeDialogChromeTests.SelectedListViewRow_UsesTheSelectionColour_FocusedOrNot (with comctl32 v6)
    run(true, selected_row_uses_the_selection_colour);
}

#[test]
#[ignore = "native: needs desktop"]
fn dialog_subclass_corrects_a_selected_list_view_row() {
    // The test of 18l that needs `controls::adjust_list_view_custom_draw`: a real list view with one selected row, an NM_CUSTOMDRAW of stage 0x10001 sent to
    // the subclassed parent.
    run(false, || {
        let owner = OwnerWindow::hidden();
        assert!(Painter::attach_dialog(owner.hwnd));
        let lv = list_view(&owner, 3);
        select_row(lv, 1);
        let mut nm = NMLVCUSTOMDRAW::default();
        nm.nmcd.hdr.hwndFrom = HWND(lv as *mut core::ffi::c_void);
        nm.nmcd.hdr.code = (-12_i32) as u32; // NM_CUSTOMDRAW
        nm.nmcd.dwDrawStage = NMCUSTOMDRAW_DRAW_STAGE(0x10001);
        nm.nmcd.uItemState = NMCUSTOMDRAW_DRAW_STATE_FLAGS(0x201); // CDIS_SELECTED | something else
        nm.nmcd.dwItemSpec = 1;
        // SAFETY: `nm` is a live local NMLVCUSTOMDRAW for the whole synchronous call (the owner is a plain STATIC, so only the dialog subclass reads it).
        let answer = unsafe { send_message(owner.hwnd, WM_NOTIFY, 1, std::ptr::from_mut(&mut nm) as isize) };
        assert_eq!(answer & 2, 2, "CDRF_NEWFONT");
        assert_eq!(nm.clrTextBk, COLORREF(color(keys::SELECTION)));
        assert_eq!(nm.clrText, COLORREF(color(keys::TEXT_STRONG)));
        assert_eq!(nm.nmcd.uItemState.0 & 1, 0, "CDIS_SELECTED is cleared");
        assert_eq!(nm.nmcd.uItemState.0, 0x200);

        // An unselected row is left alone.
        let mut other = NMLVCUSTOMDRAW::default();
        other.nmcd.hdr.hwndFrom = HWND(lv as *mut core::ffi::c_void);
        other.nmcd.hdr.code = (-12_i32) as u32;
        other.nmcd.dwDrawStage = NMCUSTOMDRAW_DRAW_STAGE(0x10001);
        other.nmcd.uItemState = NMCUSTOMDRAW_DRAW_STATE_FLAGS(0x201);
        other.nmcd.dwItemSpec = 0;
        // SAFETY: as above.
        let answer = unsafe { send_message(owner.hwnd, WM_NOTIFY, 1, std::ptr::from_mut(&mut other) as isize) };
        assert_eq!(answer & 2, 0);
        assert_eq!(
            (other.nmcd.uItemState.0, other.clrText.0, other.clrTextBk.0),
            (0x201, 0, 0)
        );
    });
}
