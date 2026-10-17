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

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::{DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::exif_parser;
use pr_imaging::exif_summary::ExifSummary;
use pr_imaging::fallback::FallbackImageDecoder;
use pr_imaging::memory_headroom::{GUARD_THRESHOLD_BYTES, MemoryInfoSource, has_headroom, output_has_headroom};
use pr_imaging::orientation;
use pr_imaging::pixels::{ApplyOrientationError, PixelBuffer, PixelBufferError, PixelLayout, apply_orientation};

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
        let handle = match &self.library {
            None => crate::native::create_decompressor(),
            Some(library) => library.create_decompressor(),
        }
        .map_err(DecodeError::from)?;
        Ok(Box::new(NativeDecompressor(handle)))
    }
}

/// [`Decompressor`] over one W5-13a [`TurboJpegHandle`]: each method calls exactly one handle method and maps
/// `Err(NativeError)` with `DecodeError::from`. Dropping it closes the handle (`tj3Destroy`, W5-13a `Drop`).
#[derive(Debug)]
pub struct NativeDecompressor(pub TurboJpegHandle);

impl Decompressor for NativeDecompressor {
    fn set(&mut self, param: TjParam, value: i32) -> Result<i32, DecodeError> {
        self.0.set(param, value).map_err(DecodeError::from)
    }
    fn get(&mut self, param: TjParam) -> Result<i32, DecodeError> {
        self.0.get(param).map_err(DecodeError::from)
    }
    fn decompress_header(&mut self, jpeg: &[u8]) -> Result<i32, DecodeError> {
        self.0.decompress_header(jpeg).map_err(DecodeError::from)
    }
    fn set_scaling_factor(&mut self, factor: TjScalingFactor) -> Result<i32, DecodeError> {
        self.0.set_scaling_factor(factor).map_err(DecodeError::from)
    }
    fn decompress8(
        &mut self,
        jpeg: &[u8],
        dst: &mut [u8],
        pitch: i32,
        pixel_format: TjPixelFormat,
    ) -> Result<i32, DecodeError> {
        self.0
            .decompress8(jpeg, dst, pitch, pixel_format)
            .map_err(DecodeError::from)
    }
    fn error_message(&mut self) -> Option<String> {
        self.0.error_message()
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
    factor.scaled(size).unwrap_or(0)
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
    SUPPORTED_SCALING_FACTORS
        .iter()
        .copied()
        .find(|&factor| {
            scaled_size(orig_width, factor) >= i64::from(target_width)
                && scaled_size(orig_height, factor) >= i64::from(target_height)
        })
        .unwrap_or(TjScalingFactor::ONE)
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
    let width = scaled_size(orig_width, factor);
    let height = scaled_size(orig_height, factor);
    let stride = if width > i64::MAX / 4 {
        i64::MAX
    } else {
        width.wrapping_mul(4)
    };
    let buffer_length = if height > i64::MAX / stride.max(1) {
        i64::MAX
    } else {
        stride.wrapping_mul(height)
    };
    let oversized = OversizedOutput {
        width,
        height,
        buffer_length,
    };
    // Every value is range checked by the `try_from`s: <= 0 or > i32::MAX is the C# "too large" verdict.
    match (
        i32::try_from(width),
        i32::try_from(height),
        i32::try_from(stride),
        i32::try_from(buffer_length),
    ) {
        (Ok(width), Ok(height), Ok(stride), Ok(buffer_length)) if width > 0 && height > 0 => Ok(OutputBuffer {
            width,
            height,
            stride,
            buffer_length,
        }),
        _ => Err(oversized),
    }
}

/// C# `EnsureOutputFits` without the error: `buffer_length < GUARD_THRESHOLD_BYTES` -> `true` WITHOUT reading
/// `memory_info`; else ONE `memory_info.read()` and `output_has_headroom(buffer_length, total, load)`.
pub fn output_fits(buffer_length: i32, memory_info: &dyn MemoryInfoSource) -> bool {
    if i64::from(buffer_length) < GUARD_THRESHOLD_BYTES {
        return true;
    }
    let info = memory_info.read();
    output_has_headroom(i64::from(buffer_length), info.total_available, info.load)
}

/// C# `EnsureSourceFits` without the error: `length < GUARD_THRESHOLD_BYTES` -> `true` WITHOUT reading `memory_info`;
/// else ONE `memory_info.read()` and `has_headroom(length, total, load)` (NOT the output peak factor).
pub fn source_fits(length: i64, memory_info: &dyn MemoryInfoSource) -> bool {
    if length < GUARD_THRESHOLD_BYTES {
        return true;
    }
    let info = memory_info.read();
    has_headroom(length, info.total_available, info.load)
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
    let len = jpeg.len();
    // C# `while (offset + 2 <= jpeg.Length)`, written so that no sum can overflow.
    while len >= 2 && *offset <= len - 2 {
        if byte_at(jpeg, *offset) != 0xFF {
            return SegmentStep::End { needs_more_data: false };
        }
        // Any marker may be preceded by repeated 0xFF fill bytes (ITU T.81 B.1.1.2).
        while *offset + 1 < len && byte_at(jpeg, *offset + 1) == 0xFF {
            *offset += 1;
        }
        if len - *offset < 2 {
            return SegmentStep::End { needs_more_data: true };
        }
        let marker = byte_at(jpeg, *offset + 1);
        if marker == 0x00 || marker == 0xFF || marker == 0xD9 {
            return SegmentStep::End { needs_more_data: false };
        }
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            *offset += 2;
            continue;
        }
        let remaining = len - *offset;
        if marker == 0xDA {
            // The SOS header itself is part of what the native header parse needs (RV-I03).
            let needs_more_data = remaining < 4 || 2 + be16(jpeg, *offset + 2) > remaining;
            return SegmentStep::End { needs_more_data };
        }
        if remaining < 4 {
            return SegmentStep::End { needs_more_data: true };
        }
        let length = be16(jpeg, *offset + 2);
        if length < 2 {
            return SegmentStep::End { needs_more_data: false };
        }
        if 2 + length > remaining {
            return SegmentStep::End { needs_more_data: true };
        }
        let payload = jpeg.get(*offset + 4..*offset + 2 + length).unwrap_or_default();
        *offset += 2 + length;
        return SegmentStep::Segment { marker, payload };
    }
    // Ran out of bytes exactly at a segment boundary.
    SegmentStep::End { needs_more_data: true }
}

/// `jpeg[index]`, `0` past the end (callers have already bounded `index`; this only keeps the walk panic-free).
fn byte_at(jpeg: &[u8], index: usize) -> u8 {
    jpeg.get(index).copied().unwrap_or(0)
}

/// Big-endian `u16` at `index` as `usize`.
fn be16(jpeg: &[u8], index: usize) -> usize {
    (usize::from(byte_at(jpeg, index)) << 8) | usize::from(byte_at(jpeg, index + 1))
}

/// `len >= 4 && FF D8` (the precondition of every header walk).
fn starts_with_soi(jpeg: &[u8]) -> bool {
    jpeg.len() >= 4 && jpeg.starts_with(&[0xFF, 0xD8])
}

/// `bytes.len() >= 3 && FF D8 FF`.
fn has_jpeg_signature(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// Orientation of an Exif TIFF block (1 when absent or invalid).
fn parse_tiff_orientation(tiff: &[u8]) -> i32 {
    exif_parser::try_read_orientation(tiff).map_or(1, i32::from)
}

/// C# `HeaderNeedsMoreData`: `false` for `len < 4` or no `FF D8` start; else walks segments from offset 2 and returns
/// the `needs_more_data` of the final [`SegmentStep::End`].
pub fn header_needs_more_data(jpeg: &[u8]) -> bool {
    if !starts_with_soi(jpeg) {
        return false;
    }
    let mut offset = 2;
    loop {
        match try_read_segment(jpeg, &mut offset) {
            SegmentStep::Segment { .. } => {}
            SegmentStep::End { needs_more_data } => return needs_more_data,
        }
    }
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
    let mut scan = HeaderScan::default();
    if !starts_with_soi(jpeg) {
        return scan;
    }
    let mut found_exif = false;
    let mut offset = 2;
    while let SegmentStep::Segment { marker, payload } = try_read_segment(jpeg, &mut offset) {
        if want_icc && !scan.has_icc && marker == 0xE2 && payload.starts_with(&ICC_SIGNATURE) {
            scan.has_icc = true;
        } else if want_exif && !found_exif && marker == 0xE1 && payload.starts_with(&EXIF_SIGNATURE) {
            scan.exif_tiff = payload.get(EXIF_SIGNATURE.len()..).unwrap_or_default();
            found_exif = true;
        }
        if (!want_icc || scan.has_icc) && (!want_exif || found_exif) {
            break;
        }
    }
    scan
}

/// C# `ReadExifOrientation` (public): `scan_header(jpeg, false, true)`; empty TIFF -> `1`, else
/// `exif_parser::try_read_orientation(tiff)` as `i32`, `1` when `None`.
pub fn read_exif_orientation(jpeg: &[u8]) -> i32 {
    parse_tiff_orientation(scan_header(jpeg, false, true).exif_tiff)
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
    if length > MAX_SOURCE_BYTES {
        return Err(SourceReadError::TooLarge { length });
    }
    let mut buffer = allocate_zeroed(length)?;
    let filled = fill_buffer(stream, &mut buffer)?;
    buffer.truncate(filled);
    Ok(buffer)
}

/// A zero-filled buffer of `length` bytes (negative = 0); allocation failure is `OutOfMemory`, never an abort.
fn allocate_zeroed(length: i64) -> Result<Vec<u8>, SourceReadError> {
    let size = usize::try_from(length.max(0)).map_err(|_| SourceReadError::OutOfMemory { bytes: length })?;
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(size)
        .map_err(|_| SourceReadError::OutOfMemory { bytes: length })?;
    buffer.resize(size, 0);
    Ok(buffer)
}

/// C# `ReadAtLeast(buffer, buffer.Length, throwOnEndOfStream: false)`: reads until `buffer` is full or the stream
/// ends. No probe read after the buffer is full; `Interrupted` is retried.
fn fill_buffer<R: Read + ?Sized>(stream: &mut R, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match stream.read(buffer.get_mut(filled..).unwrap_or_default()) {
            Ok(0) => break,
            Ok(read) => filled = filled.saturating_add(read).min(buffer.len()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

fn len_i64(buffer: &[u8]) -> i64 {
    i64::try_from(buffer.len()).unwrap_or(i64::MAX)
}

/// The one open of a source file (C# `FileStream(.., FileShare.ReadWrite | FileShare.Delete, SequentialScan)`): std's
/// default Windows share mode is read|write|delete, so another process may keep the file open for writing.
fn open_shared(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        /// `FILE_FLAG_SEQUENTIAL_SCAN`.
        const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
        options.custom_flags(FILE_FLAG_SEQUENTIAL_SCAN);
    }
    options.open(path)
}

/// C# `GrowHeaderArea(fs, length, buffer, cap)`: `limit = min(length, cap, MAX_SOURCE_BYTES)`; while
/// `buffer.len() < length && buffer.len() < limit && header_needs_more_data(&buffer)`: grow to
/// `min(limit, 2 * buffer.len())` reading the new tail from `reader` (positioned right after `buffer`); when the reader
/// ends early, return the bytes that exist. Returns the (possibly unchanged) buffer.
pub fn grow_header_area<R: Read + ?Sized>(
    reader: &mut R,
    length: i64,
    mut buffer: Vec<u8>,
    cap: i64,
) -> Result<Vec<u8>, SourceReadError> {
    let limit = length.min(cap).min(MAX_SOURCE_BYTES);
    while len_i64(&buffer) < length && len_i64(&buffer) < limit && header_needs_more_data(&buffer) {
        let old_len = buffer.len();
        let new_len = limit.min(len_i64(&buffer).saturating_mul(2));
        let Ok(new_len) = usize::try_from(new_len) else {
            break;
        };
        if new_len <= old_len {
            break;
        }
        reserve_growth(&mut buffer, new_len)?;
        buffer.resize(new_len, 0);
        let added = fill_buffer(reader, buffer.get_mut(old_len..).unwrap_or_default())?;
        if old_len + added < new_len {
            // The file ended earlier than its reported length: stop with the bytes that exist.
            buffer.truncate(old_len + added);
            return Ok(buffer);
        }
    }
    Ok(buffer)
}

fn reserve_growth(buffer: &mut Vec<u8>, new_len: usize) -> Result<(), SourceReadError> {
    reserve_growth_with(buffer, new_len, Vec::try_reserve_exact).map_err(|_| SourceReadError::OutOfMemory {
        bytes: i64::try_from(new_len).unwrap_or(i64::MAX),
    })
}

fn reserve_growth_with<E>(
    buffer: &mut Vec<u8>,
    new_len: usize,
    reserve: impl FnOnce(&mut Vec<u8>, usize) -> Result<(), E>,
) -> Result<(), E> {
    let additional = new_len - buffer.len();
    reserve(buffer, additional)
}

#[cfg(test)]
mod reserve_growth_tests {
    use super::{SourceReadError, reserve_growth, reserve_growth_with};

    #[test]
    fn grow_header_area_reserves_exact_additional_capacity() {
        // Rust-only guard for the Vec additional-capacity contract; C# allocates a buffer of the target length.
        let mut buffer = vec![0; 3];
        let mut requested = None;
        reserve_growth_with(&mut buffer, 11, |buffer, additional| {
            requested = Some(additional);
            buffer.try_reserve_exact(additional)
        })
        .unwrap();

        assert_eq!(requested, Some(8));
        assert!(buffer.capacity() >= 11);
    }

    #[test]
    fn grow_header_area_maps_reserve_capacity_overflow_to_out_of_memory() {
        // `usize::MAX` fails Vec's capacity check before allocating any large buffer.
        let mut buffer = Vec::new();
        let error = reserve_growth(&mut buffer, usize::MAX).unwrap_err();

        assert!(matches!(error, SourceReadError::OutOfMemory { bytes: i64::MAX }));
    }
}

/// C# `ReadHeaderArea(path)`: open (shared read/write/delete), `length` = file length, read `min(length, 64 KiB)`
/// (short read keeps what exists), then [`grow_header_area`] with [`HEADER_READ_CAP`]. The file is closed on return.
pub fn read_header_area(path: &Path) -> Result<Vec<u8>, SourceReadError> {
    let mut file = open_shared(path)?;
    let length = i64::try_from(file.metadata()?.len()).unwrap_or(i64::MAX);
    let first = length.min(i64::try_from(HEADER_READ_CHUNK).unwrap_or(i64::MAX));
    let mut buffer = allocate_zeroed(first)?;
    let got = fill_buffer(&mut file, &mut buffer)?;
    // The file may have shrunk since it was opened: work with what exists.
    buffer.truncate(got);
    grow_header_area(&mut file, length, buffer, HEADER_READ_CAP)
}

/// C# `ExtendHeaderArea(path, initialBuffer)` (internal): reopens the file, re-reads its length, seeks to
/// `initial.len()` and continues [`grow_header_area`] with [`EXTENDED_HEADER_CAP`] (a header still unfinished at 64 MiB
/// is treated as damaged by the caller).
pub fn extend_header_area(path: &Path, initial: Vec<u8>) -> Result<Vec<u8>, SourceReadError> {
    let mut file = open_shared(path)?;
    let length = i64::try_from(file.metadata()?.len()).unwrap_or(i64::MAX);
    // Skip to where the initial read ended.
    file.seek(SeekFrom::Start(u64::try_from(initial.len()).unwrap_or(u64::MAX)))?;
    grow_header_area(&mut file, length, initial, EXTENDED_HEADER_CAP)
}

/// C# `File.Exists(path)`: true only for an existing FILE (a directory, an unreadable/invalid path, any error -> false).
pub fn file_exists(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
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
    let output_too_large = || output_too_large_admission(texts, &plan);
    let mut current = pixels;
    if plan.fine_scale {
        match scaler.resize(&current, plan.target_width, plan.target_height) {
            Ok(resized) => {
                // The DCT-scaled source is released right after the resize returned.
                drop(std::mem::replace(&mut current, resized));
            }
            Err(FineScaleError::OutOfMemory) => return Err(output_too_large()),
            Err(FineScaleError::Failed(error)) => return Err(error),
        }
    }
    if plan.orient {
        match apply_orientation(current, plan.orientation) {
            Ok(oriented) => current = oriented,
            Err(ApplyOrientationError { error, source }) => {
                drop(source);
                return Err(match error {
                    PixelBufferError::OutOfMemory { .. } => output_too_large(),
                    PixelBufferError::Disposed => DecodeError::Disposed {
                        message: error.to_string(),
                    },
                    other => DecodeError::InvalidArgument {
                        message: other.to_string(),
                    },
                });
            }
        }
    }

    let estimated_bytes = current.byte_count();
    let (pixel_width, pixel_height) = (current.width(), current.height());
    // The codec owns the buffer from here: an error drops it.
    let image = codec.image_from_pixels(current)?;
    DecodedImage::new(
        image,
        pixel_width,
        pixel_height,
        estimated_bytes,
        DecodedImageOptions {
            downscaled: plan.downscaled,
            orientation: plan.orientation,
            actual_backend: DecoderBackend::TurboJpeg,
            original_width: plan.original_width,
            original_height: plan.original_height,
            exif: plan.exif,
            is_degraded_fallback: false,
        },
    )
    .map_err(|error| DecodeError::InvalidArgument {
        message: error.to_string(),
    })
}

/// The `MemoryAdmission` refusal for the SCALED output (C# `OutputTooLarge(plan)`), never fallbackable.
fn output_too_large_admission(texts: &Arc<dyn TurboJpegTexts>, plan: &FinishPlan) -> DecodeError {
    let (width, height, length) = (plan.scaled_width, plan.scaled_height, plan.buffer_length);
    let texts = Arc::clone(texts);
    DecodeError::memory_admission_with(format!(
        "TurboJPEG output dimensions are too large for the available memory: {width}x{height} ({length} bytes)."
    ))
    .with_user_text(Arc::new(move || {
        texts.output_too_large(i64::from(width), i64::from(height), i64::from(length))
    }))
}

/// `InvalidData` with the UI sentence `text`.
fn invalid_data(message: String, text: impl Fn() -> String + Send + Sync + 'static) -> DecodeError {
    DecodeError::InvalidData { message }.with_user_text(Arc::new(text))
}

/// `NotSupported` (fallbackable) with the UI sentence `text`.
fn not_supported(message: String, text: impl Fn() -> String + Send + Sync + 'static) -> DecodeError {
    DecodeError::not_supported_localized(message, Arc::new(text))
}

/// The localized `FileNotFound` of a missing image (C# `FileNotFoundException` + `Tr.ErrIoImageNotFound`).
fn image_not_found(texts: &Arc<dyn TurboJpegTexts>, path: &Path) -> DecodeError {
    let texts = Arc::clone(texts);
    let path = path.to_string_lossy().into_owned();
    DecodeError::FileNotFound {
        message: messages::IMAGE_NOT_FOUND.to_owned(),
    }
    .with_user_text(Arc::new(move || texts.image_not_found(&path)))
}

/// C# "JPEG source is too large ..." `NotSupported` with the `outputTooLarge(0, 0, length)` sentence.
fn source_too_large(texts: &Arc<dyn TurboJpegTexts>, reason: &str, length: i64) -> DecodeError {
    let texts = Arc::clone(texts);
    not_supported(format!("JPEG source is too {reason}: {length} bytes."), move || {
        texts.output_too_large(0, 0, length)
    })
}

/// Maps a bounded-reader failure (semantics 3, 11).
fn map_read_error(texts: &Arc<dyn TurboJpegTexts>, error: SourceReadError) -> DecodeError {
    match error {
        SourceReadError::TooLarge { length } => source_too_large(texts, "large to read into memory", length),
        SourceReadError::OutOfMemory { .. } => DecodeError::OutOfMemory {
            message: error.to_string(),
        },
        SourceReadError::Io(error) => DecodeError::from(error),
    }
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
        // (a) Magic bytes: JPEG SOI marker (FF D8 FF).
        if !has_jpeg_signature(bytes) {
            return Err(self.not_jpeg_error(&request.path));
        }

        // (b) IMG-07: ICC presence and the Exif TIFF span come from ONE marker walk.
        let scan = scan_header(bytes, true, true);
        if scan.has_icc {
            let texts = Arc::clone(&self.texts);
            return Err(not_supported(messages::ICC_FALLBACK.to_owned(), move || {
                texts.icc_fallback()
            }));
        }

        // (c) The orientation as given (no normalisation), else from the Exif block.
        let mut orientation = 1;
        if request.apply_orientation {
            orientation = request
                .source_orientation
                .unwrap_or_else(|| parse_tiff_orientation(scan.exif_tiff));
        }
        // (d) Photo information line, always parsed from the block this decode already holds.
        let exif = exif_parser::try_parse_tiff_block(scan.exif_tiff);
        // (e) The box is in displayed pixels; a transposing orientation swaps the stored axes.
        let transposed = request.apply_orientation && orientation::is_transposed(orientation);

        // Native header.
        let mut decompressor = self.decompressors.create()?;
        self.configure_strict_decoding(decompressor.as_mut())?;
        if decompressor.decompress_header(bytes)? != 0 {
            let native = decompressor.error_message();
            let detail = native
                .clone()
                .unwrap_or_else(|| messages::HEADER_PARSE_ERROR.to_owned());
            let texts = Arc::clone(&self.texts);
            return Err(invalid_data(
                format!("{}{detail}", messages::HEADER_FAILED_PREFIX),
                move || match &native {
                    Some(native) => texts.header_failed(native),
                    None => texts.header_failed(&texts.no_detail()),
                },
            ));
        }
        let orig_width = decompressor.get(TjParam::JpegWidth)?;
        let orig_height = decompressor.get(TjParam::JpegHeight)?;
        if orig_width <= 0 || orig_height <= 0 {
            let texts = Arc::clone(&self.texts);
            return Err(invalid_data(
                format!("{}{orig_width}x{orig_height}", messages::INVALID_DIMENSIONS_PREFIX),
                move || texts.invalid_dimensions(i64::from(orig_width), i64::from(orig_height)),
            ));
        }

        // Scale decision.
        let downscale = request.is_downscale_requested();
        let (mut target_width, mut target_height) = (orig_width, orig_height);
        let mut factor = TjScalingFactor::ONE;
        if downscale {
            (target_width, target_height) = request.decode_box().fit_stored(orig_width, orig_height, transposed);
            if target_width < orig_width || target_height < orig_height {
                factor = select_scaling_factor(orig_width, orig_height, target_width, target_height);
            }
        }
        if decompressor.set_scaling_factor(factor)? != 0 {
            // Decode at full size and fine-scale from there.
            factor = TjScalingFactor::ONE;
        }

        // Output buffer and guard (the SCALED size is what is allocated).
        let output = match calculate_output_buffer(orig_width, orig_height, factor) {
            Ok(output) => output,
            Err(oversized) => {
                let texts = Arc::clone(&self.texts);
                return Err(invalid_data(
                    format!(
                        "TurboJPEG output dimensions are too large: {}x{} ({} bytes).",
                        oversized.width, oversized.height, oversized.buffer_length
                    ),
                    move || texts.output_too_large(oversized.width, oversized.height, oversized.buffer_length),
                ));
            }
        };
        let (scaled_width, scaled_height, buffer_length) = (output.width, output.height, output.buffer_length);
        let plan = FinishPlan {
            scaled_width,
            scaled_height,
            target_width,
            target_height,
            buffer_length,
            fine_scale: downscale && (scaled_width > target_width || scaled_height > target_height),
            orient: request.apply_orientation && orientation > 1,
            orientation,
            downscaled: downscale
                && (scaled_width < orig_width || target_width < orig_width || target_height < orig_height),
            original_width: if transposed { orig_height } else { orig_width },
            original_height: if transposed { orig_width } else { orig_height },
            exif,
        };
        if !output_fits(buffer_length, self.memory_info.as_ref()) {
            return Err(output_too_large_admission(&self.texts, &plan));
        }

        // Decode straight into the native memory of a PixelBuffer (C-01).
        let mut pixels =
            PixelBuffer::allocate(scaled_width, scaled_height, PixelLayout::Bgr32).map_err(|error| match error {
                PixelBufferError::OutOfMemory { .. } => DecodeError::OutOfMemory {
                    message: error.to_string(),
                },
                other => DecodeError::InvalidArgument {
                    message: other.to_string(),
                },
            })?;
        let stride = pixels.stride();
        let destination = pixels.bytes_mut().map_err(|error| DecodeError::Disposed {
            message: error.to_string(),
        })?;
        if decompressor.decompress8(bytes, destination, stride, TjPixelFormat::Bgrx)? != 0 {
            let native = decompressor.error_message();
            drop(pixels);
            let detail = native
                .clone()
                .unwrap_or_else(|| messages::DECOMPRESSION_ERROR.to_owned());
            let texts = Arc::clone(&self.texts);
            return Err(invalid_data(
                format!("{}{detail}", messages::DECOMPRESS_FAILED_PREFIX),
                move || match &native {
                    Some(native) => texts.decompress_failed(native),
                    None => texts.decompress_failed(&texts.no_detail()),
                },
            ));
        }
        drop(decompressor);

        finish_with_codec(pixels, self.codec.as_ref(), self.scaler.as_ref(), &self.texts, plan)
    }

    /// `NotSupported` "File is not a valid JPEG: {path or memory buffer}" (empty path = C# `null`).
    fn not_jpeg_error(&self, path: &Path) -> DecodeError {
        let texts = Arc::clone(&self.texts);
        if path.as_os_str().is_empty() {
            return not_supported(
                format!("{}{}", messages::NOT_JPEG_PREFIX, messages::MEMORY_BUFFER),
                move || texts.not_jpeg(&texts.memory_buffer()),
            );
        }
        let label = path.to_string_lossy().into_owned();
        not_supported(format!("{}{label}", messages::NOT_JPEG_PREFIX), move || {
            texts.not_jpeg(&label)
        })
    }

    /// C# `LoadBytes` minus the caller-bytes branch: blank path -> no-source; missing file -> localized not-found; open
    /// (`NotFound` -> localized not-found); `source_fits(length)` else the localized `NotSupported`; [`read_all_bytes`].
    fn load_file(&self, request: &DecodeRequest) -> Result<Vec<u8>, DecodeError> {
        let path = request.path.as_path();
        if path.as_os_str().is_empty() || path.to_string_lossy().chars().all(char::is_whitespace) {
            let texts = Arc::clone(&self.texts);
            return Err(DecodeError::InvalidArgument {
                message: messages::NO_SOURCE.to_owned(),
            }
            .with_user_text(Arc::new(move || texts.no_source())));
        }
        if !file_exists(path) {
            return Err(image_not_found(&self.texts, path));
        }
        // INV-8: the file is closed (dropped) before the decode starts.
        let mut file = match open_shared(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(image_not_found(&self.texts, path)),
            Err(error) => return Err(DecodeError::Io(error)),
        };
        let length = i64::try_from(file.metadata()?.len()).unwrap_or(i64::MAX);
        if !source_fits(length, self.memory_info.as_ref()) {
            return Err(source_too_large(&self.texts, "large for the available memory", length));
        }
        read_all_bytes(&mut file, length).map_err(|error| map_read_error(&self.texts, error))
    }

    /// C# `ConfigureStrictDecoding`: `set(ScanLimit, MAX_PROGRESSIVE_SCANS)` (result ignored), then
    /// `set(StopOnWarning, 1) != 0` -> localized `InvalidOperation` with the native detail.
    fn configure_strict_decoding(&self, decompressor: &mut dyn Decompressor) -> Result<(), DecodeError> {
        // Bounds the work a hostile progressive JPEG can demand; an unsupported parameter just leaves the default.
        decompressor.set(TjParam::ScanLimit, MAX_PROGRESSIVE_SCANS)?;
        if decompressor.set(TjParam::StopOnWarning, 1)? != 0 {
            let native = decompressor.error_message();
            let detail = native
                .clone()
                .unwrap_or_else(|| messages::UNKNOWN_CONFIGURATION_ERROR.to_owned());
            let texts = Arc::clone(&self.texts);
            return Err(DecodeError::InvalidOperation {
                message: format!("{}{detail}", messages::STRICT_FAILED_PREFIX),
            }
            .with_user_text(Arc::new(move || match &native {
                Some(native) => texts.strict_mode_failed(native),
                None => texts.strict_mode_failed(&texts.no_detail()),
            })));
        }
        Ok(())
    }

    /// C# `TryReadDimensions`: a FRESH decompressor per call (a failed parse leaves a handle unusable), strict mode,
    /// `decompress_header`, then `JpegWidth`/`JpegHeight`. Factory and strict-mode errors propagate.
    fn try_read_dimensions(&self, jpeg: &[u8]) -> Result<DimensionsProbe, DecodeError> {
        let mut decompressor = self.decompressors.create()?;
        self.configure_strict_decoding(decompressor.as_mut())?;
        if decompressor.decompress_header(jpeg)? != 0 {
            return Ok(DimensionsProbe::Failed {
                native_error: decompressor.error_message(),
            });
        }
        let width = decompressor.get(TjParam::JpegWidth)?;
        let height = decompressor.get(TjParam::JpegHeight)?;
        if width > 0 && height > 0 {
            Ok(DimensionsProbe::Found { width, height })
        } else {
            Ok(DimensionsProbe::Failed { native_error: None })
        }
    }
}

impl ImageDecoder for TurboJpegDecoder {
    /// C# `Decode(DecodeRequest)`: caller bytes are borrowed as is; otherwise the file is read once (INV-8: closed before
    /// the decode starts). A failure of a FILE-sourced decode that is fallbackable carries the read bytes
    /// (`attach_source_bytes`), so `FallbackImageDecoder` does not read the file again; caller bytes are never attached.
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        if let Some(bytes) = &request.bytes {
            // Caller bytes are decoded in place, never attached to a failure.
            let image = self.decode_bytes(request, bytes)?;
            return Ok(Arc::new(image));
        }
        let bytes = self.load_file(request)?;
        match self.decode_bytes(request, &bytes) {
            Ok(image) => Ok(Arc::new(image)),
            // Only a fallbackable failure carries the (possibly huge) buffer, so the fallback does not re-read the file.
            Err(error) if FallbackImageDecoder::is_fallbackable(&error) => {
                Err(error.attach_source_bytes(Arc::from(bytes)))
            }
            Err(error) => Err(error),
        }
    }

    /// C# `ReadInfo(path)`: blank -> `InvalidArgument`; missing -> localized not-found; header area; signature;
    /// orientation from the FIRST header buffer; dimensions; extended read on failure; the error of the buffer that saw
    /// the most of the file (semantics 12).
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        if path.chars().all(char::is_whitespace) {
            return Err(DecodeError::InvalidArgument {
                message: messages::BLANK_PATH.to_owned(),
            });
        }
        let file = Path::new(path);
        if !file_exists(file) {
            return Err(image_not_found(&self.texts, file));
        }

        // Everything before the first scan is normally a few KB, but big APPn/COM segments are legal: read on until
        // the marker walk says the header is complete.
        let header = read_header_area(file).map_err(|error| map_read_error(&self.texts, error))?;
        if !has_jpeg_signature(&header) {
            let texts = Arc::clone(&self.texts);
            let label = path.to_owned();
            return Err(not_supported(
                format!("{}{path}", messages::NOT_JPEG_PREFIX),
                move || texts.not_jpeg(&label),
            ));
        }

        // The orientation comes from the FIRST buffer even if the extended read sees more (C# parity).
        let orientation = read_exif_orientation(&header);
        if let DimensionsProbe::Found { width, height } = self.try_read_dimensions(&header)? {
            return Ok(ImageInfo::with_orientation(width, height, orientation));
        }

        // Cut by the header cap, or damaged: extend the read, reusing the buffer already acquired.
        let first_len = header.len();
        let full = extend_header_area(file, header).map_err(|error| map_read_error(&self.texts, error))?;
        if full.len() > first_len {
            if let DimensionsProbe::Found { width, height } = self.try_read_dimensions(&full)? {
                return Ok(ImageInfo::with_orientation(width, height, orientation));
            }
        }

        // Report the failure of the buffer that saw the most of the file (a fresh decompressor, C# parity).
        let native = match self.try_read_dimensions(&full)? {
            DimensionsProbe::Failed { native_error } => native_error,
            DimensionsProbe::Found { .. } => None,
        };
        let detail = native
            .clone()
            .unwrap_or_else(|| messages::HEADER_PARSE_ERROR.to_owned());
        let texts = Arc::clone(&self.texts);
        Err(invalid_data(
            format!("{}{detail}", messages::INFO_FAILED_PREFIX),
            move || match &native {
                Some(native) => texts.info_failed(native),
                None => texts.info_failed(&texts.no_detail()),
            },
        ))
    }
}
