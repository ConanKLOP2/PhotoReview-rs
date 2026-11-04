//! N2 (mutation close-out, 2026-11-05): run-proofs and one killer for the survivors of `viewport/render_geometry.rs` that M2
//! (`docs/reviews/M2-MUT-2026-11-04.md`) only argued from reading the code. Hand written, no golden file.
//!
//! * `math_max` 344:19 `||` -> `&&`, `math_min` 361:10 `<` -> `<=` (private helpers of `intersect`): a differential test of the
//!   real `intersect` against a model of the ORIGINAL code, plus a model of each mutant, on the full lattice of 7 values in all 8
//!   rect fields (7^8 = 5,764,801 rect pairs: NaN, +inf, -1, -0, +0, 1, 2). The real function must equal the original model
//!   bit for bit (that is what fails under a non-equivalent mutant), the mutant models must equal the original model, and the
//!   counters prove that the lattice reaches the cases where the mutated helper itself returns different bits.
//! * `is_unit` 430:31 `<` -> `<=` (private): reached through `try_snap_pixel_exact`; every f64 within 6000 ulps of the four
//!   boundaries `1 +- 1e-6` (and for each of the four matrix entries that go through `is_unit`) is compared with the strict
//!   spec, and the exact significand of `1e-6` proves that `| |v| - 1 | == 1e-6` has no solution in `[0.5, 2]`.
//! * `try_snap_pixel_exact` 479:18 `<` -> `<=`: a searched f32 matrix whose f64 determinant is exactly `1e-6` (see the
//!   test). The original continues and snaps (`exact == true`), the mutant answers "not exact": a killer.

use pr_app::input::primitives::RectD;
use pr_app::viewport::render_geometry::{Matrix3x2, intersect, try_snap_pixel_exact};

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn rect_same(a: RectD, b: RectD) -> bool {
    same(a.x, b.x) && same(a.y, b.y) && same(a.width, b.width) && same(a.height, b.height)
}

// ---------------------------------------------------------------------------------------------------------------------
// math_max 344:19 and math_min 361:10 through intersect
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Hits {
    max_guard_bits_differ: u64,
    min_le_bits_differ: u64,
}

fn math_max_model(a: f64, b: f64, and_guard: bool) -> f64 {
    let nan = if and_guard {
        a.is_nan() && b.is_nan()
    } else {
        a.is_nan() || b.is_nan()
    };
    if nan {
        return f64::NAN;
    }
    if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}

fn math_min_model(a: f64, b: f64, le: bool) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let lt = if le { a <= b } else { a < b };
    if lt || (a == b && a.is_sign_negative()) { a } else { b }
}

/// `intersect` of the source with the two helpers switchable. `(false, false)` is the original.
fn intersect_model(a: RectD, b: RectD, and_guard: bool, le: bool, hits: &mut Hits) -> RectD {
    if and_guard {
        let o = math_max_model(a.x, b.x, false);
        let m = math_max_model(a.x, b.x, true);
        if o.to_bits() != m.to_bits() && !(o.is_nan() && m.is_nan()) {
            hits.max_guard_bits_differ += 1;
        }
        let o = math_max_model(a.y, b.y, false);
        let m = math_max_model(a.y, b.y, true);
        if o.to_bits() != m.to_bits() && !(o.is_nan() && m.is_nan()) {
            hits.max_guard_bits_differ += 1;
        }
    }
    let x = math_max_model(a.x, b.x, and_guard);
    let y = math_max_model(a.y, b.y, and_guard);
    let (ra, rb) = (a.x + a.width, b.x + b.width);
    let (ba, bb) = (a.y + a.height, b.y + b.height);
    if le {
        if math_min_model(ra, rb, false).to_bits() != math_min_model(ra, rb, true).to_bits() {
            hits.min_le_bits_differ += 1;
        }
        if math_min_model(ba, bb, false).to_bits() != math_min_model(ba, bb, true).to_bits() {
            hits.min_le_bits_differ += 1;
        }
    }
    let right = math_min_model(ra, rb, le);
    let bottom = math_min_model(ba, bb, le);
    if right > x && bottom > y {
        RectD {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    } else {
        RectD::default()
    }
}

#[test]
fn intersect_helpers_math_max_nan_guard_and_math_min_equal_operands_are_unobservable_on_the_full_lattice() {
    const VALS: [f64; 7] = [f64::NAN, f64::INFINITY, -1.0, -0.0, 0.0, 1.0, 2.0];
    let mut guard_hits = Hits::default();
    let mut le_hits = Hits::default();
    let mut origin_hits = Hits::default();
    let mut pairs: u64 = 0;
    let mut idx = [0usize; 8];
    loop {
        let f = |i: usize| VALS[idx[i]];
        let a = RectD {
            x: f(0),
            y: f(1),
            width: f(2),
            height: f(3),
        };
        let b = RectD {
            x: f(4),
            y: f(5),
            width: f(6),
            height: f(7),
        };
        let real = intersect(a, b);
        let orig = intersect_model(a, b, false, false, &mut origin_hits);
        assert!(
            rect_same(real, orig),
            "real intersect != model of the original for {a:?} {b:?}: {real:?} vs {orig:?}"
        );
        let guard = intersect_model(a, b, true, false, &mut guard_hits);
        assert!(
            rect_same(guard, orig),
            "`||` -> `&&` in math_max changes intersect for {a:?} {b:?}"
        );
        let le = intersect_model(a, b, false, true, &mut le_hits);
        assert!(
            rect_same(le, orig),
            "`<` -> `<=` in math_min changes intersect for {a:?} {b:?}"
        );
        pairs += 1;

        // odometer over 8 digits in base 7
        let mut d = 0;
        loop {
            idx[d] += 1;
            if idx[d] < VALS.len() {
                break;
            }
            idx[d] = 0;
            d += 1;
            if d == 8 {
                break;
            }
        }
        if d == 8 {
            break;
        }
    }
    assert_eq!(pairs, 7u64.pow(8));
    // The lattice really reaches the cases where the mutated helper itself returns different bits (a NaN `a` with a
    // number `b` for the guard; +0 against -0 for `<=`), so the equality above is not vacuous.
    assert!(
        guard_hits.max_guard_bits_differ > 100_000,
        "{}",
        guard_hits.max_guard_bits_differ
    );
    assert!(le_hits.min_le_bits_differ > 10_000, "{}", le_hits.min_le_bits_differ);
}

// ---------------------------------------------------------------------------------------------------------------------
// is_unit 430:31 through try_snap_pixel_exact
// ---------------------------------------------------------------------------------------------------------------------

const EPS: f64 = 1e-6;

/// Strict spec of C# `IsUnit`: `| |v| - 1 | < 1e-6`.
fn spec_is_unit(v: f64) -> bool {
    (v.abs() - 1.0).abs() < EPS
}

fn unit_src() -> RectD {
    RectD {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    }
}

fn rect(width: f64, height: f64) -> RectD {
    RectD {
        x: 0.0,
        y: 0.0,
        width,
        height,
    }
}

/// `exact` of a snap whose four linear entries are `(a, b, c, d)` up to the scale chosen here; the destination sits on
/// the origin so that every unit-scale case is a pixel-exact draw (`exact == true`).
fn exact_with_unit_probe(entry: char, w: f64, sign: f32) -> bool {
    let m_axis = |m11: f32, m22: f32| Matrix3x2::new(m11, 0.0, 0.0, m22, 0.0, 0.0);
    let m_swap = |m12: f32, m21: f32| Matrix3x2::new(0.0, m12, m21, 0.0, 0.0, 0.0);
    match entry {
        // a = sx * m11 = w * sign, d = 1
        'a' => try_snap_pixel_exact(m_axis(sign, 1.0), rect(w, 1.0), unit_src()).exact,
        // d = sy * m22 = w * sign, a = 1
        'd' => try_snap_pixel_exact(m_axis(1.0, sign), rect(1.0, w), unit_src()).exact,
        // b = sx * m12 = w * sign, c = 1
        'b' => try_snap_pixel_exact(m_swap(sign, 1.0), rect(w, 1.0), unit_src()).exact,
        // c = sy * m21 = w * sign, b = 1
        'c' => try_snap_pixel_exact(m_swap(1.0, sign), rect(1.0, w), unit_src()).exact,
        _ => unreachable!(),
    }
}

#[test]
fn is_unit_boundary_sweep_over_every_f64_neighbour_of_one_plus_minus_epsilon() {
    // Exact significand argument: 1e-6 = sig * 2^-72 with a 53-bit significand. For v in [1, 2) the difference |v| - 1 is a
    // multiple of 2^-52 and for v in [0.5, 1) a multiple of 2^-53 (Sterbenz: the subtraction is exact in [0.5, 2]), so the
    // difference can equal 1e-6 only if the significand has at least 20 resp. 19 trailing zero bits.
    let sig = (EPS.to_bits() & ((1u64 << 52) - 1)) | (1u64 << 52);
    assert_eq!(EPS.to_bits() >> 52, 1023 - 20, "1e-6 lies in [2^-20, 2^-19)");
    assert!(
        sig.trailing_zeros() < 19,
        "trailing zeros of the 1e-6 significand: {}",
        sig.trailing_zeros()
    );

    let upper = 1.0 + EPS; // nearest f64 of 1 + 1e-6 up to one rounding
    let lower = 1.0 - EPS;
    let mut checked = 0u64;
    let mut true_count = 0u64;
    let mut false_count = 0u64;
    for centre in [upper, lower] {
        for k in -6000i64..=6000 {
            let bits = (centre.to_bits() as i64 + k) as u64;
            let w = f64::from_bits(bits);
            // No value in range has a difference of exactly 1e-6 (an equality would separate `<` from `<=`).
            assert_ne!((w - 1.0).abs(), EPS, "w = {w:?}");
            for entry in ['a', 'b', 'c', 'd'] {
                for sign in [1.0f32, -1.0f32] {
                    let got = exact_with_unit_probe(entry, w, sign);
                    let expected = spec_is_unit(w * f64::from(sign));
                    assert_eq!(got, expected, "entry {entry} w {w:?} sign {sign}");
                    checked += 1;
                    if expected {
                        true_count += 1;
                    } else {
                        false_count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 2 * 12_001 * 8);
    // Both sides of both boundaries are inside the sweep.
    assert!(true_count > 1000 && false_count > 1000, "{true_count} {false_count}");
}

// ---------------------------------------------------------------------------------------------------------------------
// try_snap_pixel_exact 479:18: det.abs() == 1e-6 exactly
// ---------------------------------------------------------------------------------------------------------------------

/// Matrix found by a search (all four entries are f32, widened to f64 before the multiply, as the production code does):
/// `m11 * m22 - m12 * m21` is exactly the f64 `1e-6` (every product of two f32 is exact in f64; the difference rounds onto
/// `1e-6`). The linear part scaled by `diag(1 / m11, 1 / m22)` is axis aligned and unit within the 1e-6 tolerance (the
/// off-diagonal entries scale to about 6.8e-7 and 5.9e-7, below 1e-6).
fn det_epsilon_matrix(m21_bits: u32) -> Matrix3x2 {
    Matrix3x2::new(
        f32::from_bits(0x3a67_02b8),
        f32::from_bits(0x3024_ed3f),
        f32::from_bits(m21_bits),
        f32::from_bits(0x3a94_bc93),
        0.0,
        0.0,
    )
}

fn det_of(m: Matrix3x2) -> f64 {
    f64::from(m.m11) * f64::from(m.m22) - f64::from(m.m12) * f64::from(m.m21)
}

fn snap_dest(m: Matrix3x2) -> RectD {
    // sx = 1 / m11 and sy = 1 / m22 against a unit source; x, y put the device corner at a non-integer position.
    RectD {
        x: 300.0,
        y: 200.0,
        width: 1.0 / f64::from(m.m11),
        height: 1.0 / f64::from(m.m22),
    }
}

#[test]
fn snap_with_determinant_exactly_epsilon_still_inverts_and_snaps() {
    // Kills `<` -> `<=` at render_geometry.rs:479:18 (`det.abs() < EPSILON`): with `<=` this matrix is called singular.
    let m = det_epsilon_matrix(0x3039_00dc);
    assert_eq!(
        det_of(m),
        EPS,
        "the searched matrix must have a determinant of exactly 1e-6"
    );
    let dest = snap_dest(m);
    let snap = try_snap_pixel_exact(m, dest, unit_src());
    assert!(snap.exact, "determinant == 1e-6 is not below the singular threshold");

    // The snapped corner lands on an integer device pixel: apply the matrix to the snapped corner.
    let (m11, m12, m21, m22) = (f64::from(m.m11), f64::from(m.m12), f64::from(m.m21), f64::from(m.m22));
    let dev_x = snap.snapped.x * m11 + snap.snapped.y * m21;
    let dev_y = snap.snapped.x * m12 + snap.snapped.y * m22;
    assert!((dev_x - dev_x.round_ties_even()).abs() < 1e-6, "dev_x {dev_x}");
    assert!((dev_y - dev_y.round_ties_even()).abs() < 1e-6, "dev_y {dev_y}");
    // Size is untouched and the corner really moved.
    assert_eq!(snap.snapped.width, dest.width);
    assert_eq!(snap.snapped.height, dest.height);
    assert!((snap.snapped.x - dest.x).abs() > 1.0);
}

#[test]
fn snap_determinant_just_below_epsilon_is_singular_and_just_above_is_not() {
    // Controls around the exact case: a larger m21 lowers the determinant below 1e-6 (not exact), a smaller one raises it.
    let below = det_epsilon_matrix(0x3039_00dc + 20_000);
    let above = det_epsilon_matrix(0x3039_00dc - 20_000);
    assert!(det_of(below) < EPS, "{:e}", det_of(below));
    assert!(det_of(above) > EPS, "{:e}", det_of(above));
    assert!(!try_snap_pixel_exact(below, snap_dest(below), unit_src()).exact);
    assert!(try_snap_pixel_exact(above, snap_dest(above), unit_src()).exact);
}
