//! Review ST-SHELL M4: the Settings window and its update check must get a version even when the options carry none.
//! C# `SettingsDialog`: `Options.Version ?? BuildInfo.GetVersion(assembly)`.

use pr_core::updates::version::AppVersion;
use pr_shell::startup::composition_root::settings_version;

#[test]
fn settings_version_without_options_is_the_crate_version_and_parses() {
    let version = settings_version(None);
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert!(
        AppVersion::try_parse(Some(&version)).is_some(),
        "the update check must accept {version:?}"
    );
}

#[test]
fn settings_version_blank_option_falls_back_and_explicit_option_wins() {
    assert_eq!(settings_version(Some("  ".to_owned())), env!("CARGO_PKG_VERSION"));
    assert_eq!(settings_version(Some("9.8.7+abc".to_owned())), "9.8.7+abc");
}
