//! MUT3-HOST (2026-11-06) rule B proofs for equivalent-looking mutants of `wic/decoder.rs`, run over the WHOLE domain in which mutant and original could
//! differ.
//!
//! `decoder.rs:232:38`: `request.apply_orientation && orientation > 1` -> `||`. The two conditions can only differ for
//! (a) `apply_orientation == true` and `orientation <= 1`: the mutant then calls `map_to_transform_options(orientation)`; it still builds nothing because
//!     the result is `Rotate0` (the rotator is created only for `options != Rotate0`). Proven by running `map_to_transform_options` over EVERY `i32` that is
//!     not 2..=8 (all 2^32 - 7 values).
//! (b) `apply_orientation == false` and `orientation > 1`: impossible, because with `apply_orientation == false` the orientation is read through
//!     `read_frame_metadata(.., false, ..)`, which is `read_metadata_values(.., read_orientation = false, ..)`; that gives orientation 1 without a single
//!     query (checked here with a query that counts its calls and one that panics if it is called), so no stored tag value (a 16 bit short, or any other
//!     encoding) can reach the condition.
//! The `Some(given)` arm of `decode_from_stream` requires `apply_orientation == true`, so it cannot produce case (b) either.

use pr_platform_win::wic::decoder::map_to_transform_options;
use pr_platform_win::wic::exif_reader::read_metadata_values;
use windows::Win32::Graphics::Imaging::WICBitmapTransformRotate0;

#[test]
fn every_i32_outside_two_to_eight_maps_to_rotate_zero() {
    // Takes about two minutes in a debug build: runs only when MUT3_EXHAUSTIVE is set (the evidence run sets it).
    if std::env::var_os("MUT3_EXHAUSTIVE").is_none() {
        return;
    }
    let rotate0 = WICBitmapTransformRotate0;
    let mut checked = 0_u64;
    let mut orientation = i32::MIN;
    loop {
        if !(2..=8).contains(&orientation) {
            assert_eq!(
                map_to_transform_options(std::hint::black_box(orientation)),
                rotate0,
                "orientation {orientation}"
            );
            checked += 1;
        }
        if orientation == i32::MAX {
            break;
        }
        orientation += 1;
    }
    assert_eq!(checked, (1_u64 << 32) - 7);
}

#[test]
fn the_values_two_to_eight_are_the_only_rotations() {
    let rotate0 = WICBitmapTransformRotate0;
    for orientation in 2..=8 {
        assert_ne!(
            map_to_transform_options(orientation),
            rotate0,
            "orientation {orientation}"
        );
    }
}

#[test]
fn without_apply_orientation_the_orientation_is_one_and_no_tag_is_queried() {
    let mut calls = 0_u32;
    let read = read_metadata_values(
        |_name| {
            calls += 1;
            // A query that would answer if it were asked.
            Ok(None)
        },
        false,
        None,
    );
    assert_eq!(read.orientation, 1);
    assert_eq!(calls, 0, "no query for the orientation half");
    let read = read_metadata_values(|_| panic!("nothing may be queried"), false, None);
    assert_eq!(read.orientation, 1);
    assert!(read.exif.is_none());
}

// `decoder.rs:654:15`: `pixels > i64::MAX / 4` -> `>=` in `output_byte_length`. The two differ only when `width * height == i64::MAX / 4 == 2^61 - 1`.
// `2^61 - 1` is a Mersenne prime, so its only factorisations are `1 * (2^61 - 1)` and `(2^61 - 1) * 1`, and `2^61 - 1` does not fit an `i32`. This test runs
// the check over EVERY `i32` side: for each `1..=i32::MAX` no integer quotient fits an `i32`; a side of 0 gives product 0 and a negative side gives a
// negative product, so neither can equal the constant.
#[test]
fn no_pair_of_i32_sides_multiplies_to_i64_max_over_four() {
    if std::env::var_os("MUT3_EXHAUSTIVE").is_none() {
        return;
    }
    let boundary: i64 = i64::MAX / 4;
    assert_eq!(boundary, (1_i64 << 61) - 1);
    for width in 1..=i64::from(i32::MAX) {
        let width = std::hint::black_box(width);
        if boundary % width == 0 {
            let height = boundary / width;
            assert!(
                height > i64::from(i32::MAX),
                "width {width} would pair with height {height}"
            );
        }
    }
    // Negative and zero sides cannot reach a positive product; the function maps both of them to a non-saturating value.
    assert_eq!(pr_platform_win::wic::decoder::output_byte_length(0, i32::MAX), 0);
    assert_eq!(
        pr_platform_win::wic::decoder::output_byte_length(-1, i32::MAX),
        -4 * i64::from(i32::MAX)
    );
    // Saturation happens for large products and never for small ones.
    assert_eq!(
        pr_platform_win::wic::decoder::output_byte_length(i32::MAX, i32::MAX),
        i64::MAX
    );
    assert_eq!(
        pr_platform_win::wic::decoder::output_byte_length(1, i32::MAX),
        4 * i64::from(i32::MAX)
    );
}
