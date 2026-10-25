//! W4-14b part 2 of 2 (port of `PreviewImageService.cs`, C# baseline d92d6aae): preview requests with in-flight
//! de-duplication (C# `GetPreviewCoreAsync` / `GetPreviewOnceAsync`), the viewer lane (start delay, 2 viewer slots,
//! dedicated thread), `DecodeAndCache` (disk hit via W4-14c, source decode, INV-1 verify, epoch-checked publish,
//! original-dimension seeding, persist hand-off), the full-resolution original decode (1-slot gate, never cached) and
//! the original-dimension read. Contract: `contracts/W4-14b.md`.
//!
//! Target path: `crates/pr-imaging/src/service/decode.rs`.

use std::any::Any;
use std::collections::hash_map::Entry;
use std::io::{self, Read, Seek, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use pr_core::abstractions::SourceReadPriority;
use pr_core::catalog::types::is_raw_path;
use pr_core::diagnostics::perf::{self, PhotoReviewPerf};
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;

// The .NET array length limit (`Array.MaxLength`), shared with the source-bytes cache.
use crate::cache::source_bytes::ARRAY_MAX_LENGTH;

use super::deps::{
    DecodeBox, DecodeRequest, ImageCacheKey, ImageDecoder, ImageSourceKind, SourceBytes, has_known_original, try_delete,
};
use super::handle::{LoadCell, PreviewHandle, RetryContext, wait_cancellable};
use super::{
    DecodeThreadKind, LoadKey, PreviewError, PreviewImage, PreviewImageService, PreviewResult, ServiceInner,
    SourceChangePhase, lock,
};

/// C# `ISourceReader.OpenSource` default `bufferSize` (1 MiB), used by the diagnostic pre-read.
pub const DIAG_PRE_READ_BUFFER_SIZE: usize = 1024 * 1024;

impl PreviewImageService {
    /// C# `GetPreviewAsync(string path)`: `get_preview(path, get_current_cache_key(path))`; a key failure is a `Ready`
    /// `Err(PreviewError::Key)` (C#: the exception escapes the call synchronously; callers treat both alike).
    pub fn get_preview_path(&self, path: &str) -> PreviewHandle {
        match self.get_current_cache_key(path) {
            Ok(key) => self.get_preview(path, &key),
            Err(error) => PreviewHandle::ready(Err(PreviewError::Key(error))),
        }
    }

    /// C# `GetPreviewAsync(string path, ImageCacheKey key)`: a preload / compare request (not the viewer lane): no
    /// cancellation, no delay; a NEW decode runs on a [`DecodeThreadKind::Preview`] thread with source-read priority
    /// `Preload`. A RAM hit is `Ready` (counts a cache hit).
    pub fn get_preview(&self, path: &str, key: &ImageCacheKey) -> PreviewHandle {
        self.inner
            .get_preview_attempt(path, key, false, Duration::ZERO, &CancelToken::none())
    }

    /// C# `GetViewerPreviewAsync(path, key, token, startDelay)`: the viewer's decode of the image being navigated to.
    /// A NEW decode waits `start_delay` (cancellable; zero = no wait), takes one of [`super::VIEWER_DECODE_SLOTS`]
    /// (cancellable), re-checks `token` (last drop point), then decodes on a [`DecodeThreadKind::Viewer`] thread with
    /// priority `Viewer`; once started it runs to completion and its result is cached like a preload. Joining an
    /// in-flight or cached preview behaves exactly as [`Self::get_preview`]. C# negative `TimeSpan` delays (no wait)
    /// cannot be expressed by `Duration`; callers clamp at 0.
    pub fn get_viewer_preview(
        &self,
        path: &str,
        key: &ImageCacheKey,
        token: &CancelToken,
        start_delay: Duration,
    ) -> PreviewHandle {
        self.inner.get_preview_attempt(path, key, true, start_delay, token)
    }

    /// C# `IPreloadTarget.PreloadAsync(path, token)`: `Ready(Err(Canceled))` when `token` is already cancelled, else
    /// [`Self::get_preview_path`] (the token is NOT passed on: a started preload is never cancelled).
    pub fn preload(&self, path: &str, token: &CancelToken) -> PreviewHandle {
        if token.is_canceled() {
            return PreviewHandle::ready(Err(PreviewError::Canceled));
        }
        self.get_preview_path(path)
    }

    /// C# `DecodeOriginalAsync(path, sourceKey, token)` (feat(zoom), R2-F-13): full-resolution decode for the zoomed
    /// viewer, NEVER stored in the RAM or disk cache. Sequence: token check (`Ready(Err(Canceled))`); `use_raw =
    /// is_raw_full_decode_request(path)`; `key = create_original_with_kind(source_key, RAW_FULL_DECODE if use_raw else
    /// source_key.source_kind())`; a RAM hit on `key` is returned as is; otherwise ONE [`DecodeThreadKind::Original`]
    /// thread waits for the 1-slot original gate (cancellable), re-checks the token, decodes (W4-14b semantics 8),
    /// verifies the source (INV-1), seeds the original dimensions under `key`, records the source read
    /// (`include_in_decode_ewma = kind != RAW_FULL_DECODE`) and returns the slot. Not de-duplicated, never retried.
    pub fn decode_original(&self, path: &str, source_key: &ImageCacheKey, token: &CancelToken) -> PreviewHandle {
        if token.is_canceled() {
            return PreviewHandle::ready(Err(PreviewError::Canceled));
        }
        let use_raw_full_decoder = self.is_raw_full_decode_request(path);
        let source_kind = if use_raw_full_decoder {
            ImageSourceKind::RAW_FULL_DECODE
        } else {
            source_key.source_kind()
        };
        let key = ImageCacheKey::create_original_with_kind(source_key, source_kind);
        // An Original-mode preview already in the RAM cache is reused (no metrics, like C#).
        if let Some(cached) = self.inner.cache.try_get(&key) {
            return PreviewHandle::ready(Ok(cached));
        }

        let cell = Arc::new(LoadCell::new());
        let job_cell = Arc::clone(&cell);
        let inner = Arc::clone(&self.inner);
        let path = path.to_owned();
        let token = token.clone();
        let decoder_override = if use_raw_full_decoder {
            self.inner.raw_full_decoder.clone()
        } else {
            None
        };
        let perf_nav = PhotoReviewPerf::nav_context();
        let job = move || {
            PhotoReviewPerf::set_nav_context(perf_nav);
            let result = run_catching(|| {
                // R2-F-13: one full-resolution decode at a time; a superseded request waits here and is dropped.
                let _permit = inner
                    .original_gate
                    .acquire(&token)
                    .map_err(|_| PreviewError::Canceled)?;
                // Last point where a superseded request can be dropped.
                token.check().map_err(|_| PreviewError::Canceled)?;
                inner.run_original_decode(&path, &key, decoder_override, &token)
                // The permit drops here: the gate is free again before the caller sees the outcome (C# `finally`).
            });
            job_cell.complete(result);
        };
        match self.inner.spawner.spawn(DecodeThreadKind::Original, Box::new(job)) {
            Ok(()) => PreviewHandle::load(cell, None),
            Err(error) => PreviewHandle::ready(Err(PreviewError::Spawn(Arc::new(error)))),
        }
    }

    /// C# `GetOriginalDimensionsAsync(path, currentKey)` (blocking; C# `Task.Run` around `ReadInfo`, run it off the UI
    /// thread): `key = create_original(current_key)`; a known value is returned without touching the file; otherwise
    /// `record_source_open(path)`, `get_decoder(key.backend()).read_info(path)` -> `(info.width(), info.height())`
    /// (displayed size), then `key.matches_current_source(fs)` or `Err(SourceChanged { ReadingDimensions })`, then the
    /// value is stored under `key`.
    pub fn get_original_dimensions(&self, path: &str, current_key: &ImageCacheKey) -> Result<(i32, i32), PreviewError> {
        let key = ImageCacheKey::create_original(current_key);
        if let Some(dimensions) = self.inner.original_dimensions.try_get(&key) {
            return Ok(dimensions);
        }
        // ReadInfo genuinely opens the file (a header-only read): it counts as a source open like any other.
        self.inner.metrics.record_source_open(path);
        let info = self
            .inner
            .get_decoder(key.backend())
            .read_info(path)
            .map_err(|error| PreviewError::Decode(Arc::new(error)))?;
        let dimensions = (info.width(), info.height());
        // INV-1: the stat happens AFTER the read (C#).
        if !key.matches_current_source(self.inner.fs.as_ref()) {
            return Err(PreviewError::SourceChanged {
                path: path.to_owned(),
                phase: SourceChangePhase::ReadingDimensions,
            });
        }
        self.inner.original_dimensions.set(key, dimensions);
        Ok(dimensions)
    }
}

/// What the creator's load thread needs (the C# lambda captures of `GetPreviewOnceAsync`).
#[derive(Debug, Clone)]
pub(crate) struct LoadJob {
    pub(crate) path: String,
    pub(crate) key: ImageCacheKey,
    /// The epoch read BEFORE the in-flight registration (C# `cacheEpoch`): the publish and the persist hand-off compare
    /// against it.
    pub(crate) cache_epoch: i64,
    pub(crate) viewer: bool,
    pub(crate) start_delay: Duration,
    pub(crate) token: CancelToken,
    /// `PhotoReviewPerf::nav_context()` of the creating thread, set on the load thread before any work.
    pub(crate) perf_nav: i64,
}

/// Runs `work`, turning a panic into `Err(Panicked(text))` (R0003: a worker reports a panic as an error).
fn run_catching(work: impl FnOnce() -> PreviewResult) -> PreviewResult {
    panic::catch_unwind(AssertUnwindSafe(work))
        .unwrap_or_else(|payload| Err(PreviewError::Panicked(panic_text(&*payload))))
}

/// The message of a panic payload (`&str` / `String`), else a fixed text.
fn panic_text(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a preview load panicked".to_owned()
    }
}

/// C# `Interlocked.Increment(ref _activeViewerDecodes)` paired with the `finally` decrement (also on a panic).
struct ActiveViewerDecode<'a>(&'a AtomicI32);

impl<'a> ActiveViewerDecode<'a> {
    fn start(counter: &'a AtomicI32) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}

impl Drop for ActiveViewerDecode<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl ServiceInner {
    /// One attempt of C# `GetPreviewOnceAsync`, up to the point where the caller awaits (the waiting part is
    /// `PreviewHandle::wait`):
    /// 1. `token` already cancelled -> `Ready(Err(Canceled))` (C# `ThrowIfCancellationRequested` at the loop top).
    /// 2. RAM hit -> `record_cache_hit`, `Ready(Ok)`.
    /// 3. `epoch = current_epoch()`; under the in-flight lock: fetch `(key, epoch)`, or recheck RAM before inserting
    ///    a NEW cell (creator). The recheck closes a completed-load registration race (ADR-I-07).
    ///    Creator -> `record_cache_miss`, then (outside the lock) start the load (`start_load`); a start failure
    ///    completes the cell with `Err(Spawn)` and removes it. Joiner -> `record_inflight_join` (+ perf `JoinStart`).
    /// 4. Return `PreviewHandle::load(cell, Some(retry context))`.
    pub(crate) fn get_preview_attempt(
        self: &Arc<Self>,
        path: &str,
        key: &ImageCacheKey,
        viewer: bool,
        start_delay: Duration,
        token: &CancelToken,
    ) -> PreviewHandle {
        if token.is_canceled() {
            return PreviewHandle::ready(Err(PreviewError::Canceled));
        }
        if let Some(cached) = self.cache.try_get(key) {
            self.metrics.record_cache_hit();
            return PreviewHandle::ready(Ok(cached));
        }
        self.get_preview_after_cache_miss(path, key, viewer, start_delay, token)
    }

    /// Registration after the optimistic RAM miss. Kept as one stage so tests can arrange a completed
    /// publication between that miss and registration without timing, sleeps, or a production test hook.
    pub(super) fn get_preview_after_cache_miss(
        self: &Arc<Self>,
        path: &str,
        key: &ImageCacheKey,
        viewer: bool,
        start_delay: Duration,
        token: &CancelToken,
    ) -> PreviewHandle {
        // C# `cacheEpoch`: read before the registration; the load publishes and persists only under it (INV-2).
        let cache_epoch = self.current_epoch();
        let load_key: LoadKey = (key.clone(), cache_epoch);
        // C# `GetOrAdd(loadKey, candidate)` + `ReferenceEquals(lazy, candidate)`: decided under the lock.
        let (cell, is_creator) = {
            let mut in_flight = lock(&self.in_flight);
            match in_flight.entry(load_key.clone()) {
                Entry::Occupied(entry) => (Arc::clone(entry.get()), false),
                Entry::Vacant(entry) => {
                    // A prior decode can publish and unregister after our optimistic RAM miss.
                    // Recheck while registration is locked so that completed work is not decoded again.
                    if let Some(cached) = self.cache.try_get(key) {
                        self.metrics.record_cache_hit();
                        return PreviewHandle::ready(Ok(cached));
                    }
                    let candidate = Arc::new(LoadCell::new());
                    entry.insert(Arc::clone(&candidate));
                    (candidate, true)
                }
            }
        };

        let mut perf_join = None;
        if is_creator {
            self.metrics.record_cache_miss();
            let job = LoadJob {
                path: path.to_owned(),
                key: key.clone(),
                cache_epoch,
                viewer,
                start_delay,
                token: token.clone(),
                perf_nav: PhotoReviewPerf::nav_context(),
            };
            if let Err(error) = self.start_load(Arc::clone(&cell), job) {
                // The job was not run: unregister first, then fail every waiter (semantics 3 order).
                self.remove_in_flight(&load_key, &cell);
                cell.complete(Err(PreviewError::Spawn(Arc::new(error))));
            }
        } else {
            self.metrics.record_inflight_join();
            // D04 perf: a joiner records JoinStart/JoinEnd under its own nav.
            let log = PhotoReviewPerf::log();
            if log.is_enabled() {
                let nav = PhotoReviewPerf::nav_context();
                let path_id = perf::path_id(path);
                log.join_start(nav, &path_id);
                perf_join = Some((nav, path_id, perf::timestamp()));
            }
        }

        PreviewHandle::load(
            cell,
            Some(Box::new(RetryContext {
                service: Arc::clone(self),
                path: path.to_owned(),
                key: key.clone(),
                viewer,
                start_delay,
                token: token.clone(),
                is_creator,
                perf_join,
            })),
        )
    }

    /// C# `TryRemove(KeyValuePair(loadKey, lazy))`: removes the entry only while it still maps to this very cell.
    fn remove_in_flight(&self, load_key: &LoadKey, cell: &Arc<LoadCell>) {
        let mut in_flight = lock(&self.in_flight);
        if in_flight
            .get(load_key)
            .is_some_and(|registered| Arc::ptr_eq(registered, cell))
        {
            in_flight.remove(load_key);
        }
    }

    /// Starts the creator's load on a spawned thread: viewer -> `run_viewer_load`, else `decode_and_cache(..,
    /// Preload)`. The thread copies the creator's perf nav context first (C# `Task.Run` flows the `ExecutionContext`;
    /// `PhotoReviewPerf::nav_context` is a thread-local), runs the work inside `catch_unwind` (a panic becomes
    /// `Err(Panicked)`), REMOVES `(load_key, cell)` from the in-flight table (only if it is still this cell), THEN
    /// completes the cell (waiters observe the removal, C# `finally` before the awaiter resumes).
    pub(crate) fn start_load(self: &Arc<Self>, cell: Arc<LoadCell>, job: LoadJob) -> std::io::Result<()> {
        let inner = Arc::clone(self);
        let kind = if job.viewer {
            DecodeThreadKind::Viewer
        } else {
            DecodeThreadKind::Preview
        };
        self.spawner.spawn(
            kind,
            Box::new(move || {
                PhotoReviewPerf::set_nav_context(job.perf_nav);
                let result = run_catching(|| {
                    if job.viewer {
                        inner.run_viewer_load(&job.path, &job.key, job.cache_epoch, job.start_delay, &job.token)
                    } else {
                        // Q-R29 option C-2: the preload / compare path tags its own source open `Preload`.
                        inner.decode_and_cache(
                            &job.path,
                            &job.key,
                            job.key.target_box(),
                            job.cache_epoch,
                            SourceReadPriority::Preload,
                        )
                    }
                });
                let load_key: LoadKey = (job.key, job.cache_epoch);
                inner.remove_in_flight(&load_key, &cell);
                cell.complete(result);
            }),
        )
    }

    /// C# `DecodeForViewerAsync`: `wait_cancellable(start_delay)` (only when > 0), `viewer_slots.acquire(token)`, token
    /// re-check, `active_viewer_decodes += 1`, `decode_and_cache(.., Viewer)`, `-= 1` (only if it was incremented), slot
    /// returned on every path. Any cancellation is `Err(Canceled)`.
    pub(crate) fn run_viewer_load(
        self: &Arc<Self>,
        path: &str,
        key: &ImageCacheKey,
        cache_epoch: i64,
        start_delay: Duration,
        token: &CancelToken,
    ) -> PreviewResult {
        if !start_delay.is_zero() {
            wait_cancellable(start_delay, token).map_err(|_| PreviewError::Canceled)?;
        }
        let _permit = self.viewer_slots.acquire(token).map_err(|_| PreviewError::Canceled)?;
        // Last point where superseded work can be dropped: once the decode starts it runs to completion.
        token.check().map_err(|_| PreviewError::Canceled)?;
        // Declared after the permit: dropped first (C# `finally`: decrement, then return the slot).
        let _active = ActiveViewerDecode::start(&self.active_viewer_decodes);
        self.decode_and_cache(path, key, key.target_box(), cache_epoch, SourceReadPriority::Viewer)
    }

    /// C# `DecodeAndCache` (W4-14b semantics 6, in this order): RAW-full-decode guard; disk path (W4-14c
    /// `disk_cache_path_for`, `None` when the disk cache is off); disk hit (`try_read_disk_entry`) or source decode;
    /// INV-1 verify; degraded check; epoch-checked publish; original-dimension seeding; persist hand-off
    /// (`queue_persist`, only for a source decode that is not degraded and IS downscaled); `record_source_read` for a
    /// source decode.
    pub(crate) fn decode_and_cache(
        &self,
        path: &str,
        key: &ImageCacheKey,
        target_box: DecodeBox,
        cache_epoch: i64,
        priority: SourceReadPriority,
    ) -> PreviewResult {
        // A runtime guard, not a debug assertion: RawFullDecode keys only come from `decode_original`, which records
        // its own EWMA-excluded sample.
        if key.source_kind() == ImageSourceKind::RAW_FULL_DECODE {
            return Err(PreviewError::RawFullDecodeKey);
        }
        let perf_log = PhotoReviewPerf::log();
        let perf_on = perf_log.is_enabled();
        let perf_nav = if perf_on { PhotoReviewPerf::nav_context() } else { 0 };
        let perf_path_id = if perf_on { perf::path_id(path) } else { String::new() };
        let stopwatch = Instant::now();
        let mut source_read = false;
        // L02: `None` while the disk cache is disabled (the key is then never hashed).
        let cache_path = self.disk_cache_path_for(key);

        let image = match &cache_path {
            Some(cache_path) if self.disk_entry_exists(cache_path) => match self.try_read_disk_entry(cache_path) {
                Ok(image) => image,
                // A broken (or concurrently pruned) entry is a miss: delete it (best effort, logged) and decode.
                Err(error) if error.is_cache_entry_failure() => {
                    try_delete(cache_path, Some(self.log.as_ref()));
                    source_read = true;
                    self.decode_from_source(path, key.length(), key.backend(), target_box, priority)?
                }
                // C#: OutOfMemory / cancellation escape; the entry is kept and the source is not decoded.
                Err(error) => return Err(PreviewError::DiskEntry(Arc::new(error))),
            },
            _ => {
                source_read = true;
                self.decode_from_source(path, key.length(), key.backend(), target_box, priority)?
            }
        };

        // INV-1: a path replaced while the decode ran never publishes the old pixels under the new identity.
        let verify_start = if perf_on { perf::timestamp() } else { 0 };
        if !key.matches_current_source(self.fs.as_ref()) {
            return Err(PreviewError::SourceChanged {
                path: path.to_owned(),
                phase: SourceChangePhase::Decode,
            });
        }
        if perf_on {
            perf_log.verify(perf_nav, &perf_path_id, perf::ms(verify_start));
        }
        // A degraded fallback is returned but never cached in RAM or on disk (the next view retries the better one).
        let degraded = image.is_degraded_fallback();
        if !degraded {
            self.publish_if_current(key.clone(), Arc::clone(&image), cache_epoch);
        }
        // Seeds the original dimensions for free (also a degraded image and a disk hit); not epoch-checked (C#).
        if has_known_original(image.as_ref()) {
            self.original_dimensions.set(
                ImageCacheKey::create_original(key),
                (image.original_width(), image.original_height()),
            );
        }
        // Only previews that really decoded at the downscaled box are persisted (never a full-size fallback).
        if let Some(cache_path) = cache_path
            && source_read
            && !degraded
            && image.downscaled()
        {
            self.queue_persist(Arc::clone(&image), cache_path, cache_epoch);
        }
        if source_read {
            let elapsed_ms = i64::try_from(stopwatch.elapsed().as_millis()).unwrap_or(i64::MAX);
            self.metrics
                .record_source_read(source_bytes_read(&image, key), elapsed_ms, true);
        }
        Ok(image)
    }

    /// C# `DecodeFromSource` (semantics 7): optional pre-read (source-bytes cache when it can hold `source_length` and
    /// the file is not RAW; else the diagnostic pre-read when `diag.pre_read` and not RAW), `record_source_open(path)`,
    /// then `get_decoder(backend).decode(DecodeRequest::with_box(path, target_box)[.with_bytes].with_priority(priority))`
    /// plus the perf `Decode` event.
    pub(crate) fn decode_from_source(
        &self,
        path: &str,
        source_length: i64,
        backend: DecoderBackend,
        target_box: DecodeBox,
        priority: SourceReadPriority,
    ) -> PreviewResult {
        // A file the byte cache cannot hold is not pre-read (the decoder streams it); RAW is never pre-read.
        let is_raw_source = is_raw_path(path);
        let mut pre_read: Option<SourceBytes> = None;
        if !is_raw_source
            && let Some(source_bytes) = &self.source_bytes_cache
            && source_bytes.can_cache(source_length)
        {
            pre_read = Some(
                source_bytes
                    .get_or_read_with_priority(Path::new(path), priority)
                    .map_err(PreviewError::SourceBytes)?,
            );
        } else if !is_raw_source && self.diag.pre_read {
            pre_read = Some(self.diag_pre_read(path, priority)?);
        }

        self.metrics.record_source_open(path);

        let perf_log = PhotoReviewPerf::log();
        let perf_on = perf_log.is_enabled();
        let decode_start = if perf_on { perf::timestamp() } else { 0 };
        let mut request = DecodeRequest::with_box(path, target_box);
        if let Some(bytes) = pre_read {
            request = request.with_bytes(bytes);
        }
        let request = request.with_priority(priority);
        let decoded = self
            .get_decoder(backend)
            .decode(&request)
            .map_err(|error| PreviewError::Decode(Arc::new(error)))?;
        if perf_on {
            perf_log.decode(
                PhotoReviewPerf::nav_context(),
                &perf::path_id(path),
                perf::ms(decode_start),
                target_box.width,
                decoded.downscaled(),
                !target_box.is_unbounded() && !decoded.downscaled(),
            );
        }
        Ok(decoded)
    }

    /// The C# diagnostic pre-read loop: open through the source reader (`priority`, [`DIAG_PRE_READ_BUFFER_SIZE`]),
    /// allocate the stream LENGTH (seek to end), read until full or a 0-byte read (a file that shrank keeps zero bytes at
    /// the end, like C#), perf `SourceRead`.
    pub(crate) fn diag_pre_read(&self, path: &str, priority: SourceReadPriority) -> Result<SourceBytes, PreviewError> {
        let perf_log = PhotoReviewPerf::log();
        let perf_on = perf_log.is_enabled();
        let read_start = if perf_on { perf::timestamp() } else { 0 };
        let io_error = |error: io::Error| PreviewError::Io(Arc::new(error));
        let bytes = {
            let mut stream = self
                .source_reader
                .open_source(Path::new(path), priority, DIAG_PRE_READ_BUFFER_SIZE)
                .map_err(io_error)?;
            // C# `fileStream.Length`.
            let length = stream.seek(SeekFrom::End(0)).map_err(io_error)?;
            stream.seek(SeekFrom::Start(0)).map_err(io_error)?;
            // Open question 3: C# `new byte[length]` fails above the .NET array limit; refuse before allocating.
            let too_large = || {
                io_error(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("The source is too large to pre-read into memory: {length} bytes."),
                ))
            };
            if length > ARRAY_MAX_LENGTH.unsigned_abs() {
                return Err(too_large());
            }
            let length = usize::try_from(length).map_err(|_| too_large())?;
            let mut buffer: Vec<u8> = Vec::new();
            buffer.try_reserve_exact(length).map_err(|_| {
                io_error(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "Insufficient memory to pre-read the source.",
                ))
            })?;
            buffer.resize(length, 0);
            let mut offset = 0;
            while offset < buffer.len() {
                match stream.read(&mut buffer[offset..]) {
                    Ok(0) => break,
                    Ok(read) => offset += read,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(io_error(error)),
                }
            }
            buffer
        };
        if perf_on {
            perf_log.source_read(
                PhotoReviewPerf::nav_context(),
                &perf::path_id(path),
                perf::ms(read_start),
                i64::try_from(bytes.len()).unwrap_or(i64::MAX),
            );
        }
        Ok(SourceBytes::from(bytes))
    }

    /// C# `GetDecoder(backend)`: without a factory the configured decoder for every backend; with one, a per-backend
    /// cache filled from `factory.create(backend)` (never call the factory under the map lock; first insert wins).
    pub(crate) fn get_decoder(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        let Some(factory) = &self.decoder_factory else {
            return Arc::clone(&self.decoder);
        };
        if let Some(decoder) = lock(&self.decoders_by_backend).get(&backend) {
            return Arc::clone(decoder);
        }
        let created = factory.create(backend);
        Arc::clone(lock(&self.decoders_by_backend).entry(backend).or_insert(created))
    }

    /// The work of the original thread (after the gate): see [`PreviewImageService::decode_original`].
    pub(crate) fn run_original_decode(
        &self,
        path: &str,
        key: &ImageCacheKey,
        raw_full_decoder: Option<Arc<dyn ImageDecoder>>,
        token: &CancelToken,
    ) -> PreviewResult {
        let stopwatch = Instant::now();
        let decoded = match raw_full_decoder {
            None => self.decode_from_source(
                path,
                key.length(),
                key.backend(),
                DecodeBox::UNBOUNDED,
                SourceReadPriority::Viewer,
            )?,
            // C# calls the override directly: no pre-read, default priority, no source-open record.
            Some(decoder) => {
                let request = DecodeRequest::with_box(path, DecodeBox::UNBOUNDED);
                let decoded = match decoder.as_cancellable() {
                    Some(cancellable) => cancellable.decode_cancellable(&request, token),
                    None => decoder.decode(&request),
                };
                decoded.map_err(|error| PreviewError::Decode(Arc::new(error)))?
            }
        };
        if !key.matches_current_source(self.fs.as_ref()) {
            return Err(PreviewError::SourceChanged {
                path: path.to_owned(),
                phase: SourceChangePhase::Decode,
            });
        }
        if has_known_original(decoded.as_ref()) {
            self.original_dimensions
                .set(key.clone(), (decoded.original_width(), decoded.original_height()));
        }
        let elapsed_ms = i64::try_from(stopwatch.elapsed().as_millis()).unwrap_or(i64::MAX);
        self.metrics.record_source_read(
            source_bytes_read(&decoded, key),
            elapsed_ms,
            key.source_kind() != ImageSourceKind::RAW_FULL_DECODE,
        );
        Ok(decoded)
    }
}

/// C# `GetSourceBytesRead(image, key)`: `image.source_bytes_read()` when the image reports it (C# `ISourceReadMetrics`),
/// else `key.length()`.
pub(crate) fn source_bytes_read(image: &PreviewImage, key: &ImageCacheKey) -> i64 {
    image.source_bytes_read().unwrap_or_else(|| key.length())
}
