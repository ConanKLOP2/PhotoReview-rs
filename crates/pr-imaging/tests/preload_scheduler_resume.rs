//! Port of `tests/PhotoReview.Imaging.Tests/PreloadSchedulerMutationTests.Resume.cs` (C# baseline d92d6aae, 236 lines):
//! memory-pause auto-resume. A run that paused for memory headroom is re-armed by a low-frequency re-check once the load
//! is below the pause limit minus `RESUME_LOAD_MARGIN`. The re-check timer is a manual fake (fired by the test, never by
//! time) and the load is a fake probe: no sleeps.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::preload_fakes::*;
use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_imaging::preload::policy::{MAX_RESUME_CHECKS, RESUME_CHECK_BASE_DELAY, RESUME_LOAD_MARGIN, resume_check_delay};
use pr_imaging::preload::scheduler::PreloadScheduler;
use pr_imaging::preload_order::PreloadWindow;

/// PreloadOptions' default MemoryLoadLimit (Create does not override it): pause at >= 0.80, resume only below 0.75.
const PAUSE_LIMIT: f64 = 0.80;

/// An overriding answer of [`LoadProbe`] (C# `Answer`).
type AnswerFn = Arc<dyn Fn(f64) -> bool + Send + Sync>;

/// C# `LoadProbe`: headroom is `load < maximumLoad` unless an `Answer` overrides it; the snapshot reports the load as
/// `(uint)(Load * 100)` percent.
struct LoadProbe {
    load: Mutex<f64>,
    answer: Mutex<Option<AnswerFn>>,
    limits: Mutex<Vec<f64>>,
}

impl LoadProbe {
    fn new(load: f64) -> Arc<Self> {
        Arc::new(Self {
            load: Mutex::new(load),
            answer: Mutex::new(None),
            limits: Mutex::new(Vec::new()),
        })
    }

    fn load(&self) -> f64 {
        *lock(&self.load)
    }

    fn set_load(&self, load: f64) {
        *lock(&self.load) = load;
    }

    fn set_answer(&self, answer: impl Fn(f64) -> bool + Send + Sync + 'static) {
        *lock(&self.answer) = Some(Arc::new(answer));
    }

    fn limits(&self) -> Vec<f64> {
        lock(&self.limits).clone()
    }
}

impl MemoryProbe for LoadProbe {
    fn has_headroom(&self, maximum_load: f64, _reserve_bytes: i64) -> bool {
        lock(&self.limits).push(maximum_load);
        let answer = lock(&self.answer).clone();
        match answer {
            Some(answer) => answer(maximum_load),
            None => self.load() < maximum_load,
        }
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        Some(MemorySnapshot {
            load_percent: (self.load() * 100.0) as u32,
            available_bytes: 16 << 30,
        })
    }
}

/// C# `PausedRunAsync`: a run around image 0 with a 4-image forward window that pauses for memory before starting.
struct Paused {
    scheduler: Arc<PreloadScheduler>,
    target: Arc<FakeTarget>,
    probe: Arc<LoadProbe>,
    time: Arc<ManualTimers>,
}

fn paused_run(whole_folder: bool, cache_bytes: i64) -> Paused {
    let fs = fake_fs();
    let entries = entries(&fs, 10, "img");
    let target = FakeTarget::new(&fs);
    target.set_bytes(cache_bytes);
    let probe = LoadProbe::new(0.95);
    let time = ManualTimers::new();
    let mut setup = Setup::new(2, PreloadWindow::new(4, 0))
        .probe(probe.clone())
        .timers(time.clone());
    if whole_folder {
        setup = setup.whole_folder();
    }
    let scheduler = Arc::new(create(&target, &entries, setup));
    assert!(finished(&scheduler.preload_around(0)));
    assert!(target.started().is_empty());
    Paused {
        scheduler,
        target,
        probe,
        time,
    }
}

/// Due times in whole seconds (C# `Due.TotalSeconds`).
fn due_seconds(time: &ManualTimers) -> Vec<f64> {
    time.dues().iter().map(Duration::as_secs_f64).collect()
}

fn file_name(path: &str) -> String {
    path.rsplit('\\').next().unwrap_or(path).to_owned()
}

#[test]
fn memory_pause_pressure_clears_resumes_exactly_once() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_PressureClears_ResumesExactlyOnce
    let p = paused_run(false, 0);
    assert_eq!(1, p.time.created());
    assert_eq!(RESUME_CHECK_BASE_DELAY, p.time.dues()[0]);
    p.probe.set_load(0.50);
    p.time.fire(0);
    assert!(finished(
        &p.scheduler.resumed_run_for_tests().expect("the re-check resumed a run")
    ));
    assert_eq!(1, p.scheduler.resumes_for_tests());
    assert!(p.target.wait_started_count(4));
    assert_eq!(
        4,
        p.target.started().len(),
        "the whole window around the unchanged centre"
    );
    assert_eq!(1, p.time.created(), "the resumed run did not pause: nothing re-armed");
    p.time.fire(0); // a late duplicate callback of the same re-check
    assert_eq!(1, p.scheduler.resumes_for_tests());
    assert_eq!(4, p.target.started().len());
}

#[test]
fn memory_pause_pressure_persists_bounded_backoff_without_resume() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_PressurePersists_BoundedBackoffWithoutResume
    let p = paused_run(false, 0);
    for _ in 0..50 {
        match p.time.pending_index() {
            Some(index) => p.time.fire(index),
            None => break,
        }
    }
    assert_eq!(vec![2.0, 4.0, 8.0, 16.0, 30.0, 30.0, 30.0, 30.0], due_seconds(&p.time));
    assert_eq!(MAX_RESUME_CHECKS as usize, p.time.created());
    assert!(p.time.pending_index().is_none());
    assert_eq!(0, p.scheduler.resumes_for_tests());
    assert!(p.target.started().is_empty());
    // A real navigation still kicks preload as before, and gives the re-checks a fresh budget.
    assert!(finished(&p.scheduler.preload_around(5)));
    assert_eq!(MAX_RESUME_CHECKS as usize + 1, p.time.created());
    assert_eq!(RESUME_CHECK_BASE_DELAY, *p.time.dues().last().expect("a timer"));
}

#[test]
fn memory_pause_load_inside_margin_does_not_resume() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_LoadInsideMargin_DoesNotResume (hysteresis)
    let p = paused_run(false, 0);
    // 0.775: preload would not pause here, but must not resume either
    p.probe.set_load(PAUSE_LIMIT - RESUME_LOAD_MARGIN / 2.0);
    p.time.fire(p.time.pending_index().expect("a pending re-check"));
    assert_eq!(0, p.scheduler.resumes_for_tests());
    assert!(p.target.started().is_empty());
    assert_eq!(2, p.time.created(), "re-armed");
    assert!(
        p.probe
            .limits()
            .iter()
            .any(|limit| (limit - (PAUSE_LIMIT - RESUME_LOAD_MARGIN)).abs() < 1e-9),
        "the re-check asked for the resume limit"
    );
    p.probe.set_load(0.70);
    p.time.fire(p.time.pending_index().expect("a pending re-check"));
    assert!(finished(
        &p.scheduler.resumed_run_for_tests().expect("the re-check resumed a run")
    ));
    assert_eq!(1, p.scheduler.resumes_for_tests());
    assert_eq!(4, p.target.started().len());
}

#[test]
fn memory_pause_cancel_or_dispose_cancels_the_recheck() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_CancelOrDispose_CancelsTheRecheck (theory: cancel, dispose)
    for cancel in [true, false] {
        let p = paused_run(false, 0);
        assert_eq!(1, p.time.created());
        if cancel {
            p.scheduler.cancel();
        } else {
            p.scheduler.dispose();
        }
        assert!(p.time.is_disposed(0));
        p.probe.set_load(0.50);
        p.time.fire(0); // even one already firing does nothing
        assert_eq!(0, p.scheduler.resumes_for_tests());
        assert!(p.target.started().is_empty());
        assert_eq!(1, p.time.created());
    }
}

#[test]
fn memory_pause_navigation_during_pause_behaves_as_before() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_NavigationDuringPause_BehavesAsBefore
    let p = paused_run(false, 0);
    assert_eq!(1, p.time.created());
    p.probe.set_load(0.50);
    p.scheduler.notify_navigation(5);
    assert!(p.time.is_disposed(0));
    assert!(finished(&p.scheduler.preload_around(5)));
    let mut names: Vec<String> = p.target.started().iter().map(|s| file_name(s)).collect();
    names.sort();
    assert_eq!(vec!["img-6.jpg", "img-7.jpg", "img-8.jpg", "img-9.jpg"], names);
    p.time.fire(0); // a stale callback
    assert_eq!(0, p.scheduler.resumes_for_tests());
    assert_eq!(4, p.target.started().len());
}

#[test]
fn memory_pause_navigation_during_the_probe_supersedes_the_resume() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_NavigationDuringTheProbe_SupersedesTheResume
    let p = paused_run(false, 0);
    // The re-check's own probe (the only call with the resume limit) sits between its generation check and its kick.
    let weak = Arc::downgrade(&p.scheduler);
    p.probe.set_answer(move |maximum_load| {
        if maximum_load < PAUSE_LIMIT {
            if let Some(scheduler) = weak.upgrade() {
                scheduler.notify_navigation(5);
            }
        }
        true
    });
    p.time.fire(p.time.pending_index().expect("a pending re-check"));
    assert_eq!(0, p.scheduler.resumes_for_tests());
    // the navigation's own PreloadAroundAsync (not called here) is what preloads around 5
    assert!(p.target.started().is_empty());
}

#[test]
fn memory_pause_cache_at_fill_limit_does_not_resume() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_CacheAtFillLimit_DoesNotResume
    let p = paused_run(true, 950_000_000);
    p.probe.set_load(0.50);
    p.time.fire(p.time.pending_index().expect("a pending re-check"));
    assert_eq!(0, p.scheduler.resumes_for_tests());
    assert!(p.target.started().is_empty());
    assert_eq!(1, p.time.created());
    assert!(p.time.pending_index().is_none());
}

#[test]
fn memory_pause_viewer_decoding_waits_for_the_next_check() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_ViewerDecoding_WaitsForTheNextCheck
    let p = paused_run(false, 0);
    p.probe.set_load(0.50);
    p.target.set_active_viewer(1);
    p.time.fire(p.time.pending_index().expect("a pending re-check"));
    assert_eq!(0, p.scheduler.resumes_for_tests());
    assert_eq!(2, p.time.created());
    p.target.set_active_viewer(0);
    p.time.fire(p.time.pending_index().expect("a pending re-check"));
    assert!(finished(
        &p.scheduler.resumed_run_for_tests().expect("the re-check resumed a run")
    ));
    assert_eq!(1, p.scheduler.resumes_for_tests());
}

#[test]
fn memory_pause_resumed_run_pauses_again_backoff_continues() {
    // C#: PreloadSchedulerMutationTests.MemoryPause_ResumedRunPausesAgain_BackoffContinues
    let p = paused_run(false, 0);
    // The re-check sees headroom, then the pressure is back before the resumed run probes again.
    let probe = Arc::clone(&p.probe);
    p.probe.set_answer(move |maximum_load| {
        let ok = probe.load() < maximum_load;
        if ok && maximum_load < PAUSE_LIMIT {
            probe.set_load(0.95);
        }
        ok
    });
    p.probe.set_load(0.50);
    p.time.fire(p.time.pending_index().expect("a pending re-check"));
    assert!(finished(
        &p.scheduler.resumed_run_for_tests().expect("the re-check resumed a run")
    ));
    assert_eq!(1, p.scheduler.resumes_for_tests());
    assert!(p.target.started().is_empty());
    assert_eq!(2, p.time.created());
    assert_eq!(resume_check_delay(1), p.time.dues()[1]); // 4 s, not back to 2 s
}
