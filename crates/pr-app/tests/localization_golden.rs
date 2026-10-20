//! Golden-driven tests for W8-04a (oracle commands `language-options`, `translation-problems`, `localization-service`,
//! C# baseline d92d6aae). Each case of `tests/golden/<name>.json` is replayed against the Rust port; the golden files
//! are written by the oracle and never edited here. Red tests: the port panics via `todo!()` until implemented.
//!
//! Setup mirrors the oracle run that produced the goldens (see `tests/golden-inputs/*.json`): the shipped folder
//! holds no catalog (so `vi` resolves to English in these goldens), the user folder is `<ROOT>\Languages`, and
//! a fresh fake disk is used per case. `<ROOT>` is masked in every compared text, and the exception text of
//! unreadable or broken files is compared only up to the marker (`invalid JSON (*)`), because .NET and Rust
//! exception texts differ.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_app::localization::language_options::{LanguageOption, build, index_of, to_setting};
use pr_app::localization::service::{CultureLookup, LocalizationPaths, LocalizationService, TranslatorMode};
use pr_app::localization::source;
use pr_app::localization::translation_problems::{MAX_LISTED, has_problems, message};
use pr_core::abstractions::{FileSystem, Log};
use pr_l10n::loader::{LanguageInfo, LanguageLoader, UiCulture, current_ui_culture, set_current_ui_culture};
use pr_l10n::localizer::Localizer;
use pr_test_support::{FakeFileSystem, LogLevel, RecordingLog, golden};
use serde_json::Value;

const ROOT: &str = r"C:\GoldenRoot";
const SHIPPED: &str = r"C:\GoldenRoot\App\Languages";
const USER: &str = r"C:\GoldenRoot\Languages";
/// Markers of load problems whose exception text differs between .NET and Rust (compared up to the marker).
const MASK_MARKERS: [&str; 4] = ["invalid JSON", "unreadable text", "cannot read", "cannot list files"];

/// Process-wide lock for the cases that publish the ambient localizer or the ambient UI culture.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Restores the ambient localizer and UI culture when a case ends (also when it panics).
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

/// The culture names the goldens use: `en`, `vi`, `fr` are known to .NET; the rest is `None`.
struct KnownCultures;

impl CultureLookup for KnownCultures {
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        match name {
            "en" | "vi" | "fr" => Some(UiCulture::from_name(name)),
            _ => None,
        }
    }
}

/// A fresh fake disk: the shipped and user folders exist, and no catalog file is in either (oracle setup).
fn golden_disk() -> Arc<FakeFileSystem> {
    let fs = Arc::new(FakeFileSystem::new());
    fs.add_dir(SHIPPED);
    fs.add_dir(USER);
    fs
}

/// The localizer for `code` as the oracle built it: an unknown catalog (`vi` here) falls back to English.
fn golden_localizer(code: &str) -> Localizer {
    let disk = golden_disk();
    let file_system: Arc<dyn FileSystem> = disk;
    LanguageLoader::new(file_system, SHIPPED, Some(PathBuf::from(USER)))
        .load(Some(code), Some(&UiCulture::from_name(code)))
}

fn golden_service(disk: &Arc<FakeFileSystem>, log: Arc<dyn Log>) -> LocalizationService {
    let file_system: Arc<dyn FileSystem> = disk.clone();
    let paths = LocalizationPaths {
        shipped_dir: PathBuf::from(SHIPPED),
        user_languages_dir: PathBuf::from(USER),
    };
    LocalizationService::new(
        file_system,
        paths,
        UiCulture::from_name("en"),
        Arc::new(KnownCultures),
        log,
    )
}

/// Masks the temp root and the exception text of load problems (see the module doc).
fn mask(text: &str) -> String {
    let rooted = text.replace(ROOT, "<ROOT>");
    for marker in MASK_MARKERS {
        if let Some(position) = rooted.find(marker) {
            let end = position + marker.len();
            return format!("{} (*)", &rooted[..end]);
        }
    }
    rooted
}

fn masked_all(texts: &[String]) -> Vec<String> {
    texts.iter().map(|t| mask(t)).collect()
}

fn str_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected an array: {value}"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("expected a string: {v}"))
                .to_owned()
        })
        .collect()
}

fn warnings_of(log: &RecordingLog) -> Vec<String> {
    log.entries()
        .into_iter()
        .filter(|(level, _)| *level == LogLevel::Warn)
        .map(|(_, message)| message)
        .collect()
}

fn finish(name: &str, failures: &[String], cases: usize) {
    assert!(
        failures.is_empty(),
        "{name}: {} of {cases} cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------
// language-options (I18N L08)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn golden_language_options() {
    let golden = golden::load("language-options");
    assert_eq!(golden.cases.len(), 7, "case count of the golden file");
    let mut failures = Vec::new();

    for case in &golden.cases {
        let id = golden::str_of(case, "id");
        let text = golden_localizer(golden::str_of(case, "localizer"));
        let languages: Vec<LanguageInfo> = case["languages"]
            .as_array()
            .expect("languages")
            .iter()
            .map(|l| LanguageInfo {
                code: golden::str_of(l, "code").to_owned(),
                name: golden::str_of(l, "name").to_owned(),
                native_name: golden::str_of(l, "nativeName").to_owned(),
                is_user_provided: golden::bool_of(l, "isUserProvided"),
            })
            .collect();
        let options = build(&languages, &text);

        // build: code, display name and isAuto of each entry
        let expected_build = case["build"].as_array().expect("build");
        let actual_build: Vec<(String, String, bool)> = options
            .iter()
            .map(|o| (o.code.clone(), o.display_name.clone(), o.is_auto()))
            .collect();
        let want_build: Vec<(String, String, bool)> = expected_build
            .iter()
            .map(|e| {
                (
                    golden::str_of(e, "code").to_owned(),
                    golden::str_of(e, "displayName").to_owned(),
                    golden::bool_of(e, "isAuto"),
                )
            })
            .collect();
        if actual_build != want_build {
            failures.push(format!("{id}: build {actual_build:?} != {want_build:?}"));
        }

        // index_of: a null setting is None; -1 in the golden is None
        for row in case["index_of"].as_array().expect("index_of") {
            let setting = row["setting"].as_str();
            let want = row["index"].as_i64().expect("index");
            let got = index_of(&options, setting).map_or(-1, |i| i64::try_from(i).expect("small index"));
            if got != want {
                failures.push(format!("{id}: index_of({setting:?}) = {got}, want {want}"));
            }
        }

        // to_setting: selected is the entry index or null
        for row in case["to_setting"].as_array().expect("to_setting") {
            let selected: Option<&LanguageOption> = row["selected"]
                .as_u64()
                .map(|i| &options[usize::try_from(i).expect("small index")]);
            let current = row["current"].as_str();
            let want = golden::str_of(row, "out");
            let got = to_setting(selected, current);
            if got != want {
                failures.push(format!(
                    "{id}: to_setting({:?}, {current:?}) = {got:?}, want {want:?}",
                    row["selected"]
                ));
            }
        }
    }
    finish("language-options", &failures, golden.cases.len());
}

// ---------------------------------------------------------------------------------------------------------------
// translation-problems (I18N, "Reload translations")
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn golden_translation_problems() {
    let golden = golden::load("translation-problems");
    assert_eq!(golden.cases.len(), 12, "case count of the golden file");
    let mut failures = Vec::new();

    for case in &golden.cases {
        let id = golden::str_of(case, "id");
        let text = golden_localizer(golden::str_of(case, "localizer"));
        let warnings: Vec<String> = match case.get("repeat") {
            Some(repeat) => {
                let prefix = golden::str_of(repeat, "prefix");
                let count = golden::i64_of(repeat, "count");
                (1..=count).map(|i| format!("{prefix}{i:02}")).collect()
            }
            None => str_list(&case["warnings"]),
        };

        let want_max = golden::i64_of(case, "max_listed");
        if i64::try_from(MAX_LISTED).expect("small constant") != want_max {
            failures.push(format!("{id}: MAX_LISTED {MAX_LISTED} != {want_max}"));
        }
        let want_has = golden::bool_of(case, "has_problems");
        if has_problems(&warnings) != want_has {
            failures.push(format!("{id}: has_problems != {want_has}"));
        }
        let want = golden::str_of(case, "message");
        let got = message(&warnings, &text);
        if got != want {
            failures.push(format!("{id}: message\n  got  {got:?}\n  want {want:?}"));
        }
    }
    finish("translation-problems", &failures, golden.cases.len());
}

// ---------------------------------------------------------------------------------------------------------------------
// localization-service (I18N L09, Apply, Reload, ResolveCode, source)
// ---------------------------------------------------------------------------------------------------------------------

fn translator_mode(name: &str) -> TranslatorMode {
    match name {
        "None" => TranslatorMode::None,
        "Pseudo" => TranslatorMode::Pseudo,
        "Keys" => TranslatorMode::Keys,
        other => panic!("unknown mode in golden: {other}"),
    }
}

#[test]
fn golden_localization_service() {
    let golden = golden::load("localization-service");
    assert_eq!(golden.cases.len(), 35, "case count of the golden file");
    let mut failures = Vec::new();

    for case in &golden.cases {
        let id = golden::str_of(case, "id");
        let kind = golden::str_of(case, "kind");
        match kind {
            "parse_mode" => {
                let args = str_list(&case["args"]);
                let got = format!("{:?}", LocalizationService::parse_mode(&args));
                let want = golden::str_of(case, "out");
                if got != want {
                    failures.push(format!("{id}: parse_mode = {got}, want {want}"));
                }
            }
            "load" => {
                let disk = golden_disk();
                let mut service = golden_service(&disk, Arc::new(RecordingLog::new()));
                service.set_mode(translator_mode(golden::str_of(case, "mode")));
                let localizer = service.load(Some(golden::str_of(case, "language")));
                let want = &case["out"];
                let got_code = localizer.code();
                let got_title = localizer.get("app.title");
                if got_code != golden::str_of(want, "code") || got_title != golden::str_of(want, "appTitle") {
                    failures.push(format!(
                        "{id}: load = ({got_code:?}, {got_title:?}), want ({:?}, {:?})",
                        golden::str_of(want, "code"),
                        golden::str_of(want, "appTitle")
                    ));
                }
            }
            "apply_culture" => {
                let _guard = serial();
                let _restore = Restore::new();
                let disk = golden_disk();
                let mut service = golden_service(&disk, Arc::new(RecordingLog::new()));
                set_current_ui_culture(UiCulture::from_name(golden::str_of(case, "start_culture")));
                service.set_mode(translator_mode(golden::str_of(case, "mode")));
                let localizer = service.load(Some(golden::str_of(case, "load_language")));
                let load_warnings = masked_all(localizer.warnings());
                service.apply(Some(golden::str_of(case, "language")), Arc::new(localizer));

                let want = &case["out"];
                let got = (
                    service.requested_language().to_owned(),
                    Localizer::current().code().to_owned(),
                    current_ui_culture().name,
                    load_warnings,
                );
                let want_tuple = (
                    golden::str_of(want, "requestedLanguage").to_owned(),
                    golden::str_of(want, "localizerCode").to_owned(),
                    golden::str_of(want, "uiCulture").to_owned(),
                    str_list(&want["warnings"]),
                );
                if got != want_tuple {
                    failures.push(format!("{id}: apply = {got:?}, want {want_tuple:?}"));
                }
            }
            "reload_problems" => {
                let _guard = serial();
                let _restore = Restore::new();
                let disk = golden_disk();
                let log = Arc::new(RecordingLog::new());
                let mut service = golden_service(&disk, log.clone());
                // oracle order: switch first (no user file yet), then write the user files, then reload
                service.switch(Some(golden::str_of(case, "language")));
                for (name, content) in case["user_files"].as_object().expect("user_files") {
                    disk.add_file(
                        Path::new(USER).join(name),
                        content.as_str().expect("file text").as_bytes(),
                    );
                }
                let problems = service.reload();

                let want = &case["out"];
                let got = (
                    masked_all(&problems),
                    Localizer::current().get("app.title"),
                    service.requested_language().to_owned(),
                    masked_all(&warnings_of(&log)),
                );
                let want_tuple = (
                    str_list(&want["problems"]),
                    golden::str_of(want, "appTitle").to_owned(),
                    golden::str_of(want, "requestedLanguage").to_owned(),
                    str_list(&want["logWarnings"]),
                );
                if got != want_tuple {
                    failures.push(format!("{id}: reload = {got:?}, want {want_tuple:?}"));
                }
            }
            "resolve_code" => {
                let disk = golden_disk();
                let service = golden_service(&disk, Arc::new(RecordingLog::new()));
                let got = service.resolve_code(Some(golden::str_of(case, "language")));
                let want = golden::str_of(case, "out");
                if got != want {
                    failures.push(format!("{id}: resolve_code = {got:?}, want {want:?}"));
                }
            }
            "source_text" => {
                let _guard = serial();
                let _restore = Restore::new();
                Localizer::set_current(Arc::new(golden_localizer(golden::str_of(case, "localizer"))));
                let got = source::text(golden::str_of(case, "key"));
                let want = golden::str_of(case, "out");
                if got != want {
                    failures.push(format!("{id}: source::text = {got:?}, want {want:?}"));
                }
            }
            other => failures.push(format!("{id}: unknown kind {other}")),
        }
    }
    finish("localization-service", &failures, golden.cases.len());
}
