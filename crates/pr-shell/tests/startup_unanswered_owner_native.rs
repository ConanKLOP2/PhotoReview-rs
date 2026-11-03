//! Q6 (X-startup-instance): the production half of the "owner does not answer" hand-off, which the pure decision table
//! (`pr_app` `startup_instance`) and the gate half (`startup_instance::a_second_scope_is_refused_and_forwards_to_the_owner`) leave open.
//! New test, no C# twin (C# `ShellStartup` shows the box through `IDialogService.ShowMessage`; no C# test drives it).
//!
//! The owner is this test process: a real `InstanceScope` that holds the mutex and then `stop_listening()`s, so the pipe is gone while the
//! mutex stays held. The real exe is started with that prefix; its hand-off cannot be forwarded, so `phase_one` must show the TaskDialog
//! (`ExitWithMessage(AppAlreadyRunningNoResponse)`, or `FolderAlreadyOpen` in per-folder mode) and exit 0 once it is answered. The text of the box is read
//! back with UI Automation and compared with the catalog text (the catalog itself is covered by `startup_texts_golden`).
#![cfg(windows)]
#[path = "common/w8_20_process_fixture.rs"]
mod fixture;
use fixture::{ShellProcess, photos};
use pr_core::model::InstanceMode;
use pr_l10n::localizer::built_in_english_localizer;
use pr_platform_win::{instance_scope::InstanceScope, ui_driver};

/// Runs the real exe against an owner that holds the lock but does not answer, and returns the UI Automation names of its message box (after
/// answering it with OK and checking that the exe leaves with exit code 0 without having opened an image).
fn run_against_a_silent_owner(mode: InstanceMode, config: &str, folder_arg: Option<&str>) -> Vec<String> {
    let (photos_dir, a, _) = photos();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let prefix = format!("PhotoReviewRsTest{stamp:032x}");
    let owner = InstanceScope::with_options(mode, |_| {}, None, &prefix, None, false).unwrap();
    let launch = folder_arg.map_or_else(
        || a.to_string_lossy().into_owned(),
        |_| photos_dir.path().to_string_lossy().into_owned(),
    );
    let owner_folder = folder_arg.map(|_| launch.as_str());
    assert!(
        owner.try_acquire(owner_folder).unwrap(),
        "this process becomes the owner"
    );
    owner.stop_listening();

    let mut second = ShellProcess::start_with_config(&[launch], Some(&prefix), config);
    second.wait_for("the message box", |p| {
        ui_driver::main_window_of_process(p.pid).is_some()
    });
    // The box is modal and blocks the process: it is still alive and has opened no image.
    assert_eq!(second.count_events("Assign"), 0);
    let dialog = ui_driver::main_window_of_process(second.pid).expect("the box is still open");
    let texts: Vec<String> = dialog_texts(dialog).split('|').map(str::to_owned).collect();
    assert!(ui_driver::post_task_dialog_button(dialog, 1), "IDOK");
    assert_eq!(second.wait_exit().code(), Some(0));
    assert_eq!(second.count_events("Assign"), 0);
    assert!(!second.root.path().join("data/window-placement.json").exists());
    owner.dispose();
    texts
}

#[test]
#[ignore = "native: a real exe shows the startup task dialog and needs a desktop"]
fn second_exe_whose_owner_does_not_answer_shows_the_already_running_box_and_exits_zero() {
    // The language is fixed so that the expected text does not depend on the Windows UI language of the machine.
    let texts = run_against_a_silent_owner(InstanceMode::SingleWindow, r#"{"UiLanguage":"en"}"#, None);
    let localizer = built_in_english_localizer();
    assert!(texts.contains(&localizer.get("app.title")), "{texts:?}");
    assert!(
        texts.contains(&localizer.get("app.alreadyRunningNoResponse")),
        "{texts:?}"
    );
    assert!(
        !texts.contains(&localizer.get("folder.alreadyOpenInOtherInstance")),
        "{texts:?}"
    );
}

#[test]
#[ignore = "native: a real exe shows the startup task dialog and needs a desktop"]
fn second_exe_in_per_folder_mode_whose_owner_does_not_answer_says_the_folder_is_already_open() {
    let texts = run_against_a_silent_owner(
        InstanceMode::PerFolder,
        r#"{"UiLanguage":"en","InstanceMode":"PerFolder"}"#,
        Some("folder"),
    );
    let localizer = built_in_english_localizer();
    assert!(texts.contains(&localizer.get("app.title")), "{texts:?}");
    assert!(
        texts.contains(&localizer.get("folder.alreadyOpenInOtherInstance")),
        "{texts:?}"
    );
    assert!(
        !texts.contains(&localizer.get("app.alreadyRunningNoResponse")),
        "{texts:?}"
    );
}

/// Every UI Automation name below the dialog window (title, instruction, content, buttons) joined with `|`. A TaskDialog draws its text itself
/// (DirectUI), so `GetWindowText` cannot read it; this runs a short PowerShell script that does not touch the user's input.
fn dialog_texts(dialog: isize) -> String {
    let script = format!(
        "Add-Type -AssemblyName UIAutomationClient; $e=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]{dialog});          $all=$e.FindAll([System.Windows.Automation.TreeScope]::Subtree,[System.Windows.Automation.Condition]::TrueCondition);          (@($e.Current.Name)+@($all | ForEach-Object {{ $_.Current.Name }}) | Where-Object {{ $_ }}) -join '|'"
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .expect("start powershell.exe");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
