//! Mutation killers for `pixels.rs::copy_transposed` (lane cl-pixels, W4-16a).
//!
//! Targets the six identities of CX-IMAGING2 that were never run (`470:33` `*` to `+` / `/`, `471:35` and `471:75`
//! `+` to `-` / `*`) and the four hang-only detections (`461:18` and `464:22` `<` to `<=`, `462:29` and `465:33` `+` to
//! `*`). Images are small and asymmetric (every pixel unique), the expected bytes come from the closed formulas in the
//! `copy_transposed` doc comment, not from the implementation. The loop mutants never advance a tile, so each
//! conversion runs on a worker thread and the test fails by assertion after a bounded wait instead of hanging.

use std::sync::mpsc;
use std::time::Duration;

use pr_imaging::pixels::{PixelBuffer, PixelLayout, apply_orientation};

/// Bounded wait for one conversion; a healthy conversion of these images takes microseconds.
const LIMIT: Duration = Duration::from_secs(20);

/// Source pixel `(x, y)` bytes: unique for every position of an image up to 255 x 255.
fn px(x: i32, y: i32) -> [u8; 4] {
    [x as u8, y as u8, (x * 7 + y * 3) as u8, 255]
}

/// Builds a `w x h` source, applies `orientation` and returns the destination width, height and rows (bytes).
fn convert(w: i32, h: i32, orientation: i32) -> (i32, i32, Vec<u8>) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut src = PixelBuffer::allocate(w, h, PixelLayout::Pbgra32).expect("allocate");
        for y in 0..h {
            let row = src.get_row_mut(y).expect("row");
            for x in 0..w {
                row[x as usize * 4..x as usize * 4 + 4].copy_from_slice(&px(x, y));
            }
        }
        let dst = apply_orientation(src, orientation).expect("apply");
        let mut bytes = Vec::new();
        for y in 0..dst.height() {
            bytes.extend_from_slice(dst.get_row(y).expect("row"));
        }
        let _ = tx.send((dst.width(), dst.height(), bytes));
    });
    rx.recv_timeout(LIMIT)
        .expect("conversion did not finish: tile loop does not advance (hang-guard mutant)")
}

/// Source pixel `(col, row)` that destination `(x, y)` must hold, per the documented formulas (source `W x H`).
fn expected_source(orientation: i32, w: i32, h: i32, x: i32, y: i32) -> (i32, i32) {
    match orientation {
        5 => (y, x),
        6 => (y, h - 1 - x),
        7 => (w - 1 - y, h - 1 - x),
        8 => (w - 1 - y, x),
        _ => unreachable!(),
    }
}

fn check(w: i32, h: i32, orientation: i32) {
    let (dw, dh, bytes) = convert(w, h, orientation);
    assert_eq!((dw, dh), (h, w), "orientation {orientation} swaps the size");
    assert_eq!(bytes.len(), (w * h * 4) as usize);
    for y in 0..dh {
        for x in 0..dw {
            let (sx, sy) = expected_source(orientation, w, h, x, y);
            let at = (y * dw + x) as usize * 4;
            assert_eq!(
                bytes[at..at + 4],
                px(sx, sy),
                "orientation {orientation} {w}x{h}: destination ({x},{y}) must be source ({sx},{sy})"
            );
        }
    }
}

// pixels.rs:470:33 (`x as i64 * step_x`: `*` to `+` / `/`), 471:35 and 471:75 (`+` to `-` / `*` in the index sum),
// 468/469 index arithmetic: any wrong source index changes a unique pixel.
#[test]
fn transpose_3x2_every_pixel_for_each_orientation() {
    for orientation in 5..=8 {
        check(3, 2, orientation);
    }
}

#[test]
fn transpose_2x3_every_pixel_for_each_orientation() {
    for orientation in 5..=8 {
        check(2, 3, orientation);
    }
}

// Multi-tile image (tile edge is 32): partial edge tiles in both directions, a second tile start and tile ends.
#[test]
fn transpose_70x37_spans_several_tiles_for_each_orientation() {
    for orientation in 5..=8 {
        check(70, 37, orientation);
    }
}

// Exact tile multiples: 461:18 / 464:22 (`<` to `<=`) enter an extra empty tile and never advance.
#[test]
fn transpose_32x64_exact_tile_multiples_terminate() {
    for orientation in 5..=8 {
        check(32, 64, orientation);
    }
}

// 1-pixel-wide and 1-pixel-high sources: smallest loops, first tile end must be exactly the size.
#[test]
fn transpose_degenerate_strips() {
    for orientation in 5..=8 {
        check(1, 5, orientation);
        check(5, 1, orientation);
        check(1, 1, orientation);
    }
}
