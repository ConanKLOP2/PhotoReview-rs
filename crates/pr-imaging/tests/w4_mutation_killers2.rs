//! CX-MUTW4 pure error-contract mutation regressions; no files or clocks.
use pr_imaging::embedded_thumbnail::{BlankPath, ThumbnailBackendError};

#[test]
fn embedded_reader_errors_keep_their_diagnostic_text() {
    // Mutants embedded_thumbnail.rs:47:9 and 60:9, Display body -> Ok(Default::default()).
    // C#: EmbeddedThumbnailReader keeps backend detail for debug logging and rejects blank paths.
    assert_eq!(
        ThumbnailBackendError("fake decoder detail".into()).to_string(),
        "fake decoder detail"
    );
    assert_eq!(BlankPath.to_string(), "path must not be null, empty or white space");
}

struct TinyBackend;
struct TinyFrame;
struct TinyBitmap;

impl pr_imaging::embedded_thumbnail::EmbeddedThumbnailBackend for TinyBackend {
    fn open(
        &self,
        _stream: Box<dyn pr_core::abstractions::ReadSeek>,
    ) -> Result<Option<Box<dyn pr_imaging::embedded_thumbnail::EmbeddedThumbnailFrame>>, ThumbnailBackendError> {
        Ok(Some(Box::new(TinyFrame)))
    }
}
impl pr_imaging::embedded_thumbnail::EmbeddedThumbnailFrame for TinyFrame {
    fn main_size(&mut self) -> Result<(u32, u32), ThumbnailBackendError> {
        Ok((40, 20))
    }
    fn thumbnail(&mut self) -> Option<Box<dyn pr_imaging::embedded_thumbnail::EmbeddedThumbnailBitmap>> {
        Some(Box::new(TinyBitmap))
    }
    fn orientation(&mut self) -> i32 {
        6
    }
}
impl pr_imaging::embedded_thumbnail::EmbeddedThumbnailBitmap for TinyBitmap {
    fn size(&mut self) -> Result<(u32, u32), ThumbnailBackendError> {
        Ok((2, 1))
    }
    fn copy_pbgra(&mut self, dst: &mut [u8], stride: usize) -> Result<(), ThumbnailBackendError> {
        assert_eq!(stride, 8);
        assert_eq!(dst.len(), 8);
        dst.copy_from_slice(&[1, 2, 3, 255, 4, 5, 6, 255]);
        Ok(())
    }
}

#[test]
fn acceptable_embedded_thumbnail_copies_exact_stride_and_oriented_metadata() {
    // Mutants embedded_thumbnail.rs:165:8 (delete !), 170:53 (* to + or /).
    // C#: EmbeddedThumbnailReader.TryRead applies orientation after the eager copy.
    use pr_core::model::enums::DecoderBackend;
    use pr_imaging::decode_types::DecodedImageView;
    use pr_imaging::embedded_thumbnail::try_read;
    use pr_imaging::pixel_memory::PixelBufferImageCodec;
    use pr_test_support::{FakeFileSystem, TempRoot};
    let root = TempRoot::new("mutw4-embedded");
    let path = root.path().join("tiny.jpg");
    let fs = FakeFileSystem::new();
    fs.add_file(&path, b"fake frame");
    let image = try_read(&path, &fs, &TinyBackend, &PixelBufferImageCodec::INSTANCE)
        .unwrap()
        .unwrap();
    assert_eq!((image.pixel_width(), image.pixel_height()), (1, 2));
    assert_eq!((image.original_width(), image.original_height()), (20, 40));
    assert_eq!(image.orientation(), 6);
    assert_eq!(image.estimated_bytes(), 8);
    assert_eq!(image.actual_backend(), DecoderBackend::Wpf);
    assert!(image.downscaled());
}

#[test]
fn embedded_thumbnail_pixel_cap_is_exactly_four_mebipixels() {
    // Mutants embedded_thumbnail.rs:30:41/48, * to + or / in MaxThumbnailPixels.
    // C#: EmbeddedThumbnailReader.MaxThumbnailPixels is 4 * 1024 * 1024.
    assert_eq!(pr_imaging::embedded_thumbnail::MAX_THUMBNAIL_PIXELS, 4_194_304);
}

#[test]
fn thumbnail_cache_defaults_keep_one_gib_ram_and_disk_quotas() {
    // Mutants cache/thumbnails.rs:78:46/53 and 80:45/52 (* to + or /).
    // C#: ThumbnailCache constructor defaults and DefaultMaxDiskBytes are 1 GiB.
    use pr_imaging::cache::thumbnails::{DEFAULT_MAX_DISK_BYTES, DEFAULT_MAX_RAM_BYTES};
    assert_eq!(DEFAULT_MAX_DISK_BYTES, 1_073_741_824);
    assert_eq!(DEFAULT_MAX_RAM_BYTES, 1_073_741_824);
}

#[test]
fn thumbnail_key_handles_zero_length_and_maximum_timestamp_without_worker_waits() {
    // Mutants cache/thumbnails.rs:674:5, return empty/xyzzy; key collisions must fail immediately.
    // C#: ThumbnailCache.BuildKey hashes the invariant stamp as UTF-8 using SHA-256.
    use pr_imaging::cache::thumbnails::{build_key, build_stamp};
    assert_eq!(
        build_stamp("album/Frame.JpG", 0, i64::MAX),
        "album/Frame.JpG|0|9223372036854775807|800|orient=1"
    );
    assert_eq!(
        build_key("album/Frame.JpG", 0, i64::MAX),
        "cdaa4448016705ec62a1f27b055295837be8e8ba05f11d6b993ac45f167b843c"
    );
}

#[test]
fn thumbnail_cache_path_preserves_directory_and_opaque_key() {
    // Mutant cache/thumbnails.rs:686:5, cache_file_path -> Default::default().
    // C#: ThumbnailCache combines the disk directory and key + .png.
    use pr_imaging::cache::thumbnails::cache_file_path;
    let root = pr_test_support::TempRoot::new("mutw4-path");
    let directory = root.path().join("nested-é");
    assert_eq!(cache_file_path(&directory, "abc012"), directory.join("abc012.png"));
}

struct UnusedDiskReader;
impl pr_imaging::cache::thumbnails::CacheImageReader for UnusedDiskReader {
    fn decode(
        &self,
        _: &[u8],
        _: Option<pr_imaging::pixels::PixelLayout>,
        _: pr_imaging::decode_types::DecodeBox,
        _: i32,
        _: i32,
    ) -> Result<
        (
            pr_imaging::pixels::PixelBuffer,
            pr_imaging::cache::thumbnails::CacheImageInfo,
        ),
        pr_imaging::decoder_traits::DecodeError,
    > {
        panic!("this fake source has no disk cache entry");
    }
}

#[test]
fn inline_thumbnail_load_publishes_result_before_spawner_returns() {
    // Mutants cache/thumbnails.rs:841:5 (finish_load -> ()), 865:5 (load_or_create -> default).
    // C#: shared ThumbnailCache load publishes its outcome before awaiters observe it.
    // Synchronous seam: try_result observes completed work, with no timed waits or workers.
    use pr_imaging::cache::thumbnails::{
        ThumbnailCache, ThumbnailCacheOptions, ThumbnailCacheServices, ThumbnailRequest,
    };
    use pr_imaging::pixel_memory::PixelBufferImageCodec;
    use pr_test_support::{FakeClock, FakeFileSystem, TempRoot};
    use std::sync::Arc;
    let root = TempRoot::new("mutw4-inline");
    let path = root.path().join("tiny.jpg");
    let fs = Arc::new(FakeFileSystem::new());
    fs.add_file(&path, b"fake frame");
    let mut options = ThumbnailCacheOptions::new(root.path().join("cache"));
    options.clock = Arc::new(FakeClock::new());
    options.spawner = Some(Arc::new(|job| {
        job();
        Ok(())
    }));
    let cache = ThumbnailCache::new(
        ThumbnailCacheServices {
            platform_codec: Arc::new(PixelBufferImageCodec::INSTANCE),
            cache_image_reader: Arc::new(UnusedDiskReader),
            embedded_backend: Arc::new(TinyBackend),
            fs,
        },
        options,
    )
    .unwrap();
    let name = path.to_str().unwrap();
    for _ in 0..2 {
        let request = cache.get(name, None).unwrap();
        let ThumbnailRequest::Pending(pending) = request else {
            panic!("RAM was cleared before this request");
        };
        let image = pending
            .try_result()
            .expect("inline load has published")
            .unwrap()
            .unwrap();
        assert_eq!((image.pixel_width(), image.pixel_height()), (1, 2));
        assert!(matches!(
            cache.get(name, None).unwrap(),
            ThumbnailRequest::Ready(Ok(Some(_)))
        ));
        cache.clear_memory();
    }
}

struct MetadataDiskReader;
impl pr_imaging::cache::thumbnails::CacheImageReader for MetadataDiskReader {
    fn decode(
        &self,
        encoded: &[u8],
        layout: Option<pr_imaging::pixels::PixelLayout>,
        decode_box: pr_imaging::decode_types::DecodeBox,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<
        (
            pr_imaging::pixels::PixelBuffer,
            pr_imaging::cache::thumbnails::CacheImageInfo,
        ),
        pr_imaging::decoder_traits::DecodeError,
    > {
        use pr_imaging::cache::thumbnails::CacheImageInfo;
        use pr_imaging::pixels::{PixelBuffer, PixelLayout};
        assert_eq!(encoded, b"fake png");
        assert_eq!(layout, None);
        assert_eq!(decode_box, pr_imaging::decode_types::DecodeBox::new(800, 0));
        assert_eq!((expected_width, expected_height), (0, 0));
        Ok((
            PixelBuffer::allocate(3, 2, PixelLayout::Pbgra32).unwrap(),
            CacheImageInfo {
                source_width: 40,
                source_height: 20,
                downscaled: true,
            },
        ))
    }
}

#[test]
fn disk_thumbnail_keeps_original_dimensions_and_downscaled_metadata() {
    // Mutants cache/thumbnails.rs:732:13, 735:13, 736:13, delete metadata fields.
    // C#: ThumbnailCache.DecodeDiskThumbnail preserves WicCacheImageInfo after shrinking.
    use pr_core::model::enums::DecoderBackend;
    use pr_imaging::cache::thumbnails::decode_disk_thumbnail;
    use pr_imaging::decode_types::DecodedImageView;
    use pr_imaging::pixel_memory::PixelBufferImageCodec;
    use pr_test_support::{FakeFileSystem, TempRoot};
    let root = TempRoot::new("mutw4-disk-metadata");
    let path = root.path().join("entry.png");
    let fs = FakeFileSystem::new();
    fs.add_file(&path, b"fake png");
    let image = decode_disk_thumbnail(&path, &fs, &MetadataDiskReader, &PixelBufferImageCodec::INSTANCE).unwrap();
    assert_eq!((image.pixel_width(), image.pixel_height()), (3, 2));
    assert_eq!((image.original_width(), image.original_height()), (40, 20));
    assert!(image.downscaled());
    assert_eq!(image.orientation(), 1);
    assert_eq!(image.actual_backend(), DecoderBackend::WicDirect);
    assert_eq!(image.estimated_bytes(), 24);
}

#[test]
fn thumbnail_errors_keep_diagnostics_and_underlying_source() {
    // Mutants cache/thumbnails.rs:136:9 (Display -> Ok), 156:9 (source -> None).
    // C#: ThumbnailCache preserves source exception diagnostics when loads fail.
    use pr_imaging::cache::thumbnails::ThumbnailError;
    use std::error::Error;
    use std::sync::Arc;
    assert_eq!(ThumbnailError::Canceled.to_string(), "The operation was canceled.");
    let error = ThumbnailError::Stat(Arc::new(std::io::Error::other("fake stat detail")));
    assert_eq!(error.to_string(), "fake stat detail");
    assert_eq!(error.source().unwrap().to_string(), "fake stat detail");
    assert!(ThumbnailError::BlankPath.source().is_none());
}

#[test]
fn thumbnail_quota_configuration_errors_keep_parameter_and_actual_value() {
    // Mutant cache/thumbnails.rs:181:9, config Display -> Ok(Default::default()).
    // C#: ArgumentOutOfRangeException.ThrowIfNegativeOrZero reports parameter and actual value.
    use pr_imaging::cache::thumbnails::ThumbnailCacheConfigError;
    for (error, parameter) in [
        (ThumbnailCacheConfigError::MaxRamBytes(-3), "maxRamBytes"),
        (ThumbnailCacheConfigError::MaxDiskBytes(-3), "maxDiskBytes"),
    ] {
        assert_eq!(
            error.to_string(),
            format!(
                "{parameter} ('-3') must be a non-negative and non-zero value. (Parameter '{parameter}')\nActual value was -3."
            )
        );
    }
}
