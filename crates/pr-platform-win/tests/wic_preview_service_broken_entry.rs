//! Native twin (real WIC JPEG payload codec) of C# `PreviewImageServiceFaultTests.BrokenEntry_IsAMiss` row `jpeg-header-mangled-eoi`.
//!
//! The pr-imaging twin cannot run this row: its `FakePayloadCodec` payload is 13 bytes, so zeroing from offset 40 changes nothing. Here the
//! `.pv4` entry is written by the real `WicPreviewPayloadCodec` (a real JPEG payload), its header is zeroed from file offset 40 up to the
//! two-byte EOI (which stays), and a fresh `PreviewImageService` must treat the entry as a miss: decode from source once, no disk hit, and
//! replace the broken file.
//! Unit native-test-gaps (D). All `#[ignore = "native: needs WIC"]`.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::{
    DecodeBox, DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging::service::{PreviewImage, PreviewImageService, PreviewServiceConfig};
use pr_platform_win::wic::cache_adapters::WicPreviewPayloadCodec;
use pr_test_support::TempRoot;

const TIMEOUT: Duration = Duration::from_secs(30);

/// A 120 x 80 opaque gradient, always reported as downscaled (so the service persists it), like the pr-imaging `ShapeDecoder` but with
/// pixels a JPEG encoder cannot collapse to a few bytes.
struct GradientDecoder;

impl ImageDecoder for GradientDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let (width, height) = (120, 80);
        let invalid = |e: pr_imaging::pixels::PixelBufferError| DecodeError::InvalidArgument { message: e.to_string() };
        let mut pixels = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).map_err(invalid)?;
        for y in 0..height {
            let row = pixels.get_row_mut(y).map_err(invalid)?;
            for x in 0..width {
                let i = usize::try_from(x * 4).unwrap_or(0);
                row[i] = (x * 2) as u8;
                row[i + 1] = (y * 3) as u8;
                row[i + 2] = ((x + y) * 5) as u8;
                row[i + 3] = 255;
            }
        }
        let options = DecodedImageOptions {
            downscaled: true,
            orientation: 6,
            actual_backend: DecoderBackend::TurboJpeg,
            original_width: 6000,
            original_height: 4000,
            exif: None,
            is_degraded_fallback: false,
        };
        let estimated = pixels.byte_count();
        let image = DecodedImage::new(
            PlatformImage::Pixels(Arc::new(pixels)),
            width,
            height,
            estimated,
            options,
        )
        .map_err(|e| DecodeError::InvalidArgument { message: e.to_string() })?;
        Ok(Arc::new(image))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}

fn service(dir: &Path, metrics: Arc<ReviewMetrics>) -> PreviewImageService {
    let mut config = PreviewServiceConfig::new(
        metrics,
        Arc::new(|| false),
        Arc::new(|| DecodeBox::new(32, 32)),
        PixelBufferImageCodec::shared(),
        Arc::new(WicPreviewPayloadCodec),
    );
    config.capacity_bytes = 64 * 1024 * 1024;
    config.disk_cache_directory = Some(dir.to_path_buf());
    config.disable_disk_cache_override = Some(false);
    config.decoder = Some(Arc::new(GradientDecoder));
    config.physical_memory_bytes = 32 * 1024 * 1024 * 1024;
    PreviewImageService::new(config).expect("the service builds")
}

fn load(service: &PreviewImageService, path: &str) -> PreviewImage {
    service
        .get_preview_path(path)
        .wait_timeout(TIMEOUT)
        .unwrap_or_else(|_| panic!("the decode of {path} hung"))
        .unwrap_or_else(|e| panic!("the decode of {path} failed: {e}"))
}

fn retire(service: &PreviewImageService) {
    assert!(
        service.shutdown_persist_workers_timeout(TIMEOUT),
        "the persist workers exit"
    );
    assert!(service.wait_for_prune(TIMEOUT), "the prune finishes");
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pv4")))
        .collect();
    files.sort();
    files
}

// C#: PreviewImageServiceFaultTests.BrokenEntry_IsAMiss (row `jpeg-header-mangled-eoi`)
#[test]
#[ignore = "native: needs WIC"]
fn broken_entry_is_a_miss_jpeg_header_mangled_eoi() {
    let root = TempRoot::new("native-gaps-mangled-eoi");
    let source = root.join("a.png");
    std::fs::write(&source, [1, 2, 3, 4]).expect("source file");
    let source = source.to_str().expect("utf-8 path").to_owned();
    let dir = root.join("disk-jpeg-header-mangled-eoi");

    // C# `WriteEntryAsync`: decode once with a fresh service so its worker writes the entry.
    let writer = service(&dir, Arc::new(ReviewMetrics::new()));
    load(&writer, &source);
    retire(&writer);
    let files = entries(&dir);
    assert_eq!(files.len(), 1, "one entry written");
    let entry = files[0].clone();
    let valid = std::fs::read(&entry).expect("entry bytes");
    assert!(
        valid.len() > 42,
        "a real JPEG payload is longer than 42 bytes (got {})",
        valid.len()
    );

    // C# `Break("jpeg-header-mangled-eoi", valid)`: zero everything from offset 40 up to the final two bytes (the EOI marker).
    let mut broken = valid.clone();
    let len = broken.len();
    for byte in broken.iter_mut().take(len - 2).skip(40) {
        *byte = 0;
    }
    assert_ne!(broken, valid, "the entry really changed");
    std::fs::write(&entry, &broken).expect("write broken entry");

    let metrics = Arc::new(ReviewMetrics::new());
    let reader = service(&dir, metrics.clone());
    let image = load(&reader, &source);
    retire(&reader);
    assert!(image.pixel_width() > 0 && image.pixel_height() > 0);
    let snapshot = metrics.snapshot();
    assert_eq!(
        (snapshot.source_reads, snapshot.disk_cache_hits),
        (1, 0),
        "one source read, no disk hit"
    );
    if entry.exists() {
        assert_ne!(
            std::fs::read(&entry).expect("entry"),
            broken,
            "the broken entry was left in place"
        );
    }
}
