//! Command line front end of the benchmark tool.
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/Program.cs` (dispatch, usage, `--benchmark*` modes) together with
//! `BenchmarkCliArguments.cs` ([`arguments`]) and `ToolPathGuard.cs` ([`tool_path_guard`]) (C# baseline d92d6aae).
//! The profile runner itself (engine, image executor, workload runner) is reached through [`BenchmarkBackend`].

pub mod arguments;
pub mod backend;
pub mod batch;
mod bench_compare;
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
        let mut path = std::env::temp_dir().to_string_lossy().into_owned();
        if !pr_core::text::dotnet_path::ends_in_directory_separator(&path) {
            path.push('\\');
        }
        path
    }

    fn utc_stamp(&self) -> String {
        // `to_json` is `yyyy-MM-ddTHH:mm:ss[.fffffff]Z`; the first 19 characters hold every field needed.
        let json = pr_core::time::DateTimeUtc::now().to_json();
        let field = |range: std::ops::Range<usize>| json.get(range).unwrap_or("00");
        format!(
            "{}{}{}-{}{}{}",
            field(0..4),
            field(5..7),
            field(8..10),
            field(11..13),
            field(14..16),
            field(17..19)
        )
    }

    fn new_guid_n(&self) -> String {
        pr_core::io::physical::new_guid_n()
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
    let len = args.len();
    let Some(first) = args.first().map(String::as_str) else {
        return CliMode::Unknown;
    };
    let second = args.get(1).map(String::as_str);
    let third = args.get(2).map(String::as_str);
    if len == 1 && first == "--benchmark-list-profiles" {
        return CliMode::ListProfiles;
    }
    if len >= 2 {
        match first {
            "--benchmark" => return CliMode::Benchmark,
            "--benchmark-all" => return CliMode::BenchmarkAll,
            "--benchmark-actions" => return CliMode::BenchmarkActions,
            _ => {}
        }
    }
    if (len == 2 || len == 4) && first == "--ui-next-probe" && (len == 2 || third == Some("--cache-dir")) {
        return CliMode::UiNextProbe;
    }
    if first == "compare" || first == "--perf-compare" {
        return CliMode::Compare;
    }
    match first {
        "--perf-export" => return CliMode::PerfExport,
        "--startup-probe" => return CliMode::StartupProbe,
        "--perf-session" => return CliMode::PerfSession,
        _ => {}
    }
    if (len == 2 || len == 3) && first == "--preload-bench" {
        return CliMode::PreloadBench;
    }
    if len >= 2 && first == "--perf-analyze" {
        return CliMode::PerfAnalyze;
    }
    if (3..=5).contains(&len) && first == "--io-decode-split" {
        return CliMode::IoDecodeSplit;
    }
    if len >= 2 && first == "--decoder-bench" && second == Some("--raw") {
        return CliMode::DecoderBenchRaw;
    }
    if (3..=6).contains(&len) && first == "--decoder-bench" {
        return CliMode::DecoderBench;
    }
    if len == 2 && first == "--explorer-probe" {
        return CliMode::ExplorerProbe;
    }
    if len >= 2 && first == "--raw-survey" {
        return CliMode::RawSurvey;
    }
    CliMode::Unknown
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
    if args.first().is_some_and(|arg| arg == "--bench-compare") {
        return bench_compare::run(args, out, err, env);
    }
    let mode = classify(args);
    match mode {
        CliMode::ListProfiles => {
            write_profile_list(out, &*backend);
            0
        }
        CliMode::Benchmark | CliMode::BenchmarkAll | CliMode::BenchmarkActions => {
            let mut ctx = CliContext {
                backend,
                env,
                fs,
                out,
                err,
            };
            batch::run_benchmark_mode(&mut ctx, mode, args)
        }
        CliMode::Unknown => {
            write_usage(err);
            2
        }
        other => backend.run_other_mode(other, args, out, err),
    }
}

/// Header and mode lines of the usage text (`Program.cs`, the array at the end of the file).
const USAGE_LINES: [&str; 16] = [
    "--benchmark-list-profiles",
    "--benchmark <folder> <profile,...>",
    "--benchmark-all <folder> [output]",
    "--benchmark-actions <folder> [output]",
    "--ui-next-probe <folder> [--cache-dir DIR]",
    "--perf-session ...",
    "--preload-bench <folder> [n]",
    "--perf-analyze <dir> [--rules <file>]",
    "--startup-probe --shell <exe> [--wpf <exe>] --source <folder> [--count n --rounds n --warmup n --seed n --keys n --out dir --perf-json file]",
    "compare <baseline.json> <current.json> [--threshold-percent 5 --min-abs-ms 1 --min-abs-mb 16 --busy-cpu 60 --max-cv 25 --min-samples 3 --json f --markdown f]",
    "--perf-export <perf-session dir> [--out file.json]",
    "--io-decode-split ...",
    "--decoder-bench ...",
    "--decoder-bench --raw <folder> <outDir> [iterations=3]",
    "--explorer-probe <folder>",
    "--raw-survey <dir> [--markdown <out.md>]",
];

/// The text printed for an unknown or missing mode (to `err`): the header line, then the mode lines of `Program.cs`.
pub fn write_usage(err: &mut dyn Write) {
    write_line(err, &format!("{TOOL_NAME}: unknown or missing mode. Supported modes:"));
    for line in USAGE_LINES {
        write_line(err, &format!("  {line}"));
    }
}

/// `--benchmark-list-profiles` output: one tab separated line per profile of `ProfileCatalog::all`.
pub fn write_profile_list(out: &mut dyn Write, catalog: &dyn ProfileCatalog) {
    for profile in catalog.all() {
        let correctness_only = if profile.correctness_only { "True" } else { "False" };
        write_line(
            out,
            &format!(
                "{}\t{}\t{}\tmode={}\tworkers={}\titerations={}\tcorrectnessOnly={}\t{}",
                profile.id,
                profile.name,
                profile.workload,
                profile.loading_mode,
                profile.workers,
                profile.iterations,
                correctness_only,
                profile.description
            ),
        );
    }
}

/// `Console.WriteLine`: `text` followed by `\r\n`; I/O errors are ignored.
pub(crate) fn write_line(w: &mut dyn Write, text: &str) {
    let _ = w.write_all(text.as_bytes());
    let _ = w.write_all(b"\r\n");
}
