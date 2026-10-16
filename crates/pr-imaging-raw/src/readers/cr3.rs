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

// D11: removed in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code, unused_imports)]

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
    /// C# `CanRead`: extension equals `.cr3` ignoring case (`pr_core::text::eq_ignore_case`) AND at least 12 bytes AND bytes `4..12` are
    /// `ftypcrx ` (the box size in bytes `0..4` is NOT looked at). Any other brand, a shorter slice or another extension is `false`.
    fn can_read(&self, first_64_bytes: &[u8], extension: &str) -> bool {
        todo!()
    }

    /// C# `Read`. See contract.md "Algorithm" for the step order; the order of the three preview lists is part of the behaviour.
    fn read(&self, source: &mut dyn RawHeaderSource, ct: &CancelToken) -> Result<RawContainerInfo, RawHeaderError> {
        todo!()
    }
}

/// The C# `ref` parameters (`orientation`, `sensorWidth`, `sensorHeight`) plus the two collections the parse helpers fill, bundled so
/// the helpers take one `&mut`. Starts at orientation 1, sensor 0x0, nothing collected.
#[derive(Debug)]
struct ReadState {
    orientation: i32,
    sensor_width: i32,
    sensor_height: i32,
    previews: PreviewCollector,
    exif_blocks: Vec<ExifBlock>,
}

/// C# `PreviewCollector`: previews grouped by origin so the final list is PRVW, then track JPEGs, then THMB, whatever the box order.
#[derive(Debug, Default)]
struct PreviewCollector {
    prvw: Vec<EmbeddedPreview>,
    track: Vec<EmbeddedPreview>,
    thumb: Vec<EmbeddedPreview>,
}

impl PreviewCollector {
    /// C# `ToOrderedList`: `prvw ++ track ++ thumb`, each element's `index` replaced by its position in the result.
    fn into_ordered(self) -> Vec<EmbeddedPreview> {
        todo!()
    }
}

/// C# `NewJpeg`: `index` 0 (re-indexed later), kind JPEG, colour space `Unknown`, `header_resolved` false.
fn new_jpeg(offset: i64, length: i64, width: i32, height: i32) -> EmbeddedPreview {
    todo!()
}

/// C# `ReadClamped`: up to `max_count` bytes at `offset`, clamped to the source; an EMPTY slice (and NO source read) when
/// `offset < 0 || offset >= source.length() || max_count <= 0`. Otherwise reads `min(max_count, length - offset)` bytes.
fn read_clamped(source: &mut dyn RawHeaderSource, offset: i64, max_count: i64) -> Result<&[u8], RawHeaderError> {
    todo!()
}

/// C# `HasSoi`: `read_clamped(offset, 2)` is exactly `FF D8` (a 1-byte clamped read at the very end is `false`).
fn has_soi(source: &mut dyn RawHeaderSource, offset: i64) -> Result<bool, RawHeaderError> {
    todo!()
}

/// C# `TryReadHeaderedJpeg`: the JPEG after a 16-byte Canon preview header (`layout` says where width/height/size are).
///
/// `None` when `container.payload_size < 18`; otherwise reads the 16 header bytes at `payload_offset` (copy the five fields out BEFORE the
/// next read), then `jpeg_offset = payload_offset + 16`, `available = payload_size - 16`,
/// `jpeg_length = declared if 0 < declared <= available else available` (a declared size of 0 or above the box is clamped to the box; a
/// declared size of 1 is kept and then rejected), and returns `None` when `jpeg_length < 2`, when `jpeg_offset > source.length() - jpeg_length`
/// (cannot happen for a box that fits the source, kept as the C# guard) or when there is no SOI at `jpeg_offset`. `Some` carries
/// `new_jpeg(jpeg_offset, jpeg_length, width, height)` with the 16-bit big-endian header fields (0..=65535) as they are.
fn try_read_headered_jpeg(
    source: &mut dyn RawHeaderSource,
    container: &BmffBox,
    layout: HeaderLayout,
) -> Result<Option<EmbeddedPreview>, RawHeaderError> {
    todo!()
}

/// C# `ParsePreviewUuidBox` (top-level `uuid` equal to [`PREVIEW_UUID`]): children with an 8-byte payload skip; when that yields NO child
/// at all, children again without skip (tolerates producers without the prefix). Every child whose type is exactly `PRVW` (case
/// sensitive here) and that [`try_read_headered_jpeg`] accepts with [`PRVW_LAYOUT`] is pushed to `previews.prvw`; other children are ignored.
fn parse_preview_uuid_box(
    source: &mut dyn RawHeaderSource,
    uuid_box: &BmffBox,
    previews: &mut PreviewCollector,
) -> Result<(), RawHeaderError> {
    todo!()
}

/// C# `AddPrvwBox` (a top-level box whose type equals `PRVW` ignoring ASCII case, e.g. the `prvw` of the synthetic test files).
/// Payload under 2 bytes: nothing. SOI at `payload_offset`: the WHOLE payload is one preview with width/height 0 (trailing bytes
/// included). Otherwise [`try_read_headered_jpeg`] with [`PRVW_LAYOUT`]. Pushes to `previews.prvw`.
fn add_prvw_box(
    source: &mut dyn RawHeaderSource,
    prvw_box: &BmffBox,
    previews: &mut PreviewCollector,
) -> Result<(), RawHeaderError> {
    todo!()
}

/// C# `ParseMoovBox`: `read_child_boxes(moov, 0)` (at most 256 children). A child whose `uuid` equals [`CANON_MOOV_UUID`] goes to
/// [`parse_canon_moov_uuid`]; a child of type `trak` goes to [`parse_track_for_jpeg_preview`]; everything else (including a `uuid` child
/// with another uuid) is ignored.
fn parse_moov_box(
    source: &mut dyn RawHeaderSource,
    moov_box: &BmffBox,
    state: &mut ReadState,
) -> Result<(), RawHeaderError> {
    todo!()
}

/// C# `ParseCanonMoovUuid`: `read_child_boxes(uuid_box, 0)`; per child, by exact type: `CMT1` => [`parse_cmt1`]; `CMT2` => push
/// `ExifBlock::with_ifd_is_exif(payload_offset, payload_size, true, true)`; `THMB` => [`try_read_headered_jpeg`] with [`THMB_LAYOUT`]
/// pushed to `previews.thumb`. Other children (`CMT3`, `CMT4`, ...) are ignored.
fn parse_canon_moov_uuid(
    source: &mut dyn RawHeaderSource,
    uuid_box: &BmffBox,
    state: &mut ReadState,
) -> Result<(), RawHeaderError> {
    todo!()
}

/// The `CMT1` branch of C# `ParseCanonMoovUuid`.
///
/// 1. ALWAYS push `ExifBlock::new(payload_offset, payload_size, true)` (even when the TIFF below is unusable).
/// 2. Only when `payload_size >= 16`: read `min(payload_size, 4096)` bytes at `payload_offset` (the read size is observable through the
///    header budget; keep it) and parse the TIFF header from them with `pr_imaging::tiff::try_read_header` (byte order `II`/`MM`; the magic is
///    NOT checked). No header: stop.
/// 3. `ifd0` must satisfy `ifd0 >= 8 && ifd0 <= payload_size - 2` (as i64; "IFD0 must lie inside the CMT1 payload"), else stop.
/// 4. `entries = tiff_nav::read_ifd_entries(source, payload_offset + ifd0, little_endian)?.entries` (the navigator bounds itself by the FILE, not
///    by the payload, hence step 5).
/// 5. `max_entries = (payload_size - ifd0 - 2) / 12` computed in i64, cast to `i32` with wrap-around like the C# `(int)` (clamp a negative to 0
///    for `take`); only the first `max_entries` entries are looked at, so an entry straddling the payload end is never read.
/// 6. For each entry, by tag, in this order and ONLY when the tag matches (the value read costs header budget):
///    `0x0112` => `read_cmt1_value`, `Some(v)` with `1..=8` sets `orientation`; `0x0100` / `0x0101` => `Some(v)` with `1..=i32::MAX` sets
///    `sensor_width` / `sensor_height`. A later valid entry overrides an earlier one; an invalid one (0, 9, negative, above i32::MAX, unreadable)
///    changes nothing.
fn parse_cmt1(source: &mut dyn RawHeaderSource, cmt1: &BmffBox, state: &mut ReadState) -> Result<(), RawHeaderError> {
    todo!()
}

/// C# `ReadCmt1Value`: the first unsigned value of a CMT1 IFD entry.
///
/// `type_size = pr_imaging::tiff::type_size(entry.field_type)`; `0` (unknown type) or `count == 0` => `None`. If `type_size * count <= 4` (as i64)
/// the value is inline: `tiff_nav::read_tag_unsigned(source, entry, little_endian)`. Otherwise the offset is RELATIVE TO THE CMT1 PAYLOAD
/// (a standalone TIFF): `None` when `value_or_offset > cmt1.payload_size - type_size` (the value must lie fully inside the payload; the subtraction
/// is i64 and may be negative), `absolute = cmt1.payload_offset + value_or_offset`, `None` when `absolute > u32::MAX`, else
/// `read_tag_unsigned` on a copy of the entry whose `value_or_offset` is `absolute as u32` (the navigator then checks the file bounds itself).
fn read_cmt1_value(
    source: &mut dyn RawHeaderSource,
    cmt1: &BmffBox,
    entry: &TiffEntry,
    little_endian: bool,
) -> Result<Option<i64>, RawHeaderError> {
    todo!()
}

/// C# `FirstOrDefault(b => b.Type == ty)` followed by the `TotalSize == 0` "not found" test (a parsed box always has `total_size >= 8`).
fn first_child_of_type<'a>(children: &'a [BmffBox], box_type: &str) -> Option<&'a BmffBox> {
    todo!()
}

/// C# `ParseTrackForJpegPreview` for one `trak`: walk `trak -> mdia -> minf -> stbl` (`read_child_boxes(.., 0)` at each level and take the FIRST
/// child of the wanted type; a missing level ends the function silently), take the first `stsz`, first `stco` and first `co64` of `stbl`,
/// compute `sample_size` ([`read_sample_size`]) and `chunk_offset` ([`read_chunk_offset`]), and push to `previews.track` when
/// `chunk_offset > 0 && sample_size > 0 && chunk_offset < source.length() && sample_size <= source.length() - chunk_offset` AND
/// [`probe_track_jpeg`] accepts the range; the preview is `new_jpeg(chunk_offset, sample_size, probed_width, probed_height)`.
fn parse_track_for_jpeg_preview(
    source: &mut dyn RawHeaderSource,
    trak_box: &BmffBox,
    previews: &mut PreviewCollector,
) -> Result<(), RawHeaderError> {
    todo!()
}

/// The `stsz` half of C# `ParseTrackForJpegPreview`. `None`/payload under 12 => 0. Reads 12 bytes at `payload_offset`: `uniform = u32 @4`,
/// `count = u32 @8` (big-endian). `uniform > 0` => `uniform`. Else only when `count >= 1 && payload_size >= 16`: the `u32` at `payload_offset + 12`
/// (the first per-sample size). Everything else => 0. The result is a size in `0..=u32::MAX`.
fn read_sample_size(source: &mut dyn RawHeaderSource, stsz: Option<&BmffBox>) -> Result<i64, RawHeaderError> {
    todo!()
}

/// The `stco`/`co64` half of C# `ParseTrackForJpegPreview`; the FIRST chunk offset or 0.
///
/// `stco` present with `payload_size >= 8`: read 8 bytes, `count = u32 @4`; `count >= 1 && payload_size >= 12` => the `u32` at `payload_offset + 8`;
/// otherwise 0 and **`co64` is NOT consulted** (an `stco` that exists and has 8+ bytes wins even when it holds no offset).
/// Only when there is no `stco` or its payload is under 8 bytes: `co64` present with `payload_size >= 8`: read 8 bytes, `count = u32 @4`;
/// `count >= 1 && payload_size >= 16` => the `u64` at `payload_offset + 8`, kept only when `<= i64::MAX` (else 0).
fn read_chunk_offset(
    source: &mut dyn RawHeaderSource,
    stco: Option<&BmffBox>,
    co64: Option<&BmffBox>,
) -> Result<i64, RawHeaderError> {
    todo!()
}

/// C# `TryProbeTrackJpeg`: `jpeg_probe::try_read_lossy_frame(source, offset, length)`. A SOI alone is not enough: the CR3 raw track is a lossless
/// JPEG (SOF3, tens of MB) and only a lossy SOF0/1/2 8-bit frame passes. `Some((width, height))` for a lossy frame, `None` otherwise (including
/// an exhausted budget: the probe already maps every non-I/O header error to `Ok(None)`, so the C# `catch (InvalidDataException) when
/// (!IsIoFailure)` of this method has no extra work to do); an I/O failure of the source is `Err` and propagates.
fn probe_track_jpeg(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
) -> Result<Option<(i32, i32)>, RawHeaderError> {
    todo!()
}
