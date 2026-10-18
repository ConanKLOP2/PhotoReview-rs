//! Mode dispatch of `tools/PhotoReview.Benchmark.Cli/Program.cs`: `classify` (the `if` chain, first match wins, arity matters,
//! modes are case sensitive; contract note 1), the usage text, `--benchmark-list-profiles` and the unknown-mode exit code.
//!
//! No C# unit tests exist for Program.cs (contract, "Program.cs has no C# unit tests"): every test here is written from the
//! C# source. Red stage: `classify`, `run`, `write_usage` and `write_profile_list` are `todo!()` in the stub.

use pr_bench::cli::backend::{
    BenchmarkBackend, DatasetManifest, PhaseSummary, ProfileCatalog, ProfileInfo, ProfileRun, ProgressLine, ToolError,
};
use pr_bench::cli::{CliMode, SystemToolEnvironment, classify, run, write_usage};
use pr_core::io::physical::PhysicalFileSystem;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A catalog with one profile and an engine that must not be reached by the modes these tests run.
struct DispatchBackend {
    profiles: Vec<ProfileInfo>,
}

impl ProfileCatalog for DispatchBackend {
    fn all(&self) -> Vec<ProfileInfo> {
        self.profiles.clone()
    }
    fn runnable(&self) -> Vec<ProfileInfo> {
        self.profiles.clone()
    }
    fn find(&self, id: &str) -> Option<ProfileInfo> {
        self.profiles.iter().find(|p| p.id == id).cloned()
    }
}

impl BenchmarkBackend for DispatchBackend {
    fn run_profile(
        &mut self,
        _folder: &str,
        _profile: &ProfileInfo,
        _files: &[String],
        _progress: &mut dyn FnMut(&ProgressLine),
    ) -> Result<ProfileRun, ToolError> {
        Err(ToolError::new(
            "InvalidOperationException",
            "no engine in dispatch tests",
        ))
    }
    fn record_failed_profile(&mut self, _folder_full_path: &str, _profile: &ProfileInfo, _message: &str) -> ProfileRun {
        ProfileRun {
            run_id: String::new(),
            phase: PhaseSummary {
                status: pr_bench::cli::backend::PhaseStatus::Fail,
                p50: 0.0,
                p95: 0.0,
                max: 0.0,
            },
        }
    }
    fn report_json(&self, _run_id: &str) -> Result<String, ToolError> {
        Ok(String::new())
    }
    fn batch_summary_json(&self, _dataset: &DatasetManifest) -> Result<String, ToolError> {
        Ok(String::new())
    }
}

fn profile(id: &str) -> ProfileInfo {
    ProfileInfo {
        id: id.to_owned(),
        name: "Fast balanced".to_owned(),
        description: "balanced preset".to_owned(),
        workload: "Sequential".to_owned(),
        loading_mode: "Preview".to_owned(),
        workers: 4,
        next_window: 2,
        previous_window: 1,
        iterations: 3,
        correctness_only: true,
        is_file_action: false,
    }
}

/// Program.cs usage: header, then the 16 mode lines, each prefixed by two spaces (`Console.Error.WriteLine`, CRLF).
const USAGE: &str = concat!(
    "PhotoReview.Benchmark.Cli: unknown or missing mode. Supported modes:\r\n",
    "  --benchmark-list-profiles\r\n",
    "  --benchmark <folder> <profile,...>\r\n",
    "  --benchmark-all <folder> [output]\r\n",
    "  --benchmark-actions <folder> [output]\r\n",
    "  --ui-next-probe <folder> [--cache-dir DIR]\r\n",
    "  --perf-session ...\r\n",
    "  --preload-bench <folder> [n]\r\n",
    "  --perf-analyze <dir> [--rules <file>]\r\n",
    "  --startup-probe --shell <exe> [--wpf <exe>] --source <folder> [--count n --rounds n --warmup n --seed n --keys n --out dir --perf-json file]\r\n",
    "  compare <baseline.json> <current.json> [--threshold-percent 5 --min-abs-ms 1 --min-abs-mb 16 --busy-cpu 60 --max-cv 25 --min-samples 3 --json f --markdown f]\r\n",
    "  --perf-export <perf-session dir> [--out file.json]\r\n",
    "  --io-decode-split ...\r\n",
    "  --decoder-bench ...\r\n",
    "  --decoder-bench --raw <folder> <outDir> [iterations=3]\r\n",
    "  --explorer-probe <folder>\r\n",
    "  --raw-survey <dir> [--markdown <out.md>]\r\n",
);

// ---- classify: one test per row of the contract's dispatch table ----------------------------------------------------------

// Row 1: `len == 1 && a0 == "--benchmark-list-profiles"`
#[test]
fn classify_list_profiles() {
    assert_eq!(classify(&args(&["--benchmark-list-profiles"])), CliMode::ListProfiles);
}

// Row 2: `len >= 2 && a0 in {"--benchmark", "--benchmark-all", "--benchmark-actions"}`
#[test]
fn classify_benchmark_modes() {
    assert_eq!(classify(&args(&["--benchmark", "f"])), CliMode::Benchmark);
    assert_eq!(
        classify(&args(&["--benchmark", "f", "fast-balanced"])),
        CliMode::Benchmark
    );
    assert_eq!(classify(&args(&["--benchmark-all", "f"])), CliMode::BenchmarkAll);
    assert_eq!(
        classify(&args(&["--benchmark-actions", "f", "out"])),
        CliMode::BenchmarkActions
    );
}

// Row 3: `len in {2, 4} && a0 == "--ui-next-probe" && (len == 2 || a2 == "--cache-dir")`
#[test]
fn classify_ui_next_probe() {
    assert_eq!(classify(&args(&["--ui-next-probe", "f"])), CliMode::UiNextProbe);
    assert_eq!(
        classify(&args(&["--ui-next-probe", "f", "--cache-dir", "d"])),
        CliMode::UiNextProbe
    );
}

// Row 4: `len >= 1 && a0 in {"compare", "--perf-compare"}`
#[test]
fn classify_compare() {
    assert_eq!(classify(&args(&["compare", "a.json", "b.json"])), CliMode::Compare);
    assert_eq!(classify(&args(&["--perf-compare"])), CliMode::Compare);
}

// Row 5: `len >= 1 && a0 == "--perf-export"`
#[test]
fn classify_perf_export() {
    assert_eq!(classify(&args(&["--perf-export", "dir"])), CliMode::PerfExport);
}

// Row 6: `len >= 1 && a0 == "--startup-probe"`
#[test]
fn classify_startup_probe() {
    assert_eq!(classify(&args(&["--startup-probe"])), CliMode::StartupProbe);
}

// Row 7: `len >= 1 && a0 == "--perf-session"`
#[test]
fn classify_perf_session() {
    assert_eq!(
        classify(&args(&["--perf-session", "scenario.json"])),
        CliMode::PerfSession
    );
}

// Row 8: `len in {2, 3} && a0 == "--preload-bench"`
#[test]
fn classify_preload_bench() {
    assert_eq!(classify(&args(&["--preload-bench", "f"])), CliMode::PreloadBench);
    assert_eq!(classify(&args(&["--preload-bench", "f", "3"])), CliMode::PreloadBench);
    assert_eq!(classify(&args(&["--preload-bench", "f", "3", "x"])), CliMode::Unknown);
}

// Row 9: `len >= 2 && a0 == "--perf-analyze"`
#[test]
fn classify_perf_analyze() {
    assert_eq!(classify(&args(&["--perf-analyze", "runs"])), CliMode::PerfAnalyze);
    assert_eq!(
        classify(&args(&["--perf-analyze", "runs", "--rules", "r.json"])),
        CliMode::PerfAnalyze
    );
}

// Row 10: `len in 3..=5 && a0 == "--io-decode-split"`
#[test]
fn classify_io_decode_split() {
    assert_eq!(
        classify(&args(&["--io-decode-split", "a", "b"])),
        CliMode::IoDecodeSplit
    );
    assert_eq!(
        classify(&args(&["--io-decode-split", "a", "b", "c", "d"])),
        CliMode::IoDecodeSplit
    );
    assert_eq!(
        classify(&args(&["--io-decode-split", "a", "b", "c", "d", "e"])),
        CliMode::Unknown
    );
}

// Row 11: `len >= 2 && a0 == "--decoder-bench" && a1 == "--raw"`, checked before row 12
#[test]
fn classify_decoder_bench_raw_before_decoder_bench() {
    assert_eq!(classify(&args(&["--decoder-bench", "--raw"])), CliMode::DecoderBenchRaw);
    assert_eq!(
        classify(&args(&["--decoder-bench", "--raw", "raw", "out", "5"])),
        CliMode::DecoderBenchRaw
    );
}

// Row 12: `len in 3..=6 && a0 == "--decoder-bench"`
#[test]
fn classify_decoder_bench() {
    assert_eq!(
        classify(&args(&["--decoder-bench", "photos", "out", "Wpf"])),
        CliMode::DecoderBench
    );
    assert_eq!(
        classify(&args(&["--decoder-bench", "a", "b", "c", "d", "e"])),
        CliMode::DecoderBench
    );
}

// Row 13: `len == 2 && a0 == "--explorer-probe"`
#[test]
fn classify_explorer_probe() {
    assert_eq!(classify(&args(&["--explorer-probe", "f"])), CliMode::ExplorerProbe);
}

// Row 14: `len >= 2 && a0 == "--raw-survey"`
#[test]
fn classify_raw_survey() {
    assert_eq!(classify(&args(&["--raw-survey", "dir"])), CliMode::RawSurvey);
}

// Row 15 (else): the Unknown rows of contract note 1 and the empty command line.
#[test]
fn classify_unknown_rows() {
    assert_eq!(classify(&args(&[])), CliMode::Unknown);
    assert_eq!(classify(&args(&["--benchmark"])), CliMode::Unknown);
    assert_eq!(classify(&args(&["--benchmark-list-profiles", "x"])), CliMode::Unknown);
    assert_eq!(classify(&args(&["--decoder-bench", "x"])), CliMode::Unknown);
    assert_eq!(classify(&args(&["--ui-next-probe", "x", "y"])), CliMode::Unknown);
    assert_eq!(classify(&args(&["--explorer-probe"])), CliMode::Unknown);
}

#[test]
fn classify_is_case_sensitive() {
    assert_eq!(classify(&args(&["--BENCHMARK-ALL", "f"])), CliMode::Unknown);
    assert_eq!(classify(&args(&["COMPARE", "a", "b"])), CliMode::Unknown);
}

// ---- usage and list-profiles ----------------------------------------------------------------------------------------------

// Program.cs: the usage block (`Console.Error.WriteLine` for the header and every mode line, exit code 2).
#[test]
fn write_usage_prints_the_program_cs_text() {
    let mut err: Vec<u8> = Vec::new();
    write_usage(&mut err);
    assert_eq!(String::from_utf8(err).expect("UTF-8"), USAGE);
}

// No arguments: usage on stderr, nothing on stdout, exit code 2.
#[test]
fn run_without_arguments_prints_usage_and_exits_2() {
    let mut backend = DispatchBackend {
        profiles: vec![profile("fast-balanced")],
    };
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = run(
        &[],
        &mut out,
        &mut err,
        &mut backend,
        &SystemToolEnvironment,
        &PhysicalFileSystem,
    );
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert_eq!(String::from_utf8(err).expect("UTF-8"), USAGE);
}

// `--benchmark-list-profiles`: one tab separated line per profile; `correctnessOnly` prints `True`/`False` as .NET does.
#[test]
fn run_list_profiles_prints_one_tab_separated_line_per_profile() {
    let mut backend = DispatchBackend {
        profiles: vec![profile("fast-balanced")],
    };
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = run(
        &args(&["--benchmark-list-profiles"]),
        &mut out,
        &mut err,
        &mut backend,
        &SystemToolEnvironment,
        &PhysicalFileSystem,
    );
    assert_eq!(code, 0);
    assert!(err.is_empty());
    assert_eq!(
        String::from_utf8(out).expect("UTF-8"),
        "fast-balanced\tFast balanced\tSequential\tmode=Preview\tworkers=4\titerations=3\tcorrectnessOnly=True\tbalanced preset\r\n"
    );
}

// A mode outside this unit goes to `run_other_mode`; the default says the mode is not part of this build (exit 2).
#[test]
fn run_other_mode_default_is_not_available_in_this_build() {
    let mut backend = DispatchBackend { profiles: Vec::new() };
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = run(
        &args(&["--explorer-probe", "photos"]),
        &mut out,
        &mut err,
        &mut backend,
        &SystemToolEnvironment,
        &PhysicalFileSystem,
    );
    assert_eq!(code, 2);
    assert_eq!(
        String::from_utf8(err).expect("UTF-8"),
        "PhotoReview.Benchmark.Cli: mode not available in this build\r\n"
    );
}

// ---- SystemToolEnvironment ---------------------------------------------------------------------------------------------

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[test]
fn system_environment_utc_stamp_is_yyyymmdd_hhmmss_of_now() {
    use pr_bench::cli::ToolEnvironment;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs() as i64;
    let stamp = SystemToolEnvironment.utc_stamp();
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    let expected_prefix = format!("{y:04}{mo:02}{d:02}-");
    assert_eq!(stamp.len(), 15, "yyyyMMdd-HHmmss, got {stamp:?}");
    let t = &stamp[9..];
    assert!(t.bytes().all(|b| b.is_ascii_digit()), "{stamp:?}");
    let got = i64::from(t[0..2].parse::<u32>().expect("hh")) * 3600
        + i64::from(t[2..4].parse::<u32>().expect("mm")) * 60
        + i64::from(t[4..6].parse::<u32>().expect("ss"));
    assert!(
        (got - rem).abs() <= 5 || (got - rem).abs() >= 86_395,
        "{stamp:?} not within 5 s of now"
    );
    // The clock may tick over midnight between the two reads: only compare the date away from it.
    if (5..86_395).contains(&rem) {
        assert!(stamp.starts_with(&expected_prefix), "{stamp:?} vs {expected_prefix:?}");
    }
}

#[test]
fn system_environment_temp_path_ends_with_backslash_and_guid_is_32_hex() {
    use pr_bench::cli::ToolEnvironment;
    let temp = SystemToolEnvironment.temp_path();
    assert!(temp.ends_with('\\') && temp.len() > 3, "{temp:?}");
    let guid = SystemToolEnvironment.new_guid_n();
    assert_eq!(guid.len(), 32);
    assert!(
        guid.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{guid:?}"
    );
    assert_ne!(guid, SystemToolEnvironment.new_guid_n());
}
