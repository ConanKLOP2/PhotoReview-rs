//! Olympus / OM System ORF container reader (port of `PhotoReview.Imaging.Raw/Tiff/OrfContainerReader.cs`, W5-05).
//!
//! Reads an ORF through bounded header reads: orientation (IFD0 only), the raw frame size (IFD0 ImageWidth/ImageLength, with the
//! one hard-coded 4080 -> 4056 LibRaw rule), every JPEG preview of the IFD chain (JPEGInterchangeFormat 0x0201/0x0202), and the
//! previews hidden in the Olympus MakerNote (ThumbnailImage 0x0100 and CameraSettings 0x2020 -> PreviewImageStart/Length).
//! Hostile-input safe: nothing here may panic, every offset sum is `i64`, every loop is bounded by `limits::MAX_IFD_COUNT` and
//! `limits::MAX_ENTRIES_PER_IFD`, and a failed header read surfaces as `RawHeaderError` (C# `InvalidDataException`).
//!
//! Pure parser: no `unsafe`, no file system (the caller supplies a [`RawHeaderSource`]), no clock.

use pr_core::sync::CancelToken;
use pr_core::text::eq_ignore_case;
use pr_imaging::tiff;

use crate::formats::limits;
use crate::tiff_nav as nav;
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, PreviewColorSpace, RawContainerInfo, RawContainerReader, RawFormat,
    RawHeaderError, RawHeaderSource, preview_selector,
};

/// Raw frame width (IFD0 ImageWidth) of the E-PM1 / E-PL3 / E-P3 sensor: 24 of its 4080 columns are not image
/// (C# `RawFrameWidthWithMaskedColumns`, `internal`).
pub const RAW_FRAME_WIDTH_WITH_MASKED_COLUMNS: i32 = 4080;

/// Width LibRaw decodes for [`RAW_FRAME_WIDTH_WITH_MASKED_COLUMNS`]: its identify.cpp trims the 24 columns
/// (C# `DecodedWidthWithoutMaskedColumns`, `internal`).
pub const DECODED_WIDTH_WITHOUT_MASKED_COLUMNS: i32 = 4056;

/// ORF reader. Stateless: one value can be shared by every thread (the registry holds one instance).
#[derive(Debug, Clone, Copy, Default)]
pub struct OrfContainerReader;

impl OrfContainerReader {
    /// A new reader (C# `new OrfContainerReader()`).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl RawContainerReader for OrfContainerReader {
    /// `extension` equals `.orf` ignoring case (OrdinalIgnoreCase, `pr_core::text::eq_ignore_case`), at least 4 bytes are given, and
    /// the first four bytes are exactly one of `IIRO`, `IIRS`, `MMOR`, `II*\0`. Plain big-endian `MM\0*` is NOT accepted here (but
    /// [`read`](Self::read) accepts it).
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        if !eq_ignore_case(extension, ".orf") || first_64_bytes.len() < 4 {
            return false;
        }
        matches!(
            &first_64_bytes[..4],
            b"IIRO" | b"IIRS" | b"MMOR" | [b'I', b'I', 0x2A, 0x00]
        )
    }

    /// Parses the container. Errors: [`RawHeaderError::invalid_data`] with `"File too short for ORF container."` (length < 16) or
    /// `"Invalid ORF header."` (byte order letters not `II`/`MM`, or magic not 42 / 0x4F52 / 0x5352); any error of the navigator or the
    /// header source (budget exhausted, I/O failure) propagates unchanged; a cancelled `token` yields the cancellation error
    /// (checked at the top of every IFD iteration).
    fn read(&self, source: &mut dyn RawHeaderSource, token: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        if source.length() < 16 {
            return Err(RawHeaderError::invalid_data("File too short for ORF container."));
        }
        let header = source.read(0, 16)?;
        let Some(parsed) = tiff::try_read_header(header) else {
            return Err(RawHeaderError::invalid_data("Invalid ORF header."));
        };
        let (little_endian, magic, ifd0_offset) = (parsed.little_endian, parsed.magic, parsed.ifd0_offset);
        if !matches!(magic, 42 | 0x4F52 | 0x5352) {
            return Err(RawHeaderError::invalid_data("Invalid ORF header."));
        }

        let mut orientation = 1;
        let mut sensor_width = 0;
        let mut sensor_height = 0;
        let mut previews = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut current = i64::from(ifd0_offset);
        let mut ifd_index = 0;
        let mut maker_note_offset = None;

        while current > 0 && ifd_index < limits::MAX_IFD_COUNT {
            token.check()?;
            if !visited.insert(current) {
                break;
            }
            let ifd = nav::read_ifd_entries(source, current, little_endian)?;
            let mut jpeg_offset = None;
            let mut jpeg_length = None;
            let mut ifd_width = 0;
            let mut ifd_height = 0;
            for entry in &ifd.entries {
                match entry.tag {
                    0x0112 if ifd_index == 0 => {
                        if let Some(v @ 1..=8) = nav::read_tag_unsigned(source, entry, little_endian)? {
                            orientation = v as i32;
                        }
                    }
                    0x0100 => {
                        if let Some(v) = nav::read_tag_unsigned(source, entry, little_endian)? {
                            ifd_width = nav::clamp_to_int(Some(v));
                        }
                    }
                    0x0101 => {
                        if let Some(v) = nav::read_tag_unsigned(source, entry, little_endian)? {
                            ifd_height = nav::clamp_to_int(Some(v));
                        }
                    }
                    0x0201 => jpeg_offset = nav::read_tag_unsigned(source, entry, little_endian)?,
                    0x0202 => jpeg_length = nav::read_tag_unsigned(source, entry, little_endian)?,
                    0x8769 => {
                        if let Some(p) = nav::read_tag_unsigned(source, entry, little_endian)? {
                            if p > 0 {
                                for exif_entry in nav::read_ifd_entries(source, p, little_endian)?.entries {
                                    if exif_entry.tag == 0x927C && exif_entry.count > 4 {
                                        maker_note_offset = Some(i64::from(exif_entry.value_or_offset));
                                    }
                                }
                            }
                        }
                    }
                    0x927C if entry.count > 4 => maker_note_offset = Some(i64::from(entry.value_or_offset)),
                    _ => {}
                }
            }
            if ifd_index == 0 && ifd_width > 0 && ifd_height > 0 {
                sensor_width = ifd_width;
                sensor_height = ifd_height;
            }
            if let (Some(offset), Some(length)) = (jpeg_offset.filter(|v| *v > 0), jpeg_length.filter(|v| *v > 0)) {
                if nav::starts_with_soi(source, offset, length)? {
                    let (mut width, mut height) = (ifd_width, ifd_height);
                    nav::reconcile_jpeg_size(source, offset, length, &mut width, &mut height, |s, o, l| {
                        Ok(preview_selector::try_read_jpeg_frame(s, o, l)?.map(|f| (f.width, f.height)))
                    })?;
                    previews.push(EmbeddedPreview {
                        index: previews.len() as i32,
                        offset,
                        length,
                        kind: EmbeddedPreviewKind::Jpeg,
                        width,
                        height,
                        color_space: PreviewColorSpace::Unknown,
                        header_resolved: false,
                    });
                }
            }
            current = i64::from(ifd.next_ifd_offset);
            ifd_index += 1;
        }
        if let Some(offset) = maker_note_offset.filter(|n| *n > 0 && *n < source.length() - 16) {
            parse_olympus_maker_note(source, offset, little_endian, &mut previews)?;
        }
        let exif_blocks = vec![nav::compute_exif_block(source, little_endian, i64::from(ifd0_offset))?];
        Ok(RawContainerInfo {
            format: RawFormat::Orf,
            sensor_width: decoded_width(sensor_width),
            sensor_height,
            orientation,
            previews,
            exif_blocks,
        })
    }
}

/// The width the zoom (LibRaw) decode returns: `4056` for an IFD0 width of exactly `4080`, else the width unchanged
/// (C# private `DecodedWidth`). Height is never touched.
fn decoded_width(ifd0_width: i32) -> i32 {
    if ifd0_width == RAW_FRAME_WIDTH_WITH_MASKED_COLUMNS {
        DECODED_WIDTH_WITHOUT_MASKED_COLUMNS
    } else {
        ifd0_width
    }
}

/// Olympus / OM System MakerNote at `note_offset` (file offset, `0 < note_offset < length - 16`, guaranteed by the caller).
///
/// Signature dispatch on the first bytes of the note (order matters): `"OM SYSTEM\0"` (IFD at +16, byte order = marks at +12/+13 are
/// exactly `II`, offsets relative to the note), `"OLYMPUS\0"` (IFD at +12, marks at +8/+9, relative), `"OLYMP\0"` (legacy: IFD at +8,
/// file byte order, file-absolute offsets), anything else (plain TIFF IFD at the note start, file byte order, file-absolute).
/// Then: every `0x0100` entry with `count > 4` is a thumbnail (`base + value`, length `count`); the FIRST `0x2020` entry points
/// (`base + value`) at the CameraSettings IFD whose FIRST `0x0101` / `0x0102` give PreviewImageStart / Length (`base + start`).
fn parse_olympus_maker_note(
    source: &mut dyn RawHeaderSource,
    note_offset: i64,
    file_little_endian: bool,
    previews: &mut Vec<EmbeddedPreview>,
) -> Result<(), RawHeaderError> {
    let available = (source.length() - note_offset).min(32);
    if available < 16 {
        return Ok(());
    }
    let header = source.read(note_offset, available as i32)?.to_vec();
    let mut note_little_endian = file_little_endian;
    let mut ifd_start = note_offset;
    let mut base = 0i64;
    if header.get(..10) == Some(b"OM SYSTEM\0") {
        note_little_endian = header.get(12..14) == Some(b"II");
        ifd_start = note_offset + 16;
        base = note_offset;
    } else if header.get(..8) == Some(b"OLYMPUS\0") {
        note_little_endian = header.get(8..10) == Some(b"II");
        ifd_start = note_offset + 12;
        base = note_offset;
    } else if header.get(..6) == Some(b"OLYMP\0") {
        ifd_start = note_offset + 8;
    }
    let entries = nav::read_ifd_entries(source, ifd_start, note_little_endian)?.entries;
    for entry in &entries {
        if entry.tag == 0x0100 && entry.count > 4 {
            add_preview(
                source,
                base + i64::from(entry.value_or_offset),
                i64::from(entry.count),
                previews,
            )?;
        }
    }
    if let Some(settings) = nav::try_get_entry(&entries, 0x2020) {
        let settings_ifd =
            nav::read_ifd_entries(source, base + i64::from(settings.value_or_offset), note_little_endian)?.entries;
        if let (Some(start), Some(length)) = (
            nav::read_tag_value(source, &settings_ifd, 0x0101, note_little_endian)?,
            nav::read_tag_value(source, &settings_ifd, 0x0102, note_little_endian)?,
        ) {
            add_preview(source, base + start, length, previews)?;
        }
    }
    Ok(())
}

/// Adds a JPEG preview found through the MakerNote: ignored unless `nav::is_range_in_file(offset, length, len)`, `length >= 4`, the two
/// bytes at `offset` are `FF D8`, and no preview with the same `offset` is listed yet. Width and height stay 0 (resolved lazily by the
/// preview selector); `index` is the list length before the push; colour space `Unknown`; `header_resolved` false.
fn add_preview(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
    previews: &mut Vec<EmbeddedPreview>,
) -> Result<(), RawHeaderError> {
    if !nav::is_range_in_file(offset, length, source.length())
        || length < 4
        || source.read(offset, 2)? != [0xff, 0xd8]
        || previews.iter().any(|p| p.offset == offset)
    {
        return Ok(());
    }
    previews.push(EmbeddedPreview {
        index: previews.len() as i32,
        offset,
        length,
        kind: EmbeddedPreviewKind::Jpeg,
        width: 0,
        height: 0,
        color_space: PreviewColorSpace::Unknown,
        header_resolved: false,
    });
    Ok(())
}
