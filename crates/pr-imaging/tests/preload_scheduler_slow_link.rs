//! Port of `tests/PhotoReview.Imaging.Tests/PreloadSlowLinkViewerPriorityTests.cs` (C# baseline d92d6aae, 123 lines): while a
//! viewer decode is in flight, a slow link (decode EWMA above 200 ms) allows one preload decode, a fast link the wider limit.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::preload_fakes::*;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::sync::CancelToken;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload_order::PreloadWindow;

const IMAGE_COUNT: usize = 40;

/// Counts the decodes in flight (C# `GatedTarget._inFlight` / `_maxConcurrent`) while each one waits on `gate`.
fn counting_target(
    fs: &Arc<pr_test_support::FakeFileSystem>,
    gate: &Arc<Gate>,
) -> (Arc<FakeTarget>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let target = FakeTarget::new(fs);
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let gate = Arc::clone(gate);
    let in_flight_hook = Arc::clone(&in_flight);
    let max_hook = Arc::clone(&max_concurrent);
    target.set_on_preload(move |path: &str, token: &CancelToken| -> Result<(), DecodeError> {
        let now = in_flight_hook.fetch_add(1, Ordering::SeqCst) + 1;
        max_hook.fetch_max(now, Ordering::SeqCst);
        let result = gated(&gate)(path, token);
        in_flight_hook.fetch_sub(1, Ordering::SeqCst);
        result
    });
    (target, in_flight, max_concurrent)
}

#[test]
fn slow_link_viewer_decode_in_flight_caps_preload_to_one() {
    // C#: PreloadSlowLinkViewerPriorityTests.SlowLink_ViewerDecodeInFlight_CapsPreloadToOne
    let fs = fake_fs();
    let entries = entries(&fs, IMAGE_COUNT, "img");
    let gate = Gate::new();
    let (target, _in_flight, max_concurrent) = counting_target(&fs, &gate);
    target.set_active_viewer(1);
    // Pushes DecodeMillisecondsEwma past SlowLinkDecodeMsThreshold (200 ms): one sample sets the EWMA to that value.
    let metrics = Arc::new(ReviewMetrics::new());
    metrics.record_source_read(1, 5000, true);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(8, PreloadWindow::DEFAULT)
            .full_folder_threshold(1)
            .metrics(metrics),
    );

    let lifetime = scheduler.preload_around(0);
    assert!(target.wait_started_count(1), "first preload decode to start");
    assert!(
        max_concurrent.load(Ordering::SeqCst) <= 1,
        "expected at most 1 concurrent preload decode while a viewer decode is in flight on a slow link, saw {}",
        max_concurrent.load(Ordering::SeqCst)
    );
    assert!(
        !target.started().is_empty(),
        "expected at least one preload decode to still make progress"
    );
    gate.open();
    assert!(finished(&lifetime));
}

#[test]
fn fast_link_viewer_decode_in_flight_uses_the_wider_limit() {
    // C#: PreloadSlowLinkViewerPriorityTests.FastLink_ViewerDecodeInFlight_UsesTheWiderLimit
    let fs = fake_fs();
    let entries = entries(&fs, IMAGE_COUNT, "img");
    let gate = Gate::new();
    let (target, _in_flight, max_concurrent) = counting_target(&fs, &gate);
    target.set_active_viewer(1);
    // no samples: DecodeMillisecondsEwma stays 0 (not a slow link)
    let scheduler = create(
        &target,
        &entries,
        Setup::new(8, PreloadWindow::DEFAULT).full_folder_threshold(1),
    );

    let lifetime = scheduler.preload_around(0);
    assert!(target.wait_started_count(2), "at least 2 concurrent preload decodes");
    assert!(
        max_concurrent.load(Ordering::SeqCst) >= 2,
        "expected the existing viewer-busy limit to still allow >=2 concurrent preload decodes, saw {}",
        max_concurrent.load(Ordering::SeqCst)
    );
    gate.open();
    assert!(finished(&lifetime));
}
