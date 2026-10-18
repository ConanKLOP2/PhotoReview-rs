//! Port of `PhotoReview.Benchmarking/BenchmarkEngine.cs` (C# baseline d92d6aae) tests: the three tests of
//! `tests/PhotoReview.Integration.Tests/BenchmarkEngineTimingTests.cs` plus golden-driven checks over
//! `tests/golden/bench-engine-run.json` (oracle `bench-engine-run`, inputs `tests/golden-inputs/bench-engine-run.json`) and
//! `tests/golden/bench-engine-format-ms.json` (oracle `bench-engine-format-ms`). Unit W7-03a, contract `contracts/W7-03a.md`.
//! Red stage: the stub bodies are `todo!()`, so every test here fails until the porter implements `run_prepared` and friends.
//!
//! Documented skips (not portable here): `BenchmarkEngineTimingTests.RunProfileAsync_SamplesExcludeSetupTime` (the runner,
//! W7-03c); the `BenchmarkPresentation`/`BenchmarkPresenter` tests that use `ProgressOk`/`ResultCorrectnessFailed` (App wave W8).
//!
//! The C# log-culture test is ported as a plain check of the two invariant-decimal fragments: Rust has no current culture.

use std::error::Error;
use std::io;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pr_bench::engine::{
    BenchmarkError, BenchmarkProgress, BenchmarkProgressSink, MeasureOutcome, PROGRESS_CORRECTNESS_FAILED, PROGRESS_OK,
    PreparedMeasure, RESULT_CORRECTNESS_FAILED, RunPreparedOptions, SystemTimeSource, TimeSource, elapsed_milliseconds,
    format_ms, run_prepared, ticks_to_milliseconds,
};
use pr_bench::models::{
    BenchmarkProfile, BenchmarkProfileError, BenchmarkProfileProblem, BenchmarkResultStatus, BenchmarkWorkload,
};
use pr_bench::profiles;
use pr_core::abstractions::{Clock, Log, NullLog, SystemClock};
use pr_core::diagnostics::metrics::{ReviewMetricsSnapshot, SourceOpenEntry};
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::get_full_path;
use pr_test_support::golden::{self, i64_of, str_of};
use pr_test_support::{FakeClock, TempRoot};
use serde_json::{Value, json};

/// Pinned run id (32 lowercase hex digits), normalised to `<run>` in log lines like the oracle does.
const RUN_ID: &str = "0123456789abcdef0123456789abcdef";
/// Pinned machine name for the golden loop (the default-only check lives in the production smoke test).
const MACHINE: &str = "PINNED-MACHINE";
/// .NET ticks per second: the frequency of the scripted clock, so counter ticks are TimeSpan ticks.
const TICKS_PER_SECOND: i64 = 10_000_000;
/// Measure-step length when a golden step does not give `measure_ticks` (oracle default).
const DEFAULT_MEASURE_TICKS: i64 = 10_000;

// ---------------------------------------------------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------------------------------------------------

/// C# `ManualTimeProvider`: a hand-driven counter, frequency = `TimeSpan.TicksPerSecond`.
#[derive(Debug, Default)]
struct ManualTime {
    ticks: AtomicI64,
}

impl ManualTime {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn advance(&self, ticks: i64) {
        self.ticks.fetch_add(ticks, Ordering::SeqCst);
    }
}

impl TimeSource for ManualTime {
    fn timestamp(&self) -> i64 {
        self.ticks.load(Ordering::SeqCst)
    }

    fn timestamp_frequency(&self) -> i64 {
        TICKS_PER_SECOND
    }
}

/// Records the order of prepare and measure steps (the C# `order` / `callOrder` lists).
#[derive(Debug, Default)]
struct Calls(Mutex<Vec<&'static str>>);

impl Calls {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn push(&self, step: &'static str) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(step);
    }

    fn snapshot(&self) -> Vec<&'static str> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Local `ILog` (not `RecordingLog`): `INFO {msg}`, `WARN {msg}`, and `ERROR {msg}` plus ` | {type}` when an error is attached,
/// with `{type}` the C# exception name of the attached [`BenchmarkError`] (oracle `W703aLog`).
#[derive(Debug, Default)]
struct TestLog {
    lines: Mutex<Vec<String>>,
}

impl TestLog {
    fn lines(&self) -> Vec<String> {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn push(&self, line: String) {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner).push(line);
    }
}

impl Log for TestLog {
    fn enabled(&self) -> bool {
        true
    }

    fn info(&self, message: &str) {
        self.push(format!("INFO {message}"));
    }

    fn warn(&self, message: &str) {
        self.push(format!("WARN {message}"));
    }

    fn error(&self, message: &str, error: Option<&(dyn Error + 'static)>) {
        let suffix = match error {
            None => String::new(),
            Some(e) => format!(" | {}", exception_name_of(e)),
        };
        self.push(format!("ERROR {message}{suffix}"));
    }
}

/// C# exception type name of a failure, for the log and the golden `exception` field.
fn exception_name(error: &BenchmarkError) -> &'static str {
    match error {
        BenchmarkError::FolderRequired => "ArgumentException",
        BenchmarkError::DirectoryNotFound(_) => "DirectoryNotFoundException",
        BenchmarkError::Profile(_) => "BenchmarkProfileException",
        BenchmarkError::Canceled => "OperationCanceledException",
        BenchmarkError::Operation(_) => "InvalidOperationException",
    }
}

fn exception_name_of(error: &(dyn Error + 'static)) -> &'static str {
    error
        .downcast_ref::<BenchmarkError>()
        .map_or("UnknownException", exception_name)
}

/// The oracle's cumulative snapshot after `calls` measure steps: two `top_source_opens` entries (`W703aCumulative`).
fn cumulative(calls: i64) -> ReviewMetricsSnapshot {
    ReviewMetricsSnapshot {
        cache_hits: calls * 2,
        cache_misses: calls,
        source_bytes_read: calls * 1000,
        source_reads: calls * 5,
        decode_milliseconds: calls * 7,
        preload_hits: calls * 3,
        disk_cache_hits: calls,
        source_open_count: calls * 5,
        top_source_opens: vec![
            SourceOpenEntry {
                path: "a.jpg".to_owned(),
                count: calls * 5,
            },
            SourceOpenEntry {
                path: "b.jpg".to_owned(),
                count: calls,
            },
        ],
        ..Default::default()
    }
}

/// The C# test's own `Cumulative` helper: ONE `top_source_opens` entry (`a.jpg`), so `Assert.Single` holds.
fn cumulative_single(calls: i64) -> ReviewMetricsSnapshot {
    ReviewMetricsSnapshot {
        cache_hits: calls * 2,
        cache_misses: calls,
        source_bytes_read: calls * 1000,
        source_reads: calls * 5,
        decode_milliseconds: calls * 7,
        preload_hits: calls * 3,
        disk_cache_hits: calls,
        source_open_count: calls * 5,
        top_source_opens: vec![SourceOpenEntry {
            path: "a.jpg".to_owned(),
            count: calls * 5,
        }],
        ..Default::default()
    }
}

/// Scripted executor of the golden inputs. Operation call `k` (warm-ups first, then timed iterations) uses `steps[k]`;
/// past the end the step is `{prepare 0, measure 10000 ticks, correct, no metrics}` (oracle `W703aOperation`).
fn scripted(
    steps: Vec<Value>,
    time: Arc<ManualTime>,
    calls: Arc<Calls>,
    cancel: CancelToken,
) -> impl FnMut(&BenchmarkProfile, BenchmarkWorkload, i32, &CancelToken) -> Result<PreparedMeasure, BenchmarkError> {
    let mut call = 0_usize;
    move |_profile: &BenchmarkProfile,
          _workload: BenchmarkWorkload,
          _iteration: i32,
          _ct: &CancelToken|
          -> Result<PreparedMeasure, BenchmarkError> {
        let step = steps.get(call).cloned().unwrap_or_else(|| json!({}));
        call += 1;
        calls.push("prepare");
        time.advance(step.get("prepare_ticks").and_then(Value::as_i64).unwrap_or(0));
        match step.get("throw").and_then(Value::as_str) {
            Some("prepare") => return Err(BenchmarkError::Operation(Box::new(io::Error::other("boom")))),
            Some("cancel-prepare") => return Err(BenchmarkError::Canceled),
            _ => {}
        }
        let correct = step.get("correct").and_then(Value::as_bool).unwrap_or(true);
        let metrics = step.get("metrics_calls").and_then(Value::as_i64).map(cumulative);
        let throw = step.get("throw").and_then(Value::as_str).map(str::to_owned);
        let cancel_after = step.get("cancel_token_after").and_then(Value::as_bool) == Some(true);
        let measure_ticks = step
            .get("measure_ticks")
            .and_then(Value::as_i64)
            .unwrap_or(DEFAULT_MEASURE_TICKS);
        let (calls, time, cancel) = (Arc::clone(&calls), Arc::clone(&time), cancel.clone());
        let measure: PreparedMeasure = Box::new(move || {
            calls.push("measure");
            time.advance(measure_ticks);
            match throw.as_deref() {
                Some("measure") => return Err(BenchmarkError::Operation(Box::new(io::Error::other("boom")))),
                Some("cancel-measure") => return Err(BenchmarkError::Canceled),
                _ => {}
            }
            if cancel_after {
                cancel.cancel();
            }
            Ok(MeasureOutcome { correct, metrics })
        });
        Ok(measure)
    }
}

/// Reads `tests/golden-inputs/<name>.json` (the inputs the oracle ran with) and returns its `cases` array.
fn read_inputs(name: &str) -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

/// `metrics` JSON subset written by the oracle (every key the golden rows carry).
fn metrics_json(m: &ReviewMetricsSnapshot) -> Value {
    json!({
        "cache_hits": m.cache_hits,
        "cache_misses": m.cache_misses,
        "source_bytes_read": m.source_bytes_read,
        "source_reads": m.source_reads,
        "decode_milliseconds": m.decode_milliseconds,
        "preload_hits": m.preload_hits,
        "disk_cache_hits": m.disk_cache_hits,
        "source_open_count": m.source_open_count,
        "top_source_opens": m
            .top_source_opens
            .iter()
            .map(|e| json!({ "path": e.path, "count": e.count }))
            .collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported C# tests (BenchmarkEngineTimingTests)
// ---------------------------------------------------------------------------------------------------------------------

// C#: BenchmarkEngineTimingTests.RunPreparedAsync_SamplesExcludePrepareTime
#[test]
fn run_prepared_async_samples_exclude_prepare_time() {
    let time = ManualTime::new();
    let order = Calls::new();
    let root = TempRoot::new("bench-timing");
    let profile = BenchmarkProfile {
        iterations: 3,
        warmup_count: 1,
        ..profiles::find("action-move").expect("action-move").clone()
    };
    let clock = FakeClock::new();
    let log = TestLog::default();
    let cancel = CancelToken::new();
    let mut executor = {
        let (time, order) = (Arc::clone(&time), Arc::clone(&order));
        move |_: &BenchmarkProfile,
              _: BenchmarkWorkload,
              _: i32,
              _: &CancelToken|
              -> Result<PreparedMeasure, BenchmarkError> {
            order.push("prepare");
            time.advance(5_000_000); // 500 ms of setup I/O: must not show up in the sample
            let (time, order) = (Arc::clone(&time), Arc::clone(&order));
            let measure: PreparedMeasure = Box::new(move || {
                order.push("measure");
                time.advance(100_000); // 10 ms
                Ok(MeasureOutcome {
                    correct: true,
                    metrics: None,
                })
            });
            Ok(measure)
        }
    };
    let options = RunPreparedOptions {
        progress: None,
        time: &*time,
        clock: &clock,
        log: &log,
        cancel: &cancel,
        run_id: Some(RUN_ID),
        machine: Some(MACHINE),
    };

    let folder = root.path().to_string_lossy().into_owned();
    let report = run_prepared(&folder, &profile, &mut executor, &options).expect("run succeeds");

    assert_eq!(report.phases.len(), 1, "single phase");
    let phase = &report.phases[0];
    assert_eq!(phase.samples(), &[10.0, 10.0, 10.0]);
    assert_eq!(phase.status, BenchmarkResultStatus::Pass);
    // warm-up + 3 timed iterations, each strictly prepare then measure
    assert_eq!(
        order.snapshot(),
        [
            "prepare", "measure", "prepare", "measure", "prepare", "measure", "prepare", "measure"
        ]
    );
}

// C#: BenchmarkEngineTimingTests.RunPreparedAsync_MetricsExcludeWarmup
#[test]
fn run_prepared_async_metrics_exclude_warmup() {
    let time = ManualTime::new();
    let root = TempRoot::new("bench-warmup-metrics");
    let profile = BenchmarkProfile {
        iterations: 3,
        warmup_count: 2,
        ..profiles::find("action-move").expect("action-move").clone()
    };
    let clock = FakeClock::new();
    let log = TestLog::default();
    let cancel = CancelToken::new();
    let calls = Arc::new(Mutex::new(0_i64));
    // Like BenchmarkImageExecutor.Metrics, every step reports the executor's cumulative totals.
    let mut executor = {
        let calls = Arc::clone(&calls);
        move |_: &BenchmarkProfile,
              _: BenchmarkWorkload,
              _: i32,
              _: &CancelToken|
              -> Result<PreparedMeasure, BenchmarkError> {
            let calls = Arc::clone(&calls);
            let measure: PreparedMeasure = Box::new(move || {
                let mut count = calls.lock().unwrap_or_else(PoisonError::into_inner);
                *count += 1;
                Ok(MeasureOutcome {
                    correct: true,
                    metrics: Some(cumulative_single(*count)),
                })
            });
            Ok(measure)
        }
    };
    let options = RunPreparedOptions {
        progress: None,
        time: &*time,
        clock: &clock,
        log: &log,
        cancel: &cancel,
        run_id: Some(RUN_ID),
        machine: Some(MACHINE),
    };

    let folder = root.path().to_string_lossy().into_owned();
    let report = run_prepared(&folder, &profile, &mut executor, &options).expect("run succeeds");

    assert_eq!(
        *calls.lock().unwrap_or_else(PoisonError::into_inner),
        5,
        "warm-ups plus samples"
    );
    let metrics = report.metrics.as_ref().expect("metrics present");
    assert_eq!(metrics.source_reads, 15);
    assert_eq!(metrics.cache_hits, 6);
    assert_eq!(metrics.cache_misses, 3);
    assert_eq!(metrics.source_bytes_read, 3000);
    assert_eq!(metrics.preload_hits, 9);
    assert_eq!(metrics.disk_cache_hits, 3);
    assert_eq!(metrics.source_open_count, 15);
    assert_eq!(metrics.top_source_opens.len(), 1, "Assert.Single(TopSourceOpens)");
    assert_eq!(metrics.top_source_opens[0].count, 15);
}

// C#: BenchmarkEngineTimingTests.RunPreparedAsync_LogUsesInvariantDecimals
// Rust has no current culture (the C# test switched to vi-VN): only the invariant-decimal fragments are checked.
#[test]
fn run_prepared_async_log_uses_invariant_decimals() {
    let time = ManualTime::new();
    let root = TempRoot::new("bench-log-culture");
    let profile = BenchmarkProfile {
        iterations: 1,
        warmup_count: 0,
        ..profiles::find("action-move").expect("action-move").clone()
    };
    let clock = FakeClock::new();
    let log = TestLog::default();
    let cancel = CancelToken::new();
    let mut executor = {
        let time = Arc::clone(&time);
        move |_: &BenchmarkProfile,
              _: BenchmarkWorkload,
              _: i32,
              _: &CancelToken|
              -> Result<PreparedMeasure, BenchmarkError> {
            let time = Arc::clone(&time);
            let measure: PreparedMeasure = Box::new(move || {
                time.advance(125_000); // 12.5 ms
                Ok(MeasureOutcome {
                    correct: true,
                    metrics: None,
                })
            });
            Ok(measure)
        }
    };
    let options = RunPreparedOptions {
        progress: None,
        time: &*time,
        clock: &clock,
        log: &log,
        cancel: &cancel,
        run_id: Some(RUN_ID),
        machine: Some(MACHINE),
    };

    let folder = root.path().to_string_lossy().into_owned();
    run_prepared(&folder, &profile, &mut executor, &options).expect("run succeeds");

    let lines = log.lines();
    assert!(
        lines.iter().any(|l| l.contains("elapsedMs=12.5 ")),
        "elapsedMs fragment missing: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("p50Ms=12.5 p95Ms=12.5 maxMs=12.5")),
        "percentile fragment missing: {lines:?}"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven tests
// ---------------------------------------------------------------------------------------------------------------------

/// Fields the oracle writes that depend on the machine, the clock or the temp path: checked only by the production
/// smoke test (the golden loop pins the run id, the machine and the folder by normalisation).
const DEFAULT_ONLY_FIELDS: [&str; 4] = [
    "run_id_is_32_lower_hex",
    "started_utc_between_call_start_and_end",
    "report_folder_is_full_path",
    "machine_is_environment_machine_name",
];

/// Runs one `bench-engine-run` input and returns the observed fields in the golden row's shape.
fn observe_run_case(input: &Value) -> (Value, Option<Vec<f64>>) {
    let root = TempRoot::new("bench-engine-golden");
    let root_text = root.path().to_string_lossy().into_owned();
    let folder = match input["folder"].as_str().expect("folder kind") {
        "temp" => root_text.clone(),
        "empty" => String::new(),
        "whitespace" => "  ".to_owned(),
        "missing" => root.join("missing").to_string_lossy().into_owned(),
        "file" => {
            let path = root.join("afile.txt");
            std::fs::write(&path, [1_u8]).expect("write file");
            path.to_string_lossy().into_owned()
        }
        other => panic!("unknown folder kind {other}"),
    };
    let full_folder = if folder.trim().is_empty() {
        String::new()
    } else {
        get_full_path(&folder).unwrap_or_default()
    };

    let mut profile = profiles::find(input["profile"]["id"].as_str().expect("profile id"))
        .expect("profile in registry")
        .clone();
    if let Some(n) = input["profile"].get("iterations").and_then(Value::as_i64) {
        profile.iterations = n as i32;
    }
    if let Some(n) = input["profile"].get("warmup").and_then(Value::as_i64) {
        profile.warmup_count = n as i32;
    }
    if let Some(n) = input["profile"].get("workers").and_then(Value::as_i64) {
        profile.workers = n as i32;
    }
    if input["profile"].get("blank_id").and_then(Value::as_bool) == Some(true) {
        profile.id = " ".to_owned();
    }

    let steps = input
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let time = ManualTime::new();
    let calls = Calls::new();
    let cancel = CancelToken::new();
    let mut executor = scripted(steps, Arc::clone(&time), Arc::clone(&calls), cancel.clone());
    let progress: Mutex<Vec<BenchmarkProgress>> = Mutex::new(Vec::new());
    let sink = |p: BenchmarkProgress| progress.lock().unwrap_or_else(PoisonError::into_inner).push(p);
    let clock = FakeClock::new();
    let log = TestLog::default();
    let options = RunPreparedOptions {
        progress: Some(&sink),
        time: &*time,
        clock: &clock,
        log: &log,
        cancel: &cancel,
        run_id: Some(RUN_ID),
        machine: Some(MACHINE),
    };

    let result = run_prepared(&folder, &profile, &mut executor, &options);
    let normalise = |line: String| {
        let line = if full_folder.is_empty() {
            line
        } else {
            line.replace(&full_folder, "<folder>")
        };
        line.replace(RUN_ID, "<run>")
    };
    let log_lines: Vec<Value> = log.lines().into_iter().map(|l| json!(normalise(l))).collect();
    let progress_values: Vec<Value> = progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|p| {
            json!({
                "profile_id": p.profile_id,
                "workload": p.workload.name(),
                "completed": p.completed,
                "total": p.total,
                "message": p.message,
            })
        })
        .collect();
    let call_order: Vec<Value> = calls.snapshot().into_iter().map(|s| json!(s)).collect();

    let mut actual = json!({
        "exception": null,
        "exception_message": null,
        "profile_problem": null,
        "log": log_lines,
        "progress": progress_values,
        "call_order": call_order,
    });
    let mut samples = None;
    match &result {
        Ok(report) => {
            let phase = &report.phases[0];
            samples = Some(phase.samples().to_vec());
            actual["phase_count"] = json!(report.phases.len());
            actual["profile_id"] = json!(phase.profile_id);
            actual["workload"] = json!(phase.workload.name());
            actual["status"] = json!(phase.status.name());
            actual["message"] = json!(phase.message);
            actual["images_per_sample"] = json!(phase.images_per_sample);
            actual["metrics"] = report.metrics.as_ref().map_or(Value::Null, metrics_json);
        }
        Err(error) => {
            actual["exception"] = json!(exception_name(error));
            actual["exception_message"] = json!(error.to_string().replace(&root_text, "<root>"));
            actual["profile_problem"] = match error {
                BenchmarkError::Profile(e) => json!(e.problem.name()),
                _ => Value::Null,
            };
        }
    }
    (actual, samples)
}

#[test]
fn golden_bench_engine_run_matches_csharp() {
    let golden = golden::load("bench-engine-run");
    let inputs = read_inputs("bench-engine-run");
    assert_eq!(golden.cases.len(), inputs.len(), "golden and input case counts");

    for (row, input) in golden.cases.iter().zip(inputs.iter()) {
        let name = str_of(row, "name");
        assert_eq!(name, input["name"].as_str().expect("input name"), "case order");
        let (actual, samples) = observe_run_case(input);

        for (key, expected) in row.as_object().expect("golden row object") {
            if key == "name" || key == "profile" || key == "folder_kind" || DEFAULT_ONLY_FIELDS.contains(&key.as_str())
            {
                continue;
            }
            if key == "samples" {
                let want: Vec<f64> = expected
                    .as_array()
                    .expect("samples array")
                    .iter()
                    .map(|v| v.as_f64().expect("sample number"))
                    .collect();
                assert_eq!(samples.as_deref(), Some(want.as_slice()), "case {name}: samples");
                continue;
            }
            let got = actual.get(key).cloned().unwrap_or(Value::Null);
            assert_eq!(&got, expected, "case {name}: field {key}");
        }
    }
}

#[test]
fn golden_bench_engine_format_ms_matches_csharp() {
    let golden = golden::load("bench-engine-format-ms");
    let inputs = read_inputs("bench-engine-format-ms");
    assert_eq!(golden.cases.len(), inputs.len(), "golden and input case counts");

    for (row, input) in golden.cases.iter().zip(inputs.iter()) {
        let name = str_of(row, "name");
        assert_eq!(name, input["name"].as_str().expect("input name"), "case order");
        if row.get("ticks").is_some() {
            // `TimeSpan.FromTicks(ticks).TotalMilliseconds` formatted with "F1" (the private `Ms`).
            let ticks = i64_of(row, "ticks");
            assert_eq!(
                format_ms(ticks_to_milliseconds(ticks)),
                str_of(row, "text"),
                "case {name}"
            );
        } else {
            // `GetElapsedTime(start, end).Ticks`, compared through the milliseconds the engine returns (x 10 000 back to ticks).
            let frequency = i64_of(row, "frequency");
            let (start, end) = (i64_of(row, "start"), i64_of(row, "end"));
            let got = (elapsed_milliseconds(frequency, start, end) * 10_000.0).round() as i64;
            assert_eq!(got, i64_of(row, "elapsed_ticks"), "case {name}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Rust-only tests (hand-written: no C# counterpart)
// ---------------------------------------------------------------------------------------------------------------------

// new: `Ms` ties go to even on the exact binary value (verified on .NET 10: 0.25 -> 0.2, 0.75 -> 0.8, 0.05 -> 0.1).
#[test]
fn format_ms_rounds_ties_to_even_on_the_exact_binary_value() {
    assert_eq!(format_ms(0.25), "0.2");
    assert_eq!(format_ms(0.75), "0.8");
    assert_eq!(format_ms(1.25), "1.2");
    assert_eq!(format_ms(1.75), "1.8");
    assert_eq!(format_ms(2.25), "2.2");
    assert_eq!(format_ms(0.05), "0.1", "0.05 is slightly above the tie");
    assert_eq!(format_ms(0.4999), "0.5");
    assert_eq!(format_ms(0.04999), "0.0");
}

// new: negative values keep their sign; a tiny negative value prints `-0.0` like .NET.
#[test]
fn format_ms_keeps_sign_and_negative_zero() {
    assert_eq!(format_ms(-0.0001), "-0.0");
    assert_eq!(format_ms(-0.0), "-0.0");
    assert_eq!(format_ms(-0.25), "-0.2");
    assert_eq!(format_ms(-0.75), "-0.8");
    assert_eq!(format_ms(-1.75), "-1.8");
    assert_eq!(format_ms(-12.5), "-12.5");
}

// new: large values print plainly, with no grouping separators and always one decimal.
#[test]
fn format_ms_large_values_have_no_grouping() {
    assert_eq!(format_ms(0.0), "0.0");
    assert_eq!(format_ms(1234.5), "1234.5");
    assert_eq!(format_ms(123_456.75), "123456.8");
    assert_eq!(format_ms(1e15), "1000000000000000.0");
    assert_eq!(format_ms(922_337_203_685_477.0), "922337203685477.0");
}

// new: `GetElapsedTime` = `(end - start) * (10_000_000 / frequency)` ticks, then TotalMilliseconds.
#[test]
fn elapsed_milliseconds_converts_counter_ticks_to_milliseconds() {
    // 10 MHz counter: 250 ticks = 250 x 100 ns = 0.025 ms.
    assert_eq!(elapsed_milliseconds(10_000_000, 100, 350), 250.0 / 10_000.0);
    // Nanosecond counter: 1 ms = 1 000 000 ns.
    assert_eq!(elapsed_milliseconds(1_000_000_000, 0, 1_000_000), 1.0);
    assert_eq!(elapsed_milliseconds(10_000_000, 42, 42), 0.0, "zero delta");
}

// new: the conversion to TimeSpan ticks truncates toward zero, so odd frequencies lose the fraction.
#[test]
fn elapsed_milliseconds_truncates_toward_zero() {
    // 1 tick of a 3 Hz counter is 3 333 333.33 TimeSpan ticks, truncated to 3 333 333.
    assert_eq!(elapsed_milliseconds(3, 0, 1), 3_333_333.0 / 10_000.0);
    assert_eq!(
        elapsed_milliseconds(3, 1, 0),
        -3_333_333.0 / 10_000.0,
        "negative fraction truncates toward zero"
    );
}

// new: a negative delta gives a negative value.
#[test]
fn elapsed_milliseconds_negative_delta_is_negative() {
    assert_eq!(elapsed_milliseconds(10_000_000, 350, 100), -250.0 / 10_000.0);
}

// new: `TimeSpan.TotalMilliseconds` is clamped to the .NET range (long.MaxValue / 10 000 as whole milliseconds).
#[test]
fn ticks_to_milliseconds_clamps_to_the_dotnet_range() {
    assert_eq!(ticks_to_milliseconds(0), 0.0);
    assert_eq!(ticks_to_milliseconds(10_000), 1.0);
    assert_eq!(ticks_to_milliseconds(-10_000), -1.0);
    assert_eq!(ticks_to_milliseconds(i64::MAX), 922_337_203_685_477.0);
    assert_eq!(ticks_to_milliseconds(i64::MIN), -922_337_203_685_477.0);
}

// new: `BenchmarkError` Display texts equal the C# exception messages.
#[test]
fn benchmark_error_display_texts_match_csharp_messages() {
    assert_eq!(
        BenchmarkError::FolderRequired.to_string(),
        "Folder is required (Parameter 'folder')"
    );
    assert_eq!(
        BenchmarkError::DirectoryNotFound("C:/x/missing".to_owned()).to_string(),
        "C:/x/missing"
    );
    let profile_error = BenchmarkProfileError::new(
        BenchmarkProfileProblem::InvalidSettings,
        "action-move",
        "Invalid benchmark settings for profile 'action-move'",
    );
    assert_eq!(
        BenchmarkError::Profile(profile_error).to_string(),
        "Invalid benchmark settings for profile 'action-move'"
    );
    assert_eq!(BenchmarkError::Canceled.to_string(), "The operation was canceled.");
    assert_eq!(
        BenchmarkError::Operation(Box::new(io::Error::other("boom"))).to_string(),
        "boom"
    );
}

// new: `Error::source` exposes the wrapped error for `Profile` and `Operation` only.
#[test]
fn benchmark_error_source_is_the_wrapped_error() {
    let profile_error = BenchmarkProfileError::new(
        BenchmarkProfileProblem::MissingIdOrMode,
        " ",
        "Profile id and loading mode are required",
    );
    let profile = BenchmarkError::Profile(profile_error);
    assert_eq!(
        profile.source().map(ToString::to_string).as_deref(),
        Some("Profile id and loading mode are required")
    );

    let operation = BenchmarkError::Operation(Box::new(io::Error::other("boom")));
    assert_eq!(operation.source().map(ToString::to_string).as_deref(), Some("boom"));

    assert!(BenchmarkError::FolderRequired.source().is_none());
    assert!(BenchmarkError::DirectoryNotFound("x".to_owned()).source().is_none());
    assert!(BenchmarkError::Canceled.source().is_none());
}

// new: the `From` conversions used by `?` in the engine and the models.
#[test]
fn benchmark_error_from_impls_map_to_their_variants() {
    assert!(matches!(
        BenchmarkError::from(pr_core::sync::Canceled),
        BenchmarkError::Canceled
    ));
    let profile_error = BenchmarkProfileError::new(BenchmarkProfileProblem::InvalidSettings, "x", "m");
    assert!(matches!(
        BenchmarkError::from(profile_error),
        BenchmarkError::Profile(_)
    ));
}

// new: the system time source counts nanoseconds and never goes backwards.
#[test]
fn system_time_source_counts_nanoseconds_monotonically() {
    let source = SystemTimeSource;
    assert_eq!(source.timestamp_frequency(), 1_000_000_000);
    let first = source.timestamp();
    let second = source.timestamp();
    assert!(second >= first, "monotonic: {first} then {second}");
}

// new: production defaults (`RunPreparedOptions::new`, system time and clock, no progress, unpinned run id and machine).
#[test]
fn run_prepared_production_defaults_smoke() {
    let root = TempRoot::new("bench-engine-defaults");
    let folder = root.path().to_string_lossy().into_owned();
    let profile = BenchmarkProfile {
        iterations: 1,
        warmup_count: 0,
        ..profiles::find("action-move").expect("action-move").clone()
    };
    let token = CancelToken::new();
    let options = RunPreparedOptions::new(&NullLog, &token);
    let mut executor = |_: &BenchmarkProfile,
                        _: BenchmarkWorkload,
                        _: i32,
                        _: &CancelToken|
     -> Result<PreparedMeasure, BenchmarkError> {
        let measure: PreparedMeasure = Box::new(|| {
            Ok(MeasureOutcome {
                correct: true,
                metrics: None,
            })
        });
        Ok(measure)
    };

    let before = SystemClock.utc_now();
    let report = run_prepared(&folder, &profile, &mut executor, &options).expect("defaults run succeeds");
    let after = SystemClock.utc_now();

    assert_eq!(report.run_id.len(), 32, "run id length");
    assert!(
        report
            .run_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "run id is lowercase hex: {}",
        report.run_id
    );
    assert_eq!(report.machine.as_deref(), std::env::var("COMPUTERNAME").ok().as_deref());
    assert_eq!(report.folder, get_full_path(&folder).expect("full path"));
    let started = report.started_utc.utc();
    assert!(
        before <= started && started <= after,
        "started_utc between two clock reads"
    );
    let samples = report.phases[0].samples();
    assert_eq!(samples.len(), 1);
    assert!(samples[0].is_finite() && samples[0] >= 0.0, "sample {}", samples[0]);
}

// new: the message constants the presenter and the report compare against.
#[test]
fn progress_and_result_message_constants_are_the_english_texts() {
    assert_eq!(PROGRESS_OK, "OK");
    assert_eq!(PROGRESS_CORRECTNESS_FAILED, "Correctness failed");
    assert_eq!(RESULT_CORRECTNESS_FAILED, "One or more samples failed correctness");
}

/// Keeps the sink closure type exercised: a `Fn(BenchmarkProgress)` closure is a `BenchmarkProgressSink`.
#[test]
fn closure_is_a_progress_sink() {
    let seen = Mutex::new(0_usize);
    let sink = |_: BenchmarkProgress| *seen.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    let as_sink: &dyn BenchmarkProgressSink = &sink;
    as_sink.report(BenchmarkProgress {
        profile_id: "action-move".to_owned(),
        workload: BenchmarkWorkload::FileAction,
        completed: 1,
        total: 1,
        message: PROGRESS_OK.to_owned(),
    });
    assert_eq!(*seen.lock().unwrap_or_else(PoisonError::into_inner), 1);
}

// Repair round 1: tests killing mutants in SystemTimeSource, Debug, get_full_path and timestamp_frequency.
struct Ms1000(AtomicI64);
impl TimeSource for Ms1000 {
    fn timestamp(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    fn timestamp_frequency(&self) -> i64 {
        1000
    }
}

#[derive(Default)]
struct CaptureLog(Mutex<Vec<String>>);
impl Log for CaptureLog {
    fn enabled(&self) -> bool {
        true
    }
    fn info(&self, m: &str) {
        self.0.lock().unwrap().push(m.to_owned());
    }
    fn warn(&self, _: &str) {}
    fn error(&self, _: &str, _: Option<&(dyn Error + 'static)>) {}
}

fn single_iteration_profile() -> BenchmarkProfile {
    BenchmarkProfile {
        iterations: 1,
        warmup_count: 0,
        ..profiles::find("action-move").expect("profile").clone()
    }
}

// mutant: SystemTimeSource::timestamp -> 0 / 1 / -1
#[test]
fn system_time_source_advances_with_real_time() {
    let s = SystemTimeSource;
    let a = s.timestamp();
    assert!(a >= 0);
    std::thread::sleep(std::time::Duration::from_millis(30));
    let b = s.timestamp();
    assert!(
        b - a >= 20_000_000,
        "30 ms sleep must advance >= 20e6 ns, got {}",
        b - a
    );
}

// mutant: Debug for RunPreparedOptions -> Ok(Default)
#[test]
fn options_debug_names_the_type_and_pins() {
    let token = CancelToken::new();
    let mut o = RunPreparedOptions::new(&NullLog, &token);
    o.run_id = Some("abc");
    let text = format!("{o:?}");
    assert!(text.contains("RunPreparedOptions") && text.contains("abc"), "{text}");
}

// mutant: report folder / start log use the folder as given instead of the full path (relative folder)
#[test]
fn relative_folder_is_reported_as_full_path() {
    let log = CaptureLog::default();
    let token = CancelToken::new();
    let mut o = RunPreparedOptions::new(&log, &token);
    o.run_id = Some("0123456789abcdef0123456789abcdef");
    let mut exec = |_: &BenchmarkProfile,
                    _: BenchmarkWorkload,
                    _: i32,
                    _: &CancelToken|
     -> Result<PreparedMeasure, BenchmarkError> {
        Ok(Box::new(|| {
            Ok(MeasureOutcome {
                correct: true,
                metrics: None,
            })
        }))
    };
    let report = run_prepared(".", &single_iteration_profile(), &mut exec, &o).expect("run");
    let full = get_full_path(".").expect("full");
    assert_ne!(full, ".");
    assert_eq!(report.folder, full);
    let lines = log.0.lock().unwrap().clone();
    assert!(lines[0].contains(&format!("folder={full} iterations=")), "{}", lines[0]);
}

// mutant: engine ignores timestamp_frequency() (hard-codes 10 MHz)
#[test]
fn non_default_frequency_scales_the_sample() {
    let time = Arc::new(Ms1000(AtomicI64::new(0)));
    let token = CancelToken::new();
    let mut o = RunPreparedOptions::new(&NullLog, &token);
    o.time = &*time;
    let t2 = Arc::clone(&time);
    let mut exec = move |_: &BenchmarkProfile,
                         _: BenchmarkWorkload,
                         _: i32,
                         _: &CancelToken|
          -> Result<PreparedMeasure, BenchmarkError> {
        let t = Arc::clone(&t2);
        Ok(Box::new(move || {
            t.0.fetch_add(250, Ordering::SeqCst);
            Ok(MeasureOutcome {
                correct: true,
                metrics: None,
            })
        }))
    };
    let folder = std::env::temp_dir().to_string_lossy().into_owned();
    let report = run_prepared(&folder, &single_iteration_profile(), &mut exec, &o).expect("run");
    assert_eq!(report.phases[0].samples(), &[250.0]);
}
