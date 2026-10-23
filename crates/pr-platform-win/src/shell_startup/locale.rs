//! The user-locale services the secondary windows need and no earlier unit provides (D-W8B6-9; unit W8-17c, contract `contracts/W8-17c.md`).
//!
//! C# gets all of these from the CLR (`CurrentCulture`, `DateTime.ToLocalTime`, `ToString("N0")`, `ToString("P1")`, `ToString("g")`, `ToString("T")`); Rust has no
//! culture, so the three consumers (`DiagnosticsPresenter::build`, `ListDialogContent::for_batch_review`, the Recovery window) take the traits of `pr_app` and this file
//! implements them over the Windows user locale (`GetLocaleInfoEx`, `GetNumberFormatEx`, `GetDateFormatEx`, `GetTimeFormatEx`, `SystemTimeToTzSpecificLocalTime`; every
//! `unsafe` call lives here with a `// SAFETY:` comment). Nothing here is used on a hot path.
use pr_app::presenters::recovery_presentation::RecoveryTimeFormatter;
use pr_app::presenters::secondary::NumberFormatting;
use pr_core::time::DateTimeUtc;
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::Globalization::*;
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::core::PCWSTR;

const LOCALE_CAPACITY: usize = 85;
fn locale_name() -> [u16; LOCALE_CAPACITY] {
    let mut locale = [0u16; LOCALE_CAPACITY];
    // SAFETY: locale is a live 85-u16 output buffer, the documented maximum size for a locale name.
    let length = unsafe { GetUserDefaultLocaleName(&mut locale) };
    if length > 1 { locale } else { [0; LOCALE_CAPACITY] }
}
fn query_locale(kind: u32) -> Option<String> {
    let locale = locale_name();
    let mut buf = [0u16; 128];
    if locale[0] == 0 {
        return None;
    }
    // SAFETY: locale is NUL-terminated and live; buf is a writable 128-u16 buffer and the returned length bounds the read.
    let n = unsafe { GetLocaleInfoEx(PCWSTR(locale.as_ptr()), kind, Some(&mut buf)) };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..(n - 1) as usize]))
}
fn format_number(value: &str, digits: u32) -> String {
    let locale = locale_name();
    let dec = query_locale(LOCALE_SDECIMAL).unwrap_or_else(|| ".".into());
    let sep = query_locale(LOCALE_STHOUSAND).unwrap_or_default();
    let leading = query_locale(LOCALE_ILZERO)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1);
    let grouping = match query_locale(LOCALE_SGROUPING).as_deref() {
        Some("3;2;0") => 32,
        Some("0") => 0,
        _ => 3,
    };
    let dec16: Vec<u16> = dec.encode_utf16().chain(std::iter::once(0)).collect();
    let sep16: Vec<u16> = sep.encode_utf16().chain(std::iter::once(0)).collect();
    let input: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let fmt = NUMBERFMTW {
        NumDigits: digits,
        LeadingZero: leading,
        Grouping: grouping,
        lpDecimalSep: windows::core::PWSTR(dec16.as_ptr() as *mut u16),
        lpThousandSep: windows::core::PWSTR(sep16.as_ptr() as *mut u16),
        NegativeOrder: 1,
    };
    let mut out = [0u16; 128];
    if locale[0] == 0 {
        return value.to_owned();
    }
    // SAFETY: input, separators and locale are live NUL-terminated buffers; out has 128 writable UTF-16 units and API return bounds output.
    let n = unsafe {
        GetNumberFormatEx(
            PCWSTR(locale.as_ptr()),
            0,
            PCWSTR(input.as_ptr()),
            Some(&fmt),
            Some(&mut out),
        )
    };
    if n > 1 {
        String::from_utf16_lossy(&out[..(n - 1) as usize])
    } else {
        value.to_owned()
    }
}

/// `NumberFormatting` over the user locale (`LOCALE_NAME_USER_DEFAULT`): `grouped_i64` = .NET `value.ToString("N0", culture)`, `grouped_f64` = `value.ToString("N1"...)`-style
/// with the rounding rule of the trait, `percent_one_decimal` = `string.Format(culture, "{0:P1}", ratio)`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsNumberFormatting;

impl NumberFormatting for WindowsNumberFormatting {
    fn grouped_i64(&self, value: i64) -> String {
        format_number(&value.to_string(), 0)
    }

    fn grouped_f64(&self, value: f64) -> String {
        if !value.is_finite() {
            return value.to_string();
        }
        let rounded = if value >= 0.0 {
            (value + 0.5).floor()
        } else {
            (value - 0.5).ceil()
        };
        if rounded < i64::MIN as f64 || rounded > i64::MAX as f64 {
            value.to_string()
        } else {
            self.grouped_i64(rounded as i64)
        }
    }

    fn percent_one_decimal(&self, ratio: f64) -> String {
        let scaled = ratio * 100.0;
        if !scaled.is_finite() {
            return format!("{scaled}%");
        }
        let number = format_number(&format!("{scaled:.1}"), 1);
        let language = query_locale(LOCALE_SISO639LANGNAME).unwrap_or_default();
        let symbol = query_locale(LOCALE_SPERCENT).unwrap_or_else(|| "%".into());
        match language.as_str() {
            "fr" => format!("{number}\u{202f}{symbol}"),
            "tr" | "fa" => format!("{symbol}{number}"),
            "de" | "es" | "ru" | "cs" | "sk" | "sv" | "fi" | "nb" | "da" | "pl" | "uk" | "ro" | "hu" | "bg" | "el"
            | "it" | "pt" => format!("{number}\u{00a0}{symbol}"),
            _ => format!("{number}{symbol}"),
        }
    }
}

/// `RecoveryTimeFormatter` over the machine time zone and the user locale: C# `DateTime.SpecifyKind(utc, Utc).ToLocalTime().ToString("g", CurrentCulture)` (short date and
/// short time).
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsRecoveryTimeFormatter;

impl RecoveryTimeFormatter for WindowsRecoveryTimeFormatter {
    fn format_local_general(&self, utc: DateTimeUtc) -> String {
        let Some(filetime) = u64::try_from(utc.ticks())
            .ok()
            .and_then(|ticks| ticks.checked_div(1).filter(|_| ticks > 0))
        else {
            return String::new();
        };
        let mut utc_time = SYSTEMTIME::default();
        // SAFETY: utc_time is a live writable SYSTEMTIME; filetime is a valid local scalar and the Win32 API validates its range.
        if unsafe { FileTimeToSystemTime((&filetime as *const u64).cast(), &mut utc_time) }.is_err() {
            return String::new();
        }
        let mut local = SYSTEMTIME::default();
        // SAFETY: both SYSTEMTIME values are live local structs; null time-zone selects the current machine zone.
        if unsafe { SystemTimeToTzSpecificLocalTime(None, &utc_time, &mut local) }.is_err() {
            return String::new();
        }
        let date = format_system_time(&local, true);
        let time = format_system_time(&local, false);
        if date.is_empty() || time.is_empty() {
            String::new()
        } else {
            format!("{date} {time}")
        }
    }
}

/// The digit-group separator of the .NET `N0` format for the user locale (`LOCALE_STHOUSAND`; an empty value for a locale without grouping).
pub fn group_separator() -> String {
    query_locale(LOCALE_STHOUSAND).unwrap_or_default()
}

/// C# `DateTime.Now.ToString("T", CurrentCulture)`: the current local time with the long time pattern of the user locale.
pub fn local_long_time_text() -> String {
    // SAFETY: the no-argument wrapper returns a fully initialized SYSTEMTIME by value.
    let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format_system_time(&now, false)
}

fn format_system_time(value: &SYSTEMTIME, date: bool) -> String {
    let locale = locale_name();
    let mut out = [0u16; 128];
    if locale[0] == 0 {
        return String::new();
    }
    // SAFETY: locale is NUL-terminated and live; value and out are live local buffers, and the returned length bounds the read.
    let n = unsafe {
        if date {
            GetDateFormatEx(
                PCWSTR(locale.as_ptr()),
                DATE_SHORTDATE,
                Some(value),
                None,
                Some(&mut out),
                None,
            )
        } else {
            GetTimeFormatEx(
                PCWSTR(locale.as_ptr()),
                TIME_NOSECONDS,
                Some(value),
                None,
                Some(&mut out),
            )
        }
    };
    if n > 1 {
        String::from_utf16_lossy(&out[..(n - 1) as usize])
    } else {
        String::new()
    }
}
