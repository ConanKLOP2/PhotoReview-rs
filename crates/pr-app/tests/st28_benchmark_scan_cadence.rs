//! ST-LOOP28 (X-benchmark-presenter): the cancel-poll cadence of `presenters/benchmark.rs` `enumerate_scan_entries` (identities 832:14 `seen += 1` -> `*= 1`, 833:59 `== 0` -> `!= 0`,
//! 833:17 `%` -> `/` and `%` -> `+`, formerly run-supported open). The entry source is an iterator that cancels the token at a chosen file and fails at a chosen entry, so the poll is
//! observable: the scan answers `Canceled` only when a poll happened between the two. No file system, no sleep.
use std::io;
use std::path::PathBuf;

use pr_app::presenters::benchmark::{
    BenchmarkScanError, ENUMERATION_CANCELLATION_CHECK_INTERVAL, ScanEntry, enumerate_scan_entries,
};
use pr_core::sync::CancelToken;

const INTERVAL: usize = ENUMERATION_CANCELLATION_CHECK_INTERVAL;

fn file(n: usize) -> ScanEntry {
    ScanEntry {
        is_dir: false,
        path: PathBuf::from(format!("p{n}.jpg")),
    }
}

/// Files 1..=`files` (a directory entry before every file when `dirs`); the token is canceled while file `cancel_at` is produced; entry number `fail_at` (counted in files) is an `Err`.
fn source(
    token: CancelToken,
    files: usize,
    cancel_at: usize,
    fail_at: Option<usize>,
    dirs: bool,
) -> impl Iterator<Item = io::Result<ScanEntry>> {
    let mut next = 1usize;
    let mut dir_turn = dirs;
    std::iter::from_fn(move || {
        if next > files {
            return None;
        }
        if dir_turn {
            dir_turn = false;
            return Some(Ok(ScanEntry {
                is_dir: true,
                path: PathBuf::from(format!("d{next}")),
            }));
        }
        dir_turn = dirs;
        let n = next;
        next += 1;
        if Some(n) == fail_at {
            return Some(Err(io::Error::other("entry failed")));
        }
        if n == cancel_at {
            token.cancel();
        }
        Some(Ok(file(n)))
    })
}

fn run(files: usize, cancel_at: usize, fail_at: Option<usize>, dirs: bool) -> Result<Vec<String>, BenchmarkScanError> {
    let token = CancelToken::new();
    enumerate_scan_entries(source(token.clone(), files, cancel_at, fail_at, dirs), 0, &token)
}

fn io_failure() -> Result<Vec<String>, BenchmarkScanError> {
    Err(BenchmarkScanError::Io("entry failed".into()))
}

// Cancel while file INTERVAL is produced, the next entry fails: the poll at `seen == INTERVAL` sees the cancel first. Kills `+= -> *=` (seen stays 0, polls every entry is the
// other way round, see the next test), `== -> !=` and `% -> /` / `% -> +` (never poll at INTERVAL).
#[test]
fn the_token_is_polled_at_every_interval_multiple() {
    assert_eq!(
        run(INTERVAL + 5, INTERVAL, Some(INTERVAL + 1), false),
        Err(BenchmarkScanError::Canceled)
    );
    assert_eq!(
        run(3 * INTERVAL, 2 * INTERVAL, Some(2 * INTERVAL + 1), false),
        Err(BenchmarkScanError::Canceled)
    );
}

// Cancel one file before the multiple, the entry at the multiple fails: no poll happened in between, so the I/O error wins. Kills every mutant that polls too often
// (`== -> !=`, `+= -> *=` makes `seen` stay 0 so it polls at every entry, `% -> /` polls for every `seen < INTERVAL`).
#[test]
fn the_token_is_not_polled_between_interval_multiples() {
    assert_eq!(run(INTERVAL + 5, INTERVAL - 1, Some(INTERVAL), false), io_failure());
    assert_eq!(run(INTERVAL + 5, 1, Some(INTERVAL), false), io_failure());
    assert_eq!(run(3 * INTERVAL, INTERVAL + 1, Some(2 * INTERVAL), false), io_failure());
}

// A directory is not counted: with a directory before every file the poll still happens at the INTERVAL-th FILE.
#[test]
fn directories_do_not_count_towards_the_polling_interval() {
    assert_eq!(
        run(INTERVAL + 5, INTERVAL, Some(INTERVAL + 1), true),
        Err(BenchmarkScanError::Canceled)
    );
    assert_eq!(run(INTERVAL + 5, INTERVAL - 1, Some(INTERVAL), true), io_failure());
}

// No cancel and no failure: every supported file comes back in order, the limit applies, and the final poll sees a cancel that came after the last multiple.
#[test]
fn an_uncanceled_scan_lists_the_files_and_a_late_cancel_is_caught_by_the_final_poll() {
    let all = run(INTERVAL + 5, usize::MAX, None, false).expect("not canceled");
    assert_eq!(all.len(), INTERVAL + 5);
    assert_eq!(all[0], "p1.jpg");
    assert_eq!(
        run(INTERVAL + 5, INTERVAL + 2, None, false),
        Err(BenchmarkScanError::Canceled)
    );
    let token = CancelToken::new();
    let limited = enumerate_scan_entries(source(token.clone(), 10, usize::MAX, None, false), 3, &token).expect("ok");
    assert_eq!(limited.len(), 3);
}
