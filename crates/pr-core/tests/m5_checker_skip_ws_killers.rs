//! M5 killer (mutation close-out 2026-11-04) for `checker.rs::skip_ws` (`+=` -> `*=` at 437:11). With `p *= 1` the cursor
//! never leaves the first whitespace character, so the HTTP-date match loops forever and cargo-mutants can only report a
//! TIMEOUT (never a kill). The check runs on a worker thread and is awaited with `recv_timeout`: a stuck loop fails the test
//! instead of hanging the run. No sleep, no production hook.
//!
//! `Retry-After` is trimmed of SP/HT before the date match, so the whitespace that reaches `skip_ws` is the one BETWEEN the
//! tokens of the date (C# `HttpDateParser` accepts runs of white space between the parts, `DateTime.TryParseExact` with
//! `AllowWhiteSpaces`).
use pr_core::updates::checker::{HttpBody, HttpError, HttpResponse, UpdateChecker};
use std::sync::mpsc;
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(10);

struct NoBody;

impl HttpBody for NoBody {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, HttpError> {
        Ok(0)
    }
}

fn rate_limited_with_retry_after(value: &'static str) -> bool {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("checker-skip-ws".to_owned())
        .spawn(move || {
            let response = HttpResponse {
                status: 403,
                headers: vec![("Retry-After".to_owned(), value.to_owned())],
                content_length: Some(0),
                body: Box::new(NoBody),
            };
            let _ = tx.send(UpdateChecker::is_rate_limited(&response));
        })
        .expect("spawn worker");
    match rx.recv_timeout(GUARD) {
        Ok(answer) => answer,
        Err(error) => {
            panic!("{value:?}: the date match did not finish within {GUARD:?} ({error}); skip_ws no longer advances")
        }
    }
}

#[test]
fn retry_after_date_with_runs_of_white_space_between_the_parts_is_accepted() {
    // 2015-10-21 was a Wednesday.
    assert!(rate_limited_with_retry_after("Wed,  21 Oct 2015 07:28:00 GMT"));
    assert!(rate_limited_with_retry_after("Wed, 21   Oct 2015 07:28:00 GMT"));
    assert!(rate_limited_with_retry_after("Wed, 21 Oct 2015 07:28:00   GMT"));
}

#[test]
fn retry_after_date_with_trailing_white_space_inside_the_value_is_judged_not_hung() {
    // A tab after the last token is trimmed, a form feed is not: the trailing skip must reach the end of the text.
    assert!(rate_limited_with_retry_after("Wed, 21 Oct 2015 07:28:00 GMT\t"));
    assert!(rate_limited_with_retry_after("Wed, 21 Oct 2015 07:28:00 GMT\u{c}"));
}
