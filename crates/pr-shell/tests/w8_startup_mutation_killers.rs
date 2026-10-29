//! Startup/composition mutation regressions. Fakes only; no ambient clock or user data.
#![cfg(windows)]

use pr_shell::startup::error::StartupError;

#[test]
fn localized_settings_texts_match_the_english_oracle_for_every_member() {
    // Mutants: crates/pr-app/src/startup/settings_load.rs:138,141,144,148,151,155,161,165,169;
    // replace each SettingsTexts method with String::new() or "xyzzy".into().
    // C#: ShellStartup uses the Tr-backed SettingsTexts; EnglishSettingsTexts is the independent English oracle.
    use pr_core::settings::app_settings::{EnglishSettingsTexts, SettingsTexts};
    let texts =
        pr_app::startup::settings_load::LocalizerSettingsTexts::new(pr_l10n::localizer::built_in_english_localizer());
    let expected = EnglishSettingsTexts;
    for (member, actual, oracle) in [
        (
            "action_default_name",
            texts.action_default_name(17),
            expected.action_default_name(17),
        ),
        (
            "action_default_backup",
            texts.action_default_backup(),
            expected.action_default_backup(),
        ),
        (
            "core_settings_shortcut_invalid",
            texts.core_settings_shortcut_invalid("Pick photo"),
            expected.core_settings_shortcut_invalid("Pick photo"),
        ),
        (
            "core_settings_action_invalid",
            texts.core_settings_action_invalid(),
            expected.core_settings_action_invalid(),
        ),
        (
            "core_settings_action_binding_name",
            texts.core_settings_action_binding_name("Pick photo"),
            expected.core_settings_action_binding_name("Pick photo"),
        ),
        (
            "core_settings_shortcut_duplicate",
            texts.core_settings_shortcut_duplicate("Ctrl+P", "Pick photo, Previous"),
            expected.core_settings_shortcut_duplicate("Ctrl+P", "Pick photo, Previous"),
        ),
        (
            "settings_load_repaired",
            texts.settings_load_repaired("ImageCacheRamPercent"),
            expected.settings_load_repaired("ImageCacheRamPercent"),
        ),
        (
            "dialog_import_settings_repaired",
            texts.dialog_import_settings_repaired("ImageCacheRamPercent"),
            expected.dialog_import_settings_repaired("ImageCacheRamPercent"),
        ),
        (
            "settings_load_shortcut_disabled",
            texts.settings_load_shortcut_disabled("Pick photo"),
            expected.settings_load_shortcut_disabled("Pick photo"),
        ),
    ] {
        assert_eq!(actual, oracle, "{member}");
    }
}

#[test]
fn settings_texts_debug_names_the_adapter_without_dumping_the_catalog() {
    // Mutant: crates/pr-app/src/startup/settings_load.rs:125, replace Debug::fmt with Ok(Default::default()).
    // Rust diagnostic; no C# Debug counterpart.
    let texts =
        pr_app::startup::settings_load::LocalizerSettingsTexts::new(pr_l10n::localizer::built_in_english_localizer());
    assert_eq!(format!("{texts:?}"), "LocalizerSettingsTexts { .. }");
}

#[test]
fn settings_load_failure_display_preserves_both_io_and_worker_messages() {
    // Mutant: crates/pr-app/src/startup/settings_load.rs:52, replace Display::fmt with Ok(Default::default()).
    // C#: ShellStartup.RunStartupGuardedAsync -> FailStartup(ex.Message).
    use pr_app::startup::settings_load::StartupLoadError;
    for (error, expected) in [
        (
            StartupLoadError::Settings(std::io::Error::other("fake settings read failed")),
            "fake settings read failed",
        ),
        (
            StartupLoadError::Worker("fake settings worker failed".into()),
            "fake settings worker failed",
        ),
    ] {
        assert_eq!(error.to_string(), expected);
        assert_eq!(StartupError::from(error).to_string(), expected);
    }
}

#[test]
fn loaded_settings_debug_names_the_result_without_dumping_user_settings() {
    // Mutant: crates/pr-app/src/startup/settings_load.rs:36, replace Debug::fmt with Ok(Default::default()).
    // Rust diagnostic of the C# settings/localizer result; no C# Debug counterpart.
    let loaded = pr_app::startup::settings_load::LoadedSettings {
        settings: std::sync::Arc::new(pr_core::settings::app_settings::AppSettings::new(
            &pr_core::settings::app_settings::EnglishSettingsTexts,
        )),
        localizer: pr_l10n::localizer::built_in_english_localizer(),
    };
    assert_eq!(format!("{loaded:?}"), "LoadedSettings { .. }");
}

#[derive(Debug)]
struct UnusedPreviews;

impl pr_app::services::prewarm::PrewarmPreviews for UnusedPreviews {
    fn current_cache_key(
        &self,
        _: &str,
        _: Option<pr_imaging::decode_types::DecodeBox>,
    ) -> Result<pr_imaging::cache_key::ImageCacheKey, pr_imaging::cache_key::CacheKeyError> {
        panic!("formatting a decode plan must not read a file")
    }

    fn start_viewer_preview(
        &self,
        _: &str,
        _: &pr_imaging::cache_key::ImageCacheKey,
    ) -> pr_imaging::service::handle::PreviewHandle {
        panic!("formatting a decode plan must not start decoding")
    }
}

#[test]
fn early_decode_plan_debug_reports_the_predicted_box_without_resolving_previews() {
    // Mutant: crates/pr-app/src/startup/early_decode.rs:35, replace Debug::fmt with Ok(Default::default()).
    // Rust diagnostic of the C# EarlyDecodePlan; no C# Debug counterpart.
    let plan = pr_app::startup::early_decode::EarlyDecodePlan {
        previews: std::sync::Arc::new(UnusedPreviews),
        decode_box: pr_imaging::decode_types::DecodeBox {
            width: 640,
            height: 480,
        },
    };
    assert_eq!(
        format!("{plan:?}"),
        "EarlyDecodePlan { decode_box: DecodeBox { width: 640, height: 480 }, .. }"
    );
}

#[test]
fn instance_failure_display_preserves_the_lock_failure_message() {
    // Mutant: crates/pr-app/src/startup/instance.rs:24, replace Display::fmt with Ok(Default::default()).
    // C#: ShellStartup.StartupCoreAsync propagates InstanceScope failures to FailStartup.
    let error = pr_app::startup::instance::InstanceStartError {
        message: "fake mutex acquisition failed".into(),
    };
    assert_eq!(error.to_string(), "fake mutex acquisition failed");
}

#[test]
fn startup_failure_display_preserves_the_cause_for_the_failure_dialog() {
    // Mutant: crates/pr-shell/src/startup/error.rs:38, replace Display::fmt with Ok(Default::default()).
    // C#: ShellStartup.FailStartup passes ex.Message to Tr.AppStartupFailed.
    for (error, expected) in [
        (StartupError::Graph("graph failure".into()), "graph failure"),
        (StartupError::Overlay("overlay failure".into()), "overlay failure"),
        (StartupError::Worker("worker failure".into()), "worker failure"),
        (StartupError::Panicked("panic failure".into()), "panic failure"),
        (
            StartupError::Instance(pr_app::startup::instance::InstanceStartError {
                message: "instance failure".into(),
            }),
            "instance failure",
        ),
    ] {
        assert_eq!(error.to_string(), expected, "{error:?}");
    }
}
