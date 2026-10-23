//! Port of `PhotoReview.App.Shared/Presenters/RenamePromptPresenter.cs` at 7eaee2a2.
#![allow(unused_variables, dead_code)] // Stub-only: remove with the last todo.

use pr_core::abstractions::RenamePromptRequest;
use pr_core::file_actions::rename::RenameNameCheck;
use std::sync::Arc;

/// Pure policy callback chosen for a single file or a capture group.
pub type RenameCheck = Arc<dyn Fn(&str) -> RenameNameCheck>;
/// Whether another file owns the complete new name; commit may inspect disk.
pub type NameTaken = Arc<dyn Fn(&str, bool) -> bool>;

/// Pure validation state of the rename prompt; the dialog only renders this state.
pub struct RenamePromptPresenter {
    /// Current source path (including original extension).
    pub source: String,
    /// The check chosen by the caller.
    pub check: RenameCheck,
    /// Optional catalog/disk collision question.
    pub name_taken: Option<NameTaken>,
}

impl std::fmt::Debug for RenamePromptPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenamePromptPresenter")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl RenamePromptPresenter {
    /// Construct the prompt for the currently selected path.
    pub fn new(source: String, check: RenameCheck, name_taken: Option<NameTaken>) -> Self {
        todo!()
    }
    /// C# `ToRequest`.
    pub fn to_request(&self) -> RenamePromptRequest {
        todo!()
    }
    /// C# `TryAccept`: localized refusal text, or None when valid.
    pub fn try_accept(&self, typed_stem: Option<&str>, is_commit: bool) -> Option<String> {
        todo!()
    }
}
