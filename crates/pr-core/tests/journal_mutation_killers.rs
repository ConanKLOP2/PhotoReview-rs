//! Mutation killers for the operation journal files (W3-01 mutation debt, `docs/reviews/W3-01-JOURNAL-MUTATION.md`).
//! Each test names the cargo-mutants mutant it kills. Fakes only: no real disk, no Recycle Bin, no sleeps.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use pr_core::abstractions::{Clock, InProcessLiveOperationRegistry, LiveOperationRegistry};
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors;
use pr_core::file_actions::journal::{OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{self, JournalDateTime, JournalEntry};
use pr_core::file_actions::journal_rules::{is_reconcile_code, is_stale_reconcile_after_committed};
use pr_core::file_actions::transaction::JournalTransaction;
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, FakeFileSystem};

fn base() -> DateTimeUtc {
    DateTimeUtc::parse_json("2026-09-25T00:00:00Z")
        .expect("base stamp parses")
        .0
}

fn fake_paths() -> AppPaths {
    AppPaths::new(std::path::Path::new(r"C:\Users\test\AppData\Local"), None, false)
}

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

// transaction.rs:62:9 replace <impl fmt::Debug for JournalTransaction>::fmt -> fmt::Result with Ok(Default::default())
#[test]
fn transaction_debug_names_the_type_and_every_state_flag() {
    let clock = Arc::new(FakeClock::at(base()));
    let (journal, _registry) = journal_with_registry(&clock);
    let tx_clock = FakeClock::at(base());
    let mut tx = JournalTransaction::new(
        &journal,
        &tx_clock,
        entry("op7", JournalState::Prepared, FileOperationType::Move),
        None,
    );
    tx.begin().expect("begin");
    let text = format!("{tx:?}");
    assert!(text.contains("JournalTransaction"), "{text}");
    assert!(text.contains("op7"), "{text}");
    assert!(text.contains("is_prepared: true"), "{text}");
    assert!(text.contains("live: true"), "{text}");
    assert!(text.contains("mutation_completed: false"), "{text}");
}

// journal.rs:263:9 replace <impl fmt::Debug for OperationJournalOptions>::fmt -> fmt::Result with Ok(Default::default())
#[test]
fn journal_options_debug_names_the_type_and_reports_which_options_are_set() {
    let options = OperationJournalOptions {
        append_retry_delay: Some(Arc::new(|_| {})),
        ..OperationJournalOptions::default()
    };
    let text = format!("{options:?}");
    assert!(text.contains("OperationJournalOptions"), "{text}");
    assert!(text.contains("append_retry_delay: true"), "{text}");
    assert!(text.contains("durability: false"), "{text}");
}

fn plain_journal(clock: &Arc<FakeClock>) -> OperationJournal {
    journal_with_registry(clock).0
}

fn latest_line(journal: &OperationJournal, id: &str) -> JournalEntry {
    let mut latest = None;
    journal
        .read_entries(&mut |e| {
            if e.id == id {
                latest = Some(e);
            }
        })
        .expect("read entries");
    latest.expect("entry present")
}

// journal.rs:470:21 delete field state / 471:21 delete field timestamp_utc / 472:21 delete field error /
// 473:21 delete field error_code from the struct JournalEntry expression in OperationJournal::dismiss
#[test]
fn dismiss_sets_state_stamp_and_clears_error_in_the_outcome_and_in_the_written_line() {
    let clock = Arc::new(FakeClock::at(base()));
    let journal = plain_journal(&clock);
    let failed = JournalEntry {
        error: Some("boom".into()),
        error_code: Some("core.x".into()),
        ..entry("f1", JournalState::Failed, FileOperationType::Move)
    };
    journal.append(&failed).expect("append");
    clock.advance_seconds(90);
    let now = clock.utc_now();

    let outcome = journal.dismiss(std::slice::from_ref(&failed)).expect("dismiss");

    assert_eq!(outcome.dismissed.len(), 1);
    assert!(outcome.skipped.is_empty());
    for dismissed in [outcome.dismissed[0].clone(), latest_line(&journal, "f1")] {
        assert_eq!(dismissed.state, JournalState::Dismissed);
        assert_eq!(dismissed.timestamp_utc, JournalDateTime::utc(now));
        assert_ne!(dismissed.timestamp_utc, failed.timestamp_utc);
        assert_eq!(dismissed.error, None);
        assert_eq!(dismissed.error_code, None);
        assert_eq!(dismissed.source, failed.source);
    }
}

// journal_rules.rs:42:5 replace is_reconcile_code -> bool with true
#[test]
fn is_reconcile_code_accepts_only_the_two_reconcile_codes() {
    assert!(is_reconcile_code(Some(errors::PENDING_UNCONFIRMED)));
    assert!(is_reconcile_code(Some(errors::SOURCE_STILL_EXISTS_AFTER_RECOVERY)));
    assert!(!is_reconcile_code(None));
    assert!(!is_reconcile_code(Some("")));
    assert!(!is_reconcile_code(Some(errors::VERIFY_SIZE_CHANGED)));
    assert!(!is_reconcile_code(Some("pendingunconfirmed")));
}

// journal_rules.rs:54:9 replace && with || in is_stale_reconcile_after_committed
#[test]
fn stale_reconcile_needs_a_reconcile_code_and_a_committed_previous_line() {
    let committed = entry("a", JournalState::Committed, FileOperationType::Move);
    let prepared = entry("a", JournalState::Prepared, FileOperationType::Move);
    let reconcile_failed = JournalEntry {
        error_code: Some(errors::PENDING_UNCONFIRMED.to_owned()),
        ..entry("a", JournalState::Failed, FileOperationType::Move)
    };
    let other_failed = JournalEntry {
        error_code: Some(errors::VERIFY_SIZE_CHANGED.to_owned()),
        ..entry("a", JournalState::Failed, FileOperationType::Move)
    };
    assert!(is_stale_reconcile_after_committed(&reconcile_failed, Some(&committed)));
    // a reconcile code after a non-committed previous line is a real failure
    assert!(!is_stale_reconcile_after_committed(&reconcile_failed, Some(&prepared)));
    assert!(!is_stale_reconcile_after_committed(&reconcile_failed, None));
    // a different code after Committed is a real failure too
    assert!(!is_stale_reconcile_after_committed(&other_failed, Some(&committed)));
    // a Failed line without any code
    let no_code = entry("a", JournalState::Failed, FileOperationType::Move);
    assert!(!is_stale_reconcile_after_committed(&no_code, Some(&committed)));
}

fn hash_of(value: &JournalDateTime) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

// journal_line.rs:116:9 replace <impl PartialEq for JournalDateTime>::eq -> bool with true
// journal_line.rs:124:9 replace <impl std::hash::Hash for JournalDateTime>::hash with ()
#[test]
fn journal_date_time_equality_and_hash_follow_the_instant() {
    let a = JournalDateTime::utc(base());
    let a_again = JournalDateTime::utc(base());
    let b = JournalDateTime::utc(DateTimeUtc::parse_json("2026-09-25T00:00:01Z").expect("parses").0);
    assert_eq!(a, a_again);
    assert_ne!(a, b);
    assert_eq!(hash_of(&a), hash_of(&a_again));
    assert_ne!(hash_of(&a), hash_of(&b));
}

fn move_line() -> (JournalEntry, String) {
    let e = entry("a", JournalState::Committed, FileOperationType::Move);
    let line = e.to_line();
    (e, line)
}

// journal_line.rs:320:17 delete match arm b'"' in nests_too_deep
#[test]
fn line_with_many_open_brackets_inside_a_string_is_not_too_deep() {
    let (_, line) = move_line();
    let brackets = "[".repeat(80);
    let crafted = line.replacen(
        r#""Source":"C:\\photos\\a.jpg""#,
        &format!(r#""Source":"{brackets}""#),
        1,
    );
    assert_ne!(crafted, line, "the Source member was replaced");
    let parsed = journal_line::try_parse(&crafted).expect("brackets inside a string do not nest");
    assert_eq!(parsed.source, brackets);
}

// journal_line.rs:315:25 replace == with != in nests_too_deep (the backslash test inside a string)
#[test]
fn line_with_an_escaped_quote_before_many_brackets_stays_inside_the_string() {
    let (_, line) = move_line();
    let brackets = "[".repeat(80);
    let crafted = line.replacen(
        r#""Source":"C:\\photos\\a.jpg""#,
        &format!(r#""Source":"x\"{brackets}""#),
        1,
    );
    assert_ne!(crafted, line, "the Source member was replaced");
    let parsed = journal_line::try_parse(&crafted).expect("an escaped quote does not end the string");
    assert_eq!(parsed.source, format!("x\"{brackets}"));
}

// journal_line.rs:327:17 delete match arm b'}' | b']' in nests_too_deep
#[test]
fn many_sequential_closed_containers_are_not_too_deep() {
    let (expected, line) = move_line();
    let siblings = "[],".repeat(80);
    let crafted = format!(
        r#"{},"Extra":[{}[]]}}"#,
        line.trim_end().trim_end_matches('}'),
        siblings
    );
    let parsed = journal_line::try_parse(&crafted).expect("closed containers do not accumulate depth");
    assert_eq!(parsed, expected);
}

// journal_line.rs:327:17 (counterpart): a genuinely deep line is still refused
#[test]
fn line_nested_deeper_than_the_limit_is_refused() {
    let (_, line) = move_line();
    let deep = format!("{}{}", "[".repeat(70), "]".repeat(70));
    let crafted = format!(r#"{},"Extra":{}}}"#, line.trim_end().trim_end_matches('}'), deep);
    assert!(journal_line::try_parse(&crafted).is_none());
}

// journal.rs:77:55 replace * with / (and with +) in STALE_COMPACTION_FILE_AGE_TICKS: the compaction tests derive their
// "stale" and "fresh" ages from this constant, so only a pinned value can notice a changed constant.
#[test]
fn stale_compaction_file_age_is_one_hour_in_ticks() {
    assert_eq!(
        pr_core::file_actions::journal::STALE_COMPACTION_FILE_AGE_TICKS,
        3600 * 10_000_000_i64
    );
}
