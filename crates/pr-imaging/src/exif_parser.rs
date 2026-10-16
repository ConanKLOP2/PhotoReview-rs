//! Port of `PhotoReview.Imaging/Metadata/ExifParser.cs` (C# baseline d92d6aae). Target: `crates/pr-imaging/src/exif_parser.rs`.
//!
//! Small bounded EXIF reader over JPEG bytes a decoder already holds. Walks the marker segments up to SOS, takes the
//! first APP1 `Exif\0\0` block and reads IFD0 plus the Exif sub-IFD only (one level, no recursion). Fuzz-safe by
//! construction: every offset and length is bounds-checked against the block, entry counts are capped at
//! [`MAX_IFD_ENTRIES`], and nothing here panics or returns an error: bad EXIF yields `None` (or fewer fields).
//!
//! Depends on W4-03a (`crate::tiff`) and W4-04a (`crate::exif_summary`).
//! Pinned by `tests/golden/exif-parser.json`.

use crate::exif_summary::{ExifRational, ExifSummary, RawExifTags};
use crate::tiff;

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
    try_parse_guarded(find_exif_tiff_block(jpeg), false, false)
}

/// Reads the EXIF summary from an already-located Exif TIFF block (the bytes after `Exif\0\0`). `None` for an
/// empty slice, no/corrupt EXIF, or no usable field. Accepts the Olympus ORF magics 0x4F52 and 0x5352.
/// C#: `TryParseTiffBlock(ReadOnlySpan<byte>)`.
#[must_use]
pub fn try_parse_tiff_block(tiff_block: &[u8]) -> Option<ExifSummary> {
    try_parse_guarded(tiff_block, true, false)
}

/// Like [`try_parse_tiff_block`], but with `ifd_is_exif` the block's IFD0 itself is read as the Exif IFD (exposure,
/// f-number, ISO, dates, focal length, lens) and no 0x8769 pointer is followed; Make/Model/DateTime are not read
/// then (Canon CR3 CMT2 box). C#: `TryParseTiffBlock(ReadOnlySpan<byte>, bool ifdIsExif)`.
#[must_use]
pub fn try_parse_tiff_block_with(tiff_block: &[u8], ifd_is_exif: bool) -> Option<ExifSummary> {
    try_parse_guarded(tiff_block, true, ifd_is_exif)
}

/// C# `TryParseGuarded`: an empty block is `None`. The C# `catch` has no twin: nothing below can panic.
fn try_parse_guarded(tiff_block: &[u8], allow_olympus_raw_magic: bool, ifd_is_exif: bool) -> Option<ExifSummary> {
    if tiff_block.is_empty() {
        return None;
    }
    try_parse_tiff(tiff_block, allow_olympus_raw_magic, ifd_is_exif)
}

/// Orientation (tag 0x0112, IFD0) of a JPEG's first Exif APP1 segment: `Some(1..=8)` or `None`.
/// C#: `TryReadOrientationFromJpeg` (returns `int?`; the value is always 1 to 8).
#[must_use]
pub fn try_read_orientation_from_jpeg(jpeg: &[u8]) -> Option<u8> {
    try_read_orientation(find_exif_tiff_block(jpeg))
}

/// Byte order from the first two bytes: `II` little, `MM` big; one matching letter is not enough.
fn byte_order(block: &[u8]) -> Option<bool> {
    match (block.first(), block.get(1)) {
        (Some(b'I'), Some(b'I')) => Some(true),
        (Some(b'M'), Some(b'M')) => Some(false),
        _ => None,
    }
}

/// Validates the IFD offset and returns `(start, entry count)`, the count capped by [`MAX_IFD_ENTRIES`] and by the
/// entries that fit in the block. `None` when the offset is inside the header or leaves no room for the count.
fn ifd_bounds(block: &[u8], ifd_offset: u32, little: bool) -> Option<(usize, usize)> {
    let len = block.len();
    // Compared as u64: the offset is untrusted and never cast before the check.
    if ifd_offset < 8 || u64::from(ifd_offset) > (len as u64).saturating_sub(2) {
        return None;
    }
    let start = usize::try_from(ifd_offset).ok()?;
    let declared = usize::from(tiff::read_u16(block, i32::try_from(start).ok()?, little));
    let available = len.checked_sub(start)?.checked_sub(2)? / 12;
    Some((start, declared.min(MAX_IFD_ENTRIES).min(available)))
}

/// Orientation (tag 0x0112) from IFD0 of an Exif TIFF block (strict magic 42). `None` when the block is
/// empty/corrupt, has no Orientation, the FIRST orientation entry is unreadable, or the value is outside 1 to 8.
/// C#: `TryReadOrientation`.
#[must_use]
pub fn try_read_orientation(tiff_block: &[u8]) -> Option<u8> {
    if tiff_block.len() < 8 {
        return None;
    }
    let little = byte_order(tiff_block)?;
    if tiff::read_u16(tiff_block, 2, little) != 42 {
        return None;
    }
    let ifd0 = tiff::read_u32(tiff_block, 4, little);
    let (start, count) = ifd_bounds(tiff_block, ifd0, little)?;
    for i in 0..count {
        let entry = i32::try_from(start + 2 + i * 12).ok()?;
        if tiff::read_u16(tiff_block, entry, little) != TAG_ORIENTATION {
            continue;
        }
        let field_type = tiff::read_u16(tiff_block, entry.saturating_add(2), little);
        let n = tiff::read_u32(tiff_block, entry.saturating_add(4), little);
        // The FIRST orientation entry decides: unreadable span => None, later duplicates are not consulted.
        let value = tiff::try_get_value_span(tiff_block, entry, field_type, n, little)?;
        return match tiff::read_unsigned(value, field_type, little) {
            Some(v @ 1..=8) => u8::try_from(v).ok(),
            _ => None,
        };
    }
    None
}

/// The TIFF structure inside the first APP1 Exif segment of a JPEG, or an empty slice.
/// C#: `FindExifTiffBlock` (internal; `pub` here because the TurboJPEG decoder of Wave 5 runs its own combined
/// header scan but its tests and the RAW readers compare against this walk).
#[must_use]
pub fn find_exif_tiff_block(jpeg: &[u8]) -> &[u8] {
    const EMPTY: &[u8] = &[];
    if jpeg.len() < 4 || jpeg.first() != Some(&0xFF) || jpeg.get(1) != Some(&0xD8) {
        return EMPTY;
    }
    let mut offset: usize = 2;
    while offset.checked_add(4).is_some_and(|end| end <= jpeg.len()) {
        if jpeg.get(offset) != Some(&0xFF) {
            return EMPTY;
        }
        let Some(&marker) = jpeg.get(offset + 1) else {
            return EMPTY;
        };
        match marker {
            0xFF => {
                // Fill byte.
                offset += 1;
                continue;
            }
            // 0xFF00 is a stuffed byte, not a marker: the header area is corrupt.
            0x00 => return EMPTY,
            // Standalone markers: SOI, TEM, RST0..RST7.
            0xD8 | 0x01 | 0xD0..=0xD7 => {
                offset += 2;
                continue;
            }
            // Image data / end: EXIF must come before.
            0xDA | 0xD9 => return EMPTY,
            _ => {}
        }
        let length = match (jpeg.get(offset + 2), jpeg.get(offset + 3)) {
            (Some(&hi), Some(&lo)) => (usize::from(hi) << 8) | usize::from(lo),
            _ => return EMPTY,
        };
        let Some(segment_end) = offset.checked_add(2).and_then(|v| v.checked_add(length)) else {
            return EMPTY;
        };
        if length < 2 || segment_end > jpeg.len() {
            return EMPTY;
        }
        let Some(payload) = jpeg.get(offset + 4..segment_end) else {
            return EMPTY;
        };
        // The first Exif-headed APP1 wins even when its TIFF part is empty/garbage.
        if marker == 0xE1 && payload.len() >= 6 && payload.get(..6) == Some(b"Exif\0\0".as_slice()) {
            return payload.get(6..).unwrap_or(EMPTY);
        }
        offset = segment_end;
    }
    EMPTY
}

/// Parses a TIFF-structured EXIF block. C#: `TryParseTiff` (internal). The caller (`try_parse_*`) already turned an
/// empty block into `None`.
#[must_use]
pub(crate) fn try_parse_tiff(
    tiff_block: &[u8],
    allow_olympus_raw_magic: bool,
    ifd_is_exif: bool,
) -> Option<ExifSummary> {
    if tiff_block.len() < 8 {
        return None;
    }
    let little = byte_order(tiff_block)?;
    let magic = tiff::read_u16(tiff_block, 2, little);
    // Olympus ORF replaces 42 with 'RO' (0x4F52) or 'SR' (0x5352) but is otherwise plain TIFF.
    if magic != 42 && !(allow_olympus_raw_magic && matches!(magic, 0x4F52 | 0x5352)) {
        return None;
    }

    let mut values = RawValues::default();
    let ifd0 = tiff::read_u32(tiff_block, 4, little);
    if ifd_is_exif {
        read_ifd(tiff_block, ifd0, little, &mut values, true);
    } else if let Some(exif_offset) = read_ifd(tiff_block, ifd0, little, &mut values, false) {
        // Only one level is followed (no recursion), so a hostile pointer back to IFD0 cannot loop.
        read_ifd(tiff_block, exif_offset, little, &mut values, true);
    }

    // The date is chosen on raw text before parsing: a present-but-invalid original hides a valid DateTime.
    let date = values
        .date_original
        .as_deref()
        .or(values.date_digitized.as_deref())
        .or(values.date_time.as_deref());
    ExifSummary::create(&RawExifTags {
        date_taken: date,
        make: values.make.as_deref(),
        model: values.model.as_deref(),
        lens: values.lens.as_deref(),
        iso: values.iso,
        focal_length: values.focal_length,
        f_number: values.f_number,
        exposure_time: values.exposure_time,
    })
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

/// C# `??=`: set only while still `None` and the new read is `Some`.
fn fill<T>(slot: &mut Option<T>, value: Option<T>) {
    if slot.is_none() {
        *slot = value;
    }
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
    let (start, count) = ifd_bounds(tiff_block, ifd_offset, little)?;
    let mut exif_pointer: Option<u32> = None;
    for i in 0..count {
        let Ok(entry) = i32::try_from(start + 2 + i * 12) else {
            break;
        };
        let tag = tiff::read_u16(tiff_block, entry, little);
        let field_type = tiff::read_u16(tiff_block, entry.saturating_add(2), little);
        let n = tiff::read_u32(tiff_block, entry.saturating_add(4), little);
        let Some(value) = tiff::try_get_value_span(tiff_block, entry, field_type, n, little) else {
            continue;
        };
        if is_exif_ifd {
            match tag {
                TAG_EXPOSURE_TIME => fill(
                    &mut values.exposure_time,
                    tiff::read_rational(value, field_type, little),
                ),
                TAG_F_NUMBER => fill(&mut values.f_number, tiff::read_rational(value, field_type, little)),
                TAG_ISO => fill(&mut values.iso, tiff::read_unsigned(value, field_type, little)),
                TAG_DATE_TIME_ORIGINAL => fill(&mut values.date_original, tiff::read_ascii(value, field_type)),
                TAG_DATE_TIME_DIGITIZED => fill(&mut values.date_digitized, tiff::read_ascii(value, field_type)),
                TAG_FOCAL_LENGTH => fill(&mut values.focal_length, tiff::read_rational(value, field_type, little)),
                TAG_LENS_MODEL => fill(&mut values.lens, tiff::read_ascii(value, field_type)),
                _ => {}
            }
        } else {
            match tag {
                TAG_MAKE => fill(&mut values.make, tiff::read_ascii(value, field_type)),
                TAG_MODEL => fill(&mut values.model, tiff::read_ascii(value, field_type)),
                TAG_DATE_TIME => fill(&mut values.date_time, tiff::read_ascii(value, field_type)),
                TAG_EXIF_IFD if exif_pointer.is_none() => {
                    if let Some(pointer @ 8..=0xFFFF_FFFF) = tiff::read_unsigned(value, field_type, little) {
                        exif_pointer = u32::try_from(pointer).ok();
                    }
                }
                _ => {}
            }
        }
    }
    exif_pointer
}
