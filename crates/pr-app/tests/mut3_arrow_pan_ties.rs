//! P4 / mut3 (2026-11-06): the two equivalent survivors of `viewport/arrow_pan.rs::step` that Q1 argued by reading the code
//! (`81:27` `moved < 0.0` -> `<=` and `83:21` `moved > max` -> `>=`). Hand written, no golden file, no production change.
//! `step` is public, so everything below runs on the real function.
//!
//! 83:21 `moved > max` -> `>=`. The two differ only when `moved == max`. There the original keeps `moved` and the mutant takes
//! `max`: equal values, so equal bits unless both are zeros. `max` is above 0.5 (or NaN, which never equals anything): the
//! guard `max <= 0.5` returns `NotScrollable` before. The run covers the tie directly (a `moved` computed by the function's own
//! arithmetic equal to `max`, for every positive exponent class) and the guard edge. The bit-equality of equal non-zero f64 is the
//! IEEE 754 encoding rule (every non-zero value has one encoding).
//!
//! 81:27 `moved < 0.0` -> `<=`. The two differ only when `moved == 0.0`, and the targets differ in bits only when `moved` is `-0.0`
//! (original keeps it, the mutant writes `+0.0`). IEEE 754 round-to-nearest: `x + y` is `-0.0` only when both addends are `-0.0`
//! (the lemma test sweeps 2 x 2047 sign/exponent classes squared). So `moved = -0.0` needs `current = -0.0` and
//! `step_length * f = -0.0`; `step_length` is `+0.0` or positive (tested), so `f < 0` and `step_length = +0.0`. Then
//! `|target - current| = 0 < 0.5` whichever zero `target` is: `AtEdge`, the inputs unchanged, same for both. The test runs the
//! whole set D of those states (current `-0.0` on the moved axis, negative dx + dy, a zero step length, every class of max,
//! the other axis) and compares with the specification.

use pr_app::input::primitives::RectD;
use pr_app::viewport::arrow_pan::{ArrowPanOutcome, step, step_length};
use pr_app::viewport::types::ViewportLayout;

const NZ: f64 = -0.0;

fn layout(vw: f64, vh: f64, max_h: f64, max_v: f64) -> ViewportLayout {
    ViewportLayout {
        viewport_width: vw,
        viewport_height: vh,
        extent_width: 0.0,
        extent_height: 0.0,
        horizontal_bar_visible: true,
        vertical_bar_visible: true,
        image_rect: RectD::default(),
        max_horizontal_offset: max_h,
        max_vertical_offset: max_v,
    }
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// Specification of the original `Step` (C# `Math.Clamp` branch order), written with `match` on a comparison result.
fn spec_step(dx: i32, dy: i32, h: f64, v: f64, l: &ViewportLayout, frac: f64) -> (ArrowPanOutcome, f64, f64) {
    let horizontal = dx != 0;
    let (current, max) = if horizontal {
        (h, l.max_horizontal_offset)
    } else {
        (v, l.max_vertical_offset)
    };
    if max <= 0.5 {
        return (ArrowPanOutcome::NotScrollable, h, v);
    }
    let shorter = if l.viewport_width.is_nan() || l.viewport_height.is_nan() {
        f64::NAN
    } else if l.viewport_width <= l.viewport_height {
        l.viewport_width
    } else {
        l.viewport_height
    };
    let raw = shorter * frac;
    let length = if raw.is_finite() && raw > 0.0 { raw } else { 0.0 };
    let moved = current + length * f64::from(dx.wrapping_add(dy));
    let target = match moved.partial_cmp(&0.0) {
        Some(std::cmp::Ordering::Less) => 0.0,
        _ => match moved.partial_cmp(&max) {
            Some(std::cmp::Ordering::Greater) => max,
            _ => moved,
        },
    };
    if (target - current).abs() < 0.5 {
        return (ArrowPanOutcome::AtEdge, h, v);
    }
    if horizontal {
        (ArrowPanOutcome::Panned, target, v)
    } else {
        (ArrowPanOutcome::Panned, h, target)
    }
}

fn check(dx: i32, dy: i32, h: f64, v: f64, l: &ViewportLayout, frac: f64) -> ArrowPanOutcome {
    let real = step(dx, dy, h, v, l, frac);
    let want = spec_step(dx, dy, h, v, l, frac);
    assert!(
        real.0 == want.0 && same(real.1, want.1) && same(real.2, want.2),
        "step({dx}, {dy}, {h:?}, {v:?}, vw {:?} vh {:?}, max {:?} {:?}, frac {frac:?}) = {real:?}, want {want:?}",
        l.viewport_width,
        l.viewport_height,
        l.max_horizontal_offset,
        l.max_vertical_offset
    );
    real.0
}

fn offset_pool() -> Vec<f64> {
    vec![
        f64::NAN,
        f64::NEG_INFINITY,
        -1e300,
        -100.0,
        -0.5,
        NZ,
        0.0,
        0.25,
        0.5,
        1.0,
        100.0,
        799.5,
        800.0,
        1e300,
        f64::MAX,
        f64::INFINITY,
    ]
}

fn max_pool() -> Vec<f64> {
    let next_up = |x: f64| f64::from_bits(x.to_bits() + 1);
    vec![
        f64::NAN,
        f64::NEG_INFINITY,
        -1.0,
        NZ,
        0.0,
        0.25,
        0.5,
        next_up(0.5),
        1.0,
        100.0,
        800.0,
        1e300,
        f64::MAX,
        f64::INFINITY,
    ]
}

/// (dx, dy) classes: the horizontal flag (`dx != 0`) and the wrapping sum (zero, positive, negative, extremes) both occur.
fn key_pool() -> Vec<(i32, i32)> {
    vec![
        (0, 0),
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (2, -3),
        (i32::MIN, 0),
        (i32::MAX, 1),
        (i32::MAX, 0),
        (0, i32::MIN),
        (-1, i32::MIN),
        (i32::MIN, i32::MIN),
    ]
}

#[test]
fn step_equals_the_specification_on_the_class_lattice() {
    let offsets = offset_pool();
    let maxes = max_pool();
    // (viewport width, viewport height, step fraction): a zero, a positive, a NaN, an infinite and a negative step length.
    let steps = [
        (0.0, 600.0, 0.1),
        (800.0, 600.0, 0.0),
        (800.0, 600.0, 0.1),
        (800.0, 600.0, 1.0),
        (f64::NAN, 600.0, 0.1),
        (800.0, 600.0, f64::NAN),
        (f64::INFINITY, f64::INFINITY, 1.0),
        (800.0, 600.0, -0.1),
        (NZ, NZ, 1.0),
        (5e-324, 5e-324, 0.5),
    ];
    let mut outcomes = [0u64; 3];
    for &(dx, dy) in &key_pool() {
        for &h in &offsets {
            for &v in &offsets {
                for &mh in &maxes {
                    for &mv in &maxes {
                        for &(vw, vh, frac) in &steps {
                            let o = check(dx, dy, h, v, &layout(vw, vh, mh, mv), frac);
                            outcomes[o as usize] += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(outcomes.iter().all(|&n| n > 1000), "{outcomes:?}");
}

#[test]
fn step_length_is_positive_zero_or_positive_never_negative_zero_or_nan() {
    let pool = [
        f64::NAN,
        f64::NEG_INFINITY,
        -2.0,
        NZ,
        0.0,
        5e-324,
        0.5,
        800.0,
        1e300,
        f64::MAX,
        f64::INFINITY,
    ];
    for &w in &pool {
        for &h in &pool {
            for &f in &pool {
                let l = step_length(w, h, f);
                assert!(
                    l == 0.0 && l.is_sign_positive() || l > 0.0 && l.is_finite(),
                    "{w:?} {h:?} {f:?} -> {l:?}"
                );
            }
        }
    }
}

#[test]
fn ieee_sum_is_negative_zero_only_for_two_negative_zeros_over_every_sign_exponent_class() {
    let mut classes = Vec::new();
    for sign in [0u64, 1] {
        for exponent in 0..2047u64 {
            classes.push(f64::from_bits((sign << 63) | (exponent << 52)));
            if exponent % 64 == 5 {
                classes.push(f64::from_bits((sign << 63) | (exponent << 52) | ((1 << 52) - 1)));
            }
        }
    }
    classes.push(f64::INFINITY);
    classes.push(f64::NEG_INFINITY);
    let mut negative_zero_sums = 0u64;
    for &a in &classes {
        for &b in &classes {
            let s = a + b;
            if s.to_bits() == NZ.to_bits() {
                assert!(
                    a.to_bits() == NZ.to_bits() && b.to_bits() == NZ.to_bits(),
                    "{a:?} + {b:?}"
                );
                negative_zero_sums += 1;
            }
        }
    }
    assert_eq!(negative_zero_sums, 1);
}

#[test]
fn step_with_current_negative_zero_and_a_zero_step_is_at_edge_with_inputs_unchanged_for_every_class_of_the_rest() {
    // The set D of 81:27: moved = -0.0 needs current = -0.0, step length +0.0, dx + dy < 0 (wrapping).
    let mut d_states = 0u64;
    for &(dx, dy) in &key_pool() {
        if dx.wrapping_add(dy) >= 0 {
            continue;
        }
        for &other in &offset_pool() {
            for &mh in &max_pool() {
                for &mv in &max_pool() {
                    for &(vw, vh, frac) in &[
                        (0.0, 600.0, 0.1),
                        (800.0, 600.0, 0.0),
                        (f64::NAN, 1.0, 1.0),
                        (NZ, 5.0, 2.0),
                    ] {
                        let l = layout(vw, vh, mh, mv);
                        assert_eq!(step_length(vw, vh, frac).to_bits(), 0.0f64.to_bits());
                        let (h, v) = if dx != 0 { (NZ, other) } else { (other, NZ) };
                        let o = check(dx, dy, h, v, &l, frac);
                        let max = if dx != 0 { mh } else { mv };
                        if max > 0.5 || max.is_nan() {
                            assert_eq!(o, ArrowPanOutcome::AtEdge);
                            let r = step(dx, dy, h, v, &l, frac);
                            assert_eq!((r.1.to_bits(), r.2.to_bits()), (h.to_bits(), v.to_bits()));
                            d_states += 1;
                        } else {
                            assert_eq!(o, ArrowPanOutcome::NotScrollable);
                        }
                    }
                }
            }
        }
    }
    assert!(d_states > 5000, "{d_states}");
}

#[test]
fn step_with_moved_exactly_equal_to_max_pans_to_that_value_for_every_exponent_class() {
    // The tie of 83:21: current = 0, one press forward by a step of exactly `max` (viewport = max, fraction 1), so moved == max.
    let mut ties = 0u64;
    for exponent in 1022u64..=2046 {
        for m in [0u64, 1, 1 << 51, (1 << 52) - 1] {
            let max = f64::from_bits((exponent << 52) | m);
            if max <= 0.5 {
                continue;
            }
            let l = layout(max, max, max, max);
            let r = step(1, 0, 0.0, 0.0, &l, 1.0);
            assert_eq!(r.0, ArrowPanOutcome::Panned, "max {max:e}");
            assert_eq!(r.1.to_bits(), max.to_bits(), "max {max:e}");
            assert_eq!(r.2.to_bits(), 0.0f64.to_bits());
            let r = step(0, 1, 0.0, 0.0, &l, 1.0);
            assert_eq!(
                (r.0, r.1.to_bits(), r.2.to_bits()),
                (ArrowPanOutcome::Panned, 0.0f64.to_bits(), max.to_bits())
            );
            ties += 1;
        }
    }
    assert!(ties > 3000, "{ties}");
    // Guard edge: max = 0.5 is NotScrollable, the next value up is scrollable.
    let up = f64::from_bits(0.5f64.to_bits() + 1);
    assert_eq!(
        step(1, 0, 0.0, 0.0, &layout(1.0, 1.0, 0.5, 0.5), 1.0).0,
        ArrowPanOutcome::NotScrollable
    );
    assert_eq!(
        step(1, 0, 0.0, 0.0, &layout(1.0, 1.0, up, up), 1.0).0,
        ArrowPanOutcome::Panned
    );
}
