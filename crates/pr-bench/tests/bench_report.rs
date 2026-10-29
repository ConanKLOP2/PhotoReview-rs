//! Tests for `pr_bench::report` (unit W10-B3, `contracts/W10-B3.md`). The contract proposes new formatting tests (C# has
//! no report generator), each marked `new:`. Synthetic compare results only; nothing here is a measurement.

use pr_bench::compare::{
    CompareReport, DatasetIdentity, DiagnosticDelta, PhaseRow, ProfileComparison, Summary, Thresholds, Verdict, compare,
};
use pr_bench::report::{ReportContext, file_stem, format_ms, render_markdown, to_json};

fn ctx(example: bool) -> ReportContext {
    ReportContext {
        date: "2026-10-29".to_owned(),
        machine: "m1".to_owned(),
        scenario: "warm".to_owned(),
        csharp_summary: "out/cs/summary.json".to_owned(),
        rust_summary: "out/rs/summary.json".to_owned(),
        thresholds: Thresholds::default(),
        identity: vec![("C# build".to_owned(), "synthetic".to_owned())],
        environment: Vec::new(),
        attended: Vec::new(),
        waiver: None,
        evidence: vec!["cmd: pr-bench --bench-compare a b".to_owned()],
        example,
    }
}

fn profile(id: &str, verdict: Verdict, c95: f64, r95: f64) -> ProfileComparison {
    ProfileComparison {
        profile_id: id.to_owned(),
        workload: 1,
        correctness_only: false,
        verdict,
        reason: "because".to_owned(),
        repeat_count: 3,
        status: [0, 0],
        csharp_ms: [4.0, c95, 10.0],
        rust_ms: [4.2, r95, 10.4],
        sample_count: 30,
        sample_count_rust: 30,
        allowed_p95_delta_ms: Some(1.0),
    }
}

fn report(profiles: Vec<ProfileComparison>, verdict: Verdict) -> CompareReport {
    CompareReport {
        verdict,
        profiles,
        notes: vec!["a note".to_owned()],
        dataset_equal: true,
        diagnostics: vec![DiagnosticDelta {
            profile_id: "p".to_owned(),
            name: "CacheHits".to_owned(),
            csharp: Some("10".to_owned()),
            rust: Some("4".to_owned()),
            delta: Some("-6".to_owned()),
        }],
    }
}

// new: render_report_uses_fixed_section_order
#[test]
fn render_report_uses_fixed_section_order() {
    let md = render_markdown(
        &report(vec![profile("p", Verdict::Pass, 8.5, 8.5)], Verdict::Pass),
        &ctx(false),
    );
    let marks = [
        "# Benchmark parity",
        "## 2. Build and invocation identity",
        "## 3. Machine and environment",
        "## 4. Dataset manifest",
        "## 5. C#/Rust benchmark comparison",
        "## 6. Exact metrics parity",
        "## 7. Attended production surface",
        "## 8. Decision and waiver record",
        "## 9. Not measured",
        "## 10. Raw evidence",
    ];
    let mut last = 0;
    for m in marks {
        let at = md.find(m).unwrap_or_else(|| panic!("missing section {m}"));
        assert!(at >= last, "{m} out of order");
        last = at;
    }
    assert!(md.contains("attended measurement (W10-B2) not done"));
    assert!(md.contains("Overall verdict: Pass (exit code 0)"));
}

// new: render_report_rounds_half_up_invariantly
#[test]
fn render_report_rounds_half_up_invariantly() {
    for (v, want) in [
        (0.125, "0.13"),
        (0.124, "0.12"),
        (8.505, "8.51"),
        (2.675, "2.68"),
        (0.0, "0.00"),
        (1234.5, "1234.50"),
        (-0.125, "-0.13"),
        (-0.001, "0.00"),
        (f64::NAN, "n/a"),
    ] {
        assert_eq!(format_ms(v), want, "value {v}");
    }
}

// new: render_report_marks_example_values
#[test]
fn render_report_marks_example_values() {
    let r = report(
        vec![profile("p", Verdict::WithinNoise, 8.5, 8.75)],
        Verdict::WithinNoise,
    );
    let md = render_markdown(&r, &ctx(true));
    assert!(md.contains("EXAMPLE 8.50/8.75"));
    assert!(md.contains("EXAMPLE 0.25"));
    assert!(md.contains("EXAMPLE 30/30"));
    let plain = render_markdown(&r, &ctx(false));
    assert!(!plain.contains("EXAMPLE"));
    assert!(plain.contains("8.50/8.75"));
}

// new: render_report_includes_incomparability_reasons
#[test]
fn render_report_includes_incomparability_reasons() {
    let mut p = profile("p", Verdict::Incomparable, f64::NAN, f64::NAN);
    p.reason = "status mismatch: 0 vs 1".to_owned();
    let md = render_markdown(&report(vec![p], Verdict::Incomparable), &ctx(false));
    assert!(md.contains("status mismatch: 0 vs 1"));
    assert!(md.contains("Overall verdict: Incomparable (exit code 3)"));
    assert!(md.contains("n/a"));
}

// new: the markdown prints the compare verdict, it never recalculates one
#[test]
fn render_report_prints_the_given_verdict_without_recalculating() {
    // A Regression verdict with equal numbers stays Regression in the output.
    let md = render_markdown(
        &report(vec![profile("p", Verdict::Regression, 5.0, 5.0)], Verdict::Regression),
        &ctx(false),
    );
    assert!(md.contains("| Regression |"));
    assert!(md.contains("Overall verdict: Regression (exit code 1)"));
}

// new: json form of the contract (null for non-finite, verdict names, exit code)
#[test]
fn to_json_serialises_nonfinite_as_null_and_keeps_verdicts() {
    let json = to_json(
        &report(
            vec![profile("p", Verdict::Incomparable, f64::NAN, f64::INFINITY)],
            Verdict::Incomparable,
        ),
        &ctx(false),
    );
    assert!(json.contains("\"schemaVersion\":1"));
    assert!(json.contains("\"overallVerdict\":\"Incomparable\""));
    assert!(json.contains("\"exitCode\":3"));
    assert!(json.contains("\"p95Ms\":null"));
    assert!(!json.contains("NaN") && !json.contains("inf"));
    assert!(json.contains("\"minimumRepeats\":3"));
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(parsed["profiles"][0]["verdict"], "Incomparable");
    assert_eq!(parsed["diagnosticMetrics"][0]["delta"], "-6");
}

// new: end to end with the real compare on synthetic summaries, file stem
#[test]
fn report_from_a_real_compare_result_round_trips() {
    let row = |p95: f64| PhaseRow {
        profile_id: "preview-light".to_owned(),
        workload: 1,
        status: 0,
        samples_ms: vec![p95],
        p50_ms: p95,
        p95_ms: p95,
        max_ms: p95,
        correctness_only: false,
        metrics: Vec::new(),
    };
    let s = |p95| Summary {
        rows: vec![row(p95)],
        dataset: DatasetIdentity {
            folder: "f".to_owned(),
            files: Vec::new(),
            unsupported_extensions: Vec::new(),
            file_limit: 64,
        },
    };
    let t = Thresholds {
        minimum_repeats: 1,
        ..Thresholds::default()
    };
    let r = compare(&s(10.0), &s(20.0), &t).unwrap();
    assert_eq!(r.verdict, Verdict::Regression);
    let md = render_markdown(&r, &ctx(false));
    assert!(md.contains("10.00/20.00"));
    assert_eq!(file_stem(&ctx(false)), "2026-10-29-m1-warm");
}
