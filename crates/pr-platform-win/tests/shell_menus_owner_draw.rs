//! Tests of W8-14g `shell_menus::owner_draw::OwnerDrawMenuSession` (C# `OwnerDrawMenuSession`, baseline `d92d6aae`; no C# unit test exists, the behaviour
//! is proved by `ShellMenuRenderingTests`). The painting goes into a memory bitmap through `measure_item`/`draw_item` with the real GDI; the subclass is
//! tested on a hidden `STATIC` owner that is never shown. All native: `#[ignore = "native: needs desktop"]`. The popup capture ports of
//! `ShellMenuRenderingTests` need `NativePopupMenuHost::show` (W8-14f) and are not part of this unit.

#[path = "common/owner_draw_fixture.rs"]
mod fixture;

use fixture::{Canvas, OwnerWindow, brightness};
use pr_platform_win::shell_menus::owner_draw::{
    ODT_MENU, OwnerDrawItem, OwnerDrawMenuSession, SUBCLASS_ID, WM_DRAWITEM, WM_MEASUREITEM,
};
use pr_platform_win::shell_menus::owner_draw_style::{
    ODS_CHECKED, ODS_DISABLED, ODS_SELECTED, background, color_ref, item_background, item_height, item_text, scale,
    separator, separator_height,
};
use pr_platform_win::win32::dialog_native::{kernel32, user32 as dialog_user32};
use pr_platform_win::win32::structs::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, DRAWITEMSTRUCT_CTL_TYPE, MEASUREITEMSTRUCT, ODS_SELECTED as NATIVE_ODS_SELECTED,
};
use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

const W: i32 = 240;
const H: i32 = 40;

fn item(text: &str) -> OwnerDrawItem {
    OwnerDrawItem {
        text: text.to_owned(),
        shortcut: None,
        is_separator: false,
        is_submenu: false,
    }
}

fn rect() -> Rect {
    Rect {
        left: 0,
        top: 0,
        right: W,
        bottom: H,
    }
}

fn gdi_count() -> u32 {
    dialog_user32::get_gui_resources(kernel32::get_current_process(), 0)
}

fn paint(session: &OwnerDrawMenuSession, state: u32, it: &OwnerDrawItem) -> Canvas {
    let canvas = Canvas::new(W, H);
    session.draw_item(canvas.dc, rect(), state, it);
    canvas
}

fn send(hwnd: isize, message: u32, lparam: isize) -> isize {
    // SAFETY: `hwnd` is the live owner of the calling thread; the callers pass a null or misaligned pointer (never dereferenced by the owner's own
    // procedure for these messages) or a pointer to a live local struct.
    unsafe {
        SendMessageW(
            HWND(hwnd as *mut core::ffi::c_void),
            message,
            Some(WPARAM(0)),
            Some(LPARAM(lparam)),
        )
    }
    .0
}

fn measure_struct(ctl_type: u32, item_data: usize) -> MEASUREITEMSTRUCT {
    MEASUREITEMSTRUCT {
        CtlType: DRAWITEMSTRUCT_CTL_TYPE(ctl_type),
        itemData: item_data,
        ..Default::default()
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn measure_item_matches_the_style_functions_for_text_shortcut_and_separator() {
    let _serial = fixture::serial();
    for dpi in [96u32, 192] {
        let owner = OwnerWindow::new();
        let s = OwnerDrawMenuSession::new(owner.hwnd, dpi, None);
        assert_eq!(
            s.measure_item(&OwnerDrawItem {
                is_separator: true,
                ..item("")
            }),
            (scale(48, dpi), separator_height(dpi))
        );
        let (w_plain, h_plain) = s.measure_item(&item("Open"));
        let (w_long, _) = s.measure_item(&item("Open the folder in a new window"));
        let with_shortcut = OwnerDrawItem {
            shortcut: Some("Ctrl+O".to_owned()),
            ..item("Open")
        };
        let (w_short, h_short) = s.measure_item(&with_shortcut);
        let empty_shortcut = OwnerDrawItem {
            shortcut: Some(String::new()),
            ..item("Open")
        };
        assert_eq!(s.measure_item(&empty_shortcut), (w_plain, h_plain));
        assert!(w_plain > 2 * scale(32, dpi), "text adds to the padding");
        assert!(w_long > w_plain);
        assert!(
            w_short > w_plain + scale(32, dpi),
            "a shortcut adds a gap plus its width"
        );
        assert_eq!(h_short, h_plain);
        assert!(
            h_plain >= item_height(1, dpi),
            "text height + padding, floored at scale(28)"
        );
        // empty text measures as zero extents.
        assert_eq!(s.measure_item(&item("")), (2 * scale(32, dpi), item_height(0, dpi)));
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_item_normal_paints_the_panel_background_and_light_text() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let c = paint(&s, 0, &item("Open"));
    assert_eq!(c.pixel(2, 2), background());
    assert!(
        c.pixels_in(0, W).iter().any(|&p| brightness(p) > 0x80),
        "light text pixels"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_item_selected_uses_button_pressed_background_and_keeps_the_text_readable() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let c = paint(&s, ODS_SELECTED, &item("Open"));
    assert_eq!(c.pixel(2, 2), item_background(ODS_SELECTED));
    assert_ne!(c.pixel(2, 2), background());
    assert!(c.pixels_in(0, W).iter().any(|&p| brightness(p) > 0xA0));
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_item_disabled_text_is_dimmer_but_visible() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let normal = paint(&s, 0, &item("Open"));
    let disabled = paint(&s, ODS_DISABLED, &item("Open"));
    let max = |c: &Canvas| c.pixels_in(0, W).into_iter().map(brightness).max().unwrap();
    assert!(max(&disabled) < max(&normal), "dimmer");
    assert!(max(&disabled) > brightness(background()) + 0x20, "still visible");
    assert!(max(&disabled) <= brightness(item_text(ODS_DISABLED)));
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_item_separator_draws_a_one_pixel_line_inset_by_scale_8() {
    let _serial = fixture::serial();
    for dpi in [96u32, 192] {
        let owner = OwnerWindow::new();
        let s = OwnerDrawMenuSession::new(owner.hwnd, dpi, None);
        let sep = OwnerDrawItem {
            is_separator: true,
            ..item("")
        };
        // a selected state must not matter: a separator always uses the plain background.
        let c = paint(&s, ODS_SELECTED, &sep);
        let mid = H / 2;
        let inset = scale(8, dpi);
        assert_eq!(c.pixel(2, 2), background());
        assert_eq!(c.pixel(inset, mid), separator());
        assert_eq!(c.pixel(W - inset - 1, mid), separator());
        assert_eq!(c.pixel(inset - 1, mid), background());
        assert_eq!(c.pixel(W - inset, mid), background());
        assert_eq!(c.pixel(W / 2, mid - 1), background());
        assert_eq!(c.pixel(W / 2, mid + 1), background());
    }
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_item_checked_draws_the_check_mark_in_the_gutter() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let gutter = scale(32, 96);
    let plain = paint(&s, 0, &item("Open"));
    let checked = paint(&s, ODS_CHECKED, &item("Open"));
    assert!(
        plain.pixels_in(0, gutter).iter().all(|&p| p == background()),
        "no mark without the state"
    );
    let mark = checked.pixels_in(0, gutter);
    assert!(
        mark.iter().any(|&p| p == item_text(ODS_CHECKED)),
        "the mark is drawn in the text colour"
    );
    // the first vertex of the check: (cx - 4, mid) with cx = 14.
    assert_eq!(checked.pixel(10, H / 2), item_text(0));
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_item_submenu_draws_the_arrow_at_the_right() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let plain = paint(&s, 0, &item("Open"));
    let sub = paint(
        &s,
        0,
        &OwnerDrawItem {
            is_submenu: true,
            ..item("Open")
        },
    );
    let arrow_from = W - scale(12, 96) - scale(4, 96) - 1;
    assert!(plain.pixels_in(arrow_from, W).iter().all(|&p| p == background()));
    assert!(sub.pixels_in(arrow_from, W).iter().any(|&p| p == item_text(0)));
    // the tip of the arrow: (rc.right - 12, mid).
    assert_eq!(sub.pixel(W - 12, H / 2), item_text(0));
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_item_with_a_shortcut_right_aligns_it_in_the_secondary_colour() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let it = OwnerDrawItem {
        shortcut: Some("Ctrl+O".to_owned()),
        ..item("Open")
    };
    let c = paint(&s, 0, &it);
    let right_end = W - scale(16, 96);
    let bg = background();
    let right_half = c.pixels_in(W / 2, W);
    let secondary = brightness(color_ref("Dark.TextSecondary"));
    assert!(right_half.iter().any(|&p| p != bg), "shortcut pixels on the right");
    assert!(
        right_half.iter().all(|&p| brightness(p) <= secondary),
        "never brighter than the secondary colour"
    );
    assert!(
        c.pixels_in(right_end, W).iter().all(|&p| p == bg),
        "nothing in the right padding"
    );
    let left = c.pixels_in(0, W / 4).into_iter().map(brightness).max().unwrap();
    assert!(left > secondary, "the text itself is in the primary colour");
    // with a submenu the shortcut moves left by scale(16): the band next to the padding stays empty.
    let sub = paint(&s, 0, &OwnerDrawItem { is_submenu: true, ..it });
    let band_from = right_end - scale(16, 96);
    let band_to = W - scale(12, 96) - scale(4, 96) - 1;
    assert!(sub.pixels_in(band_from, band_to).iter().all(|&p| p == bg));
}

#[test]
#[ignore = "native: needs desktop"]
fn draw_item_does_not_leak_gdi_objects_over_1000_calls() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let canvas = Canvas::new(W, H);
    let rich = OwnerDrawItem {
        shortcut: Some("Ctrl+O".to_owned()),
        is_submenu: true,
        ..item("Open")
    };
    let sep = OwnerDrawItem {
        is_separator: true,
        ..item("")
    };
    // warm up every state once: the OS creates a few process-wide GDI objects lazily (font cache, DC state), which are not ours.
    for state in [ODS_CHECKED | ODS_SELECTED, ODS_DISABLED, 0] {
        s.draw_item(canvas.dc, rect(), state, &rich);
        s.draw_item(canvas.dc, rect(), state, &sep);
    }
    s.measure_item(&rich);
    let run = |rounds: usize| {
        for i in 0..rounds {
            let state = if i % 2 == 0 {
                ODS_CHECKED | ODS_SELECTED
            } else {
                ODS_DISABLED
            };
            s.draw_item(canvas.dc, rect(), state, &rich);
            s.draw_item(canvas.dc, rect(), 0, &sep);
            s.measure_item(&rich);
        }
    };
    run(10);
    let before = gdi_count();
    run(1000);
    assert_eq!(gdi_count(), before);
}

#[test]
#[ignore = "native: needs desktop"]
fn create_and_dispose_cycles_do_not_leak_gdi_objects() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    drop(OwnerDrawMenuSession::new(owner.hwnd, 96, None)); // warm up
    let before = gdi_count();
    for dpi in [0u32, 96, 144] {
        for _ in 0..100 {
            let s = OwnerDrawMenuSession::new(owner.hwnd, dpi, None);
            assert_ne!(s.background_brush(), 0);
            assert!(s.attach());
            s.dispose();
            s.dispose(); // idempotent
            assert_eq!(s.background_brush(), 0);
        }
    }
    assert_eq!(gdi_count(), before);
}

#[test]
#[ignore = "native: needs desktop"]
fn attach_registers_the_subclass_and_dispose_removes_it_before_releasing() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let data = s.register(item("Open"));
    assert_eq!(data, 1);
    assert!(!s.is_attached());
    assert!(s.attach());
    assert!(s.is_attached());
    assert!(s.attach(), "a second attach is a no-op returning the current state");
    assert_ne!(SUBCLASS_ID, 0);

    let mut m = measure_struct(ODT_MENU, data as usize);
    assert_eq!(send(owner.hwnd, WM_MEASUREITEM, &mut m as *mut _ as isize), 1);
    let expected = s.measure_item(&item("Open"));
    assert_eq!((m.itemWidth as i32, m.itemHeight as i32), expected);

    // an unknown itemData and another CtlType are not handled: the default procedure answers 0 and the struct is untouched.
    let mut unknown = measure_struct(ODT_MENU, 99);
    assert_eq!(send(owner.hwnd, WM_MEASUREITEM, &mut unknown as *mut _ as isize), 0);
    assert_eq!(unknown.itemHeight, 0);
    let mut other = measure_struct(3, 1);
    assert_eq!(send(owner.hwnd, WM_MEASUREITEM, &mut other as *mut _ as isize), 0);
    assert_eq!(other.itemHeight, 0);

    s.dispose();
    assert!(!s.is_attached());
    let mut after = measure_struct(ODT_MENU, data as usize);
    assert_eq!(send(owner.hwnd, WM_MEASUREITEM, &mut after as *mut _ as isize), 0);
    assert_eq!(after.itemHeight, 0, "the subclass is gone");
}

#[test]
#[ignore = "native: needs desktop"]
fn dropping_the_session_removes_the_subclass() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    {
        let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
        let data = s.register(item("Open"));
        assert!(s.attach());
        let mut m = measure_struct(ODT_MENU, data as usize);
        assert_eq!(send(owner.hwnd, WM_MEASUREITEM, &mut m as *mut _ as isize), 1);
    }
    let mut after = measure_struct(ODT_MENU, 1);
    assert_eq!(send(owner.hwnd, WM_MEASUREITEM, &mut after as *mut _ as isize), 0);
}

#[test]
#[ignore = "native: needs desktop"]
fn wm_drawitem_through_the_subclass_paints_into_the_item_dc() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    let data = s.register(item("Open"));
    assert!(s.attach());
    let canvas = Canvas::new(W, H);
    let mut d = DRAWITEMSTRUCT {
        CtlType: DRAWITEMSTRUCT_CTL_TYPE(ODT_MENU),
        itemState: NATIVE_ODS_SELECTED,
        hDC: windows::Win32::Graphics::Gdi::HDC(canvas.dc as *mut core::ffi::c_void),
        rcItem: rect(),
        itemData: data as usize,
        ..Default::default()
    };
    assert_eq!(send(owner.hwnd, WM_DRAWITEM, &mut d as *mut _ as isize), 1);
    assert_eq!(canvas.pixel(2, 2), item_background(ODS_SELECTED));
    s.dispose();
}

#[test]
#[ignore = "native: needs desktop"]
fn lparam_null_or_misaligned_is_not_handled() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    s.register(item("Open"));
    assert!(s.attach());
    for message in [WM_MEASUREITEM, WM_DRAWITEM] {
        assert_eq!(send(owner.hwnd, message, 0), 0, "null");
        assert_eq!(send(owner.hwnd, message, 1), 0, "misaligned");
    }
    s.dispose();
}

#[test]
#[ignore = "native: needs desktop"]
fn an_unattached_session_leaves_the_window_alone() {
    let _serial = fixture::serial();
    let owner = OwnerWindow::new();
    let s = OwnerDrawMenuSession::new(owner.hwnd, 96, None);
    s.register(item("Open"));
    let mut m = measure_struct(ODT_MENU, 1);
    assert_eq!(send(owner.hwnd, WM_MEASUREITEM, &mut m as *mut _ as isize), 0);
    assert_eq!(m.itemHeight, 0);
}

#[test]
fn item_equality_and_clone_are_by_value() {
    let a = item("A");
    assert_eq!(a.clone(), a);
    assert_ne!(
        a,
        OwnerDrawItem {
            is_submenu: true,
            ..item("A")
        }
    );
}
