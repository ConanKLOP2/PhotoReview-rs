//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 504-515 (`ReadPlacement`, baseline `d92d6aae`). Unit W8-17e. Contract: `contracts/W8-17e.md`.
//!
//! The saved window placement for `ShellPlacement::apply_before_show`: the value the prefetch (started in phase one, W8-17d) already read, else a direct read of the file. A damaged or
//! unreadable file is logged and means "no saved placement" (the window opens at its default size); it never fails the start-up.

use pr_core::abstractions::{FileSystem, Log};

use crate::windowing::contracts::{WindowPlacementData, WindowPlacementStore};
use crate::windowing::placement_store::JsonWindowPlacementStore;

/// C# `ReadPlacement(placementFile, log)`: `store.take_prefetched(file)` (at most once) `??` `JsonWindowPlacementStore::read_raw(fs, file)`; an `Err(PlacementReadError)` (C# `IOException`,
/// `UnauthorizedAccessException`, `JsonException`, `NotSupportedException`) is `log.error("Could not read the window placement", Some(&error))` and the answer is `None`. The value is NOT
/// filtered by visibility (`apply_before_show` does that through its `is_visible` argument).
pub fn read_saved_placement(
    store: &dyn WindowPlacementStore,
    fs: &dyn FileSystem,
    placement_file: &str,
    log: &dyn Log,
) -> Option<WindowPlacementData> {
    if let Some(prefetched) = store.take_prefetched(placement_file) {
        return Some(prefetched);
    }
    match JsonWindowPlacementStore::read_raw(fs, placement_file) {
        Ok(data) => data,
        Err(error) => {
            log.error("Could not read the window placement", Some(&error));
            None
        }
    }
}
