//! Native twins of C# ThumbnailCache and PreviewCacheCrossVersion tests.
#![cfg(windows)]
#[path = "common/imaging_twins_wic.rs"]
mod imaging_twins_wic;
use pr_core::{
    abstractions::{FileStat, Log},
    io::physical::PhysicalFileSystem,
    model::enums::DecoderBackend,
    sync::CancelToken,
    text::dotnet_path::get_full_path,
    time::DateTimeUtc,
};
use pr_imaging::{
    cache::{
        disk_store::{PngEncoder, is_cache_entry_failure, write_pixels_atomically},
        thumbnails::*,
    },
    decode_types::{DecodedImage, DecodedImageOptions, DecodedImageView, PlatformImage},
    embedded_thumbnail::EmbeddedThumbnailBackend,
    pixel_memory::PixelBufferImageCodec,
    pixels::{PixelBuffer, PixelLayout, is_fully_opaque},
};
use pr_platform_win::wic::{
    cache_adapters::{WicCacheImageReader, encode_error_to_io},
    cache_image,
};
use pr_test_support::golden::{bool_of, i64_of, str_of};
use pr_test_support::{FakeClock, RecordingLog, TempRoot, golden};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
const SOURCE_BYTES: &[u8] = b"fake-jpeg";
struct NativePng;
impl PngEncoder for NativePng {
    fn encode_png(
        &self,
        pixels: &PixelBuffer,
        out: &mut dyn std::io::Write,
    ) -> Result<(), pr_imaging::decoder_traits::DecodeError> {
        cache_image::encode_png(pixels, out).map_err(|e| pr_imaging::decoder_traits::DecodeError::InvalidData {
            message: encode_error_to_io(e).to_string(),
        })
    }
}
fn native_cache_image_reader() -> Arc<dyn CacheImageReader> {
    Arc::new(WicCacheImageReader)
}
fn native_embedded_backend() -> Arc<dyn EmbeddedThumbnailBackend> {
    Arc::new(imaging_twins_wic::NativeBackend)
}
fn native_png_encoder() -> Arc<dyn PngEncoder> {
    Arc::new(NativePng)
}

#[derive(Debug)]
struct FakeImage;
fn native_services() -> ThumbnailCacheServices {
    ThumbnailCacheServices {
        platform_codec: PixelBufferImageCodec::shared(),
        cache_image_reader: native_cache_image_reader(),
        embedded_backend: native_embedded_backend(),
        fs: Arc::new(PhysicalFileSystem),
    }
}

fn default_reader_options(dir: &Path, log: Arc<dyn Log>) -> ThumbnailCacheOptions {
    ThumbnailCacheOptions {
        disk_directory: dir.to_path_buf(),
        max_ram_bytes: DEFAULT_MAX_RAM_BYTES,
        max_disk_bytes: DEFAULT_MAX_DISK_BYTES,
        log,
        clock: Arc::new(FakeClock::new()),
        embedded_reader: None,
        spawner: None,
    }
}

fn fixture_path(relative: &str) -> PathBuf {
    golden::repo_root().join("tests").join("fixtures").join(relative)
}

fn thumbnail_cache_file(dir: &Path, source: &str, stat: FileStat) -> PathBuf {
    let full = get_full_path(source).expect("source path is valid");
    cache_file_path(dir, &build_key(&full, stat.length, stat.last_write_utc.ticks()))
}

fn physical_stat(path: &Path) -> FileStat {
    let meta = fs::metadata(path).expect("file exists");
    FileStat {
        length: meta.len(),
        last_write_utc: DateTimeUtc::from_system_time(meta.modified().expect("mtime is available"))
            .expect("mtime is in the DateTime range"),
    }
}

fn decoded_image(width: i32, height: i32, original: (i32, i32)) -> ThumbnailImage {
    let image = DecodedImage::new(
        PlatformImage::Opaque(Arc::new(FakeImage)),
        width,
        height,
        i64::from(width) * i64::from(height) * 4,
        DecodedImageOptions {
            downscaled: true,
            original_width: original.0,
            original_height: original.1,
            ..DecodedImageOptions::default()
        },
    )
    .expect("valid fake image");
    Arc::new(image)
}

fn settle(request: ThumbnailRequest) -> Option<ThumbnailImage> {
    request.wait().expect("the load succeeds")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

#[test]
#[ignore = "native: needs WIC"]
fn get_async_returns_embedded_thumbnail_on_miss_native() {
    // C#: ThumbnailCacheTests.GetAsyncReturnsEmbeddedThumbnailOnMiss (native twin: real W4-02b reader)
    let root = TempRoot::new("embedded-native");
    let source = fixture_path("embedded-thumbnail/thumb-main480-thumb160.jpg");
    let source_text = source.to_str().expect("utf-8 fixture path");
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    let cache = ThumbnailCache::new(native_services(), default_reader_options(&root.join("thumbs"), log))
        .expect("valid options");

    let image = settle(cache.get(source_text, None).expect("valid path")).expect("has an embedded thumbnail");

    assert_eq!(image.pixel_width(), 160, "the embedded thumbnail is 160 px wide");
    assert_eq!(image.original_width(), 480, "the main frame is 480 px wide");
}

#[test]
#[ignore = "native: needs WIC"]
fn get_async_returns_null_when_source_has_no_embedded_thumbnail_native() {
    // C#: ThumbnailCacheTests.GetAsyncReturnsNullWhenSourceHasNoEmbeddedThumbnail (native twin: real W4-02b reader)
    let root = TempRoot::new("no-embedded-native");
    let source = fixture_path("embedded-thumbnail/plain-main64.jpg");
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    let cache = ThumbnailCache::new(native_services(), default_reader_options(&root.join("thumbs"), log))
        .expect("valid options");

    let image = settle(
        cache
            .get(source.to_str().expect("utf-8 path"), None)
            .expect("valid path"),
    );

    assert!(image.is_none());
}

#[test]
#[ignore = "native: needs WIC"]
fn get_async_returns_null_for_non_jpeg_source_native() {
    // C#: ThumbnailCacheTests.GetAsyncReturnsNullForNonJpegSource (native twin: real W4-02b reader)
    let root = TempRoot::new("non-jpeg-native");
    let source = fixture_path("embedded-thumbnail/not-jpeg.png");
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    let cache = ThumbnailCache::new(native_services(), default_reader_options(&root.join("thumbs"), log))
        .expect("valid options");

    let image = settle(
        cache
            .get(source.to_str().expect("utf-8 path"), None)
            .expect("valid path"),
    );

    assert!(image.is_none());
}

#[test]
#[ignore = "native: needs WIC"]
fn mutated_cached_png_is_regenerated_not_thrown() {
    // C#: Robustness/EmbeddedThumbnailMutationTests.MutatedCachedPng_IsRegeneratedNotThrown (150 mutants, fixed seed)
    let root = TempRoot::new("mutated-png");
    let source = root.join("a.jpg");
    fs::write(&source, SOURCE_BYTES).expect("write source");
    let thumbs = root.join("thumbs");
    fs::create_dir_all(&thumbs).expect("create thumbs");
    let source_text = source.to_str().expect("utf-8 path");

    let mut opaque = PixelBuffer::allocate(16, 16, PixelLayout::Pbgra32).expect("valid size");
    for pixel in opaque.bytes_mut().expect("not disposed").chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    let entry = thumbnail_cache_file(&thumbs, source_text, physical_stat(&source));
    write_pixels_atomically(&opaque, &*native_png_encoder(), &entry, None, &CancelToken::new())
        .expect("write the opaque PNG");
    let original = fs::read(&entry).expect("read the PNG back");

    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    let services = ThumbnailCacheServices {
        platform_codec: PixelBufferImageCodec::shared(),
        cache_image_reader: native_cache_image_reader(),
        embedded_backend: native_embedded_backend(),
        fs: Arc::new(PhysicalFileSystem),
    };
    let embedded: EmbeddedThumbnailReaderFn =
        Arc::new(|_: &str, _: &CancelToken| Ok(Some(decoded_image(16, 16, (16, 16)))));
    let options = ThumbnailCacheOptions {
        embedded_reader: Some(embedded),
        ..default_reader_options(&thumbs, log)
    };
    let cache = ThumbnailCache::new(services, options).expect("valid options");

    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..150 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mut mutant = original.clone();
        let index = (state >> 33) as usize % mutant.len();
        mutant[index] ^= ((state >> 17) as u8) | 1;
        fs::write(&entry, &mutant).expect("write the mutant");
        cache.clear_memory();
        let image = settle(cache.get(source_text, None).expect("valid path"));
        assert!(image.is_some(), "a mutated cache entry is regenerated, never thrown");
    }
}

#[test]
#[ignore = "native"]
fn disk_thumbnail_fits_to_max_width_and_reports_backend_and_orientation_native() {
    // C#: WicCacheImageIoTests.DiskThumbnail_FitsToMaxWidth_AndReportsBackendAndOrientation (real-WIC twin; both PNGs are
    // written with W4-09 write_pixels_atomically)
    let root = TempRoot::new("disk-fit-native");
    let encoder = native_png_encoder();
    let reader = native_cache_image_reader();
    let codec = PixelBufferImageCodec::shared();
    for (width, height, kept_width, kept_height, downscaled) in [(1600, 400, 800, 200, true), (100, 50, 100, 50, false)]
    {
        let pixels = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).expect("valid size");
        let path = root.join(format!("{width}x{height}.png"));
        write_pixels_atomically(&pixels, &*encoder, &path, None, &CancelToken::new()).expect("write the PNG");

        let image = decode_disk_thumbnail(&path, &PhysicalFileSystem, &*reader, &*codec).expect("the PNG decodes");

        assert_eq!((image.pixel_width(), image.pixel_height()), (kept_width, kept_height));
        assert_eq!(image.downscaled(), downscaled);
        assert_eq!((image.original_width(), image.original_height()), (width, height));
        assert!(matches!(image.actual_backend(), DecoderBackend::WicDirect));
    }
}

#[test]
#[ignore = "native: needs WIC"]
fn committed_old_png_thumbnail_reads_with_new_code_bgr32() {
    // C#: PreviewCacheCrossVersionTests.CommittedOldPngThumbnail_ReadsWithNewCode (InlineData: bgr32)
    let path = fixture_path("PreviewCache/thumb-bgr32-48x32.png");
    let reader = native_cache_image_reader();
    let codec = PixelBufferImageCodec::shared();

    let image = decode_disk_thumbnail(&path, &PhysicalFileSystem, &*reader, &*codec).expect("the old PNG decodes");

    assert_eq!((image.pixel_width(), image.pixel_height()), (48, 32));
    let pixels = image
        .platform_image()
        .as_pixels()
        .expect("the platform image is a pixel buffer");
    assert_eq!(
        pixels.layout(),
        PixelLayout::Bgr32,
        "an opaque WIC format decodes to Bgr32"
    );
}

#[test]
#[ignore = "native: needs WIC"]
fn committed_old_png_thumbnail_reads_with_new_code_bgra32() {
    // C#: PreviewCacheCrossVersionTests.CommittedOldPngThumbnail_ReadsWithNewCode (InlineData: bgra32)
    let path = fixture_path("PreviewCache/thumb-bgra32-48x32.png");
    let reader = native_cache_image_reader();
    let codec = PixelBufferImageCodec::shared();

    let image = decode_disk_thumbnail(&path, &PhysicalFileSystem, &*reader, &*codec).expect("the old PNG decodes");

    assert_eq!((image.pixel_width(), image.pixel_height()), (48, 32));
    let pixels = image
        .platform_image()
        .as_pixels()
        .expect("the platform image is a pixel buffer");
    assert_eq!(
        pixels.layout(),
        PixelLayout::Pbgra32,
        "a PNG with alpha decodes to Pbgra32"
    );
}

#[test]
#[ignore = "native: needs WIC"]
fn translucent_png_round_trips_through_wic() {
    // C#: PreviewCacheCrossVersionTests.TranslucentPng_RoundTripsThroughWic (ported here: it ends in DecodeDiskThumbnail)
    let root = TempRoot::new("translucent");
    let mut source = PixelBuffer::allocate(40, 24, PixelLayout::Pbgra32).expect("valid size");
    let width = 40;
    for (index, pixel) in source
        .bytes_mut()
        .expect("not disposed")
        .chunks_exact_mut(4)
        .enumerate()
    {
        let x = index % width;
        let y = index / width;
        let alpha = (100 + (x * 3 + y) % 120) as u8;
        pixel[0] = alpha / 2;
        pixel[1] = alpha / 3;
        pixel[2] = alpha / 4;
        pixel[3] = alpha;
    }
    let path = root.join("translucent.png");
    write_pixels_atomically(&source, &*native_png_encoder(), &path, None, &CancelToken::new())
        .expect("write the translucent PNG");

    let image = decode_disk_thumbnail(
        &path,
        &PhysicalFileSystem,
        &*native_cache_image_reader(),
        &*PixelBufferImageCodec::shared(),
    )
    .expect("the translucent PNG decodes");

    let decoded = image.platform_image().as_pixels().expect("pixel buffer");
    assert_eq!(decoded.layout(), PixelLayout::Pbgra32);
    assert!(
        !is_fully_opaque(decoded).expect("not disposed"),
        "the alpha survives the round trip"
    );
    let expected = source.bytes().expect("not disposed");
    let actual = decoded.bytes().expect("not disposed");
    assert_eq!(expected.len(), actual.len());
    let total: u64 = expected
        .iter()
        .zip(actual)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    let mean = total as f64 / expected.len() as f64;
    assert!(mean <= 1.0, "mean absolute error {mean} is above 1.0");
}

#[test]
#[ignore = "native: needs WIC"]
fn thumbnail_disk_decode_matches_golden() {
    // Golden: tests/golden/thumbnail-disk-decode.json (15 cases); inputs in tests/golden-inputs/thumbnail-disk-decode.json
    // (PNG bytes inline as base64, or a committed fixture, or missing). Error type names differ from .NET: only
    // `ok` and `is_cache_entry_failure` are compared for failures (contract, semantics 6).
    let golden = golden::load("thumbnail-disk-decode");
    let inputs_text = fs::read_to_string(
        golden::repo_root()
            .join("tests")
            .join("golden-inputs")
            .join("thumbnail-disk-decode.json"),
    )
    .expect("read the disk-decode inputs");
    let inputs: serde_json::Value = serde_json::from_str(&inputs_text).expect("parse the disk-decode inputs");
    let root = TempRoot::new("disk-decode-golden");
    let reader = native_cache_image_reader();
    let codec = PixelBufferImageCodec::shared();

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = inputs["cases"]
            .as_array()
            .expect("inputs have cases")
            .iter()
            .find(|candidate| candidate["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("no input for {name}"));

        let path = if bool_of(case, "missing") {
            root.join("missing-file.png")
        } else if let Some(fixture) = input["fixture"].as_str() {
            fixture_path(fixture)
        } else {
            let path = root.join(format!("{name}.png"));
            let bytes = base64_decode(str_of(input, "png_base64"));
            fs::write(&path, bytes).expect("write the input PNG");
            path
        };

        let result = decode_disk_thumbnail(&path, &PhysicalFileSystem, &*reader, &*codec);

        assert_eq!(result.is_ok(), bool_of(case, "ok"), "{name}: success flag");
        match result {
            Ok(image) => {
                assert_eq!(
                    i64::from(image.pixel_width()),
                    i64_of(case, "pixel_width"),
                    "{name}: width"
                );
                assert_eq!(
                    i64::from(image.pixel_height()),
                    i64_of(case, "pixel_height"),
                    "{name}: height"
                );
                assert_eq!(
                    image.estimated_bytes(),
                    i64_of(case, "estimated_bytes"),
                    "{name}: estimated bytes"
                );
                assert_eq!(image.downscaled(), bool_of(case, "downscaled"), "{name}: downscaled");
                assert_eq!(
                    i64::from(image.orientation()),
                    i64_of(case, "orientation"),
                    "{name}: orientation"
                );
                assert!(
                    matches!(image.actual_backend(), DecoderBackend::WicDirect),
                    "{name}: actual backend is WicDirect"
                );
                assert_eq!(
                    i64::from(image.original_width()),
                    i64_of(case, "original_width"),
                    "{name}: original width"
                );
                assert_eq!(
                    i64::from(image.original_height()),
                    i64_of(case, "original_height"),
                    "{name}: original height"
                );
                let pixels = image.platform_image().as_pixels().expect("pixel buffer");
                let layout = match pixels.layout() {
                    PixelLayout::Bgr32 => "Bgr32",
                    PixelLayout::Pbgra32 => "Pbgra32",
                };
                assert_eq!(layout, str_of(case, "layout"), "{name}: layout");
                assert_eq!(i64::from(pixels.stride()), i64_of(case, "stride"), "{name}: stride");
                let digest = hex(&sha256(pixels.bytes().expect("not disposed")));
                assert_eq!(digest, str_of(case, "pixels_sha256"), "{name}: pixel digest");
            }
            Err(error) => {
                assert_eq!(
                    is_cache_entry_failure(&error),
                    bool_of(case, "is_cache_entry_failure"),
                    "{name}: cache-entry failure class"
                );
            }
        }
    }
}
