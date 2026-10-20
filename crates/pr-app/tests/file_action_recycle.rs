//! W8-07c: `FileActionController::recycle`, the no-bin refusal, the permanent-delete and group prompts, `prepare_core`
//! (contract `contracts/W8-07c.md`; C# `FileActionController.cs` lines 1 to 289).
//!
//! The C# tests ran on a temp folder and a fake Recycle Bin. Here the executor is a fake (`FakeExecutor`), so "the file was
//! deleted permanently" becomes "the request handed to the executor has `allow_permanent_delete` set", and nothing touches a disk.
//!
//! Two groups of tests:
//! - Refusal paths end inside `prepare_core`/`recycle` and run today.
//! - Tests that reach the executor need `execute_core` of W8-07d (still `todo!()` on master); they are `#[ignore]`d with that reason
//!   and must be un-ignored by the unit that implements it.
//!
//! The C# tests `Recycle_ConfirmThenPhotoRemovedFromCatalogMeanwhile_*`, `Recycle_PermanentDeletePromptThenPhotoRemoved*`,
//! `Recycle_GroupPromptThenCaptureRemoved*` and `GroupRecycle_EntryDegradedToStandalone*` mutate the catalog from inside the open
//! dialog. In Rust the catalog is `&mut` for the whole call (`FileActionCx`), so that interleaving cannot happen: the re-checks stay
//! in the code and the same guards are exercised with a folder switch and a busy executor instead (marked `// New test`).

mod file_action_fakes;

use file_action_fakes::{Env, EnvOptions, ExecutorCall, p};
use pr_app::coordinators::file_actions::recycle::{FileActionDone, FileActionStep};
use pr_app::coordinators::folder_load::state::WorkerLane;
use pr_core::model::FileOperationType;
use pr_core::settings::app_settings::AppSettings;
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::LocArg;

const NOT_DONE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: false });

fn en(key: &str, args: &[LocArg]) -> String {
    built_in_english_localizer().format(key, args)
}

fn title(key: &str) -> String {
    built_in_english_localizer().get(key)
}

fn settings(confirm_before_delete: bool, allow_permanent: bool) -> AppSettings {
    AppSettings {
        confirm_before_delete,
        allow_permanent_delete_without_recycle_bin: allow_permanent,
        ..AppSettings::default()
    }
}

fn arg(name: &str, value: &str) -> LocArg {
    LocArg::new(name, value.to_string())
}

fn singles(env: &Env) -> Vec<pr_core::file_actions::types::FileActionRequest> {
    env.exec
        .recorded()
        .into_iter()
        .filter_map(|c| match c {
            ExecutorCall::Single(r) => Some(r),
            ExecutorCall::Group(_) => None,
        })
        .collect()
}

fn groups(env: &Env) -> Vec<pr_core::file_actions::types::CaptureGroupActionRequest> {
    env.exec
        .recorded()
        .into_iter()
        .filter_map(|c| match c {
            ExecutorCall::Group(r) => Some(r),
            ExecutorCall::Single(_) => None,
        })
        .collect()
}

/// Nothing may have left the catalog and nothing may have been queued or reported as presented (INV-3 precondition).
fn assert_nothing_changed(env: &Env, before: &[String], folder_before: i64) {
    assert_eq!(env.catalog_paths(), before, "the catalog must be untouched");
    assert_eq!(env.clock.current_folder(), folder_before);
    assert_eq!(env.port.queued(), 0, "no worker job may be queued");
    assert!(env.exec.recorded().is_empty(), "the executor must not be called");
    assert!(env.sink.presented.is_empty(), "nothing may be presented");
    assert!(env.sink.removed.is_empty(), "no catalog change may be announced");
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerConfirmDeleteTests
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerConfirmDeleteTests.ConfirmBeforeDelete_Off_RecyclesWithoutPrompt
#[test]
#[ignore = "needs W8-07d execute_core"]
fn confirm_before_delete_off_recycles_without_prompt() {
    let mut env = Env::new(settings(false, false));
    env.load(&["a.jpg"]);
    env.dialog_answer(false); // would refuse if ever asked

    let step = env.recycle(None, Some(&p("a.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(env.dialog.confirmation_count(), 0);
    let calls = singles(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].source, p("a.jpg"));
    assert_eq!(calls[0].operation, FileOperationType::Recycle);
    assert!(!calls[0].allow_permanent_delete);
    assert_eq!(env.catalog.count(), 0);
}

// C#: FileActionControllerConfirmDeleteTests.ConfirmBeforeDelete_On_Confirmed_Recycles
#[test]
#[ignore = "needs W8-07d execute_core"]
fn confirm_before_delete_on_confirmed_recycles() {
    let mut env = Env::new(settings(true, false));
    env.load(&["a.jpg"]);

    let step = env.recycle(None, Some(&p("a.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    let (confirm_title, message) = env.dialog.only_confirmation();
    assert_eq!(confirm_title, title("dialog.confirmAction.title"));
    assert_eq!(
        message,
        en(
            "dialog.confirmAction.message",
            &[arg("actionName", &title("action.recycle.name"))]
        )
    );
    assert_eq!(singles(&env).len(), 1);
    assert_eq!(env.catalog.count(), 0);
}

// C#: FileActionControllerConfirmDeleteTests.ConfirmBeforeDelete_On_Declined_DoesNothing
#[test]
fn confirm_before_delete_on_declined_does_nothing() {
    let mut env = Env::new(settings(true, false));
    env.load(&["a.jpg"]);
    env.dialog_answer(false);
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("a.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 1);
    assert_nothing_changed(&env, &before, 0);
    assert_eq!(env.catalog.count(), 1);
}

// C#: FileActionControllerConfirmDeleteTests.ConfirmBeforeDelete_On_WithPermanentDeletePrompt_AsksOnlyOnce
#[test]
#[ignore = "needs W8-07d execute_core"]
fn confirm_before_delete_on_with_permanent_delete_prompt_asks_only_once() {
    let mut env = Env::new(settings(true, true));
    env.load(&["a.jpg"]);
    env.exec.set_no_bin(true); // this volume has no Recycle Bin -> the narrow Q-R8 prompt applies instead

    let step = env.recycle(None, Some(&p("a.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    let (confirm_title, message) = env.dialog.only_confirmation(); // not two
    assert_eq!(confirm_title, title("dialog.confirmPermanentDelete.title"));
    assert_eq!(
        message,
        en("dialog.confirmPermanentDelete.message", &[arg("fileName", "a.jpg")])
    );
    let calls = singles(&env);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].allow_permanent_delete);
}

// C#: FileActionControllerConfirmDeleteTests.ConfirmBeforeDelete_GroupWithPermanentMembers_AsksOnceAndCountsPermanentMembers
#[test]
#[ignore = "needs W8-07d execute_core"]
fn confirm_before_delete_group_with_permanent_members_asks_once_and_counts_permanent_members() {
    let mut env = Env::new(settings(true, true));
    env.load(&["pair.jpg", "pair.cr2"]);
    env.exec.set_no_bin(true);

    let step = env.recycle(None, Some(&p("pair.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    let (_, message) = env.dialog.only_confirmation();
    assert!(message.contains('2'), "{message}");
    let calls = groups(&env);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].allow_permanent_delete);
    assert_eq!(calls[0].group.paths(), [p("pair.jpg"), p("pair.cr2")]);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerConfirmGapTests (the Recycle_* ones)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerConfirmGapTests.Recycle_ConfirmBeforeDeleteWithCompareSelectionOnDriveWithoutBin_AsksOnlyThePermanentDeletePrompt
#[test]
#[ignore = "needs W8-07d execute_core"]
fn recycle_confirm_before_delete_with_compare_selection_on_drive_without_bin_asks_only_the_permanent_delete_prompt() {
    let mut env = Env::new(settings(true, true));
    env.load(&["a.jpg", "b.jpg"]);
    env.exec.add_no_bin_for(&p("b.jpg"));

    let step = env.recycle(Some(&p("b.jpg")), Some(&p("a.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    // the selected file decides: its permanent-delete prompt replaces the generic one
    let (confirm_title, _) = env.dialog.only_confirmation();
    assert_eq!(confirm_title, title("dialog.confirmPermanentDelete.title"));
    let calls = singles(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].source, p("b.jpg"));
    assert!(calls[0].allow_permanent_delete);
    assert_eq!(env.catalog_paths(), vec![p("a.jpg")]);
}

// C#: FileActionControllerConfirmGapTests.Recycle_ConfirmThenFolderSwitchedMeanwhile_DoesNotActOnTheFile
#[test]
fn recycle_confirm_then_folder_switched_meanwhile_does_not_act_on_the_file() {
    let mut env = Env::new(settings(true, false));
    env.load(&["a.jpg"]);
    let clock = env.clock.clone();
    env.dialog.set_on_confirm(move || {
        clock.next_folder();
    });
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("a.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 1);
    assert_nothing_changed(&env, &before, 1);
}

// New test: the busy re-check after the generic confirmation (R7-4); the C# photo-removed test cannot be written in Rust (see module doc)
#[test]
fn recycle_confirm_then_another_action_became_busy_does_not_act_on_the_file() {
    let mut env = Env::new(settings(true, false));
    env.load(&["a.jpg"]);
    let exec = env.exec.clone();
    env.dialog.set_on_confirm(move || exec.set_busy(true));
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("a.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_nothing_changed(&env, &before, 0);
}

// C#: FileActionControllerConfirmGapTests.Recycle_PermanentDeletePromptThenFolderSwitchedMeanwhile_DoesNotDeleteTheFile
#[test]
fn recycle_permanent_delete_prompt_then_folder_switched_meanwhile_does_not_delete_the_file() {
    let mut env = Env::new(settings(false, true));
    env.load(&["a.jpg"]);
    env.exec.set_no_bin(true);
    let clock = env.clock.clone();
    env.dialog.set_on_confirm(move || {
        clock.next_folder();
    });
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("a.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 1);
    assert_nothing_changed(&env, &before, 1);
}

// New test: busy re-check after the permanent-delete prompt (stands in for the C# photo-removed twin)
#[test]
fn recycle_permanent_delete_prompt_then_another_action_became_busy_does_not_delete_the_file() {
    let mut env = Env::new(settings(false, true));
    env.load(&["a.jpg"]);
    env.exec.set_no_bin(true);
    let exec = env.exec.clone();
    env.dialog.set_on_confirm(move || exec.set_busy(true));
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("a.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_nothing_changed(&env, &before, 0);
}

// C#: FileActionControllerConfirmGapTests.Recycle_GroupPromptThenFolderSwitchedMeanwhile_DoesNotRecycleTheCapture
#[test]
fn recycle_group_prompt_then_folder_switched_meanwhile_does_not_recycle_the_capture() {
    let mut env = Env::new(settings(true, false));
    env.load(&["pair.jpg", "pair.cr2"]);
    let clock = env.clock.clone();
    env.dialog.set_on_confirm(move || {
        clock.next_folder();
    });
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("pair.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 1);
    assert_nothing_changed(&env, &before, 1);
}

// New test: busy re-check after the group prompt (stands in for the C# capture-removed twin, which also expects no status text)
#[test]
fn recycle_group_prompt_then_another_action_became_busy_returns_silently() {
    let mut env = Env::new(settings(true, false));
    env.load(&["pair.jpg", "pair.cr2", "z.jpg"]);
    let exec = env.exec.clone();
    env.dialog.set_on_confirm(move || exec.set_busy(true));
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("pair.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_nothing_changed(&env, &before, 0);
    assert!(
        env.sink.statuses.is_empty(),
        "no group-changed status for a busy executor"
    );
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerRecycleSafetyGapTests
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerRecycleSafetyGapTests.GroupRecycle_RaceAfterTheUpFrontCheckWithSettingOff_NeverAsksPermanentDeleteNorDeletes
#[test]
#[ignore = "needs W8-07d execute_core"]
fn group_recycle_race_after_the_up_front_check_with_setting_off_never_asks_permanent_delete_nor_deletes() {
    let mut env = Env::new(settings(false, false));
    env.load(&["pair.jpg", "pair.cr2"]);
    env.exec.vanish_after(2); // the up-front refusal check queries both members, later queries say "no bin"

    let step = env.recycle(None, Some(&p("pair.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(env.dialog.confirmation_count(), 0); // the permanent-delete prompt exists only with the setting ON
    let calls = groups(&env);
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].allow_permanent_delete);
}

// C#: FileActionControllerRecycleSafetyGapTests.GroupRecycle_RaceAfterTheUpFrontCheckAfterTheGenericGroupPrompt_StaysNonPermanent
#[test]
#[ignore = "needs W8-07d execute_core"]
fn group_recycle_race_after_the_up_front_check_after_the_generic_group_prompt_stays_non_permanent() {
    let mut env = Env::new(settings(true, false));
    env.load(&["pair.jpg", "pair.cr2"]);
    env.exec.vanish_after(2);

    let step = env.recycle(None, Some(&p("pair.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    // The user confirmed a plain "recycle this capture", never a permanent delete.
    let (confirm_title, _) = env.dialog.only_confirmation();
    assert_eq!(confirm_title, title("dialog.confirmAction.title"));
    let calls = groups(&env);
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].allow_permanent_delete);
}

// C#: FileActionControllerRecycleSafetyGapTests.GroupRecycle_RaceWithSettingOnButNoPromptedMember_DoesNotGrantPermanentDelete
#[test]
#[ignore = "needs W8-07d execute_core"]
fn group_recycle_race_with_setting_on_but_no_prompted_member_does_not_grant_permanent_delete() {
    let mut env = Env::new(settings(false, true));
    env.load(&["pair.jpg", "pair.cr2"]);
    env.exec.vanish_after(2); // the permanent-member query: both members still have a bin

    let step = env.recycle(None, Some(&p("pair.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(env.dialog.confirmation_count(), 0); // nothing was asked, so nothing may be permanent
    let calls = groups(&env);
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].allow_permanent_delete);
}

// C#: FileActionControllerRecycleSafetyGapTests.SingleRecycle_RaceAfterTheUpFrontCheckWithSettingOff_NeverDeletesPermanently
#[test]
#[ignore = "needs W8-07d execute_core"]
fn single_recycle_race_after_the_up_front_check_with_setting_off_never_deletes_permanently() {
    let mut env = Env::new(settings(false, false));
    env.load(&["solo.jpg"]);
    env.exec.vanish_after(1); // the up-front refusal check queries the file once

    let step = env.recycle(None, Some(&p("solo.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(env.dialog.confirmation_count(), 0);
    let calls = singles(&env);
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].allow_permanent_delete);
}

// C#: FileActionControllerRecycleSafetyGapTests.GroupRecycle_OnlyANonSelectedMemberLacksABin_RefusesUpFrontNamingThatMemberWithoutAnyPrompt
#[test]
fn group_recycle_only_a_non_selected_member_lacks_a_bin_refuses_up_front_naming_that_member_without_any_prompt() {
    let mut env = Env::new(settings(true, false));
    env.load(&["pair.jpg", "pair.cr2"]);
    env.exec.add_no_bin_for(&p("pair.cr2")); // the selected JPEG has a bin, its RAW partner does not
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("pair.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 0); // no "delete?" question for a delete that cannot happen
    let (error_title, error_message) = env.dialog.only_error();
    assert_eq!(error_title, title("dialog.recycleNoBin.title"));
    assert!(error_message.contains("pair.cr2"), "{error_message}");
    assert_eq!(
        env.sink.last_status(),
        Some(en("core.recycle.unsupportedDrive", &[arg("name", "pair.cr2")]).as_str())
    );
    assert_nothing_changed(&env, &before, 0);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerGroupTests (prompt and refusal rules through RecycleAsync)
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerGroupTests.GroupRecycle_FromCompareSelection_AlwaysAsksAndNamesBothFiles_EvenWithConfirmBeforeDeleteOff
#[test]
#[ignore = "needs W8-07d execute_core"]
fn group_recycle_from_compare_selection_always_asks_and_names_both_files_even_with_confirm_before_delete_off() {
    let mut env = Env::new(settings(false, false));
    env.load_pair_between_two_photos(false);

    // Compare is open with the RAW member selected
    let step = env.recycle(Some(&p("pair.cr2")), Some(&p("pair.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    let (_, message) = env.dialog.only_confirmation();
    assert_eq!(
        message,
        en(
            "dialog.confirmCompareGroupRecycle.message",
            &[arg("fileName", "pair.cr2"), arg("files", "pair.jpg, pair.cr2")]
        )
    );
    let calls = groups(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].group.paths().len(), 2);
}

// C#: FileActionControllerGroupTests.GroupRecycle_FromCompareSelectionDeclined_DeletesNothingAndKeepsTheEntry
#[test]
fn group_recycle_from_compare_selection_declined_deletes_nothing_and_keeps_the_entry() {
    let mut env = Env::new(settings(false, false));
    env.load_pair_between_two_photos(false);
    env.dialog_answer(false);
    let before = env.catalog_paths();

    let step = env.recycle(Some(&p("pair.cr2")), Some(&p("pair.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 1);
    assert_nothing_changed(&env, &before, 0);
    assert_eq!(before, vec![p("zbefore.jpg"), p("pair.jpg"), p("aafter.jpg")]);
}

// C#: FileActionControllerGroupTests.GroupRecycle_WithoutCompare_StillNeedsNoPromptWhenConfirmBeforeDeleteIsOff
#[test]
#[ignore = "needs W8-07d execute_core"]
fn group_recycle_without_compare_still_needs_no_prompt_when_confirm_before_delete_is_off() {
    let mut env = Env::new(settings(false, false));
    env.load_pair_between_two_photos(false);

    let step = env.recycle(None, Some(&p("pair.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(env.dialog.confirmation_count(), 0);
    assert_eq!(groups(&env)[0].group.paths().len(), 2);
}

// C#: FileActionControllerGroupTests.GroupRecycle_GroupUnchangedWhileTheConfirmationIsOpen_StillRecyclesBothMembers
#[test]
#[ignore = "needs W8-07d execute_core"]
fn group_recycle_group_unchanged_while_the_confirmation_is_open_still_recycles_both_members() {
    let mut env = Env::new(settings(true, false));
    env.load_pair_between_two_photos(false);
    env.dialog.set_on_confirm(|| {}); // the catalog is untouched

    let step = env.recycle(None, Some(&p("pair.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    let calls = groups(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].group.paths(), [p("pair.jpg"), p("pair.cr2")]);
    assert!(!calls[0].allow_permanent_delete);
    let (_, message) = env.dialog.only_confirmation();
    assert_eq!(
        message,
        en(
            "dialog.confirmGroupRecycle.message",
            &[LocArg::new("count", 2_i64), arg("fileName", "pair.jpg")]
        )
    );
}

// C#: FileActionControllerGroupTests.GroupRecycle_PermanentDeleteSettingOff_RefusesWithoutPromptAndDeletesNothing
#[test]
fn group_recycle_permanent_delete_setting_off_refuses_without_prompt_and_deletes_nothing() {
    let mut env = Env::new(settings(false, false));
    env.load_pair_between_two_photos(true);
    env.exec.set_no_bin(true);
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("pair.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 0); // no "permanently delete" prompt when the setting is off
    assert_nothing_changed(&env, &before, 0);
    let status = env.sink.last_status().expect("a status");
    assert!(
        status.contains(&en("core.recycle.unsupportedDrive", &[arg("name", "pair.jpg")])),
        "{status}"
    );
}

// C#: FileActionControllerGroupTests.GroupRecycle_PermanentDeleteSettingOffWithConfirmBeforeDelete_RefusesUpFrontWithoutAskingOrAdvancing
#[test]
fn group_recycle_permanent_delete_setting_off_with_confirm_before_delete_refuses_up_front_without_asking_or_advancing()
{
    let mut env = Env::new(settings(true, false));
    env.load_pair_between_two_photos(false);
    assert!(env.catalog.set_current(env.catalog.index_of(&p("pair.jpg"))));
    env.exec.set_no_bin(true);
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("pair.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 0); // no "delete?" question for a delete that cannot happen
    assert_eq!(env.dialog.only_error().0, title("dialog.recycleNoBin.title"));
    assert_nothing_changed(&env, &before, 0);
    assert_eq!(env.catalog.current().map(|e| e.path().to_string()), Some(p("pair.jpg"))); // the capture stays on screen
}

// C#: FileActionControllerGroupTests.GroupRecycle_PermanentDeleteSettingOn_PromptsPermanentlyAndDeletesAllMembers
#[test]
#[ignore = "needs W8-07d execute_core"]
fn group_recycle_permanent_delete_setting_on_prompts_permanently_and_deletes_all_members() {
    let mut env = Env::new(settings(false, true));
    env.load_pair_between_two_photos(true);
    env.exec.set_no_bin(true);

    let step = env.recycle(None, Some(&p("pair.jpg")));
    env.port.run_all();

    assert_eq!(step, FileActionStep::Pending);
    let (confirm_title, message) = env.dialog.only_confirmation();
    assert_eq!(confirm_title, title("dialog.confirmPermanentDelete.title"));
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
    let calls = groups(&env);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].allow_permanent_delete);
    assert_eq!(calls[0].group.paths(), [p("pair.jpg"), p("pair.cr2"), p("pair.xmp")]);
}

// ---------------------------------------------------------------------------------------------------------------
// New tests
// ---------------------------------------------------------------------------------------------------------------

// New test: every refusal leaves the catalog, the clock and the sink alone (INV-3 precondition, nothing leaves the catalog)
#[test]
fn refused_paths_leave_catalog_and_generation_unchanged() {
    // empty source
    let mut env = Env::new(settings(false, false));
    env.load(&["a.jpg"]);
    let before = env.catalog_paths();
    assert_eq!(env.recycle(None, None), NOT_DONE);
    assert_nothing_changed(&env, &before, 0);
    assert!(env.sink.is_untouched());

    // empty catalog
    let mut env = Env::new(settings(false, false));
    assert_eq!(env.recycle(None, Some(&p("a.jpg"))), NOT_DONE);
    assert_nothing_changed(&env, &[], 0);
    assert!(env.sink.is_untouched());

    // no executor
    let mut env = Env::with_options(
        settings(false, false),
        EnvOptions {
            executor: false,
            ..EnvOptions::default()
        },
    );
    env.load(&["a.jpg"]);
    let before = env.catalog_paths();
    assert_eq!(env.recycle(None, Some(&p("a.jpg"))), NOT_DONE);
    assert_nothing_changed(&env, &before, 0);
    assert!(env.sink.is_untouched());

    // permanent-delete prompt without a dialog service: nothing can be confirmed, nothing is deleted
    let mut env = Env::with_options(
        settings(false, true),
        EnvOptions {
            dialog: false,
            ..EnvOptions::default()
        },
    );
    env.load(&["a.jpg"]);
    env.exec.set_no_bin(true);
    let before = env.catalog_paths();
    assert_eq!(env.recycle(None, Some(&p("a.jpg"))), NOT_DONE);
    assert_nothing_changed(&env, &before, 0);
    assert!(env.sink.is_untouched());

    // a capture without a file system
    let mut env = Env::with_options(
        settings(true, false),
        EnvOptions {
            file_system: false,
            ..EnvOptions::default()
        },
    );
    env.load(&["pair.jpg", "pair.cr2"]);
    let before = env.catalog_paths();
    assert_eq!(env.recycle(None, Some(&p("pair.jpg"))), NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 0);
    assert_nothing_changed(&env, &before, 0);

    // the no-bin refusal also keeps the catalog (it only writes a status, a dialog and a navigation refresh)
    let mut env = Env::new(settings(false, false));
    env.load(&["a.jpg"]);
    env.exec.set_no_bin(true);
    let before = env.catalog_paths();
    assert_eq!(env.recycle(None, Some(&p("a.jpg"))), NOT_DONE);
    assert_nothing_changed(&env, &before, 0);
    assert_eq!(env.sink.navigation_changes, 1);
    assert_eq!(env.sink.statuses.len(), 1);
    assert_eq!(env.dialog.only_error().0, title("dialog.recycleNoBin.title"));
}

// New test: INV-4, the service's own gate. No dialog is shown, nothing is queued.
#[test]
fn recycle_while_another_action_is_in_flight_is_refused() {
    let mut env = Env::new(settings(false, false));
    env.load(&["a.jpg", "b.jpg"]);
    env.exec.set_busy(true);
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("a.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 0);
    assert_nothing_changed(&env, &before, 0);
    assert!(!env.controller.is_in_flight());
}

// New test: the settings are read at use time (R2-F-03): a flag flipped after construction decides the prompt
#[test]
fn recycle_reads_confirm_before_delete_at_use_time() {
    let mut env = Env::new(settings(false, false));
    env.load(&["a.jpg"]);
    env.dialog_answer(false);
    env.set_settings(settings(true, false)); // Settings > Save replaced the instance after the controller was built
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("a.jpg")));

    assert_eq!(step, NOT_DONE);
    assert_eq!(env.dialog.confirmation_count(), 1);
    assert_nothing_changed(&env, &before, 0);
}

// New test: a plain group prompt (setting off for permanent delete) names count and file, no executor call when declined
#[test]
fn group_prompt_declined_names_count_and_file_and_changes_nothing() {
    let mut env = Env::new(settings(true, false));
    env.load(&["pair.jpg", "pair.cr2"]);
    env.dialog_answer(false);
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("pair.jpg")));

    assert_eq!(step, NOT_DONE);
    let (confirm_title, message) = env.dialog.only_confirmation();
    assert_eq!(confirm_title, title("dialog.confirmAction.title"));
    assert_eq!(
        message,
        en(
            "dialog.confirmGroupRecycle.message",
            &[LocArg::new("count", 2_i64), arg("fileName", "pair.jpg")]
        )
    );
    assert_nothing_changed(&env, &before, 0);
}

// New test: a permanent group prompt uses the permanent title and the three counted arguments in order
#[test]
fn group_permanent_prompt_declined_uses_permanent_title_and_argument_order() {
    let mut env = Env::new(settings(false, true));
    env.load_pair_between_two_photos(true);
    env.exec.add_no_bin_for(&p("pair.cr2"));
    env.dialog_answer(false);
    let before = env.catalog_paths();

    let step = env.recycle(None, Some(&p("pair.jpg")));

    assert_eq!(step, NOT_DONE);
    let (confirm_title, message) = env.dialog.only_confirmation();
    assert_eq!(confirm_title, title("dialog.confirmPermanentDelete.title"));
    assert_eq!(
        message,
        en(
            "dialog.confirmGroupPermanentDelete.message",
            &[
                LocArg::new("permanentCount", 1_i64),
                LocArg::new("count", 3_i64),
                arg("fileName", "pair.jpg")
            ]
        )
    );
    assert_nothing_changed(&env, &before, 0);
}

// New test: Ready path hands the job to the file-action lane (07d queues it; checked here because 07c owns the entry point)
#[test]
#[ignore = "needs W8-07d execute_core"]
fn recycle_ready_path_queues_one_job_on_the_file_action_lane() {
    let mut env = Env::new(settings(false, false));
    env.load(&["a.jpg", "b.jpg"]);

    let step = env.recycle(None, Some(&p("a.jpg")));

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(env.port.queued(), 1);
    assert_eq!(*env.port.lanes.lock().unwrap(), vec![WorkerLane::FileAction]);
    assert!(env.controller.is_in_flight());
}
