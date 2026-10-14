//! Port of `PhotoReview.Core/Diagnostics/PerfCsvListener.cs` (C# baseline d92d6aae, 367 lines): the opt-in CSV
//! writer for the [`PhotoReviewPerf`](super::perf::PhotoReviewPerf) events.
//!
//! Only active when the trace variable ([`TRACE_ENV_VAR`]) names a directory that can be created and written;
//! otherwise [`PerfCsvListener::try_start`] returns `None` and nothing is created on disk. Event delivery never
//! blocks and never panics: rows go onto a bounded queue that drops (and counts) instead of blocking when full, and
//! a background thread drains it to the file. The file format is byte-identical to the C# one and is read by
//! `pr_perf::csv`:
//!
//! ```text
//! # commit=<version> diag=<A=1;B=2> qpcFrequency=10000000\r\n
//! utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\r\n
//! <row>\n            (11 fields, LF only)
//! # dropped=<n>\r\n
//! ```
//!
//! Unit W2-17. Contract: `contracts/W2-17.md`.
//!
//! DIVERGENCES from C#: `qpcTicks` are a monotonic 100 ns counter, not `QueryPerformanceCounter`; the thread column is
//! a small per-thread number, not the managed thread id; the queue capacity is clamped to at least 1.

use std::cell::Cell;
use std::io::{self, BufWriter, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError, RwLock};
use std::time::Duration;

use super::perf::{
    PerfEvent, PerfListener, PerfSubscription, PerfValue, PhotoReviewPerf, STOPWATCH_FREQUENCY, ms, timestamp,
};
use crate::time::DateTimeUtc;

/// Buffer between the row formatter and the sink: small enough that a blocked sink stops the writer after a few
/// hundred rows (the drop-accounting test relies on it). The real file has its own, larger buffer.
const WRITER_BUFFER: usize = 8 * 1024;
/// Buffer of the trace file itself (C# `FileStream` bufferSize 65536).
const FILE_BUFFER: usize = 64 * 1024;

/// The directory variable. C#: `PHOTOREVIEW_PERF_TRACE`; Rust prefix per docs/adr/R0002.
pub const TRACE_ENV_VAR: &str = "PHOTOREVIEW_RS_PERF_TRACE";

/// Bounded queue size (C# `ChannelCapacity`).
pub const CHANNEL_CAPACITY: usize = 100_000;

/// Writer thread flushes after this long without a flush, and the idle timer flushes a quiet tail after the same
/// time (C#: `1000` ms in `WriterLoopAsync` and `_idleFlushInterval = 1 s`).
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// How long `dispose` waits for the writer thread before giving up (C# `_disposeWait`, 5 s).
pub const DISPOSE_WAIT: Duration = Duration::from_secs(5);

/// The header names of the diagnostic variables in C# spelling: the CSV `diag=` field and `pr_perf` use these names
/// even though the Rust app reads `PHOTOREVIEW_RS_DIAG_*` (contract section "Diag flags in the header").
pub const DIAG_FLAG_HEADER_NAMES: [&str; 3] = [
    "PHOTOREVIEW_DIAG_PREREAD",
    "PHOTOREVIEW_DIAG_PRELOAD_WORKERS",
    "PHOTOREVIEW_DIAG_DISABLE_DISKCACHE",
];

/// One CSV row, before escaping. C# `PerfCsvListener.Row`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfRow {
    pub utc_ticks: i64,
    pub qpc_ticks: i64,
    pub thread: i32,
    pub event_name: String,
    pub nav: String,
    pub path_id: String,
    pub a: String,
    pub b: String,
    pub c: String,
    pub d: String,
    pub text: String,
}

/// Local wall-clock time parts for the file name `perf-<pid>-<yyyyMMdd-HHmmss>.csv` (C# `DateTime.Now`). `pr-core`
/// has no local-time API: the platform layer supplies this (tests pass a fixed value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTimeParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// `PerfCsvListener.BuildRow(eventName, names, values)`: maps a payload to the columns, stamping `utc_ticks`
/// (`DateTimeUtc::now().ticks()`), `qpc_ticks` ([`timestamp`](super::perf::timestamp)) and `thread`
/// ([`current_thread_number`]). `None` names or values (C# null) give an empty payload. Never panics.
pub fn build_row(event_name: &str, names: Option<&[&str]>, values: Option<&[PerfValue]>) -> PerfRow {
    let utc_ticks = DateTimeUtc::now().ticks();
    let qpc_ticks = timestamp();
    let thread = current_thread_number();

    let mut nav = String::new();
    let mut path_id = String::new();
    let mut numeric: [String; 4] = Default::default();
    let mut numeric_count = 0usize;
    let mut text: Vec<String> = Vec::new();

    if let (Some(names), Some(values)) = (names, values) {
        for (name, value) in names.iter().zip(values.iter()) {
            if (*name == "nav" || *name == "gen") && nav.is_empty() {
                nav = format_value(value);
            } else if *name == "pathId" && path_id.is_empty() {
                path_id = format_value(value);
            } else if matches!(value, PerfValue::Int(_) | PerfValue::Double(_) | PerfValue::Bool(_)) {
                // Only the first four numerics get a column; later ones are counted and dropped.
                if let Some(slot) = numeric.get_mut(numeric_count) {
                    *slot = format_value(value);
                }
                numeric_count += 1;
            } else if !matches!(value, PerfValue::Null) {
                text.push(format_value(value));
            }
        }
    }

    let [a, b, c, d] = numeric;
    PerfRow {
        utc_ticks,
        qpc_ticks,
        thread,
        event_name: event_name.to_owned(),
        nav,
        path_id,
        a,
        b,
        c,
        d,
        text: text.join(";"),
    }
}

/// `PerfCsvListener.FormatValue`: `Null` -> `""`, `Bool` -> `1`/`0`, `Double` -> [`format_g17`], `Int` -> invariant
/// decimal, `Str` verbatim.
pub fn format_value(value: &PerfValue) -> String {
    match value {
        PerfValue::Null => String::new(),
        PerfValue::Bool(b) => (if *b { "1" } else { "0" }).to_owned(),
        PerfValue::Double(d) => format_g17(*d),
        PerfValue::Int(i) => i.to_string(),
        PerfValue::Str(s) => s.clone(),
    }
}

/// .NET `double.ToString("G17", CultureInfo.InvariantCulture)`: the 17-significant-digit correctly rounded expansion
/// with trailing zeros removed; scientific when the decimal exponent is `>= 17` or `< -4`.
pub fn format_g17(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return (if value > 0.0 { "Infinity" } else { "-Infinity" }).to_owned();
    }
    let negative = value.is_sign_negative();
    if value == 0.0 {
        return (if negative { "-0" } else { "0" }).to_owned();
    }
    // "d.dddddddddddddddde[-]X": 17 correctly rounded significant digits.
    let formatted = format!("{:.16e}", value.abs());
    let Some((mantissa, exponent)) = formatted.split_once('e') else {
        return formatted;
    };
    let x: i32 = exponent.parse().unwrap_or(0);
    let mut digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    let body = if !(-4..17).contains(&x) {
        let rest = if digits.len() > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        format!(
            "{}{}E{}{:02}",
            &digits[..1],
            rest,
            if x < 0 { '-' } else { '+' },
            x.abs()
        )
    } else if x >= 0 {
        let int_len = x as usize + 1;
        if digits.len() <= int_len {
            format!("{}{}", digits, "0".repeat(int_len - digits.len()))
        } else {
            format!("{}.{}", &digits[..int_len], &digits[int_len..])
        }
    } else {
        format!("0.{}{}", "0".repeat((-x - 1) as usize), digits)
    };
    if negative { format!("-{body}") } else { body }
}

/// `PerfCsvListener.CsvEscape`: line breaks (`\r\n`, `\r`, `\n`) become one space each (`\r\n` counts as ONE space),
/// then a value containing `,`, `"`, `\n` or `\r` is wrapped in quotes with `"` doubled. Empty stays empty.
pub fn csv_escape(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let flattened = if value.contains(['\r', '\n']) {
        value.replace("\r\n", " ").replace(['\r', '\n'], " ")
    } else {
        value.to_owned()
    };
    if flattened.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", flattened.replace('"', "\"\""))
    } else {
        flattened
    }
}

/// `Environment.CurrentManagedThreadId` stand-in: a small positive integer, stable per thread, assigned in order of
/// first use (the first thread that asks gets 1).
pub fn current_thread_number() -> i32 {
    static NEXT: AtomicI32 = AtomicI32::new(1);
    thread_local! {
        static NUMBER: Cell<i32> = const { Cell::new(0) };
    }
    NUMBER.with(|cell| {
        if cell.get() == 0 {
            cell.set(NEXT.fetch_add(1, Ordering::Relaxed));
        }
        cell.get()
    })
}

/// The CSV writer. Subscribes to [`PhotoReviewPerf`](super::perf::PhotoReviewPerf) when started and unsubscribes in
/// [`dispose`](Self::dispose) (also on drop).
pub struct PerfCsvListener {
    shared: Arc<Shared>,
    subscription: Mutex<Option<PerfSubscription>>,
}

impl std::fmt::Debug for PerfCsvListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PerfCsvListener").finish_non_exhaustive()
    }
}

/// State shared with the writer thread.
struct Shared {
    /// `None` before start (a never-started listener ignores rows silently) and after dispose (rows are counted).
    sender: RwLock<Option<SyncSender<PerfRow>>>,
    started: AtomicBool,
    dropped: AtomicI64,
    writer_faulted: AtomicBool,
    disposed: AtomicBool,
    /// Set by the writer thread when it has written the trailer and released the stream.
    finished: (Mutex<bool>, Condvar),
    dispose_wait: Duration,
}

impl Shared {
    fn new(started: bool, dispose_wait: Duration) -> Self {
        Self {
            sender: RwLock::new(None),
            started: AtomicBool::new(started),
            dropped: AtomicI64::new(0),
            writer_faulted: AtomicBool::new(false),
            disposed: AtomicBool::new(false),
            // A listener that never started has no writer to wait for.
            finished: (Mutex::new(!started), Condvar::new()),
            dispose_wait,
        }
    }

    fn signal_finished(&self) {
        let (lock, cv) = &self.finished;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
        cv.notify_all();
    }

    fn count_dropped(&self, n: i64) {
        self.dropped.fetch_add(n, Ordering::SeqCst);
    }

    /// The body of [`PerfCsvListener::enqueue`] (kept here so the subscription can hold an `Arc<Shared>`).
    fn enqueue(&self, build: impl FnOnce() -> PerfRow) {
        if !self.started.load(Ordering::SeqCst) {
            return; // never started: nothing was queued, nothing is lost
        }
        if self.writer_faulted.load(Ordering::SeqCst) {
            // Nobody reads the queue any more: count the row instead of letting it pile up.
            self.count_dropped(1);
            return;
        }
        // A malformed payload must never reach the emitting thread.
        let Ok(row) = catch_unwind(AssertUnwindSafe(build)) else {
            self.count_dropped(1);
            return;
        };
        let guard = self.sender.read().unwrap_or_else(PoisonError::into_inner);
        match guard.as_ref() {
            Some(tx) => match tx.try_send(row) {
                Ok(()) => {}
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => self.count_dropped(1),
            },
            None => self.count_dropped(1),
        }
    }
}

impl PerfListener for Shared {
    /// C# `OnEventWritten` -> `Enqueue(() => BuildRow(...))` using the event name, payload names and payload.
    fn on_event_written(&self, event: &PerfEvent) {
        self.enqueue(|| build_row(event.name, Some(event.payload_names), Some(&event.payload)));
    }
}

/// The diagnostic variables written to the header, in the fixed order of [`DIAG_FLAG_HEADER_NAMES`], raw values.
fn diag_field(diag: &dyn Fn(&str) -> Option<String>) -> String {
    DIAG_FLAG_HEADER_NAMES
        .iter()
        .filter_map(|name| diag(name).filter(|v| !v.is_empty()).map(|v| format!("{name}={v}")))
        .collect::<Vec<_>>()
        .join(";")
}

fn write_row(writer: &mut impl Write, row: &PerfRow) -> io::Result<()> {
    let line = format!(
        "{},{},{},{},{},{},{},{},{},{},{}\n",
        row.utc_ticks,
        row.qpc_ticks,
        row.thread,
        csv_escape(&row.event_name),
        csv_escape(&row.nav),
        csv_escape(&row.path_id),
        csv_escape(&row.a),
        csv_escape(&row.b),
        csv_escape(&row.c),
        csv_escape(&row.d),
        csv_escape(&row.text),
    );
    writer.write_all(line.as_bytes())
}

/// The writer thread body (C# `WriterLoopAsync` + `FinishWriter`).
fn writer_loop(
    rx: Receiver<PerfRow>,
    mut writer: BufWriter<Box<dyn Write + Send>>,
    shared: &Shared,
    idle_flush: Duration,
) {
    let mut last_flush = timestamp();
    // Rows handed to the writer since the last successful flush (lost if the writer faults).
    let mut unflushed: i64 = 0;
    let mut faulted = false;
    loop {
        if faulted {
            // Nobody writes any more: count what is still queued until the queue is closed. The trailer is written
            // only after that, so a sink that recovers meanwhile still receives it (C# writes it from Dispose).
            match rx.recv() {
                Ok(_) => shared.count_dropped(1),
                Err(_) => break,
            }
            continue;
        }
        let result: io::Result<()> = match rx.recv_timeout(idle_flush) {
            Err(RecvTimeoutError::Timeout) => {
                // Idle for a whole interval: flush the tail of a burst now, so a killed process does not lose it.
                let flushed = if unflushed > 0 {
                    guarded(|| writer.flush())
                } else {
                    Ok(())
                };
                if flushed.is_ok() {
                    unflushed = 0;
                    last_flush = timestamp();
                }
                flushed
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Ok(row) => {
                unflushed += 1;
                let mut step = guarded(|| write_row(&mut writer, &row));
                if step.is_ok() && ms(last_flush) >= 1000.0 {
                    step = guarded(|| writer.flush());
                    if step.is_ok() {
                        unflushed = 0;
                        last_flush = timestamp();
                    }
                }
                step
            }
        };
        if result.is_err() {
            // Rows already handed to the writer but not flushed are abandoned even if they later reach the file.
            shared.writer_faulted.store(true, Ordering::SeqCst);
            shared.count_dropped(unflushed);
            unflushed = 0;
            faulted = true;
        }
    }

    // Best effort: a broken stream makes the flush and the trailer fail silently.
    let _ = guarded(|| writer.flush());
    let _ = guarded(|| write!(writer, "# dropped={}\r\n", shared.dropped.load(Ordering::SeqCst)));
    let _ = guarded(|| writer.flush());
    // A panicking sink must not unwind out of the drop either.
    let _ = catch_unwind(AssertUnwindSafe(|| drop(writer)));
}

/// Runs one stream operation; a panic inside the sink counts as an I/O failure (C# `WriterLoopAsync` catch-all).
fn guarded(op: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    match catch_unwind(AssertUnwindSafe(op)) {
        Ok(result) => result,
        Err(_) => Err(io::Error::other("the trace sink panicked")),
    }
}

/// .NET Core addresses files through the `\\?\` extended prefix, so a directory segment that ends in a space or a dot
/// survives; plain Win32 normalization (what `std::fs` uses for ordinary paths) strips it. Such a path is rewritten
/// lexically to the extended form; every other path is used as given.
#[cfg(windows)]
fn trace_dir_path(dir: &str) -> std::path::PathBuf {
    use std::path::PathBuf;
    let s = dir.replace('/', "\\");
    let needs = s
        .split('\\')
        .any(|seg| seg != "." && seg != ".." && (seg.ends_with(' ') || seg.ends_with('.')));
    if !needs || s.starts_with(r"\\?\") || s.starts_with(r"\\.\") {
        return PathBuf::from(dir);
    }
    let bytes = s.as_bytes();
    let (prefix, rest): (String, String) = if let Some(unc) = s.strip_prefix(r"\\") {
        let mut parts = unc.splitn(3, '\\');
        match (parts.next(), parts.next()) {
            (Some(server), Some(share)) if !server.is_empty() && !share.is_empty() => (
                format!(r"\\?\UNC\{server}\{share}"),
                parts.next().unwrap_or("").to_owned(),
            ),
            _ => return PathBuf::from(dir),
        }
    } else if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        (format!(r"\\?\{}:", char::from(bytes[0])), s[3..].to_owned())
    } else if bytes.len() >= 2 && bytes[1] == b':' {
        return PathBuf::from(dir); // drive-relative: leave to the OS
    } else {
        let Ok(cwd) = std::env::current_dir() else {
            return PathBuf::from(dir);
        };
        let cwd = cwd.to_string_lossy().into_owned();
        let cb = cwd.as_bytes();
        if cb.len() < 3 || cb[1] != b':' || cwd.starts_with(r"\\") {
            return PathBuf::from(dir);
        }
        let drive = format!(r"\\?\{}:", char::from(cb[0]));
        if let Some(rooted) = s.strip_prefix('\\') {
            (drive, rooted.to_owned())
        } else {
            (drive, format!("{}\\{}", &cwd[3..], s))
        }
    };
    let mut stack: Vec<&str> = Vec::new();
    for seg in rest.split('\\') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    let mut out = prefix;
    for seg in stack {
        out.push('\\');
        out.push_str(seg);
    }
    out.push('\\');
    PathBuf::from(out)
}

#[cfg(not(windows))]
fn trace_dir_path(dir: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(dir)
}

impl PerfCsvListener {
    /// `TryStartFromEnvironment`: `lookup` reads the trace variable ([`TRACE_ENV_VAR`]); `None` or blank -> `None`.
    /// Creates the directory (and parents), then `perf-<pid>-<yyyyMMdd-HHmmss>.csv` (create/truncate, shared read),
    /// writes the header and starts the writer. Any failure leaves nothing behind and returns `None`.
    /// `commit` is the app version text for the header (`None` -> `"unknown"`). The header's diagnostic flags are read
    /// through the same `lookup`, mapping each C# header name (`PHOTOREVIEW_DIAG_*`) to its `PHOTOREVIEW_RS_` spelling.
    pub fn try_start(
        lookup: &dyn Fn(&str) -> Option<String>,
        commit: Option<&str>,
        local_now: &dyn Fn() -> LocalTimeParts,
    ) -> Option<PerfCsvListener> {
        let dir = lookup(TRACE_ENV_VAR)?;
        if dir.trim().is_empty() {
            return None;
        }
        let t = local_now();
        let file_name = format!(
            "perf-{}-{:04}{:02}{:02}-{:02}{:02}{:02}.csv",
            std::process::id(),
            t.year,
            t.month,
            t.day,
            t.hour,
            t.minute,
            t.second
        );
        let dir_path = trace_dir_path(&dir);
        std::fs::create_dir_all(&dir_path).ok()?;
        let path = dir_path.join(file_name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(windows)]
        {
            // C# FileShare.Read: a second writer on the same trace file is refused instead of truncating it.
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // FILE_SHARE_READ
        }
        let file = options.open(&path).ok()?;
        let sink: Box<dyn Write + Send> = Box::new(BufWriter::with_capacity(FILE_BUFFER, file));
        let diag = |name: &str| lookup(&name.replacen("PHOTOREVIEW_", "PHOTOREVIEW_RS_", 1));
        match Self::start(sink, CHANNEL_CAPACITY, FLUSH_INTERVAL, DISPOSE_WAIT, commit, &diag) {
            Ok(listener) => Some(listener),
            Err(_) => {
                // Nothing may stay behind when the start fails after the file exists.
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    /// `try_start` reading the real process environment (`std::env::var_os`, lossy) for both variable families.
    pub fn try_start_from_environment(
        commit: Option<&str>,
        local_now: &dyn Fn() -> LocalTimeParts,
    ) -> Option<PerfCsvListener> {
        let lookup = |name: &str| std::env::var_os(name).map(|v| v.to_string_lossy().into_owned());
        Self::try_start(&lookup, commit, local_now)
    }

    /// C# `StartForTest`: a listener writing to `writer` with a custom queue capacity. `idle_flush` /
    /// `dispose_wait` default to [`FLUSH_INTERVAL`] / [`DISPOSE_WAIT`]. `diag` gives the values of the three
    /// [`DIAG_FLAG_HEADER_NAMES`] for the header.
    ///
    /// DIVERGENCE: where C# would throw (the header cannot be written or the thread cannot start) this returns a
    /// listener that was never started.
    pub fn start_for_test(
        writer: Box<dyn Write + Send>,
        capacity: usize,
        idle_flush: Option<Duration>,
        dispose_wait: Option<Duration>,
        commit: Option<&str>,
        diag: &dyn Fn(&str) -> Option<String>,
    ) -> PerfCsvListener {
        Self::start(
            writer,
            capacity,
            idle_flush.unwrap_or(FLUSH_INTERVAL),
            dispose_wait.unwrap_or(DISPOSE_WAIT),
            commit,
            diag,
        )
        .unwrap_or_else(|_| Self::unstarted())
    }

    /// A listener that was never started (C# `new PerfCsvListener()`): `enqueue` ignores rows without counting them.
    pub fn unstarted() -> PerfCsvListener {
        PerfCsvListener {
            shared: Arc::new(Shared::new(false, DISPOSE_WAIT)),
            subscription: Mutex::new(None),
        }
    }

    fn start(
        sink: Box<dyn Write + Send>,
        capacity: usize,
        idle_flush: Duration,
        dispose_wait: Duration,
        commit: Option<&str>,
        diag: &dyn Fn(&str) -> Option<String>,
    ) -> io::Result<PerfCsvListener> {
        let shared = Arc::new(Shared::new(true, dispose_wait));
        let mut writer = BufWriter::with_capacity(WRITER_BUFFER, sink);
        // The header goes into the buffer before the thread starts; it reaches the sink with the first flush.
        write!(
            writer,
            "# commit={} diag={} qpcFrequency={}\r\n",
            commit.unwrap_or("unknown"),
            diag_field(diag),
            STOPWATCH_FREQUENCY
        )?;
        writer.write_all(b"utcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\r\n")?;

        let (tx, rx) = sync_channel::<PerfRow>(capacity.max(1));
        *shared.sender.write().unwrap_or_else(PoisonError::into_inner) = Some(tx);

        let thread_shared = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("perf-csv-writer".to_owned())
            .spawn(move || {
                // The loop swallows I/O errors itself; a panicking sink must still release dispose.
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    writer_loop(rx, writer, &thread_shared, idle_flush);
                }));
                thread_shared.signal_finished();
            });
        if let Err(e) = spawned {
            *shared.sender.write().unwrap_or_else(PoisonError::into_inner) = None;
            return Err(e);
        }

        // Subscribe AFTER the queue exists, so no event can arrive at a listener that cannot take it.
        let subscription = PhotoReviewPerf::log().subscribe(Arc::clone(&shared) as Arc<dyn PerfListener>);
        Ok(PerfCsvListener {
            shared,
            subscription: Mutex::new(Some(subscription)),
        })
    }

    /// C# `Enqueue(Func<Row>)`: builds and queues one row; never panics. A builder that panics, a full queue, a closed
    /// queue and a faulted writer each count one dropped row. A never-started listener ignores the call.
    pub fn enqueue(&self, build: impl FnOnce() -> PerfRow) {
        self.shared.enqueue(build);
    }

    /// Rows lost so far (full queue, build failure, closed queue, writer fault). C# `DroppedCount`.
    pub fn dropped_count(&self) -> i64 {
        self.shared.dropped.load(Ordering::SeqCst)
    }

    /// True after a write or flush error stopped the writer. C# `WriterFaulted`.
    pub fn writer_faulted(&self) -> bool {
        self.shared.writer_faulted.load(Ordering::SeqCst)
    }

    /// Completes the queue, waits up to the dispose wait for the writer thread to drain it and write the
    /// `# dropped=<n>` trailer, and unsubscribes. Idempotent. If the wait times out the writer thread still writes the
    /// trailer when it gets there; nothing else touches the stream meanwhile (C# `FinishTask`, see
    /// [`wait_finished`](Self::wait_finished)).
    pub fn dispose(&self) {
        if self.shared.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        // Closing the queue lets the writer thread drain it and finish.
        *self.shared.sender.write().unwrap_or_else(PoisonError::into_inner) = None;
        self.wait_finished(self.shared.dispose_wait);
        let subscription = self.subscription.lock().unwrap_or_else(PoisonError::into_inner).take();
        drop(subscription);
    }

    /// Waits until the writer thread has written the trailer and released the stream; false on timeout. Test seam for
    /// C# `FinishTask`.
    pub fn wait_finished(&self, timeout: Duration) -> bool {
        let (lock, cv) = &self.shared.finished;
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = cv
            .wait_timeout_while(guard, timeout, |finished| !*finished)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

impl Drop for PerfCsvListener {
    fn drop(&mut self) {
        self.dispose();
    }
}
