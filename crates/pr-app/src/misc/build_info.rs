//! Port of `PhotoReview.App.Shared/BuildInfo.cs` (C# baseline d92d6aae): the version line of the Settings window,
//! `"Phiên bản 2.0.41 · build 24/09/2026 07:15 · 1a2b3c4d"`.
//!
//! C# reads two assembly attributes written by `Directory.Build.targets`: `InformationalVersion`
//! (`"2.0.41+1a2b3c4d[.dirty]"`) and the `BuildTime` metadata (`"yyyy-MM-dd HH:mm:ss zzz"`). Rust has no assembly
//! attributes: the composition root builds a [`BuildStamp`] from its own build-time values (W8-20a) and passes it in.
//! The text comes from the injected [`Localizer`] (C# `Tr.*`, D-W8-4) and the local clock offset from a
//! [`UtcOffsetSource`] (C# `TimeZoneInfo.Local`, DST aware), so the pure logic is tested with fixed values.

use pr_l10n::keys;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

/// C# `BuildInfo.PartSeparator` text: `" · "` (U+0020 U+00B7 U+0020), language-neutral.
pub const PART_SEPARATOR: &str = " · ";

/// The `BuildTime` metadata format (C# `DateTimeOffset.TryParseExact` format string, invariant culture).
pub const BUILD_TIME_FORMAT: &str = "yyyy-MM-dd HH:mm:ss zzz";

/// The attributes that `Directory.Build.targets` writes into the binary (C# `AssemblyInformationalVersionAttribute` and
/// `AssemblyMetadataAttribute("BuildTime")`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildStamp {
    /// `"2.0.41+1a2b3c4d[.dirty]"`, or `None` when the attribute is absent.
    pub informational_version: Option<String>,
    /// `"yyyy-MM-dd HH:mm:ss zzz"`, or `None` when the metadata is absent.
    pub build_time: Option<String>,
}

impl BuildStamp {
    /// C# `GetVersion(assembly)`: the informational version exactly as written (`None` when absent).
    pub fn version(&self) -> Option<&str> {
        self.informational_version.as_deref()
    }

    /// C# `Describe(assembly)`: [`describe_parts`] over this stamp's two values.
    pub fn describe(&self, localizer: &Localizer, offset: &dyn UtcOffsetSource) -> String {
        describe_parts(
            localizer,
            self.informational_version.as_deref(),
            self.build_time.as_deref(),
            offset,
        )
    }
}

/// The machine's local UTC offset at a given instant (C# `TimeZoneInfo.Local.GetUtcOffset(instant)`). Implemented in
/// `pr-platform-win` (the time-zone API is Windows code); this module only consumes it. Pure tests use [`FixedOffset`].
pub trait UtcOffsetSource: Send + Sync {
    /// Minutes east of UTC at `utc_unix_seconds` (seconds since 1970-01-01T00:00:00Z), DST aware.
    fn offset_minutes_at(&self, utc_unix_seconds: i64) -> i32;
}

/// A constant offset: tests, and a caller that wants UTC (`FixedOffset(0)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedOffset(pub i32);

impl UtcOffsetSource for FixedOffset {
    fn offset_minutes_at(&self, _utc_unix_seconds: i64) -> i32 {
        self.0
    }
}

/// A parsed `BuildTime` (C# `DateTimeOffset`, reduced to the instant: the written offset only locates the instant and
/// is NOT used for display, see [`describe_parts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildTime {
    /// Seconds since 1970-01-01T00:00:00Z.
    pub utc_unix_seconds: i64,
}

/// C# `DateTimeOffset.TryParseExact(text, "yyyy-MM-dd HH:mm:ss zzz", Invariant, None, out built)`. `None` input (C#
/// `null`) and any text that does not match the format give `None`. Four-digit year, two-digit fields, real calendar
/// dates only. Like .NET: each format space matches exactly one ASCII space or U+00A0 (a tab is rejected), `zzz` takes
/// a sign, a one- or two-digit (greedy) hour, an optional `':'` and two-digit minutes (`+7:00`, `+0700`, `+07:00`),
/// and the UTC instant must lie within 0001-01-01T00:00:00Z..9999-12-31T23:59:59Z.
pub fn parse_build_time(text: Option<&str>) -> Option<BuildTime> {
    let chars: Vec<char> = text?.chars().collect();
    let mut pos = 0usize;

    let year = take_digits(&chars, &mut pos, 4, 4)?;
    take_char(&chars, &mut pos, |c| c == '-')?;
    let month = take_digits(&chars, &mut pos, 2, 2)?;
    take_char(&chars, &mut pos, |c| c == '-')?;
    let day = take_digits(&chars, &mut pos, 2, 2)?;
    take_char(&chars, &mut pos, is_format_space)?;
    let hour = take_digits(&chars, &mut pos, 2, 2)?;
    take_char(&chars, &mut pos, |c| c == ':')?;
    let minute = take_digits(&chars, &mut pos, 2, 2)?;
    take_char(&chars, &mut pos, |c| c == ':')?;
    let second = take_digits(&chars, &mut pos, 2, 2)?;
    take_char(&chars, &mut pos, is_format_space)?;
    let sign = take_char(&chars, &mut pos, |c| c == '+' || c == '-')?;
    let offset_hours = take_digits(&chars, &mut pos, 1, 2)?;
    if chars.get(pos) == Some(&':') {
        pos += 1;
    }
    let offset_minutes = take_digits(&chars, &mut pos, 2, 2)?;
    if pos != chars.len() {
        return None;
    }

    // .NET DateTimeOffset: years 0001..9999, real calendar dates, 0..23 h, 0..59 min and s, offsets within +-14:00.
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    if day == 0 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 || offset_minutes > 59 {
        return None;
    }
    let offset_total = offset_hours * 60 + offset_minutes;
    if offset_total > 14 * 60 {
        return None;
    }
    let signed_offset = if sign == '-' { -offset_total } else { offset_total };

    let local_seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    let utc = local_seconds - signed_offset * 60;
    // The UTC instant must be a valid DateTime (0001-01-01T00:00:00 .. 9999-12-31T23:59:59).
    let min_utc = days_from_civil(1, 1, 1) * 86_400;
    let max_utc = days_from_civil(9999, 12, 31) * 86_400 + 86_399;
    if utc < min_utc || utc > max_utc {
        return None;
    }
    Some(BuildTime { utc_unix_seconds: utc })
}

/// A format space in .NET `ParseExact` matches a space or, as its fallback, U+00A0.
fn is_format_space(c: char) -> bool {
    c == ' ' || c == '\u{A0}'
}

/// Consumes one char that satisfies `accept`; `None` at the end of input or on a mismatch.
fn take_char(chars: &[char], pos: &mut usize, accept: impl Fn(char) -> bool) -> Option<char> {
    let c = *chars.get(*pos)?;
    if accept(c) {
        *pos += 1;
        Some(c)
    } else {
        None
    }
}

/// Consumes between `min` and `max` ASCII digits (greedy) and returns their value.
fn take_digits(chars: &[char], pos: &mut usize, min: usize, max: usize) -> Option<i64> {
    let mut value = 0i64;
    let mut count = 0usize;
    while count < max {
        match chars.get(*pos) {
            Some(c) if c.is_ascii_digit() => {
                value = value * 10 + i64::from(*c as u8 - b'0');
                *pos += 1;
                count += 1;
            }
            _ => break,
        }
    }
    (count >= min).then_some(value)
}
/// C# `BuildInfo.Describe(string? informationalVersion, string? buildTime)` (internal overload, the one the tests call).
///
/// * `informational_version` empty or whitespace-only (C# `IsNullOrWhiteSpace`, including `None`): returns
///   `loc.get(settings.version.unknown)` and nothing else, even when `build_time` is present.
/// * Otherwise split at the FIRST `'+'`: `version` = text before it (or the whole text), `commit` = text after it
///   (`None` when there is no `'+'`). Parts, joined with [`PART_SEPARATOR`]:
///   1. `loc.format(settings.version.number, version = version)` (an empty version still gives the labelled part);
///   2. when `build_time` parses: `loc.format(settings.version.build, buildTime = format_local_minute(..))`, where the
///      minute comes from the instant shifted by `offset.offset_minutes_at(instant)`;
///   3. the commit, only when it is non-empty (`"2.0.41+"` has no commit part).
pub fn describe_parts(
    localizer: &Localizer,
    informational_version: Option<&str>,
    build_time: Option<&str>,
    offset: &dyn UtcOffsetSource,
) -> String {
    let informational = match informational_version {
        Some(text) if !text.trim().is_empty() => text,
        _ => return localizer.get(keys::SETTINGS_VERSION_UNKNOWN),
    };

    let (version, commit) = match informational.find('+') {
        Some(index) => (&informational[..index], Some(&informational[index + 1..])),
        None => (informational, None),
    };

    let mut parts = vec![localizer.format(keys::SETTINGS_VERSION_NUMBER, &[LocArg::new("version", version)])];

    if let Some(built) = parse_build_time(build_time) {
        let utc = built.utc_unix_seconds;
        let offset_minutes = offset.offset_minutes_at(utc);
        // C# ToLocalTime clamps to DateTime.MinValue / MaxValue instead of throwing; format_local_minute clamps too.
        parts.push(localizer.format(
            keys::SETTINGS_VERSION_BUILD,
            &[LocArg::new("buildTime", format_local_minute(utc, offset_minutes))],
        ));
    }

    if let Some(commit) = commit.filter(|commit| !commit.is_empty()) {
        parts.push(commit.to_owned());
    }

    parts.join(PART_SEPARATOR)
}

/// C# `built.ToLocalTime().ToString("dd'/'MM'/'yyyy HH':'mm", CultureInfo.InvariantCulture)`: the wall-clock minute
/// of `utc_unix_seconds` shifted by `offset_minutes` (`"24/09/2026 07:15"`). Proleptic Gregorian calendar, two-digit
/// day, month, hour and minute, four-digit year. Pure arithmetic (no `chrono`, no OS time zone).
pub fn format_local_minute(utc_unix_seconds: i64, offset_minutes: i32) -> String {
    // .NET `DateTime.ToLocalTime` clamps to 0001-01-01T00:00:00 .. 9999-12-31T23:59:59.9999999 (it never throws).
    let min_local = days_from_civil(1, 1, 1) * 86_400;
    let max_local = days_from_civil(9999, 12, 31) * 86_400 + 86_399;
    let local = (utc_unix_seconds + i64::from(offset_minutes) * 60).clamp(min_local, max_local);
    let days = local.div_euclid(86_400);
    let seconds_of_day = local.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    format!("{day:02}/{month:02}/{year:04} {hour:02}:{minute:02}")
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        _ => 28,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `(year, month, day)` of a day count since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}
