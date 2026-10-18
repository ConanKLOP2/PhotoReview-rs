//! The `--benchmark`, `--benchmark-all` and `--benchmark-actions` modes: `RunCliBenchmarksAsync` and the dispatch blocks around it
//! in `Program.cs` (C# baseline d92d6aae).

use super::backend::{PhaseSummary, ProfileInfo, ToolError};
use super::{CliContext, CliMode};

/// `.Take(64)`: at most this many supported files are measured.
pub const FILE_LIMIT: usize = 64;

/// The files of a folder split into the measured set and the unsupported-extension counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetPlan {
    /// First [`FILE_LIMIT`] supported files, `Path.GetFullPath` applied, in listing order.
    pub files: Vec<String>,
    /// `(key, count)` ordered by key, ordinal. Key = `"<none>"` when the extension is blank, else the extension lower-cased
    /// (`ToLowerInvariant`).
    pub unsupported_extensions: Vec<(String, usize)>,
}

/// The pure part of `RunCliBenchmarksAsync` after `Directory.EnumerateFiles`: sort `all_files` by `get_file_name`
/// (OrdinalIgnoreCase) then by whole path (Ordinal), both stable; split by `catalog::types::extension_in(SUPPORTED_EXTENSIONS, ..)` on the extension
/// (`get_extension`); count the unsupported by key; keep the first 64 supported. No supported file -> `InvalidOperationException
/// ("Benchmark folder contains no supported images")`.
pub fn plan_dataset(all_files: &[String]) -> Result<DatasetPlan, ToolError> {
    todo!("W7-03d")
}

/// `$"DONE profile={id} status={status} p50={p50:F1}ms p95={p95:F1}ms max={max:F1}ms report={report_path}"`. `F1` uses
/// `pr_perf::rules::fmt_fixed(v, 1)`-equivalent invariant text (see the contract: tie rounding and the Windows-locale question).
pub fn format_done_line(profile_id: &str, phase: &PhaseSummary, report_path: &str) -> String {
    todo!("W7-03d")
}

/// `$"START profile={id} workload={workload} mode={loading_mode} workers={workers} window={next}/{previous}"`.
pub fn format_start_line(profile: &ProfileInfo) -> String {
    todo!("W7-03d")
}

/// `RunCliBenchmarksAsync(folder, profiles, outputOverride)`. Order of effects (pinned by the process golden):
/// 1. `fs.directory_exists(folder)` else `DirectoryNotFoundException(folder)`;
/// 2. `resolve_report_directory` (guard) -> `create_directory` of it (a failure later still leaves it created);
/// 3. list the top-level files (`fs.enumerate_files(folder, "*")`, hidden/system included), [`plan_dataset`];
/// 4. manifest: `DatasetManifest { folder: get_full_path(folder), files, unsupported_extensions, file_limit: 64 }`;
/// 5. per profile: START line; `backend.run_profile` with a progress callback printing `  {profile_id}: {completed}/{total} {message}`;
///    on success write `{report_dir}\{profile.id}-{run_id}.json` (`backend.report_json` + `write_report_file`) and print the DONE line,
///    `any_failed |= status == Fail`; on ANY `Err` from that block (including a failed report write or report_json): `any_failed = true`,
///    message `Profile failed before producing samples: {type_name}: {message}`, `backend.record_failed_profile`, write its report
///    file the same way (an error there propagates), print `FAIL profile={id} error={message}` to stderr;
/// 6. `{report_dir}\summary.json` from `backend.batch_summary_json`, print `REPORT: {summary path}`;
/// 7. returns `Ok(exit_code)` = 1 when `any_failed` else 0.
///
/// `Err` is an exception that escaped to `Program.cs`; the caller decides with `is_expected_tool_failure`.
pub fn run_cli_benchmarks(
    ctx: &mut CliContext<'_>,
    folder: &str,
    profiles: &[ProfileInfo],
    output_override: Option<&str>,
) -> Result<i32, ToolError> {
    todo!("W7-03d")
}

/// The `--benchmark*` block of `Program.cs`: `args[1]` is the folder; `reject_surplus_benchmark_arguments` then `resolve_profiles`
/// (an error prints `PhotoReview.Benchmark.Cli: {message}` to `err`, exit 2); `resolve_output`; `run_cli_benchmarks`; an `Err` that
/// `is_expected_tool_failure` prints the same line and exits 2, any other `Err` prints `Unhandled exception. {type}: {message}`
/// and exits `UNHANDLED_EXIT_CODE`.
pub fn run_benchmark_mode(ctx: &mut CliContext<'_>, mode: CliMode, args: &[String]) -> i32 {
    todo!("W7-03d")
}
