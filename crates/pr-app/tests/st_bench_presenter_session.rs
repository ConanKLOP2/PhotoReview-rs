//! Lane `st-bench-presenter` (report `docs/reviews/ST-BENCH-PRESENTER-2026-10-31.md`): lifecycle tests of `ProductionBenchmarkSession`
//! with fake codecs, a `TempRoot` scratch parent and a capturing `AppLog` target. No engine iteration runs (every `run` here fails or is
//! cancelled before the first operation), nothing is measured, no real Recycle Bin, no real log file. The wait for the background scratch
//! teardown is a bounded `yield_now` poll (no sleeps).

mod support {
    pub mod preview_fakes;
}

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use pr_app::misc::app_log::{AppLog, AppLogTarget};
use pr_app::presenters::benchmark::{
    BenchmarkProfileSession, BenchmarkSessionDeps, ProductionBenchmarkSession, production_session_factory,
};
use pr_bench::engine::{BenchmarkError, BenchmarkProgress};
use pr_bench::models::BenchmarkProfile;
use pr_bench::profiles;
use pr_core::abstractions::{Log, RecycleBin};
use pr_core::sync::CancelToken;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder};
use pr_test_support::{FakeFileSystem, FakeRecycleBin, TempRoot};
use support::preview_fakes::{FakePayloadCodec, PixelsCodec};

struct NoDecoder;

impl ImageDecoder for NoDecoder {
    fn decode(&self, _request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        Err(DecodeError::Canceled)
    }
    fn read_info(&self, _path: &str) -> Result<ImageInfo, DecodeError> {
        Err(DecodeError::Canceled)
    }
}

/// The `AppLog` target of this test binary: remembers every `set_enabled` write and every line, writes nothing to disk.
#[derive(Default)]
struct CaptureLog {
    enabled: Mutex<bool>,
    writes: Mutex<Vec<bool>>,
    lines: Mutex<Vec<String>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Log for CaptureLog {
    fn enabled(&self) -> bool {
        *locked(&self.enabled)
    }
    fn info(&self, message: &str) {
        locked(&self.lines).push(message.to_owned());
    }
    fn warn(&self, message: &str) {
        locked(&self.lines).push(message.to_owned());
    }
    fn error(&self, message: &str, _error: Option<&(dyn Error + 'static)>) {
        locked(&self.lines).push(message.to_owned());
    }
}

impl AppLogTarget for CaptureLog {
    fn set_enabled(&self, enabled: bool) {
        *locked(&self.enabled) = enabled;
        locked(&self.writes).push(enabled);
    }
    fn flush(&self) {}
    fn shutdown(&self) {}
    fn file_path(&self) -> PathBuf {
        PathBuf::from("capture.log")
    }
}

/// `AppLog::global()` is process-wide: the tests that install a capture target run one at a time.
static GLOBAL_LOG: Mutex<()> = Mutex::new(());

fn install_capture(initially_enabled: bool) -> (MutexGuard<'static, ()>, Arc<CaptureLog>) {
    let guard = locked(&GLOBAL_LOG);
    let capture = Arc::new(CaptureLog::default());
    *locked(&capture.enabled) = initially_enabled;
    AppLog::global().set_instance(Some(Arc::clone(&capture) as Arc<dyn AppLogTarget>));
    (guard, capture)
}

fn deps(scratch_parent: Option<PathBuf>, decoders: &Arc<AtomicUsize>) -> BenchmarkSessionDeps {
    let counter = Arc::clone(decoders);
    BenchmarkSessionDeps {
        platform_codec: Arc::new(PixelsCodec),
        payload_codec: Arc::new(FakePayloadCodec),
        decoder_factory: Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Arc::new(NoDecoder) as Arc<dyn ImageDecoder>
        }),
        real_recycle_bin: Arc::new(|| {
            Arc::new(FakeRecycleBin::new(Arc::new(FakeFileSystem::new()))) as Arc<dyn RecycleBin>
        }),
        physical_memory_bytes: 1 << 30,
        memory_probe: None,
        scratch_parent,
    }
}

fn first_profile() -> BenchmarkProfile {
    profiles::all().first().expect("profiles").clone()
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::yield_now();
    }
    done()
}

fn no_progress() -> impl Fn(BenchmarkProgress) + Send + Sync {
    |_progress| {}
}

fn session_in(root: &TempRoot) -> ProductionBenchmarkSession {
    let decoders = Arc::new(AtomicUsize::new(0));
    ProductionBenchmarkSession::new(&first_profile(), &[], &deps(Some(root.path().to_path_buf()), &decoders))
        .expect("session")
}

#[test]
fn run_rejects_a_blank_folder_and_a_missing_folder_before_any_iteration() {
    let root = TempRoot::new("st_bp_folder_errors");
    let mut session = session_in(&root);
    let cancel = CancelToken::new();
    let reported = AtomicUsize::new(0);
    let progress = |_p: BenchmarkProgress| {
        reported.fetch_add(1, Ordering::SeqCst);
    };
    let (_guard, _capture) = install_capture(false);
    assert!(matches!(
        session.run("  ", &progress, &cancel),
        Err(BenchmarkError::FolderRequired)
    ));
    let missing = root.join("does-not-exist");
    let missing_text = missing.to_string_lossy().into_owned();
    match session.run(&missing_text, &progress, &cancel) {
        Err(BenchmarkError::DirectoryNotFound(path)) => assert_eq!(path, missing_text),
        other => panic!("expected DirectoryNotFound, got {other:?}"),
    }
    assert_eq!(
        reported.load(Ordering::SeqCst),
        0,
        "no progress before the folder is accepted"
    );
}

#[test]
fn run_with_a_cancelled_token_returns_canceled_and_restores_the_logging_flag() {
    // The profile's logging switch is applied inside `run` and restored when it returns, also on the cancel path.
    for detailed in [true, false] {
        let profile = profiles::all()
            .iter()
            .find(|candidate| candidate.detailed_logging == detailed)
            .unwrap_or_else(|| panic!("a profile with detailed_logging={detailed}"))
            .clone();
        let root = TempRoot::new("st_bp_cancelled");
        let folder = TempRoot::new("st_bp_cancelled_folder");
        let decoders = Arc::new(AtomicUsize::new(0));
        let mut session =
            ProductionBenchmarkSession::new(&profile, &[], &deps(Some(root.path().to_path_buf()), &decoders))
                .expect("session");
        let (_guard, capture) = install_capture(!detailed);
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = session.run(&folder.path().to_string_lossy(), &no_progress(), &cancel);
        assert!(matches!(result, Err(BenchmarkError::Canceled)), "got {result:?}");
        assert_eq!(
            *locked(&capture.writes),
            vec![detailed, !detailed],
            "the profile switch is applied, then the previous value restored"
        );
        assert_eq!(capture.enabled(), !detailed, "flag back to its value before the run");
        assert!(
            locked(&capture.lines)
                .iter()
                .any(|line| line.starts_with("Benchmark start")),
            "the run was entered through the engine (and stopped at the cancel check)"
        );
        session.dispose();
    }
}

#[test]
fn dispose_after_a_failed_and_after_a_cancelled_run_still_removes_the_scratch_directory() {
    // Lifecycle create -> run (fails / cancelled) -> dispose: the scratch directory goes away on both paths.
    let (_guard, _capture) = install_capture(false);
    for cancelled in [false, true] {
        let root = TempRoot::new("st_bp_dispose_after_run");
        let folder = TempRoot::new("st_bp_dispose_after_run_folder");
        let mut session = session_in(&root);
        let scratch = session.scratch_directory().to_path_buf();
        assert_eq!(scratch.parent(), Some(root.path()));
        std::fs::create_dir_all(&scratch).expect("scratch");
        std::fs::write(scratch.join("cache.bin"), b"x").expect("file");
        let cancel = CancelToken::new();
        let result = if cancelled {
            cancel.cancel();
            session.run(&folder.path().to_string_lossy(), &no_progress(), &cancel)
        } else {
            session.run("", &no_progress(), &cancel)
        };
        assert!(result.is_err());
        assert!(
            scratch.exists(),
            "run does not tear the scratch directory down; only dispose does"
        );
        session.dispose();
        assert!(
            wait_until(|| !scratch.exists()),
            "cancelled={cancelled}: dispose removes the scratch directory"
        );
        assert!(root.path().exists(), "the injected parent itself stays");
    }
}

#[test]
fn factory_builds_one_decoder_per_session_and_boxes_a_disposable_session() {
    let root = TempRoot::new("st_bp_factory");
    let decoders = Arc::new(AtomicUsize::new(0));
    let factory = production_session_factory(deps(Some(root.path().to_path_buf()), &decoders));
    let profile = first_profile();
    let mut first = factory(&profile, &[]).unwrap_or_else(|e| panic!("first session: {e}"));
    assert_eq!(decoders.load(Ordering::SeqCst), 1, "a fresh decoder per session");
    let mut second = factory(&profile, &[]).unwrap_or_else(|e| panic!("second session: {e}"));
    assert_eq!(decoders.load(Ordering::SeqCst), 2);
    first.dispose();
    first.dispose();
    second.dispose();
}

#[test]
fn factory_maps_an_executor_construction_failure_to_an_operation_error() {
    // A blank file path makes the executor's catalog build fail; the factory reports it as `BenchmarkError::Operation`
    // (the presenter ends the run with a failed status), and a decoder was still requested exactly once.
    let root = TempRoot::new("st_bp_factory_failure");
    let decoders = Arc::new(AtomicUsize::new(0));
    let factory = production_session_factory(deps(Some(root.path().to_path_buf()), &decoders));
    match factory(&first_profile(), &[String::new()]) {
        Err(BenchmarkError::Operation(error)) => assert!(!error.to_string().is_empty()),
        Err(other) => panic!("expected Operation, got {other:?}"),
        Ok(_) => panic!("a blank path must not build a session"),
    }
    assert_eq!(decoders.load(Ordering::SeqCst), 1);
}
