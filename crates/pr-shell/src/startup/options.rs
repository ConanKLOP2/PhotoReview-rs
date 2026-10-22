//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 20-82 (`ShellStartupOptions`, baseline `d92d6aae`). Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! Start-up options: the defaults are the real run, a test replaces one seam at a time and never touches the user's data. The window-free members (`EnvOptions`, constants) are `pr_app::startup::options`; this record adds the seams that name `pr-platform-win` types. Every `Option` seam is
//! `None` in production. The `Action<IServiceCollection>? ServiceOverrides` of the C# is [`ShellStartupOptions::graph_builder`] + [`ShellStartupOptions::services`] (there is no container).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::startup::early_decode::MarkFn;
use pr_app::startup::options::EnvSource;
use pr_core::abstractions::{Clock, Log};
use pr_core::file_actions::journal::OperationJournal;
use pr_core::file_actions::journal_line::JournalEntry;
use pr_platform_win::host::main_window::MainWindow;
use pr_platform_win::host::main_window::windowing::ShellWindowingSeams;
use pr_platform_win::render::keep_alive::{OpenKeepAliveWork, TickClock};
use pr_platform_win::shell_startup::cloak::SetCloakedFn;
use pr_platform_win::shell_viewing::keep_warm::KeepWarmOptions;

use super::composition_root::ShellServicesOptions;
use super::graph::GraphBuilder;

/// C# `Func<IServiceProvider, Task<IReadOnlyList<JournalEntry>>>? JournalRecovery`: replaces the journal reconcile (tests record the order). Runs on a WORKER thread (hence `Send`), with the
/// pieces the real reconcile uses: the journal, the clock and the log.
pub type JournalRecoveryFn =
    Box<dyn FnOnce(Arc<OperationJournal>, Arc<dyn Clock>, Arc<dyn Log>) -> Vec<JournalEntry> + Send>;

/// C# `Action<ShellMainWindow>? OnMainWindowReady`: called on the UI thread when the main window is built, before the first image is opened.
pub type MainWindowReadyFn = Box<dyn FnOnce(&Rc<MainWindow>)>;

/// C# `ShellStartupOptions`.
pub struct ShellStartupOptions {
    /// C# `InstanceNamePrefix` (NE-6): env `PHOTOREVIEW_RS_INSTANCE_PREFIX` or `pr_core::identity::INSTANCE_PREFIX`.
    pub instance_name_prefix: String,
    /// C# `SetCloaked`: the DWM cloak seam; `None` = `try_set_cloaked`.
    pub set_cloaked: Option<SetCloakedFn>,
    /// C# `Windowing` (WP-25): the F11 / monitor / DWM seams of the main window; `None` = real Win32.
    pub windowing: Option<ShellWindowingSeams>,
    /// C# `KeepWarm` (WP21-STALL): keep-warm options; `None` = environment (`PHOTOREVIEW_RS_GPU_KEEPWARM*`), enabled by default.
    pub keep_warm: Option<KeepWarmOptions>,
    /// C# `FrameLogPath`: the frame log file (tests); `None` = environment `PHOTOREVIEW_RS_FRAME_LOG`.
    pub frame_log_path: Option<PathBuf>,
    /// C# `KeepWarmWork`: the keep-warm work (tests: no real GPU); `None` = a private offscreen device on the real GPU.
    pub keep_warm_work: Option<OpenKeepAliveWork>,
    /// C# `KeepWarmTimestamp`: the clock of the keep-warm policy; `None` = the monotonic clock.
    pub keep_warm_timestamp: Option<TickClock>,
    /// C# `ServiceOverrides`, first half: builds the service graph; `None` = `graph::production_graph_builder()` (W8-09b).
    pub graph_builder: Option<GraphBuilder>,
    /// C# `ServiceOverrides`, second half: the replaceable parts of the host services (dialogs, pickers) of W8-17c.
    pub services: ShellServicesOptions,
    /// C# `Mark`: the start-up mark sink (C-18); `None` = `pr_core::diagnostics::perf::startup_mark`. `Send + Sync`: workers mark too.
    pub mark: Option<MarkFn>,
    /// C# `PreferWarp`: WARP instead of the GPU (env `PHOTOREVIEW_RS_WARP=1`, RDP, tests).
    pub prefer_warp: bool,
    /// C# `Activate`: take the focus when the window is shown (smoke: never). Default `true`.
    pub activate: bool,
    /// C# `WarmGraphics`: warm the graphics on a worker from the start of the process (off with `PHOTOREVIEW_RS_NOWARM=1`).
    pub warm_graphics: bool,
    /// C# `InstallProcessHooks`: the process-wide panic hook (AppDomain/Task handlers). In-process tests turn it off. Default `true`.
    pub install_process_hooks: bool,
    /// C# `StartPerfListener`: the CSV perf listener of `PHOTOREVIEW_RS_PERF_TRACE`. Default `true`.
    pub start_perf_listener: bool,
    /// C# `JournalRecovery`: replaces the journal reconcile; `None` = `pr_core::file_actions::startup_recovery::spawn`.
    pub journal_recovery: Option<JournalRecoveryFn>,
    /// C# `OnMainWindowReady`.
    pub on_main_window_ready: Option<MainWindowReadyFn>,
    /// C# `SmokeTimeout`: the longest a `--smoke` run waits for its frame. Default `DEFAULT_SMOKE_TIMEOUT` (30 s).
    pub smoke_timeout: Duration,
}

impl fmt::Debug for ShellStartupOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellStartupOptions")
            .field("instance_name_prefix", &self.instance_name_prefix)
            .field("prefer_warp", &self.prefer_warp)
            .field("activate", &self.activate)
            .field("warm_graphics", &self.warm_graphics)
            .field("smoke_timeout", &self.smoke_timeout)
            .finish_non_exhaustive()
    }
}

impl ShellStartupOptions {
    /// The C# property initialisers with the environment read through `env` (`EnvOptions::from_env`): the three env-driven members from there, `activate`, `install_process_hooks`,
    /// `start_perf_listener` = `true`, every seam `None`, `services` = `ShellServicesOptions::default()`, `smoke_timeout` = `DEFAULT_SMOKE_TIMEOUT`.
    pub fn from_env(env: &dyn EnvSource) -> Self {
        todo!()
    }
}

impl Default for ShellStartupOptions {
    /// `from_env(&ProcessEnv)`: the real run.
    fn default() -> Self {
        todo!()
    }
}
