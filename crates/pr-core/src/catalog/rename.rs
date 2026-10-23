//! Port of `PhotoReview.Core/Catalog/ReviewCatalog.cs` lines 527-621 at 7eaee2a2.
#![allow(unused_variables, dead_code)] // Stub-only: remove after the Wave 9 implementation.
use super::review_catalog::ReviewCatalog;
use std::collections::HashMap;

impl ReviewCatalog {
    /// Rename one path in place; no sort or index change. C# `RenameEntry`.
    pub fn rename_entry(&mut self, old_path: &str, new_path: &str) -> bool {
        todo!()
    }
    /// Atomically rekey every listed path and capture-group member; C# `RenameEntries`.
    pub fn rename_entries(&mut self, renames: &HashMap<String, String>) -> bool {
        todo!()
    }
}
