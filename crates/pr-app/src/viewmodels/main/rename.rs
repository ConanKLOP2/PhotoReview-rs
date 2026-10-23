//! Port of `MainViewModel` FEAT-RENAME additions at C# 7eaee2a2.
#![allow(unused_variables, dead_code)] // Stub-only: remove after Wave 9 implementation.
use super::state::{ChangeSet, MainViewModel};
use std::collections::HashMap;

impl MainViewModel {
    /// Queue the current photo or capture group for rename; C# `RenameCurrentAsync`.
    pub fn rename_current(&mut self, new_name: &str) -> ChangeSet {
        todo!()
    }
    /// Re-key session, skipped paths, cache and presenter after disk rename.
    pub fn on_files_renamed(&mut self, renames: &HashMap<String, String>) -> ChangeSet {
        todo!()
    }
}
