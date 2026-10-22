//! Windows culture and time-zone services for the localization service and the view-model texts (W8-09b slice S2, `contracts/W8-09b.md`, decision D-W8B6-151).
//!
//! `pr_app::localization::service::LocalizationService::new` takes two values the C# class got from .NET statics (its module doc names `pr-platform-win` as the home of both):
//! - `CultureInfo.InstalledUICulture` -> [`installed_ui_culture`] (the Windows display language, `GetUserDefaultLocaleName` for the user locale is NOT the same thing: the C# property is
//!   the OS UI language, `GetSystemDefaultUILanguage` + `GetLocaleInfoEx(LOCALE_SNAME)`);
//! - `CultureInfo.GetCultureInfo(name)` (throws for an unknown name) -> [`WindowsCultureLookup`] (`GetLocaleInfoEx(name, LOCALE_SNAME)` succeeds only for a name Windows knows).
//!
//! Both build a `pr_l10n::loader::UiCulture { name, two_letter_iso_language_name }`; the two-letter name is `LOCALE_SISO639LANGNAME` (the C# `TwoLetterISOLanguageName`:
//! `"vi"`, `"fil"` for Filipino is the three-letter code .NET reports as is).
//!
//! The same file holds the two services `MainViewModel` formats dates with (`FormatContextSource::to_local_time` and `FormatContext::dates`): [`utc_to_local`] and
//! [`WindowsDateText`]. `shell_startup::locale` (W8-17c) has the same Win32 calls inside `WindowsRecoveryTimeFormatter`, but private and shaped for another trait; the porter reuses its
//! helpers if 17c made them `pub`, else repeats the few calls (open question O4). No `unsafe` outside the small safe wrappers, each with a `// SAFETY:` comment (a live `u16`
//! buffer of `LOCALE_NAME_MAX_LENGTH` and the returned length bounding the read). An API failure gives `UiCulture::invariant()` / `None`.
use pr_app::localization::service::CultureLookup;
use pr_app::viewmodels::format::context::DateText;
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_l10n::loader::UiCulture;
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::Globalization::{
    DATE_SHORTDATE, GetDateFormatEx, GetLocaleInfoEx, GetSystemDefaultUILanguage, GetTimeFormatEx, LCIDToLocaleName,
    LOCALE_SENGLISHLANGUAGENAME, LOCALE_SISO639LANGNAME, LOCALE_SNAME, TIME_NOSECONDS,
};
use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
use windows::core::PCWSTR;

/// `LOCALE_NAME_MAX_LENGTH` (85, with the terminating NUL): the size of every locale-name buffer below.
const LOCALE_NAME_MAX_LENGTH: usize = 85;
/// The English language name Windows gives a well-formed but unregistered language tag.
const UNKNOWN_LANGUAGE: &str = "Unknown Language";
/// Room for a short date or a time without seconds in any locale (the longest patterns are well under 40 characters).
const FORMAT_BUFFER: usize = 128;

const TICKS_PER_MILLISECOND: i64 = TICKS_PER_SECOND / 1000;
const TICKS_PER_DAY: i64 = 86_400 * TICKS_PER_SECOND;
/// Days from 0001-01-01 (tick 0) to 1970-01-01.
const DAYS_TO_UNIX_EPOCH: i64 = 719_162;
/// `SYSTEMTIME` cannot hold a year before 1601 (the FILETIME epoch).
const MIN_SYSTEM_TIME_YEAR: i32 = 1601;
const MAX_SYSTEM_TIME_YEAR: i32 = 30_827;

/// A proleptic-Gregorian wall-clock value split into the fields of a `SYSTEMTIME` (the year is an `i32`: ticks reach year 9999 and start at year 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilTime {
    /// 1..=9999.
    pub year: i32,
    /// 1..=12.
    pub month: u32,
    /// 1..=31.
    pub day: u32,
    /// 0..=23.
    pub hour: u32,
    /// 0..=59.
    pub minute: u32,
    /// 0..=59.
    pub second: u32,
    /// 0..=999 (ticks below a millisecond are dropped).
    pub millisecond: u32,
}

/// Days since 1970-01-01 to `(year, month, day)` (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_part + 2) / 5 + 1) as u32;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `(year, month, day)` to days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let day_of_year = (153 * month_index + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Splits ticks (100 ns since 0001-01-01) into calendar fields. Ticks outside `0..=MAX` are clamped by `DateTimeUtc` already.
pub fn civil_from_ticks(utc: DateTimeUtc) -> CivilTime {
    let ticks = utc.ticks();
    let days = ticks.div_euclid(TICKS_PER_DAY);
    let in_day = ticks.rem_euclid(TICKS_PER_DAY);
    let (year, month, day) = civil_from_days(days - DAYS_TO_UNIX_EPOCH);
    let seconds = in_day / TICKS_PER_SECOND;
    CivilTime {
        year: year as i32,
        month,
        day,
        hour: (seconds / 3600) as u32,
        minute: (seconds % 3600 / 60) as u32,
        second: (seconds % 60) as u32,
        millisecond: (in_day % TICKS_PER_SECOND / TICKS_PER_MILLISECOND) as u32,
    }
}

/// Calendar fields back to ticks (the inverse of [`civil_from_ticks`] for a whole number of milliseconds).
pub fn ticks_from_civil(civil: &CivilTime) -> i64 {
    let days = days_from_civil(i64::from(civil.year), civil.month, civil.day) + DAYS_TO_UNIX_EPOCH;
    let seconds = i64::from(civil.hour) * 3600 + i64::from(civil.minute) * 60 + i64::from(civil.second);
    days * TICKS_PER_DAY + seconds * TICKS_PER_SECOND + i64::from(civil.millisecond) * TICKS_PER_MILLISECOND
}

/// The `SYSTEMTIME` of a wall-clock value, `None` when `SYSTEMTIME` cannot hold its year (before 1601).
pub fn system_time_of(utc: DateTimeUtc) -> Option<SYSTEMTIME> {
    let civil = civil_from_ticks(utc);
    if !(MIN_SYSTEM_TIME_YEAR..=MAX_SYSTEM_TIME_YEAR).contains(&civil.year) {
        return None;
    }
    Some(SYSTEMTIME {
        wYear: civil.year as u16,
        wMonth: civil.month as u16,
        wDayOfWeek: 0, // ignored by the conversion and formatting APIs
        wDay: civil.day as u16,
        wHour: civil.hour as u16,
        wMinute: civil.minute as u16,
        wSecond: civil.second as u16,
        wMilliseconds: civil.millisecond as u16,
    })
}

/// The ticks of a `SYSTEMTIME` (milliseconds resolution).
fn ticks_of_system_time(time: &SYSTEMTIME) -> i64 {
    ticks_from_civil(&CivilTime {
        year: i32::from(time.wYear),
        month: u32::from(time.wMonth),
        day: u32::from(time.wDay),
        hour: u32::from(time.wHour),
        minute: u32::from(time.wMinute),
        second: u32::from(time.wSecond),
        millisecond: u32::from(time.wMilliseconds),
    })
}

/// A NUL-terminated UTF-16 copy of `value`, or `None` when it already contains a NUL (Windows would read it as a shorter name).
fn wide_name(value: &str) -> Option<Vec<u16>> {
    if value.contains('\0') {
        return None;
    }
    Some(value.encode_utf16().chain(std::iter::once(0)).collect())
}

/// Text of a buffer the API filled: `written` includes the terminating NUL; `<= 0` is a failure.
fn text_of(buffer: &[u16], written: i32) -> Option<String> {
    let written = usize::try_from(written).ok().filter(|count| *count > 0)?;
    let units = &buffer[..written.min(buffer.len())];
    let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

/// `GetLocaleInfoEx(name, lctype)` as text; `name == None` is the user default locale. `None` when Windows does not know the locale or the value is empty.
fn locale_info(name: Option<&[u16]>, lctype: u32) -> Option<String> {
    let name_ptr = name.map_or(PCWSTR::null(), |buffer| PCWSTR(buffer.as_ptr()));
    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: `name_ptr` is null (user default locale) or points into the live NUL-terminated `name`; `buffer` is a live writable array whose length is passed as the capacity.
    let written = unsafe { GetLocaleInfoEx(name_ptr, lctype, Some(&mut buffer)) };
    text_of(&buffer, written).filter(|text| !text.is_empty())
}

/// C# `CultureInfo.InstalledUICulture`. Falls back to `UiCulture::invariant()` when the query fails (the loader then resolves to English).
pub fn installed_ui_culture() -> UiCulture {
    // SAFETY: no arguments, no pointers; returns the LANGID of the system UI language.
    let language_id = unsafe { GetSystemDefaultUILanguage() };
    let mut name_buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: `name_buffer` is a live writable array whose length is passed as the capacity.
    let written = unsafe { LCIDToLocaleName(u32::from(language_id), Some(&mut name_buffer), 0) };
    let Some(name) = text_of(&name_buffer, written).filter(|text| !text.is_empty()) else {
        return UiCulture::invariant();
    };
    culture_of(&name).unwrap_or_else(|| UiCulture::from_name(&name))
}

/// The culture Windows reports for `name`: canonical `LOCALE_SNAME` and `LOCALE_SISO639LANGNAME`; `None` when Windows does not know the name.
fn culture_of(name: &str) -> Option<UiCulture> {
    let wide = wide_name(name)?;
    let canonical = locale_info(Some(&wide), LOCALE_SNAME)?;
    // Windows accepts any well-formed tag (zz-ZZ-zz) as a custom locale and names its language Unknown Language; .NET refuses it (CultureNotFoundException).
    if locale_info(Some(&wide), LOCALE_SENGLISHLANGUAGENAME)?.starts_with(UNKNOWN_LANGUAGE) {
        return None;
    }
    let two_letter = locale_info(Some(&wide), LOCALE_SISO639LANGNAME)
        .unwrap_or_else(|| UiCulture::from_name(&canonical).two_letter_iso_language_name);
    Some(UiCulture {
        name: canonical,
        two_letter_iso_language_name: two_letter,
    })
}

/// C# `CultureInfo.GetCultureInfo(name)` without the exception. Stateless.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowsCultureLookup;

impl CultureLookup for WindowsCultureLookup {
    /// `Some(culture)` when Windows knows `name` (a blank name is the invariant culture, like .NET), `None` otherwise (C# `CultureNotFoundException`).
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        if name.is_empty() {
            return Some(UiCulture::invariant());
        }
        culture_of(name)
    }
}

/// C# `DateTime.ToLocalTime()` for a catalog last-write time: `utc` ticks to a UTC `SYSTEMTIME`, `SystemTimeToTzSpecificLocalTime(null, ..)` (the machine zone, DST aware, like
/// `TimeZoneInfo.Local`), back to ticks. The result is the local WALL-CLOCK value in the same `DateTimeUtc` type (`ExifLine::modified_local`); an instant the conversion cannot
/// represent (before 1601) or a failing call returns `utc` unchanged. Pure Win32, no state.
pub fn utc_to_local(utc: DateTimeUtc) -> DateTimeUtc {
    let Some(utc_time) = system_time_of(utc) else {
        return utc;
    };
    let mut local_time = SYSTEMTIME::default();
    // SAFETY: a null zone means the machine's current zone; `utc_time` and `local_time` are live SYSTEMTIME values on this stack and only `local_time` is written.
    let converted = unsafe { SystemTimeToTzSpecificLocalTime(None, &utc_time, &mut local_time) };
    if converted.is_err() {
        return utc;
    }
    // The offset is taken between the two millisecond values, so the sub-millisecond ticks of `utc` survive.
    let offset = ticks_of_system_time(&local_time) - ticks_of_system_time(&utc_time);
    utc.checked_add_ticks(offset).unwrap_or(utc)
}

/// `pr_app::viewmodels::format::context::DateText` over the user locale: `general(wall_clock)` = C# `wall_clock.ToString("g", CurrentCulture)` = `GetDateFormatEx(DATE_SHORTDATE)` +
/// one space + `GetTimeFormatEx(TIME_NOSECONDS)` of the wall-clock `SYSTEMTIME` (NO zone conversion); `None` when a call fails or the date is outside the Windows range.
/// `invariant_general` = `MM/dd/yyyy HH:mm` (the invariant-culture "g" pattern), digits only, no API call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowsDateText;

impl DateText for WindowsDateText {
    fn general(&self, wall_clock: DateTimeUtc) -> Option<String> {
        let time = system_time_of(wall_clock)?;
        let mut date_buffer = [0u16; FORMAT_BUFFER];
        // SAFETY: a null locale name is the user default; `time` is a live SYSTEMTIME; null format and calendar select the locale's own; `date_buffer` is a live writable array whose length is the capacity.
        let date_written = unsafe {
            GetDateFormatEx(
                PCWSTR::null(),
                DATE_SHORTDATE,
                Some(&time),
                PCWSTR::null(),
                Some(&mut date_buffer),
                PCWSTR::null(),
            )
        };
        let date = text_of(&date_buffer, date_written)?;
        let mut time_buffer = [0u16; FORMAT_BUFFER];
        // SAFETY: a null locale name is the user default; `time` is a live SYSTEMTIME; a null format selects the locale's own; `time_buffer` is a live writable array whose length is the capacity.
        let time_written = unsafe {
            GetTimeFormatEx(
                PCWSTR::null(),
                TIME_NOSECONDS,
                Some(&time),
                PCWSTR::null(),
                Some(&mut time_buffer),
            )
        };
        let time_text = text_of(&time_buffer, time_written)?;
        Some(format!("{date} {time_text}"))
    }

    fn invariant_general(&self, wall_clock: DateTimeUtc) -> String {
        let civil = civil_from_ticks(wall_clock);
        format!(
            "{:02}/{:02}/{:04} {:02}:{:02}",
            civil.month, civil.day, civil.year, civil.hour, civil.minute
        )
    }
}
