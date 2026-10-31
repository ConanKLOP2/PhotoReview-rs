//! X-stj-reader: the eight `+= 1` -> `*= 1` cursor-loop mutants of `settings/app_settings.rs` (CX-SURV 2026-10-30) used to end as
//! 90 s cargo-mutants timeouts, because the mutated loop never advances. Each input below reaches exactly one of those loops. It is
//! parsed on its own worker thread and the result is awaited on a channel with `recv_timeout`: a stuck loop fails the test with
//! "did not finish" after a bounded wait instead of hanging the run. No sleep, no production hook (pattern: FIX-WORKER-PANICS,
//! w3_jrnl_killers8). The guard is generous (a correct parse takes microseconds), so it cannot fire on a loaded runner.
//! A stuck worker thread is leaked; it dies with the test process.
use pr_core::settings::app_settings::json::{self, JsonValue};
use std::sync::mpsc;
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(10);

/// Runs `work` on a fresh thread; panics (fails the test) when it does not finish within [`GUARD`].
fn bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("stj-{what}"))
        .spawn(move || {
            let _ = tx.send(work());
        })
        .expect("spawn worker");
    match rx.recv_timeout(GUARD) {
        Ok(value) => value,
        Err(error) => {
            panic!("{what}: parser did not finish within {GUARD:?} ({error}); a cursor loop no longer advances")
        }
    }
}

fn parse(text: &'static str) -> JsonValue {
    bounded(text, move || json::parse(text)).unwrap_or_else(|e| panic!("{text:?} must parse: {e:?}"))
}

fn string_to_f64(text: &'static str) -> Option<f64> {
    bounded(text, move || {
        json::to_f64(&json::parse(&format!("\"{text}\"")).expect("string literal"))
    })
}

// app_settings.rs:1146:26 skip_plain_ws `+= 1` -> `*= 1` (leading blanks: the cursor stays on the first blank).
#[test]
fn leading_whitespace_is_skipped_to_the_value() {
    assert_eq!(parse(" true"), JsonValue::Bool(true));
    assert_eq!(parse(" \t\r\n true"), JsonValue::Bool(true));
}

// app_settings.rs:1179:53 skip_ws block-comment body `+= 1` -> `*= 1` (the cursor stays on the first body byte).
#[test]
fn block_comment_body_is_scanned_to_its_end() {
    assert_eq!(parse("/*x*/true"), JsonValue::Bool(true));
    assert_eq!(parse("/*xyz*/true"), JsonValue::Bool(true));
}

// app_settings.rs:1228:34 (integer digits), :1239:30 (fraction digits), :1251:30 (exponent digits) in parse_number.
#[test]
fn number_digit_runs_are_consumed_to_the_delimiter() {
    assert_eq!(parse("1"), JsonValue::Number("1".into()));
    assert_eq!(parse("123"), JsonValue::Number("123".into()));
    assert_eq!(parse("0.1"), JsonValue::Number("0.1".into()));
    assert_eq!(parse("0.125"), JsonValue::Number("0.125".into()));
    assert_eq!(parse("0e1"), JsonValue::Number("0e1".into()));
    assert_eq!(parse("1E+25"), JsonValue::Number("1E+25".into()));
}

// app_settings.rs:1494:15 (integer digits), :1501:19 (fraction digits), :1516:19 (exponent digits) in is_decimal_literal, reached
// through `to_f64` of a string value.
#[test]
fn decimal_string_digit_runs_are_consumed_to_the_end() {
    assert_eq!(string_to_f64("1"), Some(1.0));
    assert_eq!(string_to_f64("12"), Some(12.0));
    assert_eq!(string_to_f64(".1"), Some(0.1));
    assert_eq!(string_to_f64("1.25"), Some(1.25));
    assert_eq!(string_to_f64("0e1"), Some(0.0));
    assert_eq!(string_to_f64("1e2"), Some(100.0));
}
