//! Port of `PhotoReview.Imaging/Metadata/ExifQueryInterpreter.cs` (C# baseline d92d6aae).
//!
//! Maps the results of WIC metadata queries (`/app1/ifd/{ushort=271}` ...) to an [`ExifSummary`]. Shared by the
//! WIC decoders (W4-11c, W4-11d) so every decode path reads the same tags the same way. The C# `object?` query
//! result is modelled by the closed enum [`QueryValue`]; WIC value types: ASCII = string, SHORT = u16 (array for
//! several), LONG = u32, RATIONAL = u64 (low 32 bits numerator, high 32 bits denominator), SRATIONAL = i64.
//!
//! Pure logic: no OS, no `unsafe`, no I/O.
#![allow(unused_variables, dead_code, unused_imports)] // D11: remove with the real bodies.

use crate::exif_parser::{
    TAG_DATE_TIME, TAG_DATE_TIME_DIGITIZED, TAG_DATE_TIME_ORIGINAL, TAG_EXPOSURE_TIME, TAG_F_NUMBER, TAG_FOCAL_LENGTH,
    TAG_ISO, TAG_LENS_MODEL, TAG_MAKE, TAG_MODEL,
};
use crate::exif_summary::{ExifRational, ExifSummary};

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
    todo!("W4-04b")
}

/// `{ifd_root}/{ushort=<tag decimal>}` (IFD0 tag).
pub(crate) fn ifd0_path(ifd_root: &str, tag: u16) -> String {
    todo!("W4-04b")
}

/// `{ifd_root}/exif/{ushort=<tag decimal>}` (EXIF sub-IFD tag).
pub(crate) fn exif_path(ifd_root: &str, tag: u16) -> String {
    todo!("W4-04b")
}

/// `ExifQueryInterpreter.AsString`: `Text` as is, `Chars` as the string of those code units, anything else `None`.
pub(crate) fn as_string(value: Option<&QueryValue>) -> Option<String> {
    todo!("W4-04b")
}

/// `ExifQueryInterpreter.AsInteger`: scalar byte/u16/i16/u32/i32 widened to `i64`; non-empty arrays of
/// u16/u32/i16/i32 give element 0; everything else `None`.
pub fn as_integer(value: Option<&QueryValue>) -> Option<i64> {
    todo!("W4-04b")
}

/// `ExifQueryInterpreter.AsRational`: `U64` / first of `U64s` = unsigned packed (numerator low 32 bits,
/// denominator high 32, zeros kept: `ExifSummary::create` drops them); `I64` / first of `I64s` = signed packed,
/// kept only when both halves are `> 0` as `i32`; everything else `None`.
pub(crate) fn as_rational(value: Option<&QueryValue>) -> Option<ExifRational> {
    todo!("W4-04b")
}
