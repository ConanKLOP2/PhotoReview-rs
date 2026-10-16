//! Red tests for W4-10b: `pr_imaging::cache::thumbnails` (port of C# `ThumbnailCache`, baseline d92d6aae), plus the golden
//! packages `thumbnail-cache-key` (pure, not ignored) and `thumbnail-disk-decode` (native, ignored).
//!
//! Each test compiles against the stub and fails through `todo!()` until the porter fills the bodies.
//!
//! Native twins are `#[ignore = "native: ..."]`. They need the real W4-11e CacheImageReader, the W4-11c embedded backend
//! and the W4-11e PNG encoder. Those live in `pr-platform-win`, which depends on `pr-imaging`, so this file reaches them
//! only through [`native_cache_image_reader`], [`native_embedded_backend`] and [`native_png_encoder`] (all `todo!()`
//! until the lead wires the dev-dependency).
//!
//! Synchronisation uses channels and gates, never sleeps (AGENTS.md).

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use pr_core::abstractions::{AppendStream, FileStat, FileSystem, Log, ReadSeek, SystemClock};
use pr_core::diagnostics::perf::{PerfEvent, PerfListener, PerfValue, PhotoReviewPerf, path_id as perf_path_id};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::{PathError, get_full_path};
use pr_core::time::DateTimeUtc;
use pr_imaging::cache::disk_store::{PngEncoder, is_cache_entry_failure, write_pixels_atomically};
use pr_imaging::cache::thumbnails::{
    CacheImageInfo, CacheImageReader, DEFAULT_MAX_DISK_BYTES, DEFAULT_MAX_RAM_BYTES, EmbeddedThumbnailReaderFn,
    LoadSpawner, PendingThumbnail, ThumbnailCache, ThumbnailCacheConfigError, ThumbnailCacheOptions,
    ThumbnailCacheServices, ThumbnailError, ThumbnailImage, ThumbnailRequest, ThumbnailResult, build_key, build_stamp,
    cache_file_path, decode_disk_thumbnail, sha256,
};
use pr_imaging::decode_types::{
    DecodeBox, DecodedImage, DecodedImageOptions, DecodedImageView, PixelLease, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, PlatformImageCodec};
use pr_imaging::embedded_thumbnail::{
    EmbeddedThumbnailBackend, EmbeddedThumbnailBitmap, EmbeddedThumbnailFrame, ThumbnailBackendError,
};
use pr_imaging::pixel_memory::PixelBufferImageCodec;
use pr_imaging::pixels::{PixelBuffer, PixelLayout, is_fully_opaque};
use pr_test_support::golden::{bool_of, i64_of, str_of};
use pr_test_support::{FakeClock, FakeFileSystem, LogLevel, RecordingLog, TempRoot, golden};

/// A JPEG source the fake file system knows about (its bytes are never decoded by the fakes).
const SOURCE: &str = r"C:\Photos\a.jpg";
const SOURCE_BYTES: &[u8] = b"fake-jpeg";
/// Disk directory used with the fake file system (nothing is created on disk for it).
const FAKE_DIR: &str = r"C:\thumbs";
/// The COM HRESULT 0x88982F50 of the golden `garbage-4-bytes` case, as the `i32` that `DecodeError::Com` carries.
const WINCODEC_ERR_BADHEADER: i32 = -2_003_292_336;

// ---- Native seams (todo until the lead wires pr-platform-win into pr-imaging) ------------------------------------

/// The real W4-11e `CacheImageReader` (WIC PNG decode of a disk entry).
fn native_cache_image_reader() -> Arc<dyn CacheImageReader> {
    todo!("native: W4-11e CacheImageReader (pr_platform_win::wic::cache_image) needs the lead's dev-dependency")
}

/// The real W4-11c embedded-thumbnail backend (WIC frame thumbnail).
fn native_embedded_backend() -> Arc<dyn EmbeddedThumbnailBackend> {
    todo!("native: W4-11c embedded backend (pr_platform_win) needs the lead's dev-dependency")
}

/// The real W4-11e PNG encoder used to write a PNG fixture for a native test.
fn native_png_encoder() -> Arc<dyn PngEncoder> {
    todo!("native: W4-11e PNG encoder (pr_platform_win::wic::cache_image) needs the lead's dev-dependency")
}

/// Services wired to the real file system and the native seams.
fn native_services() -> ThumbnailCacheServices {
    ThumbnailCacheServices {
        platform_codec: PixelBufferImageCodec::shared(),
        cache_image_reader: native_cache_image_reader(),
        embedded_backend: native_embedded_backend(),
        fs: Arc::new(PhysicalFileSystem),
    }
}

/// Options with the C# defaults, the default embedded reader and a recording log.
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

// ---- Fakes ------------------------------------------------------------------------------------------------------

/// Path of a committed read-only fixture under `tests/fixtures`.
fn fixture_path(relative: &str) -> PathBuf {
    golden::repo_root().join("tests").join("fixtures").join(relative)
}

/// C# `ThumbnailCacheFiles.PathFor`: the cache file of `source` for the given stat (a FRESH stat, per the test).
fn thumbnail_cache_file(dir: &Path, source: &str, stat: FileStat) -> PathBuf {
    let full = get_full_path(source).expect("source path is valid");
    cache_file_path(dir, &build_key(&full, stat.length, stat.last_write_utc.ticks()))
}

/// The stat of a real file, as `FileInfo` would report it.
fn physical_stat(path: &Path) -> FileStat {
    let meta = fs::metadata(path).expect("file exists");
    FileStat {
        length: meta.len(),
        last_write_utc: DateTimeUtc::from_system_time(meta.modified().expect("mtime is available"))
            .expect("mtime is in the DateTime range"),
    }
}

/// A platform image marker produced by [`TagCodec`] (C# `PlatformImage` of the tagging codec).
#[derive(Debug)]
struct CodecTag;

/// Platform codec that counts calls and tags the platform image it returns.
#[derive(Default)]
struct TagCodec {
    images: AtomicUsize,
}

impl PlatformImageCodec for TagCodec {
    fn name(&self) -> &'static str {
        "tag"
    }

    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        self.images.fetch_add(1, Ordering::SeqCst);
        drop(pixels);
        Ok(PlatformImage::Opaque(Arc::new(CodecTag)))
    }

    fn to_pixels<'a>(&self, _platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        Err(DecodeError::InvalidArgument {
            message: "the tagging codec cannot encode".to_owned(),
        })
    }
}

/// Platform image of a fake embedded or disk thumbnail that carries no pixels.
#[derive(Debug)]
struct FakeImage;

/// A decoded image of `width` x `height` whose original size is `original`, as an embedded reader would return.
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

/// A label with the constructor of a `DecodeError` to inject.
type NamedDecodeError = (&'static str, fn() -> DecodeError);

/// One call to the fake cache-image reader, as the arguments reached it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DecodeCall {
    layout: Option<PixelLayout>,
    box_width: i32,
    box_height: i32,
    expected_width: i32,
    expected_height: i32,
}

type CacheDecodeScript = dyn Fn(usize, &[u8]) -> Result<(PixelBuffer, CacheImageInfo), DecodeError> + Send + Sync;

/// Fake of the WIC `CacheImageReader` seam: records every call and answers from a script.
struct FakeCacheReader {
    calls: Mutex<Vec<DecodeCall>>,
    script: Box<CacheDecodeScript>,
}

impl FakeCacheReader {
    fn new(
        script: impl Fn(usize, &[u8]) -> Result<(PixelBuffer, CacheImageInfo), DecodeError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            script: Box::new(script),
        }
    }

    /// Always decodes to a `width` x `height` Pbgra32 buffer with `info`.
    fn returning(width: i32, height: i32, info: CacheImageInfo) -> Self {
        Self::new(move |_, _| {
            let pixels = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).expect("valid fake size");
            Ok((pixels, info))
        })
    }

    /// Every call fails with the error `make` builds.
    fn failing(make: fn() -> DecodeError) -> Self {
        Self::new(move |_, _| Err(make()))
    }

    fn call_count(&self) -> usize {
        self.calls.lock().expect("calls lock").len()
    }

    fn calls(&self) -> Vec<DecodeCall> {
        self.calls.lock().expect("calls lock").clone()
    }
}

impl CacheImageReader for FakeCacheReader {
    fn decode(
        &self,
        encoded: &[u8],
        layout: Option<PixelLayout>,
        decode_box: DecodeBox,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<(PixelBuffer, CacheImageInfo), DecodeError> {
        let index = {
            let mut calls = self.calls.lock().expect("calls lock");
            calls.push(DecodeCall {
                layout,
                box_width: decode_box.width,
                box_height: decode_box.height,
                expected_width,
                expected_height,
            });
            calls.len() - 1
        };
        (self.script)(index, encoded)
    }
}

/// Embedded-thumbnail backend that finds no frames (the fake reader is used instead).
struct NoThumbnailBackend;

impl EmbeddedThumbnailBackend for NoThumbnailBackend {
    fn open(
        &self,
        _stream: Box<dyn ReadSeek>,
    ) -> Result<Option<Box<dyn EmbeddedThumbnailFrame>>, ThumbnailBackendError> {
        Ok(None)
    }
}

/// Fake W4-02b backend: one frame with a main size, an embedded thumbnail size and an orientation of 1.
struct FakeEmbeddedBackend {
    main: (u32, u32),
    thumbnail: (u32, u32),
}

struct FakeFrame {
    main: (u32, u32),
    thumbnail: (u32, u32),
}

struct FakeBitmap {
    size: (u32, u32),
}

impl EmbeddedThumbnailBackend for FakeEmbeddedBackend {
    fn open(
        &self,
        _stream: Box<dyn ReadSeek>,
    ) -> Result<Option<Box<dyn EmbeddedThumbnailFrame>>, ThumbnailBackendError> {
        Ok(Some(Box::new(FakeFrame {
            main: self.main,
            thumbnail: self.thumbnail,
        })))
    }
}

impl EmbeddedThumbnailFrame for FakeFrame {
    fn main_size(&mut self) -> Result<(u32, u32), ThumbnailBackendError> {
        Ok(self.main)
    }

    fn thumbnail(&mut self) -> Option<Box<dyn EmbeddedThumbnailBitmap>> {
        Some(Box::new(FakeBitmap { size: self.thumbnail }))
    }

    fn orientation(&mut self) -> i32 {
        1
    }
}

impl EmbeddedThumbnailBitmap for FakeBitmap {
    fn size(&mut self) -> Result<(u32, u32), ThumbnailBackendError> {
        Ok(self.size)
    }

    fn copy_pbgra(&mut self, dst: &mut [u8], _stride: usize) -> Result<(), ThumbnailBackendError> {
        dst.fill(0x80);
        Ok(())
    }
}

/// An embedded reader that counts its calls and answers from `script` (the call index is passed in).
fn counting_embedded(
    script: impl Fn(usize, &str, &CancelToken) -> ThumbnailResult + Send + Sync + 'static,
) -> (EmbeddedThumbnailReaderFn, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let reader: EmbeddedThumbnailReaderFn = Arc::new(move |path: &str, token: &CancelToken| {
        let index = counter.fetch_add(1, Ordering::SeqCst);
        script(index, path, token)
    });
    (reader, calls)
}

/// The test's side of a gated embedded reader: the first call reports its start and waits for `release`.
struct Gate {
    started: Receiver<()>,
    release: Sender<()>,
}

/// Embedded reader whose FIRST call blocks until the test releases it (it ignores the dispose token, as the
/// C# test's injected reader does). Later calls run `script` at once.
fn gate_first_call(
    script: impl Fn(usize) -> ThumbnailResult + Send + Sync + 'static,
) -> (EmbeddedThumbnailReaderFn, Arc<AtomicUsize>, Gate) {
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let started_tx = Mutex::new(started_tx);
    let release_rx = Mutex::new(release_rx);
    let (reader, calls) = counting_embedded(move |index, _, _| {
        if index == 0 {
            started_tx
                .lock()
                .expect("started sender lock")
                .send(())
                .expect("the test waits for the start");
            release_rx
                .lock()
                .expect("release receiver lock")
                .recv()
                .expect("the test releases the load");
        }
        script(index)
    });
    (
        reader,
        calls,
        Gate {
            started: started_rx,
            release: release_tx,
        },
    )
}

/// An embedded reader that never has a thumbnail (`Ok(None)`) and is never expected to be called.
fn unused_embedded() -> EmbeddedThumbnailReaderFn {
    let reader: EmbeddedThumbnailReaderFn = Arc::new(|_: &str, _: &CancelToken| Ok(None));
    reader
}

/// Test fixture: a fake file system with one source, a counting codec, a fake cache-image reader and a recording log.
struct Fixture {
    fs: Arc<FakeFileSystem>,
    codec: Arc<TagCodec>,
    reader: Arc<FakeCacheReader>,
    log: Arc<RecordingLog>,
}

impl Fixture {
    fn new(reader: FakeCacheReader) -> Self {
        let fs = Arc::new(FakeFileSystem::new());
        fs.add_file(SOURCE, SOURCE_BYTES);
        Self {
            fs,
            codec: Arc::new(TagCodec::default()),
            reader: Arc::new(reader),
            log: Arc::new(RecordingLog::new()),
        }
    }

    fn services_on(&self, fs: Arc<dyn FileSystem>) -> ThumbnailCacheServices {
        ThumbnailCacheServices {
            platform_codec: self.codec.clone(),
            cache_image_reader: self.reader.clone(),
            embedded_backend: Arc::new(NoThumbnailBackend),
            fs,
        }
    }

    fn services(&self) -> ThumbnailCacheServices {
        self.services_on(self.fs.clone())
    }

    fn options(&self, dir: impl Into<PathBuf>, embedded: EmbeddedThumbnailReaderFn) -> ThumbnailCacheOptions {
        ThumbnailCacheOptions {
            disk_directory: dir.into(),
            max_ram_bytes: DEFAULT_MAX_RAM_BYTES,
            max_disk_bytes: DEFAULT_MAX_DISK_BYTES,
            log: self.log.clone(),
            clock: Arc::new(FakeClock::new()),
            embedded_reader: Some(embedded),
            spawner: None,
        }
    }

    fn cache(&self, embedded: EmbeddedThumbnailReaderFn) -> ThumbnailCache {
        ThumbnailCache::new(self.services(), self.options(FAKE_DIR, embedded)).expect("valid options")
    }
}

/// Waits for a request and returns its image (`None` = no thumbnail).
fn settle(request: ThumbnailRequest) -> Option<ThumbnailImage> {
    request.wait().expect("the load succeeds")
}

/// The shared load of a request that must be pending.
fn pending(request: ThumbnailRequest) -> PendingThumbnail {
    match request {
        ThumbnailRequest::Pending(pending) => pending,
        other => panic!("expected a shared load, got {other:?}"),
    }
}

/// Lowercase hex of `bytes`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decodes standard base64 (padding and white space are skipped); used for the inline PNG inputs of the disk-decode golden.
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

// ---- ThumbnailCacheTests.cs ---------------------------------------------------------------------------------------

#[test]
fn constructor_validates_arguments() {
    // C#: ThumbnailCacheTests.ConstructorValidatesArguments (add the -1 cases)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    for ram in [0_i64, -1] {
        let mut options = fx.options(FAKE_DIR, unused_embedded());
        options.max_ram_bytes = ram;
        let error = ThumbnailCache::new(fx.services(), options).expect_err("a non-positive RAM limit is refused");
        assert_eq!(error, ThumbnailCacheConfigError::MaxRamBytes(ram));
    }
    for disk in [0_i64, -1] {
        let mut options = fx.options(FAKE_DIR, unused_embedded());
        options.max_disk_bytes = disk;
        let error = ThumbnailCache::new(fx.services(), options).expect_err("a non-positive disk limit is refused");
        assert_eq!(error, ThumbnailCacheConfigError::MaxDiskBytes(disk));
    }
}

#[test]
fn constructor_rejects_blank_disk_directory() {
    // Rust-only (contract: Wp06BridgeSplitTests and the DiskCacheStore ctor ArgumentException, checked after the limits)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    for blank in ["  ", ""] {
        let options = ThumbnailCacheOptions {
            log: fx.log.clone(),
            ..ThumbnailCacheOptions::new(blank)
        };
        let error = ThumbnailCache::new(fx.services(), options).expect_err("a blank directory is refused");
        assert_eq!(error, ThumbnailCacheConfigError::BlankDiskDirectory);
    }
}

#[test]
fn get_async_returns_decoded_image_and_caches_in_ram() {
    // C#: ThumbnailCacheTests.GetAsyncReturnsDecodedImageAndCachesInRam (Assert.Same -> Arc::ptr_eq)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);

    let first = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");
    let second = settle(cache.get(SOURCE, None).expect("valid path")).expect("cached thumbnail");

    assert!(Arc::ptr_eq(&first, &second), "the second get is the RAM entry");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the embedded reader ran once");
}

#[test]
fn get_async_serves_an_existing_disk_cache_entry() {
    // C#: ThumbnailCacheTests.GetAsyncServesAnExistingDiskCacheEntry
    // The entry name is built from a FRESH stat of the source; the disk decode (fake WIC) yields width 1.
    let reader = FakeCacheReader::returning(
        1,
        1,
        CacheImageInfo {
            source_width: 1,
            source_height: 1,
            downscaled: false,
        },
    );
    let fx = Fixture::new(reader);
    let stat = fx.fs.peek_stat(SOURCE).expect("the source exists");
    let entry = thumbnail_cache_file(Path::new(FAKE_DIR), SOURCE, stat);
    fx.fs.add_file(&entry, b"cached-png");
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);

    let image = settle(cache.get(SOURCE, None).expect("valid path")).expect("served from disk");

    assert_eq!(image.pixel_width(), 1, "the width comes from the fake disk decode");
    assert_eq!(calls.load(Ordering::SeqCst), 0, "the embedded reader is not called");
    assert_eq!(fx.reader.call_count(), 1, "the disk entry was decoded once");
}

#[test]
fn clear_memory_drops_ram_cache() {
    // C#: ThumbnailCacheTests.ClearMemoryDropsRamCache
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    let before = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");

    cache.clear_memory();
    let after = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");

    assert!(!Arc::ptr_eq(&before, &after), "the RAM entry is gone");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "the second get reads again");
}

#[test]
fn get_async_returns_embedded_thumbnail_on_miss() {
    // C#: ThumbnailCacheTests.GetAsyncReturnsEmbeddedThumbnailOnMiss (fake reader; native twin below)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, _calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);

    let image = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");

    assert_eq!((image.pixel_width(), image.pixel_height()), (16, 16));
    assert_eq!((image.original_width(), image.original_height()), (64, 64));
    assert!(image.downscaled());
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

#[cfg(windows)]
#[test]
fn corrupt_cached_thumbnail_that_cannot_be_deleted_logs_the_failed_delete() {
    // C#: ThumbnailCacheTests.CorruptCachedThumbnailThatCannotBeDeleted_LogsTheFailedDelete
    // The cache is built BEFORE the entry is written, as in C#. A real entry is held open with FileShare.Read only, so
    // the delete after the failed decode fails. The reader fails with a COM error, a cache-entry failure.
    use std::os::windows::fs::OpenOptionsExt;

    let root = TempRoot::new("corrupt-undeletable");
    let source = root.join("a.jpg");
    fs::write(&source, SOURCE_BYTES).expect("write source");
    let thumbs = root.join("thumbs");
    fs::create_dir_all(&thumbs).expect("create thumbs");
    let log = Arc::new(RecordingLog::new());
    let reader = Arc::new(FakeCacheReader::failing(|| DecodeError::Com {
        hresult: WINCODEC_ERR_BADHEADER,
        message: "corrupt".to_owned(),
    }));
    let embedded_calls = Arc::new(AtomicUsize::new(0));
    let embedded_counter = Arc::clone(&embedded_calls);
    let embedded: EmbeddedThumbnailReaderFn = Arc::new(move |_: &str, _: &CancelToken| {
        embedded_counter.fetch_add(1, Ordering::SeqCst);
        Ok(Some(decoded_image(16, 16, (64, 64))))
    });
    let physical: Arc<dyn FileSystem> = Arc::new(PhysicalFileSystem);
    let services = ThumbnailCacheServices {
        platform_codec: Arc::new(TagCodec::default()),
        cache_image_reader: reader.clone(),
        embedded_backend: Arc::new(NoThumbnailBackend),
        fs: physical,
    };
    let options = ThumbnailCacheOptions {
        log: log.clone(),
        embedded_reader: Some(embedded),
        ..default_reader_options(&thumbs, log.clone())
    };
    let cache = ThumbnailCache::new(services, options).expect("valid options");

    let source_text = source.to_str().expect("utf-8 path");
    let entry = thumbnail_cache_file(&thumbs, source_text, physical_stat(&source));
    fs::write(&entry, b"corrupt png").expect("write corrupt entry");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1) // FILE_SHARE_READ only: no delete sharing (C# FileShare.Read)
        .open(&entry)
        .expect("hold the entry open");

    let image = settle(cache.get(source_text, None).expect("valid path"));

    assert!(
        log.contains(LogLevel::Error, "Disk thumbnail read failed: "),
        "the corrupt entry is logged"
    );
    assert!(
        log.contains(LogLevel::Error, "Delete failed"),
        "the undeletable entry is logged"
    );
    assert_eq!(reader.call_count(), 1, "the disk decode ran once");
    assert_eq!(
        embedded_calls.load(Ordering::SeqCst),
        1,
        "the load fell through to the embedded reader"
    );
    assert!(image.is_some(), "the fall-through still returns a thumbnail");
    drop(held);
}

#[test]
fn get_async_never_performs_a_full_source_decode_on_miss() {
    // C#: ThumbnailCacheTests.GetAsyncNeverPerformsAFullSourceDecodeOnMiss
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(None));
    let cache = fx.cache(embedded);

    let image = settle(cache.get(SOURCE, None).expect("valid path"));

    assert!(image.is_none(), "no embedded thumbnail, no full decode");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(fx.reader.call_count(), 0, "no disk entry was decoded");
    assert_eq!(
        fx.codec.images.load(Ordering::SeqCst),
        0,
        "no full-source image was made"
    );
}

#[test]
fn get_async_returns_null_when_source_has_no_embedded_thumbnail() {
    // C#: ThumbnailCacheTests.GetAsyncReturnsNullWhenSourceHasNoEmbeddedThumbnail (fake reader; native twin below)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, _calls) = counting_embedded(|_, _, _| Ok(None));
    let cache = fx.cache(embedded);

    let image = settle(cache.get(SOURCE, None).expect("valid path"));

    assert!(image.is_none());
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
fn get_async_returns_null_for_non_jpeg_source() {
    // C#: ThumbnailCacheTests.GetAsyncReturnsNullForNonJpegSource (fake reader; native twin below)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    fx.fs.add_file(r"C:\Photos\a.png", b"png");
    let (embedded, _calls) = counting_embedded(|_, _, _| Ok(None));
    let cache = fx.cache(embedded);

    let image = settle(cache.get(r"C:\Photos\a.png", None).expect("valid path"));

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
fn get_async_with_known_stat_keys_by_that_version() {
    // C#: ThumbnailCacheTests.GetAsyncWithKnownStat_KeysByThatVersion
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let stat = fx.fs.peek_stat(SOURCE).expect("the source exists");
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);

    let first = settle(cache.get(SOURCE, Some(stat)).expect("valid path")).expect("has a thumbnail");
    let again = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");
    assert!(Arc::ptr_eq(&first, &again), "the same version is one RAM entry");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let changed = FileStat {
        length: stat.length + 1,
        last_write_utc: stat
            .last_write_utc
            .checked_add_ticks(10_000_000)
            .expect("one second later is in range"),
    };
    let other = settle(cache.get(SOURCE, Some(changed)).expect("valid path")).expect("has a thumbnail");
    assert!(!Arc::ptr_eq(&first, &other), "a new version is another key");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn thumbnail_get_missing_source_returns_faulted_task() {
    // C#: ErrorHandlingReviewCachingTests.ThumbnailGet_MissingSource_ReturnsFaultedTask
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let cache = fx.cache(unused_embedded());

    let request = cache
        .get(r"C:\Photos\missing.jpg", None)
        .expect("a missing file is not a synchronous error");

    match request {
        ThumbnailRequest::Ready(Err(ThumbnailError::Stat(error))) => {
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("expected Ready(Err(Stat(NotFound))), got {other:?}"),
    }
}

// ---- Robustness/EmbeddedThumbnailMutationTests.cs -----------------------------------------------------------------

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
fn mutated_cached_png_is_regenerated_not_thrown_fake_reader() {
    // C#: Robustness/EmbeddedThumbnailMutationTests.MutatedCachedPng_IsRegeneratedNotThrown (non-native twin: every
    // DecodeError that is a cache-entry failure makes the load delete the entry and fall through to the embedded reader)
    let makers: [NamedDecodeError; 4] = [
        ("InvalidData", || DecodeError::InvalidData {
            message: "Thumbnail cache entry has an invalid length.".to_owned(),
        }),
        ("Com", || DecodeError::Com {
            hresult: WINCODEC_ERR_BADHEADER,
            message: "corrupt".to_owned(),
        }),
        ("FileNotFound", || DecodeError::FileNotFound {
            message: "missing".to_owned(),
        }),
        ("Io(UnexpectedEof)", || {
            DecodeError::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "short read"))
        }),
    ];
    for (label, make) in makers {
        assert!(is_cache_entry_failure(&make()), "{label} is a cache-entry failure");

        let root = TempRoot::new("mutated-fake");
        let source = root.join("a.jpg");
        fs::write(&source, SOURCE_BYTES).expect("write source");
        let thumbs = root.join("thumbs");
        fs::create_dir_all(&thumbs).expect("create thumbs");
        let source_text = source.to_str().expect("utf-8 path");
        let entry = thumbnail_cache_file(&thumbs, source_text, physical_stat(&source));
        fs::write(&entry, b"mutated").expect("write the mutant");

        let log = Arc::new(RecordingLog::new());
        let reader = Arc::new(FakeCacheReader::failing(make));
        let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (16, 16)))));
        let services = ThumbnailCacheServices {
            platform_codec: Arc::new(TagCodec::default()),
            cache_image_reader: reader.clone(),
            embedded_backend: Arc::new(NoThumbnailBackend),
            fs: Arc::new(PhysicalFileSystem),
        };
        let options = ThumbnailCacheOptions {
            log: log.clone(),
            embedded_reader: Some(embedded),
            ..default_reader_options(&thumbs, log)
        };
        let cache = ThumbnailCache::new(services, options).expect("valid options");

        let image = settle(cache.get(source_text, None).expect("valid path"));

        assert!(
            image.is_some(),
            "{label}: regenerated from the embedded reader, not thrown"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{label}: the embedded reader ran");
        assert!(!entry.exists(), "{label}: the damaged entry was deleted");
    }
}

// ---- ThumbnailCacheLifecycleTests.cs ------------------------------------------------------------------------------

#[test]
fn dispose_during_load_ends_cleanly() {
    // C#: ThumbnailCacheLifecycleTests.DisposeDuringLoad_EndsCleanly
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, _calls, gate) = gate_first_call(|_| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    let pending = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");

    cache.dispose();
    cache.dispose();
    gate.release.send(()).expect("the load still waits for the release");

    let image = pending
        .wait()
        .expect("the in-flight load still delivers")
        .expect("has a thumbnail");
    assert_eq!(image.pixel_width(), 16);
    assert!(matches!(cache.get(SOURCE, None), Err(ThumbnailError::Disposed)));
}

#[test]
fn caller_cancellation_does_not_poison_the_shared_load() {
    // C#: ThumbnailCacheLifecycleTests.CallerCancellation_DoesNotPoisonTheSharedLoad (cancel = drop the handle)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls, gate) = gate_first_call(|_| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    let first = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");
    let second = pending(cache.get(SOURCE, None).expect("valid path"));

    drop(first);
    gate.release.send(()).expect("release the load");

    let image = second
        .wait()
        .expect("the second caller still gets the image")
        .expect("has a thumbnail");
    assert_eq!(image.pixel_width(), 16);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the load ran once for both callers");
}

#[test]
fn failed_load_is_not_cached() {
    // C#: ThumbnailCacheLifecycleTests.FailedLoad_IsNotCached
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls) = counting_embedded(|index, _, _| {
        if index == 0 {
            Err(ThumbnailError::Reader(Arc::new(io::Error::other("transient"))))
        } else {
            Ok(Some(decoded_image(16, 16, (64, 64))))
        }
    });
    let cache = fx.cache(embedded);

    let first = cache.get(SOURCE, None).expect("valid path").wait();
    assert!(matches!(first, Err(ThumbnailError::Reader(_))), "the first load fails");
    let second = settle(cache.get(SOURCE, None).expect("valid path"));

    assert!(second.is_some(), "the second get starts a new load");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn raw_source_skips_the_embedded_thumbnail_reader() {
    // C#: ThumbnailCacheLifecycleTests.RawSource_SkipsTheEmbeddedThumbnailReader (one loop case per RAW extension)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    for name in [r"C:\Photos\a.cr2", r"C:\Photos\a.DNG", r"C:\Photos\a.raf"] {
        fx.fs.add_file(name, b"raw");
        let image = settle(cache.get(name, None).expect("valid path"));
        assert!(
            image.is_none(),
            "{name}: a RAW source has no thumbnail through this path"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "{name}: the embedded reader is not called"
        );
    }
}

// ---- ThumbnailCacheMutationTests.cs -------------------------------------------------------------------------------

#[test]
fn clear_memory_during_load_result_is_not_cached() {
    // C#: ThumbnailCacheMutationTests.ClearMemoryDuringLoad_ResultIsNotCached (reads 2 then still 2)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls, gate) = gate_first_call(|_| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    let first = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");

    cache.clear_memory();
    gate.release.send(()).expect("release the load");
    let returned = first
        .wait()
        .expect("the caller still gets its result")
        .expect("has a thumbnail");

    let second = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");
    assert!(
        !Arc::ptr_eq(&returned, &second),
        "the load that ended during the clear was not cached"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let third = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");
    assert!(Arc::ptr_eq(&second, &third), "the new load is cached");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "still 2 reads");
}

#[test]
fn clear_disk_removes_thumbnail_files() {
    // C#: ThumbnailCacheMutationTests.ClearDisk_RemovesThumbnailFiles
    let root = TempRoot::new("clear-disk");
    fs::write(root.join("a.png"), b"thumb").expect("write a.png");
    fs::write(root.join("b.txt"), b"keep").expect("write b.txt");
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let cache = ThumbnailCache::new(fx.services(), fx.options(root.path(), unused_embedded())).expect("valid options");

    cache.clear_disk();

    assert!(!root.join("a.png").exists(), "the thumbnail file is removed");
    assert!(root.join("b.txt").exists(), "a non-thumbnail file is kept");
}

#[cfg(windows)]
#[test]
#[ignore = "native: changes a real ACL (Deny ListDirectory)"]
fn clear_disk_unlistable_directory_logs_and_does_not_throw() {
    // C#: ThumbnailCacheMutationTests.ClearDisk_UnlistableDirectory_LogsAndDoesNotThrow
    // A deny ACE on Everyone (SID S-1-1-0, locale independent) for ListDirectory is set with icacls and removed by a guard.
    struct DenyListDirectory<'a> {
        path: &'a Path,
    }

    impl<'a> DenyListDirectory<'a> {
        fn apply(path: &'a Path) -> Self {
            let status = std::process::Command::new("icacls")
                .arg(path)
                .args(["/deny", "*S-1-1-0:(RD)"])
                .status()
                .expect("run icacls");
            assert!(status.success(), "icacls adds the deny ACE");
            Self { path }
        }
    }

    impl Drop for DenyListDirectory<'_> {
        fn drop(&mut self) {
            let _ = std::process::Command::new("icacls")
                .arg(self.path)
                .args(["/remove:d", "*S-1-1-0"])
                .status();
        }
    }

    let root = TempRoot::new("acl-clear");
    fs::write(root.join("a.png"), b"thumb").expect("write a.png");
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let cache = ThumbnailCache::new(fx.services(), fx.options(root.path(), unused_embedded())).expect("valid options");
    assert!(
        cache.wait_startup_cleanup(Duration::from_secs(10)),
        "the start-up sweep finishes first"
    );

    let _deny = DenyListDirectory::apply(root.path());
    cache.clear_disk();

    assert!(
        fx.log.contains(LogLevel::Error, "Thumbnail disk cache clear failed"),
        "the failed clear is logged, not thrown"
    );
}

// ---- Startup, disk probe and temp-file tests ----------------------------------------------------------------------

#[test]
fn thumbnail_startup_deletes_stale_temp_files() {
    // C#: CacheTempFileAndDiskProbeTests.ThumbnailStartup_DeletesStaleTempFiles
    let root = TempRoot::new("startup-temp");
    let stale = root.join("a.png.6f1c2b1e-0000-4000-8000-000000000001.tmp");
    let fresh = root.join("b.png.6f1c2b1e-0000-4000-8000-000000000002.tmp");
    fs::write(&stale, b"partial").expect("write stale temp");
    fs::write(&fresh, b"partial").expect("write fresh temp");
    File::options()
        .write(true)
        .open(&stale)
        .expect("open stale temp")
        .set_modified(SystemTime::now() - Duration::from_secs(2 * 60 * 60))
        .expect("age the stale temp by two hours");

    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let options = ThumbnailCacheOptions {
        clock: Arc::new(SystemClock),
        ..fx.options(root.path(), unused_embedded())
    };
    let cache = ThumbnailCache::new(fx.services(), options).expect("valid options");

    assert!(
        cache.wait_startup_cleanup(Duration::from_secs(10)),
        "the start-up sweep finishes"
    );
    assert!(!stale.exists(), "the two-hour-old temp file is deleted");
    assert!(fresh.exists(), "the fresh temp file is kept");
}

#[cfg(windows)]
#[test]
#[ignore = "native: holds a real file open without sharing"]
fn clear_disk_tolerates_locked_file() {
    // C#: ThumbnailCacheDiskTests.ClearDiskToleratesLockedFile
    use std::os::windows::fs::OpenOptionsExt;

    let root = TempRoot::new("locked-clear");
    fs::write(root.join("free.png"), b"free").expect("write free.png");
    fs::write(root.join("locked.png"), b"locked").expect("write locked.png");
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0) // no sharing at all
        .open(root.join("locked.png"))
        .expect("hold the locked file");
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let cache = ThumbnailCache::new(fx.services(), fx.options(root.path(), unused_embedded())).expect("valid options");

    cache.clear_disk();

    assert!(!root.join("free.png").exists(), "the unlocked file is removed");
    assert!(root.join("locked.png").exists(), "the locked file is kept");
    drop(locked);
}

// ---- Wp06BridgeSplitTests.cs and the Rust-only lifecycle tests ----------------------------------------------------

#[test]
fn thumbnail_cache_embedded_thumbnail_uses_the_injected_codec() {
    // C#: Pixels/Wp06BridgeSplitTests.ThumbnailCache_EmbeddedThumbnail_UsesTheInjectedCodec
    // Default reader + a fake W4-02b backend; the platform image is the injected codec's Opaque image.
    let root = TempRoot::new("wp06");
    let fs_fake = Arc::new(FakeFileSystem::new());
    fs_fake.add_file(SOURCE, b"jpeg-with-exif-thumbnail");
    let codec = Arc::new(TagCodec::default());
    let services = ThumbnailCacheServices {
        platform_codec: codec.clone(),
        cache_image_reader: Arc::new(FakeCacheReader::failing(|| DecodeError::Canceled)),
        embedded_backend: Arc::new(FakeEmbeddedBackend {
            main: (64, 64),
            thumbnail: (16, 16),
        }),
        fs: fs_fake.clone(),
    };
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    let cache = ThumbnailCache::new(services, default_reader_options(root.path(), log)).expect("valid options");

    let image = settle(cache.get(SOURCE, None).expect("valid path")).expect("has an embedded thumbnail");

    assert_eq!((image.pixel_width(), image.pixel_height()), (16, 16));
    assert!(
        matches!(image.platform_image(), PlatformImage::Opaque(tag) if tag.downcast_ref::<CodecTag>().is_some()),
        "the platform image came from the injected codec"
    );
    assert_eq!(codec.images.load(Ordering::SeqCst), 1);
}

#[test]
fn get_blank_path_is_rejected_before_disposed_check() {
    // Rust-only: semantics 2a (C# ArgumentException.ThrowIfNullOrWhiteSpace happens before the disposed check)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let cache = fx.cache(unused_embedded());

    assert!(matches!(cache.get("   ", None), Err(ThumbnailError::BlankPath)));
    cache.dispose();
    assert!(
        matches!(cache.get("   ", None), Err(ThumbnailError::BlankPath)),
        "blank beats disposed"
    );
    assert!(matches!(cache.get(SOURCE, None), Err(ThumbnailError::Disposed)));
}

#[test]
fn pending_wait_timeout_and_try_result_do_not_consume_the_outcome() {
    // Rust-only: semantics 5 (wait_timeout and try_result observe without consuming)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, _calls, gate) = gate_first_call(|_| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    let pending = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");

    assert!(pending.try_result().is_none(), "the load is still running");
    assert!(
        pending.wait_timeout(Duration::ZERO).is_none(),
        "a zero timeout gives up at once"
    );

    gate.release.send(()).expect("release the load");
    let first = pending
        .wait_timeout(Duration::from_secs(10))
        .expect("the load ends")
        .expect("load succeeds")
        .expect("has a thumbnail");
    let second = pending
        .try_result()
        .expect("the outcome is still observable")
        .expect("load succeeds")
        .expect("has a thumbnail");
    assert!(Arc::ptr_eq(&first, &second), "every observation sees the same image");
    let third = pending
        .wait()
        .expect("the final wait succeeds")
        .expect("has a thumbnail");
    assert!(Arc::ptr_eq(&first, &third));
}

#[test]
fn dropped_caller_then_load_fails_removes_in_flight_entry() {
    // C#: RV-I10 (replaced). Rust has no unobserved task exceptions; drop the only handle, let the reader fail, then a
    // new get must start a NEW load (reader calls 2) and nothing panics.
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls, gate) = gate_first_call(|index| {
        if index == 0 {
            Err(ThumbnailError::Reader(Arc::new(io::Error::other("transient"))))
        } else {
            Ok(Some(decoded_image(16, 16, (64, 64))))
        }
    });
    let cache = fx.cache(embedded);
    let only = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");

    drop(only);
    gate.release.send(()).expect("release the failing load");

    let retry = settle(cache.get(SOURCE, None).expect("valid path"));
    assert!(
        retry.is_some(),
        "the failed in-flight entry is gone, a new load succeeds"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2, "the second get started a new load");
}

// ---- Rust-only adversarial checks (finish pass): INV-2, perf, error classes, seams -----------------------------------

#[test]
fn clear_disk_during_load_result_is_not_cached() {
    // Rust-only: semantics 7 (ClearDisk bumps the generation like ClearMemory; INV-2)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls, gate) = gate_first_call(|_| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    let first = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");

    cache.clear_disk();
    gate.release.send(()).expect("release the load");
    let returned = first
        .wait()
        .expect("the caller still gets its result")
        .expect("has a thumbnail");

    let second = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");
    assert!(
        !Arc::ptr_eq(&returned, &second),
        "a load that ended across clear_disk is not cached"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn joiner_after_clear_memory_caches_the_shared_result() {
    // Rust-only: semantics 2g parity (C# reads the generation per caller after GetOrAdd: the starter's predates the
    // clear, a joiner's does not, so the joiner caches the shared result)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls, gate) = gate_first_call(|_| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    let starter = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");
    cache.clear_memory();
    let joiner = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.release.send(()).expect("release the load");

    let from_starter = starter.wait().expect("load succeeds").expect("has a thumbnail");
    let from_joiner = joiner.wait().expect("load succeeds").expect("has a thumbnail");
    assert!(Arc::ptr_eq(&from_starter, &from_joiner), "one shared load");
    let again = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");
    assert!(Arc::ptr_eq(&from_joiner, &again), "the joiner cached the shared result");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn dropping_one_of_two_handles_keeps_the_shared_load_joinable() {
    // Rust-only: C# keeps the Lazy in _inFlight while any caller waits; a third caller joins the same load
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls, gate) = gate_first_call(|_| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);
    let first = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");
    let second = pending(cache.get(SOURCE, None).expect("valid path"));

    drop(first);
    let third = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.release.send(()).expect("release the load");

    let a = second.wait().expect("load succeeds").expect("has a thumbnail");
    let b = third.wait().expect("load succeeds").expect("has a thumbnail");
    assert!(Arc::ptr_eq(&a, &b), "the third caller joined the running load");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one load for every caller");
}

/// A spawner that runs each job on a thread and reports on `done` once the job (and its captures) is gone.
fn reporting_spawner() -> (LoadSpawner, Receiver<()>) {
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let done_tx = Mutex::new(done_tx);
    let spawner: LoadSpawner = Arc::new(move |job: Box<dyn FnOnce() + Send + 'static>| {
        let done = done_tx.lock().expect("done sender lock").clone();
        std::thread::spawn(move || {
            job();
            let _ = done.send(());
        });
        Ok(())
    });
    (spawner, done_rx)
}

#[test]
fn dispose_releases_the_ram_cache() {
    // Rust-only: semantics 8 (Dispose clears the RAM cache under the lock)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, _calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let (spawner, done) = reporting_spawner();
    let options = ThumbnailCacheOptions {
        spawner: Some(spawner),
        ..fx.options(FAKE_DIR, embedded)
    };
    let cache = ThumbnailCache::new(fx.services(), options).expect("valid options");
    let image = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");
    done.recv().expect("the start-up sweep job ended");
    done.recv().expect("the load job ended");
    assert_eq!(
        Arc::strong_count(&image),
        2,
        "the test and the RAM cache hold the image"
    );

    cache.dispose();

    assert_eq!(Arc::strong_count(&image), 1, "dispose dropped the RAM entry");
}

#[test]
fn dispose_cancels_the_token_of_running_loads() {
    // Rust-only: semantics 8 (Dispose cancels the dispose token the running load was given)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let started_tx = Mutex::new(started_tx);
    let release_rx = Mutex::new(release_rx);
    let embedded: EmbeddedThumbnailReaderFn = Arc::new(move |_: &str, token: &CancelToken| {
        started_tx.lock().expect("lock").send(()).expect("the test waits");
        release_rx.lock().expect("lock").recv().expect("the test releases");
        if token.is_canceled() {
            Err(ThumbnailError::Canceled)
        } else {
            Ok(Some(decoded_image(16, 16, (64, 64))))
        }
    });
    let cache = fx.cache(embedded);
    let load = pending(cache.get(SOURCE, None).expect("valid path"));
    started_rx.recv().expect("the load started");

    cache.dispose();
    release_tx.send(()).expect("release the load");

    assert!(
        matches!(load.wait(), Err(ThumbnailError::Canceled)),
        "the reader saw the cancelled token"
    );
}

#[test]
fn panicking_reader_reports_panicked_and_is_not_cached() {
    // Rust-only: semantics 3 (a panic in the load is Err(Panicked) for every waiter; the next get starts a new load)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls) = counting_embedded(|index, _, _| {
        if index == 0 {
            panic!("boom in the reader");
        }
        Ok(Some(decoded_image(16, 16, (64, 64))))
    });
    let cache = fx.cache(embedded);

    // Bounded: a panic that escaped the job would leave the load without an outcome.
    let first = pending(cache.get(SOURCE, None).expect("valid path"))
        .wait_timeout(Duration::from_secs(10))
        .expect("the panicking load still ends");
    match first {
        Err(ThumbnailError::Panicked(message)) => assert!(message.contains("boom in the reader"), "{message}"),
        other => panic!("expected Err(Panicked), got {:?}", other.map(|image| image.is_some())),
    }
    let retry = settle(cache.get(SOURCE, None).expect("valid path"));
    assert!(retry.is_some(), "the failed load was forgotten");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn failed_spawn_reports_spawn_error_and_is_not_cached() {
    // Rust-only: semantics 3 and 9 (a spawn failure publishes Err(Spawn) and forgets the entry; the start-up sweep's
    // spawn failure is logged and its signal is done, OQ8)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let spawns = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&spawns);
    let spawner: LoadSpawner = Arc::new(move |_job: Box<dyn FnOnce() + Send + 'static>| {
        counter.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::other("no threads left"))
    });
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let options = ThumbnailCacheOptions {
        spawner: Some(spawner),
        ..fx.options(FAKE_DIR, embedded)
    };
    let cache = ThumbnailCache::new(fx.services(), options).expect("valid options");
    assert!(
        cache.wait_startup_cleanup(Duration::ZERO),
        "a sweep that could not start is done"
    );
    assert!(fx.log.contains(LogLevel::Error, "Stale temp file cleanup failed: "));

    for attempt in 0..2 {
        // Bounded: an unpublished spawn failure would leave the load without an outcome.
        let result = pending(cache.get(SOURCE, None).expect("valid path"))
            .wait_timeout(Duration::from_secs(10))
            .expect("a load that could not start still ends");
        assert!(
            matches!(result, Err(ThumbnailError::Spawn(_))),
            "attempt {attempt}: Err(Spawn)"
        );
    }
    assert_eq!(spawns.load(Ordering::SeqCst), 3, "the sweep, then one new load per get");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn get_normalizes_the_path_before_keying() {
    // Rust-only: semantics 1/2d (Path.GetFullPath before the key: `/` and `.` segments name the same entry)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = fx.cache(embedded);

    let first = settle(cache.get(SOURCE, None).expect("valid path")).expect("has a thumbnail");
    let second = settle(cache.get("C:/Photos/./x/../a.jpg", None).expect("valid path")).expect("has a thumbnail");

    assert!(Arc::ptr_eq(&first, &second), "the normalized path is the same key");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn get_invalid_or_directory_path_is_a_faulted_request() {
    // Rust-only: semantics 2d/2e (GetFullPath failure and a directory are faulted requests, not synchronous errors)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    fx.fs.add_dir(r"C:\Photos\folder.jpg");
    let cache = fx.cache(unused_embedded());

    let invalid = cache.get("C:\\Photos\\a\0.jpg", None).expect("not a synchronous error");
    assert!(
        matches!(
            invalid,
            ThumbnailRequest::Ready(Err(ThumbnailError::InvalidPath(PathError::Argument)))
        ),
        "got {invalid:?}"
    );
    match cache
        .get(r"C:\Photos\folder.jpg", None)
        .expect("not a synchronous error")
    {
        ThumbnailRequest::Ready(Err(ThumbnailError::Stat(error))) => assert_eq!(error.kind(), io::ErrorKind::NotFound),
        other => panic!("expected Ready(Err(Stat(NotFound))), got {other:?}"),
    }
}

#[test]
fn get_rejects_every_white_space_path() {
    // Rust-only: semantics 2a (C# ThrowIfNullOrWhiteSpace uses char.IsWhiteSpace, not only ' ')
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let cache = fx.cache(unused_embedded());
    for blank in ["", " ", "\t\r\n", "\u{3000}\u{a0}"] {
        assert!(
            matches!(cache.get(blank, None), Err(ThumbnailError::BlankPath)),
            "{blank:?}"
        );
    }
}

#[test]
fn disk_hit_notes_the_access_with_the_injected_clock() {
    // Rust-only: semantics 4 (a disk hit calls DiskCacheStore::note_accessed, which stamps the last-access time)
    let root = TempRoot::new("note-accessed");
    let source = root.join("a.jpg");
    fs::write(&source, SOURCE_BYTES).expect("write source");
    let thumbs = root.join("thumbs");
    fs::create_dir_all(&thumbs).expect("create thumbs");
    let source_text = source.to_str().expect("utf-8 path");
    let entry = thumbnail_cache_file(&thumbs, source_text, physical_stat(&source));
    fs::write(&entry, b"cached-png").expect("write entry");
    // 2020-01-01T00:00:00Z: far from the real access time the write just set.
    let stamp = DateTimeUtc::from_ticks(637_134_336_000_000_000).expect("in range");
    let reader = Arc::new(FakeCacheReader::returning(
        1,
        1,
        CacheImageInfo {
            source_width: 1,
            source_height: 1,
            downscaled: false,
        },
    ));
    let services = ThumbnailCacheServices {
        platform_codec: Arc::new(TagCodec::default()),
        cache_image_reader: reader.clone(),
        embedded_backend: Arc::new(NoThumbnailBackend),
        fs: Arc::new(PhysicalFileSystem),
    };
    let log: Arc<dyn Log> = Arc::new(RecordingLog::new());
    let options = ThumbnailCacheOptions {
        clock: Arc::new(FakeClock::at(stamp)),
        embedded_reader: Some(unused_embedded()),
        ..default_reader_options(&thumbs, log)
    };
    let cache = ThumbnailCache::new(services, options).expect("valid options");

    let image = settle(cache.get(source_text, None).expect("valid path"));

    assert!(image.is_some(), "served from disk");
    let accessed = fs::metadata(&entry).expect("entry kept").accessed().expect("atime");
    assert_eq!(
        DateTimeUtc::from_system_time(accessed).map(DateTimeUtc::ticks),
        Some(stamp.ticks()),
        "the hit stamped the entry's last-access time with the injected clock"
    );
}

#[test]
fn non_cache_entry_disk_failure_propagates_and_keeps_the_entry() {
    // Rust-only: semantics 4 / trap 7.9 (out of memory is not a damaged entry: no delete, no fall-through)
    let root = TempRoot::new("disk-oom");
    let source = root.join("a.jpg");
    fs::write(&source, SOURCE_BYTES).expect("write source");
    let thumbs = root.join("thumbs");
    fs::create_dir_all(&thumbs).expect("create thumbs");
    let source_text = source.to_str().expect("utf-8 path");
    let entry = thumbnail_cache_file(&thumbs, source_text, physical_stat(&source));
    fs::write(&entry, b"cached-png").expect("write entry");
    let makers: [NamedDecodeError; 2] = [
        ("OutOfMemory", || DecodeError::OutOfMemory {
            message: "Not enough memory to decode the cache entry.".to_owned(),
        }),
        ("Canceled", || DecodeError::Canceled),
    ];
    for (label, make) in makers {
        assert!(!is_cache_entry_failure(&make()), "{label} is not a cache-entry failure");
        let log = Arc::new(RecordingLog::new());
        let reader = Arc::new(FakeCacheReader::failing(make));
        let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (16, 16)))));
        let services = ThumbnailCacheServices {
            platform_codec: Arc::new(TagCodec::default()),
            cache_image_reader: reader.clone(),
            embedded_backend: Arc::new(NoThumbnailBackend),
            fs: Arc::new(PhysicalFileSystem),
        };
        let options = ThumbnailCacheOptions {
            log: log.clone(),
            embedded_reader: Some(embedded),
            ..default_reader_options(&thumbs, log.clone())
        };
        let cache = ThumbnailCache::new(services, options).expect("valid options");

        let result = cache.get(source_text, None).expect("valid path").wait();

        assert!(
            matches!(result, Err(ThumbnailError::Decode(_))),
            "{label}: propagated as Decode"
        );
        assert!(entry.exists(), "{label}: the entry is kept");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "{label}: no fall-through to the embedded reader"
        );
        assert!(
            !log.contains(LogLevel::Error, "Disk thumbnail read failed"),
            "{label}: not logged as damaged"
        );
    }
}

#[test]
fn decode_disk_thumbnail_refuses_an_empty_entry_and_passes_the_exact_bytes() {
    // Rust-only: semantics 6 (length 0 is InvalidData before the decode; otherwise the whole file reaches the reader)
    let fake_fs = FakeFileSystem::new();
    fake_fs.add_file(r"C:\thumbs\empty.png", b"");
    fake_fs.add_file(r"C:\thumbs\full.png", b"png-bytes-0123456789");
    let codec = TagCodec::default();
    let reader = FakeCacheReader::new(|_, encoded| {
        assert_eq!(encoded, b"png-bytes-0123456789", "the reader gets the whole entry");
        let pixels = PixelBuffer::allocate(2, 3, PixelLayout::Pbgra32).expect("valid size");
        Ok((
            pixels,
            CacheImageInfo {
                source_width: 2,
                source_height: 3,
                downscaled: false,
            },
        ))
    });

    match decode_disk_thumbnail(Path::new(r"C:\thumbs\empty.png"), &fake_fs, &reader, &codec) {
        Err(DecodeError::InvalidData { message }) => {
            assert_eq!(message, "Thumbnail cache entry has an invalid length.");
        }
        other => panic!("expected InvalidData, got {:?}", other.map(|image| image.pixel_width())),
    }
    assert_eq!(reader.call_count(), 0, "an empty entry is refused before decoding");

    let image = decode_disk_thumbnail(Path::new(r"C:\thumbs\full.png"), &fake_fs, &reader, &codec).expect("decodes");
    assert_eq!((image.pixel_width(), image.pixel_height()), (2, 3));
    assert_eq!(
        codec.images.load(Ordering::SeqCst),
        1,
        "the codec made the platform image"
    );
    assert!(matches!(
        decode_disk_thumbnail(Path::new(r"C:\thumbs\missing.png"), &fake_fs, &reader, &codec),
        Err(DecodeError::FileNotFound { .. })
    ));
}

#[test]
fn abandoned_load_is_forgotten_before_it_ends() {
    // Rust-only (DIVERGENCE in PendingThumbnail::drop): once the last handle is dropped, a new get starts a new load
    // at once instead of joining the abandoned one (deterministic twin of the RV-I10 replacement)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls, gate) = gate_first_call(|index| {
        if index == 0 {
            Err(ThumbnailError::Reader(Arc::new(io::Error::other("transient"))))
        } else {
            Ok(Some(decoded_image(16, 16, (64, 64))))
        }
    });
    let cache = fx.cache(embedded);
    let only = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load started");

    drop(only);
    let retry = pending(cache.get(SOURCE, None).expect("valid path"));
    // Bounded: a retry that joined the abandoned load would wait for the release below forever.
    let outcome = retry.wait_timeout(Duration::from_secs(10));
    gate.release.send(()).expect("release the abandoned load");

    let image = outcome
        .expect("the retry did not join the abandoned, still-gated load")
        .expect("the new load succeeds");
    assert!(image.is_some(), "the new load did not join the abandoned, failing one");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

/// A spawner that keeps every job until the test runs it ([`HeldJobs::run_all`]).
#[derive(Default)]
struct HeldJobs {
    jobs: Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>>,
}

impl HeldJobs {
    fn spawner(self: &Arc<Self>) -> LoadSpawner {
        let held = Arc::clone(self);
        Arc::new(move |job: Box<dyn FnOnce() + Send + 'static>| {
            held.jobs.lock().expect("jobs lock").push(job);
            Ok(())
        })
    }

    fn run_all(&self) {
        let jobs = std::mem::take(&mut *self.jobs.lock().expect("jobs lock"));
        for job in jobs {
            job();
        }
    }
}

#[test]
fn load_started_after_dispose_is_canceled_before_any_read() {
    // Rust-only: semantics 4 (LoadOrCreateAsync starts with cancellationToken.ThrowIfCancellationRequested)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let held = Arc::new(HeldJobs::default());
    let options = ThumbnailCacheOptions {
        spawner: Some(held.spawner()),
        ..fx.options(FAKE_DIR, embedded)
    };
    let cache = ThumbnailCache::new(fx.services(), options).expect("valid options");
    let load = pending(cache.get(SOURCE, None).expect("valid path"));

    cache.dispose();
    held.run_all();

    assert!(matches!(load.wait(), Err(ThumbnailError::Canceled)));
    assert_eq!(calls.load(Ordering::SeqCst), 0, "the reader never ran");
}

/// A [`FakeFileSystem`] wrapper whose first `file_exists` of a `.png` path reports on `started` and waits for
/// `release` (the window between the existence check and the disk decode / embedded read).
struct GatedExistsFs {
    disk: Arc<FakeFileSystem>,
    gate: Mutex<Option<(Sender<()>, Receiver<()>)>>,
}

impl GatedExistsFs {
    fn new(disk: Arc<FakeFileSystem>) -> (Arc<Self>, Gate) {
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let fs = Arc::new(Self {
            disk,
            gate: Mutex::new(Some((started_tx, release_rx))),
        });
        (
            fs,
            Gate {
                started: started_rx,
                release: release_tx,
            },
        )
    }
}

impl FileSystem for GatedExistsFs {
    fn file_exists(&self, path: &Path) -> bool {
        if path.extension().is_some_and(|ext| ext == "png") {
            let gate = self.gate.lock().expect("gate lock").take();
            if let Some((started, release)) = gate {
                started.send(()).expect("the test waits");
                release.recv().expect("the test releases");
            }
        }
        self.disk.file_exists(path)
    }
    fn directory_exists(&self, path: &Path) -> bool {
        self.disk.directory_exists(path)
    }
    fn get_file_stat(&self, path: &Path) -> Option<FileStat> {
        self.disk.get_file_stat(path)
    }
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.disk.move_file(source, destination)
    }
    fn copy_file(&self, source: &Path, destination: &Path) -> io::Result<()> {
        self.disk.copy_file(source, destination)
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        self.disk.delete(path)
    }
    fn open_read_shared(&self, path: &Path, buffer_size: usize) -> io::Result<Box<dyn ReadSeek>> {
        self.disk.open_read_shared(path, buffer_size)
    }
    fn open_append(&self, path: &Path, durable: bool) -> io::Result<Box<dyn AppendStream>> {
        self.disk.open_append(path, durable)
    }
    fn write_all_text_atomic(&self, path: &Path, text: &str, durable: bool) -> io::Result<()> {
        self.disk.write_all_text_atomic(path, text, durable)
    }
    fn read_all_text(&self, path: &Path) -> io::Result<String> {
        self.disk.read_all_text(path)
    }
    fn enumerate_files(&self, directory: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
        self.disk.enumerate_files(directory, pattern)
    }
    fn enumerate_directories(&self, directory: &Path) -> io::Result<Vec<PathBuf>> {
        self.disk.enumerate_directories(directory)
    }
    fn create_directory(&self, path: &Path) -> io::Result<()> {
        self.disk.create_directory(path)
    }
}

#[test]
fn dispose_after_the_existence_check_cancels_the_disk_decode() {
    // Rust-only: semantics 4 (C# DecodeAsync = Task.Run(.., token): a cancelled token is Canceled, not a damaged entry;
    // the entry is neither decoded nor deleted)
    let reader = FakeCacheReader::returning(
        1,
        1,
        CacheImageInfo {
            source_width: 1,
            source_height: 1,
            downscaled: false,
        },
    );
    let fx = Fixture::new(reader);
    let stat = fx.fs.peek_stat(SOURCE).expect("the source exists");
    let entry = thumbnail_cache_file(Path::new(FAKE_DIR), SOURCE, stat);
    fx.fs.add_file(&entry, b"cached-png");
    let (gated, gate) = GatedExistsFs::new(fx.fs.clone());
    let (embedded, calls) = counting_embedded(|_, _, _| Ok(Some(decoded_image(16, 16, (64, 64)))));
    let cache = ThumbnailCache::new(fx.services_on(gated), fx.options(FAKE_DIR, embedded)).expect("valid options");
    let load = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load checks the disk entry");

    cache.dispose();
    gate.release.send(()).expect("release the load");

    assert!(matches!(load.wait(), Err(ThumbnailError::Canceled)));
    assert_eq!(fx.reader.call_count(), 0, "the disk entry was not decoded");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "no fall-through to the embedded reader"
    );
    assert!(fx.fs.file_exists(&entry), "a cancellation keeps the entry");
}

#[test]
fn default_reader_checks_the_dispose_token_before_reading() {
    // Rust-only: semantics 10 (C# DefaultReadEmbeddedThumbnailAsync: Task.Run(.., token) + ThrowIfCancellationRequested)
    let fx = Fixture::new(FakeCacheReader::failing(|| DecodeError::Canceled));
    let (gated, gate) = GatedExistsFs::new(fx.fs.clone());
    let codec = Arc::new(TagCodec::default());
    let services = ThumbnailCacheServices {
        platform_codec: codec.clone(),
        cache_image_reader: fx.reader.clone(),
        embedded_backend: Arc::new(FakeEmbeddedBackend {
            main: (64, 64),
            thumbnail: (16, 16),
        }),
        fs: gated,
    };
    let options = ThumbnailCacheOptions {
        embedded_reader: None,
        ..fx.options(FAKE_DIR, unused_embedded())
    };
    let cache = ThumbnailCache::new(services, options).expect("valid options");
    let load = pending(cache.get(SOURCE, None).expect("valid path"));
    gate.started.recv().expect("the load checks the disk entry");

    cache.dispose();
    gate.release.send(()).expect("release the load");

    assert!(matches!(load.wait(), Err(ThumbnailError::Canceled)));
    assert_eq!(
        codec.images.load(Ordering::SeqCst),
        0,
        "the embedded thumbnail was not read"
    );
}

/// Collects the `ThumbEnd` events of one path id as `(nav, source)`.
struct ThumbEndListener {
    path_id: String,
    events: Mutex<Vec<(i64, String)>>,
}

impl PerfListener for ThumbEndListener {
    fn on_event_written(&self, event: &PerfEvent) {
        if event.name != "ThumbEnd" {
            return;
        }
        if let [PerfValue::Int(nav), PerfValue::Str(path_id), PerfValue::Str(source), _] = event.payload.as_slice()
            && *path_id == self.path_id
        {
            self.events.lock().expect("events lock").push((*nav, source.clone()));
        }
    }
}

#[test]
fn perf_thumb_end_reports_each_source_with_the_callers_nav() {
    // Rust-only: semantics 2f/4 and the perf trap (ram/disk/embedded/none with the caller's nav; none for a RAW or a
    // failed reader)
    let cases = [
        (r"C:\PerfThumb\emb.jpg", Some("embedded")),
        (r"C:\PerfThumb\none.jpg", Some("none")),
        (r"C:\PerfThumb\fail.jpg", None),
        (r"C:\PerfThumb\raw.cr2", None),
        (r"C:\PerfThumb\disk.jpg", Some("disk")),
    ];
    let reader = FakeCacheReader::returning(
        1,
        1,
        CacheImageInfo {
            source_width: 1,
            source_height: 1,
            downscaled: false,
        },
    );
    let fx = Fixture::new(reader);
    for (path, _) in cases {
        fx.fs.add_file(path, b"bytes");
    }
    let disk_stat = fx.fs.peek_stat(r"C:\PerfThumb\disk.jpg").expect("exists");
    fx.fs.add_file(
        thumbnail_cache_file(Path::new(FAKE_DIR), r"C:\PerfThumb\disk.jpg", disk_stat),
        b"png",
    );
    let embedded: EmbeddedThumbnailReaderFn = Arc::new(|path: &str, _: &CancelToken| {
        if path.ends_with("none.jpg") {
            Ok(None)
        } else if path.ends_with("fail.jpg") {
            Err(ThumbnailError::Reader(Arc::new(io::Error::other("transient"))))
        } else {
            Ok(Some(decoded_image(16, 16, (64, 64))))
        }
    });
    let cache = fx.cache(embedded);
    PhotoReviewPerf::set_nav_context(4242);

    for (path, expected) in cases {
        let listener = Arc::new(ThumbEndListener {
            path_id: perf_path_id(path),
            events: Mutex::new(Vec::new()),
        });
        let subscription = PhotoReviewPerf::log().subscribe(listener.clone());
        let _ = cache.get(path, None).expect("valid path").wait();
        drop(subscription);
        let events = listener.events.lock().expect("events lock").clone();
        let expected: Vec<(i64, String)> = expected.map(|s| (4242, s.to_owned())).into_iter().collect();
        assert_eq!(events, expected, "{path}");
    }

    let listener = Arc::new(ThumbEndListener {
        path_id: perf_path_id(r"C:\PerfThumb\emb.jpg"),
        events: Mutex::new(Vec::new()),
    });
    let subscription = PhotoReviewPerf::log().subscribe(listener.clone());
    let hit = cache.get(r"C:\PerfThumb\emb.jpg", None).expect("valid path");
    assert!(matches!(hit, ThumbnailRequest::Ready(Ok(Some(_)))), "a RAM hit");
    drop(subscription);
    assert_eq!(
        *listener.events.lock().expect("events lock"),
        vec![(4242, "ram".to_owned())]
    );
    PhotoReviewPerf::set_nav_context(0);
}

// ---- WicCacheImageIoTests.cs and the native disk-decode checks ----------------------------------------------------

#[test]
fn disk_thumbnail_fits_to_max_width_and_reports_backend_and_orientation() {
    // C#: WicCacheImageIoTests.DiskThumbnail_FitsToMaxWidth_AndReportsBackendAndOrientation (fake reader; native twin below)
    let cases = [(1600, 400, 800, 200, true), (100, 50, 100, 50, false)];
    for (source_width, source_height, kept_width, kept_height, downscaled) in cases {
        let reader = Arc::new(FakeCacheReader::returning(
            kept_width,
            kept_height,
            CacheImageInfo {
                source_width,
                source_height,
                downscaled,
            },
        ));
        let fake_fs = FakeFileSystem::new();
        fake_fs.add_file(r"C:\thumbs\entry.png", b"png");
        let codec = TagCodec::default();

        let image = decode_disk_thumbnail(Path::new(r"C:\thumbs\entry.png"), &fake_fs, &*reader, &codec)
            .expect("the fake entry decodes");

        let call = reader.calls()[0];
        assert_eq!(call.layout, None, "the layout is chosen by the frame format");
        assert_eq!((call.box_width, call.box_height), (800, 0), "fit to the 800 px box");
        assert_eq!((call.expected_width, call.expected_height), (0, 0));
        assert_eq!((image.pixel_width(), image.pixel_height()), (kept_width, kept_height));
        assert_eq!(image.downscaled(), downscaled);
        assert_eq!(
            (image.original_width(), image.original_height()),
            (source_width, source_height)
        );
        assert_eq!(image.orientation(), 1);
        assert!(matches!(image.actual_backend(), DecoderBackend::WicDirect));
        assert_eq!(
            image.estimated_bytes(),
            i64::from(kept_width) * i64::from(kept_height) * 4
        );
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

// ---- Golden packages ----------------------------------------------------------------------------------------------

#[test]
fn cache_key_matches_golden() {
    // Golden: tests/golden/thumbnail-cache-key.json (oracle-cs thumbnail-cache-key, 22 cases, pure)
    let golden = golden::load("thumbnail-cache-key");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let path = str_of(case, "path");
        let length = case["length"]
            .as_u64()
            .unwrap_or_else(|| panic!("{name}: length is a u64"));
        let ticks = i64_of(case, "last_write_ticks");

        let keyed = if bool_of(case, "get_full_path") {
            get_full_path(path).expect("the golden path is valid")
        } else {
            path.to_owned()
        };
        assert_eq!(keyed, str_of(case, "keyed_path"), "{name}: keyed path");

        assert_eq!(
            build_stamp(&keyed, length, ticks),
            str_of(case, "stamp"),
            "{name}: stamp"
        );
        let key = build_key(&keyed, length, ticks);
        assert_eq!(key, str_of(case, "key"), "{name}: key");

        let file = cache_file_path(Path::new(FAKE_DIR), &key);
        assert_eq!(
            file.file_name().and_then(|n| n.to_str()),
            Some(str_of(case, "file_name")),
            "{name}: file name"
        );
    }
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
