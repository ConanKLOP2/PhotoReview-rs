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

#![allow(unused_variables, unused_imports, dead_code)] // D11: remove with the last `todo!()`.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use pr_core::abstractions::Log;
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;

use super::deps::{
    self, DecodeError, ImageCacheKey, ImageSourceKind, PreviewCacheError, delete_stale_temp_files, is_fully_opaque,
    preview_file, try_delete,
};
use super::{DecodeThreadKind, PreviewImage, PreviewImageService, ServiceInner, TestHook};

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
    todo!("W4-14c")
}

/// `Convert.ToHexString(SHA256.HashData(UTF8(identity)))` + `.pv4`: 64 UPPER-case hex digits then the extension.
pub fn disk_cache_file_name(key: &ImageCacheKey) -> String {
    todo!("W4-14c")
}

/// C# `GetDiskCachePath(key)`: `Path.Combine(directory, disk_cache_file_name(key))` (`directory.join(name)`).
pub fn disk_cache_path(directory: &Path, key: &ImageCacheKey) -> PathBuf {
    todo!("W4-14c")
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
        todo!("W4-14c")
    }

    /// C# `_persistQueue.Writer.TryWrite(..)` on a `DropWrite` channel: appends unless completed or already holding
    /// [`PERSIST_QUEUE_CAPACITY`] items (then the request is DROPPED silently; returns whether it was queued).
    pub(crate) fn try_push(&self, request: PersistRequest) -> bool {
        todo!("W4-14c")
    }

    /// C# `Writer.TryComplete()`: idempotent; wakes every worker.
    pub(crate) fn complete(&self) {
        todo!("W4-14c")
    }

    /// Blocks until an item is available (`Some`) or the queue is completed AND empty (`None`).
    pub(crate) fn pop(&self) -> Option<PersistRequest> {
        todo!("W4-14c")
    }

    /// Items waiting right now (tests).
    pub(crate) fn len(&self) -> usize {
        todo!("W4-14c")
    }
}

impl PreviewImageService {
    /// C# `ShutdownPersistWorkersAsync()` (blocking): completes the queue (nothing new is accepted) and waits until every
    /// worker drained it and exited. Only for short-lived instances (tests, benchmark runs). Idempotent.
    pub fn shutdown_persist_workers(&self) {
        todo!("W4-14c")
    }

    /// [`Self::shutdown_persist_workers`] with a hang guard: `false` when the workers had not exited after `timeout`.
    pub fn shutdown_persist_workers_timeout(&self, timeout: Duration) -> bool {
        todo!("W4-14c")
    }

    /// C# `AfterPersistWriteForTests` setter.
    #[doc(hidden)]
    pub fn set_after_persist_write_for_tests(&self, hook: Option<TestHook>) {
        todo!("W4-14c")
    }

    /// C# `StartupCleanup.WaitAsync(timeout)`: `true` once the startup cleanup finished (immediately when the disk
    /// cache is disabled: it was never scheduled).
    #[doc(hidden)]
    pub fn wait_startup_cleanup(&self, timeout: Duration) -> bool {
        todo!("W4-14c")
    }

    /// C# `ClearDisk()` (Clear Cache, C-05, INV-2): bump the epoch under the lifecycle gate (in-flight decodes and
    /// queued writes of the old epoch are then dropped), then `disk_store.clear_directory()` (entries + temp files) and
    /// the legacy sweep. A failure of the directory clear is logged `Preview disk cache clear failed: {directory}` with
    /// the error and swallowed; the RAM cache, the original dimensions and `CacheCleared` are NOT touched.
    pub fn clear_disk(&self) {
        todo!("W4-14c")
    }

    /// C# `WaitForPruneAsync(timeout)` (blocking): delegates to the disk store.
    pub fn wait_for_prune(&self, timeout: Duration) -> bool {
        todo!("W4-14c")
    }

    /// C# `HasDiskCachedPreview(key)` (also `IPreloadTarget`): `false` while the disk cache is disabled, else whether
    /// the entry file exists (one existence check, no read).
    pub fn has_disk_cached_preview(&self, key: &ImageCacheKey) -> bool {
        todo!("W4-14c")
    }
}

impl ServiceInner {
    /// Called once by the constructor (W4-14a) after the struct is built: starts [`PERSIST_WORKER_COUNT`] workers
    /// ([`DecodeThreadKind::PersistWorker`], each running `run_persist_worker`) and, unless the disk cache is disabled,
    /// the startup cleanup ([`DecodeThreadKind::StartupCleanup`]: `cleanup_legacy_cache_files` then
    /// `delete_stale_temp_files(dir, clock.utc_now(), LEGACY_CLEANUP_MAX_FILES, log)`, then mark it done). Workers are
    /// started even when the disk cache is disabled (C# does; they simply never receive work).
    pub(crate) fn start_background(self: &Arc<Self>) -> io::Result<()> {
        todo!("W4-14c")
    }

    /// The decode path's `cachePath` (C# `if (!_disableDiskCache) cachePath = GetDiskCachePath(key)`): `None` while the
    /// disk cache is disabled (the key is then never hashed, L02).
    pub(crate) fn disk_cache_path_for(&self, key: &ImageCacheKey) -> Option<PathBuf> {
        todo!("W4-14c")
    }

    /// C# `File.Exists(cachePath)` of the decode path (a file, not a directory; any error is "no").
    pub(crate) fn disk_entry_exists(&self, cache_path: &Path) -> bool {
        todo!("W4-14c")
    }

    /// The disk-hit branch of C# `DecodeAndCache`: `preview_file::read(cache_path, payload_codec)` then
    /// `preview_file::to_decoded_image(entry, platform_codec)`; on success `record_disk_cache_hit`,
    /// `disk_store.note_accessed(cache_path)` and the perf `DiskCacheRead` event (file bytes from the read). The caller
    /// (W4-14b) handles `Err`: a cache-entry failure deletes the entry (`try_delete` WITH the log) and decodes the source;
    /// any other error (`OutOfMemory`, `Canceled`) propagates as `PreviewError::DiskEntry`.
    pub(crate) fn try_read_disk_entry(&self, cache_path: &Path) -> Result<PreviewImage, PreviewCacheError> {
        todo!("W4-14c")
    }

    /// C# `PersistToDiskCache(image, cachePath, cacheEpoch)`: drop when `cache_epoch` is no longer current, else
    /// `try_push` (a full queue drops it).
    pub(crate) fn queue_persist(&self, image: PreviewImage, cache_path: PathBuf, cache_epoch: i64) {
        todo!("W4-14c")
    }

    /// C# `RunPersistWorkerAsync` loop (W4-14c semantics 5): one request at a time until the queue is completed and
    /// drained; every failure is logged `Preview disk cache write failed: {cache_path}` and the loop goes on (a worker
    /// never dies: the 2 workers are never restarted). Decrements `live_workers` on exit.
    pub(crate) fn run_persist_worker(self: &Arc<Self>) {
        todo!("W4-14c")
    }

    /// One request of the loop (semantics 5, steps 1-9). `Ok` covers the skips (stale epoch, foreign platform image,
    /// transparent pixels, went stale mid-write); `Err` is logged by the loop.
    pub(crate) fn persist_one(&self, request: &PersistRequest) -> Result<(), PersistFailure> {
        todo!("W4-14c")
    }

    /// C# `ClaimPersistPath(path, epoch)`: `(max(old epoch, epoch), writers + 1)` (a missing entry counts as `(0, 0)`).
    pub(crate) fn claim_persist_path(&self, cache_path: &Path, epoch: i64) {
        todo!("W4-14c")
    }

    /// C# `ReleasePersistPath(path)`: nothing when absent; remove the entry when `writers <= 1`, else keep its epoch with
    /// `writers - 1` (an older stale writer must still see that a newer epoch rewrote the path).
    pub(crate) fn release_persist_path(&self, cache_path: &Path) {
        todo!("W4-14c")
    }

    /// C# `DeleteStalePersistFile(path, epoch)`: under the owner lock, return when the entry's epoch is NEWER than
    /// `epoch`; else `try_delete(cache_path, None)` (no log, like C#).
    pub(crate) fn delete_stale_persist_file(&self, cache_path: &Path, epoch: i64) {
        todo!("W4-14c")
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

/// C# `CleanupLegacyCacheFiles(directory, log)`: when `directory` is an existing directory, list the FIRST
/// [`LEGACY_CLEANUP_MAX_FILES`] files matching `*.png` (Win32 wildcard semantics, `pr_core::io::physical::
/// matches_win32_expression`, files only, non-recursive, listing order) and `try_delete(path, Some(log))` each, then the
/// same for `*.png.meta`. A listing failure is logged `Legacy preview cache cleanup failed: {directory}` with the error
/// and ends the pass (C# catches `IOException`, `UnauthorizedAccessException`, `ArgumentException`,
/// `NotSupportedException` around the whole body).
pub(crate) fn cleanup_legacy_cache_files(directory: &Path, log: &dyn Log) {
    todo!("W4-14c")
}
