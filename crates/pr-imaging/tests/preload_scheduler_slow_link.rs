//! Port of `tests/PhotoReview.Imaging.Tests/PreloadSlowLinkViewerPriorityTests.cs` (C# baseline d92d6aae, 123 lines): while a
//! viewer decode is in flight, a slow link (decode EWMA above 200 ms) allows one preload decode, a fast link the wider limit.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.

mod common;

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use common::preload_fakes::*;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::sync::CancelToken;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload_order::PreloadWindow;

const IMAGE_COUNT: usize = 40;

/// The decodes in flight and the most seen at once (C# `GatedTarget._inFlight` / `_maxConcurrent`), with a condition
/// variable so a test can wait for the maximum itself (not only for `started`, which the fake target records BEFORE the
/// counting hook runs).
#[derive(Default)]
struct Concurrency {
    counts: Mutex<(usize, usize)>,
    changed: Condvar,
}

impl Concurrency {
    fn enter(&self) {
        let mut counts = lock(&self.counts);
        counts.0 += 1;
        counts.1 = counts.1.max(counts.0);
        self.changed.notify_all();
    }

    fn leave(&self) {
        lock(&self.counts).0 -= 1;
        self.changed.notify_all();
    }

    fn max(&self) -> usize {
        lock(&self.counts).1
    }

    /// Waits (guarded) until at least `count` decodes were in flight at once; `true` when they were.
    fn wait_max_at_least(&self, count: usize) -> bool {
        let deadline = Instant::now() + GUARD;
        let mut counts = lock(&self.counts);
        while counts.1 < count {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            counts = self
                .changed
                .wait_timeout(counts, remaining.min(Duration::from_millis(50)))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        true
    }
}

/// Counts the decodes in flight while each one waits on `gate`.
fn counting_target(fs: &Arc<pr_test_support::FakeFileSystem>, gate: &Arc<Gate>) -> (Arc<FakeTarget>, Arc<Concurrency>) {
    let target = FakeTarget::new(fs);
    let concurrency = Arc::new(Concurrency::default());
    let gate = Arc::clone(gate);
    let hook = Arc::clone(&concurrency);
    target.set_on_preload(move |path: &str, token: &CancelToken| -> Result<(), DecodeError> {
        hook.enter();
        let result = gated(&gate)(path, token);
        hook.leave();
        result
    });
    (target, concurrency)
}
#[test]
fn slow_link_viewer_decode_in_flight_caps_preload_to_one() {
    // C#: PreloadSlowLinkViewerPriorityTests.SlowLink_ViewerDecodeInFlight_CapsPreloadToOne
    let fs = fake_fs();
    let entries = entries(&fs, IMAGE_COUNT, "img");
    let gate = Gate::new();
    let (target, concurrency) = counting_target(&fs, &gate);
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
        concurrency.wait_max_at_least(1),
        "the first decode entered the counting hook"
    );
    assert!(
        concurrency.max() <= 1,
        "expected at most 1 concurrent preload decode while a viewer decode is in flight on a slow link, saw {}",
        concurrency.max()
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
    let (target, concurrency) = counting_target(&fs, &gate);
    target.set_active_viewer(1);
    // no samples: DecodeMillisecondsEwma stays 0 (not a slow link)
    let scheduler = create(
        &target,
        &entries,
        Setup::new(8, PreloadWindow::DEFAULT).full_folder_threshold(1),
    );

    let lifetime = scheduler.preload_around(0);
    assert!(target.wait_started_count(2), "at least 2 concurrent preload decodes");
    // DIVERGENCE (test race fix): the fake records `started` before the counting hook runs, so wait for the maximum.
    assert!(
        concurrency.wait_max_at_least(2),
        "expected the existing viewer-busy limit to still allow >=2 concurrent preload decodes, saw {}",
        concurrency.max()
    );
    gate.open();
    assert!(finished(&lifetime));
}
