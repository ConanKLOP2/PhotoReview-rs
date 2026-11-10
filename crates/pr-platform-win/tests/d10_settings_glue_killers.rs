//! D-10 / C-28 mutation killers for the Settings window glue (`context.rs`, `dialog/host.rs`), found by the 2026-11-10 time-boxed run
//! (`docs/reviews/D10-SETTINGS-GLUE-2026-11-10.md`). Pure: no window, no real browser (only rejected URLs are passed).
#![cfg(windows)]
#[path = "common/settings_dialog_fixture.rs"]
mod fixture;
#[path = "common/settings_host_fixture.rs"]
mod host_fixture;

use pr_platform_win::dialogs::settings::context::SettingsPageHost;
use pr_platform_win::dialogs::settings::dialog::host::default_open_url;

// Kills context.rs:82:9 `show_external_editors -> Ok(None)`: a host that does not wire the editors dialog answers `Err` (C# `ShowExternalEditors == null`), never "cancelled".
#[test]
fn show_external_editors_default_is_unavailable_not_cancelled() {
    let host = host_fixture::RecordingHost::new();
    assert!(host.show_external_editors(0, &[], None).is_err());
}

// Kills host.rs:34:5 `default_open_url -> Ok(())` and host.rs:41:8 `delete !` for the rejected inputs; none of these reaches ShellExecute.
#[test]
fn default_open_url_rejects_empty_and_non_web_urls_with_the_reason() {
    assert_eq!(default_open_url(""), Err("empty url".to_owned()));
    assert_eq!(default_open_url("   "), Err("empty url".to_owned()));
    assert_eq!(default_open_url("not a url"), Err("unsupported url scheme".to_owned()));
    assert_eq!(default_open_url("http://"), Err("unsupported url scheme".to_owned()));
}

// Kills frame.rs:149:9, :171:9, :365:9 (`Debug::fmt -> Ok(Default::default())` writes nothing) and :190:9 (the same for `Display`).
#[test]
fn settings_dialog_debug_and_error_texts_are_written() {
    let options = pr_platform_win::dialogs::settings::dialog::SettingsDialogOptions::default();
    assert!(format!("{options:?}").starts_with("SettingsDialogOptions"));
    assert!(format!("{:?}", fixture::args()).starts_with("SettingsDialogArgs"));
    let mut args = fixture::args();
    args.options.pages = Some(Vec::new());
    let Err(error) = pr_platform_win::dialogs::settings::dialog::SettingsDialog::new(args) else {
        panic!("an empty page list must be refused");
    };
    assert_eq!(
        error.to_string(),
        format!(
            "A settings page is required for every SettingsPresenter.Pages entry (expected {}, got 0).",
            7
        )
    );
    let dialog = fixture::dialog();
    assert!(format!("{:?}", dialog.content()).starts_with("SettingsContent"));
}
