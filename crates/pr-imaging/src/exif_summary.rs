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
        self.date_taken.is_none()
            && self.camera_make.is_none()
            && self.camera_model.is_none()
            && self.lens_model.is_none()
            && self.iso.is_none()
            && self.focal_length.is_none()
            && self.f_number.is_none()
            && self.exposure_time.is_none()
    }

    /// Builds a sanitised summary from raw tag values; `None` when nothing usable is left. Never panics.
    ///
    /// date via [`Self::parse_date`]; texts via [`Self::clean_text`]; `iso` kept only when `1..=MAX_ISO`; a rational
    /// is kept only when BOTH numerator and denominator are > 0.
    pub fn create(raw: &RawExifTags<'_>) -> Option<ExifSummary> {
        let summary = ExifSummary {
            date_taken: Self::parse_date(raw.date_taken),
            camera_make: Self::clean_text(raw.make),
            camera_model: Self::clean_text(raw.model),
            lens_model: Self::clean_text(raw.lens),
            iso: raw
                .iso
                .filter(|iso| (1..=MAX_ISO).contains(iso))
                .and_then(|iso| i32::try_from(iso).ok()),
            focal_length: positive(raw.focal_length),
            f_number: positive(raw.f_number),
            exposure_time: positive(raw.exposure_time),
        };
        if summary.is_empty() { None } else { Some(summary) }
    }

    /// C# `ExifSummary.ParseDate` (internal). `TryParseExact` of the cleaned text against, in order,
    /// `yyyy:MM:dd HH:mm:ss`, `yyyy:MM:dd HH:mm`, `yyyy-MM-dd HH:mm:ss`, `yyyy:MM:dd`, invariant culture,
    /// `DateTimeStyles.None`. Garbage and the `0000:00:00 ...` placeholder give `None`.
    pub fn parse_date(text: Option<&str>) -> Option<DateTimeUtc> {
        let cleaned = Self::clean_text(text)?;
        parse_exact(&cleaned)
    }

    /// C# `ExifSummary.CleanText` (internal). Stops at the first NUL, replaces control characters (Unicode Cc) by a
    /// space, caps at [`MAX_TEXT_LENGTH`] UTF-16 units, trims, drops a trailing half surrogate pair, empty = `None`.
    pub fn clean_text(text: Option<&str>) -> Option<String> {
        let text = text.filter(|text| !text.is_empty())?;
        let text = text.split('\0').next().unwrap_or_default();
        let mut kept = String::with_capacity(text.len().min(MAX_TEXT_LENGTH));
        let mut units = 0_usize;
        for ch in text.chars() {
            // A character that would exceed the cap is dropped whole (C# drops the half pair and trims again).
            let width = ch.len_utf16();
            if units + width > MAX_TEXT_LENGTH {
                break;
            }
            units += width;
            kept.push(if ch.is_control() { ' ' } else { ch });
        }
        let trimmed = kept.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    }
}

fn positive(value: Option<ExifRational>) -> Option<ExifRational> {
    value.filter(|rational| rational.numerator > 0 && rational.denominator > 0)
}

/// Strict parser for the four accepted EXIF date spellings on an already cleaned text.
///
/// Date: `yyyy` (4 ASCII digits, 0001..=9999), `:` or `-` (the same one twice), `MM`, the same separator, `dd`; all
/// exactly 2 digits and a real calendar day. Time (optional): exactly one Unicode whitespace character (C# matches a
/// format space against any whitespace, a NBSP included, but not two of them), then `HH:mm:ss`, or `HH:mm` only with
/// `:` date separators. Hour 0..=23, minute and second 0..=59. No zone, fraction, AM/PM or trailing text.
fn parse_exact(text: &str) -> Option<DateTimeUtc> {
    let date = text.as_bytes().get(..10)?;
    let separator = date[4];
    if (separator != b':' && separator != b'-') || date[7] != separator {
        return None;
    }
    let year = ascii_digits(&date[0..4])?;
    let month = ascii_digits(&date[5..7])?;
    let day = ascii_digits(&date[8..10])?;
    if year == 0 || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }

    // Bytes 0..10 are ASCII (checked above), so index 10 is a char boundary.
    let rest = text.get(10..)?;
    let (hour, minute, second) = if rest.is_empty() {
        // `yyyy:MM:dd` only: a dash date needs the time part (`2024-05-01` alone is not accepted).
        if separator != b':' {
            return None;
        }
        (0, 0, 0)
    } else {
        let mut chars = rest.chars();
        if !chars.next()?.is_whitespace() {
            return None;
        }
        let time = chars.as_str().as_bytes();
        match time.len() {
            8 if time[2] == b':' && time[5] == b':' => (
                ascii_digits(&time[0..2])?,
                ascii_digits(&time[3..5])?,
                ascii_digits(&time[6..8])?,
            ),
            5 if separator == b':' && time[2] == b':' => (ascii_digits(&time[0..2])?, ascii_digits(&time[3..5])?, 0),
            _ => return None,
        }
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    DateTimeUtc::from_ticks(civil_ticks(year, month, day, hour, minute, second))
}

/// Exactly `bytes.len()` (1..=4 here) ASCII digits as a number; anything else is `None`.
fn ascii_digits(bytes: &[u8]) -> Option<i64> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(bytes.iter().fold(0_i64, |acc, byte| acc * 10 + i64::from(byte - b'0')))
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

const TICKS_PER_SECOND: i64 = 10_000_000;
const TICKS_PER_DAY: i64 = 864_000_000_000;
/// Days from 0001-01-01 to 1970-01-01 (proleptic Gregorian).
const DAYS_FROM_YEAR_1_TO_1970: i64 = 719_162;

/// .NET ticks of a proleptic Gregorian wall-clock time (days-from-civil; `year >= 1`, so no negative era).
fn civil_ticks(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let year_of_era = y - era * 400;
    let march_based_month = (month + 9) % 12;
    let day_of_year = (153 * march_based_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days_since_1970 = era * 146_097 + day_of_era - 719_468;
    let days = days_since_1970 + DAYS_FROM_YEAR_1_TO_1970;
    let seconds = hour * 3600 + minute * 60 + second;
    days * TICKS_PER_DAY + seconds * TICKS_PER_SECOND
}

/// C# `ExifSummaryCodec` (internal in C#; `pub` here so integration tests and the cache-file unit can call it).
pub mod codec {
    use pr_core::time::DateTimeUtc;

    use super::{ExifRational, ExifSummary, RawExifTags};

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
        let Some(summary) = summary.filter(|summary| !summary.is_empty()) else {
            return Vec::new();
        };
        let mut buffer = Vec::with_capacity(96);
        buffer.extend_from_slice(&[FORMAT_VERSION, 0]);
        let mut mask = 0_u8;

        if let Some(date) = summary.date_taken {
            mask |= field::DATE;
            buffer.extend_from_slice(&date.ticks().to_le_bytes());
        }
        push_text(&mut buffer, &mut mask, field::MAKE, summary.camera_make.as_deref());
        push_text(&mut buffer, &mut mask, field::MODEL, summary.camera_model.as_deref());
        push_text(&mut buffer, &mut mask, field::LENS, summary.lens_model.as_deref());
        if let Some(iso) = summary.iso {
            mask |= field::ISO;
            buffer.extend_from_slice(&iso.to_le_bytes());
        }
        push_rational(&mut buffer, &mut mask, field::FOCAL, summary.focal_length);
        push_rational(&mut buffer, &mut mask, field::F_NUMBER, summary.f_number);
        push_rational(&mut buffer, &mut mask, field::EXPOSURE, summary.exposure_time);

        if let Some(slot) = buffer.get_mut(1) {
            *slot = mask;
        }
        buffer
    }

    /// Inverse of [`encode`]; `None` for an empty, unknown-version or malformed block, and when nothing usable
    /// survives re-sanitising. Never panics.
    pub fn decode(data: &[u8]) -> Option<ExifSummary> {
        let [version, mask, ..] = data else {
            return None;
        };
        if *version != FORMAT_VERSION {
            return None;
        }
        let mask = *mask;
        let mut cursor = Cursor { data, pos: 2 };

        let date = if present(mask, field::DATE) {
            let ticks = i64::from_le_bytes(cursor.array()?);
            Some(DateTimeUtc::from_ticks(ticks)?)
        } else {
            None
        };
        let make = if present(mask, field::MAKE) {
            Some(cursor.text()?)
        } else {
            None
        };
        let model = if present(mask, field::MODEL) {
            Some(cursor.text()?)
        } else {
            None
        };
        let lens = if present(mask, field::LENS) {
            Some(cursor.text()?)
        } else {
            None
        };
        let iso = if present(mask, field::ISO) {
            Some(i32::from_le_bytes(cursor.array()?))
        } else {
            None
        };
        let focal = if present(mask, field::FOCAL) {
            Some(cursor.rational()?)
        } else {
            None
        };
        let f_number = if present(mask, field::F_NUMBER) {
            Some(cursor.rational()?)
        } else {
            None
        };
        let exposure = if present(mask, field::EXPOSURE) {
            Some(cursor.rational()?)
        } else {
            None
        };
        if cursor.pos != data.len() {
            return None;
        }

        // Re-sanitise: a cache file is just bytes on disk, treat it like any other untrusted input.
        let summary = ExifSummary::create(&RawExifTags {
            date_taken: None,
            make: make.as_deref(),
            model: model.as_deref(),
            lens: lens.as_deref(),
            iso: iso.map(i64::from),
            focal_length: focal,
            f_number,
            exposure_time: exposure,
        });
        match (date, summary) {
            (None, summary) => summary,
            (Some(date), None) => Some(ExifSummary {
                date_taken: Some(date),
                ..ExifSummary::default()
            }),
            (Some(date), Some(summary)) => Some(ExifSummary {
                date_taken: Some(date),
                ..summary
            }),
        }
    }

    fn present(mask: u8, bit: u8) -> bool {
        mask & bit != 0
    }

    /// Writes a text as a length byte and its UTF-8 bytes (cut at 255 bytes, C# `AddText`). `Some("")` sets the bit.
    fn push_text(buffer: &mut Vec<u8>, mask: &mut u8, bit: u8, text: Option<&str>) {
        let Some(text) = text else {
            return;
        };
        *mask |= bit;
        let bytes = text.as_bytes();
        let kept = bytes.get(..usize::from(u8::MAX)).unwrap_or(bytes);
        buffer.push(u8::try_from(kept.len()).unwrap_or(u8::MAX));
        buffer.extend_from_slice(kept);
    }

    fn push_rational(buffer: &mut Vec<u8>, mask: &mut u8, bit: u8, value: Option<ExifRational>) {
        let Some(rational) = value else {
            return;
        };
        *mask |= bit;
        buffer.extend_from_slice(&rational.numerator.to_le_bytes());
        buffer.extend_from_slice(&rational.denominator.to_le_bytes());
    }

    /// Bounds-checked reader: every read returns `None` instead of panicking when the block is short.
    struct Cursor<'a> {
        data: &'a [u8],
        pos: usize,
    }

    impl<'a> Cursor<'a> {
        fn take(&mut self, len: usize) -> Option<&'a [u8]> {
            let end = self.pos.checked_add(len)?;
            let bytes = self.data.get(self.pos..end)?;
            self.pos = end;
            Some(bytes)
        }

        fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
            self.take(N)?.try_into().ok()
        }

        /// Length byte, then UTF-8 decoded lossily (C# `Encoding.UTF8.GetString`).
        fn text(&mut self) -> Option<String> {
            let [length] = self.array::<1>()?;
            let bytes = self.take(usize::from(length))?;
            Some(String::from_utf8_lossy(bytes).into_owned())
        }

        fn rational(&mut self) -> Option<ExifRational> {
            let numerator = u32::from_le_bytes(self.array()?);
            let denominator = u32::from_le_bytes(self.array()?);
            Some(ExifRational::new(numerator, denominator))
        }
    }
}
