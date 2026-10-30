//! Deterministic CX-BENCH mutation guards; all file I/O is confined to TempRoot.

#[path = "../../pr-imaging/tests/support/preview_service.rs"]
mod preview_service;

use std::{sync::Arc, time::Duration};

use pr_bench::image_executor::{
    BenchmarkExecutorOptions, BenchmarkImageExecutor, BenchmarkProfileScope, ExecutorError, TeardownHandle,
    WindowedPreloadTarget,
};
use pr_bench::profiles;
use pr_core::text::dotnet_path::PathError;
use pr_imaging::preload::target::PreloadTarget;
use pr_imaging::service::PreviewServiceBuildError;
use pr_test_support::{RecordingLog, TempRoot};
use preview_service::{FakeDecoder, FakePayloadCodec, PixelsCodec};

use pr_bench::models::BenchmarkWorkload;
use pr_bench::workload_runner::{DotNetRandom, prepare_iteration};
use pr_core::abstractions::RecycleBin;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, PlatformImage};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};

struct DimensionImage {
    width: i32,
    height: i32,
    inner: Arc<dyn DecodedImageView>,
}

impl DecodedImageView for DimensionImage {
    fn pixel_width(&self) -> i32 {
        self.width
    }
    fn pixel_height(&self) -> i32 {
        self.height
    }
    fn downscaled(&self) -> bool {
        false
    }
    fn orientation(&self) -> i32 {
        1
    }
    fn estimated_bytes(&self) -> i64 {
        self.inner.estimated_bytes()
    }
    fn platform_image(&self) -> &PlatformImage {
        self.inner.platform_image()
    }
}

struct DimensionDecoder(i32, i32);

impl ImageDecoder for DimensionDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Ok(Arc::new(DimensionImage {
            width: self.0,
            height: self.1,
            inner: FakeDecoder::new().decode(request)?,
        }))
    }
    fn read_info(&self, _: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(self.0, self.1))
    }
}

struct UnusedBin;

impl RecycleBin for UnusedBin {
    fn send_to_recycle_bin(&self, _: &std::path::Path) -> std::io::Result<()> {
        panic!("decode-only workloads must never use a recycle bin")
    }
    fn try_restore(&self, _: &std::path::Path, _: u64, _: pr_core::time::DateTimeUtc) -> std::io::Result<bool> {
        panic!("decode-only workloads must never restore")
    }
}

#[test]
fn workload_correctness_rejects_each_nonpositive_dimension_in_every_decode_branch() {
    // Mutants workload_runner.rs:538,555,578: > -> >=, && -> ||, &= -> |=.
    // C#: BenchmarkWorkloadRunner checks width > 0 AND height > 0 in each decode branch.
    let root = TempRoot::new("w10-dimensions");
    let source = root.join("synthetic.png");
    std::fs::write(&source, b"fake decoder input").unwrap();
    let files: Arc<[String]> = Arc::from([source.to_str().unwrap().to_owned()]);
    let profile = profiles::find("no-preload-baseline").unwrap();
    let bin: Arc<dyn RecycleBin> = Arc::new(UnusedBin);
    for workload in [
        BenchmarkWorkload::FirstFrame,
        BenchmarkWorkload::Preload,
        BenchmarkWorkload::WarmNext,
        BenchmarkWorkload::Sequential,
        BenchmarkWorkload::Random,
        BenchmarkWorkload::Correctness,
    ] {
        for (width, height) in [(0, 3), (4, 0), (-1, 3), (4, -1), (4, 3)] {
            let executor = Arc::new(
                BenchmarkImageExecutor::new(
                    profile,
                    &files,
                    Arc::new(PixelsCodec),
                    Arc::new(FakePayloadCodec),
                    BenchmarkExecutorOptions {
                        decoder: Some(Arc::new(DimensionDecoder(width, height))),
                        log: Some(Arc::new(RecordingLog::new())),
                        scratch_parent: Some(root.join("scratch")),
                        ..Default::default()
                    },
                )
                .unwrap(),
            );
            let mut random = DotNetRandom::new(1);
            let measure = prepare_iteration(
                &executor,
                &files,
                profile,
                workload,
                0,
                &mut random,
                &bin,
                &CancelToken::none(),
            )
            .unwrap();
            let outcome = measure();
            executor.dispose();
            assert!(
                executor
                    .teardown_background_task()
                    .wait_timeout(Duration::from_secs(30))
            );
            assert_eq!(
                outcome.unwrap().correct,
                width > 0 && height > 0,
                "{workload:?}: {width}x{height}"
            );
        }
    }
}

#[test]
fn executor_error_display_preserves_both_underlying_errors() {
    // Mutant image_executor.rs:76: replace Display::fmt with Ok(Default::default()).
    // C#: BenchmarkImageExecutor propagates path/service constructor failures.
    let path = PathError::Argument;
    assert_eq!(
        ExecutorError::InvalidPath(path).to_string(),
        format!("invalid benchmark file path: {path}")
    );
    let service = PreviewServiceBuildError::NoDecoder;
    let expected = service.to_string();
    assert_eq!(ExecutorError::Service(service).to_string(), expected);
}

#[test]
fn executor_debug_reports_options_handles_scope_window_and_scratch() {
    // Mutants image_executor.rs:139,157,256,295,415: replace Debug::fmt with Ok(Default::default()).
    // Rust diagnostics of the C# BenchmarkImageExecutor/ProfileScope/WindowedPreloadTarget port.
    let root = TempRoot::new("w10-executor-debug");
    let options = BenchmarkExecutorOptions {
        decoder: Some(FakeDecoder::new()),
        log: Some(Arc::new(RecordingLog::new())),
        scratch_parent: Some(root.join("scratch")),
        ..Default::default()
    };
    let diagnostic = format!("{options:?}");
    assert!(diagnostic.starts_with("BenchmarkExecutorOptions { scratch_parent: Some("));
    assert!(diagnostic.contains("prune_wait_timeout: 120s"));
    assert_eq!(format!("{:?}", TeardownHandle::completed()), "TeardownHandle { .. }");
    let profile = profiles::find("no-preload-baseline").unwrap();
    let mut scope = BenchmarkProfileScope::apply_logging(profile, || false, |_| {}, None);
    assert_eq!(format!("{scope:?}"), "LoggingScope { pending: true }");
    scope.dispose();
    assert_eq!(format!("{scope:?}"), "LoggingScope { pending: false }");
    let executor =
        BenchmarkImageExecutor::new(profile, &[], Arc::new(PixelsCodec), Arc::new(FakePayloadCodec), options).unwrap();
    let inner: Arc<dyn PreloadTarget> = executor.preview_service_for_tests().clone();
    let window = WindowedPreloadTarget::new(inner, &[], profile).unwrap();
    let window_text = format!("{window:?}");
    let executor_text = format!("{executor:?}");
    executor.dispose();
    assert!(
        executor
            .teardown_background_task()
            .wait_timeout(Duration::from_secs(30))
    );
    assert_eq!(
        window_text,
        format!(
            "WindowedPreloadTarget {{ full_folder: {}, next: {}, previous: {}, .. }}",
            profile.full_folder, profile.next_window, profile.previous_window
        )
    );
    assert!(executor_text.starts_with("BenchmarkImageExecutor { disk_cache_directory: "));
    assert!(executor_text.contains("scratch"));
}

#[test]
fn guard_cache_claim_is_idempotent_and_rejects_empty_and_blocked_paths() {
    // Mutants tool_path_guard.rs:156,157,179: claim_cache_directory -> Ok(()),
    // delete ! on marker.is_file(), delete/negate empty-path guard.
    // C#: ToolPathGuard.ClaimCacheDirectory never overwrites an existing marker.
    use pr_bench::cli::tool_path_guard::{CACHE_MARKER, CACHE_MARKER_TEXT, claim_cache_directory};
    let root = TempRoot::new("w10-cache-claim");
    let cache = root.join("cache");
    claim_cache_directory(cache.to_str().unwrap()).unwrap();
    let marker = cache.join(CACHE_MARKER);
    assert_eq!(std::fs::read(&marker).unwrap(), CACHE_MARKER_TEXT.as_bytes());
    std::fs::write(&marker, b"existing owner marker").unwrap();
    claim_cache_directory(cache.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(&marker).unwrap(), b"existing owner marker");
    let empty = claim_cache_directory("").unwrap_err();
    assert_eq!(empty.type_name, "ArgumentException");
    assert_eq!(empty.message, "The value cannot be an empty string. (Parameter 'path')");
    let blocker = root.join("blocker");
    std::fs::write(&blocker, b"preserve me").unwrap();
    for path in [blocker.clone(), blocker.join("child")] {
        let error = claim_cache_directory(path.to_str().unwrap()).unwrap_err();
        assert_eq!(error.type_name, "IOException");
        assert!(error.message.contains(blocker.to_str().unwrap()));
    }
    assert_eq!(std::fs::read(blocker).unwrap(), b"preserve me");
}

#[test]
fn guard_real_rejects_whitespace_and_nul_before_resolution() {
    // Mutants tool_path_guard.rs:40,45: delete !/constant guard replacements in real.
    // C#: ToolPathGuard.Real/ResolveRealPath validates arguments before filesystem I/O.
    use pr_bench::cli::tool_path_guard::real;
    use pr_core::io::physical::PhysicalFileSystem;
    for input in ["", " \t\r\n"] {
        let error = real(&PhysicalFileSystem, input).unwrap_err();
        assert_eq!(error.type_name, "ArgumentException");
        assert_eq!(
            error.message,
            "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'path')"
        );
    }
    let error = real(&PhysicalFileSystem, "x\0y").unwrap_err();
    assert_eq!(error.type_name, "ArgumentException");
    assert_eq!(error.message, "Null character in path. (Parameter 'path')");
}

#[derive(Default)]
struct ManualLinkTime {
    ticks: std::sync::atomic::AtomicI64,
    sleeps: std::sync::Mutex<Vec<Duration>>,
}

impl pr_bench::slow_link_fs::TimeSource for ManualLinkTime {
    fn now_ticks(&self) -> i64 {
        self.ticks.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn sleep(&self, duration: Duration) {
        let mut sleeps = self.sleeps.lock().unwrap();
        assert!(sleeps.len() < 10, "fake-clock limiter must make progress");
        sleeps.push(duration);
        self.ticks.fetch_add(
            i64::try_from(duration.as_nanos() / 100).unwrap(),
            std::sync::atomic::Ordering::SeqCst,
        );
    }
}

#[test]
fn slowlink_refill_caps_idle_credit_and_resets_backward_origin() {
    // Mutants slow_link_fs.rs:168-175: refill arithmetic, <= replacement, delete refill.
    // C#: SharedBandwidthLimiter.Refill caps tokens at one second and updates lastTicks before return.
    use pr_bench::slow_link_fs::{SharedBandwidthLimiter, TICKS_PER_SECOND};
    use std::sync::atomic::Ordering::SeqCst;
    // A fractional second must earn fractional credit, before the one-second cap.
    // Also kills :168 / -> * and :174 * -> +, which full-second probes saturate away.
    let time = Arc::new(ManualLinkTime::default());
    let limiter = SharedBandwidthLimiter::with_time_source(1000, time.clone()).unwrap();
    time.ticks.store(TICKS_PER_SECOND / 1000, SeqCst);
    limiter.consume(50);
    assert_eq!(*time.sleeps.lock().unwrap(), [Duration::from_millis(49)]);

    let time = Arc::new(ManualLinkTime::default());
    let limiter = SharedBandwidthLimiter::with_time_source(1000, time.clone()).unwrap();
    time.ticks.store(10 * TICKS_PER_SECOND, SeqCst);
    limiter.consume(500);
    assert!(time.sleeps.lock().unwrap().is_empty());
    limiter.consume(600);
    assert_eq!(*time.sleeps.lock().unwrap(), [Duration::from_millis(100)]);

    let time = Arc::new(ManualLinkTime::default());
    time.ticks.store(1000 * TICKS_PER_SECOND, SeqCst);
    let limiter = SharedBandwidthLimiter::with_time_source(1000, time.clone()).unwrap();
    time.ticks.store(500 * TICKS_PER_SECOND, SeqCst);
    limiter.consume(1);
    assert_eq!(*time.sleeps.lock().unwrap(), [Duration::from_millis(1)]);
    time.ticks.fetch_add(TICKS_PER_SECOND, SeqCst);
    limiter.consume(1000);
    assert_eq!(*time.sleeps.lock().unwrap(), [Duration::from_millis(1)]);
}
