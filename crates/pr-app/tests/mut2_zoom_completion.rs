//! N3: deterministic tests for the 25 `zoom_detail.rs` mutants that M4 left NOT RUN because they sit in the "threaded" completion
//! machinery: `catch_job`, `CompletionInbox`, `retarget_path`, `pending_load`, `queued_events`, `pump`. No thread, no timing: the
//! inbox is driven directly, and the loader gets a queued `BackgroundRunner` that the test steps itself (the worker jobs run on
//! the test thread, in the order the test chooses). Hand-written, no golden data, no production change.

#[path = "support/preview_fakes.rs"]
mod preview_fakes;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_app::coordinators::zoom_detail::{
    BackgroundJob, BackgroundRunner, CompletionInbox, ZoomDetailEffect, ZoomDetailLoader, catch_job,
};
use pr_core::catalog::generation::GenerationClock;
use pr_test_support::TempRoot;

// --- CompletionInbox ---------------------------------------------------------------------------------------------------------

fn inbox_with_wake_counter() -> (CompletionInbox<u32>, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let inbox = CompletionInbox::new(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    (inbox, wakes)
}

#[test]
fn inbox_send_queues_the_event_and_wakes_once_per_send() {
    let (inbox, wakes) = inbox_with_wake_counter();
    assert!(inbox.is_empty());
    assert_eq!(inbox.len(), 0);
    inbox.send(7);
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert!(!inbox.is_empty());
    assert_eq!(inbox.len(), 1);
    inbox.send(8);
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert_eq!(inbox.len(), 2);
    assert!(!inbox.is_empty());
}

#[test]
fn inbox_drain_returns_every_event_in_arrival_order_and_empties_the_queue() {
    let (inbox, _) = inbox_with_wake_counter();
    assert_eq!(inbox.drain(), Vec::<u32>::new());
    for n in [3, 1, 4, 1, 5] {
        inbox.send(n);
    }
    assert_eq!(inbox.len(), 5);
    assert_eq!(inbox.drain(), vec![3, 1, 4, 1, 5]);
    assert_eq!(inbox.len(), 0);
    assert!(inbox.is_empty());
    assert_eq!(inbox.drain(), Vec::<u32>::new());
}

#[test]
fn inbox_clone_shares_the_queue_and_the_wake_function() {
    let (inbox, wakes) = inbox_with_wake_counter();
    let clone = inbox.clone();
    clone.send(1);
    assert_eq!(inbox.len(), 1);
    inbox.send(2);
    assert_eq!(clone.len(), 2);
    assert_eq!(wakes.load(Ordering::SeqCst), 2);
    assert_eq!(clone.drain(), vec![1, 2]);
    assert!(inbox.is_empty());
}

#[test]
fn inbox_debug_names_the_type() {
    let (inbox, _) = inbox_with_wake_counter();
    inbox.send(1);
    let text = format!("{inbox:?}");
    assert!(text.starts_with("CompletionInbox"), "{text}");
    assert!(!text.is_empty());
}

#[test]
fn inbox_wait_ready_reports_whether_an_event_is_queued() {
    let (inbox, _) = inbox_with_wake_counter();
    // Empty: false after the (short) timeout. Zero timeout: false at once.
    assert!(!inbox.wait_ready(Duration::ZERO));
    assert!(!inbox.wait_ready(Duration::from_millis(20)));
    inbox.send(1);
    // Queued: true at once, and the event stays queued (waiting does not consume it).
    assert!(inbox.wait_ready(Duration::ZERO));
    assert!(inbox.wait_ready(Duration::from_secs(30)));
    assert_eq!(inbox.len(), 1);
    inbox.drain();
    assert!(!inbox.wait_ready(Duration::from_millis(20)));
}

#[test]
fn inbox_wait_ready_wakes_when_another_thread_sends() {
    // The long timeout is a hang guard only: the waiter returns as soon as the sender has queued its event.
    let (inbox, _) = inbox_with_wake_counter();
    let sender = inbox.clone();
    let handle = std::thread::spawn(move || sender.send(42));
    assert!(inbox.wait_ready(Duration::from_secs(60)));
    handle.join().expect("sender thread");
    assert_eq!(inbox.drain(), vec![42]);
}

// --- catch_job ---------------------------------------------------------------------------------------------------------------

#[test]
fn catch_job_returns_the_work_result_and_never_calls_on_panic_without_a_panic() {
    let on_panic_calls = AtomicUsize::new(0);
    let value = catch_job(
        || 7,
        |_| {
            on_panic_calls.fetch_add(1, Ordering::SeqCst);
            0
        },
    );
    assert_eq!(value, 7);
    assert_eq!(on_panic_calls.load(Ordering::SeqCst), 0);
    assert_eq!(catch_job(|| "ok".to_owned(), |_| "panicked".to_owned()), "ok");
}

#[test]
fn catch_job_turns_a_panic_into_the_on_panic_result_with_the_panic_text() {
    let text = catch_job(
        || -> String { panic!("boom {}", 42) },
        |message| format!("caught: {message}"),
    );
    assert!(text.starts_with("caught: "), "{text}");
    assert!(text.contains("boom 42"), "{text}");
    assert_eq!(catch_job(|| -> i32 { panic!("x") }, |_| 5), 5);
}

// --- ZoomDetailLoader through a queued runner ---------------------------------------------------------------------------------

#[derive(Default)]
struct QueuedRunner(Mutex<VecDeque<BackgroundJob>>);

impl BackgroundRunner for QueuedRunner {
    fn run(&self, _: &'static str, job: BackgroundJob) {
        self.0.lock().expect("job queue").push_back(job);
    }
}

impl QueuedRunner {
    /// Runs the oldest queued job on the calling thread.
    fn next(&self) {
        let job = self.0.lock().expect("job queue").pop_front().expect("queued job");
        job();
    }

    fn queued(&self) -> usize {
        self.0.lock().expect("job queue").len()
    }
}

struct Rig {
    root: TempRoot,
    decoder: Arc<preview_fakes::SizedDecoder>,
    clock: Arc<GenerationClock>,
    runner: Arc<QueuedRunner>,
    loader: ZoomDetailLoader,
    key: pr_imaging::cache_key::ImageCacheKey,
    path: String,
}

fn rig(name: &str) -> Rig {
    let root = TempRoot::new("n3-zoom");
    let file = root.join(name);
    std::fs::write(&file, [1, 2, 3]).expect("fixture source");
    let path = file.to_string_lossy().into_owned();
    let decoder = preview_fakes::SizedDecoder::new();
    decoder.set_original(60, 40);
    let service = preview_fakes::build_service(&root.join("cache"), decoder.clone(), Default::default());
    let clock = Arc::new(GenerationClock::default());
    let runner = Arc::new(QueuedRunner::default());
    let loader = ZoomDetailLoader::new(service.clone(), clock.clone(), runner.clone(), Arc::new(|| {}));
    let key = service.get_current_cache_key(&path).expect("source identity");
    Rig {
        root,
        decoder,
        clock,
        runner,
        loader,
        key,
        path,
    }
}

impl Rig {
    /// A downscaled preview (6 of 60 pixels wide) of the current image is on screen for a fresh navigation.
    fn present(&mut self) -> i64 {
        let token = self.clock.next_navigation();
        let preview = preview_fakes::sized_image(6, 4, 60, 40, true);
        self.loader.on_preview_presented(token, &self.path, &self.key, &preview);
        token
    }

    /// Renames the source file (same directory) and returns the new full path.
    fn rename_to(&self, name: &str) -> String {
        let new_path = self.root.join(name).to_string_lossy().into_owned();
        std::fs::rename(&self.path, &new_path).expect("rename owned fixture");
        new_path
    }
}

#[test]
fn pending_load_follows_the_decode_from_start_to_pump() {
    let mut r = rig("a.jpg");
    assert!(!r.loader.pending_load(), "nothing started");
    r.present();
    assert!(!r.loader.pending_load(), "Fit starts no decode");
    r.loader.set_zoom(Some(1.0));
    assert!(
        r.loader.pending_load(),
        "zoomed past the preview: the decode is in flight"
    );
    r.runner.next();
    assert!(
        r.loader.pending_load(),
        "the job ran, but its outcome was not pumped yet"
    );
    r.loader.pump();
    assert!(!r.loader.pending_load(), "pumped: the slot is free");
}

#[test]
fn queued_events_counts_the_completions_waiting_for_pump() {
    let mut r = rig("a.jpg");
    assert_eq!(r.loader.queued_events(), 0);
    r.present();
    r.loader.set_zoom(Some(1.0));
    assert_eq!(r.loader.queued_events(), 0, "the job did not run yet");
    r.runner.next();
    assert_eq!(r.loader.queued_events(), 1);
    r.loader.pump();
    assert_eq!(r.loader.queued_events(), 0);

    // Two completions at once: a superseded decode (its job is already queued) and the decode of the next navigation.
    let mut r = rig("b.jpg");
    r.present();
    r.loader.set_zoom(Some(1.0));
    r.present(); // reset cancels the first load; a second target is armed and decodes at the same zoom
    assert_eq!(r.runner.queued(), 2);
    r.runner.next();
    r.runner.next();
    assert_eq!(r.loader.queued_events(), 2);
    assert!(r.loader.wait_for_event(Duration::from_secs(30)));
    r.loader.pump();
    assert_eq!(r.loader.queued_events(), 0);
    assert!(!r.loader.wait_for_event(Duration::from_millis(10)));
}

#[test]
fn pump_applies_the_completion_and_returns_its_effects() {
    let mut r = rig("a.jpg");
    r.present();
    assert!(r.loader.pump().is_empty(), "nothing queued, nothing to apply");
    r.loader.set_zoom(Some(1.0));
    r.runner.next();
    let effects = r.loader.pump();
    assert_eq!(effects.len(), 2, "{effects:?}");
    assert!(
        matches!(
            &effects[0],
            ZoomDetailEffect::ShowImage {
                original_width: 60,
                original_height: 40,
                ..
            }
        ),
        "{effects:?}"
    );
    assert!(
        matches!(&effects[1], ZoomDetailEffect::OriginalShown { path } if *path == r.path),
        "{effects:?}"
    );
    assert!(r.loader.held_original().is_some());
    assert!(r.loader.is_showing_original());
    assert!(r.loader.pump().is_empty(), "a second pump has nothing left");
}

#[test]
fn retarget_path_gives_the_decode_to_the_new_path_token_and_identity() {
    let mut r = rig("old.jpg");
    r.present();
    r.loader.set_zoom(Some(1.0));
    r.runner.next(); // the decode of the old path ran, its completion waits in the inbox
    let new_path = r.rename_to("new.jpg");
    let new_token = r.clock.next_navigation();
    let effects = r.loader.retarget_path(new_token, &new_path);
    assert!(effects.is_empty(), "{effects:?}");

    // The old target's completion must be dropped (the new target has another id), the pixels are not kept.
    assert!(r.loader.pump().is_empty());
    assert!(
        r.loader.held_original().is_none(),
        "the old target's pixels were accepted by the renamed target"
    );
    assert!(!r.loader.is_showing_original());

    // The new target decodes the NEW path (token current, so the decode started) and its completion is accepted.
    assert!(
        r.loader.pending_load(),
        "the retarget restarted the decode under the new navigation token"
    );
    assert_eq!(r.runner.queued(), 1);
    r.runner.next();
    let effects = r.loader.pump();
    assert_eq!(r.decoder.original_decodes("new.jpg"), 1);
    assert_eq!(r.decoder.original_decodes("old.jpg"), 1);
    assert!(
        matches!(effects.last(), Some(ZoomDetailEffect::OriginalShown { path }) if *path == new_path),
        "{effects:?}"
    );
    assert!(r.loader.held_original().is_some());
}

#[test]
fn retarget_path_with_a_stale_navigation_token_starts_nothing() {
    // The new target carries `new_token`; when the clock has moved on, the target is not current and no decode starts. A target
    // that kept the previous token (a deleted `token` field) would be judged against the old, equally stale token: so also check
    // the positive case, where only the NEW token is current.
    let mut r = rig("old.jpg");
    let first = r.present();
    r.loader.set_zoom(Some(1.0));
    let new_path = r.rename_to("new.jpg");
    let newer = r.clock.next_navigation();
    assert_ne!(first, newer);
    // `first` is now stale; the retarget with the CURRENT token must restart the decode.
    r.loader.retarget_path(newer, &new_path);
    assert!(r.loader.pending_load());
    assert_eq!(
        r.runner.queued(),
        2,
        "the cancelled job of the old path and the new one"
    );
    // Retargeting with a token that is not current starts nothing.
    let stale = newer;
    let _moved_on = r.clock.next_navigation();
    r.loader.retarget_path(stale, &new_path);
    assert!(!r.loader.pending_load(), "a stale token must not start a decode");
    assert_eq!(r.runner.queued(), 2);
}

#[test]
fn retarget_path_moves_a_latched_failure_to_the_new_token_and_only_then() {
    // Failure latched on the old token: after the retarget the latch follows the target, so zooming does not retry.
    let mut r = rig("old.jpg");
    r.decoder.fail("old.jpg");
    r.present();
    r.loader.set_zoom(Some(1.0));
    r.runner.next();
    r.loader.pump(); // the decode failed: failed_token = the old token, the slot is free
    assert!(!r.loader.pending_load());
    let new_path = r.rename_to("new.jpg");
    let new_token = r.clock.next_navigation();
    r.loader.retarget_path(new_token, &new_path);
    assert!(
        !r.loader.pending_load(),
        "the failure of this image is still latched under the new token"
    );
    assert_eq!(r.runner.queued(), 0);

    // The same question for the retarget itself: nothing failed, so the decode must restart.
    let mut r = rig("old.jpg");
    r.present();
    r.loader.set_zoom(Some(1.0));
    let new_path = r.rename_to("new.jpg");
    let new_token = r.clock.next_navigation();
    r.loader.retarget_path(new_token, &new_path);
    assert!(
        r.loader.pending_load(),
        "no failure was latched, so nothing may block the restart"
    );
}

// --- refresh: `held || showing_original` (mutant 450:30) --------------------------------------------------------------------

use pr_app::coordinators::zoom_detail::ZoomDetailRefreshResult;

#[derive(Clone, Copy, Debug)]
enum Op {
    Present,
    ZoomFit,
    ZoomIn,
    ZoomOutSmall,
    RefreshBig,
    RefreshSmall,
    RefreshNan,
    RunJob,
    Pump,
    Reset,
    StaleNavigation,
    FailDecodes,
    RetargetSamePath,
}

const OPS: [Op; 13] = [
    Op::Present,
    Op::ZoomFit,
    Op::ZoomIn,
    Op::ZoomOutSmall,
    Op::RefreshBig,
    Op::RefreshSmall,
    Op::RefreshNan,
    Op::RunJob,
    Op::Pump,
    Op::Reset,
    Op::StaleNavigation,
    Op::FailDecodes,
    Op::RetargetSamePath,
];

/// Applies `op`; on a refresh that reached the hold branch, checks the property the mutant would break.
fn step(r: &mut Rig, op: Op, reached_hold: &mut usize, showed: &mut usize) {
    match op {
        Op::Present => {
            r.present();
        }
        Op::ZoomFit => {
            r.loader.set_zoom(None);
        }
        Op::ZoomIn => {
            r.loader.set_zoom(Some(1.0));
        }
        Op::ZoomOutSmall => {
            r.loader.set_zoom(Some(0.05));
        }
        Op::RefreshBig | Op::RefreshSmall | Op::RefreshNan => {
            let zoom = match op {
                Op::RefreshBig => 1.0,
                Op::RefreshSmall => 0.05,
                _ => f64::NAN,
            };
            let held_before = r.loader.held_original().is_some();
            let (result, _) = r.loader.refresh(zoom);
            if matches!(
                result,
                ZoomDetailRefreshResult::ShowedOriginal
                    | ZoomDetailRefreshResult::LoadingOriginal
                    | ZoomDetailRefreshResult::RetryingOriginal
            ) {
                // The hold branch ran `update`: it shows the held original at once, and nothing else can show one.
                *reached_hold += 1;
                assert_eq!(
                    held_before,
                    r.loader.is_showing_original(),
                    "held before == showing after, for {result:?}"
                );
                assert_eq!(
                    result == ZoomDetailRefreshResult::ShowedOriginal,
                    held_before,
                    "{result:?}"
                );
                *showed += usize::from(held_before);
            }
        }
        Op::RunJob => {
            if r.runner.queued() > 0 {
                r.runner.next();
            }
        }
        Op::Pump => {
            r.loader.pump();
        }
        Op::Reset => {
            r.loader.reset();
        }
        Op::StaleNavigation => {
            r.clock.next_navigation();
        }
        Op::FailDecodes => {
            r.decoder.fail("seq.jpg");
        }
        Op::RetargetSamePath => {
            let token = r.clock.next_navigation();
            let path = r.path.clone();
            r.loader.retarget_path(token, &path);
        }
    }
}

#[test]
fn refresh_hold_branch_shows_the_original_exactly_when_one_was_held_in_every_reachable_state() {
    // 450:30 (`held || showing_original` to `&&`): the two operands are equal whenever the hold branch is reached (`held` is the
    // original present before `update`; `update` shows a held original at once; nothing creates an original synchronously), so `||`
    // and `&&` return the same result. Domain: EVERY sequence of up to 4 operations over 13 operations (30k sequences, a fresh
    // loader each) and every sequence of 5 operations over the 8 that matter for the hold (32k more); the property is asserted at
    // each refresh that reaches the hold branch.
    let (reached_hold, showed, sequences);
    fn walk_after(warmed: &[Op], counters: &mut (usize, usize, usize)) {
        for a in OPS {
            for b in OPS {
                for c in OPS {
                    let mut r = rig("seq.jpg");
                    for op in warmed.iter().chain([&a, &b, &c]) {
                        step(&mut r, *op, &mut counters.0, &mut counters.1);
                    }
                    counters.2 += 1;
                }
            }
        }
    }
    fn walk(ops: &[Op], depth: usize, prefix: &mut Vec<Op>, counters: &mut (usize, usize, usize)) {
        if depth == 0 {
            let mut r = rig("seq.jpg");
            for op in prefix.iter() {
                step(&mut r, *op, &mut counters.0, &mut counters.1);
            }
            counters.2 += 1;
            return;
        }
        for op in ops {
            prefix.push(*op);
            walk(ops, depth - 1, prefix, counters);
            prefix.pop();
        }
    }
    let mut counters = (0usize, 0usize, 0usize);
    for depth in 1..=2 {
        walk(&OPS, depth, &mut Vec::new(), &mut counters);
    }
    let core = [Op::Present, Op::ZoomIn, Op::RefreshBig, Op::RunJob, Op::Pump, Op::Reset];
    walk(&core, 4, &mut Vec::new(), &mut counters);
    // Every continuation of 3 operations (over all 13) after the original was decoded, held and shown at zoom 1.
    walk_after(&[Op::Present, Op::ZoomIn, Op::RunJob, Op::Pump], &mut counters);
    // 2000 seeded random walks of 14 operations over all 13.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..600 {
        let mut r = rig("seq.jpg");
        for _ in 0..14 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            step(&mut r, OPS[(state % 13) as usize], &mut counters.0, &mut counters.1);
        }
        counters.2 += 1;
    }
    (reached_hold, showed, sequences) = counters;
    assert!(reached_hold > 100, "{reached_hold}");
    assert!(showed > 10, "the ShowedOriginal outcome was reached {showed} times");
    assert!(sequences > 2_000, "{sequences}");
}
