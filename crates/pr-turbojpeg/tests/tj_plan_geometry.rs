//! TJ-PLAN (2026-11-11): direct tests of `plan_geometry`, the pure size part of `FinishPlan` that used to sit inline in
//! `TurboJpegDecoder::decode_bytes` (P2-MUT row 81, mutant 1129:34). No `turbojpeg.dll`, no golden file, no sleep.
//!
//! The reference function below is the OLD inline expression set copied verbatim; the lattice covers the `i32` boundaries.

use pr_imaging::decode_types::DecodeBox;
use pr_turbojpeg::decoder::{
    PlanGeometry, SUPPORTED_SCALING_FACTORS, calculate_output_buffer, plan_geometry, select_scaling_factor,
};
use pr_turbojpeg::native::TjScalingFactor;

/// Sides around the powers of two, the 8-pixel DCT block, the `buffer_length` overflow edge (46340/46341: 46341^2*4 > i32::MAX)
/// and `i32::MAX`.
const SIDES: [i32; 21] = [
    1,
    2,
    3,
    7,
    8,
    9,
    15,
    16,
    17,
    63,
    64,
    65,
    4000,
    32_767,
    32_768,
    46_340,
    46_341,
    65_535,
    65_536,
    i32::MAX - 1,
    i32::MAX,
];

/// OLD inline code (before the extraction), `calculate_output_buffer` kept as the shared helper.
fn old_plan(
    downscale: bool,
    orig_width: i32,
    orig_height: i32,
    target_width: i32,
    target_height: i32,
    transposed: bool,
    factor: TjScalingFactor,
) -> Option<PlanGeometry> {
    let output = calculate_output_buffer(orig_width, orig_height, factor).ok()?;
    let (scaled_width, scaled_height, buffer_length) = (output.width, output.height, output.buffer_length);
    Some(PlanGeometry {
        scaled_width,
        scaled_height,
        buffer_length,
        fine_scale: downscale && (scaled_width > target_width || scaled_height > target_height),
        downscaled: downscale
            && (scaled_width < orig_width || target_width < orig_width || target_height < orig_height),
        original_width: if transposed { orig_height } else { orig_width },
        original_height: if transposed { orig_width } else { orig_height },
    })
}

/// `target` values to try for a source side: 0 (unconstrained), the side and its neighbours, and the lattice.
fn targets_for(side: i32) -> Vec<i32> {
    let mut values: Vec<i32> = vec![0, 1, side.saturating_sub(1).max(1), side, side.saturating_add(1)];
    values.extend(SIDES);
    values.sort_unstable();
    values.dedup();
    values
}

#[test]
fn plan_geometry_equals_the_old_inline_expressions_over_the_lattice_times_all_factors() {
    let mut compared = 0u64;
    let mut oversized = 0u64;
    for &orig_width in &SIDES {
        for &orig_height in &SIDES {
            for factor in SUPPORTED_SCALING_FACTORS {
                let tws = targets_for(orig_width);
                let ths = targets_for(orig_height);
                for downscale in [false, true] {
                    for transposed in [false, true] {
                        for &tw in &tws {
                            for &th in &ths {
                                let new = plan_geometry(downscale, orig_width, orig_height, tw, th, transposed, factor);
                                let old = old_plan(downscale, orig_width, orig_height, tw, th, transposed, factor);
                                assert_eq!(
                                    new.ok(),
                                    old,
                                    "{orig_width}x{orig_height} {factor:?} t={tw}x{th} d={downscale} tr={transposed}"
                                );
                                if old.is_none() {
                                    oversized += 1;
                                }
                                compared += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(compared > 1_000_000, "{compared}");
    assert!(oversized > 0, "the Err branch must be exercised");
}

#[test]
fn downscaled_has_three_independent_causes_and_requires_downscale() {
    let one = TjScalingFactor::ONE;
    let half = TjScalingFactor { num: 1, denom: 2 };
    let flags = |downscale, tw, th, factor| {
        let geometry = plan_geometry(downscale, 100, 80, tw, th, false, factor).expect("fits");
        (geometry.downscaled, geometry.fine_scale)
    };
    // Only the scaled width is below the source (factor 1/2 shrinks 100x80 to 50x40; target equals the source).
    assert_eq!(flags(true, 100, 80, half), (true, false));
    // Only the target width is below the source.
    assert_eq!(flags(true, 99, 80, one), (true, true));
    // Only the target height is below the source.
    assert_eq!(flags(true, 100, 79, one), (true, true));
    // Nothing is smaller.
    assert_eq!(flags(true, 100, 80, one), (false, false));
    assert_eq!(flags(true, 120, 90, one), (false, false));
    // `downscale == false` forces both flags off, whatever the sizes.
    assert_eq!(flags(false, 10, 10, half), (false, false));
    assert_eq!(flags(false, 99, 80, one), (false, false));
    // Fine scale: scaled side above the target on either axis, only with `downscale`.
    assert_eq!(flags(true, 49, 40, half), (true, true));
    assert_eq!(flags(true, 50, 39, half), (true, true));
    assert_eq!(flags(true, 50, 40, half), (true, false));
    assert_eq!(flags(false, 49, 39, half), (false, false));
}

#[test]
fn transposed_swaps_the_original_size_and_oversized_output_is_refused() {
    let geometry = plan_geometry(false, 7, 9, 0, 0, true, TjScalingFactor::ONE).expect("fits");
    assert_eq!((geometry.original_width, geometry.original_height), (9, 7));
    let geometry = plan_geometry(false, 7, 9, 0, 0, false, TjScalingFactor::ONE).expect("fits");
    assert_eq!((geometry.original_width, geometry.original_height), (7, 9));
    assert_eq!(
        (geometry.scaled_width, geometry.scaled_height, geometry.buffer_length),
        (7, 9, 7 * 9 * 4)
    );
    let refused = plan_geometry(true, i32::MAX, i32::MAX, 1, 1, false, TjScalingFactor::ONE).expect_err("too large");
    assert_eq!(
        (refused.width, refused.height),
        (i64::from(i32::MAX), i64::from(i32::MAX))
    );
}

/// Production pipeline: box -> `fit_stored` -> `select_scaling_factor` (only when a side shrinks) -> `plan_geometry`.
/// The native `set_scaling_factor` may refuse the factor, which falls back to 1/1: both variants are run.
/// Reachable-domain fact: `scaled_width < orig_width` implies `target_width < orig_width || target_height < orig_height`
/// (a factor other than 1/1 is only selected then), so the first `downscaled` disjunct is redundant in production, yet kept.
#[test]
fn production_pipeline_matches_the_old_formula_and_the_first_disjunct_is_redundant_there() {
    let boxes = [
        DecodeBox::UNBOUNDED,
        DecodeBox::new(1, 0),
        DecodeBox::new(0, 1),
        DecodeBox::new(1, 1),
        DecodeBox::new(100, 0),
        DecodeBox::new(0, 100),
        DecodeBox::new(100, 100),
        DecodeBox::new(1920, 1080),
        DecodeBox::new(65_536, 65_536),
        DecodeBox::new(i32::MAX, i32::MAX),
        DecodeBox::new(i32::MAX, 1),
    ];
    let mut redundant_cases = 0u64;
    for &orig_width in &SIDES {
        for &orig_height in &SIDES {
            for decode_box in boxes {
                for transposed in [false, true] {
                    let downscale = decode_box.width > 0 || decode_box.height > 0;
                    let (mut tw, mut th) = (orig_width, orig_height);
                    let mut factor = TjScalingFactor::ONE;
                    if downscale {
                        (tw, th) = decode_box.fit_stored(orig_width, orig_height, transposed);
                        if tw < orig_width || th < orig_height {
                            factor = select_scaling_factor(orig_width, orig_height, tw, th);
                        }
                    }
                    for used in [factor, TjScalingFactor::ONE] {
                        let new = plan_geometry(downscale, orig_width, orig_height, tw, th, transposed, used);
                        let old = old_plan(downscale, orig_width, orig_height, tw, th, transposed, used);
                        assert_eq!(new.ok(), old);
                        if let Some(old) = old {
                            let without_first = downscale && (tw < orig_width || th < orig_height);
                            assert_eq!(
                                old.downscaled, without_first,
                                "{orig_width}x{orig_height} {decode_box:?} {used:?}"
                            );
                            if old.scaled_width < orig_width {
                                redundant_cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(redundant_cases > 0, "the lattice must reach scaled_width < orig_width");
}
