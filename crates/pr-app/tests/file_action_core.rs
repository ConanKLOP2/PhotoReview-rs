//! W8-07d: `run_action`, the file-action core after the I/O (`execute_core`, `on_executed`, the parked tails), late completion,
//! ordering and the unexpected-throw paths (contract `contracts/W8-07d.md`; C# `FileActionController.cs` lines 68 to 116, 278 to 617).
//!
//! The C# tests ran on a temp folder with a fake Recycle Bin; here the executor is a fake whose results are scripted
//! (`FakeExecutor::script`), the worker is a manual queue and the presenter is a test that calls `on_present_finished` when it says
//! so (this replaces the C# `HoldPresenter` / `ReleasePresenter`). Nothing touches a disk or the real Recycle Bin.
//!
//! Not ported here (see the report): the undo tests (`UndoLast_*`, `UndoGroup*`, move positions) need `restores_outside_folder`,
//! `reload_path_after_undo` and `is_in_folder` of W8-07e, which are still `todo!()`; the C# tests that mutate the catalog from inside
//! an open dialog cannot be written (the catalog is `&mut` for the whole call); `RunAction_UndefinedOperation_*` is N/A (the Rust
//! `FileOperationType` is a closed enum, contract O1); the real-disk twins (growing file, bin that cannot hold a capture, unreadable
//! member state) belong to `pr_core::file_actions::service`, which the executor fake replaces.

mod file_action_fakes;

use file_action_fakes::{Env, ExecutorCall, Scripted, action, arg, en, p, text};
use pr_app::coordinators::file_actions::recycle::{ExecOutcome, FileActionDone, FileActionEvent, FileActionStep};
use pr_app::coordinators::folder_load::state::WorkerLane;
use pr_core::file_actions::journal_line::{JournalDateTime, JournalEntry, JournalGroupMember};
use pr_core::file_actions::types::{
    CaptureGroupActionRequest, CaptureGroupActionResult, CaptureGroupMemberResult, FileActionRequest, FileActionResult,
};
use pr_core::model::{FileOperationType, JournalState};
use pr_core::settings::app_settings::AppSettings;
use pr_core::time::DateTimeUtc;
use pr_l10n::template::LocArg;

use FileOperationType::{Copy, Move, Recycle};

const DONE_TRUE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: true });
const DONE_FALSE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: false });

// ---------------------------------------------------------------------------------------------------------------
// builders
// ---------------------------------------------------------------------------------------------------------------

fn single_ok(op: FileOperationType, source: &str, destination: Option<&str>) -> FileActionResult {
    FileActionResult::new(
        true,
        op,
        source,
        destination.map(str::to_string),
        4,
        DateTimeUtc::MIN,
        None,
    )
}

fn single_fail(op: FileOperationType, source: &str, error: &str) -> FileActionResult {
    FileActionResult::new(false, op, source, None, 4, DateTimeUtc::MIN, Some(error.to_string()))
}

fn member(source: &str, destination: Option<&str>, completed: bool, permanent: bool) -> CaptureGroupMemberResult {
    let stamp = JournalDateTime::utc(DateTimeUtc::MIN);
    let mut result = CaptureGroupMemberResult::new(
        JournalGroupMember {
            source: source.to_string(),
            destination: destination.map(str::to_string),
            size: 4,
            last_write_utc: stamp,
            permanent,
        },
        completed,
        false,
    );
    result.source_exists = !completed;
    result
}

fn entry(op: FileOperationType, destination: Option<&str>) -> Box<JournalEntry> {
    let stamp = JournalDateTime::utc(DateTimeUtc::MIN);
    Box::new(JournalEntry {
        id: "g".to_string(),
        op_type: op,
        state: JournalState::Committed,
        source: p("pair.jpg"),
        destination: destination.map(str::to_string),
        size: 4,
        last_write_utc: stamp,
        timestamp_utc: stamp,
        error: None,
        error_code: None,
        permanent: None,
        undo: None,
        group_id: Some("g".to_string()),
        group_members: None,
    })
}

fn group_ok(
    op: FileOperationType,
    members: Vec<CaptureGroupMemberResult>,
    destination: Option<&str>,
) -> CaptureGroupActionResult {
    CaptureGroupActionResult::new(true, false, op, "g", Some(entry(op, destination)), members, None)
}

fn group_failed(
    op: FileOperationType,
    members: Vec<CaptureGroupMemberResult>,
    error: &str,
) -> CaptureGroupActionResult {
    CaptureGroupActionResult::new(false, false, op, "g", None, members, Some(error.to_string()))
}

fn move_env(names: &[&str]) -> Env {
    let mut env = Env::new(AppSettings::default());
    env.load(names);
    env.set_actions(
        AppSettings::default(),
        vec![
            action("MoveToSub", Move, "Sorted", false),
            action("DeleteIt", Recycle, "", false),
            action("CopyIt", Copy, "Copies", false),
        ],
    );
    env
}

fn pair_env(settings: AppSettings, confirm_actions: bool) -> Env {
    let mut env = Env::new(settings.clone());
    env.load_pair_between_two_photos(false);
    env.set_actions(
        settings,
        vec![
            action("MoveToSub", Move, "Sorted", confirm_actions),
            action("DeleteIt", Recycle, "", confirm_actions),
        ],
    );
    env
}

fn singles(env: &Env) -> Vec<FileActionRequest> {
    env.exec
        .recorded()
        .into_iter()
        .filter_map(|c| match c {
            ExecutorCall::Single(r) => Some(r),
            ExecutorCall::Group(_) => None,
        })
        .collect()
}

fn groups(env: &Env) -> Vec<CaptureGroupActionRequest> {
    env.exec
        .recorded()
        .into_iter()
        .filter_map(|c| match c {
            ExecutorCall::Group(r) => Some(r),
            ExecutorCall::Single(_) => None,
        })
        .collect()
}

fn undo_history_len(env: &Env) -> usize {
    env.undo.as_ref().map_or(0, |u| u.lock().unwrap().move_history().len())
}

fn has_undo_action(env: &Env) -> bool {
    env.undo
        .as_ref()
        .is_some_and(|u| u.lock().unwrap().has_last_undo_action())
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerConfirmGapTests (RunAction_*)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerConfirmGapTests.RunAction_MoveWithCompareSelectionDifferentFromCurrent_MovesTheCompareSelectionOnly
#[test]
fn move_with_compare_selection_different_from_current_moves_the_compare_selection_only() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);

    assert_eq!(
        env.run_action(0, Some(&p("b.jpg")), Some(&p("a.jpg"))),
        FileActionStep::Pending
    );
    assert_eq!(env.run_worker(), DONE_TRUE);

    let calls = singles(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].source, p("b.jpg"));
    assert_eq!(calls[0].operation, Move);
    assert_eq!(calls[0].destination.as_deref(), Some("Sorted"));
    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
}

// C#: FileActionControllerConfirmGapTests.RunAction_MoveWithOnlyACompareSelection_StillMovesIt
#[test]
fn move_with_only_a_compare_selection_still_moves_it() {
    let mut env = move_env(&["b.jpg"]);

    assert_eq!(env.run_action(0, Some(&p("b.jpg")), None), FileActionStep::Pending);
    env.run_worker();

    assert_eq!(singles(&env)[0].source, p("b.jpg"));
    assert_eq!(env.catalog.count(), 0);
}

// C#: FileActionControllerConfirmGapTests.RunAction_RecycleConfirmWithCompareSelectionOnDriveWithoutBin_AsksOnlyThePermanentDeletePrompt
#[test]
fn recycle_confirm_with_compare_selection_on_drive_without_bin_asks_only_the_permanent_delete_prompt() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.set_actions(
        AppSettings {
            allow_permanent_delete_without_recycle_bin: true,
            ..AppSettings::default()
        },
        vec![action("DeleteIt", Recycle, "", true)],
    );
    env.exec.add_no_bin_for(&p("b.jpg"));

    assert_eq!(
        env.run_action(0, Some(&p("b.jpg")), Some(&p("a.jpg"))),
        FileActionStep::Pending
    );
    env.run_worker();

    let (title, _) = env.dialog.only_confirmation();
    assert_eq!(title, text("dialog.confirmPermanentDelete.title"));
    let calls = singles(&env);
    assert_eq!(calls[0].source, p("b.jpg"));
    assert!(calls[0].allow_permanent_delete);
    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
}

// C#: FileActionControllerConfirmGapTests.RunAction_RecycleConfirmWithCompareSelectionInCapture_AsksOnlyTheGroupPrompt
#[test]
fn recycle_confirm_with_compare_selection_in_capture_asks_only_the_group_prompt() {
    let mut env = Env::new(AppSettings::default());
    env.load(&["a.jpg", "pair.jpg", "pair.cr2"]);
    env.set_actions(AppSettings::default(), vec![action("DeleteIt", Recycle, "", true)]);

    assert_eq!(
        env.run_action(0, Some(&p("pair.jpg")), Some(&p("a.jpg"))),
        FileActionStep::Pending
    );
    env.run_worker();

    let (_, message) = env.dialog.only_confirmation();
    assert_eq!(
        message,
        en(
            "dialog.confirmCompareGroupRecycle.message",
            &[arg("fileName", "pair.jpg"), arg("files", "pair.jpg, pair.cr2")]
        )
    );
    assert_eq!(groups(&env).len(), 1);
    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
}

// C#: FileActionControllerConfirmGapTests.RunAction_RecycleConfirmGroupWithoutBinButPermanentDeleteOff_RefusesWithoutAnyPrompt
#[test]
fn recycle_confirm_group_without_bin_but_permanent_delete_off_refuses_without_any_prompt() {
    let mut env = Env::new(AppSettings::default());
    env.load(&["pair.jpg", "pair.cr2"]);
    env.set_actions(AppSettings::default(), vec![action("DeleteIt", Recycle, "", true)]);
    env.exec.set_no_bin(true);

    assert_eq!(env.run_action(0, None, Some(&p("pair.jpg"))), DONE_FALSE);

    assert_eq!(env.dialog.confirmation_count(), 0);
    assert_eq!(env.dialog.only_error().0, text("dialog.recycleNoBin.title"));
    assert_eq!(env.catalog.count(), 1);
    assert!(env.exec.recorded().is_empty());
}

// C#: FileActionControllerConfirmGapTests.RunAction_RecycleConfirmCaptureWithoutFileSystem_AsksTheGenericPromptAndChangesNothing
#[test]
fn recycle_confirm_capture_without_file_system_asks_the_generic_prompt_and_changes_nothing() {
    use file_action_fakes::EnvOptions;
    let settings = AppSettings {
        confirm_before_delete: true,
        ..AppSettings::default()
    };
    let mut env = Env::with_options(
        settings.clone(),
        EnvOptions {
            file_system: false,
            ..EnvOptions::default()
        },
    );
    env.load(&["pair.jpg", "pair.cr2"]);
    env.set_actions(settings, vec![action("DeleteIt", Recycle, "", true)]);

    assert_eq!(env.run_action(0, None, Some(&p("pair.jpg"))), DONE_FALSE);

    let (title, _) = env.dialog.only_confirmation();
    assert_eq!(title, text("dialog.confirmAction.title"));
    assert_eq!(env.catalog.count(), 1);
    assert!(env.exec.recorded().is_empty());
    assert_eq!(env.port.queued(), 0);
}

// C#: FileActionControllerConfirmGapTests.RunAction_MoveConfirmedWithoutDialogService_RunsWithoutPrompt
#[test]
fn move_confirmed_without_dialog_service_runs_without_prompt() {
    use file_action_fakes::EnvOptions;
    let mut env = Env::with_options(
        AppSettings::default(),
        EnvOptions {
            dialog: false,
            ..EnvOptions::default()
        },
    );
    env.load(&["a.jpg"]);
    env.set_actions(AppSettings::default(), vec![action("MoveToSub", Move, "Sorted", true)]);

    assert_eq!(env.run_action(0, None, Some(&p("a.jpg"))), FileActionStep::Pending);
    env.run_worker();

    assert_eq!(singles(&env).len(), 1);
}

// C#: FileActionControllerConfirmGapTests.RunAction_MoveWithoutConfirmFlagButDialogServicePresent_DoesNotAsk
#[test]
fn move_without_confirm_flag_but_dialog_service_present_does_not_ask() {
    let mut env = move_env(&["a.jpg"]);
    env.dialog_answer(false);

    assert_eq!(env.run_action(0, None, Some(&p("a.jpg"))), FileActionStep::Pending);
    env.run_worker();

    assert_eq!(env.dialog.confirmation_count(), 0);
    assert_eq!(singles(&env).len(), 1);
}

// C#: FileActionControllerConfirmGapTests.RunAction_MoveConfirmDeclined_LeavesFileAndCatalogUntouched
#[test]
fn move_confirm_declined_leaves_file_and_catalog_untouched() {
    let mut env = move_env(&["a.jpg"]);
    env.set_actions(AppSettings::default(), vec![action("MoveToSub", Move, "Sorted", true)]);
    env.dialog_answer(false);

    assert_eq!(env.run_action(0, None, Some(&p("a.jpg"))), DONE_FALSE);

    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
    assert_eq!(env.port.queued(), 0);
}

// C#: FileActionControllerConfirmGapTests.RunAction_ConfirmThenFolderSwitchedMeanwhile_DoesNotActOnTheFile
// (RunAction_ConfirmThenCompareSelectionRemovedFromCatalogMeanwhile_* cannot be written: the catalog is `&mut` during the call)
#[test]
fn confirm_then_folder_switched_meanwhile_does_not_act_on_the_file() {
    let mut env = move_env(&["a.jpg"]);
    env.set_actions(AppSettings::default(), vec![action("DeleteIt", Recycle, "", true)]);
    let clock = env.clock.clone();
    env.dialog.set_on_confirm(move || {
        clock.next_folder();
    });

    assert_eq!(env.run_action(0, None, Some(&p("a.jpg"))), DONE_FALSE);

    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
    assert_eq!(env.port.queued(), 0);
}

// C#: FileActionControllerConfirmGapTests.RunAction_IndexEqualToActionCount_DoesNothing
#[test]
fn index_equal_to_action_count_does_nothing() {
    let mut env = move_env(&["a.jpg"]);

    assert_eq!(env.run_action(3, None, Some(&p("a.jpg"))), DONE_FALSE);
    assert_eq!(env.run_action(-1, None, Some(&p("a.jpg"))), DONE_FALSE);

    assert!(env.sink.statuses.is_empty());
    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
    assert_eq!(env.port.queued(), 0);
}

// C#: FileActionControllerConfirmGapTests.RunAction_RecycleProfile_RecyclesTheCurrentPhotoWithoutADestination
#[test]
fn recycle_profile_recycles_the_current_photo_without_a_destination() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);

    assert_eq!(env.run_action(1, None, Some(&p("a.jpg"))), FileActionStep::Pending);
    env.run_worker();

    let calls = singles(&env);
    assert_eq!(calls[0].operation, Recycle);
    assert_eq!(calls[0].destination, None);
    assert!(
        !env.sink
            .statuses
            .contains(&en("status.actionNoDestination", &[arg("actionName", "DeleteIt")]))
    );
    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]);
}

// C#: FileActionControllerConfirmGapTests.RunAction_MoveProfileWithBlankDestination_ReportsNoDestinationAndKeepsTheFile
#[test]
fn move_profile_with_blank_destination_reports_no_destination_and_keeps_the_file() {
    let mut env = move_env(&["a.jpg"]);
    env.set_actions(AppSettings::default(), vec![action("NoDest", Move, "  ", false)]);

    assert_eq!(env.run_action(0, None, Some(&p("a.jpg"))), DONE_FALSE);

    assert_eq!(
        env.sink.statuses,
        vec![en("status.actionNoDestination", &[arg("actionName", "NoDest")])]
    );
    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
    assert_eq!(env.port.queued(), 0);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerGroupTests (profile prompts)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerGroupTests.GroupMove_FromCompareSelection_AsksAndNamesBothFiles_DeclinedMovesNothing
#[test]
fn group_move_from_compare_selection_asks_and_names_both_files_declined_moves_nothing() {
    let mut env = pair_env(AppSettings::default(), false);
    env.dialog_answer(false);
    let before = env.catalog_paths();

    assert_eq!(
        env.run_action(0, Some(&p("pair.cr2")), Some(&p("pair.jpg"))),
        DONE_FALSE
    );

    let (_, message) = env.dialog.only_confirmation();
    assert_eq!(
        message,
        en(
            "dialog.confirmCompareGroupMove.message",
            &[arg("fileName", "pair.cr2"), arg("files", "pair.jpg, pair.cr2")]
        )
    );
    assert_eq!(env.catalog_paths(), before);
    assert!(env.exec.recorded().is_empty());
}

// C#: FileActionControllerGroupTests.RecycleAction_WithConfirmAndConfirmBeforeDeleteOnAGroup_AsksOnlyTheGroupQuestion
#[test]
fn recycle_action_with_confirm_and_confirm_before_delete_on_a_group_asks_only_the_group_question() {
    let mut env = pair_env(
        AppSettings {
            confirm_before_delete: true,
            ..AppSettings::default()
        },
        true,
    );

    assert_eq!(env.run_action(1, None, Some(&p("pair.jpg"))), FileActionStep::Pending);
    env.run_worker();

    let (_, message) = env.dialog.only_confirmation();
    assert_eq!(
        message,
        en(
            "dialog.confirmGroupRecycle.message",
            &[LocArg::new("count", 2_i64), arg("fileName", "pair.jpg")]
        )
    );
    assert_eq!(groups(&env)[0].group.paths().len(), 2);
}

// C#: FileActionControllerGroupTests.MoveAction_WithConfirmFromCompareOnAGroup_AsksOnlyTheGroupQuestion
#[test]
fn move_action_with_confirm_from_compare_on_a_group_asks_only_the_group_question() {
    let mut env = pair_env(AppSettings::default(), true);

    assert_eq!(
        env.run_action(0, Some(&p("pair.cr2")), Some(&p("pair.jpg"))),
        FileActionStep::Pending
    );
    env.run_worker();

    let (_, message) = env.dialog.only_confirmation();
    assert_eq!(
        message,
        en(
            "dialog.confirmCompareGroupMove.message",
            &[arg("fileName", "pair.cr2"), arg("files", "pair.jpg, pair.cr2")]
        )
    );
    assert_eq!(groups(&env)[0].operation, Move);
}

// C#: FileActionControllerGroupTests.MoveAction_WithConfirmOnAGroupWithoutCompare_AsksTheGenericQuestionOnce
#[test]
fn move_action_with_confirm_on_a_group_without_compare_asks_the_generic_question_once() {
    let mut env = pair_env(AppSettings::default(), true);

    assert_eq!(env.run_action(0, None, Some(&p("pair.jpg"))), FileActionStep::Pending);

    let (_, message) = env.dialog.only_confirmation();
    assert_eq!(
        message,
        en("dialog.confirmAction.message", &[arg("actionName", "MoveToSub")])
    );
}

// C#: FileActionControllerGroupTests.RecycleAction_WithConfirmOnAGroupWhenNoGroupPromptApplies_AsksTheGenericQuestionOnce
#[test]
fn recycle_action_with_confirm_on_a_group_when_no_group_prompt_applies_asks_the_generic_question_once() {
    let mut env = pair_env(
        AppSettings {
            confirm_before_delete: false,
            ..AppSettings::default()
        },
        true,
    );

    assert_eq!(env.run_action(1, None, Some(&p("pair.jpg"))), FileActionStep::Pending);

    let (_, message) = env.dialog.only_confirmation();
    assert_eq!(
        message,
        en("dialog.confirmAction.message", &[arg("actionName", "DeleteIt")])
    );
}

// C#: FileActionControllerGroupTests.RecycleAction_WithConfirmOnAGroup_PermanentDeletePromptStaysSeparateAndSingle
#[test]
fn recycle_action_with_confirm_on_a_group_permanent_delete_prompt_stays_separate_and_single() {
    let mut env = Env::new(AppSettings::default());
    env.load_pair_between_two_photos(true);
    let settings = AppSettings {
        allow_permanent_delete_without_recycle_bin: true,
        ..AppSettings::default()
    };
    env.set_actions(
        settings,
        vec![
            action("MoveToSub", Move, "Sorted", true),
            action("DeleteIt", Recycle, "", true),
        ],
    );
    env.exec.set_no_bin(true);

    assert_eq!(env.run_action(1, None, Some(&p("pair.jpg"))), FileActionStep::Pending);
    env.run_worker();

    let (title, message) = env.dialog.only_confirmation();
    assert_eq!(title, text("dialog.confirmPermanentDelete.title"));
    // the argument ORDER (permanentCount, count, fileName) is pinned by the rendered text
    assert_eq!(
        message,
        en(
            "dialog.confirmGroupPermanentDelete.message",
            &[
                LocArg::new("permanentCount", 3_i64),
                LocArg::new("count", 3_i64),
                arg("fileName", "pair.jpg")
            ]
        )
    );
    assert!(groups(&env)[0].allow_permanent_delete);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerOutcomeGapTests / ordering
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerOutcomeGapTests.RunAction_MoveAfterMovingTheFirstOfTwo_SessionPathIsTheNewCurrentPhoto
#[test]
fn move_after_moving_the_first_of_two_session_path_is_the_new_current_photo() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_ok(Move, &p("a.jpg"), Some("Sorted/a.jpg"))));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();

    assert_eq!(env.sink.sessions, vec![p("b.jpg")]);
}

// C#: FileActionControllerOutcomeGapTests.RunAction_CopySingleFileWithOthersRemaining_StatusNamesTheCopyDestination
#[test]
fn copy_single_file_with_others_remaining_status_names_the_copy_destination() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_ok(Copy, &p("a.jpg"), Some("Copies/a.jpg"))));

    env.run_action(2, None, Some(&p("a.jpg")));
    assert_eq!(env.run_worker(), DONE_TRUE);

    assert_eq!(
        env.sink.statuses,
        vec![en("status.copiedTo", &[arg("fileName", "a.jpg")])]
    );
    assert_eq!(env.catalog.count(), 2);
}

// C#: FileActionControllerOutcomeGapTests.RunAction_CopyCapture_StatusNamesTheFirstCopiedFile
#[test]
fn copy_capture_status_names_the_first_copied_file() {
    let mut env = pair_env(AppSettings::default(), false);
    env.set_actions(AppSettings::default(), vec![action("CopyIt", Copy, "Copies", false)]);
    env.exec.script(Scripted::Group(group_ok(
        Copy,
        vec![
            member(&p("pair.jpg"), Some("Copies/pair.jpg"), true, false),
            member(&p("pair.cr2"), Some("Copies/pair.cr2"), true, false),
        ],
        Some("Copies/pair.jpg"),
    )));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();

    assert_eq!(
        env.sink.statuses,
        vec![en("status.copiedTo", &[arg("fileName", "pair.jpg")])]
    );
    assert_eq!(env.catalog.count(), 4 - 1); // a Copy leaves the catalog alone (the pair is one entry)
}

// C#: FileActionControllerOutcomeGapTests.RunAction_MoveWithOthersRemaining_WritesNoStatusAtAll
#[test]
fn move_with_others_remaining_writes_no_status_at_all() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();

    assert!(env.sink.statuses.is_empty(), "{:?}", env.sink.statuses);
}

// C#: FileActionControllerOutcomeGapTests.RunAction_MovingTheLastPhoto_SaysAllImagesProcessedBeforeTheMoveEvenIfItFails
#[test]
fn moving_the_last_photo_says_all_images_processed_before_the_move_even_if_it_fails() {
    let mut env = move_env(&["a.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Move, &p("a.jpg"), "disk full")));

    env.run_action(0, None, Some(&p("a.jpg")));
    // INV-3: the status is written before the executor job has run
    assert_eq!(env.sink.statuses, vec![text("status.allImagesProcessed")]);
    assert!(env.exec.recorded().is_empty());
    env.run_worker();

    // the failure replaces it afterwards and the entry is back
    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
    assert_eq!(
        env.sink.last_status(),
        Some(
            en(
                "status.actionFailed",
                &[arg("actionName", "MoveToSub"), arg("message", "disk full")]
            )
            .as_str()
        )
    );
}

// C#: FileActionControllerOutcomeGapTests.RunAction_MoveFailsWithAnException_RestoresTheEntryAtItsPositionAndReportsFailure
// C#: FileActionControllerUnexpectedThrowTests.ExecutorThrows_EntryGoesBackAtItsIndex_FailureIsReportedAndLogged
#[test]
fn executor_throws_entry_goes_back_at_its_index_failure_is_reported_and_logged() {
    let mut env = move_env(&["a.jpg", "b.jpg", "c.jpg"]);
    env.exec.script(Scripted::Panic("boom".to_string()));

    env.run_action(0, None, Some(&p("b.jpg")));
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("c.jpg")]);
    assert_eq!(env.run_worker(), FileActionStep::Pending); // the failure text waits for the presenter
    assert_eq!(env.present_finished(false), DONE_FALSE);

    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("b.jpg"), p("c.jpg")]);
    assert_eq!(
        env.sink.last_status(),
        Some(
            en(
                "status.actionFailed",
                &[arg("actionName", "MoveToSub"), arg("message", "boom")]
            )
            .as_str()
        )
    );
    assert!(env.log.errors.lock().unwrap().iter().any(|m| m.contains("threw")));
    assert_eq!(env.sink.navigation_changes, 1);
}

// C#: FileActionControllerUnexpectedThrowTests.GroupExecutorThrows_WholeCaptureGoesBackAsOneGroupedEntry
#[test]
fn group_executor_throws_whole_capture_goes_back_as_one_grouped_entry() {
    let mut env = pair_env(AppSettings::default(), false);
    env.exec.script(Scripted::Panic("boom".to_string()));
    let before = env.catalog_paths();

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();

    assert_eq!(env.catalog_paths(), before);
    assert!(
        env.catalog
            .find(&p("pair.jpg"))
            .is_some_and(|e| e.capture_group.is_some())
    );
    assert_eq!(env.sink.removed.last(), Some(&None)); // on_catalog_changed(None) after the group restore
}

// C#: FileActionControllerUnexpectedThrowTests.ExecutorThrows_AfterTheUserOpenedAnotherFolder_TouchesNeitherCatalogNorStatusOfTheNewFolder
#[test]
fn executor_throws_after_the_user_opened_another_folder_touches_neither_catalog_nor_status_of_the_new_folder() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec.script(Scripted::Panic("boom".to_string()));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    let statuses_before = env.sink.statuses.len();
    assert_eq!(env.run_worker(), FileActionStep::Pending);
    assert_eq!(env.present_finished(false), DONE_FALSE);

    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]);
    assert_eq!(env.sink.statuses.len(), statuses_before);
}

// C#: FileActionControllerUnexpectedThrowTests.SucceededAction_BookkeepingThrowsAndPresentFaults_BothAreLogged_StatusSaysCompleted_EntryNotRestored
#[test]
fn succeeded_action_bookkeeping_throws_and_present_faults_both_are_logged_status_says_completed_entry_not_restored() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.sink.panic_on_session = true;

    env.run_action(0, None, Some(&p("a.jpg")));
    assert_eq!(env.run_worker(), FileActionStep::Pending); // waits for the present
    assert_eq!(env.present_finished(true), DONE_TRUE);

    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]); // not restored
    assert_eq!(
        env.sink.last_status(),
        Some(en("status.actionCompleted", &[arg("actionName", "MoveToSub")]).as_str())
    );
    let errors = env.log.errors.lock().unwrap().clone();
    assert!(errors.iter().any(|m| m.contains("post-processing threw")), "{errors:?}");
    assert!(
        errors
            .iter()
            .any(|m| m.contains("Present after failed file action threw")),
        "{errors:?}"
    );
}

// C#: FileActionControllerOrderingTests.SucceededMove_PostProcessingThrows_DoesNotRestoreEntryOrReportFailure
#[test]
fn succeeded_move_post_processing_throws_does_not_restore_entry_or_report_failure() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.sink.panic_on_session = true;

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();
    env.present_finished(false);

    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]);
    assert!(
        !env.sink.statuses.iter().any(|s| s.contains("Could not run")),
        "{:?}",
        env.sink.statuses
    );
}

// C#: FileActionControllerUnexpectedThrowTests.ExecutorCancelled_IsNotSwallowedAsAFailure_AndTheEntryIsNotReportedFailed
#[test]
fn executor_cancelled_is_not_swallowed_as_a_failure_and_the_entry_is_not_reported_failed() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);

    env.run_action(0, None, Some(&p("a.jpg")));
    let events = env.port.run_all();
    let FileActionEvent::Executed { ticket, .. } = events.into_iter().next().expect("one job") else {
        panic!("expected an Executed event");
    };
    let step = env.handle(FileActionEvent::Executed {
        ticket,
        outcome: ExecOutcome::Canceled,
    });

    assert_eq!(step, FileActionStep::Done(FileActionDone::Canceled));
    assert_eq!(env.catalog_paths(), vec![p("b.jpg")], "nothing is restored");
    assert!(env.sink.statuses.is_empty());
    assert_eq!(env.sink.navigation_changes, 1, "the finally still runs");
    assert!(!env.controller.is_in_flight());
}

// C#: FileActionControllerOrderingTests.FastFailedMove_FailureStatus_IsNotOverwrittenByPresenter
#[test]
fn fast_failed_move_failure_status_is_not_overwritten_by_presenter() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Move, &p("a.jpg"), "locked")));

    env.run_action(0, None, Some(&p("a.jpg")));
    assert_eq!(env.run_worker(), FileActionStep::Pending); // the failure text waits for the presenter
    assert!(
        env.sink.statuses.is_empty(),
        "no status before the present finished: {:?}",
        env.sink.statuses
    );
    assert_eq!(env.present_finished(false), DONE_FALSE);

    assert_eq!(
        env.sink.last_status(),
        Some(
            en(
                "status.actionFailed",
                &[arg("actionName", "MoveToSub"), arg("message", "locked")]
            )
            .as_str()
        )
    );
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("b.jpg")]);
}

// C#: FileActionControllerOrderingTests.FailedMove_WhenThePresentFaults_KeepsTheActionsErrorText
// C#: FileActionControllerOutcomeGapTests.RunAction_MoveFailsAndThePresentOfTheNextPhotoFaults_DoesNotThrowAndRestoresTheEntry
#[test]
fn failed_move_when_the_present_faults_keeps_the_actions_error_text() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Move, &p("a.jpg"), "locked")));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();
    assert_eq!(env.present_finished(true), DONE_FALSE);

    assert_eq!(
        env.sink.last_status(),
        Some(
            en(
                "status.actionFailed",
                &[arg("actionName", "MoveToSub"), arg("message", "locked")]
            )
            .as_str()
        )
    );
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("b.jpg")]);
    assert!(
        env.log
            .errors
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("Present after failed file action threw"))
    );
}

// C#: FileActionControllerOutcomeGapTests.RunAction_MoveFailsAfterTheFolderSwitched_DoesNotRestoreIntoTheNewCatalog
// C#: FileActionControllerLateCompletionTests.FailedMove_AfterFolderSwitch_NoUndo_NoRestoreIntoNewCatalog
#[test]
fn failed_move_after_the_folder_switched_does_not_restore_into_the_new_catalog_and_registers_no_undo() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Move, &p("a.jpg"), "locked")));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);

    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]);
    assert!(env.sink.late_statuses.is_empty());
    assert!(env.sink.statuses.is_empty());
    assert_eq!(undo_history_len(&env), 0);
    assert!(!has_undo_action(&env));
}

// C#: FileActionControllerFolderScopeTests.FailedMove_PresentCompletesAfterFolderSwitch_WritesNoStatusIntoNewFolder
#[test]
fn failed_move_present_completes_after_folder_switch_writes_no_status_into_new_folder() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Move, &p("a.jpg"), "locked")));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker(); // parked behind the present
    env.clock.next_folder();
    assert_eq!(env.present_finished(false), DONE_FALSE);

    assert!(env.sink.statuses.is_empty(), "{:?}", env.sink.statuses);
    assert_eq!(env.sink.navigation_changes, 1);
}

// C#: FileActionControllerFolderScopeTests.ThrowingExecutor_PresentCompletesAfterFolderSwitch_WritesNoStatusIntoNewFolder
#[test]
fn throwing_executor_present_completes_after_folder_switch_writes_no_status_into_new_folder() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec.script(Scripted::Panic("boom".to_string()));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();
    env.clock.next_folder();
    env.present_finished(false);

    assert!(env.sink.statuses.is_empty(), "{:?}", env.sink.statuses);
}

// C#: FileActionControllerFolderScopeTests.SucceededMoveWithThrowingBookkeeping_PresentCompletesAfterFolderSwitch_WritesNoStatusIntoNewFolder
#[test]
fn succeeded_move_with_throwing_bookkeeping_present_completes_after_folder_switch_writes_no_status_into_new_folder() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.sink.panic_on_session = true;

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();
    env.clock.next_folder();
    assert_eq!(env.present_finished(false), DONE_FALSE);

    assert!(env.sink.statuses.is_empty(), "{:?}", env.sink.statuses);
}

// C#: FileActionControllerFolderScopeTests.UnverifiedMove_PresentCompletesAfterFolderSwitch_WritesNothingIntoNewFolder
#[test]
fn unverified_move_present_completes_after_folder_switch_writes_nothing_into_new_folder() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    let mut result = single_fail(Move, &p("a.jpg"), "size differs");
    result.source_removed = true;
    env.exec.script(Scripted::Single(result));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();
    env.clock.next_folder();
    assert_eq!(env.present_finished(false), DONE_FALSE);

    assert!(env.sink.statuses.is_empty());
    assert!(env.sink.sessions.is_empty());
}

// C#: FileActionControllerGapTests.RunAction_SingleMoveGrowsWhileMovingAndSourceIsGone_StaysOutOfCatalogWarnsAndRegistersNoUndo
#[test]
fn unverified_move_stays_out_of_catalog_warns_after_the_present_and_registers_no_undo() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    let mut result = single_fail(Move, &p("a.jpg"), "size differs");
    result.source_removed = true;
    env.exec.script(Scripted::Single(result));

    env.run_action(0, None, Some(&p("a.jpg")));
    assert_eq!(env.run_worker(), FileActionStep::Pending);
    assert_eq!(env.present_finished(false), DONE_FALSE);

    assert_eq!(env.catalog_paths(), vec![p("b.jpg")], "the entry is NOT restored");
    assert_eq!(
        env.sink.last_status(),
        Some(en("status.moveUnverified", &[arg("fileName", "a.jpg")]).as_str())
    );
    assert_eq!(env.sink.sessions, vec![p("b.jpg")]);
    assert!(!has_undo_action(&env));
}

// C#: FileActionControllerFolderIdentityTests.UnverifiedMove_FolderSwitchedDuringIo_AnnouncedLate
#[test]
fn unverified_move_folder_switched_during_io_announced_late() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    let mut result = single_fail(Move, &p("a.jpg"), "size differs");
    result.source_removed = true;
    env.exec.script(Scripted::Single(result));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);

    assert_eq!(
        env.sink.late_statuses,
        vec![en("status.moveUnverified", &[arg("fileName", "a.jpg")])]
    );
    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerLateCompletionTests
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerLateCompletionTests.Move_FinishingAfterFolderSwitch_IsRegistered_AndLeavesNewFolderUntouched
#[test]
fn move_finishing_after_folder_switch_is_registered_and_leaves_new_folder_untouched() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_ok(Move, &p("a.jpg"), Some("dest/a.jpg"))));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);

    assert_eq!(undo_history_len(&env), 1);
    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]);
    assert!(env.sink.sessions.is_empty());
    assert_eq!(
        env.sink.late_statuses,
        vec![en(
            "status.lateMoveUndoable",
            &[arg("fileName", "a.jpg"), arg("folder", "dest")]
        )]
    );
}

// C#: FileActionControllerLateCompletionTests.Recycle_FinishingAfterFolderSwitch_IsUndoable_AndUndoDoesNotTouchNewFolderSession
#[test]
fn recycle_finishing_after_folder_switch_is_undoable() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);

    env.run_action(1, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    env.run_worker();

    assert!(has_undo_action(&env));
    assert_eq!(
        env.sink.late_statuses,
        vec![en("status.lateRecycleUndoable", &[arg("fileName", "a.jpg")])]
    );
    assert!(env.sink.sessions.is_empty());
}

// C#: FileActionControllerLateCompletionTests.PermanentDelete_FinishingAfterFolderSwitch_ReportsNotUndoable
#[test]
fn permanent_delete_finishing_after_folder_switch_reports_not_undoable() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    let mut result = single_ok(Recycle, &p("a.jpg"), None);
    result.permanently_deleted = true;
    env.exec.script(Scripted::Single(result));

    env.run_action(1, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    env.run_worker();

    assert_eq!(
        env.sink.late_statuses,
        vec![en("status.lateDeletedPermanently", &[arg("fileName", "a.jpg")])]
    );
}

// C#: FileActionControllerLateCompletionTests.GroupRecycle_FinishingAfterFolderSwitch_ReportsPermanentMembersAccurately (theory)
#[test]
fn group_recycle_finishing_after_folder_switch_reports_permanent_members_accurately() {
    // (jpeg permanent, raw permanent) -> expected status
    let cases: [(bool, bool, String); 3] = [
        (
            false,
            false,
            en("status.lateRecycleUndoable", &[arg("fileName", "pair.jpg")]),
        ),
        (
            true,
            true,
            en("status.lateDeletedPermanently", &[arg("fileName", "pair.jpg")]),
        ),
        (
            true,
            false,
            en(
                "status.lateDeletedPartlyPermanently",
                &[arg("fileName", "pair.jpg"), arg("permanentFiles", "pair.jpg")],
            ),
        ),
    ];
    for (jpeg_permanent, raw_permanent, expected) in cases {
        let mut env = pair_env(AppSettings::default(), false);
        env.exec.script(Scripted::Group(group_ok(
            Recycle,
            vec![
                member(&p("pair.jpg"), None, true, jpeg_permanent),
                member(&p("pair.cr2"), None, true, raw_permanent),
            ],
            None,
        )));

        env.run_action(1, None, Some(&p("pair.jpg")));
        env.clock.next_folder();
        env.run_worker();

        assert_eq!(
            env.sink.late_statuses,
            vec![expected],
            "jpeg {jpeg_permanent}, raw {raw_permanent}"
        );
    }
}

// C#: FileActionControllerLateCompletionTests.GroupRecycle_FailingPartWayAfterFolderSwitch_ReportsTheProcessedMembers
#[test]
fn group_recycle_failing_part_way_after_folder_switch_reports_the_processed_members() {
    let mut env = pair_env(AppSettings::default(), false);
    env.exec.script(Scripted::Group(group_failed(
        Recycle,
        vec![
            member(&p("pair.jpg"), None, true, false),
            member(&p("pair.cr2"), None, false, false),
        ],
        "bin full",
    )));

    env.run_action(1, None, Some(&p("pair.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);

    assert_eq!(
        env.sink.late_statuses,
        vec![en(
            "status.lateGroupPartiallyProcessed",
            &[
                arg("fileName", "pair.jpg"),
                LocArg::new("done", 1_i64),
                LocArg::new("total", 2_i64)
            ]
        )]
    );
}

// C#: FileActionControllerOutcomeGapTests.RunAction_CaptureMoveFinishingAfterAFolderSwitch_ReportsTheLateMoveWithItsDestinationFolder
#[test]
fn capture_move_finishing_after_a_folder_switch_reports_the_late_move_with_its_destination_folder() {
    let mut env = pair_env(AppSettings::default(), false);
    env.exec.script(Scripted::Group(group_ok(
        Move,
        vec![
            member(&p("pair.jpg"), Some("dest/pair.jpg"), true, false),
            member(&p("pair.cr2"), Some("dest/pair.cr2"), true, false),
        ],
        Some("dest/pair.jpg"),
    )));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.clock.next_folder();
    env.run_worker();

    assert_eq!(
        env.sink.late_statuses,
        vec![en(
            "status.lateMoveUndoable",
            &[arg("fileName", "pair.jpg"), arg("folder", "dest")]
        )]
    );
}

// C#: FileActionControllerLateCompletionTests.Move_FinishingAfterSwitchAndReturn_IsStillLate
#[test]
fn move_finishing_after_switch_and_return_is_still_late() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_ok(Move, &p("a.jpg"), Some("dest/a.jpg"))));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder(); // away ...
    env.clock.next_folder(); // ... and back: a new generation, not the old one
    assert_eq!(env.run_worker(), DONE_FALSE);

    assert_eq!(env.sink.late_statuses.len(), 1);
}

// C#: FileActionControllerLateCompletionTests.TwoLateMoves_UndoInLifoOrder (registration half)
#[test]
fn two_late_moves_are_both_registered() {
    let mut env = move_env(&["a.jpg", "b.jpg", "c.jpg"]);
    env.exec
        .script(Scripted::Single(single_ok(Move, &p("a.jpg"), Some("dest/a.jpg"))));
    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    env.run_worker();
    env.exec
        .script(Scripted::Single(single_ok(Move, &p("b.jpg"), Some("dest/b.jpg"))));
    env.run_action(0, None, Some(&p("b.jpg")));
    env.clock.next_folder();
    env.run_worker();

    let history = env
        .undo
        .as_ref()
        .map(|u| u.lock().unwrap().move_history().to_vec())
        .unwrap_or_default();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].0, p("b.jpg"));
}

// ---------------------------------------------------------------------------------------------------------------
// Ordering and group results
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerOrderingTests.Copy_DoesNotStopReadsOrCancelPreload
#[test]
fn copy_does_not_stop_reads_or_cancel_preload() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    let folder_before = env.clock.current_folder();
    let navigation_before = env.clock.current_navigation();

    env.run_action(2, None, Some(&p("a.jpg")));
    env.run_worker();

    assert_eq!(env.clock.current_folder(), folder_before);
    assert_eq!(env.clock.current_navigation(), navigation_before);
    assert_eq!(env.preload.cancels.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(env.sink.presented.is_empty());
    assert_eq!(env.catalog.count(), 2);
}

// C#: FileActionControllerOrderingTests.Move_StillStopsReadsAndCancelsPreload
#[test]
fn move_still_stops_reads_and_cancels_preload() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    let folder_before = env.clock.current_folder();
    let navigation_before = env.clock.current_navigation();

    env.run_action(0, None, Some(&p("a.jpg")));

    assert!(env.clock.current_folder() > folder_before);
    assert!(env.clock.current_navigation() > navigation_before);
    assert_eq!(env.preload.cancels.load(std::sync::atomic::Ordering::SeqCst), 1);
}

// New test: INV-3, the next photo is requested before the executor job is even submitted and nothing waits for it
#[test]
fn present_is_requested_before_the_executor_job_is_submitted() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);

    let step = env.run_action(0, None, Some(&p("a.jpg")));

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(env.sink.presented, vec![0]); // "b.jpg" is now at index 0
    assert_eq!(env.port.queued(), 1);
    assert!(env.exec.recorded().is_empty(), "the I/O has not run yet");
    assert_eq!(*env.port.lanes.lock().unwrap(), vec![WorkerLane::FileAction]);
    env.run_worker();
    assert_eq!(
        env.sink.presented,
        vec![0],
        "nothing re-presents after the action (F-07)"
    );
}

// New test: INV-9, an event of an older operation is dropped
#[test]
fn late_event_with_old_ticket_is_ignored() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.run_action(0, None, Some(&p("a.jpg")));

    let step = env.handle(FileActionEvent::Executed {
        ticket: 9999,
        outcome: ExecOutcome::Single(single_fail(Move, &p("a.jpg"), "stale")),
    });

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(env.catalog_paths(), vec![p("b.jpg")], "a stale event restores nothing");
    assert!(env.controller.is_in_flight());
    assert_eq!(
        env.run_worker(),
        DONE_TRUE,
        "the real result still completes the operation"
    );
}

// New test: a status that must follow the presenter is parked and written exactly when the present finishes
#[test]
fn parked_tail_runs_when_present_finishes() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Move, &p("a.jpg"), "locked")));

    env.run_action(0, None, Some(&p("a.jpg")));
    assert_eq!(env.run_worker(), FileActionStep::Pending);
    assert!(
        env.controller.is_in_flight(),
        "the parked tail keeps the operation open"
    );
    assert_eq!(env.sink.navigation_changes, 0, "the finally runs when the tail ends");

    assert_eq!(env.present_finished(false), DONE_FALSE);
    assert!(!env.controller.is_in_flight());
    assert_eq!(env.sink.navigation_changes, 1);
}

// New test: a present that already finished before the result arrived does not park anything
#[test]
fn result_after_a_finished_present_writes_its_status_at_once() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Move, &p("a.jpg"), "locked")));

    env.run_action(0, None, Some(&p("a.jpg")));
    assert_eq!(env.present_finished(false), FileActionStep::Pending); // nothing parked yet
    assert_eq!(env.run_worker(), DONE_FALSE);

    assert!(env.sink.last_status().is_some_and(|s| s.contains("locked")));
}

// C#: FileActionControllerGroupTests.GroupRecycle_RemovedCapture_EvictsTheCachesOfEveryMemberNotOnlyTheSource
#[test]
fn group_recycle_removed_capture_evicts_the_caches_of_every_member_not_only_the_source() {
    let mut env = Env::new(AppSettings::default());
    env.load_pair_between_two_photos(true);
    env.set_actions(AppSettings::default(), vec![action("DeleteIt", Recycle, "", false)]);

    env.run_action(0, None, Some(&p("pair.jpg")));

    assert_eq!(env.sink.removed, vec![Some(p("pair.jpg"))]);
    assert!(env.sink.evicted.contains(&p("pair.cr2")));
    assert!(env.sink.evicted.contains(&p("pair.xmp")));
    assert!(!env.sink.evicted.contains(&p("pair.jpg")));
}

// C#: FileActionControllerGroupTests.SingleFileRecycle_EvictsNoExtraPaths
#[test]
fn single_file_recycle_evicts_no_extra_paths() {
    let mut env = move_env(&["plain.jpg", "other.jpg"]);

    env.run_action(1, None, Some(&p("plain.jpg")));

    assert!(env.sink.evicted.is_empty());
}

// C#: FileActionControllerGroupTests.GroupMove_PreflightRefused_RestoresTheWholeGroupedEntryAtItsOriginalIndex
// C#: FileActionControllerOrderingTests.GroupMovePartialFailure_RollsBackAndRestoresTheWholeGroupWithoutARecoveryEntry
#[test]
fn group_move_preflight_refused_restores_the_whole_grouped_entry_at_its_original_index() {
    let mut env = pair_env(AppSettings::default(), false);
    let before = env.catalog_paths();
    // no member state at all: the service refused before touching anything
    env.exec
        .script(Scripted::Group(group_failed(Move, Vec::new(), "destination exists")));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();
    env.present_finished(false);

    assert_eq!(env.catalog_paths(), before);
    assert!(
        env.catalog
            .find(&p("pair.cr2"))
            .is_some_and(|e| e.capture_group.is_some())
    );
    assert_eq!(env.catalog.index_of(&p("pair.cr2")), 1);
    assert!(env.sink.last_status().is_some_and(|s| s.contains("MoveToSub")));
}

// C#: FileActionControllerGroupTests.GroupRecycle_FailingPartWay_RegistersTheAlreadyRecycledMembersForUndo
#[test]
fn group_recycle_failing_part_way_registers_the_already_recycled_members_for_undo() {
    let mut env = pair_env(AppSettings::default(), false);
    env.exec.script(Scripted::Group(group_failed(
        Recycle,
        vec![
            member(&p("pair.jpg"), None, true, false),
            member(&p("pair.cr2"), None, false, false),
        ],
        "bin full",
    )));

    env.run_action(1, None, Some(&p("pair.jpg")));
    env.run_worker();
    env.present_finished(false);

    // INV-6: registered for undo even though the action failed; the member still on disk comes back
    assert!(has_undo_action(&env));
    assert!(env.catalog.find(&p("pair.cr2")).is_some());
    // a capture whose members partly reached the bin keeps its status text and shows no error dialog
    assert_eq!(env.dialog.errors.lock().unwrap().len(), 0);
}

// C#: FileActionControllerGroupTests.GroupRecycle_FailingPartWayWithAnUnreadableMemberState_StillReturnsThatMemberToTheCatalog
#[test]
fn group_recycle_failing_part_way_with_an_unreadable_member_state_still_returns_that_member_to_the_catalog() {
    let mut env = pair_env(AppSettings::default(), false);
    let mut unknown = member(&p("pair.cr2"), None, false, false);
    unknown.state_known = false;
    unknown.source_exists = false;
    env.exec.script(Scripted::Group(group_failed(
        Recycle,
        vec![member(&p("pair.jpg"), None, true, false), unknown],
        "state unreadable",
    )));

    env.run_action(1, None, Some(&p("pair.jpg")));
    env.run_worker();

    assert!(env.catalog.index_of(&p("pair.cr2")) >= 0, "not dropped until a reload");
    assert!(env.catalog.find(&p("zbefore.jpg")).is_some());
    assert!(env.catalog.find(&p("aafter.jpg")).is_some());
}

// C#: FileActionControllerGroupTests.GroupMove_PartnerVanishedExternally_MovesTheRemainingFileAndSaysWhichWasMissing
#[test]
fn group_move_partner_vanished_externally_moves_the_remaining_file_and_says_which_was_missing() {
    let mut env = pair_env(AppSettings::default(), false);
    let mut result = group_ok(
        Move,
        vec![member(&p("pair.jpg"), Some("Sorted/pair.jpg"), true, false)],
        Some("Sorted/pair.jpg"),
    );
    result.skipped_missing = Some(vec![p("pair.cr2")]);
    env.exec.script(Scripted::Group(result));

    env.run_action(0, None, Some(&p("pair.jpg")));
    assert_eq!(env.run_worker(), FileActionStep::Pending); // after the presenter
    assert_eq!(env.present_finished(false), DONE_TRUE);

    assert_eq!(env.catalog_paths(), vec![p("zbefore.jpg"), p("aafter.jpg")]);
    assert_eq!(
        env.sink.last_status(),
        Some(
            en(
                "status.groupPartnerMissing",
                &[arg("fileName", "pair.jpg"), arg("missing", "pair.cr2")]
            )
            .as_str()
        )
    );
}

// C#: FileActionControllerGroupTests.GroupMove_PartnerVanishedAndNothingLeftToReview_KeepsAllImagesProcessedAndNamesTheMissingFile
#[test]
fn group_move_partner_vanished_and_nothing_left_to_review_keeps_all_images_processed_and_names_the_missing_file() {
    let mut env = Env::new(AppSettings::default());
    env.load(&["pair.jpg", "pair.cr2"]);
    env.set_actions(AppSettings::default(), vec![action("MoveToSub", Move, "Sorted", false)]);
    let mut result = group_ok(
        Move,
        vec![member(&p("pair.jpg"), Some("Sorted/pair.jpg"), true, false)],
        Some("Sorted/pair.jpg"),
    );
    result.skipped_missing = Some(vec![p("pair.cr2")]);
    env.exec.script(Scripted::Group(result));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();
    env.present_finished(false);

    let missing = en(
        "status.groupPartnerMissing",
        &[arg("fileName", "pair.jpg"), arg("missing", "pair.cr2")],
    );
    assert_eq!(
        env.sink.last_status(),
        Some(format!("{} {missing}", text("status.allImagesProcessed")).as_str())
    );
}

// C#: FileActionControllerGroupTests.GroupMove_PartnerVanished_FolderSwitchedWhileThePresenterRuns_DoesNotOverwriteTheNewFoldersStatus
#[test]
fn group_move_partner_vanished_folder_switched_while_the_presenter_runs_does_not_overwrite_the_new_folders_status() {
    let mut env = pair_env(AppSettings::default(), false);
    let mut result = group_ok(
        Move,
        vec![member(&p("pair.jpg"), Some("Sorted/pair.jpg"), true, false)],
        Some("Sorted/pair.jpg"),
    );
    result.skipped_missing = Some(vec![p("pair.cr2")]);
    env.exec.script(Scripted::Group(result));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();
    env.clock.next_folder();
    assert_eq!(env.present_finished(false), DONE_TRUE);

    assert!(env.sink.statuses.is_empty(), "{:?}", env.sink.statuses);
}

// New test: the part of the C# Recycle failure that the user must not miss: a dialog as well as a status
#[test]
fn failed_recycle_with_nothing_deleted_shows_the_recycle_failed_dialog() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Recycle, &p("a.jpg"), "locked")));

    env.run_action(1, None, Some(&p("a.jpg")));
    env.run_worker();
    env.present_finished(false);

    let (title, message) = env.dialog.only_error();
    assert_eq!(title, text("dialog.recycleFailed.title"));
    assert!(message.contains("a.jpg") && message.contains("locked"), "{message}");
}
