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
#![allow(unused_variables, dead_code)] // D11: stub bodies are todo!(); the porter removes this line.

use std::ffi::OsStr;
use std::fmt;
use std::sync::{Arc, LazyLock};

use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo, SourceBytes};
use pr_imaging::decoder_traits::{CancellableImageDecoder, DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::memory_headroom::MemoryInfoSource;
use pr_imaging::pixels::PixelBuffer;

use crate::full_decode_gate::{FullDecodeGate, FullDecodeLease};
use crate::memory_guard::RawStructure;
use crate::native;
use crate::resampler::{BgraBuffer, ResampleError};

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
        todo!("self.session.iwidth()")
    }

    fn height(&self) -> i32 {
        todo!("self.session.iheight()")
    }

    fn raw_width(&self) -> i32 {
        todo!("self.session.raw_width()")
    }

    fn raw_height(&self) -> i32 {
        todo!("self.session.raw_height()")
    }

    fn decoder_name(&self) -> Option<String> {
        todo!("self.session.decoder_name()")
    }

    fn structure(&self) -> Option<RawStructure> {
        todo!("self.session.raw_structure()")
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
        todo!("debug_struct(\"LibRawDecoder\") with codec.name(), has_stage_observer, has_core_override")
    }
}

impl LibRawDecoder {
    /// `new LibRawDecoder(codec)` plus the C# `init` property `MemoryInfo` (required here). Uses the process-wide gate.
    pub fn new(
        codec: Arc<dyn PlatformImageCodec>,
        api: Arc<dyn LibRawApi>,
        memory_info: Arc<dyn MemoryInfoSource>,
    ) -> Self {
        todo!("gate: FULL_DECODE_GATE.clone(), stage_observer: None, core_override: None")
    }

    /// `new LibRawDecoder(codec, stageObserver)` (test seam).
    #[must_use]
    pub fn with_stage_observer(self, observer: StageObserver) -> Self {
        todo!()
    }

    /// C# `CoreOverride = ...` (test seam).
    #[must_use]
    pub fn with_core_override(self, core: CoreOverride) -> Self {
        todo!()
    }

    /// Rust-only test seam: a private gate instead of the process-wide one, so tests that count slots can run in parallel.
    #[must_use]
    pub fn with_gate(self, gate: FullDecodeGate) -> Self {
        todo!()
    }

    /// C# `FullDecodeSlotsAvailable` (static): free slots of the PROCESS-WIDE gate.
    pub fn full_decode_slots_available() -> i32 {
        todo!("FULL_DECODE_GATE.slots_available()")
    }

    /// C# `FullDecodeQueuedWaiters` (static): `queued_viewers() + queued_preloads()` of the process-wide gate.
    pub fn full_decode_queued_waiters() -> i32 {
        todo!()
    }

    /// The gate this decoder uses (test helper; the process-wide one unless [`Self::with_gate`]).
    pub fn gate(&self) -> &FullDecodeGate {
        todo!("&self.gate")
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
        todo!("see doc comment and contract semantics 2")
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
        todo!("see contract semantics 3")
    }

    /// C# `AdmitDecode`: [`Self::ensure_memory_headroom`] first, then (only when it passes) `lease.mark_worked()`. A refusal is an
    /// instant failure that must not age queued preloads.
    pub fn admit_decode(
        &self,
        probe: &dyn LibRawProbe,
        request: &DecodeRequest,
        lease: &FullDecodeLease,
    ) -> Result<(), DecodeError> {
        todo!()
    }

    /// C# `EnsureMemoryHeadroom`: `width`/`height` from the probe; either `<= 0` => `Ok` (nothing to estimate from, nothing else
    /// read). Else `(tw, th) = request.decode_box().fit(w, h)`, `memory_info.read()`, `family = memory_guard::classify(decoder_name,
    /// structure)`, `estimate = memory_guard::estimate_peak_bytes(w, h, tw, th, raw_width, raw_height, family)`; `!memory_headroom::
    /// has_headroom(estimate, total, load)` => `InvalidOperation` [`NOT_ENOUGH_MEMORY_MESSAGE`] (NOT `MemoryAdmission`: C# throws
    /// `InvalidOperationException`).
    fn ensure_memory_headroom(&self, probe: &dyn LibRawProbe, request: &DecodeRequest) -> Result<(), DecodeError> {
        todo!()
    }

    /// C# `ConfigureOutput` (BEFORE open, see contract trap T3): `set_output_color(1)` (sRGB), `set_output_bps(8)`,
    /// `set_no_auto_bright(1)`, `try_set_use_camera_wb(1, 8, 1)` (result ignored), then stage `configured`.
    fn configure_output(&self, session: &mut dyn LibRawSession) -> Result<(), DecodeError> {
        todo!()
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
        todo!("see doc comment")
    }

    /// C# `ReadJpegThumbnail(path, ct)` over this decoder's library: [`read_jpeg_thumbnail`]`(&*self.api, path, cancel)`.
    pub fn read_jpeg_thumbnail(&self, path: &str, cancel: &CancelToken) -> Result<Vec<u8>, DecodeError> {
        todo!()
    }

    /// Reports `stage` to the observer, if any (`Err` = the observer "threw").
    fn stage(&self, stage: &str) -> Result<(), DecodeError> {
        todo!()
    }
}

impl ImageDecoder for LibRawDecoder {
    /// C# `Decode(request)` = `Decode(request, CancellationToken.None)`.
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        todo!("self.decode_with_cancel(request, &CancelToken::none())")
    }

    /// C# `ReadInfo(path)`: blank path => [`blank_argument_error`]`("path")`; init (null => `InvalidOperation`
    /// [`INIT_FAILED_MESSAGE`]); open with the long-path retry (no progress handler, `CancelToken::none()`); `adjust_sizes_info_only`
    /// checked as `"read RAW dimensions"`; `iwidth`/`iheight` (the container flip is already applied: a portrait file reports
    /// height > width) through [`validate_image_dimensions`]; `ImageInfo::new(w, h)` (orientation 1).
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        todo!()
    }

    /// C# `LibRawDecoder : ICancellableImageDecoder`.
    fn as_cancellable(&self) -> Option<&dyn CancellableImageDecoder> {
        todo!("Some(self)")
    }
}

impl CancellableImageDecoder for LibRawDecoder {
    fn decode_cancellable(
        &self,
        request: &DecodeRequest,
        token: &CancelToken,
    ) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        todo!("self.decode_with_cancel(request, token)")
    }
}

// ------------------------------------------------------------------------------------------------ thumbnail

/// C# `static ReadJpegThumbnail(path, ct)`: LibRaw's embedded JPEG thumbnail, without demosaicing. NOT behind the gate.
/// Order: blank path => [`blank_argument_error`]`("path")`; `cancel.check()`; init; `set_progress_handler(cancel.clone())`; open (long-path
/// retry, cancel-aware); `unpack_thumb` checked as `"unpack embedded thumbnail"`; `cancel.check()`; `make_mem_thumb`
/// (`Err(code)` => `cancel.check()` then [`create_thumbnail_failure`]); [`validate_thumbnail_header`]; copy `data_size` bytes;
/// [`trim_to_end_of_image`]; `cancel.check()`.
pub fn read_jpeg_thumbnail(api: &dyn LibRawApi, path: &str, cancel: &CancelToken) -> Result<Vec<u8>, DecodeError> {
    todo!("see doc comment")
}

/// C# `ValidateThumbnailHeader`: `Ok` only for `image_type == LIBRAW_IMAGE_JPEG` and `4 <= data_size <= MAX_THUMBNAIL_BYTES`;
/// otherwise `InvalidData` `LibRaw returned an unsupported or oversized thumbnail (type={t}, width={w}, height={h}, bytes={n}).`
pub fn validate_thumbnail_header(header: &ProcessedImageHeader) -> Result<(), DecodeError> {
    todo!()
}

/// C# `TrimToEndOfImage`: [`find_end_of_image`] then truncates `bytes` IN PLACE (`Vec::truncate`, no copy). An unpadded stream
/// comes back as the same allocation (C# returns the same array).
pub fn trim_to_end_of_image(bytes: Vec<u8>) -> Result<Vec<u8>, DecodeError> {
    todo!()
}

/// The length [`trim_to_end_of_image`] keeps (pure, golden-tested). `InvalidData` [`INCOMPLETE_JPEG_MESSAGE`] when:
/// `len < 4` or the stream does not start with `FF D8`; or neither the structure walk nor the structure-free fallback finds an EOI.
/// Fast path: strip trailing `00`/`FF` bytes (never below 2); if the rest ends with `FF D9` (and is at least 4 long) keep it.
/// Else [`find_end_of_image_by_structure`], else [`find_trailing_eoi_with_structure_free_tail`]. Never panics, never reads
/// past the slice (every index is checked as in C#).
pub fn find_end_of_image(bytes: &[u8]) -> Result<usize, DecodeError> {
    todo!("see doc comment and contract semantics 5")
}

/// C# `FindEndOfImageByStructure`: index just past the EOI found by walking the segments from offset 2, or `None` when the
/// structure ends (truncated/invalid) first. Fill `FF`s before a marker are skipped; `D9` ends; `00`, `01`, `D8`, `D0..=D7` carry
/// no length; a length `< 2` or past the end stops; after `DA` the entropy data is skipped (`FF 00`, `FF D0..=D7` and `FF FF`
/// continue it, any other marker ends it).
fn find_end_of_image_by_structure(b: &[u8]) -> Option<usize> {
    todo!()
}

/// C# `FindTrailingEoiWithStructureFreeTail`: scanning backwards from `len - 2` down to `max(2, len - 64 KiB)`, the LAST `FF D9`
/// whose tail (bytes after it, pairs `FF xx` with `j < len - 1`) contains no `D8`, `DA`, `DB`, `DD`, `FE`, `C0..=CF` or `E0..=EF`
/// marker; `Some(i + 2)`. The first (= last in the file) `FF D9` found decides: a structured tail => `None` (no earlier EOI tried).
fn find_trailing_eoi_with_structure_free_tail(b: &[u8]) -> Option<usize> {
    todo!()
}

// ------------------------------------------------------------------------------------------------ header checks

/// C# `ValidateImageDimensions`: `InvalidData` [`INVALID_DIMENSIONS_MESSAGE`] when `width <= 0 || height <= 0`.
pub fn validate_image_dimensions(width: i32, height: i32) -> Result<(), DecodeError> {
    todo!()
}

/// C# `IsSupportedBitmap`: `image_type == LIBRAW_IMAGE_BITMAP && width != 0 && height != 0 && colors in {1, 3}` (W5-17
/// `is_supported_channel_count`) `&& bits == 8`.
pub fn is_supported_bitmap(header: &ProcessedImageHeader) -> bool {
    todo!()
}

/// C# `IsDownscaled`: `target_width < source_width || target_height < source_height`.
pub fn is_downscaled(target_width: i32, target_height: i32, source_width: i32, source_height: i32) -> bool {
    todo!()
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
    todo!()
}

/// C# `ToExtendedLengthPath`: the `\\?\` form of a long fully qualified path, else `None`. `None` when the length in UTF-16 code
/// units is `< LONG_PATH_THRESHOLD`, when it starts with `\\?\` or `\\.\` (ordinal), when it is not fully qualified
/// (`dotnet_path::is_path_fully_qualified`) or when `dotnet_path::get_full_path` fails. A full path starting with `\\` becomes
/// `\\?\UNC\` + `full[2..]`, any other `\\?\` + `full`. Never panics.
pub fn to_extended_length_path(path: &str) -> Option<String> {
    todo!("see doc comment and contract semantics 6")
}

/// C# `CheckResult(code, operation, ct)`: `0` => `Ok`; otherwise `cancel.is_canceled()` => `Canceled`, else
/// [`create_failure`]`(code, operation, format_error)`.
pub fn check_result(
    error_code: i32,
    operation: &str,
    cancel: &CancelToken,
    format_error: &dyn Fn(i32) -> String,
) -> Result<(), DecodeError> {
    todo!()
}

/// C# `CreateFailure`: `LIBRAW_UNSUFFICIENT_MEMORY` or `ERRNO_NO_MEMORY` => `InvalidOperation` [`NOT_ENOUGH_MEMORY_MESSAGE`]
/// (a resource failure of a healthy file; `format_error` NOT called); any other code => `InvalidData`
/// `LibRaw could not {operation}: {format_error(code)}`.
pub fn create_failure(error_code: i32, operation: &str, format_error: &dyn Fn(i32) -> String) -> DecodeError {
    todo!()
}

/// C# `CreateThumbnailFailure`: [`create_failure`]`(code, "create embedded JPEG thumbnail", ..)`.
pub fn create_thumbnail_failure(error_code: i32, format_error: &dyn Fn(i32) -> String) -> DecodeError {
    todo!()
}

/// C# `ArgumentException.ThrowIfNullOrWhiteSpace(<param>)`: `InvalidArgument` with
/// `"{EMPTY_OR_WHITE_SPACE_MESSAGE} (Parameter '{param_name}')"`.
pub fn blank_argument_error(param_name: &str) -> DecodeError {
    todo!()
}

/// .NET `string.IsNullOrWhiteSpace` on a path: empty, or every char `char::is_whitespace` (same set as .NET `char.IsWhiteSpace`).
/// Non-UTF-8 paths are not blank (they contain a non-whitespace unit).
pub fn is_blank_path(path: &OsStr) -> bool {
    todo!()
}

// ------------------------------------------------------------------------------------------------ W5-17 hand-off glue

/// The W5-17 staging buffer is the production [`StagedPixels`]: `BgraBuffer::take_pixels` mapped through [`map_resample_error`].
impl StagedPixels for BgraBuffer {
    fn take_pixels(&mut self) -> Result<PixelBuffer, DecodeError> {
        todo!("BgraBuffer::take_pixels(self).map_err(map_resample_error)")
    }
}

/// W5-17 `ResampleError` -> the error C# `Decode` lets out: `InvalidData(m)` => `InvalidData { m }`; `Canceled` => `Canceled`;
/// `Overflow` => `Overflow { e.to_string() }`; `Disposed` => `Disposed { e.to_string() }`; `PixelBuffer(OutOfMemory { .. })` =>
/// `OutOfMemory { e.to_string() }` (C# `OutOfMemoryException`, which `decode_with_cancel` step 6 turns into `InvalidOperation`
/// [`NOT_ENOUGH_MEMORY_MESSAGE`]); any other `PixelBuffer(_)` => `InvalidArgument { e.to_string() }`
/// (`ArgumentOutOfRangeException`; unreachable after `validate_target_length`).
pub fn map_resample_error(error: ResampleError) -> DecodeError {
    todo!()
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
        todo!()
    }

    /// `native::format_error(code)`.
    fn format_error(&self, error_code: i32) -> String {
        todo!()
    }
}

/// `native::NativeError` -> `DecodeError`: `InitReturnedNull` => `InvalidOperation` [`INIT_FAILED_MESSAGE`];
/// `Unavailable(MissingExport { name })` => `EntryPointNotFound { "Unable to find an entry point named '{name}' in DLL
/// 'libraw.dll'." }`; any other `Unavailable(e)` => `DllNotFound { "Unable to load DLL 'libraw.dll' or one of its
/// dependencies: {e}" }`. (C# P/Invoke throws `DllNotFoundException` / `EntryPointNotFoundException` there; both are
/// fallbackable, INV-12. The texts are not golden-pinned.)
pub fn map_native_error(error: &native::NativeError) -> DecodeError {
    todo!()
}

/// One `native::LibRawHandle<'static>` behind the [`LibRawSession`] seam: every method forwards 1:1
/// (`set_progress_handler` -> `set_cancel_token`, `open_buffer` -> `open_shared_buffer`, `make_mem_*` -> `dcraw_make_mem_*`
/// boxed as `NativeImage`). Drop = the handle's drop (`libraw_close`, then the token box and the buffer).
struct NativeSession {
    handle: native::LibRawHandle<'static>,
}

impl LibRawSession for NativeSession {
    fn set_progress_handler(&mut self, cancel: CancelToken) {
        todo!("self.handle.set_cancel_token(cancel)")
    }

    fn set_output_color(&mut self, value: i32) {
        todo!()
    }

    fn set_output_bps(&mut self, value: i32) {
        todo!()
    }

    fn set_no_auto_bright(&mut self, value: i32) {
        todo!()
    }

    fn try_set_use_camera_wb(&mut self, output_color: i32, output_bps: i32, no_auto_bright: i32) -> bool {
        todo!()
    }

    fn open_wfile(&mut self, path: &OsStr) -> i32 {
        todo!()
    }

    fn open_buffer(&mut self, bytes: SourceBytes) -> i32 {
        todo!("self.handle.open_shared_buffer(bytes)")
    }

    fn unpack(&mut self) -> i32 {
        todo!()
    }

    fn unpack_thumb(&mut self) -> i32 {
        todo!()
    }

    fn dcraw_process(&mut self) -> i32 {
        todo!()
    }

    fn adjust_sizes_info_only(&mut self) -> i32 {
        todo!()
    }

    fn iwidth(&self) -> i32 {
        todo!()
    }

    fn iheight(&self) -> i32 {
        todo!()
    }

    fn raw_width(&self) -> i32 {
        todo!()
    }

    fn raw_height(&self) -> i32 {
        todo!()
    }

    fn decoder_name(&self) -> Option<String> {
        todo!()
    }

    fn raw_structure(&self) -> Option<RawStructure> {
        todo!()
    }

    fn make_mem_image(&mut self) -> Result<Box<dyn ProcessedImage>, i32> {
        todo!("self.handle.dcraw_make_mem_image().map(|image| Box::new(NativeImage {{ image }}) as _)")
    }

    fn make_mem_thumb(&mut self) -> Result<Box<dyn ProcessedImage>, i32> {
        todo!()
    }
}

/// One `native::LibRawImage` behind the [`ProcessedImage`] seam (Drop = `libraw_dcraw_clear_mem`).
struct NativeImage {
    image: native::LibRawImage,
}

impl ProcessedImage for NativeImage {
    fn header(&self) -> ProcessedImageHeader {
        todo!("self.image.header()")
    }

    fn data(&self) -> &[u8] {
        todo!("self.image.data().unwrap_or(&[])")
    }
}
