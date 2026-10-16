//! Port tests for `pr_imaging::cache::source_bytes` (C# `PhotoReview.Imaging/Caching/SourceBytesCache.cs` and
//! `SourceBytesCachePolicy.cs`, baseline d92d6aae). Red tests: every body of the unit is `todo!()` until the porter
//! fills it in.
//!
//! Ported C# classes (`tests/PhotoReview.Imaging.Tests/`), one `mod` per class:
//! - `Caching/SourceBytesCacheTests.cs`, `Caching/SourceBytesCacheGapTests.cs`, `Caching/SourceBytesCacheMutationTests.cs`,
//!   `Caching/SourceBytesCacheRobustnessTests.cs`, `Properties/SourceBytesCacheRangePropertyTests.cs`;
//! - `MemoryBudgetClampTests.cs`: only `SourceBytesCache_HugeRequest_IsClampedToHalfOfPhysical`;
//! - `Caching/ErrorHandlingReviewCachingTests.cs`: only `Evict_BoundsPathVersions_AndKeepsInvalidation`;
//! - `Decoding/DecoderErrorLocalizationTests.cs`: only `SourceBytesCache_FileChangedWhileReading_IsLocalized`, English half;
//! - the golden runner over `tests/golden/source-bytes-scenarios.json` (inputs in `tests/golden-inputs/`).
//!
//! Test conventions:
//! - Every cache is built with `physical_memory_bytes = 64 GiB` (or an explicit value), so the capacities used here are
//!   never clamped on any machine. The clamp itself is covered by `memory_budget_clamp`.
//! - No sleeps. Every thread wait is bounded by [`RACE_TIMEOUT`] through `mpsc::Receiver::recv_timeout`, and every
//!   blocking hook is released on the test thread even when an assertion fails.
//! - Files live in `pr_test_support::TempRoot`, removed on drop.
//!
//! Skipped parts (documented, not silently dropped):
//! - D41 / P-24: `pr-imaging` has no `pr-l10n` dependency, so the Vietnamese catalog text of the "file changed while
//!   reading" error and the golden `localized` flag are NOT checked here. The composition root injects that text
//!   (the D13 pattern); those checks port with that seam.
//! - `Caching/SourceBytesCacheGapTests.TryGetRange_CacheMiss_OutValueIsEmptyNotNull`: the Rust `Option` has no
//!   "empty array, not null" twin. Only the miss check (`Ok(None)`) is kept.
//! - The C# `Debug.Assert(SynchronizationContext.Current is null)` has no Rust twin (contract trap 11).

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier, Mutex, Weak};
use std::thread::{self, ThreadId};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pr_core::abstractions::{PhysicalSourceReader, ReadSeek, SourceReadPriority, SourceReader};
use pr_imaging::cache::source_bytes::{
    ARRAY_MAX_LENGTH, MAX_TRACKED_PATH_VERSIONS, SOURCE_READ_BUFFER_SIZE, SourceBytes, SourceBytesCache,
    SourceBytesError, TestHook,
};
use pr_imaging::ram_budget::clamp_source_bytes_to_physical_memory;
use pr_test_support::{TempRoot, golden};
use serde_json::Value;

/// C# `RaceTimeout`: the longest any test waits for a thread.
const RACE_TIMEOUT: Duration = Duration::from_secs(10);
const PHYSICAL_8_GIB: i64 = 8 * 1024 * 1024 * 1024;
const PHYSICAL_32_GIB: i64 = 32 * 1024 * 1024 * 1024;
const PHYSICAL_64_GIB: i64 = 64 * 1024 * 1024 * 1024;
/// .NET ticks at the Unix epoch (`DateTime(1970, 1, 1).Ticks`).
const TICKS_AT_UNIX_EPOCH: i64 = 621_355_968_000_000_000;
const VIEWER: SourceReadPriority = SourceReadPriority::Viewer;

// ---- shared helpers ----

/// `new SourceBytesCache(capacity)` with the physical reader and 64 GiB of RAM (no clamp for the capacities used here).
fn new_cache(capacity: i64) -> SourceBytesCache {
    SourceBytesCache::new(capacity, PHYSICAL_64_GIB, None).expect("cache with a positive capacity")
}

/// `new SourceBytesCache(capacity, reader)` with 64 GiB of RAM.
fn new_cache_with(capacity: i64, reader: Arc<dyn SourceReader>) -> SourceBytesCache {
    SourceBytesCache::new(capacity, PHYSICAL_64_GIB, Some(reader)).expect("cache with a positive capacity")
}

/// C# `DateTime.Ticks` of a UTC time.
fn dotnet_ticks(time: SystemTime) -> i64 {
    let since_epoch = time.duration_since(UNIX_EPOCH).expect("file times are after 1970");
    i64::try_from(since_epoch.as_nanos() / 100).expect("ticks fit i64") + TICKS_AT_UNIX_EPOCH
}

/// `(FileInfo.Length, FileInfo.LastWriteTimeUtc.Ticks)` of an existing file.
fn stat(path: &Path) -> (i64, i64) {
    let meta = fs::metadata(path).expect("stat fixture");
    let length = i64::try_from(meta.len()).expect("length fits i64");
    (length, dotnet_ticks(meta.modified().expect("modified time")))
}

fn file_len(path: &Path) -> i64 {
    stat(path).0
}

fn file_ticks(path: &Path) -> i64 {
    stat(path).1
}

/// C# `TempRoot.File(name, content)`: writes the file under the root and returns its path.
fn write_file(root: &TempRoot, name: &str, content: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, content).expect("write fixture");
    path
}

/// C# `Pattern(size)`: byte `i` is `i % 251`.
fn pattern(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// A fixture file with its identity, the C# `Fixture(name, size)` helper.
struct Fixture {
    path: PathBuf,
    length: i64,
    ticks: i64,
    content: Vec<u8>,
}

fn fixture(root: &TempRoot, name: &str, size: usize) -> Fixture {
    let content = pattern(size);
    let path = write_file(root, name, &content);
    let (length, ticks) = stat(&path);
    Fixture {
        path,
        length,
        ticks,
        content,
    }
}

/// `GetOrReadRange(path, length, ticks, offset, count)` of a fixture, Viewer priority.
fn read_range(cache: &SourceBytesCache, f: &Fixture, offset: i64, count: i32) -> Result<SourceBytes, SourceBytesError> {
    cache.get_or_read_range(&f.path, f.length, f.ticks, offset, count, VIEWER)
}

/// Wraps bytes in a `Box<dyn ReadSeek>` result for a reader closure.
fn boxed(stream: impl ReadSeek + 'static) -> io::Result<Box<dyn ReadSeek>> {
    Ok(Box::new(stream))
}

/// A test hook (C# `Action`): an `Arc<dyn Fn()>`.
fn hook(f: impl Fn() + Send + Sync + 'static) -> TestHook {
    Arc::new(f)
}

/// Moves a file's last write time by `minutes` from `base` (C# `AddMinutes`).
fn shift(base: SystemTime, minutes: i64) -> SystemTime {
    let delta = Duration::from_secs(60 * minutes.unsigned_abs());
    if minutes >= 0 { base + delta } else { base - delta }
}

/// C# `File.SetLastWriteTimeUtc(path, File.GetLastWriteTimeUtc(path).AddMinutes(minutes))`.
fn move_mtime(path: &Path, minutes: i64) -> io::Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    let modified = file.metadata()?.modified()?;
    file.set_modified(shift(modified, minutes))
}

/// Deterministic xorshift64 (no `rand` crate). Seeds map to a non-zero state.
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(
            seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x2545_F491_4F6C_DD1D)
                | 1,
        )
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in `0..n` (`n > 0`).
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Uniform in `lo..hi` (`hi > lo`), like C# `Random.Next(lo, hi)`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = u64::try_from(hi - lo).expect("range is non-empty");
        lo + i64::try_from(self.below(span)).expect("span fits i64")
    }
}

/// `count` deterministic bytes (C# `new Random(seed).NextBytes`, with xorshift instead).
fn random_bytes(seed: u64, count: usize) -> Vec<u8> {
    let mut rng = XorShift::new(seed);
    (0..count).map(|_| rng.next_u64() as u8).collect()
}

/// C# `ChunkedStream`: serves fixed bytes in chunks of at most `chunk` bytes. `Length` can be forced, and the data can
/// end early (`eof_after`).
struct ChunkedStream {
    data: Vec<u8>,
    chunk: usize,
    length_override: Option<i64>,
    eof_after: Option<usize>,
    pos: u64,
}

impl ChunkedStream {
    fn new(data: Vec<u8>, chunk: usize) -> Self {
        Self {
            data,
            chunk,
            length_override: None,
            eof_after: None,
            pos: 0,
        }
    }

    fn ending_after(mut self, bytes: usize) -> Self {
        self.eof_after = Some(bytes);
        self
    }

    fn length(&self) -> i64 {
        self.length_override
            .unwrap_or_else(|| i64::try_from(self.data.len()).expect("length fits i64"))
    }
}

impl Read for ChunkedStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let end = self.eof_after.unwrap_or(self.data.len()).min(self.data.len());
        let start = (self.pos as usize).min(end);
        let n = buf.len().min(self.chunk).min(end - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for ChunkedStream {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => i64::try_from(offset).unwrap_or(i64::MAX),
            SeekFrom::Current(offset) => i64::try_from(self.pos).expect("pos fits i64") + offset,
            SeekFrom::End(offset) => self.length() + offset,
        };
        self.pos = u64::try_from(target).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "negative seek"))?;
        Ok(self.pos)
    }
}

type OpenFn = dyn Fn(&Path, SourceReadPriority) -> io::Result<Box<dyn ReadSeek>> + Send + Sync;

/// C# `FakeReader` / `CountingReader`: counts opens and records the priority of each one. The open behaviour is a
/// closure, so one type serves the fakes ("must not open", served bytes) and the physical pass-through.
struct CountingReader {
    opens: AtomicUsize,
    priorities: Mutex<Vec<SourceReadPriority>>,
    open: Box<OpenFn>,
}

impl CountingReader {
    fn new(open: impl Fn(&Path, SourceReadPriority) -> io::Result<Box<dyn ReadSeek>> + Send + Sync + 'static) -> Self {
        Self {
            opens: AtomicUsize::new(0),
            priorities: Mutex::new(Vec::new()),
            open: Box::new(open),
        }
    }

    /// Counts opens of the real file through `PhysicalSourceReader` (C# `CountingReader`).
    fn physical() -> Self {
        Self::new(|path, priority| PhysicalSourceReader.open_source(path, priority, SOURCE_READ_BUFFER_SIZE))
    }

    /// Fails every open with "must not open": any read through it is a bug in the test.
    fn must_not_open() -> Self {
        Self::new(|_, _| Err(io::Error::other("must not open")))
    }

    fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }

    fn priorities(&self) -> Vec<SourceReadPriority> {
        self.priorities.lock().map(|log| log.clone()).unwrap_or_default()
    }

    /// Forgets the opens and priorities seen so far (the golden runner checks them per step).
    fn reset(&self) {
        self.opens.store(0, Ordering::SeqCst);
        if let Ok(mut log) = self.priorities.lock() {
            log.clear();
        }
    }
}

impl SourceReader for CountingReader {
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        _buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut log) = self.priorities.lock() {
            log.push(priority);
        }
        (self.open)(path, priority)
    }
}

/// Reads of the physical file system, plus the cache's per-file open count helper when a test needs it.
fn physical_reader() -> Arc<CountingReader> {
    Arc::new(CountingReader::physical())
}

// ---- SourceBytesCacheTests ----

mod source_bytes_cache {
    use super::*;

    #[test]
    fn reads_once_and_reuses_bytes() {
        // C#: SourceBytesCacheTests.ReadsOnceAndReusesBytes
        let root = TempRoot::new("src-bytes-reads-once");
        let path = write_file(&root, "image.bin", &[1, 2, 3, 4]);
        let cache = new_cache(1024);

        let first = cache.get_or_read(&path).expect("first read");
        let second = cache.get_or_read(&path).expect("second read");

        assert_eq!(&*first, &[1u8, 2, 3, 4]);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.count(), 1);
    }

    #[test]
    fn get_or_read_range_caches_only_requested_ranges() {
        // C#: SourceBytesCacheTests.GetOrReadRange_CachesOnlyRequestedRanges
        let root = TempRoot::new("src-bytes-ranges");
        let f = fixture(&root, "raw.dng", 4096);
        let cache = new_cache(1024);

        let first = read_range(&cache, &f, 512, 128).expect("first range");
        let first_again = read_range(&cache, &f, 512, 128).expect("same range again");
        let second = read_range(&cache, &f, 1024, 64).expect("second range");

        assert!(Arc::ptr_eq(&first, &first_again));
        assert_eq!(&*first, &f.content[512..640]);
        assert_eq!(&*second, &f.content[1024..1088]);
        assert_eq!(cache.current_size(), 192);
        assert_eq!(cache.count(), 2);
    }

    #[test]
    fn get_or_read_range_range_larger_than_capacity_is_read_without_thrashing_the_cache() {
        // C#: SourceBytesCacheTests.GetOrReadRange_RangeLargerThanCapacity_IsReadWithoutThrashingTheCache
        let root = TempRoot::new("src-bytes-oversized-range");
        let f = fixture(&root, "raw.dng", 4096);
        let cache = new_cache(1024);
        let kept = read_range(&cache, &f, 0, 128).expect("kept range");
        let publish_attempts = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&publish_attempts);
        let counting: TestHook = hook(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        cache.set_before_publish_for_tests(Some(counting));
        assert!(!cache.can_cache_range(2048));

        let oversized = read_range(&cache, &f, 512, 2048).expect("oversized range is still returned");

        assert_eq!(&*oversized, &f.content[512..2560]);
        assert_eq!(publish_attempts.load(Ordering::SeqCst), 0, "never offered to the cache");
        assert_eq!(cache.count(), 1);
        assert_eq!(cache.current_size(), 128);
        assert!(Arc::ptr_eq(
            &kept,
            &read_range(&cache, &f, 0, 128).expect("kept range again")
        ));
    }

    fn get_or_read_range_count_above_array_limit_is_rejected_before_open(overshoot: i32) {
        // C#: SourceBytesCacheTests.GetOrReadRange_CountAboveArrayLimitBypassingTheCache_ThrowsArgumentOutOfRangeNotOutOfMemory
        let root = TempRoot::new("src-bytes-count-limit");
        let reader = Arc::new(CountingReader::must_not_open());
        let cache = new_cache_with(1024, reader.clone());
        let count = i32::MAX - overshoot; // in (Array.MaxLength, i32::MAX]
        assert!(i64::from(count) > ARRAY_MAX_LENGTH);

        let result = cache.get_or_read_range(&root.join("never-opened.bin"), 3_000_000_000, 1, 0, count, VIEWER);

        assert!(matches!(
            result,
            Err(SourceBytesError::ArgumentOutOfRange { param_name: "count" })
        ));
        assert_eq!(reader.opens(), 0);
    }

    #[test]
    fn get_or_read_range_count_above_array_limit_bypassing_the_cache_throws_argument_out_of_range_not_out_of_memory_0()
    {
        // C#: SourceBytesCacheTests.GetOrReadRange_CountAboveArrayLimitBypassingTheCache_ThrowsArgumentOutOfRangeNotOutOfMemory (theory 0)
        get_or_read_range_count_above_array_limit_is_rejected_before_open(0);
    }

    #[test]
    fn get_or_read_range_count_above_array_limit_bypassing_the_cache_throws_argument_out_of_range_not_out_of_memory_1()
    {
        // C#: SourceBytesCacheTests.GetOrReadRange_CountAboveArrayLimitBypassingTheCache_ThrowsArgumentOutOfRangeNotOutOfMemory (theory 1)
        get_or_read_range_count_above_array_limit_is_rejected_before_open(1);
    }

    #[cfg(windows)]
    #[test]
    fn try_prefetch_skips_files_the_cache_could_never_keep() {
        // C#: SourceBytesCacheTests.TryPrefetch_SkipsFilesTheCacheCouldNeverKeep
        use std::os::windows::fs::OpenOptionsExt;
        let root = TempRoot::new("src-bytes-prefetch");
        let small = write_file(&root, "small.bin", &[0u8; 100]);
        let big = write_file(&root, "big.bin", &[0u8; 5000]);
        let cache = new_cache(1024);
        assert!(
            cache.capacity_bytes() >= 1024 && cache.capacity_bytes() < 5000,
            "the fixture assumes a 1 KB cache"
        );

        // An exclusive lock makes any attempt to read the big file fail with a sharing violation. Declared after `root`,
        // so the handle is closed before the folder is removed.
        let _exclusive = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&big)
            .expect("exclusive lock on the big file");
        assert!(matches!(cache.try_prefetch(&big), Ok(false)));
        assert_eq!(cache.count(), 0);

        assert!(matches!(cache.try_prefetch(&small), Ok(true)));
        assert_eq!(cache.count(), 1);
        assert_eq!(cache.current_size(), 100);
    }

    #[test]
    fn clear_and_evict_remove_entries() {
        // C#: SourceBytesCacheTests.ClearAndEvictRemoveEntries
        let root = TempRoot::new("src-bytes-clear-evict");
        let path = write_file(&root, "image.bin", &[9]);
        let cache = new_cache(1024);
        cache.get_or_read(&path).expect("read");
        cache.evict(&path).expect("evict");
        assert_eq!(cache.count(), 0);

        cache.get_or_read(&path).expect("read again");
        cache.clear();
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn evict_only_affects_that_path() {
        // C#: SourceBytesCacheTests.Evict_OnlyAffectsThatPath
        let root = TempRoot::new("src-bytes-evict-one");
        let gone = write_file(&root, "gone.bin", &[1]);
        let kept = write_file(&root, "kept.bin", &[2]);
        let cache = new_cache(1024);
        cache.get_or_read(&gone).expect("read gone");
        let kept_bytes = cache.get_or_read(&kept).expect("read kept");

        cache.evict(&gone).expect("evict gone");

        assert_eq!(cache.count(), 1);
        assert!(Arc::ptr_eq(&kept_bytes, &cache.get_or_read(&kept).expect("kept again")));
        cache.get_or_read(&gone).expect("re-read gone"); // evicting bumps only that path's version: caches again
        assert_eq!(cache.count(), 2);
    }

    #[test]
    fn get_or_read_reads_on_calling_thread() {
        // C#: SourceBytesCacheTests.GetOrRead_ReadsOnCallingThread
        // The worker is a dedicated thread (not a pool thread), so a Task.Run-style hop would change its id.
        let root = TempRoot::new("src-bytes-calling-thread");
        let path = write_file(&root, "sync.bin", &[1, 2, 3]);
        let cache = Arc::new(new_cache(1024));
        let (sent, received) = mpsc::channel::<(ThreadId, bool)>();
        let worker_cache = Arc::clone(&cache);
        let worker_path = path.clone();
        let _worker = thread::spawn(move || {
            let id = thread::current().id();
            let ok = worker_cache.get_or_read(&worker_path).is_ok();
            let _ = sent.send((id, ok));
        });

        // Bounded wait: a read that never returns fails this test instead of hanging the run.
        let (calling_thread, ok) = received
            .recv_timeout(RACE_TIMEOUT)
            .expect("GetOrRead returned within 10 s");

        assert!(ok);
        assert_eq!(cache.last_read_thread_id_for_tests(), Some(calling_thread));
    }
}

// ---- SourceBytesCacheGapTests ----

mod source_bytes_cache_gap {
    use super::*;

    #[test]
    fn try_get_range_after_get_or_read_range_is_a_hit() {
        // C#: SourceBytesCacheGapTests.TryGetRange_AfterGetOrReadRange_IsAHit
        let root = TempRoot::new("src-bytes-gap-hit");
        let f = fixture(&root, "raw.dng", 4096);
        let cache = new_cache(1024 * 1024);
        let read = read_range(&cache, &f, 512, 128).expect("read");

        let hit = cache
            .try_get_range(&f.path, f.length, f.ticks, 512, 128)
            .expect("no error")
            .expect("cached range is a hit");

        assert!(Arc::ptr_eq(&read, &hit));
        assert_eq!(&*hit, &f.content[512..640]);
    }

    #[test]
    fn try_get_range_not_cached_is_a_miss_without_reading() {
        // C#: SourceBytesCacheGapTests.TryGetRange_NotCached_IsAMissWithoutReading
        let root = TempRoot::new("src-bytes-gap-miss");
        let f = fixture(&root, "raw.dng", 4096);
        let reader = physical_reader();
        let cache = new_cache_with(1024 * 1024, reader.clone());
        read_range(&cache, &f, 512, 128).expect("read");
        let opens_after_read = reader.opens();

        assert!(matches!(
            cache.try_get_range(&f.path, f.length, f.ticks, 0, 128),
            Ok(None)
        ));
        assert!(matches!(
            cache.try_get_range(&f.path, f.length, f.ticks, 512, 64),
            Ok(None)
        ));
        assert!(matches!(
            cache.try_get_range(&f.path, f.length, f.ticks + 1, 512, 128),
            Ok(None)
        ));

        assert_eq!(cache.count(), 1);
        assert_eq!(reader.opens(), opens_after_read, "misses read nothing");
    }

    #[test]
    fn try_get_range_cache_miss_out_value_is_empty_not_null() {
        // C#: SourceBytesCacheGapTests.TryGetRange_CacheMiss_OutValueIsEmptyNotNull
        // The "empty, not null" out value has no Rust twin (`Option`); only the miss itself is checked here.
        let root = TempRoot::new("src-bytes-gap-empty-out");
        let f = fixture(&root, "raw.dng", 4096);
        let cache = new_cache(1024 * 1024);

        assert!(matches!(
            cache.try_get_range(&f.path, f.length, f.ticks, 0, 128),
            Ok(None)
        ));
    }

    #[test]
    fn try_get_range_invalid_range_returns_false() {
        // C#: SourceBytesCacheGapTests.TryGetRange_InvalidRange_ReturnsFalse (5 theory rows)
        let root = TempRoot::new("src-bytes-gap-invalid");
        let f = fixture(&root, "raw.dng", 4096);
        let cache = new_cache(1024 * 1024);
        cache
            .get_or_read(&f.path)
            .expect("the whole file is cached: only range validation can say no");
        // (offset, count): zero count, negative count, negative offset, offset past the end, runs past the end.
        let rows: [(i64, i32); 5] = [(0, 0), (0, -1), (-1, 16), (5000, 16), (4090, 16)];

        for (offset, count) in rows {
            assert!(
                matches!(cache.try_get_range(&f.path, f.length, f.ticks, offset, count), Ok(None)),
                "row offset={offset} count={count}"
            );
        }
    }

    #[test]
    fn try_get_range_after_evict_is_a_miss() {
        // C#: SourceBytesCacheGapTests.TryGetRange_AfterEvict_IsAMiss
        let root = TempRoot::new("src-bytes-gap-evict");
        let f = fixture(&root, "raw.dng", 4096);
        let cache = new_cache(1024 * 1024);
        read_range(&cache, &f, 512, 128).expect("read");
        assert!(matches!(
            cache.try_get_range(&f.path, f.length, f.ticks, 512, 128),
            Ok(Some(_))
        ));

        cache.evict(&f.path).expect("evict");

        assert!(matches!(
            cache.try_get_range(&f.path, f.length, f.ticks, 512, 128),
            Ok(None)
        ));
    }

    #[test]
    fn evict_while_read_in_flight_read_not_cached_other_path_untouched() {
        // C#: SourceBytesCacheGapTests.Evict_WhileReadInFlight_ReadNotCached_OtherPathUntouched
        let root = TempRoot::new("src-bytes-gap-inflight");
        let evicted = write_file(&root, "evicted.bin", &[0u8; 64]);
        let other = write_file(&root, "other.bin", &[0u8; 32]);
        let cache = Arc::new(new_cache(1024 * 1024));
        let other_bytes = cache.get_or_read(&other).expect("other file");

        let (read_done_tx, read_done_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Mutex::new(release_rx);
        cache.set_after_read_for_tests(Some(hook(move || {
            let _ = read_done_tx.send(());
            if let Ok(rx) = release_rx.lock() {
                let _ = rx.recv_timeout(RACE_TIMEOUT);
            }
        })));
        let (result_tx, result_rx) = mpsc::channel();
        let reader_cache = Arc::clone(&cache);
        let reader_path = evicted.clone();
        let _reader = thread::spawn(move || {
            let _ = result_tx.send(reader_cache.get_or_read(&reader_path));
        });

        let read_finished = read_done_rx.recv_timeout(RACE_TIMEOUT).is_ok();
        cache.set_after_read_for_tests(None);
        if read_finished {
            cache.evict(&evicted).expect("evict while the read is parked");
        }
        let _ = release_tx.send(());
        let result = result_rx.recv_timeout(RACE_TIMEOUT);

        assert!(read_finished, "reader never finished its read");
        let bytes = result
            .expect("reader thread finished after release")
            .expect("read of the evicted path");
        assert_eq!(bytes.len(), 64);
        assert_eq!(cache.count(), 1, "only the unrelated path");
        assert!(Arc::ptr_eq(
            &other_bytes,
            &cache.get_or_read(&other).expect("other file again")
        ));
        assert_eq!(cache.current_size(), 32);
    }
}

// ---- SourceBytesCacheMutationTests ----

mod source_bytes_cache_mutation {
    use super::*;

    #[test]
    fn get_or_read_injected_reader_serves_its_bytes() {
        // C#: SourceBytesCacheMutationTests.GetOrRead_InjectedReader_ServesItsBytes
        let root = TempRoot::new("src-bytes-mut-injected");
        let f = fixture(&root, "a.bin", 64);
        let served = vec![0xEEu8; 64];
        let reader = Arc::new(CountingReader::new(move |_, _| {
            boxed(ChunkedStream::new(served.clone(), 1024))
        }));
        let cache = new_cache_with(1024 * 1024, reader.clone());

        let bytes = cache.get_or_read(&f.path).expect("read through the injected reader");

        assert_eq!(&*bytes, &vec![0xEEu8; 64][..]);
        assert_eq!(reader.opens(), 1);
    }

    #[test]
    fn get_or_read_short_reads_are_assembled_in_order() {
        // C#: SourceBytesCacheMutationTests.GetOrRead_ShortReads_AreAssembledInOrder
        let root = TempRoot::new("src-bytes-mut-short");
        let f = fixture(&root, "short.bin", 100);
        let content = f.content.clone();
        let reader = Arc::new(CountingReader::new(move |_, _| {
            boxed(ChunkedStream::new(content.clone(), 7))
        }));
        let cache = new_cache_with(1024 * 1024, reader);

        let bytes = cache.get_or_read(&f.path).expect("read");

        assert_eq!(&*bytes, &f.content[..]);
    }

    #[test]
    fn get_or_read_early_eof_throws_end_of_stream_and_is_not_cached() {
        // C#: SourceBytesCacheMutationTests.GetOrRead_EarlyEof_ThrowsEndOfStreamAndIsNotCached
        let root = TempRoot::new("src-bytes-mut-eof");
        let f = fixture(&root, "eof.bin", 100);
        let content = f.content.clone();
        let reader = Arc::new(CountingReader::new(move |_, _| {
            boxed(ChunkedStream::new(content.clone(), 10).ending_after(40))
        }));
        let cache = new_cache_with(1024 * 1024, reader);

        let err = cache
            .get_or_read(&f.path)
            .expect_err("a source that ends early must fail");

        assert_eq!(err.io_kind(), Some(io::ErrorKind::UnexpectedEof));
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn get_or_read_range_empty_range_at_end_returns_empty_without_reading() {
        // C#: SourceBytesCacheMutationTests.GetOrReadRange_EmptyRangeAtEnd_ReturnsEmptyWithoutReading
        let root = TempRoot::new("src-bytes-mut-empty-range");
        let f = fixture(&root, "end.bin", 4096);
        let reader = Arc::new(CountingReader::must_not_open());
        let cache = new_cache_with(1024 * 1024, reader.clone());

        assert!(
            read_range(&cache, &f, f.length, 0)
                .expect("empty range at end")
                .is_empty()
        );
        assert!(read_range(&cache, &f, 0, 0).expect("empty range at start").is_empty());

        assert_eq!(reader.opens(), 0);
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn get_or_read_range_range_ending_exactly_at_end_is_valid() {
        // C#: SourceBytesCacheMutationTests.GetOrReadRange_RangeEndingExactlyAtEnd_IsValid
        let root = TempRoot::new("src-bytes-mut-tail");
        let f = fixture(&root, "tail.bin", 4096);
        let cache = new_cache(1024 * 1024);

        let tail = read_range(&cache, &f, f.length - 16, 16).expect("tail range");

        let n = f.content.len();
        assert_eq!(&*tail, &f.content[n - 16..]);
    }

    #[test]
    fn get_or_read_range_invalid_range_throws() {
        // C#: SourceBytesCacheMutationTests.GetOrReadRange_InvalidRange_Throws (6 theory rows)
        let root = TempRoot::new("src-bytes-mut-invalid");
        let f = fixture(&root, "bad.bin", 4096);
        // (offset, count): negative offset, negative count, past the end, longer than the file, runs past the end,
        // starts exactly at the end with a byte requested.
        let rows: [(i64, i32); 6] = [(-1, 5), (0, -1), (4097, 0), (0, 4097), (4090, 16), (4096, 1)];

        for (offset, count) in rows {
            let reader = Arc::new(CountingReader::must_not_open());
            let cache = new_cache_with(1024 * 1024, reader.clone());

            let result = read_range(&cache, &f, offset, count);

            assert!(
                matches!(
                    result,
                    Err(SourceBytesError::ArgumentOutOfRange { param_name: "offset" })
                ),
                "row offset={offset} count={count}"
            );
            assert_eq!(
                reader.opens(),
                0,
                "row offset={offset} count={count}: nothing is opened"
            );
        }
    }

    #[test]
    fn get_or_read_range_count_at_array_max_length_is_not_rejected_by_validation() {
        // C#: SourceBytesCacheMutationTests.GetOrReadRange_CountAtArrayMaxLength_IsNotRejectedByValidation
        // The fake source reports length 1, so the read fails with the file-changed error BEFORE any 2 GB allocation.
        let root = TempRoot::new("src-bytes-mut-max-length");
        let reader = Arc::new(CountingReader::new(|_, _| boxed(ChunkedStream::new(vec![1], 1))));
        let cache = new_cache_with(1024 * 1024, reader.clone());
        let path = root.join("never-read.bin");
        let max_count = i32::try_from(ARRAY_MAX_LENGTH).expect("Array.MaxLength fits i32");

        let err = cache
            .get_or_read_range(&path, ARRAY_MAX_LENGTH, 1, 0, max_count, VIEWER)
            .expect_err("a source of length 1 cannot supply Array.MaxLength bytes");
        assert_eq!(err.io_kind(), Some(io::ErrorKind::Other));
        assert_eq!(reader.opens(), 1);

        let over = i32::try_from(ARRAY_MAX_LENGTH + 1).expect("Array.MaxLength + 1 fits i32");
        let err = cache
            .get_or_read_range(&path, ARRAY_MAX_LENGTH + 1, 1, 0, over, VIEWER)
            .expect_err("one more than Array.MaxLength is refused");
        assert!(matches!(
            err,
            SourceBytesError::ArgumentOutOfRange { param_name: "count" }
        ));
        assert_eq!(reader.opens(), 1, "the refused count opens nothing");
    }

    #[test]
    fn try_get_range_offset_zero_is_a_hit() {
        // C#: SourceBytesCacheMutationTests.TryGetRange_OffsetZero_IsAHit
        let root = TempRoot::new("src-bytes-mut-zero");
        let f = fixture(&root, "zero.bin", 4096);
        let cache = new_cache(1024 * 1024);
        let read = read_range(&cache, &f, 0, 128).expect("read");

        let hit = cache
            .try_get_range(&f.path, f.length, f.ticks, 0, 128)
            .expect("no error")
            .expect("offset 0 is a hit");

        assert!(Arc::ptr_eq(&read, &hit));
    }

    #[test]
    fn can_cache_capacity_boundary() {
        // C#: SourceBytesCacheMutationTests.CanCache_CapacityBoundary
        let cache = new_cache(1024);
        let capacity = cache.capacity_bytes();

        assert!(cache.can_cache(capacity));
        assert!(!cache.can_cache(capacity + 1));
        assert!(cache.can_cache_range(0));
        assert!(cache.can_cache_range(capacity));
        assert!(!cache.can_cache_range(capacity + 1));
        assert!(!cache.can_cache_range(-1));
        assert!(!cache.can_cache(i64::MAX));
    }

    #[test]
    fn can_cache_array_max_length_boundary() {
        // C#: SourceBytesCacheMutationTests.CanCache_ArrayMaxLengthBoundary
        // 64 GiB of RAM: the clamp allows 12.8 GiB, above Array.MaxLength, so the `capacity > ARRAY_MAX_LENGTH` branch
        // is always taken. The C# test skips the boundary when the machine is too small; this one never skips.
        let big = SourceBytesCache::new(i64::MAX, PHYSICAL_64_GIB, None).expect("cache");
        assert!(big.capacity_bytes() > ARRAY_MAX_LENGTH);
        assert!(big.can_cache(ARRAY_MAX_LENGTH));
        assert!(big.can_cache_range(ARRAY_MAX_LENGTH));
        assert!(!big.can_cache(ARRAY_MAX_LENGTH + 1));
        assert!(!big.can_cache_range(ARRAY_MAX_LENGTH + 1));

        // 8 GiB of RAM: the clamp gives 1.6 GiB, below Array.MaxLength, so the clamped branch is taken.
        let small = SourceBytesCache::new(i64::MAX, PHYSICAL_8_GIB, None).expect("cache");
        assert!(small.capacity_bytes() < ARRAY_MAX_LENGTH);
        assert!(!small.can_cache(ARRAY_MAX_LENGTH));
        assert!(!small.can_cache_range(ARRAY_MAX_LENGTH));
    }

    #[test]
    fn evict_twice_during_read_does_not_republish() {
        // C#: SourceBytesCacheMutationTests.Evict_TwiceDuringRead_DoesNotRepublish
        let root = TempRoot::new("src-bytes-mut-twice");
        let f = fixture(&root, "twice.bin", 256);
        let cache = Arc::new(new_cache(1024 * 1024));
        // Runs on the reading thread after the bytes are read and before the publish check. It holds a Weak handle
        // so the hook does not form an Arc cycle with the cache.
        let weak: Weak<SourceBytesCache> = Arc::downgrade(&cache);
        let path = f.path.clone();
        cache.set_after_read_for_tests(Some(hook(move || {
            if let Some(cache) = weak.upgrade() {
                cache.set_after_read_for_tests(None);
                cache.evict(&path).expect("first evict during the read");
                cache.evict(&path).expect("second evict during the read");
            }
        })));

        let bytes = cache.get_or_read(&f.path).expect("read");

        assert_eq!(&*bytes, &f.content[..]);
        assert_eq!(cache.count(), 0, "evicted twice mid-read: not republished");
        assert_eq!(&*cache.get_or_read(&f.path).expect("later read"), &f.content[..]);
        assert_eq!(cache.count(), 1, "a later read caches normally");
    }
}

// ---- SourceBytesCacheRobustnessTests ----

mod source_bytes_cache_robustness {
    use super::*;

    #[test]
    fn zero_byte_file_is_empty_array() {
        // C#: SourceBytesCacheRobustnessTests.ZeroByteFile_IsEmptyArray
        let root = TempRoot::new("src-bytes-zero-byte");
        let path = write_file(&root, "empty.jpg", &[]);
        let cache = new_cache(1024);

        let first = cache.get_or_read(&path).expect("first read");
        let second = cache.get_or_read(&path).expect("second read");

        assert!(first.is_empty());
        assert!(Arc::ptr_eq(&first, &second), "served again from the cache");
        assert_eq!(cache.current_size(), 1, "a zero-byte entry costs 1");
    }

    #[test]
    fn failed_read_is_not_cached() {
        // C#: SourceBytesCacheRobustnessTests.FailedRead_IsNotCached
        let root = TempRoot::new("src-bytes-failed-read");
        let path = root.join("late.jpg");
        let cache = new_cache(1024);

        let err = cache.get_or_read(&path).expect_err("a missing file fails");
        assert_eq!(err.io_kind(), Some(io::ErrorKind::NotFound));
        assert_eq!(cache.count(), 0);

        fs::write(&path, [7u8, 8, 9]).expect("create the file");
        assert_eq!(
            &*cache.get_or_read(&path).expect("read once the file exists"),
            &[7u8, 8, 9]
        );
    }

    /// C#: `SourceBytesCacheRobustnessTests.EvictBetweenCheckAndSet_DoesNotRepublish` with `clear`.
    fn evict_between_check_and_set_does_not_republish(clear: bool) {
        let root = TempRoot::new("src-bytes-race-publish");
        let path = write_file(&root, "racing.bin", &[0u8; 64]);
        let cache = Arc::new(new_cache(1024 * 1024));
        let (paused_tx, paused_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Mutex::new(release_rx);
        // Runs inside the publish gate, between the version check and the set: the reader is parked there.
        cache.set_before_publish_for_tests(Some(hook(move || {
            let _ = paused_tx.send(());
            if let Ok(rx) = release_rx.lock() {
                let _ = rx.recv_timeout(RACE_TIMEOUT);
            }
        })));
        let (reader_tx, reader_rx) = mpsc::channel();
        let reader_cache = Arc::clone(&cache);
        let reader_path = path.clone();
        let _reader = thread::spawn(move || {
            let _ = reader_tx.send(reader_cache.get_or_read(&reader_path).is_ok());
        });

        let paused = paused_rx.recv_timeout(RACE_TIMEOUT).is_ok();
        cache.set_before_publish_for_tests(None);
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (evictor_done_tx, evictor_done_rx) = mpsc::channel::<()>();
        let evictor_cache = Arc::clone(&cache);
        let evictor_path = path.clone();
        let _evictor = thread::spawn(move || {
            let _ = started_tx.send(());
            if clear {
                evictor_cache.clear();
            } else {
                let _ = evictor_cache.evict(&evictor_path);
            }
            let _ = evictor_done_tx.send(());
        });
        // The evictor signals before it calls evict. The reader stays parked until the release, so the eviction either
        // waits on the publish gate or runs right after the reader publishes: both leave the cache empty.
        let evictor_started = started_rx.recv_timeout(RACE_TIMEOUT).is_ok();
        let _ = release_tx.send(());
        let reader_ok = reader_rx.recv_timeout(RACE_TIMEOUT);
        let evictor_done = evictor_done_rx.recv_timeout(RACE_TIMEOUT).is_ok();

        assert!(paused, "reader never reached the publish gate (paused.Wait timed out)");
        assert!(evictor_started, "evictor thread never started");
        assert!(matches!(reader_ok, Ok(true)), "reader did not finish after release");
        assert!(evictor_done, "evictor thread did not finish after release");
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn evict_between_check_and_set_does_not_republish_evict() {
        // C#: SourceBytesCacheRobustnessTests.EvictBetweenCheckAndSet_DoesNotRepublish (theory false)
        evict_between_check_and_set_does_not_republish(false);
    }

    #[test]
    fn evict_between_check_and_set_does_not_republish_clear() {
        // C#: SourceBytesCacheRobustnessTests.EvictBetweenCheckAndSet_DoesNotRepublish (theory true)
        evict_between_check_and_set_does_not_republish(true);
    }

    #[test]
    fn clear_prunes_path_versions_and_still_invalidates_in_flight_read() {
        // C#: SourceBytesCacheRobustnessTests.Clear_PrunesPathVersions_AndStillInvalidatesInFlightRead
        let root = TempRoot::new("src-bytes-clear-versions");
        let paths: Vec<PathBuf> = (0..3)
            .map(|i| write_file(&root, &format!("evicted{i}.bin"), &[0u8; 16]))
            .collect();
        let cache = Arc::new(new_cache(1024 * 1024));
        for path in &paths {
            cache.evict(path).expect("evict");
        }
        assert_eq!(cache.path_version_count_for_tests(), 3);

        let racing = write_file(&root, "inflight.bin", &[0u8; 32]);
        let (read_done_tx, read_done_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Mutex::new(release_rx);
        cache.set_after_read_for_tests(Some(hook(move || {
            let _ = read_done_tx.send(());
            if let Ok(rx) = release_rx.lock() {
                let _ = rx.recv_timeout(RACE_TIMEOUT);
            }
        })));
        let (result_tx, result_rx) = mpsc::channel();
        let reader_cache = Arc::clone(&cache);
        let reader_path = racing.clone();
        let _reader = thread::spawn(move || {
            let _ = result_tx.send(reader_cache.get_or_read(&reader_path).is_ok());
        });

        let read_finished = read_done_rx.recv_timeout(RACE_TIMEOUT).is_ok();
        if read_finished {
            cache.clear();
        }
        cache.set_after_read_for_tests(None);
        let _ = release_tx.send(());
        let reader_ok = result_rx.recv_timeout(RACE_TIMEOUT);

        assert!(read_finished, "reader never finished its read");
        assert!(matches!(reader_ok, Ok(true)), "reader did not finish after release");
        assert_eq!(cache.path_version_count_for_tests(), 0);
        assert_eq!(cache.count(), 0, "the read that started before Clear is not cached");
        assert_eq!(cache.get_or_read(&paths[0]).expect("evicted path").len(), 16);
        assert_eq!(cache.count(), 1, "evicted paths are cacheable again after Clear");
    }

    #[test]
    fn oversized_file_is_returned_but_not_cached() {
        // C#: SourceBytesCacheRobustnessTests.OversizedFile_IsReturnedButNotCached
        let root = TempRoot::new("src-bytes-oversized-file");
        let small = write_file(&root, "small.bin", &[0u8; 100]);
        let big = write_file(&root, "big.bin", &[0u8; 5000]);
        let cache = new_cache(1000);
        let small_bytes = cache.get_or_read(&small).expect("small file");

        let big_bytes = cache.get_or_read(&big).expect("oversized file is still returned");

        assert_eq!(big_bytes.len(), 5000);
        assert_eq!(cache.count(), 1);
        assert!(Arc::ptr_eq(
            &small_bytes,
            &cache.get_or_read(&small).expect("small file again")
        ));
    }

    #[test]
    fn replaced_file_is_re_read() {
        // C#: SourceBytesCacheRobustnessTests.ReplacedFile_IsReRead
        let root = TempRoot::new("src-bytes-replaced");
        let path = write_file(&root, "changing.bin", &[1, 2, 3]);
        let cache = new_cache(1024);
        assert_eq!(&*cache.get_or_read(&path).expect("first read"), &[1u8, 2, 3]);

        // Same length: only the timestamp tells the two versions apart.
        fs::write(&path, [9u8, 8, 7]).expect("rewrite");
        move_mtime(&path, 5).expect("move the write time five minutes ahead");

        assert_eq!(&*cache.get_or_read(&path).expect("second read"), &[9u8, 8, 7]);
    }

    #[test]
    fn concurrent_readers_of_one_path_share_one_read() {
        // C#: SourceBytesCacheRobustnessTests.ConcurrentReadersOfOnePath_ShareOneRead
        let root = TempRoot::new("src-bytes-concurrent");
        let content: Vec<u8> = (0..4096usize).map(|i| i as u8).collect();
        let path = write_file(&root, "shared.bin", &content);
        let cache = Arc::new(new_cache(1024 * 1024));
        let barrier = Arc::new(Barrier::new(16));
        let (sent, received) = mpsc::channel::<Result<SourceBytes, SourceBytesError>>();
        for _ in 0..16 {
            let cache = Arc::clone(&cache);
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            let sent = sent.clone();
            let _ = thread::spawn(move || {
                barrier.wait();
                let _ = sent.send(cache.get_or_read(&path));
            });
        }
        drop(sent);

        // Bounded collection: a reader that never returns fails the test after 10 s.
        let results: Vec<SourceBytes> = (0..16)
            .map(|_| {
                received
                    .recv_timeout(RACE_TIMEOUT)
                    .expect("reader finished within 10 s")
                    .expect("read")
            })
            .collect();

        assert!(
            results.iter().all(|r| Arc::ptr_eq(r, &results[0])),
            "all readers share one array"
        );
        assert_eq!(cache.count(), 1);
    }

    #[test]
    fn late_reader_after_publish_shares_array_without_second_read() {
        // C#: SourceBytesCacheRobustnessTests.LateReaderAfterPublish_SharesArrayWithoutSecondRead
        let root = TempRoot::new("src-bytes-late-reader");
        let content: Vec<u8> = (0..4096usize).map(|i| i as u8).collect();
        let path = write_file(&root, "late-race.bin", &content);
        let reader = physical_reader();
        let cache = Arc::new(new_cache_with(1024 * 1024, reader.clone()));

        // The late reader records its thread id, so the hook can park only that thread.
        let late_thread: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
        let (missed_tx, missed_rx) = mpsc::channel::<()>();
        let (winner_done_tx, winner_done_rx) = mpsc::channel::<()>();
        let winner_done_rx = Mutex::new(winner_done_rx);
        let late_id = Arc::clone(&late_thread);
        cache.set_after_cache_miss_for_tests(Some(hook(move || {
            let is_late = late_id.lock().ok().and_then(|slot| *slot) == Some(thread::current().id());
            if !is_late {
                return;
            }
            let _ = missed_tx.send(());
            if let Ok(rx) = winner_done_rx.lock() {
                let _ = rx.recv_timeout(RACE_TIMEOUT);
            }
        })));

        let (late_tx, late_rx) = mpsc::channel();
        let late_cache = Arc::clone(&cache);
        let late_path = path.clone();
        let late_slot = Arc::clone(&late_thread);
        let _late = thread::spawn(move || {
            if let Ok(mut slot) = late_slot.lock() {
                *slot = Some(thread::current().id());
            }
            let _ = late_tx.send(late_cache.get_or_read(&late_path));
        });
        let missed = missed_rx.recv_timeout(RACE_TIMEOUT).is_ok();

        // The winner reads, publishes and removes its in-flight entry while the late reader is parked after its miss.
        let winner = cache.get_or_read(&path);
        let _ = winner_done_tx.send(());
        let late = late_rx.recv_timeout(RACE_TIMEOUT);
        cache.set_after_cache_miss_for_tests(None);

        assert!(missed, "late reader never reached the miss point");
        let winner = winner.expect("winner read");
        let late = late.expect("late reader finished within 10 s").expect("late read");
        assert!(Arc::ptr_eq(&winner, &late), "the late reader shares the winner's array");
        assert_eq!(reader.opens(), 1, "one open for both readers");
    }

    #[test]
    fn readers_racing_evict_and_clear_stay_correct_and_bounded() {
        // C#: SourceBytesCacheRobustnessTests.ReadersRacingEvictAndClear_StayCorrectAndBounded
        const FILES: usize = 6;
        const FILE_LEN: usize = 2048;
        const READERS: u64 = 4;
        const ITERATIONS: usize = 800;
        let root = TempRoot::new("src-bytes-race-evict");
        let paths: Vec<PathBuf> = (0..FILES)
            .map(|i| write_file(&root, &format!("f{i}.bin"), &vec![(i + 1) as u8; FILE_LEN]))
            .collect();
        let capacity: i64 = 3 * 2048;
        let cache = Arc::new(new_cache(capacity));
        let barrier = Arc::new(Barrier::new(READERS as usize + 1));
        let stop = Arc::new(AtomicBool::new(false));
        let (failures_tx, failures_rx) = mpsc::channel::<Vec<String>>();
        let (evictor_done_tx, evictor_done_rx) = mpsc::channel::<()>();

        for worker in 0..READERS {
            let cache = Arc::clone(&cache);
            let barrier = Arc::clone(&barrier);
            let paths = paths.clone();
            let failures_tx = failures_tx.clone();
            let _ = thread::spawn(move || {
                let mut rng = XorShift::new(worker);
                let mut failures = Vec::new();
                barrier.wait();
                for _ in 0..ITERATIONS {
                    let index = rng.below(FILES as u64) as usize;
                    let expected = (index + 1) as u8;
                    match cache.get_or_read(&paths[index]) {
                        Ok(bytes) => {
                            if bytes.len() != FILE_LEN || bytes[0] != expected || bytes[FILE_LEN - 1] != expected {
                                failures.push(format!("file {index}: wrong bytes"));
                            }
                        }
                        Err(err) => failures.push(format!("file {index}: {err}")),
                    }
                    if cache.current_size() > cache.capacity_bytes() {
                        failures.push(format!(
                            "size {} > capacity {}",
                            cache.current_size(),
                            cache.capacity_bytes()
                        ));
                    }
                }
                let _ = failures_tx.send(failures);
            });
        }
        drop(failures_tx);
        {
            let cache = Arc::clone(&cache);
            let barrier = Arc::clone(&barrier);
            let stop = Arc::clone(&stop);
            let paths = paths.clone();
            let _ = thread::spawn(move || {
                let mut rng = XorShift::new(99);
                barrier.wait();
                while !stop.load(Ordering::SeqCst) {
                    if rng.below(8) == 0 {
                        cache.clear();
                    } else {
                        let index = rng.below(FILES as u64) as usize;
                        let _ = cache.evict(&paths[index]);
                    }
                }
                let _ = evictor_done_tx.send(());
            });
        }

        // Collect every reader result within the bound, then stop the evictor even if a reader failed.
        let collected: Vec<Result<Vec<String>, mpsc::RecvTimeoutError>> =
            (0..READERS).map(|_| failures_rx.recv_timeout(RACE_TIMEOUT)).collect();
        stop.store(true, Ordering::SeqCst);
        let evictor_done = evictor_done_rx.recv_timeout(RACE_TIMEOUT).is_ok();

        let mut failures = Vec::new();
        for result in collected {
            failures.extend(result.expect("reader finished within 10 s"));
        }
        assert!(failures.is_empty(), "{failures:?}");
        assert!(evictor_done, "evictor did not stop");
        assert!(cache.current_size() <= cache.capacity_bytes());
    }
}

// ---- SourceBytesCacheRangePropertyTests ----

mod source_bytes_cache_range_property {
    use super::*;

    const FILE_LENGTH: i64 = 4096;
    const CAPACITY: i64 = 1500;

    #[test]
    fn random_ranges_return_exact_slices_and_respect_capacity() {
        // C#: SourceBytesCacheRangePropertyTests.RandomRanges_ReturnExactSlices_AndRespectCapacity
        // PropertyRunner: 15 iterations, then 2 fixed seeds, 80 steps each. Seeds are xorshift seeds, not `Random`.
        let root = TempRoot::new("src-bytes-property");
        let content = random_bytes(99, FILE_LENGTH as usize);
        let path = write_file(&root, "source.bin", &content);
        let ticks = file_ticks(&path);
        let seeds: Vec<u64> = (1000..1015).chain([1, 2]).collect();

        for seed in seeds {
            let mut rng = XorShift::new(seed);
            let cache = new_cache(CAPACITY);
            assert_eq!(
                cache.capacity_bytes(),
                CAPACITY,
                "not clamped on any machine this suite runs on"
            );

            for step in 0..80 {
                let offset = rng.range(0, FILE_LENGTH + 1);
                let remaining = FILE_LENGTH - offset;
                let count = if rng.below(5) == 0 {
                    rng.range(0, remaining + 1)
                } else {
                    rng.range(0, remaining.min(400) + 1)
                };
                let expected = &content[offset as usize..(offset + count) as usize];
                let count32 = i32::try_from(count).expect("count fits i32");

                match rng.below(8) {
                    0..=4 => {
                        let bytes = cache
                            .get_or_read_range(&path, FILE_LENGTH, ticks, offset, count32, VIEWER)
                            .expect("an in-file range reads");
                        assert_eq!(&*bytes, expected, "seed {seed} step {step}: slice");
                        if count > 0 {
                            let hit = cache
                                .try_get_range(&path, FILE_LENGTH, ticks, offset, count32)
                                .expect("no error");
                            assert_eq!(
                                hit.is_some(),
                                cache.can_cache_range(count),
                                "seed {seed} step {step}: cached iff cacheable"
                            );
                            if let Some(again) = hit {
                                assert_eq!(&*again, expected, "seed {seed} step {step}: cached slice");
                            }
                        }
                    }
                    5 => {
                        if let Some(hit) = cache
                            .try_get_range(&path, FILE_LENGTH, ticks, offset, count32)
                            .expect("no error")
                        {
                            assert_eq!(&*hit, expected, "seed {seed} step {step}: hit slice");
                        }
                    }
                    6 => {
                        cache.evict(&path).expect("evict");
                        assert_eq!(cache.count(), 0);
                        assert_eq!(cache.current_size(), 0);
                        assert!(matches!(
                            cache.try_get_range(&path, FILE_LENGTH, ticks, offset, count32.max(1)),
                            Ok(None)
                        ));
                    }
                    _ => {
                        if rng.below(6) == 0 {
                            cache.clear();
                            assert_eq!(cache.count(), 0);
                        }
                    }
                }

                assert!(
                    cache.current_size() <= cache.capacity_bytes(),
                    "seed {seed} step {step}: size {} > capacity {}",
                    cache.current_size(),
                    cache.capacity_bytes()
                );
            }
        }
    }

    #[test]
    fn out_of_file_ranges_are_refused() {
        // C#: SourceBytesCacheRangePropertyTests.OutOfFileRanges_AreRefused (500 iterations)
        let root = TempRoot::new("src-bytes-property-refuse");
        let content = random_bytes(99, FILE_LENGTH as usize);
        let path = write_file(&root, "source.bin", &content);
        let ticks = file_ticks(&path);
        let cache = new_cache(CAPACITY);
        let mut rng = XorShift::new(7);

        for _ in 0..500 {
            let (offset, count) = match rng.below(3) {
                0 => (rng.range(FILE_LENGTH + 1, FILE_LENGTH + 100), rng.range(0, 10)),
                1 => (
                    rng.range(0, FILE_LENGTH + 1),
                    rng.range(FILE_LENGTH + 1, FILE_LENGTH + 100),
                ),
                _ => (rng.range(-50, 0), rng.range(0, 10)),
            };
            if offset >= 0 && count <= FILE_LENGTH - offset {
                continue; // a valid combination: nothing to refuse
            }
            let count32 = i32::try_from(count).expect("count fits i32");

            let result = cache.get_or_read_range(&path, FILE_LENGTH, ticks, offset, count32, VIEWER);

            assert!(
                matches!(result, Err(SourceBytesError::ArgumentOutOfRange { .. })),
                "offset {offset} count {count} must be refused"
            );
            assert_eq!(cache.count(), 0);
        }
    }
}

// ---- MemoryBudgetClampTests (SourceBytesCache row only) ----

mod memory_budget_clamp {
    use super::*;

    #[test]
    fn source_bytes_cache_huge_request_is_clamped_to_half_of_physical() {
        // C#: MemoryBudgetClampTests.SourceBytesCache_HugeRequest_IsClampedToHalfOfPhysical
        // C# reads the real machine's RAM; here the physical size is fixed at 32 GiB.
        let physical = PHYSICAL_32_GIB;
        let requested = i64::MAX / 2;

        let cache = SourceBytesCache::new(requested, physical, None).expect("cache");

        assert_eq!(
            cache.capacity_bytes(),
            clamp_source_bytes_to_physical_memory(requested, physical)
        );
        assert!(cache.capacity_bytes() <= physical / 2);

        // Unknown physical memory (0) means the request is not clamped.
        let unknown = SourceBytesCache::new(requested, 0, None).expect("cache");
        assert_eq!(unknown.capacity_bytes(), requested);
    }
}

// ---- ErrorHandlingReviewCachingTests (SourceBytesCache test only) ----

mod error_handling_review_caching {
    use super::*;

    #[test]
    fn evict_bounds_path_versions_and_keeps_invalidation() {
        // C#: ErrorHandlingReviewCachingTests.Evict_BoundsPathVersions_AndKeepsInvalidation
        let root = TempRoot::new("errh-src-bytes");
        let cache = Arc::new(new_cache(1024 * 1024));
        for i in 0..MAX_TRACKED_PATH_VERSIONS {
            cache.evict(&root.join(format!("gone{i}.bin"))).expect("evict");
        }
        assert_eq!(cache.path_version_count_for_tests(), MAX_TRACKED_PATH_VERSIONS);

        // The read has loaded the bytes and is about to publish when the path is evicted (the map is full: global bump).
        let racing = write_file(&root, "racing.bin", &[0u8; 32]);
        let weak: Weak<SourceBytesCache> = Arc::downgrade(&cache);
        let racing_for_hook = racing.clone();
        cache.set_after_read_for_tests(Some(hook(move || {
            if let Some(cache) = weak.upgrade() {
                let _ = cache.evict(&racing_for_hook);
            }
        })));
        assert_eq!(cache.get_or_read(&racing).expect("read").len(), 32);

        assert_eq!(cache.count(), 0, "evicted while in flight: not cached");
        assert!(cache.path_version_count_for_tests() <= MAX_TRACKED_PATH_VERSIONS);

        for i in 0..50 {
            cache.evict(&root.join(format!("more{i}.bin"))).expect("evict");
        }
        assert!(cache.path_version_count_for_tests() <= MAX_TRACKED_PATH_VERSIONS);

        cache.set_after_read_for_tests(None);
        assert_eq!(cache.get_or_read(&racing).expect("read again").len(), 32);
        assert_eq!(cache.count(), 1, "cacheable again afterwards");
    }
}

// ---- DecoderErrorLocalizationTests (SourceBytesCache test, English half) ----

mod decoder_error_localization {
    use super::*;

    #[test]
    fn source_bytes_cache_file_changed_while_reading_is_localized() {
        // C#: DecoderErrorLocalizationTests.SourceBytesCache_FileChangedWhileReading_IsLocalized
        // SKIPPED PART (D41 / P-24): the Vietnamese text and the `IsLocalized` flag need the injected user-text seam,
        // which pr-imaging does not have yet. This test checks the English message and the kind of the inner error.
        let root = TempRoot::new("src-bytes-localized");
        let path = write_file(&root, "big.bin", &[0u8; 1024]);
        let touched = path.clone();
        let cache = SourceBytesCache::new(256 * 1024 * 1024, PHYSICAL_64_GIB, None).expect("cache");
        // Moves the mtime one minute forward between the end of the read and the stat re-check.
        cache.set_after_read_for_tests(Some(hook(move || {
            let _ = move_mtime(&touched, 1);
        })));

        let err = cache
            .get_or_read(&path)
            .expect_err("a file changed while it was read must fail");

        let SourceBytesError::Io(inner) = &err else {
            panic!("expected SourceBytesError::Io, got {err:?}");
        };
        assert_eq!(inner.kind(), io::ErrorKind::Other);
        let text = inner.to_string();
        assert!(text.starts_with("File changed while reading: "), "{text}");
        assert!(text.ends_with("big.bin"), "the message names the file: {text}");
        assert_eq!(err.to_string(), text, "the SourceBytesError Display is transparent");
    }
}

// ---- golden runner: tests/golden/source-bytes-scenarios.json ----

/// One step's returned value, compared with the golden entry of the same index.
enum Outcome {
    /// `read`, `read_priority`, `read_range`.
    Bytes(SourceBytes),
    /// `try_get_range`: `Ok(Some)` is a hit, `Ok(None)` a miss.
    Hit(Option<SourceBytes>),
    /// `prefetch`, `can_cache`, `can_cache_range`.
    Value(bool),
    /// `evict`, `clear`, `write`, `delete`.
    Done,
}

fn field_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn fnv1a64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The oracle's fixture bytes: byte `i` is `(i + seed) % 251`, or the constant `fill` when given.
fn case_content(spec: &Value) -> Vec<u8> {
    let size = field_i64(spec, "size").expect("size") as usize;
    let seed = field_i64(spec, "seed").unwrap_or(0) as usize;
    let fill = field_i64(spec, "fill").map(|f| f as u8);
    (0..size).map(|i| fill.unwrap_or(((i + seed) % 251) as u8)).collect()
}

fn step_path(root: &TempRoot, step: &Value) -> Option<PathBuf> {
    if let Some(raw) = step.get("raw_path").and_then(Value::as_str) {
        return Some(PathBuf::from(raw));
    }
    step.get("file").and_then(Value::as_str).map(|file| root.join(file))
}

/// `(length, ticks)` of a range step: explicit values, or the file's own identity plus `ticks_delta`.
fn range_identity(step: &Value, path: &Path) -> (i64, i64) {
    let length = field_i64(step, "length").unwrap_or_else(|| file_len(path));
    let base = field_i64(step, "ticks").unwrap_or_else(|| file_ticks(path));
    (length, base + field_i64(step, "ticks_delta").unwrap_or(0))
}

fn priority_of(name: Option<&str>) -> SourceReadPriority {
    match name {
        Some("Preload") => SourceReadPriority::Preload,
        _ => SourceReadPriority::Viewer,
    }
}

/// A reader for a case with a `reader` spec: `serve` is `file`, `fill` or `bytes`, with optional `chunk`,
/// `length_override`, `eof_after`, or `throw_on_open`.
fn scripted_reader(spec: &Value) -> CountingReader {
    if spec.get("throw_on_open").and_then(Value::as_bool) == Some(true) {
        return CountingReader::must_not_open();
    }
    let serve = spec.get("serve").and_then(Value::as_str).unwrap_or("file").to_owned();
    let chunk = field_i64(spec, "chunk").unwrap_or(1024 * 1024) as usize;
    let fill = field_i64(spec, "fill").map(|f| f as u8);
    let bytes: Option<Vec<u8>> = spec
        .get("bytes")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(|n| n.as_u64().unwrap_or(0) as u8).collect());
    let length_override = field_i64(spec, "length_override");
    let eof_after = field_i64(spec, "eof_after").map(|n| n as usize);
    CountingReader::new(move |path, _priority| {
        let data = match serve.as_str() {
            "file" => fs::read(path)?,
            "fill" => vec![fill.unwrap_or(0); file_len(path) as usize],
            "bytes" => bytes.clone().unwrap_or_default(),
            other => return Err(io::Error::other(format!("unknown serve {other}"))),
        };
        let mut stream = ChunkedStream::new(data, chunk);
        stream.length_override = length_override;
        stream.eof_after = eof_after;
        boxed(stream)
    })
}

fn run_step(cache: &SourceBytesCache, root: &TempRoot, step: &Value) -> Result<Outcome, SourceBytesError> {
    let op = golden::str_of(step, "op");
    match op {
        "write" => {
            let path = root.join(golden::str_of(step, "file"));
            let previous = fs::metadata(&path).ok().and_then(|meta| meta.modified().ok());
            fs::write(&path, case_content(step)).expect("write case file");
            if let Some(minutes) = field_i64(step, "add_minutes") {
                // Moves the new write time relative to the previous version's time (deterministic, D19/D27).
                let base = previous.unwrap_or_else(|| {
                    fs::metadata(&path)
                        .and_then(|meta| meta.modified())
                        .expect("stat the rewritten file")
                });
                let file = OpenOptions::new().write(true).open(&path).expect("open case file");
                file.set_modified(shift(base, minutes)).expect("set write time");
            }
            Ok(Outcome::Done)
        }
        "delete" => {
            fs::remove_file(root.join(golden::str_of(step, "file"))).expect("delete case file");
            Ok(Outcome::Done)
        }
        "clear" => {
            cache.clear();
            Ok(Outcome::Done)
        }
        "can_cache" => Ok(Outcome::Value(cache.can_cache(golden::i64_of(step, "value")))),
        "can_cache_range" => Ok(Outcome::Value(cache.can_cache_range(golden::i64_of(step, "value")))),
        _ => {
            let path = step_path(root, step).unwrap_or_else(|| panic!("step {op} names no path"));
            match op {
                "read" => cache.get_or_read(&path).map(Outcome::Bytes),
                "read_priority" => cache
                    .get_or_read_with_priority(&path, priority_of(step.get("priority").and_then(Value::as_str)))
                    .map(Outcome::Bytes),
                "read_range" => {
                    let (length, ticks) = range_identity(step, &path);
                    let offset = field_i64(step, "offset").unwrap_or(0);
                    let count = i32::try_from(golden::i64_of(step, "count")).expect("count fits i32");
                    let priority = priority_of(step.get("priority").and_then(Value::as_str));
                    cache
                        .get_or_read_range(&path, length, ticks, offset, count, priority)
                        .map(Outcome::Bytes)
                }
                "try_get_range" => {
                    let (length, ticks) = range_identity(step, &path);
                    let offset = field_i64(step, "offset").unwrap_or(0);
                    let count = i32::try_from(golden::i64_of(step, "count")).expect("count fits i32");
                    cache
                        .try_get_range(&path, length, ticks, offset, count)
                        .map(Outcome::Hit)
                }
                "prefetch" => cache.try_prefetch(&path).map(Outcome::Value),
                "evict" => cache.evict(&path).map(|()| Outcome::Done),
                other => panic!("unknown golden op {other}"),
            }
        }
    }
}

/// `same_as` is the earliest earlier step that returned the same instance; `null` when none did. Empty arrays are
/// skipped (their identity is not part of the contract).
fn check_bytes(at: &str, bytes: &SourceBytes, want: &Value, returned: &[Option<SourceBytes>]) {
    assert_eq!(bytes.len() as i64, golden::i64_of(want, "len"), "{at}: len");
    assert_eq!(fnv1a64(bytes), golden::str_of(want, "fnv"), "{at}: fnv");
    if bytes.is_empty() {
        return;
    }
    let earliest = returned
        .iter()
        .position(|earlier| earlier.as_ref().is_some_and(|e| Arc::ptr_eq(e, bytes)));
    let expected = want.get("same_as").and_then(Value::as_u64).map(|i| i as usize);
    assert_eq!(earliest, expected, "{at}: same_as");
}

fn check_error(at: &str, err: &SourceBytesError, want: &Value) {
    let ty = golden::str_of(want, "type");
    let param = want.get("param").and_then(Value::as_str);
    // The golden `localized` flag is not checked: pr-imaging has no pr-l10n dependency (D41 / P-24).
    let same = match (ty, err) {
        ("ArgumentException", SourceBytesError::Argument { param_name }) => Some(*param_name) == param,
        ("ArgumentOutOfRangeException", SourceBytesError::ArgumentOutOfRange { param_name }) => {
            Some(*param_name) == param
        }
        ("FileNotFoundException", SourceBytesError::Io(_)) => err.io_kind() == Some(io::ErrorKind::NotFound),
        ("EndOfStreamException", SourceBytesError::Io(_)) => err.io_kind() == Some(io::ErrorKind::UnexpectedEof),
        ("IOException", SourceBytesError::Io(_)) => err.io_kind() == Some(io::ErrorKind::Other),
        _ => false,
    };
    assert!(same, "{at}: expected error {want}, got {err:?}");
}

fn check_step(
    name: &str,
    index: usize,
    cache: &SourceBytesCache,
    reader: &CountingReader,
    outcome: Result<Outcome, SourceBytesError>,
    want: &Value,
    returned: &mut Vec<Option<SourceBytes>>,
) {
    let at = format!("{name} step {index} ({})", golden::str_of(want, "op"));
    assert_eq!(cache.count() as i64, golden::i64_of(want, "count"), "{at}: count");
    assert_eq!(
        cache.current_size(),
        golden::i64_of(want, "current_size"),
        "{at}: current_size"
    );
    assert_eq!(reader.opens() as i64, golden::i64_of(want, "opens"), "{at}: opens");
    let seen: Vec<&str> = reader
        .priorities()
        .iter()
        .map(|p| match p {
            SourceReadPriority::Viewer => "Viewer",
            SourceReadPriority::Preload => "Preload",
        })
        .collect();
    let wanted: Vec<&str> = want
        .get("priorities")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert_eq!(seen, wanted, "{at}: priorities");

    match outcome {
        Err(err) => {
            let expected = want
                .get("error")
                .unwrap_or_else(|| panic!("{at}: unexpected error {err:?}"));
            check_error(&at, &err, expected);
            returned.push(None);
        }
        Ok(found) => {
            assert!(want.get("error").is_none(), "{at}: expected an error {}", want["error"]);
            match found {
                Outcome::Bytes(bytes) => {
                    check_bytes(&at, &bytes, &want["bytes"], returned);
                    returned.push(Some(bytes));
                }
                Outcome::Hit(Some(bytes)) => {
                    assert_eq!(want["value"].as_bool(), Some(true), "{at}: value");
                    check_bytes(&at, &bytes, &want["hit"], returned);
                    returned.push(Some(bytes));
                }
                Outcome::Hit(None) => {
                    assert_eq!(want["value"].as_bool(), Some(false), "{at}: value");
                    assert!(want["hit"].is_null(), "{at}: hit");
                    assert_eq!(want.get("miss_len").and_then(Value::as_i64), Some(0), "{at}: miss_len");
                    returned.push(None);
                }
                Outcome::Value(value) => {
                    assert_eq!(want["value"].as_bool(), Some(value), "{at}: value");
                    returned.push(None);
                }
                Outcome::Done => returned.push(None),
            }
        }
    }
}

mod golden_source_bytes_scenarios {
    use super::*;

    fn load_inputs() -> Vec<Value> {
        let path = golden::repo_root()
            .join("tests")
            .join("golden-inputs")
            .join("source-bytes-scenarios.json");
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        doc["cases"].as_array().cloned().unwrap_or_default()
    }

    fn run_case(input: &Value, want: &Value) {
        let name = golden::str_of(want, "name").to_owned();
        assert_eq!(
            golden::str_of(input, "name"),
            name,
            "inputs and goldens are in the same order"
        );
        let root = TempRoot::new("src-bytes-golden");
        for spec in input.get("files").and_then(Value::as_array).into_iter().flatten() {
            fs::write(root.join(golden::str_of(spec, "name")), case_content(spec)).expect("write case file");
        }
        for dir in input.get("dirs").and_then(Value::as_array).into_iter().flatten() {
            fs::create_dir_all(root.join(dir.as_str().unwrap_or_default())).expect("create case dir");
        }
        let reader: Arc<CountingReader> = Arc::new(match input.get("reader") {
            Some(spec) => scripted_reader(spec),
            None => CountingReader::physical(),
        });
        let capacity = field_i64(input, "capacity").expect("capacity");
        let cache = new_cache_with(capacity, reader.clone());
        assert_eq!(
            cache.capacity_bytes(),
            golden::i64_of(want, "capacity_bytes"),
            "{name}: capacity_bytes"
        );

        let steps_in: &[Value] = input
            .get("steps")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let steps_want: &[Value] = want
            .get("steps")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        assert_eq!(steps_in.len(), steps_want.len(), "{name}: step count");

        let mut returned: Vec<Option<SourceBytes>> = Vec::new();
        for (index, (step, expect)) in steps_in.iter().zip(steps_want).enumerate() {
            reader.reset();
            let outcome = run_step(&cache, &root, step);
            check_step(&name, index, &cache, &reader, outcome, expect, &mut returned);
        }
    }

    /// C# `oracle-cs source-bytes-scenarios` against `tests/golden/source-bytes-scenarios.json` (40 cases, 170 steps).
    #[test]
    fn source_bytes_scenarios_match_golden() {
        let expected = golden::load("source-bytes-scenarios");
        let inputs = load_inputs();
        assert_eq!(expected.cases.len(), 40, "golden case count");
        assert_eq!(inputs.len(), expected.cases.len(), "input and golden case counts");

        for (input, want) in inputs.iter().zip(&expected.cases) {
            run_case(input, want);
        }
    }
}

// ---- verifier probes (W4-10a repair round 1): Rust-only tests that kill the surviving mutants ----

mod verifier_probes {
    use super::*;
    use pr_imaging::cache::source_bytes::SourceBytesCachePolicy;
    use std::error::Error as _;
    use std::fmt;

    /// A capacity above `Array.MaxLength` (64 GiB of RAM clamps `i64::MAX / 2` to well over 2 GB).
    fn huge_cache(reader: Arc<dyn SourceReader>) -> SourceBytesCache {
        let cache = new_cache_with(i64::MAX / 2, reader);
        assert!(
            cache.can_cache_range(ARRAY_MAX_LENGTH),
            "the cache could keep an Array.MaxLength range"
        );
        cache
    }

    /// Fails its first `read` with `kind`, then serves the data (a retry would succeed).
    struct FailOnceStream {
        inner: ChunkedStream,
        kind: io::ErrorKind,
        failed: bool,
    }

    impl Read for FailOnceStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.failed {
                self.failed = true;
                return Err(io::Error::new(self.kind, "injected read failure"));
            }
            self.inner.read(buf)
        }
    }

    impl Seek for FailOnceStream {
        fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
            self.inner.seek(from)
        }
    }

    #[derive(Debug)]
    struct InnerError;

    impl fmt::Display for InnerError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("inner cause")
        }
    }

    impl std::error::Error for InnerError {}

    #[derive(Debug)]
    struct OuterError(InnerError);

    impl fmt::Display for OuterError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("outer failure")
        }
    }

    impl std::error::Error for OuterError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    /// Records the `buffer_size` the cache hands to the reader, then opens the real file.
    struct BufferSizeReader(Mutex<Vec<usize>>);

    impl SourceReader for BufferSizeReader {
        fn open_source(
            &self,
            path: &Path,
            priority: SourceReadPriority,
            buffer_size: usize,
        ) -> io::Result<Box<dyn ReadSeek>> {
            if let Ok(mut log) = self.0.lock() {
                log.push(buffer_size);
            }
            PhysicalSourceReader.open_source(path, priority, buffer_size)
        }
    }

    #[test]
    fn v_range_zero_length_source_is_valid() {
        // C# ThrowIfNegative(length): length 0 is valid; an empty range of a zero-byte file is [] without a read.
        let root = TempRoot::new("src-bytes-v-zero-length");
        let f = fixture(&root, "empty.bin", 0);
        let reader = Arc::new(CountingReader::must_not_open());
        let cache = new_cache_with(1024, reader.clone());

        let bytes = cache
            .get_or_read_range(&f.path, 0, f.ticks, 0, 0, VIEWER)
            .expect("an empty range of a zero-byte source is valid");

        assert!(bytes.is_empty());
        assert_eq!(reader.opens(), 0);
    }

    #[test]
    fn v_try_get_range_edges() {
        // C# TryGetRange returns false for an invalid range BEFORE Path.GetFullPath, so a NUL path is not an error.
        let cache = new_cache(1024);
        let bad = Path::new("C:\\dir\0name.bin");
        let invalid_rows: [(i64, i64, i32); 8] = [
            (10, 5, 6),              // count > length - offset
            (10, 0, 11),             // count > length
            (10, 11, 1),             // offset > length
            (10, -1, 1),             // negative offset
            (10, 0, 0),              // empty
            (10, 0, -1),             // negative count
            (i64::MIN, 1, 1),        // offset > length; length - offset would overflow
            (i64::MAX, i64::MAX, 1), // count > length - offset at the top of the range
        ];
        for (length, offset, count) in invalid_rows {
            let got = cache.try_get_range(bad, length, 0, offset, count);
            assert!(
                matches!(got, Ok(None)),
                "length={length} offset={offset} count={count}: invalid range is a miss, got {got:?}"
            );
        }

        // A valid range reaches GetFullPath, which rejects the NUL path.
        let valid_rows: [(i64, i64, i32); 3] = [(10, 5, 5), (10, 0, 10), (i64::MAX, 0, 1)];
        for (length, offset, count) in valid_rows {
            let got = cache.try_get_range(bad, length, 0, offset, count);
            assert!(
                matches!(got, Err(SourceBytesError::Argument { param_name: "path" })),
                "length={length} offset={offset} count={count}: a valid range with a NUL path is an argument error, \
                 got {got:?}"
            );
        }
    }

    #[test]
    fn v_range_count_exactly_array_max_reaches_reader() {
        // A cacheable range of exactly Array.MaxLength goes through the cache path and is opened; the fake reports
        // length 1, so the read fails with "file changed" before any 2 GB allocation.
        let root = TempRoot::new("src-bytes-v-array-max");
        let reader = Arc::new(CountingReader::new(|_, _| boxed(ChunkedStream::new(vec![1], 1))));
        let cache = huge_cache(reader.clone());
        let path = root.join("never-read.bin");
        let max_count = i32::try_from(ARRAY_MAX_LENGTH).expect("Array.MaxLength fits i32");

        let err = cache
            .get_or_read_range(&path, ARRAY_MAX_LENGTH, 1, 0, max_count, VIEWER)
            .expect_err("a source of length 1 cannot supply Array.MaxLength bytes");

        assert_eq!(err.io_kind(), Some(io::ErrorKind::Other), "got {err:?}");
        assert_eq!(reader.opens(), 1, "count == Array.MaxLength reaches the reader");
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn v_whole_file_above_array_max_is_key_error_before_open() {
        // A whole file above Array.MaxLength is ArgumentOutOfRange("key") before the reader is opened. Sparse file:
        // no disk space is used.
        let root = TempRoot::new("src-bytes-v-huge-file");
        let path = root.join("huge.bin");
        fs::write(&path, b"").expect("create sparse fixture");
        #[cfg(windows)]
        {
            let output = std::process::Command::new("fsutil")
                .args(["sparse", "setflag"])
                .arg(&path)
                .output()
                .expect("run fsutil");
            assert!(output.status.success(), "fsutil sparse setflag: {output:?}");
        }
        let huge = u64::try_from(ARRAY_MAX_LENGTH + 1).expect("positive");
        OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_len(huge))
            .expect("extend sparse fixture");

        let reader = Arc::new(CountingReader::must_not_open());
        let cache = huge_cache(reader.clone());

        let result = cache.get_or_read(&path);
        let removed = fs::remove_file(&path);

        assert!(
            matches!(result, Err(SourceBytesError::ArgumentOutOfRange { param_name: "key" })),
            "got {:?}",
            result.map(|bytes| bytes.len())
        );
        assert_eq!(reader.opens(), 0, "nothing is opened");
        removed.expect("remove sparse fixture");
    }

    #[test]
    fn v_read_error_propagates_not_retried() {
        // C# has no retry in the read loop: the first reader error (Interrupted included) propagates, uncached.
        for kind in [
            io::ErrorKind::Other,
            io::ErrorKind::Interrupted,
            io::ErrorKind::PermissionDenied,
        ] {
            let root = TempRoot::new("src-bytes-v-read-error");
            let f = fixture(&root, "data.bin", 64);
            let content = f.content.clone();
            let reader = Arc::new(CountingReader::new(move |_, _| {
                boxed(FailOnceStream {
                    inner: ChunkedStream::new(content.clone(), 16),
                    kind,
                    failed: false,
                })
            }));
            let cache = new_cache_with(1024, reader.clone());

            let err = read_range(&cache, &f, 0, 64).expect_err("the read error propagates");
            assert_eq!(err.io_kind(), Some(kind), "kind {kind:?}: got {err:?}");
            assert_eq!(reader.opens(), 1);
            assert_eq!(cache.count(), 0, "a failed read is not cached");

            let err = cache
                .get_or_read(&f.path)
                .expect_err("a whole-file read error propagates");
            assert_eq!(err.io_kind(), Some(kind), "kind {kind:?}: got {err:?}");
            assert_eq!(cache.count(), 0);
        }
    }

    #[test]
    fn v_policy_exposes_cache() {
        let cache = Arc::new(new_cache(1024));
        let policy = SourceBytesCachePolicy::new(Some(Arc::clone(&cache)));
        let exposed = policy.cache().expect("an enabled policy exposes its cache");
        assert!(Arc::ptr_eq(exposed, &cache));
        let cloned = policy.clone();
        assert!(Arc::ptr_eq(cloned.cache().expect("a clone keeps the cache"), &cache));

        assert!(SourceBytesCachePolicy::new(None).cache().is_none());
        assert!(SourceBytesCachePolicy::default().cache().is_none());
    }

    #[test]
    fn v_error_source_is_transparent() {
        let root = TempRoot::new("src-bytes-v-error-source");
        let f = fixture(&root, "data.bin", 16);
        let reader = Arc::new(CountingReader::new(|_, _| {
            Err(io::Error::other(OuterError(InnerError)))
        }));
        let cache = new_cache_with(1024, reader);

        let err = read_range(&cache, &f, 0, 16).expect_err("open fails");
        assert_eq!(err.to_string(), "outer failure");
        let source = err.source().expect("Io(e) exposes e.source()");
        assert_eq!(source.to_string(), "inner cause");

        let arg = SourceBytesError::Argument { param_name: "path" };
        assert!(arg.source().is_none());
        let range = SourceBytesError::ArgumentOutOfRange { param_name: "offset" };
        assert!(range.source().is_none());
    }

    #[test]
    fn v_buffer_size_is_one_mib() {
        // C# passes the ISourceReader.OpenSource default bufferSize (1 MiB).
        assert_eq!(SOURCE_READ_BUFFER_SIZE, 1_048_576);
        let root = TempRoot::new("src-bytes-v-buffer");
        let f = fixture(&root, "data.bin", 32);
        let reader = Arc::new(BufferSizeReader(Mutex::new(Vec::new())));
        let cache = new_cache_with(1024, reader.clone());

        // A partial range, then the whole file (a different key, so both are real reads).
        read_range(&cache, &f, 0, 16).expect("range read");
        cache.get_or_read(&f.path).expect("whole read");

        let sizes = reader.0.lock().map(|log| log.clone()).unwrap_or_default();
        assert_eq!(sizes, vec![1_048_576, 1_048_576]);
    }

    #[test]
    fn v_debug_names_capacity() {
        let text = format!("{:?}", new_cache(1024));
        assert!(text.starts_with("SourceBytesCache"), "{text}");
        assert!(text.contains("capacity_bytes: 1024"), "{text}");
    }
}

/// Repair round 2: path shapes checked against the .NET oracle on d92d6aae (`SourceBytesCache` probe): lone UTF-16
/// surrogates in file names, invalid-name characters on the whole-file API, trailing separator / dot, `CON`.
#[cfg(windows)]
mod path_shapes {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    /// `<root>\<prefix><unit><suffix>` where `unit` is one raw UTF-16 code unit (an unpaired surrogate here).
    fn wide_name(root: &TempRoot, prefix: &str, unit: u16, suffix: &str) -> PathBuf {
        let mut wide: Vec<u16> = prefix.encode_utf16().collect();
        wide.push(unit);
        wide.extend(suffix.encode_utf16());
        root.join(OsString::from_wide(&wide))
    }

    /// Oracle: `surrogate: OK len=3` (GetOrRead), and the read is cached under the surrogate name itself.
    #[test]
    fn whole_file_with_lone_surrogate_name_is_read_and_cached() {
        let root = TempRoot::new("src-bytes-surrogate-whole");
        let path = wide_name(&root, "x", 0xD800, ".bin");
        fs::write(&path, [9u8, 9, 9]).expect("write surrogate-named fixture");
        let reader = physical_reader();
        let cache = new_cache_with(1 << 20, reader.clone());

        let first = cache.get_or_read(&path).expect("C# reads this file");
        let second = cache.get_or_read(&path).expect("cached");

        assert_eq!(&*first, &[9u8, 9, 9]);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(reader.opens(), 1);
        assert!(cache.try_prefetch(&path).expect("prefetch of a cached file"));
        assert_eq!(reader.opens(), 1);
    }

    /// Oracle: `surrogate-range: OK len=2` (GetOrReadRange with the file's own identity).
    #[test]
    fn range_with_lone_surrogate_name_is_read() {
        let root = TempRoot::new("src-bytes-surrogate-range");
        let path = wide_name(&root, "x", 0xD800, ".bin");
        fs::write(&path, [7u8, 8, 9]).expect("write surrogate-named fixture");
        let ticks = file_ticks(&path);
        let cache = new_cache(1 << 20);

        let bytes = cache
            .get_or_read_range(&path, 3, ticks, 1, 2, VIEWER)
            .expect("C# reads this range");

        assert_eq!(&*bytes, &[8u8, 9]);
        assert_eq!(cache.count(), 1);
        let hit = cache.try_get_range(&path, 3, ticks, 1, 2).expect("valid path");
        assert!(hit.is_some_and(|h| Arc::ptr_eq(&h, &bytes)));
    }

    /// Two names that differ only in WHICH unpaired surrogate they hold are two files (a lossy U+FFFD key would merge
    /// them), and evict is OrdinalIgnoreCase on the rest of the name but exact on the surrogate.
    #[test]
    fn distinct_lone_surrogates_are_distinct_keys_and_evict_matches_case_insensitively() {
        let root = TempRoot::new("src-bytes-surrogate-keys");
        let a = wide_name(&root, "x", 0xD800, ".bin");
        let b = wide_name(&root, "x", 0xD801, ".bin");
        fs::write(&a, [1u8]).expect("write a");
        fs::write(&b, [2u8, 2]).expect("write b");
        let reader = physical_reader();
        let cache = new_cache_with(1 << 20, reader.clone());

        assert_eq!(&*cache.get_or_read(&a).expect("read a"), &[1u8]);
        assert_eq!(&*cache.get_or_read(&b).expect("read b"), &[2u8, 2]);
        assert_eq!(cache.count(), 2);
        assert_eq!(reader.opens(), 2);

        // Upper-cased spelling of `a` (only the ASCII parts change): removes `a`, leaves `b`.
        cache
            .evict(&wide_name(&root, "X", 0xD800, ".BIN"))
            .expect("evict a, other case");
        assert_eq!(cache.count(), 1);
        assert_eq!(cache.path_version_count_for_tests(), 1);
        cache.get_or_read(&b).expect("b still cached");
        assert_eq!(reader.opens(), 2, "b was not evicted");
        cache.get_or_read(&a).expect("a re-read");
        assert_eq!(reader.opens(), 3, "a was evicted");
    }

    /// The I/O error message of a whole-file call that `FileInfo.Length` rejects: `"{OS message} : '{full}'."`.
    fn assert_invalid_name(err: &SourceBytesError, full: &Path) {
        assert_ne!(
            err.io_kind(),
            Some(io::ErrorKind::NotFound),
            "C# throws IOException: {err:?}"
        );
        assert!(err.io_kind().is_some(), "an I/O error: {err:?}");
        let text = err.to_string();
        assert!(!text.starts_with("Could not find file"), "{text}");
        assert!(text.ends_with(&format!(" : '{}'.", full.display())), "{text}");
        let os = io::Error::from_raw_os_error(123).to_string();
        let os_message = os.strip_suffix(" (os error 123)").unwrap_or(&os);
        assert!(text.starts_with(os_message), "{text} / {os_message}");
    }

    /// Oracle: `wildcard` and `pipe`: `IOException | The filename, directory name, or volume label syntax is
    /// incorrect. : '<root>\a?.bin'.` (ERROR_INVALID_NAME from `FileInfo.Length`), not `FileNotFoundException`.
    #[test]
    fn whole_file_invalid_name_is_io_error_not_file_not_found() {
        let root = TempRoot::new("src-bytes-invalid-name");
        let reader = Arc::new(CountingReader::must_not_open());
        let cache = new_cache_with(1 << 20, reader.clone());
        for name in ["a?.bin", "a|b.bin", "a*.bin"] {
            let path = root.join(name);
            let err = cache.get_or_read(&path).expect_err(name);
            assert_invalid_name(&err, &path);
            let err = cache.try_prefetch(&path).expect_err(name);
            assert_invalid_name(&err, &path);
        }
        assert_eq!(reader.opens(), 0, "the stat fails before any open");
        assert_eq!(cache.count(), 0);
    }

    /// Oracle: `missing-dir`, `missing-file`, `con`, `prefetch-missing-dir`: `FileNotFoundException | Could not find
    /// file '<full>'.`; `trailing-dot: OK len=4`; `trailing-sep`: an I/O error at open (os error 267), not NotFound.
    #[test]
    fn whole_file_missing_device_trailing_dot_and_separator() {
        let root = TempRoot::new("src-bytes-path-shapes");
        let a = write_file(&root, "a.bin", &[1, 2, 3, 4]);
        let cache = new_cache(1 << 20);

        for missing in [root.join("nodir").join("a.bin"), root.join("b.bin")] {
            let err = cache.get_or_read(&missing).expect_err("missing");
            assert_eq!(err.io_kind(), Some(io::ErrorKind::NotFound));
            assert_eq!(err.to_string(), format!("Could not find file '{}'.", missing.display()));
            let err = cache.try_prefetch(&missing).expect_err("missing");
            assert_eq!(err.io_kind(), Some(io::ErrorKind::NotFound));
        }
        let err = cache.get_or_read(&root.join("CON")).expect_err("device");
        assert_eq!(err.io_kind(), Some(io::ErrorKind::NotFound), "{err:?}");
        let err = cache.get_or_read(root.path()).expect_err("a directory");
        assert_eq!(err.io_kind(), Some(io::ErrorKind::NotFound), "{err:?}");

        let mut dotted = a.clone().into_os_string();
        dotted.push(".");
        assert_eq!(cache.get_or_read(Path::new(&dotted)).expect("trailing dot").len(), 4);

        let mut separated = a.into_os_string();
        separated.push("\\");
        let err = cache
            .get_or_read(Path::new(&separated))
            .expect_err("trailing separator");
        assert!(err.io_kind().is_some(), "{err:?}");
        assert_ne!(err.io_kind(), Some(io::ErrorKind::NotFound), "{err:?}");
    }
}
