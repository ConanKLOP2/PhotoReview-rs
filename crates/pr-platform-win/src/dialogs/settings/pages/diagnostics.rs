//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/DiagnosticsSettingsPage.cs` (41 lines, baseline `d92d6aae`): the "Diagnostics" page (WPF
//! `DiagnosticsPage`): the diagnostic-log switch and the "open the log location" button. Unit W8-19d. Contract: `contracts/W8-19d.md`.
//!
//! Property owned: `LoggingEnabled` (`SettingsFormState::logging_enabled`). Everything else (defaults, validation) is the presenter's.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::presenters::settings_form_state::SettingsFormState;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView};

/// C# `Ids` (internal, for tests): the two control ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticsIds {
    pub logging: i32,
    pub open_log: i32,
}

/// C# `DiagnosticsSettingsPage`. `new()` is what `catalog::FACTORIES` calls.
#[derive(Debug)]
pub struct DiagnosticsSettingsPage {
    context: PageContextSlot,
    logging: i32,
    open_log: i32,
}

impl Default for DiagnosticsSettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticsSettingsPage {
    /// Ids 0 (not declared yet), detached context.
    pub fn new() -> Self {
        todo!()
    }

    /// C# `Ids`.
    pub fn ids(&self) -> DiagnosticsIds {
        todo!()
    }
}

impl SettingsPageView for DiagnosticsSettingsPage {
    /// `"Diagnostics"`.
    fn key(&self) -> &str {
        todo!()
    }

    /// One group (title `tr::settings_nav_diagnostics`): `logging = g.check(tr::settings_logging_label, 0)`, `open_log = g.buttons(false, [tr::settings_open_log_location])[0]`.
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        todo!()
    }

    fn attach(&mut self, context: SettingsPageContext) {
        todo!()
    }

    /// `set_checked(logging, state.logging_enabled)`.
    fn load(&mut self, state: &SettingsFormState) {
        todo!()
    }

    /// `state.logging_enabled = is_checked(logging)`.
    fn collect(&mut self, state: &mut SettingsFormState) {
        todo!()
    }

    /// `BN_CLICKED` of `open_log` -> `context.run_explorer(&SettingsPresenter::prepare_log_location())`, handled; anything else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        todo!()
    }
}
