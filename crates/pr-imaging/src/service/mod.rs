//! Port of `PhotoReview.Imaging/Caching/PreviewImageService.cs` (C# baseline d92d6aae, 924 lines; PARITY I-04, I-07,
//! I-08, I-16, INV-1, INV-2). Contracts: `contracts/W4-14a.md` (this file: state, policy, keys, RAM cache, epoch),
//! `W4-14b.md` (`handle.rs`, `decode.rs`: in-flight de-duplication, viewer slots, decode, original decode),
//! `W4-14c.md` (`disk.rs`: disk-cache path, persist workers, startup cleanup, clear disk).
//!
//! Owns preview decode, the byte-bounded RAM cache, in-flight decode de-duplication and the cache invalidation epoch
//! (INV-2). The current loading mode and the target decode box are supplied by the caller (UI/settings state).
//!
//! Concurrency (docs/adr/R0003): no async runtime. The C# `Task`-returning entry points return a
//! [`handle::PreviewHandle`] at once (a cache hit or an immediate failure is `Ready`); the work runs on a thread started
//! by the [`DecodeThreadSpawner`] (C# `Task.Run` / `TaskCreationOptions.LongRunning`) and the caller blocks in
//! `PreviewHandle::wait`. Joining an in-flight decode is decided synchronously, like the C# `GetOrAdd`.
//!
//! The service is a handle over shared state (`ServiceInner`): worker threads hold an `Arc` of it. Dropping the
//! [`PreviewImageService`] completes the persist queue (the workers drain it and exit) without waiting.
//!
//! Target path: `crates/pr-imaging/src/service/mod.rs` (lead adds `pub mod service;` to `lib.rs`).

#![allow(unused_variables, unused_imports, dead_code)] // D11: remove with the last `todo!()` of the W4-14 stubs.

pub mod decode; // W4-14b
pub mod deps; // wiring stand-in, see its header
pub mod disk; // W4-14c
pub mod handle; // W4-14b

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_core::abstractions::{Clock, FileSystem, Log, SourceReader};
use pr_core::caching::lru::BoundedLruCache;
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::diag_options::DiagOptions;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_core::text::dotnet_path::PathError;

use self::deps::{
    CacheKeyError, DecodeBox, DecodeError, DecodedImageView, DiskCacheStore, ImageCacheKey, ImageDecoder,
    ImageDecoderFactory, InvalidStoreArgument, PlatformImageCodec, PreloadWindow, PreviewCacheError,
    PreviewPayloadCodec, SourceBytesCache, SourceBytesError,
};

// ---- Constants (C# values; tests pin them) ----------------------------------------------------------------------

/// C# `DefaultOriginalDimensionsCapacity`: entry cap of the original-dimensions cache (IMG-02).
pub const DEFAULT_ORIGINAL_DIMENSIONS_CAPACITY: i32 = 200_000;
/// C# constructor default `capacityBytes` (16 GiB) before the RAM clamp.
pub const DEFAULT_CAPACITY_BYTES: i64 = 16 * 1024 * 1024 * 1024;
/// C# constructor default `diskCacheCapacityBytes` (4 GiB; PARITY S-15: not the persisted setting).
pub const DEFAULT_DISK_CACHE_CAPACITY_BYTES: i64 = 4 * 1024 * 1024 * 1024;
/// C# `ViewerDecodeSlots`: concurrent viewer decodes (the current image plus one spare).
pub const VIEWER_DECODE_SLOTS: i32 = 2;
/// One MiB, the unit of the "Memory budgets" line (C# `const long mib`).
const MIB: i64 = 1024 * 1024;

// ---- Public types -----------------------------------------------------------------------------------------------

/// A decoded preview, shared between the RAM cache, every waiter and the persist queue (C# reference identity:
/// `Arc::ptr_eq`).
pub type PreviewImage = Arc<dyn DecodedImageView>;

/// What a preview/original request yields. `Clone`: one shared decode delivers the same outcome to every joiner.
pub type PreviewResult = Result<PreviewImage, PreviewError>;

/// C# `Func<bool>` (loading mode, RAW full-decode switch). Read on every key build / request.
pub type BoolSource = Arc<dyn Fn() -> bool + Send + Sync>;
/// C# `Func<DecodeBox> targetDecodeBox`, read on every key build.
pub type DecodeBoxSource = Arc<dyn Fn() -> DecodeBox + Send + Sync>;
/// C# `Func<DecoderBackend> currentBackend`, read on every key build and at construction.
pub type BackendSource = Arc<dyn Fn() -> DecoderBackend + Send + Sync>;
/// A test seam callback (C# `Action?` properties).
pub type TestHook = Arc<dyn Fn() + Send + Sync>;
/// A `CacheCleared` subscriber (C# `event EventHandler? CacheCleared`).
pub type CacheClearedListener = Arc<dyn Fn() + Send + Sync>;

/// Which "source changed" sentence a [`PreviewError::SourceChanged`] carries (C# `UserFacingError.Localized`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceChangePhase {
    /// Preview decode and original decode: catalog key `err.io.sourceChangedDuringDecode`, C# message
    /// `Image source changed during decode: {path}`.
    Decode,
    /// `get_original_dimensions`: catalog key `err.io.sourceChangedReadingDimensions`, C# message
    /// `Image source changed while reading dimensions: {path}`.
    ReadingDimensions,
}

impl SourceChangePhase {
    /// The language-catalog key of the localized sentence (`{path}` = the path as the caller passed it).
    pub const fn catalog_key(self) -> &'static str {
        match self {
            Self::Decode => "err.io.sourceChangedDuringDecode",
            Self::ReadingDimensions => "err.io.sourceChangedReadingDimensions",
        }
    }
}

/// Why a request failed. One variant per C# exception path a caller distinguishes (R0003: never collapse them).
/// `Display` is the C# `Exception.Message` (English, for logs); UI text comes from [`Self::catalog_key`] or the inner
/// error.
#[derive(Debug, Clone)]
pub enum PreviewError {
    /// C# `OperationCanceledException`: the caller's token was cancelled (before the decode started, during the start
    /// delay or while waiting for a viewer slot / the original gate). A decoder-internal cancellation is
    /// `Decode(DecodeError::Canceled)`; [`Self::is_canceled`] covers both.
    Canceled,
    /// The cache key could not be built (C# `ImageCacheKey.Create` threw: missing file, bad path). Only from the
    /// path-only entry points (`get_preview_path`, `preload`).
    Key(CacheKeyError),
    /// `Path.GetFullPath` refused the path (`evict_cached_path`).
    InvalidPath(PathError),
    /// The decoder failed; shared by every joiner of one decode. (A broken disk entry is never an error: it is deleted
    /// and the source decoded; see [`Self::DiskEntry`] for the failures that escape.)
    Decode(Arc<DecodeError>),
    /// The source-bytes cache failed to read the source (C# `IOException` / `ArgumentException` from `GetOrRead`).
    SourceBytes(SourceBytesError),
    /// The diagnostic pre-read (`PHOTOREVIEW_RS_DIAG_PREREAD=1`) could not open or read the source.
    Io(Arc<io::Error>),
    /// A disk-cache read failure that is NOT a cache-entry failure (`!PreviewCacheError::is_cache_entry_failure()`:
    /// `OutOfMemory`, C# `InsufficientMemoryException` / `OutOfMemoryException`, and `Canceled`): C# lets the exception
    /// escape `DecodeAndCache` (the entry is kept, the source is NOT decoded). Kept apart from `Canceled` because the
    /// caller's token is not involved.
    DiskEntry(Arc<PreviewCacheError>),
    /// C# localized `IOException`: the source changed between key build and the end of the decode / dimension read
    /// (INV-1: the pixels are never published). `path` is the path the caller passed (not normalized).
    SourceChanged { path: String, phase: SourceChangePhase },
    /// C# `InvalidOperationException("DecodeAndCache never handles RawFullDecode keys; use DecodeOriginalAsync.")`:
    /// a `RawFullDecode` key reached the preview path (a programming error; checked in release builds too).
    RawFullDecodeKey,
    /// A worker thread could not be started (no C# twin: `Task.Run` cannot fail this way).
    Spawn(Arc<io::Error>),
    /// The work panicked (R0003: a worker reports a panic as an error; C# a faulted task). The text of the panic.
    Panicked(String),
}

impl PreviewError {
    /// The C# message of [`Self::RawFullDecodeKey`].
    pub const RAW_FULL_DECODE_KEY_MESSAGE: &'static str =
        "DecodeAndCache never handles RawFullDecode keys; use DecodeOriginalAsync.";

    /// C# `catch (OperationCanceledException)`: `Canceled`, `DiskEntry(PreviewCacheError::Canceled)` or
    /// `Decode(DecodeError::Canceled)`.
    pub fn is_canceled(&self) -> bool {
        todo!("W4-14a")
    }

    /// C# `catch (DecoderBusyException)`: `Decode(DecodeError::Busy { .. })`.
    pub fn is_busy(&self) -> bool {
        todo!("W4-14a")
    }

    /// C# `ex is IOException`: `SourceChanged`, `Io`, `SourceBytes(Io)`, `Key(SourceNoLongerExists)`,
    /// `Key(InvalidPath(TooLong))`, and `Decode(e)` when `e.is_io_exception()`.
    pub fn is_io_exception(&self) -> bool {
        todo!("W4-14a")
    }

    /// The localized-sentence catalog key and its `{path}` argument (C# `UserFacingError.Localized`), when this error
    /// carries one of its own (`SourceChanged` only; inner errors carry theirs).
    pub fn catalog_key(&self) -> Option<(&'static str, &str)> {
        todo!("W4-14a")
    }
}

impl fmt::Display for PreviewError {
    /// C# `Exception.Message`: `Image source changed during decode: {path}`, `Image source changed while reading
    /// dimensions: {path}`, [`Self::RAW_FULL_DECODE_KEY_MESSAGE`], `The operation was canceled.`, else the inner
    /// error's text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W4-14a")
    }
}

impl std::error::Error for PreviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        todo!("W4-14a")
    }
}

/// Construction failures (C# `ArgumentNullException` / `ArgumentOutOfRangeException` / `ArgumentException`).
#[derive(Debug)]
pub enum PreviewServiceBuildError {
    /// Neither `decoder` nor `decoder_factory` was given. C# then builds `new WicDirectDecoder(codec, reader)`; that type
    /// lives in `pr_platform_win` (W4-11c), so the composition root passes it explicitly (see the contract).
    NoDecoder,
    /// `original_dimensions_capacity <= 0` (C# `BoundedLruCache` constructor, parameter `capacity`).
    OriginalDimensionsCapacity(i32),
    /// The resolved RAM capacity was `<= 0` (only reachable through a `ram_budget` defect; C# `BoundedLruCache`).
    Capacity(i64),
    /// The disk-cache directory is blank (C# `DiskCacheStore` constructor, `ArgumentException`).
    DiskDirectory(InvalidStoreArgument),
    /// A persist worker or the startup-cleanup thread could not be started (no C# twin).
    Spawn(io::Error),
}

impl fmt::Display for PreviewServiceBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("W4-14a")
    }
}

impl std::error::Error for PreviewServiceBuildError {}

/// What a thread started by [`DecodeThreadSpawner`] does (C# `Task.Run` vs `TaskCreationOptions.LongRunning` +
/// `ThreadPriority.AboveNormal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecodeThreadKind {
    /// A preload/compare decode (`get_preview`): C# `Task.Run` on the thread pool, normal priority.
    Preview,
    /// A viewer decode (`get_viewer_preview`): C# dedicated thread, `AboveNormal` priority.
    Viewer,
    /// A full-resolution original decode (`decode_original`): C# dedicated thread, `AboveNormal` priority.
    Original,
    /// One of the [`disk::PERSIST_WORKER_COUNT`] persist workers (process lifetime).
    PersistWorker,
    /// The one-shot startup cleanup (legacy files + stale temp files).
    StartupCleanup,
}

/// Starts work on another thread. The default ([`StdThreadSpawner`]) spawns one named `std::thread` per job and changes
/// no priority; `pr_platform_win` supplies one that raises `Viewer`/`Original` threads to above-normal priority
/// (`SetThreadPriority`, the C# `Thread.CurrentThread.Priority = AboveNormal`), and the App may run `Preview` jobs on a
/// bounded pool. `Err` = the thread could not be started; the job was not run and is dropped.
pub trait DecodeThreadSpawner: Send + Sync {
    fn spawn(&self, kind: DecodeThreadKind, job: Box<dyn FnOnce() + Send + 'static>) -> io::Result<()>;
}

/// Default [`DecodeThreadSpawner`]: `std::thread::Builder` with the names `PhotoReviewRs.PreviewDecode`,
/// `.ViewerDecode`, `.OriginalDecode`, `.PreviewPersist`, `.PreviewCacheCleanup`; the join handle is dropped (detached).
#[derive(Debug, Default, Clone, Copy)]
pub struct StdThreadSpawner;

impl DecodeThreadSpawner for StdThreadSpawner {
    fn spawn(&self, kind: DecodeThreadKind, job: Box<dyn FnOnce() + Send + 'static>) -> io::Result<()> {
        todo!("W4-14a")
    }
}

/// Constructor arguments (C# constructor parameters plus the statics the C# class reaches directly). Build with
/// [`PreviewServiceConfig::new`] (the required arguments, everything else at the C# default) and set the public fields.
pub struct PreviewServiceConfig {
    /// C# `metrics` (required).
    pub metrics: Arc<ReviewMetrics>,
    /// C# `isOriginalLoadingMode` (required).
    pub is_original_loading_mode: BoolSource,
    /// C# `targetDecodeBox` (required); for the width-only overload use [`PreviewServiceConfig::width_only`].
    pub target_decode_box: DecodeBoxSource,
    /// C# `platformCodec` (required, WP-04/WP-06): pixels <-> the platform image the presenter shows.
    pub platform_codec: Arc<dyn PlatformImageCodec>,
    /// The JPEG payload codec of the `.pv4` entries (C# static `WicImageEncoder` / `WicCacheImageReader`, W4-11e).
    pub payload_codec: Arc<dyn PreviewPayloadCodec>,
    /// C# `capacityBytes` (default [`DEFAULT_CAPACITY_BYTES`]); clamped by [`resolve_capacity`].
    pub capacity_bytes: i64,
    /// C# `diskCacheDirectory`; `None` = `AppPaths::from_environment().preview_cache_dir` (R0002: the Rust app's own
    /// `%LOCALAPPDATA%\PhotoReviewRs\cache`, never the C# app's `PhotoReview\cache`).
    pub disk_cache_directory: Option<PathBuf>,
    /// C# `diskCacheCapacityBytes` (default [`DEFAULT_DISK_CACHE_CAPACITY_BYTES`]); `<= 0` turns the disk cache off.
    pub disk_cache_capacity_bytes: i64,
    /// C# `disableDiskCacheOverride`: `Some(_)` wins over the capacity and the diagnostic variable.
    pub disable_disk_cache_override: Option<bool>,
    /// C# `decoder`: used for every backend when `decoder_factory` is `None`.
    pub decoder: Option<Arc<dyn ImageDecoder>>,
    /// C# `log`; `None` = `NullLog`.
    pub log: Option<Arc<dyn Log>>,
    /// C# `currentBackend`; `None` = always `DecoderBackend::Wpf`.
    pub current_backend: Option<BackendSource>,
    /// C# `decoderFactory`: when set, decoders come from it per requested backend (built once each, cached here too).
    pub decoder_factory: Option<Arc<dyn ImageDecoderFactory>>,
    /// C# `sourceBytesCache`.
    pub source_bytes_cache: Option<Arc<SourceBytesCache>>,
    /// C# `originalDimensionsCapacity` (C# `int`, default [`DEFAULT_ORIGINAL_DIMENSIONS_CAPACITY`]).
    pub original_dimensions_capacity: i32,
    /// C# `cacheRamPercent`.
    pub cache_ram_percent: Option<i32>,
    /// C# `preloadWindow`; `None` = `PreloadWindow::DEFAULT` (only used with a percent).
    pub preload_window: Option<PreloadWindow>,
    /// C# `sourceReader`; `None` = `PhysicalSourceReader`.
    pub source_reader: Option<Arc<dyn SourceReader>>,
    /// C# `rawFullDecoder`.
    pub raw_full_decoder: Option<Arc<dyn ImageDecoder>>,
    /// C# `isRawFullDecodeEnabled`; `None` = always false.
    pub is_raw_full_decode_enabled: Option<BoolSource>,
    /// Physical RAM in bytes (C# `RamBudgetPolicy.GetPhysicalMemoryBytes()`, called by the C# constructor itself; the
    /// composition root passes `ram_budget::get_physical_memory_bytes(..)`). `<= 0` = unknown. Default `0`.
    pub physical_memory_bytes: i64,
    /// The source stat seam for keys and `matches_current_source` (C# `FileInfo`); `None` = `PhysicalFileSystem`.
    pub file_system: Option<Arc<dyn FileSystem>>,
    /// Diagnostic switches (C# reads `PHOTOREVIEW_DIAG_DISABLE_DISKCACHE` once in the constructor and
    /// `PHOTOREVIEW_DIAG_PREREAD` on every decode; D4: the Rust names are `PHOTOREVIEW_RS_DIAG_*`). `None` =
    /// `DiagOptions::current()` read once at construction.
    pub diag: Option<Arc<DiagOptions>>,
    /// Clock of the disk store and of the stale-temp sweep (C# `DateTime.UtcNow`); `None` = `SystemClock`.
    pub clock: Option<Arc<dyn Clock>>,
    /// Thread starter; `None` = [`StdThreadSpawner`].
    pub spawner: Option<Arc<dyn DecodeThreadSpawner>>,
}

impl fmt::Debug for PreviewServiceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewServiceConfig")
            .field("capacity_bytes", &self.capacity_bytes)
            .field("disk_cache_directory", &self.disk_cache_directory)
            .field("disk_cache_capacity_bytes", &self.disk_cache_capacity_bytes)
            .finish_non_exhaustive()
    }
}

impl PreviewServiceConfig {
    /// The required arguments; every other field takes its C# default (see each field).
    pub fn new(
        metrics: Arc<ReviewMetrics>,
        is_original_loading_mode: BoolSource,
        target_decode_box: DecodeBoxSource,
        platform_codec: Arc<dyn PlatformImageCodec>,
        payload_codec: Arc<dyn PreviewPayloadCodec>,
    ) -> Self {
        todo!("W4-14a")
    }

    /// C# private `WidthOnly(targetDecodeWidth)`: `DecodeBox::new(target_decode_width(), 0)` on every read (height
    /// unconstrained). The width-only C# constructor overload is `new(.., width_only(f), ..)`.
    pub fn width_only(target_decode_width: Arc<dyn Fn() -> i32 + Send + Sync>) -> DecodeBoxSource {
        todo!("W4-14a")
    }
}

/// The in-flight load key (C# `(ImageCacheKey Key, long Epoch)`).
pub type LoadKey = (ImageCacheKey, i64);

/// Shared state (the C# instance fields). Fields used only by W4-14b/W4-14c are filled by those units' code; their
/// types live in `handle.rs` / `disk.rs`.
pub(crate) struct ServiceInner {
    // ---- configuration (W4-14a) ----
    pub(crate) metrics: Arc<ReviewMetrics>,
    pub(crate) is_original_loading_mode: BoolSource,
    pub(crate) target_decode_box: DecodeBoxSource,
    pub(crate) current_backend: BackendSource,
    pub(crate) platform_codec: Arc<dyn PlatformImageCodec>,
    pub(crate) payload_codec: Arc<dyn PreviewPayloadCodec>,
    pub(crate) log: Arc<dyn Log>,
    pub(crate) fs: Arc<dyn FileSystem>,
    pub(crate) source_reader: Arc<dyn SourceReader>,
    pub(crate) source_bytes_cache: Option<Arc<SourceBytesCache>>,
    pub(crate) diag: Arc<DiagOptions>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) spawner: Arc<dyn DecodeThreadSpawner>,
    /// Effective (post-clamp) RAM budget, C# `CapacityBytes`.
    pub(crate) capacity_bytes: i64,
    // ---- decoders (W4-14a builds, W4-14b uses) ----
    /// C# `_decoder`: `decoder` or `decoder_factory.create(current_backend())` (called once at construction).
    pub(crate) decoder: Arc<dyn ImageDecoder>,
    pub(crate) decoder_factory: Option<Arc<dyn ImageDecoderFactory>>,
    /// C# `_decodersByBackend` (only used with a factory).
    pub(crate) decoders_by_backend: Mutex<HashMap<DecoderBackend, Arc<dyn ImageDecoder>>>,
    pub(crate) raw_full_decoder: Option<Arc<dyn ImageDecoder>>,
    pub(crate) is_raw_full_decode_enabled: BoolSource,
    // ---- RAM cache and epoch (W4-14a; INV-2) ----
    /// C# `_cache`: size of an entry = `estimated_bytes()`.
    pub(crate) cache: BoundedLruCache<ImageCacheKey, PreviewImage>,
    /// C# `_originalDimensions` (entry-count LRU: each entry has size 1).
    pub(crate) original_dimensions: BoundedLruCache<ImageCacheKey, (i32, i32)>,
    /// C# `_cacheLifecycleGate`: makes "epoch still current -> set" atomic against "bump -> clear" and holds
    /// `evict_cached_path`'s removal + callback. Never held across a decode or file I/O.
    pub(crate) lifecycle_gate: Mutex<()>,
    /// C# `long _cacheEpoch` (D3: `AtomicI64`, wrapping increments, written only under `lifecycle_gate`).
    pub(crate) cache_epoch: AtomicI64,
    /// C# `CacheCleared` subscribers, in subscription order.
    pub(crate) cache_cleared: Mutex<Vec<(u64, CacheClearedListener)>>,
    pub(crate) next_listener_id: AtomicU64,
    // ---- in-flight and viewer/original gates (W4-14b) ----
    /// C# `_previewLoads`.
    pub(crate) in_flight: Mutex<HashMap<LoadKey, Arc<handle::LoadCell>>>,
    /// C# `_viewerSlots` ([`VIEWER_DECODE_SLOTS`] tokens).
    pub(crate) viewer_slots: handle::SlotGate,
    /// C# `_activeViewerDecodes`.
    pub(crate) active_viewer_decodes: AtomicI32,
    /// C# `_originalDecodeGate` (1 token, R2-F-13).
    pub(crate) original_gate: handle::SlotGate,
    // ---- disk cache (W4-14c) ----
    pub(crate) disk_directory: PathBuf,
    /// C# `_disableDiskCache` (resolved once, see the contract semantics 3).
    pub(crate) disable_disk_cache: bool,
    pub(crate) disk_store: DiskCacheStore,
    pub(crate) persist: disk::PersistState,
}

/// C# `PreviewImageService` (also C# `IPreloadTarget`; the W4-15 trait impl delegates to the inherent methods, see the
/// W4-14a contract). `Send + Sync`; every method takes `&self`.
pub struct PreviewImageService {
    pub(crate) inner: Arc<ServiceInner>,
}

impl fmt::Debug for PreviewImageService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewImageService")
            .field("capacity_bytes", &self.inner.capacity_bytes)
            .field("disk_directory", &self.inner.disk_directory)
            .field("disable_disk_cache", &self.inner.disable_disk_cache)
            .finish_non_exhaustive()
    }
}

/// Unsubscribes its [`CacheClearedListener`] when dropped (C# `event -=`).
#[must_use = "dropping the subscription unsubscribes at once"]
pub struct CacheClearedSubscription {
    inner: std::sync::Weak<ServiceInner>,
    id: u64,
}

impl fmt::Debug for CacheClearedSubscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheClearedSubscription")
            .field("id", &self.id)
            .finish()
    }
}

impl Drop for CacheClearedSubscription {
    fn drop(&mut self) {
        todo!("W4-14a: remove `id` from the subscriber list if the service is still alive")
    }
}

impl PreviewImageService {
    /// C# constructor (both overloads). Order (C#): codec/reader/raw decoder/metrics/sources; disk directory (default
    /// see [`PreviewServiceConfig::disk_cache_directory`]); log; the disk store (`*.pv4`, the disk capacity, the log,
    /// the clock); `disable_disk_cache` (semantics 3); the decoder (`decoder`, else `factory.create(current_backend())`,
    /// else [`PreviewServiceBuildError::NoDecoder`]); the original-dimensions cache; [`resolve_capacity`] with
    /// `source_bytes_cache.capacity_bytes()`, then ONE `log.info(line)`; the RAM cache; then W4-14c starts the persist
    /// workers and the startup cleanup (`ServiceInner::start_background`, disk.rs).
    pub fn new(config: PreviewServiceConfig) -> Result<Self, PreviewServiceBuildError> {
        todo!("W4-14a")
    }

    /// C# `CapacityBytes`: the effective (post-clamp) RAM budget.
    pub fn capacity_bytes(&self) -> i64 {
        todo!("W4-14a")
    }

    /// C# `DiskDirectory`.
    pub fn disk_directory(&self) -> &Path {
        todo!("W4-14a")
    }

    /// C# `DiskStore`.
    pub fn disk_store(&self) -> &DiskCacheStore {
        todo!("W4-14a")
    }

    /// C# `CacheCount` (also `IPreloadTarget.CacheCount`).
    pub fn cache_count(&self) -> usize {
        todo!("W4-14a")
    }

    /// C# `CacheBytes` (also `IPreloadTarget.CacheBytes`).
    pub fn cache_bytes(&self) -> i64 {
        todo!("W4-14a")
    }

    /// C# `IsOriginalLoadingMode()`: calls the configured source.
    pub fn is_original_loading_mode(&self) -> bool {
        todo!("W4-14a")
    }

    /// C# `ActiveViewerDecodes` (also `IPreloadTarget.ActiveViewerDecodes`): viewer decodes running right now.
    pub fn active_viewer_decodes(&self) -> i32 {
        todo!("W4-14a")
    }

    /// C# `GetCurrentCacheKey(string path)`: `get_current_cache_key_for_box(path, (target_decode_box)())`.
    pub fn get_current_cache_key(&self, path: &str) -> Result<ImageCacheKey, CacheKeyError> {
        todo!("W4-14a")
    }

    /// C# `GetCurrentCacheKey(string path, DecodeBox targetBox)` (P-1 startup): reads the loading mode ONCE; Original =>
    /// box `UNBOUNDED` and `is_original = true` (the given box is ignored); options: orientation applied, the CURRENT
    /// backend (one call), source kind [`source_kind_for`]`(path)`. One stat through the file system.
    pub fn get_current_cache_key_for_box(
        &self,
        path: &str,
        target_box: DecodeBox,
    ) -> Result<ImageCacheKey, CacheKeyError> {
        todo!("W4-14a")
    }

    /// C# `GetCurrentCacheKey(CatalogEntry entry)` (also `IPreloadTarget`): as above with the entry overload
    /// (`ImageCacheKey::create_for_entry`, no stat when the entry has length and write time); the box is read from the
    /// source only when NOT in Original mode (C# evaluation order: mode, kind, then box).
    pub fn get_current_cache_key_for_entry(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, CacheKeyError> {
        todo!("W4-14a")
    }

    /// C# `TryGetCachedPreview(string path, out image)` (also `IPreloadTarget.TryGetCachedPreview(string)`): builds the
    /// current key; ANY key failure (missing file, bad path) is a quiet miss (`None`), like the C# `catch (IOException)`
    /// / `catch (UnauthorizedAccessException)`. A hit makes the entry most recently used.
    pub fn try_get_cached_preview_path(&self, path: &str) -> Option<PreviewImage> {
        todo!("W4-14a")
    }

    /// C# `TryGetCachedPreview(ImageCacheKey key, out image)`: RAM cache only; no metrics.
    pub fn try_get_cached_preview(&self, key: &ImageCacheKey) -> Option<PreviewImage> {
        todo!("W4-14a")
    }

    /// C# `IPreloadTarget.CachedPreviewBytes(key)`: `estimated_bytes()` of the cached preview, `None` when not cached.
    pub fn cached_preview_bytes(&self, key: &ImageCacheKey) -> Option<i64> {
        todo!("W4-14a")
    }

    /// C# `HasInflightPreview(key)`: a load for `(key, current epoch)` is registered.
    pub fn has_inflight_preview(&self, key: &ImageCacheKey) -> bool {
        todo!("W4-14a")
    }

    /// C# `EvictCachedPath(path, alsoInvalidate)`: normalizes with the key's own normalization (`normalize_key_path`,
    /// full path + `ToUpperInvariant`; a bad path is `Err(InvalidPath)` BEFORE anything is evicted), evicts the
    /// source-bytes cache for `path` (as given; an error is `Err(SourceBytes)` and stops here, like the C# exception),
    /// then under the lifecycle gate removes every RAM entry whose key path equals the normalized path (ORDINAL) and
    /// calls `also_invalidate(normalized)`. NO epoch bump (R2-F-02). `also_invalidate` runs under a non-reentrant
    /// mutex: it must not call back into this service.
    pub fn evict_cached_path(&self, path: &str, also_invalidate: Option<&dyn Fn(&str)>) -> Result<(), PreviewError> {
        todo!("W4-14a")
    }

    /// C# `CacheCleared += handler` (WP-21, INV-2): `listener` runs on the thread that called [`Self::clear_cache`],
    /// after the RAM cache and the original dimensions were cleared, outside every service lock.
    pub fn subscribe_cache_cleared(&self, listener: CacheClearedListener) -> CacheClearedSubscription {
        todo!("W4-14a")
    }

    /// C# `ClearCache()`: under the lifecycle gate bump the epoch (wrapping) and clear the RAM cache; then clear the
    /// original dimensions; then raise `CacheCleared` (snapshot of the subscribers, called in subscription order).
    pub fn clear_cache(&self) {
        todo!("W4-14a")
    }

    /// C# `ClearSourceBytesCache()`.
    pub fn clear_source_bytes_cache(&self) {
        todo!("W4-14a")
    }

    /// C# `TryGetKnownOriginalDimensions(currentKey, out dimensions)`: lookup under
    /// `ImageCacheKey::create_original(current_key)`; never touches the file.
    pub fn try_get_known_original_dimensions(&self, current_key: &ImageCacheKey) -> Option<(i32, i32)> {
        todo!("W4-14a")
    }

    /// C# `IsRawFullDecodeRequest(path)`: a RAW full decoder is configured AND enabled now AND the extension is RAW.
    pub fn is_raw_full_decode_request(&self, path: &str) -> bool {
        todo!("W4-14a")
    }
}

impl Drop for PreviewImageService {
    /// No C# twin (the C# singleton is never disposed): completes the persist queue so the workers drain what is queued
    /// and exit (`disk::PersistState::complete`), without waiting for them.
    fn drop(&mut self) {
        todo!("W4-14a")
    }
}

impl ServiceInner {
    /// C# `Volatile.Read(ref _cacheEpoch)`.
    pub(crate) fn current_epoch(&self) -> i64 {
        todo!("W4-14a")
    }

    /// C# `lock (_cacheLifecycleGate) if (cacheEpoch == _cacheEpoch) _cache.Set(key, image)`: publishes only while the
    /// epoch the load started under is still current. Returns whether it was stored.
    pub(crate) fn publish_if_current(&self, key: ImageCacheKey, image: PreviewImage, cache_epoch: i64) -> bool {
        todo!("W4-14a")
    }
}

/// C# `SourceKindFor(path)`: `ImageSourceKind::RAW_PREVIEW` when `pr_core::catalog::types::is_raw_path(path)`, else
/// `STANDARD`.
pub fn source_kind_for(path: &str) -> u8 {
    todo!("W4-14a")
}

/// C# internal static `ResolveCapacity(requestedBytes, ramPercent, physicalBytes, sourceBytesCapacity, out logLine,
/// preloadWindow)`: the effective preview-cache capacity and the start-up "Memory budgets" line (IMG-11, R2-A-06, Q-R31).
/// Text, integer division by MiB and the branch order are byte-identical to C# (golden `preview-budget-line`):
///
/// * percent given AND `physical_bytes > 0`: `percent = clamp_cache_percent(requested, physical, window)`,
///   `capacity = preview_bytes_for_percent(percent, physical, source, window)` (cannot fail: physical > 0);
///   `Memory budgets: preview cache {capacity/MiB} MiB; cache share {percent}% of {physical/MiB} MiB physical RAM =
///   {bytes_for_percent(percent, physical)/MiB} MiB for preview + source-bytes{clamp}{source}.` where `{clamp}` is
///   ` (requested {requested}% clamped to {percent}%; allowed {minimum_cache_percent(physical, window)}-90%)` only
///   when the percent changed;
/// * otherwise: `clamped = max(1, clamp_preview_to_physical_memory(requested, physical, source))`;
///   `Memory budgets: preview cache {clamped/MiB} MiB` + (` (physical RAM unknown: byte budget used)` when
///   `physical <= 0`) + (` (clamped from {requested/MiB} MiB to 50% of physical RAM)` when `clamped != requested`) +
///   `{source}` + `.`;
///
/// `{source}` = `, source-bytes cache off` for `None`, else `, source-bytes cache {source/MiB} MiB`; `source` = the
/// capacity or 0. `window` `None` = `PreloadWindow::DEFAULT`. Division truncates toward zero (C# `long /`).
pub fn resolve_capacity(
    requested_bytes: i64,
    ram_percent: Option<i32>,
    physical_bytes: i64,
    source_bytes_capacity: Option<i64>,
    preload_window: Option<PreloadWindow>,
) -> (i64, String) {
    todo!("W4-14a")
}

/// White-box red tests (W4-14a): the epoch (INV-2) and the in-flight table are `pub(crate)`, so integration tests cannot
/// reach them. The public-API twins live in `tests/preview_service_state.rs`.
#[cfg(test)]
mod white_box_tests {
    use super::*;
    use crate::decode_types::{DecodeRequest, DecodedImage, DecodedImageOptions, ImageInfo, PixelLease, PlatformImage};
    use crate::pixels::{PixelBuffer, PixelLayout};
    use pr_core::abstractions::NullLog;
    use pr_test_support::TempRoot;

    /// Decoder that is never asked to decode: the white-box tests build keys and publish images only.
    #[derive(Debug)]
    struct NullDecoder;

    impl ImageDecoder for NullDecoder {
        fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
            Err(DecodeError::InvalidOperation {
                message: "NullDecoder was asked to decode".to_owned(),
            })
        }

        fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
            Err(DecodeError::InvalidOperation {
                message: "NullDecoder was asked to read info".to_owned(),
            })
        }
    }

    #[derive(Debug)]
    struct NullCodec;

    impl PlatformImageCodec for NullCodec {
        fn name(&self) -> &'static str {
            "null"
        }

        fn image_from_pixels(&self, _pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
            Err(DecodeError::InvalidOperation {
                message: "NullCodec".to_owned(),
            })
        }

        fn to_pixels<'a>(&self, _platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
            Err(DecodeError::InvalidOperation {
                message: "NullCodec".to_owned(),
            })
        }
    }

    #[derive(Debug)]
    struct NullPayloadCodec;

    impl PreviewPayloadCodec for NullPayloadCodec {
        fn encode_jpeg(&self, _pixels: &PixelBuffer, _quality: i32, _out: &mut dyn io::Write) -> io::Result<()> {
            Err(io::Error::other("NullPayloadCodec"))
        }

        fn decode_jpeg(
            &self,
            _payload: &[u8],
            _layout: PixelLayout,
            _expected_width: i32,
            _expected_height: i32,
        ) -> Result<PixelBuffer, PreviewCacheError> {
            Err(PreviewCacheError::InvalidData("NullPayloadCodec".to_owned()))
        }
    }

    fn service(root: &TempRoot) -> PreviewImageService {
        let mut config = PreviewServiceConfig::new(
            Arc::new(ReviewMetrics::new()),
            Arc::new(|| false),
            Arc::new(|| DecodeBox::new(512, 512)),
            Arc::new(NullCodec),
            Arc::new(NullPayloadCodec),
        );
        config.capacity_bytes = 64 * 1024 * 1024;
        config.disk_cache_directory = Some(root.path().to_path_buf());
        config.disable_disk_cache_override = Some(true);
        config.decoder = Some(Arc::new(NullDecoder));
        config.physical_memory_bytes = 32 * 1024 * 1024 * 1024;
        config.log = Some(Arc::new(NullLog));
        PreviewImageService::new(config).expect("the service builds")
    }

    fn source(root: &TempRoot) -> String {
        let path = root.join("a.jpg");
        std::fs::write(&path, [1u8, 2, 3, 4]).expect("write source file");
        path.to_string_lossy().into_owned()
    }

    fn image() -> PreviewImage {
        let pixels = PixelBuffer::allocate(4, 4, PixelLayout::Bgr32).expect("test buffer");
        let options = DecodedImageOptions {
            downscaled: false,
            orientation: 1,
            actual_backend: DecoderBackend::Wpf,
            original_width: 0,
            original_height: 0,
            exif: None,
            is_degraded_fallback: false,
        };
        let image = DecodedImage::new(PlatformImage::Pixels(Arc::new(pixels)), 4, 4, 64, options).expect("test image");
        Arc::new(image)
    }

    // Rust-only: INV-2, a publish that started under the epoch before clear_cache is refused, the current epoch publishes.
    #[test]
    fn clear_cache_bumps_the_epoch_so_publish_if_current_refuses_the_old_epoch() {
        let root = TempRoot::new("preview-white-box-epoch");
        let path = source(&root);
        let service = service(&root);
        let key = service.get_current_cache_key(&path).expect("key");
        let old_epoch = service.inner.current_epoch();

        service.clear_cache();

        assert!(!service.inner.publish_if_current(key.clone(), image(), old_epoch));
        assert_eq!(service.cache_count(), 0);
        let current = service.inner.current_epoch();
        assert!(service.inner.publish_if_current(key, image(), current));
        assert_eq!(service.cache_count(), 1);
    }

    // Rust-only: INV-2 (R2-F-02), evicting one path never bumps the epoch, so no other path's in-flight decode is dropped.
    #[test]
    fn evict_cached_path_does_not_bump_the_epoch() {
        let root = TempRoot::new("preview-white-box-evict-epoch");
        let path = source(&root);
        let service = service(&root);
        let before = service.inner.current_epoch();

        service.evict_cached_path(&path, None).expect("evict");

        assert_eq!(service.inner.current_epoch(), before);
    }

    // Rust-only: an in-flight load is registered under (key, epoch); after a clear the old epoch no longer counts.
    #[test]
    fn has_inflight_preview_is_keyed_by_the_current_epoch() {
        let root = TempRoot::new("preview-white-box-inflight");
        let path = source(&root);
        let service = service(&root);
        let key = service.get_current_cache_key(&path).expect("key");
        let epoch = service.inner.current_epoch();
        let cell = Arc::new(handle::LoadCell::new());
        service
            .inner
            .in_flight
            .lock()
            .expect("in-flight table")
            .insert((key.clone(), epoch), cell);
        assert!(service.has_inflight_preview(&key));

        service.clear_cache();

        assert!(!service.has_inflight_preview(&key));
    }
}
