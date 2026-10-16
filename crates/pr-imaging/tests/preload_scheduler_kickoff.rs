//! Port of `tests/PhotoReview.Imaging.Tests/PreloadKickOffCallerTests.cs` (C# baseline d92d6aae, 153 lines), Q-R26: with the
//! whole folder cached, a navigation's preload kick examines only the window on the caller's (UI) thread; the rest of the
//! folder is checked on the run thread.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Test notes: "examined on the caller" is read from the fake's per-path key thread (`key_thread`), recorded by every
//! `get_current_cache_key(entry)` call; the window is examined once per path, so the count is exact.

mod common;

use std::thread;

use common::preload_fakes::*;
use pr_imaging::preload::target::PreloadTarget;
use pr_imaging::preload_order::{BACKWARD_LOOKAHEAD, FORWARD_LOOKAHEAD, PreloadWindow};

const IMAGE_COUNT: usize = 500;

/// Every image cached, so a pass examines the whole folder without decoding anything.
fn all_cached(
    fs: &std::sync::Arc<pr_test_support::FakeFileSystem>,
) -> (
    std::sync::Arc<FakeTarget>,
    std::sync::Arc<[pr_core::catalog::types::CatalogEntry]>,
) {
    let entries = entries(fs, IMAGE_COUNT, "q-r26-kick");
    let target = FakeTarget::new(fs);
    for entry in entries.iter() {
        target.mark_cached(entry.path());
    }
    (target, entries)
}

/// Number of examined paths whose key was built on `caller`.
fn examined_on(
    target: &FakeTarget,
    entries: &[pr_core::catalog::types::CatalogEntry],
    caller: thread::ThreadId,
) -> usize {
    entries
        .iter()
        .filter(|e| target.key_thread(e.path()) == Some(caller))
        .count()
}

#[test]
fn whole_folder_cached_caller_thread_examines_only_the_window() {
    // C#: PreloadKickOffCallerTests.WholeFolderCached_CallerThreadExaminesOnlyTheWindow
    let fs = fake_fs();
    let (target, entries) = all_cached(&fs);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(4, PreloadWindow::DEFAULT).full_folder_threshold(1 << 40),
    );

    let caller = thread::current().id();
    let lifetime = scheduler.preload_around(IMAGE_COUNT as i32 / 2);
    let on_caller = examined_on(&target, &entries, caller);
    assert!(finished(&lifetime));

    // The window plus the center image (the decode box is read from its key).
    let window = (FORWARD_LOOKAHEAD + BACKWARD_LOOKAHEAD + 1) as usize;
    assert!((1..=window).contains(&on_caller), "examined on the caller: {on_caller}");
    let examined = entries.iter().filter(|e| target.key_thread(e.path()).is_some()).count();
    assert_eq!(
        IMAGE_COUNT, examined,
        "the far images are still checked, off the caller"
    );
}

#[test]
fn whole_folder_cached_configured_window_caller_thread_examines_only_that_window() {
    // C#: PreloadKickOffCallerTests.WholeFolderCached_ConfiguredWindow_CallerThreadExaminesOnlyThatWindow
    // The caller-thread cap (Q-R26) must follow the configured window too, not the hard-coded default constants.
    let fs = fake_fs();
    let (target, entries) = all_cached(&fs);
    let window = PreloadWindow::new(3, 1);
    let scheduler = create(&target, &entries, Setup::new(4, window).full_folder_threshold(1 << 40));

    let caller = thread::current().id();
    let lifetime = scheduler.preload_around(IMAGE_COUNT as i32 / 2);
    let on_caller = examined_on(&target, &entries, caller);
    assert!(finished(&lifetime));

    assert!(
        (1..=window.image_count() as usize).contains(&on_caller),
        "examined on the caller: {on_caller}"
    );
    let examined = entries.iter().filter(|e| target.key_thread(e.path()).is_some()).count();
    assert_eq!(
        IMAGE_COUNT, examined,
        "the far images are still checked, off the caller"
    );
}

#[test]
fn configured_window_preloads_exactly_forward_plus_backward_neighbours() {
    // C#: PreloadKickOffCallerTests.ConfiguredWindow_PreloadsExactlyForwardPlusBackwardNeighbours
    // A configured window of (3,1) preloads exactly the 3 forward + 1 backward neighbours of centre 25.
    let fs = fake_fs();
    let entries = entries(&fs, 50, "preload-window-test");
    let target = FakeTarget::new(&fs);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(4, PreloadWindow::new(3, 1)).full_folder_threshold(0),
    );

    assert!(finished(&scheduler.preload_around(25)));

    let cached: Vec<usize> = (0..entries.len())
        .filter(|&i| target.is_cached(entries[i].path()))
        .collect();
    assert_eq!(4, target.cache_count());
    assert_eq!(vec![24, 26, 27, 28], cached);
}
