//! Fujifilm RAF container reader (port of `PhotoReview.Imaging.Raw.Raf.RafContainerReader`, W5-09, C# baseline d92d6aae).
//!
//! RAF is NOT a TIFF container. Fixed big-endian pointer table: magic `"FUJIFILMCCD-RAW "` (16 bytes), embedded JPEG
//! offset/length at 84/88, CFA header offset/length at 92/96. The embedded JPEG carries its own EXIF (orientation); the CFA
//! header is a record list (`tag u16, size u16, data`, big-endian) whose records 0x0100 (full readout), 0x0111 (cropped size)
//! and 0x0121 (`RawImageSize`) give the sensor size as `(height, width)`. Never panics on a hostile file: a bad JPEG range
//! only drops the preview, a bad CFA header only leaves the size at 0x0 (the container is kept so the no-preview LibRaw
//! fallback can still decode); only a source shorter than 92 bytes, a wrong magic, or a failing header-source read is an `Err`.

#![allow(unused_variables, unused_imports, dead_code)] // D11: removed together with the last `todo!()`.

use pr_core::sync::CancelToken;

use crate::contracts::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerReader, RawHeaderError,
    RawHeaderSource,
};
use crate::{RawContainerInfo, RawFormat};

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
        todo!()
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
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        todo!()
    }
}

/// `ReadCfaSize`: the raw image size `(width, height)` from the CFA header, `(0, 0)` when it is absent or malformed.
/// Reads `read(92, 8)` unconditionally; the CFA bytes are read (and copied out of the borrowed slice) only when
/// `cfa_offset != 0 && cfa_length >= 4 && cfa_offset + cfa_length <= source.length()`.
///
/// # Errors
/// Only a failing header-source read (see `RafContainerReader::read`).
pub fn read_cfa_size(source: &mut dyn RawHeaderSource) -> Result<Size, RawHeaderError> {
    todo!()
}

/// The pure record walk of `ReadCfaSize` over the bytes actually read (`cfa` = the first `min(cfa_length, 65536)` bytes
/// of the CFA header; needs `cfa.len() >= 4`, else all sizes are `(0, 0)`). Count = big-endian u32 at 0; records start at 4;
/// the walk stops after `min(count, 256)` records, when fewer than 4 bytes remain for a record header, or when a record's
/// data would run past `cfa` (the whole record is dropped). A record is used only when `size >= 4` and the tag is one of the
/// three; `height = u16 at data`, `width = u16 at data + 2`; tag 0x0121 with width 4284 becomes 4287; stored only when
/// both are > 0 (a later valid record of the same tag overwrites an earlier one). The position always advances by
/// `4 + size`, also for unknown tags and for `size < 4`.
pub fn parse_cfa_records(cfa: &[u8]) -> CfaSizes {
    todo!()
}

/// The size choice at the end of `ReadCfaSize`: `cropped` if it "fits", else `image_size` if it fits, else `full`.
/// `fits(s)` = `s.0 > 0 && (full.0 == 0 || (s.0 <= full.0 && s.1 <= full.1))` (a cropped/image size can only shrink the
/// full readout; with no full record any valid size fits).
pub fn choose_sensor_size(sizes: &CfaSizes) -> Size {
    todo!()
}
