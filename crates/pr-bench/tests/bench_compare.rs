//! Tests for `pr_bench::compare` (unit W10-B1, `contracts/W10-B1.md`, C# baseline d92d6aae). The contract's tests are
//! new pure tests (C# has no counterpart for the comparison gate; C# `PerfComparer` only supplies the 5 percent / 1 ms /
//! 3 samples vocabulary), so each comment says `new:`. Everything runs on synthetic summaries: no timing here is a
//! measurement and nothing reads a photo library.

use pr_bench::compare::{
    CompareReport, DatasetIdentity, DiagnosticMetric, PhaseRow, Summary, Thresholds, Verdict, compare, compare_repeats,
    parse_summary,
};

fn dataset() -> DatasetIdentity {
    DatasetIdentity {
        folder: "D:\\fixture".to_owned(),
        files: vec!["a.jpg".to_owned(), "b.jpg".to_owned()],
        unsupported_extensions: vec![(".txt".to_owned(), 2)],
        file_limit: 64,
    }
}

fn row(id: &str, p95: f64) -> PhaseRow {
    PhaseRow {
        profile_id: id.to_owned(),
        workload: 1,
        status: 0,
        samples_ms: vec![p95 / 2.0, p95 / 2.0, p95],
        p50_ms: p95 / 2.0,
        p95_ms: p95,
        max_ms: p95 + 1.0,
        correctness_only: false,
        metrics: Vec::new(),
    }
}

fn summary(rows: Vec<PhaseRow>) -> Summary {
    Summary {
        rows,
        dataset: dataset(),
    }
}

fn single() -> Thresholds {
    Thresholds {
        minimum_repeats: 1,
        ..Thresholds::default()
    }
}

fn one(c: Vec<PhaseRow>, r: Vec<PhaseRow>) -> CompareReport {
    compare(&summary(c), &summary(r), &single()).expect("compare")
}

fn p95_verdict(c: f64, r: f64) -> Verdict {
    one(vec![row("preview-light", c)], vec![row("preview-light", r)]).profiles[0].verdict
}

const SUMMARY_JSON: &str = "{\r\n  \"Dataset\": {\r\n    \"Folder\": \"D:\\\\fixture\",\r\n    \"Files\": [\"a.jpg\"],\r\n    \"UnsupportedExtensions\": {\".txt\": 2},\r\n    \"FileLimit\": 64,\r\n    \"SupportedFileCount\": 1,\r\n    \"UnsupportedFileCount\": 2\r\n  },\r\n  \"Reports\": [\r\n    {\"RunId\": \"x\", \"Phases\": [], \"Metrics\": {\"CacheHits\": 3, \"PresentHistogram\": [{\"Label\": \"<8\", \"Count\": 1}]}, \"Machine\": null}\r\n  ],\r\n  \"ProfileOutcomes\": [\r\n    {\"ProfileId\": \"preview-light\", \"Workload\": 1, \"Samples\": [2.0, 4.0], \"Status\": 0, \"Message\": null, \"Count\": 2, \"P50\": 3.0, \"P95\": 4.0, \"Max\": 4.0}\r\n  ]\r\n}";

// new: compare_noise_floor_accepts_submillisecond_drift
#[test]
fn compare_noise_floor_accepts_submillisecond_drift() {
    // 0.9 ms slower on a 2 ms base: 5 percent is 0.1 ms, the 1 ms floor wins -> WithinNoise, not Regression.
    assert_eq!(p95_verdict(2.0, 2.9), Verdict::WithinNoise);
    assert_eq!(p95_verdict(2.0, 3.0), Verdict::WithinNoise); // exactly the allowed delta
    assert_eq!(p95_verdict(2.0, 3.01), Verdict::Regression);
    assert_eq!(p95_verdict(2.0, 2.0), Verdict::Pass);
    assert_eq!(p95_verdict(2.0, 1.5), Verdict::Pass);
    assert_eq!(p95_verdict(2.0, 0.99), Verdict::Improvement);
    // Large base: the 5 percent ratio (10 ms of 200) beats the floor.
    assert_eq!(p95_verdict(200.0, 209.0), Verdict::WithinNoise);
    assert_eq!(p95_verdict(200.0, 211.0), Verdict::Regression);
}

// new: compare_nonfinite_or_null_percentile_is_incomparable
#[test]
fn compare_nonfinite_or_null_percentile_is_incomparable() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut r = row("preview-light", 5.0);
        r.p95_ms = bad;
        let report = one(vec![row("preview-light", 5.0)], vec![r]);
        assert_eq!(report.profiles[0].verdict, Verdict::Incomparable);
        assert_eq!(report.verdict, Verdict::Incomparable);
        assert_eq!(report.exit_code(), 3);
    }
    // A null P95 in JSON is parsed as NaN, never as zero.
    let text = SUMMARY_JSON.replace("\"P95\": 4.0", "\"P95\": null");
    let parsed = parse_summary(&text).unwrap();
    assert!(parsed.rows[0].p95_ms.is_nan());
}

// new: compare_empty_phases_are_incomparable
#[test]
fn compare_empty_phases_are_incomparable() {
    let report = one(vec![], vec![]);
    assert_eq!(report.verdict, Verdict::Incomparable);
    let mut c = row("preview-light", 5.0);
    let mut r = row("preview-light", 5.0);
    c.samples_ms.clear();
    r.samples_ms.clear();
    let report = one(vec![c], vec![r]);
    assert_eq!(report.profiles[0].verdict, Verdict::Incomparable);
}

// new: compare_status_mismatch_is_incomparable
#[test]
fn compare_status_mismatch_is_incomparable() {
    let mut r = row("preview-light", 5.0);
    r.status = 1;
    let report = one(vec![row("preview-light", 5.0)], vec![r]);
    assert_eq!(report.profiles[0].verdict, Verdict::Incomparable);
    // Fail on both sides is not a speed result either.
    let (mut c, mut r) = (row("preview-light", 5.0), row("preview-light", 5.0));
    c.status = 2;
    r.status = 2;
    assert_eq!(one(vec![c], vec![r]).verdict, Verdict::Incomparable);
}

// new: compare_extra_or_missing_profile_is_incomparable
#[test]
fn compare_extra_or_missing_profile_is_incomparable() {
    let report = one(
        vec![row("preview-light", 5.0), row("preview-balanced", 5.0)],
        vec![row("preview-light", 5.0)],
    );
    assert_eq!(report.verdict, Verdict::Incomparable);
    assert_eq!(report.profiles.len(), 2);
    let missing = report
        .profiles
        .iter()
        .find(|p| p.profile_id == "preview-balanced")
        .unwrap();
    assert_eq!(missing.verdict, Verdict::Incomparable);
    // Different order is a schema mismatch too.
    let report = one(
        vec![row("preview-light", 5.0), row("preview-balanced", 5.0)],
        vec![row("preview-balanced", 5.0), row("preview-light", 5.0)],
    );
    assert_eq!(report.verdict, Verdict::Incomparable);
}

// new: compare_crlf_and_bom_input_parses_equally
#[test]
fn compare_crlf_and_bom_input_parses_equally() {
    let plain = parse_summary(SUMMARY_JSON).unwrap();
    let bom = parse_summary(&format!("\u{FEFF}{SUMMARY_JSON}")).unwrap();
    let lf = parse_summary(&SUMMARY_JSON.replace("\r\n", "\n")).unwrap();
    assert_eq!(plain, bom);
    assert_eq!(plain, lf);
    assert_eq!(plain.rows[0].profile_id, "preview-light");
    assert_eq!(plain.rows[0].samples_ms, vec![2.0, 4.0]);
    assert_eq!(plain.dataset.unsupported_extensions, vec![(".txt".to_owned(), 2)]);
    assert_eq!(
        plain.rows[0].metrics,
        vec![
            DiagnosticMetric {
                name: "CacheHits".to_owned(),
                value: "3".to_owned()
            },
            DiagnosticMetric {
                name: "PresentHistogram".to_owned(),
                value: "[{\"Label\":\"<8\",\"Count\":1}]".to_owned()
            },
        ]
    );
    assert!(parse_summary("{\"Dataset\": ").is_err());
    assert!(parse_summary("{} trailing").is_err());
}

fn repeats(base: &[f64]) -> Vec<Summary> {
    base.iter().map(|p| summary(vec![row("preview-light", *p)])).collect()
}

// new: compare_repeats_uses_median_of_run_statistics
#[test]
fn compare_repeats_uses_median_of_run_statistics() {
    let t = Thresholds::default();
    // One Rust outlier run (50) does not move the median.
    let report = compare_repeats(&repeats(&[10.0, 10.0, 10.0]), &repeats(&[10.0, 50.0, 10.0]), &t).unwrap();
    assert_eq!(report.profiles[0].verdict, Verdict::Pass);
    assert_eq!(report.profiles[0].rust_ms[1], 10.0);
    assert_eq!(report.profiles[0].repeat_count, 3);
    // Median of an even count is the mean of the middle two.
    let t4 = Thresholds {
        minimum_repeats: 4,
        ..Thresholds::default()
    };
    let report = compare_repeats(&repeats(&[1.0, 2.0, 3.0, 4.0]), &repeats(&[1.0, 2.0, 3.0, 4.0]), &t4).unwrap();
    assert_eq!(report.profiles[0].csharp_ms[1], 2.5);
    // Regression: median worse than allowed and a strict majority of pairs worse.
    let report = compare_repeats(&repeats(&[10.0, 10.0, 10.0]), &repeats(&[20.0, 20.0, 5.0]), &t).unwrap();
    assert_eq!(report.profiles[0].verdict, Verdict::Regression);
    assert_eq!(report.exit_code(), 1);
}

// new: strict-majority rule (contract THRESHOLDS table, last two rows)
#[test]
fn compare_repeats_without_strict_majority_is_unstable_within_noise() {
    // B = 10 x4, R = 30, 30, 10, 10 -> median R = 20 (d = 10 > 1) but only 2 of 4 pairs are worse: no strict majority.
    let t = Thresholds {
        minimum_repeats: 4,
        ..Thresholds::default()
    };
    let report = compare_repeats(&repeats(&[10.0; 4]), &repeats(&[30.0, 30.0, 10.0, 10.0]), &t).unwrap();
    assert_eq!(report.profiles[0].verdict, Verdict::WithinNoise);
    assert!(report.notes.iter().any(|n| n.contains("unstable")));
    assert_eq!(report.exit_code(), 0);
}

// new: repeat protocol errors
#[test]
fn compare_repeats_rejects_too_few_or_unequal_repeats() {
    let t = Thresholds::default();
    assert!(compare_repeats(&repeats(&[1.0, 1.0]), &repeats(&[1.0, 1.0]), &t).is_err());
    assert!(compare_repeats(&repeats(&[1.0; 3]), &repeats(&[1.0; 4]), &t).is_err());
    // Single-pair compare needs minimum_repeats == 1.
    assert!(compare(&summary(vec![]), &summary(vec![]), &t).is_err());
    let bad = Thresholds {
        ratio_tolerance: f64::NAN,
        ..Thresholds::default()
    };
    assert!(compare_repeats(&repeats(&[1.0; 3]), &repeats(&[1.0; 3]), &bad).is_err());
}

// new: compare_correctness_only_ignores_latency
#[test]
fn compare_correctness_only_ignores_latency() {
    let mut c = row("original-correctness", 1.0);
    let mut r = row("original-correctness", 500.0);
    c.correctness_only = true;
    r.correctness_only = true;
    let report = one(vec![c.clone()], vec![r.clone()]);
    assert_eq!(report.profiles[0].verdict, Verdict::Pass);
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.exit_code(), 0);
    // Exact checks still apply: a different sample count is incomparable.
    r.samples_ms.push(1.0);
    assert_eq!(one(vec![c], vec![r]).verdict, Verdict::Incomparable);
}

// new: compare_not_implemented_profile_is_skipped_only_when_both_sides_match
#[test]
fn compare_not_implemented_profile_is_skipped_only_when_both_sides_match() {
    let mk = |p: f64| {
        let mut x = row("explorer-reindex", p);
        x.workload = 6;
        x
    };
    let report = one(
        vec![mk(1.0), row("preview-light", 5.0)],
        vec![mk(900.0), row("preview-light", 5.0)],
    );
    let skipped = report
        .profiles
        .iter()
        .find(|p| p.profile_id == "explorer-reindex")
        .unwrap();
    assert_eq!(skipped.verdict, Verdict::Skipped);
    assert_eq!(report.verdict, Verdict::Pass); // its timing cannot fail the gate
    // Only one side carries it: profile-set mismatch.
    let report = one(
        vec![mk(1.0), row("preview-light", 5.0)],
        vec![row("preview-light", 5.0)],
    );
    assert_eq!(report.verdict, Verdict::Incomparable);
}

// new: compare_sample_count_mismatch_is_incomparable
#[test]
fn compare_sample_count_mismatch_is_incomparable() {
    let mut r = row("preview-light", 5.0);
    r.samples_ms.pop();
    let report = one(vec![row("preview-light", 5.0)], vec![r]);
    assert_eq!(report.profiles[0].verdict, Verdict::Incomparable);
    assert_eq!(report.profiles[0].sample_count, 3);
    assert_eq!(report.profiles[0].sample_count_rust, 2);
}

// new: dataset identity is exact (Folder, ordered Files, UnsupportedExtensions, FileLimit)
#[test]
fn compare_dataset_mismatch_is_incomparable() {
    let c = summary(vec![row("preview-light", 5.0)]);
    let mutations: [fn(&mut DatasetIdentity); 4] = [
        |d| d.folder.push('x'),
        |d| d.files.reverse(),
        |d| d.unsupported_extensions[0].1 = 3,
        |d| d.file_limit = 32,
    ];
    for mutate in mutations {
        let mut r = c.clone();
        mutate(&mut r.dataset);
        let report = compare(&c, &r, &single()).unwrap();
        assert!(!report.dataset_equal);
        assert_eq!(report.verdict, Verdict::Incomparable);
    }
    assert!(compare(&c, &c, &single()).unwrap().dataset_equal);
}

// new: diagnostic metrics are reported with a delta and never fail the gate
#[test]
fn compare_diagnostic_metrics_report_delta_and_never_fail_the_gate() {
    let metric = |name: &str, v: &str| DiagnosticMetric {
        name: name.to_owned(),
        value: v.to_owned(),
    };
    let mut c = row("preview-light", 5.0);
    let mut r = row("preview-light", 5.0);
    c.metrics = vec![metric("CacheHits", "10"), metric("PresentHistogram", "[]")];
    r.metrics = vec![metric("CacheHits", "4"), metric("PresentHistogram", "[1]")];
    let report = one(vec![c.clone()], vec![r.clone()]);
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.diagnostics[0].delta.as_deref(), Some("-6"));
    assert_eq!(report.diagnostics[1].delta, None);
    // A different metric key set is a schema mismatch.
    r.metrics.pop();
    assert_eq!(one(vec![c], vec![r]).verdict, Verdict::Incomparable);
}

// new: only p95 Regression of a performance profile sets exit 1; p50/max Regression is a warning note
#[test]
fn compare_p50_or_max_regression_is_a_warning_only() {
    let c = row("preview-light", 10.0);
    let mut r = row("preview-light", 10.0);
    r.p50_ms += 20.0;
    r.max_ms += 20.0;
    let report = one(vec![c], vec![r]);
    assert_eq!(report.exit_code(), 0);
    assert!(report.notes.iter().any(|n| n.contains("p50 Regression")));
    assert!(report.notes.iter().any(|n| n.contains("max Regression")));
}
