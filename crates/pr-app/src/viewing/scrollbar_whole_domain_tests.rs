//! P-394: whole-domain test of the private `cs_min` of `scrollbar_geometry.rs`.
//!
//! `cs_min` uses only `is_nan`, comparisons and a sign test, and returns one of its arguments, so its result depends on the arguments only through
//! their order type (NaN, tie, order, sign of each zero). The 9 pool values below (NaN, -inf, -2, -1, -0, +0, 1, 2, +inf) give all 81 order types of
//! two f64 values: this enumeration is the whole domain. Results are compared bit for bit.

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

/// C# `Math.Min` written from the .NET order (`-0` below `+0`), not from the operators of the port.
fn reference_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let order = match a.partial_cmp(&b).expect("no NaN") {
        Ordering::Equal => (a.is_sign_positive() as u8).cmp(&(b.is_sign_positive() as u8)),
        other => other,
    };
    if order == Ordering::Greater { b } else { a }
}

#[test]
fn cs_min_equals_the_net_contract_on_every_order_type_of_two_values() {
    let mut n = 0;
    for &a in &POOL {
        for &b in &POOL {
            let got = cs_min(a, b);
            let want = reference_min(a, b);
            assert!(same_bits(got, want), "cs_min({a:?}, {b:?}) = {got:?}, want {want:?}");
            n += 1;
        }
    }
    assert_eq!(n, 81);
}

#[test]
fn cs_min_pins_the_signed_zero_ties_and_the_negative_pair() {
    assert_eq!(cs_min(0.0, -0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(cs_min(-0.0, 0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(cs_min(0.0, 0.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(cs_min(-1.0, -2.0), -2.0);
    assert_eq!(cs_min(-2.0, -1.0), -2.0);
    assert!(cs_min(f64::NAN, 1.0).is_nan() && cs_min(1.0, f64::NAN).is_nan());
}
