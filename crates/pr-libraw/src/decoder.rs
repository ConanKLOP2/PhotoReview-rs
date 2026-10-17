//! Port of `PhotoReview.Imaging.LibRaw/LibRawDecoder.cs` (C# baseline d92d6aae, 502 lines; W5-16c).
//!
//! Full RAW decoder backed by the pinned LibRaw C API (`libraw.dll` 0.22.2, loaded at run time by W5-15a):
//!
//! * [`LibRawDecoder`]: `decode` / `decode_cancellable` (serialised by the process-wide [`FullDecodeGate`], W5-16a; memory
//!   admission through `memory_guard`, W5-16b) and `read_info`.
//! * [`read_jpeg_thumbnail`]: LibRaw's embedded JPEG thumbnail (the RAW preview fallback, ORF / RAW without a JPEG). NOT behind the
//!   gate: no demosaic, memory bounded by [`MAX_THUMBNAIL_BYTES`].
//! * Pure helpers (golden-tested, no native library): [`trim_to_end_of_image`], [`find_end_of_image`], [`to_extended_length_path`],
//!   [`open_file_with`], [`check_result`], [`create_failure`], the header checks.
//!
//! Every native call goes through the [`LibRawApi`] / [`LibRawSession`] / [`ProcessedImage`] seams. The production
//! implementation of the seams is the safe adapter [`NativeLibRaw`] at the end of this file, over W5-15a's `native` module
//! (`LibRawHandle<'static>`, `LibRawImage`, `format_error`), which holds every `unsafe` of the LibRaw path INCLUDING the progress
//! callback (`LibRawHandle::set_cancel_token`; review decision: one callback, one owner). This file has NO `unsafe`; the decoder
//! logic is safe Rust and every unit test runs it over fakes.

use std::ffi::OsStr;
use std::fmt;
use std::sync::{Arc, LazyLock};

use pr_core::model::enums::DecoderBackend;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path;
use pr_imaging::decode_types::{
    DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo, SourceBytes,
};
use pr_imaging::decoder_traits::{CancellableImageDecoder, DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::memory_headroom::{self, MemoryInfoSource};
use pr_imaging::pixels::{PixelBuffer, PixelBufferError};

use crate::full_decode_gate::{FullDecodeGate, FullDecodeLease};
use crate::memory_guard::{self, RawStructure};
use crate::native;
use crate::resampler::{self, BgraBuffer, ResampleError};

/// `libraw_processed_image_t` header: owned by W5-15a (FFI layout, `repr(C)`, `DATA_OFFSET = 16`), re-exported here so the
/// seams and tests can name it `decoder::ProcessedImageHeader` (review decision: no second definition).
pub use crate::native::ProcessedImageHeader;

// ------------------------------------------------------------------------------------------------ constants

/// C# `MaxThumbnailBytes` (`32 << 20`): largest embedded thumbnail accepted from `libraw_dcraw_make_mem_thumb`.
pub const MAX_THUMBNAIL_BYTES: u32 = 32 << 20;
/// C# `MaxQueuedPreloadDecodes`: at most this many preload decodes wait for the slot; extra ones fail fast with `Busy`.
pub const MAX_QUEUED_PRELOAD_DECODES: i32 = 2;
/// C# `LongPathThreshold`: paths of at least this many UTF-16 code units are retried in extended-length form when
/// `libraw_open_wfile` fails with [`LIBRAW_IO_ERROR`].
pub const LONG_PATH_THRESHOLD: usize = 248;
/// C# `TrailingEoiSearchWindow` (64 KiB): the structure-free fallback looks for the last EOI only in the final window.
pub const TRAILING_EOI_SEARCH_WINDOW: usize = 64 * 1024;

/// `LIBRAW_IO_ERROR`.
pub const LIBRAW_IO_ERROR: i32 = -100_009;
/// `LIBRAW_UNSUFFICIENT_MEMORY`.
pub const LIBRAW_UNSUFFICIENT_MEMORY: i32 = -100_007;
/// `ENOMEM`: LibRaw returns positive errno values from unpack/process for libc failures.
pub const ERRNO_NO_MEMORY: i32 = 12;

/// `LIBRAW_IMAGE_JPEG` (C# `LibRawNativeMethods.ImageJpeg`; W5-15a `native::IMAGE_JPEG`).
pub const LIBRAW_IMAGE_JPEG: i32 = native::IMAGE_JPEG;
/// `LIBRAW_IMAGE_BITMAP` (C# `LibRawNativeMethods.ImageBitmap`; W5-15a `native::IMAGE_BITMAP`).
pub const LIBRAW_IMAGE_BITMAP: i32 = native::IMAGE_BITMAP;

/// C# literal of every out-of-memory mapping (`InvalidOperationException`): native codes, managed OOM and the admission refusal.
pub const NOT_ENOUGH_MEMORY_MESSAGE: &str = "Not enough memory to decode this RAW image.";
/// `NotSupportedException` of an un-oriented full decode.
pub const ORIENTATION_REQUIRED_MESSAGE: &str =
    "LibRaw always applies the RAW container's orientation; an un-oriented full decode is unavailable.";
/// `InvalidOperationException` when `libraw_init` returns null.
pub const INIT_FAILED_MESSAGE: &str = "LibRaw could not initialize its decoder.";
/// `InvalidDataException` of [`trim_to_end_of_image`] (all three throw sites).
pub const INCOMPLETE_JPEG_MESSAGE: &str = "LibRaw thumbnail is not a complete JPEG stream.";
/// `InvalidDataException` of [`validate_image_dimensions`].
pub const INVALID_DIMENSIONS_MESSAGE: &str = "LibRaw returned invalid image dimensions.";
/// `InvalidDataException` when the processed image is not an 8-bit gray/RGB bitmap.
pub const UNSUPPORTED_PROCESSED_IMAGE_MESSAGE: &str = "LibRaw returned an unsupported processed image format.";
/// .NET `SR.Argument_EmptyOrWhiteSpaceString` (`ArgumentException.ThrowIfNullOrWhiteSpace`); the message gets
/// ` (Parameter '<name>')` appended, see [`blank_argument_error`]. Pinned by the golden `guard-*` cases.
pub const EMPTY_OR_WHITE_SPACE_MESSAGE: &str =
    "The value cannot be an empty string or composed entirely of whitespace.";

/// Stage names reported to the [`StageObserver`] (C# string literals), in decode order.
pub const STAGE_CONFIGURED: &str = "configured";
pub const STAGE_OPENED: &str = "opened";
pub const STAGE_UNPACKED: &str = "unpacked";
pub const STAGE_PROCESSED: &str = "processed";
pub const STAGE_HANDLE_CLOSED: &str = "handle-closed";
pub const STAGE_SOURCE_RELEASED: &str = "source-released";
pub const STAGE_SOURCE_HELD: &str = "source-held";
pub const STAGE_BITMAP_CREATED: &str = "bitmap-created";

/// Process-wide gate shared by every [`LibRawDecoder::new`] (C# `static readonly FullDecodeGate s_fullDecodeGate`).
static FULL_DECODE_GATE: LazyLock<FullDecodeGate> = LazyLock::new(|| FullDecodeGate::new(MAX_QUEUED_PRELOAD_DECODES));

// ------------------------------------------------------------------------------------------------ native seams

/// One `libraw_processed_image_t` (C# `SafeLibRawImageHandle`). `Drop` calls `libraw_dcraw_clear_mem` exactly once. It may outlive
/// the [`LibRawSession`] that made it (the block is separately allocated), which [`LibRawDecoder`] relies on.
pub trait ProcessedImage {
    /// The header copied out of the block (C# `Marshal.PtrToStructure<ProcessedImageHeader>`).
    fn header(&self) -> ProcessedImageHeader;
    /// The `data[]` payload: exactly `header().data_size` bytes starting at [`ProcessedImageHeader::DATA_OFFSET`].
    fn data(&self) -> &[u8];
}

/// One `libraw_data_t` handle (C# `SafeLibRawHandle` plus the `LibRawNativeMethods` calls on it). `Drop` calls `libraw_close`
/// exactly once, THEN releases whatever the session keeps alive for the native side (the [`SourceBytes`] of
/// [`LibRawSession::open_buffer`], the boxed token of [`LibRawSession::set_progress_handler`]).
pub trait LibRawSession {
    /// C# `LibRawSetProgressHandler(raw, CancellationCallback, cancellationState.Pointer)`: W5-15a `set_cancel_token(cancel)`
    /// installs its own callback (`progress_verdict`: `1` when canceled) over a box the handle keeps until after
    /// `libraw_close`. Fakes record the token so tests can prove it is the caller's (a clone shares the flag).
    fn set_progress_handler(&mut self, cancel: CancelToken);
    /// `libraw_set_output_color`.
    fn set_output_color(&mut self, value: i32);
    /// `libraw_set_output_bps`.
    fn set_output_bps(&mut self, value: i32);
    /// `libraw_set_no_auto_bright`.
    fn set_no_auto_bright(&mut self, value: i32);
    /// C# `LibRawNativeMethods.TrySetUseCameraWb(raw, outputColor, outputBps, noAutoBright)` (production overload: exact-version
    /// gate + pinned layout, W5-15a). The three output settings are applied whatever the result.
    fn try_set_use_camera_wb(&mut self, output_color: i32, output_bps: i32, no_auto_bright: i32) -> bool;
    /// `libraw_open_wfile` with `path` as a NUL-terminated UTF-16 string. LibRaw error code, `0` = success.
    fn open_wfile(&mut self, path: &OsStr) -> i32;
    /// `libraw_open_buffer(raw, bytes.as_ptr(), bytes.len())`. LibRaw keeps a pointer into the buffer (no copy) that
    /// unpack/process read later, so the session stores `bytes` until after `libraw_close` (C#: the `MemoryHandle` pin
    /// declared before `raw`).
    fn open_buffer(&mut self, bytes: SourceBytes) -> i32;
    /// `libraw_unpack`.
    fn unpack(&mut self) -> i32;
    /// `libraw_unpack_thumb`.
    fn unpack_thumb(&mut self) -> i32;
    /// `libraw_dcraw_process`.
    fn dcraw_process(&mut self) -> i32;
    /// `libraw_adjust_sizes_info_only`.
    fn adjust_sizes_info_only(&mut self) -> i32;
    /// `libraw_get_iwidth`.
    fn iwidth(&self) -> i32;
    /// `libraw_get_iheight`.
    fn iheight(&self) -> i32;
    /// `libraw_get_raw_width`.
    fn raw_width(&self) -> i32;
    /// `libraw_get_raw_height`.
    fn raw_height(&self) -> i32;
    /// C# `LibRawNativeMethods.TryGetDecoderName` (`libraw_get_decoder_info`; `None` on error or a null name).
    fn decoder_name(&self) -> Option<String>;
    /// C# `LibRawNativeMethods.TryGetRawStructure` (`libraw_get_iparams`, only on the exact pinned runtime, plausibility-checked).
    fn raw_structure(&self) -> Option<RawStructure>;
    /// `libraw_dcraw_make_mem_image`: `Err(error_code)` when it returns null.
    fn make_mem_image(&mut self) -> Result<Box<dyn ProcessedImage>, i32>;
    /// `libraw_dcraw_make_mem_thumb`: `Err(error_code)` when it returns null.
    fn make_mem_thumb(&mut self) -> Result<Box<dyn ProcessedImage>, i32>;
}

/// The loaded library (W5-15a `native::NativeLibRaw`; fakes in tests). Process-wide and shareable.
pub trait LibRawApi: Send + Sync {
    /// `libraw_init(0)`. `Err(InvalidOperation` [`INIT_FAILED_MESSAGE`]`)` when it returns null (C# `CreateHandle`); an
    /// unloadable library is `Err(DllNotFound | EntryPointNotFound)` (C# P/Invoke throws those, and they stay FALLBACKABLE,
    /// INV-12), see [`map_native_error`].
    fn init(&self) -> Result<Box<dyn LibRawSession>, DecodeError>;
    /// C# `LibRawNativeMethods.FormatError`: `libraw_strerror(code)`, or `LibRaw error {code}` when that is null/blank.
    fn format_error(&self, error_code: i32) -> String;
}

/// What the pre-decode admission reads from the opened handle (C# `ILibRawProbe`; a fake stands in for the native handle).
/// Read order inside [`LibRawDecoder::admit_decode`]: `width`, `height`, then (only when both are positive) `decoder_name`,
/// `structure`, `raw_width`, `raw_height`.
pub trait LibRawProbe {
    fn width(&self) -> i32;
    fn height(&self) -> i32;
    fn raw_width(&self) -> i32;
    fn raw_height(&self) -> i32;
    fn decoder_name(&self) -> Option<String>;
    fn structure(&self) -> Option<RawStructure>;
}

/// C# `NativeProbe(SafeLibRawHandle)`: the probe over a real session (`iwidth`, `iheight`, `raw_width`, `raw_height`,
/// `decoder_name`, `raw_structure`).
struct SessionProbe<'a> {
    session: &'a dyn LibRawSession,
}

impl LibRawProbe for SessionProbe<'_> {
    fn width(&self) -> i32 {
        self.session.iwidth()
    }

    fn height(&self) -> i32 {
        self.session.iheight()
    }

    fn raw_width(&self) -> i32 {
        self.session.raw_width()
    }

    fn raw_height(&self) -> i32 {
        self.session.raw_height()
    }

    fn decoder_name(&self) -> Option<String> {
        self.session.decoder_name()
    }

    fn structure(&self) -> Option<RawStructure> {
        self.session.raw_structure()
    }
}

/// The resampled BGRA staging buffer handed to [`LibRawDecoder::hand_off`] (C# `RgbBgraResampler.BgraBuffer`, W5-17). The porter
/// adds `impl StagedPixels for crate::<W5-17 module>::BgraBuffer` here once W5-17 is merged; tests use a fake.
pub trait StagedPixels {
    /// C# `TakePixels()`: moves the `PixelBuffer` out exactly once (no copy); a second call is `Err(DecodeError::Disposed { .. })`.
    fn take_pixels(&mut self) -> Result<PixelBuffer, DecodeError>;
}

/// C# `Action<string>? stageObserver` (test seam): told each [`STAGE_CONFIGURED`]... name as the native stage completes. `Err`
/// stands for the C# observer throwing: the decode stops with that error (an `OutOfMemory` becomes [`NOT_ENOUGH_MEMORY_MESSAGE`],
/// see [`LibRawDecoder::decode_cancellable`]).
pub type StageObserver = Arc<dyn Fn(&str) -> Result<(), DecodeError> + Send + Sync>;

/// C# `CoreOverride` (test seam): replaces the native decode body so the gate/lease handling of
/// [`LibRawDecoder::decode_cancellable`] runs without LibRaw.
pub type CoreOverride = Arc<
    dyn Fn(&DecodeRequest, &FullDecodeLease, &CancelToken) -> Result<Arc<dyn DecodedImageView>, DecodeError>
        + Send
        + Sync,
>;

// ------------------------------------------------------------------------------------------------ decoder

/// C# `public sealed class LibRawDecoder : ICancellableImageDecoder`. Shareable (`Send + Sync`): every decode creates its own
/// LibRaw handle; decodes are serialised by the gate.
pub struct LibRawDecoder {
    /// C# `_codec` (C-02, required): takes over the BGRA `PixelBuffer` without a copy.
    codec: Arc<dyn PlatformImageCodec>,
    /// The loaded LibRaw (W5-15a in production).
    api: Arc<dyn LibRawApi>,
    /// C# `_stageObserver`.
    stage_observer: Option<StageObserver>,
    /// C# `MemoryInfo` (default `MemoryHeadroom.ReadGcMemoryInfo`): no default here, the composition root passes the Windows
    /// probe (WicDirectDecoder precedent).
    memory_info: Arc<dyn MemoryInfoSource>,
    /// The process-wide [`FULL_DECODE_GATE`] (a clone sharing it) unless a test injected its own with [`Self::with_gate`].
    gate: FullDecodeGate,
    /// C# `CoreOverride`.
    core_override: Option<CoreOverride>,
}

impl fmt::Debug for LibRawDecoder {
    /// Codec name, whether an observer / override is set, and the gate (closures have no `Debug`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibRawDecoder")
            .field("codec", &self.codec.name())
            .field("has_stage_observer", &self.stage_observer.is_some())
            .field("has_core_override", &self.core_override.is_some())
            .field("gate", &self.gate)
            .finish()
    }
}

impl LibRawDecoder {
    /// `new LibRawDecoder(codec)` plus the C# `init` property `MemoryInfo` (required here). Uses the process-wide gate.
    pub fn new(
        codec: Arc<dyn PlatformImageCodec>,
        api: Arc<dyn LibRawApi>,
        memory_info: Arc<dyn MemoryInfoSource>,
    ) -> Self {
        Self {
            codec,
            api,
            stage_observer: None,
            memory_info,
            gate: FULL_DECODE_GATE.clone(),
            core_override: None,
        }
    }

    /// `new LibRawDecoder(codec, stageObserver)` (test seam).
    #[must_use]
    pub fn with_stage_observer(mut self, observer: StageObserver) -> Self {
        self.stage_observer = Some(observer);
        self
    }

    /// C# `CoreOverride = ...` (test seam).
    #[must_use]
    pub fn with_core_override(mut self, core: CoreOverride) -> Self {
        self.core_override = Some(core);
        self
    }

    /// Rust-only test seam: a private gate instead of the process-wide one, so tests that count slots can run in parallel.
    #[must_use]
    pub fn with_gate(mut self, gate: FullDecodeGate) -> Self {
        self.gate = gate;
        self
    }

    /// C# `FullDecodeSlotsAvailable` (static): free slots of the PROCESS-WIDE gate.
    pub fn full_decode_slots_available() -> i32 {
        FULL_DECODE_GATE.slots_available()
    }

    /// C# `FullDecodeQueuedWaiters` (static): `queued_viewers() + queued_preloads()` of the process-wide gate.
    pub fn full_decode_queued_waiters() -> i32 {
        FULL_DECODE_GATE.queued_viewers() + FULL_DECODE_GATE.queued_preloads()
    }

    /// The gate this decoder uses (test helper; the process-wide one unless [`Self::with_gate`]).
    pub fn gate(&self) -> &FullDecodeGate {
        &self.gate
    }

    /// C# `Decode(request, ct)`. In this order:
    /// 1. blank `request.path` => [`blank_argument_error`]`("request.Path")` (`InvalidArgument`);
    /// 2. `!request.apply_orientation` => `NotSupported` [`ORIENTATION_REQUIRED_MESSAGE`];
    /// 3. `cancel.check()` => `Canceled`;
    /// 4. `self.gate.enter(request.priority, cancel)` (`GateError` -> `DecodeError` via W5-16a's `From`: `Busy` / `Canceled`);
    /// 5. `core_override` if set, else [`Self::decode_core`], with the lease held; the lease is released on EVERY exit (`Drop`);
    /// 6. an `Err(DecodeError::OutOfMemory { .. })` from step 5 becomes `InvalidOperation` [`NOT_ENOUGH_MEMORY_MESSAGE`]
    ///    (C# `catch (OutOfMemoryException)`; no inner error in Rust). Every other error passes unchanged.
    pub fn decode_with_cancel(
        &self,
        request: &DecodeRequest,
        cancel: &CancelToken,
    ) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if is_blank_path(request.path.as_os_str()) {
            return Err(blank_argument_error("request.Path"));
        }
        if !request.apply_orientation {
            return Err(DecodeError::NotSupported {
                message: ORIENTATION_REQUIRED_MESSAGE.to_owned(),
                user_text: None,
            });
        }
        canceled_check(cancel)?;
        // The lease is released on every exit of this function (`Drop`), also when the body panics.
        let lease = self.gate.enter(request.priority, cancel)?;
        let result = match &self.core_override {
            Some(core) => core(request, &lease, cancel),
            None => self.decode_core(request, &lease, cancel),
        };
        match result {
            Err(error) if matches!(error.kind(), DecodeError::OutOfMemory { .. }) => Err(not_enough_memory()),
            other => other,
        }
    }

    /// C# `DecodeCore`: the native pipeline under the lease (contract semantics 3; stage order is pinned by tests):
    /// state -> init -> progress handler -> [`Self::configure_output`] (`configured`) -> open (buffer or file) -> `opened` ->
    /// [`Self::admit_decode`] -> unpack -> `unpacked` -> check -> process -> `processed` -> check -> make_mem_image -> drop the
    /// session -> `handle-closed` -> bitmap checks -> fit -> resample (W5-17) -> drop the image -> [`Self::hand_off`].
    fn decode_core(
        &self,
        request: &DecodeRequest,
        lease: &FullDecodeLease,
        cancel: &CancelToken,
    ) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let format_error = |code: i32| self.api.format_error(code);
        let mut session = self.api.init()?;
        session.set_progress_handler(cancel.clone());
        // The output parameters must be set BEFORE the open (trap T3): identify() adopts the embedded camera matrix only then.
        self.configure_output(&mut *session)?;
        match &request.bytes {
            // The session keeps the bytes alive until after `libraw_close` (trap T5).
            Some(bytes) => {
                let code = session.open_buffer(Arc::clone(bytes));
                check_result(code, "open RAW buffer", cancel, &format_error)?;
            }
            None => open_file_with(
                &mut |path: &OsStr| session.open_wfile(path),
                request.path.as_os_str(),
                cancel,
                &format_error,
            )?,
        }
        self.stage(STAGE_OPENED)?;
        self.admit_decode(&SessionProbe { session: &*session }, request, lease)?;
        check_result(session.unpack(), "unpack RAW data", cancel, &format_error)?;
        self.stage(STAGE_UNPACKED)?;
        canceled_check(cancel)?;
        check_result(session.dcraw_process(), "process RAW data", cancel, &format_error)?;
        self.stage(STAGE_PROCESSED)?;
        canceled_check(cancel)?;

        let image = match session.make_mem_image() {
            Ok(image) => image,
            Err(code) => {
                canceled_check(cancel)?;
                return Err(create_failure(code, "create decoded image", &format_error));
            }
        };
        let header = image.header();
        // libraw_close frees the 8 B/px working image (and only then the pinned bytes) BEFORE any BGRA allocation (trap T4).
        drop(session);
        self.stage(STAGE_HANDLE_CLOSED)?;

        if !is_supported_bitmap(&header) {
            return Err(DecodeError::InvalidData {
                message: UNSUPPORTED_PROCESSED_IMAGE_MESSAGE.to_owned(),
            });
        }
        let (width, height, colors) = (
            i32::from(header.width),
            i32::from(header.height),
            i32::from(header.colors),
        );
        let rgb_length = resampler::validate_source_length(width, height, i64::from(header.data_size), colors)
            .map_err(map_resample_error)?;
        let rgb_length = usize::try_from(rgb_length).map_err(|_| invalid_buffer_length())?;
        let (target_width, target_height) = request.decode_box().fit(width, height);
        resampler::validate_target_length(target_width, target_height).map_err(map_resample_error)?;
        let source = image.data().get(..rgb_length).ok_or_else(invalid_buffer_length)?;
        let mut pixels =
            resampler::resize_to_buffer(source, width, height, target_width, target_height, colors, cancel)
                .map_err(map_resample_error)?;
        // The RGB buffer is freed as soon as the BGRA pixels exist, before they are handed on.
        drop(image);
        let downscaled = is_downscaled(target_width, target_height, width, height);
        self.hand_off(&mut pixels, downscaled, width, height, &|| true)
    }

    /// C# `AdmitDecode`: [`Self::ensure_memory_headroom`] first, then (only when it passes) `lease.mark_worked()`. A refusal is an
    /// instant failure that must not age queued preloads.
    pub fn admit_decode(
        &self,
        probe: &dyn LibRawProbe,
        request: &DecodeRequest,
        lease: &FullDecodeLease,
    ) -> Result<(), DecodeError> {
        self.ensure_memory_headroom(probe, request)?;
        // Past the guard: real decode work starts, so this lease counts for the gate's preload aging.
        lease.mark_worked();
        Ok(())
    }

    /// C# `EnsureMemoryHeadroom`: `width`/`height` from the probe; either `<= 0` => `Ok` (nothing to estimate from, nothing else
    /// read). Else `(tw, th) = request.decode_box().fit(w, h)`, `memory_info.read()`, `family = memory_guard::classify(decoder_name,
    /// structure)`, `estimate = memory_guard::estimate_peak_bytes(w, h, tw, th, raw_width, raw_height, family)`; `!memory_headroom::
    /// has_headroom(estimate, total, load)` => `InvalidOperation` [`NOT_ENOUGH_MEMORY_MESSAGE`] (NOT `MemoryAdmission`: C# throws
    /// `InvalidOperationException`).
    fn ensure_memory_headroom(&self, probe: &dyn LibRawProbe, request: &DecodeRequest) -> Result<(), DecodeError> {
        let width = probe.width();
        let height = probe.height();
        if width <= 0 || height <= 0 {
            return Ok(()); // unknown size: nothing to estimate from
        }
        let (target_width, target_height) = request.decode_box().fit(width, height);
        let info = self.memory_info.read();
        let family = memory_guard::classify(probe.decoder_name().as_deref(), probe.structure());
        let estimate = memory_guard::estimate_peak_bytes(
            width,
            height,
            target_width,
            target_height,
            probe.raw_width(),
            probe.raw_height(),
            family,
        );
        if memory_headroom::has_headroom(estimate, info.total_available, info.load) {
            Ok(())
        } else {
            Err(not_enough_memory())
        }
    }

    /// C# `ConfigureOutput` (BEFORE open, see contract trap T3): `set_output_color(1)` (sRGB), `set_output_bps(8)`,
    /// `set_no_auto_bright(1)`, `try_set_use_camera_wb(1, 8, 1)` (result ignored), then stage `configured`.
    fn configure_output(&self, session: &mut dyn LibRawSession) -> Result<(), DecodeError> {
        session.set_output_color(1); // sRGB
        session.set_output_bps(8);
        session.set_no_auto_bright(1);
        // As-shot white balance; only written on the exact pinned LibRaw (the session decides). The result is ignored.
        let _ = session.try_set_use_camera_wb(1, 8, 1);
        self.stage(STAGE_CONFIGURED)
    }

    /// C# `HandOff`: the BGRA pixels leave this decoder, ownership moves to the codec without a copy.
    /// 1. stage `source-released` when `source_released()` else `source-held` (BEFORE the pixels are taken);
    /// 2. `buffer = pixels.take_pixels()?`; remember `width`, `height`, `byte_count`;
    /// 3. `codec.image_from_pixels(buffer)?` (the codec owns the buffer; on `Err` it is already dropped = C# `buffer.Dispose()`);
    /// 4. stage `bitmap-created`;
    /// 5. `DecodedImage::new(image, width, height, byte_count, { downscaled, orientation: 1, actual_backend: LibRaw,
    ///    original_width, original_height, .. })` (a `DecodedImageError` is unreachable; map it to `InvalidOperation`).
    pub fn hand_off(
        &self,
        pixels: &mut dyn StagedPixels,
        downscaled: bool,
        original_width: i32,
        original_height: i32,
        source_released: &dyn Fn() -> bool,
    ) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.stage(if source_released() {
            STAGE_SOURCE_RELEASED
        } else {
            STAGE_SOURCE_HELD
        })?;
        let buffer = pixels.take_pixels()?;
        let (width, height, byte_count) = (buffer.width(), buffer.height(), buffer.byte_count());
        // The codec owns the buffer; on `Err` it is already dropped (C# `buffer.Dispose()`).
        let platform_image = self.codec.image_from_pixels(buffer)?;
        self.stage(STAGE_BITMAP_CREATED)?;
        let options = DecodedImageOptions {
            downscaled,
            orientation: 1,
            actual_backend: DecoderBackend::LibRaw,
            original_width,
            original_height,
            ..DecodedImageOptions::default()
        };
        let image = DecodedImage::new(platform_image, width, height, byte_count, options).map_err(|error| {
            DecodeError::InvalidOperation {
                message: error.to_string(),
            }
        })?;
        Ok(Arc::new(image))
    }

    /// C# `ReadJpegThumbnail(path, ct)` over this decoder's library: [`read_jpeg_thumbnail`]`(&*self.api, path, cancel)`.
    pub fn read_jpeg_thumbnail(&self, path: &str, cancel: &CancelToken) -> Result<Vec<u8>, DecodeError> {
        read_jpeg_thumbnail(&*self.api, path, cancel)
    }

    /// Reports `stage` to the observer, if any (`Err` = the observer "threw").
    fn stage(&self, stage: &str) -> Result<(), DecodeError> {
        match &self.stage_observer {
            Some(observer) => observer(stage),
            None => Ok(()),
        }
    }
}

impl ImageDecoder for LibRawDecoder {
    /// C# `Decode(request)` = `Decode(request, CancellationToken.None)`.
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.decode_with_cancel(request, &CancelToken::none())
    }

    /// C# `ReadInfo(path)`: blank path => [`blank_argument_error`]`("path")`; init (null => `InvalidOperation`
    /// [`INIT_FAILED_MESSAGE`]); open with the long-path retry (no progress handler, `CancelToken::none()`); `adjust_sizes_info_only`
    /// checked as `"read RAW dimensions"`; `iwidth`/`iheight` (the container flip is already applied: a portrait file reports
    /// height > width) through [`validate_image_dimensions`]; `ImageInfo::new(w, h)` (orientation 1).
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        if is_blank_path(OsStr::new(path)) {
            return Err(blank_argument_error("path"));
        }
        let format_error = |code: i32| self.api.format_error(code);
        let none = CancelToken::none();
        let mut session = self.api.init()?;
        open_file_with(
            &mut |p: &OsStr| session.open_wfile(p),
            OsStr::new(path),
            &none,
            &format_error,
        )?;
        check_result(
            session.adjust_sizes_info_only(),
            "read RAW dimensions",
            &none,
            &format_error,
        )?;
        // adjust_sizes_info_only applies the container flip to iwidth/iheight, exactly the size `decode` returns, so the
        // orientation is already folded in: report it as 1.
        let (width, height) = (session.iwidth(), session.iheight());
        validate_image_dimensions(width, height)?;
        Ok(ImageInfo::new(width, height))
    }

    /// C# `LibRawDecoder : ICancellableImageDecoder`.
    fn as_cancellable(&self) -> Option<&dyn CancellableImageDecoder> {
        Some(self)
    }
}

impl CancellableImageDecoder for LibRawDecoder {
    fn decode_cancellable(
        &self,
        request: &DecodeRequest,
        token: &CancelToken,
    ) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        self.decode_with_cancel(request, token)
    }
}

// ------------------------------------------------------------------------------------------------ thumbnail

/// C# `static ReadJpegThumbnail(path, ct)`: LibRaw's embedded JPEG thumbnail, without demosaicing. NOT behind the gate.
/// Order: blank path => [`blank_argument_error`]`("path")`; `cancel.check()`; init; `set_progress_handler(cancel.clone())`; open (long-path
/// retry, cancel-aware); `unpack_thumb` checked as `"unpack embedded thumbnail"`; `cancel.check()`; `make_mem_thumb`
/// (`Err(code)` => `cancel.check()` then [`create_thumbnail_failure`]); [`validate_thumbnail_header`]; copy `data_size` bytes;
/// [`trim_to_end_of_image`]; `cancel.check()`.
pub fn read_jpeg_thumbnail(api: &dyn LibRawApi, path: &str, cancel: &CancelToken) -> Result<Vec<u8>, DecodeError> {
    if is_blank_path(OsStr::new(path)) {
        return Err(blank_argument_error("path"));
    }
    canceled_check(cancel)?;
    let format_error = |code: i32| api.format_error(code);
    let mut session = api.init()?;
    session.set_progress_handler(cancel.clone());
    open_file_with(
        &mut |p: &OsStr| session.open_wfile(p),
        OsStr::new(path),
        cancel,
        &format_error,
    )?;
    check_result(
        session.unpack_thumb(),
        "unpack embedded thumbnail",
        cancel,
        &format_error,
    )?;
    canceled_check(cancel)?;
    let thumbnail = match session.make_mem_thumb() {
        Ok(thumbnail) => thumbnail,
        Err(code) => {
            canceled_check(cancel)?;
            return Err(create_thumbnail_failure(code, &format_error));
        }
    };
    let header = thumbnail.header();
    validate_thumbnail_header(&header)?;
    let length = usize::try_from(header.data_size).map_err(|_| invalid_buffer_length())?;
    let bytes = thumbnail
        .data()
        .get(..length)
        .ok_or_else(invalid_buffer_length)?
        .to_vec();
    let jpeg = trim_to_end_of_image(bytes)?;
    canceled_check(cancel)?;
    Ok(jpeg)
}

/// C# `ValidateThumbnailHeader`: `Ok` only for `image_type == LIBRAW_IMAGE_JPEG` and `4 <= data_size <= MAX_THUMBNAIL_BYTES`;
/// otherwise `InvalidData` `LibRaw returned an unsupported or oversized thumbnail (type={t}, width={w}, height={h}, bytes={n}).`
pub fn validate_thumbnail_header(header: &ProcessedImageHeader) -> Result<(), DecodeError> {
    if header.image_type != LIBRAW_IMAGE_JPEG || header.data_size < 4 || header.data_size > MAX_THUMBNAIL_BYTES {
        return Err(DecodeError::InvalidData {
            message: format!(
                "LibRaw returned an unsupported or oversized thumbnail (type={}, width={}, height={}, bytes={}).",
                header.image_type, header.width, header.height, header.data_size
            ),
        });
    }
    Ok(())
}

/// C# `TrimToEndOfImage`: [`find_end_of_image`] then keeps that prefix. An unpadded stream comes back as the same allocation (C#
/// returns the same array); a padded one is copied to an exact-size `Vec` (C# `bytes[..end]`).
pub fn trim_to_end_of_image(bytes: Vec<u8>) -> Result<Vec<u8>, DecodeError> {
    let end = find_end_of_image(&bytes)?;
    if end == bytes.len() {
        return Ok(bytes);
    }
    // C# `bytes[..end]` copies: the golden `same` flag is false for every padded stream (a `Vec::truncate` could keep the
    // allocation, which the pointer-identity golden test rejects). The padded original is freed here.
    Ok(bytes[..end].to_vec())
}

/// The length [`trim_to_end_of_image`] keeps (pure, golden-tested). `InvalidData` [`INCOMPLETE_JPEG_MESSAGE`] when:
/// `len < 4` or the stream does not start with `FF D8`; or neither the structure walk nor the structure-free fallback finds an EOI.
/// Fast path: strip trailing `00`/`FF` bytes (never below 2); if the rest ends with `FF D9` (and is at least 4 long) keep it.
/// Else [`find_end_of_image_by_structure`], else [`find_trailing_eoi_with_structure_free_tail`]. Never panics, never reads
/// past the slice (every index is checked as in C#).
pub fn find_end_of_image(bytes: &[u8]) -> Result<usize, DecodeError> {
    let len = bytes.len();
    if len < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err(incomplete_jpeg());
    }
    let mut end = len;
    while end > 2 && matches!(bytes[end - 1], 0x00 | 0xFF) {
        end -= 1;
    }
    if end >= 4 && bytes[end - 1] == 0xD9 && bytes[end - 2] == 0xFF {
        return Ok(end);
    }
    find_end_of_image_by_structure(bytes)
        .or_else(|| find_trailing_eoi_with_structure_free_tail(bytes))
        .ok_or_else(incomplete_jpeg)
}

/// C# `FindEndOfImageByStructure`: index just past the EOI found by walking the segments from offset 2, or `None` when the
/// structure ends (truncated/invalid) first. Fill `FF`s before a marker are skipped; `D9` ends; `00`, `01`, `D8`, `D0..=D7` carry
/// no length; a length `< 2` or past the end stops; after `DA` the entropy data is skipped (`FF 00`, `FF D0..=D7` and `FF FF`
/// continue it, any other marker ends it).
fn find_end_of_image_by_structure(b: &[u8]) -> Option<usize> {
    let len = b.len();
    let mut i = 2usize;
    loop {
        if i >= len || b[i] != 0xFF {
            return None;
        }
        while i < len && b[i] == 0xFF {
            i += 1; // fill bytes before a marker
        }
        if i >= len {
            return None;
        }
        let marker = b[i];
        i += 1;
        if marker == 0xD9 {
            return Some(i);
        }
        if matches!(marker, 0x00 | 0x01 | 0xD8 | 0xD0..=0xD7) {
            continue; // standalone markers carry no length
        }
        if i + 2 > len {
            return None;
        }
        let segment_length = (usize::from(b[i]) << 8) | usize::from(b[i + 1]);
        if segment_length < 2 {
            return None;
        }
        i += segment_length;
        if i > len {
            return None;
        }
        if marker != 0xDA {
            continue;
        }
        // Entropy-coded data: an FF is followed by 00 (stuffed), RSTn or more FF; any other marker ends the scan.
        loop {
            if i >= len {
                return None;
            }
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            if i + 1 >= len {
                return None;
            }
            match b[i + 1] {
                0x00 | 0xD0..=0xD7 => i += 2,
                0xFF => i += 1,
                _ => break,
            }
        }
    }
}

/// C# `FindTrailingEoiWithStructureFreeTail`: scanning backwards from `len - 2` down to `max(2, len - 64 KiB)`, the LAST `FF D9`
/// whose tail (bytes after it, pairs `FF xx` with `j < len - 1`) contains no `D8`, `DA`, `DB`, `DD`, `FE`, `C0..=CF` or `E0..=EF`
/// marker; `Some(i + 2)`. The first (= last in the file) `FF D9` found decides: a structured tail => `None` (no earlier EOI tried).
fn find_trailing_eoi_with_structure_free_tail(b: &[u8]) -> Option<usize> {
    let len = b.len();
    if len < 2 {
        return None;
    }
    let start = len.saturating_sub(TRAILING_EOI_SEARCH_WINDOW).max(2);
    for i in (start..=len - 2).rev() {
        if b[i] != 0xFF || b[i + 1] != 0xD9 {
            continue;
        }
        // SOI, SOFn/DHT (C0-CF), SOS, DQT, DRI, APPn, COM: the tail is more image, not padding.
        let structured = (i + 2..len - 1)
            .any(|j| b[j] == 0xFF && matches!(b[j + 1], 0xD8 | 0xDA | 0xDB | 0xDD | 0xFE | 0xC0..=0xCF | 0xE0..=0xEF));
        return if structured { None } else { Some(i + 2) };
    }
    None
}

// ------------------------------------------------------------------------------------------------ header checks

/// C# `ValidateImageDimensions`: `InvalidData` [`INVALID_DIMENSIONS_MESSAGE`] when `width <= 0 || height <= 0`.
pub fn validate_image_dimensions(width: i32, height: i32) -> Result<(), DecodeError> {
    if width <= 0 || height <= 0 {
        return Err(DecodeError::InvalidData {
            message: INVALID_DIMENSIONS_MESSAGE.to_owned(),
        });
    }
    Ok(())
}

/// C# `IsSupportedBitmap`: `image_type == LIBRAW_IMAGE_BITMAP && width != 0 && height != 0 && colors in {1, 3}` (W5-17
/// `is_supported_channel_count`) `&& bits == 8`.
pub fn is_supported_bitmap(header: &ProcessedImageHeader) -> bool {
    header.image_type == LIBRAW_IMAGE_BITMAP
        && header.width != 0
        && header.height != 0
        && resampler::is_supported_channel_count(i32::from(header.colors))
        && header.bits == 8
}

/// C# `IsDownscaled`: `target_width < source_width || target_height < source_height`.
pub fn is_downscaled(target_width: i32, target_height: i32, source_width: i32, source_height: i32) -> bool {
    target_width < source_width || target_height < source_height
}

// ------------------------------------------------------------------------------------------------ open, results, errors

/// C# `OpenFile(Func<string, int> open, path, ct)`: `code = open(path)`; when `code == LIBRAW_IO_ERROR` and
/// [`to_extended_length_path`] gives a form and `open(extended) == 0` => `Ok` (exactly two calls). Otherwise
/// [`check_result`]`(code, "open RAW file", ..)` with the ORIGINAL code (a failed retry reports the first error).
/// `path.to_str()` is `None` (a lone surrogate) => no retry (DIVERGENCE, contract trap T7).
pub fn open_file_with(
    open: &mut dyn FnMut(&OsStr) -> i32,
    path: &OsStr,
    cancel: &CancelToken,
    format_error: &dyn Fn(i32) -> String,
) -> Result<(), DecodeError> {
    let code = open(path);
    if code == LIBRAW_IO_ERROR {
        // DIVERGENCE: a path that is not valid Unicode (`to_str()` is `None`) is opened once and never retried (C# can retry it).
        if let Some(extended) = path.to_str().and_then(to_extended_length_path) {
            if open(OsStr::new(&extended)) == 0 {
                return Ok(());
            }
        }
    }
    check_result(code, "open RAW file", cancel, format_error)
}

/// C# `ToExtendedLengthPath`: the `\\?\` form of a long fully qualified path, else `None`. `None` when the length in UTF-16 code
/// units is `< LONG_PATH_THRESHOLD`, when it starts with `\\?\` or `\\.\` (ordinal), when it is not fully qualified
/// (`dotnet_path::is_path_fully_qualified`) or when `dotnet_path::get_full_path` fails. A full path starting with `\\` becomes
/// `\\?\UNC\` + `full[2..]`, any other `\\?\` + `full`. Never panics.
pub fn to_extended_length_path(path: &str) -> Option<String> {
    if path.encode_utf16().count() < LONG_PATH_THRESHOLD || path.starts_with(r"\\?\") || path.starts_with(r"\\.\") {
        return None;
    }
    if !dotnet_path::is_path_fully_qualified(path) {
        return None;
    }
    // Collapses "." / ".." and forward slashes, which extended-length paths do not do.
    let full = dotnet_path::get_full_path(path).ok()?;
    Some(match full.strip_prefix(r"\\") {
        Some(rest) => format!(r"\\?\UNC\{rest}"),
        None => format!(r"\\?\{full}"),
    })
}

/// C# `CheckResult(code, operation, ct)`: `0` => `Ok`; otherwise `cancel.is_canceled()` => `Canceled`, else
/// [`create_failure`]`(code, operation, format_error)`.
pub fn check_result(
    error_code: i32,
    operation: &str,
    cancel: &CancelToken,
    format_error: &dyn Fn(i32) -> String,
) -> Result<(), DecodeError> {
    if error_code == 0 {
        return Ok(());
    }
    if cancel.is_canceled() {
        return Err(DecodeError::Canceled);
    }
    Err(create_failure(error_code, operation, format_error))
}

/// C# `CreateFailure`: `LIBRAW_UNSUFFICIENT_MEMORY` or `ERRNO_NO_MEMORY` => `InvalidOperation` [`NOT_ENOUGH_MEMORY_MESSAGE`]
/// (a resource failure of a healthy file; `format_error` NOT called); any other code => `InvalidData`
/// `LibRaw could not {operation}: {format_error(code)}`.
pub fn create_failure(error_code: i32, operation: &str, format_error: &dyn Fn(i32) -> String) -> DecodeError {
    if error_code == LIBRAW_UNSUFFICIENT_MEMORY || error_code == ERRNO_NO_MEMORY {
        return not_enough_memory();
    }
    DecodeError::InvalidData {
        message: format!("LibRaw could not {operation}: {}", format_error(error_code)),
    }
}

/// C# `CreateThumbnailFailure`: [`create_failure`]`(code, "create embedded JPEG thumbnail", ..)`.
pub fn create_thumbnail_failure(error_code: i32, format_error: &dyn Fn(i32) -> String) -> DecodeError {
    create_failure(error_code, "create embedded JPEG thumbnail", format_error)
}

/// C# `ArgumentException.ThrowIfNullOrWhiteSpace(<param>)`: `InvalidArgument` with
/// `"{EMPTY_OR_WHITE_SPACE_MESSAGE} (Parameter '{param_name}')"`.
pub fn blank_argument_error(param_name: &str) -> DecodeError {
    DecodeError::InvalidArgument {
        message: format!("{EMPTY_OR_WHITE_SPACE_MESSAGE} (Parameter '{param_name}')"),
    }
}

/// .NET `string.IsNullOrWhiteSpace` on a path: empty, or every char `char::is_whitespace` (same set as .NET `char.IsWhiteSpace`).
/// Non-UTF-8 paths are not blank (they contain a non-whitespace unit).
pub fn is_blank_path(path: &OsStr) -> bool {
    path.to_str().is_some_and(|text| text.chars().all(char::is_whitespace))
}

// ------------------------------------------------------------------------------------------------ W5-17 hand-off glue

/// The W5-17 staging buffer is the production [`StagedPixels`]: `BgraBuffer::take_pixels` mapped through [`map_resample_error`].
impl StagedPixels for BgraBuffer {
    fn take_pixels(&mut self) -> Result<PixelBuffer, DecodeError> {
        BgraBuffer::take_pixels(self).map_err(map_resample_error)
    }
}

/// W5-17 `ResampleError` -> the error C# `Decode` lets out: `InvalidData(m)` => `InvalidData { m }`; `Canceled` => `Canceled`;
/// `Overflow` => `Overflow { e.to_string() }`; `Disposed` => `Disposed { e.to_string() }`; `PixelBuffer(OutOfMemory { .. })` =>
/// `OutOfMemory { e.to_string() }` (C# `OutOfMemoryException`, which `decode_with_cancel` step 6 turns into `InvalidOperation`
/// [`NOT_ENOUGH_MEMORY_MESSAGE`]); any other `PixelBuffer(_)` => `InvalidArgument { e.to_string() }`
/// (`ArgumentOutOfRangeException`; unreachable after `validate_target_length`).
pub fn map_resample_error(error: ResampleError) -> DecodeError {
    match error {
        ResampleError::InvalidData(message) => DecodeError::InvalidData { message },
        ResampleError::Canceled => DecodeError::Canceled,
        ResampleError::Overflow => DecodeError::Overflow {
            message: error.to_string(),
        },
        ResampleError::Disposed => DecodeError::Disposed {
            message: error.to_string(),
        },
        ResampleError::PixelBuffer(PixelBufferError::OutOfMemory { .. }) => DecodeError::OutOfMemory {
            message: error.to_string(),
        },
        ResampleError::PixelBuffer(_) => DecodeError::InvalidArgument {
            message: error.to_string(),
        },
    }
}

// ------------------------------------------------------------------------------------------------ production seams (W5-15a)

/// Production [`LibRawApi`] over W5-15a `native` (W5-15a contract "Downstream": the adapter is owned HERE). Name clash:
/// W5-15a's `struct native::LibRawApi` (the fn-pointer table) vs this file's `trait LibRawApi`; always write `native::...`
/// qualified. The composition root builds `LibRawDecoder::new(codec, Arc::new(NativeLibRaw), memory_probe)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeLibRaw;

impl LibRawApi for NativeLibRaw {
    /// `native::LibRawHandle::init(0)` (`'static`: buffers go through `open_shared_buffer`) boxed as a `NativeSession`;
    /// `Err(e)` => [`map_native_error`]`(&e)`.
    fn init(&self) -> Result<Box<dyn LibRawSession>, DecodeError> {
        match native::LibRawHandle::init(0) {
            Ok(handle) => Ok(Box::new(NativeSession { handle })),
            Err(error) => Err(map_native_error(&error)),
        }
    }

    /// `native::format_error(code)`.
    fn format_error(&self, error_code: i32) -> String {
        native::format_error(error_code)
    }
}

/// `native::NativeError` -> `DecodeError`: `InitReturnedNull` => `InvalidOperation` [`INIT_FAILED_MESSAGE`];
/// `Unavailable(MissingExport { name })` => `EntryPointNotFound { "Unable to find an entry point named '{name}' in DLL
/// 'libraw.dll'." }`; any other `Unavailable(e)` => `DllNotFound { "Unable to load DLL 'libraw.dll' or one of its
/// dependencies: {e}" }`. (C# P/Invoke throws `DllNotFoundException` / `EntryPointNotFoundException` there; both are
/// fallbackable, INV-12. The texts are not golden-pinned.)
pub fn map_native_error(error: &native::NativeError) -> DecodeError {
    match error {
        native::NativeError::InitReturnedNull => DecodeError::InvalidOperation {
            message: INIT_FAILED_MESSAGE.to_owned(),
        },
        native::NativeError::Unavailable(native::LoadError::MissingExport { name }) => {
            DecodeError::EntryPointNotFound {
                message: format!(
                    "Unable to find an entry point named '{name}' in DLL '{}'.",
                    native::LIBRARY_NAME
                ),
            }
        }
        native::NativeError::Unavailable(other) => DecodeError::DllNotFound {
            message: format!(
                "Unable to load DLL '{}' or one of its dependencies: {other}",
                native::LIBRARY_NAME
            ),
        },
    }
}

/// One `native::LibRawHandle<'static>` behind the [`LibRawSession`] seam: every method forwards 1:1
/// (`set_progress_handler` -> `set_cancel_token`, `open_buffer` -> `open_shared_buffer`, `make_mem_*` -> `dcraw_make_mem_*`
/// boxed as `NativeImage`). Drop = the handle's drop (`libraw_close`, then the token box and the buffer).
struct NativeSession {
    handle: native::LibRawHandle<'static>,
}

impl LibRawSession for NativeSession {
    fn set_progress_handler(&mut self, cancel: CancelToken) {
        self.handle.set_cancel_token(cancel);
    }

    fn set_output_color(&mut self, value: i32) {
        self.handle.set_output_color(value);
    }

    fn set_output_bps(&mut self, value: i32) {
        self.handle.set_output_bps(value);
    }

    fn set_no_auto_bright(&mut self, value: i32) {
        self.handle.set_no_auto_bright(value);
    }

    fn try_set_use_camera_wb(&mut self, output_color: i32, output_bps: i32, no_auto_bright: i32) -> bool {
        self.handle
            .try_set_use_camera_wb(output_color, output_bps, no_auto_bright)
    }

    fn open_wfile(&mut self, path: &OsStr) -> i32 {
        self.handle.open_wfile(path)
    }

    fn open_buffer(&mut self, bytes: SourceBytes) -> i32 {
        self.handle.open_shared_buffer(bytes)
    }

    fn unpack(&mut self) -> i32 {
        self.handle.unpack()
    }

    fn unpack_thumb(&mut self) -> i32 {
        self.handle.unpack_thumb()
    }

    fn dcraw_process(&mut self) -> i32 {
        self.handle.dcraw_process()
    }

    fn adjust_sizes_info_only(&mut self) -> i32 {
        self.handle.adjust_sizes_info_only()
    }

    fn iwidth(&self) -> i32 {
        self.handle.iwidth()
    }

    fn iheight(&self) -> i32 {
        self.handle.iheight()
    }

    fn raw_width(&self) -> i32 {
        self.handle.raw_width()
    }

    fn raw_height(&self) -> i32 {
        self.handle.raw_height()
    }

    fn decoder_name(&self) -> Option<String> {
        self.handle.decoder_name()
    }

    fn raw_structure(&self) -> Option<RawStructure> {
        self.handle.raw_structure()
    }

    fn make_mem_image(&mut self) -> Result<Box<dyn ProcessedImage>, i32> {
        self.handle
            .dcraw_make_mem_image()
            .map(|image| Box::new(NativeImage { image }) as Box<dyn ProcessedImage>)
    }

    fn make_mem_thumb(&mut self) -> Result<Box<dyn ProcessedImage>, i32> {
        self.handle
            .dcraw_make_mem_thumb()
            .map(|image| Box::new(NativeImage { image }) as Box<dyn ProcessedImage>)
    }
}

/// One `native::LibRawImage` behind the [`ProcessedImage`] seam (Drop = `libraw_dcraw_clear_mem`).
struct NativeImage {
    image: native::LibRawImage,
}

impl ProcessedImage for NativeImage {
    fn header(&self) -> ProcessedImageHeader {
        self.image.header()
    }

    fn data(&self) -> &[u8] {
        self.image.data().unwrap_or(&[])
    }
}

// ------------------------------------------------------------------------------------------------ private helpers

/// `cancel.check()` as a `DecodeError` (C# `ThrowIfCancellationRequested`; there is no `From<Canceled>`).
fn canceled_check(cancel: &CancelToken) -> Result<(), DecodeError> {
    cancel.check().map_err(|_| DecodeError::Canceled)
}

/// Every out-of-memory mapping (C# `InvalidOperationException`).
fn not_enough_memory() -> DecodeError {
    DecodeError::InvalidOperation {
        message: NOT_ENOUGH_MEMORY_MESSAGE.to_owned(),
    }
}

fn incomplete_jpeg() -> DecodeError {
    DecodeError::InvalidData {
        message: INCOMPLETE_JPEG_MESSAGE.to_owned(),
    }
}

/// A processed image whose payload is shorter than its header claims (unreachable with the native adapter).
fn invalid_buffer_length() -> DecodeError {
    DecodeError::InvalidData {
        message: "LibRaw returned an invalid processed image buffer length.".to_owned(),
    }
}
