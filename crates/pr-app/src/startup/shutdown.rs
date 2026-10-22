//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 218-251 (`FailStartup`), 642-649 (`CloseSecondaryWindowsUnlessCancelled`), 651-665 (`CloseSecondaryWindows`) and
//! 667-684 (`Cleanup`) (baseline `d92d6aae`): the ORDER of the shutdown and of the start-up failure path (W-11, P-15). Unit W8-17e. Contract: `contracts/W8-17e.md`.
//!
//! Disposal order is behaviour (C# `Shutdown_ReleasesTheMainWindowBeforeTheServiceContainerIsDisposed`): the steps are a trait so the order is tested with a recording fake and the
//! real objects are touched only by `pr_shell::startup::shutdown`. Every step is idempotent: `Cleanup` runs from `Dispose` AND at the end of `RunLoop`, and the start-up failure path
//! calls part of it first.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::error::Error;

/// The disposal steps of C# `Cleanup()`, in call order. Each is a no-op when its object is already gone.
pub trait ShutdownSteps {
    /// `_instanceScope?.StopListening()`: no more forwarded requests (the pipe is closed first).
    fn stop_listening(&mut self);
    /// `Interlocked.Exchange(ref _coalescer, null)?.Dispose()`.
    fn dispose_coalescer(&mut self);
    /// `CloseSecondaryWindows()`: secondary windows close BEFORE the owner window and before the container (they are owned by the shell HWND).
    fn close_secondary_windows(&mut self);
    /// `Interlocked.Exchange(ref _main, null)?.Dispose()`: the main window and its render surface.
    fn dispose_main_window(&mut self);
    /// `_cloak?.Dispose()`.
    fn dispose_cloak(&mut self);
    /// `_frameClock?.Dispose()`.
    fn dispose_frame_clock(&mut self);
    /// `Interlocked.Exchange(ref _services, null)?.Dispose()`: the service graph (the session writer waits for its flush up to 2 s, the caches).
    fn dispose_services(&mut self);
    /// `AppLog.Shutdown()`: after the pipe and the coalescer so their shutdown warnings still reach the log.
    fn shutdown_log(&mut self);
    /// `Interlocked.Exchange(ref _instanceScope, null)?.Dispose()`: the instance lock is released next to last.
    fn dispose_instance_scope(&mut self);
    /// `Interlocked.Exchange(ref _perf, null)?.Dispose()`.
    fn dispose_perf_listener(&mut self);
}

/// C# `Cleanup()`: calls the ten steps in the order of the trait above.
pub fn run_shutdown(steps: &mut dyn ShutdownSteps) {
    todo!()
}

/// What `FailStartup(ex)` does besides the shutdown (C# lines 218-251), in call order.
pub trait FailureSteps {
    /// `AppLog.ErrorForced("Startup failed", ex)`: written even when logging is off.
    fn log_failure(&mut self, error: &dyn Error);
    /// `dialogs.ShowError(Tr.AppTitle, Tr.AppStartupFailed(ex.Message))`: the registered dialog service, or (when the services are the thing that failed) a stand-alone box that needs no service graph.
    fn show_failure_box(&mut self, message: &str);
    /// `_forcedExit = ExitStartupFailed`: the error code must win over the `Quit(0)` that closing the window raises.
    fn force_exit_code(&mut self, code: i32);
    /// `CloseSecondaryWindows()`.
    fn close_secondary_windows(&mut self);
    /// `_main?.Dispose(); _main = null`.
    fn dispose_main_window(&mut self);
    /// `MessageLoop.Quit(ExitStartupFailed)`.
    fn quit(&mut self, code: i32);
}

/// C# `FailStartup(ex)`: `log_failure` and `show_failure_box` each run under `catch_unwind(AssertUnwindSafe(..))` and a panic in either is IGNORED (C# `catch { }`: "a log that cannot be
/// written, a dialog that cannot be shown must not block the exit": the last-resort handler is an R0003 exception to "the UI thread does not catch panics"); then
/// `force_exit_code(EXIT_STARTUP_FAILED)`, `close_secondary_windows`, `dispose_main_window`, `quit(EXIT_STARTUP_FAILED)`. `message` is `error.to_string()`.
pub fn run_fail_startup(steps: &mut dyn FailureSteps, error: &dyn Error) {
    todo!()
}

/// C# `CloseSecondaryWindowsUnlessCancelled(e, close)`: `close()` only when the closing was NOT cancelled (a running file action keeps the window, and with it the secondary windows).
pub fn close_secondary_windows_unless_cancelled(cancelled: bool, close: &mut dyn FnMut()) {
    todo!()
}
