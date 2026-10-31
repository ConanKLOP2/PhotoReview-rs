//! Port of `PhotoReview.Shell.Win32/Startup/ShellStartup.cs` lines 218-251 (`FailStartup`), 651-665 (`CloseSecondaryWindows`) and 667-684 (`Cleanup`) (baseline `d92d6aae`): the real objects
//! behind `pr_app::startup::shutdown::{ShutdownSteps, FailureSteps}`. Unit W8-17e. Contract: `contracts/W8-17e.md`. The ORDER is `pr_app`'s; this file only says what each step touches.

use std::error::Error;
use std::rc::Rc;

use pr_app::misc::app_log::AppLog;
use pr_app::startup::crash::UnhandledError;
use pr_app::startup::shutdown::{FailureSteps, ShutdownSteps};
use pr_core::abstractions::DialogService;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;
use pr_platform_win::dialogs::dialog_service::{LazySecondaryPort, Win32DialogService};
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, UnwiredShellFolderDialog};
use pr_platform_win::host::frame_clock::HostFrameClock;
use pr_platform_win::host::message_loop::MessageLoop;

use super::graph::StartupGraph;
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

/// Disposes the session writer of `graph` explicitly (C# `ServiceProvider.Dispose()` disposes it: flush, bounded 2 s; idempotent), THEN drops the graph. The explicit call matters because
/// other threads (early decode, preload) can still hold an `Arc` of the services, so the last-`Arc` `Drop` may never run before `process::exit` (review ST-SHELL M5).
pub fn dispose_graph(graph: StartupGraph) {
    if let Some(writer) = &graph.session_writer {
        writer.dispose();
    }
    drop(graph);
}

impl ShutdownSteps for StateSteps {
    /// `instance.borrow().as_ref().map(|scope| scope.stop_listening())`.
    fn stop_listening(&mut self) {
        let instance = self.state.instance.borrow().clone();
        if let Some(scope) = instance {
            scope.stop_listening();
        }
    }

    /// `coalescer.borrow_mut().take()` then `dispose()` (after the borrow ended).
    fn dispose_coalescer(&mut self) {
        let coalescer = self.state.coalescer.borrow_mut().take();
        if let Some(coalescer) = coalescer {
            coalescer.dispose();
        }
    }

    /// `state.close_secondary_windows()`.
    fn close_secondary_windows(&mut self) {
        self.state.close_secondary_windows();
    }

    /// Takes the window out of the slot (`main.borrow_mut().take()`), then `window.dispose()` (idempotent; destroys the HWND and releases the render surface while the services are alive) and drops the `Rc`.
    fn dispose_main_window(&mut self) {
        dispose_main(&self.state);
    }

    /// `cloak.borrow_mut().take()` (the `Drop` of `StartupCloak` is `Dispose`).
    fn dispose_cloak(&mut self) {
        let cloak = self.state.cloak.borrow_mut().take();
        drop(cloak);
    }

    /// `frame_clock.borrow_mut().take()` and `message_loop.remove_frame_clock(..)` (the clock must not be polled after it is gone).
    fn dispose_frame_clock(&mut self) {
        let clock = self.state.frame_clock.borrow_mut().take();
        if let Some(clock) = clock {
            let host: Rc<dyn HostFrameClock> = clock;
            self.state.message_loop.remove_frame_clock(&host);
        }
    }

    /// `graph.borrow_mut().take()` and `services.borrow_mut().take()`, dropped in that order (the graph first: the dialogs of `ShellServices` are what the graph's view model called).
    fn dispose_services(&mut self) {
        let graph = self.state.graph.borrow_mut().take();
        if let Some(graph) = graph {
            dispose_graph(graph);
        }
        let services = self.state.services.borrow_mut().take();
        drop(services);
    }

    /// `AppLog::global().shutdown()`.
    fn shutdown_log(&mut self) {
        AppLog::global().shutdown();
    }

    /// `instance.borrow_mut().take()` then `dispose()` (releases the mutex).
    fn dispose_instance_scope(&mut self) {
        let scope = self.state.instance.borrow_mut().take();
        if let Some(scope) = scope {
            scope.dispose();
        }
    }

    /// `perf.borrow_mut().take()` (the `Drop` of the listener flushes and stops its writer).
    fn dispose_perf_listener(&mut self) {
        let perf = self.state.perf.borrow_mut().take();
        drop(perf);
    }
}

impl FailureSteps for StateSteps {
    /// `AppLog::global().error_forced("Startup failed", error)`.
    fn log_failure(&mut self, error: &dyn Error) {
        AppLog::global().error_forced("Startup failed", &UnhandledError(error.to_string()));
    }

    /// Same as [`ShutdownSteps::stop_listening`]: closes the pipe before the modal box (no forwarded open is swallowed meanwhile).
    fn stop_listening(&mut self) {
        ShutdownSteps::stop_listening(self);
    }

    /// With `services` built: `services.dialog_service.show_error(app_title, Localizer::current().format("app.startupFailed", [message=..]))`. Without them (the services are what failed): a stand-alone
    /// `Win32DialogService::new(Box::new(|| 0), FolderPicker::new(Rc::new(UnwiredShellFolderDialog), Box::new(|| 0)), Rc::new(LazySecondaryPort::from_value(None)))` (owner 0, no secondary
    /// port): it needs no service graph. The text uses the built-in English catalog when `Localizer::current()` was never set.
    fn show_failure_box(&mut self, message: &str) {
        let localizer = Localizer::current();
        let title = localizer.get("app.title");
        let text = localizer.format("app.startupFailed", &[LocArg::new("message", message)]);
        let services = self.state.services.borrow().clone();
        match services {
            Some(services) => services.dialog_service.show_error(&title, &text),
            None => {
                let picker = FolderPicker::new(Rc::new(UnwiredShellFolderDialog), Box::new(|| 0));
                let dialogs =
                    Win32DialogService::new(Box::new(|| 0), picker, Rc::new(LazySecondaryPort::from_value(None)));
                dialogs.show_error(&title, &text);
            }
        }
    }

    /// `state.forced_exit.set(Some(code))`.
    fn force_exit_code(&mut self, code: i32) {
        self.state.forced_exit.set(Some(code));
    }

    /// `state.close_secondary_windows()`.
    fn close_secondary_windows(&mut self) {
        self.state.close_secondary_windows();
    }

    /// As [`ShutdownSteps::dispose_main_window`] (the C# `_main?.Dispose(); _main = null;`).
    fn dispose_main_window(&mut self) {
        dispose_main(&self.state);
    }

    /// `MessageLoop::quit(code)`.
    fn quit(&mut self, code: i32) {
        MessageLoop::quit(code);
    }
}

impl StartupState {
    /// C# `CloseSecondaryWindows()`: `services.secondary_port_host.dispose()` on the UI thread, BEFORE the owner window is destroyed (D-W8B5-11 b; idempotent, "not loaded when never needed"). A panic is
    /// caught and logged `AppLog::error_forced("Could not close the secondary windows", ..)` (the cleanup must not block the exit). No services: nothing.
    pub(super) fn close_secondary_windows(self: &Rc<Self>) {
        let host = self
            .services
            .borrow()
            .as_ref()
            .map(|services| services.secondary_port_host.clone());
        let Some(host) = host else {
            return;
        };
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.dispose())) {
            let text = pr_app::startup::crash::describe_payload(payload.as_ref());
            AppLog::global().error_forced("Could not close the secondary windows", &UnhandledError(text));
        }
    }
}

/// Takes the window out of the slot, then disposes it (idempotent) after the borrow ended; the `Rc` is dropped at the end.
fn dispose_main(state: &StartupState) {
    let window = state.main.borrow_mut().take();
    if let Some(window) = window {
        window.dispose();
    }
}
