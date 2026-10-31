//! W8-17e shutdown and failure order (W-11, P-15), pure half. Contract: `contracts/W8-17e.md`.
use pr_app::startup::options::EXIT_STARTUP_FAILED;
use pr_app::startup::shutdown::*;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;

/// Records every step; a step already gone does nothing (idempotence), like the real `take()` steps.
#[derive(Default)]
struct Recorder {
    calls: Vec<String>,
    gone: HashSet<&'static str>,
}

impl Recorder {
    fn step(&mut self, name: &'static str) {
        if self.gone.insert(name) {
            self.calls.push(name.to_owned());
        }
    }
}

impl ShutdownSteps for Recorder {
    fn stop_listening(&mut self) {
        self.step("stop_listening");
    }
    fn dispose_coalescer(&mut self) {
        self.step("dispose_coalescer");
    }
    fn close_secondary_windows(&mut self) {
        self.step("close_secondary_windows");
    }
    fn dispose_main_window(&mut self) {
        self.step("dispose_main_window");
    }
    fn dispose_cloak(&mut self) {
        self.step("dispose_cloak");
    }
    fn dispose_frame_clock(&mut self) {
        self.step("dispose_frame_clock");
    }
    fn dispose_services(&mut self) {
        self.step("dispose_services");
    }
    fn shutdown_log(&mut self) {
        self.step("shutdown_log");
    }
    fn dispose_instance_scope(&mut self) {
        self.step("dispose_instance_scope");
    }
    fn dispose_perf_listener(&mut self) {
        self.step("dispose_perf_listener");
    }
}

#[test]
fn shutdown_runs_the_ten_steps_in_order_twice_without_effect_the_second_time() {
    // new: also covers C# `Shutdown_ReleasesTheMainWindowBeforeTheServiceContainerIsDisposed` (order) and
    // `StartupCore_ExitEvent_DisposesTheApp` (Cleanup twice is safe).
    let mut rec = Recorder::default();
    run_shutdown(&mut rec);
    let expected = [
        "stop_listening",
        "dispose_coalescer",
        "close_secondary_windows",
        "dispose_main_window",
        "dispose_cloak",
        "dispose_frame_clock",
        "dispose_services",
        "shutdown_log",
        "dispose_instance_scope",
        "dispose_perf_listener",
    ];
    assert_eq!(rec.calls, expected);
    run_shutdown(&mut rec);
    assert_eq!(rec.calls, expected, "the second run changes nothing");
}

#[derive(Debug)]
struct Boom;
impl fmt::Display for Boom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("boom message")
    }
}
impl Error for Boom {}

#[derive(Default)]
struct Failing {
    calls: Vec<String>,
    panic_log: bool,
    panic_box: bool,
}

impl FailureSteps for Failing {
    fn log_failure(&mut self, error: &dyn Error) {
        self.calls.push(format!("log:{error}"));
        if self.panic_log {
            panic!("log cannot be written");
        }
    }
    fn stop_listening(&mut self) {
        self.calls.push("stop_listening".to_owned());
    }
    fn show_failure_box(&mut self, message: &str) {
        self.calls.push(format!("box:{message}"));
        if self.panic_box {
            panic!("dialog cannot be shown");
        }
    }
    fn force_exit_code(&mut self, code: i32) {
        self.calls.push(format!("force:{code}"));
    }
    fn close_secondary_windows(&mut self) {
        self.calls.push("close_secondary".to_owned());
    }
    fn dispose_main_window(&mut self) {
        self.calls.push("dispose_main".to_owned());
    }
    fn quit(&mut self, code: i32) {
        self.calls.push(format!("quit:{code}"));
    }
}

#[test]
fn fail_startup_forces_the_exit_code_before_closing_the_window() {
    // new: C# `AppStartup_StartupFault_ReportsTheErrorLogsItAndShutsDownWithExitCodeOne` and
    // `FailStartup_WithServices_ShowsTheErrorThroughTheRegisteredDialogServiceAndDisposesTheApp`.
    let mut steps = Failing::default();
    run_fail_startup(&mut steps, &Boom);
    assert_eq!(
        steps.calls,
        [
            "log:boom message".to_owned(),
            // Review ST-SHELL M3: the pipe is closed before the modal box.
            "stop_listening".to_owned(),
            "box:boom message".to_owned(),
            format!("force:{EXIT_STARTUP_FAILED}"),
            "close_secondary".to_owned(),
            "dispose_main".to_owned(),
            format!("quit:{EXIT_STARTUP_FAILED}"),
        ]
    );
}

#[test]
fn fail_startup_ignores_a_panicking_log_and_a_panicking_box() {
    // new: C# `FailStartup_WhenTheDialogThrows_StillDisposesAndShutsDown`.
    let mut steps = Failing {
        panic_log: true,
        panic_box: true,
        ..Failing::default()
    };
    run_fail_startup(&mut steps, &Boom);
    assert_eq!(steps.calls.len(), 7);
    assert_eq!(steps.calls[3], format!("force:{EXIT_STARTUP_FAILED}"));
    assert_eq!(steps.calls[6], format!("quit:{EXIT_STARTUP_FAILED}"));
}

#[test]
fn close_secondary_windows_unless_cancelled_closes_only_when_not_cancelled() {
    // C#: `CloseSecondaryWindowsUnlessCancelled` (a running file action keeps the window).
    let mut closed = 0;
    close_secondary_windows_unless_cancelled(true, &mut || closed += 1);
    assert_eq!(closed, 0);
    close_secondary_windows_unless_cancelled(false, &mut || closed += 1);
    assert_eq!(closed, 1);
}
