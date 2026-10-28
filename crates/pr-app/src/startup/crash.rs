//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 175-199 (the UI loop fault counter), 318-327 (process-wide exception hooks) and 630-640 (`LogUnhandledForced`)
//! (baseline `d92d6aae`): crash handling, P-15. Unit W8-17e. Contract: `contracts/W8-17e.md`.
//!
//! C# marks a dispatcher exception handled, writes it (and flushes) EVEN WHEN LOGGING IS OFF, and keeps running; a loop that faults [`crate::startup::options::MAX_LOOP_FAULTS`] times ends with
//! [`crate::startup::options::EXIT_STARTUP_FAILED`]. Rust: a panic that a window procedure or a posted job raised is captured by `pending_panic` and resumed out of `MessageLoop::run`
//! (D-W8B3H-4); the run loop of `pr_shell::startup::run` catches it around `run` (the ONE `catch_unwind` of the UI thread, D-W8B6-25) and asks [`LoopFaultCounter`] whether to run again.

use std::any::Any;
use std::error::Error;
use std::fmt;

/// A panic payload or a non-`Error` object as an `Error` for `AppLog::error_forced` (C# `new InvalidOperationException(text)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnhandledError(pub String);

impl fmt::Display for UnhandledError {
    /// The text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for UnhandledError {}

/// The text of a panic payload: a `&'static str` or a `String` payload as it is; any other payload `"unknown exception object"` (C# `exceptionObject?.ToString() ?? "unknown exception object"`).
/// `pr_core::abstractions::panic_message` is the shared helper; this function only adds the fixed fallback text.
pub fn describe_payload(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&'static str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown exception object".to_owned()
    }
}

/// What to do after a faulted `MessageLoop::run`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopVerdict {
    /// Run the loop again.
    Continue,
    /// Too many faults: leave with `EXIT_STARTUP_FAILED` (C# `_exitCode = ExitStartupFailed; break`).
    GiveUp,
}

/// C# `faults` + `if (++faults >= MaxLoopFaults)`.
#[derive(Debug, Default)]
pub struct LoopFaultCounter {
    faults: u32,
}

impl LoopFaultCounter {
    /// Zero faults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one fault: `faults += 1` (saturating); `GiveUp` when `faults >= max_faults`, else `Continue`.
    pub fn record(&mut self, max_faults: u32) -> LoopVerdict {
        self.faults = self.faults.saturating_add(1);
        if self.faults >= max_faults {
            LoopVerdict::GiveUp
        } else {
            LoopVerdict::Continue
        }
    }

    /// The number of faults so far.
    pub fn faults(&self) -> u32 {
        self.faults
    }
}

/// `counter.record(MAX_LOOP_FAULTS)`.
pub fn record_default(counter: &mut LoopFaultCounter) -> LoopVerdict {
    counter.record(crate::startup::options::MAX_LOOP_FAULTS)
}
