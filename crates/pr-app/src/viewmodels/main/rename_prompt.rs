//! Port of `MainViewModel.PromptRenameCurrentAsync` at C# 7eaee2a2 (lines 599-627), W9-R7.
use super::state::{ChangeSet, MainViewModel};
pub use crate::presenters::rename_prompt::PreparedRenamePrompt;
use crate::presenters::rename_prompt::RenameCheck;
use pr_core::file_actions::rename::{CaptureGroupRenamePlan, RenameNamePolicy};
use pr_core::text::eq_ignore_case;
use std::sync::Arc;

impl MainViewModel {
    /// Ask for a stem, then rename only if the same photo remains selected; C# `PromptRenameCurrentAsync`.
    /// The modal box is opened with a validate closure that owns copies of what it needs (catalog lookup data, folder), never
    /// a borrow of the view model; the group plan (`CaptureGroupRenamePlan`) validates a capture, the policy a single file.
    pub fn prompt_rename_current(&mut self) -> ChangeSet {
        if self.catalog.count() == 0 {
            self.pending_rename_prompt = None;
            return ChangeSet::default();
        }
        let Some(source) = self
            .presenter
            .compare()
            .selected_path()
            .or_else(|| self.catalog.current().map(|entry| entry.path()))
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
        else {
            return ChangeSet::default();
        };
        let policy_path = source.clone();
        let group = self.catalog.find(&source).and_then(|entry| entry.capture_group.clone());
        let check: RenameCheck = Arc::new(move |typed| match &group {
            Some(group) => CaptureGroupRenamePlan::build(group, Some(typed), false).check,
            None => RenameNamePolicy::check(Some(typed), &policy_path, false),
        });
        self.pending_rename_prompt = Some(PreparedRenamePrompt::new(source, check));
        ChangeSet::default()
    }

    /// Transfer the queued plan to the shell; the caller must release its view-model borrow before showing a modal.
    pub fn take_rename_prompt(&mut self) -> Option<PreparedRenamePrompt> {
        self.pending_rename_prompt.take()
    }

    /// Finish a modal prompt only if its original selected path is still current.
    pub fn finish_rename_prompt(&mut self, source: &str, typed: Option<&str>) -> ChangeSet {
        self.pending_rename_prompt = None;
        let Some(current) = self
            .presenter
            .compare()
            .selected_path()
            .or_else(|| self.catalog.current().map(|entry| entry.path()))
        else {
            return ChangeSet::default();
        };
        if !eq_ignore_case(current, source) {
            return ChangeSet::default();
        }
        let Some(typed) = typed else {
            return ChangeSet::default();
        };
        self.rename_current(typed)
    }
}
