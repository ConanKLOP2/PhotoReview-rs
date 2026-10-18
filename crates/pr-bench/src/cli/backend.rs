//! What the command line needs from the benchmark engine, as plain data and one trait.
//!
//! The C# `Program.cs` calls `BenchmarkProfiles`, `BenchmarkImageExecutor`, `BenchmarkProfileScope`,
//! `BenchmarkWorkloadRunner.RunProfileAsync` and the `BenchmarkReport`/`BenchmarkBatchSummary` JSON writers directly.
//! In Rust those live in W7-01, W7-02 and W7-03a..c; this seam keeps the CLI compiling and testable before them
//! (tests use a fake backend) and makes the wiring a single adapter (`EngineBackend`, written by the lead).

use std::fmt;
use std::io::Write;

use super::{CliMode, write_line};

/// A .NET exception as the CLI sees it: the type NAME (what `ex.GetType().Name` prints and what the
/// `catch ... when` filters test) and the message. Argument errors use `"ArgumentException"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    pub type_name: String,
    pub message: String,
}

impl ToolError {
    pub fn new(type_name: &str, message: impl Into<String>) -> Self {
        Self {
            type_name: type_name.to_owned(),
            message: message.into(),
        }
    }

    /// `new ArgumentException(message)`.
    pub fn argument(message: impl Into<String>) -> Self {
        Self::new("ArgumentException", message)
    }

    /// `new InvalidOperationException(message)`.
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::new("InvalidOperationException", message)
    }

    /// `new DirectoryNotFoundException(path)`: the message IS the path.
    pub fn directory_not_found(path: impl Into<String>) -> Self {
        Self::new("DirectoryNotFoundException", path)
    }

    /// An `io::Error` raised by a C# `File`/`Directory` call on `path`. `NotFound` becomes `not_found_type`
    /// (`DirectoryNotFoundException`: `Could not find a part of the path '{path}'.`; `FileNotFoundException`:
    /// `Could not find file '{path}'.`), `PermissionDenied` becomes `UnauthorizedAccessException`
    /// (`Access to the path '{path}' is denied.`), anything else `IOException` with the error's description.
    pub fn from_io(err: &std::io::Error, path: &str, not_found_type: &str) -> Self {
        todo!("W7-03d")
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

/// The profile fields the CLI prints or branches on (`BenchmarkProfile`). Enum fields are carried as the .NET enum
/// member NAME because that is what string interpolation prints (`Fast`, `Preview`, `WarmNext`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    /// `BenchmarkWorkload` member name.
    pub workload: String,
    /// `LoadingMode` member name.
    pub loading_mode: String,
    pub workers: i32,
    pub next_window: i32,
    pub previous_window: i32,
    pub iterations: i32,
    pub correctness_only: bool,
    /// `workload == FileAction` (selects the `--benchmark-actions` profiles).
    pub is_file_action: bool,
}

/// `BenchmarkProfiles` (`All`, `Runnable`, `Find`); `find` compares the id OrdinalIgnoreCase.
pub trait ProfileCatalog {
    fn all(&self) -> Vec<ProfileInfo>;
    fn runnable(&self) -> Vec<ProfileInfo>;
    fn find(&self, id: &str) -> Option<ProfileInfo>;
}

/// `BenchmarkResultStatus`; `as_str` is the .NET `ToString()` text used in the `DONE` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseStatus {
    Pass,
    Warn,
    Fail,
    InsufficientData,
}

impl PhaseStatus {
    pub fn as_str(self) -> &'static str {
        todo!("W7-03d: Pass | Warn | Fail | InsufficientData")
    }
}

/// `report.Phases[0]` as the `DONE` line reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhaseSummary {
    pub status: PhaseStatus,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
}

/// `BenchmarkProgress` (the workload is not printed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressLine {
    pub profile_id: String,
    pub completed: i32,
    pub total: i32,
    pub message: String,
}

/// Result of one profile run; the backend has already stored report and phase for the batch summary.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRun {
    /// `report.RunId` (`Guid.NewGuid().ToString("N")`), used in the report file name.
    pub run_id: String,
    pub phase: PhaseSummary,
}

/// `BenchmarkDatasetManifest`; `unsupported_extensions` keeps the ordinal key order of the C# dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetManifest {
    pub folder: String,
    pub files: Vec<String>,
    pub unsupported_extensions: Vec<(String, usize)>,
    pub file_limit: i32,
}

/// The engine side of the CLI. Stateful: it accumulates `reports` and `outcomes` (the C# lists) in call order.
pub trait BenchmarkBackend: ProfileCatalog {
    /// One loop iteration of `RunCliBenchmarksAsync` up to and including `reports.Add` + `outcomes.Add(report.Phases[0])`:
    /// build the image executor for `files`, `CreateSeededRandom(profile.id)`, `BenchmarkProfileScope.ApplyLogging`
    /// (logging flag set for the run, restored after), run `RunProfileAsync`, store the report. `progress` receives
    /// every `BenchmarkProgress` in order. Errors keep their .NET type name.
    fn run_profile(
        &mut self,
        folder: &str,
        profile: &ProfileInfo,
        files: &[String],
        progress: &mut dyn FnMut(&ProgressLine),
    ) -> Result<ProfileRun, ToolError>;

    /// The `catch` block: build the failed phase (`Status=Fail`, no samples, `message` is made by the CLI:
    /// `Profile failed before producing samples: {type}: {message}`) and a report with a fresh run id, the machine name
    /// and `folder_full_path`; store both (in the same two lists) and return them.
    fn record_failed_profile(&mut self, folder_full_path: &str, profile: &ProfileInfo, message: &str) -> ProfileRun;

    /// `report.ToJson()` of the stored report with this run id (indented, byte-identical to C#).
    fn report_json(&self, run_id: &str) -> Result<String, ToolError>;

    /// `new BenchmarkBatchSummary(manifest, reports, outcomes).ToJson()`.
    fn batch_summary_json(&self, dataset: &DatasetManifest) -> Result<String, ToolError>;

    /// The other modes of the C# project (not W7-03d). Default: they are not part of this build.
    fn run_other_mode(&mut self, mode: CliMode, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
        write_line(err, &format!("{}: mode not available in this build", super::TOOL_NAME));
        2
    }
}

/// Placeholder backend of the stub `main.rs`; the lead replaces it by the engine-backed one.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnwiredBackend;

impl ProfileCatalog for UnwiredBackend {
    fn all(&self) -> Vec<ProfileInfo> {
        todo!("W7-03d wiring: BenchmarkProfiles.All")
    }
    fn runnable(&self) -> Vec<ProfileInfo> {
        todo!("W7-03d wiring: BenchmarkProfiles.Runnable")
    }
    fn find(&self, id: &str) -> Option<ProfileInfo> {
        todo!("W7-03d wiring: BenchmarkProfiles.Find")
    }
}

impl BenchmarkBackend for UnwiredBackend {
    fn run_profile(
        &mut self,
        folder: &str,
        profile: &ProfileInfo,
        files: &[String],
        progress: &mut dyn FnMut(&ProgressLine),
    ) -> Result<ProfileRun, ToolError> {
        todo!("W7-03d wiring")
    }
    fn record_failed_profile(&mut self, folder_full_path: &str, profile: &ProfileInfo, message: &str) -> ProfileRun {
        todo!("W7-03d wiring")
    }
    fn report_json(&self, run_id: &str) -> Result<String, ToolError> {
        todo!("W7-03d wiring")
    }
    fn batch_summary_json(&self, dataset: &DatasetManifest) -> Result<String, ToolError> {
        todo!("W7-03d wiring")
    }
}
