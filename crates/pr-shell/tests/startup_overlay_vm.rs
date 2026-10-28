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
