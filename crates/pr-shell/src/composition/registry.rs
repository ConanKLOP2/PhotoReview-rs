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
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

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
    file_system: OnceLock<Arc<dyn FileSystem>>,
    settings_store: OnceLock<Arc<SettingsStore>>,
    key_names: OnceLock<Arc<dyn KeyNameValidator>>,
    session_store: OnceLock<Arc<SessionStore>>,
    session_writer: OnceLock<Result<Arc<SessionWriter>, RegistryError>>,
    localization: OnceLock<SharedLocalization>,
    journal: OnceLock<Arc<OperationJournal>>,
    file_action_service: OnceLock<Arc<FileActionService>>,
    undo_service: OnceLock<Arc<Mutex<UndoService>>>,
    recovery_retry: OnceLock<Arc<RecoveryRetryService>>,
    file_hash_service: OnceLock<Result<Arc<FileHashService>, RegistryError>>,
    metrics: OnceLock<Arc<ReviewMetrics>>,
    explorer_snapshot: OnceLock<Arc<LatestExplorerSnapshot>>,
    viewport_size_source: OnceLock<Arc<ViewportSizeSource>>,
    physical_memory_bytes: OnceLock<i64>,
    decoder_factory: OnceLock<Result<Arc<dyn ImageDecoderFactory>, RegistryError>>,
    thumbnail_cache: OnceLock<Result<Arc<ThumbnailCache>, RegistryError>>,
    source_bytes_policy: OnceLock<Result<Arc<SourceBytesCachePolicy>, RegistryError>>,
    preview_state: OnceLock<PreviewStateContext>,
    preview_service: OnceLock<Result<Arc<PreviewImageService>, RegistryError>>,
    preload_scheduler_factory: OnceLock<PreloadSchedulerFactory>,
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
            inner: Arc::new(Inner {
                inputs,
                file_system: OnceLock::new(),
                settings_store: OnceLock::new(),
                key_names: OnceLock::new(),
                session_store: OnceLock::new(),
                session_writer: OnceLock::new(),
                localization: OnceLock::new(),
                journal: OnceLock::new(),
                file_action_service: OnceLock::new(),
                undo_service: OnceLock::new(),
                recovery_retry: OnceLock::new(),
                file_hash_service: OnceLock::new(),
                metrics: OnceLock::new(),
                explorer_snapshot: OnceLock::new(),
                viewport_size_source: OnceLock::new(),
                physical_memory_bytes: OnceLock::new(),
                decoder_factory: OnceLock::new(),
                thumbnail_cache: OnceLock::new(),
                source_bytes_policy: OnceLock::new(),
                preview_state: OnceLock::new(),
                preview_service: OnceLock::new(),
                preload_scheduler_factory: OnceLock::new(),
            }),
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
        Arc::clone(self.inner.file_system.get_or_init(|| {
            let counter: Arc<dyn pr_core::io::counting::StatCounter> = Arc::new({
                let metrics = self.metrics();
                move || metrics.record_stat()
            });
            Arc::new(pr_core::io::counting::CountingFileSystem::new(
                Arc::new(pr_core::io::physical::PhysicalFileSystem),
                counter,
            ))
        }))
    }

    /// Registration 14 `ISourceReader`: `inputs.source_reader`.
    pub fn source_reader(&self) -> Arc<dyn SourceReader> {
        Arc::clone(&self.inner.inputs.source_reader)
    }

    /// Registration 15 `IClock`: `inputs.clock`.
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.inner.inputs.clock)
    }

    /// Registration 16 `ILog`: `inputs.log` (the process-wide instance: the registry NEVER owns the log's life, as the C# comment of AR02c explains).
    pub fn log(&self) -> Arc<dyn Log> {
        Arc::clone(&self.inner.inputs.log)
    }

    // ---- 2. settings and session (golden 17 to 20) ----

    /// Registration 17 `SettingsStore`: `SettingsStore::new(paths.clone(), file_system(), log(), clock(), Arc::new(LocalizedTexts))` `.with_startup_error_callback(AppLog::error_forced adapter)`
    /// `.with_key_name_validator(key_names())` (AR11a). `current()` is the defaults until the start-up calls `load`.
    pub fn settings_store(&self) -> Arc<SettingsStore> {
        Arc::clone(self.inner.settings_store.get_or_init(|| {
            Arc::new(
                SettingsStore::new(
                    self.paths().clone(),
                    self.file_system(),
                    self.log(),
                    self.clock(),
                    Arc::new(super::texts::LocalizedTexts),
                )
                .with_startup_error_callback(Box::new(|message, error| {
                    pr_app::misc::app_log::AppLog::global().error_forced(message, error)
                }))
                .with_key_name_validator(self.key_names()),
            )
        }))
    }

    /// Registration 34 `IKeyNameValidator`: `Arc::new(pr_app::services::shortcut_key_name::KeyIdNameValidator)`.
    pub fn key_names(&self) -> Arc<dyn KeyNameValidator> {
        Arc::clone(
            self.inner
                .key_names
                .get_or_init(|| Arc::new(pr_app::services::shortcut_key_name::KeyIdNameValidator)),
        )
    }

    /// Registration 18 `SessionStore`: `SessionStore::new(paths(), file_system(), Some(counter))` `.with_log(log())` where `counter` is a `SessionWriteCounter` closure calling
    /// `metrics().record_session_write()`. Starts the sweep thread of the store (its own contract).
    pub fn session_store(&self) -> Arc<SessionStore> {
        Arc::clone(self.inner.session_store.get_or_init(|| {
            let metrics = self.metrics();
            let counter: Arc<dyn pr_core::session::store::SessionWriteCounter> =
                Arc::new(move || metrics.record_session_write());
            Arc::new(SessionStore::new(self.paths(), self.file_system(), Some(counter)).with_log(self.log()))
        }))
    }

    /// Registration 19 `SessionWriter`: `SessionWriter::new(session_store(), Some(log()), None, None)`; `Err` only for an invalid debounce (impossible with the defaults).
    pub fn session_writer(&self) -> Result<Arc<SessionWriter>, RegistryError> {
        self.inner
            .session_writer
            .get_or_init(|| {
                SessionWriter::new(self.session_store(), Some(self.log()), None, None)
                    .map(Arc::new)
                    .map_err(|error| RegistryError {
                        service: "session_writer",
                        message: error.to_string(),
                    })
            })
            .clone()
    }

    /// Registration 20 `LocalizationService`: `Arc::new(Mutex::new(LocalizationService::new(Arc::new(PhysicalFileSystem), LocalizationPaths { shipped_dir, user_languages_dir },
    /// platform().installed_ui_culture.clone(), platform().cultures.clone(), log())))` over a PLAIN file system on purpose (catalog reads must not count in the read budgets, ADR 0006).
    /// `shipped_dir` = `<directory of the running exe>\Languages` (C# `AppContext.BaseDirectory\Languages`), `user_languages_dir` = `<directory of paths().config_file>\Languages`.
    pub fn localization(&self) -> SharedLocalization {
        Arc::clone(self.inner.localization.get_or_init(|| {
            Arc::new(Mutex::new(pr_app::localization::service::LocalizationService::new(
                Arc::new(pr_core::io::physical::PhysicalFileSystem),
                pr_app::localization::service::LocalizationPaths {
                    shipped_dir: std::env::current_exe()
                        .ok()
                        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
                        .unwrap_or_default()
                        .join("Languages"),
                    user_languages_dir: self
                        .paths()
                        .config_file
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new(""))
                        .join("Languages"),
                },
                self.platform().installed_ui_culture.clone(),
                Arc::clone(&self.platform().cultures),
                self.log(),
            )))
        }))
    }

    // ---- 3. journal and file actions (golden 21 to 25) ----

    /// Registration 21 `OperationJournal`: `OperationJournal::with_options(paths(), file_system(), clock(), OperationJournalOptions { durability: Some(closure reading
    /// settings_store().current().journal_durability at EVERY write), live_operations: Some(live_operations()), compaction_files: Some(Arc::new(PhysicalJournalCompactionFiles)),
    /// log: Some(log()), texts: Some(Arc::new(LocalizedTexts)), ..Default::default() })`.
    pub fn journal(&self) -> Arc<OperationJournal> {
        Arc::clone(self.inner.journal.get_or_init(|| {
            let store = self.settings_store();
            Arc::new(OperationJournal::with_options(
                self.paths(),
                self.file_system(),
                self.clock(),
                pr_core::file_actions::journal::OperationJournalOptions {
                    durability: Some(Arc::new(move || store.current().journal_durability)),
                    live_operations: Some(self.live_operations()),
                    compaction_files: Some(Arc::new(
                        pr_core::io::journal_compaction_files::PhysicalJournalCompactionFiles,
                    )),
                    log: Some(self.log()),
                    texts: Some(Arc::new(super::texts::LocalizedTexts)),
                    ..Default::default()
                },
            ))
        }))
    }

    /// Registration 24 `FileActionService`: `FileActionService::new(journal(), file_system(), clock(), recycle_bin())` `.with_texts(LocalizedTexts)` and, when `inputs.move_override` is
    /// `Some`, `.with_move_override(into_move_override(over))`.
    pub fn file_action_service(&self) -> Arc<FileActionService> {
        Arc::clone(self.inner.file_action_service.get_or_init(|| {
            let mut service =
                FileActionService::new(self.journal(), self.file_system(), self.clock(), self.recycle_bin())
                    .with_texts(Arc::new(super::texts::LocalizedTexts));
            if let Some(over) = self.inner.inputs.move_override.clone() {
                service = service.with_move_override(pr_app::coordinators::traits::into_move_override(over));
            }
            Arc::new(service)
        }))
    }

    /// Registration 25 `UndoService`: `UndoService::new(journal(), file_system(), recycle_bin(), Arc::new(LocalizedTexts))` `.with_gate(file_action_service())` (INV-4: one gate for file
    /// actions and undo) and the same move override; wrapped as `Arc<Mutex<UndoService>>` (the shape of `MainViewModelDeps::undo_service` and `FileActionDeps::undo_service`).
    pub fn undo_service(&self) -> Arc<Mutex<UndoService>> {
        Arc::clone(self.inner.undo_service.get_or_init(|| {
            let mut service = UndoService::new(
                self.journal(),
                self.file_system(),
                self.recycle_bin(),
                Arc::new(super::texts::LocalizedTexts),
            )
            .with_gate(self.file_action_service());
            if let Some(over) = self.inner.inputs.move_override.clone() {
                service = service.with_move_override(pr_app::coordinators::traits::into_move_override(over));
            }
            Arc::new(Mutex::new(service))
        }))
    }

    /// Registration 22 `RecoveryRetryService`: `factories::create_recovery_retry_service(journal(), file_system(), clock(), recycle_bin(), settings_store(), Arc::new(LocalizedTexts),
    /// Some(file_action_service() as the FileActionGate))` (RV-C08: the retry shares the app's INV-4 gate).
    pub fn recovery_retry(&self) -> Arc<RecoveryRetryService> {
        Arc::clone(self.inner.recovery_retry.get_or_init(|| {
            Arc::new(super::factories::create_recovery_retry_service(
                self.journal(),
                self.file_system(),
                self.clock(),
                self.recycle_bin(),
                self.settings_store(),
                Arc::new(super::texts::LocalizedTexts),
                Some(self.file_action_service()),
            ))
        }))
    }

    /// Registration 23 `FileHashService`: `FileHashService::with_seams(source_bytes_policy()?.cache().cloned(), file_system(), source_reader(), Arc::new(LocalizedTexts))`;
    /// `Err` for `InvalidCapacity` (impossible) and for a `source_bytes_policy` error.
    pub fn file_hash_service(&self) -> Result<Arc<FileHashService>, RegistryError> {
        self.inner
            .file_hash_service
            .get_or_init(|| {
                FileHashService::with_seams(
                    self.source_bytes_policy()?.cache().cloned(),
                    self.file_system(),
                    self.source_reader(),
                    Arc::new(super::texts::LocalizedTexts),
                )
                .map(Arc::new)
                .map_err(|error| RegistryError {
                    service: "file_hash_service",
                    message: error.to_string(),
                })
            })
            .clone()
    }

    // ---- 4. diagnostics (golden 26, 27) ----

    /// Registration 26 `ReviewMetrics`: `Arc::new(ReviewMetrics::new())`.
    pub fn metrics(&self) -> Arc<ReviewMetrics> {
        Arc::clone(self.inner.metrics.get_or_init(|| Arc::new(ReviewMetrics::new())))
    }

    /// Registration 27 `LatestExplorerSnapshot`: `Arc::new(LatestExplorerSnapshot::new())`.
    pub fn explorer_snapshot(&self) -> Arc<LatestExplorerSnapshot> {
        Arc::clone(
            self.inner
                .explorer_snapshot
                .get_or_init(|| Arc::new(LatestExplorerSnapshot::new())),
        )
    }

    // ---- 5. platform services (golden 28 to 33, 35, 36) ----

    /// Registration 28 `IExplorerOrderProvider`: `platform().explorer_order`.
    pub fn explorer_order(&self) -> Arc<dyn ExplorerOrderProvider> {
        Arc::clone(&self.inner.inputs.platform.explorer_order)
    }

    /// Registration 29 `IRecycleBin`: `platform().recycle_bin`.
    pub fn recycle_bin(&self) -> Arc<dyn RecycleBin> {
        Arc::clone(&self.inner.inputs.platform.recycle_bin)
    }

    /// Registration 30 `ILiveOperationRegistry`: `platform().live_operations`.
    pub fn live_operations(&self) -> Arc<dyn LiveOperationRegistry> {
        Arc::clone(&self.inner.inputs.platform.live_operations)
    }

    /// Registration 31 `IMemoryProbe`: `platform().memory_probe`.
    pub fn memory_probe(&self) -> Arc<dyn MemoryProbe> {
        Arc::clone(&self.inner.inputs.platform.memory_probe)
    }

    /// Registration 32 `INaturalComparer`: `platform().natural_comparer`.
    pub fn natural_comparer(&self) -> Arc<dyn NaturalComparer> {
        Arc::clone(&self.inner.inputs.platform.natural_comparer)
    }

    /// Registration 33 `IDisplayClock`: `platform().display_clock`.
    pub fn display_clock(&self) -> Arc<dyn DisplayClock> {
        Arc::clone(&self.inner.inputs.platform.display_clock)
    }

    /// Registration 35 `ViewportSizeSource`: `Arc::new(ViewportSizeSource::new())` (`viewport_size()` is `(0, 0)` until the window installs its accessor).
    pub fn viewport_size_source(&self) -> Arc<ViewportSizeSource> {
        Arc::clone(
            self.inner
                .viewport_size_source
                .get_or_init(|| Arc::new(ViewportSizeSource::new())),
        )
    }

    /// The OS seams (`PlatformServices`), for the few consumers outside the registry (`DialogServices`, the benchmark session factory).
    pub fn platform(&self) -> &PlatformServices {
        &self.inner.inputs.platform
    }

    /// `ram_budget::get_physical_memory_bytes(&*platform().physical_memory)`, read ONCE and cached (C# `RamBudgetPolicy.GetPhysicalMemoryBytes()` at each use; the value cannot change).
    pub fn physical_memory_bytes(&self) -> i64 {
        *self.inner.physical_memory_bytes.get_or_init(|| {
            pr_imaging::ram_budget::get_physical_memory_bytes(&*self.inner.inputs.platform.physical_memory)
        })
    }

    // ---- 6. imaging and decoding (golden 37 to 42) ----

    /// The `DecoderDeps` of 09a: `{ source_reader(), codec: options.codec, memory_info: platform().memory_info, libraw_api: platform().libraw_api, fine_scaler: platform().fine_scaler,
    /// turbojpeg_texts: Arc::new(LocalizedTexts) }`.
    pub fn decoder_deps(&self) -> DecoderDeps {
        DecoderDeps {
            source_reader: self.source_reader(),
            codec: Arc::clone(&self.inner.inputs.options.codec),
            memory_info: Arc::clone(&self.platform().memory_info),
            libraw_api: Arc::clone(&self.platform().libraw_api),
            fine_scaler: Arc::clone(&self.platform().fine_scaler),
            turbojpeg_texts: Arc::new(super::texts::LocalizedTexts),
        }
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
        self.inner
            .decoder_factory
            .get_or_init(|| {
                let deps = self.decoder_deps();
                let options = self.inner.inputs.options.clone();
                let wpf = options.wpf_fallback_decoder.as_ref().map(|provider| provider());
                let webp =
                    super::factories::create_webp_heic_decoder(&deps, wpf, Some(self.log()), Some(self.metrics()));
                let source_bytes_cache = self.source_bytes_policy()?.cache().cloned();
                let store = self.settings_store();
                let enabled: Arc<dyn Fn() -> bool + Send + Sync> =
                    Arc::new(move || store.current().webp_heic_support_enabled);
                let codecs: Arc<dyn Fn() -> pr_imaging::webp_heic::WicCodecSupport + Send + Sync> =
                    Arc::new(|| pr_platform_win::wic::availability::current().clone());
                let raw_setting_store = self.settings_store();
                let is_libraw_needed: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
                    let settings = raw_setting_store.current();
                    settings.raw_support_enabled
                        || settings.decoder_backend == Some(pr_core::model::enums::DecoderBackend::LibRaw)
                });
                let providers = super::decoders::create_decoder_providers(
                    &deps,
                    options.wpf_fallback_decoder.clone(),
                    Some(is_libraw_needed),
                );
                let shared = self.clone();
                let decoder_decorator: pr_imaging::factory::DecoderDecorator = Arc::new(move |_backend, standard| {
                    let texts: Arc<dyn pr_imaging::webp_heic::WebpHeicTexts> = Arc::new(super::texts::LocalizedTexts);
                    let standard: Arc<dyn pr_imaging::decoder_traits::ImageDecoder> =
                        Arc::new(pr_imaging::webp_heic::WebpHeicRoutingDecoder::new(
                            standard,
                            Arc::clone(&webp),
                            Arc::clone(&enabled),
                            Arc::clone(&codecs),
                            Arc::clone(&texts),
                            None,
                        ));
                    let raw_enabled_store = shared.settings_store();
                    let raw_enabled: Arc<dyn Fn() -> bool + Send + Sync> =
                        Arc::new(move || raw_enabled_store.current().raw_support_enabled);
                    let raw =
                        super::factories::create_raw_decoder(Arc::clone(&standard), source_bytes_cache.clone(), &deps);
                    let raw_texts: Arc<dyn pr_imaging_raw::routing::RawRoutingTexts> =
                        Arc::new(super::texts::LocalizedTexts);
                    let routed: Arc<dyn pr_imaging::decoder_traits::ImageDecoder> =
                        Arc::new(pr_imaging_raw::routing::FormatRoutingDecoder::new(
                            standard,
                            Arc::new(raw),
                            raw_enabled,
                            raw_texts,
                        ));
                    routed
                });
                let factory = pr_imaging::factory::DefaultImageDecoderFactory::new(
                    providers,
                    pr_imaging::factory::DecoderFactoryOptions {
                        log: Some(self.log()),
                        metrics: Some(self.metrics()),
                        decoder_decorator: Some(decoder_decorator),
                        wpf_decoder_factory: options.wpf_fallback_decoder,
                    },
                )
                .map_err(|error| RegistryError {
                    service: "decoder_factory",
                    message: error.to_string(),
                })?;
                let factory: Arc<dyn ImageDecoderFactory> = Arc::new(factory);
                Ok(self
                    .inner
                    .inputs
                    .decoder_wrapper
                    .as_ref()
                    .map_or(factory.clone(), |wrap| wrap(factory)))
            })
            .clone()
    }

    /// Registration 38 `ThumbnailCache`: `ThumbnailCache::new(ThumbnailCacheServices { platform_codec: options.codec, cache_image_reader: platform().cache_image_reader,
    /// embedded_backend: platform().embedded_thumbnails, fs: file_system() }, ThumbnailCacheOptions { disk_directory: paths().thumbnail_cache_dir, log: log(), clock: clock(), ..new(dir) })`.
    pub fn thumbnail_cache(&self) -> Result<Arc<ThumbnailCache>, RegistryError> {
        self.inner
            .thumbnail_cache
            .get_or_init(|| {
                let services = pr_imaging::cache::thumbnails::ThumbnailCacheServices {
                    platform_codec: Arc::clone(&self.inner.inputs.options.codec),
                    cache_image_reader: Arc::clone(&self.platform().cache_image_reader),
                    embedded_backend: Arc::clone(&self.platform().embedded_thumbnails),
                    fs: self.file_system(),
                };
                let options =
                    pr_imaging::cache::thumbnails::ThumbnailCacheOptions::new(self.paths().thumbnail_cache_dir.clone());
                ThumbnailCache::new(
                    services,
                    pr_imaging::cache::thumbnails::ThumbnailCacheOptions {
                        log: self.log(),
                        clock: self.clock(),
                        ..options
                    },
                )
                .map(Arc::new)
                .map_err(|error| RegistryError {
                    service: "thumbnail_cache",
                    message: error.to_string(),
                })
            })
            .clone()
    }

    /// Registration 39 `SourceBytesCachePolicy`: reads `settings_store().current()`. `use_source_bytes_cache == false` -> `SourceBytesCachePolicy::new(None)`. Else
    /// `bytes = ram_budget::source_bytes_for_percent(s.source_bytes_capacity_bytes, s.image_cache_ram_percent, physical_memory_bytes(), PreloadWindow::from_settings(&s))`;
    /// `bytes <= 0` -> `log().warn("Source-bytes cache disabled: the RAM cache share leaves no room beyond the preview preload window.")` and `new(None)`; else
    /// `SourceBytesCache::new(bytes, physical_memory_bytes(), Some(source_reader()))` `.with_texts(LocalizedTexts)`.
    pub fn source_bytes_policy(&self) -> Result<Arc<SourceBytesCachePolicy>, RegistryError> {
        self.inner.source_bytes_policy.get_or_init(|| {
            let settings = self.settings_store().current();
            let cache = if !settings.use_source_bytes_cache { None } else {
                let window = pr_imaging::preload_order::PreloadWindow::from_settings(&settings);
                let bytes = pr_imaging::ram_budget::source_bytes_for_percent(settings.source_bytes_capacity_bytes,
                    settings.image_cache_ram_percent, self.physical_memory_bytes(), window);
                if bytes <= 0 { self.log().warn("Source-bytes cache disabled: the RAM cache share leaves no room beyond the preview preload window."); None }
                else { Some(Arc::new(pr_imaging::cache::source_bytes::SourceBytesCache::new(bytes, self.physical_memory_bytes(), Some(self.source_reader()))
                    .map_err(|error| RegistryError { service: "source_bytes_policy", message: error.to_string() })?
                    .with_texts(Arc::new(super::texts::LocalizedTexts)))) }
            };
            Ok(Arc::new(SourceBytesCachePolicy::new(cache)))
        }).clone()
    }

    /// Registration 40 `PreviewStateContext`: `PreviewStateContext::new()` whose three closures are replaced by the ones of registration 41 (`is_original_loading_mode` = loading mode is
    /// `Original`, `target_decode_box` = `viewport_size_source().target_decode_box()`, `current_backend` = the settings backend). Cloning shares the closures. The closures are
    /// installed the first time `preview_service()` runs (C# `PreviewImageService` factory), so a test that reads the context before it sees the C# defaults.
    pub fn preview_state(&self) -> PreviewStateContext {
        self.inner
            .preview_state
            .get()
            .cloned()
            .unwrap_or_else(PreviewStateContext::new)
    }

    /// Registration 41 `PreviewImageService`: `PreviewImageService::new(PreviewServiceConfig { metrics: metrics(), is_original_loading_mode, target_decode_box, platform_codec: options.codec,
    /// payload_codec: platform().payload_codec, capacity_bytes: s.image_cache_capacity_bytes, disk_cache_directory: Some(paths().preview_cache_dir), decoder_factory:
    /// Some(decoder_factory()?), current_backend, log: Some(log()), source_bytes_cache: source_bytes_policy()?.cache().cloned(), cache_ram_percent: Some(s.image_cache_ram_percent),
    /// preload_window: Some(PreloadWindow::from_settings(&s)), source_reader: Some(source_reader()), raw_full_decoder: libraw available ? Some(LibRawDecoder::new(codec, libraw_api,
    /// memory_info)) : None, is_raw_full_decode_enabled: raw_support_enabled && raw_full_decode == OnZoom (live), physical_memory_bytes: physical_memory_bytes(), file_system:
    /// Some(file_system()), clock: Some(clock()), ..PreviewServiceConfig::new(..) })` with `s = settings_store().current()` read ONCE (the capacity, percent and window apply after a restart,
    /// Q-AR6/Q-R19). Resolving it reads the settings: call it only after they are loaded.
    pub fn preview_service(&self) -> Result<Arc<PreviewImageService>, RegistryError> {
        self.inner
            .preview_service
            .get_or_init(|| {
                let settings = self.settings_store().current();
                let preview_state = PreviewStateContext {
                    is_original_loading_mode: Arc::new({
                        let store = self.settings_store();
                        move || store.current().loading_mode == Some(pr_core::model::enums::LoadingMode::Original)
                    }),
                    target_decode_box: Arc::new({
                        let viewport = self.viewport_size_source();
                        move || viewport.target_decode_box()
                    }),
                    current_backend: Arc::new({
                        let store = self.settings_store();
                        move || {
                            store
                                .current()
                                .decoder_backend
                                .unwrap_or(pr_core::model::enums::DecoderBackend::Wpf)
                        }
                    }),
                };
                let _ = self.inner.preview_state.set(preview_state.clone());
                let mut config = pr_imaging::service::PreviewServiceConfig::new(
                    self.metrics(),
                    Arc::clone(&preview_state.is_original_loading_mode),
                    Arc::clone(&preview_state.target_decode_box),
                    Arc::clone(&self.inner.inputs.options.codec),
                    Arc::clone(&self.platform().payload_codec),
                );
                config.capacity_bytes = settings.image_cache_capacity_bytes;
                config.disk_cache_directory = Some(self.paths().preview_cache_dir.clone());
                config.decoder_factory = Some(self.decoder_factory()?);
                config.current_backend = Some(Arc::clone(&preview_state.current_backend));
                config.log = Some(self.log());
                config.source_bytes_cache = self.source_bytes_policy()?.cache().cloned();
                config.cache_ram_percent = Some(settings.image_cache_ram_percent);
                config.preload_window = Some(pr_imaging::preload_order::PreloadWindow::from_settings(&settings));
                config.source_reader = Some(self.source_reader());
                config.physical_memory_bytes = self.physical_memory_bytes();
                config.file_system = Some(self.file_system());
                config.clock = Some(self.clock());
                let raw_enabled_store = self.settings_store();
                if pr_libraw::availability::probe().available {
                    config.raw_full_decoder = Some(Arc::new(pr_libraw::decoder::LibRawDecoder::new(
                        Arc::clone(&self.inner.inputs.options.codec),
                        Arc::clone(&self.platform().libraw_api),
                        Arc::clone(&self.platform().memory_info),
                    )));
                }
                config.is_raw_full_decode_enabled = Some(Arc::new(move || {
                    let current = raw_enabled_store.current();
                    current.raw_support_enabled
                        && current.raw_full_decode == pr_core::model::enums::RawFullDecode::OnZoom
                }));
                PreviewImageService::new(config)
                    .map(Arc::new)
                    .map_err(|error| RegistryError {
                        service: "preview_service",
                        message: error.to_string(),
                    })
            })
            .clone()
    }

    /// Registration 42: the scheduler factory (see [`PreloadSchedulerFactory`]). Each call builds `PreloadScheduler::with_thresholds(preview_service()?, metrics(), entries, memory_probe(),
    /// ThresholdOptions { full_folder_ram_threshold_bytes: preview_service.capacity_bytes(), memory_load_limit: s.preload_memory_load_limit, worker_count_override:
    /// Some(s.preload_worker_count), window: Some(PreloadWindow::from_settings(&s)) }, PreloadSchedulerDeps { log: Some(log()), prefetch_source_bytes: policy.cache().map(|c|
    /// factories::create_source_prefetch(c, webp_heic_enabled, codecs)), snapshot_version: Some(version), ..Default::default() })` with `s` read at THAT call.
    pub fn preload_scheduler_factory(&self) -> PreloadSchedulerFactory {
        Arc::clone(self.inner.preload_scheduler_factory.get_or_init(|| {
            let shared = self.clone();
            Arc::new(move |entries, version| {
                let settings = shared.settings_store().current();
                let service = shared.preview_service()?;
                let thresholds = pr_imaging::preload::scheduler::ThresholdOptions {
                    full_folder_ram_threshold_bytes: service.capacity_bytes(),
                    memory_load_limit: settings.preload_memory_load_limit,
                    worker_count_override: Some(settings.preload_worker_count),
                    window: Some(pr_imaging::preload_order::PreloadWindow::from_settings(&settings)),
                };
                let policy = shared.source_bytes_policy()?;
                let mut deps = pr_imaging::preload::scheduler::PreloadSchedulerDeps {
                    log: Some(shared.log()),
                    snapshot_version: Some(version),
                    ..Default::default()
                };
                if let Some(cache) = policy.cache() {
                    let store = shared.settings_store();
                    let enabled: Arc<dyn Fn() -> bool + Send + Sync> =
                        Arc::new(move || store.current().webp_heic_support_enabled);
                    let codecs: Arc<dyn Fn() -> pr_imaging::webp_heic::WicCodecSupport + Send + Sync> =
                        Arc::new(|| pr_platform_win::wic::availability::current().clone());
                    deps.prefetch_source_bytes = Some(super::factories::create_source_prefetch(
                        Arc::clone(cache),
                        enabled,
                        codecs,
                    ));
                }
                Ok(Arc::new(PreloadScheduler::with_thresholds(
                    service,
                    shared.metrics(),
                    entries,
                    shared.memory_probe(),
                    thresholds,
                    deps,
                )))
            }) as PreloadSchedulerFactory
        }))
    }
}
