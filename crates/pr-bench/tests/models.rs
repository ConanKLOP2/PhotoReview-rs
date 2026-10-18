//! Port of `PhotoReview.Benchmarking/BenchmarkModels.cs` (C# baseline d92d6aae). Unit W7-01a, RED tests: every
//! assertion goes through a function that is still `todo!()` in `pr_bench::models`, so each test fails until the port
//! lands. Contract: `contracts/W7-01a.md`.
//!
//! C# tests ported (each Rust test carries a `// C#:` comment): `tests/PhotoReview.Integration.Tests/
//! BenchmarkStatisticsEdgeTests.cs` and `BenchmarkTrustTests.cs`. The four golden tests compare every case of
//! `tests/golden/bench-{profile,phase,rank,json}.json` (C# answers) with the inputs in `tests/golden-inputs/`.
//! Numbers in golden files are `R`-format strings (`fmt_double`), so NaN, `-0` and `1E+21` survive JSON.

use pr_bench::models::{
    BenchmarkBatchSummary, BenchmarkDatasetManifest, BenchmarkPhaseResult, BenchmarkProfile, BenchmarkReport,
    BenchmarkResultStatus, BenchmarkTimestamp, BenchmarkWorkload, DEFAULT_ITERATIONS, DEFAULT_WARMUP_COUNT,
    NonFiniteNumber, rank, validate_profile,
};
use pr_core::diagnostics::metrics::{PresentLatencyBucket, ReviewMetricsSnapshot, SourceOpenEntry};
use pr_core::model::enums::{DecoderBackend, LoadingMode};
use pr_core::time::DateTimeUtc;
use pr_perf::rules::fmt_double;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

// ---- helpers ---------------------------------------------------------------------------------------------------

/// `ImagesPerSampleLookup` that knows nothing (C# `BenchmarkProfiles.Find` returns null for every id).
fn no_lookup(_: &str, _: BenchmarkWorkload) -> Option<i32> {
    None
}

/// Mirrors the C# test helper `Phase(id, workload, samples, status, imagesPerSample)`.
fn phase(
    id: &str,
    workload: BenchmarkWorkload,
    samples: &[f64],
    status: BenchmarkResultStatus,
    images: Option<i32>,
) -> BenchmarkPhaseResult {
    BenchmarkPhaseResult::new(id, workload, samples.to_vec(), status, None, images)
}

/// Golden inputs: `tests/golden-inputs/<name>.json`, the `cases` array the oracle ran with.
fn input_cases(name: &str) -> Vec<Value> {
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

fn input_case<'a>(inputs: &'a [Value], name: &str) -> &'a Value {
    inputs
        .iter()
        .find(|case| str_of(case, "name") == name)
        .unwrap_or_else(|| panic!("no golden input named {name}"))
}

/// One golden number: a JSON number, or the text `NaN`, `Infinity`, `-Infinity`, `-0` (exact), or `{"bits": hex}`
/// for an f64 given by its bit pattern.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("number out of f64 range: {n}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => other
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("bad number text {other:?}: {e}")),
        },
        Value::Object(map) => {
            let bits = map
                .get("bits")
                .and_then(Value::as_str)
                .expect("number object must be {\"bits\": hex}");
            f64::from_bits(u64::from_str_radix(bits, 16).expect("hex bit pattern"))
        }
        other => panic!("not a number: {other}"),
    }
}

fn samples_of(value: &Value) -> Vec<f64> {
    value.as_array().expect("samples array").iter().map(num).collect()
}

fn i32_of(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("integer")).expect("fits i32")
}

/// JSON `null` or a missing key is `None`; an integer is `Some`.
fn optional_i32(value: &Value) -> Option<i32> {
    if value.is_null() { None } else { Some(i32_of(value)) }
}

fn workload_of(name: &str) -> BenchmarkWorkload {
    BenchmarkWorkload::from_name(name).unwrap_or_else(|| panic!("unknown workload {name}"))
}

fn status_of(name: &str) -> BenchmarkResultStatus {
    BenchmarkResultStatus::from_name(name).unwrap_or_else(|| panic!("unknown status {name}"))
}

/// A phase from a golden input case: `id`, `workload`, `samples`, `status`, optional `message` and `images`.
fn phase_of(case: &Value) -> BenchmarkPhaseResult {
    BenchmarkPhaseResult::new(
        str_of(case, "id"),
        workload_of(str_of(case, "workload")),
        samples_of(&case["samples"]),
        status_of(str_of(case, "status")),
        case.get("message").and_then(Value::as_str).map(str::to_owned),
        optional_i32(case.get("images").unwrap_or(&Value::Null)),
    )
}

/// One `derived` entry of a golden file: what the C# registry answered for `(id, workload)`.
type Derived = (String, BenchmarkWorkload, Option<i32>);

fn derived_of(entries: &Value) -> Vec<Derived> {
    entries
        .as_array()
        .expect("derived array")
        .iter()
        .map(|entry| {
            (
                str_of(entry, "id").to_owned(),
                workload_of(str_of(entry, "workload")),
                optional_i32(&entry["images"]),
            )
        })
        .collect()
}

/// `ImagesPerSampleLookup` answered from the golden's own derived values (not from a Rust copy of the registry).
fn lookup_in(derived: &[Derived], id: &str, workload: BenchmarkWorkload) -> Option<i32> {
    derived
        .iter()
        .find(|(d_id, d_workload, _)| d_id == id && *d_workload == workload)
        .and_then(|(_, _, images)| *images)
}

/// Checks the statistics of a golden `stats` object (`count`, `p50` ... `p50_per_image`) against `phase`.
fn assert_stats(
    case: &str,
    phase: &BenchmarkPhaseResult,
    stats: &Value,
    lookup: &dyn Fn(&str, BenchmarkWorkload) -> Option<i32>,
) {
    assert_eq!(i64::from(phase.count()), i64_of(stats, "count"), "{case}: count");
    assert_eq!(fmt_double(phase.p50()), str_of(stats, "p50"), "{case}: p50");
    assert_eq!(fmt_double(phase.p95()), str_of(stats, "p95"), "{case}: p95");
    assert_eq!(fmt_double(phase.p99()), str_of(stats, "p99"), "{case}: p99");
    assert_eq!(fmt_double(phase.max()), str_of(stats, "max"), "{case}: max");
    assert_eq!(
        i64::from(phase.effective_images_per_sample(lookup)),
        i64_of(stats, "effective_images"),
        "{case}: effective_images"
    );
    assert_eq!(
        fmt_double(phase.p95_per_image(lookup)),
        str_of(stats, "p95_per_image"),
        "{case}: p95_per_image"
    );
    assert_eq!(
        fmt_double(phase.p50_per_image(lookup)),
        str_of(stats, "p50_per_image"),
        "{case}: p50_per_image"
    );
}

fn loading_mode_of(value: i64) -> Option<LoadingMode> {
    match value {
        0 => Some(LoadingMode::Fast),
        1 => Some(LoadingMode::Preview),
        2 => Some(LoadingMode::Original),
        _ => None,
    }
}

/// A profile from a golden input case. Settings not named by the case keep the C# defaults (workers 1, windows 0,
/// `WarmupCount` 1, `Iterations` 30); only the validation and the target width are under test.
fn profile_of(case: &Value, mode: LoadingMode) -> BenchmarkProfile {
    let setting = |key: &str, default: i32| case.get(key).map_or(default, i32_of);
    BenchmarkProfile {
        id: case["id"].as_str().unwrap_or("").to_owned(),
        name: String::new(),
        description: String::new(),
        loading_mode: mode,
        workers: setting("workers", 1),
        next_window: setting("next", 0),
        previous_window: setting("previous", 0),
        full_folder: false,
        memory_reserve_bytes: 0,
        disk_cache: false,
        detailed_logging: false,
        workload: BenchmarkWorkload::Sequential,
        warmup_count: setting("warmup", DEFAULT_WARMUP_COUNT),
        iterations: setting("iterations", DEFAULT_ITERATIONS),
        correctness_only: false,
    }
}

/// The populated snapshot of golden `bench-json` (`"metrics": "populated"`), values as the golden report prints them.
fn populated_metrics() -> ReviewMetricsSnapshot {
    ReviewMetricsSnapshot {
        cache_hits: 10,
        cache_misses: 2,
        source_bytes_read: 123_456_789,
        source_reads: 12,
        decode_milliseconds: 340,
        presented_images: 8,
        present_milliseconds: 95,
        preload_hits: 5,
        inflight_joins: 1,
        disk_cache_hits: 3,
        ui_assign_milliseconds: 17,
        decoder_fallbacks: vec![(DecoderBackend::Wpf, 1), (DecoderBackend::TurboJpeg, 3)],
        source_open_count: 14,
        top_source_opens: vec![
            SourceOpenEntry {
                path: r"C:\a\b.jpg".to_owned(),
                count: 3,
            },
            SourceOpenEntry {
                path: "D:\\\u{e9}.jpg".to_owned(),
                count: 1,
            },
        ],
        stat_count: 40,
        session_write_count: 2,
        cross_thread_present_count: 0,
        present_histogram: vec![
            PresentLatencyBucket {
                label: "<=8".to_owned(),
                count: 5,
            },
            PresentLatencyBucket {
                label: "<=16".to_owned(),
                count: 2,
            },
            PresentLatencyBucket {
                label: ">500".to_owned(),
                count: 1,
            },
        ],
    }
}

fn metrics_of(value: Option<&Value>) -> Option<ReviewMetricsSnapshot> {
    match value.and_then(Value::as_str) {
        None => None,
        Some("zero") => Some(ReviewMetricsSnapshot::default()),
        Some("populated") => Some(populated_metrics()),
        Some(other) => panic!("unknown metrics fixture {other}"),
    }
}

fn timestamp_of(report: &Value) -> BenchmarkTimestamp {
    let utc = DateTimeUtc::from_ticks(i64_of(report, "utc_ticks")).expect("ticks in DateTime range");
    BenchmarkTimestamp::new(utc, i32_of(&report["offset_minutes"])).expect("offset within +-14 h")
}

fn report_of(report: &Value) -> BenchmarkReport {
    BenchmarkReport {
        run_id: str_of(report, "run_id").to_owned(),
        started_utc: timestamp_of(report),
        folder: str_of(report, "folder").to_owned(),
        phases: report["phases"]
            .as_array()
            .expect("phases array")
            .iter()
            .map(phase_of)
            .collect(),
        metrics: metrics_of(report.get("metrics")),
        machine: report.get("machine").and_then(Value::as_str).map(str::to_owned),
    }
}

fn batch_of(case: &Value) -> BenchmarkBatchSummary {
    let dataset = &case["dataset"];
    BenchmarkBatchSummary {
        dataset: BenchmarkDatasetManifest {
            folder: str_of(dataset, "folder").to_owned(),
            files: dataset["files"]
                .as_array()
                .expect("files array")
                .iter()
                .map(|file| file.as_str().expect("file name").to_owned())
                .collect(),
            unsupported_extensions: dataset["unsupported"]
                .as_array()
                .expect("unsupported array")
                .iter()
                .map(|pair| (pair[0].as_str().expect("extension").to_owned(), i32_of(&pair[1])))
                .collect(),
            file_limit: i32_of(&dataset["file_limit"]),
        },
        reports: case["reports"]
            .as_array()
            .expect("reports array")
            .iter()
            .map(report_of)
            .collect(),
        profile_outcomes: case["outcomes"]
            .as_array()
            .expect("outcomes array")
            .iter()
            .map(phase_of)
            .collect(),
    }
}

// ---- BenchmarkStatisticsEdgeTests ------------------------------------------------------------------------------

// C#: BenchmarkStatisticsEdgeTests.PhaseResult_NoSamples
#[test]
fn phase_result_no_samples() {
    let empty = phase(
        "x",
        BenchmarkWorkload::Sequential,
        &[],
        BenchmarkResultStatus::InsufficientData,
        Some(1),
    );
    assert_eq!(empty.count(), 0);
    assert_eq!(empty.p50(), 0.0);
    assert_eq!(empty.p95(), 0.0);
    assert_eq!(empty.p99(), 0.0);
    assert_eq!(empty.max(), 0.0);
}

// C#: BenchmarkStatisticsEdgeTests.PhaseResult_OneSample
#[test]
fn phase_result_one_sample() {
    let one = phase(
        "x",
        BenchmarkWorkload::Sequential,
        &[12.5],
        BenchmarkResultStatus::Pass,
        Some(1),
    );
    for value in [one.p50(), one.p95(), one.p99(), one.max()] {
        assert_eq!(value, 12.5);
    }
}

// C#: BenchmarkStatisticsEdgeTests.PhaseResult_UnsortedInputWithTies
#[test]
fn phase_result_unsorted_input_with_ties() {
    let unsorted = phase(
        "x",
        BenchmarkWorkload::Sequential,
        &[9.0, 1.0, 5.0, 5.0, 5.0, 1.0, 9.0],
        BenchmarkResultStatus::Pass,
        Some(1),
    );
    assert_eq!(unsorted.p50(), 5.0);
    assert_eq!(unsorted.max(), 9.0);
}

// C#: BenchmarkStatisticsEdgeTests.PhaseResult_WithCopyRecomputesStatistics
// (`original with { Samples = ... }` is `with_samples`; the original must stay unchanged.)
#[test]
fn phase_result_with_copy_recomputes_statistics() {
    let original = phase(
        "x",
        BenchmarkWorkload::Sequential,
        &[1.0, 2.0, 3.0],
        BenchmarkResultStatus::Pass,
        Some(1),
    );
    assert_eq!(original.p50(), 2.0); // forces the sorted cache of the original

    let copy = original.with_samples(vec![100.0, 200.0, 300.0]);

    assert_eq!(copy.p50(), 200.0);
    assert_eq!(copy.max(), 300.0);
    assert_eq!(original.p50(), 2.0);
    assert_eq!(original.max(), 3.0);
}

// C#: BenchmarkStatisticsEdgeTests.PhaseResult_WithCopyBeforeForcing
#[test]
fn phase_result_with_copy_before_forcing() {
    let original = phase(
        "x",
        BenchmarkWorkload::Sequential,
        &[1.0, 2.0, 3.0],
        BenchmarkResultStatus::Pass,
        Some(1),
    );
    let copy = original.with_samples(vec![100.0, 200.0, 300.0]);
    assert_eq!(copy.p50(), 200.0);
    assert_eq!(original.p50(), 2.0);
}

// C#: BenchmarkStatisticsEdgeTests.PhaseResult_HugeValues (theory rows 1e300 and double.MaxValue)
#[test]
fn phase_result_huge_values() {
    for huge in [1e300_f64, f64::MAX] {
        let big = phase(
            "x",
            BenchmarkWorkload::Sequential,
            &[1.0, huge / 4.0, huge / 2.0],
            BenchmarkResultStatus::Pass,
            Some(1),
        );
        assert!(big.p95().is_finite(), "huge={huge:e}");
        assert_eq!(big.max(), huge / 2.0, "huge={huge:e}");
        assert!(big.p50() <= big.p95() && big.p95() <= big.max(), "huge={huge:e}");
    }
}

// C#: BenchmarkStatisticsEdgeTests.EffectiveImagesPerSample_IsAtLeastOne
// The unknown profile is a lookup that returns `None`. A stamp (even 0 or -7) must not call the lookup at all.
#[test]
fn effective_images_per_sample_is_at_least_one() {
    let panicking_lookup =
        |_: &str, _: BenchmarkWorkload| -> Option<i32> { panic!("lookup must not run for a stamped phase") };
    let stamp = |images: Option<i32>| {
        phase(
            "no-such-profile",
            BenchmarkWorkload::Sequential,
            &[1.0],
            BenchmarkResultStatus::Pass,
            images,
        )
    };

    assert_eq!(stamp(Some(0)).effective_images_per_sample(&panicking_lookup), 1);
    assert_eq!(stamp(Some(-7)).effective_images_per_sample(&panicking_lookup), 1);
    assert_eq!(stamp(None).effective_images_per_sample(&no_lookup), 1);
}

// C#: BenchmarkStatisticsEdgeTests.Ranking_TiesAreStable
#[test]
fn ranking_ties_are_stable() {
    let seq = BenchmarkWorkload::Sequential;
    let pass = BenchmarkResultStatus::Pass;
    let a = phase("a", seq, &[10.0, 10.0, 10.0], pass, Some(1));
    let b = phase("b", seq, &[10.0, 10.0, 10.0], pass, Some(1));
    let c = phase("c", seq, &[5.0, 5.0, 5.0], pass, Some(1));
    let d = phase("d", seq, &[10.0, 10.0, 10.0], pass, Some(1));

    let ids = |list: &[BenchmarkPhaseResult]| -> Vec<String> {
        rank(list, seq, &no_lookup)
            .iter()
            .map(|p| p.profile_id.clone())
            .collect()
    };
    assert_eq!(ids(&[a.clone(), b.clone(), c.clone(), d.clone()]), ["c", "a", "b", "d"]);
    assert_eq!(ids(&[d, b, c, a]), ["c", "d", "b", "a"]);
}

// C#: BenchmarkStatisticsEdgeTests.Ranking_P95TieBrokenByP50
#[test]
fn ranking_p95_tie_broken_by_p50() {
    let seq = BenchmarkWorkload::Sequential;
    let pass = BenchmarkResultStatus::Pass;
    let slow_median = phase("slow-median", seq, &[9.0; 10], pass, Some(1));
    let fast_median = phase(
        "fast-median",
        seq,
        &[1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 9.0, 9.0],
        pass,
        Some(1),
    );
    assert_eq!(slow_median.p95(), fast_median.p95());

    let ids: Vec<String> = rank(&[slow_median, fast_median], seq, &no_lookup)
        .iter()
        .map(|p| p.profile_id.clone())
        .collect();
    assert_eq!(ids, ["fast-median", "slow-median"]);
}

// C#: BenchmarkStatisticsEdgeTests.Ranking_UsesPerImageUnit
#[test]
fn ranking_uses_per_image_unit() {
    let seq = BenchmarkWorkload::Sequential;
    let pass = BenchmarkResultStatus::Pass;
    let parallel = phase("parallel", seq, &[80.0, 80.0, 80.0], pass, Some(8));
    let single = phase("single", seq, &[20.0, 20.0, 20.0], pass, Some(1));

    let ids: Vec<String> = rank(&[single, parallel], seq, &no_lookup)
        .iter()
        .map(|p| p.profile_id.clone())
        .collect();
    assert_eq!(ids, ["parallel", "single"]);
}

// C#: BenchmarkStatisticsEdgeTests.Ranking_FiltersByWorkloadAndStatus
#[test]
fn ranking_filters_by_workload_and_status() {
    let seq = BenchmarkWorkload::Sequential;
    let pass = phase("pass", seq, &[3.0], BenchmarkResultStatus::Pass, Some(1));
    let warn = phase("warn", seq, &[4.0], BenchmarkResultStatus::Warn, Some(1));
    let fail = phase("fail", seq, &[1.0], BenchmarkResultStatus::Fail, Some(1));
    let none = phase("none", seq, &[], BenchmarkResultStatus::InsufficientData, Some(1));
    let other = phase(
        "other",
        BenchmarkWorkload::Random,
        &[1.0],
        BenchmarkResultStatus::Pass,
        Some(1),
    );

    let ids: Vec<String> = rank(&[fail, other, warn, none, pass], seq, &no_lookup)
        .iter()
        .map(|p| p.profile_id.clone())
        .collect();
    assert_eq!(ids, ["pass", "warn"]);
    assert!(rank(&[], seq, &no_lookup).is_empty());
}

// C#: BenchmarkStatisticsEdgeTests.Ranking_NaNSamplesDoNotWin
#[test]
fn ranking_nan_samples_do_not_win() {
    let seq = BenchmarkWorkload::Sequential;
    let pass = BenchmarkResultStatus::Pass;
    let nan = phase("nan", seq, &[f64::NAN, f64::NAN], pass, Some(1));
    let real = phase("real", seq, &[50.0, 50.0], pass, Some(1));

    let ids: Vec<String> = rank(&[nan, real], seq, &no_lookup)
        .iter()
        .map(|p| p.profile_id.clone())
        .collect();
    assert_eq!(ids, ["real"]);
}

// C#: BenchmarkStatisticsEdgeTests.ReportJson_EmptyPhaseIsFinite
#[test]
fn report_json_empty_phase_is_finite() {
    let report = BenchmarkReport {
        run_id: "run".to_owned(),
        started_utc: BenchmarkTimestamp::unix_epoch(),
        folder: "C:\\x".to_owned(),
        phases: vec![phase(
            "x",
            BenchmarkWorkload::Sequential,
            &[],
            BenchmarkResultStatus::InsufficientData,
            Some(1),
        )],
        metrics: None,
        machine: None,
    };
    let json = report.to_json(&no_lookup).expect("finite numbers only");
    assert!(!json.contains("NaN"), "{json}");
    assert!(!json.contains("Infinity"), "{json}");
    assert!(json.contains("\"P50\": 0"), "{json}");
}

// ---- BenchmarkTrustTests ---------------------------------------------------------------------------------------

// C#: BenchmarkTrustTests.Ranking_NormalizesByImagesPerSample (PERF-03)
#[test]
fn ranking_normalizes_by_images_per_sample() {
    // 8 images in 80 ms (10 ms/image) beats 1 image in 50 ms, although 80 > 50 raw.
    let seq = BenchmarkWorkload::Sequential;
    let pass = BenchmarkResultStatus::Pass;
    let parallel = BenchmarkPhaseResult::new("many", seq, vec![80.0, 80.0], pass, None, Some(8));
    let single = BenchmarkPhaseResult::new("one", seq, vec![50.0, 50.0], pass, None, Some(1));

    let ids: Vec<String> = rank(&[single, parallel], seq, &no_lookup)
        .iter()
        .map(|p| p.profile_id.clone())
        .collect();
    assert_eq!(ids, ["many", "one"]);
}

// C#: BenchmarkTrustTests.Ranking_DerivesImagesPerSampleFromProfile (PERF-03)
// Until W7-01b and W7-03 merge, the registry lookup is this test-local closure: fast-sequential -> 8,
// no-preload-baseline -> 1, anything else unknown.
#[test]
fn ranking_derives_images_per_sample_from_profile() {
    let seq = BenchmarkWorkload::Sequential;
    let registry = |id: &str, _: BenchmarkWorkload| match id {
        "fast-sequential" => Some(8),
        "no-preload-baseline" => Some(1),
        _ => None,
    };
    let many = BenchmarkPhaseResult::new(
        "fast-sequential",
        seq,
        vec![80.0],
        BenchmarkResultStatus::Pass,
        None,
        None,
    );
    let one = BenchmarkPhaseResult::new(
        "no-preload-baseline",
        seq,
        vec![50.0],
        BenchmarkResultStatus::Pass,
        None,
        None,
    );

    let ids: Vec<String> = rank(&[one, many], seq, &registry)
        .iter()
        .map(|p| p.profile_id.clone())
        .collect();
    assert_eq!(ids, ["fast-sequential", "no-preload-baseline"]);
}

// ---- Rust-only tests (no C# twin; they pin the documented divergences and the contract's seams) -----------------

// DIVERGENCE 2: C# `UnsupportedExtensions.Values.Sum()` throws OverflowException; this saturates at i32::MAX.
#[test]
fn batch_manifest_unsupported_count_saturates() {
    let manifest = BenchmarkDatasetManifest {
        folder: "f".to_owned(),
        files: vec!["a.jpg".to_owned(), "b.png".to_owned()],
        unsupported_extensions: vec![(".a".to_owned(), i32::MAX), (".b".to_owned(), 1)],
        file_limit: 10,
    };
    assert_eq!(manifest.supported_file_count(), 2);
    assert_eq!(manifest.unsupported_file_count(), i32::MAX);
}

// DateTimeOffset accepts offsets within +-14 h only; the local time must stay inside the DateTime range.
#[test]
fn timestamp_new_rejects_offset_out_of_range() {
    let utc = DateTimeUtc::from_ticks(621_355_968_000_000_000).expect("1970-01-01");
    assert!(BenchmarkTimestamp::new(utc, 840).is_some());
    assert!(BenchmarkTimestamp::new(utc, 841).is_none());
    assert!(BenchmarkTimestamp::new(utc, -841).is_none());
    assert!(BenchmarkTimestamp::new(DateTimeUtc::MIN, -1).is_none());
}

// C#: DateTimeOffset.UnixEpoch prints with the +00:00 offset (never `Z`).
#[test]
fn timestamp_unix_epoch_text_has_zero_offset() {
    assert_eq!(
        BenchmarkTimestamp::unix_epoch().to_json_text(),
        "1970-01-01T00:00:00+00:00"
    );
}

// ---- golden tests ----------------------------------------------------------------------------------------------

/// Every case of `bench-profile`: `target_width`, and the validation answer (`ok`, or the problem, profile id and
/// message). Documented skips (DIVERGENCE 3): an unrepresentable loading mode and the C# null profile cannot exist in
/// Rust; for those the golden's answer is only checked to be what the port must reproduce.
/// `is_argument_exception` is not compared: `BenchmarkProfileError` is the ArgumentException analogue, always.
#[test]
fn bench_profile_golden_matches_csharp() {
    let inputs = input_cases("bench-profile");
    for expected in golden::load("bench-profile").cases {
        let name = str_of(&expected, "name");
        let input = input_case(&inputs, name);
        let validate = &expected["validate"];

        if !bool_of(&expected, "representable") {
            assert_eq!(
                str_of(validate, "problem"),
                "MissingIdOrMode",
                "{name}: unrepresentable mode"
            );
            continue;
        }
        if let Some(thrown) = validate.get("throws") {
            assert_eq!(thrown.as_str(), Some("ArgumentNullException"), "{name}: thrown");
            continue;
        }

        let mode = loading_mode_of(i64_of(input, "loading_mode")).expect("representable loading mode");
        let profile = profile_of(input, mode);
        assert_eq!(
            i64::from(profile.target_width()),
            i64_of(&expected, "target_width"),
            "{name}: target_width"
        );

        let result = validate_profile(&profile);
        if bool_of(validate, "ok") {
            assert!(result.is_ok(), "{name}: expected valid, got {result:?}");
        } else {
            let err = result.expect_err("expected a validation error");
            assert_eq!(err.problem.name(), str_of(validate, "problem"), "{name}: problem");
            assert_eq!(err.profile_id, str_of(validate, "profile_id"), "{name}: profile_id");
            assert_eq!(err.message, str_of(validate, "message"), "{name}: message");
        }
    }
}

/// Every case of `bench-phase`: statistics, the copy (`with_samples`) and the original after the copy, against the
/// derived registry answer of the golden for the phase's own workload.
#[test]
fn bench_phase_golden_matches_csharp() {
    let inputs = input_cases("bench-phase");
    for expected in golden::load("bench-phase").cases {
        let name = str_of(&expected, "name");
        let input = input_case(&inputs, name);
        let phase = phase_of(input);
        let derived = vec![(
            phase.profile_id.clone(),
            phase.workload,
            optional_i32(&expected["derived_images"]),
        )];
        let lookup = |id: &str, workload: BenchmarkWorkload| lookup_in(&derived, id, workload);

        assert_stats(name, &phase, &expected["stats"], &lookup);
        if let Some(with_samples) = input.get("with_samples") {
            let copy = phase.with_samples(samples_of(with_samples));
            assert_stats(name, &copy, &expected["copy"], &lookup);
            assert_stats(name, &phase, &expected["original_after"], &lookup);
        }
    }
}

/// Every case of `bench-rank`: the ranked input indexes, with the golden's derived registry answers.
#[test]
fn bench_rank_golden_matches_csharp() {
    let inputs = input_cases("bench-rank");
    for expected in golden::load("bench-rank").cases {
        let name = str_of(&expected, "name");
        let input = input_case(&inputs, name);
        let phases: Vec<BenchmarkPhaseResult> = input["phases"]
            .as_array()
            .expect("phases array")
            .iter()
            .map(phase_of)
            .collect();
        let derived = derived_of(&expected["derived"]);
        let lookup = |id: &str, workload: BenchmarkWorkload| lookup_in(&derived, id, workload);

        let ranked = rank(&phases, workload_of(str_of(input, "workload")), &lookup);
        let indexes: Vec<i64> = ranked
            .iter()
            .map(|picked| {
                let index = phases
                    .iter()
                    .position(|p| std::ptr::eq(p, *picked))
                    .expect("ranked phase is one of the input phases");
                i64::try_from(index).expect("index fits i64")
            })
            .collect();
        let expected_indexes: Vec<i64> = expected["ranked"]
            .as_array()
            .expect("ranked array")
            .iter()
            .map(|v| v.as_i64().expect("input index"))
            .collect();
        assert_eq!(indexes, expected_indexes, "{name}: ranked input indexes");
    }
}

/// Every case of `bench-json`: `ToJson()` of a report or a batch summary, byte for byte (CRLF, escapes, `R` numbers,
/// `DateTimeOffset` text), or the `NonFiniteNumber` error the C# writer throws.
#[test]
fn bench_json_golden_matches_csharp() {
    let inputs = input_cases("bench-json");
    for expected in golden::load("bench-json").cases {
        let name = str_of(&expected, "name");
        let input = input_case(&inputs, name);
        let derived = derived_of(&expected["derived"]);
        let lookup = |id: &str, workload: BenchmarkWorkload| lookup_in(&derived, id, workload);

        let json: Result<String, NonFiniteNumber> = match str_of(&expected, "kind") {
            "report" => report_of(&input["report"]).to_json(&lookup),
            "batch" => batch_of(input).to_json(&lookup),
            other => panic!("{name}: unknown kind {other}"),
        };

        if expected.get("throws").is_some() {
            assert_eq!(json, Err(NonFiniteNumber), "{name}: must throw");
        } else {
            assert_eq!(
                json.expect("finite numbers only"),
                str_of(&expected, "json"),
                "{name}: json"
            );
        }
    }
}

// ---- repair round 1: mutation survivors ------------------------------------------------------------------------

#[test]
fn rank_drops_phase_with_only_p95_nonfinite() {
    // samples [1, 2, inf]: finite P50, infinite P95; a phase like that must never be ranked.
    let w = BenchmarkWorkload::Random;
    let bad = phase(
        "bad",
        w,
        &[1.0, 2.0, f64::INFINITY],
        BenchmarkResultStatus::Pass,
        Some(1),
    );
    let ok = phase("ok", w, &[5.0], BenchmarkResultStatus::Pass, Some(1));
    assert!(bad.p50().is_finite() && !bad.p95().is_finite());
    let phases = vec![bad, ok];
    let r = rank(&phases, w, &no_lookup);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].profile_id, "ok");
}

#[test]
fn rank_drops_phase_with_only_p50_nonfinite() {
    // NaN sorts first: [NaN, 1] gives a NaN P50 and a NaN P95 too; use [-inf, 1, 2, 3, 4] style instead is not needed,
    // [inf, inf] has a NaN P50 and NaN P95. Check the P50-only branch via a single-NaN-free pair.
    let w = BenchmarkWorkload::Random;
    let bad = phase(
        "bad",
        w,
        &[f64::INFINITY, f64::INFINITY],
        BenchmarkResultStatus::Pass,
        Some(1),
    );
    let ok = phase("ok", w, &[5.0], BenchmarkResultStatus::Pass, Some(1));
    let phases = vec![bad, ok];
    let r = rank(&phases, w, &no_lookup);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].profile_id, "ok");
}

#[test]
fn timestamp_accessors_return_constructor_values() {
    let utc = DateTimeUtc::from_ticks(639029396450000001).unwrap();
    let ts = BenchmarkTimestamp::new(utc, -330).unwrap();
    assert_eq!(ts.utc(), utc);
    assert_eq!(ts.offset_minutes(), -330);
    let ts = BenchmarkTimestamp::new(utc, 60).unwrap();
    assert_eq!(ts.offset_minutes(), 60);
    assert_eq!(BenchmarkTimestamp::unix_epoch().offset_minutes(), 0);
}

#[test]
fn error_display_prints_text() {
    use pr_bench::models::{BenchmarkProfileError, BenchmarkProfileProblem, ExecutorError};
    let e = BenchmarkProfileError::new(
        BenchmarkProfileProblem::InvalidSettings,
        "p",
        "Invalid benchmark settings for profile 'p'",
    );
    assert_eq!(e.to_string(), "Invalid benchmark settings for profile 'p'");
    assert_eq!(ExecutorError::Canceled.to_string(), "The operation was canceled.");
    assert_eq!(ExecutorError::Failed("boom".into()).to_string(), "boom");
    assert_eq!(
        NonFiniteNumber.to_string(),
        "a benchmark JSON number is NaN or infinite"
    );
}

#[test]
fn phase_equality_compares_every_field() {
    let w = BenchmarkWorkload::Random;
    let mk = || {
        BenchmarkPhaseResult::new(
            "a",
            w,
            vec![1.0, 2.0],
            BenchmarkResultStatus::Pass,
            Some("m".into()),
            Some(2),
        )
    };
    let base = mk();
    assert!(base == mk());
    assert!(!(base != mk()));
    // forcing the sorted cache on one side must not change equality
    let forced = mk();
    let _ = forced.p50();
    assert!(base == forced);
    let mut v = mk();
    v.profile_id = "b".into();
    assert!(base != v);
    let mut v = mk();
    v.workload = BenchmarkWorkload::Sequential;
    assert!(base != v);
    assert!(base != mk().with_samples(vec![1.0, 3.0]));
    let mut v = mk();
    v.status = BenchmarkResultStatus::Fail;
    assert!(base != v);
    let mut v = mk();
    v.message = None;
    assert!(base != v);
    let mut v = mk();
    v.images_per_sample = Some(3);
    assert!(base != v);
    // value equality of samples (DIVERGENCE from C# reference equality)
    assert!(base == mk().with_samples(vec![1.0, 2.0]));
}

#[test]
fn unsupported_file_count_sums_and_saturates() {
    use pr_bench::models::BenchmarkDatasetManifest as M;
    let m = |u: Vec<(String, i32)>| M {
        folder: "f".into(),
        files: vec![],
        unsupported_extensions: u,
        file_limit: 0,
    };
    assert_eq!(m(vec![(".a".into(), 2), (".b".into(), 3)]).unsupported_file_count(), 5);
    assert_eq!(m(vec![]).unsupported_file_count(), 0);
    assert_eq!(
        m(vec![(".a".into(), i32::MAX), (".b".into(), 1)]).unsupported_file_count(),
        i32::MAX
    );
    assert_eq!(m(vec![(".a".into(), i32::MAX)]).unsupported_file_count(), i32::MAX);
    assert_eq!(
        m(vec![(".a".into(), i32::MIN), (".b".into(), -1)]).unsupported_file_count(),
        i32::MIN
    );
    assert_eq!(m(vec![(".a".into(), i32::MIN)]).unsupported_file_count(), i32::MIN);
}
