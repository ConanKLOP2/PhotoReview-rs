//! Mutation killers of lane cl-input2 for `shell_viewing/composer.rs`, `shell_input/dispatch.rs`, `shell_input/main_window.rs` and
//! `shell_menus/main_window.rs`. Pure logic on fakes: no window, no GUI, no sleeps. Each test names the mutant (file:line, operator) it kills.
//! Evidence: docs/reviews/CL-INPUT2-2026-10-30.md.
#![cfg(windows)]

#[path = "common/viewing_composer_fixture.rs"]
mod composer_fixture;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

// ---- shell_viewing/composer.rs ----

// composer.rs:276:9 `<Debug for PresentationComposer>::fmt -> Ok(Default::default())` writes nothing.
#[test]
fn composer_debug_names_the_type() {
    let rig = composer_fixture::Rig::new();
    let text = format!("{:?}", rig.composer);
    assert!(text.starts_with("PresentationComposer"), "{text}");
}
