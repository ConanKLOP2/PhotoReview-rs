//! Shared fakes of the W4-14 preview service tests (W4-14a owns this file; W4-14b and W4-14c tests include it too).
//! Included from each test crate with `mod support { pub mod preview_service; }`.
//!
//! Never WIC and never a real decoder: [`FakeDecoder`] scripts every answer per file name, [`PixelsCodec`] is the
//! `pixels` platform codec (C-02), [`FakePayloadCodec`] stands in for the JPEG payload codec of the `.pv4` entries
//! (W4-08b), [`RecordingSpawner`] records the thread kinds the service starts and delegates to `StdThreadSpawner`.
//! Every wait a test does on these fakes is bounded: [`TIMEOUT`] (hang guard, AGENTS.md "Tests").

#![allow(dead_code)] // each test binary uses a different subset of these helpers

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;
use std::time::Duration;

use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
use pr_imaging::decode_types::{
    DecodeBox, DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PixelLease, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, ImageDecoderFactory, PlatformImageCodec};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging::service::{
    DecodeThreadKind, DecodeThreadSpawner, PreviewImage, PreviewServiceConfig, StdThreadSpawner,
};

/// Hang guard for every blocking wait in a test.
pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const MIB: i64 = 1024 * 1024;
pub const GIB: i64 = 1024 * MIB;

/// The physical RAM every test pretends to have (the C# tests read the real machine; the Rust ones pass it explicitly).
pub const PHYSICAL_32_GIB: i64 = 32 * GIB;

/// Locks a fake's mutex; a poisoned lock is still usable (the fakes hold no invariant a panic could break).
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- decoder --------------------------------------------------------------------------------------------------------

/// Which error a scripted decode fails with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    InvalidData,
    Busy,
    Canceled,
}

/// What one scripted file decodes to. The default is a 4x3 `Wpf` image, not downscaled, with no failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub width: i32,
    pub height: i32,
    pub downscaled: bool,
    pub backend: DecoderBackend,
    /// Original (post-orientation) size reported with the image; `None` = the pixel size.
    pub original: Option<(i32, i32)>,
    pub degraded: bool,
    pub source_bytes_read: Option<i64>,
    pub failure: Option<Failure>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            width: 4,
            height: 3,
            downscaled: false,
            backend: DecoderBackend::Wpf,
            original: None,
            degraded: false,
            source_bytes_read: None,
            failure: None,
        }
    }
}

/// One `decode` call as the fake saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub request: DecodeRequest,
    pub thread: ThreadId,
}

/// A scripted [`ImageDecoder`]. Scripts are keyed by the file NAME (`a.jpg`), the default applies to every other file.
/// Records every request; optionally blocks each decode until [`FakeDecoder::release`]; signals each started decode.
#[derive(Debug)]
pub struct FakeDecoder {
    scripts: Mutex<HashMap<String, Script>>,
    default: Mutex<Script>,
    recorded: Mutex<Vec<Recorded>>,
    blocked: Mutex<bool>,
    unblocked: Condvar,
    started: Mutex<Vec<String>>,
    started_changed: Condvar,
    calls: AtomicUsize,
}

impl FakeDecoder {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            scripts: Mutex::new(HashMap::new()),
            default: Mutex::new(Script::default()),
            recorded: Mutex::new(Vec::new()),
            blocked: Mutex::new(false),
            unblocked: Condvar::new(),
            started: Mutex::new(Vec::new()),
            started_changed: Condvar::new(),
            calls: AtomicUsize::new(0),
        })
    }

    /// Scripts the decode of the file called `file_name`.
    pub fn script(&self, file_name: &str, script: Script) {
        lock(&self.scripts).insert(file_name.to_owned(), script);
    }

    /// Scripts every file without its own script.
    pub fn script_default(&self, script: Script) {
        *lock(&self.default) = script;
    }

    /// Makes every following decode block until [`Self::release`] (bounded by [`TIMEOUT`] per decode).
    pub fn block(&self) {
        *lock(&self.blocked) = true;
    }

    /// Lets blocked and following decodes run.
    pub fn release(&self) {
        *lock(&self.blocked) = false;
        self.unblocked.notify_all();
    }

    /// Waits until a decode of `file_name` has started; `false` when [`TIMEOUT`] passes first.
    pub fn wait_started(&self, file_name: &str) -> bool {
        let guard = lock(&self.started);
        let (guard, _) = self
            .started_changed
            .wait_timeout_while(guard, TIMEOUT, |started| !started.iter().any(|name| name == file_name))
            .unwrap_or_else(PoisonError::into_inner);
        guard.iter().any(|name| name == file_name)
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Every request, in arrival order.
    pub fn requests(&self) -> Vec<DecodeRequest> {
        lock(&self.recorded).iter().map(|r| r.request.clone()).collect()
    }

    /// Every decode with the thread that ran it.
    pub fn recorded(&self) -> Vec<Recorded> {
        lock(&self.recorded).clone()
    }

    fn script_for(&self, file_name: &str) -> Script {
        lock(&self.scripts)
            .get(file_name)
            .cloned()
            .unwrap_or_else(|| lock(&self.default).clone())
    }

    fn wait_while_blocked(&self) {
        let guard = lock(&self.blocked);
        let _ = self
            .unblocked
            .wait_timeout_while(guard, TIMEOUT, |blocked| *blocked)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl ImageDecoder for FakeDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        lock(&self.recorded).push(Recorded {
            request: request.clone(),
            thread: std::thread::current().id(),
        });
        let name = file_name_of(&request.path);
        lock(&self.started).push(name.clone());
        self.started_changed.notify_all();
        self.wait_while_blocked();

        let script = self.script_for(&name);
        match script.failure {
            Some(Failure::InvalidData) => {
                return Err(DecodeError::InvalidData {
                    message: format!("scripted failure for {name}"),
                });
            }
            Some(Failure::Busy) => return Err(DecodeError::busy()),
            Some(Failure::Canceled) => return Err(DecodeError::Canceled),
            None => {}
        }
        let pixels = PixelBuffer::allocate(script.width, script.height, PixelLayout::Bgr32).map_err(|e| {
            DecodeError::InvalidArgument {
                message: format!("{e:?}"),
            }
        })?;
        let estimated = i64::from(script.width) * i64::from(script.height) * 4;
        let (original_width, original_height) = script.original.unwrap_or((0, 0));
        let image = DecodedImage::new(
            PlatformImage::Pixels(Arc::new(pixels)),
            script.width,
            script.height,
            estimated,
            DecodedImageOptions {
                downscaled: script.downscaled,
                orientation: 1,
                actual_backend: script.backend,
                original_width,
                original_height,
                exif: None,
                is_degraded_fallback: script.degraded,
            },
        )
        .map_err(|e| DecodeError::InvalidArgument {
            message: format!("{e}"),
        })?;
        Ok(Arc::new(image))
    }

    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        let script = self.script_for(&file_name_of(Path::new(path)));
        Ok(ImageInfo::new(script.width, script.height))
    }
}

/// A decoder that must never be asked to decode (key tests). Every call fails loudly.
#[derive(Debug, Default)]
pub struct NeverDecoder;

impl ImageDecoder for NeverDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Err(DecodeError::InvalidOperation {
            message: "NeverDecoder was asked to decode".to_owned(),
        })
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::InvalidOperation {
            message: "NeverDecoder was asked to read info".to_owned(),
        })
    }
}

/// A factory that counts its `create` calls and returns one shared [`FakeDecoder`] for every backend.
#[derive(Debug)]
pub struct CountingFactory {
    pub decoder: Arc<FakeDecoder>,
    creates: AtomicUsize,
}

impl CountingFactory {
    pub fn new(decoder: Arc<FakeDecoder>) -> Arc<Self> {
        Arc::new(Self {
            decoder,
            creates: AtomicUsize::new(0),
        })
    }

    pub fn creates(&self) -> usize {
        self.creates.load(Ordering::SeqCst)
    }
}

impl ImageDecoderFactory for CountingFactory {
    fn create(&self, _backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        self.decoder.clone()
    }

    fn is_registered(&self, _backend: DecoderBackend) -> bool {
        true
    }
}

// ---- codecs ---------------------------------------------------------------------------------------------------------

/// The `pixels` platform codec: the platform image IS the pixel buffer.
#[derive(Debug, Default)]
pub struct PixelsCodec;

impl PlatformImageCodec for PixelsCodec {
    fn name(&self) -> &'static str {
        "pixels"
    }

    fn image_from_pixels(&self, pixels: PixelBuffer) -> Result<PlatformImage, DecodeError> {
        Ok(PlatformImage::Pixels(Arc::new(pixels)))
    }

    fn to_pixels<'a>(&self, platform_image: &'a PlatformImage) -> Result<PixelLease<'a>, DecodeError> {
        match platform_image {
            PlatformImage::Pixels(pixels) => Ok(PixelLease::borrowed(pixels.as_ref())),
            PlatformImage::Opaque(_) => Err(DecodeError::InvalidArgument {
                message: "a foreign platform image cannot be cached".to_owned(),
            }),
        }
    }
}

/// Stand-in for the `.pv4` JPEG payload codec (W4-08b). Layout of a payload: `FF D8`, width (i32 LE), height (i32 LE),
/// layout byte, `FF D9`. `decode_jpeg` rebuilds a buffer of the expected size and fails `InvalidData` on a short or
/// mismatched payload.
#[derive(Debug, Default)]
pub struct FakePayloadCodec;

const PAYLOAD_LEN: usize = 2 + 4 + 4 + 1 + 2;

impl PreviewPayloadCodec for FakePayloadCodec {
    fn encode_jpeg(&self, pixels: &PixelBuffer, _quality: i32, out: &mut dyn Write) -> io::Result<()> {
        out.write_all(&[0xFF, 0xD8])?;
        out.write_all(&pixels.width().to_le_bytes())?;
        out.write_all(&pixels.height().to_le_bytes())?;
        out.write_all(&[u8::try_from(pixels.layout().as_i32()).unwrap_or(0)])?;
        out.write_all(&[0xFF, 0xD9])
    }

    fn decode_jpeg(
        &self,
        payload: &[u8],
        layout: PixelLayout,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<PixelBuffer, PreviewCacheError> {
        if payload.len() < PAYLOAD_LEN {
            return Err(PreviewCacheError::InvalidData(
                "Cache entry payload is empty.".to_owned(),
            ));
        }
        let width = i32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
        let height = i32::from_le_bytes([payload[6], payload[7], payload[8], payload[9]]);
        if width != expected_width || height != expected_height {
            return Err(PreviewCacheError::InvalidData(
                "Cache entry payload dimensions do not match its header.".to_owned(),
            ));
        }
        PixelBuffer::allocate(expected_width, expected_height, layout)
            .map_err(|e| PreviewCacheError::InvalidData(format!("{e:?}")))
    }
}

// ---- thread spawner -------------------------------------------------------------------------------------------------

/// Records the kind of every thread the service starts, then delegates to [`StdThreadSpawner`].
#[derive(Debug, Default)]
pub struct RecordingSpawner {
    kinds: Mutex<Vec<DecodeThreadKind>>,
}

impl RecordingSpawner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn kinds(&self) -> Vec<DecodeThreadKind> {
        lock(&self.kinds).clone()
    }
}

impl DecodeThreadSpawner for RecordingSpawner {
    fn spawn(&self, kind: DecodeThreadKind, job: Box<dyn FnOnce() + Send + 'static>) -> io::Result<()> {
        lock(&self.kinds).push(kind);
        StdThreadSpawner.spawn(kind, job)
    }
}

// ---- images ---------------------------------------------------------------------------------------------------------

/// A fully opaque `w x h` image over pixel data (`Bgr32`, every byte 255).
pub fn opaque_image(width: i32, height: i32) -> PreviewImage {
    image_with_layout(width, height, PixelLayout::Bgr32, 255)
}

/// A `w x h` image with alpha 0 everywhere (`Pbgra32`, all bytes 0).
pub fn transparent_image(width: i32, height: i32) -> PreviewImage {
    image_with_layout(width, height, PixelLayout::Pbgra32, 0)
}

fn image_with_layout(width: i32, height: i32, layout: PixelLayout, fill: u8) -> PreviewImage {
    let mut pixels = PixelBuffer::allocate(width, height, layout).expect("test image buffer");
    pixels.bytes_mut().expect("test image bytes").fill(fill);
    let estimated = i64::from(width) * i64::from(height) * 4;
    let image = DecodedImage::new(
        PlatformImage::Pixels(Arc::new(pixels)),
        width,
        height,
        estimated,
        DecodedImageOptions {
            downscaled: false,
            orientation: 1,
            actual_backend: DecoderBackend::Wpf,
            original_width: 0,
            original_height: 0,
            exif: None,
            is_degraded_fallback: false,
        },
    )
    .expect("test image");
    Arc::new(image)
}

// ---- configuration --------------------------------------------------------------------------------------------------

/// A service configuration with the C# test defaults: 64 MiB RAM budget, the disk cache disabled, 32 GiB of RAM, a
/// fixed `512 x 512` decode box, the `pixels` codec and the fake payload codec, and `decoder` for every backend.
/// `dir` is the disk-cache directory (never touched while the disk cache is disabled). Adjust the public fields after.
pub fn config(dir: &Path, decoder: Arc<dyn ImageDecoder>) -> PreviewServiceConfig {
    let mut config = PreviewServiceConfig::new(
        Arc::new(ReviewMetrics::new()),
        Arc::new(|| false),
        Arc::new(|| DecodeBox::new(512, 512)),
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
    );
    config.capacity_bytes = 64 * MIB;
    config.disk_cache_directory = Some(dir.to_path_buf());
    config.disable_disk_cache_override = Some(true);
    config.decoder = Some(decoder);
    config.physical_memory_bytes = PHYSICAL_32_GIB;
    config
}

/// A box source reading `box_value` on every call.
pub fn box_source(box_value: Arc<Mutex<DecodeBox>>) -> Arc<dyn Fn() -> DecodeBox + Send + Sync> {
    Arc::new(move || *lock(&box_value))
}
