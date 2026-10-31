//! cl-mut-zoomcore (2026-10-31): native killers for the live-window cases of the `core.rs` UI-thread registry (`with_window`, `with_current_window`,
//! `find_live`, `newest_live`, `register_window`, `unregister_window`). They were MISSED in CL-MAINWIN because no seam reached a live `MainWindowInner`;
//! `MainWindow::{id, registered_id, current_registered_id}` are read-only probes. Hidden, never shown windows; every wait is bounded, no sleeps.
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

use main_window_fixture::{BuildOptions, build, finish, run, start_ui, take_built};
use pr_platform_win::host::main_window::core::MainWindow;

// `with_window` finds the live window by id and not another id (kills `-> None`, `-> Some(default)`, `find_live -> None`, `find_live ==` -> `!=`);
// `with_current_window` resolves to the live window (kills `-> None`, `delete !`, `newest_live -> None`).
#[test]
#[ignore = "native: needs desktop"]
fn registry_resolves_a_live_window_by_id_and_as_current() {
    let ui = start_ui();
    let (own, found, unknown, current) = run(&ui, || {
        let b = build(BuildOptions::default());
        let own = b.window.id();
        let unknown = own.wrapping_add(1_000_003);
        (
            own,
            MainWindow::registered_id(own),
            MainWindow::registered_id(unknown),
            MainWindow::current_registered_id(),
        )
    });
    assert_eq!(found, Some(own), "the live window is found under its own id");
    assert_eq!(unknown, None, "an id that was never registered finds nothing");
    assert_eq!(current, Some(own), "the only live window is the current window");
    finish(ui);
}

// `register_window` keeps live entries when the next window registers (kills `> -> <`, `> -> ==`), the newest live window is the current one, and a
// dropped window leaves the registry (`unregister_window` / dead weak) so the older one is current again.
#[test]
#[ignore = "native: needs desktop"]
fn registry_keeps_live_windows_and_follows_the_newest() {
    let ui = start_ui();
    let (first_after_second, second_after_second, current_two, first_after_drop, second_after_drop, current_one) =
        run(&ui, || {
            let first = build(BuildOptions::default());
            let first_id = first.window.id();
            let second = build(BuildOptions::default());
            let second_id = second.window.id();
            assert_ne!(first_id, second_id);
            let two = (
                MainWindow::registered_id(first_id),
                MainWindow::registered_id(second_id),
                MainWindow::current_registered_id() == Some(second_id),
            );
            drop(second); // the thread-local still holds the other `Rc<Built>`
            drop(take_built());
            let one = (
                MainWindow::registered_id(first_id).is_some(),
                MainWindow::registered_id(second_id),
                MainWindow::current_registered_id() == Some(first_id),
            );
            drop(first);
            assert_eq!(
                MainWindow::registered_id(first_id),
                None,
                "a dropped window is gone from the registry"
            );
            (
                two.0 == Some(first_id),
                two.1 == Some(second_id),
                two.2,
                one.0,
                one.1,
                one.2,
            )
        });
    assert!(
        first_after_second,
        "registering a second window must not drop the first"
    );
    assert!(second_after_second);
    assert!(current_two, "the newest live window is the current one");
    assert!(
        first_after_drop,
        "the first window is still live after the second one ended"
    );
    assert_eq!(second_after_drop, None, "the dropped window left the registry");
    assert!(current_one, "the older window is current again");
    finish(ui);
}
