//! W8-07d/07e: `undo_last`, the remembered Move positions (`remember_move_position`, `insert_restored_move`,
//! `sorted_insert_index`) and the pure helpers `restores_outside_folder`, `reload_path_after_undo`
//! (contracts `contracts/W8-07d.md`, `contracts/W8-07e.md`; C# `FileActionController.cs` lines 529 to 590 and 619 to 721, 815 to 857).
//!
//! The undo SERVICE (the file moves, the fingerprints, the Recycle Bin restore) is tested in `pr_core`; here its outcome is a scripted
//! `UndoResult` delivered through the worker event (`Env::undo_with`), so the controller logic (catalog, session, present, status) runs
//! without a disk. The action that precedes an undo goes through the real controller with a scripted executor result, so the move
//! position is really remembered by `on_executed`.

mod file_action_fakes;

use file_action_fakes::{Env, Scripted, action, group_ok, member, p, single_ok, text};
use pr_app::coordinators::file_actions::recycle::{FileActionController, FileActionDone, FileActionStep};
use pr_core::file_actions::types::UndoResult;
use pr_core::model::{FileOperationType, RawPairMode};
use pr_core::settings::app_settings::AppSettings;

use FileOperationType::{Copy, Move, Recycle};

const FOLDER: &str = "photos";

fn undone_ok(step: &FileActionStep) -> bool {
    matches!(step, FileActionStep::Done(FileActionDone::Undone(Some(r))) if r.succeeded)
}

/// An undo whose present is parked (a Move restored in place): the test presenter finishes it.
fn undo_and_finish(env: &mut Env, undo: UndoResult, folder: Option<&str>) -> FileActionStep {
    let step = env.undo_with(undo, folder);
    if step == FileActionStep::Pending {
        env.present_finished(false)
    } else {
        step
    }
}

fn result(
    ok: bool,
    op: FileOperationType,
    source: &str,
    restored: Option<Vec<String>>,
    error: Option<&str>,
) -> UndoResult {
    let mut r = UndoResult::new(ok, Some(op), source, None, error.map(str::to_string));
    r.restored_paths = restored;
    r
}

fn move_env(names: &[&str]) -> Env {
    let mut env = Env::new(AppSettings::default());
    env.load(names);
    env.set_actions(
        AppSettings::default(),
        vec![
            action("MoveToSub", Move, "Sorted", false),
            action("DeleteIt", Recycle, "", false),
        ],
    );
    env
}

fn pair_env(mode: RawPairMode, with_xmp: bool) -> Env {
    let mut env = Env::new(AppSettings::default());
    env.load_pair_with_mode(mode, with_xmp);
    env.set_actions(
        AppSettings::default(),
        vec![
            action("MoveToSub", Move, "Sorted", false),
            action("DeleteIt", Recycle, "", false),
        ],
    );
    env
}

/// Moves `name` (succeeds, others remain) so its position is remembered.
fn do_move(env: &mut Env, name: &str) {
    env.exec
        .script(Scripted::Single(single_ok(Move, &p(name), Some("photos/Sorted/x.jpg"))));
    env.run_action(0, None, Some(&p(name)));
    env.run_worker();
}

fn do_recycle(env: &mut Env, name: &str) {
    env.run_action(1, None, Some(&p(name)));
    env.run_worker();
}

fn do_group_move(env: &mut Env, representative: &str) {
    env.exec.script(Scripted::Group(group_ok(
        Move,
        vec![
            member(&p("pair.jpg"), Some("photos/Sorted/pair.jpg"), true, false),
            member(&p("pair.cr2"), Some("photos/Sorted/pair.cr2"), true, false),
        ],
        Some("photos/Sorted/pair.jpg"),
    )));
    env.run_action(0, None, Some(&p(representative)));
    env.run_worker();
}

fn do_group_recycle(env: &mut Env) {
    env.exec.script(Scripted::Group(group_ok(
        Recycle,
        vec![
            member(&p("pair.jpg"), None, true, false),
            member(&p("pair.cr2"), None, true, false),
        ],
        None,
    )));
    env.run_action(1, None, Some(&p("pair.jpg")));
    env.run_worker();
}

fn pair_paths() -> Vec<String> {
    vec![p("pair.jpg"), p("pair.cr2")]
}

// ---------------------------------------------------------------------------------------------------------------
// Move positions
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerOrderingTests.UndoMove_RestoresReviewOrderPosition_NotNameOrder
#[test]
fn undo_move_restores_review_order_position_not_name_order() {
    let mut env = move_env(&["c.jpg", "a.jpg", "b.jpg"]); // e.g. Z-A / size / Explorer order
    do_move(&mut env, "a.jpg");
    assert_eq!(env.catalog_paths(), vec![p("c.jpg"), p("b.jpg")]);

    let step = undo_and_finish(&mut env, result(true, Move, &p("a.jpg"), None, None), Some(FOLDER));

    assert!(undone_ok(&step));
    assert_eq!(env.catalog_paths(), vec![p("c.jpg"), p("a.jpg"), p("b.jpg")]); // name order would give a, c, b
}

// C#: FileActionControllerOrderingTests.UndoMove_OfFirstPhoto_RestoresAtFront
#[test]
fn undo_move_of_first_photo_restores_at_front() {
    let mut env = move_env(&["z.jpg", "a.jpg"]);
    do_move(&mut env, "z.jpg");

    env.undo_with(result(true, Move, &p("z.jpg"), None, None), Some(FOLDER));

    assert_eq!(env.catalog_paths(), vec![p("z.jpg"), p("a.jpg")]);
}

// C#: FileActionControllerOutcomeGapTests.UndoLast_MoveWhosePredecessorIsFirstInTheCatalog_ReturnsAfterItNotByName
#[test]
fn undo_last_move_whose_predecessor_is_first_in_the_catalog_returns_after_it_not_by_name() {
    let mut env = move_env(&["z.jpg", "a.jpg"]);
    do_move(&mut env, "a.jpg");

    let step = undo_and_finish(&mut env, result(true, Move, &p("a.jpg"), None, None), Some(FOLDER));

    assert!(undone_ok(&step));
    assert_eq!(env.catalog_paths(), vec![p("z.jpg"), p("a.jpg")]); // by name it would be a, z
}

// C#: FileActionControllerOutcomeGapTests.UndoLast_MoveWhosePredecessorLeftTheCatalog_FallsBackToNaturalNameOrder
#[test]
fn undo_last_move_whose_predecessor_left_the_catalog_falls_back_to_natural_name_order() {
    let mut env = move_env(&["a.jpg", "b.jpg", "c.jpg", "d.jpg"]);
    do_move(&mut env, "c.jpg");
    env.catalog.remove(&p("b.jpg")); // the predecessor is gone

    let step = undo_and_finish(&mut env, result(true, Move, &p("c.jpg"), None, None), Some(FOLDER));

    assert!(undone_ok(&step));
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("c.jpg"), p("d.jpg")]);
}

// New test: the remembered positions are bounded (MAX_REMEMBERED_MOVE_POSITIONS = 256): when the map is full it is cleared, and an
// older move then falls back to the name order, while the newest one is still remembered.
#[test]
fn remembered_move_positions_are_cleared_when_the_map_holds_the_maximum() {
    let names: Vec<String> = (0..260).map(|i| format!("f{:03}.jpg", 259 - i)).collect(); // review order is DESCENDING by name
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut env = move_env(&refs);
    for name in &names[..257] {
        do_move(&mut env, name);
    }
    assert_eq!(env.catalog.count(), 3);

    // the first move (f259, at the front) was forgotten: the natural name order puts it last
    env.undo_with(result(true, Move, &p(&names[0]), None, None), Some(FOLDER));
    assert_eq!(env.catalog.path_at(env.catalog.count() - 1), p(&names[0]));

    // the last move is still remembered at its review position (the front: its predecessor was nothing)
    env.undo_with(result(true, Move, &p(&names[256]), None, None), Some(FOLDER));
    assert_eq!(env.catalog.path_at(0), p(&names[256]));
}

// New test: taking the remembered position of a capture forgets it under EVERY member path, so a later undo that names another
// member does not reuse the stale review position.
#[test]
fn taking_a_capture_position_forgets_every_member_path() {
    let mut env = Env::new(AppSettings::default());
    env.load(&["zz.jpg", "pair.jpg", "pair.cr2", "aa.jpg"]); // review order z, pair, a (descending by name)
    env.set_actions(AppSettings::default(), vec![action("MoveToSub", Move, "Sorted", false)]);
    do_group_move(&mut env, "pair.jpg");
    env.undo_with(
        result(true, Move, &p("pair.jpg"), Some(pair_paths()), None),
        Some(FOLDER),
    );
    assert_eq!(env.catalog_paths(), vec![p("zz.jpg"), p("pair.jpg"), p("aa.jpg")]);

    // the capture leaves again by another way; undoing it names the RAW member this time and no position is remembered for it
    env.catalog.remove(&p("pair.jpg"));
    env.undo_with(result(true, Move, &p("pair.cr2"), None, None), Some(FOLDER));
    env.present_finished(false);

    // by name order: "pair.cr2" is not greater than "zz.jpg", so it goes first; a stale position would put it after "zz.jpg"
    assert_eq!(env.catalog.index_of(&p("pair.cr2")), 0);
}

// ---------------------------------------------------------------------------------------------------------------
// Group undo
// ---------------------------------------------------------------------------------------------------------------

// C#: FileActionControllerGroupTests.UndoGroupMove_RestoresOneGroupedEntryAtItsFormerPosition_AndNeverListsTheSidecar (theory)
#[test]
fn undo_group_move_restores_one_grouped_entry_at_its_former_position_and_never_lists_the_sidecar() {
    for (mode, representative) in [
        (RawPairMode::PreferJpeg, "pair.jpg"),
        (RawPairMode::PreferRaw, "pair.cr2"),
    ] {
        let mut env = pair_env(mode, true);
        do_group_move(&mut env, representative);
        assert_eq!(env.catalog_paths(), vec![p("zbefore.jpg"), p("aafter.jpg")]);

        let restored = vec![p("pair.jpg"), p("pair.cr2"), p("pair.xmp")];
        let step = undo_and_finish(
            &mut env,
            result(true, Move, &p(representative), Some(restored), None),
            Some(FOLDER),
        );

        assert!(undone_ok(&step));
        assert_eq!(
            env.catalog_paths(),
            vec![p("zbefore.jpg"), p(representative), p("aafter.jpg")],
            "{mode:?}"
        ); // not index 0
        let entry = env.catalog.find(&p(representative)).expect("entry");
        assert_eq!(
            entry.capture_group.as_ref().and_then(|g| g.xmp_path()),
            Some(p("pair.xmp").as_str())
        );
        assert_eq!(env.catalog.index_of(&p("pair.xmp")), -1);
        assert_eq!(env.catalog.index_of(&p("pair.jpg")), 1);
        assert_eq!(env.catalog.index_of(&p("pair.cr2")), 1);
    }
}

// C#: FileActionControllerGroupTests.UndoGroupMove_OfTheFirstPhoto_RestoresAtFront
#[test]
fn undo_group_move_of_the_first_photo_restores_at_front() {
    let mut env = Env::new(AppSettings::default());
    env.load(&["pair.jpg", "pair.cr2", "b.jpg"]);
    env.set_actions(AppSettings::default(), vec![action("MoveToSub", Move, "Sorted", false)]);
    do_group_move(&mut env, "pair.jpg");

    env.undo_with(
        result(true, Move, &p("pair.jpg"), Some(pair_paths()), None),
        Some(FOLDER),
    );

    assert_eq!(env.catalog_paths(), vec![p("pair.jpg"), p("b.jpg")]);
    assert!(
        env.catalog
            .find(&p("pair.jpg"))
            .is_some_and(|e| e.capture_group.is_some())
    );
}

// C#: FileActionControllerGroupTests.UndoGroupRecycle_RestoresTheCaptureInPlaceAsOneEntry_WithoutPresentingOrReloading
#[test]
fn undo_group_recycle_restores_the_capture_in_place_as_one_entry_without_presenting_or_reloading() {
    let mut env = pair_env(RawPairMode::PreferJpeg, false);
    assert!(env.catalog.set_current(env.catalog.index_of(&p("pair.jpg"))));
    do_group_recycle(&mut env);
    assert_eq!(
        env.catalog.current().map(|e| e.path().to_string()),
        Some(p("aafter.jpg"))
    );
    let presents_before = env.sink.presented.len();

    let undo = result(true, Recycle, &p("pair.jpg"), Some(pair_paths()), None);
    let step = env.undo_with(undo.clone(), Some(FOLDER));

    assert!(undone_ok(&step));
    assert_eq!(
        env.catalog_paths(),
        vec![p("zbefore.jpg"), p("pair.jpg"), p("aafter.jpg")]
    );
    assert!(
        env.catalog
            .find(&p("pair.jpg"))
            .is_some_and(|e| e.capture_group.is_some())
    );
    assert_eq!(
        env.catalog.current().map(|e| e.path().to_string()),
        Some(p("aafter.jpg"))
    ); // review stays on the shown photo
    assert_eq!(env.sink.presented.len(), presents_before);
    assert!(!FileActionController::restores_outside_folder(
        Some(&undo),
        Some(FOLDER)
    )); // the view model does not reload
}

// C#: FileActionControllerGroupTests.UndoGroupRecycle_FailingMidWay_ShowsTheAlreadyRestoredMemberInTheCatalog
#[test]
fn undo_group_recycle_failing_mid_way_shows_the_already_restored_member_in_the_catalog() {
    let mut env = pair_env(RawPairMode::PreferJpeg, false);
    do_group_recycle(&mut env);
    assert_eq!(env.catalog_paths(), vec![p("zbefore.jpg"), p("aafter.jpg")]);

    let failed = result(
        false,
        Recycle,
        &p("pair.jpg"),
        Some(vec![p("pair.jpg")]),
        Some("restore failed"),
    );
    let step = env.undo_with(failed, Some(FOLDER));

    assert!(!undone_ok(&step));
    assert!(env.catalog.index_of(&p("pair.jpg")) >= 0); // reflected without waiting for a reload
    assert_eq!(env.sink.last_status(), Some("restore failed"));
}

// C#: FileActionControllerGroupTests.UndoGroupMove_FailingMidWay_AppliesTheRestoredMembersEvenThoughItFailed
#[test]
fn undo_group_move_failing_mid_way_applies_the_restored_members_even_though_it_failed() {
    let mut env = pair_env(RawPairMode::PreferJpeg, false);
    do_group_move(&mut env, "pair.jpg");

    let failed = result(
        false,
        Move,
        &p("pair.jpg"),
        Some(vec![p("pair.jpg")]),
        Some("simulated undo failure"),
    );
    let step = env.undo_with(failed, Some(FOLDER));

    assert!(!undone_ok(&step));
    assert_eq!(
        env.catalog_paths(),
        vec![p("zbefore.jpg"), p("pair.jpg"), p("aafter.jpg")]
    );
    // ungrouped: its partner is not back
    assert!(
        env.catalog
            .find(&p("pair.jpg"))
            .is_some_and(|e| e.capture_group.is_none())
    );
}

// C#: FileActionControllerGroupTests.UndoGroupMove_FailedThenRetried_FoldsTheMembersBackIntoOneGroupedEntry
#[test]
fn undo_group_move_failed_then_retried_folds_the_members_back_into_one_grouped_entry() {
    let mut env = pair_env(RawPairMode::PreferJpeg, false);
    do_group_move(&mut env, "pair.jpg");

    let failed = result(
        false,
        Move,
        &p("pair.jpg"),
        Some(vec![p("pair.jpg")]),
        Some("simulated undo failure"),
    );
    assert!(!undone_ok(&env.undo_with(failed, Some(FOLDER))));
    assert!(
        env.catalog
            .find(&p("pair.jpg"))
            .is_some_and(|e| e.capture_group.is_none())
    );

    // the retry restores the rest; the remembered position (and its group) survived the failed attempt (keep_position)
    let retry = result(true, Move, &p("pair.jpg"), Some(pair_paths()), None);
    let step = undo_and_finish(&mut env, retry, Some(FOLDER));

    assert!(undone_ok(&step));
    assert_eq!(
        env.catalog_paths(),
        vec![p("zbefore.jpg"), p("pair.jpg"), p("aafter.jpg")]
    ); // one entry, not two
    assert!(
        env.catalog
            .find(&p("pair.jpg"))
            .is_some_and(|e| e.capture_group.is_some())
    );
}

// C#: FileActionControllerGroupTests.UndoGroupRecycle_FailingMidWay_WhenTheOnlyCaptureWasRemoved_PresentsTheRestoredEntry
// C#: FileActionControllerGapTests.UndoLastAsync_PartialCaptureUndoIntoEmptyCatalog_FolderStaysCurrent_PresentsOnceAndWritesSessionAndStatus
#[test]
fn undo_group_recycle_failing_mid_way_when_the_only_capture_was_removed_presents_the_restored_entry() {
    let mut env = Env::new(AppSettings::default());
    env.load(&["only.jpg", "only.cr2"]);
    env.set_actions(AppSettings::default(), vec![action("DeleteIt", Recycle, "", false)]);
    env.exec.script(Scripted::Group(group_ok(
        Recycle,
        vec![
            member(&p("only.jpg"), None, true, false),
            member(&p("only.cr2"), None, true, false),
        ],
        None,
    )));
    env.run_action(0, None, Some(&p("only.jpg")));
    env.run_worker();
    assert_eq!(env.catalog.count(), 0);
    env.sink.presented.clear();
    env.sink.statuses.clear();
    env.sink.sessions.clear();

    let failed = result(
        false,
        Recycle,
        &p("only.jpg"),
        Some(vec![p("only.jpg")]),
        Some("restore failed"),
    );
    assert_eq!(env.undo_with(failed, Some(FOLDER)), FileActionStep::Pending); // parked behind the present
    assert_eq!(env.catalog_paths(), vec![p("only.jpg")]);
    assert_eq!(env.sink.presented, vec![0]); // otherwise the view stays blank although HasImages is true
    let step = env.present_finished(false);

    assert!(matches!(step, FileActionStep::Done(FileActionDone::Undone(Some(_)))));
    assert_eq!(env.sink.presented, vec![0], "presented once");
    assert_eq!(env.sink.sessions, vec![p("only.jpg")]);
    assert_eq!(env.sink.statuses, vec!["restore failed".to_string()]);
}

// C#: FileActionControllerGapTests.UndoLastAsync_PartialCaptureUndoIntoEmptyCatalog_FolderSwitchedDuringPresent_WritesNoSessionOrStatus
#[test]
fn undo_last_partial_capture_undo_into_empty_catalog_folder_switched_during_present_writes_no_session_or_status() {
    let mut env = Env::new(AppSettings::default());
    env.load(&["only.jpg", "only.cr2"]);
    env.set_actions(AppSettings::default(), vec![action("DeleteIt", Recycle, "", false)]);
    env.exec.script(Scripted::Group(group_ok(
        Recycle,
        vec![
            member(&p("only.jpg"), None, true, false),
            member(&p("only.cr2"), None, true, false),
        ],
        None,
    )));
    env.run_action(0, None, Some(&p("only.jpg")));
    env.run_worker();
    env.sink.presented.clear();
    env.sink.statuses.clear();
    env.sink.sessions.clear();
    let navigation_before = env.sink.navigation_changes;

    let failed = result(
        false,
        Recycle,
        &p("only.jpg"),
        Some(vec![p("only.jpg")]),
        Some("restore failed"),
    );
    env.undo_with(failed, Some(FOLDER));
    // the user opens another folder while the restored entry is being presented
    env.clock.next_folder();
    env.load(&["other.jpg"]);
    let step = env.present_finished(false);

    assert!(matches!(step, FileActionStep::Done(FileActionDone::Undone(Some(_)))));
    assert_eq!(env.sink.presented, vec![0]);
    assert!(env.sink.sessions.is_empty());
    assert!(env.sink.statuses.is_empty());
    assert_eq!(env.sink.navigation_changes, navigation_before);
    assert_eq!(env.catalog_paths(), vec![p("other.jpg")]);
}

// C#: FileActionControllerGroupTests.UndoGroupRecycle_FailingMidWay_WhileAnotherPhotoIsShown_KeepsItDisplayed
#[test]
fn undo_group_recycle_failing_mid_way_while_another_photo_is_shown_keeps_it_displayed() {
    let mut env = pair_env(RawPairMode::PreferJpeg, false);
    assert!(env.catalog.set_current(env.catalog.index_of(&p("pair.jpg"))));
    do_group_recycle(&mut env);
    assert_eq!(
        env.catalog.current().map(|e| e.path().to_string()),
        Some(p("aafter.jpg"))
    );
    let presents_before = env.sink.presented.len();

    env.undo_with(
        result(
            false,
            Recycle,
            &p("pair.jpg"),
            Some(vec![p("pair.jpg")]),
            Some("restore failed"),
        ),
        Some(FOLDER),
    );

    assert_eq!(env.sink.presented.len(), presents_before);
    assert_eq!(
        env.catalog.current().map(|e| e.path().to_string()),
        Some(p("aafter.jpg"))
    );
}

// C#: FileActionControllerGroupTests.UndoGroupRecycle_FailingMidWay_RestoresTheMemberAtTheOriginalPosition
#[test]
fn undo_group_recycle_failing_mid_way_restores_the_member_at_the_original_position() {
    let mut env = pair_env(RawPairMode::PreferJpeg, false);
    do_group_recycle(&mut env);

    env.undo_with(
        result(
            false,
            Recycle,
            &p("pair.jpg"),
            Some(vec![p("pair.jpg")]),
            Some("restore failed"),
        ),
        Some(FOLDER),
    );

    assert_eq!(
        env.catalog_paths(),
        vec![p("zbefore.jpg"), p("pair.jpg"), p("aafter.jpg")]
    ); // not appended at the end
}

// C#: FileActionControllerGroupTests.UndoGroupMove_MembersSplitAcrossFolders_DoesNotInsertOrPresentBecauseTheCallerReloads
#[test]
fn undo_group_move_members_split_across_folders_does_not_insert_or_present_because_the_caller_reloads() {
    let mut env = pair_env(RawPairMode::PreferJpeg, false);
    do_group_move(&mut env, "pair.jpg");
    let presents_before = env.sink.presented.len();

    let undo = result(
        true,
        Move,
        &p("pair.jpg"),
        Some(vec![p("pair.jpg"), "other/pair.cr2".to_string()]),
        None,
    );
    let step = env.undo_with(undo.clone(), Some(FOLDER));

    assert!(undone_ok(&step));
    assert!(FileActionController::restores_outside_folder(Some(&undo), Some(FOLDER)));
    assert_eq!(env.sink.presented.len(), presents_before);
    assert_eq!(env.catalog_paths(), vec![p("zbefore.jpg"), p("aafter.jpg")]);
    // the source IS in this folder: the catalog change is announced and the session points at it
    assert_eq!(env.sink.sessions.last(), Some(&p("pair.jpg")));
}

// C#: FileActionControllerOutcomeGapTests.UndoLast_PartialCaptureUndoWhileAnotherPhotoIsCurrent_SessionPathStaysOnTheCurrentPhoto
#[test]
fn undo_last_partial_capture_undo_while_another_photo_is_current_session_path_stays_on_the_current_photo() {
    let mut env = Env::new(AppSettings::default());
    env.load(&["a.jpg", "p.jpg", "p.cr2"]);
    env.set_actions(AppSettings::default(), vec![action("DeleteIt", Recycle, "", false)]);
    env.exec.script(Scripted::Group(group_ok(
        Recycle,
        vec![
            member(&p("p.jpg"), None, true, false),
            member(&p("p.cr2"), None, true, false),
        ],
        None,
    )));
    env.run_action(0, None, Some(&p("p.jpg")));
    env.run_worker();
    env.sink.sessions.clear();

    let failed = result(
        false,
        Recycle,
        &p("p.jpg"),
        Some(vec![p("p.jpg")]),
        Some("restore failed"),
    );
    let step = env.undo_with(failed, Some(FOLDER));

    assert!(!undone_ok(&step));
    assert_eq!(env.sink.sessions, vec![p("a.jpg")]);
    assert!(env.catalog.index_of(&p("p.jpg")) >= 0);
}

// ---------------------------------------------------------------------------------------------------------------
// FileActionControllerLateCompletionTests (undo)
// ---------------------------------------------------------------------------------------------------------------

/// Folder A with `a.jpg` (moved late) and `b.jpg`; the move finishes after the user opened folder B (`other/x.jpg`).
fn late_move_env() -> (Env, String) {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.exec.script(Scripted::Single(single_ok(
        Move,
        &p("a.jpg"),
        Some("photos/Sorted/a.jpg"),
    )));
    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    env.load_paths(&["other/x.jpg".to_string()]);
    env.run_worker();
    (env, p("a.jpg"))
}

// C#: FileActionControllerLateCompletionTests.LateMove_Undo_WhileOtherFolderOpen_RestoresFile_WithoutChangingCatalog
#[test]
fn late_move_undo_while_other_folder_open_restores_file_without_changing_catalog() {
    let (mut env, moved) = late_move_env();
    env.sink = Default::default();
    let version_before = env.catalog.structural_version();

    let undo = result(true, Move, &moved, None, None);
    let step = env.undo_with(undo.clone(), Some("other"));

    assert!(undone_ok(&step));
    assert_eq!(env.catalog_paths(), vec!["other/x.jpg".to_string()]); // no entry gained or lost
    assert_eq!(env.catalog.structural_version(), version_before);
    assert!(env.sink.removed.is_empty() && env.sink.sessions.is_empty() && env.sink.presented.is_empty());
    assert!(FileActionController::restores_outside_folder(
        Some(&undo),
        Some("other")
    )); // the caller reopens folder A (R7-2)
}

// C#: FileActionControllerLateCompletionTests.Undo_WhenOriginalFolderIsCurrent_InsertsIntoCatalog
#[test]
fn undo_when_original_folder_is_current_inserts_into_catalog() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    do_move(&mut env, "a.jpg");
    assert_eq!(env.catalog_paths(), vec![p("b.jpg")]);

    let undo = result(true, Move, &p("a.jpg"), None, None);
    let step = undo_and_finish(&mut env, undo.clone(), Some(FOLDER));

    assert!(undone_ok(&step));
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("b.jpg")]);
    assert!(env.sink.sessions.contains(&p("a.jpg")));
    assert!(!FileActionController::restores_outside_folder(
        Some(&undo),
        Some(FOLDER)
    ));
}

// C#: FileActionControllerLateCompletionTests.Recycle_FinishingAfterFolderSwitch_IsUndoable_AndUndoDoesNotTouchNewFolderSession (undo half)
#[test]
fn recycle_finishing_after_folder_switch_undo_does_not_touch_new_folder_session() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.run_action(1, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    env.load_paths(&["other/x.jpg".to_string()]);
    env.run_worker();
    env.sink = Default::default();

    let step = env.undo_with(result(true, Recycle, &p("a.jpg"), None, None), Some("other"));

    assert!(undone_ok(&step));
    assert_eq!(env.catalog_paths(), vec!["other/x.jpg".to_string()]);
    // the restored path belongs to folder A: writing it as B's session CurrentPath would corrupt B's resume state
    assert!(env.sink.sessions.is_empty());
}

// C#: FileActionControllerLateCompletionTests.RecycleUndo_InSameFolder_UpdatesSessionPath
#[test]
fn recycle_undo_in_same_folder_updates_session_path() {
    let mut env = move_env(&["a.jpg", "other.jpg"]);
    do_recycle(&mut env, "a.jpg");
    env.sink.sessions.clear();
    let presents_before = env.sink.presented.len();

    env.undo_with(result(true, Recycle, &p("a.jpg"), None, None), Some(FOLDER));

    // the restored photo is back in the list, but the review stays on the photo shown since the delete
    assert_eq!(env.catalog_paths(), vec![p("a.jpg"), p("other.jpg")]);
    assert_eq!(
        env.catalog.current().map(|e| e.path().to_string()),
        Some(p("other.jpg"))
    );
    assert!(env.sink.sessions.contains(&p("other.jpg")));
    assert!(!env.sink.sessions.contains(&p("a.jpg")));
    assert_eq!(env.sink.presented.len(), presents_before);
    assert_eq!(
        env.preload.preload_centers.lock().unwrap().len(),
        1,
        "the restored photo is warmed"
    );
}

// C#: FileActionControllerLateCompletionTests.FolderSwitchDuringUndo_DoesNotTouchNewCatalog
#[test]
fn folder_switch_during_undo_does_not_touch_new_catalog() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    do_move(&mut env, "a.jpg");
    let ticket = env.undo_begin(Some(FOLDER));
    // the undo runs; meanwhile the user opens folder B
    env.clock.next_folder();
    env.load_paths(&["other/x.jpg".to_string()]);
    env.sink.sessions.clear();

    let step = env.undo_deliver(ticket, result(true, Move, &p("a.jpg"), None, None));

    assert!(undone_ok(&step));
    assert_eq!(env.catalog_paths(), vec!["other/x.jpg".to_string()]);
    assert!(env.sink.sessions.is_empty());
}

// C#: FileActionControllerLateCompletionTests.UndoMove_FolderSwitchDuringPresent_DoesNotWriteSessionOrStatus
#[test]
fn undo_move_folder_switch_during_present_does_not_write_session_or_status() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    do_move(&mut env, "a.jpg");
    assert_eq!(
        env.undo_with(result(true, Move, &p("a.jpg"), None, None), Some(FOLDER)),
        FileActionStep::Pending
    );
    // the presenter of the restored photo is still running when the user opens folder B
    env.clock.next_folder();
    env.load_paths(&["other/x.jpg".to_string()]);
    env.sink = Default::default();

    let step = env.present_finished(false);

    assert!(undone_ok(&step));
    assert!(env.sink.sessions.is_empty());
    assert!(env.sink.statuses.is_empty());
}

// C#: FileActionControllerLateCompletionTests.FailedUndo_FolderSwitchDuringUndo_DoesNotWriteStatus
#[test]
fn failed_undo_folder_switch_during_undo_does_not_write_status() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    do_recycle(&mut env, "a.jpg");
    let ticket = env.undo_begin(Some(FOLDER));
    env.clock.next_folder();
    env.load_paths(&["other/x.jpg".to_string()]);
    env.sink = Default::default();

    let step = env.undo_deliver(
        ticket,
        result(
            false,
            Recycle,
            &p("a.jpg"),
            None,
            Some("the Recycle Bin no longer has the file"),
        ),
    );

    assert!(!undone_ok(&step));
    assert!(env.sink.statuses.is_empty());
}

// C#: FileActionControllerLateCompletionTests.TwoLateMoves_UndoInLifoOrder
// (the LIFO choice is made by `UndoService`, tested in pr-core; here: both moves are registered in order and each undo reports its own source)
#[test]
fn two_late_moves_are_registered_in_order_and_each_undo_is_handled_with_its_own_result() {
    let mut env = move_env(&["a.jpg", "b.jpg", "c.jpg"]);
    for name in ["a.jpg", "b.jpg"] {
        env.exec.script(Scripted::Single(single_ok(
            Move,
            &p(name),
            Some(&format!("photos/Sorted/{name}")),
        )));
        env.run_action(0, None, Some(&p(name)));
        env.clock.next_folder();
        env.run_worker();
    }
    let history = env
        .undo
        .as_ref()
        .map(|u| u.lock().unwrap().move_history().to_vec())
        .unwrap_or_default();
    assert_eq!(
        history.iter().map(|h| h.0.clone()).collect::<Vec<_>>(),
        vec![p("a.jpg"), p("b.jpg")]
    );

    env.load_paths(&["other/x.jpg".to_string()]);
    let first = env.undo_with(result(true, Move, &p("b.jpg"), None, None), Some("other"));
    let second = env.undo_with(result(true, Move, &p("a.jpg"), None, None), Some("other"));

    assert!(undone_ok(&first) && undone_ok(&second));
}

// C#: FileActionControllerLateCompletionTests.Copy_FinishingAfterFolderSwitch_HasNoUndo_AndLeavesNewFolderUntouched
#[test]
fn copy_finishing_after_folder_switch_has_no_undo_and_leaves_new_folder_untouched() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    env.set_actions(AppSettings::default(), vec![action("CopyIt", Copy, "Copies", false)]);
    env.exec.script(Scripted::Single(single_ok(
        Copy,
        &p("a.jpg"),
        Some("photos/Copies/a.jpg"),
    )));

    env.run_action(0, None, Some(&p("a.jpg")));
    env.clock.next_folder();
    env.load_paths(&["other/x.jpg".to_string()]);
    env.run_worker();

    assert!(env.sink.late_statuses.is_empty());
    assert_eq!(env.catalog_paths(), vec!["other/x.jpg".to_string()]);
    assert!(
        !env.undo
            .as_ref()
            .is_some_and(|u| u.lock().unwrap().has_last_undo_action())
    );
}

// ---------------------------------------------------------------------------------------------------------------
// New tests
// ---------------------------------------------------------------------------------------------------------------

// New test: no undo service
#[test]
fn undo_without_an_undo_service_ends_at_once_with_no_result() {
    use file_action_fakes::EnvOptions;
    let mut env = Env::with_options(
        AppSettings::default(),
        EnvOptions {
            undo: false,
            ..EnvOptions::default()
        },
    );
    env.load(&["a.jpg"]);

    let started = {
        let mut cx = pr_app::coordinators::file_actions::recycle::FileActionCx {
            catalog: &mut env.catalog,
            sink: &mut env.sink,
        };
        env.controller.undo_last(&mut cx, Some(FOLDER))
    };

    assert_eq!(started, FileActionStep::Done(FileActionDone::Undone(None)));
}

// New test: an undo event of an older operation is dropped
#[test]
fn undo_event_with_an_old_ticket_is_ignored() {
    let mut env = move_env(&["a.jpg"]);
    let ticket = env.undo_begin(Some(FOLDER));

    let stale = env.undo_deliver(ticket + 100, result(true, Move, &p("a.jpg"), None, None));
    assert_eq!(stale, FileActionStep::Pending);
    assert!(env.controller.is_in_flight());

    let real = env.undo_deliver(ticket, result(false, Move, &p("a.jpg"), None, Some("nothing to undo")));
    assert!(!undone_ok(&real));
    assert!(!env.controller.is_in_flight());
}

// New test: a failed undo with no message says "nothing to undo"
#[test]
fn failed_undo_without_a_message_says_nothing_to_undo() {
    let mut env = move_env(&["a.jpg"]);

    env.undo_with(result(false, Move, "", None, None), Some(FOLDER));

    assert_eq!(env.sink.statuses, vec![text("status.nothingToUndo")]);
}

// New test: a note on a succeeded (partly restored) capture undo is shown
#[test]
fn succeeded_undo_with_an_error_note_shows_the_note_and_refreshes_navigation() {
    let mut env = move_env(&["a.jpg", "b.jpg"]);
    do_move(&mut env, "a.jpg");
    let navigation_before = env.sink.navigation_changes;

    env.undo_with(
        result(true, Move, &p("a.jpg"), None, Some("one companion could not come back")),
        Some(FOLDER),
    );
    env.present_finished(false);

    assert_eq!(env.sink.last_status(), Some("one companion could not come back"));
    assert_eq!(env.sink.navigation_changes, navigation_before + 1);
}
