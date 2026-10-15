//! Port of `PhotoReview.Imaging/Metadata/ExifSummary.cs` and `ExifSummaryCodec.cs` (C# baseline d92d6aae).
//!
//! * [`ExifSummary`] / [`ExifRational`]: the handful of EXIF fields the photo information line shows. Every field is
//!   optional and already sanitised (trimmed, length-capped, zero or invalid values dropped). [`ExifSummary::create`] is
//!   the only sanitising entry point; it never fails and returns `None` when nothing usable is left.
//! * [`codec`]: the compact binary form stored in the preview disk-cache entry (PreviewCacheFile v7, PARITY D-06).
//!   It is a PERSISTED FORMAT: byte-identical to C#, little-endian, version byte 1.
//!
//! `DateTaken` is a .NET `DateTime` of kind Unspecified (a camera wall-clock time, no zone). It is carried as
//! [`DateTimeUtc`] purely for its tick arithmetic and 0..=`DateTime.MaxValue` range; it is NOT a UTC instant and must
//! never be converted with a time zone. Equality compares ticks (C# record equality ignores `Kind`).
//!
//! Text lengths follow C# UTF-16 semantics: [`MAX_TEXT_LENGTH`] counts UTF-16 code units, not Unicode scalar values.
#![allow(unused_variables, dead_code)] // D11: remove when the bodies are filled in.

use pr_core::time::DateTimeUtc;

/// Longest camera/lens text kept, in UTF-16 code units (C# `ExifSummary.MaxTextLength`).
pub const MAX_TEXT_LENGTH: usize = 64;

/// Largest value accepted for ISO (`Create`: `iso is > 0 and <= 10_000_000`).
pub const MAX_ISO: i64 = 10_000_000;

// D30: `ExifRational` lives in `crate::tiff` (W4-03a, `read_rational` returns it); re-exported here for W4-04a users.
pub use crate::tiff::ExifRational;

/// Raw tag values handed to [`ExifSummary::create`] (the C# method has eight positional parameters; a struct keeps
/// clippy's `too_many_arguments` quiet and the call sites readable). `None` = tag absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawExifTags<'a> {
    /// "YYYY:MM:DD HH:MM:SS" and the other accepted spellings, see [`ExifSummary::parse_date`].
    pub date_taken: Option<&'a str>,
    pub make: Option<&'a str>,
    pub model: Option<&'a str>,
    pub lens: Option<&'a str>,
    /// Any width: out-of-range values (<= 0 or > [`MAX_ISO`]) are dropped.
    pub iso: Option<i64>,
    pub focal_length: Option<ExifRational>,
    pub f_number: Option<ExifRational>,
    pub exposure_time: Option<ExifRational>,
}

/// The EXIF fields shown by the photo information line (C# `record ExifSummary`). All fields are public like the C#
/// init-only properties, so a value can be built unsanitised (tests do); only [`ExifSummary::create`] and
/// [`codec::decode`] guarantee the sanitised invariants.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ExifSummary {
    /// Camera wall-clock time, kind Unspecified (see the module note).
    pub date_taken: Option<DateTimeUtc>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_model: Option<String>,
    pub iso: Option<i32>,
    /// Focal length in millimetres.
    pub focal_length: Option<ExifRational>,
    /// Aperture as an f-number (2.8 = f/2.8).
    pub f_number: Option<ExifRational>,
    /// Exposure time in seconds.
    pub exposure_time: Option<ExifRational>,
}

impl ExifSummary {
    /// True when every field is `None` (note: `Some("")` text is NOT empty here, as in C#).
    pub fn is_empty(&self) -> bool {
        todo!("W4-04a")
    }

    /// Builds a sanitised summary from raw tag values; `None` when nothing usable is left. Never panics.
    ///
    /// date via [`Self::parse_date`]; texts via [`Self::clean_text`]; `iso` kept only when `1..=MAX_ISO`; a rational
    /// is kept only when BOTH numerator and denominator are > 0.
    pub fn create(raw: &RawExifTags<'_>) -> Option<ExifSummary> {
        todo!("W4-04a")
    }

    /// C# `ExifSummary.ParseDate` (internal). `TryParseExact` of the cleaned text against, in order,
    /// `yyyy:MM:dd HH:mm:ss`, `yyyy:MM:dd HH:mm`, `yyyy-MM-dd HH:mm:ss`, `yyyy:MM:dd`, invariant culture,
    /// `DateTimeStyles.None`. Garbage and the `0000:00:00 ...` placeholder give `None`.
    pub fn parse_date(text: Option<&str>) -> Option<DateTimeUtc> {
        todo!("W4-04a")
    }

    /// C# `ExifSummary.CleanText` (internal). Stops at the first NUL, replaces control characters (Unicode Cc) by a
    /// space, caps at [`MAX_TEXT_LENGTH`] UTF-16 units, trims, drops a trailing half surrogate pair, empty = `None`.
    pub fn clean_text(text: Option<&str>) -> Option<String> {
        todo!("W4-04a")
    }
}

/// C# `ExifSummaryCodec` (internal in C#; `pub` here so integration tests and the cache-file unit can call it).
pub mod codec {
    use super::ExifSummary;

    /// Version byte of the encoded block (C# `FormatVersion`).
    pub const FORMAT_VERSION: u8 = 1;

    /// Upper bound of an encoded summary (C# `MaxEncodedLength`); the cache header stores the length as a u16.
    pub const MAX_ENCODED_LENGTH: usize = 1024;

    /// Presence-mask bits in field order (C# `[Flags] enum Field : byte`).
    pub mod field {
        pub const DATE: u8 = 1;
        pub const MAKE: u8 = 2;
        pub const MODEL: u8 = 4;
        pub const LENS: u8 = 8;
        pub const ISO: u8 = 16;
        pub const FOCAL: u8 = 32;
        pub const F_NUMBER: u8 = 64;
        pub const EXPOSURE: u8 = 128;
    }

    /// `[]` for `None` or an empty summary; otherwise `[FORMAT_VERSION, mask, fields...]` (see the contract).
    pub fn encode(summary: Option<&ExifSummary>) -> Vec<u8> {
        todo!("W4-04a")
    }

    /// Inverse of [`encode`]; `None` for an empty, unknown-version or malformed block, and when nothing usable
    /// survives re-sanitising. Never panics.
    pub fn decode(data: &[u8]) -> Option<ExifSummary> {
        todo!("W4-04a")
    }
}
