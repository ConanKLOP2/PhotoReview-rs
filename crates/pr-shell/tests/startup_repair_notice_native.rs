//! Q5 (D-10 / C-28 / X-startup-settings-load): the repaired-settings notice reaches the PRODUCTION dialog service during a real startup.
//! Port of the C# `App.StartupCoreAsync` lines 388-391 (`SettingsLoadRepairText` -> information dialog, once; a valid config shows nothing).
//! Real exe-less shell rig: real `phase_two` step 6, real `TaskDialogIndirect` (comctl32 v6 activated by `task_dialogs`), no fake dialog service.
#![cfg(windows)]
#[path = "common/ui_smoke_fixture.rs"]
mod rig;
use pr_l10n::localizer::built_in_english_localizer;
use rig::{SmokeConfig, UiSmokeRun, isolated_test};

// C#: no direct counterpart test (the C# notice is covered by SettingsLoadRepairText unit tests); this is the Rust-side integration proof of step 6.
#[test]
#[ignore = "native: needs desktop and the startup information dialog"]
fn startup_with_a_repaired_config_shows_the_notice_once_through_the_production_dialog_service() {
    isolated_test(
        "startup_with_a_repaired_config_shows_the_notice_once_through_the_production_dialog_service",
        || {
            let run = UiSmokeRun::start(SmokeConfig {
                config_json: Some(r#"{"UiLanguage":"en","ImageCacheRamPercent":999}"#.into()),
                task_dialogs: true,
                ..Default::default()
            })
            .unwrap();
            let notice = run.wait_for_owned_window("repaired-settings notice").unwrap();
            let title = pr_platform_win::win32::dialog_native::user32::get_window_text(notice);
            assert_eq!(title, "Photo Review");
            // C-28 box text read back (ST-LOOP28): the content is the catalog sentence `settings.load.repaired` with the repaired field name filled in.
            let texts = dialog_texts(notice);
            let catalog = built_in_english_localizer().get("settings.load.repaired");
            let (head, tail) = catalog
                .split_once("{items}")
                .expect("the catalog text has an {items} slot");
            assert!(texts.contains(head), "{texts:?} lacks {head:?}");
            assert!(texts.contains("ImageCacheRamPercent"), "{texts:?}");
            assert!(texts.contains(tail), "{texts:?} lacks {tail:?}");
            // OK closes the information box; it must not come back.
            assert!(pr_platform_win::ui_driver::post_task_dialog_button(notice, 1));
            run.wait_no_owned_window().unwrap();
            run.wait_first_image().unwrap();
            run.on_ui(|_| ()).unwrap();
            run.wait_no_owned_window().expect("the notice must be shown once");
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

#[test]
#[ignore = "native: needs desktop and the startup information dialog"]
fn startup_with_a_valid_config_shows_no_repair_notice() {
    isolated_test("startup_with_a_valid_config_shows_no_repair_notice", || {
        let run = UiSmokeRun::start(SmokeConfig {
            config_json: Some(r#"{"ImageCacheRamPercent":50}"#.into()),
            task_dialogs: true,
            ..Default::default()
        })
        .unwrap();
        run.wait_first_image().unwrap();
        run.on_ui(|_| ()).unwrap();
        run.wait_no_owned_window().expect("a valid config shows no box");
        assert_eq!(run.close_and_wait(), Ok(0));
    });
}

/// Every UI Automation name below the dialog window (title, instruction, content, buttons) joined with `|`. A TaskDialog draws its text itself (DirectUI), so `GetWindowText` cannot read
/// it; same short PowerShell reader as `startup_unanswered_owner_native` (it does not touch the user's input).
fn dialog_texts(dialog: isize) -> String {
    let script = format!(
        "Add-Type -AssemblyName UIAutomationClient; $e=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]{dialog}); $all=$e.FindAll([System.Windows.Automation.TreeScope]::Subtree,[System.Windows.Automation.Condition]::TrueCondition); (@($e.Current.Name)+@($all | ForEach-Object {{ $_.Current.Name }}) | Where-Object {{ $_ }}) -join '|'"
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .expect("start powershell.exe");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
