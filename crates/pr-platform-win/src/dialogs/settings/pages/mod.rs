//! The seven Settings pages (Wave 8 B5, units W8-19d..19i; port of `PhotoReview.Shell.Win32/Dialogs/SettingsUi/Pages`). One file per page, each with one
//! `SettingsPageView` implementation that `settings::catalog::FACTORIES` lists. The list below is final (lead-owned, alphabetical like the catalog).

pub mod diagnostics;
pub mod display;
pub mod files;
pub mod general;
pub mod mouse;
pub mod performance;
pub mod shortcuts;
