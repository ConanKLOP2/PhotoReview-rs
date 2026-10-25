//! Settings-window collaborators (W8-19j..m), no user configuration or real Shell integration.
#![allow(dead_code)] // Different Settings test binaries consume different fixture parts.
use pr_app::presenters::settings::SettingsPresenterOptions;
use pr_core::abstractions::{ShellIntegration, ShellMenuState};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_platform_win::dialogs::action_profiles::DialogWarningSink;
use pr_platform_win::dialogs::settings::dialog::{SettingsDialog, SettingsDialogArgs, SettingsDialogOptions};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug)]
struct InertShell;
impl ShellIntegration for InertShell {
    fn get_state(&self, _: &str) -> ShellMenuState {
        ShellMenuState::NotRegistered
    }
    fn register(&self, _: &str) -> std::io::Result<()> {
        panic!("unexpected real Shell registration")
    }
    fn unregister(&self) -> std::io::Result<()> {
        panic!("unexpected real Shell unregistration")
    }
}
#[derive(Default)]
pub struct Warnings(pub RefCell<Vec<(String, String)>>);
impl DialogWarningSink for Warnings {
    fn show_warning(&self, _: isize, title: &str, message: &str) {
        self.0.borrow_mut().push((title.to_owned(), message.to_owned()));
    }
}
pub fn args() -> SettingsDialogArgs {
    SettingsDialogArgs {
        current: AppSettings::new(&EnglishSettingsTexts),
        presenter: SettingsPresenterOptions::new(16 << 30, Arc::new(EnglishSettingsTexts), Arc::new(InertShell)),
        decoder_factory: None,
        options: SettingsDialogOptions {
            warnings: Some(Rc::new(Warnings::default())),
            ..Default::default()
        },
        chrome: None,
    }
}
pub fn dialog() -> SettingsDialog {
    SettingsDialog::new(args()).unwrap()
}

/// Page with observable lifecycle callbacks for frame ownership tests.
pub struct ProbePage {
    pub key: &'static str,
    pub events: Rc<RefCell<Vec<String>>>,
}
impl pr_platform_win::dialogs::settings::page_view::SettingsPageView for ProbePage {
    fn key(&self) -> &str {
        self.key
    }
    fn declare(&mut self, page: &mut pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder) {
        page.note(
            Some(pr_platform_win::dialogs::settings::page_view::fixed_text(self.key)),
            false,
        );
        self.events.borrow_mut().push(format!("declare:{}", self.key));
    }
    fn attach(&mut self, _: pr_platform_win::dialogs::settings::context::SettingsPageContext) {}
    fn load(&mut self, _: &pr_app::presenters::settings_form_state::SettingsFormState) {}
    fn collect(&mut self, _: &mut pr_app::presenters::settings_form_state::SettingsFormState) {}
    fn on_closed(&mut self) {
        self.events.borrow_mut().push(format!("close:{}", self.key));
    }
}
