//! Port of `tests/PhotoReview.App.Tests/Localization/LocalizationServiceModeTests.cs` and of the service parts of
//! `LocalizationServiceTests.cs` (C# baseline d92d6aae; I18N L09 translator modes, the reload path, `Apply`).
//! Each test names its C# twin in a comment. Red tests (W8-04a): every call into `service` or `source` panics via
//! `todo!()` until the porter implements the bodies.
//!
//! Tests that publish the ambient localizer or the ambient UI culture take the process-wide `SERIAL` lock and
//! restore both on drop (the C# `[Collection("GlobalState")]` and `Dispose` pattern).
//!
//! DIVERGENCE: `LocalizationSource` `Item[]` change notification (C# `Switch_RaisesIndexerChangeOnLocalizationSource`,
//! first half) is not ported: WPF binding only. The shell subscribes to `on_current_changed` instead (D-W8-1). Only
//! the lookup half survives, as `source_text_follows_current_localizer`.
//! DIVERGENCE: C# null-argument tests (`ParseMode_NullArguments_Throw`, `Apply_NullLocalizer_Throws`,
//! `Constructor_NullPaths_Throws`) have no Rust counterpart: references cannot be null. Not ported.
//! DIVERGENCE: `ExportTodo_WritesTheTodoFileForTheResolvedLanguageIntoTheUserFolder` belongs to W8-04b (`export`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_app::localization::service::{CultureLookup, LocalizationPaths, LocalizationService, TranslatorMode};
use pr_app::localization::source;
use pr_core::abstractions::{FileSystem, Log};
use pr_l10n::loader::{AUTO_CODE, UiCulture, current_ui_culture, set_current_ui_culture};
use pr_l10n::localizer::Localizer;
use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog};

const SHIPPED: &str = r"C:\App\Languages";
const USER: &str = r"C:\Users\u\AppData\Local\PhotoReviewRs\Languages";
const VI_CATALOG: &str = include_str!("../../../assets/languages/vi.json");

/// Process-wide lock for tests that publish the ambient localizer or the ambient UI culture.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Restores the ambient localizer and UI culture when a test ends (also when it panics).
struct Restore {
    localizer: Arc<Localizer>,
    culture: UiCulture,
}

impl Restore {
    fn new() -> Self {
        Self {
            localizer: Localizer::current(),
            culture: current_ui_culture(),
        }
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        Localizer::set_current(Arc::clone(&self.localizer));
        set_current_ui_culture(self.culture.clone());
    }
}

/// C# `CultureInfo.GetCultureInfo` for the names the tests use. Only `en`, `vi` and `fr` are known; everything else
/// (including `community` and `not a culture!`) is `None`, the .NET `CultureNotFoundException` case.
struct KnownCultures;

impl CultureLookup for KnownCultures {
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        match name {
            "en" | "vi" | "fr" => Some(UiCulture::from_name(name)),
            _ => None,
        }
    }
}

fn paths() -> LocalizationPaths {
    LocalizationPaths {
        shipped_dir: PathBuf::from(SHIPPED),
        user_languages_dir: PathBuf::from(USER),
    }
}

/// A fake disk with the shipped Vietnamese catalog (the real `assets/languages/vi.json`) in the shipped folder.
fn fake_disk() -> Arc<FakeFileSystem> {
    let fs = Arc::new(FakeFileSystem::new());
    fs.add_dir(SHIPPED);
    fs.add_file(Path::new(SHIPPED).join("vi.json"), VI_CATALOG.as_bytes());
    fs.add_dir(USER);
    fs
}

fn service(disk: &Arc<FakeFileSystem>, log: Arc<dyn Log>) -> LocalizationService {
    let file_system: Arc<dyn FileSystem> = disk.clone();
    LocalizationService::new(
        file_system,
        paths(),
        UiCulture::from_name("en"),
        Arc::new(KnownCultures),
        log,
    )
}

fn write_user_file(disk: &FakeFileSystem, name: &str, content: &str) {
    disk.add_file(Path::new(USER).join(name), content.as_bytes());
}

fn user_file_path(name: &str) -> String {
    Path::new(USER).join(name).display().to_string()
}

fn warnings_of(log: &RecordingLog) -> Vec<String> {
    log.entries()
        .into_iter()
        .filter(|(level, _)| *level == LogLevel::Warn)
        .map(|(_, message)| message)
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Translator modes (LocalizationServiceModeTests)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn parse_mode_reads_the_translator_switches_from_the_command_line() {
    // C#: LocalizationServiceModeTests.ParseMode_ReadsTheTranslatorSwitchesFromTheCommandLine (7 rows)
    let rows: &[(TranslatorMode, &[&str])] = &[
        (TranslatorMode::None, &[]),
        (TranslatorMode::Keys, &["--i18n-keys"]),
        (TranslatorMode::Keys, &["--I18N-KEYS"]),
        (TranslatorMode::Pseudo, &["--i18n-pseudo"]),
        (TranslatorMode::Pseudo, &["--I18N-Pseudo"]),
        // keys wins when both are given
        (TranslatorMode::Keys, &["--i18n-pseudo", "--i18n-keys"]),
        // near misses are not the switches
        (TranslatorMode::None, &["--i18n", "i18n-keys", "pseudo"]),
        (TranslatorMode::Pseudo, &["C:\\photos", "--i18n-pseudo"]),
    ];
    for (expected, args) in rows {
        assert_eq!(LocalizationService::parse_mode(args), *expected, "args {args:?}");
    }
}

#[test]
fn parse_mode_elements_are_matched_whole() {
    // Rust-only (golden `parse-leading-space-not-a-switch`, `parse-suffix-not-a-switch`): only an exact element counts.
    assert_eq!(LocalizationService::parse_mode(&[" --i18n-keys"]), TranslatorMode::None);
    assert_eq!(
        LocalizationService::parse_mode(&["--i18n-keys-x"]),
        TranslatorMode::None
    );
}

#[test]
fn load_in_keys_mode_shows_each_texts_catalog_key() {
    // C#: LocalizationServiceModeTests.Load_InKeysMode_ShowsEachTextsCatalogKey
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.set_mode(TranslatorMode::Keys);

    let localizer = service.load(Some("en"));

    assert_eq!(localizer.get("app.title"), "[app.title]");
    assert_eq!(localizer.code(), "en"); // the language itself is unchanged
}

#[test]
fn mode_getter_returns_what_set_mode_stored() {
    // Repair round 1 (kills the `mode() -> Default::default()` mutant). No direct C# twin: `Mode` get/set property.
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    assert_eq!(service.mode(), TranslatorMode::None);
    service.set_mode(TranslatorMode::Pseudo);
    assert_eq!(service.mode(), TranslatorMode::Pseudo);
    service.set_mode(TranslatorMode::Keys);
    assert_eq!(service.mode(), TranslatorMode::Keys);
}

#[test]
fn debug_output_names_the_service_and_its_mode() {
    // Repair round 1 (kills the `Debug::fmt -> Ok(())` mutant): diagnostic output must not be empty.
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.set_mode(TranslatorMode::Keys);
    let text = format!("{service:?}");
    assert!(text.contains("LocalizationService"), "{text}");
    assert!(text.contains("Keys"), "{text}");
}

#[test]
fn load_in_pseudo_mode_accents_and_brackets_the_text_under_the_pseudo_code() {
    // C#: LocalizationServiceModeTests.Load_InPseudoMode_AccentsAndBracketsTheTextUnderThePseudoCode
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    let plain = service.load(Some("en")).get("app.title");
    service.set_mode(TranslatorMode::Pseudo);

    let pseudo = service.load(Some("en"));

    let text = pseudo.get("app.title");
    assert!(text.starts_with('['), "{text}");
    assert!(text.ends_with(']'), "{text}");
    assert_ne!(plain, text);
    assert!(
        text.chars().count() > plain.chars().count(),
        "pseudo text is padded to look longer than the original"
    );
    assert_ne!(pseudo.code(), "en");
}

#[test]
fn load_without_a_mode_returns_the_catalog_text_unchanged() {
    // C#: LocalizationServiceModeTests.Load_WithoutAMode_ReturnsTheCatalogTextUnchanged
    let disk = fake_disk();
    let service = service(&disk, Arc::new(RecordingLog::new()));

    let localizer = service.load(Some("en"));

    assert_eq!(localizer.code(), "en");
    assert_ne!(localizer.get("app.title"), "[app.title]");
    assert!(!localizer.get("app.title").contains('['));
}

#[test]
fn apply_in_pseudo_mode_keeps_the_english_ui_culture_whatever_the_localizers_code() {
    // C#: LocalizationServiceModeTests.Apply_InPseudoMode_KeepsTheEnglishUiCultureWhateverTheLocalizersCode
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.set_mode(TranslatorMode::Pseudo);
    set_current_ui_culture(UiCulture::from_name("fr"));

    service.switch(Some("vi"));

    assert_eq!(current_ui_culture().name, "en");
    assert_eq!(service.requested_language(), "vi");
}

#[test]
fn apply_a_language_code_dotnet_does_not_know_still_publishes_the_text_and_keeps_the_culture() {
    // C#: LocalizationServiceModeTests.Apply_ALanguageCodeDotNetDoesNotKnow_StillPublishesTheTextAndKeepsTheCulture
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    write_user_file(
        &disk,
        "community.json",
        r#"{ "_meta": { "code": "community", "name": "Community", "nativeName": "Community" } }"#,
    );
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.switch(Some("en"));
    set_current_ui_culture(UiCulture::from_name("fr"));
    let before = current_ui_culture();
    let unknown = Arc::new(service.load(Some("community")));

    service.apply(Some("community"), Arc::clone(&unknown));

    assert!(Arc::ptr_eq(&Localizer::current(), &unknown));
    assert_eq!(current_ui_culture().name, before.name);
    assert_eq!(service.requested_language(), "community");
}

#[test]
fn apply_a_blank_language_is_recorded_as_auto() {
    // C#: LocalizationServiceModeTests.Apply_ABlankLanguage_IsRecordedAsAuto (3 rows)
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));

    for language in [None, Some(""), Some("   ")] {
        service.switch(Some("vi"));
        service.apply(language, Arc::new(service.load(Some("en"))));

        assert_eq!(service.requested_language(), AUTO_CODE, "language {language:?}");
    }
}

#[test]
fn apply_logs_every_load_warning_with_the_i18n_prefix() {
    // C#: LocalizationServiceModeTests.Apply_LogsEveryLoadWarningWithTheI18nPrefix
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let log = Arc::new(RecordingLog::new());
    let mut service = service(&disk, log.clone());
    write_user_file(
        &disk,
        "vi.json",
        r#"{ "_meta": { "code": "vi" }, "app.title": "Duyệt ảnh" "#,
    ); // missing '}'

    service.switch(Some("vi"));

    let broken = user_file_path("vi.json");
    let matching: Vec<String> = warnings_of(&log).into_iter().filter(|w| w.contains(&broken)).collect();
    assert_eq!(matching.len(), 1, "one warning names the broken file: {matching:?}");
    assert!(matching[0].starts_with("i18n: "), "{}", matching[0]);
}

#[test]
fn resolve_code_follows_the_setting_and_falls_back_to_english_for_unknown_codes() {
    // C#: LocalizationServiceModeTests.ResolveCode_FollowsTheSettingAndFallsBackToEnglishForUnknownCodes
    let disk = fake_disk();
    let service = service(&disk, Arc::new(RecordingLog::new()));

    assert_eq!(service.resolve_code(Some("vi")), "vi");
    assert_eq!(service.resolve_code(Some("en")), "en");
    assert_eq!(service.resolve_code(Some("zz-not-shipped")), "en");
}

// ---------------------------------------------------------------------------------------------------------------
// Service (LocalizationServiceTests)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn switch_to_english_publishes_localizer_and_ui_culture() {
    // C#: LocalizationServiceTests.Switch_ToEnglish_PublishesLocalizerAndUiCulture
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));

    service.switch(Some("en"));

    assert_eq!(Localizer::current().code(), "en");
    assert_eq!(current_ui_culture().name, "en");
    assert_eq!(service.requested_language(), "en");
}

#[test]
fn switch_to_vietnamese_uses_shipped_catalog() {
    // C#: LocalizationServiceTests.Switch_ToVietnamese_UsesShippedCatalog
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));

    service.switch(Some("vi"));

    assert_eq!(Localizer::current().code(), "vi");
    assert_eq!(Localizer::current().native_name(), "Tiếng Việt");
}

#[test]
fn source_text_follows_current_localizer() {
    // C#: LocalizationServiceTests.Switch_RaisesIndexerChangeOnLocalizationSource, lookup half (the Item[] half is a
    // DIVERGENCE, see the module doc). `source::text` reads the ambient localizer, so a switch changes its answer.
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));

    service.switch(Some("en"));
    assert_eq!(source::text("app.title"), Localizer::current().get("app.title"));
    assert_eq!(source::text("app.title"), "Photo Review");

    service.set_mode(TranslatorMode::Keys);
    service.switch(Some("en"));
    assert_eq!(source::text("app.title"), "[app.title]");
}

#[test]
fn source_text_unknown_key_is_key() {
    // Rust-only (golden `source-unknown-key-is-the-key`): an unknown key comes back unchanged.
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.switch(Some("en"));

    assert_eq!(source::text("no.such.key"), "no.such.key");
}

#[test]
fn reload_picks_up_edited_user_file() {
    // C#: LocalizationServiceTests.Reload_PicksUpEditedUserFile
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.switch(Some("vi"));
    write_user_file(
        &disk,
        "vi.json",
        r#"{ "_meta": { "code": "vi" }, "app.title": "Duyệt ảnh" }"#,
    );

    let problems = service.reload();

    assert_eq!(Localizer::current().get("app.title"), "Duyệt ảnh");
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn reload_broken_user_file_returns_the_problem_and_keeps_shipped_text() {
    // C#: LocalizationServiceTests.Reload_BrokenUserFile_ReturnsTheProblem_AndKeepsShippedText
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.switch(Some("vi"));
    let shipped_title = Localizer::current().get("app.title");
    write_user_file(
        &disk,
        "vi.json",
        r#"{ "_meta": { "code": "vi" }, "app.title": "Duyệt ảnh" "#,
    ); // missing '}'

    let problems = service.reload();

    let broken = user_file_path("vi.json");
    assert!(
        problems
            .iter()
            .any(|p| p.contains(&broken) && p.contains("invalid JSON")),
        "problems: {problems:?}"
    );
    assert_eq!(
        Localizer::current().get("app.title"),
        shipped_title,
        "the whole file is skipped"
    );
}

#[test]
fn reload_bad_placeholder_returns_the_problem_for_that_key() {
    // C#: LocalizationServiceTests.Reload_BadPlaceholder_ReturnsTheProblemForThatKey
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.switch(Some("vi"));
    write_user_file(
        &disk,
        "vi.json",
        r#"{ "_meta": { "code": "vi" }, "main.skipped.warning.other": "Bỏ qua {soLuong} tệp", "app.title": "Duyệt ảnh" }"#,
    );

    let problems = service.reload();

    assert!(
        problems.iter().any(|p| p.contains("'main.skipped.warning.other'")),
        "problems: {problems:?}"
    );
    assert_eq!(
        Localizer::current().get("app.title"),
        "Duyệt ảnh",
        "the other entries still load"
    );
}

#[test]
fn discover_languages_lists_english_first_and_shipped_vietnamese() {
    // C#: LocalizationServiceTests.DiscoverLanguages_ListsEnglishFirstAndShippedVietnamese
    let disk = fake_disk();
    let languages = service(&disk, Arc::new(RecordingLog::new())).discover_languages();

    assert_eq!(languages[0].code, "en");
    assert!(languages.iter().any(|l| l.code == "vi" && !l.is_user_provided));
}

// ---------------------------------------------------------------------------------------------------------------
// Rust-only contract tests (W8-04a test plan)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn apply_keeps_same_arc() {
    // Rust-only (contract trap T12): `apply` publishes the SAME `Arc` it was given.
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    let localizer = Arc::new(service.load(Some("vi")));

    service.apply(Some("vi"), Arc::clone(&localizer));

    assert!(Arc::ptr_eq(&Localizer::current(), &localizer));
}

#[test]
fn apply_keys_uses_localizer_code_culture() {
    // Rust-only (contract trap T7): `Keys` mode keeps the localizer's own code for the UI culture (not `en`).
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.set_mode(TranslatorMode::Keys);
    set_current_ui_culture(UiCulture::from_name("fr"));

    service.switch(Some("vi"));

    assert_eq!(Localizer::current().code(), "vi");
    assert_eq!(current_ui_culture().name, "vi");
}

#[test]
fn load_never_publishes() {
    // Rust-only (contract `load`, semantics 9): `load` returns the localizer and does not publish it.
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let service = service(&disk, Arc::new(RecordingLog::new()));
    let before = Localizer::current();

    let _loaded = service.load(Some("vi"));

    assert!(Arc::ptr_eq(&Localizer::current(), &before));
}

#[test]
fn reload_uses_requested_language() {
    // Rust-only (contract semantics 13): `reload` uses the REQUESTED language, never a value the caller passes.
    let _guard = serial();
    let _restore = Restore::new();
    let disk = fake_disk();
    let mut service = service(&disk, Arc::new(RecordingLog::new()));
    service.switch(Some("en"));
    write_user_file(
        &disk,
        "vi.json",
        r#"{ "_meta": { "code": "vi" }, "app.title": "Duyệt ảnh" }"#,
    );

    service.reload();

    assert_eq!(service.requested_language(), "en");
    assert_eq!(Localizer::current().get("app.title"), "Photo Review");
}
