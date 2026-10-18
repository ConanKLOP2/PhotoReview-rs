//! `--benchmark`, `--benchmark-all` and `--benchmark-actions` (`RunCliBenchmarksAsync` in `Program.cs`) against a fake backend
//! and temp folders, plus the golden checks over `tests/golden/cli-dataset-plan.json` (inputs in
//! `tests/golden-inputs/cli-dataset-plan.json`; oracle `oracle/W7-03d.cs`, the file-selection expressions of Program.cs).
//!
//! Port of `tools/PhotoReview.Benchmark.Cli/Program.cs` (C# baseline d92d6aae). Unit W7-03d, contract `contracts/W7-03d.md`.
//! Red stage: every stub body is `todo!()`, so each test here fails until the porter implements it.
//!
//! The `batch_*` tests have no C# twin (Program.cs has no unit tests); they are written from the C# source as the contract lists.
//! Every test uses a fake backend and temp folders it removes. The default report folder (`%TEMP%`) is cleaned up by the test
//! that writes there.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;

use pr_bench::cli::backend::{
    BenchmarkBackend, DatasetManifest, PhaseStatus, PhaseSummary, ProfileCatalog, ProfileInfo, ProfileRun,
    ProgressLine, ToolError,
};
use pr_bench::cli::batch::{format_done_line, format_start_line, plan_dataset, run_benchmark_mode, run_cli_benchmarks};
use pr_bench::cli::{CliContext, CliMode, ToolEnvironment, UNHANDLED_EXIT_CODE, write_profile_list};
use pr_core::io::physical::PhysicalFileSystem;
use pr_test_support::TempRoot;
use pr_test_support::golden;
use serde_json::Value;

/// Fixed process state, so the default report folder name is predictable: `{temp}PhotoReview-Benchmark-Reports\20260101-000000-abc123`.
struct FixedEnv {
    temp: String,
}

impl ToolEnvironment for FixedEnv {
    fn temp_path(&self) -> String {
        self.temp.clone()
    }
    fn utc_stamp(&self) -> String {
        "20260101-000000".to_owned()
    }
    fn new_guid_n(&self) -> String {
        "abc123000000000000000000000000ff".to_owned()
    }
}

fn info(id: &str, file_action: bool) -> ProfileInfo {
    ProfileInfo {
        id: id.to_owned(),
        name: id.to_owned(),
        description: format!("description of {id}"),
        workload: if file_action { "FileAction" } else { "Sequential" }.to_owned(),
        loading_mode: "Fast".to_owned(),
        workers: 2,
        next_window: 3,
        previous_window: 1,
        iterations: 5,
        correctness_only: false,
        is_file_action: file_action,
    }
}

/// Catalog with the ids the batch modes select: `recommended-auto` (the `--benchmark` default), two runnable profiles and
/// two file-action profiles.
fn catalog() -> Vec<ProfileInfo> {
    vec![
        info("recommended-auto", false),
        info("fast-balanced", false),
        info("logging-on", false),
        info("action-move", true),
        info("action-copy", true),
    ]
}

/// Records what the CLI asks of the engine and returns scripted results.
struct FakeBackend {
    profiles: Vec<ProfileInfo>,
    /// `run_profile` returns `BenchmarkProfileException` for these ids (a profile that fails before producing samples).
    fail_ids: Vec<String>,
    /// `run_profile` returns a `Fail` phase for these ids.
    fail_status_ids: Vec<String>,
    /// `report_json` fails for the first run of this profile id only (the report write after `run_profile` returned).
    json_fail_profile: Option<String>,
    json_failures_left: Cell<usize>,
    /// `batch_summary_json` returns this error when set.
    summary_error: Option<ToolError>,
    /// Profile ids in the order `run_profile` was called.
    ran: Vec<String>,
    /// The number of files each `run_profile` call received.
    files_seen: Vec<usize>,
    /// The file list each `run_profile` call received.
    files_lists: Vec<Vec<String>>,
    /// Every stored report: (run id, profile id, failed-before-samples).
    stored: Vec<(String, String, bool)>,
    next_run: usize,
}

impl FakeBackend {
    fn new(profiles: Vec<ProfileInfo>) -> Self {
        Self {
            profiles,
            fail_ids: Vec::new(),
            fail_status_ids: Vec::new(),
            json_fail_profile: None,
            json_failures_left: Cell::new(0),
            summary_error: None,
            ran: Vec::new(),
            files_seen: Vec::new(),
            files_lists: Vec::new(),
            stored: Vec::new(),
            next_run: 0,
        }
    }

    fn new_run_id(&mut self) -> String {
        self.next_run += 1;
        format!("r{}", self.next_run)
    }
}

impl ProfileCatalog for FakeBackend {
    fn all(&self) -> Vec<ProfileInfo> {
        self.profiles.clone()
    }
    fn runnable(&self) -> Vec<ProfileInfo> {
        self.profiles.iter().filter(|p| !p.is_file_action).cloned().collect()
    }
    fn find(&self, id: &str) -> Option<ProfileInfo> {
        self.profiles.iter().find(|p| p.id.eq_ignore_ascii_case(id)).cloned()
    }
}

impl BenchmarkBackend for FakeBackend {
    fn run_profile(
        &mut self,
        _folder: &str,
        profile: &ProfileInfo,
        files: &[String],
        progress: &mut dyn FnMut(&ProgressLine),
    ) -> Result<ProfileRun, ToolError> {
        self.ran.push(profile.id.clone());
        self.files_seen.push(files.len());
        self.files_lists.push(files.to_vec());
        if self.fail_ids.contains(&profile.id) {
            return Err(ToolError::new("BenchmarkProfileException", "boom"));
        }
        for done in 1..=2 {
            progress(&ProgressLine {
                profile_id: profile.id.clone(),
                completed: done,
                total: 2,
                message: "warm".to_owned(),
            });
        }
        let run_id = self.new_run_id();
        self.stored.push((run_id.clone(), profile.id.clone(), false));
        let status = if self.fail_status_ids.contains(&profile.id) {
            PhaseStatus::Fail
        } else {
            PhaseStatus::Pass
        };
        Ok(ProfileRun {
            run_id,
            phase: PhaseSummary {
                status,
                p50: 12.25,
                p95: 13.0,
                max: 20.0,
            },
        })
    }

    fn record_failed_profile(&mut self, _folder_full_path: &str, profile: &ProfileInfo, _message: &str) -> ProfileRun {
        let run_id = self.new_run_id();
        self.stored.push((run_id.clone(), profile.id.clone(), true));
        ProfileRun {
            run_id,
            phase: PhaseSummary {
                status: PhaseStatus::Fail,
                p50: 0.0,
                p95: 0.0,
                max: 0.0,
            },
        }
    }

    fn report_json(&self, run_id: &str) -> Result<String, ToolError> {
        let profile = self
            .stored
            .iter()
            .find(|(id, _, _)| id == run_id)
            .map(|(_, p, _)| p.clone());
        if profile.is_some() && profile == self.json_fail_profile && self.json_failures_left.get() > 0 {
            self.json_failures_left.set(self.json_failures_left.get() - 1);
            return Err(ToolError::new("IOException", "report disk full"));
        }
        Ok(format!("{{\"runId\":\"{run_id}\"}}"))
    }

    fn batch_summary_json(&self, dataset: &DatasetManifest) -> Result<String, ToolError> {
        if let Some(err) = &self.summary_error {
            return Err(err.clone());
        }
        Ok(format!("{{\"files\":{}}}", dataset.files.len()))
    }
}

/// The two captured streams of a run, as text.
struct Captured {
    out: String,
    err: String,
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("CLI output is UTF-8")
}

fn photos(root: &TempRoot, name: &str, names: &[&str]) -> String {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("photo folder");
    for file in names {
        std::fs::write(dir.join(file), [1, 2, 3]).expect("photo file");
    }
    dir.to_string_lossy().into_owned()
}

fn env_for(root: &TempRoot) -> FixedEnv {
    FixedEnv {
        temp: format!("{}\\", root.path().display()),
    }
}

/// Runs `--benchmark*` style modes against the fake backend: returns the exit code and the captured output.
fn run_mode(backend: &mut FakeBackend, root: &TempRoot, mode: CliMode, args: &[&str]) -> (i32, Captured) {
    let env = env_for(root);
    let fs = PhysicalFileSystem;
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let code = {
        let mut ctx = CliContext {
            backend,
            env: &env,
            fs: &fs,
            out: &mut out,
            err: &mut err,
        };
        run_benchmark_mode(&mut ctx, mode, &owned)
    };
    (
        code,
        Captured {
            out: text(&out),
            err: text(&err),
        },
    )
}

fn lines(s: &str) -> Vec<&str> {
    s.split("\r\n").filter(|l| !l.is_empty()).collect()
}

// ---- successful batch ---------------------------------------------------------------------------------------------------

#[test]
fn batch_success_writes_reports_and_prints_start_progress_done_report_in_order() {
    let root = TempRoot::new("batch-success");
    let photo_dir = photos(&root, "photos", &["a.jpg", "b.png"]);
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    let env = env_for(&root);
    let fs = PhysicalFileSystem;
    let profiles = vec![info("recommended-auto", false), info("fast-balanced", false)];
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let result = {
        let mut ctx = CliContext {
            backend: &mut backend,
            env: &env,
            fs: &fs,
            out: &mut stdout,
            err: &mut stderr,
        };
        run_cli_benchmarks(&mut ctx, &photo_dir, &profiles, Some(&out))
    };
    assert_eq!(result.expect("batch runs"), 0);

    let printed = text(&stdout);
    let got = lines(&printed);
    assert_eq!(
        got[0],
        "START profile=recommended-auto workload=Sequential mode=Fast workers=2 window=3/1"
    );
    assert_eq!(got[1], "  recommended-auto: 1/2 warm");
    assert_eq!(got[2], "  recommended-auto: 2/2 warm");
    assert_eq!(
        got[3],
        format!(
            "DONE profile=recommended-auto status=Pass p50=12.2ms p95=13.0ms max=20.0ms report={out}\\recommended-auto-r1.json"
        )
    );
    assert!(got[4].starts_with("START profile=fast-balanced "), "{}", got[4]);
    assert_eq!(
        got.last().copied(),
        Some(format!("REPORT: {out}\\summary.json").as_str())
    );
    assert!(text(&stderr).is_empty());

    assert!(Path::new(&out).join("recommended-auto-r1.json").is_file());
    assert!(Path::new(&out).join("fast-balanced-r2.json").is_file());
    assert_eq!(
        std::fs::read_to_string(Path::new(&out).join("summary.json")).expect("summary"),
        "{\"files\":2}"
    );
}

// C#-shaped: a `Fail` status makes the batch exit 1 but prints no FAIL line (only an exception does).
#[test]
fn batch_fail_status_exits_1_without_fail_line() {
    let root = TempRoot::new("batch-fail-status");
    let photo_dir = photos(&root, "photos", &["a.jpg"]);
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    backend.fail_status_ids.push("fast-balanced".to_owned());
    let profiles = vec![info("fast-balanced", false)];
    let env = env_for(&root);
    let fs = PhysicalFileSystem;
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let code = {
        let mut ctx = CliContext {
            backend: &mut backend,
            env: &env,
            fs: &fs,
            out: &mut stdout,
            err: &mut stderr,
        };
        run_cli_benchmarks(&mut ctx, &photo_dir, &profiles, Some(&out)).expect("batch runs")
    };
    assert_eq!(code, 1);
    assert!(text(&stdout).contains("status=Fail"));
    assert!(text(&stderr).is_empty(), "{}", text(&stderr));
}

// A profile whose run throws writes a failed report, prints FAIL to stderr and the batch keeps going (exit 1).
#[test]
fn batch_profile_error_writes_failed_report_and_continues() {
    let root = TempRoot::new("batch-profile-error");
    let photo_dir = photos(&root, "photos", &["a.jpg"]);
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    backend.fail_ids.push("fast-balanced".to_owned());
    let profiles = vec![info("fast-balanced", false), info("logging-on", false)];
    let env = env_for(&root);
    let fs = PhysicalFileSystem;
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let code = {
        let mut ctx = CliContext {
            backend: &mut backend,
            env: &env,
            fs: &fs,
            out: &mut stdout,
            err: &mut stderr,
        };
        run_cli_benchmarks(&mut ctx, &photo_dir, &profiles, Some(&out)).expect("batch runs")
    };
    assert_eq!(code, 1);
    assert_eq!(text(&stderr), "FAIL profile=fast-balanced error=boom\r\n");
    assert_eq!(backend.ran, vec!["fast-balanced", "logging-on"]);
    // run ids: the failed record of fast-balanced takes r1 (no run id before the throw), logging-on's run takes r2.
    assert!(Path::new(&out).join("fast-balanced-r1.json").is_file());
    assert!(Path::new(&out).join("logging-on-r2.json").is_file());
    let failed: Vec<_> = backend.stored.iter().filter(|(_, _, failed)| *failed).collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].1, "fast-balanced");
}

// A report write that fails after `run_profile` returned keeps that report and adds a second, failed entry (C# behaviour).
#[test]
fn batch_report_write_failure_adds_duplicate_failed_entry() {
    let root = TempRoot::new("batch-report-write");
    let photo_dir = photos(&root, "photos", &["a.jpg"]);
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    backend.json_fail_profile = Some("fast-balanced".to_owned());
    backend.json_failures_left.set(1);
    let profiles = vec![info("fast-balanced", false)];
    let env = env_for(&root);
    let fs = PhysicalFileSystem;
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let code = {
        let mut ctx = CliContext {
            backend: &mut backend,
            env: &env,
            fs: &fs,
            out: &mut stdout,
            err: &mut stderr,
        };
        run_cli_benchmarks(&mut ctx, &photo_dir, &profiles, Some(&out)).expect("batch runs")
    };
    assert_eq!(code, 1);
    let for_profile: Vec<bool> = backend
        .stored
        .iter()
        .filter(|(_, p, _)| p == "fast-balanced")
        .map(|(_, _, failed)| *failed)
        .collect();
    assert_eq!(
        for_profile,
        vec![false, true],
        "the successful run stays and a failed duplicate is added"
    );
    assert!(
        text(&stderr).contains("FAIL profile=fast-balanced error=report disk full"),
        "{}",
        text(&stderr)
    );
}

// ---- refusals (exit code 2) ---------------------------------------------------------------------------------------------

// A folder that does not exist is a DirectoryNotFoundException with the folder as message: exit 2.
#[test]
fn batch_missing_folder_is_refused() {
    let root = TempRoot::new("batch-missing");
    let missing = root.join("no-such-photos").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    let (code, captured) = run_mode(
        &mut backend,
        &root,
        CliMode::BenchmarkAll,
        &["--benchmark-all", &missing],
    );
    assert_eq!(code, 2);
    assert_eq!(captured.err, format!("PhotoReview.Benchmark.Cli: {missing}\r\n"));
    assert!(backend.ran.is_empty());
}

// The report folder may not overlap the photo folder: refused before anything is created, exit 2.
#[test]
fn batch_overlapping_output_is_refused() {
    let root = TempRoot::new("batch-overlap");
    let photo_dir = photos(&root, "photos", &["a.jpg"]);
    let mut backend = FakeBackend::new(catalog());
    let (code, captured) = run_mode(
        &mut backend,
        &root,
        CliMode::BenchmarkAll,
        &["--benchmark-all", &photo_dir, &photo_dir],
    );
    assert_eq!(code, 2);
    assert!(
        captured
            .err
            .starts_with("PhotoReview.Benchmark.Cli: output directory '"),
        "{}",
        captured.err
    );
    assert!(
        captured
            .err
            .contains("must not equal, contain or sit inside the photo folder"),
        "{}",
        captured.err
    );
    assert!(backend.ran.is_empty());
}

// No supported image: the error is printed, exit 2, and the report folder created before the check stays behind.
#[test]
fn batch_no_supported_images_leaves_output_folder() {
    let root = TempRoot::new("batch-no-images");
    let photo_dir = photos(&root, "photos", &["notes.txt", "README"]);
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    let (code, captured) = run_mode(
        &mut backend,
        &root,
        CliMode::BenchmarkAll,
        &["--benchmark-all", &photo_dir, &out],
    );
    assert_eq!(code, 2);
    assert_eq!(
        captured.err,
        "PhotoReview.Benchmark.Cli: Benchmark folder contains no supported images\r\n"
    );
    assert!(
        Path::new(&out).is_dir(),
        "the report folder is created before the dataset check"
    );
    assert!(backend.ran.is_empty());
}

// An exception type outside the expected-failure filter is an unhandled crash: one line and the .NET exit code.
#[test]
fn batch_unexpected_error_returns_unhandled_exit_code() {
    let root = TempRoot::new("batch-unhandled");
    let photo_dir = photos(&root, "photos", &["a.jpg"]);
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    backend.summary_error = Some(ToolError::new("OperationCanceledException", "cancelled"));
    let (code, captured) = run_mode(
        &mut backend,
        &root,
        CliMode::BenchmarkAll,
        &["--benchmark-all", &photo_dir, &out],
    );
    assert_eq!(code, UNHANDLED_EXIT_CODE);
    assert_eq!(
        captured.err,
        "Unhandled exception. OperationCanceledException: cancelled\r\n"
    );
}

// ---- mode selection -----------------------------------------------------------------------------------------------------

// Only 64 supported files reach the engine, in the sorted order (`Take(64)`).
#[test]
fn batch_at_most_64_files_reach_backend() {
    let root = TempRoot::new("batch-64");
    let names: Vec<String> = (0..70).map(|i| format!("img{i:03}.jpg")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let photo_dir = photos(&root, "photos", &refs);
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    let (code, _) = run_mode(
        &mut backend,
        &root,
        CliMode::BenchmarkAll,
        &["--benchmark-all", &photo_dir, &out],
    );
    assert_eq!(code, 0);
    assert!(!backend.files_seen.is_empty());
    assert!(backend.files_seen.iter().all(|&n| n == 64), "{:?}", backend.files_seen);
}

// `--benchmark <folder>` with no profile list runs recommended-auto only. The default report folder is under %TEMP%:
// the test removes it afterwards.
#[test]
fn batch_benchmark_default_profile_is_recommended_auto() {
    let root = TempRoot::new("batch-default-profile");
    let photo_dir = photos(&root, "photos", &["a.jpg"]);
    let mut backend = FakeBackend::new(catalog());
    let (code, captured) = run_mode(&mut backend, &root, CliMode::Benchmark, &["--benchmark", &photo_dir]);
    assert_eq!(code, 0);
    assert_eq!(backend.ran, vec!["recommended-auto"]);
    let report_line = lines(&captured.out).last().copied().unwrap_or_default().to_owned();
    if let Some(summary) = report_line.strip_prefix("REPORT: ") {
        // Remove the default report folder this run created under %TEMP%.
        if let Some(dir) = Path::new(summary).parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

// `--benchmark-actions` selects only the file-action profiles (in catalog order).
#[test]
fn batch_actions_selects_file_action_profiles() {
    let root = TempRoot::new("batch-actions");
    let photo_dir = photos(&root, "photos", &["a.jpg"]);
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    let (code, _) = run_mode(
        &mut backend,
        &root,
        CliMode::BenchmarkActions,
        &["--benchmark-actions", &photo_dir, &out],
    );
    assert_eq!(code, 0);
    assert_eq!(backend.ran, vec!["action-move", "action-copy"]);
}

// ---- formatting (Program.cs strings) -------------------------------------------------------------------------------------

#[test]
fn start_line_matches_program_cs() {
    assert_eq!(
        format_start_line(&info("fast-balanced", false)),
        "START profile=fast-balanced workload=Sequential mode=Fast workers=2 window=3/1"
    );
}

#[test]
fn done_line_matches_program_cs_with_invariant_one_decimal() {
    let phase = PhaseSummary {
        status: PhaseStatus::Warn,
        p50: 0.75,
        p95: 1234.56,
        max: 3.0,
    };
    assert_eq!(
        format_done_line("fast-balanced", &phase, "C:\\out\\fast-balanced-r1.json"),
        "DONE profile=fast-balanced status=Warn p50=0.8ms p95=1234.6ms max=3.0ms report=C:\\out\\fast-balanced-r1.json"
    );
}

#[test]
fn done_line_rounds_exact_ties_half_to_even() {
    let phase = PhaseSummary {
        status: PhaseStatus::Pass,
        p50: 0.25,
        p95: -0.04,
        max: 2.5,
    };
    assert_eq!(
        format_done_line("p", &phase, "r"),
        "DONE profile=p status=Pass p50=0.2ms p95=-0.0ms max=2.5ms report=r"
    );
}

// C#: Program.cs `--benchmark-list-profiles` (one tab separated line per profile, `Console.WriteLine` line ends).
#[test]
fn profile_list_line_is_tab_separated() {
    let backend = FakeBackend::new(vec![info("logging-on", false)]);
    let mut out: Vec<u8> = Vec::new();
    write_profile_list(&mut out, &backend);
    assert_eq!(
        text(&out),
        "logging-on\tlogging-on\tSequential\tmode=Fast\tworkers=2\titerations=5\tcorrectnessOnly=False\tdescription of logging-on\r\n"
    );
}

// ---- golden: tests/golden/cli-dataset-plan.json ------------------------------------------------------------------------

fn golden_inputs(name: &str) -> HashMap<String, Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .expect("golden input has cases")
        .iter()
        .map(|c| (golden::str_of(c, "name").to_owned(), c.clone()))
        .collect()
}

#[test]
fn golden_cli_dataset_plan() {
    let golden = golden::load("cli-dataset-plan");
    let inputs = golden_inputs("cli-dataset-plan");
    let mut failures = Vec::new();
    for case in &golden.cases {
        let name = golden::str_of(case, "name");
        let input = inputs
            .get(name)
            .unwrap_or_else(|| panic!("no golden input named {name}"));
        let files: Vec<String> = input["files"]
            .as_array()
            .expect("files list")
            .iter()
            .map(|f| f.as_str().expect("path").to_owned())
            .collect();
        let got = plan_dataset(&files);
        let expected = &case["result"];
        match (expected.get("throws"), got) {
            (Some(type_name), Err(err)) => {
                if type_name != err.type_name.as_str() || golden::str_of(expected, "message") != err.message {
                    failures.push(format!(
                        "{name}: want {type_name} {:?}, got {} {:?}",
                        expected["message"], err.type_name, err.message
                    ));
                }
            }
            (Some(type_name), Ok(plan)) => failures.push(format!("{name}: want {type_name}, got ok {:?}", plan.files)),
            (None, Err(err)) => failures.push(format!("{name}: want ok, got {} {:?}", err.type_name, err.message)),
            (None, Ok(plan)) => {
                let want_files: Vec<String> = expected["ok"]["files"]
                    .as_array()
                    .expect("files")
                    .iter()
                    .map(|v| v.as_str().expect("path").to_owned())
                    .collect();
                let want_unsupported: Vec<(String, usize)> = expected["ok"]["unsupported"]
                    .as_array()
                    .expect("unsupported")
                    .iter()
                    .map(|pair| {
                        (
                            pair[0].as_str().expect("key").to_owned(),
                            pair[1].as_u64().expect("count") as usize,
                        )
                    })
                    .collect();
                if plan.files != want_files {
                    failures.push(format!("{name}: files {:?}, want {want_files:?}", plan.files));
                }
                if plan.unsupported_extensions != want_unsupported {
                    failures.push(format!(
                        "{name}: unsupported {:?}, want {want_unsupported:?}",
                        plan.unsupported_extensions
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} cli-dataset-plan golden mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// C# builds the engine's file list from `Directory.EnumerateFiles(folder, ...)` as listed (`rel\a.jpg` for a relative folder); only
// the manifest gets `Path.GetFullPath`. Decode errors that echo the path therefore name the listing text.
#[test]
fn batch_engine_receives_the_listing_paths_not_full_paths() {
    let root = TempRoot::new("batch-listing");
    let rel = format!("zz-w703d-rel-{}", &pr_core::io::physical::new_guid_n()[..6]);
    std::fs::create_dir_all(&rel).expect("relative photo folder");
    std::fs::write(Path::new(&rel).join("a.jpg"), [1u8]).expect("photo");
    let out = root.join("out").to_string_lossy().into_owned();
    let mut backend = FakeBackend::new(catalog());
    let (code, _) = run_mode(
        &mut backend,
        &root,
        CliMode::BenchmarkAll,
        &["--benchmark-all", &rel, &out],
    );
    let _ = std::fs::remove_dir_all(&rel);
    assert_eq!(code, 0);
    assert!(!backend.files_lists.is_empty());
    for list in &backend.files_lists {
        assert_eq!(list, &vec![format!("{rel}\\a.jpg")]);
    }
}

// C#: Directory.CreateDirectory on a name with an invalid character is IOException("... syntax is incorrect. : '<path>'.");
// the CLI prints it with the path suffix and exits 2 (probe on .NET 10).
#[test]
fn batch_report_directory_creation_failure_names_the_path() {
    let root = TempRoot::new("batch-badname");
    let photo_dir = photos(&root, "photos", &["a.jpg"]);
    let bad = format!("{}\\bad?name", root.path().display());
    let mut backend = FakeBackend::new(catalog());
    let (code, captured) = run_mode(
        &mut backend,
        &root,
        CliMode::BenchmarkAll,
        &["--benchmark-all", &photo_dir, &bad],
    );
    assert_eq!(code, 2);
    assert_eq!(
        captured.err,
        format!(
            "PhotoReview.Benchmark.Cli: The filename, directory name, or volume label syntax is incorrect. : '{bad}'.\r\n"
        )
    );
    assert!(backend.ran.is_empty());
}
