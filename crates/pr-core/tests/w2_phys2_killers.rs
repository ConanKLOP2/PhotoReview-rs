//! Focused physical-filesystem mutation guards. Fakes only; no sleeps or user data.
//! Port of PhotoReview.Core/IO/PhysicalFileSystem.cs ReplaceWithRetry contract.

use std::cell::Cell;
use std::io;

use pr_core::io::physical::{REPLACE_ATTEMPTS, replace_with_retry};

// C#: ReplaceWithRetry transient-failure contract.
// physical.rs:113 `<` -> `<=`, guard -> true / `&&` -> `||`, :116 `+=` -> `*=` / `-=`.
// Bound the fake itself: a broken retry loop fails an assertion, never a timeout.
#[test]
fn replace_with_retry_transient_failure_stops_after_eight_attempts() {
    for code in [5, 32, 33] {
        let calls = Cell::new(0);
        let sleeps = Cell::new(0);
        let error = replace_with_retry(
            || {
                calls.set(calls.get() + 1);
                assert!(calls.get() <= REPLACE_ATTEMPTS, "retry exceeded its attempt budget");
                Err(io::Error::from_raw_os_error(code))
            },
            |delay| {
                sleeps.set(sleeps.get() + 1);
                assert!(sleeps.get() < REPLACE_ATTEMPTS, "slept after the final attempt");
                assert_eq!(delay, sleeps.get() * 3, "retry delay must advance exactly once");
            },
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(code));
        assert_eq!(calls.get(), REPLACE_ATTEMPTS);
        assert_eq!(sleeps.get(), REPLACE_ATTEMPTS - 1);
    }
}

// C#: ReplaceWithRetry nontransient errors propagate immediately; success ends retries.
// physical.rs:109 body -> Ok(()), :113 guard -> false / `<` -> `==` / `>`, `*` -> `+` / `/`.
#[test]
fn replace_with_retry_success_and_nontransient_error_finish_without_extra_calls() {
    let calls = Cell::new(0);
    let sleeps = Cell::new(0);
    replace_with_retry(
        || {
            calls.set(calls.get() + 1);
            assert!(calls.get() <= 3);
            if calls.get() < 3 {
                Err(io::Error::from_raw_os_error(32))
            } else {
                Ok(())
            }
        },
        |delay| {
            sleeps.set(sleeps.get() + 1);
            assert_eq!(delay, sleeps.get() * 3);
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 3);
    assert_eq!(sleeps.get(), 2);

    for code in [2, 3, 87, 1314] {
        let calls = Cell::new(0);
        let error = replace_with_retry(
            || {
                calls.set(calls.get() + 1);
                assert_eq!(calls.get(), 1);
                Err(io::Error::from_raw_os_error(code))
            },
            |_| panic!("nontransient error must not sleep"),
        )
        .unwrap_err();
        assert_eq!(calls.get(), 1);
        assert_eq!(error.raw_os_error(), Some(code));
    }
}
