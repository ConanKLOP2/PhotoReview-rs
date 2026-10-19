//! Port of `PhotoReview.Shell.Win32/Overlay/ToolsMenu.cs` (WP-23): the `IOverlayCommands` seam the toolbar calls, and the
//! Tools menu (`⋮` button) model and dispatch.
//!
//! Baseline `d92d6aae`, 74 lines. Owned by W8-15e (contract `contracts/W8-15e.md`). Test kind F
//! (the C# tests use a recording fake of `IOverlayCommands`; the Rust tests use the same kind of fake).
//!
//! Pure: no window, no `windows` crate. The Win32 host that shows the menu (`IPopupMenuHost`, anchor from the `⋮`
//! button) is `pr_platform_win::shell_overlay` (W8-15f / W8-15i), not here.
//!
//! Traps (contract section "Traps"): T1 an unknown or absent id is `false` and calls nothing (C# returns `null`);
//! T2 a duplicate-removal id with no images is `true` (C# returns a completed Task) and calls nothing; T3 ids are
//! ordinal (case-sensitive) string equality, never `eq_ignore_case`; T4 the async commands are sync in Rust: the trait
//! method submits work and returns, the host reports completion through `UiDispatcher` (D-W8-2).

use crate::menus::model::MenuItemModel;

/// C# `ToolsMenu.Recovery`: `"Tools.Recovery"`.
pub const RECOVERY: &str = "Tools.Recovery";
/// C# `ToolsMenu.Diagnostics`: `"Tools.Diagnostics"`.
pub const DIAGNOSTICS: &str = "Tools.Diagnostics";
/// C# `ToolsMenu.Benchmark`: `"Tools.Benchmark"`.
pub const BENCHMARK: &str = "Tools.Benchmark";
/// C# `ToolsMenu.ClearCache`: `"Tools.ClearCache"`.
pub const CLEAR_CACHE: &str = "Tools.ClearCache";
/// C# `ToolsMenu.RemoveNumberedDuplicates`: `"Tools.RemoveNumberedDuplicates"`.
pub const REMOVE_NUMBERED_DUPLICATES: &str = "Tools.RemoveNumberedDuplicates";
/// C# `ToolsMenu.RemoveOriginalDuplicates`: `"Tools.RemoveOriginalDuplicates"`.
pub const REMOVE_ORIGINAL_DUPLICATES: &str = "Tools.RemoveOriginalDuplicates";

/// The six Tools menu ids in XAML order (C# `ToolsMenu.Build` order, test `ToolsMenu_BuildGivesTheSixToolsItemsInXamlOrder`).
pub const TOOLS_IDS: [&str; 6] = [
    RECOVERY,
    DIAGNOSTICS,
    BENCHMARK,
    CLEAR_CACHE,
    REMOVE_NUMBERED_DUPLICATES,
    REMOVE_ORIGINAL_DUPLICATES,
];

/// C# `IOverlayCommands`: the commands the toolbar and the Tools menu call. The shell implements it by bridging to
/// `MainViewModel` (tests use a fake). In C# the async members return `Task` and the caller wraps them "guarded"
/// (errors to the log and the status line, like `RunGuardedAsync`). In Rust every member is synchronous: an async
/// command submits its work to the worker pool and returns; the implementation reports failures through the same
/// guarded path and completes on the UI thread (D-W8-2).
pub trait OverlayCommands {
    /// C# `OpenFolderAsync()`: open the folder picker and load the chosen folder.
    fn open_folder(&mut self);

    /// C# `ShowSettings()`: open the settings dialog (modal, synchronous in C#).
    fn show_settings(&mut self);

    /// C# `FitAsync()`: the Fit command (same path as key F).
    fn fit(&mut self);

    /// C# `ShowSkippedFiles()`: open the skipped-files list.
    fn show_skipped_files(&mut self);

    /// C# `ShowRecovery()`: open the Recovery window.
    fn show_recovery(&mut self);

    /// C# `ShowDiagnostics()`: open the Diagnostics window.
    fn show_diagnostics(&mut self);

    /// C# `ShowBenchmark()`: open the Benchmark window.
    fn show_benchmark(&mut self);

    /// C# `ClearCacheAsync()`: clear the image cache.
    fn clear_cache(&mut self);

    /// C# `RemoveDuplicatesAsync(bool numbered)`: remove duplicates; `numbered = true` removes the numbered copies
    /// (`REMOVE_NUMBERED_DUPLICATES`), `false` the originals (`REMOVE_ORIGINAL_DUPLICATES`).
    fn remove_duplicates(&mut self, numbered: bool);
}

/// C# `ToolsMenu.Build(bool hasImages)`: the menu model (`ContextMenuModelBuilder.BuildToolsMenu`, W8-02a). The six
/// items are enabled; the two duplicate items are disabled when there are no images (C# `hasImages` rule of the
/// builder). Ids are exactly `TOOLS_IDS`, in that order.
pub fn build(has_images: bool) -> Vec<MenuItemModel> {
    crate::menus::builder::build_tools_menu(has_images)
}

/// C# `ToolsMenu.Execute(string? id, IOverlayCommands commands, bool hasImages)`. Runs the command of item `id`:
/// - `RECOVERY` -> `commands.show_recovery()`; `DIAGNOSTICS` -> `show_diagnostics()`; `BENCHMARK` -> `show_benchmark()`;
/// - `CLEAR_CACHE` -> `clear_cache()`;
/// - `REMOVE_NUMBERED_DUPLICATES` -> `remove_duplicates(true)` when `has_images`, nothing otherwise;
/// - `REMOVE_ORIGINAL_DUPLICATES` -> `remove_duplicates(false)` when `has_images`, nothing otherwise.
///
/// Returns `true` when `id` is one of the six ids (including the no-op duplicate case, trap T2), `false` for `None`
/// and any other id (trap T1). Ordinal match only (trap T3). Commands are never invoked for an unknown id.
pub fn execute(id: Option<&str>, commands: &mut dyn OverlayCommands, has_images: bool) -> bool {
    match id {
        Some(RECOVERY) => commands.show_recovery(),
        Some(DIAGNOSTICS) => commands.show_diagnostics(),
        Some(BENCHMARK) => commands.show_benchmark(),
        Some(CLEAR_CACHE) => commands.clear_cache(),
        Some(REMOVE_NUMBERED_DUPLICATES) if has_images => commands.remove_duplicates(true),
        Some(REMOVE_ORIGINAL_DUPLICATES) if has_images => commands.remove_duplicates(false),
        Some(REMOVE_NUMBERED_DUPLICATES | REMOVE_ORIGINAL_DUPLICATES) => return true,
        _ => return false,
    }
    true
}
