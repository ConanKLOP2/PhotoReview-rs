//! Mutation killers (part 3) for `file_actions/journal.rs` (append, torn tail, readers, reverse tail reader,
//! `append_if*`; W3-01 mutation debt, `docs/reviews/W3-01-JOURNAL-MUTATION.md`). Each test names the mutant it kills.
//! `FakeFileSystem` only: no real disk, no Recycle Bin, no sleeps.

use std::sync::Arc;

use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{OperationJournal, REVERSE_READ_INITIAL_WINDOW};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
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

#[allow(dead_code)]
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

#[allow(dead_code)]
fn rig() -> (Arc<FakeClock>, Arc<FakeFileSystem>, OperationJournal) {
    let clock = Arc::new(FakeClock::at(base()));
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    let journal = OperationJournal::new(&fake_paths(), fs.clone(), clock.clone());
    (clock, fs, journal)
}

// journal.rs:360:9 replace <impl fmt::Debug for OperationJournal>::fmt -> fmt::Result with Ok(Default::default())
#[test]
fn operation_journal_debug_names_the_type_the_path_and_the_compaction_flag() {
    let (_clock, _fs, journal) = rig();
    let text = format!("{journal:?}");
    assert!(text.contains("OperationJournal"), "{text}");
    assert!(text.contains("operations.jsonl"), "{text}");
    assert!(text.contains("compaction: false"), "{text}");
}

fn seed_journal(fs: &FakeFileSystem, lines: &[String]) {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push_str("\r\n");
    }
    fs.add_file(fake_paths().journal_file, text.as_bytes());
}

// Regression test for the window widening of read_committed_moves_reverse (journal.rs:899..924), not a mutant killer:
// the survivor journal.rs:914:48 `first_newline + 1` -> `first_newline * 1` is EQUIVALENT (see the W3-01 report): the
// chunk then starts at the line feed itself, which only adds an empty first line, and the next boundary ends before that
// line feed, where the last piece of a chunk is parsed without a terminator anyway. The `-` variant of the same line is
// killed by `file_actions_journal` (a move straddling a window edge). Here: a journal over 1 MiB whose five committed
// moves sit at the head behind filler needs several widening passes and must still return all five.
#[test]
fn read_committed_moves_big_journal_with_moves_only_at_the_head_widens_until_it_finds_them() {
    let (_clock, fs, journal) = rig();
    let mut lines: Vec<String> = Vec::new();
    for i in 0..5 {
        lines.push(entry(&format!("mv{i}"), JournalState::Committed, FileOperationType::Move).to_line());
    }
    let filler = entry("filler", JournalState::Prepared, FileOperationType::Copy).to_line();
    let mut total: usize = lines.iter().map(|l| l.len() + 2).sum();
    while total < 1_100_000 {
        total += filler.len() + 2;
        lines.push(filler.clone());
    }
    seed_journal(&fs, &lines);
    let moves = journal.read_committed_moves().expect("read committed moves");
    let ids: Vec<&str> = moves.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["mv0", "mv1", "mv2", "mv3", "mv4"]);
}

// journal.rs:58:50 replace * with + in REVERSE_READ_INITIAL_WINDOW (256 * 1024 -> 1280; `/` gives 0 and hangs). The window
// only sizes the first reverse read, so the result is the same for any window: the constant is pinned (C# 256 * 1024).
#[test]
fn reverse_read_initial_window_is_256_kib() {
    assert_eq!(REVERSE_READ_INITIAL_WINDOW, 262_144);
}
