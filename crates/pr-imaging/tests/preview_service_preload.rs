//! Port tests for `impl PreloadTarget for PreviewImageService` (W4-14d, D43) and of the 8 tests of the C# class
//! `PreloadSchedulerTests` (`tests/PhotoReview.Imaging.Tests/PreviewImageServiceTests.cs`, C# baseline d92d6aae, lines
//! 216-426). The scheduler is driven through the trait object `Arc<dyn PreloadTarget>`, so the adapter is on the path.
//!
//! Red by design: `PreviewServiceConfig::new` and the scheduler bodies are `todo!()`, so every test here fails until the
//! W4-14a / W4-14b / W4-14c / W4-15b porters land. The adapter bodies in `service/mod.rs` are `todo!()` too.
//!
//! Adaptations (C# -> Rust, no behaviour change intended):
//! - The C# tests decode a real PNG (`TestImages.PreviewPng`) through WPF. Here the decoder is `FakeDecoder` (a 64 x 48
//!   opaque `Bgr32` buffer) and the platform codec is `PixelBufferImageCodec`; the files on disk only need to exist (the key
//!   stats them). The real-codec variants are native WIC tests (W4-11c / W4-11e).
//! - `FakeMemoryProbe(true)` of C# becomes `DelegateMemoryProbe::new(|_| true)` (the same "always headroom" probe).
//! - C# `DisposeAsync` (shut down the persist workers, wait for the prune, delete the temp root) is `Fixture::drop`.
//! - C# `PreloadAroundAsync` (awaited) is `preload_around(..)` plus `run_to_completion`, a bounded wait (10 s guard).
//! - C# `TryGetCachedPreview(path, out image)` is `try_get_cached_preview_path(path)`.
//! - Test names mirror the C# `[Fact(DisplayName)]` method names in snake case; each has a comment naming its C# test.
//!
//! Documented skips (not ported here):
//! - Persist-related behaviour of the service (disk cache files, workers) is owned by W4-14c; the fixture only shuts the
//!   workers down so the temp root can be deleted.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
use pr_imaging::decode_types::{
    DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging::preload::scheduler::{
    CallbackError, EntriesSnapshotFn, PreloadScheduler, PreloadSchedulerDeps, RunHandle, ThresholdOptions,
};
use pr_imaging::preload::target::{DelegateMemoryProbe, PreloadTarget};
use pr_imaging::preload_order::{PreloadOptions, PreloadWindow};
use pr_imaging::service::{PreviewImageService, PreviewServiceConfig};
use pr_test_support::TempRoot;

/// Hang guard for every wait of a test (C# has no bound; the Rust tests never wait unbounded).
const RUN_TIMEOUT: Duration = Duration::from_secs(10);
/// C# `DisposeAsync` waits for the persist workers and the prune with these bounds.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const PRUNE_TIMEOUT: Duration = Duration::from_secs(5);
/// Dimensions of the fake decoded image (C# `TestImages.PreviewPng` is a small opaque PNG).
const IMAGE_WIDTH: i32 = 64;
const IMAGE_HEIGHT: i32 = 48;

/// Fake decoder: every decode is an opaque `Bgr32` buffer of `IMAGE_WIDTH x IMAGE_HEIGHT`; no file is read.
struct FakeDecoder;

impl ImageDecoder for FakeDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let pixels = PixelBuffer::allocate(IMAGE_WIDTH, IMAGE_HEIGHT, PixelLayout::Bgr32).expect("fake pixel buffer");
        let estimated_bytes = i64::from(IMAGE_WIDTH) * i64::from(IMAGE_HEIGHT) * 4;
        let image = DecodedImage::new(
            PlatformImage::Pixels(Arc::new(pixels)),
            IMAGE_WIDTH,
            IMAGE_HEIGHT,
            estimated_bytes,
            DecodedImageOptions::default(),
        )
        .expect("fake decoded image");
        let view: Arc<dyn DecodedImageView> = Arc::new(image);
        Ok(view)
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(IMAGE_WIDTH, IMAGE_HEIGHT))
    }
}

/// Fake `.pv4` payload codec: writes a marker payload; decoding is refused (no test here reads the disk cache).
struct FakePayloadCodec;

impl PreviewPayloadCodec for FakePayloadCodec {
    fn encode_jpeg(&self, _pixels: &PixelBuffer, _quality: i32, out: &mut dyn std::io::Write) -> std::io::Result<()> {
        out.write_all(b"fake-jpeg-payload")
    }

    fn decode_jpeg(
        &self,
        _payload: &[u8],
        _layout: PixelLayout,
        _expected_width: i32,
        _expected_height: i32,
    ) -> Result<PixelBuffer, PreviewCacheError> {
        Err(PreviewCacheError::InvalidData(
            "fake payload codec does not decode".to_owned(),
        ))
    }
}

/// Writes `count` small files named `<prefix>-<i>.png` into `dir` (created) and returns their paths.
fn write_files(dir: &Path, prefix: &str, count: usize) -> Vec<String> {
    std::fs::create_dir_all(dir).expect("create test folder");
    (0..count)
        .map(|i| {
            let path = dir.join(format!("{prefix}-{i}.png"));
            std::fs::write(&path, b"fake preview bytes").expect("write test image");
            path.to_string_lossy().into_owned()
        })
        .collect()
}

/// C# `ToEntries(files)`: a catalog snapshot of the given paths.
fn entries_snapshot(files: &[String]) -> EntriesSnapshotFn {
    let entries: Arc<[CatalogEntry]> = files
        .iter()
        .map(|file| CatalogEntry::new(file.clone()).expect("catalog entry"))
        .collect::<Vec<_>>()
        .into();
    Arc::new(move || -> Result<Arc<[CatalogEntry]>, CallbackError> { Ok(Arc::clone(&entries)) })
}

/// Scheduler dependencies that never read the process environment (hermetic worker count, fixed processor count).
fn hermetic_deps(options: Option<PreloadOptions>) -> PreloadSchedulerDeps {
    PreloadSchedulerDeps {
        options,
        env_lookup: Some(Arc::new(|_: &str| -> Option<String> { None })),
        processor_count: Some(8),
        ..PreloadSchedulerDeps::default()
    }
}

/// Bounded wait for one scheduler run (C# `await PreloadAroundAsync(..)`).
fn run_to_completion(run: &RunHandle) {
    assert!(
        run.wait_timeout(RUN_TIMEOUT),
        "preload run did not finish within {RUN_TIMEOUT:?}"
    );
}

/// Owns the temp root, the test files and every service a test created. `Drop` is C# `DisposeAsync`: the persist
/// workers of each service are shut down and its prune is awaited BEFORE the temp root is deleted.
struct Fixture {
    root: TempRoot,
    preload_files: Vec<String>,
    single_files: Vec<String>,
    services: Vec<Arc<PreviewImageService>>,
}

impl Fixture {
    /// C# `PreloadSchedulerTests()`: four preload files and two single files.
    fn new() -> Self {
        let root = TempRoot::new("preload-scheduler");
        let preload_files = write_files(&root.join("preload-scheduler"), "preload", 4);
        let single_files = write_files(&root.join("preload-single"), "single", 2);
        Self {
            root,
            preload_files,
            single_files,
            services: Vec::new(),
        }
    }

    /// A `PreviewImageService` with the C# test defaults (64 MiB capacity, original mode off, width 256, the fake decoder
    /// and a disk cache directory under the temp root). C# `new PreviewImageService(metrics, () => false, () => 256,
    /// WpfBitmapSourceCodec.Instance, capacityBytes: 64 MiB, diskCacheDirectory: ...)`.
    fn service(&mut self, metrics: &Arc<ReviewMetrics>, disk_directory_name: &str) -> Arc<PreviewImageService> {
        let disk_directory = self.root.join(disk_directory_name);
        let decoder: Arc<dyn ImageDecoder> = Arc::new(FakeDecoder);
        let mut config = PreviewServiceConfig::new(
            Arc::clone(metrics),
            Arc::new(|| false),
            PreviewServiceConfig::width_only(Arc::new(|| 256)),
            PixelBufferImageCodec::shared(),
            Arc::new(FakePayloadCodec),
        );
        config.capacity_bytes = 64 * 1024 * 1024;
        config.disk_cache_directory = Some(disk_directory);
        config.decoder = Some(decoder);
        let service = Arc::new(PreviewImageService::new(config).expect("preview service builds"));
        self.services.push(Arc::clone(&service));
        service
    }

    /// C# `NewWarmScheduler(hasHeadroom, workerCount)`: the convenience constructor with `memoryLoadLimit: 1.0`, the
    /// given headroom answer and the given worker override (`None` = the default count).
    fn warm_scheduler(
        &self,
        files: &[String],
        metrics: &Arc<ReviewMetrics>,
        service: &Arc<PreviewImageService>,
        has_headroom: impl Fn(f64) -> bool + Send + Sync + 'static,
        worker_count: Option<i32>,
    ) -> PreloadScheduler {
        let target: Arc<dyn PreloadTarget> = service.clone();
        let memory_probe = Arc::new(DelegateMemoryProbe::new(has_headroom));
        let thresholds = ThresholdOptions {
            full_folder_ram_threshold_bytes: i64::MAX,
            memory_load_limit: 1.0,
            worker_count_override: worker_count,
            window: None,
        };
        PreloadScheduler::with_thresholds(
            target,
            Arc::clone(metrics),
            entries_snapshot(files),
            memory_probe,
            thresholds,
            hermetic_deps(None),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for service in &self.services {
            // Every queued write must finish before the temp root is deleted.
            let _ = service.shutdown_persist_workers_timeout(SHUTDOWN_TIMEOUT);
            let _ = service.wait_for_prune(PRUNE_TIMEOUT);
        }
    }
}

#[test]
fn preload_target_adapter_is_object_safe_and_shared() {
    // No C# twin: pins that the adapter can be shared as the scheduler's `Arc<dyn PreloadTarget>` (the App keeps the
    // concrete service too, so both handles must refer to one cache).
    let mut fixture = Fixture::new();
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-adapter");
    let target: Arc<dyn PreloadTarget> = service.clone();
    let path = fixture.preload_files[0].clone();
    assert!(
        !target.try_get_cached_preview_for_path(&path),
        "nothing is cached before any preload"
    );
    assert_eq!(target.cache_count(), 0);
}

// C# `PreloadSchedulerTests.MemoryGuardDecodesNothingWithoutHeadroom` ("Background preload memory guard decodes nothing
// when there is no memory headroom").
#[test]
fn memory_guard_decodes_nothing_without_headroom() {
    let mut fixture = Fixture::new();
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-memory-guard");
    let scheduler = fixture.warm_scheduler(&fixture.preload_files, &metrics, &service, |_| false, None);
    run_to_completion(&scheduler.preload_around(0));
    assert!(service.cache_count() == 0 && metrics.snapshot().source_reads == 0);
}

// C# `PreloadSchedulerTests.PreloadWarmsCacheAroundCurrentIndex` ("Background preload warms the cache around the current
// index when memory headroom allows").
#[test]
fn preload_warms_cache_around_current_index() {
    let mut fixture = Fixture::new();
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-warm");
    let scheduler = fixture.warm_scheduler(&fixture.preload_files, &metrics, &service, |_| true, None);
    run_to_completion(&scheduler.preload_around(0));
    assert!(service.cache_count() > 0 && metrics.snapshot().source_reads > 0);
}

// C# `PreloadSchedulerTests.PreloadPrioritizesNextImage` ("Background preload prioritizes the next image after the
// current index").
#[test]
fn preload_prioritizes_next_image() {
    let mut fixture = Fixture::new();
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-priority");
    let scheduler = fixture.warm_scheduler(&fixture.preload_files, &metrics, &service, |_| true, None);
    run_to_completion(&scheduler.preload_around(0));
    assert!(service.try_get_cached_preview_path(&fixture.preload_files[1]).is_some());
}

// C# `PreloadSchedulerTests.NavigatingToPreloadedImageNeedsNoExtraSourceOpenForOriginalDimensions` ("Navigating to a
// preloaded image needs 0 extra source opens to learn its original dimensions"). The preload seeds the dimensions that
// the navigation reads, so the lookup opens no source.
#[test]
fn navigating_to_preloaded_image_needs_no_extra_source_open_for_original_dimensions() {
    let mut fixture = Fixture::new();
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-dimensions");
    let scheduler = fixture.warm_scheduler(&fixture.preload_files, &metrics, &service, |_| true, None);
    run_to_completion(&scheduler.preload_around(0));

    let preloaded_path = fixture.preload_files[1].clone();
    let preloaded = service
        .try_get_cached_preview_path(&preloaded_path)
        .expect("the preloaded image is in the RAM cache");
    let opens_after_preload = metrics.snapshot().source_open_count;

    let key = service
        .get_current_cache_key(&preloaded_path)
        .expect("current cache key");
    let (width, height) = service
        .get_original_dimensions(&preloaded_path, &key)
        .expect("original dimensions");

    assert_eq!(opens_after_preload, metrics.snapshot().source_open_count);
    assert_eq!(
        (width, height),
        (preloaded.original_width(), preloaded.original_height())
    );
}

// C# `PreloadSchedulerTests.CancellingPreloadDoesNotPoisonScheduler` ("Cancelling preload does not poison the scheduler;
// the next request starts a fresh lifetime").
#[test]
fn cancelling_preload_does_not_poison_scheduler() {
    let mut fixture = Fixture::new();
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-cancel");
    let scheduler = fixture.warm_scheduler(&fixture.preload_files, &metrics, &service, |_| true, None);
    run_to_completion(&scheduler.preload_around(0));
    scheduler.cancel();
    let reloaded_count = service.cache_count();
    run_to_completion(&scheduler.preload_around(2));
    assert!(service.cache_count() >= reloaded_count);
}

// C# `PreloadSchedulerTests.ClearingPreloadedKeysDropsEveryRecord` ("Clearing preloaded keys drops every warmed-key
// record").
#[test]
fn clearing_preloaded_keys_drops_every_record() {
    let mut fixture = Fixture::new();
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-clear-keys");
    let scheduler = fixture.warm_scheduler(&fixture.preload_files, &metrics, &service, |_| true, None);
    run_to_completion(&scheduler.preload_around(0));
    scheduler.cancel();
    run_to_completion(&scheduler.preload_around(2));
    scheduler.clear_preloaded_keys();
    let key = service
        .get_current_cache_key(&fixture.preload_files[0])
        .expect("current cache key");
    assert!(!scheduler.try_consume_preloaded_key(&key));
}

// C# `PreloadSchedulerTests.PreloadedKeyIsReportedOnceThenConsumed` ("A preloaded key is reported once and then consumed
// so a hit is not counted twice"). Two files, so exactly one preload worker runs (C# comment: concurrent workers lose
// records); here the worker count is pinned to 1 for the same reason.
#[test]
fn preloaded_key_is_reported_once_then_consumed() {
    let mut fixture = Fixture::new();
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-single");
    let scheduler = fixture.warm_scheduler(&fixture.single_files, &metrics, &service, |_| true, Some(1));
    run_to_completion(&scheduler.preload_around(0));
    let warmed_key = service
        .get_current_cache_key(&fixture.single_files[1])
        .expect("current cache key");
    assert!(scheduler.try_consume_preloaded_key(&warmed_key));
    assert!(!scheduler.try_consume_preloaded_key(&warmed_key));
}

// C# `PreloadSchedulerTests.PreloadWithoutDispatcherContinuesBeyondFirstBatchAndLoadsAllFiles` ("Preload without
// Dispatcher continues beyond first batch and loads all files"). Regression D10: no UI scheduler may stop the run after
// the first batch of workers.
#[test]
fn preload_without_dispatcher_continues_beyond_first_batch_and_loads_all_files() {
    let mut fixture = Fixture::new();
    let batch_files = write_files(&fixture.root.join("preload-batch"), "batch", 6);
    let metrics = Arc::new(ReviewMetrics::new());
    let service = fixture.service(&metrics, "disk-cache-batch");

    // 2 workers, 6 files: the run needs several batches.
    let options = PreloadOptions {
        worker_count: 2,
        memory_load_limit: 1.0,
        reserve_bytes: 2 * 1024 * 1024 * 1024,
        full_folder_threshold_bytes: 16 * 1024 * 1024 * 1024,
        window: PreloadWindow::DEFAULT,
    };
    let target: Arc<dyn PreloadTarget> = service.clone();
    let memory_probe = Arc::new(DelegateMemoryProbe::new(|_| true));
    let scheduler = PreloadScheduler::new(
        target,
        Arc::clone(&metrics),
        entries_snapshot(&batch_files),
        memory_probe,
        hermetic_deps(Some(options)),
    );

    run_to_completion(&scheduler.preload_around(0));

    // Files around center 0 are cached across the batches.
    assert!(
        service.cache_count() >= 5,
        "Expected at least 5 cached files, but got {}",
        service.cache_count()
    );
}
