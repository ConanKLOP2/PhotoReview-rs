//! Port of `SettingsDialog.cs` lines 797 to 832 (the `switch` of `TrySaveAll` and `FocusField`), baseline `d92d6aae`. Unit W8-19l. Contract: `contracts/W8-19l.md`.
//!
//! The rules of validation live in the presenter (`SettingsPresenter::try_save`); this file is the UI decision about its answer, in pure form so it is tested without a
//! window: which warning (title, text) a refusal shows, whether the focus moves, and which page owns a field.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use pr_app::presenters::settings_form_state::{SettingsFocusField, SettingsSaveResult};

use super::frame::SettingsContent;
use crate::dialogs::native::dialog::NativeDialog;

/// What `try_save_all` does with a save result (C# `switch (result.Outcome)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavePlan {
    /// `Saved`: `saved = true`, the save succeeded.
    Saved,
    /// `Invalid` / `SaveFailed`: show the warning, then (only for `Invalid`) move the focus to `focus`.
    Warn {
        /// `tr::dialog_settings_invalid_title()` for `Invalid`, `tr::dialog_settings_save_failed_title()` for `SaveFailed`.
        title: String,
        /// `result.message` or `""` when `None`.
        message: String,
        /// `result.focus` for `Invalid`; `SettingsFocusField::None` for `SaveFailed` (the C# does not focus then).
        focus: SettingsFocusField,
    },
}

/// The pure plan of a save result. Reads the ambient language for the title.
pub fn plan_for(result: &SettingsSaveResult) -> SavePlan {
    todo!()
}

/// C# `FocusField`'s search (lines 815 to 832), pure: `targets[i]` is `pages[i].focus_target(field)`; the answer is the FIRST page that has a target, as `(page index, control
/// id)`; `None` when no page has one. (`SettingsFocusField::None` is handled by the caller before: nothing happens.)
pub fn locate_focus(targets: &[Option<i32>]) -> Option<(usize, i32)> {
    todo!()
}

impl SettingsContent {
    /// C# `FocusField(field)`: `field == None` -> nothing. Else `targets` = `pages[i].focus_target(field)` for every page in order (a page that is borrowed answers `None`),
    /// `locate_focus(&targets)`: `Some((i, id))` -> `activate_page(i, true, true)` (this creates the page controls when needed) then `focus_control(id)`.
    pub(super) fn focus_field(&self, dialog: &NativeDialog, field: SettingsFocusField) {
        todo!()
    }
}
