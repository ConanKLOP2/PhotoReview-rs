//! Port of `PhotoReview.Core/Catalog/ReviewCatalog.cs` lines 527-621 at 7eaee2a2.
use super::review_catalog::ReviewCatalog;
use crate::catalog::types::{CaptureGroup, CatalogEntry};
use crate::file_actions::rename::PathKeyMap;
use crate::text::{eq_ignore_case, ignore_case_key};
use std::collections::{BTreeMap, HashSet};

impl ReviewCatalog {
    /// Rename one path in place; no sort or index change. C# `RenameEntry`.
    pub fn rename_entry(&mut self, old_path: &str, new_path: &str) -> bool {
        let mut renames = PathKeyMap::new();
        renames.insert(old_path.to_owned(), new_path.to_owned());
        self.rename_entries(&renames)
    }

    /// Atomically rekey every listed path and capture-group member; C# `RenameEntries`.
    pub fn rename_entries(&mut self, renames: &PathKeyMap) -> bool {
        self.assert_owner_thread("RenameEntries");
        if renames.is_empty() || self.entries.is_empty() {
            return false;
        }

        // Validate the entire map before looking up any key. PathKeyMap has the same case-insensitive key behavior
        // as the C# dictionary while retaining insertion order for map traversal.
        if renames
            .iter()
            .any(|(from, to)| from.trim().is_empty() || to.trim().is_empty())
        {
            return false;
        }

        // IndexOf deliberately chooses the first entry for duplicate keys. Stage at most one replacement per public
        // catalog index; RenameInEntry sees the full map so every listed group member and sidecar travels together.
        let mut staged = BTreeMap::<usize, CatalogEntry>::new();
        for (from, _) in renames.iter() {
            let index = self.index_of(from);
            let Ok(index) = usize::try_from(index) else {
                continue;
            };
            if staged.contains_key(&index) {
                continue;
            }
            if let Some(entry) = self.entries.get(index) {
                if let Some(renamed) = rename_in_entry(entry, renames) {
                    staged.insert(index, renamed);
                }
            }
        }
        if staged.is_empty() {
            return false;
        }

        // Keep all names belonging to untouched entries. Existing duplicate keys are tolerated here, matching the
        // C# HashSet.Add behavior: only collisions introduced by staged entries reject the operation.
        let mut taken = HashSet::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if staged.contains_key(&index) {
                continue;
            }
            taken.insert(ignore_case_key(entry.path()));
            if let Some(group) = &entry.capture_group {
                for member in group.image_paths() {
                    taken.insert(ignore_case_key(member));
                }
            }
        }
        for renamed in staged.values() {
            if !taken.insert(ignore_case_key(renamed.path())) {
                return false;
            }
            if let Some(group) = &renamed.capture_group {
                for member in group.image_paths() {
                    if !eq_ignore_case(member, renamed.path()) && !taken.insert(ignore_case_key(member)) {
                        return false;
                    }
                }
            }
        }

        for (index, renamed) in staged {
            if let Some(entry) = self.entries.get_mut(index) {
                *entry = renamed;
            }
        }
        self.invalidate_index();
        true
    }
}

/// Returns the entry with every matching representative, group member, and sidecar respelled, or `None` when no
/// exact spelling changes. Lookup is case insensitive, but change detection matches C# ordinal exact string equality.
fn rename_in_entry(entry: &CatalogEntry, renames: &PathKeyMap) -> Option<CatalogEntry> {
    let path = renames
        .get(entry.path())
        .map_or_else(|| entry.path().to_owned(), Clone::clone);
    let group = entry.capture_group.as_ref();
    let new_group = group.map(|group| {
        let jpeg = renames
            .get(group.jpeg_path())
            .map_or_else(|| group.jpeg_path().to_owned(), Clone::clone);
        let raw = renames
            .get(group.raw_path())
            .map_or_else(|| group.raw_path().to_owned(), Clone::clone);
        let xmp = group
            .xmp_path()
            .map(|old| renames.get(old).map_or_else(|| old.to_owned(), Clone::clone));
        CaptureGroup::new(jpeg, raw, xmp).ok()
    });
    let new_group = match new_group {
        Some(Some(group)) => Some(group),
        Some(None) => return None,
        None => None,
    };

    let path_changed = path != entry.path();
    let group_changed = match (group, new_group.as_ref()) {
        (Some(old), Some(new)) => {
            old.jpeg_path() != new.jpeg_path() || old.raw_path() != new.raw_path() || old.xmp_path() != new.xmp_path()
        }
        _ => false,
    };
    if !path_changed && !group_changed {
        return None;
    }

    let mut renamed = entry.clone().with_path(path).ok()?;
    if group.is_some() {
        renamed.capture_group = new_group;
    }
    Some(renamed)
}
