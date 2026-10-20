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
// Stub only: the Move/Copy bodies are still `todo!()` (W8-08e).
#![allow(unused_variables, unused_imports, dead_code)]

use pr_core::abstractions::{Log, SkippedEntry};
use pr_core::file_actions::journal_line::JournalDateTime;
use pr_core::model::FileOperationType;
use pr_core::session::store::SessionState;
use pr_core::time::DateTimeUtc;

use super::navigation::OpenPurpose;
use super::state::{ChangeSet, MainProperty, MainViewModel, SettingsSource};
use crate::coordinators::file_actions::recycle::{FileActionCx, FileActionStep};
use crate::coordinators::presenter::state::PresentRequest;
use crate::coordinators::traits::{IDuplicateCleanupSink, IFileActionSink, IFolderLoadSink, ISiblingNavigatorSink};
use crate::viewmodels::format::exif::{ExifLine, format_exif};
use crate::viewmodels::format::status;

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
        std::mem::take(&mut self.actions)
    }

    /// True when nothing is recorded.
    pub(super) fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    fn record(&mut self, action: SinkAction) {
        self.actions.push(action);
    }
}

impl IFolderLoadSink for SinkLog {
    fn reset_caches(&mut self) {
        self.record(SinkAction::ResetCaches);
    }

    fn on_catalog_ready(&mut self, folder: &str, count: i32, session: &SessionState) {
        self.record(SinkAction::CatalogReady {
            folder: folder.to_owned(),
            count,
            session: session.clone(),
        });
    }

    fn present(&mut self, index: i32, presentation_generation: i64) {
        self.record(SinkAction::FolderPresent {
            index,
            presentation_generation,
        });
    }

    fn on_empty(&mut self, folder: &str, session: &SessionState) {
        self.record(SinkAction::Empty {
            folder: folder.to_owned(),
            session: session.clone(),
        });
    }

    /// The C# `MainViewModel` overrides the default (Q-R47): records `EmptyWithSubfolders`.
    fn on_empty_with_subfolders(&mut self, folder: &str, session: &SessionState, subfolder_count: i32) {
        self.record(SinkAction::EmptyWithSubfolders {
            folder: folder.to_owned(),
            session: session.clone(),
            subfolder_count,
        });
    }

    fn on_order_applied(&mut self, count: i32, current_index: i32, current_kept: bool) {
        self.record(SinkAction::OrderApplied {
            count,
            current_index,
            current_kept,
        });
    }

    fn on_files_skipped(&mut self, folder: &str, skipped: &[SkippedEntry]) {
        self.record(SinkAction::FilesSkipped {
            folder: folder.to_owned(),
            skipped: skipped.to_vec(),
        });
    }

    fn on_unreadable_removed(&mut self, removed_paths: &[String], current_removed: bool) {
        self.record(SinkAction::UnreadableRemoved {
            removed_paths: removed_paths.to_vec(),
            current_removed,
        });
    }

    fn on_failed(&mut self, folder: &str, error: &dyn std::error::Error) {
        self.record(SinkAction::Failed {
            folder: folder.to_owned(),
            message: describe_error(error),
        });
    }
}

impl IFileActionSink for SinkLog {
    fn set_status_text(&mut self, status: &str) {
        self.record(SinkAction::SetStatus(status.to_owned()));
    }

    fn show_late_action_status(&mut self, status: &str) {
        self.record(SinkAction::LateStatus(status.to_owned()));
    }

    fn on_catalog_changed(&mut self, removed_path: Option<&str>) {
        self.record(SinkAction::CatalogChanged {
            removed_path: removed_path.map(str::to_owned),
        });
    }

    fn evict_cached_paths(&mut self, paths: &[String]) {
        self.record(SinkAction::EvictPaths(paths.to_vec()));
    }

    fn present(&mut self, index: i32) {
        self.record(SinkAction::ActionPresent { index });
    }

    fn update_session_path(&mut self, current_path: &str) {
        self.record(SinkAction::UpdateSessionPath(current_path.to_owned()));
    }

    fn notify_navigation_state_changed(&mut self) {
        self.record(SinkAction::NavigationStateChanged);
    }
}

impl ISiblingNavigatorSink for SinkLog {
    fn set_status_text(&mut self, status: &str) {
        self.record(SinkAction::SetStatus(status.to_owned()));
    }

    fn open_folder(&mut self, folder: &str, initial_path: Option<&str>) {
        self.record(SinkAction::OpenFolder {
            folder: folder.to_owned(),
            initial_path: initial_path.map(str::to_owned),
        });
    }
}

impl IDuplicateCleanupSink for SinkLog {
    fn set_status_text(&mut self, status: &str) {
        self.record(SinkAction::SetStatus(status.to_owned()));
    }

    /// C# `((IFileActionSink)this).ShowLateActionStatus(status)`: records `LateStatus`.
    fn show_late_action_status(&mut self, status: &str) {
        self.record(SinkAction::LateStatus(status.to_owned()));
    }

    fn open_folder(&mut self, folder: &str, initial_path: Option<&str>) {
        self.record(SinkAction::OpenFolder {
            folder: folder.to_owned(),
            initial_path: initial_path.map(str::to_owned),
        });
    }

    /// Records `ReloadFolder` and returns `true` ("accepted", decision D-06d-4); the controller learns the real outcome from
    /// `on_reload_finished`, which the view model calls from `finish_open`.
    fn reload_folder(&mut self, folder: &str) -> bool {
        self.record(SinkAction::ReloadFolder {
            folder: folder.to_owned(),
        });
        true
    }
}

/// `UserFacingError.Describe(exception)` for the failure text of `on_failed`. No Rust port of `UserFacingError` exists yet; the default
/// is `error.to_string()` (the `FolderLoadError` `Display` text is already the localized sentence). One place to change later.
pub(super) fn describe_error(error: &dyn std::error::Error) -> String {
    error.to_string()
}

// ---- remembered Move/Copy folder ---------------------------------------------------------------------------------------------

/// C# `MainViewModel.RememberMoveCopyFolder(operation, folder)` (line 1333), the `rememberFolder` delegate of the file-action controller.
/// The composition root wraps it: `Box::new(move |op, folder| remember_move_copy_folder(&settings, log.as_ref(), op, folder))`.
/// Sets `last_move_to_folder` (Move) or `last_copy_to_folder` (Copy) in a copy of the current settings (`SettingsSource::begin_edit`, so
/// readers see it at once) and saves it with `commit_edit`. A save failure (IOException, UnauthorizedAccess, NotSupported, Argument in C#)
/// is logged as a warning `Could not save the last Move-to/Copy-to folder: <message>` and the value stays in memory for this session
/// (the provisional override). The file operation already succeeded, so nothing is reported to the user.
pub fn remember_move_copy_folder(settings: &SettingsSource, log: &dyn Log, operation: FileOperationType, folder: &str) {
    // Clones the CURRENT settings (store or override) at the time of the call: another setting changed since is not lost (T8).
    let edit = settings.begin_edit(|new| match operation {
        FileOperationType::Move => new.last_move_to_folder = Some(folder.to_owned()),
        _ => new.last_copy_to_folder = Some(folder.to_owned()),
    });
    if let Err(error) = settings.commit_edit(edit) {
        // The file operation already succeeded; only the remembered folder is lost (kept in memory for this session).
        log.warn(&format!("Could not save the last Move-to/Copy-to folder: {error}"));
    }
}

impl MainViewModel {
    // ---- change notification ----

    /// C# `NotifyPresentationChanged()` (line 1086) = `notify_navigation_state_changed()`.
    pub fn notify_presentation_changed(&mut self) -> ChangeSet {
        self.notify_navigation_state_changed()
    }

    /// C# `NotifyNavigationStateChanged()` (line 1088): reports `CanNavigateNext`, `CanNavigatePrevious`, `HasImages`, `CurrentHasCapturePair`,
    /// `CapturePairBadge`, `CurrentIndex`, `TotalFiles`, `CurrentImage`, `StatusText`, then `notify_exif_line_changed()`, then
    /// `update_folder_title(None)` (the title may include the current file / EXIF). `StatusNeedsAttention` is NOT in the set (as C#).
    pub(super) fn notify_navigation_state_changed(&mut self) -> ChangeSet {
        let mut result = ChangeSet::default();
        for property in [
            MainProperty::CanNavigateNext,
            MainProperty::CanNavigatePrevious,
            MainProperty::HasImages,
            MainProperty::CurrentHasCapturePair,
            MainProperty::CapturePairBadge,
            MainProperty::CurrentIndex,
            MainProperty::TotalFiles,
            MainProperty::CurrentImage,
            MainProperty::StatusText,
        ] {
            result.main.insert(property);
        }
        result.merge(self.notify_exif_line_changed());
        result.merge(self.update_folder_title(None));
        result
    }

    // ---- replay of the recorded sink calls ----

    /// Replays `sink.take()` in order until the log stays empty (a replayed action may call a coordinator again, which records more), then,
    /// when `controller_present_outstanding` and the presenter is idle, tells the file-action controller `on_present_finished(cx, false)`
    /// and replays what that records. Returns the combined changes.
    pub(super) fn apply_sink_actions(&mut self) -> ChangeSet {
        let mut result = self.replay_recorded();
        if self.controller_present_outstanding && self.presenter.is_idle() {
            self.controller_present_outstanding = false;
            let step = {
                let mut cx = FileActionCx {
                    catalog: &mut self.catalog,
                    sink: &mut self.sink,
                };
                self.file_action_controller.on_present_finished(&mut cx, false)
            };
            result.merge(self.replay_recorded());
            result.merge(self.handle_step(step));
        }
        result
    }

    fn replay_recorded(&mut self) -> ChangeSet {
        let mut result = ChangeSet::default();
        loop {
            let actions = self.sink.take();
            if actions.is_empty() {
                return result;
            }
            for action in actions {
                result.merge(self.apply_sink_action(action));
            }
        }
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
        let mut result = ChangeSet::default();
        match action {
            SinkAction::ResetCaches => {
                if let Some(action) = self.reset_caches_action.as_mut() {
                    action();
                }
            }
            SinkAction::CatalogReady { folder, count, session } => {
                self.cancel_preload();
                self.current_session = Some(session); // loaded once by FolderLoadCoordinator (APP-02)
                self.on_folder_shown(&folder);
                if !self.skipped_entries.is_empty() {
                    result.merge(self.set_skipped_entries(Vec::new())); // a new load; OnFilesSkipped follows if needed
                }
                result.merge(self.set_folder_text(&folder, count, false));
                result.merge(self.update_folder_title(Some(&folder)));
                result.merge(self.set_overlay_folder(&folder));
                // `_statusText = ..`: assigned without a change report (the notification below reports StatusText).
                self.status_text = self.with_ctx(|ctx| status::index_only(0, count, ctx));
                result.catalog_changed = true;
                result.merge(self.notify_navigation_state_changed());
            }
            SinkAction::FolderPresent { index, .. } => {
                self.status_text.clear();
                result.merge(self.present_request(PresentRequest::new(index)));
            }
            SinkAction::Empty { folder, session } => {
                let text = self.with_ctx(status::no_supported_images);
                result.merge(self.apply_empty(&folder, session, &text));
            }
            SinkAction::EmptyWithSubfolders {
                folder,
                session,
                subfolder_count,
            } => {
                let text = self.with_ctx(|ctx| status::no_supported_images_but_subfolders(subfolder_count, ctx));
                result.merge(self.apply_empty(&folder, session, &text));
            }
            SinkAction::OrderApplied {
                count,
                current_index,
                current_kept,
            } => {
                if let Some(folder) = self.current_session.as_ref().map(|session| session.folder.clone()) {
                    result.merge(self.set_folder_text(&folder, count, true));
                } else if let Some(shown) = self.folder_text_folder.clone()
                    && !self.explorer_order_applied
                {
                    // No session folder: keep the folder and count already shown, just mark the order.
                    let shown_count = self.folder_text_count;
                    result.merge(self.set_folder_text(&shown, shown_count, true));
                }
                // The loop still walks the pre-order snapshot: cancel it so the fresh lifetime sees the new order (R2-F-01), and only
                // then re-centre on the new index of the image that kept its place.
                self.cancel_preload();
                if current_kept && current_index >= 0 {
                    self.preload_around(current_index);
                }
                result.catalog_changed = true;
                result.merge(self.notify_navigation_state_changed());
            }
            SinkAction::FilesSkipped { skipped, .. } => {
                result.merge(self.set_skipped_entries(skipped));
            }
            SinkAction::UnreadableRemoved {
                removed_paths,
                current_removed,
            } => {
                result.merge(self.apply_unreadable_removed(&removed_paths, current_removed));
            }
            SinkAction::Failed { message, .. } => {
                let text = self.with_ctx(|ctx| status::folder_open_failed(&message, ctx));
                result.merge(self.set_status_text(&text));
                result.merge(self.notify_navigation_state_changed());
            }
            SinkAction::SetStatus(text) => {
                result.merge(self.set_status_text(&text));
            }
            SinkAction::LateStatus(text) => {
                // Event text is non-empty while the current folder shows its own message: never overwrite it.
                if self.status_text.is_empty() {
                    result.merge(self.set_status_text(&text));
                }
            }
            SinkAction::CatalogChanged { removed_path } => {
                result.catalog_changed = true;
                // Only the removed file leaves the cache; the new current image is usually already preloaded (R2-F-02).
                if let Some(path) = removed_path {
                    self.evict_path(&path);
                }
                result.compare = self.clear_compare();
            }
            SinkAction::EvictPaths(paths) => {
                for path in &paths {
                    self.evict_path(path);
                }
            }
            SinkAction::ActionPresent { index } => {
                self.controller_present_outstanding = true;
                result.merge(self.present_request(PresentRequest::new(index)));
            }
            SinkAction::UpdateSessionPath(path) => {
                if let Some(session) = self.current_session.as_mut() {
                    session.current_path = Some(path);
                    session.updated_utc = JournalDateTime::utc(DateTimeUtc::now());
                    self.persist_session();
                }
            }
            SinkAction::NavigationStateChanged => {
                result.merge(self.notify_navigation_state_changed());
            }
            SinkAction::OpenFolder { folder, initial_path } => match self.open_folder(&folder, initial_path.as_deref())
            {
                Ok(changes) => result.merge(changes),
                Err(error) => self.log.error("A requested folder open failed", Some(&error)),
            },
            SinkAction::ReloadFolder { folder } => {
                match self.open_folder_core(&folder, None, None, OpenPurpose::DuplicateReload) {
                    Ok(changes) => result.merge(changes),
                    Err(error) => self.log.error("A requested folder reload failed", Some(&error)),
                }
            }
        }
        result
    }

    /// C# `_preloadController?.Cancel()`.
    fn cancel_preload(&self) {
        if let Some(preload) = &self.preload_controller {
            preload.cancel();
        }
    }

    /// C# `TaskLogging.FireAndLog(() => _preloadController?.PreloadAroundAsync(index), ..)`: the handle is dropped (O4).
    fn preload_around(&self, index: i32) {
        if let Some(preload) = &self.preload_controller {
            drop(preload.preload_around(index));
        }
    }

    /// C# `_presenter.EvictCachedPath(path)`; a failure of the cache is logged and never stops the caller.
    fn evict_path(&self, path: &str) {
        if let Err(error) = self.presenter.evict_cached_path(path) {
            self.log
                .warn(&format!("Could not evict '{path}' from the image cache: {error}"));
        }
    }

    /// C# `_compare.Clear()`.
    fn clear_compare(&mut self) -> crate::viewmodels::compare::CompareChanges {
        let format = std::sync::Arc::clone(&self.format);
        let mut changes = crate::viewmodels::compare::CompareChanges::default();
        let presenter = &mut self.presenter;
        format.with_context(&mut |ctx| {
            changes = presenter.compare_mut().clear(ctx);
        });
        changes
    }

    /// C# `InfoOverlay.SetFolder(folder)`.
    fn set_overlay_folder(&mut self, folder: &str) -> ChangeSet {
        let inputs = self.view_inputs();
        let settings = self.settings.current();
        let refresh = self.info_overlay.set_folder(&settings, inputs, Some(folder));
        self.apply_overlay_refresh(refresh)
    }

    /// The common body of `OnEmpty` and `OnEmptyWithSubfolders` (C# lines 1132 to 1158).
    fn apply_empty(&mut self, folder: &str, session: SessionState, status_text: &str) -> ChangeSet {
        let mut result = ChangeSet::default();
        self.cancel_preload();
        self.current_session = Some(session); // loaded once by FolderLoadCoordinator (APP-02)
        self.on_folder_shown(folder);
        result.merge(self.set_folder_text(folder, 0, false));
        result.merge(self.update_folder_title(Some(folder)));
        result.merge(self.set_overlay_folder(folder));
        result.merge(self.set_status_text(status_text));
        let changes = self.with_presenter(|presenter, env| presenter.clear_presentation(env));
        result.merge(self.finish_presenter_call(changes));
        result.catalog_changed = true;
        result.merge(self.notify_navigation_state_changed());
        result
    }

    /// C# `OnUnreadableRemovedAsync` (lines 1186 to 1226), after the whole probe step ran (see the contract, note 4).
    fn apply_unreadable_removed(&mut self, removed_paths: &[String], current_removed: bool) -> ChangeSet {
        let mut result = ChangeSet::default();
        // AR16: the running preload loop walks a snapshot that still holds the removed files: stop it, and drop anything already
        // cached or preloaded for them, before a fresh lifetime starts below.
        self.cancel_preload();
        for path in removed_paths {
            self.evict_path(path);
        }
        if let Some(shown) = self.folder_text_folder.clone() {
            let count = i32::try_from(self.catalog.count()).unwrap_or(i32::MAX);
            let applied = self.explorer_order_applied;
            result.merge(self.set_folder_text(&shown, count, applied));
        }
        result.catalog_changed = true;
        if current_removed {
            // Same as a Delete of the current image: show the next one (it saves the session), or the empty state.
            result.compare = self.clear_compare();
            if self.catalog.current_index() >= 0 {
                self.status_text.clear();
                result.merge(self.present_request(PresentRequest::new(self.catalog.current_index())));
            } else {
                let changes = self.with_presenter(|presenter, env| presenter.clear_presentation(env));
                result.merge(self.finish_presenter_call(changes));
                let text = self.with_ctx(status::no_images_remaining);
                result.merge(self.set_status_text(&text));
            }
        } else if self.catalog.current_index() >= 0 {
            // The current image stays on screen; its neighbours changed, so preload re-centres on its new index.
            self.preload_around(self.catalog.current_index());
        }
        result.merge(self.notify_navigation_state_changed());
        result
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
        let Some(info) = self.presenter.current_photo_info() else {
            return String::new();
        };
        let fields = self.settings.current().exif_info_fields;
        // ModifiedDate: the catalog entry from the folder scan, no extra disk read.
        let modified_local = self
            .catalog
            .current()
            .and_then(|entry| entry.last_write_utc)
            .map(|utc| self.format.to_local_time(utc));
        self.with_ctx(|ctx| {
            format_exif(
                fields,
                &ExifLine {
                    file_name: Some(&info.file_name),
                    width: info.width,
                    height: info.height,
                    exif: info.exif.as_ref(),
                    modified_local,
                    raw_preview_width: info.raw_preview_width,
                    raw_preview_height: info.raw_preview_height,
                },
                ctx,
            )
        })
    }

    /// C# `IsExifLineVisible` (line 1367): `show_info_overlay && show_exif_info && !exif_text().is_empty()`.
    pub fn is_exif_line_visible(&self) -> bool {
        let settings = self.settings.current();
        settings.show_info_overlay && settings.show_exif_info && !self.exif_text().is_empty()
    }

    /// C# `ExifAutomationName` (line 1370): `main.exif.automationName` with `{details}` = `exif_text()`.
    pub fn exif_automation_name(&self) -> String {
        let details = self.exif_text();
        self.with_ctx(|ctx| {
            ctx.texts.format_with(
                "main.exif.automationName",
                &[pr_l10n::template::LocArg::new("details", details.as_str())],
                ctx.numbers,
            )
        })
    }

    /// C# `NotifyExifLineChanged()` (line 1372): reports `ExifText`, `IsExifLineVisible`, `ExifAutomationName`, `IsStatusPanelVisible`.
    pub(super) fn notify_exif_line_changed(&self) -> ChangeSet {
        let mut result = ChangeSet::default();
        for property in [
            MainProperty::ExifText,
            MainProperty::IsExifLineVisible,
            MainProperty::ExifAutomationName,
            MainProperty::IsStatusPanelVisible,
        ] {
            result.main.insert(property);
        }
        result
    }
}
