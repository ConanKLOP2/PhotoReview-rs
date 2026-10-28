//! Port of Startup/ShellStartupTests.cs (W8-17e), using the bounded isolated whole-shell rig.
#![cfg(windows)]
#[path = "common/ui_smoke_fixture.rs"]
mod rig;
use pr_platform_win::host::contracts::{ShellWindowApi, ShellWindowEvent};
use rig::{SmokeConfig, UiSmokeRun, isolated_test};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FailureRecorder {
    calls: Vec<String>,
    throwing_dialog: bool,
}
impl pr_app::startup::shutdown::FailureSteps for FailureRecorder {
    fn log_failure(&mut self, error: &dyn std::error::Error) {
        self.calls.push(format!("log:{error}"));
    }
    fn show_failure_box(&mut self, message: &str) {
        self.calls.push(format!("box:{message}"));
        assert!(!self.throwing_dialog, "test dialog failure");
    }
    fn force_exit_code(&mut self, code: i32) {
        self.calls.push(format!("force:{code}"));
    }
    fn close_secondary_windows(&mut self) {
        self.calls.push("close-secondary".into());
    }
    fn dispose_main_window(&mut self) {
        self.calls.push("dispose-main".into());
    }
    fn quit(&mut self, code: i32) {
        self.calls.push(format!("quit:{code}"));
    }
}

// C#: AppStartupBranchTests.AppStartup_StartupFault_ReportsTheErrorLogsItAndShutsDownWithExitCodeOne.
#[test]
fn app_startup_startup_fault_reports_the_error_logs_it_and_shuts_down_with_exit_code_one() {
    let mut steps = FailureRecorder::default();
    pr_app::startup::shutdown::run_fail_startup(&mut steps, &std::io::Error::other("startup fault"));
    assert_eq!(
        steps.calls,
        [
            "log:startup fault",
            "box:startup fault",
            "force:1",
            "close-secondary",
            "dispose-main",
            "quit:1"
        ]
    );
}

// C#: AppStartupBranchTests.FailStartup_WithServices_ShowsTheErrorThroughTheRegisteredDialogServiceAndDisposesTheApp.
#[test]
fn fail_startup_with_services_shows_the_error_through_the_registered_dialog_service_and_disposes_the_app() {
    let mut steps = FailureRecorder::default();
    pr_app::startup::shutdown::run_fail_startup(&mut steps, &std::io::Error::other("registered service error"));
    assert_eq!(
        steps
            .calls
            .iter()
            .filter(|m| m.as_str() == "box:registered service error")
            .count(),
        1
    );
    let disposed = steps.calls.iter().position(|m| m == "dispose-main").unwrap();
    let quit = steps.calls.iter().position(|m| m == "quit:1").unwrap();
    assert!(disposed < quit);
}

// C#: AppStartupBranchTests.FailStartup_WhenTheDialogThrows_StillDisposesAndShutsDown.
#[test]
fn fail_startup_when_the_dialog_throws_still_disposes_and_shuts_down() {
    let mut steps = FailureRecorder {
        throwing_dialog: true,
        ..Default::default()
    };
    pr_app::startup::shutdown::run_fail_startup(&mut steps, &std::io::Error::other("startup fault"));
    assert_eq!(
        &steps.calls[2..],
        ["force:1", "close-secondary", "dispose-main", "quit:1"]
    );
}

#[derive(Default)]
struct ShutdownRecorder {
    calls: Vec<&'static str>,
}
impl ShutdownRecorder {
    fn step(&mut self, name: &'static str) {
        if !self.calls.contains(&name) {
            self.calls.push(name);
        }
    }
}
impl pr_app::startup::shutdown::ShutdownSteps for ShutdownRecorder {
    fn stop_listening(&mut self) {
        self.step("listen");
    }
    fn dispose_coalescer(&mut self) {
        self.step("coalescer");
    }
    fn close_secondary_windows(&mut self) {
        self.step("secondary");
    }
    fn dispose_main_window(&mut self) {
        self.step("main");
    }
    fn dispose_cloak(&mut self) {
        self.step("cloak");
    }
    fn dispose_frame_clock(&mut self) {
        self.step("clock");
    }
    fn dispose_services(&mut self) {
        self.step("services");
    }
    fn shutdown_log(&mut self) {
        self.step("log");
    }
    fn dispose_instance_scope(&mut self) {
        self.step("instance");
    }
    fn dispose_perf_listener(&mut self) {
        self.step("perf");
    }
}

// C#: AppStartupBranchTests.StartupCore_ExitEvent_DisposesTheApp.
#[test]
fn startup_core_exit_event_disposes_the_app() {
    let mut steps = ShutdownRecorder::default();
    pr_app::startup::shutdown::run_shutdown(&mut steps);
    pr_app::startup::shutdown::run_shutdown(&mut steps);
    assert_eq!(
        steps.calls,
        [
            "listen",
            "coalescer",
            "secondary",
            "main",
            "cloak",
            "clock",
            "services",
            "log",
            "instance",
            "perf"
        ]
    );
}

// C#: AppStartupBranchTests.StartupCore_AppDomainException_IsLoggedAsCrashReport.
#[test]
#[ignore = "native: process-wide panic hook is tested in its own isolated child"]
fn startup_core_app_domain_exception_is_logged_as_crash_report() {
    isolated_test("startup_core_app_domain_exception_is_logged_as_crash_report", || {
        // C# reads the application log file (AppLog is a static): the hook writes a forced crash report to `AppLog::global()`.
        pr_shell::startup::run::install_process_hooks(Arc::new(pr_test_support::RecordingLog::new()));
        assert!(std::panic::catch_unwind(|| panic!("test unhandled domain fault")).is_err());
        let log = std::fs::read_to_string(pr_app::misc::app_log::AppLog::global().file_path()).unwrap();
        assert!(
            log.contains("Unhandled panic") && log.contains("test unhandled domain fault"),
            "{log}"
        );
    });
}

struct PanicOnce(std::cell::Cell<bool>);
impl pr_platform_win::host::contracts::WindowMessageHandler for PanicOnce {
    fn try_handle(&self, message: &pr_platform_win::host::contracts::WindowMessage) -> Option<isize> {
        if message.msg == 0x100 && self.0.replace(false) {
            panic!("test dispatcher fault");
        }
        None
    }
}

// C#: AppStartupBranchTests.StartupCore_DispatcherException_IsLoggedAsCrashReportAndMarkedHandled.
#[test]
#[ignore = "native: needs desktop and a real window-procedure panic"]
fn startup_core_dispatcher_exception_is_logged_as_crash_report_and_marked_handled() {
    isolated_test(
        "startup_core_dispatcher_exception_is_logged_as_crash_report_and_marked_handled",
        || {
            let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
            run.wait_first_image().unwrap();
            run.on_ui(|p| {
                p.main_window
                    .window()
                    .insert_message_handler_first(std::rc::Rc::new(PanicOnce(std::cell::Cell::new(true))))
            })
            .unwrap();
            run.key(0x27).unwrap();
            run.on_ui(|_| ()).unwrap(); // The loop must recover and process subsequent jobs.
            assert_eq!(run.close_and_wait(), Ok(0));
            let crash = std::fs::read_to_string(pr_app::misc::app_log::AppLog::global().file_path()).unwrap();
            assert!(
                crash.contains("UI loop exception") && crash.contains("test dispatcher fault"),
                "{crash}"
            );
        },
    );
}

struct GatedBin {
    inner: Arc<rig::TempFolderRecycleBin>,
    entered: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl pr_core::abstractions::RecycleBin for GatedBin {
    fn send_to_recycle_bin(&self, path: &std::path::Path) -> std::io::Result<()> {
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv_timeout(rig::BOUND).unwrap();
        pr_core::abstractions::RecycleBin::send_to_recycle_bin(self.inner.as_ref(), path)
    }
    fn try_restore(
        &self,
        path: &std::path::Path,
        size: u64,
        when: pr_core::time::DateTimeUtc,
    ) -> std::io::Result<bool> {
        pr_core::abstractions::RecycleBin::try_restore(self.inner.as_ref(), path, size, when)
    }
}

// C#: ShellStartupTests.Close_WhileAFileActionHoldsTheGate_KeepsTheWindowUntilTheActionIsDone.
#[test]
#[ignore = "native: needs desktop; only a private temporary bin is used"]
fn close_while_a_file_action_holds_the_gate_keeps_the_window_until_the_action_is_done() {
    isolated_test(
        "close_while_a_file_action_holds_the_gate_keeps_the_window_until_the_action_is_done",
        || {
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let run = UiSmokeRun::start(SmokeConfig {
                graph: rig::GraphChoice::Custom(Box::new(move |paths| {
                    let bin = Arc::new(GatedBin {
                        inner: paths.bin.clone(),
                        entered: entered_tx,
                        release: Mutex::new(release_rx),
                    });
                    pr_shell::startup::graph::production_graph_builder_with(pr_shell::startup::graph::GraphOverrides {
                        paths: Some(paths.paths.clone()),
                        recycle_bin: Some(bin),
                        ..Default::default()
                    })
                })),
                ..Default::default()
            })
            .unwrap();
            run.wait_first_image().unwrap();
            run.key(0x2e).unwrap(); // Delete enters the fake bin on the file-action worker.
            entered_rx.recv_timeout(rig::BOUND).unwrap();
            run.request_close().unwrap();
            let still_alive = run
                .on_ui(|p| {
                    p.view_model.borrow().is_file_action_in_progress()
                        && pr_platform_win::win32::dialog_native::user32::is_window(p.hwnd)
                })
                .unwrap();
            assert!(still_alive, "close must wait for the held file-action gate");
            release_tx.send(()).unwrap();
            assert_eq!(run.wait_exit(), Ok(0));
            assert_eq!(run.bin().sent().len(), 1);
        },
    );
}

// C#: ShellStartupTests.Startup_SavedPlacement_IsRestoredWhileTheWindowIsStillHidden.
#[test]
#[ignore = "native: needs desktop"]
fn startup_saved_placement_is_restored_while_the_window_is_still_hidden() {
    isolated_test(
        "startup_saved_placement_is_restored_while_the_window_is_still_hidden",
        || {
            let hwnd = Arc::new(Mutex::new(0));
            let captured = hwnd.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let mut config = saved_placement_config(false, 1);
            config.configure = Some(Box::new(move |o| {
                o.set_cloaked = Some(Arc::new(move |handle, _| {
                    *captured.lock().unwrap() = handle;
                    true
                }));
                o.mark = Some(Arc::new(move |m| {
                    if m == "placementRestoredBeforeShow" {
                        let handle = *hwnd.lock().unwrap();
                        let rect = pr_platform_win::win32::api::user32::get_window_rect(handle).unwrap();
                        tx.send((
                            pr_platform_win::win32::api::user32::is_window_visible(handle),
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                        ))
                        .unwrap();
                    }
                }));
            }));
            let run = UiSmokeRun::start(config).unwrap();
            let (visible, width, height) = rx.recv_timeout(rig::BOUND).unwrap();
            assert!(!visible);
            assert_eq!((width, height), (800, 600));
            run.wait_first_image().unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

// C#: ShellStartupTests.Startup_SavedMaximizedPlacement_ReopensMaximized.
#[test]
#[ignore = "native: needs desktop"]
fn startup_saved_maximized_placement_reopens_maximized() {
    isolated_test("startup_saved_maximized_placement_reopens_maximized", || {
        let run = UiSmokeRun::start(saved_placement_config(false, 3)).unwrap();
        run.wait_first_image().unwrap();
        let command = run
            .on_ui(|p| {
                let mut placement = pr_platform_win::win32::structs::new_window_placement();
                pr_platform_win::win32::api::user32::get_window_placement(p.hwnd, &mut placement).unwrap();
                placement.showCmd
            })
            .unwrap();
        assert_eq!(command, 3);
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

// C#: ShellStartupTests.Startup_WithoutSavedPlacement_OpensAtTheWpfOuterSize.
#[test]
#[ignore = "native: needs desktop"]
fn startup_without_saved_placement_opens_at_the_wpf_outer_size() {
    isolated_test("startup_without_saved_placement_opens_at_the_wpf_outer_size", || {
        let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
        run.wait_first_image().unwrap();
        let (width, height, scale) = run
            .on_ui(|p| {
                let rect = pr_platform_win::win32::api::user32::get_window_rect(p.hwnd).unwrap();
                (
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    p.main_window.window().dpi_scale(),
                )
            })
            .unwrap();
        assert!((f64::from(width) / scale - 1200.0).abs() <= 1.0);
        assert!((f64::from(height) / scale - 800.0).abs() <= 1.0);
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

// C#: ShellStartupTests.SecondInstance_ForwardingWhileTheFirstIsStillStarting_IsAppliedOnceTheWindowIsReady.
#[test]
#[ignore = "native: startup and forwarded real exe need desktop"]
fn second_instance_forwarding_while_the_first_is_still_starting_is_applied_once_the_window_is_ready() {
    isolated_test(
        "second_instance_forwarding_while_the_first_is_still_starting_is_applied_once_the_window_is_ready",
        || {
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let run = UiSmokeRun::start(SmokeConfig {
                configure: Some(Box::new(move |o| {
                    o.on_main_window_ready = Some(Box::new(move |_| {
                        ready_tx.send(()).unwrap();
                        release_rx.recv_timeout(rig::BOUND).unwrap();
                    }));
                })),
                ..Default::default()
            })
            .unwrap();
            ready_rx.recv_timeout(rig::BOUND).unwrap();
            let path = run.photos()[1].to_string_lossy().into_owned();
            let prefix = std::env::var("PHOTOREVIEW_RS_INSTANCE_PREFIX").unwrap();
            let mut second = process_fixture::ShellProcess::start(&[path], Some(&prefix));
            assert_eq!(second.wait_exit().code(), Some(0));
            release_tx.send(()).unwrap();
            let snapshot = run
                .wait_until("forwarded image after startup", |s| {
                    s.current_index == 1 && s.first_image_presented
                })
                .unwrap();
            assert!(snapshot.current_path.unwrap().ends_with("s01.jpg"));
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

// C#: ShellStartupTests.Startup_FailureAfterTheWindowExists_ReportsOnceAndExitsWithTheFailureCode.
#[test]
#[ignore = "native: needs desktop and startup error dialog"]
fn startup_failure_after_the_window_exists_reports_once_and_exits_with_the_failure_code() {
    isolated_test(
        "startup_failure_after_the_window_exists_reports_once_and_exits_with_the_failure_code",
        || {
            let run = UiSmokeRun::start(SmokeConfig {
                configure: Some(Box::new(|o| {
                    o.on_main_window_ready = Some(Box::new(|_| panic!("test startup fault after window")));
                })),
                task_dialogs: true,
                ..Default::default()
            })
            .unwrap();
            let error = run.wait_for_owned_window("startup failure message").unwrap();
            assert!(pr_platform_win::ui_driver::post_command(error, 1));
            assert_eq!(run.wait_exit(), Ok(pr_app::startup::options::EXIT_STARTUP_FAILED));
        },
    );
}

// C#: ShellStartupTests.Startup_FailedJournalEntries_AskOnceToOpenRecovery.
#[test]
#[ignore = "native: needs desktop and recovery confirmation"]
fn startup_failed_journal_entries_ask_once_to_open_recovery() {
    isolated_test("startup_failed_journal_entries_ask_once_to_open_recovery", || {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let called = calls.clone();
        let run = UiSmokeRun::start(SmokeConfig {
            configure: Some(Box::new(move |o| {
                o.journal_recovery = Some(Box::new(move |_, _, _| {
                    use pr_core::{
                        file_actions::journal_line::{JournalDateTime, JournalEntry},
                        model::{FileOperationType, JournalState},
                        time::DateTimeUtc,
                    };
                    called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let when = JournalDateTime::utc(DateTimeUtc::from_ticks(639_000_000_000_000_000).unwrap());
                    vec![JournalEntry {
                        id: "failed-startup-test".into(),
                        op_type: FileOperationType::Move,
                        state: JournalState::Failed,
                        source: "failed.jpg".into(),
                        destination: None,
                        size: 1,
                        last_write_utc: when,
                        timestamp_utc: when,
                        error: None,
                        error_code: None,
                        permanent: None,
                        undo: None,
                        group_id: None,
                        group_members: None,
                    }]
                }));
            })),
            task_dialogs: true,
            ..Default::default()
        })
        .unwrap();
        let prompt = run.wait_for_owned_window("failed-journal confirmation").unwrap();
        assert!(pr_platform_win::ui_driver::post_task_dialog_button(prompt, 7)); // IDNO: no Recovery window should open.
        run.wait_no_owned_window().unwrap();
        run.wait_first_image().unwrap();
        run.on_ui(|_| ()).unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

// C#: ShellStartupTests.Startup_ShutdownIsClean_NoWindowDispatcherOrGuiObjectLeak.
#[test]
#[ignore = "native: needs desktop and exclusive GUI resource counters"]
fn startup_shutdown_is_clean_no_window_dispatcher_or_gui_object_leak() {
    isolated_test(
        "startup_shutdown_is_clean_no_window_dispatcher_or_gui_object_leak",
        || {
            use pr_platform_win::win32::dialog_native::{kernel32, user32};
            let resources = || {
                let process = kernel32::get_current_process();
                (
                    user32::get_gui_resources(process, 0),
                    user32::get_gui_resources(process, 1),
                )
            };
            let cycle = || {
                let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
                run.wait_first_image().unwrap();
                let hwnd = run.on_ui(|p| p.hwnd).unwrap();
                assert_eq!(run.close_and_wait(), Ok(0));
                drop(run);
                assert!(!user32::is_window(hwnd));
            };
            cycle();
            let before = resources();
            for _ in 0..4 {
                cycle();
            }
            let after = resources();
            // C#: four cycles after a warm-up, at most 2 more GDI and at most 2 more USER objects (a lazily created system object may appear once; a leak per cycle would add 4 or more).
            assert!(
                after.0 <= before.0 + 2 && after.1 <= before.1 + 2,
                "GUI objects {before:?} -> {after:?}"
            );
        },
    );
}

#[path = "common/w8_20_process_fixture.rs"]
mod process_fixture;

// C#: ShellStartupTests.Smoke_WithAnImage_TakesNoLockNoPlacementNoReconcileAndExitsZero.
#[test]
#[ignore = "native: real smoke window needs desktop"]
fn smoke_with_an_image_takes_no_lock_no_placement_no_reconcile_and_exits_zero() {
    isolated_test(
        "smoke_with_an_image_takes_no_lock_no_placement_no_reconcile_and_exits_zero",
        || {
            let (_photos, a, _) = process_fixture::photos();
            let run = UiSmokeRun::start(SmokeConfig {
                open: rig::OpenTarget::Arguments(vec!["--smoke".into(), a.to_string_lossy().into_owned()]),
                configure: Some(Box::new(|o| {
                    o.journal_recovery = Some(Box::new(|_, _, _| panic!("smoke must not reconcile")));
                })),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(run.wait_exit(), Ok(0));
            assert!(!run.root().join("data/window-placement.json").exists());
            assert!(!run.root().join("data/operations.jsonl").exists());
            assert!(
                !run.marks()
                    .iter()
                    .any(|m| m == "instanceLock" || m == "placementRestoredBeforeShow")
            );
        },
    );
}

// C#: ShellStartupTests.Smoke_WhenNoImageFrameEverArrives_ExitsWithTheSmokeTimeoutCode.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_when_no_image_frame_ever_arrives_exits_with_the_smoke_timeout_code() {
    isolated_test(
        "smoke_when_no_image_frame_ever_arrives_exits_with_the_smoke_timeout_code",
        || {
            // A file-shaped path with invalid bytes asks smoke for an image-present event that cannot arrive.
            let root = pr_test_support::TempRoot::new("startup-smoke-broken-image");
            let photo = root.path().join("broken.jpg");
            std::fs::write(&photo, b"not a JPEG").unwrap();
            let run = UiSmokeRun::start(SmokeConfig {
                open: rig::OpenTarget::Arguments(vec!["--smoke".into(), photo.to_string_lossy().into_owned()]),
                configure: Some(Box::new(|o| o.smoke_timeout = std::time::Duration::from_millis(200))),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(run.wait_exit(), Ok(pr_app::startup::options::EXIT_SMOKE_TIMEOUT));
            assert!(!run.root().join("data/window-placement.json").exists());
        },
    );
}

fn saved_placement_config(smoke: bool, show_command: i32) -> SmokeConfig {
    use pr_app::windowing::{contracts::WindowPlacementData, placement_store::JsonWindowPlacementStore};
    SmokeConfig {
        open: if smoke {
            rig::OpenTarget::Arguments(vec!["--smoke".into()])
        } else {
            rig::OpenTarget::FirstPhoto
        },
        graph: rig::GraphChoice::Custom(Box::new(move |paths| {
            let placement = WindowPlacementData {
                length: 44,
                flags: 0,
                show_command,
                min_x: -1,
                min_y: -1,
                max_x: -1,
                max_y: -1,
                normal_left: 120,
                normal_top: 120,
                normal_right: 920,
                normal_bottom: 720,
            };
            std::fs::write(
                &paths.paths.window_placement_file,
                JsonWindowPlacementStore::serialize(&placement),
            )
            .unwrap();
            pr_shell::startup::graph::production_graph_builder_with(pr_shell::startup::graph::GraphOverrides {
                paths: Some(paths.paths.clone()),
                recycle_bin: Some(paths.bin.clone()),
                ..Default::default()
            })
        })),
        ..Default::default()
    }
}

// C#: ShellStartupTests.Smoke_WithASavedPlacement_NeitherAppliesItNorRewritesIt.
#[test]
#[ignore = "native: needs desktop"]
fn smoke_with_a_saved_placement_neither_applies_it_nor_rewrites_it() {
    isolated_test(
        "smoke_with_a_saved_placement_neither_applies_it_nor_rewrites_it",
        || {
            let config = saved_placement_config(true, 1);
            let run = UiSmokeRun::start(config).unwrap();
            assert_eq!(run.wait_exit(), Ok(0));
            assert!(!run.marks().iter().any(|m| m == "placementRestoredBeforeShow"));
            let text = std::fs::read_to_string(run.root().join("data/window-placement.json")).unwrap();
            let placement = pr_app::windowing::placement_store::JsonWindowPlacementStore::deserialize(&text)
                .unwrap()
                .unwrap();
            assert_eq!(
                (
                    placement.normal_left,
                    placement.normal_top,
                    placement.normal_right,
                    placement.normal_bottom
                ),
                (120, 120, 920, 720)
            );
            assert_eq!(placement.show_command, 1);
        },
    );
}

// C#: ShellStartupTests.Startup_SavedPlacement_IsAppliedBeforeTheWindowIsShown.
#[test]
#[ignore = "native: needs desktop"]
fn startup_saved_placement_is_applied_before_the_window_is_shown() {
    isolated_test("startup_saved_placement_is_applied_before_the_window_is_shown", || {
        let run = UiSmokeRun::start(saved_placement_config(false, 1)).unwrap();
        run.wait_first_image().unwrap();
        let marks = run.marks();
        let placement = marks
            .iter()
            .position(|m| m == "placementRestoredBeforeShow")
            .expect("placement applied");
        let shown = marks.iter().position(|m| m == "windowShown").unwrap();
        assert!(placement < shown, "{marks:?}");
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

// C#: ShellStartupTests.Startup_WithJpeg_OpensWindowThenDrawsFirstImageThenClosesCleanly.
#[test]
#[ignore = "native: needs desktop"]
fn startup_with_jpeg_opens_window_then_draws_first_image_then_closes_cleanly() {
    isolated_test(
        "startup_with_jpeg_opens_window_then_draws_first_image_then_closes_cleanly",
        || {
            let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
            assert!(run.wait_first_image().unwrap().first_image_presented);
            let marks = run.marks();
            let shown = marks.iter().position(|m| m == "windowShown").expect("shown mark");
            let present = marks
                .iter()
                .position(|m| m == "firstPresent")
                .expect("first present mark");
            assert!(shown < present, "{marks:?}");
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

// C#: ShellStartupTests.Startup_WindowIsCloakedFromItsHandleUntilTheFirstPresent.
#[test]
#[ignore = "native: needs desktop"]
fn startup_window_is_cloaked_from_its_handle_until_the_first_present() {
    isolated_test(
        "startup_window_is_cloaked_from_its_handle_until_the_first_present",
        || {
            let events = Arc::new(Mutex::new(Vec::new()));
            let captured = events.clone();
            let run = UiSmokeRun::start(SmokeConfig {
                configure: Some(Box::new(move |o| {
                    let marks = captured.clone();
                    o.mark = Some(Arc::new(move |m| marks.lock().unwrap().push(m.to_owned())));
                    o.set_cloaked = Some(Arc::new(move |hwnd, cloak| {
                        assert_ne!(hwnd, 0);
                        captured.lock().unwrap().push(format!("cloak:{cloak}"));
                        true
                    }));
                })),
                ..Default::default()
            })
            .unwrap();
            run.wait_first_image().unwrap();
            // A dispatcher barrier lets the reveal continuation finish before inspecting its marks.
            run.on_ui(|_| ()).unwrap();
            let marks = events.lock().unwrap().clone();
            let hidden = marks.iter().position(|m| m == "cloak:true").unwrap();
            let shown = marks.iter().position(|m| m == "windowShown").unwrap();
            let first = marks.iter().position(|m| m == "firstPresent").unwrap();
            let reveal = marks.iter().position(|m| m == "cloak:false").unwrap();
            assert!(hidden < shown && shown <= first && first <= reveal, "{marks:?}");
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

// C#: ShellStartupTests.Startup_CloakIsRequestedWhileTheWindowIsNotYetVisible.
#[test]
#[ignore = "native: needs desktop"]
fn startup_cloak_is_requested_while_the_window_is_not_yet_visible() {
    isolated_test("startup_cloak_is_requested_while_the_window_is_not_yet_visible", || {
        let requested = Arc::new(Mutex::new(Vec::new()));
        let captured = requested.clone();
        let run = UiSmokeRun::start(SmokeConfig {
            configure: Some(Box::new(move |o| {
                o.set_cloaked = Some(Arc::new(move |hwnd, cloak| {
                    if cloak {
                        captured
                            .lock()
                            .unwrap()
                            .push(pr_platform_win::win32::api::user32::is_window_visible(hwnd));
                    }
                    true
                }));
            })),
            ..Default::default()
        })
        .unwrap();
        run.wait_first_image().unwrap();
        assert_eq!(*requested.lock().unwrap(), vec![false]);
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

// C#: ShellStartupTests.Startup_JournalReconcile_RunsAfterTheLockAndShowAndBeforeAnyQueuedInput.
#[test]
#[ignore = "native: needs desktop"]
fn startup_journal_reconcile_runs_after_the_lock_and_show_and_before_any_queued_input() {
    isolated_test(
        "startup_journal_reconcile_runs_after_the_lock_and_show_and_before_any_queued_input",
        || {
            let (tx, rx) = std::sync::mpsc::channel();
            let marks = Arc::new(Mutex::new(Vec::new()));
            let captured = marks.clone();
            let run = UiSmokeRun::start(SmokeConfig {
                configure: Some(Box::new(move |o| {
                    let recorded = captured.clone();
                    o.mark = Some(Arc::new(move |m| recorded.lock().unwrap().push(m.to_owned())));
                    o.journal_recovery = Some(Box::new(move |_, _, _| {
                        let marks = captured.lock().unwrap().clone();
                        tx.send(marks).unwrap();
                        Vec::new()
                    }));
                })),
                ..Default::default()
            })
            .unwrap();
            let marks = rx.recv_timeout(rig::BOUND).unwrap();
            let lock = marks.iter().position(|m| m == "instanceLock").unwrap();
            let shown = marks.iter().position(|m| m == "windowShown").unwrap();
            assert!(lock < shown, "{marks:?}");
            run.wait_first_image().unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

// C#: ShellStartupTests.Shutdown_ReleasesTheMainWindowBeforeTheServiceContainerIsDisposed.
#[test]
#[ignore = "native: needs desktop"]
fn shutdown_releases_the_main_window_before_the_service_container_is_disposed() {
    isolated_test(
        "shutdown_releases_the_main_window_before_the_service_container_is_disposed",
        || {
            let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
            run.wait_first_image().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            run.on_ui(move |p| {
                let weak = std::rc::Rc::downgrade(&p.main_window);
                p.main_window.window().subscribe(
                    ShellWindowEvent::Closed,
                    Box::new(move || {
                        let window = weak.upgrade().expect("main window alive during Closed");
                        tx.send(!window.has_surface()).unwrap();
                    }),
                );
            })
            .unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
            assert!(rx.recv_timeout(rig::BOUND).unwrap());
        },
    );
}
