//! Inline settings declaration and detached-host contracts; no golden files or windows.
use std::rc::Rc;
use std::sync::Arc;

use pr_app::presenters::settings::SettingsPresenterOptions;
use pr_core::abstractions::{ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_platform_win::dialogs::action_profiles::DialogWarningSink;
use pr_platform_win::dialogs::native::dialog::DialogContent;
use pr_platform_win::dialogs::native::template::DialogControlClass;
use pr_platform_win::dialogs::settings::context::SettingsPageHost;
use pr_platform_win::dialogs::settings::dialog::frame::{SettingsDialog, SettingsDialogArgs, SettingsDialogOptions};
use pr_platform_win::dialogs::settings::dialog::host::PageHost;

struct InertShell;
impl ShellIntegration for InertShell {
    fn get_state(&self, _: &str) -> ShellMenuState {
        ShellMenuState::NotRegistered
    }
    fn register(&self, _: &str) -> std::io::Result<()> {
        panic!("unexpected Shell registration")
    }
    fn unregister(&self) -> std::io::Result<()> {
        panic!("unexpected Shell unregistration")
    }
}

struct NoWarnings;
impl DialogWarningSink for NoWarnings {
    fn show_warning(&self, _: isize, _: &str, _: &str) {
        panic!("unexpected modal warning")
    }
}

fn dialog() -> SettingsDialog {
    SettingsDialog::new(SettingsDialogArgs {
        current: AppSettings::new(&EnglishSettingsTexts),
        presenter: SettingsPresenterOptions::new(16 << 30, Arc::new(EnglishSettingsTexts), Arc::new(InertShell)),
        decoder_factory: None,
        options: SettingsDialogOptions {
            version: Some("settingswin3-inline".to_owned()),
            warnings: Some(Rc::new(NoWarnings)),
            ..Default::default()
        },
        chrome: None,
    })
    .unwrap()
}

#[test]
fn build_items_frame_styles_and_order_match_csharp_declaration() {
    // C# SettingsDialog.BuildItems. frame.rs:462 return Default; 464/484/502 | -> &/^; 502 == -> !=.
    let dialog = dialog();
    assert_eq!(dialog.hwnd(), 0);
    let items = DialogContent::items(dialog.content().as_ref());
    let expected = [
        (DialogControlClass::Static, 100, 0x5000_0080, 0),
        (DialogControlClass::ListBox, 101, 0x5001_0151, 0x200),
        (DialogControlClass::Static, 102, 0x5000_0080, 0),
        (DialogControlClass::Static, 103, 0x5000_0080, 0),
        (DialogControlClass::ScrollBar, 104, 0x5000_0001, 0),
        (DialogControlClass::Static, 110, 0x5600_0080, 0x1_0000),
        (DialogControlClass::Static, 111, 0x5600_0080, 0x1_0000),
        (DialogControlClass::Static, 112, 0x5600_0080, 0x1_0000),
        (DialogControlClass::Static, 113, 0x5600_0080, 0x1_0000),
        (DialogControlClass::Static, 114, 0x5600_0080, 0x1_0000),
        (DialogControlClass::Static, 115, 0x5600_0080, 0x1_0000),
        (DialogControlClass::Static, 116, 0x5600_0080, 0x1_0000),
        (DialogControlClass::Button, 121, 0x5001_0000, 0),
        (DialogControlClass::Button, 122, 0x5001_0000, 0),
        (DialogControlClass::Button, 123, 0x5001_0000, 0),
        (DialogControlClass::Button, 124, 0x5001_0000, 0),
        (DialogControlClass::Button, 2, 0x5001_0000, 0),
        (DialogControlClass::Button, 120, 0x5001_0000, 0),
        (DialogControlClass::Button, 1, 0x5001_0001, 0),
    ];
    let actual: Vec<_> = items
        .iter()
        .map(|item| (item.class, item.id, item.style, item.ex_style))
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(items[2].text, "settingswin3-inline");
}

#[test]
fn detached_host_reads_return_neutral_values_without_native_calls() {
    // Rust detached contract D-W8B5-705. host.rs:202/237/280/285 return substitutions.
    let host = PageHost::new(std::rc::Weak::new());
    assert_eq!(host.combo_selected_index(9001), -1);
    assert!(host.current_shortcut_texts().is_empty());
    assert_eq!(host.actions_json(), "");
    assert_eq!(host.refresh_actions_json_from_pages(), "");
}
