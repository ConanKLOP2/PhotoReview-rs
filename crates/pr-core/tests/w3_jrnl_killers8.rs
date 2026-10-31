//! Journal gate and constant guards (CL-JRNL4): fail fast, never hang.
//! Oracle: src/PhotoReview.Core/FileActions/OperationJournal.cs at C# 7eaee2a2 (`lock (_gate)`, `ReverseReadInitialWindow`).
//!
//! The reentrant gate is private, so it is observed through the public API on worker threads. The test thread waits
//! on a channel with a deadline: when the gate deadlocks (a mutant), the test FAILS after `DEADLINE` instead of
//! hanging; the stuck worker is abandoned and dies with the test process. The deadline is an upper bound, not a delay.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use pr_core::app_paths::AppPaths;
use pr_core::file_actions::journal::{OperationJournal, REVERSE_READ_INITIAL_WINDOW, decode_stream_reader_lines};
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeClock, FakeFileSystem};

const DEADLINE: Duration = Duration::from_secs(2);

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

// Fake storage under a fake path: nothing touches the disk, so an abandoned (deadlocked) worker leaks nothing.
fn journal() -> Arc<OperationJournal> {
    let paths = AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false);
    let clock = Arc::new(FakeClock::at(
        DateTimeUtc::from_ticks(638_943_000_000_000_000).expect("fixed stamp"),
    ));
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    Arc::new(OperationJournal::new(&paths, fs, clock))
}

// journal.rs:58:50 `256 * 1024` -> `256 / 1024` (window 0: the reverse reader never advances; the old run timed out).
// C#: `ReverseReadInitialWindow = 256L * 1024`.
#[test]
fn reverse_read_initial_window_is_256_kib() {
    assert_eq!(REVERSE_READ_INITIAL_WINDOW, 262_144);
}

// journal.rs:312:32 guard `owner == me` -> false and :312:38 `==` -> `!=` (a same-thread re-lock waits on itself).
// C#: `lock (_gate)` is a reentrant monitor; Dismiss/ReadEntries call back into the journal on the same thread.
#[test]
fn same_thread_reentry_under_the_gate_completes_instead_of_deadlocking() {
    let journal = journal();
    journal.append(&entry("op", JournalState::Prepared)).expect("prepared");
    let (tx, rx) = mpsc::channel();
    let worker = Arc::clone(&journal);
    std::thread::spawn(move || {
        let mut inner = Vec::new();
        worker
            .read_entries(&mut |e| {
                // Runs with the gate held: this nested lock is the reentrant path.
                inner.push(worker.read_latest_entry(&e.id).expect("nested read"));
            })
            .expect("outer read");
        let _ = tx.send(inner);
    });
    let inner = rx
        .recv_timeout(DEADLINE)
        .expect("reentrant gate deadlocked: nested lock on the owning thread did not complete");
    assert_eq!(inner.len(), 1);
    assert_eq!(inner[0].as_ref().map(|e| e.id.as_str()), Some("op"));
}

// journal.rs:327:9 `GateGuard::drop` -> () (the gate is never released, the next thread waits forever).
// C#: leaving `lock (_gate)` releases the monitor.
#[test]
fn gate_is_released_when_the_guard_drops_so_another_thread_can_enter() {
    let journal = journal();
    let first = Arc::clone(&journal);
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = first.append(&entry("one", JournalState::Prepared));
        let _ = done_tx.send(result.is_ok());
    });
    assert!(
        done_rx.recv_timeout(DEADLINE).expect("first append did not finish"),
        "first append failed"
    );
    let (tx, rx) = mpsc::channel();
    let second = Arc::clone(&journal);
    std::thread::spawn(move || {
        let appended = second.append(&entry("two", JournalState::Prepared)).is_ok();
        let read_back = second.read_latest_entry("two").map(|e| e.is_some()).unwrap_or(false);
        let _ = tx.send((appended, read_back));
    });
    // The test thread never touches the journal itself: under a broken gate that call would block with no deadline.
    let (appended, read_back) = rx
        .recv_timeout(DEADLINE)
        .expect("gate never released: a second thread could not enter after the first one finished");
    assert!(appended);
    assert!(read_back);
}

// ---------------------------------------------------------------------------------------------------------------
// Reverse tail reader (journal.rs 914:48, 924:60, 938:11, 940:73) and reconcile delegation (530:9).
// The reverse reader and the committed-move parser loop; a mutated loop may never end, so the work runs on a worker
// thread and the test thread waits with a deadline (a hang becomes a failure, never a stuck run).
// ---------------------------------------------------------------------------------------------------------------

fn journal_path() -> std::path::PathBuf {
    AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false).journal_file
}

fn fake_with_file(bytes: &[u8]) -> Arc<OperationJournal> {
    let paths = AppPaths::new(Path::new(r"C:\Users\test\AppData\Local"), None, false);
    let clock = Arc::new(FakeClock::at(
        DateTimeUtc::from_ticks(638_943_000_000_000_000).expect("fixed stamp"),
    ));
    let fs = Arc::new(FakeFileSystem::with_clock(clock.clone()));
    fs.add_file(journal_path(), bytes);
    Arc::new(OperationJournal::new(&paths, fs, clock))
}

fn move_line(id: &str) -> Vec<u8> {
    let line = JournalEntry {
        size: 4,
        destination: Some(format!(r"C:\sel\{id}.jpg")),
        source: format!(r"C:\photos\{id}.jpg"),
        ..entry(id, JournalState::Committed)
    };
    format!("{}\r\n", line.to_line()).into_bytes()
}

fn recycle_line(i: usize) -> Vec<u8> {
    let line = JournalEntry {
        op_type: FileOperationType::Recycle,
        destination: None,
        source: format!(r"C:\photos\r{i}.jpg"),
        ..entry(&format!("r{i}"), JournalState::Committed)
    };
    format!("{}\r\n", line.to_line()).into_bytes()
}

fn committed_ids_within_deadline(journal: Arc<OperationJournal>) -> Vec<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let ids: Vec<String> = journal
            .read_committed_moves()
            .expect("read committed moves")
            .into_iter()
            .map(|e| e.id)
            .collect();
        let _ = tx.send(ids);
    });
    rx.recv_timeout(DEADLINE)
        .expect("read_committed_moves did not finish: the reverse tail reader or the line parser never ends")
}

// journal.rs:924:60 `||` -> `&&`, 938:11 delete `!`, 940:73 `newline + 1` -> `- 1` / `* 1`: a journal of exactly 1 MiB
// with fewer than 200 moves (one pass, the pad is parsed as a malformed line) returns every move once, in file order.
// C#: ReadCommittedMovesReverse / ParseCommittedMoves.
#[test]
fn reverse_reader_returns_every_move_of_a_big_journal_with_few_moves_once_and_in_order() {
    let moves: Vec<Vec<u8>> = (0..100).map(|i| move_line(&format!("m{i:04}"))).collect();
    let rest: Vec<u8> = moves.concat();
    let pad = 1024 * 1024 + 5000 - rest.len();
    let mut bytes = vec![b'x'; pad];
    bytes[pad - 2] = b'\r';
    bytes[pad - 1] = b'\n';
    bytes.extend_from_slice(&rest);
    let ids = committed_ids_within_deadline(fake_with_file(&bytes));
    let expected: Vec<String> = (0..100).map(|i| format!("m{i:04}")).collect();
    assert_eq!(ids, expected);
}

// journal.rs:914:48 `start + line_start` -> `start * line_start` (the next boundary): a journal read in several windows
// (2 MiB of Recycle lines with a Move every 400th line: fewer than 200 moves, so the reader widens 256 KiB, 512 KiB,
// 1 MiB, 2 MiB) returns each move once, in order.
#[test]
fn reverse_reader_widening_windows_keep_the_boundary_on_a_line_start() {
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    let mut i = 0usize;
    while bytes.len() < 2 * 1024 * 1024 {
        if i % 400 == 0 {
            let id = format!("m{i:05}");
            bytes.extend(move_line(&id));
            expected.push(id);
        } else {
            bytes.extend(recycle_line(i));
        }
        i += 1;
    }
    assert!(expected.len() < 200 && expected.len() > 10);
    let ids = committed_ids_within_deadline(fake_with_file(&bytes));
    assert_eq!(ids, expected);
}

// journal.rs:530:9 `reconcile_pending_operations -> Ok(vec![])`: a Prepared move whose source and destination are both
// missing is judged and the outcome line is returned to the caller.
// C#: OperationJournal.ReconcilePendingOperations delegates to the reconcile pass and returns its outcomes.
#[test]
fn reconcile_pending_operations_returns_the_outcomes_it_appended() {
    let journal = fake_with_file(&[]);
    journal
        .append(&entry("pending", JournalState::Prepared))
        .expect("prepared");
    let (tx, rx) = mpsc::channel();
    let worker = Arc::clone(&journal);
    std::thread::spawn(move || {
        let outcomes = worker.reconcile_pending_operations(None).expect("reconcile");
        let _ = tx.send(outcomes.into_iter().map(|e| (e.id, e.state)).collect::<Vec<_>>());
    });
    let outcomes = rx.recv_timeout(DEADLINE).expect("reconcile did not finish");
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert_eq!(outcomes[0].0, "pending");
    assert_ne!(outcomes[0].1, JournalState::Prepared);
}

// journal.rs:914:48 `first_newline + 1` -> `first_newline - 1`: with bare-LF lines (what the torn-tail repair leaves
// between records), a Move line that straddles the first window start must be read whole by the second pass. The
// mutant moves the pass boundary one byte into that line and drops its last character, so the move is lost.
// (`first_newline * 1` is equivalent: the boundary then sits ON the line's `\n`, which the parser never needs; see
// docs/reviews/CL-JRNL4-2026-10-31.md.)
// C#: ReadCommittedMovesReverse (boundary always on a line start).
#[test]
fn reverse_reader_reads_a_move_that_straddles_the_first_window_start_in_a_bare_lf_journal() {
    let mut move_text = move_line("straddler");
    assert_eq!(move_text.pop(), Some(b'\n'));
    assert_eq!(move_text.pop(), Some(b'\r'));
    move_text.push(b'\n'); // bare LF
    assert!(move_text.len() > 100);
    // `start = length - 256 KiB` must fall 100 bytes before the end of the Move line.
    let tail_len = 256 * 1024 - 100;
    let mut tail = vec![b'x'; tail_len];
    tail[tail_len - 1] = b'\n';
    let mut bytes = b"x\n".repeat(500_000);
    bytes.extend_from_slice(&move_text);
    bytes.extend_from_slice(&tail);
    assert!(bytes.len() as u64 >= 1024 * 1024);
    assert_eq!(bytes.len() - 256 * 1024, bytes.len() - tail_len - 100);
    let ids = committed_ids_within_deadline(fake_with_file(&bytes));
    assert_eq!(ids, vec!["straddler".to_owned()]);
}

// journal.rs:999:33 `at + terminator` -> `at - terminator` (the cursor goes backwards: a leading terminator underflows, any
// other line re-reads itself forever, which is what the old full-suite run timed out on).
// C#: StreamReader.ReadLine splits at "\r\n", "\n" and a lone "\r"; empty lines are kept.
#[test]
fn decode_stream_reader_lines_advances_past_every_terminator() {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(decode_stream_reader_lines(b"\r\nx\n\ny\rz\r\n"));
    });
    let lines = rx
        .recv_timeout(DEADLINE)
        .expect("decode_stream_reader_lines panicked or never finished");
    assert_eq!(lines, ["", "x", "", "y", "z"]);
}
