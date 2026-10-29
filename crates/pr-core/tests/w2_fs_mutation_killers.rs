//! Mutation killers for the Wave 2 file-system rows (X-physical-fs, X-counting-fs, D-05, RV-C05, AR16, SEC-01 / F-05,
//! F-23, X-perf-diagnostics). Each test names the mutant (cargo-mutants description) it kills. Fakes and `TempRoot` only;
//! nothing here touches the Recycle Bin and nothing sleeps.
#![cfg(windows)]

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use pr_core::abstractions::FileSystem;
use pr_core::app_paths::AppPaths;
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::session::writer::{Completion, DebounceTimer, RunningTimer, SessionWriter, TimerError, TimerOutcome};
use pr_core::sync::CancelToken;
use pr_test_support::{FakeFileSystem, HookFileSystem, RecordingLog};

/// Hang guard for every wait on another thread.
const HANG: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------------------------------------------
// Session writer doubles (a trimmed copy of the ones in `session_writer.rs`)
// ---------------------------------------------------------------------------------------------------------------

type Waiter = Arc<(Mutex<Option<TimerOutcome>>, Condvar)>;

/// A timer the test fires by hand.
struct ManualTimer {
    waiters: Mutex<Vec<Waiter>>,
}

impl ManualTimer {
    fn fire_all(&self) {
        let waiters = self.waiters.lock().unwrap_or_else(PoisonError::into_inner).clone();
        for waiter in waiters {
            let mut outcome = waiter.0.lock().unwrap_or_else(PoisonError::into_inner);
            if outcome.is_none() {
                *outcome = Some(TimerOutcome::Elapsed);
            }
            waiter.1.notify_all();
        }
    }
}

impl DebounceTimer for ManualTimer {
    fn start(&self, _span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        let waiter: Waiter = Arc::new((Mutex::new(None), Condvar::new()));
        self.waiters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::clone(&waiter));
        Ok(Box::new(ManualRun {
            waiter,
            cancel: cancel.clone(),
        }))
    }
}

struct ManualRun {
    waiter: Waiter,
    cancel: CancelToken,
}

impl RunningTimer for ManualRun {
    fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError> {
        let mut outcome = self.waiter.0.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(result) = *outcome {
                return Ok(result);
            }
            if self.cancel.is_canceled() {
                return Ok(TimerOutcome::Canceled);
            }
            outcome = self
                .waiter
                .1
                .wait_timeout(outcome, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

fn paths() -> AppPaths {
    AppPaths {
        config_file: PathBuf::from(r"C:\data\config.json"),
        journal_file: PathBuf::from(r"C:\data\operations.jsonl"),
        sessions_dir: PathBuf::from(r"C:\data\Sessions"),
        log_file: PathBuf::from(r"C:\data\logs\app.log"),
        preview_cache_dir: PathBuf::from(r"C:\data\cache"),
        thumbnail_cache_dir: PathBuf::from(r"C:\data\thumbnails"),
        window_placement_file: PathBuf::from(r"C:\data\window-placement.json"),
    }
}

fn state(folder: &str, current: &str) -> SessionState {
    SessionState {
        folder: folder.to_owned(),
        current_path: Some(current.to_owned()),
        ..SessionState::default()
    }
}

// F-23: writer.rs:594:95 `replace == with != in Shared::requeue_skipped` (`!gate.pending.iter().any(|(k, _)| k == key)`).
// A bounded flush that gave up on a busy writer puts BOTH drained folders back; with the mutant the second folder is
// treated as "already pending" because the first one is, and its state is lost.
// C#: SessionWriterTests.Flush_WriterBusy_ReturnsAfterBoundedWaitAndRequeues (extended to two folders).
#[test]
fn requeue_skipped_puts_back_every_drained_folder_not_only_the_first() {
    let disk = Arc::new(FakeFileSystem::new());
    let fs = Arc::new(HookFileSystem::new(Arc::clone(&disk)));
    let store = Arc::new(SessionStore::new(
        &paths(),
        Arc::clone(&fs) as Arc<dyn FileSystem>,
        None,
    ));
    let timer = Arc::new(ManualTimer {
        waiters: Mutex::new(Vec::new()),
    });
    let log = Arc::new(RecordingLog::new());
    let writer = Arc::new(
        SessionWriter::new(
            Arc::clone(&store),
            Some(log as Arc<dyn pr_core::abstractions::Log>),
            None,
            Some(Arc::clone(&timer) as Arc<dyn DebounceTimer>),
        )
        .unwrap_or_else(|e| panic!("writer: {e}")),
    );
    writer.set_bounded_wait_for_tests(Duration::from_millis(100));

    let (entered, release) = (Completion::new(), Completion::new());
    let (e, r) = (entered.clone(), release.clone());
    fs.set_write_hook(Some(Arc::new(move |_| {
        e.complete();
        r.wait_timeout(Duration::from_secs(60));
        None
    })));
    writer
        .update(&state(r"C:\a", "a1"))
        .unwrap_or_else(|_| panic!("update a1"));
    timer.fire_all(); // the timer thread's write blocks inside the file system while holding the writer lock
    assert!(entered.wait_timeout(HANG), "the debounced write never started");
    fs.set_write_hook(None);

    writer
        .update(&state(r"C:\a", "a2"))
        .unwrap_or_else(|_| panic!("update a2"));
    writer
        .update(&state(r"C:\b", "b1"))
        .unwrap_or_else(|_| panic!("update b1"));
    let w = Arc::clone(&writer);
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        w.flush(); // drains a2 + b1, cannot get the writer lock within 100 ms, requeues both
        let _ = tx.send(());
    });
    rx.recv_timeout(HANG)
        .unwrap_or_else(|e| panic!("flush behind a held lock: {e}"));

    release.complete();
    writer.when_idle();
    writer.flush();

    let a = store.load(r"C:\a").unwrap_or_else(|_| panic!("load a"));
    let b = store.load(r"C:\b").unwrap_or_else(|_| panic!("load b"));
    assert_eq!(a.current_path.as_deref(), Some("a2"));
    assert_eq!(
        b.current_path.as_deref(),
        Some("b1"),
        "the second requeued folder was lost"
    );
}

// X-perf-diagnostics (P-16, W-05): `path_id` builds its absolute form with `std::path::absolute` ("self-contained"), the
// C# `PathId` uses `Path.GetFullPath`. The two must agree on the Windows path quirks (trailing space/dot, `/`, `..`,
// doubled separators, UNC, drive-relative), otherwise the same file would get two ids in the CSV.
// C#: PhotoReviewPerfTests.PathId_RelativeEqualsFull (extended to the quirk paths of the dotnet-path golden).
#[test]
fn path_id_agrees_with_the_dotnet_full_path_on_windows_path_quirks() {
    use pr_core::diagnostics::perf::path_id;
    use pr_core::text::dotnet_path::get_full_path;
    for path in [
        r"C:\a\b ",
        r"C:\a\b.",
        r"C:\a\b...",
        r"C:/a/b",
        r"C:\a\..\b",
        r"C:\a\.\b",
        r"C:\a\\b",
        r"\\srv\sh\x\..\y",
        r"\\?\C:\a\..\b",
        r"\a",
        r"C:a",
        r"c:\a  \b",
        r"C:\a.\b",
    ] {
        let full = get_full_path(path).unwrap_or_else(|e| panic!("get_full_path {path:?}: {e:?}"));
        assert_eq!(path_id(path), path_id(&full), "path {path:?} (full {full:?})");
        assert_ne!(path_id(path), path_id(r"C:\other\file.jpg"), "path {path:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// P-09 (W2-18): the Retry-After HTTP-date calendar, through `UpdateChecker::is_rate_limited` (status 403)
// ---------------------------------------------------------------------------------------------------------------

struct NoBody;

impl pr_core::updates::checker::HttpBody for NoBody {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, pr_core::updates::checker::HttpError> {
        Ok(0)
    }
}

fn retry_after_date_is_accepted(value: &str) -> bool {
    let response = pr_core::updates::checker::HttpResponse {
        status: 403,
        headers: vec![("Retry-After".to_owned(), value.to_owned())],
        content_length: Some(0),
        body: Box::new(NoBody),
    };
    pr_core::updates::checker::UpdateChecker::is_rate_limited(&response)
}

// P-09: checker.rs:336:9 `delete match arm 1 | 3 | 5 | 7 | 8 | 10 | 12 in days_in_month` and checker.rs:337:9
// `delete match arm 4 | 6 | 9 | 11` (a 31-day month would fall into the February arm), and checker.rs:356:20
// `replace * with / in weekday_of` (the weekday of most months would be wrong).
// 2023 is not a leap year and 2023-01-01 was a Sunday, so the weekday of every date is computed here without the
// production algorithm: (days before the month + day - 1) % 7, 0 = Sunday.
// C#: UpdateCheckerGapTests.Http403_RetryAfter_IsRateLimited (HttpDateParser accepts a real calendar date only).
#[test]
fn retry_after_http_date_accepts_the_last_real_day_of_every_month_and_rejects_the_next() {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const LENGTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let mut before = 0u32;
    for (m, name) in MONTHS.iter().enumerate() {
        for day in [1, 15, LENGTH[m]] {
            let weekday = (before + day - 1) % 7;
            let date = format!("{}, {day} {name} 2023 00:00:00 GMT", WEEKDAYS[weekday as usize]);
            assert!(retry_after_date_is_accepted(&date), "{date}");
            let wrong = format!(
                "{}, {day} {name} 2023 00:00:00 GMT",
                WEEKDAYS[((weekday + 1) % 7) as usize]
            );
            assert!(!retry_after_date_is_accepted(&wrong), "{wrong}");
        }
        let past_end = LENGTH[m] + 1;
        let weekday = (before + past_end - 1) % 7;
        let date = format!("{}, {past_end} {name} 2023 00:00:00 GMT", WEEKDAYS[weekday as usize]);
        assert!(!retry_after_date_is_accepted(&date), "{date} does not exist");
        before += LENGTH[m];
    }
}

// P-09: checker.rs:617:22 `replace > with >= in find_property` (`raw.len() > key_bytes.len()`) and checker.rs:617:59
// `replace > with >= in find_property` (`key_bytes.len() > backslash`).
// A property NAME holding a lone surrogate is only unescaped by a lookup that gets that far. The bad name is the LAST
// property, so the (reverse) scan meets it first: `ht\ud800` is as long as the key `html_url` (the escape must NOT be
// unescaped), `tag_name\ud800` starts its escape exactly where the key `tag_name` ends (must NOT be unescaped either).
// C#: JsonElement.TryGetProperty differential (see `lone_surrogate_property_name_only_fails_when_compared`).
#[test]
fn lone_surrogate_name_as_long_as_the_key_or_escaping_right_after_it_is_skipped_not_unescaped() {
    use pr_core::updates::version::{AppVersion, UpdateCheckResult};
    const RELEASES: &str = "https://github.com/ConanKLOP2/PhotoReview/releases";
    const TAG_URL: &str = "https://github.com/ConanKLOP2/PhotoReview/releases/tag/v2.1.0";
    let current = AppVersion::try_parse(Some("2.0.93")).expect("version");
    let cases = [
        (
            format!(r#"{{"tag_name":"v2.1.0","html_url":"{TAG_URL}","ht\ud800":1}}"#),
            UpdateCheckResult::update_available("2.1.0", TAG_URL),
        ),
        (
            r#"{"tag_name":"v2.1.0","tag_name\ud800":1}"#.to_owned(),
            UpdateCheckResult::update_available("2.1.0", RELEASES),
        ),
    ];
    for (body, expected) in &cases {
        assert_eq!(
            &pr_core::updates::checker::UpdateChecker::interpret(body, current),
            expected,
            "{body}"
        );
    }
}

// F-23: writer.rs:46:9 `replace <impl fmt::Display for InvalidDebounce>::fmt -> fmt::Result with Ok(Default::default())`
// (the message of the rejected debounce must reach the caller, not an empty string).
// C#: SessionWriterShutdownAndVersionTests.Constructor_InvalidDebounce_Throws (the ArgumentOutOfRangeException message).
#[test]
fn invalid_debounce_message_names_the_rejected_value() {
    let text = pr_core::session::writer::InvalidDebounce(Duration::from_secs(0)).to_string();
    assert_eq!(
        text,
        "The debounce must be positive and no longer than Task.Delay supports (got 0ns)."
    );
}

// X-counting-fs: counting.rs:38:9 `replace <impl fmt::Debug for CountingFileSystem>::fmt -> fmt::Result with
// Ok(Default::default())`; P-09: checker.rs:96:9 (Debug for HttpResponse) and checker.rs:129:9 (Debug for UpdateChecker).
// Debug output only, but a type that prints nothing hides itself in assertion failures and logs.
#[test]
fn debug_output_of_the_decorators_names_the_type() {
    use pr_core::io::counting::CountingFileSystem;
    use pr_core::updates::checker::{HttpError, HttpRequest, HttpResponse, HttpTransport, UpdateChecker};

    struct Offline;
    impl HttpTransport for Offline {
        fn get(
            &self,
            _request: &HttpRequest,
            _timeout: Duration,
            _cancel: &CancelToken,
        ) -> Result<HttpResponse, HttpError> {
            Err(HttpError::Timeout)
        }
    }

    let counting = CountingFileSystem::new(Arc::new(FakeFileSystem::new()), Arc::new(|| {}));
    assert_eq!(format!("{counting:?}"), "CountingFileSystem { .. }");

    let response = HttpResponse {
        status: 403,
        headers: Vec::new(),
        content_length: Some(7),
        body: Box::new(NoBody),
    };
    assert_eq!(
        format!("{response:?}"),
        "HttpResponse { status: 403, content_length: Some(7), .. }"
    );

    let checker = UpdateChecker::new(Arc::new(Offline), Some(Duration::from_secs(3)));
    assert_eq!(format!("{checker:?}"), "UpdateChecker { timeout: 3s, .. }");
}

// F-23: writer.rs:324:9 `replace <impl fmt::Debug for SessionWriter>::fmt -> fmt::Result with Ok(Default::default())`.
#[test]
fn session_writer_debug_output_shows_the_debounce() {
    let disk = Arc::new(FakeFileSystem::new());
    let store = Arc::new(SessionStore::new(&paths(), disk as Arc<dyn FileSystem>, None));
    let timer = Arc::new(ManualTimer {
        waiters: Mutex::new(Vec::new()),
    });
    let writer = SessionWriter::new(store, None, None, Some(timer as Arc<dyn DebounceTimer>))
        .unwrap_or_else(|e| panic!("writer: {e}"));
    assert_eq!(format!("{writer:?}"), "SessionWriter { debounce: 500ms, .. }");
}

// ---------------------------------------------------------------------------------------------------------------
// X-perf-diagnostics: PerfCsvListener
// ---------------------------------------------------------------------------------------------------------------

/// A sink that records the length of every `write` call it receives.
struct ChunkSink(Arc<Mutex<Vec<usize>>>);

impl std::io::Write for ChunkSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// X-perf-diagnostics: perf_csv.rs:37:32 `replace * with + in` (`WRITER_BUFFER = 8 * 1024` would become 1032 bytes) and its
// `/` twin (a zero-byte buffer, reported as a timeout by cargo-mutants). The writer thread buffers rows in an 8 KiB
// `BufWriter` before they reach the sink: with an idle-flush interval of one hour nothing but buffer overflow and the
// final flush pushes bytes to the sink, so the largest chunk the sink sees is bigger than 2000 bytes (the mutant's
// buffer cannot hold more than 1032) and never bigger than 8192.
// C#: PerfCsvListenerMappingTests (rows reach the stream through a 4096+ buffered StreamWriter).
#[test]
fn writer_buffers_rows_in_an_eight_kib_buffer_before_the_sink() {
    use pr_core::diagnostics::perf_csv::{PerfCsvListener, build_row};
    let chunks = Arc::new(Mutex::new(Vec::new()));
    let no_diag = |_: &str| -> Option<String> { None };
    let listener = PerfCsvListener::start_for_test(
        Box::new(ChunkSink(Arc::clone(&chunks))),
        10_000,
        Some(Duration::from_secs(3600)),
        None,
        None,
        &no_diag,
    );
    for _ in 0..600 {
        listener.enqueue(|| build_row("Evt", None, None));
    }
    listener.dispose();

    let sizes = chunks.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let largest = sizes.iter().copied().max().unwrap_or(0);
    assert!(largest > 2000, "the sink never received a buffered chunk: {sizes:?}");
    assert!(largest <= 8192, "the buffer is larger than 8 KiB: {sizes:?}");
}

// X-perf-diagnostics: perf_csv.rs:241:9 `replace <impl Debug for PerfCsvListener>::fmt -> fmt::Result with Ok(Default::default())`.
#[test]
fn perf_csv_listener_debug_output_names_the_type() {
    let listener = pr_core::diagnostics::perf_csv::PerfCsvListener::unstarted();
    assert_eq!(format!("{listener:?}"), "PerfCsvListener { .. }");
}

/// A sink that tells the test, through a channel, when a `write` carries `marker`.
struct MarkerSink {
    seen: std::sync::mpsc::Sender<()>,
    marker: &'static [u8],
}

impl std::io::Write for MarkerSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.windows(self.marker.len()).any(|w| w == self.marker) {
            let _ = self.seen.send(());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// X-perf-diagnostics: the idle flush of `writer_loop`. The existing `idle_writer_flushes_the_tail_of_a_burst` only waits
// for ANY byte, which the header already provides; this test waits for the ROW itself, before dispose, so only the
// idle flush can deliver it. (perf_csv.rs:366:44 `> with ==` / `>=` on `unflushed > 0` delay the flush by one idle
// interval or flush an empty buffer once more: timing-only, no data difference, treated as equivalent.)
// C#: PerfCsvListenerFaultTests.IdleWriter_FlushesTheTailOfABurst.
#[test]
fn idle_writer_delivers_the_row_itself_before_dispose() {
    use pr_core::diagnostics::perf_csv::{PerfCsvListener, build_row};
    let (tx, rx) = std::sync::mpsc::channel();
    let no_diag = |_: &str| -> Option<String> { None };
    let listener = PerfCsvListener::start_for_test(
        Box::new(MarkerSink {
            seen: tx,
            marker: b"IdleTailEvt",
        }),
        100,
        Some(Duration::from_millis(20)),
        None,
        None,
        &no_diag,
    );

    listener.enqueue(|| build_row("IdleTailEvt", None, None));

    rx.recv_timeout(HANG)
        .unwrap_or_else(|e| panic!("the row never reached the sink while the writer was idle: {e}"));
    listener.dispose();
}

// ---------------------------------------------------------------------------------------------------------------
// X-perf-diagnostics: clocks, process start, subscription bookkeeping, path ids of rejected paths
// ---------------------------------------------------------------------------------------------------------------

/// Spins (no sleep) until `done()` holds or the hang guard passes; `false` on timeout.
fn spin_until(done: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + HANG;
    while !done() {
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::hint::spin_loop();
    }
    true
}

// X-perf-diagnostics: perf.rs:35:5 `replace timestamp -> i64 with 1`, perf.rs:38:45 `replace / with % in timestamp` and
// `replace / with *` (the counter must count 100 ns ticks: after one millisecond it is at least 10 000 ticks ahead, and
// never more ticks ahead than the wall clock measured around the two reads), and every value is positive so a
// caller's "0 = not started" sentinel cannot misfire. (The `+ 1` offset itself is only observable on the very first
// call of the process: `+ with -` / `+ with *` on it are equivalent for deltas, the only thing callers use.)
// C#: PhotoReviewPerfTests.Timestamp_IsMonotonicAtStopwatchFrequency.
#[test]
fn timestamp_counts_ticks_forward_and_is_never_zero() {
    use pr_core::diagnostics::perf::{STOPWATCH_FREQUENCY, ms, timestamp};
    let wall = std::time::Instant::now();
    let first = timestamp();
    assert!(first >= 1, "{first}");
    assert_eq!(STOPWATCH_FREQUENCY, 10_000_000);
    assert!(
        spin_until(|| timestamp() - first >= STOPWATCH_FREQUENCY / 1000),
        "the counter did not advance by one millisecond of ticks"
    );
    let last = timestamp();
    let wall_ticks = wall.elapsed().as_nanos() / 100;
    assert!(
        u128::try_from(last - first).unwrap_or(u128::MAX) <= wall_ticks + 1,
        "{} ticks counted in {wall_ticks} wall ticks",
        last - first
    );
    assert!(ms(first) >= 1.0, "a millisecond of ticks must read as at least 1 ms");
}

// X-perf-diagnostics: perf.rs:52:5 `replace ms_since_process_start -> f64 with 0.0 / 1.0` and perf.rs:80:5
// `replace init_process_start with ()`. This is the only test of this binary that touches the process start, so the first
// `init_process_start` call is ours: a start one hour in the past must be what the clock measures from (without the
// call the lazy capture would read about 0 ms).
// C#: PhotoReviewPerfTests.MsSinceProcessStart_IsPositiveAndGrows.
#[test]
fn ms_since_process_start_measures_from_the_captured_start_and_grows() {
    use pr_core::diagnostics::perf::{init_process_start, ms_since_process_start};
    use pr_core::time::DateTimeUtc;
    const HOUR_TICKS: i64 = 36_000_000_000;
    let an_hour_ago = DateTimeUtc::now()
        .checked_add_ticks(-HOUR_TICKS)
        .unwrap_or_else(|| panic!("an hour ago is representable"));
    init_process_start(Some(an_hour_ago));
    init_process_start(None); // a second call keeps the first value

    let first = ms_since_process_start();
    assert!(
        (3_600_000.0..3_700_000.0).contains(&first),
        "an hour since the captured start, got {first} ms"
    );
    assert!(
        spin_until(|| ms_since_process_start() > first + 2.0),
        "the wall clock did not advance by 2 ms"
    );
}

// X-perf-diagnostics: perf.rs:253:70 `replace == with != in <impl Drop for PerfSubscription>::drop`. With two DIFFERENT
// listeners, dropping the first subscription must remove that listener and keep the second one (the existing
// `is_enabled_follows_the_subscriptions` subscribes the same capture twice, so removing the wrong entry goes unnoticed).
// C#: PhotoReviewPerfTests (EventListener.Dispose unsubscribes only that listener).
#[test]
fn dropping_a_subscription_removes_that_listener_and_keeps_the_others() {
    use pr_core::diagnostics::perf::{PerfEvent, PerfListener, PhotoReviewPerf};
    #[derive(Default)]
    struct Count(std::sync::atomic::AtomicUsize);
    impl PerfListener for Count {
        fn on_event_written(&self, _event: &PerfEvent) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let (dropped, kept) = (Arc::new(Count::default()), Arc::new(Count::default()));
    let log = PhotoReviewPerf::log();
    let first = log.subscribe(Arc::clone(&dropped) as Arc<dyn PerfListener>);
    let _second = log.subscribe(Arc::clone(&kept) as Arc<dyn PerfListener>);

    drop(first);
    log.stat(0, 1.0);

    assert_eq!(
        dropped.0.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the dropped listener still hears events"
    );
    assert_eq!(
        kept.0.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the kept listener lost its subscription"
    );
}

// X-perf-diagnostics: perf.rs:101:24 `replace || with && in full_path_or_raw`. Documented divergence (the C# `PathId`
// throws): an empty path and a path with a NUL are hashed AS GIVEN (upper-cased), never made absolute. The ids are
// SHA-256 prefixes computed outside the code under test: sha256("")[..8] and sha256("A\0B")[..8].
// C#: PhotoReviewPerfTests.PathId_NeverThrows (Rust: never panics, hashes the raw text).
#[test]
fn path_id_of_an_empty_or_nul_path_hashes_the_text_as_given() {
    use pr_core::diagnostics::perf::path_id;
    assert_eq!(path_id(""), "e3b0c44298fc1c14");
    assert_eq!(path_id("a\0b"), "76fe3925c7167317");
}

// P-09: checker.rs:445:13 `replace < with <= in read_digits` (one digit more than the field allows), checker.rs:449:10
// `replace - with + in read_digits` (the minimum digit count is not enforced), checker.rs:474:27 `replace || with && in
// match_format` (a literal separator that differs is accepted), checker.rs:535:27 (same for the sign of a zone) and
// checker.rs:543:22 `replace < with <= in match_format` (reads past the end of a zone that ends after the hour digits),
// checker.rs:566:15 `< with ==` / `< with <=` and :19 / :51 `|| with &&` (year zero, day zero are rejected; year one is valid).
// A value that starts with a digit is a number of seconds, so every date here starts with a weekday. The weekday is the
// REAL one of the date the mutant would read (Zeller's congruence, independent of the production code), so a rejection
// can only come from the digit count or the literal, never from a weekday mismatch. Well-formed dates are accepted first.
// C#: UpdateCheckerGapTests.Http403_RetryAfter_IsRateLimited (HttpDateParser: exact digit counts and literals).
#[test]
fn retry_after_http_date_enforces_digit_counts_literals_and_zone_signs() {
    /// Zeller's congruence for the proleptic Gregorian calendar, 0 = Sunday.
    fn weekday(year: i64, month: i64, day: i64) -> &'static str {
        let (m, y) = if month < 3 {
            (month + 12, year - 1)
        } else {
            (month, year)
        };
        let (k, j) = (y % 100, y / 100);
        let h = (day + 13 * (m + 1) / 5 + k + k / 4 + j / 4 + 5 * j) % 7; // 0 = Saturday
        ["Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri"][h as usize]
    }
    assert_eq!(weekday(2023, 1, 1), "Sun");
    assert_eq!(weekday(2005, 1, 1), "Sat");
    assert_eq!(weekday(2022, 12, 31), "Sat"); // the day before 1 Jan 2023: what a mutant reading "day 0" would compute

    for good in [
        "Sun, 1 Jan 2023 00:00:00 GMT".to_owned(),
        "Sun, 01 Jan 2023 0:0:0 GMT".to_owned(),
        "Sun, 1 Jan 23 00:00:00 GMT".to_owned(),
        "Sun Jan 1 00:00:00 2023".to_owned(),
        format!("{}, 1 Jan 0001 00:00:00 GMT", weekday(1, 1, 1)), // year 1 is the first valid year
        "Sunday, 1-Jan-23 00:00:00 +0000".to_owned(),
        "Sunday, 1-Jan-23 00:00:00 -01:30".to_owned(),
        "Sunday, 1-Jan-23 00:00:00 +14:00".to_owned(),
    ] {
        assert!(retry_after_date_is_accepted(&good), "{good}");
    }
    let year5 = format!("{}, 1 Jan 20230 00:00:00 GMT", weekday(20230, 1, 1));
    let year0 = format!("{}, 1 Mar 0000 00:00:00 GMT", weekday(0, 3, 1)); // year 0 does not exist
    let year3 = format!("{}, 1 Jan 202 00:00:00 GMT", weekday(202, 1, 1));
    for bad in [
        "Sun, 001 Jan 2023 00:00:00 GMT",     // day with three digits
        year5.as_str(),                       // four-digit year with five digits
        "Sun, 1 Jan 123 00:00:00 GMT",        // two-digit year with three digits (the mutant reads 1900 + 123)
        "Sun, 1 Jan 2023 000:00:00 GMT",      // hour with three digits
        year3.as_str(),                       // four-digit year with three digits
        year0.as_str(),                       // year zero
        "Sat, 0 Jan 2023 00:00:00 GMT",       // day zero (the weekday is the one of the day before)
        "Sat, 1 Jan 5 00:00:00 GMT",          // two-digit year with one digit
        "Sunday, 1-Jan-23 00:00:00 +0",       // zone minutes missing
        "Sunday, 1-Jan-23 00:00:00 +00",      // zone ends after the hour digits
        "Sunday, 1-Jan-23 00:00:00 +0:00:00", // too many zone parts
        "Sun; 1 Jan 2023 00:00:00 GMT",       // wrong separator after the weekday
        "Sun, 1 Jan 2023 00-00-00 GMT",       // wrong time separator
        "Sunday, 1-Jan-23 00:00:00 x0000",    // zone without a sign
        "Sunday, 1-Jan-23 00:00:00 *0000",    // zone with a wrong sign character
        "Sun",                                // ends where the separator is expected
    ] {
        assert!(!retry_after_date_is_accepted(bad), "{bad}");
    }
}

// X-perf-diagnostics: perf_csv.rs:511:9 `replace PerfCsvListener::try_start_from_environment -> Option<PerfCsvListener>
// with None`. The wrapper reads the REAL process environment, and `set_var` is unsafe in edition 2024, so the test runs
// this very test binary as a child process with the trace variable set (an ordinary `Command::env`). The child entry
// below does nothing unless the parent set `W2FS_ENV_PROBE`.
// C#: PerfCsvListenerStartupAndFaultCoverageTests (TryStartFromEnvironment with PHOTOREVIEW_PERF_TRACE set).
#[test]
fn env_probe_child_starts_the_listener_from_the_environment() {
    use pr_core::diagnostics::perf_csv::{LocalTimeParts, PerfCsvListener};
    if std::env::var_os("W2FS_ENV_PROBE").is_none() {
        return; // the normal run: only the parent test below uses this entry
    }
    let now = || LocalTimeParts {
        year: 2026,
        month: 10,
        day: 29,
        hour: 9,
        minute: 5,
        second: 3,
    };
    let listener = PerfCsvListener::try_start_from_environment(Some("w2fs"), &now)
        .unwrap_or_else(|| panic!("no listener from the environment"));
    listener.dispose();
}

#[test]
#[ignore = "native: runs this test binary as a child process with the trace variable set"]
fn try_start_from_environment_reads_the_trace_variable_of_the_process() {
    use pr_core::diagnostics::perf_csv::TRACE_ENV_VAR;
    let root = pr_test_support::TempRoot::new("w2fs-perf-env");
    let trace_dir = root.join("trace");
    let exe = std::env::current_exe().unwrap_or_else(|e| panic!("current_exe: {e}"));
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "env_probe_child_starts_the_listener_from_the_environment",
            "--test-threads=1",
        ])
        .env(TRACE_ENV_VAR, &trace_dir)
        .env("W2FS_ENV_PROBE", "1")
        .output()
        .unwrap_or_else(|e| panic!("child: {e}"));
    assert!(
        output.status.success(),
        "child failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let files: Vec<_> = std::fs::read_dir(&trace_dir)
        .unwrap_or_else(|e| panic!("trace dir: {e}"))
        .filter_map(Result::ok)
        .collect();
    assert_eq!(files.len(), 1, "exactly one trace file");
    let name = files[0].file_name().to_string_lossy().into_owned();
    assert!(
        name.starts_with("perf-") && name.ends_with("-20261029-090503.csv"),
        "{name}"
    );
    let text = std::fs::read_to_string(files[0].path()).unwrap_or_else(|e| panic!("read trace: {e}"));
    assert!(
        text.contains("# dropped=0"),
        "the child disposed the listener: {text:?}"
    );
}
