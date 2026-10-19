//! `SettingsPresenter` (Wave 8 batch B2). One C# class (`Presenters/SettingsPresenter.cs`, 636 lines) split over four Rust files:
//! `load` (W8-10d: struct, constructor, pages, `load_fields`, RAM slider, hints, defaults), `validate` (W8-10d: `try_save`,
//! live duplicate-shortcut preview), `apply` (W8-10e: import/export, actions hand-off, context-menu rows, language picker,
//! Explorer integration, update check) and `shortcuts` (W8-10e: zoom summary, capture box). The module name `settings` is a NEW
//! directory module: it does not collide with the B1 files `presenters::settings_field_map` and `presenters::settings_form_state`.

pub mod apply;
pub mod load;
pub mod shortcuts;
pub mod validate;

pub use apply::{
    ContextMenuRow, SettingsExplorerRequest, SettingsImportResult, ShellIntegrationStatus, TranslationReloadResult,
    UpdateCheckCompleted, UpdateCheckDisplay, UpdateCheckJob, UpdateCheckOutcome,
};
pub use load::{SettingsPageInfo, SettingsPresenter, SettingsPresenterOptions, SharedLocalization};
