//! W8-09b slice S2: `culture` (installed UI culture, culture lookup, `utc_to_local`, `WindowsDateText`).
//!
//! C# twins: none directly (the C# code called `CultureInfo.InstalledUICulture`, `CultureInfo.GetCultureInfo`, `DateTime.ToLocalTime` and `ToString("g")` of the CLR); every test is `// new:`.
//! The calendar arithmetic and the invariant pattern are pure and run everywhere; the Win32 locale and time-zone calls are `#[ignore = "native: ..."]`, each on its own thread bounded by 30 s.
#![cfg(windows)]

use std::sync::mpsc;
use std::time::Duration;

use pr_app::localization::service::CultureLookup;
use pr_app::viewmodels::format::context::DateText;
use pr_core::time::{DateTimeUtc, TICKS_PER_SECOND};
use pr_platform_win::culture::{
    CivilTime, WindowsCultureLookup, WindowsDateText, civil_from_ticks, installed_ui_culture, system_time_of,
    ticks_from_civil, utc_to_local,
};

const BOUND: Duration = Duration::from_secs(30);
const TICKS_PER_DAY: i64 = 86_400 * TICKS_PER_SECOND;
/// `new DateTime(2000, 1, 1).Ticks` in .NET.
const TICKS_2000_01_01: i64 = 630_822_816_000_000_000;
/// `DateTime.UnixEpoch.Ticks` in .NET.
const TICKS_1970_01_01: i64 = 621_355_968_000_000_000;
/// `new DateTime(1601, 1, 1).Ticks` (the FILETIME epoch).
const TICKS_1601_01_01: i64 = 504_911_232_000_000_000;

/// Runs `body` on its own thread and fails the test when it does not finish within [`BOUND`] (or panics).
fn bounded(body: impl FnOnce() + Send + 'static) {
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        body();
        let _ = sender.send(());
    });
    match receiver.recv_timeout(BOUND) {
        Ok(()) => handle.join().expect("test thread"),
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Ok(()) => panic!("test thread ended without a result"),
            Err(panic) => std::panic::resume_unwind(panic),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("native culture test exceeded {BOUND:?}"),
    }
}

fn at(ticks: i64) -> DateTimeUtc {
    DateTimeUtc::from_ticks(ticks).expect("in range")
}

fn civil(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32, millisecond: u32) -> CivilTime {
    CivilTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        millisecond,
    }
}

// new: the calendar split against the tick values .NET reports for well-known dates.
#[test]
fn civil_from_ticks_matches_known_dotnet_tick_values() {
    assert_eq!(civil_from_ticks(at(0)), civil(1, 1, 1, 0, 0, 0, 0));
    assert_eq!(civil_from_ticks(at(TICKS_1601_01_01)), civil(1601, 1, 1, 0, 0, 0, 0));
    assert_eq!(civil_from_ticks(at(TICKS_1970_01_01)), civil(1970, 1, 1, 0, 0, 0, 0));
    assert_eq!(civil_from_ticks(at(TICKS_2000_01_01)), civil(2000, 1, 1, 0, 0, 0, 0));
    // 2000 is a leap year (divisible by 400): day 60 is the 29th of February.
    assert_eq!(
        civil_from_ticks(at(TICKS_2000_01_01 + 59 * TICKS_PER_DAY)),
        civil(2000, 2, 29, 0, 0, 0, 0)
    );
    assert_eq!(
        civil_from_ticks(at(TICKS_2000_01_01 + 60 * TICKS_PER_DAY)),
        civil(2000, 3, 1, 0, 0, 0, 0)
    );
    // `DateTime.MaxValue`.
    assert_eq!(civil_from_ticks(DateTimeUtc::MAX), civil(9999, 12, 31, 23, 59, 59, 999));
    // Time of day and milliseconds (1 ms = 10 000 ticks; sub-millisecond ticks are dropped).
    let ticks = TICKS_2000_01_01 + (13 * 3600 + 45 * 60 + 59) * TICKS_PER_SECOND + 123 * 10_000 + 9_999;
    assert_eq!(civil_from_ticks(at(ticks)), civil(2000, 1, 1, 13, 45, 59, 123));
}

// new: 1900 is NOT a leap year, 2100 neither, 2400 is.
#[test]
fn century_years_follow_the_gregorian_leap_rule() {
    for (year, leap) in [(1900, false), (2100, false), (2400, true), (2024, true), (2023, false)] {
        let first_march = ticks_from_civil(&civil(year, 3, 1, 0, 0, 0, 0));
        let last_february = civil_from_ticks(at(first_march - TICKS_PER_DAY));
        assert_eq!(last_february.month, 2, "{year}");
        assert_eq!(last_february.day, if leap { 29 } else { 28 }, "{year}");
    }
}

// new: the two directions are inverse over the whole range (every 7th day, plus both ends).
#[test]
fn ticks_and_civil_time_round_trip() {
    let mut ticks = 0;
    while ticks < DateTimeUtc::MAX.ticks() - TICKS_PER_DAY {
        let value = at(ticks + 12 * 3600 * TICKS_PER_SECOND + 345 * 10_000);
        let back = ticks_from_civil(&civil_from_ticks(value));
        assert_eq!(
            back,
            value.ticks() - value.ticks() % 10_000,
            "{:?}",
            civil_from_ticks(value)
        );
        ticks += 7 * TICKS_PER_DAY;
    }
    assert_eq!(
        ticks_from_civil(&civil_from_ticks(DateTimeUtc::MAX)),
        DateTimeUtc::MAX.ticks() / 10_000 * 10_000
    );
}

// new: SYSTEMTIME cannot hold a year before 1601; the boundary instant itself can.
#[test]
fn system_time_of_refuses_years_before_1601_only() {
    assert!(system_time_of(at(0)).is_none());
    assert!(system_time_of(at(TICKS_1601_01_01 - 1)).is_none());
    let first = system_time_of(at(TICKS_1601_01_01)).expect("1601-01-01 is representable");
    assert_eq!((first.wYear, first.wMonth, first.wDay), (1601, 1, 1));
    let sample = system_time_of(at(TICKS_2000_01_01
        + (13 * 3600 + 45 * 60 + 59) * TICKS_PER_SECOND
        + 123 * 10_000))
    .expect("representable");
    assert_eq!(
        (
            sample.wYear,
            sample.wMonth,
            sample.wDay,
            sample.wHour,
            sample.wMinute,
            sample.wSecond,
            sample.wMilliseconds
        ),
        (2000, 1, 1, 13, 45, 59, 123)
    );
}

// new: C# `DateTime.ToString("g", CultureInfo.InvariantCulture)` = `MM/dd/yyyy HH:mm`.
#[test]
fn invariant_general_is_month_day_year_with_24_hour_time_and_no_seconds() {
    let text = WindowsDateText;
    let sample = ticks_from_civil(&civil(2024, 3, 5, 14, 7, 59, 999));
    assert_eq!(text.invariant_general(at(sample)), "03/05/2024 14:07");
    assert_eq!(
        text.invariant_general(at(0)),
        "01/01/0001 00:00",
        "year 1 is zero padded"
    );
    assert_eq!(text.invariant_general(DateTimeUtc::MAX), "12/31/9999 23:59");
    assert_eq!(
        text.invariant_general(at(TICKS_2000_01_01)),
        "01/01/2000 00:00",
        "midnight is 00:00, not 24:00"
    );
}

// new: Windows knows the machine's UI language (a Windows 10 machine always has one).
#[test]
#[ignore = "native: GetSystemDefaultUILanguage"]
fn installed_ui_culture_has_a_name_and_a_language_code() {
    bounded(|| {
        let culture = installed_ui_culture();
        assert!(!culture.name.is_empty(), "{culture:?}");
        assert!(
            (2..=3).contains(&culture.two_letter_iso_language_name.len()),
            "{culture:?}"
        );
        assert!(
            culture
                .name
                .to_ascii_lowercase()
                .starts_with(&culture.two_letter_iso_language_name),
            "{culture:?}"
        );
    });
}

// new: CultureInfo.GetCultureInfo("vi-VN").TwoLetterISOLanguageName == "vi"; an unknown name is CultureNotFoundException (None).
#[test]
#[ignore = "native: GetLocaleInfoEx"]
fn culture_lookup_knows_vietnamese_and_refuses_an_unknown_name() {
    bounded(|| {
        let lookup = WindowsCultureLookup;

        let vietnamese = lookup.lookup("vi-VN").expect("vi-VN is installed on every Windows");
        assert_eq!(vietnamese.name, "vi-VN");
        assert_eq!(vietnamese.two_letter_iso_language_name, "vi");

        assert_eq!(lookup.lookup("zz-ZZ-zz"), None);
        assert_eq!(lookup.lookup("not a locale"), None);
        assert_eq!(lookup.lookup("vi\0VN"), None, "an embedded NUL is never a name");
    });
}

// new: the canonical spelling comes from Windows (like .NET `CultureInfo.Name`), the neutral culture is found, and "" is the invariant culture.
#[test]
#[ignore = "native: GetLocaleInfoEx"]
fn culture_lookup_canonicalizes_the_name_and_maps_the_blank_name_to_the_invariant_culture() {
    bounded(|| {
        let lookup = WindowsCultureLookup;

        let english = lookup.lookup("EN-us").expect("en-US");
        assert_eq!(english.name, "en-US");
        assert_eq!(english.two_letter_iso_language_name, "en");

        let neutral = lookup.lookup("de").expect("de");
        assert_eq!(neutral.name, "de");
        assert_eq!(neutral.two_letter_iso_language_name, "de");

        let invariant = lookup.lookup("").expect("invariant");
        assert_eq!(invariant.name, "");
        assert_eq!(invariant.two_letter_iso_language_name, "iv");
    });
}

// new: the local value is within the largest real zone offsets of the input, never goes backwards, and keeps sub-millisecond ticks.
#[test]
#[ignore = "native: SystemTimeToTzSpecificLocalTime reads the machine time zone"]
fn utc_to_local_is_monotonic_and_within_fourteen_hours_of_the_input() {
    bounded(|| {
        let fourteen_hours = 14 * 3600 * TICKS_PER_SECOND;
        let mut previous_offset_ticks: Option<i64> = None;
        let mut previous_local = 0;
        for day in (0..366).step_by(5) {
            let utc = at(TICKS_2000_01_01 + 24 * TICKS_PER_DAY * 365 + day * TICKS_PER_DAY + 12_345);
            let local = utc_to_local(utc);
            let offset = local.ticks() - utc.ticks();
            assert!(offset.abs() <= fourteen_hours, "offset {offset} ticks on day {day}");
            // The sub-millisecond part of the input survives.
            assert_eq!(local.ticks() % 10_000, utc.ticks() % 10_000);
            assert!(local.ticks() > previous_local);
            previous_local = local.ticks();
            if let Some(previous) = previous_offset_ticks {
                // Between two samples five days apart only a DST change can alter the offset, by at most two hours.
                assert!((offset - previous).abs() <= 2 * 3600 * TICKS_PER_SECOND);
            }
            previous_offset_ticks = Some(offset);
        }
    });
}

// new: an instant before 1601 cannot be converted and is returned unchanged; so are the extremes that stay in range.
#[test]
#[ignore = "native: SystemTimeToTzSpecificLocalTime reads the machine time zone"]
fn utc_to_local_returns_an_unrepresentable_instant_unchanged() {
    bounded(|| {
        assert_eq!(utc_to_local(at(0)), at(0));
        assert_eq!(utc_to_local(at(TICKS_1601_01_01 - 1)), at(TICKS_1601_01_01 - 1));
        let max = utc_to_local(DateTimeUtc::MAX);
        assert!(max.ticks() <= DateTimeUtc::MAX.ticks());
    });
}

// new: the user-locale "g" text has a date, one space and a time, and refuses a date Windows cannot format.
#[test]
#[ignore = "native: GetDateFormatEx / GetTimeFormatEx read the user locale"]
fn windows_date_text_general_formats_a_date_and_a_time_and_refuses_pre_1601() {
    bounded(|| {
        let text = WindowsDateText;
        let sample = at(ticks_from_civil(&civil(2024, 3, 5, 14, 7, 0, 0)));

        let general = text.general(sample).expect("a formatted date");

        assert!(general.contains(' '), "{general}");
        assert!(general.chars().filter(char::is_ascii_digit).count() >= 6, "{general}");
        assert_eq!(text.general(at(0)), None, "year 1 is outside the Windows range");
        // The wall-clock value is formatted as given: no zone conversion (two instants a minute apart differ only in the time).
        let later = text
            .general(at(sample.ticks() + 60 * TICKS_PER_SECOND))
            .expect("formatted");
        assert_ne!(general, later);
    });
}
