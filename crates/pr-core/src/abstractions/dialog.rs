//! Port of `IDialogService.cs`. Implemented by `pr-app`; core code only asks through this seam.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::SkippedEntry;
use crate::settings::external_editors::ExternalEditor;

/// Validation callback for a rename prompt; `is_commit` permits one disk probe.
pub type RenameValidateFn = Arc<dyn Fn(&str, bool) -> Option<String>>;

/// FEAT-RENAME prompt data: shown stem, fixed extension, and a cheap validation callback.
pub struct RenamePromptRequest {
    pub initial_name: String,
    pub extension: String,
    /// Returns localized refusal text, or None when valid; `is_commit` permits one disk probe.
    pub validate: RenameValidateFn,
}

impl std::fmt::Debug for RenamePromptRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenamePromptRequest")
            .field("initial_name", &self.initial_name)
            .field("extension", &self.extension)
            .finish_non_exhaustive()
    }
}

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
    /// C# `PromptRename`: None means cancel or unsupported port (the WPF fallback).
    fn prompt_rename(&self, request: &RenamePromptRequest) -> Option<String> {
        let _ = request;
        None
    }
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

    /// W9-E1, C# `PromptAddExternalEditor(string? initialFolder = null)` ("Add more editors..."): asks for an editor program (file picker) and the menu
    /// name; `None` when cancelled. Default (C# default interface method, the frozen WPF shell): `None`. The returned editor has the picked `path` and the
    /// typed `name` (`id` and `arguments` are the `ExternalEditor::new()` values; the caller assigns a fresh id).
    fn prompt_add_external_editor(&self, _initial_folder: Option<&Path>) -> Option<ExternalEditor> {
        None
    }

    /// W9-E1, C# `PromptDefaultExternalEditor(editors, currentId)` ("Set default editor..."): a choice list; the chosen editor's id, `None` when cancelled.
    /// Default: `None`.
    fn prompt_default_external_editor(&self, _editors: &[ExternalEditor], _current_id: Option<&str>) -> Option<String> {
        None
    }
}
