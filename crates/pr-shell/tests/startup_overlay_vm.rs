#![cfg(windows)]
#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/startup_fixture.rs"]
mod startup_fixture;
#[path = "common/startup_part_a_fixture.rs"]
mod startup_part_a_fixture;
use pr_core::abstractions::NullLog;
use startup_fixture::on_ui;
use std::{cell::RefCell, rc::Rc, sync::Arc};

use pr_platform_win::shell_overlay::main_window::OverlayViewModel;
use pr_shell::startup::overlay_vm::MainWindowOverlayViewModel;
use std::sync::atomic::Ordering;
use support::main_env::MainEnv;

fn no_window_commands(vm: &MainWindowOverlayViewModel) {
    vm.refresh_localized_text();
    vm.report_action_failed("Open", "failed");
    vm.pick_and_open_folder();
    vm.show_settings();
    vm.show_skipped_files();
    vm.show_recovery();
    vm.show_diagnostics();
    vm.show_benchmark();
    vm.clear_cache();
    vm.remove_duplicates(false);
    vm.remove_duplicates(true);
}

#[test]
fn overlay_vm_commands_without_a_window_do_not_panic() {
    // new: W8-17e missing-window contract, no C# twin.
    let env = MainEnv::new();
    let view_model = Rc::new(RefCell::new(env.vm));
    let slot = Rc::new(RefCell::new(None));
    let vm = MainWindowOverlayViewModel::new(view_model.clone(), &slot, Arc::new(NullLog));
    let status = vm.overlay_model().status_text;
    no_window_commands(&vm);
    drop(slot);
    no_window_commands(&vm);
    assert_eq!(vm.overlay_model().status_text, status);
    assert_eq!(env.dialog.recovery_shown.load(Ordering::SeqCst), 0);
    assert!(env.dialog.settings_targets.lock().unwrap().is_empty());
    let settings = Arc::new(pr_core::settings::app_settings::AppSettings::default());
    vm.set_settings(settings.clone());
    assert!(Arc::ptr_eq(&vm.settings(), &settings));
}

#[test]
fn overlay_vm_reads_while_a_modal_command_holds_the_view_model_return_the_last_values() {
    // new: exe walkthrough 2026-10-29 (no C# twin). A modal command runs inside `update_view_model` with the view model borrowed mutably; its nested
    // message loop runs the overlay's posted refresh, which reads the model and the settings. Before the fix both reads panicked with "already mutably borrowed".
    let env = MainEnv::new();
    let view_model = Rc::new(RefCell::new(env.vm));
    let slot = Rc::new(RefCell::new(None));
    let vm = MainWindowOverlayViewModel::new(view_model.clone(), &slot, Arc::new(NullLog));
    let model = vm.overlay_model();
    let settings = vm.settings();
    let _modal = view_model.borrow_mut();
    assert_eq!(vm.overlay_model().status_text, model.status_text);
    assert!(Arc::ptr_eq(&vm.settings(), &settings));
}

#[test]
fn overlay_vm_writes_while_a_modal_command_holds_the_view_model_are_applied_afterwards() {
    // new: exe walkthrough 2 2026-10-29 (no C# twin). Apply in Settings changes the language and the settings of the main window behind the modal dialog:
    // the overlay's posted refresh used to panic with "already borrowed" (app.log `Unhandled panic`) and the main window kept the old text.
    let env = MainEnv::new();
    let view_model = Rc::new(RefCell::new(env.vm));
    let slot = Rc::new(RefCell::new(None));
    let vm = MainWindowOverlayViewModel::new(view_model.clone(), &slot, Arc::new(NullLog));
    let original = vm.settings();
    let changed = Arc::new(pr_core::settings::app_settings::AppSettings {
        ui_language: "zz".to_owned(),
        ..pr_core::settings::app_settings::AppSettings::default()
    });
    assert_ne!(original.ui_language, changed.ui_language);
    let modal = view_model.borrow_mut();
    vm.set_settings(changed.clone());
    vm.refresh_localized_text();
    vm.apply_deferred(); // still busy: nothing happens, nothing panics
    drop(modal);
    assert_eq!(
        view_model.borrow().settings().ui_language,
        original.ui_language,
        "not applied before the modal command ends"
    );
    vm.apply_deferred();
    assert_eq!(view_model.borrow().settings().ui_language, "zz");
}

#[test]
#[ignore = "native: needs desktop; hidden MainWindow"]
fn overlay_vm_read_only_dialogs_allow_a_shared_view_model_borrow() {
    // C# ViewModelOverlayCommands: four read-only dialogs; D-W8B6-27 shared-borrow guard.
    on_ui(|| {
        startup_part_a_fixture::with_window(|main, slot, dialog| {
            let vm = MainWindowOverlayViewModel::new(main.view_model().clone(), slot, Arc::new(NullLog));
            let shared = main.view_model().borrow();
            vm.show_skipped_files();
            vm.show_recovery();
            vm.show_diagnostics();
            vm.show_benchmark();
            assert_eq!(vm.overlay_model().status_text, shared.overlay_model().status_text);
            assert_eq!(dialog.recovery_shown.load(Ordering::SeqCst), 1);
            assert_eq!(dialog.diagnostics_shown.load(Ordering::SeqCst), 1);
            assert_eq!(dialog.skipped_shown.lock().unwrap().len(), 1);
            assert_eq!(dialog.benchmark_folders.lock().unwrap().len(), 1);
        })
    });
}

#[test]
#[ignore = "native: needs desktop; hidden MainWindow"]
fn overlay_vm_read_only_dialogs_are_ignored_while_the_view_model_is_borrowed_mutably() {
    // review L6: used to panic with "already borrowed" when a modal command held the view model.
    on_ui(|| {
        startup_part_a_fixture::with_window(|main, slot, dialog| {
            let vm = MainWindowOverlayViewModel::new(main.view_model().clone(), slot, Arc::new(NullLog));
            let modal = main.view_model().borrow_mut();
            vm.show_skipped_files();
            vm.show_recovery();
            vm.show_diagnostics();
            vm.show_benchmark();
            drop(modal);
            assert_eq!(dialog.recovery_shown.load(Ordering::SeqCst), 0);
            assert_eq!(dialog.diagnostics_shown.load(Ordering::SeqCst), 0);
            assert!(dialog.skipped_shown.lock().unwrap().is_empty());
            assert!(dialog.benchmark_folders.lock().unwrap().is_empty());
        })
    });
}

#[test]
#[ignore = "native: needs desktop; hidden MainWindow"]
fn settings_dialog_command_does_not_panic_the_paint() {
    // C# ViewModelOverlayCommands.ShowSettings; contract T7 reentrant paint guard.
    on_ui(|| {
        startup_part_a_fixture::with_window(|main, slot, dialog| {
            let vm = MainWindowOverlayViewModel::new(main.view_model().clone(), slot, Arc::new(NullLog));
            thread_local! {
                static PAINT_WINDOW: RefCell<std::rc::Weak<pr_platform_win::host::main_window::MainWindow>> = const { RefCell::new(std::rc::Weak::new()) };
            }
            main.attach_renderer(None).unwrap();
            PAINT_WINDOW.with(|slot| *slot.borrow_mut() = Rc::downgrade(main));
            let painted = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let callback_painted = painted.clone();
            *dialog.on_settings.lock().unwrap() = Some(Box::new(move |_| {
                PAINT_WINDOW.with(|slot| {
                    let window = slot.borrow().upgrade().unwrap();
                    window.render_now();
                    window.pump_view_model();
                });
                callback_painted.store(true, Ordering::SeqCst);
                false
            }));
            vm.show_settings();
            assert!(painted.load(Ordering::SeqCst));
            PAINT_WINDOW.with(|slot| *slot.borrow_mut() = std::rc::Weak::new());
            vm.report_action_failed("Open", "injected");
            assert_eq!(
                vm.overlay_model().status_text,
                pr_l10n::tr::status_action_failed("Open", "injected".to_owned())
            );
        })
    });
}
