//! P-394: whole-domain tests of the private `net_max`, `net_min` and `arrange_length` of `layout.rs`.
//!
//! These functions use only comparisons, `is_nan`, sign tests and the selection of one argument (no arithmetic), so each result depends on the
//! arguments only through their ORDER TYPE: NaN, ties, order, and the sign of each zero. The pool below holds one representative of every class
//! (NaN, -inf, two negatives, -0, +0, two positives, +inf); the tuples of 2 resp. 3 pool values (81 resp. 729) enumerate every order type of two
//! resp. three f64 values, hence the whole domain of the functions. Results are compared bit for bit (a `-0.0` vs `+0.0` difference is a difference).

use super::*;
use std::cmp::Ordering;

const POOL: [f64; 9] = [
    f64::NAN,
    f64::NEG_INFINITY,
    -2.0,
    -1.0,
    -0.0,
    0.0,
    1.0,
    2.0,
    f64::INFINITY,
];

fn same_bits(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// .NET total order of two non-NaN values: `-0` is below `+0`.
fn net_order(a: f64, b: f64) -> Ordering {
    match a.partial_cmp(&b).expect("no NaN") {
        Ordering::Equal => (a.is_sign_positive() as u8).cmp(&(b.is_sign_positive() as u8)),
        other => other,
    }
}

fn reference_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    match net_order(a, b) {
        Ordering::Less => b,
        _ => a,
    }
}

fn reference_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    match net_order(a, b) {
        Ordering::Greater => b,
        _ => a,
    }
}

#[test]
fn net_max_equals_the_net_contract_on_every_order_type_of_two_values() {
    let mut n = 0;
    for &a in &POOL {
        for &b in &POOL {
            let got = net_max(a, b);
            let want = reference_max(a, b);
            assert!(same_bits(got, want), "net_max({a:?}, {b:?}) = {got:?}, want {want:?}");
            n += 1;
        }
    }
    assert_eq!(n, 81);
}

#[test]
fn net_min_equals_the_net_contract_on_every_order_type_of_two_values() {
    let mut n = 0;
    for &a in &POOL {
        for &b in &POOL {
            let got = net_min(a, b);
            let want = reference_min(a, b);
            assert!(same_bits(got, want), "net_min({a:?}, {b:?}) = {got:?}, want {want:?}");
            n += 1;
        }
    }
    assert_eq!(n, 81);
}

#[test]
fn net_max_and_net_min_pin_the_signed_zero_and_nan_cases() {
    assert_eq!(net_max(-0.0, 0.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(net_max(0.0, -0.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(net_min(-0.0, 0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(net_min(0.0, -0.0).to_bits(), (-0.0f64).to_bits());
    assert!(net_max(f64::NAN, 1.0).is_nan() && net_max(1.0, f64::NAN).is_nan());
    assert!(net_min(f64::NAN, 1.0).is_nan() && net_min(1.0, f64::NAN).is_nan());
}

/// C# `ArrangeLength` written with `partial_cmp` instead of the operators of the port.
fn reference_arrange_length(slot: f64, unclipped: f64, max: f64) -> f64 {
    let mut length = slot;
    if length.partial_cmp(&unclipped) == Some(Ordering::Less) {
        length = unclipped;
    }
    let effective_max = reference_max(unclipped, max);
    if effective_max.partial_cmp(&length) == Some(Ordering::Less) {
        length = effective_max;
    }
    length
}

#[test]
fn arrange_length_equals_the_reference_on_every_order_type_of_three_values() {
    let mut n = 0;
    for &slot in &POOL {
        for &unclipped in &POOL {
            for &max in &POOL {
                let got = arrange_length(slot, unclipped, max);
                let want = reference_arrange_length(slot, unclipped, max);
                assert!(
                    same_bits(got, want),
                    "arrange_length({slot:?}, {unclipped:?}, {max:?}) = {got:?}, want {want:?}"
                );
                n += 1;
            }
        }
    }
    assert_eq!(n, 729);
}

#[test]
fn arrange_length_keeps_the_slot_zero_when_unclipped_is_the_other_zero() {
    // 349:15 `<` -> `<=`: slot +0, unclipped -0 compare equal; the slot must be kept (bit pattern +0).
    assert_eq!(arrange_length(0.0, -0.0, 5.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(arrange_length(-0.0, 0.0, 5.0).to_bits(), (-0.0f64).to_bits());
}

#[test]
fn arrange_length_keeps_the_length_when_it_equals_the_effective_max() {
    // 353:22 `<` -> `<=`: slot -0, unclipped -0, max +0: the effective max is +0 and compares equal to the length -0; the length must stay -0.
    assert_eq!(arrange_length(-0.0, -0.0, 0.0).to_bits(), (-0.0f64).to_bits());
}
