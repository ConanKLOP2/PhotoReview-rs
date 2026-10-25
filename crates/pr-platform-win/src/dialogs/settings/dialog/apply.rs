//! Port of `SettingsDialog.cs` lines 745 to 776 (`OnCommand`), 795 to 813 (`OnOk`, `TrySaveAll`) and 834 to 907 (`ResetActivePage`, `ExportSettings`, `ImportSettings`,
//! `ImportSettingsFrom`, `Warn`), baseline `d92d6aae`. Unit W8-19l. Contract: `contracts/W8-19l.md`.
//!
//! Save / Apply / Defaults / Reset page / Import / Export and the dispatch of `WM_COMMAND`. The decision "what a save result means for the user" is `validate::plan_for`;
//! this file runs it. No `RefCell` borrow (presenter, page, builder) is held across `warn`, the file dialogs or a page call (D-W8B5-904, D-W8B5-906).

use std::path::Path;

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;
use crate::dialogs::settings::context::with_field_context;
use pr_app::presenters::settings::SettingsPresenter;

/// C# `"PhotoReview-settings.json"`: the suggested file name of Export (a file name, not UI text).
pub const EXPORT_FILE_NAME: &str = "PhotoReview-settings.json";
/// C# `"json"`: the default extension of the Export dialog.
pub const EXPORT_DEFAULT_EXTENSION: &str = "json";

/// What a `WM_COMMAND` of the frame means (C# `OnCommand` switch, lines 747 to 765), pure. `notification` is the high word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameCommand {
    /// `NAV_ID` + `LBN_SELCHANGE`.
    NavSelection,
    /// `APPLY_ID` + `BN_CLICKED`.
    Apply,
    /// `DEFAULTS_ID` + `BN_CLICKED`.
    Defaults,
    /// `RESET_PAGE_ID` + `BN_CLICKED`.
    ResetPage,
    /// `EXPORT_ID` + `BN_CLICKED`.
    Export,
    /// `IMPORT_ID` + `BN_CLICKED`.
    Import,
    /// Not a frame command: swallowed while a page is loading, else routed to the page that owns the id.
    Other,
}

/// The pure switch of `OnCommand`. The notification must match: `NAV_ID` with anything but `LBN_SELCHANGE` (1), or a button with anything but `BN_CLICKED` (0), is `Other`
/// (and then goes to the page range check, which answers `-1` for ids below 1000). `SAVE_ID` and `CANCEL_ID` never get here (the dialog manager sends `IDOK` / `IDCANCEL`
/// to `on_ok` / `on_cancel`).
pub fn classify_command(id: i32, notification: i32) -> FrameCommand {
    use super::frame::*;
    match (id, notification) {
        (NAV_ID, 1) => FrameCommand::NavSelection,
        (APPLY_ID, 0) => FrameCommand::Apply,
        (DEFAULTS_ID, 0) => FrameCommand::Defaults,
        (RESET_PAGE_ID, 0) => FrameCommand::ResetPage,
        (EXPORT_ID, 0) => FrameCommand::Export,
        (IMPORT_ID, 0) => FrameCommand::Import,
        _ => FrameCommand::Other,
    }
}

impl SettingsContent {
    /// C# `OnCommand(id, notification)`: `classify_command`; `NavSelection` -> `on_nav_selection_changed`; `Apply` -> `try_save_all`; `Defaults` ->
    /// `load_all(presenter.borrow_mut().apply_defaults())` (the state is a LOCAL before `load_all` so the presenter borrow ended); `ResetPage` -> `reset_active_page`; `Export` ->
    /// `export_settings`; `Import` -> `import_settings`; each returns `true`. `Other`: `loading` -> `true` (the notification was caused by loading values); else
    /// `nav::page_of(id)` in range -> `page.try_borrow_mut()`: `Ok` -> `page.on_command(id, notification as u32)` (its answer); `Err` (the page is running: an echo of its own write,
    /// D-W8B5-805) -> `true`; out of range -> `false`.
    pub(super) fn handle_command(&self, dialog: &NativeDialog, id: i32, notification: i32) -> bool {
        match classify_command(id, notification) {
            FrameCommand::NavSelection => self.on_nav_selection_changed(dialog),
            FrameCommand::Apply => {
                self.try_save_all(dialog);
            }
            FrameCommand::Defaults => {
                let defaults = self.presenter.borrow_mut().apply_defaults();
                self.load_all(dialog, &defaults);
            }
            FrameCommand::ResetPage => self.reset_active_page(dialog),
            FrameCommand::Export => self.export_settings(dialog),
            FrameCommand::Import => self.import_settings(dialog),
            FrameCommand::Other => {
                if self.view.loading.get() {
                    return true;
                }
                let Some(page) = self.page(super::nav::page_of(id)) else {
                    return false;
                };
                return page
                    .try_borrow_mut()
                    .map_or(true, |mut page| page.on_command(id, notification as u32));
            }
        }
        true
    }

    /// C# `TrySaveAll()` (lines 797 to 813): `state = collect_all()`; `result = with_field_context(|ctx| presenter.borrow_mut().try_save(&state, ctx))` (the borrow ends with the
    /// call); `validate::plan_for(&result)`: `Warn { title, message, focus }` -> `warn(title, message)`, then `focus_field(focus)`, return `false`; `Saved` -> `saved = true`,
    /// `true`. A `Saved` result of an earlier call is never reset by a later refusal.
    pub fn try_save_all(&self, dialog: &NativeDialog) -> bool {
        let state = self.collect_all();
        let result = with_field_context(|ctx| self.presenter.borrow_mut().try_save(&state, ctx));
        match super::validate::plan_for(&result) {
            super::validate::SavePlan::Saved => {
                self.view.saved.set(true);
                true
            }
            super::validate::SavePlan::Warn { title, message, focus } => {
                self.warn(dialog, &title, &message);
                self.focus_field(dialog, focus);
                false
            }
        }
    }

    /// C# `ResetActivePage()` (lines 834 to 854): `active < 0` -> nothing. Else `scratch` = `self.scratch.presenter(&presenter.borrow().settings().clone())` (a NEW presenter with NO
    /// store, NO localization and NO update checker, the same RAM size, texts and shell integration as the real one), `defaults = scratch.apply_defaults()`, then with `loading` set
    /// `pages[active].load(&defaults)` (skipped when borrowed), then `relayout_active_page`. The working copy of the real presenter does NOT change until Save.
    pub fn reset_active_page(&self, dialog: &NativeDialog) {
        let Some(page) = self.page(self.view.active.get()) else {
            return;
        };
        let settings = self.presenter.borrow().settings().clone();
        let defaults = self.scratch.presenter(&settings).apply_defaults();
        {
            let _loading = super::state::LoadingGuard::new(self);
            if let Ok(mut page) = page.try_borrow_mut() {
                page.load(&defaults);
            }
        }
        self.relayout_active_page(dialog);
    }

    /// C# `ExportSettings()` (lines 856 to 874): `options.files` is `None` -> nothing. `path = files.pick_save_file(dialog.hwnd(), tr::dialog_export_settings_title(),
    /// FileFilterText::parse(tr::dialog_file_filter_json()), Some("json"), Some("PhotoReview-settings.json"), Some(settings_file_dialog_directory))`; `Ok(None)` or `Err(_)` (a failed
    /// dialog is treated as a cancelled one, D-W8B5-906) -> nothing; `Ok(Some(path))` -> `presenter.borrow().export_settings_to(path, ctx)`; `Some(error)` ->
    /// `warn(tr::dialog_settings_invalid_title(), error)`.
    pub(super) fn export_settings(&self, dialog: &NativeDialog) {
        let Some(files) = self.options.files.clone() else {
            return;
        };
        let directory = SettingsPresenter::settings_file_dialog_directory();
        let filters = crate::dialogs::action_profiles::FileFilterText::parse(&pr_l10n::tr::dialog_file_filter_json());
        let Ok(Some(path)) = files.pick_save_file(
            dialog.hwnd(),
            &pr_l10n::tr::dialog_export_settings_title(),
            &filters,
            Some(EXPORT_DEFAULT_EXTENSION),
            Some(EXPORT_FILE_NAME),
            directory.to_str(),
        ) else {
            return;
        };
        let error = with_field_context(|ctx| self.presenter.borrow().export_settings_to(Path::new(&path), ctx));
        if let Some(error) = error {
            self.warn(dialog, &pr_l10n::tr::dialog_settings_invalid_title(), &error);
        }
    }

    /// C# `ImportSettings()` (lines 876 to 892): no `files` -> nothing. `files.pick_open_file(hwnd, tr::dialog_import_settings_title(), parse(tr::dialog_file_filter_json()),
    /// Some(settings_file_dialog_directory))`; `Ok(None)` / `Err` -> nothing; `Ok(Some(path))` -> `import_settings_from(path)`.
    pub(super) fn import_settings(&self, dialog: &NativeDialog) {
        let Some(files) = self.options.files.clone() else {
            return;
        };
        let directory = SettingsPresenter::settings_file_dialog_directory();
        let filters = crate::dialogs::action_profiles::FileFilterText::parse(&pr_l10n::tr::dialog_file_filter_json());
        if let Ok(Some(path)) = files.pick_open_file(
            dialog.hwnd(),
            &pr_l10n::tr::dialog_import_settings_title(),
            &filters,
            directory.to_str(),
        ) {
            self.import_settings_from(dialog, Path::new(&path));
        }
    }

    /// C# `ImportSettingsFrom(path)` (lines 894 to 905): the presenter's `import_settings_from(path, refresh, ctx)` where `refresh` only RECORDS `p.load_fields()` and returns
    /// `Ok(())` (D-W8B5-906: the pages call `context.presenter().borrow()` while loading, which would panic inside the presenter's `&mut self` call); AFTER the call, the
    /// last recorded state (if any) goes through `load_all` (the failure arm of the presenter refreshes twice; only the last state matters). Then `result.error` ->
    /// `warn(tr::dialog_settings_invalid_title(), error)`; else `result.repairs_notice` -> `warn(tr::dialog_import_settings_title(), notice)`.
    pub fn import_settings_from(&self, dialog: &NativeDialog, path: &Path) {
        let mut refresh = None;
        let result = with_field_context(|ctx| {
            self.presenter.borrow_mut().import_settings_from(
                path,
                &mut |presenter| {
                    refresh = Some(presenter.load_fields());
                    Ok(())
                },
                ctx,
            )
        });
        if let Some(state) = refresh {
            self.load_all(dialog, &state);
        }
        if let Some(error) = result.error {
            self.warn(dialog, &pr_l10n::tr::dialog_settings_invalid_title(), &error);
        } else if let Some(notice) = result.repairs_notice {
            self.warn(dialog, &pr_l10n::tr::dialog_import_settings_title(), &notice);
        }
    }

    /// C# `Warn(title, message)` = `warnings.show_warning(dialog.hwnd(), title, message)`; modal. No borrow is held by the caller.
    pub fn warn(&self, dialog: &NativeDialog, title: &str, message: &str) {
        self.warnings.show_warning(dialog.hwnd(), title, message);
    }
}
