//! Journal mutation guards, with fake storage and a fixed clock.
//! Oracle: src/PhotoReview.Core/FileActions/OperationJournal.cs at C# 7eaee2a2.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pr_core::app_paths::AppPaths;
use pr_core::file_actions::errors::PENDING_UNCONFIRMED;
use pr_core::file_actions::journal::{OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, FakeFileSystem, FsOp, TempRoot};

fn entry(id: &str, state: JournalState) -> JournalEntry {
    let stamp = JournalDateTime::utc(DateTimeUtc::from_ticks(638_943_000_000_000_000).expect("fixed stamp"));
    JournalEntry {
        id: id.to_owned(),
        op_type: FileOperationType::Move,
        state,
        source: "source.jpg".to_owned(),
        destination: Some("selected.jpg".to_owned()),
        size: 7,
        last_write_utc: stamp,
        timestamp_utc: stamp,
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

fn fixture(root: &TempRoot) -> (AppPaths, Arc<FakeClock>, Arc<FakeFileSystem>) {
    let paths = AppPaths::new(root.path(), None, false);
    let clock = Arc::new(FakeClock::at(
        DateTimeUtc::from_ticks(638_943_000_000_000_000).expect("fixed stamp"),
    ));
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    (paths, clock, fs)
}

// journal.rs:557:9 return None/default; :560:31 && -> ||; :560:25 == -> !=; :560:34 delete !.
// C#: OperationJournal.ReadLatestEntry (703..714), ordinal Id and stale reconcile rule.
#[test]
fn read_latest_entry_filters_ordinal_ids_and_ignores_stale_reconcile_after_committed() {
    let root = TempRoot::new("jrnl5-latest");
    let (paths, clock, fs) = fixture(&root);
    let journal = OperationJournal::new(&paths, fs, clock);
    assert_eq!(journal.read_latest_entry("op").expect("missing journal"), None);
    let prepared = entry("op", JournalState::Prepared);
    journal.append(&prepared).expect("prepared");
    assert_eq!(journal.read_latest_entry("op").expect("latest"), Some(prepared));
    let committed = entry("op", JournalState::Committed);
    journal.append(&committed).expect("committed");
    let stale = JournalEntry {
        error_code: Some(PENDING_UNCONFIRMED.to_owned()),
        ..entry("op", JournalState::Failed)
    };
    journal.append(&stale).expect("stale reconcile");
    journal
        .append(&entry("OP", JournalState::Prepared))
        .expect("other ordinal id");
    assert_eq!(journal.read_latest_entry("op").expect("latest"), Some(committed));
    assert_eq!(journal.read_latest_entry("absent").expect("absent id"), None);
    let failed = JournalEntry {
        error: Some("ordinary failure".to_owned()),
        ..entry("op", JournalState::Failed)
    };
    journal.append(&failed).expect("ordinary failed");
    assert_eq!(journal.read_latest_entry("op").expect("latest"), Some(failed));
}

// journal.rs:543:9 return true/false; :546:58 == -> !=; :546:87 + -> -/*; :547:35 != -> ==.
// C#: OperationJournal.AppendIfUnchangedSince (682..698), FindLastIndex and missing-anchor behavior.
#[test]
fn append_if_unchanged_since_uses_last_equal_anchor_and_whole_history_when_absent() {
    let root = TempRoot::new("jrnl5-conditional");
    let (paths, clock, fs) = fixture(&root);
    let journal = OperationJournal::new(&paths, fs.clone(), clock);
    let anchor = entry("op", JournalState::Failed);
    let own = entry("op", JournalState::Prepared);
    let committed = entry("op", JournalState::Committed);
    for line in [&anchor, &own, &anchor, &own] {
        journal.append(line).expect("history");
    }
    assert!(
        journal
            .append_if_unchanged_since(&anchor, std::slice::from_ref(&own), &committed)
            .expect("last anchor")
    );
    let before = fs.file_content(&paths.journal_file).expect("bytes");
    assert!(
        !journal
            .append_if_unchanged_since(&anchor, &[own], &committed)
            .expect("foreign tail")
    );
    assert_eq!(fs.file_content(&paths.journal_file).expect("bytes"), before);
    let missing = entry("new-id", JournalState::Failed);
    let first = entry("new-id", JournalState::Prepared);
    assert!(
        journal
            .append_if_unchanged_since(&missing, &[], &first)
            .expect("absent id")
    );
    let outcome = entry("new-id", JournalState::Committed);
    assert!(
        journal
            .append_if_unchanged_since(&missing, std::slice::from_ref(&first), &outcome)
            .expect("whole history")
    );
    assert_eq!(journal.read_latest_entry("new-id").expect("latest"), Some(outcome));
}

// journal.rs:693:29 guard -> true, :693:55 && -> ||, :693:37 < -> <=, :695:29 += -> *=.
// C#: JournalAppendRobustnessTests.Append_SharingViolationNeverReleased_ThrowsAfterBoundedAttempts.
// The delay seam asserts the retry bound immediately, so an endless retry fails instead of timing out.
#[test]
fn append_persistent_sharing_violation_never_requests_a_fifth_delay() {
    let root = TempRoot::new("jrnl5-retry");
    let (paths, clock, fs) = fixture(&root);
    fs.add_dir(paths.journal_file.parent().expect("journal parent"));
    fs.fail_locked(FsOp::OpenAppend, &paths.journal_file);
    let delays = Arc::new(AtomicUsize::new(0));
    let count = delays.clone();
    let journal = OperationJournal::with_options(
        &paths,
        fs.clone(),
        clock,
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(move |delay| {
                let index = count.fetch_add(1, Ordering::SeqCst);
                assert!(index < 4, "retry exceeded the four-delay bound");
                assert_eq!(delay, Duration::from_millis(10 * (index as u64 + 1)));
            })),
            ..OperationJournalOptions::default()
        },
    );
    assert!(journal.append(&entry("op", JournalState::Prepared)).is_err());
    assert_eq!(delays.load(Ordering::SeqCst), 4);
    assert_eq!(fs.count(FsOp::OpenAppend), 5);
    assert_eq!(fs.file_content(&paths.journal_file), None);
}

// journal.rs:470:21, :471:21, :472:21, :473:21 delete state/timestamp_utc/error/error_code.
// C#: OperationJournal.Dismiss: a Dismissed copy uses one current stamp and clears both error fields.
#[test]
fn dismiss_persists_dismissed_state_current_stamp_and_cleared_errors() {
    let root = TempRoot::new("jrnl5-dismiss");
    let (paths, clock, fs) = fixture(&root);
    let journal = OperationJournal::new(&paths, fs, clock);
    let failed = JournalEntry {
        timestamp_utc: JournalDateTime::utc(
            DateTimeUtc::from_ticks(638_942_999_999_999_990).expect("older fixed stamp"),
        ),
        error: Some("old error".to_owned()),
        error_code: Some(PENDING_UNCONFIRMED.to_owned()),
        ..entry("op", JournalState::Failed)
    };
    journal.append(&failed).expect("failed");
    let result = journal.dismiss(&[failed]).expect("dismiss");
    assert!(result.skipped.is_empty());
    let expected = entry("op", JournalState::Dismissed);
    assert_eq!(result.dismissed, vec![expected.clone()]);
    let mut written = Vec::new();
    journal
        .read_entries(&mut |line| written.push(line))
        .expect("written lines");
    assert_eq!(written.len(), 2);
    assert_eq!(written[1], expected);
}
