//! Killers for the `session/writer.rs` mutants CL-MUT-PERFCOM never ran (lane cl-mut-writer2).
//!
//! Every wait on another thread is a bounded `recv_timeout`, so a mutant that makes the writer hang (lock never
//! released, completion never completed, `when_idle` blocked) fails an assertion instead of hanging the binary.
//! No fixed sleeps, no clock, no golden files; the file system is the in-memory fake.
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use pr_core::abstractions::{AppendStream, FileStat, FileSystem, Log, ReadSeek};
use pr_core::app_paths::AppPaths;
use pr_core::session::store::{SessionState, SessionStore};
use pr_core::session::writer::{
    Completion, DebounceTimer, RunningTimer, SaveSpawner, SessionWriter, TimerError, TimerOutcome,
};
use pr_core::sync::CancelToken;
use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog};

/// Hang guard for every wait on another thread.
const BOUND: Duration = Duration::from_secs(5);

/// Runs `work` on its own thread; fails the test when it does not return within `limit`.
fn bounded_for<T: Send + 'static>(limit: Duration, what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(limit)
        .unwrap_or_else(|e| panic!("{what} did not return within {limit:?}: {e}"))
}

fn bounded<T: Send + 'static>(what: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    bounded_for(BOUND, what, work)
}

// ---------------------------------------------------------------------------------------------------------------
// Test doubles (small copies of the ones in tests/session_writer.rs)
// ---------------------------------------------------------------------------------------------------------------

type Waiter = Arc<(Mutex<bool>, Condvar)>;

/// A timer the test fires by hand. `fail_wait`: the running timer's `wait` returns `Err`.
struct ManualTimer {
    waiters: Mutex<Vec<Waiter>>,
    requests: AtomicUsize,
    fail_wait: bool,
}

impl ManualTimer {
    fn new(fail_wait: bool) -> Self {
        Self {
            waiters: Mutex::new(Vec::new()),
            requests: AtomicUsize::new(0),
            fail_wait,
        }
    }

    fn requests(&self) -> usize {
        self.requests.load(SeqCst)
    }

    fn fire_all(&self) {
        let waiters = self.waiters.lock().unwrap_or_else(PoisonError::into_inner).clone();
        for waiter in waiters {
            *waiter.0.lock().unwrap_or_else(PoisonError::into_inner) = true;
            waiter.1.notify_all();
        }
    }
}

impl DebounceTimer for ManualTimer {
    fn start(&self, _span: Duration, cancel: &CancelToken) -> Result<Box<dyn RunningTimer>, TimerError> {
        self.requests.fetch_add(1, SeqCst);
        let waiter: Waiter = Arc::new((Mutex::new(false), Condvar::new()));
        self.waiters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::clone(&waiter));
        Ok(Box::new(ManualRun {
            waiter,
            cancel: cancel.clone(),
            fail_wait: self.fail_wait,
        }))
    }
}

struct ManualRun {
    waiter: Waiter,
    cancel: CancelToken,
    fail_wait: bool,
}

impl RunningTimer for ManualRun {
    fn wait(self: Box<Self>) -> Result<TimerOutcome, TimerError> {
        if self.fail_wait {
            return Err(Box::new(io::Error::other("timer broke while waiting")));
        }
        let mut fired = self.waiter.0.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if *fired {
                return Ok(TimerOutcome::Elapsed);
            }
            if self.cancel.is_canceled() {
                return Ok(TimerOutcome::Canceled);
            }
            fired = self
                .waiter
                .1
                .wait_timeout(fired, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

type WriteHook = Arc<dyn Fn(&Path) + Send + Sync>;

/// `FileSystem` decorator: `write_all_text_atomic` first runs the hook (block, signal), then writes.
struct GateFs {
    inner: Arc<FakeFileSystem>,
    hook: Mutex<Option<WriteHook>>,
}

impl GateFs {
    fn set_hook(&self, hook: Option<WriteHook>) {
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = hook;
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
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.inner.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        let hook = self.hook.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(hook) = hook {
            hook(path);
        }
        self.inner.write_all_text_atomic(path, text, durable)
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

const FOLDER: &str = r"C:\photos";

struct Fixture {
    fs: Arc<GateFs>,
    timer: Arc<ManualTimer>,
    log: Arc<RecordingLog>,
    store: Arc<SessionStore>,
    writer: Arc<SessionWriter>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_timer(false)
    }

    fn with_timer(fail_wait: bool) -> Self {
        let fs = Arc::new(GateFs {
            inner: Arc::new(FakeFileSystem::new()),
            hook: Mutex::new(None),
        });
        let store = Arc::new(SessionStore::new(
            &paths(),
            Arc::clone(&fs) as Arc<dyn FileSystem>,
            None,
        ));
        let timer = Arc::new(ManualTimer::new(fail_wait));
        let log = Arc::new(RecordingLog::new());
        let writer = SessionWriter::new(
            Arc::clone(&store),
            Some(Arc::clone(&log) as Arc<dyn Log>),
            None,
            Some(Arc::clone(&timer) as Arc<dyn DebounceTimer>),
        )
        .unwrap_or_else(|e| panic!("writer: {e}"));
        Self {
            fs,
            timer,
            log,
            store,
            writer: Arc::new(writer),
        }
    }

    fn update(&self, current: &str) {
        let state = SessionState {
            folder: FOLDER.to_owned(),
            current_path: Some(current.to_owned()),
            ..SessionState::default()
        };
        self.writer.update(&state).unwrap_or_else(|_| panic!("update"));
    }

    /// The `current_path` the store has on "disk" for [`FOLDER`].
    fn current(&self) -> Option<String> {
        self.store.load(FOLDER).unwrap_or_else(|_| panic!("load")).current_path
    }

    fn has_error(&self, needle: &str) -> bool {
        self.log.contains(LogLevel::Error, needle)
    }

    fn errors(&self) -> Vec<String> {
        self.log
            .entries()
            .into_iter()
            .filter(|(level, _)| *level == LogLevel::Error)
            .map(|(_, message)| message)
            .collect()
    }

    fn flush(&self) {
        let writer = Arc::clone(&self.writer);
        bounded("flush", move || writer.flush());
    }

    fn when_idle(&self) {
        let writer = Arc::clone(&self.writer);
        bounded("when_idle", move || writer.when_idle());
    }

    fn dispose(&self) {
        let writer = Arc::clone(&self.writer);
        bounded("dispose", move || writer.dispose());
    }

    /// One debounced write on the timer thread (the unbounded path), finished when this returns.
    fn timer_write(&self, current: &str) {
        self.update(current);
        self.timer.fire_all();
        self.when_idle();
    }

    /// Makes the next file write block (after signalling `entered`) until `release` completes.
    fn block_writes(&self, entered: &Completion, release: &Completion) {
        let (entered, release) = (entered.clone(), release.clone());
        self.fs.set_hook(Some(Arc::new(move |_: &Path| {
            entered.complete();
            release.wait_timeout(BOUND);
        })));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Killers
// ---------------------------------------------------------------------------------------------------------------

// writer.rs:441:9 `run_timer -> ()`, 457:16 `delete !`, 457:73 `== -> !=` (the timer thread must write when its own id
// is current), 197:9 `CompleteOnDrop::drop -> ()` (the timer thread's completion, so `when_idle` returns).
#[test]
fn timer_thread_writes_when_it_is_the_current_timer_and_completes_when_idle() {
    let f = Fixture::new();
    f.timer_write("a");
    assert_eq!(f.current().as_deref(), Some("a"));
    assert!(f.errors().is_empty(), "unexpected errors: {:?}", f.errors());
}

// writer.rs:344:5 `spawn_save_thread -> Ok(Default::default())` (the work never runs), 559:12 `delete !` (a finished save
// must not be reported as "may be lost"), 524:12 `delete !` and 243:9 `acquire_timeout -> false` (a free lock must be
// acquired), 197:9 (the save thread's completion).
#[test]
fn bounded_flush_with_a_free_lock_writes_and_reports_no_error() {
    let f = Fixture::new();
    f.writer.set_bounded_wait_for_tests(Duration::from_millis(300));
    f.update("a");
    f.flush();
    assert_eq!(f.current().as_deref(), Some("a"));
    assert!(f.errors().is_empty(), "unexpected errors: {:?}", f.errors());
}

// writer.rs:234:9 `acquire -> ()` and 243:9 `acquire_timeout -> true` (a lock held by a timer-thread write must make a
// bounded flush give up and requeue), 716:9 `set_bounded_wait_for_tests -> ()` (the 50 ms override must apply: the
// default would wait 2 s, the flush is allowed 1 s).
#[test]
fn bounded_flush_behind_a_held_lock_gives_up_within_the_override_and_requeues() {
    let f = Fixture::new();
    let (entered, release) = (Completion::new(), Completion::new());
    f.block_writes(&entered, &release);
    f.update("a");
    f.timer.fire_all();
    assert!(entered.wait_timeout(BOUND), "the timer write never started");
    f.fs.set_hook(None);

    f.writer.set_bounded_wait_for_tests(Duration::from_millis(50));
    f.update("b");
    let writer = Arc::clone(&f.writer);
    bounded_for(Duration::from_secs(1), "flush behind a held lock", move || {
        writer.flush()
    });

    assert!(f.has_error("Session write skipped: writer busy"), "{:?}", f.errors());
    assert_eq!(f.current(), None, "nothing may land while the lock is held");

    release.complete();
    // The update of "b" scheduled a new timer, so `when_idle` does not cover the blocked write of "a": wait for it.
    let deadline = std::time::Instant::now() + BOUND;
    while f.current().is_none() && std::time::Instant::now() < deadline {
        thread::yield_now();
    }
    assert_eq!(
        f.current().as_deref(),
        Some("a"),
        "the blocked write of a lands once released"
    );
    f.flush();
    assert_eq!(
        f.current().as_deref(),
        Some("b"),
        "the skipped state is requeued and written"
    );
}

// writer.rs:262:9 `WriteLock::release -> ()` and 272:9 `ReleaseOnDrop::drop -> ()`: after one timer write and one
// flush the lock must be free again, so a later flush writes.
#[test]
fn the_lock_is_free_again_after_a_timer_write_and_after_a_flush() {
    let f = Fixture::new();
    f.writer.set_bounded_wait_for_tests(Duration::from_millis(100));
    f.timer_write("a");

    f.update("b");
    f.flush();
    assert_eq!(f.current().as_deref(), Some("b"), "flush after an unbounded write");

    f.update("c");
    f.flush();
    assert_eq!(f.current().as_deref(), Some("c"), "flush after a bounded write");
    assert!(!f.has_error("writer busy"), "{:?}", f.errors());
}

// writer.rs:510:12 `delete !` (flush is bounded, the timer write is not) and 736:9 `set_save_spawner_for_tests -> ()`
// (the installed spawner must be the one used for the bounded save).
#[test]
fn only_the_bounded_flush_starts_its_save_through_the_spawner() {
    let f = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let spawner: SaveSpawner = Box::new(move |work| {
        counter.fetch_add(1, SeqCst);
        let completion = Completion::new();
        let done = completion.clone();
        thread::spawn(move || {
            work();
            done.complete();
        });
        Ok(completion)
    });
    f.writer.set_save_spawner_for_tests(Some(spawner));

    f.timer_write("a");
    assert_eq!(calls.load(SeqCst), 0, "the timer-thread write does not use the spawner");

    f.update("b");
    f.flush();
    assert_eq!(calls.load(SeqCst), 1, "the bounded flush uses the spawner once");
    assert_eq!(f.current().as_deref(), Some("b"));
}

// writer.rs:723:9 `set_after_drain_for_tests -> ()`.
#[test]
fn the_after_drain_hook_runs_for_a_drained_batch() {
    let f = Fixture::new();
    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    f.writer
        .set_after_drain_for_tests(Some(Box::new(move || flag.store(true, SeqCst))));
    f.update("a");
    f.flush();
    assert!(
        ran.load(SeqCst),
        "the hook must run after a non-empty batch was drained"
    );
}

// writer.rs:360:9 `InFlightGuard::drop -> ()` (in_flight must drop back to 0, else dispose waits 2 s and logs), 490:32
// `+= -> -=` (u32 underflow panics inside the timer thread, so the write is lost); 490:32 `+= -> *=` is covered by
// `dispose_waits_for_the_write_the_timer_thread_already_drained`.
#[test]
fn dispose_after_a_completed_write_does_not_report_an_in_flight_write() {
    let f = Fixture::new();
    f.timer_write("a");
    assert_eq!(f.current().as_deref(), Some("a"));
    f.dispose();
    assert!(!f.has_error("still in flight"), "{:?}", f.errors());
    assert!(!f.has_error("Session write failed"), "{:?}", f.errors());
}

// writer.rs:489:16 `delete !` (an EMPTY batch must not count as in flight) and 619:27 `== -> !=` (the before-shutdown
// hook must not run when nothing is in flight).
#[test]
fn dispose_with_nothing_pending_does_not_wait_for_an_in_flight_write() {
    let f = Fixture::new();
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hook_calls);
    f.writer.set_before_shutdown_wait_for_tests(Some(Box::new(move || {
        counter.fetch_add(1, SeqCst);
    })));
    f.dispose();
    assert_eq!(
        hook_calls.load(SeqCst),
        0,
        "nothing is in flight, so there is nothing to wait for"
    );
    assert!(!f.has_error("still in flight"), "{:?}", f.errors());
}

// writer.rs:617:9 `wait_for_in_flight -> ()`, 626:30 `> -> ==`, `> -> <`, `> -> >=` (the loop must wait while a write is
// in flight and stop as soon as it landed, without a timeout error), 490:32 `+= -> *=` and 489:16 (the drained batch
// must count as in flight), 730:9 `set_before_shutdown_wait_for_tests -> ()` (the hook must run exactly once).
#[test]
fn dispose_waits_for_the_write_the_timer_thread_already_drained() {
    let f = Fixture::new();
    let (drained, dispose_waiting) = (Completion::new(), Completion::new());
    let (d, w) = (drained.clone(), dispose_waiting.clone());
    f.writer.set_after_drain_for_tests(Some(Box::new(move || {
        d.complete();
        w.wait_timeout(BOUND);
    })));
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let (counter, w) = (Arc::clone(&hook_calls), dispose_waiting.clone());
    f.writer.set_before_shutdown_wait_for_tests(Some(Box::new(move || {
        counter.fetch_add(1, SeqCst);
        w.complete();
    })));
    f.update("a");
    f.timer.fire_all();
    assert!(drained.wait_timeout(BOUND), "the timer thread never drained its batch");

    f.dispose();

    assert_eq!(
        f.current().as_deref(),
        Some("a"),
        "dispose must return only after the drained write landed"
    );
    assert_eq!(hook_calls.load(SeqCst), 1);
    assert!(!f.has_error("still in flight"), "{:?}", f.errors());
}

// writer.rs:701:9 `when_idle -> ()`: it must block while the timer thread is inside its write. The waiter is NOT allowed
// to return before the write is released (negative wait of 200 ms), and the write must have landed when it does.
#[test]
fn when_idle_blocks_until_the_timer_write_finished() {
    let f = Fixture::new();
    let (entered, release) = (Completion::new(), Completion::new());
    f.block_writes(&entered, &release);
    f.update("a");
    f.timer.fire_all();
    assert!(entered.wait_timeout(BOUND), "the timer write never started");

    let (tx, rx) = mpsc::channel();
    let writer = Arc::clone(&f.writer);
    thread::spawn(move || {
        writer.when_idle();
        let _ = tx.send(());
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "when_idle returned while the write was still blocked"
    );
    release.complete();
    assert!(
        rx.recv_timeout(BOUND).is_ok(),
        "when_idle never returned after the write finished"
    );
    assert_eq!(f.current().as_deref(), Some("a"));
}

// writer.rs:449:76 `== -> !=` in the timer-error branch: a timer whose `wait` fails must forget its own schedule, so
// the next update starts a second timer.
#[test]
fn a_timer_whose_wait_fails_forgets_the_schedule_so_the_next_update_reschedules() {
    let f = Fixture::with_timer(true);
    f.update("a");
    f.when_idle();
    assert!(f.has_error("Session write timer failed"), "{:?}", f.errors());
    assert_eq!(f.timer.requests(), 1);

    f.update("b");
    assert_eq!(f.timer.requests(), 2, "the failed timer must not stay scheduled");
    f.when_idle();
}
