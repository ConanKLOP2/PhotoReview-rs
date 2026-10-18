//! Command line front end of the benchmark tool.
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/Program.cs` (dispatch, usage, `--benchmark*` modes) together with
//! `BenchmarkCliArguments.cs` ([`arguments`]) and `ToolPathGuard.cs` ([`tool_path_guard`]) (C# baseline d92d6aae).
//! The profile runner itself (engine, image executor, workload runner) is reached through [`BenchmarkBackend`].

// W7-03d stub: remove this line in the commit that removes the last `todo!()`.
#![allow(unused_variables, dead_code)]

pub mod arguments;
pub mod backend;
pub mod batch;
pub mod tool_path_guard;

use std::io::Write;

use pr_core::abstractions::FileSystem;

pub use backend::{
    BenchmarkBackend, DatasetManifest, PhaseStatus, PhaseSummary, ProfileCatalog, ProfileInfo, ProfileRun,
    ProgressLine, ToolError, UnwiredBackend,
};

/// Prefix of every error line: `PhotoReview.Benchmark.Cli: <message>`.
pub const TOOL_NAME: &str = "PhotoReview.Benchmark.Cli";

/// Exit code of a .NET process that dies of an unhandled exception (`0xE0434352`), used for a failure that
/// `arguments::is_expected_tool_failure` rejects.
pub const UNHANDLED_EXIT_CODE: i32 = 0xE043_4352_u32 as i32;

/// Process state the C# code reads from `Path.GetTempPath()`, `DateTime.UtcNow` and `Guid.NewGuid()`; injected so
/// the report-directory name is testable and deterministic.
pub trait ToolEnvironment {
    /// `Path.GetTempPath()`: absolute, ends with a backslash.
    fn temp_path(&self) -> String;
    /// `DateTime.UtcNow.ToString("yyyyMMdd-HHmmss", Invariant)`.
    fn utc_stamp(&self) -> String;
    /// `Guid.NewGuid().ToString("N")`: 32 lowercase hex digits.
    fn new_guid_n(&self) -> String;
}

/// The real environment (OS temp folder, system clock, `pr_core::io::physical::new_guid_n`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemToolEnvironment;

impl ToolEnvironment for SystemToolEnvironment {
    fn temp_path(&self) -> String {
        todo!("W7-03d: std::env::temp_dir() as a string with a trailing backslash (Path.GetTempPath)")
    }
    fn utc_stamp(&self) -> String {
        todo!("W7-03d: pr_core::time::DateTimeUtc::now() formatted yyyyMMdd-HHmmss")
    }
    fn new_guid_n(&self) -> String {
        todo!("W7-03d: pr_core::io::physical::new_guid_n()")
    }
}

/// The mode a command line selects. The first four variants are implemented by this unit; the others are other tools
/// of the same C# project (not part of W7-03d) and are handed to [`BenchmarkBackend::run_other_mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliMode {
    ListProfiles,
    Benchmark,
    BenchmarkAll,
    BenchmarkActions,
    UiNextProbe,
    Compare,
    PerfExport,
    StartupProbe,
    PerfSession,
    PreloadBench,
    PerfAnalyze,
    IoDecodeSplit,
    DecoderBenchRaw,
    DecoderBench,
    ExplorerProbe,
    RawSurvey,
    /// No mode matched: print the usage and exit 2.
    Unknown,
}

/// Picks the mode exactly like the `if` chain of `Program.cs` (order and arity conditions matter, see the contract
/// table). Pure.
pub fn classify(args: &[String]) -> CliMode {
    todo!("W7-03d: port the if-chain of Program.cs")
}

/// Everything a mode needs besides its arguments (keeps function signatures short).
#[allow(missing_debug_implementations)] // holds `&mut dyn Write`/`dyn BenchmarkBackend`, which have no Debug
pub struct CliContext<'a> {
    pub backend: &'a mut dyn BenchmarkBackend,
    pub env: &'a dyn ToolEnvironment,
    pub fs: &'a dyn FileSystem,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

/// Runs the tool and returns the process exit code. Text goes to `out`/`err` with `\r\n` line ends
/// (`Console.WriteLine` on Windows).
pub fn run(
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
    backend: &mut dyn BenchmarkBackend,
    env: &dyn ToolEnvironment,
    fs: &dyn FileSystem,
) -> i32 {
    todo!("W7-03d: dispatch on classify(args)")
}

/// The text printed for an unknown or missing mode (to `err`): the header line, then the mode lines of `Program.cs`.
pub fn write_usage(err: &mut dyn Write) {
    todo!("W7-03d: header line + the mode lines of Program.cs, each prefixed by two spaces")
}

/// `--benchmark-list-profiles` output: one tab separated line per profile of `ProfileCatalog::all`.
pub fn write_profile_list(out: &mut dyn Write, catalog: &dyn ProfileCatalog) {
    todo!("W7-03d: id, name, workload, mode=.., workers=.., iterations=.., correctnessOnly=True|False, description")
}

/// `Console.WriteLine`: `text` followed by `\r\n`; I/O errors are ignored.
pub(crate) fn write_line(w: &mut dyn Write, text: &str) {
    todo!("W7-03d")
}
