//! Port of `MainViewModel.PromptRenameCurrentAsync` at C# 7eaee2a2 (lines 599-627), W9-R7.
#![allow(unused_variables, dead_code)] // Stub-only: remove after Wave 9 implementation.
use super::state::{ChangeSet, MainViewModel};

impl MainViewModel {
    /// Ask for a stem, then rename only if the same photo remains selected; C# `PromptRenameCurrentAsync`.
    /// The modal box is opened with a validate closure that owns copies of what it needs (catalog lookup data, folder), never
    /// a borrow of the view model; the group plan (`CaptureGroupRenamePlan`) validates a capture, the policy a single file.
    pub fn prompt_rename_current(&mut self) -> ChangeSet {
        todo!()
    }
}
