//! Oracle golden `percent-one-decimal` against `WindowsNumberFormatting::percent_one_decimal` (BACKLOG G3 L6 / G15 L6).
//!
//! C# source: `DiagnosticsPresenter.Build`, `SecondaryDialogPresenters.cs:56`, `$"{ratio:P1}"` under `CultureInfo.CurrentCulture`.
//! The golden holds `ratio.ToString("P1")` for 37 cultures (every language code of the Rust shape table in
//! `shell_startup/locale.rs`, plus `en`, `vi` and a few default-shape languages) and 12 ratios.
//! The Rust function reads the Windows USER locale, which a test cannot change, so only the machine's own locale can be compared.
//! `percent_one_decimal` reads the percent pattern and symbol from the OS (P-92 option A, decided 2026-11-03). Cultures where Windows NLS data differs from the .NET ICU
//! data the golden came from are listed in `NLS_DIFFERENT_CULTURES` / `NLS_DIFFERENT_NEGATIVE_CULTURES` and are not asserted (recorded in docs/reviews/D4-IMPL-2026-11-03.md).
use pr_app::presenters::secondary::NumberFormatting;
use pr_platform_win::shell_startup::locale::{WindowsNumberFormatting, percent_one_decimal_for_locale};
use pr_test_support::golden::{self, str_of};
use serde_json::Value;

/// Language codes the golden must keep covering (12 ratios each; formerly the shape table of `percent_one_decimal`, plus `en` and `vi`).
const GOLDEN_LANGUAGES: [&str; 22] = [
    "fr", "tr", "fa", "de", "es", "ru", "cs", "sk", "sv", "fi", "nb", "da", "pl", "uk", "ro", "hu", "bg", "el", "it",
    "pt", "en", "vi",
];
/// Cultures whose Windows NLS percent data differs from the ICU data of the golden for every ratio (decimal separator, digits, pattern or bidi marks), on the
/// Windows 10 19045 dev machine.
const NLS_DIFFERENT_CULTURES: [&str; 4] = ["fa", "fa-IR", "ar", "ru"];
/// Cultures whose only NLS/ICU difference is the negative form (U+2212 or a bidi mark in the golden, `-%n` for tr); positive ratios match.
const NLS_DIFFERENT_NEGATIVE_CULTURES: [&str; 5] = ["tr", "sv", "fi", "nb", "he"];

/// Newer Windows NLS builds use U+202F (narrow no-break space) as the French group separator where the .NET ICU data of the golden has U+00A0; the number part is produced by
/// GetNumberFormatEx and is not part of P-92, so the two are compared as equal (seen on the windows-latest runner for fr and fr-CH with ratios of 1000% and more).
fn same_text(actual: &str, expected: &str) -> bool {
    actual.replace('\u{202f}', "\u{a0}") == expected.replace('\u{202f}', "\u{a0}")
}

fn user_locale_name() -> Option<String> {
    let mut locale = [0u16; 85];
    // SAFETY: the output buffer is live and has the documented locale-name maximum capacity.
    let length = unsafe { windows::Win32::Globalization::GetUserDefaultLocaleName(&mut locale) };
    (length > 1).then(|| String::from_utf16_lossy(&locale[..(length - 1) as usize]))
}

fn cases_of(culture: &str) -> Vec<Value> {
    golden::load("percent-one-decimal")
        .cases
        .into_iter()
        .filter(|case| str_of(case, "culture") == culture)
        .collect()
}

/// The one ratio that differs in every locale (P-92 part 2): C# keeps the sign of a negative value that rounds to zero (`-0.0%`), Rust prints `0.0%`.
const NEGATIVE_ZERO_RATIO: f64 = -0.000_049;

fn mismatches(culture: &str) -> Vec<String> {
    mismatches_where(culture, false)
}

fn mismatches_where(culture: &str, include_negative_zero: bool) -> Vec<String> {
    let formatter = WindowsNumberFormatting;
    cases_of(culture)
        .iter()
        .filter_map(|case| {
            let ratio = case["ratio"].as_f64().expect("ratio");
            if NLS_DIFFERENT_CULTURES.contains(&culture)
                || (ratio < 0.0 && NLS_DIFFERENT_NEGATIVE_CULTURES.contains(&culture))
            {
                return None;
            }
            if !include_negative_zero && ratio == NEGATIVE_ZERO_RATIO {
                return None;
            }
            let actual = formatter.percent_one_decimal(ratio);
            let expected = str_of(case, "text");
            (!same_text(&actual, expected))
                .then(|| format!("{culture} ratio {ratio}: rust {actual:?} vs C# {expected:?}"))
        })
        .collect()
}

// C# oracle `percent-one-decimal`: the golden must cover every language of the Rust table (a pruned golden would hide a language).
#[test]
fn percent_golden_covers_every_language_of_the_rust_table() {
    let golden = golden::load("percent-one-decimal");
    for language in GOLDEN_LANGUAGES.iter() {
        let count = golden
            .cases
            .iter()
            .filter(|c| str_of(c, "culture") == *language)
            .count();
        assert_eq!(count, 12, "{language}: expected 12 ratios in the golden");
    }
}

// C# `{ratio:P1}`: for the machine's own locale, when its full name is in the golden, Rust equals C# for all 12 ratios (except the NLS-vs-ICU cultures above).
#[test]
fn percent_one_decimal_matches_csharp_golden_for_a_golden_user_locale() {
    let Some(name) = user_locale_name() else { return };
    if cases_of(&name).is_empty() {
        return;
    }
    let diffs = mismatches_where(&name, true);
    assert!(diffs.is_empty(), "{diffs:#?}");
}

// Always-on guard that the comparison above is real on the usual dev/CI locale: en-US is in the golden and must be asserted.
#[test]
fn percent_one_decimal_matches_csharp_golden_when_the_user_locale_is_en_us() {
    if user_locale_name().as_deref() != Some("en-US") {
        return;
    }
    assert_eq!(cases_of("en-US").len(), 12);
    assert!(mismatches("en-US").is_empty());
}

// P-92 option A: every golden culture, formatted by explicit locale name (NLS knows all of them whatever the user locale is), equals C# for all 12 ratios.
#[test]
fn percent_one_decimal_matches_csharp_for_every_golden_ratio() {
    let golden = golden::load("percent-one-decimal");
    let mut diffs = Vec::new();
    let mut compared = 0;
    for case in &golden.cases {
        let culture = str_of(case, "culture");
        let ratio = case["ratio"].as_f64().expect("ratio");
        if NLS_DIFFERENT_CULTURES.contains(&culture)
            || (ratio < 0.0 && NLS_DIFFERENT_NEGATIVE_CULTURES.contains(&culture))
        {
            continue;
        }
        compared += 1;
        let actual = percent_one_decimal_for_locale(culture, ratio);
        let expected = str_of(case, "text");
        if !same_text(&actual, expected) {
            diffs.push(format!("{culture} ratio {ratio}: rust {actual:?} vs C# {expected:?}"));
        }
    }
    assert!(compared > 350, "{compared} compared cases");
    assert!(diffs.is_empty(), "{diffs:#?}");
}

// P-92 part 2: a negative ratio that rounds to zero keeps its sign in C# (`-0.0%`), as does negative zero itself.
#[test]
fn percent_one_decimal_keeps_the_sign_of_a_negative_value_that_rounds_to_zero() {
    assert_eq!(percent_one_decimal_for_locale("en-US", NEGATIVE_ZERO_RATIO), "-0.0%");
    assert_eq!(percent_one_decimal_for_locale("fr-FR", NEGATIVE_ZERO_RATIO), "-0,0 %");
    assert_eq!(percent_one_decimal_for_locale("en-US", 0.0), "0.0%");
}

// .NET `PercentPositivePattern` 0 and 3 use a plain U+0020; the pattern comes from the OS, not a language table (de-CH is 1 `n%`, de is 0 `n %`).
#[test]
fn percent_one_decimal_uses_the_os_pattern_with_a_plain_space() {
    assert_eq!(percent_one_decimal_for_locale("de-DE", 0.5), "50,0 %");
    assert_eq!(percent_one_decimal_for_locale("de-CH", 0.5), "50.0%");
    assert_eq!(percent_one_decimal_for_locale("fr-FR", 0.5), "50,0 %");
    assert_eq!(percent_one_decimal_for_locale("pl-PL", 0.5), "50,0%");
    assert_eq!(percent_one_decimal_for_locale("tr-TR", 0.5), "%50,0");
}
