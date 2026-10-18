//! Display-culture facts shared by the formatters (C# `IFormatProvider` and `CultureInfo.CurrentCulture`).
//! Part of `pr_app::viewmodels::format` (W8-02b). Port target: `PhotoReview.App.Shared/ViewModels/{ExifFormatter,StatusFormatter}.cs`
//! (C# baseline d92d6aae). No `unsafe`, no `windows` crate: this is a pure module.

use std::fmt::Debug;

use pr_core::text::ordinal_upper;
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
///
/// .NET custom formats first round the value to 15 significant digits, then round the digits half away from zero.
/// The same two steps are done here on the decimal digits, so `1.005` gives `1.01` like .NET (not the binary-exact result).
pub fn format_custom_fixed(value: f64, max_decimals: u8, numbers: &NumberCulture) -> String {
    let negative = value.is_sign_negative();
    let (digits, point) = decimal_digits_15(value.abs());
    let max = i64::from(max_decimals);
    let keep = i64::from(point) + max;
    let len = i64::try_from(digits.len()).unwrap_or(i64::MAX);

    // Round at `keep` digits (digits before the index are kept). A negative `keep` rounds to zero.
    let (mut kept, round_up): (Vec<u8>, bool) = if keep < 0 {
        (Vec::new(), false)
    } else if keep >= len {
        (digits.clone(), false)
    } else {
        let cut = usize::try_from(keep).unwrap_or(0);
        (digits[..cut].to_vec(), digits[cut] >= 5)
    };
    let mut point = point;
    if round_up {
        let mut index = kept.len();
        loop {
            if index == 0 {
                kept.insert(0, 1);
                point += 1;
                break;
            }
            index -= 1;
            if kept[index] == 9 {
                kept[index] = 0;
            } else {
                kept[index] += 1;
                break;
            }
        }
    }

    let digit_at = |index: i64| -> u8 {
        usize::try_from(index)
            .ok()
            .and_then(|i| kept.get(i))
            .copied()
            .unwrap_or(0)
    };

    let mut out = String::new();
    if negative {
        out.push_str(&numbers.negative_sign);
    }
    if point <= 0 {
        out.push('0');
    } else {
        for index in 0..i64::from(point) {
            out.push(digit_char(digit_at(index)));
        }
    }

    let mut fraction = String::new();
    for k in 0..max {
        fraction.push(digit_char(digit_at(i64::from(point) + k)));
    }
    let trimmed = fraction.trim_end_matches('0');
    if !trimmed.is_empty() {
        out.push_str(&numbers.decimal_separator);
        out.push_str(trimmed);
    }
    out
}

/// The 15 significant decimal digits of a non-negative finite value and the position of the decimal point: the value is
/// `0.d1d2d3... * 10^point`. Zero gives no digits and point 0.
fn decimal_digits_15(value: f64) -> (Vec<u8>, i32) {
    if value == 0.0 {
        return (Vec::new(), 0);
    }
    let text = format!("{value:.14e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    while digits.len() > 1 && digits.last() == Some(&0) {
        digits.pop();
    }
    (digits, exponent + 1)
}

fn digit_char(digit: u8) -> char {
    char::from(b'0' + digit.min(9))
}

/// C# `value.ToString("N0", provider)` on a 64-bit integer: digits grouped in threes with `group_separator`,
/// sign from `numbers.negative_sign` (`-1048576` gives `-1,048,576` in the invariant culture).
pub fn format_grouped_integer(value: i64, numbers: &NumberCulture, group_separator: &str) -> String {
    let digits = value.unsigned_abs().to_string();
    let count = digits.len();
    let mut out = String::new();
    if value < 0 {
        out.push_str(&numbers.negative_sign);
    }
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (count - index) % 3 == 0 {
            out.push_str(group_separator);
        }
        out.push(ch);
    }
    out
}

/// C# `string.IsNullOrWhiteSpace(text)`: true for empty text and for text made only of Unicode whitespace.
pub fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

/// C# `text.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)`. Compares per char with `pr_core::text::ordinal_upper`;
/// never `to_lowercase` (contract trap T8). An empty prefix is a prefix of every text.
pub fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    let mut chars = text.chars();
    prefix
        .chars()
        .all(|p| chars.next().is_some_and(|c| ordinal_upper(c) == ordinal_upper(p)))
}
