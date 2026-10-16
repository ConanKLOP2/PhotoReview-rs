//! Port of `tests/PhotoReview.Imaging.Tests/PreloadRequeueTests.cs` (C# baseline d92d6aae, 109 lines): an evicted preview is
//! requeued within the same lifetime; a failed preload is not retried within it.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.

mod common;

use common::preload_fakes::*;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload_order::{FORWARD_LOOKAHEAD, PreloadWindow};

const IMAGE_COUNT: usize = 40;
/// Last index of the 32-ahead window around 0: held inside `preload` so the lifetime stays open.
const BLOCKED_INDEX: usize = FORWARD_LOOKAHEAD as usize;

#[test]
fn evicted_preview_is_requeued_within_same_lifetime() {
    // C#: PreloadRequeueTests.EvictedPreview_IsRequeued_WithinSameLifetime
    let fs = fake_fs();
    let entries = entries(&fs, IMAGE_COUNT, "img");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    let blocked = entries[BLOCKED_INDEX].path().to_owned();
    let gate_for_blocked = std::sync::Arc::clone(&gate);
    target.set_on_preload(move |path, token| {
        if path == blocked {
            gated_ignoring_cancel(&gate_for_blocked)(path, token)
        } else {
            Ok(())
        }
    });
    let scheduler = create(
        &target,
        &entries,
        Setup::new(4, PreloadWindow::DEFAULT).full_folder_threshold(1),
    );

    let lifetime = scheduler.preload_around(0);
    assert!(target.wait_started(entries[BLOCKED_INDEX].path()));
    assert_eq!(1, target.started_count(entries[1].path()));
    target.evict(entries[1].path()); // e.g. a compare/zoom decode pushed it out of the LRU
    let _ = scheduler.preload_around(0); // navigation: same lifetime, new order pass
    gate.open();
    assert!(finished(&lifetime));
    assert_eq!(2, target.started_count(entries[1].path()));
    assert!(target.is_cached(entries[1].path()));
}

#[test]
fn failed_preload_is_not_retried_within_same_lifetime() {
    // C#: PreloadRequeueTests.FailedPreload_IsNotRetried_WithinSameLifetime
    let fs = fake_fs();
    let entries = entries(&fs, IMAGE_COUNT, "img");
    let target = FakeTarget::new(&fs);
    let gate = Gate::new();
    let blocked = entries[BLOCKED_INDEX].path().to_owned();
    let failing = entries[2].path().to_owned();
    let gate_for_blocked = std::sync::Arc::clone(&gate);
    target.set_on_preload(move |path, token| {
        if path == blocked {
            gated_ignoring_cancel(&gate_for_blocked)(path, token)
        } else if path == failing {
            Err(DecodeError::Io(io_error(std::io::ErrorKind::Other, "unreadable")))
        } else {
            Ok(())
        }
    });
    let scheduler = create(
        &target,
        &entries,
        Setup::new(4, PreloadWindow::DEFAULT).full_folder_threshold(1),
    );

    let lifetime = scheduler.preload_around(0);
    assert!(target.wait_started(entries[BLOCKED_INDEX].path()));
    let _ = scheduler.preload_around(0);
    gate.open();
    assert!(finished(&lifetime));
    assert_eq!(1, target.started_count(entries[2].path()));
}
