//! Ports of the W2-16 C# tests (`FileLogTests`, `FileLogLifecycleTests`, `FileLogRotationFailureTests`,
//! `FileLogTimingAndHandleTests`) plus the goldens `file-log-format`, `file-log-backup-name` and `file-log-bytes`.
//!
//! C#: tests not ported (no wait handles exist in Rust, nothing to assert; reflection on private fields):
//! - C#: `FileLogTimingAndHandleTests.Dispose_ClosesWaitHandles` not ported: handle lifetime
//! - C#: `FileLogTimingAndHandleTests.Flush_SignalDisposed_ReturnsImmediately` not ported: handle lifetime
//! - C#: `FileLogTimingAndHandleTests.Flush_DrainedEventDisposed_ReturnsImmediately` not ported: handle lifetime
//!
//! Ported by their observable half only: `DisposeWithBlockedWriterDoesNotDisposeHandlesUnderTheWriter`,
//! `Dispose_StuckWriter_DefersHandleRelease` and `Flush_DrainedSetWhileEntryQueued_WaitsUntilWritten` (the C# test swaps a
//! private field, which Rust cannot; the observable rule "Flush does not return before the queued entry is on disk" stays).
//!
//! DIVERGENCE: an attached error is rendered with `Display` (plus `source()` chain) on the caller's thread, not with
//! `Exception.ToString()` on the writer thread (contract Open question 3).

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant};

use pr_core::abstractions::Log;
use pr_core::diagnostics::file_log::{
    DEFAULT_MAX_LOG_BYTES, FileLog, LocalTimeSource, LogTimestamp, MAX_QUEUED_ENTRIES, MIN_MAX_LOG_BYTES,
    SystemLocalTime, backup_path, current_thread_number, format_entry,
};
use pr_test_support::TempRoot;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};

// ---- helpers ----------------------------------------------------------------------------------------------------

/// A manual-reset event (C# `ManualResetEventSlim`).
#[derive(Debug, Default)]
struct Event {
    state: Mutex<bool>,
    cv: Condvar,
}

impl Event {
    fn set(&self) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.cv.notify_all();
    }

    fn wait(&self, timeout: Duration) -> bool {
        let guard = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |set| !*set)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

/// A bounded timed wait on a never-signalled event (C# `Pause`): the moment waited out has no observable signal.
fn pause(milliseconds: u64) {
    let never = Event::default();
    never.wait(Duration::from_millis(milliseconds));
}

/// An error whose `Display` is the given text (what the oracle injects as `exceptionText`).
#[derive(Debug)]
struct FixedText(String);

impl fmt::Display for FixedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for FixedText {}

/// C# `ThrowingToStringException`: rendering it panics.
#[derive(Debug)]
struct ThrowingDisplay;

impl fmt::Display for ThrowingDisplay {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `resume_unwind` unwinds without running the panic hook, so the test output stays quiet.
        std::panic::resume_unwind(Box::new("boom"))
    }
}

impl Error for ThrowingDisplay {}

fn new_log(path: &Path) -> FileLog {
    FileLog::new(path, DEFAULT_MAX_LOG_BYTES).expect("log path is valid")
}

fn enabled_log(path: &Path) -> FileLog {
    let log = new_log(path);
    log.set_enabled(true);
    log
}

/// A log path whose parent is an existing regular file can never be created: every drain fails.
fn unwritable_log_path(root: &TempRoot) -> PathBuf {
    let blocker = root.join("blocker");
    fs::write(&blocker, "x").expect("write blocker");
    blocker.join("logs").join("test.log")
}

fn read_text(path: &Path) -> String {
    String::from_utf8_lossy(&fs::read(path).expect("read log")).into_owned()
}

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

/// Matches `yyyy-MM-dd HH:mm:ss.fff [LEVEL] [T<digits>]` at the start of `s`; returns (end index, level).
fn match_prefix(s: &str) -> Option<(usize, &str)> {
    let b = s.as_bytes();
    if b.len() < 23 {
        return None;
    }
    for (i, &c) in b[..23].iter().enumerate() {
        let ok = match i {
            4 | 7 => c == b'-',
            10 => c == b' ',
            13 | 16 => c == b':',
            19 => c == b'.',
            _ => c.is_ascii_digit(),
        };
        if !ok {
            return None;
        }
    }
    let rest = &s[23..];
    let rest_after_space = rest.strip_prefix(" [")?;
    let close = rest_after_space.find("] [T")?;
    let level = &rest_after_space[..close];
    if !matches!(level, "INFO" | "WARN" | "ERROR") {
        return None;
    }
    let digits_start = 23 + 2 + close + "] [T".len();
    let digits_len = s[digits_start..].bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 || s.as_bytes().get(digits_start + digits_len) != Some(&b']') {
        return None;
    }
    Some((digits_start + digits_len + 1, level))
}

/// The message of a line that has the standard prefix (`<prefix> <message>`).
fn message_of(line: &str) -> Option<&str> {
    let (end, _) = match_prefix(line)?;
    line[end..].strip_prefix(' ')
}

/// Replaces the prefix at the start of every line by `<ts> [LEVEL] [T]`.
fn mask_prefixes(text: &str) -> String {
    let mut out = String::new();
    for piece in text.split_inclusive('\n') {
        match match_prefix(piece) {
            Some((end, level)) => {
                out.push_str(&format!("<ts> [{level}] [T]"));
                out.push_str(&piece[end..]);
            }
            None => out.push_str(piece),
        }
    }
    out
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day) (civil-from-days).
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// `DateTime` ticks (100 ns since 0001-01-01) to the printed fields; milliseconds are truncated.
fn timestamp_from_ticks(ticks: i64) -> LogTimestamp {
    const PER_DAY: i64 = 864_000_000_000;
    let days = ticks / PER_DAY;
    let in_day = ticks % PER_DAY;
    let (year, month, day) = civil_from_days(days - 719_162);
    LogTimestamp {
        year: year as u16,
        month,
        day,
        hour: (in_day / 36_000_000_000) as u8,
        minute: ((in_day / 600_000_000) % 60) as u8,
        second: ((in_day / 10_000_000) % 60) as u8,
        millisecond: ((ticks % 10_000_000) / 10_000) as u16,
    }
}

/// A fixed time for deterministic tests.
#[derive(Debug)]
struct FixedTime(LogTimestamp);

impl LocalTimeSource for FixedTime {
    fn now(&self) -> LogTimestamp {
        self.0
    }
}

/// Port of `FileLogTimingAndHandleTests.StuckWriterLog`: an enabled log whose writer is parked in the drain hook.
struct Stuck {
    entered: Arc<Event>,
    gate: Arc<Event>,
    gate_after_batch: Arc<Event>,
}

impl Stuck {
    fn new() -> Self {
        Self {
            entered: Arc::default(),
            gate: Arc::default(),
            gate_after_batch: Arc::default(),
        }
    }

    fn log(&self, path: &Path, message: &str) -> FileLog {
        let log = new_log(path);
        let after_batch = Arc::new(AtomicBool::new(false));
        let (entered, gate, gate_after) = (self.entered.clone(), self.gate.clone(), self.gate_after_batch.clone());
        let flag = after_batch.clone();
        log.set_drain_hook(Some(Arc::new(move || {
            entered.set();
            if flag.load(Ordering::SeqCst) {
                gate_after.wait(Duration::from_secs(20));
            } else {
                gate.wait(Duration::from_secs(20));
            }
        })));
        log.set_after_batch_written_hook(Some(Arc::new(move || after_batch.store(true, Ordering::SeqCst))));
        log.set_enabled(true);
        log.info(message);
        assert!(
            self.entered.wait(Duration::from_secs(10)),
            "writer never reached the drain hook"
        );
        log
    }

    fn release_all(&self) {
        self.gate.set();
        self.gate_after_batch.set();
    }
}

impl Drop for Stuck {
    fn drop(&mut self) {
        self.release_all();
    }
}

fn log_path(root: &TempRoot) -> PathBuf {
    root.join("logs").join("test.log")
}

// ---- FileLogTests -----------------------------------------------------------------------------------------------

#[test]
fn queue_is_bounded_when_log_file_cannot_be_written() {
    // C#: FileLogTests.QueueIsBoundedWhenLogFileCannotBeWritten
    let root = TempRoot::new("filelog-bounded");
    let log = enabled_log(&unwritable_log_path(&root));

    for i in 0..MAX_QUEUED_ENTRIES * 3 {
        log.info(&format!("entry {i}"));
    }

    assert!(
        log.pending_count() <= MAX_QUEUED_ENTRIES,
        "pending={}",
        log.pending_count()
    );
    assert!(log.dropped_count() >= (MAX_QUEUED_ENTRIES * 2) as i64);
}

#[test]
fn shutdown_does_not_stall_on_unwritable_log_file() {
    // C#: FileLogTests.ShutdownDoesNotStallOnUnwritableLogFile
    let root = TempRoot::new("filelog-nostall");
    let log = enabled_log(&unwritable_log_path(&root));
    log.info("lost");

    let start = Instant::now();
    log.shutdown();
    let elapsed = start.elapsed();
    log.dispose();

    assert!(elapsed < Duration::from_millis(1500), "shutdown took {elapsed:?}");
}

#[test]
fn disabled_logging_creates_no_directory_or_file() {
    // C#: FileLogTests.DisabledLoggingCreatesNoDirectoryOrFile (INV-10)
    let root = TempRoot::new("filelog-disabled");
    let file = log_path(&root);
    let log = new_log(&file);
    assert!(!log.enabled());

    log.info("disabled-info");
    log.warn("disabled-warn");
    log.error("disabled-error", Some(&FixedText("disabled-exception".to_owned())));
    log.flush();

    assert!(!file.parent().expect("parent").exists());
    assert!(!file.exists());
    assert!(!log.is_writer_alive());
}

#[test]
fn explicitly_enabled_logging_writes_formatted_entries() {
    // C#: FileLogTests.ExplicitlyEnabledLoggingWritesFormattedEntries
    let root = TempRoot::new("filelog-enabled");
    let file = log_path(&root);
    let log = enabled_log(&file);

    log.info("info-message");
    log.warn("warn-message");
    log.error(
        "error-message",
        Some(&FixedText("System.InvalidOperationException: boom-error".to_owned())),
    );
    log.flush();

    assert!(file.exists());
    let content = read_text(&file);
    for needle in [
        "[INFO]",
        "info-message",
        "[WARN]",
        "warn-message",
        "[ERROR]",
        "error-message",
        "System.InvalidOperationException: boom-error",
    ] {
        assert!(content.contains(needle), "missing {needle:?} in {content:?}");
    }
}

#[test]
fn disabling_logging_stops_writes() {
    // C#: FileLogTests.DisablingLoggingStopsWrites
    let root = TempRoot::new("filelog-disabling");
    let file = log_path(&root);
    let log = enabled_log(&file);

    log.info("enabled-message");
    log.flush();
    let initial = read_text(&file);

    log.set_enabled(false);
    log.info("after-disabled-message");
    log.flush();

    let finalc = read_text(&file);
    assert_eq!(initial, finalc);
    assert!(!finalc.contains("after-disabled-message"));
}

#[test]
fn concurrent_logging_preserves_all_entries() {
    // C#: FileLogTests.ConcurrentLoggingPreservesAllEntries
    let root = TempRoot::new("filelog-concurrent");
    let file = log_path(&root);
    let log = enabled_log(&file);
    let markers: Vec<String> = (0..100).map(|i| format!("concurrent-log-{i:03}")).collect();

    thread::scope(|scope| {
        for chunk in markers.chunks(10) {
            let log = &log;
            scope.spawn(move || {
                for marker in chunk {
                    log.info(marker);
                }
            });
        }
    });
    log.flush();

    let content = read_text(&file);
    for marker in &markers {
        assert!(content.contains(marker.as_str()), "missing {marker}");
    }
}

#[test]
fn dispose_with_blocked_writer_does_not_dispose_handles_under_the_writer() {
    // C#: FileLogTests.DisposeWithBlockedWriterDoesNotDisposeHandlesUnderTheWriter (observable half)
    let root = TempRoot::new("filelog-blocked-dispose");
    let file = log_path(&root);
    let gate = Arc::new(Event::default());
    let log = new_log(&file);
    let hook_gate = gate.clone();
    log.set_drain_hook(Some(Arc::new(move || {
        hook_gate.wait(Duration::from_secs(30));
    })));
    log.set_enabled(true); // writer starts and blocks in its first drain
    log.info("pending");
    log.flush(); // waits (and times out)

    log.dispose(); // the wait for the writer times out (~2 s) while it is still blocked
    assert!(log.is_writer_alive());

    gate.set(); // writer resumes
    assert!(
        log.wait_writer_exit(Duration::from_secs(10)),
        "writer thread did not exit"
    );
    assert!(read_text(&file).contains("pending"));
}

#[test]
fn log_file_rotates_at_threshold() {
    // C#: FileLogTests.LogFileRotatesAtThreshold
    let root = TempRoot::new("filelog-rotate");
    let file = log_path(&root);
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(&file, vec![0u8; 2048]).expect("plant");
    let backup = file.with_file_name("test.1.log");

    let log = FileLog::new(&file, 1024).expect("log");
    log.set_enabled(true);
    log.info("post-rotation-entry");
    log.flush();

    assert!(backup.exists(), "backup .1 file should exist after rotation");
    assert!(fs::metadata(&backup).expect("backup").len() >= 2048);
    assert!(file.exists());
    assert!(fs::metadata(&file).expect("log").len() < 2048);
    assert!(read_text(&file).contains("post-rotation-entry"));
}

#[test]
fn partial_batch_failure_does_not_duplicate_flushed_entries() {
    // C#: FileLogTests.PartialBatchFailure_DoesNotDuplicateFlushedEntries
    let root = TempRoot::new("filelog-partial");
    let file = log_path(&root);
    let gate = Arc::new(Event::default());
    let retried = Arc::new(Event::default());
    let log = new_log(&file);
    let gated = Arc::new(AtomicBool::new(false));
    {
        let (gate, gated) = (gate.clone(), gated.clone());
        log.set_drain_hook(Some(Arc::new(move || {
            if !gated.swap(true, Ordering::SeqCst) {
                gate.wait(Duration::from_secs(30));
            }
        })));
    }
    let failed = Arc::new(AtomicBool::new(false));
    log.set_before_entry_written_hook(Some(Arc::new(move |index| {
        if index == 1 && !failed.swap(true, Ordering::SeqCst) {
            // The write failure (C# `IOException("disk full (simulated)")`) as a quiet unwind.
            std::panic::resume_unwind(Box::new("disk full (simulated)"));
        }
    })));
    {
        let retried = retried.clone();
        log.set_after_batch_written_hook(Some(Arc::new(move || retried.set())));
    }
    log.set_enabled(true);
    log.info("entry-one");
    log.info("entry-two");
    log.info("entry-three");
    gate.set();
    assert!(retried.wait(Duration::from_secs(30)));
    log.flush();
    log.dispose();

    let content = read_text(&file);
    assert_eq!(count(&content, "entry-one"), 1);
    assert_eq!(count(&content, "entry-two"), 1);
    assert_eq!(count(&content, "entry-three"), 1);
}

#[test]
fn throwing_exception_to_string_does_not_repeat_earlier_lines_nor_block_later_ones() {
    // C#: FileLogTests.ThrowingExceptionToString_DoesNotRepeatEarlierLinesNorBlockLaterOnes
    let root = TempRoot::new("filelog-throwing");
    let file = log_path(&root);
    let gate = Arc::new(Event::default());
    let written = Arc::new(Event::default());
    let log = new_log(&file);
    let gated = Arc::new(AtomicBool::new(false));
    {
        let (gate, gated) = (gate.clone(), gated.clone());
        log.set_drain_hook(Some(Arc::new(move || {
            if !gated.swap(true, Ordering::SeqCst) {
                gate.wait(Duration::from_secs(30));
            }
        })));
    }
    {
        let written = written.clone();
        log.set_after_batch_written_hook(Some(Arc::new(move || written.set())));
    }
    log.set_enabled(true);
    log.info("first-line");
    log.error("poisoned-entry", Some(&ThrowingDisplay));
    log.info("trailing-entry");
    gate.set();
    assert!(written.wait(Duration::from_secs(30)));
    log.flush();
    log.dispose();

    let content = read_text(&file);
    assert_eq!(count(&content, "first-line"), 1);
    assert_eq!(count(&content, "poisoned-entry"), 1);
    assert_eq!(count(&content, "trailing-entry"), 1);
    assert!(content.contains("\n[exception text unavailable]"));
}

#[test]
fn drain_keeps_unwritten_entries_when_the_producer_trims_during_the_write() {
    // C#: FileLogTests.DrainKeepsUnwrittenEntriesWhenTheProducerTrimsDuringTheWrite
    let root = TempRoot::new("filelog-trim");
    let file = log_path(&root);
    let gate = Arc::new(Event::default());
    let log = Arc::new(new_log(&file));
    let gated = Arc::new(AtomicBool::new(false));
    {
        let (gate, gated) = (gate.clone(), gated.clone());
        log.set_drain_hook(Some(Arc::new(move || {
            if !gated.swap(true, Ordering::SeqCst) {
                gate.wait(Duration::from_secs(30));
            }
        })));
    }
    let injected = Arc::new(AtomicBool::new(false));
    {
        let weak: Weak<FileLog> = Arc::downgrade(&log);
        log.set_after_batch_written_hook(Some(Arc::new(move || {
            // Runs once, on the writer thread, while the first (full) batch is already on disk: the queue is over its
            // bound, so the producer trims the oldest (already written) entries and the newest are not yet written.
            if injected.swap(true, Ordering::SeqCst) {
                return;
            }
            if let Some(log) = weak.upgrade() {
                for i in 0..5 {
                    log.info(&format!("late-entry-{i}"));
                }
            }
        })));
    }
    log.set_enabled(true);
    for i in 0..MAX_QUEUED_ENTRIES {
        log.info(&format!("early-entry-{i}"));
    }
    gate.set();
    log.flush();

    let content = read_text(&file);
    for i in 0..5 {
        assert!(content.contains(&format!("late-entry-{i}")), "late-entry-{i} lost");
    }
    assert_eq!(log.pending_count(), 0);
    log.dispose();
}

// ---- FileLogLifecycleTests --------------------------------------------------------------------------------------

#[test]
fn use_after_dispose_never_throws() {
    // C#: FileLogLifecycleTests.UseAfterDispose_NeverThrows
    let root = TempRoot::new("filelog-after-dispose");
    let log = enabled_log(&log_path(&root));
    log.info("before");
    log.dispose();

    log.info("after");
    log.warn("after");
    log.error("after", Some(&FixedText("x".to_owned())));
    log.flush();
    log.set_enabled(false);
    log.shutdown();
    log.dispose();
}

#[test]
fn enable_after_dispose_does_not_start_writer() {
    // C#: FileLogLifecycleTests.EnableAfterDispose_DoesNotStartWriter
    let root = TempRoot::new("filelog-enable-after-dispose");
    let file = log_path(&root);
    let log = new_log(&file);
    log.dispose();

    log.set_enabled(true);
    log.info("late");

    assert!(!log.is_writer_alive());
    assert!(!log.enabled());
    assert!(!file.exists());
}

#[test]
fn dispose_drains_every_queued_entry_to_disk() {
    // C#: FileLogLifecycleTests.Dispose_DrainsEveryQueuedEntryToDisk
    let root = TempRoot::new("filelog-dispose-drains");
    for round in 0..5 {
        let path = root.join(format!("drain{round}")).join("drain.log");
        let log = enabled_log(&path);
        for i in 0..2_000 {
            log.info(&format!("t0-{i}"));
        }

        log.dispose();

        assert_eq!(log.dropped_count(), 0);
        assert_eq!(read_text(&path).lines().count(), 2_000);
    }
}

#[test]
fn dispose_under_load_writers_never_throw() {
    // C#: FileLogLifecycleTests.DisposeUnderLoad_WritersNeverThrow
    let root = TempRoot::new("filelog-dispose-load");
    for round in 0..6usize {
        let path = root.join(format!("round{round}")).join("load.log");
        let log = Arc::new(enabled_log(&path));
        let start = Arc::new(Event::default());
        let handles: Vec<_> = (0..6)
            .map(|t| {
                let (log, start) = (log.clone(), start.clone());
                thread::spawn(move || {
                    start.wait(Duration::from_secs(30));
                    for i in 0..1_500 {
                        log.info(&format!("t{t}-{i}"));
                    }
                })
            })
            .collect();
        start.set();
        // Land dispose inside the writers' active window: wait for real evidence that entries were queued.
        let threshold = round * 25;
        let deadline = Instant::now() + Duration::from_secs(10);
        while log.pending_count() <= threshold && !handles.iter().all(thread::JoinHandle::is_finished) {
            assert!(
                Instant::now() < deadline,
                "writers never queued more than {threshold} entries (round {round})"
            );
            thread::yield_now();
        }
        log.dispose();
        for handle in handles {
            assert!(handle.join().is_ok(), "a producer thread panicked");
        }
        assert_on_disk_is_per_thread_prefix(&path, log.dropped_count());
    }
}

/// Port of `AssertOnDiskIsPerThreadPrefix`: per producer the retained entries are strictly increasing; with no drops they
/// are the contiguous prefix; with drops the gaps never exceed the dropped count.
fn assert_on_disk_is_per_thread_prefix(path: &Path, dropped: i64) {
    let mut seen: HashMap<u32, Vec<u32>> = HashMap::new();
    if path.exists() {
        for line in read_text(path).lines() {
            let message = message_of(line).unwrap_or_else(|| panic!("torn or malformed log line: {line:?}"));
            let rest = message
                .strip_prefix('t')
                .unwrap_or_else(|| panic!("torn line: {line:?}"));
            let (t, i) = rest.split_once('-').unwrap_or_else(|| panic!("torn line: {line:?}"));
            let t: u32 = t.parse().unwrap_or_else(|_| panic!("torn line: {line:?}"));
            let i: u32 = i.parse().unwrap_or_else(|_| panic!("torn line: {line:?}"));
            seen.entry(t).or_default().push(i);
        }
    }
    for (t, list) in &seen {
        for pair in list.windows(2) {
            assert!(
                pair[1] > pair[0],
                "thread {t}: retained entry {} is not strictly after {} (reordered or duplicated)",
                pair[1],
                pair[0]
            );
        }
        if dropped == 0 {
            let expected: Vec<u32> = (0..list.len() as u32).collect();
            assert_eq!(&expected, list, "thread {t}: not a contiguous prefix");
        } else if let Some(&last) = list.last() {
            let missing = i64::from(last) + 1 - list.len() as i64;
            assert!(
                missing <= dropped,
                "thread {t}: {missing} entries missing below index {last} exceeds dropped count {dropped}"
            );
        }
    }
}

#[test]
fn odd_messages_written_verbatim() {
    // C#: FileLogLifecycleTests.OddMessages_WrittenVerbatim
    let root = TempRoot::new("filelog-odd");
    let file = log_path(&root);
    let log = enabled_log(&file);

    let odd = "line1\nline2 {0} {name} {{x}} İı 日本語 مرحبا 😀";
    log.info(odd);
    log.info("");
    log.flush();

    let text = read_text(&file).replace("\r\n", "\n");
    assert!(text.contains(odd));
}

#[test]
fn timestamp_is_culture_independent() {
    // C#: FileLogLifecycleTests.Timestamp_IsCultureIndependent (one row: Rust has no culture; the digits are ASCII)
    let root = TempRoot::new("filelog-culture");
    let file = log_path(&root);
    let log = enabled_log(&file);
    log.info("marker");
    log.flush();

    let text = read_text(&file);
    let line = text.lines().find(|l| l.contains("marker")).expect("marker line");
    let (end, level) = match_prefix(line).expect("standard prefix");
    assert_eq!(level, "INFO");
    assert_eq!(&line[end..], " marker");
    assert!(line[..4].bytes().all(|b| b.is_ascii_digit()));
    let year: i32 = line[..4].parse().expect("year");
    let now = i32::from(SystemLocalTime.now().year);
    assert!((now - 1..=now + 1).contains(&year), "year {year} vs {now}");
}

// ---- FileLogRotationFailureTests --------------------------------------------------------------------------------

/// Clears the read-only attribute so the temp folder can be removed.
struct ReadOnlyGuard(PathBuf);

impl Drop for ReadOnlyGuard {
    fn drop(&mut self) {
        if let Ok(meta) = fs::metadata(&self.0) {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)] // cleanup of a temp file inside the test's own folder
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&self.0, permissions);
        }
    }
}

#[test]
fn read_only_backup_logging_continues() {
    // C#: FileLogRotationFailureTests.ReadOnlyBackup_LoggingContinues
    let root = TempRoot::new("filelog-readonly-backup");
    let log_file = root.join("app.log");
    let backup = root.join("app.1.log");
    fs::write(&log_file, vec![0u8; 4096]).expect("log");
    fs::write(&backup, "old backup").expect("backup");
    let mut permissions = fs::metadata(&backup).expect("meta").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&backup, permissions).expect("readonly");
    let _guard = ReadOnlyGuard(backup);

    let file = FileLog::new(&log_file, 1024).expect("log");
    file.set_enabled(true);
    file.info("still-logging-after-failed-rotation");
    file.flush();

    assert!(read_text(&log_file).contains("still-logging-after-failed-rotation"));
}

#[cfg(windows)]
#[test]
fn locked_backup_logging_continues() {
    // C#: FileLogRotationFailureTests.LockedBackup_LoggingContinues
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    let root = TempRoot::new("filelog-locked-backup");
    let log_file = root.join("app.log");
    let backup = root.join("app.1.log");
    fs::write(&log_file, vec![0u8; 4096]).expect("log");
    fs::write(&backup, "old backup").expect("backup");
    let holder = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&backup)
        .expect("hold backup");

    let file = FileLog::new(&log_file, 1024).expect("log");
    file.set_enabled(true);
    file.info("still-logging-with-locked-backup");
    file.flush();

    assert!(read_text(&log_file).contains("still-logging-with-locked-backup"));
    drop(holder);
}

#[test]
fn rotation_keeps_one_backup_and_no_entry_is_lost() {
    // C#: FileLogRotationFailureTests.Rotation_NoEntryLost
    let root = TempRoot::new("filelog-rotation-lossless");
    let log_file = root.join("app.log");
    fs::write(&log_file, vec![0u8; 8192]).expect("log");

    let file = FileLog::new(&log_file, 4096).expect("log");
    file.set_enabled(true);
    for i in 0..20 {
        file.info(&format!("entry-{i}"));
    }
    file.flush();

    let all = read_text(&log_file) + &read_text(&root.join("app.1.log"));
    for i in 0..20 {
        assert!(all.contains(&format!("entry-{i}\r\n")), "entry-{i} lost");
    }
    assert_eq!(fs::read_dir(root.path()).expect("dir").count(), 2);
}

#[cfg(windows)]
#[test]
fn unwritable_log_entries_are_kept_for_the_next_drain() {
    // C#: FileLogRotationFailureTests.UnwritableLog_EntriesAreKeptForTheNextDrain
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    let root = TempRoot::new("filelog-unwritable-kept");
    let log_file = root.join("app.log");
    fs::write(&log_file, "").expect("log");
    let file = new_log(&log_file);
    file.set_enabled(true);

    let holder = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&log_file)
        .expect("hold log");
    file.info("written-while-locked");
    file.flush();
    drop(holder);

    file.info("written-after-unlock");
    file.flush();

    let content = read_text(&log_file);
    assert!(content.contains("written-while-locked"));
    assert!(content.contains("written-after-unlock"));
    assert_eq!(count(&content, "written-while-locked"), 1);
}

// ---- FileLogTimingAndHandleTests --------------------------------------------------------------------------------

#[test]
fn shutdown_stuck_writer_uses_one_shared_budget() {
    // C#: FileLogTimingAndHandleTests.Shutdown_StuckWriter_UsesOneSharedBudget
    let root = TempRoot::new("filelog-shared-budget");
    let stuck = Stuck::new();
    let log = stuck.log(&log_path(&root), "stuck-entry");

    let start = Instant::now();
    log.shutdown();
    let elapsed = start.elapsed();

    stuck.release_all();
    assert!(log.wait_writer_exit(Duration::from_secs(10)));
    log.dispose();
    assert!(
        (Duration::from_millis(1_500)..=Duration::from_millis(3_900)).contains(&elapsed),
        "shutdown took {elapsed:?}"
    );
}

#[test]
fn dispose_stuck_writer_defers_handle_release() {
    // C#: FileLogTimingAndHandleTests.Dispose_StuckWriter_DefersHandleRelease (observable half)
    let root = TempRoot::new("filelog-stuck-dispose");
    let file = log_path(&root);
    let stuck = Stuck::new();
    let log = stuck.log(&file, "stuck-entry");

    log.dispose();

    assert!(log.is_writer_alive());
    log.set_enabled(true); // must not revive a disposed log
    assert!(!log.enabled());

    stuck.release_all();
    assert!(
        log.wait_writer_exit(Duration::from_secs(10)),
        "writer did not exit after dispose"
    );
    assert!(read_text(&file).contains("stuck-entry"));
}

#[test]
fn flush_drained_set_while_entry_queued_waits_until_written() {
    // C#: FileLogTimingAndHandleTests.Flush_DrainedSetWhileEntryQueued_WaitsUntilWritten (observable half: the C# test swaps
    // the private `_drained` field for a set event, which Rust cannot; the rule "Flush returns only once the entry is on
    // disk" is asserted with the writer stuck while Flush is already waiting)
    let root = TempRoot::new("filelog-flush-polls");
    let file = log_path(&root);
    let stuck = Stuck::new();
    let log = stuck.log(&file, "polled-entry");

    let gate = stuck.gate.clone();
    let releaser = thread::spawn(move || {
        pause(400);
        gate.set();
    });

    log.flush();

    assert!(file.exists(), "flush returned before the queued entry was written");
    assert!(read_text(&file).contains("polled-entry"));
    assert!(releaser.join().is_ok());
    stuck.release_all();
    log.dispose();
}

#[test]
fn flush_released_when_drain_finishes() {
    // C#: FileLogTimingAndHandleTests.Flush_ReleasedWhenDrainFinishes
    let root = TempRoot::new("filelog-flush-released");
    let file = log_path(&root);
    let stuck = Stuck::new();
    let log = Arc::new(stuck.log(&file, "released-entry"));

    let flushed = Arc::new(Event::default());
    {
        let (log, flushed) = (log.clone(), flushed.clone());
        thread::spawn(move || {
            log.flush();
            flushed.set();
        });
    }
    pause(200); // let flush start waiting on the drained event

    stuck.gate.set(); // writer drains and writes the entry, then parks in the next (empty) drain's hook

    assert!(
        flushed.wait(Duration::from_secs(1)),
        "flush was not released when the drain finished"
    );
    assert!(read_text(&file).contains("released-entry"));
    stuck.release_all();
    log.dispose();
}

#[test]
fn queue_cap_boundary() {
    // C#: FileLogTimingAndHandleTests.QueueCap_Boundary
    let root = TempRoot::new("filelog-cap");
    let log = enabled_log(&unwritable_log_path(&root));

    for i in 0..MAX_QUEUED_ENTRIES {
        log.info(&format!("entry {i}"));
    }
    assert_eq!(log.pending_count(), MAX_QUEUED_ENTRIES);
    assert_eq!(log.dropped_count(), 0);

    log.info("one too many");
    assert_eq!(log.pending_count(), MAX_QUEUED_ENTRIES);
    assert_eq!(log.dropped_count(), 1);
}

#[test]
fn enabled_but_idle_creates_nothing_on_disk() {
    // C#: FileLogTimingAndHandleTests.EnabledButIdle_CreatesNothingOnDisk
    let root = TempRoot::new("filelog-idle");
    let file = log_path(&root);
    let log = new_log(&file);
    let drains = Arc::new(AtomicUsize::new(0));
    let twice = Arc::new(Event::default());
    {
        let (drains, twice) = (drains.clone(), twice.clone());
        log.set_drain_hook(Some(Arc::new(move || {
            if drains.fetch_add(1, Ordering::SeqCst) + 1 >= 2 {
                twice.set();
            }
        })));
    }
    log.set_enabled(true);

    assert!(twice.wait(Duration::from_secs(10)), "writer did not drain twice");

    assert!(!file.parent().expect("parent").exists());
    assert!(!file.exists());
}

#[test]
fn log_at_exact_limit_is_rotated() {
    // C#: FileLogTimingAndHandleTests.LogAtExactLimit_IsRotated
    let root = TempRoot::new("filelog-exact-limit");
    let path = root.join("app.log");
    fs::write(&path, vec![0u8; 1024]).expect("plant");

    let log = FileLog::new(&path, 1024).expect("log");
    log.set_enabled(true);
    log.info("after-rotation");
    log.flush();

    let backup = root.join("app.1.log");
    assert!(backup.exists());
    assert_eq!(fs::metadata(&backup).expect("backup").len(), 1024);
    assert!(read_text(&path).contains("after-rotation"));
    assert!(fs::metadata(&path).expect("log").len() < 1024);
}

// ---- unit tests of the Rust surface -----------------------------------------------------------------------------

#[test]
fn new_rejects_blank_path() {
    // C#: ArgumentException.ThrowIfNullOrWhiteSpace in the FileLog constructor
    for blank in ["", "   ", "\t"] {
        let err = FileLog::new(blank, 1024).expect_err("blank path");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{blank:?}");
    }
}

#[test]
fn new_stores_absolute_path_and_touches_no_disk() {
    // C#: FileLog(string) uses Path.GetFullPath, which does not touch the disk
    let root = TempRoot::new("filelog-ctor");
    let file = root.join("a").join("..").join("logs").join("x.log");
    let log = new_log(&file);
    assert_eq!(log.file_path(), root.join("logs").join("x.log"));
    assert!(log.file_path().is_absolute());
    assert!(!root.join("logs").exists());
    assert!(!root.join("a").exists());
    assert!(!log.enabled());
    assert!(!log.is_writer_alive());
}

#[test]
fn max_log_bytes_is_clamped_to_one_kib() {
    // C#: Math.Max(1024, maxLogBytes): a 1000-byte file is below the clamped limit, so it is not rotated
    let root = TempRoot::new("filelog-clamp");
    let path = root.join("app.log");
    fs::write(&path, vec![b'x'; 1000]).expect("plant");
    let log = FileLog::new(&path, 1).expect("log");
    log.set_enabled(true);
    log.info("appended");
    log.flush();
    assert!(!root.join("app.1.log").exists());
    assert!(fs::metadata(&path).expect("log").len() > 1000);
    assert_eq!(MIN_MAX_LOG_BYTES, 1024);
}

#[test]
fn backup_path_follows_path_get_extension() {
    // C#: Rotate naming (Path.GetFileNameWithoutExtension + ".1" + Path.GetExtension)
    let cases = [
        (r"C:\logs\app.log", r"C:\logs\app.1.log"),
        (r"C:\logs\noext", r"C:\logs\noext.1"),
        (r"C:\logs\.log", r"C:\logs\.1.log"),
        (r"C:\logs\a..log", r"C:\logs\a..1.log"),
        (r"C:\logs\x.tar.gz", r"C:\logs\x.tar.1.gz"),
        (r"C:\logs\noext.", r"C:\logs\noext.1"),
        (r"C:\app.log", r"C:\app.1.log"),
    ];
    for (log, expected) in cases {
        assert_eq!(backup_path(Path::new(log)), PathBuf::from(expected), "{log}");
    }
}

#[test]
fn format_entry_and_timestamp_are_ascii_and_zero_padded() {
    let ts = LogTimestamp {
        year: 5,
        month: 1,
        day: 2,
        hour: 3,
        minute: 4,
        second: 5,
        millisecond: 6,
    };
    assert_eq!(ts.format(), "0005-01-02 03:04:05.006");
    assert_eq!(
        format_entry(&ts, "WARN", 7, "m {0}", Some("e")),
        "0005-01-02 03:04:05.006 [WARN] [T7] m {0}\ne"
    );
}

#[test]
fn thread_numbers_are_stable_per_thread_and_distinct() {
    let here = current_thread_number();
    assert!(here >= 1);
    assert_eq!(here, current_thread_number());
    let other = thread::spawn(current_thread_number).join().expect("join");
    assert_ne!(here, other);
}

#[test]
fn injected_time_source_is_used_on_the_producer_side() {
    let root = TempRoot::new("filelog-time");
    let file = log_path(&root);
    let ts = LogTimestamp {
        year: 2026,
        month: 10,
        day: 14,
        hour: 9,
        minute: 5,
        second: 3,
        millisecond: 7,
    };
    let log = FileLog::with_time_source(&file, 1024, Arc::new(FixedTime(ts))).expect("log");
    log.set_enabled(true);
    log.info("hello");
    log.flush();
    let text = read_text(&file);
    assert!(text.starts_with("2026-10-14 09:05:03.007 [INFO] [T"), "{text:?}");
}

// ---- goldens ----------------------------------------------------------------------------------------------------

#[test]
fn golden_file_log_format() {
    let g = golden::load("file-log-format");
    let mut skipped = 0;
    for case in &g.cases {
        let ticks: i64 = str_of(case, "ticks").parse().expect("ticks");
        let ts = timestamp_from_ticks(ticks);
        let thread_id = i64_of(case, "threadId");
        // DIVERGENCE: the stub pins `thread_number: u32` (Rust numbers threads from 1, never negative); the golden's
        // negative id case documents C# `int` formatting and cannot be represented, so it is skipped.
        let Ok(thread) = u32::try_from(thread_id) else {
            skipped += 1;
            continue;
        };
        let exception = case["exceptionText"].as_str();
        let line = format_entry(&ts, str_of(case, "level"), thread, str_of(case, "message"), exception);
        assert_eq!(line, str_of(case, "line"), "case {case}");
    }
    assert!(
        skipped <= 1,
        "only the negative thread id case may be skipped, skipped {skipped}"
    );
}

#[test]
fn golden_file_log_backup_name() {
    let g = golden::load("file-log-backup-name");
    let root = TempRoot::new("filelog-golden-backup");
    for (index, case) in g.cases.iter().enumerate() {
        let name = str_of(case, "name");
        let dir = root.join(format!("case{index}"));
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join(name);
        fs::write(&path, vec![0u8; 2048]).expect("plant");

        let log = FileLog::new(&path, 1024).expect("log");
        log.set_enabled(true);
        log.info("entry");
        log.flush();
        log.dispose();

        let mut actual: Vec<(String, bool)> = fs::read_dir(&dir)
            .expect("dir")
            .map(|e| {
                let e = e.expect("entry");
                (
                    e.file_name().to_string_lossy().into_owned(),
                    e.metadata().expect("meta").len() == 2048,
                )
            })
            .collect();
        actual.sort();
        let mut expected: Vec<(String, bool)> = case["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|f| (str_of(f, "name").to_owned(), bool_of(f, "length_is_2048")))
            .collect();
        expected.sort();
        assert_eq!(actual, expected, "case {name:?}");
    }
}

#[test]
fn golden_file_log_bytes() {
    let g = golden::load("file-log-bytes");
    let root = TempRoot::new("filelog-golden-bytes");
    for (index, case) in g.cases.iter().enumerate() {
        let name = str_of(case, "name");
        let path = root.join(format!("case{index}")).join("bytes.log");
        let log = enabled_log(&path);
        for entry in case["entries"].as_array().expect("entries") {
            let message = str_of(entry, "message");
            match str_of(entry, "level") {
                "INFO" => log.info(message),
                "WARN" => log.warn(message),
                "ERROR" => match entry["exceptionText"].as_str() {
                    Some(text) => log.error(message, Some(&FixedText(text.to_owned()))),
                    None => log.error(message, None),
                },
                other => panic!("unknown level {other}"),
            }
        }
        log.flush();
        log.dispose();

        let bytes = fs::read(&path).expect("read log");
        let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        assert_eq!(has_bom, bool_of(case, "has_bom"), "case {name:?}");
        let masked = mask_prefixes(&String::from_utf8(bytes).expect("utf-8"));
        assert_eq!(masked, str_of(case, "masked_text"), "case {name:?}");
        assert_eq!(
            hex_upper(masked.as_bytes()),
            str_of(case, "masked_hex"),
            "case {name:?}"
        );
    }
}
