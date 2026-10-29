//! Synthetic benchmark comparison/report regressions; no measurements or real config.
use pr_bench::compare::{
    CompareReport, DatasetIdentity, DiagnosticDelta, PhaseRow, ProfileComparison, Summary, Thresholds, Verdict,
    compare, compare_repeats, parse_summary,
};
use pr_bench::report::{ReportContext, format_ms, render_markdown, to_json};

#[test]
fn report_rounding_handles_sign_ties_carry_and_nonfinite_values() {
    // Mutants report.rs:76-83: >= -> >, += -> -=, < -> <=/> and && -> ||.
    for (input, expected) in [
        (1.125, "1.13"),
        (-1.125, "-1.13"),
        (1.124, "1.12"),
        (1.999, "2.00"),
        (-0.001, "0.00"),
        (0.0, "0.00"),
        (f64::NAN, "n/a"),
        (f64::INFINITY, "n/a"),
    ] {
        assert_eq!(format_ms(input), expected);
    }
}

fn summary(p95: f64) -> Summary {
    Summary {
        dataset: DatasetIdentity {
            folder: "synthetic".into(),
            files: vec!["a.jpg".into()],
            unsupported_extensions: vec![],
            file_limit: 1,
        },
        rows: vec![PhaseRow {
            profile_id: "preview-light".into(),
            workload: 1,
            status: 0,
            samples_ms: vec![1.0, 2.0, p95],
            p50_ms: 2.0,
            p95_ms: p95,
            max_ms: p95,
            correctness_only: false,
            metrics: vec![],
        }],
    }
}

#[test]
fn compare_threshold_boundaries_preserve_direction_and_exit_status() {
    // Mutants compare.rs: classify comparisons/arithmetic and CompareReport::exit_code replacement.
    let threshold = Thresholds {
        minimum_repeats: 1,
        ..Thresholds::default()
    };
    for (base, actual, verdict, exit) in [
        (2.0, 2.0, Verdict::Pass, 0),
        (2.0, 3.0, Verdict::WithinNoise, 0),
        (2.0, 3.01, Verdict::Regression, 1),
        (200.0, 210.0, Verdict::WithinNoise, 0),
        (200.0, 210.01, Verdict::Regression, 1),
        (200.0, 189.99, Verdict::Improvement, 0),
        (200.0, 190.0, Verdict::Pass, 0),
    ] {
        let report = compare(&summary(base), &summary(actual), &threshold).unwrap();
        assert_eq!(report.profiles[0].verdict, verdict, "{base} -> {actual}");
        assert_eq!(report.verdict, verdict);
        assert_eq!(report.exit_code(), exit);
    }
}

fn profile(id: &str, verdict: Verdict) -> ProfileComparison {
    ProfileComparison {
        profile_id: id.to_owned(),
        workload: 1,
        correctness_only: false,
        verdict,
        reason: format!("reason-{id}"),
        repeat_count: 3,
        status: [0, 0],
        csharp_ms: [4.0, 8.0, 10.0],
        rust_ms: [4.0, 9.0, 10.0],
        sample_count: 30,
        sample_count_rust: 30,
        allowed_p95_delta_ms: Some(1.0),
    }
}

fn context(identity: Vec<(String, String)>) -> ReportContext {
    ReportContext {
        date: "2026-10-30".into(),
        machine: "m".into(),
        scenario: "s\u{1f} \u{20}x".into(),
        csharp_summary: "cs".into(),
        rust_summary: "rs".into(),
        thresholds: Thresholds::default(),
        identity,
        environment: vec![],
        attended: vec![],
        waiver: None,
        evidence: vec![],
        example: false,
    }
}

fn report() -> CompareReport {
    CompareReport {
        verdict: Verdict::Pass,
        profiles: vec![
            profile("ok", Verdict::Pass),
            profile("inc", Verdict::Incomparable),
            profile("skp", Verdict::Skipped),
        ],
        notes: vec![],
        dataset_equal: true,
        diagnostics: vec![],
    }
}

#[test]
fn markdown_report_prints_exact_key_value_rows_deltas_notes_and_threshold_percent() {
    // Mutants report.rs:184 kv_block -> String::new()/"xyzzy"; :237 and :250 `==` -> `!=`;
    // :313 format_pct -> String::new()/"xyzzy". (report.rs:75 `<` -> `<=` is equivalent:
    // value 0.0 gives cents 0, so the sign is empty either way.)
    let md = render_markdown(&report(), &context(vec![("Build".into(), "abc".into())]));
    assert!(
        md.contains("## 2. Build and invocation identity\n\n- Build: abc\n\n## 3."),
        "{md}"
    );
    assert!(
        md.contains("## 3. Machine and environment\n\nNot recorded.\n\n## 4."),
        "{md}"
    );
    let rows: Vec<&str> = md
        .lines()
        .filter(|l| l.starts_with("| ok ") || l.starts_with("| inc "))
        .collect();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].ends_with("| 1.00 | 1.00 | Pass |"), "{}", rows[0]);
    assert!(rows[1].contains("| n/a | 1.00 |"), "{}", rows[1]);
    assert!(md.contains("\n- inc Incomparable: reason-inc\n"));
    assert!(md.contains("\n- skp Skipped: reason-skp\n"));
    assert!(
        !md.contains("reason-ok"),
        "comparable rows must not print a reason line"
    );
    assert!(md.contains("`allowed = max(5% * |B|, 1.00 ms)`"), "{md}");
}

#[test]
fn json_report_escapes_control_characters_but_not_space() {
    // Mutants report.rs:88: match guard -> false, `<` -> `==`, `<` -> `<=`.
    let json = to_json(&report(), &context(vec![]));
    assert!(json.contains(r#""scenario":"s\u001f  x""#), "{json}");
}

fn row_summary(profile_id: &str, status: i64, p95: f64) -> Summary {
    let mut s = summary(p95);
    s.rows[0].profile_id = profile_id.into();
    s.rows[0].status = status;
    s
}

#[test]
fn verdict_names_and_error_text_are_exact() {
    // Mutants compare.rs:108 Verdict::name -> ""/"xyzzy"; :206 CompareError::fmt -> Ok(()).
    for (verdict, name) in [
        (Verdict::Pass, "Pass"),
        (Verdict::WithinNoise, "WithinNoise"),
        (Verdict::Regression, "Regression"),
        (Verdict::Improvement, "Improvement"),
        (Verdict::Incomparable, "Incomparable"),
        (Verdict::Skipped, "Skipped"),
    ] {
        assert_eq!(verdict.name(), name);
    }
    let one = [summary(2.0)];
    let bad = Thresholds {
        minimum_repeats: 0,
        ..Thresholds::default()
    };
    assert_eq!(
        compare_repeats(&one, &one, &bad).unwrap_err().to_string(),
        "invalid thresholds"
    );
    let mismatch = compare_repeats(&one, &[], &Thresholds::default()).unwrap_err();
    assert_eq!(
        mismatch.to_string(),
        "repeat count mismatch: 1 C# summaries, 0 Rust summaries"
    );
    let few = compare_repeats(&one, &one, &Thresholds::default()).unwrap_err();
    assert_eq!(few.to_string(), "too few repeats: 1 paired, 3 required");
}

#[test]
fn threshold_validation_rejects_each_invalid_field_alone_and_accepts_zero() {
    // Mutants compare.rs:233-236: `||` -> `&&`, `< 0.0` -> `<= 0.0` / `== 0.0`.
    let one = [summary(2.0)];
    let base = Thresholds {
        minimum_repeats: 1,
        ..Thresholds::default()
    };
    for bad in [
        Thresholds {
            ratio_tolerance: f64::NAN,
            ..base
        },
        Thresholds {
            ratio_tolerance: f64::INFINITY,
            ..base
        },
        Thresholds {
            ratio_tolerance: -0.1,
            ..base
        },
        Thresholds {
            absolute_floor_ms: f64::NAN,
            ..base
        },
        Thresholds {
            absolute_floor_ms: f64::INFINITY,
            ..base
        },
        Thresholds {
            absolute_floor_ms: -0.1,
            ..base
        },
        Thresholds {
            minimum_repeats: 0,
            ..base
        },
    ] {
        let err = compare_repeats(&one, &one, &bad).unwrap_err();
        assert_eq!(err.to_string(), "invalid thresholds", "{bad:?}");
    }
    let zero = Thresholds {
        ratio_tolerance: 0.0,
        absolute_floor_ms: 0.0,
        ..base
    };
    assert!(compare_repeats(&one, &one, &zero).is_ok());
}

#[test]
fn only_skipped_profiles_give_pass_not_improvement() {
    // Mutant compare.rs:301 `&&` -> `||`: an all-Skipped set is vacuously "all Improvement".
    let s = [row_summary("explorer-reindex", 0, 2.0)];
    let report = compare_repeats(
        &s,
        &s,
        &Thresholds {
            minimum_repeats: 1,
            ..Thresholds::default()
        },
    )
    .unwrap();
    assert_eq!(report.profiles[0].verdict, Verdict::Skipped);
    assert_eq!(report.verdict, Verdict::Pass);
}

#[test]
fn unusable_status_is_incomparable_with_sentinel_fields() {
    // Mutants compare.rs:333 delete `-` (status sentinel [-1, -1]) and :386 `||` -> `&&`
    // (status 3 and out-of-range statuses must be rejected independently of status 2).
    let th = Thresholds {
        minimum_repeats: 1,
        ..Thresholds::default()
    };
    for status in [2, 3, 5, -1] {
        let s = [row_summary("preview-light", status, 2.0)];
        let report = compare_repeats(&s, &s, &th).unwrap();
        let p = &report.profiles[0];
        assert_eq!(p.verdict, Verdict::Incomparable, "status {status}");
        assert_eq!(p.reason, format!("status {status} is not a speed result"));
        assert_eq!(p.status, [-1, -1]);
        assert_eq!(report.exit_code(), 3);
    }
}

#[test]
fn regression_needs_a_strict_majority_of_pairs_strictly_above_allowed() {
    // Mutants compare.rs:519 `>` -> `>=` (pair delta equal to allowed is not worse) and
    // :520 `* 2` -> `+ 2` (1 worse of 2 pairs is not a strict majority).
    let th = Thresholds {
        minimum_repeats: 2,
        ..Thresholds::default()
    };
    // allowed = max(0.05 * 10, 1) = 1; pair deltas 1 (equal, not worse) and 3 (worse); median delta 2.
    let report = compare_repeats(&[summary(10.0), summary(10.0)], &[summary(11.0), summary(13.0)], &th).unwrap();
    assert_eq!(report.profiles[0].verdict, Verdict::WithinNoise);
    assert!(
        report.profiles[0].reason.ends_with("(unstable)"),
        "{}",
        report.profiles[0].reason
    );
    assert_eq!(report.exit_code(), 0);
    // Both pairs strictly worse: a strict majority.
    let report = compare_repeats(&[summary(10.0), summary(10.0)], &[summary(12.0), summary(13.0)], &th).unwrap();
    assert_eq!(report.profiles[0].verdict, Verdict::Regression);
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn mixed_skipped_and_improved_profiles_are_overall_improvement() {
    // Mutant compare.rs:299 `!=` -> `==` (the Skipped filter of the Improvement rule).
    let th = Thresholds {
        minimum_repeats: 1,
        ..Thresholds::default()
    };
    let mut baseline = row_summary("explorer-reindex", 0, 2.0);
    let mut fast = row_summary("preview-light", 0, 100.0);
    baseline.rows.append(&mut fast.rows);
    let mut candidate = baseline.clone();
    candidate.rows[1].p95_ms = 10.0;
    candidate.rows[1].max_ms = 10.0;
    let report = compare_repeats(&[baseline], &[candidate], &th).unwrap();
    assert_eq!(report.profiles[0].verdict, Verdict::Skipped);
    assert_eq!(report.profiles[1].verdict, Verdict::Improvement);
    assert_eq!(report.verdict, Verdict::Improvement);
}

fn summary_json(extra_root: &str, metrics: &str, reports: usize) -> String {
    let report = format!("{{\"Metrics\":{metrics}}}");
    let reports = vec![report; reports].join(",");
    let folder = r#"a\"b\\c\/d\n\r\t\b\f@u0041@u00e9z"#.replace("@u", r"\u");
    format!(
        "\u{feff} \r\n{{\"Dataset\":{{\"Folder\":\"{folder}\",\"Files\":[\"x\"],\
         \"UnsupportedExtensions\":{{\".raw\":3}},\"FileLimit\":64}},\"Reports\":[{reports}],\
         \"ProfileOutcomes\":[{{\"ProfileId\":\"preview-light\",\"Workload\":1,\"Status\":0,\
         \"Samples\":[1.5,2.5e0],\"P50\":2,\"P95\":3,\"Max\":4}}]{extra_root}}}"
    )
}

#[test]
fn summary_parser_decodes_escapes_literals_numbers_and_metrics() {
    // Mutants compare.rs Reader::string (escape arms, position arithmetic), Reader::value
    // (true/false/null guards and `pos +=` lengths), parse_summary (Reports length guard,
    // Value::Arr arm).
    let metrics = r#"{"t":true,"f":false,"n":null,"num":-1.5e+2,"arr":[1,"s"],"o":{"k":2}}"#;
    let parsed = parse_summary(&summary_json("", metrics, 1)).unwrap();
    assert_eq!(parsed.dataset.folder, "a\"b\\c/d\n\r\t\u{8}\u{c}A\u{e9}z");
    assert_eq!(parsed.dataset.files, ["x"]);
    assert_eq!(parsed.dataset.unsupported_extensions, [(".raw".to_owned(), 3)]);
    assert_eq!(parsed.dataset.file_limit, 64);
    let row = &parsed.rows[0];
    assert_eq!(row.samples_ms, [1.5, 2.5]);
    assert_eq!((row.p50_ms, row.p95_ms, row.max_ms), (2.0, 3.0, 4.0));
    let got: Vec<(&str, &str)> = row
        .metrics
        .iter()
        .map(|m| (m.name.as_str(), m.value.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("t", "true"),
            ("f", "false"),
            ("n", "null"),
            ("num", "-1.5e+2"),
            ("arr", "[1,\"s\"]"),
            ("o", "{\"k\":2}")
        ]
    );
    // Reports of a different length than ProfileOutcomes are ignored.
    assert!(
        parse_summary(&summary_json("", metrics, 2)).unwrap().rows[0]
            .metrics
            .is_empty()
    );
    assert!(
        parse_summary(&summary_json("", metrics, 0)).unwrap().rows[0]
            .metrics
            .is_empty()
    );
}

#[test]
fn summary_parser_rejects_bad_literals_numbers_and_excess_nesting_without_panicking() {
    // Mutants compare.rs Reader::value: literal guards, `depth > 64` (+ `depth + 1` in both
    // containers) and the number loop bound `pos < len`.
    for bad in ["t", "tru", "trux", "f", "fals", "falsx", "n", "nul", "nulx"] {
        let err = parse_summary(bad).unwrap_err().to_string();
        assert_eq!(err, "malformed summary JSON at byte 0: unexpected token", "{bad}");
    }
    assert!(parse_summary("5").unwrap_err().to_string().contains("no Dataset"));
    for bad in ["-", "1.2.3", "[1 2]", r#"{"a" 1}"#, r#""abc"#, r#""\q""#, r#""\u12""#] {
        let err = parse_summary(bad).unwrap_err().to_string();
        assert!(err.starts_with("malformed summary JSON"), "{bad}: {err}");
    }
    let nested = |value: String| summary_json(&format!(",\"X\":{value}"), "{}", 1);
    let arrays = |levels: usize| nested(format!("{}{}", "[".repeat(levels), "]".repeat(levels)));
    let objects = |levels: usize| nested(format!("{}null{}", "{\"a\":".repeat(levels), "}".repeat(levels)));
    assert!(parse_summary(&arrays(64)).is_ok());
    assert!(parse_summary(&objects(63)).is_ok());
    for deep in [arrays(65), objects(64)] {
        assert!(
            parse_summary(&deep)
                .unwrap_err()
                .to_string()
                .contains("nesting too deep")
        );
    }
}

#[test]
fn cli_threshold_floor_zero_is_valid_and_negative_is_a_usage_error() {
    // Mutants compare.rs:791 `absolute_floor_ms < 0.0` -> `<=` / `==` in parse_thresholds.
    // Synthetic JSON files in a TempRoot, one short-lived owned process per case.
    let root = pr_test_support::TempRoot::new("w7-threshold-floor");
    let summary = r#"{"Dataset":{"Folder":"f","Files":["a"],"UnsupportedExtensions":{},"FileLimit":1},
        "Reports":[],"ProfileOutcomes":[{"ProfileId":"preview-light","Workload":1,"Status":0,
        "Samples":[2.0],"P50":2.0,"P95":2.0,"Max":2.0}]}"#;
    std::fs::write(root.join("b.json"), summary).unwrap();
    std::fs::write(root.join("r.json"), summary).unwrap();
    for (thresholds, code) in [
        (r#"{"minimumRepeats":1,"absoluteFloorMs":0,"ratioTolerance":0}"#, 0),
        (r#"{"minimumRepeats":1,"absoluteFloorMs":-1}"#, 2),
    ] {
        std::fs::write(root.join("t.json"), thresholds).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_pr-bench"))
            .current_dir(root.path())
            .args([
                "--bench-compare",
                "b.json",
                "r.json",
                "--thresholds",
                "t.json",
                "--out",
                "out",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code), "{thresholds}");
    }
}

#[test]
fn markdown_diagnostics_mark_equal_and_different_values() {
    // Mutant report.rs:250 `d.csharp == d.rust` -> `!=` (the Equal column).
    let mut report = report();
    report.diagnostics = vec![
        DiagnosticDelta {
            profile_id: "p".into(),
            name: "same".into(),
            csharp: Some("5".into()),
            rust: Some("5".into()),
            delta: None,
        },
        DiagnosticDelta {
            profile_id: "p".into(),
            name: "diff".into(),
            csharp: Some("10".into()),
            rust: Some("4".into()),
            delta: Some("-6".into()),
        },
    ];
    let md = render_markdown(&report, &context(vec![]));
    let lines: Vec<&str> = md.lines().collect();
    assert!(lines.contains(&"| p/same | 5 | 5 | yes | delta n/a |"), "{md}");
    assert!(lines.contains(&"| p/diff | 10 | 4 | no | delta -6 |"), "{md}");
}
