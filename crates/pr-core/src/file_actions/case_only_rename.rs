//! Port of `PhotoReview.Core/FileActions/CaseOnlyRename.cs` at 7eaee2a2 (W9-R2).
use std::io;
use std::path::Path;

use crate::abstractions::{FileStat, FileSystem};
use crate::text::eq_ignore_case;

/// Stored spelling checks for a case-only same-folder rename.
#[derive(Debug)]
pub struct CaseOnlyRename;

impl CaseOnlyRename {
    /// C# `IsCaseOnlyPair`.
    pub fn is_case_only_pair(first: Option<&str>, second: Option<&str>) -> bool {
        let (Some(first), Some(second)) = (first, second) else {
            return false;
        };
        !first.trim().is_empty() && !second.trim().is_empty() && first != second && eq_ignore_case(first, second)
    }

    /// C# `StatAs`: stat only when the exact spelling is stored; inspection errors stay errors.
    pub fn stat_as(fs: &dyn FileSystem, path: &str, twin: Option<&str>) -> io::Result<Option<FileStat>> {
        let stat = fs.try_get_file_stat(Path::new(path))?;
        if stat.is_none() || !Self::is_case_only_pair(Some(path), twin) {
            return Ok(stat);
        }
        if Self::is_stored_as(fs, path, twin.unwrap_or_default())? {
            Ok(stat)
        } else {
            Ok(None)
        }
    }

    /// C# `ExistsAs`: exact stored spelling for case-only pairs.
    pub fn exists_as(fs: &dyn FileSystem, path: &str, twin: Option<&str>) -> io::Result<bool> {
        if !fs.file_exists(Path::new(path)) {
            return Ok(false);
        }
        if !Self::is_case_only_pair(Some(path), twin) {
            return Ok(true);
        }
        Self::is_stored_as(fs, path, twin.unwrap_or_default())
    }

    /// C# `ProbeTarget`; `(conflict, already_named)`.
    pub fn probe_target(fs: &dyn FileSystem, source: &str, destination: &str) -> io::Result<(bool, bool)> {
        if source == destination {
            return Ok((false, true));
        }
        if !Self::is_case_only_pair(Some(source), Some(destination)) {
            return Ok((
                fs.file_exists(Path::new(destination)) || fs.directory_exists(Path::new(destination)),
                false,
            ));
        }
        let stored = fs.get_stored_file_names(Path::new(destination))?;
        if stored.len() > 1 {
            return Ok((true, false));
        }
        let target_name = Path::new(destination)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stored.first().is_some_and(|name| name == target_name) {
            return Ok((false, true));
        }
        Ok((false, false))
    }

    fn is_stored_as(fs: &dyn FileSystem, path: &str, twin: &str) -> io::Result<bool> {
        let stored = fs.get_stored_file_names(Path::new(path))?;
        if stored.is_empty() {
            return Ok(false);
        }
        if stored.len() > 1 {
            return Ok(true);
        }
        let mine = Path::new(path).file_name().and_then(|s| s.to_str()).unwrap_or_default();
        let other = Path::new(twin).file_name().and_then(|s| s.to_str()).unwrap_or_default();
        Ok(stored[0] == mine || stored[0] != other)
    }
}
