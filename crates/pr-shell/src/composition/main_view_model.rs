//! Port of `PhotoReview.App.Shared/Composition/MainViewModelCompositionRoot.cs` (98 lines, C# baseline `d92d6aae`) and of the registration
//! `services.AddTransient<MainViewModel>(sp => MainViewModelCompositionRoot.Create(sp))` (`SharedServiceRegistration.cs` line 228). Unit W8-09b. Contract: `contracts/W8-09b.md`.
//!
//! One call builds ONE complete `MainViewModel` with everything C# wired by hand: a FRESH catalog, generation clock, viewer and compare view model (registrations 43 to 46 are
//! `AddTransient`), the SHARED singletons of [`SharedServices`], and the host's UI-thread services. The C# trick "VM <-> sink cycle by null-initialisation" (`MainViewModel? vm = null`
//! captured by the sink callbacks and by `ForwardingFolderSink`) is gone: the Rust coordinators take their sink PER CALL and the presenter's sink reaches the view model through the
//! [`PresentationBridge`] (08a), so the order is simply viewer, bridge, sink, presenter, coordinators, view model.
//!
//! UI thread only: the result is `!Send`. The result is NOT wrapped: the shell wraps it as `SharedViewModel = Rc<RefCell<MainViewModel>>` (`graph::build_shared_view_model`).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_app::coordinators::sink_callbacks::PresentationSinkFactory;
use pr_app::coordinators::traits::{IFolderPicker, IPreloadController};
use pr_app::coordinators::zoom_detail::{BackgroundRunner, WakeFn};
use pr_app::services::contracts::{ClipboardService, PresentationObserver};
use pr_app::viewmodels::main::MainViewModel;
use pr_core::abstractions::{DialogService, UiScheduler};
use pr_core::catalog::review_catalog::ReviewCatalog;
use pr_core::catalog::types::CatalogEntry;
use pr_imaging::cache_key::ImageCacheKey;
use pr_imaging::preload::scheduler::{EntriesSnapshotFn, RunHandle, SnapshotVersionFn};

use super::registry::{RegistryError, SharedServices};

/// What the HOST registers for the view model (C# `IUiScheduler`, `IDialogService`, `IFolderPicker`, `IClipboardService`, `IPresentationSinkFactory`, `IPresentationObserver`,
/// and the optional `IPreloadController` override). All required except the two `Option`s, which are the C# `GetService` (optional) calls: a host that gives no folder picker
/// gets a view model without the Move-to/Copy-to picker; a host that gives a preload controller replaces the real scheduler entirely (AR02a step 5: the real one is NEVER created).
pub struct MainViewModelHost {
    /// C# `IUiScheduler` (`GetRequiredService`): `ImagePresenterDeps::ui_scheduler`, the file-action gate's scheduler. The shell dispatcher.
    pub ui_scheduler: Arc<dyn UiScheduler>,
    /// C# `IDialogService`.
    pub dialog_service: Arc<dyn DialogService>,
    /// C# `IFolderPicker` (`GetService`).
    pub folder_picker: Option<Box<dyn IFolderPicker>>,
    /// C# `IClipboardService` (`GetRequiredService`): there is NO fallback to a real clipboard (SharedServiceRegistrationTests `WithoutAHostClipboard_FailsLoudly`; in Rust the field is
    /// not optional, so the test is unrepresentable).
    pub clipboard: Box<dyn ClipboardService>,
    /// C# `IPresentationSinkFactory`: required.
    pub sink_factory: Box<dyn PresentationSinkFactory>,
    /// C# `IPresentationObserver`: the `on_presented` callback of the sink forwards to it. `NullPresentationObserver` in production.
    pub observer: Rc<dyn PresentationObserver>,
    /// The wake function of the view model's inbox (`MainWindow`'s `view_model_wake(dispatcher)` in production: it works before the window exists).
    pub wake: WakeFn,
    /// C# `GetService<IPreloadController>()`: `Some` = used as is and `preload_scheduler_factory` is not called.
    pub preload_controller: Option<Arc<dyn IPreloadController>>,
    /// Runs the sibling searches, the ownership negotiation and the presenter's waits (`ThreadRunner` in production).
    pub runner: Arc<dyn BackgroundRunner>,
}

impl std::fmt::Debug for MainViewModelHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainViewModelHost").finish_non_exhaustive()
    }
}

/// The catalog entries the preload scheduler reads, published by the UI thread (decision D-W8B6-155, prerequisite amendment A12). C# gave the scheduler the closure
/// `() => catalog.EntriesSnapshot()` over the one shared catalog object. In Rust the `ReviewCatalog` is owned by value by `MainViewModel` and is `!Sync`, while the scheduler's
/// `EntriesSnapshotFn` / `SnapshotVersionFn` must be `Send + Sync`; so the closures read this feed instead, and the view model side publishes into it ([`CatalogFeed::publish_if_changed`])
/// right before every preload kick (the hook `IPreloadController::catalog_changed`, amendment A12). Cloning shares the cell.
#[derive(Clone)]
pub struct CatalogFeed {
    state: Arc<Mutex<FeedState>>,
}

struct FeedState {
    /// `ReviewCatalog::structural_version` of the published list; `-1` before the first publication.
    version: i32,
    /// The published list; the `Arc` identity is stable until the version changes (the scheduler's whole-folder estimate cache keys on it, W4-07a).
    entries: Arc<[CatalogEntry]>,
}

impl std::fmt::Debug for CatalogFeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogFeed").finish_non_exhaustive()
    }
}

impl CatalogFeed {
    /// An empty feed (version `-1`, no entries).
    pub fn new() -> Self {
        todo!()
    }

    /// UI thread. `catalog.structural_version() == published version` -> nothing (no copy, same `Arc`); else `entries = Arc::from(catalog.entries_snapshot())` and the new version.
    pub fn publish_if_changed(&self, catalog: &ReviewCatalog) {
        todo!()
    }

    /// The `EntriesSnapshotFn` the scheduler factory takes: `Ok(published list)` (a poisoned lock is recovered).
    pub fn entries_fn(&self) -> EntriesSnapshotFn {
        todo!()
    }

    /// The `SnapshotVersionFn` the scheduler factory takes: `Ok(published version)`.
    pub fn version_fn(&self) -> SnapshotVersionFn {
        todo!()
    }
}

impl Default for CatalogFeed {
    fn default() -> Self {
        Self::new()
    }
}

/// The production `IPreloadController` (C# `PreloadControllerAdapter(() => preloadScheduler)` over the real `PreloadScheduler`), plus the feed. Every method forwards to the
/// adapter; AFTER amendment A12 it also overrides `catalog_changed(&self, catalog)` as `self.feed.publish_if_changed(catalog)` (the stub cannot override a method the trait does not
/// have yet). `dispose` disposes the scheduler (the main window calls it at close).
pub struct FeedPreloadController {
    scheduler: Arc<pr_imaging::preload::scheduler::PreloadScheduler>,
    feed: CatalogFeed,
}

impl std::fmt::Debug for FeedPreloadController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeedPreloadController").finish_non_exhaustive()
    }
}

impl FeedPreloadController {
    /// Wraps an already built scheduler (the one `preload_scheduler_factory(feed.entries_fn(), feed.version_fn())` returned).
    pub fn new(scheduler: Arc<pr_imaging::preload::scheduler::PreloadScheduler>, feed: CatalogFeed) -> Self {
        Self { scheduler, feed }
    }
}

impl IPreloadController for FeedPreloadController {
    fn preload_around(&self, center: i32) -> RunHandle {
        todo!()
    }

    fn try_consume_preloaded_key(&self, key: &ImageCacheKey) -> bool {
        todo!()
    }

    fn cancel(&self) {
        todo!()
    }

    fn remove_preloaded_keys_for_path(&self, normalized_path: &str) {
        todo!()
    }

    fn clear_preloaded_keys(&self) {
        todo!()
    }

    fn is_idle(&self) -> bool {
        todo!()
    }

    fn notify_navigation(&self, index: i32) {
        todo!()
    }

    fn get_viewer_decode_delay(&self) -> Duration {
        todo!()
    }

    fn dispose(&self) {
        todo!()
    }
}

/// C# `MainViewModelCompositionRoot.Create(sp)`, in this order:
///
/// 1. `catalog = ReviewCatalog::new()` then `catalog.bind_to_current_thread()` (ADR 0005: the catalog is bound to the thread that builds it, the UI thread); `clock =
///    Arc::new(GenerationClock::default())`; `viewer = ViewerHandle::new(ViewerState::new())`; `bridge = PresentationBridge::new(viewer.clone())`; `settings = SettingsSource::new(
///    services.settings_store())`; `texts = Arc::new(LocalizedTexts)`; `format = Arc::new(LocalizedFormat::new())`; `inbox = CompletionInbox::new(host.wake.clone())`.
/// 2. The preload controller: `host.preload_controller` when `Some` (no feed, no scheduler); else `feed = CatalogFeed::new()`, `scheduler = (services.preload_scheduler_factory())(
///    feed.entries_fn(), feed.version_fn())?` and `Arc::new(FeedPreloadController::new(scheduler, feed))`.
/// 3. The sink: `host.sink_factory.create(MainViewModelSinkCallbacks { set_current_image: forwards to bridge.on_set_current_image, set_status_text: bridge.on_set_status_text,
///    apply_initial_view_mode: bridge.apply_initial_view(viewport_size_source().viewport_size(), &settings_store().current()), on_presented: host.observer.on_presented(path),
///    take_viewer_zoom_change: bridge.take_viewer_zoom_change })` (the C# `ApplyInitialView` static is `PresentationBridge::apply_initial_view`, already ported by 08a).
/// 4. `ImagePresenter::new(ImagePresenterDeps { clock, preview_service, thumbnail_cache, preload, hasher: file_hash_service, metrics, session_store: Some(..), session_writer: Some(..),
///    sink, file_system: Some(..), on_presented_hook: None, ui_scheduler: Some(host.ui_scheduler), busy_retry_delay: None, runner: Some(host.runner), stat_worker: None, wake,
///    indicator_delay: None }, CompareViewModel::new(&format_context))`.
/// 5. `FolderLoadCoordinator::new(FolderLoadDeps { clock, explorer_order, file_system, session_store, settings_store, session_writer: Some(..), latest_explorer:
///    Some(explorer_snapshot()), worker: Arc::new(MainWorkerPort::new(runner, inbox.clone(), MainEvent::FolderLoad)), log, perf: None })`.
/// 6. `FileActionController::new(FileActionDeps { clock, file_actions: Some(Arc::new(FileActionServiceExecutor::new(file_action_service()))), undo_service: Some(undo_service()),
///    dialog_service: Some(..), preload_controller, natural_comparer, get_settings: closure over `settings`, file_system: Some(..), remember_folder: Some(closure over
///    `remember_move_copy_folder(&settings, &*log, ..)`), worker: MainWorkerPort(.., MainEvent::FileAction), texts, log })`; `DuplicateCleanupController::new(DuplicateCleanupDeps {..})`
///    with `worker: MainWorkerPort(.., MainEvent::Duplicate)`; `FileActionGate::new(host.ui_scheduler)`; `SiblingFolderNavigator::new(clock, file_system, Some(raw_support_enabled),
///    Some(webp_heic_support_enabled))`.
/// 7. `MainViewModel::new(MainViewModelDeps { .. })` with `reset_caches_action: None` (the C# root did not pass `resetCachesAction`) and `external_editor: None`.
///
/// The `Err` is a `RegistryError` of a lazily built service (preview service, thumbnail cache, session writer, file hash service, scheduler).
pub fn create_main_view_model(
    services: &SharedServices,
    host: MainViewModelHost,
) -> Result<MainViewModel, RegistryError> {
    todo!()
}
