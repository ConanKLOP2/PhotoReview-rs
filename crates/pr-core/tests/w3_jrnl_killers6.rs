//! Journal mutation guards, with fake storage and a fixed clock.
//! Oracle: src/PhotoReview.Core/FileActions/OperationJournal.cs at C# 7eaee2a2.

use std::sync::Arc;

use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::OperationJournal;
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

// Mutants journal.rs:454:9 and :655:9 -> Ok(()); :658:16 delete !;
// :627:9 path_text -> empty or xyzzy (the required parent is then not created).
// C#: OperationJournalTests.Append_CreatesDirectoryAndWritesReadableEntries.
// An assertion on fake storage also detects :658 without the native writer-thread timeout.
#[test]
fn append_creates_parent_and_persists_the_exact_record_bytes() {
    let root = TempRoot::new("jrnl6-create");
    let (paths, clock, fs) = fixture(&root);
    let journal = OperationJournal::new(&paths, fs.clone(), clock);
    let record = entry("created", JournalState::Prepared);
    journal.append(&record).expect("append");
    assert_eq!(fs.count(FsOp::CreateDirectory), 1);
    assert_eq!(
        fs.file_content(&paths.journal_file),
        Some(format!("{}\r\n", record.to_line()).into_bytes())
    );
}

// Mutants journal.rs:704:9 -> true/false; :704:12 delete !; :709:19 == -> !=;
// :712:44 - -> + or /; :715:22 && -> ||; :715:17 == -> !=; :715:33 != -> ==.
// C#: OperationJournalTests.Append_AfterTornLastLine_KeepsTheNewEntry.
#[test]
fn append_repairs_only_a_nonempty_tail_without_a_final_lf() {
    for previous in [b"".as_slice(), b"\n", b"\r\n", b"torn", b"\r", b"x\ny"] {
        let root = TempRoot::new("jrnl6-tail");
        let (paths, clock, fs) = fixture(&root);
        fs.add_file(&paths.journal_file, previous);
        let journal = OperationJournal::new(&paths, fs.clone(), clock);
        let record = entry("after-tail", JournalState::Committed);
        journal.append(&record).expect("append after tail");
        let mut expected = previous.to_vec();
        if !previous.is_empty() && previous.last() != Some(&b'\n') {
            expected.extend_from_slice(b"\r\n");
        }
        expected.extend_from_slice(record.to_line().as_bytes());
        expected.extend_from_slice(b"\r\n");
        assert_eq!(
            fs.file_content(&paths.journal_file),
            Some(expected),
            "old tail {previous:?}"
        );
    }
}

// Mutants journal.rs:610:9 append_if -> Ok(true)/Ok(false); :632:9 read_lines_of -> empty;
// :634:25 ordinal Id == -> !=. C#: OperationJournal AppendIfStillPending/AppendIfLatestIs.
#[test]
fn append_if_passes_only_the_ids_raw_history_and_reports_whether_it_appended() {
    let root = TempRoot::new("jrnl6-conditional");
    let (paths, clock, fs) = fixture(&root);
    let journal = OperationJournal::new(&paths, fs.clone(), clock);
    let prepared = entry("op", JournalState::Prepared);
    let committed = entry("op", JournalState::Committed);
    journal.append(&prepared).expect("prepared");
    journal
        .append(&entry("OP", JournalState::Failed))
        .expect("other ordinal Id");
    let mut called = false;
    assert!(
        journal
            .append_if("op", &mut |lines| {
                called = true;
                assert_eq!(lines, std::slice::from_ref(&prepared));
                Some(committed.clone())
            })
            .expect("conditional append")
    );
    assert!(called, "decision callback must be evaluated");
    assert_eq!(
        journal.read_latest_entry("op").expect("latest"),
        Some(committed.clone())
    );
    let before = fs.file_content(&paths.journal_file);
    fs.reset_counts();
    let mut declined = false;
    assert!(
        !journal
            .append_if("op", &mut |lines| {
                declined = true;
                assert_eq!(lines, &[prepared.clone(), committed.clone()]);
                None
            })
            .expect("conditional decline")
    );
    assert!(declined, "decline callback must be evaluated");
    assert_eq!(fs.count(FsOp::OpenAppend), 0);
    assert_eq!(fs.file_content(&paths.journal_file), before);
}
