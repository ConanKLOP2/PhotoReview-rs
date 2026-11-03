//! RP-JOURNAL-NOTICE-01 (P-72 option C / P-84 option A, C# PR #537): a file action that SUCCEEDED but whose journal line could
//! not be written (`journal_persisted == false`) writes one error log line (with `journal_error`) and shows
//! `status.actionJournalNotSaved` without blocking, for a single-file and a capture-group result alike. The notice is appended to
//! the statuses the success already writes (partner missing, all images processed, copied to), shown after the presenter when
//! nothing else wrote a status, and goes through the late-status channel (instead of the "press Ctrl+Z" hint) when the user
//! already left the folder. Port of C# `FileActionControllerJournalNoticeTests`. Fake executor and log, no disk, no Recycle Bin.

mod file_action_fakes;

use file_action_fakes::{Env, Scripted, action, arg, en, group_ok, member, p, single_ok, text};
use pr_app::coordinators::file_actions::recycle::{FileActionDone, FileActionStep};
use pr_core::model::FileOperationType::{self, Copy, Move};
use pr_core::settings::app_settings::AppSettings;

const DONE_TRUE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: true });
const DONE_FALSE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: false });
const JOURNAL_ERROR: &str = "simulated journal failure (disk full)";
const NOTICE_KEY: &str = "status.actionJournalNotSaved";

fn two_files_env(names: &[&str]) -> Env {
    let mut env = Env::new(AppSettings::default());
    env.load(names);
    env.set_actions(
        AppSettings::default(),
        vec![
            action("MoveToSub", Move, "Sorted", false),
            action("CopyIt", Copy, "Copies", false),
        ],
    );
    env
}

fn pair_env() -> Env {
    let mut env = Env::new(AppSettings::default());
    env.load_pair_between_two_photos(false);
    env.set_actions(AppSettings::default(), vec![action("MoveToSub", Move, "Sorted", false)]);
    env
}

fn unsaved_single(op: FileOperationType, source: &str, destination: &str) -> Scripted {
    let mut result = single_ok(op, source, Some(destination));
    result.journal_persisted = false;
    result.journal_error = Some(JOURNAL_ERROR.to_string());
    Scripted::Single(result)
}

fn group_result(journal_persisted: bool, skipped_missing: Option<Vec<String>>) -> Scripted {
    let mut result = group_ok(
        Move,
        vec![member(&p("pair.jpg"), Some("Sorted/pair.jpg"), true, false)],
        Some("Sorted/pair.jpg"),
    );
    result.journal_persisted = journal_persisted;
    result.journal_error = (!journal_persisted).then(|| JOURNAL_ERROR.to_string());
    result.skipped_missing = skipped_missing;
    Scripted::Group(result)
}

fn logged_errors(env: &Env) -> Vec<String> {
    env.log.errors.lock().unwrap().clone()
}

// C#: FileActionControllerJournalNoticeTests.SingleMove_SucceededButJournalNotPersisted_ShowsNoticeAndLogsOneErrorLine
#[test]
fn single_move_succeeded_but_journal_not_persisted_shows_notice_and_logs_one_error_line() {
    let mut env = two_files_env(&["a.jpg", "z.jpg"]);
    env.exec.script(unsaved_single(Move, &p("a.jpg"), "Sorted/a.jpg"));

    env.run_action(0, None, Some(&p("a.jpg")));
    assert_eq!(env.run_worker(), FileActionStep::Pending); // after the presenter
    assert_eq!(env.present_finished(false), DONE_TRUE);

    assert_eq!(env.sink.statuses, vec![text(NOTICE_KEY)]);
    let errors = logged_errors(&env);
    assert_eq!(
        errors.iter().filter(|m| m.contains(JOURNAL_ERROR)).count(),
        1,
        "{errors:?}"
    );
}

// C#: FileActionControllerJournalNoticeTests.SingleMove_SucceededAndJournalPersisted_ShowsNoNoticeAndLogsNothing
#[test]
fn single_move_succeeded_and_journal_persisted_shows_no_notice_and_logs_nothing() {
    let mut env = two_files_env(&["a.jpg", "z.jpg"]);
    env.exec
        .script(Scripted::Single(single_ok(Move, &p("a.jpg"), Some("Sorted/a.jpg"))));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();
    env.present_finished(false);

    assert!(!env.sink.statuses.contains(&text(NOTICE_KEY)));
    assert!(env.sink.late_statuses.is_empty());
    assert!(
        logged_errors(&env)
            .iter()
            .all(|m| !m.to_lowercase().contains("journal"))
    );
}

// C#: FileActionControllerJournalNoticeTests.SingleMove_JournalNotPersistedAfterFolderSwitch_AnnouncedThroughTheLateChannel
#[test]
fn single_move_journal_not_persisted_after_folder_switch_announced_through_the_late_channel() {
    let mut env = two_files_env(&["a.jpg", "z.jpg"]);
    env.exec.script(unsaved_single(Move, &p("a.jpg"), "Sorted/a.jpg"));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);

    // The notice REPLACES the "press Ctrl+Z" hint (status.lateMoveUndoable): Undo may not see this action.
    assert_eq!(env.sink.late_statuses, vec![text(NOTICE_KEY)]);
    assert!(env.sink.statuses.is_empty());
    assert!(logged_errors(&env).iter().any(|m| m.contains(JOURNAL_ERROR)));
}

// C#: FileActionControllerJournalNoticeTests.GroupMove_JournalPersistedFlag_DecidesWhetherTheNoticeAndLogLineAppear (false)
#[test]
fn group_move_journal_not_persisted_shows_notice_and_logs_the_error() {
    let mut env = pair_env();
    env.exec.script(group_result(false, None));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();
    env.present_finished(false);

    assert_eq!(env.sink.statuses, vec![text(NOTICE_KEY)]);
    assert!(logged_errors(&env).iter().any(|m| m.contains(JOURNAL_ERROR)));
}

// C#: FileActionControllerJournalNoticeTests.GroupMove_JournalPersistedFlag_DecidesWhetherTheNoticeAndLogLineAppear (true)
#[test]
fn group_move_journal_persisted_shows_no_notice_and_logs_no_error() {
    let mut env = pair_env();
    env.exec.script(group_result(true, None));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();
    env.present_finished(false);

    assert!(!env.sink.statuses.contains(&text(NOTICE_KEY)));
    assert!(logged_errors(&env).iter().all(|m| !m.contains(JOURNAL_ERROR)));
}

// Extra twins of the C# append branches (FileActionController.cs lines 407 to 430 at b6847767); the C# suite has no test for them.
#[test]
fn copy_journal_not_persisted_appends_the_notice_to_copied_to() {
    let mut env = two_files_env(&["a.jpg", "z.jpg"]);
    env.exec.script(unsaved_single(Copy, &p("a.jpg"), "Copies/a.jpg"));

    env.run_action(1, None, Some(&p("a.jpg")));
    env.run_worker();
    env.present_finished(false);

    let copied = en("status.copiedTo", &[arg("fileName", "a.jpg")]);
    assert_eq!(
        env.sink.last_status(),
        Some(format!("{copied} {}", text(NOTICE_KEY)).as_str())
    );
}

#[test]
fn group_move_partner_missing_appends_the_notice_to_the_partner_missing_status() {
    let mut env = pair_env();
    env.exec.script(group_result(false, Some(vec![p("pair.cr2")])));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.run_worker();
    assert_eq!(env.present_finished(false), DONE_TRUE);

    let missing = en(
        "status.groupPartnerMissing",
        &[arg("fileName", "pair.jpg"), arg("missing", "pair.cr2")],
    );
    assert_eq!(
        env.sink.last_status(),
        Some(format!("{missing} {}", text(NOTICE_KEY)).as_str())
    );
}

#[test]
fn move_that_empties_the_catalog_appends_the_notice_to_all_images_processed() {
    let mut env = two_files_env(&["a.jpg"]);
    env.exec.script(unsaved_single(Move, &p("a.jpg"), "Sorted/a.jpg"));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.run_worker();
    env.present_finished(false);

    let all = text("status.allImagesProcessed");
    assert_eq!(
        env.sink.last_status(),
        Some(format!("{all} {}", text(NOTICE_KEY)).as_str())
    );
}

#[test]
fn group_move_journal_not_persisted_after_folder_switch_announced_through_the_late_channel() {
    let mut env = pair_env();
    env.exec.script(group_result(false, None));

    env.run_action(0, None, Some(&p("pair.jpg")));
    env.clock.next_folder();
    assert_eq!(env.run_worker(), DONE_FALSE);

    assert_eq!(env.sink.late_statuses, vec![text(NOTICE_KEY)]);
    assert!(env.sink.statuses.is_empty());
}
