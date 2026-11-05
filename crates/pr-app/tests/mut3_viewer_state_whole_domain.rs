//! P4 / mut3 (2026-11-06): whole-domain runs for the three held survivors of `viewmodels/viewer_state.rs`, through the real public
//! API only. Hand written, no golden file, no production change. Replaces the sampled differential of N2 (a model copy of the
//! function and 200,000 random inputs) by arguments whose every step is executed on the REAL `ViewerState`.
//!
//! `fit_zoom` 323:35 and `fit_axis_zoom` 715:35, `fit > 0.0` -> `>= 0.0`: the two differ only for a finite `fit` that is `+-0`.
//!   Fact 1 (executed): for every state the real functions return `0.0` on the guard edges, and past the guards the inputs are
//!   exactly `limit in (1, f64::MAX]`, `source in [1, i32::MAX]` (`set_source_size` clamps negatives to 0), `dpi` normalised into
//!   `{1.0} u [0.25, 16.0]`. The test derives the lower end of the DPI range from the real function (the result for any dpi below
//!   0.25 equals the result for 1.0 and the result at 0.25 is the smallest of all) instead of assuming it.
//!   Fact 2 (IEEE 754, executed at the corner): `fl(fl(limit * dpi) / source)` is monotone non-decreasing in limit and dpi and
//!   non-increasing in source, because correct rounding is monotone. So its minimum over the domain is attained at
//!   `limit = next_up(1.0)`, `dpi = 0.25`, `source = i32::MAX`. The test evaluates the real function there: a positive normal
//!   number. Hence `fit >= corner > 0` for every state with a known fit and `fit` is never `+-0`; `>` and `>=` agree.
//!   A broad lattice (every exponent of the limit with edge mantissas x 24 dpi classes x 10 source sizes) confirms on the real
//!   function that every result is either `+0.0` (only when the raw quotient overflows to infinity) or `>= corner`.
//!
//! `fit_axis_zoom` 711:24, guard `source <= 0 || !limit.is_finite() || limit <= 1.0` -> `source <= 0 && !limit.is_finite() ||
//! limit <= 1.0`. The two guards disagree exactly on D = D_A u D_B with D_A = {source <= 0, limit finite and > 1} and
//! D_B = {source > 0, limit = +inf or NaN} (`limit = -inf` is caught by `limit <= 1.0` in both). In D the mutant would compute
//! `fit = (limit * dpi) / source`. The result is `0.0` anyway, by IEEE special-value rules that do not depend on the mantissas:
//!   D_A: `limit * dpi >= 0.25 * next_up(1.0) > 0` (finite or `+inf`), `source` is 0 in the public domain, so the quotient is
//!        `+inf`; for a negative source (private-function domain only) it is negative or `-inf`. Neither is `> 0.0` and finite.
//!   D_B: `+inf * dpi = +inf`, `+inf / source = +inf`; `NaN` stays `NaN`. Neither is finite.
//! The test runs the real `fit_width_zoom` and `fit_height_zoom` on D_A (every exponent of the limit with edge mantissas, source
//! 0, 24 dpi classes), on D_B (limit `+inf` and several NaN payloads, 14 source sizes, 24 dpi classes) and checks `+0.0` bit for
//! bit; and on all other classes against a specification. Honest scope: the negative-source part of D_A is outside the public
//! domain (clamped to 0) and is covered by the sign rule only.

use pr_app::viewmodels::viewer_state::ViewerState;

fn state(max_w: f64, max_h: f64, src_w: i32, src_h: i32, dpi: f64) -> ViewerState {
    let mut s = ViewerState::new();
    s.set_max_image_width(max_w);
    s.set_max_image_height(max_h);
    s.set_source_size(src_w, src_h, true);
    s.set_dpi_scale(dpi);
    s
}

fn next_up(x: f64) -> f64 {
    f64::from_bits(x.to_bits() + 1)
}

fn next_down(x: f64) -> f64 {
    f64::from_bits(x.to_bits() - 1)
}

fn dpi_classes() -> Vec<f64> {
    vec![
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -f64::MAX,
        -1.0,
        -0.0,
        0.0,
        5e-324,
        0.1,
        next_down(0.25),
        0.25,
        next_up(0.25),
        0.5,
        1.0,
        1.25,
        4.0,
        next_down(16.0),
        16.0,
        next_up(16.0),
        17.0,
        100.0,
        1e300,
        f64::MAX,
        f64::from_bits(0x7ff8_0000_0000_0001), // NaN with a payload
    ]
}

/// Every positive limit class past 1.0: all exponents from 1023 (1.x) to 2046 with edge mantissas, plus `next_up(1.0)`.
fn limit_classes_above_one() -> Vec<f64> {
    let mut v = vec![next_up(1.0)];
    for exponent in 1023u64..=2046 {
        for m in [0u64, 1, 2, 1 << 51, (1 << 52) - 2, (1 << 52) - 1] {
            let x = f64::from_bits((exponent << 52) | m);
            if x > 1.0 {
                v.push(x);
            }
        }
    }
    v
}

/// `(limit * dpi_eff) / source` as the C# formula, `dpi_eff` the normalised DPI (a copy of the rule, checked against the real
/// function by every comparison below).
fn raw_fit(limit: f64, dpi: f64, source: i32) -> f64 {
    let dpi_eff = if (0.25..=16.0).contains(&dpi) { dpi } else { 1.0 };
    (limit * dpi_eff) / f64::from(source)
}

fn expected_axis(limit: f64, source: i32, dpi: f64) -> f64 {
    if source <= 0 || !limit.is_finite() || limit <= 1.0 {
        return 0.0;
    }
    let fit = raw_fit(limit, dpi, source);
    if fit.is_finite() && fit > 0.0 { fit } else { 0.0 }
}

const SOURCES: [i32; 10] = [1, 2, 3, 7, 1000, 65_536, 1 << 24, 1 << 30, i32::MAX - 1, i32::MAX];

fn corner() -> f64 {
    state(next_up(1.0), next_up(1.0), i32::MAX, i32::MAX, 0.25).fit_width_zoom()
}

#[test]
fn fit_axis_and_fit_zoom_corner_is_a_positive_normal_number_and_the_dpi_minimum_is_the_real_one() {
    let c = corner();
    assert!(c > 0.0 && c.is_normal() && c.is_sign_positive(), "{c:e}");
    // The same corner through fit_height_zoom and fit_zoom (the two quotients are equal, so the minimum is the corner).
    assert_eq!(
        state(next_up(1.0), next_up(1.0), i32::MAX, i32::MAX, 0.25)
            .fit_height_zoom()
            .to_bits(),
        c.to_bits()
    );
    assert_eq!(
        state(next_up(1.0), next_up(1.0), i32::MAX, i32::MAX, 0.25)
            .fit_zoom()
            .to_bits(),
        c.to_bits()
    );
    // DPI: every implausible value gives the result of 1.0, which is larger than the corner; at and above 0.25 the result grows.
    let at_one = state(next_up(1.0), next_up(1.0), i32::MAX, i32::MAX, 1.0).fit_width_zoom();
    assert!(at_one > c);
    for dpi in [
        f64::NAN,
        f64::NEG_INFINITY,
        -1.0,
        -0.0,
        0.0,
        5e-324,
        0.1,
        next_down(0.25),
        17.0,
        next_up(16.0),
        1e300,
        f64::MAX,
        f64::INFINITY,
    ] {
        assert_eq!(
            state(next_up(1.0), next_up(1.0), i32::MAX, i32::MAX, dpi)
                .fit_width_zoom()
                .to_bits(),
            at_one.to_bits(),
            "dpi {dpi:?} must fall back to 1.0"
        );
    }
    let mut dpi = 0.25;
    let mut previous = c;
    for _ in 0..4000 {
        dpi = next_up(dpi);
        let r = state(next_up(1.0), next_up(1.0), i32::MAX, i32::MAX, dpi).fit_width_zoom();
        assert!(r >= previous, "the result must not decrease with the dpi: {dpi:?}");
        previous = r;
    }
}

#[test]
fn fit_axis_and_fit_zoom_never_return_a_value_between_zero_and_the_corner_over_the_class_lattice() {
    let c = corner();
    let limits = limit_classes_above_one();
    let dpis = dpi_classes();
    let mut positive = 0u64;
    let mut overflow_zero = 0u64;
    for &limit in &limits {
        for &dpi in &dpis {
            for &source in &SOURCES {
                let s = state(limit, limit, source, source, dpi);
                let (w, h, z) = (s.fit_width_zoom(), s.fit_height_zoom(), s.fit_zoom());
                let want = expected_axis(limit, source, dpi);
                assert_eq!(w.to_bits(), want.to_bits(), "fit_width_zoom {limit:e} {source} {dpi:?}");
                assert_eq!(
                    h.to_bits(),
                    want.to_bits(),
                    "fit_height_zoom {limit:e} {source} {dpi:?}"
                );
                assert_eq!(z.to_bits(), want.to_bits(), "fit_zoom {limit:e} {source} {dpi:?}");
                if want == 0.0 {
                    assert!(want.is_sign_positive());
                    assert!(
                        !raw_fit(limit, dpi, source).is_finite(),
                        "a zero result needs an infinite quotient"
                    );
                    overflow_zero += 1;
                } else {
                    assert!(want >= c, "{want:e} below the corner {c:e}");
                    positive += 1;
                }
            }
        }
    }
    assert!(positive > 100_000 && overflow_zero > 100, "{positive} {overflow_zero}");
}

#[test]
fn fit_zoom_takes_the_smaller_axis_and_is_never_between_zero_and_the_corner_for_mixed_sizes() {
    let c = corner();
    let limits: Vec<f64> = limit_classes_above_one().into_iter().step_by(23).collect();
    let dpis = dpi_classes();
    let mut known = 0u64;
    for &lw in &limits {
        for &lh in limits.iter().step_by(5) {
            for &dpi in dpis.iter().step_by(2) {
                for (sw, sh) in [(1, i32::MAX), (i32::MAX, 1), (1000, 3), (7, 7)] {
                    let s = state(lw, lh, sw, sh, dpi);
                    let z = s.fit_zoom();
                    let (a, b) = (raw_fit(lw, dpi, sw), raw_fit(lh, dpi, sh));
                    let fit = if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) };
                    let want = if fit.is_finite() && fit > 0.0 { fit } else { 0.0 };
                    assert_eq!(z.to_bits(), want.to_bits(), "fit_zoom {lw:e} {lh:e} {sw} {sh} {dpi:?}");
                    if want != 0.0 {
                        assert!(want >= c);
                        known += 1;
                    }
                }
            }
        }
    }
    assert!(known > 1000, "{known}");
    // Guard edges of fit_zoom: unknown source, unusable limits.
    for (lw, lh, sw, sh) in [
        (1000.0, 1000.0, 0, 5),
        (1000.0, 1000.0, 5, 0),
        (1.0, 1000.0, 5, 5),
        (1000.0, 1.0, 5, 5),
        (f64::NAN, 1000.0, 5, 5),
        (1000.0, f64::INFINITY, 5, 5),
        (f64::NEG_INFINITY, 1000.0, 5, 5),
    ] {
        assert_eq!(
            state(lw, lh, sw, sh, 1.0).fit_zoom().to_bits(),
            0.0f64.to_bits(),
            "{lw:?} {lh:?} {sw} {sh}"
        );
    }
}

#[test]
fn fit_axis_zoom_guard_disagreement_set_d_always_gives_positive_zero() {
    let dpis = dpi_classes();
    let limits = limit_classes_above_one();
    let mut d_a = 0u64;
    let mut d_b = 0u64;
    // D_A: source 0 (the only non-positive source the public API can set; negative sizes are clamped to 0), limit finite > 1.
    for &limit in &limits {
        for &dpi in &dpis {
            let s = state(limit, limit, 0, 0, dpi);
            assert_eq!(
                s.fit_width_zoom().to_bits(),
                0.0f64.to_bits(),
                "D_A width {limit:e} {dpi:?}"
            );
            assert_eq!(
                s.fit_height_zoom().to_bits(),
                0.0f64.to_bits(),
                "D_A height {limit:e} {dpi:?}"
            );
            // Negative sources set through the public setter are clamped to 0 and give the same.
            let n = state(limit, limit, i32::MIN, -1, dpi);
            assert_eq!(n.fit_width_zoom().to_bits(), 0.0f64.to_bits());
            assert_eq!(n.fit_height_zoom().to_bits(), 0.0f64.to_bits());
            d_a += 1;
        }
    }
    // D_B: limit +inf or NaN (several payloads and both signs), source > 0.
    let nan_payloads = [
        f64::NAN,
        -f64::NAN,
        f64::from_bits(0x7ff0_0000_0000_0001),
        f64::from_bits(0xfff8_0000_0000_0001),
        f64::from_bits(0x7fff_ffff_ffff_ffff),
    ];
    let mut limits_b = vec![f64::INFINITY];
    limits_b.extend(nan_payloads);
    for &limit in &limits_b {
        assert!(limit.is_nan() || limit == f64::INFINITY);
        for &dpi in &dpis {
            for &source in &SOURCES {
                let s = state(limit, limit, source, source, dpi);
                assert_eq!(
                    s.fit_width_zoom().to_bits(),
                    0.0f64.to_bits(),
                    "D_B width {limit:?} {source} {dpi:?}"
                );
                assert_eq!(
                    s.fit_height_zoom().to_bits(),
                    0.0f64.to_bits(),
                    "D_B height {limit:?} {source} {dpi:?}"
                );
                assert_eq!(s.fit_zoom().to_bits(), 0.0f64.to_bits());
                d_b += 1;
            }
        }
    }
    // limit = -inf is NOT in D (limit <= 1.0 is true in both guards) and gives the same.
    for &dpi in &dpis {
        for &source in &SOURCES {
            assert_eq!(
                state(f64::NEG_INFINITY, f64::NEG_INFINITY, source, source, dpi)
                    .fit_width_zoom()
                    .to_bits(),
                0.0f64.to_bits()
            );
        }
    }
    assert!(d_a > 20_000 && d_b > 1000, "{d_a} {d_b}");
}
