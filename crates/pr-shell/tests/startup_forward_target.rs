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
use startup_fixture::on_ui;
use std::{cell::RefCell, rc::Rc};

use pr_app::startup::forwarded_open::ForwardedOpenTarget;
use pr_shell::startup::forward_target::MainWindowForwardTarget;

fn assert_gone(target: &MainWindowForwardTarget) {
    assert!(target.is_closing_or_closed());
    assert_eq!(
        target.is_minimized().unwrap_err().message,
        "The main window is already destroyed."
    );
    assert_eq!(
        target.restore().unwrap_err().message,
        "The main window is already destroyed."
    );
    assert_eq!(
        target.activate().unwrap_err().message,
        "The main window is already destroyed."
    );
    assert_eq!(
        target.open_path("unused").unwrap_err().to_string(),
        "The main window is already destroyed."
    );
}

#[test]
fn forward_target_without_a_window_is_closing_and_fails_the_other_calls() {
    // new: W8-17e missing-window contract, no C# twin.
    let slot = Rc::new(RefCell::new(None));
    let target = MainWindowForwardTarget::new(Rc::downgrade(&slot));
    assert_gone(&target);
    drop(slot);
    assert_gone(&target);
}

#[test]
#[ignore = "native: needs desktop; hidden MainWindow"]
fn forward_target_clones_the_window_out_of_the_slot_before_calling_it() {
    // new: a view-model change callback empties the slot during OpenPathAsync.
    on_ui(|| {
        startup_part_a_fixture::with_window(|main, slot, _dialog| {
            use pr_platform_win::host::main_window::core::MainWindowFeature;
            struct EmptySlot(std::rc::Weak<RefCell<Option<Rc<pr_platform_win::host::main_window::MainWindow>>>>);
            impl MainWindowFeature for EmptySlot {
                fn name(&self) -> &'static str {
                    "empty-slot"
                }
                fn as_any(&self) -> &dyn std::any::Any {
                    self
                }
                fn on_changes(&self, _: &pr_app::viewmodels::main::ChangeSet) {
                    if let Some(slot) = self.0.upgrade() {
                        slot.borrow_mut().take();
                    }
                }
            }
            let target = MainWindowForwardTarget::new(Rc::downgrade(slot));
            assert!(!target.is_closing_or_closed());
            assert!(!target.is_minimized().unwrap());
            main.add_feature(Rc::new(EmptySlot(Rc::downgrade(slot))));
            let root = pr_test_support::TempRoot::new("forward-target");
            target.open_path(&root.path().to_string_lossy()).unwrap();
            assert!(slot.borrow().is_none());
            assert_gone(&target);
        })
    });
}

#[test]
#[ignore = "native: needs desktop; hidden MainWindow"]
fn activate_destroyed_window_returns_window_destroyed_message() {
    // C# OnForwardedOpen: ObjectDisposedException maps to WindowGone.
    on_ui(|| {
        startup_part_a_fixture::with_window(|main, slot, _dialog| {
            let target = MainWindowForwardTarget::new(Rc::downgrade(slot));
            main.dispose();
            assert!(target.is_closing_or_closed());
            assert_eq!(
                target.activate().unwrap_err().message,
                "The main window is already destroyed."
            );
        })
    });
}
