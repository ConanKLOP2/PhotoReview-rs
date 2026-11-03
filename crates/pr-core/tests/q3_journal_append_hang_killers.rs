//! F-13 (Q3 2026-11-04): fail-fast killers for the four `OperationJournal::append` mutants that ended as cargo-mutants TIMEOUTs
//! (`journal.rs:665:16` delete `!`, `:700:29` guard -> true, `:700:55` `&&` -> `||`, `:702:29` `+=` -> `*=`). Each mutant makes the append
//! retry loop (or the folder step) spin or wait forever; here the retry-delay seam (`OperationJournalOptions::append_retry_delay`) panics as
//! soon as the loop exceeds the C# bound, so the hang becomes a prompt test failure. Fake file system, fake path, no real disk, no sleep.
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_core::abstractions::FileSystem;
use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{APPEND_ATTEMPTS, OperationJournal, OperationJournalOptions};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, FakeFileSystem, FsOp};

fn stamp() -> DateTimeUtc {
    DateTimeUtc::parse_json("2026-09-25T00:00:00Z").expect("stamp parses").0
}

fn paths() -> AppPaths {
    AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false)
}

fn entry() -> JournalEntry {
    JournalEntry {
        id: "a".to_owned(),
        op_type: FileOperationType::Move,
        state: JournalState::Prepared,
        source: r"C:\photos\a.jpg".to_owned(),
        destination: Some(r"C:\photos\sel\a.jpg".to_owned()),
        size: 1,
        last_write_utc: JournalDateTime::utc(stamp()),
        timestamp_utc: JournalDateTime::utc(stamp()),
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: None,
        group_members: None,
    }
}

/// A journal whose retry delay panics once it has been asked for more than `limit` delays, and records each delay.
fn journal_with_delay_cap(fs: &Arc<FakeFileSystem>, limit: usize) -> (OperationJournal, Arc<Mutex<Vec<Duration>>>) {
    let clock = Arc::new(FakeClock::at(stamp()));
    let delays: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = delays.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let journal = OperationJournal::with_options(
        &paths(),
        fs.clone(),
        clock,
        OperationJournalOptions {
            append_retry_delay: Some(Arc::new(move |delay| {
                let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                assert!(n <= limit, "append retried {n} times: the retry loop is not bounded");
                recorder.lock().expect("delays").push(delay);
            })),
            ..OperationJournalOptions::default()
        },
    );
    (journal, delays)
}

// journal.rs:700:55 (`&&` -> `||`: a sharing violation retried without the attempt bound) and :702:29 (`attempt += 1` -> `*= 1`: the
// attempt number never grows, so the bound is never reached). C#: Append_SharingViolationNeverReleased_ThrowsAfterBoundedAttempts.
#[test]
fn a_sharing_violation_that_never_clears_is_retried_exactly_four_times_then_fails() {
    let fs = Arc::new(FakeFileSystem::with_clock(Arc::new(FakeClock::at(stamp()))));
    let path = paths().journal_file;
    fs.add_dir(path.parent().expect("parent"));
    fs.fail_locked(FsOp::OpenAppend, &path);
    let (journal, delays) = journal_with_delay_cap(&fs, (APPEND_ATTEMPTS - 1) as usize);

    assert!(journal.append(&entry()).is_err());

    assert_eq!(
        *delays.lock().expect("delays"),
        [10, 20, 30, 40].map(Duration::from_millis).to_vec()
    );
    assert_eq!(fs.count(FsOp::OpenAppend), APPEND_ATTEMPTS as usize);
}

// journal.rs:700:29 (match guard -> true: every error is retried). C#: Append_OtherIoError_IsNotRetried.
#[test]
fn an_error_that_is_not_a_sharing_violation_is_never_retried() {
    let fs = Arc::new(FakeFileSystem::with_clock(Arc::new(FakeClock::at(stamp()))));
    let path = paths().journal_file;
    fs.add_dir(path.parent().expect("parent"));
    fs.fail_times(FsOp::OpenAppend, &path, io::ErrorKind::Other, 1000);
    let (journal, delays) = journal_with_delay_cap(&fs, 0);

    assert!(journal.append(&entry()).is_err());

    assert!(delays.lock().expect("delays").is_empty());
    assert_eq!(fs.count(FsOp::OpenAppend), 1);
}

// journal.rs:665:16 (`if !dir.is_empty()` -> `if dir.is_empty()`: the journal folder is not created before the first open).
// C#: AppendLines creates the folder on every append.
#[test]
fn append_creates_the_journal_folder_when_it_does_not_exist() {
    let fs = Arc::new(FakeFileSystem::with_clock(Arc::new(FakeClock::at(stamp()))));
    let path = paths().journal_file;
    let (journal, delays) = journal_with_delay_cap(&fs, 0);

    journal
        .append(&entry())
        .expect("append creates the folder and the file");

    assert!(fs.directory_exists(path.parent().expect("parent")));
    assert!(fs.file_content(&path).is_some_and(|bytes| !bytes.is_empty()));
    assert!(delays.lock().expect("delays").is_empty());
}
