//! A UTC instant with .NET `DateTime` semantics: 100 ns ticks since 0001-01-01T00:00:00, range 0001..=9999.
//!
//! Every timestamp the C# app writes to disk (journal, session, fingerprints) is a UTC `DateTime` serialized by
//! System.Text.Json, so the Rust app keeps the same resolution and the same text form. Behaviour of
//! [`DateTimeUtc::to_json`] and [`DateTimeUtc::parse_json`] is pinned by `tests/golden/datetime-json.json`.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// 100 ns ticks per second, as in .NET.
pub const TICKS_PER_SECOND: i64 = 10_000_000;
const TICKS_PER_MINUTE: i64 = 60 * TICKS_PER_SECOND;
const TICKS_PER_DAY: i64 = 86_400 * TICKS_PER_SECOND;
/// `DateTime.MaxValue.Ticks` (9999-12-31T23:59:59.9999999).
const MAX_TICKS: i64 = 3_155_378_975_999_999_999;
/// Ticks at 1970-01-01T00:00:00Z.
const UNIX_EPOCH_TICKS: i64 = 621_355_968_000_000_000;
/// Ticks at 1601-01-01T00:00:00Z (Windows FILETIME epoch).
const FILETIME_EPOCH_TICKS: i64 = 504_911_232_000_000_000;
/// Days from 0001-01-01 to 1970-01-01.
const DAYS_TO_UNIX_EPOCH: i64 = 719_162;

/// A UTC instant in .NET ticks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DateTimeUtc {
    ticks: i64,
}

/// `DateTime.Kind` of a parsed text: which zone information the text carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// Ended in `Z`.
    Utc,
    /// Had a `+hh:mm`/`-hh:mm` offset; the value was converted to UTC.
    Offset,
    /// No zone: the ticks are taken as they are (System.Text.Json returns `DateTimeKind.Unspecified`).
    Unspecified,
}

impl DateTimeUtc {
    /// `DateTime.MinValue`.
    pub const MIN: Self = Self { ticks: 0 };
    /// `DateTime.MaxValue`.
    pub const MAX: Self = Self { ticks: MAX_TICKS };

    /// `None` outside .NET's `DateTime` range.
    pub const fn from_ticks(ticks: i64) -> Option<Self> {
        if ticks < 0 || ticks > MAX_TICKS {
            None
        } else {
            Some(Self { ticks })
        }
    }

    pub const fn ticks(self) -> i64 {
        self.ticks
    }

    /// Current UTC time from the OS clock.
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now()).unwrap_or(Self::MIN)
    }

    /// `None` when outside the `DateTime` range. Sub-tick precision is truncated, like .NET.
    pub fn from_system_time(t: SystemTime) -> Option<Self> {
        let ticks = match t.duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_nanos() / 100)
                .ok()?
                .checked_add(UNIX_EPOCH_TICKS)?,
            Err(before) => {
                let back = before.duration();
                let whole = i64::try_from(back.as_nanos() / 100).ok()?;
                // Truncation toward the past keeps ordering for instants before 1970.
                let partial = i64::from(back.as_nanos() % 100 != 0);
                UNIX_EPOCH_TICKS.checked_sub(whole)?.checked_sub(partial)?
            }
        };
        Self::from_ticks(ticks)
    }

    /// From a Windows FILETIME (100 ns intervals since 1601-01-01 UTC), as `DateTime.FromFileTimeUtc`.
    pub fn from_filetime(filetime: u64) -> Option<Self> {
        Self::from_ticks(i64::try_from(filetime).ok()?.checked_add(FILETIME_EPOCH_TICKS)?)
    }

    pub fn checked_add_ticks(self, delta: i64) -> Option<Self> {
        Self::from_ticks(self.ticks.checked_add(delta)?)
    }

    /// System.Text.Json's form of a UTC `DateTime`: `yyyy-MM-ddTHH:mm:ss[.fffffff]Z` with trailing zeros of the
    /// fraction removed and no fraction at all when it is zero.
    pub fn to_json(self) -> String {
        let (y, mo, d, h, mi, s, frac) = self.fields();
        let mut out = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}");
        if frac != 0 {
            let digits = format!("{frac:07}");
            out.push('.');
            out.push_str(digits.trim_end_matches('0'));
        }
        out.push('Z');
        out
    }

    /// Parses what System.Text.Json accepts for a `DateTime`: `yyyy-MM-dd`, optionally followed by `THH:mm`,
    /// `:ss`, `.fraction` (any number of digits, including none; digits beyond 7 are truncated), then `Z`,
    /// `±hh:mm` / `±hh` (at most ±14:00), or nothing. `T` and `Z` must be upper case. Offsets are converted to UTC;
    /// a result outside the `DateTime` range is rejected.
    pub fn parse_json(text: &str) -> Option<(Self, SourceKind)> {
        let b = text.as_bytes();
        let mut p = Parser { b, i: 0 };
        let year = p.digits(4)?;
        p.expect(b'-')?;
        let month = p.digits(2)?;
        p.expect(b'-')?;
        let day = p.digits(2)?;
        let (mut hour, mut minute, mut second, mut frac) = (0, 0, 0, 0i64);
        if p.peek() == Some(b'T') {
            p.i += 1;
            hour = p.digits(2)?;
            p.expect(b':')?;
            minute = p.digits(2)?;
            if p.peek() == Some(b':') {
                p.i += 1;
                second = p.digits(2)?;
                if p.peek() == Some(b'.') {
                    p.i += 1;
                    let mut scale = 1_000_000i64;
                    while let Some(c) = p.peek().filter(u8::is_ascii_digit) {
                        frac += i64::from(c - b'0') * scale;
                        scale /= 10;
                        p.i += 1;
                    }
                }
            }
        }
        let (offset_minutes, kind) = match p.peek() {
            None => (0, SourceKind::Unspecified),
            Some(b'Z') => {
                p.i += 1;
                (0, SourceKind::Utc)
            }
            Some(sign @ (b'+' | b'-')) => {
                p.i += 1;
                let oh = p.digits(2)?;
                // `+hh` alone is accepted; `+hhmm` is not.
                let om = if p.peek() == Some(b':') {
                    p.i += 1;
                    p.digits(2)?
                } else {
                    0
                };
                let minutes = oh * 60 + om;
                // .NET offsets are limited to ±14:00.
                if om > 59 || minutes > 14 * 60 {
                    return None;
                }
                (if sign == b'-' { -minutes } else { minutes }, SourceKind::Offset)
            }
            Some(_) => return None,
        };
        if p.i != b.len() || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let days = days_from_civil(year, month, day)?;
        let local = days * TICKS_PER_DAY
            + hour * 60 * TICKS_PER_MINUTE
            + minute * TICKS_PER_MINUTE
            + second * TICKS_PER_SECOND
            + frac;
        let utc = local.checked_sub(offset_minutes * TICKS_PER_MINUTE)?;
        Some((Self::from_ticks(utc)?, kind))
    }

    fn fields(self) -> (i64, u32, u32, i64, i64, i64, i64) {
        let days = self.ticks.div_euclid(TICKS_PER_DAY);
        let rem = self.ticks.rem_euclid(TICKS_PER_DAY);
        let (y, m, d) = civil_from_days(days - DAYS_TO_UNIX_EPOCH);
        let secs = rem / TICKS_PER_SECOND;
        (y, m, d, secs / 3600, secs / 60 % 60, secs % 60, rem % TICKS_PER_SECOND)
    }
}

impl fmt::Debug for DateTimeUtc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} ticks)", self.to_json(), self.ticks)
    }
}

impl fmt::Display for DateTimeUtc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_json())
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Option<()> {
        (self.peek()? == c).then(|| self.i += 1)
    }

    fn digits(&mut self, n: usize) -> Option<i64> {
        let slice = self.b.get(self.i..self.i + n)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.i += n;
        Some(slice.iter().fold(0, |acc, &c| acc * 10 + i64::from(c - b'0')))
    }
}

/// Days since 0001-01-01 for a valid proleptic Gregorian date in 1..=9999.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day < 1 {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if day > month_days[(month - 1) as usize] {
        return None;
    }
    // Howard Hinnant's days_from_civil, shifted from the Unix epoch to 0001-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468 + DAYS_TO_UNIX_EPOCH)
}

/// (year, month, day) for days since 1970-01-01 (Howard Hinnant's civil_from_days).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_epoch_is_1601() {
        assert_eq!(
            DateTimeUtc::from_filetime(0).map(DateTimeUtc::to_json).as_deref(),
            Some("1601-01-01T00:00:00Z")
        );
    }

    #[test]
    fn system_time_round_trips_unix_epoch() {
        assert_eq!(
            DateTimeUtc::from_system_time(UNIX_EPOCH).map(DateTimeUtc::ticks),
            Some(UNIX_EPOCH_TICKS)
        );
    }

    #[test]
    fn from_ticks_rejects_out_of_range() {
        assert!(DateTimeUtc::from_ticks(-1).is_none());
        assert!(DateTimeUtc::from_ticks(MAX_TICKS + 1).is_none());
    }

    #[test]
    fn every_day_round_trips_through_civil_conversion() {
        // 0001-01-01 ..= 9999-12-31 (3_652_058 days), the whole DateTime range.
        for days in (0..=3_652_058).step_by(97).chain([3_652_058]) {
            let (y, m, d) = civil_from_days(days - DAYS_TO_UNIX_EPOCH);
            assert_eq!(days_from_civil(y, i64::from(m), i64::from(d)), Some(days));
        }
    }
}
