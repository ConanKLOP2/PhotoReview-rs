//! MUT3-HOST (2026-11-06): the `Debug` impls of `host/main_window/core.rs` (`FeatureOptions`, `MainWindowOptions`, `ViewportCallbacks`,
//! `MainWindowDeps`) were never read by a test, so `fmt -> Ok(Default::default())` (prints nothing) survived. `FeatureOptions` and `MainWindowOptions`
//! are plain values; the other two are captured by the fixture while it builds a real hidden window.
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

use std::path::PathBuf;

use main_window_fixture::{DEBUG_TEXT, build, finish, run, start_ui, take_built};
use pr_platform_win::host::main_window::core::{FeatureOptions, MainWindowOptions};

#[test]
fn feature_options_debug_names_the_type_and_counts_entries() {
    let mut options = FeatureOptions::default();
    assert_eq!(format!("{options:?}"), "FeatureOptions { entries: 0 }");
    options.insert(5_u32);
    assert_eq!(format!("{options:?}"), "FeatureOptions { entries: 1 }");
    options.insert(String::from("x"));
    assert_eq!(format!("{options:?}"), "FeatureOptions { entries: 2 }");
    assert_eq!(format!("{options:#?}"), "FeatureOptions {\n    entries: 2,\n}");
}

#[test]
fn main_window_options_debug_shows_the_three_plain_fields() {
    let defaults = MainWindowOptions::default();
    assert_eq!(
        format!("{defaults:?}"),
        "MainWindowOptions { prefer_warp: false, placement_file: None, activate: true, .. }"
    );
    let changed = MainWindowOptions {
        prefer_warp: true,
        placement_file: Some(PathBuf::from("p.json")),
        activate: false,
        ..MainWindowOptions::default()
    };
    assert_eq!(
        format!("{changed:?}"),
        "MainWindowOptions { prefer_warp: true, placement_file: Some(\"p.json\"), activate: false, .. }"
    );
}

#[test]
#[ignore = "native: needs desktop"]
fn deps_and_viewport_callbacks_debug_name_their_types() {
    let ui = start_ui();
    let texts = run(&ui, || {
        let built = build(Default::default());
        let texts = DEBUG_TEXT.with(|slot| slot.borrow().clone());
        drop(built);
        drop(take_built());
        texts
    });
    finish(ui);
    assert!(texts.contains(&"MainWindowDeps { .. }".to_owned()), "{texts:?}");
    assert!(texts.contains(&"ViewportCallbacks { .. }".to_owned()), "{texts:?}");
}
