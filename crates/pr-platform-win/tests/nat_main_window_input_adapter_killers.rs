//! Killers for the survivors of the whole-file native mutation run of `shell_input/main_window.rs` (lane F, 33 open identities) that sit in the PRODUCTION
//! adapter `MainWindowInputAdapter` (private): `ticks_per_ms`, `with_view_model` (the apply guard), `show_status`, `folder_open_failed_text`,
//! `action_failed_text`, `action_names`, `invalidate_layout_and_window`. The adapter is reached through the P-432 seam `MainWindowInput::services_for_test`
//! over a REAL hidden `MainWindow` (the same rig as `shell_input_e2e.rs`). The host side of the file is in `nat_main_window_input_host_killers.rs`.
//!
//! Every test is `#[ignore = "native: needs desktop"]`; every wait is bounded, no sleeps.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use main_window_fixture::{BuildOptions, build, built, finish, run, start_ui, wait_until};
use pr_app::viewmodels::main::ChangeSet;
use pr_platform_win::host::main_window::core::MainWindowFeature;
use pr_platform_win::shell_input::main_window::{MainWindowInputFeature, MainWindowInputServices};

fn services() -> Rc<dyn MainWindowInputServices> {
    built()
        .window
        .with_feature::<MainWindowInputFeature, _>(|feature| feature.input().services_for_test())
        .expect("the input feature is registered")
}

/// Counts the `ChangeSet`s the window applies (`apply_changes` hands every non-empty one to every feature).
struct CountingFeature {
    changes: Cell<u32>,
}

impl MainWindowFeature for CountingFeature {
    fn name(&self) -> &'static str {
        "nat-g-counter"
    }
    fn on_changes(&self, _changes: &ChangeSet) {
        self.changes.set(self.changes.get() + 1);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// main_window.rs 702:9 (`ticks_per_ms -> 0.0 / 1.0 / -1.0`) and 702:63 (`/` -> `%` / `*`): `Stopwatch.Frequency / 1000.0`, with the frequency of 10 MHz.
#[test]
#[ignore = "native: needs desktop"]
fn ticks_per_ms_is_the_stopwatch_frequency_over_one_thousand() {
    let ui = start_ui();
    run(&ui, || {
        build(BuildOptions::default());
        assert_eq!(10_000.0, services().ticks_per_ms());
        assert_eq!(
            pr_core::diagnostics::perf::STOPWATCH_FREQUENCY as f64 / 1000.0,
            services().ticks_per_ms()
        );
    });
    finish(ui);
}

// main_window.rs 725:9, 728:9, 731:9: the adapter's localized texts are the catalog's, with the error text and the action name in them.
#[test]
#[ignore = "native: needs desktop"]
fn the_adapter_texts_are_the_catalog_texts() {
    let ui = start_ui();
    run(&ui, || {
        build(BuildOptions::default());
        let services = services();
        let error = std::io::Error::other("disk on fire");

        let folder = services.folder_open_failed_text(&error);
        assert_eq!(
            pr_l10n::tr::status_folder_open_failed("disk on fire".to_owned()),
            folder
        );
        assert!(folder.contains("disk on fire"), "{folder:?}");

        let action = services.action_failed_text("Nat-G action", &error);
        assert_eq!(
            pr_l10n::tr::status_action_failed("Nat-G action", "disk on fire".to_owned()),
            action
        );
        assert!(
            action.contains("Nat-G action") && action.contains("disk on fire"),
            "{action:?}"
        );
        assert_ne!(folder, action);

        let names = services.action_names();
        use pr_l10n::tr;
        assert_eq!(tr::main_menu_next_folder(), names.next_folder);
        assert_eq!(tr::main_menu_previous_folder(), names.previous_folder);
        assert_eq!(tr::action_move_to_folder_name(), names.move_to_folder);
        assert_eq!(tr::action_copy_to_folder_name(), names.copy_to_folder);
        assert_eq!(
            tr::main_menu_open_folder().trim_end_matches(['\u{2026}', '.', ' ']),
            names.open_folder
        );
        assert_eq!(tr::main_menu_zoom(), names.zoom);
        for (what, name) in [
            ("next", &names.next_folder),
            ("previous", &names.previous_folder),
            ("move", &names.move_to_folder),
            ("copy", &names.copy_to_folder),
            ("open", &names.open_folder),
            ("zoom", &names.zoom),
        ] {
            assert!(!name.is_empty(), "{what}");
        }
    });
    finish(ui);
}

// main_window.rs 721:9 (`show_status -> ()`) and 714:16 (`delete !` of `with_view_model`): a status goes through the view model and is applied to the window; a query
// that changes nothing is not.
#[test]
#[ignore = "native: needs desktop"]
fn show_status_reaches_the_view_model_and_only_a_real_change_is_applied() {
    let ui = start_ui();
    run(&ui, || {
        build(BuildOptions::default());
        let counter = Rc::new(CountingFeature { changes: Cell::new(0) });
        built()
            .window
            .add_feature(Rc::clone(&counter) as Rc<dyn MainWindowFeature>);
        let services = services();

        services.show_status("nat-g status");
        assert_eq!("nat-g status", built().window.view_model().borrow().status_text());
        assert_eq!(1, counter.changes.get(), "the status change is applied to the window");

        services.show_status("nat-g status");
        assert_eq!(
            1,
            counter.changes.get(),
            "the same status is no change: nothing is applied"
        );

        let mut seen = None;
        services.with_view_model(&mut |vm| {
            seen = Some(vm.status_text());
            ChangeSet::default()
        });
        assert_eq!(
            Some("nat-g status".to_owned()),
            seen,
            "the closure ran on the view model"
        );
        assert_eq!(1, counter.changes.get(), "an empty ChangeSet is not applied");
    });
    finish(ui);
}

// main_window.rs 747:9 (`invalidate_layout_and_window -> ()`): the viewport layout is invalidated (and a frame asked for).
#[test]
#[ignore = "native: needs desktop"]
fn invalidate_layout_and_window_invalidates_the_viewport_layout() {
    let ui = start_ui();
    run(&ui, || {
        build(BuildOptions::default());
    });
    wait_until(&ui, "the first layout pass is done", || {
        !built().window.viewport().is_layout_posted()
    });
    run(&ui, || {
        services().invalidate_layout_and_window();
        assert!(
            built().window.viewport().is_layout_posted(),
            "the layout pass is posted again"
        );
    });
    finish(ui);
}
