//! N1 (mutation close-out 2026-11-05): the string-scanner cursor loops of `settings/app_settings.rs` `json::Parser::parse_string`
//! (`1291:34` and `1295:34` `+= 1` -> `-= 1`, `1339:34` `+= c.len_utf8()` -> `*= ...`) never advance, so cargo-mutants can
//! only report TIMEOUT when a test binary that is not bounded runs first. Each input below reaches one of them and is parsed
//! on its own worker thread, awaited with `recv_timeout` (pattern of `w3_stj_timeout_killers`): a stuck loop fails the test
//! with "did not finish" after a bounded wait. No sleep, no production hook.
use pr_core::settings::app_settings::json::{self, JsonValue};
use std::sync::mpsc;
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(10);

fn bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("stj-str-{what}"))
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

fn string_of(text: &'static str) -> Option<String> {
    match bounded(text, move || json::parse(text)).unwrap_or_else(|e| panic!("{text:?} must parse: {e:?}")) {
        JsonValue::String(s) => s,
        other => panic!("{text:?}: expected a string, got {other:?}"),
    }
}

// 1339:34: plain characters of one and of several bytes advance the cursor by their UTF-8 length.
#[test]
fn plain_characters_advance_the_cursor_by_their_utf8_length() {
    assert_eq!(string_of(r#""abc""#), Some("abc".to_owned()));
    assert_eq!(
        string_of("\"\u{e9}\u{20ac}\u{1f600}x\""),
        Some("\u{e9}\u{20ac}\u{1f600}x".to_owned())
    );
}

// 1291:34 (cursor past the backslash) and 1295:34 (cursor past the escape letter).
#[test]
fn simple_escapes_advance_past_the_backslash_and_the_escape_letter() {
    assert_eq!(string_of(r#""a\nb""#), Some("a\nb".to_owned()));
    assert_eq!(string_of(r#""\"\\\/""#), Some("\"\\/".to_owned()));
    assert_eq!(string_of(r#""A\t""#), Some("A\t".to_owned()));
    assert_eq!(string_of(r#""😀""#), Some("\u{1f600}".to_owned()));
}

#[test]
fn the_closing_quote_ends_the_scan() {
    assert_eq!(string_of(r#""""#), Some(String::new()));
    assert_eq!(string_of(r#""\ud800""#), None);
}
