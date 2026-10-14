//! W2-17: ports of the `PerfCsvListener*Tests` (escape, mapping, robustness, drop, fault, startup) plus the goldens
//! `perf-csv-escape`, `perf-csv-row` and `perf-csv-file` (C# `PhotoReview.Core.Tests/Diagnostics`, baseline d92d6aae).
//!
//! The listener registry is process-wide (C# `[Collection("GlobalState")]`): every test that starts a listener or emits
//! events holds `LOCK`, because `cargo test` runs the tests of one binary on parallel threads and a row of one test
//! would land in another's file. No test touches the process environment (`set_var` is `unsafe`): lookups are injected.

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use pr_core::diagnostics::diag_options::{DISABLE_DISK_CACHE_VAR, PRE_READ_VAR, PRELOAD_WORKERS_VAR};
use pr_core::diagnostics::perf::{PerfValue, PhotoReviewPerf};
use pr_core::diagnostics::perf_csv::{
    DIAG_FLAG_HEADER_NAMES, LocalTimeParts, PerfCsvListener, PerfRow, TRACE_ENV_VAR, build_row, csv_escape,
    current_thread_number, format_g17, format_value,
};
use pr_test_support::TempRoot;
use pr_test_support::golden;
use serde_json::Value;

static LOCK: Mutex<()> = Mutex::new(());

fn global_state() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- sinks ----------------------------------------------------------------------------------------------------

/// A one-shot gate: `wait` blocks (bounded) until `open` was called.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn open(&self) {
        *self.open.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.changed.notify_all();
    }

    fn wait(&self, timeout: Duration) -> bool {
        let guard = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, timeout, |open| !*open)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GateMode {
    None,
    /// C# `ProbeStream(blockFirstWrite)`: only the first write waits.
    FirstWrite,
    /// C# `GatedStream`: every write and flush waits.
    EveryOperation,
}

struct ProbeState {
    data: Mutex<Vec<u8>>,
    fail: AtomicBool,
    flushes: AtomicUsize,
    written: AtomicI64,
    mode: GateMode,
    gate: Gate,
    entered: Gate,
    blocked: AtomicBool,
}

/// A `Write` stub with the knobs of the C# `MemoryStream`/`CountingStream`/`ProbeStream`/`GatedStream`/`FailingStream`.
#[derive(Clone)]
struct Probe {
    state: Arc<ProbeState>,
}

impl Probe {
    fn with_mode(mode: GateMode) -> Self {
        Self {
            state: Arc::new(ProbeState {
                data: Mutex::new(Vec::new()),
                fail: AtomicBool::new(false),
                flushes: AtomicUsize::new(0),
                written: AtomicI64::new(0),
                mode,
                gate: Gate::default(),
                entered: Gate::default(),
                blocked: AtomicBool::new(false),
            }),
        }
    }

    fn memory() -> Self {
        Self::with_mode(GateMode::None)
    }

    fn failing() -> Self {
        let probe = Self::memory();
        probe.state.fail.store(true, Ordering::SeqCst);
        probe
    }

    fn sink(&self) -> Box<dyn Write + Send> {
        Box::new(self.clone())
    }

    fn set_fail(&self, fail: bool) {
        self.state.fail.store(fail, Ordering::SeqCst);
    }

    fn flushes(&self) -> usize {
        self.state.flushes.load(Ordering::SeqCst)
    }

    fn written(&self) -> i64 {
        self.state.written.load(Ordering::SeqCst)
    }

    fn open_gate(&self) {
        self.state.gate.open();
    }

    fn wait_entered(&self, timeout: Duration) -> bool {
        self.state.entered.wait(timeout)
    }

    fn bytes(&self) -> Vec<u8> {
        self.state.data.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn text(&self) -> String {
        String::from_utf8(self.bytes()).expect("the trace is UTF-8")
    }
}

fn simulated_failure() -> io::Error {
    io::Error::other("disk full (simulated)")
}

impl Write for Probe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let state = &self.state;
        if state.fail.load(Ordering::SeqCst) {
            return Err(simulated_failure());
        }
        match state.mode {
            GateMode::None => {}
            GateMode::FirstWrite => {
                if !state.blocked.swap(true, Ordering::SeqCst) {
                    state.entered.open();
                    state.gate.wait(Duration::from_secs(30));
                }
            }
            GateMode::EveryOperation => {
                state.gate.wait(Duration::from_secs(30));
            }
        }
        state
            .data
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        state.written.fetch_add(buf.len() as i64, Ordering::SeqCst);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let state = &self.state;
        if state.mode == GateMode::EveryOperation {
            state.gate.wait(Duration::from_secs(30));
        }
        if state.fail.load(Ordering::SeqCst) {
            return Err(simulated_failure());
        }
        state.flushes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

// ---- helpers --------------------------------------------------------------------------------------------------

fn no_diag(_: &str) -> Option<String> {
    None
}

fn start(sink: &Probe, capacity: usize) -> PerfCsvListener {
    PerfCsvListener::start_for_test(sink.sink(), capacity, None, None, None, &no_diag)
}

/// `BuildRow("Evt", names, values)`.
fn build(names: Option<&[&str]>, values: Option<&[PerfValue]>) -> PerfRow {
    build_row("Evt", names, values)
}

fn int(value: i64) -> PerfValue {
    PerfValue::Int(value)
}

fn text(value: &str) -> PerfValue {
    PerfValue::Str(value.to_owned())
}

/// The single-column row `Evt,...,a=1` that most C# tests enqueue.
fn simple_row() -> PerfRow {
    build(Some(&["a"]), Some(&[int(1)]))
}

/// Polls `condition` until it holds or `timeout` passes (C# `SpinWait.SpinUntil`).
fn spin_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    condition()
}

/// Minimal RFC 4180 reader for one line (C# `SplitCsvLine`).
fn split_csv_line(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if quoted {
            if c == '"' && i + 1 < chars.len() && chars[i + 1] == '"' {
                current.push('"');
                i += 1;
            } else if c == '"' {
                quoted = false;
            } else {
                current.push(c);
            }
        } else if c == '"' && current.is_empty() {
            quoted = true;
        } else if c == ',' {
            fields.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
        i += 1;
    }
    fields.push(current);
    fields
}

/// C# `Flatten`: every line break becomes one space (`\r\n` is one).
fn flatten(value: &str) -> String {
    value.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// Data rows (split into fields), the trailer value and the whole text of a written trace.
fn read_csv(text: &str) -> (Vec<Vec<String>>, i64) {
    let lines: Vec<&str> = text.split('\n').filter(|l| !l.is_empty()).collect();
    let rows = lines
        .iter()
        .filter(|l| !l.starts_with('#') && !l.starts_with("utcTicks,"))
        .map(|l| split_csv_line(l))
        .collect();
    let dropped = lines
        .iter()
        .rev()
        .find(|l| l.starts_with("# dropped="))
        .and_then(|l| l["# dropped=".len()..].trim().parse::<i64>().ok())
        .expect("a '# dropped=' trailer");
    (rows, dropped)
}

fn trailer_lines(text: &str) -> Vec<&str> {
    text.split('\n').filter(|l| l.starts_with("# dropped=")).collect()
}

fn fixed_local_now() -> LocalTimeParts {
    LocalTimeParts {
        year: 2026,
        month: 10,
        day: 14,
        hour: 9,
        minute: 5,
        second: 3,
    }
}

// ---- PerfCsvEscapeTests ----------------------------------------------------------------------------------------

// C#: PerfCsvEscapeTests.CsvEscape_FlattensLineBreaks (5 cases)
#[test]
fn csv_escape_flattens_line_breaks() {
    for (input, expected) in [
        ("a\nb", "a b"),
        ("a\r\nb", "a b"),
        ("a\rb", "a b"),
        ("plain", "plain"),
        ("", ""),
    ] {
        assert_eq!(csv_escape(input), expected, "{input:?}");
    }
}

// C#: PerfCsvEscapeTests.CsvEscape_QuotesCommasAndDoublesQuotes_WithoutLineBreaks
#[test]
fn csv_escape_quotes_commas_and_doubles_quotes_without_line_breaks() {
    let escaped = csv_escape("x,\"y\"\nz");

    assert_eq!(escaped, "\"x,\"\"y\"\" z\"");
    assert!(!escaped.contains('\n'));
}

// Golden: perf-csv-escape (27 cases).
#[test]
fn golden_perf_csv_escape_matches_csharp() {
    let golden = golden::load("perf-csv-escape");
    assert_eq!(golden.cases.len(), 27);
    for case in &golden.cases {
        let value = golden::str_of(case, "value");
        assert_eq!(csv_escape(value), golden::str_of(case, "escaped"), "{value:?}");
    }
}

// ---- PerfCsvListenerMappingTests -------------------------------------------------------------------------------

// C#: PerfCsvListenerMappingTests.BuildRow_MapsNamedAndPositionalPayload
#[test]
fn build_row_maps_named_and_positional_payload() {
    let row = build(
        Some(&["first", "nav", "pathId", "second", "third", "name", "fourth"]),
        Some(&[
            int(1),
            int(42),
            text("abc123"),
            PerfValue::Double(2.5),
            PerfValue::Bool(true),
            text("hello"),
            int(7),
        ]),
    );

    assert_eq!(row.event_name, "Evt");
    assert_eq!(row.nav, "42");
    assert_eq!(row.path_id, "abc123");
    assert_eq!(row.a, "1");
    assert_eq!(row.b, "2.5");
    assert_eq!(row.c, "1"); // bool -> 1
    assert_eq!(row.d, "7");
    assert_eq!(row.text, "hello");
}

// C#: PerfCsvListenerMappingTests.BuildRow_GenFillsNav
#[test]
fn build_row_gen_fills_nav() {
    let row = build(Some(&["gen", "phase"]), Some(&[int(9), text("scanned")]));

    assert_eq!(row.nav, "9");
    assert_eq!(row.text, "scanned");
    assert_eq!(row.a, "");
}

// C#: PerfCsvListenerMappingTests.BuildRow_SecondNavOrPathId_IsOrdinaryValue
#[test]
fn build_row_second_nav_or_path_id_is_ordinary_value() {
    let row = build(
        Some(&["nav", "gen", "pathId", "pathId"]),
        Some(&[int(10), int(20), text("p1"), text("p2")]),
    );

    assert_eq!(row.nav, "10");
    assert_eq!(row.path_id, "p1");
    assert_eq!(row.a, "20");
    assert_eq!(row.text, "p2");
}

// C#: PerfCsvListenerMappingTests.BuildRow_OtherNamesNeverFillNavOrPathId
#[test]
fn build_row_other_names_never_fill_nav_or_path_id() {
    let row = build(
        Some(&["slot", "label", "ms"]),
        Some(&[int(3), text("x"), PerfValue::Double(1.5)]),
    );

    assert_eq!(row.nav, "");
    assert_eq!(row.path_id, "");
    assert_eq!(row.a, "3");
    assert_eq!(row.b, "1.5");
    assert_eq!(row.text, "x");
}

// C#: PerfCsvListenerMappingTests.BuildRow_MoreThanFourNumerics_AreCappedAtFour
#[test]
fn build_row_more_than_four_numerics_are_capped_at_four() {
    let row = build(
        Some(&["a", "b", "c", "d", "e", "f"]),
        Some(&[int(1), int(2), int(3), int(4), int(5), int(6)]),
    );

    assert_eq!([row.a.as_str(), &row.b, &row.c, &row.d], ["1", "2", "3", "4"]);
    assert_eq!(row.text, "");
}

// C#: PerfCsvListenerMappingTests.BuildRow_TextValues_SkipNullsAndJoin
#[test]
fn build_row_text_values_skip_nulls_and_join() {
    let row = build(
        Some(&["t1", "t2", "t3", "t4"]),
        Some(&[text("alpha"), PerfValue::Null, text("beta"), text("gamma")]),
    );

    assert_eq!(row.text, "alpha;beta;gamma");
    assert_eq!(row.a, "");
}

// C#: PerfCsvListenerMappingTests.BuildRow_MismatchedLengths_UseTheShorterList
#[test]
fn build_row_mismatched_lengths_use_the_shorter_list() {
    let more_names = build(Some(&["a", "b", "c"]), Some(&[int(1), int(2)]));
    let more_values = build(Some(&["a", "b"]), Some(&[int(1), int(2), int(3)]));

    assert_eq!(
        (more_names.a.as_str(), &more_names.b, &more_names.c),
        ("1", &"2".to_owned(), &String::new())
    );
    assert_eq!(
        (more_values.a.as_str(), &more_values.b, &more_values.c),
        ("1", &"2".to_owned(), &String::new())
    );
}

// C#: PerfCsvListenerMappingTests.BuildRow_NullLists_YieldEmptyRow
#[test]
fn build_row_null_lists_yield_empty_row() {
    let no_names = build(None, Some(&[int(1), int(2)]));
    let no_values = build(Some(&["a"]), None);

    for row in [no_names, no_values] {
        assert_eq!(
            (row.nav.as_str(), &row.a, &row.b, &row.text),
            ("", &String::new(), &String::new(), &String::new())
        );
    }
}

// C#: PerfCsvListenerMappingTests.BuildRow_FormatsValuesInvariantly
#[test]
fn build_row_formats_values_invariantly() {
    let row = build(
        Some(&["a", "b", "c"]),
        Some(&[PerfValue::Double(0.1), PerfValue::Bool(false), int(1_234_567_890_123)]),
    );

    assert_eq!(row.a, "0.10000000000000001");
    assert_eq!(row.b, "0");
    assert_eq!(row.c, "1234567890123");
}

// C#: PerfCsvListenerRobustnessTests.Rows_AreCultureInvariant (4 theories; Rust has no culture: one run, the
// expectations are those of a de-DE machine)
#[test]
fn rows_are_culture_invariant() {
    let key = build_row(
        "KeyInput",
        Some(&["nav", "key", "inputDelayMs"]),
        Some(&[int(1_234_567), text("Right"), PerfValue::Double(12.5)]),
    );
    let disk = build_row(
        "DiskCacheRead",
        Some(&["nav", "pathId", "ms", "bytes"]),
        Some(&[int(2), text("id"), PerfValue::Double(0.125), int(9_876_543_210)]),
    );

    assert_eq!(key.nav, "1234567");
    assert_eq!(key.a, "12.5");
    assert_eq!(disk.a, "0.125");
    assert_eq!(disk.b, "9876543210");
    assert_eq!(format_value(&PerfValue::Double(12.5)), "12.5");
    for field in [&key.nav, &key.a, &disk.a, &disk.b] {
        assert!(!field.contains('\u{066B}') && !field.contains(' ') && !field.contains('\u{00A0}'));
    }
}

#[test]
fn current_thread_number_is_small_positive_and_stable_per_thread() {
    let here = current_thread_number();
    assert!(here > 0);
    assert_eq!(current_thread_number(), here);

    let other = std::thread::spawn(current_thread_number).join().expect("thread joins");
    assert!(other > 0);
    assert_ne!(other, here);
}

#[test]
fn format_g17_pins_the_scientific_switch_points() {
    assert_eq!(format_g17(1e16), "10000000000000000");
    assert_eq!(format_g17(1e17), "1E+17");
    assert_eq!(format_g17(0.0001), "0.0001");
    assert_eq!(format_g17(1e-5), "1.0000000000000001E-05");
    assert_eq!(format_g17(-0.0), "-0");
    assert_eq!(format_g17(f64::NAN), "NaN");
}

// Golden: perf-csv-row (49 cases, 24 of them g17).
#[test]
fn golden_perf_csv_row_matches_csharp() {
    fn value_of(node: &Value) -> PerfValue {
        let kind = golden::str_of(node, "t");
        match kind {
            "null" => PerfValue::Null,
            "long" | "int" => PerfValue::Int(node["v"].as_i64().expect("integer payload")),
            "bool" => PerfValue::Bool(node["v"].as_bool().expect("bool payload")),
            "string" => PerfValue::Str(golden::str_of(node, "v").to_owned()),
            "double" => {
                if let Some(bits) = node.get("bits").and_then(Value::as_str) {
                    PerfValue::Double(f64::from_bits(u64::from_str_radix(bits, 16).expect("hex bits")))
                } else if let Some(v) = node["v"].as_str() {
                    PerfValue::Double(v.parse::<f64>().expect("double text"))
                } else {
                    PerfValue::Double(node["v"].as_f64().expect("double payload"))
                }
            }
            other => panic!("unknown payload type {other}"),
        }
    }

    let golden = golden::load("perf-csv-row");
    assert_eq!(golden.cases.len(), 49);
    for case in &golden.cases {
        let id = golden::str_of(case, "id");
        let names: Option<Vec<String>> = case["names"]
            .as_array()
            .map(|a| a.iter().map(|n| n.as_str().expect("name text").to_owned()).collect());
        let values: Option<Vec<PerfValue>> = case["values"].as_array().map(|a| a.iter().map(value_of).collect());
        let name_refs: Option<Vec<&str>> = names.as_ref().map(|n| n.iter().map(String::as_str).collect());

        let row = build_row(golden::str_of(case, "event"), name_refs.as_deref(), values.as_deref());

        let out = &case["out"];
        let actual = [
            ("event", row.event_name.as_str()),
            ("nav", &row.nav),
            ("pathId", &row.path_id),
            ("a", &row.a),
            ("b", &row.b),
            ("c", &row.c),
            ("d", &row.d),
            ("text", &row.text),
        ];
        for (field, got) in actual {
            assert_eq!(got, golden::str_of(out, field), "{id}: {field}");
        }
    }
}

// ---- enqueue / dispose / flush cadence ------------------------------------------------------------------------

// C#: PerfCsvListenerMappingTests.Enqueue_BuilderThrows_IsCountedAsDropped
#[test]
fn enqueue_builder_throws_is_counted_as_dropped() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener = start(&sink, 10);

    // The closure panics: enqueue must contain it (C# `Record.Exception` is null).
    listener.enqueue(|| -> PerfRow { panic!("bad payload") });

    assert_eq!(listener.dropped_count(), 1);
    listener.dispose();
}

// C#: PerfCsvListenerMappingTests.Enqueue_AfterDispose_IsCountedAsDropped
#[test]
fn enqueue_after_dispose_is_counted_as_dropped() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener = start(&sink, 10);
    listener.dispose();
    let length = sink.bytes().len();

    listener.enqueue(simple_row);

    assert_eq!(listener.dropped_count(), 1);
    assert_eq!(sink.bytes().len(), length);
}

// C#: PerfCsvListenerMappingTests.Enqueue_Normal_IsWrittenNotDropped
#[test]
fn enqueue_normal_is_written_not_dropped() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener = start(&sink, 10);

    listener.enqueue(simple_row);
    listener.dispose();

    assert_eq!(listener.dropped_count(), 0);
    let text = sink.text();
    assert!(text.contains(",Evt,,,1,,,,\n"), "{text:?}");
    assert!(text.contains("# dropped=0"));
}

// C#: PerfCsvListenerMappingTests.Writer_DoesNotFlushPerRow
#[test]
fn writer_does_not_flush_per_row() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener = start(&sink, 1000);

    for _ in 0..50 {
        listener.enqueue(simple_row);
    }
    listener.dispose();

    assert!(sink.flushes() < 10, "flushes={}", sink.flushes());
}

// C#: PerfCsvListenerMappingTests.Writer_FlushesAfterOneSecond
#[test]
#[ignore = "slow: 1.2 s wall clock"]
fn writer_flushes_after_one_second() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener = start(&sink, 1000);

    // The moment being waited out (the 1 s flush interval elapsing) has no observable signal.
    std::thread::sleep(Duration::from_millis(1200));
    let before = sink.flushes();

    listener.enqueue(simple_row);

    assert!(
        spin_until(Duration::from_secs(10), || sink.flushes() > before),
        "no periodic flush after 1 s"
    );
    listener.dispose();
}

// C#: PerfCsvListenerMappingTests.Header_ContainsVersionAndDiagFlags (`commit` is a parameter in Rust)
#[test]
fn header_contains_version_and_diag_flags() {
    let _guard = global_state();
    let sink = Probe::memory();
    let diag = |name: &str| (name == "PHOTOREVIEW_DIAG_PREREAD").then(|| "1".to_owned());

    PerfCsvListener::start_for_test(sink.sink(), 10, None, None, Some("1.2.3"), &diag).dispose();

    let text = sink.text();
    let header = text.split('\n').next().expect("a header line");
    assert!(
        header.starts_with("# commit=1.2.3 diag=PHOTOREVIEW_DIAG_PREREAD=1 qpcFrequency=10000000\r"),
        "{header:?}"
    );
}

#[test]
fn header_without_commit_says_unknown() {
    let _guard = global_state();
    let sink = Probe::memory();

    start(&sink, 10).dispose();

    assert!(
        sink.text()
            .starts_with("# commit=unknown diag= qpcFrequency=10000000\r\n")
    );
}

// Contract note 7: the header keeps the C# names while the Rust app reads PHOTOREVIEW_RS_DIAG_*.
#[test]
fn diag_header_names_map_to_the_rust_variable_names() {
    let mapped: Vec<String> = DIAG_FLAG_HEADER_NAMES
        .iter()
        .map(|name| name.replacen("PHOTOREVIEW_", "PHOTOREVIEW_RS_", 1))
        .collect();

    assert_eq!(mapped, [PRE_READ_VAR, PRELOAD_WORKERS_VAR, DISABLE_DISK_CACHE_VAR]);
}

// ---- PerfCsvListenerRobustnessTests ----------------------------------------------------------------------------

// C#: PerfCsvListenerRobustnessTests.CsvEscape_RoundTrips (seeds 1-3, deterministic xorshift RNG)
#[test]
fn csv_escape_round_trips() {
    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self, bound: usize) -> usize {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            (x % bound as u64) as usize
        }
    }

    let pieces = [
        ",", "\"", "\"\"", "\r", "\n", "\r\n", "a", " ", "\t", "é", "日", "😀", ";", "'", "=", "\0", "#",
    ];
    for seed in 1..=3u64 {
        let mut rng = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for _ in 0..5000 {
            let count = rng.next(8);
            let value: String = (0..count).map(|_| pieces[rng.next(pieces.len())]).collect();

            let escaped = csv_escape(&value);
            let parsed = split_csv_line(&format!("x,{escaped},y"));

            assert!(!escaped.contains('\n') && !escaped.contains('\r'), "{value:?}");
            assert_eq!(parsed.len(), 3, "{value:?} -> {escaped:?}");
            assert_eq!(parsed[1], flatten(&value), "{value:?}");
        }
    }
}

// C#: PerfCsvListenerRobustnessTests.ConcurrentProducers_NothingLost
#[test]
fn concurrent_producers_nothing_lost() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener = start(&sink, 100_000);
    const THREADS: i32 = 8;
    const PER_THREAD: i64 = 4000;

    std::thread::scope(|scope| {
        for t in 0..THREADS {
            scope.spawn(move || {
                for i in 0..PER_THREAD {
                    PhotoReviewPerf::log().preload_paused(t, i);
                }
            });
        }
    });
    listener.dispose();

    let (rows, dropped) = read_csv(&sink.text());
    assert_eq!(rows.len() as i64 + dropped, i64::from(THREADS) * PER_THREAD);
    assert!(rows.iter().all(|r| r.len() == 11));
    assert!(rows.iter().all(|r| r[3] == "PreloadPaused"));
}

// C#: PerfCsvListenerRobustnessTests.DisposeUnderLoad_ProducesWellFormedFile
#[test]
fn dispose_under_load_produces_well_formed_file() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener = start(&sink, 100_000);
    let stop = AtomicBool::new(false);
    let produced = AtomicI64::new(0);

    std::thread::scope(|scope| {
        for t in 0..4 {
            let (stop, produced) = (&stop, &produced);
            scope.spawn(move || {
                let mut i = 0;
                while !stop.load(Ordering::SeqCst) {
                    PhotoReviewPerf::log().preload_paused(t, i);
                    i += 1;
                    produced.fetch_add(1, Ordering::SeqCst);
                }
            });
        }
        assert!(
            spin_until(Duration::from_secs(30), || produced.load(Ordering::SeqCst) > 200),
            "the producers never ran"
        );

        listener.dispose(); // must not panic or hang while producers are still emitting
        stop.store(true, Ordering::SeqCst);
    });

    let text = sink.text();
    let (rows, _) = read_csv(&text);
    assert!(rows.iter().all(|r| r.len() == 11));
    assert_eq!(trailer_lines(&text).len(), 1);
    assert!(
        text.trim_end()
            .split('\n')
            .next_back()
            .is_some_and(|l| l.starts_with("# dropped="))
    );
}

// C#: PerfCsvListenerRobustnessTests.Dispose_Twice_AndEventsAfterwardsIgnored
#[test]
fn dispose_twice_and_events_afterwards_ignored() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener = start(&sink, 100);
    listener.dispose();
    let length = sink.bytes().len();

    listener.dispose();
    PhotoReviewPerf::log().preload_paused(1, 2);

    assert_eq!(sink.bytes().len(), length);
    assert!(
        !PhotoReviewPerf::log().is_enabled(),
        "dispose unsubscribed the listener"
    );
}

// C#: PerfCsvListenerRobustnessTests.TryStartFromEnvironment_BadPaths_ReturnNull
#[test]
fn try_start_bad_paths_return_none_and_leave_nothing_behind() {
    let _guard = global_state();
    let temp = TempRoot::new("perf-env");
    let as_file = temp.join("not-a-dir");
    std::fs::write(&as_file, "x").expect("write the file used as a directory");
    let below_file = as_file.join("sub");

    let values: [Option<String>; 6] = [
        None,
        Some(String::new()),
        Some("   ".to_owned()),
        Some(as_file.to_string_lossy().into_owned()),
        Some(below_file.to_string_lossy().into_owned()),
        Some(r"Z:\definitely\missing\drive\|invalid".to_owned()),
    ];
    for value in values {
        let lookup = |name: &str| if name == TRACE_ENV_VAR { value.clone() } else { None };

        let listener = PerfCsvListener::try_start(&lookup, None, &fixed_local_now);

        assert!(listener.is_none(), "{value:?}");
    }

    let names: Vec<String> = std::fs::read_dir(temp.path())
        .expect("read the temp dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["not-a-dir"]);
}

// ---- PerfCsvListenerDropTests ----------------------------------------------------------------------------------

// C#: PerfCsvListenerDropTests.DroppedRowsAreCountedAndReported
#[test]
fn dropped_rows_are_counted_and_reported() {
    let _guard = global_state();
    let sink = Probe::with_mode(GateMode::EveryOperation);
    let listener = start(&sink, 4);

    // The writer blocks on its first buffer spill, so the 4-slot queue fills and the rest is discarded.
    const EMITTED: i64 = 3000;
    for _ in 0..EMITTED {
        PhotoReviewPerf::log().preload_paused(85, 512);
    }

    let dropped_before_release = listener.dropped_count();
    sink.open_gate();
    listener.dispose();

    // The writer can consume at most a few hundred rows (one buffer) before blocking; everything else was dropped.
    assert!(
        dropped_before_release >= EMITTED - 500,
        "dropped={dropped_before_release}"
    );
    let (_, dropped) = read_csv(&sink.text());
    assert!(dropped >= EMITTED - 500, "trailer={dropped}");
}

// ---- PerfCsvListenerFaultTests ---------------------------------------------------------------------------------

// C#: PerfCsvListenerFaultTests.IdleWriter_FlushesTheTailOfABurst
#[test]
fn idle_writer_flushes_the_tail_of_a_burst() {
    let _guard = global_state();
    let sink = Probe::memory();
    let listener =
        PerfCsvListener::start_for_test(sink.sink(), 100, Some(Duration::from_millis(50)), None, None, &no_diag);

    listener.enqueue(simple_row);

    assert!(
        spin_until(Duration::from_secs(20), || sink.written() > 0),
        "the burst tail was never flushed while idle"
    );
    listener.dispose();
}

// C#: PerfCsvListenerFaultTests.WriterFault_CountsEveryAbandonedRow
#[test]
fn writer_fault_counts_every_abandoned_row() {
    let _guard = global_state();
    let sink = Probe::failing();
    let listener = start(&sink, 1000);
    const EMITTED: i64 = 200;
    for _ in 0..EMITTED {
        listener.enqueue(simple_row);
    }

    assert!(
        spin_until(Duration::from_secs(20), || listener.writer_faulted()),
        "the writer never faulted"
    );
    sink.set_fail(false);
    listener.dispose();

    let text = sink.text();
    let trailers = trailer_lines(&text);
    assert_eq!(trailers.len(), 1, "{text:?}");
    assert_eq!(trailers[0].trim(), format!("# dropped={EMITTED}"));
}

// C#: PerfCsvListenerFaultTests.DisposeTimeout_DefersTheTrailerUntilTheWriterTaskEnds
#[test]
fn dispose_timeout_defers_the_trailer_until_the_writer_task_ends() {
    let _guard = global_state();
    let sink = Probe::with_mode(GateMode::FirstWrite);
    let listener = PerfCsvListener::start_for_test(
        sink.sink(),
        1000,
        None,
        Some(Duration::from_millis(100)),
        None,
        &no_diag,
    );
    for _ in 0..200 {
        listener.enqueue(simple_row);
    }
    assert!(
        sink.wait_entered(Duration::from_secs(20)),
        "the writer never reached the stream"
    );

    listener.dispose(); // times out: the writer thread is blocked inside its first sink write
    let during_write = sink.written();

    sink.open_gate();
    assert!(
        listener.wait_finished(Duration::from_secs(20)),
        "the writer never finished"
    );
    let text = sink.text();

    assert_eq!(during_write, 0);
    assert_eq!(trailer_lines(&text).len(), 1);
    assert!(
        text.trim_end()
            .split('\n')
            .next_back()
            .is_some_and(|l| l.starts_with("# dropped="))
    );
}

// ---- PerfCsvListenerStartupAndFaultCoverageTests ---------------------------------------------------------------

// C#: PerfCsvListenerStartupAndFaultCoverageTests.TryStartFromEnvironment_ValidDirectory_WritesAWellFormedTraceFile
#[test]
fn try_start_valid_directory_writes_a_well_formed_trace_file() {
    let _guard = global_state();
    let temp = TempRoot::new("perf-env");
    let trace_dir = temp.join("nested").join("trace"); // does not exist yet
    let trace_value = trace_dir.to_string_lossy().into_owned();
    let lookup = |name: &str| (name == TRACE_ENV_VAR).then(|| trace_value.clone());

    {
        let listener = PerfCsvListener::try_start(&lookup, Some("1.0.0"), &fixed_local_now);
        assert!(listener.is_some());
        PhotoReviewPerf::log().key_input(42, "Right", 12.5);
    } // drop = dispose: drains the queue and writes the trailer

    let files: Vec<_> = std::fs::read_dir(&trace_dir)
        .expect("trace dir exists")
        .map(|e| e.expect("entry").path())
        .collect();
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().expect("file name").to_string_lossy().into_owned();
    assert_eq!(name, format!("perf-{}-20261014-090503.csv", std::process::id()));

    let content = std::fs::read_to_string(&files[0]).expect("read trace");
    let lines: Vec<&str> = content.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
    assert!(lines[0].starts_with("# commit=1.0.0 "), "{}", lines[0]);
    assert_eq!(lines[1], "utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text");
    assert!(
        lines.iter().any(|l| {
            let f: Vec<&str> = l.split(',').collect();
            !l.starts_with('#') && f.len() == 11 && f[3] == "KeyInput" && f[6] == "12.5"
        }),
        "{lines:?}"
    );
    assert_eq!(lines[lines.len() - 1], "# dropped=0");
}

// C#: PerfCsvListenerStartupAndFaultCoverageTests.Header_ListsOnlyTheDiagnosticFlagsThatAreSet
#[test]
fn header_lists_only_the_diagnostic_flags_that_are_set() {
    let _guard = global_state();
    let sink = Probe::memory();
    let diag = |name: &str| match name {
        "PHOTOREVIEW_DIAG_PRELOAD_WORKERS" => Some("3".to_owned()),
        "PHOTOREVIEW_DIAG_DISABLE_DISKCACHE" => Some("1".to_owned()),
        _ => None,
    };

    PerfCsvListener::start_for_test(sink.sink(), 10, None, None, None, &diag).dispose();

    let text = sink.text();
    let header = text.split('\n').next().expect("a header line");
    assert!(
        header.contains(" diag=PHOTOREVIEW_DIAG_PRELOAD_WORKERS=3;PHOTOREVIEW_DIAG_DISABLE_DISKCACHE=1 "),
        "{header:?}"
    );
    assert!(!header.contains("PHOTOREVIEW_DIAG_PREREAD"));
}

// C#: PerfCsvListenerStartupAndFaultCoverageTests.Header_NoDiagnosticFlags_EmptyDiagField
#[test]
fn header_no_diagnostic_flags_empty_diag_field() {
    let _guard = global_state();
    let sink = Probe::memory();
    let diag = |name: &str| (name == "PHOTOREVIEW_DIAG_PREREAD").then(String::new);

    PerfCsvListener::start_for_test(sink.sink(), 10, None, None, None, &diag).dispose();

    let text = sink.text();
    assert!(text.split('\n').next().expect("a header line").contains(" diag= "));
}

// C#: string.IsNullOrEmpty keeps a whitespace-only diag value (extra coverage, no C# test)
#[test]
fn header_keeps_whitespace_only_diag_value() {
    let _guard = global_state();
    let sink = Probe::memory();
    let diag = |name: &str| (name == "PHOTOREVIEW_DIAG_PREREAD").then(|| "  ".to_owned());

    PerfCsvListener::start_for_test(sink.sink(), 10, None, None, None, &diag).dispose();

    let text = sink.text();
    assert!(
        text.split('\n')
            .next()
            .expect("a header line")
            .contains(" diag=PHOTOREVIEW_DIAG_PREREAD=   ")
    );
}

// C# FileShare.Read: a second start on the same file name (same pid, same second) is refused.
#[cfg(windows)]
#[test]
fn second_start_same_second_same_pid_is_refused_like_fileshare_read() {
    let _guard = global_state();
    let temp = TempRoot::new("perf-env");
    let value = temp.join("trace").to_string_lossy().into_owned();
    let lookup = |name: &str| (name == TRACE_ENV_VAR).then(|| value.clone());

    let first = PerfCsvListener::try_start(&lookup, None, &fixed_local_now);
    let second = PerfCsvListener::try_start(&lookup, None, &fixed_local_now);

    assert!(first.is_some());
    assert!(second.is_none());
}

// C#: PerfCsvListenerStartupAndFaultCoverageTests.EnqueueAfterWriterFault_CountsEachRowAsDroppedAndDisposeStaysQuiet
#[test]
fn enqueue_after_writer_fault_counts_each_row_as_dropped_and_dispose_stays_quiet() {
    let _guard = global_state();
    let sink = Probe::failing();
    let listener = start(&sink, 1000);
    const BEFORE: i64 = 200;
    for _ in 0..BEFORE {
        listener.enqueue(simple_row);
    }
    assert!(
        spin_until(Duration::from_secs(20), || listener.writer_faulted()),
        "the writer never faulted"
    );
    // writer_faulted flips before the writer has drained the abandoned rows into the counter: wait for the counter to
    // settle (a condition, not a delay) before asserting that every abandoned row is counted exactly once.
    assert!(
        spin_until(Duration::from_secs(20), || listener.dropped_count() >= BEFORE),
        "the abandoned rows were never all counted"
    );
    assert_eq!(listener.dropped_count(), BEFORE);

    for _ in 0..5 {
        listener.enqueue(simple_row);
    }

    assert_eq!(listener.dropped_count(), BEFORE + 5);
    listener.dispose(); // the trailer cannot reach the broken stream: best effort, no panic
}

// C#: PerfCsvListenerStartupAndFaultCoverageTests.NeverStartedListener_IgnoresEnqueueAndDisposes
#[test]
fn never_started_listener_ignores_enqueue_and_disposes() {
    let listener = PerfCsvListener::unstarted();

    listener.enqueue(simple_row);

    assert_eq!(listener.dropped_count(), 0); // nothing was queued, so nothing counts as lost
    assert!(!listener.writer_faulted());
    listener.dispose();
    listener.dispose();
}

// ---- Golden perf-csv-file --------------------------------------------------------------------------------------

fn arg_i64(args: &[Value], i: usize) -> i64 {
    args[i]
        .as_i64()
        .unwrap_or_else(|| panic!("arg {i} is not an integer: {args:?}"))
}

fn arg_i32(args: &[Value], i: usize) -> i32 {
    i32::try_from(arg_i64(args, i)).unwrap_or_else(|_| panic!("arg {i} does not fit an int: {args:?}"))
}

fn arg_f64(args: &[Value], i: usize) -> f64 {
    match &args[i] {
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => other.parse().unwrap_or_else(|_| panic!("arg {i}: bad double {other}")),
        },
        other => other
            .as_f64()
            .unwrap_or_else(|| panic!("arg {i} is not a double: {other}")),
    }
}

fn arg_str(args: &[Value], i: usize) -> &str {
    args[i]
        .as_str()
        .unwrap_or_else(|| panic!("arg {i} is not a string: {args:?}"))
}

fn arg_bool(args: &[Value], i: usize) -> bool {
    args[i]
        .as_bool()
        .unwrap_or_else(|| panic!("arg {i} is not a bool: {args:?}"))
}

/// Calls the event method named like the C# method (what the oracle does through reflection).
fn emit(name: &str, a: &[Value]) {
    let log = PhotoReviewPerf::log();
    match name {
        "KeyInput" => log.key_input(arg_i64(a, 0), arg_str(a, 1), arg_f64(a, 2)),
        "ShowStart" => log.show_start(arg_i64(a, 0), arg_i32(a, 1), arg_str(a, 2)),
        "Stat" => log.stat(arg_i64(a, 0), arg_f64(a, 1)),
        "Lookup" => log.lookup(arg_i64(a, 0), arg_str(a, 1), arg_str(a, 2)),
        "ThumbStart" => log.thumb_start(arg_i64(a, 0), arg_str(a, 1)),
        "ThumbEnd" => log.thumb_end(arg_i64(a, 0), arg_str(a, 1), arg_str(a, 2), arg_f64(a, 3)),
        "JoinStart" => log.join_start(arg_i64(a, 0), arg_str(a, 1)),
        "JoinEnd" => log.join_end(arg_i64(a, 0), arg_str(a, 1), arg_f64(a, 2)),
        "DiskCacheRead" => log.disk_cache_read(arg_i64(a, 0), arg_str(a, 1), arg_f64(a, 2), arg_i64(a, 3)),
        "SourceRead" => log.source_read(arg_i64(a, 0), arg_str(a, 1), arg_f64(a, 2), arg_i64(a, 3)),
        "Decode" => log.decode(
            arg_i64(a, 0),
            arg_str(a, 1),
            arg_f64(a, 2),
            arg_i32(a, 3),
            arg_bool(a, 4),
            arg_bool(a, 5),
        ),
        "Verify" => log.verify(arg_i64(a, 0), arg_str(a, 1), arg_f64(a, 2)),
        "Assign" => log.assign(arg_i64(a, 0), arg_f64(a, 1), arg_i32(a, 2), arg_i32(a, 3)),
        "Rendered" => log.rendered(arg_i64(a, 0), arg_f64(a, 1)),
        "Presented" => log.presented(arg_i64(a, 0), arg_str(a, 1)),
        "PostStart" => log.post_start(arg_i64(a, 0), arg_str(a, 1)),
        "PostEnd" => log.post_end(arg_i64(a, 0), arg_str(a, 1), arg_f64(a, 2)),
        "PreloadItem" => log.preload_item(
            arg_i32(a, 0),
            arg_str(a, 1),
            arg_f64(a, 2),
            arg_str(a, 3),
            arg_f64(a, 4),
        ),
        "PreloadPaused" => log.preload_paused(arg_i32(a, 0), arg_i64(a, 1)),
        "PreloadCancel" => log.preload_cancel(arg_str(a, 0)),
        "DispatcherLongOp" => log.dispatcher_long_op(arg_f64(a, 0), arg_str(a, 1), arg_str(a, 2)),
        "Folder" => log.folder(arg_i64(a, 0), arg_str(a, 1), arg_f64(a, 2)),
        "DiagMode" => log.diag_mode(arg_str(a, 0)),
        "RenderedFrame" => log.rendered_frame(arg_i64(a, 0), arg_f64(a, 1)),
        "Startup" => log.startup(arg_str(a, 0), arg_f64(a, 1)),
        "FolderInfo" => log.folder_info(arg_i64(a, 0), arg_str(a, 1), arg_i64(a, 2), arg_str(a, 3)),
        other => panic!("unknown event method {other}"),
    }
}

/// The oracle's masking: header `commit=\S*` and `qpcFrequency=\d+`, first three columns of every data row.
fn mask(text: &str) -> String {
    let masked: Vec<String> = text
        .split('\n')
        .map(|line| {
            if let Some(after) = line.strip_prefix("# commit=") {
                let (_, rest) = after.split_once(' ').unwrap_or((after, ""));
                let Some((diag_part, freq_part)) = rest.rsplit_once("qpcFrequency=") else {
                    return line.to_owned();
                };
                let digits = freq_part.chars().take_while(char::is_ascii_digit).count();
                format!(
                    "# commit=<commit> {diag_part}qpcFrequency=<freq>{}",
                    &freq_part[digits..]
                )
            } else if !line.starts_with('#') && !line.starts_with("utcTicks,") {
                let parts: Vec<&str> = line.splitn(4, ',').collect();
                let numeric = parts.len() == 4
                    && parts[..3]
                        .iter()
                        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
                if numeric {
                    format!("<utc>,<qpc>,<thread>,{}", parts[3])
                } else {
                    line.to_owned()
                }
            } else {
                line.to_owned()
            }
        })
        .collect();
    masked.join("\n")
}

// Golden: perf-csv-file (17 cases: header flag order and raw values, all 26 events, escaping, extremes).
#[test]
fn golden_perf_csv_file_matches_csharp() {
    let _guard = global_state();
    let golden = golden::load("perf-csv-file");
    assert_eq!(golden.cases.len(), 17);
    for case in &golden.cases {
        let id = golden::str_of(case, "id");
        let env: HashMap<String, String> = case["env"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        // The golden environment is keyed by the C# names; the injected lookup answers by them (no set_var).
        let diag = |name: &str| env.get(name).cloned();
        let sink = Probe::memory();

        let listener = PerfCsvListener::start_for_test(sink.sink(), 1000, None, None, None, &diag);
        for event in case["events"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let args = event["args"].as_array().expect("event args");
            emit(golden::str_of(event, "name"), args);
        }
        listener.dispose();

        assert_eq!(mask(&sink.text()), golden::str_of(case, "masked_text"), "{id}");
    }
}

// ---- repair round 2 ---------------------------------------------------------------------------------------------

struct PanickingSink;

impl Write for PanickingSink {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        panic!("sink exploded")
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// C# WriterLoopAsync catch-all: any exception from the stream marks the writer faulted and counts the rows.
#[test]
fn panicking_sink_marks_writer_faulted_and_counts_rows() {
    let _guard = global_state();
    let no_diag = |_: &str| None;
    let listener = PerfCsvListener::start_for_test(
        Box::new(PanickingSink),
        100,
        Some(Duration::from_millis(50)),
        Some(Duration::from_secs(5)),
        None,
        &no_diag,
    );
    for _ in 0..10 {
        listener.enqueue(simple_row);
    }
    assert!(
        spin_until(Duration::from_secs(5), || listener.writer_faulted()),
        "a sink panic must set writer_faulted"
    );
    // Later rows are counted, not queued forever.
    let before = listener.dropped_count();
    listener.enqueue(simple_row);
    assert_eq!(listener.dropped_count(), before + 1);
    listener.dispose();
    assert!(listener.dropped_count() >= 1);
}

// A trace directory with a middle segment ending in a space is accepted by C# (.NET Core extended-prefix paths).
#[cfg(windows)]
#[test]
fn trace_dir_with_trailing_space_segment_is_accepted() {
    let _guard = global_state();
    let base = std::env::temp_dir().join(format!("w217-trailing-space-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("temp base");
    let dir = format!(r"{}\x \y", base.display());
    let lookup = |name: &str| (name == TRACE_ENV_VAR).then(|| dir.clone());
    let now = || LocalTimeParts {
        year: 2020,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    };
    let listener = PerfCsvListener::try_start(&lookup, None, &now);
    let started = listener.is_some();
    drop(listener);
    let _ = std::fs::remove_dir_all(format!(r"\\?\{}", base.display()));
    assert!(started, "trailing-space directory segment must be accepted");
}

// The 1 s rule must flush while rows keep arriving (the idle flush never fires with a 30 s interval).
#[test]
fn one_second_rule_flushes_while_rows_keep_arriving() {
    let _guard = global_state();
    let sink = Probe::memory();
    let no_diag = |_: &str| None;
    let listener =
        PerfCsvListener::start_for_test(sink.sink(), 1000, Some(Duration::from_secs(30)), None, None, &no_diag);
    let end = Instant::now() + Duration::from_millis(1600);
    while Instant::now() < end {
        listener.enqueue(simple_row);
        std::thread::sleep(Duration::from_millis(20));
    }
    let seen = sink.flushes();
    listener.dispose();
    assert!(seen >= 1, "no flush during a 1.6 s busy stream (flushes={seen})");
}
