//! Port of `PhotoReview.Benchmarking/BenchmarkImageExecutor.cs` (C# baseline d92d6aae, 239 lines / 14 064 bytes): the
//! production-compatible image executor of a benchmark run, `BenchmarkProfileScope` and the internal
//! `WindowedPreloadTarget`. Contract: `contracts/W7-03b.md` (draft: `contracts/drafts-w7/W7-03b/contract.md`).
//! PARITY: W-06 (BenchmarkWindow runs profiles through this executor), X-benchmark (new rows, see the contract).
//!
//! Target path: `crates/pr-bench/src/image_executor.rs` (lead adds `pub mod image_executor;` to `lib.rs`).
//!
//! Every decode goes through a REAL `PreviewImageService` / `PreloadScheduler` pair (same RAM cache, disk-cache
//! probe/persist path and preload-priority scheduling as the viewer), with a private scratch disk-cache directory per
//! executor. R0003: no async runtime. C# `Task`/`await` become blocking calls and handles
//! (`PreviewHandle`, `RunHandle`, [`TeardownHandle`]); nothing here may be called from a UI thread.

#![allow(unused_variables, dead_code)] // W7-03b stub: remove with the last `todo!()`.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use pr_core::abstractions::{Log, MemoryProbe};
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::{ReviewMetrics, ReviewMetricsSnapshot};
use pr_core::model::enums::LoadingMode;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;
use pr_imaging::cache::preview_file::PreviewPayloadCodec;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::preload::{PreloadScheduler, RunHandle};
use pr_imaging::service::handle::PreviewHandle;
use pr_imaging::service::{PreviewError, PreviewImageService, PreviewResult, PreviewServiceBuildError};

// ---- W7-01 stand-in -----------------------------------------------------------------------------------------------
// `BenchmarkProfile` is owned by W7-01 (`crate::models`, `BenchmarkModels.cs`). This private copy lets the stub compile
// on its own; the lead replaces the whole block by `use crate::models::BenchmarkProfile;` at wiring. Only the fields
// this unit reads are listed; the real record has more (id, name, description, workload, warmup_count, iterations,
// correctness_only).

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkProfile {
    pub id: String,
    pub loading_mode: LoadingMode,
    pub workers: i32,
    pub next_window: i32,
    pub previous_window: i32,
    pub full_folder: bool,
    pub memory_reserve_bytes: i64,
    pub disk_cache: bool,
    pub detailed_logging: bool,
}

impl BenchmarkProfile {
    /// C# extension `TargetWidth()`.
    pub fn target_width(&self) -> i32 {
        match self.id.as_str() {
            "preview-light" => 960,
            "preview-balanced" => 1600,
            "preview-quality" => 2400,
            "preview-high-quality" => 3200,
            "original-correctness" => 0,
            _ => 1920,
        }
    }
}

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
    /// C# `hasHeadroom`: `None` = the real OS memory check (`pr_platform_win::WindowsMemoryProbe`, production
    /// behaviour); `Some(f)` = `DelegateMemoryProbe::new(f)` (tests inject a fixed answer).
    pub has_headroom: Option<HasHeadroomFn>,
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
        todo!("W7-03b: a handle whose flag is already true")
    }

    /// A pending teardown; `finish` completes it.
    pub(crate) fn pending() -> Self {
        todo!("W7-03b: a handle whose flag is false")
    }

    /// Marks the teardown finished and wakes every waiter.
    pub(crate) fn finish(&self) {
        todo!("W7-03b")
    }

    /// Whether the teardown has finished.
    pub fn is_complete(&self) -> bool {
        todo!("W7-03b")
    }

    /// Blocks until the teardown has finished. Tests use [`Self::wait_timeout`] (hang guard, AGENTS.md "Tests").
    pub fn wait(&self) {
        todo!("W7-03b")
    }

    /// Blocks up to `timeout`; `true` when the teardown finished (C# `WaitAsync(timeout)` that did not throw
    /// `TimeoutException`).
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        todo!("W7-03b")
    }
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
        todo!("W7-03b")
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
        todo!("W7-03b: take the closure out of `restore` and call it")
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
        todo!("W7-03b")
    }

    /// C# `SetCenter` (`Volatile.Write`): release store.
    pub fn set_center(&self, center: i32) {
        todo!("W7-03b")
    }

    /// C# private `OutsideWindow(path)`. `false` when `full_folder` or `path` is not in the index (a path the profile
    /// does not know is never held back). Else `delta = index - center` and the result is `delta > next || -delta >
    /// previous` with C# UNCHECKED `int` arithmetic: use `wrapping_sub` / `wrapping_neg` on `i32`, NOT `i64` (golden
    /// `window-center-int-min-wraps`). The lookup key is `ignore_case_key(path)` of the path AS GIVEN (no
    /// `get_full_path`): a non-normalized spelling is simply "not in the index" (golden `window-unnormalized-by-path`).
    fn outside_window(&self, path: &str) -> bool {
        todo!("W7-03b")
    }
}

impl PreloadTarget for WindowedPreloadTarget {
    /// `outside_window(path) || inner.try_get_cached_preview_for_path(path)` (short-circuit: the inner is not asked
    /// for an out-of-window path).
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
        todo!("W7-03b")
    }

    /// `outside_window(key.path()) || inner.try_get_cached_preview(key)`.
    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        todo!("W7-03b")
    }

    /// Straight forward to the inner target (preloading is never windowed here; the scheduler's queue decides).
    fn preload(&self, path: &str, cancel: &CancelToken) -> Result<(), DecodeError> {
        todo!("W7-03b")
    }

    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
        todo!("W7-03b")
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        todo!("W7-03b")
    }

    fn cache_count(&self) -> i32 {
        todo!("W7-03b")
    }

    fn cache_bytes(&self) -> i64 {
        todo!("W7-03b")
    }

    /// C# forwards the three default members too (`ActiveViewerDecodes`, `CachedPreviewBytes`, `HasDiskCachedPreview`):
    /// the trait defaults (0 / `None` / `false`) would silently disable the inner target's answers.
    fn active_viewer_decodes(&self) -> i32 {
        todo!("W7-03b")
    }

    fn cached_preview_bytes(&self, key: &ImageCacheKey) -> Option<i64> {
        todo!("W7-03b")
    }

    fn has_disk_cached_preview(&self, key: &ImageCacheKey) -> bool {
        todo!("W7-03b")
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
    todo!("W7-03b")
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
        todo!("W7-03b")
    }

    /// C# `EvictForColdDecode(path)`: `service.evict_cached_path(path, Some(&|normalized|
    /// scheduler.remove_preloaded_keys_for_path(normalized)))?` THEN `service.clear_disk()` (RAM first, then disk: a
    /// persisted copy must not satisfy the next "cold" decode). `Err` only for a path `get_full_path` refuses
    /// (`PreviewError::InvalidPath`; `PreviewError::SourceBytes` is unreachable: no source-bytes cache is configured).
    /// `clear_disk` is not reached after an `Err` (C# threw before it).
    pub fn evict_for_cold_decode(&self, path: &str) -> Result<(), PreviewError> {
        todo!("W7-03b")
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
        todo!("W7-03b")
    }

    /// `decode_async(..)?.wait()`.
    pub fn decode(&self, path: &str, token: &CancelToken) -> PreviewResult {
        todo!("W7-03b")
    }

    /// C# `Metrics`: `metrics.snapshot()` (cumulative since the executor was built).
    pub fn metrics(&self) -> ReviewMetricsSnapshot {
        todo!("W7-03b")
    }

    /// C# `WarmPreloadAround(center)`: `preload_target.set_center(center)` BEFORE the kick, then
    /// `*last_preload = Some(scheduler.preload_around(center))` (the previous handle is overwritten, not awaited).
    pub fn warm_preload_around(&self, center: i32) {
        todo!("W7-03b")
    }

    /// C# `IsPreviewCached(path)` test seam: `service.try_get_cached_preview(&service.get_current_cache_key(path)?)`
    /// `.is_some()`. `Err` for a missing file (C# threw).
    pub fn is_preview_cached(&self, path: &str) -> Result<bool, CacheKeyError> {
        todo!("W7-03b")
    }

    /// C# `WhenPreloadSettledAsync()`: waits for the run started by the last `warm_preload_around` (returns at once when
    /// there was none). Clone the handle out of the mutex BEFORE waiting.
    pub fn when_preload_settled(&self) {
        todo!("W7-03b")
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
        todo!("W7-03b")
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
        todo!("W7-03b")
    }
}

/// C# `DeleteScratchDirectoryAfterPruneAsync`, the body of the teardown thread.
///
/// `settled = service.wait_for_prune(timeout)`. `false` => `log.error("Benchmark disk cache prune did not finish in
/// time; leaving scratch directory: {dir}", None)` and return WITHOUT deleting. `true` => when `dir.exists()`
/// `std::fs::remove_dir_all(dir)`; an `Err` (or a panic) is logged `log.error("Benchmark scratch directory could not be
/// removed: {dir}", Some(&err))` and swallowed (this task is never awaited by production code). `{dir}` is
/// `dir.display()`.
pub(crate) fn delete_scratch_directory_after_prune(
    service: &PreviewImageService,
    dir: &Path,
    timeout: Duration,
    log: &dyn Log,
) {
    todo!("W7-03b")
}

/// Picks the memory probe: `Some(f)` => `DelegateMemoryProbe::new(f)`, `None` => `pr_platform_win::WindowsMemoryProbe`.
pub(crate) fn choose_memory_probe(has_headroom: Option<HasHeadroomFn>, log: Arc<dyn Log>) -> Arc<dyn MemoryProbe> {
    todo!("W7-03b")
}
