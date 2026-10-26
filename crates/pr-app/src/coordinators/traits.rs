//! Port of `PhotoReview.App.Shared/Coordinators/`: `IDuplicateCleanupSink.cs` (31 lines), `IFileActionExecutor.cs` (32),
//! `IFileActionSink.cs` (39), `IFolderLoadSink.cs` (56), `IFolderPicker.cs` (14), `IMoveOverride.cs` (14),
//! `IPreloadController.cs` (44), `IPresentationSink.cs` (27), `ISiblingNavigatorSink.cs` (14) and
//! `PreloadControllerAdapter.cs` (32). Total 303 C# lines. Target: `crates/pr-app/src/coordinators/traits.rs` (`coordinators::traits`).
//!
//! Conventions (contract `contracts/W8-06d.md`, decisions D-06d-1 to D-06d-5):
//! - Sinks are implemented by the shell or a view model on the UI thread. They take `&mut self` and return `()`: the shell
//!   repaints from the calls (D-W8-1). A C# `Task` return becomes a synchronous call that only posts the work (D-W8-2);
//!   the completion comes back through the UI dispatcher, never through a return value.
//! - The C# default interface members keep their C# default bodies. A member without a C# default is required here too
//!   (`IFolderLoadSink::on_unreadable_removed`, `IFileActionSink::show_late_action_status`).
//! - Paths are `&str` / `&[String]`, as in the C# signatures. `IMoveOverride` takes `&Path`, because it feeds pr-core's
//!   `MoveOverride` (`pr_core::file_actions::undo`).
//! - Pure module: no `unsafe`, no `windows` crate (AGENTS.md, WAVE-8-PLAN "Crates and layering").

use pr_core::file_actions::rename::PathKeyMap;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pr_core::abstractions::SkippedEntry;
use pr_core::catalog::types::CaptureGroup;
use pr_core::file_actions::rename::RenameRequest;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, FileActionRequest, FileActionResult,
};
use pr_core::file_actions::undo::MoveOverride;
use pr_core::session::store::SessionState;
use pr_core::sync::CancelToken;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::decode_types::PlatformImage;

use crate::coordinators::presenter::state::ImageChange;
use pr_imaging::preload::scheduler::{PreloadScheduler, RunHandle};

/// C# `IDuplicateCleanupSink` (31 lines): status, late status, open folder, reload after a batch.
pub trait IDuplicateCleanupSink {
    /// C# `SetStatusText(string)`: status line for the user.
    fn set_status_text(&mut self, status: &str);

    /// C# default: `SetStatusText(status)`. RV-A10: a batch that finished after the user left its folder. The view model
    /// drops it unless the status line is idle, so an implementation overrides this to keep the new folder's status.
    fn show_late_action_status(&mut self, status: &str) {
        self.set_status_text(status)
    }

    /// C# `OpenFolderAsync(folder, initialPath)`: asks the shell to open the folder. Returns once the request is posted (D-W8-2).
    fn open_folder(&mut self, folder: &str, initial_path: Option<&str>);

    /// C# default `ReloadFolderAsync(folder)`: `OpenFolderAsync(folder)`, then `true` (the default has no way to tell that
    /// another load started; see D-06d-4 for the Rust meaning of the returned bool).
    fn reload_folder(&mut self, folder: &str) -> bool {
        self.open_folder(folder, None);
        true
    }
}

/// C# `IFileActionExecutor` (32 lines, with the production `FileActionServiceExecutor` in the same file).
/// The seam lets the controllers exercise their "unexpected exception" paths; `FileActionService` turns its own failures
/// into results. C# `CancellationToken ct = default` becomes an explicit `&CancelToken` (`CancelToken::none()` for "no token").
pub trait IFileActionExecutor {
    /// C# `IsBusy` (`FileActionService.IsBusy`).
    fn is_busy(&self) -> bool;

    /// C# `LacksRecycleBin(path)`.
    fn lacks_recycle_bin(&self, path: &str) -> bool;

    /// C# `ExecuteAsync(request, ct)`, run synchronously and blocking (D-W8-2, R0003).
    fn execute(&self, request: &FileActionRequest, cancel: &CancelToken) -> FileActionResult;

    /// C# `ExecuteGroupAsync(request, ct)`, run synchronously and blocking (D-W8-2, R0003).
    fn execute_group(&self, request: &CaptureGroupActionRequest, cancel: &CancelToken) -> CaptureGroupActionResult;

    /// FEAT-RENAME (C# default member `RenameAsync`, `FileActionService.RenameAsync`), blocking on the file-action lane (R0003).
    /// W9-R5 stub: the C# default throws `NotSupportedException`; the porter returns a failed result instead of panicking.
    fn rename(&self, request: &RenameRequest, cancel: &CancelToken) -> FileActionResult {
        let _ = (request, cancel);
        todo!()
    }

    /// FEAT-RENAME (C# default member `RenameGroupAsync`, `FileActionService.RenameGroupAsync`), blocking (R0003).
    fn rename_group(&self, group: &CaptureGroup, typed_stem: &str, cancel: &CancelToken) -> CaptureGroupActionResult {
        let _ = (group, typed_stem, cancel);
        todo!()
    }
}

/// C# `FileActionServiceExecutor(FileActionService service)`: production executor, forwards to the real service.
/// The service is shared (`Arc`), as `FileActionService` is documented to be (pr-core `file_actions::service`).
pub struct FileActionServiceExecutor {
    service: Arc<FileActionService>,
}

impl FileActionServiceExecutor {
    /// C# primary constructor. Stores the handle, does nothing else.
    pub fn new(service: Arc<FileActionService>) -> Self {
        Self { service }
    }
}

impl IFileActionExecutor for FileActionServiceExecutor {
    fn is_busy(&self) -> bool {
        self.service.is_busy()
    }

    fn lacks_recycle_bin(&self, path: &str) -> bool {
        self.service.lacks_recycle_bin(path)
    }

    fn execute(&self, request: &FileActionRequest, cancel: &CancelToken) -> FileActionResult {
        self.service.execute(request, cancel)
    }

    fn execute_group(&self, request: &CaptureGroupActionRequest, cancel: &CancelToken) -> CaptureGroupActionResult {
        self.service.execute_group(request, cancel)
    }

    fn rename(&self, request: &RenameRequest, cancel: &CancelToken) -> FileActionResult {
        self.service.rename(request, cancel)
    }

    fn rename_group(&self, group: &CaptureGroup, typed_stem: &str, cancel: &CancelToken) -> CaptureGroupActionResult {
        self.service.rename_group(group, typed_stem, cancel)
    }
}

/// C# `IFileActionSink` (39 lines): what a file action (move, copy, recycle, undo) needs from the UI.
pub trait IFileActionSink {
    /// C# `SetStatusText(string)`.
    fn set_status_text(&mut self, status: &str);

    /// C# `ShowLateActionStatus(string)` (APP-03). No C# default: the sink must decide. It must not replace text that the
    /// current folder already shows (a load in progress, an error); dropping the hint is allowed.
    fn show_late_action_status(&mut self, status: &str);

    /// C# `OnCatalogChanged(string? removedPath)`: an item was deleted or restored. `removed_path` is the one image that
    /// left the catalog (only it is evicted from the cache; the next image must stay in RAM), `None` when none left.
    fn on_catalog_changed(&mut self, removed_path: Option<&str>);

    /// C# default `EvictCachedPaths(paths)`: the JPEG/RAW partners of a capture that left with the path given to
    /// `on_catalog_changed`. C# default body is empty, so an implementation that caches nothing keeps the default.
    fn evict_cached_paths(&mut self, _paths: &[String]) {}

    /// C# `PresentAsync(int index)`: show the image at `index`. Returns once the request is posted (D-W8-2).
    fn present(&mut self, index: i32);

    /// C# `UpdateSessionPath(string currentPath)`: save the current path in the session.
    fn update_session_path(&mut self, current_path: &str);

    /// C# `NotifyNavigationStateChanged()`: undo history, button states and similar must be refreshed.
    fn notify_navigation_state_changed(&mut self);

    /// FEAT-RENAME (C# default member `OnFilesRenamed`): files were renamed in place and the catalog already lists the new
    /// names (`renames`: old path to new path, every member of a capture; empty after an attempt that changed nothing, where
    /// it only re-arms what `StopForAction` stopped). Returns true when the shown image was kept in place; false when the
    /// caller must present the current entry again. C# default body: `false`.
    fn on_files_renamed(&mut self, renames: &PathKeyMap) -> bool {
        let _ = renames;
        false
    }
}

/// C# `IFolderLoadSink` (56 lines): progress and result events of a folder load, delivered to the UI.
pub trait IFolderLoadSink {
    /// C# `ResetCaches()`: drop the image cache and the scheduler of the previous folder.
    fn reset_caches(&mut self);

    /// C# `OnCatalogReady(folder, count, session)`: the preliminary catalog is ready. `session` was loaded exactly once by
    /// the coordinator and is passed on, not read again.
    fn on_catalog_ready(&mut self, folder: &str, count: i32, session: &SessionState);

    /// C# `PresentAsync(int index, long presentationGeneration)`: show `index`. Returns once the request is posted.
    /// `presentation_generation` is the INV-9 token the completion must still match.
    fn present(&mut self, index: i32, presentation_generation: i64);

    /// C# `OnEmpty(folder, session)`: the folder holds no supported images.
    fn on_empty(&mut self, folder: &str, session: &SessionState);

    /// C# default `OnEmptyWithSubfolders(folder, session, subfolderCount)` (Q-R47): forwards to `on_empty`. An implementation
    /// overrides it to show the "subfolders, but no images here" message.
    fn on_empty_with_subfolders(&mut self, folder: &str, session: &SessionState, _subfolder_count: i32) {
        self.on_empty(folder, session)
    }

    /// C# `OnOrderApplied(count, currentIndex, currentKept)`: the Explorer natural order is in the catalog. When
    /// `current_kept` is true the current image stays on screen at its new `current_index` (no re-present follows), so the
    /// preload around the old index must re-center.
    fn on_order_applied(&mut self, count: i32, current_index: i32, current_kept: bool);

    /// C# default `OnFilesSkipped(folder, skipped)` (IO05): unreadable entries were skipped. The C# default is empty, so the
    /// default here is empty too.
    fn on_files_skipped(&mut self, _folder: &str, _skipped: &[SkippedEntry]) {}

    /// C# `OnUnreadableRemovedAsync(removedPaths, currentRemoved)` (AR16). No default: a forgetful forwarding sink would leave
    /// a removed file in the preload. The sink drops the preload and cache of every path in `removed_paths`, rebuilds the
    /// preload for the new catalog, and presents the new current image when `current_removed` is true.
    fn on_unreadable_removed(&mut self, removed_paths: &[String], current_removed: bool);

    /// C# `OnFailed(folder, exception)`: the folder load failed.
    fn on_failed(&mut self, folder: &str, error: &dyn std::error::Error);
}

/// C# `IFolderPicker` (14 lines): the modal "Move to / Copy to" folder picker, owned by the main window.
pub trait IFolderPicker {
    /// C# `PickFolder(title, initialFolder)`: returns the chosen folder, or `None` when the user cancelled. The initial folder
    /// is ignored when `None` or missing. Runs a nested message loop, so the caller re-checks its state afterwards.
    fn pick_folder(&mut self, title: &str, initial_folder: Option<&str>) -> Option<String>;
}

/// C# `IMoveOverride` (14 lines, AR02a): test seam for the physical move step of `FileActionService` and `UndoService`.
/// Production registers none, so the services move files for real. `Send + Sync` because pr-core's `MoveOverride` is
/// `Arc<dyn Fn(..) + Send + Sync>`.
pub trait IMoveOverride: Send + Sync {
    /// C# `MoveAsync(source, destination)`: `Err` where C# throws.
    fn move_file(&self, source: &Path, destination: &Path) -> io::Result<()>;
}

/// Turns an `IMoveOverride` into the pr-core `MoveOverride` closure (the C# registration of `Func<string, string, Task>`).
/// The result shares the same implementation (`Arc`), so the composition root hands one override to both services.
pub fn into_move_override(over: Arc<dyn IMoveOverride>) -> MoveOverride {
    Arc::new(move |source: &Path, destination: &Path| over.move_file(source, destination))
}

/// C# `IPreloadController` (44 lines): the preload seam around the current image. Methods take `&self`, because the
/// scheduler is shared and uses interior mutability.
pub trait IPreloadController {
    /// Publishes the current catalog before a preload kick (composition amendment A12).
    fn catalog_changed(&self, _catalog: &pr_core::catalog::review_catalog::ReviewCatalog) {}

    /// C# `PreloadAroundAsync(center)`. Returns the handle of the run (the C# `Task`): `wait()` / `is_complete()` replace `await`.
    fn preload_around(&self, center: i32) -> RunHandle;

    /// C# `TryConsumePreloadedKey(key)`: `true` once per warmed key (records a preload hit).
    fn try_consume_preloaded_key(&self, key: &ImageCacheKey) -> bool;

    /// C# `Cancel()`: cancel the unfinished preload work.
    fn cancel(&self);

    /// C# `RemovePreloadedKeysForPath(normalizedPath)`.
    fn remove_preloaded_keys_for_path(&self, normalized_path: &str);

    /// C# `ClearPreloadedKeys()`.
    fn clear_preloaded_keys(&self);

    /// C# default `IsIdle => true`: no preload work in flight. Best-effort diagnostics signal (the perf harness); the default
    /// suits fakes that track no state. The adapter forwards to the scheduler.
    fn is_idle(&self) -> bool {
        true
    }

    /// C# default `NotifyNavigation(index)`: called at the start of every navigation, before the image's own decode. Empty
    /// default (fakes).
    fn notify_navigation(&self, _index: i32) {}

    /// C# default `GetViewerDecodeDelay()`: delay before the viewer decodes a not-yet-cached image (non-zero only during a
    /// key-held burst). Zero default (fakes).
    fn get_viewer_decode_delay(&self) -> Duration {
        Duration::ZERO
    }

    /// C# `(controller as IDisposable)?.Dispose()` (`ShellMainWindow.cs` 401): releases what the controller owns (the scheduler's
    /// workers). Empty default for fakes that own nothing; [`PreloadControllerAdapter`] overrides it. Reconciliation amendment A5.
    fn dispose(&self) {}
}

/// C# `IPresentationSink` (27 lines): where the image presenter sends its results (the image, the status line, trace marks).
pub trait IPresentationSink {
    /// C# `SetCurrentImage(object? image, bool isFileChange)`. `image` is `None` in Compare mode or for an empty folder.
    /// `is_file_change` is true only for the FIRST presentation of a navigation to another file (crossfade, feat/image-crossfade);
    /// false for upgrades of the same image (thumbnail -> preview -> original), for the first image after a folder open, and when
    /// `image` is `None`. The C# default `false` has no Rust equivalent: every caller passes it explicitly.
    ///
    /// Rust: the image is the cheap-to-clone [`PlatformImage`] (the C# `object`), and the facts the C# callback reads back from the
    /// presenter (`CurrentPresentedPath`, `IsSameSourceSwap`, `CurrentOriginalWidth/Height`) ride in `change` (contract W8-07h).
    fn set_current_image(&mut self, image: Option<PlatformImage>, is_file_change: bool, change: &ImageChange<'_>);

    /// The zoom (`None` = Fit) the view model asked for during the last sink call (C# re-entrant `ZoomModeChanged -> SetViewerZoom`), if
    /// any; the presenter pulls it right after `set_current_image` / `apply_initial_view_mode`. Default: no change.
    fn take_viewer_zoom_change(&mut self) -> Option<Option<f64>> {
        None
    }

    /// C# `SetStatusText(string)`.
    fn set_status_text(&mut self, status: &str);

    /// C# `ApplyInitialViewMode()`: Fit, Uniform or 100 %, as the settings say.
    fn apply_initial_view_mode(&mut self);

    /// C# `OnPresented(path)`: the T14a seam, fired when the image is ready to show.
    fn on_presented(&mut self, path: &str);

    /// C# `TracePresented(long token, string kind, long assignedTimestamp)`: the Rendered and Presented marks (ETW/perf).
    fn trace_presented(&mut self, token: i64, kind: &str, assigned_timestamp: i64);
}

/// C# `ISiblingNavigatorSink` (14 lines): the sibling-folder navigation UI.
pub trait ISiblingNavigatorSink {
    /// C# `SetStatusText(string)`.
    fn set_status_text(&mut self, status: &str);

    /// C# `OpenFolderAsync(folder, initialPath)`: returns once the request is posted.
    fn open_folder(&mut self, folder: &str, initial_path: Option<&str>);
}

/// C# `PreloadControllerAdapter` (32 lines): forwards every `IPreloadController` member to the scheduler that the factory
/// returns NOW. The factory is read on every call and never cached (C# `_getScheduler()` on each member), so a replaced
/// scheduler is seen at once and construction never touches it (lazy).
pub struct PreloadControllerAdapter<F>
where
    F: Fn() -> Arc<PreloadScheduler>,
{
    get_scheduler: F,
}

impl<F> PreloadControllerAdapter<F>
where
    F: Fn() -> Arc<PreloadScheduler>,
{
    /// C# constructor. The C# null check (`ArgumentNullException`) has no Rust form: a closure is always present. Does not
    /// call `get_scheduler`.
    pub fn new(get_scheduler: F) -> Self {
        Self { get_scheduler }
    }

    /// C# `Dispose()`: disposes the scheduler that the factory returns now. Afterwards a `preload_around` starts nothing
    /// (the scheduler's own disposed behaviour, pr-imaging `PreloadScheduler::dispose`).
    pub fn dispose(&self) {
        (self.get_scheduler)().dispose();
    }
}

impl<F> IPreloadController for PreloadControllerAdapter<F>
where
    F: Fn() -> Arc<PreloadScheduler>,
{
    fn preload_around(&self, center: i32) -> RunHandle {
        (self.get_scheduler)().preload_around(center)
    }

    fn try_consume_preloaded_key(&self, key: &ImageCacheKey) -> bool {
        (self.get_scheduler)().try_consume_preloaded_key(key)
    }

    fn cancel(&self) {
        (self.get_scheduler)().cancel();
    }

    fn remove_preloaded_keys_for_path(&self, normalized_path: &str) {
        (self.get_scheduler)().remove_preloaded_keys_for_path(normalized_path);
    }

    fn clear_preloaded_keys(&self) {
        (self.get_scheduler)().clear_preloaded_keys();
    }

    fn is_idle(&self) -> bool {
        (self.get_scheduler)().is_idle()
    }

    fn notify_navigation(&self, index: i32) {
        (self.get_scheduler)().notify_navigation(index);
    }

    fn get_viewer_decode_delay(&self) -> Duration {
        (self.get_scheduler)().get_viewer_decode_delay()
    }

    fn dispose(&self) {
        (self.get_scheduler)().dispose();
    }
}

impl std::fmt::Debug for FileActionServiceExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileActionServiceExecutor").finish_non_exhaustive()
    }
}

impl<F> std::fmt::Debug for PreloadControllerAdapter<F>
where
    F: Fn() -> Arc<PreloadScheduler>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreloadControllerAdapter").finish_non_exhaustive()
    }
}
