//! MUT3 (2026-11-06) rule B proof for the one equivalent mutant of `pixels.rs` (W4-16a): `copy_transposed` line 468 col 75,
//! `y as i64 * step_y` -> `y as i64 / step_y`.
//!
//! The mutant and the original can differ only if `y * step_y != y / step_y`. In `copy_transposed` `step_y` is one of the two constants of the
//! `match orientation` table (`1` for orientations 5 and 6, `-1` for 7 and 8) and `y` is the destination row `0..dst_height` with
//! `dst_height = src.width` (a `u32` value, so `y <= u32::MAX`). This test runs the identity over that WHOLE domain: every `y` in `0..=u32::MAX`
//! for both steps. It is `#[ignore]`d only because it takes a few seconds to minutes in a debug build; run it with `--include-ignored`.
//!
//! The second test runs the real function for every orientation 5..=8 and every source size `1..=70` x `1..=70` (all tile-edge classes of the 32 pixel
//! tile: 31, 32, 33, 63, 64, 65, 66..70) against the closed formulas of the doc comment, so the table the first test relies on is the table in use.

use std::hint::black_box;

use pr_imaging::pixels::{PixelBuffer, PixelLayout, apply_orientation};

#[test]
#[ignore = "slow: exhaustive over the whole u32 row domain (2 x 2^32 iterations)"]
fn row_times_step_equals_row_divided_by_step_for_every_u32_row() {
    for step in [1_i64, -1_i64] {
        let step = black_box(step);
        for y in 0..=u32::MAX {
            let y = black_box(i64::from(y));
            assert_eq!(y * step, y / step, "y = {y}, step = {step}");
        }
    }
}

fn px(x: i32, y: i32) -> [u8; 4] {
    [x as u8, y as u8, (x * 7 + y * 3) as u8, 255]
}

#[test]
fn transposed_orientations_match_the_closed_formulas_for_every_size_up_to_70() {
    for orientation in 5..=8 {
        for w in 1..=70_i32 {
            for h in 1..=70_i32 {
                let mut src = PixelBuffer::allocate(w, h, PixelLayout::Pbgra32).expect("allocate");
                for y in 0..h {
                    let row = src.get_row_mut(y).expect("row");
                    for x in 0..w {
                        row[x as usize * 4..x as usize * 4 + 4].copy_from_slice(&px(x, y));
                    }
                }
                let out = apply_orientation(src, orientation).expect("apply");
                assert_eq!(
                    (out.width(), out.height()),
                    (h, w),
                    "size, orientation {orientation} {w}x{h}"
                );
                for y in 0..out.height() {
                    let row = out.get_row(y).expect("dst row");
                    for x in 0..out.width() {
                        // Destination (x, y) of the doc comment of `copy_transposed` (source W x H).
                        let (sx, sy) = match orientation {
                            5 => (y, x),
                            6 => (y, h - 1 - x),
                            7 => (w - 1 - y, h - 1 - x),
                            _ => (w - 1 - y, x),
                        };
                        let at = x as usize * 4;
                        assert_eq!(
                            row[at..at + 4],
                            px(sx, sy),
                            "orientation {orientation} {w}x{h} at ({x},{y})"
                        );
                    }
                }
            }
        }
    }
}
