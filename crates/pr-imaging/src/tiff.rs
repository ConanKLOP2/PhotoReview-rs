//! Low-level TIFF reader and IFD structure parser (port of `PhotoReview.Imaging.Metadata.TiffStructure`, W4-03a).
//!
//! Hostile-input safe: every offset and length is bounds checked and no function panics or allocates on a bad input.
//! Shared by the EXIF parser (W4-03b) and the RAW container readers (Wave 5). Pure: no I/O, no clock, no `unsafe`.
//!
//! Stub: signatures and data types are final, bodies are `todo!()`. Remove the `#![allow]` line when implemented (D11).
#![allow(unused_variables, dead_code)]

/// Longest ASCII value returned by [`try_get_value_span`] and by [`read_ascii`], in bytes (C# `MaxAsciiBytes`).
pub const MAX_ASCII_BYTES: usize = 256;

/// Longest out-of-line non-ASCII value (array or UNDEFINED blob) returned by [`try_get_value_span`], in bytes
/// (C# `MaxValueBytes`).
pub const MAX_VALUE_BYTES: usize = 4096;

/// Result of a successful [`try_read_header`] (the three `out` values of the C# method).
///
/// `magic` is NOT validated (42 for TIFF, 0x4F52 for ORF, 0x55 for RW2): callers decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TiffHeader {
    /// `true` for `II`, `false` for `MM`.
    pub little_endian: bool,
    /// The u16 at offset 2, read in the file's byte order.
    pub magic: u16,
    /// The u32 at offset 4 (offset of IFD0, relative to the start of the block; NOT range checked here).
    pub ifd0_offset: u32,
}

/// An unsigned EXIF RATIONAL, kept exact so a shutter speed of 1/250 stays `1/250` (C# `ExifRational`).
///
/// Defined here because [`read_rational`] returns it; `exif_summary` re-exports it (D30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ExifRational {
    pub numerator: u32,
    pub denominator: u32,
}

impl ExifRational {
    /// Builds a rational from its two halves (C# positional record constructor).
    #[must_use]
    pub const fn new(numerator: u32, denominator: u32) -> Self {
        Self { numerator, denominator }
    }

    /// `numerator / denominator` as `f64`; `0.0` when the denominator is 0 (C# `Value`).
    #[must_use]
    pub fn value(self) -> f64 {
        todo!("W4-03a: 0.0 when denominator is 0, else numerator / denominator as f64")
    }
}

/// Reads the TIFF header (byte order, magic, first IFD offset). `None` when fewer than 8 bytes are given or the first
/// two bytes are neither `II` nor `MM` (both letters must match: `IM` and `MI` are rejected).
#[must_use]
pub fn try_read_header(tiff: &[u8]) -> Option<TiffHeader> {
    todo!("W4-03a")
}

/// Byte size of one element of a TIFF field type; `0` for an unknown type (including BigTIFF type 16).
///
/// 1, 2, 6, 7 => 1; 3, 8 => 2; 4, 9, 11, 13 => 4; 5, 10, 12 => 8.
#[must_use]
pub fn type_size(field_type: u16) -> i32 {
    todo!("W4-03a")
}

/// Reads a u16 at `offset`; `0` when `offset` is negative or `offset > data.len() - 2` (never panics, never wraps).
///
/// `offset` is `i32` on purpose (C# `int`): hostile callers pass `entry + 2 + 12 * i` that overflowed or went negative.
#[must_use]
pub fn read_u16(data: &[u8], offset: i32, little_endian: bool) -> u16 {
    todo!("W4-03a")
}

/// Reads a u32 at `offset`; `0` when `offset` is negative or `offset > data.len() - 4`.
#[must_use]
pub fn read_u32(data: &[u8], offset: i32, little_endian: bool) -> u32 {
    todo!("W4-03a")
}

/// Returns the bytes of an IFD entry's value (inline in the entry, or at the entry's offset field) inside `tiff`.
///
/// `entry_offset` is the start of the 12-byte entry (tag at +0, type at +2, count at +4, value/offset at +8). The
/// returned slice covers the whole value: every element of an array, and all bytes of an ASCII (up to
/// [`MAX_ASCII_BYTES`]) or UNDEFINED (up to [`MAX_VALUE_BYTES`]) value. A text or UNDEFINED value running past the end
/// of the block is rejected; a numeric array is cut to the whole elements that exist. `None` when not even one element
/// (or one byte) is readable. Callers that want a scalar read the first element of the slice.
///
/// `field_type` and `count` are the raw entry fields (they are NOT read from the entry here).
#[must_use]
pub fn try_get_value_span(
    tiff: &[u8],
    entry_offset: i32,
    field_type: u16,
    count: u32,
    little_endian: bool,
) -> Option<&[u8]> {
    todo!("W4-03a")
}

/// Reads an ASCII (type 2) or UNDEFINED (type 7) value as text: clamped to [`MAX_ASCII_BYTES`], cut at the first NUL,
/// decoded as UTF-8 with the .NET replacement behaviour. `None` for any other type or when the text is empty.
#[must_use]
pub fn read_ascii(value: &[u8], field_type: u16) -> Option<String> {
    todo!("W4-03a")
}

/// Reads the FIRST element of an integer value as `i64` (signed types are sign-extended): BYTE 1, SHORT 3, LONG 4,
/// IFD 13 (unsigned); SSHORT 8, SLONG 9 (signed). `None` for every other type or when `value` is shorter than one
/// element.
#[must_use]
pub fn read_unsigned(value: &[u8], field_type: u16, little_endian: bool) -> Option<i64> {
    todo!("W4-03a")
}

/// Reads the FIRST rational of a value. RATIONAL (5): both words unsigned, zero allowed. SRATIONAL (10): both words
/// as `i32`, and `None` unless both are strictly positive. `None` for other types or when fewer than 8 bytes.
#[must_use]
pub fn read_rational(value: &[u8], field_type: u16, little_endian: bool) -> Option<ExifRational> {
    todo!("W4-03a")
}
