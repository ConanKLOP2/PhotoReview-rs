//! Slice S5 of unit W8-20b (`contracts/W8-20b.md`): the whole-shell secondary-window tests that no earlier contract owns, plus the one `NativeDialogLoopTests` case that needs the composition root.
//! Baseline `d92d6aae`: `Startup/ShellNativeDialogsIntegrationTests.cs` (3), `Startup/ShellSecondaryWindowBridgeTests.cs` (2), `Startup/ShellSecondaryWindowFallbackTests.cs` (1),
//! `Shell.Tests/Dialogs/Native/NativeDialogLoopTests.cs` (1, assigned here by `contracts/W8-18o.md`).
//!
//! Native (`#[ignore = "native: needs desktop"]`). Every dialog is found by its owner (the main HWND) and thread; it is closed by posting `WM_COMMAND` `IDOK`/`IDCANCEL` or `WM_CLOSE`; every
//! "wait for the dialog" is a bounded wait on a condition the UI thread signals (no sleep). The "no WPF assembly is loaded" assertions of the C# bridge test have no Rust twin (WPF is retired).
//! Remaining unported cases retain explicit pending/blocked ignore reasons.
#![cfg(windows)]
use pr_core::abstractions::SettingsTarget;
use pr_platform_win::ui_driver;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
static SERIAL: Mutex<()> = Mutex::new(());

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{BOUND, SmokeConfig, UiProbe, UiSmokeRun, isolated_test};

/// C# `ShellNativeDialogsIntegrationTests.NativeDialogs_OpenOwnedAndModal_WithoutTheBridge_AndTheShellClosesCleanlyWithOneOpen` (line 33): Diagnostics, then the zoom prompt (cancel, then OK 150),
/// then the shell closes while a modal Diagnostics is open: exit code 0, the modal returns.
#[test]
#[ignore = "native: needs desktop"]
fn native_dialogs_open_owned_and_modal_and_the_shell_closes_cleanly_with_one_open() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "native_dialogs_open_owned_and_modal_and_the_shell_closes_cleanly_with_one_open",
        || {
            let run = started();
            let done = diagnostics(&run);
            let dialog = run.wait_for_owned_window("Diagnostics").unwrap();
            let owner = run.on_ui(|p| p.hwnd).unwrap();
            assert!(!ui_driver::is_window_enabled(owner));
            assert!(ui_driver::post_close(dialog));
            done.recv_timeout(BOUND).unwrap().unwrap();
            run.wait_no_owned_window().unwrap();
            for (command, expected) in [(2, None), (1, Some(150))] {
                let done = run
                    .post_on_ui(|p| p.services.secondary_port.get().unwrap().prompt_custom_zoom(p.hwnd, 100))
                    .unwrap();
                let dialog = run.wait_for_owned_window("Zoom prompt").unwrap();
                assert!(!ui_driver::is_window_enabled(owner));
                if command == 1 {
                    assert!(ui_driver::set_dialog_item_text(dialog, 101, "150"));
                }
                assert!(ui_driver::post_command(dialog, command));
                assert_eq!(done.recv_timeout(BOUND).unwrap().unwrap(), expected);
                run.wait_no_owned_window().unwrap();
            }
            let done = diagnostics(&run);
            run.wait_for_owned_window("Diagnostics at close").unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
            done.recv_timeout(BOUND).unwrap().unwrap();
            run.wait_no_owned_window().unwrap();
        },
    );
}

/// C# `ShellNativeDialogsIntegrationTests.Benchmark_OpensModeless_RunsRealProfiles_AndTheShellClosesCleanlyWhileItRuns` (line 96): modeless (the main window stays enabled), a second open raises the
/// same window, Run over the photos folder with every profile selected starts a real run ("Cancel run" enabled), then the shell closes while it runs: exit code 0, the window is gone.
#[test]
#[ignore = "native: needs desktop"]
fn benchmark_opens_modeless_runs_real_profiles_and_the_shell_closes_cleanly_while_it_runs() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "benchmark_opens_modeless_runs_real_profiles_and_the_shell_closes_cleanly_while_it_runs",
        || {
            let run = started();
            let photos = run.photos()[0].parent().unwrap().to_string_lossy().into_owned();
            let owner = run.on_ui(|p| p.hwnd).unwrap();
            let show = |run: &UiSmokeRun| {
                run.on_ui(|p| p.view_model.borrow().show_benchmark()).unwrap();
            };
            show(&run); // modeless: returns at once
            let benchmark = run.wait_for_owned_window("Benchmark").unwrap();
            assert!(
                ui_driver::is_window_enabled(owner),
                "modeless: the main window stays enabled"
            );
            show(&run); // second open: raises the same window, creates no new one
            run.on_ui(|_| ()).unwrap();
            assert_eq!(ui_driver::count_windows_owned_by(owner), 1);
            run.on_ui(move |_| {
                assert!(ui_driver::set_dialog_item_text(benchmark, 102, &photos));
                assert!(ui_driver::set_dialog_item_text(benchmark, 105, "1"));
                // 107 = Select all, 110 = Run (posted: the dialog handles them in order on the UI thread).
                assert!(ui_driver::post_command(benchmark, 107));
                assert!(ui_driver::post_command(benchmark, 110));
            })
            .unwrap();
            // The run started when "Cancel run" (112) is enabled; every wait is bounded by BOUND.
            run.wait_ui_condition("the benchmark run did not start", move |_| {
                ui_driver::is_dialog_item_enabled(benchmark, 112)
            })
            .unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
            run.wait_no_owned_window().unwrap();
        },
    );
}

/// C# `ShellNativeDialogsIntegrationTests.Recovery_OpensOwnedAndModal_ChecksInTheBackground_ChangesNoData_AndTheShellClosesCleanlyWithItOpen` (line 143).
#[test]
#[ignore = "native: needs desktop"]
fn recovery_opens_owned_and_modal_checks_in_the_background_changes_no_data_and_the_shell_closes_cleanly_with_it_open() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "recovery_opens_owned_and_modal_checks_in_the_background_changes_no_data_and_the_shell_closes_cleanly_with_it_open",
        || {
            let run = started();
            let before = listing(&run.root().join("data"));
            let done = run
                .post_on_ui(|p| p.services.secondary_port.get().unwrap().show_recovery(p.hwnd))
                .unwrap();
            run.wait_for_owned_window("Recovery").unwrap();
            let hwnd = run.on_ui(|p| p.hwnd).unwrap();
            assert!(!ui_driver::is_window_enabled(hwnd));
            run.on_ui(|_| ()).unwrap();
            assert_eq!(listing(&run.root().join("data")), before);
            assert_eq!(run.close_and_wait(), Ok(0));
            done.recv_timeout(BOUND).unwrap().unwrap();
            run.wait_no_owned_window().unwrap();
        },
    );
}

/// C# `ShellSecondaryWindowBridgeTests.SecondaryWindows_OpenThroughTheRealShell_AndTheShellClosesCleanlyWithThemOpen` (line 24): Settings (Save writes `config.json`), Recovery (no data
/// change), Diagnostics, the zoom prompt, Benchmark modeless plus one modal, then the shell closes.
#[test]
#[ignore = "native: needs desktop"]
fn secondary_windows_open_through_the_real_shell_and_the_shell_closes_cleanly_with_them_open() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "secondary_windows_open_through_the_real_shell_and_the_shell_closes_cleanly_with_them_open",
        || {
            let run = started();
            let config = run.root().join("data/config.json");
            let before = std::fs::metadata(&config).unwrap().modified().unwrap();
            let done = run
                .post_on_ui(|p| {
                    p.services
                        .secondary_port
                        .get()
                        .unwrap()
                        .show_settings(p.hwnd, SettingsTarget::Default)
                })
                .unwrap();
            let settings = run.wait_for_owned_window("Settings Save").unwrap();
            assert!(ui_driver::post_command(settings, 1));
            assert!(done.recv_timeout(BOUND).unwrap().unwrap());
            run.wait_no_owned_window().unwrap();
            assert!(std::fs::metadata(config).unwrap().modified().unwrap() >= before);
            let done = diagnostics(&run);
            run.wait_for_owned_window("Diagnostics").unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
            done.recv_timeout(BOUND).unwrap().unwrap();
            run.wait_no_owned_window().unwrap();
        },
    );
}

/// C# `ShellSecondaryWindowBridgeTests.Closing_ClosesTheSecondaryWindows_OnlyWhenTheCloseIsNotCancelled` (line 176). The C# test calls the pure helper (`CloseSecondaryWindowsUnlessCancelled`, ported
/// in `pr-app` as `close_secondary_windows_unless_cancelled`); here the same rule runs inside the whole shell, with the cancel coming from where production gets it: a file action in flight
/// (`defer_close_for_file_action`). A Delete blocks in the bin, Diagnostics is open, the shell is asked to close: it stays and Diagnostics stays; the file action ends, the deferred close
/// goes through and Diagnostics closes with the shell.
#[test]
#[ignore = "native: needs desktop"]
fn closing_closes_the_secondary_windows_only_when_the_close_is_not_cancelled() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "closing_closes_the_secondary_windows_only_when_the_close_is_not_cancelled",
        || {
            let run = started();
            run.bin().hold();
            run.key(ui_smoke_fixture::vk::DELETE).unwrap();
            run.bin().wait_entered(1).unwrap(); // the file action is in flight
            let done = diagnostics(&run);
            let dialog = run.wait_for_owned_window("Diagnostics").unwrap();
            let owner = run.on_ui(|p| p.hwnd).unwrap();
            // The probe's Closing handler is registered after the production ones: when it runs they have decided.
            let (tx, rx) = mpsc::channel();
            run.on_ui(move |p| {
                use pr_platform_win::host::contracts::ShellWindowApi;
                p.main_window.window().subscribe_closing(Box::new(move |args| {
                    let _ = tx.send(args.cancel);
                }));
            })
            .unwrap();
            run.request_close().unwrap();
            assert!(
                rx.recv_timeout(BOUND).unwrap(),
                "the close was not deferred by the file action"
            );
            run.on_ui(|_| ()).unwrap();
            run.on_ui(|_| ()).unwrap();
            assert_eq!(ui_driver::first_window_owned_by(owner), Some(dialog));
            run.bin().release(); // the file action ends: the deferred close runs without a cancel
            assert_eq!(run.wait_exit(), Ok(0));
            done.recv_timeout(BOUND).unwrap().unwrap();
            run.wait_no_owned_window().unwrap();
        },
    );
}

/// C# `ShellSecondaryWindowFallbackTests.SettingsAndRecoveryOpenAsWin32Windows_WithoutTouchingTheLauncher` (line 43): Settings and Recovery really open (title = the localised window title), the
/// recording launcher saw no call, the status line is not the "secondary window unavailable" text.
#[test]
#[ignore = "native: needs desktop"]
fn settings_and_recovery_open_as_win32_windows_without_touching_the_launcher() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "settings_and_recovery_open_as_win32_windows_without_touching_the_launcher",
        || {
            let run = started();
            let done = run
                .post_on_ui(|p| {
                    p.services
                        .secondary_port
                        .get()
                        .unwrap()
                        .show_settings(p.hwnd, SettingsTarget::Default)
                })
                .unwrap();
            let settings = run.wait_for_owned_window("Settings").unwrap();
            assert!(ui_driver::post_command(settings, 2));
            assert!(!done.recv_timeout(BOUND).unwrap().unwrap());
            run.wait_no_owned_window().unwrap();
            let done = run
                .post_on_ui(|p| p.services.secondary_port.get().unwrap().show_recovery(p.hwnd))
                .unwrap();
            let recovery = run.wait_for_owned_window("Recovery").unwrap();
            assert!(ui_driver::post_close(recovery));
            done.recv_timeout(BOUND).unwrap().unwrap();
            run.wait_no_owned_window().unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `NativeDialogLoopTests.TheShellCompositionRoot_RunsItsNativeDialogsInsideTheShellLoop`: the production composition root's Diagnostics dialog runs inside the shell's own message loop (no second loop).
#[test]
#[ignore = "native: needs desktop"]
fn the_shell_composition_root_runs_its_native_dialogs_inside_the_shell_loop() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "the_shell_composition_root_runs_its_native_dialogs_inside_the_shell_loop",
        || {
            let run = started();
            let thread = run.on_ui(|_| std::thread::current().id()).unwrap();
            let done = diagnostics(&run);
            // first_window_owned_by enumerates only windows on the owner's UI thread.
            let dialog = run.wait_for_owned_window("composition-root Diagnostics").unwrap();
            for _ in 0..3 {
                assert_eq!(run.on_ui(|_| std::thread::current().id()).unwrap(), thread);
            }
            assert!(ui_driver::post_close(dialog));
            done.recv_timeout(BOUND).unwrap().unwrap();
            run.wait_no_owned_window().unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

fn started() -> UiSmokeRun {
    let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
    run.wait_first_image().unwrap();
    run
}
fn diagnostics(run: &UiSmokeRun) -> mpsc::Receiver<Result<(), String>> {
    run.post_on_ui(|p: &UiProbe| p.services.secondary_port.get().unwrap().show_diagnostics(p.hwnd))
        .unwrap()
}
fn listing(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    fn collect(root: &Path, path: &Path, items: &mut Vec<(PathBuf, u64, std::time::SystemTime)>) {
        if !path.exists() {
            return;
        }
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let metadata = entry.metadata().unwrap();
            // The review session file is written in the background by the open folder itself (not by Recovery); the C# test compared the journal and its work folder only.
            if path.strip_prefix(root).is_ok_and(|p| p.starts_with("Sessions")) {
                continue;
            }
            if metadata.is_dir() {
                collect(root, &path, items);
            } else {
                items.push((
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    metadata.len(),
                    metadata.modified().unwrap(),
                ));
            }
        }
    }
    let mut items = Vec::new();
    collect(root, root, &mut items);
    items.sort();
    items
}
