//! MUT7 (2026-11-06) rule B proofs for `wic/decoder.rs` mutants that no test can tell from the original, each run over the WHOLE set of inputs on which
//! mutant and original differ. No WIC, no window station.
//!
//! * `674:61` `buffer_length < 0` -> `<= 0` in `ensure_output_fits`. The branch is reached only after `(0..GUARD_THRESHOLD_BYTES).contains(&buffer_length)`
//!   returned `false`; `0` is in that range (the guard threshold is positive), so `buffer_length == 0`, the only value on which `< 0` and `<= 0` differ, never
//!   reaches the condition. Run over that one input (and a memory source that panics when read).
//! * `803:75` and `805:75` `FlipHorizontal.0 | Rotate270.0` / `FlipHorizontal.0 | Rotate90.0` -> `^`: `a | b == a ^ b` exactly when `a & b == 0`. Both
//!   operands are constants, so the whole domain is the two pairs checked here.
//! * `232:53` `orientation > 1` -> `>= 1`: the two differ only at `orientation == 1`, where `map_to_transform_options` is `Rotate0` (no rotator is created).
//!   The whole divergence domain is the single value 1. (The all-`i32` version is `mut3_wic_decoder_proofs`, run with `MUT3_EXHAUSTIVE=1`.)

use pr_imaging::memory_headroom::{GUARD_THRESHOLD_BYTES, MemoryInfo};
use pr_platform_win::wic::decoder::{ensure_output_fits, map_to_transform_options};
use windows::Win32::Graphics::Imaging::{
    WICBitmapTransformFlipHorizontal, WICBitmapTransformOptions, WICBitmapTransformRotate0, WICBitmapTransformRotate90,
    WICBitmapTransformRotate270,
};

#[test]
fn a_zero_byte_buffer_is_admitted_before_the_negative_check_and_without_reading_memory() {
    const { assert!(GUARD_THRESHOLD_BYTES > 0, "0 lies inside 0..GUARD_THRESHOLD_BYTES") };
    let must_not_be_read =
        || -> MemoryInfo { panic!("memory must not be read for a buffer below the guard threshold") };
    for (width, height) in [(0, 0), (0, 5), (5, 0), (0, i32::MAX), (i32::MAX, 0)] {
        ensure_output_fits(width, height, 0, &must_not_be_read).expect("a zero byte buffer is admitted");
    }
    // The negative side of the same condition still fails without reading memory.
    assert!(ensure_output_fits(1, 1, -1, &must_not_be_read).is_err());
}

#[test]
fn the_combined_flip_and_rotate_flags_have_disjoint_bits() {
    let flip = WICBitmapTransformFlipHorizontal.0;
    for (orientation, rotate) in [(5, WICBitmapTransformRotate270.0), (7, WICBitmapTransformRotate90.0)] {
        assert_eq!(flip & rotate, 0, "flip and rotate share no bit, so `|` equals `^`");
        assert_eq!(
            map_to_transform_options(orientation),
            WICBitmapTransformOptions(flip + rotate),
            "orientation {orientation}"
        );
        assert_eq!(flip | rotate, flip ^ rotate);
    }
}

#[test]
fn orientation_one_maps_to_rotate_zero_so_the_rotator_branch_is_a_no_op() {
    assert_eq!(map_to_transform_options(1), WICBitmapTransformRotate0);
}
