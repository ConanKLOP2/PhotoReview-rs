//! Red tests for W7-02a: `pr_bench::slow_link_fs` (port of `PhotoReview.Benchmarking/SlowLinkFileSystem.cs`, C# baseline
//! d92d6aae). Every test fails through `todo!()` until the implementation lands. No test sleeps for real: time goes
//! through [`FakeTime`] or [`ParkingTime`], except the golden `limiter-consume-immediate` cases, which use the real clock
//! and never sleep (they only check that nothing waits).

use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use pr_bench::slow_link_fs::{
    InvalidBandwidth, SharedBandwidthLimiter, SlowLinkFileSystem, SystemTimeSource, TICKS_PER_SECOND,
    ThrottledReadStream, TimeSource,
};
use pr_core::abstractions::{
    AppendStream, CopyCreationProof, FileStat, FileSystem, ReadSeek, SkippedEntry, SkippedKind,
};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::time::DateTimeUtc;
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::{Value, json};

type Log = Arc<Mutex<Vec<String>>>;

fn new_log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Every event so far (inner calls by name, and `"sleep"` for each sleep), in order.
fn events(log: &Log) -> Vec<String> {
    lock(log).clone()
}

/// The inner calls only (events other than `"sleep"`), in order.
fn inner_calls(log: &Log) -> Vec<String> {
    events(log).into_iter().filter(|event| event != "sleep").collect()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// Ticks (100 ns) a duration occupies on the fake clock, rounded up so a sleep always moves the clock forward.
fn ticks_of(duration: Duration) -> i64 {
    i64::try_from(duration.as_nanos().div_ceil(100)).expect("duration fits the tick range")
}

/// The deterministic byte pattern every fixture uses: `(i * 7 + 3) % 251`.
fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from((i * 7 + 3) % 251).expect("below 251"))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn stat(length: u64) -> FileStat {
    FileStat {
        length,
        last_write_utc: DateTimeUtc::parse_json("2026-01-01T00:00:00Z")
            .expect("valid literal")
            .0,
    }
}

/// The last path segment as text (what the golden listings print).
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Fake clock: time moves only through [`FakeTime::advance`] / [`FakeTime::set_ticks`] and through `sleep`, which
/// records the duration and advances the clock by it. Sleeps are also written to the shared event log.
#[derive(Debug)]
struct FakeTime {
    ticks: AtomicI64,
    now_calls: AtomicUsize,
    sleeps: Mutex<Vec<Duration>>,
    log: Log,
}

impl FakeTime {
    fn new(log: Log) -> Arc<Self> {
        Arc::new(Self {
            ticks: AtomicI64::new(0),
            now_calls: AtomicUsize::new(0),
            sleeps: Mutex::new(Vec::new()),
            log,
        })
    }

    fn advance(&self, by: Duration) {
        self.ticks.fetch_add(ticks_of(by), Ordering::SeqCst);
    }

    fn set_ticks(&self, ticks: i64) {
        self.ticks.store(ticks, Ordering::SeqCst);
    }

    fn slept(&self) -> Vec<Duration> {
        lock(&self.sleeps).clone()
    }

    fn total(&self) -> Duration {
        self.slept().iter().sum()
    }

    fn now_calls(&self) -> usize {
        self.now_calls.load(Ordering::SeqCst)
    }
}

impl TimeSource for FakeTime {
    fn now_ticks(&self) -> i64 {
        self.now_calls.fetch_add(1, Ordering::SeqCst);
        self.ticks.load(Ordering::SeqCst)
    }

    fn sleep(&self, duration: Duration) {
        lock(&self.log).push("sleep".to_owned());
        lock(&self.sleeps).push(duration);
        self.advance(duration);
    }
}

/// Clock whose FIRST sleep parks until the test releases it (a bounded wait, so a broken build fails instead of hanging).
struct ParkingTime {
    ticks: AtomicI64,
    parked: AtomicBool,
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl TimeSource for ParkingTime {
    fn now_ticks(&self) -> i64 {
        self.ticks.load(Ordering::SeqCst)
    }

    fn sleep(&self, duration: Duration) {
        if !self.parked.swap(true, Ordering::SeqCst) {
            let _ = lock(&self.entered).send(());
            let _ = lock(&self.release).recv_timeout(Duration::from_secs(30));
        }
        self.ticks.fetch_add(ticks_of(duration), Ordering::SeqCst);
    }
}

fn limiter(bytes_per_second: i64, time: &Arc<FakeTime>) -> Arc<SharedBandwidthLimiter> {
    Arc::new(SharedBandwidthLimiter::with_time_source(bytes_per_second, time.clone()).expect("valid bandwidth"))
}

/// Inner [`FileSystem`] that records every call (method name into the shared log, `method:args` into `args`) and returns
/// canned values. Each argument is shown as its last path segment (`a.jpg`, `folder`), `>`-joined for two paths.
struct RecordingFs {
    log: Log,
    args: Mutex<Vec<String>>,
    payload: Vec<u8>,
    listing: Vec<(PathBuf, Option<FileStat>)>,
    fail_listing: bool,
    skip_one: bool,
    include_answers: Mutex<Vec<bool>>,
    proof_addr: Mutex<Option<usize>>,
    fail_stat: bool,
    fail_resolve: bool,
    open_error: Option<io::ErrorKind>,
}

impl RecordingFs {
    fn new(log: Log) -> Self {
        Self {
            log,
            args: Mutex::new(Vec::new()),
            payload: pattern(100),
            listing: Vec::new(),
            fail_listing: false,
            skip_one: false,
            include_answers: Mutex::new(Vec::new()),
            proof_addr: Mutex::new(None),
            fail_stat: false,
            fail_resolve: false,
            open_error: None,
        }
    }

    fn with_listing(log: Log, listing: Vec<(PathBuf, Option<FileStat>)>) -> Self {
        Self {
            listing,
            ..Self::new(log)
        }
    }

    fn note(&self, method: &str, args: String) {
        lock(&self.log).push(method.to_owned());
        lock(&self.args).push(format!("{method}:{args}"));
    }
}

/// Writer used only so `open_append` has something to return.
struct NullAppend;

impl Write for NullAppend {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AppendStream for NullAppend {}

impl FileSystem for RecordingFs {
    fn file_exists(&self, path: &Path) -> bool {
        self.note("file_exists", name_of(path));
        true
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.note("directory_exists", name_of(path));
        true
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.note("get_file_stat", name_of(path));
        Some(stat(7))
    }

    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        self.note("try_get_file_stat", name_of(path));
        if self.fail_stat {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        }
        Ok(Some(stat(7)))
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.note("move_file", format!("{}>{}", name_of(source), name_of(destination)));
        Ok(())
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.note("copy_file", format!("{}>{}", name_of(source), name_of(destination)));
        Ok(())
    }

    fn try_copy_new(&self, source: &Path, destination: &Path) -> io::Result<bool> {
        self.note("try_copy_new", format!("{}>{}", name_of(source), name_of(destination)));
        Ok(true)
    }

    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        self.note(
            "try_copy_new_with_proof",
            format!("{}>{}", name_of(source), name_of(destination)),
        );
        *lock(&self.proof_addr) = Some(proof as *const CopyCreationProof as usize);
        proof.mark_created();
        Ok(true)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        self.note("delete", name_of(path));
        Ok(())
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.note("open_read_shared", format!("{},{buffer_size}", name_of(path)));
        if let Some(kind) = self.open_error {
            return Err(io::Error::from(kind));
        }
        Ok(Box::new(Cursor::new(self.payload.clone())))
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.note("open_append", format!("{},durable={durable}", name_of(path)));
        Ok(Box::new(NullAppend))
    }

    fn write_all_text_atomic(&self, path: &Path, _text: &str, durable: bool) -> io::Result<()> {
        self.note("write_all_text_atomic", format!("{},durable={durable}", name_of(path)));
        Ok(())
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.note("read_all_text", name_of(path));
        Ok("text".to_owned())
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.note("enumerate_files", format!("{},{pattern}", name_of(directory)));
        Ok(Vec::new())
    }

    fn enumerate_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        self.note("enumerate_files_with_stat", name_of(directory));
        lock(&self.include_answers).push(include(Path::new("probe.jpg")));
        if self.skip_one {
            on_skipped(SkippedEntry {
                path: PathBuf::from("skipped.jpg"),
                reason: "locked".to_owned(),
                kind: SkippedKind::FileUnreadable,
            });
        }
        if self.fail_listing {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(self.listing.clone())
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        self.note("try_probe_readable", name_of(path));
        Ok(())
    }

    fn enumerate_readable_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        self.note("enumerate_readable_files_with_stat", name_of(directory));
        lock(&self.include_answers).push(include(Path::new("probe.jpg")));
        if self.skip_one {
            on_skipped(SkippedEntry {
                path: PathBuf::from("skipped.jpg"),
                reason: "locked".to_owned(),
                kind: SkippedKind::FileUnreadable,
            });
        }
        if self.fail_listing {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(self.listing.clone())
    }

    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.note("enumerate_directories", name_of(directory));
        Ok(Vec::new())
    }

    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.note("create_directory", name_of(path));
        Ok(())
    }

    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        self.note("try_delete_empty_directory", name_of(path));
        true
    }

    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.note("resolve_real_path", name_of(path));
        if self.fail_resolve {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        }
        Ok(path.to_path_buf())
    }
}

/// The six methods that sleep exactly once BEFORE the inner call (fixed latency, no per-entry charge).
const DELAYED_BEFORE: [&str; 6] = [
    "file_exists",
    "directory_exists",
    "get_file_stat",
    "try_get_file_stat",
    "try_probe_readable",
    "resolve_real_path",
];

/// One call per [`FileSystem`] method, through the decorator. `dir` is the folder every listing uses.
fn drive(method: &str, fs: &dyn FileSystem, dir: &Path) {
    let a = dir.join("a.jpg");
    let b = dir.join("b.jpg");
    let new = dir.join("new");
    let proof = CopyCreationProof::new();
    match method {
        "file_exists" => {
            fs.file_exists(&a);
        }
        "directory_exists" => {
            fs.directory_exists(&a);
        }
        "get_file_stat" => {
            fs.get_file_stat(&a);
        }
        "try_get_file_stat" => {
            let _ = fs.try_get_file_stat(&a);
        }
        "move_file" => {
            let _ = fs.move_file(&a, &b);
        }
        "copy_file" => {
            let _ = fs.copy_file(&a, &b);
        }
        "try_copy_new" => {
            let _ = fs.try_copy_new(&a, &b);
        }
        "try_copy_new_with_proof" => {
            let _ = fs.try_copy_new_with_proof(&a, &b, &proof);
        }
        "delete" => {
            let _ = fs.delete(&a);
        }
        "open_read_shared" => {
            let _ = fs.open_read_shared(&a, 4096);
        }
        "open_append" => {
            let _ = fs.open_append(&a, true);
        }
        "write_all_text_atomic" => {
            let _ = fs.write_all_text_atomic(&a, "text", false);
        }
        "read_all_text" => {
            let _ = fs.read_all_text(&a);
        }
        "enumerate_files" => {
            let _ = fs.enumerate_files(dir, "*.jpg");
        }
        "enumerate_files_with_stat" => {
            let _ = fs.enumerate_files_with_stat(dir, &|_: &Path| true, &mut |_: SkippedEntry| {});
        }
        "try_probe_readable" => {
            let _ = fs.try_probe_readable(&a);
        }
        "enumerate_readable_files_with_stat" => {
            let _ = fs.enumerate_readable_files_with_stat(dir, &|_: &Path| true, &mut |_: SkippedEntry| {});
        }
        "enumerate_directories" => {
            let _ = fs.enumerate_directories(dir);
        }
        "create_directory" => {
            let _ = fs.create_directory(&new);
        }
        "try_delete_empty_directory" => {
            fs.try_delete_empty_directory(&new);
        }
        "resolve_real_path" => {
            let _ = fs.resolve_real_path(&a);
        }
        other => panic!("no drive for trait method {other}"),
    }
}

/// Golden input cases of one `kind`, each paired with its expected output case (matched by `name`).
fn golden_cases(kind: &str) -> Vec<(Value, Value)> {
    let inputs_path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("slow-link-fs.json");
    let text = std::fs::read_to_string(&inputs_path).unwrap_or_else(|e| panic!("read {}: {e}", inputs_path.display()));
    let inputs: Value = serde_json::from_str(&text).expect("golden input parses");
    let expected = golden::load("slow-link-fs").cases;
    inputs["cases"]
        .as_array()
        .expect("golden input has cases")
        .iter()
        .filter(|case| case["kind"].as_str() == Some(kind))
        .map(|input| {
            let name = str_of(input, "name");
            let output = expected
                .iter()
                .find(|case| case["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("golden has no output for case {name}"))
                .clone();
            (input.clone(), output)
        })
        .collect()
}

fn sorted_names(paths: Vec<PathBuf>) -> Value {
    let mut rows: Vec<String> = paths.iter().map(|path| name_of(path)).collect();
    rows.sort();
    json!(rows)
}

fn sorted_entries(list: Vec<(PathBuf, Option<FileStat>)>) -> Value {
    let mut rows: Vec<(String, Option<u64>)> = list
        .into_iter()
        .map(|(path, stat)| (name_of(&path), stat.map(|stat| stat.length)))
        .collect();
    rows.sort();
    Value::Array(
        rows.into_iter()
            .map(|(name, length)| json!({"name": name, "length": length}))
            .collect(),
    )
}

fn map_of(names: &[String], value: impl Fn(&str) -> Value) -> Value {
    Value::Object(names.iter().map(|name| (name.clone(), value(name))).collect())
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("list of names")
        .iter()
        .map(|name| name.as_str().expect("name is a string").to_owned())
        .collect()
}

/// Replays golden `ops` on a stream; returns the observed results and whether any byte was read.
fn replay(stream: &mut dyn ReadSeek, ops: &[Value]) -> (Vec<Value>, bool) {
    let mut results = Vec::new();
    let mut read_any = false;
    for op in ops {
        match str_of(op, "op") {
            "read" => {
                let count = usize::try_from(i64_of(op, "count")).expect("count is not negative");
                let mut buf = vec![0u8; count];
                let n = stream.read(&mut buf).expect("read");
                read_any |= n > 0;
                results.push(json!({"op": "read", "n": n, "hex": hex(&buf[..n.min(8)])}));
            }
            "position" => {
                let position = stream.stream_position().expect("position");
                results.push(json!({"op": "position", "position": position}));
            }
            "seek" => {
                let offset = i64_of(op, "offset");
                let target = match str_of(op, "origin") {
                    "begin" => SeekFrom::Start(u64::try_from(offset).expect("begin offset")),
                    "current" => SeekFrom::Current(offset),
                    "end" => SeekFrom::End(offset),
                    other => panic!("unknown origin {other}"),
                };
                let position = stream.seek(target).expect("seek");
                results.push(json!({"op": "seek", "position": position}));
            }
            "length" => {
                let current = stream.stream_position().expect("position");
                let length = stream.seek(SeekFrom::End(0)).expect("seek to end");
                stream.seek(SeekFrom::Start(current)).expect("restore position");
                results.push(json!({"op": "length", "length": length}));
            }
            "read_to_end" => {
                let mut all = Vec::new();
                let n = stream.read_to_end(&mut all).expect("read_to_end");
                read_any |= n > 0;
                let sum: u64 = all.iter().map(|&byte| u64::from(byte)).sum();
                results.push(json!({"op": "read_to_end", "n": n, "sum": sum}));
            }
            other => panic!("unknown op {other}"),
        }
    }
    (results, read_any)
}

/// Inner stream whose every read fails.
struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
    }
}

impl Seek for FailingReader {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

/// Inner stream that sets a flag when dropped (C# `Dispose` closes the inner stream; Rust: `Drop`).
struct DropFlag {
    dropped: Arc<AtomicBool>,
}

impl Read for DropFlag {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}

impl Seek for DropFlag {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

fn two_entries() -> Vec<(PathBuf, Option<FileStat>)> {
    vec![
        (PathBuf::from("folder/a.jpg"), Some(stat(10))),
        (PathBuf::from("folder/b.jpg"), Some(stat(20))),
    ]
}

// ---------------------------------------------------------------------------------------------------------------------
// 1. Limiter, fake clock, exact sleeps (NEW)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn limiter_rejects_non_positive_rate() {
    // NEW: C# `ArgumentOutOfRangeException` from the `SharedBandwidthLimiter` constructor.
    let time = FakeTime::new(new_log());
    for bad in [0_i64, -1, i64::MIN] {
        let err = SharedBandwidthLimiter::with_time_source(bad, time.clone())
            .expect_err("non-positive rate must be rejected");
        assert_eq!(err, InvalidBandwidth { bytes_per_second: bad });
        assert_eq!(
            err.to_string(),
            format!("bytesPerSecond must be greater than zero (was {bad})")
        );
    }
    for ok in [1_i64, i64::MAX] {
        assert!(
            SharedBandwidthLimiter::with_time_source(ok, time.clone()).is_ok(),
            "rate {ok} is valid"
        );
    }
}

#[test]
fn bucket_starts_empty() {
    // NEW: the bucket starts EMPTY, so the first request waits for its full size; a sub-millisecond wait is raised to 1 ms.
    let time = FakeTime::new(new_log());
    limiter(1000, &time).consume(100);
    assert_eq!(time.slept(), vec![ms(100)]);

    let time = FakeTime::new(new_log());
    limiter(1_000_000, &time).consume(1);
    assert_eq!(
        time.slept(),
        vec![ms(1)],
        "the exact wait is 1 us, the floor makes it 1 ms"
    );
}

#[test]
fn bucket_is_capped_at_one_second() {
    // NEW: after 10 s idle the bucket holds one second's budget (1000 bytes), not 10 000.
    let time = FakeTime::new(new_log());
    let bandwidth = limiter(1000, &time);
    time.advance(Duration::from_secs(10));
    bandwidth.consume(500);
    assert!(time.slept().is_empty(), "500 of 1000 available: no wait");
    bandwidth.consume(600);
    assert_eq!(time.slept(), vec![ms(100)], "500 left, 100 missing for 600");
}

#[test]
fn request_larger_than_budget_is_chunked() {
    // C# `Limiter_ReadLargerThanPerSecondBudget_Completes` (fake-clock twin): the request is metered in chunks of at most
    // one second's budget, so the bucket never has to hold more than it can.
    let time = FakeTime::new(new_log());
    limiter(1000, &time).consume(2500);
    assert_eq!(
        time.slept(),
        vec![Duration::from_secs(1), Duration::from_secs(1), ms(500)]
    );

    let time = FakeTime::new(new_log());
    limiter(100_000, &time).consume(250_000);
    assert_eq!(time.total(), ms(2500), "250 000 bytes at 100 kB/s take 2.5 s");
}

#[test]
fn consume_of_non_positive_returns_at_once() {
    // NEW: `bytes <= 0` returns before taking the lock or reading the clock.
    let time = FakeTime::new(new_log());
    let bandwidth = limiter(1000, &time);
    let clock_reads_before = time.now_calls();
    bandwidth.consume(0);
    bandwidth.consume(-1);
    bandwidth.consume(i32::MIN);
    assert!(time.slept().is_empty());
    assert_eq!(
        time.now_calls(),
        clock_reads_before,
        "a non-positive consume must not read the clock"
    );
}

#[test]
fn refill_resets_last_ticks_on_a_backwards_clock() {
    // NEW: a clock that jumps back must not make the next refill count from the old, later origin.
    let time = FakeTime::new(new_log());
    time.set_ticks(1000 * TICKS_PER_SECOND);
    let bandwidth = limiter(1000, &time); // last_ticks = 1000 s
    time.set_ticks(500 * TICKS_PER_SECOND); // the clock steps back 500 s
    bandwidth.consume(1); // elapsed <= 0: last_ticks becomes 500 s; 1 byte missing -> 1 ms sleep
    assert_eq!(time.slept(), vec![ms(1)]);

    time.advance(Duration::from_secs(1));
    bandwidth.consume(1000); // 1 s since 500.001 s refills the full budget: no sleep
    assert_eq!(
        time.slept(),
        vec![ms(1)],
        "elapsed time must be counted from 500 s, not from 1000 s"
    );
}

#[test]
fn exact_availability_is_enough() {
    // NEW: `available >= bytes` (not `>`): exactly enough tokens is served without a sleep.
    let time = FakeTime::new(new_log());
    let bandwidth = limiter(1000, &time);
    time.advance(ms(100));
    bandwidth.consume(100);
    assert!(time.slept().is_empty());
}

#[test]
fn sleep_happens_outside_the_lock() {
    // NEW: a reader parked in its sleep must not hold the bucket lock; a second consumer proceeds meanwhile.
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let time = Arc::new(ParkingTime {
        ticks: AtomicI64::new(0),
        parked: AtomicBool::new(false),
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release_rx),
    });
    let bandwidth = Arc::new(SharedBandwidthLimiter::with_time_source(1000, time.clone()).expect("valid bandwidth"));

    let parked = {
        let bandwidth = bandwidth.clone();
        thread::spawn(move || bandwidth.consume(100))
    };
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the first consume must park in its sleep");

    time.ticks.fetch_add(TICKS_PER_SECOND, Ordering::SeqCst); // one second passes while the first reader sleeps
    let (done_tx, done_rx) = mpsc::channel();
    let other = {
        let bandwidth = bandwidth.clone();
        thread::spawn(move || {
            bandwidth.consume(10);
            let _ = done_tx.send(());
        })
    };
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("consume(10) must not wait behind the parked sleep (sleep outside the lock)");
    other.join().expect("second consumer thread");

    release_tx.send(()).expect("release the parked sleep");
    parked.join().expect("first consumer thread");
}

#[test]
fn limiter_is_shared_between_streams() {
    // NEW: two streams on one limiter draw from one bucket: 10 + 10 bytes take the time of 20 bytes, not of 10.
    let time = FakeTime::new(new_log());
    let bandwidth = limiter(1000, &time);
    let data = pattern(100);
    let mut first = ThrottledReadStream::new(Box::new(Cursor::new(data.clone())), bandwidth.clone());
    let mut second = ThrottledReadStream::new(Box::new(Cursor::new(data.clone())), bandwidth.clone());
    let mut buf = [0u8; 10];
    assert_eq!(first.read(&mut buf).expect("first read"), 10);
    assert_eq!(second.read(&mut buf).expect("second read"), 10);
    assert_eq!(time.total(), ms(20), "20 bytes at 1000 B/s");
}

// ---------------------------------------------------------------------------------------------------------------------
// 2. Throttled stream (NEW)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn throttled_read_returns_inner_bytes_and_meters_them() {
    // NEW: a read returns the inner bytes and is charged after the inner read (10 bytes at 1000 B/s = 10 ms).
    let time = FakeTime::new(new_log());
    let data = pattern(100);
    let mut stream = ThrottledReadStream::new(Box::new(Cursor::new(data.clone())), limiter(1000, &time));
    let mut buf = [0u8; 10];
    assert_eq!(stream.read(&mut buf).expect("read"), 10);
    assert_eq!(buf.to_vec(), data[..10].to_vec());
    assert_eq!(time.slept(), vec![ms(10)]);
}

#[test]
fn zero_length_read_and_eof_cost_nothing() {
    // NEW: `read > 0` guard: an empty buffer and end of file consume no budget and sleep nothing.
    let time = FakeTime::new(new_log());
    let mut stream = ThrottledReadStream::new(Box::new(Cursor::new(pattern(4))), limiter(1000, &time));
    let mut empty: [u8; 0] = [];
    assert_eq!(stream.read(&mut empty).expect("empty read"), 0);
    let mut buf = [0u8; 4];
    assert_eq!(stream.read(&mut buf).expect("read to the end"), 4);
    let slept_before_eof = time.slept().len();
    assert_eq!(stream.read(&mut buf).expect("read at EOF"), 0);
    assert_eq!(time.slept().len(), slept_before_eof, "EOF read sleeps nothing");
}

#[test]
fn inner_read_error_is_returned_and_consumes_nothing() {
    // NEW: the inner read happens first; its error is returned as is and nothing is metered.
    let time = FakeTime::new(new_log());
    let mut stream = ThrottledReadStream::new(Box::new(FailingReader), limiter(1000, &time));
    let mut buf = [0u8; 10];
    let err = stream.read(&mut buf).expect_err("inner error is returned");
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(time.slept().is_empty(), "a failed read consumes nothing");
}

#[test]
fn seek_is_forwarded_and_free() {
    // NEW: seeking, position and length are free; the inner position is returned.
    let time = FakeTime::new(new_log());
    let mut stream = ThrottledReadStream::new(Box::new(Cursor::new(pattern(100))), limiter(1000, &time));
    assert_eq!(stream.seek(SeekFrom::Start(5)).expect("seek start"), 5);
    assert_eq!(stream.seek(SeekFrom::Current(3)).expect("seek current"), 8);
    assert_eq!(stream.seek(SeekFrom::End(-10)).expect("seek end"), 90);
    assert_eq!(stream.stream_position().expect("position"), 90);
    assert!(time.slept().is_empty());
}

#[test]
fn read_to_end_is_metered() {
    // NEW: `read_to_end` goes through `read`, so 100 bytes at 1000 B/s take about 100 ms (chunks may add 1 ms floors).
    let time = FakeTime::new(new_log());
    let data = pattern(100);
    let mut stream = ThrottledReadStream::new(Box::new(Cursor::new(data.clone())), limiter(1000, &time));
    let mut all = Vec::new();
    stream.read_to_end(&mut all).expect("read_to_end");
    assert_eq!(all, data);
    let total = time.total();
    assert!(total >= ms(100), "total {total:?} must cover 100 bytes");
    assert!(
        total <= ms(105),
        "total {total:?} must not exceed 100 bytes plus floors"
    );
}

#[test]
fn dropping_the_stream_drops_the_inner_stream() {
    // NEW: `Dispose` in C# closes the inner stream; in Rust dropping the wrapper drops the boxed inner.
    let time = FakeTime::new(new_log());
    let dropped = Arc::new(AtomicBool::new(false));
    let stream = ThrottledReadStream::new(
        Box::new(DropFlag {
            dropped: dropped.clone(),
        }),
        limiter(1000, &time),
    );
    assert!(!dropped.load(Ordering::SeqCst));
    drop(stream);
    assert!(
        dropped.load(Ordering::SeqCst),
        "the inner stream is dropped with the wrapper"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// 3. Decorator, fake clock and RecordingFs (C# SlowLinkFileSystemTests + NEW)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn enumerate_readable_applies_latency_per_entry() {
    // C# `SlowLinkFileSystemTests.EnumerateReadableFilesWithStat_AppliesMetadataLatencyPerEntry`: two entries, 250 ms each.
    let log = new_log();
    let time = FakeTime::new(log.clone());
    let inner = Arc::new(RecordingFs::with_listing(log.clone(), two_entries()));
    let fs = SlowLinkFileSystem::with_time_source(inner, ms(250), None, time.clone());

    let listed = fs
        .enumerate_readable_files_with_stat(Path::new("folder"), &|_: &Path| true, &mut |_: SkippedEntry| {})
        .expect("listing");
    assert_eq!(listed.len(), 2);
    assert_eq!(time.slept(), vec![ms(250), ms(250)]);
    assert_eq!(inner_calls(&log), names(&["enumerate_readable_files_with_stat"]));
    assert_eq!(
        events(&log),
        names(&["enumerate_readable_files_with_stat", "sleep", "sleep"]),
        "the inner listing runs first, then one sleep per entry"
    );
}

#[test]
fn enumerate_files_applies_latency_per_entry_and_forwards_include_and_on_skipped() {
    // NEW: the same per-entry charge for `enumerate_files_with_stat`; `include` and `on_skipped` reach the inner call.
    let log = new_log();
    let time = FakeTime::new(log.clone());
    let inner = Arc::new(RecordingFs {
        skip_one: true,
        ..RecordingFs::with_listing(log.clone(), two_entries())
    });
    let fs = SlowLinkFileSystem::with_time_source(inner.clone(), ms(250), None, time.clone());

    let mut skipped: Vec<PathBuf> = Vec::new();
    let listed = fs
        .enumerate_files_with_stat(
            Path::new("folder"),
            &|path: &Path| path.ends_with("keep.jpg"),
            &mut |entry: SkippedEntry| skipped.push(entry.path),
        )
        .expect("listing");
    assert_eq!(listed.len(), 2);
    assert_eq!(time.slept(), vec![ms(250), ms(250)]);
    assert_eq!(inner_calls(&log), names(&["enumerate_files_with_stat"]));
    assert_eq!(
        *lock(&inner.include_answers),
        vec![false],
        "the include predicate reached the inner call"
    );
    assert_eq!(
        skipped,
        vec![PathBuf::from("skipped.jpg")],
        "on_skipped reached the inner call"
    );
}

#[test]
fn empty_listing_sleeps_nothing() {
    // NEW: zero entries means zero charges (both enumerate overloads).
    let log = new_log();
    let time = FakeTime::new(log.clone());
    let fs = SlowLinkFileSystem::with_time_source(Arc::new(RecordingFs::new(log.clone())), ms(250), None, time.clone());
    let plain = fs
        .enumerate_files_with_stat(Path::new("folder"), &|_: &Path| true, &mut |_: SkippedEntry| {})
        .expect("listing");
    let readable = fs
        .enumerate_readable_files_with_stat(Path::new("folder"), &|_: &Path| true, &mut |_: SkippedEntry| {})
        .expect("listing");
    assert!(plain.is_empty() && readable.is_empty());
    assert!(time.slept().is_empty());
}

#[test]
fn inner_listing_error_is_returned_without_sleep() {
    // NEW: a listing error is returned untouched and charges nothing.
    let log = new_log();
    let time = FakeTime::new(log.clone());
    let inner = Arc::new(RecordingFs {
        fail_listing: true,
        ..RecordingFs::with_listing(log.clone(), two_entries())
    });
    let fs = SlowLinkFileSystem::with_time_source(inner, ms(250), None, time.clone());

    let err = fs
        .enumerate_files_with_stat(Path::new("folder"), &|_: &Path| true, &mut |_: SkippedEntry| {})
        .expect_err("inner error is returned");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(time.slept().is_empty());
    assert_eq!(inner_calls(&log), names(&["enumerate_files_with_stat"]));
}

#[test]
fn delayed_methods_sleep_once_before_the_inner_call() {
    // NEW: `file_exists`, `directory_exists`, `get_file_stat`, `try_get_file_stat`, `try_probe_readable`,
    // `resolve_real_path` sleep once BEFORE the inner call, also when that call then fails.
    for method in DELAYED_BEFORE {
        let log = new_log();
        let time = FakeTime::new(log.clone());
        let fs =
            SlowLinkFileSystem::with_time_source(Arc::new(RecordingFs::new(log.clone())), ms(250), None, time.clone());
        drive(method, &fs, Path::new("folder"));
        assert_eq!(
            events(&log),
            names(&["sleep", method]),
            "{method}: sleep, then inner call"
        );
        assert_eq!(time.slept(), vec![ms(250)], "{method}: exactly one charge");
    }
}

#[test]
fn delayed_methods_return_inner_values_and_errors_unchanged() {
    // NEW: values and errors pass through; a denied stat is never folded into `Ok(None)`.
    let log = new_log();
    let time = FakeTime::new(log.clone());
    let fs = SlowLinkFileSystem::with_time_source(Arc::new(RecordingFs::new(log.clone())), ms(5), None, time.clone());
    assert!(fs.file_exists(Path::new("a.jpg")));
    assert!(fs.directory_exists(Path::new("a.jpg")));
    assert_eq!(fs.get_file_stat(Path::new("a.jpg")).map(|stat| stat.length), Some(7));
    assert_eq!(
        fs.try_get_file_stat(Path::new("a.jpg"))
            .expect("ok")
            .map(|stat| stat.length),
        Some(7)
    );
    assert!(fs.try_probe_readable(Path::new("a.jpg")).is_ok());
    assert_eq!(
        fs.resolve_real_path(Path::new("a.jpg")).expect("resolved"),
        PathBuf::from("a.jpg")
    );

    let denied_log = new_log();
    let denied_time = FakeTime::new(denied_log.clone());
    let denied = SlowLinkFileSystem::with_time_source(
        Arc::new(RecordingFs {
            fail_stat: true,
            ..RecordingFs::new(denied_log.clone())
        }),
        ms(5),
        None,
        denied_time.clone(),
    );
    let err = denied
        .try_get_file_stat(Path::new("a.jpg"))
        .expect_err("a denied stat stays an error");
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(
        denied_time.slept(),
        vec![ms(5)],
        "the charge happens even when the inner call fails"
    );

    let failing_log = new_log();
    let failing_time = FakeTime::new(failing_log.clone());
    let failing = SlowLinkFileSystem::with_time_source(
        Arc::new(RecordingFs {
            fail_resolve: true,
            ..RecordingFs::new(failing_log.clone())
        }),
        ms(5),
        None,
        failing_time.clone(),
    );
    let err = failing
        .resolve_real_path(Path::new("a.jpg"))
        .expect_err("inner error is returned");
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(failing_time.slept(), vec![ms(5)]);
}

#[test]
fn zero_latency_never_sleeps() {
    // NEW: `Duration::ZERO` means no `sleep` call at all (C# `> TimeSpan.Zero`), every delayed method included.
    let log = new_log();
    let time = FakeTime::new(log.clone());
    let fs = SlowLinkFileSystem::with_time_source(
        Arc::new(RecordingFs::with_listing(log.clone(), two_entries())),
        Duration::ZERO,
        None,
        time.clone(),
    );
    for method in DELAYED_BEFORE {
        drive(method, &fs, Path::new("folder"));
    }
    drive("enumerate_files_with_stat", &fs, Path::new("folder"));
    drive("enumerate_readable_files_with_stat", &fs, Path::new("folder"));
    assert!(time.slept().is_empty(), "no sleep at zero latency");
    assert!(!events(&log).iter().any(|event| event == "sleep"));
}

const FORWARDING_TABLE: [(&str, &str); 21] = [
    ("file_exists", "a.jpg"),
    ("directory_exists", "a.jpg"),
    ("get_file_stat", "a.jpg"),
    ("try_get_file_stat", "a.jpg"),
    ("move_file", "a.jpg>b.jpg"),
    ("copy_file", "a.jpg>b.jpg"),
    ("try_copy_new", "a.jpg>b.jpg"),
    ("try_copy_new_with_proof", "a.jpg>b.jpg"),
    ("delete", "a.jpg"),
    ("open_read_shared", "a.jpg,4096"),
    ("open_append", "a.jpg,durable=true"),
    ("write_all_text_atomic", "a.jpg,durable=false"),
    ("read_all_text", "a.jpg"),
    ("enumerate_files", "folder,*.jpg"),
    ("enumerate_files_with_stat", "folder"),
    ("try_probe_readable", "a.jpg"),
    ("enumerate_readable_files_with_stat", "folder"),
    ("enumerate_directories", "folder"),
    ("create_directory", "new"),
    ("try_delete_empty_directory", "new"),
    ("resolve_real_path", "a.jpg"),
];

#[test]
fn every_trait_method_is_forwarded_exactly_once() {
    // C# `FileSystemWrapperForwardingTests.EveryFileSystemDecorator_ForwardsTheCreateNewPrimitivesToTheInnerFileSystem`
    // (the `SlowLinkFileSystem` half). Per method: exactly one inner call of the same name (no default body ran), the
    // arguments arrive unchanged, and only the metadata methods sleep (before the call).
    for (method, expected_args) in FORWARDING_TABLE {
        let log = new_log();
        let time = FakeTime::new(log.clone());
        let inner = Arc::new(RecordingFs::new(log.clone()));
        let fs = SlowLinkFileSystem::with_time_source(inner.clone(), ms(5), None, time.clone());
        drive(method, &fs, Path::new("folder"));
        let expected_events = if DELAYED_BEFORE.contains(&method) {
            names(&["sleep", method])
        } else {
            names(&[method])
        };
        assert_eq!(events(&log), expected_events, "{method}: events");
        assert_eq!(
            *lock(&inner.args),
            vec![format!("{method}:{expected_args}")],
            "{method}: arguments"
        );
    }
}

#[test]
fn try_copy_new_with_proof_forwards_the_same_proof() {
    // C# `FileSystemWrapperForwardingTests` (create-new half): the proof reference is passed through, not copied.
    let log = new_log();
    let time = FakeTime::new(log.clone());
    let inner = Arc::new(RecordingFs::new(log.clone()));
    let fs = SlowLinkFileSystem::with_time_source(inner.clone(), Duration::ZERO, None, time.clone());
    let proof = CopyCreationProof::new();
    assert!(
        fs.try_copy_new_with_proof(Path::new("a.jpg"), Path::new("b.jpg"), &proof)
            .expect("copy")
    );
    assert!(proof.destination_created(), "the caller's proof is the one marked");
    assert_eq!(
        *lock(&inner.proof_addr),
        Some(&proof as *const CopyCreationProof as usize),
        "the inner call received the same proof object"
    );
    assert_eq!(inner_calls(&log), names(&["try_copy_new_with_proof"]));
}

#[test]
fn open_read_shared_wraps_only_with_a_limiter() {
    // NEW: no limiter: the inner stream is returned as is (free); with a limiter: 100 bytes at 1000 B/s take 100 ms.
    let log = new_log();
    let time = FakeTime::new(log.clone());
    let inner = Arc::new(RecordingFs::new(log.clone()));
    let plain = SlowLinkFileSystem::with_time_source(inner.clone(), Duration::ZERO, None, time.clone());
    let mut stream = plain.open_read_shared(Path::new("a.jpg"), 4096).expect("open");
    let mut buf = vec![0u8; 100];
    stream.read_exact(&mut buf).expect("read");
    assert_eq!(buf, pattern(100));
    assert!(time.slept().is_empty(), "no limiter: nothing is metered");
    assert_eq!(
        *lock(&inner.args),
        vec!["open_read_shared:a.jpg,4096".to_owned()],
        "buffer size reaches the inner call"
    );

    let time = FakeTime::new(new_log());
    let metered = SlowLinkFileSystem::with_time_source(
        Arc::new(RecordingFs::new(new_log())),
        Duration::ZERO,
        Some(limiter(1000, &time)),
        time.clone(),
    );
    let mut stream = metered.open_read_shared(Path::new("a.jpg"), 4096).expect("open");
    let mut buf = vec![0u8; 100];
    let n = stream.read(&mut buf).expect("read");
    assert_eq!(n, 100);
    assert_eq!(buf, pattern(100), "the bytes are identical");
    assert_eq!(time.slept(), vec![ms(100)], "metered at 1000 B/s");

    let time = FakeTime::new(new_log());
    let failing = SlowLinkFileSystem::with_time_source(
        Arc::new(RecordingFs {
            open_error: Some(io::ErrorKind::NotFound),
            ..RecordingFs::new(new_log())
        }),
        Duration::ZERO,
        Some(limiter(1000, &time)),
        time.clone(),
    );
    let kind = match failing.open_read_shared(Path::new("a.jpg"), 4096) {
        Ok(_) => panic!("the inner open error must be returned"),
        Err(err) => err.kind(),
    };
    assert_eq!(kind, io::ErrorKind::NotFound);
    assert!(time.slept().is_empty(), "an open error charges nothing");
}

// ---------------------------------------------------------------------------------------------------------------------
// 4. Golden `slow-link-fs` (NEW, required; 29 cases, see tests/golden-inputs/slow-link-fs.json)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn golden_limiter_ctor() {
    let cases = golden_cases("limiter-ctor");
    assert_eq!(cases.len(), 6);
    for (input, expected) in cases {
        let bytes_per_second = i64_of(&input, "bytes_per_second");
        let got = match SharedBandwidthLimiter::with_time_source(bytes_per_second, FakeTime::new(new_log())) {
            Ok(_) => Value::Null,
            Err(_) => json!("ArgumentOutOfRangeException"),
        };
        assert_eq!(got, expected["throws"], "case {}", str_of(&input, "name"));
    }
}

#[test]
fn golden_limiter_consume_immediate() {
    // Real clock: the non-positive consumes must return at once.
    for (input, expected) in golden_cases("limiter-consume-immediate") {
        let name = str_of(&input, "name");
        let bandwidth = SharedBandwidthLimiter::new(i64_of(&input, "bytes_per_second")).expect("valid bandwidth");
        let started = Instant::now();
        for bytes in input["bytes"].as_array().expect("bytes list") {
            let bytes = i32::try_from(bytes.as_i64().expect("integer")).expect("i32 request");
            bandwidth.consume(bytes);
        }
        let fast = started.elapsed() < Duration::from_millis(500);
        assert_eq!(json!(true), expected["completed"], "case {name}");
        assert!(fast, "case {name}: non-positive consumes must not wait");
        assert_eq!(json!(fast), expected["fast"], "case {name}");
    }
}

#[test]
fn golden_limiter_consume_lower_bound() {
    for (input, expected) in golden_cases("limiter-consume-lower-bound") {
        let name = str_of(&input, "name");
        let time = FakeTime::new(new_log());
        let bandwidth = limiter(i64_of(&input, "bytes_per_second"), &time);
        let bytes = i32::try_from(i64_of(&input, "bytes")).expect("i32 request");
        bandwidth.consume(bytes);
        let total_ms = u64::try_from(time.total().as_millis()).expect("fits");
        let lower_bound_ms = u64::try_from(i64_of(&input, "lower_bound_ms")).expect("bound");
        let met = total_ms >= lower_bound_ms;
        assert_eq!(json!(true), expected["completed"], "case {name}");
        assert!(met, "case {name}: slept {total_ms} ms, lower bound {lower_bound_ms} ms");
        assert_eq!(json!(met), expected["lower_bound_met"], "case {name}");
    }
}

#[test]
fn golden_fs_forwarding() {
    const KEYS: [&str; 11] = [
        "file_exists",
        "directory_exists",
        "stat_length",
        "probe_readable",
        "enumerate_with_stat",
        "enumerate_with_stat_filtered_to_jpg",
        "enumerate_readable_with_stat",
        "enumerate_files",
        "enumerate_directories",
        "resolve_real_path_keeps_name",
        "skipped",
    ];
    for (input, expected) in golden_cases("fs-forwarding") {
        let name = str_of(&input, "name");
        let root = TempRoot::new("slow-link-fs-forwarding");
        for file in input["files"].as_array().expect("files") {
            let size = usize::try_from(i64_of(file, "size")).expect("size");
            std::fs::write(root.join(str_of(file, "name")), vec![0u8; size]).expect("write fixture");
        }
        for dir in string_list(&input["dirs"]) {
            std::fs::create_dir_all(root.join(dir)).expect("create fixture folder");
        }

        let time = FakeTime::new(new_log());
        let bandwidth = input
            .get("bandwidth_bps")
            .and_then(Value::as_i64)
            .map(|bytes_per_second| limiter(bytes_per_second, &time));
        let fs =
            SlowLinkFileSystem::with_time_source(Arc::new(PhysicalFileSystem), Duration::ZERO, bandwidth, time.clone());

        let probe_files = string_list(&input["probe_files"]);
        let probe_dirs = string_list(&input["probe_dirs"]);
        let mut actual = serde_json::Map::new();
        actual.insert(
            "file_exists".into(),
            map_of(&probe_files, |n| json!(fs.file_exists(&root.join(n)))),
        );
        actual.insert(
            "directory_exists".into(),
            map_of(&probe_dirs, |n| json!(fs.directory_exists(&root.join(n)))),
        );
        actual.insert(
            "stat_length".into(),
            map_of(&probe_files, |n| {
                fs.get_file_stat(&root.join(n))
                    .map_or(Value::Null, |stat| json!(stat.length))
            }),
        );
        actual.insert(
            "probe_readable".into(),
            map_of(&probe_files, |n| json!(fs.try_probe_readable(&root.join(n)).is_ok())),
        );

        let mut skipped = 0_usize;
        let all = fs
            .enumerate_files_with_stat(root.path(), &|_: &Path| true, &mut |_: SkippedEntry| skipped += 1)
            .expect("listing");
        actual.insert("enumerate_with_stat".into(), sorted_entries(all));
        let jpg = fs
            .enumerate_files_with_stat(
                root.path(),
                &|path: &Path| name_of(path).ends_with(".jpg"),
                &mut |_: SkippedEntry| skipped += 1,
            )
            .expect("listing");
        actual.insert(
            "enumerate_with_stat_filtered_to_jpg".into(),
            sorted_names(jpg.into_iter().map(|(path, _)| path).collect()),
        );
        let readable = fs
            .enumerate_readable_files_with_stat(root.path(), &|_: &Path| true, &mut |_: SkippedEntry| skipped += 1)
            .expect("listing");
        actual.insert(
            "enumerate_readable_with_stat".into(),
            sorted_names(readable.into_iter().map(|(path, _)| path).collect()),
        );
        let files = fs.enumerate_files(root.path(), "*").expect("listing");
        actual.insert("enumerate_files".into(), sorted_names(files));
        let folders = fs.enumerate_directories(root.path()).expect("listing");
        actual.insert("enumerate_directories".into(), sorted_names(folders));

        let first = probe_files.first().expect("at least one probe file");
        let resolved = fs.resolve_real_path(&root.join(first)).expect("resolve");
        actual.insert(
            "resolve_real_path_keeps_name".into(),
            json!(name_of(&resolved) == *first),
        );
        actual.insert("skipped".into(), json!(skipped));

        let actual = Value::Object(actual);
        for key in KEYS {
            assert_eq!(actual[key], expected[key], "case {name}: field {key}");
        }
        assert!(time.slept().is_empty(), "case {name}: zero latency, nothing charged");
    }
}

#[test]
fn golden_latency_lower_bound() {
    for (input, expected) in golden_cases("latency-lower-bound") {
        let name = str_of(&input, "name");
        let latency_ms = u64::try_from(i64_of(&input, "latency_ms")).expect("latency");
        let file_count = usize::try_from(i64_of(&input, "file_count")).expect("count");
        let repeat = usize::try_from(i64_of(&input, "repeat")).expect("repeat");
        let op = str_of(&input, "op");

        let root = TempRoot::new("slow-link-fs-latency");
        for index in 0..file_count {
            std::fs::write(root.join(format!("f{index}.txt")), b"x").expect("write fixture");
        }
        let time = FakeTime::new(new_log());
        let latency = ms(latency_ms);
        let fs = SlowLinkFileSystem::with_time_source(Arc::new(PhysicalFileSystem), latency, None, time.clone());
        let first = root.join("f0.txt");

        let mut calls = 0_usize;
        for _ in 0..repeat {
            match op {
                "file_exists" => {
                    fs.file_exists(&first);
                    calls += 1;
                }
                "directory_exists" => {
                    fs.directory_exists(root.path());
                    calls += 1;
                }
                "get_file_stat" => {
                    fs.get_file_stat(&first);
                    calls += 1;
                }
                "try_probe_readable" => {
                    let _ = fs.try_probe_readable(&first);
                    calls += 1;
                }
                "resolve_real_path" => {
                    let _ = fs.resolve_real_path(&first);
                    calls += 1;
                }
                "enumerate_files_with_stat" => {
                    calls += fs
                        .enumerate_files_with_stat(root.path(), &|_: &Path| true, &mut |_: SkippedEntry| {})
                        .expect("listing")
                        .len();
                }
                "enumerate_readable_files_with_stat" => {
                    calls += fs
                        .enumerate_readable_files_with_stat(root.path(), &|_: &Path| true, &mut |_: SkippedEntry| {})
                        .expect("listing")
                        .len();
                }
                other => panic!("case {name}: unknown op {other}"),
            }
        }

        assert_eq!(json!(calls), expected["calls"], "case {name}: charged calls");
        let calls_u32 = u32::try_from(calls).expect("call count fits u32");
        assert_eq!(
            time.total(),
            latency * calls_u32,
            "case {name}: every call charged its latency"
        );
        assert!(
            time.total().as_secs_f64() >= latency.as_secs_f64() * calls as f64 * 0.8,
            "case {name}: lower bound"
        );
        assert_eq!(json!(true), expected["lower_bound_met"], "case {name}");
    }
}

#[test]
fn golden_throttled_read() {
    for (input, expected) in golden_cases("throttled-read") {
        let name = str_of(&input, "name");
        let size = usize::try_from(i64_of(&input, "size")).expect("size");
        let buffer_size = input
            .get("buffer_size")
            .and_then(Value::as_u64)
            .map_or(65_536, |size| usize::try_from(size).expect("buffer size"));
        let bandwidth_bps = input.get("bandwidth_bps").and_then(Value::as_i64);

        let root = TempRoot::new("slow-link-fs-throttled");
        let path = root.join("data.bin");
        std::fs::write(&path, pattern(size)).expect("write fixture");

        let time = FakeTime::new(new_log());
        let bandwidth = bandwidth_bps.map(|bytes_per_second| limiter(bytes_per_second, &time));
        let fs =
            SlowLinkFileSystem::with_time_source(Arc::new(PhysicalFileSystem), Duration::ZERO, bandwidth, time.clone());
        let mut stream = fs.open_read_shared(&path, buffer_size).expect("open fixture");
        let (results, read_any) = replay(&mut *stream, input["ops"].as_array().expect("ops"));

        assert_eq!(Value::Array(results), expected["results"], "case {name}: results");
        assert_eq!(
            json!(bandwidth_bps.is_some()),
            expected["is_throttled"],
            "case {name}: is_throttled"
        );
        assert_eq!(json!(true), expected["can_seek"], "case {name}");
        assert_eq!(
            time.slept().is_empty(),
            !(bandwidth_bps.is_some() && read_any),
            "case {name}: metered iff a limiter was given and bytes were read"
        );
    }
}

#[test]
fn golden_open_missing() {
    for (input, expected) in golden_cases("open-missing") {
        let name = str_of(&input, "name");
        let root = TempRoot::new("slow-link-fs-open-missing");
        let path = root.join("no-such-folder").join("x.jpg");
        let time = FakeTime::new(new_log());
        let bandwidth = input
            .get("bandwidth_bps")
            .and_then(Value::as_i64)
            .map(|bytes_per_second| limiter(bytes_per_second, &time));
        let fs =
            SlowLinkFileSystem::with_time_source(Arc::new(PhysicalFileSystem), Duration::ZERO, bandwidth, time.clone());
        let kind = match fs.open_read_shared(&path, 65_536) {
            Ok(_) => panic!("case {name}: opening under a missing folder must fail"),
            Err(err) => err.kind(),
        };
        assert_eq!(kind, io::ErrorKind::NotFound, "case {name}");
        assert!(
            matches!(
                expected["throws"].as_str(),
                Some("DirectoryNotFoundException" | "FileNotFoundException")
            ),
            "case {name}: golden says {}",
            expected["throws"]
        );
        assert!(time.slept().is_empty(), "case {name}: a failed open charges nothing");
    }
}

#[test]
fn system_time_source_tracks_real_elapsed_time() {
    let clock = SystemTimeSource::new();
    let first = clock.now_ticks();
    assert!(first >= 0);
    let started = Instant::now();
    clock.sleep(ms(30));
    let wall = started.elapsed();
    let second = clock.now_ticks();
    assert!(
        wall >= ms(30),
        "sleep must take at least the requested time, took {wall:?}"
    );
    let grown = second - first;
    assert!(grown >= ticks_of(ms(30)), "ticks grew by {grown}");
    // Ticks are 100 ns units: they must not run faster or slower than wall time by orders of magnitude.
    assert!(
        grown <= ticks_of(wall) + ticks_of(ms(200)),
        "ticks grew by {grown}, wall {wall:?}"
    );
}

#[test]
fn bytes_per_second_getter_and_debug_output() {
    let time = FakeTime::new(new_log());
    let bandwidth = limiter(12_345, &time);
    assert_eq!(bandwidth.bytes_per_second(), 12_345);
    assert_eq!(SharedBandwidthLimiter::new(7).expect("valid").bytes_per_second(), 7);
    let text = format!("{bandwidth:?}");
    assert!(
        text.contains("SharedBandwidthLimiter") && text.contains("12345"),
        "{text}"
    );

    let stream = ThrottledReadStream::new(Box::new(Cursor::new(pattern(1))), bandwidth.clone());
    assert!(format!("{stream:?}").contains("ThrottledReadStream"));

    let fs = SlowLinkFileSystem::new(Arc::new(PhysicalFileSystem), ms(5), Some(bandwidth));
    let text = format!("{fs:?}");
    assert!(
        text.contains("SlowLinkFileSystem") && text.contains("metadata_latency"),
        "{text}"
    );
}
