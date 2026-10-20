//! W8-07b: `FolderLoadCoordinator` late Explorer order, pending-order gate (INV-9), INV-7, dispose and superseded loads.
//! Ports of the `FolderLoadCoordinatorTests` partial class (contract `contracts/W8-07b.md`). Test kind F.
//!
//! "Snapshot not in yet" is expressed by running the Scan job while the Explorer job is still queued; the late snapshot is
//! then delivered by running the Explorer job. Nothing sleeps: every ordering is an explicit `run`/`deliver` call.

#[path = "common/folder_load_harness.rs"]
mod harness;

use harness::*;
use pr_app::coordinators::folder_load::state::{FolderLoadEvent, FolderLoadStep, LoadPhase, ScanFailure, WorkerLane};
use pr_core::catalog::explorer_types::ExplorerOrderStatus;
use pr_core::model::ImageSortMode;

fn presented_indexes(env: &Env) -> Vec<i32> {
    env.sink.presented().iter().map(|p| p.0).collect()
}

// C#: FolderLoadCoordinatorTests.LoadAsync_WithExplorerOrder_INV7_IgnoredAfterUserInteraction
#[test]
fn late_order_is_ignored_after_user_interaction() {
    let mut env = Env::new();
    let [a, b, _] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&b, &a, "C:\\photos\\c.jpg"]);
    env.open("C:\\photos", None);
    env.run(WorkerLane::Scan);
    assert_eq!(env.sink.catalog_ready(), 1);

    // The user interacts (INV-7) after the catalog was ready, then the snapshot arrives.
    env.clock.next_interaction();
    let update = env.run(WorkerLane::Explorer);

    assert_eq!(update.step, FolderLoadStep::Finished);
    assert!(env.sink.orders_applied().is_empty());
    assert_eq!(env.paths()[0], a);
    assert!(env.phases().contains(&"explorerIgnored".to_owned()));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_WithExplorerOrder_AppliedWhenNoInteraction
#[test]
fn late_order_applies_when_no_interaction() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&c, &b, &a]);
    env.open("C:\\photos", None);
    env.run(WorkerLane::Scan);
    assert_eq!(env.paths(), vec![a.clone(), b.clone(), c.clone()]);

    env.run(WorkerLane::Explorer);

    assert_eq!(env.sink.orders_applied().len(), 1);
    assert_eq!(env.paths(), vec![c, b, a]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DirectFileOpen_PresentsRequestedFileBeforeSnapshot_ThenAppliesOrderKeepingIt
#[test]
fn file_open_presents_before_snapshot_then_keeps_file_on_order() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&c, &b, &a]);
    env.open("C:\\photos", Some(&b));

    env.run(WorkerLane::Scan);
    // INV-9: the opened file is presented while the snapshot is pending, and navigation is gated.
    assert_eq!(presented_indexes(&env), vec![1]);
    assert_eq!(env.current_path(), Some(b.clone()));
    assert!(env.coordinator.pending_order());
    assert_eq!(env.coordinator.phase(), LoadPhase::AwaitingOrder);

    let update = env.run(WorkerLane::Explorer);

    assert_eq!(update.step, FolderLoadStep::Finished);
    assert!(update.order_gate_released);
    assert!(!env.coordinator.pending_order());
    assert_eq!(env.sink.orders_applied(), vec![(3, 1, true)]);
    assert_eq!(env.paths(), vec![c, b.clone(), a]);
    // Same image, new index; nothing re-presented.
    assert_eq!(env.current_path(), Some(b));
    assert_eq!(env.catalog.current_index(), 1);
    assert_eq!(presented_indexes(&env), vec![1]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_DirectFileOpen_UnavailableSnapshot_ReleasesGateAndKeepsFallback
#[test]
fn unavailable_snapshot_releases_gate_and_keeps_fallback() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_status(ExplorerOrderStatus::TimedOut);
    env.open("C:\\photos", Some(&c));
    env.run(WorkerLane::Scan);
    assert!(env.coordinator.pending_order());

    let update = env.run(WorkerLane::Explorer);

    assert!(update.order_gate_released);
    assert!(!env.coordinator.pending_order());
    assert!(env.sink.orders_applied().is_empty());
    // Fallback order is the natural sort; the opened file stays at its own place in it.
    assert_eq!(env.paths(), vec![a, b, c.clone()]);
    assert_eq!(env.current_path(), Some(c));
    assert_eq!(presented_indexes(&env), vec![2]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_SupersedingLoad_ReleasesPendingOrderGate
#[test]
fn superseding_load_releases_pending_order_gate() {
    let mut env = Env::new();
    let [_, b, _] = env.three_images("C:\\photos");
    env.add_dir("C:\\other");
    env.add("C:\\other\\x.jpg", 1);
    env.explorer.set_order(&["C:\\photos\\b.jpg"]);
    env.open("C:\\photos", Some(&b));
    env.run(WorkerLane::Scan);
    assert!(env.coordinator.pending_order());
    let stale_snapshot = env.run_held(WorkerLane::Explorer);

    let update = env.open("C:\\other", None);
    assert!(
        update.order_gate_released,
        "A superseded file-open load must not keep navigation gated."
    );
    assert!(!env.coordinator.pending_order());

    env.pump();
    let late = env.deliver(stale_snapshot);
    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert!(env.sink.orders_applied().is_empty());
}

// C#: FolderLoadCoordinatorTests.LoadAsync_FolderOpen_NeverGatesNavigation_AndReportsCurrentReplaced
#[test]
fn folder_open_never_gates_navigation_and_reports_current_replaced() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&c, &b, &a]);
    env.open("C:\\photos", None);
    env.run(WorkerLane::Scan);
    // INV-9b: a folder open presents the fallback and does not gate navigation.
    assert_eq!(presented_indexes(&env), vec![0]);
    assert!(!env.coordinator.pending_order());

    env.run(WorkerLane::Explorer);

    let applied = env.sink.orders_applied();
    assert_eq!(applied.len(), 1);
    assert!(!applied[0].2, "current_kept must be false: the first image is replaced");
    assert_eq!(presented_indexes(&env), vec![0, 0]);
    assert_eq!(env.current_path(), Some(c));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_NameSort_StillAppliesExplorerOrder
#[test]
fn name_sort_still_applies_explorer_order() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images_scan_order_cab("C:\\photos");
    env.set_sort(ImageSortMode::Name);
    env.explorer.set_order(&[&b, &c, &a]);

    env.open("C:\\photos", None);
    env.pump();

    assert_eq!(env.explorer.queries(), 1);
    assert_eq!(env.sink.orders_applied().len(), 1);
    assert_eq!(env.paths(), vec![b, c, a]);
}

// C#: FolderLoadCoordinatorTests.Dispose_DuringBlockedScan_CancelsLoad_AndNoSinkUpdateFollows
#[test]
fn dispose_during_blocked_scan_cancels_load_and_no_sink_update_follows() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    env.open("C:\\photos", None);
    // The scan has finished its work but its event is still in flight when the window closes.
    let scan = env.run_held(WorkerLane::Scan);

    env.coordinator.dispose();
    let late = env.deliver(scan);

    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert!(env.sink.calls.is_empty());
    assert_eq!(env.catalog.count(), 0);
}

// New test
#[test]
fn dispose_cancels_the_token_of_the_active_scan() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    env.open("C:\\photos", None);

    env.coordinator.dispose();
    let scan = env.run_held(WorkerLane::Scan);

    assert!(matches!(
        scan,
        FolderLoadEvent::ScanFinished {
            outcome: Err(ScanFailure::Canceled),
            ..
        }
    ));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_StaleExplorerSnapshotFromSupersededLoad_IsIgnored
#[test]
fn stale_explorer_snapshot_from_superseded_load_is_ignored() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.add_dir("C:\\other");
    env.add("C:\\other\\x.jpg", 1);
    env.add("C:\\other\\y.jpg", 1);
    env.explorer.set_order(&[&c, &b, &a]);
    env.open("C:\\photos", None);
    env.run(WorkerLane::Scan);
    assert_eq!(presented_indexes(&env), vec![0]);
    let stale = env.run_held(WorkerLane::Explorer);
    env.open("C:\\other", None);
    env.explorer.set_status(ExplorerOrderStatus::NoMatchingWindow);
    env.pump();
    let presented_before = env.sink.presented().len();

    let late = env.deliver(stale);

    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert!(env.sink.orders_applied().is_empty());
    assert_eq!(env.sink.presented().len(), presented_before);
    assert_eq!(env.paths(), strings(&["C:\\other\\x.jpg", "C:\\other\\y.jpg"]));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_FolderGenerationBumpedWhileOrderPending_DoesNotApplyTheOrder
#[test]
fn folder_generation_bumped_while_order_pending_does_not_apply_the_order() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&c, &b, &a]);
    env.open("C:\\photos", None);
    env.run(WorkerLane::Scan);

    env.clock.next_folder();
    let update = env.run(WorkerLane::Explorer);

    assert_eq!(update.step, FolderLoadStep::Finished);
    assert!(env.sink.orders_applied().is_empty());
    assert_eq!(env.paths(), vec![a, b, c]);
}

// C#: FolderLoadCoordinatorTests.LoadAsync_AfterDispose_ThrowsObjectDisposed_AndDoesNotTouchTheSink
#[test]
fn dispose_is_idempotent_and_clears_the_gate() {
    let mut env = Env::new();
    env.three_images("C:\\photos");

    let first = env.coordinator.dispose();
    let second = env.coordinator.dispose(); // idempotent

    assert_eq!(first.step, FolderLoadStep::Finished);
    assert_eq!(second.step, FolderLoadStep::Ignored);
    assert!(env.coordinator.is_disposed());
    assert!(!env.coordinator.pending_order());
    assert!(env.load("C:\\photos", None).is_err());
    assert!(env.sink.calls.is_empty());
}

// C#: FolderLoadCoordinatorTests.Dispose_WhileExplorerOrderPending_ReleasesGate_AndAppliesNothing
#[test]
fn dispose_releases_gate_and_drops_late_events() {
    let mut env = Env::new();
    let [_, b, _] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&b]);
    env.open("C:\\photos", Some(&b));
    env.run(WorkerLane::Scan);
    assert!(env.coordinator.pending_order());

    let update = env.coordinator.dispose();

    assert!(update.order_gate_released);
    assert!(!env.coordinator.pending_order());
    let late = env.run(WorkerLane::Explorer);
    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert!(env.sink.orders_applied().is_empty());
}

// C#: FolderLoadCoordinatorTests.LoadAsync_SupersededLoadSinkThrows_DoesNotEscapeAndNewerLoadIsUnaffected
// D-07a-4: a Rust sink does not throw; a failure of a superseded load is dropped and the newer load is unaffected.
#[test]
fn superseded_load_failure_is_dropped_and_the_newer_load_is_unaffected() {
    let mut env = Env::new();
    env.three_images("C:\\a");
    env.add_dir("C:\\b");
    env.add("C:\\b\\x.jpg", 1);
    *env.fs
        .fail_listing
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(std::io::ErrorKind::PermissionDenied);
    env.open("C:\\a", None);
    let failed_scan = env.run_held(WorkerLane::Scan);
    *env.fs
        .fail_listing
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

    env.open("C:\\b", None);
    env.pump();
    let late = env.deliver(failed_scan);

    assert_eq!(late.step, FolderLoadStep::Ignored);
    assert!(env.sink.failures().is_empty()); // a stale load reports nothing to the user either
    assert_eq!(env.paths(), strings(&["C:\\b\\x.jpg"]));
}

// C#: FolderLoadCoordinatorTests.LoadAsync_SupersededLoadUnwindsLate_KeepsTheNewerProbe
#[test]
fn superseded_load_unwinds_late_keeps_the_newer_probe() {
    let mut env = Env::new();
    env.three_images("C:\\a");
    env.add_dir("C:\\b");
    env.add("C:\\b\\x.jpg", 1);
    env.open("C:\\a", None);
    env.run(WorkerLane::Scan); // A presents, starts its probe and waits for the (held) Explorer snapshot
    let stale_snapshot = env.run_held(WorkerLane::Explorer);
    let stale_probe = env.run_held(WorkerLane::Probe);

    env.open("C:\\b", None);
    env.run(WorkerLane::Scan);
    assert!(env.coordinator.probe_pending());

    // A unwinds now (its late events arrive): the newer load's probe state is untouched.
    assert_eq!(env.deliver(stale_snapshot).step, FolderLoadStep::Ignored);
    assert_eq!(env.deliver(stale_probe).step, FolderLoadStep::Ignored);
    assert!(env.coordinator.probe_pending());
    assert_eq!(env.coordinator.phase(), LoadPhase::AwaitingOrder);

    env.pump();
    assert!(!env.coordinator.probe_pending());
    assert_eq!(env.paths(), strings(&["C:\\b\\x.jpg"]));
}

// C#: FolderLoadCoordinatorTests.PerfMarks_LateSnapshotThatNoLongerMatchesTheCatalog_IsMarkedIgnored
#[test]
fn late_snapshot_that_no_longer_matches_the_catalog_is_marked_ignored() {
    let mut env = Env::new();
    env.add_dir("C:\\photos");
    let (a, b) = ("C:\\photos\\a.jpg", "C:\\photos\\b.jpg");
    env.add(a, 1);
    env.add(b, 2);
    env.explorer.set_order(&[a, b]);
    env.open("C:\\photos", None);
    env.run(WorkerLane::Scan);
    env.catalog.remove(b); // the catalog changed after the scan the snapshot was validated against

    env.run(WorkerLane::Explorer);

    let phases = env.phases();
    assert!(phases.contains(&"explorerIgnored".to_owned()));
    assert!(!phases.contains(&"explorerApplied".to_owned()));
}

// C#: FolderLoadCoordinatorTests.PerfMarks_LateSnapshotThatIsUnavailable_IsMarkedFallback
#[test]
fn late_snapshot_that_is_unavailable_is_marked_fallback() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    env.explorer.set_status(ExplorerOrderStatus::NativeViewUnavailable);
    env.open("C:\\photos", None);
    env.run(WorkerLane::Scan);

    env.run(WorkerLane::Explorer);

    let phases = env.phases();
    assert!(phases.contains(&"explorerFallback".to_owned()));
    assert!(!phases.contains(&"explorerApplied".to_owned()));
}

// New test
#[test]
fn failed_explorer_query_is_reported_like_a_load_failure_and_releases_the_gate() {
    let mut env = Env::new();
    let [_, b, _] = env.three_images("C:\\photos");
    env.explorer
        .panic_on_query
        .store(true, std::sync::atomic::Ordering::SeqCst);
    env.open("C:\\photos", Some(&b));
    env.run(WorkerLane::Scan);
    assert!(env.coordinator.pending_order());

    let update = env.run(WorkerLane::Explorer);

    assert_eq!(update.step, FolderLoadStep::Finished);
    assert!(update.order_gate_released);
    assert_eq!(env.sink.failures(), vec![("C:\\photos".to_owned(), false)]);
    assert_eq!(env.coordinator.phase(), LoadPhase::Idle);
}

// New test
#[test]
fn explorer_event_before_the_scan_is_applied_when_the_scan_arrives() {
    let mut env = Env::new();
    let [a, b, c] = env.three_images("C:\\photos");
    env.explorer.set_order(&[&c, &b, &a]);
    env.open("C:\\photos", None);

    let early = env.run(WorkerLane::Explorer);
    assert_eq!(early.step, FolderLoadStep::Pending);
    assert!(env.sink.calls.is_empty());
    env.run(WorkerLane::Scan);

    assert_eq!(env.sink.orders_applied().len(), 1);
    assert_eq!(env.paths(), vec![c, b, a]);
    assert_eq!(env.coordinator.phase(), LoadPhase::Idle);
}

// New test
#[test]
fn unknown_explorer_event_is_ignored() {
    let mut env = Env::new();
    env.three_images("C:\\photos");
    env.open("C:\\photos", None);

    let update = env.deliver(FolderLoadEvent::ExplorerFinished {
        load_id: 999,
        snapshot: None,
    });

    assert_eq!(update.step, FolderLoadStep::Ignored);
    assert_eq!(env.coordinator.phase(), LoadPhase::Scanning);
}
