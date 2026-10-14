//! Port of the `LanguageCatalog` tests of `PhotoReview.Core.Tests/Localization` (C# baseline d92d6aae):
//! `LocalizerTests.LanguageCatalogTests`, `LanguageCatalogRobustnessTests`, and the catalog parts of
//! `LocalizationHardeningTests` and `LocalizationFuzzTests`. Each test names its C# twin in a comment.

use pr_l10n::catalog::{LanguageCatalog, MAX_FILE_BYTES, PluralRule};

fn parse(json: &str) -> LanguageCatalog {
    let mut warnings = Vec::new();
    LanguageCatalog::try_parse(json, "test", &mut warnings)
        .unwrap_or_else(|| panic!("catalog is invalid: {}", warnings.join("; ")))
}

fn overlay(code: &str, entries: &str, plural: &str) -> LanguageCatalog {
    let sep = if entries.is_empty() { "" } else { ", " };
    parse(&format!(
        r#"{{ "_meta": {{ "code": "{code}", "name": "{code}", "plural": "{plural}" }}{sep}{entries} }}"#
    ))
}

fn has_warning(warnings: &[String], fragment: &str) -> bool {
    warnings.iter().any(|w| w.contains(fragment))
}

/// Deterministic xorshift (the C# tests used `System.Random` with fixed seeds; the exact stream is not needed).
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn try_parse_valid_file_with_meta_reads_meta_and_entries() {
    // C#: LanguageCatalogTests.TryParse_ValidFileWithMeta_ReadsMetaAndEntries
    let json = r#"
        {
          "_meta": { "code": "VI", "name": "Vietnamese", "nativeName": "Tiếng Việt", "plural": "none",
                     "authors": ["A", 5, "B"], "formatVersion": 1 },
          "app.title": "Xem ảnh",
          "greet": "Chào {name}"
        }"#;
    let mut warnings = Vec::new();

    let catalog = LanguageCatalog::try_parse(json, "vi.json", &mut warnings).expect("valid");

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(catalog.code(), "vi");
    assert_eq!(catalog.name(), "Vietnamese");
    assert_eq!(catalog.native_name(), "Tiếng Việt");
    assert_eq!(catalog.plural(), PluralRule::None);
    assert_eq!(catalog.authors(), ["A", "B"]);
    assert_eq!(catalog.entries().len(), 2);
    assert_eq!(catalog.entry("greet"), Some("Chào {name}"));
    assert_eq!(catalog.entry("missing"), None);
}

#[test]
fn try_parse_plural_not_none_defaults_to_one_other() {
    // C#: LanguageCatalogTests.TryParse_PluralNotNone_DefaultsToOneOther
    assert_eq!(overlay("fr", "", "one-other").plural(), PluralRule::OneOther);
    assert_eq!(parse(r#"{ "_meta": { "code": "fr" } }"#).plural(), PluralRule::OneOther);
    assert_eq!(overlay("ja", "", "NONE").plural(), PluralRule::None);
}

#[test]
fn try_parse_names_missing_fall_back_to_name_then_code() {
    // C#: LanguageCatalogTests.TryParse_NamesMissing_FallBackToNameThenCode
    let code_only = parse(r#"{ "_meta": { "code": "fr" } }"#);
    let name_only = parse(r#"{ "_meta": { "code": "fr", "name": "French" } }"#);

    assert_eq!(code_only.name(), "fr");
    assert_eq!(code_only.native_name(), "fr");
    assert_eq!(name_only.native_name(), "French");
    assert!(!code_only.native_name_declared());
    assert!(name_only.native_name_declared());
}

#[test]
fn try_parse_missing_or_invalid_code_returns_false_with_warning() {
    // C#: LanguageCatalogTests.TryParse_MissingOrInvalidCode_ReturnsFalseWithWarning
    for json in [
        r#"{ "k": "v" }"#,
        r#"{ "_meta": { "name": "No code" }, "k": "v" }"#,
        r#"{ "_meta": { "code": "" } }"#,
        r#"{ "_meta": { "code": "x" } }"#,
        r#"{ "_meta": { "code": "1a" } }"#,
        r#"{ "_meta": { "code": "v i" } }"#,
        r#"{ "_meta": { "code": "vi_VN" } }"#,
        r#"{ "_meta": { "code": "abcdefghijklmnopq" } }"#,
        r#"{ "_meta": { "code": 12 } }"#,
    ] {
        let mut warnings = Vec::new();

        assert!(
            LanguageCatalog::try_parse(json, "bad.json", &mut warnings).is_none(),
            "{json}"
        );

        assert!(has_warning(&warnings, "_meta.code"), "{json}: {warnings:?}");
        assert!(warnings.iter().all(|w| w.starts_with("bad.json")), "{warnings:?}");
    }
}

#[test]
fn try_parse_meta_not_object_returns_false_with_warnings() {
    // C#: LanguageCatalogTests.TryParse_MetaNotObject_ReturnsFalseWithWarnings
    let mut warnings = Vec::new();

    assert!(LanguageCatalog::try_parse(r#"{ "_meta": "vi" }"#, "m.json", &mut warnings).is_none());

    assert!(has_warning(&warnings, "_meta must be an object"));
}

#[test]
fn try_parse_non_object_root_returns_false_with_warning() {
    // C#: LanguageCatalogTests.TryParse_NonObjectRoot_ReturnsFalseWithWarning
    for json in ["[]", "\"text\"", "42", "null"] {
        let mut warnings = Vec::new();

        assert!(LanguageCatalog::try_parse(json, "root.json", &mut warnings).is_none());

        assert!(has_warning(&warnings, "root must be a JSON object"), "{json}");
    }
}

#[test]
fn try_parse_invalid_json_returns_false_with_warning_instead_of_throwing() {
    // C#: LanguageCatalogTests.TryParse_InvalidJson_ReturnsFalseWithWarningInsteadOfThrowing
    for json in [
        "",
        "{",
        "{ \"_meta\": { \"code\": \"vi\" }, \"k\": }",
        "not json at all",
        "{ 'k': 'single quotes' }",
    ] {
        let mut warnings = Vec::new();

        assert!(LanguageCatalog::try_parse(json, "broken.json", &mut warnings).is_none());

        assert!(
            has_warning(&warnings, "broken.json: invalid JSON"),
            "{json:?}: {warnings:?}"
        );
    }
}

#[test]
fn try_parse_non_string_values_ignored_with_warning() {
    // C#: LanguageCatalogTests.TryParse_NonStringValues_IgnoredWithWarning
    let json =
        r#"{ "_meta": { "code": "vi" }, "num": 1, "obj": { "a": "b" }, "arr": ["x"], "nil": null, "ok": "fine" }"#;
    let mut warnings = Vec::new();

    let catalog = LanguageCatalog::try_parse(json, "v.json", &mut warnings).expect("valid");

    let keys: Vec<&str> = catalog.entries().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["ok"]);
    assert_eq!(warnings.len(), 4);
    assert!(has_warning(&warnings, "'num' is not a string"));
    assert!(has_warning(&warnings, "'nil' is not a string"));
}

#[test]
fn try_parse_underscore_keys_skipped_silently() {
    // C#: LanguageCatalogTests.TryParse_UnderscoreKeys_SkippedSilently
    let json = r#"{ "_meta": { "code": "vi" }, "_comment": "note", "_comment2": 5, "_section": { "x": 1 }, "k": "v" }"#;
    let mut warnings = Vec::new();

    let catalog = LanguageCatalog::try_parse(json, "v.json", &mut warnings).expect("valid");

    assert!(warnings.is_empty());
    let keys: Vec<&str> = catalog.entries().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["k"]);
}

#[test]
fn try_parse_comments_and_trailing_commas_accepted() {
    // C#: LanguageCatalogTests.TryParse_CommentsAndTrailingCommas_Accepted
    let json = "{\n  // line comment\n  \"_meta\": { \"code\": \"vi\", },\n  /* block comment */\n  \"k\": \"v\",\n}";
    let mut warnings = Vec::new();

    let catalog = LanguageCatalog::try_parse(json, "v.json", &mut warnings).expect("valid");

    assert!(warnings.is_empty());
    assert_eq!(catalog.entry("k"), Some("v"));
}

#[test]
fn try_parse_duplicate_key_last_value_wins() {
    // C#: LanguageCatalogTests.TryParse_DuplicateKey_LastValueWins
    let catalog = parse(r#"{ "_meta": { "code": "vi" }, "k": "first", "k": "second" }"#);

    assert_eq!(catalog.entry("k"), Some("second"));
    assert_eq!(catalog.entries().len(), 1);
}

#[test]
fn is_valid_code_various_codes_matches_rule() {
    // C#: LanguageCatalogTests.IsValidCode_VariousCodes_MatchesRule
    for (code, expected) in [
        ("vi", true),
        ("pt-br", true),
        ("zh-Hant-TW", true),
        ("x", false),
        ("-vi", false),
        ("vi.json", false),
        ("ví", false),
        ("a-", true),
        (" vi", false),
    ] {
        assert_eq!(LanguageCatalog::is_valid_code(code), expected, "{code:?}");
    }
}

#[test]
fn try_parse_lone_surrogate_escape_reports_warning_instead_of_throwing() {
    // C#: LanguageCatalogRobustnessTests.TryParse_LoneSurrogateEscape_ReportsWarningInsteadOfThrowing
    let json = r#"{ "_meta": { "code": "xx" }, "key": "\ud800" }"#;
    let mut warnings = Vec::new();

    let parsed = LanguageCatalog::try_parse(json, "bad.json", &mut warnings);

    assert!(parsed.is_none());
    assert!(warnings.iter().any(|w| w.contains("bad.json")));
    assert!(has_warning(&warnings, "unreadable text"));
}

#[test]
fn blank_display_names_fall_back() {
    // C#: LanguageCatalogRobustnessTests.BlankDisplayNames_FallBack (W2CM-03)
    for (name_json, native_json, expected_name, expected_native) in [
        ("\"\"", "\" \"", "xx", "xx"),
        ("\"French\"", "\" \\t\\n \"", "French", "French"),
        ("\"\"", "\"Français\"", "xx", "Français"),
    ] {
        let json =
            format!("{{ \"_meta\": {{ \"code\": \"xx\", \"name\": {name_json}, \"nativeName\": {native_json} }} }}");

        let catalog = parse(&json);

        assert_eq!(catalog.name(), expected_name, "{json}");
        assert_eq!(catalog.native_name(), expected_native, "{json}");
    }
}

#[test]
fn display_names_are_cleaned() {
    // C#: LanguageCatalogRobustnessTests.DisplayNames_AreCleaned (W2CM-03)
    let json = format!(
        "{{ \"_meta\": {{ \"code\": \"xx\", \"name\": \"  Eng\\u0007lish \", \"nativeName\": \"{}\" }} }}",
        "n".repeat(500)
    );

    let catalog = parse(&json);

    assert_eq!(catalog.name(), "Eng lish");
    assert_eq!(catalog.native_name().chars().count(), 64);
}

#[test]
fn warning_text_has_no_raw_line_breaks() {
    // C#: LanguageCatalogRobustnessTests.WarningText_HasNoRawLineBreaks (W2CM-02)
    let json = r#"{ "_meta": { "code": "xx", "plural": "a\nb" }, "k\n2026-10-04 [ERROR] forged": 5, "d\r\ne": "1", "d\r\ne": "2" }"#;
    let mut warnings = Vec::new();

    LanguageCatalog::try_parse(json, "t.json", &mut warnings);

    assert!(warnings.len() >= 3, "{warnings:?}");
    assert!(
        warnings.iter().all(|w| !w.chars().any(char::is_control)),
        "{warnings:?}"
    );
}

#[test]
fn duplicate_key_warns() {
    // C#: LocalizationHardeningTests.DuplicateKey_Warns
    let mut warnings = Vec::new();

    let catalog = LanguageCatalog::try_parse(
        r#"{ "_meta": {"code":"xx"}, "a": "first", "a": "second", "b": "x" }"#,
        "dup.json",
        &mut warnings,
    )
    .expect("valid");

    assert_eq!(catalog.entry("a"), Some("second"));
    let matching: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("dup.json") && w.contains("'a'"))
        .collect();
    assert_eq!(matching.len(), 1, "{warnings:?}");
}

#[test]
fn unknown_plural_warns() {
    // C#: LocalizationHardeningTests.UnknownPlural_Warns
    for (value, expected, warns) in [
        ("none", PluralRule::None, false),
        ("NONE", PluralRule::None, false),
        ("one-other", PluralRule::OneOther, false),
        ("other", PluralRule::OneOther, true),
        ("", PluralRule::OneOther, true),
        ("zero-one-few-many", PluralRule::OneOther, true),
    ] {
        let mut warnings = Vec::new();
        let json = format!("{{ \"_meta\": {{\"code\":\"xx\", \"plural\": \"{value}\"}} }}");

        let catalog = LanguageCatalog::try_parse(&json, "p.json", &mut warnings).expect("valid");

        assert_eq!(catalog.plural(), expected, "{value:?}");
        assert!(catalog.plural_declared(), "{value:?}");
        assert_eq!(
            warnings.iter().any(|w| w.contains("plural")),
            warns,
            "{value:?}: {warnings:?}"
        );
    }
}

#[test]
fn catalog_corrupted_bytes_never_throw() {
    // C#: LocalizationFuzzTests.Catalog_CorruptedBytes_NeverThrow
    let original = r#"{
  "_meta": { "code": "xx", "name": "Test", "nativeName": "Tést", "plural": "one-other", "authors": ["a"] },
  "_comment": "c",
  "plain": "Plain",
  "greet": "Hello {name}",
  "files.one": "{count} file",
  "files.other": "{count} files"
}"#
    .as_bytes()
    .to_vec();
    for seed in 1..=3 {
        let mut r = XorShift::new(seed);
        for _ in 0..1500 {
            let mut bytes = original.clone();
            match r.below(3) {
                0 => {
                    let i = r.below(bytes.len());
                    bytes[i] = r.below(256) as u8;
                }
                1 => bytes.truncate(r.below(bytes.len())),
                _ => {
                    for _ in 0..4 {
                        let i = r.below(bytes.len());
                        bytes[i] = r.below(256) as u8;
                    }
                }
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let mut warnings = Vec::new();

            // No panic is the assertion; the outcome may be either.
            let parsed = LanguageCatalog::try_parse(&text, "fuzz.json", &mut warnings);

            if let Some(catalog) = parsed {
                assert!(!catalog.code().is_empty());
            } else {
                assert!(!warnings.is_empty(), "a rejected catalog explains itself: {text:?}");
            }
        }
    }
}

#[test]
fn catalog_large_loads() {
    // C#: LocalizationFuzzTests.Catalog_Large_Loads
    let mut json = String::from("{ \"_meta\": {\"code\":\"xx\"}, \"big\": \"");
    json.push_str(&"y".repeat(900_000));
    json.push('"');
    for i in 0..50_000 {
        json.push_str(&format!(",\"k{i}\":\"v{i}\""));
    }
    json.push('}');
    assert!(
        json.len() as u64 > MAX_FILE_BYTES,
        "the parser itself has no size limit"
    );
    let mut warnings = Vec::new();

    let catalog = LanguageCatalog::try_parse(&json, "big.json", &mut warnings).expect("valid");

    assert!(warnings.is_empty());
    assert_eq!(catalog.entries().len(), 50_001);
    assert_eq!(catalog.entry("k49999"), Some("v49999"));
}

#[test]
fn catalog_code_turkish_i() {
    // C#: LocalizationFuzzTests.CatalogCode_TurkishI (the Rust lower-casing is ASCII only, so no culture can matter)
    let mut warnings = Vec::new();

    let id = LanguageCatalog::try_parse(r#"{"_meta":{"code":"ID"}}"#, "s", &mut warnings).expect("valid");
    let it = LanguageCatalog::try_parse(r#"{"_meta":{"code":"IT"}}"#, "s", &mut warnings).expect("valid");

    assert_eq!(id.code(), "id");
    assert_eq!(it.code(), "it");
}

#[test]
fn safe_replaces_control_characters_and_cuts_long_text() {
    // Rust addition (golden `source-with-control-chars-is-sanitized`, `source-longer-than-120-is-cut`).
    assert_eq!(LanguageCatalog::safe(None), "");
    assert_eq!(
        LanguageCatalog::safe(Some("a\nb\u{85}c\u{2028}")),
        "a\u{FFFD}b\u{FFFD}c\u{2028}"
    );
    let long = "k".repeat(121);
    assert_eq!(LanguageCatalog::safe(Some(&long)), format!("{}...", "k".repeat(120)));
    assert_eq!(LanguageCatalog::safe(Some(&"k".repeat(120))), "k".repeat(120));
    // A cut inside a surrogate pair keeps U+FFFD instead of a lone surrogate.
    let astral = format!("{}{}", "k".repeat(119), "\u{1F600}");
    assert_eq!(
        LanguageCatalog::safe(Some(&astral)),
        format!("{}\u{FFFD}...", "k".repeat(119))
    );
}
