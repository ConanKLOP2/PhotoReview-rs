//! Mutation killers for `coordinators/presenter/state.rs` (lane cl-mut-state2, from the CL-MUT-MISC survivor list of the X-w9r6-rename-view-retarget row).
//! Fakes only (kind F), no oracle, no golden, no fixed sleep: every hang guard is a thread plus `mpsc::recv_timeout`, every wait is bounded.

mod support {
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use pr_app::coordinators::presenter::state::{ImagePresenterDeps, PresentError, PresenterChanges};
use pr_core::abstractions::FileSystem;
use support::present_helpers::{ScriptedFs, start};
use support::presenter_fakes::{
    FakeHasher, FixtureOptions, PresenterFixture, RecordingSink, build_thumbnail_cache, env,
};
use support::preview_fakes::{SizedDecoder, TIMEOUT};

fn fixture(names: &[&str]) -> PresenterFixture {
    PresenterFixture::new(SizedDecoder::new(), names)
}

fn fixture_with_fs(names: &[&str], fs: &Arc<ScriptedFs>) -> PresenterFixture {
    PresenterFixture::with_options(
        SizedDecoder::new(),
        names,
        FixtureOptions {
            file_system: Some(Arc::clone(fs) as _),
            ..FixtureOptions::default()
        },
    )
}

/// Runs `body` on its own thread: a hung `pump` loop fails the test after 10 s instead of hanging the run (a thread plus `mpsc::recv_timeout`, no fixed sleep).
fn guarded(body: impl FnOnce() + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        body();
        let _ = tx.send(());
    });
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => handle.join().expect("body finished"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(handle.join().expect_err("body panicked"))
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("hang guard: the body did not finish in 10 s (an endless pump loop?)")
        }
    }
}

// ---- Display / Debug / bitor / clock ---------------------------------------------------------------------------------------

// Kills state.rs `<impl Display for PresentError>::fmt -> Ok(Default::default())` (writes nothing).
#[test]
fn present_error_display_is_the_csharp_argument_exception_text() {
    assert_eq!(
        PresentError::PathNotInEntry.to_string(),
        "The presentation path must belong to the selected catalog entry."
    );
}

// Kills `<impl Debug for ImagePresenterDeps>::fmt -> Ok(Default::default())`.
#[test]
fn deps_debug_names_the_struct() {
    let f = fixture(&[]);
    let (sink, _log) = RecordingSink::new();
    let deps = ImagePresenterDeps {
        clock: Arc::clone(&f.clock),
        preview_service: Arc::clone(&f.service),
        thumbnail_cache: build_thumbnail_cache(&f.root.join("thumbs-deps-debug")),
        preload: Arc::clone(&f.preload) as _,
        hasher: FakeHasher::new("HASH"),
        metrics: Arc::clone(&f.metrics),
        session_store: None,
        session_writer: None,
        sink: Box::new(sink),
        file_system: None,
        on_presented_hook: None,
        ui_scheduler: None,
        busy_retry_delay: None,
        runner: None,
        stat_worker: None,
        wake: Arc::new(|| {}),
        indicator_delay: None,
    };
    let text = format!("{deps:?}");
    assert!(text.starts_with("ImagePresenterDeps"), "got {text}");
}

// Kills `<impl Debug for ImagePresenter>::fmt -> Ok(Default::default())` and `|| -> &&` in the `busy` field
// (a present in flight with no missing run: `navigation.is_some() || missing.is_some()` is true, `&&` is false).
#[test]
fn presenter_debug_reports_the_path_the_status_and_busy() {
    guarded(|| {
        let mut f = fixture(&["a.jpg"]);
        let idle = format!("{:?}", f.presenter);
        assert!(idle.starts_with("ImagePresenter"), "got {idle}");
        assert!(idle.contains("current_navigation_path: None"), "got {idle}");
        assert!(idle.contains("busy: false"), "got {idle}");

        start(&mut f, 0);
        let busy = format!("{:?}", f.presenter);
        assert!(busy.contains("busy: true"), "a present in flight is busy: {busy}");
        assert!(
            busy.contains(&format!("current_navigation_path: Some({:?})", f.path("a.jpg"))),
            "got {busy}"
        );
        f.pump_until_idle();
        let done = format!("{:?}", f.presenter);
        assert!(done.contains("busy: false"), "got {done}");
        assert!(done.contains("status_text"), "got {done}");
    });
}

// Kills `bitor`: `| -> ^` (the same bit twice must stay set) and keeps `& / Default` honest.
#[test]
fn bitor_unions_the_flags_and_is_idempotent() {
    let both = PresenterChanges::STATUS | PresenterChanges::PHOTO_INFO;
    assert_eq!(both.0, 2 | 4);
    assert!(both.contains(PresenterChanges::STATUS) && both.contains(PresenterChanges::PHOTO_INFO));
    let same = PresenterChanges::CURRENT_IMAGE | PresenterChanges::CURRENT_IMAGE;
    assert_eq!(same, PresenterChanges::CURRENT_IMAGE);
    assert!(!same.is_empty());
    let mut acc = PresenterChanges::COMPARE;
    acc |= PresenterChanges::COMPARE;
    acc |= PresenterChanges::CURRENT_PATH;
    assert_eq!(acc.0, 8 | 32);
}

// Kills `clock -> Box::leak(Box::new(Arc::new(Default::default())))`: the accessor must return the presenter's own clock.
#[test]
fn clock_returns_the_injected_generation_clock() {
    let f = fixture(&[]);
    assert!(Arc::ptr_eq(f.presenter.clock(), &f.clock));
    let before = f.clock.current_navigation();
    let token = f.presenter.clock().next_navigation();
    assert_eq!(token, before + 1);
    assert_eq!(f.clock.current_navigation(), token, "the shared clock moved");
}

// ---- clear_presentation / pump -------------------------------------------------------------------------------------------------

// Kills `delete !` at `if !self.compare.clear(..).is_empty()`: the non-empty compare change set must raise COMPARE.
#[test]
fn clear_presentation_reports_the_compare_changes() {
    let mut f = fixture(&[]);
    let changes = {
        let mut env = env(&mut f.catalog, &f.settings, &f.format);
        f.presenter.clear_presentation(&mut env)
    };
    assert!(changes.contains(PresenterChanges::CURRENT_PATH));
    assert!(changes.contains(PresenterChanges::PHOTO_INFO));
    assert!(changes.contains(PresenterChanges::CURRENT_IMAGE));
    assert!(
        changes.contains(PresenterChanges::COMPARE),
        "compare.clear reports a non-empty change set on a fresh presenter, so COMPARE must be raised: {changes:?}"
    );
}

// Kills `pump: > -> >=` at `zoom_detail.queued_events() > 0` (an idle pump would never leave its loop) and `Default::default()` / `&& -> ||` / `delete !` variants.
// The call runs on its own thread so a hung loop fails the test instead of the run.
#[test]
fn pump_on_an_idle_presenter_returns_empty_changes() {
    guarded(|| {
        let mut f = fixture(&[]);
        let changes = f.pump();
        assert!(changes.is_empty(), "got {changes:?}");
        assert!(f.presenter.is_idle());
    });
}

// ---- wait_for_event ---------------------------------------------------------------------------------------------------------

// Kills `+ -> -` at `Instant::now() + timeout` and `>= -> <` at `Instant::now() >= deadline`: with nothing queued the wait returns false, and
// only after the whole timeout elapsed (the loop leaves with false only when `now >= deadline`).
#[test]
fn wait_for_event_without_events_waits_the_whole_timeout_then_reports_false() {
    let f = fixture(&[]);
    let timeout = Duration::from_millis(120);
    let started = Instant::now();
    let got = f.presenter.wait_for_event(timeout);
    assert!(!got);
    assert!(
        started.elapsed() >= timeout,
        "returned early after {:?}",
        started.elapsed()
    );
}

// Kills the `true` / `false` constants and the queued-events fast path: a worker completion wakes the wait with `true`, and a second
// call with a zero timeout still reports the queued event.
#[test]
fn wait_for_event_wakes_on_a_worker_completion_and_keeps_reporting_it_until_pumped() {
    guarded(|| {
        let fs = ScriptedFs::new();
        let mut f = fixture_with_fs(&["a.jpg"], &fs);
        let a = f.path("a.jpg");
        fs.hold(&a);
        start(&mut f, 0);
        assert!(fs.wait_started(&a));
        assert!(
            !f.presenter.wait_for_event(Duration::ZERO),
            "the held stat has not completed: no event queued"
        );

        let woke = std::thread::scope(|scope| {
            scope.spawn(|| fs.release(&a));
            f.presenter.wait_for_event(TIMEOUT)
        });
        assert!(woke, "the stat completion must end the wait");
        assert!(f.presenter.wait_for_event(Duration::ZERO), "still queued until pumped");
        f.pump_until_idle();
        assert!(!f.presenter.wait_for_event(Duration::ZERO), "consumed by the pump");
    });
}

// ---- dispatch: StatAbandoned / NextForMissing -------------------------------------------------------------------------------

fn panic_stat(fs: &Arc<ScriptedFs>, path: &str) {
    fs.hold(path);
    fs.script(path, |_| panic!("scripted stat worker panic (mut_state2_killers)"));
}

fn pump_until(f: &mut PresenterFixture, what: &str, mut done: impl FnMut(&PresenterFixture) -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !done(f) {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        f.presenter.wait_for_event(Duration::from_millis(50));
        f.pump();
    }
}

// Kills dispatch `803:58 == -> !=` and `803:67 && -> ||`'s same-token half: the stat of the only missing run panics, the abandonment ends the run.
#[test]
fn abandoned_next_stat_ends_the_pending_missing_run() {
    guarded(|| {
        let fs = ScriptedFs::new();
        let mut f = fixture_with_fs(&["m.jpg", "n.jpg"], &fs);
        let (missing, next) = (f.path("m.jpg"), f.path("n.jpg"));
        std::fs::remove_file(&missing).expect("delete m.jpg");
        panic_stat(&fs, &next);
        let token = f.clock.next_navigation();
        {
            let mut env = env(&mut f.catalog, &f.settings, &f.format);
            f.presenter.remove_missing_catalog_item(&mut env, &missing, token);
        }
        assert!(fs.wait_started(&next));
        assert!(!f.presenter.is_idle(), "the next stat is pending");

        fs.release(&next);
        pump_until(&mut f, "the abandoned run to retire", |f| f.presenter.is_idle());
        assert!(f.presenter.current_presented_path().is_none(), "nothing was presented");
    });
}

// Kills dispatch `803:58 == -> !=` and `803:67 && -> ||` from the other side: the abandonment of an OLDER run (other token) must not end the
// newer run that replaced it.
#[test]
fn abandonment_of_an_older_missing_run_leaves_the_newer_run_pending() {
    guarded(|| {
        let fs = ScriptedFs::new();
        let mut f = fixture_with_fs(&["m1.jpg", "n1.jpg", "m2.jpg", "n2.jpg"], &fs);
        let (m1, n1, m2) = (f.path("m1.jpg"), f.path("n1.jpg"), f.path("m2.jpg"));
        std::fs::remove_file(&m1).expect("delete m1.jpg");
        std::fs::remove_file(&m2).expect("delete m2.jpg");
        // Only the FIRST stat of n1 (run 1's) panics; which of n1/n2 run 2 stats depends on the catalog's index rule, so both are served by the disk after that.
        let real = |path: &str| pr_core::io::physical::PhysicalFileSystem.try_get_file_stat(std::path::Path::new(path));
        fs.hold(&n1);
        let n1_for_script = n1.clone();
        fs.script(&n1, move |call| {
            assert!(call != 1, "scripted stat worker panic (mut_state2_killers)");
            real(&n1_for_script)
        });

        let t1 = f.clock.next_navigation();
        {
            let mut env = env(&mut f.catalog, &f.settings, &f.format);
            f.presenter.remove_missing_catalog_item(&mut env, &m1, t1);
        }
        assert!(fs.wait_started(&n1));
        let t2 = f.clock.next_navigation();
        {
            let mut env = env(&mut f.catalog, &f.settings, &f.format);
            f.presenter.remove_missing_catalog_item(&mut env, &m2, t2);
        }
        assert!(!f.presenter.is_idle(), "run 2 waits for its own next stat");

        // Run 1's stat panics now: its abandonment event (token t1) reaches the presenter while run 2 (token t2) is pending.
        fs.release(&n1);
        let deadline = Instant::now() + TIMEOUT;
        while !f.presenter.wait_for_event(Duration::from_millis(50)) {
            assert!(Instant::now() < deadline, "the abandonment of run 1 never arrived");
        }
        f.pump();
        assert!(
            !f.presenter.is_idle(),
            "run 2 (token {t2}) is still waiting for its stat after run 1 (token {t1}) was abandoned"
        );

        // Run 2 finishes normally: the next photo is found and presented.
        fs.release_all();
        f.pump_until_idle();
        assert!(f.presenter.is_idle());
    });
}

// ---- pump: zoom-only completions -----------------------------------------------------------------------------------------------

// Kills `pump: > -> <` at `zoom_detail.queued_events() > 0` (a queued zoom completion with no presenter event must still be applied) and
// `is_same_source_swap -> true` (the flag is only true inside the sink callback of the swap).
#[test]
fn pump_applies_a_zoom_only_completion_and_clears_the_swap_flag() {
    guarded(|| {
        let mut f = fixture(&["a.jpg"]);
        start(&mut f, 0);
        f.pump_until_idle();
        let images_before = support::preview_fakes::lock(&f.sink_log).images.len();
        f.presenter.set_viewer_zoom(Some(2.0));
        f.pump_until(|f| f.presenter.zoom_detail().is_showing_original());
        let log = support::preview_fakes::lock(&f.sink_log);
        assert!(log.images.len() > images_before, "the original replaced the preview");
        assert!(
            log.images.last().is_some_and(|call| call.is_same_source_swap),
            "the sink saw the swap"
        );
        drop(log);
        assert!(!f.presenter.is_same_source_swap(), "false again once the swap returned");
    });
}
