//! N1 run-proof (mutation close-out 2026-11-05) for the `settings/app_settings.rs:1687:34` survivor `exponent < 0` ->
//! `exponent <= 0` in `json::format_f64` (the sign character of the exponential form). The exponential form is only used when
//! the decimal exponent is `<= -5` or `>= 17`, so `exponent == 0` never reaches that line and `<` and `<=` pick the same
//! sign. This file runs the claim over every decimal exponent a finite `f64` has (-324 to 308), with several mantissas, both
//! signs, plus the exact boundaries of the plain/exponential switch, plus 400 000 pseudo-random bit patterns, against an
//! independent oracle built from Rust's own `{:e}` / `{}` formatting:
//!
//! * plain form (exponent -4 to 16): `format!("{v}")` (Rust's `Display` never uses an exponent), except that a whole number
//!   keeps no `.0`, which `Display` also omits;
//! * exponential form: `d[.ddd]E` + sign + at least two exponent digits, taken from `{:e}`.
//!
//! Where `.NET` rounds an exact decimal tie to even and Rust's shortest form rounds it up, the last digit can differ by one;
//! the oracle accepts exactly that and nothing else, and compares the form, the point position, the `E` and the exponent
//! part strictly (the exponent sign is what the mutant would break).
//! The test is meant to PASS with the original code and with the hand-applied mutant (proof of equivalence, not a killer).

use pr_core::settings::app_settings::json;

fn oracle(v: f64) -> String {
    let sci = format!("{v:e}");
    let (mantissa, exp) = sci.split_once('e').expect("exponent");
    let exponent: i32 = exp.parse().expect("exponent number");
    if exponent > -5 && exponent < 17 {
        return format!("{v}");
    }
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}E{sign}{:02}", exponent.abs())
}

/// `.NET` rounds an exact decimal tie to even, Rust's shortest form rounds it up: the two renderings can then differ by one
/// unit in the last digit. Everything else (sign, decimal point position, length, `E`, exponent sign and digits) must be equal.
fn same_up_to_a_last_digit_tie(actual: &str, expected: &str) -> bool {
    let skeleton = |t: &str| {
        t.chars()
            .map(|c| if c.is_ascii_digit() { 'd' } else { c })
            .collect::<String>()
    };
    let (a_main, a_tail) = actual.split_once('E').unwrap_or((actual, ""));
    let (e_main, e_tail) = expected.split_once('E').unwrap_or((expected, ""));
    if a_tail != e_tail || skeleton(a_main) != skeleton(e_main) {
        return false;
    }
    let number = |t: &str| {
        t.chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u128>()
            .unwrap_or(0)
    };
    number(a_main).abs_diff(number(e_main)) <= 1
}

fn check(v: f64) {
    let actual = json::format_f64(v).expect("finite");
    let expected = oracle(v);
    assert!(
        same_up_to_a_last_digit_tie(&actual, &expected),
        "format_f64({v:e}) = {actual} but the oracle says {expected} (bits {:#018x})",
        v.to_bits()
    );
    // The exponent part, strictly: it is what `exponent < 0` decides.
    if let Some((_, tail)) = actual.split_once('E') {
        assert!(tail.starts_with('-') || tail.starts_with('+'), "{actual}");
        assert!(tail.len() >= 3, "{actual}");
    }
}

#[test]
fn every_decimal_exponent_with_several_mantissas_matches_the_oracle() {
    let mut checked = 0;
    for exponent in -324..=308 {
        for mantissa in ["1", "1.5", "9.999", "2.2250738585072014", "1.2345678901234567", "5"] {
            let Ok(v) = format!("{mantissa}e{exponent}").parse::<f64>() else {
                continue;
            };
            if !v.is_finite() {
                continue;
            }
            check(v);
            check(-v);
            checked += 2;
        }
    }
    assert!(checked > 6_000, "checked {checked}");
}

#[test]
fn the_plain_to_exponential_boundaries_and_zero_match_the_oracle() {
    for v in [
        0.0,
        1.0,
        9.999e-5,
        1e-5,
        1.0000001e-5,
        9.9999e-5,
        1e-4,
        0.00012,
        999_999_999_999_999.9,
        9_999_999_999_999_998.0,
        1e16,
        1.2345e16,
        9.999_999_999_999_998e16,
        1e17,
        1.5e17,
        f64::MIN_POSITIVE,
        f64::MAX,
        5e-324,
    ] {
        check(v);
        check(-v);
    }
    assert_eq!(json::format_f64(-0.0).as_deref(), Some("-0"));
    assert_eq!(json::format_f64(0.0).as_deref(), Some("0"));
}

#[test]
fn four_hundred_thousand_pseudo_random_bit_patterns_match_the_oracle() {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut checked = 0;
    while checked < 400_000 {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let bits = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let v = f64::from_bits(bits);
        if v.is_finite() && v != 0.0 {
            check(v);
            checked += 1;
        }
    }
}
