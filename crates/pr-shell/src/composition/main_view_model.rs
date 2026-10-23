//! Port of `PhotoReview.App.Shared/Composition/MainViewModelCompositionRoot.cs` (98 lines, C# baseline `7eaee2a2`) and of the registration
//! `services.AddTransient<MainViewModel>(sp => MainViewModelCompositionRoot.Create(sp))` (`SharedServiceRegistration.cs` line 228). Unit W8-09b. Contract: `contracts/W8-09b.md`.
//!
//! One call builds ONE complete `MainViewModel` with everything C# wired by hand: a FRESH catalog, generation clock, viewer and compare view model (registrations 43 to 46 are
//! `AddTransient`), the SHARED singletons of [`SharedServices`], and the host's UI-thread services. The C# trick "VM <-> sink cycle by null-initialisation" (`MainViewModel? vm = null`
//! captured by the sink callbacks and by `ForwardingFolderSink`) is gone: the Rust coordinators take their sink PER CALL and the presenter's sink reaches the view model through the
//! [`PresentationBridge`] (08a), so the order is simply viewer, bridge, sink, presenter, coordinators, view model.
//!
//! UI thread only: the result is `!Send`. The result is NOT wrapped: the shell wraps it as `SharedViewModel = Rc<RefCell<MainViewModel>>` (`graph::build_shared_view_model`).

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
        Self {
            state: Arc::new(Mutex::new(FeedState {
                version: -1,
                entries: Arc::from([]),
            })),
        }
    }

    /// UI thread. `catalog.structural_version() == published version` -> nothing (no copy, same `Arc`); else `entries = Arc::from(catalog.entries_snapshot())` and the new version.
    pub fn publish_if_changed(&self, catalog: &ReviewCatalog) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.version != catalog.structural_version() {
            state.entries = Arc::from(catalog.entries_snapshot());
            state.version = catalog.structural_version();
        }
    }

    /// The `EntriesSnapshotFn` the scheduler factory takes: `Ok(published list)` (a poisoned lock is recovered).
    pub fn entries_fn(&self) -> EntriesSnapshotFn {
        let feed = self.clone();
        Arc::new(move || {
            Ok(feed
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entries
                .clone())
        })
    }

    /// The `SnapshotVersionFn` the scheduler factory takes: `Ok(published version)`.
    pub fn version_fn(&self) -> SnapshotVersionFn {
        let feed = self.clone();
        Arc::new(move || {
            Ok(feed
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .version)
        })
    }
}

impl Default for CatalogFeed {
    fn default() -> Self {
        Self::new()
    }
}

/// The production `IPreloadController` (C# `PreloadControllerAdapter(() => preloadScheduler)` over the real `PreloadScheduler`), plus the feed. Every method forwards to the
/// adapter; amendment A12 publishes the current catalog through `catalog_changed` immediately before each preload kick.
/// `dispose` disposes the scheduler (the main window calls it at close).
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
    fn catalog_changed(&self, catalog: &ReviewCatalog) {
        self.feed.publish_if_changed(catalog);
    }

    fn preload_around(&self, center: i32) -> RunHandle {
        self.scheduler.preload_around(center)
    }

    fn try_consume_preloaded_key(&self, key: &ImageCacheKey) -> bool {
        self.scheduler.try_consume_preloaded_key(key)
    }

    fn cancel(&self) {
        self.scheduler.cancel();
    }

    fn remove_preloaded_keys_for_path(&self, normalized_path: &str) {
        self.scheduler.remove_preloaded_keys_for_path(normalized_path);
    }

    fn clear_preloaded_keys(&self) {
        self.scheduler.clear_preloaded_keys();
    }

    fn is_idle(&self) -> bool {
        self.scheduler.is_idle()
    }

    fn notify_navigation(&self, index: i32) {
        self.scheduler.notify_navigation(index);
    }

    fn get_viewer_decode_delay(&self) -> Duration {
        self.scheduler.get_viewer_decode_delay()
    }

    fn dispose(&self) {
        self.scheduler.dispose();
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
    use super::{adapters::LocalizedFormat, texts::LocalizedTexts};
    use pr_app::coordinators::{
        duplicate_cleanup::{DuplicateCleanupController, DuplicateCleanupDeps},
        file_action_gate::FileActionGate,
        file_actions::recycle::{FileActionController, FileActionDeps},
        folder_load::state::{FolderLoadCoordinator, FolderLoadDeps},
        presenter::{ImagePresenter, ImagePresenterDeps},
        sibling_navigator::SiblingFolderNavigator,
        sink_callbacks::MainViewModelSinkCallbacks,
        traits::FileActionServiceExecutor,
        zoom_detail::CompletionInbox,
    };
    use pr_app::viewmodels::{
        compare::CompareViewModel,
        main::{
            FormatContextSource, MainEvent, MainViewModelDeps, MainWorkerPort, PresentationBridge, SettingsSource,
            ViewerHandle, remember_move_copy_folder,
        },
        viewer_state::ViewerState,
    };
    use pr_core::catalog::generation::GenerationClock;

    let mut catalog = ReviewCatalog::new();
    catalog.bind_to_current_thread();
    let clock = Arc::new(GenerationClock::default());
    let viewer = ViewerHandle::new(ViewerState::new());
    let bridge = PresentationBridge::new(viewer.clone());
    let settings = SettingsSource::new(services.settings_store());
    let texts = Arc::new(LocalizedTexts);
    let format = Arc::new(LocalizedFormat::new());
    let inbox = CompletionInbox::new(host.wake.clone());
    let preload = match host.preload_controller {
        Some(controller) => controller,
        None => {
            let feed = CatalogFeed::new();
            let scheduler = (services.preload_scheduler_factory())(feed.entries_fn(), feed.version_fn())?;
            Arc::new(FeedPreloadController::new(scheduler, feed)) as Arc<dyn IPreloadController>
        }
    };
    let image_bridge = bridge.clone();
    let status_bridge = bridge.clone();
    let initial_bridge = bridge.clone();
    let zoom_bridge = bridge.clone();
    let viewport = services.viewport_size_source();
    let store = services.settings_store();
    let observer = host.observer;
    let sink = host.sink_factory.create(MainViewModelSinkCallbacks {
        set_current_image: Box::new(move |_, is_file_change, change| {
            image_bridge.on_set_current_image(change, is_file_change)
        }),
        set_status_text: Box::new(move |_| status_bridge.on_set_status_text()),
        apply_initial_view_mode: Box::new(move || {
            initial_bridge.apply_initial_view(viewport.viewport_size(), &store.current());
        }),
        on_presented: Box::new(move |path| observer.on_presented(path)),
        take_viewer_zoom_change: Box::new(move || zoom_bridge.take_viewer_zoom_change()),
    });
    let mut compare = None;
    format.with_context(&mut |context| compare = Some(CompareViewModel::new(context)));
    let compare = compare.ok_or_else(|| RegistryError {
        service: "compare_view_model",
        message: "format context unavailable".to_owned(),
    })?;
    let preview = services.preview_service()?;
    let thumbnails = services.thumbnail_cache()?;
    let hash = services.file_hash_service()?;
    let writer = services.session_writer()?;
    let fs = services.file_system();
    let metrics = services.metrics();
    let log = services.log();
    let presenter = ImagePresenter::new(
        ImagePresenterDeps {
            clock: clock.clone(),
            preview_service: preview.clone(),
            thumbnail_cache: thumbnails.clone(),
            preload: preload.clone(),
            hasher: hash.clone(),
            metrics: metrics.clone(),
            session_store: Some(services.session_store()),
            session_writer: Some(writer.clone()),
            sink,
            file_system: Some(fs.clone()),
            on_presented_hook: None,
            ui_scheduler: Some(host.ui_scheduler.clone()),
            busy_retry_delay: None,
            runner: Some(host.runner.clone()),
            stat_worker: None,
            wake: host.wake,
            indicator_delay: None,
        },
        compare,
    );
    let folder_coordinator = FolderLoadCoordinator::new(FolderLoadDeps {
        clock: clock.clone(),
        explorer_order: services.explorer_order(),
        file_system: fs.clone(),
        session_store: services.session_store(),
        settings_store: services.settings_store(),
        session_writer: Some(writer.clone()),
        latest_explorer: Some(services.explorer_snapshot()),
        worker: Arc::new(MainWorkerPort::new(
            host.runner.clone(),
            inbox.clone(),
            MainEvent::FolderLoad,
        )),
        log: log.clone(),
        perf: None,
    });
    let actions = Arc::new(FileActionServiceExecutor::new(services.file_action_service()));
    let undo = services.undo_service();
    let action_settings = settings.clone();
    let remember_settings = settings.clone();
    let remember_log = log.clone();
    let file_action_controller = FileActionController::new(FileActionDeps {
        clock: clock.clone(),
        file_actions: Some(actions.clone()),
        undo_service: Some(undo.clone()),
        dialog_service: Some(host.dialog_service.clone()),
        preload_controller: Some(preload.clone()),
        natural_comparer: services.natural_comparer(),
        get_settings: Box::new(move || action_settings.current()),
        file_system: Some(fs.clone()),
        remember_folder: Some(Box::new(move |operation, folder| {
            remember_move_copy_folder(&remember_settings, remember_log.as_ref(), operation, folder)
        })),
        worker: Arc::new(MainWorkerPort::new(
            host.runner.clone(),
            inbox.clone(),
            MainEvent::FileAction,
        )),
        texts: texts.clone(),
        log: log.clone(),
    });
    let duplicate_controller = DuplicateCleanupController::new(DuplicateCleanupDeps {
        clock: clock.clone(),
        file_actions: Some(actions),
        hash_service: Some(hash),
        file_system: fs.clone(),
        dialog_service: Some(host.dialog_service.clone()),
        preload_controller: Some(preload.clone()),
        thumbnail_cache: Some(thumbnails),
        preview_service: Some(preview.clone()),
        clear_disk_caches: None,
        worker: Arc::new(MainWorkerPort::new(
            host.runner.clone(),
            inbox.clone(),
            MainEvent::Duplicate,
        )),
        texts: texts.clone(),
        log: log.clone(),
    });
    let raw_settings = settings.clone();
    let webp_settings = settings.clone();
    let sibling_navigator = Arc::new(SiblingFolderNavigator::new(
        clock.clone(),
        fs.clone(),
        Some(Arc::new(move || raw_settings.current().raw_support_enabled)),
        Some(Arc::new(move || webp_settings.current().webp_heic_support_enabled)),
    ));
    Ok(MainViewModel::new(MainViewModelDeps {
        catalog,
        clock,
        folder_coordinator,
        presenter,
        viewer,
        bridge,
        settings,
        file_system: fs,
        file_action_controller,
        file_action_gate: FileActionGate::new(host.ui_scheduler),
        sibling_navigator,
        duplicate_controller,
        dialog_service: host.dialog_service,
        session_writer: writer,
        undo_service: undo,
        preview_service: preview,
        preload_controller: Some(preload),
        reset_caches_action: None,
        metrics,
        folder_picker: host.folder_picker,
        clipboard: Some(host.clipboard),
        external_editor: None,
        texts,
        format,
        inbox,
        runner: host.runner,
        log,
    }))
}
