//! P-331 (owner decision 2026-11-03): the startup reconcile of a single Recycle whose source is gone confirms the item in
//! the Recycle Bin before it writes Committed. Different from C# `ReconcileOne` (R15 trusts "source gone") until the C#
//! branch `feat/reconcile-recycle-bin-check` merges; the C# twin tests are `OperationJournalTests.ReconcileRecycle*Bin*`.
//! The bin is `FakeRecycleBin` (never the real one, AGENTS.md).

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pr_core::abstractions::{Log, RecycleBin};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors::{PENDING_UNCONFIRMED, SOURCE_STILL_EXISTS_AFTER_RECOVERY};
use pr_core::file_actions::journal::{AppendRetryDelay, OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::{FakeClock, FakeFileSystem, FakeRecycleBin, LogLevel, RecordingLog};

const JOURNAL: &str = r"C:\Users\test\AppData\Local\PhotoReview\Data\operations.jsonl";
const PHOTO: &str = r"C:\photos\a.jpg";

fn at(secs: i64) -> DateTimeUtc {
    let base = DateTimeUtc::parse_json("2026-09-29T00:00:00Z").expect("stamp").0;
    DateTimeUtc::from_ticks(base.ticks() + secs * TICKS_PER_SECOND).expect("stamp in range")
}

fn prepared_recycle(id: &str, source: &str, size: i64, stamp: DateTimeUtc) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type: FileOperationType::Recycle,
        state: JournalState::Prepared,
        source: source.to_owned(),
        destination: None,
        size,
        last_write_utc: JournalDateTime::utc(stamp),
        timestamp_utc: JournalDateTime::utc(stamp),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

fn paths() -> AppPaths {
    let mut paths = AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false);
    paths.journal_file = JOURNAL.into();
    paths
}

struct World {
    disk: Arc<FakeFileSystem>,
    bin: Arc<FakeRecycleBin>,
    log: Arc<RecordingLog>,
    journal: OperationJournal,
}

impl World {
    /// `wire_bin` false = the journal has no bin wired.
    fn new(wire_bin: bool) -> Self {
        let disk = Arc::new(FakeFileSystem::new());
        let bin = Arc::new(FakeRecycleBin::new(Arc::clone(&disk)));
        let log = Arc::new(RecordingLog::new());
        let delay: AppendRetryDelay = Arc::new(|_: Duration| {});
        let bin_for_journal: Option<Arc<dyn RecycleBin>> = wire_bin.then(|| Arc::clone(&bin) as Arc<dyn RecycleBin>);
        let log_for_journal: Arc<dyn Log> = log.clone();
        let journal = OperationJournal::with_options(
            &paths(),
            disk.clone(),
            Arc::new(FakeClock::at(at(60))),
            OperationJournalOptions {
                append_retry_delay: Some(delay),
                log: Some(log_for_journal),
                recycle_bin: bin_for_journal,
                ..Default::default()
            },
        );
        Self {
            disk,
            bin,
            log,
            journal,
        }
    }

    /// A 5-byte file at `PHOTO` moved to the bin, and a Prepared Recycle line for it (the crash after the Shell
    /// finished but before Committed was written).
    fn recycled_and_journaled(&self, id: &str) {
        self.disk.add_file_at(PHOTO, b"12345", at(0));
        self.bin.send_to_recycle_bin(Path::new(PHOTO)).expect("recycle");
        self.journal
            .append(&prepared_recycle(id, PHOTO, 5, at(0)))
            .expect("append");
    }

    fn reconcile(&self) -> Vec<JournalEntry> {
        self.journal.reconcile_pending_operations(None).expect("reconcile")
    }

    fn latest(&self, id: &str) -> JournalState {
        let mut state = None;
        self.journal
            .read_entries(&mut |entry| {
                if entry.id == id {
                    state = Some(entry.state);
                }
            })
            .expect("read");
        state.expect("entry present")
    }
}

#[test]
fn reconcile_recycle_source_gone_and_item_in_bin_is_committed() {
    // C#: OperationJournalTests.ReconcileRecycleSourceGoneAndItemInBinIsCommitted (feat/reconcile-recycle-bin-check)
    let w = World::new(true);
    w.recycled_and_journaled("rec");

    let reconciled = w.reconcile();

    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, JournalState::Committed);
    assert_eq!(reconciled[0].error, None);
    assert_eq!(w.latest("rec"), JournalState::Committed);
}

#[test]
fn reconcile_recycle_source_gone_but_item_not_in_bin_is_failed_pending_unconfirmed() {
    // C#: OperationJournalTests.ReconcileRecycleSourceGoneButItemNotInBinIsFailed (feat/reconcile-recycle-bin-check)
    let w = World::new(true);
    // The file vanished (deleted for good, moved away): nothing is in the bin.
    w.journal
        .append(&prepared_recycle("gone", PHOTO, 5, at(0)))
        .expect("append");

    let reconciled = w.reconcile();

    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, JournalState::Failed);
    assert_eq!(reconciled[0].error_code.as_deref(), Some(PENDING_UNCONFIRMED));
    assert_eq!(w.latest("gone"), JournalState::Failed);
}

#[test]
fn reconcile_recycle_item_in_bin_with_other_size_is_not_committed() {
    // C#: OperationJournalTests.ReconcileRecycleItemInBinWithOtherSizeIsFailed (feat/reconcile-recycle-bin-check)
    let w = World::new(true);
    w.disk.add_file_at(PHOTO, b"12345", at(0));
    w.bin.send_to_recycle_bin(Path::new(PHOTO)).expect("recycle");
    w.journal
        .append(&prepared_recycle("other", PHOTO, 6, at(0)))
        .expect("append");

    let reconciled = w.reconcile();

    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, JournalState::Failed);
}

#[test]
fn reconcile_recycle_bin_query_error_leaves_entry_prepared() {
    // C#: OperationJournalTests.ReconcileRecycleBinQueryErrorLeavesEntryPrepared (feat/reconcile-recycle-bin-check)
    // INV-5: a bin that cannot be asked is never "not in the bin" and never "in the bin".
    let w = World::new(true);
    w.recycled_and_journaled("err");
    w.bin
        .set_contains_hook(Some(Arc::new(|_| Some(io::Error::other("shell unavailable")))));

    let reconciled = w.reconcile();

    assert!(reconciled.is_empty());
    assert_eq!(w.latest("err"), JournalState::Prepared);
    assert!(
        w.log
            .contains(LogLevel::Warn, "Journal reconcile: skipped err: shell unavailable")
    );

    // The next start retries and, once the bin answers, confirms.
    w.bin.set_contains_hook(None);
    let retried = w.reconcile();
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].state, JournalState::Committed);
}

#[test]
fn reconcile_recycle_with_bin_that_cannot_answer_leaves_entry_prepared() {
    // INV-5 for a bin that keeps the trait default (`Unsupported`): never Committed by guess.
    struct SilentBin;
    impl RecycleBin for SilentBin {
        fn send_to_recycle_bin(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }
        fn try_restore(&self, _p: &Path, _s: u64, _w: DateTimeUtc) -> io::Result<bool> {
            Ok(false)
        }
    }
    let journal = OperationJournal::with_options(
        &paths(),
        Arc::new(FakeFileSystem::new()),
        Arc::new(FakeClock::at(at(60))),
        OperationJournalOptions {
            log: Some(Arc::new(RecordingLog::new())),
            recycle_bin: Some(Arc::new(SilentBin)),
            ..Default::default()
        },
    );
    journal
        .append(&prepared_recycle("silent", PHOTO, 5, at(0)))
        .expect("append");

    assert!(
        journal
            .reconcile_pending_operations(None)
            .expect("reconcile")
            .is_empty()
    );
}

#[test]
fn reconcile_recycle_source_present_is_failed_without_asking_the_bin() {
    // C#: OperationJournalTests.ReconcileRecycle (source present half), unchanged by P-331.
    let w = World::new(true);
    w.disk.add_file_at(PHOTO, b"12345", at(0));
    w.journal
        .append(&prepared_recycle("present", PHOTO, 5, at(0)))
        .expect("append");
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&asked);
    w.bin.set_contains_hook(Some(Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        None
    })));

    let reconciled = w.reconcile();

    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, JournalState::Failed);
    assert_eq!(
        reconciled[0].error_code.as_deref(),
        Some(SOURCE_STILL_EXISTS_AFTER_RECOVERY)
    );
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

#[test]
fn reconcile_recycle_without_wired_bin_trusts_source_gone_as_before() {
    // No bin wired (C# R15 as is): Committed on "source gone".
    let w = World::new(false);
    w.journal
        .append(&prepared_recycle("legacy", PHOTO, 5, at(0)))
        .expect("append");

    let reconciled = w.reconcile();

    assert_eq!(reconciled.len(), 1);
    assert_eq!(reconciled[0].state, JournalState::Committed);
}
