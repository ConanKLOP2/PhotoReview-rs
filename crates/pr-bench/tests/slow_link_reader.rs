//! Port of `PhotoReview.Benchmarking/SlowLinkSourceReader.cs` (C# baseline d92d6aae), W7-02b.
//!
//! The C# class has no test of its own. Its behaviour is pinned by the golden `slow-link-source-reader` (oracle-cs,
//! 15 cases) and by two new lower-bound timing tests (no C# twin). Timing asserts are lower bounds only; they never
//! compare durations for equality.

use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pr_bench::slow_link_fs::SharedBandwidthLimiter;
use pr_bench::slow_link_reader::SlowLinkSourceReader;
use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// The C# default buffer size (1 MiB), passed by the caller when a case does not set one.
const DEFAULT_BUFFER_SIZE: usize = 1_048_576;

/// What the recording inner reader saw.
#[derive(Debug, Clone, Default)]
struct Calls {
    count: usize,
    path: Option<PathBuf>,
    priority: Option<SourceReadPriority>,
    buffer_size: Option<usize>,
}

/// Inner reader for the golden cases: `Cursor` over fixed bytes, optional `NotFound` failure, every open recorded.
#[derive(Debug)]
struct RecordingReader {
    data: Vec<u8>,
    fails: bool,
    calls: Mutex<Calls>,
}

impl RecordingReader {
    fn new(data: Vec<u8>, fails: bool) -> Arc<Self> {
        Arc::new(Self {
            data,
            fails,
            calls: Mutex::new(Calls::default()),
        })
    }

    fn snapshot(&self) -> Calls {
        self.calls.lock().expect("calls lock").clone()
    }
}

impl SourceReader for RecordingReader {
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        {
            let mut calls = self.calls.lock().expect("calls lock");
            calls.count += 1;
            calls.path = Some(path.to_path_buf());
            calls.priority = Some(priority);
            calls.buffer_size = Some(buffer_size);
        }
        if self.fails {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(Box::new(Cursor::new(self.data.clone())))
    }
}

/// The golden byte pattern: `(i * 31 + 7) % 251`.
fn golden_data(length: usize) -> Vec<u8> {
    (0..length)
        .map(|i| u8::try_from((i * 31 + 7) % 251).expect("value below 251"))
        .collect()
}

/// Checksum of the golden: `s = (s * 131 + b) % 1_000_000_007`, starting at 0 for each read.
fn checksum(bytes: &[u8]) -> i64 {
    let mut s: i64 = 0;
    for &b in bytes {
        s = (s * 131 + i64::from(b)) % 1_000_000_007;
    }
    s
}

fn parse_priority(text: &str) -> SourceReadPriority {
    match text {
        "Viewer" => SourceReadPriority::Viewer,
        "Preload" => SourceReadPriority::Preload,
        other => panic!("unknown priority {other}"),
    }
}

/// Seeks like the C# `Seek(offset, origin)`. A negative absolute target (`Begin` below 0) cannot be expressed as
/// `SeekFrom::Start`, so it is sent as a `Current` seek to that absolute offset, which fails the same way.
fn seek_to(stream: &mut dyn ReadSeek, offset: i64, origin: &str) -> io::Result<u64> {
    match origin {
        "Begin" if offset >= 0 => stream.seek(SeekFrom::Start(u64::try_from(offset).expect("offset >= 0"))),
        "Begin" => {
            let position = stream.stream_position()?;
            let position = i64::try_from(position).expect("position fits i64");
            stream.seek(SeekFrom::Current(offset - position))
        }
        "Current" => stream.seek(SeekFrom::Current(offset)),
        "End" => stream.seek(SeekFrom::End(offset)),
        other => panic!("unknown seek origin {other}"),
    }
}

fn run_case(case: &Value) {
    let name = str_of(case, "name");
    let length = usize::try_from(i64_of(case, "length")).expect("length");
    let inner = RecordingReader::new(golden_data(length), bool_of(case, "inner_fails"));
    let limiter = case["bandwidth_bytes_per_second"]
        .as_i64()
        .map(|bps| Arc::new(SharedBandwidthLimiter::new(bps).expect("golden limiter rate")));
    let priority = parse_priority(str_of(case, "priority"));
    let buffer_size = case["buffer_size"]
        .as_u64()
        .map_or(DEFAULT_BUFFER_SIZE, |v| usize::try_from(v).expect("buffer size"));

    let reader = SlowLinkSourceReader::new(inner.clone(), limiter);
    let opened = reader.open_source(Path::new(str_of(case, "inner_path")), priority, buffer_size);

    // The inner reader is called exactly once, with the same three arguments.
    let calls = inner.snapshot();
    assert_eq!(
        calls.count,
        usize::try_from(i64_of(case, "inner_calls")).expect("inner_calls"),
        "{name}: inner open count"
    );
    assert_eq!(
        calls.path.as_deref(),
        Some(Path::new(str_of(case, "inner_path"))),
        "{name}: inner path"
    );
    assert_eq!(
        calls.priority,
        Some(parse_priority(str_of(case, "inner_priority"))),
        "{name}: inner priority"
    );
    assert_eq!(
        calls.buffer_size,
        Some(usize::try_from(i64_of(case, "inner_buffer_size")).expect("inner_buffer_size")),
        "{name}: inner buffer size"
    );

    if let Some(expected) = case["open_throws"].as_str() {
        assert_eq!(expected, "FileNotFoundException", "{name}: golden open failure kind");
        let err = opened.err().unwrap_or_else(|| panic!("{name}: open must fail"));
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "{name}: open error kind");
        return;
    }

    let mut stream = opened.unwrap_or_else(|e| panic!("{name}: open failed: {e}"));
    let ops = case["ops"].as_array().expect("ops array");
    let results = case["op_results"].as_array().expect("op_results array");
    assert_eq!(ops.len(), results.len(), "{name}: one result per op");

    for (index, (op, expected)) in ops.iter().zip(results).enumerate() {
        match str_of(op, "op") {
            "read" => {
                let count = usize::try_from(i64_of(op, "count")).expect("read count");
                let mut buf = vec![0u8; count];
                let read = stream
                    .read(&mut buf)
                    .unwrap_or_else(|e| panic!("{name} op {index}: read failed: {e}"));
                assert_eq!(
                    i64::try_from(read).expect("read fits i64"),
                    i64_of(expected, "read"),
                    "{name} op {index}: bytes read"
                );
                assert_eq!(
                    checksum(&buf[..read]),
                    i64_of(expected, "checksum"),
                    "{name} op {index}: checksum"
                );
            }
            "seek" => {
                let result = seek_to(stream.as_mut(), i64_of(op, "offset"), str_of(op, "origin"));
                if expected.get("throws").is_some() {
                    assert!(result.is_err(), "{name} op {index}: seek must fail");
                } else {
                    let position = result.unwrap_or_else(|e| panic!("{name} op {index}: seek failed: {e}"));
                    assert_eq!(
                        i64::try_from(position).expect("position fits i64"),
                        i64_of(expected, "position"),
                        "{name} op {index}: position"
                    );
                }
            }
            other => panic!("{name}: unknown op {other}"),
        }
    }

    // Final position first (reads and seeks move it), then the length via a seek to the end.
    let final_position = stream.stream_position().expect("final position");
    assert_eq!(
        i64::try_from(final_position).expect("position fits i64"),
        i64_of(case, "final_position"),
        "{name}: final position"
    );
    let final_length = stream.seek(SeekFrom::End(0)).expect("length");
    assert_eq!(
        i64::try_from(final_length).expect("length fits i64"),
        i64_of(case, "final_length"),
        "{name}: final length"
    );
    // `can_read` and `can_seek` are not observable on `ReadSeek` (every stream reads and seeks); the golden values
    // are all `true`, which the successful reads and seeks above already show.
}

// C#: none (new golden test; the C# SlowLinkSourceReader has no test). Golden `slow-link-source-reader`, 15 cases.
#[test]
fn golden_slow_link_source_reader() {
    let golden = golden::load("slow-link-source-reader");
    for case in &golden.cases {
        run_case(case);
    }
}

// New lower-bound timing test (no C# twin). The bucket starts empty, so reading 1000 bytes at 10_000 B/s must take
// at least about 100 ms; the assert accepts 80 ms. Lower bound only: no equality, no fixed delay.
#[test]
fn limiter_throttles_reads_lower_bound() {
    let limiter = Arc::new(SharedBandwidthLimiter::new(10_000).expect("limiter rate"));
    let inner = RecordingReader::new(golden_data(1000), false);
    let reader = SlowLinkSourceReader::new(inner, Some(limiter));
    let mut stream = reader
        .open_source(
            Path::new(r"C:\bench\throttled.jpg"),
            SourceReadPriority::Viewer,
            DEFAULT_BUFFER_SIZE,
        )
        .expect("open through the limiter");

    let started = Instant::now();
    let mut buf = vec![0u8; 1000];
    stream.read_exact(&mut buf).expect("read 1000 bytes");
    let elapsed = started.elapsed();

    assert!(
        elapsed >= Duration::from_millis(80),
        "1000 bytes at 10_000 B/s finished in {elapsed:?}; the limiter did not throttle"
    );
}

// New lower-bound timing test (no C# twin). Two readers built from clones of ONE limiter share its budget: 1000
// bytes in total at 10_000 B/s take at least about 100 ms. Separate limiters would take about 50 ms, so the 80 ms
// lower bound tells them apart. Lower bound only.
#[test]
fn two_readers_sharing_one_limiter_split_the_budget() {
    let limiter = Arc::new(SharedBandwidthLimiter::new(10_000).expect("limiter rate"));
    let first = SlowLinkSourceReader::new(
        RecordingReader::new(golden_data(500), false),
        Some(Arc::clone(&limiter)),
    );
    let second = SlowLinkSourceReader::new(RecordingReader::new(golden_data(500), false), Some(limiter));
    let mut first_stream = first
        .open_source(
            Path::new(r"C:\bench\shared-a.jpg"),
            SourceReadPriority::Viewer,
            DEFAULT_BUFFER_SIZE,
        )
        .expect("open first");
    let mut second_stream = second
        .open_source(
            Path::new(r"C:\bench\shared-b.jpg"),
            SourceReadPriority::Preload,
            DEFAULT_BUFFER_SIZE,
        )
        .expect("open second");

    let started = Instant::now();
    thread::scope(|scope| {
        scope.spawn(move || {
            let mut buf = vec![0u8; 500];
            first_stream.read_exact(&mut buf).expect("first reader: 500 bytes");
        });
        scope.spawn(move || {
            let mut buf = vec![0u8; 500];
            second_stream.read_exact(&mut buf).expect("second reader: 500 bytes");
        });
    });
    let elapsed = started.elapsed();

    assert!(
        elapsed >= Duration::from_millis(80),
        "1000 bytes over two readers at 10_000 B/s finished in {elapsed:?}; the budget was not shared"
    );
}

// Not ported: "without a limiter nothing is throttled" is a behaviour of the pass-through stream itself (no wrapper
// is observable through `Box<dyn ReadSeek>`). The golden cases `no-limiter-*` and `explicit-buffer-size-without-limiter`
// cover the byte, seek and length fidelity of that path.
