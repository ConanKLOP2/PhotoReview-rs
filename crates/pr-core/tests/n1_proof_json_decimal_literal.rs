//! N1 run-proof (mutation close-out 2026-11-05) for the `settings/app_settings.rs:1487:9` survivor
//! `json::is_decimal_literal -> bool` replaced with `true`. `to_f64` of a string value only reaches `str::parse::<f64>` (then
//! `is_finite`) after the grammar check, so the replacement changes nothing exactly when every string that Rust parses to a
//! FINITE `f64` already matches the grammar `[+-]?(digits[.digits*]|.digits)([eE][+-]?digits)?`. This file runs that claim
//! through the public `to_f64`:
//!
//! * every string of length 1 to 6 over the 12 symbols `0 1 . e E + - space i n f N` (3 257 436 strings, covering `inf`,
//!   `nan`, `-`, `+`, `.`, `e`, bare exponents, signs and blanks in every position);
//! * a list of strings Rust's float parser is known to be liberal about or that look numeric (`infinity`, `NaN` in every case,
//!   `1_0`, `0x10`, `1e400`, full-width and Arabic-Indic digits, padded blanks, a lone `.`, `5.`, `.5e1`).
//!
//! The expected answer comes from an independent grammar matcher written below plus `parse::<f64>` for the value. The test is
//! meant to PASS with the original code and with the hand-applied mutant (proof of equivalence, not a killer).

use pr_core::settings::app_settings::json::{self, JsonValue};

/// Independent matcher for `[+-]?(digits[.digits*]|.digits)([eE][+-]?digits)?`.
fn grammar(s: &str) -> bool {
    let rest = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], Some(&rest[at + 1..])),
        None => (rest, None),
    };
    let all_digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((int, frac)) => all_digits(int) && all_digits(frac) && (!int.is_empty() || !frac.is_empty()),
        None => !mantissa.is_empty() && all_digits(mantissa),
    };
    let exponent_ok = match exponent {
        None => true,
        Some(e) => {
            let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
            !digits.is_empty() && all_digits(digits)
        }
    };
    mantissa_ok && exponent_ok
}

fn expected(s: &str) -> Option<f64> {
    match s {
        "NaN" => return Some(f64::NAN),
        "Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if !grammar(s) {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn same(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()),
        _ => false,
    }
}

fn check(s: &str) {
    let actual = json::to_f64(&JsonValue::String(Some(s.to_owned())));
    assert!(
        same(actual, expected(s)),
        "to_f64({s:?}) = {actual:?}, expected {:?}",
        expected(s)
    );
}

fn rec(alphabet: &[char], max_len: usize, current: &mut String, count: &mut usize) {
    if !current.is_empty() {
        check(current);
        *count += 1;
    }
    if current.chars().count() == max_len {
        return;
    }
    for &symbol in alphabet {
        current.push(symbol);
        rec(alphabet, max_len, current, count);
        current.pop();
    }
}

#[test]
fn every_short_string_over_the_numeric_alphabet_matches_the_grammar_oracle() {
    let alphabet = ['0', '1', '.', 'e', 'E', '+', '-', ' ', 'i', 'n', 'f', 'N'];
    let mut count = 0;
    rec(&alphabet, 6, &mut String::new(), &mut count);
    assert_eq!(count, 3_257_436);
}

#[test]
fn liberal_or_numeric_looking_strings_match_the_grammar_oracle() {
    for s in [
        "inf",
        "-inf",
        "+inf",
        "INF",
        "Inf",
        "infinity",
        "Infinity",
        "-Infinity",
        "+Infinity",
        "INFINITY",
        "nan",
        "NaN",
        "NAN",
        "+nan",
        "-NaN",
        "1_0",
        "0x10",
        "1e400",
        "-1e400",
        "1e-400",
        "\u{ff11}",
        "\u{661}",
        "1 ",
        " 1",
        "\u{a0}1",
        "1\t",
        ".",
        "5.",
        ".5",
        ".5e1",
        "5.e1",
        "+.5",
        "-.5e-1",
        "1e",
        "e1",
        "1e+",
        "0.0",
        "-0",
        "00012.50",
        "1.7976931348623157e308",
        "1.7976931348623159e308",
        "",
    ] {
        check(s);
    }
}
