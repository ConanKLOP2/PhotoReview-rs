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
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::localization::service::CultureLookup;
use pr_app::viewmodels::format::context::DateText;
use pr_core::time::DateTimeUtc;
use pr_l10n::loader::UiCulture;

/// C# `CultureInfo.InstalledUICulture`. Falls back to `UiCulture::invariant()` when the query fails (the loader then resolves to English).
pub fn installed_ui_culture() -> UiCulture {
    todo!()
}

/// C# `CultureInfo.GetCultureInfo(name)` without the exception. Stateless.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowsCultureLookup;

impl CultureLookup for WindowsCultureLookup {
    /// `Some(culture)` when Windows knows `name` (a blank name is the invariant culture, like .NET), `None` otherwise (C# `CultureNotFoundException`).
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        todo!()
    }
}

/// C# `DateTime.ToLocalTime()` for a catalog last-write time: `utc` ticks to a UTC `SYSTEMTIME`, `SystemTimeToTzSpecificLocalTime(null, ..)` (the machine zone, DST aware, like
/// `TimeZoneInfo.Local`), back to ticks. The result is the local WALL-CLOCK value in the same `DateTimeUtc` type (`ExifLine::modified_local`); an instant the conversion cannot
/// represent (before 1601) or a failing call returns `utc` unchanged. Pure Win32, no state.
pub fn utc_to_local(utc: DateTimeUtc) -> DateTimeUtc {
    todo!()
}

/// `pr_app::viewmodels::format::context::DateText` over the user locale: `general(wall_clock)` = C# `wall_clock.ToString("g", CurrentCulture)` = `GetDateFormatEx(DATE_SHORTDATE)` +
/// one space + `GetTimeFormatEx(TIME_NOSECONDS)` of the wall-clock `SYSTEMTIME` (NO zone conversion); `None` when a call fails or the date is outside the Windows range.
/// `invariant_general` = `MM/dd/yyyy HH:mm` (the invariant-culture "g" pattern), digits only, no API call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowsDateText;

impl DateText for WindowsDateText {
    fn general(&self, wall_clock: DateTimeUtc) -> Option<String> {
        todo!()
    }

    fn invariant_general(&self, wall_clock: DateTimeUtc) -> String {
        todo!()
    }
}
