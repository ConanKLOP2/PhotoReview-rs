//! N1 run-proof (mutation close-out 2026-11-05) for the survivor `transaction.rs:315:9` `impl Drop for JournalTransaction`
//! `drop` -> `()` (M5: argued equivalent by reading only). This file turns the argument into a run: it enumerates every
//! lifecycle state a `JournalTransaction` can be dropped in (never begun, begun, outcome written by commit / fail /
//! dismiss, begun twice, dropped while a panic unwinds) and asserts the observable contract of `drop`: by the time the
//! owner's `drop(tx)` statement has returned, the live marker has been released, and it was released exactly once. The
//! recording registry also logs the ORDER (marker released, then the end of `drop(tx)`).
//!
//! The file is meant to pass both with the real `drop` and with the empty `drop` (hand-applied in the N1 evidence file):
//! the compiler drops the `live_marker` field when the struct goes away, so an empty `drop` is indistinguishable at the
//! boundary of `drop(tx)`. It is therefore a proof of equivalence, not a killer. Nothing touches a real disk.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::abstractions::{LiveMarker, LiveOperationRegistry};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::file_actions::transaction::JournalTransaction;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, FakeFileSystem};

#[derive(Default)]
struct Log {
    events: Mutex<Vec<String>>,
}

impl Log {
    fn push(&self, event: &str) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.to_owned());
    }
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

struct Registry {
    log: Arc<Log>,
}

struct Marker {
    log: Arc<Log>,
}

impl Drop for Marker {
    fn drop(&mut self) {
        self.log.push("marker-released");
    }
}

impl LiveOperationRegistry for Registry {
    fn begin(&self, _operation_id: &str) -> LiveMarker {
        self.log.push("marker-created");
        Box::new(Marker { log: self.log.clone() })
    }
    fn is_live(&self, _operation_id: &str) -> bool {
        false
    }
}

fn base() -> DateTimeUtc {
    DateTimeUtc::parse_json("2026-09-25T00:00:00Z")
        .expect("base stamp parses")
        .0
}

fn entry() -> JournalEntry {
    JournalEntry {
        id: "op".to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Prepared,
        source: r"C:\photos\op.jpg".to_owned(),
        destination: Some(r"C:\sel\op.jpg".to_owned()),
        size: 5,
        last_write_utc: JournalDateTime::utc(base()),
        timestamp_utc: JournalDateTime::utc(base()),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

fn journal(log: &Arc<Log>) -> OperationJournal {
    let clock = Arc::new(FakeClock::at(base()));
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    OperationJournal::with_options(
        &AppPaths::new(std::path::Path::new(r"C:\Users\test\AppData\Local"), None, false),
        fs,
        clock,
        OperationJournalOptions {
            live_operations: Some(Arc::new(Registry { log: log.clone() })),
            append_retry_delay: Some(Arc::new(|_| {})),
            ..OperationJournalOptions::default()
        },
    )
}

/// Runs `body` on a fresh transaction, then drops it and logs the end of `drop(tx)`.
fn lifecycle(body: impl FnOnce(&mut JournalTransaction<'_>)) -> Vec<String> {
    let log = Arc::new(Log::default());
    let journal = journal(&log);
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(&journal, &tx_clock, entry(), None);
    body(&mut tx);
    drop(tx);
    log.push("tx-dropped");
    log.take()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn drop_of_a_transaction_that_never_began_creates_and_releases_nothing() {
    assert_eq!(lifecycle(|_| {}), strings(&["tx-dropped"]));
}

#[test]
fn drop_of_a_begun_transaction_releases_the_marker_exactly_once_before_drop_returns() {
    let events = lifecycle(|tx| tx.begin().expect("begin"));
    assert_eq!(events, strings(&["marker-created", "marker-released", "tx-dropped"]));
}

#[test]
fn begin_twice_holds_one_marker_and_drop_releases_it_once() {
    let events = lifecycle(|tx| {
        tx.begin().expect("begin");
        tx.begin().expect("second begin keeps the marker");
    });
    assert_eq!(events, strings(&["marker-created", "marker-released", "tx-dropped"]));
}

#[test]
fn commit_releases_before_drop_and_drop_adds_no_second_release() {
    let events = lifecycle(|tx| {
        tx.begin().expect("begin");
        tx.mark_mutation_completed();
        let _ = tx.commit();
    });
    assert_eq!(events, strings(&["marker-created", "marker-released", "tx-dropped"]));
}

#[test]
fn fail_releases_before_drop_and_drop_adds_no_second_release() {
    let events = lifecycle(|tx| {
        tx.begin().expect("begin");
        let _ = tx.fail(&io::Error::other("boom"));
    });
    assert_eq!(events, strings(&["marker-created", "marker-released", "tx-dropped"]));
}

#[test]
fn dismiss_releases_before_drop_and_drop_adds_no_second_release() {
    let events = lifecycle(|tx| {
        tx.begin().expect("begin");
        let _ = tx.dismiss_rolled_back();
    });
    assert_eq!(events, strings(&["marker-created", "marker-released", "tx-dropped"]));
}

#[test]
fn a_panic_after_begin_still_releases_the_marker_while_unwinding() {
    let log = Arc::new(Log::default());
    let journal = journal(&log);
    let tx_clock = FakeClock::at(base());
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut tx = JournalTransaction::new(&journal, &tx_clock, entry(), None);
        tx.begin().expect("begin");
        panic!("caller bug between begin and the outcome");
    }));
    assert!(result.is_err());
    log.push("after-unwind");
    assert_eq!(
        log.take(),
        strings(&["marker-created", "marker-released", "after-unwind"])
    );
}
