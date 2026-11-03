//! Review-confirm tests of group G8 (`docs/reviews/G7-G15-2026-11-03.md`): the `OpenInDefaultEditor` arm of `MainWindowInputHost::execute` is wired (W9-E9), not a
//! `todo!()`. These tests pin the CURRENT, correct behaviour on the same real view model rig as `shell_input_surface.rs`.
//! New (Rust only): the C# `ShellMainWindow.Input.cs` arm calls `_viewModel.OpenInExternalEditor()`.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/shell_input_dispatch_fixture.rs"]
mod fixture;
#[path = "common/input_feature_fixture.rs"]
mod input_feature_fixture;
#[allow(dead_code)]
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;

use input_feature_fixture::FeatureRig;
use pr_app::input::review_command::ReviewCommand;
use pr_core::abstractions::SettingsTarget;

fn rig_with_one_photo() -> FeatureRig {
    let mut env = support::main_env::MainEnv::new();
    let (folder, _) = env.images("rc-g8-shots", &["a.jpg"]);
    env.open(&folder);
    FeatureRig::with_env(env)
}

#[test]
fn open_in_default_editor_with_no_editor_declared_opens_settings_at_the_editor_field_and_does_not_panic() {
    // C#: MainViewModel.OpenInExternalEditor (no declared editor opens Settings at the External Editor field); the input arm returns Ok.
    let rig = rig_with_one_photo();
    let host = rig.input.input_host();

    let result = host.execute(ReviewCommand::OPEN_IN_DEFAULT_EDITOR);
    rig.settle();

    assert!(result.is_ok(), "the arm must not fail without an editor");
    let dialog = rig.env.borrow().dialog.clone();
    let targets = dialog.settings_targets.lock().expect("targets").clone();
    assert_eq!(targets, vec![SettingsTarget::ExternalEditor]);
    assert!(
        rig.statuses().is_empty(),
        "no action-failed status for a normal outcome"
    );
}
