//! F tests for `pr_app::presenters::benchmark` (W8-10i): the Benchmark window's behaviour without a window.
//!
//! Port of `tests/PhotoReview.App.Tests/Presenters/BenchmarkPresenterTests.cs` (baseline `d92d6aae`). Every test names its C# twin in a
//! `// C#:` comment; tests with no C# twin are marked `// New test`. D-W8-2: the C# `async` run is a step machine; the harness drives
//! the jobs inline (`advance` / `drive`) or, where a test must act while a profile is running, on a worker thread. The C# queued UI
//! scheduler becomes the `queue` of posted progress messages that `pump` feeds to `on_progress`. No sleeps: tests wait for the exact state
//! they need with a bounded spin on `yield_now` (C# `Bound`, 20 s).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pr_app::presenters::benchmark::{
    BenchmarkFileScan, BenchmarkPresenter, BenchmarkProfileJob, BenchmarkProfileOutcome, BenchmarkProfileSession,
    BenchmarkProgressMessage, BenchmarkSaveJob, BenchmarkScanError, BenchmarkScanJob, BenchmarkScanOutcome,
    BenchmarkSessionFactory, BenchmarkStep, BenchmarkSummarySave, BenchmarkView, DEFAULT_IMAGE_LIMIT,
    QUICK_CHECK_ITERATIONS, QUICK_CHECK_PROFILE_ID, default_report_root, enumerate_image_files, report_file_name,
    save_session,
};
use pr_app::presenters::benchmark_presentation::{
    BEST_SYMBOL, BenchmarkResultRow, KEY_RESULT_CORRECTNESS_FAILED, KEY_RESULT_FAILED_DETAIL, KEY_STATUS_FAIL,
    KEY_STATUS_INSUFFICIENT_DATA, KEY_STATUS_PASS, KEY_STATUS_WARN, RowError, describe_error, format_milliseconds_f0,
    loading_mode_text, profile_name, progress_message, workload_text,
};
use pr_app::presenters::settings_field_map::FieldContext;
use pr_bench::engine::{
    BenchmarkError, BenchmarkProgress, BenchmarkProgressSink, PROGRESS_CORRECTNESS_FAILED, PROGRESS_OK,
    RESULT_CORRECTNESS_FAILED,
};
use pr_bench::models::{
    BenchmarkPhaseResult, BenchmarkProfile, BenchmarkProfileError, BenchmarkProfileProblem, BenchmarkReport,
    BenchmarkResultStatus, BenchmarkTimestamp, BenchmarkWorkload,
};
use pr_bench::profiles;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};
use pr_l10n::template::{LocArg, NumberCulture};
use pr_test_support::TempRoot;

/// C# `Bound`.
const BOUND: Duration = Duration::from_secs(20);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Spins (yielding) until `condition` holds; fails the test after [`BOUND`]. No sleep.
fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + BOUND;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::yield_now();
    }
}

fn flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

// ---- Fakes ----

/// C# `FakeView`: records every call (`calls` keeps the order across methods, trap T6).
#[derive(Default)]
struct FakeView {
    status: Option<String>,
    progress: f64,
    running: bool,
    refreshes: usize,
    running_history: Vec<bool>,
    statuses: Vec<String>,
    explorer: Vec<String>,
    calls: Vec<String>,
}

impl BenchmarkView for FakeView {
    fn set_status(&mut self, text: &str) {
        self.status = Some(text.to_owned());
        self.statuses.push(text.to_owned());
        self.calls.push("status".to_owned());
    }
    fn set_progress(&mut self, fraction: f64) {
        self.progress = fraction;
        self.calls.push(format!("progress:{fraction}"));
    }
    fn set_running(&mut self, running: bool) {
        self.running = running;
        self.running_history.push(running);
        self.calls.push(format!("running:{running}"));
    }
    fn refresh_rows(&mut self) {
        self.refreshes += 1;
        self.calls.push("refresh".to_owned());
    }
    fn open_in_explorer(&mut self, arguments: &str) {
        self.explorer.push(arguments.to_owned());
    }
}

type Behavior =
    Arc<dyn Fn(&dyn BenchmarkProgressSink, &CancelToken) -> Result<BenchmarkReport, BenchmarkError> + Send + Sync>;
type ScanFn = Arc<dyn Fn(&str, i32, &CancelToken) -> Result<Vec<String>, BenchmarkScanError> + Send + Sync>;
type Hook = Arc<dyn Fn(&BenchmarkProfile) -> Result<(), BenchmarkError> + Send + Sync>;

/// What a fake session records (C# `FakeSession.Disposed` / `Token`).
struct SessionState {
    disposed: AtomicUsize,
    token: Mutex<Option<CancelToken>>,
}

struct FakeSession {
    behavior: Behavior,
    state: Arc<SessionState>,
}

impl BenchmarkProfileSession for FakeSession {
    fn run(
        &mut self,
        _folder: &str,
        progress: &dyn BenchmarkProgressSink,
        cancel: &CancelToken,
    ) -> Result<BenchmarkReport, BenchmarkError> {
        *lock(&self.state.token) = Some(cancel.clone());
        (self.behavior)(progress, cancel)
    }

    fn dispose(&mut self) {
        self.state.disposed.fetch_add(1, Ordering::SeqCst);
    }
}

/// Everything the injected seams (factory, scan, save) of one harness share with the test.
struct Shared {
    behaviors: Mutex<HashMap<String, Behavior>>,
    sessions: Mutex<Vec<Arc<SessionState>>>,
    saved: Mutex<Vec<Vec<BenchmarkReport>>>,
    scan: Mutex<ScanFn>,
    hook: Mutex<Option<Hook>>,
    summary_path: Mutex<String>,
    save_error: Mutex<Option<String>>,
    /// The collecting `post` of the profile jobs (C# `QueueUi`).
    queue: Mutex<Vec<BenchmarkProgressMessage>>,
}

fn report(profile_id: &str, samples: &[f64], workload: BenchmarkWorkload) -> BenchmarkReport {
    BenchmarkReport {
        run_id: format!("run-{profile_id}"),
        started_utc: BenchmarkTimestamp::unix_epoch(),
        folder: "bench".to_owned(),
        phases: vec![BenchmarkPhaseResult::new(
            profile_id,
            workload,
            samples.to_vec(),
            BenchmarkResultStatus::Pass,
            None,
            None,
        )],
        metrics: None,
        machine: None,
    }
}

fn ok_behavior(id: &str, samples: &[f64], workload: BenchmarkWorkload) -> Behavior {
    let (id, samples) = (id.to_owned(), samples.to_vec());
    Arc::new(move |_, _| Ok(report(&id, &samples, workload)))
}

/// C# `UntilCancelled`: ends with `Canceled` as soon as the token is cancelled.
fn until_cancelled(cancel: &CancelToken) -> Result<BenchmarkReport, BenchmarkError> {
    let deadline = Instant::now() + BOUND;
    while !cancel.is_canceled() {
        if Instant::now() >= deadline {
            return Err(BenchmarkError::Operation("never cancelled".into()));
        }
        std::thread::yield_now();
    }
    Err(BenchmarkError::Canceled)
}

fn operation_error(message: &str) -> BenchmarkError {
    BenchmarkError::Operation(message.to_owned().into())
}

fn progress_of(id: &str, workload: BenchmarkWorkload, completed: i32, total: i32, message: &str) -> BenchmarkProgress {
    BenchmarkProgress {
        profile_id: id.to_owned(),
        workload,
        completed,
        total,
        message: message.to_owned(),
    }
}

fn registry(id: &str) -> BenchmarkProfile {
    profiles::find(id)
        .unwrap_or_else(|| panic!("registry has no profile '{id}'"))
        .clone()
}

fn vietnamese() -> Localizer {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("languages")
        .join("vi.json");
    let text = std::fs::read_to_string(&path).expect("assets/languages/vi.json");
    let mut warnings = Vec::new();
    let vi = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings).expect("valid vi.json");
    Localizer::create(built_in_english(), &[vi], &[])
}

struct Harness {
    view: FakeView,
    presenter: BenchmarkPresenter,
    shared: Arc<Shared>,
    texts: Arc<Localizer>,
    numbers: NumberCulture,
}

fn expect_profile(step: BenchmarkStep) -> BenchmarkProfileJob {
    match step {
        BenchmarkStep::Profile(job) => job,
        other => panic!("expected a profile step, got {other:?}"),
    }
}

fn expect_scan(step: BenchmarkStep) -> BenchmarkScanJob {
    match step {
        BenchmarkStep::Scan(job) => job,
        other => panic!("expected a scan step, got {other:?}"),
    }
}

fn expect_save(step: BenchmarkStep) -> BenchmarkSaveJob {
    match step {
        BenchmarkStep::Save(job) => job,
        other => panic!("expected a save step, got {other:?}"),
    }
}

fn expect_idle(step: &BenchmarkStep) {
    assert!(matches!(step, BenchmarkStep::Idle), "expected Idle, got {step:?}");
}

impl Harness {
    fn new() -> Self {
        Self::with_texts(built_in_english_localizer())
    }

    fn with_texts(texts: Arc<Localizer>) -> Self {
        let default_scan: ScanFn = Arc::new(|_, _, _| Ok(vec!["a.jpg".to_owned(), "b.jpg".to_owned()]));
        let shared = Arc::new(Shared {
            behaviors: Mutex::new(HashMap::new()),
            sessions: Mutex::new(Vec::new()),
            saved: Mutex::new(Vec::new()),
            scan: Mutex::new(default_scan),
            hook: Mutex::new(None),
            summary_path: Mutex::new("bench-fake-summary/summary.json".to_owned()),
            save_error: Mutex::new(None),
            queue: Mutex::new(Vec::new()),
        });
        let factory_shared = Arc::clone(&shared);
        let factory: BenchmarkSessionFactory = Arc::new(move |profile, _files| {
            let hook = lock(&factory_shared.hook).clone();
            if let Some(hook) = hook {
                hook(profile)?;
            }
            let behavior = lock(&factory_shared.behaviors)
                .get(&profile.id)
                .cloned()
                .unwrap_or_else(|| ok_behavior(&profile.id, &[10.0, 20.0, 30.0], BenchmarkWorkload::Sequential));
            let state = Arc::new(SessionState {
                disposed: AtomicUsize::new(0),
                token: Mutex::new(None),
            });
            lock(&factory_shared.sessions).push(Arc::clone(&state));
            Ok(Box::new(FakeSession { behavior, state }) as Box<dyn BenchmarkProfileSession>)
        });
        let scan_shared = Arc::clone(&shared);
        let scan: BenchmarkFileScan = Arc::new(move |folder, limit, cancel| {
            let scan = lock(&scan_shared.scan).clone();
            scan(folder, limit, cancel)
        });
        let save_shared = Arc::clone(&shared);
        let save: BenchmarkSummarySave = Arc::new(move |reports| {
            if let Some(error) = lock(&save_shared.save_error).clone() {
                return Err(error);
            }
            lock(&save_shared.saved).push(reports.to_vec());
            Ok(lock(&save_shared.summary_path).clone())
        });
        Self {
            view: FakeView::default(),
            presenter: BenchmarkPresenter::new(factory, Some(scan), Some(save)),
            shared,
            texts,
            numbers: NumberCulture::invariant(),
        }
    }

    fn set_behavior(
        &self,
        id: &str,
        behavior: impl Fn(&dyn BenchmarkProgressSink, &CancelToken) -> Result<BenchmarkReport, BenchmarkError>
        + Send
        + Sync
        + 'static,
    ) {
        lock(&self.shared.behaviors).insert(id.to_owned(), Arc::new(behavior));
    }

    fn set_ok(&self, id: &str, samples: &[f64], workload: BenchmarkWorkload) {
        lock(&self.shared.behaviors).insert(id.to_owned(), ok_behavior(id, samples, workload));
    }

    fn set_scan(
        &self,
        scan: impl Fn(&str, i32, &CancelToken) -> Result<Vec<String>, BenchmarkScanError> + Send + Sync + 'static,
    ) {
        *lock(&self.shared.scan) = Arc::new(scan);
    }

    fn sessions(&self) -> Vec<Arc<SessionState>> {
        lock(&self.shared.sessions).clone()
    }

    fn saved_count(&self) -> usize {
        lock(&self.shared.saved).len()
    }

    fn summary_path(&self) -> String {
        lock(&self.shared.summary_path).clone()
    }

    fn profiles(ids: &[&str]) -> Vec<BenchmarkProfile> {
        ids.iter().map(|id| registry(id)).collect()
    }

    fn run_selected(&mut self, ids: &[&str], folder: &str, limit: Option<&str>) -> BenchmarkStep {
        let chosen = Self::profiles(ids);
        let ctx = FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        };
        self.presenter
            .run_selected(&chosen, folder, limit, &mut self.view, &ctx)
    }

    fn quick_check(&mut self, folder: &str, limit: Option<&str>) -> BenchmarkStep {
        let ctx = FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        };
        self.presenter.quick_check(folder, limit, &mut self.view, &ctx)
    }

    fn on_scan_finished(&mut self, outcome: BenchmarkScanOutcome) -> BenchmarkStep {
        let ctx = FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        };
        self.presenter.on_scan_finished(outcome, &mut self.view, &ctx)
    }

    fn on_profile_finished(&mut self, outcome: BenchmarkProfileOutcome) -> BenchmarkStep {
        let ctx = FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        };
        self.presenter.on_profile_finished(outcome, &mut self.view, &ctx)
    }

    fn on_progress(&mut self, message: BenchmarkProgressMessage) {
        let ctx = FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        };
        self.presenter.on_progress(message, &mut self.view, &ctx);
    }

    /// C# `QueueUi.Pump`: applies the posted progress on the "UI thread".
    fn pump(&mut self) {
        for message in self.take_queue() {
            self.on_progress(message);
        }
    }

    fn take_queue(&self) -> Vec<BenchmarkProgressMessage> {
        std::mem::take(&mut *lock(&self.shared.queue))
    }

    fn post_target(&self) -> impl Fn(BenchmarkProgressMessage) + Send + Sync + 'static {
        let shared = Arc::clone(&self.shared);
        move |message| lock(&shared.queue).push(message)
    }

    /// Runs one job inline (posting progress into the queue, pumped before the profile outcome like a FIFO scheduler) and feeds its outcome back.
    fn advance(&mut self, step: BenchmarkStep) -> BenchmarkStep {
        match step {
            BenchmarkStep::Idle => BenchmarkStep::Idle,
            BenchmarkStep::Scan(job) => {
                let outcome = job.run();
                self.on_scan_finished(outcome)
            }
            BenchmarkStep::Profile(job) => {
                let post = self.post_target();
                let outcome = job.run(&post);
                self.pump();
                self.on_profile_finished(outcome)
            }
            BenchmarkStep::Save(job) => {
                let result = job.run();
                let ctx = FieldContext {
                    texts: &self.texts,
                    numbers: &self.numbers,
                };
                self.presenter.on_save_finished(result, &mut self.view, &ctx)
            }
        }
    }

    fn drive(&mut self, mut step: BenchmarkStep) {
        while !matches!(step, BenchmarkStep::Idle) {
            step = self.advance(step);
        }
    }

    /// Starts a run and drives it to the end, inline.
    fn run_to_end(&mut self, ids: &[&str], folder: &str, limit: Option<&str>) {
        let step = self.run_selected(ids, folder, limit);
        self.drive(step);
    }

    /// Starts a run and drives it until the first profile job is out (scan done), returning that job.
    fn start_until_profile(&mut self, ids: &[&str]) -> BenchmarkProfileJob {
        let step = self.run_selected(ids, "bench", Some(""));
        let step = self.advance(step);
        expect_profile(step)
    }

    fn spawn_profile(&self, job: BenchmarkProfileJob) -> JoinHandle<BenchmarkProfileOutcome> {
        let post = self.post_target();
        std::thread::spawn(move || job.run(&post))
    }

    fn spawn_scan(job: BenchmarkScanJob) -> JoinHandle<BenchmarkScanOutcome> {
        std::thread::spawn(move || job.run())
    }

    fn finish_profile(&mut self, handle: JoinHandle<BenchmarkProfileOutcome>) -> BenchmarkStep {
        let outcome = handle.join().expect("profile worker panicked");
        self.on_profile_finished(outcome)
    }

    fn row_profile_ids(&self) -> Vec<String> {
        self.presenter.rows().iter().map(|r| r.profile.id.clone()).collect()
    }

    fn progress_status(&self, index: i64, count: i64, id: &str, completed: i64, total: i64, message: &str) -> String {
        self.texts.format_with(
            "benchmark.status.progress",
            &[
                LocArg::new("profileIndex", index),
                LocArg::new("profileCount", count),
                LocArg::new("profileId", id),
                LocArg::new("completed", completed),
                LocArg::new("total", total),
                LocArg::new("message", message),
            ],
            &self.numbers,
        )
    }

    fn done_status(&self, completed: i64, total: i64, path: &str) -> String {
        self.texts.format_with(
            "benchmark.status.done",
            &[
                LocArg::new("completed", completed),
                LocArg::new("total", total),
                LocArg::new("reportPath", path),
            ],
            &self.numbers,
        )
    }

    fn cannot_read_status(&self, message: &str) -> String {
        self.texts.format_with(
            "benchmark.status.cannotReadFolder",
            &[LocArg::new("message", message)],
            &self.numbers,
        )
    }

    fn failed_status(&self, inner: &str) -> String {
        self.texts
            .format_with("bench.status.failed", &[LocArg::new("message", inner)], &self.numbers)
    }

    fn profile_failed_status(&self, profile: &BenchmarkProfile, message: &str) -> String {
        self.texts.format_with(
            "benchmark.status.profileFailed",
            &[
                LocArg::new("profileName", profile_name(profile, &self.texts)),
                LocArg::new("message", message),
            ],
            &self.numbers,
        )
    }

    fn csv(&self) -> String {
        let ctx = FieldContext {
            texts: &self.texts,
            numbers: &self.numbers,
        };
        self.presenter.results_to_csv(&ctx)
    }
}

// ---- Normal run ----

// C#: BenchmarkPresenterTests.NormalRun_ListsRowsInOrder_SavesOnce_EndsIdle
#[test]
fn normal_run_lists_rows_in_order_saves_once_ends_idle() {
    let mut h = Harness::new();
    h.set_behavior("fast-sequential", |progress, _| {
        progress.report(progress_of(
            "fast-sequential",
            BenchmarkWorkload::Sequential,
            1,
            4,
            PROGRESS_OK,
        ));
        Ok(report("fast-sequential", &[10.0, 20.0], BenchmarkWorkload::Sequential))
    });
    h.set_ok("preview-light", &[5.0, 6.0, 7.0], BenchmarkWorkload::Sequential);

    h.run_to_end(&["fast-sequential", "preview-light"], "bench", Some("5"));

    assert_eq!(h.view.running_history, [true, false]);
    // T6: busy first, then the rows are cleared and the bar reset.
    assert_eq!(&h.view.calls[..3], ["running:true", "refresh", "progress:0"]);
    assert!(!h.presenter.is_running());
    assert_eq!(h.row_profile_ids(), ["fast-sequential", "preview-light"]);
    assert_eq!(h.saved_count(), 1);
    assert_eq!(lock(&h.shared.saved)[0].len(), 2);
    assert_eq!(h.presenter.last_report_path(), Some(h.summary_path().as_str()));
    assert_eq!(h.view.status, Some(h.done_status(2, 2, &h.summary_path())));
    for session in h.sessions() {
        assert_eq!(session.disposed.load(Ordering::SeqCst), 1);
    }
}

// C#: BenchmarkPresenterTests.Progress_IsPostedThroughScheduler_AndFormatted
#[test]
fn progress_is_posted_through_scheduler_and_formatted() {
    let mut h = Harness::new();
    let (entered, release) = (flag(), flag());
    let (e, r) = (Arc::clone(&entered), Arc::clone(&release));
    h.set_behavior("fast-balanced", move |progress, _| {
        progress.report(progress_of(
            "fast-balanced",
            BenchmarkWorkload::WarmNext,
            3,
            4,
            PROGRESS_OK,
        ));
        e.store(true, Ordering::SeqCst);
        wait_until("release", || r.load(Ordering::SeqCst));
        Ok(report("fast-balanced", &[1.0], BenchmarkWorkload::Sequential))
    });

    let job = h.start_until_profile(&["fast-balanced"]);
    let handle = h.spawn_profile(job);
    wait_until("the session", || entered.load(Ordering::SeqCst));
    assert_eq!(h.view.progress, 0.0); // nothing applied until the UI thread runs on_progress
    h.pump();

    assert_eq!(h.view.progress, 0.75);
    let expected = h.progress_status(1, 1, "fast-balanced", 3, 4, &h.texts.get("benchmark.progress.ok"));
    assert_eq!(h.view.status, Some(expected));
    release.store(true, Ordering::SeqCst);
    let step = h.finish_profile(handle);
    h.drive(step);
}

// C#: BenchmarkPresenterTests.Progress_ZeroTotal_AndProfileIndex
#[test]
fn progress_zero_total_and_profile_index() {
    let mut h = Harness::new();
    h.set_behavior("fast-sequential", |progress, _| {
        progress.report(progress_of("fast-sequential", BenchmarkWorkload::Sequential, 0, 0, "x"));
        Ok(report("fast-sequential", &[1.0], BenchmarkWorkload::Sequential))
    });
    h.set_behavior("preview-light", |progress, _| {
        progress.report(progress_of("preview-light", BenchmarkWorkload::Sequential, 1, 2, "x"));
        Ok(report("preview-light", &[1.0], BenchmarkWorkload::Sequential))
    });

    let step = h.run_selected(&["fast-sequential", "preview-light"], "bench", None);
    let step = h.advance(step); // scan -> first profile
    // The first profile's progress is applied before its outcome (FIFO); capture the bar after it.
    let first_job = expect_profile(step);
    let post = h.post_target();
    let outcome = first_job.run(&post);
    h.pump();
    let first_bar = h.view.progress;
    let step = h.on_profile_finished(outcome);
    let second_job = expect_profile(step);
    let outcome = second_job.run(&post);
    h.pump();
    let (second_bar, second_status) = (h.view.progress, h.view.status.clone());
    let step = h.on_profile_finished(outcome);
    h.drive(step);

    assert_eq!(first_bar, 0.0);
    assert_eq!(second_bar, 0.5);
    assert_eq!(second_status, Some(h.progress_status(2, 2, "preview-light", 1, 2, "x")));
}

// C#: BenchmarkPresenterTests.Progress_MessageIsTranslated
#[test]
fn progress_message_is_translated() {
    let vi = Arc::new(vietnamese());
    let mut h = Harness::with_texts(Arc::clone(&vi));
    h.set_behavior("fast-sequential", |progress, _| {
        progress.report(progress_of(
            "fast-sequential",
            BenchmarkWorkload::Sequential,
            1,
            2,
            PROGRESS_CORRECTNESS_FAILED,
        ));
        Ok(report("fast-sequential", &[1.0], BenchmarkWorkload::Sequential))
    });

    let job = h.start_until_profile(&["fast-sequential"]);
    let post = h.post_target();
    let outcome = job.run(&post);
    h.pump();

    let translated = progress_message(PROGRESS_CORRECTNESS_FAILED, &vi);
    assert_ne!(translated, PROGRESS_CORRECTNESS_FAILED);
    assert_eq!(
        h.view.status,
        Some(h.progress_status(1, 1, "fast-sequential", 1, 2, &translated))
    );
    let step = h.on_profile_finished(outcome);
    h.drive(step);
}

// ---- Cancel ----

// C#: BenchmarkPresenterTests.Cancel_MidProfile_EndsCancelled
#[test]
fn cancel_mid_profile_ends_cancelled() {
    let mut h = Harness::new();
    let entered = flag();
    let e = Arc::clone(&entered);
    h.set_behavior("fast-sequential", move |_, cancel| {
        e.store(true, Ordering::SeqCst);
        until_cancelled(cancel)
    });

    let job = h.start_until_profile(&["fast-sequential"]);
    let handle = h.spawn_profile(job);
    wait_until("the session", || entered.load(Ordering::SeqCst));
    assert!(h.presenter.is_running());
    h.presenter.cancel();
    let step = h.finish_profile(handle);

    expect_idle(&step);
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.canceled")));
    assert!(h.presenter.rows().is_empty());
    assert_eq!(h.saved_count(), 0);
    assert_eq!(h.view.running_history, [true, false]);
    let sessions = h.sessions();
    assert_eq!(sessions[0].disposed.load(Ordering::SeqCst), 1);
    assert!(lock(&sessions[0].token).as_ref().is_some_and(CancelToken::is_canceled));
}

// C#: BenchmarkPresenterTests.Cancel_DuringScan_NoSessionCreated
#[test]
fn cancel_during_scan_no_session_created() {
    let mut h = Harness::new();
    let scanning = flag();
    let s = Arc::clone(&scanning);
    h.set_scan(move |_, _, cancel| {
        s.store(true, Ordering::SeqCst);
        wait_until("the cancel", || cancel.is_canceled());
        cancel.check().map_err(|_| BenchmarkScanError::Canceled)?;
        Ok(vec!["a.jpg".to_owned()])
    });

    let step = h.run_selected(&["fast-sequential"], "bench", Some(""));
    let handle = Harness::spawn_scan(expect_scan(step));
    wait_until("the scan", || scanning.load(Ordering::SeqCst));
    h.presenter.cancel();
    let outcome = handle.join().expect("scan worker panicked");
    let step = h.on_scan_finished(outcome);

    expect_idle(&step);
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.canceled")));
    assert!(h.sessions().is_empty());
    assert_eq!(h.view.running_history, [true, false]);
}

// C#: BenchmarkPresenterTests.Cancel_WhenIdle_IsNoOp
#[test]
fn cancel_when_idle_is_no_op() {
    let mut h = Harness::new();
    h.presenter.cancel();
    h.presenter.close();
    h.presenter.close();

    assert!(h.view.running_history.is_empty());
    assert!(h.view.status.is_none());
}

// ---- Close (Dispose) ----

// C#: BenchmarkPresenterTests.Close_BetweenProfiles_EndsCancelled_NoFailRow
#[test]
fn close_between_profiles_ends_cancelled_no_fail_row() {
    let mut h = Harness::new();
    h.set_behavior("preview-light", |_, cancel| {
        cancel.check().map_err(|_| BenchmarkError::Canceled)?;
        Ok(report("preview-light", &[1.0], BenchmarkWorkload::Sequential))
    });

    let first = h.start_until_profile(&["fast-sequential", "preview-light"]);
    let post = h.post_target();
    let outcome = first.run(&post);
    // The C# collection event closes right after the first row was added; the next job is out already.
    let second = expect_profile(h.on_profile_finished(outcome));
    assert_eq!(h.presenter.rows().len(), 1);
    h.presenter.close();
    let outcome = second.run(&post);
    let step = h.on_profile_finished(outcome);

    expect_idle(&step);
    assert_eq!(h.presenter.rows().len(), 1);
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.canceled")));
    assert_eq!(h.saved_count(), 0);
    assert_eq!(h.view.running_history, [true, false]);
    for session in h.sessions() {
        assert_eq!(session.disposed.load(Ordering::SeqCst), 1);
    }
}

// C#: BenchmarkPresenterTests.Close_DuringProfile_CancelsToken
#[test]
fn close_during_profile_cancels_token() {
    let mut h = Harness::new();
    let entered = flag();
    let e = Arc::clone(&entered);
    h.set_behavior("fast-sequential", move |_, cancel| {
        e.store(true, Ordering::SeqCst);
        until_cancelled(cancel)
    });

    let job = h.start_until_profile(&["fast-sequential"]);
    let handle = h.spawn_profile(job);
    wait_until("the session", || entered.load(Ordering::SeqCst));
    h.presenter.close();
    let step = h.finish_profile(handle);

    expect_idle(&step);
    let sessions = h.sessions();
    assert!(lock(&sessions[0].token).as_ref().is_some_and(CancelToken::is_canceled));
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.canceled")));
    assert!(!h.presenter.is_running());
    assert_eq!(sessions[0].disposed.load(Ordering::SeqCst), 1);
}

// C#: BenchmarkPresenterTests.Close_DuringScan_NoSession
#[test]
fn close_during_scan_no_session() {
    let mut h = Harness::new();
    let scanning = flag();
    let s = Arc::clone(&scanning);
    h.set_scan(move |_, _, cancel| {
        s.store(true, Ordering::SeqCst);
        wait_until("the cancel", || cancel.is_canceled());
        cancel.check().map_err(|_| BenchmarkScanError::Canceled)?;
        Ok(vec!["a.jpg".to_owned()])
    });

    let step = h.run_selected(&["fast-sequential"], "bench", Some(""));
    let handle = Harness::spawn_scan(expect_scan(step));
    wait_until("the scan", || scanning.load(Ordering::SeqCst));
    h.presenter.close();
    let outcome = handle.join().expect("scan worker panicked");
    let step = h.on_scan_finished(outcome);

    expect_idle(&step);
    assert!(h.sessions().is_empty());
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.canceled")));
}

// ---- One profile fails ----

// C#: BenchmarkPresenterTests.FailingProfile_BecomesFailRow_NextStillRuns
#[test]
fn failing_profile_becomes_fail_row_next_still_runs() {
    let mut h = Harness::new();
    h.set_behavior("fast-sequential", |_, _| Err(operation_error("boom")));

    h.run_to_end(&["fast-sequential", "preview-light"], "bench", Some(""));

    let rows = h.presenter.rows();
    assert_eq!(rows.len(), 2);
    let fail = &rows[0];
    assert_eq!(fail.status(), BenchmarkResultStatus::Fail);
    assert_eq!(fail.count(), 0);
    assert_eq!(fail.p95_text(&h.texts), h.texts.get("bench.noValue"));
    let error = fail.error.clone().expect("fail row carries its error");
    let described = describe_error(&error, &h.texts);
    assert_eq!(
        fail.status_text(&h.texts),
        h.texts
            .format(KEY_RESULT_FAILED_DETAIL, &[LocArg::new("message", described.clone())])
    );
    assert_eq!(rows[1].status(), BenchmarkResultStatus::Pass);
    assert!(
        h.view
            .statuses
            .contains(&h.profile_failed_status(&registry("fast-sequential"), &described))
    );
    assert_eq!(h.view.status, Some(h.done_status(1, 2, &h.summary_path())));
    let sessions = h.sessions();
    assert_eq!(sessions.len(), 2);
    for session in sessions {
        assert_eq!(session.disposed.load(Ordering::SeqCst), 1);
    }
}

// C#: BenchmarkPresenterTests.AllProfilesFail_NothingSaved
#[test]
fn all_profiles_fail_nothing_saved() {
    let mut h = Harness::new();
    h.set_behavior("fast-sequential", |_, _| Err(operation_error("boom")));

    h.run_to_end(&["fast-sequential"], "bench", Some(""));

    assert_eq!(h.saved_count(), 0);
    assert_eq!(h.presenter.last_report_path(), None);
    assert_eq!(h.presenter.rows().len(), 1);
    let row = &h.presenter.rows()[0];
    assert_eq!(row.status(), BenchmarkResultStatus::Fail);
    let described = describe_error(row.error.as_ref().expect("error"), &h.texts);
    assert_eq!(
        h.view.status,
        Some(h.profile_failed_status(&registry("fast-sequential"), &described))
    );
    assert_eq!(h.view.running_history, [true, false]);
}

// C#: BenchmarkPresenterTests.FactoryThrows_EndsRunFailed
#[test]
fn factory_throws_ends_run_failed() {
    let mut h = Harness::new();
    *lock(&h.shared.hook) = Some(Arc::new(|profile| {
        if profile.id != "cache-recovery" {
            return Ok(());
        }
        Err(BenchmarkError::Profile(BenchmarkProfileError::new(
            BenchmarkProfileProblem::NotImplemented,
            "cache-recovery",
            "nope",
        )))
    }));

    h.run_to_end(&["cache-recovery", "fast-sequential"], "bench", Some(""));

    let error = RowError::Profile(BenchmarkProfileError::new(
        BenchmarkProfileProblem::NotImplemented,
        "cache-recovery",
        "nope",
    ));
    assert_eq!(h.view.status, Some(h.failed_status(&describe_error(&error, &h.texts))));
    assert!(h.presenter.rows().is_empty());
    assert!(h.sessions().is_empty()); // the next profile is never started after the factory failed
    assert_eq!(h.view.running_history, [true, false]);
}

// ---- Two Starts in a row ----

// C#: BenchmarkPresenterTests.SecondStart_WhileRunning_IsIgnored
#[test]
fn second_start_while_running_is_ignored() {
    let mut h = Harness::new();
    let (entered, release) = (flag(), flag());
    let (e, r) = (Arc::clone(&entered), Arc::clone(&release));
    h.set_behavior("fast-sequential", move |_, _| {
        e.store(true, Ordering::SeqCst);
        wait_until("release", || r.load(Ordering::SeqCst));
        Ok(report("fast-sequential", &[1.0, 2.0], BenchmarkWorkload::Sequential))
    });

    let job = h.start_until_profile(&["fast-sequential"]);
    let handle = h.spawn_profile(job);
    wait_until("the session", || entered.load(Ordering::SeqCst));
    let second = h.run_selected(&["preview-light"], "other", Some("1"));
    let quick = h.quick_check("other", Some("1"));
    let empty = h.run_selected(&[], "bench", Some(""));
    expect_idle(&second);
    expect_idle(&quick);
    expect_idle(&empty); // the running guard is checked before the empty-selection status

    assert_eq!(h.sessions().len(), 1);
    assert_eq!(h.view.running_history, [true]);
    assert!(
        !h.view
            .statuses
            .contains(&h.texts.get("benchmark.status.noProfileSelected"))
    );
    release.store(true, Ordering::SeqCst);
    let step = h.finish_profile(handle);
    h.drive(step);
    assert_eq!(h.view.running_history, [true, false]);
    assert_eq!(h.row_profile_ids(), ["fast-sequential"]);

    h.run_to_end(&["preview-light"], "bench", Some(""));
    assert_eq!(h.view.running_history, [true, false, true, false]);
    assert_eq!(h.row_profile_ids(), ["preview-light"]); // rows cleared at the start
}

// ---- Late results ----

// C#: BenchmarkPresenterTests.LateProgress_AfterCancel_IsDropped
#[test]
fn late_progress_after_cancel_is_dropped() {
    let mut h = Harness::new();
    let entered = flag();
    let e = Arc::clone(&entered);
    h.set_behavior("fast-sequential", move |progress, cancel| {
        progress.report(progress_of(
            "fast-sequential",
            BenchmarkWorkload::Sequential,
            3,
            4,
            "late",
        ));
        e.store(true, Ordering::SeqCst);
        until_cancelled(cancel)
    });

    let job = h.start_until_profile(&["fast-sequential"]);
    let handle = h.spawn_profile(job);
    wait_until("the session", || entered.load(Ordering::SeqCst));
    h.presenter.cancel();
    let step = h.finish_profile(handle);
    expect_idle(&step);
    let status_after = h.view.status.clone();

    // The message the worker posted before the run ended arrives only now.
    let late = h.take_queue();
    assert_eq!(late.len(), 1);
    for message in late {
        h.on_progress(message);
    }

    assert_eq!(status_after, Some(h.texts.get("benchmark.status.canceled")));
    assert_eq!(h.view.status, status_after);
    assert_eq!(h.view.progress, 0.0);
}

// C#: BenchmarkPresenterTests.LateProgress_FromEarlierRun_IsDroppedDuringNewRun
#[test]
fn late_progress_from_earlier_run_is_dropped_during_new_run() {
    let mut h = Harness::new();
    let first_entered = flag();
    let e = Arc::clone(&first_entered);
    h.set_behavior("fast-sequential", move |progress, cancel| {
        progress.report(progress_of(
            "fast-sequential",
            BenchmarkWorkload::Sequential,
            1,
            4,
            "stale",
        ));
        e.store(true, Ordering::SeqCst);
        until_cancelled(cancel)
    });
    let job = h.start_until_profile(&["fast-sequential"]);
    let handle = h.spawn_profile(job);
    wait_until("the first session", || first_entered.load(Ordering::SeqCst));
    h.presenter.cancel();
    expect_idle(&h.finish_profile(handle));
    let stale = h.take_queue();
    assert_eq!(stale.len(), 1);

    let (second_entered, release) = (flag(), flag());
    let (e, r) = (Arc::clone(&second_entered), Arc::clone(&release));
    h.set_behavior("preview-light", move |_, _| {
        e.store(true, Ordering::SeqCst);
        wait_until("release", || r.load(Ordering::SeqCst));
        Ok(report("preview-light", &[1.0], BenchmarkWorkload::Sequential))
    });
    let job = h.start_until_profile(&["preview-light"]);
    let handle = h.spawn_profile(job);
    wait_until("the second session", || second_entered.load(Ordering::SeqCst));
    let before = h.view.status.clone();
    for message in stale {
        h.on_progress(message);
    }

    assert_eq!(h.view.status, before);
    assert_eq!(h.view.progress, 0.0);
    release.store(true, Ordering::SeqCst);
    let step = h.finish_profile(handle);
    h.drive(step);
}

// C#: BenchmarkPresenterTests.ReportCompletingAfterCancel_IsKeptAsRow
#[test]
fn report_completing_after_cancel_is_kept_as_row() {
    let mut h = Harness::new();
    let (entered, release) = (flag(), flag());
    let (e, r) = (Arc::clone(&entered), Arc::clone(&release));
    h.set_behavior("fast-sequential", move |_, _| {
        e.store(true, Ordering::SeqCst);
        wait_until("release", || r.load(Ordering::SeqCst)); // ignores the token and finishes anyway
        Ok(report("fast-sequential", &[4.0, 5.0], BenchmarkWorkload::Sequential))
    });

    let job = h.start_until_profile(&["fast-sequential"]);
    let handle = h.spawn_profile(job);
    wait_until("the session", || entered.load(Ordering::SeqCst));
    h.presenter.cancel();
    release.store(true, Ordering::SeqCst);
    let step = h.finish_profile(handle);
    h.drive(step);

    assert_eq!(h.presenter.rows().len(), 1);
    assert_eq!(h.view.running_history, [true, false]);
    assert!(!h.presenter.is_running());
}

// ---- Start guards and folder problems ----

// C#: BenchmarkPresenterTests.NoSelection_ReportsStatus
#[test]
fn no_selection_reports_status() {
    let mut h = Harness::new();

    let step = h.run_selected(&[], "bench", Some(""));

    expect_idle(&step);
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.noProfileSelected")));
    assert!(h.view.running_history.is_empty());
    assert!(h.sessions().is_empty());
}

// C#: BenchmarkPresenterTests.FolderProblems_StatusAndIdle
#[test]
fn folder_problems_status_and_idle() {
    let mut h = Harness::new();

    h.set_scan(|_, _, _| Err(BenchmarkScanError::DirectoryNotFound));
    h.run_to_end(&["fast-sequential"], "gone", Some(""));
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.folderMissing")));

    h.set_scan(|_, _, _| Err(BenchmarkScanError::Io("share offline".to_owned())));
    h.run_to_end(&["fast-sequential"], "bench", Some(""));
    assert_eq!(h.view.status, Some(h.cannot_read_status("share offline")));

    h.set_scan(|_, _, _| Err(BenchmarkScanError::Io("denied".to_owned())));
    h.run_to_end(&["fast-sequential"], "bench", Some(""));
    assert_eq!(h.view.status, Some(h.cannot_read_status("denied")));

    h.set_scan(|_, _, _| Ok(Vec::new()));
    h.run_to_end(&["fast-sequential"], "bench", Some(""));
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.noImages")));

    assert_eq!(
        h.view.running_history,
        [true, false, true, false, true, false, true, false]
    );
    assert!(h.sessions().is_empty());
    assert_eq!(h.saved_count(), 0);
}

// C#: BenchmarkPresenterTests.UnexpectedScanError_Failed
#[test]
fn unexpected_scan_error_failed() {
    let mut h = Harness::new();
    h.set_scan(|_, _, _| Err(BenchmarkScanError::Other("weird".to_owned())));

    h.run_to_end(&["fast-sequential"], "bench", Some(""));

    let inner = describe_error(&RowError::Other("weird".to_owned()), &h.texts);
    assert_eq!(h.view.status, Some(h.failed_status(&inner)));
    assert_eq!(inner, "Unexpected error: weird");
    assert!(!h.view.running);
}

// C#: BenchmarkPresenterTests.ImageLimit_IsPassedToScan
#[test]
fn image_limit_is_passed_to_scan() {
    let mut h = Harness::new();
    let limits = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&limits);
    h.set_scan(move |_, limit, _| {
        lock(&seen).push(limit);
        Ok(vec!["a.jpg".to_owned()])
    });

    for text in [Some("5"), Some(""), Some("abc"), Some("-3"), Some("0"), None] {
        h.run_to_end(&["fast-sequential"], "bench", text);
    }

    assert_eq!(*lock(&limits), [5, 0, 0, 0, 0, 0]);
}

// C#: BenchmarkPresenterTests.QuickCheck_RunsFastSequentialWithTenIterations
#[test]
fn quick_check_runs_fast_sequential_with_ten_iterations() {
    let mut h = Harness::new();
    let seen_profile: Arc<Mutex<Option<BenchmarkProfile>>> = Arc::new(Mutex::new(None));
    let seen_folder: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let (p, f) = (Arc::clone(&seen_profile), Arc::clone(&seen_folder));
    *lock(&h.shared.hook) = Some(Arc::new(move |profile| {
        *lock(&p) = Some(profile.clone());
        Ok(())
    }));
    h.set_scan(move |folder, _, _| {
        *lock(&f) = Some(folder.to_owned());
        Ok(vec!["a.jpg".to_owned()])
    });

    let step = h.quick_check("pics", Some("7"));
    h.drive(step);

    assert_eq!(lock(&seen_folder).as_deref(), Some("pics"));
    let profile = lock(&seen_profile).clone().expect("factory saw a profile");
    assert_eq!(profile.id, "fast-sequential");
    assert_eq!(profile.id, QUICK_CHECK_PROFILE_ID);
    assert_eq!(profile.iterations, 10);
    assert_eq!(profile.iterations, QUICK_CHECK_ITERATIONS);
    assert_eq!(profile.warmup_count, registry("fast-sequential").warmup_count);
    assert_eq!(h.presenter.rows().len(), 1);
}

// ---- Ranking / rows ----

// C#: BenchmarkPresenterTests.BestMarker_PerWorkloadGroup
#[test]
fn best_marker_per_workload_group() {
    let mut h = Harness::new();
    h.set_ok("fast-sequential", &[10.0, 12.0], BenchmarkWorkload::Sequential);
    h.set_ok("preview-light", &[3.0, 4.0], BenchmarkWorkload::Sequential);
    h.set_ok("instant-review", &[50.0, 60.0], BenchmarkWorkload::FirstFrame);

    h.run_to_end(
        &["fast-sequential", "preview-light", "instant-review"],
        "bench",
        Some(""),
    );

    let rows = h.presenter.rows();
    let markers: Vec<&str> = rows.iter().map(BenchmarkResultRow::best_marker).collect();
    assert_eq!(markers, ["", BEST_SYMBOL, BEST_SYMBOL]);
    let best: Vec<bool> = rows.iter().map(|r| r.is_best).collect();
    assert_eq!(best, [false, true, true]);
    assert!(h.view.refreshes >= 3); // the grid is repainted after every recompute
}

// C#: BenchmarkPresenterTests.BestMarker_MovesWhenBeaten
#[test]
fn best_marker_moves_when_beaten() {
    let mut h = Harness::new();
    h.set_ok("fast-sequential", &[10.0], BenchmarkWorkload::Sequential);
    h.set_ok("preview-light", &[20.0], BenchmarkWorkload::Sequential);
    h.set_ok("no-preload-baseline", &[1.0], BenchmarkWorkload::Sequential);

    h.run_to_end(
        &["fast-sequential", "preview-light", "no-preload-baseline"],
        "bench",
        Some(""),
    );

    let best: Vec<bool> = h.presenter.rows().iter().map(|r| r.is_best).collect();
    assert_eq!(best, [false, false, true]);
}

// C#: BenchmarkPresenterTests.Row_CellTexts_TenColumnsInOrder
#[test]
fn row_cell_texts_ten_columns_in_order() {
    let texts = built_in_english_localizer();
    let profile = registry("fast-sequential");
    let phase = BenchmarkPhaseResult::new(
        profile.id.clone(),
        BenchmarkWorkload::Sequential,
        vec![10.0, 20.0, 30.0, 40.0],
        BenchmarkResultStatus::Pass,
        None,
        None,
    );
    let mut row = BenchmarkResultRow::new(profile.clone(), phase, None);
    row.is_best = true;
    let ms = |value: f64| {
        texts.format(
            "unit.milliseconds",
            &[LocArg::new("value", format_milliseconds_f0(value))],
        )
    };

    let cells = row.cell_texts(&texts);

    assert_eq!(cells.len(), 10);
    assert_eq!(cells[0], BEST_SYMBOL);
    assert_eq!(cells[1], profile_name(&profile, &texts));
    assert_eq!(cells[2], loading_mode_text(profile.loading_mode, &texts));
    assert_eq!(cells[3], workload_text(BenchmarkWorkload::Sequential, &texts));
    assert_eq!(cells[4], "4");
    assert_eq!(cells[5], ms(row.phase.p50()));
    assert_eq!(cells[6], ms(row.phase.p95()));
    assert_eq!(cells[7], ms(row.phase.p99()));
    assert_eq!(cells[8], ms(40.0));
    assert_eq!(cells[9], texts.get(KEY_STATUS_PASS));

    let empty = BenchmarkResultRow::new(
        profile.clone(),
        BenchmarkPhaseResult::new(
            profile.id.clone(),
            BenchmarkWorkload::Sequential,
            Vec::new(),
            BenchmarkResultStatus::InsufficientData,
            None,
            None,
        ),
        None,
    );
    let empty_cells = empty.cell_texts(&texts);
    let no_value = texts.get("bench.noValue");
    assert_eq!(
        empty_cells[5..9],
        [no_value.clone(), no_value.clone(), no_value.clone(), no_value]
    );
    assert_eq!(empty_cells[0], "");
    assert_eq!(empty_cells[9], texts.get(KEY_STATUS_INSUFFICIENT_DATA));
}

// C#: BenchmarkPresenterTests.Row_StatusText_Branches
#[test]
fn row_status_text_branches() {
    let texts = built_in_english_localizer();
    let profile = registry("fast-sequential");
    let row = |status: BenchmarkResultStatus, message: Option<&str>| {
        BenchmarkResultRow::new(
            profile.clone(),
            BenchmarkPhaseResult::new(
                profile.id.clone(),
                BenchmarkWorkload::Sequential,
                vec![1.0],
                status,
                message.map(str::to_owned),
                None,
            ),
            None,
        )
    };

    assert_eq!(
        row(BenchmarkResultStatus::Warn, None).status_text(&texts),
        texts.get(KEY_STATUS_WARN)
    );
    assert_eq!(
        row(BenchmarkResultStatus::Fail, None).status_text(&texts),
        texts.get(KEY_STATUS_FAIL)
    );
    assert_eq!(
        row(BenchmarkResultStatus::Fail, Some(RESULT_CORRECTNESS_FAILED)).status_text(&texts),
        texts.get(KEY_RESULT_CORRECTNESS_FAILED)
    );
    assert_eq!(
        row(BenchmarkResultStatus::Fail, Some("disk")).status_text(&texts),
        texts.format(KEY_RESULT_FAILED_DETAIL, &[LocArg::new("message", "disk")])
    );
}

// ---- Export ----

// C#: BenchmarkPresenterTests.Csv_HeaderAndRows
#[test]
fn csv_header_and_rows() {
    let mut h = Harness::new();
    h.set_ok("fast-sequential", &[10.0, 20.0], BenchmarkWorkload::Sequential);
    h.set_behavior("preview-light", |_, _| Err(operation_error("a,\"b\"")));
    h.run_to_end(&["fast-sequential", "preview-light"], "bench", Some(""));

    let csv = h.csv();
    let lines: Vec<&str> = csv.split("\r\n").filter(|l| !l.is_empty()).collect();

    assert!(csv.ends_with("\r\n"));
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with(&format!("{BEST_SYMBOL},")));
    assert!(lines[0].contains(",P50,P95,P99,Max,"));
    assert!(lines[1].contains(&profile_name(&registry("fast-sequential"), &h.texts)));
    assert!(lines[2].contains("\"\"b\"\"")); // quotes doubled inside a quoted cell
}

// ---- Reports / Explorer ----

/// `Path.GetTempPath()` form: with a trailing backslash.
fn temp_path_with_separator() -> String {
    let mut text = std::env::temp_dir().to_string_lossy().into_owned();
    if !text.ends_with('\\') {
        text.push('\\');
    }
    text
}

// C#: BenchmarkPresenterTests.OpenReport_AndFolder
#[test]
fn open_report_and_folder() {
    let mut h = Harness::new();
    h.presenter.open_report(&mut h.view);
    h.presenter.open_result_folder(&mut h.view);
    assert_eq!(h.view.explorer, [temp_path_with_separator()]);

    let dir = TempRoot::new("bench-presenter");
    let summary = dir.join("summary.json").to_string_lossy().into_owned();
    let mut h2 = Harness::new();
    *lock(&h2.shared.summary_path) = summary.clone();
    h2.run_to_end(&["fast-sequential"], "bench", Some(""));

    h2.presenter.open_report(&mut h2.view); // the file does not exist yet
    assert!(h2.view.explorer.is_empty());
    std::fs::write(&summary, "[]").expect("write summary");
    h2.presenter.open_report(&mut h2.view);
    h2.presenter.open_result_folder(&mut h2.view);

    assert_eq!(
        h2.view.explorer,
        [
            format!("/select,\"{summary}\""),
            dir.path().to_string_lossy().into_owned()
        ]
    );
}

// C#: BenchmarkPresenterTests.ReportStore_WritesFiles
#[test]
fn report_store_writes_files() {
    let root = TempRoot::new("bench-store");
    let mut empty = report("r0", &[], BenchmarkWorkload::Sequential);
    empty.run_id = "r0".to_owned();
    empty.phases.clear();
    let reports = [
        report("fast-sequential", &[1.0, 2.0], BenchmarkWorkload::Sequential),
        empty,
    ];
    let (when, _) = DateTimeUtc::parse_json("2026-10-13T08:09:10Z").expect("fixed instant");

    let summary = save_session(&reports, root.path(), when).expect("save_session");

    let dir = root.path().join("20261013-080910");
    assert_eq!(summary, dir.join("summary.json"));
    assert!(dir.join("fast-sequential-run-fast-sequential.json").is_file());
    assert!(dir.join("report-r0.json").is_file());
    let text = std::fs::read_to_string(&summary).expect("read summary");
    let value: serde_json::Value = serde_json::from_str(&text).expect("summary is JSON");
    assert_eq!(value.as_array().map(Vec::len), Some(2));
    assert!(text.contains('\n')); // indented
}

// ---- Language / defaults ----

// C#: BenchmarkPresenterTests.LanguageChange_NotifiesItemsAndRepaints
#[test]
fn language_change_repaints_the_grid() {
    let mut h = Harness::new();

    h.presenter.notify_language_changed(&mut h.view);

    assert_eq!(h.view.refreshes, 1);
    assert_eq!(h.presenter.profile_items().len(), profiles::all().len());
}

// C#: BenchmarkPresenterTests.Defaults
#[test]
fn defaults() {
    let h = Harness::new();

    let ids: Vec<&str> = h
        .presenter
        .profile_items()
        .iter()
        .map(|i| i.profile.id.as_str())
        .collect();
    let expected: Vec<&str> = profiles::all().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, expected);
    assert_eq!(h.presenter.default_selection().profile.id, "fast-sequential");
    assert_eq!(BenchmarkPresenter::default_image_limit_text(), "64");
    assert_eq!(DEFAULT_IMAGE_LIMIT, 64);
    assert_eq!(
        BenchmarkPresenter::initial_folder(Some("D:\\x"), "P:\\pictures"),
        "D:\\x"
    );
    assert_eq!(BenchmarkPresenter::initial_folder(None, "P:\\pictures"), "P:\\pictures");
}

// C#: BenchmarkPresenterTests.ParseImageLimit_Table
#[test]
fn parse_image_limit_table() {
    let rows: [(Option<&str>, i32); 7] = [
        (Some("64"), 64),
        (Some(" 12"), 12),
        (Some("0"), 0),
        (Some("-1"), 0),
        (Some("x"), 0),
        (Some(""), 0),
        (None, 0),
    ];
    for (text, expected) in rows {
        assert_eq!(BenchmarkPresenter::parse_image_limit(text), expected, "{text:?}");
    }
}

// ---- New tests ----

// New test: the cancellation poll after the loop, and a missing folder.
#[test]
fn enumerate_image_files_with_a_cancelled_token_is_canceled() {
    let dir = TempRoot::new("bench-enum-cancel");
    std::fs::write(dir.join("a.jpg"), b"x").expect("write");
    let token = CancelToken::new();
    token.cancel();

    let result = enumerate_image_files(&dir.path().to_string_lossy(), 0, &token);
    assert_eq!(result, Err(BenchmarkScanError::Canceled));

    let missing = dir.join("no-such-folder");
    let result = enumerate_image_files(&missing.to_string_lossy(), 0, &CancelToken::new());
    assert_eq!(result, Err(BenchmarkScanError::DirectoryNotFound));
}

// New test: RAW and directories are never listed, the limit takes the first n in enumeration order.
#[test]
fn enumerate_image_files_skips_raw_and_directories_and_keeps_order() {
    let dir = TempRoot::new("bench-enum");
    for name in ["a.jpg", "b.png", "c.cr2", "noext", "d.txt"] {
        std::fs::write(dir.join(name), b"x").expect("write");
    }
    std::fs::create_dir(dir.join("folder.jpg")).expect("mkdir");
    let folder = dir.path().to_string_lossy().into_owned();

    let all = enumerate_image_files(&folder, 0, &CancelToken::new()).expect("scan");
    let mut names: Vec<String> = all
        .iter()
        .map(|p| Path::new(p).file_name().expect("name").to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["a.jpg", "b.png"]);

    let limited = enumerate_image_files(&folder, 1, &CancelToken::new()).expect("scan");
    assert_eq!(limited, all[..1]);
    let over = enumerate_image_files(&folder, 50, &CancelToken::new()).expect("scan");
    assert_eq!(over, all);
}

// New test: every created session is disposed exactly once on success, failure and cancel paths.
#[test]
fn session_dispose_runs_once_per_session_on_every_path() {
    let mut h = Harness::new();
    h.set_behavior("fast-sequential", |_, _| Err(operation_error("boom")));
    h.run_to_end(&["fast-sequential", "preview-light"], "bench", Some(""));
    let sessions = h.sessions();
    assert_eq!(sessions.len(), 2);
    for session in &sessions {
        assert_eq!(session.disposed.load(Ordering::SeqCst), 1);
    }

    h.set_behavior("preview-light", |_, _| Err(BenchmarkError::Canceled));
    h.run_to_end(&["preview-light", "fast-sequential"], "bench", Some(""));
    let sessions = h.sessions();
    assert_eq!(sessions.len(), 3); // the second profile never starts after a cancellation
    assert_eq!(sessions[2].disposed.load(Ordering::SeqCst), 1);
    assert_eq!(h.view.status, Some(h.texts.get("benchmark.status.canceled")));
}

// New test: a failing save is reported as failed and nothing is remembered.
#[test]
fn save_failure_is_failed_status_and_not_saved() {
    let mut h = Harness::new();
    *lock(&h.shared.save_error) = Some("disk full".to_owned());

    h.run_to_end(&["fast-sequential"], "bench", Some(""));

    assert_eq!(h.presenter.last_report_path(), None);
    assert_eq!(h.saved_count(), 0);
    assert_eq!(h.view.status, Some(h.failed_status("Unexpected error: disk full")));
    assert_eq!(h.view.running_history, [true, false]);
}

// New test: a scan job whose token is already cancelled reports Canceled without calling the scan.
#[test]
fn scan_job_with_cancelled_token_does_not_call_the_scan() {
    let mut h = Harness::new();
    let called = Arc::new(AtomicBool::new(false));
    let c = Arc::clone(&called);
    h.set_scan(move |_, _, _| {
        c.store(true, Ordering::SeqCst);
        Ok(vec!["a.jpg".to_owned()])
    });

    let step = h.run_selected(&["fast-sequential"], "bench", Some(""));
    h.presenter.cancel();
    let outcome = expect_scan(step).run();

    assert_eq!(outcome, BenchmarkScanOutcome::Error(BenchmarkScanError::Canceled));
    assert!(!called.load(Ordering::SeqCst));
    let step = h.on_scan_finished(outcome);
    expect_idle(&step);
    assert_eq!(h.view.running_history, [true, false]);
}

// New test: a panic inside a session is a Fail row, and the session is still disposed.
#[test]
fn profile_job_panic_becomes_failed_and_session_is_disposed() {
    let mut h = Harness::new();
    h.set_behavior("fast-sequential", |_, _| panic!("kaboom"));

    let job = h.start_until_profile(&["fast-sequential"]);
    let post = h.post_target();
    let outcome = job.run(&post);

    match &outcome {
        BenchmarkProfileOutcome::Failed { message, error } => {
            assert!(message.contains("kaboom"), "{message}");
            assert_eq!(*error, RowError::Other(message.clone()));
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(h.sessions()[0].disposed.load(Ordering::SeqCst), 1);
    let step = h.on_profile_finished(outcome);
    expect_idle(&step); // the only profile failed: nothing to save
    assert_eq!(h.presenter.rows().len(), 1);
    assert_eq!(h.presenter.rows()[0].status(), BenchmarkResultStatus::Fail);
}

// New test: file name rule and the default root name of the report store.
#[test]
fn report_file_name_and_default_root() {
    let one = report("fast-sequential", &[1.0], BenchmarkWorkload::Sequential);
    assert_eq!(report_file_name(&one), "fast-sequential-run-fast-sequential.json");
    let mut none = one;
    none.phases.clear();
    none.run_id = "r0".to_owned();
    assert_eq!(report_file_name(&none), "report-r0.json");
    assert!(default_report_root().ends_with("PhotoReviewRs-Benchmark-Reports"));
}

// New test: a profile job with progress for a profile index beyond the first carries the run's identity.
#[test]
fn progress_message_carries_run_identity() {
    let mut h = Harness::new();
    h.set_behavior("fast-sequential", |progress, _| {
        progress.report(progress_of("fast-sequential", BenchmarkWorkload::Sequential, 1, 2, "m"));
        Ok(report("fast-sequential", &[1.0], BenchmarkWorkload::Sequential))
    });
    let job = h.start_until_profile(&["fast-sequential"]);
    let post = h.post_target();
    let outcome = job.run(&post);
    let queued = h.take_queue();

    assert_eq!(queued.len(), 1);
    assert_eq!(
        (queued[0].run_id, queued[0].profile_index, queued[0].profile_count),
        (1, 1, 1)
    );
    assert_eq!(queued[0].progress.completed, 1);
    let step = h.on_profile_finished(outcome);
    h.drive(step);
}

// New test: the save job returns the injected save's result.
#[test]
fn save_job_runs_the_injected_save() {
    let mut h = Harness::new();
    let step = h.run_selected(&["fast-sequential"], "bench", Some(""));
    let step = h.advance(step);
    let job = expect_profile(step);
    let post = h.post_target();
    let outcome = job.run(&post);
    let step = h.on_profile_finished(outcome);

    let save = expect_save(step);
    assert_eq!(save.run(), Ok(h.summary_path()));
}
