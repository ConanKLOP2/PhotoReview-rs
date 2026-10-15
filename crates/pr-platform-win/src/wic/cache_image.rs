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
#![allow(unused_variables, dead_code)] // D11: only while a body is `todo!()`; the porter removes this line.

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
    todo!("Pbgra32 => PIXEL_FORMAT_32BPP_PBGRA, else PIXEL_FORMAT_32BPP_BGR")
}

/// C# `WicCacheImageReader.IsOutOfMemory(COMException)`: `hresult == E_OUTOFMEMORY` and nothing else. A resource failure,
/// never "corrupt entry".
pub fn is_out_of_memory(hresult: i32) -> bool {
    todo!("hresult == E_OUTOFMEMORY")
}

/// C# `WicCacheImageReader.IsOpaqueFormat(format)`: exact 16-byte membership in [`OPAQUE_FORMATS`].
pub fn is_opaque_format(format: &Guid) -> bool {
    todo!("OPAQUE_FORMATS.contains(format)")
}

/// The layout a payload decodes to when the caller asks for none: opaque frame => `Bgr32`, anything else => `Pbgra32`.
pub fn target_layout(requested: Option<PixelLayout>, frame_format: &Guid) -> PixelLayout {
    todo!("requested, else Bgr32 for an opaque frame format and Pbgra32 otherwise")
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
    /// A WIC failure other than `E_OUTOFMEMORY` (C# raw `COMException`): a damaged payload. `hresult` is signed.
    Com { hresult: i32 },
    /// `E_OUTOFMEMORY` from WIC (C# `InsufficientMemoryException`): not a damaged entry, the entry is kept.
    InsufficientMemory { hresult: i32 },
    /// The destination buffer could not be allocated or described.
    Pixels(PixelBufferError),
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
        todo!(
            "InvalidData => the text; InsufficientMemory => \"Not enough memory to decode the cache entry.\"; Com => the HRESULT in hex"
        )
    }
}

impl std::error::Error for CacheImageError {}

impl CacheImageError {
    /// `DiskCacheStore.IsCacheEntryFailure` for these types: `InvalidData` and `Com` are entry failures (delete the entry,
    /// decode again from the source); `InsufficientMemory` and `Pixels(OutOfMemory)` are not (the entry is kept and the
    /// error propagates).
    pub fn is_entry_failure(&self) -> bool {
        todo!()
    }
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
    todo!()
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
    todo!()
}

/// C# `Math.Clamp(BandBytes / (width * 3), 1, height)`: rows per band of the BGRX -> BGR conversion.
/// `width >= 1`, `height >= 1`.
pub fn rows_per_band(width: i32, height: i32) -> i32 {
    todo!()
}

/// C# `quality / 100f`: the `ImageQuality` value written to the property bag (`VT_R4`).
pub fn image_quality_value(quality: i32) -> f32 {
    todo!("quality as f32 / 100.0")
}

/// Destination of [`bgrx32_to_bgr24`] is shorter than 3 bytes per source pixel (C# `ArgumentException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DestinationTooShort {
    pub required: usize,
    pub actual: usize,
}

impl fmt::Display for DestinationTooShort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("\"Destination is too short.\"")
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
    todo!()
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
        todo!()
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
    todo!()
}

/// C# `WicImageEncoder.EncodePng`: PNG with WIC default options (no interlace), same parameters as `PngBitmapEncoder`.
pub fn encode_png(pixels: &PixelBuffer, output: &mut dyn Write) -> Result<(), EncodeError> {
    todo!()
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
    todo!("empty check, then native::decode_payload")
}

// ---------------------------------------------------------------------------------------------------------------------
// Native seam: the only `unsafe` of this file (W4-11a wrappers; one `// SAFETY:` per block)
// ---------------------------------------------------------------------------------------------------------------------

pub(crate) mod native {
    use super::{CacheImageError, DecodeBox, EncodeError, PixelBuffer, PixelLayout, WicCacheImageInfo};
    use pr_core::catalog::explorer_types::Guid;
    use std::io::Write;

    /// C# `PropBag2` (wincodec `PROPBAG2`): `ImageQuality` is written through it. Natural `repr(C)` layout, 8-byte aligned
    /// pointer on x64. Delete this struct when the `windows` crate's `PROPBAG2` is used (contract, Depends on).
    #[repr(C)]
    pub(crate) struct PropBag2 {
        pub dw_type: u32,
        pub vt: u16,
        pub cf_type: u16,
        pub dw_hint: u32,
        pub pstr_name: *mut u16,
        pub clsid: [u8; 16],
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
        todo!("W4-11a wrappers; see the doc comment for the exact call order")
    }

    /// Steps of C# `Encode` (after validation): factory; `CreateEncoder(container, VENDOR_MICROSOFT)`;
    /// `Initialize(IStream adapter over output, WIC_BITMAP_ENCODER_NO_CACHE)`; `CreateNewFrame`; when `quality > 0` write
    /// `ImageQuality` (`VT_R4`, `image_quality_value`) into the option bag through [`PropBag2`] and a 24-byte zeroed
    /// VARIANT (type at byte 0, float at byte 8); `frame.Initialize(options)` (also for PNG, with the untouched bag);
    /// `SetSize`; `SetResolution(DPI, DPI)`;
    /// `SetPixelFormat(source_format)`; then by [`super::plan_encode_path`]: `WritePixels` of the whole buffer, or the
    /// band loop of `bgrx32_to_bgr24` (one pooled band buffer of `rows_per_band * width * 3` bytes, reused), or a
    /// temporary `IWICBitmap` (`CreateBitmapFromMemory`) + `WriteSource`; then `frame.Commit()`, `encoder.Commit()`.
    /// Everything is released on every path; the pixel buffer stays alive until after `Commit`.
    pub(crate) fn encode(
        pixels: &PixelBuffer,
        output: &mut dyn Write,
        container: &Guid,
        quality: i32,
    ) -> Result<(), EncodeError> {
        todo!("W4-11a wrappers; see the doc comment for the exact call order")
    }
}
