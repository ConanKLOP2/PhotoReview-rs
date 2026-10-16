//! Port of `PhotoReview.Imaging/Caching/WicImageEncoder.cs` and `Caching/WicCacheImageReader.cs` (C# baseline d92d6aae):
//! WIC JPEG/PNG encoding of a [`PixelBuffer`] for the disk cache, and decoding of an in-memory cache payload straight
//! into a new [`PixelBuffer`] (Q-CACHE-IO, WP-04). PARITY: see contracts/drafts-w4/W4-11e.
//!
//! Target path: `crates/pr-platform-win/src/wic/cache_image.rs` (lead adds `pub mod cache_image;` and `pub mod scaler;` to
//! `crates/pr-platform-win/src/wic/mod.rs`, `pub mod wic;` to `lib.rs`, and `pr-imaging` to `pr-platform-win/Cargo.toml`).
//!
//! Layout of this file: constants and pure decisions first (no `unsafe`, unit-testable without WIC), the public API
//! second (thin: validation, then the native seam), the `native` module last (the only `unsafe`, one `// SAFETY:` per
//! block, every COM pointer released on every path by RAII guards; it uses W4-11a's wrappers).
//!
//! `DecodeBox` and the pixel types are the real `pr_imaging` ones (W4-01b, W4-16a).

use std::fmt;
use std::io::Write;

use pr_core::catalog::explorer_types::Guid;

pub use pr_imaging::decode_types::DecodeBox;
pub use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};

// ---------------------------------------------------------------------------------------------------------------------
// Constants (byte-identical to C#; GUIDs are in .NET `Guid.ToByteArray()` order, like every `Guid` of pr-core).
// If W4-11a (`wic::interop`) already exports these GUIDs, import them from there and delete the duplicates (D28).
// ---------------------------------------------------------------------------------------------------------------------

/// `GUID_ContainerFormatJpeg` `19e4a5aa-5662-4fc5-a0c0-1758028e1057`.
pub const CONTAINER_FORMAT_JPEG: Guid = Guid([
    0xaa, 0xa5, 0xe4, 0x19, 0x62, 0x56, 0xc5, 0x4f, 0xa0, 0xc0, 0x17, 0x58, 0x02, 0x8e, 0x10, 0x57,
]);
/// `GUID_ContainerFormatPng` `1b7cfaf4-713f-473c-bbcd-6137425faeaf`.
pub const CONTAINER_FORMAT_PNG: Guid = Guid([
    0xf4, 0xfa, 0x7c, 0x1b, 0x3f, 0x71, 0x3c, 0x47, 0xbb, 0xcd, 0x61, 0x37, 0x42, 0x5f, 0xae, 0xaf,
]);
/// Vendor GUID WPF's `BitmapEncoder` passes to `CreateEncoder`: `f0e749ca-edef-4589-a73a-ee0e626a2a2b` (Microsoft).
pub const VENDOR_MICROSOFT: Guid = Guid([
    0xca, 0x49, 0xe7, 0xf0, 0xef, 0xed, 0x89, 0x45, 0xa7, 0x3a, 0xee, 0x0e, 0x62, 0x6a, 0x2a, 0x2b,
]);

macro_rules! wic_pixel_format {
    ($last:expr) => {
        Guid([
            0x24, 0xc3, 0xdd, 0x6f, 0x03, 0x4e, 0xfe, 0x4b, 0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d, 0xc9, $last,
        ])
    };
}

/// `GUID_WICPixelFormatBlackWhite` `6fddc324-4e03-4bfe-b185-3d77768dc905`.
pub const PIXEL_FORMAT_BLACK_WHITE: Guid = wic_pixel_format!(0x05);
/// `GUID_WICPixelFormat2bppGray` (`...dc906`).
pub const PIXEL_FORMAT_2BPP_GRAY: Guid = wic_pixel_format!(0x06);
/// `GUID_WICPixelFormat4bppGray` (`...dc907`).
pub const PIXEL_FORMAT_4BPP_GRAY: Guid = wic_pixel_format!(0x07);
/// `GUID_WICPixelFormat8bppGray` (`...dc908`).
pub const PIXEL_FORMAT_8BPP_GRAY: Guid = wic_pixel_format!(0x08);
/// `GUID_WICPixelFormat16bppBGR555` (`...dc909`).
pub const PIXEL_FORMAT_16BPP_BGR555: Guid = wic_pixel_format!(0x09);
/// `GUID_WICPixelFormat16bppBGR565` (`...dc90a`).
pub const PIXEL_FORMAT_16BPP_BGR565: Guid = wic_pixel_format!(0x0a);
/// `GUID_WICPixelFormat16bppGray` (`...dc90b`).
pub const PIXEL_FORMAT_16BPP_GRAY: Guid = wic_pixel_format!(0x0b);
/// `GUID_WICPixelFormat24bppBGR` (`...dc90c`).
pub const PIXEL_FORMAT_24BPP_BGR: Guid = wic_pixel_format!(0x0c);
/// `GUID_WICPixelFormat24bppRGB` (`...dc90d`).
pub const PIXEL_FORMAT_24BPP_RGB: Guid = wic_pixel_format!(0x0d);
/// `GUID_WICPixelFormat32bppBGR` (`...dc90e`): the layout `Bgr32`.
pub const PIXEL_FORMAT_32BPP_BGR: Guid = wic_pixel_format!(0x0e);
/// `GUID_WICPixelFormat32bppBGRA` (`...dc90f`).
pub const PIXEL_FORMAT_32BPP_BGRA: Guid = wic_pixel_format!(0x0f);
/// `GUID_WICPixelFormat32bppPBGRA` (`...dc910`): the layout `Pbgra32`.
pub const PIXEL_FORMAT_32BPP_PBGRA: Guid = wic_pixel_format!(0x10);
/// `GUID_WICPixelFormat48bppRGB` (`...dc915`).
pub const PIXEL_FORMAT_48BPP_RGB: Guid = wic_pixel_format!(0x15);
/// `GUID_WICPixelFormat32bppCMYK` (`...dc91c`).
pub const PIXEL_FORMAT_32BPP_CMYK: Guid = wic_pixel_format!(0x1c);
/// `GUID_WICPixelFormat64bppCMYK` (`...dc91f`).
pub const PIXEL_FORMAT_64BPP_CMYK: Guid = wic_pixel_format!(0x1f);
/// `GUID_WICPixelFormat48bppBGR` `e605a384-b468-46ce-bb2e-36f180e64313`.
pub const PIXEL_FORMAT_48BPP_BGR: Guid = Guid([
    0x84, 0xa3, 0x05, 0xe6, 0x68, 0xb4, 0xce, 0x46, 0xbb, 0x2e, 0x36, 0xf1, 0x80, 0xe6, 0x43, 0x13,
]);

/// The frame formats WIC reports as having NO alpha (C# `OpaqueFormats`, same list as `WicDirectDecoder`). Every other
/// format (BGRA, indexed with a possibly transparent palette, ...) decodes to `Pbgra32`.
pub const OPAQUE_FORMATS: [Guid; 14] = [
    PIXEL_FORMAT_BLACK_WHITE,
    PIXEL_FORMAT_2BPP_GRAY,
    PIXEL_FORMAT_4BPP_GRAY,
    PIXEL_FORMAT_8BPP_GRAY,
    PIXEL_FORMAT_16BPP_GRAY,
    PIXEL_FORMAT_16BPP_BGR555,
    PIXEL_FORMAT_16BPP_BGR565,
    PIXEL_FORMAT_24BPP_BGR,
    PIXEL_FORMAT_24BPP_RGB,
    PIXEL_FORMAT_32BPP_BGR,
    PIXEL_FORMAT_48BPP_RGB,
    PIXEL_FORMAT_48BPP_BGR,
    PIXEL_FORMAT_32BPP_CMYK,
    PIXEL_FORMAT_64BPP_CMYK,
];

/// A cache entry that decodes to more pixel bytes than this cannot be a valid preview/thumbnail (C# `MaxDecodedBytes`).
pub const MAX_DECODED_BYTES: i64 = i32::MAX as i64;
/// `E_OUTOFMEMORY` = `WINCODEC_ERR_OUTOFMEMORY` (`0x8007000E`) as the signed HRESULT .NET exposes.
pub const E_OUTOFMEMORY: i32 = 0x8007_000E_u32 as i32;
/// `E_FAIL` (`0x80004005`).
pub const E_FAIL: i32 = 0x8000_4005_u32 as i32;
/// `WICBitmapEncoderNoCache`, as WPF's `BitmapEncoder.Save` passes it to `IWICBitmapEncoder::Initialize`.
pub const WIC_BITMAP_ENCODER_NO_CACHE: u32 = 2;
/// DPI of every decoder `BitmapSource`; JFIF / `pHYs` are written with exactly this value, like WPF.
pub const DPI: f64 = 96.0;
/// Size of the BGRX -> BGR band buffer: fits L2, never large-object-heap sized.
pub const BAND_BYTES: usize = 256 * 1024;
/// `VT_R4`: the variant type of the `ImageQuality` property.
pub const VT_R4: u16 = 4;
/// `PROPBAG2_TYPE_DATA`.
pub const PROPBAG2_TYPE_DATA: u32 = 1;
/// The JPEG option name WIC reads.
pub const IMAGE_QUALITY_PROPERTY: &str = "ImageQuality";

// ---------------------------------------------------------------------------------------------------------------------
// Pure decisions (no WIC, no unsafe)
// ---------------------------------------------------------------------------------------------------------------------

/// C# `WicImageEncoder.SourceFormat(layout)`: `Pbgra32` => 32bppPBGRA, everything else => 32bppBGR.
pub fn source_format(layout: PixelLayout) -> Guid {
    if layout == PixelLayout::Pbgra32 {
        PIXEL_FORMAT_32BPP_PBGRA
    } else {
        PIXEL_FORMAT_32BPP_BGR
    }
}

/// C# `WicCacheImageReader.IsOutOfMemory(COMException)`: `hresult == E_OUTOFMEMORY` and nothing else. A resource failure,
/// never "corrupt entry".
pub fn is_out_of_memory(hresult: i32) -> bool {
    hresult == E_OUTOFMEMORY
}

/// C# `WicCacheImageReader.IsOpaqueFormat(format)`: exact 16-byte membership in [`OPAQUE_FORMATS`].
pub fn is_opaque_format(format: &Guid) -> bool {
    OPAQUE_FORMATS.contains(format)
}

/// The layout a payload decodes to when the caller asks for none: opaque frame => `Bgr32`, anything else => `Pbgra32`.
pub fn target_layout(requested: Option<PixelLayout>, frame_format: &Guid) -> PixelLayout {
    match requested {
        Some(layout) => layout,
        None if is_opaque_format(frame_format) => PixelLayout::Bgr32,
        None => PixelLayout::Pbgra32,
    }
}

/// Result of [`plan_decode`]: what the native path allocates and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodePlan {
    /// Frame size as stored in the payload.
    pub source_width: i32,
    pub source_height: i32,
    /// Size of the buffer to allocate (equals the source size when not downscaled).
    pub width: i32,
    pub height: i32,
    /// C# `WicCacheImageInfo.Downscaled`: `width < source_width || height < source_height`.
    pub downscaled: bool,
}

/// C# `WicCacheImageInfo(SourceWidth, SourceHeight, Downscaled)`: the frame size before shrinking and whether it was shrunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WicCacheImageInfo {
    pub source_width: i32,
    pub source_height: i32,
    pub downscaled: bool,
}

/// Errors of [`decode`]. The .NET type each stands for is in the variant doc; `is_entry_failure` mirrors
/// `DiskCacheStore.IsCacheEntryFailure` for these types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheImageError {
    /// `InvalidDataException` with one of the four fixed texts of the C# (see [`INVALID_DATA_*`] constants).
    InvalidData(&'static str),
    /// A WIC failure other than `E_OUTOFMEMORY` (C# type from `Marshal.GetExceptionForHR`, usually `COMException`: a damaged payload; see [`com_is_entry_failure`]). `hresult` is signed.
    Com { hresult: i32 },
    /// `E_OUTOFMEMORY` from WIC (C# `InsufficientMemoryException`): not a damaged entry, the entry is kept.
    InsufficientMemory { hresult: i32 },
    /// The destination buffer could not be allocated or described.
    Pixels(PixelBufferError),
}

/// `DiskCacheStore.IsCacheEntryFailure` for a raw WIC failure: C# decides on the .NET type `Marshal.GetExceptionForHR`
/// makes of the HRESULT. True only for `COMException`, `IOException` (and subclasses), `UnauthorizedAccess`,
/// `NotSupported`, `Format`, `InvalidData`, `Argument` (and subclasses), `InvalidOperation` (and subclasses), `Overflow`
/// and `InvalidCast`. False for e.g. `ArithmeticException` (0x80070216), `NotImplementedException` (E_NOTIMPL),
/// `NullReferenceException` (E_POINTER) and `OutOfMemoryException`: those propagate and the entry is kept.
pub fn com_is_entry_failure(hresult: i32) -> bool {
    use super::interop::{DotnetExceptionKind as K, exception_kind_for_hresult};
    let kind = exception_kind_for_hresult(hresult).unwrap_or(K::Com);
    // `NotSupportedException` includes `PlatformNotSupportedException` (0x80131539); `FormatException` includes
    // `CustomAttributeFormatException` (0x80131605). Both verified against the .NET 10 oracle.
    matches!(
        kind,
        K::Com | K::UnauthorizedAccess | K::Format | K::CustomAttributeFormat | K::InvalidCast | K::Overflow
    ) || kind.is_not_supported()
        || kind.is_io()
        || kind.is_argument()
        || kind.is_invalid_operation()
}

/// "Cache entry payload is empty."
pub const INVALID_DATA_EMPTY: &str = "Cache entry payload is empty.";
/// "Cache entry payload has invalid dimensions."
pub const INVALID_DATA_DIMENSIONS: &str = "Cache entry payload has invalid dimensions.";
/// "Cache entry payload dimensions do not match its header."
pub const INVALID_DATA_HEADER_MISMATCH: &str = "Cache entry payload dimensions do not match its header.";
/// "Cache entry payload is implausibly large."
pub const INVALID_DATA_TOO_LARGE: &str = "Cache entry payload is implausibly large.";

impl fmt::Display for CacheImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidData(text) => f.write_str(text),
            Self::Com { hresult } => write!(f, "WIC call failed (0x{:08X}).", *hresult as u32),
            Self::InsufficientMemory { .. } => f.write_str("Not enough memory to decode the cache entry."),
            Self::Pixels(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CacheImageError {}

impl CacheImageError {
    /// `DiskCacheStore.IsCacheEntryFailure` for these types: `InvalidData` and `Com` are entry failures (delete the entry,
    /// decode again from the source); `InsufficientMemory` and `Pixels(OutOfMemory)` are not (the entry is kept and the
    /// error propagates).
    pub fn is_entry_failure(&self) -> bool {
        match self {
            Self::InvalidData(_) => true,
            Self::Com { hresult } => com_is_entry_failure(*hresult),
            Self::InsufficientMemory { .. } => false,
            Self::Pixels(error) => !matches!(error, PixelBufferError::OutOfMemory { .. }),
        }
    }
}

/// WIC's memory stream takes a 32-bit length; a payload longer than `u32::MAX` bytes is not a cache entry (C# spans are
/// `int`, so C# cannot even express it). Exactly `u32::MAX` is accepted.
pub fn check_payload_len(len: usize) -> Result<(), CacheImageError> {
    if len as u64 > u64::from(u32::MAX) {
        return Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE));
    }
    Ok(())
}

/// The managed part of C# `Decode` that runs after `frame.GetSize` / `GetPixelFormat`, in the C# order:
/// 1. `raw_width == 0 || raw_height == 0 || raw_width > i32::MAX || raw_height > i32::MAX` => `InvalidData(DIMENSIONS)`.
/// 2. `expected_width > 0 && (source != expected)` (BOTH dimensions compared; `expected_height` is only read when
///    `expected_width > 0`) => `InvalidData(HEADER_MISMATCH)`. Checked BEFORE any allocation.
/// 3. `(width, height)` = `source` when the box is unbounded, else `decode_box.fit_stored(source, transposed: false)`;
///    `downscaled = width < source_width || height < source_height`; when not downscaled the size is the source size.
/// 4. `width as u64 * height as u64 * 4 > MAX_DECODED_BYTES as u64` => `InvalidData(TOO_LARGE)` (checked on the FINAL size; `u64`,
///    never `i64`: `i32::MAX * i32::MAX * 4` overflows `i64`, C# wraps and passes, Rust must not panic).
pub fn plan_decode(
    raw_width: u32,
    raw_height: u32,
    decode_box: DecodeBox,
    expected_width: i32,
    expected_height: i32,
) -> Result<DecodePlan, CacheImageError> {
    if raw_width == 0 || raw_height == 0 || raw_width > i32::MAX as u32 || raw_height > i32::MAX as u32 {
        return Err(CacheImageError::InvalidData(INVALID_DATA_DIMENSIONS));
    }
    // Both fit in `i32` now.
    let source_width = raw_width as i32;
    let source_height = raw_height as i32;
    if expected_width > 0 && (source_width != expected_width || source_height != expected_height) {
        return Err(CacheImageError::InvalidData(INVALID_DATA_HEADER_MISMATCH));
    }
    let (fit_width, fit_height) = if decode_box.is_unbounded() {
        (source_width, source_height)
    } else {
        decode_box.fit_stored(source_width, source_height, false)
    };
    let downscaled = fit_width < source_width || fit_height < source_height;
    let (width, height) = if downscaled {
        (fit_width, fit_height)
    } else {
        (source_width, source_height)
    };
    // DIVERGENCE: C# checks the size after `CreateBitmapScaler`/`Initialize`; the plan checks it up front, so a failing
    // scaler `Initialize` on an implausibly large final size reports `InvalidData` here and the COM error in C#.
    // DIVERGENCE: the product is `u64`; C# `(long)w * h * 4` wraps negative for `i32::MAX` squared and passes the check
    // (then `Allocate` fails). Rust reports `InvalidData(TOO_LARGE)` for it.
    let bytes = (width as u64) * (height as u64) * 4;
    if bytes > MAX_DECODED_BYTES as u64 {
        return Err(CacheImageError::InvalidData(INVALID_DATA_TOO_LARGE));
    }
    Ok(DecodePlan {
        source_width,
        source_height,
        width,
        height,
        downscaled,
    })
}

/// How the encoder hands the pixels to `IWICBitmapFrameEncode` after `SetPixelFormat` (C# `Encode` branch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodePath {
    /// The encoder accepted the source format: one `WritePixels` of the whole buffer (no copy).
    Direct,
    /// The encoder wants 24bppBGR (JPEG, opaque PNG): [`bgrx32_to_bgr24`] per band of [`rows_per_band`] rows.
    Bgr24Bands,
    /// Any other accepted format (PNG with alpha wants 32bppBGRA): a temporary `IWICBitmap` and `WriteSource`, so WIC
    /// converts (and un-premultiplies) exactly like WPF.
    WicConversion,
}

/// C# branch after `SetPixelFormat(ref acceptedFormat)`: `accepted == source` => `Direct`; `accepted == 24bppBGR` =>
/// `Bgr24Bands`; otherwise `WicConversion`. Exact 16-byte comparison.
pub fn plan_encode_path(source_format: &Guid, accepted_format: &Guid) -> EncodePath {
    if accepted_format == source_format {
        EncodePath::Direct
    } else if *accepted_format == PIXEL_FORMAT_24BPP_BGR {
        EncodePath::Bgr24Bands
    } else {
        EncodePath::WicConversion
    }
}

/// C# `Math.Clamp(BandBytes / (width * 3), 1, height)`: rows per band of the BGRX -> BGR conversion.
/// `width >= 1`, `height >= 1`.
pub fn rows_per_band(width: i32, height: i32) -> i32 {
    let row_bytes = (i64::from(width) * 3).max(1);
    let rows = BAND_BYTES as i64 / row_bytes;
    // `Math.Clamp(rows, 1, height)`; `height >= 1` by contract (guard a bad caller instead of panicking).
    rows.clamp(1, i64::from(height.max(1))) as i32
}

/// C# `quality / 100f`: the `ImageQuality` value written to the property bag (`VT_R4`).
pub fn image_quality_value(quality: i32) -> f32 {
    quality as f32 / 100.0
}

/// Destination of [`bgrx32_to_bgr24`] is shorter than 3 bytes per source pixel (C# `ArgumentException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DestinationTooShort {
    pub required: usize,
    pub actual: usize,
}

impl fmt::Display for DestinationTooShort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Destination is too short.")
    }
}

impl std::error::Error for DestinationTooShort {}

/// C# `WicImageEncoder.Bgrx32ToBgr24`: BGRX (4 bytes per pixel) -> BGR (3 bytes per pixel), dropping byte 3 of every pixel.
///
/// * `pixel_count = source.len() / 4` (a trailing partial pixel is ignored).
/// * `destination.len() < pixel_count * 3` => `Err(DestinationTooShort)` and nothing is written.
/// * Writes exactly `3 * pixel_count` bytes. The C# vector body may also write up to 4 scratch bytes after them when the
///   destination has room; the Rust port never touches bytes beyond `3 * pixel_count` (callers must not rely on either).
/// * The C# needs `+ 16` slack for its 16-byte vector stores; a safe Rust implementation does not (chunked scalar
///   copies auto-vectorise). Results are identical for every width.
pub fn bgrx32_to_bgr24(source: &[u8], destination: &mut [u8]) -> Result<(), DestinationTooShort> {
    let pixel_count = source.len() / 4;
    let required = pixel_count * 3;
    if destination.len() < required {
        return Err(DestinationTooShort {
            required,
            actual: destination.len(),
        });
    }
    for (from, to) in source.chunks_exact(4).zip(destination.chunks_exact_mut(3)) {
        to.copy_from_slice(&from[..3]);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Public API (thin: validate, then the native seam)
// ---------------------------------------------------------------------------------------------------------------------

/// Errors of [`encode_jpeg`] / [`encode_png`].
#[derive(Debug)]
pub enum EncodeError {
    /// JPEG `quality` outside `1..=100` (C# `ArgumentOutOfRangeException`). Checked before anything else.
    QualityOutOfRange { quality: i32 },
    /// The source buffer was disposed (C# `ObjectDisposedException`).
    Disposed,
    /// `byte_count > u32::MAX` (C# `ArgumentException`): WIC takes 32-bit buffer sizes.
    TooLarge { byte_count: i64 },
    /// A WIC failure (C# raw `COMException`); `hresult` is signed.
    Com { hresult: i32 },
    /// Writing to the output failed (the `IStream` adapter turns the `io::Error` into an HRESULT; the original error is
    /// kept here when the adapter recorded it). With the in-memory strategy of contract note 10 this is the error of
    /// the final copy to `output`.
    Io(std::io::Error),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QualityOutOfRange { quality } => {
                write!(f, "JPEG quality must be between 1 and 100 (was {quality}).")
            }
            Self::Disposed => f.write_str("Cannot access a disposed object (PixelBuffer)."),
            Self::TooLarge { byte_count } => {
                write!(f, "Pixel buffer is too large to encode ({byte_count} bytes).")
            }
            Self::Com { hresult } => write!(f, "WIC call failed (0x{:08X}).", *hresult as u32),
            Self::Io(error) => write!(f, "Writing the encoded image failed: {error}"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// C# `WicImageEncoder.EncodeJpeg`: baseline JPEG of the Microsoft WIC encoder with `ImageQuality = quality / 100`,
/// written to `output` at its current position. `output` needs no `Seek`: the encoder writes into W4-11a's
/// `ComStream::writable_memory()` and the bytes are copied to `output` after `Commit` (contract note 10).
///
/// Order of checks: `quality` range, then disposed, then size. No partial file contract here: the caller
/// (`AtomicCacheFile`) writes to a temp file and publishes atomically.
pub fn encode_jpeg(pixels: &PixelBuffer, output: &mut dyn Write, quality: i32) -> Result<(), EncodeError> {
    if !(1..=100).contains(&quality) {
        return Err(EncodeError::QualityOutOfRange { quality });
    }
    validate_pixels(pixels)?;
    native::encode(pixels, output, &CONTAINER_FORMAT_JPEG, quality)
}

/// C# `WicImageEncoder.EncodePng`: PNG with WIC default options (no interlace), same parameters as `PngBitmapEncoder`.
pub fn encode_png(pixels: &PixelBuffer, output: &mut dyn Write) -> Result<(), EncodeError> {
    validate_pixels(pixels)?;
    native::encode(pixels, output, &CONTAINER_FORMAT_PNG, 0)
}

/// `WINCODEC_ERR_STREAMWRITE` (`0x88982F71`) as the signed HRESULT: what the C# PNG encoder throws (`COMException`,
/// "Failed to write to the stream") when the output stream's `Write` fails.
pub const WINCODEC_ERR_STREAMWRITE: i32 = 0x8898_2F71_u32 as i32;

/// Outcome of the final copy of the encoded bytes to the caller's `output`, as C# observes a failing output stream
/// (probed against the baseline d92d6aae):
/// * JPEG: WIC's JPEG encoder swallows the stream failure and `EncodeJpeg` RETURNS NORMALLY (the output stays empty or
///   truncated) => `Ok(())`. DIVERGENCE kept on purpose for parity; the caller's atomic temp file is the safety net.
/// * PNG: `COMException` 0x88982F71 => `Com { WINCODEC_ERR_STREAMWRITE }` (an entry failure in C#).
///
/// C# streams while encoding (a mid-way failure leaves a partial write); Rust writes all or nothing (contract note 10).
pub fn finish_output(is_jpeg: bool, written: std::io::Result<()>) -> Result<(), EncodeError> {
    match written {
        Ok(()) => Ok(()),
        Err(_) if is_jpeg => Ok(()),
        Err(_) => Err(EncodeError::Com {
            hresult: WINCODEC_ERR_STREAMWRITE,
        }),
    }
}

/// The managed checks of C# `Encode` after the quality range: disposed, then `byte_count > u32::MAX`.
fn validate_pixels(pixels: &PixelBuffer) -> Result<(), EncodeError> {
    if pixels.is_disposed() {
        return Err(EncodeError::Disposed);
    }
    if pixels.byte_count() > i64::from(u32::MAX) {
        return Err(EncodeError::TooLarge {
            byte_count: pixels.byte_count(),
        });
    }
    Ok(())
}

/// `windows` GUID of a `pr_core` [`Guid`] (.NET `ToByteArray()` order: the first three fields little-endian).
pub(crate) fn guid_to_windows(guid: &Guid) -> windows_core::GUID {
    let b = guid.0;
    windows_core::GUID {
        data1: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        data2: u16::from_le_bytes([b[4], b[5]]),
        data3: u16::from_le_bytes([b[6], b[7]]),
        data4: [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
    }
}

/// The `pr_core` [`Guid`] of a `windows` GUID (inverse of [`guid_to_windows`]).
pub(crate) fn guid_from_windows(guid: &windows_core::GUID) -> Guid {
    let a = guid.data1.to_le_bytes();
    let b = guid.data2.to_le_bytes();
    let c = guid.data3.to_le_bytes();
    let d = guid.data4;
    Guid([
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7],
    ])
}

/// C# `WicCacheImageReader.Decode`: decodes an in-memory JPEG/PNG cache payload into a NEW buffer the caller owns.
///
/// * `encoded` empty => `InvalidData(INVALID_DATA_EMPTY)` without touching WIC.
/// * `layout`: `None` = by the frame format ([`target_layout`]); the result always has that layout.
/// * `decode_box`: shrink only (never upscales); [`DecodeBox::UNBOUNDED`] keeps the frame size. The shrink is the Fant
///   scaler, the layout conversion a WIC format converter (no dither, no palette), in that order (scale, then convert).
/// * `expected_width > 0`: the frame size must equal `(expected_width, expected_height)` (header check, before allocating).
/// * Errors: see [`CacheImageError`]; `E_OUTOFMEMORY` from any WIC call becomes `InsufficientMemory`, other WIC failures
///   stay `Com` (a garbage payload is `Com`, not `InvalidData`).
///
/// Returns the pixels and the [`WicCacheImageInfo`] (C# `out info`).
pub fn decode(
    encoded: &[u8],
    layout: Option<PixelLayout>,
    decode_box: DecodeBox,
    expected_width: i32,
    expected_height: i32,
) -> Result<(PixelBuffer, WicCacheImageInfo), CacheImageError> {
    if encoded.is_empty() {
        return Err(CacheImageError::InvalidData(INVALID_DATA_EMPTY));
    }
    native::decode_payload(encoded, layout, decode_box, expected_width, expected_height)
}

// ---------------------------------------------------------------------------------------------------------------------
// Native seam: the only `unsafe` of this file (W4-11a wrappers; one `// SAFETY:` per block)
// ---------------------------------------------------------------------------------------------------------------------

pub(crate) mod native {
    use super::{
        CacheImageError, DPI, DecodeBox, E_FAIL, EncodeError, EncodePath, IMAGE_QUALITY_PROPERTY, PROPBAG2_TYPE_DATA,
        PixelBuffer, PixelLayout, VENDOR_MICROSOFT, VT_R4, WIC_BITMAP_ENCODER_NO_CACHE, WicCacheImageInfo,
        bgrx32_to_bgr24, check_payload_len, guid_from_windows, guid_to_windows, image_quality_value, is_out_of_memory,
        plan_decode, plan_encode_path, rows_per_band, source_format, target_layout,
    };
    use crate::wic::interop::{self, Com, ComStream, WicError};
    use pr_core::catalog::explorer_types::Guid;
    use std::io::{Cursor, Write};
    use windows::Win32::Graphics::Imaging::{
        IWICBitmap, IWICBitmapEncoder, IWICBitmapFrameEncode, IWICBitmapScaler, IWICPalette, WICBitmapDitherTypeNone,
        WICBitmapEncoderCacheOption, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
        WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::StructuredStorage::{IPropertyBag2, PROPBAG2};
    use windows::Win32::System::Variant::{VARENUM, VARIANT};
    use windows_core::PWSTR;

    /// C# `catch (COMException) when IsOutOfMemory(e)` => `InsufficientMemory`, every other failure => `Com`.
    fn decode_error(error: &WicError) -> CacheImageError {
        let hresult = error.hresult();
        if is_out_of_memory(hresult) {
            CacheImageError::InsufficientMemory { hresult }
        } else {
            CacheImageError::Com { hresult }
        }
    }

    fn decode_com(error: &windows_core::Error) -> CacheImageError {
        decode_error(&WicError::from_windows(error))
    }

    fn encode_error(error: &WicError) -> EncodeError {
        EncodeError::Com {
            hresult: error.hresult(),
        }
    }

    fn encode_com(error: &windows_core::Error) -> EncodeError {
        encode_error(&WicError::from_windows(error))
    }

    /// Steps of C# `Decode` after `plan_decode` is known (all objects released by guards on every path, in reverse order
    /// of creation: converter, scaler, frame, decoder, stream, factory):
    /// 1. `interop::create_factory()` (W4-11a: `WICCreateImagingFactory_Proxy(WINCODEC_SDK_VERSION1)`; `hr < 0` or a null
    ///    factory with `hr >= 0` => `E_FAIL`).
    /// 2. `CreateStream`; `InitializeFromMemory(encoded.as_ptr(), encoded.len() as u32)` (the slice must stay alive and
    ///    unmoved until the guards drop: it is borrowed for the whole function).
    /// 3. `CreateDecoderFromStream(stream, null vendor, WICDecodeMetadataCacheOnDemand)`; `GetFrame(0)`; `GetSize`;
    ///    `GetPixelFormat`; [`super::plan_decode`] with the raw sizes.
    /// 4. When `plan.downscaled`: `CreateBitmapScaler`, `Initialize(frame, width, height, WICBitmapInterpolationModeFant)`;
    ///    the converter then reads from the scaler, otherwise from the frame.
    /// 5. `CreateFormatConverter`; `Initialize(source, source_format(target_layout), WICBitmapDitherTypeNone, null palette,
    ///    0.0, WICBitmapPaletteTypeCustom)`.
    /// 6. `PixelBuffer::allocate(plan.width, plan.height, target_layout)`; `converter.CopyPixels(null rect, stride,
    ///    byte_count, address)`. A buffer allocated and then failed on is freed by `Drop`.
    /// 7. A COM error whose HRESULT is `E_OUTOFMEMORY` => `InsufficientMemory`; any other => `Com`.
    pub(crate) fn decode_payload(
        encoded: &[u8],
        layout: Option<PixelLayout>,
        decode_box: DecodeBox,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<(PixelBuffer, WicCacheImageInfo), CacheImageError> {
        check_payload_len(encoded.len())?;
        // Locals drop in reverse order of declaration: converter, scaler, frame, decoder, stream, factory.
        let factory = interop::create_factory().map_err(|e| decode_error(&e))?;
        let stream = interop::create_stream(&factory).map_err(|e| decode_error(&e))?;
        // SAFETY: `stream` is a live `IWICStream`; `encoded` is borrowed for this whole function and `stream` (which keeps
        // the pointer, the memory is not copied) is dropped before the function returns.
        unsafe { stream.InitializeFromMemory(encoded) }.map_err(|e| decode_com(&e))?;
        let decoder = interop::create_decoder_from_stream(&factory, &stream, WICDecodeMetadataCacheOnDemand)
            .map_err(|e| decode_error(&e))?;
        let frame = interop::get_frame(&decoder, 0).map_err(|e| decode_error(&e))?;

        let (mut raw_width, mut raw_height) = (0u32, 0u32);
        // SAFETY: `frame` is a live `IWICBitmapFrameDecode`; both out pointers are valid `u32` locals.
        unsafe { frame.GetSize(&mut raw_width, &mut raw_height) }.map_err(|e| decode_com(&e))?;
        // SAFETY: `frame` is a live `IWICBitmapFrameDecode`; `GetPixelFormat` has no pointer arguments.
        let frame_format = unsafe { frame.GetPixelFormat() }.map_err(|e| decode_com(&e))?;
        let plan = plan_decode(raw_width, raw_height, decode_box, expected_width, expected_height)?;
        let target = target_layout(layout, &guid_from_windows(&frame_format));

        let scaler: Option<Com<IWICBitmapScaler>> = if plan.downscaled {
            let scaler = interop::create_bitmap_scaler(&factory).map_err(|e| decode_error(&e))?;
            // SAFETY: `scaler` and `frame` are live COM objects owned by guards in this frame; the size is positive
            // (`plan_decode` validated it).
            unsafe {
                scaler.Initialize(
                    &*frame,
                    plan.width as u32,
                    plan.height as u32,
                    WICBitmapInterpolationModeFant,
                )
            }
            .map_err(|e| decode_com(&e))?;
            Some(scaler)
        } else {
            None
        };

        let converter = interop::create_format_converter(&factory).map_err(|e| decode_error(&e))?;
        let destination_format = guid_to_windows(&source_format(target));
        // SAFETY: `converter` is a live `IWICFormatConverter`; its source (scaler or frame) is a live bitmap source owned
        // by a guard that outlives the converter; `destination_format` outlives the call; the palette is a typed null.
        let initialized = unsafe {
            match &scaler {
                Some(scaler) => converter.Initialize(
                    &**scaler,
                    &destination_format,
                    WICBitmapDitherTypeNone,
                    None::<&IWICPalette>,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                ),
                None => converter.Initialize(
                    &*frame,
                    &destination_format,
                    WICBitmapDitherTypeNone,
                    None::<&IWICPalette>,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                ),
            }
        };
        initialized.map_err(|e| decode_com(&e))?;

        // Allocated only now (C# order); a failure below drops it (the C# `finally { pixels?.Dispose(); }`).
        let mut pixels = PixelBuffer::allocate(plan.width, plan.height, target).map_err(CacheImageError::Pixels)?;
        let stride = pixels.stride() as u32;
        let target_bytes = pixels.bytes_mut().map_err(CacheImageError::Pixels)?;
        // SAFETY: `converter` is initialised; `target_bytes` is exactly `stride * height` writable bytes owned by `pixels`,
        // which outlives the call (the length fits `u32`: `plan_decode` limits it to `i32::MAX`); a null rect is the
        // whole image.
        unsafe { converter.CopyPixels(std::ptr::null(), stride, target_bytes) }.map_err(|e| decode_com(&e))?;

        Ok((
            pixels,
            WicCacheImageInfo {
                source_width: plan.source_width,
                source_height: plan.source_height,
                downscaled: plan.downscaled,
            },
        ))
    }

    /// Writes `ImageQuality` (`VT_R4`) into the encoder option bag (C# `WriteImageQuality`).
    fn write_image_quality(options: &IPropertyBag2, quality: f32) -> Result<(), EncodeError> {
        // NUL-terminated UTF-16 name that outlives the call (the C# string is CoTaskMem; any such buffer is fine).
        let mut name: Vec<u16> = IMAGE_QUALITY_PROPERTY
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let bag = PROPBAG2 {
            dwType: PROPBAG2_TYPE_DATA,
            vt: VARENUM(VT_R4),
            pstrName: PWSTR(name.as_mut_ptr()),
            ..PROPBAG2::default()
        };
        let mut variant = VARIANT::default();
        // SAFETY: `variant` is a zeroed, owned VARIANT; writing the type tag and the `fltVal` member of the freshly
        // initialised union is sound and `VT_R4` needs no cleanup.
        unsafe {
            let inner = &mut *variant.Anonymous.Anonymous;
            inner.vt = VARENUM(VT_R4);
            inner.Anonymous.fltVal = quality;
        }
        // SAFETY: `options` is a live `IPropertyBag2`; `bag` and `variant` are valid for one property and outlive the call,
        // and `name` (referenced by `bag`) is alive until the end of this function.
        let written = unsafe { options.Write(1, &bag, &variant) };
        // Keep the VARIANT from running a destructor over the union: nothing to clear for VT_R4.
        std::mem::forget(variant);
        written.map_err(|e| encode_com(&e))
    }

    /// Steps of C# `Encode` (after validation): factory; `CreateEncoder(container, VENDOR_MICROSOFT)`;
    /// `Initialize(IStream adapter over output, WIC_BITMAP_ENCODER_NO_CACHE)`; `CreateNewFrame`; when `quality > 0` write
    /// `ImageQuality` (`VT_R4`, `image_quality_value`) into the option bag; `frame.Initialize(options)` (also for PNG,
    /// with the untouched bag); `SetSize`; `SetResolution(DPI, DPI)`;
    /// `SetPixelFormat(source_format)`; then by [`super::plan_encode_path`]: `WritePixels` of the whole buffer, or the
    /// band loop of `bgrx32_to_bgr24` (one band buffer of `rows_per_band * width * 3` bytes, reused), or a
    /// temporary `IWICBitmap` (`CreateBitmapFromMemory`) + `WriteSource`; then `frame.Commit()`, `encoder.Commit()`.
    /// Everything is released on every path; the pixel buffer stays alive until after `Commit`.
    pub(crate) fn encode(
        pixels: &PixelBuffer,
        output: &mut dyn Write,
        container: &Guid,
        quality: i32,
    ) -> Result<(), EncodeError> {
        // Contract note 10: WIC writes into an in-memory COM stream; the bytes are copied to `output` after `Commit`.
        // Release order (reverse of declaration): options, frame, encoder, factory, stream.
        let stream = ComStream::writable_memory();
        let factory = interop::create_factory().map_err(|e| encode_error(&e))?;
        let container_format = guid_to_windows(container);
        let vendor = guid_to_windows(&VENDOR_MICROSOFT);
        // SAFETY: `factory` is a live `IWICImagingFactory`; both GUID pointers are valid for the call.
        let encoder: Com<IWICBitmapEncoder> =
            Com::adopt(unsafe { factory.CreateEncoder(&container_format, &vendor) }).map_err(|e| encode_error(&e))?;
        let istream = stream.istream();
        // SAFETY: `encoder` is a live encoder; `istream` is a live COM stream (adapter over the owner's memory stream)
        // that stays alive until the end of this function.
        unsafe {
            encoder.Initialize(
                &istream,
                WICBitmapEncoderCacheOption(WIC_BITMAP_ENCODER_NO_CACHE as i32),
            )
        }
        .map_err(|e| encode_com(&e))?;

        let mut frame_out: Option<IWICBitmapFrameEncode> = None;
        let mut options_out: Option<IPropertyBag2> = None;
        // SAFETY: `encoder` is initialised; both out pointers refer to valid `Option` locals.
        unsafe { encoder.CreateNewFrame(&mut frame_out, &mut options_out) }.map_err(|e| encode_com(&e))?;
        let frame = Com::adopt(frame_out.ok_or_else(windows_core::Error::empty)).map_err(|e| encode_error(&e))?;
        let options = Com::adopt(options_out.ok_or_else(windows_core::Error::empty)).map_err(|e| encode_error(&e))?;

        if quality > 0 {
            write_image_quality(&options, image_quality_value(quality))?;
        }
        let width = pixels.width();
        let height = pixels.height();
        let layout_format = guid_to_windows(&source_format(pixels.layout()));
        let mut accepted = layout_format;
        // SAFETY: `frame` is a live frame encoder and `options` the bag it handed out; `accepted` is a valid in/out GUID.
        unsafe {
            frame.Initialize(&*options).map_err(|e| encode_com(&e))?;
            frame.SetSize(width as u32, height as u32).map_err(|e| encode_com(&e))?;
            frame.SetResolution(DPI, DPI).map_err(|e| encode_com(&e))?;
            frame.SetPixelFormat(&mut accepted).map_err(|e| encode_com(&e))?;
        }

        let path = plan_encode_path(&guid_from_windows(&layout_format), &guid_from_windows(&accepted));
        match path {
            EncodePath::Direct => {
                let bytes = pixels.bytes().map_err(|_| EncodeError::Disposed)?;
                // SAFETY: `frame` is configured; `bytes` is the whole buffer (`height * stride` bytes, `<= u32::MAX`
                // checked by the caller) and outlives the call.
                unsafe { frame.WritePixels(height as u32, pixels.stride() as u32, bytes) }
                    .map_err(|e| encode_com(&e))?;
            }
            EncodePath::Bgr24Bands => {
                let row_bytes = width as usize * 3;
                let rows_per = rows_per_band(width, height) as usize;
                let mut band = vec![0u8; rows_per * row_bytes];
                let total = height as usize;
                let mut y = 0usize;
                while y < total {
                    let rows = rows_per.min(total - y);
                    for r in 0..rows {
                        let source = pixels.get_row((y + r) as i32).map_err(|_| EncodeError::Disposed)?;
                        bgrx32_to_bgr24(source, &mut band[r * row_bytes..(r + 1) * row_bytes])
                            .map_err(|_| EncodeError::Com { hresult: E_FAIL })?;
                    }
                    // SAFETY: `frame` is configured; the slice is `rows * row_bytes` initialised bytes of `band`.
                    unsafe { frame.WritePixels(rows as u32, row_bytes as u32, &band[..rows * row_bytes]) }
                        .map_err(|e| encode_com(&e))?;
                    y += rows;
                }
            }
            EncodePath::WicConversion => {
                let bytes = pixels.bytes().map_err(|_| EncodeError::Disposed)?;
                // SAFETY: `factory` is live; `layout_format` outlives the call; `bytes` is the whole buffer and is only
                // read (WIC copies it into the temporary bitmap).
                let bitmap: Com<IWICBitmap> = Com::adopt(unsafe {
                    factory.CreateBitmapFromMemory(
                        width as u32,
                        height as u32,
                        &layout_format,
                        pixels.stride() as u32,
                        bytes,
                    )
                })
                .map_err(|e| encode_error(&e))?;
                // SAFETY: `frame` is configured and `bitmap` is a live bitmap source; a null rect means the whole image.
                unsafe { frame.WriteSource(&*bitmap, std::ptr::null()) }.map_err(|e| encode_com(&e))?;
            }
        }

        // SAFETY: `frame` and `encoder` are live and fully written; `pixels` is borrowed until after the commits.
        unsafe {
            frame.Commit().map_err(|e| encode_com(&e))?;
            encoder.Commit().map_err(|e| encode_com(&e))?;
        }

        let backend = stream.dispose().ok_or(EncodeError::Com { hresult: E_FAIL })?;
        let payload = backend
            .into_any()
            .downcast::<Cursor<Vec<u8>>>()
            .map_err(|_| EncodeError::Com { hresult: E_FAIL })?;
        super::finish_output(
            *container == super::CONTAINER_FORMAT_JPEG,
            output.write_all(payload.get_ref()),
        )
    }
}
