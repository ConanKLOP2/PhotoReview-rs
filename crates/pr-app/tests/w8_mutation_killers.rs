//! Regressions for observable survivors from the postmerge Wave 8 pure-logic mutation run.

// The shared pointer fake also supports wheel and zoom tests; this binary uses only pan operations.
#[allow(dead_code)]
#[path = "common/pointer_fixture.rs"]
mod pointer_fixture;

#[path = "support/preview_fakes.rs"]
mod preview_fakes;

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use pr_app::coordinators::zoom_detail::{BackgroundJob, BackgroundRunner, ZoomDetailEffect, ZoomDetailLoader};
use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_core::abstractions::Log;
use pr_core::catalog::generation::GenerationClock;
use pr_test_support::TempRoot;

use pointer_fixture::{Fixture, point};
use pr_app::input::pointer::PointerStep;
use pr_app::input::primitives::PointerButton;

static ZOOM_LOG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct MemoryLog {
    enabled: AtomicBool,
    messages: Mutex<Vec<String>>,
}

impl Log for MemoryLog {
    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }
    fn info(&self, message: &str) {
        self.messages.lock().expect("messages lock").push(message.to_owned());
    }
    fn warn(&self, message: &str) {
        self.info(message);
    }
    fn error(&self, message: &str, _error: Option<&(dyn Error + 'static)>) {
        self.info(message);
    }
}

impl AppLogTarget for MemoryLog {
    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }
    fn flush(&self) {}
    fn shutdown(&self) {}
    fn file_path(&self) -> PathBuf {
        PathBuf::new()
    }
}

struct LogScope {
    _lock: MutexGuard<'static, ()>,
    target: Arc<MemoryLog>,
    previous: Arc<dyn AppLogTarget>,
}

impl LogScope {
    fn new() -> Self {
        let lock = ZOOM_LOG_LOCK.lock().expect("log fixture lock");
        let target = Arc::new(MemoryLog::default());
        target.set_enabled(true);
        let previous = AppLog::global().instance();
        AppLog::global().set_instance(Some(target.clone()));
        Self {
            _lock: lock,
            target,
            previous,
        }
    }
}

impl Drop for LogScope {
    fn drop(&mut self) {
        AppLog::global().set_instance(Some(self.previous.clone()));
    }
}

#[derive(Default)]
struct QueuedRunner {
    jobs: Mutex<Vec<BackgroundJob>>,
}

impl BackgroundRunner for QueuedRunner {
    fn run(&self, _name: &'static str, job: BackgroundJob) {
        self.jobs.lock().expect("queue lock").push(job);
    }
}

impl QueuedRunner {
    fn run_next(&self) {
        let job = self.jobs.lock().expect("queue lock").remove(0);
        job();
    }

    fn len(&self) -> usize {
        self.jobs.lock().expect("queue lock").len()
    }
}

#[test]
fn vertical_drag_crosses_threshold_from_a_nonzero_origin() {
    // C#: PointerInputController.OnImageMove / IsBeyondDragThreshold.
    // Mutant: crates/pr-app/src/input/pointer/pan.rs:149:28, replace - with /.
    // A quotient close to one hides a vertical drag when the press starts away from zero.
    for end_y in [180.0, 220.0] {
        let mut f = Fixture::new();
        f.settings.click_to_zoom_enabled = true;
        f.run(|controller, ctx| {
            assert!(controller.on_image_press(ctx, PointerButton::Left, 1, point(200.0, 200.0), 100));
            assert!(controller.on_image_move(ctx, true, point(200.0, end_y), 110));
            let release = controller.on_image_release(ctx, point(200.0, end_y), 120);
            assert!(release.handled);
            assert!(matches!(release.step, PointerStep::Done));
            PointerStep::Done
        });
        assert_eq!(f.viewer.zoom(), 1.0, "a vertical drag must not turn into click zoom");
    }
}

#[test]
fn vertical_jitter_below_threshold_is_not_a_drag() {
    // C#: PointerInputController.OnImageMove / IsBeyondDragThreshold.
    // Mutant: crates/pr-app/src/input/pointer/pan.rs:149:28, replace - with +.
    // The survivor-only line filter also generated this operator; a large origin must not count as movement.
    for end_y in [199.0, 201.0] {
        let mut f = Fixture::new();
        f.settings.click_to_zoom_enabled = true;
        f.run(|controller, ctx| {
            assert!(controller.on_image_press(ctx, PointerButton::Left, 1, point(200.0, 200.0), 100));
            assert!(!controller.on_image_move(ctx, true, point(200.0, end_y), 110));
            PointerStep::Done
        });
    }
}

#[test]
fn retarget_drops_the_old_completion_and_decodes_the_renamed_file() {
    // C#: ZoomDetailLoader.RetargetPath / LoadAsync stale-completion guard.
    // Mutants: crates/pr-app/src/coordinators/zoom_detail.rs:306..309,
    // delete id/token/path/key from the Target struct update (generated even with the W8 function filter).
    let _log = LogScope::new();
    let root = TempRoot::new("w8-mut-zoom-retarget");
    let old_file = root.join("old.jpg");
    let new_file = root.join("new.jpg");
    std::fs::write(&old_file, [0xFF, 0xD8, 0xFF, 0xD9]).expect("fixture source");
    let old_path = old_file.to_string_lossy().into_owned();
    let new_path = new_file.to_string_lossy().into_owned();
    let decoder = preview_fakes::SizedDecoder::new();
    let service = preview_fakes::build_service(&root.join("cache"), decoder.clone(), Default::default());
    let clock = Arc::new(GenerationClock::default());
    let runner = Arc::new(QueuedRunner::default());
    let mut loader = ZoomDetailLoader::new(service.clone(), clock.clone(), runner.clone(), Arc::new(|| {}));
    let preview = preview_fakes::sized_image(600, 400, 6000, 4000, true);
    let key = service.get_current_cache_key(&old_path).expect("old source key");
    loader.on_preview_presented(clock.next_navigation(), &old_path, &key, &preview);
    loader.set_zoom(Some(1.0));
    assert_eq!(runner.len(), 1);
    runner.run_next(); // Old decode completed, but its UI completion has not been pumped.
    std::fs::rename(&old_file, &new_file).expect("rename isolated fixture");
    loader.retarget_path(clock.next_navigation(), &new_path);
    assert_eq!(runner.len(), 1, "new navigation must schedule the renamed source");
    assert!(loader.pump().is_empty(), "the old target completion must be discarded");
    assert!(loader.held_original().is_none());
    assert!(loader.pending_load());
    runner.run_next();
    let effects = loader.pump();
    assert!(
        loader.held_original().is_some(),
        "the retargeted key must name the existing source"
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, ZoomDetailEffect::OriginalShown { path } if path == &new_path))
    );
    assert_eq!(decoder.original_decodes("old.jpg"), 1);
    assert_eq!(decoder.original_decodes("new.jpg"), 1);
}

#[test]
fn stale_decode_failure_logs_the_absent_target_sentinel() {
    // C#: ZoomDetailLoader.LoadAsync error diagnostic after navigation/reset.
    // Mutant: crates/pr-app/src/coordinators/zoom_detail.rs:632:57, delete unary - from -1.
    let log = LogScope::new();
    let root = TempRoot::new("w8-mut-zoom-error");
    let file = root.join("failed.jpg");
    std::fs::write(&file, [1, 2, 3]).expect("fixture source");
    let path = file.to_string_lossy().into_owned();
    let decoder = preview_fakes::SizedDecoder::new();
    decoder.fail("failed.jpg");
    let service = preview_fakes::build_service(&root.join("cache"), decoder, Default::default());
    let clock = Arc::new(GenerationClock::default());
    let runner = Arc::new(QueuedRunner::default());
    let mut loader = ZoomDetailLoader::new(service.clone(), clock.clone(), runner.clone(), Arc::new(|| {}));
    let key = service.get_current_cache_key(&path).expect("source key");
    loader.on_preview_presented(
        clock.next_navigation(),
        &path,
        &key,
        &preview_fakes::sized_image(600, 400, 6000, 4000, true),
    );
    loader.set_zoom(Some(1.0));
    runner.run_next(); // Queue a failed completion before reset removes its target.
    loader.reset();
    assert!(loader.pump().is_empty());
    assert!(
        log.target
            .messages
            .lock()
            .expect("messages lock")
            .iter()
            .any(|m| m == "ZoomDetail original decode failed token=-1 path=")
    );
}

#[test]
fn elapsed_indicator_from_an_old_slot_does_not_show_the_new_indicator() {
    // C#: ZoomDetailLoader.ShowIndicatorAfterDelayAsync / ShowIfCurrent.
    // Mutant: crates/pr-app/src/coordinators/zoom_detail.rs:705:57, replace && with ||.
    let _log = LogScope::new();
    let root = TempRoot::new("w8-mut-zoom-indicator");
    let file = root.join("image.nef");
    std::fs::write(&file, [1, 2, 3]).expect("fixture source");
    let path = file.to_string_lossy().into_owned();
    let decoder = preview_fakes::SizedDecoder::new();
    let options = preview_fakes::ServiceOptions {
        raw_full_decoder: Some(decoder.clone()),
        raw_full_decode_enabled: true,
        ..Default::default()
    };
    let service = preview_fakes::build_service(&root.join("cache"), decoder, options);
    let clock = Arc::new(GenerationClock::default());
    let runner = Arc::new(QueuedRunner::default());
    let mut loader = ZoomDetailLoader::new(service.clone(), clock.clone(), runner.clone(), Arc::new(|| {}))
        .with_indicator_delay(Arc::new(|_, _| true));
    let key = service.get_current_cache_key(&path).expect("source key");
    let preview = preview_fakes::sized_image(600, 400, 6000, 4000, true);
    loader.on_preview_presented(clock.next_navigation(), &path, &key, &preview);
    loader.set_zoom(Some(1.0));
    assert_eq!(runner.len(), 2);
    runner.run_next(); // Timer elapsed, but its message has not reached the UI pump yet.
    loader.reset();
    loader.on_preview_presented(clock.next_navigation(), &path, &key, &preview);
    assert_eq!(runner.len(), 3); // Cancelled old decode plus the two new jobs.
    assert!(loader.pump().is_empty());
    assert!(!loader.is_raw_decode_indicator_visible());
    runner.run_next(); // Old decode observes its cancellation.
    assert!(loader.pump().is_empty());
    runner.run_next(); // The current timer is allowed to show its indicator.
    assert!(
        loader
            .pump()
            .iter()
            .any(|effect| matches!(effect, ZoomDetailEffect::RawDecodeIndicatorChanged { visible: true }))
    );
    assert!(loader.is_raw_decode_indicator_visible());
    loader.reset(); // Cancel queued work before dropping the fake queue.
}
