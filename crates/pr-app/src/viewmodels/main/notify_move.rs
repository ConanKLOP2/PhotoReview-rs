//! Port of `PhotoReview.App.Shared/ViewModels/MainViewModel.cs` lines 1086 to 1385 (baseline `d92d6aae`): `NotifyPresentationChanged`,
//! `NotifyNavigationStateChanged`, the four sink implementations (`IFolderLoadSink`, `IFileActionSink`, `ISiblingNavigatorSink`,
//! `IDuplicateCleanupSink`), `MoveToFolderAsync` / `CopyToFolderAsync`, `RememberMoveCopyFolder`, `ExifText`, `IsExifLineVisible`,
//! `ExifAutomationName`, `NotifyExifLineChanged` and `ImageChangingEventArgs` (that type is defined in `state.rs`)
//! (contract `contracts/W8-08e.md`). Target: `crates/pr-app/src/viewmodels/main/notify_move.rs` (`viewmodels::main::notify_move`).
//!
//! DECISION D-08e-1 (the sinks are recorders). The coordinators receive the catalog AND the sink in one call (`FolderLoadCx`,
//! `FileActionCx`, `DuplicateCleanupCx`), so the sink cannot be the view model (it needs the catalog, which the coordinator holds
//! `&mut`). [`SinkLog`] implements the four sink traits by RECORDING each call as a [`SinkAction`]; after every coordinator call the
//! view model replays the log in order with `apply_sink_actions`, with full access to the catalog, the presenter and the settings.
//! The order of the calls is kept; what changes is that a replayed action sees the catalog AFTER the whole coordinator step (see the
//! contract, trap T3).
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]

use pr_core::abstractions::{Log, SkippedEntry};
use pr_core::model::FileOperationType;
use pr_core::session::store::SessionState;

use super::state::{ChangeSet, MainViewModel, SettingsSource};
use crate::coordinators::file_actions::recycle::FileActionStep;
use crate::coordinators::traits::{IDuplicateCleanupSink, IFileActionSink, IFolderLoadSink, ISiblingNavigatorSink};

// ---- the sink recorder -----------------------------------------------------------------------------------------------------

/// One recorded sink call. Owned data only (the call has returned when it is replayed).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum SinkAction {
    // IFolderLoadSink
    ResetCaches,
    CatalogReady {
        folder: String,
        count: i32,
        session: SessionState,
    },
    /// `present(index, presentation_generation)`; the generation is not used by C# `MainViewModel` either.
    FolderPresent {
        index: i32,
        presentation_generation: i64,
    },
    Empty {
        folder: String,
        session: SessionState,
    },
    EmptyWithSubfolders {
        folder: String,
        session: SessionState,
        subfolder_count: i32,
    },
    OrderApplied {
        count: i32,
        current_index: i32,
        current_kept: bool,
    },
    FilesSkipped {
        folder: String,
        skipped: Vec<SkippedEntry>,
    },
    UnreadableRemoved {
        removed_paths: Vec<String>,
        current_removed: bool,
    },
    /// `on_failed`: the error already described (`describe_error`).
    Failed {
        folder: String,
        message: String,
    },
    // IFileActionSink / ISiblingNavigatorSink / IDuplicateCleanupSink
    SetStatus(String),
    LateStatus(String),
    CatalogChanged {
        removed_path: Option<String>,
    },
    EvictPaths(Vec<String>),
    /// `IFileActionSink::present(index)`: also marks `controller_present_outstanding`.
    ActionPresent {
        index: i32,
    },
    UpdateSessionPath(String),
    NavigationStateChanged,
    OpenFolder {
        folder: String,
        initial_path: Option<String>,
    },
    /// `IDuplicateCleanupSink::reload_folder`.
    ReloadFolder {
        folder: String,
    },
}

/// The recorder that implements the four sink traits (decision D-08e-1).
#[derive(Debug, Default)]
pub struct SinkLog {
    actions: Vec<SinkAction>,
}

impl SinkLog {
    /// An empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the recorded calls in call order.
    pub(super) fn take(&mut self) -> Vec<SinkAction> {
        todo!()
    }

    /// True when nothing is recorded.
    pub(super) fn is_empty(&self) -> bool {
        todo!()
    }
}

impl IFolderLoadSink for SinkLog {
    fn reset_caches(&mut self) {
        todo!()
    }

    fn on_catalog_ready(&mut self, folder: &str, count: i32, session: &SessionState) {
        todo!()
    }

    fn present(&mut self, index: i32, presentation_generation: i64) {
        todo!()
    }

    fn on_empty(&mut self, folder: &str, session: &SessionState) {
        todo!()
    }

    /// The C# `MainViewModel` overrides the default (Q-R47): records `EmptyWithSubfolders`.
    fn on_empty_with_subfolders(&mut self, folder: &str, session: &SessionState, subfolder_count: i32) {
        todo!()
    }

    fn on_order_applied(&mut self, count: i32, current_index: i32, current_kept: bool) {
        todo!()
    }

    fn on_files_skipped(&mut self, folder: &str, skipped: &[SkippedEntry]) {
        todo!()
    }

    fn on_unreadable_removed(&mut self, removed_paths: &[String], current_removed: bool) {
        todo!()
    }

    fn on_failed(&mut self, folder: &str, error: &dyn std::error::Error) {
        todo!()
    }
}

impl IFileActionSink for SinkLog {
    fn set_status_text(&mut self, status: &str) {
        todo!()
    }

    fn show_late_action_status(&mut self, status: &str) {
        todo!()
    }

    fn on_catalog_changed(&mut self, removed_path: Option<&str>) {
        todo!()
    }

    fn evict_cached_paths(&mut self, paths: &[String]) {
        todo!()
    }

    fn present(&mut self, index: i32) {
        todo!()
    }

    fn update_session_path(&mut self, current_path: &str) {
        todo!()
    }

    fn notify_navigation_state_changed(&mut self) {
        todo!()
    }
}

impl ISiblingNavigatorSink for SinkLog {
    fn set_status_text(&mut self, status: &str) {
        todo!()
    }

    fn open_folder(&mut self, folder: &str, initial_path: Option<&str>) {
        todo!()
    }
}

impl IDuplicateCleanupSink for SinkLog {
    fn set_status_text(&mut self, status: &str) {
        todo!()
    }

    /// C# `((IFileActionSink)this).ShowLateActionStatus(status)`: records `LateStatus`.
    fn show_late_action_status(&mut self, status: &str) {
        todo!()
    }

    fn open_folder(&mut self, folder: &str, initial_path: Option<&str>) {
        todo!()
    }

    /// Records `ReloadFolder` and returns `true` ("accepted", decision D-06d-4); the controller learns the real outcome from
    /// `on_reload_finished`, which the view model calls from `finish_open`.
    fn reload_folder(&mut self, folder: &str) -> bool {
        todo!()
    }
}

/// `UserFacingError.Describe(exception)` for the failure text of `on_failed`. No Rust port of `UserFacingError` exists yet; the default
/// is `error.to_string()` (the `FolderLoadError` `Display` text is already the localized sentence). One place to change later.
pub(super) fn describe_error(error: &dyn std::error::Error) -> String {
    todo!()
}

// ---- remembered Move/Copy folder ---------------------------------------------------------------------------------------------

/// C# `MainViewModel.RememberMoveCopyFolder(operation, folder)` (line 1333), the `rememberFolder` delegate of the file-action controller.
/// The composition root wraps it: `Box::new(move |op, folder| remember_move_copy_folder(&settings, log.as_ref(), op, folder))`.
/// Sets `last_move_to_folder` (Move) or `last_copy_to_folder` (Copy) in a copy of the current settings (`SettingsSource::begin_edit`, so
/// readers see it at once) and saves it with `commit_edit`. A save failure (IOException, UnauthorizedAccess, NotSupported, Argument in C#)
/// is logged as a warning `Could not save the last Move-to/Copy-to folder: <message>` and the value stays in memory for this session
/// (the provisional override). The file operation already succeeded, so nothing is reported to the user.
pub fn remember_move_copy_folder(settings: &SettingsSource, log: &dyn Log, operation: FileOperationType, folder: &str) {
    todo!()
}

impl MainViewModel {
    // ---- change notification ----

    /// C# `NotifyPresentationChanged()` (line 1086) = `notify_navigation_state_changed()`.
    pub fn notify_presentation_changed(&mut self) -> ChangeSet {
        todo!()
    }

    /// C# `NotifyNavigationStateChanged()` (line 1088): reports `CanNavigateNext`, `CanNavigatePrevious`, `HasImages`, `CurrentHasCapturePair`,
    /// `CapturePairBadge`, `CurrentIndex`, `TotalFiles`, `CurrentImage`, `StatusText`, then `notify_exif_line_changed()`, then
    /// `update_folder_title(None)` (the title may include the current file / EXIF). `StatusNeedsAttention` is NOT in the set (as C#).
    pub(super) fn notify_navigation_state_changed(&mut self) -> ChangeSet {
        todo!()
    }

    // ---- replay of the recorded sink calls ----

    /// Replays `sink.take()` in order until the log stays empty (a replayed action may call a coordinator again, which records more), then,
    /// when `controller_present_outstanding` and the presenter is idle, tells the file-action controller `on_present_finished(cx, false)`
    /// and replays what that records. Returns the combined changes.
    pub(super) fn apply_sink_actions(&mut self) -> ChangeSet {
        todo!()
    }

    /// One recorded action (the bodies of C# lines 1103 to 1309). Per action:
    /// - `ResetCaches`: `reset_caches_action()`.
    /// - `CatalogReady` (line 1110): `preload.cancel()`; `current_session = session`; `on_folder_shown(folder)`; clear the skipped entries when
    ///   there were any; `set_folder_text(folder, count, false)`; `update_folder_title(Some(folder))`; `info_overlay.set_folder(Some(folder))` (applied);
    ///   the EVENT status becomes `status::index_only(0, count)` (assigned without a change report, as C# `_statusText = ..`); `catalog_changed`;
    ///   `notify_navigation_state_changed()`.
    /// - `FolderPresent` (line 1126): event status cleared; `present(PresentRequest::new(index))`.
    /// - `Empty` / `EmptyWithSubfolders` (lines 1132, 1146): as `CatalogReady` up to the title and overlay, then `set_status_text(status::no_supported_images()
    ///   | status::no_supported_images_but_subfolders(n))`, `presenter.clear_presentation(env)`, `catalog_changed`, `notify_navigation_state_changed()`.
    /// - `OrderApplied` (line 1160): `set_folder_text(session folder, count, true)` when a session folder exists, else (folder line shown and order not
    ///   yet applied) `set_folder_text(shown folder, stored count, true)`; `preload.cancel()`; `current_kept && current_index >= 0` ->
    ///   `preload.preload_around(current_index)` (fire and forget, the handle is dropped); `catalog_changed`; `notify_navigation_state_changed()`.
    /// - `FilesSkipped`: `set_skipped_entries(list)`.
    /// - `UnreadableRemoved` (line 1186): `preload.cancel()`; `presenter.evict_cached_path` per removed path (an `Err` is logged); the folder line is
    ///   re-rendered with `catalog.count()` when shown; `catalog_changed`; when `current_removed`: `compare.clear(ctx)` then, if `current_index >= 0`,
    ///   event status cleared + `present(current_index)`, else `presenter.clear_presentation(env)` + `set_status_text(status::no_images_remaining())`;
    ///   when the current image stays and `current_index >= 0`: `preload.preload_around(current_index)`; finally `notify_navigation_state_changed()`.
    /// - `Failed`: `set_status_text(status::folder_open_failed(message))`, `notify_navigation_state_changed()`.
    /// - `SetStatus`: `set_status_text`. `LateStatus`: only when the event status is empty. `CatalogChanged` (line 1242): `catalog_changed`; the removed
    ///   path is evicted from the preview cache; `compare.clear(ctx)`. `EvictPaths`: evict each. `ActionPresent`: `present(PresentRequest::new(index))` and
    ///   `controller_present_outstanding = true`. `UpdateSessionPath` (line 1264): when a session exists, `current_path = path`,
    ///   `updated_utc = now`, `persist_session()`. `NavigationStateChanged`: `notify_navigation_state_changed()`.
    /// - `OpenFolder`: `open_folder(folder, initial_path)`. `ReloadFolder`: `open_folder_core(folder, None, None, OpenPurpose::DuplicateReload)`.
    pub(super) fn apply_sink_action(&mut self, action: SinkAction) -> ChangeSet {
        todo!()
    }

    // ---- Move to / Copy to ----

    /// C# `MoveToFolderAsync(forcePicker = false)` (line 1316): submits `QueuedKind::MoveCopy { Move, force_picker }` (same gate and pipeline
    /// as an action profile).
    pub fn move_to_folder(&mut self, force_picker: bool) -> ChangeSet {
        todo!()
    }

    /// C# `CopyToFolderAsync(forcePicker = false)` (line 1319): as `move_to_folder` with `Copy`.
    pub fn copy_to_folder(&mut self, force_picker: bool) -> ChangeSet {
        todo!()
    }

    /// The body of a `MoveCopy` command once its turn came (C# lambda of `MoveOrCopyToFolderAsync`, line 1326):
    /// `file_action_controller.move_or_copy_to_folder(cx, folder_picker, operation, force_picker, get_source)` where `get_source` returns the
    /// SNAPSHOT `(compare().selected_path(), catalog.current().path)` taken just before the call (the picker's nested loop cannot change the
    /// state of a view model that is mutably borrowed by this call, so the C# "read again after the picker" cannot differ). An `Err(operation)`
    /// (C# `ArgumentOutOfRangeException`, impossible here) is treated as `Done(Finished { succeeded: false })`.
    pub(super) fn begin_move_copy(&mut self, operation: FileOperationType, force_picker: bool) -> FileActionStep {
        todo!()
    }

    // ---- photo information (EXIF) line ----

    /// C# `ExifText` (line 1356): `format_exif(settings().exif_info_fields, ExifLine { file_name, width, height, exif, modified_local, raw preview
    /// size from presenter.current_photo_info() }, ctx)`; `modified_local` = the catalog current entry's `last_write_utc` converted with
    /// `FormatContextSource::to_local_time` (no extra disk read). Empty while loading, in compare mode or with nothing shown.
    pub fn exif_text(&self) -> String {
        todo!()
    }

    /// C# `IsExifLineVisible` (line 1367): `show_info_overlay && show_exif_info && !exif_text().is_empty()`.
    pub fn is_exif_line_visible(&self) -> bool {
        todo!()
    }

    /// C# `ExifAutomationName` (line 1370): `main.exif.automationName` with `{details}` = `exif_text()`.
    pub fn exif_automation_name(&self) -> String {
        todo!()
    }

    /// C# `NotifyExifLineChanged()` (line 1372): reports `ExifText`, `IsExifLineVisible`, `ExifAutomationName`, `IsStatusPanelVisible`.
    pub(super) fn notify_exif_line_changed(&self) -> ChangeSet {
        todo!()
    }
}
