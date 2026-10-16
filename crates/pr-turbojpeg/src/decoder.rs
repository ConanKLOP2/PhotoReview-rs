//! Port of `PhotoReview.Imaging.TurboJpeg/TurboJpegDecoder.cs` (C# baseline d92d6aae, 696 lines): the libjpeg-turbo 3.x
//! JPEG decoder. DCT-scaled decode straight into a [`PixelBuffer`] (`Bgr32`, C-01), fine scale by an injected
//! [`FineScaler`] (production: WIC Fant, `pr_platform_win::wic::scaler::resize`), EXIF orientation by
//! `pr_imaging::pixels::apply_orientation`, then the [`PlatformImageCodec`] takes the buffer (C-02). JPEGs with an
//! embedded ICC profile are refused with a fallbackable `NotSupported` so the colour-managed WIC/WPF path decodes them.
//! `read_info` reads only the header area (64 KiB, doubling up to 8 MiB, then up to 64 MiB).
//!
//! PARITY: I-02 (decoder backends), Q-DEC-TURBOJPEG, I-12 (output/source memory guards), I-18 (EXIF on the decode),
//! INV-8 (file handle closed right after the read), INV-12 (admission refusals are never fallbackable).
//! Contract: `contracts/W5-14b.md` (draft `contracts/drafts-w5/W5-14b/contract.md`).
//!
//! Layering. Every decision between two native calls is a pure `pub fn` here and is tested without `turbojpeg.dll`:
//! [`select_scaling_factor`], [`calculate_output_buffer`], [`output_fits`], [`source_fits`], the marker walk
//! ([`try_read_segment`], [`header_needs_more_data`], [`scan_header`], [`read_exif_orientation`]), the bounded readers
//! ([`read_all_bytes`], [`grow_header_area`], [`read_header_area`], [`extend_header_area`]) and the finishing step
//! ([`finish_with_codec`]). The native calls go through the [`Decompressor`] seam; its real implementation
//! ([`NativeDecompressors`]) is a thin adapter over the W5-13a `native` module, which owns every `unsafe` block.
//! This file has NO `unsafe`.
// Stub scaffolding (D11): the porter removes this line together with the last `todo!()`.
#![allow(unused_variables, unused_imports, dead_code)]

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::{DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec, UserText};
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging::memory_headroom::{GUARD_THRESHOLD_BYTES, MemoryInfoSource};
use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};

use crate::native::{TjParam, TjPixelFormat, TjScalingFactor, TurboJpegHandle, TurboJpegLibrary};

// ---------------------------------------------------------------------------------------------------------------
// Constants (C# values; the golden row "constants" pins them)
// ---------------------------------------------------------------------------------------------------------------

/// C# `SupportedScalingFactors`, smallest first. All eighths on purpose (measured faster than power-of-two only).
pub const SUPPORTED_SCALING_FACTORS: [TjScalingFactor; 8] = [
    TjScalingFactor { num: 1, denom: 8 },
    TjScalingFactor { num: 1, denom: 4 },
    TjScalingFactor { num: 3, denom: 8 },
    TjScalingFactor { num: 1, denom: 2 },
    TjScalingFactor { num: 5, denom: 8 },
    TjScalingFactor { num: 3, denom: 4 },
    TjScalingFactor { num: 7, denom: 8 },
    TjScalingFactor { num: 1, denom: 1 },
];

/// C# `MaxProgressiveScans`: `TJPARAM_SCANLIMIT` (real files have about 10 scans).
pub const MAX_PROGRESSIVE_SCANS: i32 = 500;

/// C# `HeaderReadChunk`: first read of `read_info`, in bytes.
pub const HEADER_READ_CHUNK: usize = 64 * 1024;

/// C# `HeaderReadCap`: most of a file the first `read_info` pass reads (8 MiB).
pub const HEADER_READ_CAP: i64 = 8 * 1024 * 1024;

/// C# `ExtendedHeaderCap`: most of a file [`extend_header_area`] reads (64 MiB).
pub const EXTENDED_HEADER_CAP: i64 = 64 * 1024 * 1024;

/// C# `MaxSourceBytes` (`int.MaxValue - 64`): largest source read into one buffer; bigger files go to WIC (streams).
pub const MAX_SOURCE_BYTES: i64 = i32::MAX as i64 - 64;

/// APP2 signature of an ICC profile segment (12 bytes, NUL included).
pub const ICC_SIGNATURE: [u8; 12] = *b"ICC_PROFILE\0";

/// APP1 signature of an Exif segment (6 bytes).
pub const EXIF_SIGNATURE: [u8; 6] = *b"Exif\0\0";

/// English log texts (C# `Exception.Message`). Placeholders are filled with invariant digits, no separators.
pub mod messages {
    /// `"File is not a valid JPEG: {path or MEMORY_BUFFER}"` (`NotSupported`).
    pub const NOT_JPEG_PREFIX: &str = "File is not a valid JPEG: ";
    /// The label of a request without a path (C# `request.Path == null`).
    pub const MEMORY_BUFFER: &str = "memory buffer";
    /// `NotSupported`, fallbackable.
    pub const ICC_FALLBACK: &str = "Embedded ICC profile detected in JPEG; falling back to WIC/WPF.";
    /// `"TurboJPEG failed to decompress header: {native or HEADER_PARSE_ERROR}"` (`InvalidData`).
    pub const HEADER_FAILED_PREFIX: &str = "TurboJPEG failed to decompress header: ";
    /// Detail when the native library gave no text (header parse, read info).
    pub const HEADER_PARSE_ERROR: &str = "Header parse error";
    /// `"Invalid image dimensions reported by TurboJPEG: {w}x{h}"` (`InvalidData`).
    pub const INVALID_DIMENSIONS_PREFIX: &str = "Invalid image dimensions reported by TurboJPEG: ";
    /// `"TurboJPEG decompression failed: {native or DECOMPRESSION_ERROR}"` (`InvalidData`).
    pub const DECOMPRESS_FAILED_PREFIX: &str = "TurboJPEG decompression failed: ";
    /// Detail when the native library gave no text (decompress).
    pub const DECOMPRESSION_ERROR: &str = "Decompression error";
    /// `"TurboJPEG failed to read image info: {native or HEADER_PARSE_ERROR}"` (`InvalidData`).
    pub const INFO_FAILED_PREFIX: &str = "TurboJPEG failed to read image info: ";
    /// `InvalidArgument` (C# `ArgumentException`), localized `err.decoder.noSource`.
    pub const NO_SOURCE: &str = "DecodeRequest must provide either Path or Bytes.";
    /// `FileNotFound`, localized `err.io.imageNotFound` with the path.
    pub const IMAGE_NOT_FOUND: &str = "Image file not found.";
    /// `"TurboJPEG could not enable strict decoding: {native or UNKNOWN_CONFIGURATION_ERROR}"` (`InvalidOperation`).
    pub const STRICT_FAILED_PREFIX: &str = "TurboJPEG could not enable strict decoding: ";
    /// Detail when `tj3Set(StopOnWarning)` failed without a native text.
    pub const UNKNOWN_CONFIGURATION_ERROR: &str = "Unknown configuration error";
    /// .NET `ArgumentException.ThrowIfNullOrWhiteSpace` text of `read_info("")` (`InvalidArgument`, not localized).
    pub const BLANK_PATH: &str =
        "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'path')";
}

// ---------------------------------------------------------------------------------------------------------------
// Seams
// ---------------------------------------------------------------------------------------------------------------

/// One TurboJPEG decompressor instance (C# `SafeTurboJpegHandle` + the `TurboJpegNative.tj3*` calls the decoder makes).
/// `Ok(code)` is the raw libjpeg-turbo return (`0` = success, anything else = failure); the decoder interprets it.
/// `Err` is what the C# P/Invoke call would THROW (a missing export = `EntryPointNotFound`, a closed handle = `Disposed`,
/// the W5-13a Rust-only soundness guards = `InvalidArgument`): the decoder propagates it unchanged with `?` (C# lets the
/// exception escape), it never turns it into a return code.
/// Not `Send`/`Sync`: one instance per call, created and dropped on the calling thread (C# `using var decompressor`).
pub trait Decompressor {
    /// `tj3Set(handle, param, value)`.
    fn set(&mut self, param: TjParam, value: i32) -> Result<i32, DecodeError>;
    /// `tj3Get(handle, param)`.
    fn get(&mut self, param: TjParam) -> Result<i32, DecodeError>;
    /// `tj3DecompressHeader(handle, jpeg, jpeg.len())`.
    fn decompress_header(&mut self, jpeg: &[u8]) -> Result<i32, DecodeError>;
    /// `tj3SetScalingFactor(handle, factor)`.
    fn set_scaling_factor(&mut self, factor: TjScalingFactor) -> Result<i32, DecodeError>;
    /// `tj3Decompress8(handle, jpeg, jpeg.len(), dst, pitch, pixel_format)`. The caller passes the whole destination of
    /// the scaled size; the real adapter relies on the W5-13a guard (`dst`/`pitch` too small -> `Err(InvalidArgument)`
    /// without a native call; unreachable from the C# call sequence).
    fn decompress8(
        &mut self,
        jpeg: &[u8],
        dst: &mut [u8],
        pitch: i32,
        pixel_format: TjPixelFormat,
    ) -> Result<i32, DecodeError>;
    /// C# `TurboJpegNative.GetErrorMessage`: `tj3GetErrorStr` as text, `None` for a null pointer (never fails).
    fn error_message(&mut self) -> Option<String>;
}

/// Creates decompressors (C# `TurboJpegNative.CreateDecompressor()`). Production: [`NativeDecompressors`].
pub trait DecompressorFactory: Send + Sync {
    /// `Err(InvalidOperation "Failed to initialize TurboJPEG decompressor instance.")` when `tj3Init` returns null;
    /// `Err(DllNotFound)` / `Err(EntryPointNotFound)` when the library is not loadable (C# P/Invoke exceptions, both
    /// fallbackable; normally impossible because the backend is registered only after the W5-13b probe succeeded).
    fn create(&self) -> Result<Box<dyn Decompressor>, DecodeError>;
}

impl<F> DecompressorFactory for F
where
    F: Fn() -> Result<Box<dyn Decompressor>, DecodeError> + Send + Sync,
{
    fn create(&self) -> Result<Box<dyn Decompressor>, DecodeError> {
        self()
    }
}

/// The real factory (semantics 13). `library: None` (the [`Default`], production) = `crate::native::create_decompressor()`
/// (the process-wide DLL next to the executable, W5-13a `TurboJpegLibrary::shared`); `Some(lib)` =
/// `lib.create_decompressor()` (native tests load `native/x64/turbojpeg.dll` by absolute path, W5-13a "Native tests").
/// `NativeError` maps with W5-13a's `From<NativeError> for DecodeError` (DllNotFound/BadImageFormat/EntryPointNotFound
/// stay fallbackable, `tj3Init` NULL is `InvalidOperation`). Every `unsafe` block stays in `native`.
#[derive(Debug, Default, Clone)]
pub struct NativeDecompressors {
    library: Option<Arc<TurboJpegLibrary>>,
}

impl NativeDecompressors {
    /// Production: the process-wide library (same as [`Default`]).
    #[must_use]
    pub fn shared() -> Self {
        Self { library: None }
    }

    /// Tests and tools: decompressors from an explicitly loaded library.
    #[must_use]
    pub fn with_library(library: Arc<TurboJpegLibrary>) -> Self {
        Self { library: Some(library) }
    }
}

impl DecompressorFactory for NativeDecompressors {
    fn create(&self) -> Result<Box<dyn Decompressor>, DecodeError> {
        todo!(
            "W5-14b semantics 13: Box::new(NativeDecompressor(handle)) from the library or native::create_decompressor()"
        )
    }
}

/// [`Decompressor`] over one W5-13a [`TurboJpegHandle`]: each method calls exactly one handle method and maps
/// `Err(NativeError)` with `DecodeError::from`. Dropping it closes the handle (`tj3Destroy`, W5-13a `Drop`).
#[derive(Debug)]
pub struct NativeDecompressor(pub TurboJpegHandle);

impl Decompressor for NativeDecompressor {
    fn set(&mut self, param: TjParam, value: i32) -> Result<i32, DecodeError> {
        todo!("self.0.set(param, value).map_err(DecodeError::from)")
    }
    fn get(&mut self, param: TjParam) -> Result<i32, DecodeError> {
        todo!("self.0.get(param).map_err(DecodeError::from)")
    }
    fn decompress_header(&mut self, jpeg: &[u8]) -> Result<i32, DecodeError> {
        todo!("self.0.decompress_header(jpeg).map_err(DecodeError::from)")
    }
    fn set_scaling_factor(&mut self, factor: TjScalingFactor) -> Result<i32, DecodeError> {
        todo!("self.0.set_scaling_factor(factor).map_err(DecodeError::from)")
    }
    fn decompress8(
        &mut self,
        jpeg: &[u8],
        dst: &mut [u8],
        pitch: i32,
        pixel_format: TjPixelFormat,
    ) -> Result<i32, DecodeError> {
        todo!("self.0.decompress8(jpeg, dst, pitch, pixel_format).map_err(DecodeError::from)")
    }
    fn error_message(&mut self) -> Option<String> {
        todo!("self.0.error_message()")
    }
}

/// Failure of a [`FineScaler`]. `OutOfMemory` is the C# `catch (OutOfMemoryException)` arm (WIC `E_OUTOFMEMORY` =
/// `InsufficientMemoryException`, or a failed destination allocation): the decoder turns it into the "too large for the
/// available memory" admission refusal. Everything else propagates unchanged as `Failed(error)`.
#[derive(Debug)]
pub enum FineScaleError {
    OutOfMemory,
    Failed(DecodeError),
}

/// The fine-scale step (C# `WicPixelScaler.Resize(current, targetWidth, targetHeight)`): returns a NEW
/// `width x height` buffer, `source` is only read. Injected because `pr-turbojpeg` must not depend on
/// `pr-platform-win`; the composition root passes an adapter over `pr_platform_win::wic::scaler::resize`.
pub trait FineScaler: Send + Sync {
    fn resize(&self, source: &PixelBuffer, width: i32, height: i32) -> Result<PixelBuffer, FineScaleError>;
}

impl<F> FineScaler for F
where
    F: Fn(&PixelBuffer, i32, i32) -> Result<PixelBuffer, FineScaleError> + Send + Sync,
{
    fn resize(&self, source: &PixelBuffer, width: i32, height: i32) -> Result<PixelBuffer, FineScaleError> {
        self(source, width, height)
    }
}

/// The UI sentences this decoder attaches (C# `Tr.ErrDecoder*` / `Tr.ErrIoImageNotFound`), injected (D13/D41: no
/// `pr-l10n` here). Each call renders in the language current NOW: the decoder stores `Arc<dyn TurboJpegTexts>` plus the
/// values in the error's `UserText` closure. Keys and English texts are in each method's doc.
pub trait TurboJpegTexts: Send + Sync {
    /// `err.decoder.notJpeg`: "The file is not a valid JPEG: {source}".
    fn not_jpeg(&self, source: &str) -> String;
    /// `err.decoder.memoryBuffer`: "memory buffer" (the `{source}` of `not_jpeg` without a path, also rendered lazily).
    fn memory_buffer(&self) -> String;
    /// `err.decoder.iccFallback`.
    fn icc_fallback(&self) -> String;
    /// `err.decoder.noDetail`: "no details available" (the `{detail}` when the native library gave no text).
    fn no_detail(&self) -> String;
    /// `err.decoder.headerFailed`: "The JPEG header could not be read: {detail}".
    fn header_failed(&self, detail: &str) -> String;
    /// `err.decoder.invalidDimensions`: "The decoder reported invalid image dimensions: {width}x{height}".
    fn invalid_dimensions(&self, width: i64, height: i64) -> String;
    /// `err.decoder.decompressFailed`: "The image could not be decoded: {detail}".
    fn decompress_failed(&self, detail: &str) -> String;
    /// `err.decoder.infoFailed`: "The image information could not be read: {detail}".
    fn info_failed(&self, detail: &str) -> String;
    /// `err.decoder.noSource`.
    fn no_source(&self) -> String;
    /// `err.decoder.outputTooLarge`: "The decoded image would be too large: {width}x{height} ({bytes} bytes)."
    fn output_too_large(&self, width: i64, height: i64, bytes: i64) -> String;
    /// `err.decoder.strictModeFailed`: "The decoder could not switch to strict mode: {detail}".
    fn strict_mode_failed(&self, detail: &str) -> String;
    /// `err.io.imageNotFound`: "Image file not found: {path}".
    fn image_not_found(&self, path: &str) -> String;
}

/// The English catalog texts of `assets/languages/en.json` at the baseline (tests and goldens; the composition root
/// injects the catalog-backed implementation). Numbers are invariant digits without separators.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnglishTurboJpegTexts;

impl TurboJpegTexts for EnglishTurboJpegTexts {
    fn not_jpeg(&self, source: &str) -> String {
        format!("The file is not a valid JPEG: {source}")
    }
    fn memory_buffer(&self) -> String {
        "memory buffer".to_owned()
    }
    fn icc_fallback(&self) -> String {
        "The JPEG has an embedded ICC color profile, so another decoder is used to keep colors accurate.".to_owned()
    }
    fn no_detail(&self) -> String {
        "no details available".to_owned()
    }
    fn header_failed(&self, detail: &str) -> String {
        format!("The JPEG header could not be read: {detail}")
    }
    fn invalid_dimensions(&self, width: i64, height: i64) -> String {
        format!("The decoder reported invalid image dimensions: {width}x{height}")
    }
    fn decompress_failed(&self, detail: &str) -> String {
        format!("The image could not be decoded: {detail}")
    }
    fn info_failed(&self, detail: &str) -> String {
        format!("The image information could not be read: {detail}")
    }
    fn no_source(&self) -> String {
        "A decode request needs either a file path or image data.".to_owned()
    }
    fn output_too_large(&self, width: i64, height: i64, bytes: i64) -> String {
        format!("The decoded image would be too large: {width}x{height} ({bytes} bytes).")
    }
    fn strict_mode_failed(&self, detail: &str) -> String {
        format!("The decoder could not switch to strict mode: {detail}")
    }
    fn image_not_found(&self, path: &str) -> String {
        format!("Image file not found: {path}")
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Pure size decisions
// ---------------------------------------------------------------------------------------------------------------

/// `ceil(size * num / denom)` in `i64` (the libjpeg-turbo `TJSCALED` rounding; never overflows for `i32` sides).
pub fn scaled_size(size: i32, factor: TjScalingFactor) -> i64 {
    todo!("(size as i64 * num + denom - 1) / denom")
}

/// C# `SelectScalingFactor` (public): the FIRST of [`SUPPORTED_SCALING_FACTORS`] whose scaled size is `>= target` on both
/// axes; [`TjScalingFactor::ONE`] when none is (also when the target exceeds the source). Pure, total, never panics
/// (sides up to `i32::MAX`, any target).
pub fn select_scaling_factor(
    orig_width: i32,
    orig_height: i32,
    target_width: i32,
    target_height: i32,
) -> TjScalingFactor {
    todo!("W5-14b semantics 6")
}

/// The DCT-scaled output buffer (C# tuple `(Width, Height, Stride, BufferLength)`): `stride = width * 4`, no padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputBuffer {
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub buffer_length: i32,
}

/// The values of the `InvalidData` "output dimensions are too large" refusal (C# prints the `long` locals, which may
/// be `long.MaxValue` for the saturated stride/length).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OversizedOutput {
    pub width: i64,
    pub height: i64,
    pub buffer_length: i64,
}

/// C# `CalculateOutputBuffer`: `width/height = scaled_size(..)`, `stride = width > i64::MAX / 4 ? i64::MAX : width * 4`,
/// `buffer_length = height > i64::MAX / max(1, stride) ? i64::MAX : stride * height`; `Err` when any of width/height is
/// `<= 0` or `> i32::MAX`, or stride/buffer_length `> i32::MAX` (golden `calculate-output-buffer`).
pub fn calculate_output_buffer(
    orig_width: i32,
    orig_height: i32,
    factor: TjScalingFactor,
) -> Result<OutputBuffer, OversizedOutput> {
    todo!("W5-14b semantics 7")
}

/// C# `EnsureOutputFits` without the error: `buffer_length < GUARD_THRESHOLD_BYTES` -> `true` WITHOUT reading
/// `memory_info`; else ONE `memory_info.read()` and `output_has_headroom(buffer_length, total, load)`.
pub fn output_fits(buffer_length: i32, memory_info: &dyn MemoryInfoSource) -> bool {
    todo!("W5-14b semantics 7")
}

/// C# `EnsureSourceFits` without the error: `length < GUARD_THRESHOLD_BYTES` -> `true` WITHOUT reading `memory_info`;
/// else ONE `memory_info.read()` and `has_headroom(length, total, load)` (NOT the output peak factor).
pub fn source_fits(length: i64, memory_info: &dyn MemoryInfoSource) -> bool {
    todo!("W5-14b semantics 3")
}

// ---------------------------------------------------------------------------------------------------------------
// Marker walk (IMG-07: one walk for ICC + EXIF + "header complete")
// ---------------------------------------------------------------------------------------------------------------

/// One step of [`try_read_segment`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentStep<'a> {
    /// A marker segment with a length field: `marker` byte and `payload` (the bytes after the 2-byte length).
    Segment { marker: u8, payload: &'a [u8] },
    /// The walk is over (C# `return false`). `needs_more_data` is true exactly when it stopped because `jpeg` ended (a
    /// marker, length field, segment or the SOS header cut off by the buffer end, including a trailing run of `FF` fill
    /// bytes); false at SOS (complete), EOI, a stuffed `FF 00`, a non-`FF` byte where a marker must start, or a length `< 2`.
    /// (The C# `marker == 0xFF` arm is unreachable after the fill skip; keep it, it is harmless.)
    End { needs_more_data: bool },
}

/// C# `TryReadSegment(jpeg, ref offset, out marker, out payload, out needsMoreData)`. Start with `*offset = 2` (just past
/// SOI). Skips fill bytes (`FF FF ..`) and the parameterless markers `01`, `D0..=D7`, `D8`. On `Segment`, `*offset` is
/// past the segment. Every index is checked; never panics for any `jpeg` and any `*offset` (an offset past the end is
/// `End { needs_more_data: true }`, the C# "ran out of bytes" exit).
pub fn try_read_segment<'a>(jpeg: &'a [u8], offset: &mut usize) -> SegmentStep<'a> {
    todo!("W5-14b semantics 9")
}

/// C# `HeaderNeedsMoreData`: `false` for `len < 4` or no `FF D8` start; else walks segments from offset 2 and returns
/// the `needs_more_data` of the final [`SegmentStep::End`].
pub fn header_needs_more_data(jpeg: &[u8]) -> bool {
    todo!("W5-14b semantics 9")
}

/// Result of [`scan_header`] (C# out parameters `hasIcc`, `exifTiff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeaderScan<'a> {
    /// An APP2 (`E2`) segment whose payload starts with [`ICC_SIGNATURE`] (payload `>= 12` bytes). Only when `want_icc`.
    pub has_icc: bool,
    /// The bytes after `Exif\0\0` of the FIRST Exif-headed APP1 (`E1`, payload `>= 6`), a sub-slice of `jpeg`; empty when
    /// not wanted, not found, or that first segment has no TIFF part (still "found": later APP1s are ignored).
    pub exif_tiff: &'a [u8],
}

/// C# `ScanHeader(jpeg, wantIcc, wantExif, ...)`: `len < 4` or no `FF D8` -> default. Walks with [`try_read_segment`];
/// a segment is tested for ICC first (`want_icc && !has_icc && marker == E2 && signature`), ELSE for Exif
/// (`want_exif && !found && marker == E1 && signature`); after each segment it stops when every wanted answer is known
/// (`(!want_icc || has_icc) && (!want_exif || found)`, so with both false it stops after the first segment).
pub fn scan_header(jpeg: &[u8], want_icc: bool, want_exif: bool) -> HeaderScan<'_> {
    todo!("W5-14b semantics 10")
}

/// C# `ReadExifOrientation` (public): `scan_header(jpeg, false, true)`; empty TIFF -> `1`, else
/// `exif_parser::try_read_orientation(tiff)` as `i32`, `1` when `None`.
pub fn read_exif_orientation(jpeg: &[u8]) -> i32 {
    todo!("W5-14b semantics 10")
}

// ---------------------------------------------------------------------------------------------------------------
// Bounded source reads
// ---------------------------------------------------------------------------------------------------------------

/// Failures of the bounded readers, mapped by the decoder (semantics 3, 11).
#[derive(Debug)]
pub enum SourceReadError {
    /// `length > MAX_SOURCE_BYTES` (C# `NotSupportedException` "JPEG source is too large to read into memory: {length}
    /// bytes."), detected BEFORE any read.
    TooLarge { length: i64 },
    /// The buffer could not be allocated (C# `OutOfMemoryException` from `new byte[]`): `try_reserve_exact` failed.
    OutOfMemory { bytes: i64 },
    /// Open/seek/read failure (C# `IOException` and friends).
    Io(io::Error),
}

impl fmt::Display for SourceReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { length } => write!(f, "JPEG source is too large to read into memory: {length} bytes."),
            Self::OutOfMemory { .. } => f.write_str("Insufficient memory to continue the execution of the program."),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SourceReadError {}

impl From<io::Error> for SourceReadError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// C# `ReadAllBytes(Stream)` (internal): `length` is the stream's reported length (C# `stream.Length`).
/// `length > MAX_SOURCE_BYTES` -> `TooLarge` with ZERO reads; else one buffer of exactly `length` bytes, filled by
/// repeated `read` calls until full or a read returns 0 (C# `ReadAtLeast(.., throwOnEndOfStream: false)`); a short
/// stream returns the truncated buffer (no error). A stream that fills the buffer in one `read` is read exactly ONCE
/// (no probe read after the buffer is full). `Interrupted` is retried; other errors are `Io`.
pub fn read_all_bytes<R: Read + ?Sized>(stream: &mut R, length: i64) -> Result<Vec<u8>, SourceReadError> {
    todo!("W5-14b semantics 3")
}

/// C# `GrowHeaderArea(fs, length, buffer, cap)`: `limit = min(length, cap, MAX_SOURCE_BYTES)`; while
/// `buffer.len() < length && buffer.len() < limit && header_needs_more_data(&buffer)`: grow to
/// `min(limit, 2 * buffer.len())` reading the new tail from `reader` (positioned right after `buffer`); when the reader
/// ends early, return the bytes that exist. Returns the (possibly unchanged) buffer.
pub fn grow_header_area<R: Read + ?Sized>(
    reader: &mut R,
    length: i64,
    buffer: Vec<u8>,
    cap: i64,
) -> Result<Vec<u8>, SourceReadError> {
    todo!("W5-14b semantics 11")
}

/// C# `ReadHeaderArea(path)`: open (shared read/write/delete), `length` = file length, read `min(length, 64 KiB)`
/// (short read keeps what exists), then [`grow_header_area`] with [`HEADER_READ_CAP`]. The file is closed on return.
pub fn read_header_area(path: &Path) -> Result<Vec<u8>, SourceReadError> {
    todo!("W5-14b semantics 11")
}

/// C# `ExtendHeaderArea(path, initialBuffer)` (internal): reopens the file, re-reads its length, seeks to
/// `initial.len()` and continues [`grow_header_area`] with [`EXTENDED_HEADER_CAP`] (a header still unfinished at 64 MiB
/// is treated as damaged by the caller).
pub fn extend_header_area(path: &Path, initial: Vec<u8>) -> Result<Vec<u8>, SourceReadError> {
    todo!("W5-14b semantics 11")
}

/// C# `File.Exists(path)`: true only for an existing FILE (a directory, an unreadable/invalid path, any error -> false).
pub fn file_exists(path: &Path) -> bool {
    todo!("std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)")
}

// ---------------------------------------------------------------------------------------------------------------
// Finishing (fine scale, orientation, codec)
// ---------------------------------------------------------------------------------------------------------------

/// C# `FinishPlan`: what happens to the DCT-scaled pixels after the native decode.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishPlan {
    pub scaled_width: i32,
    pub scaled_height: i32,
    pub target_width: i32,
    pub target_height: i32,
    pub buffer_length: i32,
    /// `downscale requested && (scaled_width > target_width || scaled_height > target_height)`.
    pub fine_scale: bool,
    /// `apply_orientation && orientation > 1`.
    pub orient: bool,
    /// Reported as `DecodedImage::orientation` (1 when orientation is not applied; an out-of-range value is reported as is).
    pub orientation: i32,
    pub downscaled: bool,
    pub original_width: i32,
    pub original_height: i32,
    pub exif: Option<ExifSummary>,
}

/// C# `FinishWithCodec(pixels, codec, plan)`: TAKES `pixels`. Fine scale (`scaler.resize(&pixels, target_w, target_h)`,
/// the source is dropped right after), then `apply_orientation` when `plan.orient`; `FineScaleError::OutOfMemory` or an
/// orientation allocation failure (`PixelBufferError::OutOfMemory`) -> the admission refusal for the SCALED size
/// (semantics 8); any other failure propagates; every intermediate buffer is dropped on every path (exactly one buffer
/// alive on success: the one the codec received). Then `codec.image_from_pixels(buffer)` and
/// `DecodedImage::new(image, w, h, byte_count, {downscaled, orientation, TurboJpeg, original_w, original_h, exif})`.
pub fn finish_with_codec(
    pixels: PixelBuffer,
    codec: &dyn PlatformImageCodec,
    scaler: &dyn FineScaler,
    texts: &Arc<dyn TurboJpegTexts>,
    plan: FinishPlan,
) -> Result<DecodedImage, DecodeError> {
    todo!("W5-14b semantics 8")
}

// ---------------------------------------------------------------------------------------------------------------
// The decoder
// ---------------------------------------------------------------------------------------------------------------

/// Outcome of the header-only parse of `read_info` (C# `TryReadDimensions` bool + out values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DimensionsProbe {
    /// Header parsed and both sides `> 0`.
    Found { width: i32, height: i32 },
    /// Header parse failed (`native_error` = the native text, if any) or a side was `<= 0` (`native_error: None`).
    Failed { native_error: Option<String> },
}

/// `TurboJpegDecoder`. Stateless apart from its seams: one shared instance serves every thread (`Send + Sync`); each call
/// creates and drops its own decompressor(s). Implements [`ImageDecoder`] but NOT `CancellableImageDecoder` (C# parity:
/// `as_cancellable()` keeps the default `None`).
pub struct TurboJpegDecoder {
    /// C-02 platform codec, receives the finished buffer BY VALUE (C# constructor argument, no default).
    codec: Arc<dyn PlatformImageCodec>,
    /// Fine-scale step (C# `WicPixelScaler`).
    scaler: Arc<dyn FineScaler>,
    /// C# `MemoryInfo` (default `MemoryHeadroom.ReadGcMemoryInfo`): consulted only above [`GUARD_THRESHOLD_BYTES`].
    memory_info: Arc<dyn MemoryInfoSource>,
    /// Native seam ([`NativeDecompressors`] in production).
    decompressors: Arc<dyn DecompressorFactory>,
    /// UI sentences (D41).
    texts: Arc<dyn TurboJpegTexts>,
}

impl fmt::Debug for TurboJpegDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TurboJpegDecoder").finish_non_exhaustive()
    }
}

impl TurboJpegDecoder {
    /// `new TurboJpegDecoder(codec)` plus the C# `init`-only `MemoryInfo` and the Rust seams. The C#
    /// `ArgumentNullException` for a null codec cannot occur (test `NullCodec_Throws` is not ported).
    #[must_use]
    pub fn new(
        codec: Arc<dyn PlatformImageCodec>,
        scaler: Arc<dyn FineScaler>,
        memory_info: Arc<dyn MemoryInfoSource>,
        decompressors: Arc<dyn DecompressorFactory>,
        texts: Arc<dyn TurboJpegTexts>,
    ) -> Self {
        Self {
            codec,
            scaler,
            memory_info,
            decompressors,
            texts,
        }
    }

    /// Production wiring: [`NativeDecompressors`].
    #[must_use]
    pub fn native(
        codec: Arc<dyn PlatformImageCodec>,
        scaler: Arc<dyn FineScaler>,
        memory_info: Arc<dyn MemoryInfoSource>,
        texts: Arc<dyn TurboJpegTexts>,
    ) -> Self {
        Self::new(
            codec,
            scaler,
            memory_info,
            Arc::new(NativeDecompressors::shared()),
            texts,
        )
    }

    /// C# private `Decode(request, bytesMemory)`: the whole pipeline over the bytes (semantics 4..8). `label` is the
    /// request path, `None` for an empty path (C# `request.Path == null`).
    fn decode_bytes(&self, request: &DecodeRequest, bytes: &[u8]) -> Result<DecodedImage, DecodeError> {
        todo!("W5-14b semantics 4-8")
    }

    /// C# `LoadBytes` minus the caller-bytes branch: blank path -> no-source; missing file -> localized not-found; open
    /// (`NotFound` -> localized not-found); `source_fits(length)` else the localized `NotSupported`; [`read_all_bytes`].
    fn load_file(&self, request: &DecodeRequest) -> Result<Vec<u8>, DecodeError> {
        todo!("W5-14b semantics 2-3")
    }

    /// C# `ConfigureStrictDecoding`: `set(ScanLimit, MAX_PROGRESSIVE_SCANS)` (result ignored), then
    /// `set(StopOnWarning, 1) != 0` -> localized `InvalidOperation` with the native detail.
    fn configure_strict_decoding(&self, decompressor: &mut dyn Decompressor) -> Result<(), DecodeError> {
        todo!("W5-14b semantics 5")
    }

    /// C# `TryReadDimensions`: a FRESH decompressor per call (a failed parse leaves a handle unusable), strict mode,
    /// `decompress_header`, then `JpegWidth`/`JpegHeight`. Factory and strict-mode errors propagate.
    fn try_read_dimensions(&self, jpeg: &[u8]) -> Result<DimensionsProbe, DecodeError> {
        todo!("W5-14b semantics 12")
    }
}

impl ImageDecoder for TurboJpegDecoder {
    /// C# `Decode(DecodeRequest)`: caller bytes are borrowed as is; otherwise the file is read once (INV-8: closed before
    /// the decode starts). A failure of a FILE-sourced decode that is fallbackable carries the read bytes
    /// (`attach_source_bytes`), so `FallbackImageDecoder` does not read the file again; caller bytes are never attached.
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        todo!("W5-14b semantics 1")
    }

    /// C# `ReadInfo(path)`: blank -> `InvalidArgument`; missing -> localized not-found; header area; signature;
    /// orientation from the FIRST header buffer; dimensions; extended read on failure; the error of the buffer that saw
    /// the most of the file (semantics 12).
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        todo!("W5-14b semantics 12")
    }
}
