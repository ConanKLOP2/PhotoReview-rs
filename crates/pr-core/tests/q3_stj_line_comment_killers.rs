//! X-stj-reader (Q3 2026-11-04): the `skip_ws` line-comment loop (`app_settings.rs:1166:47`, `self.pos += c.len_utf8()`) was the one cursor loop
//! that `w3_stj_timeout_killers` did not reach: its mutants (`+=` -> `*=` / `-=`) left the cursor on the comment's first character, so the
//! parse never ended (cargo-mutants TIMEOUT in the combined run, MISSED when only the timeout killers ran). The parse runs on a worker
//! thread and is awaited with `recv_timeout`; a stuck loop fails the test instead of hanging the run. No sleep, no production hook.
use pr_core::settings::app_settings::json::{self, JsonValue};
use std::sync::mpsc;
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(10);

fn bounded(text: &'static str) -> Result<JsonValue, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("stj-line-comment".to_owned())
        .spawn(move || {
            let _ = tx.send(json::parse(text).map_err(|e| format!("{e:?}")));
        })
        .expect("spawn worker");
    match rx.recv_timeout(GUARD) {
        Ok(value) => value,
        Err(error) => panic!(
            "{text:?}: parser did not finish within {GUARD:?} ({error}); the line-comment loop no longer advances"
        ),
    }
}

#[test]
fn line_comment_body_is_scanned_to_the_line_end() {
    assert_eq!(bounded("// x\ntrue"), Ok(JsonValue::Bool(true)));
    assert_eq!(bounded("//xyz\r\nfalse"), Ok(JsonValue::Bool(false)));
}

#[test]
fn line_comment_with_multibyte_characters_advances_by_whole_characters() {
    // 2-byte, 3-byte and 4-byte characters: the cursor must move by each character's UTF-8 length (a fixed +1 would split a character).
    assert_eq!(bounded("// \u{e9}\u{20ac}\u{1f600}\ntrue"), Ok(JsonValue::Bool(true)));
}

#[test]
fn line_comment_at_the_end_of_the_text_ends_the_value_with_an_error_not_a_hang() {
    assert!(bounded("// only a comment").is_err());
}

#[test]
fn line_comment_containing_a_line_separator_is_refused() {
    assert!(bounded("// a\u{2028}b\ntrue").is_err());
    assert!(bounded("// a\u{2029}b\ntrue").is_err());
}
