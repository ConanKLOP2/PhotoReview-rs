//! Port of `PhotoReview.Imaging/Metadata/ExifParser.cs` (C# baseline d92d6aae). Target: `crates/pr-imaging/src/exif_parser.rs`.
//!
//! Small bounded EXIF reader over JPEG bytes a decoder already holds. Walks the marker segments up to SOS, takes the
//! first APP1 `Exif\0\0` block and reads IFD0 plus the Exif sub-IFD only (one level, no recursion). Fuzz-safe by
//! construction: every offset and length is bounds-checked against the block, entry counts are capped at
//! [`MAX_IFD_ENTRIES`], and nothing here panics or returns an error: bad EXIF yields `None` (or fewer fields).
//!
//! Depends on W4-03a (`crate::tiff`) and W4-04a (`crate::exif_summary`); contracts/drafts-w4/W4-03b/contract.md
//! lists the exact signatures assumed from them.
//! Pinned by `tests/golden/exif-parser.json` (see contract.md "Golden package").
#![allow(unused_variables, dead_code)] // D11: removed by the porter together with the last todo!()

use crate::exif_summary::{ExifRational, ExifSummary};

/// IFD0 `Make`.
pub const TAG_MAKE: u16 = 0x010F;
/// IFD0 `Model`.
pub const TAG_MODEL: u16 = 0x0110;
/// IFD0 `DateTime`.
pub const TAG_DATE_TIME: u16 = 0x0132;
/// IFD0 `Orientation`.
pub const TAG_ORIENTATION: u16 = 0x0112;
/// IFD0 pointer to the Exif sub-IFD.
pub const TAG_EXIF_IFD: u16 = 0x8769;
/// Exif IFD `ExposureTime`.
pub const TAG_EXPOSURE_TIME: u16 = 0x829A;
/// Exif IFD `FNumber`.
pub const TAG_F_NUMBER: u16 = 0x829D;
/// Exif IFD `ISOSpeedRatings`.
pub const TAG_ISO: u16 = 0x8827;
/// Exif IFD `DateTimeOriginal`.
pub const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
/// Exif IFD `DateTimeDigitized`.
pub const TAG_DATE_TIME_DIGITIZED: u16 = 0x9004;
/// Exif IFD `FocalLength`.
pub const TAG_FOCAL_LENGTH: u16 = 0x920A;
/// Exif IFD `LensModel`.
pub const TAG_LENS_MODEL: u16 = 0xA434;

/// More entries than any real IFD has; a larger count means a corrupt block (only this many are read).
pub const MAX_IFD_ENTRIES: usize = 256;

/// Reads the EXIF summary of a JPEG; `None` for a non-JPEG, no/corrupt EXIF, or no usable field.
/// C#: `TryParseJpeg` (strict magic 42: an ORF-magic block inside a JPEG is rejected).
#[must_use]
pub fn try_parse_jpeg(jpeg: &[u8]) -> Option<ExifSummary> {
    todo!()
}

/// Reads the EXIF summary from an already-located Exif TIFF block (the bytes after `Exif\0\0`). `None` for an
/// empty slice, no/corrupt EXIF, or no usable field. Accepts the Olympus ORF magics 0x4F52 and 0x5352.
/// C#: `TryParseTiffBlock(ReadOnlySpan<byte>)`.
#[must_use]
pub fn try_parse_tiff_block(tiff_block: &[u8]) -> Option<ExifSummary> {
    todo!()
}

/// Like [`try_parse_tiff_block`], but with `ifd_is_exif` the block's IFD0 itself is read as the Exif IFD (exposure,
/// f-number, ISO, dates, focal length, lens) and no 0x8769 pointer is followed; Make/Model/DateTime are not read
/// then (Canon CR3 CMT2 box). C#: `TryParseTiffBlock(ReadOnlySpan<byte>, bool ifdIsExif)`.
#[must_use]
pub fn try_parse_tiff_block_with(tiff_block: &[u8], ifd_is_exif: bool) -> Option<ExifSummary> {
    todo!()
}

/// Orientation (tag 0x0112, IFD0) of a JPEG's first Exif APP1 segment: `Some(1..=8)` or `None`.
/// C#: `TryReadOrientationFromJpeg` (returns `int?`; the value is always 1 to 8).
#[must_use]
pub fn try_read_orientation_from_jpeg(jpeg: &[u8]) -> Option<u8> {
    todo!()
}

/// Orientation (tag 0x0112) from IFD0 of an Exif TIFF block (strict magic 42). `None` when the block is
/// empty/corrupt, has no Orientation, the FIRST orientation entry is unreadable, or the value is outside 1 to 8.
/// C#: `TryReadOrientation`.
#[must_use]
pub fn try_read_orientation(tiff_block: &[u8]) -> Option<u8> {
    todo!()
}

/// The TIFF structure inside the first APP1 Exif segment of a JPEG, or an empty slice.
/// C#: `FindExifTiffBlock` (internal; `pub` here because the TurboJPEG decoder of Wave 5 runs its own combined
/// header scan but its tests and the RAW readers compare against this walk).
#[must_use]
pub fn find_exif_tiff_block(jpeg: &[u8]) -> &[u8] {
    todo!()
}

/// Parses a TIFF-structured EXIF block. C#: `TryParseTiff` (internal). The caller (`try_parse_*`) already turned an
/// empty block into `None`.
#[must_use]
pub(crate) fn try_parse_tiff(
    tiff_block: &[u8],
    allow_olympus_raw_magic: bool,
    ifd_is_exif: bool,
) -> Option<ExifSummary> {
    todo!()
}

/// Raw tag values collected from IFD0 and the Exif IFD before sanitising (C# `RawValues`). First readable value
/// wins per field (`??=`): a tag whose value reads as `None` (empty text, wrong type) does not block a later duplicate.
#[derive(Debug, Default)]
struct RawValues {
    make: Option<String>,
    model: Option<String>,
    date_time: Option<String>,
    date_original: Option<String>,
    date_digitized: Option<String>,
    lens: Option<String>,
    iso: Option<i64>,
    focal_length: Option<ExifRational>,
    f_number: Option<ExifRational>,
    exposure_time: Option<ExifRational>,
}

/// Reads one IFD into `values`; returns the Exif sub-IFD offset when this is IFD0 (`is_exif_ifd == false`) and it
/// has a pointer in `8..=u32::MAX` (the first in-range pointer wins). C#: `ReadIfd`.
fn read_ifd(
    tiff_block: &[u8],
    ifd_offset: u32,
    little: bool,
    values: &mut RawValues,
    is_exif_ifd: bool,
) -> Option<u32> {
    todo!()
}
