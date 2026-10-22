//! Wave 8 batch B6 native start-up parts (lead-owned index, contract writer of 17a..17c): DPI awareness (W8-17a), the placement before the first show and the DWM
//! start-up cloak (W8-17b), the start-up warm-ups and the user-locale services of the dialogs (W8-17c). Port of `PhotoReview.Shell.Win32/Startup/*` (baseline `d92d6aae`).
//!
//! The window-free logic (`ShellLaunch`, `ForwardedOpenHandler`, `ShellErrorLog`) is `pr_app::startup`; the composition root and the production dialog adapter are
//! `pr_shell::startup` (D-W8B6-1). W8-17d adds two small native modules here (`monitor_probe`, `local_time`: no window involved); the run loop of 17d/17e lives in `pr_shell::startup`. Re-read this file before editing it.

pub mod cloak;
pub mod dpi;
pub mod locale;
pub mod placement;
pub mod warmup;

// W8-17d (`contracts/W8-17d.md`): the real `MonitorProbe` of the early-decode prediction and the local time of the perf CSV name.
pub mod local_time;
pub mod monitor_probe;

// W8-20a (`contracts/W8-20a.md`): the STA apartment of the process's main thread (C# `[STAThread]`), entered by `pr_shell::entry` before `ShellStartup::run`.
pub mod apartment;
