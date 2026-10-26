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
use pr_l10n::template::LocArg;

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
use crate::overlay::model::OverlayModel;
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
        match self {
            Self::StatusText => "StatusText",
            Self::StatusNeedsAttention => "StatusNeedsAttention",
            Self::SkippedEntries => "SkippedEntries",
            Self::HasSkippedEntries => "HasSkippedEntries",
            Self::SkippedWarningText => "SkippedWarningText",
            Self::IsStatusPanelVisible => "IsStatusPanelVisible",
            Self::FolderTitle => "FolderTitle",
            Self::FolderText => "FolderText",
            Self::IsExplorerOrderApplied => "IsExplorerOrderApplied",
            Self::CurrentImage => "CurrentImage",
            Self::CurrentIndex => "CurrentIndex",
            Self::TotalFiles => "TotalFiles",
            Self::HasImages => "HasImages",
            Self::CanNavigateNext => "CanNavigateNext",
            Self::CanNavigatePrevious => "CanNavigatePrevious",
            Self::CurrentHasCapturePair => "CurrentHasCapturePair",
            Self::CapturePairBadge => "CapturePairBadge",
            Self::CapturePairBadgeToolTip => "CapturePairBadgeToolTip",
            Self::ExifText => "ExifText",
            Self::IsExifLineVisible => "IsExifLineVisible",
            Self::ExifAutomationName => "ExifAutomationName",
        }
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
        self.0 |= 1_u32 << (property as u32);
    }

    /// True when `property` is in the set.
    pub fn contains(self, property: MainProperty) -> bool {
        self.0 & (1_u32 << (property as u32)) != 0
    }

    /// True when the set is empty.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Set union.
    pub fn union(self, other: MainChanges) -> MainChanges {
        MainChanges(self.0 | other.0)
    }

    /// The members in [`MainProperty`] declaration order.
    pub fn iter(self) -> std::vec::IntoIter<MainProperty> {
        MainProperty::ALL
            .into_iter()
            .filter(|property| self.contains(*property))
            .collect::<Vec<_>>()
            .into_iter()
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
        self.main.is_empty()
            && self.viewer.is_empty()
            && self.overlay.is_empty()
            && self.compare.is_empty()
            && self.presenter.is_empty()
            && self.image_changing.is_none()
            && !self.catalog_changed
    }

    /// Adds `other` to `self` (set unions; `image_changing` keeps the first `Some`; `catalog_changed` is or-ed).
    pub fn merge(&mut self, other: ChangeSet) {
        self.main = self.main.union(other.main);
        self.viewer = self.viewer.union(other.viewer);
        for property in other.overlay.iter() {
            self.overlay.insert(property);
        }
        self.compare = self.compare.union(other.compare);
        self.presenter.0 |= other.presenter.0;
        if self.image_changing.is_none() {
            self.image_changing = other.image_changing;
        }
        self.catalog_changed |= other.catalog_changed;
    }

    /// A change set holding only `property`.
    pub fn of(property: MainProperty) -> ChangeSet {
        let mut result = Self::default();
        result.main.insert(property);
        result
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
    provisional: Arc<Mutex<bool>>,
}

impl SettingsSource {
    /// A source over `store` with no override.
    pub fn new(store: Arc<SettingsStore>) -> Self {
        Self {
            store,
            override_settings: Arc::new(Mutex::new(None)),
            provisional: Arc::new(Mutex::new(false)),
        }
    }

    /// C# `Settings` getter: the override when set, else `store.current()`. A poisoned lock is recovered (R0003).
    pub fn current(&self) -> Arc<AppSettings> {
        self.override_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .cloned()
            .unwrap_or_else(|| self.store.current())
    }

    /// C# `_settingsStore.Current`, ignoring the override (used by `ShowSettings` and the prewarm).
    pub fn store_current(&self) -> Arc<AppSettings> {
        self.store.current()
    }

    /// The store (key-name validator, `save`).
    pub fn store(&self) -> &Arc<SettingsStore> {
        &self.store
    }

    /// C# `Settings` setter / test seam: `Some` replaces the reads until cleared; `None` clears the override (and a provisional one).
    pub fn set_override(&self, value: Option<Arc<AppSettings>>) {
        *self
            .override_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = value;
        *self
            .provisional
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    }

    /// True while an override is installed.
    pub fn has_override(&self) -> bool {
        self.override_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    /// Starts an in-memory edit of the current settings. C# mutates the live `AppSettings` object in place (`settings.ShowInfoOverlay = ..`,
    /// `settings.LastMoveToFolder = ..`) and saves it afterwards; readers see the new value before the save. Rust settings are shared
    /// immutable values, so: clone `current()`, apply `mutate`, install the copy as the override NOW, and return the edit.
    pub fn begin_edit(&self, mutate: impl FnOnce(&mut AppSettings)) -> SettingsEdit {
        let had_override = self.has_override();
        let mut new = (*self.current()).clone();
        mutate(&mut new);
        let new = Arc::new(new);
        *self
            .override_settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&new));
        SettingsEdit { new, had_override }
    }

    /// Saves the edited copy with `store.save`. `Ok`: the store holds the value, so the override is removed again unless one was installed
    /// before the edit (then it is replaced by the saved value). `Err`: the override STAYS and is marked provisional ("the overlay still
    /// toggles for this session", "the folder is kept in memory"); the error is returned for the caller's log line.
    pub fn commit_edit(&self, edit: SettingsEdit) -> Result<(), SettingsSaveError> {
        match self.store.save((*edit.new).clone()) {
            Ok(saved) => {
                *self
                    .override_settings
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = edit.had_override.then_some(saved);
                *self
                    .provisional
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
                Ok(())
            }
            Err(error) => {
                *self
                    .provisional
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                Err(error)
            }
        }
    }

    /// Removes a provisional override left by a failed `commit_edit` (called when the Settings dialog reports a change: the store then holds
    /// the user's values, as the C# `Current` replaced by the dialog's save). An explicit `set_override` is not touched.
    pub fn clear_provisional_override(&self) {
        let mut provisional = self
            .provisional
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *provisional {
            *self
                .override_settings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            *provisional = false;
        }
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
    on_source_size_swapping: Option<SourceSizeSwappingHook>,
}

type SourceSizeSwappingHook = Box<dyn FnMut(&ViewerState)>;

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
        Self {
            inner: Rc::new(RefCell::new(BridgeState {
                viewer,
                last_notified_image_path: None,
                pending: BridgePending::default(),
                zoom_change: None,
                on_source_size_swapping: None,
            })),
        }
    }

    /// The shared viewer.
    pub fn viewer(&self) -> ViewerHandle {
        self.inner.borrow().viewer.clone()
    }

    /// Installs the C# `SourceSizeSwapping` subscriber (called with the viewer state BEFORE the size is swapped).
    // The public callback type is fixed by the W8-08a contract.
    #[allow(clippy::type_complexity)]
    pub fn set_source_size_swapping_hook(&self, hook: Option<Box<dyn FnMut(&ViewerState)>>) {
        self.inner.borrow_mut().on_source_size_swapping = hook;
    }

    /// C# `MainViewModel.NotifyCurrentImageChanged(isFileChange)` (lines 140 to 160), run synchronously inside the presenter's
    /// `set_current_image` sink call. In this order: (1) `is_file_change` records `ImageChangingEventArgs` (first one wins);
    /// (2) `new_image = !eq_ignore_case(change.presented_path, last_notified_image_path)` (`None` equals `None`), then the last
    /// path is replaced; (3) `new_image` resets `ForceHighQualityScaling` to false; (4) `change.is_same_source_swap` ->
    /// `swap_source_size(w, h, hook)` else `set_source_size(w, h, new_image)`; a `zoom_mode_changed` update stores
    /// `Some(viewer.effective_zoom())` for [`take_viewer_zoom_change`](Self::take_viewer_zoom_change); (5) the viewer changes are
    /// accumulated; (6) `navigation_dirty = true` (the view model runs `NotifyNavigationStateChanged` after the call).
    pub fn on_set_current_image(&self, change: &ImageChange<'_>, is_file_change: bool) {
        let mut state = self.inner.borrow_mut();
        if is_file_change && state.pending.changes.image_changing.is_none() {
            state.pending.changes.image_changing = Some(ImageChangingEventArgs { is_file_change });
        }
        let new_image = match (change.presented_path, state.last_notified_image_path.as_deref()) {
            (None, None) => false,
            (Some(a), Some(b)) => !pr_core::text::eq_ignore_case(a, b),
            _ => true,
        };
        state.last_notified_image_path = change.presented_path.map(str::to_owned);
        let viewer = state.viewer.clone();
        let update = {
            let mut viewer = viewer.borrow_mut();
            if new_image {
                let changes = viewer.set_force_high_quality_scaling(false);
                state.pending.changes.viewer = state.pending.changes.viewer.union(changes);
            }
            if change.is_same_source_swap {
                viewer.swap_source_size(change.original_width, change.original_height, &mut |before| {
                    if let Some(hook) = state.on_source_size_swapping.as_mut() {
                        hook(before);
                    }
                })
            } else {
                viewer.set_source_size(change.original_width, change.original_height, new_image)
            }
        };
        state.pending.changes.viewer = state.pending.changes.viewer.union(update.changes);
        if update.zoom_mode_changed {
            state.zoom_change = Some(viewer.borrow().effective_zoom());
        }
        state.pending.navigation_dirty = true;
    }

    /// C# `SetStatusText: _ => vm?.NotifyPresentationChanged()`: marks `navigation_dirty`.
    pub fn on_set_status_text(&self) {
        self.inner.borrow_mut().pending.navigation_dirty = true;
    }

    /// C# `MainViewModelCompositionRoot.ApplyInitialView(vm, viewport, settingsStore)`: `viewer.apply_initial_view_mode(
    /// settings.initial_view_mode, w, h, settings.click_zoom_percent, settings.keep_zoom_across_images)`; true when it applied.
    /// A `zoom_mode_changed` update stores the pull-channel value; the viewer changes are accumulated.
    pub fn apply_initial_view(&self, viewport: (f64, f64), settings: &AppSettings) -> bool {
        let mut state = self.inner.borrow_mut();
        let viewer = state.viewer.clone();
        let update = viewer.borrow_mut().apply_initial_view_mode(
            settings.initial_view_mode,
            viewport.0,
            viewport.1,
            settings.click_zoom_percent,
            settings.keep_zoom_across_images,
        );
        if let Some(update) = update {
            state.pending.changes.viewer = state.pending.changes.viewer.union(update.changes);
            if update.zoom_mode_changed {
                state.zoom_change = Some(viewer.borrow().effective_zoom());
            }
            true
        } else {
            false
        }
    }

    /// Pull side of `IPresentationSink::take_viewer_zoom_change`: returns and clears the stored zoom (`Some(None)` = Fit).
    pub fn take_viewer_zoom_change(&self) -> Option<Option<f64>> {
        self.inner.borrow_mut().zoom_change.take()
    }

    /// Returns and clears the accumulated changes.
    pub(super) fn take_pending(&self) -> BridgePending {
        std::mem::take(&mut self.inner.borrow_mut().pending)
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
        self.texts
            .format("main.folderInfo.current", &[LocArg::new("folderName", folder_name)])
    }

    fn previous(&self, key: &str, folder_name: &str) -> String {
        self.texts.format(
            "main.folderInfo.previous",
            &[LocArg::new("key", key), LocArg::new("folderName", folder_name)],
        )
    }

    fn next(&self, key: &str, folder_name: &str) -> String {
        self.texts.format(
            "main.folderInfo.next",
            &[LocArg::new("key", key), LocArg::new("folderName", folder_name)],
        )
    }

    fn pending(&self) -> String {
        self.texts.get("main.folderInfo.pending")
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
        let _ = lane;
        let inbox = self.inbox.clone();
        let wrap = self.wrap;
        self.runner
            .run("main-view-model-worker", Box::new(move || inbox.send(wrap(job()))));
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
    /// Worker jobs of this view model whose result event has not been applied yet: the ownership negotiation, the PageUp / PageDown
    /// search and the info-overlay sibling search (they run on `runner`, so nothing else says they are outstanding).
    pub(super) pending_worker_jobs: u32,
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
        let settings = deps.settings.current();
        {
            let mut viewer = deps.viewer.borrow_mut();
            viewer.set_scaling_quality(settings.scaling_quality_or_default());
            viewer.set_zoom_step(f64::from(settings.keyboard_zoom_step_percent) / 100.0);
        }
        let mut presenter = deps.presenter;
        presenter.set_viewer_zoom(deps.viewer.borrow().effective_zoom());
        let title = deps.texts.get("app.title");
        let info_overlay = InfoOverlayViewModel::new(MainFolderInfoLabels::new(Arc::clone(&deps.texts)));
        Self {
            catalog: deps.catalog,
            clock: deps.clock,
            folder_coordinator: deps.folder_coordinator,
            presenter,
            viewer: deps.viewer,
            bridge: deps.bridge,
            info_overlay,
            settings: deps.settings,
            file_system: deps.file_system,
            file_action_controller: deps.file_action_controller,
            file_action_gate: deps.file_action_gate,
            sibling_navigator: deps.sibling_navigator,
            duplicate_controller: deps.duplicate_controller,
            dialog_service: deps.dialog_service,
            session_writer: deps.session_writer,
            undo_service: deps.undo_service,
            preview_service: deps.preview_service,
            preload_controller: deps.preload_controller,
            reset_caches_action: deps.reset_caches_action,
            metrics: deps.metrics,
            folder_picker: deps.folder_picker,
            clipboard: deps
                .clipboard
                .unwrap_or_else(|| Box::new(crate::services::contracts::UnavailableClipboardService)),
            external_editor: deps
                .external_editor
                .unwrap_or_else(|| Box::new(super::commands::ProcessEditorLauncher)),
            format: deps.format,
            inbox: deps.inbox,
            runner: deps.runner,
            log: deps.log,
            sink: SinkLog::new(),
            folder_title: title,
            folder_text: String::new(),
            folder_text_folder: None,
            folder_text_count: 0,
            explorer_order_applied: false,
            status_text: String::new(),
            current_session: None,
            is_closed: false,
            skipped_entries: Vec::new(),
            instance_label: None,
            folder_load_epoch: 0,
            shown_folder: None,
            folder_ownership: None,
            pending_open: None,
            deferred: VecDeque::new(),
            queued_file_commands: HashMap::new(),
            next_ticket: 0,
            active_gate_turn: None,
            controller_present_outstanding: false,
            settings_reloads_waiting: 0,
            pending_worker_jobs: 0,
            duplicate_gate_turn: None,
            undo_context: None,
        }
    }

    // ---- accessors (C# lines 174 to 296) ----

    /// C# `Metrics`.
    pub fn metrics(&self) -> &Arc<ReviewMetrics> {
        &self.metrics
    }

    /// C# `Presenter` (read-only; mutations go through the view model so the environment is lent).
    pub fn presenter(&self) -> &ImagePresenter {
        &self.presenter
    }

    /// C# `Presenter.DeferNextPreloadKick = value` (P-1 startup).
    pub fn set_defer_next_preload_kick(&mut self, value: bool) {
        self.presenter.set_defer_next_preload_kick(value);
    }

    /// C# `Compare` (the instance lives in the presenter).
    pub fn compare(&self) -> &CompareViewModel {
        self.presenter.compare()
    }

    /// C# `Viewer`.
    pub fn viewer(&self) -> &ViewerHandle {
        &self.viewer
    }

    /// The bridge the sink callbacks use (for the composition root).
    pub fn bridge(&self) -> &PresentationBridge {
        &self.bridge
    }

    /// C# `Catalog` (read-only).
    pub fn catalog(&self) -> &ReviewCatalog {
        &self.catalog
    }

    /// C# `Session`.
    pub fn session(&self) -> Option<&SessionState> {
        self.current_session.as_ref()
    }

    /// C# `PreloadController`.
    pub fn preload_controller(&self) -> Option<&Arc<dyn IPreloadController>> {
        self.preload_controller.as_ref()
    }

    /// C# `PreviewService`.
    pub fn preview_service(&self) -> &Arc<PreviewImageService> {
        &self.preview_service
    }

    /// C# `UndoService`.
    pub fn undo_service(&self) -> &Arc<Mutex<UndoService>> {
        &self.undo_service
    }

    /// C# `PrewarmInitialImage(path)` (line 183): `services::prewarm::start(Some(preview_service), &settings.store_current(), Some(path), None)`
    /// (the STORE value, not the override). Returns `None` when nothing was started.
    pub fn prewarm_initial_image(&self, path: &str) -> Option<PrewarmTask> {
        let previews: Arc<dyn PrewarmPreviews> = self.preview_service.clone();
        crate::services::prewarm::start(Some(previews), &self.settings.store_current(), Some(path), None)
    }

    // ---- settings ----

    /// C# `Settings` getter.
    pub fn settings(&self) -> Arc<AppSettings> {
        self.settings.current()
    }

    /// C# `Settings` setter: installs the override, then `InfoOverlay.Refresh()` (the overlay changes and a possible sibling search
    /// are applied through `apply_overlay_refresh`).
    pub fn set_settings(&mut self, value: Arc<AppSettings>) -> ChangeSet {
        self.settings.set_override(Some(value));
        self.refresh_overlay()
    }

    /// The shared settings cell (a clone shares the override and the store with this view model, as the closures the composition
    /// root builds for the controllers do). C# reads `Settings` through the view model; the shell windows need the same live source.
    pub fn settings_source(&self) -> SettingsSource {
        self.settings.clone()
    }

    /// C# `InfoOverlay`.
    pub fn info_overlay(&self) -> &InfoOverlayViewModel<MainFolderInfoLabels> {
        &self.info_overlay
    }

    // ---- status ----

    /// C# `IsStatusPanelVisible` = `InfoOverlay.IsFileInfoVisible || IsExifLineVisible || HasSkippedEntries`
    /// (`is_exif_line_visible` is W8-08e).
    pub fn is_status_panel_visible(&self) -> bool {
        self.has_skipped_entries()
            || InfoOverlayViewModel::<MainFolderInfoLabels>::is_file_info_visible(&self.settings.current())
            || self.is_exif_line_visible()
    }

    /// C# `FolderTitle`.
    pub fn folder_title(&self) -> &str {
        &self.folder_title
    }

    /// C# `FolderText`.
    pub fn folder_text(&self) -> &str {
        &self.folder_text
    }

    /// C# `IsExplorerOrderApplied`.
    pub fn is_explorer_order_applied(&self) -> bool {
        self.explorer_order_applied
    }

    /// C# `StatusText` getter. Precedence: non-empty event text; else, while the RAW-decode indicator is visible
    /// (`presenter.zoom_detail().is_raw_decode_indicator_visible()`) and the presenter does not need attention, the localized
    /// "decoding RAW" line (`status::decoding_raw`); else the presenter's line.
    pub fn status_text(&self) -> String {
        if !self.status_text.is_empty() {
            return self.status_text.clone();
        }
        if self.presenter.zoom_detail().is_raw_decode_indicator_visible() && !self.presenter.status_needs_attention() {
            return self.with_ctx(crate::viewmodels::format::status::decoding_raw);
        }
        self.presenter.status_text().to_owned()
    }

    /// C# `StatusText` setter (`SetProperty`): stores the event text; `main.StatusText` is in the result only when the stored
    /// text differs (ordinal). `StatusNeedsAttention` is NOT raised by the setter (as in C#).
    pub fn set_status_text(&mut self, value: &str) -> ChangeSet {
        if self.status_text == value {
            return ChangeSet::default();
        }
        self.status_text = value.to_owned();
        ChangeSet::of(MainProperty::StatusText)
    }

    /// C# `StatusNeedsAttention` (Q-R34): RAW indicator visible, or non-empty event text, or the presenter needs attention, or
    /// skipped entries exist.
    pub fn status_needs_attention(&self) -> bool {
        self.presenter.zoom_detail().is_raw_decode_indicator_visible()
            || !self.status_text.is_empty()
            || self.presenter.status_needs_attention()
            || self.has_skipped_entries()
    }

    // ---- skipped entries (IO05) ----

    /// C# `SkippedEntries`.
    pub fn skipped_entries(&self) -> &[SkippedEntry] {
        &self.skipped_entries
    }

    /// C# `HasSkippedEntries`.
    pub fn has_skipped_entries(&self) -> bool {
        !self.skipped_entries.is_empty()
    }

    /// C# `SkippedWarningText`: empty without entries, else the plural catalog text `main.skipped.warning` with the count.
    pub fn skipped_warning_text(&self) -> String {
        if self.skipped_entries.is_empty() {
            return String::new();
        }
        let count = i64::try_from(self.skipped_entries.len()).unwrap_or(i64::MAX);
        let args = [LocArg::new("count", count)];
        self.with_ctx(|ctx| {
            ctx.texts
                .format_plural_with("main.skipped.warning", count, &args, ctx.numbers)
        })
    }

    /// C# `SetSkippedEntries` (private): stores the list and reports `SkippedEntries`, `HasSkippedEntries`, `SkippedWarningText`,
    /// `IsStatusPanelVisible` (always, even for an equal list).
    pub(super) fn set_skipped_entries(&mut self, entries: Vec<SkippedEntry>) -> ChangeSet {
        self.skipped_entries = entries;
        let mut result = ChangeSet::default();
        for property in [
            MainProperty::SkippedEntries,
            MainProperty::HasSkippedEntries,
            MainProperty::SkippedWarningText,
            MainProperty::IsStatusPanelVisible,
        ] {
            result.main.insert(property);
        }
        result
    }

    // ---- catalog-derived state ----

    /// C# `CurrentImage` (the presenter's bitmap).
    pub fn current_image(&self) -> Option<&PlatformImage> {
        self.presenter.current_image()
    }

    /// C# `CurrentIndex`.
    pub fn current_index(&self) -> i32 {
        self.catalog.current_index()
    }

    /// C# `TotalFiles`.
    pub fn total_files(&self) -> usize {
        self.catalog.count()
    }

    /// C# `HasImages`.
    pub fn has_images(&self) -> bool {
        self.catalog.count() > 0
    }

    /// C# `CanNavigateNext`.
    pub fn can_navigate_next(&self) -> bool {
        self.catalog.count() > 0
            && usize::try_from(self.catalog.current_index()).is_ok_and(|index| index < self.catalog.count() - 1)
    }

    /// C# `CanNavigatePrevious`.
    pub fn can_navigate_previous(&self) -> bool {
        self.catalog.count() > 0 && self.catalog.current_index() > 0
    }

    /// C# `CurrentHasComparePair`: `current_index` in range and `presenter.has_compare_pair(catalog, path_at(current_index))`.
    /// `&mut self` because the presenter caches its compare index. A path error (C#: exception) logs and returns false.
    pub fn current_has_compare_pair(&mut self) -> bool {
        let Ok(index) = usize::try_from(self.catalog.current_index()) else {
            return false;
        };
        if index >= self.catalog.count() {
            return false;
        }
        match self
            .presenter
            .has_compare_pair(&self.catalog, self.catalog.path_at(index))
        {
            Ok(found) => found,
            Err(error) => {
                self.log.error("Could not inspect the compare pair", Some(&error));
                false
            }
        }
    }

    /// C# `CurrentHasCapturePair`.
    pub fn current_has_capture_pair(&self) -> bool {
        self.catalog
            .current()
            .is_some_and(|entry| entry.capture_group.is_some())
    }

    /// C# `CapturePairBadge`: empty without a capture group; else `main.capturePair.badge.raw` when the presented path equals the
    /// group's RAW path (OrdinalIgnoreCase), otherwise `main.capturePair.badge.jpeg`.
    pub fn capture_pair_badge(&self) -> String {
        let Some(group) = self.catalog.current().and_then(|entry| entry.capture_group.as_ref()) else {
            return String::new();
        };
        let is_raw = self
            .presenter
            .current_presented_path()
            .is_some_and(|path| pr_core::text::eq_ignore_case(path, group.raw_path()));
        self.with_ctx(|ctx| {
            ctx.texts.get(if is_raw {
                "main.capturePair.badge.raw"
            } else {
                "main.capturePair.badge.jpeg"
            })
        })
    }

    /// C# `CapturePairBadgeToolTip`: `main.capturePair.badge.tooltipBound` with the TRIMMED key when `settings.shortcuts.toggle_capture_member`
    /// is not blank and `settings.store().key_names().is_valid_key_name(configured)` (the untrimmed text is validated);
    /// else `main.capturePair.badge.tooltipUnbound`.
    pub fn capture_pair_badge_tool_tip(&self) -> String {
        let settings = self.settings.current();
        let configured = settings.shortcuts.toggle_capture_member.as_str();
        if !configured.trim().is_empty() && self.settings.store().key_names().is_valid_key_name(configured) {
            return self.with_ctx(|ctx| {
                ctx.texts.format_with(
                    "main.capturePair.badge.tooltipBound",
                    &[LocArg::new("key", configured.trim())],
                    ctx.numbers,
                )
            });
        }
        self.with_ctx(|ctx| ctx.texts.get("main.capturePair.badge.tooltipUnbound"))
    }

    // ---- lifetime ----

    /// True after `close_session` (W8-08d).
    pub fn is_closed(&self) -> bool {
        self.is_closed
    }

    /// True when nothing is outstanding: no folder load or ownership negotiation, no queued or running file command, no deferred
    /// command, no presenter work, no inbox event. Tests loop `while !vm.is_idle() { inbox.wait_ready(..); vm.pump(); }` (replaces the
    /// C# `FolderLoadTask`, `CompareToggleTask`, `SettingsRefreshTask` seams).
    pub fn is_idle(&self) -> bool {
        self.pending_open.is_none()
            && self.deferred.is_empty()
            && self.queued_file_commands.is_empty()
            && self.active_gate_turn.is_none()
            && self.duplicate_gate_turn.is_none()
            && self.settings_reloads_waiting == 0
            && self.pending_worker_jobs == 0
            && !self.controller_present_outstanding
            && self.presenter.is_idle()
            && self.inbox.is_empty()
            && self.sink.is_empty()
    }

    // ---- the event pump ----

    /// Applies every completion that arrived since the last call, on the UI thread, until nothing is left: first the view model's
    /// inbox (in arrival order, `dispatch`), then `presenter.pump(env)`, then the debts that follow (`on_present_finished` to the
    /// file-action controller, release of the deferred commands). Returns the union of all changes.
    pub fn pump(&mut self) -> ChangeSet {
        let mut result = ChangeSet::default();
        loop {
            let events = self.inbox.drain();
            if events.is_empty() {
                break;
            }
            for event in events {
                result.merge(self.dispatch(event));
            }
        }
        let presenter = self.with_presenter(|presenter, env| presenter.pump(env));
        result.merge(self.finish_presenter_call(presenter));
        if !self.sink.is_empty() || self.controller_present_outstanding {
            result.merge(self.apply_sink_actions());
        }
        result
    }

    /// Routes one [`MainEvent`] to its handler: `FolderLoad` -> `navigation::handle_folder_event`, `FileAction` ->
    /// `commands::handle_file_action_event`, `Duplicate` -> `commands::handle_duplicate_event`, `SiblingFound` ->
    /// `navigation::on_sibling_found`, `OverlaySiblings` -> `on_overlay_siblings` (this file), `Ownership` ->
    /// `navigation::on_ownership_decided`, `GateTurn` -> `commands::on_gate_turn`, `GateReleased` -> `commands::on_gate_released`.
    pub(super) fn dispatch(&mut self, event: MainEvent) -> ChangeSet {
        match event {
            MainEvent::FolderLoad(event) => self.handle_folder_event(event),
            MainEvent::FileAction(event) => self.handle_file_action_event(event),
            MainEvent::Duplicate(event) => self.handle_duplicate_event(event),
            MainEvent::SiblingFound { search, result } => self.on_sibling_found(search, result),
            MainEvent::OverlaySiblings { search, outcome } => self.on_overlay_siblings(search, outcome),
            MainEvent::Ownership { request, result } => self.on_ownership_decided(request, result),
            MainEvent::GateTurn { ticket, turn } => self.on_gate_turn(ticket, turn),
            MainEvent::GateReleased => self.on_gate_released(),
        }
    }

    // ---- helpers shared by the five slices ----

    /// Runs `f` with the presenter and its per-call environment (catalog, settings snapshot, session, format context) lent
    /// (the split borrow of `catalog`, `presenter` and `current_session`; the pattern of contract W8-07h note 2).
    pub(super) fn with_presenter<R>(
        &mut self,
        f: impl FnOnce(&mut ImagePresenter, &mut crate::coordinators::presenter::state::PresenterEnv<'_>) -> R,
    ) -> R {
        let format = Arc::clone(&self.format);
        let settings = self.settings.current();
        let mut run = Some(f);
        let mut result = None;
        format.with_context(&mut |ctx| {
            if let Some(f) = run.take() {
                let mut env = crate::coordinators::presenter::state::PresenterEnv {
                    catalog: &mut self.catalog,
                    settings: &settings,
                    session: self.current_session.as_mut(),
                    format: *ctx,
                };
                result = Some(f(&mut self.presenter, &mut env));
            }
        });
        result.unwrap_or_else(|| unreachable!("format source must call its callback"))
    }

    /// Runs `f` with the format context (D-W8-4).
    pub(super) fn with_ctx<R>(&self, f: impl FnOnce(&FormatContext<'_>) -> R) -> R {
        let mut run = Some(f);
        let mut result = None;
        self.format.with_context(&mut |ctx| {
            if let Some(f) = run.take() {
                result = Some(f(ctx));
            }
        });
        result.unwrap_or_else(|| unreachable!("format source must call its callback"))
    }

    /// Turns what a presenter call reported into a [`ChangeSet`]: drains the bridge (viewer changes, `image_changing`), runs
    /// `notify_navigation_state_changed` when a sink callback asked for it (or `PHOTO_INFO` / `STATUS` / `CURRENT_IMAGE` bits are set),
    /// maps `RAW_DECODE_INDICATOR` to `StatusText` + `StatusNeedsAttention`, forwards `COMPARE`, and refreshes the overlay when the
    /// viewer's `DisplayZoomPercent` changed (C# `_viewerState.PropertyChanged` hook, line 126).
    pub(super) fn finish_presenter_call(&mut self, changes: PresenterChanges) -> ChangeSet {
        let pending = self.bridge.take_pending();
        let mut result = pending.changes;
        result.presenter = changes;
        if changes.contains(PresenterChanges::RAW_DECODE_INDICATOR) {
            result.main.insert(MainProperty::StatusText);
            result.main.insert(MainProperty::StatusNeedsAttention);
        }
        if changes.contains(PresenterChanges::COMPARE) {
            result.compare = crate::viewmodels::compare::CompareChanges::all();
        }
        if pending.navigation_dirty
            || changes.contains(PresenterChanges::PHOTO_INFO)
            || changes.contains(PresenterChanges::STATUS)
            || changes.contains(PresenterChanges::CURRENT_IMAGE)
        {
            result.merge(self.notify_navigation_state_changed());
        }
        if result
            .viewer
            .contains(crate::viewmodels::viewer_state::ViewerProperty::DisplayZoomPercent)
        {
            result.merge(self.refresh_overlay());
        }
        result
    }

    /// The two C# viewer hooks (lines 117 to 129) for a change made by the view model: `zoom_mode_changed` ->
    /// `presenter.set_viewer_zoom(viewer.effective_zoom())` (lines 117/118); `DisplayZoomPercent` in the changes ->
    /// `InfoOverlay.Refresh()` (line 127). Returns the combined set.
    pub(super) fn apply_viewer_update(&mut self, update: ViewerUpdate) -> ChangeSet {
        let mut result = ChangeSet {
            viewer: update.changes,
            ..ChangeSet::default()
        };
        if update.zoom_mode_changed {
            let zoom = self.viewer.borrow().effective_zoom();
            let changes = self.presenter.set_viewer_zoom(zoom);
            result.merge(self.finish_presenter_call(changes));
        }
        if update
            .changes
            .contains(crate::viewmodels::viewer_state::ViewerProperty::DisplayZoomPercent)
        {
            result.merge(self.refresh_overlay());
        }
        result
    }

    /// Applies an overlay refresh: `IsFileInfoVisible` in the changes adds `main.IsStatusPanelVisible` (C# line 112); a requested
    /// sibling search is submitted to the runner (`find_sibling_image_folders` -> `MainEvent::OverlaySiblings`).
    pub(super) fn apply_overlay_refresh(&mut self, refresh: InfoOverlayRefresh) -> ChangeSet {
        let mut result = ChangeSet {
            overlay: refresh.changes,
            ..ChangeSet::default()
        };
        if refresh
            .changes
            .contains(crate::viewmodels::info_overlay::InfoOverlayProperty::IsFileInfoVisible)
        {
            result.main.insert(MainProperty::IsStatusPanelVisible);
        }
        if let Some(search) = refresh.search {
            let navigator = Arc::clone(&self.sibling_navigator);
            let inbox = self.inbox.clone();
            self.pending_worker_jobs += 1;
            self.runner.run(
                "overlay-siblings",
                Box::new(move || {
                    let outcome = match navigator
                        .find_sibling_image_folders(&search.folder, &pr_core::sync::CancelToken::none())
                    {
                        Ok(found) => SearchOutcome::Found(found),
                        Err(SiblingNavError::Cancelled) => SearchOutcome::Cancelled,
                        Err(_) => SearchOutcome::Failed,
                    };
                    inbox.send(MainEvent::OverlaySiblings { search, outcome });
                }),
            );
        }
        result
    }

    /// The overlay's inputs now (`has_image`, `viewer.display_zoom_percent()`), C# `() => HasImages`, `() => DisplayZoomPercent`.
    pub(super) fn view_inputs(&self) -> ViewInputs {
        ViewInputs {
            has_image: self.has_images(),
            zoom_percent: self.viewer.borrow().display_zoom_percent(),
        }
    }

    /// C# `OverlayModel.From(MainViewModel)` (`Shell.Win32/Overlay/OverlayModel.cs`): the immutable snapshot of everything the native
    /// overlay shows. Strings are never null in Rust (the C# `?? string.Empty` is the identity). The capture-pair text and tool tip are
    /// empty when the current entry has no pair. `exif_visible` is `is_exif_line_visible()` computed from the one `exif_text()` read.
    pub fn overlay_model(&self) -> OverlayModel {
        let settings = self.settings.current();
        let inputs = self.view_inputs();
        let has_pair = self.current_has_capture_pair();
        let exif_text = self.exif_text();
        OverlayModel {
            has_images: self.has_images(),
            status_needs_attention: self.status_needs_attention(),
            compare_visible: self.presenter.compare().is_visible(),
            status_panel_visible: self.is_status_panel_visible(),
            file_info_visible: InfoOverlayViewModel::<MainFolderInfoLabels>::is_file_info_visible(&settings),
            status_text: self.status_text(),
            font_size: InfoOverlayViewModel::<MainFolderInfoLabels>::font_size(&settings),
            exif_font_size: InfoOverlayViewModel::<MainFolderInfoLabels>::exif_font_size(&settings),
            exif_visible: settings.show_info_overlay && settings.show_exif_info && !exif_text.is_empty(),
            exif_text,
            has_skipped_entries: self.has_skipped_entries(),
            skipped_warning_text: self.skipped_warning_text(),
            folder_info_visible: self.info_overlay.is_folder_info_visible(&settings),
            folder_info_text: self.info_overlay.folder_info_text().to_owned(),
            zoom_indicator_visible: InfoOverlayViewModel::<MainFolderInfoLabels>::is_zoom_indicator_visible(
                &settings, inputs,
            ),
            zoom_indicator_text: InfoOverlayViewModel::<MainFolderInfoLabels>::zoom_indicator_text(inputs),
            capture_pair_visible: has_pair,
            capture_pair_text: if has_pair {
                self.capture_pair_badge()
            } else {
                String::new()
            },
            capture_pair_tool_tip: if has_pair {
                self.capture_pair_badge_tool_tip()
            } else {
                String::new()
            },
        }
    }

    /// `InfoOverlay.Refresh()` with the current settings and inputs, applied.
    pub(super) fn refresh_overlay(&mut self) -> ChangeSet {
        let inputs = self.view_inputs();
        let settings = self.settings.current();
        let refresh = self.info_overlay.refresh(&settings, inputs);
        self.apply_overlay_refresh(refresh)
    }

    /// Completion of an overlay sibling search (`MainEvent::OverlaySiblings`).
    pub(super) fn on_overlay_siblings(&mut self, search: OverlaySiblingSearch, outcome: SearchOutcome) -> ChangeSet {
        self.pending_worker_jobs = self.pending_worker_jobs.saturating_sub(1);
        let inputs = self.view_inputs();
        let settings = self.settings.current();
        let refresh = self.info_overlay.complete_search(&settings, inputs, &search, outcome);
        self.apply_overlay_refresh(refresh)
    }

    /// C# `PersistSession(session)` (line 976, hoisted from the 08d range because 08b and 08e call it): `session_writer.update(&session)`.
    /// The C# fallback `_sessionStore.Save` is unreachable (the writer is a required constructor argument) and not ported.
    pub(super) fn persist_session(&self) {
        if let Some(session) = &self.current_session {
            let _ = self.session_writer.update(session);
        }
    }

    /// C# `PresentedPathOfCurrentEntry()` (line 944, hoisted from the 08d range because 08b and 08d call it): the presenter's
    /// current path when the current entry has a capture group, a presented path exists and `catalog.index_of(presented) ==
    /// current_index`; else `None` (stale after a catalog change, or no pair).
    pub(super) fn presented_path_of_current_entry(&self) -> Option<String> {
        self.catalog.current()?.capture_group.as_ref()?;
        let path = self.presenter.current_presented_path()?;
        (self.catalog.index_of(path) == self.catalog.current_index()).then(|| path.to_owned())
    }

    /// Starts a presentation: `presenter.present(env, request)` and `finish_presenter_call`. A `PresentError::PathNotInEntry`
    /// (the C# `ArgumentException`) is logged and changes nothing (callers pre-validate the override).
    pub(super) fn present_request(
        &mut self,
        request: crate::coordinators::presenter::state::PresentRequest,
    ) -> ChangeSet {
        let changes = self.with_presenter(|presenter, env| presenter.present(env, request));
        match changes {
            Ok(changes) => self.finish_presenter_call(changes),
            Err(error) => {
                self.log.error("Could not present the selected path", Some(&error));
                ChangeSet::default()
            }
        }
    }
}
