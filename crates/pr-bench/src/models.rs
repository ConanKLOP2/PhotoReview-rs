//! Port of `PhotoReview.Benchmarking/BenchmarkModels.cs` (C# baseline d92d6aae, 146 lines): the benchmark data
//! model (profile, workload and status enums, phase result with its statistics, report, dataset manifest, batch
//! summary), the profile validation and the per-image ranking.
//!
//! Unit W7-01a. Contract: `contracts/W7-01a.md`.
//!
//! Seams that the C# file reaches through other classes (so this module stays free of cycles):
//! * `BenchmarkProfiles.Find` + `BenchmarkWorkloadRunner.ImagesPerSample` (W7-01b / W7-03) are reached through the
//!   [`ImagesPerSampleLookup`] callback that every function needing the derived image count takes.
//! * `BenchmarkStatistics.Percentile` is `pr_core::diagnostics::bench_stats::percentile` (W1-15, merged).
//! * The `ReviewMetricsSnapshot` JSON is the `serde::Serialize` impl of W2-17 (merged).

use std::cmp::Ordering;
use std::fmt::{self, Write as _};
use std::sync::OnceLock;

use pr_core::diagnostics::metrics::ReviewMetricsSnapshot;
use pr_core::model::enums::LoadingMode;
use pr_core::model::serde_ext::LenientEnum;
use pr_core::sync::{CancelToken, Canceled};
use pr_core::time::DateTimeUtc;
use pr_perf::report::json_string_literal;
use pr_perf::rules::fmt_double;

/// Default of `BenchmarkProfile.WarmupCount`.
pub const DEFAULT_WARMUP_COUNT: i32 = 1;
/// Default of `BenchmarkProfile.Iterations`.
pub const DEFAULT_ITERATIONS: i32 = 30;

/// Callback standing for `BenchmarkProfiles.Find(id) is { } p ? BenchmarkWorkloadRunner.ImagesPerSample(p, workload,
/// int.MaxValue) : 1`: `Some(images)` when the registry knows `profile_id` (OrdinalIgnoreCase), `None` otherwise.
/// It receives the PHASE's workload, not the profile's own.
pub type ImagesPerSampleLookup<'a> = &'a dyn Fn(&str, BenchmarkWorkload) -> Option<i32>;

/// C# `BenchmarkWorkload`. The numeric values are written into report JSON (no string converter on this enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum BenchmarkWorkload {
    FirstFrame = 0,
    Sequential = 1,
    Random = 2,
    WarmNext = 3,
    Preload = 4,
    FileAction = 5,
    Correctness = 6,
}

impl BenchmarkWorkload {
    pub const ALL: [Self; 7] = [
        Self::FirstFrame,
        Self::Sequential,
        Self::Random,
        Self::WarmNext,
        Self::Preload,
        Self::FileAction,
        Self::Correctness,
    ];

    /// The C# member name (`ToString()`), e.g. `"FirstFrame"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::FirstFrame => "FirstFrame",
            Self::Sequential => "Sequential",
            Self::Random => "Random",
            Self::WarmNext => "WarmNext",
            Self::Preload => "Preload",
            Self::FileAction => "FileAction",
            Self::Correctness => "Correctness",
        }
    }

    pub fn value(self) -> i32 {
        self as i32
    }

    /// Exact (case-sensitive) member name, as `Enum.Parse` with a valid name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.name() == name)
    }
}

/// C# `BenchmarkResultStatus`. Written as a number in report JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum BenchmarkResultStatus {
    Pass = 0,
    Warn = 1,
    Fail = 2,
    InsufficientData = 3,
}

impl BenchmarkResultStatus {
    pub const ALL: [Self; 4] = [Self::Pass, Self::Warn, Self::Fail, Self::InsufficientData];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pass => "Pass",
            Self::Warn => "Warn",
            Self::Fail => "Fail",
            Self::InsufficientData => "InsufficientData",
        }
    }

    pub fn value(self) -> i32 {
        self as i32
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// C# `BenchmarkProfile` record. `Id` is never null in Rust: a C# null id behaves like `""` everywhere it is used
/// (`TargetWidth`, `Validate`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub loading_mode: LoadingMode,
    pub workers: i32,
    pub next_window: i32,
    pub previous_window: i32,
    pub full_folder: bool,
    pub memory_reserve_bytes: i64,
    pub disk_cache: bool,
    pub detailed_logging: bool,
    pub workload: BenchmarkWorkload,
    /// C# default 1 ([`DEFAULT_WARMUP_COUNT`]). Not checked by [`validate_profile`].
    pub warmup_count: i32,
    /// C# default 30 ([`DEFAULT_ITERATIONS`]).
    pub iterations: i32,
    /// C# default `false`.
    pub correctness_only: bool,
}

impl BenchmarkProfile {
    /// `BenchmarkProfileExtensions.TargetWidth`: `preview-light` 960, `preview-balanced` 1600, `preview-quality` 2400,
    /// `preview-high-quality` 3200, `original-correctness` 0, anything else 1920. The id match is ORDINAL (case-sensitive).
    pub fn target_width(&self) -> i32 {
        match self.id.as_str() {
            "preview-light" => 960,
            "preview-balanced" => 1600,
            "preview-quality" => 2400,
            "preview-high-quality" => 3200,
            "original-correctness" => 0,
            _ => 1920,
        }
    }
}

/// C# `BenchmarkProfileProblem` (stable codes; the window maps them to catalog text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum BenchmarkProfileProblem {
    MissingIdOrMode = 0,
    InvalidSettings = 1,
    /// The profile has no real check yet and refuses to run (raised by the runner, W7-03, not by [`validate_profile`]).
    NotImplemented = 2,
}

impl BenchmarkProfileProblem {
    pub fn name(self) -> &'static str {
        match self {
            Self::MissingIdOrMode => "MissingIdOrMode",
            Self::InvalidSettings => "InvalidSettings",
            Self::NotImplemented => "NotImplemented",
        }
    }
}

/// C# `BenchmarkProfileException : ArgumentException`. `message` stays English (CLI, logs, reports); `problem` and
/// `profile_id` let the UI show translated text. `Display` prints `message` only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkProfileError {
    pub problem: BenchmarkProfileProblem,
    pub profile_id: String,
    pub message: String,
}

impl BenchmarkProfileError {
    pub fn new(problem: BenchmarkProfileProblem, profile_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            problem,
            profile_id: profile_id.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for BenchmarkProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BenchmarkProfileError {}

/// C# `BenchmarkProfileValidation.Validate`.
///
/// 1. id empty or only whitespace (`string.IsNullOrWhiteSpace`) -> `MissingIdOrMode`, `profile_id` = the id,
///    message `"Profile id and loading mode are required"`. (A `LoadingMode` outside the enum cannot exist in Rust.)
/// 2. else `workers < 1 || next_window < 0 || previous_window < 0 || iterations < 1` -> `InvalidSettings`,
///    message `"Invalid benchmark settings for profile '{id}'"`.
/// 3. else `Ok(())`. `warmup_count` and everything else are not checked.
pub fn validate_profile(profile: &BenchmarkProfile) -> Result<(), BenchmarkProfileError> {
    // `char::is_whitespace` and .NET `char.IsWhiteSpace` agree on every code point (the golden pins this).
    if profile.id.chars().all(char::is_whitespace) {
        return Err(BenchmarkProfileError::new(
            BenchmarkProfileProblem::MissingIdOrMode,
            profile.id.clone(),
            "Profile id and loading mode are required",
        ));
    }
    if profile.workers < 1 || profile.next_window < 0 || profile.previous_window < 0 || profile.iterations < 1 {
        return Err(BenchmarkProfileError::new(
            BenchmarkProfileProblem::InvalidSettings,
            profile.id.clone(),
            format!("Invalid benchmark settings for profile '{}'", profile.id),
        ));
    }
    Ok(())
}

/// C# `Func<Task<(bool Correct, ReviewMetricsSnapshot? Metrics)>>` result: what one measure step returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IterationOutcome {
    pub correct: bool,
    pub metrics: Option<ReviewMetricsSnapshot>,
}

/// Failure of a prepare or measure step. `Canceled` stands for `OperationCanceledException` (the engine logs it as a
/// cancellation and rethrows); `Failed` is any other exception (the engine records a Fail phase with this text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorError {
    Canceled,
    Failed(String),
}

impl From<Canceled> for ExecutorError {
    fn from(_: Canceled) -> Self {
        Self::Canceled
    }
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canceled => f.write_str("The operation was canceled."),
            Self::Failed(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for ExecutorError {}

/// The timed half of one iteration (C# `Func<Task<(bool, ReviewMetricsSnapshot?)>>`). Invoked exactly once.
pub type MeasureStep = Box<dyn FnOnce() -> Result<IterationOutcome, ExecutorError> + Send>;

/// C# `BenchmarkPreparedWorkloadExecutor` delegate: `prepare` does the untimed setup of one iteration (copy a scratch
/// file, evict caches) and returns the measure step; only that step is timed (R2-F-14).
pub trait BenchmarkPreparedWorkloadExecutor: Send + Sync {
    fn prepare(
        &self,
        profile: &BenchmarkProfile,
        workload: BenchmarkWorkload,
        iteration: i32,
        cancel: &CancelToken,
    ) -> Result<MeasureStep, ExecutorError>;
}

/// C# `BenchmarkPhaseResult` record. `samples` is private so the sorted cache can never go stale; every "with" copy
/// goes through a `with_*` method that starts with an EMPTY cache (the C# explicit copy constructor exists for exactly
/// this: `phase with { Samples = ... }` must answer from the new samples).
#[derive(Debug, Clone)]
pub struct BenchmarkPhaseResult {
    pub profile_id: String,
    pub workload: BenchmarkWorkload,
    samples: Vec<f64>,
    pub status: BenchmarkResultStatus,
    pub message: Option<String>,
    /// Images one sample decodes, stamped by the runner; `None` = derive from the registry.
    pub images_per_sample: Option<i32>,
    /// Samples sorted like `Samples.OrderBy(x => x)` (.NET `double.CompareTo`: NaN first, `-0.0 == 0.0`, stable).
    sorted: OnceLock<Vec<f64>>,
}

impl PartialEq for BenchmarkPhaseResult {
    /// Field equality (the cache is ignored). DIVERGENCE: C# record equality compares `Samples` by reference.
    fn eq(&self, other: &Self) -> bool {
        self.profile_id == other.profile_id
            && self.workload == other.workload
            && self.samples == other.samples
            && self.status == other.status
            && self.message == other.message
            && self.images_per_sample == other.images_per_sample
    }
}

impl BenchmarkPhaseResult {
    /// `new BenchmarkPhaseResult(profileId, workload, samples, status, message, imagesPerSample)`.
    pub fn new(
        profile_id: impl Into<String>,
        workload: BenchmarkWorkload,
        samples: Vec<f64>,
        status: BenchmarkResultStatus,
        message: Option<String>,
        images_per_sample: Option<i32>,
    ) -> Self {
        Self {
            profile_id: profile_id.into(),
            workload,
            samples,
            status,
            message,
            images_per_sample,
            sorted: OnceLock::new(),
        }
    }

    pub fn samples(&self) -> &[f64] {
        &self.samples
    }

    /// `phase with { Samples = samples }`: a copy whose statistics come from the NEW samples.
    pub fn with_samples(&self, samples: Vec<f64>) -> Self {
        Self::new(
            self.profile_id.clone(),
            self.workload,
            samples,
            self.status,
            self.message.clone(),
            self.images_per_sample,
        )
    }

    /// `phase with { ImagesPerSample = images }` (the runner's stamp, W7-03).
    pub fn with_images_per_sample(&self, images_per_sample: Option<i32>) -> Self {
        Self::new(
            self.profile_id.clone(),
            self.workload,
            self.samples.clone(),
            self.status,
            self.message.clone(),
            images_per_sample,
        )
    }

    /// `Samples.Count`.
    pub fn count(&self) -> i32 {
        i32::try_from(self.samples.len()).unwrap_or(i32::MAX)
    }

    /// The samples ordered like .NET `Samples.OrderBy(x => x)`, built at most once per instance.
    fn sorted(&self) -> &[f64] {
        self.sorted.get_or_init(|| {
            let mut copy = self.samples.clone();
            copy.sort_by(|a, b| cmp_dotnet(*a, *b));
            copy
        })
    }

    fn percentile_of(&self, p: f64) -> f64 {
        // The percentiles used are in range, so the `Err` is unreachable; map it to 0 without unwrapping.
        pr_core::diagnostics::bench_stats::percentile(self.sorted(), p).unwrap_or(0.0)
    }

    /// `BenchmarkStatistics.Percentile(sorted, .50)` (0 when there are no samples).
    pub fn p50(&self) -> f64 {
        self.percentile_of(0.50)
    }

    /// Percentile .95.
    pub fn p95(&self) -> f64 {
        self.percentile_of(0.95)
    }

    /// Percentile .99.
    pub fn p99(&self) -> f64 {
        self.percentile_of(0.99)
    }

    /// `Samples.Count == 0 ? 0 : sorted[^1]` (the sign of a zero maximum follows the stable .NET sort).
    pub fn max(&self) -> f64 {
        self.sorted().last().copied().unwrap_or(0.0)
    }

    /// `Math.Max(1, ImagesPerSample ?? lookup(ProfileId, Workload) ?? 1)`. The stamp wins (the lookup is not called
    /// when it is `Some`, even `Some(0)` or `Some(-7)`, which both give 1).
    pub fn effective_images_per_sample(&self, lookup: ImagesPerSampleLookup<'_>) -> i32 {
        match self.images_per_sample {
            Some(stamp) => stamp.max(1),
            None => lookup(&self.profile_id, self.workload).unwrap_or(1).max(1),
        }
    }

    /// `P95 / EffectiveImagesPerSample` (f64 division).
    pub fn p95_per_image(&self, lookup: ImagesPerSampleLookup<'_>) -> f64 {
        self.p95() / f64::from(self.effective_images_per_sample(lookup))
    }

    /// `P50 / EffectiveImagesPerSample`.
    pub fn p50_per_image(&self, lookup: ImagesPerSampleLookup<'_>) -> f64 {
        self.p50() / f64::from(self.effective_images_per_sample(lookup))
    }
}

/// C# `BenchmarkRanking.Rank`: keeps phases of `workload` whose status is neither Fail nor InsufficientData (Pass and
/// Warn stay) and whose per-image P95 and P50 are both finite, ordered by per-image P95 then per-image P50 ascending,
/// STABLE (ties keep input order). Returns references into `phases`.
pub fn rank<'a>(
    phases: &'a [BenchmarkPhaseResult],
    workload: BenchmarkWorkload,
    lookup: ImagesPerSampleLookup<'_>,
) -> Vec<&'a BenchmarkPhaseResult> {
    // Each key is computed once per kept element, not inside the comparator.
    let mut keyed: Vec<(f64, f64, &'a BenchmarkPhaseResult)> = phases
        .iter()
        .filter(|p| {
            p.workload == workload
                && p.status != BenchmarkResultStatus::Fail
                && p.status != BenchmarkResultStatus::InsufficientData
        })
        .filter_map(|p| {
            let p95 = p.p95_per_image(lookup);
            let p50 = p.p50_per_image(lookup);
            (p95.is_finite() && p50.is_finite()).then_some((p95, p50, p))
        })
        .collect();
    // Finite keys make `partial_cmp` total; `sort_by` is stable so ties keep input order.
    keyed.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal))
    });
    keyed.into_iter().map(|(_, _, p)| p).collect()
}

/// .NET `double.CompareTo`: NaN before every number, NaN equal to NaN, `-0.0` equal to `0.0`.
fn cmp_dotnet(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

const TICKS_PER_MINUTE: i64 = 600_000_000;

/// A `DateTimeOffset`: a UTC instant plus the offset it prints with. Only the text form matters here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchmarkTimestamp {
    utc: DateTimeUtc,
    offset_minutes: i32,
}

impl BenchmarkTimestamp {
    /// `None` when `offset_minutes` is outside +-840 (14 h) or the local time falls outside the `DateTime` range.
    pub fn new(utc: DateTimeUtc, offset_minutes: i32) -> Option<Self> {
        if !(-840..=840).contains(&offset_minutes) {
            return None;
        }
        // The local time must stay inside the `DateTime` range.
        utc.checked_add_ticks(i64::from(offset_minutes) * TICKS_PER_MINUTE)?;
        Some(Self { utc, offset_minutes })
    }

    /// `DateTimeOffset.UtcNow` (offset 0).
    pub fn now_utc() -> Self {
        Self {
            utc: DateTimeUtc::now(),
            offset_minutes: 0,
        }
    }

    /// `DateTimeOffset.UnixEpoch`.
    pub fn unix_epoch() -> Self {
        Self {
            utc: DateTimeUtc::from_ticks(621_355_968_000_000_000).unwrap_or(DateTimeUtc::MIN),
            offset_minutes: 0,
        }
    }

    pub fn utc(self) -> DateTimeUtc {
        self.utc
    }

    pub fn offset_minutes(self) -> i32 {
        self.offset_minutes
    }

    /// System.Text.Json's `DateTimeOffset` text: local time `yyyy-MM-ddTHH:mm:ss[.fffffff]` (fraction trimmed of
    /// trailing zeros, absent when zero) followed by `+hh:mm` / `-hh:mm` (always, `+00:00` for UTC, never `Z`).
    pub fn to_json_text(self) -> String {
        let local = self
            .utc
            .checked_add_ticks(i64::from(self.offset_minutes) * TICKS_PER_MINUTE)
            .unwrap_or(self.utc);
        let text = local.to_json();
        let mut out = text.strip_suffix('Z').unwrap_or(&text).to_owned();
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let abs = self.offset_minutes.unsigned_abs();
        let _ = write!(out, "{sign}{:02}:{:02}", abs / 60, abs % 60);
        out
    }
}

/// C# `BenchmarkReport` record.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkReport {
    pub run_id: String,
    pub started_utc: BenchmarkTimestamp,
    pub folder: String,
    pub phases: Vec<BenchmarkPhaseResult>,
    pub metrics: Option<ReviewMetricsSnapshot>,
    pub machine: Option<String>,
}

/// `to_json` could not write a number: C# `Utf8JsonWriter` throws `ArgumentException` for NaN and +-Infinity
/// (a phase with such samples cannot be serialized; the derived statistics are then non-finite too).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonFiniteNumber;

impl fmt::Display for NonFiniteNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a benchmark JSON number is NaN or infinite")
    }
}

impl std::error::Error for NonFiniteNumber {}

impl BenchmarkReport {
    /// C# `ToJson()` (source-generated, `WriteIndented = true`). Byte layout: see contract "Semantics notes" 6-8.
    /// Newlines are CRLF (what System.Text.Json writes on Windows, README D9).
    pub fn to_json(&self, lookup: ImagesPerSampleLookup<'_>) -> Result<String, NonFiniteNumber> {
        Ok(render(&report_node(self, lookup)?))
    }
}

/// C# `BenchmarkDatasetManifest` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkDatasetManifest {
    pub folder: String,
    pub files: Vec<String>,
    /// Extension -> count, in insertion order (C# dictionary enumeration order).
    pub unsupported_extensions: Vec<(String, i32)>,
    pub file_limit: i32,
}

impl BenchmarkDatasetManifest {
    /// `Files.Count`.
    pub fn supported_file_count(&self) -> i32 {
        i32::try_from(self.files.len()).unwrap_or(i32::MAX)
    }

    /// `UnsupportedExtensions.Values.Sum()`. DIVERGENCE: C# throws `OverflowException` past `int.MaxValue`; this
    /// saturates (unreachable for a folder listing).
    pub fn unsupported_file_count(&self) -> i32 {
        let sum: i64 = self.unsupported_extensions.iter().map(|(_, n)| i64::from(*n)).sum();
        i32::try_from(sum).unwrap_or(if sum < 0 { i32::MIN } else { i32::MAX })
    }
}

/// C# `BenchmarkBatchSummary` record.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkBatchSummary {
    pub dataset: BenchmarkDatasetManifest,
    pub reports: Vec<BenchmarkReport>,
    pub profile_outcomes: Vec<BenchmarkPhaseResult>,
}

impl BenchmarkBatchSummary {
    /// C# `ToJson()` (reflection serializer, `WriteIndented = true`, default options). Same layout rules as
    /// [`BenchmarkReport::to_json`]; `Reports[i].StartedUtc` etc. use the report layout.
    pub fn to_json(&self, lookup: ImagesPerSampleLookup<'_>) -> Result<String, NonFiniteNumber> {
        let dataset = &self.dataset;
        let dataset_node = Json::Object(vec![
            field("Folder", Json::Str(dataset.folder.clone())),
            field(
                "Files",
                Json::Array(dataset.files.iter().map(|f| Json::Str(f.clone())).collect()),
            ),
            field(
                "UnsupportedExtensions",
                Json::Object(
                    dataset
                        .unsupported_extensions
                        .iter()
                        .map(|(ext, n)| (ext.clone(), int(i64::from(*n))))
                        .collect(),
                ),
            ),
            field("FileLimit", int(i64::from(dataset.file_limit))),
            field("SupportedFileCount", int(i64::from(dataset.supported_file_count()))),
            field("UnsupportedFileCount", int(i64::from(dataset.unsupported_file_count()))),
        ]);
        let reports = self
            .reports
            .iter()
            .map(|r| report_node(r, lookup))
            .collect::<Result<Vec<_>, _>>()?;
        let outcomes = self
            .profile_outcomes
            .iter()
            .map(|p| phase_node(p, lookup))
            .collect::<Result<Vec<_>, _>>()?;
        let root = Json::Object(vec![
            field("Dataset", dataset_node),
            field("Reports", Json::Array(reports)),
            field("ProfileOutcomes", Json::Array(outcomes)),
        ]);
        Ok(render(&root))
    }
}

// ---- JSON writer (System.Text.Json layout: 2-space indent, CRLF, `R` numbers, default encoder escaping) ------------

enum Json {
    Null,
    /// Token text written as is (integers, `fmt_double` output).
    Raw(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

fn int(value: i64) -> Json {
    Json::Raw(value.to_string())
}

/// A double property: non-finite values make `Utf8JsonWriter` throw in C#.
fn double(value: f64) -> Result<Json, NonFiniteNumber> {
    if value.is_finite() {
        Ok(Json::Raw(fmt_double(value)))
    } else {
        Err(NonFiniteNumber)
    }
}

fn opt_str(value: &Option<String>) -> Json {
    value.as_ref().map_or(Json::Null, |s| Json::Str(s.clone()))
}

fn field(name: &str, value: Json) -> (String, Json) {
    (name.to_owned(), value)
}

fn phase_node(phase: &BenchmarkPhaseResult, lookup: ImagesPerSampleLookup<'_>) -> Result<Json, NonFiniteNumber> {
    let samples = phase
        .samples()
        .iter()
        .map(|s| double(*s))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json::Object(vec![
        field("ProfileId", Json::Str(phase.profile_id.clone())),
        field("Workload", int(i64::from(phase.workload.value()))),
        field("Samples", Json::Array(samples)),
        field("Status", int(i64::from(phase.status.value()))),
        field("Message", opt_str(&phase.message)),
        field(
            "ImagesPerSample",
            phase.images_per_sample.map_or(Json::Null, |n| int(i64::from(n))),
        ),
        field("Count", int(i64::from(phase.count()))),
        field("P50", double(phase.p50())?),
        field("P95", double(phase.p95())?),
        field("P99", double(phase.p99())?),
        field("Max", double(phase.max())?),
        field(
            "EffectiveImagesPerSample",
            int(i64::from(phase.effective_images_per_sample(lookup))),
        ),
        field("P95PerImage", double(phase.p95_per_image(lookup))?),
        field("P50PerImage", double(phase.p50_per_image(lookup))?),
    ]))
}

fn metrics_node(m: &ReviewMetricsSnapshot) -> Json {
    let fallbacks = m
        .decoder_fallbacks
        .iter()
        .map(|(backend, n)| (backend.name().to_owned(), int(*n)))
        .collect();
    let top = m
        .top_source_opens
        .iter()
        .map(|e| {
            Json::Object(vec![
                field("Path", Json::Str(e.path.clone())),
                field("Count", int(e.count)),
            ])
        })
        .collect();
    let histogram = m
        .present_histogram
        .iter()
        .map(|b| {
            Json::Object(vec![
                field("Label", Json::Str(b.label.clone())),
                field("Count", int(b.count)),
            ])
        })
        .collect();
    Json::Object(vec![
        field("CacheHits", int(m.cache_hits)),
        field("CacheMisses", int(m.cache_misses)),
        field("SourceBytesRead", int(m.source_bytes_read)),
        field("SourceReads", int(m.source_reads)),
        field("DecodeMilliseconds", int(m.decode_milliseconds)),
        field("PresentedImages", int(m.presented_images)),
        field("PresentMilliseconds", int(m.present_milliseconds)),
        field("PreloadHits", int(m.preload_hits)),
        field("InflightJoins", int(m.inflight_joins)),
        field("DiskCacheHits", int(m.disk_cache_hits)),
        field("UiAssignMilliseconds", int(m.ui_assign_milliseconds)),
        field("DecoderFallbacks", Json::Object(fallbacks)),
        field("DecoderFallbackCount", int(m.decoder_fallback_count())),
        field("SourceOpenCount", int(m.source_open_count)),
        field("TopSourceOpens", Json::Array(top)),
        field("StatCount", int(m.stat_count)),
        field("SessionWriteCount", int(m.session_write_count)),
        field("CrossThreadPresentCount", int(m.cross_thread_present_count)),
        field("PresentHistogram", Json::Array(histogram)),
    ])
}

fn report_node(report: &BenchmarkReport, lookup: ImagesPerSampleLookup<'_>) -> Result<Json, NonFiniteNumber> {
    let phases = report
        .phases
        .iter()
        .map(|p| phase_node(p, lookup))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json::Object(vec![
        field("RunId", Json::Str(report.run_id.clone())),
        // DateTimeOffset text is written by its converter: quoted, but not run through the string encoder (`+` stays).
        field(
            "StartedUtc",
            Json::Raw(format!("\"{}\"", report.started_utc.to_json_text())),
        ),
        field("Folder", Json::Str(report.folder.clone())),
        field("Phases", Json::Array(phases)),
        field("Metrics", report.metrics.as_ref().map_or(Json::Null, metrics_node)),
        field("Machine", opt_str(&report.machine)),
    ]))
}

fn render(node: &Json) -> String {
    let mut out = String::new();
    write_node(&mut out, node, 0);
    out
}

fn newline(out: &mut String, depth: usize) {
    out.push_str("\r\n");
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_node(out: &mut String, node: &Json, depth: usize) {
    match node {
        Json::Null => out.push_str("null"),
        Json::Raw(text) => out.push_str(text),
        Json::Str(text) => out.push_str(&json_string_literal(text)),
        Json::Array(items) if items.is_empty() => out.push_str("[]"),
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_node(out, item, depth + 1);
            }
            newline(out, depth);
            out.push(']');
        }
        Json::Object(props) if props.is_empty() => out.push_str("{}"),
        Json::Object(props) => {
            out.push('{');
            for (i, (name, value)) in props.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                out.push_str(&json_string_literal(name));
                out.push_str(": ");
                write_node(out, value, depth + 1);
            }
            newline(out, depth);
            out.push('}');
        }
    }
}
