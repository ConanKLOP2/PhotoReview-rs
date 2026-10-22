//! Settings UI (Wave 8 B5; port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi`). The submodule list is appended to by each unit group (one line per group, in the
//! order the groups are merged); a group never edits another group's lines.

// W8-19a..19c (contract group G7): page contract, catalog, context, page builder.
pub mod builder;
pub mod catalog;
pub mod context;
pub mod page_view;

// W8-19d..19i (contract group G8): the seven Settings pages.
pub mod pages;

// W8-19j..19m (contract group G9): the Settings window (frame, state, apply, layout).
pub mod dialog;
