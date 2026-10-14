//! Tests for `pr_core::session::writer` (port of `SessionWriter.cs`, C# baseline d92d6aae).
//!
//! Ports `tests/PhotoReview.Core.Tests/Session/SessionWriterTests.cs` and `SessionWriterShutdownAndVersionTests.cs`
//! (each Rust test names its C# twin), the golden `tests/golden/session-writer-sequence.json`, and own tests for the
//! Rust-only pieces (`Completion`, `ThreadTimer`, `Drop`).
//!
//! No sleeping to wait for a thread: every wait blocks on an event or a channel with a generous timeout (hang guard).
//!
//! C# tests without a Rust twin:
//! - `SessionWriterTests.Update_NullSkipped_DoesNotThrow`: `SessionState::skipped` is a `Vec<String>`, it cannot be null.
//! - `SessionWriterShutdownAndVersionTests.Constructor_InvalidDebounce_Throws` rows `-1` and `-5000`: a `Duration`
//!   cannot be negative (the golden row `ctor-negative-debounce` is skipped for the same reason).

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use pr_core::abstractions::{FileStat, FileSystem, ReadSeek};
use pr_core::app_paths::AppPaths;
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::session::writer::{
    Completion, DEFAULT_DEBOUNCE, DebounceTimer, InvalidDebounce, RunningTimer, SessionWriter, ThreadTimer, TimerError,
    TimerOutcome,
};
use pr_core::sync::CancelToken;
use pr_test_support::golden::{self, str_of};
use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog};
use serde_json::Value;

/// Hang guard for every wait on another thread.
const HANG: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------------------------------------------

type Waiter = Arc<(Mutex<Option<TimerOutcome>>, Condvar)>;

/// A timer the test fires by hand (C# `ManualDelay`).
struct ManualTimer {
    requested: Mutex<Vec<Duration>>,
    waiters: Mutex<Vec<Waiter>>,
    /// `false`: a cancelled timer keeps waiting until fired, so a "fire after flush" really reports `Elapsed` to the
    /// writer (it is then the writer's timer-id check that prevents a second write).
    honor_cancel: bool,
}

impl ManualTimer {
    fn new() -> Self {
        Self::with_cancel(true)
    }

    fn with_cancel(honor_cancel: bool) -> Self {
        Self {
            requested: Mutex::new(Vec::new()),
            waiters: Mutex::new(Vec::new()),
            honor_cancel,
        }
    }

    fn requested(&self) -> Vec<Duration> {
        self.requested.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn requests(&self) -> usize {
        self.requested().len()
    }

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
    fn start(&self, span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        let waiter: Waiter = Arc::new((Mutex::new(None), Condvar::new()));
        self.requested.lock().unwrap_or_else(PoisonError::into_inner).push(span);
        self.waiters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::clone(&waiter));
        Ok(Box::new(ManualRun {
            waiter,
            cancel: cancel.clone(),
            honor_cancel: self.honor_cancel,
        }))
    }
}

struct ManualRun {
    waiter: Waiter,
    cancel: CancelToken,
    honor_cancel: bool,
}

impl RunningTimer for ManualRun {
    fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError> {
        let mut outcome = self.waiter.0.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(result) = *outcome {
                return Ok(result);
            }
            if self.honor_cancel && self.cancel.is_canceled() {
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

/// The first `start` fails, later ones delegate to a [`ManualTimer`].
struct BrokenTimer {
    calls: AtomicUsize,
    inner: ManualTimer,
}

impl DebounceTimer for BrokenTimer {
    fn start(&self, span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        if self.calls.fetch_add(1, SeqCst) == 0 {
            return Err(Box::new(io::Error::other("timer broke")));
        }
        self.inner.start(span, cancel)
    }
}

type WriteHook = Arc<dyn Fn(&Path) + Send + Sync>;

/// `FileSystem` decorator over the fake (C# `InMemoryFileSystem.WriteHook`): `write_all_text_atomic` first runs the hook
/// (block, signal, panic), then writes, then reports the written path on the `landed` channel if one is set.
struct GateFs {
    inner: Arc<FakeFileSystem>,
    hook: Mutex<Option<WriteHook>>,
    landed: Mutex<Option<mpsc::Sender<PathBuf>>>,
}

impl GateFs {
    fn set_hook(&self, hook: Option<WriteHook>) {
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = hook;
    }

    fn landed_channel(&self) -> mpsc::Receiver<PathBuf> {
        let (tx, rx) = mpsc::channel();
        *self.landed.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
        rx
    }
}

impl FileSystem for GateFs {
    fn file_exists(&self, path: &Path) -> bool {
        self.inner.file_exists(path)
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.inner.directory_exists(path)
    }
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.inner.get_file_stat(path)
    }
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.move_file(source, destination)
    }
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.copy_file(source, destination)
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        self.inner.delete(path)
    }
    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.inner.open_read_shared(path, buffer_size)
    }
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn Write + Send>> {
        self.inner.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        let hook = self.hook.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(hook) = hook {
            hook(path);
        }
        let result = self.inner.write_all_text_atomic(path, text, durable);
        if let Some(tx) = &*self.landed.lock().unwrap_or_else(PoisonError::into_inner) {
            let _ = tx.send(path.to_path_buf());
        }
        result
    }
    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.inner.read_all_text(path)
    }
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_files(directory, pattern)
    }
    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_directories(directory)
    }
    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.inner.create_directory(path)
    }
    fn try_delete_empty_directory(&self, path: &Path) -> bool {
        self.inner.try_delete_empty_directory(path)
    }
    fn resolve_real_path(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.resolve_real_path(path)
    }
}

/// C# `ManualResetEventSlim` stand-in.
type Event = Completion;

fn wait_event(event: &Event, what: &str) {
    assert!(event.wait_timeout(HANG), "{what}");
}

/// Blocks the writing thread until `release` is set (bounded), after signalling `entered`.
fn blocking_hook(entered: &Event, release: &Event) -> WriteHook {
    let (entered, release) = (entered.clone(), release.clone());
    Arc::new(move |_: &Path| {
        entered.complete();
        release.wait_timeout(Duration::from_secs(60));
    })
}

/// Runs `work` on its own thread and returns its result, failing the test when it does not finish within [`HANG`].
fn run_bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(HANG)
        .unwrap_or_else(|e| panic!("{what} did not return within {HANG:?}: {e}"))
}

fn sessions_dir() -> PathBuf {
    PathBuf::from(r"C:\data\Sessions")
}

fn paths() -> AppPaths {
    AppPaths {
        config_file: PathBuf::from(r"C:\data\config.json"),
        journal_file: PathBuf::from(r"C:\data\operations.jsonl"),
        sessions_dir: sessions_dir(),
        log_file: PathBuf::from(r"C:\data\logs\app.log"),
        preview_cache_dir: PathBuf::from(r"C:\data\cache"),
        thumbnail_cache_dir: PathBuf::from(r"C:\data\thumbnails"),
        window_placement_file: PathBuf::from(r"C:\data\window-placement.json"),
    }
}

struct Fixture {
    inner: Arc<FakeFileSystem>,
    fs: Arc<GateFs>,
    writes: Arc<AtomicUsize>,
    timer: Arc<ManualTimer>,
    log: Arc<RecordingLog>,
    store: Arc<SessionStore>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_timer(ManualTimer::new())
    }

    fn with_timer(timer: ManualTimer) -> Self {
        let inner = Arc::new(FakeFileSystem::new());
        let fs = Arc::new(GateFs {
            inner: Arc::clone(&inner),
            hook: Mutex::new(None),
            landed: Mutex::new(None),
        });
        let writes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&writes);
        let store = Arc::new(SessionStore::new(
            &paths(),
            Arc::clone(&fs) as Arc<dyn FileSystem>,
            Some(Arc::new(move || {
                counter.fetch_add(1, SeqCst);
            })),
        ));
        Self {
            inner,
            fs,
            writes,
            timer: Arc::new(timer),
            log: Arc::new(RecordingLog::new()),
            store,
        }
    }

    fn writer_with(&self, debounce: Option<Duration>) -> SessionWriter {
        SessionWriter::new(
            Arc::clone(&self.store),
            Some(Arc::clone(&self.log) as Arc<dyn pr_core::abstractions::Log>),
            debounce,
            Some(Arc::clone(&self.timer) as Arc<dyn DebounceTimer>),
        )
        .unwrap_or_else(|e| panic!("writer: {e}"))
    }

    fn writer(&self) -> SessionWriter {
        self.writer_with(None)
    }

    fn writes(&self) -> usize {
        self.writes.load(SeqCst)
    }

    fn load(&self, folder: &str) -> SessionState {
        self.store.load(folder).unwrap_or_else(|_| panic!("load {folder:?}"))
    }

    fn current(&self, folder: &str) -> Option<String> {
        self.load(folder).current_path
    }

    fn errors(&self) -> Vec<String> {
        self.log
            .entries()
            .into_iter()
            .filter(|(level, _)| *level == LogLevel::Error)
            .map(|(_, message)| message)
            .collect()
    }

    fn has_error(&self, needle: &str) -> bool {
        self.log.contains(LogLevel::Error, needle)
    }

    fn session_files(&self) -> usize {
        let dir = sessions_dir();
        self.inner
            .all_files()
            .into_iter()
            .filter(|p| p.parent() == Some(dir.as_path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
            .count()
    }

    fn key_stem(&self, folder: &str) -> String {
        let path = self
            .store
            .get_path(folder)
            .unwrap_or_else(|e| panic!("get_path {folder:?}: {e}"));
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// A write hook that panics with "boom" for `folder`'s session file and lets every other write through.
    fn panic_for(&self, folder: &str) -> WriteHook {
        let stem = self.key_stem(folder).to_uppercase();
        Arc::new(move |path: &Path| {
            if path.to_string_lossy().to_uppercase().contains(&stem) {
                panic!("boom");
            }
        })
    }
}

fn state(folder: &str, current: &str, skipped: &[&str]) -> SessionState {
    SessionState {
        folder: folder.to_owned(),
        current_path: Some(current.to_owned()),
        skipped: skipped.iter().map(|s| (*s).to_owned()).collect(),
        ..SessionState::default()
    }
}

fn update(writer: &SessionWriter, folder: &str, current: &str) {
    writer
        .update(&state(folder, current, &[]))
        .unwrap_or_else(|_| panic!("update {folder:?}"));
}

// ---------------------------------------------------------------------------------------------------------------
// SessionWriterTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: SessionWriterTests.ManyUpdatesCoalesceIntoOneWrite
#[test]
fn many_updates_coalesce_into_one_write() {
    let f = Fixture::new();
    let writer = f.writer();
    for i in 0..100 {
        update(&writer, r"C:\photos", &format!(r"C:\photos\img{i}.jpg"));
    }

    assert_eq!(f.writes(), 0);
    assert_eq!(f.timer.requests(), 1);

    f.timer.fire_all();
    writer.when_idle();

    assert_eq!(f.writes(), 1);
    assert_eq!(f.current(r"C:\photos").as_deref(), Some(r"C:\photos\img99.jpg"));
}

// C#: SessionWriterTests.UpdateAfterWriteStartsNewWindow
#[test]
fn update_after_write_starts_new_window() {
    let f = Fixture::new();
    let writer = f.writer();
    update(&writer, r"C:\photos", "a");
    f.timer.fire_all();
    writer.when_idle();

    update(&writer, r"C:\photos", "b");

    assert_eq!(f.timer.requests(), 2);
    let handle = writer.flush_async().unwrap_or_else(|e| panic!("flush_async: {e}"));
    run_bounded("flush_async", move || handle.join().is_ok());
    assert_eq!(f.writes(), 2);
}

// C#: SessionWriterTests.NewerSnapshotWinsOverOlderTimerBatch
#[test]
fn newer_snapshot_wins_over_older_timer_batch() {
    let f = Fixture::new();
    let writer = f.writer();
    update(&writer, r"C:\photos", "old");

    f.timer.fire_all();
    update(&writer, r"C:\photos", "new");

    let handle = writer.flush_async().unwrap_or_else(|e| panic!("flush_async: {e}"));
    assert!(run_bounded("flush_async", move || handle.join().is_ok()));
    writer.when_idle();

    assert_eq!(f.current(r"C:\photos").as_deref(), Some("new"));
}

// C#: SessionWriterTests.FlushWritesImmediatelyAndOnlyOnce
#[test]
fn flush_writes_immediately_and_only_once() {
    // The timer ignores the cancel, so the late fire really reports Elapsed: the timer-id check must stop the 2nd write.
    let f = Fixture::with_timer(ManualTimer::with_cancel(false));
    let writer = f.writer();
    update(&writer, r"C:\photos", r"C:\photos\last.jpg");

    writer.flush();
    f.timer.fire_all();
    writer.when_idle();

    assert_eq!(f.writes(), 1);
    assert_eq!(f.current(r"C:\photos").as_deref(), Some(r"C:\photos\last.jpg"));
}

// C#: SessionWriterTests.FlushOnFolderChangePersistsOldFolder
#[test]
fn flush_on_folder_change_persists_old_folder() {
    let f = Fixture::new();
    let writer = f.writer();
    writer
        .update(&state(r"C:\old", r"C:\old\5.jpg", &[r"C:\old\skip.jpg"]))
        .unwrap_or_else(|_| panic!("update"));

    writer.flush();
    let loaded = f.load(r"C:\old");

    assert_eq!(loaded.current_path.as_deref(), Some(r"C:\old\5.jpg"));
    assert_eq!(loaded.skipped, vec![r"C:\old\skip.jpg".to_owned()]);
}

// C#: SessionWriterTests.EachFolderKeepsItsOwnPendingState
#[test]
fn each_folder_keeps_its_own_pending_state() {
    let f = Fixture::new();
    let writer = f.writer();
    update(&writer, r"C:\a", "a1");
    update(&writer, r"C:\b", "b1");
    update(&writer, r"C:\a", "a2");

    writer.flush();

    assert_eq!(f.current(r"C:\a").as_deref(), Some("a2"));
    assert_eq!(f.current(r"C:\b").as_deref(), Some("b1"));
    assert_eq!(f.writes(), 2);
}

// C#: SessionWriterTests.UpdateCopiesTheState
#[test]
fn update_copies_the_state() {
    let f = Fixture::new();
    let writer = f.writer();
    let mut s = state(r"C:\photos", "first", &["s1"]);
    writer.update(&s).unwrap_or_else(|_| panic!("update"));
    s.current_path = Some("mutated".to_owned());
    s.skipped.push("s2".to_owned());

    writer.flush();
    let loaded = f.load(r"C:\photos");

    assert_eq!(loaded.current_path.as_deref(), Some("first"));
    assert_eq!(loaded.skipped, vec!["s1".to_owned()]);
}

// C#: SessionWriterTests.DisposeFlushes
#[test]
fn dispose_flushes() {
    let f = Fixture::new();
    let writer = f.writer();
    update(&writer, r"C:\photos", "x");

    writer.dispose();

    assert_eq!(f.current(r"C:\photos").as_deref(), Some("x"));
}

// C#: SessionWriterTests.Dispose_ReturnsWithinBound_WhenWriteInFlight
#[test]
fn dispose_returns_within_bound_when_write_in_flight() {
    let f = Fixture::new();
    let writer = Arc::new(f.writer());
    writer.set_bounded_wait_for_tests(Duration::from_millis(200)); // C# runs with the 2 s default
    let (entered, release) = (Event::new(), Event::new());
    f.fs.set_hook(Some(blocking_hook(&entered, &release)));

    update(&writer, r"C:\photos", "a");
    f.timer.fire_all();
    wait_event(&entered, "the debounced write never started");
    update(&writer, r"C:\photos", "b"); // pending while the write for "a" is blocked

    let w = Arc::clone(&writer);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        w.dispose();
        let _ = tx.send(());
    });
    let completed = rx.recv_timeout(HANG);
    let landed = f.fs.landed_channel();
    release.complete();
    assert!(completed.is_ok(), "dispose did not return within its bound");

    writer.when_idle();
    f.fs.set_hook(None);
    // The in-flight write of "a" finishes on the timer thread after the release; when_idle does not cover it (C#
    // Wait.UntilAsync).
    assert!(
        landed.recv_timeout(HANG).is_ok(),
        "the in-flight write of a never landed"
    );
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("a")); // "b" was skipped, not written
    writer.dispose(); // double dispose is a no-op
    // Rust addition: the skipped "b" was not requeued after dispose, so not even a later flush writes it.
    writer.flush();
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("a"));
}

// C#: SessionWriterTests.Dispose_WhileTimerHoldsDrainedBatch_StillWritesIt
#[test]
fn dispose_while_timer_holds_drained_batch_still_writes_it() {
    let f = Fixture::new();
    let writer = f.writer();
    let (drained, proceed) = (Event::new(), Event::new());
    let (d, p) = (drained.clone(), proceed.clone());
    writer.set_after_drain_for_tests(Some(Box::new(move || {
        d.complete();
        p.wait_timeout(Duration::from_secs(30));
    })));
    update(&writer, r"C:\photos", "a");
    f.timer.fire_all();
    wait_event(&drained, "the timer never drained its batch");

    writer.set_after_drain_for_tests(None);
    let p = proceed.clone();
    writer.set_before_shutdown_wait_for_tests(Some(Box::new(move || p.complete())));
    writer.dispose();

    writer.when_idle();
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("a"));
}

// C#: SessionWriterTests.Dispose_WhileTimerHoldsDrainedBatch_ReturnsOnlyAfterTheWriteLanded
#[test]
fn dispose_while_timer_holds_drained_batch_returns_only_after_the_write_landed() {
    let f = Fixture::new();
    let writer = f.writer();
    let (drained, dispose_waiting) = (Event::new(), Event::new());
    let (d, w) = (drained.clone(), dispose_waiting.clone());
    writer.set_after_drain_for_tests(Some(Box::new(move || {
        d.complete();
        w.wait_timeout(Duration::from_secs(10));
    })));
    let w = dispose_waiting.clone();
    writer.set_before_shutdown_wait_for_tests(Some(Box::new(move || w.complete())));
    update(&writer, r"C:\photos", "a");
    f.timer.fire_all();
    wait_event(&drained, "the timer never drained its batch");

    writer.dispose();

    assert_eq!(f.current(r"C:\photos").as_deref(), Some("a")); // no waiting: dispose itself waited
}

// C#: SessionWriterTests.WriteBatch_NonIoException_DoesNotDropRestOfBatch
#[test]
fn write_batch_non_io_exception_does_not_drop_rest_of_batch() {
    let f = Fixture::new();
    let writer = f.writer();
    f.fs.set_hook(Some(f.panic_for(r"C:\bad"))); // a panic: the Rust image of a non-IO exception
    update(&writer, r"C:\bad", "x");
    update(&writer, r"C:\good", "y");

    f.timer.fire_all();
    writer.when_idle();

    assert_eq!(f.current(r"C:\good").as_deref(), Some("y"));
    assert!(f.has_error(r"Session write failed: C:\bad"));
}

// C#: SessionWriterTests.Update_TrailingSeparatorVariants_NewestWins
#[test]
fn update_trailing_separator_variants_newest_wins() {
    let f = Fixture::new();
    let writer = f.writer();
    update(&writer, r"C:\a\", "old");
    update(&writer, r"C:\a", "new");
    update(&writer, r"C:\A\", "newest");

    f.timer.fire_all();
    writer.when_idle();

    assert_eq!(f.writes(), 1);
    assert_eq!(f.current(r"C:\a").as_deref(), Some("newest"));
}

// C#: SessionWriterTests.Flush_WriterBusy_ReturnsAfterBoundedWaitAndRequeues
#[test]
fn flush_writer_busy_returns_after_bounded_wait_and_requeues() {
    let f = Fixture::new();
    let writer = Arc::new(f.writer());
    writer.set_bounded_wait_for_tests(Duration::from_millis(100));
    let (entered, release) = (Event::new(), Event::new());
    f.fs.set_hook(Some(blocking_hook(&entered, &release)));
    update(&writer, r"C:\photos", r"C:\photos\one.jpg");
    f.timer.fire_all(); // the timer thread's write blocks inside the file system while holding the writer lock
    wait_event(&entered, "the debounced write never started");
    f.fs.set_hook(None);

    update(&writer, r"C:\photos", r"C:\photos\two.jpg");
    let w = Arc::clone(&writer);
    run_bounded("flush behind a held lock", move || w.flush());

    release.complete();
    writer.when_idle();
    writer.flush();

    assert_eq!(f.current(r"C:\photos").as_deref(), Some(r"C:\photos\two.jpg"));
    assert!(f.has_error("Session write skipped: writer busy"));
}

// ---------------------------------------------------------------------------------------------------------------
// SessionWriterShutdownAndVersionTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: SessionWriterShutdownAndVersionTests.Update_CustomDebounce_IsPassedToTheTimer
#[test]
fn update_custom_debounce_is_passed_to_the_timer() {
    let f = Fixture::new();
    let writer = f.writer_with(Some(Duration::from_secs(7)));

    update(&writer, r"C:\photos", "a");

    assert_eq!(f.timer.requested(), vec![Duration::from_secs(7)]);
}

// C#: SessionWriterShutdownAndVersionTests.Update_NoDebounceGiven_UsesDefaultDebounce
#[test]
fn update_no_debounce_given_uses_default_debounce() {
    let f = Fixture::new();
    let writer = f.writer();

    update(&writer, r"C:\photos", "a");

    assert_eq!(f.timer.requested(), vec![DEFAULT_DEBOUNCE]);
    assert_eq!(DEFAULT_DEBOUNCE, Duration::from_millis(500));
}

// C#: SessionWriterShutdownAndVersionTests.Update_AfterDispose_IsIgnored
#[test]
fn update_after_dispose_is_ignored() {
    let f = Fixture::new();
    let writer = f.writer();
    writer.dispose();

    update(&writer, r"C:\photos", "late");
    writer.flush();

    assert!(f.timer.requested().is_empty());
    assert_eq!(f.writes(), 0);
    assert_eq!(f.current(r"C:\photos"), None);
}

// C#: SessionWriterShutdownAndVersionTests.Update_FolderThatCannotBeCanonicalised_DoesNotThrowAndOtherFoldersAreStillWritten
#[test]
fn update_folder_that_cannot_be_canonicalised_does_not_throw_and_other_folders_are_still_written() {
    let f = Fixture::new();
    let writer = f.writer();

    let result = writer.update(&state("C:\\bad\0folder", "x", &[]));
    update(&writer, r"C:\good", "y");
    writer.flush();

    assert!(result.is_ok());
    assert_eq!(f.current(r"C:\good").as_deref(), Some("y"));
}

// C#: SessionWriterShutdownAndVersionTests.Flush_StaleBatchDrainedBeforeNewerUpdate_DoesNotOverwriteTheNewerState
#[test]
fn flush_stale_batch_drained_before_newer_update_does_not_overwrite_the_newer_state() {
    let f = Fixture::new();
    let writer = Arc::new(f.writer());
    update(&writer, r"C:\photos", "old");
    let weak = Arc::downgrade(&writer);
    writer.set_after_drain_for_tests(Some(Box::new(move || {
        if let Some(w) = weak.upgrade() {
            w.set_after_drain_for_tests(None);
            update(&w, r"C:\photos", "new");
            w.flush(); // writes "new" while the outer flush still holds the drained "old" batch
        }
    })));

    writer.flush();

    assert_eq!(f.current(r"C:\photos").as_deref(), Some("new"));
    assert_eq!(f.writes(), 1);
}

// C#: SessionWriterShutdownAndVersionTests.Flush_SaveWorkerCannotBeStarted_ReleasesTheWriterLockAndTheStateIsWrittenByTheNextFlush
#[test]
fn flush_save_worker_cannot_be_started_releases_the_writer_lock_and_the_state_is_written_by_the_next_flush() {
    let f = Fixture::new();
    let writer = f.writer();
    writer.set_bounded_wait_for_tests(Duration::from_millis(200)); // a leaked lock makes the second flush give up after this
    writer.set_save_spawner_for_tests(Some(Box::new(|_work| {
        Err(io::Error::other("cannot start the save worker"))
    })));
    update(&writer, r"C:\photos", "kept");

    writer.flush();
    writer.set_save_spawner_for_tests(None);
    writer.flush();

    assert!(f.has_error("could not be started"));
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("kept"));
}

// C#: SessionWriterShutdownAndVersionTests.Flush_TimedOutBatchWhoseStateWasSuperseded_IsNotRequeuedOverTheNewerState
#[test]
fn flush_timed_out_batch_whose_state_was_superseded_is_not_requeued_over_the_newer_state() {
    let f = Fixture::new();
    let writer = Arc::new(f.writer());
    writer.set_bounded_wait_for_tests(Duration::from_millis(100));
    let (drained, go, entered, release) = (Event::new(), Event::new(), Event::new(), Event::new());

    update(&writer, r"C:\photos", "s1");
    let (d, g) = (drained.clone(), go.clone());
    writer.set_after_drain_for_tests(Some(Box::new(move || {
        d.complete();
        g.wait_timeout(Duration::from_secs(30));
    })));
    let w = Arc::clone(&writer);
    let flush1 = thread::spawn(move || w.flush()); // drains "s1", then pauses before taking the writer lock
    wait_event(&drained, "the first flush never drained its batch");
    writer.set_after_drain_for_tests(None);

    update(&writer, r"C:\photos", "s2");
    f.fs.set_hook(Some(blocking_hook(&entered, &release)));
    let landed = f.fs.landed_channel();
    let w = Arc::clone(&writer);
    let flush2 = thread::spawn(move || w.flush()); // drains "s2" and blocks inside the write holding the writer lock
    wait_event(&entered, "the second flush never started writing");
    f.fs.set_hook(None);

    go.complete();
    run_bounded("flush1", move || flush1.join().is_ok()); // times out on the held lock; "s1" is obsolete: dropped
    release.complete();
    run_bounded("flush2", move || flush2.join().is_ok());

    // flush2's caller may have given up waiting while its save was parked in the hook: wait for that save to land.
    landed
        .recv_timeout(HANG)
        .unwrap_or_else(|e| panic!("the superseding state s2 was never written: {e}"));
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("s2"));

    // A further flush must not requeue the obsolete "s1" over the newer state.
    writer.flush();
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("s2"));
}

// C#: SessionWriterShutdownAndVersionTests.Flush_FailingWrite_IsLoggedWithTheFolderAndTheException
// (`Assert.Same(boom, error.Exception)` has no twin: the panic is wrapped, its text is asserted instead.)
#[test]
fn flush_failing_write_is_logged_with_the_folder_and_the_exception() {
    let f = Fixture::new();
    let writer = f.writer();
    f.fs.set_hook(Some(f.panic_for(r"C:\bad")));
    update(&writer, r"C:\bad", "x");

    writer.flush();

    let errors = f.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains(r"C:\bad"), "{errors:?}");
    assert!(errors[0].contains("boom"), "{errors:?}");
}

// C#: SessionWriterShutdownAndVersionTests.Dispose_NothingEverPending_LogsNoError
#[test]
fn dispose_nothing_ever_pending_logs_no_error() {
    let f = Fixture::new();
    let writer = f.writer();
    writer.flush(); // an empty batch must not count as an in-flight write

    writer.dispose();

    assert!(f.errors().is_empty(), "{:?}", f.errors());
}

// C#: SessionWriterShutdownAndVersionTests.Dispose_AfterACompletedWrite_LogsNoError
#[test]
fn dispose_after_a_completed_write_logs_no_error() {
    let f = Fixture::new();
    let writer = f.writer();
    update(&writer, r"C:\photos", "a");
    writer.flush();

    writer.dispose();

    assert!(f.errors().is_empty(), "{:?}", f.errors());
}

// C#: SessionWriterShutdownAndVersionTests.Dispose_WhenTheDrainedBatchIsWrittenWhileWaiting_ReturnsWithoutTimeoutError
#[test]
fn dispose_when_the_drained_batch_is_written_while_waiting_returns_without_timeout_error() {
    let f = Fixture::new();
    let writer = f.writer();
    let (drained, dispose_waiting) = (Event::new(), Event::new());
    let (d, w) = (drained.clone(), dispose_waiting.clone());
    writer.set_after_drain_for_tests(Some(Box::new(move || {
        d.complete();
        w.wait_timeout(Duration::from_secs(10));
    })));
    let w = dispose_waiting.clone();
    writer.set_before_shutdown_wait_for_tests(Some(Box::new(move || w.complete())));
    update(&writer, r"C:\photos", "a");
    f.timer.fire_all();
    wait_event(&drained, "the timer never drained its batch");

    writer.dispose();

    assert_eq!(f.current(r"C:\photos").as_deref(), Some("a"));
    assert!(f.errors().is_empty(), "{:?}", f.errors());
}

// C#: SessionWriterShutdownAndVersionTests.Dispose_WriteStuckLongerThanShutdownWait_GivesUpAfterTheBoundIncludingTimeAlreadySpent
// The C# bound is 5.3 s; here 3.5 s, so that waiting the full 2 s again after the 1.9 s flush (~3.9 s) fails too.
#[test]
#[ignore = "slow: waits the 2 s shutdown budget"]
fn dispose_write_stuck_longer_than_shutdown_wait_gives_up_after_the_bound_including_time_already_spent() {
    let f = Fixture::new();
    let writer = Arc::new(f.writer());
    writer.set_bounded_wait_for_tests(Duration::from_millis(1900));
    let (entered, release) = (Event::new(), Event::new());
    f.fs.set_hook(Some(blocking_hook(&entered, &release)));
    update(&writer, r"C:\photos", "a");
    f.timer.fire_all();
    wait_event(&entered, "the debounced write never started");
    update(&writer, r"C:\photos", "b");

    let w = Arc::clone(&writer);
    let elapsed = run_bounded("dispose", move || {
        let started = Instant::now();
        w.dispose();
        started.elapsed()
    });
    release.complete();
    writer.when_idle();

    assert!(elapsed < Duration::from_millis(3500), "dispose took {elapsed:?}");
    assert!(f.has_error("may be lost at shutdown"));
}

// C#: SessionWriterShutdownAndVersionTests.Dispose_SaveStallsAfterTheLockWasAcquired_StillReturnsWithinTheBoundAndWritesLaterInTheBackground
#[test]
fn dispose_save_stalls_after_the_lock_was_acquired_still_returns_within_the_bound_and_writes_later_in_the_background() {
    let f = Fixture::new();
    let writer = Arc::new(f.writer());
    writer.set_bounded_wait_for_tests(Duration::from_millis(200));
    let (entered, release) = (Event::new(), Event::new());
    f.fs.set_hook(Some(blocking_hook(&entered, &release)));
    let landed = f.fs.landed_channel();
    update(&writer, r"C:\photos", "last");

    let w = Arc::clone(&writer);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        w.dispose();
        let _ = tx.send(());
    });
    let returned = rx.recv_timeout(HANG).is_ok();
    release.complete(); // also frees the stalled thread when the bound was exceeded

    assert!(entered.is_complete(), "the save never started");
    assert!(
        returned,
        "dispose blocked behind a stalled save instead of honouring the shutdown budget"
    );
    assert!(f.has_error("may be lost at shutdown"));
    // The write is only detached, not dropped: once the disk answers the state lands.
    landed
        .recv_timeout(HANG)
        .unwrap_or_else(|e| panic!("the detached save never completed: {e}"));
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("last"));
}

// C#: SessionWriterShutdownAndVersionTests.Constructor_InvalidDebounce_Throws (rows 0 and 4294967295; the negative rows
// have no `Duration` twin)
#[test]
fn constructor_invalid_debounce_throws() {
    let f = Fixture::new();
    for millis in [0_u64, 4_294_967_295] {
        let debounce = Duration::from_millis(millis);
        let result = SessionWriter::new(
            Arc::clone(&f.store),
            Some(Arc::clone(&f.log) as Arc<dyn pr_core::abstractions::Log>),
            Some(debounce),
            Some(Arc::clone(&f.timer) as Arc<dyn DebounceTimer>),
        );
        assert_eq!(result.err(), Some(InvalidDebounce(debounce)), "{millis} ms");
    }
    // A fraction above the limit is over it too (C# compares TotalMilliseconds as a double).
    let over = Duration::from_millis(4_294_967_294) + Duration::from_micros(1500);
    assert!(SessionWriter::new(Arc::clone(&f.store), None, Some(over), None).is_err());
    assert!(
        SessionWriter::new(
            Arc::clone(&f.store),
            None,
            Some(Duration::from_millis(4_294_967_294)),
            None
        )
        .is_ok()
    );
}

// C#: SessionWriterShutdownAndVersionTests.BrokenTimer_DoesNotFaultOrWedgeTheWriter
#[test]
fn broken_timer_does_not_fault_or_wedge_the_writer() {
    let f = Fixture::new();
    let timer = Arc::new(BrokenTimer {
        calls: AtomicUsize::new(0),
        inner: ManualTimer::new(),
    });
    let writer = Arc::new(
        SessionWriter::new(
            Arc::clone(&f.store),
            Some(Arc::clone(&f.log) as Arc<dyn pr_core::abstractions::Log>),
            None,
            Some(Arc::clone(&timer) as Arc<dyn DebounceTimer>),
        )
        .unwrap_or_else(|e| panic!("writer: {e}")),
    );

    update(&writer, r"C:\photos", "first");
    let w = Arc::clone(&writer);
    run_bounded("when_idle after a broken timer", move || w.when_idle());
    update(&writer, r"C:\photos", "second");
    writer.flush();

    assert_eq!(timer.calls.load(SeqCst), 2);
    assert!(f.has_error("Session write timer failed: timer broke"));
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("second"));
}

// ---------------------------------------------------------------------------------------------------------------
// Own tests (Rust-only pieces)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn completion_new_is_pending_and_completed_is_done() {
    let pending = Completion::new();
    assert!(!pending.is_complete());
    assert!(!pending.wait_timeout(Duration::from_millis(10)));
    assert!(Completion::completed().is_complete());
    assert!(Completion::completed().wait_timeout(Duration::ZERO));
    Completion::completed().wait();
}

#[test]
fn completion_complete_is_idempotent_and_wakes_waiters_on_other_threads() {
    let completion = Completion::new();
    let clone = completion.clone();
    let waiter = thread::spawn(move || clone.wait_timeout(HANG));
    completion.complete();
    completion.complete();
    assert!(completion.is_complete());
    assert!(run_bounded("waiter", move || waiter.join().unwrap_or(false)));
    completion.wait();
}

#[test]
fn thread_timer_elapses_after_the_span() {
    let token = CancelToken::new();
    let started = Instant::now();
    let running = ThreadTimer
        .start(Duration::from_millis(20), &token)
        .unwrap_or_else(|e| panic!("start: {e}"));
    let outcome = run_bounded("thread timer", move || running.wait().ok());
    assert_eq!(outcome, Some(TimerOutcome::Elapsed));
    assert!(started.elapsed() >= Duration::from_millis(20));
}

#[test]
fn thread_timer_cancelled_before_waiting_returns_canceled() {
    let token = CancelToken::new();
    let running = ThreadTimer
        .start(Duration::from_secs(30), &token)
        .unwrap_or_else(|e| panic!("start: {e}"));
    token.cancel();
    let outcome = run_bounded("cancelled thread timer", move || running.wait().ok());
    assert_eq!(outcome, Some(TimerOutcome::Canceled));
}

#[test]
fn thread_timer_cancelled_while_waiting_returns_canceled_well_before_the_span() {
    let token = CancelToken::new();
    let running = ThreadTimer
        .start(Duration::from_secs(30), &token)
        .unwrap_or_else(|e| panic!("start: {e}"));
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(running.wait().ok());
    });
    token.cancel();
    let outcome = rx
        .recv_timeout(HANG)
        .unwrap_or_else(|e| panic!("timer did not end: {e}"));
    assert_eq!(outcome, Some(TimerOutcome::Canceled));
}

#[test]
fn session_writer_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SessionWriter>();
    assert_send_sync::<Completion>();
}

#[test]
fn dropping_a_writer_with_a_pending_state_writes_it() {
    let f = Fixture::new();
    let writer = f.writer();
    update(&writer, r"C:\photos", "dropped");

    drop(writer);

    assert_eq!(f.current(r"C:\photos").as_deref(), Some("dropped"));
    assert_eq!(f.writes(), 1);
}

#[test]
fn production_thread_timer_writes_after_the_debounce() {
    let f = Fixture::new();
    let writer = SessionWriter::new(Arc::clone(&f.store), None, Some(Duration::from_millis(20)), None)
        .unwrap_or_else(|e| panic!("writer: {e}"));
    let landed = f.fs.landed_channel();
    update(&writer, r"C:\photos", "timed");

    landed
        .recv_timeout(HANG)
        .unwrap_or_else(|e| panic!("the debounced write never ran: {e}"));
    writer.when_idle();

    assert_eq!(f.current(r"C:\photos").as_deref(), Some("timed"));
    assert_eq!(f.writes(), 1);
}

// ---------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/session-writer-sequence.json (inputs: tests/golden-inputs/session-writer-sequence.json)
// ---------------------------------------------------------------------------------------------------------------

fn golden_inputs() -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("session-writer-sequence.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: no cases", path.display()))
        .clone()
}

fn opt_str(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn int(value: &Value) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("not an unsigned integer: {value}"))
}

#[test]
fn golden_session_writer_sequence() {
    let expected = golden::load("session-writer-sequence");
    let inputs = golden_inputs();
    assert_eq!(expected.cases.len(), inputs.len());

    for (input, case) in inputs.iter().zip(&expected.cases) {
        let name = str_of(case, "name");
        assert_eq!(str_of(input, "name"), name);

        let debounce = match &input["debounce_ms"] {
            Value::Null => None,
            value => match value.as_u64() {
                Some(ms) => Some(Duration::from_millis(ms)),
                None => {
                    // A negative TimeSpan has no Duration twin; C# rejects it.
                    assert_eq!(
                        case["ctor_exception"].as_str(),
                        Some("ArgumentOutOfRangeException"),
                        "{name}"
                    );
                    continue;
                }
            },
        };

        // The late-fire case needs a timer that reports Elapsed after the flush cancelled it.
        let honor_cancel = !name.contains("late-timer-fire");
        let f = Fixture::with_timer(ManualTimer::with_cancel(honor_cancel));
        let created = SessionWriter::new(
            Arc::clone(&f.store),
            Some(Arc::clone(&f.log) as Arc<dyn pr_core::abstractions::Log>),
            debounce,
            Some(Arc::clone(&f.timer) as Arc<dyn DebounceTimer>),
        );
        let writer = match created {
            Ok(writer) => {
                assert_eq!(case["ctor_exception"], Value::Null, "{name}");
                writer
            }
            Err(_) => {
                assert_eq!(
                    case["ctor_exception"].as_str(),
                    Some("ArgumentOutOfRangeException"),
                    "{name}"
                );
                continue;
            }
        };

        let ops = input["ops"].as_array().cloned().unwrap_or_default();
        let steps = case["steps"].as_array().cloned().unwrap_or_default();
        assert_eq!(ops.len(), steps.len(), "{name}");
        for (index, (op, step)) in ops.iter().zip(&steps).enumerate() {
            let kind = str_of(op, "op");
            assert_eq!(str_of(step, "op"), kind, "{name} step {index}");
            let exception = match kind {
                "update" => {
                    let s = SessionState {
                        folder: str_of(op, "folder").to_owned(),
                        current_path: opt_str(&op["current"]),
                        skipped: op["skipped"]
                            .as_array()
                            .map(|a| a.iter().filter_map(opt_str).collect())
                            .unwrap_or_default(),
                        ..SessionState::default()
                    };
                    writer.update(&s).err().map(|_| "ArgumentException")
                }
                "fire" => {
                    f.timer.fire_all();
                    writer.when_idle();
                    None
                }
                "flush" => {
                    writer.flush();
                    None
                }
                "dispose" => {
                    writer.dispose();
                    None
                }
                other => panic!("{name}: unknown op {other}"),
            };
            assert_eq!(exception, step["exception"].as_str(), "{name} step {index} exception");
            assert_eq!(
                f.timer.requests() as u64,
                int(&step["timer_requests"]),
                "{name} step {index} timer_requests"
            );
            assert_eq!(
                f.session_files() as u64,
                int(&step["files"]),
                "{name} step {index} files"
            );
        }

        let spans: Vec<u64> = f
            .timer
            .requested()
            .iter()
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .collect();
        let expected_spans: Vec<u64> = case["spans_ms"]
            .as_array()
            .map(|a| a.iter().map(int).collect())
            .unwrap_or_default();
        assert_eq!(spans, expected_spans, "{name} spans");

        for probe in case["probes"].as_array().cloned().unwrap_or_default() {
            let folder = str_of(&probe, "folder");
            let loaded = f.load(folder);
            assert_eq!(loaded.current_path, opt_str(&probe["current"]), "{name} probe {folder}");
            let skipped: Vec<String> = probe["skipped"]
                .as_array()
                .map(|a| a.iter().filter_map(opt_str).collect())
                .unwrap_or_default();
            assert_eq!(loaded.skipped, skipped, "{name} probe {folder} skipped");
        }
        assert_eq!(f.errors().len() as u64, int(&case["log_errors"]), "{name} log errors");
    }
}

// ===================== Verifier regressions (repair round 1) =====================

impl ManualTimer {
    /// Fires only the `index`-th registered timer (0 = first `start`).
    fn fire_index(&self, index: usize) {
        let waiter = self.waiters.lock().unwrap_or_else(PoisonError::into_inner)[index].clone();
        let mut outcome = waiter.0.lock().unwrap_or_else(PoisonError::into_inner);
        if outcome.is_none() {
            *outcome = Some(TimerOutcome::Elapsed);
        }
        waiter.1.notify_all();
    }
}

// Own test (contract mutation "drop the timer-id check"): a stale timer of a flushed window that reports Elapsed must not
// write the NEW window early.
#[test]
fn stale_timer_after_flush_does_not_write_the_new_window() {
    let f = Fixture::with_timer(ManualTimer::with_cancel(false));
    let writer = f.writer();
    update(&writer, r"C:\photos", "a");
    writer.flush();
    assert_eq!(f.writes(), 1);
    update(&writer, r"C:\photos", "b");
    assert_eq!(f.timer.requests(), 2);
    f.timer.fire_index(0); // the stale timer of the flushed window
    thread::sleep(Duration::from_millis(300));
    assert_eq!(f.writes(), 1, "stale timer wrote the new window early");
    f.timer.fire_index(1);
    writer.when_idle();
    assert_eq!(f.writes(), 2);
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("b"));
}

// Own test: flush forgets the schedule, so the next update opens a new debounce window.
#[test]
fn flush_forgets_the_schedule() {
    let f = Fixture::new();
    let writer = f.writer();
    update(&writer, r"C:\photos", "a");
    writer.flush();
    update(&writer, r"C:\photos", "b");
    assert_eq!(
        f.timer.requests(),
        2,
        "after flush the next update must schedule a new window"
    );
    f.timer.fire_all();
    writer.when_idle();
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("b"));
}

/// A timer whose `wait` fails (`Err`) or panics, counting `start` calls.
struct FailingWaitTimer {
    calls: AtomicUsize,
    panic: bool,
}

struct FailingRun {
    panic: bool,
}

impl RunningTimer for FailingRun {
    fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError> {
        if self.panic {
            panic!("wait panicked");
        }
        Err(Box::new(io::Error::other("wait broke")))
    }
}

impl DebounceTimer for FailingWaitTimer {
    fn start(&self, _span: Duration, _cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        self.calls.fetch_add(1, SeqCst);
        Ok(Box::new(FailingRun { panic: self.panic }))
    }
}

fn failing_wait_case(panic: bool) {
    let f = Fixture::new();
    let timer = Arc::new(FailingWaitTimer {
        calls: AtomicUsize::new(0),
        panic,
    });
    let writer = SessionWriter::new(
        Arc::clone(&f.store),
        Some(Arc::clone(&f.log) as Arc<dyn pr_core::abstractions::Log>),
        None,
        Some(Arc::clone(&timer) as Arc<dyn DebounceTimer>),
    )
    .unwrap_or_else(|e| panic!("writer: {e}"));
    update(&writer, r"C:\photos", "a");
    writer.when_idle();
    assert!(f.has_error("Session write timer failed"));
    update(&writer, r"C:\photos", "b");
    writer.when_idle();
    assert_eq!(timer.calls.load(SeqCst), 2, "second update must schedule again");
    writer.flush();
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("b"));
}

// Own test (C# RunTimerAsync catch around `await _delay`): a failing timer wait forgets the schedule.
#[test]
fn timer_wait_error_forgets_the_schedule() {
    failing_wait_case(false);
}

// Own test: a panicking timer wait is handled like an error (C# catches any exception).
#[test]
fn timer_wait_panic_forgets_the_schedule() {
    failing_wait_case(true);
}

/// A timer whose first `start` panics (the C# `_delay` throwing synchronously); later calls delegate to a ManualTimer.
struct PanicOnceTimer {
    calls: AtomicUsize,
    inner: ManualTimer,
}

impl DebounceTimer for PanicOnceTimer {
    fn start(&self, span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        if self.calls.fetch_add(1, SeqCst) == 0 {
            panic!("delay threw");
        }
        self.inner.start(span, cancel)
    }
}

// Own test (C# BrokenTimer_DoesNotFaultOrWedgeTheWriter with a throwing `_delay`): a panicking `start` neither escapes
// `update` nor wedges the writer.
#[test]
fn panicking_timer_start_does_not_escape_update_or_wedge_the_writer() {
    let f = Fixture::new();
    let timer = Arc::new(PanicOnceTimer {
        calls: AtomicUsize::new(0),
        inner: ManualTimer::new(),
    });
    let writer = SessionWriter::new(
        Arc::clone(&f.store),
        Some(Arc::clone(&f.log) as Arc<dyn pr_core::abstractions::Log>),
        None,
        Some(Arc::clone(&timer) as Arc<dyn DebounceTimer>),
    )
    .unwrap_or_else(|e| panic!("writer: {e}"));
    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        writer.update(&state(r"C:\photos", "a", &[]))
    }));
    assert!(
        matches!(first, Ok(Ok(()))),
        "C# Update never throws for a throwing delay"
    );
    assert!(f.has_error("Session write timer failed"));
    update(&writer, r"C:\photos", "b");
    assert_eq!(timer.calls.load(SeqCst), 2, "second update must schedule again");
    timer.inner.fire_all();
    writer.when_idle();
    assert_eq!(f.current(r"C:\photos").as_deref(), Some("b"));
}

// Own test (C# compares the fractional TimeSpan.TotalMilliseconds): 4294967294.5 ms is over the limit.
#[test]
fn debounce_fractional_ms_over_limit_rejected() {
    let f = Fixture::new();
    let d = Duration::from_micros(4_294_967_294_500);
    assert_eq!(
        SessionWriter::new(Arc::clone(&f.store), None, Some(d), None).err(),
        Some(InvalidDebounce(d))
    );
}

// Own test (semantics 7): one budget covers the lock wait AND the save; a flush that waited for the lock only has the
// rest of the budget left for its own save.
#[test]
fn bounded_flush_one_budget_covers_lock_wait_and_save() {
    let f = Fixture::new();
    let writer = Arc::new(f.writer());
    writer.set_bounded_wait_for_tests(Duration::from_millis(1200));
    let (entered, release) = (Event::new(), Event::new());
    f.fs.set_hook(Some(blocking_hook(&entered, &release)));
    update(&writer, r"C:\photos", "one");
    f.timer.fire_all();
    wait_event(&entered, "first save entered");
    // The flush's own save will block until released after the assertion.
    let (entered2, release2) = (Event::new(), Event::new());
    f.fs.set_hook(Some(blocking_hook(&entered2, &release2)));
    update(&writer, r"C:\photos", "two");
    let first_release = release.clone();
    let releaser = thread::spawn(move || {
        thread::sleep(Duration::from_millis(700));
        first_release.complete();
    });
    let started = Instant::now();
    let w = Arc::clone(&writer);
    run_bounded("flush", move || w.flush());
    let elapsed = started.elapsed();
    release2.complete();
    let _ = releaser.join();
    assert!(
        elapsed < Duration::from_millis(1500),
        "flush took {elapsed:?}; the budget must cover lock wait + save"
    );
    assert!(f.has_error("the save is still running after the wait budget"));
}
