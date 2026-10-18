//! `pr_app::presenters::benchmark_presentation`: display rows and texts of the Benchmark window (port of
//! `PhotoReview.App.Shared/Presenters/BenchmarkPresentation.cs`, 143 lines, baseline `d92d6aae`). Pure: no window,
//! no `windows` crate, no `unsafe`. Every text comes from the [`Localizer`] passed in (D-W8-4), never from a literal.
//!
//! The C# `INotifyPropertyChanged` part is not ported: texts are recomputed on each call (decision D-10h-2).

use pr_bench::engine::{PROGRESS_CORRECTNESS_FAILED, PROGRESS_OK, RESULT_CORRECTNESS_FAILED};
use pr_bench::models::{
    BenchmarkPhaseResult, BenchmarkProfile, BenchmarkProfileError, BenchmarkProfileProblem, BenchmarkResultStatus,
    BenchmarkWorkload,
};
use pr_core::model::enums::LoadingMode;
use pr_l10n::localizer::Localizer;
use pr_l10n::template::LocArg;

/// C# `BenchmarkResultRow.BestSymbol` (trophy marker; also the language-neutral column header).
pub const BEST_SYMBOL: &str = "🏆";

/// Prefix of the catalog keys of profile texts: `benchmark.profile.<camelCaseId>.name|description`.
pub const PROFILE_KEY_PREFIX: &str = "benchmark.profile.";

// Catalog keys (English catalog `assets/languages/en.json`). Constants so a typo is one edit, not many.
pub const KEY_NO_VALUE: &str = "bench.noValue";
pub const KEY_UNIT_MILLISECONDS: &str = "unit.milliseconds";
pub const KEY_STATUS_PASS: &str = "enum.benchmarkResultStatus.pass";
pub const KEY_STATUS_WARN: &str = "enum.benchmarkResultStatus.warn";
pub const KEY_STATUS_FAIL: &str = "enum.benchmarkResultStatus.fail";
pub const KEY_STATUS_INSUFFICIENT_DATA: &str = "enum.benchmarkResultStatus.insufficientData";
pub const KEY_RESULT_CORRECTNESS_FAILED: &str = "benchmark.result.correctnessFailed";
pub const KEY_RESULT_FAILED_DETAIL: &str = "bench.result.failedDetail";
pub const KEY_ERROR_NOT_IMPLEMENTED: &str = "bench.error.notImplemented";
pub const KEY_ERROR_PROFILE_INCOMPLETE: &str = "bench.error.profileIncomplete";
pub const KEY_ERROR_PROFILE_SETTINGS: &str = "bench.error.profileSettings";
pub const KEY_ERROR_UNEXPECTED: &str = "bench.error.unexpected";
pub const KEY_PROGRESS_OK: &str = "benchmark.progress.ok";
pub const KEY_PROGRESS_CORRECTNESS_FAILED: &str = "benchmark.progress.correctnessFailed";

/// The failure behind a row that shows a failed profile. C# `Exception? Error` of `BenchmarkResultRow`, reduced to the
/// two shapes `BenchmarkText.Describe` tells apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowError {
    /// C# `BenchmarkProfileException` (problem code and profile id drive the translated text).
    Profile(BenchmarkProfileError),
    /// Any other exception: its technical message (C# `ex.Message`), shown inside a translated wrapper.
    Other(String),
}

/// C# `BenchmarkResultRow`: one profile's result in the comparison grid. Plain data; `is_best` is set by the presenter
/// after ranking (C# `IsBest { get; set; }`).
#[derive(Debug, Clone)]
pub struct BenchmarkResultRow {
    pub profile: BenchmarkProfile,
    pub phase: BenchmarkPhaseResult,
    pub is_best: bool,
    pub error: Option<RowError>,
}

impl BenchmarkResultRow {
    /// C# `new BenchmarkResultRow(profile, phase, error)`; `is_best` starts `false`.
    pub fn new(profile: BenchmarkProfile, phase: BenchmarkPhaseResult, error: Option<RowError>) -> Self {
        Self {
            profile,
            phase,
            is_best: false,
            error,
        }
    }

    /// C# `ProfileName` = [`profile_name`] of `Profile`.
    pub fn profile_name(&self, localizer: &Localizer) -> String {
        profile_name(&self.profile, localizer)
    }

    /// C# `LoadingMode` = `Profile.LoadingMode`.
    pub fn loading_mode(&self) -> LoadingMode {
        self.profile.loading_mode
    }

    /// C# `LoadingModeText`.
    pub fn loading_mode_text(&self, localizer: &Localizer) -> String {
        loading_mode_text(self.loading_mode(), localizer)
    }

    /// C# `Workload` = `Phase.Workload`.
    pub fn workload(&self) -> BenchmarkWorkload {
        self.phase.workload
    }

    /// C# `WorkloadText`.
    pub fn workload_text(&self, localizer: &Localizer) -> String {
        workload_text(self.workload(), localizer)
    }

    /// C# `Count` = `Phase.Count` (number of samples).
    pub fn count(&self) -> i32 {
        self.phase.count()
    }

    /// C# `Status` = `Phase.Status`.
    pub fn status(&self) -> BenchmarkResultStatus {
        self.phase.status
    }

    /// C# `BestMarker`: [`BEST_SYMBOL`] when `is_best`, else the empty string.
    pub fn best_marker(&self) -> &'static str {
        if self.is_best { BEST_SYMBOL } else { "" }
    }

    /// C# `P50Text` = [`milliseconds_text`] of `Phase.P50`.
    pub fn p50_text(&self, localizer: &Localizer) -> String {
        milliseconds_text(self.phase.p50(), self.count(), localizer)
    }

    /// C# `P95Text`.
    pub fn p95_text(&self, localizer: &Localizer) -> String {
        milliseconds_text(self.phase.p95(), self.count(), localizer)
    }

    /// C# `P99Text`.
    pub fn p99_text(&self, localizer: &Localizer) -> String {
        milliseconds_text(self.phase.p99(), self.count(), localizer)
    }

    /// C# `MaxText` = [`milliseconds_text`] of `Phase.Max`.
    pub fn max_text(&self, localizer: &Localizer) -> String {
        milliseconds_text(self.phase.max(), self.count(), localizer)
    }

    /// C# `StatusText` (switch order matters: see contract, trap T6).
    pub fn status_text(&self, localizer: &Localizer) -> String {
        match self.status() {
            BenchmarkResultStatus::Pass => localizer.get(KEY_STATUS_PASS),
            BenchmarkResultStatus::Warn => localizer.get(KEY_STATUS_WARN),
            BenchmarkResultStatus::Fail => match self.phase.message.as_deref() {
                None => localizer.get(KEY_STATUS_FAIL),
                Some(RESULT_CORRECTNESS_FAILED) => localizer.get(KEY_RESULT_CORRECTNESS_FAILED),
                Some(message) => {
                    let detail = match &self.error {
                        Some(error) => describe_error(error, localizer),
                        None => message.to_owned(),
                    };
                    localizer.format(KEY_RESULT_FAILED_DETAIL, &[LocArg::new("message", detail)])
                }
            },
            BenchmarkResultStatus::InsufficientData => localizer.get(KEY_STATUS_INSUFFICIENT_DATA),
        }
    }

    /// C# `CellTexts`: the ten grid cells in column order: best marker, profile name, loading mode, workload, sample
    /// count, P50, P95, P99, Max, status.
    pub fn cell_texts(&self, localizer: &Localizer) -> Vec<String> {
        vec![
            self.best_marker().to_owned(),
            self.profile_name(localizer),
            self.loading_mode_text(localizer),
            self.workload_text(localizer),
            self.count().to_string(),
            self.p50_text(localizer),
            self.p95_text(localizer),
            self.p99_text(localizer),
            self.max_text(localizer),
            self.status_text(localizer),
        ]
    }
}

/// C# `BenchmarkProfileItem`: list item for one profile. Name and description come from the catalog, else from the
/// library's English text. The C# `INotifyPropertyChanged` is dropped (decision D-10h-2).
#[derive(Debug, Clone)]
pub struct BenchmarkProfileItem {
    pub profile: BenchmarkProfile,
}

impl BenchmarkProfileItem {
    pub fn new(profile: BenchmarkProfile) -> Self {
        Self { profile }
    }

    /// C# `Name` = [`profile_name`].
    pub fn name(&self, localizer: &Localizer) -> String {
        profile_name(&self.profile, localizer)
    }

    /// C# `Description` = [`profile_description`].
    pub fn description(&self, localizer: &Localizer) -> String {
        profile_description(&self.profile, localizer)
    }
}

/// C# `BenchmarkText.ProfileKey(profileId, field)`: `"fast-sequential"` + `"name"` gives
/// `"benchmark.profile.fastSequential.name"`. Each `-` uppercases the next character; the prefix and the
/// `.field` suffix are always appended. Case mapping is per char, invariant, BMP only (trap T1).
pub fn profile_key(profile_id: &str, field: &str) -> String {
    let mut key = String::with_capacity(PROFILE_KEY_PREFIX.len() + profile_id.len() + field.len() + 1);
    key.push_str(PROFILE_KEY_PREFIX);
    let mut upper = false;
    for c in profile_id.chars() {
        if c == '-' {
            upper = true;
            continue;
        }
        if upper {
            key.push(invariant_upper(c));
        } else {
            key.push(c);
        }
        upper = false;
    }
    key.push('.');
    key.push_str(field);
    key
}

/// The `.NET char.ToUpperInvariant` mapping for one char (trap T2): the one-char `to_uppercase` result, else the char
/// itself (`ß` stays `ß`). `ı` (U+0131) stays `ı` as the invariant culture does (golden `key-dotless-i`).
fn invariant_upper(c: char) -> char {
    if c == '\u{0131}' {
        return c;
    }
    let mut mapped = c.to_uppercase();
    match (mapped.next(), mapped.next()) {
        (Some(upper), None) => upper,
        _ => c,
    }
}

/// Catalog text of `key` when the localizer has it, else `fallback` (C# `Lookup`).
fn lookup(key: &str, fallback: &str, localizer: &Localizer) -> String {
    if localizer.contains(key) {
        localizer.get(key)
    } else {
        fallback.to_owned()
    }
}

/// C# `BenchmarkText.ProfileName(profile)`: catalog text of `profile_key(id, "name")` when the localizer has the key,
/// else `profile.name`.
pub fn profile_name(profile: &BenchmarkProfile, localizer: &Localizer) -> String {
    lookup(&profile_key(&profile.id, "name"), &profile.name, localizer)
}

/// C# `BenchmarkText.ProfileDescription(profile)`: same rule with `"description"` and `profile.description`.
pub fn profile_description(profile: &BenchmarkProfile, localizer: &Localizer) -> String {
    lookup(
        &profile_key(&profile.id, "description"),
        &profile.description,
        localizer,
    )
}

/// C# `BenchmarkText.ProgressMessage(message)`: maps the two fixed engine messages (`PROGRESS_OK`,
/// `PROGRESS_CORRECTNESS_FAILED`) to catalog text; any other message passes through unchanged.
pub fn progress_message(message: &str, localizer: &Localizer) -> String {
    match message {
        PROGRESS_OK => localizer.get(KEY_PROGRESS_OK),
        PROGRESS_CORRECTNESS_FAILED => localizer.get(KEY_PROGRESS_CORRECTNESS_FAILED),
        other => other.to_owned(),
    }
}

/// C# `BenchmarkText.LoadingModeText`: `Fast`, `Preview`, `Original` from the catalog.
pub fn loading_mode_text(mode: LoadingMode, localizer: &Localizer) -> String {
    let key = match mode {
        LoadingMode::Fast => "enum.loadingMode.fast",
        LoadingMode::Preview => "enum.loadingMode.preview",
        LoadingMode::Original => "enum.loadingMode.original",
    };
    localizer.get(key)
}

/// C# `BenchmarkText.WorkloadText`: the seven workloads from the catalog (`enum.benchmarkWorkload.*`).
pub fn workload_text(workload: BenchmarkWorkload, localizer: &Localizer) -> String {
    let key = match workload {
        BenchmarkWorkload::FirstFrame => "enum.benchmarkWorkload.firstFrame",
        BenchmarkWorkload::Sequential => "enum.benchmarkWorkload.sequential",
        BenchmarkWorkload::Random => "enum.benchmarkWorkload.random",
        BenchmarkWorkload::WarmNext => "enum.benchmarkWorkload.warmNext",
        BenchmarkWorkload::Preload => "enum.benchmarkWorkload.preload",
        BenchmarkWorkload::FileAction => "enum.benchmarkWorkload.fileAction",
        BenchmarkWorkload::Correctness => "enum.benchmarkWorkload.correctness",
    };
    localizer.get(key)
}

/// C# `BenchmarkText.Describe(Exception)`:
/// - `Profile` with problem `NotImplemented`: `bench.error.notImplemented` with `profileName` = the catalog name of
///   `pr_bench::profiles::find(profile_id)` when found, else the raw id;
/// - `Profile` with problem `MissingIdOrMode`: `bench.error.profileIncomplete` (no arguments);
/// - any other `Profile` (`InvalidSettings`): `bench.error.profileSettings` with `profileId`;
/// - `Other(message)`: `bench.error.unexpected` with `message`.
pub fn describe_error(error: &RowError, localizer: &Localizer) -> String {
    match error {
        RowError::Profile(e) => match e.problem {
            BenchmarkProfileProblem::NotImplemented => {
                let name = match pr_bench::profiles::find(&e.profile_id) {
                    Some(profile) => profile_name(profile, localizer),
                    None => e.profile_id.clone(),
                };
                localizer.format(KEY_ERROR_NOT_IMPLEMENTED, &[LocArg::new("profileName", name)])
            }
            BenchmarkProfileProblem::MissingIdOrMode => localizer.get(KEY_ERROR_PROFILE_INCOMPLETE),
            BenchmarkProfileProblem::InvalidSettings => localizer.format(
                KEY_ERROR_PROFILE_SETTINGS,
                &[LocArg::new("profileId", e.profile_id.clone())],
            ),
        },
        RowError::Other(message) => localizer.format(KEY_ERROR_UNEXPECTED, &[LocArg::new("message", message.clone())]),
    }
}

/// C# `BenchmarkResultRow.MillisecondsText(value)` (private there): `bench.noValue` when `count == 0` (the value is
/// ignored), else `unit.milliseconds` with `{value}` = [`format_milliseconds_f0`].
pub fn milliseconds_text(value: f64, count: i32, localizer: &Localizer) -> String {
    if count == 0 {
        return localizer.get(KEY_NO_VALUE);
    }
    localizer.format(
        KEY_UNIT_MILLISECONDS,
        &[LocArg::new("value", format_milliseconds_f0(value))],
    )
}

/// C# `value.ToString("F0", CurrentCulture)` as the invariant culture prints it: `NaN`, `Infinity`, `-Infinity`
/// for the non-finite values; otherwise `{:.0}` (ties to even on the exact binary value; `-0.4` gives `-0`), trap T4.
pub fn format_milliseconds_f0(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f64::INFINITY {
        "Infinity".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        format!("{value:.0}")
    }
}
