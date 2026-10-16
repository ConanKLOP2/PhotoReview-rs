//! Port of `PhotoReview.Imaging/Caching/ThumbnailCache.cs` (C# baseline d92d6aae).
//!
//! Thread-safe thumbnail cache for the Preview-mode first frame (PARITY I-09, D-07). The source image is never
//! modified and never fully decoded here: a hit comes from RAM (byte-bounded LRU, 1 GiB) or from a PNG already in the
//! disk cache (`<dir>\<sha256 hex>.png`, written by someone else: this type only READS the disk cache); a miss reads the
//! JPEG's embedded EXIF thumbnail (W4-02b) and nothing else. RAW sources skip the embedded reader.
//!
//! Concurrency (docs/adr/R0003): no async runtime. [`ThumbnailCache::get`] returns at once with a
//! [`ThumbnailRequest`]: `Ready` (RAM hit or a failed key build, C# `Task.FromResult` / `Task.FromException`) or
//! `Pending` (a shared load, C# `Lazy<Task>` in `_inFlight`). The shared load runs on a worker started by the
//! [`LoadSpawner`]; every caller that asks for the same key while it runs joins it. C# "the caller cancels its wait"
//! is "the caller drops its [`PendingThumbnail`]": the shared load goes on for the others (INV-2: clear/dispose bumps
//! the generation so a late result is returned but never cached).
//!
//! Layering (P-22): pure. The WIC PNG decode of a disk entry (`WicCacheImageReader.Decode`) is the
//! [`CacheImageReader`] seam (implemented in `pr_platform_win::wic::cache_image`, W4-11e); the embedded thumbnail
//! backend is W4-02b's [`EmbeddedThumbnailBackend`] (implemented by W4-11c).
//!
//! Target path: `crates/pr-imaging/src/cache/thumbnails.rs` (lead adds `pub mod cache;` to `lib.rs` and
//! `pub mod thumbnails;` to `cache/mod.rs`).

use std::any::Any;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use pr_core::abstractions::{Clock, FileStat, FileSystem, Log, NullLog, SystemClock};
use pr_core::caching::lru::BoundedLruCache;
use pr_core::catalog::types::is_raw_path;
use pr_core::diagnostics::perf::{self, PhotoReviewPerf};
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{PathError, get_full_path};

use crate::cache::disk_store::{DiskCacheStore, delete_stale_temp_files, is_cache_entry_failure, try_delete};
use crate::decode_types::{DecodeBox, DecodedImage, DecodedImageOptions, DecodedImageView, PlatformImage};
use crate::decoder_traits::{DecodeError, PlatformImageCodec};
use crate::embedded_thumbnail::{BlankPath, EmbeddedThumbnailBackend, try_read};
use crate::pixels::{PixelBuffer, PixelLayout};
use pr_core::model::enums::DecoderBackend;

// D32: the seam of C# `WicCacheImageReader.Decode`. Its WIC implementation is W4-11e (`pr_platform_win::wic::cache_image`).
/// C# `WicCacheImageInfo(int SourceWidth, int SourceHeight, bool Downscaled)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheImageInfo {
    pub source_width: i32,
    pub source_height: i32,
    pub downscaled: bool,
}

/// C# `WicCacheImageReader.Decode(encoded, layout, box, expectedWidth, expectedHeight, out info)`.
pub trait CacheImageReader: Send + Sync {
    /// `layout: None` = by frame format (opaque WIC formats -> `Bgr32`, everything else -> `Pbgra32`). Never
    /// upscales; Fant scaler; `InvalidData` for empty payload, zero/oversized dimensions, a header mismatch or more
    /// than `i32::MAX` output bytes; `OutOfMemory` for WIC `E_OUTOFMEMORY`; `Com` for every other WIC failure.
    fn decode(
        &self,
        encoded: &[u8],
        layout: Option<PixelLayout>,
        decode_box: DecodeBox,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<(PixelBuffer, CacheImageInfo), DecodeError>;
}

pub use pr_core::session::store::sha256;

// ---- Constants (C# values; mutation-tested at their boundaries) -------------------------------------------------

/// C# `MaxThumbnailWidth`: the decode box width of a disk entry (`DecodeBox(800, 0)`) and the `|800|` in the key stamp.
pub const MAX_THUMBNAIL_WIDTH: i32 = 800;
/// C# `DefaultMaxDiskBytes`: 1 GiB disk quota.
pub const DEFAULT_MAX_DISK_BYTES: i64 = 1024 * 1024 * 1024;
/// C# constructor default `maxRamBytes`: 1 GiB.
pub const DEFAULT_MAX_RAM_BYTES: i64 = 1024 * 1024 * 1024;
/// C# `StartupTempCleanupMaxFiles`.
pub const STARTUP_TEMP_CLEANUP_MAX_FILES: i32 = 5000;
/// The disk store's search pattern (C# `new DiskCacheStore(_diskDirectory, "*.png", ...)`).
pub const DISK_CACHE_SEARCH_PATTERN: &str = "*.png";
/// C# `WicCacheImageReader.MaxDecodedBytes` (`int.MaxValue`): a longer disk entry is refused before allocating.
pub const MAX_DISK_ENTRY_BYTES: u64 = i32::MAX as u64;
/// C# `new FileStream(..., bufferSize: 1, FileOptions.SequentialScan)`: unbuffered sequential read.
const DISK_ENTRY_READ_BUFFER_SIZE: usize = 1;
/// Thread name of a shared load started by the default spawner.
const LOAD_THREAD_NAME: &str = "PhotoReview.ThumbnailLoad";
/// Thread name of the start-up temp sweep started by the default spawner.
const CLEANUP_THREAD_NAME: &str = "PhotoReview.ThumbnailCleanup";

// ---- Public types ----------------------------------------------------------------------------------------------

/// A cached thumbnail; shared between the RAM cache and every caller (C# reference identity: `Arc::ptr_eq`).
pub type ThumbnailImage = Arc<dyn DecodedImageView>;

/// What a load yields: `Ok(None)` = no thumbnail (RAW source, no embedded thumbnail, not a JPEG).
pub type ThumbnailResult = Result<Option<ThumbnailImage>, ThumbnailError>;

/// C# `Func<string, CancellationToken, Task<IDecodedImage?>> embeddedThumbnailReader`: called on the load worker
/// with the FULL source path and the cache's dispose token. An `Err` propagates to every waiter and is not cached.
pub type EmbeddedThumbnailReaderFn = Arc<dyn Fn(&str, &CancelToken) -> ThumbnailResult + Send + Sync>;

/// Starts `work` on another thread (C# `Task.Run`). `Err` = the worker could not be started. The default spawns a
/// thread named `PhotoReview.ThumbnailLoad` per load (and one `PhotoReview.ThumbnailCleanup` at construction).
pub type LoadSpawner = Arc<dyn Fn(Box<dyn FnOnce() + Send + 'static>) -> io::Result<()> + Send + Sync>;

/// Why a thumbnail request failed. Cloneable because one shared load delivers the same outcome to every waiter.
#[derive(Debug, Clone)]
pub enum ThumbnailError {
    /// C# `ObjectDisposedException`, thrown synchronously by `GetAsync` after `Dispose`.
    Disposed,
    /// C# `ArgumentException.ThrowIfNullOrWhiteSpace(sourcePath)`, thrown synchronously (before the disposed check).
    BlankPath,
    /// `Path.GetFullPath` refused the source path (C# `ArgumentException` / `PathTooLongException`): a faulted request.
    InvalidPath(PathError),
    /// The stat for the key failed (C# `new FileInfo(path).Length`: `FileNotFoundException` for a missing file or a
    /// directory): a faulted request.
    Stat(Arc<io::Error>),
    /// C# `OperationCanceledException` from the dispose token (load started after `Dispose`, or a step checked it).
    Canceled,
    /// A disk-entry decode failure that is NOT a cache-entry failure (out of memory, cancellation, programming error).
    Decode(Arc<DecodeError>),
    /// The embedded-thumbnail reader failed (C#: whatever it threw, e.g. `IOException("transient")`).
    Reader(Arc<dyn Error + Send + Sync>),
    /// The load worker could not be started.
    Spawn(Arc<io::Error>),
    /// The load panicked (R0003: a worker reports a panic as an error; C# a faulted task).
    Panicked(String),
}

impl fmt::Display for ThumbnailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disposed => f.write_str(
                "Cannot access a disposed object.\nObject name: 'PhotoReview.Imaging.Caching.ThumbnailCache'.",
            ),
            Self::BlankPath => f.write_str(
                "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'sourcePath')",
            ),
            Self::InvalidPath(error) => fmt::Display::fmt(error, f),
            Self::Stat(error) => fmt::Display::fmt(error, f),
            Self::Canceled => f.write_str("The operation was canceled."),
            Self::Decode(error) => fmt::Display::fmt(error, f),
            Self::Reader(error) => fmt::Display::fmt(error, f),
            Self::Spawn(error) => write!(f, "The thumbnail load could not be started: {error}"),
            Self::Panicked(message) => write!(f, "The thumbnail load panicked: {message}"),
        }
    }
}

impl Error for ThumbnailError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPath(error) => Some(error),
            Self::Stat(error) | Self::Spawn(error) => Some(error.as_ref()),
            Self::Decode(error) => Some(error.as_ref()),
            Self::Reader(error) => Some(error.as_ref()),
            Self::Disposed | Self::BlankPath | Self::Canceled | Self::Panicked(_) => None,
        }
    }
}

/// C# constructor argument failures (`ArgumentOutOfRangeException.ThrowIfNegativeOrZero`, then the store's
/// `ArgumentException.ThrowIfNullOrWhiteSpace(directory)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailCacheConfigError {
    /// `maxRamBytes <= 0`.
    MaxRamBytes(i64),
    /// `maxDiskBytes <= 0`.
    MaxDiskBytes(i64),
    /// The disk directory is empty or white space (C# `DiskCacheStore` ctor `ArgumentException`, param `directory`).
    BlankDiskDirectory,
}

impl fmt::Display for ThumbnailCacheConfigError {
    /// The .NET texts of `ThrowIfNegativeOrZero` / `ThrowIfNullOrWhiteSpace`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxRamBytes(value) => write!(
                f,
                "maxRamBytes ('{value}') must be a non-negative and non-zero value. (Parameter 'maxRamBytes')\nActual value was {value}."
            ),
            Self::MaxDiskBytes(value) => write!(
                f,
                "maxDiskBytes ('{value}') must be a non-negative and non-zero value. (Parameter 'maxDiskBytes')\nActual value was {value}."
            ),
            Self::BlankDiskDirectory => f.write_str(
                "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'directory')",
            ),
        }
    }
}

impl Error for ThumbnailCacheConfigError {}

/// Required collaborators (C# `platformCodec` plus the statics the C# class reaches directly).
pub struct ThumbnailCacheServices {
    /// C-02 codec (required, WP-06): makes the platform image of a disk/embedded thumbnail.
    pub platform_codec: Arc<dyn PlatformImageCodec>,
    /// WIC PNG decode of a disk entry (C# `WicCacheImageReader`).
    pub cache_image_reader: Arc<dyn CacheImageReader>,
    /// Backend of the DEFAULT embedded reader (C# `EmbeddedThumbnailReader.TryRead`); unused when
    /// [`ThumbnailCacheOptions::embedded_reader`] is set.
    pub embedded_backend: Arc<dyn EmbeddedThumbnailBackend>,
    /// Stat of the source, existence and shared read of the disk entry.
    pub fs: Arc<dyn FileSystem>,
}

impl fmt::Debug for ThumbnailCacheServices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThumbnailCacheServices").finish_non_exhaustive()
    }
}

/// The optional C# constructor arguments.
pub struct ThumbnailCacheOptions {
    /// C# `diskDirectory`. No default here: the App passes `AppPaths::thumbnail_cache_dir` (`%LOCALAPPDATA%\
    /// PhotoReviewRs\...\thumbnails`, R0002); the C# default `%LOCALAPPDATA%\PhotoReview\thumbnails` is the C# app's.
    pub disk_directory: PathBuf,
    /// C# `maxRamBytes` (default [`DEFAULT_MAX_RAM_BYTES`]); must be > 0.
    pub max_ram_bytes: i64,
    /// C# `maxDiskBytes` (default [`DEFAULT_MAX_DISK_BYTES`]); must be > 0.
    pub max_disk_bytes: i64,
    /// C# `log` (default `NullLog`).
    pub log: Arc<dyn Log>,
    /// `DateTime.UtcNow` of the startup temp sweep (and the disk store's access stamps).
    pub clock: Arc<dyn Clock>,
    /// C# `embeddedThumbnailReader`; `None` = the default (W4-02b `try_read` on the load worker).
    pub embedded_reader: Option<EmbeddedThumbnailReaderFn>,
    /// Worker starter; `None` = one named thread per job.
    pub spawner: Option<LoadSpawner>,
}

impl ThumbnailCacheOptions {
    /// The C# defaults for everything but the directory: 1 GiB RAM, 1 GiB disk, `NullLog`, `SystemClock`, default
    /// reader and spawner.
    pub fn new(disk_directory: impl Into<PathBuf>) -> Self {
        Self {
            disk_directory: disk_directory.into(),
            max_ram_bytes: DEFAULT_MAX_RAM_BYTES,
            max_disk_bytes: DEFAULT_MAX_DISK_BYTES,
            log: Arc::new(NullLog),
            clock: Arc::new(SystemClock),
            embedded_reader: None,
            spawner: None,
        }
    }
}

impl fmt::Debug for ThumbnailCacheOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThumbnailCacheOptions")
            .field("disk_directory", &self.disk_directory)
            .field("max_ram_bytes", &self.max_ram_bytes)
            .field("max_disk_bytes", &self.max_disk_bytes)
            .finish_non_exhaustive()
    }
}

/// The answer of [`ThumbnailCache::get`] (C# the returned `Task<IDecodedImage?>`).
pub enum ThumbnailRequest {
    /// Already decided: a RAM hit (`Ok(Some)`), or a failed key build (`Err(InvalidPath | Stat)`).
    Ready(ThumbnailResult),
    /// Joined or started a shared load.
    Pending(PendingThumbnail),
}

impl ThumbnailRequest {
    /// Blocks until the outcome is known (C# `await`). Never call on the UI thread.
    pub fn wait(self) -> ThumbnailResult {
        match self {
            Self::Ready(result) => result,
            Self::Pending(pending) => pending.wait(),
        }
    }
}

impl fmt::Debug for ThumbnailRequest {
    /// `Ready(Ok(Some(WxH)))`, `Ready(Ok(None))`, `Ready(Err(..))` or `Pending(key)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready(Ok(Some(image))) => {
                write!(f, "Ready(Ok(Some({}x{})))", image.pixel_width(), image.pixel_height())
            }
            Self::Ready(Ok(None)) => f.write_str("Ready(Ok(None))"),
            Self::Ready(Err(error)) => write!(f, "Ready(Err({error:?}))"),
            Self::Pending(pending) => write!(f, "Pending({})", pending.key),
        }
    }
}

/// One caller's view of a shared load (C# one `AwaitAndCacheAsync` call). Dropping it without waiting is C#
/// "this caller cancelled its wait": the load continues for the other callers and this caller caches nothing.
pub struct PendingThumbnail {
    load: Arc<SharedLoad>,
    owner: Arc<Inner>,
    key: String,
    /// `_cacheGeneration` read right after this caller joined or inserted the shared load (C# `Volatile.Read` at the
    /// top of `AwaitAndCacheAsync`, after `GetOrAdd`); for the caller that starts the load, before it starts. The
    /// result is put in RAM only while the cache is not disposed and still at it.
    generation: i64,
}

impl PendingThumbnail {
    /// Blocks until the shared load ends, then (for `Ok(Some)`) caches it in RAM under the lifecycle lock when
    /// `!disposed && generation == current`. Returns the image either way.
    pub fn wait(self) -> ThumbnailResult {
        let outcome = {
            let state = lock(&self.load.state);
            let state = self
                .load
                .done
                .wait_while(state, |state| state.outcome.is_none())
                .unwrap_or_else(PoisonError::into_inner);
            state.outcome.clone()
        };
        match outcome {
            Some(result) => self.observe(result),
            // Unreachable: `wait_while` only returns once the outcome is set.
            None => Err(ThumbnailError::Panicked(
                "the shared load ended without an outcome".to_owned(),
            )),
        }
    }

    /// As [`wait`](Self::wait) but gives up after `timeout`: `None` = still running (the handle stays usable).
    pub fn wait_timeout(&self, timeout: Duration) -> Option<ThumbnailResult> {
        let outcome = {
            let state = lock(&self.load.state);
            let (state, _) = self
                .load
                .done
                .wait_timeout_while(state, timeout, |state| state.outcome.is_none())
                .unwrap_or_else(PoisonError::into_inner);
            state.outcome.clone()
        };
        outcome.map(|result| self.observe(result))
    }

    /// The outcome if the load already ended (same RAM-caching rule), else `None`. Never blocks.
    pub fn try_result(&self) -> Option<ThumbnailResult> {
        let outcome = lock(&self.load.state).outcome.clone();
        outcome.map(|result| self.observe(result))
    }

    /// C# the tail of `AwaitAndCacheAsync`: an image is put in RAM only while the cache is not disposed and still at
    /// this caller's generation (checked under the lifecycle lock, INV-2). Returns `result` unchanged.
    fn observe(&self, result: ThumbnailResult) -> ThumbnailResult {
        if let Ok(Some(image)) = &result {
            let lifecycle = lock(&self.owner.lifecycle);
            if !lifecycle.disposed && self.generation == lifecycle.generation {
                self.owner.ram.set(self.key.clone(), Arc::clone(image));
            }
        }
        result
    }
}

impl Drop for PendingThumbnail {
    /// The last handle of a load that has not ended takes the load out of the in-flight map, so the next `get` for the
    /// key starts a new load instead of joining one nobody waits for.
    // DIVERGENCE: C# keeps an abandoned `Lazy` in `_inFlight` until it completes (a later `GetAsync` joins it). Here a
    // load that every caller dropped is forgotten at once (it still runs to completion, its result is discarded:
    // nobody observes it, so nothing is cached either way). Needed for the RV-I10 replacement test
    // `dropped_caller_then_load_fails_removes_in_flight_entry`, which would otherwise race the worker's own removal.
    fn drop(&mut self) {
        let mut in_flight = lock(&self.owner.in_flight);
        let abandoned = {
            let mut state = lock(&self.load.state);
            state.waiters = state.waiters.saturating_sub(1);
            state.waiters == 0 && state.outcome.is_none()
        };
        if abandoned {
            remove_if_current(&mut in_flight, &self.key, &self.load);
        }
    }
}

impl fmt::Debug for PendingThumbnail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingThumbnail")
            .field("key", &self.key)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// C# `ThumbnailCache`. Dropping it disposes it ([`ThumbnailCache::dispose`]).
pub struct ThumbnailCache {
    inner: Arc<Inner>,
    /// Done when the start-up stale temp-file sweep finished (C# `StartupCleanup` test seam).
    startup_cleanup: Arc<Signal>,
}

impl ThumbnailCache {
    /// C# constructor: validates `max_ram_bytes` then `max_disk_bytes` (`> 0`), builds the disk store
    /// (`*.png`, `max_disk_bytes`), starts the stale temp sweep (W4-09 `delete_stale_temp_files(dir,
    /// clock.utc_now(), 5000, log)`) on a worker (off the caller's thread), and builds the RAM LRU (capacity
    /// `max_ram_bytes`, size = `estimated_bytes`).
    ///
    /// # Errors
    /// [`ThumbnailCacheConfigError`] for a non-positive limit or a blank disk directory.
    pub fn new(
        services: ThumbnailCacheServices,
        options: ThumbnailCacheOptions,
    ) -> Result<Self, ThumbnailCacheConfigError> {
        let ThumbnailCacheOptions {
            disk_directory,
            max_ram_bytes,
            max_disk_bytes,
            log,
            clock,
            embedded_reader,
            spawner,
        } = options;
        if max_ram_bytes <= 0 {
            return Err(ThumbnailCacheConfigError::MaxRamBytes(max_ram_bytes));
        }
        if max_disk_bytes <= 0 {
            return Err(ThumbnailCacheConfigError::MaxDiskBytes(max_disk_bytes));
        }
        let disk_store = DiskCacheStore::new(
            disk_directory.clone(),
            DISK_CACHE_SEARCH_PATTERN,
            max_disk_bytes,
            Some(Arc::clone(&log)),
            Some(Arc::clone(&clock)),
        )
        .map_err(|_| ThumbnailCacheConfigError::BlankDiskDirectory)?;
        let ram = BoundedLruCache::new(max_ram_bytes, |image: &ThumbnailImage| image.estimated_bytes())
            .map_err(|_| ThumbnailCacheConfigError::MaxRamBytes(max_ram_bytes))?;

        let ThumbnailCacheServices {
            platform_codec,
            cache_image_reader,
            embedded_backend,
            fs,
        } = services;
        let embedded_reader = embedded_reader
            .unwrap_or_else(|| default_embedded_reader(Arc::clone(&fs), embedded_backend, Arc::clone(&platform_codec)));
        let cleanup_spawner = spawner
            .clone()
            .unwrap_or_else(|| named_thread_spawner(CLEANUP_THREAD_NAME));
        let spawner = spawner.unwrap_or_else(default_spawner);

        // Atomic-write temp files orphaned by a killed process; off the caller's thread (C# `Task.Run`).
        let startup_cleanup = Arc::new(Signal::default());
        let sweep_done = SetOnDrop(Arc::clone(&startup_cleanup));
        let sweep_directory = disk_directory.clone();
        let sweep_log = Arc::clone(&log);
        let now = clock.utc_now();
        let sweep = Box::new(move || {
            let _done = sweep_done;
            delete_stale_temp_files(&sweep_directory, now, STARTUP_TEMP_CLEANUP_MAX_FILES, Some(&*sweep_log));
        });
        // A failed spawn drops `sweep` (and so marks the signal done); OQ8: logged, not run inline.
        if let Err(error) = cleanup_spawner(sweep) {
            log.error(
                &format!("Stale temp file cleanup failed: {}", disk_directory.display()),
                Some(&error),
            );
            startup_cleanup.set();
        }

        Ok(Self {
            inner: Arc::new(Inner {
                disk_directory,
                log,
                fs,
                codec: platform_codec,
                cache_image_reader,
                embedded_reader,
                spawner,
                disk_store,
                ram,
                in_flight: Mutex::new(HashMap::new()),
                dispose_token: CancelToken::new(),
                lifecycle: Mutex::new(Lifecycle::default()),
            }),
            startup_cleanup,
        })
    }

    /// C# `DiskDirectory`.
    pub fn disk_directory(&self) -> &Path {
        &self.inner.disk_directory
    }

    /// C# `GetAsync(sourcePath, knownStat)` (Q-R29 option C). `known_stat` is a stat of `source_path` the caller just
    /// took; `None` = stat it here (through the file system). The key names exactly that Length/LastWriteUtc version.
    ///
    /// Order: blank path -> `Err(BlankPath)`; disposed -> `Err(Disposed)` (both synchronous, C# throws); full path and
    /// key (failure -> `Ready(Err)`); RAM hit -> `Ready(Ok(Some))` with a `ram` perf event; else join or start the
    /// shared load for the key -> `Pending`.
    ///
    /// # Errors
    /// Only [`ThumbnailError::BlankPath`] and [`ThumbnailError::Disposed`]; everything else is in the request.
    pub fn get(&self, source_path: &str, known_stat: Option<FileStat>) -> Result<ThumbnailRequest, ThumbnailError> {
        if source_path.chars().all(char::is_whitespace) {
            return Err(ThumbnailError::BlankPath);
        }
        let perf_log = PhotoReviewPerf::log();
        let perf_t0 = if perf_log.is_enabled() { perf::timestamp() } else { 0 };
        let dispose_token = {
            let lifecycle = lock(&self.inner.lifecycle);
            if lifecycle.disposed {
                return Err(ThumbnailError::Disposed);
            }
            self.inner.dispose_token.clone()
        };

        let full_path = match get_full_path(source_path) {
            Ok(full_path) => full_path,
            Err(error) => return Ok(ThumbnailRequest::Ready(Err(ThumbnailError::InvalidPath(error)))),
        };
        let stat = match known_stat {
            Some(stat) => stat,
            None => match self.inner.fs.get_file_stat(Path::new(&full_path)) {
                Some(stat) => stat,
                None => {
                    let error = io::Error::new(io::ErrorKind::NotFound, format!("Could not find file '{full_path}'."));
                    return Ok(ThumbnailRequest::Ready(Err(ThumbnailError::Stat(Arc::new(error)))));
                }
            },
        };
        let key = build_key(&full_path, stat.length, stat.last_write_utc.ticks());

        if let Some(cached) = self.inner.ram.try_get(&key) {
            if perf_t0 != 0 {
                perf_log.thumb_end(
                    PhotoReviewPerf::nav_context(),
                    &perf::path_id(&full_path),
                    "ram",
                    perf::ms(perf_t0),
                );
            }
            return Ok(ThumbnailRequest::Ready(Ok(Some(cached))));
        }

        // C# `_inFlight.GetOrAdd`: join the running load for the key, or insert a new one. This caller's handle is
        // counted under the map lock so a concurrent drop of the last other handle cannot forget the load meanwhile.
        let (load, inserted) = {
            let mut in_flight = lock(&self.inner.in_flight);
            let (load, inserted) = match in_flight.get(&key) {
                Some(load) => (Arc::clone(load), false),
                None => {
                    let load = Arc::new(SharedLoad::default());
                    in_flight.insert(key.clone(), Arc::clone(&load));
                    (load, true)
                }
            };
            lock(&load.state).waiters += 1;
            (load, inserted)
        };
        // C# `Volatile.Read(ref _cacheGeneration)` at the top of `AwaitAndCacheAsync`: after `GetOrAdd`, and (for the
        // starter) before `lazy.Value` starts the load.
        let generation = lock(&self.inner.lifecycle).generation;
        let pending = PendingThumbnail {
            load: Arc::clone(&load),
            owner: Arc::clone(&self.inner),
            key: key.clone(),
            generation,
        };
        if inserted {
            self.start_load(full_path, key, load, dispose_token);
        }
        Ok(ThumbnailRequest::Pending(pending))
    }

    /// C# `lazy.Value` + the `ContinueWith` that removes the in-flight entry: runs [`load_or_create`] on a worker,
    /// forgets the entry if it is still this load, then publishes the outcome.
    fn start_load(&self, full_path: String, key: String, load: Arc<SharedLoad>, token: CancelToken) {
        // The nav context is thread-local: capture it on the caller's thread (C# reads its AsyncLocal there too).
        let perf_context = PhotoReviewPerf::log().is_enabled().then(|| LoadPerf {
            t0: perf::timestamp(),
            nav: PhotoReviewPerf::nav_context(),
            path_id: perf::path_id(&full_path),
        });
        let inner = Arc::clone(&self.inner);
        let job_load = Arc::clone(&load);
        let job_key = key.clone();
        let job = Box::new(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                load_or_create(&inner, &full_path, &job_key, &token, perf_context.as_ref())
            }))
            .unwrap_or_else(|payload| Err(ThumbnailError::Panicked(panic_message(payload.as_ref()))));
            finish_load(&inner, &job_key, &job_load, outcome);
        });
        if let Err(error) = (self.inner.spawner)(job) {
            finish_load(&self.inner, &key, &load, Err(ThumbnailError::Spawn(Arc::new(error))));
        }
    }

    /// C# `ClearMemory`: under the lifecycle lock, bump the generation and clear the RAM cache. In-flight loads keep
    /// running; their results are returned but not cached.
    pub fn clear_memory(&self) {
        let mut lifecycle = lock(&self.inner.lifecycle);
        lifecycle.generation = lifecycle.generation.wrapping_add(1);
        self.inner.ram.clear();
    }

    /// C# `ClearDisk`: bump the generation (RAM is NOT cleared), then `disk_store.clear_directory()`; a listing
    /// failure is logged `Thumbnail disk cache clear failed: {dir}` and swallowed. Works after `dispose` too.
    pub fn clear_disk(&self) {
        {
            let mut lifecycle = lock(&self.inner.lifecycle);
            lifecycle.generation = lifecycle.generation.wrapping_add(1);
        }
        if let Err(error) = self.inner.disk_store.clear_directory() {
            self.inner.log.error(
                &format!(
                    "Thumbnail disk cache clear failed: {}",
                    self.inner.disk_directory.display()
                ),
                Some(&error),
            );
        }
    }

    /// C# `Dispose`: idempotent. First call: disposed = true, bump the generation, clear RAM (all under the lock),
    /// then cancel the dispose token. Loads already running finish; their results are returned but not cached.
    pub fn dispose(&self) {
        {
            let mut lifecycle = lock(&self.inner.lifecycle);
            if lifecycle.disposed {
                return;
            }
            lifecycle.disposed = true;
            lifecycle.generation = lifecycle.generation.wrapping_add(1);
            self.inner.ram.clear();
        }
        self.inner.dispose_token.cancel();
    }

    /// Test seam (C# `await cache.StartupCleanup`): waits for the start-up temp sweep; `true` when it finished.
    #[doc(hidden)]
    pub fn wait_startup_cleanup(&self, timeout: Duration) -> bool {
        let done = lock(&self.startup_cleanup.done);
        let (done, _) = self
            .startup_cleanup
            .changed
            .wait_timeout_while(done, timeout, |done| !*done)
            .unwrap_or_else(PoisonError::into_inner);
        *done
    }
}

impl Drop for ThumbnailCache {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl fmt::Debug for ThumbnailCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThumbnailCache").finish_non_exhaustive()
    }
}

// ---- Pure helpers (golden-tested) -------------------------------------------------------------------------------

/// C# `BuildKey` stamp: `{full_path}|{length}|{last_write_ticks}|800|orient=1`. `full_path` exactly as given (case and
/// Unicode form kept; NOT upper-cased, unlike the preview key); numbers in invariant decimal.
pub fn build_stamp(full_path: &str, length: u64, last_write_ticks: i64) -> String {
    format!("{full_path}|{length}|{last_write_ticks}|{MAX_THUMBNAIL_WIDTH}|orient=1")
}

/// C# `BuildKey(path, length, lastWriteUtc)`: lowercase hex of SHA-256 over the UTF-8 [`build_stamp`] (64 chars).
/// The key is the persisted cache file name: byte-identical to C# (D-07).
pub fn build_key(full_path: &str, length: u64, last_write_ticks: i64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = sha256(build_stamp(full_path, length, last_write_ticks).as_bytes());
    let mut key = String::with_capacity(digest.len() * 2);
    for byte in digest {
        key.push(char::from(HEX[usize::from(byte >> 4)]));
        key.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    key
}

/// `Path.Combine(diskDirectory, key + ".png")`.
pub fn cache_file_path(disk_directory: &Path, key: &str) -> PathBuf {
    disk_directory.join(format!("{key}.png"))
}

/// C# `DecodeDiskThumbnail(path, codec)` (WP-04): one shared read of the cached PNG (`fs.open_read_shared(path, 1)`,
/// ReadWrite|Delete sharing, SequentialScan, INV-8); a length of 0 or above [`MAX_DISK_ENTRY_BYTES`] is
/// `InvalidData("Thumbnail cache entry has an invalid length.")` before allocating; `read_exact`; decode with
/// `layout None, DecodeBox(800, 0), expected 0x0` (fit, never upscale); `codec.image_from_pixels` (the pixels are
/// dropped on failure); `DecodedImage` with `estimated_bytes = w * h * 4` (i64), `downscaled = info.downscaled`,
/// `orientation 1`, `WicDirect`, `original = info.source_width/height`. A `DecodedImage` guard failure is
/// `DecodeError::InvalidArgument` (C# `ArgumentException`, a cache-entry failure).
///
/// # Errors
/// Any [`DecodeError`]; the caller decides with `is_cache_entry_failure`.
pub fn decode_disk_thumbnail(
    path: &Path,
    fs: &dyn FileSystem,
    reader: &dyn CacheImageReader,
    codec: &dyn PlatformImageCodec,
) -> Result<DecodedImage, DecodeError> {
    // The handle is closed at the end of this block, before the decode (INV-8).
    let encoded = {
        let mut stream = fs.open_read_shared(path, DISK_ENTRY_READ_BUFFER_SIZE)?;
        let length = stream.seek(SeekFrom::End(0))?;
        if length == 0 || length > MAX_DISK_ENTRY_BYTES {
            return Err(DecodeError::InvalidData {
                message: "Thumbnail cache entry has an invalid length.".to_owned(),
            });
        }
        stream.seek(SeekFrom::Start(0))?;
        let length = usize::try_from(length).map_err(|_| DecodeError::InvalidData {
            message: "Thumbnail cache entry has an invalid length.".to_owned(),
        })?;
        let mut encoded = vec![0_u8; length];
        stream.read_exact(&mut encoded)?;
        encoded
    };

    let (pixels, info) = reader.decode(&encoded, None, DecodeBox::new(MAX_THUMBNAIL_WIDTH, 0), 0, 0)?;
    let (width, height) = (pixels.width(), pixels.height());
    let platform_image: PlatformImage = codec.image_from_pixels(pixels)?;
    DecodedImage::new(
        platform_image,
        width,
        height,
        i64::from(width) * i64::from(height) * 4,
        DecodedImageOptions {
            downscaled: info.downscaled,
            orientation: 1,
            actual_backend: DecoderBackend::WicDirect,
            original_width: info.source_width,
            original_height: info.source_height,
            ..DecodedImageOptions::default()
        },
    )
    .map_err(|error| DecodeError::InvalidArgument {
        message: error.to_string(),
    })
}

// ---- Private state ---------------------------------------------------------------------------------------------

/// Everything the cache and its pending handles share.
struct Inner {
    disk_directory: PathBuf,
    log: Arc<dyn Log>,
    fs: Arc<dyn FileSystem>,
    codec: Arc<dyn PlatformImageCodec>,
    cache_image_reader: Arc<dyn CacheImageReader>,
    embedded_reader: EmbeddedThumbnailReaderFn,
    spawner: LoadSpawner,
    disk_store: DiskCacheStore,
    /// C# `BoundedLruCache<string, IDecodedImage>` (OrdinalIgnoreCase is moot: keys are lowercase hex).
    ram: BoundedLruCache<String, ThumbnailImage>,
    /// C# `_inFlight`; an entry is removed by the load that put it there (pointer-equal), right before it
    /// publishes, or by the drop of its last handle.
    in_flight: Mutex<HashMap<String, Arc<SharedLoad>>>,
    /// C# `_disposeCts.Token`; cancelled by `dispose` after the lifecycle lock is released.
    dispose_token: CancelToken,
    /// C# `_lifecycleGate`.
    lifecycle: Mutex<Lifecycle>,
}

/// State guarded by the lifecycle lock.
#[derive(Debug, Default)]
struct Lifecycle {
    /// C# `_cacheGeneration` (`long`, wrapping, D3).
    generation: i64,
    disposed: bool,
}

/// One shared load (C# `Lazy<Task<IDecodedImage?>>`): set once, then every waiter wakes.
#[derive(Default)]
struct SharedLoad {
    state: Mutex<LoadState>,
    done: Condvar,
}

/// The part of a [`SharedLoad`] guarded by its lock.
#[derive(Default)]
struct LoadState {
    /// `None` while the load runs; then the outcome every waiter observes (never consumed).
    outcome: Option<ThumbnailResult>,
    /// Live [`PendingThumbnail`] handles of this load.
    waiters: usize,
}

/// Perf context captured by the caller that starts a load (the nav context is thread-local in Rust; C# reads its
/// AsyncLocal before the first await, on the caller's thread).
#[derive(Debug, Clone)]
struct LoadPerf {
    t0: i64,
    nav: i64,
    path_id: String,
}

/// A one-shot done flag (start-up sweep).
#[derive(Debug, Default)]
struct Signal {
    done: Mutex<bool>,
    changed: Condvar,
}

impl Signal {
    fn set(&self) {
        *lock(&self.done) = true;
        self.changed.notify_all();
    }
}

/// Marks the signal done when dropped: after the sweep ran, when it panicked, or when its job was dropped unrun.
struct SetOnDrop(Arc<Signal>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.set();
    }
}

/// Takes a lock; a poisoned lock is recovered (the C# `lock` simply continues).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// C# `_inFlight.TryRemove(KeyValuePair(key, lazy))`: removes the entry only while it is still `load`.
fn remove_if_current(in_flight: &mut HashMap<String, Arc<SharedLoad>>, key: &str, load: &Arc<SharedLoad>) {
    if in_flight.get(key).is_some_and(|current| Arc::ptr_eq(current, load)) {
        in_flight.remove(key);
    }
}

/// Ends a shared load: forgets the in-flight entry if it is still this load, THEN publishes the outcome to every
/// waiter. C# registers the `TryRemove` continuation (`ExecuteSynchronously`) before any caller awaits the task, so it
/// runs before a waiter resumes: a caller that saw a failure and asks again always starts a new load
/// (`FailedLoad_IsNotCached`). A `get` in between starts a new load too, which is the C# outcome as well.
fn finish_load(inner: &Inner, key: &str, load: &Arc<SharedLoad>, outcome: ThumbnailResult) {
    remove_if_current(&mut lock(&inner.in_flight), key, load);
    lock(&load.state).outcome = Some(outcome);
    load.done.notify_all();
}

/// The text of a panic payload (`&str` or `String`), else a fixed text.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "the thumbnail load panicked".to_owned()
    }
}

/// The shared load body (C# `LoadOrCreateAsync`), run on the worker. See the contract, semantics 4.
fn load_or_create(
    inner: &Inner,
    full_path: &str,
    key: &str,
    token: &CancelToken,
    perf: Option<&LoadPerf>,
) -> ThumbnailResult {
    token.check().map_err(|_| ThumbnailError::Canceled)?;
    let cache_path = cache_file_path(&inner.disk_directory, key);
    if inner.fs.file_exists(&cache_path) {
        // C# `Task.Run(.., token)`: a cancelled token is a cancellation, not a damaged entry.
        token.check().map_err(|_| ThumbnailError::Canceled)?;
        match decode_disk_thumbnail(&cache_path, &*inner.fs, &*inner.cache_image_reader, &*inner.codec) {
            Ok(image) => {
                inner.disk_store.note_accessed(&cache_path);
                if let Some(perf) = perf {
                    thumb_end(perf, "disk");
                }
                return Ok(Some(Arc::new(image)));
            }
            Err(error) if is_cache_entry_failure(&error) => {
                inner.log.error(
                    &format!("Disk thumbnail read failed: {}", cache_path.display()),
                    Some(&error),
                );
                try_delete(&cache_path, Some(&*inner.log));
            }
            Err(error) => return Err(ThumbnailError::Decode(Arc::new(error))),
        }
    }

    // A RAW has no EXIF-thumbnail APP1 to read here; the preview is its first frame. No perf event.
    if is_raw_path(full_path) {
        return Ok(None);
    }
    let embedded = (inner.embedded_reader)(full_path, token)?;
    if let Some(perf) = perf {
        thumb_end(perf, if embedded.is_some() { "embedded" } else { "none" });
    }
    Ok(embedded)
}

/// C# `PhotoReviewPerf.Log.ThumbEnd(perfNav, perfPathId, source, Ms(perfT0))` with the starter's context.
fn thumb_end(perf: &LoadPerf, source: &str) {
    PhotoReviewPerf::log().thumb_end(perf.nav, &perf.path_id, source, perf::ms(perf.t0));
}

/// The default embedded reader (C# `DefaultReadEmbeddedThumbnailAsync`): `token.check()` (C# `Task.Run(..., token)`
/// + `ThrowIfCancellationRequested`) then W4-02b `try_read(path, fs, backend, codec)`.
fn default_embedded_reader(
    fs: Arc<dyn FileSystem>,
    backend: Arc<dyn EmbeddedThumbnailBackend>,
    codec: Arc<dyn PlatformImageCodec>,
) -> EmbeddedThumbnailReaderFn {
    Arc::new(move |path: &str, token: &CancelToken| {
        token.check().map_err(|_| ThumbnailError::Canceled)?;
        match try_read(Path::new(path), &*fs, &*backend, &*codec) {
            Ok(Some(image)) => Ok(Some(Arc::new(image) as ThumbnailImage)),
            Ok(None) => Ok(None),
            Err(BlankPath) => Err(ThumbnailError::InvalidPath(PathError::Argument)),
        }
    })
}

/// The default [`LoadSpawner`]: one named thread per job.
fn default_spawner() -> LoadSpawner {
    named_thread_spawner(LOAD_THREAD_NAME)
}

/// A [`LoadSpawner`] that starts one thread named `name` per job.
fn named_thread_spawner(name: &'static str) -> LoadSpawner {
    Arc::new(move |work: Box<dyn FnOnce() + Send + 'static>| {
        thread::Builder::new().name(name.to_owned()).spawn(work).map(drop)
    })
}
