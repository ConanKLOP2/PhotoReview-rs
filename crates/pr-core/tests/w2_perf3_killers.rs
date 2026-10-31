//! Fail-fast killers for the CX-PERF2 timeout mutants in `diagnostics/perf_csv.rs` (lane st-perfkill).
//!
//! Every wait here is a bounded condvar wait (`wait_finished(BOUND)`); nothing sleeps, polls or reads a clock, so a
//! mutant that removes the finish signal, the queueing or the panic guard fails an assertion instead of hanging.
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_core::diagnostics::perf::PerfValue;
use pr_core::diagnostics::perf_csv::{PerfCsvListener, build_row};

/// Upper bound for a wait that returns at once when the code is correct (the writer thread finishes in microseconds).
const BOUND: Duration = Duration::from_secs(5);

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

struct PanickingSink;

impl Write for PanickingSink {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("perf3 controlled sink panic");
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// perf_csv.rs:273:9 Shared::signal_finished -> () (timeout); :284:9 Shared::enqueue -> () and :284:12 delete !
// (timeouts: rows never reach the sink, so waiting tests never saw them).
// C#: PerfCsvListenerMappingTests (row reaches the stream) / PerfCsvListenerRobustnessTests (Dispose finishes).
// dispose_wait ZERO makes `dispose` return at once; only the bounded `wait_finished` can observe the signal.
#[test]
fn finish_signal_and_queued_row_are_observable_within_a_bound() {
    let sink = MemorySink(Arc::new(Mutex::new(Vec::new())));
    let listener = PerfCsvListener::start_for_test(
        Box::new(sink.clone()),
        16,
        None,
        Some(Duration::ZERO),
        Some("perf3"),
        &|_| None,
    );
    listener.enqueue(|| build_row("Perf3Row", Some(&["text"]), Some(&[PerfValue::Str("payload".into())])));
    listener.dispose();
    assert!(
        listener.wait_finished(BOUND),
        "the writer thread must signal that it finished"
    );
    let output = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert!(output.starts_with("# commit=perf3 diag= qpcFrequency="), "{output}");
    assert!(output.contains(",Perf3Row,,,,,,,payload\n"), "{output}");
    assert!(output.ends_with("# dropped=0\r\n"), "{output}");
    assert_eq!(listener.dropped_count(), 0);
    assert!(!listener.writer_faulted());
}

// A never-started listener has no writer to wait for and counts nothing (the `finished` flag starts true).
// C#: PerfCsvListenerRobustnessTests (new PerfCsvListener() ignores Enqueue).
#[test]
fn unstarted_listener_is_already_finished_and_counts_no_rows() {
    let listener = PerfCsvListener::unstarted();
    listener.enqueue(|| build_row("Ignored", None, None));
    assert!(listener.wait_finished(Duration::ZERO));
    assert_eq!(listener.dropped_count(), 0);
    assert!(!listener.writer_faulted());
}

// perf_csv.rs:410:9 guarded -> Ok(()) (timeout): the stream operation would never run and a panicking sink would
// go unnoticed. A sink whose write panics must count as an I/O failure: fault flag set, every row accounted.
// C#: PerfCsvListenerFaultTests (WriterLoopAsync catch-all). The 20 000-char rows exceed the 8 KiB buffer, so the
// panic happens inside `write_row` rather than at the final flush.
#[test]
fn panicking_sink_is_treated_as_a_write_failure_and_every_row_is_dropped() {
    let listener =
        PerfCsvListener::start_for_test(Box::new(PanickingSink), 16, None, Some(Duration::ZERO), None, &|_| None);
    for _ in 0..2 {
        listener.enqueue(|| build_row("Panic", Some(&["text"]), Some(&[PerfValue::Str("x".repeat(20_000))])));
    }
    listener.dispose();
    assert!(
        listener.wait_finished(BOUND),
        "a panicking sink must not stop the writer thread from finishing"
    );
    assert!(listener.writer_faulted());
    assert_eq!(listener.dropped_count(), 2);
}
