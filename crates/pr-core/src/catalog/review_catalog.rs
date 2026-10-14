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
// Temporary (D11): bodies are `todo!()`; the porter removes this line with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread::ThreadId;

use crate::catalog::types::{CaptureGroup, CatalogEntry};
use crate::model::RawPairMode;
use crate::time::DateTimeUtc;

/// `MaxRemovedBeforeRebuild`: after this many incremental removals the next lookup rebuilds the whole path index.
pub const MAX_REMOVED_BEFORE_REBUILD: usize = 64;

/// C# `ReviewCatalog`. Index-like values use `i32` with `-1` meaning "none" exactly like C# (`current_index`, `index_of`,
/// `remove`, `restore*`'s `index` parameter, which is clamped into `[0, count]` and may be negative); counts are `usize`.
#[derive(Debug)]
pub struct ReviewCatalog {
    entries: Vec<CatalogEntry>,
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
        todo!()
    }

    /// `AssertOwnerThread(member)` (`[Conditional("DEBUG")]`): when `cfg!(debug_assertions)` and the catalog is bound to
    /// another thread than the caller, panic with exactly
    /// `"ReviewCatalog.{member} called on thread {current:?}; the catalog is bound to thread {owner:?} (UI thread only, ADR 0005)."`
    /// where `member` is the C# method name (`"Reset"`, `"SetCurrent"`, ...). Unbound or same thread: nothing. Reads never
    /// check. Every mutator calls it first with its C# name.
    fn assert_owner_thread(&self, member: &str) {
        todo!()
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
        todo!()
    }

    /// `Paths`: the entry paths in order (a capture-group entry contributes only its own representative path). Cached in
    /// an `Arc<[String]>` that is rebuilt after any structural change; a previously returned `Arc` is a snapshot and
    /// is never mutated.
    pub fn paths(&self) -> Arc<[String]> {
        todo!()
    }

    /// `PathAt(index)`: the path of entry `index`. Panics when out of range (C# `ArgumentOutOfRangeException`: a caller bug).
    pub fn path_at(&self, index: usize) -> &str {
        todo!()
    }

    /// `IndexOf(path)`: index of the entry whose path, OR (capture group) whose other image-member path, equals `path`
    /// OrdinalIgnoreCase; the FIRST such entry wins; `-1` for blank (`trim().is_empty()`) or unknown paths and for
    /// paths of removed entries. Rebuilds the index lazily when dirty (counts in `index_rebuild_count_for_tests`).
    /// With `removed_positions` empty the stored position is returned; otherwise `-1` if the stored position is itself in
    /// `removed_positions`, else `stored - (count of removed positions below stored)`.
    pub fn index_of(&self, path: &str) -> i32 {
        todo!()
    }

    /// `EnsureIndex()`: no-op when clean; else clear the map and `removed_positions`, clear
    /// `index_has_duplicate_keys`, bump the rebuild counter, then for each entry `i`: `try_add(key(entry.path), i)` (FIRST
    /// occurrence wins; a failed add sets `index_has_duplicate_keys`), and for a capture-group entry also
    /// `try_add(key(member), i)` for each `image_paths` member that is not the entry's own path (OrdinalIgnoreCase;
    /// a failed add sets the duplicate flag too). Finally mark clean.
    fn ensure_index(&self) {
        todo!()
    }

    /// `InvalidateIndex()`: index dirty, paths cache dropped, `structural_version += 1` (wrapping).
    fn invalidate_index(&mut self) {
        todo!()
    }

    /// `Reset(entries, rawPairMode, sidecarPaths)`: owner check; keep only entries with a non-blank path (always true for a
    /// valid `CatalogEntry`, keep the filter for parity); `capture_groups::group_entries(&fresh, raw_pair_mode, sidecar_paths)`;
    /// replace all entries; remember the mode; `current_index = 0` if non-empty else `-1`; `invalidate_index`.
    /// Calling it with `Separate` and no sidecars is the "plain paths" reset used by tests and the unpaired load.
    pub fn reset(&mut self, entries: Vec<CatalogEntry>, raw_pair_mode: RawPairMode, sidecar_paths: Option<&[String]>) {
        todo!()
    }

    /// `Find(path)`: the entry `index_of(path)` points to.
    pub fn find(&self, path: &str) -> Option<&CatalogEntry> {
        todo!()
    }

    /// `UpdateMetadata(path, length, lastWriteUtc, width, height)`: owner check; false when `index_of(path) < 0`, and ALSO
    /// false (nothing changes) when the entry found is not the entry whose own path equals `path` (OrdinalIgnoreCase): a
    /// capture-group entry found through its other member must not get that member's stat. Otherwise replace the entry
    /// by `entry.with_metadata(length, last_write_utc, width, height)` and return true. No version bump, no index change.
    pub fn update_metadata(
        &mut self,
        path: &str,
        length: i64,
        last_write_utc: DateTimeUtc,
        width: Option<i32>,
        height: Option<i32>,
    ) -> bool {
        todo!()
    }

    /// `SetCurrent(index)`: owner check; empty catalog and `index == -1` => `current_index = -1`, true; `0 <= index < count`
    /// => set, true; else false and unchanged.
    pub fn set_current(&mut self, index: i32) -> bool {
        todo!()
    }

    /// `Remove(path)`: owner check; returns the new `current_index`.
    /// * `removed = index_of(path)`; `< 0` => return the unchanged `current_index`.
    /// * Fast path (when `!index_has_duplicate_keys && removed_positions.len() < MAX_REMOVED_BEFORE_REBUILD` and the map has
    ///   `key(path)`): insert that stored position into the sorted `removed_positions`, remove the entry, drop the paths
    ///   cache, `structural_version += 1`; the index stays clean. Otherwise remove the entry and `invalidate_index`.
    /// * Now empty => `current_index = -1`, return -1.
    /// * `current >= 0 && removed < current` => `current_index = current - 1`, return it; `current >= 0 && removed > current`
    ///   => unchanged; otherwise (removed the current entry, or there was none) `next = min(max(removed, 0), count - 1)`,
    ///   set and return it (the entry that took its place, or the last one).
    pub fn remove(&mut self, path: &str) -> i32 {
        todo!()
    }

    /// `Restore(path, index)`: owner check; false for a blank path or when `index_of(path) >= 0` (no duplicates). Else insert
    /// `CatalogEntry::new(path)` at `clamp(index, 0, count)`, `invalidate_index`; `current_index == -1` => becomes the
    /// inserted index; else if `inserted <= current_index` => `current_index += 1`. True.
    pub fn restore(&mut self, path: &str, index: i32) -> bool {
        todo!()
    }

    /// `RestoreMembers(paths, index, group, rawEnabled)` (undo of a recycle/move of a plain photo or a whole capture). True
    /// when at least one entry was restored or re-formed. Steps (see contracts/W2-10.md for the line-by-line description):
    /// owner check; `group = None` when `!raw_enabled`; `all` = non-blank `paths`, distinct OrdinalIgnoreCase, input order;
    /// `reformed = try_reform_group(&all, group)`; when reformed drop the group's image members from `all`;
    /// `unique` = `all` filtered by `is_restorable_image(p, raw_enabled)` and `index_of(p) < 0`; empty => return `reformed`;
    /// in a pairing mode build the groups (the explicit `group` when BOTH its image members are in `unique`, plus
    /// `capture_groups::build(rest, xmp paths of all)`); emit entries in `unique` order (a group's single entry, with
    /// `get_representative_path(mode)`, at the position of its first member; non-members as plain `CatalogEntry::new(path)`);
    /// insert at `clamp(index, 0, count)`; `invalidate_index`; `current_index < 0` => `insert_at`, else if
    /// `insert_at <= current_index` => `current_index += restored.len()`.
    pub fn restore_members(
        &mut self,
        paths: &[String],
        index: i32,
        group: Option<&CaptureGroup>,
        raw_enabled: bool,
    ) -> bool {
        todo!()
    }

    /// `TryReformGroup(restoredPaths, group)`: folds a partially restored capture back into one grouped entry. Returns false
    /// (nothing changed) when `group` is `None`, the mode is `Separate`, an image member that is already listed belongs to an
    /// entry that already has a group, a member that is NOT listed is not in `restored_paths` (OrdinalIgnoreCase) or is not
    /// restorable (`is_restorable_image(member, true)`), or no member is listed. Otherwise (details in the contract) the
    /// lowest-indexed standalone entry becomes the grouped entry (keeping the displayed member if the current entry is one of
    /// them), the other standalone entry is removed, `current_index` follows, `invalidate_index`, true.
    fn try_reform_group(&mut self, restored_paths: &[String], group: Option<&CaptureGroup>) -> bool {
        todo!()
    }

    /// `IsRestorableImage(path, rawEnabled)`: `get_extension(path)` is in `SUPPORTED_EXTENSIONS` or `WEBP_HEIC_EXTENSIONS`
    /// (whatever the WebP/HEIC switch says), or `raw_enabled` and it is in `RAW_EXTENSIONS` (all OrdinalIgnoreCase).
    /// `.xmp`, `.avif`, `.txt` are not restorable.
    fn is_restorable_image(path: &str, raw_enabled: bool) -> bool {
        todo!()
    }

    /// `ReplaceOrder(newOrder)`: owner check; applies the order Explorer reported. In a pairing mode first try
    /// `try_collapse_member_order` (a list naming every image MEMBER, jpeg and raw, collapses to the representatives in
    /// first-seen order). Returns false and changes nothing unless the (collapsed) order is a true permutation of the entry
    /// paths: same count, every element known (OrdinalIgnoreCase), no repeats. On success rebuild `entries` in that order,
    /// `invalidate_index`, then keep the CURRENT entry by path: `current_index = index_of(old current path)` (0 if lost / empty),
    /// or `0`/`-1` when there was no current entry.
    pub fn replace_order(&mut self, new_order: &[String]) -> bool {
        todo!()
    }

    /// `TryCollapseMemberOrder(memberOrder, out collapsedOrder)`: every entry contributes its `capture_group.image_paths()` (or
    /// just its path); `expected_members` (OrdinalIgnoreCase set) and `representative_by_member` (first wins) are built; the
    /// order must have exactly `expected_members.len()` elements, each known and not repeated; the result lists each
    /// representative once, at the position of its first member seen; it must have exactly `entries.len()` items.
    fn try_collapse_member_order(&self, member_order: &[String]) -> Option<Vec<String>> {
        todo!()
    }

    /// `RemovePaths(paths)` (AR16: the background readability probe found these files unreadable): owner check; returns the
    /// paths actually removed, in catalog order. `doomed` = non-blank `paths` (OrdinalIgnoreCase set); `doomed` or the catalog
    /// empty => empty result. Per entry: a capture-group entry with ALL image members doomed is removed (reported: its own
    /// path, then the other doomed members); with ONE doomed member it degrades in place to the surviving member as a
    /// standalone entry (kept as `entry` with `capture_group = None` when the survivor is the entry's own path, else a NEW
    /// `CatalogEntry::new(survivor)` without metadata; the doomed members are reported); a plain doomed entry is removed
    /// (reported). Nothing removed/degraded => empty result and NOTHING changes (no version bump). Otherwise replace the
    /// entries, `invalidate_index`, and `current_index` = `-1` when empty; the new position of the first surviving/degraded
    /// entry at or after the old current position (so a removed current advances like `remove`, and a kept current stays
    /// on the same photo at its new index); the last entry when none follows; `0` when there was no current.
    pub fn remove_paths(&mut self, paths: &[String]) -> Vec<String> {
        todo!()
    }

    /// `EntriesSnapshot()`: a clone of every entry (metadata included) for consumers off the UI thread.
    pub fn entries_snapshot(&self) -> Vec<CatalogEntry> {
        todo!()
    }

    /// `IndexRebuildCountForTests`: how many times the path index was rebuilt from scratch (public only because
    /// integration tests live outside the crate).
    #[doc(hidden)]
    pub fn index_rebuild_count_for_tests(&self) -> u32 {
        self.index_rebuild_count.get()
    }
}
