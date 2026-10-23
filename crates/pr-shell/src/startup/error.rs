//! New (no C# twin): the error of a start-up step. Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! The C# start-up is `try { await StartupCoreAsync(); } catch (Exception ex) when (ex is not OutOfMemoryException) { FailStartup(ex); }` (R2-F-04: a failure used to leave a process with no
//! window that kept the instance mutex). Exceptions become [`StartupError`] values; a panic inside a start-up step is turned into [`StartupError::Panicked`] by [`catch_step`], the
//! last-resort handler that R0003 allows on the UI thread (D-W8B6-21). Every failure ends in `pr_app::startup::shutdown::run_fail_startup`.

use std::fmt;

use pr_app::startup::instance::InstanceStartError;
use pr_app::startup::settings_load::StartupLoadError;
use pr_platform_win::host::error::HostError;
use pr_platform_win::host::main_window::core::MainWindowError;

/// Why a start-up step failed.
#[derive(Debug)]
pub enum StartupError {
    /// The service graph could not be built (`GraphBuilder` returned `Err`): the text of the cause.
    Graph(String),
    /// The settings load failed ([`StartupLoadError`]).
    Settings(StartupLoadError),
    /// The instance lock or the hand-off failed.
    Instance(InstanceStartError),
    /// A host call failed (dispatcher, message loop, window, frame clock).
    Host(HostError),
    /// The main window could not be built or attached (`attach_renderer`: no GPU and no WARP, a surface error).
    MainWindow(MainWindowError),
    /// The overlay could not be attached (the DirectWrite text renderer failed): the text of the cause.
    Overlay(String),
    /// A worker ended without a result (`HopLost`): the text.
    Worker(String),
    /// A start-up step panicked: the panic text (`pr_app::startup::crash::describe_payload`).
    Panicked(String),
}

impl fmt::Display for StartupError {
    /// The text of the cause (the box says `Tr.AppStartupFailed(ex.Message)`, so the text is what `ex.Message` was): `Graph(m)`/`Overlay(m)`/`Worker(m)`/`Panicked(m)` = `m`; the others delegate to their error.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Graph(m) | Self::Overlay(m) | Self::Worker(m) | Self::Panicked(m) => f.write_str(m),
            Self::Settings(e) => e.fmt(f),
            Self::Instance(e) => e.fmt(f),
            Self::Host(e) => e.fmt(f),
            Self::MainWindow(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for StartupError {}

impl From<StartupLoadError> for StartupError {
    fn from(error: StartupLoadError) -> Self {
        Self::Settings(error)
    }
}

impl From<InstanceStartError> for StartupError {
    fn from(error: InstanceStartError) -> Self {
        Self::Instance(error)
    }
}

impl From<HostError> for StartupError {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}

impl From<MainWindowError> for StartupError {
    fn from(error: MainWindowError) -> Self {
        Self::MainWindow(error)
    }
}

/// Runs `step` under `catch_unwind(AssertUnwindSafe(step))`: `Ok(value)` and `Err(e)` pass through; a panic becomes `Err(StartupError::Panicked(describe_payload(&payload)))`. The panic hook
/// already printed the message where the panic started, so nothing is logged here. Used by every UI-thread start-up step and continuation (17d `phase_one`, 17e `run`).
pub fn catch_step<R>(step: impl FnOnce() -> Result<R, StartupError>) -> Result<R, StartupError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(step)).unwrap_or_else(|payload| {
        // Same payload mapping as 17e's crash::describe_payload; independent of that unported module.
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown exception object".to_owned());
        Err(StartupError::Panicked(message))
    })
}
