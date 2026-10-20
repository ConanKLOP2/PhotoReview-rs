//! Red tests for the in-flight hand-over and the post-hash validation of `pr_app::misc::file_hash` (contract W8-04e, section
//! "tests/file_hash_gap.rs"). Port of `tests/PhotoReview.App.Tests/Services/FileHashServiceGapTests.cs` (C# baseline d92d6aae),
//! RV-T29.
//!
//! The C# windows are reached through byte-cache test hooks (`BeforePublishForTests`, `AfterReadForTests`) that the Rust stub
//! does not expose. Each window is reached here through a seam the stub does have:
//! - "changed before publish" and "clear while in flight": a [`HookFileSystem`] stat hook. The SECOND stat of a call is the
//!   post-hash check (C# `CompleteIfUnchanged`), so the hook changes the file, or clears the service, just before it runs.
//! - "second caller joins a cancelled entry": a [`GateReader`] holds the first open until the test releases it, since a Rust
//!   hook cannot throw `OperationCanceledException` (contract section "Port" and decision D-04e-2).
//!
//! Fakes and a temp root only. Every wait is bounded by 30 s, and nothing sleeps.
//!
//! Every test calls the stub, so each one fails through `todo!()` until the porter fills the bodies.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pr_app::misc::file_hash::{EnglishFileHashTexts, FileHashError, FileHashService, FileHashTexts, FileHasher};
use pr_core::abstractions::{ReadSeek, SourceReadPriority, SourceReader};
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::get_full_path;
use pr_core::time::DateTimeUtc;
use pr_test_support::{FakeFileSystem, HookFileSystem, TempRoot};

const BOUND: Duration = Duration::from_secs(30);
const STAMP_UNIX_SECONDS: u64 = 1_767_323_045;

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

/// Deterministic bytes (C# `new Random(11).NextBytes`), so the expectation is reproducible.
fn content_bytes(len: usize) -> Vec<u8> {
    let mut state: u64 = 11u64.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
        })
        .collect()
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + BOUND;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::yield_now();
    }
}

fn stamp() -> DateTimeUtc {
    DateTimeUtc::from_system_time(UNIX_EPOCH + Duration::from_secs(STAMP_UNIX_SECONDS)).unwrap()
}

/// A fake disk with `photo.bin` (64 KiB) under a temp root. The fake keeps the file; the real disk is never touched.
struct Fixture {
    _root: TempRoot,
    disk: Arc<FakeFileSystem>,
    path: PathBuf,
    content: Vec<u8>,
}

fn fixture() -> Fixture {
    let root = TempRoot::new("hash-gap");
    let path = root.join("photo.bin");
    let content = content_bytes(64 * 1024);
    let disk = Arc::new(FakeFileSystem::new());
    disk.add_file_at(&path, &content, stamp());
    Fixture {
        _root: root,
        disk,
        path,
        content,
    }
}

fn is_photo(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "photo.bin")
}

/// Which property changes between the read and the post-hash check.
#[derive(Clone, Copy, Debug)]
enum Change {
    Mtime,
    Length,
}

// C#: FileHashServiceGapTests.GetAsync_WithoutAnyInterference_CachesTheHash
#[test]
fn get_without_interference_caches_the_hash() {
    let fx = fixture();
    let service = FileHashService::with_seams(
        None,
        Arc::clone(&fx.disk) as Arc<dyn pr_core::abstractions::FileSystem>,
        Arc::new(GateReader::new(Arc::clone(&fx.disk), None)),
        Arc::new(EnglishFileHashTexts),
    )
    .unwrap();

    let hash = service.get(&fx.path, &CancelToken::none()).unwrap();

    assert_eq!(hash, sha_hex(&fx.content));
    assert_eq!(service.cache_count_for_tests(), 1);
}

// C#: FileHashServiceGapTests.GetAsync_FileChangesAfterTheReadButBeforeCompletion_ThrowsLocalizedIOExceptionAndCachesNothing
// (Theory rows `mtime` and `length`)
#[test]
fn get_file_changed_before_publish_by_mtime_is_localized_and_uncached() {
    assert_changed_while_hashing(Change::Mtime);
}

#[test]
fn get_file_changed_before_publish_by_length_is_localized_and_uncached() {
    assert_changed_while_hashing(Change::Length);
}

fn assert_changed_while_hashing(change: Change) {
    let fx = fixture();
    let stat_calls = Arc::new(AtomicUsize::new(0));
    let hooked = Arc::new(HookFileSystem::new(Arc::clone(&fx.disk)));
    {
        let disk = Arc::clone(&fx.disk);
        let target = fx.path.clone();
        let content = fx.content.clone();
        let stat_calls = Arc::clone(&stat_calls);
        hooked.set_stat_hook(Some(Arc::new(move |p: &Path| {
            // The second stat of the call is the post-hash check: change the file just before it runs.
            if is_photo(p) && stat_calls.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                match change {
                    Change::Mtime => {
                        let later = SystemTime::now() + Duration::from_secs(300);
                        disk.set_last_write(&target, DateTimeUtc::from_system_time(later).unwrap());
                    }
                    Change::Length => {
                        let mut grown = content.clone();
                        grown.extend_from_slice(b"grew");
                        disk.add_file_at(&target, &grown, stamp());
                    }
                }
            }
            None
        })));
    }
    let service = FileHashService::with_seams(
        None,
        hooked as Arc<dyn pr_core::abstractions::FileSystem>,
        Arc::new(GateReader::new(Arc::clone(&fx.disk), None)),
        Arc::new(EnglishFileHashTexts),
    )
    .unwrap();

    let err = service.get(&fx.path, &CancelToken::none()).unwrap_err();

    assert!(matches!(err, FileHashError::ChangedWhileHashing { .. }), "got {err:?}");
    assert!(err.is_localized());
    let full = get_full_path(&fx.path.to_string_lossy()).unwrap();
    assert_eq!(err.describe(), EnglishFileHashTexts.file_changed_while_hashing(&full));
    assert_eq!(service.cache_count_for_tests(), 0);
}

// C#: FileHashServiceGapTests.GetAsync_ClearRunsWhileTheHashIsInFlight_ReturnsTheHashButDoesNotCacheIt
#[test]
fn get_clear_while_in_flight_returns_hash_but_does_not_cache_it() {
    let fx = fixture();
    let stat_calls = Arc::new(AtomicUsize::new(0));
    let service_slot: Arc<OnceLock<Weak<FileHashService>>> = Arc::new(OnceLock::new());
    let hooked = Arc::new(HookFileSystem::new(Arc::clone(&fx.disk)));
    {
        let stat_calls = Arc::clone(&stat_calls);
        let service_slot = Arc::clone(&service_slot);
        hooked.set_stat_hook(Some(Arc::new(move |p: &Path| {
            // Clear the service between the read and the publish (one-shot: only the first call's post-hash check).
            if is_photo(p) && stat_calls.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                if let Some(service) = service_slot.get().and_then(Weak::upgrade) {
                    service.clear();
                }
            }
            None
        })));
    }
    let service = Arc::new(
        FileHashService::with_seams(
            None,
            hooked as Arc<dyn pr_core::abstractions::FileSystem>,
            Arc::new(GateReader::new(Arc::clone(&fx.disk), None)),
            Arc::new(EnglishFileHashTexts),
        )
        .unwrap(),
    );
    assert!(service_slot.set(Arc::downgrade(&service)).is_ok());

    let hash = service.get(&fx.path, &CancelToken::none()).unwrap();

    assert_eq!(hash, sha_hex(&fx.content));
    assert_eq!(service.cache_count_for_tests(), 0);
    assert_eq!(
        service.get(&fx.path, &CancelToken::none()).unwrap(),
        hash,
        "a later request caches normally again"
    );
    assert_eq!(service.cache_count_for_tests(), 1);
}

// C#: FileHashServiceGapTests.GetAsync_SecondCallerJoinsAnEntryWhoseLastWaiterJustCancelled_RetriesAndReturnsTheRealHash
#[test]
fn get_second_caller_joining_cancelled_entry_retries_and_returns_real_hash() {
    let fx = fixture();
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let reader = Arc::new(GateReader::new(Arc::clone(&fx.disk), Some(gate_rx)));
    let service = Arc::new(
        FileHashService::with_seams(
            None,
            Arc::clone(&fx.disk) as Arc<dyn pr_core::abstractions::FileSystem>,
            Arc::clone(&reader) as Arc<dyn SourceReader>,
            Arc::new(EnglishFileHashTexts),
        )
        .unwrap(),
    );
    let first_cancel = CancelToken::new();
    let first = {
        let service = Arc::clone(&service);
        let path = fx.path.clone();
        let cancel = first_cancel.clone();
        thread::spawn(move || service.get(&path, &cancel))
    };
    wait_until("the first caller to register", || {
        service.in_flight_waiters_for_tests(&fx.path) == Some(1)
    });
    // The worker checks its cancel token before it opens the file, so cancelling too early would skip the first
    // open. Wait until the first read is held at its gate.
    wait_until("the first read to be held at the gate", || reader.opens() == 1);

    first_cancel.cancel(); // the only waiter leaves: the shared computation is cancelled
    assert!(matches!(first.join().unwrap(), Err(FileHashError::Canceled)));
    // The computation is still held at its gate. Intermediate registration states are not awaited here: on a loaded
    // runner the second caller can finish before a poll sees them. The outcome checks below still cover the behaviour.
    let second = {
        let service = Arc::clone(&service);
        let path = fx.path.clone();
        thread::spawn(move || service.get(&path, &CancelToken::none()))
    };
    drop(gate_tx); // release the held read: the computation observes its cancel, the second caller retries

    assert_eq!(second.join().unwrap().unwrap(), sha_hex(&fx.content));
    assert_eq!(reader.opens(), 2, "the retry starts a fresh computation");
}

// new: `clear()` empties the cache, and the next request hashes the file again with the same result (contract semantic C.22).
#[test]
fn clear_empties_the_cache_and_next_request_rehashes() {
    let fx = fixture();
    let service = FileHashService::with_seams(
        None,
        Arc::clone(&fx.disk) as Arc<dyn pr_core::abstractions::FileSystem>,
        Arc::new(GateReader::new(Arc::clone(&fx.disk), None)),
        Arc::new(EnglishFileHashTexts),
    )
    .unwrap();
    let hash = service.get(&fx.path, &CancelToken::none()).unwrap();
    assert_eq!(service.cache_count_for_tests(), 1);

    service.clear();

    assert_eq!(service.cache_count_for_tests(), 0);
    assert_eq!(service.get(&fx.path, &CancelToken::none()).unwrap(), hash);
}
