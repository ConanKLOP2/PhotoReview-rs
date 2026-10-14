//! Tests for `pr_core::file_actions::transaction::JournalTransaction` (F-12, F-15; port of
//! `PhotoReview.Core/FileActions/JournalTransaction.cs` and the C# tests `FileActions/JournalTransactionTests.cs`,
//! `FileActions/JournalSmallGapsTests.cs` (the three live-marker tests) and the `journal-transaction` golden). Red until the
//! W3-01 bodies land. Nothing here touches the Recycle Bin or a real disk: every journal is on a [`FakeFileSystem`].

use std::error::Error;
use std::io;
use std::sync::Arc;

use pr_core::abstractions::{FileStat, InProcessLiveOperationRegistry, LiveOperationRegistry};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors;
use pr_core::file_actions::journal::{EnglishJournalTexts, JournalTexts, OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{self, JournalDateTime, JournalEntry};
use pr_core::file_actions::transaction::JournalTransaction;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::sync::Canceled;
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_test_support::golden::{self, str_of};
use pr_test_support::{FakeClock, FakeFileSystem, FsOp};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------

/// C# `Stamp` of these tests: `2026-09-29T00:00:00Z` (here the golden base, `2026-09-25T00:00:00Z`).
fn base() -> DateTimeUtc {
    DateTimeUtc::parse_json("2026-09-25T00:00:00Z")
        .expect("base stamp parses")
        .0
}

fn at(seconds: i64) -> DateTimeUtc {
    base()
        .checked_add_ticks(seconds * TICKS_PER_SECOND)
        .expect("stamp in range")
}

fn fake_paths() -> AppPaths {
    AppPaths::new(std::path::Path::new(r"C:\Users\test\AppData\Local"), None, false)
}

/// A journal line with size 5 (the C# `Entry` helper of the small-gaps tests) and both stamps at `base()`.
fn entry(id: &str, state: JournalState, op: FileOperationType) -> JournalEntry {
    JournalEntry {
        id: id.to_owned(),
        op_type: op,
        state,
        source: format!(r"C:\photos\{id}.jpg"),
        destination: Some(format!(r"C:\sel\{id}.jpg")),
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

/// The C# `JournalTransactionTests` entry: a Move `op` with size 10 at `C:\photos\a.jpg`.
fn move_op(state: JournalState) -> JournalEntry {
    JournalEntry {
        size: 10,
        ..entry("op", state, FileOperationType::Move)
    }
}

fn parse_entry(line: &str) -> JournalEntry {
    journal_line::try_parse(line).unwrap_or_else(|| panic!("not an accepted journal line: {line}"))
}

/// `journal` over a fresh fake file system, appends never retry (the C# `appendRetryDelay: _ => { }`).
fn fake_journal(clock: &Arc<FakeClock>) -> (Arc<FakeFileSystem>, OperationJournal) {
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    let journal = OperationJournal::with_options(
        &fake_paths(),
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(|_| {})),
            ..OperationJournalOptions::default()
        },
    );
    (fs, journal)
}

fn journal_file() -> std::path::PathBuf {
    fake_paths().journal_file
}

/// Pending (Prepared) records of the journal (the C# `ReadPendingOperations` helper).
fn read_pending(journal: &OperationJournal) -> Vec<JournalEntry> {
    read_by_state(journal, JournalState::Prepared)
}

/// Failed records of the journal (the C# `ReadFailedOperations` helper).
fn read_failed(journal: &OperationJournal) -> Vec<JournalEntry> {
    read_by_state(journal, JournalState::Failed)
}

fn read_by_state(journal: &OperationJournal, state: JournalState) -> Vec<JournalEntry> {
    journal
        .read_pending_and_failed_operations()
        .expect("read pending and failed")
        .into_iter()
        .filter(|e| e.state == state)
        .collect()
}

/// A `Failed` record with the reconcile code the retry tests start from.
fn failed_retry_record() -> JournalEntry {
    JournalEntry {
        error: Some("x".into()),
        error_code: Some(errors::PENDING_UNCONFIRMED.to_owned()),
        ..move_op(JournalState::Failed)
    }
}

fn specs_bytes(specs: &Value) -> Vec<u8> {
    specs
        .as_array()
        .map(|list| {
            list.iter()
                .flat_map(|spec| {
                    let eol = spec.get("eol").and_then(Value::as_str).unwrap_or("\r\n").to_owned();
                    let text = spec.get("text").and_then(Value::as_str).unwrap_or("").to_owned();
                    format!("{text}{eol}").into_bytes()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// JournalTransactionTests
// ---------------------------------------------------------------------------------------------------------------

// C#: JournalTransactionTests.Fail_RetrySupersededByAnotherWriter_ReturnsNullAndAppendsNothing
#[test]
fn fail_retry_superseded_by_another_writer_returns_null_and_appends_nothing() {
    let clock = Arc::new(FakeClock::at(base()));
    let (_fs, journal) = fake_journal(&clock);
    let failed = failed_retry_record();
    journal.append(&failed).expect("append failed");
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(
        &journal,
        &tx_clock,
        JournalEntry {
            state: JournalState::Prepared,
            ..failed.clone()
        },
        Some(failed.clone()),
    );
    assert!(tx.try_begin().expect("try_begin"));
    // Another process resolves the Id after this retry's Prepared.
    journal
        .append(&JournalEntry {
            state: JournalState::Committed,
            error: None,
            error_code: None,
            ..failed.clone()
        })
        .expect("other process commits");

    let (result, journal_error) = tx.fail(&io::Error::other("boom"));

    assert!(result.is_none());
    assert!(journal_error.is_none());
    assert!(tx.superseded());
    assert!(read_failed(&journal).is_empty());
}

// C#: JournalTransactionTests.Fail_RetryWhosePreparedAppendThrew_StillJournalsTheFailure
#[test]
fn fail_retry_whose_prepared_append_threw_still_journals_the_failure() {
    let clock = Arc::new(FakeClock::at(base()));
    let (fs, journal) = fake_journal(&clock);
    let failed = failed_retry_record();
    journal.append(&failed).expect("append failed");
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(
        &journal,
        &tx_clock,
        JournalEntry {
            state: JournalState::Prepared,
            ..failed.clone()
        },
        Some(failed.clone()),
    );
    fs.fail(FsOp::OpenAppend, journal_file(), io::ErrorKind::Other); // "disk full"
    assert!(tx.try_begin().is_err());
    fs.clear_faults();
    assert!(!tx.is_prepared());

    // A retry always records its outcome (the retried item stays a Recovery item with the NEW reason), even without its own Prepared.
    let (result, journal_error) = tx.fail(&io::Error::other("boom"));

    assert!(journal_error.is_none());
    let result = result.expect("the failed record is returned");
    assert_eq!(result.state, JournalState::Failed);
    let latest = read_failed(&journal);
    assert_eq!(latest.len(), 1);
    assert_ne!(latest[0].error.as_deref(), Some("x"));
}

// C#: JournalTransactionTests.Fail_FirstAttemptWhosePreparedAppendThrew_WritesNoLine
#[test]
fn fail_first_attempt_whose_prepared_append_threw_writes_no_line() {
    let clock = Arc::new(FakeClock::at(base()));
    let (fs, journal) = fake_journal(&clock);
    let prepared = move_op(JournalState::Prepared);
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(&journal, &tx_clock, prepared, None);
    fs.fail(FsOp::OpenAppend, journal_file(), io::ErrorKind::Other);
    assert!(tx.try_begin().is_err());
    fs.clear_faults();

    let (result, journal_error) = tx.fail(&io::Error::other("boom"));

    assert!(result.is_none());
    assert!(journal_error.is_none());
    assert!(journal.read_pending_and_failed_operations().expect("read").is_empty());
}

// C#: JournalTransactionTests.Fail_RetryStillCurrent_ReturnsTheFailedRecord
#[test]
fn fail_retry_still_current_returns_the_failed_record() {
    let clock = Arc::new(FakeClock::at(base()));
    let (_fs, journal) = fake_journal(&clock);
    let failed = failed_retry_record();
    journal.append(&failed).expect("append failed");
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(
        &journal,
        &tx_clock,
        JournalEntry {
            state: JournalState::Prepared,
            ..failed.clone()
        },
        Some(failed.clone()),
    );
    assert!(tx.try_begin().expect("try_begin"));

    let (result, _) = tx.fail(&io::Error::other("boom"));

    let result = result.expect("the failed record is returned");
    assert_eq!(result.state, JournalState::Failed);
    assert!(!tx.superseded());
    assert_eq!(read_failed(&journal).len(), 1);
    assert_eq!(read_failed(&journal)[0].id, "op");
}

// C#: JournalTransactionTests.Fail_OutcomeAppendThrows_ReturnsNullAndReportsTheJournalError
#[test]
fn fail_outcome_append_throws_returns_null_and_reports_the_journal_error() {
    let clock = Arc::new(FakeClock::at(base()));
    let (fs, journal) = fake_journal(&clock);
    let prepared = move_op(JournalState::Prepared);
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(&journal, &tx_clock, prepared, None);
    tx.begin().expect("begin");
    fs.fail(FsOp::OpenAppend, journal_file(), io::ErrorKind::Other); // only the outcome line fails

    let (result, journal_error) = tx.fail(&io::Error::other("boom"));

    assert!(
        result.is_none(),
        "no record was written: a caller must not treat it as a journaled Failed line"
    );
    assert!(journal_error.is_some());
    fs.clear_faults();
    let pending = read_pending(&journal);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "op"); // still only Prepared: reconcile will judge it
}

// C#: JournalTransactionTests.DismissRolledBack_OutcomeAppendThrows_ReturnsNullAndReportsTheJournalError
#[test]
fn dismiss_rolled_back_outcome_append_throws_returns_null_and_reports_the_journal_error() {
    let clock = Arc::new(FakeClock::at(base()));
    let (fs, journal) = fake_journal(&clock);
    let prepared = move_op(JournalState::Prepared);
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(&journal, &tx_clock, prepared, None);
    tx.begin().expect("begin");
    fs.fail(FsOp::OpenAppend, journal_file(), io::ErrorKind::Other);

    let (result, journal_error) = tx.dismiss_rolled_back();

    assert!(result.is_none());
    assert!(journal_error.is_some());
}

// C#: JournalTransactionTests.ReadLatestEntry_StaleReconcileFailedAfterCommittedRetry_ReturnsTheCommittedLine
#[test]
fn read_latest_entry_stale_reconcile_failed_after_committed_retry_returns_the_committed_line() {
    let clock = Arc::new(FakeClock::at(base()));
    let (_fs, journal) = fake_journal(&clock);
    let failed = failed_retry_record();
    let committed = JournalEntry {
        state: JournalState::Committed,
        error: None,
        error_code: None,
        ..failed.clone()
    };
    journal.append(&failed).expect("failed");
    journal.append(&committed).expect("committed");
    journal
        .append(&failed)
        .expect("another process's reconcile verdict from a stale snapshot");

    assert_eq!(journal.read_latest_entry("op").expect("read latest"), Some(committed));
}

// C#: JournalTransactionTests.ReadLatestEntry_NonReconcileFailedAfterCommitted_StillWins
#[test]
fn read_latest_entry_non_reconcile_failed_after_committed_still_wins() {
    let clock = Arc::new(FakeClock::at(base()));
    let (_fs, journal) = fake_journal(&clock);
    let committed = move_op(JournalState::Committed);
    let failed = JournalEntry {
        state: JournalState::Failed,
        error: Some("io".into()),
        error_code: None,
        ..committed.clone()
    };
    journal.append(&committed).expect("committed");
    journal.append(&failed).expect("failed");

    assert_eq!(journal.read_latest_entry("op").expect("read latest"), Some(failed));
}

// ---------------------------------------------------------------------------------------------------------------
// JournalSmallGapsTests: the live marker is released as soon as the outcome line is written (the Build_*, RunAsync_* and
// JournalCodedException_* / TryParse_* members of that file belong to W3-02, W3-03 and the done W1-13/W1-14 units).
// ---------------------------------------------------------------------------------------------------------------

/// A journal sharing one live-operation registry, so the test can look at the marker the transaction holds.
fn journal_with_registry(clock: &Arc<FakeClock>) -> (OperationJournal, Arc<dyn LiveOperationRegistry>) {
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    let registry: Arc<dyn LiveOperationRegistry> = Arc::new(InProcessLiveOperationRegistry::new());
    let journal = OperationJournal::with_options(
        &fake_paths(),
        fs,
        clock.clone(),
        OperationJournalOptions {
            live_operations: Some(registry.clone()),
            append_retry_delay: Some(Arc::new(|_| {})),
            ..OperationJournalOptions::default()
        },
    );
    (journal, registry)
}

// C#: JournalSmallGapsTests.Commit_ReleasesTheLiveMarkerBeforeDispose
#[test]
fn commit_releases_the_live_marker_before_dispose() {
    let clock = Arc::new(FakeClock::at(base()));
    let (journal, registry) = journal_with_registry(&clock);
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(
        &journal,
        &tx_clock,
        entry("op", JournalState::Prepared, FileOperationType::Move),
        None,
    );
    tx.begin().expect("begin");
    assert!(registry.is_live("op"));

    let _ = tx.commit();

    assert!(!registry.is_live("op"));
}

// C#: JournalSmallGapsTests.Fail_ReleasesTheLiveMarkerBeforeDispose
#[test]
fn fail_releases_the_live_marker_before_dispose() {
    let clock = Arc::new(FakeClock::at(base()));
    let (journal, registry) = journal_with_registry(&clock);
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(
        &journal,
        &tx_clock,
        entry("op", JournalState::Prepared, FileOperationType::Move),
        None,
    );
    tx.begin().expect("begin");
    assert!(registry.is_live("op"));

    let _ = tx.fail(&io::Error::other("boom"));

    assert!(!registry.is_live("op"));
}

// C#: JournalSmallGapsTests.DismissRolledBack_ReleasesTheLiveMarkerBeforeDispose
#[test]
fn dismiss_rolled_back_releases_the_live_marker_before_dispose() {
    let clock = Arc::new(FakeClock::at(base()));
    let (journal, registry) = journal_with_registry(&clock);
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(
        &journal,
        &tx_clock,
        entry("op", JournalState::Prepared, FileOperationType::Move),
        None,
    );
    tx.begin().expect("begin");

    let _ = tx.dismiss_rolled_back();

    assert!(!registry.is_live("op"));
}

// C#: JournalCommitClockFailureTests.Commit_ClockThrows_ReturnsEntryAndJournalErrorWithoutThrowing
// DIVERGENCE: not portable. `Clock::utc_now` cannot fail in Rust (trap T20), so there is no throwing-clock path to test; the
// C# `catch` around the stamp has no counterpart. `ExecuteAsync_ClockThrows...` (W3-04) has the same divergence.

// ---------------------------------------------------------------------------------------------------------------
// Golden: journal-transaction (a JournalTransaction driven step by step; the flags and the appended lines after each step).
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn golden_journal_transaction() {
    let golden = golden::load("journal-transaction");
    for case in &golden.cases {
        replay_transaction_case(case);
    }
}

/// The failure a `fail` step names: `{"io": m}`, `{"coded": c}` (a journal-coded error with the English text) or
/// `{"canceled": true}`.
fn failure_of(error: &Value) -> Box<dyn Error> {
    if let Some(message) = error.get("io").and_then(Value::as_str) {
        return Box::new(io::Error::other(message.to_owned()));
    }
    if let Some(code) = error.get("coded").and_then(Value::as_str) {
        let key = errors::catalog_key(code).expect("known code");
        let text = EnglishJournalTexts.english(key);
        return Box::new(errors::JournalCodedError::new(code, text).into_io_error());
    }
    assert_eq!(
        error.get("canceled").and_then(Value::as_bool),
        Some(true),
        "unknown failure {error}"
    );
    Box::new(io::Error::other(Canceled))
}

fn replay_transaction_case(case: &Value) {
    let name = str_of(case, "name");
    let input = &case["case"];
    let clock = Arc::new(FakeClock::at(base()));
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    let paths = fake_paths();
    let path = paths.journal_file.clone();
    fs.add_dir(path.parent().expect("parent"));
    if input["initial"].is_array() {
        fs.add_file(&path, &specs_bytes(&input["initial"]));
    }
    let registry: Arc<dyn LiveOperationRegistry> = Arc::new(InProcessLiveOperationRegistry::new());
    let journal = OperationJournal::with_options(
        &paths,
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(|_| {})),
            live_operations: Some(registry.clone()),
            ..OperationJournalOptions::default()
        },
    );
    // The second journal instance on the same file (`other_append`).
    let other = OperationJournal::with_options(
        &paths,
        fs.clone(),
        clock.clone(),
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(|_| {})),
            ..OperationJournalOptions::default()
        },
    );
    let tx_clock = FakeClock::at(at(input.get("tx_now").and_then(Value::as_i64).unwrap_or(100)));
    let prepared = parse_entry(str_of(input, "prepared"));
    let retry_of = input.get("retry_of").and_then(Value::as_str).map(parse_entry);
    let mut tx = Some(JournalTransaction::new(&journal, &tx_clock, prepared.clone(), retry_of));

    let steps = input["steps"].as_array().expect("steps");
    let wanted = case["steps"].as_array().expect("expected steps");
    assert_eq!(steps.len(), wanted.len(), "case {name}: one expectation per step");
    for (step, want) in steps.iter().zip(wanted) {
        let kind = str_of(step, "do");
        if let Some(n) = step.get("fail_append").and_then(Value::as_u64).filter(|n| *n > 0) {
            fs.fail_times(
                FsOp::OpenAppend,
                &path,
                io::ErrorKind::Other,
                usize::try_from(n).expect("count"),
            );
        }
        let opens_before = fs.count(FsOp::OpenAppend);
        let mut throws = false;
        let mut got = serde_json::Map::new();
        let mut flags_at_dispose = None;
        match kind {
            "try_begin" => match tx.as_mut().expect("transaction").try_begin() {
                Ok(result) => {
                    got.insert("result".into(), json!(result));
                }
                Err(_) => throws = true,
            },
            "begin" => throws = tx.as_mut().expect("transaction").begin().is_err(),
            "verify_stat" => {
                let stat = step.get("length").and_then(Value::as_u64).map(|length| FileStat {
                    length,
                    last_write_utc: base(),
                });
                throws = tx
                    .as_mut()
                    .expect("transaction")
                    .verify_destination_stat(stat, str_of(step, "code"))
                    .is_err();
            }
            "mark" => tx.as_mut().expect("transaction").mark_mutation_completed(),
            "commit" => {
                let (entry, journal_error) = tx.as_mut().expect("transaction").commit();
                got.insert("entry".into(), json!(entry.to_line()));
                got.insert("journal_error_set".into(), json!(journal_error.is_some()));
            }
            "fail" => {
                let failure = failure_of(&step["error"]);
                let (entry, journal_error) = tx.as_mut().expect("transaction").fail(failure.as_ref());
                got.insert("entry".into(), json!(entry.map(|e| e.to_line())));
                got.insert("journal_error_set".into(), json!(journal_error.is_some()));
            }
            "dismiss_rolled_back" => {
                let (entry, journal_error) = tx.as_mut().expect("transaction").dismiss_rolled_back();
                got.insert("entry".into(), json!(entry.map(|e| e.to_line())));
                got.insert("journal_error_set".into(), json!(journal_error.is_some()));
            }
            "other_append" => other
                .append(&parse_entry(str_of(step, "entry")))
                .expect("other writer appends"),
            "dispose" => {
                // Read the flags, drop the transaction, then read the marker (the golden's order).
                let finished = tx.take().expect("transaction");
                flags_at_dispose = Some((
                    finished.is_prepared(),
                    finished.superseded(),
                    finished.mutation_completed(),
                ));
                drop(finished);
            }
            other => panic!("case {name}: unknown step {other}"),
        }
        fs.clear_faults();
        let (is_prepared, superseded, mutation_completed) = flags_at_dispose.unwrap_or_else(|| {
            let current = tx.as_ref().expect("transaction");
            (
                current.is_prepared(),
                current.superseded(),
                current.mutation_completed(),
            )
        });
        let live = registry.is_live(&prepared.id);

        assert_eq!(throws, want.get("throws").is_some(), "case {name}: {kind} throws");
        for key in ["result", "entry", "journal_error_set"] {
            if let Some(expected) = want.get(key) {
                assert_eq!(got.get(key), Some(expected), "case {name}: {kind} {key}");
            }
        }
        assert_eq!(
            fs.count(FsOp::OpenAppend) - opens_before,
            want["append_opens"].as_u64().unwrap_or(0) as usize,
            "case {name}: {kind} append_opens"
        );
        assert_eq!(
            is_prepared,
            want["is_prepared"].as_bool().unwrap_or(false),
            "case {name}: {kind} is_prepared"
        );
        assert_eq!(
            superseded,
            want["superseded"].as_bool().unwrap_or(false),
            "case {name}: {kind} superseded"
        );
        assert_eq!(
            mutation_completed,
            want["mutation_completed"].as_bool().unwrap_or(false),
            "case {name}: {kind} mutation_completed"
        );
        assert_eq!(
            live,
            want["live"].as_bool().unwrap_or(false),
            "case {name}: {kind} live"
        );
    }

    let final_bytes = fs.file_content(&path);
    assert_eq!(
        final_bytes.as_deref().map(hex_upper),
        case["final_hex"].as_str().map(str::to_owned),
        "case {name}: final_hex"
    );
}
