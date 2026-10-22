//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 218-251 (`FailStartup`), 651-665 (`CloseSecondaryWindows`) and 667-684 (`Cleanup`) (baseline `d92d6aae`): the real objects
//! behind `pr_app::startup::shutdown::{ShutdownSteps, FailureSteps}`. Unit W8-17e. Contract: `contracts/W8-17e.md`. The ORDER is `pr_app`'s; this file only says what each step touches.
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::error::Error;
use std::rc::Rc;

use pr_app::startup::shutdown::{FailureSteps, ShutdownSteps};

use super::state::StartupState;

/// The steps over a state. Created for one shutdown / one failure; borrows nothing across a step (each step takes its object out of the state first).
pub struct StateSteps {
    state: Rc<StartupState>,
}

impl std::fmt::Debug for StateSteps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateSteps").finish_non_exhaustive()
    }
}

impl StateSteps {
    /// Steps over `state`.
    pub fn new(state: Rc<StartupState>) -> Self {
        Self { state }
    }
}

impl ShutdownSteps for StateSteps {
    /// `instance.borrow().as_ref().map(|scope| scope.stop_listening())`.
    fn stop_listening(&mut self) {
        todo!()
    }

    /// `coalescer.borrow_mut().take()` then `dispose()` (after the borrow ended).
    fn dispose_coalescer(&mut self) {
        todo!()
    }

    /// `state.close_secondary_windows()`.
    fn close_secondary_windows(&mut self) {
        todo!()
    }

    /// Takes the window out of the slot (`main.borrow_mut().take()`), then `window.dispose()` (idempotent; destroys the HWND and releases the render surface while the services are alive) and drops the `Rc`.
    fn dispose_main_window(&mut self) {
        todo!()
    }

    /// `cloak.borrow_mut().take()` (the `Drop` of `StartupCloak` is `Dispose`).
    fn dispose_cloak(&mut self) {
        todo!()
    }

    /// `frame_clock.borrow_mut().take()` and `message_loop.remove_frame_clock(..)` (the clock must not be polled after it is gone).
    fn dispose_frame_clock(&mut self) {
        todo!()
    }

    /// `graph.borrow_mut().take()` and `services.borrow_mut().take()`, dropped in that order (the graph first: the dialogs of `ShellServices` are what the graph's view model called).
    fn dispose_services(&mut self) {
        todo!()
    }

    /// `AppLog::global().shutdown()`.
    fn shutdown_log(&mut self) {
        todo!()
    }

    /// `instance.borrow_mut().take()` then `dispose()` (releases the mutex).
    fn dispose_instance_scope(&mut self) {
        todo!()
    }

    /// `perf.borrow_mut().take()` (the `Drop` of the listener flushes and stops its writer).
    fn dispose_perf_listener(&mut self) {
        todo!()
    }
}

impl FailureSteps for StateSteps {
    /// `AppLog::global().error_forced("Startup failed", error)`.
    fn log_failure(&mut self, error: &dyn Error) {
        todo!()
    }

    /// With `services` built: `services.dialog_service.show_error(app_title, Localizer::current().format("app.startupFailed", [message=..]))`. Without them (the services are what failed): a stand-alone
    /// `Win32DialogService::new(Box::new(|| 0), FolderPicker::new(Rc::new(UnwiredShellFolderDialog), Box::new(|| 0)), Rc::new(LazySecondaryPort::from_value(None)))` (owner 0, no secondary
    /// port): it needs no service graph. The text uses the built-in English catalog when `Localizer::current()` was never set.
    fn show_failure_box(&mut self, message: &str) {
        todo!()
    }

    /// `state.forced_exit.set(Some(code))`.
    fn force_exit_code(&mut self, code: i32) {
        todo!()
    }

    /// `state.close_secondary_windows()`.
    fn close_secondary_windows(&mut self) {
        todo!()
    }

    /// As [`ShutdownSteps::dispose_main_window`] (the C# `_main?.Dispose(); _main = null;`).
    fn dispose_main_window(&mut self) {
        todo!()
    }

    /// `MessageLoop::quit(code)`.
    fn quit(&mut self, code: i32) {
        todo!()
    }
}

impl StartupState {
    /// C# `CloseSecondaryWindows()`: `services.secondary_port_host.dispose()` on the UI thread, BEFORE the owner window is destroyed (D-W8B5-11 b; idempotent, "not loaded when never needed"). A panic is
    /// caught and logged `AppLog::error_forced("Could not close the secondary windows", ..)` (the cleanup must not block the exit). No services: nothing.
    pub(super) fn close_secondary_windows(self: &Rc<Self>) {
        todo!()
    }
}
