//! W8-08c: `MainViewModel` queued file actions, Recycle, Undo, zoom / fit / fullscreen, the info overlay, Refresh, the external editor,
//! the clipboard copies, the folder picker, the duplicate commands and the dialog shortcuts (`contracts/W8-08c.md`, C# `MainViewModel.cs`
//! lines 565 to 854). Each test names the C# test it ports (`// C#: Class.Method`) or is marked `// New test`. The window is the whole
//! `MainViewModel` over real coordinators, the real `FileActionService` / `UndoService` on files of a temp folder, a recycle bin that moves
//! files into the test root, and a worker queue the test runs by hand (`support/main_env.rs`): "the move is in flight" is a queued job.
//!
//! Not ported, with the reason (no silent skips):
//! - `RunActionAsync_EntryRemovedDuringConfirm_DoesNotExecute`: the C# dialog hook removes the entry from the shared catalog while the
//!   Confirm dialog is open. The view model owns the catalog and is mutably borrowed during the call, so nothing can change it there
//!   (contract O3); the controller twin lives in `file_action_recycle.rs`.
//! - `RemoveDuplicatesAsync_EntryRemovedDuringReview_SkipsIt`: same reason (the hook removes a catalog entry); controller twin in
//!   `duplicate_cleanup.rs`.
//! - `MainViewModelFileActionTests.Recycle_*` / `RunAction_*` tests of the no-Recycle-Bin rules (Q-R8) are ported one per behaviour that
//!   the view model wires (refusal with the dialog, decline, confirmed permanent delete, fixed drive); the rest is controller logic (07c).

mod file_action_fakes;
mod support {
    pub mod main_env;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_app::coordinators::file_action_gate::QueuedOutcome;
use pr_app::services::contracts::ClipboardService;
use pr_app::viewmodels::format::status;
use pr_app::viewmodels::main::{ExternalEditorFailure, ExternalEditorLauncher, MainProperty};
use pr_app::viewmodels::viewer_state::{ACTUAL_SIZE_ZOOM, ViewerProperty};
use pr_app::viewport::fit::ZoomDetailRefreshResult;
use pr_core::model::ScalingQuality;
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::shortcuts::ReviewAction;
use pr_l10n::template::LocArg;
use support::main_env::{EnvOptions, MainEnv, en_with, update_settings};
use support::presenter_fakes::FormatFixture;
use support::preview_fakes::{SizedDecoder, lock};

// ---- helpers ----------------------------------------------------------------------------------------------------------------

/// Creates the folder, opens it and settles.
fn album(env: &mut MainEnv, name: &str, files: &[&str]) -> (String, Vec<String>) {
    let (folder, paths) = env.images(name, files);
    env.open(&folder);
    (folder, paths)
}

/// Settles everything except the file operation (the queued `FileAction` job), i.e. "the move is in flight".
fn in_flight(env: &mut MainEnv) {
    env.settle_holding("everything but the file operation", &["FileAction"], |_| false);
}

fn sorted(folder: &str, name: &str) -> PathBuf {
    Path::new(folder).join("Sorted").join(name)
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

fn undo_len(env: &MainEnv) -> usize {
    lock(&env.undo).move_history().len()
}

fn error_dialogs(env: &MainEnv) -> Vec<(String, String)> {
    lock(&env.dialog.errors).clone()
}

fn confirmations(env: &MainEnv) -> Vec<(String, String)> {
    lock(&env.dialog.confirmations).clone()
}

fn failing_move() -> pr_core::file_actions::undo::MoveOverride {
    Arc::new(|_: &Path, _: &Path| Err(std::io::Error::other("Simulated disk error.")))
}

// ---- gate and queue ---------------------------------------------------------------------------------------------------------

#[test]
fn queued_move_runs_after_the_previous_one_finished_its_catalog_update() {
    // C#: MainViewModelFileActionTests.RunActionAsync_ConcurrentAction_IsQueued_PreservingInv4, plus O3 of W8-06e (the turn outlives the action)
    let mut env = MainEnv::new();
    let (folder, files) = album(&mut env, "inv4_album", &["1.jpg", "2.jpg", "3.jpg"]);

    env.vm.run_action(0);
    assert!(env.vm.is_file_action_in_progress());
    let second = env.vm.run_action(0);
    assert!(second.is_empty());
    in_flight(&mut env);
    assert_eq!(
        env.runner.count_of("FileAction"),
        1,
        "only the first operation is in flight; the second waits for its turn"
    );

    // The first operation finishes on its worker; its result is in the inbox but its catalog update has not been applied yet.
    assert!(env.runner.run_tagged("FileAction"));
    assert!(env.vm.is_file_action_in_progress());
    assert_eq!(
        env.runner.count_of("FileAction"),
        0,
        "INV-4: the second action does not start before the first one is applied"
    );

    // Applying the result finishes the first turn; the gate posts the second command (never inline on the finishing stack).
    env.pump();
    assert_eq!(env.scheduler.pending(), 1, "the successor is posted, not run inline");
    env.settle();

    assert!(!env.vm.is_file_action_in_progress());
    assert!(!env.service.is_busy());
    assert!(sorted(&folder, "1.jpg").exists());
    assert!(
        sorted(&folder, "2.jpg").exists(),
        "the queued command ran on the then-current photo"
    );
    assert!(exists(&files[2]));
    assert_eq!(undo_len(&env), 2);
}

#[test]
fn queued_move_whose_folder_changed_before_its_turn_does_not_move_the_new_folder_photo() {
    // C#: FolderScope.QueuedMove_FolderChangesBeforeItsTurn_DoesNotMoveNewFolderPhoto (R02)
    let mut env = MainEnv::new();
    let (folder_a, files_a) = album(&mut env, "queue_move_a", &["1.jpg", "2.jpg"]);
    let (folder_b, files_b) = env.images("queue_move_b", &["x.jpg"]);

    env.vm.run_action(0);
    assert!(env.vm.is_file_action_in_progress());
    env.vm.run_action(0); // queued behind the first
    in_flight(&mut env);
    env.vm
        .open_folder(&folder_b, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle();

    assert!(
        exists(&files_b[0]),
        "An action queued in folder A moved folder B's photo."
    );
    assert!(!sorted(&folder_b, "x.jpg").exists());
    assert_eq!(env.vm.catalog().paths().to_vec(), files_b);
    // Only the command that had started in A ran; the stale queued one was dropped, not re-targeted.
    assert!(!exists(&files_a[0]));
    assert!(exists(&files_a[1]));
    assert_eq!(undo_len(&env), 1);
    assert!(!env.vm.is_file_action_in_progress());
    let _ = folder_a;
}

#[test]
fn queued_recycle_whose_folder_changed_before_its_turn_does_not_recycle_the_new_folder_photo() {
    // C#: FolderScope.QueuedRecycle_FolderChangesBeforeItsTurn_DoesNotRecycleNewFolderPhoto (R02)
    let mut env = MainEnv::new();
    let (_, files_a) = album(&mut env, "queue_recycle_a", &["1.jpg", "2.jpg"]);
    let (folder_b, files_b) = env.images("queue_recycle_b", &["x.jpg"]);

    env.vm.recycle();
    assert!(env.vm.is_file_action_in_progress());
    env.vm.recycle();
    in_flight(&mut env);
    env.vm
        .open_folder(&folder_b, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle();

    assert!(
        exists(&files_b[0]),
        "A Recycle queued in folder A recycled folder B's photo."
    );
    assert_eq!(env.bin.recycled(), vec![files_a[0].clone()]);
    assert!(exists(&files_a[1]));
    assert_eq!(env.vm.catalog().paths().to_vec(), files_b);
    assert!(!env.vm.is_file_action_in_progress());
    let _ = folder_b;
}

#[test]
fn queued_moves_in_the_same_folder_all_run_on_the_live_selection() {
    // C#: FolderScope.QueuedMoves_SameFolder_AllRunOnLiveSelection (control)
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "queue_same", &["1.jpg", "2.jpg", "3.jpg"]);

    env.vm.run_action(0);
    env.vm.run_action(0);
    env.settle();

    assert!(!exists(&files[0]));
    assert!(!exists(&files[1]));
    assert!(exists(&files[2]));
    assert_eq!(env.vm.catalog().paths().to_vec(), vec![files[2].clone()]);
    assert_eq!(undo_len(&env), 2);
}

#[test]
fn queued_command_is_rejected_silently_while_undo_holds_the_gate() {
    // C#: OC14_FileActionDuringUndo_IsNoOp_AndConcurrentUndoDoesNotDoubleRestore
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "oc14_action_during_undo", &["1.jpg", "2.jpg", "3.jpg"]);
    env.vm.run_action(0);
    env.settle();
    env.vm.run_action(0);
    env.settle();
    assert_eq!(undo_len(&env), 2);
    let total = env.vm.total_files();

    env.vm.undo(); // the undo is in flight on its worker
    in_flight(&mut env);
    assert!(env.vm.is_file_action_in_progress());
    let rejected = env.vm.run_action(0); // no-op: gate held by undo
    let second_undo = env.vm.undo(); // second undo: no-op
    assert!(rejected.is_empty() && second_undo.is_empty());
    assert_eq!(env.vm.total_files(), total);
    assert_eq!(env.runner.count_of("FileAction"), 1, "only the first undo is in flight");

    env.settle();

    assert!(!env.vm.is_file_action_in_progress());
    assert_eq!(env.vm.total_files(), total + 1);
    assert_eq!(undo_len(&env), 1);
    assert!(
        env.vm.catalog().paths().iter().any(|path| path == &files[1]),
        "the most recent move was undone exactly once"
    );
}

#[test]
fn undo_during_a_file_action_is_a_no_op_and_the_gate_is_released_after() {
    // C#: OC14_UndoDuringFileAction_IsNoOp_AndGateReleasedAfter
    let mut env = MainEnv::new();
    album(&mut env, "oc14_undo_during_action", &["1.jpg", "2.jpg", "3.jpg"]);
    env.vm.run_action(0); // move 1.jpg, registers undo
    env.settle();
    assert_eq!(undo_len(&env), 1);

    env.vm.run_action(0); // move 2.jpg, in flight
    in_flight(&mut env);
    assert!(env.vm.is_file_action_in_progress());
    let ignored = env.vm.undo(); // must be a no-op while the action holds the gate
    assert!(ignored.is_empty());
    assert_eq!(undo_len(&env), 1);

    env.settle();
    assert!(!env.vm.is_file_action_in_progress());
    assert_eq!(undo_len(&env), 2);
}

#[test]
fn gate_turn_is_kept_while_the_controller_is_pending_and_finished_on_done() {
    // New test (T1: the turn must outlive the action)
    let mut env = MainEnv::new();
    album(&mut env, "turn_lifetime", &["1.jpg", "2.jpg"]);

    env.vm.run_action(0);
    in_flight(&mut env);
    assert!(
        env.gate.is_held(),
        "the controller is pending: the view model keeps the turn"
    );
    assert!(env.vm.is_file_action_in_progress());

    assert!(env.runner.run_tagged("FileAction"));
    assert!(
        env.gate.is_held(),
        "the result is in the inbox but not applied: still held"
    );
    env.settle();

    assert!(!env.gate.is_held(), "released once the controller reported Done");
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn a_command_parked_behind_the_explorer_order_runs_after_the_release_and_re_checks_the_epoch() {
    // New test (T4: the epoch is compared at the turn and again after the Explorer wait)
    let mut env = MainEnv::new();
    let (folder, files) = env.images("order_pending", &["1.png", "2.png", "3.png"]);
    env.explorer.set_order(files.clone());
    env.vm
        .open_folder(&folder, Some(&files[1]))
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("the opened file to be presented", &["Explorer"], |_| false);
    assert_eq!(env.vm.current_index(), 1);

    // (1) Parked: nothing ran, the turn is held.
    let parked = env.vm.run_action(0);
    assert!(parked.is_empty());
    assert!(env.vm.is_file_action_in_progress());
    assert_eq!(env.runner.count_of("FileAction"), 0);
    assert!(exists(&files[1]));

    // (2) Released: the command runs on the live selection.
    env.settle();
    assert!(sorted(&folder, "2.png").exists());
    assert!(!env.vm.is_file_action_in_progress());

    // (3) A command parked in one load whose epoch is outdated when the gate is released does not run.
    let (other, other_files) = env.images("order_other", &["x.png"]);
    let (folder2, files2) = env.images("order_pending2", &["a.png", "b.png"]);
    env.explorer.set_order(files2.clone());
    env.vm
        .open_folder(&folder2, Some(&files2[1]))
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("the opened file to be presented", &["Explorer"], |_| false);
    env.vm.run_action(0); // parked behind the order of folder2
    env.vm
        .open_folder(&other, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle();

    assert!(exists(&files2[1]), "the stale parked command did not move a photo");
    assert!(exists(&other_files[0]));
    assert!(!env.vm.is_file_action_in_progress());
    assert_eq!(env.vm.catalog().paths().to_vec(), other_files);
}

#[test]
fn when_file_action_idle_runs_inline_when_free_and_posts_when_held() {
    // New test (R7-7: the window close waits for the gate)
    let mut env = MainEnv::new();
    album(&mut env, "when_idle", &["1.jpg", "2.jpg"]);
    let ran = Arc::new(AtomicUsize::new(0));

    let counter = Arc::clone(&ran);
    assert!(env.vm.when_file_action_idle(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })));
    assert_eq!(ran.load(Ordering::SeqCst), 1, "free gate: the waiter ran inline");

    env.vm.run_action(0);
    let counter = Arc::clone(&ran);
    assert!(!env.vm.when_file_action_idle(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })));
    assert_eq!(ran.load(Ordering::SeqCst), 1, "held gate: the waiter waits");
    env.settle();
    assert_eq!(
        ran.load(Ordering::SeqCst),
        2,
        "the waiter was posted when the gate became free"
    );
}

// ---- file actions through the view model ------------------------------------------------------------------------------------

#[test]
fn run_action_move_presents_next_before_the_move_completes() {
    // C#: MainViewModelFileActionTests.RunActionAsync_Move_PresentsNextBeforeMoveCompletes_PreservingInv3
    let mut env = MainEnv::new();
    let (folder, files) = album(&mut env, "inv3_album", &["1.jpg", "2.jpg"]);
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.current_index(), 0);

    env.vm.run_action(0); // Move to "Sorted"
    in_flight(&mut env);

    assert_eq!(env.vm.total_files(), 1);
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[1].clone())
    );
    assert_eq!(
        env.vm.presenter().current_presented_path(),
        Some(files[1].as_str()),
        "INV-3: the next photo is shown first"
    );
    assert!(exists(&files[0]), "the move has not run yet");

    env.settle();

    assert!(sorted(&folder, "1.jpg").exists());
    assert!(!exists(&files[0]));
}

#[test]
fn run_action_move_failure_restores_the_image_to_the_catalog() {
    // C#: MainViewModelFileActionTests.RunActionAsync_MoveFailure_RestoresImageToCatalog_PreservingInv5
    let mut env = MainEnv::with_options(EnvOptions {
        move_override: Some(failing_move()),
        ..EnvOptions::default()
    });
    let (_, files) = album(&mut env, "inv5_album", &["1.jpg", "2.jpg"]);
    assert_eq!(env.vm.total_files(), 2);

    env.vm.run_action(0);
    env.settle();

    // The source is back in the catalog at its place.
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.catalog().paths()[0], files[0]);
    assert!(
        env.vm.status_text().contains("MoveToSub"),
        "the failure names the action: {}",
        env.vm.status_text()
    );
}

#[test]
fn run_action_with_compare_selection_acts_on_the_selected_path() {
    // C#: MainViewModelFileActionTests.RunActionAsync_WithCompareSelectedPath_ActsOnSelectedPath
    let mut env = MainEnv::new();
    let (folder, files) = album(&mut env, "compare_album", &["1.jpg", "2.jpg"]);
    env.vm.compare_mut_for_tests().select(Some(files[1].clone()));

    env.vm.run_action(0); // Move to "Sorted"
    env.settle();

    assert!(sorted(&folder, "2.jpg").exists());
    assert!(!exists(&files[1]));
    assert!(exists(&files[0]));
    assert_eq!(env.vm.total_files(), 1);
}

#[test]
fn run_action_when_confirm_is_declined_does_not_execute() {
    // C#: MainViewModelFileActionTests.RunActionAsync_WhenConfirmDeclined_DoesNotExecute
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "confirm_album", &["1.jpg"]);
    env.dialog.confirm_answer.store(false, Ordering::SeqCst);

    env.vm.run_action(2); // ConfirmRecycle
    env.settle();

    assert!(exists(&files[0]));
    assert_eq!(env.vm.total_files(), 1);
    assert!(env.bin.recycled().is_empty());
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn run_action_folder_switch_during_confirm_does_not_execute() {
    // C#: MainViewModelFileActionTests.RunActionAsync_FolderSwitchDuringConfirm_DoesNotExecute (R7-4)
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "confirm_switch", &["1.jpg"]);
    let clock = Arc::clone(&env.clock);
    *lock(&env.dialog.on_confirm) = Some(Box::new(move || {
        clock.next_folder(); // a forwarded open ran in the nested loop
    }));

    env.vm.run_action(2); // ConfirmRecycle
    env.settle();

    assert_eq!(confirmations(&env).len(), 1);
    assert!(env.bin.recycled().is_empty());
    assert!(exists(&files[0]));
}

#[test]
fn run_action_after_settings_saved_uses_the_saved_profiles() {
    // C#: MainViewModelFileActionTests.RunActionAsync_AfterSettingsSaved_UsesSavedProfiles (R2-F-03)
    let mut env = MainEnv::new();
    let (folder, files) = album(&mut env, "saved_settings_album", &["1.jpg", "2.jpg"]);

    // Simulates Settings > Save: the window saves a NEW AppSettings instance.
    update_settings(&env.store, |settings| {
        settings.actions = vec![ReviewAction {
            name: "MoveElsewhere".to_owned(),
            destination: "NewDest".to_owned(),
            confirm: true,
            ..ReviewAction::new("MoveElsewhere")
        }];
    });
    env.dialog.confirm_answer.store(true, Ordering::SeqCst);

    env.vm.run_action(0);
    env.settle();

    assert!(Path::new(&folder).join("NewDest").join("1.jpg").exists());
    assert!(!sorted(&folder, "1.jpg").exists());
    assert!(!exists(&files[0]));
}

#[test]
fn run_action_after_settings_saved_with_confirm_asks_for_confirmation() {
    // C#: MainViewModelFileActionTests.RunActionAsync_AfterSettingsSavedWithConfirm_AsksForConfirmation
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "saved_confirm_album", &["1.jpg"]);
    update_settings(&env.store, |settings| {
        settings.actions = vec![ReviewAction {
            name: "MoveElsewhere".to_owned(),
            destination: "NewDest".to_owned(),
            confirm: true,
            ..ReviewAction::new("MoveElsewhere")
        }];
    });
    env.dialog.confirm_answer.store(false, Ordering::SeqCst);

    env.vm.run_action(0);
    env.settle();

    assert!(exists(&files[0]));
    assert_eq!(env.vm.total_files(), 1);
    assert_eq!(confirmations(&env).len(), 1);
}

#[test]
fn run_action_when_the_folder_switched_during_io_registers_undo_and_leaves_the_new_folder_untouched() {
    // C#: MainViewModelFileActionTests.RunActionAsync_WhenFolderSwitchedDuringIo_RegistersUndo_AndLeavesNewFolderUntouched (APP-03)
    let mut env = MainEnv::new();
    let (_, _) = album(&mut env, "album_stale1", &["1.jpg"]);
    let (folder2, files2) = env.images("album_stale2", &["x.jpg"]);

    env.vm.run_action(0);
    in_flight(&mut env);
    env.vm
        .open_folder(&folder2, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("folder 2 to load", &["FileAction"], |_| false);
    env.settle();

    // The move is real, so it is undoable; folder 2 is not touched.
    assert_eq!(undo_len(&env), 1);
    assert_eq!(env.vm.catalog().paths().to_vec(), files2);
    assert_eq!(env.vm.session().map(|session| session.folder.clone()), Some(folder2));
    assert_eq!(
        env.vm.session().and_then(|session| session.current_path.clone()),
        Some(files2[0].clone())
    );
}

#[test]
fn recycle_executes_and_advances() {
    // C#: MainViewModelFileActionTests.RecycleAsync_ExecutesRecycleAndAdvances
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "recycle_album", &["1.jpg", "2.jpg"]);

    env.vm.recycle();
    env.settle();

    assert!(env.bin.recycled().contains(&files[0]));
    assert_eq!(env.vm.total_files(), 1);
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[1].clone())
    );
}

#[test]
fn run_action_move_keeps_the_next_image_cached_and_evicts_only_the_removed_one() {
    // C#: MainViewModelFileActionTests.RunActionAsync_Move_KeepsNextImageCachedAndEvictsOnlyRemovedOne (R2-F-02, T5 per-path eviction)
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "evict_album", &["1.jpg", "2.jpg"]);
    // The next image is already warm (as after a preload) before the action key is pressed.
    assert!(env.preview_service.get_preview_path(&files[1]).wait().is_ok());
    assert!(env.preview_service.try_get_cached_preview_path(&files[1]).is_some());
    assert!(env.preview_service.try_get_cached_preview_path(&files[0]).is_some());
    let reads_before = env.metrics.snapshot().source_reads;

    env.vm.run_action(0); // Move 1.jpg to Sorted; 2.jpg becomes current
    env.settle();

    assert!(env.preview_service.try_get_cached_preview_path(&files[0]).is_none());
    assert!(env.preview_service.try_get_cached_preview_path(&files[1]).is_some());
    // Presenting the next image was served from RAM: no new source read.
    assert_eq!(env.metrics.snapshot().source_reads, reads_before);
}

#[test]
fn move_completing_after_a_folder_switch_is_undoable_and_undo_restores_it() {
    // C#: FolderChange.MoveCompletingAfterFolderSwitch_IsUndoable_AndUndoRestoresIt (APP-03)
    let mut env = MainEnv::new();
    let (folder1, files1) = album(&mut env, "app03_a", &["1.jpg"]);
    let (folder2, files2) = env.images("app03_b", &["x.jpg"]);

    env.vm.run_action(0); // Move to "Sorted"
    in_flight(&mut env);
    env.vm
        .open_folder(&folder2, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("folder 2 to load", &["FileAction"], |_| false);
    env.settle();

    let moved = sorted(&folder1, "1.jpg");
    assert!(!exists(&files1[0]));
    assert!(moved.exists());
    assert_eq!(undo_len(&env), 1);
    // Folder 2 idle: the hint is visible. Its catalog, session and gate are untouched.
    assert_eq!(
        env.vm.status_text(),
        en_with(
            "status.lateMoveUndoable",
            &[
                LocArg::new("fileName", "1.jpg"),
                LocArg::new("folder", Path::new(&folder1).join("Sorted").to_string_lossy().as_ref())
            ]
        )
    );
    assert_eq!(env.vm.catalog().paths().to_vec(), files2);
    assert!(!env.vm.is_file_action_in_progress());

    env.vm.undo();
    env.settle();

    assert!(exists(&files1[0]));
    assert!(!moved.exists());
    assert_eq!(
        env.vm.session().map(|session| session.folder.clone()),
        Some(folder1),
        "R7-2: folder 1 is reopened"
    );
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files1[0].clone())
    );
}

#[test]
fn late_completion_hint_does_not_overwrite_the_new_folder_status() {
    // C#: FolderChange.LateCompletionHint_DoesNotOverwriteNewFolderStatus (APP-03)
    let mut env = MainEnv::new();
    album(&mut env, "app03_h1", &["1.jpg"]);
    let empty = env.folder("app03_h2");

    env.vm.run_action(0);
    in_flight(&mut env);
    env.vm
        .open_folder(&empty, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("the empty folder to load", &["FileAction"], |_| false);
    let before = env.vm.status_text();
    assert_eq!(before, env.text("status.noSupportedImages"));
    env.settle();

    assert_eq!(undo_len(&env), 1, "still undoable, only the hint is dropped");
    assert_eq!(env.vm.status_text(), before);
}

#[test]
fn recycle_completing_after_a_folder_switch_is_undoable_and_does_not_corrupt_the_new_session() {
    // C#: FolderChange.RecycleCompletingAfterFolderSwitch_IsUndoable_AndDoesNotCorruptNewSession (APP-03)
    let mut env = MainEnv::new();
    let (folder1, files1) = album(&mut env, "app03_r1", &["1.jpg"]);
    let (folder2, files2) = env.images("app03_r2", &["x.jpg"]);

    env.vm.recycle();
    in_flight(&mut env);
    env.vm
        .open_folder(&folder2, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("folder 2 to load", &["FileAction"], |_| false);
    env.settle();

    assert_eq!(env.bin.recycled(), vec![files1[0].clone()]);
    assert_eq!(env.vm.catalog().paths().to_vec(), files2);
    assert_eq!(
        env.vm.status_text(),
        en_with("status.lateRecycleUndoable", &[LocArg::new("fileName", "1.jpg")])
    );

    env.vm.undo();
    env.settle();

    assert!(exists(&files1[0]));
    assert_eq!(env.bin.restore_calls(), 1);
    assert_eq!(env.vm.session().map(|session| session.folder.clone()), Some(folder1));
    // Folder 2's persisted session still points at its own file, not at the restored file of folder 1.
    env.session_writer.flush();
    let saved = env
        .sessions
        .load(&folder2)
        .unwrap_or_else(|error| panic!("load session: {error}"));
    assert_eq!(saved.current_path, Some(files2[0].clone()));
}

#[test]
fn copy_completing_after_a_folder_switch_has_no_undo_and_leaves_the_new_folder_untouched() {
    // C#: FolderChange.CopyCompletingAfterFolderSwitch_HasNoUndo_AndLeavesNewFolderUntouched (APP-03)
    let mut env = MainEnv::new();
    let (folder1, files1) = album(&mut env, "app03_c1", &["1.jpg"]);
    let (folder2, files2) = env.images("app03_c2", &["x.jpg"]);

    env.vm.run_action(1); // Copy to "Backup"
    in_flight(&mut env);
    env.vm
        .open_folder(&folder2, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("folder 2 to load", &["FileAction"], |_| false);
    env.settle();

    assert!(exists(&files1[0]));
    assert!(Path::new(&folder1).join("Backup").join("1.jpg").exists());
    assert_eq!(undo_len(&env), 0);
    assert_eq!(env.vm.catalog().paths().to_vec(), files2);
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn actions_without_images_or_with_a_bad_index_are_no_ops() {
    // C#: FolderChange.ActionsWithoutImagesOrWithBadIndex_AreNoOps
    let mut env = MainEnv::new();
    env.vm.run_action(0);
    env.vm.recycle();
    env.settle();

    let (_, files) = album(&mut env, "app03_e", &["only.jpg"]);
    for index in [-1, 999, i32::MIN, i32::MAX] {
        env.vm.run_action(index);
        env.settle();
    }

    assert!(exists(&files[0]));
    assert_eq!(env.vm.total_files(), 1);
    assert_eq!(undo_len(&env), 0);
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn empty_folder_file_and_zoom_commands_are_no_ops() {
    // C#: MainViewModelNavigationTests.Model.EmptyFolder_EveryCommandIsANoOp (the file and zoom commands of the same C# test; the
    // navigation commands are in viewmodels_main_navigation.rs)
    let mut env = MainEnv::new();
    let folder = env.folder("empty_model");
    std::fs::write(Path::new(&folder).join("notes.txt"), "not an image")
        .unwrap_or_else(|error| panic!("write: {error}"));
    env.open(&folder);

    env.vm.recycle();
    env.vm.run_action(0);
    env.vm.undo();
    env.vm.zoom_in();
    env.vm.zoom_out();
    env.vm.zoom_actual_size();
    env.vm.toggle_compare();
    env.settle();

    assert!(!env.vm.has_images());
    assert!(!env.vm.is_file_action_in_progress());
    assert_eq!(undo_len(&env), 0);
}

// ---- Q-R8: permanent delete on drives without a Recycle Bin -----------------------------------------------------------------

fn permanent_delete_album(name: &str, no_bin: bool, allow: bool) -> (MainEnv, Vec<String>) {
    let mut env = MainEnv::new();
    env.bin.no_bin.store(no_bin, Ordering::SeqCst);
    env.update_settings(|settings| settings.allow_permanent_delete_without_recycle_bin = allow);
    let (_, files) = album(&mut env, name, &["1.jpg", "2.jpg"]);
    (env, files)
}

#[test]
fn recycle_on_a_drive_without_a_bin_with_the_setting_off_refuses_and_keeps_the_file() {
    // C#: MainViewModelFileActionTests.Recycle_NoRecycleBinDriveAndSettingOff_RefusesAndKeepsFile
    let (mut env, files) = permanent_delete_album("qr8_off", true, false);
    let presents_before = lock(&env.sink_seen).presented.len();

    env.vm.recycle();
    env.settle();

    assert!(exists(&files[0]));
    assert!(env.bin.permanently_deleted().is_empty());
    assert!(env.bin.recycled().is_empty());
    assert!(confirmations(&env).is_empty());
    assert_eq!(env.vm.total_files(), 2);
    // Refused before the photo left the list: the view does NOT move on to the next photo.
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[0].clone())
    );
    assert_eq!(lock(&env.sink_seen).presented.len(), presents_before);
    let errors = error_dialogs(&env);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, env.text("dialog.recycleNoBin.title"));
    assert!(
        errors[0].1.contains(&env.text("settings.allowPermanentDelete.label")),
        "{}",
        errors[0].1
    );
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn recycle_with_confirm_before_delete_on_a_drive_without_a_bin_refuses_without_asking_first() {
    // C#: MainViewModelFileActionTests.Recycle_NoRecycleBinDriveAndSettingOff_WithConfirmBeforeDelete_RefusesWithoutAskingFirst
    let (mut env, files) = permanent_delete_album("qr8_off_confirm", true, false);
    env.update_settings(|settings| settings.confirm_before_delete = true);

    env.vm.recycle();
    env.settle();

    assert!(
        confirmations(&env).is_empty(),
        "a \"delete?\" question would be pointless: the answer cannot delete it"
    );
    assert_eq!(error_dialogs(&env).len(), 1);
    assert!(exists(&files[0]));
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[0].clone())
    );
}

#[test]
fn recycle_profile_with_confirm_on_a_drive_without_a_bin_refuses_without_asking_first() {
    // C#: MainViewModelFileActionTests.RunAction_RecycleProfileWithConfirm_NoRecycleBinDriveAndSettingOff_RefusesWithoutAskingFirst
    let (mut env, files) = permanent_delete_album("qr8_off_profile", true, false);

    env.vm.run_action(2); // "ConfirmRecycle": Recycle with Confirm = true
    env.settle();

    assert!(confirmations(&env).is_empty());
    let errors = error_dialogs(&env);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, env.text("dialog.recycleNoBin.title"));
    assert!(exists(&files[0]));
}

#[test]
fn recycle_failing_during_the_file_operation_keeps_the_advance_restores_the_photo_and_shows_a_dialog() {
    // C#: MainViewModelFileActionTests.Recycle_FailingDuringTheFileOperation_KeepsTheAdvance_RestoresThePhotoAndShowsADialog
    let (mut env, files) = permanent_delete_album("recycle_io_fail", false, false);
    env.bin.fail_send.store(true, Ordering::SeqCst);

    env.vm.recycle();
    env.settle();

    assert!(exists(&files[0]));
    // INV-3: the next photo was presented before the I/O and is not taken back; INV-5: the photo is back in the list.
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files[1].clone())
    );
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.catalog().paths()[0], files[0]);
    let errors = error_dialogs(&env);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, env.text("dialog.recycleFailed.title"));
    assert!(errors[0].1.contains("1.jpg"));
    assert!(errors[0].1.contains("locked by another process"));
}

#[test]
fn recycle_on_a_drive_without_a_bin_with_the_setting_on_but_the_user_declines_keeps_the_file() {
    // C#: MainViewModelFileActionTests.Recycle_NoRecycleBinDriveSettingOnButUserDeclines_KeepsFile
    let (mut env, files) = permanent_delete_album("qr8_decline", true, true);
    env.dialog.confirm_answer.store(false, Ordering::SeqCst);

    env.vm.recycle();
    env.settle();

    let asked = confirmations(&env);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, env.text("dialog.confirmPermanentDelete.title"));
    assert!(exists(&files[0]));
    assert!(env.bin.permanently_deleted().is_empty());
    assert_eq!(env.vm.total_files(), 2);
}

#[test]
fn recycle_on_a_drive_without_a_bin_with_the_setting_on_and_confirmed_deletes_permanently_and_undo_says_cannot_restore()
{
    // C#: MainViewModelFileActionTests.Recycle_NoRecycleBinDriveSettingOnAndConfirmed_DeletesPermanentlyAndUndoSaysCannotRestore
    let (mut env, files) = permanent_delete_album("qr8_on", true, true);

    env.vm.recycle();
    env.settle();

    let asked = confirmations(&env);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].1,
        en_with(
            "dialog.confirmPermanentDelete.message",
            &[LocArg::new("fileName", "1.jpg")]
        )
    );
    assert!(!exists(&files[0]));
    assert_eq!(env.bin.permanently_deleted(), vec![files[0].clone()]);
    assert!(env.bin.recycled().is_empty());
    assert_eq!(env.vm.total_files(), 1);

    env.vm.undo();
    env.settle();

    assert_eq!(
        env.vm.status_text(),
        en_with("core.undo.permanentlyDeleted", &[LocArg::new("fileName", "1.jpg")])
    );
    assert_eq!(env.bin.restore_calls(), 0);
    assert!(!exists(&files[0]));
}

#[test]
fn recycle_profile_with_confirm_on_a_drive_without_a_bin_asks_only_once_and_explicitly() {
    // C#: MainViewModelFileActionTests.RunAction_RecycleProfileWithConfirmOnNoRecycleBinDrive_AsksOnlyOnceAndExplicitly
    let (mut env, files) = permanent_delete_album("qr8_profile", true, true);

    env.vm.run_action(2); // "ConfirmRecycle": Recycle with Confirm = true
    env.settle();

    let asked = confirmations(&env);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, env.text("dialog.confirmPermanentDelete.title"));
    assert_eq!(env.bin.permanently_deleted(), vec![files[0].clone()]);
}

#[test]
fn recycle_on_a_fixed_drive_with_the_setting_on_behaves_as_before() {
    // C#: MainViewModelFileActionTests.Recycle_FixedDriveWithSettingOn_BehavesAsBefore
    let (mut env, files) = permanent_delete_album("qr8_fixed", false, true);

    env.vm.recycle();
    env.settle();

    assert!(confirmations(&env).is_empty());
    assert!(env.bin.permanently_deleted().is_empty());
    assert_eq!(env.bin.recycled(), vec![files[0].clone()]);

    env.vm.undo();
    env.settle();

    assert_eq!(env.bin.restore_calls(), 1);
    assert!(exists(&files[0]));
}

// ---- undo -------------------------------------------------------------------------------------------------------------------

#[test]
fn undo_restores_a_moved_file_inserts_sorted_and_presents() {
    // C#: MainViewModelFileActionTests.UndoAsync_RestoresMovedFile_InsertsSortedAndPresents
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "undo_album", &["1.jpg", "2.jpg"]);
    env.vm.run_action(0); // Move 1.jpg to Sorted/1.jpg
    env.settle();
    assert_eq!(env.vm.total_files(), 1);

    env.vm.undo(); // Ctrl+Z
    env.settle();

    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(env.vm.catalog().paths()[0], files[0]);
    assert!(exists(&files[0]));
}

#[test]
fn undo_of_a_move_from_a_previous_folder_opens_that_folder() {
    // C#: MainViewModelFileActionTests.UndoAsync_MoveFromPreviousFolder_OpensThatFolderInsteadOfInsertingHere (R7-2)
    let mut env = MainEnv::new();
    let (folder_a, files_a) = album(&mut env, "undo_prev_a", &["1.jpg", "2.jpg"]);
    let (folder_b, files_b) = env.images("undo_prev_b", &["x.jpg"]);
    env.vm.run_action(0); // Move A\1.jpg to A\Sorted
    env.settle();
    env.open(&folder_b);
    assert_eq!(env.vm.catalog().paths().to_vec(), files_b);

    env.vm.undo();
    env.settle();

    assert!(exists(&files_a[0]));
    assert_eq!(env.vm.session().map(|session| session.folder.clone()), Some(folder_a));
    assert_eq!(env.vm.total_files(), 2);
    assert!(!env.vm.catalog().paths().iter().any(|path| path == &files_b[0]));
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files_a[0].clone())
    );
}

#[test]
fn undo_registered_in_a_previous_folder_reopens_that_folder() {
    // C#: FolderChange.UndoRegisteredInPreviousFolder_ReopensThatFolder (APP-03)
    let mut env = MainEnv::new();
    let (folder1, files1) = album(&mut env, "app03_u1", &["1.jpg", "2.jpg"]);
    let (folder2, _) = env.images("app03_u2", &["x.jpg"]);
    env.vm.run_action(0);
    env.settle();
    assert_eq!(undo_len(&env), 1);
    env.open(&folder2);

    env.vm.undo();
    env.settle();

    assert!(exists(&files1[0]));
    assert_eq!(env.vm.session().map(|session| session.folder.clone()), Some(folder1));
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files1[0].clone())
    );
}

#[test]
fn undo_reopen_is_dropped_when_the_user_opened_another_folder_during_a_recycle_undo() {
    // C#: FolderScope.UndoRecycle_UserOpensOtherFolderDuringUndo_DoesNotReopenUndoFolder (R09)
    let mut env = MainEnv::new();
    let (_, files_a) = album(&mut env, "undo_scope_a", &["1.jpg", "2.jpg"]);
    let (folder_c, files_c) = env.images("undo_scope_c", &["x.jpg"]);
    env.vm.recycle();
    env.settle();
    assert!(!exists(&files_a[0]));

    env.vm.undo();
    in_flight(&mut env);
    assert!(env.vm.is_file_action_in_progress());
    env.vm
        .open_folder(&folder_c, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("folder C to load", &["FileAction"], |_| false);
    env.settle();

    assert!(exists(&files_a[0]), "the undo itself is real on disk");
    assert_eq!(env.vm.session().map(|session| session.folder.clone()), Some(folder_c));
    assert_eq!(env.vm.catalog().paths().to_vec(), files_c);
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn undo_reopen_is_dropped_when_the_user_opened_another_folder_during_a_move_undo() {
    // C#: FolderScope.UndoMoveFromPreviousFolder_UserOpensOtherFolderDuringUndo_DoesNotReopenUndoFolder (R09)
    let mut env = MainEnv::new();
    let (_, files_a) = album(&mut env, "undo_scope_move_a", &["1.jpg", "2.jpg"]);
    let (folder_b, _) = env.images("undo_scope_move_b", &["y.jpg"]);
    let (folder_c, files_c) = env.images("undo_scope_move_c", &["x.jpg"]);
    env.vm.run_action(0); // Move A\1.jpg to A\Sorted
    env.settle();
    env.open(&folder_b);

    env.vm.undo();
    in_flight(&mut env);
    assert!(env.vm.is_file_action_in_progress());
    env.vm
        .open_folder(&folder_c, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("folder C to load", &["FileAction"], |_| false);
    env.settle();

    assert!(exists(&files_a[0]));
    assert_eq!(env.vm.session().map(|session| session.folder.clone()), Some(folder_c));
    assert_eq!(env.vm.catalog().paths().to_vec(), files_c);
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn undo_holds_the_gate_until_the_reopen_finished() {
    // New test (the gate turn of an undo that reopens a folder is released by the reopen, as the C# `finally`)
    let mut env = MainEnv::new();
    let (_, files_a) = album(&mut env, "undo_gate_a", &["1.jpg", "2.jpg"]);
    let (folder_b, _) = env.images("undo_gate_b", &["y.jpg"]);
    env.vm.run_action(0);
    env.settle();
    env.open(&folder_b);

    env.vm.undo();
    env.settle_holding("the reopen to start", &["Scan", "Explorer"], |_| false);

    assert!(
        env.vm.is_file_action_in_progress(),
        "the reopen of the undo's folder is still loading"
    );
    assert!(
        env.vm.run_action(0).is_empty(),
        "no file action may start while the undo's reopen runs"
    );
    env.settle();
    assert!(!env.vm.is_file_action_in_progress());
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(files_a[0].clone())
    );
}

#[test]
fn undo_with_no_history_reports_nothing_to_undo_and_releases_the_gate() {
    // C#: MainViewModelFileActionTests.UndoAsync_WithNoHistory_ReportsNothingToUndo_LeavesCatalog_AndReleasesGate
    let mut env = MainEnv::new();
    let (_, files) = album(&mut env, "undo_no_history", &["1.jpg", "2.jpg"]);
    let current_before = env.vm.catalog().current().map(|entry| entry.path().to_owned());

    env.vm.undo();
    env.settle();

    assert_eq!(env.vm.status_text(), env.text("core.undo.nothingToUndo"));
    assert_eq!(env.vm.total_files(), 2);
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        current_before
    );
    assert!(!env.vm.is_file_action_in_progress());

    // The gate was released: the next file action runs and registers an undo entry.
    env.vm.run_action(0);
    env.settle();
    assert!(!exists(&files[0]));
    assert_eq!(undo_len(&env), 1);
}

#[test]
fn undo_without_any_folder_reports_nothing_to_undo() {
    // C#: FolderChange.UndoWithoutAnyFolder_ReportsNothingToUndo
    let mut env = MainEnv::new();

    env.vm.undo();
    env.settle();

    assert_eq!(env.vm.status_text(), env.text("core.undo.nothingToUndo"));
    assert!(!env.vm.is_file_action_in_progress());
    assert_eq!(env.vm.total_files(), 0);
}

#[test]
fn folder_switch_mid_undo_releases_the_gate_and_does_not_touch_the_new_catalog() {
    // C#: MainViewModelFileActionTests.OC14_FolderSwitchMidUndo_ReleasesGate_AndDoesNotTouchNewCatalog
    let mut env = MainEnv::new();
    album(&mut env, "oc14_switch1", &["1.jpg", "2.jpg"]);
    let (folder2, files2) = env.images("oc14_switch2", &["x.jpg"]);
    env.vm.run_action(0);
    env.settle();

    env.vm.undo();
    in_flight(&mut env);
    env.vm
        .open_folder(&folder2, None)
        .unwrap_or_else(|error| panic!("open: {error}"));
    env.settle_holding("folder 2 to load", &["FileAction"], |_| false);
    env.settle();

    assert!(!env.vm.is_file_action_in_progress());
    assert_eq!(env.vm.catalog().paths().to_vec(), files2);
}

#[test]
fn undo_of_a_recycle_in_the_same_folder_puts_the_photo_back_at_its_position_and_keeps_the_current_photo_without_reload()
{
    // C#: MainViewModelFileActionTests.UndoRecycle_InTheSameFolder_PutsThePhotoBackAtItsPosition_KeepsTheCurrentPhoto_WithoutReload
    let mut env = MainEnv::new();
    let (_, paths) = album(
        &mut env,
        "undo_recycle_in_place",
        &["1.jpg", "2.jpg", "3.jpg", "4.jpg", "5.jpg"],
    );
    env.vm.next();
    env.settle();
    env.vm.next();
    env.settle();
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(paths[2].clone())
    );
    let queries_before = env.explorer.queries();
    assert!(queries_before > 0);

    env.vm.recycle();
    env.settle();
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(paths[3].clone())
    );
    let presents_before_undo = lock(&env.sink_seen).presented.len();
    env.preload.clear_events();

    env.vm.undo();
    env.settle();

    assert!(exists(&paths[2]));
    // Back at its old place (3rd), not at the front; the photo under review is still 4.jpg.
    assert_eq!(env.vm.catalog().paths().to_vec(), paths);
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(paths[3].clone())
    );
    assert_eq!(env.vm.current_index(), 3);
    // No folder reload (it would query the Explorer order again) and the restored photo is not presented.
    assert_eq!(env.explorer.queries(), queries_before);
    assert!(!lock(&env.sink_seen).presented[presents_before_undo..].contains(&paths[2]));
    assert_eq!(
        env.vm.session().and_then(|session| session.current_path.clone()),
        Some(paths[3].clone())
    );
    // Preload re-centred on the shifted index, so stepping back to the restored photo is warm.
    assert!(env.preload.preload_centers().contains(&3), "{:?}", env.preload.events());

    env.vm.previous();
    env.settle();
    assert_eq!(
        env.vm.catalog().current().map(|entry| entry.path().to_owned()),
        Some(paths[2].clone())
    );
}

// ---- viewer commands --------------------------------------------------------------------------------------------------------

#[test]
fn viewer_controls_delegate_to_the_viewer_state() {
    // C#: MainViewModelNavigationTests.ViewerControls_DelegateCorrectlyToViewerState
    let mut env = MainEnv::new();
    let step = f64::from(env.vm.settings().keyboard_zoom_step_percent) / 100.0;

    let zoomed = env.vm.zoom_in();
    assert_eq!(env.viewer.borrow().zoom(), 1.0 + step);
    assert!(zoomed.viewer.contains(ViewerProperty::Zoom));

    env.vm.zoom_out();
    assert_eq!(env.viewer.borrow().zoom(), 1.0);

    env.vm.toggle_fit();
    assert!(env.viewer.borrow().is_fit());

    env.vm.toggle_fullscreen();
    assert!(env.viewer.borrow().is_fullscreen());

    env.vm.exit_fullscreen();
    assert!(!env.viewer.borrow().is_fullscreen());
}

#[test]
fn zoom_actual_size_with_an_image_is_100_percent_of_source_pixels_and_a_no_op_without_one() {
    // C#: MainViewModelNavigationTests.ZoomActualSize_WithImage_Sets100PercentOfSourcePixels_WithoutImage_IsNoOp
    let mut env = MainEnv::new();

    assert!(env.vm.zoom_actual_size().is_empty());
    assert!(env.viewer.borrow().is_fit());

    album(&mut env, "album_zoom100", &["a.jpg"]);
    env.vm.zoom_in();
    env.vm.zoom_actual_size();

    assert!(!env.viewer.borrow().is_fit());
    assert_eq!(env.viewer.borrow().zoom(), ACTUAL_SIZE_ZOOM);
    assert_eq!(env.viewer.borrow().effective_zoom(), Some(1.0));
}

#[test]
fn toggle_fit_resets_with_a_zero_viewport() {
    // New test (C# `ResetFit()` defaults: viewport 0 x 0, which is not a usable reading and leaves the limits alone)
    let mut env = MainEnv::new();
    env.viewer.borrow_mut().update_viewport(640.0, 480.0, true);
    env.vm.zoom_in();
    assert!(!env.viewer.borrow().is_fit());
    assert_eq!(env.viewer.borrow().max_image_width(), f64::INFINITY);

    let changes = env.vm.toggle_fit();

    assert!(env.viewer.borrow().is_fit());
    assert!(changes.viewer.contains(ViewerProperty::Zoom) || changes.viewer.contains(ViewerProperty::Stretch));
    // Zooming set the limits to "unbounded"; the 0 x 0 viewport of the default `ResetFit()` is not a usable reading, so they stay so.
    assert_eq!(env.viewer.borrow().max_image_width(), f64::INFINITY);
    assert_eq!(env.viewer.borrow().max_image_height(), f64::INFINITY);
}

#[test]
fn zoom_mode_change_asks_the_presenter_for_the_new_effective_zoom() {
    // New test (D-08a-6: a zoom mode change reaches the presenter, which arms the full-resolution decode)
    let decoder = SizedDecoder::new();
    let gate = decoder.gate("a.jpg");
    let mut env = MainEnv::with_options(EnvOptions {
        decoder: decoder.clone(),
        ..EnvOptions::default()
    });
    album(&mut env, "zoom_detail", &["a.jpg"]);
    assert!(!env.vm.presenter().zoom_detail().pending_load());

    env.vm.zoom_actual_size();

    assert!(
        env.vm.presenter().zoom_detail().pending_load(),
        "zooming to 100 % of a 600 px preview needs the original"
    );
    gate.open();
    env.settle();
    assert!(env.vm.presenter().zoom_detail().is_showing_original());
}

// ---- info overlay and refresh -----------------------------------------------------------------------------------------------

#[test]
fn toggle_info_overlay_flips_visibility_and_persists_to_config() {
    // C#: MainViewModelNavigationTests.ToggleInfoOverlay_FlipsVisibility_AndPersistsToConfig
    let env = MainEnv::new();
    let mut env = env;
    assert!(is_file_info_visible(&env));
    assert!(env.vm.is_status_panel_visible());

    let changes = env.vm.toggle_info_overlay();

    assert!(!is_file_info_visible(&env));
    assert!(!env.vm.is_status_panel_visible());
    assert!(changes.main.contains(MainProperty::IsStatusPanelVisible));
    assert!(!reload_settings(&env).show_info_overlay);

    env.vm.toggle_info_overlay();

    assert!(is_file_info_visible(&env));
    assert!(reload_settings(&env).show_info_overlay);
}

fn is_file_info_visible(env: &MainEnv) -> bool {
    pr_app::viewmodels::info_overlay::InfoOverlayViewModel::<pr_app::viewmodels::main::MainFolderInfoLabels>::is_file_info_visible(
        &env.vm.settings(),
    )
}

/// The settings a second store reads back from `config.json`.
fn reload_settings(env: &MainEnv) -> Arc<AppSettings> {
    let store = pr_core::settings::store::SettingsStore::new(
        env.paths.clone(),
        Arc::new(pr_core::io::physical::PhysicalFileSystem),
        Arc::new(pr_test_support::RecordingLog::new()),
        Arc::new(pr_core::abstractions::SystemClock),
        Arc::new(pr_core::settings::app_settings::EnglishSettingsTexts),
    );
    store
        .load(None)
        .unwrap_or_else(|error| panic!("reload the settings: {error}"))
}

#[test]
fn toggle_info_overlay_when_saving_fails_still_toggles_for_this_session() {
    // C#: MutationGaps.ToggleInfoOverlay_WhenSavingTheSettingFails_StillTogglesForThisSession
    let mut env = MainEnv::new();
    let before = env.vm.settings().show_info_overlay;
    let config = env.paths.config_file.clone();
    let _ = std::fs::remove_file(&config);
    std::fs::create_dir_all(&config).unwrap_or_else(|error| panic!("block the settings file: {error}")); // it can no longer be written

    env.vm.toggle_info_overlay();

    assert_eq!(env.vm.settings().show_info_overlay, !before);
    assert!(
        env.log.contains(
            pr_test_support::LogLevel::Error,
            "Could not save the info overlay setting"
        ),
        "{:?}",
        env.log.entries()
    );
}

fn refresh_env(decoder: Arc<SizedDecoder>) -> MainEnv {
    let mut env = MainEnv::with_options(EnvOptions {
        decoder,
        ..EnvOptions::default()
    });
    album(&mut env, "refresh", &["a.jpg", "b.jpg"]);
    env.viewer.borrow_mut().set_dpi_scale(1.0);
    env
}

#[test]
fn refresh_view_at_fit_with_a_window_larger_than_the_preview_loads_the_original_and_stays_at_fit() {
    // C#: ZoomSwapTests.Refresh.RefreshView_AtFit_WindowLargerThanThePreview_LoadsTheOriginalAndStaysAtFit
    let decoder = SizedDecoder::new();
    let gate = decoder.gate("a.jpg");
    let mut env = refresh_env(decoder);
    env.viewer.borrow_mut().update_viewport(1200.0, 800.0, true); // Fit zoom 0.2 -> 1200 px on screen, the preview has 600
    assert!(env.viewer.borrow().is_fit());

    let (outcome, _) = env.vm.refresh_view();

    let outcome = outcome.expect("an image is shown");
    assert_eq!(outcome.resolution, ZoomDetailRefreshResult::LoadingOriginal);
    assert!(env.vm.presenter().zoom_detail().pending_load());
    gate.open();
    env.settle();

    assert!(env.vm.presenter().zoom_detail().is_showing_original());
    assert!(
        env.viewer.borrow().is_fit(),
        "the view did not move: still Fit, same zoom"
    );
    assert_eq!(env.viewer.borrow().zoom(), 1.0);
    assert_eq!(
        env.viewer.borrow().effective_scaling_quality(),
        ScalingQuality::HighQuality
    );
}

#[test]
fn refresh_view_at_fit_with_a_large_enough_preview_loads_nothing() {
    // C#: ZoomSwapTests.Refresh.RefreshView_AtFit_PreviewLargeEnough_LoadsNothing
    let decoder = SizedDecoder::new();
    let mut env = refresh_env(decoder);
    env.viewer.borrow_mut().update_viewport(600.0, 400.0, true); // Fit zoom 0.1 -> 600 px = the preview

    let (outcome, _) = env.vm.refresh_view();

    assert_eq!(
        outcome.map(|outcome| outcome.resolution),
        Some(ZoomDetailRefreshResult::PreviewSufficient)
    );
    assert!(!env.vm.presenter().zoom_detail().pending_load());
    assert!(!env.vm.presenter().zoom_detail().is_showing_original());
}

#[test]
fn refresh_view_zoomed_uses_the_zoom_not_the_fit_zoom_and_keeps_it() {
    // C#: ZoomSwapTests.Refresh.RefreshView_Zoomed_UsesTheZoomNotTheFitZoom_AndKeepsIt
    let decoder = SizedDecoder::new();
    let gate = decoder.gate("a.jpg");
    let mut env = refresh_env(decoder);
    env.viewer.borrow_mut().update_viewport(600.0, 400.0, true); // Fit would need only 600 px = the preview...
    env.viewer.borrow_mut().set_zoom(2.0); // ...but the view is zoomed to 12000 px: the original is needed

    let (outcome, _) = env.vm.refresh_view();

    assert_eq!(
        outcome.map(|outcome| outcome.resolution),
        Some(ZoomDetailRefreshResult::LoadingOriginal)
    );
    assert_eq!(env.viewer.borrow().zoom(), 2.0);
    assert!(!env.viewer.borrow().is_fit());
    gate.open();
    env.settle();
}

#[test]
fn refresh_view_with_linear_scaling_is_forced_to_high_quality_for_this_image_only() {
    // C#: ZoomSwapTests.Refresh.RefreshView_LinearScaling_IsForcedToHighQualityForThisImageOnly
    let decoder = SizedDecoder::new();
    let mut env = refresh_env(decoder);
    env.viewer.borrow_mut().set_scaling_quality(ScalingQuality::Linear);
    assert_eq!(env.viewer.borrow().effective_scaling_quality(), ScalingQuality::Linear);

    let (outcome, changes) = env.vm.refresh_view();

    assert!(outcome.expect("an image is shown").scaling_changed);
    assert_eq!(
        env.viewer.borrow().effective_scaling_quality(),
        ScalingQuality::HighQuality
    );
    assert_eq!(
        env.viewer.borrow().scaling_quality(),
        ScalingQuality::Linear,
        "the setting itself is untouched"
    );
    assert!(changes.viewer.contains(ViewerProperty::ForceHighQualityScaling));

    assert!(
        !env.vm.refresh_view().0.expect("an image is shown").scaling_changed,
        "already HighQuality now"
    );

    env.vm.next();
    env.settle();
    assert_eq!(
        env.viewer.borrow().effective_scaling_quality(),
        ScalingQuality::Linear,
        "the next image follows the setting again"
    );
}

#[test]
fn refresh_view_without_images_does_nothing() {
    // C#: ZoomSwapTests.Refresh.RefreshView_WithoutImages_DoesNothing
    let mut env = MainEnv::new();

    let (outcome, changes) = env.vm.refresh_view();

    assert!(outcome.is_none());
    assert!(changes.is_empty());
    assert!(!env.viewer.borrow().force_high_quality_scaling());
}

// ---- external editor and clipboard ------------------------------------------------------------------------------------------

fn fake_editor_exe(env: &MainEnv, name: &str) -> String {
    let path = env.root.join(name);
    std::fs::write(&path, b"fake editor; never executed").unwrap();
    path.to_string_lossy().into_owned()
}

fn configure_editor(env: &MainEnv) {
    let path = fake_editor_exe(env, "editor.exe");
    env.update_settings(|settings| settings.external_editor_path = path);
}

type TemplateLaunches = Arc<Mutex<Vec<(String, String, String)>>>;
struct TemplateLauncher(TemplateLaunches);
impl ExternalEditorLauncher for TemplateLauncher {
    fn launch(&mut self, exe: &str, file: &str, arguments: &str) -> Result<(), ExternalEditorFailure> {
        lock(&self.0).push((exe.into(), file.into(), arguments.into()));
        Ok(())
    }
}

fn template_launcher(env: &mut MainEnv) -> TemplateLaunches {
    let trace = Arc::new(Mutex::new(Vec::new()));
    env.vm
        .set_external_editor_launcher(Box::new(TemplateLauncher(trace.clone())));
    trace
}

// C#: MainViewModelAdvancedTests.OpenInExternalEditor_ById_LaunchesThatEditorWithItsArgumentTemplate.
#[test]
fn open_in_external_editor_by_id_launches_that_editor_with_its_argument_template() {
    let mut env = MainEnv::new();
    let exe = fake_editor_exe(&env, "second.exe");
    env.update_settings(|s| {
        let mut editor = editor_fixture("second", &exe);
        editor.arguments = "--edit {file}".into();
        s.external_editors = vec![editor_fixture("first", "missing.exe"), editor];
        s.default_external_editor_id = "first".into();
    });
    let (_, photo) = single_image_album(&mut env, "editor_by_id");
    let trace = template_launcher(&mut env);
    env.vm.open_in_external_editor_by_id("second").unwrap();
    assert_eq!(*lock(&trace), [(exe, photo.clone(), format!("--edit \"{photo}\""))]);
}

// C#: MainViewModelAdvancedTests.OpenInExternalEditor_NoId_UsesTheDefaultEditor_AndAnUnknownIdDoesNothing.
#[test]
fn open_in_external_editor_no_id_uses_default_and_unknown_id_does_nothing() {
    let mut env = MainEnv::new();
    let exe = fake_editor_exe(&env, "default.exe");
    let mut settings = (*env.settings.current()).clone();
    settings.external_editors = vec![editor_fixture("a", "missing.exe"), editor_fixture("b", &exe)];
    settings.default_external_editor_id = "b".into();
    settings.external_editor_path.clear(); // stale mirror must not override the declared default.
    env.settings.set_override(Some(Arc::new(settings)));
    let (_, photo) = single_image_album(&mut env, "editor_default");
    assert!(env.vm.can_open_in_external_editor());
    let trace = template_launcher(&mut env);
    env.vm.open_in_external_editor().unwrap();
    env.vm.open_in_external_editor_by_id("unknown").unwrap();
    assert_eq!(*lock(&trace), [(exe, photo.clone(), format!("\"{photo}\""))]);
    assert!(lock(&env.dialog.settings_targets).is_empty());
    assert!(error_dialogs(&env).is_empty());
}

// C#: MainViewModelAdvancedTests.OpenInExternalEditor_RawCompanionOn_SendsTheRawOriginal_Off_SendsTheShownFile.
#[test]
fn open_in_external_editor_raw_companion_on_sends_raw_off_sends_shown_file() {
    let mut env = MainEnv::new();
    configure_editor(&env);
    let (_, photo) = single_image_album(&mut env, "editor_raw");
    let raw = Path::new(&photo).with_extension("arw");
    std::fs::write(&raw, b"fake raw; never decoded").unwrap();
    let trace = template_launcher(&mut env);
    env.vm.open_in_external_editor().unwrap();
    env.update_settings(|s| s.external_editor_sends_raw_original = false);
    env.vm.open_in_external_editor().unwrap();
    let trace = lock(&trace);
    assert_eq!(trace[0].1, raw.to_string_lossy());
    assert_eq!(trace[1].1, photo);
    assert_eq!(trace[0].2, format!("\"{}\"", raw.display()));
}

// Source-derived: catalog pairing wins over sibling search, even when its RAW has a different stem.
#[test]
fn open_in_external_editor_prefers_catalog_raw_pair_over_sibling_search() {
    let mut env = MainEnv::with_options(EnvOptions {
        catalog_builder: Some(Box::new(|photos| {
            use pr_core::catalog::{
                review_catalog::ReviewCatalog,
                types::{CaptureGroup, CatalogEntry},
            };
            let shown = photos.join("shown.png");
            let paired = photos.join("catalog-pair.nef");
            let sibling = photos.join("shown.arw");
            for path in [&shown, &paired, &sibling] {
                std::fs::write(path, b"fake image; never decoded").unwrap();
            }
            let shown = shown.to_string_lossy().into_owned();
            let mut entry = CatalogEntry::new(shown.clone()).unwrap();
            entry.capture_group = Some(CaptureGroup::new(shown, paired.to_string_lossy().into_owned(), None).unwrap());
            let mut catalog = ReviewCatalog::new();
            catalog.reset(vec![entry], pr_core::model::RawPairMode::PreferJpeg, None);
            catalog
        })),
        ..EnvOptions::default()
    });
    configure_editor(&env);
    let trace = template_launcher(&mut env);
    env.vm.open_in_external_editor().unwrap();
    assert_eq!(lock(&trace)[0].1, env.photos.join("catalog-pair.nef").to_string_lossy());
}

// C#: MainViewModelAdvancedTests.OpenInExternalEditor_EditorProgramMissing_ShowsAnErrorAndDoesNotLaunch.
#[test]
fn open_in_external_editor_editor_program_missing_shows_error_and_does_not_launch() {
    let mut env = MainEnv::new();
    let missing = env.root.join("missing-editor.exe").to_string_lossy().into_owned();
    env.update_settings(|s| s.external_editor_path = missing.clone());
    single_image_album(&mut env, "editor_exe_missing");
    let trace = template_launcher(&mut env);
    env.vm.open_in_external_editor().unwrap();
    assert!(lock(&trace).is_empty());
    let errors = error_dialogs(&env);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].1.contains(&missing));
}

fn editor_fixture(id: &str, path: &str) -> pr_core::settings::external_editors::ExternalEditor {
    pr_core::settings::external_editors::ExternalEditor {
        id: id.into(),
        name: id.into(),
        path: path.into(),
        ..pr_core::settings::external_editors::ExternalEditor::default()
    }
}

// C#: MainViewModelAdvancedTests.AddExternalEditor_AppendsTheChosenEditor_AssignsAFreshId_AndKeepsTheDefault.
#[test]
fn add_external_editor_appends_the_chosen_editor_assigns_a_fresh_id_and_keeps_the_default() {
    let mut env = MainEnv::new();
    env.update_settings(|s| {
        s.external_editors = vec![editor_fixture("old", r"C:\Old\editor.exe")];
        s.default_external_editor_id = "old".into();
    });
    *lock(&env.dialog.added_editor) = Some(editor_fixture("from-dialog", r"C:\New\editor.exe"));
    env.vm.add_external_editor();
    let settings = env.settings.current();
    assert_eq!(settings.external_editors.len(), 2);
    assert_eq!(settings.default_external_editor_id, "old");
    assert_eq!(settings.external_editors[1].path, r"C:\New\editor.exe");
    assert_ne!(settings.external_editors[1].id, "from-dialog");
    assert_ne!(settings.external_editors[1].id, "old");
    assert_eq!(*lock(&env.dialog.editor_initials), [Some(PathBuf::from(r"C:\Old"))]);
    assert_eq!(env.store.current().external_editors, settings.external_editors);
}

// C#: MainViewModelAdvancedTests.AddExternalEditor_Cancelled_ChangesNothing.
#[test]
fn add_external_editor_cancelled_changes_nothing() {
    let mut env = MainEnv::new();
    let before = env.settings.current();
    env.vm.add_external_editor();
    assert_eq!(env.settings.current().external_editors, before.external_editors);
    assert_eq!(lock(&env.dialog.editor_initials).len(), 1);
}

// C#: MainViewModelAdvancedTests.AddExternalEditor_WhenOnlyTheLegacyPathExists_KeepsItAsTheDefaultAndAppends.
#[test]
fn add_external_editor_when_only_the_legacy_path_exists_keeps_it_as_the_default_and_appends() {
    let mut env = MainEnv::new();
    let mut legacy = (*env.settings.current()).clone();
    legacy.external_editor_path = r"C:\Legacy\retouch.exe".into();
    env.settings.set_override(Some(Arc::new(legacy)));
    *lock(&env.dialog.added_editor) = Some(editor_fixture("new", r"C:\New\editor.exe"));
    env.vm.add_external_editor();
    let settings = env.settings.current();
    assert_eq!(settings.external_editors.len(), 2);
    assert_eq!(settings.external_editors[0].path, r"C:\Legacy\retouch.exe");
    assert_eq!(settings.default_external_editor_id, settings.external_editors[0].id);
    assert_eq!(settings.external_editor_path, r"C:\Legacy\retouch.exe");
}

// C#: MainViewModelAdvancedTests.ChooseDefaultExternalEditor_StoresTheChosenId_AndMirrorsItsPath.
#[test]
fn choose_default_external_editor_stores_the_chosen_id_and_mirrors_its_path() {
    let mut env = MainEnv::new();
    env.update_settings(|s| {
        s.external_editors = vec![editor_fixture("a", r"C:\a.exe"), editor_fixture("b", r"C:\b.exe")];
        s.default_external_editor_id = "a".into();
    });
    *lock(&env.dialog.chosen_editor) = Some("b".into());
    env.vm.choose_default_external_editor();
    assert_eq!(env.settings.current().default_external_editor_id, "b");
    assert_eq!(env.settings.current().external_editor_path, r"C:\b.exe");
    assert_eq!(lock(&env.dialog.editor_choices)[0].1.as_deref(), Some("a"));
}

// C#: MainViewModelAdvancedTests.ChooseDefaultExternalEditor_CancelledOrUnknownId_KeepsTheDefault.
#[test]
fn choose_default_external_editor_cancelled_or_unknown_id_keeps_the_default() {
    for answer in [None, Some("gone".into())] {
        let mut env = MainEnv::new();
        env.update_settings(|s| {
            s.external_editors = vec![editor_fixture("a", r"C:\a.exe")];
            s.default_external_editor_id = "a".into();
        });
        *lock(&env.dialog.chosen_editor) = answer;
        env.vm.choose_default_external_editor();
        assert_eq!(env.settings.current().default_external_editor_id, "a");
        assert_eq!(lock(&env.dialog.editor_choices).len(), 1);
    }
}

// Source-derived W9-E5 cap/blank guards must return before begin_edit or a disk save.
#[test]
fn add_external_editor_full_list_or_blank_path_never_saves_or_installs_an_override() {
    for full in [false, true] {
        let mut env = MainEnv::new();
        if full {
            env.update_settings(|s| {
                s.external_editors = (0..32)
                    .map(|i| editor_fixture(&format!("e{i}"), &format!(r"C:\e{i}.exe")))
                    .collect();
                s.default_external_editor_id = "e0".into();
            });
        }
        let before = std::fs::read(&env.paths.config_file).unwrap();
        *lock(&env.dialog.added_editor) = Some(editor_fixture("added", if full { r"C:\added.exe" } else { " " }));
        env.vm.add_external_editor();
        assert_eq!(std::fs::read(&env.paths.config_file).unwrap(), before);
        assert!(!env.settings.has_override());
        assert_eq!(env.settings.current().external_editors.len(), if full { 32 } else { 0 });
    }
}

// Source-derived cap guard: an unwritable destination distinguishes no save from saving identical bytes.
#[test]
fn add_external_editor_full_list_skips_save_even_when_config_cannot_be_written() {
    let mut env = MainEnv::new();
    env.update_settings(|settings| {
        settings.external_editors = (0..32)
            .map(|i| editor_fixture(&format!("e{i}"), &format!(r"C:\e{i}.exe")))
            .collect();
        settings.default_external_editor_id = "e0".into();
    });
    let config = env.paths.config_file.clone();
    std::fs::remove_file(&config).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    *lock(&env.dialog.added_editor) = Some(editor_fixture("added", r"C:\added.exe"));
    env.vm.add_external_editor();
    assert!(
        !env.settings.has_override(),
        "a full list must not begin/commit an edit"
    );
    assert!(!env.log.contains(
        pr_test_support::LogLevel::Error,
        "Could not save the external editors setting"
    ));
    assert_eq!(env.settings.current().external_editors.len(), 32);
}

// Source-derived W9-E5 persistence failure keeps the edit in this session and logs it.
#[test]
fn add_external_editor_save_failure_keeps_session_edit_and_logs_failure() {
    let mut env = MainEnv::new();
    let config = env.paths.config_file.clone();
    std::fs::remove_file(&config).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    *lock(&env.dialog.added_editor) = Some(editor_fixture("new", r"C:\new.exe"));
    env.vm.add_external_editor();
    assert_eq!(env.settings.current().external_editors.len(), 1);
    assert_eq!(env.settings.current().external_editor_path, r"C:\new.exe");
    assert!(env.settings.has_override());
    assert!(env.log.contains(
        pr_test_support::LogLevel::Error,
        "Could not save the external editors setting"
    ));
}

// C# materializes the live legacy list before prompting even when ChooseDefault is canceled.
#[test]
fn choose_default_external_editor_legacy_cancel_materializes_without_saving() {
    let mut env = MainEnv::new();
    let mut legacy = (*env.settings.current()).clone();
    legacy.external_editor_path = r"C:\Legacy\editor.exe".into();
    env.settings.set_override(Some(Arc::new(legacy)));
    let before = std::fs::read(&env.paths.config_file).unwrap();
    env.vm.choose_default_external_editor();
    assert_eq!(env.settings.current().external_editors.len(), 1);
    assert_eq!(lock(&env.dialog.editor_choices)[0].0.len(), 1);
    assert_eq!(std::fs::read(&env.paths.config_file).unwrap(), before);
}

type Launches = Arc<Mutex<Vec<(String, String)>>>;

struct TestLauncher {
    launches: Launches,
    failure: Arc<Mutex<Option<ExternalEditorFailure>>>,
}

impl ExternalEditorLauncher for TestLauncher {
    fn launch(&mut self, exe_path: &str, file_path: &str, _command_line: &str) -> Result<(), ExternalEditorFailure> {
        lock(&self.launches).push((exe_path.to_owned(), file_path.to_owned()));
        match lock(&self.failure).clone() {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }
}

fn install_launcher(env: &mut MainEnv) -> (Launches, Arc<Mutex<Option<ExternalEditorFailure>>>) {
    let launches: Launches = Arc::new(Mutex::new(Vec::new()));
    let failure = Arc::new(Mutex::new(None));
    env.vm.set_external_editor_launcher(Box::new(TestLauncher {
        launches: Arc::clone(&launches),
        failure: Arc::clone(&failure),
    }));
    (launches, failure)
}

fn single_image_album(env: &mut MainEnv, name: &str) -> (String, String) {
    let (folder, files) = album(env, name, &["1.png"]);
    (folder, files[0].clone())
}

#[test]
fn can_open_in_external_editor_is_true_only_when_the_path_is_set_and_an_image_is_open() {
    // C#: MainViewModelAdvancedTests.CanOpenInExternalEditor_TrueOnlyWhenPathSetAndImageOpen
    let mut env = MainEnv::new();
    assert!(!env.vm.can_open_in_external_editor(), "no editor path, no image");

    configure_editor(&env);
    assert!(
        !env.vm.can_open_in_external_editor(),
        "editor path set, but no image open yet"
    );

    single_image_album(&mut env, "editor_can_open");
    assert!(env.vm.has_images());
    assert!(env.vm.can_open_in_external_editor(), "both conditions now hold");

    env.update_settings(|settings| {
        // Saving now materializes the legacy editor. Clear the declared list as well
        // when arranging "no editor"; changing only the mirror preserves the editor.
        settings.external_editors.clear();
        settings.default_external_editor_id.clear();
        settings.external_editor_path = "   ".to_owned();
    });
    assert!(
        !env.vm.can_open_in_external_editor(),
        "a whitespace-only path counts as not configured"
    );
}

#[test]
fn open_in_external_editor_launches_with_the_current_photo_path() {
    // C#: MainViewModelAdvancedTests.OpenInExternalEditor_LaunchesConfiguredEditorWithCurrentPhotoPath
    let mut env = MainEnv::new();
    configure_editor(&env);
    let (_, photo) = single_image_album(&mut env, "editor_launch");
    let (launches, _) = install_launcher(&mut env);

    env.vm
        .open_in_external_editor()
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        lock(&launches).clone(),
        vec![(env.settings.current().external_editor_path.clone(), photo)]
    );
    assert!(error_dialogs(&env).is_empty());
}

#[test]
fn open_in_external_editor_when_the_file_no_longer_exists_shows_an_error_and_does_not_launch() {
    // C#: MainViewModelAdvancedTests.OpenInExternalEditor_FileNoLongerExists_ShowsErrorAndDoesNotLaunch
    let mut env = MainEnv::new();
    configure_editor(&env);
    let (_, photo) = single_image_album(&mut env, "editor_missing_file");
    std::fs::remove_file(&photo).unwrap_or_else(|error| panic!("delete: {error}")); // missing on disk, still the catalog's current entry
    let (launches, _) = install_launcher(&mut env);

    env.vm
        .open_in_external_editor()
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(lock(&launches).is_empty());
    let errors = error_dialogs(&env);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, env.text("dialog.externalEditor.failedTitle"));
    assert_eq!(
        errors[0].1,
        en_with("dialog.externalEditor.fileMissing", &[LocArg::new("fileName", "1.png")])
    );
}

#[test]
fn open_in_external_editor_with_an_editor_configured_does_not_open_settings() {
    // C#: MainViewModelAdvancedTests.OpenInExternalEditor_EditorConfigured_DoesNotOpenSettings
    let mut env = MainEnv::new();
    configure_editor(&env);
    single_image_album(&mut env, "editor_configured_no_settings");
    install_launcher(&mut env);

    env.vm
        .open_in_external_editor()
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(lock(&env.dialog.settings_targets).is_empty());
}

#[test]
fn open_in_external_editor_without_a_selection_uses_the_presented_photo_even_when_the_catalog_moved_on() {
    // C#: AppGaps.OpenInExternalEditor_WithoutASelection_UsesThePresentedPhotoEvenWhenTheCatalogMovedOn
    let mut env = MainEnv::new();
    configure_editor(&env);
    let (_, files) = album(&mut env, "ed_presented", &["a.png", "b.png"]);
    env.vm.catalog_mut_for_tests().set_current(1); // the catalog moved, the presenter still shows a
    let (launches, _) = install_launcher(&mut env);

    env.vm
        .open_in_external_editor()
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        lock(&launches).iter().map(|(_, file)| file.clone()).collect::<Vec<_>>(),
        vec![files[0].clone()]
    );
}

/// A catalog of one photo that exists under the photos folder of the environment, with no folder opened (nothing presented, no session).
fn catalog_only_options(folder_name: &'static str, shared: Arc<Mutex<Option<(String, String)>>>) -> EnvOptions {
    EnvOptions {
        catalog_builder: Some(Box::new(move |photos: &Path| {
            let folder = photos.join(folder_name);
            std::fs::create_dir_all(&folder).unwrap_or_else(|error| panic!("create: {error}"));
            let image = folder.join("x.png");
            std::fs::write(&image, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap_or_else(|error| panic!("write: {error}"));
            let image = image.to_string_lossy().into_owned();
            *lock(&shared) = Some((folder.to_string_lossy().into_owned(), image.clone()));
            let mut catalog = pr_core::catalog::review_catalog::ReviewCatalog::new();
            let entry = pr_core::catalog::types::CatalogEntry::new(image).unwrap_or_else(|error| panic!("{error:?}"));
            catalog.reset(vec![entry], pr_core::model::RawPairMode::Separate, None);
            catalog
        })),
        ..EnvOptions::default()
    }
}

#[test]
fn open_in_external_editor_when_nothing_is_presented_yet_falls_back_to_the_catalog_current() {
    // C#: AppGaps.OpenInExternalEditor_WhenNothingIsPresentedYet_FallsBackToTheCatalogCurrent
    let paths = Arc::new(Mutex::new(None));
    let mut env = MainEnv::with_options(catalog_only_options("ed_catalog", Arc::clone(&paths)));
    configure_editor(&env);
    let (_, image) = lock(&paths).clone().expect("the catalog was built");
    let (launches, _) = install_launcher(&mut env);

    env.vm
        .open_in_external_editor()
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        lock(&launches).iter().map(|(_, file)| file.clone()).collect::<Vec<_>>(),
        vec![image]
    );
}

#[test]
fn open_in_external_editor_with_a_comparison_selection_opens_the_selected_photo() {
    // C#: MutationGaps.OpenInExternalEditor_WithAComparisonSelection_OpensTheSelectedPhoto
    let mut env = MainEnv::new();
    configure_editor(&env);
    let (_, files) = album(&mut env, "editor_compare", &["1.png", "2.png"]);
    env.vm.compare_mut_for_tests().select(Some(files[1].clone()));
    let (launches, _) = install_launcher(&mut env);

    env.vm
        .open_in_external_editor()
        .unwrap_or_else(|error| panic!("{error}"));

    assert_eq!(
        lock(&launches).iter().map(|(_, file)| file.clone()).collect::<Vec<_>>(),
        vec![files[1].clone()]
    );
}

#[test]
fn open_in_external_editor_when_the_launch_fails_shows_the_reason_instead_of_throwing() {
    // C#: MutationGaps.OpenInExternalEditor_WhenTheLaunchFails_ShowsTheReasonInsteadOfThrowing (Win32Exception, InvalidOperation, FileNotFound)
    for message in ["no such editor", "the editor could not start", "file not found"] {
        let mut env = MainEnv::new();
        configure_editor(&env);
        single_image_album(&mut env, "editor_launch_fails");
        let (_, failure) = install_launcher(&mut env);
        *lock(&failure) = Some(ExternalEditorFailure::Expected {
            message: message.to_owned(),
        });

        env.vm
            .open_in_external_editor()
            .unwrap_or_else(|error| panic!("{error}"));

        let errors = error_dialogs(&env);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, env.text("dialog.externalEditor.failedTitle"));
        assert!(errors[0].1.contains(message));
    }
}

#[test]
fn open_in_external_editor_when_the_launch_is_unexpected_returns_the_bug_and_shows_no_dialog() {
    // C#: MutationGaps.OpenInExternalEditor_WhenTheLaunchThrowsSomethingUnexpected_DoesNotHideIt
    let mut env = MainEnv::new();
    configure_editor(&env);
    single_image_album(&mut env, "editor_launch_unexpected");
    let (_, failure) = install_launcher(&mut env);
    *lock(&failure) = Some(ExternalEditorFailure::Unexpected {
        message: "bug".to_owned(),
    });

    let error = env.vm.open_in_external_editor().unwrap_err();

    assert_eq!(error.to_string(), "bug");
    assert!(error_dialogs(&env).is_empty());
}

#[test]
fn the_production_launcher_missing_executable_is_an_expected_failure() {
    // New test (T10: a path with spaces or `&` must stay one argument; a missing exe is an expected failure)
    let mut launcher = pr_app::viewmodels::main::ProcessEditorLauncher;
    let missing = launcher.launch(
        r"C:\definitely\missing\editor.exe",
        r"C:\x y\a & b.png",
        r#""C:\x y\a & b.png""#,
    );
    assert!(
        matches!(missing, Err(ExternalEditorFailure::Expected { .. })),
        "{missing:?}"
    );
}

/// C# `FakeClipboard`.
struct FakeClipboard {
    succeed: bool,
    text: Arc<Mutex<Option<String>>>,
    calls: Arc<AtomicUsize>,
}

impl ClipboardService for FakeClipboard {
    fn try_set_text(&self, text: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.succeed {
            *lock(&self.text) = Some(text.to_owned());
        }
        self.succeed
    }
}

fn install_clipboard(env: &mut MainEnv, succeed: bool) -> (Arc<Mutex<Option<String>>>, Arc<AtomicUsize>) {
    let text = Arc::new(Mutex::new(None));
    let calls = Arc::new(AtomicUsize::new(0));
    env.vm.set_clipboard(Box::new(FakeClipboard {
        succeed,
        text: Arc::clone(&text),
        calls: Arc::clone(&calls),
    }));
    (text, calls)
}

#[test]
fn copy_file_name_puts_the_name_with_extension_on_the_clipboard_and_confirms_on_the_status_line() {
    // C#: CopyPath.CopyFileName_PutsNameWithExtensionOnClipboardAndConfirmsOnStatusLine
    let mut env = MainEnv::new();
    single_image_album(&mut env, "copy_name");
    let (text, _) = install_clipboard(&mut env, true);

    let changes = env.vm.copy_file_name();

    assert_eq!(lock(&text).clone(), Some("1.png".to_owned()));
    assert_eq!(
        env.vm.status_text(),
        en_with("status.copiedFileName", &[LocArg::new("fileName", "1.png")])
    );
    assert!(changes.main.contains(MainProperty::StatusText));
}

#[test]
fn copy_full_pathname_puts_the_full_path_on_the_clipboard_and_confirms_on_the_status_line() {
    // C#: CopyPath.CopyFullPathname_PutsFullPathOnClipboardAndConfirmsOnStatusLine
    let mut env = MainEnv::new();
    let (_, photo) = single_image_album(&mut env, "copy_path");
    let (text, _) = install_clipboard(&mut env, true);

    env.vm.copy_full_pathname();

    assert_eq!(lock(&text).clone(), Some(photo.clone()));
    assert_eq!(
        env.vm.status_text(),
        en_with("status.copiedFullPath", &[LocArg::new("path", photo.as_str())])
    );
}

#[test]
fn copy_with_a_busy_clipboard_shows_the_failure_status_instead_of_the_confirmation() {
    // C#: CopyPath.Copy_ClipboardBusy_ShowsFailureStatusInsteadOfConfirmation
    let mut env = MainEnv::new();
    single_image_album(&mut env, "copy_busy");
    let (_, calls) = install_clipboard(&mut env, false);

    env.vm.copy_file_name();
    assert_eq!(env.vm.status_text(), env.text("status.clipboardCopyFailed"));
    env.vm.copy_full_pathname();
    assert_eq!(env.vm.status_text(), env.text("status.clipboardCopyFailed"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn copy_without_an_injected_clipboard_reports_unavailable_instead_of_touching_the_real_clipboard() {
    // C#: CopyPath.Copy_WithoutAnInjectedClipboard_ReportsUnavailableInsteadOfTouchingTheRealClipboard
    let mut env = MainEnv::new();
    single_image_album(&mut env, "copy_unavailable");

    env.vm.copy_file_name();
    assert_eq!(env.vm.status_text(), env.text("status.clipboardCopyFailed"));
    env.vm.copy_full_pathname();
    assert_eq!(env.vm.status_text(), env.text("status.clipboardCopyFailed"));
}

#[test]
fn copy_with_no_image_open_is_disabled_and_does_not_touch_the_clipboard() {
    // C#: CopyPath.Copy_NoImageOpen_IsDisabledAndDoesNotTouchClipboard
    let mut env = MainEnv::new();
    let (_, calls) = install_clipboard(&mut env, true);

    assert!(!env.vm.can_copy_current_file_path());
    assert!(env.vm.copy_file_name().is_empty());
    assert!(env.vm.copy_full_pathname().is_empty());

    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn can_copy_current_file_path_is_true_once_an_image_is_open() {
    // C#: CopyPath.CanCopyCurrentFilePath_TrueOnceAnImageIsOpen
    let mut env = MainEnv::new();
    single_image_album(&mut env, "copy_can");
    assert!(env.vm.can_copy_current_file_path());
}

// ---- folder picker and dialogs ----------------------------------------------------------------------------------------------

#[test]
fn pick_and_open_folder_opens_the_selected_folder() {
    // C#: MainViewModelAdvancedTests.PickAndOpenFolderAsync_WhenFolderSelected_OpensFolder
    let mut env = MainEnv::new();
    let (folder, _) = env.images("picked_folder", &["a.png"]);
    *lock(&env.dialog.picked) = Some(PathBuf::from(&folder));

    env.vm.pick_and_open_folder();
    env.settle();

    assert_eq!(env.vm.total_files(), 1);
    assert_eq!(env.vm.current_index(), 0);
}

#[test]
fn pick_and_open_folder_starts_the_picker_in_the_open_folder() {
    // C#: MutationGaps.PickAndOpenFolderAsync_StartsThePickerInTheOpenFolder
    let mut env = MainEnv::new();
    let (folder, _) = single_image_album(&mut env, "picker_start");

    env.vm.pick_and_open_folder();

    assert_eq!(
        lock(&env.dialog.pick_initials).clone(),
        vec![Some(PathBuf::from(&folder))]
    );
}

#[test]
fn pick_and_open_folder_prefers_the_session_folder_over_the_catalog_folder() {
    // C#: AppGaps.PickAndOpenFolderAsync_PrefersTheSessionFolderOverTheCatalogFolder
    let mut env = MainEnv::new();
    let (folder, _) = single_image_album(&mut env, "pick_session_first");
    let other = env.folder("pick_session_first_other");
    let other_image = env.add_image(&other, "x.png");
    let entry = pr_core::catalog::types::CatalogEntry::new(other_image).unwrap_or_else(|error| panic!("{error:?}"));
    env.vm
        .catalog_mut_for_tests()
        .reset(vec![entry], pr_core::model::RawPairMode::Separate, None);

    env.vm.pick_and_open_folder();

    assert_eq!(
        lock(&env.dialog.pick_initials).clone(),
        vec![Some(PathBuf::from(folder))]
    );
}

#[test]
fn pick_and_open_folder_without_a_session_starts_in_the_catalog_folder() {
    // C#: AppGaps.PickAndOpenFolderAsync_WithoutASession_StartsInTheCatalogFolder
    let paths = Arc::new(Mutex::new(None));
    let mut env = MainEnv::with_options(catalog_only_options("pick_catalog_only", Arc::clone(&paths)));
    let (folder, _) = lock(&paths).clone().expect("the catalog was built");

    env.vm.pick_and_open_folder();

    assert_eq!(
        lock(&env.dialog.pick_initials).clone(),
        vec![Some(PathBuf::from(folder))]
    );
}

#[test]
fn pick_and_open_folder_with_nothing_open_starts_nowhere() {
    // C#: AppGaps.PickAndOpenFolderAsync_WithNothingOpen_StartsNowhere
    let mut env = MainEnv::new();

    env.vm.pick_and_open_folder();

    assert_eq!(lock(&env.dialog.pick_initials).clone(), vec![None]);
}

#[test]
fn show_benchmark_passes_the_open_folder() {
    // C#: MutationGaps.ShowBenchmark_PassesTheOpenFolder
    let mut env = MainEnv::new();
    let (folder, _) = single_image_album(&mut env, "benchmark_start");

    env.vm.show_benchmark();

    assert_eq!(
        lock(&env.dialog.benchmark_folders).clone(),
        vec![Some(PathBuf::from(folder))]
    );
}

#[test]
fn show_benchmark_prefers_the_session_folder_over_the_catalog_folder() {
    // C#: AppGaps.ShowBenchmark_PrefersTheSessionFolderOverTheCatalogFolder
    let mut env = MainEnv::new();
    let (folder, _) = single_image_album(&mut env, "bench_session_first");
    let other = env.folder("bench_session_first_other");
    let other_image = env.add_image(&other, "x.png");
    let entry = pr_core::catalog::types::CatalogEntry::new(other_image).unwrap_or_else(|error| panic!("{error:?}"));
    env.vm
        .catalog_mut_for_tests()
        .reset(vec![entry], pr_core::model::RawPairMode::Separate, None);

    env.vm.show_benchmark();

    assert_eq!(
        lock(&env.dialog.benchmark_folders).clone(),
        vec![Some(PathBuf::from(folder))]
    );
}

#[test]
fn show_benchmark_without_a_session_uses_the_catalog_folder_and_nothing_without_a_catalog() {
    // C#: AppGaps.ShowBenchmark_WithoutASession_UsesTheCatalogFolder_AndNothingWithoutACatalog
    let mut env = MainEnv::new();
    env.vm.show_benchmark();
    assert_eq!(lock(&env.dialog.benchmark_folders).clone(), vec![None]);

    let folder = env.folder("bench_catalog_only");
    let image = env.add_image(&folder, "x.png");
    let entry = pr_core::catalog::types::CatalogEntry::new(image).unwrap_or_else(|error| panic!("{error:?}"));
    env.vm
        .catalog_mut_for_tests()
        .reset(vec![entry], pr_core::model::RawPairMode::Separate, None);

    env.vm.show_benchmark();

    assert_eq!(
        lock(&env.dialog.benchmark_folders).last().cloned(),
        Some(Some(PathBuf::from(folder)))
    );
}

#[test]
fn show_skipped_files_shows_the_skipped_entries_through_the_dialog_service() {
    // C#: MainViewModelNavigationTests.ShowSkippedFiles_ShowsTheSkippedEntriesThroughTheDialogService
    let mut env = MainEnv::new();
    // The sink reports skipped entries when a folder load found unreadable files: `replay_sink_calls_for_tests` is the C# direct call
    // `((IFolderLoadSink)vm).OnFilesSkipped(..)`.
    let entry = pr_core::abstractions::SkippedEntry {
        path: env.photos.join("x.jpg"),
        reason: "locked".to_owned(),
        kind: pr_core::abstractions::SkippedKind::FileUnreadable,
    };
    let shown = entry.clone();
    env.vm.replay_sink_calls_for_tests(|sink| {
        use pr_app::coordinators::traits::IFolderLoadSink;
        sink.on_files_skipped("folder", std::slice::from_ref(&shown));
    });

    env.vm.show_skipped_files();

    assert_eq!(lock(&env.dialog.skipped_shown).clone(), vec![vec![entry]]);
}

#[test]
fn show_recovery_and_diagnostics_go_through_the_dialog_service() {
    // New test (C# ShowRecovery / ShowDiagnostics)
    let env = MainEnv::new();
    env.vm.show_recovery();
    env.vm.show_diagnostics();
    assert_eq!(env.dialog.recovery_shown.load(Ordering::SeqCst), 1);
    assert_eq!(env.dialog.diagnostics_shown.load(Ordering::SeqCst), 1);
}

// ---- duplicates and cache ---------------------------------------------------------------------------------------------------

fn duplicate_album(env: &mut MainEnv, name: &str, files: &[&str]) -> Vec<String> {
    let (_, paths) = album(env, name, files);
    paths
}

#[test]
fn remove_duplicates_recycles_the_numbered_copy_and_reloads() {
    // C#: AdvancedTests.RemoveDuplicatesAsync_NumberedDuplicates_RecyclesNumberedAndReloads
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_numbered", &["photo.png", "photo (1).png"]);
    assert_eq!(env.vm.total_files(), 2);

    env.vm.remove_duplicates(true);
    env.settle();

    assert!(env.bin.recycled().contains(&paths[1]));
    assert!(!env.bin.recycled().contains(&paths[0]));
    assert!(!exists(&paths[1]));
    assert!(exists(&paths[0]));
    assert_eq!(env.vm.total_files(), 1);
    let fixture = FormatFixture::new();
    assert_eq!(
        env.vm.status_text(),
        status::batch_done(1, 0, &fixture.ctx()),
        "set after the reload, which clears the status line"
    );
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn remove_duplicates_holds_the_gate_while_the_review_is_open() {
    // C#: AdvancedTests.RemoveDuplicatesAsync_HoldsFileActionGate_WhileReviewIsOpen (R2-F-20; the hook cannot call the view model, so it
    // asks the gate itself: a second cleanup / a recycle during the review hit the same gate and are refused)
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_gate", &["photo.png", "photo (1).png"]);
    let gate = env.gate.clone();
    let observed: Arc<Mutex<Vec<(bool, bool, QueuedOutcome)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&observed);
    *lock(&env.dialog.on_batch_review) = Some(Box::new(move || {
        lock(&seen).push((gate.is_held(), gate.try_enter(), gate.run_queued(|_turn| {})));
    }));

    env.vm.remove_duplicates(true);
    env.settle();

    assert_eq!(lock(&observed).clone(), vec![(true, false, QueuedOutcome::Rejected)]);
    assert_eq!(lock(&env.dialog.batch_reviews).len(), 1);
    assert_eq!(
        env.bin.recycled(),
        vec![paths[1].clone()],
        "only the reviewed duplicate, the current image was not recycled"
    );
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn defer_close_defers_only_while_the_gate_is_held_and_keeps_the_batch() {
    // C#: AdvancedTests.DeferCloseForFileAction_DefersOnlyWhileGateHeld_AndKeepsTheBatch
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_close", &["photo.png", "photo (1).png"]);
    assert!(
        !env.vm.defer_close_for_file_action(),
        "idle: the window may close at once"
    );

    env.vm.remove_duplicates(true);
    // Hashing and the review are over; the recycle of the batch is in flight.
    env.settle_holding("the batch to be in flight", &["FileAction"], |env| {
        env.runner.count_of("FileAction") > 0
    });
    assert!(env.vm.defer_close_for_file_action(), "a batch holds the gate");
    assert!(
        !env.vm.cancel_duplicate_check(),
        "hashing is over, so the close request does not abort the batch"
    );

    env.settle();
    assert_eq!(env.bin.recycled(), vec![paths[1].clone()]);
    assert!(!env.vm.defer_close_for_file_action());
}

#[test]
fn remove_duplicates_when_the_folder_switched_during_the_review_recycles_nothing() {
    // C#: AdvancedTests.RemoveDuplicatesAsync_FolderSwitchDuringReview_RecyclesNothing (R7-4)
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_switch", &["photo.png", "photo (1).png"]);
    let clock = Arc::clone(&env.clock);
    *lock(&env.dialog.on_batch_review) = Some(Box::new(move || {
        clock.next_folder(); // a forwarded open ran in the nested loop
    }));

    env.vm.remove_duplicates(true);
    env.settle();

    assert!(env.bin.recycled().is_empty());
    assert!(exists(&paths[1]));
}

#[test]
fn remove_duplicates_with_the_original_option_recycles_the_original_and_reloads() {
    // C#: AdvancedTests.RemoveDuplicatesAsync_OriginalDuplicates_RecyclesOriginalAndReloads
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_orig", &["photo.png", "photo (1).png"]);

    env.vm.remove_duplicates(false);
    env.settle();

    assert!(env.bin.recycled().contains(&paths[0]));
    assert!(!env.bin.recycled().contains(&paths[1]));
    assert!(!exists(&paths[0]));
    assert!(exists(&paths[1]));
    assert_eq!(env.vm.total_files(), 1);
    let fixture = FormatFixture::new();
    assert_eq!(env.vm.status_text(), status::batch_done(1, 0, &fixture.ctx()));
}

#[test]
fn remove_duplicates_when_there_are_none_sets_the_status_and_does_not_open_the_dialog() {
    // C#: AdvancedTests.RemoveDuplicatesAsync_WhenNoDuplicates_SetsStatusAndDoesNotOpenDialog
    let mut env = MainEnv::new();
    env.hasher.set("img2.png", "OTHER");
    duplicate_album(&mut env, "dup_none", &["img1.png", "img2.png"]);

    env.vm.remove_duplicates(true);
    env.settle();

    assert!(lock(&env.dialog.batch_reviews).is_empty());
    assert_eq!(env.vm.status_text(), env.text("status.noDuplicatesFound"));
    assert_eq!(env.vm.total_files(), 2);
}

#[test]
fn remove_duplicates_when_the_batch_review_is_declined_cancels_the_batch() {
    // C#: AdvancedTests.RemoveDuplicatesAsync_WhenBatchReviewDeclined_CancelsBatch
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_declined", &["pic.png", "pic (1).png"]);
    env.dialog.batch_review_answer.store(false, Ordering::SeqCst);

    env.vm.remove_duplicates(true);
    env.settle();

    assert_eq!(env.vm.status_text(), env.text("status.batchCanceled"));
    assert!(exists(&paths[0]) && exists(&paths[1]));
    assert!(env.bin.recycled().is_empty());
}

#[test]
fn remove_duplicates_on_an_empty_catalog_does_nothing() {
    // C#: DuplicateBatch.RemoveDuplicatesAsync_EmptyCatalog_DoesNothing
    let mut env = MainEnv::new();

    env.vm.remove_duplicates(true);
    env.settle();

    assert!(lock(&env.dialog.batch_reviews).is_empty());
    assert!(env.bin.recycled().is_empty());
    assert_eq!(env.vm.total_files(), 0);
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn remove_duplicates_with_a_single_image_reports_no_duplicates() {
    // C#: DuplicateBatch.RemoveDuplicatesAsync_SingleImage_ReportsNoDuplicates
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_single", &["only.png"]);

    env.vm.remove_duplicates(true);
    env.settle();

    assert!(lock(&env.dialog.batch_reviews).is_empty());
    assert_eq!(env.vm.status_text(), env.text("status.noDuplicatesFound"));
    assert!(exists(&paths[0]));
    assert_eq!(env.vm.total_files(), 1);
}

#[test]
fn remove_duplicates_recycles_every_numbered_copy_of_many_groups() {
    // C#: DuplicateBatch.RemoveDuplicatesAsync_ManyDuplicates_RecyclesEveryNumberedCopy
    let mut env = MainEnv::new();
    let mut names = Vec::new();
    let mut copies = Vec::new();
    for group in 0..4 {
        names.push(format!("g{group}.png"));
        env.hasher.set(&format!("g{group}.png"), &format!("HASH{group}"));
        for copy in 1..=5 {
            let name = format!("g{group} ({copy}).png");
            env.hasher.set(&name, &format!("HASH{group}"));
            copies.push(name.clone());
            names.push(name);
        }
    }
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let (folder, _) = album(&mut env, "dup_many", &name_refs);
    assert_eq!(env.vm.total_files(), 24);

    env.vm.remove_duplicates(true);
    env.settle();

    assert_eq!(env.bin.recycled().len(), copies.len());
    for copy in &copies {
        assert!(!Path::new(&folder).join(copy).exists());
    }
    assert_eq!(env.vm.total_files(), 4);
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn remove_duplicates_with_one_failing_recycle_reports_it_and_continues() {
    // C#: DuplicateBatch.RemoveDuplicatesAsync_OneRecycleFails_ReportsItAndContinues
    let mut env = MainEnv::new();
    let (folder, paths) = env.images(
        "dup_partial",
        &["photo.png", "photo (1).png", "photo (2).png", "photo (3).png"],
    );
    env.open(&folder);
    // The bin refuses "photo (2)" only (a locked file).
    let locked = paths[2].clone();
    env.bin.fail_for(&locked);

    env.vm.remove_duplicates(true);
    env.settle();

    let mut recycled = env.bin.recycled();
    recycled.sort_by_key(|path| path.to_lowercase());
    assert_eq!(recycled, vec![paths[1].clone(), paths[3].clone()]);
    assert!(exists(&locked));
    let errors = error_dialogs(&env);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].1.contains("photo (2).png"));
    assert!(!errors[0].1.contains("photo (1).png"));
    assert_eq!(env.vm.total_files(), 2, "the original and the locked copy");
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn remove_duplicates_with_every_recycle_failing_reports_every_file_and_keeps_the_catalog() {
    // C#: DuplicateBatch.RemoveDuplicatesAsync_AllRecyclesFail_ReportsAllAndKeepsCatalog
    let mut env = MainEnv::new();
    let paths = duplicate_album(
        &mut env,
        "dup_allfail",
        &["photo.png", "photo (1).png", "photo (2).png"],
    );
    env.bin.fail_for(&paths[1]);
    env.bin.fail_for(&paths[2]);

    env.vm.remove_duplicates(true);
    env.settle();

    assert!(env.bin.recycled().is_empty());
    let errors = error_dialogs(&env);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].1.contains("photo (1).png") && errors[0].1.contains("photo (2).png"));
    let fixture = FormatFixture::new();
    assert_eq!(env.vm.status_text(), status::batch_done(0, 2, &fixture.ctx()));
    assert_eq!(env.vm.total_files(), 3);
}

#[test]
fn remove_duplicates_with_a_file_gone_before_hashing_does_not_crash_or_strand_the_gate() {
    // C#: DuplicateBatch.RemoveDuplicatesAsync_FileGoneBeforeHashing_DoesNotCrashOrStrandTheGate
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_gone", &["photo.png", "photo (1).png", "photo (2).png"]);
    std::fs::remove_file(&paths[2]).unwrap_or_else(|error| panic!("delete: {error}")); // removed by another program after the scan

    env.vm.remove_duplicates(true);
    env.settle();

    assert!(!env.vm.is_file_action_in_progress());
    assert!(!env.bin.recycled().contains(&paths[2]));
    assert!(!env.vm.status_text().is_empty());
}

#[test]
fn cancel_duplicate_check_while_hashing_recycles_nothing() {
    // C#: DuplicateBatch.CancelDuplicateCheck_WhileHashing_RecyclesNothing (Q-R25)
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_cancel_vm", &["photo.png", "photo (1).png"]);
    assert!(
        !env.vm.cancel_duplicate_check(),
        "idle: Esc keeps its normal meaning (exit fullscreen / close)"
    );

    env.vm.remove_duplicates(true);
    assert_eq!(env.runner.count_of("Hashing"), 1, "the check is hashing");
    assert!(env.vm.cancel_duplicate_check(), "Esc is consumed while a check hashes");
    env.settle();

    assert!(env.bin.recycled().is_empty());
    assert!(lock(&env.dialog.batch_reviews).is_empty());
    assert_eq!(env.vm.status_text(), env.text("status.duplicateCheckCanceled"));
    assert!(exists(&paths[1]));
    assert!(!env.vm.is_file_action_in_progress());
    assert!(!env.vm.cancel_duplicate_check());
}

#[test]
fn clear_cache_when_declined_does_not_clear() {
    // C#: AdvancedTests.ClearCacheAsync_WhenDeclined_DoesNotClear
    let mut env = MainEnv::new();
    env.dialog.confirm_answer.store(false, Ordering::SeqCst);

    env.vm.clear_cache();
    env.settle();

    assert_eq!(env.preload.count_of("cancel"), 0);
    assert_eq!(env.preload.count_of("clear"), 0);
}

#[test]
fn clear_cache_when_confirmed_clears_all_caches() {
    // C#: AdvancedTests.ClearCacheAsync_WhenConfirmed_ClearsAllCaches
    let mut env = MainEnv::new();
    env.dialog.confirm_answer.store(true, Ordering::SeqCst);

    env.vm.clear_cache();
    env.settle();

    assert!(env.preload.count_of("cancel") >= 1);
    assert!(env.preload.count_of("clear") >= 1);
    assert_eq!(env.vm.status_text(), env.text("status.cacheCleared"));
}

#[test]
fn clear_cache_is_not_gated_and_a_duplicate_check_started_meanwhile_is_refused_without_holding_the_gate() {
    // New test (C# `ClearCacheAsync` is not under the gate; the controller refuses a second run while the disk clear is in flight, and the
    // view model must give the gate turn back at once)
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_and_cache", &["photo.png", "photo (1).png"]);

    env.vm.clear_cache(); // the disk clear is in flight (not gated)
    assert!(
        !env.vm.is_file_action_in_progress(),
        "clearing the cache does not hold the gate"
    );
    env.vm.remove_duplicates(true); // refused by the controller while the clear runs
    assert!(
        !env.vm.is_file_action_in_progress(),
        "a refused check must not keep the gate"
    );
    env.settle();

    assert_eq!(env.vm.status_text(), env.text("status.cacheCleared"));
    assert_eq!(env.runner.count_of("Hashing"), 0);
    assert!(exists(&paths[1]));
}

#[test]
fn defer_close_cancels_a_duplicate_check_that_is_still_hashing() {
    // C#: DeferCloseForFileAction (R7-7): a check that is only hashing is read-only, so the close cancels it and the wait ends quickly
    let mut env = MainEnv::new();
    let paths = duplicate_album(&mut env, "dup_close_hashing", &["photo.png", "photo (1).png"]);

    env.vm.remove_duplicates(true);
    assert!(env.vm.defer_close_for_file_action());
    env.settle();

    assert!(env.bin.recycled().is_empty());
    assert_eq!(env.vm.status_text(), env.text("status.duplicateCheckCanceled"));
    assert!(exists(&paths[1]));
    assert!(!env.vm.is_file_action_in_progress());
}

#[test]
fn a_dialog_start_folder_stays_empty_for_a_session_with_an_empty_folder() {
    // New test (C# `_currentSession?.Folder ?? ...` tests for null only: an EMPTY session folder does not fall back to the catalog)
    let paths = Arc::new(Mutex::new(None));
    let mut env = MainEnv::with_options(catalog_only_options("empty_session", Arc::clone(&paths)));
    env.vm.replay_sink_calls_for_tests(|sink| {
        use pr_app::coordinators::traits::IFolderLoadSink;
        sink.on_catalog_ready("", 1, &pr_core::session::store::SessionState::new(""));
    });

    env.vm.pick_and_open_folder();
    env.vm.show_benchmark();

    assert_eq!(lock(&env.dialog.pick_initials).clone(), vec![Some(PathBuf::from(""))]);
    assert_eq!(
        lock(&env.dialog.benchmark_folders).clone(),
        vec![Some(PathBuf::from(""))]
    );
}

#[test]
fn undo_reopen_puts_back_the_note_of_a_capture_that_was_only_partly_restorable() {
    // C#: UndoCoreAsync (lines 659 to 661): the reload wrote its own status, the undo's note about a partly restorable capture comes back
    let mut env = MainEnv::with_options(EnvOptions {
        settings: Box::new(|settings| {
            settings.raw_support_enabled = true;
            settings.raw_pair_mode = pr_core::model::RawPairMode::PreferJpeg;
            settings.allow_permanent_delete_without_recycle_bin = true;
        }),
        ..EnvOptions::default()
    });
    let (_, files) = album(&mut env, "undo_note_a", &["a.jpg", "a.cr2", "b.jpg"]);
    let (folder_b, _) = env.images("undo_note_b", &["x.jpg"]);
    env.bin.no_bin_for(&files[1]); // the RAW member lies on a drive without a Recycle Bin: it is deleted permanently
    env.vm.recycle();
    env.settle();
    assert!(
        !exists(&files[1]),
        "the RAW member was deleted permanently: {:?}",
        env.vm.status_text()
    );
    env.open(&folder_b);

    env.vm.undo();
    env.settle();

    assert!(exists(&files[0]), "the JPEG member was restored");
    assert_eq!(
        env.vm.session().map(|session| session.folder.clone()),
        Some(Path::new(&files[0]).parent().unwrap().to_string_lossy().into_owned())
    );
    assert_eq!(
        env.vm.status_text(),
        en_with(
            "core.undo.groupPartiallyRestored",
            &[
                LocArg::new("restored", 1_i64),
                LocArg::new("total", 2_i64),
                LocArg::new("names", "a.cr2")
            ]
        )
    );
}
