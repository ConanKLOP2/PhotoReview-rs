//! Nikon NEF container reader (port of `PhotoReview.Imaging.Raw.Tiff.NefContainerReader`, W5-04).
//!
//! NEF is a TIFF container: IFD0 is a 160x120 thumbnail, the SubIFDs (tag 0x014A) hold the raw image, the full-size
//! `JpgFromRaw` and reduced previews, and the Nikon MakerNote carries one more preview (PreviewIFD, tag 0x0011).
//! The reader walks at most `MAX_IFD_COUNT` IFDs breadth first through bounded header reads and never panics on a hostile
//! file: every failure is an `Err` (C# `InvalidDataException`).

use pr_core::sync::CancelToken;
use pr_core::text::eq_ignore_case;
use pr_imaging::tiff;

use crate::formats::limits::MAX_IFD_COUNT;
use crate::jpeg_probe;
use crate::preview_selector;
use crate::tiff_nav::{self as nav, DEFAULT_ARRAY_ITEMS, TiffEntry};
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, PreviewColorSpace, RawContainerInfo, RawContainerReader, RawFormat,
    RawHeaderError, RawHeaderSource,
};

/// Largest masked margin (pixels per axis) between the raw IFD and a JpgFromRaw frame that still counts as the same image
/// (C# `MaxMaskedMargin`). The test pins both sides of it: 256 matches, 257 does not.
const MAX_MASKED_MARGIN: i64 = 256;

/// Size of the Nikon type-3 MakerNote prefix: `"Nikon\0"` (6) + version (1) + 3 more bytes; the embedded TIFF header
/// starts at +10 (C# `TiffHeaderDelta`).
const NIKON_TIFF_HEADER_DELTA: i64 = 10;

/// Stateless reader for `.nef` files (C# `NefContainerReader`).
#[derive(Debug, Default, Clone, Copy)]
pub struct NefContainerReader;

impl RawContainerReader for NefContainerReader {
    /// True when `extension` equals `.nef` (OrdinalIgnoreCase), at least 4 bytes are given and they are `II*\0` or
    /// `MM\0*`. Only those four bytes are examined (a longer or shorter-than-64 slice makes no difference).
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        if !eq_ignore_case(extension, ".nef") || first_64_bytes.len() < 4 {
            return false;
        }
        matches!(first_64_bytes[..4], [0x49, 0x49, 0x2A, 0x00] | [0x4D, 0x4D, 0x00, 0x2A])
    }

    /// Reads the container through bounded header reads.
    ///
    /// # Errors
    /// `RawHeaderError::InvalidData` when the source is shorter than 16 bytes, the TIFF magic is not 42 or a header read fails
    /// (past-end, exhausted budget, I/O: an I/O failure keeps its io flag); `RawHeaderError::Canceled` when `cancel` fires
    /// (checked once per dequeued IFD).
    fn read(&self, source: &mut dyn RawHeaderSource, cancel: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        if source.length() < 16 {
            return Err(RawHeaderError::invalid_data("File too short for NEF container."));
        }
        let header = {
            let span = source.read(0, 16)?;
            tiff::try_read_header(span)
        };
        let header = match header {
            Some(h) if h.magic == 42 => h,
            _ => return Err(RawHeaderError::invalid_data("Invalid NEF TIFF header.")),
        };
        let little_endian = header.little_endian;
        let ifd0_offset = i64::from(header.ifd0_offset);

        let mut orientation = 1i32;
        let mut previews: Vec<EmbeddedPreview> = Vec::new();
        let mut pending: std::collections::VecDeque<i64> = std::collections::VecDeque::new();
        pending.push_back(ifd0_offset);
        let mut visited: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut maker_note_offset: Option<i64> = None;
        let mut ifd_count = 0i32;
        let mut ifd0_width = 0i32;
        let mut ifd0_height = 0i32;
        let mut raw_area = 0i64;
        let mut sensor_width = 0i32;
        let mut sensor_height = 0i32;
        let mut crop_width = 0i32;
        let mut crop_height = 0i32;
        let mut exif_pixel_width = 0i32;
        let mut exif_pixel_height = 0i32;

        // IFD0 chain first, then the SubIFDs it announces (raw image, JpgFromRaw and reduced previews live there).
        // The cap is tested BEFORE the pop: once reached, the rest of the queue is dropped unread.
        while ifd_count < MAX_IFD_COUNT {
            let Some(ifd_offset) = pending.pop_front() else {
                break;
            };
            cancel.check()?;
            if ifd_offset <= 0 || !visited.insert(ifd_offset) {
                continue;
            }

            let nav::IfdRead {
                entries,
                next_ifd_offset,
            } = nav::read_ifd_entries(source, ifd_offset, little_endian)?;
            let is_ifd0 = ifd_count == 0;
            ifd_count += 1;

            let (width, height) = nav::read_image_size(source, &entries, little_endian)?;
            // C# `(uint)long`: the low 32 bits (the mask makes the value fit `u32`, so the conversion never fails).
            let sub_file_type =
                nav::read_tag_value(source, &entries, 0x00FE, little_endian)?.unwrap_or(0) & 0xFFFF_FFFF;
            let sub_file_type = u32::try_from(sub_file_type).unwrap_or(0);
            let compression = nav::read_tag_value(source, &entries, 0x0103, little_endian)?.unwrap_or(1);

            if is_ifd0 {
                if let Some(o) = nav::read_tag_value(source, &entries, 0x0112, little_endian)? {
                    if (1..=8).contains(&o) {
                        orientation = i32::try_from(o).unwrap_or(1);
                    }
                }
                ifd0_width = width;
                ifd0_height = height;
            }

            if let Some(sub_entry) = nav::try_get_entry(&entries, 0x014A) {
                for sub in nav::read_tag_unsigned_array(source, &sub_entry, little_endian, DEFAULT_ARRAY_ITEMS)? {
                    pending.push_back(sub);
                }
            }

            if let Some(exif_entry) = nav::try_get_entry(&entries, 0x8769) {
                if let Some(exif_offset) = nav::read_tag_unsigned(source, &exif_entry, little_endian)? {
                    if exif_offset > 0 {
                        let exif_ifd = nav::read_ifd_entries(source, exif_offset, little_endian)?;
                        for exif_tag in &exif_ifd.entries {
                            if exif_tag.tag == 0x927C && exif_tag.count > 4 && maker_note_offset.is_none() {
                                maker_note_offset = Some(i64::from(exif_tag.value_or_offset));
                            }
                        }

                        if exif_pixel_width == 0 {
                            if let Some((pixel_width, pixel_height)) =
                                nav::try_read_exif_pixel_dimensions(source, exif_offset, little_endian)?
                            {
                                exif_pixel_width = pixel_width;
                                exif_pixel_height = pixel_height;
                            }
                        }
                    }
                }
            }

            let mut is_preview = false;
            if let Some((jpeg_offset, jpeg_length)) = nav::try_read_jpeg_interchange(source, &entries, little_endian)? {
                let (mut preview_width, mut preview_height) = (width, height);
                nav::reconcile_jpeg_size(
                    source,
                    jpeg_offset,
                    jpeg_length,
                    &mut preview_width,
                    &mut preview_height,
                    read_frame_size,
                )?;
                add_preview(&mut previews, jpeg_offset, jpeg_length, preview_width, preview_height);
                is_preview = true;
            } else if compression == 6 || compression == 7 {
                if let Some((strip_offset, strip_length)) = nav::try_read_single_strip(source, &entries, little_endian)?
                {
                    if let Some(frame) = jpeg_probe::try_read_lossy_frame(source, strip_offset, strip_length)? {
                        // The probed frame size wins over a declared IFD size.
                        add_preview(&mut previews, strip_offset, strip_length, frame.width, frame.height);
                        is_preview = true;
                    }
                }
            }

            // The raw image is the largest IFD that is neither a JPEG preview nor a reduced-resolution image
            // (in real NEFs IFD0 is a 160x120 thumbnail and the sensor size lives in a SubIFD).
            let area = i64::from(width) * i64::from(height);
            if !is_preview && (sub_file_type & 1) == 0 && area > raw_area {
                raw_area = area;
                sensor_width = width;
                sensor_height = height;
                // The crop belongs to the winning raw IFD: reset to 0 when this one has none.
                (crop_width, crop_height) =
                    nav::try_read_default_crop_size(source, &entries, little_endian)?.unwrap_or((0, 0));
            }

            if next_ifd_offset > 0 {
                pending.push_back(i64::from(next_ifd_offset));
            }
        }

        if sensor_width == 0 {
            sensor_width = ifd0_width;
            sensor_height = ifd0_height;
        }

        if let Some(note_offset) = maker_note_offset {
            if note_offset > 0 {
                parse_nikon_maker_note_preview(source, note_offset, &mut previews)?;
            }
        }

        // The raw IFD size includes the masked margins (Z 7: 8288x5520 vs the 8256x5504 active area). Prefer
        // DefaultCropSize, then the Exif pixel size, then the full-size JpgFromRaw frame (Nikon writes it at the active
        // area size and no Exif pixel size), and only then the raw IFD size.
        (sensor_width, sensor_height) = nav::choose_active_sensor_size(
            sensor_width,
            sensor_height,
            crop_width,
            crop_height,
            exif_pixel_width,
            exif_pixel_height,
        );
        if crop_width == 0 && exif_pixel_width == 0 {
            if let Some((frame_width, frame_height)) =
                try_read_full_size_jpeg_frame(source, &previews, sensor_width, sensor_height)?
            {
                sensor_width = frame_width;
                sensor_height = frame_height;
            }
        }

        let exif_block = nav::compute_exif_block(source, little_endian, ifd0_offset)?;

        Ok(RawContainerInfo {
            format: RawFormat::Nef,
            sensor_width,
            sensor_height,
            orientation,
            previews,
            exif_blocks: vec![exif_block],
        })
    }
}

/// Frame size of the JPEG at `offset`/`length` (the `read_frame` of `reconcile_jpeg_size`): `Err` only for header errors,
/// `Ok(None)` when no SOF0/1/2 frame is found.
fn read_frame_size(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<(i32, i32)>, RawHeaderError> {
    Ok(preview_selector::try_read_jpeg_frame(source, offset, length)?.map(|f| (f.width, f.height)))
}

/// Finds the largest embedded JPEG whose frame is 0..=`MAX_MASKED_MARGIN` pixels smaller than the raw IFD size on both axes
/// (a full-size JpgFromRaw), reading its dimensions from the SOF header when the container gave none (C#
/// `TryReadFullSizeJpegFrame`). Candidates: the 4 previews with the greatest `length`, longest first, ties in list order
/// (stable). The first examined preview wins an equal area. `Ok(None)` when none qualifies.
///
/// # Errors
/// Only an I/O failure of the header source; an exhausted budget while walking one preview skips that preview.
fn try_read_full_size_jpeg_frame(
    source: &mut dyn RawHeaderSource,
    previews: &[EmbeddedPreview],
    raw_width: i32,
    raw_height: i32,
) -> Result<Option<(i32, i32)>, RawHeaderError> {
    if raw_width <= 0 || raw_height <= 0 {
        return Ok(None);
    }
    // `sort_by_key` is stable: equal lengths keep list order, as LINQ `OrderByDescending` does.
    let mut candidates: Vec<&EmbeddedPreview> = previews.iter().collect();
    candidates.sort_by_key(|p| std::cmp::Reverse(p.length));

    let mut best: Option<(i32, i32)> = None;
    let mut best_area = 0i64;
    for preview in candidates.into_iter().take(4) {
        let (mut w, mut h) = (preview.width, preview.height);
        if w <= 0 || h <= 0 {
            match preview_selector::try_read_jpeg_frame(source, preview.offset, preview.length) {
                Ok(Some(frame)) => {
                    w = frame.width;
                    h = frame.height;
                }
                Ok(None) => continue,
                // Exhausted header budget (or malformed data): this preview cannot confirm a full-size frame.
                Err(e @ RawHeaderError::InvalidData { .. }) if !e.is_io_failure() => continue,
                Err(e) => return Err(e),
            }
        }

        let margin_x = i64::from(raw_width) - i64::from(w);
        let margin_y = i64::from(raw_height) - i64::from(h);
        if !(0..=MAX_MASKED_MARGIN).contains(&margin_x) || !(0..=MAX_MASKED_MARGIN).contains(&margin_y) {
            continue;
        }

        let area = i64::from(w) * i64::from(h);
        if area > best_area {
            best_area = area;
            best = Some((w, h));
        }
    }
    Ok(best)
}

/// Appends a JPEG preview unless one with the same `offset` is listed (C# `AddPreview`): `index` = current count, colour
/// space `Unknown`, `header_resolved` false.
fn add_preview(previews: &mut Vec<EmbeddedPreview>, offset: i64, length: i64, width: i32, height: i32) {
    if previews.iter().any(|p| p.offset == offset) {
        return;
    }
    let index = i32::try_from(previews.len()).unwrap_or(i32::MAX);
    previews.push(EmbeddedPreview::new(
        index,
        offset,
        length,
        EmbeddedPreviewKind::Jpeg,
        width,
        height,
        PreviewColorSpace::Unknown,
    ));
}

/// Parses the Nikon type-3 MakerNote at `maker_note_offset` and lists its PreviewIFD JPEG (C# `ParseNikonMakerNotePreview`).
/// Anything that does not look right (offset < 8 or within 18 bytes of the end, signature or version byte other than
/// `"Nikon\0"` + 2, bad embedded TIFF header or magic, missing tags, range outside the file, no SOI) returns `Ok(())`
/// without a preview. All offsets inside the note are relative to its embedded TIFF header at `maker_note_offset + 10`.
///
/// # Errors
/// A header read that fails (e.g. the SOI probe of a 1-byte range at the last byte of the file reads past the end).
fn parse_nikon_maker_note_preview(
    source: &mut dyn RawHeaderSource,
    maker_note_offset: i64,
    previews: &mut Vec<EmbeddedPreview>,
) -> Result<(), RawHeaderError> {
    if maker_note_offset < 8 || maker_note_offset > source.length() - 18 {
        return Ok(());
    }
    // Copied: the next read invalidates the slice borrowed from the source.
    let Ok(head) = <[u8; 18]>::try_from(source.read(maker_note_offset, 18)?) else {
        return Ok(());
    };
    if head[..6] != *b"Nikon\0" || head[6] != 2 {
        return Ok(());
    }

    let tiff_base = maker_note_offset + NIKON_TIFF_HEADER_DELTA;
    let Some(note_header) = tiff::try_read_header(&head[10..18]) else {
        return Ok(());
    };
    if note_header.magic != 42 {
        return Ok(());
    }
    let note_le = note_header.little_endian;

    let note_ifd = nav::read_ifd_entries(source, tiff_base + i64::from(note_header.ifd0_offset), note_le)?;
    let Some(preview_ifd_entry) = nav::try_get_entry(&note_ifd.entries, 0x0011) else {
        return Ok(());
    };
    let Some(preview_ifd_offset) = nav::read_tag_unsigned(source, &preview_ifd_entry, note_le)? else {
        return Ok(());
    };

    let preview_ifd: Vec<TiffEntry> = nav::read_ifd_entries(source, tiff_base + preview_ifd_offset, note_le)?.entries;
    let Some(start) = nav::read_tag_value(source, &preview_ifd, 0x0201, note_le)? else {
        return Ok(());
    };
    let Some(length) = nav::read_tag_value(source, &preview_ifd, 0x0202, note_le)? else {
        return Ok(());
    };

    let absolute_start = tiff_base + start;
    if !nav::is_range_in_file(absolute_start, length, source.length()) {
        return Ok(());
    }
    if !matches!(source.read(absolute_start, 2)?, [0xFF, 0xD8, ..]) {
        return Ok(());
    }

    add_preview(previews, absolute_start, length, 0, 0);
    Ok(())
}
