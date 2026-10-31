//! Deterministic assertion probes for the CX-PERF2 remaining mutation inventory.
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_core::app_paths::AppPaths;
use pr_core::diagnostics::perf::PerfValue;
use pr_core::diagnostics::perf_csv::{
    PerfCsvListener, build_row, csv_escape, current_thread_number, format_g17, format_value,
};
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::session::writer::{
    Completion, DebounceTimer, InvalidDebounce, MAX_DEBOUNCE_MILLISECONDS, RunningTimer, SessionWriter, TimerError,
};
use pr_core::sync::CancelToken;
use pr_test_support::{FakeFileSystem, TempRoot};

// perf_csv.rs:141:5 format_value -> empty/xyzzy; :202:5 csv_escape -> empty/xyzzy.
// C#: PerfCsvListenerMappingTests invariant value formatting and PerfCsvEscapeTests.
#[test]
fn value_and_csv_formatting_preserve_invariant_payloads() {
    for (value, expected) in [
        (PerfValue::Null, ""),
        (PerfValue::Bool(true), "1"),
        (PerfValue::Bool(false), "0"),
        (PerfValue::Int(-123), "-123"),
        (PerfValue::Double(12.5), "12.5"),
        (PerfValue::Str("text".into()), "text"),
    ] {
        assert_eq!(format_value(&value), expected);
    }
    assert_eq!(csv_escape(""), "");
    assert_eq!(csv_escape("a\r\nb\nc\rd"), "a b c d");
    assert_eq!(csv_escape("a,\"b\""), "\"a,\"\"b\"\"\"");
}

// perf_csv.rs:153:5 replacement; :157:26 > replacements; :160:14 == -> !=;
// :169:61 != -> ==; :170:24/28 > and && replacements; :173:19/21 delete !/-;
// :174:36 > replacements; :186:17 >= -> <; :187:34 + -> -/*; :188:25 <= -> >.
// C#: PerfCsvListener G17 mapping; exact decimal inputs avoid fixture clocks and locale.
#[test]
fn g17_preserves_sign_digits_and_fixed_or_scientific_placement() {
    for (value, expected) in [
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
        (0.0, "0"),
        (-0.0, "-0"),
        (1.0, "1"),
        (-1.0, "-1"),
        (12.5, "12.5"),
        (0.125, "0.125"),
        (0.0001, "0.0001"),
        (1e16, "10000000000000000"),
        (1e17, "1E+17"),
        (-1e17, "-1E+17"),
        (1e-5, "1.0000000000000001E-05"),
        (1.25e20, "1.25E+20"),
    ] {
        assert_eq!(format_g17(value), expected, "input {value:?}");
    }
}

// perf_csv.rs:220:5 -> 0/1/-1 and :225:23 == -> !=.
// C#: CurrentManagedThreadId mapping; positive distinct identities stable for each thread.
#[test]
fn thread_numbers_are_positive_stable_and_distinct_for_live_threads() {
    let here = current_thread_number();
    assert!(here > 0);
    assert_eq!(current_thread_number(), here);
    let other = std::thread::spawn(|| {
        let first = current_thread_number();
        assert!(first > 0);
        assert_eq!(current_thread_number(), first);
        first
    })
    .join()
    .unwrap();
    assert_ne!(here, other);
}

// writer.rs:142:9 completed -> Default; :161:9 is_complete -> true/false;
// :174:9 wait_timeout -> true/false; :176:15 delete !.
// C#: Task.CompletedTask / SessionWriter.WhenIdleAsync bridge. Zero budget is deterministic.
#[test]
fn completion_reports_pending_and_completed_without_a_blocking_wait() {
    let completion = Completion::new();
    assert!(!completion.is_complete());
    assert!(!completion.wait_timeout(Duration::ZERO));
    completion.complete();
    assert!(completion.is_complete());
    assert!(completion.wait_timeout(Duration::ZERO));
    let ready = Completion::completed();
    assert!(ready.is_complete());
    assert!(ready.wait_timeout(Duration::ZERO));
}

#[derive(Clone)]
struct MemorySink(Arc<Mutex<Vec<u8>>>);

impl Write for MemorySink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// perf_csv.rs:241:9 Debug -> Ok(Default); :273:9 signal_finished -> (); :279:9 count_dropped -> ();
// :284:9 enqueue -> (); :284:12 delete !; :317:5 diag_field replacements / :319:50 delete !;
// :325:5 write_row -> Ok(()); :597:9 enqueue -> (); :602:9 dropped_count -> 0/1/-1;
// :607:9 writer_faulted -> true; :628:9 wait_finished -> false; :631:60 delete !.
// C#: PerfCsvListenerMappingTests / PerfCsvListenerRobustnessTests: drain rows and account closed-queue drops.
#[test]
fn listener_drains_rows_and_accounts_each_post_dispose_drop() {
    let sink = MemorySink(Arc::new(Mutex::new(Vec::new())));
    let diag = |_: &str| Some("enabled".to_owned());
    let listener = PerfCsvListener::start_for_test(Box::new(sink.clone()), 16, None, None, Some("perf2"), &diag);
    assert_eq!(format!("{listener:?}"), "PerfCsvListener { .. }");
    assert_eq!(listener.dropped_count(), 0);
    assert!(!listener.writer_faulted());
    listener.enqueue(|| build_row("Perf2Row", None, None));
    listener.dispose();
    assert!(listener.wait_finished(Duration::ZERO));
    let output = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert!(output.contains("Perf2Row"), "{output}");
    assert!(output.contains("=enabled"), "{output}");
    assert!(output.contains("# dropped=0"), "{output}");
    listener.enqueue(|| build_row("closed1", None, None));
    listener.enqueue(|| build_row("closed2", None, None));
    assert_eq!(listener.dropped_count(), 2);
    assert!(!listener.writer_faulted());
}

struct FailingSink;

impl Write for FailingSink {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("perf2 controlled sink failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// perf_csv.rs:279:9 count_dropped -> (); :379:27 += -> -=/*=;
// :607:9 writer_faulted -> false. Large row forces I/O failure before disconnect flush.
// C#: PerfCsvListenerFaultTests: failed buffered row plus queued rows are all lost.
#[test]
fn failing_sink_counts_the_failed_row_and_every_queued_row() {
    let listener = PerfCsvListener::start_for_test(Box::new(FailingSink), 16, None, None, None, &|_| None);
    for _ in 0..3 {
        listener.enqueue(|| build_row("Fault", Some(&["text"]), Some(&[PerfValue::Str("x".repeat(20_000))])));
    }
    listener.dispose();
    assert!(listener.wait_finished(Duration::ZERO));
    assert!(listener.writer_faulted());
    assert_eq!(listener.dropped_count(), 3);
}

// perf_csv.rs:639:9 Drop::drop -> ().
// C#: PerfCsvListener.Dispose; Rust Drop must close the queue and write its trailer before returning.
#[test]
fn dropping_a_listener_drains_its_row_and_writes_the_trailer() {
    let sink = MemorySink(Arc::new(Mutex::new(Vec::new())));
    let listener = PerfCsvListener::start_for_test(Box::new(sink.clone()), 16, None, None, None, &|_| None);
    listener.enqueue(|| build_row("DropPerf2Row", None, None));
    drop(listener);
    let output = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert!(output.contains("DropPerf2Row"), "{output}");
    assert!(output.contains("# dropped=0"), "{output}");
}

// writer.rs:46:9 InvalidDebounce::fmt -> Ok(Default); :324:9 Debug -> Ok(Default).
// C#: SessionWriterTests constructor validation; Rust formatting must name the rejected duration.
#[test]
fn writer_debounce_boundaries_and_diagnostics_preserve_the_rejected_value() {
    let root = TempRoot::new("w2-perf2-debounce");
    let paths = AppPaths::new(root.path(), None, true);
    let store = Arc::new(SessionStore::new(&paths, Arc::new(FakeFileSystem::new()), None));
    assert!(SessionWriter::new(Arc::clone(&store), None, Some(Duration::ZERO), None).is_err());
    assert!(
        SessionWriter::new(
            Arc::clone(&store),
            None,
            Some(Duration::from_millis(MAX_DEBOUNCE_MILLISECONDS + 1)),
            None
        )
        .is_err()
    );
    for span in [
        Duration::from_millis(1),
        Duration::from_millis(MAX_DEBOUNCE_MILLISECONDS),
    ] {
        let writer = SessionWriter::new(Arc::clone(&store), None, Some(span), None).unwrap();
        let debug = format!("{writer:?}");
        assert!(debug.contains("SessionWriter") && debug.contains("debounce"), "{debug}");
    }
    assert!(
        InvalidDebounce(Duration::from_millis(123))
            .to_string()
            .contains("123ms")
    );
}

struct FailedTimer;

impl DebounceTimer for FailedTimer {
    fn start(&self, _: Duration, _: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        Err(Box::new(io::Error::other("perf2 controlled timer startup failure")))
    }
}

// writer.rs:680:9 SessionWriter::update -> Ok(()); :686:9 SessionWriter::flush -> ();
// :344:5 spawn_save_thread -> Ok(Default). Failed timer leaves pending state with no live waiter.
// C#: SessionWriterTests.FlushWritesImmediatelyAndOnlyOnce / DisposeFlushes.
#[test]
fn explicit_flush_persists_pending_state_without_a_live_timer() {
    let root = TempRoot::new("w2-perf2-explicit-flush");
    let paths = AppPaths::new(root.path(), None, true);
    let store = Arc::new(SessionStore::new(&paths, Arc::new(FakeFileSystem::new()), None));
    let writer = SessionWriter::new(Arc::clone(&store), None, None, Some(Arc::new(FailedTimer))).unwrap();
    let snapshot = SessionState {
        folder: root.join("photos").to_string_lossy().into_owned(),
        current_path: Some("latest.jpg".into()),
        ..SessionState::default()
    };
    writer.update(&snapshot).unwrap();
    assert_eq!(store.load(&snapshot.folder).unwrap().current_path, None);
    writer.flush();
    assert_eq!(
        store.load(&snapshot.folder).unwrap().current_path,
        snapshot.current_path
    );
    writer.dispose();
}
