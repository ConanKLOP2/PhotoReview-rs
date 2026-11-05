//! MUT3-HOST (2026-11-06): pixel killers for the pen based strokes of `painter/widgets.rs::with_pen` (the check mark and the combo chevron).
//! The existing native tests only look for colours that the label text shares (`TEXT_STRONG`), so a `with_pen -> ()` mutant (no stroke at all) survived.
//! Here the probe pixel is a vertex of the stroke, taken from the pure layout (`check_layout` / `combo_layout`) at the window's real paint scale, and the
//! colour under it is compared with the layout's pen key AND with the colour the pixel has without the stroke.
#![cfg(windows)]

mod common {
    pub mod dialogs_painter_fixture;
}

use common::dialogs_painter_fixture::{OwnerWindow, Shot, child, client_on_screen, on_sta, rgb};
use pr_platform_win::dialogs::native::painter::NativeDialogChromePainter as Painter;
use pr_platform_win::dialogs::native::painter::geometry::{ChromePaintKind as K, keys, paint_scale};
use pr_platform_win::dialogs::native::painter::widgets::{CheckInput, ComboInput, check_layout, combo_layout};
use pr_platform_win::dialogs::native::template::DialogControlClass as C;
use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::dialog_native::user32::send_message;
use pr_platform_win::win32::structs::Hwnd;

const BS_AUTOCHECKBOX: u32 = 3;
const BM_SETCHECK: u32 = 0x00F1;
const CBS_DROPDOWNLIST: u32 = 3;
const CB_ADDSTRING: u32 = 0x0143;
const CB_SETCURSEL: u32 = 0x014E;

fn pixel(shot: &Shot, hwnd: Hwnd, dx: i32, dy: i32) -> u32 {
    let client = client_on_screen(hwnd);
    shot.at(client.left + dx, client.top + dy)
}

#[test]
#[ignore = "native: needs desktop"]
fn the_check_mark_stroke_is_painted_in_the_pen_colour() {
    on_sta(|| {
        let owner = OwnerWindow::dark();
        let check = child(&owner, C::Button, BS_AUTOCHECKBOX, "", 150, 24);
        assert!(Painter::attach(check, K::CheckBox));
        // SAFETY: integer parameters only.
        unsafe { send_message(check, BM_SETCHECK, 1, 0) };
        let client = client_on_screen(check);
        let (width, height) = (client.right - client.left, client.bottom - client.top);
        let scale = paint_scale(user32::get_dpi_for_window(check));
        let layout = check_layout(CheckInput {
            width,
            height,
            scale,
            radio: false,
            enabled: true,
            hot: false,
            checked: true,
            focused: false,
            multiline: false,
            left_text: false,
            text_height: 15,
            text_width: 0,
            hide_focus: true,
            hide_accel: true,
        });
        let (key, _pen_width, points) = layout.mark.expect("a checked box has a mark");
        assert_ne!(
            key, layout.fill_key,
            "the probe needs the pen colour to differ from the box fill"
        );
        let shot = Shot::capture(owner.hwnd);
        // The middle vertex is on the stroke whatever the pen width is.
        assert_eq!(
            pixel(&shot, check, points[1].x, points[1].y),
            rgb(key),
            "the mark vertex is stroked"
        );
        // A box pixel away from the stroke keeps the fill (the stroke does not flood the box).
        assert_eq!(
            pixel(&shot, check, layout.box_left + 1, layout.box_top + 1),
            rgb(layout.fill_key)
        );
    });
}

#[test]
#[ignore = "native: needs desktop"]
fn the_combo_chevron_stroke_is_painted_in_the_arrow_colour() {
    on_sta(|| {
        let owner = OwnerWindow::dark();
        let combo = child(&owner, C::ComboBox, CBS_DROPDOWNLIST, "", 160, 120);
        let text: Vec<u16> = "Item\0".encode_utf16().collect();
        // SAFETY: `text` is a NUL-terminated UTF-16 local that lives past the synchronous call; the control copies it.
        unsafe {
            send_message(combo, CB_ADDSTRING, 0, text.as_ptr() as isize);
            send_message(combo, CB_SETCURSEL, 0, 0);
        }
        assert!(Painter::attach(combo, K::ComboBox));
        let client = client_on_screen(combo);
        let (width, height) = (client.right - client.left, client.bottom - client.top);
        let scale = paint_scale(user32::get_dpi_for_window(combo));
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
            scale,
        );
        let shot = Shot::capture(owner.hwnd);
        assert_ne!(
            rgb(layout.arrow_key),
            rgb(keys::INPUT),
            "the probe needs the chevron colour to differ from the input fill"
        );
        let tip = layout.arrow_points[1];
        assert_eq!(
            pixel(&shot, combo, tip.x, tip.y),
            rgb(layout.arrow_key),
            "the chevron tip is stroked"
        );
        // Beside the arrow button there is only the input fill.
        assert_eq!(
            pixel(&shot, combo, layout.arrow_points[0].x, tip.y + 6),
            rgb(keys::INPUT)
        );
    });
}
