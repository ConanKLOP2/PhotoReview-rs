//! Assertion killers for the remaining W2 perf CSV/session writer mutation debt.
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_core::app_paths::AppPaths;
use pr_core::diagnostics::perf::PerfValue;
use pr_core::diagnostics::perf_csv::{PerfCsvListener, build_row};
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::session::writer::{Completion, DebounceTimer, RunningTimer, SessionWriter, TimerError};
use pr_core::sync::CancelToken;
use pr_test_support::{FakeFileSystem, TempRoot};

struct FailedTimer;

// perf_csv.rs:115:31 numeric_count += -> -= (underflow) / *= (stuck at zero).
// C#: PerfCsvListenerMappingTests.BuildRow_MoreThanFourNumerics_AreCappedAtFour.
// Direct formatting avoids background-sink waits when a mutant panics during payload construction.
#[test]
fn numeric_payload_advances_each_column_and_discards_the_fifth_value() {
    let row = build_row(
        "Evt",
        Some(&["one", "two", "three", "four", "five"]),
        Some(&[
            PerfValue::Int(11),
            PerfValue::Int(22),
            PerfValue::Int(33),
            PerfValue::Int(44),
            PerfValue::Int(55),
        ]),
    );
    assert_eq!([row.a.as_str(), &row.b, &row.c, &row.d], ["11", "22", "33", "44"]);
    assert!(row.text.is_empty());
}

// writer.rs:153:9 Completion::complete -> (); :142:9 completed -> Default::default();
// :161:9 is_complete -> true/false; :174:9 wait_timeout -> true/false; :176:15 delete !.
// Rust completion bridge for C# Task.CompletedTask; unlike a blocking waiter, this asserts the state directly.
#[test]
fn completion_state_and_zero_budget_wait_agree_before_and_after_complete() {
    let completion = Completion::new();
    assert!(!completion.is_complete());
    assert!(!completion.wait_timeout(Duration::ZERO));
    completion.complete();
    assert!(completion.is_complete());
    assert!(completion.wait_timeout(Duration::ZERO));
    completion.complete();
    assert!(completion.is_complete());
    assert!(Completion::completed().is_complete());
}

impl DebounceTimer for FailedTimer {
    fn start(&self, _: Duration, _: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        Err(Box::new(io::Error::other("controlled timer failure")))
    }
}

fn fixture() -> (TempRoot, Arc<SessionStore>, SessionWriter) {
    let root = TempRoot::new("w2-perfcsv-writer");
    let paths = AppPaths::new(root.path(), None, true);
    let store = Arc::new(SessionStore::new(&paths, Arc::new(FakeFileSystem::new()), None));
    let writer = SessionWriter::new(Arc::clone(&store), None, None, Some(Arc::new(FailedTimer))).unwrap();
    (root, store, writer)
}

fn state(root: &TempRoot, current: &str) -> SessionState {
    SessionState {
        folder: root.join("photos").to_string_lossy().into_owned(),
        current_path: Some(current.into()),
        ..SessionState::default()
    }
}

// writer.rs:602:9 Shared::dispose -> (); :709:9 SessionWriter::dispose -> ().
// Also writer.rs:384:9 Shared::update -> Ok(()), :470:9 Shared::flush -> (),
// :479:9 Shared::write_pending -> (), :507:9 Shared::write_batch -> ().
// writer.rs:569:9 Shared::save_batch -> (); :571:46 != -> == in Shared::save_batch.
// C#: SessionWriterTests.Dispose_Flushes / Update_AfterDispose_IsIgnored.
// Failed start leaves pending data without a timer thread: deletion fails an assertion, never hangs a waiter.
#[test]
fn dispose_persists_pending_state_and_rejects_later_updates_without_a_live_timer() {
    let (root, store, writer) = fixture();
    let first = state(&root, "first.jpg");
    writer.update(&first).unwrap();
    writer.dispose();
    assert_eq!(store.load(&first.folder).unwrap().current_path, first.current_path);
    writer.update(&state(&root, "later.jpg")).unwrap();
    writer.flush();
    assert_eq!(store.load(&first.folder).unwrap().current_path, first.current_path);
}

// writer.rs:743:9 Drop::drop -> (); also exercises the normal save/flush composition.
// C#: SessionWriterTests.Dispose_Flushes (Rust Drop is the IDisposable bridge).
#[test]
fn drop_persists_pending_state_after_a_failed_timer_start() {
    let (root, store, writer) = fixture();
    let pending = state(&root, "drop.jpg");
    writer.update(&pending).unwrap();
    drop(writer);
    assert_eq!(store.load(&pending.folder).unwrap().current_path, pending.current_path);
}

// writer.rs:594:95 == -> != in Shared::requeue_skipped (the second key must not match the first).
// C#: SessionWriterTests.Flush_SaveWorkerCannotBeStarted_ReleasesTheWriterLockAndTheStateIsWrittenByTheNextFlush,
// extended to two folders, as in the previous W2 requeue killer, using a failed worker instead of timer contention.
#[test]
fn failed_save_worker_requeues_both_folders_for_the_next_flush() {
    let (root, store, writer) = fixture();
    let first = state(&root, "one.jpg");
    let mut second = state(&root, "two.jpg");
    second.folder = root.join("other-photos").to_string_lossy().into_owned();
    writer.update(&first).unwrap();
    writer.update(&second).unwrap();
    writer.set_save_spawner_for_tests(Some(Box::new(|_| Err(io::Error::other("controlled worker failure")))));
    writer.flush();
    writer.set_save_spawner_for_tests(None);
    writer.flush();
    assert_eq!(store.load(&first.folder).unwrap().current_path, first.current_path);
    assert_eq!(store.load(&second.folder).unwrap().current_path, second.current_path);
}

#[derive(Clone)]
struct ChunkSink(Arc<Mutex<Vec<usize>>>);

impl Write for ChunkSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().push(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// perf_csv.rs:37:32 WRITER_BUFFER * -> / (zero) and * -> + (1032).
// C#: PerfCsvListenerMappingTests: buffered stream. No blocked sink and no idle-clock assertion.
#[test]
fn row_chunks_require_the_eight_kib_buffer_even_when_capacity_mutates_to_zero() {
    let chunks = Arc::new(Mutex::new(Vec::new()));
    let listener = PerfCsvListener::start_for_test(
        Box::new(ChunkSink(Arc::clone(&chunks))),
        1024,
        Some(Duration::from_secs(3600)),
        None,
        None,
        &|_| None,
    );
    for _ in 0..600 {
        listener.enqueue(|| build_row("Evt", None, None));
    }
    listener.dispose();
    let chunks = chunks.lock().unwrap();
    let largest = chunks.iter().copied().max().unwrap_or(0);
    assert!(largest > 2000 && largest <= 8192, "unexpected chunks: {chunks:?}");
    assert_eq!(listener.dropped_count(), 0);
}
