//! The user-locale services the secondary windows need and no earlier unit provides (D-W8B6-9; unit W8-17c, contract `contracts/W8-17c.md`).
//!
//! C# gets all of these from the CLR (`CurrentCulture`, `DateTime.ToLocalTime`, `ToString("N0")`, `ToString("P1")`, `ToString("g")`, `ToString("T")`); Rust has no
//! culture, so the three consumers (`DiagnosticsPresenter::build`, `ListDialogContent::for_batch_review`, the Recovery window) take the traits of `pr_app` and this file
//! implements them over the Windows user locale (`GetLocaleInfoEx`, `GetNumberFormatEx`, `GetDateFormatEx`, `GetTimeFormatEx`, `SystemTimeToTzSpecificLocalTime`; every
//! `unsafe` call lives here with a `// SAFETY:` comment). Nothing here is used on a hot path.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::presenters::recovery_presentation::RecoveryTimeFormatter;
use pr_app::presenters::secondary::NumberFormatting;
use pr_core::time::DateTimeUtc;

/// `NumberFormatting` over the user locale (`LOCALE_NAME_USER_DEFAULT`): `grouped_i64` = .NET `value.ToString("N0", culture)`, `grouped_f64` = `value.ToString("N1"...)`-style
/// with the rounding rule of the trait, `percent_one_decimal` = `string.Format(culture, "{0:P1}", ratio)`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsNumberFormatting;

impl NumberFormatting for WindowsNumberFormatting {
    fn grouped_i64(&self, value: i64) -> String {
        todo!()
    }

    fn grouped_f64(&self, value: f64) -> String {
        todo!()
    }

    fn percent_one_decimal(&self, ratio: f64) -> String {
        todo!()
    }
}

/// `RecoveryTimeFormatter` over the machine time zone and the user locale: C# `DateTime.SpecifyKind(utc, Utc).ToLocalTime().ToString("g", CurrentCulture)` (short date and
/// short time).
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsRecoveryTimeFormatter;

impl RecoveryTimeFormatter for WindowsRecoveryTimeFormatter {
    fn format_local_general(&self, utc: DateTimeUtc) -> String {
        todo!()
    }
}

/// The digit-group separator of the .NET `N0` format for the user locale (`LOCALE_STHOUSAND`; an empty value for a locale without grouping).
pub fn group_separator() -> String {
    todo!()
}

/// C# `DateTime.Now.ToString("T", CurrentCulture)`: the current local time with the long time pattern of the user locale.
pub fn local_long_time_text() -> String {
    todo!()
}
