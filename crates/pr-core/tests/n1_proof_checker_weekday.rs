//! N1 run-proof (mutation close-out 2026-11-05) for the `checker.rs:357:20` survivor `era * 146_097` -> `era / 146_097` in
//! `weekday_of` (M5: argued equivalent by reading only). `weekday_of` is private; it decides whether the weekday NAME of a
//! `Retry-After` HTTP-date matches the date, which `UpdateChecker::is_rate_limited` (403 + `Retry-After`) exposes as a
//! yes/no. The run covers the whole domain the parser can hand to `weekday_of`: every valid calendar date of the years
//! 1 to 9999 (the date formats take at most four year digits), 3 652 059 dates.
//!
//! For each date the header is built with the TRUE weekday (computed here by counting days from 0001-01-01, a Monday) and
//! with the next weekday as a WRONG name; the true one must be accepted and the wrong one rejected. Together that pins
//! `weekday_of(y, m, d)` to the true weekday for the whole domain. The test is meant to PASS with the original code and
//! with the hand-applied mutant, so it is a proof of equivalence, not a killer. The full run is `#[ignore]`d (slow in the
//! debug profile); a default run covers a spread sample of years and every date of years 1580 to 1620 and 1990 to 2030.

use pr_core::updates::checker::{HttpBody, HttpError, HttpResponse, UpdateChecker};

struct NoBody;

impl HttpBody for NoBody {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, HttpError> {
        Ok(0)
    }
}

fn accepted(value: String) -> bool {
    UpdateChecker::is_rate_limited(&HttpResponse {
        status: 403,
        headers: vec![("Retry-After".to_owned(), value)],
        content_length: Some(0),
        body: Box::new(NoBody),
    })
}

const NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn is_leap(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in(year: u32, month: usize) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

/// Walks every date from 0001-01-01 (a Monday) to 9999-12-31 with a day counter and calls `check(year, month0, day, weekday)`
/// for the years `want` selects. Returns the weekday of 9999-12-31 and the number of dates visited.
fn walk(want: impl Fn(u32) -> bool, mut check: impl FnMut(u32, usize, u32, usize)) -> (usize, u64) {
    let mut weekday = 1usize; // Monday
    let mut visited = 0u64;
    let mut last = 1usize;
    for year in 1..=9999u32 {
        for month in 0..12usize {
            for day in 1..=days_in(year, month + 1) {
                if want(year) {
                    check(year, month, day, weekday);
                    visited += 1;
                }
                last = weekday;
                weekday = (weekday + 1) % 7;
            }
        }
    }
    (last, visited)
}

fn header(weekday: usize, day: u32, month: usize, year: u32) -> String {
    format!("{}, {day:02} {} {year:04} 00:00:00 GMT", NAMES[weekday], MONTHS[month])
}

fn run(want: impl Fn(u32) -> bool) -> u64 {
    let (last_weekday, visited) = walk(want, |year, month, day, weekday| {
        assert!(
            accepted(header(weekday, day, month, year)),
            "true weekday rejected: {year}-{}-{day}",
            month + 1
        );
        assert!(
            !accepted(header((weekday + 1) % 7, day, month, year)),
            "wrong weekday accepted: {year}-{}-{day}",
            month + 1
        );
    });
    // The counting reference itself: 9999-12-31 is a Friday (proleptic Gregorian).
    assert_eq!(last_weekday, 5);
    visited
}

#[test]
fn the_counting_reference_knows_the_documented_weekdays() {
    let mut seen = 0;
    walk(
        |y| matches!(y, 1970 | 2000 | 2015),
        |year, month, day, weekday| {
            match (year, month, day) {
                (1970, 0, 1) => assert_eq!(weekday, 4, "1970-01-01 was a Thursday"),
                (2000, 1, 29) => assert_eq!(weekday, 2, "2000-02-29 was a Tuesday"),
                (2015, 9, 21) => assert_eq!(weekday, 3, "2015-10-21 was a Wednesday"),
                _ => return,
            }
            seen += 1;
        },
    );
    assert_eq!(seen, 3);
}

#[test]
fn weekday_names_match_for_a_spread_sample_of_years_and_two_dense_windows() {
    let visited = run(|y| y % 97 == 1 || (1580..=1620).contains(&y) || (1990..=2030).contains(&y) || y >= 9990);
    assert!(visited > 30_000, "visited {visited}");
}

#[test]
#[ignore = "slow: 3.65 million dates, two HTTP-date parses each (N1 run-proof of the weekday_of era * 146097 survivor)"]
fn weekday_names_match_for_every_valid_date_of_the_years_1_to_9999() {
    assert_eq!(run(|_| true), 3_652_059);
}
