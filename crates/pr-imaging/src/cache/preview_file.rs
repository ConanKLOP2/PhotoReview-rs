//! Port of `PhotoReview.Imaging/Caching/PreviewCacheFile.cs` (C# baseline d92d6aae; PARITY D-06, I-08; INV-1, INV-2).
//!
//! The `.pv4` preview disk-cache entry: ONE file with a fixed 24-byte little-endian header, an optional EXIF block
//! (v7) and a JPEG payload to end of file. It is a PERSISTED FORMAT shared with the C# build (both read each other's
//! caches, D-06): every byte written here must equal what C# writes.
//!
//! ```text
//! 0  "PRVC"          4  version (7; 6 still READ, no EXIF block)   5  DecoderBackend byte (0..=3)
//! 6  orientation 1-8 7  alpha flag (0 = Bgr32 payload, else Pbgra32; the writer always stores 0)
//! 8  i32 width       12 i32 height   16 i32 original width   20 i32 original height   (all > 0)
//! 24 u16 EXIF length N (v7 only; N <= 1024)    26 N bytes `exif_summary::codec` data    then JPEG bytes
//! ```
//!
//! Split of the C# static class:
//! * the PURE half (this file, no OS): header build/parse, validation order, EXIF block bounds, the JPEG end-marker
//!   check ([`parse_entry`], [`build_entry_prefix`]);
//! * file I/O ([`read`], [`write_atomically`]): `std::fs` here; the temp-file-then-rename primitive is W4-08a
//!   (`cache::atomic_file`);
//! * the JPEG encode/decode is behind [`PreviewPayloadCodec`]; the WIC implementation is W4-11e
//!   (`pr_platform_win::wic::cache_image`, `WicImageEncoder` + `WicCacheImageReader`). `pr-imaging` stays OS-free.
//!
//! Exceptions become [`PreviewCacheError`]; `InvalidDataException` is `InvalidData` (the cache layer deletes such an
//! entry and re-decodes the source), `IOException` and friends are `Io`; `OperationCanceledException` is `Canceled` and
//! `InsufficientMemoryException` (decoder out of memory: the entry is intact and must be KEPT) is `OutOfMemory`.

#![allow(unused_variables, dead_code, unused_imports)] // D11: remove with the last `todo!()`.

use std::path::Path;

use pr_core::model::DecoderBackend;
use pr_core::sync::CancelToken;

use crate::decode_types::{DecodedImage, DecodedImageOptions};
use crate::decoder_traits::PlatformImageCodec;
use crate::exif_summary::ExifSummary;
use crate::pixels::{PixelBuffer, PixelLayout};

pub const CURRENT_VERSION: u8 = 7;
/// `PreviewCacheFile.NoExifVersion`: the previous layout, still readable (as "no EXIF").
pub const NO_EXIF_VERSION: u8 = 6;
/// `PreviewCacheFile.DefaultJpegQuality`: JPEG q95, 4:4:4 (chosen by measurement; do not change).
pub const DEFAULT_JPEG_QUALITY: i32 = 95;
/// Fixed header length (C# private `HeaderSize`).
pub const HEADER_SIZE: usize = 24;
/// Header magic (C# `"PRVC"u8`).
pub const MAGIC: [u8; 4] = *b"PRVC";
/// Largest EXIF block a v7 entry may declare (`ExifSummaryCodec.MaxEncodedLength`, W4-04a `codec::MAX_ENCODED_LENGTH`).
pub const MAX_EXIF_BLOCK: usize = 1024;

/// What can go wrong reading or writing an entry. The C# exception type decides how the caller reacts
/// (`DiskCacheStore.IsCacheEntryFailure`: everything except [`Canceled`](Self::Canceled) and
/// [`OutOfMemory`](Self::OutOfMemory) means "corrupt or unusable entry").
#[derive(Debug)]
pub enum PreviewCacheError {
    /// `InvalidDataException`: bad magic/version/backend/orientation/dimensions, bad EXIF bounds, no or truncated payload,
    /// payload that does not decode to the header's size. The message text is the C# message (golden-compared).
    InvalidData(String),
    /// `ArgumentException` of the writer (transparent pixels, blank path).
    InvalidArgument(String),
    /// `ArgumentOutOfRangeException` of the writer (orientation outside 1..=8).
    ArgumentOutOfRange(String),
    /// `IOException` / `EndOfStreamException` (file shorter than the 24-byte header is `UnexpectedEof`) /
    /// `UnauthorizedAccessException` (`PermissionDenied`).
    Io(std::io::Error),
    /// `OperationCanceledException`: the write was superseded; no file is published and the temp file is removed.
    Canceled,
    /// `InsufficientMemoryException` / `OutOfMemoryException` (`WicCacheImageReader.Decode` maps a WIC out-of-memory COM
    /// error to it): NOT a corrupt entry, the entry file is kept and the error propagates. `io::ErrorKind::OutOfMemory`
    /// from the payload encoder maps here too.
    OutOfMemory(String),
}

impl PreviewCacheError {
    /// `DiskCacheStore.IsCacheEntryFailure`: true for every variant except [`Self::Canceled`] and [`Self::OutOfMemory`].
    pub fn is_cache_entry_failure(&self) -> bool {
        todo!("W4-08b")
    }
}

impl std::fmt::Display for PreviewCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!("W4-08b")
    }
}

impl std::error::Error for PreviewCacheError {}

impl From<std::io::Error> for PreviewCacheError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// The 24 fixed header bytes, decoded and validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewHeader {
    /// The backend that ACTUALLY produced the pixels (may differ from the key's backend after a fallback).
    pub backend: DecoderBackend,
    /// EXIF orientation already baked into the pixels, 1..=8.
    pub orientation: u8,
    /// Header byte 7 != 0: the payload decodes to [`PixelLayout::Pbgra32`], else [`PixelLayout::Bgr32`].
    pub has_alpha: bool,
    /// Pixel size of the entry itself, both > 0.
    pub width: i32,
    pub height: i32,
    /// Full source size (post-orientation), both > 0.
    pub original_width: i32,
    pub original_height: i32,
}

/// Everything [`parse_entry`] learns from the bytes of an entry file without decoding the JPEG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEntry {
    pub header: PreviewHeader,
    /// `None` for v6, for an empty block, and for a block `codec::decode` cannot read (never an error).
    pub exif: Option<ExifSummary>,
    /// Index of the first payload byte (`24` for v6, `26 + exif length` for v7).
    pub payload_offset: usize,
    /// `bytes.len() - payload_offset`, always `>= 4` and ending in `FF D9`.
    pub payload_len: usize,
    /// `bytes.len()` as `i64` (C# `ReadResult.FileBytes`).
    pub file_bytes: i64,
}

/// C# `ReadResult`: a decoded entry. `pixels` is a fresh buffer owned by the caller.
#[derive(Debug)]
pub struct ReadResult {
    pub pixels: PixelBuffer,
    pub actual_backend: DecoderBackend,
    pub orientation: i32,
    pub file_bytes: i64,
    pub original_width: i32,
    pub original_height: i32,
    pub exif: Option<ExifSummary>,
}

/// JPEG encode/decode of the payload (W4-11e implements it with WIC; tests use a fake). Thread-safe.
pub trait PreviewPayloadCodec: Send + Sync {
    /// `WicImageEncoder.EncodeJpeg(pixels, stream, quality)`: writes a complete JPEG (SOI..EOI) of `pixels`
    /// (never modified) to `out`; byte-identical to the WPF `JpegBitmapEncoder` output for the same pixels (D-06).
    fn encode_jpeg(&self, pixels: &PixelBuffer, quality: i32, out: &mut dyn std::io::Write) -> std::io::Result<()>;

    /// `WicCacheImageReader.Decode(payload, layout, DecodeBox.Unbounded, expected_width, expected_height)`: a NEW buffer
    /// of exactly `expected_width x expected_height` in `layout`. Every decoder failure (COM error, size mismatch,
    /// empty payload, zero or `> i32::MAX` frame size, decoded size `> i32::MAX` bytes) is [`PreviewCacheError::InvalidData`], C# messages
    /// `"Preview cache entry payload could not be decoded."` (COM error wrapper) / `"Cache entry payload dimensions do not match its header."` /
    /// `"Cache entry payload is empty."` / `"Cache entry payload has invalid dimensions."` / `"Cache entry payload is implausibly large."`;
    /// a WIC out-of-memory error is [`PreviewCacheError::OutOfMemory`] (not InvalidData). The size check runs BEFORE any allocation.
    fn decode_jpeg(
        &self,
        payload: &[u8],
        layout: PixelLayout,
        expected_width: i32,
        expected_height: i32,
    ) -> Result<PixelBuffer, PreviewCacheError>;
}

/// Maps a header backend byte to the enum; `None` when `Enum.IsDefined` would be false (only 0..=3 are defined).
pub fn backend_from_byte(value: u8) -> Option<DecoderBackend> {
    todo!("W4-08b")
}

/// Validates and splits the bytes of an entry file; no JPEG decoding, no pixel allocation. The check order and the
/// message texts are those of C# `Read` (see the contract, "Validation order").
///
/// # Errors
/// `Io(UnexpectedEof)` when `bytes.len() < 24`; otherwise [`PreviewCacheError::InvalidData`].
pub fn parse_entry(bytes: &[u8]) -> Result<ParsedEntry, PreviewCacheError> {
    todo!("W4-08b")
}

/// Header (24) + u16 EXIF length + EXIF block of a NEW entry (C# `BuildHeader` + the v7 length/EXIF bytes), always v7 with
/// alpha flag 0. `original_*` `<= 0` fall back to `width`/`height`. The JPEG payload is appended by the caller.
///
/// # Errors
/// [`PreviewCacheError::ArgumentOutOfRange`] when `orientation` is outside 1..=8 (C# message
/// `"EXIF orientation must be 1-8. (Parameter 'orientation')\nActual value was {orientation}."`).
pub fn build_entry_prefix(
    backend: DecoderBackend,
    orientation: i32,
    width: i32,
    height: i32,
    original_width: i32,
    original_height: i32,
    exif: Option<&ExifSummary>,
) -> Result<Vec<u8>, PreviewCacheError> {
    todo!("W4-08b")
}

/// C# `Read(cachePath)`: opens the file with share mode Read|Delete (so a concurrent prune/delete is not blocked),
/// reads it once, [`parse_entry`], then decodes the payload through `codec` into Bgr32 (alpha flag 0) or Pbgra32.
///
/// # Errors
/// See [`PreviewCacheError`]; the caller treats every error as a corrupt entry (delete it, decode the source).
pub fn read(cache_path: &Path, codec: &dyn PreviewPayloadCodec) -> Result<ReadResult, PreviewCacheError> {
    todo!("W4-08b")
}

/// C# `WriteAtomicallyAsync(PixelBuffer, ...)`: validates, then writes `prefix + JPEG` through W4-08a's atomic
/// temp-file-then-rename (random temp name matching `DiskCacheStore.TempFilePattern`; the temp file is removed on any
/// failure or cancellation; `cancel` is checked before the temp file is created and again right before the rename).
/// Blocking: persist workers call it. `pixels` is only read.
///
/// Validation order (C#): blank path, then `!opacity_verified && !is_fully_opaque(pixels)` (InvalidArgument,
/// `"Bitmaps with transparent pixels cannot be stored in the JPEG preview cache."`), then the orientation range.
/// An `Err(PixelBufferError::Disposed)` from W4-16a's `is_fully_opaque` is `InvalidArgument` (C# `ObjectDisposedException`
/// is an `InvalidOperationException`: a cache-entry failure).
///
/// # Errors
/// See [`PreviewCacheError`].
#[allow(clippy::too_many_arguments)]
pub fn write_atomically(
    pixels: &PixelBuffer,
    actual_backend: DecoderBackend,
    orientation: i32,
    original_width: i32,
    original_height: i32,
    cache_path: &Path,
    opacity_verified: bool,
    exif: Option<&ExifSummary>,
    codec: &dyn PreviewPayloadCodec,
    cancel: &CancelToken,
) -> Result<(), PreviewCacheError> {
    todo!("W4-08b")
}

/// C# `WriteAtomicallyAsync(IDecodedImage, IPlatformImageCodec, ...)` (C-02): `platform.to_pixels(image.platform_image())`
/// (the lease is dropped afterwards), then [`write_atomically`] with the image's backend, orientation, original size and
/// EXIF; `opacity_verified = false`.
///
/// # Errors
/// See [`PreviewCacheError`]; a codec failure is `InvalidArgument`.
pub fn write_decoded_image(
    image: &DecodedImage,
    platform: &dyn PlatformImageCodec,
    cache_path: &Path,
    codec: &dyn PreviewPayloadCodec,
    cancel: &CancelToken,
) -> Result<(), PreviewCacheError> {
    todo!("W4-08b")
}

/// C# `ReadAsDecodedImage`: [`read`] then [`to_decoded_image`].
///
/// # Errors
/// See [`PreviewCacheError`].
pub fn read_as_decoded_image(
    cache_path: &Path,
    platform: &dyn PlatformImageCodec,
    codec: &dyn PreviewPayloadCodec,
) -> Result<DecodedImage, PreviewCacheError> {
    todo!("W4-08b")
}

/// C# `ToDecodedImage`: `platform.image_from_pixels(entry.pixels)` (takes ownership; on failure the buffer is gone with
/// the call), then `DecodedImage` with `estimated_bytes = width * height * 4` (i64 maths), `downscaled = true`,
/// orientation, actual backend, original size and EXIF of the entry.
///
/// # Errors
/// A platform-codec or `DecodedImage::new` failure is `InvalidArgument` (still a cache-entry failure).
pub fn to_decoded_image(
    entry: ReadResult,
    platform: &dyn PlatformImageCodec,
) -> Result<DecodedImage, PreviewCacheError> {
    todo!("W4-08b")
}
