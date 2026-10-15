//! Port of `PhotoReview.Imaging/Metadata/ExifQueryInterpreter.cs` (C# baseline d92d6aae).
//!
//! Maps the results of WIC metadata queries (`/app1/ifd/{ushort=271}` ...) to an [`ExifSummary`]. Shared by the
//! WIC decoders (W4-11c, W4-11d) so every decode path reads the same tags the same way. The C# `object?` query
//! result is modelled by the closed enum [`QueryValue`]; WIC value types: ASCII = string, SHORT = u16 (array for
//! several), LONG = u32, RATIONAL = u64 (low 32 bits numerator, high 32 bits denominator), SRATIONAL = i64.
//!
//! Pure logic: no OS, no `unsafe`, no I/O.

use crate::exif_parser::{
    TAG_DATE_TIME, TAG_DATE_TIME_DIGITIZED, TAG_DATE_TIME_ORIGINAL, TAG_EXPOSURE_TIME, TAG_F_NUMBER, TAG_FOCAL_LENGTH,
    TAG_ISO, TAG_LENS_MODEL, TAG_MAKE, TAG_MODEL,
};
use crate::exif_summary::{ExifRational, ExifSummary, RawExifTags};

/// IFD root of the EXIF block inside APP1 for JPEG (`ExifQueryInterpreter.JpegIfdRoot`).
pub const JPEG_IFD_ROOT: &str = "/app1/ifd";
/// IFD root for TIFF, where the file itself is the IFD (`ExifQueryInterpreter.TiffIfdRoot`).
pub const TIFF_IFD_ROOT: &str = "/ifd";

/// One metadata-query result (the C# `object?`; `None` of `Option<QueryValue>` is C# `null` = absent/unreadable).
///
/// Each variant is a distinct CLR runtime type the C# `switch` tells apart; keep them separate (see contract trap 2).
#[derive(Debug, Clone, PartialEq)]
pub enum QueryValue {
    /// `string` (WIC ASCII).
    Text(String),
    /// `char[]` as UTF-16 code units.
    Chars(Vec<u16>),
    /// `byte`.
    U8(u8),
    /// `ushort`.
    U16(u16),
    /// `short`.
    I16(i16),
    /// `uint`.
    U32(u32),
    /// `int`.
    I32(i32),
    /// `ushort[]`.
    U16s(Vec<u16>),
    /// `uint[]`.
    U32s(Vec<u32>),
    /// `short[]`.
    I16s(Vec<i16>),
    /// `int[]`.
    I32s(Vec<i32>),
    /// `ulong` (packed unsigned RATIONAL).
    U64(u64),
    /// `long` (packed SRATIONAL).
    I64(i64),
    /// `ulong[]`.
    U64s(Vec<u64>),
    /// `long[]`.
    I64s(Vec<i64>),
    /// Any other runtime type (double, bool, byte[], ...): never accepted by any of the three conversions.
    Other,
}

/// Reads the eight summary fields through `query` (path in, value out; `None` = absent). `ifd_root` is
/// [`JPEG_IFD_ROOT`] or [`TIFF_IFD_ROOT`]. Returns `None` when nothing usable is left (`ExifSummary::create`).
///
/// Query order (observable through a recording `query`): date taken tries EXIF `DateTimeOriginal`, then EXIF
/// `DateTimeDigitized`, then IFD0 `DateTime`, stopping at the first value that converts to a string (an empty
/// string stops the chain too); the other seven fields are always queried, in the order make, model, lens, iso,
/// focal length, f-number, exposure time.
pub fn read(query: &dyn Fn(&str) -> Option<QueryValue>, ifd_root: &str) -> Option<ExifSummary> {
    // Date chain: the first value that converts to a string wins, even an empty one (`Some("")` stops the chain).
    let date = as_string(query(&exif_path(ifd_root, TAG_DATE_TIME_ORIGINAL)).as_ref())
        .or_else(|| as_string(query(&exif_path(ifd_root, TAG_DATE_TIME_DIGITIZED)).as_ref()))
        .or_else(|| as_string(query(&ifd0_path(ifd_root, TAG_DATE_TIME)).as_ref()));
    let make = as_string(query(&ifd0_path(ifd_root, TAG_MAKE)).as_ref());
    let model = as_string(query(&ifd0_path(ifd_root, TAG_MODEL)).as_ref());
    let lens = as_string(query(&exif_path(ifd_root, TAG_LENS_MODEL)).as_ref());
    let iso = as_integer(query(&exif_path(ifd_root, TAG_ISO)).as_ref());
    let focal_length = as_rational(query(&exif_path(ifd_root, TAG_FOCAL_LENGTH)).as_ref());
    let f_number = as_rational(query(&exif_path(ifd_root, TAG_F_NUMBER)).as_ref());
    let exposure_time = as_rational(query(&exif_path(ifd_root, TAG_EXPOSURE_TIME)).as_ref());

    ExifSummary::create(&RawExifTags {
        date_taken: date.as_deref(),
        make: make.as_deref(),
        model: model.as_deref(),
        lens: lens.as_deref(),
        iso,
        focal_length,
        f_number,
        exposure_time,
    })
}

/// `{ifd_root}/{ushort=<tag decimal>}` (IFD0 tag).
pub(crate) fn ifd0_path(ifd_root: &str, tag: u16) -> String {
    format!("{ifd_root}/{{ushort={tag}}}")
}

/// `{ifd_root}/exif/{ushort=<tag decimal>}` (EXIF sub-IFD tag).
pub(crate) fn exif_path(ifd_root: &str, tag: u16) -> String {
    format!("{ifd_root}/exif/{{ushort={tag}}}")
}

/// `ExifQueryInterpreter.AsString`: `Text` as is, `Chars` as the string of those code units, anything else `None`.
// DIVERGENCE: a lone surrogate becomes U+FFFD here, where C# keeps it. Both end up as U+FFFD once the persisted
// cache codec (W4-04a) writes UTF-8, so the stored bytes match.
pub(crate) fn as_string(value: Option<&QueryValue>) -> Option<String> {
    match value? {
        QueryValue::Text(text) => Some(text.clone()),
        QueryValue::Chars(units) => Some(String::from_utf16_lossy(units)),
        _ => None,
    }
}

/// `ExifQueryInterpreter.AsInteger`: scalar byte/u16/i16/u32/i32 widened to `i64`; non-empty arrays of
/// u16/u32/i16/i32 give element 0; everything else `None`. Public for the WIC EXIF reader (amendment A3).
pub fn as_integer(value: Option<&QueryValue>) -> Option<i64> {
    match value? {
        QueryValue::U8(v) => Some(i64::from(*v)),
        QueryValue::U16(v) => Some(i64::from(*v)),
        QueryValue::I16(v) => Some(i64::from(*v)),
        QueryValue::U32(v) => Some(i64::from(*v)),
        QueryValue::I32(v) => Some(i64::from(*v)),
        QueryValue::U16s(values) => values.first().map(|v| i64::from(*v)),
        QueryValue::U32s(values) => values.first().map(|v| i64::from(*v)),
        // CLR quirk, kept on purpose: a `short[]` also passes the C# `ushort[]` type test and an `int[]` passes the
        // `uint[]` test, so element 0 is read as the unsigned bit pattern (-1 -> 65535, -400 -> 4294966896).
        QueryValue::I16s(values) => values.first().map(|v| i64::from(*v as u16)),
        QueryValue::I32s(values) => values.first().map(|v| i64::from(*v as u32)),
        QueryValue::U64(_) | QueryValue::I64(_) | QueryValue::U64s(_) | QueryValue::I64s(_) => None,
        QueryValue::Text(_) | QueryValue::Chars(_) | QueryValue::Other => None,
    }
}

/// `ExifQueryInterpreter.AsRational`: `U64` / first of `U64s` = unsigned packed (numerator low 32 bits,
/// denominator high 32, zeros kept: `ExifSummary::create` drops them); `I64` / first of `I64s` = signed packed,
/// kept only when both halves are `> 0` as `i32`; everything else `None`.
pub(crate) fn as_rational(value: Option<&QueryValue>) -> Option<ExifRational> {
    match value? {
        QueryValue::U64(packed) => Some(unsigned_rational(*packed)),
        QueryValue::U64s(values) => values.first().map(|packed| unsigned_rational(*packed)),
        QueryValue::I64(packed) => signed_rational(*packed),
        QueryValue::I64s(values) => values.first().and_then(|packed| signed_rational(*packed)),
        _ => None,
    }
}

/// Splits an unsigned packed RATIONAL: numerator in the low 32 bits, denominator in the high 32 bits.
fn unsigned_rational(packed: u64) -> ExifRational {
    // The masks and shifts make both casts exact: the value fits in 32 bits.
    ExifRational::new((packed & 0xFFFF_FFFF) as u32, (packed >> 32) as u32)
}

/// Splits a signed packed SRATIONAL. The low half is read through `u32` then `i32` (sign kept), the high half is an
/// arithmetic shift then a truncating cast (C# `unchecked((int)(packed >> 32))`). Kept only when both halves are `> 0`.
fn signed_rational(packed: i64) -> Option<ExifRational> {
    // Intentional sign-bit reinterpretation of the low half (C# `(int)(uint)packed`).
    let numerator = (packed & 0xFFFF_FFFF) as u32 as i32;
    // Intentional truncation to the high 32 bits after the arithmetic shift (C# `unchecked((int)(packed >> 32))`).
    let denominator = (packed >> 32) as i32;
    (numerator > 0 && denominator > 0).then(|| ExifRational::new(numerator as u32, denominator as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Packed unsigned RATIONAL as WIC returns it: numerator in the low 32 bits, denominator in the high 32 bits.
    fn packed(numerator: u32, denominator: u32) -> u64 {
        (u64::from(denominator) << 32) | u64::from(numerator)
    }

    /// Packed SRATIONAL as the C# test builds it: `((long)denominator << 32) | (uint)numerator`.
    fn packed_signed(numerator: i32, denominator: i32) -> i64 {
        (i64::from(denominator) << 32) | i64::from(numerator as u32)
    }

    /// Second element of the signed arrays in the C# tests (`0x0000_0001_0000_0001L`, a valid positive rational).
    const SIGNED_SECOND_ELEMENT: i64 = 0x0000_0001_0000_0001;

    fn integer(value: QueryValue) -> Option<i64> {
        as_integer(Some(&value))
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_EmptyArray_IsNull (Array.Empty<ushort>())
    #[test]
    fn as_integer_empty_array_is_null_u16() {
        assert_eq!(integer(QueryValue::U16s(Vec::new())), None);
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_EmptyArray_IsNull (Array.Empty<uint>())
    #[test]
    fn as_integer_empty_array_is_null_u32() {
        assert_eq!(integer(QueryValue::U32s(Vec::new())), None);
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_EmptyArray_IsNull (Array.Empty<short>())
    #[test]
    fn as_integer_empty_array_is_null_i16() {
        assert_eq!(integer(QueryValue::I16s(Vec::new())), None);
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_EmptyArray_IsNull (Array.Empty<int>())
    #[test]
    fn as_integer_empty_array_is_null_i32() {
        assert_eq!(integer(QueryValue::I32s(Vec::new())), None);
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_SupportedWicTypes_ReturnTheFirstValue ((ushort)400)
    #[test]
    fn as_integer_supported_wic_types_return_the_first_value_u16() {
        assert_eq!(integer(QueryValue::U16(400)), Some(400));
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_SupportedWicTypes_ReturnTheFirstValue ((short)400)
    #[test]
    fn as_integer_supported_wic_types_return_the_first_value_i16() {
        assert_eq!(integer(QueryValue::I16(400)), Some(400));
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_SupportedWicTypes_ReturnTheFirstValue (400u)
    #[test]
    fn as_integer_supported_wic_types_return_the_first_value_u32() {
        assert_eq!(integer(QueryValue::U32(400)), Some(400));
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_SupportedWicTypes_ReturnTheFirstValue (400, an int)
    #[test]
    fn as_integer_supported_wic_types_return_the_first_value_i32() {
        assert_eq!(integer(QueryValue::I32(400)), Some(400));
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_SupportedWicTypes_ReturnTheFirstValue (new ushort[] { 400, 1 })
    #[test]
    fn as_integer_supported_wic_types_return_the_first_value_u16_array() {
        assert_eq!(integer(QueryValue::U16s(vec![400, 1])), Some(400));
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_SupportedWicTypes_ReturnTheFirstValue (new uint[] { 400, 1 })
    #[test]
    fn as_integer_supported_wic_types_return_the_first_value_u32_array() {
        assert_eq!(integer(QueryValue::U32s(vec![400, 1])), Some(400));
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_SupportedWicTypes_ReturnTheFirstValue (new short[] { 400, 1 })
    #[test]
    fn as_integer_supported_wic_types_return_the_first_value_i16_array() {
        assert_eq!(integer(QueryValue::I16s(vec![400, 1])), Some(400));
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_SupportedWicTypes_ReturnTheFirstValue (new int[] { 400, 1 })
    #[test]
    fn as_integer_supported_wic_types_return_the_first_value_i32_array() {
        assert_eq!(integer(QueryValue::I32s(vec![400, 1])), Some(400));
    }

    // C#: ExifMetadataMutationGapTests.AsInteger_ByteAndUnsupportedTypes (byte 144 is read; string and null are not)
    #[test]
    fn as_integer_byte_and_unsupported_types() {
        assert_eq!(integer(QueryValue::U8(144)), Some(144));
        assert_eq!(integer(QueryValue::Text("400".to_owned())), None);
        assert_eq!(as_integer(None), None);
    }

    // C#: ExifMetadataMutationGapTests.AsRational_PackedUnsigned_SplitsNumeratorAndDenominator
    #[test]
    fn as_rational_packed_unsigned_splits_numerator_and_denominator() {
        assert_eq!(
            as_rational(Some(&QueryValue::U64(packed(1, 250)))),
            Some(ExifRational::new(1, 250))
        );
        let array = QueryValue::U64s(vec![packed(1, 250), packed(9, 9)]);
        assert_eq!(as_rational(Some(&array)), Some(ExifRational::new(1, 250)));
        assert_eq!(
            as_rational(Some(&QueryValue::U64(packed(u32::MAX, 5)))),
            Some(ExifRational::new(u32::MAX, 5))
        );
    }

    // C#: ExifMetadataMutationGapTests.AsRational_PackedSigned_NeedsPositiveNumeratorAndDenominator (1, 250, true)
    #[test]
    fn as_rational_packed_signed_positive_parts_are_kept() {
        let packed = packed_signed(1, 250);
        assert_eq!(
            as_rational(Some(&QueryValue::I64(packed))),
            Some(ExifRational::new(1, 250))
        );
        let array = QueryValue::I64s(vec![packed, SIGNED_SECOND_ELEMENT]);
        assert_eq!(as_rational(Some(&array)), Some(ExifRational::new(1, 250)));
    }

    // C#: ExifMetadataMutationGapTests.AsRational_PackedSigned_NeedsPositiveNumeratorAndDenominator (0, 250, false)
    #[test]
    fn as_rational_packed_signed_zero_numerator_is_dropped() {
        let packed = packed_signed(0, 250);
        assert_eq!(as_rational(Some(&QueryValue::I64(packed))), None);
        let array = QueryValue::I64s(vec![packed, SIGNED_SECOND_ELEMENT]);
        assert_eq!(as_rational(Some(&array)), None);
    }

    // C#: ExifMetadataMutationGapTests.AsRational_PackedSigned_NeedsPositiveNumeratorAndDenominator (1, 0, false)
    #[test]
    fn as_rational_packed_signed_zero_denominator_is_dropped() {
        let packed = packed_signed(1, 0);
        assert_eq!(as_rational(Some(&QueryValue::I64(packed))), None);
        let array = QueryValue::I64s(vec![packed, SIGNED_SECOND_ELEMENT]);
        assert_eq!(as_rational(Some(&array)), None);
    }

    // C#: ExifMetadataMutationGapTests.AsRational_PackedSigned_NeedsPositiveNumeratorAndDenominator (-1, 250, false)
    #[test]
    fn as_rational_packed_signed_negative_numerator_is_dropped() {
        let packed = packed_signed(-1, 250);
        assert_eq!(as_rational(Some(&QueryValue::I64(packed))), None);
        let array = QueryValue::I64s(vec![packed, SIGNED_SECOND_ELEMENT]);
        assert_eq!(as_rational(Some(&array)), None);
    }

    // C#: ExifMetadataMutationGapTests.AsRational_PackedSigned_NeedsPositiveNumeratorAndDenominator (1, -250, false)
    #[test]
    fn as_rational_packed_signed_negative_denominator_is_dropped() {
        let packed = packed_signed(1, -250);
        assert_eq!(as_rational(Some(&QueryValue::I64(packed))), None);
        let array = QueryValue::I64s(vec![packed, SIGNED_SECOND_ELEMENT]);
        assert_eq!(as_rational(Some(&array)), None);
    }

    // C#: ExifMetadataMutationGapTests.AsRational_PackedSigned_NeedsPositiveNumeratorAndDenominator (-1, -250, false)
    #[test]
    fn as_rational_packed_signed_both_negative_is_dropped() {
        let packed = packed_signed(-1, -250);
        assert_eq!(as_rational(Some(&QueryValue::I64(packed))), None);
        let array = QueryValue::I64s(vec![packed, SIGNED_SECOND_ELEMENT]);
        assert_eq!(as_rational(Some(&array)), None);
    }

    // C#: ExifMetadataMutationGapTests.AsRational_EmptyArraysAndOtherTypes_AreNull
    #[test]
    fn as_rational_empty_arrays_and_other_types_are_null() {
        assert_eq!(as_rational(Some(&QueryValue::U64s(Vec::new()))), None);
        assert_eq!(as_rational(Some(&QueryValue::I64s(Vec::new()))), None);
        assert_eq!(as_rational(Some(&QueryValue::Other)), None);
        assert_eq!(as_rational(None), None);
    }

    // C#: ExifMetadataMutationGapTests.AsString_StringAndCharArray_AreAcceptedOthersAreNull
    #[test]
    fn as_string_string_and_char_array_are_accepted_others_are_null() {
        assert_eq!(as_string(Some(&QueryValue::Text("a".to_owned()))), Some("a".to_owned()));
        let chars = QueryValue::Chars(vec![u16::from(b'a'), u16::from(b'b')]);
        assert_eq!(as_string(Some(&chars)), Some("ab".to_owned()));
        assert_eq!(as_string(Some(&QueryValue::I32(5))), None);
        assert_eq!(as_string(None), None);
    }
}
