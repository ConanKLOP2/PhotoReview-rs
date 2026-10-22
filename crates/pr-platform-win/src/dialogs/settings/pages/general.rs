//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/GeneralSettingsPage.cs` (305 lines, baseline `d92d6aae`): the "General" page (WPF `GeneralPage`): UI
//! language (+ open folder / reload / export translation), image order, initial zoom, window mode, the manual update check, the Explorer menu integration.
//! Unit W8-19g. Contract: `contracts/W8-19g.md`.
//!
//! Properties owned: `UiLanguage` (through `selected_language_code`), `ImageSortMode`, `InitialViewMode`, `InstanceMode`.
//!
//! The C# update check is `async` and posts back with `PostToUi`; here (D-W8B5-809) the page starts the presenter's `UpdateCheckJob` on a worker through a
//! [`BackgroundRunner`] and the answer returns as a `ContextUiAction` posted with `SettingsPageContext::ui_post()`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pr_app::localization::language_options::LanguageOption;
use pr_app::presenters::settings::{ShellIntegrationStatus, UpdateCheckCompleted, UpdateCheckJob};
use pr_app::presenters::settings_form_state::SettingsFormState;

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::SettingsPageContext;
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView};

// Compile-time proof that the update check data may cross to the worker and back (the posted action must be `Send`).
const _: fn() = || {
    fn is_send<T: Send>() {}
    is_send::<UpdateCheckJob>();
    is_send::<UpdateCheckCompleted>();
};

/// C# `SortTags`: the sort combo item order = declaration order = the `SettingsFormState::sort_mode_tag` keys. Real code (data).
pub const SORT_TAGS: [&str; 6] = [
    "Name",
    "NameAscending",
    "NameDescending",
    "SizeDescending",
    "SizeAscending",
    "Default",
];

/// Runs a job off the UI thread (production: a named `std::thread`; tests: run it inline). Replaces the C# `async`/`await` of `RunUpdateCheckAsync`.
/// Answers `false` when the job could not be started (a thread spawn failure); the page then ends the check with an error line.
pub type BackgroundRunner = Rc<dyn Fn(Box<dyn FnOnce() + Send + 'static>) -> bool>;

/// C# `Ids` (internal, for tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneralIds {
    pub language: i32,
    pub sort_mode: i32,
    pub view_mode: i32,
    pub instance_mode: i32,
    pub check_update: i32,
    pub open_update_page: i32,
    pub update_status: i32,
    pub shell_status: i32,
    pub shell_add: i32,
    pub shell_remove: i32,
    pub language_group: i32,
    pub open_folder: i32,
    pub reload: i32,
    pub export: i32,
}

/// C# `GeneralSettingsPage`.
pub struct GeneralSettingsPage {
    context: PageContextSlot,
    ids: GeneralIds,
    languages: Vec<LanguageOption>,
    /// C# `_updateStatusText`; shared with the posted action (which is `Send`), locked only for a copy or an assignment.
    update_status_text: Arc<Mutex<String>>,
    shell: Option<ShellIntegrationStatus>,
    runner: BackgroundRunner,
}

impl std::fmt::Debug for GeneralSettingsPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneralSettingsPage")
            .field("ids", &self.ids)
            .finish_non_exhaustive()
    }
}

impl Default for GeneralSettingsPage {
    fn default() -> Self {
        Self::new()
    }
}

impl GeneralSettingsPage {
    /// Production constructor: the runner spawns `std::thread::Builder::new().name("settings-update-check")` and answers `spawn(..).is_ok()` (a failure is logged with
    /// `pr_app::misc::app_log::AppLog::global().error(..)`).
    pub fn new() -> Self {
        todo!()
    }

    /// Test constructor: the runner is the caller's.
    pub fn with_background_runner(runner: BackgroundRunner) -> Self {
        todo!()
    }

    /// C# `Ids`.
    pub fn ids(&self) -> GeneralIds {
        todo!()
    }

    /// C# `Languages`: the entries of the language combo (empty until `attach`, or when there is no localization service).
    pub fn languages(&self) -> &[LanguageOption] {
        todo!()
    }
}

impl SettingsPageView for GeneralSettingsPage {
    /// `"General"`.
    fn key(&self) -> &str {
        todo!()
    }

    /// Four groups, exactly the C# order and texts (see the contract table).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        todo!()
    }

    /// Stores the context, `set_visible(open_update_page, false)`, `load_languages(None)`, `refresh_shell_status()`.
    fn attach(&mut self, context: SettingsPageContext) {
        todo!()
    }

    fn load(&mut self, state: &SettingsFormState) {
        todo!()
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        todo!()
    }

    /// `BN_CLICKED` only; the eight buttons of the C# chain, else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        todo!()
    }

    fn on_language_changed(&mut self) {
        todo!()
    }
}
