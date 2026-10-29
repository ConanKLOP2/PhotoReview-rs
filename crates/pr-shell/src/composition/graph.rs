//! The production `GraphBuilder` of W8-17d: `SharedServices` + the view model factory + `DialogServices` over the
//! host services of W8-17c. Unit W8-09b. Contract: `contracts/W8-09b.md`. C# counterparts: `ShellCompositionRoot.Build(host, hub, overrides)` (the graph half, section 2) and
//! `AppHost.BuildServices(overrides)` (`PhotoReview.App/Composition/AppHost.cs`, folded in), baseline `7eaee2a2`.
//!
//! Why this file is in `composition` and not in `startup/graph.rs`: `graph.rs` belongs to W8-17d (its stub declares `StartupGraph`, `GraphInputs`, `GraphBuilder`). This unit adds ONE
//! cross-unit edit there at port time, listed in the contract: `production_graph_builder()` becomes `production_graph_builder_with(GraphOverrides::default())` and `graph.rs` gains
//! `pub use crate::composition::graph::{GraphOverrides, production_graph_builder_with};`, so the public paths are `pr_shell::startup::graph::{GraphOverrides,
//! production_graph_builder_with}` as D-W8B6-47 (W8-20b) requires.

use std::rc::Rc;
use std::sync::Arc;

use pr_core::abstractions::{Clock, ExplorerOrderProvider, Log, RecycleBin};
use pr_core::app_paths::AppPaths;
use pr_platform_win::host::main_window::core::SharedViewModel;

use super::registry::{DecoderFactoryWrapper, RegistryError, SharedServices};
use crate::startup::graph::{GraphBuilder, GraphInputs};
use crate::startup::secondary_dialogs::DialogServices;

/// The Rust twin of C# `ShellStartupOptions.ServiceOverrides` (`Action<IServiceCollection>`: `services.AddSingleton<IAppPaths>(..)`, `ILog`, `IRecycleBin`, ...), restricted to the six
/// overrides the integration tests use (D-W8B6-47). `None` = the production service. Shape fixed by the W8-20b contract.
#[derive(Default)]
pub struct GraphOverrides {
    /// Replaces `AppPaths::from_environment()` (a test points every file under a temp root; R0002).
    pub paths: Option<AppPaths>,
    /// Replaces the process log given in `GraphInputs::log`.
    pub log: Option<Arc<dyn Log>>,
    /// Replaces the real Recycle Bin. A test that moves or deletes files MUST pass one (AGENTS.md: never the user's real bin).
    pub recycle_bin: Option<Arc<dyn RecycleBin>>,
    /// Replaces the Explorer order service (no STA pump thread is started when `Some`).
    pub explorer_order: Option<Arc<dyn ExplorerOrderProvider>>,
    /// Replaces `SystemClock`.
    pub clock: Option<Arc<dyn Clock>>,
    /// Wraps the registry's decoder factory (`ShellStalePreviewTests` decorates `IImageDecoderFactory`).
    pub decoder_wrapper: Option<DecoderFactoryWrapper>,
}

impl std::fmt::Debug for GraphOverrides {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphOverrides")
            .field("paths", &self.paths.is_some())
            .field("log", &self.log.is_some())
            .field("recycle_bin", &self.recycle_bin.is_some())
            .field("explorer_order", &self.explorer_order.is_some())
            .field("clock", &self.clock.is_some())
            .field("decoder_wrapper", &self.decoder_wrapper.is_some())
            .finish()
    }
}

/// The production builder with `overrides` applied (C# `ShellCompositionRoot.Build(host, hub, overrides)` + `AppHost.BuildServices`). The returned closure runs on the UI thread, once:
///
/// 1. `log = overrides.log.unwrap_or(inputs.log)`; `paths = overrides.paths.unwrap_or_else(AppPaths::from_environment)`; `clock = overrides.clock.unwrap_or(SystemClock)`.
/// 2. `platform = PlatformServices::production(log, PlatformOverrides { recycle_bin: overrides.recycle_bin, explorer_order: overrides.explorer_order })?` (an overridden adapter is NOT
///    created: no Explorer pump thread, no real Recycle Bin facade).
/// 3. `services = SharedServices::build(SharedServiceInputs { options: SharedServiceOptions { codec: PixelBufferImageCodec::shared(), wpf_fallback_decoder: None, thumbnail_decoder:
///    WicDirectDecoder provider }, paths, log, clock, source_reader: PhysicalSourceReader, platform, decoder_wrapper: overrides.decoder_wrapper, move_override: None })`.
/// 4. `StartupGraph { log, config_file: paths.config_file, window_placement_file: paths.window_placement_file, explorer_order, settings_store, localization, previews:
///    Arc::new(LazyPrewarmPreviews::new(services.clone())), size_source: viewport_size_source(), display_clock, view_model: build_shared_view_model(..)?, journal, clock,
///    dialog_services: dialog_services_of(&services) }`. D-W8B6-160 makes `view_model` a factory the window step calls after loaded settings have been applied.
///
/// An `Err` becomes `StartupError::Graph(text)` in 17d. Nothing here reads `%LOCALAPPDATA%` when `overrides.paths` is `Some` (R0002; 17c/09b note of the B6 reconciliation).
pub fn production_graph_builder_with(overrides: GraphOverrides) -> GraphBuilder {
    Box::new(move |inputs| {
        use super::adapters::LazyPrewarmPreviews;
        use super::contracts::SharedServiceOptions;
        use super::platform::{PlatformOverrides, PlatformServices};
        use super::registry::SharedServiceInputs;
        use crate::startup::{error::StartupError, graph::StartupGraph};
        use pr_core::abstractions::{PhysicalSourceReader, SystemClock};
        use pr_imaging::pixel_memory::PixelBufferImageCodec;

        let log = overrides.log.unwrap_or_else(|| inputs.log.clone());
        let paths = overrides.paths.unwrap_or_else(AppPaths::from_environment);
        let clock = overrides.clock.unwrap_or_else(|| Arc::new(SystemClock));
        let platform = PlatformServices::production(
            log.clone(),
            PlatformOverrides {
                recycle_bin: overrides.recycle_bin,
                explorer_order: overrides.explorer_order,
            },
        )?;
        let codec = PixelBufferImageCodec::shared();
        let thumbnail_codec = codec.clone();
        let memory = platform.memory_info.clone();
        let services = SharedServices::build(SharedServiceInputs {
            options: SharedServiceOptions {
                codec,
                wpf_fallback_decoder: None,
                thumbnail_decoder: Arc::new(move || {
                    Arc::new(pr_platform_win::wic::decoder::WicDirectDecoder::new(
                        thumbnail_codec.clone(),
                        None,
                        memory.clone(),
                    ))
                }),
            },
            paths: paths.clone(),
            log: log.clone(),
            clock: clock.clone(),
            source_reader: Arc::new(PhysicalSourceReader),
            platform,
            decoder_wrapper: overrides.decoder_wrapper,
            move_override: None,
        });
        let lazy_services = services.clone();
        Ok(StartupGraph {
            log,
            config_file: paths.config_file,
            window_placement_file: paths.window_placement_file,
            explorer_order: services.explorer_order(),
            settings_store: services.settings_store(),
            localization: services.localization(),
            previews: Arc::new(LazyPrewarmPreviews::new(services.clone())),
            size_source: services.viewport_size_source(),
            display_clock: services.display_clock(),
            view_model: Box::new(move || {
                build_shared_view_model(&lazy_services, &inputs).map_err(|error| StartupError::Graph(error.to_string()))
            }),
            journal: services.journal(),
            clock,
            dialog_services: dialog_services_of(&services),
        })
    })
}

/// C# `MainViewModelCompositionRoot.Create` + the shell's registrations, wrapped as the `Rc<RefCell<MainViewModel>>` the window shares (`SharedViewModel`):
/// `MainViewModelHost { ui_scheduler: inputs.services.ui_scheduler.clone(), dialog_service: Arc::new(SharedDialogService(inputs.services.dialog_service.clone())), folder_picker:
/// Some(inputs.services.new_folder_picker()), clipboard: Box::new(SharedClipboard(inputs.services.clipboard.clone())), sink_factory:
/// Box::new(Win32SinkFactoryAdapter(inputs.services.sink_factory.clone())), observer: Rc::new(NullPresentationObserver), wake: view_model_wake(inputs.dispatcher.clone()),
/// preload_controller: None, runner: Arc::new(ThreadRunner) }`, then `Rc::new(RefCell::new(create_main_view_model(services, host)?))`.
/// The `Arc::new(SharedDialogService(..))` line carries `#[allow(clippy::arc_with_non_send_sync)]` (justification in `adapters.rs`).
pub fn build_shared_view_model(
    services: &SharedServices,
    inputs: &GraphInputs,
) -> Result<SharedViewModel, RegistryError> {
    use super::{
        adapters::{SharedClipboard, SharedDialogService, Win32SinkFactoryAdapter},
        main_view_model::{MainViewModelHost, create_main_view_model},
    };
    use pr_app::coordinators::zoom_detail::ThreadRunner;
    use pr_app::services::contracts::NullPresentationObserver;
    use pr_platform_win::host::main_window::lifecycle::view_model_wake;
    use std::cell::RefCell;
    // The dialog facade is confined to the UI thread, like the C# host service.
    #[allow(clippy::arc_with_non_send_sync)]
    let dialog_service = Arc::new(SharedDialogService(inputs.services.dialog_service.clone()));
    let host = MainViewModelHost {
        ui_scheduler: inputs.services.ui_scheduler.clone(),
        dialog_service,
        folder_picker: Some(inputs.services.new_folder_picker()),
        clipboard: Box::new(SharedClipboard(inputs.services.clipboard.clone())),
        sink_factory: Box::new(Win32SinkFactoryAdapter(inputs.services.sink_factory.clone())),
        observer: Rc::new(NullPresentationObserver),
        wake: view_model_wake(inputs.dispatcher.clone()),
        preload_controller: None,
        runner: Arc::new(ThreadRunner),
    };
    Ok(Rc::new(RefCell::new(create_main_view_model(services, host)?)))
}

/// The 14 fields of W8-17c's `DialogServices` from the registry (D-W8B6-10, filled HERE and not by 17e: 17d's `StartupGraph::dialog_services` is the seam that exists):
/// `settings_store: Some(settings_store())`, `localization: Some(localization())`, `update_checker: Some(update_http::update_checker())` (inert factory replacing the C# presenter's
/// default checker fallback), `decoder_factory: Some(Arc::new(LazyDecoderFactory::new(services.clone())))`, `journal: Some(journal())`, `recovery_retry: Some(recovery_retry())`,
/// `file_system: file_system()`, `metrics: Some(metrics())`, `explorer_snapshot: Some(explorer_snapshot())`, `settings_texts: Arc::new(LocalizedTexts)`, `shell_integration:
/// platform().shell_integration`, `physical_memory_bytes: physical_memory_bytes()`, `runner: Arc::new(ThreadRunner)`, `benchmark_sessions: production_session_factory(
/// BenchmarkSessionDeps { platform_codec: PixelBufferImageCodec::shared(), payload_codec: platform().payload_codec, decoder_factory: Arc::new(|| WicDirectDecoder::new(codec, None,
/// memory_info)), real_recycle_bin: Arc::new(move || recycle_bin()), physical_memory_bytes, memory_probe: Some(memory_probe()) })`.
pub fn dialog_services_of(services: &SharedServices) -> Rc<DialogServices> {
    use super::{adapters::LazyDecoderFactory, texts::LocalizedTexts};
    use pr_app::{
        coordinators::zoom_detail::ThreadRunner,
        presenters::benchmark::{BenchmarkSessionDeps, production_session_factory},
    };
    use pr_imaging::pixel_memory::PixelBufferImageCodec;
    let platform = services.platform();
    let codec = PixelBufferImageCodec::shared();
    let memory = platform.memory_info.clone();
    let bin = services.recycle_bin();
    let physical_memory_bytes = services.physical_memory_bytes();
    Rc::new(DialogServices {
        settings_store: Some(services.settings_store()),
        localization: Some(services.localization()),
        update_checker: Some(super::update_http::update_checker()),
        decoder_factory: Some(Arc::new(LazyDecoderFactory::new(services.clone()))),
        journal: Some(services.journal()),
        recovery_retry: Some(services.recovery_retry()),
        file_system: services.file_system(),
        metrics: Some(services.metrics()),
        explorer_snapshot: Some(services.explorer_snapshot()),
        settings_texts: Arc::new(LocalizedTexts),
        shell_integration: platform.shell_integration.clone(),
        physical_memory_bytes,
        runner: Arc::new(ThreadRunner),
        benchmark_sessions: production_session_factory(BenchmarkSessionDeps {
            platform_codec: codec.clone(),
            payload_codec: platform.payload_codec.clone(),
            decoder_factory: Arc::new(move || {
                Arc::new(pr_platform_win::wic::decoder::WicDirectDecoder::new(
                    codec.clone(),
                    None,
                    memory.clone(),
                ))
            }),
            real_recycle_bin: Arc::new(move || bin.clone()),
            physical_memory_bytes,
            memory_probe: Some(platform.memory_probe.clone()),
        }),
    })
}
