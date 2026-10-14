//! Port of `IDialogService.cs`. Implemented by `pr-app`; core code only asks through this seam.

use std::path::{Path, PathBuf};

use super::SkippedEntry;

/// One line of the batch-review dialog: the file and its size in bytes when known (`None` = unknown or gone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReviewItem {
    pub path: PathBuf,
    pub length: Option<u64>,
}

/// Which part of the Settings dialog to open on (C# `SettingsTarget`, d92d6aae).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTarget {
    /// The last page shown (default behaviour).
    #[default]
    Default,
    /// The Files page with the External Editor path field focused.
    ExternalEditor,
}

/// Confirmation prompts and secondary windows.
pub trait DialogService {
    /// Yes/No or OK/Cancel question; `true` = confirmed.
    fn show_confirmation(&self, title: &str, message: &str) -> bool;
    fn show_message(&self, title: &str, message: &str);
    fn show_error(&self, title: &str, message: &str);
    fn pick_folder(&self, initial_folder: Option<&Path>) -> Option<PathBuf>;
    /// RV-A13: sizes the caller already knows, so the dialog never stats files on the UI thread. (C# also has a
    /// path-only overload that forwards here with unknown sizes.)
    fn show_batch_review(&self, items: &[BatchReviewItem]) -> bool;
    fn show_recovery(&self);
    fn show_diagnostics(&self);
    /// `true` when the user saved changes.
    fn show_settings(&self) -> bool;
    /// Same dialog opened on the page/field for `target` (C# d92d6aae). Default ignores the target.
    fn show_settings_at(&self, target: SettingsTarget) -> bool {
        let _ = target;
        self.show_settings()
    }
    fn show_benchmark(&self, folder: Option<&Path>);
    /// Files skipped while loading a folder (AR19).
    fn show_skipped_files(&self, entries: &[SkippedEntry]);
}
