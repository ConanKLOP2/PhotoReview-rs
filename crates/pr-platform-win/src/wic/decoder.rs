//! Port of `PhotoReview.Imaging/Decoding/Wic/WicDirectDecoder.cs` (C# baseline d92d6aae): the WIC decode pipeline
//! (decoder -> frame -> optional native DCT pre-scale -> high-quality scale -> optional ICC/EXIF-colour-space transform
//! to sRGB -> Bgr32/Pbgra32 format converter -> EXIF flip/rotate -> ONE `CopyPixels` into a native [`PixelBuffer`]),
//! plus `ReadInfo` (size + orientation without pixels). PARITY: I-02, I-03, I-12, I-18, I-20, INV-8, INV-12.
//! Contract: `contracts/W4-11c.md`.
//!
//! Layering. Every decision C# makes between two COM calls is a pure `pub fn` here and is tested without WIC:
//! [`to_int_size`], [`output_byte_length`], [`ensure_output_fits`], [`copy_pixels_guarded`], [`allocate_and_fill`],
//! [`is_opaque_format`], [`native_reduction_is_useful`], [`select_source_color_context`], [`map_to_transform_options`],
//! and the exception mapping ([`decode_error_from_wic`], [`relabel_invalid_argument`], [`is_color_metadata_fault`],
//! [`color_chain_failure`]). The COM half uses the W4-11a wrappers (`super::interop`: every created object is a
//! counted [`Com`]) and the W4-11d metadata reader (`super::exif_reader`). The `windows` crate exposes every WIC
//! method as `unsafe fn`; each call sits in its own one-line helper below with one `// SAFETY:` comment.
//!
//! Lifetimes (T6, INV-8). The [`ComStream`] is owned by [`ImageDecoder::decode`] / [`ImageDecoder::read_info`] and
//! outlives every WIC object of the call: all of them are locals of `decode_from_stream` / `read_info_core`, dropped
//! (released) before those return; the stream (and with it the file handle) is dropped right after.

use std::any::Any;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use pr_core::abstractions::{PhysicalSourceReader, ReadSeek, SourceReadPriority, SourceReader};
use pr_core::catalog::explorer_types::Guid as DotnetGuid;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::{DecodeRequest, DecodedImage, DecodedImageOptions, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, PlatformImageCodec};
use pr_imaging::memory_headroom::{GUARD_THRESHOLD_BYTES, MemoryInfoSource, output_has_headroom};
use pr_imaging::orientation::is_transposed;
use pr_imaging::pixels::{PixelBuffer, PixelBufferError, PixelLayout};
use windows::Win32::Graphics::Imaging::{
    GUID_WICPixelFormat2bppGray, GUID_WICPixelFormat4bppGray, GUID_WICPixelFormat8bppGray,
    GUID_WICPixelFormat16bppBGR555, GUID_WICPixelFormat16bppBGR565, GUID_WICPixelFormat16bppGray,
    GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat24bppRGB, GUID_WICPixelFormat32bppBGR,
    GUID_WICPixelFormat32bppCMYK, GUID_WICPixelFormat48bppBGR, GUID_WICPixelFormat48bppRGB,
    GUID_WICPixelFormat64bppCMYK, GUID_WICPixelFormatBlackWhite, IWICBitmapDecoder, IWICBitmapFlipRotator,
    IWICBitmapFrameDecode, IWICBitmapScaler, IWICBitmapSource, IWICBitmapSourceTransform, IWICColorContext,
    IWICColorTransform, IWICFormatConverter, IWICImagingFactory, IWICMetadataQueryReader, IWICPalette,
    WICBitmapDitherTypeNone, WICBitmapInterpolationMode, WICBitmapInterpolationModeFant,
    WICBitmapInterpolationModeHighQualityCubic, WICBitmapPaletteTypeCustom, WICBitmapTransformFlipHorizontal,
    WICBitmapTransformFlipVertical, WICBitmapTransformOptions, WICBitmapTransformRotate0, WICBitmapTransformRotate90,
    WICBitmapTransformRotate180, WICBitmapTransformRotate270, WICColorContextExifColorSpace, WICColorContextProfile,
    WICColorContextType, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::{GUID, Interface, PCWSTR};

use super::exif_reader::{
    DecoderContainer, FrameMetadata, HResult, MetadataQueryReader, PropVariant, exif_ifd_root_of, read_frame_metadata,
    read_orientation,
};
use super::interop::{
    Com, ComStream, DotnetExceptionKind, StreamBackend, StreamFault, WicError, create_bitmap_flip_rotator,
    create_bitmap_scaler, create_color_context, create_color_transformer, create_decoder_from_stream, create_factory,
    create_format_converter, get_frame, get_metadata_query_reader, guids,
};

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

/// .NET `ArgumentException.ThrowIfNullOrWhiteSpace` message (the parameter name is appended by the caller).
const BLANK_ARGUMENT_MESSAGE: &str = "The value cannot be an empty string or composed entirely of whitespace.";

/// .NET `OverflowException` message of a failing `checked` operation.
const CHECKED_OVERFLOW_MESSAGE: &str = "Arithmetic operation resulted in an overflow.";

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
        Self {
            codec,
            source_reader: source_reader.unwrap_or_else(|| Arc::new(PhysicalSourceReader)),
            memory_info,
        }
    }

    /// C# `DecodeFromStream`: the whole pipeline over an opened source (contract semantics 3..12). The stream outlives
    /// every WIC object of the call (they are locals of this function; WIC reads lazily up to `CopyPixels`). Errors are
    /// NOT relabelled here: [`ImageDecoder::decode`] applies [`relabel_invalid_argument`] to whatever comes out.
    fn decode_from_stream(&self, stream: &ComStream, request: &DecodeRequest) -> Result<DecodedImage, DecodeError> {
        // Semantics 3: factory (A6: puts this thread in the MTA), decoder, frame 0, stored size.
        let factory = create_factory().map_err(|e| decode_error_from_wic(&e))?;
        let decoder = create_decoder_from_stream(&factory, &stream.istream(), WICDecodeMetadataCacheOnDemand)
            .map_err(|e| decode_error_from_wic(&e))?;
        let frame = get_frame(&decoder, 0).map_err(|e| decode_error_from_wic(&e))?;
        let (orig_w, orig_h) = source_size(&frame)?;
        // Every later computation is int-based: refuse a size the casts below would wrap.
        let (orig_w_i32, orig_h_i32) = to_int_size(orig_w, orig_h)?;

        // Semantics 4: orientation and EXIF from ONE metadata read over the metadata this decode parses anyway.
        let root = exif_ifd_root_of(&DecoderView(&decoder));
        let frame_view = FrameView(&frame);
        let (orientation, exif) = match request.source_orientation {
            // T10: the caller's value AS GIVEN (callers normalise); only the EXIF summary comes from the frame.
            Some(given) if request.apply_orientation => (given, read_frame_metadata(&frame_view, false, root).exif),
            _ => {
                let read = read_frame_metadata(&frame_view, request.apply_orientation, root);
                (read.orientation, read.exif)
            }
        };
        let transposed = request.apply_orientation && is_transposed(orientation);

        let mut current: IWICBitmapSource = (*frame).clone().into();
        let mut downscaled = false;
        let mut pre_scaler: Option<Com<IWICBitmapScaler>> = None;
        let mut scaler: Option<Com<IWICBitmapScaler>> = None;

        // Semantics 5 + 6: the box applies to the DISPLAYED image; fit_stored maps it back onto the stored grid, so the
        // pipeline scales first and rotates last.
        if request.is_downscale_requested() {
            let (fit_w, fit_h) = request.decode_box().fit_stored(orig_w_i32, orig_h_i32, transposed);
            // C# `(uint)fitW`: a fitted size is never negative.
            let (target_w, target_h) = (fit_w as u32, fit_h as u32);
            if target_w < orig_w || target_h < orig_h {
                // Stage 1: let the codec reduce natively (JPEG DCT 1/2, 1/4, 1/8) to the smallest size still >= the
                // target; a scaler directly on the frame at exactly that size is served by IWICBitmapSourceTransform.
                if let Some((native_w, native_h)) =
                    try_get_native_reduced_size(&frame, (target_w, target_h), (orig_w, orig_h))?
                {
                    let pre = create_bitmap_scaler(&factory).map_err(|e| decode_error_from_wic(&e))?;
                    scaler_initialize(&pre, &current, native_w, native_h, WICBitmapInterpolationModeFant)?;
                    current = (*pre).clone().into();
                    pre_scaler = Some(pre);
                }

                // Stage 2: high-quality resample of the (already reduced) image to the target; only a COMException
                // (T1: the `Com` kind) retries the SAME scaler with Fant.
                let stage2 = create_bitmap_scaler(&factory).map_err(|e| decode_error_from_wic(&e))?;
                match scaler_initialize(
                    &stage2,
                    &current,
                    target_w,
                    target_h,
                    WICBitmapInterpolationModeHighQualityCubic,
                ) {
                    Ok(()) => {}
                    Err(DecodeError::Com { .. }) => {
                        scaler_initialize(&stage2, &current, target_w, target_h, WICBitmapInterpolationModeFant)?;
                    }
                    Err(other) => return Err(other),
                }
                current = (*stage2).clone().into();
                scaler = Some(stage2);
                downscaled = true;
            }
        }

        // Semantics 7: Bgr32 for opaque sources (JPEG), premultiplied Pbgra32 for anything that may carry alpha.
        let source_format = pixel_format(&current)?;
        let opaque = is_opaque_format(&source_format);

        // Semantics 8: embedded ICC (or non-sRGB EXIF colour space) to sRGB after downscaling, read from the FRAME.
        let source_contexts = read_color_contexts(&factory, &frame)?;
        let mut color_chain = ColorTransformChain::default();
        if let Some(contexts) = &source_contexts {
            let chosen = select_source_color_context(contexts, |c| context_type(c), |c| exif_color_space(c))?;
            if let Some(Some(profile)) = chosen.and_then(|index| contexts.get(index)) {
                current = color_chain.build(&factory, &current, profile, opaque)?;
            }
        }

        // Semantics 9: the format converter (its failures keep their own mapping: IMG-04, not an ICC failure).
        let converter = create_format_converter(&factory).map_err(|e| decode_error_from_wic(&e))?;
        let output_guid = if opaque {
            guids::PIXEL_FORMAT_32BPP_BGR
        } else {
            guids::PIXEL_FORMAT_32BPP_PBGRA
        };
        converter_initialize(&converter, &current, &output_guid)?;
        current = (*converter).clone().into();

        // Semantics 10: EXIF orientation through a flip-rotator; 9, 0, negatives (T10) create none.
        let mut rotator: Option<Com<IWICBitmapFlipRotator>> = None;
        if request.apply_orientation && orientation > 1 {
            let options = map_to_transform_options(orientation);
            if options != WICBitmapTransformRotate0 {
                let flip = create_bitmap_flip_rotator(&factory).map_err(|e| decode_error_from_wic(&e))?;
                flip_rotator_initialize(&flip, &current, options)?;
                current = (*flip).clone().into();
                rotator = Some(flip);
            }
        }

        // Semantics 11 (R16): admission first, on a saturating byte count, then C#'s checked int maths.
        let (final_w, final_h) = source_size(&current)?;
        let (width, height) = to_int_size(final_w, final_h)?;
        ensure_output_fits(
            width,
            height,
            output_byte_length(width, height),
            self.memory_info.as_ref(),
        )?;
        let overflow = || DecodeError::Overflow {
            message: CHECKED_OVERFLOW_MESSAGE.to_owned(),
        };
        let stride = width.checked_mul(4).ok_or_else(overflow)?;
        let buffer_size = stride.checked_mul(height).ok_or_else(overflow)?;
        let layout = if opaque {
            PixelLayout::Bgr32
        } else {
            PixelLayout::Pbgra32
        };

        // WP-03: WIC writes straight into the native PixelBuffer (one copy); the codec then owns it.
        let source = &current;
        let pixels = allocate_and_fill(
            width,
            height,
            layout,
            |buffer| {
                let bytes = buffer.bytes_mut().map_err(|e| decode_error_from_pixels(&e))?;
                copy_pixels(source, stride.unsigned_abs(), bytes)
            },
            color_chain.is_active(),
        )?;

        // Semantics 12: ownership of the buffer moves to the codec (on error it is already dropped).
        let platform_image = self.codec.image_from_pixels(pixels)?;
        let (original_width, original_height) = if transposed {
            (orig_h_i32, orig_w_i32)
        } else {
            (orig_w_i32, orig_h_i32)
        };
        let image = DecodedImage::new(
            platform_image,
            width,
            height,
            i64::from(buffer_size),
            DecodedImageOptions {
                downscaled,
                orientation,
                actual_backend: DecoderBackend::WicDirect,
                original_width,
                original_height,
                exif,
                is_degraded_fallback: false,
            },
        )
        .map_err(|e| DecodeError::InvalidArgument { message: e.to_string() })?;

        // C# `finally` order: rotator, converter, colour chain, contexts, scaler, pre-scaler, frame, decoder, factory.
        drop(current);
        drop(rotator);
        drop(converter);
        drop(color_chain);
        drop(source_contexts);
        drop(scaler);
        drop(pre_scaler);
        drop(frame);
        drop(decoder);
        drop(factory);
        Ok(image)
    }

    /// C# `ReadInfoCore`: factory, decoder (`WICDecodeMetadataCacheOnDemand`), frame 0, `GetSize` through
    /// [`to_int_size`], orientation through `exif_reader::read_orientation` (never fails); no colour or pixel work.
    fn read_info_core(stream: &ComStream) -> Result<ImageInfo, DecodeError> {
        let factory = create_factory().map_err(|e| decode_error_from_wic(&e))?;
        let decoder = create_decoder_from_stream(&factory, &stream.istream(), WICDecodeMetadataCacheOnDemand)
            .map_err(|e| decode_error_from_wic(&e))?;
        let frame = get_frame(&decoder, 0).map_err(|e| decode_error_from_wic(&e))?;
        let (stored_w, stored_h) = source_size(&frame)?;
        let (width, height) = to_int_size(stored_w, stored_h)?;
        let orientation = read_orientation(&FrameView(&frame));
        Ok(ImageInfo::with_orientation(width, height, orientation))
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
        let stream = match &request.bytes {
            // No copy and no file-system access: the path is only a label.
            Some(bytes) => ComStream::from_bytes(Arc::clone(bytes)),
            None => {
                if is_blank_path(&request.path) {
                    return Err(blank_argument("request.Path"));
                }
                let source =
                    self.source_reader
                        .open_source(&request.path, request.priority, DECODE_SOURCE_BUFFER_SIZE)?;
                ComStream::new(Box::new(SourceStreamBackend::with_buffer_size(
                    source,
                    DECODE_SOURCE_BUFFER_SIZE,
                )))
            }
        };
        let result = self
            .decode_from_stream(&stream, request)
            .map_err(|e| relabel_invalid_argument(e, DECODER_LABEL));
        // INV-8: every WIC object is already released; dropping the stream closes the file handle now.
        drop(stream);
        result.map(|image| Arc::new(image) as Arc<dyn DecodedImageView>)
    }

    /// C# `ReadInfo`: blank `path` -> `Err(InvalidArgument)` not relabelled; open with `SourceReadPriority::Viewer` and
    /// [`READ_INFO_SOURCE_BUFFER_SIZE`]; [`Self::read_info_core`] with [`relabel_invalid_argument`] on its errors.
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        if path.chars().all(char::is_whitespace) {
            return Err(blank_argument("path"));
        }
        let source = self.source_reader.open_source(
            Path::new(path),
            SourceReadPriority::Viewer,
            READ_INFO_SOURCE_BUFFER_SIZE,
        )?;
        let stream = ComStream::new(Box::new(SourceStreamBackend::with_buffer_size(
            source,
            READ_INFO_SOURCE_BUFFER_SIZE,
        )));
        let result = Self::read_info_core(&stream).map_err(|e| relabel_invalid_argument(e, DECODER_LABEL));
        drop(stream);
        result
    }
}

/// .NET `string.IsNullOrWhiteSpace` over a `PathBuf` (T9: Unicode White_Space, empty included). A path that is not
/// valid Unicode text cannot be all whitespace.
fn is_blank_path(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.chars().all(char::is_whitespace))
}

/// The `ArgumentException` of `ArgumentException.ThrowIfNullOrWhiteSpace(argument)`.
fn blank_argument(parameter: &str) -> DecodeError {
    DecodeError::InvalidArgument {
        message: format!("{BLANK_ARGUMENT_MESSAGE} (Parameter '{parameter}')"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Source stream adapter (SourceReader -> W4-11a ComStream)
// ---------------------------------------------------------------------------------------------------------------

/// The opened source as a W4-11a [`StreamBackend`] (C# hands the `Stream` from `ISourceReader.OpenSource` to
/// `ManagedIStream`). Read-only: `write_all` / `set_len` fail with `ErrorKind::Unsupported` (C# `NotSupportedException`
/// from a read-only `FileStream`/`MemoryStream`).
///
/// Disk reads (AGENTS priority 1: read each file once). Like the .NET `FileStream` C# reads through, the backend keeps
/// its own read window of `buffer_size` bytes and serves every seek and read that stays inside it without touching
/// the source; `len` asks the source for its end without dropping the window. (A `BufReader` drops its buffer on
/// every `seek`, and WIC seeks constantly, so forwarding seeks made a decode read the file 4x to 13x.) The logical
/// position lives here; the source is only moved when a read has to fetch outside the window. At most one source
/// read per call (C# calls `Stream.Read` once per `IStream::Read`, and `FileStream` does at most one more fetch).
pub struct SourceStreamBackend {
    inner: Box<dyn ReadSeek>,
    /// Window capacity (C# `FileStream` `bufferSize`).
    buffer_size: usize,
    /// The bytes at `[window_start, window_start + window.len())` of the source.
    window: Vec<u8>,
    window_start: u64,
    /// Logical stream position; `None` until first needed (then the source's own position).
    position: Option<u64>,
    /// Where the source currently is; `None` when unknown (after a failed source call).
    inner_position: Option<u64>,
}

impl SourceStreamBackend {
    /// Wraps the stream returned by `SourceReader::open_source` with the decode window
    /// ([`DECODE_SOURCE_BUFFER_SIZE`]).
    #[must_use]
    pub fn new(inner: Box<dyn ReadSeek>) -> Self {
        Self::with_buffer_size(inner, DECODE_SOURCE_BUFFER_SIZE)
    }

    /// Wraps `inner` with a read window of `buffer_size` bytes (the size the source was opened with; 0 counts as 1).
    #[must_use]
    pub fn with_buffer_size(inner: Box<dyn ReadSeek>, buffer_size: usize) -> Self {
        Self {
            inner,
            buffer_size: buffer_size.max(1),
            window: Vec::new(),
            window_start: 0,
            position: None,
            inner_position: None,
        }
    }

    /// The logical position (the source's own position the first time).
    fn position(&mut self) -> io::Result<u64> {
        if let Some(position) = self.position {
            return Ok(position);
        }
        let position = self.inner.stream_position()?;
        self.position = Some(position);
        self.inner_position = Some(position);
        Ok(position)
    }

    /// The window bytes from `position` on (empty when `position` is outside the window).
    fn buffered_from(&self, position: u64) -> &[u8] {
        position
            .checked_sub(self.window_start)
            .and_then(|offset| usize::try_from(offset).ok())
            .and_then(|offset| self.window.get(offset..))
            .unwrap_or(&[])
    }

    /// Moves the source to `target` unless it is already there.
    fn move_inner_to(&mut self, target: u64) -> io::Result<()> {
        if self.inner_position != Some(target) {
            self.inner_position = None;
            self.inner.seek(SeekFrom::Start(target))?;
            self.inner_position = Some(target);
        }
        Ok(())
    }

    /// One source read at `position` into `out` (the source is moved there first if needed).
    fn read_inner_at(&mut self, position: u64, out: &mut [u8]) -> io::Result<usize> {
        self.move_inner_to(position)?;
        match self.inner.read(out) {
            Ok(read) => {
                self.inner_position = Some(position + read as u64);
                Ok(read)
            }
            Err(error) => {
                self.inner_position = None;
                Err(error)
            }
        }
    }

    /// Fetches the source from `position` on: straight into `out` when it is at least a window long (.NET
    /// `FileStream` does the same) or the source has nothing left, else one window refill (sized to what is left of the source, so a source reader
    /// that buffers itself never fetches more than this window holds) and a copy out of it.
    fn fetch(&mut self, position: u64, out: &mut [u8]) -> io::Result<usize> {
        if out.len() >= self.buffer_size {
            return self.read_inner_at(position, out);
        }
        let remaining = self.len()?.saturating_sub(position);
        if remaining == 0 {
            // At (or past) the end: ask the source directly (normally 0 bytes) and keep the window for later seeks.
            return self.read_inner_at(position, out);
        }
        let chunk = usize::try_from(remaining).map_or(self.buffer_size, |r| r.min(self.buffer_size));
        let mut window = std::mem::take(&mut self.window);
        window.clear();
        window.resize(chunk, 0);
        let read = self.read_inner_at(position, &mut window);
        let read = match read {
            Ok(read) => read,
            Err(error) => {
                window.clear();
                self.window = window;
                return Err(error);
            }
        };
        window.truncate(read);
        self.window = window;
        self.window_start = position;
        let copied = read.min(out.len());
        out[..copied].copy_from_slice(&self.window[..copied]);
        Ok(copied)
    }
}

impl fmt::Debug for SourceStreamBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceStreamBackend").finish_non_exhaustive()
    }
}

/// .NET `NotSupportedException` text of a write to a read-only stream.
const READ_ONLY_STREAM_MESSAGE: &str = "Stream does not support writing.";

impl StreamBackend for SourceStreamBackend {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let position = self.position()?;
        let buffered = self.buffered_from(position);
        let copied = buffered.len().min(buf.len());
        buf[..copied].copy_from_slice(&buffered[..copied]);
        self.position = Some(position + copied as u64);
        if copied == buf.len() {
            return Ok(copied);
        }
        // One more fetch from the source for the rest; a failure after a partial copy reports the copied bytes
        // (the next call meets the error again).
        match self.fetch(position + copied as u64, &mut buf[copied..]) {
            Ok(fetched) => {
                self.position = Some(position + (copied + fetched) as u64);
                Ok(copied + fetched)
            }
            Err(_) if copied > 0 => Ok(copied),
            Err(error) => Err(error),
        }
    }
    fn write_all(&mut self, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, READ_ONLY_STREAM_MESSAGE))
    }
    /// Moves the logical position only (no source I/O, the window is kept). A target before 0 (or past `i64::MAX`)
    /// fails with `InvalidInput` and leaves the position unchanged (the W4-11a seek-before-begin fault).
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(delta) => i128::from(self.position()?) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(self.len()?) + i128::from(delta),
        };
        match u64::try_from(target) {
            Ok(target) if i64::try_from(target).is_ok() => {
                self.position = Some(target);
                Ok(target)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            )),
        }
    }
    fn set_len(&mut self, _len: u64) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, READ_ONLY_STREAM_MESSAGE))
    }
    /// The source's current end (asked each time, like .NET `FileStream.Length`); the logical position and the window
    /// are unchanged.
    fn len(&mut self) -> io::Result<u64> {
        // Pin the logical position before the source moves (the first call reads it from the source).
        self.position()?;
        self.inner_position = None;
        let end = self.inner.seek(SeekFrom::End(0))?;
        self.inner_position = Some(end);
        Ok(end)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// The C# source is a .NET `FileStream` (`PhysicalSourceReader`): a seek before 0 (or past `i64::MAX`, which
    /// wraps negative in .NET) throws `IOException` with HResult `HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER)`
    /// (`0x80070057`, checked against .NET 10), not the `MemoryStream` `COR_E_IO` default. Repair round 3.
    fn seek_before_begin_fault(&self) -> StreamFault {
        StreamFault::Io(io::Error::from_raw_os_error(ERROR_INVALID_PARAMETER))
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

/// `ERROR_INVALID_PARAMETER`: what .NET `FileStream.Seek` reports for a target before 0.
const ERROR_INVALID_PARAMETER: i32 = 87;

// ---------------------------------------------------------------------------------------------------------------
// Pure decisions (all `pub` so the integration tests can reach them; C# `internal` + InternalsVisibleTo)
// ---------------------------------------------------------------------------------------------------------------

/// C# `ToIntSize(uint, uint)`: both sides `<= i32::MAX` -> `Ok((w, h))`; else `Err(InvalidData)` with message
/// `"WIC reported image dimensions beyond the supported range: {width}x{height}"` (localized key
/// `err.decoder.invalidDimensions`, contract OQ1). Never wraps to a negative size.
pub fn to_int_size(width: u32, height: u32) -> Result<(i32, i32), DecodeError> {
    match (i32::try_from(width), i32::try_from(height)) {
        (Ok(w), Ok(h)) => Ok((w, h)),
        _ => Err(DecodeError::InvalidData {
            message: format!("WIC reported image dimensions beyond the supported range: {width}x{height}"),
        }),
    }
}

/// C# `AsInvalidData(ArgumentException ex, string decoder = "WicDirect")`: `InvalidData` with message
/// `"{decoder} rejected the image data: {detail}"` where `detail` is the inner exception's message (localized key
/// `err.decoder.decompressFailed` with `{detail}`, contract OQ1).
pub fn as_invalid_data(detail: &str, decoder: &str) -> DecodeError {
    DecodeError::InvalidData {
        message: format!("{decoder} rejected the image data: {detail}"),
    }
}

/// The `catch (ArgumentException ex) when (ex is not ArgumentNullException)` of `Decode` and the `catch
/// (ArgumentException)` of `ReadInfo`: `InvalidArgument { message }` -> [`as_invalid_data`]`(message, decoder)`; every
/// other error is returned unchanged. (Rust has no `ArgumentNullException`: the exclusion cannot trigger.)
/// The test is on the TYPE ([`DecodeError::kind`]); like the C# `new InvalidDataException(.., ex)`, the relabelled
/// error is a new one (attached source bytes are not carried over).
pub fn relabel_invalid_argument(error: DecodeError, decoder: &str) -> DecodeError {
    match error.kind() {
        DecodeError::InvalidArgument { message } => as_invalid_data(message, decoder),
        _ => error,
    }
}

/// C# `OutputByteLength(int, int)`: `width as i64 * height as i64 * 4`, saturating at `i64::MAX` (never wraps).
/// Callers pass non-negative sides.
pub fn output_byte_length(width: i32, height: i32) -> i64 {
    // Two ints cannot overflow an i64.
    let pixels = i64::from(width) * i64::from(height);
    if pixels > i64::MAX / 4 { i64::MAX } else { pixels * 4 }
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
    if (0..GUARD_THRESHOLD_BYTES).contains(&buffer_length) {
        return Ok(());
    }
    if buffer_length > i64::from(i32::MAX) || buffer_length < 0 {
        return Err(output_too_large(width, height, buffer_length));
    }
    let info = memory_info.read();
    if output_has_headroom(buffer_length, info.total_available, info.load) {
        Ok(())
    } else {
        Err(output_too_large(width, height, buffer_length))
    }
}

/// C# `OutputTooLarge`: `MemoryAdmission` with message
/// `"WicDirect output dimensions are too large for the available memory: {w}x{h} ({bytes} bytes)."` (invariant digits,
/// no separators; localized key `err.decoder.outputTooLarge`, contract OQ1).
pub fn output_too_large(width: i32, height: i32, buffer_length: i64) -> DecodeError {
    DecodeError::memory_admission_with(format!(
        "WicDirect output dimensions are too large for the available memory: {width}x{height} ({buffer_length} bytes)."
    ))
}

/// C# `CopyPixelsGuarded(Action, bool)`: runs `copy_pixels`; when it fails with a COM error (`DecodeError::Com`) AND
/// `color_transform_active`, the error becomes [`icc_transform_failed`]`(message)`. Any other error (or any error with
/// the transform inactive) is returned unchanged (IMG-04: only the lazily evaluated transform is an ICC failure).
pub fn copy_pixels_guarded<F>(copy_pixels: F, color_transform_active: bool) -> Result<(), DecodeError>
where
    F: FnOnce() -> Result<(), DecodeError>,
{
    match copy_pixels() {
        Err(error) if color_transform_active && matches!(error.kind(), DecodeError::Com { .. }) => {
            let hresult = match error.kind() {
                DecodeError::Com { hresult, .. } => *hresult,
                _ => unreachable!("guarded COM error"),
            };
            Err(icc_transform_failed(&error.message(), Some(hresult)))
        }
        other => other,
    }
}

/// The ICC failure: `NotSupported` (fallbackable: the colour-managed fallback may succeed) with message
/// [`ICC_TRANSFORM_FAILED_PREFIX`]` + os_message` (localized key `err.decoder.iccTransformFailed` with `{detail}`;
/// the sentence waits for an injected text seam, contract OQ1 / A1 / D41).
pub fn icc_transform_failed(os_message: &str, inner_hresult: Option<i32>) -> DecodeError {
    match inner_hresult {
        Some(hresult) => {
            DecodeError::not_supported_with_inner_hresult(format!("{ICC_TRANSFORM_FAILED_PREFIX}{os_message}"), hresult)
        }
        None => DecodeError::NotSupported {
            message: format!("{ICC_TRANSFORM_FAILED_PREFIX}{os_message}"),
            user_text: None,
            inner_hresult: None,
        },
    }
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
    let mut buffer = PixelBuffer::allocate(width, height, layout).map_err(|e| decode_error_from_pixels(&e))?;
    match copy_pixels_guarded(|| copy(&mut buffer), color_transform_active) {
        Ok(()) => Ok(buffer),
        Err(error) => {
            drop(buffer);
            Err(error)
        }
    }
}

/// C# `IsOpaqueFormat(Guid)`: membership in [`OPAQUE_PIXEL_FORMATS`].
pub fn is_opaque_format(pixel_format: &GUID) -> bool {
    OPAQUE_PIXEL_FORMATS.contains(pixel_format)
}

/// The decision half of C# `TryGetNativeReducedSize`: the codec's closest size is useful only when it does not
/// undershoot the target on either axis AND reduces at least one axis:
/// `cw >= tw && ch >= th && (cw < ow || ch < oh)`.
pub fn native_reduction_is_useful(target: (u32, u32), original: (u32, u32), closest: (u32, u32)) -> bool {
    closest.0 >= target.0 && closest.1 >= target.1 && (closest.0 < original.0 || closest.1 < original.1)
}

/// C# `SelectSourceColorContext`, generic over the context type so it is testable without COM. Walks `contexts` in
/// order, skipping `None`: `context_type(c)` for each; `Profile` -> return its index at once (later entries are not
/// queried); `ExifColorSpace` while no candidate yet -> `exif_color_space(c)`, and a value `!= SRGB_EXIF_COLOR_SPACE`
/// becomes the candidate (a later ExifColorSpace entry is then not asked for its value); any other type is ignored.
/// Result: the profile index, else the candidate, else `None` (also for an empty slice). An `Err` from either closure
/// propagates at once (C# has no catch here).
pub fn select_source_color_context<C, T, E>(
    contexts: &[Option<C>],
    mut context_type: T,
    mut exif_color_space: E,
) -> Result<Option<usize>, DecodeError>
where
    T: FnMut(&C) -> Result<WICColorContextType, DecodeError>,
    E: FnMut(&C) -> Result<u32, DecodeError>,
{
    let mut exif_candidate = None;
    for (index, context) in contexts.iter().enumerate() {
        let Some(context) = context else { continue };
        let kind = context_type(context)?;
        if kind == WICColorContextProfile {
            return Ok(Some(index));
        }
        if kind == WICColorContextExifColorSpace
            && exif_candidate.is_none()
            && exif_color_space(context)? != SRGB_EXIF_COLOR_SPACE
        {
            exif_candidate = Some(index);
        }
    }
    Ok(exif_candidate)
}

/// C# `MapToTransformOptions`: 2 FlipHorizontal, 3 Rotate180, 4 FlipVertical, 5 FlipHorizontal|Rotate270, 6 Rotate90,
/// 7 FlipHorizontal|Rotate90, 8 Rotate270, anything else Rotate0 (no rotator is created for Rotate0).
pub fn map_to_transform_options(orientation: i32) -> WICBitmapTransformOptions {
    match orientation {
        2 => WICBitmapTransformFlipHorizontal,
        3 => WICBitmapTransformRotate180,
        4 => WICBitmapTransformFlipVertical,
        5 => WICBitmapTransformOptions(WICBitmapTransformFlipHorizontal.0 | WICBitmapTransformRotate270.0),
        6 => WICBitmapTransformRotate90,
        7 => WICBitmapTransformOptions(WICBitmapTransformFlipHorizontal.0 | WICBitmapTransformRotate90.0),
        8 => WICBitmapTransformRotate270,
        _ => WICBitmapTransformRotate0,
    }
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
    let message = error.to_string();
    match error.exception_kind() {
        DotnetExceptionKind::Com => DecodeError::Com {
            hresult: error.hresult(),
            message,
        },
        DotnetExceptionKind::Argument | DotnetExceptionKind::ArgumentOutOfRange => {
            DecodeError::InvalidArgument { message }
        }
        DotnetExceptionKind::OutOfMemory => DecodeError::OutOfMemory { message },
        DotnetExceptionKind::InvalidCast => DecodeError::InvalidCast { message },
        DotnetExceptionKind::NotSupported | DotnetExceptionKind::PlatformNotSupported => DecodeError::NotSupported {
            message,
            user_text: None,
            inner_hresult: None,
        },
        DotnetExceptionKind::FileNotFound => DecodeError::FileNotFound { message },
        DotnetExceptionKind::DirectoryNotFound => DecodeError::DirectoryNotFound { message },
        DotnetExceptionKind::Io
        | DotnetExceptionKind::EndOfStream
        | DotnetExceptionKind::DriveNotFound
        | DotnetExceptionKind::PathTooLong
        | DotnetExceptionKind::FileLoad => DecodeError::Io(io::Error::other(message)),
        DotnetExceptionKind::OperationCanceled => DecodeError::Canceled,
        DotnetExceptionKind::Overflow => DecodeError::Overflow { message },
        DotnetExceptionKind::Format => DecodeError::Format { message },
        DotnetExceptionKind::BadImageFormat => DecodeError::BadImageFormat { message },
        DotnetExceptionKind::DllNotFound => DecodeError::DllNotFound { message },
        DotnetExceptionKind::EntryPointNotFound => DecodeError::EntryPointNotFound { message },
        DotnetExceptionKind::InvalidOperation => DecodeError::InvalidOperation { message },
        DotnetExceptionKind::ObjectDisposed => DecodeError::Disposed { message },
        // NullReference, NotImplemented, UnauthorizedAccess, Arithmetic, DivideByZero, ... (not on the whitelist).
        _ => DecodeError::Other { message },
    }
}

/// `PixelBuffer.Allocate` failures as the C# exception types: `WidthOutOfRange`/`HeightOutOfRange`/`UnknownLayout`/
/// `RowOutOfRange` (`ArgumentOutOfRangeException`, an `ArgumentException`) -> `InvalidArgument`; `OutOfMemory` ->
/// `OutOfMemory`; `Disposed` -> `Disposed`. Message = `error.to_string()`.
pub fn decode_error_from_pixels(error: &PixelBufferError) -> DecodeError {
    let message = error.to_string();
    match error {
        PixelBufferError::WidthOutOfRange { .. }
        | PixelBufferError::HeightOutOfRange { .. }
        | PixelBufferError::UnknownLayout(_)
        | PixelBufferError::RowOutOfRange { .. } => DecodeError::InvalidArgument { message },
        PixelBufferError::OutOfMemory { .. } => DecodeError::OutOfMemory { message },
        PixelBufferError::Disposed => DecodeError::Disposed { message },
    }
}

/// The catch filter of C# `ReadColorContexts` (`ex is ArgumentException or COMException`): `InvalidArgument` or
/// `Com`. Such a fault means "treat the picture as untagged sRGB"; anything else propagates.
pub fn is_color_metadata_fault(error: &DecodeError) -> bool {
    matches!(
        error.kind(),
        DecodeError::InvalidArgument { .. } | DecodeError::Com { .. }
    )
}

/// The outer catch of C# `ColorTransformChain.Build` (`COMException or InvalidCastException or ArgumentException`):
/// those three become [`icc_transform_failed`]`(message)`; anything else (e.g. `OutOfMemory`) is returned unchanged.
pub fn color_chain_failure(error: DecodeError) -> DecodeError {
    match error.kind() {
        DecodeError::Com { hresult, .. } => icc_transform_failed(&error.message(), Some(*hresult)),
        DecodeError::InvalidCast { .. } | DecodeError::InvalidArgument { .. } => {
            icc_transform_failed(&error.message(), None)
        }
        _ => error,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// COM steps (private; built on super::interop helpers, which count and release every reference)
// ---------------------------------------------------------------------------------------------------------------

/// A failed `windows` crate call as the C# exception (`WicError::from_windows`: code 0 becomes `E_FAIL`).
fn com_error(error: &windows::core::Error) -> DecodeError {
    decode_error_from_wic(&WicError::from_windows(error))
}

/// `IWICBitmapSource::GetSize`.
fn source_size(source: &IWICBitmapSource) -> Result<(u32, u32), DecodeError> {
    let (mut width, mut height) = (0u32, 0u32);
    // SAFETY: `source` is a live interface borrowed for the call; both out pointers are valid, writable locals.
    unsafe { source.GetSize(&raw mut width, &raw mut height) }.map_err(|e| com_error(&e))?;
    Ok((width, height))
}

/// `IWICBitmapSource::GetPixelFormat`.
fn pixel_format(source: &IWICBitmapSource) -> Result<GUID, DecodeError> {
    // SAFETY: `source` is a live interface borrowed for the call; the crate supplies the out GUID.
    unsafe { source.GetPixelFormat() }.map_err(|e| com_error(&e))
}

/// `IWICBitmapScaler::Initialize(source, w, h, mode)`.
fn scaler_initialize(
    scaler: &IWICBitmapScaler,
    source: &IWICBitmapSource,
    width: u32,
    height: u32,
    mode: WICBitmapInterpolationMode,
) -> Result<(), DecodeError> {
    // SAFETY: both interfaces are live for the call; the scaler AddRefs the source it keeps.
    unsafe { scaler.Initialize(source, width, height, mode) }.map_err(|e| com_error(&e))
}

/// `IWICFormatConverter::Initialize(source, format, DitherTypeNone, NULL palette, 0.0, PaletteTypeCustom)`.
fn converter_initialize(
    converter: &IWICFormatConverter,
    source: &IWICBitmapSource,
    format: &GUID,
) -> Result<(), DecodeError> {
    // SAFETY: both interfaces are live for the call; `format` points to a GUID that outlives it; the palette is NULL
    // as in C# (`IntPtr.Zero`).
    unsafe {
        converter.Initialize(
            source,
            format,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }
    .map_err(|e| com_error(&e))
}

/// `IWICBitmapFlipRotator::Initialize(source, options)`.
fn flip_rotator_initialize(
    rotator: &IWICBitmapFlipRotator,
    source: &IWICBitmapSource,
    options: WICBitmapTransformOptions,
) -> Result<(), DecodeError> {
    // SAFETY: both interfaces are live for the call; the rotator AddRefs the source it keeps.
    unsafe { rotator.Initialize(source, options) }.map_err(|e| com_error(&e))
}

/// The one `CopyPixels(NULL rect, stride, buffer)` of the decode, straight into the native pixel buffer (WP-03).
fn copy_pixels(source: &IWICBitmapSource, stride: u32, buffer: &mut [u8]) -> Result<(), DecodeError> {
    // SAFETY: `source` is live; a NULL rect means the whole image (C# `IntPtr.Zero`); `buffer` is an exclusive,
    // writable slice whose length (stride * height) is what the crate passes as `cbBufferSize`, so WIC never writes
    // past it.
    unsafe { source.CopyPixels(std::ptr::null(), stride, buffer) }.map_err(|e| com_error(&e))
}

/// `IWICColorContext::GetType`.
fn context_type(context: &IWICColorContext) -> Result<WICColorContextType, DecodeError> {
    // SAFETY: `context` is a live interface borrowed for the call.
    unsafe { context.GetType() }.map_err(|e| com_error(&e))
}

/// `IWICColorContext::GetExifColorSpace`.
fn exif_color_space(context: &IWICColorContext) -> Result<u32, DecodeError> {
    // SAFETY: `context` is a live interface borrowed for the call.
    unsafe { context.GetExifColorSpace() }.map_err(|e| com_error(&e))
}

/// C# `TryGetNativeReducedSize`: `frame` QueryInterface `IWICBitmapSourceTransform` (failure -> `Ok(None)`);
/// `GetClosestSize(&mut w, &mut h)` starting from the target; a `Com`-kind failure -> `Ok(None)`, any other failure
/// propagates (C# catches `COMException` only); then [`native_reduction_is_useful`] -> `Ok(Some((w, h)))` or `Ok(None)`.
fn try_get_native_reduced_size(
    frame: &IWICBitmapFrameDecode,
    target: (u32, u32),
    original: (u32, u32),
) -> Result<Option<(u32, u32)>, DecodeError> {
    // C# `frame is not IWICBitmapSourceTransform`: a view of the counted frame (OQ5), any QI failure is "no".
    let Ok(transform) = frame.cast::<IWICBitmapSourceTransform>() else {
        return Ok(None);
    };
    let (mut width, mut height) = target;
    // SAFETY: `transform` is a live interface owned by this frame; both in/out pointers are valid, writable locals.
    match unsafe { transform.GetClosestSize(&raw mut width, &raw mut height) }.map_err(|e| com_error(&e)) {
        Ok(()) => {}
        Err(DecodeError::Com { .. }) => return Ok(None),
        Err(other) => return Err(other),
    }
    Ok(native_reduction_is_useful(target, original, (width, height)).then_some((width, height)))
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
    let contexts = read_color_contexts_with(
        || {
            let mut count = 0u32;
            // T8: C# passes `IntPtr.Zero`. The `windows` wrapper would pass an empty slice's dangling NON-null pointer,
            // which some codecs (JPEG XR) refuse with E_INVALIDARG (the colour contexts were then silently skipped), so
            // the raw vtable entry is called with a null array.
            // SAFETY: `frame` is a live interface pointer and `vtable` its own vtable; cCount = 0 with a null array is
            // the documented count query; `count` is a valid, writable local.
            let hr = unsafe {
                (Interface::vtable(frame).GetColorContexts)(frame.as_raw(), 0, std::ptr::null_mut(), &raw mut count)
            };
            hr.ok().map_err(|e| com_error(&e))?;
            Ok(count)
        },
        || create_color_context(factory).map_err(|e| decode_error_from_wic(&e)),
        |contexts: &[Com<IWICColorContext>]| {
            let mut slots: Vec<Option<IWICColorContext>> = contexts.iter().map(|c| Some((**c).clone())).collect();
            let mut actual = 0u32;
            // SAFETY: `frame` is live; `slots` holds live, initialised-by-us colour contexts that WIC fills in place
            // (it does not replace the pointers); `actual` is a valid, writable local.
            unsafe { frame.GetColorContexts(&mut slots, &raw mut actual) }.map_err(|e| com_error(&e))
        },
    )?;
    Ok(contexts.map(|contexts| contexts.into_iter().map(Some).collect()))
}

/// The decisions of C# `ReadColorContexts` over three seams (the WIC calls in production, closures in the tests):
/// `count_query` (a [`is_color_metadata_fault`] -> `Ok(None)`, anything else propagates); `0` -> `Ok(None)` without
/// creating anything; `create` once per context; `fill` over all of them. A metadata fault in `create` or `fill`
/// drops every context made so far and gives `Ok(None)`; any other error drops them and propagates.
fn read_color_contexts_with<C>(
    count_query: impl FnOnce() -> Result<u32, DecodeError>,
    mut create: impl FnMut() -> Result<C, DecodeError>,
    fill: impl FnOnce(&[C]) -> Result<(), DecodeError>,
) -> Result<Option<Vec<C>>, DecodeError> {
    let count = match count_query() {
        Ok(count) => count,
        Err(fault) if is_color_metadata_fault(&fault) => return Ok(None),
        Err(other) => return Err(other),
    };
    if count == 0 {
        return Ok(None);
    }

    // Dropping `contexts` on any early return releases every reference made so far (C# `ReleaseRaw`/`ReleaseAll`).
    let filled = (|| {
        let mut contexts = Vec::with_capacity(count as usize);
        for _ in 0..count {
            contexts.push(create()?);
        }
        fill(&contexts)?;
        Ok(contexts)
    })();
    match filled {
        Ok(contexts) => Ok(Some(contexts)),
        Err(fault) if is_color_metadata_fault(&fault) => Ok(None),
        Err(other) => Err(other),
    }
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
        self.build_inner(factory, source, source_profile, opaque)
            .map_err(color_chain_failure)
    }

    fn build_inner(
        &mut self,
        factory: &IWICImagingFactory,
        source: &IWICBitmapSource,
        source_profile: &IWICColorContext,
        opaque: bool,
    ) -> Result<IWICBitmapSource, DecodeError> {
        // Straight (non-premultiplied) alpha: colour maths on premultiplied values would be wrong.
        let transform_format = if opaque {
            guids::PIXEL_FORMAT_32BPP_BGR
        } else {
            guids::PIXEL_FORMAT_32BPP_BGRA
        };
        let destination = create_color_context(factory).map_err(|e| decode_error_from_wic(&e))?;
        // SAFETY: `destination` is a live, freshly created colour context.
        unsafe { destination.InitializeFromExifColorSpace(SRGB_EXIF_COLOR_SPACE) }.map_err(|e| com_error(&e))?;
        let destination: &IWICColorContext = self.destination.insert(destination);

        // First let the transform consume the native format (keeps CMYK/gray profiles valid).
        let transform = create_color_transformer(factory).map_err(|e| decode_error_from_wic(&e))?;
        match color_transform_initialize(&transform, source, source_profile, destination, &transform_format) {
            Ok(()) => {
                let view: IWICBitmapSource = (*transform).clone().into();
                self.transform = Some(transform);
                return Ok(view);
            }
            // Source format not accepted directly: release the transform, normalise to 32bpp BGR(A) and retry.
            Err(DecodeError::Com { .. }) => drop(transform),
            Err(other) => return Err(other),
        }

        let normalizer = create_format_converter(factory).map_err(|e| decode_error_from_wic(&e))?;
        let normalizer: &IWICFormatConverter = self.normalizer.insert(normalizer);
        converter_initialize(normalizer, source, &transform_format)?;
        let normalized: IWICBitmapSource = (*normalizer).clone().into();
        let transform = create_color_transformer(factory).map_err(|e| decode_error_from_wic(&e))?;
        let transform: &IWICColorTransform = self.transform.insert(transform);
        color_transform_initialize(transform, &normalized, source_profile, destination, &transform_format)?;
        Ok((*transform).clone().into())
    }
}

/// `IWICColorTransform::Initialize(source, sourceContext, destinationContext, format)`.
fn color_transform_initialize(
    transform: &IWICColorTransform,
    source: &IWICBitmapSource,
    source_context: &IWICColorContext,
    destination_context: &IWICColorContext,
    format: &GUID,
) -> Result<(), DecodeError> {
    // SAFETY: all four interfaces are live for the call; `format` points to a GUID that outlives it.
    unsafe { transform.Initialize(source, source_context, destination_context, format) }.map_err(|e| com_error(&e))
}

impl fmt::Debug for ColorTransformChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ColorTransformChain")
            .field("active", &self.is_active())
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// W4-11d seams over the WIC objects of this call (borrowed views; the reader is a counted `Com`)
// ---------------------------------------------------------------------------------------------------------------

/// The frame as `exif_reader::FrameMetadata` (C# `WicExifReader.ReadFrameMetadata(frame, ..)`).
struct FrameView<'a>(&'a IWICBitmapFrameDecode);

/// The decoder as `exif_reader::DecoderContainer` (C# `WicExifReader.ExifIfdRootOf(decoder)`).
struct DecoderView<'a>(&'a IWICBitmapDecoder);

/// The frame's metadata query reader (counted; released when the read ends).
struct QueryReaderView(Com<IWICMetadataQueryReader>);

impl FrameMetadata for FrameView<'_> {
    type Reader = QueryReaderView;

    fn metadata_query_reader(&self) -> Result<Self::Reader, HResult> {
        get_metadata_query_reader(self.0)
            .map(QueryReaderView)
            .map_err(|e| e.hresult())
    }
}

impl MetadataQueryReader for QueryReaderView {
    fn get_metadata_by_name(&self, name: &str, value: &mut PropVariant) -> HResult {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the reader is live; `wide` is a NUL-terminated UTF-16 string that outlives the call; `value` is an
        // exclusive, 8-byte aligned, zeroed 24-byte PROPVARIANT buffer (W4-11d), exactly `sizeof(PROPVARIANT)` on x64,
        // which WIC fills and the caller clears.
        let result = unsafe {
            self.0
                .GetMetadataByName(PCWSTR(wide.as_ptr()), value.as_mut_ptr().cast::<PROPVARIANT>())
        };
        match result {
            Ok(()) => 0,
            Err(error) => error.code().0,
        }
    }
}

impl DecoderContainer for DecoderView<'_> {
    fn container_format(&self) -> Result<DotnetGuid, HResult> {
        // SAFETY: `self.0` is a live decoder borrowed for the call.
        let guid = unsafe { self.0.GetContainerFormat() }.map_err(|e| e.code().0)?;
        Ok(dotnet_guid(&guid))
    }
}

/// A `windows` GUID in .NET `Guid.ToByteArray()` order (`data1..3` little endian, then `data4`).
fn dotnet_guid(guid: &GUID) -> DotnetGuid {
    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&guid.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
    bytes[8..16].copy_from_slice(&guid.data4);
    DotnetGuid(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn com_fault() -> DecodeError {
        DecodeError::Com {
            hresult: 0x8898_2F04_u32 as i32,
            message: "com".into(),
        }
    }

    fn argument_fault() -> DecodeError {
        DecodeError::InvalidArgument { message: "arg".into() }
    }

    fn out_of_memory() -> DecodeError {
        DecodeError::OutOfMemory { message: "oom".into() }
    }

    // C#: WicDirectFaultInjectionTests.ReadColorContexts_CountQueryFails_IsUntaggedSrgb (both caught types)
    #[test]
    fn count_query_metadata_fault_is_untagged() {
        for fault in [com_fault, argument_fault] {
            let mut created = 0;
            let result = read_color_contexts_with(
                || Err(fault()),
                || {
                    created += 1;
                    Ok(())
                },
                |_| Ok(()),
            );
            assert!(matches!(result, Ok(None)));
            assert_eq!(created, 0);
        }
    }

    // C#: WicDirectFaultInjectionTests.ReadColorContexts_UnexpectedFault_Propagates (count query)
    #[test]
    fn count_query_other_fault_propagates() {
        let result = read_color_contexts_with(|| Err(out_of_memory()), || Ok(()), |_| Ok(()));
        assert!(matches!(result, Err(DecodeError::OutOfMemory { .. })));
        let result = read_color_contexts_with(
            || Err(DecodeError::InvalidCast { message: "cast".into() }),
            || Ok(()),
            |_| Ok(()),
        );
        assert!(matches!(result, Err(DecodeError::InvalidCast { .. })));
    }

    #[test]
    fn zero_contexts_create_nothing() {
        let mut created = 0;
        let mut filled = false;
        let result = read_color_contexts_with(
            || Ok(0),
            || {
                created += 1;
                Ok(())
            },
            |_| {
                filled = true;
                Ok(())
            },
        );
        assert!(matches!(result, Ok(None)));
        assert_eq!((created, filled), (0, false));
    }

    #[test]
    fn every_context_is_created_and_filled() {
        let mut next = 0u32;
        let mut seen = Vec::new();
        let result = read_color_contexts_with(
            || Ok(3),
            || {
                next += 1;
                Ok(next)
            },
            |contexts| {
                seen.extend_from_slice(contexts);
                Ok(())
            },
        );
        assert_eq!(result.ok().flatten(), Some(vec![1, 2, 3]));
        assert_eq!(seen, vec![1, 2, 3]);
    }

    // C#: WicDirectFaultInjectionTests.ReadColorContexts_SecondQueryOrContextCreationFails_IsUntaggedSrgb
    #[test]
    fn creation_or_fill_metadata_fault_is_untagged() {
        for fault in [com_fault, argument_fault] {
            let mut made = 0;
            let result = read_color_contexts_with(
                || Ok(2),
                || {
                    made += 1;
                    if made == 2 { Err(fault()) } else { Ok(made) }
                },
                |_| Ok(()),
            );
            assert!(matches!(result, Ok(None)), "creation");
            let result = read_color_contexts_with(|| Ok(2), || Ok(0u8), |_| Err(fault()));
            assert!(matches!(result, Ok(None)), "fill");
        }
    }

    // C#: WicDirectFaultInjectionTests.ReadColorContexts_UnexpectedFault_Propagates (creation and fill)
    #[test]
    fn creation_or_fill_other_fault_propagates() {
        let result = read_color_contexts_with(|| Ok(2), || Err::<u8, _>(out_of_memory()), |_| Ok(()));
        assert!(matches!(result, Err(DecodeError::OutOfMemory { .. })), "creation");
        let result = read_color_contexts_with(|| Ok(2), || Ok(0u8), |_| Err(out_of_memory()));
        assert!(matches!(result, Err(DecodeError::OutOfMemory { .. })), "fill");
    }

    fn fixture_bytes(name: &str) -> Arc<[u8]> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/wic-direct")
            .join(name);
        Arc::from(std::fs::read(path).expect("fixture"))
    }

    /// `GetExifColorSpace` returns the context's own value (the EXIF branch of the colour selection), for sRGB (1)
    /// and Adobe RGB (2).
    #[test]
    #[ignore = "native: needs WIC"]
    fn exif_color_space_reads_the_context_value() {
        let factory = create_factory().expect("factory");
        for value in [1u32, 2] {
            let context = create_color_context(&factory).expect("context");
            // SAFETY: `context` is a live, freshly created colour context.
            unsafe { context.InitializeFromExifColorSpace(value) }.expect("initialize");
            assert_eq!(context_type(&context).expect("type"), WICColorContextExifColorSpace);
            assert_eq!(exif_color_space(&context).expect("value"), value);
        }
    }

    /// C# `ColorTransformChain.IsActive` after a build over the Display P3 JPEG: the copy guard then reports a COM
    /// failure of the (lazy) transform as the fallbackable ICC `NotSupported` (WicIccDiagnosticsTests).
    #[test]
    #[ignore = "native: needs WIC"]
    fn color_chain_over_a_profiled_frame_is_active() {
        let stream = ComStream::from_bytes(fixture_bytes("icc-p3-64x48.jpg"));
        let factory = create_factory().expect("factory");
        let decoder =
            create_decoder_from_stream(&factory, &stream.istream(), WICDecodeMetadataCacheOnDemand).expect("decoder");
        let frame = get_frame(&decoder, 0).expect("frame");
        let contexts = read_color_contexts(&factory, &frame)
            .expect("contexts")
            .expect("tagged");
        let chosen = select_source_color_context(&contexts, |c| context_type(c), |c| exif_color_space(c))
            .expect("select")
            .expect("a profile is chosen");
        let profile = contexts[chosen].as_ref().expect("context");
        let source: IWICBitmapSource = frame.cast().expect("source view");
        assert!(!ColorTransformChain::default().is_active());
        let mut chain = ColorTransformChain::default();
        let output = chain.build(&factory, &source, profile, true).expect("chain");
        assert!(chain.is_active());
        let error = copy_pixels_guarded(|| Err(com_fault()), chain.is_active()).expect_err("copy fails");
        assert!(
            matches!(error.kind(), DecodeError::NotSupported { .. })
                && error.message().starts_with(ICC_TRANSFORM_FAILED_PREFIX),
            "{error:?}"
        );
        drop(output);
        drop(chain);
    }
}
