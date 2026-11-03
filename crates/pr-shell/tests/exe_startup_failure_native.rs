//! Q6 (X-exe-entry): the real exe returns the failure exit code. New test, no C# twin (C# `ShellProcessTests` has no failing-start case; the in-process
//! twin is `startup_run_native::app_startup_startup_fault_reports_the_error_logs_it_and_shuts_down_with_exit_code_one`).
//!
//! A whitespace-only `PHOTOREVIEW_RS_INSTANCE_PREFIX` is kept by the options (C# `Length > 0`) and refused by `InstanceScope`, so the start-up fails before
//! any window exists; the exe must report it in an error TaskDialog and leave with `EXIT_STARTUP_FAILED` (1). This separates
//! `entry::production_run`/`main_entry` from a constant exit code.
#![cfg(windows)]
use pr_platform_win::ui_driver;
use pr_test_support::TempRoot;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(60);

#[test]
#[ignore = "native: a real exe shows the startup error task dialog and needs a desktop"]
fn exe_with_a_blank_instance_prefix_shows_the_error_box_and_exits_one() {
    let root = TempRoot::new("q6-exe-failure");
    let data = root.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    // Prevent first-run import from ever inspecting the C# configuration.
    std::fs::write(data.join("config.json"), "{}").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_PhotoReviewRs"))
        .env("PHOTOREVIEW_RS_DATA_ROOT", &data)
        .env("PHOTOREVIEW_RS_ISOLATE_CONFIG", "1")
        .env("PHOTOREVIEW_RS_INSTANCE_PREFIX", "   ")
        .env("PHOTOREVIEW_RS_WARP", "1")
        .env("PHOTOREVIEW_RS_NOWARM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start real PhotoReviewRs.exe");
    let pid = child.id();
    let (tx, exit) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let deadline = Instant::now() + BOUND;
    let status = loop {
        if let Some(dialog) = ui_driver::main_window_of_process(pid) {
            let _ = ui_driver::post_task_dialog_button(dialog, 1);
        }
        if Instant::now() >= deadline {
            // Only the exact child PID created above; never kill by image name.
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            panic!("the exe did not exit within {BOUND:?}");
        }
        // No external notification exists for a foreign dialog; an exit cuts this tick short.
        match exit.recv_timeout(Duration::from_millis(100)) {
            Ok(status) => break status.expect("wait child"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("child waiter disconnected: {error}"),
        }
    };
    assert_eq!(status.code(), Some(1));
}
