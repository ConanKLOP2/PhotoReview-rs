//! W4-14c (port of `PreviewImageService.cs`, C# baseline d92d6aae; PARITY I-08, D-06, C-05, INV-2): the preview disk
//! cache as the service uses it. The PERSISTED file name (`<SHA-256 hex of the identity>.pv4`, identity
//! `preview-v5-box|...`), the disk-hit read, the bounded persist queue with its two workers (epoch re-checks, the
//! per-path newest-writer table so a stale worker never deletes a newer epoch's file), the one-shot startup cleanup
//! (pre-v4 legacy files + stale temp files) and Clear Disk. Contract: `contracts/W4-14c.md`.
//!
//! The entry FORMAT is W4-08b (`cache::preview_file`); quota prune, access touch and the temp sweep are W4-09
//! (`cache::disk_store`). Like the C# class, this file uses the real file system (`std::fs` / W4-09 helpers) for the
//! cache directory; sources go through the injected `FileSystem` / `SourceReader`.
//!
//! Target path: `crates/pr-imaging/src/service/disk.rs`.

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use pr_core::abstractions::{FileSystem, Log};
use pr_core::diagnostics::perf::{self, PhotoReviewPerf};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::serde_ext::LenientEnum;
use pr_core::sync::CancelToken;
use pr_core::text::path_key;

use super::deps::{
    self, DecodeError, ImageCacheKey, ImageSourceKind, PreviewCacheError, delete_stale_temp_files, is_fully_opaque,
    preview_file, sha256, try_delete,
};
use super::{DecodeThreadKind, PreviewImage, PreviewImageService, ServiceInner, TestHook, lock};

/// C# `PersistWorkerCount` (pinned with the queue capacity: `PersistQueueBound_IsPinnedAt18`).
pub const PERSIST_WORKER_COUNT: usize = 2;
/// C# `PersistQueueCapacity`: queued previews stay alive outside the RAM budget until written; a full queue DROPS the
/// new request (C# `BoundedChannelFullMode.DropWrite`).
pub const PERSIST_QUEUE_CAPACITY: usize = 16;
/// C# `LegacyCleanupMaxFiles`: per pattern, per pass (legacy `*.png`, `*.png.meta`) and for the stale temp sweep.
pub const LEGACY_CLEANUP_MAX_FILES: i32 = 5000;
/// The store's search pattern (C# `new DiskCacheStore(dir, "*.pv4", ...)`).
pub const DISK_CACHE_SEARCH_PATTERN: &str = "*.pv4";
/// The entry file extension appended to the hash.
pub const DISK_CACHE_EXTENSION: &str = ".pv4";
/// First field of the identity (C# "preview-v5-box": box key + v5 header bump; persisted, never change it).
pub const DISK_CACHE_IDENTITY_PREFIX: &str = "preview-v5-box";
/// Legacy (pre-v4) entry patterns swept at startup and by Clear Disk, in this order.
pub const LEGACY_PATTERNS: [&str; 2] = ["*.png", "*.png.meta"];

/// Upper-case hex digits (C# `Convert.ToHexString`).
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// C# `ToString()` of `bool` inside an interpolated string: `True` / `False`.
pub(crate) fn dotnet_bool(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

/// C# `$"preview-v5-box|{key.Path}|{key.Length}|{key.LastWriteUtcTicks}|{key.IsOriginal}|{key.TargetWidth}x{key.
/// TargetHeight}|{key.OrientationApplied}|{key.Backend}"` (invariant culture), plus `|kind{key.SourceKind}` when the
/// source kind is not `STANDARD` (so every standard entry kept its hash when the field was added). Numbers are plain
/// invariant decimals (`-5` for a negative width); booleans `True`/`False`; the backend is the .NET enum NAME
/// (`LenientEnum::name`, an unnamed value prints its number like .NET).
pub fn disk_cache_identity(key: &ImageCacheKey) -> String {
    // Rust integer formatting is culture-free: `-` and ASCII digits, exactly the C# invariant culture.
    let mut identity = format!(
        "{DISK_CACHE_IDENTITY_PREFIX}|{}|{}|{}|{}|{}x{}|{}|{}",
        key.path(),
        key.length(),
        key.last_write_utc_ticks(),
        dotnet_bool(key.is_original()),
        key.target_width(),
        key.target_height(),
        dotnet_bool(key.orientation_applied()),
        key.backend().name(),
    );
    if key.source_kind() != ImageSourceKind::STANDARD {
        identity.push_str("|kind");
        identity.push_str(&key.source_kind().to_string());
    }
    identity
}

/// `Convert.ToHexString(SHA256.HashData(UTF8(identity)))` + `.pv4`: 64 UPPER-case hex digits then the extension.
pub fn disk_cache_file_name(key: &ImageCacheKey) -> String {
    let hash = sha256(disk_cache_identity(key).as_bytes());
    let mut name = String::with_capacity(hash.len() * 2 + DISK_CACHE_EXTENSION.len());
    for byte in hash {
        name.push(char::from(HEX_UPPER[usize::from(byte >> 4)]));
        name.push(char::from(HEX_UPPER[usize::from(byte & 0x0F)]));
    }
    name.push_str(DISK_CACHE_EXTENSION);
    name
}

/// C# `GetDiskCachePath(key)`: `Path.Combine(directory, disk_cache_file_name(key))`.
pub fn disk_cache_path(directory: &Path, key: &ImageCacheKey) -> PathBuf {
    combine(directory, &disk_cache_file_name(key))
}

/// .NET (Core) `Path.Combine(directory, name)` for a relative `name`: an empty directory gives `name`; a directory
/// ending in `\` or `/` is followed by `name` directly; anything else gets ONE `\`, a bare drive `D:` included (.NET
/// Core no longer treats the volume separator as a separator: `D:` + `x` is `D:\x`, where `Path::join` gives `D:x`).
/// The golden `preview-disk-cache-path` pins trailing separators, `/` directories, UNC, drive roots and `D:`.
fn combine(directory: &Path, name: &str) -> PathBuf {
    let directory = directory.as_os_str();
    if directory.is_empty() {
        return PathBuf::from(name);
    }
    let mut combined = directory.to_os_string();
    let text = directory.to_string_lossy();
    if !(text.ends_with('\\') || text.ends_with('/')) {
        combined.push("\\");
    }
    combined.push(name);
    PathBuf::from(combined)
}

/// One queued write (C# `(IDecodedImage Image, string CachePath, long Epoch)`).
pub(crate) struct PersistRequest {
    pub(crate) image: PreviewImage,
    pub(crate) cache_path: PathBuf,
    pub(crate) epoch: i64,
}

impl fmt::Debug for PersistRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PersistRequest")
            .field("cache_path", &self.cache_path)
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

/// C# `_persistQueue` state: bounded FIFO + "writer completed" flag.
#[derive(Debug, Default)]
pub(crate) struct QueueState {
    pub(crate) items: VecDeque<PersistRequest>,
    /// C# `Writer.TryComplete()` happened: nothing new is accepted; workers drain `items` and exit.
    pub(crate) completed: bool,
    /// Workers that have not exited yet (shutdown waits for 0).
    pub(crate) live_workers: usize,
}

/// Persist machinery owned by [`ServiceInner`] (C# `_persistQueue`, `_persistWorkers`, `_persistOwnerEpochs`,
/// `AfterPersistWriteForTests`, `StartupCleanup`).
pub(crate) struct PersistState {
    pub(crate) queue: Mutex<QueueState>,
    /// Signalled on push, on complete and when a worker exits.
    pub(crate) changed: Condvar,
    /// C# `_persistOwnerEpochs` (`OrdinalIgnoreCase`): key = `pr_core::text::path_key(cache_path)`, value = (newest
    /// epoch that started writing the path, writers in flight). Guarded by its own mutex; `delete_stale_file` deletes
    /// UNDER it (the check and the delete must be atomic against a claim; only persist workers take it).
    pub(crate) owner_epochs: Mutex<HashMap<String, (i64, i32)>>,
    /// C# `AfterPersistWriteForTests`: runs on the worker right after a successful write, before the stale re-check.
    pub(crate) after_write_for_tests: Mutex<Option<TestHook>>,
    /// C# `StartupCleanup` task: `true` once the startup cleanup finished (or was never scheduled).
    pub(crate) startup_cleanup_done: Mutex<bool>,
    pub(crate) startup_cleanup_signal: Condvar,
}

impl fmt::Debug for PersistState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PersistState").finish_non_exhaustive()
    }
}

impl PersistState {
    /// Empty queue, no worker yet, startup cleanup "done" (C# `Task.CompletedTask` until scheduled).
    pub(crate) fn new() -> Self {
        Self {
            queue: Mutex::new(QueueState::default()),
            changed: Condvar::new(),
            owner_epochs: Mutex::new(HashMap::new()),
            after_write_for_tests: Mutex::new(None),
            startup_cleanup_done: Mutex::new(true),
            startup_cleanup_signal: Condvar::new(),
        }
    }

    /// C# `_persistQueue.Writer.TryWrite(..)` on a `DropWrite` channel: appends unless completed or already holding
    /// [`PERSIST_QUEUE_CAPACITY`] items (then the request is DROPPED silently; returns whether it was queued).
    pub(crate) fn try_push(&self, request: PersistRequest) -> bool {
        let mut queue = lock(&self.queue);
        if queue.completed || queue.items.len() >= PERSIST_QUEUE_CAPACITY {
            // Dropped here: the request (and the image it holds) is released at once.
            return false;
        }
        queue.items.push_back(request);
        drop(queue);
        self.changed.notify_all();
        true
    }

    /// C# `Writer.TryComplete()`: idempotent; wakes every worker.
    pub(crate) fn complete(&self) {
        lock(&self.queue).completed = true;
        self.changed.notify_all();
    }

    /// Blocks until an item is available (`Some`) or the queue is completed AND empty (`None`).
    pub(crate) fn pop(&self) -> Option<PersistRequest> {
        let mut queue = lock(&self.queue);
        loop {
            if let Some(request) = queue.items.pop_front() {
                return Some(request);
            }
            if queue.completed {
                return None;
            }
            queue = self.changed.wait(queue).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Items waiting right now (tests).
    pub(crate) fn len(&self) -> usize {
        lock(&self.queue).items.len()
    }

    /// Marks one worker as gone and wakes the shutdown waiters.
    fn worker_exited(&self) {
        let mut queue = lock(&self.queue);
        queue.live_workers = queue.live_workers.saturating_sub(1);
        drop(queue);
        self.changed.notify_all();
    }

    /// Marks the startup cleanup as finished and wakes its waiters.
    fn startup_cleanup_finished(&self) {
        *lock(&self.startup_cleanup_done) = true;
        self.startup_cleanup_signal.notify_all();
    }
}

/// Decrements `live_workers` when a persist worker leaves its loop, however it leaves (R0003: a worker never dies
/// silently without the shutdown waiters noticing).
struct WorkerExitGuard<'a>(&'a PersistState);

impl Drop for WorkerExitGuard<'_> {
    fn drop(&mut self) {
        self.0.worker_exited();
    }
}

/// Marks the startup cleanup as done on every exit path of its thread (a panic included).
struct StartupCleanupGuard(Arc<ServiceInner>);

impl Drop for StartupCleanupGuard {
    fn drop(&mut self) {
        self.0.persist.startup_cleanup_finished();
    }
}

/// C# `finally { ReleasePersistPath(request.CachePath); }`: runs on success, on a write error and on a panic.
struct ClaimGuard<'a> {
    inner: &'a ServiceInner,
    cache_path: &'a Path,
}

impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        self.inner.release_persist_path(self.cache_path);
    }
}

impl PreviewImageService {
    /// C# `ShutdownPersistWorkersAsync()` (blocking): completes the queue (nothing new is accepted) and waits until every
    /// worker drained it and exited. Only for short-lived instances (tests, benchmark runs). Idempotent.
    pub fn shutdown_persist_workers(&self) {
        let persist = &self.inner.persist;
        persist.complete();
        let queue = lock(&persist.queue);
        let _queue = persist
            .changed
            .wait_while(queue, |queue| queue.live_workers > 0)
            .unwrap_or_else(PoisonError::into_inner);
    }

    /// [`Self::shutdown_persist_workers`] with a hang guard: `false` when the workers had not exited after `timeout`.
    pub fn shutdown_persist_workers_timeout(&self, timeout: Duration) -> bool {
        self.shutdown_persist_workers_timeout_observed_for_tests(timeout, |_, _| {})
    }

    /// Observes the actual bounded-shutdown predicate before the condition variable can wait.
    /// The ordinary entry point uses a no-op observer; queue completion, locking and waiting are shared.
    /// The observer runs under the queue lock and must not reenter the service.
    #[doc(hidden)]
    pub fn shutdown_persist_workers_timeout_observed_for_tests(
        &self,
        timeout: Duration,
        mut observe: impl FnMut(usize, bool),
    ) -> bool {
        let persist = &self.inner.persist;
        persist.complete();
        let queue = lock(&persist.queue);
        let (queue, _) = persist
            .changed
            .wait_timeout_while(queue, timeout, |queue| {
                let pending = queue.live_workers > 0;
                observe(queue.live_workers, pending);
                pending
            })
            .unwrap_or_else(PoisonError::into_inner);
        queue.live_workers == 0
    }

    /// C# `AfterPersistWriteForTests` setter.
    #[doc(hidden)]
    pub fn set_after_persist_write_for_tests(&self, hook: Option<TestHook>) {
        *lock(&self.inner.persist.after_write_for_tests) = hook;
    }

    /// C# `StartupCleanup.WaitAsync(timeout)`: `true` once the startup cleanup finished (immediately when the disk
    /// cache is disabled: it was never scheduled).
    #[doc(hidden)]
    pub fn wait_startup_cleanup(&self, timeout: Duration) -> bool {
        let persist = &self.inner.persist;
        let done = lock(&persist.startup_cleanup_done);
        let (done, _) = persist
            .startup_cleanup_signal
            .wait_timeout_while(done, timeout, |done| !*done)
            .unwrap_or_else(PoisonError::into_inner);
        *done
    }

    /// C# `ClearDisk()` (Clear Cache, C-05, INV-2): bump the epoch under the lifecycle gate (in-flight decodes and
    /// queued writes of the old epoch are then dropped), then `disk_store.clear_directory()` (entries + temp files) and
    /// the legacy sweep. A failure of the directory clear is logged `Preview disk cache clear failed: {directory}` with
    /// the error and swallowed; the RAM cache, the original dimensions and `CacheCleared` are NOT touched.
    pub fn clear_disk(&self) {
        let inner = &self.inner;
        {
            // INV-2: the bump comes BEFORE the delete, so a worker that finishes its write after the delete sees a stale
            // epoch and removes its own file instead of recreating an entry of the cleared generation.
            let _gate = lock(&inner.lifecycle_gate);
            inner.bump_epoch_under_gate();
        }
        match inner.disk_store.clear_directory() {
            // An explicit user action: the leftover pre-v4 files go now, not with the once-per-process startup pass.
            Ok(()) => cleanup_legacy_cache_files(&inner.disk_directory, inner.log.as_ref()),
            // C#: the exception leaves the `try` before the legacy sweep.
            Err(error) => inner.log.error(
                &format!("Preview disk cache clear failed: {}", inner.disk_directory.display()),
                Some(&error),
            ),
        }
    }

    /// C# `WaitForPruneAsync(timeout)` (blocking): delegates to the disk store.
    pub fn wait_for_prune(&self, timeout: Duration) -> bool {
        self.inner.disk_store.wait_for_prune(timeout)
    }

    /// C# `HasDiskCachedPreview(key)` (also `IPreloadTarget`): `false` while the disk cache is disabled, else whether
    /// the entry file exists (one existence check, no read).
    pub fn has_disk_cached_preview(&self, key: &ImageCacheKey) -> bool {
        // C# `&&`: the key is not hashed and nothing is probed while the disk cache is off.
        !self.inner.disable_disk_cache
            && self
                .inner
                .disk_entry_exists(&disk_cache_path(&self.inner.disk_directory, key))
    }
}

impl ServiceInner {
    /// Called once by the constructor (W4-14a) after the struct is built: starts [`PERSIST_WORKER_COUNT`] workers
    /// ([`DecodeThreadKind::PersistWorker`], each running `run_persist_worker`) and, unless the disk cache is disabled,
    /// the startup cleanup ([`DecodeThreadKind::StartupCleanup`]: `cleanup_legacy_cache_files` then
    /// `delete_stale_temp_files(dir, clock.utc_now(), LEGACY_CLEANUP_MAX_FILES, log)`, then mark it done). Workers are
    /// started even when the disk cache is disabled (C# does; they simply never receive work).
    pub(crate) fn start_background(self: &Arc<Self>) -> io::Result<()> {
        for _ in 0..PERSIST_WORKER_COUNT {
            // Counted BEFORE the spawn: a shutdown racing the start still waits for this worker.
            lock(&self.persist.queue).live_workers += 1;
            let inner = Arc::clone(self);
            let job: Box<dyn FnOnce() + Send> = Box::new(move || inner.run_persist_worker());
            if let Err(error) = self.spawner.spawn(DecodeThreadKind::PersistWorker, job) {
                // The job was dropped without running: it never decrements, so undo the count here.
                self.persist.worker_exited();
                return Err(error);
            }
        }
        // C# `ScheduleLegacyCacheCleanup`: nothing at all while the disk cache is off.
        if self.disable_disk_cache {
            return Ok(());
        }
        *lock(&self.persist.startup_cleanup_done) = false;
        let inner = Arc::clone(self);
        let job: Box<dyn FnOnce() + Send> = Box::new(move || {
            // Done on every exit path (a panicking log included): `wait_startup_cleanup` never waits forever.
            let guard = StartupCleanupGuard(inner);
            let inner = &guard.0;
            cleanup_legacy_cache_files(&inner.disk_directory, inner.log.as_ref());
            // Atomic-write temp files orphaned by a killed process: no prune/clear pattern ever matches them.
            delete_stale_temp_files(
                &inner.disk_directory,
                inner.clock.utc_now(),
                LEGACY_CLEANUP_MAX_FILES,
                Some(inner.log.as_ref()),
            );
        });
        if let Err(error) = self.spawner.spawn(DecodeThreadKind::StartupCleanup, job) {
            self.persist.startup_cleanup_finished();
            return Err(error);
        }
        Ok(())
    }

    /// The decode path's `cachePath` (C# `if (!_disableDiskCache) cachePath = GetDiskCachePath(key)`): `None` while the
    /// disk cache is disabled (the key is then never hashed, L02).
    pub(crate) fn disk_cache_path_for(&self, key: &ImageCacheKey) -> Option<PathBuf> {
        if self.disable_disk_cache {
            None
        } else {
            Some(disk_cache_path(&self.disk_directory, key))
        }
    }

    /// C# `File.Exists(cachePath)` of the decode path (a file, not a directory; any error is "no").
    pub(crate) fn disk_entry_exists(&self, cache_path: &Path) -> bool {
        std::fs::metadata(cache_path).is_ok_and(|metadata| metadata.is_file())
    }

    /// The disk-hit branch of C# `DecodeAndCache`: `preview_file::read(cache_path, payload_codec)` then
    /// `preview_file::to_decoded_image(entry, platform_codec)`; on success `record_disk_cache_hit`,
    /// `disk_store.note_accessed(cache_path)` and the perf `DiskCacheRead` event (file bytes from the read). The caller
    /// (W4-14b) handles `Err`: a cache-entry failure deletes the entry (`try_delete` WITH the log) and decodes the source;
    /// any other error (`OutOfMemory`, `Canceled`) propagates as `PreviewError::DiskEntry`.
    pub(crate) fn try_read_disk_entry(&self, cache_path: &Path) -> Result<PreviewImage, PreviewCacheError> {
        if let Some(observe) = &self.before_disk_read_for_tests {
            observe();
        }
        let perf_log = PhotoReviewPerf::log();
        let perf_on = perf_log.is_enabled();
        let started = if perf_on { perf::timestamp() } else { 0 };
        // perf(cache) v4: one file open, one header read, one decode. The header's backend (not the key's) is the
        // image's: a fallback-decoded preview is stored under the requested backend's path.
        let entry = preview_file::read(cache_path, self.payload_codec.as_ref())?;
        let file_bytes = entry.file_bytes;
        let image = preview_file::to_decoded_image(entry, self.platform_codec.as_ref())?;
        self.metrics.record_disk_cache_hit();
        self.disk_store.note_accessed(cache_path);
        if perf_on {
            // DIVERGENCE (reported to the lead): C# passes `PathId(source path)`; this signature only has the cache path,
            // so the event carries the id of the entry path until W4-14b hands the source path in.
            let path_id = perf::path_id(&cache_path.to_string_lossy());
            perf_log.disk_cache_read(PhotoReviewPerf::nav_context(), &path_id, perf::ms(started), file_bytes);
        }
        Ok(Arc::new(image))
    }

    /// C# `PersistToDiskCache(image, cachePath, cacheEpoch)`: drop when `cache_epoch` is no longer current, else
    /// `try_push` (a full queue drops it).
    pub(crate) fn queue_persist(&self, image: PreviewImage, cache_path: PathBuf, cache_epoch: i64) {
        if cache_epoch != self.current_epoch() {
            return;
        }
        // Best effort: a full queue means persistence is falling behind decode; the preview is dropped (no log).
        let _queued = self.persist.try_push(PersistRequest {
            image,
            cache_path,
            epoch: cache_epoch,
        });
    }

    /// C# `RunPersistWorkerAsync` loop (W4-14c semantics 5): one request at a time until the queue is completed and
    /// drained; every failure is logged `Preview disk cache write failed: {cache_path}` and the loop goes on (a worker
    /// never dies: the 2 workers are never restarted). Decrements `live_workers` on exit.
    pub(crate) fn run_persist_worker(self: &Arc<Self>) {
        let _exit = WorkerExitGuard(&self.persist);
        while let Some(request) = self.persist.pop() {
            let failure: Option<Box<dyn Error + Send + Sync>> =
                match catch_unwind(AssertUnwindSafe(|| self.persist_one(&request))) {
                    Ok(Ok(())) => None,
                    Ok(Err(failure)) => Some(Box::new(failure)),
                    Err(payload) => Some(Box::new(PersistPanic::from_payload(payload.as_ref()))),
                };
            if let Some(failure) = failure {
                let message = format!("Preview disk cache write failed: {}", request.cache_path.display());
                // A panicking log must not kill the worker either (only 2 exist for the process lifetime).
                let _logged = catch_unwind(AssertUnwindSafe(|| self.log.error(&message, Some(failure.as_ref()))));
            }
            // The request (and the image it keeps alive outside the RAM budget) is dropped here, before the next pop.
        }
    }

    /// One request of the loop (semantics 5, steps 1-9). `Ok` covers the skips (stale epoch, foreign platform image,
    /// transparent pixels, went stale mid-write); `Err` is logged by the loop.
    pub(crate) fn persist_one(&self, request: &PersistRequest) -> Result<(), PersistFailure> {
        // 1. The folder/cache may have moved on since this was queued: re-check before any I/O (INV-2).
        if request.epoch != self.current_epoch() {
            return Ok(());
        }
        let image = &request.image;
        // 2. WP-04: borrow (or copy) the pixels only now that the write really happens.
        let lease = match self.platform_codec.to_pixels(image.platform_image()) {
            Ok(lease) => lease,
            // C# `catch (ArgumentException) { continue; }`: a foreign platform image is simply not persisted.
            Err(error) if matches!(error.kind(), DecodeError::InvalidArgument { .. }) => return Ok(()),
            Err(error) => return Err(PersistFailure::Pixels(Some(error))),
        };
        let Some(pixels) = lease.pixels() else {
            return Err(PersistFailure::Pixels(None));
        };
        // 3. IMG-01/Q-R7: JPEG would flatten transparency; alpha previews persist only when no pixel is transparent.
        if !is_fully_opaque(pixels).map_err(PersistFailure::Opacity)? {
            return Ok(());
        }
        // 4. Newest-writer bookkeeping; the guard releases on every exit path (C# `finally`).
        self.claim_persist_path(&request.cache_path, request.epoch);
        let claim = ClaimGuard {
            inner: self,
            cache_path: &request.cache_path,
        };
        // 5. C# passes no cancellation token to the write.
        preview_file::write_atomically(
            pixels,
            image.actual_backend(),
            image.orientation(),
            image.original_width(),
            image.original_height(),
            &request.cache_path,
            true,
            image.exif(),
            self.payload_codec.as_ref(),
            &CancelToken::none(),
        )
        .map_err(PersistFailure::Write)?;
        // 6. Test seam: cloned out of its mutex, never called under a lock.
        let hook = lock(&self.persist.after_write_for_tests).clone();
        if let Some(hook) = hook {
            hook();
        }
        // 7. Went stale mid-write (Clear Cache ran concurrently): delete the file unless a newer epoch claimed the path.
        // BEFORE the release: this writer is still counted, so a newer claim is still visible in the table.
        if request.epoch != self.current_epoch() {
            self.delete_stale_persist_file(&request.cache_path, request.epoch);
            return Ok(());
        }
        // 8. Release, then the store bookkeeping (coalesced prune, W4-09).
        drop(claim);
        self.disk_store.note_written(&request.cache_path);
        self.disk_store.schedule_prune();
        // 9. The lease drops here (an owned lease frees its buffer).
        Ok(())
    }

    /// C# `ClaimPersistPath(path, epoch)`: `(max(old epoch, epoch), writers + 1)` (a missing entry counts as `(0, 0)`).
    pub(crate) fn claim_persist_path(&self, cache_path: &Path, epoch: i64) {
        let mut owners = lock(&self.persist.owner_epochs);
        let entry = owners.entry(path_key(cache_path)).or_insert((0, 0));
        // C# `Math.Max(entry.Epoch, epoch)` with a default entry of 0 and an unchecked `Writers + 1`.
        *entry = (entry.0.max(epoch), entry.1.wrapping_add(1));
    }

    /// C# `ReleasePersistPath(path)`: nothing when absent; remove the entry when `writers <= 1`, else keep its epoch with
    /// `writers - 1` (an older stale writer must still see that a newer epoch rewrote the path).
    pub(crate) fn release_persist_path(&self, cache_path: &Path) {
        let mut owners = lock(&self.persist.owner_epochs);
        let key = path_key(cache_path);
        let Some(&(epoch, writers)) = owners.get(&key) else {
            return;
        };
        if writers <= 1 {
            owners.remove(&key);
        } else {
            owners.insert(key, (epoch, writers - 1));
        }
    }

    /// C# `DeleteStalePersistFile(path, epoch)`: under the owner lock, return when the entry's epoch is NEWER than
    /// `epoch`; else `try_delete(cache_path, None)` (no log, like C#).
    pub(crate) fn delete_stale_persist_file(&self, cache_path: &Path, epoch: i64) {
        // R0003 exception (documented in the contract): the delete runs UNDER the owner mutex so the check and the delete
        // are atomic against a newer worker's claim; only the two persist workers take this mutex, for one file.
        let owners = lock(&self.persist.owner_epochs);
        if owners
            .get(&path_key(cache_path))
            .is_some_and(|&(owner_epoch, _)| owner_epoch > epoch)
        {
            return;
        }
        try_delete(cache_path, None);
        drop(owners);
    }
}

/// Why one persist request failed (logged, never propagated).
#[derive(Debug)]
pub(crate) enum PersistFailure {
    /// `to_pixels` failed with anything but `InvalidArgument` (C# e.g. `ObjectDisposedException`), or the lease has no
    /// pixels.
    Pixels(Option<DecodeError>),
    /// `is_fully_opaque` failed (disposed buffer).
    Opacity(deps::PixelBufferError),
    /// `preview_file::write_atomically` failed.
    Write(PreviewCacheError),
}

impl fmt::Display for PersistFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pixels(Some(error)) => fmt::Display::fmt(error, f),
            Self::Pixels(None) => f.write_str("The platform codec returned no pixels."),
            Self::Opacity(error) => fmt::Display::fmt(error, f),
            Self::Write(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl Error for PersistFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Pixels(Some(error)) => Some(error),
            Self::Pixels(None) => None,
            Self::Opacity(error) => Some(error),
            Self::Write(error) => Some(error),
        }
    }
}

/// A panic inside one persist request, logged like a failed write (C# `catch (Exception)` around the request).
#[derive(Debug)]
struct PersistPanic(String);

impl PersistPanic {
    fn from_payload(payload: &(dyn std::any::Any + Send)) -> Self {
        let text = payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a persist request panicked".to_owned());
        Self(text)
    }
}

impl fmt::Display for PersistPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for PersistPanic {}

/// C# `CleanupLegacyCacheFiles(directory, log)`: when `directory` is an existing directory, list the FIRST
/// [`LEGACY_CLEANUP_MAX_FILES`] files matching `*.png` (Win32 wildcard semantics, `pr_core::io::physical::
/// matches_win32_expression`, files only, non-recursive, listing order) and `try_delete(path, Some(log))` each, then the
/// same for `*.png.meta`. A listing failure is logged `Legacy preview cache cleanup failed: {directory}` with the error
/// and ends the pass (C# catches `IOException`, `UnauthorizedAccessException`, `ArgumentException`,
/// `NotSupportedException` around the whole body).
pub(crate) fn cleanup_legacy_cache_files(directory: &Path, log: &dyn Log) {
    let fs = PhysicalFileSystem;
    // C# `Directory.Exists`: nothing is created and nothing is logged for a missing directory.
    if !fs.directory_exists(directory) {
        return;
    }
    let max_files = usize::try_from(LEGACY_CLEANUP_MAX_FILES).unwrap_or(0);
    for pattern in LEGACY_PATTERNS {
        // W2-14 `enumerate_files`: .NET `Directory.EnumerateFiles` (Win32 wildcard over the long name, files only,
        // listing order); the eager listing then deletes, the lazy C# one deletes while listing: same effects.
        match fs.enumerate_files(directory, pattern) {
            Ok(files) => {
                for path in files.iter().take(max_files) {
                    try_delete(path, Some(log));
                }
            }
            Err(error) => {
                log.error(
                    &format!("Legacy preview cache cleanup failed: {}", directory.display()),
                    Some(&error),
                );
                return;
            }
        }
    }
}

/// White-box tests (W4-14c): the persist queue, the workers, the owner table and the disk-hit read are crate-internal,
/// and the public path that queues a write (`get_preview` -> `queue_persist`) is W4-14b's. The C# twins that go through
/// `GetPreviewAsync` are in `tests/preview_service_disk.rs`; here the persist hand-off is driven directly with
/// `queue_persist(image, disk_cache_path_for(key), current_epoch())`, exactly what the decode path does after a source
/// decode.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::preview_file::PreviewPayloadCodec;
    use crate::cache_key::KeyOptions;
    use crate::decode_types::{
        DecodeBox, DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PixelLease,
        PlatformImage,
    };
    use crate::decoder_traits::{ImageDecoder, PlatformImageCodec};
    use crate::exif_summary::{ExifRational, ExifSummary};
    use crate::pixels::{PixelBuffer, PixelLayout};
    use crate::service::PreviewServiceConfig;
    use pr_core::catalog::types::CatalogEntry;
    use pr_core::diagnostics::metrics::ReviewMetrics;
    use pr_core::model::enums::DecoderBackend;
    use pr_core::time::DateTimeUtc;
    use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog, TempRoot};
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    /// Hang guard of every blocking wait (AGENTS.md "Tests").
    const TIMEOUT: Duration = Duration::from_secs(30);

    #[derive(Debug)]
    struct NeverDecoder;

    impl ImageDecoder for NeverDecoder {
        fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
            Err(DecodeError::InvalidOperation {
                message: "NeverDecoder".to_owned(),
            })
        }

        fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
            Err(DecodeError::InvalidOperation {
                message: "NeverDecoder".to_owned(),
            })
        }
    }

    /// The `pixels` platform codec (C# `PixelBufferImageCodec`): a foreign (`Opaque`) image is `InvalidArgument`.
    #[derive(Debug)]
    struct PixelsCodec;

    impl PlatformImageCodec for PixelsCodec {
        fn name(&self) -> &'static str {
            "pixels"
        }

        fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
            Ok(PlatformImage::Pixels(Arc::new(pixels)))
        }

        fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
            match platform_image {
                PlatformImage::Pixels(pixels) => Ok(PixelLease::borrowed(pixels.as_ref())),
                PlatformImage::Opaque(_) => Err(DecodeError::InvalidArgument {
                    message: "foreign platform image".to_owned(),
                }),
            }
        }
    }

    /// A codec whose `to_pixels` fails with something other than `InvalidArgument` (C# `ObjectDisposedException`), or
    /// returns an empty lease.
    #[derive(Debug)]
    struct BrokenCodec {
        empty_lease: bool,
    }

    impl PlatformImageCodec for BrokenCodec {
        fn name(&self) -> &'static str {
            "broken"
        }

        fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
            Ok(PlatformImage::Pixels(Arc::new(pixels)))
        }

        fn to_pixels<'a>(&self, _platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
            if self.empty_lease {
                Ok(PixelLease::empty())
            } else {
                Err(DecodeError::Disposed {
                    message: "disposed".to_owned(),
                })
            }
        }
    }

    /// Payload codec that keeps the raw pixel bytes: `FF D8`, width, height (i32 LE), the bytes, `FF D9`. Fails every
    /// encode of a buffer `fail_height` pixels high (0 = never).
    #[derive(Debug, Default)]
    struct RawPayloadCodec {
        fail_height: i32,
        encodes: AtomicUsize,
    }

    impl RawPayloadCodec {
        fn failing_height(fail_height: i32) -> Self {
            Self {
                fail_height,
                encodes: AtomicUsize::new(0),
            }
        }
    }

    impl PreviewPayloadCodec for RawPayloadCodec {
        fn encode_jpeg(&self, pixels: &PixelBuffer, _quality: i32, out: &mut dyn Write) -> io::Result<()> {
            self.encodes.fetch_add(1, Ordering::SeqCst);
            if self.fail_height != 0 && pixels.height() == self.fail_height {
                return Err(io::Error::other("scripted encode failure"));
            }
            out.write_all(&[0xFF, 0xD8])?;
            out.write_all(&pixels.width().to_le_bytes())?;
            out.write_all(&pixels.height().to_le_bytes())?;
            out.write_all(pixels.bytes().map_err(io::Error::other)?)?;
            out.write_all(&[0xFF, 0xD9])
        }

        fn decode_jpeg(
            &self,
            payload: &[u8],
            layout: PixelLayout,
            expected_width: i32,
            expected_height: i32,
        ) -> Result<PixelBuffer, PreviewCacheError> {
            let invalid = |text: &str| PreviewCacheError::InvalidData(text.to_owned());
            if payload.len() < 12 || payload[..2] != [0xFF, 0xD8] {
                return Err(invalid("not a raw payload"));
            }
            let width = i32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
            let height = i32::from_le_bytes([payload[6], payload[7], payload[8], payload[9]]);
            if (width, height) != (expected_width, expected_height) {
                return Err(invalid("size mismatch"));
            }
            let mut pixels = PixelBuffer::allocate(width, height, layout).map_err(|e| invalid(&e.to_string()))?;
            let body = &payload[10..payload.len() - 2];
            let target = pixels.bytes_mut().map_err(|e| invalid(&e.to_string()))?;
            if body.len() != target.len() {
                return Err(invalid("payload length mismatch"));
            }
            target.copy_from_slice(body);
            Ok(pixels)
        }
    }

    struct Fixture {
        _root: TempRoot,
        dir: PathBuf,
        log: Arc<RecordingLog>,
        metrics: Arc<ReviewMetrics>,
        payload: Arc<RawPayloadCodec>,
        service: PreviewImageService,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            Self::with(label, Arc::new(PixelsCodec), RawPayloadCodec::default(), false)
        }

        fn with(label: &str, codec: Arc<dyn PlatformImageCodec>, payload: RawPayloadCodec, disabled: bool) -> Self {
            Self::with_capacity(label, codec, payload, disabled, 1 << 30)
        }

        fn with_capacity(
            label: &str,
            codec: Arc<dyn PlatformImageCodec>,
            payload: RawPayloadCodec,
            disabled: bool,
            disk_capacity: i64,
        ) -> Self {
            let root = TempRoot::new(label);
            let dir = root.join("cache");
            let log = Arc::new(RecordingLog::new());
            let metrics = Arc::new(ReviewMetrics::new());
            let payload = Arc::new(payload);
            let mut config = PreviewServiceConfig::new(
                metrics.clone(),
                Arc::new(|| false),
                Arc::new(|| DecodeBox::new(512, 512)),
                codec,
                payload.clone(),
            );
            config.capacity_bytes = 64 * 1024 * 1024;
            config.disk_cache_directory = Some(dir.clone());
            config.disable_disk_cache_override = Some(disabled);
            config.disk_cache_capacity_bytes = disk_capacity;
            config.decoder = Some(Arc::new(NeverDecoder));
            config.physical_memory_bytes = 32 * 1024 * 1024 * 1024;
            config.log = Some(log.clone());
            let service = PreviewImageService::new(config).expect("the service builds");
            assert!(service.wait_startup_cleanup(TIMEOUT));
            Self {
                _root: root,
                dir,
                log,
                metrics,
                payload,
                service,
            }
        }

        fn inner(&self) -> &Arc<ServiceInner> {
            &self.service.inner
        }

        /// The hand-off of the decode path: queue under the CURRENT epoch.
        fn persist(&self, image: PreviewImage, path: &Path) {
            let epoch = self.inner().current_epoch();
            self.inner().queue_persist(image, path.to_path_buf(), epoch);
        }

        fn path(&self, name: &str) -> PathBuf {
            disk_cache_path(&self.dir, &key(name))
        }

        fn shutdown(&self) {
            assert!(self.service.shutdown_persist_workers_timeout(TIMEOUT), "workers exit");
        }

        fn entries(&self) -> Vec<PathBuf> {
            let Ok(listing) = std::fs::read_dir(&self.dir) else {
                return Vec::new();
            };
            let mut entries: Vec<PathBuf> = listing
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "pv4"))
                .collect();
            entries.sort();
            entries
        }

        fn errors(&self) -> Vec<String> {
            self.log
                .entries()
                .into_iter()
                .filter(|(level, _)| *level == LogLevel::Error)
                .map(|(_, text)| text)
                .collect()
        }

        fn owners(&self) -> HashMap<String, (i64, i32)> {
            lock(&self.inner().persist.owner_epochs).clone()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.service.shutdown_persist_workers_timeout(TIMEOUT);
            let _ = self.service.wait_for_prune(TIMEOUT);
        }
    }

    /// A key with length and write time set (no disk access).
    fn key(name: &str) -> ImageCacheKey {
        let mut entry = CatalogEntry::new(format!("C:\\Pics\\{name}")).expect("path");
        entry.length = Some(1234);
        entry.last_write_utc = DateTimeUtc::from_ticks(638_000_000_000_000_000);
        ImageCacheKey::create_for_entry(
            &FakeFileSystem::new(),
            &entry,
            false,
            DecodeBox::new(2400, 1350),
            KeyOptions::default(),
        )
        .expect("key")
    }

    fn exif() -> ExifSummary {
        ExifSummary {
            date_taken: DateTimeUtc::from_ticks(638_501_690_020_000_000),
            camera_make: Some("Canon".to_owned()),
            camera_model: Some("Canon EOS R5".to_owned()),
            lens_model: Some("RF24-70mm F2.8 L IS USM".to_owned()),
            iso: Some(400),
            focal_length: Some(ExifRational::new(50, 1)),
            f_number: Some(ExifRational::new(28, 10)),
            exposure_time: Some(ExifRational::new(1, 250)),
        }
    }

    fn options() -> DecodedImageOptions {
        DecodedImageOptions {
            downscaled: true,
            orientation: 1,
            actual_backend: DecoderBackend::Wpf,
            original_width: 0,
            original_height: 0,
            exif: None,
            is_degraded_fallback: false,
        }
    }

    /// A 120 x 80 downscaled preview (orientation 6, TurboJpeg, original 6000 x 4000; C# `PixelDecoder`) whose pixel
    /// bytes follow a pattern; `alpha` sets every 4th byte.
    fn image_with(layout: PixelLayout, alpha: impl Fn(i32, i32) -> u8, exif: Option<ExifSummary>) -> PreviewImage {
        let (width, height) = (120, 80);
        let mut pixels = PixelBuffer::allocate(width, height, layout).expect("buffer");
        for y in 0..height {
            let row = pixels.get_row_mut(y).expect("row");
            for x in 0..width {
                let i = usize::try_from(x * 4).expect("index");
                let seed = u8::try_from((x * 7 + y * 13) % 251).expect("byte");
                row[i] = seed;
                row[i + 1] = seed.wrapping_add(1);
                row[i + 2] = seed.wrapping_add(2);
                row[i + 3] = alpha(x, y);
            }
        }
        let options = DecodedImageOptions {
            orientation: 6,
            actual_backend: DecoderBackend::TurboJpeg,
            original_width: 6000,
            original_height: 4000,
            exif,
            ..options()
        };
        let estimated = pixels.byte_count();
        let image = DecodedImage::new(
            PlatformImage::Pixels(Arc::new(pixels)),
            width,
            height,
            estimated,
            options,
        );
        Arc::new(image.expect("image"))
    }

    fn opaque() -> PreviewImage {
        image_with(PixelLayout::Bgr32, |_, _| 255, None)
    }

    /// A hook that counts its calls.
    fn counting_hook(calls: &Arc<AtomicUsize>) -> TestHook {
        let calls = calls.clone();
        Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
        })
    }

    /// A hook that parks on `gate` (bounded by [`TIMEOUT`]).
    fn parking_hook(gate: &Arc<Gate>) -> TestHook {
        let gate = gate.clone();
        Arc::new(move || {
            gate.wait();
        })
    }

    /// A one-shot gate the hook parks on (bounded by [`TIMEOUT`]).
    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        changed: Condvar,
    }

    impl Gate {
        fn open(&self) {
            *lock(&self.open) = true;
            self.changed.notify_all();
        }

        fn wait(&self) -> bool {
            let guard = lock(&self.open);
            let (guard, _) = self
                .changed
                .wait_timeout_while(guard, TIMEOUT, |open| !*open)
                .unwrap_or_else(PoisonError::into_inner);
            *guard
        }
    }

    fn with(bytes: &[u8], change: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut copy = bytes.to_vec();
        change(&mut copy);
        copy
    }

    // ---- identity ---------------------------------------------------------------------------------------------------

    // Rust-only: the exact identity text of a standard key and of a RAW-preview key (D-06; the golden pins the hashes).
    #[test]
    fn disk_cache_identity_text_examples() {
        let standard = key("A.jpg");
        assert_eq!(
            disk_cache_identity(&standard),
            "preview-v5-box|C:\\PICS\\A.JPG|1234|638000000000000000|False|2400x1350|True|Wpf"
        );
        let mut entry = CatalogEntry::new("C:\\Pics\\A.jpg").expect("path");
        entry.length = Some(1234);
        entry.last_write_utc = DateTimeUtc::from_ticks(638_000_000_000_000_000);
        let options = KeyOptions {
            source_kind: ImageSourceKind::RAW_PREVIEW,
            ..KeyOptions::default()
        };
        let raw = ImageCacheKey::create_for_entry(
            &FakeFileSystem::new(),
            &entry,
            false,
            DecodeBox::new(2400, 1350),
            options,
        )
        .expect("key");
        assert_eq!(
            disk_cache_identity(&raw),
            "preview-v5-box|C:\\PICS\\A.JPG|1234|638000000000000000|False|2400x1350|True|Wpf|kind1"
        );
        let original = ImageCacheKey::create_original_with_kind(&standard, 255);
        assert_eq!(
            disk_cache_identity(&original),
            "preview-v5-box|C:\\PICS\\A.JPG|1234|638000000000000000|True|0x0|True|Wpf|kind255"
        );
        assert_eq!(dotnet_bool(true), "True");
        assert_eq!(dotnet_bool(false), "False");
        let name = disk_cache_file_name(&standard);
        assert_eq!(name.len(), 64 + 4);
        assert!(name.ends_with(".pv4"));
        assert!(
            name[..64]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
        );
    }

    // Rust-only: `Path.Combine` (.NET Core) for the entry path: one backslash unless the directory already ends in a
    // separator; a bare drive `D:` gets one too (unlike `Path::join`); an empty directory gives the name.
    #[test]
    fn combine_follows_dotnet_path_combine() {
        let text = |dir: &str| combine(Path::new(dir), "x.pv4").into_os_string();
        assert_eq!(text(""), "x.pv4");
        assert_eq!(text("D:"), r"D:\x.pv4");
        assert_eq!(text(r"C:\a\"), r"C:\a\x.pv4");
        assert_eq!(text(r"C:\a"), r"C:\a\x.pv4");
        assert_eq!(text("C:/a/"), "C:/a/x.pv4");
        assert_eq!(text("C:/a"), r"C:/a\x.pv4");
        assert_eq!(text("rel"), r"rel\x.pv4");
        assert_eq!(text(r"\\server\share"), r"\\server\share\x.pv4");
        assert_eq!(
            disk_cache_path(Path::new("D:"), &key("A.jpg")).into_os_string(),
            format!(r"D:\{}", disk_cache_file_name(&key("A.jpg"))).as_str()
        );
    }

    // ---- disabled disk cache ----------------------------------------------------------------------------------------

    // Rust-only (semantics 2): disabled = no path (no hashing), no probe, no startup cleanup, workers still started.
    #[test]
    fn disabled_disk_cache_has_no_path_and_no_startup_cleanup() {
        let fx = Fixture::with(
            "w414c-disabled",
            Arc::new(PixelsCodec),
            RawPayloadCodec::default(),
            true,
        );
        assert!(fx.inner().disk_cache_path_for(&key("A.jpg")).is_none());
        assert!(fx.service.wait_startup_cleanup(Duration::ZERO));
        assert_eq!(lock(&fx.inner().persist.queue).live_workers, PERSIST_WORKER_COUNT);
        // An entry file at the key's path is not even looked at.
        std::fs::create_dir_all(&fx.dir).expect("dir");
        std::fs::write(fx.path("A.jpg"), [1u8]).expect("entry");
        assert!(!fx.service.has_disk_cached_preview(&key("A.jpg")));
        let enabled = Fixture::new("w414c-enabled");
        assert_eq!(
            enabled.inner().disk_cache_path_for(&key("A.jpg")),
            Some(enabled.path("A.jpg"))
        );
        assert_eq!(lock(&enabled.inner().persist.queue).live_workers, PERSIST_WORKER_COUNT);
    }

    // ---- persist: C# twins driven through the hand-off ----------------------------------------------------------------

    // C#: PreviewImageServicePersistStaleTests.Persist_ClearCacheDuringWrite_DeletesStaleFile (white-box hand-off twin)
    #[test]
    fn persist_clear_cache_during_write_deletes_stale_file() {
        let fx = Fixture::new("w414c-stale");
        let calls = Arc::new(AtomicUsize::new(0));
        let weak = Arc::downgrade(&fx.service.inner);
        let hook_calls = calls.clone();
        fx.service.set_after_persist_write_for_tests(Some(Arc::new(move || {
            if hook_calls.fetch_add(1, Ordering::SeqCst) == 0
                && let Some(inner) = weak.upgrade()
            {
                // ClearCache: bumps the epoch, touches no disk file.
                let _gate = lock(&inner.lifecycle_gate);
                inner.bump_epoch_under_gate();
            }
        })));
        let path = fx.path("A.jpg");
        fx.persist(opaque(), &path);
        fx.shutdown();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!fx.service.has_disk_cached_preview(&key("A.jpg")));
        assert!(fx.entries().is_empty());
        assert!(fx.owners().is_empty(), "the claim was released");
        assert!(fx.errors().is_empty());
    }

    // C#: PreviewImageServicePersistStaleTests.Persist_NoClearCache_KeepsFile (white-box hand-off twin)
    #[test]
    fn persist_no_clear_cache_keeps_file() {
        let fx = Fixture::new("w414c-keep");
        let calls = Arc::new(AtomicUsize::new(0));
        fx.service
            .set_after_persist_write_for_tests(Some(counting_hook(&calls)));
        fx.persist(opaque(), &fx.path("A.jpg"));
        fx.shutdown();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(fx.service.has_disk_cached_preview(&key("A.jpg")));
        assert_eq!(fx.entries(), vec![fx.path("A.jpg")]);
        assert!(fx.owners().is_empty());
        assert!(fx.service.wait_for_prune(TIMEOUT));
    }

    // C#: PreviewImageServicePersistTwoWorkerTests.StaleWorkerCleanup_AfterNewerWorkerWroteSamePath_KeepsNewerFile
    #[test]
    fn stale_worker_cleanup_after_newer_worker_wrote_same_path_keeps_newer_file() {
        let fx = Fixture::new("w414c-two-workers");
        let first_done = Arc::new(Gate::default());
        let release_first = Arc::new(Gate::default());
        let second_done = Arc::new(Gate::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let timed_out = Arc::new(AtomicUsize::new(0));
        {
            let (first_done, release_first, second_done, calls, timed_out) = (
                first_done.clone(),
                release_first.clone(),
                second_done.clone(),
                calls.clone(),
                timed_out.clone(),
            );
            fx.service.set_after_persist_write_for_tests(Some(Arc::new(move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    first_done.open(); // worker 1 (old epoch) wrote the file and parks before its stale re-check
                    if !release_first.wait() {
                        timed_out.fetch_add(1, Ordering::SeqCst);
                    }
                } else {
                    second_done.open(); // worker 2 (new epoch) wrote the same path
                }
            })));
        }
        let path = fx.path("A.jpg");
        fx.persist(opaque(), &path);
        assert!(first_done.wait());
        fx.service.clear_cache();
        fx.service.clear_disk(); // newer epoch; removes worker 1's file
        assert!(fx.entries().is_empty());
        fx.persist(opaque(), &path);
        assert!(second_done.wait());
        assert!(fx.service.has_disk_cached_preview(&key("A.jpg")));
        release_first.open();
        fx.shutdown();
        assert_eq!(timed_out.load(Ordering::SeqCst), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(
            fx.service.has_disk_cached_preview(&key("A.jpg")),
            "the newer worker's file survives"
        );
        assert!(fx.owners().is_empty());
    }

    // C#: PreviewImageServicePixelCodecTests.PixelBufferPreview_PersistsAndReadsBack_WithEitherCodec (pixel-buffer
    // codec half; the WPF codec and `LegacyWpfPreviewCache` halves have no Rust twin: there is no WPF in the port)
    #[test]
    fn pixel_buffer_preview_persists_and_reads_back() {
        let fx = Fixture::new("w414c-readback");
        let produced = opaque();
        fx.persist(produced.clone(), &fx.path("photo.jpg"));
        fx.shutdown();
        let entry = fx.entries();
        assert_eq!(entry, vec![fx.path("photo.jpg")]);
        let from_disk = fx.inner().try_read_disk_entry(&entry[0]).expect("a disk hit");
        assert_eq!(fx.metrics.snapshot().disk_cache_hits, 1);
        let PlatformImage::Pixels(pixels) = from_disk.platform_image() else {
            panic!("the pixels codec serves pixel buffers");
        };
        assert_eq!(
            (pixels.width(), pixels.height(), pixels.layout()),
            (120, 80, PixelLayout::Bgr32)
        );
        assert_eq!(
            (
                from_disk.orientation(),
                from_disk.actual_backend(),
                from_disk.original_width(),
                from_disk.original_height()
            ),
            (6, DecoderBackend::TurboJpeg, 6000, 4000)
        );
        assert!(from_disk.downscaled());
        let PlatformImage::Pixels(original) = produced.platform_image() else {
            panic!("pixels");
        };
        assert_eq!(pixels.bytes().expect("bytes"), original.bytes().expect("bytes"));
        assert_eq!(from_disk.estimated_bytes(), 120 * 80 * 4);
    }

    // C#: PreviewImageServicePixelCodecTests.Pbgra32Preview_PersistsOnlyWhenOpaque (theory: true -> 1, false -> 0)
    #[test]
    fn pbgra32_preview_persists_only_when_opaque() {
        for (opaque, expected) in [(true, 1usize), (false, 0usize)] {
            let fx = Fixture::new("w414c-pbgra");
            let alpha = move |x: i32, y: i32| if !opaque && x == 0 && y == 40 { 0 } else { 255 };
            fx.persist(image_with(PixelLayout::Pbgra32, alpha, None), &fx.path("photo.jpg"));
            fx.shutdown();
            assert_eq!(fx.entries().len(), expected, "opaque = {opaque}");
            assert!(fx.errors().is_empty());
        }
    }

    // C#: PreviewImageServicePixelCodecTests.UnconvertiblePlatformImage_IsNotPersisted_NoError
    #[test]
    fn unconvertible_platform_image_is_not_persisted_no_error() {
        let fx = Fixture::new("w414c-foreign");
        let foreign: Arc<dyn std::any::Any + Send + Sync> = Arc::new(7u8);
        let image = DecodedImage::new(PlatformImage::Opaque(foreign), 8, 8, 256, options()).expect("image");
        fx.persist(Arc::new(image), &fx.path("photo.jpg"));
        fx.shutdown();
        assert!(fx.entries().is_empty());
        assert!(fx.errors().is_empty());
        assert!(fx.owners().is_empty(), "nothing was claimed");
        assert_eq!(fx.payload.encodes.load(Ordering::SeqCst), 0);
    }

    // C#: PreviewImageServiceDiskCacheTests.{OpaquePng_IsStillPersisted, OpaqueRgbaPng_IsPersisted,
    // HalfTransparentRgbaPng_IsNotPersisted, PartiallyTransparentRgbaPng_IsNotPersisted,
    // IndexedPng_PersistedOnlyWhenPaletteOpaque, TransparentPng_IsNotPersistedToDiskCache} as pixel-buffer shapes through
    // the hand-off (the decode-path twins are in tests/preview_service_disk.rs).
    #[test]
    fn alpha_shapes_persist_only_when_every_pixel_is_opaque() {
        type Alpha = fn(i32, i32) -> u8;
        let shapes: [(&str, PixelLayout, Alpha, usize); 7] = [
            ("opaque-bgr32", PixelLayout::Bgr32, |_, _| 0, 1), // Bgr32 has no alpha: the 4th byte is ignored
            ("rgba-opaque", PixelLayout::Pbgra32, |_, _| 255, 1),
            ("rgba-half", PixelLayout::Pbgra32, |_, _| 128, 0),
            (
                "rgba-corner",
                PixelLayout::Pbgra32,
                |x, y| if x >= 60 && y >= 40 { 0 } else { 255 },
                0,
            ),
            ("palette-opaque", PixelLayout::Pbgra32, |_, _| 255, 1),
            (
                "palette-translucent",
                PixelLayout::Pbgra32,
                |x, _| if x % 30 == 0 { 0 } else { 255 },
                0,
            ),
            ("transparent", PixelLayout::Pbgra32, |_, _| 0, 0),
        ];
        for (name, layout, alpha, expected) in shapes {
            let fx = Fixture::new("w414c-shapes");
            let image = image_with(layout, alpha, None);
            fx.persist(image.clone(), &fx.path("photo.jpg"));
            fx.shutdown();
            assert_eq!(fx.entries().len(), expected, "{name}");
            // The returned image keeps its alpha: the persist never touches the shared buffer.
            if let PlatformImage::Pixels(pixels) = image.platform_image() {
                assert_eq!(pixels.layout(), layout, "{name}");
                assert_eq!(pixels.get_row(79).expect("row")[119 * 4 + 3], alpha(119, 79), "{name}");
            }
        }
    }

    // C#: CacheTempFileAndDiskProbeTests.HasDiskCachedPreview_ReflectsPersistedEntry (white-box hand-off twin)
    #[test]
    fn has_disk_cached_preview_reflects_persisted_entry() {
        let fx = Fixture::new("w414c-probe");
        assert!(!fx.service.has_disk_cached_preview(&key("A.jpg")));
        fx.persist(opaque(), &fx.path("A.jpg"));
        fx.shutdown();
        assert!(fx.service.has_disk_cached_preview(&key("A.jpg")));
        assert!(!fx.service.has_disk_cached_preview(&key("B.jpg")));
        // A directory at the entry path is not an entry (C# `File.Exists`).
        std::fs::create_dir_all(fx.path("B.jpg")).expect("dir");
        assert!(!fx.service.has_disk_cached_preview(&key("B.jpg")));
        assert!(!fx.inner().disk_entry_exists(&fx.path("B.jpg")));
        assert!(fx.inner().disk_entry_exists(&fx.path("A.jpg")));
    }

    // C#: PreviewCacheExifTests.DiskHitCarriesExifWithoutSourceRead (persist + disk-hit half)
    #[test]
    fn disk_hit_carries_exif() {
        let fx = Fixture::new("w414c-exif");
        fx.persist(
            image_with(PixelLayout::Bgr32, |_, _| 255, Some(exif())),
            &fx.path("A.jpg"),
        );
        fx.shutdown();
        let from_disk = fx.inner().try_read_disk_entry(&fx.path("A.jpg")).expect("hit");
        assert_eq!(from_disk.exif(), Some(&exif()));
    }

    // C#: PreviewImageServiceMutationTests.DiskCacheHit_DoesNotRewriteTheEntry (the read half: a hit writes nothing)
    #[test]
    fn disk_hit_does_not_rewrite_the_entry() {
        let fx = Fixture::new("w414c-hit-no-write");
        fx.persist(opaque(), &fx.path("A.jpg"));
        fx.shutdown();
        let entry = fx.path("A.jpg");
        let sentinel = std::time::UNIX_EPOCH + Duration::from_secs(981_173_106); // 2001-02-03 04:05:06 UTC
        std::fs::File::options()
            .write(true)
            .open(&entry)
            .and_then(|file| file.set_modified(sentinel))
            .expect("set the write time");
        let encodes = fx.payload.encodes.load(Ordering::SeqCst);
        fx.inner().try_read_disk_entry(&entry).expect("hit");
        assert_eq!(
            std::fs::metadata(&entry).and_then(|m| m.modified()).expect("mtime"),
            sentinel
        );
        assert_eq!(fx.payload.encodes.load(Ordering::SeqCst), encodes);
        assert_eq!(fx.entries().len(), 1);
        assert_eq!(fx.inner().persist.len(), 0, "a hit queues nothing");
    }

    // C#: PreviewImageServiceFaultTests.BrokenEntry_IsAMiss (the read half: every broken shape is a cache-entry failure
    // and no hit; the delete + source decode is W4-14b's and is pinned by the decode-path twin)
    #[test]
    fn broken_entries_are_cache_entry_failures() {
        let fx = Fixture::new("w414c-broken");
        fx.persist(opaque(), &fx.path("A.jpg"));
        fx.shutdown();
        let entry = fx.path("A.jpg");
        let valid = std::fs::read(&entry).expect("entry bytes");
        let shapes: Vec<(&str, Vec<u8>)> = vec![
            ("zero-bytes", Vec::new()),
            ("one-byte", vec![b'P']),
            ("header-only", valid[..24].to_vec()),
            (
                "cut-mid-payload",
                valid[..valid.len() - (valid.len() / 3).max(4)].to_vec(),
            ),
            ("bad-magic", with(&valid, |b| b[0] = b'X')),
            ("future-version", with(&valid, |b| b[4] = b[4].wrapping_add(1))),
            ("orientation-0", with(&valid, |b| b[6] = 0)),
            ("backend-255", with(&valid, |b| b[5] = 255)),
            ("width-negative", with(&valid, |b| b[11] = 0x80)),
            (
                "exif-length-past-eof",
                with(&valid, |b| {
                    b[24] = 0xFF;
                    b[25] = 0x03;
                }),
            ),
            (
                "garbage-payload",
                with(&valid, |b| {
                    for (i, byte) in b.iter_mut().enumerate().skip(26) {
                        *byte = u8::try_from(i * 31 % 256).expect("byte");
                    }
                }),
            ),
        ];
        for (name, bytes) in shapes {
            std::fs::write(&entry, &bytes).expect("write broken entry");
            match fx.inner().try_read_disk_entry(&entry) {
                Ok(_) => panic!("{name}: a broken entry must not be a hit"),
                Err(error) => assert!(error.is_cache_entry_failure(), "{name}: {error:?}"),
            }
        }
        assert_eq!(
            fx.metrics.snapshot().disk_cache_hits,
            0,
            "no hit recorded for a broken entry"
        );
    }

    // C#: PreviewImageServiceFaultTests.DamagedExifSegment_PixelsIntact_IsAHit (read half). The C# damages the APP1
    // segment inside the JPEG payload (WIC, W4-11e); here the entry's own EXIF block is damaged (same length): the pixels
    // are intact, so the read is a hit with the clean pixels and no EXIF.
    #[test]
    fn damaged_exif_block_pixels_intact_is_a_hit() {
        let fx = Fixture::new("w414c-damaged-exif");
        let produced = image_with(PixelLayout::Bgr32, |_, _| 255, Some(exif()));
        fx.persist(produced.clone(), &fx.path("A.jpg"));
        fx.shutdown();
        let entry = fx.path("A.jpg");
        let mut bytes = std::fs::read(&entry).expect("entry");
        let exif_len = usize::from(u16::from_le_bytes([bytes[24], bytes[25]]));
        assert!(exif_len > 0, "the entry carries an EXIF block");
        bytes[26..26 + exif_len].fill(0xEE);
        std::fs::write(&entry, &bytes).expect("damage");
        let from_disk = fx.inner().try_read_disk_entry(&entry).expect("still a hit");
        assert_eq!(from_disk.exif(), None);
        let (PlatformImage::Pixels(served), PlatformImage::Pixels(clean)) =
            (from_disk.platform_image(), produced.platform_image())
        else {
            panic!("pixels");
        };
        assert_eq!(served.bytes().expect("bytes"), clean.bytes().expect("bytes"));
    }

    // C#: PreviewImageServiceFaultTests.LockedEntry_FallsBackToSource (read half: an entry held exclusively by another
    // handle is a cache-entry failure, so the decode path deletes (fails, logged) and decodes the source)
    #[cfg(windows)]
    #[test]
    fn locked_entry_is_a_cache_entry_failure() {
        use std::os::windows::fs::OpenOptionsExt;
        let fx = Fixture::new("w414c-locked");
        fx.persist(opaque(), &fx.path("A.jpg"));
        fx.shutdown();
        let entry = fx.path("A.jpg");
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&entry)
            .expect("hold the entry exclusively");
        let error = match fx.inner().try_read_disk_entry(&entry) {
            Ok(_) => panic!("a locked entry cannot be read"),
            Err(error) => error,
        };
        assert!(error.is_cache_entry_failure(), "{error:?}");
        assert_eq!(fx.metrics.snapshot().disk_cache_hits, 0);
        drop(held);
        fx.inner().try_read_disk_entry(&entry).expect("readable again");
    }

    // ---- persist: Rust-only ------------------------------------------------------------------------------------------

    // Rust-only: both workers blocked after their write, 16 more requests fill the queue, 2 more are DROPPED; after the
    // release exactly 2 + 16 = 18 writes happen (C# `PersistQueueBound_IsPinnedAt18`, R06).
    #[test]
    fn persist_queue_drops_requests_beyond_16_while_workers_are_busy() {
        let fx = Fixture::new("w414c-bound");
        let release = Arc::new(Gate::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let started_tx = Mutex::new(started_tx);
        {
            let (release, calls) = (release.clone(), calls.clone());
            fx.service.set_after_persist_write_for_tests(Some(Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                let _ = lock(&started_tx).send(());
                release.wait();
            })));
        }
        fx.persist(opaque(), &fx.path("w0.jpg"));
        fx.persist(opaque(), &fx.path("w1.jpg"));
        for _ in 0..PERSIST_WORKER_COUNT {
            started_rx
                .recv_timeout(TIMEOUT)
                .expect("both workers parked in the hook");
        }
        for i in 0..PERSIST_QUEUE_CAPACITY + 2 {
            fx.persist(opaque(), &fx.path(&format!("q{i}.jpg")));
        }
        assert_eq!(fx.inner().persist.len(), PERSIST_QUEUE_CAPACITY);
        release.open();
        fx.shutdown();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            PERSIST_QUEUE_CAPACITY + PERSIST_WORKER_COUNT
        );
        assert_eq!(fx.entries().len(), 18);
        assert!(
            !fx.path("q16.jpg").exists() && !fx.path("q17.jpg").exists(),
            "the last two were dropped"
        );
        assert!(fx.path("q15.jpg").exists());
        assert!(fx.errors().is_empty(), "a dropped request is not logged");
    }

    // Rust-only: shutdown completes the queue, the workers drain everything queued, then it returns; later pushes are
    // refused.
    #[test]
    fn shutdown_drains_the_queue_then_returns() {
        let fx = Fixture::new("w414c-drain");
        let release = Arc::new(Gate::default());
        fx.service
            .set_after_persist_write_for_tests(Some(parking_hook(&release)));
        for i in 0..6 {
            fx.persist(opaque(), &fx.path(&format!("d{i}.jpg")));
        }
        assert!(
            !fx.service.shutdown_persist_workers_timeout(Duration::from_millis(50)),
            "still draining"
        );
        release.open();
        fx.service.shutdown_persist_workers();
        assert_eq!(fx.entries().len(), 6);
        assert_eq!(lock(&fx.inner().persist.queue).live_workers, 0);
        fx.persist(opaque(), &fx.path("late.jpg"));
        assert_eq!(fx.inner().persist.len(), 0, "a completed queue accepts nothing");
        assert!(
            fx.service.shutdown_persist_workers_timeout(Duration::ZERO),
            "idempotent"
        );
        fx.service.shutdown_persist_workers();
    }

    // Rust-only (C# `NoteWritten` + `SchedulePrune` after a current write; PreviewImageServiceDiskCacheTests.
    // DiskCacheIsPrunedToConfiguredQuota through the hand-off): with a 1-byte quota every written entry is pruned.
    #[test]
    fn a_current_write_is_noted_and_pruned_to_the_quota() {
        let fx = Fixture::with_capacity(
            "w414c-quota",
            Arc::new(PixelsCodec),
            RawPayloadCodec::default(),
            false,
            1,
        );
        fx.persist(opaque(), &fx.path("A.jpg"));
        fx.shutdown();
        assert!(fx.service.wait_for_prune(TIMEOUT));
        assert!(
            fx.entries().is_empty(),
            "the write scheduled a prune that enforced the quota"
        );
        let generous = Fixture::new("w414c-quota-generous");
        generous.persist(opaque(), &generous.path("A.jpg"));
        generous.shutdown();
        assert!(generous.service.wait_for_prune(TIMEOUT));
        assert_eq!(generous.entries().len(), 1);
    }

    // Rust-only: a failed write is logged once with the C# text, releases its claim, and the other request still lands.
    #[test]
    fn persist_worker_survives_a_failed_write() {
        let fx = Fixture::with(
            "w414c-fail",
            Arc::new(PixelsCodec),
            RawPayloadCodec::failing_height(4),
            false,
        );
        let small = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).expect("buffer");
        let failing = DecodedImage::new(PlatformImage::Pixels(Arc::new(small)), 4, 4, 64, options()).expect("image");
        fx.persist(Arc::new(failing), &fx.path("first.jpg"));
        fx.persist(opaque(), &fx.path("second.jpg"));
        fx.shutdown();
        assert_eq!(fx.entries(), vec![fx.path("second.jpg")]);
        let errors = fx.errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        let expected = format!("Preview disk cache write failed: {}: ", fx.path("first.jpg").display());
        assert!(errors[0].starts_with(&expected), "{errors:?}");
        assert!(
            errors[0].contains("scripted encode failure"),
            "the error is attached: {errors:?}"
        );
        assert!(fx.owners().is_empty(), "the failed write released its claim");
    }

    // Rust-only: a panicking hook is caught per request (logged as a failed write), the claim is released and the next
    // write of the same path is a normal, current write.
    #[test]
    fn persist_worker_survives_a_panicking_hook() {
        let fx = Fixture::new("w414c-panic");
        let calls = Arc::new(AtomicUsize::new(0));
        {
            let calls = calls.clone();
            fx.service.set_after_persist_write_for_tests(Some(Arc::new(move || {
                assert!(calls.fetch_add(1, Ordering::SeqCst) != 0, "hook panic");
            })));
        }
        let path = fx.path("A.jpg");
        fx.persist(opaque(), &path);
        assert!(fx.service.shutdown_persist_workers_timeout(TIMEOUT));
        assert!(fx.owners().is_empty(), "the panicking write released its claim");
        let errors = fx.errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].starts_with(&format!("Preview disk cache write failed: {}: ", path.display())));
        assert!(errors[0].contains("hook panic"), "{errors:?}");
        // The next write of the same path, under the current epoch, is not stale: it lands and is noted.
        let request = PersistRequest {
            image: opaque(),
            cache_path: path.clone(),
            epoch: fx.inner().current_epoch(),
        };
        std::fs::remove_file(&path).expect("remove the first file");
        fx.inner().persist_one(&request).expect("second write");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(path.exists());
        assert!(fx.owners().is_empty());
    }

    // Rust-only: the worker loop keeps going after a panic (the two workers are never restarted).
    #[test]
    fn persist_worker_loop_continues_after_a_panic() {
        let fx = Fixture::new("w414c-panic-loop");
        let calls = Arc::new(AtomicUsize::new(0));
        {
            let calls = calls.clone();
            fx.service.set_after_persist_write_for_tests(Some(Arc::new(move || {
                assert!(calls.fetch_add(1, Ordering::SeqCst) >= 2, "hook panic");
            })));
        }
        for i in 0..6 {
            fx.persist(opaque(), &fx.path(&format!("p{i}.jpg")));
        }
        fx.shutdown();
        assert_eq!(calls.load(Ordering::SeqCst), 6, "both workers survived their panic");
        assert_eq!(fx.errors().len(), 2);
    }

    // Rust-only: `to_pixels` failing with anything but InvalidArgument, an empty lease and a disposed buffer are failures.
    #[test]
    fn persist_pixel_failures_are_logged() {
        for empty_lease in [false, true] {
            let codec = Arc::new(BrokenCodec { empty_lease });
            let fx = Fixture::with("w414c-pixels", codec, RawPayloadCodec::default(), false);
            fx.persist(opaque(), &fx.path("A.jpg"));
            fx.shutdown();
            assert!(fx.entries().is_empty());
            let errors = fx.errors();
            assert_eq!(errors.len(), 1, "empty lease {empty_lease}: {errors:?}");
            assert!(errors[0].starts_with("Preview disk cache write failed: "));
            assert!(fx.owners().is_empty());
        }
        let fx = Fixture::new("w414c-disposed");
        let mut pixels = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).expect("buffer");
        pixels.dispose();
        let image = DecodedImage::new(PlatformImage::Pixels(Arc::new(pixels)), 4, 4, 64, options()).expect("image");
        let failure = fx.inner().persist_one(&PersistRequest {
            image: Arc::new(image),
            cache_path: fx.path("A.jpg"),
            epoch: fx.inner().current_epoch(),
        });
        assert!(matches!(failure, Err(PersistFailure::Opacity(_))), "{failure:?}");
        assert!(fx.owners().is_empty(), "no claim before the opacity scan");
    }

    // Rust-only (INV-2): a request whose epoch went stale while queued does no I/O and is not logged.
    #[test]
    fn persist_one_with_a_stale_epoch_does_nothing() {
        let fx = Fixture::new("w414c-stale-queued");
        let calls = Arc::new(AtomicUsize::new(0));
        fx.service
            .set_after_persist_write_for_tests(Some(counting_hook(&calls)));
        let request = PersistRequest {
            image: opaque(),
            cache_path: fx.path("A.jpg"),
            epoch: fx.inner().current_epoch(),
        };
        fx.service.clear_cache();
        fx.inner().persist_one(&request).expect("a skip is Ok");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!fx.path("A.jpg").exists());
        assert!(fx.owners().is_empty());
        assert_eq!(fx.payload.encodes.load(Ordering::SeqCst), 0);
    }

    // Rust-only (INV-2): `queue_persist` with an epoch that is no longer current queues nothing.
    #[test]
    fn queue_persist_with_a_stale_epoch_queues_nothing() {
        let fx = Fixture::new("w414c-queue-stale");
        let release = Arc::new(Gate::default());
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let started_tx = Mutex::new(started_tx);
        {
            let release = release.clone();
            fx.service.set_after_persist_write_for_tests(Some(Arc::new(move || {
                let _ = lock(&started_tx).send(());
                release.wait();
            })));
        }
        // Park both workers so the queue content is observable.
        fx.persist(opaque(), &fx.path("w0.jpg"));
        fx.persist(opaque(), &fx.path("w1.jpg"));
        for _ in 0..PERSIST_WORKER_COUNT {
            started_rx.recv_timeout(TIMEOUT).expect("both workers parked");
        }
        let stale = fx.inner().current_epoch();
        fx.service.clear_cache();
        fx.inner().queue_persist(opaque(), fx.path("stale.jpg"), stale);
        fx.inner()
            .queue_persist(opaque(), fx.path("stale2.jpg"), stale.wrapping_add(2));
        let current = fx.inner().current_epoch();
        fx.inner().queue_persist(opaque(), fx.path("current.jpg"), current);
        let queued: Vec<PathBuf> = lock(&fx.inner().persist.queue)
            .items
            .iter()
            .map(|r| r.cache_path.clone())
            .collect();
        assert_eq!(queued, vec![fx.path("current.jpg")]);
        release.open();
        fx.shutdown();
        assert!(fx.path("current.jpg").exists());
        assert!(!fx.path("stale.jpg").exists());
    }

    // Rust-only: white-box on claim / release / delete-stale (C1 fix): the entry keeps the NEWEST epoch while an older
    // writer is in flight, so the older one's stale cleanup keeps the newer file; once alone it deletes.
    #[test]
    fn release_keeps_the_newest_epoch_while_an_older_writer_is_in_flight() {
        let fx = Fixture::new("w414c-owners");
        let inner = fx.inner();
        std::fs::create_dir_all(&fx.dir).expect("dir");
        let path = fx.path("A.jpg");
        let write = || std::fs::write(&path, [1u8]).expect("file");
        let owner = |fx: &Fixture| fx.owners().get(&path_key(&path)).copied();

        inner.claim_persist_path(&path, 1); // older writer
        inner.claim_persist_path(&path, 2); // newer writer
        assert_eq!(owner(&fx), Some((2, 2)));
        inner.release_persist_path(&path); // newer finished
        assert_eq!(owner(&fx), Some((2, 1)), "the newest epoch outlives its writer");
        write();
        inner.delete_stale_persist_file(&path, 1);
        assert!(path.exists(), "an older writer never deletes a newer epoch's file");
        inner.release_persist_path(&path);
        assert!(fx.owners().is_empty(), "the last writer removes the entry");
        inner.release_persist_path(&path); // absent: nothing
        assert!(fx.owners().is_empty());

        // No entry: the stale file is deleted. Same epoch: deleted too (only a strictly NEWER epoch protects it).
        inner.delete_stale_persist_file(&path, 1);
        assert!(!path.exists());
        write();
        inner.claim_persist_path(&path, 3);
        inner.delete_stale_persist_file(&path, 3);
        assert!(!path.exists(), "an equal epoch does not protect the file");
        write();
        inner.delete_stale_persist_file(&path, 4);
        assert!(!path.exists(), "an older entry does not protect the file");
        inner.release_persist_path(&path);

        // max(old, new): a later claim with a smaller epoch keeps the larger one; a missing entry counts as 0.
        inner.claim_persist_path(&path, 5);
        inner.claim_persist_path(&path, 3);
        assert_eq!(owner(&fx), Some((5, 2)));
        inner.release_persist_path(&path);
        assert_eq!(owner(&fx), Some((5, 1)));
        inner.release_persist_path(&path);
        inner.claim_persist_path(&path, -7);
        assert_eq!(owner(&fx), Some((0, 1)), "C# Math.Max with the default 0");
        inner.release_persist_path(&path);
        assert!(fx.owners().is_empty());
        assert!(fx.errors().is_empty(), "the stale delete logs nothing");
    }

    // Rust-only: the owner table is OrdinalIgnoreCase (C# `StringComparer.OrdinalIgnoreCase`).
    #[test]
    fn owner_table_is_ordinal_ignore_case() {
        let fx = Fixture::new("w414c-owners-case");
        let inner = fx.inner();
        std::fs::create_dir_all(&fx.dir).expect("dir");
        let upper = fx.dir.join("ABCDEF.PV4");
        let lower = fx.dir.join("abcdef.pv4");
        std::fs::write(&upper, [1u8]).expect("file");
        inner.claim_persist_path(&upper, 9);
        inner.claim_persist_path(&lower, 2);
        assert_eq!(fx.owners().len(), 1, "two spellings share one entry");
        assert_eq!(fx.owners().get(&path_key(&upper)), Some(&(9, 2)));
        inner.delete_stale_persist_file(&lower, 3);
        assert!(
            upper.exists(),
            "the newer epoch claimed under the other spelling protects the file"
        );
        inner.release_persist_path(&lower);
        inner.release_persist_path(&upper);
        assert!(fx.owners().is_empty());
    }

    // Rust-only (C-05): Clear Disk bumps the epoch only: the RAM cache, the original dimensions and the CacheCleared
    // subscribers are untouched.
    #[test]
    fn clear_disk_does_not_touch_the_ram_cache_or_raise_cache_cleared() {
        let fx = Fixture::new("w414c-clear-ram");
        let inner = fx.inner();
        let k = key("A.jpg");
        assert!(inner.publish_if_current(k.clone(), opaque(), inner.current_epoch()));
        inner
            .original_dimensions
            .set(ImageCacheKey::create_original(&k), (6000, 4000));
        let raised = Arc::new(AtomicUsize::new(0));
        let _subscription = fx.service.subscribe_cache_cleared(counting_hook(&raised));
        let before = inner.current_epoch();
        fx.service.clear_disk();
        assert_eq!(inner.current_epoch(), before.wrapping_add(1));
        assert!(fx.service.try_get_cached_preview(&k).is_some());
        assert_eq!(fx.service.try_get_known_original_dimensions(&k), Some((6000, 4000)));
        assert_eq!(raised.load(Ordering::SeqCst), 0);
        // The epoch wraps (D3).
        inner.cache_epoch.store(i64::MAX, Ordering::SeqCst);
        fx.service.clear_disk();
        assert_eq!(inner.current_epoch(), i64::MIN);
    }

    // Rust-only: a disk hit records the hit; a missing entry is an error and records nothing.
    #[test]
    fn disk_hit_records_the_hit_and_a_missing_entry_is_an_error() {
        let fx = Fixture::new("w414c-hit-metrics");
        assert!(fx.inner().try_read_disk_entry(&fx.path("missing.jpg")).is_err());
        assert_eq!(fx.metrics.snapshot().disk_cache_hits, 0);
        fx.persist(opaque(), &fx.path("A.jpg"));
        fx.shutdown();
        fx.inner().try_read_disk_entry(&fx.path("A.jpg")).expect("hit");
        fx.inner().try_read_disk_entry(&fx.path("A.jpg")).expect("hit");
        assert_eq!(fx.metrics.snapshot().disk_cache_hits, 2);
    }

    // Rust-only: the Debug forms name the path and epoch of a request (never the pixels) and hide the queue internals.
    #[test]
    fn debug_forms() {
        let request = PersistRequest {
            image: opaque(),
            cache_path: PathBuf::from("x.pv4"),
            epoch: 42,
        };
        assert_eq!(
            format!("{request:?}"),
            "PersistRequest { cache_path: \"x.pv4\", epoch: 42, .. }"
        );
        assert_eq!(format!("{:?}", PersistState::new()), "PersistState { .. }");
    }

    // Rust-only: the logged error texts (the panic payload of both types, a fallback, the failure variants).
    #[test]
    fn persist_failure_texts() {
        assert_eq!(PersistPanic::from_payload(&"static text").to_string(), "static text");
        assert_eq!(
            PersistPanic::from_payload(&String::from("owned text")).to_string(),
            "owned text"
        );
        assert_eq!(
            PersistPanic::from_payload(&7u8).to_string(),
            "a persist request panicked"
        );
        assert_eq!(
            PersistFailure::Pixels(None).to_string(),
            "The platform codec returned no pixels."
        );
        assert!(PersistFailure::Pixels(None).source().is_none());
        let bad = || PreviewCacheError::InvalidData("bad".to_owned());
        let write = PersistFailure::Write(bad());
        assert!(write.source().is_some());
        assert_eq!(write.to_string(), bad().to_string());
        let pixels = PersistFailure::Pixels(Some(DecodeError::Disposed {
            message: "gone".to_owned(),
        }));
        assert!(pixels.source().is_some());
        assert!(pixels.to_string().contains("gone"));
    }
}
