//! Port of `PhotoReview.Imaging.LibRaw/RgbBgraResampler.cs` (C# baseline d92d6aae): packed 8-bit RGB (or 1-channel gray)
//! from LibRaw -> BGRA (Bgr32, alpha 255) for the full-decode path. Pure managed pixel code, no native library.
//!
//! Three paths, chosen from the ratios `source / target` per axis ([`select_path`]): same size -> plain channel swap
//! ([`ResamplePath::Convert`]); either axis reduced by MORE than 2.0 -> area average ([`ResamplePath::BoxAverage`], rounded
//! half up in integers); otherwise bilinear at pixel centres ([`ResamplePath::Bilinear`], `f64`, rounded half to EVEN).
//! [`resize_to_buffer`] writes straight into a native [`BgraBuffer`] (an owner over `pr_imaging::pixels::PixelBuffer`).
//!
//! PARITY: I-15 (RAW full decode pixel path). No `unsafe` in this file.

use std::fmt;

use pr_core::sync::{CancelToken, Canceled};
use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};

/// Above this per-axis reduction ratio (strictly greater) bilinear would skip source pixels, so the area average is used.
pub const BOX_AVERAGE_RATIO_THRESHOLD: f64 = 2.0;

/// .NET `Array.MaxLength` (0x7FFFFFC7): the largest BGRA byte count [`validate_target_length`] accepts.
pub const MAX_ARRAY_LENGTH: i64 = 0x7FFF_FFC7;

/// Every failure of this module. The .NET exception each variant stands for is given by [`ResampleError::dotnet_type_name`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResampleError {
    /// `InvalidDataException` (the decoder contract: every bad header, size or buffer is this, never an overflow). Carries the
    /// C# message verbatim where the C# has one; Rust-only argument checks (a slice shorter than the geometry) use their own text.
    InvalidData(String),
    /// `OperationCanceledException` (the token was cancelled at one of the check points).
    Canceled,
    /// `OverflowException` of `checked(width * 4)` / `checked(stride * height)` in the `BgraBuffer` constructor.
    Overflow,
    /// `ObjectDisposedException` on a `BgraBuffer` that was disposed or whose pixels were taken.
    Disposed,
    /// `PixelBuffer.Allocate` refused the geometry or memory (`ArgumentOutOfRangeException` / `OutOfMemoryException`).
    PixelBuffer(PixelBufferError),
}

impl fmt::Display for ResampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidData(message) => f.write_str(message),
            Self::Canceled => f.write_str("The operation was canceled."),
            Self::Overflow => f.write_str("Arithmetic operation resulted in an overflow."),
            Self::Disposed => f.write_str(
                "Cannot access a disposed object.\r\nObject name: 'PhotoReview.Imaging.LibRaw.RgbBgraResampler+BgraBuffer'.",
            ),
            Self::PixelBuffer(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for ResampleError {}

impl From<Canceled> for ResampleError {
    fn from(_: Canceled) -> Self {
        Self::Canceled
    }
}

impl ResampleError {
    /// The .NET exception type name (the golden compares these): `InvalidDataException`, `OperationCanceledException`,
    /// `OverflowException`, `ObjectDisposedException`, or the inner [`PixelBufferError::dotnet_type_name`].
    pub fn dotnet_type_name(&self) -> &'static str {
        match self {
            Self::InvalidData(_) => "InvalidDataException",
            Self::Canceled => "OperationCanceledException",
            Self::Overflow => "OverflowException",
            Self::Disposed => "ObjectDisposedException",
            Self::PixelBuffer(error) => error.dotnet_type_name(),
        }
    }
}

/// Which of the three algorithms [`resize`] runs for a geometry (the C# `Resize` if/else-if/else chain as a value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResamplePath {
    /// `target == source` on both axes.
    Convert,
    /// Not the same size and `source_width / target_width > 2.0` or `source_height / target_height > 2.0` (`f64` division).
    BoxAverage,
    /// Everything else (upscaling, mild reduction, a ratio of exactly 2.0).
    Bilinear,
}

/// `channels` is 1 (gray, expanded to B = G = R) or 3 (R, G, B). C# `IsSupportedChannelCount`.
pub fn is_supported_channel_count(channels: i32) -> bool {
    channels == 1 || channels == 3
}

/// C# `ValidateSourceLength(width, height, dataSize, channels)` (Rust has no default: callers pass 3 for RGB).
/// Order: unsupported channels -> `InvalidData("LibRaw returned an unsupported processed image format.")`; then
/// `rgb_length = (width as i64 * height as i64).wrapping_mul(channels as i64)` (C# `(long)width * height * channels` is UNCHECKED:
/// `i32::MAX x i32::MAX x 3` wraps negative and PASSES the checks, returning `rgb_length as i32` truncated; keep this, golden
/// pins it); then `width <= 0 || height <= 0 || data_size < rgb_length || rgb_length > i32::MAX` ->
/// `InvalidData("LibRaw returned an invalid processed image buffer length.")`. Returns `rgb_length as i32`.
pub fn validate_source_length(width: i32, height: i32, data_size: i64, channels: i32) -> Result<i32, ResampleError> {
    let _ = (width, height, data_size, channels);
    todo!("W5-17: validate_source_length")
}

/// C# `ValidateTargetLength(width, height)`: `length = (width as i64 * height as i64).wrapping_mul(4)` (unchecked in C#, same wrap
/// trap as above: `i32::MAX x i32::MAX` returns 4); `width <= 0 || height <= 0 || length > MAX_ARRAY_LENGTH` ->
/// `InvalidData("The decoded RAW image is too large to hold in memory at {width}x{height}.")`. Returns `length as i32`.
pub fn validate_target_length(width: i32, height: i32) -> Result<i32, ResampleError> {
    let _ = (width, height);
    todo!("W5-17: validate_target_length")
}

/// The path [`resize`] takes. Pure function of the four sizes (`f64` ratios, as C#); a zero target size gives an infinite ratio,
/// i.e. `BoxAverage`, unless the sizes are equal. Not a C# method (the chain is inline there): exposed so it can be unit-tested.
pub fn select_path(source_width: i32, source_height: i32, target_width: i32, target_height: i32) -> ResamplePath {
    let _ = (source_width, source_height, target_width, target_height);
    todo!("W5-17: select_path")
}

/// C# `Resize(rgb, sw, sh, bgra, tw, th, ct, channels)`: fills the first `tw * th * 4` bytes of `bgra` (B, G, R, 255 per pixel).
///
/// Errors, in order: unsupported `channels` -> `InvalidData("LibRaw returned an unsupported processed image format.")`; Rust-only
/// argument checks (see the contract, "Different"): a non-positive size or a slice shorter than the geometry -> `InvalidData`
/// (C# would throw `IndexOutOfRangeException` / `ArgumentOutOfRangeException` / `DivideByZeroException` there; callers validate
/// with [`validate_source_length`] first). Cancellation: [`ResamplePath::Convert`] checks the token before pixel 0 and then every
/// 262144 pixels (so a cancelled token always fails, even for 1 pixel); the other two check after each finished target row.
#[allow(clippy::too_many_arguments)]
pub fn resize(
    rgb: &[u8],
    source_width: i32,
    source_height: i32,
    bgra: &mut [u8],
    target_width: i32,
    target_height: i32,
    cancel: &CancelToken,
    channels: i32,
) -> Result<(), ResampleError> {
    let _ = (
        rgb,
        source_width,
        source_height,
        bgra,
        target_width,
        target_height,
        cancel,
        channels,
    );
    todo!("W5-17: resize")
}

/// C# `ResizeToBuffer`: guard (any size `<= 0` -> `InvalidData("Invalid RAW resize dimensions {sw}x{sh} -> {tw}x{th}.")`,
/// before any allocation), then [`BgraBuffer::new`] (`Overflow` / `PixelBuffer` errors), then [`resize`]; on any error the buffer
/// is dropped (freed) and the error returned.
pub fn resize_to_buffer(
    rgb: &[u8],
    source_width: i32,
    source_height: i32,
    target_width: i32,
    target_height: i32,
    channels: i32,
    cancel: &CancelToken,
) -> Result<BgraBuffer, ResampleError> {
    let _ = (
        rgb,
        source_width,
        source_height,
        target_width,
        target_height,
        channels,
        cancel,
    );
    todo!("W5-17: resize_to_buffer")
}

/// BGRA (Bgr32) target of a resample: a thin owner over a native `PixelBuffer` (64-byte aligned, `stride = width * 4`, no padding).
/// Dropping (or [`BgraBuffer::dispose`]) frees it unless [`BgraBuffer::take_pixels`] handed it over.
#[derive(Debug)]
pub struct BgraBuffer {
    width: i32,
    height: i32,
    stride: i32,
    length: i32,
    pixels: Option<PixelBuffer>,
}

impl BgraBuffer {
    /// C# constructor, in this order: `stride = width.checked_mul(4)` and `length = stride.checked_mul(height)` (`None` ->
    /// [`ResampleError::Overflow`]; non-positive operands that do not overflow continue), then
    /// `PixelBuffer::allocate(width, height, PixelLayout::Bgr32)` (`Err(e)` -> `ResampleError::PixelBuffer(e)`).
    /// So a `BgraBuffer` never exceeds `i32::MAX` bytes.
    pub fn new(width: i32, height: i32) -> Result<Self, ResampleError> {
        let _ = (width, height, PixelLayout::Bgr32);
        todo!("W5-17: BgraBuffer::new")
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn stride(&self) -> i32 {
        self.stride
    }

    /// `stride * height` (always `<= i32::MAX`).
    pub fn length(&self) -> i32 {
        self.length
    }

    /// C# `Pointer`: the 64-byte aligned start while the pixels are owned, null after dispose or `take_pixels`.
    pub fn pointer(&self) -> *const u8 {
        let _ = &self.pixels;
        todo!("W5-17: BgraBuffer::pointer")
    }

    /// C# `AsSpan`: the writable bytes. `Err(Disposed)` after dispose/take; `PixelBuffer::try_get_span_mut` returning `None`
    /// (unreachable: the length is `<= i32::MAX`) is `InvalidData("The decoded RAW image is too large to hold in memory.")`.
    pub fn as_mut_slice(&mut self) -> Result<&mut [u8], ResampleError> {
        todo!("W5-17: BgraBuffer::as_mut_slice")
    }

    /// Read-only view (Rust addition for tests and the hand-off; same errors).
    pub fn as_slice(&self) -> Result<&[u8], ResampleError> {
        todo!("W5-17: BgraBuffer::as_slice")
    }

    /// C# `TakePixels`: moves the `PixelBuffer` (and its ownership) out, no copy; the second call is `Err(Disposed)`.
    pub fn take_pixels(&mut self) -> Result<PixelBuffer, ResampleError> {
        todo!("W5-17: BgraBuffer::take_pixels")
    }

    /// C# `Dispose`: frees the pixels now; idempotent.
    pub fn dispose(&mut self) {
        todo!("W5-17: BgraBuffer::dispose")
    }
}
