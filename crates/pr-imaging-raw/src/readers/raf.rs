//! Fujifilm RAF container reader (port of `PhotoReview.Imaging.Raw.Raf.RafContainerReader`, W5-09, C# baseline d92d6aae).
//!
//! RAF is NOT a TIFF container. Fixed big-endian pointer table: magic `"FUJIFILMCCD-RAW "` (16 bytes), embedded JPEG
//! offset/length at 84/88, CFA header offset/length at 92/96. The embedded JPEG carries its own EXIF (orientation); the CFA
//! header is a record list (`tag u16, size u16, data`, big-endian) whose records 0x0100 (full readout), 0x0111 (cropped size)
//! and 0x0121 (`RawImageSize`) give the sensor size as `(height, width)`. Never panics on a hostile file: a bad JPEG range
//! only drops the preview, a bad CFA header only leaves the size at 0x0 (the container is kept so the no-preview LibRaw
//! fallback can still decode); only a source shorter than 92 bytes, a wrong magic, or a failing header-source read is an `Err`.

use pr_core::sync::CancelToken;
use pr_core::text::eq_ignore_case;
use pr_imaging::exif_parser::try_read_orientation_from_jpeg;

use crate::contracts::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerReader, RawHeaderError,
    RawHeaderSource,
};
use crate::formats::limits;
use crate::{RawContainerInfo, RawFormat};

/// Big-endian `u32` at `at`. The callers pass slices of exactly the length they asked the source for, so a short slice
/// (impossible, the source checks its own bounds) reads as 0 instead of panicking.
fn be_u32_at(bytes: &[u8], at: usize) -> u32 {
    bytes
        .get(at..at + 4)
        .and_then(|raw| <[u8; 4]>::try_from(raw).ok())
        .map_or(0, u32::from_be_bytes)
}

/// Big-endian `u16` at `at`; a missing byte reads as 0 (callers bounds-check first).
fn be_u16_at(bytes: &[u8], at: usize) -> u16 {
    bytes
        .get(at..at + 2)
        .and_then(|raw| <[u8; 2]>::try_from(raw).ok())
        .map_or(0, u16::from_be_bytes)
}

/// C# `Magic`: ASCII `"FUJIFILMCCD-RAW "` (16 bytes, trailing space included).
pub const MAGIC: [u8; 16] = *b"FUJIFILMCCD-RAW ";

/// C# `PointerTableEnd`: end of the fixed pointer table (JPEG offset/length at 84/88, CFA header offset/length at 92/96).
/// An embedded JPEG may not start inside it.
pub const POINTER_TABLE_END: i64 = 100;

/// Bytes `read` needs before it looks at anything (magic ... CFA offset field): shorter is "File too short for RAF container."
pub const MIN_CONTAINER_BYTES: i64 = 92;

/// C# `MaxCfaHeaderBytes`: a hostile CFA header length must not force a large read.
pub const MAX_CFA_HEADER_BYTES: i64 = 64 * 1024;

/// C# `MaxCfaRecords`: a hostile record count must not force an unbounded record walk.
pub const MAX_CFA_RECORDS: u32 = 256;

/// CFA record tag: full readout size incl. masked margins (`RawImageFullSize`), data = height u16, width u16.
pub const TAG_FULL_SIZE: u16 = 0x0100;
/// CFA record tag: cropped size (`RawImageCroppedSize`), data = height u16, width u16.
pub const TAG_CROPPED_SIZE: u16 = 0x0111;
/// CFA record tag: `RawImageSize` as LibRaw's `parse_fuji` reads it, data = height u16, width u16.
pub const TAG_IMAGE_SIZE: u16 = 0x0121;

/// A `(width, height)` pair in pixels; `(0, 0)` means "not declared". C# uses `(int Width, int Height)` tuples.
pub type Size = (i32, i32);

/// The three sizes found by the CFA record walk (each `(0, 0)` when absent or invalid; the LAST valid record of a tag wins).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CfaSizes {
    /// Record 0x0100.
    pub full: Size,
    /// Record 0x0111.
    pub cropped: Size,
    /// Record 0x0121, after the `4284 -> 4287` widening.
    pub image_size: Size,
}

/// Stateless reader for `.raf` files (C# `RafContainerReader`, shared across threads by the registry).
#[derive(Debug, Default, Clone, Copy)]
pub struct RafContainerReader;

impl RafContainerReader {
    /// `new RafContainerReader()`.
    pub const fn new() -> Self {
        Self
    }
}

impl RawContainerReader for RafContainerReader {
    /// True when `extension` equals `.raf` (OrdinalIgnoreCase via `pr_core::text::eq_ignore_case`; checked FIRST),
    /// at least 16 bytes are given and the first 16 are [`MAGIC`]. Bytes past 16 are not examined.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        eq_ignore_case(extension, ".raf") && first_64_bytes.get(..16) == Some(&MAGIC[..])
    }

    /// Reads the container through bounded header reads (read pattern, in this order: `read(0, 92)`, then only when the JPEG
    /// range is plausible `read(jpeg_offset, 2)` and `read(jpeg_offset, min(jpeg_length, 128 KiB))`, then ALWAYS
    /// `read(92, 8)` and, when the CFA range is plausible, `read(cfa_offset, min(cfa_length, 64 KiB))`).
    ///
    /// The returned [`RawContainerInfo`] is `format: Raf`, at most one preview (`index 0`, `kind Jpeg`, width/height `0`,
    /// `color_space Unknown`, `header_resolved false`), at most one EXIF block (`ExifBlock::new(jpeg_offset,
    /// min(jpeg_length, 131072), false)`) and `orientation` = EXIF orientation of the embedded JPEG or 1.
    ///
    /// The token is NEVER observed (the C# reader ignores its `CancellationToken`): a cancelled token still reads normally.
    ///
    /// # Errors
    /// `RawHeaderError::InvalidData` with message `"File too short for RAF container."` when the source is shorter than 92
    /// bytes, `"Invalid RAF magic header."` when the first 16 bytes are not [`MAGIC`], and whatever the header source
    /// reports (a source of 92..=99 bytes fails the unconditional `read(92, 8)` as "past end"; a budget exhaustion; an I/O
    /// failure keeps its flag). Never `Canceled`, never a panic.
    fn read(&self, source: &mut dyn RawHeaderSource, _ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        // Port of `RafContainerReader.Read`: the token is never observed (see the doc comment above).
        let length = source.length();
        if length < MIN_CONTAINER_BYTES {
            return Err(RawHeaderError::invalid_data("File too short for RAF container."));
        }

        // Copy the pointer fields out: the slice borrows `source`, and the next read needs it mutably.
        let (jpeg_offset, jpeg_length) = {
            let header = source.read(0, 92)?;
            if header.get(..16) != Some(&MAGIC[..]) {
                return Err(RawHeaderError::invalid_data("Invalid RAF magic header."));
            }
            (i64::from(be_u32_at(header, 84)), i64::from(be_u32_at(header, 88)))
        };

        let mut previews = Vec::new();
        let mut exif_blocks = Vec::new();
        let mut orientation = 1;

        // The C# order: each condition is checked before the SOI read, so that read only happens for a plausible range.
        let range_valid =
            jpeg_offset >= POINTER_TABLE_END && jpeg_length >= 2 && jpeg_offset + jpeg_length <= length && {
                let soi = source.read(jpeg_offset, 2)?;
                soi == [0xFF, 0xD8]
            };
        if range_valid {
            let exif_length = jpeg_length.min(i64::from(limits::DEFAULT_EXIF_BLOCK_BYTES));
            previews.push(EmbeddedPreview::new(
                0,
                jpeg_offset,
                jpeg_length,
                EmbeddedPreviewKind::Jpeg,
                0,
                0,
                PreviewColorSpace::Unknown,
            ));
            exif_blocks.push(ExifBlock::new(jpeg_offset, exif_length, false));
            // `exif_length <= 128 KiB`, so the conversion never fails; `unwrap_or` keeps the path panic-free regardless.
            let exif_count = i32::try_from(exif_length).unwrap_or(limits::DEFAULT_EXIF_BLOCK_BYTES);
            let exif = source.read(jpeg_offset, exif_count)?;
            orientation = try_read_orientation_from_jpeg(exif).map_or(1, i32::from);
        }

        let (sensor_width, sensor_height) = read_cfa_size(source)?;

        Ok(RawContainerInfo {
            format: RawFormat::Raf,
            sensor_width,
            sensor_height,
            orientation,
            previews,
            exif_blocks,
        })
    }
}

/// `ReadCfaSize`: the raw image size `(width, height)` from the CFA header, `(0, 0)` when it is absent or malformed.
/// Reads `read(92, 8)` unconditionally; the CFA bytes are read (and copied out of the borrowed slice) only when
/// `cfa_offset != 0 && cfa_length >= 4 && cfa_offset + cfa_length <= source.length()`.
///
/// # Errors
/// Only a failing header-source read (see `RafContainerReader::read`).
pub fn read_cfa_size(source: &mut dyn RawHeaderSource) -> Result<Size, RawHeaderError> {
    let length = source.length();
    // Unconditional: a 92..=99-byte source fails here with the source's own "past end" error (kept on purpose, golden).
    let (cfa_offset, cfa_length) = {
        let pointers = source.read(92, 8)?;
        (i64::from(be_u32_at(pointers, 0)), i64::from(be_u32_at(pointers, 4)))
    };
    if cfa_offset == 0 || cfa_length < 4 || cfa_offset + cfa_length > length {
        return Ok((0, 0));
    }
    // At most 64 KiB (`MAX_CFA_HEADER_BYTES`), so the conversion cannot fail; `unwrap_or` keeps it panic-free anyway.
    let window = i32::try_from(cfa_length.min(MAX_CFA_HEADER_BYTES)).unwrap_or(0);
    let cfa = source.read(cfa_offset, window)?;
    Ok(choose_sensor_size(&parse_cfa_records(cfa)))
}

/// The pure record walk of `ReadCfaSize` over the bytes actually read (`cfa` = the first `min(cfa_length, 65536)` bytes
/// of the CFA header; needs `cfa.len() >= 4`, else all sizes are `(0, 0)`). Count = big-endian u32 at 0; records start at 4;
/// the walk stops after `min(count, 256)` records, when fewer than 4 bytes remain for a record header, or when a record's
/// data would run past `cfa` (the whole record is dropped). A record is used only when `size >= 4` and the tag is one of the
/// three; `height = u16 at data`, `width = u16 at data + 2`; tag 0x0121 with width 4284 becomes 4287; stored only when
/// both are > 0 (a later valid record of the same tag overwrites an earlier one). The position always advances by
/// `4 + size`, also for unknown tags and for `size < 4`.
pub fn parse_cfa_records(cfa: &[u8]) -> CfaSizes {
    let mut sizes = CfaSizes::default();
    if cfa.len() < 4 {
        return sizes;
    }
    let count = be_u32_at(cfa, 0);
    let mut position: usize = 4;
    for _ in 0..count.min(MAX_CFA_RECORDS) {
        // Record header: tag u16, size u16 (both big-endian); fewer than 4 bytes left ends the walk.
        if position + 4 > cfa.len() {
            break;
        }
        let tag = be_u16_at(cfa, position);
        let size = usize::from(be_u16_at(cfa, position + 2));
        let data = position + 4;
        let Some(end) = data.checked_add(size) else {
            break;
        };
        // A record whose data runs past the window is dropped and ends the walk (earlier records stay).
        if end > cfa.len() {
            break;
        }
        if size >= 4 && matches!(tag, TAG_FULL_SIZE | TAG_CROPPED_SIZE | TAG_IMAGE_SIZE) {
            let height = i32::from(be_u16_at(cfa, data));
            let mut width = i32::from(be_u16_at(cfa, data + 2));
            // LibRaw `parse_fuji`: only the 0x0121 record has the 4284 -> 4287 quirk.
            if tag == TAG_IMAGE_SIZE && width == 4284 {
                width = 4287;
            }
            if width > 0 && height > 0 {
                let size_pair = (width, height);
                match tag {
                    TAG_FULL_SIZE => sizes.full = size_pair,
                    TAG_CROPPED_SIZE => sizes.cropped = size_pair,
                    _ => sizes.image_size = size_pair,
                }
            }
        }
        position = end;
    }
    sizes
}

/// The size choice at the end of `ReadCfaSize`: `cropped` if it "fits", else `image_size` if it fits, else `full`.
/// `fits(s)` = `s.0 > 0 && (full.0 == 0 || (s.0 <= full.0 && s.1 <= full.1))` (a cropped/image size can only shrink the
/// full readout; with no full record any valid size fits).
pub fn choose_sensor_size(sizes: &CfaSizes) -> Size {
    let full = sizes.full;
    let fits = |candidate: Size| candidate.0 > 0 && (full.0 == 0 || (candidate.0 <= full.0 && candidate.1 <= full.1));
    if fits(sizes.cropped) {
        sizes.cropped
    } else if fits(sizes.image_size) {
        sizes.image_size
    } else {
        full
    }
}
