//! Port of `GeneratedTrTests` (C# baseline d92d6aae): the generated `tr` API (from `assets/languages/en.json` by
//! `build.rs`) resolves through `Localizer::current()`. The ambient localizer is process-wide, so every test holds
//! [`serial`] and restores it. Each test names its C# twin in a comment.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::keys;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};
use pr_l10n::template::LocValue;
use pr_l10n::tr;

static GLOBAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    GLOBAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// C# `IDisposable` fixture: restores the previous ambient localizer.
struct Restore(Arc<Localizer>);

impl Restore {
    fn new() -> Self {
        Self(Localizer::current())
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        Localizer::set_current(Arc::clone(&self.0));
    }
}

#[test]
fn test_sample_english_returns_english_text() {
    // C#: GeneratedTrTests.TestSample_English_ReturnsEnglishText
    let _lock = serial();
    let _restore = Restore::new();
    Localizer::set_current(built_in_english_localizer());

    assert_eq!(tr::test_sample(), "Sample");
    assert_eq!(keys::TEST_SAMPLE, "test.sample");
}

#[test]
fn test_greeting_english_fills_placeholder() {
    // C#: GeneratedTrTests.TestGreeting_English_FillsPlaceholder
    let _lock = serial();
    let _restore = Restore::new();
    Localizer::set_current(built_in_english_localizer());

    assert_eq!(tr::test_greeting("An"), "Hello An");
    assert_eq!(tr::test_greeting(String::from("An")), "Hello An");
    assert_eq!(tr::test_greeting(LocValue::Str("An".to_owned())), "Hello An");
}

#[test]
fn test_items_english_picks_plural_form() {
    // C#: GeneratedTrTests.TestItems_English_PicksPluralForm
    let _lock = serial();
    let _restore = Restore::new();
    Localizer::set_current(built_in_english_localizer());

    for (count, expected) in [(1, "1 item"), (2, "2 items")] {
        assert_eq!(tr::test_items(count), expected);
    }
    assert_eq!(keys::TEST_ITEMS_ONE, "test.items.one");
    assert_eq!(keys::TEST_ITEMS_OTHER, "test.items.other");
}

#[test]
fn generated_members_vietnamese_return_translated_text() {
    // C#: GeneratedTrTests.GeneratedMembers_Vietnamese_ReturnTranslatedText
    let _lock = serial();
    let _restore = Restore::new();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("languages")
        .join("vi.json");
    let text = std::fs::read_to_string(&path).expect("assets/languages/vi.json");
    let mut warnings = Vec::new();
    let vi = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings).expect("valid catalog");
    assert!(warnings.is_empty(), "{warnings:?}");
    Localizer::set_current(Arc::new(Localizer::create(built_in_english(), &[vi], &[])));

    assert_eq!(tr::test_sample(), "Mẫu");
    assert_eq!(tr::test_greeting("An"), "Xin chào An");
    assert_eq!(tr::test_items(1), "1 mục");
    assert_eq!(tr::test_items(2), "2 mục");
}

#[test]
fn every_generated_const_names_a_key_of_the_built_in_catalog() {
    // Rust addition: `keys` and the English catalog come from the same file, so every const resolves.
    let english = built_in_english_localizer();

    for key in [
        keys::TEST_SAMPLE,
        keys::TEST_GREETING,
        keys::TEST_ITEMS_ONE,
        keys::TEST_ITEMS_OTHER,
    ] {
        assert!(english.contains(key), "{key}");
    }
}
