//! The `PreloadItem` perf event of C# `PreloadOneAsync` (baseline d92d6aae, `Preload/PreloadScheduler.cs` l. 775-890):
//! slot number (`_workerCount - 1 - _preloadSlots.CurrentCount` at the take), queue wait (dispatch up to the slot, read
//! BEFORE the decode), kind (`decoded` / `hit` / `skipped` / `busy`) and the item stopwatch; plus the perf navigation
//! context (-1) on the decode thread (contract W8).
//!
//! The perf listener list is process-wide: this file has its own test binary and its tests run one at a time.

mod common;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use common::preload_fakes::*;
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::diagnostics::perf::{self, PerfEvent, PerfListener, PerfValue, PhotoReviewPerf};
use pr_imaging::preload_order::PreloadWindow;

/// Serializes the tests of this binary (one global perf listener at a time).
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One `PreloadItem` event.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    slot: i64,
    path_id: String,
    queue_wait_ms: f64,
    kind: String,
    ms: f64,
}

#[derive(Default)]
struct Recorder {
    items: Mutex<Vec<Item>>,
}

impl PerfListener for Recorder {
    fn on_event_written(&self, event: &PerfEvent) {
        if event.name != "PreloadItem" {
            return;
        }
        if let [
            PerfValue::Int(slot),
            PerfValue::Str(path_id),
            PerfValue::Double(queue_wait_ms),
            PerfValue::Str(kind),
            PerfValue::Double(ms),
        ] = event.payload.as_slice()
        {
            lock(&self.items).push(Item {
                slot: *slot,
                path_id: path_id.clone(),
                queue_wait_ms: *queue_wait_ms,
                kind: kind.clone(),
                ms: *ms,
            });
        }
    }
}

impl Recorder {
    fn item(&self, path: &str) -> Option<Item> {
        let id = perf::path_id(path);
        lock(&self.items).iter().find(|item| item.path_id == id).cloned()
    }
}

/// Busy-waits (no sleep) until `duration` passed: a decode that takes a known minimum time.
fn spin(duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {
        std::hint::spin_loop();
    }
}

#[test]
fn queue_wait_excludes_the_decode_time_and_the_stopwatch_covers_it() {
    // C#: `queueWaitMs = PhotoReviewPerf.Ms(perfEnqueue)` right after WaitAsync; the stopwatch runs over PreloadAsync.
    let _serial = serial();
    let recorder = Arc::new(Recorder::default());
    let _subscription = PhotoReviewPerf::log().subscribe(Arc::clone(&recorder) as Arc<dyn PerfListener>);
    let fs = fake_fs();
    let entries = entries(&fs, 2, "perf-wait");
    let target = FakeTarget::new(&fs);
    target.set_on_preload(|_path, _token| {
        spin(Duration::from_millis(60));
        Ok(())
    });
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(1, 0)));
    assert!(finished(&scheduler.preload_around(0)));
    let item = recorder.item(entries[1].path()).expect("a PreloadItem for img-1");
    assert!(item.queue_wait_ms < 30.0, "a free slot: no queue wait, saw {item:?}");
    assert!(item.ms >= 60.0, "the stopwatch covers the decode, saw {item:?}");
    assert!(
        item.ms < 900.0,
        "milliseconds, not seconds plus something, saw {item:?}"
    );
    assert_eq!(item.slot, 0);
}

#[test]
fn slot_number_follows_the_csharp_formula() {
    // C#: slot = _workerCount - 1 - CurrentCount at the take. Two workers: img-1 takes 0, img-2 takes 1; img-1 ends and
    // img-3 starts while img-2 still runs: the second slot in use again, so 1 (not the free slot id 0).
    let _serial = serial();
    let recorder = Arc::new(Recorder::default());
    let _subscription = PhotoReviewPerf::log().subscribe(Arc::clone(&recorder) as Arc<dyn PerfListener>);
    let fs = fake_fs();
    let entries = entries(&fs, 4, "perf-slot");
    let target = FakeTarget::new(&fs);
    let first = Gate::new();
    let second = Gate::new();
    let (first_hook, second_hook) = (Arc::clone(&first), Arc::clone(&second));
    let second_path = entries[2].path().to_owned();
    let first_path = entries[1].path().to_owned();
    target.set_on_preload(move |path, token| {
        if path.eq_ignore_ascii_case(&first_path) {
            gated(&first_hook)(path, token)
        } else if path.eq_ignore_ascii_case(&second_path) {
            gated(&second_hook)(path, token)
        } else {
            Ok(())
        }
    });
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(3, 0)));
    let run = scheduler.preload_around(0);
    assert!(target.wait_started(entries[1].path()));
    assert!(target.wait_started(entries[2].path()));
    first.open();
    assert!(target.wait_started(entries[3].path()));
    second.open();
    assert!(finished(&run));
    let slots: Vec<i64> = (1..4)
        .map(|i| recorder.item(entries[i].path()).map_or(-1, |item| item.slot))
        .collect();
    assert_eq!(slots, [0, 1, 1]);
}

#[test]
fn kind_is_decoded_only_when_the_item_read_its_source() {
    // C#: `kind = sourceRead ? "decoded" : isHit ? "hit" : "miss"`, sourceRead = SourceReads grew during the item.
    let _serial = serial();
    let recorder = Arc::new(Recorder::default());
    let _subscription = PhotoReviewPerf::log().subscribe(Arc::clone(&recorder) as Arc<dyn PerfListener>);
    let fs = fake_fs();
    let entries = entries(&fs, 3, "perf-kind");
    let target = FakeTarget::new(&fs);
    let metrics = Arc::new(ReviewMetrics::new());
    let reads = Arc::clone(&metrics);
    let read_path = entries[1].path().to_owned();
    target.set_on_preload(move |path, _token| {
        if path.eq_ignore_ascii_case(&read_path) {
            reads.record_source_read(1, 0, false);
        }
        Ok(())
    });
    // One worker: the two items run one after the other, so a read of one never shows in the other.
    let scheduler = create(
        &target,
        &entries,
        Setup::new(1, PreloadWindow::new(2, 0)).metrics(metrics),
    );
    assert!(finished(&scheduler.preload_around(0)));
    let kinds: Vec<String> = (1..3)
        .map(|i| {
            recorder
                .item(entries[i].path())
                .map_or_else(String::new, |item| item.kind)
        })
        .collect();
    assert_eq!(kinds, ["decoded", "hit"]);
}

#[test]
fn the_decode_runs_with_the_preload_navigation_context() {
    // C# D04: `PhotoReviewPerf.NavContext = -1` for the item's downstream calls while tracing.
    let _serial = serial();
    let recorder = Arc::new(Recorder::default());
    let _subscription = PhotoReviewPerf::log().subscribe(Arc::clone(&recorder) as Arc<dyn PerfListener>);
    let fs = fake_fs();
    let entries = entries(&fs, 2, "perf-nav");
    let target = FakeTarget::new(&fs);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    target.set_on_preload(move |_path, _token| {
        lock(&record).push(PhotoReviewPerf::nav_context());
        Ok(())
    });
    let scheduler = create(&target, &entries, Setup::new(2, PreloadWindow::new(1, 0)));
    PhotoReviewPerf::set_nav_context(42);
    assert!(finished(&scheduler.preload_around(0)));
    assert_eq!(*lock(&seen), [-1]);
    assert_eq!(PhotoReviewPerf::nav_context(), 42, "the caller's context is untouched");
    assert!(recorder.item(entries[1].path()).is_some());
}
