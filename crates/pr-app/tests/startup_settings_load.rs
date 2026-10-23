//! W8-17d settings/language sequencing, using only fake disk data under a TempRoot.
use pr_app::localization::service::{CultureLookup, LocalizationPaths, LocalizationService};
use pr_app::startup::settings_load::*;
use pr_core::abstractions::NullLog;
use pr_core::app_paths::AppPaths;
use pr_core::diagnostics::diag_options::DiagOptions;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::store::SettingsStore;
use pr_l10n::loader::UiCulture;
use pr_test_support::{FakeClock, FakeFileSystem, TempRoot};
use std::sync::{Arc, Mutex};

struct Cultures;
impl CultureLookup for Cultures {
    fn lookup(&self, name: &str) -> Option<UiCulture> {
        Some(UiCulture::from_name(name))
    }
}

#[test]
fn startup_core_settings_file_hook_and_language_marks_are_ordered() {
    // C#: ShellStartupTests.Startup_EarlyDecode_StartsOnlyAfterTheInstanceLockIsHeld (preparation half).
    let root = TempRoot::new("startup-settings");
    let disk = Arc::new(FakeFileSystem::new());
    let paths = AppPaths::new(root.path(), Some(&root.path().to_string_lossy()), true);
    let mut settings = AppSettings::new(&EnglishSettingsTexts);
    settings.ui_language = "en".into();
    settings.logging_enabled = true;
    disk.add_file(&paths.config_file, settings.to_json().unwrap().as_bytes());
    let store = SettingsStore::new(
        paths,
        disk.clone(),
        Arc::new(NullLog),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishSettingsTexts),
    );
    let localization = Arc::new(Mutex::new(LocalizationService::new(
        disk,
        LocalizationPaths {
            shipped_dir: root.join("shipped"),
            user_languages_dir: root.join("languages"),
        },
        UiCulture::from_name("en"),
        Arc::new(Cultures),
        Arc::new(NullLog),
    )));
    let events = Mutex::new(Vec::new());
    let loaded = load_settings_and_language(
        &store,
        &localization,
        &|name| {
            assert!(localization.try_lock().is_ok());
            events.lock().unwrap().push(name.to_owned());
        },
        &|s| {
            assert!(s.logging_enabled);
            events.lock().unwrap().push("hook".into());
        },
    )
    .unwrap();
    assert_eq!(loaded.settings.ui_language, "en");
    assert!(Arc::ptr_eq(&loaded.settings, &store.current()));
    assert_eq!(
        *events.lock().unwrap(),
        ["settingsFileLoaded", "hook", "languageLoaded"]
    );
    assert_eq!(
        repair_notice(&store, &LocalizerSettingsTexts::new(loaded.localizer)),
        None
    );
}

#[test]
fn startup_core_settings_changed_toggles_logging() {
    // C#: AppStartupBranchTests.StartupCore_SettingsChanged_TogglesLogging.
    let mut s = AppSettings::new(&EnglishSettingsTexts);
    for enabled in [false, true] {
        s.logging_enabled = enabled;
        assert_eq!(logging_enabled(&s, &DiagOptions::default()), enabled);
        assert!(logging_enabled(
            &s,
            &DiagOptions {
                force_log: true,
                ..DiagOptions::default()
            }
        ));
    }
}

#[test]
fn startup_core_logging_enabled_in_settings_logs_the_raw_arguments() {
    // C#: AppStartupBranchTests.StartupCore_LoggingEnabledInSettings_LogsTheArguments.
    let args = vec!["--i18n-keys".into(), "raw path.jpg".into()];
    assert_eq!(
        startup_log_lines(&args, &DiagOptions::default()),
        ("Startup (shell) args=--i18n-keys | raw path.jpg".into(), None)
    );
    let diag = DiagOptions {
        force_log: true,
        instance_label: Some(" B ".into()),
        ..DiagOptions::default()
    };
    assert_eq!(
        startup_log_lines(&[], &diag),
        (
            "Startup (shell) args=".into(),
            Some(format!("DIAG MODE: {}", diag.describe()))
        )
    );
}

fn fixture() -> (
    TempRoot,
    Arc<FakeFileSystem>,
    SettingsStore,
    pr_app::presenters::settings::load::SharedLocalization,
) {
    let root = TempRoot::new("startup-settings-more");
    let fs = Arc::new(FakeFileSystem::new());
    let store = SettingsStore::new(
        AppPaths::new(root.path(), Some(&root.path().to_string_lossy()), true),
        fs.clone(),
        Arc::new(NullLog),
        Arc::new(FakeClock::new()),
        Arc::new(EnglishSettingsTexts),
    );
    let localization = Arc::new(Mutex::new(LocalizationService::new(
        fs.clone(),
        LocalizationPaths {
            shipped_dir: root.join("shipped"),
            user_languages_dir: root.join("languages"),
        },
        UiCulture::from_name("en"),
        Arc::new(Cultures),
        Arc::new(NullLog),
    )));
    (root, fs, store, localization)
}

#[test]
fn startup_core_repaired_settings_are_reported_once_through_the_notice_text() {
    // C#: AppStartupBranchTests.StartupCore_RepairedSettings_AreReportedOnceThroughTheDialogService (notice half; dialog is 17e).
    let (root, fs, store, localization) = fixture();
    let mut json: serde_json::Value =
        serde_json::from_str(&AppSettings::new(&EnglishSettingsTexts).to_json().unwrap()).unwrap();
    json["ImageCacheRamPercent"] = serde_json::json!(999);
    fs.add_file(
        root.join("config.json"),
        serde_json::to_string(&json).unwrap().as_bytes(),
    );
    let loaded = load_settings_and_language(&store, &localization, &|_| {}, &|_| {}).unwrap();
    assert!(!store.last_load_repairs().is_empty());
    let texts = LocalizerSettingsTexts::new(loaded.localizer);
    assert_eq!(
        repair_notice(&store, &texts),
        Some(pr_core::settings::normalizer::build_repair_text(
            &store.last_load_repairs(),
            false,
            &texts
        ))
    );
}

#[test]
fn load_error_stops_before_marks_and_early_decode_hook() {
    // New: SettingsStore Changed handler errors map to StartupLoadError::Settings.
    let (_, _, store, localization) = fixture();
    store.subscribe_changed(Box::new(|_| Err(std::io::Error::other("changed failed"))));
    let result = load_settings_and_language(
        &store,
        &localization,
        &|_| panic!("load failed before any mark"),
        &|_| panic!("load failed before early decode"),
    );
    assert!(matches!(result, Err(StartupLoadError::Settings(e)) if e.to_string() == "changed failed"));
}

#[test]
fn startup_core_ui_language_is_published_before_the_loaded_mark() {
    // C#: AppStartupBranchTests.StartupCore_UiLanguageSetting_IsPublishedToTheLocalizationService.
    struct Restore(Arc<pr_l10n::localizer::Localizer>, UiCulture);
    impl Drop for Restore {
        fn drop(&mut self) {
            pr_l10n::localizer::Localizer::set_current(self.0.clone());
            pr_l10n::loader::set_current_ui_culture(self.1.clone());
        }
    }
    let _restore = Restore(
        pr_l10n::localizer::Localizer::current(),
        pr_l10n::loader::current_ui_culture(),
    );
    let (_root, _, store, localization) = fixture();
    let loaded = load_settings_and_language(&store, &localization, &|_| {}, &|_| {}).unwrap();
    let mut marks = Vec::new();
    let mark = Mutex::new(&mut marks);
    apply_loaded_settings(&localization, &loaded, &|name| {
        assert!(Arc::ptr_eq(
            &pr_l10n::localizer::Localizer::current(),
            &loaded.localizer
        ));
        assert!(
            localization.try_lock().is_ok(),
            "mark after releasing localization lock"
        );
        mark.lock().unwrap().push(name.to_owned());
    });
    assert_eq!(marks, vec!["settingsLoaded"]);
}

#[test]
fn startup_context_preserves_launch_and_settings_with_an_empty_placement_handle() {
    // C#: ShellStartupContext (C-16); Rust single-use task conversion.
    let root = TempRoot::new("startup-context");
    let fs = FakeFileSystem::new();
    let path = root.join("photo.jpg");
    fs.add_file(&path, b"fake");
    let launch = pr_app::startup::launch::ShellLaunch::parse(&[path.to_string_lossy()], &fs, &NullLog);
    let start = std::time::Instant::now();
    let settings = Arc::new(AppSettings::new(&EnglishSettingsTexts));
    let context = build_startup_context(&launch, &settings, None, start);
    assert_eq!(context.arguments, launch.arguments);
    assert_eq!(context.initial_path.as_deref(), launch.initial_path());
    assert_eq!(context.launch_folder, launch.launch_folder);
    assert_eq!(context.process_start, start);
    assert!(context.placement.join().is_none());
    assert_eq!(
        context.settings.join().unwrap().to_json().unwrap(),
        settings.to_json().unwrap()
    );
}
