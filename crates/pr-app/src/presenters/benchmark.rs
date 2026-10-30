//! Port of `PhotoReview.App.Shared/Presenters/BenchmarkPresenter.cs` (C# baseline `d92d6aae`, 416 lines): the Benchmark
//! window's behaviour without a window (WP-35 G2a) -- profile list, Run / Quick check / Cancel, progress, the result grid (ranking,
//! best marker), report saving and a close in the middle of a run -- plus the two helpers the same file holds: `BenchmarkReportStore`
//! (lines 354 to 377) and the production `BenchmarkProfileSession` (lines 384 to 416). Contract: `contracts/W8-10i.md`.
//!
//! D-W8-2: the C# `RunAsync` is ONE async method with four awaits (scan, per-profile run, per-profile dispose, save). Rust has no
//! futures: the run is a state machine on the UI thread. Each blocking part is a `Send` job the host runs on a worker; the job's
//! outcome comes back to the UI thread (host: `UiScheduler::post`, FIFO) and is fed to the matching `on_*_finished`, which returns the
//! next [`BenchmarkStep`]. Progress from the engine is posted as a [`BenchmarkProgressMessage`] and fed to `on_progress`.
//! D-W8-1: `ObservableCollection<BenchmarkResultRow>` becomes a `Vec` plus `BenchmarkView::refresh_rows`.
//! INV-9 style guard: progress is applied only while the run that produced it is the live one (`run_id`).
//! Texts: `Localizer` + `NumberCulture` through the B1 [`FieldContext`]; no ambient `Tr`.

use std::any::Any;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pr_bench::bench_recycle_bin;
use pr_bench::engine::{BenchmarkError, BenchmarkProgress, BenchmarkProgressSink, RunPreparedOptions, run_prepared};
use pr_bench::image_executor::{BenchmarkExecutorOptions, BenchmarkImageExecutor, BenchmarkProfileScope};
use pr_bench::models::{
    BenchmarkPhaseResult, BenchmarkProfile, BenchmarkReport, BenchmarkResultStatus, BenchmarkWorkload, rank,
};
use pr_bench::profiles;
use pr_bench::workload_runner::{create_seeded_random, prepare_iteration, registry_images_per_sample};
use pr_core::abstractions::{MemoryProbe, RecycleBin};
use pr_core::catalog::types::is_supported;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::get_directory_name;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache::preview_file::PreviewPayloadCodec;
use pr_imaging::decoder_traits::{ImageDecoder, PlatformImageCodec};
use pr_l10n::template::LocArg;

use super::benchmark_presentation::{
    BEST_SYMBOL, BenchmarkProfileItem, BenchmarkResultRow, RowError, describe_error, profile_name, progress_message,
};
use super::settings_field_map::{FieldContext, try_int};
use crate::misc::app_log::AppLog;

/// C# `QuickCheckProfileId`: "Quick check" always runs this profile.
pub const QUICK_CHECK_PROFILE_ID: &str = "fast-sequential";
/// C# `QuickCheckIterations`.
pub const QUICK_CHECK_ITERATIONS: i32 = 10;
/// C# `DefaultImageLimit`: default first-N image cap (mirrors the CLI's `--benchmark-all` `Take(64)`); 0 means "all".
pub const DEFAULT_IMAGE_LIMIT: i32 = 64;
/// C# `EnumerationCancellationCheckInterval`: entries between two cancellation polls of the folder scan.
pub const ENUMERATION_CANCELLATION_CHECK_INTERVAL: usize = 256;
/// Folder (below the temp directory) of the saved reports. The C# value is `PhotoReview-Benchmark-Reports`; the Rust window uses its
/// own name so two apps never mix files (R0002, `Different` row; see the contract, O3).
pub const DEFAULT_REPORT_ROOT_NAME: &str = "PhotoReviewRs-Benchmark-Reports";

/// C# `IBenchmarkView`. UI-thread object (no `Send`).
pub trait BenchmarkView {
    /// The status line under the progress bar.
    fn set_status(&mut self, text: &str);
    /// The progress bar, 0..1.
    fn set_progress(&mut self, fraction: f64);
    /// True while a run is active (Run, Quick check, Browse, the profile list, the limit and folder boxes disabled, Cancel enabled).
    fn set_running(&mut self, running: bool);
    /// The result rows changed (a row was added or cleared, the best marker moved, the language switched): repaint the grid.
    fn refresh_rows(&mut self);
    /// Opens Explorer with `arguments` (a folder, or `/select,"file"`).
    fn open_in_explorer(&mut self, arguments: &str);
}

/// C# `IBenchmarkProfileSession` (`IAsyncDisposable` becomes an explicit `dispose`): one profile's measurement, created before the run
/// and disposed after it, also when it fails or is cancelled. `Send`: created on the UI thread, driven on a worker.
pub trait BenchmarkProfileSession: Send {
    /// BLOCKING. `Err(BenchmarkError::Canceled)` is the C# `OperationCanceledException`.
    fn run(
        &mut self,
        folder: &str,
        progress: &dyn BenchmarkProgressSink,
        cancel: &CancelToken,
    ) -> Result<BenchmarkReport, BenchmarkError>;
    /// Called exactly once per created session, on every path.
    fn dispose(&mut self);
}

/// C# `BenchmarkSessionFactory`: creates the session for `profile` over `files`. Called on the UI thread, OUTSIDE the per-profile failure
/// handling: an `Err` ends the whole run as "failed".
pub type BenchmarkSessionFactory =
    Arc<dyn Fn(&BenchmarkProfile, &[String]) -> Result<Box<dyn BenchmarkProfileSession>, BenchmarkError> + Send + Sync>;

/// What the folder scan can fail with. The C# distinguishes by exception type; `DirectoryNotFound` must be tested BEFORE `Io` (in .NET it
/// derives from `IOException`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchmarkScanError {
    /// C# `DirectoryNotFoundException`.
    DirectoryNotFound,
    /// C# `IOException` / `UnauthorizedAccessException`: the message is shown ("Cannot read the folder: ...").
    Io(String),
    /// C# `OperationCanceledException`.
    Canceled,
    /// Anything else (unexpected): shown as "Failed: Unexpected error: ...".
    Other(String),
}

/// C# `BenchmarkFileScan(folder, imageLimit, cancellationToken)`: BLOCKING, worker thread. Default: [`enumerate_image_files`].
pub type BenchmarkFileScan =
    Arc<dyn Fn(&str, i32, &CancelToken) -> Result<Vec<String>, BenchmarkScanError> + Send + Sync>;

/// C# `BenchmarkSummarySave`: writes the reports, returns the summary file path (`Err` = the exception message). BLOCKING, worker thread.
/// Default: [`save_session`] under [`default_report_root`] with the current UTC time.
pub type BenchmarkSummarySave = Arc<dyn Fn(&[BenchmarkReport]) -> Result<String, String> + Send + Sync>;

/// Progress of the engine plus the identity of the run and profile it belongs to (C# `RunProgress`). The host posts it to the UI thread
/// and calls [`BenchmarkPresenter::on_progress`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkProgressMessage {
    pub run_id: i32,
    /// 1-based index of the profile in this run.
    pub profile_index: usize,
    pub profile_count: usize,
    pub progress: BenchmarkProgress,
}

/// A started folder scan, ready for a worker. `Send`.
pub struct BenchmarkScanJob {
    scan: BenchmarkFileScan,
    folder: String,
    image_limit: i32,
    cancel: CancelToken,
}

impl fmt::Debug for BenchmarkScanJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkScanJob")
            .field("folder", &self.folder)
            .field("image_limit", &self.image_limit)
            .finish_non_exhaustive()
    }
}

/// What the scan job reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchmarkScanOutcome {
    Files(Vec<String>),
    Error(BenchmarkScanError),
}

impl BenchmarkScanJob {
    /// BLOCKING, worker thread. A token already cancelled before the scan starts gives `Error(Canceled)` without calling the scan (C#
    /// `Task.Run(..., token)`); a panic of the scan is `Error(Other(message))`.
    pub fn run(self) -> BenchmarkScanOutcome {
        let Self {
            scan,
            folder,
            image_limit,
            cancel,
        } = self;
        if cancel.is_canceled() {
            return BenchmarkScanOutcome::Error(BenchmarkScanError::Canceled);
        }
        match catch_unwind(AssertUnwindSafe(|| scan(&folder, image_limit, &cancel))) {
            Ok(Ok(files)) => BenchmarkScanOutcome::Files(files),
            Ok(Err(error)) => BenchmarkScanOutcome::Error(error),
            Err(payload) => BenchmarkScanOutcome::Error(BenchmarkScanError::Other(panic_message(payload.as_ref()))),
        }
    }
}

/// A started profile, ready for a worker. `Send`: owns the session created by the factory.
pub struct BenchmarkProfileJob {
    profile: Box<BenchmarkProfile>,
    session: Box<dyn BenchmarkProfileSession>,
    folder: String,
    cancel: CancelToken,
    run_id: i32,
    profile_index: usize,
    profile_count: usize,
}

impl fmt::Debug for BenchmarkProfileJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkProfileJob")
            .field("profile", &self.profile.id)
            .field("run_id", &self.run_id)
            .finish_non_exhaustive()
    }
}

/// What the profile job reports. The session is ALREADY disposed when the outcome exists.
// The outcome is built once per profile and moved straight into `on_profile_finished`; boxing would change the contract's public shape.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum BenchmarkProfileOutcome {
    /// The session returned a report (also when Cancel was requested meanwhile: a report that finished is kept).
    Report(BenchmarkReport),
    /// `Err(BenchmarkError::Canceled)`: the run ends as cancelled.
    Canceled,
    /// Any other error or panic of the session: a Fail row, the run continues with the next profile.
    Failed { message: String, error: RowError },
}

impl BenchmarkProfileJob {
    /// BLOCKING, worker thread. `session.run(folder, sink, token)` where `sink` forwards every engine progress as a
    /// [`BenchmarkProgressMessage`] to `post` (called on the worker; the host marshals it); then `session.dispose()` ALWAYS (also after a
    /// failure, a cancellation or a panic; a panic of `run` is caught and becomes `Failed`). `Ok(report)` => `Report`;
    /// `Err(Canceled)` => `Canceled`; other `Err(e)` => `Failed { message: e.to_string(), error }` with `error` =
    /// `RowError::Profile(..)` for `BenchmarkError::Profile` and `RowError::Other(e.to_string())` otherwise.
    pub fn run(self, post: &(dyn Fn(BenchmarkProgressMessage) + Send + Sync)) -> BenchmarkProfileOutcome {
        let Self {
            profile: _,
            mut session,
            folder,
            cancel,
            run_id,
            profile_index,
            profile_count,
        } = self;
        let sink = move |progress: BenchmarkProgress| {
            post(BenchmarkProgressMessage {
                run_id,
                profile_index,
                profile_count,
                progress,
            });
        };
        let result = catch_unwind(AssertUnwindSafe(|| session.run(&folder, &sink, &cancel)));
        // The C# `finally { await session.DisposeAsync(); }`: on every path; a panic of the dispose itself must not hide the outcome.
        let _ = catch_unwind(AssertUnwindSafe(|| session.dispose()));
        match result {
            Ok(Ok(report)) => BenchmarkProfileOutcome::Report(report),
            Ok(Err(BenchmarkError::Canceled)) => BenchmarkProfileOutcome::Canceled,
            Ok(Err(error)) => {
                let row_error = row_error_of(&error);
                BenchmarkProfileOutcome::Failed {
                    message: error.to_string(),
                    error: row_error,
                }
            }
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                BenchmarkProfileOutcome::Failed {
                    error: RowError::Other(message.clone()),
                    message,
                }
            }
        }
    }
}

/// A started report save, ready for a worker. `Send`.
pub struct BenchmarkSaveJob {
    save: BenchmarkSummarySave,
    reports: Vec<BenchmarkReport>,
}

impl fmt::Debug for BenchmarkSaveJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkSaveJob")
            .field("reports", &self.reports.len())
            .finish_non_exhaustive()
    }
}

impl BenchmarkSaveJob {
    /// BLOCKING, worker thread: the injected save; a panic becomes `Err(message)`.
    pub fn run(self) -> Result<String, String> {
        let Self { save, reports } = self;
        match catch_unwind(AssertUnwindSafe(|| save(&reports))) {
            Ok(result) => result,
            Err(payload) => Err(panic_message(payload.as_ref())),
        }
    }
}

/// What the host does next. `Idle` = the run is over (or nothing started): re-enable nothing, the view was already told.
#[derive(Debug)]
pub enum BenchmarkStep {
    Idle,
    Scan(BenchmarkScanJob),
    Profile(BenchmarkProfileJob),
    Save(BenchmarkSaveJob),
}

/// State of the run in progress (C# locals of `RunAsync` and the fields `_cts`, `_runId`).
struct RunState {
    profiles: Vec<BenchmarkProfile>,
    folder: String,
    files: Vec<String>,
    /// Index of the profile whose job is out or about to start (0-based).
    next_profile: usize,
    reports: Vec<BenchmarkReport>,
    /// C# `runToken`: captured ONCE at the start; `close()` cancels it but the run keeps using this clone.
    token: CancelToken,
}

/// C# `BenchmarkPresenter`. UI-thread object, except the four job types it hands out.
pub struct BenchmarkPresenter {
    session_factory: BenchmarkSessionFactory,
    scan: BenchmarkFileScan,
    save: BenchmarkSummarySave,
    profile_items: Vec<BenchmarkProfileItem>,
    rows: Vec<BenchmarkResultRow>,
    /// C# `_cts`: the slot `cancel()` and `close()` act on. `None` when idle or after `close()`.
    cancel_slot: Option<CancelToken>,
    last_report: Option<String>,
    run_id: i32,
    is_running: bool,
    run: Option<RunState>,
}

impl fmt::Debug for BenchmarkPresenter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkPresenter")
            .field("is_running", &self.is_running)
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}

impl BenchmarkPresenter {
    /// C# constructor. `scan` / `save` `None` = the defaults named on their type aliases. `profile_items` = one
    /// `BenchmarkProfileItem` per `pr_bench::profiles::all()` entry, in registry order.
    pub fn new(
        session_factory: BenchmarkSessionFactory,
        scan: Option<BenchmarkFileScan>,
        save: Option<BenchmarkSummarySave>,
    ) -> Self {
        Self {
            session_factory,
            scan: scan.unwrap_or_else(|| Arc::new(enumerate_image_files)),
            save: save.unwrap_or_else(default_summary_save),
            profile_items: profiles::all().iter().cloned().map(BenchmarkProfileItem::new).collect(),
            rows: Vec::new(),
            cancel_slot: None,
            last_report: None,
            run_id: 0,
            is_running: false,
            run: None,
        }
    }

    // ---- State the view binds to ----

    /// C# `ProfileItems`.
    pub fn profile_items(&self) -> &[BenchmarkProfileItem] {
        &self.profile_items
    }

    /// C# `DefaultSelection`: the item whose profile id is [`QUICK_CHECK_PROFILE_ID`].
    pub fn default_selection(&self) -> &BenchmarkProfileItem {
        // The registry always holds the quick-check profile (a unit test pins it); the first item is only the unreachable fallback.
        self.profile_items
            .iter()
            .find(|item| item.profile.id == QUICK_CHECK_PROFILE_ID)
            .unwrap_or_else(|| &self.profile_items[0])
    }

    /// C# `Rows`.
    pub fn rows(&self) -> &[BenchmarkResultRow] {
        &self.rows
    }

    /// C# `IsRunning`.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// C# `LastReportPath`.
    pub fn last_report_path(&self) -> Option<&str> {
        self.last_report.as_deref()
    }

    /// C# static `InitialFolder(initialFolder)`: `initial_folder` or, when `None`, `pictures_folder` (the caller resolves the user's Pictures
    /// folder, C# `Environment.GetFolderPath(MyPictures)`; no `windows` call in this module).
    pub fn initial_folder(initial_folder: Option<&str>, pictures_folder: &str) -> String {
        initial_folder.unwrap_or(pictures_folder).to_owned()
    }

    /// C# static `DefaultImageLimitText`: `"64"`.
    pub fn default_image_limit_text() -> String {
        DEFAULT_IMAGE_LIMIT.to_string()
    }

    /// C# static `ParseImageLimit(text)`: `settings_field_map::try_int(text, 1, i32::MAX)` (invariant culture; leading and trailing whitespace
    /// allowed: `" 12"` is 12) or 0 for everything else (blank, invalid, zero, negative, `None`).
    pub fn parse_image_limit(text: Option<&str>) -> i32 {
        text.and_then(|text| try_int(text, 1, i32::MAX)).unwrap_or(0)
    }

    /// C# static `ApplyImageLimit(files, limit)`: the first `limit` files when `limit > 0 && files.len() > limit`, else a copy; order kept.
    pub fn apply_image_limit(files: &[String], limit: i32) -> Vec<String> {
        match usize::try_from(limit) {
            Ok(limit) if limit > 0 && files.len() > limit => files[..limit].to_vec(),
            _ => files.to_vec(),
        }
    }

    /// C# static `BuildQuickCheckProfile()`: `pr_bench::profiles::find("fast-sequential")` cloned with `iterations = 10`; the warm-up count stays
    /// the profile's own. `None` when the registry lacks it.
    pub fn build_quick_check_profile() -> Option<BenchmarkProfile> {
        profiles::find(QUICK_CHECK_PROFILE_ID).map(|profile| {
            let mut quick = profile.clone();
            quick.iterations = QUICK_CHECK_ITERATIONS;
            quick
        })
    }

    // ---- Commands ----

    /// C# `RunSelectedAsync(profiles, folder, imageLimitText)`, step 1 (UI thread). A run in progress: `Idle`, nothing happens (checked
    /// BEFORE the empty-selection check). No profile: `view.set_status(benchmark.status.noProfileSelected)` and `Idle` (never busy). Else the run
    /// starts: see [`Self::start`].
    pub fn run_selected(
        &mut self,
        profiles: &[BenchmarkProfile],
        folder: &str,
        image_limit_text: Option<&str>,
        view: &mut dyn BenchmarkView,
        ctx: &FieldContext<'_>,
    ) -> BenchmarkStep {
        if self.is_running {
            return BenchmarkStep::Idle;
        }
        if profiles.is_empty() {
            view.set_status(&ctx.texts.get("benchmark.status.noProfileSelected"));
            return BenchmarkStep::Idle;
        }
        self.start(profiles.to_vec(), folder, image_limit_text, view)
    }

    /// C# `QuickCheckAsync(folder, imageLimitText)`: never looks at the selection. A run in progress: `Idle`. A missing profile: status
    /// `bench.error.unexpected` with `"Unknown profile 'fast-sequential'"` and `Idle`. Else a run of `[build_quick_check_profile()]`.
    pub fn quick_check(
        &mut self,
        folder: &str,
        image_limit_text: Option<&str>,
        view: &mut dyn BenchmarkView,
        ctx: &FieldContext<'_>,
    ) -> BenchmarkStep {
        if self.is_running {
            return BenchmarkStep::Idle;
        }
        match Self::build_quick_check_profile() {
            Some(profile) => self.start(vec![profile], folder, image_limit_text, view),
            None => {
                let error = RowError::Other(format!("Unknown profile '{QUICK_CHECK_PROFILE_ID}'"));
                view.set_status(&describe_error(&error, ctx.texts));
                BenchmarkStep::Idle
            }
        }
    }

    /// Start of C# `RunAsync` (private): `image_limit = parse_image_limit(text)`; `run_id += 1`; `is_running = true`; a fresh token stored in
    /// BOTH `cancel_slot` and the `RunState` (so Cancel/Close can interrupt a slow scan too); `view.set_running(true)`; rows cleared and
    /// `view.refresh_rows()`; `view.set_progress(0.0)`; returns `Scan(job)`.
    fn start(
        &mut self,
        profiles: Vec<BenchmarkProfile>,
        folder: &str,
        image_limit_text: Option<&str>,
        view: &mut dyn BenchmarkView,
    ) -> BenchmarkStep {
        let image_limit = Self::parse_image_limit(image_limit_text);
        self.run_id = self.run_id.wrapping_add(1);
        self.is_running = true;
        // Created before the scan so Cancel/Close can interrupt a slow enumeration too, not only the profile run; the run keeps its own clone.
        let token = CancelToken::new();
        self.cancel_slot = Some(token.clone());
        view.set_running(true);
        self.rows.clear();
        view.refresh_rows();
        view.set_progress(0.0);
        self.run = Some(RunState {
            profiles,
            folder: folder.to_owned(),
            files: Vec::new(),
            next_profile: 0,
            reports: Vec::new(),
            token: token.clone(),
        });
        BenchmarkStep::Scan(BenchmarkScanJob {
            scan: Arc::clone(&self.scan),
            folder: folder.to_owned(),
            image_limit,
            cancel: token,
        })
    }

    /// Step 2 (UI thread), with the scan's outcome. `Error(DirectoryNotFound)` => status `benchmark.status.folderMissing`; `Error(Io(m))` =>
    /// `benchmark.status.cannotReadFolder {message}`; `Error(Canceled)` => `benchmark.status.canceled`; `Error(Other(m))` =>
    /// `bench.status.failed {message: describe_error(RowError::Other(m))}`; `Files([])` => `benchmark.status.noImages`; each of these ends the run
    /// (`finish_run`) and returns `Idle`. `Files(files)` => [`Self::next_profile_step`].
    pub fn on_scan_finished(
        &mut self,
        outcome: BenchmarkScanOutcome,
        view: &mut dyn BenchmarkView,
        ctx: &FieldContext<'_>,
    ) -> BenchmarkStep {
        if self.run.is_none() {
            return BenchmarkStep::Idle;
        }
        let status = match outcome {
            BenchmarkScanOutcome::Files(files) if !files.is_empty() => {
                if let Some(run) = self.run.as_mut() {
                    run.files = files;
                }
                return self.next_profile_step(view, ctx);
            }
            BenchmarkScanOutcome::Files(_) => ctx.texts.get("benchmark.status.noImages"),
            // `DirectoryNotFound` before `Io`: in .NET it derives from `IOException`.
            BenchmarkScanOutcome::Error(BenchmarkScanError::DirectoryNotFound) => {
                ctx.texts.get("benchmark.status.folderMissing")
            }
            BenchmarkScanOutcome::Error(BenchmarkScanError::Io(message)) => ctx.texts.format_with(
                "benchmark.status.cannotReadFolder",
                &[LocArg::new("message", message)],
                ctx.numbers,
            ),
            BenchmarkScanOutcome::Error(BenchmarkScanError::Canceled) => ctx.texts.get("benchmark.status.canceled"),
            BenchmarkScanOutcome::Error(BenchmarkScanError::Other(message)) => {
                failed_status(&RowError::Other(message), ctx)
            }
        };
        view.set_status(&status);
        self.finish_run(view);
        BenchmarkStep::Idle
    }

    /// The top of the C# `foreach (var profile in profiles)` loop: when profiles remain, `profile_index = next_profile + 1`, then the factory is
    /// called (UI thread) with the profile and the files; `Err(e)` ends the run as failed (status `bench.status.failed {describe_error(..)}`,
    /// `finish_run`, `Idle`; sessions created earlier are already disposed) and `Ok(session)` gives `Profile(job)` carrying the run's captured
    /// token. When no profile remains: with at least one report, `Save(job)`; with none, the run ends (`finish_run`, `Idle`) leaving the last status.
    fn next_profile_step(&mut self, view: &mut dyn BenchmarkView, ctx: &FieldContext<'_>) -> BenchmarkStep {
        let Some(run) = self.run.as_ref() else {
            return BenchmarkStep::Idle;
        };
        let Some(profile) = run.profiles.get(run.next_profile).cloned() else {
            // No profile left: save when at least one report exists, else the run ends with the last status.
            if run.reports.is_empty() {
                self.finish_run(view);
                return BenchmarkStep::Idle;
            }
            return BenchmarkStep::Save(BenchmarkSaveJob {
                save: Arc::clone(&self.save),
                reports: run.reports.clone(),
            });
        };
        let profile_index = run.next_profile + 1;
        let profile_count = run.profiles.len();
        let (cancel, folder) = (run.token.clone(), run.folder.clone());
        // Outside the profile's failure handling: a factory failure ends the whole run.
        match (self.session_factory)(&profile, &run.files) {
            Ok(session) => BenchmarkStep::Profile(BenchmarkProfileJob {
                profile: Box::new(profile),
                session,
                folder,
                cancel,
                run_id: self.run_id,
                profile_index,
                profile_count,
            }),
            Err(BenchmarkError::Canceled) => {
                view.set_status(&ctx.texts.get("benchmark.status.canceled"));
                self.finish_run(view);
                BenchmarkStep::Idle
            }
            Err(error) => {
                view.set_status(&failed_status(&row_error_of(&error), ctx));
                self.finish_run(view);
                BenchmarkStep::Idle
            }
        }
    }

    /// Step 3 (UI thread), with a profile's outcome. `Report(r)`: push `r` to the session reports; one `BenchmarkResultRow::new(profile,
    /// phase.clone(), None)` per phase; `recompute_best`. `Canceled`: status `benchmark.status.canceled`, `finish_run`, `Idle` (no Fail row).
    /// `Failed`: log `"Benchmark profile failed: {id}"`, a Fail row (`BenchmarkPhaseResult::new(id, workload, vec![], Fail, Some(message), None)`
    /// with `Some(error)`), `recompute_best`, status `benchmark.status.profileFailed {profileName, message: describe_error(error)}`. After
    /// `Report` and `Failed` the loop goes on with [`Self::next_profile_step`] (`next_profile += 1`).
    pub fn on_profile_finished(
        &mut self,
        outcome: BenchmarkProfileOutcome,
        view: &mut dyn BenchmarkView,
        ctx: &FieldContext<'_>,
    ) -> BenchmarkStep {
        let Some(profile) = self
            .run
            .as_ref()
            .and_then(|run| run.profiles.get(run.next_profile).cloned())
        else {
            return BenchmarkStep::Idle;
        };
        match outcome {
            BenchmarkProfileOutcome::Report(report) => {
                for phase in &report.phases {
                    self.rows
                        .push(BenchmarkResultRow::new(profile.clone(), phase.clone(), None));
                }
                if let Some(run) = self.run.as_mut() {
                    run.reports.push(report);
                }
                self.recompute_best(view);
            }
            BenchmarkProfileOutcome::Canceled => {
                view.set_status(&ctx.texts.get("benchmark.status.canceled"));
                self.finish_run(view);
                return BenchmarkStep::Idle;
            }
            BenchmarkProfileOutcome::Failed { message, error } => {
                AppLog::global().error(&format!("Benchmark profile failed: {}", profile.id), None);
                let phase = BenchmarkPhaseResult::new(
                    profile.id.clone(),
                    profile.workload,
                    Vec::new(),
                    BenchmarkResultStatus::Fail,
                    Some(message),
                    None,
                );
                self.rows
                    .push(BenchmarkResultRow::new(profile.clone(), phase, Some(error.clone())));
                self.recompute_best(view);
                let text = ctx.texts.format_with(
                    "benchmark.status.profileFailed",
                    &[
                        LocArg::new("profileName", profile_name(&profile, ctx.texts)),
                        LocArg::new("message", describe_error(&error, ctx.texts)),
                    ],
                    ctx.numbers,
                );
                view.set_status(&text);
            }
        }
        if let Some(run) = self.run.as_mut() {
            run.next_profile += 1;
        }
        self.next_profile_step(view, ctx)
    }

    /// Step 4 (UI thread), with the save's result. `Ok(path)`: `last_report = path`, status `benchmark.status.done {completed: reports,
    /// total: profiles, reportPath}`. `Err(m)`: status `bench.status.failed {describe_error(Other(m))}`. Either way `finish_run`, `Idle`.
    pub fn on_save_finished(
        &mut self,
        result: Result<String, String>,
        view: &mut dyn BenchmarkView,
        ctx: &FieldContext<'_>,
    ) -> BenchmarkStep {
        let Some(run) = self.run.as_ref() else {
            return BenchmarkStep::Idle;
        };
        let (completed, total) = (run.reports.len(), run.profiles.len());
        let status = match result {
            Ok(path) => {
                let text = ctx.texts.format_with(
                    "benchmark.status.done",
                    &[
                        LocArg::new("completed", count_arg(completed)),
                        LocArg::new("total", count_arg(total)),
                        LocArg::new("reportPath", path.clone()),
                    ],
                    ctx.numbers,
                );
                self.last_report = Some(path);
                text
            }
            Err(message) => failed_status(&RowError::Other(message), ctx),
        };
        view.set_status(&status);
        self.finish_run(view);
        BenchmarkStep::Idle
    }

    /// C# `ApplyProgress(runId, index, count, p)`, UI thread. Dropped when `!is_running || message.run_id != self.run_id`. Else
    /// `view.set_progress(total == 0 ? 0.0 : completed / total)` and status `benchmark.status.progress {profileIndex, profileCount, profileId,
    /// completed, total, message: progress_message(p.message)}`.
    pub fn on_progress(
        &mut self,
        message: BenchmarkProgressMessage,
        view: &mut dyn BenchmarkView,
        ctx: &FieldContext<'_>,
    ) {
        // INV-9: only the live run's progress counts (`is_running` alone passes a finished run's message, the id alone a reused flag).
        if !self.is_running || message.run_id != self.run_id {
            return;
        }
        let progress = &message.progress;
        let fraction = if progress.total == 0 {
            0.0
        } else {
            f64::from(progress.completed) / f64::from(progress.total)
        };
        view.set_progress(fraction);
        let text = ctx.texts.format_with(
            "benchmark.status.progress",
            &[
                LocArg::new("profileIndex", count_arg(message.profile_index)),
                LocArg::new("profileCount", count_arg(message.profile_count)),
                LocArg::new("profileId", progress.profile_id.clone()),
                LocArg::new("completed", progress.completed),
                LocArg::new("total", progress.total),
                LocArg::new("message", progress_message(&progress.message, ctx.texts)),
            ],
            ctx.numbers,
        );
        view.set_status(&text);
    }

    /// The C# `finally`: `is_running = false`, `view.set_running(false)`, `close()`, `run = None`.
    fn finish_run(&mut self, view: &mut dyn BenchmarkView) {
        self.is_running = false;
        view.set_running(false);
        self.close();
        self.run = None;
    }

    /// C# `RecomputeBest()`: every `is_best` reset; for each distinct workload of the rows (first-appearance order) the ranked list
    /// `pr_bench::models::rank(phases, workload, &registry_images_per_sample)` gives the winner id (first element); the FIRST row with that
    /// workload and `phase.profile_id == winner` gets `is_best`; no winner, no marker. Ends with `view.refresh_rows()`.
    fn recompute_best(&mut self, view: &mut dyn BenchmarkView) {
        for row in &mut self.rows {
            row.is_best = false;
        }
        let phases: Vec<BenchmarkPhaseResult> = self.rows.iter().map(|row| row.phase.clone()).collect();
        let mut workloads: Vec<BenchmarkWorkload> = Vec::new();
        for row in &self.rows {
            if !workloads.contains(&row.workload()) {
                workloads.push(row.workload());
            }
        }
        for workload in workloads {
            let winner = rank(&phases, workload, &registry_images_per_sample)
                .first()
                .map(|phase| phase.profile_id.clone());
            let Some(winner) = winner else {
                continue;
            };
            if let Some(row) = self
                .rows
                .iter_mut()
                .find(|row| row.workload() == workload && row.phase.profile_id == winner)
            {
                row.is_best = true;
            }
        }
        view.refresh_rows();
    }

    /// C# `Cancel()`: cancels the current token; nothing when idle (the slot is empty).
    pub fn cancel(&self) {
        if let Some(token) = &self.cancel_slot {
            token.cancel();
        }
    }

    /// C# `Dispose()` (the window closed): takes the slot, cancels it. Idempotent; a run keeps going through its captured token and ends via the
    /// normal cancel path (a close between two profiles is "Canceled", never a Fail row).
    pub fn close(&mut self) {
        if let Some(token) = self.cancel_slot.take() {
            token.cancel();
        }
    }

    /// C# `NotifyLanguageChanged()`: `view.refresh_rows()` (profile names and cells are recomputed on read, decision D-10h-2).
    pub fn notify_language_changed(&self, view: &mut dyn BenchmarkView) {
        view.refresh_rows();
    }

    /// C# `OpenReport()`: when `last_report` is `Some` and that FILE exists (`Path::is_file`), `view.open_in_explorer("/select,\"<path>\"")`.
    pub fn open_report(&self, view: &mut dyn BenchmarkView) {
        if let Some(path) = &self.last_report
            && Path::new(path).is_file()
        {
            view.open_in_explorer(&format!("/select,\"{path}\""));
        }
    }

    /// C# `OpenResultFolder()`: the directory of the last report (`get_directory_name`), or the temp path before any run (`Path.GetTempPath()`
    /// form: with a trailing backslash).
    pub fn open_result_folder(&self, view: &mut dyn BenchmarkView) {
        let dir = self
            .last_report
            .as_deref()
            .and_then(get_directory_name)
            .unwrap_or_else(temp_path_with_separator);
        view.open_in_explorer(&dir);
    }

    /// C# `ResultsToCsv()`: header `[BEST_SYMBOL, benchmark.column.profile, benchmark.column.loadingMode, benchmark.column.workload,
    /// benchmark.column.samples, "P50", "P95", "P99", "Max", benchmark.column.status]`, then `row.cell_texts(ctx.texts)` per row. Cells containing `,`,
    /// `"`, CR or LF are quoted with `"` doubled; every line ends with `"\r\n"` (including the last).
    pub fn results_to_csv(&self, ctx: &FieldContext<'_>) -> String {
        let header = [
            BEST_SYMBOL.to_owned(),
            ctx.texts.get("benchmark.column.profile"),
            ctx.texts.get("benchmark.column.loadingMode"),
            ctx.texts.get("benchmark.column.workload"),
            ctx.texts.get("benchmark.column.samples"),
            "P50".to_owned(),
            "P95".to_owned(),
            "P99".to_owned(),
            "Max".to_owned(),
            ctx.texts.get("benchmark.column.status"),
        ];
        let mut csv = String::new();
        append_csv_line(&mut csv, &header);
        for row in &self.rows {
            append_csv_line(&mut csv, &row.cell_texts(ctx.texts));
        }
        csv
    }
}

/// C# `EnumerateImageFiles(folder, imageLimit, ct)`: BLOCKING scan. `DirectoryNotFound` when the folder is not a directory. Lists the TOP-LEVEL FILES
/// only (`std::fs::read_dir`, keep the enumeration order, never sort), keeps `pr_core::catalog::types::is_supported(path)` (RAW is NOT listed: the
/// executor builds its preview service without the RAW-routed decoder), counts every enumerated file in `seen` and polls the token when `seen %
/// ENUMERATION_CANCELLATION_CHECK_INTERVAL == 0`, polls once more after the loop (`Err(Canceled)`), then `apply_image_limit`. An `io::Error`
/// (also from a single entry) is `Io(message)`.
pub fn enumerate_image_files(
    folder: &str,
    image_limit: i32,
    cancel: &CancelToken,
) -> Result<Vec<String>, BenchmarkScanError> {
    if !Path::new(folder).is_dir() {
        return Err(BenchmarkScanError::DirectoryNotFound);
    }
    let io_error = |error: std::io::Error| BenchmarkScanError::Io(error.to_string());
    let canceled = |_| BenchmarkScanError::Canceled;
    let mut supported: Vec<String> = Vec::new();
    let mut seen = 0usize;
    for entry in std::fs::read_dir(folder).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        // Top-level FILES only (C# `EnumerateFiles`): a directory is skipped and not counted.
        if entry.file_type().map_err(io_error)?.is_dir() {
            continue;
        }
        seen += 1;
        if seen % ENUMERATION_CANCELLATION_CHECK_INTERVAL == 0 {
            cancel.check().map_err(canceled)?;
        }
        let path = entry.path().to_string_lossy().into_owned();
        if is_supported(&path) {
            supported.push(path);
        }
    }
    cancel.check().map_err(canceled)?;
    Ok(BenchmarkPresenter::apply_image_limit(&supported, image_limit))
}

// ---- BenchmarkReportStore ----

/// C# `BenchmarkReportStore.DefaultRoot`: `<temp>/PhotoReviewRs-Benchmark-Reports` ([`DEFAULT_REPORT_ROOT_NAME`]).
pub fn default_report_root() -> PathBuf {
    std::env::temp_dir().join(DEFAULT_REPORT_ROOT_NAME)
}

/// C# `ReportFileName(report)`: `"{first phase profile id or "report"}-{run_id}.json"`; a report with no phases must not fail.
pub fn report_file_name(report: &BenchmarkReport) -> String {
    let first = report
        .phases
        .first()
        .map_or("report", |phase| phase.profile_id.as_str());
    format!("{first}-{}.json", report.run_id)
}

/// C# `SaveSessionAsync(reports, root, utcNow)`, synchronous: `<root>/<yyyyMMdd-HHmmss>` (digits of `utc_now.to_json()`, invariant) is created
/// (`create_dir_all`); one `report_file_name` file per report holding `report.to_json(&registry_images_per_sample)`; then `summary.json` holding the
/// indented JSON ARRAY of the reports (empty list: `[]`). Returns the summary path. An error of any step is returned (C# lets the exception propagate).
pub fn save_session(reports: &[BenchmarkReport], root: &Path, utc_now: DateTimeUtc) -> std::io::Result<PathBuf> {
    let dir = root.join(timestamp_folder_name(utc_now));
    std::fs::create_dir_all(&dir)?;
    let mut jsons = Vec::with_capacity(reports.len());
    for report in reports {
        let json = report
            .to_json(&registry_images_per_sample)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        std::fs::write(dir.join(report_file_name(report)), &json)?;
        jsons.push(json);
    }
    let summary_path = dir.join("summary.json");
    std::fs::write(&summary_path, summary_json(&jsons))?;
    Ok(summary_path)
}

// ---- BenchmarkProfileSession (production) ----

/// What the production session needs from the host (C# `Factory(codec, decoderFactory)` plus the globals it reads).
#[derive(Clone)]
pub struct BenchmarkSessionDeps {
    pub platform_codec: Arc<dyn PlatformImageCodec>,
    pub payload_codec: Arc<dyn PreviewPayloadCodec>,
    /// C# `Func<IImageDecoder> decoderFactory`: a fresh decoder per session.
    pub decoder_factory: Arc<dyn Fn() -> Arc<dyn ImageDecoder> + Send + Sync>,
    /// C# `WindowsRecycleBin.Instance`, only used when `PHOTOREVIEW_RS_BENCH_REAL_RECYCLE_BIN=1` (`bench_recycle_bin::create`).
    pub real_recycle_bin: Arc<dyn Fn() -> Arc<dyn RecycleBin> + Send + Sync>,
    /// `BenchmarkExecutorOptions::physical_memory_bytes`.
    pub physical_memory_bytes: i64,
    /// `BenchmarkExecutorOptions::memory_probe`: the host passes `pr_platform_win::memory_probe::WindowsMemoryProbe` (this crate cannot name
    /// it, D-W8B1-1); `None` = fail closed.
    pub memory_probe: Option<Arc<dyn MemoryProbe>>,
    /// `BenchmarkExecutorOptions::scratch_parent`: where the per-session scratch directory is created; `None` = `<temp>/PhotoReviewBench`
    /// (production). A test passes a `TempRoot` so `dispose` is observable.
    pub scratch_parent: Option<std::path::PathBuf>,
}

impl fmt::Debug for BenchmarkSessionDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BenchmarkSessionDeps")
            .field("physical_memory_bytes", &self.physical_memory_bytes)
            .finish_non_exhaustive()
    }
}

/// C# `BenchmarkProfileSession.Factory(codec, decoderFactory)`: the factory the app passes to [`BenchmarkPresenter::new`]; each call builds a
/// [`ProductionBenchmarkSession`].
pub fn production_session_factory(deps: BenchmarkSessionDeps) -> BenchmarkSessionFactory {
    Arc::new(move |profile, files| {
        ProductionBenchmarkSession::new(profile, files, &deps)
            .map(|session| Box::new(session) as Box<dyn BenchmarkProfileSession>)
    })
}

/// C# `BenchmarkProfileSession`: the real image executor for one profile with the profile's logging switch applied around the run.
pub struct ProductionBenchmarkSession {
    profile: BenchmarkProfile,
    files: Arc<[String]>,
    executor: Arc<BenchmarkImageExecutor>,
    random: pr_bench::workload_runner::DotNetRandom,
    recycle_bin: Arc<dyn RecycleBin>,
    disposed: bool,
}

impl fmt::Debug for ProductionBenchmarkSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProductionBenchmarkSession")
            .field("profile", &self.profile.id)
            .finish_non_exhaustive()
    }
}

impl ProductionBenchmarkSession {
    /// C# constructor: `BenchmarkImageExecutor::new(profile, files, platform_codec, payload_codec, options{decoder: Some(decoder_factory()),
    /// physical_memory_bytes})`; `random = workload_runner::create_seeded_random(&profile.id)`; recycle bin = `bench_recycle_bin::create(real)`.
    /// The logging switch is NOT applied here (C# applies `BenchmarkProfileScope.ApplyLogging` in the constructor): `LoggingScope` holds a
    /// non-`Send` closure and the session must be `Send`, so `run` applies it (see there). An executor construction failure is the factory's
    /// `Err(BenchmarkError::Operation(..))`.
    pub fn new(
        profile: &BenchmarkProfile,
        files: &[String],
        deps: &BenchmarkSessionDeps,
    ) -> Result<Self, BenchmarkError> {
        let options = BenchmarkExecutorOptions {
            decoder: Some((deps.decoder_factory)()),
            physical_memory_bytes: deps.physical_memory_bytes,
            memory_probe: deps.memory_probe.clone(),
            scratch_parent: deps.scratch_parent.clone(),
            ..BenchmarkExecutorOptions::default()
        };
        let executor = BenchmarkImageExecutor::new(
            profile,
            files,
            Arc::clone(&deps.platform_codec),
            Arc::clone(&deps.payload_codec),
            options,
        )
        .map_err(|error| BenchmarkError::Operation(Box::new(error)))?;
        Ok(Self {
            profile: profile.clone(),
            files: Arc::from(files.to_vec()),
            executor: Arc::new(executor),
            random: create_seeded_random(&profile.id),
            recycle_bin: bench_recycle_bin::create(|| (deps.real_recycle_bin)()),
            disposed: false,
        })
    }
}

/// The session's scratch (disk cache) directory; `dispose` deletes it on a background thread.
impl ProductionBenchmarkSession {
    pub fn scratch_directory(&self) -> &std::path::Path {
        self.executor.disk_cache_directory()
    }
}

impl BenchmarkProfileSession for ProductionBenchmarkSession {
    /// C# `RunAsync`: `engine::run_prepared(folder, profile, operation, options)` where `operation` forwards to `workload_runner::prepare_iteration(
    /// &executor, &files, profile, workload, iteration, &mut random, &recycle_bin, ct)` and `options` = `RunPreparedOptions::new(log, cancel)` with
    /// `progress = Some(progress)`. NOT `workload_runner::run_profile` (it also stamps `images_per_sample`, which the C# window path does not).
    /// The profile's logging switch is applied FIRST (`BenchmarkProfileScope::apply_logging(profile, || AppLog::global().enabled(), |e|
    /// AppLog::global().set_enabled(e), Some(Box::new(|| AppLog::global().enabled_write_count())))`) and the scope is dropped (restored) when
    /// `run` returns, on every path. C# restores it later, in `DisposeAsync`; the difference is not observable (process-global flag, nothing
    /// reads it in between).
    fn run(
        &mut self,
        folder: &str,
        progress: &dyn BenchmarkProgressSink,
        cancel: &CancelToken,
    ) -> Result<BenchmarkReport, BenchmarkError> {
        let log = AppLog::global();
        // Dropped (restored) when `run` returns, on every path.
        let _scope = BenchmarkProfileScope::apply_logging(
            &self.profile,
            || log.enabled(),
            move |enabled| log.set_enabled(enabled),
            Some(Box::new(move || log.enabled_write_count())),
        );
        let mut options = RunPreparedOptions::new(log, cancel);
        options.progress = Some(progress);
        let (executor, files, profile, recycle_bin) = (&self.executor, &self.files, &self.profile, &self.recycle_bin);
        let random = &mut self.random;
        let mut operation =
            |_profile: &BenchmarkProfile, workload: BenchmarkWorkload, iteration: i32, ct: &CancelToken| {
                prepare_iteration(executor, files, profile, workload, iteration, random, recycle_bin, ct)
            };
        run_prepared(folder, profile, &mut operation, &options)
    }

    /// C# `DisposeAsync`: `executor.dispose()` (starts the background scratch teardown). Idempotent (`disposed` flag).
    fn dispose(&mut self) {
        if !self.disposed {
            self.disposed = true;
            self.executor.dispose();
        }
    }
}

// ---- Private helpers ----

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "panic".to_owned()
    }
}

/// C# `BenchmarkText.Describe(Exception)` input: a profile problem keeps its shape, everything else is its message.
fn row_error_of(error: &BenchmarkError) -> RowError {
    match error {
        BenchmarkError::Profile(profile_error) => RowError::Profile(profile_error.clone()),
        other => RowError::Other(other.to_string()),
    }
}

/// `Tr.BenchStatusFailed(BenchmarkText.Describe(ex))`.
fn failed_status(error: &RowError, ctx: &FieldContext<'_>) -> String {
    ctx.texts.format_with(
        "bench.status.failed",
        &[LocArg::new("message", describe_error(error, ctx.texts))],
        ctx.numbers,
    )
}

fn count_arg(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

fn default_summary_save() -> BenchmarkSummarySave {
    Arc::new(|reports| {
        save_session(reports, &default_report_root(), DateTimeUtc::now())
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|error| error.to_string())
    })
}

/// `Path.GetTempPath()`: the temp directory with a trailing backslash.
fn temp_path_with_separator() -> String {
    let mut text = std::env::temp_dir().to_string_lossy().into_owned();
    if !text.ends_with('\\') {
        text.push('\\');
    }
    text
}

fn append_csv_line(out: &mut String, cells: &[String]) {
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        if cell.contains([',', '"', '\r', '\n']) {
            out.push('"');
            out.push_str(&cell.replace('"', "\"\""));
            out.push('"');
        } else {
            out.push_str(cell);
        }
    }
    out.push_str("\r\n");
}

/// `yyyyMMdd-HHmmss` of a UTC instant, from its invariant JSON text (`2026-10-13T08:09:10Z`).
fn timestamp_folder_name(utc: DateTimeUtc) -> String {
    let text = utc.to_json();
    let part = |from: usize, to: usize| text.get(from..to).unwrap_or("");
    format!(
        "{}{}{}-{}{}{}",
        part(0, 4),
        part(5, 7),
        part(8, 10),
        part(11, 13),
        part(14, 16),
        part(17, 19)
    )
}

/// The indented JSON array of the reports (C# `WriteIndented`): each report's own indented JSON, indented once more.
fn summary_json(report_jsons: &[String]) -> String {
    if report_jsons.is_empty() {
        return "[]".to_owned();
    }
    let items: Vec<String> = report_jsons
        .iter()
        .map(|json| {
            json.split("\r\n")
                .map(|line| format!("  {line}"))
                .collect::<Vec<_>>()
                .join("\r\n")
        })
        .collect();
    format!("[\r\n{}\r\n]", items.join(",\r\n"))
}
