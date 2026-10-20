//! W8-07e: `move_or_copy_to_folder` (Move to / Copy to), `restores_outside_folder`, `reload_path_after_undo`, `is_in_folder`
//! (contract `contracts/W8-07e.md`; C# `FileActionController.cs` lines 723 to 858).
//!
//! Folders are real directories inside a `TempRoot` (the destination check asks the file system), the photos are only names in the
//! catalog (the executor is a fake), so nothing is moved on disk and the real Recycle Bin is never involved.
//!
//! Not ported: `MoveToFolder_PhotoRemovedFromCatalogWhilePickerOpen_ReportsStateChangedAndKeepsTheFile` removes the photo from the
//! catalog from inside the open picker; the catalog is `&mut` for the whole call in Rust, so that interleaving cannot happen. The
//! same re-check is covered by `..._another_action_became_busy_while_picker_open_...` and the folder-switch test.

mod file_action_fakes;

use std::cell::Cell;
use std::fs;
use std::rc::Rc;

use file_action_fakes::{
    Env, EnvOptions, ExecutorCall, FixedPicker, HidingFs, Scripted, arg, en, group_ok, member, single_fail, single_ok,
    text,
};
use pr_app::coordinators::file_actions::recycle::{FileActionDone, FileActionStep};
use pr_core::file_actions::types::UndoResult;
use pr_core::model::FileOperationType;
use pr_core::settings::app_settings::AppSettings;
use pr_test_support::TempRoot;

use FileOperationType::{Copy, Move, Recycle};

const DONE_FALSE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: false });
const DONE_TRUE: FileActionStep = FileActionStep::Done(FileActionDone::Finished { succeeded: true });

struct Fixture {
    root: TempRoot,
    photos: String,
    target: String,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = TempRoot::new(label);
        let photos = root.join("photos");
        let target = root.join("Target");
        fs::create_dir_all(&photos).expect("photos folder");
        fs::create_dir_all(&target).expect("target folder");
        Self {
            photos: photos.to_string_lossy().into_owned(),
            target: target.to_string_lossy().into_owned(),
            root,
        }
    }

    fn photo(&self, name: &str) -> String {
        format!("{}\\{name}", self.photos)
    }

    fn env(&self, settings: AppSettings, names: &[&str]) -> Env {
        let mut env = Env::new(settings);
        env.load_paths(&names.iter().map(|n| self.photo(n)).collect::<Vec<_>>());
        env
    }
}

fn reuse(target: &str) -> AppSettings {
    AppSettings {
        move_copy_reuse_last_folder: true,
        last_move_to_folder: Some(target.to_string()),
        last_copy_to_folder: Some(target.to_string()),
        ..AppSettings::default()
    }
}

fn src(compare: Option<String>, current: Option<String>) -> impl Fn() -> (Option<String>, Option<String>) {
    move || (compare.clone(), current.clone())
}

fn requests(env: &Env) -> Vec<(String, FileOperationType, Option<String>)> {
    env.exec
        .recorded()
        .into_iter()
        .filter_map(|c| match c {
            ExecutorCall::Single(r) => Some((r.source, r.operation, r.destination)),
            ExecutorCall::Group(_) => None,
        })
        .collect()
}

fn move_name() -> String {
    text("action.moveToFolder.name")
}

fn state_changed() -> String {
    en("status.moveCopyTo.stateChanged", &[arg("actionName", &move_name())])
}

// C#: FileActionControllerOutcomeGapTests.MoveToFolder_CompareSelectionDifferentFromCurrentAndLastFolderReused_MovesTheCompareSelection
#[test]
fn move_to_with_compare_selection_and_reused_last_folder_moves_the_compare_selection() {
    let f = Fixture::new("mc1");
    let mut env = f.env(reuse(&f.target), &["a.jpg", "b.jpg"]);

    let step = env
        .move_to_folder(None, Move, false, &src(Some(f.photo("b.jpg")), Some(f.photo("a.jpg"))))
        .expect("move");
    assert_eq!(step, FileActionStep::Pending);
    env.run_worker();

    let calls = requests(&env);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, f.photo("b.jpg"));
    assert_eq!(calls[0].1, Move);
    assert_eq!(calls[0].2.as_deref(), Some(f.target.as_str()));
    assert_eq!(env.catalog_paths(), vec![f.photo("a.jpg")]);
}

// C#: FileActionControllerOutcomeGapTests.MoveToFolder_PickerReturnsAndTheCompareSelectionStillWinsOverCurrent_Proceeds
#[test]
fn move_to_picker_returns_and_the_compare_selection_still_wins_over_current_proceeds() {
    let f = Fixture::new("mc2");
    let mut env = f.env(AppSettings::default(), &["a.jpg", "b.jpg"]);
    let mut picker = FixedPicker::new(Some(&f.target));

    let step = env
        .move_to_folder(
            Some(&mut picker),
            Move,
            true,
            &src(Some(f.photo("b.jpg")), Some(f.photo("a.jpg"))),
        )
        .expect("move");
    env.run_worker();

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(requests(&env)[0].0, f.photo("b.jpg"));
    assert!(!env.sink.statuses.contains(&state_changed()));
}

// C#: FileActionControllerOutcomeGapTests.MoveToFolder_PhotoAtLaterIndexStillInCatalogAfterPicker_Proceeds
#[test]
fn move_to_photo_at_later_index_still_in_catalog_after_picker_proceeds() {
    let f = Fixture::new("mc3");
    let mut env = f.env(AppSettings::default(), &["a.jpg", "b.jpg"]);
    let mut picker = FixedPicker::new(Some(&f.target));

    let step = env
        .move_to_folder(Some(&mut picker), Move, true, &src(None, Some(f.photo("b.jpg"))))
        .expect("move");
    env.run_worker();

    assert_eq!(step, FileActionStep::Pending);
    assert_eq!(requests(&env)[0].0, f.photo("b.jpg"));
    assert_eq!(env.catalog_paths(), vec![f.photo("a.jpg")]);
    // the picker was opened on the last folder or, failing that, the parent of the photo folder
    assert_eq!(picker.picked.len(), 1);
    assert_eq!(picker.picked[0].0, text("dialog.moveToFolder.title"));
}

// New test: the busy re-check of the picker condition (stands in for the C# "photo removed from the catalog while the picker is open")
#[test]
fn move_to_another_action_became_busy_while_picker_open_reports_state_changed_and_keeps_the_file() {
    let f = Fixture::new("mc4");
    let mut env = f.env(AppSettings::default(), &["a.jpg", "b.jpg"]);
    let exec = env.exec.clone();
    let mut picker = FixedPicker::new(Some(&f.target));
    picker.on_pick = Some(Box::new(move || exec.set_busy(true)));
    let before = env.catalog_paths();

    let step = env
        .move_to_folder(Some(&mut picker), Move, true, &src(None, Some(f.photo("b.jpg"))))
        .expect("move");

    assert_eq!(step, DONE_FALSE);
    assert_eq!(env.sink.last_status(), Some(state_changed().as_str()));
    assert_eq!(env.catalog_paths(), before);
    assert!(env.exec.recorded().is_empty());
    assert_eq!(env.port.queued(), 0);
}

// New test: a forwarded open switched the folder while the picker was open
#[test]
fn move_to_folder_switched_while_picker_open_reports_state_changed_and_keeps_the_file() {
    let f = Fixture::new("mc5");
    let mut env = f.env(AppSettings::default(), &["a.jpg", "b.jpg"]);
    let clock = env.clock.clone();
    let mut picker = FixedPicker::new(Some(&f.target));
    picker.on_pick = Some(Box::new(move || {
        clock.next_folder();
    }));

    let step = env
        .move_to_folder(Some(&mut picker), Move, true, &src(None, Some(f.photo("b.jpg"))))
        .expect("move");

    assert_eq!(step, DONE_FALSE);
    assert_eq!(env.sink.last_status(), Some(state_changed().as_str()));
    assert!(env.exec.recorded().is_empty());
}

// C#: FileActionControllerOutcomeGapTests.MoveToFolder_SelectionChangesWhilePickerOpen_ReportsStateChangedAndKeepsTheFile
#[test]
fn move_to_selection_changes_while_picker_open_reports_state_changed_and_keeps_the_file() {
    let f = Fixture::new("mc6");
    let mut env = f.env(AppSettings::default(), &["a.jpg", "b.jpg"]);
    let mut picker = FixedPicker::new(Some(&f.target));
    let calls = Rc::new(Cell::new(0));
    let (a, b) = (f.photo("a.jpg"), f.photo("b.jpg"));
    let counter = calls.clone();
    let get_source = move || {
        let n = counter.get();
        counter.set(n + 1);
        if n == 0 {
            (Some(b.clone()), Some(a.clone()))
        } else {
            (None, Some(a.clone()))
        }
    };

    let step = env
        .move_to_folder(Some(&mut picker), Move, true, &get_source)
        .expect("move");

    assert_eq!(step, DONE_FALSE);
    assert_eq!(env.sink.last_status(), Some(state_changed().as_str()));
    assert!(env.exec.recorded().is_empty());
}

// C#: FileActionControllerOutcomeGapTests.MoveToFolder_MovingTheOnlyPhoto_FinalStatusStaysAllImagesProcessed
#[test]
fn move_to_moving_the_only_photo_final_status_stays_all_images_processed() {
    let f = Fixture::new("mc7");
    let mut env = f.env(reuse(&f.target), &["a.jpg"]);

    env.move_to_folder(None, Move, false, &src(None, Some(f.photo("a.jpg"))))
        .expect("move");
    let step = env.run_worker();

    assert_eq!(step, DONE_TRUE);
    assert_eq!(env.sink.last_status(), Some(text("status.allImagesProcessed").as_str()));
    let moved = en(
        "status.movedToFolder",
        &[arg("fileName", "a.jpg"), arg("folder", &f.target)],
    );
    assert!(!env.sink.statuses.contains(&moved));
}

// C#: FileActionControllerOutcomeGapTests.MoveToFolder_WithMorePhotos_FinalStatusIsMovedToFolder
// C#: FileActionControllerOrderingTests.MoveToFolder_NextPresentStillRunning_FinalStatusIsMovedTo
#[test]
fn move_to_with_more_photos_final_status_is_moved_to_folder_after_the_next_present_finished() {
    let f = Fixture::new("mc8");
    let mut env = f.env(reuse(&f.target), &["a.jpg", "b.jpg"]);
    let moved = en(
        "status.movedToFolder",
        &[arg("fileName", "a.jpg"), arg("folder", &f.target)],
    );

    env.move_to_folder(None, Move, false, &src(None, Some(f.photo("a.jpg"))))
        .expect("move");
    // the file operation is over, the presenter of the next photo is still running: no final status yet (RV-A09)
    assert_eq!(env.run_worker(), FileActionStep::Pending);
    assert!(!env.sink.statuses.contains(&moved), "{:?}", env.sink.statuses);
    assert_eq!(env.sink.navigation_changes, 1, "the core's own finally already ran");

    // the presenter finishes and writes its own status; ours comes after it
    assert_eq!(env.present_finished(false), DONE_TRUE);

    assert_eq!(env.sink.last_status(), Some(moved.as_str()));
    assert_eq!(env.sink.navigation_changes, 1, "no second refresh for the final status");
}

// C#: FileActionControllerFolderIdentityTests.MoveTo_PresentCompletesAfterFolderSwitch_WritesNoMovedStatusIntoNewFolder (switchFolder axis)
#[test]
fn move_to_present_completes_after_folder_switch_writes_no_moved_status_into_new_folder() {
    for switch_folder in [false, true] {
        let f = Fixture::new("mc9");
        let mut env = f.env(reuse(&f.target), &["a.jpg", "b.jpg"]);
        let moved = en(
            "status.movedToFolder",
            &[arg("fileName", "a.jpg"), arg("folder", &f.target)],
        );

        env.move_to_folder(None, Move, false, &src(None, Some(f.photo("a.jpg"))))
            .expect("move");
        assert_eq!(env.run_worker(), FileActionStep::Pending); // parked on the present of the next photo
        if switch_folder {
            env.clock.next_folder();
        }
        env.present_finished(false);

        if switch_folder {
            assert!(!env.sink.statuses.contains(&moved), "switched: {:?}", env.sink.statuses);
        } else {
            assert_eq!(env.sink.statuses.last(), Some(&moved));
        }
    }
}

// C#: FileActionControllerGroupTests.MoveToFolder_PartnerVanishedExternally_KeepsTheCompanionMissingWarningInTheStatus
#[test]
fn move_to_partner_vanished_externally_keeps_the_companion_missing_warning_in_the_status() {
    let f = Fixture::new("mc10");
    let mut env = Env::new(reuse(&f.target));
    env.load_paths(&[
        f.photo("zbefore.jpg"),
        f.photo("pair.jpg"),
        f.photo("pair.cr2"),
        f.photo("aafter.jpg"),
    ]);
    let mut result = group_ok(
        Move,
        vec![member(
            &f.photo("pair.jpg"),
            Some(&format!("{}\\pair.jpg", f.target)),
            true,
            false,
        )],
        Some(&format!("{}\\pair.jpg", f.target)),
    );
    result.skipped_missing = Some(vec![f.photo("pair.cr2")]);
    env.exec.script(Scripted::Group(result));

    env.move_to_folder(None, Move, false, &src(None, Some(f.photo("pair.jpg"))))
        .expect("move");
    assert_eq!(env.run_worker(), FileActionStep::Pending); // the partner-missing tail waits for the present
    assert_eq!(env.present_finished(false), DONE_TRUE); // ... and the final status follows it
    let status = env.sink.last_status().expect("status").to_string();

    let missing = en(
        "status.groupPartnerMissing",
        &[arg("fileName", "pair.jpg"), arg("missing", "pair.cr2")],
    );
    let moved = en(
        "status.movedToFolder",
        &[arg("fileName", "pair.jpg"), arg("folder", &f.target)],
    );
    assert!(status.contains(&missing), "{status}");
    assert!(status.contains(&moved), "{status}");
}

// C#: FileActionControllerOutcomeGapTests.MoveToFolder_FileSystemSaysDestinationFolderMissing_ReportsMissingEvenWhenItExistsOnDisk
#[test]
fn move_to_file_system_says_destination_folder_missing_reports_missing_even_when_it_exists_on_disk() {
    let f = Fixture::new("mc11");
    let hiding = HidingFs::hiding(&f.target);
    let mut env = Env::with_options(
        AppSettings::default(),
        EnvOptions {
            file_system_override: Some(hiding),
            ..EnvOptions::default()
        },
    );
    env.load_paths(&[f.photo("a.jpg")]);
    let mut picker = FixedPicker::new(Some(&f.target));

    let step = env
        .move_to_folder(Some(&mut picker), Move, true, &src(None, Some(f.photo("a.jpg"))))
        .expect("move");

    assert_eq!(step, DONE_FALSE);
    assert_eq!(
        env.sink.last_status(),
        Some(
            en(
                "status.moveCopyTo.folderMissing",
                &[arg("actionName", &move_name()), arg("folder", &f.target)]
            )
            .as_str()
        )
    );
    assert!(env.exec.recorded().is_empty());
}

// New test
#[test]
fn unsupported_operation_returns_err() {
    let f = Fixture::new("mc12");
    let mut env = f.env(reuse(&f.target), &["a.jpg"]);

    let result = env.move_to_folder(None, Recycle, false, &src(None, Some(f.photo("a.jpg"))));

    assert_eq!(result, Err(Recycle));
    assert_eq!(env.catalog.count(), 1);
}

// New test: early returns without a status
#[test]
fn move_to_early_returns_do_nothing() {
    let f = Fixture::new("mc13");
    // empty catalog
    let mut env = Env::new(reuse(&f.target));
    assert_eq!(
        env.move_to_folder(None, Move, false, &src(None, Some(f.photo("a.jpg")))),
        Ok(DONE_FALSE)
    );
    // empty source
    let mut env = f.env(reuse(&f.target), &["a.jpg"]);
    assert_eq!(env.move_to_folder(None, Move, false, &src(None, None)), Ok(DONE_FALSE));
    // a picker is needed (nothing to reuse) but there is none
    let mut env = f.env(AppSettings::default(), &["a.jpg"]);
    assert_eq!(
        env.move_to_folder(None, Move, false, &src(None, Some(f.photo("a.jpg")))),
        Ok(DONE_FALSE)
    );
    // the user cancels the picker
    let mut picker = FixedPicker::new(None);
    assert_eq!(
        env.move_to_folder(Some(&mut picker), Move, true, &src(None, Some(f.photo("a.jpg")))),
        Ok(DONE_FALSE)
    );
    // busy executor
    let mut env = f.env(reuse(&f.target), &["a.jpg"]);
    env.exec.set_busy(true);
    assert_eq!(
        env.move_to_folder(None, Move, false, &src(None, Some(f.photo("a.jpg")))),
        Ok(DONE_FALSE)
    );
    assert!(env.sink.is_untouched());
    assert_eq!(env.port.queued(), 0);
}

// New test: the destination checks and their status texts
#[test]
fn move_to_destination_checks_report_their_status() {
    let f = Fixture::new("mc14");
    let name = move_name();
    let cases: [(String, String); 3] = [
        (
            f.photos.clone(),
            en("status.moveCopyTo.sameFolder", &[arg("actionName", &name)]),
        ),
        (
            f.root.join("nope").to_string_lossy().into_owned(),
            en(
                "status.moveCopyTo.folderMissing",
                &[
                    arg("actionName", &name),
                    arg("folder", &f.root.join("nope").to_string_lossy()),
                ],
            ),
        ),
        (
            "relative\\folder".to_string(),
            en("status.moveCopyTo.notAbsolute", &[arg("actionName", &name)]),
        ),
    ];
    for (destination, expected) in cases {
        let mut env = f.env(AppSettings::default(), &["a.jpg"]);
        let mut picker = FixedPicker::new(Some(&destination));
        let step = env
            .move_to_folder(Some(&mut picker), Move, true, &src(None, Some(f.photo("a.jpg"))))
            .expect("move");
        assert_eq!(step, DONE_FALSE, "{destination}");
        assert_eq!(env.sink.last_status(), Some(expected.as_str()), "{destination}");
        assert!(env.exec.recorded().is_empty());
    }
}

// New test: a NUL in the destination is a path error; D-07e-1: it becomes the "choose a full folder path" status
#[test]
fn path_error_in_validation_reports_not_absolute() {
    let f = Fixture::new("mc15");
    let mut env = f.env(AppSettings::default(), &["a.jpg"]);
    let mut picker = FixedPicker::new(Some(&format!("{}\0bad", f.target)));

    let step = env
        .move_to_folder(Some(&mut picker), Copy, true, &src(None, Some(f.photo("a.jpg"))))
        .expect("copy");

    assert_eq!(step, DONE_FALSE);
    let copy_name = text("action.copyToFolder.name");
    assert_eq!(
        env.sink.last_status(),
        Some(en("status.moveCopyTo.notAbsolute", &[arg("actionName", &copy_name)]).as_str())
    );
}

// New test: the folder is remembered once the core succeeded, not when it is the same folder in another case, not when the core failed
#[test]
fn remember_folder_is_called_for_a_new_folder_only_and_not_after_a_failure() {
    let f = Fixture::new("mc16");

    // a different folder than the last one: remembered
    let other_last = f.root.join("Elsewhere");
    fs::create_dir_all(&other_last).expect("elsewhere");
    let mut env = f.env(reuse(&other_last.to_string_lossy()), &["a.jpg", "b.jpg"]);
    let mut first_picker = FixedPicker::new(Some(&f.target));
    env.move_to_folder(Some(&mut first_picker), Move, true, &src(None, Some(f.photo("a.jpg"))))
        .expect("move");
    env.run_worker();
    env.present_finished(false);
    assert_eq!(*env.remembered.lock().unwrap(), vec![(Move, f.target.clone())]);

    // the same folder spelled in another case: not remembered again
    let mut env = f.env(reuse(&f.target.to_uppercase()), &["a.jpg", "b.jpg"]);
    let mut picker = FixedPicker::new(Some(&f.target));
    env.move_to_folder(Some(&mut picker), Move, true, &src(None, Some(f.photo("a.jpg"))))
        .expect("move");
    env.run_worker();
    env.present_finished(false);
    assert!(
        env.remembered.lock().unwrap().is_empty(),
        "{:?}",
        env.remembered.lock().unwrap()
    );

    // the core failed: nothing is remembered and no final status is written
    let mut env = f.env(AppSettings::default(), &["a.jpg", "b.jpg"]);
    env.exec
        .script(Scripted::Single(single_fail(Move, &f.photo("a.jpg"), "locked")));
    let mut picker = FixedPicker::new(Some(&f.target));
    env.move_to_folder(Some(&mut picker), Move, true, &src(None, Some(f.photo("a.jpg"))))
        .expect("move");
    env.run_worker();
    env.present_finished(false);
    assert!(env.remembered.lock().unwrap().is_empty());
    assert!(!env.sink.statuses.iter().any(|s| s.contains(&f.target)));
}

// New test: Copy to: a Copy leaves the catalog alone, no present to wait for, the final status is written at once
#[test]
fn copy_to_writes_the_copied_to_folder_status_without_waiting() {
    let f = Fixture::new("mc17");
    let mut env = f.env(reuse(&f.target), &["a.jpg", "b.jpg"]);
    env.exec.script(Scripted::Single(single_ok(
        Copy,
        &f.photo("a.jpg"),
        Some(&format!("{}\\a.jpg", f.target)),
    )));

    env.move_to_folder(None, Copy, false, &src(None, Some(f.photo("a.jpg"))))
        .expect("copy");
    let step = env.run_worker();

    assert_eq!(step, DONE_TRUE);
    assert_eq!(
        env.sink.last_status(),
        Some(
            en(
                "status.copiedToFolder",
                &[arg("fileName", "a.jpg"), arg("folder", &f.target)]
            )
            .as_str()
        )
    );
    assert_eq!(env.catalog.count(), 2);
    // the folder is the remembered one already: nothing to remember
    assert!(env.remembered.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------------------------------------------

fn undo(ok: bool, op: Option<FileOperationType>, source: &str, restored: Option<Vec<String>>) -> UndoResult {
    let mut r = UndoResult::new(ok, op, source, None, None);
    r.restored_paths = restored;
    r
}

// C#: FileActionControllerGapTests.RestoresOutsideFolder_RecycleUndo_IsInsideOnlyForItsOwnFolder
// C#: FileActionControllerGapTests.RestoresOutsideFolder_MoveUndoWithNullOrEmptyCurrentFolder_CountsAsOutside (theory)
// C#: FileActionControllerGapTests.RestoresOutsideFolder_MoveUndoInCurrentFolderWithCaseAndTrailingSeparatorVariants_IsInside
// C#: FileActionControllerGapTests.RestoresOutsideFolder_MoveUndoRestoredPathsWithoutTheSource_JudgesByTheRestoredPaths
// C#: FileActionControllerGapTests.RestoresOutsideFolder_NullFailedOrCopyOrEmptySourceResult_IsFalse
// C#: FileActionControllerOutcomeGapTests.RestoresOutsideFolder_CurrentFolderWithInvalidCharacters_CountsAsOutsideInsteadOfThrowing
#[test]
fn restores_outside_folder_cases() {
    use pr_app::coordinators::file_actions::recycle::FileActionController as C;
    let root = TempRoot::new("mc-restores");
    let r = root.path().to_string_lossy().into_owned();
    let a = format!("{r}\\a.jpg");
    let sep = std::path::MAIN_SEPARATOR;

    // (label, result, current folder, expected)
    let cases: Vec<(&str, Option<UndoResult>, Option<String>, bool)> = vec![
        (
            "recycle in its own folder",
            Some(undo(true, Some(Recycle), &a, None)),
            Some(r.clone()),
            false,
        ),
        (
            "recycle, other folder",
            Some(undo(true, Some(Recycle), &a, None)),
            Some(format!("{r}\\other")),
            true,
        ),
        (
            "recycle, null folder",
            Some(undo(true, Some(Recycle), &a, None)),
            None,
            true,
        ),
        (
            "recycle, empty folder",
            Some(undo(true, Some(Recycle), &a, None)),
            Some(String::new()),
            true,
        ),
        (
            "capture with a member restored in another folder",
            Some(undo(
                true,
                Some(Recycle),
                &a,
                Some(vec![a.clone(), format!("{r}\\other\\a.cr2")]),
            )),
            Some(r.clone()),
            true,
        ),
        ("move, null folder", Some(undo(true, Some(Move), &a, None)), None, true),
        (
            "move, empty folder",
            Some(undo(true, Some(Move), &a, None)),
            Some(String::new()),
            true,
        ),
        (
            "move, upper case and trailing separator",
            Some(undo(true, Some(Move), &a, None)),
            Some(format!("{}{sep}", r.to_uppercase())),
            false,
        ),
        (
            "move, plain folder",
            Some(undo(true, Some(Move), &a, None)),
            Some(r.clone()),
            false,
        ),
        (
            "restored paths without the source, all inside",
            Some(undo(
                true,
                Some(Move),
                &a,
                Some(vec![format!("{r}\\a.cr2").to_uppercase()]),
            )),
            Some(r.clone()),
            false,
        ),
        (
            "restored paths without the source, one outside",
            Some(undo(
                true,
                Some(Move),
                &a,
                Some(vec![format!("{r}\\a.cr2"), format!("{r}\\elsewhere\\a.xmp")]),
            )),
            Some(r.clone()),
            true,
        ),
        ("null result", None, Some(r.clone()), false),
        (
            "failed result",
            Some(undo(false, Some(Recycle), &a, None)),
            Some(r.clone()),
            false,
        ),
        (
            "copy result",
            Some(undo(true, Some(Copy), &a, None)),
            Some(r.clone()),
            false,
        ),
        (
            "copy result, other folder",
            Some(undo(true, Some(Copy), &a, None)),
            Some(format!("{r}\\other")),
            false,
        ),
        ("no operation", Some(undo(true, None, &a, None)), Some(r.clone()), false),
        (
            "empty source",
            Some(undo(true, Some(Recycle), "", None)),
            Some(r.clone()),
            false,
        ),
        (
            "current folder with an invalid character",
            Some(undo(true, Some(Move), &a, None)),
            Some(format!("{r}\0bad")),
            true,
        ),
    ];
    for (label, result, folder, expected) in cases {
        assert_eq!(
            C::restores_outside_folder(result.as_ref(), folder.as_deref()),
            expected,
            "{label}"
        );
    }
}

// C#: FileActionControllerGapTests.ReloadPathAfterUndo_SourceRestoredUnderADifferentCase_KeepsTheSourceSpelling
// C#: FileActionControllerGapTests.ReloadPathAfterUndo_NoRestoredPathsOrOnlySidecars_FallsBackToTheSource
// C#: FileActionControllerGapTests.ReloadPathAfterUndo_EmptySourceWithRestoredImage_PicksTheImage
// C#: FileActionControllerGroupTests.ReloadPathAfterUndo_SourceWasNotRestored_PicksARestoredImageMember
// C#: FileActionControllerGroupTests.ReloadPathAfterUndo_SourceWasRestored_KeepsTheSource
#[test]
fn reload_path_after_undo_cases() {
    use pr_app::coordinators::file_actions::recycle::FileActionController as C;
    let r = "dir";
    let cases: Vec<(&str, UndoResult, Option<String>)> = vec![
        (
            "source restored under another case keeps the source spelling",
            undo(
                true,
                Some(Recycle),
                &format!("{r}\\A.jpg"),
                Some(vec![format!("{r}\\a.cr2"), format!("{r}\\a.jpg")]),
            ),
            Some(format!("{r}\\A.jpg")),
        ),
        (
            "no restored paths",
            undo(true, Some(Recycle), &format!("{r}\\gone.jpg"), None),
            Some(format!("{r}\\gone.jpg")),
        ),
        (
            "empty restored paths",
            undo(true, Some(Recycle), &format!("{r}\\gone.jpg"), Some(vec![])),
            Some(format!("{r}\\gone.jpg")),
        ),
        (
            "only a sidecar restored",
            undo(
                true,
                Some(Recycle),
                &format!("{r}\\gone.jpg"),
                Some(vec![format!("{r}\\kept.xmp")]),
            ),
            Some(format!("{r}\\gone.jpg")),
        ),
        (
            "empty source with a restored image",
            undo(true, Some(Recycle), "", Some(vec![format!("{r}\\kept.cr2")])),
            Some(format!("{r}\\kept.cr2")),
        ),
        (
            "source not restored: the first restored image member, never the sidecar",
            undo(
                true,
                Some(Recycle),
                &format!("{r}\\gone.jpg"),
                Some(vec![format!("{r}\\kept.xmp"), format!("{r}\\kept.cr2")]),
            ),
            Some(format!("{r}\\kept.cr2")),
        ),
        (
            "source restored keeps the source",
            undo(
                true,
                Some(Recycle),
                &format!("{r}\\a.jpg"),
                Some(vec![format!("{r}\\a.cr2"), format!("{r}\\a.jpg")]),
            ),
            Some(format!("{r}\\a.jpg")),
        ),
    ];
    for (label, result, expected) in cases {
        assert_eq!(C::reload_path_after_undo(&result), expected, "{label}");
    }
}
