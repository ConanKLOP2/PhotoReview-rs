//! Port of `PhotoReview.Benchmarking/BenchmarkEngine.cs` (C# baseline d92d6aae, 104 lines): the shared benchmark loop.
//! `run_prepared` runs `warmup_count` untimed warm-ups and then `iterations` timed samples. Each iteration is
//! prepared first (untimed) and only the returned measure step is timed (R2-F-14).
//!
//! R0003 mapping: `Task`/`async` -> blocking calls on the caller's thread (the executor does its own threading),
//! `CancellationToken` -> [`CancelToken`], exceptions -> [`BenchmarkError`], `IProgress<T>` -> [`BenchmarkProgressSink`]
//! (called synchronously on the calling thread), `TimeProvider` -> [`TimeSource`], `DateTimeOffset.UtcNow` ->
//! [`Clock`], `Environment.MachineName` -> `COMPUTERNAME`.
//!
//! Unit W7-03a. Contract: `contracts/W7-03a.md`. Depends on W7-01 (`crate::models`, `crate::metrics`).

use std::error::Error;
use std::fmt;
use std::sync::OnceLock;
use std::time::Instant;

use pr_core::abstractions::{Clock, Log, SystemClock};
use pr_core::diagnostics::metrics::ReviewMetricsSnapshot;
use pr_core::io::physical::new_guid_n;
use pr_core::sync::{CancelToken, Canceled};
use pr_core::text::dotnet_path::get_full_path;

use crate::metrics;
use crate::models::{
    BenchmarkPhaseResult, BenchmarkProfile, BenchmarkProfileError, BenchmarkReport, BenchmarkResultStatus,
    BenchmarkTimestamp, BenchmarkWorkload, validate_profile,
};

/// C# `BenchmarkEngine.ProgressOk`. Fixed English (Q-L4): also goes to the CLI.
pub const PROGRESS_OK: &str = "OK";
/// C# `BenchmarkEngine.ProgressCorrectnessFailed`.
pub const PROGRESS_CORRECTNESS_FAILED: &str = "Correctness failed";
/// C# `BenchmarkEngine.ResultCorrectnessFailed`: `BenchmarkPhaseResult.message` of a failed phase (also in the report JSON).
pub const RESULT_CORRECTNESS_FAILED: &str = "One or more samples failed correctness";

/// C# `BenchmarkProgress(ProfileId, Workload, Completed, Total, Message)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkProgress {
    pub profile_id: String,
    pub workload: BenchmarkWorkload,
    pub completed: i32,
    pub total: i32,
    pub message: String,
}

/// C# `IProgress<BenchmarkProgress>`. Called synchronously on the thread that runs [`run_prepared`], once per timed
/// sample (never for warm-ups). Any `Fn(BenchmarkProgress) + Send + Sync` closure is a sink.
pub trait BenchmarkProgressSink: Send + Sync {
    fn report(&self, progress: BenchmarkProgress);
}

impl<F: Fn(BenchmarkProgress) + Send + Sync> BenchmarkProgressSink for F {
    fn report(&self, progress: BenchmarkProgress) {
        self(progress);
    }
}

/// C# `TimeProvider.GetTimestamp` / `TimestampFrequency`, so tests drive time by hand.
pub trait TimeSource: Send + Sync {
    /// A monotonic counter in units of `1 / timestamp_frequency()` seconds.
    fn timestamp(&self) -> i64;
    /// Counter ticks per second; always > 0.
    fn timestamp_frequency(&self) -> i64;
}

/// Production [`TimeSource`] (C# `TimeProvider.System`): nanoseconds since a process-wide `Instant`
/// (`timestamp_frequency() == 1_000_000_000`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTimeSource;

impl TimeSource for SystemTimeSource {
    fn timestamp(&self) -> i64 {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        let origin = ORIGIN.get_or_init(Instant::now);
        i64::try_from(origin.elapsed().as_nanos()).unwrap_or(i64::MAX)
    }

    fn timestamp_frequency(&self) -> i64 {
        1_000_000_000
    }
}

/// C# `TimeProvider.GetElapsedTime(start, end).TotalMilliseconds`: `(end - start)` counter ticks converted to
/// 100 ns `TimeSpan` ticks with `(long)((end - start) * (10_000_000.0 / frequency))` (truncating cast), then
/// [`ticks_to_milliseconds`].
pub fn elapsed_milliseconds(frequency: i64, start: i64, end: i64) -> f64 {
    // `as` truncates toward zero like the C# `(long)` cast (and saturates on overflow, which C# leaves unspecified).
    let ticks = (end.wrapping_sub(start) as f64 * (10_000_000.0 / frequency as f64)) as i64;
    ticks_to_milliseconds(ticks)
}

/// C# `TimeSpan.FromTicks(ticks).TotalMilliseconds`: `ticks as f64 / 10_000.0` clamped to
/// `[i64::MIN / 10_000, i64::MAX / 10_000]` as whole numbers (`-922337203685477.0 ..= 922337203685477.0`, as .NET does).
pub fn ticks_to_milliseconds(ticks: i64) -> f64 {
    const MAX_MILLISECONDS: f64 = 922_337_203_685_477.0;
    (ticks as f64 / 10_000.0).clamp(-MAX_MILLISECONDS, MAX_MILLISECONDS)
}

/// C# private `Ms(double)`: `value.ToString("F1", InvariantCulture)`. Verified on .NET 10: ties round to even on the
/// exact binary value, so it equals Rust `format!("{value:.1}")` (`0.25` -> `0.2`, `1.75` -> `1.8`).
pub fn format_ms(value: f64) -> String {
    format!("{value:.1}")
}

/// What one measure step returns: C# `(bool Correct, ReviewMetricsSnapshot? Metrics)`. `metrics` is the executor's
/// CUMULATIVE snapshot, not a delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasureOutcome {
    pub correct: bool,
    pub metrics: Option<ReviewMetricsSnapshot>,
}

/// The timed half of one iteration (the `Func<Task<(bool, ReviewMetricsSnapshot?)>>` the C# executor returns).
/// Run exactly once, on the thread that called [`run_prepared`]. Not `Send` (the executor may capture non-`Send` state).
pub type PreparedMeasure = Box<dyn FnOnce() -> Result<MeasureOutcome, BenchmarkError>>;

/// C# `BenchmarkPreparedWorkloadExecutor(profile, workload, iteration, ct)`. The call is the UNTIMED prepare step
/// (temp-file copies, cache eviction) and returns the measure step. `iteration` is the warm-up index for warm-ups
/// and the sample index for timed runs, both from 0.
pub type PreparedWorkloadExecutor<'a> =
    dyn FnMut(&BenchmarkProfile, BenchmarkWorkload, i32, &CancelToken) -> Result<PreparedMeasure, BenchmarkError> + 'a;

/// Everything `run_prepared` can fail with. `Display` texts equal the C# exception `Message`s.
#[derive(Debug)]
pub enum BenchmarkError {
    /// C# `ArgumentException("Folder is required", "folder")` (blank or whitespace folder).
    /// `Display`: `Folder is required (Parameter 'folder')`.
    FolderRequired,
    /// C# `DirectoryNotFoundException(folder)`; the payload is the folder text as given, which is also the message.
    DirectoryNotFound(String),
    /// C# `BenchmarkProfileException` from `BenchmarkProfileValidation.Validate`.
    Profile(BenchmarkProfileError),
    /// C# `OperationCanceledException`. `Display`: `The operation was canceled.`
    Canceled,
    /// Any other exception thrown by the executor's prepare or measure step.
    Operation(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for BenchmarkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FolderRequired => f.write_str("Folder is required (Parameter 'folder')"),
            Self::DirectoryNotFound(path) => f.write_str(path),
            Self::Profile(e) => fmt::Display::fmt(e, f),
            Self::Canceled => f.write_str("The operation was canceled."),
            Self::Operation(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl Error for BenchmarkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Profile(e) => Some(e),
            Self::Operation(e) => Some(&**e),
            Self::FolderRequired | Self::DirectoryNotFound(_) | Self::Canceled => None,
        }
    }
}

impl From<Canceled> for BenchmarkError {
    fn from(_: Canceled) -> Self {
        BenchmarkError::Canceled
    }
}

impl From<BenchmarkProfileError> for BenchmarkError {
    fn from(error: BenchmarkProfileError) -> Self {
        BenchmarkError::Profile(error)
    }
}

/// The optional arguments of C# `RunPreparedAsync` plus the nondeterministic inputs made injectable.
#[derive(Clone, Copy)]
pub struct RunPreparedOptions<'a> {
    /// C# `progress` (null = none).
    pub progress: Option<&'a dyn BenchmarkProgressSink>,
    /// C# `timeProvider` (default `TimeProvider.System`).
    pub time: &'a dyn TimeSource,
    /// C# `DateTimeOffset.UtcNow` for `BenchmarkReport.started_utc`.
    pub clock: &'a dyn Clock,
    /// C# `log` (default `FileLog.Default`: the CLI/window pass their file log).
    pub log: &'a dyn Log,
    /// C# `cancellationToken`.
    pub cancel: &'a CancelToken,
    /// `None`: a fresh `new_guid_n()` (32 lowercase hex digits, C# `Guid.NewGuid().ToString("N")`). Tests pin it.
    pub run_id: Option<&'a str>,
    /// `None`: the `COMPUTERNAME` environment variable (`None` in the report when unset). Tests pin it.
    pub machine: Option<&'a str>,
}

impl fmt::Debug for RunPreparedOptions<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunPreparedOptions")
            .field("run_id", &self.run_id)
            .field("machine", &self.machine)
            .finish_non_exhaustive()
    }
}

impl<'a> RunPreparedOptions<'a> {
    /// Production defaults: no progress, system time and clock, the given log and token.
    pub fn new(log: &'a dyn Log, cancel: &'a CancelToken) -> Self {
        static SYSTEM_TIME: SystemTimeSource = SystemTimeSource;
        static SYSTEM_CLOCK: SystemClock = SystemClock;
        Self {
            progress: None,
            time: &SYSTEM_TIME,
            clock: &SYSTEM_CLOCK,
            log,
            cancel,
            run_id: None,
            machine: None,
        }
    }
}

/// C# `BenchmarkEngine.RunPreparedAsync(folder, profile, operation, progress, timeProvider, log, cancellationToken)`.
///
/// Order of effects (see the contract for the exact log lines): validate folder text, folder exists, profile
/// (`validate_profile`) -> `started = clock.utc_now()`, run id -> `Benchmark start` log -> warm-ups -> timed
/// iterations -> metrics baseline subtraction -> one-phase report.
pub fn run_prepared(
    folder: &str,
    profile: &BenchmarkProfile,
    operation: &mut PreparedWorkloadExecutor<'_>,
    options: &RunPreparedOptions<'_>,
) -> Result<BenchmarkReport, BenchmarkError> {
    let (log, cancel, time) = (options.log, options.cancel, options.time);
    if folder.chars().all(char::is_whitespace) {
        return Err(BenchmarkError::FolderRequired);
    }
    if !std::fs::metadata(folder).map(|m| m.is_dir()).unwrap_or(false) {
        return Err(BenchmarkError::DirectoryNotFound(folder.to_owned()));
    }
    validate_profile(profile)?;

    let started = options.clock.utc_now();
    let run_id = options.run_id.map_or_else(new_guid_n, str::to_owned);
    // DIVERGENCE: unreachable after the directory check except for paths beyond MAX_PATH; reported as not found.
    let full = get_full_path(folder).map_err(|_| BenchmarkError::DirectoryNotFound(folder.to_owned()))?;
    let id = profile.id.as_str();
    let workload = profile.workload;
    log.info(&format!(
        "Benchmark start runId={run_id} profile={id} workload={} folder={full} iterations={} workers={}",
        workload.name(),
        profile.iterations,
        profile.workers
    ));

    let mut samples: Vec<f64> = Vec::new();
    let mut metrics: Option<ReviewMetricsSnapshot> = None;
    let mut all_correct = true;
    let total = profile.iterations;
    // Executors report cumulative metrics; the last warm-up's snapshot is the baseline subtracted from the final one,
    // so the report's reads/hits cover the measured iterations only.
    let mut warmup_baseline: Option<ReviewMetricsSnapshot> = None;

    for w in 0..profile.warmup_count {
        // Outside the failure handling: a canceled token at the loop top returns without a log line.
        cancel.check()?;
        let attempt = operation(profile, workload, w, cancel).and_then(|measure| measure());
        match attempt {
            Ok(outcome) => warmup_baseline = outcome.metrics.or(warmup_baseline),
            Err(BenchmarkError::Canceled) => {
                log.info(&format!("Benchmark canceled runId={run_id} profile={id} warmup={w}"));
                return Err(BenchmarkError::Canceled);
            }
            Err(error) => {
                log.error(
                    &format!("Benchmark warmup failed runId={run_id} profile={id} warmup={w}"),
                    Some(&error),
                );
                return Err(error);
            }
        }
    }

    // `iterations` counts only timed samples; warm-ups are additional and excluded from the samples.
    for i in 0..total {
        cancel.check()?;
        let attempt = operation(profile, workload, i, cancel).and_then(|measure| {
            let t0 = time.timestamp();
            let outcome = measure()?;
            // The end timestamp is read right after the measure step returns, before anything else.
            let elapsed = elapsed_milliseconds(time.timestamp_frequency(), t0, time.timestamp());
            Ok((outcome, elapsed))
        });
        let (outcome, elapsed_ms) = match attempt {
            Ok(done) => done,
            Err(BenchmarkError::Canceled) => {
                log.info(&format!("Benchmark canceled runId={run_id} profile={id} iteration={i}"));
                return Err(BenchmarkError::Canceled);
            }
            Err(error) => {
                log.error(
                    &format!("Benchmark phase failed runId={run_id} profile={id} iteration={i}"),
                    Some(&error),
                );
                return Err(error);
            }
        };
        samples.push(elapsed_ms);
        metrics = outcome.metrics.or(metrics);
        all_correct &= outcome.correct;
        log.info(&format!(
            "Benchmark sample runId={run_id} profile={id} phase={} iteration={}/{total} elapsedMs={} correct={}",
            workload.name(),
            i + 1,
            format_ms(elapsed_ms),
            if outcome.correct { "True" } else { "False" }
        ));
        if let Some(sink) = options.progress {
            sink.report(BenchmarkProgress {
                profile_id: profile.id.clone(),
                workload,
                completed: i + 1,
                total,
                message: if outcome.correct {
                    PROGRESS_OK
                } else {
                    PROGRESS_CORRECTNESS_FAILED
                }
                .to_owned(),
            });
        }
    }

    if let (Some(current), Some(baseline)) = (&metrics, &warmup_baseline) {
        // A duplicate baseline key throws in C# (`ToDictionary`) and escapes unlogged.
        let delta = metrics::since(current, baseline).map_err(|e| BenchmarkError::Operation(Box::new(e)))?;
        metrics = Some(delta);
    }
    let correct = !samples.is_empty() && all_correct;
    let status = if samples.is_empty() {
        BenchmarkResultStatus::InsufficientData
    } else if correct {
        BenchmarkResultStatus::Pass
    } else {
        BenchmarkResultStatus::Fail
    };
    let phase = BenchmarkPhaseResult::new(
        profile.id.clone(),
        workload,
        samples,
        status,
        if correct {
            None
        } else {
            Some(RESULT_CORRECTNESS_FAILED.to_owned())
        },
        None,
    );
    log.info(&format!(
        "Benchmark complete runId={run_id} profile={id} phase={} status={} p50Ms={} p95Ms={} maxMs={}",
        workload.name(),
        phase.status.name(),
        format_ms(phase.p50()),
        format_ms(phase.p95()),
        format_ms(phase.max())
    ));
    Ok(BenchmarkReport {
        run_id,
        started_utc: BenchmarkTimestamp::new(started, 0).unwrap_or_else(BenchmarkTimestamp::now_utc),
        folder: full,
        phases: vec![phase],
        metrics,
        machine: options
            .machine
            .map(str::to_owned)
            .or_else(|| std::env::var("COMPUTERNAME").ok()),
    })
}
