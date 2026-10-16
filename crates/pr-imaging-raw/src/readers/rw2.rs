//! Panasonic RW2 container reader (port of `PhotoReview.Imaging.Raw/Tiff/Rw2ContainerReader.cs`, W5-08).
//!
//! Magic `II` + `0x55 0x00` (little-endian only; the TIFF magic field is 0x0055 instead of 42). The embedded JPEG preview is
//! tag 0x002E (`JpgFromRaw`) of IFD0: unlike every other TIFF tag the entry's *offset* field is the JPEG's file offset and its
//! *count* field is the JPEG's length in bytes (the type field is ignored). EXIF lives inside that JPEG (an `ExifBlock` with
//! `is_tiff_header = false`); the Orientation is IFD0 tag 0x0112 when it is 1 to 8, else the Orientation of the JPEG's own EXIF.
//! The image size comes from the border tags 4 to 7 (right - left, bottom - top), else from the sensor size tags 2 and 3.
//!
//! Reads only through the bounded [`RawHeaderSource`]: never the whole file, never past `MAX_HEADER_BYTES`. A hostile file
//! is an `Err` (invalid data) or a result with fewer fields; nothing here may panic, overflow or loop (INV-12 spirit: a RAW
//! container error never falls back to another decoder).
use pr_core::sync::CancelToken;
use pr_core::text::eq_ignore_case;
use pr_imaging::exif_parser::try_read_orientation_from_jpeg;
use pr_imaging::tiff::try_read_header;

use crate::contracts::RawHeaderError;
use crate::contracts::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerReader, RawHeaderSource,
};
use crate::formats::limits::DEFAULT_EXIF_BLOCK_BYTES;
use crate::formats::{RawContainerInfo, RawFormat};
use crate::tiff_nav::{is_range_in_file, read_ifd_entries, read_tag_value, try_get_entry};

/// File extension (compared with `OrdinalIgnoreCase`) this reader accepts.
pub const RW2_EXTENSION: &str = ".rw2";

/// The u16 at offset 2 of an RW2 header (`0x0055`; the bytes are `55 00` in the little-endian file).
pub const RW2_MAGIC: u16 = 0x0055;

/// Smallest file this reader opens (header probe size); shorter is `invalid data` "File too short for RW2 container.".
pub const MIN_FILE_BYTES: i64 = 16;

/// Exact message of the "too short" error (the golden `error: "too-short"` pins it).
pub const TOO_SHORT_MESSAGE: &str = "File too short for RW2 container.";

/// Exact message of the "bad header" error (the golden `error: "invalid-header"` pins it).
pub const INVALID_HEADER_MESSAGE: &str = "Invalid RW2 header.";

/// IFD0 tag: sensor width including masked margins (C# `sensorWidthTag`).
pub const TAG_SENSOR_WIDTH: u16 = 0x0002;
/// IFD0 tag: sensor height including masked margins.
pub const TAG_SENSOR_HEIGHT: u16 = 0x0003;
/// IFD0 tag: top border (first image row).
pub const TAG_TOP_BORDER: u16 = 0x0004;
/// IFD0 tag: left border (first image column).
pub const TAG_LEFT_BORDER: u16 = 0x0005;
/// IFD0 tag: bottom border (one past the last image row).
pub const TAG_BOTTOM_BORDER: u16 = 0x0006;
/// IFD0 tag: right border (one past the last image column).
pub const TAG_RIGHT_BORDER: u16 = 0x0007;
/// IFD0 tag: Orientation (EXIF 0x0112).
pub const TAG_ORIENTATION: u16 = 0x0112;
/// IFD0 tag: `JpgFromRaw` (count = JPEG length, offset field = JPEG file offset).
pub const TAG_JPG_FROM_RAW: u16 = 0x002E;

/// `Rw2ContainerReader`: stateless, so it is `Copy` and `Default`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rw2ContainerReader;

impl Rw2ContainerReader {
    /// `new Rw2ContainerReader()`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// One image extent (width or height) of the C# `Read`: `end - start` when both borders are present with
/// `start >= 0`, `end > start` and `end <= i32::MAX`; else the size tag when `0 < tag <= i32::MAX`; else `0`.
///
/// `start`/`end`/`size_tag` are the `ReadTagValue` results (`long?`: negative values are possible for SSHORT/SLONG fields).
/// Pure; exposed for exact-value tests of the boundaries (equal borders, `i32::MAX`, `i32::MAX + 1`, negative left).
#[must_use]
pub fn image_extent(start: Option<i64>, end: Option<i64>, size_tag: Option<i64>) -> i32 {
    let max = i64::from(i32::MAX);
    if let (Some(s), Some(e)) = (start, end) {
        if s >= 0 && e > s && e <= max {
            return i32::try_from(e - s).unwrap_or(0);
        }
    }
    match size_tag {
        Some(t) if t > 0 && t <= max => i32::try_from(t).unwrap_or(0),
        _ => 0,
    }
}

/// IFD0 Orientation as the reader accepts it: `Some(1..=8)` for a value inside that range, else `None` (the JPEG EXIF fallback
/// then applies). `value` is the `ReadTagValue` result for tag 0x0112 (negative values are possible).
#[must_use]
pub fn ifd0_orientation(value: Option<i64>) -> Option<i32> {
    match value {
        Some(v) if (1..=8).contains(&v) => i32::try_from(v).ok(),
        _ => None,
    }
}

/// How many bytes of the preview JPEG the EXIF block declares: `min(jpeg_length, DEFAULT_EXIF_BLOCK_BYTES)`.
#[must_use]
pub fn exif_block_length(jpeg_length: i64) -> i64 {
    jpeg_length.min(i64::from(DEFAULT_EXIF_BLOCK_BYTES))
}

/// Bytes to read from the start of the preview for the orientation fallback:
/// `min(min(jpeg_length, DEFAULT_EXIF_BLOCK_BYTES), file_length - jpeg_offset)` (C# `exifLength`; `<= 0` means "do not read").
#[must_use]
pub fn exif_read_length(jpeg_length: i64, jpeg_offset: i64, file_length: i64) -> i64 {
    exif_block_length(jpeg_length).min(file_length.saturating_sub(jpeg_offset))
}

impl RawContainerReader for Rw2ContainerReader {
    /// C# `CanRead`: extension `.rw2` (`OrdinalIgnoreCase`), at least 4 bytes, bytes `49 49 55 00`. Pure, never panics.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        eq_ignore_case(extension, RW2_EXTENSION) && first_64_bytes.starts_with(&[0x49, 0x49, 0x55, 0x00])
    }

    /// C# `Read`. Errors are `RawHeaderError` invalid-data values: `source.length() < 16` => [`TOO_SHORT_MESSAGE`]; header not
    /// `II` + magic 0x55 => [`INVALID_HEADER_MESSAGE`]; any error of `source.read` (range, budget, I/O) propagates unchanged
    /// (never swallowed, never turned into a smaller result). `token` is accepted for the trait and never observed (as in C#).
    ///
    /// Read order (pinned by the golden `bytes_read`): `read(0, 16)`; IFD0 entries; the tag values (2, 3, 4, 5, 6, 7, 0x0112)
    /// as `read_tag_value` issues them; then, only when the JPEG entry passes, `read(offset, 2)` and, only when IFD0 has no
    /// usable orientation, `read(offset, exif_read_length)`.
    fn read(&self, source: &mut dyn RawHeaderSource, token: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        let _ = token;
        let file_length = source.length();
        if file_length < MIN_FILE_BYTES {
            return Err(RawHeaderError::invalid_data(TOO_SHORT_MESSAGE));
        }

        let header = try_read_header(source.read(0, 16)?);
        let (little_endian, ifd0_offset) = match header {
            Some(h) if h.little_endian && h.magic == RW2_MAGIC => (true, h.ifd0_offset),
            _ => return Err(RawHeaderError::invalid_data(INVALID_HEADER_MESSAGE)),
        };

        let entries = read_ifd_entries(source, i64::from(ifd0_offset), little_endian)?.entries;

        let sensor_width_tag = read_tag_value(source, &entries, TAG_SENSOR_WIDTH, little_endian)?;
        let sensor_height_tag = read_tag_value(source, &entries, TAG_SENSOR_HEIGHT, little_endian)?;
        let top = read_tag_value(source, &entries, TAG_TOP_BORDER, little_endian)?;
        let left = read_tag_value(source, &entries, TAG_LEFT_BORDER, little_endian)?;
        let bottom = read_tag_value(source, &entries, TAG_BOTTOM_BORDER, little_endian)?;
        let right = read_tag_value(source, &entries, TAG_RIGHT_BORDER, little_endian)?;
        let ifd0_orient = ifd0_orientation(read_tag_value(source, &entries, TAG_ORIENTATION, little_endian)?);

        let sensor_width = image_extent(left, right, sensor_width_tag);
        let sensor_height = image_extent(top, bottom, sensor_height_tag);

        let mut previews = Vec::new();
        let mut exif_blocks = Vec::new();
        let mut jpeg_orientation: Option<i32> = None;

        if let Some(entry) = try_get_entry(&entries, TAG_JPG_FROM_RAW) {
            if entry.count > 4 {
                let offset = i64::from(entry.value_or_offset);
                let length = i64::from(entry.count);
                if is_range_in_file(offset, length, file_length) && source.read(offset, 2)? == [0xFF, 0xD8] {
                    previews.push(EmbeddedPreview::new(
                        0,
                        offset,
                        length,
                        EmbeddedPreviewKind::Jpeg,
                        0,
                        0,
                        PreviewColorSpace::Unknown,
                    ));
                    exif_blocks.push(ExifBlock::new(offset, exif_block_length(length), false));
                    if ifd0_orient.is_none() {
                        let n = exif_read_length(length, offset, file_length);
                        if n > 0 {
                            if let Ok(count) = i32::try_from(n) {
                                jpeg_orientation =
                                    try_read_orientation_from_jpeg(source.read(offset, count)?).map(i32::from);
                            }
                        }
                    }
                }
            }
        }

        Ok(RawContainerInfo {
            format: RawFormat::Rw2,
            sensor_width,
            sensor_height,
            orientation: ifd0_orient.or(jpeg_orientation).unwrap_or(1),
            previews,
            exif_blocks,
        })
    }
}
