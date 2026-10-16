//! Sony ARW container reader (W5-07). Port of `PhotoReview.Imaging.Raw/Tiff/ArwContainerReader.cs` (170 lines, baseline d92d6aae).
//!
//! ARW is a little-endian TIFF (`II*\0`): IFD0 carries the large JPEG preview in 0x0201/0x0202, IFD1 the thumbnail, the
//! SubIFDs (0x014A) the raw image (sometimes a JPEG), and the Exif MakerNote the Sony `PreviewImage` (0x2001).
//!
//! Hostile-input rules (hardening tests in C#): never panics, every offset/length goes through the W5-02 navigator's
//! bounds checks, at most [`MAX_IFD_COUNT`] IFDs are read, a repeated IFD offset is read once. The only errors are the
//! ones the header source raises (`RawHeaderError::InvalidData`, budget exhausted, I/O) and `RawHeaderError::Canceled`.
//! Pure: no file system access of its own (all bytes come through the `RawHeaderSource`), no `unsafe`.

use std::collections::{HashSet, VecDeque};

use pr_core::sync::CancelToken;
use pr_imaging::tiff::try_read_header;

use crate::formats::limits::MAX_IFD_COUNT;
use crate::preview_selector::try_read_jpeg_frame;
use crate::tiff_nav::{self as nav, DEFAULT_ARRAY_ITEMS};
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, PreviewColorSpace, RawContainerInfo, RawContainerReader, RawFormat,
    RawHeaderError, RawHeaderSource,
};

/// The only extension this reader accepts, compared with `OrdinalIgnoreCase` (use `pr_core::text::eq_ignore_case`).
const ARW_EXTENSION: &str = ".arw";

/// A file shorter than this is `InvalidData("File too short for ARW container.")`.
const MIN_FILE_LENGTH: i64 = 16;

/// Messages of the two errors this reader raises itself (golden `reader_message`).
const TOO_SHORT_MESSAGE: &str = "File too short for ARW container.";
const BAD_HEADER_MESSAGE: &str = "Invalid ARW TIFF header.";

/// TIFF magic number `read` requires (only `can_read` is little-endian only).
const TIFF_MAGIC: u16 = 42;

/// TIFF tags used by this reader.
const TAG_ORIENTATION: u16 = 0x0112;
const TAG_SUB_IFDS: u16 = 0x014A;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_MAKER_NOTE: u16 = 0x927C;
/// Sony MakerNote `PreviewImage` (UNDEFINED bytes, `count` = length, offset relative to the TIFF header).
const TAG_SONY_PREVIEW_IMAGE: u16 = 0x2001;

/// Some bodies prefix the MakerNote with `SONY DSC \0\0\0`; its IFD then starts 12 bytes in. Offsets stay file-absolute.
const SONY_DSC_PREFIX: &[u8; 9] = b"SONY DSC ";
const SONY_DSC_IFD_SKIP: i64 = 12;

/// A MakerNote that starts closer than this to the end of the file is ignored without reading (`note > length - 14`).
const MAKER_NOTE_MIN_TAIL: i64 = 14;
/// A MakerNote offset below the end of the TIFF header (8) is ignored.
const MAKER_NOTE_MIN_OFFSET: i64 = 8;

/// Reader for Sony ARW (C# `ArwContainerReader`). Stateless; one value serves every file.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArwContainerReader;

impl RawContainerReader for ArwContainerReader {
    /// C# `CanRead`: extension equals `.arw` ignoring case AND at least 4 bytes AND bytes `49 49 2A 00` (little-endian
    /// TIFF only; a big-endian `MM\0*` file and magic 43 are NOT accepted). Looks only at the first four bytes.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        pr_core::text::eq_ignore_case(extension, ARW_EXTENSION)
            && matches!(first_64_bytes.first_chunk::<4>(), Some(&[0x49, 0x49, 0x2A, 0x00]))
    }

    /// C# `Read`. See contract.md "Algorithm" for the exact step order; the order of the IFD queue decides the order of
    /// `previews`, so it is part of the behaviour.
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        if source.length() < MIN_FILE_LENGTH {
            return Err(RawHeaderError::invalid_data(TOO_SHORT_MESSAGE));
        }
        // Copy the header fields out: the slice returned by `read` dies at the next call.
        let header = try_read_header(source.read(0, 16)?).filter(|h| h.magic == TIFF_MAGIC);
        let Some(header) = header else {
            return Err(RawHeaderError::invalid_data(BAD_HEADER_MESSAGE));
        };
        let le = header.little_endian;
        let ifd0 = i64::from(header.ifd0_offset);

        let mut orientation = 1i32;
        let mut previews: Vec<EmbeddedPreview> = Vec::new();
        let mut pending: VecDeque<i64> = VecDeque::new();
        pending.push_back(ifd0);
        let mut visited: HashSet<i64> = HashSet::new();
        let mut maker_note_offset: Option<i64> = None;
        let mut ifd_count: i32 = 0;
        let mut largest_area: i64 = 0;
        let (mut sensor_w, mut sensor_h) = (0i32, 0i32);
        let (mut crop_w, mut crop_h) = (0i32, 0i32);
        let (mut exif_pixel_w, mut exif_pixel_h) = (0i32, 0i32);

        while !pending.is_empty() && ifd_count < MAX_IFD_COUNT {
            ct.check()?;
            let Some(ifd_offset) = pending.pop_front() else {
                break;
            };
            if ifd_offset <= 0 || !visited.insert(ifd_offset) {
                continue;
            }

            let read = nav::read_ifd_entries(source, ifd_offset, le)?;
            let entries = read.entries;
            let next_ifd = i64::from(read.next_ifd_offset);
            let is_ifd0 = ifd_count == 0;
            ifd_count += 1;

            let (width, height) = nav::read_image_size(source, &entries, le)?;

            if is_ifd0
                && let Some(value) = nav::read_tag_value(source, &entries, TAG_ORIENTATION, le)?
                && (1..=8).contains(&value)
            {
                orientation = i32::try_from(value).unwrap_or(1);
            }

            if let Some(sub_ifds) = nav::try_get_entry(&entries, TAG_SUB_IFDS) {
                for sub in nav::read_tag_unsigned_array(source, &sub_ifds, le, DEFAULT_ARRAY_ITEMS)? {
                    pending.push_back(sub);
                }
            }

            if let Some(exif_entry) = nav::try_get_entry(&entries, TAG_EXIF_IFD)
                && let Some(exif_offset) = nav::read_tag_unsigned(source, &exif_entry, le)?
                && exif_offset > 0
            {
                let exif_entries = nav::read_ifd_entries(source, exif_offset, le)?.entries;
                // QUIRK (C# `??=`): the first such entry of the whole file latches, even at offset 0.
                for entry in exif_entries.iter().filter(|e| e.tag == TAG_MAKER_NOTE && e.count > 4) {
                    maker_note_offset.get_or_insert(i64::from(entry.value_or_offset));
                }
                if exif_pixel_w == 0
                    && let Some((w, h)) = nav::try_read_exif_pixel_dimensions(source, exif_offset, le)?
                {
                    exif_pixel_w = w;
                    exif_pixel_h = h;
                }
            }

            let jpeg = nav::try_read_jpeg_interchange(source, &entries, le)?;
            if let Some((jpeg_offset, jpeg_length)) = jpeg {
                let (mut preview_w, mut preview_h) = (width, height);
                nav::reconcile_jpeg_size(
                    source,
                    jpeg_offset,
                    jpeg_length,
                    &mut preview_w,
                    &mut preview_h,
                    jpeg_frame_size,
                )?;
                add_preview(&mut previews, jpeg_offset, jpeg_length, preview_w, preview_h);
            }

            // The crop is assigned unconditionally inside the branch (C# `out` parameters): a later larger raw IFD
            // without a crop resets it to 0. Equal area keeps the first IFD.
            let area = i64::from(width) * i64::from(height);
            if jpeg.is_none() && area > largest_area {
                largest_area = area;
                sensor_w = width;
                sensor_h = height;
                (crop_w, crop_h) = nav::try_read_default_crop_size(source, &entries, le)?.unwrap_or((0, 0));
            }

            if next_ifd > 0 {
                pending.push_back(next_ifd);
            }
        }

        let (sensor_width, sensor_height) =
            nav::choose_active_sensor_size(sensor_w, sensor_h, crop_w, crop_h, exif_pixel_w, exif_pixel_h);

        if let Some(note) = maker_note_offset
            && note > 0
        {
            parse_sony_maker_note_preview(source, note, le, &mut previews)?;
        }

        let exif_blocks = vec![nav::compute_exif_block(source, le, ifd0)?];
        Ok(RawContainerInfo {
            format: RawFormat::Arw,
            sensor_width,
            sensor_height,
            orientation,
            previews,
            exif_blocks,
        })
    }
}

/// `(width, height)` of the JPEG frame at `[offset, offset + length)`, for [`nav::reconcile_jpeg_size`].
fn jpeg_frame_size(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<(i32, i32)>, RawHeaderError> {
    Ok(try_read_jpeg_frame(source, offset, length)?.map(|frame| (frame.width, frame.height)))
}

/// C# `AddPreview`: appends a JPEG preview unless one with the same `offset` is already listed (offset-only dedupe).
/// `index` = the list length before the push; `color_space` = `Unknown`; `header_resolved` = false.
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

/// C# `ParseSonyMakerNotePreview`: finds the 0x2001 `PreviewImage` entry of the Sony MakerNote at `note_offset` and lists
/// it with width/height 0 (the W5-11 `PreviewSelector` resolves them lazily). Anything wrong with the note (offset out of
/// range, no 0x2001 entry, count <= 4, range outside the file, not SOI) returns `Ok(())` without a preview; only errors
/// raised by the source propagate.
fn parse_sony_maker_note_preview(
    source: &mut dyn RawHeaderSource,
    note_offset: i64,
    little_endian: bool,
    previews: &mut Vec<EmbeddedPreview>,
) -> Result<(), RawHeaderError> {
    if note_offset < MAKER_NOTE_MIN_OFFSET || note_offset > source.length().saturating_sub(MAKER_NOTE_MIN_TAIL) {
        return Ok(());
    }
    let mut ifd_start = note_offset;
    if source.read(note_offset, 9)? == &SONY_DSC_PREFIX[..] {
        ifd_start = note_offset.saturating_add(SONY_DSC_IFD_SKIP);
    }

    let entries = nav::read_ifd_entries(source, ifd_start, little_endian)?.entries;
    let Some(entry) = nav::try_get_entry(&entries, TAG_SONY_PREVIEW_IMAGE) else {
        return Ok(());
    };
    if entry.count <= 4 {
        return Ok(());
    }
    let start = i64::from(entry.value_or_offset);
    let length = i64::from(entry.count);
    if length < 4 || !nav::is_range_in_file(start, length, source.length()) {
        return Ok(());
    }
    if !matches!(source.read(start, 2)?, [0xFF, 0xD8]) {
        return Ok(());
    }
    add_preview(previews, start, length, 0, 0);
    Ok(())
}
