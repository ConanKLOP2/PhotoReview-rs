//! P4 / mut3 (2026-11-06): tie-set tests for `viewport/layout.rs`. Hand written, no golden file, no production change.
//!
//! * `wpf_double::greater_than` 77:16 `value1 > value2` -> `>=`. The mutant differs from the original only where
//!   `value1 == value2`. Whole-domain argument, executed here: (1) for `value1 != value2` (or a NaN) `>` and `>=` are the same
//!   predicate, so the two functions are the same function off the tie set; (2) the tie set is `{(x, x) : x not NaN}` plus
//!   `(+0, -0)` and `(-0, +0)`; on it `are_close` returns on its first statement without arithmetic, so `greater_than` is
//!   `false` for every tie, whatever x is. The tests run the real public function on the whole order-type lattice of the tie
//!   set (every equal pair of the 9-value pool, all four zero pairs) and on a sweep of every sign x exponent class with edge
//!   mantissas (2 x 2048 x 192 bit patterns), and compare with a reference written from the C# text off the tie set.
//! * The remaining four mutants of `layout.rs` this task was asked about (`net_max` 96:10 `>=`, `net_min` 108:10 `<=`,
//!   `arrange_length` 349:15 `<=` and 353:22 `<=`) are in PRIVATE functions that no input of `compute` can drive onto the
//!   operands that tell them apart (see the evidence file). The last test here is run-support for that claim, not a proof.

use pr_app::viewport::layout::{compute, wpf_double};
use pr_app::viewport::types::{ScrollBarPolicy, ViewerStretchMode, ViewportInput};

const NZ: f64 = -0.0;
const ORDER_POOL: [f64; 9] = [
    f64::NAN,
    f64::NEG_INFINITY,
    -2.0,
    -1.0,
    NZ,
    0.0,
    1.0,
    2.0,
    f64::INFINITY,
];

/// `DoubleUtil.AreClose` from the C# text.
fn reference_are_close(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let eps = (a.abs() + b.abs() + 10.0) * f64::EPSILON;
    let delta = a - b;
    -eps < delta && eps > delta
}

fn reference_greater_than(a: f64, b: f64) -> bool {
    a > b && !reference_are_close(a, b)
}

#[test]
fn greater_than_is_false_on_every_tie_of_the_order_pool_and_matches_the_reference_on_every_pair() {
    let mut ties = 0u32;
    for &a in &ORDER_POOL {
        for &b in &ORDER_POOL {
            assert_eq!(
                wpf_double::greater_than(a, b),
                reference_greater_than(a, b),
                "greater_than({a:?}, {b:?})"
            );
            if a == b {
                assert!(!wpf_double::greater_than(a, b), "tie ({a:?}, {b:?})");
                ties += 1;
            }
        }
    }
    // Ties of the pool: the 8 non-NaN values equal to themselves, plus (+0, -0) and (-0, +0).
    assert_eq!(ties, 8 + 2);
    assert!(!wpf_double::greater_than(0.0, NZ));
    assert!(!wpf_double::greater_than(NZ, 0.0));
}

#[test]
fn greater_than_is_false_on_the_tie_of_every_sign_exponent_class_with_edge_mantissas() {
    let mut mantissas: Vec<u64> = (0..64).collect();
    mantissas.extend(((1u64 << 52) - 64)..(1u64 << 52));
    for bit in 0..52 {
        mantissas.push(1u64 << bit);
        mantissas.push(((1u64 << 52) - 1) ^ (1u64 << bit));
    }
    mantissas.extend([0x5_5555_5555_5555, 0xA_AAAA_AAAA_AAAA]);
    mantissas.sort_unstable();
    mantissas.dedup();
    let mut tested = 0u64;
    for sign in [0u64, 1] {
        for exponent in 0..2048u64 {
            for &m in &mantissas {
                let x = f64::from_bits((sign << 63) | (exponent << 52) | m);
                if x.is_nan() {
                    continue;
                }
                assert!(!wpf_double::greater_than(x, x), "tie {:#x}", x.to_bits());
                tested += 1;
            }
        }
    }
    assert!(tested > 700_000, "{tested}");
}

#[test]
fn greater_than_matches_the_reference_on_pairs_around_every_tie() {
    // 0..=40 ulps either side of ten centres, both orders: the neighbourhood of the tie, where the are_close arithmetic decides.
    let centres = [
        0.0,
        1.0,
        1e-300,
        1e300,
        12345.678,
        5e-324,
        f64::MAX,
        -1.0,
        -7.5e10,
        f64::MIN_POSITIVE,
    ];
    for &c in &centres {
        for k in -40i64..=40 {
            for j in -40i64..=40 {
                let a = f64::from_bits((c.to_bits() as i64).wrapping_add(k) as u64);
                let b = f64::from_bits((c.to_bits() as i64).wrapping_add(j) as u64);
                assert_eq!(
                    wpf_double::greater_than(a, b),
                    reference_greater_than(a, b),
                    "greater_than({a:?}, {b:?})"
                );
            }
        }
    }
}

/// `net_max` of the layout (used only to rebuild the arrange slot from the public outputs): .NET `Math.Max`.
fn net_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b {
        a
    } else if b > a {
        b
    } else if a.is_sign_positive() {
        a
    } else {
        b
    }
}

#[test]
fn compute_never_yields_a_negative_zero_extent_viewport_or_arrange_slot_over_the_lattice_of_zero_carrying_inputs() {
    // Run-support (NOT a proof) for the unobservability of `arrange_length` 349:15 `<=` and 353:22 `<=`, which differ from the
    // original only when the slot and the unclipped size (resp. the effective max and the length) are a +0 / -0 pair. A -0 can
    // enter `compute` only through the explicit size and the max size (the other fields are sanitized to +0). This lattice
    // takes every class of those four fields, both zeros included, with every policy and stretch mode.
    let zero_carriers = [f64::NAN, NZ, 0.0, 1.0, f64::INFINITY, -1.0];
    let mut checked = 0u64;
    for &image_width in &zero_carriers {
        for &image_height in &zero_carriers {
            for &max_image_width in &zero_carriers {
                for &max_image_height in &zero_carriers {
                    for client in [0.0, 7.0] {
                        for thickness in [0.0, 1.0] {
                            for bitmap in [0.0, 3.0] {
                                for scroll_bars in
                                    [ScrollBarPolicy::Auto, ScrollBarPolicy::Overlay, ScrollBarPolicy::Hidden]
                                {
                                    for stretch in [ViewerStretchMode::Uniform, ViewerStretchMode::None] {
                                        let layout = compute(&ViewportInput {
                                            client_width: client,
                                            client_height: client,
                                            scroll_bar_thickness: thickness,
                                            scroll_bars,
                                            stretch,
                                            image_width,
                                            image_height,
                                            max_image_width,
                                            max_image_height,
                                            bitmap_width: bitmap,
                                            bitmap_height: bitmap,
                                        });
                                        let nz = NZ.to_bits();
                                        for v in [
                                            layout.extent_width,
                                            layout.extent_height,
                                            layout.viewport_width,
                                            layout.viewport_height,
                                            layout.max_horizontal_offset,
                                            layout.max_vertical_offset,
                                            net_max(layout.extent_width, layout.viewport_width),
                                            net_max(layout.extent_height, layout.viewport_height),
                                        ] {
                                            assert_ne!(v.to_bits(), nz, "-0 output for {layout:?}");
                                        }
                                        checked += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 6u64.pow(4) * 2 * 2 * 2 * 3 * 2);
}
