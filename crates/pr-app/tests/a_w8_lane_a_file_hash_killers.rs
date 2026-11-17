//! Mutation killers for `pr_app::misc::file_hash` (lane A, part 1).
//!
//! The `a_` prefix is deliberate: `cargo test` runs test binaries in name order and stops at the first failing one. The
//! wait-loop mutants of `get` hang the unbounded tests in `file_hash.rs`, so this bounded file must run (and fail) first.
//! If it is renamed, or tests run with `--no-fail-fast`, those mutants time out again.
//!
//! Why the existing `file_hash*.rs` tests missed these mutants:
//! - the whole-file/stream compute and the post-hash check live in the private free function `compute_and_cache_parts`
//!   (run by `get` on its worker), which no test could reach directly; they are driven through `compute_and_cache_for_tests`.
//! - the constants, the English text, `Debug`/`Display`, `is_localized`/`user_text` were never asserted.
//! - the in-flight bookkeeping after a finished or cancelled flight and the "hash returns with a token that never fires"
//!   loop condition were only exercised through code paths that hide the difference.
//!
//! Expected values are hand-derived (SHA-256 of "abc" is the FIPS 180 vector; 16 MiB and 1 MiB are spelled out).
//! Every test that could block under a mutant runs on a worker thread with a 20 s `recv_timeout`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pr_app::misc::file_hash::{
    EnglishFileHashTexts, FileHashError, FileHashService, FileHashTexts, FileHasher, HASH_CACHE_CAPACITY_BYTES,
    HASH_STREAM_BUFFER_BYTES,
};
use pr_core::abstractions::{FileSystem, PhysicalSourceReader, ReadSeek, SourceReadPriority, SourceReader};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache::source_bytes::{SourceBytesCache, SourceBytesError};
use pr_test_support::{FakeFileSystem, TempRoot};

const BOUND: Duration = Duration::from_secs(20);
const PHYSICAL_MEMORY_BYTES: i64 = 64 * 1024 * 1024 * 1024;
/// FIPS 180-2 vector: SHA-256("abc"), upper-case hex (C# `Convert.ToHexString`).
const ABC_SHA256: &str = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";

/// Runs `f` on a worker thread; fails the test when it does not finish within [`BOUND`].
fn bounded<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(BOUND)
        .unwrap_or_else(|_| panic!("{what} did not finish within 20 s"))
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + BOUND;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(1));
    }
}

// ---------------------------------------------------------------- constants and texts

// C#: `16L * 1024 * 1024` and `1024 * 1024`.
#[test]
fn constants_are_16_mib_and_1_mib() {
    assert_eq!(HASH_CACHE_CAPACITY_BYTES, 16_777_216);
    assert_eq!(HASH_STREAM_BUFFER_BYTES, 1_048_576);
}

#[test]
fn english_text_is_the_catalog_sentence() {
    assert_eq!(
        EnglishFileHashTexts.file_changed_while_hashing(r"C:\x\a.jpg"),
        r"The file changed while its hash was being computed: C:\x\a.jpg"
    );
}

// ---------------------------------------------------------------- error formatting and localization

fn changed(texts: Arc<dyn FileHashTexts>) -> FileHashError {
    let path = "p.jpg".to_string();
    FileHashError::ChangedWhileHashing {
        path: path.clone(),
        user_text: Arc::new(move || texts.file_changed_while_hashing(&path)),
    }
}

/// A real `SourceBytesError::Io` that carries UI text: the reader reports a stream shorter than the stat length.
fn localized_source_error() -> SourceBytesError {
    struct ShortReader;
    impl SourceReader for ShortReader {
        fn open_source(&self, _p: &Path, _pr: SourceReadPriority, _b: usize) -> io::Result<Box<dyn ReadSeek>> {
            Ok(Box::new(io::Cursor::new(vec![1u8, 2])))
        }
    }
    let root = TempRoot::new("hash-lane-a-src");
    let path = root.join("f.bin");
    fs::write(&path, [1u8, 2, 3, 4, 5]).unwrap();
    let cache = SourceBytesCache::new(1024 * 1024, PHYSICAL_MEMORY_BYTES, Some(Arc::new(ShortReader))).unwrap();
    cache.get_or_read(&path).unwrap_err()
}

#[test]
fn debug_and_display_render_each_variant() {
    let io_err = FileHashError::Io(Arc::new(io::Error::new(io::ErrorKind::NotFound, "gone")));
    let src = FileHashError::Source(SourceBytesError::Argument { param_name: "x" });
    for e in [
        &FileHashError::Canceled,
        &io_err,
        &src,
        &changed(Arc::new(EnglishFileHashTexts)),
    ] {
        assert_eq!(format!("{e:?}"), format!("{e}"));
    }
    assert_eq!(format!("{}", FileHashError::Canceled), "operation canceled");
    assert_eq!(format!("{io_err}"), "gone");
    assert_eq!(format!("{src:?}"), "Invalid argument. (Parameter 'x')");
    assert_eq!(
        format!("{:?}", changed(Arc::new(EnglishFileHashTexts))),
        "The file changed while its hash was being computed: p.jpg"
    );
}

#[test]
fn is_localized_and_user_text_per_variant() {
    // not localized: no text, describe falls back to Display
    let plain = [
        FileHashError::Canceled,
        FileHashError::Io(Arc::new(io::Error::other("boom"))),
        FileHashError::Source(SourceBytesError::Argument { param_name: "x" }),
    ];
    for e in &plain {
        assert!(!e.is_localized(), "{e:?}");
        assert!(e.user_text().is_none(), "{e:?}");
        assert_eq!(e.describe(), e.to_string());
    }
    // ChangedWhileHashing: localized, text is the injected sentence
    struct Texts;
    impl FileHashTexts for Texts {
        fn file_changed_while_hashing(&self, path: &str) -> String {
            format!("TXT[{path}]")
        }
    }
    let c = changed(Arc::new(Texts));
    assert!(c.is_localized());
    assert_eq!((c.user_text().expect("text"))(), "TXT[p.jpg]");
    assert_eq!(c.describe(), "TXT[p.jpg]");
    // Source carrying UI text: localized and the text is forwarded
    let s = FileHashError::Source(localized_source_error());
    assert!(s.is_localized());
    let text = (s.user_text().expect("source text"))();
    assert!(text.starts_with("The file changed while it was being read: "), "{text}");
    assert_eq!(s.describe(), text);
}

#[test]
fn service_debug_is_the_opaque_struct_name() {
    let service = FileHashService::new(None).unwrap();
    assert_eq!(format!("{service:?}"), "FileHashService { .. }");
}

// ---------------------------------------------------------------- compute_and_cache_parts (seam)

struct CountingReader {
    opens: AtomicUsize,
}

impl CountingReader {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            opens: AtomicUsize::new(0),
        })
    }
    fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }
}

impl SourceReader for CountingReader {
    fn open_source(&self, p: &Path, pr: SourceReadPriority, b: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        PhysicalSourceReader.open_source(p, pr, b)
    }
}

struct Rig {
    _root: TempRoot,
    path: PathBuf,
    length: u64,
    stamp: DateTimeUtc,
    stream_reader: Arc<CountingReader>,
    cache_reader: Arc<CountingReader>,
    service: FileHashService,
}

/// A real file `name` with `content`; a byte cache of `cache_capacity` (or none) and a service whose stream reader and cache
/// reader are separate counters, so the chosen read path is visible.
fn rig(name: &str, content: &[u8], cache_capacity: Option<i64>, texts: Arc<dyn FileHashTexts>) -> Rig {
    let root = TempRoot::new("hash-lane-a");
    let path = root.join(name);
    fs::write(&path, content).unwrap();
    let stat = PhysicalFileSystem.try_get_file_stat(&path).unwrap().unwrap();
    let stream_reader = CountingReader::new();
    let cache_reader = CountingReader::new();
    let cache = cache_capacity.map(|c| {
        Arc::new(
            SourceBytesCache::new(
                c,
                PHYSICAL_MEMORY_BYTES,
                Some(cache_reader.clone() as Arc<dyn SourceReader>),
            )
            .unwrap(),
        )
    });
    let service = FileHashService::with_seams(
        cache,
        Arc::new(PhysicalFileSystem),
        stream_reader.clone() as Arc<dyn SourceReader>,
        texts,
    )
    .unwrap();
    Rig {
        _root: root,
        path,
        length: stat.length,
        stamp: stat.last_write_utc,
        stream_reader,
        cache_reader,
        service,
    }
}

fn compute(r: &Rig, length: u64, stamp: DateTimeUtc, generation: u64) -> Result<String, FileHashError> {
    r.service.compute_and_cache_for_tests(
        r.path.to_str().unwrap(),
        length,
        stamp,
        generation,
        &CancelToken::none(),
    )
}

#[test]
fn compute_small_non_raw_file_uses_whole_file_cache_and_publishes() {
    let r = rig("a.bin", b"abc", Some(1024 * 1024), Arc::new(EnglishFileHashTexts));
    assert_eq!(compute(&r, r.length, r.stamp, 0).unwrap(), ABC_SHA256);
    assert_eq!((r.cache_reader.opens(), r.stream_reader.opens()), (1, 0));
    assert_eq!(r.service.cache_count_for_tests(), 1);
}

#[test]
fn compute_raw_file_streams_even_when_the_cache_could_hold_it() {
    let r = rig("a.cr3", b"abc", Some(1024 * 1024), Arc::new(EnglishFileHashTexts));
    assert_eq!(compute(&r, r.length, r.stamp, 0).unwrap(), ABC_SHA256);
    assert_eq!((r.cache_reader.opens(), r.stream_reader.opens()), (0, 1));
}

#[test]
fn compute_file_larger_than_the_cache_streams() {
    let content = vec![7u8; 2048];
    let r = rig("big.bin", &content, Some(1024), Arc::new(EnglishFileHashTexts));
    let expected = {
        let w = rig("same.bin", &content, None, Arc::new(EnglishFileHashTexts));
        compute(&w, w.length, w.stamp, 0).unwrap()
    };
    assert_eq!(compute(&r, r.length, r.stamp, 0).unwrap(), expected);
    assert_eq!((r.cache_reader.opens(), r.stream_reader.opens()), (0, 1));
}

#[test]
fn compute_without_a_byte_cache_streams() {
    let r = rig("a.bin", b"abc", None, Arc::new(EnglishFileHashTexts));
    assert_eq!(compute(&r, r.length, r.stamp, 0).unwrap(), ABC_SHA256);
    assert_eq!(r.stream_reader.opens(), 1);
}

#[test]
fn compute_rejects_a_length_mismatch_alone() {
    struct Texts;
    impl FileHashTexts for Texts {
        fn file_changed_while_hashing(&self, path: &str) -> String {
            format!("CHANGED {path}")
        }
    }
    let r = rig("a.bin", b"abc", None, Arc::new(Texts));
    let err = compute(&r, r.length + 1, r.stamp, 0).unwrap_err();
    assert!(matches!(err, FileHashError::ChangedWhileHashing { .. }), "{err:?}");
    assert!(err.is_localized());
    assert_eq!(err.describe(), format!("CHANGED {}", r.path.to_str().unwrap()));
    assert_eq!(r.service.cache_count_for_tests(), 0);
}

#[test]
fn compute_rejects_a_last_write_mismatch_alone() {
    let r = rig("a.bin", b"abc", None, Arc::new(EnglishFileHashTexts));
    let other = DateTimeUtc::from_system_time(std::time::SystemTime::now() + Duration::from_secs(3600)).unwrap();
    assert_ne!(other, r.stamp);
    let err = compute(&r, r.length, other, 0).unwrap_err();
    assert!(matches!(err, FileHashError::ChangedWhileHashing { .. }), "{err:?}");
    assert_eq!(r.service.cache_count_for_tests(), 0);
}

#[test]
fn compute_publishes_only_for_the_current_generation() {
    let r = rig("a.bin", b"abc", None, Arc::new(EnglishFileHashTexts));
    r.service.clear(); // generation is now 1
    assert_eq!(compute(&r, r.length, r.stamp, 0).unwrap(), ABC_SHA256);
    assert_eq!(
        r.service.cache_count_for_tests(),
        0,
        "stale generation must not publish"
    );
    assert_eq!(compute(&r, r.length, r.stamp, 1).unwrap(), ABC_SHA256);
    assert_eq!(r.service.cache_count_for_tests(), 1);
}

// ---------------------------------------------------------------- get(): in-flight bookkeeping

struct Disk {
    _root: TempRoot,
    fs: Arc<FakeFileSystem>,
    path: PathBuf,
}

fn disk() -> Disk {
    let root = TempRoot::new("hash-lane-a-get");
    let path = root.join("photo.bin");
    let fs = Arc::new(FakeFileSystem::new());
    fs.add_file_at(
        &path,
        b"abc",
        DateTimeUtc::from_system_time(std::time::UNIX_EPOCH + Duration::from_secs(1_767_323_045)).unwrap(),
    );
    Disk { _root: root, fs, path }
}

struct FakeReader {
    fs: Arc<FakeFileSystem>,
    gate: Mutex<Option<mpsc::Receiver<()>>>,
}

impl SourceReader for FakeReader {
    fn open_source(&self, p: &Path, _pr: SourceReadPriority, _b: usize) -> io::Result<Box<dyn ReadSeek>> {
        if let Some(g) = self.gate.lock().unwrap().take() {
            let _ = g.recv_timeout(BOUND);
        }
        let bytes = self
            .fs
            .file_content(p)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no fake file"))?;
        Ok(Box::new(io::Cursor::new(bytes)))
    }
}

fn service_on(d: &Disk, gate: Option<mpsc::Receiver<()>>) -> Arc<FileHashService> {
    Arc::new(
        FileHashService::with_seams(
            None,
            d.fs.clone() as Arc<dyn FileSystem>,
            Arc::new(FakeReader {
                fs: d.fs.clone(),
                gate: Mutex::new(gate),
            }),
            Arc::new(EnglishFileHashTexts),
        )
        .unwrap(),
    )
}

// `while result.is_none() && !canceled`: with `||` a token that never fires never leaves the wait loop.
#[test]
fn get_returns_with_a_token_that_never_fires() {
    let d = disk();
    let service = service_on(&d, None);
    let path = d.path.clone();
    let hash = bounded("get with CancelToken::none()", move || {
        service.get(&path, &CancelToken::none())
    });
    assert_eq!(hash.unwrap(), ABC_SHA256);
}

// After a successful computation the worker unregisters its flight (not canceled and still the registered one).
#[test]
fn get_unregisters_the_flight_after_success() {
    let d = disk();
    let service = service_on(&d, None);
    let path = d.path.clone();
    let s2 = service.clone();
    let p2 = path.clone();
    let hash = bounded("get", move || s2.get(&p2, &CancelToken::none()));
    assert_eq!(hash.unwrap(), ABC_SHA256);
    wait_until("worker to unregister its flight", || {
        service.in_flight_waiters_for_tests(&path).is_none()
    });
}

// A cancelled flight stays registered (C# keeps it so a late joiner can see it was cancelled and replace it):
// the worker must not unregister a flight whose token was cancelled.
#[test]
fn get_keeps_a_cancelled_flight_registered_after_the_worker_ends() {
    let d = disk();
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let service = service_on(&d, Some(gate_rx));
    let token = CancelToken::new();
    let caller = {
        let (s, p, t) = (service.clone(), d.path.clone(), token.clone());
        thread::spawn(move || s.get(&p, &t))
    };
    wait_until("the flight to start with one waiter", || {
        service.in_flight_waiters_for_tests(&d.path) == Some(1)
    });
    token.cancel();
    let result = bounded("cancelled caller", move || caller.join().unwrap());
    assert!(matches!(result, Err(FileHashError::Canceled)), "{result:?}");
    assert_eq!(service.in_flight_waiters_for_tests(&d.path), Some(0));
    drop(gate_tx); // release the blocked open: the worker now finishes as canceled
    // The worker ends within microseconds of the release; observe 300 ms for any unregistration.
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        assert_eq!(
            service.in_flight_waiters_for_tests(&d.path),
            Some(0),
            "a cancelled flight must stay registered"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
