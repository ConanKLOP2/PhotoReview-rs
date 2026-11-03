//! Q7 (PARITY row `X-w9r11-rename-dialog-e2e`, gap "the E2E drives a fixture window, not the real `ShellStartup` process"): F2 on the REAL shell (`UiSmokeRun`, the production graph
//! over a temp root and a temp-folder Recycle Bin), the real Win32 rename box over the real secondary-window port, the real file rename and the real view following it.
//! Port target: C# `ShellRenameEndToEndTests.F2AndTheMenuItem_...` (the user steps), now through `ShellStartup::run` instead of a `main_window_fixture` window.
//!
//! Native (`#[ignore = "native: needs desktop"]`). Every wait is bounded and signalled by the UI thread (no sleep). The name typed contains Vietnamese diacritics so the Unicode path
//! (`SetDlgItemTextW` -> `EN_CHANGE` -> policy -> `File.Move`) is exercised end to end; real IME composition still needs a person.
#![cfg(windows)]
use pr_platform_win::ui_driver;
use std::sync::Mutex;
static SERIAL: Mutex<()> = Mutex::new(());

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{SmokeConfig, UiSmokeRun, isolated_test};

/// `VK_F2` (the default shortcut of `RENAME_FILE`).
const VK_F2: u16 = 0x71;
/// Dialog item ids: the name box and the IDOK / IDCANCEL buttons of the rename dialog.
const EDIT_ID: i32 = 101;
const OK: u16 = 1;
const CANCEL: u16 = 2;

fn started() -> UiSmokeRun {
    let run = UiSmokeRun::start(SmokeConfig::default()).unwrap();
    run.wait_first_image().unwrap();
    run
}

/// C# `ShellRenameEndToEndTests.F2AndTheMenuItem_...` through the real shell: F2 opens the owned modal box, an invalid name is refused in place (OK disabled), Cancel changes nothing,
/// then a Vietnamese name is accepted and the real file, the catalog and the presented path follow; the shell closes with exit code 0.
#[test]
#[ignore = "native: needs desktop"]
fn f2_on_the_real_shell_refuses_invalid_cancel_changes_nothing_and_a_vietnamese_name_renames_the_real_file() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "f2_on_the_real_shell_refuses_invalid_cancel_changes_nothing_and_a_vietnamese_name_renames_the_real_file",
        || {
            let run = started();
            let first = run.photos()[0].clone();
            assert!(first.exists());
            let owner = run.on_ui(|p| p.hwnd).unwrap();

            // F2, an invalid name: refused in place, OK disabled, then Cancel: nothing changed.
            run.key(VK_F2).unwrap();
            let dialog = run.wait_for_owned_window("rename box").unwrap();
            assert!(
                !ui_driver::is_window_enabled(owner),
                "the box is modal over the main window"
            );
            assert!(
                ui_driver::is_dialog_item_enabled(dialog, i32::from(OK)),
                "the untouched stem is valid"
            );
            assert!(ui_driver::set_dialog_item_text(dialog, EDIT_ID, "a:b"));
            assert!(
                !ui_driver::is_dialog_item_enabled(dialog, i32::from(OK)),
                "an invalid name disables Rename"
            );
            assert!(ui_driver::post_command(dialog, CANCEL));
            run.wait_no_owned_window().unwrap();
            assert!(first.exists(), "Cancel leaves the file");
            assert_eq!(std::fs::read_dir(first.parent().unwrap()).unwrap().count(), 3);

            // F2 again, a Vietnamese name, OK.
            run.key(VK_F2).unwrap();
            let dialog = run.wait_for_owned_window("rename box again").unwrap();
            assert!(ui_driver::set_dialog_item_text(dialog, EDIT_ID, "Ảnh đẹp"));
            assert!(ui_driver::is_dialog_item_enabled(dialog, i32::from(OK)));
            assert!(ui_driver::post_command(dialog, OK));
            run.wait_no_owned_window().unwrap();

            let renamed = first.with_file_name("Ảnh đẹp.jpg");
            run.wait_ui_condition("the presented path follows the rename", |p| {
                p.view_model
                    .borrow()
                    .presenter()
                    .current_presented_path()
                    .is_some_and(|path| path.ends_with("Ảnh đẹp.jpg"))
            })
            .unwrap();
            assert!(renamed.exists(), "the real file carries the new name");
            assert!(!first.exists(), "the old name is gone");
            let names: Vec<String> = std::fs::read_dir(first.parent().unwrap())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert!(names.iter().any(|n| n == "Ảnh đẹp.jpg"), "{names:?}");
            assert_eq!(names.len(), 3, "{names:?}");
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}
