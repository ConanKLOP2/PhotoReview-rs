//! Port of `PhotoReview.Benchmarking/BenchmarkImageExecutor.cs` (C# baseline d92d6aae, 239 lines / 14 064 bytes): the
//! production-compatible image executor of a benchmark run, `BenchmarkProfileScope` and the internal
//! `WindowedPreloadTarget`. Contract: `contracts/W7-03b.md` (draft: `contracts/drafts-w7/W7-03b/contract.md`).
//! PARITY: W-06 (BenchmarkWindow runs profiles through this executor), X-benchmark (new rows, see the contract).
//!
//! Every decode goes through a REAL `PreviewImageService` / `PreloadScheduler` pair (same RAM cache, disk-cache
//! probe/persist path and preload-priority scheduling as the viewer), with a private scratch disk-cache directory per
//! executor. R0003: no async runtime. C# `Task`/`await` become blocking calls and handles
//! (`PreviewHandle`, `RunHandle`, [`TeardownHandle`]); nothing here may be called from a UI thread.
//!
//! Intentional deviations from C# (the observable benchmark output is unchanged): (a) blocking handles instead of
//! `Task`; (b) `dispose` is explicitly idempotent and there is no `Drop`; (c) the scratch parent is
//! `PhotoReviewRs-Benchmark-Cache` (R0002) instead of `PhotoReview-Benchmark-Cache`; (d) `decoder: None` is an error
//! (`NoDecoder`) because the WIC decoder lives in `pr_platform_win` and the host passes it; (e) the constructor takes
//! the payload codec too; (f) `physical_memory_bytes` defaults to 0 (unknown), the production host must pass the real
//! value; (g) `evict_for_cold_decode` could return `PreviewError::SourceBytes`, unreachable here.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::error::Error;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pr_core::abstractions::{Log, MemoryProbe};
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::file_log::FileLog;
use pr_core::diagnostics::metrics::{ReviewMetrics, ReviewMetricsSnapshot};
use pr_core::model::enums::LoadingMode;
use pr_core::settings::perf_options;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{PathError, get_full_path};
use pr_core::text::ignore_case_key;
use pr_imaging::cache::preview_file::PreviewPayloadCodec;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::preload::target::{DelegateMemoryProbe, KeyError, PreloadTarget};
use pr_imaging::preload::{PreloadScheduler, PreloadSchedulerDeps, RunHandle};
use pr_imaging::preload_order::{PreloadOptions, PreloadWindow};
use pr_imaging::service::handle::PreviewHandle;
use pr_imaging::service::{
    PreviewError, PreviewImageService, PreviewResult, PreviewServiceBuildError, PreviewServiceConfig,
};

pub use crate::models::BenchmarkProfile;

// ---- constants ----------------------------------------------------------------------------------------------------

/// Directory (under the system temp dir) that holds the per-executor scratch directories. NOT the C# name
/// (`PhotoReview-Benchmark-Cache`): a Rust run never shares a scratch parent with a C# run (R0002, proposal P-W7-03b-1).
pub const SCRATCH_PARENT_NAME: &str = "PhotoReviewRs-Benchmark-Cache";

/// C# bound of the background teardown wait for the prune pass (`TimeSpan.FromMinutes(2)`).
pub const PRUNE_WAIT_TIMEOUT: Duration = Duration::from_secs(120);

/// C# `hasHeadroom` parameter: answers "is memory load below this maximum (0..1)?".
pub type HasHeadroomFn = Arc<dyn Fn(f64) -> bool + Send + Sync>;

// ---- errors -------------------------------------------------------------------------------------------------------

/// Why [`BenchmarkImageExecutor::new`] failed (the exceptions the C# constructor can throw).
#[derive(Debug)]
pub enum ExecutorError {
    /// `Path.GetFullPath(files[i])` threw (`WindowedPreloadTarget` index build): an empty/blank path or a path with `\0`.
    InvalidPath(PathError),
    /// `new PreviewImageService(..)` threw.
    Service(PreviewServiceBuildError),
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutorError::InvalidPath(error) => write!(f, "invalid benchmark file path: {error}"),
            ExecutorError::Service(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl Error for ExecutorError {}

impl From<PathError> for ExecutorError {
    fn from(error: PathError) -> Self {
        ExecutorError::InvalidPath(error)
    }
}

impl From<PreviewServiceBuildError> for ExecutorError {
    fn from(error: PreviewServiceBuildError) -> Self {
        ExecutorError::Service(error)
    }
}

// ---- construction options -----------------------------------------------------------------------------------------

/// The optional / Rust-only constructor inputs. `Default` = the C# defaults.
pub struct BenchmarkExecutorOptions {
    /// C# `hasHeadroom`: `Some(f)` = `DelegateMemoryProbe::new(f)` (tests inject a fixed answer); `None` = [`memory_probe`](Self::memory_probe).
    pub has_headroom: Option<HasHeadroomFn>,
    /// The OS memory check used when `has_headroom` is `None`. The production host passes `pr_platform_win::memory_probe::WindowsMemoryProbe`
    /// (this crate cannot name it: `pr-platform-win` depends on `pr-app`, which depends on this crate, D-W8B1-1). `None` = fail closed (no
    /// headroom, the same answer `WindowsMemoryProbe` gives when `GlobalMemoryStatusEx` fails).
    pub memory_probe: Option<Arc<dyn MemoryProbe>>,
    /// C# `decoder`: the decoder the preview service decodes with. C# `null` = "WIC Direct over the platform codec"; the
    /// WIC decoder lives in `pr_platform_win`, so the host passes it explicitly and `None` is an error
    /// ([`PreviewServiceBuildError::NoDecoder`]).
    pub decoder: Option<Arc<dyn ImageDecoder>>,
    /// Sink for the teardown messages (C# `FileLog.Default`). `None` = an adapter over `FileLog::default_instance()`.
    pub log: Option<Arc<dyn Log>>,
    /// Parent of the scratch directory. `None` = `std::env::temp_dir().join(SCRATCH_PARENT_NAME)`. Tests pass a
    /// `TempRoot` subdirectory so no scratch directory ever leaks into the real temp dir.
    pub scratch_parent: Option<PathBuf>,
    /// How long the background teardown waits for the prune pass. Default [`PRUNE_WAIT_TIMEOUT`]; a test seam.
    pub prune_wait_timeout: Duration,
    /// Physical RAM in bytes for the preview-service budget (`PreviewServiceConfig::physical_memory_bytes`). `<= 0` =
    /// unknown. The production host passes `ram_budget::get_physical_memory_bytes(..)`; tests pass a fixed value.
    pub physical_memory_bytes: i64,
}

impl Default for BenchmarkExecutorOptions {
    fn default() -> Self {
        Self {
            has_headroom: None,
            memory_probe: None,
            decoder: None,
            log: None,
            scratch_parent: None,
            prune_wait_timeout: PRUNE_WAIT_TIMEOUT,
            physical_memory_bytes: 0,
        }
    }
}

impl fmt::Debug for BenchmarkExecutorOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkExecutorOptions")
            .field("scratch_parent", &self.scratch_parent)
            .field("prune_wait_timeout", &self.prune_wait_timeout)
            .finish_non_exhaustive()
    }
}

// ---- teardown handle ----------------------------------------------------------------------------------------------

/// Completion of the background scratch-directory teardown (C# `Task TeardownBackgroundTask`). Clones share the state.
/// Never fails: a teardown problem is logged and the handle still completes (the C# task observes its own failures).
#[derive(Clone)]
pub struct TeardownHandle {
    state: Arc<(Mutex<bool>, Condvar)>,
}

impl fmt::Debug for TeardownHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TeardownHandle").finish_non_exhaustive()
    }
}

impl TeardownHandle {
    /// C# `Task.CompletedTask`: nothing pending (before `dispose`, and what an executor reports until then).
    pub fn completed() -> Self {
        Self {
            state: Arc::new((Mutex::new(true), Condvar::new())),
        }
    }

    /// A pending teardown; `finish` completes it.
    pub(crate) fn pending() -> Self {
        Self {
            state: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// Marks the teardown finished and wakes every waiter.
    pub(crate) fn finish(&self) {
        let (flag, changed) = &*self.state;
        *lock(flag) = true;
        changed.notify_all();
    }

    /// Whether the teardown has finished.
    pub fn is_complete(&self) -> bool {
        *lock(&self.state.0)
    }

    /// Blocks until the teardown has finished. Tests use [`Self::wait_timeout`] (hang guard, AGENTS.md "Tests").
    pub fn wait(&self) {
        let (flag, changed) = &*self.state;
        let _done = changed
            .wait_while(lock(flag), |done| !*done)
            .unwrap_or_else(PoisonError::into_inner);
    }

    /// Blocks up to `timeout`; `true` when the teardown finished (C# `WaitAsync(timeout)` that did not throw
    /// `TimeoutException`).
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let (flag, changed) = &*self.state;
        let (done, _) = changed
            .wait_timeout_while(lock(flag), timeout, |done| !*done)
            .unwrap_or_else(PoisonError::into_inner);
        *done
    }
}

/// Locks a mutex; a poisoned lock still holds consistent data here (plain flags and handles).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- BenchmarkProfileScope ----------------------------------------------------------------------------------------

/// Applies a profile's per-run process state (detailed logging) around a benchmark run (C# static class
/// `BenchmarkProfileScope`).
#[derive(Debug)]
pub struct BenchmarkProfileScope;

impl BenchmarkProfileScope {
    /// C# `ApplyLogging(profile, getEnabled, setEnabled, getWriteCount)`.
    ///
    /// Order (golden pins it): `previous = get_enabled()`; `set_enabled(profile.detailed_logging)` (ALWAYS called, even
    /// when the value is unchanged); `writes_after_apply = get_write_count.map(|f| f())`. The returned scope's
    /// `dispose` restores: with a counter present and `get_write_count() != writes_after_apply` it does nothing (someone
    /// else changed the flag during the run, their choice wins), else `set_enabled(previous)`. The restore runs AT MOST
    /// ONCE however often `dispose`/`drop` is called (C# `Interlocked.Exchange(ref _restore, null)`).
    pub fn apply_logging<'a>(
        profile: &BenchmarkProfile,
        get_enabled: impl Fn() -> bool,
        set_enabled: impl Fn(bool) + 'a,
        get_write_count: Option<Box<dyn Fn() -> i32 + 'a>>,
    ) -> LoggingScope<'a> {
        let previous = get_enabled();
        set_enabled(profile.detailed_logging);
        let writes_after_apply = get_write_count.as_ref().map(|count| count());
        let restore: Box<dyn FnOnce() + 'a> = Box::new(move || {
            if let (Some(count), Some(after_apply)) = (&get_write_count, writes_after_apply) {
                if count() != after_apply {
                    // Someone else wrote the flag during the run: their choice wins.
                    return;
                }
            }
            set_enabled(previous);
        });
        LoggingScope { restore: Some(restore) }
    }
}

/// The disposable returned by [`BenchmarkProfileScope::apply_logging`] (C# private `Restore`). Dropping it disposes it.
pub struct LoggingScope<'a> {
    restore: Option<Box<dyn FnOnce() + 'a>>,
}

impl fmt::Debug for LoggingScope<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoggingScope")
            .field("pending", &self.restore.is_some())
            .finish()
    }
}

impl LoggingScope<'_> {
    /// Runs the restore once (idempotent).
    pub fn dispose(&mut self) {
        if let Some(restore) = self.restore.take() {
            restore();
        }
    }
}

impl Drop for LoggingScope<'_> {
    fn drop(&mut self) {
        self.dispose();
    }
}

// ---- WindowedPreloadTarget ----------------------------------------------------------------------------------------

/// Restricts preload to the profile's window around the navigation centre by reporting out-of-window paths as already
/// cached, so the scheduler never queues them (C# internal `WindowedPreloadTarget`). `full_folder` profiles are not
/// restricted. Public (and `#[doc(hidden)]`) only so integration tests and the golden test can reach it.
#[doc(hidden)]
pub struct WindowedPreloadTarget {
    inner: Arc<dyn PreloadTarget>,
    /// `ignore_case_key(get_full_path(file))` -> index in the file list; a later duplicate overwrites (C# indexer set).
    index_by_path: HashMap<String, usize>,
    full_folder: bool,
    next: i32,
    previous: i32,
    center: AtomicI32,
}

impl fmt::Debug for WindowedPreloadTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowedPreloadTarget")
            .field("full_folder", &self.full_folder)
            .field("next", &self.next)
            .field("previous", &self.previous)
            .finish_non_exhaustive()
    }
}

impl WindowedPreloadTarget {
    /// C# constructor. `Path.GetFullPath(files[i])` can throw => `Err(PathError)`.
    pub fn new(inner: Arc<dyn PreloadTarget>, files: &[String], profile: &BenchmarkProfile) -> Result<Self, PathError> {
        let mut index_by_path = HashMap::with_capacity(files.len());
        for (position, file) in files.iter().enumerate() {
            // A later duplicate (same path up to OrdinalIgnoreCase) overwrites the earlier one, like the C# indexer set.
            index_by_path.insert(ignore_case_key(&get_full_path(file)?), position);
        }
        Ok(Self {
            inner,
            index_by_path,
            full_folder: profile.full_folder,
            next: profile.next_window,
            previous: profile.previous_window,
            center: AtomicI32::new(0),
        })
    }

    /// C# `SetCenter` (`Volatile.Write`): release store.
    pub fn set_center(&self, center: i32) {
        self.center.store(center, Ordering::Release);
    }

    /// C# private `OutsideWindow(path)`. `false` when `full_folder` or `path` is not in the index (a path the profile
    /// does not know is never held back). Else `delta = index - center` and the result is `delta > next || -delta >
    /// previous` with C# UNCHECKED `int` arithmetic: use `wrapping_sub` / `wrapping_neg` on `i32`, NOT `i64` (golden
    /// `window-center-int-min-wraps`). The lookup key is `ignore_case_key(path)` of the path AS GIVEN (no
    /// `get_full_path`): a non-normalized spelling is simply "not in the index" (golden `window-unnormalized-by-path`).
    fn outside_window(&self, path: &str) -> bool {
        if self.full_folder {
            return false;
        }
        let Some(&position) = self.index_by_path.get(&ignore_case_key(path)) else {
            return false;
        };
        // C# unchecked `int` arithmetic: wrap, never widen.
        let position = i32::try_from(position).unwrap_or(i32::MAX);
        let delta = position.wrapping_sub(self.center.load(Ordering::Acquire));
        delta > self.next || delta.wrapping_neg() > self.previous
    }
}

impl PreloadTarget for WindowedPreloadTarget {
    /// `outside_window(path) || inner.try_get_cached_preview_for_path(path)` (short-circuit: the inner is not asked
    /// for an out-of-window path).
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
        self.outside_window(path) || self.inner.try_get_cached_preview_for_path(path)
    }

    /// `outside_window(key.path()) || inner.try_get_cached_preview(key)`.
    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        self.outside_window(key.path()) || self.inner.try_get_cached_preview(key)
    }

    /// Straight forward to the inner target (preloading is never windowed here; the scheduler's queue decides).
    fn preload(&self, path: &str, cancel: &CancelToken) -> Result<(), DecodeError> {
        self.inner.preload(path, cancel)
    }

    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
        self.inner.get_current_cache_key_for_path(path)
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        self.inner.get_current_cache_key(entry)
    }

    fn cache_count(&self) -> i32 {
        self.inner.cache_count()
    }

    fn cache_bytes(&self) -> i64 {
        self.inner.cache_bytes()
    }

    /// C# forwards the three default members too (`ActiveViewerDecodes`, `CachedPreviewBytes`, `HasDiskCachedPreview`):
    /// the trait defaults (0 / `None` / `false`) would silently disable the inner target's answers.
    fn active_viewer_decodes(&self) -> i32 {
        self.inner.active_viewer_decodes()
    }

    fn cached_preview_bytes(&self, key: &ImageCacheKey) -> Option<i64> {
        self.inner.cached_preview_bytes(key)
    }

    fn has_disk_cached_preview(&self, key: &ImageCacheKey) -> bool {
        self.inner.has_disk_cached_preview(key)
    }
}

// ---- BenchmarkImageExecutor ---------------------------------------------------------------------------------------

/// Production-compatible image executor for benchmark runs (C# `BenchmarkImageExecutor : IAsyncDisposable`).
///
/// `Send + Sync`: the workload runner (W7-03c) may decode from several threads. `dispose` is explicit and idempotent;
/// there is NO `Drop` (C# has no finalizer; a drop must never block a profile on a slow prune). An executor that is
/// dropped without `dispose` leaves its scratch directory for the OS temp cleaner, like a C# one that is not disposed.
pub struct BenchmarkImageExecutor {
    metrics: Arc<ReviewMetrics>,
    preview_service: Arc<PreviewImageService>,
    preload_scheduler: PreloadScheduler,
    disk_cache_directory: PathBuf,
    preload_target: Arc<WindowedPreloadTarget>,
    last_preload: Mutex<Option<RunHandle>>,
    teardown: Mutex<TeardownHandle>,
    log: Arc<dyn Log>,
    prune_wait_timeout: Duration,
    disposed: AtomicBool,
}

impl fmt::Debug for BenchmarkImageExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkImageExecutor")
            .field("disk_cache_directory", &self.disk_cache_directory)
            .finish_non_exhaustive()
    }
}

// Compile-time guard: the executor is shared across worker threads.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BenchmarkImageExecutor>();
    assert_send_sync::<TeardownHandle>();
};

/// A fresh 32-character lowercase hexadecimal name (C# `Guid.NewGuid().ToString("N")`). Unique per call within and
/// across processes (process id + clock + counter + `RandomState`); no new crate (`uuid`/`getrandom` are lead-only).
pub fn new_scratch_directory_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let process = std::process::id();
    let part = |salt: u64| {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u32(process);
        hasher.write_u128(nanos);
        hasher.write_u64(counter);
        hasher.write_u64(salt);
        hasher.finish()
    };
    format!("{:016x}{:016x}", part(0), part(1))
}

impl BenchmarkImageExecutor {
    /// C# constructor `(profile, files, platformCodec, hasHeadroom, decoder)`. In order:
    ///
    /// 1. `disk_cache_directory = scratch_parent.join(new_scratch_directory_name())` (the directory itself is created
    ///    lazily by the disk store; nothing is created here).
    /// 2. `metrics = Arc::new(ReviewMetrics::new())`; `is_original = profile.loading_mode == LoadingMode::Original`
    ///    captured BY VALUE once (C# captures the local, not the profile).
    /// 3. `PreviewServiceConfig::new(metrics, || is_original, width_only(|| profile.target_width()), platform_codec,
    ///    payload_codec)` with `capacity_bytes = perf_options::IMAGE_CACHE_CAPACITY_BYTES`, `disk_cache_directory =
    ///    Some(dir)`, `disable_disk_cache_override = Some(!profile.disk_cache)`, `decoder = options.decoder`,
    ///    `physical_memory_bytes = options.physical_memory_bytes`; everything else at the C# default (disk capacity 4 GiB).
    ///    `target_width()` is read on EVERY key build (the profile is captured as a value, not re-read).
    /// 4. `catalog_entries: Arc<[CatalogEntry]>` = `files.map(CatalogEntry::new)` (a blank path makes `CatalogEntry::new`
    ///    fail with `ArgumentError`, the C# `ArgumentException`: map it to `ExecutorError::InvalidPath(PathError::Argument)`, like the
    ///    `get_full_path` failure of step 5). The scheduler's `snapshot_entries` closure returns a clone of this `Arc` every time.
    /// 5. `preload_target = WindowedPreloadTarget::new(service as Arc<dyn PreloadTarget>, files, profile)?`.
    /// 6. Scheduler: `PreloadOptions { worker_count: profile.workers, memory_load_limit:
    ///    perf_options::PRELOAD_MEMORY_LOAD_LIMIT, reserve_bytes: profile.memory_reserve_bytes,
    ///    full_folder_threshold_bytes: if profile.full_folder { service.capacity_bytes() } else { 0 }, window:
    ///    PreloadWindow::DEFAULT }` (a non-`full_folder` profile must NEVER escalate to a whole-folder pass: threshold 0),
    ///    target = the windowed target, memory probe = `DelegateMemoryProbe` when `has_headroom` is `Some`, else the OS
    ///    probe. `PreloadSchedulerDeps { options: Some(..), log: None, ..Default }` (the scheduler logs nothing here).
    pub fn new(
        profile: &BenchmarkProfile,
        files: &[String],
        platform_codec: Arc<dyn PlatformImageCodec>,
        payload_codec: Arc<dyn PreviewPayloadCodec>,
        options: BenchmarkExecutorOptions,
    ) -> Result<Self, ExecutorError> {
        let scratch_parent = options
            .scratch_parent
            .unwrap_or_else(|| std::env::temp_dir().join(SCRATCH_PARENT_NAME));
        let disk_cache_directory = scratch_parent.join(new_scratch_directory_name());
        let metrics = Arc::new(ReviewMetrics::new());
        // Evaluated ONCE and captured by value (C# closes over the local, not the profile).
        let is_original = profile.loading_mode == LoadingMode::Original;
        let width_profile = profile.clone();

        let mut config = PreviewServiceConfig::new(
            Arc::clone(&metrics),
            Arc::new(move || is_original),
            PreviewServiceConfig::width_only(Arc::new(move || width_profile.target_width())),
            platform_codec,
            payload_codec,
        );
        config.capacity_bytes = perf_options::IMAGE_CACHE_CAPACITY_BYTES;
        config.disk_cache_directory = Some(disk_cache_directory.clone());
        // ALWAYS `Some`: the diagnostic variable must not turn the disk cache on or off in a benchmark.
        config.disable_disk_cache_override = Some(!profile.disk_cache);
        config.decoder = options.decoder;
        config.physical_memory_bytes = options.physical_memory_bytes;
        let preview_service = Arc::new(PreviewImageService::new(config)?);

        match Self::build_preload(
            profile,
            files,
            &preview_service,
            &metrics,
            options.has_headroom,
            options.memory_probe,
        ) {
            Ok((preload_target, preload_scheduler)) => Ok(Self {
                metrics,
                preview_service,
                preload_scheduler,
                disk_cache_directory,
                preload_target,
                last_preload: Mutex::new(None),
                teardown: Mutex::new(TeardownHandle::completed()),
                log: options.log.unwrap_or_else(|| Arc::new(DefaultFileLog) as Arc<dyn Log>),
                prune_wait_timeout: options.prune_wait_timeout,
                disposed: AtomicBool::new(false),
            }),
            Err(error) => {
                // Do not leak the service's persist workers when the construction fails half way.
                preview_service.shutdown_persist_workers();
                Err(error)
            }
        }
    }

    /// Steps 4 to 6 of [`Self::new`]: the catalog snapshot, the windowed target and the scheduler.
    fn build_preload(
        profile: &BenchmarkProfile,
        files: &[String],
        preview_service: &Arc<PreviewImageService>,
        metrics: &Arc<ReviewMetrics>,
        has_headroom: Option<HasHeadroomFn>,
        memory_probe: Option<Arc<dyn MemoryProbe>>,
    ) -> Result<(Arc<WindowedPreloadTarget>, PreloadScheduler), ExecutorError> {
        let entries = files
            .iter()
            .map(CatalogEntry::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ExecutorError::InvalidPath(PathError::Argument))?;
        let catalog_entries: Arc<[CatalogEntry]> = Arc::from(entries);

        let inner: Arc<dyn PreloadTarget> = Arc::clone(preview_service) as Arc<dyn PreloadTarget>;
        let preload_target = Arc::new(WindowedPreloadTarget::new(inner, files, profile)?);

        let preload_options = PreloadOptions {
            worker_count: profile.workers,
            memory_load_limit: perf_options::PRELOAD_MEMORY_LOAD_LIMIT,
            reserve_bytes: profile.memory_reserve_bytes,
            // A profile without `full_folder` must never escalate to a whole-folder pass, however small the folder.
            full_folder_threshold_bytes: if profile.full_folder {
                preview_service.capacity_bytes()
            } else {
                0
            },
            window: PreloadWindow::DEFAULT,
        };
        let memory_probe = choose_memory_probe(has_headroom, memory_probe);
        let target: Arc<dyn PreloadTarget> = Arc::clone(&preload_target) as Arc<dyn PreloadTarget>;
        let scheduler = PreloadScheduler::new(
            target,
            Arc::clone(metrics),
            Arc::new(move || Ok(Arc::clone(&catalog_entries))),
            memory_probe,
            PreloadSchedulerDeps {
                options: Some(preload_options),
                ..PreloadSchedulerDeps::default()
            },
        );
        Ok((preload_target, scheduler))
    }

    /// C# `EvictForColdDecode(path)`: `service.evict_cached_path(path, Some(&|normalized|
    /// scheduler.remove_preloaded_keys_for_path(normalized)))?` THEN `service.clear_disk()` (RAM first, then disk: a
    /// persisted copy must not satisfy the next "cold" decode). `Err` only for a path `get_full_path` refuses
    /// (`PreviewError::InvalidPath`; `PreviewError::SourceBytes` is unreachable: no source-bytes cache is configured).
    /// `clear_disk` is not reached after an `Err` (C# threw before it).
    pub fn evict_for_cold_decode(&self, path: &str) -> Result<(), PreviewError> {
        self.preview_service.evict_cached_path(
            path,
            Some(&|normalized: &str| self.preload_scheduler.remove_preloaded_keys_for_path(normalized)),
        )?;
        self.preview_service.clear_disk();
        Ok(())
    }

    /// C# `DecodeAsync(path, token)` minus the await: does everything C# does synchronously and returns the
    /// not-yet-awaited request.
    ///
    /// 1. `token.check()` => `Err(PreviewError::Canceled)` (C# `ThrowIfCancellationRequested`). Cancellation takes effect
    ///    only HERE, between decodes: the token is never passed to the service (C# `GetPreviewAsync` has no token).
    /// 2. `key = service.get_current_cache_key(path)` (`Err(CacheKeyError)` => `PreviewError::Key`; C# throws
    ///    synchronously from `DecodeAsync`).
    /// 3. preload hit: `if service.try_get_cached_preview(&key).is_some() && scheduler.try_consume_preloaded_key(&key)`
    ///    then `metrics.record_preload_hit()`. BOTH conditions, in this order, `&&` short-circuit: the key is consumed
    ///    only when the image really sits in RAM; a cold decode never counts a hit and never consumes.
    /// 4. `Ok(service.get_preview(path, &key))`.
    pub fn decode_async(&self, path: &str, token: &CancelToken) -> Result<PreviewHandle, PreviewError> {
        token.check().map_err(|_| PreviewError::Canceled)?;
        let key = self
            .preview_service
            .get_current_cache_key(path)
            .map_err(PreviewError::Key)?;
        // Both conditions, in this order: the key is consumed only when the image really sits in RAM.
        if self.preview_service.try_get_cached_preview(&key).is_some()
            && self.preload_scheduler.try_consume_preloaded_key(&key)
        {
            self.metrics.record_preload_hit();
        }
        Ok(self.preview_service.get_preview(path, &key))
    }

    /// `decode_async(..)?.wait()`.
    pub fn decode(&self, path: &str, token: &CancelToken) -> PreviewResult {
        self.decode_async(path, token)?.wait()
    }

    /// C# `Metrics`: `metrics.snapshot()` (cumulative since the executor was built).
    pub fn metrics(&self) -> ReviewMetricsSnapshot {
        self.metrics.snapshot()
    }

    /// C# `WarmPreloadAround(center)`: `preload_target.set_center(center)` BEFORE the kick, then
    /// `*last_preload = Some(scheduler.preload_around(center))` (the previous handle is overwritten, not awaited).
    pub fn warm_preload_around(&self, center: i32) {
        // The windowed target must know the new centre before the scheduler asks it about candidates.
        self.preload_target.set_center(center);
        let run = self.preload_scheduler.preload_around(center);
        *lock(&self.last_preload) = Some(run);
    }

    /// C# `IsPreviewCached(path)` test seam: `service.try_get_cached_preview(&service.get_current_cache_key(path)?)`
    /// `.is_some()`. `Err` for a missing file (C# threw).
    pub fn is_preview_cached(&self, path: &str) -> Result<bool, CacheKeyError> {
        let key = self.preview_service.get_current_cache_key(path)?;
        Ok(self.preview_service.try_get_cached_preview(&key).is_some())
    }

    /// C# `WhenPreloadSettledAsync()`: waits for the run started by the last `warm_preload_around` (returns at once when
    /// there was none). Clone the handle out of the mutex BEFORE waiting.
    pub fn when_preload_settled(&self) {
        let run = lock(&self.last_preload).clone();
        if let Some(run) = run {
            run.wait();
        }
    }

    /// C# `DiskCacheDirectory` test seam.
    pub fn disk_cache_directory(&self) -> &Path {
        &self.disk_cache_directory
    }

    /// C# `PreviewServiceForTests` test seam (tests reach `disk_store()` through it).
    pub fn preview_service_for_tests(&self) -> &Arc<PreviewImageService> {
        &self.preview_service
    }

    /// C# `TeardownBackgroundTask`: [`TeardownHandle::completed`] until `dispose` has run, then the handle of the
    /// background pass.
    pub fn teardown_background_task(&self) -> TeardownHandle {
        lock(&self.teardown).clone()
    }

    /// C# `DisposeAsync()` (blocking). Idempotent (a second call returns at once; the C# second call re-ran harmless
    /// no-ops). In order:
    ///
    /// 1. `scheduler.cancel()` FIRST, so the fire-and-forget last preload unwinds instead of running while the scheduler
    ///    is disposed.
    /// 2. wait for `last_preload` (clone out of the mutex; a failed background preload must not fail teardown: nothing
    ///    to catch in Rust, but a PANIC in the wait must not propagate either).
    /// 3. `scheduler.dispose()`.
    /// 4. `service.shutdown_persist_workers()` (blocking, unbounded like C#; only after every persist write has finished
    ///    is it safe to delete the directory). A panic here is caught (`catch_unwind`) and logged
    ///    `log.error("Benchmark persist workers failed to shut down cleanly", None)`; teardown continues.
    /// 5. start the background pass on a NAMED std thread and return WITHOUT waiting for it (the next profile must not be
    ///    blocked by a slow prune): `teardown = TeardownHandle::pending()`; the thread runs
    ///    [`delete_scratch_directory_after_prune`] and then `finish()` (also when the pass failed or panicked). A spawn
    ///    failure logs `Benchmark scratch directory could not be removed: {dir}` with the `io::Error`, completes the
    ///    handle and returns.
    pub fn dispose(&self) {
        if self.disposed.swap(true, Ordering::AcqRel) {
            return;
        }
        // 1. Cancel first so the fire-and-forget last preload unwinds instead of running against a disposed scheduler.
        self.preload_scheduler.cancel();
        // 2. A failed (or panicking) background preload must not fail the teardown.
        let last = lock(&self.last_preload).clone();
        if let Some(run) = last {
            let _ = panic::catch_unwind(AssertUnwindSafe(|| run.wait()));
        }
        // 3.
        self.preload_scheduler.dispose();
        // 4. Only after every persist write has finished is it safe to delete the scratch directory.
        if panic::catch_unwind(AssertUnwindSafe(|| self.preview_service.shutdown_persist_workers())).is_err() {
            self.log
                .error("Benchmark persist workers failed to shut down cleanly", None);
        }
        // 5. Prune wait and directory delete run on a background thread that this call does not await.
        let handle = TeardownHandle::pending();
        *lock(&self.teardown) = handle.clone();
        let service = Arc::clone(&self.preview_service);
        let dir = self.disk_cache_directory.clone();
        let timeout = self.prune_wait_timeout;
        let log = Arc::clone(&self.log);
        let thread_handle = handle.clone();
        let spawned = std::thread::Builder::new()
            .name("benchmark-scratch-teardown".to_owned())
            .spawn(move || {
                let _ = panic::catch_unwind(AssertUnwindSafe(|| {
                    delete_scratch_directory_after_prune(&service, &dir, timeout, log.as_ref());
                }));
                thread_handle.finish();
            });
        if let Err(error) = spawned {
            self.log.error(
                &format!(
                    "Benchmark scratch directory could not be removed: {}",
                    self.disk_cache_directory.display()
                ),
                Some(&error),
            );
            handle.finish();
        }
    }
}

/// C# `DeleteScratchDirectoryAfterPruneAsync`, the body of the teardown thread.
///
/// `settled = service.wait_for_prune(timeout)`. `false` => `log.error("Benchmark disk cache prune did not finish in
/// time; leaving scratch directory: {dir}", None)` and return WITHOUT deleting. `true` => when `dir.is_dir()`
/// `std::fs::remove_dir_all(dir)`; an `Err` (or a panic) is logged `log.error("Benchmark scratch directory could not be
/// removed: {dir}", Some(&err))` and swallowed (this task is never awaited by production code). `{dir}` is
/// `dir.display()`.
pub(crate) fn delete_scratch_directory_after_prune(
    service: &PreviewImageService,
    dir: &Path,
    timeout: Duration,
    log: &dyn Log,
) {
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), std::io::Error> {
        if !service.wait_for_prune(timeout) {
            log.error(
                &format!(
                    "Benchmark disk cache prune did not finish in time; leaving scratch directory: {}",
                    dir.display()
                ),
                None,
            );
            return Ok(());
        }
        if dir.is_dir() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }));
    let failure: Option<Box<dyn Error + 'static>> = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(Box::new(error)),
        Err(_) => Some("the teardown pass panicked".into()),
    };
    if let Some(error) = failure {
        log.error(
            &format!("Benchmark scratch directory could not be removed: {}", dir.display()),
            Some(error.as_ref()),
        );
    }
}

/// Picks the memory probe: `has_headroom` => `DelegateMemoryProbe::new(f)`, else the injected OS probe, else [`FailClosedMemoryProbe`].
pub(crate) fn choose_memory_probe(
    has_headroom: Option<HasHeadroomFn>,
    memory_probe: Option<Arc<dyn MemoryProbe>>,
) -> Arc<dyn MemoryProbe> {
    match (has_headroom, memory_probe) {
        (Some(has_headroom), _) => Arc::new(DelegateMemoryProbe::new(move |load: f64| has_headroom(load))),
        (None, Some(probe)) => probe,
        (None, None) => Arc::new(FailClosedMemoryProbe),
    }
}

/// No OS probe was injected: never any headroom, no snapshot (what `WindowsMemoryProbe` answers when the API fails).
struct FailClosedMemoryProbe;

impl MemoryProbe for FailClosedMemoryProbe {
    fn has_headroom(&self, _maximum_load: f64, _reserve_bytes: i64) -> bool {
        false
    }

    fn get_snapshot(&self) -> Option<pr_core::abstractions::MemorySnapshot> {
        None
    }
}

/// `Log` over the process-wide `FileLog::default_instance()` (C# `FileLog.Default`), resolved on every call.
struct DefaultFileLog;

impl Log for DefaultFileLog {
    fn enabled(&self) -> bool {
        FileLog::default_instance().enabled()
    }

    fn info(&self, message: &str) {
        FileLog::default_instance().info(message);
    }

    fn warn(&self, message: &str) {
        FileLog::default_instance().warn(message);
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        FileLog::default_instance().error(message, error);
    }
}

#[cfg(test)]
mod default_file_log_tests {
    use super::*;

    /// `enabled` mirrors the process-wide log in both states (kills `-> true` / `-> false`). Only the flag is toggled and then
    /// restored; no message is written, so the real app log is untouched. `info`/`warn`/`error` forward to the process-wide
    /// log, which is not injectable without writing the real app log, so they are not asserted here.
    #[test]
    fn enabled_mirrors_the_process_wide_log() {
        let global = FileLog::default_instance();
        let original = global.enabled();
        let log = DefaultFileLog;
        global.set_enabled(true);
        let when_on = log.enabled();
        global.set_enabled(false);
        let when_off = log.enabled();
        global.set_enabled(original);
        assert!(when_on, "enabled must follow the global log when on");
        assert!(!when_off, "enabled must follow the global log when off");
    }

    /// No injected probe and no `has_headroom`: fail closed (never headroom, no snapshot). Kills `has_headroom -> true`.
    #[test]
    fn missing_probe_fails_closed() {
        let probe = choose_memory_probe(None, None);
        assert!(!probe.has_headroom(0.0, 0));
        assert!(!probe.has_headroom(1.0, i64::MAX));
        assert!(probe.get_snapshot().is_none());
    }
}
