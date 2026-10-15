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

use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::Path;

use pr_core::model::DecoderBackend;
use pr_core::model::serde_ext::LenientEnum;
use pr_core::sync::CancelToken;

use crate::cache::atomic_file::{AtomicWriteError, write_atomic};
use crate::decode_types::{DecodedImage, DecodedImageOptions, DecodedImageView};
use crate::decoder_traits::PlatformImageCodec;
use crate::exif_summary::{ExifSummary, codec};
use crate::pixels::{PixelBuffer, PixelLayout, is_fully_opaque};

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
        !matches!(self, Self::Canceled | Self::OutOfMemory(_))
    }
}

impl fmt::Display for PreviewCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidData(m) | Self::InvalidArgument(m) | Self::ArgumentOutOfRange(m) | Self::OutOfMemory(m) => {
                f.write_str(m)
            }
            Self::Io(e) => fmt::Display::fmt(e, f),
            Self::Canceled => f.write_str("The operation was canceled."),
        }
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
    DecoderBackend::MEMBERS
        .iter()
        .copied()
        .find(|m| m.value() == i32::from(value))
}

/// Validates and splits the bytes of an entry file; no JPEG decoding, no pixel allocation. The check order and the
/// message texts are those of C# `Read` (see the contract, "Validation order").
///
/// # Errors
/// `Io(UnexpectedEof)` when `bytes.len() < 24`; otherwise [`PreviewCacheError::InvalidData`].
pub fn parse_entry(bytes: &[u8]) -> Result<ParsedEntry, PreviewCacheError> {
    let len = bytes.len();
    if len < HEADER_SIZE {
        return Err(PreviewCacheError::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Preview cache entry is shorter than its header.",
        )));
    }
    let invalid = |m: &str| Err(PreviewCacheError::InvalidData(m.to_owned()));
    if bytes[..4] != MAGIC {
        return invalid("Preview cache entry has an invalid header magic.");
    }
    let version = bytes[4];
    if version != CURRENT_VERSION && version != NO_EXIF_VERSION {
        return Err(PreviewCacheError::InvalidData(format!(
            "Preview cache entry is version {version}, expected {CURRENT_VERSION}."
        )));
    }
    let Some(backend) = backend_from_byte(bytes[5]) else {
        return invalid("Preview cache entry has an invalid decoder backend byte.");
    };
    let orientation = bytes[6];
    if !(1..=8).contains(&orientation) {
        return invalid("Preview cache entry has an invalid orientation byte.");
    }
    let has_alpha = bytes[7] != 0;
    let i32_at = |at: usize| i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let (width, height) = (i32_at(8), i32_at(12));
    if width <= 0 || height <= 0 {
        return invalid("Preview cache entry has invalid pixel dimensions.");
    }
    let (original_width, original_height) = (i32_at(16), i32_at(20));
    if original_width <= 0 || original_height <= 0 {
        return invalid("Preview cache entry has invalid original dimensions.");
    }

    let mut exif = None;
    let mut exif_block_size = 0usize;
    if version == CURRENT_VERSION {
        let after_header = len - HEADER_SIZE;
        if after_header < 2 {
            return invalid("Preview cache entry has no payload.");
        }
        let n = usize::from(u16::from_le_bytes([bytes[HEADER_SIZE], bytes[HEADER_SIZE + 1]]));
        if n > MAX_EXIF_BLOCK || n > after_header - 2 {
            return invalid("Preview cache entry has an oversized EXIF block.");
        }
        if n > 0 {
            // Malformed EXIF bytes mean "no EXIF"; the pixels are still good.
            exif = codec::decode(&bytes[HEADER_SIZE + 2..HEADER_SIZE + 2 + n]);
        }
        exif_block_size = 2 + n;
    }

    let payload_offset = HEADER_SIZE + exif_block_size;
    let payload_len = len - payload_offset;
    if payload_len == 0 {
        return invalid("Preview cache entry has no payload.");
    }
    if i32::try_from(payload_len).is_err() {
        return invalid("Preview cache entry payload is implausibly large.");
    }
    if payload_len < 4 || bytes[len - 2] != 0xFF || bytes[len - 1] != 0xD9 {
        return invalid("Preview cache entry payload is truncated (no JPEG end-of-image marker).");
    }

    Ok(ParsedEntry {
        header: PreviewHeader {
            backend,
            orientation,
            has_alpha,
            width,
            height,
            original_width,
            original_height,
        },
        exif,
        payload_offset,
        payload_len,
        file_bytes: i64::try_from(len).unwrap_or(i64::MAX),
    })
}

/// Header (24) + u16 EXIF length + EXIF block of a NEW entry (C# `BuildHeader` + the v7 length/EXIF bytes), always v7 with
/// alpha flag 0. `original_*` `<= 0` fall back to `width`/`height`. The JPEG payload is appended by the caller.
///
/// # Errors
/// [`PreviewCacheError::ArgumentOutOfRange`] when `orientation` is outside 1..=8 (C# message
/// `"EXIF orientation must be 1-8. (Parameter 'orientation')\r\nActual value was {orientation}."`).
pub fn build_entry_prefix(
    backend: DecoderBackend,
    orientation: i32,
    width: i32,
    height: i32,
    original_width: i32,
    original_height: i32,
    exif: Option<&ExifSummary>,
) -> Result<Vec<u8>, PreviewCacheError> {
    if !(1..=8).contains(&orientation) {
        return Err(PreviewCacheError::ArgumentOutOfRange(format!(
            "EXIF orientation must be 1-8. (Parameter 'orientation')\r\nActual value was {orientation}."
        )));
    }
    let header_original_width = if original_width > 0 { original_width } else { width };
    let header_original_height = if original_height > 0 { original_height } else { height };
    let exif_bytes = codec::encode(exif);
    // `encode` never exceeds MAX_ENCODED_LENGTH (1024), so the u16 length cannot overflow.
    let exif_length = u16::try_from(exif_bytes.len()).map_err(|_| {
        PreviewCacheError::InvalidArgument("The EXIF block does not fit the preview cache entry.".to_owned())
    })?;

    let mut prefix = Vec::with_capacity(HEADER_SIZE + 2 + exif_bytes.len());
    prefix.extend_from_slice(&MAGIC);
    prefix.push(CURRENT_VERSION);
    prefix.push(u8::try_from(backend.value()).unwrap_or(0));
    prefix.push(u8::try_from(orientation).unwrap_or(1));
    prefix.push(0); // a JPEG payload never carries alpha
    prefix.extend_from_slice(&width.to_le_bytes());
    prefix.extend_from_slice(&height.to_le_bytes());
    prefix.extend_from_slice(&header_original_width.to_le_bytes());
    prefix.extend_from_slice(&header_original_height.to_le_bytes());
    prefix.extend_from_slice(&exif_length.to_le_bytes());
    prefix.extend_from_slice(&exif_bytes);
    Ok(prefix)
}

/// C# `Read(cachePath)`: opens the file with share mode Read|Delete (so a concurrent prune/delete is not blocked),
/// reads it once, [`parse_entry`], then decodes the payload through `codec` into Bgr32 (alpha flag 0) or Pbgra32.
///
/// # Errors
/// See [`PreviewCacheError`]; the caller treats every error as a corrupt entry (delete it, decode the source).
pub fn read(cache_path: &Path, codec: &dyn PreviewPayloadCodec) -> Result<ReadResult, PreviewCacheError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ (1) | FILE_SHARE_DELETE (4): a pruner can delete the entry while a reader holds it.
        options.share_mode(0x1 | 0x4);
    }
    let mut file = options.open(cache_path)?;
    let mut bytes = Vec::new();
    if let Ok(meta) = file.metadata() {
        bytes.reserve(usize::try_from(meta.len()).unwrap_or(0));
    }
    file.read_to_end(&mut bytes)?;
    drop(file);

    let entry = parse_entry(&bytes)?;
    let header = entry.header;
    let layout = if header.has_alpha {
        PixelLayout::Pbgra32
    } else {
        PixelLayout::Bgr32
    };
    let payload = &bytes[entry.payload_offset..entry.payload_offset + entry.payload_len];
    let pixels = codec.decode_jpeg(payload, layout, header.width, header.height)?;
    Ok(ReadResult {
        pixels,
        actual_backend: header.backend,
        orientation: i32::from(header.orientation),
        file_bytes: entry.file_bytes,
        original_width: header.original_width,
        original_height: header.original_height,
        exif: entry.exif,
    })
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
    let blank = cache_path.as_os_str().is_empty() || cache_path.to_str().is_some_and(|p| p.trim().is_empty());
    if blank {
        return Err(PreviewCacheError::InvalidArgument(
            "The value cannot be an empty string or composed entirely of whitespace. (Parameter 'cachePath')"
                .to_owned(),
        ));
    }
    // IMG-01: JPEG cannot carry alpha. `opacity_verified` skips the scan (the caller already did it).
    if !opacity_verified {
        match is_fully_opaque(pixels) {
            Ok(true) => {}
            Ok(false) => {
                return Err(PreviewCacheError::InvalidArgument(
                    "Bitmaps with transparent pixels cannot be stored in the JPEG preview cache. (Parameter 'pixels')"
                        .to_owned(),
                ));
            }
            Err(e) => return Err(PreviewCacheError::InvalidArgument(e.to_string())),
        }
    }
    let prefix = build_entry_prefix(
        actual_backend,
        orientation,
        pixels.width(),
        pixels.height(),
        original_width,
        original_height,
        exif,
    )?;

    let result = write_atomic(
        cache_path,
        |out: &mut dyn Write| {
            out.write_all(&prefix)?;
            codec.encode_jpeg(pixels, DEFAULT_JPEG_QUALITY, out)
        },
        None,
        cancel,
    );
    match result {
        Ok(()) => Ok(()),
        Err(AtomicWriteError::Canceled) => Err(PreviewCacheError::Canceled),
        Err(AtomicWriteError::Io(e)) if e.kind() == io::ErrorKind::OutOfMemory => {
            Err(PreviewCacheError::OutOfMemory(e.to_string()))
        }
        Err(AtomicWriteError::Io(e)) => Err(PreviewCacheError::Io(e)),
    }
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
    let lease = platform
        .to_pixels(image.platform_image())
        .map_err(|e| PreviewCacheError::InvalidArgument(e.to_string()))?;
    let Some(pixels) = lease.pixels() else {
        return Err(PreviewCacheError::InvalidArgument(
            "The platform codec returned no pixels.".to_owned(),
        ));
    };
    write_atomically(
        pixels,
        image.actual_backend(),
        image.orientation(),
        image.original_width(),
        image.original_height(),
        cache_path,
        false,
        image.exif(),
        codec,
        cancel,
    )
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
    to_decoded_image(read(cache_path, codec)?, platform)
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
    let (width, height) = (entry.pixels.width(), entry.pixels.height());
    let platform_image = platform
        .image_from_pixels(entry.pixels)
        .map_err(|e| PreviewCacheError::InvalidArgument(e.to_string()))?;
    let options = DecodedImageOptions {
        downscaled: true,
        orientation: entry.orientation,
        actual_backend: entry.actual_backend,
        original_width: entry.original_width,
        original_height: entry.original_height,
        exif: entry.exif,
        is_degraded_fallback: false,
    };
    DecodedImage::new(
        platform_image,
        width,
        height,
        i64::from(width) * i64::from(height) * 4,
        options,
    )
    .map_err(|e| PreviewCacheError::InvalidArgument(e.to_string()))
}
