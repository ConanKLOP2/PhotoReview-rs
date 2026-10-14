//! Ported tests for `pr_l10n::template` (C# `LocTemplate`, `LocArg`, `UserFacingError`, `TranslatorModes`).
//!
//! Each test names the C# test it ports in a `// C#:` comment. Golden-driven tests check EVERY case of
//! `tests/golden/loc-{template,render,pseudo}.json`.
//!
//! C# tests that cannot port (documented, not silently dropped):
//! * `Render_HostileArguments_DoNotThrowForFormatting`: the `decimal.MinValue` part is SKIPPED (no decimal type);
//!   `i64::MIN` stands in for the huge-number case.
//! * `Render_NumbersUseCurrentCulture`: the `date` argument is SKIPPED (`LocValue` has no dates; the app formats
//!   dates to `Str` first).
//! * `CreatePseudo_PluralKeys_KeepPlural`, `FormatPlural_*`, `JsonAlias_*`, `Overlay_*`, `Catalog_*`,
//!   `LanguageCatalogTests`, `LocalizerTests`: need `Localizer` / `LanguageCatalog` (later work units).
//! * `CreatePseudo_*` / `CreateShowKeys_*` on a `Localizer`: exercised here through `pseudo_template` and
//!   `show_key_template` on a parsed template; the `Localizer` plumbing is the Localizer unit.
//! * `Pseudo_KeepsExoticText`: Rust strings are always valid UTF-8, so the lone-surrogate check is implied and not
//!   repeated; the other assertions are kept.

use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use pr_l10n::template::{
    LocArg, LocTemplate, LocValue, LocalizedError, NumberCulture, PSEUDO_CODE, accent, describe, format_f64,
    is_localized, is_valid_placeholder_name, localize_io, localized_io, pseudo_padding, pseudo_template,
    show_key_template,
};
use pr_test_support::golden::{bool_of, load, str_of};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------------------------

fn parse(text: &str) -> LocTemplate {
    match LocTemplate::try_parse(text) {
        Some(t) => t,
        None => panic!("template should parse: {text:?}"),
    }
}

fn inv() -> NumberCulture {
    NumberCulture::invariant()
}

fn str_arg(name: &str, value: &str) -> LocArg {
    LocArg::new(name, LocValue::Str(value.to_owned()))
}

fn lazy_ok(text: &'static str) -> LocValue {
    LocValue::Lazy(Arc::new(move |_: &NumberCulture| Some(text.to_owned())))
}

fn lazy_none() -> LocValue {
    LocValue::Lazy(Arc::new(|_: &NumberCulture| None))
}

fn lazy_panics() -> LocValue {
    LocValue::Lazy(Arc::new(|_: &NumberCulture| -> Option<String> { panic!("boom") }))
}

/// The culture facts stored in `tests/golden/loc-render.json` (`kind == "culture"`), keyed by culture name.
fn golden_culture(name: &str) -> NumberCulture {
    let golden = load("loc-render");
    let case = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "culture" && c["name"] == name)
        .unwrap_or_else(|| panic!("no culture {name:?} in loc-render golden"));
    NumberCulture {
        negative_sign: str_of(case, "negative_sign").to_owned(),
        positive_sign: str_of(case, "positive_sign").to_owned(),
        decimal_separator: str_of(case, "decimal_separator").to_owned(),
        nan_symbol: str_of(case, "nan_symbol").to_owned(),
        positive_infinity_symbol: str_of(case, "positive_infinity_symbol").to_owned(),
        negative_infinity_symbol: str_of(case, "negative_infinity_symbol").to_owned(),
    }
}

/// The argument set of the golden `loc-template` / `loc-render` template cases (`LocFixedArgs` in the oracle).
fn fixed_args() -> Vec<LocArg> {
    vec![
        str_arg("x", "<X>"),
        str_arg("name", "<N>"),
        str_arg("count", "<C>"),
        str_arg("X", "<BIG>"),
    ]
}

fn names_of(case: &Value) -> Vec<String> {
    case["names"]
        .as_array()
        .unwrap_or_else(|| panic!("case has no names: {case}"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("name is not a string: {case}"))
                .to_owned()
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// LocTemplateTests (LocalizerTests.cs)
// ---------------------------------------------------------------------------------------------------------------

// C#: LocTemplateTests.Render_LiteralText_ReturnsTextUnchanged
#[test]
fn render_literal_text_returns_text_unchanged() {
    let template = parse("Just text.");

    assert_eq!(template.render(&[], &inv()), "Just text.");
    assert_eq!(template.text(), "Just text.");
}

// C#: LocTemplateTests.Render_EmptyText_ReturnsEmpty
#[test]
fn render_empty_text_returns_empty() {
    assert_eq!(parse("").render(&[], &inv()), "");
}

// C#: LocTemplateTests.Render_NamedPlaceholder_SubstitutesValue
#[test]
fn render_named_placeholder_substitutes_value() {
    let template = parse("Hello {name}!");

    assert_eq!(template.placeholder_names(), ["name".to_owned()]);
    assert_eq!(template.render(&[str_arg("name", "Ann")], &inv()), "Hello Ann!");
}

// C#: LocTemplateTests.Render_MultiplePlaceholdersInAnyArgOrder_SubstitutesEach
#[test]
fn render_multiple_placeholders_in_any_arg_order_substitutes_each() {
    let template = parse("{a}-{b}{c}");

    let args = [str_arg("c", "3"), str_arg("a", "1"), str_arg("b", "2")];
    assert_eq!(template.render(&args, &inv()), "1-23");
}

// C#: LocTemplateTests.Render_RepeatedPlaceholder_SubstitutesEveryOccurrence
#[test]
fn render_repeated_placeholder_substitutes_every_occurrence() {
    let template = parse("{x} and {x} again");

    assert_eq!(template.placeholder_names(), ["x".to_owned(), "x".to_owned()]);
    assert_eq!(template.render(&[str_arg("x", "7")], &inv()), "7 and 7 again");
}

// C#: LocTemplateTests.Render_DoubledBraces_RendersLiteralBraces
#[test]
fn render_doubled_braces_renders_literal_braces() {
    let template = parse("{{literal}} {v} }}{{");

    assert_eq!(template.placeholder_names(), ["v".to_owned()]);
    assert_eq!(template.render(&[str_arg("v", "ok")], &inv()), "{literal} ok }{");
}

// C#: LocTemplateTests.Render_MissingArgument_WritesPlaceholderBack
#[test]
fn render_missing_argument_writes_placeholder_back() {
    let template = parse("Moved {count} to {folder}");

    assert_eq!(template.render(&[str_arg("count", "3")], &inv()), "Moved 3 to {folder}");
}

// C#: LocTemplateTests.Render_ArgumentNameCaseDiffers_TreatedAsMissing
#[test]
fn render_argument_name_case_differs_treated_as_missing() {
    assert_eq!(parse("{name}").render(&[str_arg("Name", "x")], &inv()), "{name}");
}

// C#: LocTemplateTests.Render_NullValue_RendersEmpty
#[test]
fn render_null_value_renders_empty() {
    let args = [LocArg::new("v", LocValue::Null)];
    assert_eq!(parse("[{v}]").render(&args, &inv()), "[]");
}

// C#: LocTemplateTests.Render_NonFormattableObject_UsesToString
#[test]
fn render_non_formattable_object_uses_to_string() {
    // The C# `Token("path")` whose ToString is "path" is a Lazy formatter returning "path".
    let args = [LocArg::new("v", lazy_ok("path"))];
    assert_eq!(parse("[{v}]").render(&args, &inv()), "[path]");
}

// C#: LocTemplateTests.Render_FormattableValue_UsesCurrentCulture
#[test]
fn render_formattable_value_uses_given_culture() {
    let template = parse("{n}");
    let de = golden_culture("de-DE");

    assert_eq!(template.render(&[LocArg::new("n", 1234.5_f64)], &de), "1234,5");
    assert_eq!(template.render(&[LocArg::new("n", 1234.5_f64)], &inv()), "1234.5");
}

// C#: LocTemplateTests.TryParse_InvalidTemplate_ReturnsFalse (10 rows)
#[test]
fn try_parse_invalid_template_returns_false() {
    let rows = [
        "Unbalanced {name",
        "Stray } brace",
        "Trailing }",
        "Leading {",
        "{1x}",
        "{}",
        "{with space}",
        "{dot.name}",
        "{a{b}",
        "{ñ}",
    ];
    for text in rows {
        assert!(LocTemplate::try_parse(text).is_none(), "{text:?} must not parse");
    }
}

// C#: LocTemplateTests.TryParse_ValidTemplate_ReturnsTrue (4 rows)
#[test]
fn try_parse_valid_template_returns_true() {
    for text in ["{a1}", "{ABC}", "{{}}", "{{0}}"] {
        assert!(LocTemplate::try_parse(text).is_some(), "{text:?} must parse");
    }
}

// C#: LocTemplateTests.Literal_TextWithBraces_RendersVerbatim
#[test]
fn literal_text_with_braces_renders_verbatim() {
    let template = LocTemplate::literal("{not a placeholder");

    assert_eq!(template.render(&[str_arg("not", "x")], &inv()), "{not a placeholder");
}

// C#: TryParse rule "Valid name": non-empty, ASCII letter first, ASCII letters or digits after; no trimming.
#[test]
fn is_valid_placeholder_name_follows_the_ascii_rule() {
    assert!(is_valid_placeholder_name("a"));
    assert!(is_valid_placeholder_name("a1"));
    assert!(is_valid_placeholder_name("ABC"));
    assert!(!is_valid_placeholder_name(""));
    assert!(!is_valid_placeholder_name("1a"));
    assert!(!is_valid_placeholder_name(" x"));
    assert!(!is_valid_placeholder_name("x "));
    assert!(!is_valid_placeholder_name("a_b"));
    assert!(!is_valid_placeholder_name("ñ"));
}

// ---------------------------------------------------------------------------------------------------------------
// Golden-driven tests
// ---------------------------------------------------------------------------------------------------------------

// Golden `loc-template`: every parse and literal case (TryParse, PlaceholderNames, TransformLiterals identity,
// AppendLiteral, Render with and without arguments, Literal).
// C#: LocTemplateTests (golden twin of the parse/render rows)
#[test]
fn golden_loc_template_every_case() {
    let golden = load("loc-template");
    let args = fixed_args();
    let culture = inv();

    for case in &golden.cases {
        match str_of(case, "kind") {
            "parse" => {
                let text = str_of(case, "text");
                let parsed = LocTemplate::try_parse(text);
                assert_eq!(parsed.is_some(), bool_of(case, "ok"), "parse ok flag for {text:?}");
                let Some(t) = parsed else { continue };
                assert_eq!(t.text(), text, "text() for {text:?}");
                assert_eq!(t.placeholder_names(), names_of(case).as_slice(), "names for {text:?}");
                assert_eq!(
                    t.transform_literals(str::to_owned).text(),
                    str_of(case, "identity_text"),
                    "identity text for {text:?}"
                );
                let appended = t.append_literal("}{ {x} ");
                assert_eq!(
                    appended.text(),
                    str_of(case, "appended_text"),
                    "appended text for {text:?}"
                );
                assert_eq!(
                    appended.render(&args, &culture),
                    str_of(case, "appended_render"),
                    "appended render for {text:?}"
                );
                assert_eq!(
                    t.render(&[], &culture),
                    str_of(case, "render_no_args"),
                    "render without args for {text:?}"
                );
                assert_eq!(
                    t.render(&args, &culture),
                    str_of(case, "render_args"),
                    "render with args for {text:?}"
                );
            }
            "literal" => {
                let text = str_of(case, "text");
                let t = LocTemplate::literal(text);
                assert_eq!(t.text(), str_of(case, "template_text"), "literal text for {text:?}");
                assert_eq!(
                    t.placeholder_names(),
                    names_of(case).as_slice(),
                    "literal names for {text:?}"
                );
                assert_eq!(
                    t.render(&args, &culture),
                    str_of(case, "render_args"),
                    "literal render for {text:?}"
                );
                assert_eq!(
                    t.transform_literals(str::to_owned).text(),
                    str_of(case, "identity_text"),
                    "literal identity for {text:?}"
                );
                assert_eq!(
                    t.append_literal("}{ {x} ").text(),
                    str_of(case, "appended_text"),
                    "literal appended for {text:?}"
                );
            }
            other => panic!("unknown loc-template golden kind {other:?}"),
        }
    }
}

// Golden `loc-render`: NumberCulture facts, number rendering under every culture, argument scenarios (first match
// wins, failing formatter written back, null, bool, char, nulltostring).
// C#: LocTemplateTests.Render_FormattableValue_UsesCurrentCulture (generalised over the golden cultures)
#[test]
fn golden_loc_render_every_case() {
    let golden = load("loc-render");
    let template = parse("{n}");

    for case in &golden.cases {
        match str_of(case, "kind") {
            "culture" => {
                // Checked by every number case below, through the facts the NumberCulture is built from.
                assert!(!str_of(case, "decimal_separator").is_empty(), "culture {case}");
            }
            "number" => {
                let kind = str_of(case, "type");
                let value = str_of(case, "value");
                let rendered = case["rendered"]
                    .as_object()
                    .unwrap_or_else(|| panic!("number case has no rendered map: {case}"));
                for (culture_name, expected) in rendered {
                    let culture = golden_culture(culture_name);
                    let arg = typed_value(kind, value);
                    let got = template.render(&[LocArg::new("n", arg)], &culture);
                    assert_eq!(
                        Some(got.as_str()),
                        expected.as_str(),
                        "{kind} {value} under culture {culture_name:?}"
                    );
                }
            }
            "scenario" => {
                let text = str_of(case, "template");
                let args: Vec<LocArg> = case["args"]
                    .as_array()
                    .unwrap_or_else(|| panic!("scenario has no args: {case}"))
                    .iter()
                    .map(|a| {
                        let name = str_of(a, "name");
                        let kind = str_of(a, "type");
                        let value = a["value"].as_str();
                        LocArg::new(name, scenario_value(kind, value))
                    })
                    .collect();
                let t = parse(text);
                assert_eq!(t.render(&args, &inv()), str_of(case, "rendered"), "scenario {text:?}");
            }
            other => panic!("unknown loc-render golden kind {other:?}"),
        }
    }
}

/// Builds the `LocValue` for a number golden case (`LocArgValue` of the oracle, numeric kinds only).
fn typed_value(kind: &str, value: &str) -> LocValue {
    match kind {
        "i64" => LocValue::Int(value.parse().unwrap_or_else(|e| panic!("i64 {value}: {e}"))),
        "u64" => LocValue::UInt(value.parse().unwrap_or_else(|e| panic!("u64 {value}: {e}"))),
        "f64" => LocValue::F64(value.parse().unwrap_or_else(|e| panic!("f64 {value}: {e}"))),
        "f32" => LocValue::F32(value.parse().unwrap_or_else(|e| panic!("f32 {value}: {e}"))),
        other => panic!("unknown number type {other:?}"),
    }
}

/// Builds the `LocValue` for a scenario argument (`LocArgValue` of the oracle, all kinds).
fn scenario_value(kind: &str, value: Option<&str>) -> LocValue {
    match kind {
        "null" => LocValue::Null,
        "str" => LocValue::Str(value.unwrap_or_default().to_owned()),
        "i64" => typed_value("i64", value.unwrap_or_default()),
        "u64" => typed_value("u64", value.unwrap_or_default()),
        // The oracle writes C# "True"/"False" (not Rust's lowercase spelling).
        "bool" => LocValue::Bool(match value {
            Some("True") => true,
            Some("False") => false,
            other => panic!("bool golden value {other:?}"),
        }),
        "char" => LocValue::Char(
            value
                .unwrap_or_default()
                .chars()
                .next()
                .unwrap_or_else(|| panic!("empty char")),
        ),
        // C# object whose ToString throws: the formatter fails.
        "throw" => lazy_none(),
        // C# object whose ToString returns null: rendered as "".
        "nulltostring" => lazy_ok(""),
        other => panic!("unknown scenario arg type {other:?}"),
    }
}

// Golden `loc-pseudo`: padding for lengths 0..=300, and Accent / pseudo / show-keys output for 39 texts.
// C#: TranslatorModesTests + MutationGapLocalizationTests.CreatePseudo_PaddingLength (golden twin)
#[test]
fn golden_loc_pseudo_every_case() {
    let golden = load("loc-pseudo");
    let culture = inv();
    let pseudo_args = [
        str_arg("name", "N"),
        LocArg::new("n", LocValue::F64(1234.5)),
        str_arg("x", "<X>"),
    ];

    for case in &golden.cases {
        match str_of(case, "kind") {
            "padding" => {
                let length = case["length"]
                    .as_u64()
                    .unwrap_or_else(|| panic!("padding length: {case}"));
                let length = usize::try_from(length).unwrap_or_else(|e| panic!("length {length}: {e}"));
                assert_eq!(
                    pseudo_padding(length),
                    str_of(case, "padding"),
                    "padding for length {length}"
                );
            }
            "text" => {
                let text = str_of(case, "text");
                // The Localizer keeps a catalog text that does not parse as a verbatim literal (C# Localizer, W2);
                // the golden pins that: pseudo and English output of such texts are the raw text, accented.
                let t = LocTemplate::try_parse(text).unwrap_or_else(|| LocTemplate::literal(text));

                assert_eq!(accent(text), str_of(case, "accent"), "accent of {text:?}");
                assert_eq!(
                    t.render(&[], &culture),
                    str_of(case, "english_get"),
                    "english get of {text:?}"
                );
                let pseudo = pseudo_template(&t);
                assert_eq!(
                    pseudo.render(&[], &culture),
                    str_of(case, "pseudo_get"),
                    "pseudo get of {text:?}"
                );
                assert_eq!(
                    pseudo.render(&pseudo_args, &culture),
                    str_of(case, "pseudo_format"),
                    "pseudo format of {text:?}"
                );
                assert_eq!(
                    show_key_template("k").render(&[], &culture),
                    str_of(case, "show_keys_get"),
                    "show keys of {text:?}"
                );
                assert_eq!(str_of(case, "pseudo_code"), PSEUDO_CODE);
            }
            other => panic!("unknown loc-pseudo golden kind {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// LocalizationFuzzTests.cs
// ---------------------------------------------------------------------------------------------------------------

/// The `Pieces` list of `LocalizationFuzzTests`.
const PIECES: &[&str] = &[
    "{",
    "}",
    "{{",
    "}}",
    "a",
    "b1",
    " ",
    "é",
    "İ",
    "ı",
    "{x}",
    "{name}",
    "{count}",
    "{1}",
    "{ x}",
    "{}",
    "{x y}",
    "日本語",
    "مرحبا",
    "😀",
    "\n",
    "{{x}}",
    "{x}}",
    "{{x}",
    "%s",
    "{0}",
    "{X}",
    "{ñ}",
];

/// Deterministic SplitMix64 PRNG (stands in for `new Random(seed)`).
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n` (n > 0).
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_u64() % (n as u64)).unwrap_or(0)
    }
}

fn random_template_text(rng: &mut Rng) -> String {
    let count = rng.below(9);
    let mut text = String::new();
    for _ in 0..count {
        text.push_str(PIECES[rng.below(PIECES.len())]);
    }
    text
}

/// Independent reference renderer, written from the grammar rather than from the parser: `{{` and `}}` are
/// literal braces, `{name}` (ASCII letter then ASCII alphanumerics) is a placeholder (unknown names are written
/// back), any other brace is invalid. Returns `None` for an invalid template.
fn reference(text: &str, args: &[(&str, &str)]) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            if chars.get(i + 1) == Some(&'{') {
                out.push('{');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            if !chars.get(j).is_some_and(char::is_ascii_alphabetic) {
                return None;
            }
            j += 1;
            while chars.get(j).is_some_and(char::is_ascii_alphanumeric) {
                j += 1;
            }
            if chars.get(j) != Some(&'}') {
                return None;
            }
            let name: String = chars[i + 1..j].iter().collect();
            match args.iter().find(|(n, _)| *n == name) {
                Some((_, value)) => out.push_str(value),
                None => {
                    out.push('{');
                    out.push_str(&name);
                    out.push('}');
                }
            }
            i = j + 1;
        } else if c == '}' {
            if chars.get(i + 1) == Some(&'}') {
                out.push('}');
                i += 2;
                continue;
            }
            return None;
        } else {
            out.push(c);
            i += 1;
        }
    }
    Some(out)
}

// C#: LocalizationFuzzTests.LocTemplate_MatchesReference (6 seeds x 3000 templates)
#[test]
fn loc_template_matches_reference_six_seeds() {
    let ref_args = [("x", "<X>"), ("name", "<N>"), ("count", "<C>"), ("X", "<BIG>")];
    let locargs = fixed_args();
    let culture = inv();

    for seed in 1..=6_u64 {
        let mut rng = Rng(seed);
        for _ in 0..3000 {
            let text = random_template_text(&mut rng);
            let expected_valid = reference(&text, &[]).is_some();

            let parsed = LocTemplate::try_parse(&text);
            assert_eq!(parsed.is_some(), expected_valid, "validity of {text:?} (seed {seed})");
            let Some(template) = parsed else { continue };

            let expected = reference(&text, &ref_args).unwrap_or_default();
            assert_eq!(template.render(&locargs, &culture), expected, "render of {text:?}");
            assert_eq!(
                template.render(&[], &culture),
                reference(&text, &[]).unwrap_or_default(),
                "render without args of {text:?}"
            );

            // Identity transform and appended text keep the placeholders and stay parseable.
            let same = template.transform_literals(str::to_owned);
            let reparsed = parse_or_panic(same.text());
            assert_eq!(
                template.placeholder_names(),
                reparsed.placeholder_names(),
                "names of {text:?}"
            );
            assert_eq!(template.render(&locargs, &culture), reparsed.render(&locargs, &culture));

            let appended = template.append_literal("}{ {x} ");
            let appended_reparsed = parse_or_panic(appended.text());
            let expected_appended = template.render(&locargs, &culture) + "}{ {x} ";
            assert_eq!(
                appended.render(&locargs, &culture),
                expected_appended,
                "appended render of {text:?}"
            );
            assert_eq!(
                appended.render(&locargs, &culture),
                appended_reparsed.render(&locargs, &culture),
                "reparsed appended render of {text:?}"
            );
        }
    }
}

fn parse_or_panic(text: &str) -> LocTemplate {
    match LocTemplate::try_parse(text) {
        Some(t) => t,
        None => panic!("re-parse failed for {text:?}"),
    }
}

// C#: LocalizationFuzzTests.Render_HostileArguments_DoNotThrowForFormatting
// SKIPPED part: decimal.MinValue (no decimal type); i64::MIN stands in as the huge-number argument.
#[test]
fn render_hostile_arguments_do_not_throw_for_formatting() {
    let t = parse("[{a}] [{b}] [{c}]");
    let args = [
        LocArg::new("a", LocValue::Null),
        LocArg::new("b", lazy_ok("")),
        LocArg::new("c", LocValue::Int(i64::MIN)),
    ];

    let text = t.render(&args, &inv());

    assert_eq!(text, "[] [] [-9223372036854775808]");
}

// C#: LocalizationFuzzTests.Render_ThrowingFormatter_WritesThePlaceholderBack (R21)
#[test]
fn render_throwing_formatter_writes_the_placeholder_back() {
    let t = parse("[{a}] [{b}] [{c}]");
    let args = [
        LocArg::new("a", lazy_none()),
        LocArg::new("b", lazy_panics()),
        LocArg::new("c", LocValue::Int(7)),
    ];

    let text = t.render(&args, &inv());

    assert_eq!(text, "[{a}] [{b}] [7]");
    // The render after the failing one is unaffected (no shared state left behind).
    assert_eq!(parse("[{a}]").render(&[str_arg("a", "ok")], &inv()), "[ok]");
}

// C#: LocalizationFuzzTests.Render_NumbersUseCurrentCulture (date argument SKIPPED: LocValue has no dates)
#[test]
fn render_numbers_use_culture_for_six_cultures() {
    let t = parse("{n} | {n2}");
    for name in ["tr-TR", "de-DE", "ja-JP", "ar-SA", "fa-IR", ""] {
        let culture = golden_culture(name);
        let text = t.render(
            &[
                LocArg::new("n", LocValue::F64(1234.5)),
                LocArg::new("n2", LocValue::Int(1234)),
            ],
            &culture,
        );
        // The f64 part is pinned by the golden loc-render table for this culture; the integer has no separators.
        let golden_f64 = golden_rendered_f64(1234.5, name);
        assert_eq!(text, format!("{golden_f64} | 1234"), "culture {name:?}");
        assert_eq!(
            text,
            format!("{} | 1234", format_f64(1234.5, &culture)),
            "culture {name:?}"
        );
    }
}

fn golden_rendered_f64(value: f64, culture_name: &str) -> String {
    let golden = load("loc-render");
    let wanted = format!("{value}");
    let case = golden
        .cases
        .iter()
        .find(|c| c["kind"] == "number" && c["type"] == "f64" && c["value"] == wanted.as_str())
        .unwrap_or_else(|| panic!("no f64 {wanted} case in loc-render golden"));
    str_of(&case["rendered"], culture_name).to_owned()
}

// C#: LocalizationFuzzTests.Render_ParallelThreads_NoCrossTalk (8 threads x 2000 renders)
#[test]
fn render_parallel_threads_no_cross_talk() {
    let templates: Vec<LocTemplate> = (0..8)
        .map(|i| {
            let letter = char::from(b'a' + u8::try_from(i).unwrap_or(0));
            parse(&format!("T{i}-{{a}}-{}-{{b}}", letter.to_string().repeat(40)))
        })
        .collect();
    let culture = inv();

    let failures: Vec<String> = thread::scope(|scope| {
        let handles: Vec<_> = templates
            .iter()
            .enumerate()
            .map(|(i, template)| {
                let culture = &culture;
                scope.spawn(move || {
                    let letter = char::from(b'a' + u8::try_from(i).unwrap_or(0));
                    let expected = format!("T{i}-A{i}-{}-B{i}", letter.to_string().repeat(40));
                    let args = [str_arg("a", &format!("A{i}")), str_arg("b", &format!("B{i}"))];
                    for _ in 0..2000 {
                        let got = template.render(&args, culture);
                        if got != expected {
                            return Some(got);
                        }
                    }
                    None
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().unwrap_or_else(|_| Some("thread panicked".to_owned())))
            .collect()
    });

    assert!(failures.is_empty(), "mixed output: {failures:?}");
}

// C#: LocalizationFuzzTests.Render_Nested_KeepsOuterText
#[test]
fn render_nested_keeps_outer_text() {
    let outer = parse("outer[{inner}]end");
    let inner = Arc::new(parse("in{v}ner"));
    let inner_for_arg = Arc::clone(&inner);
    let nested = LocValue::Lazy(Arc::new(move |culture: &NumberCulture| {
        Some(inner_for_arg.render(&[str_arg("v", "<V>")], culture))
    }));

    let text = outer.render(&[LocArg::new("inner", nested)], &inv());

    assert_eq!(text, "outer[in<V>ner]end");
}

// C#: LocalizationFuzzTests.Pseudo_KeepsExoticText (texts parsed directly; Localizer plumbing is later)
#[test]
fn pseudo_keeps_exotic_text() {
    let texts = [
        "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} {name}",
        "\u{65E5}\u{672C}\u{8A9E} {name} \u{3067}\u{3059}",
        "\u{1F600} {name} \u{1F600}",
    ];
    for source in texts {
        let pseudo = pseudo_template(&parse(source));
        let text = pseudo.render(&[str_arg("name", "N")], &inv());
        assert!(text.starts_with('['), "{text}");
        assert!(text.ends_with(']'), "{text}");
        assert!(text.contains('N'), "{text}");
        assert!(!text.contains('\u{FFFD}'), "{text}");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// LocalizationHardeningTests.cs (UserFacingError part) and UserFacingError semantics
// ---------------------------------------------------------------------------------------------------------------

// C#: LocalizationHardeningTests.Describe_ThrowingFactory_FallsBackToMessage
#[test]
fn describe_throwing_factory_falls_back_to_message() {
    let err = localized_io(std::io::ErrorKind::Other, "os message", || -> String {
        panic!("catalog broke")
    });

    assert!(is_localized(&err));
    assert_eq!(describe(&err), "os message");
}

// C#: LocalizationHardeningTests.Describe_FollowsLanguageAtDisplayTime
// The "current language" is a shared Mutex<String> read by the sentence factory (no Localizer in this unit).
#[test]
fn describe_follows_language_at_display_time() {
    let language = Arc::new(Mutex::new("English text".to_owned()));
    let for_factory = Arc::clone(&language);
    let err = localized_io(std::io::ErrorKind::Other, "os", move || {
        for_factory.lock().unwrap_or_else(PoisonError::into_inner).clone()
    });

    let first = describe(&err);
    *language.lock().unwrap_or_else(PoisonError::into_inner) = "Pseudo text".to_owned();
    let second = describe(&err);

    assert_eq!(first, "English text");
    assert_eq!(second, "Pseudo text");
}

// localized_io keeps the kind and the English message (C# UserFacingError.Localized(new IOException(english), text)).
#[test]
fn localized_io_keeps_kind_and_english_message() {
    let err = localized_io(std::io::ErrorKind::InvalidData, "bad data", || "Dữ liệu lỗi".to_owned());

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "bad data");
    assert!(is_localized(&err));
    assert_eq!(describe(&err), "Dữ liệu lỗi");
}

// localize_io keeps kind and message of an existing OS error and makes the call idempotent.
#[test]
fn localize_io_keeps_kind_and_is_idempotent() {
    let original = std::io::Error::from(std::io::ErrorKind::NotFound);
    let message = original.to_string();

    let once = localize_io(original, || "Không tìm thấy".to_owned());
    assert_eq!(once.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(once.to_string(), message);
    assert_eq!(describe(&once), "Không tìm thấy");

    let twice = localize_io(once, || "other sentence".to_owned());
    assert_eq!(
        describe(&twice),
        "Không tìm thấy",
        "a second localize must not replace the sentence"
    );
}

// Plain errors are not localized; describe is their own message.
#[test]
fn describe_plain_error_is_its_message() {
    let err = std::io::Error::other("plain");

    assert!(!is_localized(&err));
    assert_eq!(describe(&err), "plain");
}

// describe strips Rust's " (os error N)" suffix so the text equals .NET Exception.Message (contract D5).
// DIVERGENCE: Rust appends the code to OS error text, .NET does not.
#[test]
fn describe_os_error_strips_rust_suffix() {
    let err = std::io::Error::from_raw_os_error(2);
    let raw = err.to_string();
    assert!(raw.ends_with(" (os error 2)"), "precondition: {raw}");

    let described = describe(&err);

    assert_eq!(described, raw.trim_end_matches(" (os error 2)"));
    assert!(!described.contains("(os error"));
}

// A localized OS error whose sentence factory panics falls back to the OS message without the suffix.
#[test]
fn describe_localized_os_error_with_panicking_factory_strips_suffix() {
    let localized = localize_io(std::io::Error::from_raw_os_error(5), || -> String {
        panic!("factory broke")
    });

    let described = describe(&localized);

    assert!(!described.contains("(os error"), "{described}");
    assert!(!described.is_empty());
}

// `is_localized` looks at the error itself only (the source chain is not searched).
#[test]
fn is_localized_does_not_walk_the_source_chain() {
    let inner = localized_io(std::io::ErrorKind::Other, "inner", || "x".to_owned());
    let outer = std::io::Error::other(LocalizedError::new("outer", Arc::new(|| "y".to_owned())));
    let wrapped = std::io::Error::other(inner);

    assert!(is_localized(&outer));
    assert!(!is_localized(&wrapped), "source() must not be searched");
}

// ---------------------------------------------------------------------------------------------------------------
// TranslatorModesTests.cs and MutationGapLocalizationTests.cs
// ---------------------------------------------------------------------------------------------------------------

// C#: TranslatorModesTests.CreatePseudo_PlainText_IsAccentedBracketedAndLonger (on the template "Sample")
#[test]
fn create_pseudo_plain_text_is_accented_bracketed_and_longer() {
    let text = pseudo_template(&parse("Sample")).render(&[], &inv());

    assert!(text.starts_with('['), "{text}");
    assert!(text.ends_with(']'), "{text}");
    assert!(!text.contains("Sample"), "{text}");
    assert!(
        text.chars().count() as f64 > "Sample".chars().count() as f64 * 1.3,
        "{text}"
    );
    assert_eq!(PSEUDO_CODE, "qps-ploc");
}

// C#: TranslatorModesTests.CreatePseudo_Placeholders_StillFormatted
#[test]
fn create_pseudo_placeholders_still_formatted() {
    let pseudo = pseudo_template(&parse("Hello {name}"));

    let text = pseudo.render(&[str_arg("name", "Ann")], &inv());

    assert!(text.contains("Ann"), "{text}");
    assert!(!text.contains("{name}"), "{text}");
}

// C#: TranslatorModesTests.CreateShowKeys_EveryText_IsItsKey
#[test]
fn create_show_keys_every_text_is_its_key() {
    assert_eq!(show_key_template("test.sample").render(&[], &inv()), "[test.sample]");
    assert_eq!(
        show_key_template("test.greeting").render(&[str_arg("name", "Ann")], &inv()),
        "[test.greeting]"
    );
}

// C#: TranslatorModesTests.Accent_AsciiLetters_MappedOneToOne
#[test]
fn accent_ascii_letters_mapped_one_to_one() {
    let accented = accent("abc XYZ 1!");

    assert_eq!(accented.chars().count(), 10);
    assert!(accented.ends_with(" 1!"), "{accented}");
    assert_ne!(accented, "abc XYZ 1!");
}

// C#: MutationGapLocalizationTests.Accent_MapsFirstLetter
#[test]
fn accent_maps_first_letter() {
    assert_eq!(accent("a"), "\u{00E1}");
    assert_eq!(accent("A"), "\u{00C1}");
    assert_eq!(accent("1-a"), "1-\u{00E1}");
}

// C#: MutationGapLocalizationTests.CreatePseudo_PaddingLength
#[test]
fn create_pseudo_padding_length() {
    let ten = pseudo_template(&parse("abcdefghij")).render(&[], &inv());
    let empty = pseudo_template(&parse("")).render(&[], &inv());

    assert_eq!(ten, format!("[{} ~~~~]", accent("abcdefghij")));
    assert_eq!(empty, "[]");
}

// Padding is ceil(0.35 x UTF-16 length), at least one tilde, and empty for length 0 (spot checks; golden covers 0..=300).
#[test]
fn pseudo_padding_spot_checks() {
    assert_eq!(pseudo_padding(0), "");
    assert_eq!(pseudo_padding(1), " ~");
    assert_eq!(pseudo_padding(10), " ~~~~");
    assert_eq!(pseudo_padding(40), " ~~~~~~~~~~~~~~");
}
