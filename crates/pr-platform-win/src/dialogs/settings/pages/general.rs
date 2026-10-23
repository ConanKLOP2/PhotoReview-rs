//! Port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages/GeneralSettingsPage.cs` (305 lines, baseline `d92d6aae`): the "General" page (WPF `GeneralPage`): UI
//! language (+ open folder / reload / export translation), image order, initial zoom, window mode, the manual update check, the Explorer menu integration.
//! Unit W8-19g. Contract: `contracts/W8-19g.md`.
//!
//! Properties owned: `UiLanguage` (through `selected_language_code`), `ImageSortMode`, `InitialViewMode`, `InstanceMode`.
//!
//! The C# update check is `async` and posts back with `PostToUi`; here (D-W8B5-809) the page starts the presenter's `UpdateCheckJob` on a worker through a
//! [`BackgroundRunner`] and the answer returns as a `ContextUiAction` posted with `SettingsPageContext::ui_post()`.
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use crate::dialogs::settings::builder::inputs::SettingsPageBuilder;
use crate::dialogs::settings::context::{SettingsPageContext, with_field_context};
use crate::dialogs::settings::page_view::{PageContextSlot, SettingsPageView, text_fn};
use crate::win32::dialog_native::BN_CLICKED;
use pr_app::localization::language_options::LanguageOption;
use pr_app::misc::app_log::AppLog;
use pr_app::presenters::settings::{ShellIntegrationStatus, UpdateCheckCompleted, UpdateCheckJob};
use pr_app::presenters::settings_form_state::SettingsFormState;

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
        Self::with_background_runner(Rc::new(|job| {
            match std::thread::Builder::new()
                .name("settings-update-check".to_owned())
                .spawn(job)
            {
                Ok(_) => true,
                Err(error) => {
                    AppLog::global().error("Update check thread failed to start", Some(&error));
                    false
                }
            }
        }))
    }

    /// Test constructor: the runner is the caller's.
    pub fn with_background_runner(runner: BackgroundRunner) -> Self {
        Self {
            context: PageContextSlot::new(),
            ids: GeneralIds {
                language: 0,
                sort_mode: 0,
                view_mode: 0,
                instance_mode: 0,
                check_update: 0,
                open_update_page: 0,
                update_status: 0,
                shell_status: 0,
                shell_add: 0,
                shell_remove: 0,
                language_group: 0,
                open_folder: 0,
                reload: 0,
                export: 0,
            },
            languages: Vec::new(),
            update_status_text: Arc::new(Mutex::new(String::new())),
            shell: None,
            runner,
        }
    }

    /// C# `Ids`.
    pub fn ids(&self) -> GeneralIds {
        self.ids
    }

    /// C# `Languages`: the entries of the language combo (empty until `attach`, or when there is no localization service).
    pub fn languages(&self) -> &[LanguageOption] {
        &self.languages
    }
}

impl SettingsPageView for GeneralSettingsPage {
    /// `"General"`.
    fn key(&self) -> &str {
        "General"
    }

    /// Four groups, exactly the C# order and texts (see the contract table).
    fn declare(&mut self, page: &mut SettingsPageBuilder) {
        let id = page.group(text_fn(pr_l10n::tr::settings_group_language), |g| {
            self.ids.language = g.combo(text_fn(pr_l10n::tr::settings_language_label), None, 0).control;
            let b = g.buttons(
                true,
                &[
                    text_fn(pr_l10n::tr::settings_language_open_folder),
                    text_fn(pr_l10n::tr::settings_language_reload),
                    text_fn(pr_l10n::tr::settings_language_export),
                ],
            );
            self.ids.open_folder = *b.first().unwrap_or(&0);
            self.ids.reload = *b.get(1).unwrap_or(&0);
            self.ids.export = *b.get(2).unwrap_or(&0);
            g.note(Some(text_fn(pr_l10n::tr::settings_language_hint)), true);
        });
        self.ids.language_group = id;
        page.group(text_fn(pr_l10n::tr::settings_nav_general), |g| {
            let tags = [
                pr_l10n::tr::enum_image_sort_mode_name,
                pr_l10n::tr::enum_image_sort_mode_name_ascending,
                pr_l10n::tr::enum_image_sort_mode_name_descending,
                pr_l10n::tr::enum_image_sort_mode_size_descending,
                pr_l10n::tr::enum_image_sort_mode_size_ascending,
                pr_l10n::tr::enum_image_sort_mode_default,
            ];
            self.ids.sort_mode = g
                .combo(
                    text_fn(pr_l10n::tr::settings_sort_mode_label),
                    Some(tags.into_iter().map(text_fn).collect()),
                    0,
                )
                .control;
            g.note(Some(text_fn(pr_l10n::tr::settings_sort_mode_hint)), true);
            let views = [
                pr_l10n::tr::enum_initial_view_mode_fit,
                pr_l10n::tr::enum_initial_view_mode_fit_width,
                pr_l10n::tr::enum_initial_view_mode_fit_height,
                pr_l10n::tr::enum_initial_view_mode_click_zoom_level,
            ];
            let mut items: Vec<_> = views.into_iter().map(text_fn).collect();
            items.push(text_fn(|| "100%".into()));
            items.push(text_fn(|| "200%".into()));
            self.ids.view_mode = g
                .combo(text_fn(pr_l10n::tr::settings_view_mode_label), Some(items), 0)
                .control;
            let modes = [
                pr_l10n::tr::enum_instance_mode_single_window,
                pr_l10n::tr::enum_instance_mode_per_folder,
            ];
            self.ids.instance_mode = g
                .combo(
                    text_fn(pr_l10n::tr::settings_instance_mode_label),
                    Some(modes.into_iter().map(text_fn).collect()),
                    0,
                )
                .control;
            g.note(Some(text_fn(pr_l10n::tr::settings_instance_mode_hint)), true);
        });
        page.group(text_fn(pr_l10n::tr::update_section_title), |g| {
            let b = g.buttons(
                false,
                &[
                    text_fn(pr_l10n::tr::update_check_button),
                    text_fn(pr_l10n::tr::update_open_page_button),
                ],
            );
            self.ids.check_update = *b.first().unwrap_or(&0);
            self.ids.open_update_page = *b.get(1).unwrap_or(&0);
            self.ids.update_status = g.text(None);
            g.note(Some(text_fn(pr_l10n::tr::update_hint)), false);
        });
        page.group(text_fn(pr_l10n::tr::settings_shell_group), |g| {
            self.ids.shell_status = g.text(None);
            let b = g.buttons(
                false,
                &[
                    text_fn(pr_l10n::tr::settings_shell_add),
                    text_fn(pr_l10n::tr::settings_shell_remove),
                ],
            );
            self.ids.shell_add = *b.first().unwrap_or(&0);
            self.ids.shell_remove = *b.get(1).unwrap_or(&0);
            g.note(Some(text_fn(pr_l10n::tr::settings_shell_hint)), false);
        });
    }

    /// Stores the context, `set_visible(open_update_page, false)`, `load_languages(None)`, `refresh_shell_status()`.
    fn attach(&mut self, context: SettingsPageContext) {
        self.context.attach(context.clone());
        context.set_visible(self.ids.open_update_page, false);
        self.load_languages(None);
        self.refresh_shell_status();
    }

    fn load(&mut self, state: &SettingsFormState) {
        let c = self.context.context();
        c.set_combo_index(self.ids.view_mode, state.view_mode_index);
        c.set_combo_index(self.ids.instance_mode, state.instance_mode_index);
        let i = state
            .sort_mode_tag
            .as_deref()
            .and_then(|tag| SORT_TAGS.iter().position(|t| *t == tag))
            .unwrap_or(0);
        c.set_combo_index(self.ids.sort_mode, i32::try_from(i).unwrap_or(0));
    }

    fn collect(&mut self, state: &mut SettingsFormState) {
        let c = self.context.context();
        c.set_combo_index(self.ids.view_mode, state.view_mode_index);
        c.set_combo_index(self.ids.instance_mode, state.instance_mode_index);
        let sort = c.combo_index(self.ids.sort_mode);
        state.sort_mode_tag = usize::try_from(sort)
            .ok()
            .and_then(|i| SORT_TAGS.get(i))
            .map(|s| (*s).to_owned());
        let lang = c.combo_index(self.ids.language);
        state.selected_language_code = usize::try_from(lang)
            .ok()
            .and_then(|i| self.languages.get(i))
            .map(|l| l.code.clone());
    }

    /// `BN_CLICKED` only; the eight buttons of the C# chain, else `false`.
    fn on_command(&mut self, id: i32, notification: u32) -> bool {
        if notification != BN_CLICKED {
            return false;
        }
        let c = self.context.context();
        if id == self.ids.open_folder {
            let has = c.presenter().borrow().has_localization();
            if has {
                let req = c.presenter().borrow().prepare_languages_folder();
                c.run_explorer(&req);
            }
        } else if id == self.ids.reload {
            self.reload_translations();
        } else if id == self.ids.export {
            self.export_translation();
        } else if id == self.ids.check_update {
            if c.is_enabled(id) {
                self.start_update_check();
            }
        } else if id == self.ids.open_update_page {
            let failed = with_field_context(|f| c.presenter().borrow().open_update_page(&mut |url| c.open_url(url), f));
            if let Some(t) = failed {
                self.set_update_status(&t);
            }
        } else if id == self.ids.shell_add || id == self.ids.shell_remove {
            let add = id == self.ids.shell_add;
            let status = with_field_context(|f| c.presenter().borrow().change_shell_registration(add, f));
            self.show_shell_status(status);
        } else {
            return false;
        }
        true
    }

    fn on_language_changed(&mut self) {
        let c = self.context.context();
        let text = self
            .update_status_text
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        c.set_text(self.ids.update_status, &text);
        if self.shell.is_some() {
            self.refresh_shell_status();
        }
    }
}

impl GeneralSettingsPage {
    fn load_languages(&mut self, current: Option<&str>) {
        let loaded = with_field_context(|f| self.context.context().presenter().borrow().load_languages(current, f));
        let c = self.context.context();
        let Some((options, index)) = loaded else {
            c.set_visible(self.ids.language_group, false);
            return;
        };
        let names: Vec<_> = options.iter().map(|o| o.display_name.clone()).collect();
        self.languages = options;
        c.set_combo_items(
            self.ids.language,
            &names,
            index.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1),
        );
    }
    fn selected_language(&self) -> Option<LanguageOption> {
        usize::try_from(self.context.context().combo_index(self.ids.language))
            .ok()
            .and_then(|i| self.languages.get(i))
            .cloned()
    }
    fn set_update_status(&self, text: &str) {
        *self.update_status_text.lock().unwrap_or_else(PoisonError::into_inner) = text.to_owned();
        let c = self.context.context();
        c.set_text(self.ids.update_status, text);
        c.relayout();
    }
    fn show_shell_status(&mut self, status: ShellIntegrationStatus) {
        self.shell = Some(status.clone());
        let c = self.context.context();
        c.set_text(self.ids.shell_status, &status.text);
        if let Some(label) = status.add_label.as_deref() {
            c.set_text(self.ids.shell_add, label)
        }
        c.set_enabled(self.ids.shell_add, status.add_enabled);
        c.set_enabled(self.ids.shell_remove, status.remove_enabled);
        c.relayout();
    }
    fn refresh_shell_status(&mut self) {
        let status = with_field_context(|f| self.context.context().presenter().borrow().refresh_shell_status(f));
        self.show_shell_status(status);
    }
    fn reload_translations(&mut self) {
        let Some(result) = self.context.context().presenter().borrow().reload_translations() else {
            return;
        };
        let code = self.selected_language().map(|l| l.code);
        self.load_languages(code.as_deref());
        self.context
            .context()
            .show_warning(&pr_l10n::tr::dialog_reload_translations_title(), &result.message);
    }
    fn export_translation(&mut self) {
        let c = self.context.context();
        if !c.presenter().borrow().has_localization() {
            return;
        }
        let (request, error) = c
            .presenter()
            .borrow()
            .export_translation(self.selected_language().as_ref());
        if let Some(e) = error {
            c.show_warning(&pr_l10n::tr::dialog_export_translation_failed_title(), &e);
            return;
        }
        if let Some(r) = request {
            c.run_explorer(&r)
        }
    }
    fn start_update_check(&self) {
        let c = self.context.context();
        c.set_enabled(self.ids.check_update, false);
        c.set_visible(self.ids.open_update_page, false);
        self.set_update_status(&pr_l10n::tr::update_check_checking());
        let job = c.presenter().borrow_mut().begin_update_check(c.version().as_deref());
        let post = c.ui_post();
        let ids = self.ids;
        let text = Arc::clone(&self.update_status_text);
        let started = (self.runner)(Box::new(move || {
            let completed = job.run();
            post.post(Box::new(move |ctx| apply_update_result(ctx, completed, ids, text)));
        }));
        if !started {
            self.set_update_status(&pr_l10n::tr::update_status_failed_bad_response());
            c.set_enabled(self.ids.check_update, true);
        }
    }
}

fn apply_update_result(
    ctx: &SettingsPageContext,
    completed: UpdateCheckCompleted,
    ids: GeneralIds,
    text: Arc<Mutex<String>>,
) {
    let shared_presenter = ctx.presenter();
    let Ok(mut presenter) = shared_presenter.try_borrow_mut() else {
        let again = Arc::clone(&text);
        ctx.post_to_ui(Box::new(move |c| apply_update_result(c, completed, ids, again)));
        return;
    };
    let display = with_field_context(|f| presenter.finish_update_check(completed, f));
    drop(presenter);
    let Some(display) = display else { return };
    *text.lock().unwrap_or_else(PoisonError::into_inner) = display.status_text.clone();
    ctx.set_text(ids.update_status, &display.status_text);
    ctx.relayout();
    ctx.set_visible(ids.open_update_page, display.show_open_page());
    ctx.set_enabled(ids.check_update, true);
}
