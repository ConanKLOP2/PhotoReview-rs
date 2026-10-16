//! TIFF IFD navigation over a random-access header source (port of `PhotoReview.Imaging.Raw.Tiff.TiffHeaderNavigator`, W5-02a).
//!
//! Shared base of the TIFF-family container readers (CR2, NEF, ORF, DNG, ARW, RW2). Every offset and count in the file is
//! hostile: no function here may panic, wrap an integer, or allocate more than a bounded amount. A read that the header
//! source refuses (outside the source, over the 8 MiB header budget, I/O failure) comes back as `Err(RawHeaderError)`,
//! the twin of the C# `InvalidDataException`; only [`compute_exif_block`] and [`reconcile_jpeg_size`] absorb the
//! non-I/O ones (C# `catch (InvalidDataException) when (!IsIoFailure)`).
//!
//! Pure apart from the reads on the source: no clock, no file system, no `unsafe`.

// W5-01 names (W5-01a `formats::limits`, W5-01b `contracts`, both re-exported at the crate root by their mod-lines). Adjust ONLY these
// `use` lines if the lead merges them elsewhere; `RawHeaderError::is_io_failure(&self)` is the W5-01b method (W5-01a also has a free
// function `is_io_failure(&RawHeaderError)`: the lead keeps ONE, see contract.md "Assumed W5-01 API").
use crate::formats::limits::{DEFAULT_EXIF_BLOCK_BYTES, MAX_ENTRIES_PER_IFD, MAX_EXIF_BLOCK_BYTES};
use crate::{ExifBlock, RawHeaderError, RawHeaderSource};
use pr_imaging::tiff::{read_u16, read_u32, read_unsigned, type_size};

/// IFD0 tags whose values the EXIF summary reads (Make, Model, DateTime, ExifIFD pointer).
const IFD0_TAGS: [u16; 4] = [0x010F, 0x0110, 0x0132, 0x8769];
/// Exif IFD tags whose values the EXIF summary reads.
const EXIF_TAGS: [u16; 7] = [0x829A, 0x829D, 0x8827, 0x9003, 0x9004, 0x920A, 0xA434];
/// Exif IFD pointer tag.
const EXIF_POINTER_TAG: u16 = 0x8769;

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
    let len = source.length();
    let empty = IfdRead::default();
    if ifd_offset < 8 || ifd_offset > len - 2 {
        return Ok(empty);
    }
    let entry_count = {
        let span = source.read(ifd_offset, 2)?;
        if span.len() < 2 {
            return Ok(empty);
        }
        i64::from(read_u16(span, 0, little_endian))
    };
    let max_entries = i64::from(MAX_ENTRIES_PER_IFD);
    let mut clamped = entry_count.min(max_entries);
    if ifd_offset + 2 + clamped * 12 > len {
        clamped = (len - ifd_offset - 2) / 12;
        if clamped <= 0 {
            return Ok(empty);
        }
    }
    let bytes_needed = clamped * 12;
    let Ok(bytes_i32) = i32::try_from(bytes_needed) else {
        return Ok(empty);
    };
    let mut entries = Vec::with_capacity(usize::try_from(clamped).unwrap_or(0));
    {
        let span = source.read(ifd_offset + 2, bytes_i32)?;
        let span_len = i64::try_from(span.len()).unwrap_or(0);
        for i in 0..clamped {
            let off = i * 12;
            if off + 12 > span_len {
                break;
            }
            let Ok(o) = i32::try_from(off) else { break };
            entries.push(TiffEntry {
                tag: read_u16(span, o, little_endian),
                field_type: read_u16(span, o + 2, little_endian),
                count: read_u32(span, o + 4, little_endian),
                value_or_offset: read_u32(span, o + 8, little_endian),
            });
        }
    }
    let mut next = 0u32;
    if entry_count <= max_entries {
        let pos = ifd_offset + 2 + entry_count * 12;
        if pos + 4 <= len {
            let span = source.read(pos, 4)?;
            if span.len() >= 4 {
                let candidate = read_u32(span, 0, little_endian);
                if candidate >= 8 && i64::from(candidate) <= len - 6 {
                    next = candidate;
                }
            }
        }
    }
    Ok(IfdRead {
        entries,
        next_ifd_offset: next,
    })
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
    let size = i64::from(type_size(entry.field_type));
    if size == 0 || entry.count == 0 {
        return Ok(None);
    }
    if size * i64::from(entry.count) <= 4 {
        let buf = inline_bytes(entry.value_or_offset, little_endian);
        return Ok(read_unsigned(&buf, entry.field_type, little_endian));
    }
    let len = source.length();
    let offset = i64::from(entry.value_or_offset);
    if offset + size > len {
        return Ok(None);
    }
    let Ok(size_i32) = i32::try_from(size) else {
        return Ok(None);
    };
    let span = source.read(offset, size_i32)?;
    Ok(read_unsigned(span, entry.field_type, little_endian))
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
    let size = i64::from(type_size(entry.field_type));
    let Ok(max_u32) = u32::try_from(max_items) else {
        return Ok(Vec::new());
    };
    if size == 0 || entry.count == 0 || max_u32 == 0 {
        return Ok(Vec::new());
    }
    let n = i64::from(entry.count.min(max_u32));
    let total = size * i64::from(entry.count);
    let mut result = Vec::new();
    if total <= 4 {
        let buf = inline_bytes(entry.value_or_offset, little_endian);
        collect_items(&buf, n, size, entry.field_type, little_endian, &mut result);
        return Ok(result);
    }
    let len = source.length();
    let offset = i64::from(entry.value_or_offset);
    let want = n * size;
    if offset + want > len {
        return Ok(result);
    }
    // DIVERGENCE: C# computes `n * size` in `int` (wraps for huge max_items); here it is `i64`, and a read size that
    // does not fit the source's `i32` count gives an empty list.
    let Ok(want_i32) = i32::try_from(want) else {
        return Ok(result);
    };
    let span = source.read(offset, want_i32)?;
    collect_items(span, n, size, entry.field_type, little_endian, &mut result);
    Ok(result)
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
    let len = source.length();
    let default_len = len.min(i64::from(DEFAULT_EXIF_BLOCK_BYTES));
    let grown = (|| -> Result<i64, RawHeaderError> {
        let mut length = default_len;
        let (mut needed, exif_pointer) = exif_summary_extent(source, ifd0_offset, &IFD0_TAGS, little_endian)?;
        if exif_pointer > 0 {
            let (exif_needed, _) = exif_summary_extent(source, exif_pointer, &EXIF_TAGS, little_endian)?;
            needed = needed.max(exif_needed);
        }
        if needed <= len.min(i64::from(MAX_EXIF_BLOCK_BYTES)) {
            length = length.max(needed);
        }
        Ok(length)
    })();
    match grown {
        Ok(length) => Ok(ExifBlock::new(0, length, true)),
        Err(e) if e.is_io_failure() => Err(e),
        Err(_) => Ok(ExifBlock::new(0, default_len, true)),
    }
}

/// First entry with `tag` (a copy), `None` when absent.
#[must_use]
pub fn try_get_entry(entries: &[TiffEntry], tag: u16) -> Option<TiffEntry> {
    entries.iter().find(|e| e.tag == tag).copied()
}

/// `[offset, offset + length)` lies inside a file of `file_length` bytes; offset `0` is never a payload start.
/// Overflow-free: `offset > 0 && length > 0 && offset <= file_length - length`.
#[must_use]
pub fn is_range_in_file(offset: i64, length: i64, file_length: i64) -> bool {
    offset > 0 && length > 0 && offset <= file_length.saturating_sub(length)
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
    let (Some(offsets), Some(counts)) = (try_get_entry(entries, 0x0111), try_get_entry(entries, 0x0117)) else {
        return Ok(None);
    };
    if offsets.count != 1 || counts.count != 1 {
        return Ok(None);
    }
    let Some(strip) = read_tag_unsigned(source, &offsets, little_endian)? else {
        return Ok(None);
    };
    let Some(bytes) = read_tag_unsigned(source, &counts, little_endian)? else {
        return Ok(None);
    };
    if !is_range_in_file(strip, bytes, source.length()) {
        return Ok(None);
    }
    Ok(Some((strip, bytes)))
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
    let read = read_ifd_entries(source, exif_ifd_offset, little_endian)?;
    let mut width = 0i32;
    let mut height = 0i32;
    for entry in &read.entries {
        if entry.tag != 0xA002 && entry.tag != 0xA003 {
            continue;
        }
        let Some(v) = read_tag_unsigned(source, entry, little_endian)? else {
            continue;
        };
        let Ok(v) = i32::try_from(v) else { continue };
        if v < 1 {
            continue;
        }
        if entry.tag == 0xA002 {
            width = v;
        } else {
            height = v;
        }
    }
    Ok(if width > 0 && height > 0 {
        Some((width, height))
    } else {
        None
    })
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
    let Some(entry) = try_get_entry(entries, 0xC620) else {
        return Ok(None);
    };
    let values = read_tag_unsigned_array(source, &entry, little_endian, 2)?;
    if values.len() < 2 {
        return Ok(None);
    }
    let (Ok(w), Ok(h)) = (i32::try_from(values[0]), i32::try_from(values[1])) else {
        return Ok(None);
    };
    if w <= 0 || h <= 0 {
        return Ok(None);
    }
    Ok(Some((w, h)))
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
    let Some(entry) = try_get_entry(entries, 0xC61E) else {
        return Ok(true);
    };
    let len = source.length();
    let offset = i64::from(entry.value_or_offset);
    if entry.field_type != 5 || entry.count < 2 || offset < 8 || offset > len - 16 {
        return Ok(false);
    }
    let span = source.read(offset, 16)?;
    if span.len() < 16 {
        return Ok(false);
    }
    let hn = read_u32(span, 0, little_endian);
    let hd = read_u32(span, 4, little_endian);
    let vn = read_u32(span, 8, little_endian);
    let vd = read_u32(span, 12, little_endian);
    if hn == 0 || hd == 0 || vn == 0 || vd == 0 {
        return Ok(false);
    }
    Ok(u64::from(hn) * u64::from(vd) == u64::from(vn) * u64::from(hd))
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
    let fits =
        |w: i32, h: i32| w > 0 && h > 0 && (raw_width <= 0 || raw_height <= 0 || (w <= raw_width && h <= raw_height));
    if fits(crop_width, crop_height) {
        (crop_width, crop_height)
    } else if fits(exif_width, exif_height) {
        (exif_width, exif_height)
    } else {
        (raw_width, raw_height)
    }
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
    match try_get_entry(entries, tag) {
        Some(entry) => read_tag_unsigned(source, &entry, little_endian),
        None => Ok(None),
    }
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
    let width = read_tag_value(source, entries, 0x0100, little_endian)?;
    let height = read_tag_value(source, entries, 0x0101, little_endian)?;
    Ok((clamp_to_int(width), clamp_to_int(height)))
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
    let Some(offset) = read_tag_value(source, entries, 0x0201, little_endian)? else {
        return Ok(None);
    };
    let Some(length) = read_tag_value(source, entries, 0x0202, little_endian)? else {
        return Ok(None);
    };
    if !is_range_in_file(offset, length, source.length()) || !starts_with_soi(source, offset, length)? {
        return Ok(None);
    }
    Ok(Some((offset, length)))
}

/// The range is at least 4 bytes, inside the source, and its first two bytes are `FF D8`.
///
/// # Errors
/// The source refused the read.
pub fn starts_with_soi(source: &mut dyn RawHeaderSource, offset: i64, length: i64) -> Result<bool, RawHeaderError> {
    if length < 4 || !is_range_in_file(offset, length, source.length()) {
        return Ok(false);
    }
    let span = source.read(offset, 2)?;
    Ok(span.len() >= 2 && span[0] == 0xFF && span[1] == 0xD8)
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
    if *width <= 0 || *height <= 0 {
        return Ok(());
    }
    match read_frame(source, offset, length) {
        Ok(Some((w, h))) => {
            *width = w;
            *height = h;
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(e) if !e.is_io_failure() => Ok(()),
        Err(e) => Err(e),
    }
}

/// Positive values clamped to `i32::MAX`; `None`, zero and negative give `0`.
#[must_use]
pub fn clamp_to_int(value: Option<i64>) -> i32 {
    match value {
        Some(v) if v > 0 => i32::try_from(v).unwrap_or(i32::MAX),
        _ => 0,
    }
}

/// The 4 inline bytes of an entry's value field in the file's byte order.
fn inline_bytes(value: u32, little_endian: bool) -> [u8; 4] {
    if little_endian {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    }
}

/// Decodes up to `n` items of `size` bytes from `span` into `out` (RATIONAL rounded half away from zero, items without an
/// unsigned reading skipped).
fn collect_items(span: &[u8], n: i64, size: i64, field_type: u16, little_endian: bool, out: &mut Vec<i64>) {
    let span_len = i64::try_from(span.len()).unwrap_or(i64::MAX);
    for i in 0..n {
        if (i + 1) * size > span_len {
            break;
        }
        let Ok(start) = usize::try_from(i * size) else { break };
        let item = &span[start..];
        if field_type == 5 {
            if item.len() < 8 {
                continue;
            }
            let num = read_u32(item, 0, little_endian);
            let den = read_u32(item, 4, little_endian);
            if den == 0 {
                continue;
            }
            // Half away from zero (`MidpointRounding.AwayFromZero`); the quotient is within 0..=u32::MAX.
            out.push((f64::from(num) / f64::from(den)).round() as i64);
        } else if let Some(v) = read_unsigned(item, field_type, little_endian) {
            out.push(v);
        }
    }
}

/// End of the IFD plus the out-of-line values of the `wanted` tags, and the 0x8769 pointer seen (0 when none).
fn exif_summary_extent(
    source: &mut dyn RawHeaderSource,
    ifd: i64,
    wanted: &[u16],
    little_endian: bool,
) -> Result<(i64, i64), RawHeaderError> {
    let read = read_ifd_entries(source, ifd, little_endian)?;
    if read.entries.is_empty() {
        return Ok((0, 0));
    }
    let count = i64::try_from(read.entries.len()).unwrap_or(0);
    let mut end = ifd + 2 + count * 12 + 4;
    let mut exif_pointer = 0i64;
    for entry in &read.entries {
        if entry.tag == EXIF_POINTER_TAG {
            exif_pointer = i64::from(entry.value_or_offset);
        }
        if !wanted.contains(&entry.tag) {
            continue;
        }
        let bytes = i64::from(type_size(entry.field_type)) * i64::from(entry.count);
        if bytes > 4 {
            end = end.max(i64::from(entry.value_or_offset) + bytes);
        }
    }
    Ok((end, exif_pointer))
}
