//! M5 killers (mutation close-out 2026-11-04) for `area_resampler.rs`. Hand-written, no golden file.
//!
//! The file is named `a_m5_...` so that cargo runs it BEFORE the big `area_resampler` binary: a mutant of `build_area_weights` that
//! makes the table explode (`87:21`, `hi = lo * sw`) otherwise only slows that binary down until the harness times out, and a timeout
//! is not a kill. The table tests run on a worker thread awaited with `recv_timeout`, so a stuck or exploding loop fails the test.
//!
//! `impl Display for ResizeError` (42:9 `-> Ok(Default::default())`): the messages are the text of the .NET exceptions
//! (`ArgumentException` / `ArgumentOutOfRangeException` from `PixelAreaResampler.Resize`), so they are compared whole.

use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_turbojpeg::area_resampler::{AreaWeights, ResizeError, build_area_weights, resize};
use std::sync::mpsc;
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(20);

fn weights_bounded(sw: usize, tw: usize) -> AreaWeights {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("area-weights".to_owned())
        .spawn(move || {
            let _ = tx.send(build_area_weights(sw, tw));
        })
        .expect("spawn worker");
    rx.recv_timeout(GUARD)
        .unwrap_or_else(|e| panic!("build_area_weights({sw}, {tw}) did not finish within {GUARD:?} ({e})"))
}

// Expected tables worked out by hand from the definition: output pixel x covers [x*sw, (x+1)*sw) in 1/tw units, source pixel s
// covers [s*tw, (s+1)*tw), the weight is the overlap.
#[test]
fn weights_for_5_to_2_are_the_hand_computed_overlaps() {
    let t = weights_bounded(5, 2);
    assert_eq!(t.first, vec![0, 2]);
    assert_eq!(t.offset, vec![0, 3, 6]);
    assert_eq!(t.weights, vec![2, 2, 1, 1, 2, 2]);
}

#[test]
fn weights_for_7_to_3_are_the_hand_computed_overlaps() {
    // x0: [0,7): s0 [0,3) 3, s1 [3,6) 3, s2 [6,9) 1. x1: [7,14): s2 2, s3 [9,12) 3, s4 [12,15) 2. x2: [14,21): s4 1, s5 3, s6 3.
    let t = weights_bounded(7, 3);
    assert_eq!(t.first, vec![0, 2, 4]);
    assert_eq!(t.offset, vec![0, 3, 6, 9]);
    assert_eq!(t.weights, vec![3, 3, 1, 2, 3, 2, 1, 3, 3]);
}

#[test]
fn weights_for_equal_widths_are_one_source_pixel_each() {
    let t = weights_bounded(4, 4);
    assert_eq!(t.first, vec![0, 1, 2, 3]);
    assert_eq!(t.offset, vec![0, 1, 2, 3, 4]);
    assert_eq!(t.weights, vec![4, 4, 4, 4]);
}

#[test]
fn weights_for_one_target_column_cover_the_whole_row() {
    let t = weights_bounded(3, 1);
    assert_eq!(t.first, vec![0]);
    assert_eq!(t.offset, vec![0, 3]);
    assert_eq!(t.weights, vec![1, 1, 1]);
}

#[test]
fn display_of_a_non_bgr32_source_names_the_layout() {
    let pbgra = PixelBuffer::allocate(2, 2, PixelLayout::Pbgra32).expect("valid geometry");
    let error = resize(&pbgra, 1, 1).expect_err("refused");
    assert_eq!(error.to_string(), "Only Bgr32 buffers are resampled (got Pbgra32).");
}

#[test]
fn display_of_a_bad_target_size_names_the_argument_and_the_value() {
    let bgr = PixelBuffer::allocate(2, 2, PixelLayout::Bgr32).expect("valid geometry");
    assert_eq!(
        resize(&bgr, 0, 1).expect_err("refused").to_string(),
        "targetWidth ('0') must be greater than or equal to '1'."
    );
    assert_eq!(
        resize(&bgr, 1, -3).expect_err("refused").to_string(),
        "targetHeight ('-3') must be greater than or equal to '1'."
    );
}

#[test]
fn display_of_a_buffer_error_is_the_buffer_errors_own_text() {
    let inner = PixelBuffer::allocate(0, 1, PixelLayout::Bgr32).expect_err("zero width is refused");
    let expected = inner.to_string();
    assert!(!expected.is_empty());
    assert_eq!(ResizeError::Buffer(inner).to_string(), expected);
}

// ---------------------------------------------------------------------------------------------------------------
// `resize` against an independent, naive reference (exact rational area averaging with `i128`, O(source x target)): every output
// channel is `(sum(value * overlap_x * overlap_y) + total / 2) / total`, `total = sw * sh`, alpha 0xFF. The reference shares no
// code with the table / streaming accumulators of `resize_core`. The call runs on a worker thread awaited with `recv_timeout`
// (the mutants of `resize_core`'s row loops otherwise spin until the harness times out).
// ---------------------------------------------------------------------------------------------------------------

fn source_pixels(sw: usize, sh: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(12_345);
    (0..sw * sh * 4)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 8) as u8
        })
        .collect()
}

fn overlap(index: usize, from: usize, to: usize, other: usize) -> i128 {
    // Target cell `index` covers [index * from, (index + 1) * from) in 1/to units; source cell `s` covers [s * to, (s + 1) * to).
    let lo = (index * from) as i128;
    let hi = lo + from as i128;
    let s_lo = (other * to) as i128;
    let s_hi = s_lo + to as i128;
    (hi.min(s_hi) - lo.max(s_lo)).max(0)
}

fn reference(src: &[u8], sw: usize, sh: usize, tw: usize, th: usize) -> Vec<u8> {
    let total = (sw * sh) as i128;
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        for tx in 0..tw {
            for channel in 0..3 {
                let mut sum = 0i128;
                for sy in 0..sh {
                    let oy = overlap(ty, sh, th, sy);
                    if oy == 0 {
                        continue;
                    }
                    for sx in 0..sw {
                        let ox = overlap(tx, sw, tw, sx);
                        sum += i128::from(src[(sy * sw + sx) * 4 + channel]) * ox * oy;
                    }
                }
                out[(ty * tw + tx) * 4 + channel] = ((sum + total / 2) / total) as u8;
            }
            out[(ty * tw + tx) * 4 + 3] = 0xFF;
        }
    }
    out
}

fn resize_bounded(pixels: Vec<u8>, sw: usize, sh: usize, tw: i32, th: i32) -> (u32, u32, Vec<u8>) {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("area-resize".to_owned())
        .spawn(move || {
            let mut src = PixelBuffer::allocate(sw as i32, sh as i32, PixelLayout::Bgr32).expect("valid geometry");
            for y in 0..sh {
                src.get_row_mut(y as i32)
                    .expect("row")
                    .copy_from_slice(&pixels[y * sw * 4..(y + 1) * sw * 4]);
            }
            let dst = resize(&src, tw, th).expect("resize");
            let mut bytes = Vec::new();
            for y in 0..dst.height() {
                bytes.extend_from_slice(dst.get_row(y).expect("row"));
            }
            let _ = tx.send((dst.width() as u32, dst.height() as u32, bytes));
        })
        .expect("spawn worker");
    rx.recv_timeout(GUARD)
        .unwrap_or_else(|e| panic!("resize {sw}x{sh} -> {tw}x{th} did not finish within {GUARD:?} ({e})"))
}

#[test]
fn resize_matches_the_naive_area_average_for_many_shapes() {
    let shapes: &[(usize, usize, usize, usize)] = &[
        (5, 7, 2, 3),
        (7, 5, 3, 2),
        (6, 6, 4, 4),
        (9, 1, 4, 1),
        (1, 9, 1, 4),
        (10, 10, 1, 1),
        (3, 3, 2, 2),
        (8, 8, 8, 3),
        (8, 8, 3, 8),
        (13, 11, 5, 7),
        (11, 13, 7, 5),
        (4, 9, 3, 2),
        (2, 2, 1, 2),
        (2, 2, 2, 1),
    ];
    for (n, &(sw, sh, tw, th)) in shapes.iter().enumerate() {
        let pixels = source_pixels(sw, sh, n as u32 + 1);
        let expected = reference(&pixels, sw, sh, tw, th);
        let (w, h, got) = resize_bounded(pixels, sw, sh, tw as i32, th as i32);
        assert_eq!((w as usize, h as usize), (tw, th), "size of {sw}x{sh} -> {tw}x{th}");
        assert_eq!(got, expected, "pixels of {sw}x{sh} -> {tw}x{th}");
    }
}

#[test]
fn resize_never_upscales_an_axis_and_still_averages_the_other() {
    // target 10x2 of a 4x6 source: the width stays 4, the height becomes 2.
    let pixels = source_pixels(4, 6, 99);
    let expected = reference(&pixels, 4, 6, 4, 2);
    let (w, h, got) = resize_bounded(pixels, 4, 6, 10, 2);
    assert_eq!((w, h), (4, 2));
    assert_eq!(got, expected);
}
