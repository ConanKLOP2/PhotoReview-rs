//! Port of `PhotoReview.App.Shared/Composition/SharedServiceRegistration.cs` (243 lines, C# baseline `d92d6aae`) minus the transient view-model block (that is
//! `main_view_model`), and of `PhotoReview.App/Composition/AppHost.cs` (24 lines, folded in: `AppHost.BuildServices(overrides)` is `graph::production_graph_builder_with(GraphOverrides)`).
//! Unit W8-09b. Contract: `contracts/W8-09b.md`.
//!
//! There is no DI container (plan section 09, P-38): [`SharedServices`] is a cheap handle (`Arc`) with ONE accessor per C# `AddSingleton` of `AddPhotoReviewShared`. Each accessor
//! builds its service on FIRST CALL and caches it (C# singletons are lazy; start-up cost and the order of settings reads depend on that, decision D-W8B6-154): nothing is built by
//! [`SharedServices::build`]. The handle is `Clone + Send + Sync` because workers use it (the early decode resolves the preview service on a worker; the decoder factory and the caches
//! are used by preload threads). The transient C# registrations (`ViewerState`, `CompareViewModel`, `ReviewCatalog`, `GenerationClock`, `MainViewModel`) are NOT here: they are `!Send`
//! or per-view-model and live in `main_view_model::create_main_view_model`.
//!
//! The golden `shell-composition-registrations` (indices 12 to 47) lists the C# registrations this module maps; the test `shared_registrations_are_all_mapped` pins the map.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

use pr_app::coordinators::traits::IMoveOverride;
use pr_app::misc::file_hash::FileHashService;
use pr_app::presenters::settings::load::SharedLocalization;
use pr_app::services::preview_state::PreviewStateContext;
use pr_app::services::viewport_size_source::ViewportSizeSource;
use pr_core::abstractions::{
    Clock, DisplayClock, ExplorerOrderProvider, FileSystem, KeyNameValidator, LiveOperationRegistry, Log, MemoryProbe,
    NaturalComparer, RecycleBin, SourceReader,
};
use pr_core::app_paths::AppPaths;
use pr_core::catalog::types::LatestExplorerSnapshot;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::recovery_retry::RecoveryRetryService;
use pr_core::file_actions::service::FileActionService;
use pr_core::file_actions::undo::UndoService;
use pr_core::session::store::SessionStore;
use pr_core::session::writer::SessionWriter;
use pr_core::settings::store::SettingsStore;
use pr_imaging::cache::source_bytes::SourceBytesCachePolicy;
use pr_imaging::cache::thumbnails::ThumbnailCache;
use pr_imaging::decoder_traits::ImageDecoderFactory;
use pr_imaging::preload::scheduler::{EntriesSnapshotFn, PreloadScheduler, SnapshotVersionFn};
use pr_imaging::service::PreviewImageService;

use super::contracts::SharedServiceOptions;
use super::decoders::DecoderDeps;
use super::platform::PlatformServices;

/// The Rust form of C# `AppHost.BuildServices(Action<IServiceCollection> overrides)` for ONE decoder-related override the integration tests use (`ShellStalePreviewTests`
/// decorates the `IImageDecoderFactory`): receives the factory the registry built and returns the one to share.
pub type DecoderFactoryWrapper =
    Arc<dyn Fn(Arc<dyn ImageDecoderFactory>) -> Arc<dyn ImageDecoderFactory> + Send + Sync>;

/// C# `Func<Func<CatalogEntry[]>, Func<int>, PreloadScheduler>` (registration 42): `(entries snapshot, structural version)` -> a NEW scheduler built from the settings read NOW
/// (worker count, memory load limit, preload window, `full_folder_ram_threshold = preview_service.capacity_bytes()`), RV-I13.
pub type PreloadSchedulerFactory =
    Arc<dyn Fn(EntriesSnapshotFn, SnapshotVersionFn) -> Result<Arc<PreloadScheduler>, RegistryError> + Send + Sync>;

/// A lazily built service could not be created. The only real causes are constructor errors of the imaging services (blank cache directory, a thread that cannot start); the C#
/// container threw the same from `GetRequiredService`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError {
    /// The accessor name (`"preview_service"`).
    pub service: &'static str,
    /// The text of the underlying error.
    pub message: String,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} could not be created: {}", self.service, self.message)
    }
}

impl Error for RegistryError {}

/// Everything [`SharedServices`] needs from outside (the arguments of `AddPhotoReviewShared` and the "overrides" a test adds after it). No hidden defaults: the graph builder fills it.
#[derive(Clone)]
pub struct SharedServiceInputs {
    /// C# `SharedServiceOptions` (09a): the platform codec, the optional Wpf provider (`None` on the Win32 host, NE-8) and the reserved thumbnail decoder.
    pub options: SharedServiceOptions,
    /// C# `IAppPaths`: `AppPaths::from_environment()` in production (R0002: `%LOCALAPPDATA%\PhotoReviewRs`, `PHOTOREVIEW_RS_DATA_ROOT`); a test passes a temp-root `AppPaths::new(..)`.
    pub paths: AppPaths,
    /// C# `ILog`: the process log (`GlobalLog`) in production.
    pub log: Arc<dyn Log>,
    /// C# `IClock`: `SystemClock` in production.
    pub clock: Arc<dyn Clock>,
    /// C# `ISourceReader` (Q-R29 C-2): `PhysicalSourceReader` in production; only the benchmark CLI replaces it.
    pub source_reader: Arc<dyn SourceReader>,
    /// The OS seams.
    pub platform: PlatformServices,
    /// Test seam: wraps the registry's `IImageDecoderFactory` (`GraphOverrides::decoder_wrapper`). `None` = the factory as built.
    pub decoder_wrapper: Option<DecoderFactoryWrapper>,
    /// C# `IMoveOverride` (AR02a step 6): `None` in production = files are moved for real; `Some` reaches `FileActionService` and `UndoService` through `into_move_override`.
    pub move_override: Option<Arc<dyn IMoveOverride>>,
}

impl fmt::Debug for SharedServiceInputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedServiceInputs").finish_non_exhaustive()
    }
}

struct Inner {
    inputs: SharedServiceInputs,
    // The porter adds one `std::sync::OnceLock<..>` cell per cached accessor below (or one `OnceLock<Result<..>>` for the fallible ones).
}

/// C# `AddPhotoReviewShared`: the singletons of the shared service graph. `Clone` shares the cells.
#[derive(Clone)]
pub struct SharedServices {
    inner: Arc<Inner>,
}

impl fmt::Debug for SharedServices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedServices").finish_non_exhaustive()
    }
}

impl SharedServices {
    /// Stores the inputs. Builds NOTHING, reads no file, starts no thread (C# `AddPhotoReviewShared` only registered; the `ArgumentNullException` checks of `options`, `Codec` and
    /// `ThumbnailDecoder` have no Rust form: the types cannot be null).
    pub fn build(inputs: SharedServiceInputs) -> SharedServices {
        SharedServices {
            inner: Arc::new(Inner { inputs }),
        }
    }

    // ---- 1. core abstractions (golden 12 to 16) ----

    /// Registration 12 `IAppPaths`: the value of `inputs.paths`.
    pub fn paths(&self) -> &AppPaths {
        &self.inner.inputs.paths
    }

    /// Registration 13 `IFileSystem`: `CountingFileSystem::new(Arc::new(PhysicalFileSystem), counter)` where `counter` is a `StatCounter` closure calling `metrics().record_stat()` (the
    /// C# `CountingFileSystem(new PhysicalFileSystem(), ReviewMetrics)`: metadata queries are counted for the diagnostics and benchmark reports).
    pub fn file_system(&self) -> Arc<dyn FileSystem> {
        todo!()
    }

    /// Registration 14 `ISourceReader`: `inputs.source_reader`.
    pub fn source_reader(&self) -> Arc<dyn SourceReader> {
        todo!()
    }

    /// Registration 15 `IClock`: `inputs.clock`.
    pub fn clock(&self) -> Arc<dyn Clock> {
        todo!()
    }

    /// Registration 16 `ILog`: `inputs.log` (the process-wide instance: the registry NEVER owns the log's life, as the C# comment of AR02c explains).
    pub fn log(&self) -> Arc<dyn Log> {
        todo!()
    }

    // ---- 2. settings and session (golden 17 to 20) ----

    /// Registration 17 `SettingsStore`: `SettingsStore::new(paths.clone(), file_system(), log(), clock(), Arc::new(LocalizedTexts))` `.with_startup_error_callback(AppLog::error_forced adapter)`
    /// `.with_key_name_validator(key_names())` (AR11a). `current()` is the defaults until the start-up calls `load`.
    pub fn settings_store(&self) -> Arc<SettingsStore> {
        todo!()
    }

    /// Registration 34 `IKeyNameValidator`: `Arc::new(pr_app::services::shortcut_key_name::KeyIdNameValidator)`.
    pub fn key_names(&self) -> Arc<dyn KeyNameValidator> {
        todo!()
    }

    /// Registration 18 `SessionStore`: `SessionStore::new(paths(), file_system(), Some(counter))` `.with_log(log())` where `counter` is a `SessionWriteCounter` closure calling
    /// `metrics().record_session_write()`. Starts the sweep thread of the store (its own contract).
    pub fn session_store(&self) -> Arc<SessionStore> {
        todo!()
    }

    /// Registration 19 `SessionWriter`: `SessionWriter::new(session_store(), Some(log()), None, None)`; `Err` only for an invalid debounce (impossible with the defaults).
    pub fn session_writer(&self) -> Result<Arc<SessionWriter>, RegistryError> {
        todo!()
    }

    /// Registration 20 `LocalizationService`: `Arc::new(Mutex::new(LocalizationService::new(Arc::new(PhysicalFileSystem), LocalizationPaths { shipped_dir, user_languages_dir },
    /// platform().installed_ui_culture.clone(), platform().cultures.clone(), log())))` over a PLAIN file system on purpose (catalog reads must not count in the read budgets, ADR 0006).
    /// `shipped_dir` = `<directory of the running exe>\Languages` (C# `AppContext.BaseDirectory\Languages`), `user_languages_dir` = `<directory of paths().config_file>\Languages`.
    pub fn localization(&self) -> SharedLocalization {
        todo!()
    }

    // ---- 3. journal and file actions (golden 21 to 25) ----

    /// Registration 21 `OperationJournal`: `OperationJournal::with_options(paths(), file_system(), clock(), OperationJournalOptions { durability: Some(closure reading
    /// settings_store().current().journal_durability at EVERY write), live_operations: Some(live_operations()), compaction_files: Some(Arc::new(PhysicalJournalCompactionFiles)),
    /// log: Some(log()), texts: Some(Arc::new(LocalizedTexts)), ..Default::default() })`.
    pub fn journal(&self) -> Arc<OperationJournal> {
        todo!()
    }

    /// Registration 24 `FileActionService`: `FileActionService::new(journal(), file_system(), clock(), recycle_bin())` `.with_texts(LocalizedTexts)` and, when `inputs.move_override` is
    /// `Some`, `.with_move_override(into_move_override(over))`.
    pub fn file_action_service(&self) -> Arc<FileActionService> {
        todo!()
    }

    /// Registration 25 `UndoService`: `UndoService::new(journal(), file_system(), recycle_bin(), Arc::new(LocalizedTexts))` `.with_gate(file_action_service())` (INV-4: one gate for file
    /// actions and undo) and the same move override; wrapped as `Arc<Mutex<UndoService>>` (the shape of `MainViewModelDeps::undo_service` and `FileActionDeps::undo_service`).
    pub fn undo_service(&self) -> Arc<Mutex<UndoService>> {
        todo!()
    }

    /// Registration 22 `RecoveryRetryService`: `factories::create_recovery_retry_service(journal(), file_system(), clock(), recycle_bin(), settings_store(), Arc::new(LocalizedTexts),
    /// Some(file_action_service() as the FileActionGate))` (RV-C08: the retry shares the app's INV-4 gate).
    pub fn recovery_retry(&self) -> Arc<RecoveryRetryService> {
        todo!()
    }

    /// Registration 23 `FileHashService`: `FileHashService::with_seams(source_bytes_policy()?.cache().cloned(), file_system(), source_reader(), Arc::new(LocalizedTexts))`;
    /// `Err` for `InvalidCapacity` (impossible) and for a `source_bytes_policy` error.
    pub fn file_hash_service(&self) -> Result<Arc<FileHashService>, RegistryError> {
        todo!()
    }

    // ---- 4. diagnostics (golden 26, 27) ----

    /// Registration 26 `ReviewMetrics`: `Arc::new(ReviewMetrics::new())`.
    pub fn metrics(&self) -> Arc<ReviewMetrics> {
        todo!()
    }

    /// Registration 27 `LatestExplorerSnapshot`: `Arc::new(LatestExplorerSnapshot::new())`.
    pub fn explorer_snapshot(&self) -> Arc<LatestExplorerSnapshot> {
        todo!()
    }

    // ---- 5. platform services (golden 28 to 33, 35, 36) ----

    /// Registration 28 `IExplorerOrderProvider`: `platform().explorer_order`.
    pub fn explorer_order(&self) -> Arc<dyn ExplorerOrderProvider> {
        todo!()
    }

    /// Registration 29 `IRecycleBin`: `platform().recycle_bin`.
    pub fn recycle_bin(&self) -> Arc<dyn RecycleBin> {
        todo!()
    }

    /// Registration 30 `ILiveOperationRegistry`: `platform().live_operations`.
    pub fn live_operations(&self) -> Arc<dyn LiveOperationRegistry> {
        todo!()
    }

    /// Registration 31 `IMemoryProbe`: `platform().memory_probe`.
    pub fn memory_probe(&self) -> Arc<dyn MemoryProbe> {
        todo!()
    }

    /// Registration 32 `INaturalComparer`: `platform().natural_comparer`.
    pub fn natural_comparer(&self) -> Arc<dyn NaturalComparer> {
        todo!()
    }

    /// Registration 33 `IDisplayClock`: `platform().display_clock`.
    pub fn display_clock(&self) -> Arc<dyn DisplayClock> {
        todo!()
    }

    /// Registration 35 `ViewportSizeSource`: `Arc::new(ViewportSizeSource::new())` (`viewport_size()` is `(0, 0)` until the window installs its accessor).
    pub fn viewport_size_source(&self) -> Arc<ViewportSizeSource> {
        todo!()
    }

    /// The OS seams (`PlatformServices`), for the few consumers outside the registry (`DialogServices`, the benchmark session factory).
    pub fn platform(&self) -> &PlatformServices {
        &self.inner.inputs.platform
    }

    /// `ram_budget::get_physical_memory_bytes(&*platform().physical_memory)`, read ONCE and cached (C# `RamBudgetPolicy.GetPhysicalMemoryBytes()` at each use; the value cannot change).
    pub fn physical_memory_bytes(&self) -> i64 {
        todo!()
    }

    // ---- 6. imaging and decoding (golden 37 to 42) ----

    /// The `DecoderDeps` of 09a: `{ source_reader(), codec: options.codec, memory_info: platform().memory_info, libraw_api: platform().libraw_api, fine_scaler: platform().fine_scaler,
    /// turbojpeg_texts: Arc::new(LocalizedTexts) }`.
    pub fn decoder_deps(&self) -> DecoderDeps {
        todo!()
    }

    /// Registration 37 `IImageDecoderFactory`, then `inputs.decoder_wrapper` when `Some`. `wpf = options.wpf_fallback_decoder.as_ref().map(|provider| provider())` is evaluated ONCE here
    /// (C# `wpfDecoder?.Invoke()`; production passes `None`), `codecs = Arc::new(|| pr_platform_win::wic::availability::current().clone())`. Built from
    /// `factories::create_webp_heic_decoder(&deps, wpf, Some(log()), Some(metrics()))`,
    /// `decoders::create_decoder_providers(&deps, options.wpf_fallback_decoder.clone(), Some(is_libraw_needed))` with `is_libraw_needed = settings.raw_support_enabled ||
    /// settings.decoder_backend == Some(DecoderBackend::LibRaw)` read at call time, and `DefaultImageDecoderFactory::new(providers, DecoderFactoryOptions { log, metrics,
    /// decoder_decorator: Some(|_backend, standard| FormatRoutingDecoder::new(WebpHeicRoutingDecoder::new(standard, webp_heic, enabled, codecs, texts, Some(log)),
    /// factories::create_raw_decoder(standard, source_bytes_policy()?.cache().cloned(), &deps), || settings.raw_support_enabled, texts)), wpf_decoder_factory: options.wpf_fallback_decoder })`.
    /// Resolving it builds `source_bytes_policy`, which reads the settings: call it only after they are loaded.
    pub fn decoder_factory(&self) -> Result<Arc<dyn ImageDecoderFactory>, RegistryError> {
        todo!()
    }

    /// Registration 38 `ThumbnailCache`: `ThumbnailCache::new(ThumbnailCacheServices { platform_codec: options.codec, cache_image_reader: platform().cache_image_reader,
    /// embedded_backend: platform().embedded_thumbnails, fs: file_system() }, ThumbnailCacheOptions { disk_directory: paths().thumbnail_cache_dir, log: log(), clock: clock(), ..new(dir) })`.
    pub fn thumbnail_cache(&self) -> Result<Arc<ThumbnailCache>, RegistryError> {
        todo!()
    }

    /// Registration 39 `SourceBytesCachePolicy`: reads `settings_store().current()`. `use_source_bytes_cache == false` -> `SourceBytesCachePolicy::new(None)`. Else
    /// `bytes = ram_budget::source_bytes_for_percent(s.source_bytes_capacity_bytes, s.image_cache_ram_percent, physical_memory_bytes(), PreloadWindow::from_settings(&s))`;
    /// `bytes <= 0` -> `log().warn("Source-bytes cache disabled: the RAM cache share leaves no room beyond the preview preload window.")` and `new(None)`; else
    /// `SourceBytesCache::new(bytes, physical_memory_bytes(), Some(source_reader()))` `.with_texts(LocalizedTexts)`.
    pub fn source_bytes_policy(&self) -> Result<Arc<SourceBytesCachePolicy>, RegistryError> {
        todo!()
    }

    /// Registration 40 `PreviewStateContext`: `PreviewStateContext::new()` whose three closures are replaced by the ones of registration 41 (`is_original_loading_mode` = loading mode is
    /// `Original`, `target_decode_box` = `viewport_size_source().target_decode_box()`, `current_backend` = the settings backend). Cloning shares the closures. The closures are
    /// installed the first time `preview_service()` runs (C# `PreviewImageService` factory), so a test that reads the context before it sees the C# defaults.
    pub fn preview_state(&self) -> PreviewStateContext {
        todo!()
    }

    /// Registration 41 `PreviewImageService`: `PreviewImageService::new(PreviewServiceConfig { metrics: metrics(), is_original_loading_mode, target_decode_box, platform_codec: options.codec,
    /// payload_codec: platform().payload_codec, capacity_bytes: s.image_cache_capacity_bytes, disk_cache_directory: Some(paths().preview_cache_dir), decoder_factory:
    /// Some(decoder_factory()?), current_backend, log: Some(log()), source_bytes_cache: source_bytes_policy()?.cache().cloned(), cache_ram_percent: Some(s.image_cache_ram_percent),
    /// preload_window: Some(PreloadWindow::from_settings(&s)), source_reader: Some(source_reader()), raw_full_decoder: libraw available ? Some(LibRawDecoder::new(codec, libraw_api,
    /// memory_info)) : None, is_raw_full_decode_enabled: raw_support_enabled && raw_full_decode == OnZoom (live), physical_memory_bytes: physical_memory_bytes(), file_system:
    /// Some(file_system()), clock: Some(clock()), ..PreviewServiceConfig::new(..) })` with `s = settings_store().current()` read ONCE (the capacity, percent and window apply after a restart,
    /// Q-AR6/Q-R19). Resolving it reads the settings: call it only after they are loaded.
    pub fn preview_service(&self) -> Result<Arc<PreviewImageService>, RegistryError> {
        todo!()
    }

    /// Registration 42: the scheduler factory (see [`PreloadSchedulerFactory`]). Each call builds `PreloadScheduler::with_thresholds(preview_service()?, metrics(), entries, memory_probe(),
    /// ThresholdOptions { full_folder_ram_threshold_bytes: preview_service.capacity_bytes(), memory_load_limit: s.preload_memory_load_limit, worker_count_override:
    /// Some(s.preload_worker_count), window: Some(PreloadWindow::from_settings(&s)) }, PreloadSchedulerDeps { log: Some(log()), prefetch_source_bytes: policy.cache().map(|c|
    /// factories::create_source_prefetch(c, webp_heic_enabled, codecs)), snapshot_version: Some(version), ..Default::default() })` with `s` read at THAT call.
    pub fn preload_scheduler_factory(&self) -> PreloadSchedulerFactory {
        todo!()
    }
}
