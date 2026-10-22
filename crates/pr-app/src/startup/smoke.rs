//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 463-502 (`ArmSmokeExit`, baseline `d92d6aae`) and `ShellLaunch.SmokeArgument`: the `--smoke` run. Unit W8-17e.
//! Contract: `contracts/W8-17e.md`.
//!
//! `--smoke` runs the REAL path but takes no instance lock, reads and writes no placement, reconciles no journal, takes no focus, and closes right after the first frame that shows the
//! image (or the first frame when there is nothing to open); a run that sees no such frame within the smoke time limit closes with [`crate::startup::options::EXIT_SMOKE_TIMEOUT`].
//! This module is the window-free half: the one-shot latch and the choice of the trigger. The window events and the timer are `pr_shell::startup::smoke`.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::sync::atomic::AtomicBool;

/// Which window event ends a smoke run (C# lines 487-498).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmokeTrigger {
    /// No path to open: the first presented frame (`FirstPresented`; also immediately when `FirstPresentDone` is already true).
    FirstPresented,
    /// A path is opened: the first frame that holds the image (`FirstImagePresented`).
    FirstImagePresented,
}

/// C# `if (_launch.InitialPath is null) FirstPresented... else FirstImagePresentedEvent...`.
pub fn smoke_trigger(initial_path: Option<&str>) -> SmokeTrigger {
    todo!()
}

/// C# `CloseOnce(exitCode)` guard (`Interlocked.Exchange(ref closing, 1) != 0` -> return): the FIRST caller wins, every later one is ignored (the trigger and the timeout race
/// on different threads).
#[derive(Debug, Default)]
pub struct SmokeLatch {
    closing: AtomicBool,
}

impl SmokeLatch {
    /// A latch that has not fired.
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` for the first call only (atomic swap, `SeqCst`).
    pub fn try_close(&self) -> bool {
        todo!()
    }
}
