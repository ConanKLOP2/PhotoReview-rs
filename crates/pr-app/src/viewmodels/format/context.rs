//! Display-culture facts shared by the formatters (C# `IFormatProvider` and `CultureInfo.CurrentCulture`).
//! Part of `pr_app::viewmodels::format` (W8-02b). Port target: `PhotoReview.App.Shared/ViewModels/{ExifFormatter,StatusFormatter}.cs`
//! (C# baseline d92d6aae). No `unsafe`, no `windows` crate: this is a pure module.
#![allow(unused_variables, dead_code)] // D11: todo!() bodies only; the porter removes this with the last todo!()

use std::fmt::Debug;

use pr_core::time::DateTimeUtc;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::NumberCulture;

/// C# `DateTime.ToString("g", provider)` of a wall-clock value (no zone conversion happens here).
/// The platform layer answers it from the display culture; golden tests replay the values the oracle recorded.
pub trait DateText: Debug {
    /// `None` when the culture's calendar cannot represent the date (C# `ArgumentOutOfRangeException`).
    fn general(&self, wall_clock: DateTimeUtc) -> Option<String>;
    /// C# `wall_clock.ToString("g", CultureInfo.InvariantCulture)`: the fallback when [`DateText::general`] gives `None`.
    fn invariant_general(&self, wall_clock: DateTimeUtc) -> String;
}

/// Everything the formatters read from the environment, passed explicitly (no ambient `Localizer`, no ambient culture).
#[derive(Debug, Clone, Copy)]
pub struct FormatContext<'a> {
    /// UI text from the language catalog (`pr_l10n`, D-W8-4). Keys come from `pr_l10n::keys::*`; plural groups use the base key.
    pub texts: &'a Localizer,
    /// Number format of the display culture: the C# `provider` (and `CurrentCulture` for `FormatFileSize`).
    pub numbers: &'a NumberCulture,
    /// `NumberFormatInfo.NumberGroupSeparator`, used by the "N0" format only (`compare_size_suffix`).
    /// Not part of `pr_l10n::NumberCulture` yet (contract open question Q1).
    pub group_separator: &'a str,
    /// Date text of the display culture (`DateText::general`, fallback `invariant_general`).
    pub dates: &'a dyn DateText,
}

/// C# `value.ToString(format, provider)` for the custom numeric formats `"0"` (`max_decimals` 0), `"0.#"` (1) and
/// `"0.##"` (2) applied to a `double`. Digits after the decimal point are rounded to `max_decimals`, trailing zeros are
/// dropped, the point is `numbers.decimal_separator`, the sign is `numbers.negative_sign`. Only finite values are passed.
/// Rounding and digit output must match the golden file (contract trap T1).
pub fn format_custom_fixed(value: f64, max_decimals: u8, numbers: &NumberCulture) -> String {
    todo!("W8-02b: format_custom_fixed, see contract trap T1 and golden format-status/format-exif")
}

/// C# `value.ToString("N0", provider)` on a 64-bit integer: digits grouped in threes with `group_separator`,
/// sign from `numbers.negative_sign` (`-1048576` gives `-1,048,576` in the invariant culture).
pub fn format_grouped_integer(value: i64, numbers: &NumberCulture, group_separator: &str) -> String {
    todo!("W8-02b: format_grouped_integer, see contract trap T22")
}

/// C# `string.IsNullOrWhiteSpace(text)`: true for empty text and for text made only of Unicode whitespace.
pub fn is_blank(text: &str) -> bool {
    todo!("W8-02b: is_blank")
}

/// C# `text.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)`. Compares per char with `pr_core::text::ordinal_upper`;
/// never `to_lowercase` (contract trap T8). An empty prefix is a prefix of every text.
pub fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    todo!("W8-02b: starts_with_ignore_case")
}
