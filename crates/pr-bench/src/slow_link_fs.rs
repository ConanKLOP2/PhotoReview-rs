//! Port of `PhotoReview.Benchmarking/SlowLinkFileSystem.cs` (C# baseline d92d6aae, Q-R29 measurement harness).
//!
//! NOT a production default: only the benchmark CLI wires it in. Three types, in C# file order:
//! [`SlowLinkFileSystem`] (a [`FileSystem`] decorator that sleeps a fixed latency before every metadata query and
//! meters `open_read_shared` bytes), [`SharedBandwidthLimiter`] (token bucket shared by every reader handed the same
//! `Arc`) and [`ThrottledReadStream`] (C# `internal`; `pub` here because `SlowLinkSourceReader`, W7-02b, and the
//! integration tests use it).
//!
//! Time and sleeping go through [`TimeSource`] so tests can run on a fake clock; production code uses
//! [`SystemTimeSource`].

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pr_core::abstractions::{AppendStream, CopyCreationProof, FileStat, FileSystem, ReadSeek, SkippedEntry};

/// 100 ns ticks per second (C# `TimeSpan.TicksPerSecond`).
pub const TICKS_PER_SECOND: i64 = 10_000_000;

/// Clock + sleeper behind the harness (C# `DateTime.UtcNow.Ticks` and `Thread.Sleep`).
pub trait TimeSource: Send + Sync {
    /// Monotonic time in 100 ns ticks since an arbitrary origin. Only differences are used.
    fn now_ticks(&self) -> i64;
    /// Blocks the calling thread for `duration`.
    fn sleep(&self, duration: Duration);
}

/// Real clock (`Instant` origin fixed at construction) and `std::thread::sleep`.
#[derive(Debug, Clone, Copy)]
pub struct SystemTimeSource {
    origin: std::time::Instant,
}

impl SystemTimeSource {
    pub fn new() -> Self {
        Self {
            origin: std::time::Instant::now(),
        }
    }
}

impl Default for SystemTimeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeSource for SystemTimeSource {
    fn now_ticks(&self) -> i64 {
        i64::try_from(self.origin.elapsed().as_nanos() / 100).unwrap_or(i64::MAX)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// C# `ArgumentOutOfRangeException` for the constructor guard of [`SharedBandwidthLimiter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidBandwidth {
    pub bytes_per_second: i64,
}

impl fmt::Display for InvalidBandwidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "bytesPerSecond must be greater than zero (was {})",
            self.bytes_per_second
        )
    }
}

impl std::error::Error for InvalidBandwidth {}

/// Token-bucket rate limiter shared by every reader that models one link's total bandwidth. Thread-safe; blocks the
/// calling thread until enough tokens have accumulated. The bucket starts EMPTY and is capped at one second's budget.
pub struct SharedBandwidthLimiter {
    bytes_per_second: i64,
    time: Arc<dyn TimeSource>,
    bucket: Mutex<Bucket>,
}

#[derive(Debug)]
struct Bucket {
    available_bytes: f64,
    last_ticks: i64,
}

impl fmt::Debug for SharedBandwidthLimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedBandwidthLimiter")
            .field("bytes_per_second", &self.bytes_per_second)
            .finish_non_exhaustive()
    }
}

impl SharedBandwidthLimiter {
    /// C# `new SharedBandwidthLimiter(bytesPerSecond)`: `Err` when `bytes_per_second <= 0`. Uses [`SystemTimeSource`].
    pub fn new(bytes_per_second: i64) -> Result<Self, InvalidBandwidth> {
        Self::with_time_source(bytes_per_second, Arc::new(SystemTimeSource::new()))
    }

    /// As [`new`](Self::new) with an injected clock (tests). `last_ticks` starts at `time.now_ticks()`, `available_bytes` at 0.
    pub fn with_time_source(bytes_per_second: i64, time: Arc<dyn TimeSource>) -> Result<Self, InvalidBandwidth> {
        if bytes_per_second <= 0 {
            return Err(InvalidBandwidth { bytes_per_second });
        }
        let last_ticks = time.now_ticks();
        Ok(Self {
            bytes_per_second,
            time,
            bucket: Mutex::new(Bucket {
                available_bytes: 0.0,
                last_ticks,
            }),
        })
    }

    pub fn bytes_per_second(&self) -> i64 {
        self.bytes_per_second
    }

    /// Blocks until `bytes` worth of the shared budget is available. `bytes <= 0` returns at once. A request larger than
    /// one second's budget is metered in chunks of at most `bytes_per_second` (the bucket can never hold more).
    pub fn consume(&self, bytes: i32) {
        if bytes <= 0 {
            return;
        }
        let mut remaining = i64::from(bytes);
        while remaining > 0 {
            let chunk = remaining.min(self.bytes_per_second);
            // `chunk <= bytes`, which fits in i32.
            let chunk = i32::try_from(chunk).unwrap_or(i32::MAX);
            self.consume_chunk(chunk);
            remaining -= i64::from(chunk);
        }
    }

    /// One chunk (`bytes <= bytes_per_second`): loop { lock; refill; if available >= bytes { subtract; return }; compute
    /// wait = (bytes - available) / bytes_per_second seconds; unlock; sleep max(wait, 1 ms) } (sleep OUTSIDE the lock).
    fn consume_chunk(&self, bytes: i32) {
        let needed = f64::from(bytes);
        loop {
            let wait_secs = {
                let mut bucket = self.bucket.lock().unwrap_or_else(PoisonError::into_inner);
                self.refill(&mut bucket);
                if bucket.available_bytes >= needed {
                    bucket.available_bytes -= needed;
                    return;
                }
                let missing = needed - bucket.available_bytes;
                missing / self.bytes_per_second as f64
            };
            // C# `Thread.Sleep(TimeSpan)` truncates to whole milliseconds; the 1 ms floor applies after truncation.
            let whole_ms = (wait_secs * 1000.0) as u64;
            self.time.sleep(Duration::from_millis(whole_ms.max(1)));
        }
    }

    /// `elapsed = (now - last_ticks) / TICKS_PER_SECOND` as f64; `last_ticks = now` BEFORE the early return;
    /// `elapsed <= 0` returns; else `available = min(bytes_per_second, available + elapsed * bytes_per_second)`.
    fn refill(&self, bucket: &mut Bucket) {
        let now = self.time.now_ticks();
        let elapsed = now.saturating_sub(bucket.last_ticks) as f64 / TICKS_PER_SECOND as f64;
        bucket.last_ticks = now;
        if elapsed <= 0.0 {
            return;
        }
        let rate = self.bytes_per_second as f64;
        bucket.available_bytes = rate.min(bucket.available_bytes + elapsed * rate);
    }
}

/// Wraps a stream so every byte read is metered against a [`SharedBandwidthLimiter`] (C# `ThrottledReadStream`).
/// Read-only by construction (C# `Write` throws `NotSupportedException`: Rust simply has no `Write` impl). Dropping it
/// drops (closes) the inner stream.
pub struct ThrottledReadStream {
    inner: Box<dyn ReadSeek>,
    limiter: Arc<SharedBandwidthLimiter>,
}

impl fmt::Debug for ThrottledReadStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThrottledReadStream").finish_non_exhaustive()
    }
}

impl ThrottledReadStream {
    pub fn new(inner: Box<dyn ReadSeek>, limiter: Arc<SharedBandwidthLimiter>) -> Self {
        Self { inner, limiter }
    }
}

impl Read for ThrottledReadStream {
    /// `read = inner.read(buf)?`; when `read > 0` call `limiter.consume(read)` (as `i32`, saturating) AFTER the inner read
    /// (so the bytes are paid for once read); return `read`. An inner error is returned without consuming.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        if read > 0 {
            self.limiter.consume(i32::try_from(read).unwrap_or(i32::MAX));
        }
        Ok(read)
    }
}

impl Seek for ThrottledReadStream {
    /// Forwarded untouched (C# `Seek`, `Position`, `Length`): seeking is free.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// Decorator that simulates a slow NAS/wifi link (C# `SlowLinkFileSystem`).
///
/// Delayed by `metadata_latency` (when > 0): `file_exists`, `directory_exists`, `get_file_stat`, `try_get_file_stat`,
/// `try_probe_readable`, `resolve_real_path` (once per call, BEFORE delegating, also when the call then fails) and the
/// two `enumerate_*_with_stat` overloads (once per entry returned by the inner listing). Metered: `open_read_shared`
/// when a limiter was given. Everything else is forwarded untouched. EVERY trait method is overridden, including those
/// with default bodies (see `pr_core::io::counting`): a left-over default would replace the inner file system's atomic
/// create-new copy by "exists-check then copy" (C# `FileSystemWrapperForwardingTests`).
pub struct SlowLinkFileSystem {
    inner: Arc<dyn FileSystem>,
    metadata_latency: Duration,
    bandwidth: Option<Arc<SharedBandwidthLimiter>>,
    time: Arc<dyn TimeSource>,
}

impl fmt::Debug for SlowLinkFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlowLinkFileSystem")
            .field("metadata_latency", &self.metadata_latency)
            .finish_non_exhaustive()
    }
}

impl SlowLinkFileSystem {
    /// C# `new SlowLinkFileSystem(inner, metadataLatency, bandwidth)` with the real clock. (C# throws for a negative
    /// latency or a null inner; `Duration` is never negative and `Arc` never null, so this cannot fail.)
    pub fn new(
        inner: Arc<dyn FileSystem>,
        metadata_latency: Duration,
        bandwidth: Option<Arc<SharedBandwidthLimiter>>,
    ) -> Self {
        Self::with_time_source(inner, metadata_latency, bandwidth, Arc::new(SystemTimeSource::new()))
    }

    /// As [`new`](Self::new) with an injected sleeper (tests).
    pub fn with_time_source(
        inner: Arc<dyn FileSystem>,
        metadata_latency: Duration,
        bandwidth: Option<Arc<SharedBandwidthLimiter>>,
        time: Arc<dyn TimeSource>,
    ) -> Self {
        Self {
            inner,
            metadata_latency,
            bandwidth,
            time,
        }
    }

    /// C# `DelayMetadata`: sleeps `metadata_latency` when it is greater than zero.
    fn delay_metadata(&self) {
        if self.metadata_latency > Duration::ZERO {
            // C# `Thread.Sleep(TimeSpan)` truncates to whole ms; a 0 ms result is a yield, not a sleep.
            let whole_ms = u64::try_from(self.metadata_latency.as_millis()).unwrap_or(u64::MAX);
            if whole_ms == 0 {
                std::thread::yield_now();
            } else {
                self.time.sleep(Duration::from_millis(whole_ms));
            }
        }
    }

    /// One latency charge per entry the inner listing returned (C# yields and sleeps per entry).
    fn delay_per_entry(&self, listing: Vec<(PathBuf, Option<FileStat>)>) -> Vec<(PathBuf, Option<FileStat>)> {
        for _ in &listing {
            self.delay_metadata();
        }
        listing
    }
}

impl FileSystem for SlowLinkFileSystem {
    fn file_exists(&self, path: &Path) -> bool {
        self.delay_metadata();
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> bool {
        self.delay_metadata();
        self.inner.directory_exists(path)
    }

    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.delay_metadata();
        self.inner.get_file_stat(path)
    }

    /// P-12: not in the C# baseline; a metadata query like `get_file_stat` (delay once, then the INNER typed-error stat so
    /// a denied stat is never folded into `Ok(None)`).
    fn try_get_file_stat(&self, path: &Path) -> io::Result<Option<FileStat>> {
        self.delay_metadata();
        self.inner.try_get_file_stat(path)
    }

    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.move_file(source, destination)
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.inner.copy_file(source, destination)
    }

    fn try_copy_new(&self, source: &Path, destination: &Path) -> io::Result<bool> {
        self.inner.try_copy_new(source, destination)
    }

    fn try_copy_new_with_proof(
        &self,
        source: &Path,
        destination: &Path,
        proof: &CopyCreationProof,
    ) -> io::Result<bool> {
        self.inner.try_copy_new_with_proof(source, destination, proof)
    }

    fn delete(&self, path: &Path) -> io::Result<()> {
        self.inner.delete(path)
    }

    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        let stream = self.inner.open_read_shared(path, buffer_size)?;
        match &self.bandwidth {
            Some(limiter) => Ok(Box::new(ThrottledReadStream::new(stream, Arc::clone(limiter)))),
            None => Ok(stream),
        }
    }

    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.inner.open_append(path, durable)
    }

    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.inner.write_all_text_atomic(path, text, durable)
    }

    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.inner.read_all_text(path)
    }

    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.inner.enumerate_files(directory, pattern)
    }

    fn enumerate_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        let listing = self.inner.enumerate_files_with_stat(directory, include, on_skipped)?;
        Ok(self.delay_per_entry(listing))
    }

    fn try_probe_readable(&self, path: &Path) -> Result<(), String> {
        self.delay_metadata();
        self.inner.try_probe_readable(path)
    }

    fn enumerate_readable_files_with_stat(
        &self,
        directory: &Path,
        include: &dyn Fn(&Path) -> bool,
        on_skipped: &mut dyn FnMut(SkippedEntry),
    ) -> io::Result<Vec<(PathBuf, Option<FileStat>)>> {
        let listing = self
            .inner
            .enumerate_readable_files_with_stat(directory, include, on_skipped)?;
        Ok(self.delay_per_entry(listing))
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
        self.delay_metadata();
        self.inner.resolve_real_path(path)
    }
}
