//! CX-W4REST deterministic mutation regressions; fakes and TempRoot only.
use pr_test_support::TempRoot;

struct BufferCheckingFileSystem;

impl pr_core::abstractions::FileSystem for BufferCheckingFileSystem {
    fn open_read_shared(
        &self,
        _: &std::path::Path,
        buffer_size: usize,
    ) -> std::io::Result<Box<dyn pr_core::abstractions::ReadSeek>> {
        assert_eq!(buffer_size, 65_536);
        Err(std::io::Error::other(
            "intentional stop after observing the read buffer",
        ))
    }
    fn file_exists(&self, _: &std::path::Path) -> bool {
        panic!("unexpected filesystem operation")
    }
    fn directory_exists(&self, _: &std::path::Path) -> bool {
        panic!("unexpected filesystem operation")
    }
    fn get_file_stat(&self, _: &std::path::Path) -> Option<pr_core::abstractions::FileStat> {
        panic!("unexpected filesystem operation")
    }
    fn move_file(&self, _: &std::path::Path, _: &std::path::Path) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
    fn copy_file(&self, _: &std::path::Path, _: &std::path::Path) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
    fn delete(&self, _: &std::path::Path) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
    fn open_append(
        &self,
        _: &std::path::Path,
        _: bool,
    ) -> std::io::Result<Box<dyn pr_core::abstractions::AppendStream>> {
        panic!("unexpected filesystem operation")
    }
    fn write_all_text_atomic(&self, _: &std::path::Path, _: &str, _: bool) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
    fn read_all_text(&self, _: &std::path::Path) -> std::io::Result<String> {
        panic!("unexpected filesystem operation")
    }
    fn enumerate_files(&self, _: &std::path::Path, _: &str) -> std::io::Result<Vec<std::path::PathBuf>> {
        panic!("unexpected filesystem operation")
    }
    fn enumerate_directories(&self, _: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
        panic!("unexpected filesystem operation")
    }
    fn create_directory(&self, _: &std::path::Path) -> std::io::Result<()> {
        panic!("unexpected filesystem operation")
    }
}

struct NeverOpenedBackend;
impl pr_imaging::embedded_thumbnail::EmbeddedThumbnailBackend for NeverOpenedBackend {
    fn open(
        &self,
        _: Box<dyn pr_core::abstractions::ReadSeek>,
    ) -> Result<
        Option<Box<dyn pr_imaging::embedded_thumbnail::EmbeddedThumbnailFrame>>,
        pr_imaging::embedded_thumbnail::ThumbnailBackendError,
    > {
        panic!("the intentional open error must stop before the backend")
    }
}

#[test]
fn embedded_thumbnail_shared_read_requests_the_csharp_64_kib_buffer() {
    // Mutants embedded_thumbnail.rs:26:38, replace * with + or /.
    // C#: EmbeddedThumbnailReader.TryRead opens with a 64 * 1024 buffer.
    let root = TempRoot::new("w4rest-buffer");
    let image = pr_imaging::embedded_thumbnail::try_read(
        &root.path().join("fake.jpg"),
        &BufferCheckingFileSystem,
        &NeverOpenedBackend,
        pr_imaging::pixel_memory::PixelBufferImageCodec::shared().as_ref(),
    )
    .unwrap();
    assert!(image.is_none());
}
use pr_imaging::cache::thumbnails::{
    CacheImageInfo, CacheImageReader, ThumbnailCache, ThumbnailCacheOptions, ThumbnailCacheServices, ThumbnailRequest,
};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use std::sync::Arc;

struct UnusedDiskReader;
impl CacheImageReader for UnusedDiskReader {
    fn decode(
        &self,
        _: &[u8],
        _: Option<PixelLayout>,
        _: DecodeBox,
        _: i32,
        _: i32,
    ) -> Result<(PixelBuffer, CacheImageInfo), DecodeError> {
        panic!("fake disk entry does not exist")
    }
}

#[derive(Default)]
struct ThumbnailPerfLog(std::sync::Mutex<Vec<pr_core::diagnostics::perf::PerfEvent>>);
impl pr_core::diagnostics::perf::PerfListener for ThumbnailPerfLog {
    fn on_event_written(&self, event: &pr_core::diagnostics::perf::PerfEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

#[test]
fn thumbnail_debug_and_directory_preserve_the_cache_and_pending_identity() {
    // Mutants cache/thumbnails.rs:214/255/284/384/659 (Debug -> Ok), 489 (directory -> empty).
    // C#: ThumbnailCache.DiskDirectory; Debug is a Rust diagnostic contract.
    let root = TempRoot::new("w4rest-thumbnail-debug");
    // Mutant cache/thumbnails.rs:902:5 deletes thumb_end; assert event fields, never elapsed time.
    let perf_log = Arc::new(ThumbnailPerfLog::default());
    let _subscription = pr_core::diagnostics::perf::PhotoReviewPerf::log().subscribe(perf_log.clone());
    let path = root.path().join("fake.jpg");
    let fs = Arc::new(pr_test_support::FakeFileSystem::new());
    fs.add_file(&path, b"fake");
    let services = ThumbnailCacheServices {
        platform_codec: pr_imaging::pixel_memory::PixelBufferImageCodec::shared(),
        cache_image_reader: Arc::new(UnusedDiskReader),
        embedded_backend: Arc::new(NeverOpenedBackend),
        fs,
    };
    assert_eq!(format!("{services:?}"), "ThumbnailCacheServices { .. }");
    let directory = root.path().join("cache");
    let mut options = ThumbnailCacheOptions::new(directory.clone());
    options.clock = Arc::new(pr_test_support::FakeClock::new());
    options.spawner = Some(Arc::new(|job| {
        job();
        Ok(())
    }));
    options.embedded_reader = Some(Arc::new(|_, _| Ok(None)));
    assert!(format!("{options:?}").starts_with("ThumbnailCacheOptions { disk_directory:"));
    let cache = ThumbnailCache::new(services, options).unwrap();
    assert_eq!(cache.disk_directory(), directory);
    assert_eq!(format!("{cache:?}"), "ThumbnailCache { .. }");
    let request = cache.get(path.to_str().unwrap(), None).unwrap();
    assert!(format!("{request:?}").starts_with("Pending("));
    let ThumbnailRequest::Pending(pending) = request else {
        panic!("initial load is pending");
    };
    assert!(format!("{pending:?}").starts_with("PendingThumbnail { key:"));
    assert!(
        pending
            .try_result()
            .expect("inline loader must publish immediately")
            .unwrap()
            .is_none()
    );
    assert_eq!(format!("{:?}", ThumbnailRequest::Ready(Ok(None))), "Ready(Ok(None))");
    assert!(cache.wait_startup_cleanup(std::time::Duration::ZERO));
    let path_id =
        pr_core::diagnostics::perf::PerfValue::Str(pr_core::diagnostics::perf::path_id(path.to_str().unwrap()));
    assert!(perf_log.0.lock().unwrap().iter().any(|event| event.name == "ThumbEnd"
        && event.payload[1] == path_id
        && event.payload[2] == pr_core::diagnostics::perf::PerfValue::Str("none".into())));
}

mod support {
    pub mod preview_service;
}

#[derive(Default)]
struct ErrorChainLog(std::sync::Mutex<Vec<(String, Option<String>)>>);
impl pr_core::abstractions::Log for ErrorChainLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, _: &str) {}
    fn warn(&self, _: &str) {}
    fn error(&self, _: &str, error: Option<&(dyn std::error::Error + 'static)>) {
        if let Some(error) = error {
            self.0
                .lock()
                .unwrap()
                .push((error.to_string(), error.source().map(ToString::to_string)));
        }
    }
}
struct PersistFailureCodec {
    panic: bool,
}
impl pr_imaging::decoder_traits::PlatformImageCodec for PersistFailureCodec {
    fn name(&self) -> &'static str {
        "w4rest-failure"
    }
    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<pr_imaging::decode_types::PlatformImage, DecodeError> {
        Ok(pr_imaging::decode_types::PlatformImage::Pixels(Arc::new(pixels)))
    }
    fn to_pixels<'a>(
        &self,
        _: &'a pr_imaging::decode_types::PlatformImage,
    ) -> Result<pr_imaging::decode_types::PixelLease<'a>, DecodeError> {
        assert!(!self.panic, "w4rest persist panic");
        Err(DecodeError::Disposed {
            message: "w4rest disposed pixels".into(),
        })
    }
}

#[test]
fn persist_failure_log_keeps_error_chain_and_panic_payload() {
    // Mutants service/disk.rs:583 (Display -> Ok), 594 (source -> None),
    // 609 (panic payload -> default), 620 (panic Display -> Ok).
    // C#: PreviewImageService persist worker logs the original exception diagnostics.
    use pr_imaging::service::PreviewImageService;
    use support::preview_service::{FakeDecoder, Script, TIMEOUT, config};
    for panic in [false, true] {
        let root = TempRoot::new("w4rest-persist-error-chain");
        let path = root.path().join("fake.jpg");
        let fs = Arc::new(pr_test_support::FakeFileSystem::new());
        fs.add_file(&path, b"fake source");
        let decoder = FakeDecoder::new();
        decoder.script_default(Script {
            downscaled: true,
            ..Script::default()
        });
        let mut cfg = config(&root.path().join("cache"), decoder);
        cfg.file_system = Some(fs);
        cfg.clock = Some(Arc::new(pr_test_support::FakeClock::new()));
        cfg.disable_disk_cache_override = Some(false);
        cfg.platform_codec = Arc::new(PersistFailureCodec { panic });
        let log = Arc::new(ErrorChainLog::default());
        cfg.log = Some(log.clone());
        let service = PreviewImageService::new(cfg).unwrap();
        assert!(
            service
                .get_preview_path(path.to_str().unwrap())
                .wait_timeout(TIMEOUT)
                .unwrap()
                .is_ok()
        );
        assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
        assert!(service.wait_startup_cleanup(TIMEOUT));
        let expected = if panic {
            "w4rest persist panic"
        } else {
            "w4rest disposed pixels"
        };
        let entries = log.0.lock().unwrap();
        assert!(
            entries
                .iter()
                .any(|(text, source)| text == expected && (panic || source.as_deref() == Some(expected))),
            "{entries:?}"
        );
    }
}

#[test]
fn thumbnail_startup_signal_reports_pending_then_completed_without_waiting() {
    // Mutants cache/thumbnails.rs:641 (wait_startup_cleanup -> true / false),
    // 810/820 (delete Signal::set / SetOnDrop::drop). No clock or scheduling dependency.
    // C#: StartupCleanup completes only after its scheduled cleanup job is completed.
    use std::sync::Mutex;
    let root = TempRoot::new("w4rest-cleanup-signal");
    // Mutant cache/thumbnails.rs:653:9 deletes ThumbnailCache::drop -> dispose.
    let fs = Arc::new(pr_test_support::FakeFileSystem::new());
    let path = root.path().join("fake.jpg");
    fs.add_file(&path, b"fake source");
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let captured = jobs.clone();
    let mut options = ThumbnailCacheOptions::new(root.path().join("cache"));
    options.clock = Arc::new(pr_test_support::FakeClock::new());
    options.spawner = Some(Arc::new(move |job| {
        captured.lock().unwrap().push(job);
        Ok(())
    }));
    options.embedded_reader = Some(Arc::new(|_, _| Ok(None)));
    let cache = ThumbnailCache::new(
        ThumbnailCacheServices {
            platform_codec: pr_imaging::pixel_memory::PixelBufferImageCodec::shared(),
            cache_image_reader: Arc::new(UnusedDiskReader),
            embedded_backend: Arc::new(NeverOpenedBackend),
            fs,
        },
        options,
    )
    .unwrap();
    assert!(!cache.wait_startup_cleanup(std::time::Duration::ZERO));
    let job = jobs.lock().unwrap().pop().expect("startup cleanup was scheduled");
    job();
    assert!(cache.wait_startup_cleanup(std::time::Duration::ZERO));
    let ThumbnailRequest::Pending(pending) = cache.get(path.to_str().unwrap(), None).unwrap() else {
        panic!("load was queued");
    };
    let job = jobs.lock().unwrap().pop().expect("load was scheduled");
    drop(cache);
    job();
    assert!(matches!(
        pending.try_result(),
        Some(Err(pr_imaging::cache::thumbnails::ThumbnailError::Canceled))
    ));
}

struct ShortReadStream {
    cursor: std::io::Cursor<Vec<u8>>,
}
impl std::io::Read for ShortReadStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        assert!(
            !buffer.is_empty(),
            "pre-read must stop when the declared length is filled"
        );
        let count = buffer.len().min(2);
        std::io::Read::read(&mut self.cursor, &mut buffer[..count])
    }
}
impl std::io::Seek for ShortReadStream {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.cursor, position)
    }
}
struct DiagnosticSourceReader;
impl pr_core::abstractions::SourceReader for DiagnosticSourceReader {
    fn open_source(
        &self,
        _: &std::path::Path,
        priority: pr_core::abstractions::SourceReadPriority,
        buffer_size: usize,
    ) -> std::io::Result<Box<dyn pr_core::abstractions::ReadSeek>> {
        assert_eq!(priority, pr_core::abstractions::SourceReadPriority::Preload);
        assert_eq!(buffer_size, 1_048_576);
        Ok(Box::new(ShortReadStream {
            cursor: std::io::Cursor::new(vec![1, 2, 3, 4, 5, 6]),
        }))
    }
}

#[test]
fn diagnostic_pre_read_collects_short_reads_and_stops_exactly_at_declared_length() {
    // Mutants service/decode.rs:37 (* -> + or /), 564 (< -> <=), 567 (+= -> -= / *=).
    // C#: PreviewImageService diagnostic pre-read reads until the length is filled, then stops.
    use pr_core::diagnostics::diag_options::{DiagOptions, PRE_READ_VAR};
    use support::preview_service::{FakeDecoder, TIMEOUT, config};
    let root = TempRoot::new("w4rest-diag-short-read");
    let path = root.path().join("fake.jpg");
    let fs = Arc::new(pr_test_support::FakeFileSystem::new());
    fs.add_file(&path, &[1, 2, 3, 4, 5, 6]);
    let decoder = FakeDecoder::new();
    let mut cfg = config(&root.path().join("cache"), decoder.clone());
    cfg.file_system = Some(fs);
    cfg.clock = Some(Arc::new(pr_test_support::FakeClock::new()));
    cfg.source_reader = Some(Arc::new(DiagnosticSourceReader));
    cfg.diag = Some(Arc::new(DiagOptions::from_lookup(|name| {
        (name == PRE_READ_VAR).then(|| "1".into())
    })));
    let service = pr_imaging::service::PreviewImageService::new(cfg).unwrap();
    assert!(
        service
            .get_preview_path(path.to_str().unwrap())
            .wait_timeout(TIMEOUT)
            .unwrap()
            .is_ok()
    );
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
    let requests = decoder.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].bytes.as_deref(), Some([1, 2, 3, 4, 5, 6].as_slice()));
}

struct BoundaryStream(u64);
impl std::io::Read for BoundaryStream {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("w4rest boundary reached read"))
    }
}
impl std::io::Seek for BoundaryStream {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        match position {
            std::io::SeekFrom::End(0) => Ok(self.0),
            std::io::SeekFrom::Start(0) => Ok(0),
            _ => panic!("unexpected boundary stream seek"),
        }
    }
}
struct BoundaryReader(u64);
impl pr_core::abstractions::SourceReader for BoundaryReader {
    fn open_source(
        &self,
        _: &std::path::Path,
        _: pr_core::abstractions::SourceReadPriority,
        _: usize,
    ) -> std::io::Result<Box<dyn pr_core::abstractions::ReadSeek>> {
        Ok(Box::new(BoundaryStream(self.0)))
    }
}

#[test]
#[ignore = "slow: allocates the 2 GiB diagnostic pre-read boundary; run explicitly"]
fn diagnostic_pre_read_accepts_exact_array_max_length_and_refuses_the_next_byte() {
    // Mutants service/decode.rs:551:23, replace > with >= / == / <.
    // C#: diagnostic pre-read accepts Array.MaxLength, then surfaces a stream read error.
    use pr_core::diagnostics::diag_options::{DiagOptions, PRE_READ_VAR};
    use support::preview_service::{FakeDecoder, config};
    let limit = pr_imaging::cache::source_bytes::ARRAY_MAX_LENGTH as u64;
    for length in [limit, limit + 1] {
        let root = TempRoot::new("w4rest-diag-boundary");
        let path = root.path().join("fake.jpg");
        let fs = Arc::new(pr_test_support::FakeFileSystem::new());
        fs.add_file(&path, b"fake source");
        let decoder = FakeDecoder::new();
        let mut cfg = config(&root.path().join("cache"), decoder.clone());
        cfg.file_system = Some(fs);
        cfg.clock = Some(Arc::new(pr_test_support::FakeClock::new()));
        cfg.source_reader = Some(Arc::new(BoundaryReader(length)));
        cfg.diag = Some(Arc::new(DiagOptions::from_lookup(|name| {
            (name == PRE_READ_VAR).then(|| "1".into())
        })));
        let service = pr_imaging::service::PreviewImageService::new(cfg).unwrap();
        let error = service
            .get_preview_path(path.to_str().unwrap())
            .wait_timeout(std::time::Duration::from_secs(60))
            .unwrap()
            .err()
            .expect("scripted read or size refusal");
        if let pr_imaging::service::PreviewError::Io(inner) = &error {
            assert_eq!(
                inner.kind(),
                if length == limit {
                    std::io::ErrorKind::Other
                } else {
                    std::io::ErrorKind::InvalidData
                }
            );
        } else {
            panic!("boundary failure must retain the I/O type: {error:?}");
        }
        let expected = if length == limit {
            "w4rest boundary reached read".to_owned()
        } else {
            format!("The source is too large to pre-read into memory: {length} bytes.")
        };
        assert_eq!(error.to_string(), expected);
        assert_eq!(decoder.calls(), 0);
        assert!(service.shutdown_persist_workers_timeout(std::time::Duration::from_secs(60)));
    }
}

struct InlineDecodeSpawner;
impl pr_imaging::service::DecodeThreadSpawner for InlineDecodeSpawner {
    fn spawn(
        &self,
        kind: pr_imaging::service::DecodeThreadKind,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> std::io::Result<()> {
        use pr_imaging::service::DecodeThreadKind;
        if matches!(
            kind,
            DecodeThreadKind::Preview | DecodeThreadKind::Viewer | DecodeThreadKind::Original
        ) {
            job();
            Ok(())
        } else {
            pr_imaging::service::StdThreadSpawner.spawn(kind, job)
        }
    }
}

#[test]
fn failed_inline_preview_is_removed_before_the_next_request_starts_a_fresh_decode() {
    // Mutant service/decode.rs:321:9 deletes remove_in_flight; old joiner retry tests hang.
    // C#: GetPreview_CreatorBusy_RemovesInflightEntry; inspect synchronous call count before awaiting a joiner.
    use support::preview_service::{Failure, FakeDecoder, Script, TIMEOUT, config};
    let root = TempRoot::new("w4rest-inflight-removal");
    let path = root.path().join("fake.jpg");
    let fs = Arc::new(pr_test_support::FakeFileSystem::new());
    fs.add_file(&path, b"fake source");
    let decoder = FakeDecoder::new();
    decoder.script_default(Script {
        failure: Some(Failure::Busy),
        ..Script::default()
    });
    let mut cfg = config(&root.path().join("cache"), decoder.clone());
    cfg.file_system = Some(fs);
    cfg.clock = Some(Arc::new(pr_test_support::FakeClock::new()));
    cfg.spawner = Some(Arc::new(InlineDecodeSpawner));
    let service = pr_imaging::service::PreviewImageService::new(cfg).unwrap();
    let first = service.get_preview_path(path.to_str().unwrap());
    assert!(
        first
            .wait_timeout(std::time::Duration::ZERO)
            .unwrap()
            .err()
            .expect("scripted busy result")
            .is_busy()
    );
    assert_eq!(decoder.calls(), 1);
    decoder.script_default(Script::default());
    let second = service.get_preview_path(path.to_str().unwrap());
    assert_eq!(
        decoder.calls(),
        2,
        "the completed failed cell must be forgotten before a new get"
    );
    assert!(second.wait_timeout(std::time::Duration::ZERO).unwrap().is_ok());
    assert!(service.shutdown_persist_workers_timeout(TIMEOUT));
}

type BackgroundJob = (
    pr_imaging::service::DecodeThreadKind,
    Box<dyn FnOnce() + Send + 'static>,
);
#[derive(Default)]
struct QueuedBackgroundSpawner(std::sync::Mutex<Vec<BackgroundJob>>);
impl pr_imaging::service::DecodeThreadSpawner for QueuedBackgroundSpawner {
    fn spawn(
        &self,
        kind: pr_imaging::service::DecodeThreadKind,
        job: Box<dyn FnOnce() + Send + 'static>,
    ) -> std::io::Result<()> {
        self.0.lock().unwrap().push((kind, job));
        Ok(())
    }
}

#[test]
fn unbounded_persist_shutdown_completes_waiting_workers_and_is_idempotent() {
    // Mutants service/disk.rs:270:9 (shutdown body -> ()), 275:59 (> -> == / >=).
    // C#: ShutdownPersistWorkersAsync completes the channel and joins all workers, including a repeated call.
    // recv_timeout is only a hang guard for completion, never an elapsed-time or negative-delay assertion.
    use support::preview_service::{FakeDecoder, config};
    let shutdown_guard = std::time::Duration::from_secs(5);
    let root = TempRoot::new("w4rest-unbounded-shutdown");
    let spawner = Arc::new(QueuedBackgroundSpawner::default());
    let mut cfg = config(&root.path().join("cache"), FakeDecoder::new());
    cfg.spawner = Some(spawner.clone());
    cfg.clock = Some(Arc::new(pr_test_support::FakeClock::new()));
    let service = Arc::new(pr_imaging::service::PreviewImageService::new(cfg).unwrap());
    let jobs = std::mem::take(&mut *spawner.0.lock().unwrap());
    assert_eq!(jobs.len(), 2);
    let (tx, rx) = std::sync::mpsc::channel();
    let shutdown_service = service.clone();
    let shutdown_tx = tx.clone();
    let shutdown = std::thread::spawn(move || {
        shutdown_service.shutdown_persist_workers();
        shutdown_tx.send(()).unwrap();
    });
    let workers: Vec<_> = jobs
        .into_iter()
        .map(|(kind, job)| {
            assert_eq!(kind, pr_imaging::service::DecodeThreadKind::PersistWorker);
            let worker_tx = tx.clone();
            std::thread::spawn(move || {
                job();
                worker_tx.send(()).unwrap();
            })
        })
        .collect();
    drop(tx);
    for _ in 0..3 {
        rx.recv_timeout(shutdown_guard)
            .expect("shutdown must complete every worker and return");
    }
    shutdown.join().unwrap();
    for worker in workers {
        worker.join().unwrap();
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let repeated = std::thread::spawn(move || {
        service.shutdown_persist_workers();
        tx.send(()).unwrap();
    });
    rx.recv_timeout(shutdown_guard)
        .expect("a second shutdown with zero live workers must return");
    repeated.join().unwrap();
}

#[test]
fn guarded_shutdown_reports_queued_workers_then_finished_workers_without_waiting() {
    // Mutants service/disk.rs:281 (return true/false), 288 (== -> !=).
    // C#: the bounded shutdown reports false until queued workers have actually exited.
    use support::preview_service::{FakeDecoder, config};
    let root = TempRoot::new("w4rest-guarded-shutdown");
    let spawner = Arc::new(QueuedBackgroundSpawner::default());
    let mut cfg = config(&root.path().join("cache"), FakeDecoder::new());
    cfg.spawner = Some(spawner.clone());
    cfg.clock = Some(Arc::new(pr_test_support::FakeClock::new()));
    let service = pr_imaging::service::PreviewImageService::new(cfg).unwrap();
    assert!(!service.shutdown_persist_workers_timeout(std::time::Duration::ZERO));
    let jobs = std::mem::take(&mut *spawner.0.lock().unwrap());
    assert_eq!(jobs.len(), 2);
    for (kind, job) in jobs {
        assert_eq!(kind, pr_imaging::service::DecodeThreadKind::PersistWorker);
        job();
    }
    assert!(service.shutdown_persist_workers_timeout(std::time::Duration::ZERO));
}
