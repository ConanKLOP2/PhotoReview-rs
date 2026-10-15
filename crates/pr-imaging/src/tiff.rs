//! Low-level TIFF reader and IFD structure parser (port of `PhotoReview.Imaging.Metadata.TiffStructure`, W4-03a).
//!
//! Hostile-input safe: every offset and length is bounds checked and no function panics or allocates on a bad input.
//! Shared by the EXIF parser (W4-03b) and the RAW container readers (Wave 5). Pure: no I/O, no clock, no `unsafe`.

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
        if self.denominator == 0 {
            0.0
        } else {
            f64::from(self.numerator) / f64::from(self.denominator)
        }
    }
}

/// `N` bytes at `offset`, or `None` when `offset` is negative or the window runs past the end of `data`.
/// The end is computed with `checked_add` on `usize`, and the offset arrives as `i64`, so no input can wrap.
fn window<const N: usize>(data: &[u8], offset: i64) -> Option<[u8; N]> {
    let start = usize::try_from(offset).ok()?;
    data.get(start..start.checked_add(N)?)?.try_into().ok()
}

fn u16_of(bytes: [u8; 2], little_endian: bool) -> u16 {
    if little_endian {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    }
}

fn u32_of(bytes: [u8; 4], little_endian: bool) -> u32 {
    if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    }
}

fn i16_of(bytes: [u8; 2], little_endian: bool) -> i16 {
    if little_endian {
        i16::from_le_bytes(bytes)
    } else {
        i16::from_be_bytes(bytes)
    }
}

fn i32_of(bytes: [u8; 4], little_endian: bool) -> i32 {
    if little_endian {
        i32::from_le_bytes(bytes)
    } else {
        i32::from_be_bytes(bytes)
    }
}

/// Reads the TIFF header (byte order, magic, first IFD offset). `None` when fewer than 8 bytes are given or the first
/// two bytes are neither `II` nor `MM` (both letters must match: `IM` and `MI` are rejected).
#[must_use]
pub fn try_read_header(tiff: &[u8]) -> Option<TiffHeader> {
    if tiff.len() < 8 {
        return None;
    }
    let little_endian = match (tiff[0], tiff[1]) {
        (b'I', b'I') => true,
        (b'M', b'M') => false,
        _ => return None,
    };
    Some(TiffHeader {
        little_endian,
        magic: read_u16(tiff, 2, little_endian),
        ifd0_offset: read_u32(tiff, 4, little_endian),
    })
}

/// Byte size of one element of a TIFF field type; `0` for an unknown type (including BigTIFF type 16).
///
/// 1, 2, 6, 7 => 1; 3, 8 => 2; 4, 9, 11, 13 => 4; 5, 10, 12 => 8.
#[must_use]
pub fn type_size(field_type: u16) -> i32 {
    match field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => 0,
    }
}

/// Reads a u16 at `offset`; `0` when `offset` is negative or `offset > data.len() - 2` (never panics, never wraps).
///
/// `offset` is `i32` on purpose (C# `int`): hostile callers pass `entry + 2 + 12 * i` that overflowed or went negative.
#[must_use]
pub fn read_u16(data: &[u8], offset: i32, little_endian: bool) -> u16 {
    window::<2>(data, i64::from(offset)).map_or(0, |bytes| u16_of(bytes, little_endian))
}

/// Reads a u32 at `offset`; `0` when `offset` is negative or `offset > data.len() - 4`.
#[must_use]
pub fn read_u32(data: &[u8], offset: i32, little_endian: bool) -> u32 {
    window::<4>(data, i64::from(offset)).map_or(0, |bytes| u32_of(bytes, little_endian))
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
    // The 12-byte entry must lie fully inside the block: an unreadable offset field would otherwise read as 0.
    let len = i64::try_from(tiff.len()).ok()?;
    let entry = i64::from(entry_offset);
    if entry < 0 || entry > len - 12 {
        return None;
    }
    let size = i64::from(type_size(field_type));
    if size == 0 || count == 0 {
        return None;
    }
    let mut total = size * i64::from(count);
    if total <= 4 {
        // Inline: the value sits in the entry's offset field (the 12-byte entry was checked above).
        return slice_at(tiff, entry + 8, total);
    }
    let offset = window::<4>(tiff, entry + 8).map_or(0, |bytes| u32_of(bytes, little_endian));
    if i64::from(offset) >= len {
        return None;
    }
    let available = len - i64::from(offset);
    let cap = if field_type == 2 {
        MAX_ASCII_BYTES
    } else {
        MAX_VALUE_BYTES
    };
    total = total.min(i64::try_from(cap).unwrap_or(i64::MAX));
    if matches!(field_type, 2 | 7) {
        // Text and blobs must lie fully inside the block; a value running past its end is rejected, not shortened.
        if total > available {
            return None;
        }
    } else {
        // A numeric array is cut to the whole elements that exist, so its first element stays readable.
        total = total.min(available / size * size);
    }
    if total < size {
        return None;
    }
    slice_at(tiff, i64::from(offset), total)
}

/// `len` bytes at `start`, or `None` when the range is negative or runs past the end of `data`.
fn slice_at(data: &[u8], start: i64, len: i64) -> Option<&[u8]> {
    let start = usize::try_from(start).ok()?;
    let len = usize::try_from(len).ok()?;
    data.get(start..start.checked_add(len)?)
}

/// Reads an ASCII (type 2) or UNDEFINED (type 7) value as text: clamped to [`MAX_ASCII_BYTES`], cut at the first NUL,
/// decoded as UTF-8 with the .NET replacement behaviour. `None` for any other type or when the text is empty.
#[must_use]
pub fn read_ascii(value: &[u8], field_type: u16) -> Option<String> {
    if !matches!(field_type, 2 | 7) {
        return None;
    }
    // The clamp comes before the NUL search, so a NUL past the cap is never seen (UNDEFINED carries up to 4096 bytes).
    let clamped = value.get(..value.len().min(MAX_ASCII_BYTES)).unwrap_or(value);
    let text = match clamped.iter().position(|&b| b == 0) {
        Some(end) => clamped.get(..end).unwrap_or(clamped),
        None => clamped,
    };
    if text.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(text).into_owned())
    }
}

/// Reads the FIRST element of an integer value as `i64` (signed types are sign-extended): BYTE 1, SHORT 3, LONG 4,
/// IFD 13 (unsigned); SSHORT 8, SLONG 9 (signed). `None` for every other type or when `value` is shorter than one
/// element.
#[must_use]
pub fn read_unsigned(value: &[u8], field_type: u16, little_endian: bool) -> Option<i64> {
    match field_type {
        1 => value.first().map(|&b| i64::from(b)),
        3 => window::<2>(value, 0).map(|b| i64::from(u16_of(b, little_endian))),
        4 | 13 => window::<4>(value, 0).map(|b| i64::from(u32_of(b, little_endian))),
        8 => window::<2>(value, 0).map(|b| i64::from(i16_of(b, little_endian))),
        9 => window::<4>(value, 0).map(|b| i64::from(i32_of(b, little_endian))),
        _ => None,
    }
}

/// Reads the FIRST rational of a value. RATIONAL (5): both words unsigned, zero allowed. SRATIONAL (10): both words
/// as `i32`, and `None` unless both are strictly positive. `None` for other types or when fewer than 8 bytes.
#[must_use]
pub fn read_rational(value: &[u8], field_type: u16, little_endian: bool) -> Option<ExifRational> {
    let numerator = u32_of(window::<4>(value, 0)?, little_endian);
    let denominator = u32_of(window::<4>(value, 4)?, little_endian);
    match field_type {
        5 => Some(ExifRational::new(numerator, denominator)),
        10 => {
            // Same bits, read as signed: a negative or zero word is rejected.
            let n = u32::try_from(i32::from_ne_bytes(numerator.to_ne_bytes())).ok()?;
            let d = u32::try_from(i32::from_ne_bytes(denominator.to_ne_bytes())).ok()?;
            (n > 0 && d > 0).then(|| ExifRational::new(n, d))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::panic::AssertUnwindSafe;

    use pr_test_support::golden;
    use serde_json::Value;

    use super::*;
    use support::{TiffBytes, at, short};

    /// Test-only TIFF builder: the members of `PhotoReview.Imaging.Tests.Raw.Tiff.TiffBytes` these tests use.
    mod support {
        /// One 12-byte IFD entry (`TiffBytes.Entry`).
        pub struct Entry {
            pub tag: u16,
            pub ty: u16,
            pub count: u32,
            pub value: u32,
        }

        /// Out-of-line entry whose data lives at `offset` (`TiffBytes.At`).
        pub fn at(tag: u16, ty: u16, count: u32, offset: u32) -> Entry {
            Entry {
                tag,
                ty,
                count,
                value: offset,
            }
        }

        /// Inline SHORT x 1; the value is left-justified in the offset field (`TiffBytes.Short`).
        pub fn short(tag: u16, value: u32) -> Entry {
            Entry {
                tag,
                ty: 3,
                count: 1,
                value,
            }
        }

        /// Zero-filled block with a TIFF header and IFDs laid out at explicit offsets.
        pub struct TiffBytes {
            little: bool,
            buf: Vec<u8>,
        }

        impl TiffBytes {
            pub fn new(little: bool, size: usize) -> Self {
                Self {
                    little,
                    buf: vec![0; size],
                }
            }

            pub fn header(self, ifd0: u32) -> Self {
                let magic: &[u8] = if self.little { b"II*\0" } else { b"MM\0*" };
                self.put(0, magic).u32(4, ifd0)
            }

            pub fn put(mut self, pos: usize, bytes: &[u8]) -> Self {
                self.buf[pos..pos + bytes.len()].copy_from_slice(bytes);
                self
            }

            pub fn u16(self, pos: usize, v: u16) -> Self {
                let raw = if self.little { v.to_le_bytes() } else { v.to_be_bytes() };
                self.put(pos, &raw)
            }

            pub fn u32(self, pos: usize, v: u32) -> Self {
                let raw = if self.little { v.to_le_bytes() } else { v.to_be_bytes() };
                self.put(pos, &raw)
            }

            /// Writes an IFD of `entries` at `pos`, then the next-IFD pointer.
            pub fn ifd(self, pos: usize, next: u32, entries: &[Entry]) -> Self {
                let mut me = self.u16(pos, entries.len() as u16);
                for (i, e) in entries.iter().enumerate() {
                    let base = pos + 2 + i * 12;
                    me = me.u16(base, e.tag).u16(base + 2, e.ty).u32(base + 4, e.count);
                    me = if e.ty == 3 && e.count == 1 {
                        me.u16(base + 8, e.value as u16)
                    } else {
                        me.u32(base + 8, e.value)
                    };
                }
                me.u32(pos + 2 + entries.len() * 12, next)
            }

            pub fn finish(self) -> Vec<u8> {
                self.buf
            }
        }
    }

    /// Entry offset of the IFD0 entry used by the value-span tests (IFD at 8, so 8 + 2).
    const ENTRY: i32 = 10;
    /// Out-of-line data offset used by the value-span tests.
    const DATA: usize = 100;

    /// `Build(type, count, size)`: a little-endian block with one IFD0 entry (tag 0x1234) pointing at `DATA`.
    fn build(field_type: u16, count: u32, size: usize) -> Vec<u8> {
        TiffBytes::new(true, size)
            .header(8)
            .ifd(8, 0, &[at(0x1234, field_type, count, DATA as u32)])
            .finish()
    }

    /// `TryGetValueSpan(tiff, ENTRY, type, count, little)` copied out, as the C# `span.ToArray()` does.
    fn span(tiff: &[u8], field_type: u16, count: u32) -> Option<Vec<u8>> {
        try_get_value_span(tiff, ENTRY, field_type, count, true).map(<[u8]>::to_vec)
    }

    /// 12-byte IFD entry as `FakeEntry(tag, type, count, value)` writes it (little-endian).
    fn fake_entry(tag: u16, field_type: u16, count: u32, value: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(12);
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&field_type.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes
    }

    // ------------------------------------------------------------------ TiffStructureValueSpanTests

    #[test]
    // C#: TiffStructureValueSpanTests.EntryTruncatedBeforeItsOffsetField_ReturnsFalse
    fn try_get_value_span_entry_truncated_before_offset_field_returns_none() {
        let mut tiff = vec![0u8; 20];
        tiff[..2].copy_from_slice(b"II");
        tiff[2] = 42;
        tiff[14] = 0x34;
        tiff[15] = 0x12; // tag
        tiff[16] = 4; // LONG
        tiff[18] = 2; // count 2 -> 8 bytes, out of line; the offset field would start at 22 (past the end)

        assert_eq!(try_get_value_span(&tiff, 14, 4, 2, true), None);
    }

    #[test]
    // C#: TiffStructureValueSpanTests.ShortArray_OutOfLine_ReturnsEveryElement
    fn short_array_out_of_line_returns_every_element() {
        let mut tiff = build(3, 5, 600);
        for i in 0..5 {
            tiff[DATA + i * 2] = (i + 1) as u8;
        }

        let value = span(&tiff, 3, 5).expect("out-of-line SHORT array is read");

        assert_eq!(value.len(), 10);
        assert_eq!(read_unsigned(&value, 3, true), Some(1)); // scalar callers still read the first element
    }

    #[test]
    // C#: TiffStructureValueSpanTests.RationalArray_ReturnsAllRationals
    fn rational_array_returns_all_rationals() {
        let value = span(&build(5, 3, 600), 5, 3).expect("RATIONAL x 3 is read");

        assert_eq!(value.len(), 24);
    }

    #[test]
    // C#: TiffStructureValueSpanTests.Undefined_OutOfLine_ReturnsAllBytesAndReadsAsText
    fn undefined_out_of_line_returns_all_bytes_and_reads_as_text() {
        let mut tiff = build(7, 12, 600);
        tiff[DATA..DATA + 12].copy_from_slice(b"Canon EOS M\0");

        let value = span(&tiff, 7, 12).expect("UNDEFINED x 12 is read");

        assert_eq!(value.len(), 12);
        assert_eq!(read_ascii(&value, 7), Some("Canon EOS M".to_owned()));
    }

    #[test]
    // C#: TiffStructureValueSpanTests.ReadAscii_UndefinedValueLongerThanTheTextCap_IsClampedToTheAsciiCap
    fn read_ascii_undefined_longer_than_text_cap_is_clamped_to_ascii_cap() {
        let value = vec![b'A'; 4000]; // UNDEFINED spans may be up to MaxValueBytes, with no NUL

        assert_eq!(read_ascii(&value, 7).map(|s| s.len()), Some(MAX_ASCII_BYTES));
    }

    #[test]
    // C#: TiffStructureValueSpanTests.Ascii_LongerThanTheCap_IsClampedNotRejected
    fn ascii_longer_than_cap_is_clamped_not_rejected() {
        let mut tiff = build(2, 1000, 2000);
        tiff[DATA..DATA + 1000].fill(b'A');

        let value = span(&tiff, 2, 1000).expect("long ASCII is clamped, not rejected");

        assert_eq!(value.len(), MAX_ASCII_BYTES);
    }

    #[test]
    // C#: TiffStructureValueSpanTests.Undefined_LongerThanTheValueCap_IsClampedToTheCap
    fn undefined_longer_than_value_cap_is_clamped_to_cap() {
        let tiff = build(7, 100_000, 6000);

        let value = span(&tiff, 7, 100_000).expect("long UNDEFINED is clamped to the cap");

        assert_eq!(value.len(), MAX_VALUE_BYTES);
    }

    #[test]
    // C#: TiffStructureValueSpanTests.Text_RunningPastTheEndOfTheBlock_IsRejected (rows 2 and 7)
    fn text_running_past_end_of_block_is_rejected() {
        for field_type in [2u16, 7] {
            let tiff = build(field_type, 200, DATA + 30);
            assert_eq!(span(&tiff, field_type, 200), None, "type {field_type}");
        }
    }

    #[test]
    // C#: TiffStructureValueSpanTests.Array_ClaimingMoreThanTheBlockHolds_KeepsTheWholeElementsThatExist
    fn array_claiming_more_than_block_holds_keeps_whole_elements_that_exist() {
        let value = span(&build(4, 50, DATA + 11), 4, 50).expect("two whole LONGs exist");

        assert_eq!(value.len(), 8);
    }

    #[test]
    // C#: TiffStructureValueSpanTests.OffsetPastTheBlock_IsRejected (rows 2, 3 and 7)
    fn offset_past_block_is_rejected() {
        for field_type in [2u16, 3, 7] {
            let mut tiff = build(field_type, 40, 600);
            tiff[ENTRY as usize + 8] = 0xFF;
            tiff[ENTRY as usize + 9] = 0xFF; // offset 0xFFFF > length
            assert_eq!(span(&tiff, field_type, 40), None, "type {field_type}");
        }
    }

    #[test]
    // C#: TiffStructureValueSpanTests.ArrayWithNoWholeElementInTheBlock_IsRejected
    fn array_with_no_whole_element_in_block_is_rejected() {
        assert_eq!(span(&build(4, 50, DATA + 3), 4, 50), None);
    }

    #[test]
    // C#: TiffStructureValueSpanTests.InlineValues_AreUnchanged
    fn inline_values_are_unchanged() {
        let tiff = TiffBytes::new(true, 100)
            .header(8)
            .ifd(8, 0, &[short(0x1234, 800)])
            .finish();

        let value = try_get_value_span(&tiff, ENTRY, 3, 1, true).expect("inline SHORT is read");

        assert_eq!(value.len(), 2);
        assert_eq!(read_unsigned(value, 3, true), Some(800));
    }

    // ------------------------------------------------------------------ ExifMetadataMutationGapTests (TiffStructure half)

    #[test]
    // C#: ExifMetadataMutationGapTests.TryReadHeader_ByteOrderMarkWithOnlyOneMatchingLetter_IsRejected
    fn try_read_header_byte_order_mark_with_only_one_matching_letter_is_rejected() {
        assert_eq!(
            try_read_header(b"II*\0\x08\0\0\0"),
            Some(TiffHeader {
                little_endian: true,
                magic: 42,
                ifd0_offset: 8
            })
        );
        assert_eq!(
            try_read_header(b"MM\0*\0\0\0\x08"),
            Some(TiffHeader {
                little_endian: false,
                magic: 42,
                ifd0_offset: 8
            })
        );
        assert_eq!(try_read_header(b"IM*\0\x08\0\0\0"), None);
        assert_eq!(try_read_header(b"MI\0*\0\0\0\x08"), None);
    }

    #[test]
    // C#: ExifMetadataMutationGapTests.TryGetValueSpan_EntryThatIsExactlyTheWholeBlock_IsRead (rows SHORT x 1, LONG x 1)
    fn try_get_value_span_entry_exactly_whole_block_is_read() {
        for (field_type, expected_len) in [(3u16, 2usize), (4, 4)] {
            let block = fake_entry(0x0112, field_type, 1, 0x0000_1234);

            let value = try_get_value_span(&block, 0, field_type, 1, true).expect("inline value is read");

            assert_eq!(value.len(), expected_len, "type {field_type}");
            assert_eq!(value[0], 0x34, "type {field_type}");
        }
    }

    #[test]
    // C#: ExifMetadataMutationGapTests.TryGetValueSpan_UnknownTypeOrZeroCount_HasNoValue (rows (0, 1) and (3, 0))
    fn try_get_value_span_unknown_type_or_zero_count_has_no_value() {
        for (field_type, count) in [(0u16, 1u32), (3, 0)] {
            let block = fake_entry(0x0112, field_type, count, 0x0000_1234);

            assert_eq!(
                try_get_value_span(&block, 0, field_type, count, true),
                None,
                "type {field_type} count {count}"
            );
        }
    }

    #[test]
    // C#: ExifMetadataMutationGapTests.ReadAscii_ValueStartingWithNul_IsNull
    fn read_ascii_value_starting_with_nul_is_none() {
        assert_eq!(read_ascii(b"\0abc", 2), None);
        assert_eq!(read_ascii(b"ab\0c", 2), Some("ab".to_owned()));
        assert_eq!(read_ascii(b"abc", 7), Some("abc".to_owned()));
    }

    #[test]
    // C#: ExifMetadataMutationGapTests.ReadUnsigned_ValuesOfExactlyTheTypeSize_AreReadAndShorterOnesAreNull
    fn read_unsigned_values_of_exactly_type_size_are_read_and_shorter_ones_are_none() {
        assert_eq!(read_unsigned(&[7], 1, true), Some(7));
        assert_eq!(read_unsigned(&[], 1, true), None);
        assert_eq!(read_unsigned(&[1, 2], 3, true), Some(0x0201));
        assert_eq!(read_unsigned(&[1], 3, true), None);
        assert_eq!(read_unsigned(&[1, 2, 3, 4], 4, true), Some(0x0403_0201));
        assert_eq!(read_unsigned(&[1, 2, 3], 4, true), None);
        assert_eq!(read_unsigned(&[0xFE, 0xFF], 8, true), Some(-2));
        assert_eq!(read_unsigned(&[0xFE], 8, true), None);
        assert_eq!(read_unsigned(&[], 8, true), None);
        assert_eq!(read_unsigned(&[0xFE, 0xFF, 0xFF, 0xFF], 9, true), Some(-2));
        assert_eq!(read_unsigned(&[0xFE, 0xFF, 0xFF], 9, true), None);
        assert_eq!(read_unsigned(&[], 9, true), None);
    }

    #[test]
    // C#: ExifMetadataMutationGapTests.ReadRational_FirstOfSeveralRationals_IsReturnedAndAShortValueIsNull
    fn read_rational_first_of_several_rationals_is_returned_and_a_short_value_is_none() {
        let two: [u8; 16] = [1, 0, 0, 0, 250, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];

        assert_eq!(read_rational(&two, 5, true), Some(ExifRational::new(1, 250)));
        assert_eq!(read_rational(&two[..8], 5, true), Some(ExifRational::new(1, 250)));
        assert_eq!(read_rational(&two[..7], 5, true), None);
    }

    #[test]
    // C#: ExifMetadataMutationGapTests.ReadRational_SignedRational_NeedsPositiveNumeratorAndDenominator (6 rows)
    fn read_rational_signed_rational_needs_positive_numerator_and_denominator() {
        let rows: [(i32, i32, bool); 6] = [
            (1, 250, true),
            (0, 250, false),
            (1, 0, false),
            (-1, 250, false),
            (1, -250, false),
            (-1, -250, false),
        ];
        for (numerator, denominator, expected_value) in rows {
            let mut bytes = Vec::with_capacity(8);
            bytes.extend_from_slice(&numerator.to_le_bytes());
            bytes.extend_from_slice(&denominator.to_le_bytes());

            let want = expected_value.then(|| ExifRational::new(1, 250));

            assert_eq!(
                read_rational(&bytes, 10, true),
                want,
                "numerator {numerator} denominator {denominator}"
            );
        }
    }

    // ------------------------------------------------------------------ RawHeaderHostileInputTests (TiffStructure section)

    #[test]
    // C#: RawHeaderHostileInputTests.TiffStructure_ReadU16_OffsetNearOrPastEnd_ReturnsZero (4 rows)
    fn read_u16_offset_near_or_past_end_returns_zero() {
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
        for offset in [i32::MAX, i32::MAX - 1, i32::MAX - 3, 8] {
            assert_eq!(read_u16(&data, offset, true), 0, "offset {offset}");
        }
    }

    #[test]
    // C#: RawHeaderHostileInputTests.TiffStructure_ReadU32_OffsetNearOrPastEnd_ReturnsZero (4 rows)
    fn read_u32_offset_near_or_past_end_returns_zero() {
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
        for offset in [i32::MAX, i32::MAX - 2, i32::MAX - 3, 5] {
            assert_eq!(read_u32(&data, offset, false), 0, "offset {offset}");
        }
    }

    #[test]
    // C#: RawHeaderHostileInputTests.TiffStructure_ReadU16AndU32_LastValidOffsets_StillRead
    fn read_u16_and_u32_last_valid_offsets_still_read() {
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8];

        assert_eq!(read_u16(&data, 6, false), 0x0708);
        assert_eq!(read_u32(&data, 4, true), 0x0807_0605);
    }

    // ------------------------------------------------------------------ TiffSubIfdTypeTests (TypeSize section)

    #[test]
    // C#: TiffSubIfdTypeTests.TypeSize_IfdType_IsFourBytesAndReadsAsUnsigned
    fn type_size_ifd_type_is_four_bytes_and_reads_as_unsigned() {
        assert_eq!(type_size(13), 4);
        assert_eq!(read_unsigned(&[4, 3, 2, 1], 13, true), Some(0x0102_0304));
        assert_eq!(type_size(16), 0); // BigTIFF LONG8 stays unsupported
    }

    // ------------------------------------------------------------------ BinaryReaderFuzzTests (TiffStructure half)

    /// Little or big endian IFD0 with one entry of each value shape the readers take (ASCII, RATIONAL, SHORT,
    /// LONG array, UNDEFINED blob, SHORT array).
    fn seed_tiff(little: bool) -> Vec<u8> {
        TiffBytes::new(little, 300)
            .header(8)
            .ifd(
                8,
                0,
                &[
                    at(0x010F, 2, 6, 200),
                    short(0x0112, 6),
                    at(0x011A, 5, 1, 220),
                    at(0x0201, 4, 3, 240),
                    at(0x927C, 7, 16, 260),
                    at(0x0102, 3, 3, 280),
                ],
            )
            .put(200, b"Canon\0")
            .put(220, &[1, 0, 0, 0, 250, 0, 0, 0])
            .put(240, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])
            .put(260, b"Canon EOS M\0\0\0\0\0")
            .put(280, &[1, 0, 2, 0, 3, 0])
            .finish()
    }

    /// `xorshift64`, so the mutation schedule is deterministic without the `rand` crate.
    struct XorShift(u64);

    impl XorShift {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// Flips 1 to 8 bytes, and sometimes truncates the block.
    fn mutate(block: &mut Vec<u8>, rng: &mut XorShift) {
        for _ in 0..=rng.below(8) {
            let at = rng.below(block.len());
            block[at] ^= (rng.next_u64() as u8) | 1;
        }
        if rng.below(4) == 0 {
            let keep = rng.below(block.len() + 1);
            block.truncate(keep);
        }
    }

    /// Port of `BinaryReaderFuzzTests.WalkEveryEntry`: drives every TiffStructure reader over each IFD0 entry the way the
    /// parsers do, whatever the (hostile) counts claim.
    fn walk_every_entry(tiff: &[u8]) {
        let Some(header) = try_read_header(tiff) else {
            return;
        };
        let Ok(ifd0) = i32::try_from(header.ifd0_offset) else {
            return;
        };
        let little = header.little_endian;
        let count = read_u16(tiff, ifd0, little);
        for i in 0..usize::from(count).min(256) {
            let entry = ifd0.wrapping_add(2 + 12 * i as i32);
            let field_type = read_u16(tiff, entry.wrapping_add(2), little);
            let n = read_u32(tiff, entry.wrapping_add(4), little);
            if let Some(value) = try_get_value_span(tiff, entry, field_type, n, little) {
                let _ = read_ascii(value, field_type);
                let _ = read_unsigned(value, field_type, little);
                let _ = read_rational(value, field_type, little);
            }
        }
    }

    #[test]
    // C#: BinaryReaderFuzzTests.Fuzz_ExifParserAndTiffStructure_NeverThrow (TiffStructure half)
    fn tiff_value_readers_never_panic_on_mutated_blocks() {
        for (little, seed) in [(true, 4100_u64), (false, 4101_u64)] {
            let base = seed_tiff(little);
            let mut rng = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            for _ in 0..4000 {
                let mut block = base.clone();
                mutate(&mut block, &mut rng);
                walk_every_entry(&block);
            }
        }
    }

    // ------------------------------------------------------------------ golden: tests/golden/tiff-structure.json

    fn from_hex(text: &str) -> Vec<u8> {
        assert!(text.len() % 2 == 0, "odd hex length in {text:?}");
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex digit"))
            .collect()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The `data` node of a golden input: `{"hex": ...}` or `{"len": n, "patches": [{"at", "hex"}]}` (zero filled).
    fn data_bytes(node: &Value) -> Vec<u8> {
        if let Some(text) = node["hex"].as_str() {
            return from_hex(text);
        }
        let len = node["len"].as_u64().expect("data len") as usize;
        let mut bytes = vec![0u8; len];
        for patch in node["patches"].as_array().into_iter().flatten() {
            let at = patch["at"].as_u64().expect("patch at") as usize;
            let raw = from_hex(patch["hex"].as_str().expect("patch hex"));
            bytes[at..at + raw.len()].copy_from_slice(&raw);
        }
        bytes
    }

    fn int_of(node: &Value, key: &str) -> i64 {
        node[key]
            .as_i64()
            .unwrap_or_else(|| panic!("no integer '{key}' in {node}"))
    }

    fn i32_of(node: &Value, key: &str) -> i32 {
        i32::try_from(int_of(node, key)).expect("i32 input")
    }

    fn u32_of(node: &Value, key: &str) -> u32 {
        u32::try_from(int_of(node, key)).expect("u32 input")
    }

    fn u16_of(node: &Value, key: &str) -> u16 {
        u16::try_from(int_of(node, key)).expect("u16 input")
    }

    fn flag(node: &Value, key: &str) -> bool {
        node[key]
            .as_bool()
            .unwrap_or_else(|| panic!("no bool '{key}' in {node}"))
    }

    fn expect_eq<T: PartialEq + std::fmt::Debug>(what: &str, got: T, want: T) -> Result<(), String> {
        if got == want {
            Ok(())
        } else {
            Err(format!("{what}: got {got:?}, want {want:?}"))
        }
    }

    /// Replays one golden case against the port. `Err` names the mismatch; a panic (e.g. `todo!()`) is caught by the caller.
    fn check_case(op: &str, out: &Value, input: &Value) -> Result<(), String> {
        match op {
            "header" => {
                let got = try_read_header(&data_bytes(&input["data"]));
                let want = out["ok"].as_bool().expect("ok").then(|| TiffHeader {
                    little_endian: flag(out, "little"),
                    magic: int_of(out, "magic") as u16,
                    ifd0_offset: int_of(out, "ifd0") as u32,
                });
                expect_eq("header", got, want)
            }
            "type-size" => expect_eq(
                "type-size",
                Some(i64::from(type_size(u16_of(input, "type")))),
                Some(int_of(out, "size")),
            ),
            "u16" => expect_eq(
                "u16",
                Some(i64::from(read_u16(
                    &data_bytes(&input["data"]),
                    i32_of(input, "offset"),
                    flag(input, "little"),
                ))),
                Some(int_of(out, "value")),
            ),
            "u32" => expect_eq(
                "u32",
                Some(i64::from(read_u32(
                    &data_bytes(&input["data"]),
                    i32_of(input, "offset"),
                    flag(input, "little"),
                ))),
                Some(int_of(out, "value")),
            ),
            "value-span" => {
                let data = data_bytes(&input["data"]);
                let got = try_get_value_span(
                    &data,
                    i32_of(input, "entry_offset"),
                    u16_of(input, "type"),
                    u32_of(input, "count"),
                    flag(input, "little"),
                )
                .map(|s| (s.len() as i64, hex(s)));
                let want = out["ok"]
                    .as_bool()
                    .expect("ok")
                    // The oracle writes upper-case hex (Convert.ToHexString); `hex()` writes lower-case.
                    .then(|| {
                        (
                            int_of(out, "length"),
                            out["hex"].as_str().expect("hex").to_ascii_lowercase(),
                        )
                    });
                expect_eq("value-span", got, want)
            }
            "ascii" => expect_eq(
                "ascii",
                read_ascii(&data_bytes(&input["data"]), u16_of(input, "type")),
                out["text"].as_str().map(str::to_owned),
            ),
            "unsigned" => expect_eq(
                "unsigned",
                read_unsigned(
                    &data_bytes(&input["data"]),
                    u16_of(input, "type"),
                    flag(input, "little"),
                ),
                out["value"].as_i64(),
            ),
            "rational" => {
                let got = read_rational(
                    &data_bytes(&input["data"]),
                    u16_of(input, "type"),
                    flag(input, "little"),
                )
                .map(|r| (r.numerator, r.denominator));
                let want = (!out["value"].is_null())
                    .then(|| (int_of(&out["value"], "n") as u32, int_of(&out["value"], "d") as u32));
                expect_eq("rational", got, want)
            }
            other => Err(format!("unknown op {other}")),
        }
    }

    fn golden_inputs() -> HashMap<String, Value> {
        let path = golden::repo_root()
            .join("tests")
            .join("golden-inputs")
            .join("tiff-structure.json");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let doc: Value = serde_json::from_str(&text).expect("golden-inputs JSON");
        doc["cases"]
            .as_array()
            .expect("golden-inputs cases")
            .iter()
            .map(|c| (golden::str_of(c, "name").to_owned(), c.clone()))
            .collect()
    }

    #[test]
    // Golden twin of tests/golden/tiff-structure.json (oracle-cs tiff-structure, 212 cases; inputs in golden-inputs/).
    fn golden_tiff_structure() {
        let golden = golden::load("tiff-structure");
        let inputs = golden_inputs();
        let mut failures: Vec<String> = Vec::new();
        for out in &golden.cases {
            let name = golden::str_of(out, "name");
            let op = golden::str_of(out, "op");
            let input = inputs.get(name).unwrap_or_else(|| panic!("no input for case {name}"));
            match std::panic::catch_unwind(AssertUnwindSafe(|| check_case(op, out, input))) {
                Ok(Ok(())) => {}
                Ok(Err(why)) => failures.push(format!("{name} ({why})")),
                Err(_) => failures.push(format!("{name} (panicked)")),
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} golden cases failed: {}",
            failures.len(),
            golden.cases.len(),
            failures.join(", ")
        );
    }
}
