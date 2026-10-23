//! Port of `PhotoReview.Core/FileActions/CaseOnlyRename.cs` at 7eaee2a2 (W9-R2).
#![allow(unused_variables, dead_code)] // Stub-only: Wave 9 porters remove this with the last todo.

use crate::abstractions::FileSystem;

/// Stored spelling checks for a case-only same-folder rename.
#[derive(Debug)]
pub struct CaseOnlyRename;
impl CaseOnlyRename {
    /// C# `IsCaseOnlyPair`.
    pub fn is_case_only_pair(first: Option<&str>, second: Option<&str>) -> bool {
        todo!()
    }
    /// C# `StatAs`: stat only when the exact spelling is stored; inspection errors stay errors.
    pub fn stat_as(
        fs: &dyn FileSystem,
        path: &str,
        twin: Option<&str>,
    ) -> std::io::Result<Option<crate::abstractions::FileStat>> {
        todo!()
    }
    /// C# `ExistsAs`: exact stored spelling for case-only pairs.
    pub fn exists_as(fs: &dyn FileSystem, path: &str, twin: Option<&str>) -> std::io::Result<bool> {
        todo!()
    }
    /// C# `ProbeTarget`; `(conflict, already_named)`.
    pub fn probe_target(fs: &dyn FileSystem, source: &str, destination: &str) -> std::io::Result<(bool, bool)> {
        todo!()
    }
}
