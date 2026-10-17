//! Canon CR3 container reader (W5-10b). Port of `PhotoReview.Imaging.Raw/Bmff/Cr3ContainerReader.cs` (383 lines, baseline d92d6aae).
//!
//! CR3 is ISO-BMFF with the `crx ` brand. Where the data lives (verified by the C# author against real EOS R6 / EOS M50 files):
//! - `moov/uuid(85c0b687-..)`: boxes `CMT1` (a standalone TIFF with IFD0: orientation, sensor size, make/model), `CMT2` (a standalone
//!   TIFF whose IFD0 IS the Exif IFD) and `THMB` (a 160x120 JPEG after a 16-byte header);
//! - top-level `uuid(eaf42b5e-..)`: an 8-byte prefix (`u32 0, u32 1`), then `PRVW` (1620x1080 JPEG after a 16-byte header);
//! - `moov/trak/mdia/minf/stbl`: `stsz` + `stco`/`co64` locate the first sample of the track in `mdat`; track 1 is a full-size lossy
//!   JPEG, the raw track is a lossless JPEG (SOF3) that must NOT become a preview.
//!
//! The final preview list is PRVW (medium) first, then full-size track JPEGs, then THMB (tiny), re-indexed `0..n`. Everything is
//! read through bounded `RawHeaderSource` reads (the header budget is part of the behaviour: a flood of tiny boxes ends in
//! `InvalidData`), and every offset/size in the file is hostile: no function here may panic, wrap an integer, loop without
//! progress or allocate by a file-controlled count.
//!
//! Errors: only what the source raises (`RawHeaderError::InvalidData`, budget/short/IO), the reader's own
//! `InvalidData("File too short for CR3 container.")` and `RawHeaderError::Canceled`. Malformed boxes are skipped silently.
//! Pure: no file system access of its own, no `unsafe`.
//!
//! Written against the W5 sibling stubs (D28: when they differ, THEY win and only names/`Result` plumbing change here):
//! W5-01b `crate::{RawContainerReader, RawHeaderSource, RawHeaderError, EmbeddedPreview, ExifBlock, ..}`, W5-01a
//! `crate::{RawContainerInfo, RawFormat}`, W5-10a `crate::bmff`, W5-02a `crate::tiff_nav`, W5-02b `crate::jpeg_probe`, and
//! `pr_imaging::tiff` (W4-03a, merged).

use pr_core::sync::CancelToken;
use pr_imaging::tiff as tiff_structure;

use crate::bmff::{self, BmffBox};
use crate::jpeg_probe;
use crate::tiff_nav::{self, TiffEntry};
use crate::{
    EmbeddedPreview, EmbeddedPreviewKind, ExifBlock, PreviewColorSpace, RawContainerInfo, RawContainerReader,
    RawFormat, RawHeaderError, RawHeaderSource,
};

/// Canon `uuid` of the `moov` child that carries `CMT1..CMT4` and `THMB`: 85c0b687-820f-11e0-8111-f4ce462b6a48.
const CANON_MOOV_UUID: [u8; 16] = [
    0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A, 0x48,
];

/// Canon `uuid` of the top-level preview box that carries `PRVW`: eaf42b5e-1c98-4b88-b9fb-b7dc406e4d16.
const PREVIEW_UUID: [u8; 16] = [
    0xEA, 0xF4, 0x2B, 0x5E, 0x1C, 0x98, 0x4B, 0x88, 0xB9, 0xFB, 0xB7, 0xDC, 0x40, 0x6E, 0x4D, 0x16,
];

/// Payload bytes of the preview `uuid` box before its first child (`u32 0, u32 1`).
const PREVIEW_UUID_PREFIX_BYTES: i32 = 8;
/// Header in front of the JPEG in both `PRVW` and `THMB` payloads.
const PREVIEW_HEADER_BYTES: i64 = 16;
/// Length of the JPEG start-of-image marker `FF D8`.
const SOI_BYTES: i64 = 2;
/// A file shorter than this is `InvalidData("File too short for CR3 container.")`.
const MIN_FILE_LENGTH: i64 = 16;
/// Largest prefix of a `CMT1` payload read to find the TIFF header (the IFD itself is read separately, from the source).
const CMT1_HEADER_READ_CAP: i64 = 4096;
/// Size of one IFD entry; bounds the entries that can lie inside a `CMT1` payload.
const IFD_ENTRY_BYTES: i64 = 12;

/// TIFF tags read from `CMT1`.
const TAG_IMAGE_WIDTH: u16 = 0x0100;
const TAG_IMAGE_LENGTH: u16 = 0x0101;
const TAG_ORIENTATION: u16 = 0x0112;

/// Where the width, height and JPEG size sit in a 16-byte preview header (all big-endian):
/// `PRVW`: `u32 0, u16 1, u16 width @6, u16 height @8, u16 1, u32 jpegSize @12`;
/// `THMB`: `u32 0, u16 width @4, u16 height @6, u32 jpegSize @8, u16 1, u16 0`.
#[derive(Debug, Clone, Copy)]
struct HeaderLayout {
    size_field_offset: usize,
    width_offset: usize,
    height_offset: usize,
}

const PRVW_LAYOUT: HeaderLayout = HeaderLayout {
    size_field_offset: 12,
    width_offset: 6,
    height_offset: 8,
};
const THMB_LAYOUT: HeaderLayout = HeaderLayout {
    size_field_offset: 8,
    width_offset: 4,
    height_offset: 6,
};

/// Reader for Canon CR3 (C# `Cr3ContainerReader`). Stateless; one value serves every file and thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cr3ContainerReader;

impl RawContainerReader for Cr3ContainerReader {
    /// C# `CanRead`: extension equals `.cr3` ignoring case AND at least 12 bytes AND bytes `4..12` are `ftypcrx `.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        if !pr_core::text::eq_ignore_case(extension, ".cr3") {
            return false;
        }
        matches!(first_64_bytes.get(4..12), Some(b"ftypcrx "))
    }

    /// C# `Read`: top-level scan; the order of the three preview lists is part of the behaviour.
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        if source.length() < MIN_FILE_LENGTH {
            return Err(RawHeaderError::invalid_data("File too short for CR3 container."));
        }

        let mut state = ReadState {
            orientation: 1,
            sensor_width: 0,
            sensor_height: 0,
            previews: PreviewCollector::default(),
            exif_blocks: Vec::new(),
        };

        let mut offset: i64 = 0;
        while offset <= source.length() - 8 {
            ct.check()?;
            let Some(b) = bmff::try_read_box(source, offset, None)? else {
                break;
            };

            if b.uuid == Some(PREVIEW_UUID) {
                parse_preview_uuid_box(source, &b, &mut state.previews)?;
            } else if b.box_type.eq_ignore_ascii_case("PRVW") {
                add_prvw_box(source, &b, &mut state.previews)?;
            } else if b.box_type == "moov" {
                parse_moov_box(source, &b, &mut state)?;
            }

            offset += b.total_size;
        }

        Ok(RawContainerInfo {
            format: RawFormat::Cr3,
            sensor_width: state.sensor_width,
            sensor_height: state.sensor_height,
            orientation: state.orientation,
            previews: state.previews.into_ordered(),
            exif_blocks: state.exif_blocks,
        })
    }
}

/// The C# `ref` parameters (`orientation`, `sensorWidth`, `sensorHeight`) plus the two collections the parse helpers fill.
#[derive(Debug)]
struct ReadState {
    orientation: i32,
    sensor_width: i32,
    sensor_height: i32,
    previews: PreviewCollector,
    exif_blocks: Vec<ExifBlock>,
}

/// C# `PreviewCollector`: previews grouped by origin so the final list is PRVW, then track JPEGs, then THMB.
#[derive(Debug, Default)]
struct PreviewCollector {
    prvw: Vec<EmbeddedPreview>,
    track: Vec<EmbeddedPreview>,
    thumb: Vec<EmbeddedPreview>,
}

impl PreviewCollector {
    /// C# `ToOrderedList`: `prvw ++ track ++ thumb`, each element's `index` replaced by its position in the result.
    fn into_ordered(self) -> Vec<EmbeddedPreview> {
        let mut ordered = Vec::with_capacity(self.prvw.len() + self.track.len() + self.thumb.len());
        ordered.extend(self.prvw);
        ordered.extend(self.track);
        ordered.extend(self.thumb);
        for (position, preview) in ordered.iter_mut().enumerate() {
            preview.index = i32::try_from(position).unwrap_or(i32::MAX);
        }
        ordered
    }
}

/// C# `NewJpeg`: `index` 0 (re-indexed later), kind JPEG, colour space `Unknown`, `header_resolved` false.
fn new_jpeg(offset: i64, length: i64, width: i32, height: i32) -> EmbeddedPreview {
    EmbeddedPreview::new(
        0,
        offset,
        length,
        EmbeddedPreviewKind::Jpeg,
        width,
        height,
        PreviewColorSpace::Unknown,
    )
}

fn short_read() -> RawHeaderError {
    RawHeaderError::invalid_data("Header source returned fewer bytes than requested.")
}

/// Reads exactly `N` bytes at `offset` and copies them out (the slice from the source dies with the next read).
/// A short slice from a non-conforming source is `InvalidData`, never an index panic.
fn read_array<const N: usize>(source: &mut dyn RawHeaderSource, offset: i64) -> Result<[u8; N], RawHeaderError> {
    let count = i32::try_from(N).unwrap_or(i32::MAX);
    let slice = source.read(offset, count)?;
    slice
        .get(..N)
        .and_then(|s| <[u8; N]>::try_from(s).ok())
        .ok_or_else(short_read)
}

fn be_u16(bytes: &[u8], at: usize) -> u16 {
    match bytes.get(at..at + 2).and_then(|s| <[u8; 2]>::try_from(s).ok()) {
        Some(b) => u16::from_be_bytes(b),
        None => 0,
    }
}

fn be_u32(bytes: &[u8], at: usize) -> u32 {
    match bytes.get(at..at + 4).and_then(|s| <[u8; 4]>::try_from(s).ok()) {
        Some(b) => u32::from_be_bytes(b),
        None => 0,
    }
}

/// C# `ReadClamped`: up to `max_count` bytes at `offset`, clamped to the source; an EMPTY slice (and NO source read) when
/// `offset < 0 || offset >= source.length() || max_count <= 0`.
fn read_clamped(source: &mut dyn RawHeaderSource, offset: i64, max_count: i64) -> Result<&[u8], RawHeaderError> {
    let length = source.length();
    if offset < 0 || offset >= length || max_count <= 0 {
        return Ok(&[]);
    }
    let count = max_count.min(length - offset);
    source.read(offset, i32::try_from(count).unwrap_or(i32::MAX))
}

/// C# `HasSoi`: `read_clamped(offset, 2)` is exactly `FF D8`.
fn has_soi(source: &mut dyn RawHeaderSource, offset: i64) -> Result<bool, RawHeaderError> {
    Ok(matches!(read_clamped(source, offset, SOI_BYTES)?, [0xFF, 0xD8]))
}

/// C# `TryReadHeaderedJpeg`: the JPEG after a 16-byte Canon preview header.
fn try_read_headered_jpeg(
    source: &mut dyn RawHeaderSource,
    container: &BmffBox,
    layout: HeaderLayout,
) -> Result<Option<EmbeddedPreview>, RawHeaderError> {
    if container.payload_size < PREVIEW_HEADER_BYTES + SOI_BYTES {
        return Ok(None);
    }

    let header: [u8; 16] = read_array(source, container.payload_offset)?;
    let width = i32::from(be_u16(&header, layout.width_offset));
    let height = i32::from(be_u16(&header, layout.height_offset));
    let declared = be_u32(&header, layout.size_field_offset);

    let jpeg_offset = container.payload_offset + PREVIEW_HEADER_BYTES;
    let available = container.payload_size - PREVIEW_HEADER_BYTES;
    let jpeg_length = if declared > 0 && i64::from(declared) <= available {
        i64::from(declared)
    } else {
        available
    };

    if jpeg_length < SOI_BYTES || jpeg_offset > source.length() - jpeg_length || !has_soi(source, jpeg_offset)? {
        return Ok(None);
    }
    Ok(Some(new_jpeg(jpeg_offset, jpeg_length, width, height)))
}

/// C# `ParsePreviewUuidBox`: children with an 8-byte skip; when none, again without skip.
fn parse_preview_uuid_box(
    source: &mut dyn RawHeaderSource,
    uuid_box: &BmffBox,
    previews: &mut PreviewCollector,
) -> Result<(), RawHeaderError> {
    let mut children = bmff::read_child_boxes(source, uuid_box, PREVIEW_UUID_PREFIX_BYTES)?;
    if children.is_empty() {
        children = bmff::read_child_boxes(source, uuid_box, 0)?;
    }

    for child in &children {
        if child.box_type == "PRVW"
            && let Some(preview) = try_read_headered_jpeg(source, child, PRVW_LAYOUT)?
        {
            previews.prvw.push(preview);
        }
    }
    Ok(())
}

/// C# `AddPrvwBox` (top-level box whose type equals `PRVW` ignoring ASCII case).
fn add_prvw_box(
    source: &mut dyn RawHeaderSource,
    prvw_box: &BmffBox,
    previews: &mut PreviewCollector,
) -> Result<(), RawHeaderError> {
    if prvw_box.payload_size < SOI_BYTES {
        return Ok(());
    }

    if has_soi(source, prvw_box.payload_offset)? {
        previews
            .prvw
            .push(new_jpeg(prvw_box.payload_offset, prvw_box.payload_size, 0, 0));
        return Ok(());
    }

    if let Some(preview) = try_read_headered_jpeg(source, prvw_box, PRVW_LAYOUT)? {
        previews.prvw.push(preview);
    }
    Ok(())
}

/// C# `ParseMoovBox`.
fn parse_moov_box(
    source: &mut dyn RawHeaderSource,
    moov_box: &BmffBox,
    state: &mut ReadState,
) -> Result<(), RawHeaderError> {
    let children = bmff::read_child_boxes(source, moov_box, 0)?;
    for child in &children {
        if child.uuid == Some(CANON_MOOV_UUID) {
            parse_canon_moov_uuid(source, child, state)?;
        } else if child.box_type == "trak" {
            parse_track_for_jpeg_preview(source, child, &mut state.previews)?;
        }
    }
    Ok(())
}

/// C# `ParseCanonMoovUuid`.
fn parse_canon_moov_uuid(
    source: &mut dyn RawHeaderSource,
    uuid_box: &BmffBox,
    state: &mut ReadState,
) -> Result<(), RawHeaderError> {
    let children = bmff::read_child_boxes(source, uuid_box, 0)?;
    for child in &children {
        match child.box_type.as_str() {
            "CMT1" => parse_cmt1(source, child, state)?,
            "CMT2" => state.exif_blocks.push(ExifBlock::with_ifd_is_exif(
                child.payload_offset,
                child.payload_size,
                true,
                true,
            )),
            "THMB" => {
                if let Some(thumb) = try_read_headered_jpeg(source, child, THMB_LAYOUT)? {
                    state.previews.thumb.push(thumb);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The `CMT1` branch of C# `ParseCanonMoovUuid`.
fn parse_cmt1(source: &mut dyn RawHeaderSource, cmt1: &BmffBox, state: &mut ReadState) -> Result<(), RawHeaderError> {
    state
        .exif_blocks
        .push(ExifBlock::new(cmt1.payload_offset, cmt1.payload_size, true));
    if cmt1.payload_size < 16 {
        return Ok(());
    }

    let read_len = cmt1.payload_size.min(CMT1_HEADER_READ_CAP);
    let tiff_span = source.read(cmt1.payload_offset, i32::try_from(read_len).unwrap_or(i32::MAX))?;
    let Some(header) = tiff_structure::try_read_header(tiff_span) else {
        return Ok(());
    };
    let little_endian = header.little_endian;
    let ifd0 = i64::from(header.ifd0_offset);
    if ifd0 < 8 || ifd0 > cmt1.payload_size - 2 {
        return Ok(());
    }

    let entries = tiff_nav::read_ifd_entries(source, cmt1.payload_offset + ifd0, little_endian)?.entries;
    // C# `(int)Math.Max(0, (payload - ifd0 - 2) / 12)`: the cast wraps; a negative `Take` count takes nothing.
    let max_entries = ((cmt1.payload_size - ifd0 - 2) / IFD_ENTRY_BYTES).max(0) as i32;
    let take = usize::try_from(max_entries).unwrap_or(0);

    for entry in entries.iter().take(take) {
        match entry.tag {
            TAG_ORIENTATION => {
                if let Some(v) = read_cmt1_value(source, cmt1, entry, little_endian)?
                    && (1..=8).contains(&v)
                {
                    state.orientation = i32::try_from(v).unwrap_or(state.orientation);
                }
            }
            TAG_IMAGE_WIDTH => {
                if let Some(v) = read_cmt1_value(source, cmt1, entry, little_endian)?
                    && (1..=i64::from(i32::MAX)).contains(&v)
                {
                    state.sensor_width = i32::try_from(v).unwrap_or(state.sensor_width);
                }
            }
            TAG_IMAGE_LENGTH => {
                if let Some(v) = read_cmt1_value(source, cmt1, entry, little_endian)?
                    && (1..=i64::from(i32::MAX)).contains(&v)
                {
                    state.sensor_height = i32::try_from(v).unwrap_or(state.sensor_height);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// C# `ReadCmt1Value`: the first unsigned value of a CMT1 IFD entry (out-of-line offsets are relative to the CMT1 payload).
fn read_cmt1_value(
    source: &mut dyn RawHeaderSource,
    cmt1: &BmffBox,
    entry: &TiffEntry,
    little_endian: bool,
) -> Result<Option<i64>, RawHeaderError> {
    let type_size = i64::from(tiff_structure::type_size(entry.field_type));
    if type_size == 0 || entry.count == 0 {
        return Ok(None);
    }
    if type_size * i64::from(entry.count) <= 4 {
        return tiff_nav::read_tag_unsigned(source, entry, little_endian);
    }

    if i64::from(entry.value_or_offset) > cmt1.payload_size - type_size {
        return Ok(None);
    }
    let absolute = cmt1.payload_offset + i64::from(entry.value_or_offset);
    let Ok(absolute) = u32::try_from(absolute) else {
        return Ok(None);
    };
    let shifted = TiffEntry {
        value_or_offset: absolute,
        ..*entry
    };
    tiff_nav::read_tag_unsigned(source, &shifted, little_endian)
}

/// `FirstOrDefault(b => b.Type == ty)` plus the `TotalSize == 0` "not found" test.
fn first_child_of_type<'a>(children: &'a [BmffBox], box_type: &str) -> Option<&'a BmffBox> {
    children.iter().find(|b| b.box_type == box_type)
}

/// C# `ParseTrackForJpegPreview` for one `trak`.
fn parse_track_for_jpeg_preview(
    source: &mut dyn RawHeaderSource,
    trak_box: &BmffBox,
    previews: &mut PreviewCollector,
) -> Result<(), RawHeaderError> {
    let children = bmff::read_child_boxes(source, trak_box, 0)?;
    let Some(mdia) = first_child_of_type(&children, "mdia") else {
        return Ok(());
    };
    let children = bmff::read_child_boxes(source, mdia, 0)?;
    let Some(minf) = first_child_of_type(&children, "minf") else {
        return Ok(());
    };
    let children = bmff::read_child_boxes(source, minf, 0)?;
    let Some(stbl) = first_child_of_type(&children, "stbl") else {
        return Ok(());
    };

    let stbl_children = bmff::read_child_boxes(source, stbl, 0)?;
    let stsz = first_child_of_type(&stbl_children, "stsz");
    let stco = first_child_of_type(&stbl_children, "stco");
    let co64 = first_child_of_type(&stbl_children, "co64");

    let sample_size = read_sample_size(source, stsz)?;
    let chunk_offset = read_chunk_offset(source, stco, co64)?;

    // `sample_size <= length - chunk_offset` is the overflow-free form of `chunk_offset + sample_size <= length`.
    let length = source.length();
    if chunk_offset > 0
        && sample_size > 0
        && chunk_offset < length
        && sample_size <= length - chunk_offset
        && let Some((width, height)) = probe_track_jpeg(source, chunk_offset, sample_size)?
    {
        previews.track.push(new_jpeg(chunk_offset, sample_size, width, height));
    }
    Ok(())
}

/// The `stsz` half of C# `ParseTrackForJpegPreview`.
fn read_sample_size(source: &mut dyn RawHeaderSource, stsz: Option<&BmffBox>) -> Result<i64, RawHeaderError> {
    let Some(stsz) = stsz.filter(|b| b.payload_size >= 12) else {
        return Ok(0);
    };
    let data: [u8; 12] = read_array(source, stsz.payload_offset)?;
    let uniform = be_u32(&data, 4);
    let count = be_u32(&data, 8);
    if uniform > 0 {
        return Ok(i64::from(uniform));
    }
    if count >= 1 && stsz.payload_size >= 16 {
        let entry: [u8; 4] = read_array(source, stsz.payload_offset + 12)?;
        return Ok(i64::from(u32::from_be_bytes(entry)));
    }
    Ok(0)
}

/// The `stco`/`co64` half of C# `ParseTrackForJpegPreview`; the FIRST chunk offset or 0.
fn read_chunk_offset(
    source: &mut dyn RawHeaderSource,
    stco: Option<&BmffBox>,
    co64: Option<&BmffBox>,
) -> Result<i64, RawHeaderError> {
    if let Some(stco) = stco.filter(|b| b.payload_size >= 8) {
        let data: [u8; 8] = read_array(source, stco.payload_offset)?;
        let count = be_u32(&data, 4);
        if count >= 1 && stco.payload_size >= 12 {
            let offset: [u8; 4] = read_array(source, stco.payload_offset + 8)?;
            return Ok(i64::from(u32::from_be_bytes(offset)));
        }
    } else if let Some(co64) = co64.filter(|b| b.payload_size >= 8) {
        let data: [u8; 8] = read_array(source, co64.payload_offset)?;
        let count = be_u32(&data, 4);
        if count >= 1 && co64.payload_size >= 16 {
            let offset: [u8; 8] = read_array(source, co64.payload_offset + 8)?;
            return Ok(i64::try_from(u64::from_be_bytes(offset)).unwrap_or(0));
        }
    }
    Ok(0)
}

/// C# `TryProbeTrackJpeg`: only a lossy SOF0/1/2 8-bit frame passes; an I/O failure of the source propagates.
fn probe_track_jpeg(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<(i32, i32)>, RawHeaderError> {
    Ok(jpeg_probe::try_read_lossy_frame(source, offset, length)?.map(|f| (f.width, f.height)))
}
