//! Port of `PhotoReview.App.Shared/ViewModels/MainViewModel.cs` lines 1 to 326 (fields, constructor, `NotifyCurrentImageChanged`,
//! properties, status, skipped entries, capture-pair badge; baseline `d92d6aae`) plus the Rust-only pieces that the split needs:
//! the shared change set, the event inbox, `pump`, and the helpers every other slice calls (contract `contracts/W8-08a.md`).
//! Target: `crates/pr-app/src/viewmodels/main/state.rs` (`viewmodels::main::state`).
//!
//! `MainViewModel` is defined HERE; W8-08b..08e add `impl MainViewModel { .. }` blocks in `navigation`, `commands`, `session` and
//! `notify_move` (fields are `pub(super)`).
//!
//! Design (decisions D-08a-1 to D-08a-6 in the contract):
//! - D-W8-1: every mutator returns a [`ChangeSet`] (properties of this view model, of the viewer, the overlay, the compare view
//!   model and the presenter, plus the former `ImageChanging` / `CatalogChanged` events). No observer lists.
//! - D-W8-2: no `async`. Work that C# awaited is submitted to a worker; its result comes back as a [`MainEvent`] through the
//!   [`CompletionInbox`] and is applied by [`MainViewModel::pump`] on the UI thread. INV-9 tokens stay inside the coordinators.
//! - The coordinators take the catalog and their sink PER CALL. The sinks of this view model are recorders (`notify_move::SinkLog`)
//!   that the view model replays after each call (it needs the catalog, which the coordinator holds `&mut` during the call).
//! - The presenter owns its sink; the sink callbacks cannot reach the view model, so they go through [`PresentationBridge`]
//!   (one shared handle: the viewer plus the "last notified image" state), the single sanctioned shared-state exception.
// Stub only: the porter removes this allow line once every body is real.
#![allow(unused_variables, unused_imports, dead_code)]
// Stub only (contract text in doc comments, boxed seams): the porter fixes the doc layout / introduces type aliases and removes this line.
#![allow(clippy::type_complexity)]

use std::cell::{Ref, RefCell, RefMut};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pr_core::abstractions::{DialogService, FileSystem, Log, SkippedEntry};
use pr_core::catalog::generation::GenerationClock;
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::file_actions::undo::UndoService;
use pr_core::instance::ownership::{FolderOpenDecision, FolderOwnership, FolderOwnershipError};
use pr_core::session::store::SessionState;
use pr_core::session::writer::SessionWriter;
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::store::{SettingsSaveError, SettingsStore};
use pr_core::time::DateTimeUtc;
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::service::PreviewImageService;

use super::commands::{ExternalEditorLauncher, QueuedFileCommand, UndoContext};
use super::navigation::{Deferred, OwnershipRequest, PendingOpen};
use super::notify_move::SinkLog;
use crate::coordinators::duplicate_cleanup::{DuplicateCleanupController, DuplicateCleanupEvent};
use crate::coordinators::file_action_gate::{FileActionGate, GateTurn};
use crate::coordinators::file_actions::recycle::{FileActionController, FileActionEvent, TextSource};
use crate::coordinators::folder_load::state::{FolderLoadCoordinator, FolderLoadEvent};
use crate::coordinators::presenter::state::{ImageChange, ImagePresenter, PresenterChanges};
use crate::coordinators::sibling_navigator::{
    SiblingFolderNavigator, SiblingNavError, SiblingSearch as NavSiblingSearch,
};
use crate::coordinators::traits::{IFolderPicker, IPreloadController};
use crate::coordinators::zoom_detail::{BackgroundRunner, CompletionInbox};
use crate::services::contracts::ClipboardService;
use crate::services::prewarm::{PrewarmPreviews, PrewarmTask};
use crate::viewmodels::compare::{CompareChanges, CompareViewModel};
use crate::viewmodels::format::context::FormatContext;
use crate::viewmodels::info_overlay::{
    FolderInfoLabels, InfoOverlayChanges, InfoOverlayRefresh, InfoOverlayViewModel, SearchOutcome,
    SiblingSearch as OverlaySiblingSearch, ViewInputs,
};
use crate::viewmodels::viewer_state::{ViewerChanges, ViewerState, ViewerUpdate};

// ---- Change notification (D-W8-1, decision D-08a-1) ------------------------------------------------------------------------

/// Every property this view model raised `PropertyChanged` for in C# (verified: the `OnPropertyChanged` / `SetProperty` calls of
/// `MainViewModel.cs`). Properties that are computed but never raised in C# (`CurrentHasComparePair`, `CanOpenInExternalEditor`,
/// `CanCopyCurrentFilePath`, `IsFileActionInProgress`, `Session`, `Settings`) are NOT here: the shell reads them on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MainProperty {
    StatusText,
    StatusNeedsAttention,
    SkippedEntries,
    HasSkippedEntries,
    SkippedWarningText,
    IsStatusPanelVisible,
    FolderTitle,
    FolderText,
    IsExplorerOrderApplied,
    CurrentImage,
    CurrentIndex,
    TotalFiles,
    HasImages,
    CanNavigateNext,
    CanNavigatePrevious,
    CurrentHasCapturePair,
    CapturePairBadge,
    CapturePairBadgeToolTip,
    ExifText,
    IsExifLineVisible,
    ExifAutomationName,
}

impl MainProperty {
    /// All 21 properties in declaration order.
    pub const ALL: [MainProperty; 21] = [
        MainProperty::StatusText,
        MainProperty::StatusNeedsAttention,
        MainProperty::SkippedEntries,
        MainProperty::HasSkippedEntries,
        MainProperty::SkippedWarningText,
        MainProperty::IsStatusPanelVisible,
        MainProperty::FolderTitle,
        MainProperty::FolderText,
        MainProperty::IsExplorerOrderApplied,
        MainProperty::CurrentImage,
        MainProperty::CurrentIndex,
        MainProperty::TotalFiles,
        MainProperty::HasImages,
        MainProperty::CanNavigateNext,
        MainProperty::CanNavigatePrevious,
        MainProperty::CurrentHasCapturePair,
        MainProperty::CapturePairBadge,
        MainProperty::CapturePairBadgeToolTip,
        MainProperty::ExifText,
        MainProperty::IsExifLineVisible,
        MainProperty::ExifAutomationName,
    ];

    /// The C# `nameof(MainViewModel.X)` text (tests and logs).
    pub fn csharp_name(self) -> &'static str {
        todo!()
    }
}

/// The set of [`MainProperty`] values a call changed. `Default` is the empty set. The set loses the raise ORDER and the repeat
/// count of C# (no consumer reads them; the C# pin `NextAsync_RaisesNavigationStateChanged_ExactlyThreeTimes` is a `Different` row).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MainChanges(u32);

impl MainChanges {
    /// The empty set.
    pub const EMPTY: MainChanges = MainChanges(0);

    /// Adds `property`.
    pub fn insert(&mut self, property: MainProperty) {
        todo!()
    }

    /// True when `property` is in the set.
    pub fn contains(self, property: MainProperty) -> bool {
        todo!()
    }

    /// True when the set is empty.
    pub fn is_empty(self) -> bool {
        todo!()
    }

    /// Set union.
    pub fn union(self, other: MainChanges) -> MainChanges {
        todo!()
    }

    /// The members in [`MainProperty`] declaration order.
    pub fn iter(self) -> std::vec::IntoIter<MainProperty> {
        todo!()
    }
}

/// C# `ImageChangingEventArgs` (end of the file). Raised only for the first bitmap of a navigation to a different file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageChangingEventArgs {
    /// C# `IsFileChange`.
    pub is_file_change: bool,
}

/// Everything a call to the view model changed (decision D-08a-1: the shared "ChangeSet" of D-W8-1 for the whole main window).
/// The shell repaints from it. `Default` is "nothing changed".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChangeSet {
    /// Properties of the view model itself.
    pub main: MainChanges,
    /// Properties of the viewer (zoom, fit, fullscreen, scaling ...).
    pub viewer: ViewerChanges,
    /// Properties of the info overlay.
    pub overlay: InfoOverlayChanges,
    /// Properties of the compare view model (read it through `MainViewModel::compare`).
    pub compare: CompareChanges,
    /// What the presenter reported (raw bits, in addition to the mapped `main` bits).
    pub presenter: PresenterChanges,
    /// C# `ImageChanging` event: set when the presenter replaced the bitmap of a navigation to ANOTHER file. The shell must
    /// handle it BEFORE it applies `main.CurrentImage` (C#: raised before `PropertyChanged(CurrentImage)`).
    pub image_changing: Option<ImageChangingEventArgs>,
    /// C# `CatalogChanged` event (raised once per sink callback that changed the catalog; here: at least once in the call).
    pub catalog_changed: bool,
}

impl ChangeSet {
    /// True when nothing changed and no event was raised.
    pub fn is_empty(&self) -> bool {
        todo!()
    }

    /// Adds `other` to `self` (set unions; `image_changing` keeps the first `Some`; `catalog_changed` is or-ed).
    pub fn merge(&mut self, other: ChangeSet) {
        todo!()
    }

    /// A change set holding only `property`.
    pub fn of(property: MainProperty) -> ChangeSet {
        todo!()
    }
}

// ---- Shared handles ------------------------------------------------------------------------------------------------------

/// The settings the view model reads (C# `Settings`: `_settingsOverride or else _settingsStore.Current`). One cell shared by the view
/// model and the closures the composition root builds for the controllers (`get_settings`, `remember_folder`): the C# lambdas
/// `() => Settings` read the live property, so the override must be visible to them.
#[derive(Clone)]
pub struct SettingsSource {
    store: Arc<SettingsStore>,
    override_settings: Arc<Mutex<Option<Arc<AppSettings>>>>,
}

impl SettingsSource {
    /// A source over `store` with no override.
    pub fn new(store: Arc<SettingsStore>) -> Self {
        todo!()
    }

    /// C# `Settings` getter: the override when set, else `store.current()`. A poisoned lock is recovered (R0003).
    pub fn current(&self) -> Arc<AppSettings> {
        todo!()
    }

    /// C# `_settingsStore.Current`, ignoring the override (used by `ShowSettings` and the prewarm).
    pub fn store_current(&self) -> Arc<AppSettings> {
        todo!()
    }

    /// The store (key-name validator, `save`).
    pub fn store(&self) -> &Arc<SettingsStore> {
        todo!()
    }

    /// C# `Settings` setter / test seam: `Some` replaces the reads until cleared; `None` clears the override (and a provisional one).
    pub fn set_override(&self, value: Option<Arc<AppSettings>>) {
        todo!()
    }

    /// True while an override is installed.
    pub fn has_override(&self) -> bool {
        todo!()
    }

    /// Starts an in-memory edit of the current settings. C# mutates the live `AppSettings` object in place (`settings.ShowInfoOverlay = ..`,
    /// `settings.LastMoveToFolder = ..`) and saves it afterwards; readers see the new value before the save. Rust settings are shared
    /// immutable values, so: clone `current()`, apply `mutate`, install the copy as the override NOW, and return the edit.
    pub fn begin_edit(&self, mutate: impl FnOnce(&mut AppSettings)) -> SettingsEdit {
        todo!()
    }

    /// Saves the edited copy with `store.save`. `Ok`: the store holds the value, so the override is removed again unless one was installed
    /// before the edit (then it is replaced by the saved value). `Err`: the override STAYS and is marked provisional ("the overlay still
    /// toggles for this session", "the folder is kept in memory"); the error is returned for the caller's log line.
    pub fn commit_edit(&self, edit: SettingsEdit) -> Result<(), SettingsSaveError> {
        todo!()
    }

    /// Removes a provisional override left by a failed `commit_edit` (called when the Settings dialog reports a change: the store then holds
    /// the user's values, as the C# `Current` replaced by the dialog's save). An explicit `set_override` is not touched.
    pub fn clear_provisional_override(&self) {
        todo!()
    }
}

/// An in-progress settings edit (see [`SettingsSource::begin_edit`]).
#[derive(Debug, Clone)]
pub struct SettingsEdit {
    new: Arc<AppSettings>,
    had_override: bool,
}

impl SettingsEdit {
    /// The edited copy (what readers see until the commit).
    pub fn settings(&self) -> &Arc<AppSettings> {
        &self.new
    }
}

impl fmt::Debug for SettingsSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsSource")
            .field("has_override", &self.has_override())
            .finish_non_exhaustive()
    }
}

/// The viewer, shared between the view model and the presentation bridge (the sink callbacks must change it synchronously, in the
/// C# order, from inside a presenter call). RULE: never hold a `borrow_mut` across a call into the presenter or a coordinator.
#[derive(Debug, Clone)]
pub struct ViewerHandle(Rc<RefCell<ViewerState>>);

impl ViewerHandle {
    /// Wraps `viewer`.
    pub fn new(viewer: ViewerState) -> Self {
        Self(Rc::new(RefCell::new(viewer)))
    }

    /// Shared read access.
    pub fn borrow(&self) -> Ref<'_, ViewerState> {
        self.0.borrow()
    }

    /// Exclusive access (panics when already borrowed: a bug, see the RULE above).
    pub fn borrow_mut(&self) -> RefMut<'_, ViewerState> {
        self.0.borrow_mut()
    }
}

/// What the bridge collected since the view model last asked (see [`PresentationBridge::take_pending`]).
#[derive(Debug, Default)]
pub(super) struct BridgePending {
    /// Viewer changes and `image_changing`; the other fields of the set stay empty.
    pub changes: ChangeSet,
    /// A sink callback asked for `NotifyNavigationStateChanged` (C# `NotifyCurrentImageChanged` end, `NotifyPresentationChanged`).
    pub navigation_dirty: bool,
}

struct BridgeState {
    viewer: ViewerHandle,
    /// C# `_lastNotifiedImagePath`.
    last_notified_image_path: Option<String>,
    pending: BridgePending,
    /// The zoom the viewer asked the loader for during the last sink call (pull channel of contract W8-07h note 7).
    zoom_change: Option<Option<f64>>,
    /// Test/shell hook for C# `ViewerState.SourceSizeSwapping`.
    on_source_size_swapping: Option<Box<dyn FnMut(&ViewerState)>>,
}

/// The four C# `MainViewModelSinkCallbacks` as methods on a shared handle. The composition root (W8-09b) builds the closures of
/// `composition::contracts::MainViewModelSinkCallbacks` around a clone of this handle; the view model holds another clone and drains
/// it with [`MainViewModel::finish_presenter_call`]. UI thread only.
#[derive(Clone)]
pub struct PresentationBridge {
    inner: Rc<RefCell<BridgeState>>,
}

impl PresentationBridge {
    /// A bridge over the shared viewer; no image notified yet.
    pub fn new(viewer: ViewerHandle) -> Self {
        todo!()
    }

    /// The shared viewer.
    pub fn viewer(&self) -> ViewerHandle {
        todo!()
    }

    /// Installs the C# `SourceSizeSwapping` subscriber (called with the viewer state BEFORE the size is swapped).
    pub fn set_source_size_swapping_hook(&self, hook: Option<Box<dyn FnMut(&ViewerState)>>) {
        todo!()
    }

    /// C# `MainViewModel.NotifyCurrentImageChanged(isFileChange)` (lines 140 to 160), run synchronously inside the presenter's
    /// `set_current_image` sink call. In this order: (1) `is_file_change` records `ImageChangingEventArgs` (first one wins);
    /// (2) `new_image = !eq_ignore_case(change.presented_path, last_notified_image_path)` (`None` equals `None`), then the last
    /// path is replaced; (3) `new_image` resets `ForceHighQualityScaling` to false; (4) `change.is_same_source_swap` ->
    /// `swap_source_size(w, h, hook)` else `set_source_size(w, h, new_image)`; a `zoom_mode_changed` update stores
    /// `Some(viewer.effective_zoom())` for [`take_viewer_zoom_change`](Self::take_viewer_zoom_change); (5) the viewer changes are
    /// accumulated; (6) `navigation_dirty = true` (the view model runs `NotifyNavigationStateChanged` after the call).
    pub fn on_set_current_image(&self, change: &ImageChange<'_>, is_file_change: bool) {
        todo!()
    }

    /// C# `SetStatusText: _ => vm?.NotifyPresentationChanged()`: marks `navigation_dirty`.
    pub fn on_set_status_text(&self) {
        todo!()
    }

    /// C# `MainViewModelCompositionRoot.ApplyInitialView(vm, viewport, settingsStore)`: `viewer.apply_initial_view_mode(
    /// settings.initial_view_mode, w, h, settings.click_zoom_percent, settings.keep_zoom_across_images)`; true when it applied.
    /// A `zoom_mode_changed` update stores the pull-channel value; the viewer changes are accumulated.
    pub fn apply_initial_view(&self, viewport: (f64, f64), settings: &AppSettings) -> bool {
        todo!()
    }

    /// Pull side of `IPresentationSink::take_viewer_zoom_change`: returns and clears the stored zoom (`Some(None)` = Fit).
    pub fn take_viewer_zoom_change(&self) -> Option<Option<f64>> {
        todo!()
    }

    /// Returns and clears the accumulated changes.
    pub(super) fn take_pending(&self) -> BridgePending {
        todo!()
    }
}

impl fmt::Debug for PresentationBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PresentationBridge").finish_non_exhaustive()
    }
}

// ---- Seams ---------------------------------------------------------------------------------------------------------------------

/// Hands out the display-text environment (current localizer, number culture, date text) for one call. A closure-style API,
/// because a `FormatContext` borrows from values the source owns. Production reads the CURRENT localizer each call, so a language
/// switch applies at once (D-W8-4); tests give a fixed English one.
pub trait FormatContextSource {
    /// Builds a [`FormatContext`] and calls `run` with it.
    fn with_context(&self, run: &mut dyn FnMut(&FormatContext<'_>));

    /// C# `DateTime.ToLocalTime()` of a catalog last-write time (UTC ticks in, local wall-clock ticks out, same `DateTimeUtc` type as
    /// `ExifLine::modified_local`). No such conversion exists in `pr_core::time`; the platform layer provides it (open question O3).
    fn to_local_time(&self, utc: DateTimeUtc) -> DateTimeUtc;
}

/// `FolderInfoLabels` of the on-image folder block, from the language catalog keys `main.folderInfo.{current,previous,next,pending}`.
pub struct MainFolderInfoLabels {
    texts: Arc<dyn TextSource>,
}

impl MainFolderInfoLabels {
    /// Labels over `texts`.
    pub fn new(texts: Arc<dyn TextSource>) -> Self {
        Self { texts }
    }
}

impl fmt::Debug for MainFolderInfoLabels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainFolderInfoLabels").finish_non_exhaustive()
    }
}

impl FolderInfoLabels for MainFolderInfoLabels {
    fn current(&self, folder_name: &str) -> String {
        todo!()
    }

    fn previous(&self, key: &str, folder_name: &str) -> String {
        todo!()
    }

    fn next(&self, key: &str, folder_name: &str) -> String {
        todo!()
    }

    fn pending(&self) -> String {
        todo!()
    }
}

/// A worker result for the view model (everything C# resumed with `await` on the UI context). Sent from worker threads through the
/// [`CompletionInbox`] (`Send`), applied by [`MainViewModel::pump`].
#[derive(Debug)]
pub enum MainEvent {
    /// Scan / Explorer / probe result of the folder load (`FolderLoadDeps::worker` adapter).
    FolderLoad(FolderLoadEvent),
    /// Executor / undo result (`FileActionDeps::worker` adapter).
    FileAction(FileActionEvent),
    /// Hash / recycle / cache-clear result (`DuplicateCleanupDeps::worker` adapter).
    Duplicate(DuplicateCleanupEvent),
    /// PageUp / PageDown search finished (`SiblingFolderNavigator::find_next_image_folder` on a worker).
    SiblingFound {
        search: NavSiblingSearch,
        result: Result<Option<String>, SiblingNavError>,
    },
    /// Info-overlay sibling search finished (`SiblingFolderNavigator::find_sibling_image_folders` on a worker).
    OverlaySiblings {
        search: OverlaySiblingSearch,
        outcome: SearchOutcome,
    },
    /// `FolderOwnership::before_open` finished (it can block on a pipe, so it runs on a worker).
    Ownership {
        request: OwnershipRequest,
        result: Result<FolderOpenDecision, FolderOwnershipError>,
    },
    /// A queued file command got its turn at the gate (`FileActionGate::run_queued` closure, any thread).
    GateTurn { ticket: u64, turn: GateTurn },
    /// The gate became completely free (`FileActionGate::when_released` waiter).
    GateReleased,
}

/// Adapter the composition root uses to build the `WorkerPort<E>` of each coordinator: runs the job on a [`BackgroundRunner`]
/// thread and sends `wrap(event)` into the view model's inbox. The `WorkerLane` is accepted and ignored (open question O2: lane
/// to thread mapping is R0003 / W8-09b; one thread per job is the default). A panic of the job is the caller's `catch_unwind`.
pub struct MainWorkerPort<E> {
    runner: Arc<dyn BackgroundRunner>,
    inbox: CompletionInbox<MainEvent>,
    wrap: fn(E) -> MainEvent,
}

impl<E> MainWorkerPort<E> {
    /// A port that wraps every event with `wrap`.
    pub fn new(runner: Arc<dyn BackgroundRunner>, inbox: CompletionInbox<MainEvent>, wrap: fn(E) -> MainEvent) -> Self {
        Self { runner, inbox, wrap }
    }
}

impl<E> fmt::Debug for MainWorkerPort<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainWorkerPort").finish_non_exhaustive()
    }
}

impl<E: Send + 'static> crate::coordinators::folder_load::state::WorkerPort<E> for MainWorkerPort<E> {
    fn submit(&self, lane: crate::coordinators::folder_load::state::WorkerLane, job: Box<dyn FnOnce() -> E + Send>) {
        todo!()
    }
}

// ---- Construction -----------------------------------------------------------------------------------------------------------------

/// The collaborators of the C# constructor (`MainViewModel(catalog, clock, folderCoordinator, presenter, viewerState, compare,
/// settingsStore, sessionStore, fileSystem, fileActionService, undoService, dialogService, hashService, previewService,
/// thumbnailCache, sessionWriter, preloadController, naturalComparer, resetCachesAction, metrics, uiScheduler, folderPicker,
/// clipboard)`), changed as follows (decision D-08a-3): the controllers are built by the composition root and passed in (C# AR14
/// built them inside the view model because they needed it as their sink; here the sink is a recorder, so there is no cycle);
/// `compare` lives inside the presenter; `hashService`, `thumbnailCache`, `naturalComparer`, `uiScheduler` and `sessionStore` are
/// only used to build the controllers (or by an unreachable branch) and are not needed here. C# `ArgumentNullException` checks have
/// no Rust form (the types cannot be null).
pub struct MainViewModelDeps {
    pub catalog: ReviewCatalog,
    pub clock: Arc<GenerationClock>,
    pub folder_coordinator: FolderLoadCoordinator,
    pub presenter: ImagePresenter,
    /// The viewer, shared with the bridge (create the bridge from the same handle).
    pub viewer: ViewerHandle,
    pub bridge: PresentationBridge,
    pub settings: SettingsSource,
    pub file_system: Arc<dyn FileSystem>,
    pub file_action_controller: FileActionController,
    /// OC14 gate: file actions queue behind each other, Undo and duplicate cleanup are exclusive.
    pub file_action_gate: FileActionGate,
    pub sibling_navigator: Arc<SiblingFolderNavigator>,
    pub duplicate_controller: DuplicateCleanupController,
    pub dialog_service: Arc<dyn DialogService>,
    pub session_writer: Arc<SessionWriter>,
    /// The same `Arc` that `FileActionDeps::undo_service` holds.
    pub undo_service: Arc<Mutex<UndoService>>,
    pub preview_service: Arc<PreviewImageService>,
    /// C# `IPreloadController? preloadController = null`.
    pub preload_controller: Option<Arc<dyn IPreloadController>>,
    /// C# `Action? resetCachesAction = null`.
    pub reset_caches_action: Option<Box<dyn FnMut()>>,
    pub metrics: Arc<ReviewMetrics>,
    /// C# `IFolderPicker? folderPicker = null`.
    pub folder_picker: Option<Box<dyn IFolderPicker>>,
    /// C# `IClipboardService? clipboard = null`; `None` = `UnavailableClipboardService`.
    pub clipboard: Option<Box<dyn ClipboardService>>,
    /// C# `StartExternalEditor` seam; `None` = [`commands::ProcessEditorLauncher`](super::commands::ProcessEditorLauncher).
    pub external_editor: Option<Box<dyn ExternalEditorLauncher>>,
    pub texts: Arc<dyn TextSource>,
    pub format: Arc<dyn FormatContextSource>,
    /// The inbox every `MainWorkerPort` of this window sends into; the shell's wake function posts `pump`.
    pub inbox: CompletionInbox<MainEvent>,
    /// Runs the sibling searches and the ownership negotiation.
    pub runner: Arc<dyn BackgroundRunner>,
    pub log: Arc<dyn Log>,
}

impl fmt::Debug for MainViewModelDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainViewModelDeps").finish_non_exhaustive()
    }
}

/// C# `MainViewModel`. Owned by the UI thread; not `Send`.
pub struct MainViewModel {
    pub(super) catalog: ReviewCatalog,
    pub(super) clock: Arc<GenerationClock>,
    pub(super) folder_coordinator: FolderLoadCoordinator,
    pub(super) presenter: ImagePresenter,
    pub(super) viewer: ViewerHandle,
    pub(super) bridge: PresentationBridge,
    pub(super) info_overlay: InfoOverlayViewModel<MainFolderInfoLabels>,
    pub(super) settings: SettingsSource,
    pub(super) file_system: Arc<dyn FileSystem>,
    pub(super) file_action_controller: FileActionController,
    pub(super) file_action_gate: FileActionGate,
    pub(super) sibling_navigator: Arc<SiblingFolderNavigator>,
    pub(super) duplicate_controller: DuplicateCleanupController,
    pub(super) dialog_service: Arc<dyn DialogService>,
    pub(super) session_writer: Arc<SessionWriter>,
    pub(super) undo_service: Arc<Mutex<UndoService>>,
    pub(super) preview_service: Arc<PreviewImageService>,
    pub(super) preload_controller: Option<Arc<dyn IPreloadController>>,
    pub(super) reset_caches_action: Option<Box<dyn FnMut()>>,
    pub(super) metrics: Arc<ReviewMetrics>,
    pub(super) folder_picker: Option<Box<dyn IFolderPicker>>,
    pub(super) clipboard: Box<dyn ClipboardService>,
    pub(super) external_editor: Box<dyn ExternalEditorLauncher>,
    pub(super) format: Arc<dyn FormatContextSource>,
    pub(super) inbox: CompletionInbox<MainEvent>,
    pub(super) runner: Arc<dyn BackgroundRunner>,
    pub(super) log: Arc<dyn Log>,
    /// The recorder every coordinator sink call goes to (`notify_move`).
    pub(super) sink: SinkLog,
    // ---- C# fields ----
    pub(super) folder_title: String,
    pub(super) folder_text: String,
    /// C# `_folderTextFolder` / `_folderTextCount`: `FolderText` is rendered from this state, never parsed back.
    pub(super) folder_text_folder: Option<String>,
    pub(super) folder_text_count: i32,
    pub(super) explorer_order_applied: bool,
    /// C# `_statusText` (event text; empty = "show the presenter's line").
    pub(super) status_text: String,
    pub(super) current_session: Option<SessionState>,
    pub(super) is_closed: bool,
    pub(super) skipped_entries: Vec<SkippedEntry>,
    pub(super) instance_label: Option<String>,
    /// C# `_folderLoadEpoch` (08c range; held here because 08b reads it).
    pub(super) folder_load_epoch: i64,
    /// C# `_shownFolder`.
    pub(super) shown_folder: Option<String>,
    /// C# `FolderOwnership` property (Q-R18).
    pub(super) folder_ownership: Option<Arc<dyn FolderOwnership>>,
    // ---- Rust-only state of the event machines ----
    /// The open whose load is running (`navigation`).
    pub(super) pending_open: Option<PendingOpen>,
    /// Commands that wait for the Explorer order (`WaitForPendingExplorerOrderAsync`), in submission order.
    pub(super) deferred: VecDeque<Deferred>,
    /// File commands waiting for their gate turn, by ticket (`commands`).
    pub(super) queued_file_commands: HashMap<u64, QueuedFileCommand>,
    pub(super) next_ticket: u64,
    /// The turn of the file command that is running (released when the controller reports `Done`).
    pub(super) active_gate_turn: Option<GateTurn>,
    /// A `present` requested by a controller sink is outstanding; `on_present_finished` is owed to the file-action controller.
    pub(super) controller_present_outstanding: bool,
    /// Settings-triggered reloads that wait for the gate (`session`); each one opens the folder when the gate is free.
    pub(super) settings_reloads_waiting: u32,
    /// The turn of a running duplicate cleanup (`remove_duplicates`), released when the controller reports `Done` / `Canceled`.
    pub(super) duplicate_gate_turn: Option<GateTurn>,
    /// Set while an undo runs (its turn is `active_gate_turn`).
    pub(super) undo_context: Option<UndoContext>,
}

impl fmt::Debug for MainViewModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MainViewModel")
            .field("folder_title", &self.folder_title)
            .field("status_text", &self.status_text)
            .field("is_closed", &self.is_closed)
            .finish_non_exhaustive()
    }
}

impl MainViewModel {
    /// C# constructor (lines 55 to 138): stores the collaborators, then (1) `viewer.set_scaling_quality(settings.scaling_quality_or_default())`,
    /// (2) `viewer.set_zoom_step(settings.keyboard_zoom_step_percent as f64 / 100.0)` (Q-R41), (3) `presenter.set_viewer_zoom(viewer.effective_zoom())`.
    /// Starts no work, reads no file, makes no sink call. `folder_title` starts as the localized app title (`app.title`).
    pub fn new(deps: MainViewModelDeps) -> Self {
        todo!()
    }

    // ---- accessors (C# lines 174 to 296) ----

    /// C# `Metrics`.
    pub fn metrics(&self) -> &Arc<ReviewMetrics> {
        todo!()
    }

    /// C# `Presenter` (read-only; mutations go through the view model so the environment is lent).
    pub fn presenter(&self) -> &ImagePresenter {
        todo!()
    }

    /// C# `Presenter.DeferNextPreloadKick = value` (P-1 startup).
    pub fn set_defer_next_preload_kick(&mut self, value: bool) {
        todo!()
    }

    /// C# `Compare` (the instance lives in the presenter).
    pub fn compare(&self) -> &CompareViewModel {
        todo!()
    }

    /// C# `Viewer`.
    pub fn viewer(&self) -> &ViewerHandle {
        todo!()
    }

    /// The bridge the sink callbacks use (for the composition root).
    pub fn bridge(&self) -> &PresentationBridge {
        todo!()
    }

    /// C# `Catalog` (read-only).
    pub fn catalog(&self) -> &ReviewCatalog {
        todo!()
    }

    /// C# `Session`.
    pub fn session(&self) -> Option<&SessionState> {
        todo!()
    }

    /// C# `PreloadController`.
    pub fn preload_controller(&self) -> Option<&Arc<dyn IPreloadController>> {
        todo!()
    }

    /// C# `PreviewService`.
    pub fn preview_service(&self) -> &Arc<PreviewImageService> {
        todo!()
    }

    /// C# `UndoService`.
    pub fn undo_service(&self) -> &Arc<Mutex<UndoService>> {
        todo!()
    }

    /// C# `PrewarmInitialImage(path)` (line 183): `services::prewarm::start(Some(preview_service), &settings.store_current(), Some(path), None)`
    /// (the STORE value, not the override). Returns `None` when nothing was started.
    pub fn prewarm_initial_image(&self, path: &str) -> Option<PrewarmTask> {
        todo!()
    }

    // ---- settings ----

    /// C# `Settings` getter.
    pub fn settings(&self) -> Arc<AppSettings> {
        todo!()
    }

    /// C# `Settings` setter: installs the override, then `InfoOverlay.Refresh()` (the overlay changes and a possible sibling search
    /// are applied through `apply_overlay_refresh`).
    pub fn set_settings(&mut self, value: Arc<AppSettings>) -> ChangeSet {
        todo!()
    }

    /// C# `InfoOverlay`.
    pub fn info_overlay(&self) -> &InfoOverlayViewModel<MainFolderInfoLabels> {
        todo!()
    }

    // ---- status ----

    /// C# `IsStatusPanelVisible` = `InfoOverlay.IsFileInfoVisible || IsExifLineVisible || HasSkippedEntries`
    /// (`is_exif_line_visible` is W8-08e).
    pub fn is_status_panel_visible(&self) -> bool {
        todo!()
    }

    /// C# `FolderTitle`.
    pub fn folder_title(&self) -> &str {
        todo!()
    }

    /// C# `FolderText`.
    pub fn folder_text(&self) -> &str {
        todo!()
    }

    /// C# `IsExplorerOrderApplied`.
    pub fn is_explorer_order_applied(&self) -> bool {
        todo!()
    }

    /// C# `StatusText` getter. Precedence: non-empty event text; else, while the RAW-decode indicator is visible
    /// (`presenter.zoom_detail().is_raw_decode_indicator_visible()`) and the presenter does not need attention, the localized
    /// "decoding RAW" line (`status::decoding_raw`); else the presenter's line.
    pub fn status_text(&self) -> String {
        todo!()
    }

    /// C# `StatusText` setter (`SetProperty`): stores the event text; `main.StatusText` is in the result only when the stored
    /// text differs (ordinal). `StatusNeedsAttention` is NOT raised by the setter (as in C#).
    pub fn set_status_text(&mut self, value: &str) -> ChangeSet {
        todo!()
    }

    /// C# `StatusNeedsAttention` (Q-R34): RAW indicator visible, or non-empty event text, or the presenter needs attention, or
    /// skipped entries exist.
    pub fn status_needs_attention(&self) -> bool {
        todo!()
    }

    // ---- skipped entries (IO05) ----

    /// C# `SkippedEntries`.
    pub fn skipped_entries(&self) -> &[SkippedEntry] {
        todo!()
    }

    /// C# `HasSkippedEntries`.
    pub fn has_skipped_entries(&self) -> bool {
        todo!()
    }

    /// C# `SkippedWarningText`: empty without entries, else the plural catalog text `main.skipped.warning` with the count.
    pub fn skipped_warning_text(&self) -> String {
        todo!()
    }

    /// C# `SetSkippedEntries` (private): stores the list and reports `SkippedEntries`, `HasSkippedEntries`, `SkippedWarningText`,
    /// `IsStatusPanelVisible` (always, even for an equal list).
    pub(super) fn set_skipped_entries(&mut self, entries: Vec<SkippedEntry>) -> ChangeSet {
        todo!()
    }

    // ---- catalog-derived state ----

    /// C# `CurrentImage` (the presenter's bitmap).
    pub fn current_image(&self) -> Option<&PlatformImage> {
        todo!()
    }

    /// C# `CurrentIndex`.
    pub fn current_index(&self) -> i32 {
        todo!()
    }

    /// C# `TotalFiles`.
    pub fn total_files(&self) -> usize {
        todo!()
    }

    /// C# `HasImages`.
    pub fn has_images(&self) -> bool {
        todo!()
    }

    /// C# `CanNavigateNext`.
    pub fn can_navigate_next(&self) -> bool {
        todo!()
    }

    /// C# `CanNavigatePrevious`.
    pub fn can_navigate_previous(&self) -> bool {
        todo!()
    }

    /// C# `CurrentHasComparePair`: `current_index` in range and `presenter.has_compare_pair(catalog, path_at(current_index))`.
    /// `&mut self` because the presenter caches its compare index. A path error (C#: exception) logs and returns false.
    pub fn current_has_compare_pair(&mut self) -> bool {
        todo!()
    }

    /// C# `CurrentHasCapturePair`.
    pub fn current_has_capture_pair(&self) -> bool {
        todo!()
    }

    /// C# `CapturePairBadge`: empty without a capture group; else `main.capturePair.badge.raw` when the presented path equals the
    /// group's RAW path (OrdinalIgnoreCase), otherwise `main.capturePair.badge.jpeg`.
    pub fn capture_pair_badge(&self) -> String {
        todo!()
    }

    /// C# `CapturePairBadgeToolTip`: `main.capturePair.badge.tooltipBound` with the TRIMMED key when `settings.shortcuts.toggle_capture_member`
    /// is not blank and `settings.store().key_names().is_valid_key_name(configured)` (the untrimmed text is validated);
    /// else `main.capturePair.badge.tooltipUnbound`.
    pub fn capture_pair_badge_tool_tip(&self) -> String {
        todo!()
    }

    // ---- lifetime ----

    /// True after `close_session` (W8-08d).
    pub fn is_closed(&self) -> bool {
        todo!()
    }

    /// True when nothing is outstanding: no folder load or ownership negotiation, no queued or running file command, no deferred
    /// command, no presenter work, no inbox event. Tests loop `while !vm.is_idle() { inbox.wait_ready(..); vm.pump(); }` (replaces the
    /// C# `FolderLoadTask`, `CompareToggleTask`, `SettingsRefreshTask` seams).
    pub fn is_idle(&self) -> bool {
        todo!()
    }

    // ---- the event pump ----

    /// Applies every completion that arrived since the last call, on the UI thread, until nothing is left: first the view model's
    /// inbox (in arrival order, `dispatch`), then `presenter.pump(env)`, then the debts that follow (`on_present_finished` to the
    /// file-action controller, release of the deferred commands). Returns the union of all changes.
    pub fn pump(&mut self) -> ChangeSet {
        todo!()
    }

    /// Routes one [`MainEvent`] to its handler: `FolderLoad` -> `navigation::handle_folder_event`, `FileAction` ->
    /// `commands::handle_file_action_event`, `Duplicate` -> `commands::handle_duplicate_event`, `SiblingFound` ->
    /// `navigation::on_sibling_found`, `OverlaySiblings` -> `on_overlay_siblings` (this file), `Ownership` ->
    /// `navigation::on_ownership_decided`, `GateTurn` -> `commands::on_gate_turn`, `GateReleased` -> `commands::on_gate_released`.
    pub(super) fn dispatch(&mut self, event: MainEvent) -> ChangeSet {
        todo!()
    }

    // ---- helpers shared by the five slices ----

    /// Runs `f` with the presenter and its per-call environment (catalog, settings snapshot, session, format context) lent
    /// (the split borrow of `catalog`, `presenter` and `current_session`; the pattern of contract W8-07h note 2).
    pub(super) fn with_presenter<R>(
        &mut self,
        f: impl FnOnce(&mut ImagePresenter, &mut crate::coordinators::presenter::state::PresenterEnv<'_>) -> R,
    ) -> R {
        todo!()
    }

    /// Runs `f` with the format context (D-W8-4).
    pub(super) fn with_ctx<R>(&self, f: impl FnOnce(&FormatContext<'_>) -> R) -> R {
        todo!()
    }

    /// Turns what a presenter call reported into a [`ChangeSet`]: drains the bridge (viewer changes, `image_changing`), runs
    /// `notify_navigation_state_changed` when a sink callback asked for it (or `PHOTO_INFO` / `STATUS` / `CURRENT_IMAGE` bits are set),
    /// maps `RAW_DECODE_INDICATOR` to `StatusText` + `StatusNeedsAttention`, forwards `COMPARE`, and refreshes the overlay when the
    /// viewer's `DisplayZoomPercent` changed (C# `_viewerState.PropertyChanged` hook, line 126).
    pub(super) fn finish_presenter_call(&mut self, changes: PresenterChanges) -> ChangeSet {
        todo!()
    }

    /// The two C# viewer hooks (lines 117 to 129) for a change made by the view model: `zoom_mode_changed` ->
    /// `presenter.set_viewer_zoom(viewer.effective_zoom())` (lines 117/118); `DisplayZoomPercent` in the changes ->
    /// `InfoOverlay.Refresh()` (line 127). Returns the combined set.
    pub(super) fn apply_viewer_update(&mut self, update: ViewerUpdate) -> ChangeSet {
        todo!()
    }

    /// Applies an overlay refresh: `IsFileInfoVisible` in the changes adds `main.IsStatusPanelVisible` (C# line 112); a requested
    /// sibling search is submitted to the runner (`find_sibling_image_folders` -> `MainEvent::OverlaySiblings`).
    pub(super) fn apply_overlay_refresh(&mut self, refresh: InfoOverlayRefresh) -> ChangeSet {
        todo!()
    }

    /// The overlay's inputs now (`has_image`, `viewer.display_zoom_percent()`), C# `() => HasImages`, `() => DisplayZoomPercent`.
    pub(super) fn view_inputs(&self) -> ViewInputs {
        todo!()
    }

    /// `InfoOverlay.Refresh()` with the current settings and inputs, applied.
    pub(super) fn refresh_overlay(&mut self) -> ChangeSet {
        todo!()
    }

    /// Completion of an overlay sibling search (`MainEvent::OverlaySiblings`).
    pub(super) fn on_overlay_siblings(&mut self, search: OverlaySiblingSearch, outcome: SearchOutcome) -> ChangeSet {
        todo!()
    }

    /// C# `PersistSession(session)` (line 976, hoisted from the 08d range because 08b and 08e call it): `session_writer.update(&session)`.
    /// The C# fallback `_sessionStore.Save` is unreachable (the writer is a required constructor argument) and not ported.
    pub(super) fn persist_session(&self) {
        todo!()
    }

    /// C# `PresentedPathOfCurrentEntry()` (line 944, hoisted from the 08d range because 08b and 08d call it): the presenter's
    /// current path when the current entry has a capture group, a presented path exists and `catalog.index_of(presented) ==
    /// current_index`; else `None` (stale after a catalog change, or no pair).
    pub(super) fn presented_path_of_current_entry(&self) -> Option<String> {
        todo!()
    }

    /// Starts a presentation: `presenter.present(env, request)` and `finish_presenter_call`. A `PresentError::PathNotInEntry`
    /// (the C# `ArgumentException`) is logged and changes nothing (callers pre-validate the override).
    pub(super) fn present_request(
        &mut self,
        request: crate::coordinators::presenter::state::PresentRequest,
    ) -> ChangeSet {
        todo!()
    }
}
