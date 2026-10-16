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

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, Metadata};
use std::io::{self, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{self, ThreadId};

use pr_core::abstractions::{FileStat, FileSystem, PhysicalSourceReader, SourceReadPriority, SourceReader};
use pr_core::caching::lru::BoundedLruCache;
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::text::dotnet_path::{PathError, get_full_path};
use pr_core::text::ignore_case_key;
use pr_core::time::DateTimeUtc;

use crate::decoder_traits::UserText;
use crate::ram_budget::clamp_source_bytes_to_physical_memory;

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
/// Localization (C# `UserFacingError`, D41): the "file changed while reading" and "unexpected EOF" errors carry a UI
/// sentence rendered by the injected [`SourceBytesTexts`]; read it with [`SourceBytesError::is_localized`],
/// [`SourceBytesError::describe`] or [`SourceBytesError::user_text`]. `pr_l10n::template::is_localized` does NOT see
/// it (pr-imaging cannot build a `pr_l10n::LocalizedError`); a consumer that needs a pr-l10n error re-wraps the inner
/// `io::Error` with `pr_l10n::template::localize_io` and the closure from `user_text`.
#[derive(Debug, Clone)]
pub enum SourceBytesError {
    Argument { param_name: &'static str },
    ArgumentOutOfRange { param_name: &'static str },
    Io(Arc<io::Error>),
}

impl SourceBytesError {
    /// The `io::ErrorKind` of an [`SourceBytesError::Io`]; `None` for the argument errors.
    pub fn io_kind(&self) -> Option<io::ErrorKind> {
        match self {
            Self::Io(e) => Some(e.kind()),
            Self::Argument { .. } | Self::ArgumentOutOfRange { .. } => None,
        }
    }

    /// The UI sentence attached to the error (C# `UserFacingError.Localized`, D41): only the "file changed while
    /// reading" and "unexpected EOF" errors carry one. The composition root can hand it to
    /// `pr_l10n::template::localize_io` (the closure renders in the language current when it is called).
    pub fn user_text(&self) -> Option<UserText> {
        match self {
            Self::Io(e) => user_text_of(e),
            Self::Argument { .. } | Self::ArgumentOutOfRange { .. } => None,
        }
    }

    /// C# `UserFacingError.IsLocalized(ex)`: a UI sentence is attached (see [`Self::user_text`]).
    pub fn is_localized(&self) -> bool {
        self.user_text().is_some()
    }

    /// C# `UserFacingError.Describe(ex)`: the attached sentence, else the `Display` text. A sentence closure that
    /// panics falls back to the `Display` text (C# swallows the exception).
    pub fn describe(&self) -> String {
        match self.user_text() {
            Some(text) => panic::catch_unwind(AssertUnwindSafe(|| text())).unwrap_or_else(|_| self.to_string()),
            None => self.to_string(),
        }
    }
}

/// The two localized sentences of the cache (C# `Tr.ErrIoFileChangedWhileReading` / `Tr.ErrIoUnexpectedEof`, catalog
/// keys `err.io.fileChangedWhileReading` / `err.io.unexpectedEof`, argument `{path}`). Injected instead of a static
/// `Tr` (D41 / P-24: no `pr-l10n` dependency in pr-imaging; the D13 pattern). The cache stores the texts and the path
/// in the error's [`UserText`] closure, so a language switch applies at display time.
pub trait SourceBytesTexts: Send + Sync {
    /// `err.io.fileChangedWhileReading`: "The file changed while it was being read: {path}".
    fn file_changed_while_reading(&self, path: &str) -> String;
    /// `err.io.unexpectedEof`: "The file ended unexpectedly while reading: {path}".
    fn unexpected_eof(&self, path: &str) -> String;
}

/// The built-in English catalog (`assets/languages/en.json`): what C# `Tr` renders while the UI language is English.
/// The default texts of [`SourceBytesCache::new`].
#[derive(Debug, Clone, Copy, Default)]
pub struct EnglishSourceBytesTexts;

impl SourceBytesTexts for EnglishSourceBytesTexts {
    fn file_changed_while_reading(&self, path: &str) -> String {
        format!("The file changed while it was being read: {path}")
    }

    fn unexpected_eof(&self, path: &str) -> String {
        format!("The file ended unexpectedly while reading: {path}")
    }
}

/// The payload of a localized `io::Error` (C# `exception.Data["PhotoReview.UserFacingText"]`): `Display` is the
/// English .NET message, `text` the UI sentence.
struct UserFacingIo {
    message: String,
    text: UserText,
}

impl fmt::Debug for UserFacingIo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserFacingIo")
            .field("message", &self.message)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for UserFacingIo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UserFacingIo {}

/// The sentence of an `io::Error` built by this module (`None` for any other error).
fn user_text_of(err: &io::Error) -> Option<UserText> {
    err.get_ref()
        .and_then(|inner| inner.downcast_ref::<UserFacingIo>())
        .map(|payload| Arc::clone(&payload.text))
}

/// `Io(e)`: transparent (`e`'s own text, the English .NET message). `Argument { p }`: `"Invalid argument. (Parameter '{p}')"`.
/// `ArgumentOutOfRange { p }`: `"Specified argument was out of the range of valid values. (Parameter '{p}')"`.
impl fmt::Display for SourceBytesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => fmt::Display::fmt(&**e, f),
            Self::Argument { param_name } => write!(f, "Invalid argument. (Parameter '{param_name}')"),
            Self::ArgumentOutOfRange { param_name } => write!(
                f,
                "Specified argument was out of the range of valid values. (Parameter '{param_name}')"
            ),
        }
    }
}

impl std::error::Error for SourceBytesError {
    /// Transparent like `Display`: `Io(e)` => `e.source()`; the argument variants => `None`.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => std::error::Error::source(&**e),
            Self::Argument { .. } | Self::ArgumentOutOfRange { .. } => None,
        }
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
///
/// `path` is an `OsString`, not the `String` of the contract sketch: a Windows file name may hold an unpaired UTF-16
/// surrogate, which a C# `string` keeps (and the file opens), while a lossy `String` turns it into U+FFFD and names a
/// different file. `OsStr` equality on Windows is UTF-16 ordinal equality (NOT `PathBuf`, whose equality compares
/// components: `a.bin\` would equal `a.bin`, and `C:/x` would equal `C:\x`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RangeKey {
    /// `Path.GetFullPath(path)`, spelling kept as given.
    path: OsString,
    source_length: i64,
    last_write_utc_ticks: i64,
    offset: i64,
    count: i64,
}

impl RangeKey {
    fn path(&self) -> &Path {
        Path::new(&self.path)
    }
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
    /// C# `ConcurrentDictionary<string, int>(OrdinalIgnoreCase) _pathVersions`; key = [`os_ignore_case_key`] of the
    /// full path (`pr_core::text::ignore_case_key` on the Unicode runs, unpaired surrogates kept as they are).
    path_versions: Mutex<HashMap<Vec<u8>, i32>>,
    before_publish_for_tests: Mutex<Option<TestHook>>,
    after_read_for_tests: Mutex<Option<TestHook>>,
    after_cache_miss_for_tests: Mutex<Option<TestHook>>,
    /// C# `LastReadManagedThreadId`.
    last_read_thread_id: Mutex<Option<ThreadId>>,
    /// The localized sentences of the read errors (C# `Tr`, D41). Default [`EnglishSourceBytesTexts`].
    texts: Arc<dyn SourceBytesTexts>,
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
        let capacity = clamp_source_bytes_to_physical_memory(capacity_bytes, physical_memory_bytes);
        let cache = BoundedLruCache::new(capacity, |bytes: &SourceBytes| {
            i64::try_from(bytes.len()).unwrap_or(i64::MAX)
        })
        .map_err(|_| SourceBytesError::ArgumentOutOfRange { param_name: "capacity" })?;
        let source_reader = source_reader.unwrap_or_else(|| Arc::new(PhysicalSourceReader));
        Ok(Self {
            source_reader,
            capacity_bytes: capacity,
            cache,
            in_flight: Mutex::new(HashMap::new()),
            generation: AtomicI32::new(0),
            publish_gate: Mutex::new(()),
            path_versions: Mutex::new(HashMap::new()),
            before_publish_for_tests: Mutex::new(None),
            after_read_for_tests: Mutex::new(None),
            after_cache_miss_for_tests: Mutex::new(None),
            last_read_thread_id: Mutex::new(None),
            texts: Arc::new(EnglishSourceBytesTexts),
        })
    }

    /// Replaces the localized sentences of the read errors (D41: the composition root passes a catalog-backed
    /// [`SourceBytesTexts`]; the default is [`EnglishSourceBytesTexts`]). Additive to the C# constructor, which reads the
    /// ambient `Tr`.
    #[must_use]
    pub fn with_texts(mut self, texts: Arc<dyn SourceBytesTexts>) -> Self {
        self.texts = texts;
        self
    }

    /// C# `CapacityBytes`: the effective (post-clamp) capacity in bytes.
    pub fn capacity_bytes(&self) -> i64 {
        self.capacity_bytes
    }

    /// C# `CurrentSize`: sum of the cached entry sizes (`max(1, len)` each, see `BoundedLruCache`).
    pub fn current_size(&self) -> i64 {
        self.cache.current_size()
    }

    /// C# `Count`.
    pub fn count(&self) -> usize {
        self.cache.count()
    }

    /// C# `GetOrRead(string path)`: the whole file, priority [`SourceReadPriority::Viewer`].
    pub fn get_or_read(&self, path: &Path) -> Result<SourceBytes, SourceBytesError> {
        self.get_or_read_with_priority(path, SourceReadPriority::Viewer)
    }

    /// C# `GetOrRead(string path, SourceReadPriority priority)`. `priority` is metadata for the reader only; it never
    /// changes which bytes are cached or returned.
    pub fn get_or_read_with_priority(
        &self,
        path: &Path,
        priority: SourceReadPriority,
    ) -> Result<SourceBytes, SourceBytesError> {
        self.get_or_read_key(Self::create_key(path)?, priority)
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
        // ArgumentException.ThrowIfNullOrWhiteSpace(path): only this entry point has it.
        if path.to_string_lossy().chars().all(char::is_whitespace) {
            return Err(SourceBytesError::Argument { param_name: "path" });
        }
        if length < 0 {
            return Err(SourceBytesError::ArgumentOutOfRange { param_name: "length" });
        }
        let count_long = i64::from(count);
        // `length >= 0` and `0 <= offset <= length` hold before `length - offset`, so it cannot overflow.
        if offset < 0 || count_long < 0 || offset > length || count_long > length - offset {
            return Err(SourceBytesError::ArgumentOutOfRange { param_name: "offset" });
        }
        // Past the .NET array limit no read can succeed (checked before the uncacheable-range bypass below).
        if count_long > ARRAY_MAX_LENGTH {
            return Err(SourceBytesError::ArgumentOutOfRange { param_name: "count" });
        }
        let key = Self::create_range_key(path, length, last_write_utc_ticks, offset, count)?;
        if count == 0 {
            return Ok(Arc::from(Vec::new()));
        }
        // A range the cache could never keep is read straight through: no in-flight entry, no publish attempt.
        if !self.can_cache_range(count_long) {
            return self.read_and_cache(&key, 0, 0, priority, false);
        }
        self.get_or_read_key(key, priority)
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
        let count_long = i64::from(count);
        if offset < 0 || count_long <= 0 || offset > length || count_long > length - offset {
            return Ok(None);
        }
        let key = Self::create_range_key(path, length, last_write_utc_ticks, offset, count)?;
        Ok(self.cache.try_get(&key))
    }

    /// C# `TryPrefetch`: caches the file (priority [`SourceReadPriority::Preload`]) unless [`Self::can_cache`] says
    /// it never could, in which case nothing is opened at all and the result is `Ok(false)`.
    pub fn try_prefetch(&self, path: &Path) -> Result<bool, SourceBytesError> {
        let key = Self::create_key(path)?;
        if !self.can_cache(key.source_length) {
            return Ok(false);
        }
        self.get_or_read_key(key, SourceReadPriority::Preload)?;
        Ok(true)
    }

    /// C# `CanCache`: `length <= capacity_bytes && length <= ARRAY_MAX_LENGTH` (negative lengths are `true`, as in C#).
    pub fn can_cache(&self, length: i64) -> bool {
        length <= self.capacity_bytes && length <= ARRAY_MAX_LENGTH
    }

    /// C# `CanCacheRange`: `count >= 0 && count <= capacity_bytes && count <= ARRAY_MAX_LENGTH`.
    pub fn can_cache_range(&self, count: i64) -> bool {
        count >= 0 && count <= self.capacity_bytes && count <= ARRAY_MAX_LENGTH
    }

    /// C# `Clear`: under the publish gate, drop every per-path version, bump the generation, clear the LRU.
    pub fn clear(&self) {
        let _gate = lock(&self.publish_gate);
        // RV-I16: versions cleared BEFORE the bump, so a reader that sees the new generation never reads a pre-clear
        // version that would make its publish fail spuriously.
        lock(&self.path_versions).clear();
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.cache.clear();
    }

    /// C# `Evict(string path)`: invalidates in-flight reads of this path (per-path version, or a global generation
    /// bump once [`MAX_TRACKED_PATH_VERSIONS`] distinct paths are tracked) and removes every cached key whose path is
    /// OrdinalIgnoreCase-equal to `Path.GetFullPath(path)`. `Err` only for a path `GetFullPath` rejects.
    pub fn evict(&self, path: &Path) -> Result<(), SourceBytesError> {
        let full = full_path(path)?;
        let version_key = os_ignore_case_key(full.as_os_str());
        let _gate = lock(&self.publish_gate);
        {
            let mut versions = lock(&self.path_versions);
            if versions.len() >= MAX_TRACKED_PATH_VERSIONS && !versions.contains_key(&version_key) {
                // Bound the map: fall back to the global invalidation `clear` uses (the dropped versions are no
                // longer needed, every in-flight read now fails its generation check).
                versions.clear();
                self.generation.fetch_add(1, Ordering::SeqCst);
            } else {
                let version = versions.entry(version_key.clone()).or_insert(0);
                *version = version.wrapping_add(1);
            }
        }
        self.cache
            .remove_where(|key| os_ignore_case_key(key.path.as_os_str()) == version_key);
        Ok(())
    }

    // ---- test seams (C# `internal ... ForTests`; `pub` + `doc(hidden)` so `crates/pr-imaging/tests` can use them) ----

    /// C# `BeforePublishForTests`: runs inside the publish gate between the version check and the `set`.
    #[doc(hidden)]
    pub fn set_before_publish_for_tests(&self, hook: Option<TestHook>) {
        *lock(&self.before_publish_for_tests) = hook;
    }

    /// C# `AfterReadForTests`: runs after the read loop, just before the length / last-write re-check (outside the gate).
    #[doc(hidden)]
    pub fn set_after_read_for_tests(&self, hook: Option<TestHook>) {
        *lock(&self.after_read_for_tests) = hook;
    }

    /// C# `AfterCacheMissForTests`: runs on the calling thread between the cache miss and the in-flight lookup.
    #[doc(hidden)]
    pub fn set_after_cache_miss_for_tests(&self, hook: Option<TestHook>) {
        *lock(&self.after_cache_miss_for_tests) = hook;
    }

    /// C# `PathVersionCountForTests` (RV-I16): number of per-path eviction versions still tracked.
    #[doc(hidden)]
    pub fn path_version_count_for_tests(&self) -> usize {
        lock(&self.path_versions).len()
    }

    /// C# `LastReadManagedThreadId`: the thread that performed the most recent real read (not a cache hit).
    #[doc(hidden)]
    pub fn last_read_thread_id_for_tests(&self) -> Option<ThreadId> {
        *lock(&self.last_read_thread_id)
    }

    // ---- private (C# private methods; signatures are a suggestion, the porter may reshape them) ----

    /// C# `GetOrRead(RangeKey key, SourceReadPriority priority)`: cache hit, else the shared in-flight read.
    fn get_or_read_key(&self, key: RangeKey, priority: SourceReadPriority) -> Result<SourceBytes, SourceBytesError> {
        // Past the .NET array limit no read can succeed (a whole file above 2 GB).
        if key.count > ARRAY_MAX_LENGTH {
            return Err(SourceBytesError::ArgumentOutOfRange { param_name: "key" });
        }
        if let Some(cached) = self.cache.try_get(&key) {
            return Ok(cached);
        }
        run_hook(&self.after_cache_miss_for_tests);
        let generation = self.generation.load(Ordering::SeqCst);
        let path_version = self.path_version_of(key.path());
        // The entry keeps the values of the caller that CREATED it (C# factory closure, trap 2).
        let entry = Arc::clone(lock(&self.in_flight).entry(key.clone()).or_insert_with(|| {
            Arc::new(InFlight {
                generation,
                path_version,
                priority,
                result: OnceLock::new(),
            })
        }));
        // Removes the entry after the attempt (also on error or panic), only if it is still this one.
        let _remove = InFlightRemoval {
            in_flight: &self.in_flight,
            key: &key,
            entry: &entry,
        };
        // Joiners block in `get_or_init` (C# Lazy ExecutionAndPublication). Double-checked: a late caller that missed
        // the cache before the winner published shares the published bytes instead of reading again (trap 3).
        entry
            .result
            .get_or_init(|| match self.cache.try_get(&key) {
                Some(published) => Ok(published),
                None => self.read_and_cache(&key, entry.generation, entry.path_version, entry.priority, true),
            })
            .clone()
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
        *lock(&self.last_read_thread_id) = Some(thread::current().id());
        let mut stream = self
            .source_reader
            .open_source(key.path(), priority, SOURCE_READ_BUFFER_SIZE)?;
        let stream_length = stream.seek(SeekFrom::End(0))?;
        if i64::try_from(stream_length).ok() != Some(key.source_length) {
            return Err(file_changed_while_reading(&self.texts, key.path()));
        }
        // `0 <= count <= ARRAY_MAX_LENGTH` and `offset >= 0` hold for every key that reaches here.
        let count =
            usize::try_from(key.count).map_err(|_| SourceBytesError::ArgumentOutOfRange { param_name: "key" })?;
        let offset =
            u64::try_from(key.offset).map_err(|_| SourceBytesError::ArgumentOutOfRange { param_name: "offset" })?;
        // One zeroed allocation, filled in place (no second copy, trap 6).
        let mut bytes: SourceBytes = std::iter::repeat_n(0u8, count).collect();
        stream.seek(SeekFrom::Start(offset))?;
        {
            let buffer = Arc::get_mut(&mut bytes)
                .ok_or_else(|| io::Error::other("source bytes buffer is shared before it was filled"))?;
            // C# `while (read < count) { n = stream.Read(..); if (n == 0) throw EndOfStream; }`: every reader error
            // (Interrupted included, .NET has no EINTR retry) propagates on the first failure.
            let mut filled = 0;
            while filled < buffer.len() {
                let read = stream.read(&mut buffer[filled..])?;
                if read == 0 {
                    return Err(unexpected_eof(&self.texts, key.path()));
                }
                filled += read;
            }
        }
        run_hook(&self.after_read_for_tests);
        let current = file_stat(key.path())?;
        if i64::try_from(current.length).unwrap_or(i64::MAX) != key.source_length
            || current.last_write_utc.ticks() != key.last_write_utc_ticks
        {
            return Err(file_changed_while_reading(&self.texts, key.path()));
        }
        if !publish {
            return Ok(bytes);
        }
        {
            let _gate = lock(&self.publish_gate);
            if generation == self.generation.load(Ordering::SeqCst) && path_version == self.path_version_of(key.path())
            {
                run_hook(&self.before_publish_for_tests);
                self.cache.set(key.clone(), Arc::clone(&bytes));
            }
        }
        drop(stream);
        Ok(bytes)
    }

    /// C# `CreateKey(path)`: full path + `new FileInfo(full)` length and last write ticks; offset 0, count = length.
    fn create_key(path: &Path) -> Result<RangeKey, SourceBytesError> {
        let full = full_path(path)?;
        let stat = file_stat(&full)?;
        let length = i64::try_from(stat.length).unwrap_or(i64::MAX);
        Ok(RangeKey {
            path: full.into_os_string(),
            source_length: length,
            last_write_utc_ticks: stat.last_write_utc.ticks(),
            offset: 0,
            count: length,
        })
    }

    /// C# `CreateRangeKey(path, length, lastWriteUtcTicks, offset, count)`: full path, no file access.
    fn create_range_key(
        path: &Path,
        length: i64,
        last_write_utc_ticks: i64,
        offset: i64,
        count: i32,
    ) -> Result<RangeKey, SourceBytesError> {
        Ok(RangeKey {
            path: full_path(path)?.into_os_string(),
            source_length: length,
            last_write_utc_ticks,
            offset,
            count: i64::from(count),
        })
    }
}

impl SourceBytesCache {
    /// `_pathVersions.GetValueOrDefault(path)` (OrdinalIgnoreCase), read without the publish gate.
    fn path_version_of(&self, full: &Path) -> i32 {
        lock(&self.path_versions)
            .get(&os_ignore_case_key(full.as_os_str()))
            .copied()
            .unwrap_or(0)
    }
}

/// Removes an in-flight entry when the attempt ends (also on an error or a panic), but only if the map still holds
/// THIS entry (C# `TryRemove(KeyValuePair)`): a newer attempt for the same key is left alone.
struct InFlightRemoval<'a> {
    in_flight: &'a Mutex<HashMap<RangeKey, Arc<InFlight>>>,
    key: &'a RangeKey,
    entry: &'a Arc<InFlight>,
}

impl Drop for InFlightRemoval<'_> {
    fn drop(&mut self) {
        let mut map = lock(self.in_flight);
        if map
            .get(self.key)
            .is_some_and(|current| Arc::ptr_eq(current, self.entry))
        {
            map.remove(self.key);
        }
    }
}

/// Takes a lock; a poisoned lock is recovered (the C# `lock` simply continues).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs a test seam hook. The hook is cloned out of its mutex first: it may call `evict`, `clear` or a setter.
fn run_hook(slot: &Mutex<Option<TestHook>>) {
    let hook = lock(slot).clone();
    if let Some(hook) = hook {
        hook();
    }
}

/// `Path.GetFullPath(path)` with the C# exception mapping (`ArgumentException` / `PathTooLongException`).
///
/// A path that is valid Unicode goes through `pr_core::text::dotnet_path::get_full_path` (the W2-00 port). A Windows
/// path holding an unpaired UTF-16 surrogate cannot go through that `&str` API without losing the surrogate (a C#
/// `string` keeps it and the file opens), so the same rules run on the `OsStr` itself: a NUL is the
/// `ArgumentException`, the NT prefix `\??\` is returned unchanged, anything else is `std::path::absolute`
/// (`GetFullPathNameW`, like .NET). Known gap: the 8.3 short-name expansion .NET applies to a full path containing `~`
/// is not applied to such a path (pr-core's expansion takes `&str`).
fn full_path(path: &Path) -> Result<PathBuf, SourceBytesError> {
    let argument = || SourceBytesError::Argument { param_name: "path" };
    // ERROR_FILENAME_EXCED_RANGE, as `pr_core::io::physical` maps `PathError::TooLong`.
    let too_long = || SourceBytesError::Io(Arc::new(io::Error::from_raw_os_error(206)));
    if let Some(text) = path.to_str() {
        return get_full_path(text).map(PathBuf::from).map_err(|err| match err {
            PathError::Argument => argument(),
            PathError::TooLong => too_long(),
        });
    }
    // Not valid Unicode, so neither empty nor only spaces: only the NUL rule of `GetFullPath` can apply.
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.contains(&0) {
        return Err(argument());
    }
    if bytes.starts_with(br"\??\") {
        return Ok(path.to_path_buf());
    }
    std::path::absolute(path).map_err(|_| too_long())
}

/// OrdinalIgnoreCase key of a full path: [`ignore_case_key`] on every Unicode run, the bytes of an unpaired surrogate
/// (WTF-8 on Windows) kept verbatim. Two paths share a key exactly when C# `OrdinalIgnoreCase` calls them equal (an
/// unpaired surrogate only equals itself); for a Unicode path it is the bytes of `ignore_case_key(path)`.
fn os_ignore_case_key(path: &OsStr) -> Vec<u8> {
    let mut key = Vec::with_capacity(path.len());
    for chunk in path.as_encoded_bytes().utf8_chunks() {
        key.extend_from_slice(ignore_case_key(chunk.valid()).as_bytes());
        key.extend_from_slice(chunk.invalid());
    }
    key
}

/// C# `new FileInfo(full)` then `.Length` / `.LastWriteTimeUtc` (no link following).
///
/// A missing entry, a missing folder, a drive that is not ready, or a directory is the C# `FileNotFoundException`
/// ("Could not find file '{full}'."). Any other failure of the attribute query (a name with `?`, `*` or `|` is
/// ERROR_INVALID_NAME, access denied, ...) is the `IOException` `FileInfo.Length` throws: the OS message followed by
/// `" : '{full}'."`, with the `io::ErrorKind` of that OS error.
fn file_stat(full: &Path) -> Result<FileStat, SourceBytesError> {
    let unicode = full.to_str().is_some();
    if unicode && let Some(stat) = PhysicalFileSystem.get_file_stat(full) {
        return Ok(stat);
    }
    // `new FileInfo(path)` trims one trailing separator; `components()` drops it (the full path has no `.` parts).
    let trimmed: PathBuf = full.components().collect();
    match fs::symlink_metadata(&trimmed) {
        // `get_file_stat` cannot take a path that is not valid Unicode: its stat is answered here. For any other path
        // `get_file_stat` already said "missing or a directory".
        Ok(metadata) if !unicode && !is_directory(&metadata) => Ok(FileStat {
            length: metadata.len(),
            last_write_utc: last_write_of(&metadata),
        }),
        Ok(_) => Err(could_not_find_file(full)),
        Err(err) if is_not_found(&err) => Err(could_not_find_file(full)),
        Err(err) => Err(win32_io_error(&err, full)),
    }
}

/// .NET `FileSystem.FillAttributeInfo(returnErrorOnNotFound: false)`: the errors that mean "does not exist".
fn is_not_found(err: &io::Error) -> bool {
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PATH_NOT_FOUND: i32 = 3;
    const ERROR_NOT_READY: i32 = 21;
    match err.raw_os_error() {
        Some(code) => matches!(code, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_NOT_READY),
        None => err.kind() == io::ErrorKind::NotFound,
    }
}

#[cfg(windows)]
fn is_directory(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0
}

#[cfg(not(windows))]
fn is_directory(metadata: &Metadata) -> bool {
    metadata.is_dir()
}

/// Last write time from the FILETIME (100 ns, UTC), as `pr_core::io::physical` reads it.
#[cfg(windows)]
fn last_write_of(metadata: &Metadata) -> DateTimeUtc {
    use std::os::windows::fs::MetadataExt;
    DateTimeUtc::from_filetime(metadata.last_write_time()).unwrap_or(DateTimeUtc::MIN)
}

#[cfg(not(windows))]
fn last_write_of(metadata: &Metadata) -> DateTimeUtc {
    metadata
        .modified()
        .ok()
        .and_then(DateTimeUtc::from_system_time)
        .unwrap_or(DateTimeUtc::MIN)
}

/// .NET `Win32Marshal.GetExceptionForWin32Error(code, path)` default case: `"{OS message} : '{path}'."`. The kind is
/// the OS error's own kind (ERROR_INVALID_NAME is not `NotFound`).
fn win32_io_error(err: &io::Error, full: &Path) -> SourceBytesError {
    let text = err.to_string();
    let message = match err.raw_os_error() {
        Some(code) => text
            .strip_suffix(&format!(" (os error {code})"))
            .unwrap_or(&text)
            .to_owned(),
        None => text,
    };
    io::Error::new(err.kind(), format!("{message} : '{}'.", full.display())).into()
}

/// C# `FileNotFoundException` of `new FileInfo(full).Length`.
fn could_not_find_file(full: &Path) -> SourceBytesError {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("Could not find file '{}'.", full.display()),
    )
    .into()
}

/// C# `UserFacingError.Localized(new IOException("File changed while reading: {path}"),
/// () => Tr.ErrIoFileChangedWhileReading(path))`; the sentence comes from the injected texts (D41 / P-24).
fn file_changed_while_reading(texts: &Arc<dyn SourceBytesTexts>, path: &Path) -> SourceBytesError {
    let shown = path.display().to_string();
    let texts = Arc::clone(texts);
    let argument = shown.clone();
    localized(
        io::ErrorKind::Other,
        format!("File changed while reading: {shown}"),
        Arc::new(move || texts.file_changed_while_reading(&argument)),
    )
}

/// C# `UserFacingError.Localized(new EndOfStreamException("Unexpected EOF while reading {path}"),
/// () => Tr.ErrIoUnexpectedEof(path))`.
fn unexpected_eof(texts: &Arc<dyn SourceBytesTexts>, path: &Path) -> SourceBytesError {
    let shown = path.display().to_string();
    let texts = Arc::clone(texts);
    let argument = shown.clone();
    localized(
        io::ErrorKind::UnexpectedEof,
        format!("Unexpected EOF while reading {shown}"),
        Arc::new(move || texts.unexpected_eof(&argument)),
    )
}

/// An `io::Error` of `kind` whose `Display` is the English `message` and which carries the UI sentence `text`.
fn localized(kind: io::ErrorKind, message: String, text: UserText) -> SourceBytesError {
    io::Error::new(kind, UserFacingIo { message, text }).into()
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
        Self { cache }
    }

    /// C# `Cache`: `None` when the cache is disabled.
    pub fn cache(&self) -> Option<&Arc<SourceBytesCache>> {
        self.cache.as_ref()
    }
}
