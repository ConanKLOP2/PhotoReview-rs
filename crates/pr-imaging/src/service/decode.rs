//! W4-14b part 2 of 2 (port of `PreviewImageService.cs`, C# baseline d92d6aae): preview requests with in-flight
//! de-duplication (C# `GetPreviewCoreAsync` / `GetPreviewOnceAsync`), the viewer lane (start delay, 2 viewer slots,
//! dedicated thread), `DecodeAndCache` (disk hit via W4-14c, source decode, INV-1 verify, epoch-checked publish,
//! original-dimension seeding, persist hand-off), the full-resolution original decode (1-slot gate, never cached) and
//! the original-dimension read. Contract: `contracts/W4-14b.md`.
//!
//! Target path: `crates/pr-imaging/src/service/decode.rs`.

#![allow(unused_variables, unused_imports, dead_code)] // D11: remove with the last `todo!()`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pr_core::abstractions::SourceReadPriority;
use pr_core::diagnostics::perf::{self, PhotoReviewPerf};
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;

use super::deps::{DecodeBox, DecodeRequest, ImageCacheKey, ImageDecoder, ImageSourceKind, SourceBytes};
use super::handle::{LoadCell, PreviewHandle, RetryContext};
use super::{
    DecodeThreadKind, PreviewError, PreviewImage, PreviewImageService, PreviewResult, ServiceInner, SourceChangePhase,
};

/// C# `ISourceReader.OpenSource` default `bufferSize` (1 MiB), used by the diagnostic pre-read.
pub const DIAG_PRE_READ_BUFFER_SIZE: usize = 1024 * 1024;

impl PreviewImageService {
    /// C# `GetPreviewAsync(string path)`: `get_preview(path, get_current_cache_key(path))`; a key failure is a `Ready`
    /// `Err(PreviewError::Key)` (C#: the exception escapes the call synchronously; callers treat both alike).
    pub fn get_preview_path(&self, path: &str) -> PreviewHandle {
        todo!("W4-14b")
    }

    /// C# `GetPreviewAsync(string path, ImageCacheKey key)`: a preload / compare request (not the viewer lane): no
    /// cancellation, no delay; a NEW decode runs on a [`DecodeThreadKind::Preview`] thread with source-read priority
    /// `Preload`. A RAM hit is `Ready` (counts a cache hit).
    pub fn get_preview(&self, path: &str, key: &ImageCacheKey) -> PreviewHandle {
        todo!("W4-14b")
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
        todo!("W4-14b")
    }

    /// C# `IPreloadTarget.PreloadAsync(path, token)`: `Ready(Err(Canceled))` when `token` is already cancelled, else
    /// [`Self::get_preview_path`] (the token is NOT passed on: a started preload is never cancelled).
    pub fn preload(&self, path: &str, token: &CancelToken) -> PreviewHandle {
        todo!("W4-14b")
    }

    /// C# `DecodeOriginalAsync(path, sourceKey, token)` (feat(zoom), R2-F-13): full-resolution decode for the zoomed
    /// viewer, NEVER stored in the RAM or disk cache. Sequence: token check (`Ready(Err(Canceled))`); `use_raw =
    /// is_raw_full_decode_request(path)`; `key = create_original_with_kind(source_key, RAW_FULL_DECODE if use_raw else
    /// source_key.source_kind())`; a RAM hit on `key` is returned as is; otherwise ONE [`DecodeThreadKind::Original`]
    /// thread waits for the 1-slot original gate (cancellable), re-checks the token, decodes (W4-14b semantics 8),
    /// verifies the source (INV-1), seeds the original dimensions under `key`, records the source read
    /// (`include_in_decode_ewma = kind != RAW_FULL_DECODE`) and returns the slot. Not de-duplicated, never retried.
    pub fn decode_original(&self, path: &str, source_key: &ImageCacheKey, token: &CancelToken) -> PreviewHandle {
        todo!("W4-14b")
    }

    /// C# `GetOriginalDimensionsAsync(path, currentKey)` (blocking; C# `Task.Run` around `ReadInfo`, run it off the UI
    /// thread): `key = create_original(current_key)`; a known value is returned without touching the file; otherwise
    /// `record_source_open(path)`, `get_decoder(key.backend()).read_info(path)` -> `(info.width(), info.height())`
    /// (displayed size), then `key.matches_current_source(fs)` or `Err(SourceChanged { ReadingDimensions })`, then the
    /// value is stored under `key`.
    pub fn get_original_dimensions(&self, path: &str, current_key: &ImageCacheKey) -> Result<(i32, i32), PreviewError> {
        todo!("W4-14b")
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

impl ServiceInner {
    /// One attempt of C# `GetPreviewOnceAsync`, up to the point where the caller awaits (the waiting part is
    /// `PreviewHandle::wait`):
    /// 1. `token` already cancelled -> `Ready(Err(Canceled))` (C# `ThrowIfCancellationRequested` at the loop top).
    /// 2. RAM hit -> `record_cache_hit`, `Ready(Ok)`.
    /// 3. `epoch = current_epoch()`; under the in-flight lock: fetch `(key, epoch)` or insert a NEW cell (creator).
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
        todo!("W4-14b")
    }

    /// Starts the creator's load on a spawned thread: viewer -> `run_viewer_load`, else `decode_and_cache(..,
    /// Preload)`. The thread copies the creator's perf nav context first (C# `Task.Run` flows the `ExecutionContext`;
    /// `PhotoReviewPerf::nav_context` is a thread-local), runs the work inside `catch_unwind` (a panic becomes
    /// `Err(Panicked)`), REMOVES `(load_key, cell)` from the in-flight table (only if it is still this cell), THEN
    /// completes the cell (waiters observe the removal, C# `finally` before the awaiter resumes).
    pub(crate) fn start_load(self: &Arc<Self>, cell: Arc<LoadCell>, job: LoadJob) -> std::io::Result<()> {
        todo!("W4-14b")
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
        todo!("W4-14b")
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
        todo!("W4-14b")
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
        todo!("W4-14b")
    }

    /// The C# diagnostic pre-read loop: open through the source reader (`priority`, [`DIAG_PRE_READ_BUFFER_SIZE`]),
    /// allocate the stream LENGTH (seek to end), read until full or a 0-byte read (a file that shrank keeps zero bytes at
    /// the end, like C#), perf `SourceRead`.
    pub(crate) fn diag_pre_read(&self, path: &str, priority: SourceReadPriority) -> Result<SourceBytes, PreviewError> {
        todo!("W4-14b")
    }

    /// C# `GetDecoder(backend)`: without a factory the configured decoder for every backend; with one, a per-backend
    /// cache filled from `factory.create(backend)` (never call the factory under the map lock; first insert wins).
    pub(crate) fn get_decoder(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        todo!("W4-14b")
    }

    /// The work of the original thread (after the gate): see [`PreviewImageService::decode_original`].
    pub(crate) fn run_original_decode(
        &self,
        path: &str,
        key: &ImageCacheKey,
        raw_full_decoder: Option<Arc<dyn ImageDecoder>>,
        token: &CancelToken,
    ) -> PreviewResult {
        todo!("W4-14b")
    }
}

/// C# `GetSourceBytesRead(image, key)`: `image.source_bytes_read()` when the image reports it (C# `ISourceReadMetrics`),
/// else `key.length()`.
pub(crate) fn source_bytes_read(image: &PreviewImage, key: &ImageCacheKey) -> i64 {
    todo!("W4-14b")
}
