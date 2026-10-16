//! Port of `PhotoReview.Imaging/Decoding/EmbeddedThumbnailReader.cs` (C# baseline d92d6aae).
//!
//! Reads a JPEG's embedded EXIF thumbnail (APP1 IFD1) without decoding the main image. `ThumbnailCache` calls it on a
//! cache miss so an open-folder placeholder never forces a second full decode of a source the preview is already
//! decoding (PARITY I-09, INV-8). Not a general decoder: every failure is "no thumbnail" (`Ok(None)`), never an error.
//!
//! Layering (P-22): this file is PURE. The WIC calls (`CreateDecoderFromStream`, `GetFrame`, `GetThumbnail`, the
//! 32bppPBGRA format converter, the frame metadata query reader) live in `pr_platform_win::wic` behind the three
//! traits below ([`EmbeddedThumbnailBackend`] -> [`EmbeddedThumbnailFrame`] -> [`EmbeddedThumbnailBitmap`]), implemented
//! by W4-11c/W4-11d. Tests of this file use a fake backend; the real-WIC differential tests are `#[ignore = "native"]`.
//!
//! Target path: `crates/pr-imaging/src/embedded_thumbnail.rs` (lead adds `pub mod embedded_thumbnail;`).

use std::fmt;
use std::path::Path;

use pr_core::abstractions::{FileSystem, ReadSeek};
use pr_core::model::enums::DecoderBackend;

use crate::decode_types::{DecodedImage, DecodedImageOptions};
use crate::decoder_traits::PlatformImageCodec;
use crate::orientation::is_transposed;
use crate::pixels::{PixelBuffer, PixelLayout, apply_orientation};

/// Read-ahead for the shared open (C# `FileStreamOptions` buffer, sequential scan; INV-8).
const SHARED_READ_BUFFER: usize = 64 * 1024;

/// EXIF thumbnails are about 160x120; anything above this many pixels is treated as corrupt
/// (C# `MaxThumbnailPixels`, 4 MP = 16 MB of BGRA).
pub const MAX_THUMBNAIL_PIXELS: i64 = 4 * 1024 * 1024;

/// True when a thumbnail of this size is positive and within [`MAX_THUMBNAIL_PIXELS`] (C# `IsAcceptableSize`).
/// The product is computed in `i64` (C# `(long)width * height`): `100000 x 100000` must be rejected, not overflow.
#[must_use]
pub fn is_acceptable_size(width: i32, height: i32) -> bool {
    width > 0 && height > 0 && i64::from(width) * i64::from(height) <= MAX_THUMBNAIL_PIXELS
}

/// Why the backend could not do a step. The reader treats EVERY value as "no thumbnail" (C# catches IOException,
/// NotSupported, InvalidOperation, UnauthorizedAccess, Argument, Overflow, InvalidCast, Format and COMException alike);
/// the text is only for a debug log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailBackendError(pub String);

impl fmt::Display for ThumbnailBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ThumbnailBackendError {}

/// The caller passed a blank path (C# `ArgumentException.ThrowIfNullOrWhiteSpace(path)`, thrown OUTSIDE the catch).
/// Rust has no null path; an empty or whitespace-only one is the same programming error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlankPath;

impl fmt::Display for BlankPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("path must not be null, empty or white space")
    }
}

impl std::error::Error for BlankPath {}

/// The embedded thumbnail of one frame, already a decodable bitmap source (C# `IWICBitmapSource` from
/// `IWICBitmapFrameDecode::GetThumbnail`).
pub trait EmbeddedThumbnailBitmap {
    /// Pixel size the thumbnail claims (C# `GetSize`, unsigned: the reader rejects > `i32::MAX`).
    ///
    /// # Errors
    /// Any backend failure; the reader maps it to "no thumbnail".
    fn size(&mut self) -> Result<(u32, u32), ThumbnailBackendError>;

    /// Converts to 32bppPBGRA (no dither, no palette, alpha threshold 0) and copies `dst.len()` bytes with the given
    /// stride (C# `CreateFormatConverter` + `Initialize` + `CopyPixels`). Called only after the size passed
    /// [`is_acceptable_size`], with `dst.len() == stride * height`.
    ///
    /// # Errors
    /// Any backend failure; the reader drops the buffer and maps it to "no thumbnail".
    fn copy_pbgra(&mut self, dst: &mut [u8], stride: usize) -> Result<(), ThumbnailBackendError>;
}

/// Frame 0 of an opened container.
pub trait EmbeddedThumbnailFrame {
    /// The MAIN frame's header dimensions (free once the header is parsed; the main pixels are never decoded).
    ///
    /// # Errors
    /// Any backend failure; the reader maps it to "no thumbnail".
    fn main_size(&mut self) -> Result<(u32, u32), ThumbnailBackendError>;

    /// The frame's embedded thumbnail; `None` when the codec has none (`WINCODEC_ERR_CODECNOTHUMBNAIL` and kin) or
    /// `GetThumbnail` failed in any other COM way (C# `TryGetThumbnail` returns false for every `COMException`).
    fn thumbnail(&mut self) -> Option<Box<dyn EmbeddedThumbnailBitmap>>;

    /// EXIF orientation 1..=8 via the frame's metadata query reader (`/app1/ifd/{ushort=274}` then
    /// `System.Photo.Orientation`); 1 for absent, out of range, or any failure (C# `WicExifReader.ReadOrientation`
    /// never throws). Pure rule: W4-02a `ExifOrientation::read_from_wic`.
    fn orientation(&mut self) -> i32;
}

/// Opens an image container from a stream (C# `WicDirectDecoder.CreateFactory` + `CreateDecoderFromStream` with
/// `WICDecodeMetadataCacheOnDemand`, `GetFrameCount`, `GetFrame(0)`). Implemented in `pr_platform_win::wic`.
pub trait EmbeddedThumbnailBackend: Send + Sync {
    /// `Ok(None)` when the container has zero frames. `Err` for anything WIC refuses (not an image, damaged header).
    /// The backend takes ownership of `stream` and releases it when the returned frame is dropped.
    ///
    /// # Errors
    /// Any backend failure; the reader maps it to "no thumbnail".
    fn open(&self, stream: Box<dyn ReadSeek>)
    -> Result<Option<Box<dyn EmbeddedThumbnailFrame>>, ThumbnailBackendError>;
}

/// The source's embedded EXIF thumbnail with orientation applied, or `Ok(None)` when it has none (wrong format, no
/// APP1 thumbnail, unreadable file, any decode failure: all the same, "nothing to show yet").
///
/// Pixels are premultiplied BGRA (`PixelLayout::Pbgra32`, A = 255 for a JPEG thumbnail) so the thumbnail disk cache
/// keeps writing the same PNG format. `downscaled` is always true; `actual_backend` is `DecoderBackend::Wpf` on
/// purpose (the backend label is part of the cache identity: do not "fix" it to WicDirect); `original_width/height`
/// are the MAIN frame's header size, swapped when the orientation is 5..=8.
///
/// Order of effects (each step may return `Ok(None)`): blank path -> `Err(BlankPath)` first; open the file shared
/// (`fs.open_read_shared(path, 64 * 1024)`: ReadWrite|Delete sharing, sequential scan, INV-8); `backend.open`;
/// frame count 0; `main_size` (a side above `i32::MAX`); `thumbnail()`; thumbnail `size()` (a side above
/// `i32::MAX` or `!is_acceptable_size`: refuse BEFORE any allocation); allocate `PixelBuffer(w, h, Pbgra32)`;
/// `copy_pbgra` (on failure drop the buffer); `orientation()`; `apply_orientation` (owns the buffer);
/// `codec.image_from_pixels`; build the `DecodedImage` with `estimated_bytes` = oriented byte count.
///
/// # Errors
/// Only [`BlankPath`]; I/O and decode failures are `Ok(None)`.
pub fn try_read(
    path: &Path,
    fs: &dyn FileSystem,
    backend: &dyn EmbeddedThumbnailBackend,
    codec: &dyn PlatformImageCodec,
) -> Result<Option<DecodedImage>, BlankPath> {
    // C# ArgumentException.ThrowIfNullOrWhiteSpace(path) runs before the try block: a blank path is the caller's bug.
    if path.to_string_lossy().trim().is_empty() {
        return Err(BlankPath);
    }
    Ok(read_thumbnail(path, fs, backend, codec))
}

/// Every step after the blank-path check. Any failure is `None`; the frame (and with it the shared stream) is dropped
/// when this returns, before the caller sees the result (contract note 3).
fn read_thumbnail(
    path: &Path,
    fs: &dyn FileSystem,
    backend: &dyn EmbeddedThumbnailBackend,
    codec: &dyn PlatformImageCodec,
) -> Option<DecodedImage> {
    let stream = fs.open_read_shared(path, SHARED_READ_BUFFER).ok()?;
    let mut frame = backend.open(stream).ok()??;

    // Main size first: a side above i32::MAX is refused before the thumbnail is even fetched.
    let (main_width, main_height) = frame.main_size().ok()?;
    let main_width = i32::try_from(main_width).ok()?;
    let main_height = i32::try_from(main_height).ok()?;

    let mut bitmap = frame.thumbnail()?;
    let (thumb_width, thumb_height) = bitmap.size().ok()?;
    let thumb_width = i32::try_from(thumb_width).ok()?;
    let thumb_height = i32::try_from(thumb_height).ok()?;
    // The size gate precedes every allocation (C# MaxThumbnailPixels; WIC refuses big thumbnails anyway).
    if !is_acceptable_size(thumb_width, thumb_height) {
        return None;
    }

    let mut pixels = PixelBuffer::allocate(thumb_width, thumb_height, PixelLayout::Pbgra32).ok()?;
    let stride = usize::try_from(thumb_width).ok()? * 4;
    bitmap.copy_pbgra(pixels.bytes_mut().ok()?, stride).ok()?;

    // Read after the copy, like C#. Infallible on the backend side (1 on any failure).
    let orientation = frame.orientation();
    let oriented = apply_orientation(pixels, orientation).ok()?;

    let transposed = is_transposed(orientation);
    let (original_width, original_height) = if transposed {
        (main_height, main_width)
    } else {
        (main_width, main_height)
    };
    let pixel_width = oriented.width();
    let pixel_height = oriented.height();
    let estimated_bytes = oriented.byte_count();
    let platform_image = codec.image_from_pixels(oriented).ok()?;

    DecodedImage::new(
        platform_image,
        pixel_width,
        pixel_height,
        estimated_bytes,
        DecodedImageOptions {
            downscaled: true,
            orientation,
            // Deliberately the WPF label (cache identity, PARITY D-07); never WicDirect.
            actual_backend: DecoderBackend::Wpf,
            original_width,
            original_height,
            exif: None,
            is_degraded_fallback: false,
        },
    )
    .ok()
}
