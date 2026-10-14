//! Port of `PhotoReview.Core/Diagnostics/FileLog.cs` (C# baseline d92d6aae).
//!
//! Thread-safe, file-backed [`Log`] with an asynchronous writer thread, a bounded queue (10 000 entries, oldest dropped),
//! size-based rotation (one backup `app.1.log`, 10 MiB) and a bounded, non-spinning `flush`. INV-10: DISABLED BY DEFAULT and
//! then it does nothing at all: no thread, no directory, no file. Log lines are machine-read: invariant culture, UTF-8
//! without BOM, lines end with CRLF (what `StreamWriter.WriteLine` writes on Windows).
//!
//! Line: `yyyy-MM-dd HH:mm:ss.fff [LEVEL] [T<thread number>] <message>` and, when an error is attached, `\n` + the error
//! text. The timestamp is LOCAL time (C# `DateTime.Now`) truncated, not rounded, to milliseconds.
//!
//! Mapping from C# (docs/adr/R0003-concurrency.md): no kernel wait handles exist in Rust, so the C# handle-lifetime
//! machinery (`HandlesReleased`, deferred `ReleaseHandles`, `ObjectDisposedException` guards) is not ported; its
//! observable rules are: every method is a safe no-op after `dispose`, `dispose` never blocks longer than the shared
//! 2 s budget even when the writer is stuck, and enabling a disposed log starts nothing.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::abstractions::Log;
use crate::app_paths::AppPaths;

/// C# `FileLog.MaxQueuedEntries` (internal const): the queue never holds more entries; the OLDEST is dropped on overflow.
pub const MAX_QUEUED_ENTRIES: usize = 10_000;

/// C# default `maxLogBytes`: 10 MiB.
pub const DEFAULT_MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

/// Smallest accepted rotation size (C# `Math.Max(1024, maxLogBytes)`).
pub const MIN_MAX_LOG_BYTES: u64 = 1024;

/// The whole flush / shutdown budget (C# `Flush(2000)`, `Join(2000)`).
const FLUSH_BUDGET_MS: i64 = 2000;

/// The writer wakes at least this often even without a signal (C# `_signal.WaitOne(250)`).
const WRITER_POLL: Duration = Duration::from_millis(250);

/// Replaces an error text whose rendering panicked (C# `[exception text unavailable: ...]`; Rust has no type names).
const ERROR_TEXT_UNAVAILABLE: &str = "[exception text unavailable]";

/// A local wall-clock time as the log prints it (C# `DateTime.Now` fields).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogTimestamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// 0..=999, truncated from the finer clock reading (NOT rounded: `.9999999` prints `.999`).
    pub millisecond: u16,
}

impl LogTimestamp {
    /// `yyyy-MM-dd HH:mm:ss.fff` with ASCII digits, zero padded (year to 4 digits).
    pub fn format(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.millisecond
        )
    }
}

/// The source of "local now" (C# `DateTime.Now`). A trait so tests control it and `pr-core` stays free of platform code.
pub trait LocalTimeSource: Send + Sync {
    /// The current local time.
    fn now(&self) -> LogTimestamp;
}

/// Default source. `std` has no local time and lead decision D16 keeps platform code out of `pr-core`, so this reads
/// UTC; the application injects a real local-time source through [`FileLog::with_time_source`].
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemLocalTime;

impl LocalTimeSource for SystemLocalTime {
    fn now(&self) -> LogTimestamp {
        let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let secs = i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX / 2);
        // Truncated, not rounded: `subsec_millis` drops the finer digits.
        let millisecond = since_epoch.subsec_millis() as u16;
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        LogTimestamp {
            year: u16::try_from(year.clamp(1, 9999)).unwrap_or(1),
            month,
            day,
            hour: (rem / 3600) as u8,
            minute: ((rem % 3600) / 60) as u8,
            second: (rem % 60) as u8,
            millisecond,
        }
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day).
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// One queued log entry (private).
#[derive(Debug)]
struct Entry {
    level: &'static str,
    message: String,
    /// The attached error already rendered to text on the caller's thread (C# kept the `Exception` and called `ToString`
    /// on the writer thread; Rust cannot keep a borrowed `&dyn Error`).
    error_text: Option<String>,
    timestamp: LogTimestamp,
    thread_number: u32,
    /// `true` once this entry is on disk but still queued (a later entry of the same batch failed): a retry skips it.
    flushed: AtomicBool,
}

/// Test hooks, all optional (private storage of the C# `*Hook` properties).
#[derive(Default)]
struct Hooks {
    drain: Option<Arc<dyn Fn() + Send + Sync>>,
    after_batch_written: Option<Arc<dyn Fn() + Send + Sync>>,
    before_entry_written: Option<Arc<dyn Fn(usize) + Send + Sync>>,
}

/// State shared with the writer thread.
struct Inner {
    file_path: PathBuf,
    max_log_bytes: u64,
    time: Arc<dyn LocalTimeSource>,
    enabled: AtomicBool,
    stopping: AtomicBool,
    writing: AtomicBool,
    last_drain_failed: AtomicBool,
    disposed: AtomicBool,
    dropped: AtomicI64,
    queue: Mutex<VecDeque<Arc<Entry>>>,
    /// Auto-reset wake-up of the writer (C# `AutoResetEvent _signal`), waited with a 250 ms timeout.
    signal: (Mutex<bool>, Condvar),
    /// Manual-reset "queue drained" event (C# `ManualResetEventSlim _drained`, initially set).
    drained: (Mutex<bool>, Condvar),
    /// The writer thread handle; this mutex is also C# `_sync` (it serialises `set_enabled(true)`).
    writer: Mutex<Option<JoinHandle<()>>>,
    /// `true` while a writer thread is running; the writer clears it on exit and notifies `wait_writer_exit`.
    alive: (Mutex<bool>, Condvar),
    hooks: Mutex<Hooks>,
}

/// Locks `m`, ignoring poisoning: a panicking hook must not take the log down.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Clears the writer's `alive` flag when the thread ends, however it ends.
struct ExitGuard(Arc<Inner>);

impl Drop for ExitGuard {
    fn drop(&mut self) {
        let (flag, cv) = &self.0.alive;
        *lock(flag) = false;
        cv.notify_all();
    }
}

impl Inner {
    fn new(file_path: PathBuf, max_log_bytes: u64, time: Arc<dyn LocalTimeSource>) -> Self {
        Self {
            file_path,
            max_log_bytes: max_log_bytes.max(MIN_MAX_LOG_BYTES),
            time,
            enabled: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            writing: AtomicBool::new(false),
            last_drain_failed: AtomicBool::new(false),
            disposed: AtomicBool::new(false),
            dropped: AtomicI64::new(0),
            queue: Mutex::new(VecDeque::new()),
            signal: (Mutex::new(false), Condvar::new()),
            drained: (Mutex::new(true), Condvar::new()),
            writer: Mutex::new(None),
            alive: (Mutex::new(false), Condvar::new()),
            hooks: Mutex::new(Hooks::default()),
        }
    }

    fn signal_set(&self) {
        let (flag, cv) = &self.signal;
        *lock(flag) = true;
        cv.notify_one();
    }

    /// Waits for the signal or the timeout; consumes the signal (auto-reset).
    fn signal_wait(&self, timeout: Duration) {
        let (flag, cv) = &self.signal;
        let guard = lock(flag);
        let (mut guard, _) = cv
            .wait_timeout_while(guard, timeout, |set| !*set)
            .unwrap_or_else(PoisonError::into_inner);
        *guard = false;
    }

    fn drained_set(&self) {
        let (flag, cv) = &self.drained;
        *lock(flag) = true;
        cv.notify_all();
    }

    fn drained_reset(&self) {
        *lock(&self.drained.0) = false;
    }

    fn drained_is_set(&self) -> bool {
        *lock(&self.drained.0)
    }

    fn drained_wait(&self, timeout: Duration) {
        let (flag, cv) = &self.drained;
        let guard = lock(flag);
        let _ = cv
            .wait_timeout_while(guard, timeout, |set| !*set)
            .unwrap_or_else(PoisonError::into_inner);
    }

    fn is_alive(&self) -> bool {
        *lock(&self.alive.0)
    }

    /// `true` when no writer is running or it exited within `timeout`.
    fn wait_exit(&self, timeout: Duration) -> bool {
        let (flag, cv) = &self.alive;
        let guard = lock(flag);
        let (guard, _) = cv
            .wait_timeout_while(guard, timeout, |alive| *alive)
            .unwrap_or_else(PoisonError::into_inner);
        !*guard
    }

    fn queue_is_empty(&self) -> bool {
        lock(&self.queue).is_empty()
    }

    /// C# `Flush(int timeoutMs)`.
    fn flush_within(&self, timeout_ms: i64) {
        let mut remaining = timeout_ms;
        while ((!self.queue_is_empty() && !self.last_drain_failed.load(Ordering::SeqCst))
            || self.writing.load(Ordering::SeqCst))
            && remaining > 0
            // A disposed log whose writer is gone has nothing left to wait for (C#: `!HandlesReleased`).
            && !(self.disposed.load(Ordering::SeqCst) && !self.is_alive())
        {
            self.signal_set();
            let start = Instant::now();
            // `drained` can read set while an entry is already (or about to be) queued: the writer's empty-queue drain can
            // run between a producer's reset and enqueue. Waiting on a set event returns at once and would burn the whole
            // budget in a spin, so back off briefly and re-check the real condition instead.
            if self.drained_is_set() {
                std::thread::sleep(Duration::from_millis(1));
            } else {
                self.drained_wait(Duration::from_millis(remaining as u64));
            }
            let elapsed = i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX);
            remaining -= elapsed.max(1);
        }
    }

    fn write_entry(&self, level: &'static str, message: &str, error_text: Option<String>) {
        if !self.enabled.load(Ordering::SeqCst) || self.stopping.load(Ordering::SeqCst) {
            return;
        }
        self.drained_reset();
        self.last_drain_failed.store(false, Ordering::SeqCst);
        let entry = Arc::new(Entry {
            level,
            message: message.to_owned(),
            error_text,
            timestamp: self.time.now(),
            thread_number: current_thread_number(),
            flushed: AtomicBool::new(false),
        });
        {
            let mut queue = lock(&self.queue);
            queue.push_back(entry);
            if queue.len() > MAX_QUEUED_ENTRIES {
                queue.pop_front();
                self.dropped.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.signal_set();
    }

    fn writer_loop(&self) {
        while !self.stopping.load(Ordering::SeqCst) {
            self.signal_wait(WRITER_POLL);
            self.drain();
        }
        self.drain();
    }

    fn drain(&self) {
        let drain_hook = lock(&self.hooks).drain.clone();
        if let Some(hook) = drain_hook {
            // A panicking hook must not kill the writer thread.
            let _ = catch_unwind(AssertUnwindSafe(|| hook()));
        }
        if self.queue_is_empty() {
            self.drained_set();
            return;
        }

        self.writing.store(true, Ordering::SeqCst);
        let outcome = catch_unwind(AssertUnwindSafe(|| self.drain_to_disk()));
        if !matches!(outcome, Ok(Ok(()))) {
            // Log file unavailable (locked, read-only, bad path): entries stay queued (bounded by MAX_QUEUED_ENTRIES) and
            // are retried on the next wake-up, but waiters must not block for the full flush timeout.
            self.last_drain_failed.store(true, Ordering::SeqCst);
        }
        self.writing.store(false, Ordering::SeqCst);
        if self.queue_is_empty() || self.last_drain_failed.load(Ordering::SeqCst) {
            self.drained_set();
        }
    }

    fn needs_rotation(&self) -> bool {
        fs::metadata(&self.file_path)
            .map(|m| m.is_file() && m.len() >= self.max_log_bytes)
            .unwrap_or(false)
    }

    fn drain_to_disk(&self) -> io::Result<()> {
        let path = &self.file_path;
        // C# `Path.GetDirectoryName`: everything before the LAST separator, so a trailing separator makes the whole
        // path the directory (`C:\x\newdir\` -> `C:\x\newdir`), unlike `Path::parent`.
        let text = path.to_string_lossy();
        if let Some(idx) = text.rfind(['\\', '/']) {
            let dir = &text[..idx];
            if !dir.is_empty() {
                // `C:` (from `C:\x`) is a drive-relative path; `C:\` is the root.
                let dir = if dir.ends_with(':') {
                    format!("{dir}\\")
                } else {
                    dir.to_owned()
                };
                fs::create_dir_all(dir)?;
            }
        }
        if self.needs_rotation() {
            // The backup is read-only or held open (antivirus, an editor): keep appending to the current file rather than
            // failing every drain. Rotation is retried next drain. `rename` replaces an existing backup on Windows.
            let _ = fs::rename(path, backup_path(path));
        }

        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
        // Entries leave the queue only after they were written, so a failed write keeps them queued for the next drain.
        loop {
            let batch: Vec<Arc<Entry>> = lock(&self.queue).iter().cloned().collect();
            if batch.is_empty() {
                break;
            }
            // Format every entry BEFORE touching the file; entries already on disk (flushed) are not written again.
            let lines: Vec<Option<String>> = batch
                .iter()
                .map(|e| {
                    if e.flushed.load(Ordering::SeqCst) {
                        None
                    } else {
                        Some(format_queued(e))
                    }
                })
                .collect();

            let before_entry = lock(&self.hooks).before_entry_written.clone();
            let mut buffer: Vec<u8> = Vec::new();
            let mut last_written: Option<usize> = None;
            let staged = catch_unwind(AssertUnwindSafe(|| {
                for (i, line) in lines.iter().enumerate() {
                    let Some(line) = line else { continue };
                    if let Some(hook) = &before_entry {
                        hook(i);
                    }
                    buffer.extend_from_slice(line.as_bytes());
                    buffer.extend_from_slice(b"\r\n");
                    last_written = Some(i);
                }
            }));

            // What was staged is written even when staging was interrupted, so the retry does not repeat those lines.
            let mut offset = 0usize;
            let written = write_remaining(&mut file, &buffer, &mut offset);
            if staged.is_err() || written.is_err() {
                let on_disk = written.is_ok() || write_remaining(&mut file, &buffer, &mut offset).is_ok();
                if on_disk {
                    if let Some(last) = last_written {
                        for e in &batch[..=last] {
                            e.flushed.store(true, Ordering::SeqCst);
                        }
                    }
                }
                return Err(match written {
                    Err(e) => e,
                    Ok(()) => io::Error::other("log write interrupted"),
                });
            }

            for e in &batch {
                e.flushed.store(true, Ordering::SeqCst);
            }
            self.last_drain_failed.store(false, Ordering::SeqCst);
            let after_batch = lock(&self.hooks).after_batch_written.clone();
            if let Some(hook) = after_batch {
                hook();
            }
            // A producer may have trimmed the written head meanwhile, so the head can already be a newer, unwritten
            // entry: remove only entries that are still the written ones (identity, not position).
            let mut queue = lock(&self.queue);
            for e in &batch {
                if queue.front().is_some_and(|head| Arc::ptr_eq(head, e)) {
                    queue.pop_front();
                }
            }
        }
        Ok(())
    }
}

/// Writes `buffer[*offset..]`, advancing `offset` by what reached the file (so a retry never repeats bytes).
fn write_remaining(file: &mut fs::File, buffer: &[u8], offset: &mut usize) -> io::Result<()> {
    while *offset < buffer.len() {
        match file.write(&buffer[*offset..]) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
            Ok(n) => *offset += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    file.flush()
}

fn format_queued(e: &Entry) -> String {
    format_entry(
        &e.timestamp,
        e.level,
        e.thread_number,
        &e.message,
        e.error_text.as_deref(),
    )
}

/// Renders an error on the caller's thread: `Display`, then every `source()` as `"\n ---> {source}"` (rough analogue of
/// the .NET inner-exception chain). A `Display` that panics yields a fixed fallback text.
fn render_error(error: &(dyn Error + 'static)) -> String {
    catch_unwind(AssertUnwindSafe(|| {
        let mut text = error.to_string();
        let mut source = error.source();
        let mut depth = 0;
        while let Some(inner) = source {
            text.push_str("\n ---> ");
            text.push_str(&inner.to_string());
            source = inner.source();
            depth += 1;
            if depth >= 32 {
                break;
            }
        }
        text
    }))
    .unwrap_or_else(|_| ERROR_TEXT_UNAVAILABLE.to_owned())
}

/// See the module docs. Dropping a `FileLog` does the same as [`dispose`](Self::dispose).
pub struct FileLog {
    inner: Arc<Inner>,
}

impl fmt::Debug for FileLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileLog")
            .field("file_path", &self.inner.file_path)
            .finish_non_exhaustive()
    }
}

impl FileLog {
    /// C# `FileLog(string filePath, long maxLogBytes)`: stores `std::path::absolute(file_path)` and
    /// `max(MIN_MAX_LOG_BYTES, max_log_bytes)`. Disabled; starts nothing and touches no disk. `Err` for a blank path
    /// (`InvalidInput`) or when the path cannot be made absolute (embedded NUL, too long).
    pub fn new(file_path: impl AsRef<Path>, max_log_bytes: u64) -> io::Result<Self> {
        Self::with_time_source(file_path, max_log_bytes, Arc::new(SystemLocalTime))
    }

    /// C# `FileLog(IAppPaths, maxLogBytes)`: logs to `paths.log_file`.
    pub fn from_app_paths(paths: &AppPaths, max_log_bytes: u64) -> io::Result<Self> {
        Self::new(&paths.log_file, max_log_bytes)
    }

    /// Same as [`new`](Self::new) with an injected local-time source (tests).
    pub fn with_time_source(
        file_path: impl AsRef<Path>,
        max_log_bytes: u64,
        time: Arc<dyn LocalTimeSource>,
    ) -> io::Result<Self> {
        let path = file_path.as_ref();
        if path.to_string_lossy().trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the log file path must not be empty or white space",
            ));
        }
        let full = std::path::absolute(path)?;
        Ok(Self {
            inner: Arc::new(Inner::new(full, max_log_bytes, time)),
        })
    }

    /// C# `FileLog.Default`: the process-wide log at `AppPaths::from_environment().log_file` with the default size,
    /// created on first use, never disposed.
    pub fn default_instance() -> &'static FileLog {
        static DEFAULT: OnceLock<FileLog> = OnceLock::new();
        DEFAULT.get_or_init(|| {
            let paths = AppPaths::from_environment();
            Self::from_app_paths(&paths, DEFAULT_MAX_LOG_BYTES).unwrap_or_else(|_| FileLog {
                // The path could not be made absolute (never in practice): keep it as given rather than panic.
                inner: Arc::new(Inner::new(
                    paths.log_file.clone(),
                    DEFAULT_MAX_LOG_BYTES,
                    Arc::new(SystemLocalTime),
                )),
            })
        })
    }

    /// C# `FilePath`: the absolute log file path.
    pub fn file_path(&self) -> &Path {
        &self.inner.file_path
    }

    /// C# `Enabled { set }`. `true` starts the writer thread (named `PhotoReview.LogWriter`, background) when none is
    /// alive and enables logging; a no-op on a disposed log. `false` disables, wakes the writer and calls
    /// [`flush`](Self::flush): entries queued before the call are still written.
    pub fn set_enabled(&self, enabled: bool) {
        let inner = &self.inner;
        if enabled {
            let mut writer = lock(&inner.writer);
            // Enabling a disposed log must be a no-op.
            if inner.disposed.load(Ordering::SeqCst) {
                return;
            }
            inner.stopping.store(false, Ordering::SeqCst);
            inner.enabled.store(true, Ordering::SeqCst);
            if inner.is_alive() {
                return;
            }
            if let Some(old) = writer.take() {
                let _ = old.join(); // already exited
            }
            *lock(&inner.alive.0) = true;
            let shared = Arc::clone(inner);
            let spawned = std::thread::Builder::new()
                .name("PhotoReview.LogWriter".to_owned())
                .spawn(move || {
                    let _guard = ExitGuard(Arc::clone(&shared));
                    shared.writer_loop();
                });
            match spawned {
                Ok(handle) => *writer = Some(handle),
                Err(_) => {
                    // No thread, no logging (never a panic).
                    *lock(&inner.alive.0) = false;
                    inner.enabled.store(false, Ordering::SeqCst);
                }
            }
        } else {
            inner.enabled.store(false, Ordering::SeqCst);
            inner.signal_set();
            self.flush();
        }
    }

    /// C# `Flush()`: waits at most 2000 ms for the writer to drain the queue, without spinning; returns at once when
    /// the last drain failed (log file unavailable) so the UI thread never stalls on an unwritable log.
    pub fn flush(&self) {
        self.inner.flush_within(FLUSH_BUDGET_MS);
    }

    /// C# `Shutdown()`: stops accepting entries, wakes the writer, waits for it to exit and flushes, within ONE shared
    /// 2000 ms budget (join + flush together, not 2000 ms each).
    pub fn shutdown(&self) {
        let inner = &self.inner;
        inner.stopping.store(true, Ordering::SeqCst);
        inner.enabled.store(false, Ordering::SeqCst);
        inner.signal_set();
        let start = Instant::now();
        let exited = inner.wait_exit(Duration::from_millis(FLUSH_BUDGET_MS as u64));
        if exited {
            // The thread has already exited, so the join returns at once.
            let handle = lock(&inner.writer).take();
            if let Some(handle) = handle {
                let _ = handle.join();
            }
        }
        let used = i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX);
        inner.flush_within((FLUSH_BUDGET_MS - used).max(0));
    }

    /// C# `Dispose()`: [`shutdown`](Self::shutdown), once. Safe to call repeatedly and concurrently with writers.
    pub fn dispose(&self) {
        if self
            .inner
            .disposed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.shutdown();
        }
    }

    /// C# `PendingCount` (internal): entries currently queued.
    #[doc(hidden)]
    pub fn pending_count(&self) -> usize {
        lock(&self.inner.queue).len()
    }

    /// C# `DroppedCount` (internal): entries dropped because the queue was full.
    #[doc(hidden)]
    pub fn dropped_count(&self) -> i64 {
        self.inner.dropped.load(Ordering::SeqCst)
    }

    /// C# `IsWriterAlive` (internal).
    #[doc(hidden)]
    pub fn is_writer_alive(&self) -> bool {
        self.inner.is_alive()
    }

    /// C# `WaitWriterExit(timeoutMs)` (internal): `true` when no writer thread exists or it exited within `timeout`.
    #[doc(hidden)]
    pub fn wait_writer_exit(&self, timeout: Duration) -> bool {
        self.inner.wait_exit(timeout)
    }

    /// C# `DrainHook`: runs at the start of every drain, ON THE WRITER THREAD.
    #[doc(hidden)]
    pub fn set_drain_hook(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        lock(&self.inner.hooks).drain = hook;
    }

    /// C# `AfterBatchWrittenHook`: runs on the writer thread after a batch was flushed and before the written entries
    /// leave the queue.
    #[doc(hidden)]
    pub fn set_after_batch_written_hook(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        lock(&self.inner.hooks).after_batch_written = hook;
    }

    /// C# `BeforeEntryWrittenHook`: runs with the batch index before each entry is written to the stream.
    #[doc(hidden)]
    pub fn set_before_entry_written_hook(&self, hook: Option<Arc<dyn Fn(usize) + Send + Sync>>) {
        lock(&self.inner.hooks).before_entry_written = hook;
    }
}

impl Drop for FileLog {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl Log for FileLog {
    fn enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::SeqCst)
    }

    fn info(&self, message: &str) {
        self.inner.write_entry("INFO", message, None);
    }

    fn warn(&self, message: &str) {
        self.inner.write_entry("WARN", message, None);
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        let inner = &self.inner;
        // Cheap early exit so a disabled log never renders the error text.
        if !inner.enabled.load(Ordering::SeqCst) || inner.stopping.load(Ordering::SeqCst) {
            return;
        }
        inner.write_entry("ERROR", message, error.map(render_error));
    }
}

/// The number printed after `T` in a line: a small per-thread integer, 1 for the first thread that ever logs, 2 for the
/// next, and so on (C# `Environment.CurrentManagedThreadId`). Stable for the life of the thread.
pub fn current_thread_number() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    thread_local! {
        static NUMBER: u32 = NEXT.fetch_add(1, Ordering::Relaxed);
    }
    NUMBER.try_with(|n| *n).unwrap_or(0)
}

/// C# `FormatEntry`: `"{timestamp} [{level}] [T{thread}] {message}"` and, when `error_text` is `Some`, `"\n"` + the text.
/// No trailing line terminator (the writer adds CRLF).
pub fn format_entry(
    timestamp: &LogTimestamp,
    level: &str,
    thread_number: u32,
    message: &str,
    error_text: Option<&str>,
) -> String {
    let mut line = format!("{} [{level}] [T{thread_number}] {message}", timestamp.format());
    if let Some(text) = error_text {
        line.push('\n');
        line.push_str(text);
    }
    line
}

/// C# `Rotate` naming: `<dir>\<stem>.1<ext>` for a log file `<dir>\<stem><ext>` where `<stem>` is
/// `Path.GetFileNameWithoutExtension` and `<ext>` is `Path.GetExtension` including its dot (`app.log` -> `app.1.log`,
/// `noext` -> `noext.1`, `.log` -> `.1.log`, `x.tar.gz` -> `x.tar.1.gz`). Pinned by golden `file-log-backup-name`.
pub fn backup_path(log_file: &Path) -> PathBuf {
    let name = log_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // `Path.GetExtension`: from the LAST dot, dot included; "" when there is no dot or the dot is last.
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i + 1 < name.len() => (&name[..i], &name[i..]),
        Some(i) => (&name[..i], ""),
        None => (name.as_str(), ""),
    };
    let backup_name = format!("{stem}.1{ext}");
    match log_file.parent() {
        Some(dir) => dir.join(backup_name),
        None => PathBuf::from(backup_name),
    }
}
