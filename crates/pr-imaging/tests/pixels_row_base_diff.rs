//! P-pixels-468: `copy_transposed` computes the per-row source base `origin -/+ y` once per row instead of
//! `y * step_y` per pixel. Differential test against the OLD formula `origin + x * step_x + y * step_y`
//! (copied below as a reference) through the public `apply_orientation`.
//!
//! Domain: `copy_transposed` is private and called only from `apply_orientation` for orientations 5..=8, so
//! `(origin, step_x, step_y)` takes exactly the four table rows below. Sizes cover every 32 pixel tile edge class.

use pr_imaging::pixels::{PixelBuffer, PixelLayout, apply_orientation};

fn old_formula(orientation: i32, w: i64, h: i64, x: i64, y: i64) -> i64 {
    let (origin, step_x, step_y): (i64, i64, i64) = match orientation {
        5 => (0, w, 1),
        6 => ((h - 1) * w, -w, 1),
        7 => ((h - 1) * w + w - 1, -w, -1),
        _ => (w - 1, w, -1),
    };
    origin + x * step_x + y * step_y
}

fn check(orientation: i32, w: i32, h: i32) {
    let mut src = PixelBuffer::allocate(w, h, PixelLayout::Pbgra32).expect("allocate");
    for y in 0..h {
        let row = src.get_row_mut(y).expect("row");
        for x in 0..w {
            let id = (y * w + x) as u32;
            row[x as usize * 4..x as usize * 4 + 4].copy_from_slice(&id.to_le_bytes());
        }
    }
    let out = apply_orientation(src, orientation).expect("apply");
    assert_eq!((out.width(), out.height()), (h, w));
    for y in 0..out.height() {
        let row = out.get_row(y).expect("dst row");
        for x in 0..out.width() {
            let want = old_formula(orientation, i64::from(w), i64::from(h), i64::from(x), i64::from(y));
            let at = x as usize * 4;
            let got = u32::from_le_bytes(row[at..at + 4].try_into().unwrap());
            assert_eq!(i64::from(got), want, "orientation {orientation} {w}x{h} at ({x},{y})");
        }
    }
}

#[test]
fn new_row_base_equals_old_formula_for_every_orientation_and_size() {
    for orientation in 5..=8 {
        for w in 1..=70 {
            for h in 1..=70 {
                check(orientation, w, h);
            }
        }
        for (w, h) in [(130, 65), (65, 130), (97, 1), (1, 97), (257, 33), (33, 257), (200, 200)] {
            check(orientation, w, h);
        }
    }
}
