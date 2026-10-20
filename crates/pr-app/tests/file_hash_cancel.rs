//! Red tests for the cancellation of `pr_app::misc::file_hash` (contract W8-04e, section "tests/file_hash_cancel.rs").
//! Port of `tests/PhotoReview.App.Tests/Services/FileHashServiceCancelTests.cs` (C# baseline d92d6aae). Requirement Q-R25:
//! cancelling one caller stops only that caller; the last caller stops the read.
//!
//! Fakes only: a [`FakeFileSystem`] holds the file and a [`GateReader`] counts the opens and can hold the first open at a gate
//! (a Rust reader cannot throw from a hook, so the gate replaces C#'s blocking `ContinueWith` window; contract section
//! "Test plan notes"). The gate is released by dropping its sender, and every wait is bounded by 30 s.
//!
//! Every test calls the stub, so each one fails through `todo!()` until the porter fills the bodies.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use pr_app::misc::file_hash::{EnglishFileHashTexts, FileHashError, FileHashService, FileHasher};
use pr_core::abstractions::{FileSystem, ReadSeek, SourceReadPriority, SourceReader};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeFileSystem, TempRoot};

const BOUND: Duration = Duration::from_secs(30);

/// Fake reader: counts the opens, and holds the first open until its gate sender is dropped (or [`BOUND`] passes).
struct GateReader {
    disk: Arc<FakeFileSystem>,
    opens: AtomicUsize,
    gate: Mutex<Option<Receiver<()>>>,
}

impl GateReader {
    fn new(disk: Arc<FakeFileSystem>, gate: Option<Receiver<()>>) -> Self {
        Self {
            disk,
            opens: AtomicUsize::new(0),
            gate: Mutex::new(gate),
        }
    }

    fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }
}

impl SourceReader for GateReader {
    fn open_source(
        &self,
        path: &Path,
        _priority: SourceReadPriority,
        _buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            // Released when the test drops the sender; `Disconnected` and a timeout both let the read continue.
            let _ = gate.recv_timeout(BOUND);
        }
        let bytes = self
            .disk
            .file_content(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no fake file"))?;
        Ok(Box::new(io::Cursor::new(bytes)))
    }
}

/// C# `Convert.ToHexString(SHA256.HashData(bytes))`: upper-case hex of the one-shot digest.
fn sha_hex(content: &[u8]) -> String {
    pr_core::session::store::sha256(content)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

/// Bounded wait for a condition the service reaches on its own threads (no fixed sleep).
fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + BOUND;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::yield_now();
    }
}

/// A fake disk holding one file of `len` bytes at `temp/photo.bin`, with a fixed last write time.
struct Fixture {
    _root: TempRoot,
    disk: Arc<FakeFileSystem>,
    path: PathBuf,
    content: Vec<u8>,
}

fn fixture(len: usize) -> Fixture {
    let root = TempRoot::new("hash-cancel");
    let path = root.join("photo.bin");
    let content: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
    let disk = Arc::new(FakeFileSystem::new());
    let stamp = DateTimeUtc::from_system_time(UNIX_EPOCH + Duration::from_secs(1_767_323_045)).unwrap();
    disk.add_file_at(&path, &content, stamp);
    Fixture {
        _root: root,
        disk,
        path,
        content,
    }
}

fn service_over(fx: &Fixture, reader: Arc<GateReader>) -> Arc<FileHashService> {
    let fs: Arc<dyn FileSystem> = Arc::clone(&fx.disk) as Arc<dyn FileSystem>;
    Arc::new(FileHashService::with_seams(None, fs, reader, Arc::new(EnglishFileHashTexts)).unwrap())
}

/// Starts `get` on its own thread and returns its join handle.
fn spawn_get(
    service: &Arc<FileHashService>,
    path: &Path,
    cancel: CancelToken,
) -> thread::JoinHandle<Result<String, FileHashError>> {
    let service = Arc::clone(service);
    let path = path.to_path_buf();
    thread::spawn(move || service.get(&path, &cancel))
}

// C#: FileHashServiceCancelTests.GetAsync_PreCanceled_ThrowsWithoutReading
#[test]
fn get_precanceled_returns_canceled_without_reading() {
    let fx = fixture(64 * 1024);
    let reader = Arc::new(GateReader::new(Arc::clone(&fx.disk), None));
    let service = service_over(&fx, Arc::clone(&reader));
    let cancel = CancelToken::new();
    cancel.cancel();

    let result = service.get(&fx.path, &cancel);

    assert!(matches!(result, Err(FileHashError::Canceled)), "got {result:?}");
    assert_eq!(reader.opens(), 0, "no read may be started for a cancelled request");
}

// C#: FileHashServiceCancelTests.GetAsync_CancelSoleCaller_ThenRetryGivesRealHash
#[test]
fn get_cancel_sole_caller_then_retry_gives_real_hash() {
    let fx = fixture(64 * 1024);
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let reader = Arc::new(GateReader::new(Arc::clone(&fx.disk), Some(gate_rx)));
    let service = service_over(&fx, reader);
    let cancel = CancelToken::new();
    let first = spawn_get(&service, &fx.path, cancel.clone());
    wait_until("the first caller to register", || {
        service.in_flight_waiters_for_tests(&fx.path) == Some(1)
    });

    cancel.cancel(); // the sole caller leaves: the shared computation is cancelled too

    assert!(matches!(first.join().unwrap(), Err(FileHashError::Canceled)));
    // No wait for the retry to be registered: on a loaded runner the retry can finish before a poll sees it,
    // so the test only asserts the outcome (a later request is not poisoned by the cancelled computation).
    let retry = spawn_get(&service, &fx.path, CancelToken::none());
    drop(gate_tx); // release the held read

    let hash = retry.join().unwrap().unwrap();
    assert_eq!(
        hash,
        sha_hex(&fx.content),
        "a cancelled computation must not poison a later request"
    );
}

// C#: FileHashServiceCancelTests.GetAsync_CancelOneOfTwoCallers_OtherStillCompletes
#[test]
fn get_cancel_one_of_two_callers_other_still_completes() {
    let fx = fixture(64 * 1024);
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let reader = Arc::new(GateReader::new(Arc::clone(&fx.disk), Some(gate_rx)));
    let service = service_over(&fx, Arc::clone(&reader));
    let first_cancel = CancelToken::new();
    let first = spawn_get(&service, &fx.path, first_cancel.clone());
    wait_until("the first caller to register", || {
        service.in_flight_waiters_for_tests(&fx.path) == Some(1)
    });
    let second = spawn_get(&service, &fx.path, CancelToken::none());
    wait_until("the second caller to join", || {
        service.in_flight_waiters_for_tests(&fx.path) == Some(2)
    });

    first_cancel.cancel();

    assert!(matches!(first.join().unwrap(), Err(FileHashError::Canceled)));
    drop(gate_tx);
    assert_eq!(second.join().unwrap().unwrap(), sha_hex(&fx.content));
    assert_eq!(reader.opens(), 1, "both callers share one computation");
}

// new: no computation is registered before the first call (the test seam reports None, contract `in_flight_waiters_for_tests`).
#[test]
fn in_flight_waiters_is_none_when_nothing_is_running() {
    let fx = fixture(4096);
    let reader = Arc::new(GateReader::new(Arc::clone(&fx.disk), None));
    let service = service_over(&fx, reader);

    assert_eq!(service.in_flight_waiters_for_tests(&fx.path), None);
}
