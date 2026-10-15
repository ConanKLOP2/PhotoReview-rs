//! Port of `DecodeBox.cs` and `Decoding/{DecodeRequest, DecodedImageSize, ImageInfo, DecodedImage, IDecodedImage, PixelLease,
//! DecodeFailureSourceBytes}.cs` (baseline d92d6aae). Contract: `contracts/W4-01b.md`.
//!
//! Pure data and pure arithmetic: no I/O, no unsafe, no Windows types. The decoder traits are W4-01c, orientation maths is W4-02a.
//! C# exceptions of this unit (`ArgumentOutOfRangeException`, `ArgumentException` from the `DecodedImage` constructor) become
//! [`DecodedImageError`]; "null" arguments cannot occur in Rust.
//!
//! Stub: every body is `todo!()`; signatures and data types are final. Remove the `#![allow(...)]` line when implemented.

#![allow(unused_variables, dead_code, clippy::unused_self)]

use std::any::Any;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use pr_core::abstractions::SourceReadPriority;
use pr_core::model::enums::DecoderBackend;

use crate::exif_summary::ExifSummary; // W4-04a
use crate::pixels::PixelBuffer; // W4-16a: needs `width() -> i32`, `height() -> i32`

/// Whole source file bytes handed around instead of re-reading the file (C# `ReadOnlyMemory<byte>`; always the whole file).
pub type SourceBytes = Arc<[u8]>;

/// `DecodeBox`: bounding box in device pixels of the DISPLAYED (orientation-applied) image. `<= 0` on an axis = unconstrained;
/// both unconstrained = full size. `Default` is `UNBOUNDED` (C# `default`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DecodeBox {
    pub width: i32,
    pub height: i32,
}

impl DecodeBox {
    /// `DecodeBox.Unbounded` (0 x 0).
    pub const UNBOUNDED: Self = Self { width: 0, height: 0 };

    #[must_use]
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }

    /// `Width <= 0 && Height <= 0`.
    #[must_use]
    pub const fn is_unbounded(&self) -> bool {
        self.width <= 0 && self.height <= 0
    }

    /// `Fit`: largest size inside the box with the source aspect; never upscales; the constraining side is exactly the box side,
    /// the other is floored and at least 1. Source `<= 0` on either axis, or an unbounded box, returns the source unchanged.
    /// All products in `i64` (sizes up to `i32::MAX`).
    #[must_use]
    pub fn fit(&self, source_width: i32, source_height: i32) -> (i32, i32) {
        todo!()
    }

    /// `FitStored`: `transposed` (EXIF 5-8) fits against the rotated size and swaps the result back.
    #[must_use]
    pub fn fit_stored(&self, raw_width: i32, raw_height: i32, transposed: bool) -> (i32, i32) {
        todo!()
    }
}

/// `DecodeRequest`. Construct with [`DecodeRequest::new`] (width-only, the C# primary constructor with defaults) or
/// [`DecodeRequest::with_box`] (the `DecodeBox` constructor), then the `with_*` setters for the optional arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeRequest {
    /// Absolute path of the image file (also the label when `bytes` is given).
    pub path: PathBuf,
    /// Box width in displayed pixels; `<= 0` = unconstrained.
    pub target_width: i32,
    /// Apply EXIF orientation. C# default `true`.
    pub apply_orientation: bool,
    /// Pre-read whole-file bytes; decoders may decode from memory.
    pub bytes: Option<SourceBytes>,
    /// Box height in displayed pixels; `<= 0` = unconstrained (width-only).
    pub target_height: i32,
    /// Lane of the decoder's own source open (only when `bytes` is `None`). C# default `Viewer`.
    pub priority: SourceReadPriority,
    /// Orientation already known to the caller (skips the decoder's own read); `None` = read it.
    pub source_orientation: Option<i32>,
}

impl DecodeRequest {
    /// `new DecodeRequest(path, targetWidth)`: `apply_orientation = true`, `bytes = None`, `target_height = 0`,
    /// `priority = Viewer`, `source_orientation = None`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, target_width: i32) -> Self {
        todo!()
    }

    /// `new DecodeRequest(path, box, ...)`: width and height come from the box; the other defaults as in [`Self::new`].
    #[must_use]
    pub fn with_box(path: impl Into<PathBuf>, decode_box: DecodeBox) -> Self {
        todo!()
    }

    #[must_use]
    pub fn with_target_height(self, target_height: i32) -> Self {
        todo!()
    }

    #[must_use]
    pub fn with_apply_orientation(self, apply_orientation: bool) -> Self {
        todo!()
    }

    #[must_use]
    pub fn with_bytes(self, bytes: SourceBytes) -> Self {
        todo!()
    }

    #[must_use]
    pub fn with_priority(self, priority: SourceReadPriority) -> Self {
        todo!()
    }

    #[must_use]
    pub fn with_source_orientation(self, orientation: i32) -> Self {
        todo!()
    }

    /// `Box`: `DecodeBox { width: target_width, height: target_height }` (no clamping).
    #[must_use]
    pub fn decode_box(&self) -> DecodeBox {
        todo!()
    }

    /// `IsDownscaleRequested`: `target_width > 0 || target_height > 0`.
    #[must_use]
    pub fn is_downscale_requested(&self) -> bool {
        todo!()
    }
}

/// `ImageInfo`: stored pixel size plus EXIF orientation (default 1). No validation: `width`/`height` swap exactly for 5..=8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageInfo {
    pub pixel_width: i32,
    pub pixel_height: i32,
    pub orientation: i32,
}

impl ImageInfo {
    /// `new ImageInfo(w, h)` with orientation 1.
    #[must_use]
    pub const fn new(pixel_width: i32, pixel_height: i32) -> Self {
        Self {
            pixel_width,
            pixel_height,
            orientation: 1,
        }
    }

    /// `new ImageInfo(w, h, orientation)`.
    #[must_use]
    pub const fn with_orientation(pixel_width: i32, pixel_height: i32, orientation: i32) -> Self {
        Self {
            pixel_width,
            pixel_height,
            orientation,
        }
    }

    /// Displayed width: `pixel_height` when the orientation is 5..=8, else `pixel_width`.
    #[must_use]
    pub const fn width(&self) -> i32 {
        todo!()
    }

    /// Displayed height: `pixel_width` when the orientation is 5..=8, else `pixel_height`.
    #[must_use]
    pub const fn height(&self) -> i32 {
        todo!()
    }
}

/// What `IDecodedImage.PlatformImage` (`object`) holds: the native pixel buffer, or an opaque toolkit/test object.
#[derive(Clone)]
pub enum PlatformImage {
    Pixels(Arc<PixelBuffer>),
    Opaque(Arc<dyn Any + Send + Sync>),
}

impl PlatformImage {
    /// The pixel buffer when this is `Pixels`.
    #[must_use]
    pub fn as_pixels(&self) -> Option<&Arc<PixelBuffer>> {
        todo!()
    }
}

impl fmt::Debug for PlatformImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// `IDecodedImage` (+ the optional `ISourceReadMetrics` and `IRawPreviewInfo`, which C# discovers with `is`; here they are
/// default methods returning `None`). Provided methods carry the C# default interface implementations.
pub trait DecodedImageView: Send + Sync {
    fn pixel_width(&self) -> i32;
    fn pixel_height(&self) -> i32;
    fn downscaled(&self) -> bool;
    fn orientation(&self) -> i32;
    fn estimated_bytes(&self) -> i64;
    fn platform_image(&self) -> &PlatformImage;

    /// C# default: `DecoderBackend.Wpf` (NOT `WicDirect`; only [`DecodedImage`] defaults to `WicDirect`).
    fn actual_backend(&self) -> DecoderBackend {
        DecoderBackend::Wpf
    }

    /// Full-resolution width after orientation; C# default `PixelWidth`.
    fn original_width(&self) -> i32 {
        self.pixel_width()
    }

    /// Full-resolution height after orientation; C# default `PixelHeight`.
    fn original_height(&self) -> i32 {
        self.pixel_height()
    }

    /// EXIF fields read during the decode; C# default `null`.
    fn exif(&self) -> Option<&ExifSummary> {
        None
    }

    /// Next-best fallback after a better source failed; must never be cached. C# default `false`.
    fn is_degraded_fallback(&self) -> bool {
        false
    }

    /// `ISourceReadMetrics.SourceBytesRead`; `None` = the image does not report it.
    fn source_bytes_read(&self) -> Option<i64> {
        None
    }

    /// `IRawPreviewInfo`: (`EmbeddedPreviewWidth`, `EmbeddedPreviewHeight`); `None` = not a RAW preview image. Not persisted.
    fn embedded_preview_size(&self) -> Option<(i32, i32)> {
        None
    }
}

/// `DecodedImageSize.HasKnownOriginal`: `!downscaled || original_width > pixel_width || original_height > pixel_height`.
#[must_use]
pub fn has_known_original(image: &dyn DecodedImageView) -> bool {
    todo!()
}

/// Why a [`DecodedImage`] could not be built (C# constructor guards).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedImageError {
    /// `pixelWidth < 1` (`ArgumentOutOfRangeException`).
    PixelWidthTooSmall(i32),
    /// `pixelHeight < 1`.
    PixelHeightTooSmall(i32),
    /// `estimatedBytes < 0`.
    NegativeEstimatedBytes(i64),
    /// The platform image is a [`PixelBuffer`] whose size differs from the declared size (`ArgumentException`).
    PixelBufferSizeMismatch { declared: (i32, i32), buffer: (i32, i32) },
}

impl fmt::Display for DecodedImageError {
    /// Text mirrors the C# messages (the mismatch one is `Pixel size {w}x{h} does not match the {bw}x{bh} pixel buffer.`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl std::error::Error for DecodedImageError {}

/// The optional constructor arguments of `DecodedImage`, with the C# defaults.
#[derive(Debug, Clone)]
pub struct DecodedImageOptions {
    pub downscaled: bool,
    pub orientation: i32,
    pub actual_backend: DecoderBackend,
    /// `<= 0` = unknown, replaced by the pixel width.
    pub original_width: i32,
    /// `<= 0` = unknown, replaced by the pixel height.
    pub original_height: i32,
    pub exif: Option<ExifSummary>,
    pub is_degraded_fallback: bool,
}

impl Default for DecodedImageOptions {
    /// `downscaled: false, orientation: 1, actual_backend: WicDirect, original_*: 0, exif: None, is_degraded_fallback: false`.
    fn default() -> Self {
        todo!()
    }
}

/// `DecodedImage`: the framework-independent decoded image (C-02). Immutable after construction.
pub struct DecodedImage {
    platform_image: PlatformImage,
    pixel_width: i32,
    pixel_height: i32,
    estimated_bytes: i64,
    downscaled: bool,
    orientation: i32,
    actual_backend: DecoderBackend,
    original_width: i32,
    original_height: i32,
    exif: Option<ExifSummary>,
    is_degraded_fallback: bool,
}

impl DecodedImage {
    /// `new DecodedImage(platformImage, pixelWidth, pixelHeight, estimatedBytes, ...)`: guards in the C# order (width, height,
    /// bytes, then buffer-size match); `original_*` `<= 0` become the pixel size.
    ///
    /// # Errors
    /// See [`DecodedImageError`].
    pub fn new(
        platform_image: PlatformImage,
        pixel_width: i32,
        pixel_height: i32,
        estimated_bytes: i64,
        options: DecodedImageOptions,
    ) -> Result<Self, DecodedImageError> {
        todo!()
    }
}

impl fmt::Debug for DecodedImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl DecodedImageView for DecodedImage {
    fn pixel_width(&self) -> i32 {
        todo!()
    }
    fn pixel_height(&self) -> i32 {
        todo!()
    }
    fn downscaled(&self) -> bool {
        todo!()
    }
    fn orientation(&self) -> i32 {
        todo!()
    }
    fn estimated_bytes(&self) -> i64 {
        todo!()
    }
    fn platform_image(&self) -> &PlatformImage {
        todo!()
    }
    fn actual_backend(&self) -> DecoderBackend {
        todo!()
    }
    fn original_width(&self) -> i32 {
        todo!()
    }
    fn original_height(&self) -> i32 {
        todo!()
    }
    fn exif(&self) -> Option<&ExifSummary> {
        todo!()
    }
    fn is_degraded_fallback(&self) -> bool {
        todo!()
    }
}

/// `PixelLease`: pixels borrowed from, or owned by, the caller of `IPlatformImageCodec.ToPixels`. Dropping an owned lease
/// frees the buffer (C# `Dispose`); dropping a borrowed one leaves it to its owner; `empty()` is C# `default(PixelLease)`.
pub struct PixelLease<'a> {
    inner: PixelLeaseInner<'a>,
}

enum PixelLeaseInner<'a> {
    Empty,
    Borrowed(&'a PixelBuffer),
    Owned(PixelBuffer),
}

impl<'a> PixelLease<'a> {
    /// `default(PixelLease)`: no pixels, `is_owned() == false`.
    #[must_use]
    pub fn empty() -> Self {
        todo!()
    }

    /// `new PixelLease(pixels, owned: false)`.
    #[must_use]
    pub fn borrowed(pixels: &'a PixelBuffer) -> Self {
        todo!()
    }

    /// `new PixelLease(pixels, owned: true)`.
    #[must_use]
    pub fn owned_buffer(pixels: PixelBuffer) -> Self {
        todo!()
    }

    /// `Pixels`; `None` only for `empty()`.
    #[must_use]
    pub fn pixels(&self) -> Option<&PixelBuffer> {
        todo!()
    }

    /// `Owned`.
    #[must_use]
    pub fn is_owned(&self) -> bool {
        todo!()
    }
}

impl fmt::Debug for PixelLease<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// `DecodeFailureSourceBytes`: the whole source file a primary decoder already read, carried on its failure so the fallback decoder
/// does not read it again. C# stores it in `Exception.Data["PhotoReview.Imaging.SourceBytes"]`; here it is a field of the failure
/// value (W4-01c's decode error variants that can carry it embed one of these).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodeFailureSourceBytes {
    bytes: Option<SourceBytes>,
}

impl DecodeFailureSourceBytes {
    /// A value with nothing attached (`try_get` is `None`).
    #[must_use]
    pub const fn none() -> Self {
        Self { bytes: None }
    }

    /// `Attach`: replaces any earlier bytes.
    pub fn attach(&mut self, bytes: SourceBytes) {
        self.bytes = Some(bytes);
    }

    /// `TryGet`: the attached bytes, if any (an attached EMPTY buffer is still `Some`).
    #[must_use]
    pub fn try_get(&self) -> Option<&SourceBytes> {
        self.bytes.as_ref()
    }
}
