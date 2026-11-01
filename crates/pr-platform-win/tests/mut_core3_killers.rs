//! cl-mut-core3 (2026-11-01): killers for MISSED mutants of `host/main_window/core.rs` that the existing binaries did not pin: the `Debug` impls of
//! the public option/poster types and `UiLocalPoster::post_render`. Hidden, never shown windows; every wait is bounded, no sleeps.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/ui_thread.rs"]
mod ui_thread;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}

use std::rc::Rc;
use std::sync::mpsc::channel;

use main_window_fixture::{BuildOptions, build, dispatcher, finish, run, start_ui};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::main_window::core::{FeatureOptions, MainWindowOptions, UiLocalPoster};
use pr_platform_win::shell_viewing::viewport_controller::RenderPoster;

// core.rs:94 `Debug for FeatureOptions -> Ok(Default::default())`: the text names the type and counts the stored entries.
#[test]
fn feature_options_debug_names_the_type_and_counts_entries() {
    let mut options = FeatureOptions::default();
    assert!(format!("{options:?}").contains("entries: 0"));
    options.insert(7_u32);
    let text = format!("{options:?}");
    assert!(text.contains("FeatureOptions"), "{text}");
    assert!(text.contains("entries: 1"), "{text}");
}

// core.rs:145 `Debug for MainWindowOptions -> Ok(Default::default())`: the text names the type and its three plain fields.
#[test]
fn main_window_options_debug_names_the_type_and_its_fields() {
    let options = MainWindowOptions {
        prefer_warp: true,
        activate: false,
        ..MainWindowOptions::default()
    };
    let text = format!("{options:?}");
    assert!(text.contains("MainWindowOptions"), "{text}");
    assert!(text.contains("prefer_warp: true"), "{text}");
    assert!(text.contains("activate: false"), "{text}");
    assert!(text.contains("placement_file"), "{text}");
}

// core.rs:522 `Debug for UiLocalPoster -> Ok(Default::default())`.
#[test]
#[ignore = "native: needs desktop"]
fn ui_local_poster_debug_names_the_type() {
    let ui = start_ui();
    let text = run(&ui, || format!("{:?}", UiLocalPoster::new(dispatcher())));
    assert!(text.contains("UiLocalPoster"), "{text}");
    finish(ui);
}

// core.rs:563 `RenderPoster::post_render -> ()`: the job posted through the render seam really runs on the UI thread, not inline.
#[test]
#[ignore = "native: needs desktop"]
fn post_render_runs_the_job_later_on_the_ui_thread() {
    let ui = start_ui();
    let (tx, rx) = channel::<std::thread::ThreadId>();
    let ui_id = ui.thread_id();
    let ran_inline = run(&ui, move || {
        let poster = UiLocalPoster::new(dispatcher());
        let ran = Rc::new(std::cell::Cell::new(false));
        let flag = Rc::clone(&ran);
        poster.post_render(Box::new(move || {
            flag.set(true);
            tx.send(std::thread::current().id()).unwrap();
        }));
        ran.get()
    });
    assert!(!ran_inline, "post_render never runs the job inline");
    assert_eq!(rx.recv_timeout(ui_thread::BOUND).expect("the render job runs"), ui_id);
    finish(ui);
}

// core.rs:1097 `Debug for MainWindow -> Ok(Default::default())`: the text names the type and the window id.
// core.rs:1142 `MainWindow::hwnd -> Default::default()`: the handle is the real, non-zero handle of the native shell window.
#[test]
#[ignore = "native: needs desktop"]
fn main_window_debug_names_the_id_and_hwnd_is_the_shell_handle() {
    let ui = start_ui();
    let (text, id, hwnd, shell_hwnd) = run(&ui, || {
        let b = build(BuildOptions::default());
        (
            format!("{:?}", b.window),
            b.window.id(),
            b.window.hwnd(),
            b.shell.hwnd(),
        )
    });
    assert!(text.contains("MainWindow"), "{text}");
    assert!(text.contains(&id.to_string()), "{text}");
    assert_ne!(hwnd, 0, "a live window has a handle");
    assert_eq!(
        hwnd, shell_hwnd,
        "the main window reports the handle of its shell window"
    );
    finish(ui);
}
