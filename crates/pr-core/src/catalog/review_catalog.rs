//! Port of `PhotoReview.Core/Catalog/ReviewCatalog.cs` (C# baseline d92d6aae): the ordered list of image entries plus the
//! current review position. Every file action, undo, Explorer-order apply and unreadable-file removal goes through it
//! (INV-3, INV-5, INV-7, INV-9, PARITY I-17).
//!
//! Threading (ADR 0005 / docs/adr/R0003-concurrency.md): UI thread only. `ReviewCatalog` is `Send` but NOT `Sync`
//! (interior `RefCell`/`Cell` hold the lazily rebuilt path index and the paths cache so that lookups take `&self`, like
//! the C# getters); background consumers (the preload scheduler) receive an [`entries_snapshot`](ReviewCatalog::entries_snapshot)
//! taken on the UI thread, never a reference to the catalog. [`bind_to_current_thread`](ReviewCatalog::bind_to_current_thread)
//! reproduces the C# debug-only owner check: in builds with `debug_assertions` every mutator called from another thread
//! panics with the C# message; in release builds nothing is checked.
//!
//! The catalog does NOT use `GenerationClock`; stale-result dropping (INV-1/5/7) is done by its callers
//! (`FolderLoadCoordinator`, `FileActionController`) with `StructuralVersion` / generations they hold themselves.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread::ThreadId;

use crate::catalog::capture_groups;
use crate::catalog::types::{
    CaptureGroup, CatalogEntry, RAW_EXTENSIONS, SUPPORTED_EXTENSIONS, WEBP_HEIC_EXTENSIONS, extension_in, get_extension,
};
use crate::model::RawPairMode;
use crate::text::{eq_ignore_case, ignore_case_key};
use crate::time::DateTimeUtc;

/// `MaxRemovedBeforeRebuild`: after this many incremental removals the next lookup rebuilds the whole path index.
pub const MAX_REMOVED_BEFORE_REBUILD: usize = 64;

/// C# `ReviewCatalog`. Index-like values use `i32` with `-1` meaning "none" exactly like C# (`current_index`, `index_of`,
/// `remove`, `restore*`'s `index` parameter, which is clamped into `[0, count]` and may be negative); counts are `usize`.
#[derive(Debug)]
pub struct ReviewCatalog {
    // Shared with the sibling W9-R4 rename implementation; catalog mutation remains internal.
    pub(super) entries: Vec<CatalogEntry>,
    raw_pair_mode: RawPairMode,
    /// `None` = unbound (no owner check). C# `_ownerThreadId == 0`.
    owner_thread: Option<ThreadId>,
    /// `ignore_case_key(path)` -> position stored when the index was built (stale after incremental removals, see
    /// `removed_positions`). Holds the entry path AND, for a capture-group entry, each other image-member path.
    index_by_path: RefCell<HashMap<String, i32>>,
    index_dirty: Cell<bool>,
    /// Sorted positions (as stored in `index_by_path`) of entries removed since the last rebuild; the live index of a
    /// stored position is `stored - (number of removed positions below it)`.
    removed_positions: RefCell<Vec<i32>>,
    /// Two case-variant duplicate paths exist: a removal could unhide the second one, so removal must rebuild.
    index_has_duplicate_keys: Cell<bool>,
    index_rebuild_count: Cell<u32>,
    paths_cache: RefCell<Option<Arc<[String]>>>,
    structural_version: i32,
    current_index: i32,
}

impl Default for ReviewCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ReviewCatalog {
    /// `new ReviewCatalog()`: empty, `current_index == -1`, `structural_version == 0`, mode `Separate`, unbound,
    /// index dirty.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            raw_pair_mode: RawPairMode::Separate,
            owner_thread: None,
            index_by_path: RefCell::new(HashMap::new()),
            index_dirty: Cell::new(true),
            removed_positions: RefCell::new(Vec::new()),
            index_has_duplicate_keys: Cell::new(false),
            index_rebuild_count: Cell::new(0),
            paths_cache: RefCell::new(None),
            structural_version: 0,
            current_index: -1,
        }
    }

    /// `BindToCurrentThread()`: remembers `std::thread::current().id()` as the owner.
    pub fn bind_to_current_thread(&mut self) {
        self.owner_thread = Some(std::thread::current().id());
    }

    /// `AssertOwnerThread(member)` (`[Conditional("DEBUG")]`): in builds with `debug_assertions`, panics when the catalog
    /// is bound to another thread than the caller. Unbound or same thread: nothing. Reads never check.
    pub(super) fn assert_owner_thread(&self, member: &str) {
        if !cfg!(debug_assertions) {
            return;
        }
        if let Some(owner) = self.owner_thread {
            let current = std::thread::current().id();
            if owner != current {
                panic!(
                    "ReviewCatalog.{member} called on thread {current:?}; the catalog is bound to thread {owner:?} (UI thread only, ADR 0005)."
                );
            }
        }
    }

    /// `StructuralVersion`: bumped (wrapping `i32`) whenever membership or order changes (Reset, Remove, Restore,
    /// RestoreMembers that restores/folds, ReplaceOrder, RemovePaths that removes). NOT bumped by `set_current` or
    /// `update_metadata`, nor by a call that returns false/empty without changing anything.
    pub fn structural_version(&self) -> i32 {
        self.structural_version
    }

    /// `Count`.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// `CurrentIndex`: `-1` when empty.
    pub fn current_index(&self) -> i32 {
        self.current_index
    }

    /// `Current`: the entry at `current_index` or `None` (empty catalog, or index out of range).
    pub fn current(&self) -> Option<&CatalogEntry> {
        usize::try_from(self.current_index)
            .ok()
            .and_then(|index| self.entries.get(index))
    }

    /// `Paths`: the entry paths in order (a capture-group entry contributes only its own representative path). Cached in
    /// an `Arc<[String]>` that is rebuilt after any structural change; a previously returned `Arc` is a snapshot and
    /// is never mutated.
    pub fn paths(&self) -> Arc<[String]> {
        let mut cache = self.paths_cache.borrow_mut();
        if let Some(cached) = cache.as_ref() {
            return Arc::clone(cached);
        }
        let built: Arc<[String]> = self.entries.iter().map(|e| e.path().to_owned()).collect();
        *cache = Some(Arc::clone(&built));
        built
    }

    /// `PathAt(index)`: the path of entry `index`. Panics when out of range (C# `ArgumentOutOfRangeException`: a caller bug).
    pub fn path_at(&self, index: usize) -> &str {
        self.entries[index].path()
    }

    /// `IndexOf(path)`: index of the entry whose path, OR (capture group) whose other image-member path, equals `path`
    /// OrdinalIgnoreCase; the FIRST such entry wins; `-1` for blank or unknown paths and for paths of removed entries.
    pub fn index_of(&self, path: &str) -> i32 {
        if path.trim().is_empty() {
            return -1;
        }
        self.ensure_index();
        let stored = match self.index_by_path.borrow().get(&ignore_case_key(path)) {
            Some(&stored) => stored,
            None => return -1,
        };
        let removed = self.removed_positions.borrow();
        if removed.is_empty() {
            return stored;
        }
        match removed.binary_search(&stored) {
            Ok(_) => -1,
            // `Err(n)`: `n` removed positions lie below `stored`.
            Err(below) => stored - below as i32,
        }
    }

    /// `EnsureIndex()`: no-op when clean; else rebuild the path index (first occurrence wins).
    fn ensure_index(&self) {
        if !self.index_dirty.get() {
            return;
        }
        let mut map = self.index_by_path.borrow_mut();
        map.clear();
        self.removed_positions.borrow_mut().clear();
        let mut duplicates = false;
        self.index_rebuild_count.set(self.index_rebuild_count.get() + 1);
        for (i, entry) in self.entries.iter().enumerate() {
            let position = i as i32;
            if try_add(&mut map, entry.path(), position) {
                duplicates = true;
            }
            if let Some(group) = &entry.capture_group {
                for member in group.image_paths() {
                    if !eq_ignore_case(member, entry.path()) && try_add(&mut map, member, position) {
                        duplicates = true;
                    }
                }
            }
        }
        self.index_has_duplicate_keys.set(duplicates);
        self.index_dirty.set(false);
    }

    /// `InvalidateIndex()`: index dirty, paths cache dropped, `structural_version += 1` (wrapping).
    pub(super) fn invalidate_index(&mut self) {
        self.index_dirty.set(true);
        *self.paths_cache.borrow_mut() = None;
        self.structural_version = self.structural_version.wrapping_add(1);
    }

    /// `Reset(entries, rawPairMode, sidecarPaths)`: owner check; keep only entries with a non-blank path; group pairs;
    /// replace all entries; remember the mode; `current_index = 0` if non-empty else `-1`; `invalidate_index`.
    pub fn reset(&mut self, entries: Vec<CatalogEntry>, raw_pair_mode: RawPairMode, sidecar_paths: Option<&[String]>) {
        self.assert_owner_thread("Reset");
        let fresh: Vec<CatalogEntry> = entries.into_iter().filter(|e| !e.path().trim().is_empty()).collect();
        self.entries = capture_groups::group_entries(&fresh, raw_pair_mode, sidecar_paths);
        self.raw_pair_mode = raw_pair_mode;
        self.current_index = if self.entries.is_empty() { -1 } else { 0 };
        self.invalidate_index();
    }

    /// `Find(path)`: the entry `index_of(path)` points to.
    pub fn find(&self, path: &str) -> Option<&CatalogEntry> {
        usize::try_from(self.index_of(path))
            .ok()
            .and_then(|index| self.entries.get(index))
    }

    /// `UpdateMetadata(path, length, lastWriteUtc, width, height)`: false when the path is unknown or is only a
    /// capture-group member path (the stat belongs to ONE file); no version bump, no index change.
    pub fn update_metadata(
        &mut self,
        path: &str,
        length: i64,
        last_write_utc: DateTimeUtc,
        width: Option<i32>,
        height: Option<i32>,
    ) -> bool {
        self.assert_owner_thread("UpdateMetadata");
        let Ok(index) = usize::try_from(self.index_of(path)) else {
            return false;
        };
        if !eq_ignore_case(self.entries[index].path(), path) {
            return false;
        }
        self.entries[index] = self.entries[index].with_metadata(length, last_write_utc, width, height);
        true
    }

    /// `SetCurrent(index)`: empty catalog and `index == -1` => `-1`, true; `0 <= index < count` => set, true; else false.
    pub fn set_current(&mut self, index: i32) -> bool {
        self.assert_owner_thread("SetCurrent");
        if self.entries.is_empty() && index == -1 {
            self.current_index = -1;
            return true;
        }
        if index >= 0 && (index as usize) < self.entries.len() {
            self.current_index = index;
            return true;
        }
        false
    }

    /// `Remove(path)`: returns the new `current_index`. Incremental fast path keeps the index stale and records the
    /// stored position in the sorted `removed_positions`.
    pub fn remove(&mut self, path: &str) -> i32 {
        self.assert_owner_thread("Remove");
        let removed = self.index_of(path);
        let Ok(removed_at) = usize::try_from(removed) else {
            return self.current_index;
        };

        let stored = if !self.index_has_duplicate_keys.get()
            && self.removed_positions.borrow().len() < MAX_REMOVED_BEFORE_REBUILD
        {
            self.index_by_path.borrow().get(&ignore_case_key(path)).copied()
        } else {
            None
        };
        if let Some(stored) = stored {
            {
                let mut positions = self.removed_positions.borrow_mut();
                let at = positions.binary_search(&stored).unwrap_or_else(|e| e);
                positions.insert(at, stored);
            }
            self.entries.remove(removed_at);
            *self.paths_cache.borrow_mut() = None;
            self.structural_version = self.structural_version.wrapping_add(1);
        } else {
            self.entries.remove(removed_at);
            self.invalidate_index();
        }

        if self.entries.is_empty() {
            self.current_index = -1;
            return -1;
        }

        let current = self.current_index;
        if current >= 0 && removed < current {
            self.current_index = current - 1;
            return self.current_index;
        }
        if current >= 0 && removed > current {
            return current;
        }
        let next = removed.max(0).min(self.entries.len() as i32 - 1);
        self.current_index = next;
        next
    }

    /// `Restore(path, index)`: false for a blank path or an already listed one; else insert at `clamp(index, 0, count)`.
    pub fn restore(&mut self, path: &str, index: i32) -> bool {
        self.assert_owner_thread("Restore");
        if path.trim().is_empty() {
            return false;
        }
        if self.index_of(path) >= 0 {
            return false;
        }
        let Ok(entry) = CatalogEntry::new(path) else {
            return false;
        };
        let clamped = index.clamp(0, self.entries.len() as i32);
        self.entries.insert(clamped as usize, entry);
        self.invalidate_index();
        if self.current_index == -1 {
            self.current_index = clamped;
        } else if clamped <= self.current_index {
            self.current_index += 1;
        }
        true
    }

    /// `RestoreMembers(paths, index, group, rawEnabled)` (undo of a recycle/move of a plain photo or a whole capture).
    /// True when at least one entry was restored or re-formed (see contracts/W2-10.md).
    pub fn restore_members(
        &mut self,
        paths: &[String],
        index: i32,
        group: Option<&CaptureGroup>,
        raw_enabled: bool,
    ) -> bool {
        self.assert_owner_thread("RestoreMembers");
        let group = if raw_enabled { group } else { None };
        let mut seen_keys: HashSet<String> = HashSet::new();
        let mut all: Vec<String> = Vec::new();
        for path in paths {
            if path.trim().is_empty() {
                continue;
            }
            if seen_keys.insert(ignore_case_key(path)) {
                all.push(path.clone());
            }
        }
        let reformed = self.try_reform_group(&all, group);
        if reformed {
            if let Some(group) = group {
                all.retain(|path| !group.image_paths().iter().any(|member| eq_ignore_case(member, path)));
            }
        }
        let unique: Vec<String> = all
            .iter()
            .filter(|path| Self::is_restorable_image(path, raw_enabled))
            .filter(|path| self.index_of(path) < 0)
            .cloned()
            .collect();
        if unique.is_empty() {
            return reformed;
        }

        let mut groups: Vec<CaptureGroup> = Vec::new();
        if self.raw_pair_mode != RawPairMode::Separate {
            let present: HashSet<String> = unique.iter().map(|p| ignore_case_key(p)).collect();
            let mut explicit_added = false;
            if let Some(group) = group {
                if group
                    .image_paths()
                    .iter()
                    .all(|p| present.contains(&ignore_case_key(p)))
                {
                    groups.push(group.clone());
                    explicit_added = true;
                }
            }
            let rest: Vec<String> = unique
                .iter()
                .filter(|path| match group {
                    Some(group) if explicit_added => {
                        !group.image_paths().iter().any(|member| eq_ignore_case(member, path))
                    }
                    _ => true,
                })
                .cloned()
                .collect();
            let xmps: Vec<String> = all
                .iter()
                .filter(|path| eq_ignore_case(get_extension(path), ".xmp"))
                .cloned()
                .collect();
            groups.extend(capture_groups::build(&rest, Some(&xmps)));
        }

        let mut group_by_member: HashMap<String, usize> = HashMap::new();
        for (position, candidate) in groups.iter().enumerate() {
            group_by_member.insert(ignore_case_key(candidate.jpeg_path()), position);
            group_by_member.insert(ignore_case_key(candidate.raw_path()), position);
        }

        let mut emitted: HashSet<CaptureGroup> = HashSet::new();
        let mut restored: Vec<CatalogEntry> = Vec::with_capacity(unique.len());
        for path in &unique {
            match group_by_member.get(&ignore_case_key(path)) {
                None => {
                    if let Ok(entry) = CatalogEntry::new(path.as_str()) {
                        restored.push(entry);
                    }
                }
                Some(&owner_at) => {
                    let owner = &groups[owner_at];
                    if emitted.insert(owner.clone()) {
                        // Groups only exist in a pairing mode, so the representative lookup cannot fail.
                        if let Ok(representative) = owner.get_representative_path(self.raw_pair_mode) {
                            if let Ok(mut entry) = CatalogEntry::new(representative) {
                                entry.capture_group = Some(owner.clone());
                                restored.push(entry);
                            }
                        }
                    }
                }
            }
        }

        let insert_at = index.clamp(0, self.entries.len() as i32);
        let restored_count = restored.len() as i32;
        let tail = self.entries.split_off(insert_at as usize);
        self.entries.extend(restored);
        self.entries.extend(tail);
        self.invalidate_index();
        if self.current_index < 0 {
            self.current_index = insert_at;
        } else if insert_at <= self.current_index {
            self.current_index += restored_count;
        }
        true
    }

    /// `TryReformGroup(restoredPaths, group)`: folds a partially restored capture back into one grouped entry.
    fn try_reform_group(&mut self, restored_paths: &[String], group: Option<&CaptureGroup>) -> bool {
        let Some(group) = group else {
            return false;
        };
        if self.raw_pair_mode == RawPairMode::Separate {
            return false;
        }
        let mut standalone: Vec<usize> = Vec::new();
        for member in group.image_paths() {
            match usize::try_from(self.index_of(member)) {
                Ok(at) => {
                    if self.entries[at].capture_group.is_some() {
                        return false;
                    }
                    standalone.push(at);
                }
                Err(_) => {
                    let listed = restored_paths.iter().any(|p| eq_ignore_case(p, member));
                    if !listed || !Self::is_restorable_image(member, true) {
                        return false;
                    }
                }
            }
        }
        if standalone.is_empty() {
            return false;
        }

        standalone.sort_unstable();
        let slot = standalone[0];
        let Ok(representative) = group.get_representative_path(self.raw_pair_mode) else {
            return false;
        };
        let current_entry = usize::try_from(self.current_index)
            .ok()
            .filter(|c| standalone.contains(c))
            .map(|c| self.entries[c].clone());
        let keep_existing = current_entry.is_some();
        let existing = current_entry.unwrap_or_else(|| self.entries[slot].clone());
        let replacement = if keep_existing || eq_ignore_case(existing.path(), representative) {
            let mut kept = existing;
            kept.capture_group = Some(group.clone());
            kept
        } else {
            match CatalogEntry::new(representative) {
                Ok(mut fresh) => {
                    fresh.capture_group = Some(group.clone());
                    fresh
                }
                Err(_) => return false,
            }
        };
        self.entries[slot] = replacement;
        for i in (1..standalone.len()).rev() {
            let removed = standalone[i];
            self.entries.remove(removed);
            if self.current_index == removed as i32 {
                self.current_index = slot as i32;
            } else if self.current_index > removed as i32 {
                self.current_index -= 1;
            }
        }
        self.invalidate_index();
        true
    }

    /// `IsRestorableImage(path, rawEnabled)`: supported or WebP/HEIC extension, or RAW when `raw_enabled`.
    fn is_restorable_image(path: &str, raw_enabled: bool) -> bool {
        let extension = get_extension(path);
        extension_in(SUPPORTED_EXTENSIONS, extension)
            || extension_in(WEBP_HEIC_EXTENSIONS, extension)
            || (raw_enabled && extension_in(RAW_EXTENSIONS, extension))
    }

    /// `ReplaceOrder(newOrder)`: applies the order Explorer reported; false (nothing changes) unless it is a true
    /// permutation of the entry paths (after collapsing a member order in a pairing mode).
    pub fn replace_order(&mut self, new_order: &[String]) -> bool {
        self.assert_owner_thread("ReplaceOrder");
        let collapsed = if self.raw_pair_mode != RawPairMode::Separate {
            self.try_collapse_member_order(new_order)
        } else {
            None
        };
        let effective: &[String] = collapsed.as_deref().unwrap_or(new_order);
        if effective.len() != self.entries.len() {
            return false;
        }

        let current_set: HashSet<String> = self.entries.iter().map(|e| ignore_case_key(e.path())).collect();
        let mut seen: HashSet<String> = HashSet::new();
        for path in effective {
            let key = ignore_case_key(path);
            if !current_set.contains(&key) || !seen.insert(key) {
                return false;
            }
        }

        let current_path = self.current().map(|e| e.path().to_owned());

        // First wins; only reached for a true permutation.
        let mut entry_map: HashMap<String, usize> = HashMap::new();
        for (i, entry) in self.entries.iter().enumerate() {
            entry_map.entry(ignore_case_key(entry.path())).or_insert(i);
        }
        let mut reordered = Vec::with_capacity(self.entries.len());
        for path in effective {
            match entry_map.get(&ignore_case_key(path)) {
                Some(&i) => reordered.push(self.entries[i].clone()),
                None => return false,
            }
        }
        self.entries = reordered;
        self.invalidate_index();

        let fallback = if self.entries.is_empty() { -1 } else { 0 };
        self.current_index = match current_path {
            Some(path) => {
                let new_index = self.index_of(&path);
                if new_index >= 0 { new_index } else { fallback }
            }
            None => fallback,
        };
        true
    }

    /// `TryCollapseMemberOrder(memberOrder, out collapsedOrder)`: a list naming every image member collapses to the
    /// representatives in first-seen order.
    fn try_collapse_member_order(&self, member_order: &[String]) -> Option<Vec<String>> {
        let mut representative_by_member: HashMap<String, &str> = HashMap::new();
        let mut expected: HashSet<String> = HashSet::new();
        for entry in &self.entries {
            match &entry.capture_group {
                Some(group) => {
                    for path in group.image_paths() {
                        expected.insert(ignore_case_key(path));
                        representative_by_member
                            .entry(ignore_case_key(path))
                            .or_insert_with(|| entry.path());
                    }
                }
                None => {
                    expected.insert(ignore_case_key(entry.path()));
                    representative_by_member
                        .entry(ignore_case_key(entry.path()))
                        .or_insert_with(|| entry.path());
                }
            }
        }

        if member_order.len() != expected.len() {
            return None;
        }
        let mut seen_members: HashSet<String> = HashSet::new();
        let mut seen_representatives: HashSet<String> = HashSet::new();
        let mut result: Vec<String> = Vec::with_capacity(self.entries.len());
        for path in member_order {
            let key = ignore_case_key(path);
            if !expected.contains(&key) || !seen_members.insert(key.clone()) {
                return None;
            }
            let representative = representative_by_member.get(&key)?;
            if seen_representatives.insert(ignore_case_key(representative)) {
                result.push((*representative).to_owned());
            }
        }
        if result.len() != self.entries.len() {
            return None;
        }
        Some(result)
    }

    /// `RemovePaths(paths)` (AR16): removes unreadable files, returns the paths actually removed in catalog order.
    pub fn remove_paths(&mut self, paths: &[String]) -> Vec<String> {
        self.assert_owner_thread("RemovePaths");
        let doomed: HashSet<String> = paths
            .iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| ignore_case_key(p))
            .collect();
        if doomed.is_empty() || self.entries.is_empty() {
            return Vec::new();
        }
        let is_doomed = |p: &str| doomed.contains(&ignore_case_key(p));

        let mut removed: Vec<String> = Vec::new();
        let mut kept: Vec<CatalogEntry> = Vec::with_capacity(self.entries.len());
        let old_current = self.current_index;
        let mut next_current: i32 = -1;
        for (i, original) in self.entries.iter().enumerate() {
            let mut entry = original.clone();
            let mut entry_doomed = is_doomed(entry.path());
            if let Some(group) = &original.capture_group {
                let members = group.image_paths();
                let bad: Vec<&String> = members.iter().filter(|p| is_doomed(p)).collect();
                if bad.len() == members.len() {
                    removed.push(entry.path().to_owned());
                    removed.extend(
                        bad.iter()
                            .filter(|p| !eq_ignore_case(p, entry.path()))
                            .map(|p| (*p).clone()),
                    );
                    continue;
                }
                if !bad.is_empty() {
                    // Degrade in place: the readable partner stays reachable as a standalone entry.
                    let survivor = members.iter().find(|p| !is_doomed(p));
                    removed.extend(bad.iter().map(|p| (*p).clone()));
                    if let Some(survivor) = survivor {
                        if eq_ignore_case(entry.path(), survivor) {
                            entry.capture_group = None;
                        } else if let Ok(fresh) = CatalogEntry::new(survivor.as_str()) {
                            entry = fresh;
                        } else {
                            entry.capture_group = None;
                        }
                    }
                    entry_doomed = false;
                }
            }

            if entry_doomed {
                removed.push(entry.path().to_owned());
                continue;
            }
            if next_current < 0 && old_current >= 0 && i as i32 >= old_current {
                next_current = kept.len() as i32;
            }
            kept.push(entry);
        }

        if removed.is_empty() {
            return Vec::new();
        }
        self.entries = kept;
        self.invalidate_index();
        self.current_index = if self.entries.is_empty() {
            -1
        } else if old_current < 0 {
            0
        } else if next_current >= 0 {
            next_current
        } else {
            self.entries.len() as i32 - 1
        };
        removed
    }

    /// `EntriesSnapshot()`: a clone of every entry (metadata included) for consumers off the UI thread.
    pub fn entries_snapshot(&self) -> Vec<CatalogEntry> {
        self.entries.clone()
    }

    /// `IndexRebuildCountForTests`: how many times the path index was rebuilt from scratch.
    #[doc(hidden)]
    pub fn index_rebuild_count_for_tests(&self) -> u32 {
        self.index_rebuild_count.get()
    }
}

/// `Dictionary.TryAdd` with the OrdinalIgnoreCase key: returns true when the key ALREADY existed (nothing added).
fn try_add(map: &mut HashMap<String, i32>, path: &str, position: i32) -> bool {
    match map.entry(ignore_case_key(path)) {
        std::collections::hash_map::Entry::Occupied(_) => true,
        std::collections::hash_map::Entry::Vacant(slot) => {
            slot.insert(position);
            false
        }
    }
}
