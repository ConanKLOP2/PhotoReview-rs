//! TIFF IFD navigation over a random-access header source (port of `PhotoReview.Imaging.Raw.Tiff.TiffHeaderNavigator`, W5-02a).
//!
//! Shared base of the TIFF-family container readers (CR2, NEF, ORF, DNG, ARW, RW2). Every offset and count in the file is
//! hostile: no function here may panic, wrap an integer, or allocate more than a bounded amount. A read that the header
//! source refuses (outside the source, over the 8 MiB header budget, I/O failure) comes back as `Err(RawHeaderError)`,
//! the twin of the C# `InvalidDataException`; only [`compute_exif_block`] and [`reconcile_jpeg_size`] absorb the
//! non-I/O ones (C# `catch (InvalidDataException) when (!IsIoFailure)`).
//!
//! Pure apart from the reads on the source: no clock, no file system, no `unsafe`.

#![allow(unused_variables, unused_imports, dead_code)] // D11: remove with the last todo!()

// W5-01 names (W5-01a `formats::limits`, W5-01b `contracts`, both re-exported at the crate root by their mod-lines). Adjust ONLY these
// `use` lines if the lead merges them elsewhere; `RawHeaderError::is_io_failure(&self)` is the W5-01b method (W5-01a also has a free
// function `is_io_failure(&RawHeaderError)`: the lead keeps ONE, see contract.md "Assumed W5-01 API").
use crate::formats::limits::{DEFAULT_EXIF_BLOCK_BYTES, MAX_ENTRIES_PER_IFD, MAX_EXIF_BLOCK_BYTES};
use crate::{ExifBlock, RawHeaderError, RawHeaderSource};

/// Default `max_items` of [`read_tag_unsigned_array`] (C# optional parameter `maxItems = 64`).
pub const DEFAULT_ARRAY_ITEMS: i32 = 64;

/// One 12-byte IFD directory entry (C# `TiffEntry(ushort Tag, ushort Type, uint Count, uint ValueOrOffset)`).
///
/// `value_or_offset` is the raw 4-byte field decoded in the file's byte order: an inline value when
/// `type_size * count <= 4`, otherwise an offset from the start of the TIFF header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TiffEntry {
    pub tag: u16,
    /// C# `Type`.
    pub field_type: u16,
    pub count: u32,
    pub value_or_offset: u32,
}

/// Result of [`read_ifd_entries`]: the C# return value plus its `out uint nextIfdOffset`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IfdRead {
    pub entries: Vec<TiffEntry>,
    /// `0` = the chain ends here (also for every early return and for an out-of-range pointer).
    pub next_ifd_offset: u32,
}

/// Reads the directory entries of the IFD at `ifd_offset`.
///
/// Empty result (and `next_ifd_offset == 0`) when `ifd_offset < 8` or `ifd_offset > length - 2`, when the 2-byte count
/// cannot be read, or when not even one whole entry fits. At most `MAX_ENTRIES_PER_IFD` (4096) entries are returned;
/// a count that claims more yields the first 4096 and ENDS the chain (the trailing pointer cannot be located). A count
/// that runs past the end of the source yields the whole entries that exist. The next pointer is reported only when it
/// lands in `8..=length - 6`.
///
/// # Errors
/// The source refused a read (budget, I/O).
pub fn read_ifd_entries(
    source: &mut dyn RawHeaderSource,
    ifd_offset: i64,
    little_endian: bool,
) -> Result<IfdRead, RawHeaderError> {
    todo!("W5-02a")
}

/// Reads the FIRST element of a tag as an unsigned (type-dependent) integer; `None` for an unknown type, `count == 0`,
/// a type `read_unsigned` does not know (RATIONAL, ASCII, ...) or an out-of-line value that does not lie fully inside
/// the source. Signed types (SSHORT 8, SLONG 9) can yield negative numbers. Offset `0` is a valid out-of-line offset.
///
/// # Errors
/// The source refused the read.
pub fn read_tag_unsigned(
    source: &mut dyn RawHeaderSource,
    entry: &TiffEntry,
    little_endian: bool,
) -> Result<Option<i64>, RawHeaderError> {
    todo!("W5-02a")
}

/// Reads up to `max_items` elements of a tag (RATIONAL elements are rounded half away from zero; elements of a type
/// without an unsigned reading are skipped). Empty for an unknown type, `count == 0`, `max_items <= 0`, or an
/// out-of-line array whose first `min(count, max_items)` elements do not lie fully inside the source.
///
/// # Errors
/// The source refused the read.
pub fn read_tag_unsigned_array(
    source: &mut dyn RawHeaderSource,
    entry: &TiffEntry,
    little_endian: bool,
    max_items: i32,
) -> Result<Vec<i64>, RawHeaderError> {
    todo!("W5-02a")
}

/// Sizes the TIFF-header EXIF block: starts at 0, at least `min(length, 128 KiB)`, grown to cover IFD0, the Exif IFD
/// (via the 0x8769 pointer) and the out-of-line values of the tags the EXIF summary reads, but only when the whole extent
/// fits `min(length, 4 MiB)`. A non-I/O header error (budget, hostile structure) keeps the default block.
///
/// # Errors
/// Only an I/O failure of the source ([`RawHeaderError::is_io_failure`]).
pub fn compute_exif_block(
    source: &mut dyn RawHeaderSource,
    little_endian: bool,
    ifd0_offset: i64,
) -> Result<ExifBlock, RawHeaderError> {
    todo!("W5-02a")
}

/// First entry with `tag` (a copy), `None` when absent.
#[must_use]
pub fn try_get_entry(entries: &[TiffEntry], tag: u16) -> Option<TiffEntry> {
    todo!("W5-02a")
}

/// `[offset, offset + length)` lies inside a file of `file_length` bytes; offset `0` is never a payload start.
/// Overflow-free: `offset > 0 && length > 0 && offset <= file_length - length`.
#[must_use]
pub fn is_range_in_file(offset: i64, length: i64, file_length: i64) -> bool {
    todo!("W5-02a")
}

/// StripOffsets (0x0111) / StripByteCounts (0x0117) when the image is exactly one strip (both counts == 1) and the byte
/// range fits the file. Returns `Some((offset, length))`; `None` for multi-strip, missing or unreadable values.
///
/// # Errors
/// The source refused a read.
pub fn try_read_single_strip(
    source: &mut dyn RawHeaderSource,
    entries: &[TiffEntry],
    little_endian: bool,
) -> Result<Option<(i64, i64)>, RawHeaderError> {
    todo!("W5-02a")
}

/// PixelXDimension (0xA002) / PixelYDimension (0xA003) of the Exif IFD at `exif_ifd_offset` as `(width, height)`.
/// `None` unless BOTH are present in `1..=i32::MAX` (the last valid duplicate wins).
///
/// # Errors
/// The source refused a read.
pub fn try_read_exif_pixel_dimensions(
    source: &mut dyn RawHeaderSource,
    exif_ifd_offset: i64,
    little_endian: bool,
) -> Result<Option<(i32, i32)>, RawHeaderError> {
    todo!("W5-02a")
}

/// DNG DefaultCropSize (0xC620) as `(width, height)`; `None` when absent, fewer than two values, or a value outside
/// `1..=i32::MAX`.
///
/// # Errors
/// The source refused a read.
pub fn try_read_default_crop_size(
    source: &mut dyn RawHeaderSource,
    entries: &[TiffEntry],
    little_endian: bool,
) -> Result<Option<(i32, i32)>, RawHeaderError> {
    todo!("W5-02a")
}

/// `false` when the IFD carries a DefaultScale (0xC61E) that is not 1:1 (wrong type or count, offset outside `8..=length - 16`,
/// a zero numerator or denominator, unequal ratios). Absent means square (`true`).
///
/// # Errors
/// The source refused a read.
pub fn has_square_default_scale(
    source: &mut dyn RawHeaderSource,
    entries: &[TiffEntry],
    little_endian: bool,
) -> Result<bool, RawHeaderError> {
    todo!("W5-02a")
}

/// Active sensor size: DefaultCropSize, else the Exif pixel size, else the raw IFD size. A candidate is ignored unless
/// both sides are positive and (when the raw size is fully known) not larger than the raw size on either side.
#[must_use]
pub fn choose_active_sensor_size(
    raw_width: i32,
    raw_height: i32,
    crop_width: i32,
    crop_height: i32,
    exif_width: i32,
    exif_height: i32,
) -> (i32, i32) {
    todo!("W5-02a")
}

/// First entry with `tag`, read with [`read_tag_unsigned`]; `None` when absent or unreadable.
///
/// # Errors
/// The source refused the read.
pub fn read_tag_value(
    source: &mut dyn RawHeaderSource,
    entries: &[TiffEntry],
    tag: u16,
    little_endian: bool,
) -> Result<Option<i64>, RawHeaderError> {
    todo!("W5-02a")
}

/// ImageWidth (0x0100) / ImageLength (0x0101) as `(width, height)`; a missing, zero, negative or unreadable value is `0`,
/// an oversize one saturates at `i32::MAX` ([`clamp_to_int`]).
///
/// # Errors
/// The source refused a read.
pub fn read_image_size(
    source: &mut dyn RawHeaderSource,
    entries: &[TiffEntry],
    little_endian: bool,
) -> Result<(i32, i32), RawHeaderError> {
    todo!("W5-02a")
}

/// JPEGInterchangeFormat (0x0201) / JPEGInterchangeFormatLength (0x0202) as `(offset, length)`, required to fit the file
/// and to start with the SOI marker ([`starts_with_soi`]).
///
/// # Errors
/// The source refused a read.
pub fn try_read_jpeg_interchange(
    source: &mut dyn RawHeaderSource,
    entries: &[TiffEntry],
    little_endian: bool,
) -> Result<Option<(i64, i64)>, RawHeaderError> {
    todo!("W5-02a")
}

/// The range is at least 4 bytes, inside the source, and its first two bytes are `FF D8`.
///
/// # Errors
/// The source refused the read.
pub fn starts_with_soi(source: &mut dyn RawHeaderSource, offset: i64, length: i64) -> Result<bool, RawHeaderError> {
    todo!("W5-02a")
}

/// A declared IFD size is only a hint for an embedded JPEG: when BOTH `width` and `height` are positive, the JPEG's own
/// frame header (found by `read_frame(source, offset, length)`, in practice `PreviewSelector::try_read_jpeg_frame`, W5-11)
/// wins if it returns `Some((w, h))`. Otherwise both stay untouched. A non-I/O header error from `read_frame` keeps the
/// declared size.
///
/// `read_frame` is a parameter so W5-02a does not depend on W5-11 (the registry in W5-11 depends on the readers).
///
/// # Errors
/// Only an I/O failure of the source.
pub fn reconcile_jpeg_size<F>(
    source: &mut dyn RawHeaderSource,
    offset: i64,
    length: i64,
    width: &mut i32,
    height: &mut i32,
    read_frame: F,
) -> Result<(), RawHeaderError>
where
    F: FnOnce(&mut dyn RawHeaderSource, i64, i64) -> Result<Option<(i32, i32)>, RawHeaderError>,
{
    todo!("W5-02a")
}

/// Positive values clamped to `i32::MAX`; `None`, zero and negative give `0`.
#[must_use]
pub fn clamp_to_int(value: Option<i64>) -> i32 {
    todo!("W5-02a")
}
