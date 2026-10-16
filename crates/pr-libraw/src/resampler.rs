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
use pr_imaging::pixels::{BYTES_PER_PIXEL, PixelBuffer, PixelBufferError, PixelLayout};

/// Above this per-axis reduction ratio (strictly greater) bilinear would skip source pixels, so the area average is used.
pub const BOX_AVERAGE_RATIO_THRESHOLD: f64 = 2.0;

/// .NET `Array.MaxLength` (0x7FFFFFC7): the largest BGRA byte count [`validate_target_length`] accepts.
pub const MAX_ARRAY_LENGTH: i64 = 0x7FFF_FFC7;

/// C# message of the unsupported channel count (`IsSupportedChannelCount` is false).
const UNSUPPORTED_FORMAT: &str = "LibRaw returned an unsupported processed image format.";

/// C# message of a source header whose byte count is absurd, inconsistent or too short.
const INVALID_BUFFER_LENGTH: &str = "LibRaw returned an invalid processed image buffer length.";

/// C# `Convert` tests the token whenever `(pixel_index & 0x3FFFF) == 0`: a block of 262144 pixels per check.
const CONVERT_CHECK_STRIDE: usize = 0x4_0000;

fn unsupported_format() -> ResampleError {
    ResampleError::InvalidData(UNSUPPORTED_FORMAT.to_owned())
}

/// Byte offsets of the green and blue channels inside one source pixel (C# `greenOffset` / `blueOffset`). Gray collapses both to 0.
fn channel_offsets(channels: usize) -> (usize, usize) {
    if channels == 3 { (1, 2) } else { (0, 0) }
}

/// The source footprint `[start, end)` of target index `index` on one axis (C# `y0` / `y1` of `BoxAverage`), in `i64` arithmetic.
fn box_footprint(index: usize, source: i64, target: i64) -> (usize, usize) {
    let i = index as i64;
    let start = i * source / target;
    let end = std::cmp::max(start + 1, (i + 1) * source / target);
    (start as usize, end as usize)
}

/// Bilinear sample position of target index `index` on one axis (C# `sourceY` / `y0` / `y1` / `fy` of `Bilinear`).
/// Returns `(lower, upper, fraction)` with `lower <= upper < source`.
fn bilinear_sample(index: usize, source: i64, target: i64) -> (usize, usize, f64) {
    let max = (source - 1) as f64;
    // C# evaluation order: `((y + 0.5) * sourceHeight) / targetHeight - 0.5`, then `Math.Clamp(value, 0, size - 1)`.
    let position = ((index as f64 + 0.5) * source as f64) / target as f64 - 0.5;
    let clamped = if position < 0.0 {
        0.0
    } else if position > max {
        max
    } else {
        position
    };
    let lower = clamped as usize;
    let upper = std::cmp::min(lower + 1, (source - 1) as usize);
    (lower, upper, clamped - lower as f64)
}

/// C# `Interpolate`: `corners` are the byte offsets of the top-left, top-right, bottom-left and bottom-right source pixels.
/// Subtractions are integer (`i32`), the blend is `f64`, and the result is `Math.Round` (half to EVEN) then clamped.
fn interpolate(rgb: &[u8], corners: [usize; 4], channel: usize, fx: f64, fy: f64) -> u8 {
    let top_left = i32::from(rgb[corners[0] + channel]);
    let top_right = i32::from(rgb[corners[1] + channel]);
    let bottom_left = i32::from(rgb[corners[2] + channel]);
    let bottom_right = i32::from(rgb[corners[3] + channel]);
    let top = f64::from(top_left) + f64::from(top_right - top_left) * fx;
    let bottom = f64::from(bottom_left) + f64::from(bottom_right - bottom_left) * fx;
    let value = (top + (bottom - top) * fy).round_ties_even();
    value.clamp(0.0, 255.0) as u8
}

/// C# `Convert`: channel swap at equal size, alpha 255. Pixel count is `rgb.len() / channels`, not `sw * sh`.
fn convert(rgb: &[u8], bgra: &mut [u8], channels: usize, cancel: &CancelToken) -> Result<(), ResampleError> {
    let pixel_count = rgb.len() / channels;
    if (pixel_count as i64) * 4 > bgra.len() as i64 {
        return Err(ResampleError::InvalidData(
            "The RGB source pixel count does not fit the BGRA target.".to_owned(),
        ));
    }
    let (green, blue) = channel_offsets(channels);
    let mut first = 0;
    while first < pixel_count {
        // Checked before the block's first pixel, so a cancelled token fails before anything is written (index 0 included).
        cancel.check()?;
        let last = std::cmp::min(first + CONVERT_CHECK_STRIDE, pixel_count);
        let source = &rgb[first * channels..last * channels];
        let target = &mut bgra[first * 4..last * 4];
        for (src, dst) in source.chunks_exact(channels).zip(target.chunks_exact_mut(4)) {
            dst[0] = src[blue];
            dst[1] = src[green];
            dst[2] = src[0];
            dst[3] = u8::MAX;
        }
        first = last;
    }
    Ok(())
}

/// Validated geometry of one resample (all sizes positive, channel count 1 or 3, every size already fits `i32`).
#[derive(Debug, Clone, Copy)]
struct Geometry {
    source_width: usize,
    source_height: usize,
    target_width: usize,
    target_height: usize,
    channels: usize,
}

/// C# `BoxAverage`: every target pixel is the rounded (half up, integers) mean of its source footprint. Token checked after each row.
fn box_average(rgb: &[u8], geometry: Geometry, bgra: &mut [u8], cancel: &CancelToken) -> Result<(), ResampleError> {
    let Geometry {
        source_width,
        source_height,
        target_width,
        target_height,
        channels,
    } = geometry;
    let (green, blue) = channel_offsets(channels);
    let (sw, sh) = (source_width as i64, source_height as i64);
    let (tw, th) = (target_width as i64, target_height as i64);
    let columns: Vec<(usize, usize)> = (0..target_width).map(|x| box_footprint(x, sw, tw)).collect();
    for (y, row) in bgra.chunks_exact_mut(target_width * 4).take(target_height).enumerate() {
        let (y0, y1) = box_footprint(y, sh, th);
        for (&(x0, x1), out) in columns.iter().zip(row.chunks_exact_mut(4)) {
            let (mut red, mut green_sum, mut blue_sum) = (0i64, 0i64, 0i64);
            for sy in y0..y1 {
                let row_start = sy * source_width;
                for sx in x0..x1 {
                    let source = (row_start + sx) * channels;
                    red += i64::from(rgb[source]);
                    green_sum += i64::from(rgb[source + green]);
                    blue_sum += i64::from(rgb[source + blue]);
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as i64;
            out[0] = ((blue_sum + count / 2) / count) as u8;
            out[1] = ((green_sum + count / 2) / count) as u8;
            out[2] = ((red + count / 2) / count) as u8;
            out[3] = u8::MAX;
        }
        cancel.check()?;
    }
    Ok(())
}

/// C# `Bilinear`: pixel-centre sampling with `f64` weights. Token checked after each row.
fn bilinear(rgb: &[u8], geometry: Geometry, bgra: &mut [u8], cancel: &CancelToken) -> Result<(), ResampleError> {
    let Geometry {
        source_width,
        source_height,
        target_width,
        target_height,
        channels,
    } = geometry;
    let (green, blue) = channel_offsets(channels);
    let (sw, sh) = (source_width as i64, source_height as i64);
    let (tw, th) = (target_width as i64, target_height as i64);
    let columns: Vec<(usize, usize, f64)> = (0..target_width).map(|x| bilinear_sample(x, sw, tw)).collect();
    for (y, row) in bgra.chunks_exact_mut(target_width * 4).take(target_height).enumerate() {
        let (y0, y1, fy) = bilinear_sample(y, sh, th);
        let top_row = y0 * source_width;
        let bottom_row = y1 * source_width;
        for (&(x0, x1, fx), out) in columns.iter().zip(row.chunks_exact_mut(4)) {
            let corners = [
                (top_row + x0) * channels,
                (top_row + x1) * channels,
                (bottom_row + x0) * channels,
                (bottom_row + x1) * channels,
            ];
            out[0] = interpolate(rgb, corners, blue, fx, fy);
            out[1] = interpolate(rgb, corners, green, fx, fy);
            out[2] = interpolate(rgb, corners, 0, fx, fy);
            out[3] = u8::MAX;
        }
        cancel.check()?;
    }
    Ok(())
}

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
    if !is_supported_channel_count(channels) {
        return Err(unsupported_format());
    }
    // Both factors are `i32`, so their product fits `i64`; only the multiplication by `channels` can wrap (C# unchecked).
    let rgb_length = (i64::from(width) * i64::from(height)).wrapping_mul(i64::from(channels));
    if width <= 0 || height <= 0 || data_size < rgb_length || rgb_length > i64::from(i32::MAX) {
        return Err(ResampleError::InvalidData(INVALID_BUFFER_LENGTH.to_owned()));
    }
    Ok(rgb_length as i32)
}

/// C# `ValidateTargetLength(width, height)`: `length = (width as i64 * height as i64).wrapping_mul(4)` (unchecked in C#, same wrap
/// trap as above: `i32::MAX x i32::MAX` returns 4); `width <= 0 || height <= 0 || length > MAX_ARRAY_LENGTH` ->
/// `InvalidData("The decoded RAW image is too large to hold in memory at {width}x{height}.")`. Returns `length as i32`.
pub fn validate_target_length(width: i32, height: i32) -> Result<i32, ResampleError> {
    // Same unchecked wrap as the source validator (C# `(long)width * height * 4`), kept for parity.
    let length = (i64::from(width) * i64::from(height)).wrapping_mul(i64::from(BYTES_PER_PIXEL));
    if width <= 0 || height <= 0 || length > MAX_ARRAY_LENGTH {
        return Err(ResampleError::InvalidData(format!(
            "The decoded RAW image is too large to hold in memory at {width}x{height}."
        )));
    }
    Ok(length as i32)
}

/// The path [`resize`] takes. Pure function of the four sizes (`f64` ratios, as C#); a zero target size gives an infinite ratio,
/// i.e. `BoxAverage`, unless the sizes are equal. Not a C# method (the chain is inline there): exposed so it can be unit-tested.
pub fn select_path(source_width: i32, source_height: i32, target_width: i32, target_height: i32) -> ResamplePath {
    if source_width == target_width && source_height == target_height {
        ResamplePath::Convert
    } else if f64::from(source_width) / f64::from(target_width) > BOX_AVERAGE_RATIO_THRESHOLD
        || f64::from(source_height) / f64::from(target_height) > BOX_AVERAGE_RATIO_THRESHOLD
    {
        ResamplePath::BoxAverage
    } else {
        ResamplePath::Bilinear
    }
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
    if !is_supported_channel_count(channels) {
        return Err(unsupported_format());
    }
    if source_width <= 0 || source_height <= 0 || target_width <= 0 || target_height <= 0 {
        return Err(ResampleError::InvalidData(format!(
            "Invalid RAW resize dimensions {source_width}x{source_height} -> {target_width}x{target_height}."
        )));
    }
    let geometry = Geometry {
        source_width: source_width as usize,
        source_height: source_height as usize,
        target_width: target_width as usize,
        target_height: target_height as usize,
        channels: channels as usize,
    };
    match select_path(source_width, source_height, target_width, target_height) {
        // `convert` checks the BGRA length itself (its pixel count comes from `rgb.len()`).
        ResamplePath::Convert => convert(rgb, bgra, geometry.channels, cancel),
        ResamplePath::BoxAverage => {
            check_source_length(rgb, source_width, source_height, channels)?;
            check_target_length(target_width, target_height, bgra)?;
            box_average(rgb, geometry, bgra, cancel)
        }
        ResamplePath::Bilinear => {
            check_source_length(rgb, source_width, source_height, channels)?;
            check_target_length(target_width, target_height, bgra)?;
            bilinear(rgb, geometry, bgra, cancel)
        }
    }
}

/// Box and bilinear read `sw * sh * channels` source bytes (`i64` product of positive `i32`s). Convert does not call this: like
/// C# it converts `rgb.len() / channels` pixels and ignores the source geometry.
fn check_source_length(rgb: &[u8], source_width: i32, source_height: i32, channels: i32) -> Result<(), ResampleError> {
    let needed = i64::from(source_width) * i64::from(source_height) * i64::from(channels);
    if (rgb.len() as i64) < needed {
        return Err(ResampleError::InvalidData(
            "The RGB source is shorter than its geometry.".to_owned(),
        ));
    }
    Ok(())
}

/// The BGRA slice must hold `tw * th * 4` bytes (`i64` product, the sizes are positive `i32`s).
fn check_target_length(target_width: i32, target_height: i32, bgra: &[u8]) -> Result<(), ResampleError> {
    let needed = i64::from(target_width) * i64::from(target_height) * i64::from(BYTES_PER_PIXEL);
    if (bgra.len() as i64) < needed {
        return Err(ResampleError::InvalidData(
            "The BGRA target is shorter than its geometry.".to_owned(),
        ));
    }
    Ok(())
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
    if source_width <= 0 || source_height <= 0 || target_width <= 0 || target_height <= 0 {
        return Err(ResampleError::InvalidData(format!(
            "Invalid RAW resize dimensions {source_width}x{source_height} -> {target_width}x{target_height}."
        )));
    }
    let mut buffer = BgraBuffer::new(target_width, target_height)?;
    resize(
        rgb,
        source_width,
        source_height,
        buffer.as_mut_slice()?,
        target_width,
        target_height,
        cancel,
        channels,
    )?;
    Ok(buffer)
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
        let stride = width.checked_mul(BYTES_PER_PIXEL).ok_or(ResampleError::Overflow)?;
        let length = stride.checked_mul(height).ok_or(ResampleError::Overflow)?;
        let pixels = PixelBuffer::allocate(width, height, PixelLayout::Bgr32).map_err(ResampleError::PixelBuffer)?;
        Ok(Self {
            width,
            height,
            stride,
            length,
            pixels: Some(pixels),
        })
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
        match self.pixels.as_ref().map(PixelBuffer::address) {
            Some(Ok(address)) => address,
            _ => std::ptr::null(),
        }
    }

    /// C# `AsSpan`: the writable bytes. `Err(Disposed)` after dispose/take; `PixelBuffer::try_get_span_mut` returning `None`
    /// (unreachable: the length is `<= i32::MAX`) is `InvalidData("The decoded RAW image is too large to hold in memory.")`.
    pub fn as_mut_slice(&mut self) -> Result<&mut [u8], ResampleError> {
        let pixels = self.pixels.as_mut().ok_or(ResampleError::Disposed)?;
        match pixels.try_get_span_mut() {
            Ok(Some(span)) => Ok(span),
            Ok(None) => Err(too_large_to_hold()),
            Err(error) => Err(ResampleError::PixelBuffer(error)),
        }
    }

    /// Read-only view (Rust addition for tests and the hand-off; same errors).
    pub fn as_slice(&self) -> Result<&[u8], ResampleError> {
        let pixels = self.pixels.as_ref().ok_or(ResampleError::Disposed)?;
        match pixels.try_get_span() {
            Ok(Some(span)) => Ok(span),
            Ok(None) => Err(too_large_to_hold()),
            Err(error) => Err(ResampleError::PixelBuffer(error)),
        }
    }

    /// C# `TakePixels`: moves the `PixelBuffer` (and its ownership) out, no copy; the second call is `Err(Disposed)`.
    pub fn take_pixels(&mut self) -> Result<PixelBuffer, ResampleError> {
        self.pixels.take().ok_or(ResampleError::Disposed)
    }

    /// C# `Dispose`: frees the pixels now; idempotent.
    pub fn dispose(&mut self) {
        if let Some(mut pixels) = self.pixels.take() {
            pixels.dispose();
        }
    }
}

fn too_large_to_hold() -> ResampleError {
    ResampleError::InvalidData("The decoded RAW image is too large to hold in memory.".to_owned())
}
