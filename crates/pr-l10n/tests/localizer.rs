//! Port of the `Localizer` tests of `PhotoReview.Core.Tests/Localization` (C# baseline d92d6aae): `LocalizerTests`
//! (the non-template half), `LocalizerPartialOverlayTests`, `TranslatorModesTests`, `MutationGapLocalizationTests`,
//! `FormatPluralResolutionTests`, `LocalizerConcurrencyTests` and the localizer parts of
//! `LocalizationFuzzTests`. Each test names its C# twin in a comment.
//!
//! `Localizer::current()` is process-wide: every test that reads or replaces it holds [`serial`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_core::abstractions::FileSystem;
use pr_l10n::catalog::{LanguageCatalog, PluralRule};
use pr_l10n::loader::LanguageLoader;
use pr_l10n::localizer::{
    EMBEDDED_ENGLISH_JSON, ENGLISH_CODE, Localizer, built_in_english, built_in_english_localizer, create_pseudo,
    create_show_keys, number_culture, on_current_changed,
};
use pr_l10n::template::{LocArg, NumberCulture, PSEUDO_CODE, accent};
use pr_test_support::FakeFileSystem;

const ENGLISH_JSON: &str = r#"
{
  "_meta": { "code": "en", "name": "English", "nativeName": "English", "plural": "one-other" },
  "plain": "Plain text",
  "greet": "Hello {name}",
  "move": "Moved {count} files to {folder}",
  "files.one": "{count} file",
  "files.other": "{count} files",
  "items.other": "{count} items"
}"#;

static GLOBAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    GLOBAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Restores the ambient localizer when a test ends (also when it panics).
struct RestoreCurrent(Arc<Localizer>);

impl RestoreCurrent {
    fn new() -> Self {
        Self(Localizer::current())
    }
}

impl Drop for RestoreCurrent {
    fn drop(&mut self) {
        Localizer::set_current(Arc::clone(&self.0));
    }
}

fn parse(json: &str) -> LanguageCatalog {
    let mut warnings = Vec::new();
    LanguageCatalog::try_parse(json, "test", &mut warnings)
        .unwrap_or_else(|| panic!("catalog is invalid: {}", warnings.join("; ")))
}

fn english() -> LanguageCatalog {
    parse(ENGLISH_JSON)
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

fn arg(name: &str, value: impl Into<pr_l10n::template::LocValue>) -> LocArg {
    LocArg::new(name, value)
}

fn sorted_keys(localizer: &Localizer) -> Vec<String> {
    let mut keys: Vec<String> = localizer.keys().map(str::to_owned).collect();
    keys.sort_unstable();
    keys
}

#[test]
fn create_english_only_uses_english_without_warnings() {
    // C#: LocalizerTests.Create_EnglishOnly_UsesEnglishWithoutWarnings
    let english = english();

    let localizer = Localizer::create(&english, &[], &[]);

    assert_eq!(localizer.code(), "en");
    assert_eq!(localizer.plural(), PluralRule::OneOther);
    assert!(localizer.warnings().is_empty());
    assert_eq!(localizer.get("plain"), "Plain text");
    assert_eq!(localizer.format("greet", &[arg("name", "Ann")]), "Hello Ann");
    assert!(localizer.contains("greet"));
    let mut expected: Vec<String> = english.entries().iter().map(|(k, _)| k.clone()).collect();
    expected.sort_unstable();
    assert_eq!(sorted_keys(&localizer), expected);
}

#[test]
fn create_overlay_overrides_english_and_takes_overlay_meta() {
    // C#: LocalizerTests.Create_Overlay_OverridesEnglishAndTakesOverlayMeta
    let vi =
        parse(r#"{ "_meta": { "code": "vi", "nativeName": "Tiếng Việt", "plural": "none" }, "greet": "Chào {name}" }"#);

    let localizer = Localizer::create(&english(), &[vi], &[]);

    assert!(localizer.warnings().is_empty());
    assert_eq!(localizer.code(), "vi");
    assert_eq!(localizer.native_name(), "Tiếng Việt");
    assert_eq!(localizer.plural(), PluralRule::None);
    assert_eq!(localizer.format("greet", &[arg("name", "Ann")]), "Chào Ann");
    assert_eq!(localizer.get("plain"), "Plain text"); // untranslated key falls back to English
}

#[test]
fn create_unknown_key_ignored_with_warning() {
    // C#: LocalizerTests.Create_UnknownKey_IgnoredWithWarning
    let vi = overlay("vi", "\"not.in.english\": \"x\"", "one-other");

    let localizer = Localizer::create(&english(), &[vi], &[]);

    assert!(!localizer.contains("not.in.english"));
    assert_eq!(localizer.get("not.in.english"), "not.in.english");
    assert!(has_warning(
        localizer.warnings(),
        "vi: unknown key 'not.in.english' ignored"
    ));
}

#[test]
fn create_translation_with_unknown_placeholder_rejected_keeps_english_with_warning() {
    // C#: LocalizerTests.Create_TranslationWithUnknownPlaceholder_RejectedKeepsEnglishWithWarning
    let vi = overlay("vi", "\"greet\": \"Chào {nam}\"", "one-other");

    let localizer = Localizer::create(&english(), &[vi], &[]);

    assert_eq!(localizer.format("greet", &[arg("name", "Ann")]), "Hello Ann");
    assert!(has_warning(
        localizer.warnings(),
        "'greet' uses unknown placeholder {nam}"
    ));
}

#[test]
fn create_translation_dropping_placeholder_accepted_with_warning() {
    // C#: LocalizerTests.Create_TranslationDroppingPlaceholder_AcceptedWithWarning
    let vi = overlay("vi", "\"move\": \"Đã chuyển {count} tệp\"", "one-other");

    let localizer = Localizer::create(&english(), &[vi], &[]);

    assert_eq!(
        localizer.format("move", &[arg("count", 2), arg("folder", "X")]),
        "Đã chuyển 2 tệp"
    );
    assert_eq!(localizer.warnings().len(), 1);
    assert!(has_warning(
        localizer.warnings(),
        "'move' does not use placeholder {folder}"
    ));
}

#[test]
fn create_translation_reordering_placeholders_accepted_without_warning() {
    // C#: LocalizerTests.Create_TranslationReorderingPlaceholders_AcceptedWithoutWarning
    let vi = overlay("vi", "\"move\": \"{folder} <- {count}\"", "one-other");

    let localizer = Localizer::create(&english(), &[vi], &[]);

    assert!(localizer.warnings().is_empty());
    assert_eq!(
        localizer.format("move", &[arg("count", 2), arg("folder", "D")]),
        "D <- 2"
    );
}

#[test]
fn create_translation_with_bad_braces_rejected_keeps_english_with_warning() {
    // C#: LocalizerTests.Create_TranslationWithBadBraces_RejectedKeepsEnglishWithWarning
    for text in ["Chào {name", "Chào } {name}", "Chào {}"] {
        let vi = overlay("vi", &format!("\"greet\": \"{text}\""), "one-other");

        let localizer = Localizer::create(&english(), &[vi], &[]);

        assert_eq!(localizer.format("greet", &[arg("name", "Ann")]), "Hello Ann", "{text}");
        assert!(
            has_warning(localizer.warnings(), "'greet' has unbalanced braces"),
            "{text}"
        );
    }
}

#[test]
fn create_english_with_bad_braces_uses_literal_with_warning() {
    // C#: LocalizerTests.Create_EnglishWithBadBraces_UsesLiteralWithWarning
    let english = parse(r#"{ "_meta": { "code": "en" }, "bad": "50% {off" }"#);

    let localizer = Localizer::create(&english, &[], &[]);

    assert_eq!(localizer.get("bad"), "50% {off");
    assert!(has_warning(localizer.warnings(), "en: 'bad' has invalid placeholders"));
}

#[test]
fn create_later_overlay_wins_per_key() {
    // C#: LocalizerTests.Create_LaterOverlay_WinsPerKey
    let shipped = overlay(
        "vi",
        "\"plain\": \"shipped plain\", \"greet\": \"shipped {name}\"",
        "one-other",
    );
    let user = overlay("vi", "\"greet\": \"user {name}\"", "one-other");

    let localizer = Localizer::create(&english(), &[shipped, user], &[]);

    assert_eq!(localizer.format("greet", &[arg("name", "A")]), "user A");
    assert_eq!(localizer.get("plain"), "shipped plain");
}

#[test]
fn create_rejected_user_entry_keeps_earlier_overlay_value() {
    // C#: LocalizerTests.Create_RejectedUserEntry_KeepsEarlierOverlayValue
    let shipped = overlay("vi", "\"greet\": \"shipped {name}\"", "one-other");
    let user = overlay("vi", "\"greet\": \"user {bogus}\"", "one-other");

    let localizer = Localizer::create(&english(), &[shipped, user], &[]);

    assert_eq!(localizer.format("greet", &[arg("name", "A")]), "shipped A");
    assert!(has_warning(localizer.warnings(), "unknown placeholder {bogus}"));
}

#[test]
fn create_prior_warnings_kept_first() {
    // C#: LocalizerTests.Create_PriorWarnings_KeptFirst
    let vi = overlay("vi", "\"nope\": \"x\"", "one-other");

    let localizer = Localizer::create(&english(), &[vi], &["file: broken".to_owned()]);

    assert_eq!(localizer.warnings()[0], "file: broken");
    assert_eq!(localizer.warnings().len(), 2);
}

#[test]
fn get_missing_key_returns_key() {
    // C#: LocalizerTests.Get_MissingKey_ReturnsKey
    let localizer = Localizer::create(&english(), &[], &[]);

    assert_eq!(localizer.get("missing.key"), "missing.key");
    assert_eq!(localizer.format("missing.key", &[arg("a", 1)]), "missing.key");
}

#[test]
fn get_template_with_placeholders_leaves_placeholders_visible() {
    // C#: LocalizerTests.Get_TemplateWithPlaceholders_LeavesPlaceholdersVisible
    assert_eq!(Localizer::create(&english(), &[], &[]).get("greet"), "Hello {name}");
}

#[test]
fn format_plural_one_other_picks_one_only_for_count_one() {
    // C#: LocalizerTests.FormatPlural_OneOther_PicksOneOnlyForCountOne (the ambient number culture is invariant)
    let localizer = Localizer::create(&english(), &[], &[]);

    for (count, expected) in [
        (0, "0 files"),
        (1, "1 file"),
        (2, "2 files"),
        (-1, "-1 files"),
        (21, "21 files"),
    ] {
        assert_eq!(
            localizer.format_plural("files", count, &[arg("count", count)]),
            expected,
            "count {count}"
        );
    }
}

#[test]
fn format_plural_pinned_culture_literal_expectation() {
    // C#: LocalizerTests.FormatPlural_PinnedCulture_LiteralExpectation
    let localizer = Localizer::create(&english(), &[], &[]);
    let culture = NumberCulture::invariant();

    for (count, expected) in [
        (0, "0 files"),
        (1, "1 file"),
        (2, "2 files"),
        (-1, "-1 files"),
        (1234, "1234 files"),
    ] {
        assert_eq!(
            localizer.format_plural_with("files", count, &[arg("count", count)], &culture),
            expected
        );
    }
}

#[test]
fn format_plural_plural_none_always_uses_other() {
    // C#: LocalizerTests.FormatPlural_PluralNone_AlwaysUsesOther
    let vi = overlay("vi", "\"files.one\": \"MỘT\", \"files.other\": \"{count} tệp\"", "none");
    let localizer = Localizer::create(&english(), &[vi], &[]);

    for count in [0, 1, 5] {
        assert_eq!(
            localizer.format_plural("files", count, &[arg("count", count)]),
            format!("{count} tệp")
        );
    }
}

#[test]
fn format_plural_one_form_absent_falls_back_to_other() {
    // C#: LocalizerTests.FormatPlural_OneFormAbsent_FallsBackToOther
    let localizer = Localizer::create(&english(), &[], &[]);

    assert_eq!(localizer.format_plural("items", 1, &[arg("count", 1)]), "1 items");
}

#[test]
fn format_plural_no_forms_at_all_returns_other_key() {
    // C#: LocalizerTests.FormatPlural_NoFormsAtAll_ReturnsOtherKey
    assert_eq!(
        Localizer::create(&english(), &[], &[]).format_plural("nothing", 1, &[]),
        "nothing.other"
    );
}

#[test]
fn built_in_english_embedded_catalog_is_valid_and_complete() {
    // C#: LocalizerTests.BuiltInEnglish_EmbeddedCatalog_IsValidAndComplete
    let english = built_in_english();
    let localizer = built_in_english_localizer();

    assert_eq!(english.code(), "en");
    assert!(!english.entries().is_empty());
    assert!(localizer.warnings().is_empty());
    assert_eq!(localizer.code(), ENGLISH_CODE);
}

fn asset_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("languages")
        .join(file)
}

#[test]
fn shipped_english_file_matches_embedded_catalog() {
    // C#: LocalizerTests.ShippedEnglishFile_MatchesEmbeddedCatalog
    let text = std::fs::read_to_string(asset_path("en.json")).expect("assets/languages/en.json");
    let from_disk = parse(&text);

    let mut embedded: Vec<(String, String)> = built_in_english().entries().to_vec();
    let mut shipped: Vec<(String, String)> = from_disk.entries().to_vec();
    embedded.sort();
    shipped.sort();
    assert_eq!(embedded, shipped);
    assert_eq!(text, EMBEDDED_ENGLISH_JSON);
}

#[test]
fn shipped_vietnamese_against_built_in_english_has_no_warnings() {
    // C#: LocalizerTests.ShippedVietnamese_AgainstBuiltInEnglish_HasNoWarnings (parity gate)
    let path = asset_path("vi.json");
    let text = std::fs::read_to_string(&path).expect("assets/languages/vi.json");
    let mut warnings = Vec::new();

    let vi = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings).expect("valid");

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(vi.code(), "vi");
    assert_eq!(vi.plural(), PluralRule::None);
    let localizer = Localizer::create(built_in_english(), &[vi], &[]);
    assert!(localizer.warnings().is_empty(), "{:?}", localizer.warnings());
}

#[test]
fn current_not_replaced_is_built_in_english() {
    // C#: LocalizerCurrentTests.Current_NotReplaced_IsBuiltInEnglish
    let _lock = serial();
    let _restore = RestoreCurrent::new();
    Localizer::set_current(Arc::new(Localizer::create(&english(), &[], &[])));

    Localizer::clear_current_for_tests();

    assert!(Arc::ptr_eq(&Localizer::current(), &built_in_english_localizer()));
    assert_eq!(Localizer::current().code(), "en");
}

#[test]
fn set_current_new_localizer_replaces_current_and_raises_current_changed() {
    // C#: LocalizerCurrentTests.SetCurrent_NewLocalizer_ReplacesCurrentAndRaisesCurrentChanged
    let _lock = serial();
    let previous = Localizer::current();
    let _restore = RestoreCurrent::new();
    let replacement = Arc::new(Localizer::create(
        &english(),
        &[overlay("vi", "\"plain\": \"Chữ\"", "one-other")],
        &[],
    ));
    let raised = Arc::new(AtomicUsize::new(0));
    let seen: Arc<Mutex<Option<Arc<Localizer>>>> = Arc::new(Mutex::new(None));
    let subscription = {
        let raised = Arc::clone(&raised);
        let seen = Arc::clone(&seen);
        on_current_changed(move || {
            raised.fetch_add(1, Ordering::SeqCst);
            *seen.lock().unwrap_or_else(PoisonError::into_inner) = Some(Localizer::current());
        })
    };

    Localizer::set_current(Arc::clone(&replacement));

    assert_eq!(raised.load(Ordering::SeqCst), 1);
    let seen_in_handler = seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .expect("handler ran");
    assert!(Arc::ptr_eq(&seen_in_handler, &replacement));
    assert!(Arc::ptr_eq(&Localizer::current(), &replacement));
    assert_eq!(Localizer::current().get("plain"), "Chữ");

    // Dropping the subscription is `CurrentChanged -= handler`.
    drop(subscription);
    Localizer::set_current(Arc::clone(&previous));
    assert_eq!(raised.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&Localizer::current(), &previous));
}

#[test]
fn create_partial_user_overlay_keeps_shipped_meta() {
    // C#: LocalizerPartialOverlayTests.Create_PartialUserOverlay_KeepsShippedMeta
    let shipped_vi = parse(
        r#"{ "_meta": { "code": "vi", "nativeName": "Tiếng Việt", "plural": "none" }, "files.other": "{count} tệp" }"#,
    );
    let user_vi = parse(r#"{ "_meta": { "code": "vi" }, "greet": "Chào {name}" }"#);

    let localizer = Localizer::create(&english(), &[shipped_vi, user_vi], &[]);

    assert_eq!(localizer.code(), "vi");
    assert_eq!(localizer.native_name(), "Tiếng Việt");
    assert_eq!(localizer.plural(), PluralRule::None);
    assert_eq!(localizer.format_plural("files", 1, &[arg("count", 1)]), "1 tệp");
    assert_eq!(localizer.format("greet", &[arg("name", "Ann")]), "Chào Ann");
}

#[test]
fn create_user_overlay_declaring_meta_wins() {
    // C#: LocalizerPartialOverlayTests.Create_UserOverlayDeclaringMeta_Wins
    let shipped_vi = parse(
        r#"{ "_meta": { "code": "vi", "nativeName": "Tiếng Việt", "plural": "none" }, "files.other": "{count} tệp" }"#,
    );
    let user_vi = parse(r#"{ "_meta": { "code": "vi", "nativeName": "VN", "plural": "one-other" } }"#);

    let localizer = Localizer::create(&english(), &[shipped_vi, user_vi], &[]);

    assert_eq!(localizer.native_name(), "VN");
    assert_eq!(localizer.plural(), PluralRule::OneOther);
}

#[test]
fn create_different_language_overlay_uses_its_own_defaults() {
    // C#: LocalizerPartialOverlayTests.Create_DifferentLanguageOverlay_UsesItsOwnDefaults
    let fr = parse(r#"{ "_meta": { "code": "fr" } }"#);

    let localizer = Localizer::create(&english(), &[fr], &[]);

    assert_eq!(localizer.native_name(), "fr");
    assert_eq!(localizer.plural(), PluralRule::OneOther);
}

#[test]
fn create_pseudo_plain_text_is_accented_bracketed_and_longer() {
    // C#: TranslatorModesTests.CreatePseudo_PlainText_IsAccentedBracketedAndLonger
    let pseudo = create_pseudo(&built_in_english_localizer());

    let text = pseudo.get(pr_l10n::keys::TEST_SAMPLE);

    assert!(text.starts_with('['), "{text}");
    assert!(text.ends_with(']'), "{text}");
    assert!(!text.contains("Sample"), "{text}");
    assert!(text.chars().count() as f64 > "Sample".len() as f64 * 1.3, "{text}");
    assert_eq!(pseudo.code(), PSEUDO_CODE);
    assert_eq!(pseudo.native_name(), "Pseudo");
}

#[test]
fn create_pseudo_placeholders_still_formatted() {
    // C#: TranslatorModesTests.CreatePseudo_Placeholders_StillFormatted
    let pseudo = create_pseudo(&built_in_english_localizer());

    let text = pseudo.format(pr_l10n::keys::TEST_GREETING, &[arg("name", "Ann")]);

    assert!(text.contains("Ann"), "{text}");
    assert!(!text.contains("{name}"), "{text}");
}

#[test]
fn create_pseudo_plural_keys_keep_plural() {
    // C#: TranslatorModesTests.CreatePseudo_PluralKeys_KeepPlural
    let pseudo = create_pseudo(&built_in_english_localizer());

    assert!(pseudo.format_plural("test.items", 2, &[arg("count", 2)]).contains('2'));
}

#[test]
fn create_show_keys_every_text_is_its_key() {
    // C#: TranslatorModesTests.CreateShowKeys_EveryText_IsItsKey
    let keys = create_show_keys(&built_in_english_localizer());

    assert_eq!(keys.get(pr_l10n::keys::TEST_SAMPLE), "[test.sample]");
    assert_eq!(
        keys.format(pr_l10n::keys::TEST_GREETING, &[arg("name", "Ann")]),
        "[test.greeting]"
    );
    assert_eq!(keys.code(), "en");
}

fn only_one_form(plural: &str) -> Localizer {
    let english = parse(
        r#"{ "_meta": { "code": "en", "name": "English", "nativeName": "English", "plural": "one-other" },
             "solo.one": "single {count}" }"#,
    );
    if plural == "one-other" {
        Localizer::create(&english, &[], &[])
    } else {
        Localizer::create(&english, &[overlay("xx", "", plural)], &[])
    }
}

#[test]
fn format_plural_only_one_form_one_other() {
    // C#: MutationGapLocalizationTests.FormatPlural_OnlyOneForm_OneOther
    let localizer = only_one_form("one-other");

    assert_eq!(localizer.format_plural("solo", 1, &[arg("count", 1)]), "single 1");
    assert_eq!(localizer.format_plural("solo", 5, &[arg("count", 5)]), "solo.other");
}

#[test]
fn format_plural_only_one_form_none_rule() {
    // C#: MutationGapLocalizationTests.FormatPlural_OnlyOneForm_NoneRule
    let localizer = only_one_form("none");

    assert_eq!(localizer.format_plural("solo", 1, &[arg("count", 1)]), "solo.other");
}

#[test]
fn create_pseudo_padding_length() {
    // C#: MutationGapLocalizationTests.CreatePseudo_PaddingLength
    let english = parse(
        r#"{ "_meta": { "code": "en", "name": "English", "nativeName": "English", "plural": "one-other" },
             "ten": "abcdefghij", "empty": "" }"#,
    );
    let localizer = Localizer::create(&english, &[], &[]);

    let pseudo = create_pseudo(&localizer);

    assert_eq!(pseudo.get("ten"), format!("[{} ~~~~]", accent("abcdefghij")));
    assert_eq!(pseudo.get("empty"), "[]");
}

#[test]
fn format_plural_picks_form_by_count_and_rule() {
    // C#: FormatPluralResolutionTests.FormatPlural_PicksFormByCountAndRule
    let one_other = Localizer::create(&english(), &[], &[]);
    assert_eq!(one_other.format_plural("files", 1, &[arg("count", 1)]), "1 file");
    assert_eq!(one_other.format_plural("files", 0, &[arg("count", 0)]), "0 files");
    assert_eq!(one_other.format_plural("files", 2, &[arg("count", 2)]), "2 files");
    assert_eq!(one_other.format_plural("items", 1, &[arg("count", 1)]), "1 items");

    let none = Localizer::create(&english(), &[overlay("xx", "", "none")], &[]);
    assert_eq!(none.format_plural("files", 1, &[arg("count", 1)]), "1 files");
}

#[test]
fn format_plural_uses_transformed_templates() {
    // C#: FormatPluralResolutionTests.FormatPlural_UsesTransformedTemplates
    let shouting = Localizer::create(&english(), &[], &[])
        .transform("xx", "xx", PluralRule::OneOther, |_, t| t.append_literal("!"));

    assert_eq!(shouting.format_plural("files", 1, &[arg("count", 1)]), "1 file!");
    assert_eq!(shouting.format_plural("files", 5, &[arg("count", 5)]), "5 files!");
}

#[test]
fn format_plural_unknown_base_key_returns_key() {
    // C#: FormatPluralResolutionTests.FormatPlural_UnknownBaseKey_ReturnsKey
    let localizer = Localizer::create(&english(), &[], &[]);

    assert_eq!(localizer.format_plural("nope", 3, &[arg("count", 3)]), "nope.other");
    assert_eq!(localizer.format_plural("nope", 1, &[arg("count", 1)]), "nope.other");
}

#[test]
fn transform_carries_warnings_over() {
    // Rust addition: C# `Transform` passes `Warnings` to the new instance.
    let vi = overlay("vi", "\"nope\": \"x\"", "one-other");
    let localizer = Localizer::create(&english(), &[vi], &["prior".to_owned()]);

    let copy = localizer.transform("xx", "XX", PluralRule::None, |_, t| t.clone());

    assert_eq!(copy.warnings(), localizer.warnings());
    assert_eq!(copy.code(), "xx");
    assert_eq!(copy.native_name(), "XX");
    assert_eq!(copy.plural(), PluralRule::None);
}

#[test]
fn swap_while_rendering_never_torn_or_null() {
    // C#: LocalizerConcurrencyTests.SwapWhileRendering_NeverTornOrNull
    let _lock = serial();
    let _restore = RestoreCurrent::new();
    let english = built_in_english_localizer();
    let pseudo = Arc::new(create_pseudo(&english));
    let show_keys = Arc::new(create_show_keys(&english));
    let key = sorted_keys(&english)
        .into_iter()
        .find(|k| english.get(k).len() > 3)
        .expect("a key with text");
    let allowed = [english.get(&key), pseudo.get(&key), show_keys.get(&key)];
    let stop = AtomicBool::new(false);
    let swaps = AtomicUsize::new(0);
    let bad: Mutex<Vec<String>> = Mutex::new(Vec::new());
    Localizer::set_current(Arc::clone(&english)); // pin a known value before the readers start

    std::thread::scope(|scope| {
        scope.spawn(|| {
            let mut i = 0_usize;
            while !stop.load(Ordering::SeqCst) {
                let next = match i % 3 {
                    0 => Arc::clone(&english),
                    1 => Arc::clone(&pseudo),
                    _ => Arc::clone(&show_keys),
                };
                i += 1;
                Localizer::set_current(next);
                swaps.fetch_add(1, Ordering::SeqCst);
            }
        });
        let readers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    for _ in 0..50_000 {
                        let text = Localizer::current().get(&key);
                        if !allowed.contains(&text) {
                            bad.lock().unwrap_or_else(PoisonError::into_inner).push(text);
                        }
                    }
                })
            })
            .collect();
        for reader in readers {
            reader.join().expect("reader thread");
        }
        stop.store(true, Ordering::SeqCst);
    });

    assert!(bad.lock().unwrap_or_else(PoisonError::into_inner).is_empty());
    assert!(swaps.load(Ordering::SeqCst) > 0);
}

#[test]
fn current_changed_sees_new_localizer() {
    // C#: LocalizerConcurrencyTests.CurrentChanged_SeesNewLocalizer
    let _lock = serial();
    let _restore = RestoreCurrent::new();
    let pseudo = Arc::new(create_pseudo(&built_in_english_localizer()));
    let seen: Arc<Mutex<Option<Arc<Localizer>>>> = Arc::new(Mutex::new(None));
    let count = Arc::new(AtomicUsize::new(0));
    let subscription = {
        let seen = Arc::clone(&seen);
        let count = Arc::clone(&count);
        on_current_changed(move || {
            *seen.lock().unwrap_or_else(PoisonError::into_inner) = Some(Localizer::current());
            count.fetch_add(1, Ordering::SeqCst);
        })
    };

    Localizer::set_current(Arc::clone(&pseudo));
    drop(subscription);

    let seen = seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .expect("handler ran");
    assert!(Arc::ptr_eq(&seen, &pseudo));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn unknown_and_mismatched_arguments_do_not_throw() {
    // C#: LocalizerConcurrencyTests.UnknownAndMismatchedArguments_DoNotThrow
    let english = built_in_english_localizer();

    assert_eq!(english.get("no.such.key"), "no.such.key");
    assert_eq!(english.format("no.such.key", &[arg("x", 1)]), "no.such.key");
    assert_eq!(english.format_plural("no.such.key", 3, &[]), "no.such.key.other");
    assert_eq!(english.get(""), "");
}

#[test]
fn overlay_hostile_is_contained() {
    // C#: LocalizationFuzzTests.Overlay_Hostile_IsContained
    let hostile = overlay(
        "xx",
        r#""greet": "Hi {name} {evil}", "move": "{{{count}}}", "plain": "}{", "files.one": "{count", "not.a.key": "x", "_x": "y",
           "files.other": "{count} {{files}}""#,
        "one-other",
    );

    let localizer = Localizer::create(&english(), &[hostile], &[]);

    assert!(!localizer.contains("not.a.key"));
    // Unknown placeholder: English kept.
    assert_eq!(localizer.format("greet", &[arg("name", "Ann")]), "Hello Ann");
    // Escaped braces around a placeholder.
    assert_eq!(localizer.format("move", &[arg("count", 5), arg("folder", "f")]), "{5}");
    // Unbalanced braces: English kept.
    assert_eq!(localizer.get("plain"), "Plain text");
    // Broken .one: English kept.
    assert_eq!(localizer.format_plural("files", 1, &[arg("count", 1)]), "1 file");
    assert_eq!(localizer.format_plural("files", 3, &[arg("count", 3)]), "3 {files}");
    assert!(has_warning(localizer.warnings(), "not.a.key"));
}

#[test]
fn format_plural_extreme_counts() {
    // C#: LocalizationFuzzTests.FormatPlural_ExtremeCounts
    let one_other = Localizer::create(&english(), &[], &[]);
    let none = Localizer::create(&english(), &[overlay("xx", "", "none")], &[]);

    for count in [i64::MIN, -1, 0, 1, 2, i64::MAX] {
        let a = one_other.format_plural("files", count, &[arg("count", count)]);
        let b = none.format_plural("files", count, &[arg("count", count)]);

        let expected_other = format!("{count} files");
        assert_eq!(
            a,
            if count == 1 {
                "1 file".to_owned()
            } else {
                expected_other.clone()
            }
        );
        assert_eq!(b, expected_other);
        assert_eq!(one_other.format_plural("missing", count, &[]), "missing.other");
    }
}

#[test]
fn shipped_catalogs_render_everywhere() {
    // C#: LocalizationFuzzTests.ShippedCatalogs_RenderEverywhere (cultures are explicit `NumberCulture`s)
    let fs = FakeFileSystem::new();
    for file in ["en.json", "vi.json"] {
        let bytes = std::fs::read(asset_path(file)).expect("shipped catalog");
        fs.add_file(format!(r"C:\App\Languages\{file}"), &bytes);
    }
    let file_system: Arc<dyn FileSystem> = Arc::new(fs);
    let loader = LanguageLoader::new(file_system, r"C:\App\Languages", None);
    let german = NumberCulture {
        decimal_separator: ",".to_owned(),
        ..NumberCulture::invariant()
    };
    let cultures = [NumberCulture::invariant(), german];

    let languages = loader.discover_languages();
    assert!(languages.iter().any(|l| l.code == "vi"));
    for language in languages {
        let localizer = loader.load(Some(&language.code), None);
        let sources = [create_pseudo(&localizer), create_show_keys(&localizer), localizer];
        for source in &sources {
            for key in sorted_keys(source) {
                let args = [
                    arg("count", 1_234_567_i64),
                    arg("name", "日本語 مرحبا 😀"),
                    arg("path", r"C:\x\y"),
                    arg("value", 1234.5678_f64),
                    arg("folder", "f"),
                    arg("size", 1024_i64),
                    arg("index", 3),
                ];
                for culture in &cultures {
                    let text = source.format_with(&key, &args, culture);
                    assert!(
                        !text.contains("{{") && !text.contains("}}"),
                        "{}:{key} -> {text}",
                        language.code
                    );
                }
            }
        }
    }
}

#[test]
fn pseudo_keeps_exotic_text() {
    // C#: LocalizationFuzzTests.Pseudo_KeepsExoticText
    let english = overlay(
        "en",
        r#""rtl": "\u0645\u0631\u062D\u0628\u0627 {name}", "cjk": "\u65E5\u672C\u8A9E {name} \u3067\u3059", "emoji": "\ud83d\ude00 {name} \ud83d\ude00""#,
        "one-other",
    );
    let localizer = Localizer::create(&english, &[], &[]);

    let pseudo = create_pseudo(&localizer);

    for key in ["rtl", "cjk", "emoji"] {
        let text = pseudo.format(key, &[arg("name", "N")]);
        assert!(text.starts_with('['), "{key}: {text}");
        assert!(text.ends_with(']'), "{key}: {text}");
        assert!(text.contains('N'), "{key}: {text}");
        assert!(!text.contains('\u{FFFD}'), "{key}: {text}");
    }
    assert!(pseudo.get("emoji").contains("\u{1F600}"));
}

#[test]
fn number_culture_defaults_to_invariant_and_is_not_the_ui_language() {
    // Rust addition (contract §14): the ambient number culture is independent of the localizer.
    let culture = number_culture();

    assert_eq!(*culture, NumberCulture::invariant());
}
