//! Port of `PhotoReview.Imaging/Decoding/Wic/WicDirectDecoder.cs` (C# baseline d92d6aae): the WIC decode pipeline
//! (decoder -> frame -> optional native DCT pre-scale -> high-quality scale -> optional ICC/EXIF-colour-space transform
//! to sRGB -> Bgr32/Pbgra32 format converter -> EXIF flip/rotate -> ONE `CopyPixels` into a native [`PixelBuffer`]),
//! plus `ReadInfo` (size + orientation without pixels). PARITY: I-02, I-03, I-12, I-18, I-20, INV-8, INV-12.
//! Contract: `contracts/drafts-w4/W4-11c/contract.md`.
//!
//! Target path: `crates/pr-platform-win/src/wic/decoder.rs` (lead adds `pub mod decoder;` to `wic/mod.rs`; the crate
//! needs `pr-imaging` and `windows` with `Win32_Foundation`, `Win32_Graphics_Imaging`, `Win32_System_Com`).
//!
//! Layering. Every decision C# makes between two COM calls is a pure `pub fn` here and is tested without WIC:
//! [`to_int_size`], [`output_byte_length`], [`ensure_output_fits`], [`copy_pixels_guarded`], [`allocate_and_fill`],
//! [`is_opaque_format`], [`native_reduction_is_useful`], [`select_source_color_context`], [`map_to_transform_options`],
//! and the exception mapping ([`decode_error_from_wic`], [`relabel_invalid_argument`], [`is_color_metadata_fault`],
//! [`color_chain_failure`]). The COM half uses the W4-11a wrappers (`super::interop`) and the W4-11d metadata reader
//! (`super::exif_reader`); this file holds no `unsafe` of its own unless a wrapper is missing (then: `// SAFETY:` per
//! block, AGENTS.md).
//!
//! Stub: every body is `todo!()`; public signatures and data types are final. Remove the `#![allow(...)]` line when done.

#![allow(unused_variables, dead_code, clippy::unused_self)] // D11: only while a body is `todo!()`.

use std::any::Any;
use std::fmt;
use std::io::{self, SeekFrom};
use std::sync::Arc;

use pr_core::abstractions::{ReadSeek, SourceReader};
use pr_imaging::decode_types::{DecodeRequest, DecodedImage, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::memory_headroom::MemoryInfoSource;
use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};
use windows::Win32::Graphics::Imaging::{
    GUID_WICPixelFormat2bppGray, GUID_WICPixelFormat4bppGray, GUID_WICPixelFormat8bppGray,
    GUID_WICPixelFormat16bppBGR555, GUID_WICPixelFormat16bppBGR565, GUID_WICPixelFormat16bppGray,
    GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat24bppRGB, GUID_WICPixelFormat32bppBGR,
    GUID_WICPixelFormat32bppCMYK, GUID_WICPixelFormat48bppBGR, GUID_WICPixelFormat48bppRGB,
    GUID_WICPixelFormat64bppCMYK, GUID_WICPixelFormatBlackWhite, IWICBitmapFrameDecode, IWICBitmapSource,
    IWICColorContext, IWICColorTransform, IWICFormatConverter, IWICImagingFactory, WICBitmapTransformOptions,
    WICColorContextType,
};
use windows::core::GUID;

use super::interop::{Com, ComStream, StreamBackend, WicError};

/// The `decoder` label of [`as_invalid_data`] for this decoder (C# default argument `"WicDirect"`).
pub const DECODER_LABEL: &str = "WicDirect";

/// Buffer size of the source open in `Decode` (C# `1024 * 1024`). Pinned by `SourceReaderSeamMutationTests`.
pub const DECODE_SOURCE_BUFFER_SIZE: usize = 1024 * 1024;

/// Buffer size of the source open in `ReadInfo` (C# `64 * 1024`, always on the `Viewer` lane).
pub const READ_INFO_SOURCE_BUFFER_SIZE: usize = 64 * 1024;

/// EXIF `ColorSpace` value for sRGB (C# `SrgbExifColorSpace`): needs no transform, and is the transform destination.
pub const SRGB_EXIF_COLOR_SPACE: u32 = 1;

/// English prefix of the ICC failure (C# `"WicDirect could not transform the embedded ICC profile to sRGB: " + osMessage`).
pub const ICC_TRANSFORM_FAILED_PREFIX: &str = "WicDirect could not transform the embedded ICC profile to sRGB: ";

/// C# `OpaquePixelFormats`: the 14 WIC formats that cannot carry alpha. Anything else (indexed, unknown, any alpha
/// format) is treated as possibly transparent and decodes to premultiplied `Pbgra32`.
pub const OPAQUE_PIXEL_FORMATS: [GUID; 14] = [
    GUID_WICPixelFormatBlackWhite,
    GUID_WICPixelFormat2bppGray,
    GUID_WICPixelFormat4bppGray,
    GUID_WICPixelFormat8bppGray,
    GUID_WICPixelFormat16bppGray,
    GUID_WICPixelFormat16bppBGR555,
    GUID_WICPixelFormat16bppBGR565,
    GUID_WICPixelFormat24bppBGR,
    GUID_WICPixelFormat24bppRGB,
    GUID_WICPixelFormat32bppBGR,
    GUID_WICPixelFormat48bppRGB,
    GUID_WICPixelFormat48bppBGR,
    GUID_WICPixelFormat32bppCMYK,
    GUID_WICPixelFormat64bppCMYK,
];

// ---------------------------------------------------------------------------------------------------------------
// The decoder
// ---------------------------------------------------------------------------------------------------------------

/// `WicDirectDecoder`: decodes through WIC COM directly. Stateless apart from its three seams, so one instance is shared
/// by every thread (`Send + Sync`, like every `ImageDecoder`); each call builds and releases its own WIC objects.
pub struct WicDirectDecoder {
    /// C-02 platform codec: receives the filled [`PixelBuffer`] BY VALUE (no default: a wrong codec is never picked
    /// up by accident).
    codec: Arc<dyn PlatformImageCodec>,
    /// Q-R29 C-2 seam: every source open goes through it (production: `PhysicalSourceReader`, INV-8 sharing flags).
    source_reader: Arc<dyn SourceReader>,
    /// C# `MemoryInfo` (default `MemoryHeadroom.ReadGcMemoryInfo`): consulted only by [`ensure_output_fits`] above the
    /// 128 MiB guard threshold.
    memory_info: Arc<dyn MemoryInfoSource>,
}

impl WicDirectDecoder {
    /// `new WicDirectDecoder(codec, sourceReader)` plus the C# `init`-only `MemoryInfo` property.
    /// `source_reader: None` = `PhysicalSourceReader` (C# `sourceReader ?? PhysicalSourceReader.Instance`).
    /// `memory_info` has no default here: the production reader (C# `ReadGcMemoryInfo`) is supplied by the composition
    /// root (owner: W4-07b open question 2). The C# `ArgumentNullException` for a null codec cannot occur.
    #[must_use]
    pub fn new(
        codec: Arc<dyn PlatformImageCodec>,
        source_reader: Option<Arc<dyn SourceReader>>,
        memory_info: Arc<dyn MemoryInfoSource>,
    ) -> Self {
        todo!("source_reader.unwrap_or_else(|| Arc::new(PhysicalSourceReader))")
    }

    /// C# `DecodeFromStream`: the whole pipeline over an opened source (contract semantics 3..12). The stream outlives
    /// every WIC object of the call (declare it first; WIC reads lazily up to `CopyPixels`). Errors are NOT relabelled
    /// here: [`ImageDecoder::decode`] applies [`relabel_invalid_argument`] to whatever comes out.
    fn decode_from_stream(&self, stream: &ComStream, request: &DecodeRequest) -> Result<DecodedImage, DecodeError> {
        todo!("contract semantics 3..12")
    }

    /// C# `ReadInfoCore`: factory, decoder (`WICDecodeMetadataCacheOnDemand`), frame 0, `GetSize` through
    /// [`to_int_size`], orientation through `exif_reader::read_orientation` (never fails); no colour or pixel work.
    fn read_info_core(stream: &ComStream) -> Result<ImageInfo, DecodeError> {
        todo!("contract semantics 2")
    }
}

impl fmt::Debug for WicDirectDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WicDirectDecoder")
            .field("codec", &self.codec.name())
            .finish_non_exhaustive()
    }
}

impl ImageDecoder for WicDirectDecoder {
    /// C# `Decode`. Order: without `bytes`, a null/whitespace `path` (Unicode `char::is_whitespace`, same set as .NET
    /// `char.IsWhiteSpace`) is `Err(InvalidArgument)` NOT relabelled; the source is `bytes` (read-only stream, the path
    /// is only a label) or `source_reader.open_source(path, request.priority, DECODE_SOURCE_BUFFER_SIZE)` (open errors
    /// propagate unchanged via `From<io::Error>`); then [`Self::decode_from_stream`] whose `InvalidArgument` errors become
    /// `InvalidData` ([`relabel_invalid_argument`]). The source is closed before returning, success or failure (INV-8).
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        todo!("contract semantics 1")
    }

    /// C# `ReadInfo`: blank `path` -> `Err(InvalidArgument)` not relabelled; open with `SourceReadPriority::Viewer` and
    /// [`READ_INFO_SOURCE_BUFFER_SIZE`]; [`Self::read_info_core`] with [`relabel_invalid_argument`] on its errors.
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        todo!("contract semantics 2")
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Source stream adapter (SourceReader -> W4-11a ComStream)
// ---------------------------------------------------------------------------------------------------------------

/// The opened source as a W4-11a [`StreamBackend`] (C# hands the `Stream` from `ISourceReader.OpenSource` to
/// `ManagedIStream`). Read-only: `write_all` / `set_len` fail with `ErrorKind::Unsupported` (C# `NotSupportedException`
/// from a read-only `FileStream`/`MemoryStream`). `len` = seek to end and back (the position is unchanged).
pub struct SourceStreamBackend {
    inner: Box<dyn ReadSeek>,
}

impl SourceStreamBackend {
    /// Wraps the stream returned by `SourceReader::open_source`.
    #[must_use]
    pub fn new(inner: Box<dyn ReadSeek>) -> Self {
        Self { inner }
    }
}

impl fmt::Debug for SourceStreamBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceStreamBackend").finish_non_exhaustive()
    }
}

impl StreamBackend for SourceStreamBackend {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        todo!("one inner.read per call")
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        todo!("Err(ErrorKind::Unsupported)")
    }
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        todo!("inner.seek")
    }
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        todo!("Err(ErrorKind::Unsupported)")
    }
    fn len(&mut self) -> io::Result<u64> {
        todo!("stream_position, seek End(0), seek back")
    }
    fn flush(&mut self) -> io::Result<()> {
        todo!("Ok(())")
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Pure decisions (all `pub` so the integration tests can reach them; C# `internal` + InternalsVisibleTo)
// ---------------------------------------------------------------------------------------------------------------

/// C# `ToIntSize(uint, uint)`: both sides `<= i32::MAX` -> `Ok((w, h))`; else `Err(InvalidData)` with message
/// `"WIC reported image dimensions beyond the supported range: {width}x{height}"` (localized key
/// `err.decoder.invalidDimensions`, contract OQ1). Never wraps to a negative size.
pub fn to_int_size(width: u32, height: u32) -> Result<(i32, i32), DecodeError> {
    todo!()
}

/// C# `AsInvalidData(ArgumentException ex, string decoder = "WicDirect")`: `InvalidData` with message
/// `"{decoder} rejected the image data: {detail}"` where `detail` is the inner exception's message (localized key
/// `err.decoder.decompressFailed` with `{detail}`, contract OQ1).
pub fn as_invalid_data(detail: &str, decoder: &str) -> DecodeError {
    todo!()
}

/// The `catch (ArgumentException ex) when (ex is not ArgumentNullException)` of `Decode` and the `catch
/// (ArgumentException)` of `ReadInfo`: `InvalidArgument { message }` -> [`as_invalid_data`]`(message, decoder)`; every
/// other error is returned unchanged. (Rust has no `ArgumentNullException`: the exclusion cannot trigger.)
pub fn relabel_invalid_argument(error: DecodeError, decoder: &str) -> DecodeError {
    todo!()
}

/// C# `OutputByteLength(int, int)`: `width as i64 * height as i64 * 4`, saturating at `i64::MAX` (never wraps).
/// Callers pass non-negative sides.
pub fn output_byte_length(width: i32, height: i32) -> i64 {
    todo!("pixels > i64::MAX / 4 => i64::MAX")
}

/// C# `EnsureOutputFits`. In this order:
/// 1. `0 <= buffer_length < GUARD_THRESHOLD_BYTES` -> `Ok(())` WITHOUT reading `memory_info`;
/// 2. `buffer_length > i32::MAX` or `< 0` -> `Err(`[`output_too_large`]`)` WITHOUT reading `memory_info`;
/// 3. `memory_info.read()` exactly once; `output_has_headroom(buffer_length, total, load)` -> `Ok(())`, else
///    `Err(`[`output_too_large`]`)`.
///
/// # Errors
/// `DecodeError::MemoryAdmission` (never fallbackable, INV-12).
pub fn ensure_output_fits(
    width: i32,
    height: i32,
    buffer_length: i64,
    memory_info: &dyn MemoryInfoSource,
) -> Result<(), DecodeError> {
    todo!()
}

/// C# `OutputTooLarge`: `MemoryAdmission` with message
/// `"WicDirect output dimensions are too large for the available memory: {w}x{h} ({bytes} bytes)."` (invariant digits,
/// no separators; localized key `err.decoder.outputTooLarge`, contract OQ1).
pub fn output_too_large(width: i32, height: i32, buffer_length: i64) -> DecodeError {
    todo!()
}

/// C# `CopyPixelsGuarded(Action, bool)`: runs `copy_pixels`; when it fails with a COM error (`DecodeError::Com`) AND
/// `color_transform_active`, the error becomes [`icc_transform_failed`]`(message)`. Any other error (or any error with
/// the transform inactive) is returned unchanged (IMG-04: only the lazily evaluated transform is an ICC failure).
pub fn copy_pixels_guarded<F>(copy_pixels: F, color_transform_active: bool) -> Result<(), DecodeError>
where
    F: FnOnce() -> Result<(), DecodeError>,
{
    todo!()
}

/// The ICC failure: `NotSupported` (fallbackable: the colour-managed fallback may succeed) with message
/// [`ICC_TRANSFORM_FAILED_PREFIX`]` + os_message` (localized key `err.decoder.iccTransformFailed` with `{detail}`).
pub fn icc_transform_failed(os_message: &str) -> DecodeError {
    todo!()
}

/// C# `AllocateAndFill`: `PixelBuffer::allocate(width, height, layout)` (its error through [`decode_error_from_pixels`]),
/// then `copy(&mut buffer)` under [`copy_pixels_guarded`]. On failure the buffer is dropped (freed) BEFORE the error is
/// returned, so a failed decode never holds native memory; on success the caller owns it.
pub fn allocate_and_fill<F>(
    width: i32,
    height: i32,
    layout: PixelLayout,
    copy: F,
    color_transform_active: bool,
) -> Result<PixelBuffer, DecodeError>
where
    F: FnOnce(&mut PixelBuffer) -> Result<(), DecodeError>,
{
    todo!()
}

/// C# `IsOpaqueFormat(Guid)`: membership in [`OPAQUE_PIXEL_FORMATS`].
pub fn is_opaque_format(pixel_format: &GUID) -> bool {
    todo!()
}

/// The decision half of C# `TryGetNativeReducedSize`: the codec's closest size is useful only when it does not
/// undershoot the target on either axis AND reduces at least one axis:
/// `cw >= tw && ch >= th && (cw < ow || ch < oh)`.
pub fn native_reduction_is_useful(target: (u32, u32), original: (u32, u32), closest: (u32, u32)) -> bool {
    todo!()
}

/// C# `SelectSourceColorContext`, generic over the context type so it is testable without COM. Walks `contexts` in
/// order, skipping `None`: `context_type(c)` for each; `Profile` -> return its index at once (later entries are not
/// queried); `ExifColorSpace` while no candidate yet -> `exif_color_space(c)`, and a value `!= SRGB_EXIF_COLOR_SPACE`
/// becomes the candidate (a later ExifColorSpace entry is then not asked for its value); any other type is ignored.
/// Result: the profile index, else the candidate, else `None` (also for an empty slice). An `Err` from either closure
/// propagates at once (C# has no catch here).
pub fn select_source_color_context<C, T, E>(
    contexts: &[Option<C>],
    context_type: T,
    exif_color_space: E,
) -> Result<Option<usize>, DecodeError>
where
    T: FnMut(&C) -> Result<WICColorContextType, DecodeError>,
    E: FnMut(&C) -> Result<u32, DecodeError>,
{
    todo!()
}

/// C# `MapToTransformOptions`: 2 FlipHorizontal, 3 Rotate180, 4 FlipVertical, 5 FlipHorizontal|Rotate270, 6 Rotate90,
/// 7 FlipHorizontal|Rotate90, 8 Rotate270, anything else Rotate0 (no rotator is created for Rotate0).
pub fn map_to_transform_options(orientation: i32) -> WICBitmapTransformOptions {
    todo!()
}

/// What C# throws for a failing WIC step, as a [`DecodeError`]: by `error.exception_kind()` (W4-11a, the
/// `Marshal.GetExceptionForHR(hr, -1)` type): `Com` -> `Com { hresult, message }`; `Argument`/`ArgumentOutOfRange` ->
/// `InvalidArgument`; `OutOfMemory` -> `OutOfMemory`; `InvalidCast` -> `InvalidCast`; `NotSupported`/
/// `PlatformNotSupported` -> `NotSupported`; `FileNotFound` -> `FileNotFound`; `DirectoryNotFound` ->
/// `DirectoryNotFound`; `Io`/`EndOfStream`/`DriveNotFound`/`PathTooLong`/`FileLoad` -> `Io`; `OperationCanceled` ->
/// `Canceled`; `Overflow` -> `Overflow`; `Format` -> `Format`; `BadImageFormat`/`DllNotFound`/`EntryPointNotFound` ->
/// same-named variant; `InvalidOperation` -> `InvalidOperation`; `ObjectDisposed` -> `Disposed`; everything else
/// (`NullReference`, `NotImplemented`, `UnauthorizedAccess`, `Arithmetic`, ...) -> `Other`. The resulting
/// `is_fallbackable` answer must equal C# `FallbackImageDecoder.IsFallbackable` for that .NET type (contract trap T3).
/// `message` is `error.to_string()` (C# shows the OS text; never compared by tests).
pub fn decode_error_from_wic(error: &WicError) -> DecodeError {
    todo!()
}

/// `PixelBuffer.Allocate` failures as the C# exception types: `WidthOutOfRange`/`HeightOutOfRange`/`UnknownLayout`/
/// `RowOutOfRange` (`ArgumentOutOfRangeException`, an `ArgumentException`) -> `InvalidArgument`; `OutOfMemory` ->
/// `OutOfMemory`; `Disposed` -> `Disposed`. Message = `error.to_string()`.
pub fn decode_error_from_pixels(error: &PixelBufferError) -> DecodeError {
    todo!()
}

/// The catch filter of C# `ReadColorContexts` (`ex is ArgumentException or COMException`): `InvalidArgument` or
/// `Com`. Such a fault means "treat the picture as untagged sRGB"; anything else propagates.
pub fn is_color_metadata_fault(error: &DecodeError) -> bool {
    todo!()
}

/// The outer catch of C# `ColorTransformChain.Build` (`COMException or InvalidCastException or ArgumentException`):
/// those three become [`icc_transform_failed`]`(message)`; anything else (e.g. `OutOfMemory`) is returned unchanged.
pub fn color_chain_failure(error: DecodeError) -> DecodeError {
    todo!()
}

// ---------------------------------------------------------------------------------------------------------------
// COM steps (private; built on super::interop helpers, which count and release every reference)
// ---------------------------------------------------------------------------------------------------------------

/// C# `TryGetNativeReducedSize`: `frame` QueryInterface `IWICBitmapSourceTransform` (failure -> `Ok(None)`);
/// `GetClosestSize(&mut w, &mut h)` starting from the target; a `Com`-kind failure -> `Ok(None)`, any other failure
/// propagates (C# catches `COMException` only); then [`native_reduction_is_useful`] -> `Ok(Some((w, h)))` or `Ok(None)`.
fn try_get_native_reduced_size(
    frame: &IWICBitmapFrameDecode,
    target: (u32, u32),
    original: (u32, u32),
) -> Result<Option<(u32, u32)>, DecodeError> {
    todo!()
}

/// C# `ReadColorContexts`: count query `GetColorContexts(0, NULL, &count)` ([`is_color_metadata_fault`] -> `Ok(None)`);
/// `count == 0` -> `Ok(None)`; create `count` contexts with `CreateColorContext`, fill them with
/// `GetColorContexts(count, ..)`; a metadata fault in creation or fill releases every reference made so far and gives
/// `Ok(None)`; any other error releases them and propagates. Every context is a counted W4-11a [`Com`] (made by
/// `interop::create_color_context`); the `GetColorContexts` fill call borrows them as a temporary
/// `Vec<Option<IWICColorContext>>` of clones (uncounted AddRefs dropped right after the call), so the lifetime-balance
/// tests see every reference (contract OQ5).
fn read_color_contexts(
    factory: &IWICImagingFactory,
    frame: &IWICBitmapFrameDecode,
) -> Result<Option<Vec<Option<Com<IWICColorContext>>>>, DecodeError> {
    todo!()
}

/// C# `ColorTransformChain`: owns the COM objects of the source-profile -> sRGB stage. Dropping it releases them
/// (C# `Release()` in the `finally`). Fields are counted W4-11a [`Com`] values (contract OQ5).
#[derive(Default)]
struct ColorTransformChain {
    destination: Option<Com<IWICColorContext>>,
    transform: Option<Com<IWICColorTransform>>,
    normalizer: Option<Com<IWICFormatConverter>>,
}

impl ColorTransformChain {
    /// C# `IsActive`: a transform object exists.
    fn is_active(&self) -> bool {
        self.transform.is_some()
    }

    /// C# `Build`: transform format = 32bppBGR (opaque) or 32bppBGRA (straight alpha); destination = new context
    /// `InitializeFromExifColorSpace(1)`; first `CreateColorTransformer` + `Initialize(source, profile, destination,
    /// format)`; a `Com`-kind failure of that `Initialize` ONLY releases the transform (`None`) and retries through a
    /// format converter to the transform format (`None` palette, dither None, alpha 0.0, palette Custom) and a new
    /// transformer. Every failure of the whole build goes through [`color_chain_failure`].
    fn build(
        &mut self,
        factory: &IWICImagingFactory,
        source: &IWICBitmapSource,
        source_profile: &IWICColorContext,
        opaque: bool,
    ) -> Result<IWICBitmapSource, DecodeError> {
        todo!()
    }
}

impl fmt::Debug for ColorTransformChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ColorTransformChain")
            .field("active", &self.is_active())
            .finish_non_exhaustive()
    }
}
