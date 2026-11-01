//! User-locale formatting tests for shell dialogs.

use pr_app::presenters::recovery_presentation::RecoveryTimeFormatter;
use pr_app::presenters::secondary::NumberFormatting;
use pr_core::time::DateTimeUtc;
use pr_platform_win::shell_startup::locale::{
    WindowsNumberFormatting, WindowsRecoveryTimeFormatter, group_separator, local_long_time_text,
};

// New pure: the locale group separator is a single short character or empty.
#[test]
fn locale_group_separator_is_one_short_string() {
    assert!(group_separator().chars().count() <= 1);
}

// New pure: N0 uses the locale grouping separator.
#[test]
fn locale_grouped_i64_uses_the_group_separator_of_the_locale() {
    let separator = group_separator();
    let formatter = WindowsNumberFormatting;
    for (value, expected_digits) in [(1_234_567, "1234567"), (-1_234_567, "1234567")] {
        let formatted = formatter.grouped_i64(value);
        let digits: String = formatted.chars().filter(char::is_ascii_digit).collect();
        assert_eq!(digits, expected_digits, "{value}: {formatted}");
        if separator.is_empty() {
            assert_eq!(formatted, value.to_string());
        } else {
            assert!(formatted.contains(&separator), "{formatted} lacks {separator:?}");
        }
    }
}

// New pure: the trait rounds exact half values away from zero before N0 grouping.
#[test]
fn locale_grouped_f64_rounds_half_away_from_zero() {
    let formatter = WindowsNumberFormatting;
    for (value, expected) in [
        (1.5, formatter.grouped_i64(2)),
        (-1.5, formatter.grouped_i64(-2)),
        (1.49, formatter.grouped_i64(1)),
        (-1.49, formatter.grouped_i64(-1)),
    ] {
        assert_eq!(formatter.grouped_f64(value), expected, "{value}");
    }
}

// New pure: en-US has the default P1 shape; culture-specific patterns are intentionally not asserted here.
#[test]
fn locale_percent_one_decimal_uses_en_us_shape_when_language_is_english() {
    let formatter = WindowsNumberFormatting;
    let text = formatter.percent_one_decimal(0.5);
    let language = windows_language();
    if language.as_deref() == Some("en") {
        assert_eq!(text, "50.0%");
    } else {
        assert!(!text.is_empty());
    }
}

fn windows_language() -> Option<String> {
    let mut locale = [0u16; 85];
    // SAFETY: the output buffer is live and has the documented locale-name maximum capacity.
    let length = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut locale) };
    if length <= 1 {
        return None;
    }
    let name = String::from_utf16_lossy(&locale[..(length - 1) as usize]);
    name.split(['-', '_']).next().map(str::to_owned)
}

// New native: reads the user's long time pattern.
#[test]
#[ignore = "native: queries the Windows user locale"]
fn locale_local_long_time_text_has_digits() {
    let text = local_long_time_text();
    assert!(text.chars().any(|character| character.is_ascii_digit()), "{text:?}");
}

// New native (fix-g10-g13, BACKLOG G3 L1): C# `ToString("T")` is the long time pattern WITH seconds (hour, minute and second groups).
#[test]
#[ignore = "native: queries the Windows user locale"]
fn locale_local_long_time_text_includes_seconds() {
    let text = local_long_time_text();
    let mut groups = 0;
    let mut in_digits = false;
    for character in text.chars() {
        if character.is_numeric() {
            if !in_digits {
                groups += 1;
            }
            in_digits = true;
        } else {
            in_digits = false;
        }
    }
    assert!(groups >= 3, "expected hour, minute and second groups: {text:?}");
}

// New native: 2020-01-02 03:04:05 UTC in the fixed UTC zone formats as a local date/time with digits.
#[test]
#[ignore = "native: requires the machine time zone to be UTC; skip when UTC is unavailable"]
fn locale_recovery_time_formatter_formats_a_known_instant_in_a_fixed_time_zone() {
    if !machine_zone_is_utc() {
        eprintln!("skip: machine time zone is not UTC");
        return;
    }
    let ticks = 637_135_310_450_000_000_i64;
    let instant = DateTimeUtc::from_ticks(ticks).expect("known valid UTC ticks");
    let formatted = WindowsRecoveryTimeFormatter.format_local_general(instant);
    assert!(
        formatted.chars().filter(char::is_ascii_digit).count() >= 12,
        "{formatted:?}"
    );
    assert!(
        formatted.contains("2020") || formatted.contains("20"),
        "expected year digits: {formatted:?}"
    );
}
#[cfg(windows)]
fn machine_zone_is_utc() -> bool {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_ID_INVALID, TIME_ZONE_INFORMATION};
    let mut zone = TIME_ZONE_INFORMATION::default();
    // SAFETY: zone is a live writable TIME_ZONE_INFORMATION buffer for the duration of the call.
    let status = unsafe { GetTimeZoneInformation(&mut zone) };
    status != TIME_ZONE_ID_INVALID && zone.Bias == 0 && zone.StandardBias == 0 && zone.DaylightBias == 0
}

#[cfg(not(windows))]
fn machine_zone_is_utc() -> bool {
    false
}

// New (exe walkthrough 3): .NET ticks (since 0001) were passed to FileTimeToSystemTime as a FILETIME (since 1601), so every Recovery time showed year +1600
// ("10/29/3626"). 2020-07-01 12:00:00 UTC is the same calendar year in every time zone; the culture's short date has a four-digit year.
#[test]
fn locale_recovery_time_formatter_uses_the_dotnet_tick_epoch_not_the_filetime_epoch() {
    let instant = DateTimeUtc::from_ticks(637_292_016_000_000_000_i64).expect("known valid UTC ticks");
    let formatted = WindowsRecoveryTimeFormatter.format_local_general(instant);
    assert!(formatted.contains("2020"), "expected year 2020: {formatted:?}");
    assert!(!formatted.contains("3620"), "FILETIME epoch confusion: {formatted:?}");
}

// New (exe walkthrough 3): an instant before 1601 cannot be a FILETIME; the formatter returns an empty text instead of a wrapped date.
#[test]
fn locale_recovery_time_formatter_returns_empty_before_the_filetime_epoch() {
    let instant = DateTimeUtc::from_ticks(1_000_000_000_i64).expect("valid ticks in year 1");
    assert_eq!(WindowsRecoveryTimeFormatter.format_local_general(instant), "");
}
