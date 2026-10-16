//! Port of `tests/PhotoReview.Imaging.Tests/PreloadSchedulerReviewFixTests.cs` (C# baseline d92d6aae, 224 lines): the
//! review fixes RV-I12 (exit race), RV-I13 (snapshot change without Cancel), I6 (throwing callbacks), RV-I14 (foreign
//! cancellation) and RV-I15 (box change drops the old warmed keys).
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.

mod common;

use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use common::preload_fakes::*;
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload::scheduler::{
    CallbackError, EntriesSnapshotFn, PreloadScheduler, RunHandle, SnapshotVersionFn,
};
use pr_imaging::preload_order::PreloadWindow;

/// Scheduler slot that a hook can reach through a `Weak` (hooks must not keep the scheduler alive).
type Slot = Arc<OnceLock<PreloadScheduler>>;

fn slot_with(scheduler: PreloadScheduler) -> (Slot, Weak<OnceLock<PreloadScheduler>>) {
    let slot: Slot = Arc::new(OnceLock::new());
    assert!(slot.set(scheduler).is_ok());
    let weak = Arc::downgrade(&slot);
    (slot, weak)
}

#[test]
fn navigation_during_scheduler_exit_is_preloaded() {
    // C#: PreloadSchedulerReviewFixTests.NavigationDuringSchedulerExit_IsPreloaded
    // RV-I12: a navigation landing between the loop's final version check and its completion is still preloaded.
    let fs = fake_fs();
    let entries = entries(&fs, 8, "a");
    let target = FakeTarget::new(&fs);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(1, PreloadWindow::new(1, 0)).full_folder_threshold(-1),
    );
    let (slot, weak) = slot_with(scheduler);
    let after_exit: Arc<Mutex<Option<RunHandle>>> = Arc::new(Mutex::new(None));
    let fired = Arc::new(AtomicUsize::new(0));
    let hook_after_exit = Arc::clone(&after_exit);
    let hook_fired = Arc::clone(&fired);
    slot.get()
        .expect("scheduler set")
        .set_before_scheduler_exit_for_tests(Some(Arc::new(move || {
            if hook_fired.swap(1, Ordering::SeqCst) != 0 {
                return;
            }
            if let Some(scheduler) = weak.upgrade() {
                let scheduler = scheduler.get().expect("scheduler set");
                scheduler.notify_navigation(5);
                *lock(&hook_after_exit) = Some(scheduler.preload_around(5));
            }
        })));

    assert!(finished(&slot.get().expect("scheduler set").preload_around(0)));

    let handle = lock(&after_exit).clone();
    let handle = handle.expect("the exit hook kicked a navigation");
    assert!(finished(&handle));
    let started = target.started();
    assert!(started.iter().any(|p| p == entries[1].path()));
    assert!(
        started.iter().any(|p| p == entries[6].path()),
        "the new center's next image"
    );
}

#[test]
fn entries_changed_without_cancel_next_preload_uses_new_snapshot() {
    // C#: PreloadSchedulerReviewFixTests.EntriesChangedWithoutCancel_NextPreloadUsesNewSnapshot
    // RV-I13: entries that changed without Cancel restart the running lifetime on the new snapshot.
    let fs = fake_fs();
    let old_entries = entries(&fs, 8, "old");
    let new_entries = entries(&fs, 8, "new");
    let current: Arc<Mutex<Arc<[CatalogEntry]>>> = Arc::new(Mutex::new(Arc::clone(&old_entries)));
    let version = Arc::new(AtomicI32::new(1));
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    let old_second = old_entries[1].path().to_owned();
    let gate_for_old = Arc::clone(&gate);
    target.set_on_preload(move |path, token| {
        if path.eq_ignore_ascii_case(&old_second) {
            gated(&gate_for_old)(path, token)
        } else {
            Ok(())
        }
    });
    let snapshot_source = Arc::clone(&current);
    let snapshot: EntriesSnapshotFn =
        Arc::new(move || -> Result<Arc<[CatalogEntry]>, CallbackError> { Ok(Arc::clone(&lock(&snapshot_source))) });
    let version_source = Arc::clone(&version);
    let snapshot_version: SnapshotVersionFn =
        Arc::new(move || -> Result<i32, CallbackError> { Ok(version_source.load(Ordering::SeqCst)) });
    let scheduler = create_with_snapshot(
        &target,
        snapshot,
        Setup::new(1, PreloadWindow::new(1, 0))
            .full_folder_threshold(-1)
            .snapshot_version(snapshot_version),
    );

    let _first = scheduler.preload_around(0);
    assert!(
        target.wait_started(old_entries[1].path()),
        "the first run is live, its worker blocked"
    );
    // e.g. undo re-inserted a file: no Cancel() on that path
    *lock(&current) = Arc::clone(&new_entries);
    version.fetch_add(1, Ordering::SeqCst);
    let run = scheduler.preload_around(0);
    gate.open();
    assert!(finished(&run));
    assert!(target.started().iter().any(|p| p == new_entries[1].path()));
}

#[test]
fn unchanged_snapshot_version_joins_running_lifetime() {
    // C#: PreloadSchedulerReviewFixTests.UnchangedSnapshotVersion_JoinsRunningLifetime
    // RV-I13: an unchanged snapshot version keeps joining the running lifetime (no restart per navigation).
    let fs = fake_fs();
    let entries = entries(&fs, 8, "same");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    target.set_on_preload(gated(&gate));
    let scheduler = create(
        &target,
        &entries,
        Setup::new(1, PreloadWindow::new(1, 0))
            .full_folder_threshold(-1)
            .snapshot_version(Arc::new(|| -> Result<i32, CallbackError> { Ok(7) })),
    );
    let first = scheduler.preload_around(0);
    assert!(target.wait_started(entries[1].path()));
    let second = scheduler.preload_around(0);
    assert!(first.same_run(&second));
    gate.open();
    assert!(finished(&first));
}

#[test]
fn throwing_snapshot_version_does_not_escape_preload_around() {
    // C#: PreloadSchedulerReviewFixTests.ThrowingSnapshotVersion_DoesNotEscapePreloadAround
    // I6: a throwing snapshot-version callback is logged and PreloadAroundAsync returns a completed task instead of throwing.
    let fs = fake_fs();
    let entries = entries(&fs, 4, "ver-throw");
    let target = FakeTarget::new(&fs);
    let log = CaptureLog::new();
    let scheduler = create(
        &target,
        &entries,
        Setup::new(1, PreloadWindow::new(1, 0))
            .full_folder_threshold(-1)
            .log(&log)
            .snapshot_version(Arc::new(|| -> Result<i32, CallbackError> {
                Err(CallbackError::from("catalog replaced"))
            })),
    );
    let run = scheduler.preload_around(0);
    assert!(finished(&run));
    assert_eq!(1, log.errors().len(), "{:?}", log.errors());
}

#[test]
fn throwing_snapshot_entries_does_not_escape_preload_around() {
    // C#: PreloadSchedulerReviewFixTests.ThrowingSnapshotEntries_DoesNotEscapePreloadAround
    // I6: a throwing snapshot-entries callback is logged and PreloadAroundAsync returns a completed task instead of throwing.
    let fs = fake_fs();
    let target = FakeTarget::new(&fs);
    let log = CaptureLog::new();
    let snapshot: EntriesSnapshotFn =
        Arc::new(|| -> Result<Arc<[CatalogEntry]>, CallbackError> { Err(CallbackError::from("catalog replaced")) });
    let scheduler = create_with_snapshot(
        &target,
        snapshot,
        Setup::new(1, PreloadWindow::new(1, 0))
            .full_folder_threshold(-1)
            .log(&log),
    );
    let run = scheduler.preload_around(0);
    assert!(finished(&run));
    assert_eq!(1, log.errors().len(), "{:?}", log.errors());
}

#[test]
fn foreign_cancellation_skips_only_that_item() {
    // C#: PreloadSchedulerReviewFixTests.ForeignCancellation_SkipsOnlyThatItem
    // RV-I14: a foreign cancellation from one item fails only that item; later items still preload.
    let fs = fake_fs();
    let entries = entries(&fs, 8, "oce");
    let target = FakeTarget::new(&fs);
    let foreign = entries[2].path().to_owned();
    target.set_on_preload(move |path, _token: &CancelToken| {
        if path.eq_ignore_ascii_case(&foreign) {
            // the lifetime token is NOT cancelled: this is a foreign cancellation
            Err(DecodeError::Canceled)
        } else {
            Ok(())
        }
    });
    let log = CaptureLog::new();
    let scheduler = create(
        &target,
        &entries,
        Setup::new(1, PreloadWindow::new(7, 0))
            .full_folder_threshold(-1)
            .log(&log),
    );

    assert!(finished(&scheduler.preload_around(0)));

    for entry in entries.iter().skip(3) {
        assert!(target.started_count(entry.path()) >= 1, "{} preloaded", entry.path());
    }
    assert_eq!(1, target.started_count(entries[2].path()), "failed, not retried");
    assert!(!log.errors().iter().any(|m| m.contains("Preload scheduler failed")));
}

#[test]
fn box_change_drops_old_box_keys() {
    // C#: PreloadSchedulerReviewFixTests.BoxChange_DropsOldBoxKeys
    // RV-I15: warmed keys of an old target box are dropped when the box changes; items re-preload at the new box.
    let fs = fake_fs();
    let entries = entries(&fs, 8, "box");
    let target = FakeTarget::new(&fs);
    let log = CaptureLog::new();
    let scheduler = create(
        &target,
        &entries,
        Setup::new(1, PreloadWindow::new(3, 0))
            .full_folder_threshold(-1)
            .log(&log),
    );

    assert!(finished(&scheduler.preload_around(0)));
    assert_eq!(
        3,
        scheduler.preloaded_key_count_for_tests(),
        "errors={:?}",
        log.errors()
    );
    let old_key = target.key(entries[1].path());
    target.set_box(DecodeBox::new(200, 150)); // window resized: previews are cached at a new box
    assert!(finished(&scheduler.preload_around(0)));

    assert_eq!(
        3,
        scheduler.preloaded_key_count_for_tests(),
        "only the new box's keys, not 6"
    );
    assert_eq!(6, target.started().len(), "every item re-preloaded at the new box");
    assert!(!scheduler.try_consume_preloaded_key(&old_key));
    assert!(scheduler.try_consume_preloaded_key(&target.key(entries[1].path())));
}
