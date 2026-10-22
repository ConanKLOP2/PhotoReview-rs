//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 20-82 (`ShellStartupOptions`, the env-driven defaults) and the constants of lines 102-114
//! (baseline `d92d6aae`). Unit W8-17d. Contract: `contracts/W8-17d.md`.
//!
//! The window-free half of the options: the exit codes, the time limits and the three values that the C# record reads from the environment
//! (`PHOTOREVIEW_SHELL_INSTANCE_PREFIX`, `PHOTOREVIEW_SHELL_WARP`, `PHOTOREVIEW_SHELL_NOWARM`; renamed `PHOTOREVIEW_RS_*`, R0002). The full options record, with the
//! seams that name `pr-platform-win` types, is `pr_shell::startup::options::ShellStartupOptions`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::time::Duration;

/// C# `ShellStartup.ExitStartupFailed`: the start-up failed (the error box was shown).
pub const EXIT_STARTUP_FAILED: i32 = 1;
/// C# `ShellStartup.ExitSmokeTimeout`: smoke mode saw no first frame within the smoke time limit.
pub const EXIT_SMOKE_TIMEOUT: i32 = 7;
/// C# `ShellStartup.ExplorerPrefetchTimeout` (3 s).
pub const EXPLORER_PREFETCH_TIMEOUT: Duration = Duration::from_secs(3);
/// C# `ShellStartupOptions.SmokeTimeout` default (30 s).
pub const DEFAULT_SMOKE_TIMEOUT: Duration = Duration::from_secs(30);
/// C# `ShellStartup.MaxLoopFaults`: a UI loop that faults this many times ends the process with [`EXIT_STARTUP_FAILED`].
pub const MAX_LOOP_FAULTS: u32 = 50;

/// C# `PHOTOREVIEW_SHELL_INSTANCE_PREFIX` (R0002 spelling): replaces the mutex/pipe prefix (diagnostics, process tests).
pub const INSTANCE_PREFIX_VAR: &str = "PHOTOREVIEW_RS_INSTANCE_PREFIX";
/// C# `PHOTOREVIEW_SHELL_WARP`: `1` selects WARP instead of the GPU.
pub const PREFER_WARP_VAR: &str = "PHOTOREVIEW_RS_WARP";
/// C# `PHOTOREVIEW_SHELL_NOWARM`: `1` turns the graphics warm-up off.
pub const NO_WARM_VAR: &str = "PHOTOREVIEW_RS_NOWARM";

/// Reads environment variables (the test seam of the three defaults).
pub trait EnvSource {
    /// The value of `name`, `None` when unset.
    fn var(&self, name: &str) -> Option<String>;
}

/// The process environment: `std::env::var_os`, lossy UTF-8.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    /// `std::env::var_os(name)` converted with `to_string_lossy`.
    fn var(&self, name: &str) -> Option<String> {
        todo!()
    }
}

/// The three environment-driven members of C# `ShellStartupOptions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvOptions {
    /// C# `InstanceNamePrefix`: the variable when it is set and NOT EMPTY (`Length > 0`: a whitespace-only value is kept as it is and later refused by
    /// `InstanceScope`), else [`crate::startup::launch::INSTANCE_NAME_PREFIX`].
    pub instance_name_prefix: String,
    /// C# `PreferWarp`: the variable is exactly `"1"`.
    pub prefer_warp: bool,
    /// C# `WarmGraphics`: the NOWARM variable is NOT exactly `"1"`.
    pub warm_graphics: bool,
}

impl EnvOptions {
    /// Applies the rules above to `env`.
    pub fn from_env(env: &dyn EnvSource) -> Self {
        todo!()
    }

    /// `from_env(&ProcessEnv)`.
    pub fn from_process() -> Self {
        todo!()
    }
}

impl Default for EnvOptions {
    /// C# property initialisers: the process environment ([`EnvOptions::from_process`]).
    fn default() -> Self {
        Self::from_process()
    }
}
