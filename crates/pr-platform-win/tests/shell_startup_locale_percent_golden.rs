//! Oracle golden `percent-one-decimal` against `WindowsNumberFormatting::percent_one_decimal` (BACKLOG G3 L6 / G15 L6).
//!
//! C# source: `DiagnosticsPresenter.Build`, `SecondaryDialogPresenters.cs:56`, `$"{ratio:P1}"` under `CultureInfo.CurrentCulture`.
//! The golden holds `ratio.ToString("P1")` for 37 cultures (every language code of the Rust shape table in
//! `shell_startup/locale.rs`, plus `en`, `vi` and a few default-shape languages) and 12 ratios.
//! The Rust function reads the Windows USER locale, which a test cannot change, so only the machine's own locale can be compared.
//! Known differences from C# (-0.0% sign, plain space vs U+00A0/U+202F, suffix vs prefix, no space for pl/uk/hu/bg/el/it/pt) are proposal P-92
//! (docs/PROPOSED-DECISIONS.md); `percent_one_decimal` is unchanged.
use pr_app::presenters::secondary::NumberFormatting;
use pr_platform_win::shell_startup::locale::WindowsNumberFormatting;
use pr_test_support::golden::{self, str_of};
use serde_json::Value;

/// The language codes of the hard-coded table in `percent_one_decimal` (`"fr"`, `"tr" | "fa"`, and the NBSP list).
const TABLE_LANGUAGES: [&str; 20] = [
    "fr", "tr", "fa", "de", "es", "ru", "cs", "sk", "sv", "fi", "nb", "da", "pl", "uk", "ro", "hu", "bg", "el", "it",
    "pt",
];
/// Languages whose golden text differs from the Rust shape table (P-92).
const P92_LANGUAGES: [&str; 18] = [
    "fr", "fa", "de", "es", "ru", "cs", "sk", "sv", "fi", "nb", "da", "pl", "uk", "ro", "hu", "bg", "el", "it",
];

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
            if !include_negative_zero && ratio == NEGATIVE_ZERO_RATIO {
                return None;
            }
            let actual = formatter.percent_one_decimal(ratio);
            let expected = str_of(case, "text");
            (actual != expected).then(|| format!("{culture} ratio {ratio}: rust {actual:?} vs C# {expected:?}"))
        })
        .collect()
}

// C# oracle `percent-one-decimal`: the golden must cover every language of the Rust table (a pruned golden would hide a language).
#[test]
fn percent_golden_covers_every_language_of_the_rust_table() {
    let golden = golden::load("percent-one-decimal");
    for language in TABLE_LANGUAGES.iter().chain(["en", "vi"].iter()) {
        let count = golden
            .cases
            .iter()
            .filter(|c| str_of(c, "culture") == *language)
            .count();
        assert_eq!(count, 12, "{language}: expected 12 ratios in the golden");
    }
}

// C# `{ratio:P1}`: for the machine's own locale, when its full name is in the golden and its language is not a P-92 language, Rust equals C# for all 12 ratios.
// Locales outside that set (including every P-92 language) are not asserted here; `..._matches_csharp_for_every_golden_ratio` documents them.
#[test]
fn percent_one_decimal_matches_csharp_golden_for_a_non_p92_user_locale() {
    let Some(name) = user_locale_name() else { return };
    let language = name.split(['-', '_']).next().unwrap_or_default().to_owned();
    if P92_LANGUAGES.contains(&language.as_str()) {
        return;
    }
    let cases = cases_of(&name);
    if cases.is_empty() {
        return;
    }
    let diffs = mismatches(&name);
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

// P-92: the user locale's language is in the Rust shape table and the table shape differs from C# `P1`.
#[test]
#[ignore = "P-92: Rust per-language shape table differs from C# P1 (docs/PROPOSED-DECISIONS.md); run on a fr/de/fa/pl/... machine"]
fn percent_one_decimal_matches_csharp_for_every_golden_ratio() {
    let name = user_locale_name().expect("user locale");
    let diffs = mismatches(&name);
    assert!(!cases_of(&name).is_empty(), "{name} is not a golden culture");
    assert!(diffs.is_empty(), "{diffs:#?}");
}

// P-92 part 2: a negative ratio that rounds to zero keeps its sign in C# (`-0.0%`); Rust drops it (`0.0%`). Fails today on every locale.
#[test]
#[ignore = "P-92: C# prints -0.0% for -0.000049, Rust prints 0.0% (docs/PROPOSED-DECISIONS.md)"]
fn percent_one_decimal_keeps_the_sign_of_a_negative_value_that_rounds_to_zero() {
    let name = user_locale_name().expect("user locale");
    let diffs = mismatches_where(&name, true);
    assert!(diffs.is_empty(), "{diffs:#?}");
}
