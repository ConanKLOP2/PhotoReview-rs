//! M4 mutation killers for `coordinators/zoom_detail.rs` (X-zoom-detail). Hand-written, no golden data. Decodes run through a
//! queued runner the test steps itself, so no timing is involved except one LOWER bound (the test sleeps before it lets the
//! queued decode run, and the log must report at least that many milliseconds).

#[path = "support/preview_fakes.rs"]
mod preview_fakes;

use std::collections::VecDeque;
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pr_app::coordinators::zoom_detail::{BackgroundJob, BackgroundRunner, ZoomDetailLoader, ZoomDetailRefreshResult};
use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_core::abstractions::Log;
use pr_core::catalog::generation::GenerationClock;
use pr_test_support::TempRoot;

#[derive(Default)]
struct QueuedRunner(Mutex<VecDeque<BackgroundJob>>);

impl BackgroundRunner for QueuedRunner {
    fn run(&self, _: &'static str, job: BackgroundJob) {
        self.0.lock().expect("job queue").push_back(job);
    }
}

impl QueuedRunner {
    fn next(&self) {
        let job = self.0.lock().expect("job queue").pop_front().expect("queued job");
        job();
    }

    fn queued(&self) -> usize {
        self.0.lock().expect("job queue").len()
    }
}

#[derive(Default)]
struct CaptureLog {
    lines: Mutex<Vec<String>>,
}

impl Log for CaptureLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, message: &str) {
        self.lines.lock().unwrap().push(message.to_owned());
    }
    fn warn(&self, _message: &str) {}
    fn error(&self, message: &str, _error: Option<&(dyn Error + 'static)>) {
        self.lines.lock().unwrap().push(message.to_owned());
    }
}

impl AppLogTarget for CaptureLog {
    fn set_enabled(&self, _enabled: bool) {}
    fn flush(&self) {}
    fn shutdown(&self) {}
    fn file_path(&self) -> PathBuf {
        PathBuf::from("capture.log")
    }
}

/// The process-wide log is shared by the tests of this binary: the two log tests take this lock and restore the default.
static LOG_LOCK: Mutex<()> = Mutex::new(());

struct Rig {
    _root: TempRoot,
    decoder: Arc<preview_fakes::SizedDecoder>,
    clock: Arc<GenerationClock>,
    runner: Arc<QueuedRunner>,
    loader: ZoomDetailLoader,
    key: pr_imaging::cache_key::ImageCacheKey,
    path: String,
}

fn rig(name: &str) -> Rig {
    let root = TempRoot::new("m4-zoom");
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
        _root: root,
        decoder,
        clock,
        runner,
        loader,
        key,
        path,
    }
}

// C# `Refresh`: `!(effectiveZoom > 0)` lands on "preview sufficient", so a NaN zoom must not start a decode.
#[test]
fn refresh_with_a_nan_zoom_reports_preview_sufficient_and_decodes_nothing() {
    let mut r = rig("nan.jpg");
    let preview = preview_fakes::sized_image(6, 4, 60, 40, true);
    r.loader.set_zoom(None);
    r.loader
        .on_preview_presented(r.clock.next_navigation(), &r.path, &r.key, &preview);
    let (result, effects) = r.loader.refresh(f64::NAN);
    assert_eq!(result, ZoomDetailRefreshResult::PreviewSufficient);
    assert!(effects.is_empty());
    assert_eq!(r.runner.queued(), 0);
    assert!(!r.loader.pending_load());
    assert_eq!(r.decoder.original_decodes("nan.jpg"), 0);
}

// `LoadAsync` continuation, success: the log line carries the elapsed milliseconds of the load that owns the slot.
#[test]
fn original_ready_log_reports_the_elapsed_time_of_the_owning_load() {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let capture = Arc::new(CaptureLog::default());
    AppLog::global().set_instance(Some(capture.clone()));
    let mut r = rig("slow.jpg");
    let preview = preview_fakes::sized_image(6, 4, 60, 40, true);
    r.loader
        .on_preview_presented(r.clock.next_navigation(), &r.path, &r.key, &preview);
    r.loader.set_zoom(Some(1.0));
    // The "slow decode" of the scenario: the job waits in the queue for 40 ms; the load clock runs meanwhile.
    std::thread::sleep(Duration::from_millis(40));
    r.runner.next();
    r.loader.pump();
    AppLog::global().set_instance(None);

    let lines = capture.lines.lock().unwrap().clone();
    let line = lines
        .iter()
        .find(|l| l.starts_with("ZoomDetail original ready token="))
        .unwrap_or_else(|| panic!("no ready line in {lines:?}"));
    let ms: u128 = line
        .split(" ms=")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no ms in {line}"));
    assert!(ms >= 40, "the load ran for at least 40 ms, the line says {line}");
    assert!(line.ends_with(&format!("path={}", r.path)), "{line}");
}

// `LoadAsync` continuation, failure of a superseded target: token -1 and an empty path are logged.
#[test]
fn decode_failure_of_a_replaced_target_is_logged_with_token_minus_one_and_no_path() {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let capture = Arc::new(CaptureLog::default());
    AppLog::global().set_instance(Some(capture.clone()));
    let mut r = rig("old.jpg");
    r.decoder.fail("old.jpg");
    let preview = preview_fakes::sized_image(6, 4, 60, 40, true);
    r.loader
        .on_preview_presented(r.clock.next_navigation(), &r.path, &r.key, &preview);
    r.loader.set_zoom(Some(1.0));
    r.runner.next(); // the decode fails before the target is replaced
    let new_path = r.path.replace("old.jpg", "new.jpg");
    std::fs::rename(&r.path, &new_path).expect("rename owned fixture");
    r.loader.retarget_path(r.clock.next_navigation(), &new_path);
    r.loader.pump();
    AppLog::global().set_instance(None);

    let lines = capture.lines.lock().unwrap().clone();
    assert!(
        lines
            .iter()
            .any(|l| l == "ZoomDetail original decode failed token=-1 path="),
        "{lines:?}"
    );
}
