//! P-394: the drop effect of `Reselect` (`widgets.rs` 48:9 `drop -> ()` and 48:21 `!= -> ==`).
//!
//! `with_pen` / `with_pen_and_brush` must select the caller's previous pen and brush back into the DC when the draw closure ends. The tests use a
//! caller-owned memory DC (no window, no desktop): `SelectObject` returns the object that was current, so selecting a stock object reads the state.

use super::{with_pen, with_pen_and_brush};
use crate::win32::api::gdi32::get_stock_object;
use crate::win32::comctl::gdi32::create_pen;
use crate::win32::dialog_chrome::{create_compatible_dc, delete_dc};
use crate::win32::dialog_native::gdi32::{create_solid_brush, delete_object, select_object};
use crate::win32::structs::{Hdc, Hgdi};

const WHITE_BRUSH: i32 = 0;
const BLACK_PEN: i32 = 7;

/// The pen current in `hdc` (selects the stock black pen; the DC is not used afterwards except to read the other kind).
fn current_pen(hdc: Hdc) -> Hgdi {
    select_object(hdc, get_stock_object(BLACK_PEN))
}

fn current_brush(hdc: Hdc) -> Hgdi {
    select_object(hdc, get_stock_object(WHITE_BRUSH))
}

#[test]
fn with_pen_selects_the_callers_pen_back_after_the_draw() {
    let hdc = create_compatible_dc(0);
    assert_ne!(hdc, 0, "a memory DC");
    let mine = create_pen(0, 1, 0x00FF_00FF);
    assert_ne!(mine, 0);
    select_object(hdc, mine);

    let mut inside = 0;
    with_pen(hdc, 3, 0x0000_FF00, || inside = current_pen_peek(hdc));
    assert_ne!(inside, mine, "inside the draw the new pen is selected");
    assert_ne!(inside, 0);

    assert_eq!(
        current_pen(hdc),
        mine,
        "after the draw the caller's pen is current again"
    );

    delete_object(mine);
    delete_dc(hdc);
}

/// Reads the current pen without keeping the stock pen selected: select the stock pen, then the pen it returned.
fn current_pen_peek(hdc: Hdc) -> Hgdi {
    let cur = current_pen(hdc);
    select_object(hdc, cur);
    cur
}

#[test]
fn with_pen_and_brush_selects_both_of_the_callers_objects_back_after_the_draw() {
    let hdc = create_compatible_dc(0);
    assert_ne!(hdc, 0, "a memory DC");
    let my_pen = create_pen(0, 1, 0x00FF_00FF);
    let my_brush = create_solid_brush(0x00FF_FF00);
    assert!(my_pen != 0 && my_brush != 0);
    select_object(hdc, my_pen);
    select_object(hdc, my_brush);

    with_pen_and_brush(hdc, 0x0000_FF00, 0x0000_00FF, || {
        assert_ne!(current_pen_peek(hdc), my_pen);
        let brush = current_brush(hdc);
        assert_ne!(brush, my_brush);
        select_object(hdc, brush);
    });

    assert_eq!(current_pen(hdc), my_pen, "the caller's pen is current again");
    assert_eq!(current_brush(hdc), my_brush, "the caller's brush is current again");

    delete_object(my_pen);
    delete_object(my_brush);
    delete_dc(hdc);
}
