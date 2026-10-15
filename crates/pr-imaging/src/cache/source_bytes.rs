//! Port of `PhotoReview.Imaging/Caching/SourceBytesCache.cs` and `SourceBytesCachePolicy.cs` (C# baseline d92d6aae).
//!
//! An optional RAM cache of source-file bytes (whole files and byte ranges), keyed by the full path plus the source
//! identity (length and last write time in .NET ticks). Concurrent readers of one key share ONE disk read (in-flight
//! dedup, the read runs on the calling thread). `clear` and `evict` invalidate reads that are still in flight
//! (INV-2): a generation counter (global) and per-path versions are captured before the read and re-checked under the
//! publish gate, so a read that finishes after an eviction returns its bytes but never republishes them. Every open
//! goes through [`SourceReader`] (INV-8: shared ReadWrite | Delete, SequentialScan).
//!
//! Threading (C# IMG-09): every read is synchronous on the calling thread. Never call the read methods from the UI
//! thread.
//!
//! Target path: `crates/pr-imaging/src/cache/source_bytes.rs` (`pr_imaging::cache::source_bytes`).

// Stub: the bodies are todo!() until the porter fills them. Remove this line when done.
#![allow(unused_variables, dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicI32;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::ThreadId;

use pr_core::abstractions::{SourceReadPriority, SourceReader};
use pr_core::caching::lru::BoundedLruCache;

/// The bytes the cache hands out. C# `byte[]`: every caller of one cached key gets the SAME allocation
/// (`Assert.Same` in C# is `Arc::ptr_eq` in Rust).
pub type SourceBytes = Arc<[u8]>;

/// .NET `Array.MaxLength` (0x7FFFFFC7): the largest byte array C# can allocate. Ranges and files above it are refused
/// exactly where C# refuses them, even though a Rust `Vec` could be larger (parity of `can_cache` and the errors).
pub const ARRAY_MAX_LENGTH: i64 = 0x7FFF_FFC7;

/// C# `MaxTrackedPathVersions`: most per-path eviction versions kept before [`SourceBytesCache::evict`] switches to a
/// global generation bump.
pub const MAX_TRACKED_PATH_VERSIONS: usize = 4096;

/// C# `ISourceReader.OpenSource` default `bufferSize` (1 MiB): the value this cache passes to the reader.
pub const SOURCE_READ_BUFFER_SIZE: usize = 1024 * 1024;

/// A test seam callback (C# `Action?` properties `BeforePublishForTests`, `AfterReadForTests`, `AfterCacheMissForTests`).
pub type TestHook = Arc<dyn Fn() + Send + Sync>;

/// Errors of the cache. C# exception types map one to one:
/// - `ArgumentException` (`ThrowIfNullOrWhiteSpace(path)`, `Path.GetFullPath` of an empty / space-only / NUL path)
///   => [`SourceBytesError::Argument`];
/// - `ArgumentOutOfRangeException` => [`SourceBytesError::ArgumentOutOfRange`] with the C# `ParamName`
///   (`"offset"`, `"count"`, `"length"`, `"key"`, `"capacity"`);
/// - every `IOException` (incl. `FileNotFoundException`, `EndOfStreamException`, `PathTooLongException`, the
///   localized "file changed while reading") => [`SourceBytesError::Io`].
///
/// `Clone` because one failed shared read is replayed to every caller that waited on it (C# `Lazy<T>` caches the
/// exception); the `io::Error` is therefore behind an `Arc`.
///
/// Localization (C# `UserFacingError`) lives on the INNER `io::Error`: apply `pr_l10n::template::{is_localized, describe}`
/// to `&**e` of `Io(e)`, not to the `SourceBytesError` (its payload is not visible through it).
#[derive(Debug, Clone)]
pub enum SourceBytesError {
    Argument { param_name: &'static str },
    ArgumentOutOfRange { param_name: &'static str },
    Io(Arc<io::Error>),
}

impl SourceBytesError {
    /// The `io::ErrorKind` of an [`SourceBytesError::Io`]; `None` for the argument errors.
    pub fn io_kind(&self) -> Option<io::ErrorKind> {
        todo!()
    }
}

/// `Io(e)`: transparent (`e`'s own text, the English .NET message). `Argument { p }`: `"Invalid argument. (Parameter '{p}')"`.
/// `ArgumentOutOfRange { p }`: `"Specified argument was out of the range of valid values. (Parameter '{p}')"`.
impl fmt::Display for SourceBytesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for SourceBytesError {
    /// Transparent like `Display`: `Io(e)` => `e.source()`; the argument variants => `None`.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        todo!()
    }
}

impl From<io::Error> for SourceBytesError {
    fn from(err: io::Error) -> Self {
        Self::Io(Arc::new(err))
    }
}

/// C# `private readonly record struct RangeKey(string Path, long SourceLength, long LastWriteUtcTicks, long Offset,
/// long Count)`. Equality and hash are ORDINAL on `path` (record struct default `string` equality): `C:\a\X.jpg` and
/// `C:\a\x.jpg` are two different keys (trap 1). A whole-file key has `offset == 0` and `count == source_length`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RangeKey {
    /// `Path.GetFullPath(path)`, spelling kept as given.
    path: String,
    source_length: i64,
    last_write_utc_ticks: i64,
    offset: i64,
    count: i64,
}

/// One shared read (C# `Lazy<byte[]>` with `ExecutionAndPublication`). The generation, path version and priority are
/// those of the caller that CREATED the entry (they are captured in the C# factory closure), not of whichever waiter
/// happens to run the initializer.
struct InFlight {
    generation: i32,
    path_version: i32,
    priority: SourceReadPriority,
    result: OnceLock<Result<SourceBytes, SourceBytesError>>,
}

/// C# `SourceBytesCache`.
pub struct SourceBytesCache {
    source_reader: Arc<dyn SourceReader>,
    /// Effective (post-clamp) capacity.
    capacity_bytes: i64,
    cache: BoundedLruCache<RangeKey, SourceBytes>,
    /// C# `ConcurrentDictionary<RangeKey, Lazy<byte[]>> _inFlight`.
    in_flight: Mutex<HashMap<RangeKey, Arc<InFlight>>>,
    /// C# `int _generation` (Interlocked.Increment wraps: use `fetch_add`, which wraps too).
    generation: AtomicI32,
    /// C# `_publishGate`: makes "version still current -> set" atomic against `evict`/`clear`'s "bump -> remove".
    /// Held only around in-memory work, never a disk read.
    publish_gate: Mutex<()>,
    /// C# `ConcurrentDictionary<string, int>(OrdinalIgnoreCase) _pathVersions`; key = `pr_core::text::ignore_case_key`
    /// of the full path.
    path_versions: Mutex<HashMap<String, i32>>,
    before_publish_for_tests: Mutex<Option<TestHook>>,
    after_read_for_tests: Mutex<Option<TestHook>>,
    after_cache_miss_for_tests: Mutex<Option<TestHook>>,
    /// C# `LastReadManagedThreadId`.
    last_read_thread_id: Mutex<Option<ThreadId>>,
}

impl fmt::Debug for SourceBytesCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceBytesCache")
            .field("capacity_bytes", &self.capacity_bytes)
            .finish_non_exhaustive()
    }
}

impl SourceBytesCache {
    /// C# constructor `SourceBytesCache(long capacityBytes, ISourceReader? sourceReader = null)`.
    ///
    /// The capacity is clamped with `ram_budget::clamp_source_bytes_to_physical_memory(capacity_bytes,
    /// physical_memory_bytes)` (W4-07a; IMG-11/R2-A-06: never more than 20 % of physical RAM). C# calls
    /// `RamBudgetPolicy.GetPhysicalMemoryBytes()` itself; Rust takes the value so tests are machine-independent (the
    /// composition root passes `ram_budget::get_physical_memory_bytes(..)`). `physical_memory_bytes <= 0` means
    /// unknown: no clamp. `source_reader == None` uses [`pr_core::abstractions::PhysicalSourceReader`].
    ///
    /// Errors: a clamped capacity `<= 0` is `ArgumentOutOfRange { param_name: "capacity" }` (C# `BoundedLruCache`
    /// constructor, `ThrowIfNegativeOrZero(capacity)`).
    pub fn new(
        capacity_bytes: i64,
        physical_memory_bytes: i64,
        source_reader: Option<Arc<dyn SourceReader>>,
    ) -> Result<Self, SourceBytesError> {
        todo!()
    }

    /// C# `CapacityBytes`: the effective (post-clamp) capacity in bytes.
    pub fn capacity_bytes(&self) -> i64 {
        todo!()
    }

    /// C# `CurrentSize`: sum of the cached entry sizes (`max(1, len)` each, see `BoundedLruCache`).
    pub fn current_size(&self) -> i64 {
        todo!()
    }

    /// C# `Count`.
    pub fn count(&self) -> usize {
        todo!()
    }

    /// C# `GetOrRead(string path)`: the whole file, priority [`SourceReadPriority::Viewer`].
    pub fn get_or_read(&self, path: &Path) -> Result<SourceBytes, SourceBytesError> {
        todo!()
    }

    /// C# `GetOrRead(string path, SourceReadPriority priority)`. `priority` is metadata for the reader only; it never
    /// changes which bytes are cached or returned.
    pub fn get_or_read_with_priority(
        &self,
        path: &Path,
        priority: SourceReadPriority,
    ) -> Result<SourceBytes, SourceBytesError> {
        todo!()
    }

    /// C# `GetOrReadRange(path, length, lastWriteUtcTicks, offset, count, priority = Viewer)`: only the requested range,
    /// cached under the source identity plus offset and count. `count` is `i32` like C# `int` (negative values are
    /// valid input and must be refused with the C# error).
    pub fn get_or_read_range(
        &self,
        path: &Path,
        length: i64,
        last_write_utc_ticks: i64,
        offset: i64,
        count: i32,
        priority: SourceReadPriority,
    ) -> Result<SourceBytes, SourceBytesError> {
        todo!()
    }

    /// C# `TryGetRange`: the cached bytes of exactly this range without reading anything. `Ok(None)` is the C#
    /// `false` (miss or invalid range; the C# out value is then an empty array). `Err` only where C# throws:
    /// `Path.GetFullPath` of a valid-range request with an empty / space-only / NUL path.
    pub fn try_get_range(
        &self,
        path: &Path,
        length: i64,
        last_write_utc_ticks: i64,
        offset: i64,
        count: i32,
    ) -> Result<Option<SourceBytes>, SourceBytesError> {
        todo!()
    }

    /// C# `TryPrefetch`: caches the file (priority [`SourceReadPriority::Preload`]) unless [`Self::can_cache`] says
    /// it never could, in which case nothing is opened at all and the result is `Ok(false)`.
    pub fn try_prefetch(&self, path: &Path) -> Result<bool, SourceBytesError> {
        todo!()
    }

    /// C# `CanCache`: `length <= capacity_bytes && length <= ARRAY_MAX_LENGTH` (negative lengths are `true`, as in C#).
    pub fn can_cache(&self, length: i64) -> bool {
        todo!()
    }

    /// C# `CanCacheRange`: `count >= 0 && count <= capacity_bytes && count <= ARRAY_MAX_LENGTH`.
    pub fn can_cache_range(&self, count: i64) -> bool {
        todo!()
    }

    /// C# `Clear`: under the publish gate, drop every per-path version, bump the generation, clear the LRU.
    pub fn clear(&self) {
        todo!()
    }

    /// C# `Evict(string path)`: invalidates in-flight reads of this path (per-path version, or a global generation
    /// bump once [`MAX_TRACKED_PATH_VERSIONS`] distinct paths are tracked) and removes every cached key whose path is
    /// OrdinalIgnoreCase-equal to `Path.GetFullPath(path)`. `Err` only for a path `GetFullPath` rejects.
    pub fn evict(&self, path: &Path) -> Result<(), SourceBytesError> {
        todo!()
    }

    // ---- test seams (C# `internal ... ForTests`; `pub` + `doc(hidden)` so `crates/pr-imaging/tests` can use them) ----

    /// C# `BeforePublishForTests`: runs inside the publish gate between the version check and the `set`.
    #[doc(hidden)]
    pub fn set_before_publish_for_tests(&self, hook: Option<TestHook>) {
        todo!()
    }

    /// C# `AfterReadForTests`: runs after the read loop, just before the length / last-write re-check (outside the gate).
    #[doc(hidden)]
    pub fn set_after_read_for_tests(&self, hook: Option<TestHook>) {
        todo!()
    }

    /// C# `AfterCacheMissForTests`: runs on the calling thread between the cache miss and the in-flight lookup.
    #[doc(hidden)]
    pub fn set_after_cache_miss_for_tests(&self, hook: Option<TestHook>) {
        todo!()
    }

    /// C# `PathVersionCountForTests` (RV-I16): number of per-path eviction versions still tracked.
    #[doc(hidden)]
    pub fn path_version_count_for_tests(&self) -> usize {
        todo!()
    }

    /// C# `LastReadManagedThreadId`: the thread that performed the most recent real read (not a cache hit).
    #[doc(hidden)]
    pub fn last_read_thread_id_for_tests(&self) -> Option<ThreadId> {
        todo!()
    }

    // ---- private (C# private methods; signatures are a suggestion, the porter may reshape them) ----

    /// C# `GetOrRead(RangeKey key, SourceReadPriority priority)`: cache hit, else the shared in-flight read.
    fn get_or_read_key(&self, key: RangeKey, priority: SourceReadPriority) -> Result<SourceBytes, SourceBytesError> {
        todo!()
    }

    /// C# `ReadAndCache(key, generation, pathVersion, priority, publish = true)`.
    fn read_and_cache(
        &self,
        key: &RangeKey,
        generation: i32,
        path_version: i32,
        priority: SourceReadPriority,
        publish: bool,
    ) -> Result<SourceBytes, SourceBytesError> {
        todo!()
    }

    /// C# `CreateKey(path)`: full path + `new FileInfo(full)` length and last write ticks; offset 0, count = length.
    fn create_key(path: &Path) -> Result<RangeKey, SourceBytesError> {
        todo!()
    }

    /// C# `CreateRangeKey(path, length, lastWriteUtcTicks, offset, count)`: full path, no file access.
    fn create_range_key(
        path: &Path,
        length: i64,
        last_write_utc_ticks: i64,
        offset: i64,
        count: i32,
    ) -> Result<RangeKey, SourceBytesError> {
        todo!()
    }
}

/// C# `SourceBytesCachePolicy`: the single decision point for whether the source-bytes cache is enabled. Resolved once
/// from `AppSettings.use_source_bytes_cache` at composition; consumers read [`Self::cache`] instead of re-checking the
/// setting.
#[derive(Debug, Clone, Default)]
pub struct SourceBytesCachePolicy {
    cache: Option<Arc<SourceBytesCache>>,
}

impl SourceBytesCachePolicy {
    /// C# primary constructor `SourceBytesCachePolicy(SourceBytesCache? cache)`.
    pub fn new(cache: Option<Arc<SourceBytesCache>>) -> Self {
        todo!()
    }

    /// C# `Cache`: `None` when the cache is disabled.
    pub fn cache(&self) -> Option<&Arc<SourceBytesCache>> {
        todo!()
    }
}
