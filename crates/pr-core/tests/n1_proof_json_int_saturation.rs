//! N1 run-proof (mutation close-out 2026-11-05) for the three `settings/app_settings.rs` survivors in `json::parse_int_digits`
//! (`1453:18` `>` -> `==` and `>` -> `>=`, `1454:42` `+` -> `-` in `v = u64::MAX + 1`). The clamp only changes WHICH huge
//! value an out-of-range integer is saturated to; the only readers of `parse_int_digits` are `to_i32` and `to_i64`, which
//! accept nothing above `i64::MAX` (or below `i64::MIN`), and every clamp value (`u64::MAX - 1`, `u64::MAX`, `2^64`,
//! the unclamped `i128::MAX`) is far outside that range. This file runs the claim over a finite, boundary-heavy domain and is
//! meant to PASS with the original code and with each hand-applied mutant (proof of equivalence, not a killer):
//!
//! * every digit string of length 1 to 8 over `{0, 1, 8, 9}` (87 380 strings) with every sign form the two token kinds allow;
//! * for every length 1 to 60: `9..9`, `10..0`, and the boundary values `i32`/`i64`/`u64`/`i128` `MIN`/`MAX` and their
//!   neighbours (+-3), each also with 0 to 3 leading zeros.
//!
//! The expected value comes from an independent oracle on the digit string (leading zeros stripped, more than 19 significant
//! digits is out of range, otherwise `u128` arithmetic), not from `parse_int_digits`.

use pr_core::settings::app_settings::json::{self, JsonValue};

fn oracle(sign_negative: bool, digits: &str) -> Option<i64> {
    let significant = digits.trim_start_matches('0');
    if significant.len() > 19 {
        return None;
    }
    let magnitude: u128 = if significant.is_empty() {
        0
    } else {
        significant.parse().ok()?
    };
    let signed = if sign_negative {
        -(magnitude as i128)
    } else {
        magnitude as i128
    };
    i64::try_from(signed).ok()
}

fn oracle32(sign_negative: bool, digits: &str) -> Option<i32> {
    oracle(sign_negative, digits).and_then(|v| i32::try_from(v).ok())
}

/// Checks one digit string in every token form: a `Number` token (`digits`, `-digits`) and a `String` value (`digits`,
/// `+digits`, `-digits`).
fn check(digits: &str) {
    for (prefix, negative, as_number) in [
        ("", false, true),
        ("-", true, true),
        ("", false, false),
        ("+", false, false),
        ("-", true, false),
    ] {
        let token = format!("{prefix}{digits}");
        let value = if as_number {
            JsonValue::Number(token.clone())
        } else {
            JsonValue::String(Some(token.clone()))
        };
        assert_eq!(
            json::to_i64(&value),
            oracle(negative, digits),
            "to_i64 {} {token:?}",
            if as_number { "number" } else { "string" }
        );
        assert_eq!(
            json::to_i32(&value),
            oracle32(negative, digits),
            "to_i32 {} {token:?}",
            if as_number { "number" } else { "string" }
        );
    }
}

fn rec(alphabet: &[u8], max_len: usize, current: &mut Vec<u8>, count: &mut usize) {
    if !current.is_empty() {
        check(std::str::from_utf8(current).expect("ascii"));
        *count += 1;
    }
    if current.len() == max_len {
        return;
    }
    for &symbol in alphabet {
        current.push(symbol);
        rec(alphabet, max_len, current, count);
        current.pop();
    }
}

#[test]
fn every_short_digit_string_over_0_1_8_9_matches_the_oracle() {
    let mut count = 0;
    rec(b"0189", 8, &mut Vec::new(), &mut count);
    assert_eq!(count, 87_380);
}

#[test]
fn long_and_boundary_digit_strings_match_the_oracle_and_saturate_out_of_range() {
    let boundaries: Vec<i128> = [
        i128::from(i32::MIN),
        i128::from(i32::MAX),
        i128::from(i64::MIN),
        i128::from(i64::MAX),
        i128::from(u64::MAX),
        i128::from(u64::MAX) + 1,
        i128::MAX,
        10_i128.pow(38),
    ]
    .into_iter()
    .flat_map(|b| (-3..=3).map(move |d| b.saturating_add(d)))
    .collect();
    let mut checked = 0;
    for n in 1..=60usize {
        let mut shapes = vec!["9".repeat(n), format!("1{}", "0".repeat(n - 1))];
        for b in &boundaries {
            shapes.push(b.unsigned_abs().to_string());
        }
        for shape in shapes {
            for zeros in 0..=3 {
                check(&format!("{}{shape}", "0".repeat(zeros)));
                checked += 1;
            }
        }
    }
    assert!(checked > 10_000, "checked {checked}");
    // The clamp region itself, spelled out: just below, at and above u64::MAX and 2^64.
    for digits in [
        "18446744073709551614",
        "18446744073709551615",
        "18446744073709551616",
        "18446744073709551617",
        "170141183460469231731687303715884105727",
        "170141183460469231731687303715884105728",
        "999999999999999999999999999999999999999999",
    ] {
        check(digits);
        assert_eq!(json::to_i64(&JsonValue::Number(digits.to_owned())), None, "{digits}");
        assert_eq!(
            json::to_i64(&JsonValue::Number(format!("-{digits}"))),
            None,
            "-{digits}"
        );
    }
}
