//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartupContext.cs` (8 lines, baseline `d92d6aae`). Unit W8-17a. Contract: `contracts/W8-17a.md`.
//!
//! C-16 shell part: the inputs of the start-up, handed to the feature packages and to diagnostics. The C# `Task<T>` members become [`StartupTask`] (a named
//! worker thread whose result is taken once, D-W8-2).
use std::thread::JoinHandle;
use std::time::Instant;

use pr_core::settings::app_settings::AppSettings;

use crate::windowing::contracts::PrefetchHandle;

/// A value computed on a worker thread (C# `Task<T>` started with `Task.Run`).
#[derive(Debug)]
pub struct StartupTask<T> {
    handle: Option<JoinHandle<T>>,
}

impl<T: Send + 'static> StartupTask<T> {
    /// Starts `work` on a thread named `name`. When the thread cannot be started the task holds nothing and [`join`](Self::join) answers `None`.
    pub fn spawn(name: &str, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let handle = std::thread::Builder::new().name(name.to_owned()).spawn(work).ok();
        Self { handle }
    }

    /// Waits for the result. `None` when the thread could not be started or panicked.
    pub fn join(self) -> Option<T> {
        self.handle?.join().ok()
    }
}

/// C# `ShellStartupContext` record. Not `Clone`: the tasks are consumed once.
#[derive(Debug)]
pub struct ShellStartupContext {
    /// `ShellLaunch.arguments`.
    pub arguments: Vec<String>,
    /// `ShellLaunch::initial_path()`.
    pub initial_path: Option<String>,
    /// `ShellLaunch.launch_folder`.
    pub launch_folder: Option<String>,
    /// The settings being loaded on a worker.
    pub settings: StartupTask<AppSettings>,
    /// The window placement being read on a worker (`WindowPlacementStore::prefetch`).
    pub placement: PrefetchHandle,
    /// C# `ProcessStartTimestamp` (`Stopwatch.GetTimestamp()` at the entry of `Main`).
    pub process_start: Instant,
}
