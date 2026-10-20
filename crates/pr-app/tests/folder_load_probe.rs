//! W8-07b: the AR16 readability probe of `FolderLoadCoordinator` (`skipped.rs`): it runs after the first frame, removes and
//! reports unreadable files once the Explorer order is settled, and a superseded or disposed load's result is dropped.
//! Ports of `FolderLoadCoordinatorTests.Probe.cs` and the probe part of `.SlowStorage.cs` (contract `contracts/W8-07b.md`).
//!
//! The probe job is held simply by not running it: the first frame is observable while the `Probe` lane still has a queued
//! job. No test sleeps.

#[path = "common/folder_load_harness.rs"]
mod harness;

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use harness::*;
use pr_app::coordinators::folder_load::state::{FolderLoadEvent, FolderLoadStep, WorkerLane};
use pr_core::model::RawPairMode;

fn unreadable(env: &Env, path: &str, reason: &str) {
    env.fs.inner.set_unreadable(path, reason);
}

fn removals(env: &Env) -> usize {
    env.sink.removals().len()
}

/// Loads `a.jpg` + `a.cr2` (a capture) and `b.jpg` and runs every job.
fn load_pair_folder(mode: RawPairMode, unreadable_files: &[&str]) -> Env {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    env.add("C:\\photos\\a.jpg", 4);
    env.add("C:\\photos\\a.cr2", 3);
    env.add("C:\\photos\\b.jpg", 5);
    env.set_raw(true, mode);
    for file in unreadable_files {
        unreadable(&env, file, "locked");
    }
    env.open("C:\\photos", None);
    env.pump();
    env
}

// C#: FolderLoadCoordinatorTests.Probe_FirstFrameDoesNotWaitForProbe_ThenUnreadableIsRemovedAndReported
#[test]
fn probe_first_frame_does_not_wait_for_probe_then_unreadable_is_removed_and_reported() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    unreadable(&env, &b, "locked");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);

    env.run(WorkerLane::Scan);
    // The first frame is out while the probe job is still queued (held).
    assert_eq!(env.sink.presented().len(), 1);
    assert_eq!(env.port.pending(WorkerLane::Probe), 1);
    assert_eq!(env.paths(), vec![a.clone(), b.clone(), c.clone()]);
    assert!(env.sink.skipped_calls().is_empty());
    assert_eq!(removals(&env), 0);

    env.run(WorkerLane::Probe);

    assert_eq!(env.paths(), vec![a.clone(), c]);
    assert_eq!(env.current_path(), Some(a));
    let calls = env.sink.skipped_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "C:\\photos");
    assert_eq!(calls[0].1.len(), 1);
    assert_eq!(calls[0].1[0].path.to_string_lossy(), b);
    assert_eq!(calls[0].1[0].reason, "locked");
    assert_eq!(removals(&env), 1);
    assert_eq!(env.sink.presented().len(), 1); // the current image is kept, not re-presented
    assert!(env.sink.failures().is_empty());
    assert!(!env.coordinator.probe_pending());
}

// C#: FolderLoadCoordinatorTests.Probe_SinkThrowsWhileApplyingResult_ProbeTaskDoesNotFault
// D-07a-4: a Rust sink does not throw; a failed probe result is logged and removes nothing.
#[test]
fn probe_failed_result_is_logged_and_removes_nothing() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);
    let load_id = 1; // the first load of a fresh coordinator

    let update = env.deliver(FolderLoadEvent::ProbeFinished {
        load_id,
        outcome: Err(Some("probe bug".to_owned())),
    });

    assert_eq!(update.step, FolderLoadStep::Finished);
    assert!(env.log.contains(LogLevel::Error, "Readability probe failed"));
    assert_eq!(env.paths(), paths.to_vec());
    assert!(env.sink.skipped_calls().is_empty());
    assert_eq!(removals(&env), 0);
    assert!(!env.coordinator.probe_pending());
}

// C#: FolderLoadCoordinatorTests.Probe_UnreadableRepresentativeOfCapture_DegradesToTheReadablePartner
#[test]
fn probe_unreadable_representative_of_capture_degrades_to_the_readable_partner() {
    let env = load_pair_folder(RawPairMode::PreferJpeg, &["C:\\photos\\a.jpg"]);

    assert_eq!(env.paths(), strings(&["C:\\photos\\a.cr2", "C:\\photos\\b.jpg"]));
    assert!(env.catalog.entries_snapshot()[0].capture_group.is_none());
    assert_eq!(env.catalog.current_index(), 0);
    assert_eq!(removals(&env), 1);
    assert_eq!(env.sink.removals(), vec![(strings(&["C:\\photos\\a.jpg"]), true)]);
    let skipped = env.sink.skipped_calls();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].1.len(), 1);
    assert_eq!(skipped[0].1[0].path.to_string_lossy(), "C:\\photos\\a.jpg");
}

// C#: FolderLoadCoordinatorTests.Probe_UnreadablePartnerOfCapture_KeepsTheRepresentativeAndReportsThePartner
#[test]
fn probe_unreadable_partner_of_capture_keeps_the_representative_and_reports_the_partner() {
    let env = load_pair_folder(RawPairMode::PreferJpeg, &["C:\\photos\\a.cr2"]);

    assert_eq!(env.paths(), strings(&["C:\\photos\\a.jpg", "C:\\photos\\b.jpg"]));
    assert_eq!(env.catalog.current_index(), 0);
    assert_eq!(removals(&env), 1);
    let skipped = env.sink.skipped_calls();
    assert_eq!(skipped[0].1.len(), 1);
    assert_eq!(skipped[0].1[0].path.to_string_lossy(), "C:\\photos\\a.cr2");
}

// C#: FolderLoadCoordinatorTests.Probe_BothMembersOfCaptureUnreadable_RemovesEntryAndReportsBoth
#[test]
fn probe_both_members_of_capture_unreadable_removes_entry_and_reports_both() {
    let env = load_pair_folder(RawPairMode::PreferJpeg, &["C:\\photos\\a.jpg", "C:\\photos\\a.cr2"]);

    assert_eq!(env.paths(), strings(&["C:\\photos\\b.jpg"]));
    let skipped = env.sink.skipped_calls();
    assert_eq!(skipped.len(), 1);
    let mut reported: Vec<String> = skipped[0]
        .1
        .iter()
        .map(|s| s.path.to_string_lossy().into_owned())
        .collect();
    reported.sort_by_key(|p| p.to_uppercase());
    assert_eq!(reported, strings(&["C:\\photos\\a.cr2", "C:\\photos\\a.jpg"]));
}

// C#: FolderLoadCoordinatorTests.Probe_NonCurrentRemoved_KeepsCurrentPathAtNewIndex
#[test]
fn probe_non_current_removed_keeps_current_path_at_new_index() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    unreadable(&env, &a, "denied");
    env.explorer.set_order(&[&a, &b, &c]);

    env.open("C:\\photos", Some(&c));
    env.pump();

    assert_eq!(env.paths(), vec![b, c.clone()]);
    assert_eq!(env.current_path(), Some(c));
    assert_eq!(env.catalog.current_index(), 1);
    assert_eq!(env.sink.removals(), vec![(vec![a.clone()], false)]);
}

// C#: FolderLoadCoordinatorTests.Probe_CurrentUnreadable_AdvancesLikeDelete
#[test]
fn probe_current_unreadable_advances_like_delete() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    unreadable(&env, &b, "locked");
    env.explorer.set_order(&[&a, &b, &c]);

    env.open("C:\\photos", Some(&b));
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);
    assert_eq!(env.catalog.current_index(), 1);
    env.run(WorkerLane::Probe);

    // Delete of index 1 in [a, b, c] -> [a, c] at index 1.
    assert_eq!(env.paths(), vec![a, c.clone()]);
    assert_eq!(env.current_path(), Some(c));
    assert_eq!(env.catalog.current_index(), 1);
    assert_eq!(env.sink.removals(), vec![(vec![b.clone()], true)]);
    assert_eq!(env.sink.skipped_calls()[0].1[0].path.to_string_lossy(), b);
}

// C#: FolderLoadCoordinatorTests.Probe_StopForActionWhileProbing_StillRemovesAndReportsOnce
#[test]
fn probe_stop_for_action_while_probing_still_removes_and_reports_once() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    unreadable(&env, &b, "locked");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);

    env.clock.stop_for_action(); // what a file action does: bumps the folder generation, same folder
    env.run(WorkerLane::Probe);

    assert_eq!(env.paths(), vec![a, c]);
    let calls = env.sink.skipped_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1.len(), 1);
    assert_eq!(calls[0].1[0].path.to_string_lossy(), b);
    assert_eq!(removals(&env), 1);
    assert!(env.sink.failures().is_empty());
}

// C#: FolderLoadCoordinatorTests.Probe_StopForActionAndFileAlreadyRemoved_ReportsNothing
#[test]
fn probe_stop_for_action_and_file_already_removed_reports_nothing() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    unreadable(&env, &b, "locked");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);

    env.clock.stop_for_action();
    env.catalog.remove(&b); // the action moved/recycled it first
    env.run(WorkerLane::Probe);

    assert_eq!(env.paths(), vec![a, c]);
    assert!(env.sink.skipped_calls().is_empty());
    assert_eq!(removals(&env), 0);
}

// C#: FolderLoadCoordinatorTests.Probe_DisposedBeforeResult_IsDropped
#[test]
fn probe_disposed_before_result_is_dropped() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");
    unreadable(&env, &paths[1], "locked");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);

    env.coordinator.dispose(); // cancels the load token before the probe job runs
    let late = env.run(WorkerLane::Probe);

    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert_eq!(env.paths(), paths.to_vec());
    assert!(env.sink.skipped_calls().is_empty());
    assert_eq!(removals(&env), 0);
}

// C#: FolderLoadCoordinatorTests.Probe_LateResultAfterFolderSwitch_DoesNotTouchTheNewFolder
#[test]
fn probe_late_result_after_folder_switch_does_not_touch_the_new_folder() {
    let mut env = Env::new();
    let [_, b, _] = env.three_images("C:\\photos");
    env.add_dir("C:\\other");
    env.add("C:\\other\\x.jpg", 1);
    env.add("C:\\other\\y.jpg", 1);
    unreadable(&env, &b, "locked");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);
    let first_probe = env.run_held(WorkerLane::Probe); // the old folder's probe finished, its event is in flight

    env.open("C:\\other", None);
    env.pump();
    let late = env.deliver(first_probe);

    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert_eq!(env.paths(), strings(&["C:\\other\\x.jpg", "C:\\other\\y.jpg"]));
    assert!(env.sink.skipped_calls().is_empty());
    assert_eq!(removals(&env), 0);
}

// C#: FolderLoadCoordinatorTests.Probe_DoneBeforePendingExplorerOrder_IsAppliedAfterTheOrder
#[test]
fn probe_done_before_pending_explorer_order_is_applied_after_the_order() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    unreadable(&env, &a, "locked");
    env.explorer.set_order(&[&c, &b, &a]);
    env.open("C:\\photos", Some(&b));
    env.run(WorkerLane::Scan);
    env.run(WorkerLane::Probe); // all files probed while the order is still pending

    // The catalog keeps the full listing and navigation stays gated.
    assert_eq!(env.catalog.count(), 3);
    assert!(env.coordinator.pending_order());
    assert!(env.sink.skipped_calls().is_empty());
    assert!(env.coordinator.probe_pending());

    env.run(WorkerLane::Explorer);

    assert_eq!(env.sink.orders_applied().len(), 1);
    assert_eq!(env.paths(), vec![c, b.clone()]);
    assert_eq!(env.current_path(), Some(b));
    let calls = env.sink.skipped_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1[0].path.to_string_lossy(), a);
    assert!(!env.coordinator.probe_pending());
}

// C#: FolderLoadCoordinatorTests.Probe_FileDeletedAfterListing_IsNotReportedOrRemoved
#[test]
fn probe_file_deleted_after_listing_is_not_reported_or_removed() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");
    unreadable(&env, &paths[1], "Could not find file");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);

    assert!(env.fs.inner.delete(std::path::Path::new(&paths[1])).is_ok()); // deleted behind the app's back
    env.run(WorkerLane::Probe);

    assert_eq!(env.paths(), paths.to_vec());
    assert!(env.sink.skipped_calls().is_empty());
    assert_eq!(removals(&env), 0);
}

// C#: FolderLoadCoordinatorTests.Probe_AllReadable_ReportsNothing
#[test]
fn probe_all_readable_reports_nothing() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.paths(), paths.to_vec());
    assert_eq!(env.fs.probes().len(), 3);
    assert!(env.sink.skipped_calls().is_empty());
    assert_eq!(removals(&env), 0);
}

// C#: FolderLoadCoordinatorTests.Probe_DoneButNotYetApplied_ThenDisposed_IsDropped
#[test]
fn probe_done_but_not_yet_applied_then_disposed_is_dropped() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");
    unreadable(&env, &paths[1], "locked");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);
    // The probe finished and found the unreadable file, but its event has not reached the UI thread yet.
    let done = env.run_held(WorkerLane::Probe);

    env.coordinator.dispose();
    let late = env.deliver(done);

    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert_eq!(env.paths(), paths.to_vec());
    assert!(env.sink.skipped_calls().is_empty());
    assert_eq!(removals(&env), 0);
}

// C#: FolderLoadCoordinatorTests.Load_PerFileCallsBeforeFirstFrame_DoNotGrowWithFolderSize (probe-count part)
// New test (no C# name): the probe covers every file, and only after the first frame.
#[test]
fn probe_covers_every_file_after_the_first_frame() {
    for count in [20_usize, 500] {
        let mut env = Env::new();
        let folder = format!("C:\\nas\\folder{count}");
        env.add_dir(&folder);
        for i in 0..count {
            env.add(&format!("{folder}\\img{i:05}.jpg"), 1);
        }
        env.open(&folder, None);
        env.run(WorkerLane::Explorer);

        env.run(WorkerLane::Scan);
        assert_eq!(env.sink.presented().len(), 1, "{count}");
        assert_eq!(env.fs.probes().len(), 0, "{count}: no probe before the first frame");
        env.run(WorkerLane::Probe);

        assert_eq!(env.fs.probes().len(), count);
    }
}

// New test
#[test]
fn probe_result_is_in_listing_order_not_completion_order() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    for path in [&a, &b, &c] {
        unreadable(&env, path, "locked");
    }
    // `a` is held until `c` has been probed, so `c` completes first.
    let gate: std::sync::Arc<(Mutex<bool>, Condvar)> = std::sync::Arc::new((Mutex::new(false), Condvar::new()));
    let hook_gate = std::sync::Arc::clone(&gate);
    *env.fs
        .probe_hook
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(std::sync::Arc::new(move |path: &str| {
        let (flag, signal) = &*hook_gate;
        if path.ends_with("c.jpg") {
            *flag.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
            signal.notify_all();
        } else if path.ends_with("a.jpg") {
            let guard = flag.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // Bounded wait on the exact condition (never a fixed delay).
            let _ = signal.wait_timeout_while(guard, Duration::from_secs(30), |done| !*done);
        }
    }));
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);

    env.run(WorkerLane::Probe);

    let skipped = env.sink.skipped_calls();
    assert_eq!(skipped.len(), 1);
    let order: Vec<String> = skipped[0]
        .1
        .iter()
        .map(|s| s.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(order, vec![a, b, c]);
}

// New test
#[test]
fn probe_canceled_by_a_newer_load_returns_canceled_and_applies_nothing() {
    let mut env = Env::new();
    let paths = env.three_images("C:\\photos");
    unreadable(&env, &paths[0], "locked");
    env.open("C:\\photos", None);
    env.run(WorkerLane::Explorer);
    env.run(WorkerLane::Scan);

    env.open("C:\\photos", None); // cancels the first load's token before its probe job runs
    let canceled = env.run_held(WorkerLane::Probe);

    assert!(matches!(
        canceled,
        FolderLoadEvent::ProbeFinished { outcome: Err(None), .. }
    ));
    assert!(
        env.fs.probes().is_empty(),
        "a canceled probe checks the token before every file"
    );
}
