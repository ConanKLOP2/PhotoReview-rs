//! Shared fakes of the presenter-side tests that sit on top of `PreviewImageService` (W8-07g..07k). Included from each test
//! crate with `mod support { pub mod preview_fakes; }`.
//!
//! Never WIC and never a real decoder. [`SizedDecoder`] scripts every answer (size, per-file [`Gate`], failure, cancel) and
//! counts the full-resolution decodes; [`IndicatorGates`] replaces the 300 ms RAW indicator delay by gates the test opens
//! itself; [`PixelsCodec`] / [`FakePayloadCodec`] are the platform and `.pv4` payload codecs the service config requires
//! (copied from `crates/pr-imaging/tests/support/preview_service.rs`, which is test-only in another crate).
//! Every wait on these fakes is bounded by [`TIMEOUT`] (hang guard, AGENTS.md "Tests"); no fixed sleeps.

// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;
use pr_imaging::cache::preview_file::{PreviewCacheError, PreviewPayloadCodec};
use pr_imaging::decode_types::{
    DecodeBox, DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, PixelLease, PlatformImage,
};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_imaging::service::{PreviewImage, PreviewImageService, PreviewServiceConfig};

use pr_app::coordinators::zoom_detail::IndicatorDelay;

/// Hang guard for every blocking wait in a test.
pub const TIMEOUT: Duration = Duration::from_secs(20);
pub const MIB: i64 = 1024 * 1024;
pub const GIB: i64 = 1024 * MIB;

/// Locks a fake's mutex; a poisoned lock is still usable (the fakes hold no invariant a panic could break).
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- gates ----------------------------------------------------------------------------------------------------------

/// C# `SemaphoreSlim(0)` used as a one-shot gate: a decode waits on it until the test opens it.
#[derive(Debug, Default)]
pub struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Lets every current and future waiter through.
    pub fn open(&self) {
        *lock(&self.open) = true;
        self.changed.notify_all();
    }

    /// Blocks until the gate is open; `false` when [`TIMEOUT`] passes first.
    pub fn wait(&self) -> bool {
        let guard = lock(&self.open);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |open| !*open)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

// ---- images ---------------------------------------------------------------------------------------------------------

/// A platform image that is only an identity (no pixel memory: a 6000 x 4000 test image would cost 96 MB).
pub fn opaque_platform_image() -> PlatformImage {
    PlatformImage::Opaque(Arc::new(0_u8))
}

/// A decoded image of `width x height` pixels whose full-resolution size is `original_width x original_height`.
pub fn sized_image(
    width: i32,
    height: i32,
    original_width: i32,
    original_height: i32,
    downscaled: bool,
) -> PreviewImage {
    let image = DecodedImage::new(
        opaque_platform_image(),
        width,
        height,
        i64::from(width) * i64::from(height) * 4,
        DecodedImageOptions {
            downscaled,
            orientation: 1,
            actual_backend: DecoderBackend::Wpf,
            original_width,
            original_height,
            exif: None,
            is_degraded_fallback: false,
        },
    )
    .unwrap_or_else(|error| panic!("test image: {error}"));
    Arc::new(image)
}

// ---- decoder --------------------------------------------------------------------------------------------------------

/// C# `SizedDecoder`: a bounded request is answered with a downscaled preview, an unbounded one (the full-resolution
/// decode) with the original. Scripts are keyed by the file NAME; every full-resolution decode is counted.
#[derive(Debug)]
pub struct SizedDecoder {
    original: Mutex<(i32, i32)>,
    decoded: Mutex<(i32, i32)>,
    preview_width: Mutex<i32>,
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    fail: Mutex<HashSet<String>>,
    cancel: Mutex<HashSet<String>>,
    counts: Mutex<HashMap<String, usize>>,
    total: AtomicUsize,
    started: Mutex<Vec<String>>,
    started_changed: Condvar,
}

impl SizedDecoder {
    /// A 6000 x 4000 source, previews 600 px wide, no gate, no failure.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            original: Mutex::new((6000, 4000)),
            decoded: Mutex::new((0, 0)),
            preview_width: Mutex::new(600),
            gates: Mutex::new(HashMap::new()),
            fail: Mutex::new(HashSet::new()),
            cancel: Mutex::new(HashSet::new()),
            counts: Mutex::new(HashMap::new()),
            total: AtomicUsize::new(0),
            started: Mutex::new(Vec::new()),
            started_changed: Condvar::new(),
        })
    }

    /// The reader's (post-orientation) size of every file.
    pub fn set_original(&self, width: i32, height: i32) {
        *lock(&self.original) = (width, height);
    }

    /// The size of the full decode when it differs from the reader's size (RAW: LibRaw sensor area).
    pub fn set_decoded(&self, width: i32, height: i32) {
        *lock(&self.decoded) = (width, height);
    }

    /// Holds every full-resolution decode of `file_name` until the returned gate is opened.
    pub fn gate(&self, file_name: &str) -> Arc<Gate> {
        let gate = Gate::new();
        lock(&self.gates).insert(file_name.to_owned(), Arc::clone(&gate));
        gate
    }

    /// Makes the full-resolution decode of `file_name` fail (`InvalidData`).
    pub fn fail(&self, file_name: &str) {
        lock(&self.fail).insert(file_name.to_owned());
    }

    /// Undoes [`Self::fail`].
    pub fn stop_failing(&self, file_name: &str) {
        lock(&self.fail).remove(file_name);
    }

    /// Makes the full-resolution decode of `file_name` end as cancelled by the decoder.
    pub fn cancel(&self, file_name: &str) {
        lock(&self.cancel).insert(file_name.to_owned());
    }

    /// Number of full-resolution decodes of `file_name` that entered the decoder.
    pub fn original_decodes(&self, file_name: &str) -> usize {
        lock(&self.counts).get(file_name).copied().unwrap_or(0)
    }

    /// Number of full-resolution decodes of every file.
    pub fn total_original_decodes(&self) -> usize {
        self.total.load(Ordering::SeqCst)
    }

    /// Waits until a full-resolution decode of `file_name` has entered the decoder; `false` when [`TIMEOUT`] passes first.
    pub fn wait_started(&self, file_name: &str) -> bool {
        let guard = lock(&self.started);
        let (guard, _) = self
            .started_changed
            .wait_timeout_while(guard, TIMEOUT, |started| !started.iter().any(|name| name == file_name))
            .unwrap_or_else(PoisonError::into_inner);
        guard.iter().any(|name| name == file_name)
    }

    /// Opens every gate (test teardown: no worker stays blocked on a failed test).
    pub fn open_all_gates(&self) {
        for gate in lock(&self.gates).values() {
            gate.open();
        }
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl ImageDecoder for SizedDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let (original_width, original_height) = *lock(&self.original);
        if !request.decode_box().is_unbounded() {
            let width = *lock(&self.preview_width);
            let height = i32::try_from(i64::from(width) * i64::from(original_height) / i64::from(original_width))
                .unwrap_or(1)
                .max(1);
            return Ok(sized_image(width, height, original_width, original_height, true));
        }

        let name = file_name_of(&request.path);
        *lock(&self.counts).entry(name.clone()).or_insert(0) += 1;
        self.total.fetch_add(1, Ordering::SeqCst);
        lock(&self.started).push(name.clone());
        self.started_changed.notify_all();

        let gate = lock(&self.gates).get(&name).cloned();
        if let Some(gate) = gate {
            gate.wait();
        }
        if lock(&self.cancel).contains(&name) {
            return Err(DecodeError::Canceled);
        }
        if lock(&self.fail).contains(&name) {
            return Err(DecodeError::InvalidData {
                message: "original decode failed".to_owned(),
            });
        }
        let (width, height) = match *lock(&self.decoded) {
            (0, _) | (_, 0) => (original_width, original_height),
            decoded => decoded,
        };
        Ok(sized_image(width, height, width, height, false))
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        let (width, height) = *lock(&self.original);
        Ok(ImageInfo::new(width, height))
    }
}

/// A RAW's regular decode: the embedded JPEG (2000 px wide) of a 6000 x 4000 sensor, whatever the box (C# `EmbeddedRawDecoder`).
#[derive(Debug, Default)]
pub struct EmbeddedRawDecoder {
    pub downscaled: bool,
}

impl ImageDecoder for EmbeddedRawDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Ok(if self.downscaled {
            sized_image(2000, 1333, 6000, 4000, true)
        } else {
            sized_image(6000, 4000, 6000, 4000, false)
        })
    }

    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Ok(ImageInfo::new(6000, 4000))
    }
}

// ---- RAW indicator delay --------------------------------------------------------------------------------------------

/// What one call of the injected indicator delay did.
#[derive(Debug)]
struct DelayCall {
    open: bool,
    cancel: CancelToken,
    returned: Option<bool>,
}

/// The injected `IndicatorDelay`: one gate per RAW load that the test opens itself (cancelled with the load's token like the
/// real delay), so the "300 ms" never depends on the clock. A call returns `true` when its gate was opened, `false` when
/// its token was cancelled first.
#[derive(Debug, Default)]
pub struct IndicatorGates {
    calls: Mutex<Vec<DelayCall>>,
    changed: Condvar,
}

impl IndicatorGates {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The function to hand to `ZoomDetailLoader::with_indicator_delay`.
    pub fn delay(self: &Arc<Self>) -> IndicatorDelay {
        let gates = Arc::clone(self);
        Arc::new(move |_delay: Duration, cancel: &CancelToken| gates.run_call(cancel))
    }

    fn run_call(&self, cancel: &CancelToken) -> bool {
        let mut calls = lock(&self.calls);
        let index = calls.len();
        calls.push(DelayCall {
            open: false,
            cancel: cancel.clone(),
            returned: None,
        });
        self.changed.notify_all();
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let outcome = if calls[index].open {
                Some(true)
            } else if cancel.is_canceled() || Instant::now() >= deadline {
                Some(false)
            } else {
                None
            };
            if let Some(outcome) = outcome {
                calls[index].returned = Some(outcome);
                self.changed.notify_all();
                return outcome;
            }
            // The token has no wake-up, so the wait is sliced (worker thread only; the tests never wait on a slice).
            calls = self
                .changed
                .wait_timeout(calls, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Waits until `count` delays were started; `false` when [`TIMEOUT`] passes first.
    pub fn wait_calls(&self, count: usize) -> bool {
        let guard = lock(&self.calls);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |calls| calls.len() < count)
            .unwrap_or_else(PoisonError::into_inner);
        guard.len() >= count
    }

    /// The 300 ms of delay `index` "elapsed".
    pub fn open(&self, index: usize) {
        if let Some(call) = lock(&self.calls).get_mut(index) {
            call.open = true;
        }
        self.changed.notify_all();
    }

    /// True once delay `index` was told to stop.
    pub fn is_cancelled(&self, index: usize) -> bool {
        lock(&self.calls)
            .get(index)
            .is_some_and(|call| call.cancel.is_canceled())
    }

    /// Waits until delay `index` returned and gives its result (`true` = elapsed, `false` = cancelled).
    pub fn wait_returned(&self, index: usize) -> Option<bool> {
        let guard = lock(&self.calls);
        let (guard, _) = self
            .changed
            .wait_timeout_while(guard, TIMEOUT, |calls| {
                calls.get(index).is_none_or(|call| call.returned.is_none())
            })
            .unwrap_or_else(PoisonError::into_inner);
        guard.get(index).and_then(|call| call.returned)
    }

    /// Number of delays started so far.
    pub fn call_count(&self) -> usize {
        lock(&self.calls).len()
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

// ---- service --------------------------------------------------------------------------------------------------------

/// What differs between the services of the zoom tests (C# constructor arguments `isOriginalLoadingMode`, `rawFullDecoder`,
/// `isRawFullDecodeEnabled`).
#[derive(Default)]
pub struct ServiceOptions {
    pub original_loading_mode: bool,
    pub raw_full_decoder: Option<Arc<dyn ImageDecoder>>,
    pub raw_full_decode_enabled: bool,
}

/// A preview service over `decoder` with the C# test defaults: 64 MiB RAM budget, the disk cache disabled, 32 GiB of RAM,
/// a fixed `1920 x 1080` decode box, the `pixels` codec and the fake payload codec. `dir` is the (unused) disk folder.
pub fn build_service(dir: &Path, decoder: Arc<dyn ImageDecoder>, options: ServiceOptions) -> Arc<PreviewImageService> {
    let original_mode = options.original_loading_mode;
    let raw_enabled = options.raw_full_decode_enabled;
    let mut config = PreviewServiceConfig::new(
        Arc::new(ReviewMetrics::new()),
        Arc::new(move || original_mode),
        Arc::new(|| DecodeBox::new(1920, 1080)),
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
    );
    config.capacity_bytes = 64 * MIB;
    config.disk_cache_directory = Some(dir.to_path_buf());
    config.disable_disk_cache_override = Some(true);
    config.decoder = Some(decoder);
    config.physical_memory_bytes = 32 * GIB;
    config.raw_full_decoder = options.raw_full_decoder;
    config.is_raw_full_decode_enabled = Some(Arc::new(move || raw_enabled));
    match PreviewImageService::new(config) {
        Ok(service) => Arc::new(service),
        Err(error) => panic!("the preview service did not build: {error}"),
    }
}
