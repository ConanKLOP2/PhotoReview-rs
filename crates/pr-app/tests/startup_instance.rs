//! W8-17d: single-instance decisions, no kernel objects.
use pr_app::startup::instance::*;
use pr_core::model::InstanceMode;
use pr_l10n::localizer::built_in_english_localizer;

#[test]
fn second_launch_delivered_to_the_owner_exits_silently() {
    // C#: AppStartupBranchTests.StartupCore_SecondLaunchDeliveredToTheOwner_ForwardsTheExistingPathsAndExitsSilently.
    for mode in [InstanceMode::SingleWindow, InstanceMode::PerFolder] {
        assert_eq!(handoff_decision(true, mode), HandoffDecision::ExitSilently);
    }
}

#[test]
fn second_launch_without_answer_uses_the_mode_specific_message() {
    // C#: AppStartupBranchTests.StartupCore_SecondLaunchWithoutAnswer_ShowsTheNoResponseMessageForTheSingleWindowMode.
    // C#: AppStartupBranchTests.StartupCore_SecondLaunchWithoutAnswer_ShowsTheFolderOpenMessageForThePerFolderMode.
    assert_eq!(
        handoff_decision(false, InstanceMode::SingleWindow),
        HandoffDecision::ExitWithMessage(InstanceMessageKind::AppAlreadyRunningNoResponse)
    );
    assert_eq!(
        handoff_decision(false, InstanceMode::PerFolder),
        HandoffDecision::ExitWithMessage(InstanceMessageKind::FolderAlreadyOpen)
    );
    let loc = built_in_english_localizer();
    assert_eq!(startup_box_title(&loc), loc.get("app.title"));
    for (kind, key) in [
        (
            InstanceMessageKind::FolderAlreadyOpen,
            "folder.alreadyOpenInOtherInstance",
        ),
        (
            InstanceMessageKind::AppAlreadyRunningNoResponse,
            "app.alreadyRunningNoResponse",
        ),
    ] {
        assert_eq!(instance_message_text(kind, &loc), loc.get(key));
    }
}

#[test]
fn taking_the_lock_preserves_folder_result_and_error_without_forwarding() {
    // New: pure instance seam used by ShellStartup.StartupCoreAsync before the handoff worker.
    struct Gate(std::sync::Mutex<Vec<Option<String>>>, Result<bool, InstanceStartError>);
    impl InstanceGate for Gate {
        fn mode(&self) -> InstanceMode {
            InstanceMode::PerFolder
        }
        fn try_acquire(&self, folder: Option<&str>) -> Result<bool, InstanceStartError> {
            self.0.lock().unwrap().push(folder.map(str::to_owned));
            self.1.clone()
        }
        fn forward_to_owner(&self, _: Option<&str>, _: &[String]) -> Result<bool, InstanceStartError> {
            panic!("lock helper must not forward on the UI thread")
        }
    }
    for outcome in [
        Ok(true),
        Ok(false),
        Err(InstanceStartError {
            message: "lock failure".into(),
        }),
    ] {
        let gate = Gate(std::sync::Mutex::new(vec![]), outcome.clone());
        assert_eq!(try_take_lock(&gate, Some("folder")), outcome);
        assert_eq!(*gate.0.lock().unwrap(), vec![Some("folder".into())]);
    }
}
