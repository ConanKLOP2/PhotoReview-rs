//! W5-18 integration checks assigned by P-31 (C# baseline d92d6aae).
//! These exercise the real RAW decoder and preview service over synthetic files; corpus sweeps run only when a local
//! corpus is configured and never fetch samples.

#[path = "common/corpus_skip.rs"]
mod corpus_skip;

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pr_core::abstractions::{Log, PhysicalSourceReader, ReadSeek, SourceReadPriority, SourceReader};
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
use pr_imaging::cache::source_bytes::SourceBytesCache;
use pr_imaging::decode_types::{
    DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging::preload::scheduler::{
    CallbackError, EntriesSnapshotFn, PreloadScheduler, PreloadSchedulerDeps, ThresholdOptions,
};
use pr_imaging::preload::target::{DelegateMemoryProbe, PreloadTarget};
use pr_imaging::preload_order::PreloadWindow;
use pr_imaging::service::{PreviewImageService, PreviewServiceConfig};
use pr_imaging_raw::RawHeaderSource;
use pr_imaging_raw::decoder::{RawDecoder, RawDecoderOptions};
use pr_imaging_raw::header_source::SourceRawHeaderSource;
use pr_imaging_raw::registry::RawContainerReaderRegistry;
use pr_test_support::TempRoot;
use pr_test_support::raw::{build_tiff, create_minimal_jpeg};

const WAIT: Duration = Duration::from_secs(30);
const IMAGE_COUNT: usize = 200;
const PRELOAD_CACHE_CAPACITY: i64 = 512 * 1024 * 1024;

struct JpegImageDecoder;

#[derive(Default)]
struct CountingSourceReader {
    bytes_by_path: Arc<Mutex<HashMap<String, i64>>>,
}

impl CountingSourceReader {
    fn bytes_by_path(&self) -> HashMap<String, i64> {
        self.bytes_by_path
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SourceReader for CountingSourceReader {
    fn open_source(
        &self,
        path: &Path,
        priority: SourceReadPriority,
        buffer_size: usize,
    ) -> io::Result<Box<dyn ReadSeek>> {
        let inner = PhysicalSourceReader.open_source(path, priority, buffer_size)?;
        Ok(Box::new(CountingStream {
            inner,
            path_key: path.to_string_lossy().to_ascii_lowercase(),
            bytes_by_path: Arc::clone(&self.bytes_by_path),
        }))
    }
}

struct CountingStream {
    inner: Box<dyn ReadSeek>,
    path_key: String,
    bytes_by_path: Arc<Mutex<HashMap<String, i64>>>,
}

impl Read for CountingStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        if read > 0 {
            let mut counts = self.bytes_by_path.lock().unwrap_or_else(PoisonError::into_inner);
            *counts.entry(self.path_key.clone()).or_default() += read as i64;
        }
        Ok(read)
    }
}

impl Seek for CountingStream {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

impl ImageDecoder for JpegImageDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let width = if request.target_width > 0 {
            request.target_width.min(640)
        } else {
            640
        };
        let height = if request.target_height > 0 {
            request.target_height.min(480)
        } else {
            width * 3 / 4
        };
        let pixels = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).map_err(|error| DecodeError::Other {
            message: error.to_string(),
        })?;
        let bytes = i64::from(width) * i64::from(height) * 4;
        let image = DecodedImage::new(
            PlatformImage::Pixels(Arc::new(pixels)),
            width,
            height,
            bytes,
            DecodedImageOptions::default(),
        )
        .map_err(|error| DecodeError::Other {
            message: error.to_string(),
        })?;
        Ok(Arc::new(image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(640, 480))
    }
}

struct NoDiskCodec;

impl PreviewPayloadCodec for NoDiskCodec {
    fn encode_jpeg(&self, _pixels: &PixelBuffer, _quality: i32, _out: &mut dyn std::io::Write) -> std::io::Result<()> {
        Ok(())
    }

    fn decode_jpeg(
        &self,
        _payload: &[u8],
        _layout: PixelLayout,
        _expected_width: i32,
        _expected_height: i32,
    ) -> Result<PixelBuffer, PreviewCacheError> {
        Err(PreviewCacheError::InvalidData("disk cache is disabled".to_owned()))
    }
}

#[derive(Default)]
struct CapturedLog(Mutex<Vec<String>>);

impl Log for CapturedLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, message: &str) {
        self.0.lock().expect("log mutex").push(message.to_owned());
    }

    fn warn(&self, message: &str) {
        self.0.lock().expect("log mutex").push(message.to_owned());
    }

    fn error(&self, message: &str, _error: Option<&(dyn std::error::Error + 'static)>) {
        self.0.lock().expect("log mutex").push(message.to_owned());
    }
}

fn make_service(
    root: &Path,
    cache: Arc<SourceBytesCache>,
    metrics: Arc<ReviewMetrics>,
    width: i32,
    height: i32,
    source_reader: Option<Arc<dyn SourceReader>>,
) -> PreviewImageService {
    let decoder: Arc<dyn ImageDecoder> = Arc::new(RawDecoder::with_options(
        Arc::new(JpegImageDecoder),
        RawDecoderOptions {
            source_reader: source_reader.clone(),
            source_bytes_cache: Some(Arc::clone(&cache)),
            ..RawDecoderOptions::default()
        },
    ));
    let mut config = PreviewServiceConfig::new(
        metrics,
        Arc::new(|| false),
        Arc::new(move || pr_imaging::decode_types::DecodeBox::new(width, height)),
        PixelBufferImageCodec::shared(),
        Arc::new(NoDiskCodec),
    );
    config.decoder = Some(decoder);
    config.source_bytes_cache = Some(cache);
    config.source_reader = source_reader;
    config.disk_cache_capacity_bytes = 0;
    config.disable_disk_cache_override = Some(true);
    config.disk_cache_directory = Some(root.join("preview-cache"));
    PreviewImageService::new(config).expect("preview service builds")
}

#[test]
// C#: RawDecoderTests.PreviewService_DoesNotPreReadWholeRawFileIntoSourceCache
fn preview_service_does_not_pre_read_whole_raw_file_into_source_cache() {
    let root = TempRoot::new("w5-18-raw-cache");
    let jpeg = create_minimal_jpeg(640, 480);
    let tiff = build_tiff(true, Some(&jpeg), 1, 42);
    let mut raw = vec![0; 10 * 1024 * 1024];
    raw[..tiff.len()].copy_from_slice(&tiff);
    let path = root.join("camera.dng");
    fs::write(&path, raw).expect("write synthetic RAW");
    let cache = Arc::new(SourceBytesCache::new(16 * 1024 * 1024, 0, None).expect("source cache"));
    let metrics = Arc::new(ReviewMetrics::new());
    let service = make_service(root.path(), cache.clone(), Arc::clone(&metrics), 320, 240, None);

    let result = service
        .get_preview_path(path.to_string_lossy().as_ref())
        .wait_timeout(WAIT)
        .expect("preview decode completes within the hang guard")
        .expect("synthetic RAW preview decodes");

    assert!(result.pixel_width() > 0);
    assert_eq!(cache.count(), 1);
    assert_eq!(cache.current_size(), jpeg.len() as i64);
    assert!(cache.current_size() < 10 * 1024 * 1024);
    assert!(metrics.snapshot().source_bytes_read < 10 * 1024 * 1024);
    assert!(service.shutdown_persist_workers_timeout(WAIT));
}

#[test]
// C#: RawPreloadBudgetSlowTests.TwoHundredRawFiles_EstimateMatchesMeasuredCache_AndReadsOnlyHeaderAndPreview
fn two_hundred_raw_files_estimate_matches_measured_cache_and_reads_only_header_and_preview() {
    let root = TempRoot::new("w5-18-raw-preload");
    let jpeg = create_minimal_jpeg(640, 480);
    let tiff = build_tiff(true, Some(&jpeg), 1, 42);
    let mut contents = vec![0; 512 * 1024];
    contents[..tiff.len()].copy_from_slice(&tiff);
    let paths = (0..IMAGE_COUNT)
        .map(|index| {
            let path = root.join(format!("image-{index:03}.dng"));
            fs::write(&path, &contents).expect("write synthetic RAW");
            path.to_string_lossy().into_owned()
        })
        .collect::<Vec<_>>();
    let entries: Arc<[CatalogEntry]> = paths
        .iter()
        .map(|path| CatalogEntry::new(path.clone()).expect("catalog entry"))
        .collect::<Vec<_>>()
        .into();
    let snapshot_entries: EntriesSnapshotFn = {
        let entries = Arc::clone(&entries);
        Arc::new(move || -> Result<Arc<[CatalogEntry]>, CallbackError> { Ok(Arc::clone(&entries)) })
    };
    let source_reader = Arc::new(CountingSourceReader::default());
    let source_reader_trait: Arc<dyn SourceReader> = source_reader.clone();
    let source_cache = Arc::new(
        SourceBytesCache::new(16 * 1024 * 1024, 0, Some(Arc::clone(&source_reader_trait))).expect("source cache"),
    );
    let metrics = Arc::new(ReviewMetrics::new());
    let service = Arc::new(make_service(
        root.path(),
        Arc::clone(&source_cache),
        Arc::clone(&metrics),
        640,
        480,
        Some(source_reader_trait),
    ));
    let log = Arc::new(CapturedLog::default());

    let first = service
        .get_preview_path(&paths[0])
        .wait_timeout(WAIT)
        .expect("first preview completes within the hang guard");
    assert!(first.is_ok(), "first synthetic preview decodes");
    let target: Arc<dyn PreloadTarget> = service.clone();
    let scheduler = PreloadScheduler::with_thresholds(
        target,
        Arc::clone(&metrics),
        snapshot_entries,
        Arc::new(DelegateMemoryProbe::new(|_| true)),
        ThresholdOptions {
            full_folder_ram_threshold_bytes: PRELOAD_CACHE_CAPACITY,
            memory_load_limit: 1.0,
            worker_count_override: Some(8),
            window: Some(PreloadWindow::new(IMAGE_COUNT as i32 - 1, 0)),
        },
        PreloadSchedulerDeps {
            log: Some(log.clone()),
            env_lookup: Some(Arc::new(|_| None)),
            processor_count: Some(8),
            ..PreloadSchedulerDeps::default()
        },
    );
    assert!(
        scheduler.preload_around(0).wait_timeout(WAIT),
        "preload run completes within the hang guard"
    );

    let measured_bytes = service.cache_bytes();
    let expected_measured = IMAGE_COUNT as i64 * 640 * 480 * 4;
    assert_eq!(service.cache_count(), IMAGE_COUNT);
    assert_eq!(measured_bytes, expected_measured);
    let estimate_line = log
        .0
        .lock()
        .expect("log mutex")
        .iter()
        .rev()
        .find(|line| line.contains("estimatedBytes="))
        .cloned()
        .expect("preload policy estimate was logged");
    let estimate = estimate_line
        .split("estimatedBytes=")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|value| value.parse::<i64>().ok())
        .expect("estimatedBytes is invariant decimal");
    assert!(
        (measured_bytes * 4 / 5..=measured_bytes * 6 / 5).contains(&estimate),
        "estimated {estimate} differs from measured {measured_bytes}; line={estimate_line}"
    );
    // C# `SourceRawHeaderSource.BlockSize` is 64 KiB; the 2-block cap is the reader contract.
    let max_expected_read = (2 * 64 * 1024_i64).min(contents.len() as i64) + jpeg.len() as i64;
    assert!(max_expected_read < contents.len() as i64);
    assert_eq!(source_cache.current_size(), IMAGE_COUNT as i64 * jpeg.len() as i64);
    assert!(metrics.snapshot().source_bytes_read <= IMAGE_COUNT as i64 * max_expected_read);
    let per_path_reads = source_reader.bytes_by_path();
    assert_eq!(per_path_reads.len(), IMAGE_COUNT);
    assert_eq!(per_path_reads.values().copied().max(), Some(max_expected_read));
    assert!(
        per_path_reads
            .values()
            .all(|read| (1..=max_expected_read).contains(read))
    );
    assert!(service.shutdown_persist_workers_timeout(WAIT));
}

fn corpus_dir() -> PathBuf {
    std::env::var_os("PHOTOREVIEW_RAW_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| pr_test_support::golden::repo_root().join("tests/fixtures/raw-corpus"))
}

fn corpus_strict() -> bool {
    std::env::var("PHOTOREVIEW_RAW_CORPUS_STRICT")
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"))
        .unwrap_or(false)
}

#[test]
// C#: TiffCorpusTests.AllCorpusTiffSamples_CanBeParsed
fn all_corpus_tiff_samples_can_be_parsed_through_registry() {
    let dir = corpus_dir();
    if !dir.is_dir() {
        if corpus_strict() {
            panic!("RAW corpus is required but missing: {}", dir.display());
        }
        corpus_skip::skipped(&format!("RAW corpus not present at {}", dir.display()));
        return;
    }

    let mut files = fs::read_dir(&dir)
        .expect("read RAW corpus directory")
        .map(|entry| entry.expect("read directory entry").path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "cr2" | "nef" | "arw" | "dng" | "orf" | "rw2"
                    )
                })
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
    });

    let registry = RawContainerReaderRegistry::new();
    let mut parsed = 0;
    for path in files {
        let mut file = fs::File::open(&path).expect("open corpus RAW");
        let mut header = [0; 64];
        let count = file.read(&mut header).expect("read corpus header");
        file.rewind().expect("rewind corpus RAW");
        let extension = format!(
            ".{}",
            path.extension().and_then(|value| value.to_str()).unwrap_or_default()
        );
        let reader = registry
            .find_reader(&header[..count], &extension)
            .unwrap_or_else(|| panic!("{}: no registry reader for {extension}", path.display()));
        let mut source = SourceRawHeaderSource::from_stream(Box::new(file)).expect("open header source");
        let info = reader
            .read(&mut source, &pr_core::sync::CancelToken::none())
            .unwrap_or_else(|error| panic!("{}: registry reader failed: {error}", path.display()));
        if info.format == pr_imaging_raw::RawFormat::Dng && info.previews.is_empty() {
            assert!(
                info.sensor_width > 0 || !info.previews.is_empty(),
                "{} ({:?}) had no previews and no sensor dimensions",
                path.display(),
                info.format
            );
        } else {
            assert!(
                !info.previews.is_empty(),
                "{} ({:?}) had no previews",
                path.display(),
                info.format
            );
            assert!(
                info.previews[0].length > 0,
                "{} first preview has zero length",
                path.display()
            );
        }
        parsed += 1;
    }

    if parsed == 0 {
        if corpus_strict() {
            panic!("strict RAW corpus contains no TIFF-family files in {}", dir.display());
        }
        corpus_skip::skipped(&format!("no TIFF-family samples in {}", dir.display()));
        return;
    }
    if corpus_strict() {
        assert!(parsed >= 18, "expected at least 18 TIFF corpus files, parsed {parsed}");
    }
}

#[test]
// C#: TiffCorpusTests.AllTiffCorpusFiles_EveryListedPreviewLiesInsideFileAndStartsWithSoi
fn all_tiff_corpus_files_every_listed_preview_lies_inside_file_and_starts_with_soi() {
    let dir = corpus_dir();
    if !dir.is_dir() {
        if corpus_strict() {
            panic!("RAW corpus is required but missing: {}", dir.display());
        }
        corpus_skip::skipped(&format!("RAW corpus not present at {}", dir.display()));
        return;
    }

    let mut files = fs::read_dir(&dir)
        .expect("read RAW corpus directory")
        .map(|entry| entry.expect("read directory entry").path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "cr2" | "nef" | "arw" | "dng" | "orf" | "rw2"
                    )
                })
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
    });

    let registry = RawContainerReaderRegistry::new();
    for path in files {
        let metadata = fs::metadata(&path).expect("stat corpus RAW");
        let length = i64::try_from(metadata.len()).expect("corpus file length fits i64");
        let mut file = fs::File::open(&path).expect("open corpus RAW");
        let mut header = [0; 64];
        let count = file.read(&mut header).expect("read corpus header");
        file.rewind().expect("rewind corpus RAW");
        let extension = format!(
            ".{}",
            path.extension().and_then(|value| value.to_str()).unwrap_or_default()
        );
        let reader = registry
            .find_reader(&header[..count], &extension)
            .unwrap_or_else(|| panic!("{}: no registry reader for {extension}", path.display()));
        let mut source = SourceRawHeaderSource::from_stream(Box::new(file)).expect("open header source");
        let info = reader
            .read(&mut source, &pr_core::sync::CancelToken::none())
            .unwrap_or_else(|error| panic!("{}: registry reader failed: {error}", path.display()));
        for preview in &info.previews {
            let end = preview
                .offset
                .checked_add(preview.length)
                .expect("preview range does not overflow");
            assert!(
                preview.offset >= 0 && preview.length > 0 && end <= length,
                "{} preview {}+{} outside file length {length}",
                path.display(),
                preview.offset,
                preview.length
            );
            let count = preview.length.min(2) as i32;
            let head = source
                .read(preview.offset, count)
                .unwrap_or_else(|error| panic!("{} preview at {}: {error}", path.display(), preview.offset));
            assert_eq!(
                head,
                [0xFF, 0xD8],
                "{} preview at {} does not start with SOI",
                path.display(),
                preview.offset
            );
        }
    }
}
