//! `pr_app::presenters::benchmark_presentation`: display rows and texts of the Benchmark window (port of
//! `PhotoReview.App.Shared/Presenters/BenchmarkPresentation.cs`, 143 lines, baseline `d92d6aae`). Pure: no window,
//! no `windows` crate, no `unsafe`. Every text comes from the [`Localizer`] passed in (D-W8-4), never from a literal.
//!
//! Stub (W8-10h contract). Signatures are final; bodies are `todo!()`. The C# `IProgress`/`INotifyPropertyChanged`
//! parts are not ported: texts are recomputed on each call (see contract, decision D-10h-2).

// Stub only: the port removes this line (its imports are used by the bodies).
#![allow(unused_variables, dead_code, unused_imports)]

use pr_bench::engine::{PROGRESS_CORRECTNESS_FAILED, PROGRESS_OK, RESULT_CORRECTNESS_FAILED};
use pr_bench::models::{
    BenchmarkPhaseResult, BenchmarkProfile, BenchmarkProfileError, BenchmarkResultStatus, BenchmarkWorkload,
};
use pr_core::model::enums::LoadingMode;
use pr_l10n::localizer::Localizer;

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
        todo!()
    }

    /// C# `ProfileName` = [`profile_name`] of `Profile`.
    pub fn profile_name(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `LoadingMode` = `Profile.LoadingMode`.
    pub fn loading_mode(&self) -> LoadingMode {
        todo!()
    }

    /// C# `LoadingModeText`.
    pub fn loading_mode_text(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `Workload` = `Phase.Workload`.
    pub fn workload(&self) -> BenchmarkWorkload {
        todo!()
    }

    /// C# `WorkloadText`.
    pub fn workload_text(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `Count` = `Phase.Count` (number of samples).
    pub fn count(&self) -> i32 {
        todo!()
    }

    /// C# `Status` = `Phase.Status`.
    pub fn status(&self) -> BenchmarkResultStatus {
        todo!()
    }

    /// C# `BestMarker`: [`BEST_SYMBOL`] when `is_best`, else the empty string.
    pub fn best_marker(&self) -> &'static str {
        todo!()
    }

    /// C# `P50Text` = [`milliseconds_text`] of `Phase.P50`.
    pub fn p50_text(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `P95Text`.
    pub fn p95_text(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `P99Text`.
    pub fn p99_text(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `MaxText` = [`milliseconds_text`] of `Phase.Max`.
    pub fn max_text(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `StatusText` (switch order matters: see contract, trap T6).
    pub fn status_text(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `CellTexts`: the ten grid cells in column order: best marker, profile name, loading mode, workload, sample
    /// count, P50, P95, P99, Max, status.
    pub fn cell_texts(&self, localizer: &Localizer) -> Vec<String> {
        todo!()
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
        todo!()
    }

    /// C# `Name` = [`profile_name`].
    pub fn name(&self, localizer: &Localizer) -> String {
        todo!()
    }

    /// C# `Description` = [`profile_description`].
    pub fn description(&self, localizer: &Localizer) -> String {
        todo!()
    }
}

/// C# `BenchmarkText.ProfileKey(profileId, field)`: `"fast-sequential"` + `"name"` gives
/// `"benchmark.profile.fastSequential.name"`. Each `-` uppercases the next character; the prefix and the
/// `.field` suffix are always appended. Case mapping is per char, invariant, BMP only (trap T1).
pub fn profile_key(profile_id: &str, field: &str) -> String {
    todo!()
}

/// C# `BenchmarkText.ProfileName(profile)`: catalog text of `profile_key(id, "name")` when the localizer has the key,
/// else `profile.name`.
pub fn profile_name(profile: &BenchmarkProfile, localizer: &Localizer) -> String {
    todo!()
}

/// C# `BenchmarkText.ProfileDescription(profile)`: same rule with `"description"` and `profile.description`.
pub fn profile_description(profile: &BenchmarkProfile, localizer: &Localizer) -> String {
    todo!()
}

/// C# `BenchmarkText.ProgressMessage(message)`: maps the two fixed engine messages (`PROGRESS_OK`,
/// `PROGRESS_CORRECTNESS_FAILED`) to catalog text; any other message passes through unchanged.
pub fn progress_message(message: &str, localizer: &Localizer) -> String {
    todo!()
}

/// C# `BenchmarkText.LoadingModeText`: `Fast`, `Preview`, `Original` from the catalog.
pub fn loading_mode_text(mode: LoadingMode, localizer: &Localizer) -> String {
    todo!()
}

/// C# `BenchmarkText.WorkloadText`: the seven workloads from the catalog (`enum.benchmarkWorkload.*`).
pub fn workload_text(workload: BenchmarkWorkload, localizer: &Localizer) -> String {
    todo!()
}

/// C# `BenchmarkText.Describe(Exception)`:
/// - `Profile` with problem `NotImplemented`: `bench.error.notImplemented` with `profileName` = the catalog name of
///   `pr_bench::profiles::find(profile_id)` when found, else the raw id;
/// - `Profile` with problem `MissingIdOrMode`: `bench.error.profileIncomplete` (no arguments);
/// - any other `Profile` (`InvalidSettings`): `bench.error.profileSettings` with `profileId`;
/// - `Other(message)`: `bench.error.unexpected` with `message`.
pub fn describe_error(error: &RowError, localizer: &Localizer) -> String {
    todo!()
}

/// C# `BenchmarkResultRow.MillisecondsText(value)` (private there): `bench.noValue` when `count == 0` (the value is
/// ignored), else `unit.milliseconds` with `{value}` = [`format_milliseconds_f0`].
pub fn milliseconds_text(value: f64, count: i32, localizer: &Localizer) -> String {
    todo!()
}

/// C# `value.ToString("F0", CurrentCulture)` as the invariant culture prints it: `NaN`, `Infinity`, `-Infinity`
/// for the non-finite values; otherwise `{:.0}` (ties to even on the exact binary value; `-0.4` gives `-0`), trap T4.
pub fn format_milliseconds_f0(value: f64) -> String {
    todo!()
}
