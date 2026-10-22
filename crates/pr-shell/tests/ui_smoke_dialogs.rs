//! Slice S5 of unit W8-20b (`contracts/W8-20b.md`): the whole-shell secondary-window tests that no earlier contract owns, plus the one `NativeDialogLoopTests` case that needs the composition root.
//! Baseline `d92d6aae`: `Startup/ShellNativeDialogsIntegrationTests.cs` (3), `Startup/ShellSecondaryWindowBridgeTests.cs` (2), `Startup/ShellSecondaryWindowFallbackTests.cs` (1),
//! `Shell.Tests/Dialogs/Native/NativeDialogLoopTests.cs` (1, assigned here by `contracts/W8-18o.md`).
//!
//! Native (`#[ignore = "native: needs desktop"]`). Every dialog is found by its owner (the main HWND) and thread; it is closed by posting `WM_COMMAND` `IDOK`/`IDCANCEL` or `WM_CLOSE`; every
//! "wait for the dialog" is a bounded wait on a condition the UI thread signals (no sleep). The "no WPF assembly is loaded" assertions of the C# bridge test have no Rust twin (WPF is retired).
//! STUB: the bodies are `todo!()`.
#![cfg(windows)]
#![allow(dead_code, unused_imports)] // stub

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{OpenTarget, SmokeConfig, UiSmokeRun, vk};

/// C# `ShellNativeDialogsIntegrationTests.NativeDialogs_OpenOwnedAndModal_WithoutTheBridge_AndTheShellClosesCleanlyWithOneOpen` (line 33): Diagnostics, then the zoom prompt (cancel, then OK 150),
/// then the shell closes while a modal Diagnostics is open: exit code 0, the modal returns.
#[test]
#[ignore = "native: needs desktop"]
fn native_dialogs_open_owned_and_modal_and_the_shell_closes_cleanly_with_one_open() {
    todo!("W8-20b S5: contracts/W8-20b.md")
}

/// C# `ShellNativeDialogsIntegrationTests.Benchmark_OpensModeless_RunsRealProfiles_AndTheShellClosesCleanlyWhileItRuns` (line 96).
#[test]
#[ignore = "native: needs desktop"]
fn benchmark_opens_modeless_runs_real_profiles_and_the_shell_closes_cleanly_while_it_runs() {
    todo!("W8-20b S5: contracts/W8-20b.md")
}

/// C# `ShellNativeDialogsIntegrationTests.Recovery_OpensOwnedAndModal_ChecksInTheBackground_ChangesNoData_AndTheShellClosesCleanlyWithItOpen` (line 143).
#[test]
#[ignore = "native: needs desktop"]
fn recovery_opens_owned_and_modal_checks_in_the_background_changes_no_data_and_the_shell_closes_cleanly_with_it_open() {
    todo!("W8-20b S5: contracts/W8-20b.md")
}

/// C# `ShellSecondaryWindowBridgeTests.SecondaryWindows_OpenThroughTheRealShell_AndTheShellClosesCleanlyWithThemOpen` (line 24): Settings (Save writes `config.json`), Recovery (no data
/// change), Diagnostics, the zoom prompt, Benchmark modeless plus one modal, then the shell closes.
#[test]
#[ignore = "native: needs desktop"]
fn secondary_windows_open_through_the_real_shell_and_the_shell_closes_cleanly_with_them_open() {
    todo!("W8-20b S5: contracts/W8-20b.md")
}

/// C# `ShellSecondaryWindowBridgeTests.Closing_ClosesTheSecondaryWindows_OnlyWhenTheCloseIsNotCancelled` (line 176).
#[test]
#[ignore = "native: needs desktop"]
fn closing_closes_the_secondary_windows_only_when_the_close_is_not_cancelled() {
    todo!("W8-20b S5: contracts/W8-20b.md")
}

/// C# `ShellSecondaryWindowFallbackTests.SettingsAndRecoveryOpenAsWin32Windows_WithoutTouchingTheLauncher` (line 43): Settings and Recovery really open (title = the localised window title), the
/// recording launcher saw no call, the status line is not the "secondary window unavailable" text.
#[test]
#[ignore = "native: needs desktop"]
fn settings_and_recovery_open_as_win32_windows_without_touching_the_launcher() {
    todo!("W8-20b S5: contracts/W8-20b.md")
}

/// C# `NativeDialogLoopTests.TheShellCompositionRoot_RunsItsNativeDialogsInsideTheShellLoop`: the production composition root's Diagnostics dialog runs inside the shell's own message loop (no second loop).
#[test]
#[ignore = "native: needs desktop"]
fn the_shell_composition_root_runs_its_native_dialogs_inside_the_shell_loop() {
    todo!("W8-20b S5: contracts/W8-20b.md")
}
