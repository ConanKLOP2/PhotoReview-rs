//! Port of the service lookups of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` (the `services.GetRequiredService<T>()` calls of lines 262-282, 400, 430, 439-442, 578; baseline
//! `d92d6aae`). Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! Rust has no DI container (plan section 09). The shared graph (settings store, journal, view model, preview service...) is built by W8-09b (`composition::registry` +
//! `composition::main_view_model`, batch B6, contracted separately); its type names were not fixed when this contract was written. The seam is a plain struct with ONE field per
//! `GetRequiredService<T>()` the start-up makes ([`StartupGraph`], the same decision as `DialogServices`, D-W8B6-10) and a builder closure ([`GraphBuilder`], the C#
//! `ShellCompositionRoot.Build(host, hub, overrides)` step): the 09b porter fills the body of [`production_graph_builder`] and nothing else here changes.

use std::error::Error;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::presenters::settings::load::SharedLocalization;
use pr_app::services::prewarm::PrewarmPreviews;
use pr_app::services::viewport_size_source::ViewportSizeSource;
use pr_app::startup::early_decode::MarkFn;
use pr_core::abstractions::{Clock, DisplayClock, ExplorerOrderProvider, Log};
use pr_core::diagnostics::diag_options::DiagOptions;
use pr_core::file_actions::journal::OperationJournal;
use pr_core::session::writer::SessionWriter;
use pr_core::settings::store::SettingsStore;
use pr_platform_win::host::main_window::core::SharedViewModel;
use pr_platform_win::host::message_loop::MessageLoop;
use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;
use pr_platform_win::shell_viewing::presentation_sink::ShellPresentationHub;

use super::composition_root::ShellServices;
use super::secondary_dialogs::DialogServices;

/// What the graph builder gets from the host (the C# `host` and `hub` arguments of `ShellCompositionRoot.Build`, plus the services of W8-17c it needs).
pub struct GraphInputs {
    /// The host services of W8-17c (dialog service, folder picker, clipboard, zoom prompt, presentation sink factory, secondary window port). The graph's view model is built over them.
    pub services: Rc<ShellServices>,
    /// The UI dispatcher (the view model's wake function is `host::main_window::lifecycle::view_model_wake(dispatcher.clone())`).
    pub dispatcher: Win32UiDispatcher,
    /// The shell message loop.
    pub message_loop: Rc<MessageLoop>,
    /// C# `ShellPresentationHub` (the one the main window and the sink factory share).
    pub hub: Rc<ShellPresentationHub>,
    /// The process log (`AppLog` facade, see [`global_log`]).
    pub log: Arc<dyn Log>,
    /// The start-up mark sink.
    pub mark: MarkFn,
}

impl std::fmt::Debug for GraphInputs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphInputs").finish_non_exhaustive()
    }
}

/// One field per service the start-up resolves. Built once on the UI thread; dropped by the shutdown (C# `ServiceProvider.Dispose`, APP-01: everything the container created is released,
/// the session writer waits up to 2 s for its flush).
pub struct StartupGraph {
    /// `GetRequiredService<ILog>()`.
    pub log: Arc<dyn Log>,
    /// `GetRequiredService<IAppPaths>().ConfigFile`: where `shell-errors.log` goes (next to it).
    pub config_file: PathBuf,
    /// `GetRequiredService<IAppPaths>().WindowPlacementFile`.
    pub window_placement_file: PathBuf,
    /// `GetRequiredService<IExplorerOrderProvider>()`.
    pub explorer_order: Arc<dyn ExplorerOrderProvider>,
    /// `GetRequiredService<SettingsStore>()`.
    pub settings_store: Arc<SettingsStore>,
    /// `GetRequiredService<LocalizationService>()`.
    pub localization: SharedLocalization,
    /// `GetRequiredService<PreviewImageService>()` (as the prewarm seam).
    pub previews: Arc<dyn PrewarmPreviews>,
    /// `GetRequiredService<ViewportSizeSource>()`.
    pub size_source: Arc<ViewportSizeSource>,
    /// `GetRequiredService<IDisplayClock>()`.
    pub display_clock: Arc<dyn DisplayClock>,
    /// `GetRequiredService<MainViewModel>()`, shared with the window and its features.
    pub view_model: ViewModelFactory,
    /// `GetRequiredService<OperationJournal>()`.
    pub journal: Arc<OperationJournal>,
    /// `GetRequiredService<IClock>()`.
    pub clock: Arc<dyn Clock>,
    /// `GetRequiredService<SessionWriter>()` (review ST-SHELL M5): kept so the shutdown can `dispose()` it explicitly (flush, bounded 2 s) instead of relying on the last `Arc` dropping before
    /// `process::exit`; background threads (early decode, preload) may still hold the registry. `None` only if the writer could not be built (the view model reports that itself).
    pub session_writer: Option<Arc<SessionWriter>>,
    /// The services the secondary windows need, for `ShellServices::bind_dialog_services` (D-W8B6-11, D-W8B6-15 c).
    pub dialog_services: Rc<DialogServices>,
}

impl std::fmt::Debug for StartupGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StartupGraph").finish_non_exhaustive()
    }
}

/// C# `ShellCompositionRoot.Build(host, hub, overrides)` for the graph half: consumes the inputs and returns the graph. The error text becomes `StartupError::Graph`.
pub type GraphBuilder = Box<dyn FnOnce(GraphInputs) -> Result<StartupGraph, Box<dyn Error>>>;

/// Resolves the view model after loaded settings have been applied (D-W8B6-160).
pub type ViewModelFactory = Box<dyn FnOnce() -> Result<SharedViewModel, super::error::StartupError>>;

pub use crate::composition::graph::{GraphOverrides, production_graph_builder_with};

/// The production builder: W8-09b's `SharedServiceRegistration` + `MainViewModelCompositionRoot` over the host services.
/// Tests of 17d and 17e can pass a fake builder instead.
pub fn production_graph_builder() -> GraphBuilder {
    production_graph_builder_with(GraphOverrides::default())
}

/// `Log` over the process-wide `pr_app::misc::app_log::AppLog` facade (`AppLog::global()`), so the dispatcher, the instance scope and the services log where the C# `FileLog.Default` /
/// `AppLog` did. Zero-sized; every method forwards.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalLog;

impl Log for GlobalLog {
    /// `AppLog::global().enabled()`.
    fn enabled(&self) -> bool {
        pr_app::misc::app_log::AppLog::global().enabled()
    }

    /// `AppLog::global().info(message)`.
    fn info(&self, message: &str) {
        pr_app::misc::app_log::AppLog::global().info(message)
    }

    /// `AppLog::global().warn(message)`.
    fn warn(&self, message: &str) {
        pr_app::misc::app_log::AppLog::global().warn(message)
    }

    /// `AppLog::global().error(message, error)`.
    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        pr_app::misc::app_log::AppLog::global().error(message, error)
    }
}

/// `Arc::new(GlobalLog)`.
pub fn global_log() -> Arc<dyn Log> {
    Arc::new(GlobalLog)
}

/// The process-wide DIAG snapshot (`DiagOptions::current()`), the value of C# `DiagOptions.*`.
pub fn diag_options() -> Arc<DiagOptions> {
    DiagOptions::current()
}
