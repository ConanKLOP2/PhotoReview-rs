//! N2 (mutation close-out, 2026-11-05): run-proofs for the four survivors of `viewmodels/viewer_state.rs` that M3
//! (`docs/reviews/M3-MUT-2026-11-04.md`) only argued from reading the code. Hand written, no golden file.
//!
//! * `is_fit` 296:79 `<` -> `<=` (`|zoom - 1.0| < 0.001`): `|zoom - 1.0|` is exact in `[0.5, 2]` (Sterbenz) and then a
//!   multiple of 2^-53 (below 1) or 2^-52 (above 1); the f64 `0.001` has 2 trailing zero bits in its significand and sits
//!   at 2^-62 granularity, so the difference can never equal it. The test asserts that significand fact and sweeps every
//!   f64 within 6000 ulps of `1 +- 0.001` against the strict spec through the public API.
//! * `fit_zoom` 323:35 and `fit_axis_zoom` 715:35 `>` -> `>=` (`fit > 0.0`): the two differ only for `fit == 0.0`. For a
//!   known source (`> 0` pixels, `<= i32::MAX`), a limit `> 1` and a plausible DPI scale (`>= 0.25`, or the fallback 1.0), `fit`
//!   is a correctly rounded `limit * dpi / source` and rounding is monotone, so `fit >= fl(fl(next_up(1.0) * 0.25) /
//!   i32::MAX)`, a normal positive number. The test computes that bound and sweeps a lattice plus 400,000 random
//!   combinations through `fit_zoom`, `fit_width_zoom` and `fit_height_zoom`, counting the raw `fit == 0.0` cases (there
//!   are none) and comparing every result bit for bit with a model of the ORIGINAL.
//! * `fit_axis_zoom` 711:24 `||` -> `&&` (the guard `source <= 0 || !limit.is_finite() || limit <= 1.0` becomes
//!   `source <= 0 && !limit.is_finite() || limit <= 1.0`): the same differential, with a model of the mutant run next to the
//!   model of the original. The counters show the lattice reaches the cases where the two guards disagree (a non-positive
//!   source with a finite limit above 1, and a non-finite limit with a positive source); in every one the final
//!   `fit.is_finite() && fit > 0.0` check yields `0.0` as well, so no output differs.

use pr_app::viewmodels::viewer_state::ViewerState;
use pr_app::viewport::types::ViewerStretchMode;

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

// ---------------------------------------------------------------------------------------------------------------------
// is_fit 296:79
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn is_fit_tolerance_boundary_sweep_over_every_f64_neighbour() {
    const TOL: f64 = 0.001;
    // 0.001 lies in [2^-10, 2^-9): its ulp is 2^-62. |zoom - 1| for zoom in [0.5, 1) is a multiple of 2^-53, and in [1, 2) of
    // 2^-52, so equality needs a significand divisible by 2^9 resp. 2^10.
    let sig = (TOL.to_bits() & ((1u64 << 52) - 1)) | (1u64 << 52);
    assert_eq!(TOL.to_bits() >> 52, 1023 - 10);
    assert!(
        sig.trailing_zeros() < 9,
        "trailing zeros of the 0.001 significand: {}",
        sig.trailing_zeros()
    );

    let mut state = ViewerState::new();
    state.set_stretch(ViewerStretchMode::Uniform);
    let (mut inside, mut outside) = (0u64, 0u64);
    for centre in [1.0 + TOL, 1.0 - TOL] {
        for k in -6000i64..=6000 {
            let z = f64::from_bits((centre.to_bits() as i64 + k) as u64);
            assert_ne!((z - 1.0).abs(), TOL, "z = {z:?}");
            state.set_zoom_raw(z);
            let expected = (z - 1.0).abs() < TOL;
            assert_eq!(state.is_fit(), expected, "z = {z:?}");
            if expected {
                inside += 1;
            } else {
                outside += 1;
            }
        }
    }
    assert!(inside > 5000 && outside > 5000, "{inside} {outside}");
    // Stretch None is never fit, whatever the zoom.
    let mut none = ViewerState::new();
    none.set_stretch(ViewerStretchMode::None);
    none.set_zoom_raw(1.0);
    assert!(!none.is_fit());
}

// ---------------------------------------------------------------------------------------------------------------------
// fit_zoom 323:35, fit_axis_zoom 711:24 and 715:35
// ---------------------------------------------------------------------------------------------------------------------

fn normalize_dpi(dpi: f64) -> f64 {
    if (0.25..=16.0).contains(&dpi) { dpi } else { 1.0 }
}

fn math_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

#[derive(Default)]
struct Counts {
    raw_zero_fit: u64,
    guard_disagree_nonpositive_source: u64,
    guard_disagree_nonfinite_limit: u64,
    known_results: u64,
}

/// `fit_axis_zoom` of the source with the guard at 711:24 and the compare at 715:35 switchable.
fn fit_axis_model(limit: f64, source: i32, dpi_scale: f64, mutate_guard: bool, ge: bool, counts: &mut Counts) -> f64 {
    let a = source <= 0;
    let b = !limit.is_finite();
    let c = limit <= 1.0;
    let orig_guard = a || b || c;
    let mut_guard = (a && b) || c;
    if orig_guard != mut_guard {
        if a && !b && !c {
            counts.guard_disagree_nonpositive_source += 1;
        } else {
            counts.guard_disagree_nonfinite_limit += 1;
        }
    }
    let guard = if mutate_guard { mut_guard } else { orig_guard };
    if guard {
        return 0.0;
    }
    let fit = (limit * normalize_dpi(dpi_scale)) / f64::from(source);
    if !orig_guard && fit == 0.0 {
        counts.raw_zero_fit += 1;
    }
    if !orig_guard {
        counts.known_results += 1;
    }
    let positive = if ge { fit >= 0.0 } else { fit > 0.0 };
    if fit.is_finite() && positive { fit } else { 0.0 }
}

fn fit_zoom_model(
    max_w: f64,
    max_h: f64,
    src_w: i32,
    src_h: i32,
    dpi_scale: f64,
    ge: bool,
    counts: &mut Counts,
) -> f64 {
    if src_w <= 0 || src_h <= 0 {
        return 0.0;
    }
    let usable = |v: f64| v.is_finite() && v > 1.0;
    if !usable(max_w) || !usable(max_h) {
        return 0.0;
    }
    let dpi = normalize_dpi(dpi_scale);
    let fit = math_min((max_w * dpi) / f64::from(src_w), (max_h * dpi) / f64::from(src_h));
    if fit == 0.0 {
        counts.raw_zero_fit += 1;
    }
    counts.known_results += 1;
    let positive = if ge { fit >= 0.0 } else { fit > 0.0 };
    if fit.is_finite() && positive { fit } else { 0.0 }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn limit_pool() -> Vec<f64> {
    vec![
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        -0.0,
        0.0,
        5e-324,
        f64::MIN_POSITIVE,
        0.5,
        1.0,
        1.0 + f64::EPSILON,
        1.5,
        2.0,
        100.0,
        1920.0,
        1e6,
        1e300,
        f64::MAX,
    ]
}

fn dpi_pool() -> Vec<f64> {
    vec![
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        0.0,
        0.1,
        0.25 - f64::EPSILON / 8.0,
        0.25,
        1.0,
        1.25,
        2.0,
        16.0,
        16.000000000000004,
        100.0,
        f64::MAX,
    ]
}

const SRC_POOL: [i32; 9] = [i32::MIN, -1, 0, 1, 2, 3, 1000, 65_536, i32::MAX];

fn state_with(max_w: f64, max_h: f64, src_w: i32, src_h: i32, dpi: f64) -> ViewerState {
    let mut s = ViewerState::new();
    s.set_max_image_width(max_w);
    s.set_max_image_height(max_h);
    s.set_source_size(src_w, src_h, true);
    s.set_dpi_scale(dpi);
    s
}

#[test]
fn fit_zoom_lower_bound_is_a_positive_normal_number_so_zero_never_reaches_the_greater_than() {
    // Monotone rounding: the smallest `fit` for a known source is attained at the smallest usable limit (the f64 after 1.0),
    // the smallest plausible DPI (0.25) and the largest source (i32::MAX).
    let smallest = ((1.0f64 + f64::EPSILON) * 0.25) / f64::from(i32::MAX);
    assert!(smallest > 0.0 && smallest.is_normal(), "{smallest:e}");
    // The same bound by brute force over the extreme lattice (any other combination is larger).
    for limit in [1.0 + f64::EPSILON, 1.5, 2.0, 1e6, f64::MAX / 16.0] {
        for dpi in [0.25, 0.5, 1.0, 16.0] {
            for src in [1, 2, 1000, i32::MAX] {
                let fit = (limit * dpi) / f64::from(src);
                assert!(fit >= smallest, "{limit:e} {dpi} {src}");
            }
        }
    }
    // The fallback dpi (1.0, for NaN or implausible values) is above 0.25, so it cannot lower the bound.
    assert!(normalize_dpi(f64::NAN) >= 0.25 && normalize_dpi(0.0) >= 0.25 && normalize_dpi(1e-300) >= 0.25);
}

#[test]
fn fit_zoom_matches_the_original_model_and_never_sees_a_zero_fit() {
    let limits = limit_pool();
    let dpis = dpi_pool();
    let mut counts = Counts::default();
    let mut checked = 0u64;
    let mut visit = |max_w: f64, max_h: f64, sw: i32, sh: i32, dpi: f64, counts: &mut Counts| {
        let s = state_with(max_w, max_h, sw, sh, dpi);
        let real = s.fit_zoom();
        let orig = fit_zoom_model(max_w, max_h, sw, sh, dpi, false, counts);
        let mutant = fit_zoom_model(max_w, max_h, sw, sh, dpi, true, &mut Counts::default());
        assert!(
            same(real, orig),
            "fit_zoom {real:?} vs {orig:?} for {max_w:?} {max_h:?} {sw} {sh} {dpi:?}"
        );
        assert!(
            same(mutant, orig),
            "`>` -> `>=` changes fit_zoom for {max_w:?} {max_h:?} {sw} {sh} {dpi:?}"
        );
        if real != 0.0 {
            assert!(real.is_finite() && real > 0.0 && real.is_sign_positive());
        } else {
            assert!(real.is_sign_positive(), "unknown fit must be +0.0");
        }
        checked += 1;
    };
    for &mw in &limits {
        for &mh in &limits {
            for &sw in &SRC_POOL {
                for &dpi in &dpis {
                    visit(mw, mh, sw, 1, dpi, &mut counts);
                    visit(mw, mh, 1, sw, dpi, &mut counts);
                }
            }
        }
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..200_000 {
        let r = rng.next();
        let pick = |rng: &mut Rng, pool: &[f64]| pool[(rng.next() % pool.len() as u64) as usize];
        let mw = if r & 1 == 0 {
            f64::from_bits(rng.next())
        } else {
            pick(&mut rng, &limits)
        };
        let mh = if r & 2 == 0 {
            f64::from_bits(rng.next())
        } else {
            pick(&mut rng, &limits)
        };
        let dpi = if r & 4 == 0 {
            f64::from_bits(rng.next())
        } else {
            pick(&mut rng, &dpis)
        };
        let sw = if r & 8 == 0 {
            rng.next() as i32
        } else {
            SRC_POOL[(rng.next() % 9) as usize]
        };
        let sh = if r & 16 == 0 {
            rng.next() as i32
        } else {
            SRC_POOL[(rng.next() % 9) as usize]
        };
        visit(mw, mh, sw, sh, dpi, &mut counts);
    }
    assert_eq!(
        counts.raw_zero_fit, 0,
        "a zero `fit` is the only value on which `>` and `>=` differ"
    );
    assert!(counts.known_results > 1000, "{}", counts.known_results);
    assert!(checked > 200_000);
}

#[test]
fn fit_axis_zoom_matches_the_original_model_and_both_mutants_are_unobservable() {
    let limits = limit_pool();
    let dpis = dpi_pool();
    let mut orig_counts = Counts::default();
    let mut mut_counts = Counts::default();
    let mut visit = |limit: f64, src: i32, dpi: f64| {
        let width_state = state_with(limit, 0.0, src, 1, dpi);
        let height_state = state_with(0.0, limit, 1, src, dpi);
        let real_w = width_state.fit_width_zoom();
        let real_h = height_state.fit_height_zoom();
        let orig = fit_axis_model(limit, src, dpi, false, false, &mut orig_counts);
        let guard = fit_axis_model(limit, src, dpi, true, false, &mut mut_counts);
        let ge = fit_axis_model(limit, src, dpi, false, true, &mut Counts::default());
        assert!(
            same(real_w, orig),
            "fit_width_zoom {real_w:?} vs {orig:?} for {limit:?} {src} {dpi:?}"
        );
        assert!(
            same(real_h, orig),
            "fit_height_zoom {real_h:?} vs {orig:?} for {limit:?} {src} {dpi:?}"
        );
        assert!(
            same(guard, orig),
            "`||` -> `&&` at 711:24 changes the result for {limit:?} {src} {dpi:?}"
        );
        assert!(
            same(ge, orig),
            "`>` -> `>=` at 715:35 changes the result for {limit:?} {src} {dpi:?}"
        );
        assert!(orig.is_sign_positive());
    };
    for &limit in &limits {
        for &src in &SRC_POOL {
            for &dpi in &dpis {
                visit(limit, src, dpi);
            }
        }
    }
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for _ in 0..200_000 {
        let r = rng.next();
        let limit = if r & 1 == 0 {
            f64::from_bits(rng.next())
        } else {
            limits[(rng.next() % limits.len() as u64) as usize]
        };
        let src = if r & 2 == 0 {
            rng.next() as i32
        } else {
            SRC_POOL[(rng.next() % 9) as usize]
        };
        let dpi = if r & 4 == 0 {
            f64::from_bits(rng.next())
        } else {
            dpis[(rng.next() % dpis.len() as u64) as usize]
        };
        visit(limit, src, dpi);
    }
    // 715:35: no known input produces a zero fit.
    assert_eq!(orig_counts.raw_zero_fit, 0);
    assert!(orig_counts.known_results > 1000, "{}", orig_counts.known_results);
    // 711:24: the lattice reaches both kinds of guard disagreement, and none of them changed an output (asserted above).
    assert!(
        mut_counts.guard_disagree_nonpositive_source > 1000,
        "{}",
        mut_counts.guard_disagree_nonpositive_source
    );
    assert!(
        mut_counts.guard_disagree_nonfinite_limit > 1000,
        "{}",
        mut_counts.guard_disagree_nonfinite_limit
    );
}
