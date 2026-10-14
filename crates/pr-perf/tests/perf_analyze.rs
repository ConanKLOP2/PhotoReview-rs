//! W1-21: the `--perf-analyze` pipeline (`PerfAnalyze.cs`, `NaNAsNullDoubleConverter.cs`).
//!
//! The end-to-end tests drive `pr_perf::analyze::run` (units W1-16..W1-20 are merged; all tests run).

use std::fs;
use std::path::Path;

use pr_perf::analyze::{AnalysisResult, is_burst_scenario, nan_as_null_double_json, nan_as_null_double_read, run};
use pr_perf::csv::PerfError;
use pr_test_support::TempRoot;
use pr_test_support::golden;
use serde_json::Value;

const HEADER: &str =
    "# commit=test diag= qpcFrequency=1000000\nutcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\n";

fn write(root: &Path, rel: &str, content: &[u8]) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("create dirs");
    }
    fs::write(&p, content).expect("write file");
}

fn one_nav(nav: i64) -> String {
    format!(
        "1,{},1,ShowStart,{nav},,0,,,,Preview\n2,{},1,Presented,{nav},,1,,,,final\n",
        1000 * nav,
        1000 * nav + 500
    )
}

fn sample_csv() -> String {
    let g = golden::load("perf-analyze");
    let case = g
        .cases
        .iter()
        .find(|c| c["kind"] == "run" && c["name"] == "sample-dir")
        .expect("sample-dir case");
    case["files"]["perf-sample.csv"]
        .as_str()
        .expect("sample text")
        .to_owned()
}

// ---- IsBurstScenario ----

// C#: PerfAnalyzeTests.BurstScenarioMatchesTheScenarioPartOnly
#[test]
fn is_burst_scenario_matches_the_scenario_part_only() {
    let rows = [
        ("S3", true),
        ("s4@F1", true),
        ("S3-burst@F2", true),
        ("S2@pics3", false),
        ("S30", false),
        ("S2@class4", false),
    ];
    for (key, expected) in rows {
        assert_eq!(is_burst_scenario(key), expected, "key {key:?}");
    }
}

// C#: PerfAnalyzeTests.BurstScenarioMatchesTheScenarioPartOnly (golden perf-analyze `burst`)
#[test]
fn golden_burst_cases() {
    let g = golden::load("perf-analyze");
    let mut n = 0;
    for c in g.cases.iter().filter(|c| c["kind"] == "burst") {
        let key = golden::str_of(c, "key");
        assert_eq!(is_burst_scenario(key), golden::bool_of(c, "is_burst"), "key {key:?}");
        n += 1;
    }
    assert_eq!(n, 19);
}

// ---- NaNAsNullDoubleConverter ----

// C#: NaNAsNullDoubleConverter.Read (no direct C# test)
#[test]
fn nan_as_null_double_read_maps_null_number_and_other_tokens() {
    assert!(nan_as_null_double_read(&Value::Null).is_some_and(f64::is_nan));
    assert_eq!(nan_as_null_double_read(&serde_json::json!(12.5)), Some(12.5));
    assert_eq!(nan_as_null_double_read(&serde_json::json!(3)), Some(3.0));
    assert_eq!(nan_as_null_double_read(&serde_json::json!("NaN")), None);
    assert_eq!(nan_as_null_double_read(&serde_json::json!(true)), None);
}

// C#: NaNAsNullDoubleConverter.Write (golden perf-analyze `json_double`)
#[test]
fn golden_json_double_cases() {
    let g = golden::load("perf-analyze");
    let mut n = 0;
    for c in g.cases.iter().filter(|c| c["kind"] == "json_double") {
        let text = golden::str_of(c, "text");
        let v = match text {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            t => t.parse::<f64>().unwrap_or_else(|e| panic!("parse {t:?}: {e}")),
        };
        assert_eq!(nan_as_null_double_json(v), golden::str_of(c, "json"), "text {text:?}");
        n += 1;
    }
    assert_eq!(n, 25);
}

// ---- end-to-end ----

// C#: PerfAnalyzeTests.PerfAnalyzeOnTheSampleDirectoryWritesSummaryMdAndJson
#[test]
fn run_on_the_sample_directory_writes_summary_md_and_json() {
    let root = TempRoot::new("perf-analyze-sample");
    write(root.path(), "perf-sample.csv", sample_csv().as_bytes());

    let result = run(root.path(), None).expect("run");

    assert_eq!(result.csv_file_count, 1);
    assert_eq!(result.groups.len(), 1);
    assert!(result.summary_md_path.is_file());
    assert!(result.summary_json_path.is_file());
    let md = fs::read_to_string(&result.summary_md_path).expect("md");
    assert!(md.contains("Perf analyze summary"));
    assert!(md.contains("R-DEC"));
    assert!(md.contains("renderedFrame P50"));
    let json = fs::read_to_string(&result.summary_json_path).expect("json");
    assert!(json.contains("\"csvFileCount\": 1"));
    assert!(json.contains("\"renderedFrameMs\""));
    let summary = &result.groups[0].summary;
    assert!(!summary.rendered_frame_p50.is_nan());
    assert!(!summary.rendered_frame_p95.is_nan());
}

// C#: PerfAnalyzeTests.PerfAnalyzeWithOnlyIncompleteNavsStillWritesJson
#[test]
fn run_with_only_incomplete_navs_still_writes_json() {
    let root = TempRoot::new("perf-analyze-incomplete");
    write(
        root.path(),
        "perf-1-x.csv",
        b"# commit=test diag= qpcFrequency=5000000\nutcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\n1,1000,1,ShowStart,1,,0,,,,Preview\n",
    );

    let result = run(root.path(), None).expect("run");

    let json = fs::read_to_string(&result.summary_json_path).expect("json");
    assert!(json.contains("\"groups\""));
    let doc: Value = serde_json::from_str(&json).expect("valid json");
    assert!(doc["groups"][0]["firstVisualMs"]["p50"].is_null());
}

// C#: PerfAnalyzeTests.PerfAnalyzeThrowsWhenNoCsvFilesArePresent
#[test]
fn run_without_csv_files_is_invalid_operation() {
    let root = TempRoot::new("perf-analyze-empty");
    let err = run(root.path(), None).expect_err("must fail");
    assert!(matches!(err, PerfError::InvalidOperation(_)), "{err:?}");
}

// C#: PerfAnalyzeTests.PerfAnalyzeKeepsWorkerTreatmentsSeparateAndOnlyComparesSameCondition
#[test]
fn run_keeps_worker_treatments_separate_and_only_compares_same_condition() {
    let root = TempRoot::new("perf-analyze-workers");
    let csv = sample_csv();
    write(root.path(), "S2/fixture-Preview-warm/run-01/perf-0.csv", csv.as_bytes());
    write(
        root.path(),
        "S2/fixture-Preview-cold-app/run-01/perf-8.csv",
        csv.as_bytes(),
    );
    write(
        root.path(),
        "S2/fixture-Preview-warm/run-01/session.json",
        br#"{"scenario":"S2","mode":"Preview","condition":"warm","preloadWorkers":0}"#,
    );
    write(
        root.path(),
        "S2/fixture-Preview-cold-app/run-01/session.json",
        br#"{"scenario":"S2","mode":"Preview","condition":"cold-app","preloadWorkers":8}"#,
    );

    let result = run(root.path(), None).expect("run");

    assert_eq!(result.groups.len(), 2);
    assert!(
        result
            .groups
            .iter()
            .any(|g| g.summary.key.preload_workers == Some(0) && g.summary.key.cond == "warm")
    );
    assert!(
        result
            .groups
            .iter()
            .any(|g| g.summary.key.preload_workers == Some(8) && g.summary.key.cond == "cold-app")
    );
    for g in &result.groups {
        let r = g.rules.iter().find(|r| r.rule == "R-CONT").expect("R-CONT");
        assert_eq!(r.triggered, None);
    }
}

// C#: PerfAnalyzeReportTests.DroppedRowsAreSurfacedInMarkdownAndJson
#[test]
fn dropped_rows_are_surfaced_in_markdown_and_json() {
    let root = TempRoot::new("perf-analyze-dropped");
    write(
        root.path(),
        "a/perf-1.csv",
        format!("{HEADER}{}# dropped=5\n", one_nav(1)).as_bytes(),
    );
    write(
        root.path(),
        "a/session.json",
        br#"{"scenario":"S2","mode":"Preview","condition":"warm"}"#,
    );

    let result = run(root.path(), None).expect("run");

    assert_eq!(result.groups.len(), 1);
    assert_eq!(result.groups[0].summary.dropped_rows, 5);
    let md = fs::read_to_string(&result.summary_md_path).expect("md");
    assert!(md.contains("dropped 5"));
    let doc: Value =
        serde_json::from_str(&fs::read_to_string(&result.summary_json_path).expect("json")).expect("parse");
    assert_eq!(doc["groups"][0]["droppedRows"].as_i64(), Some(5));
}

// C#: PerfAnalyzeReportTests.NoDroppedRowsMeansNoWarning
#[test]
fn no_dropped_rows_means_no_warning() {
    let root = TempRoot::new("perf-analyze-nodrop");
    write(
        root.path(),
        "a/perf-1.csv",
        format!("{HEADER}{}", one_nav(1)).as_bytes(),
    );
    write(
        root.path(),
        "a/session.json",
        br#"{"scenario":"S2","mode":"Preview","condition":"warm"}"#,
    );

    let result = run(root.path(), None).expect("run");

    let md = fs::read_to_string(&result.summary_md_path).expect("md");
    assert!(!md.to_lowercase().contains("dropped"));
}

// C#: PerfAnalyzeReportTests.GroupsAreOrderedByWorkerCountNumericallyNotAsText
#[test]
fn groups_are_ordered_by_worker_count_numerically_not_as_text() {
    let root = TempRoot::new("perf-analyze-order");
    for workers in [10, 2] {
        write(
            root.path(),
            &format!("w{workers}/perf-1.csv"),
            format!("{HEADER}{}", one_nav(1)).as_bytes(),
        );
        write(
            root.path(),
            &format!("w{workers}/session.json"),
            format!(r#"{{"scenario":"S2","mode":"Preview","condition":"warm","preloadWorkers":{workers}}}"#).as_bytes(),
        );
    }

    let result = run(root.path(), None).expect("run");

    let workers: Vec<i32> = result
        .groups
        .iter()
        .map(|g| g.summary.key.preload_workers.expect("workers"))
        .collect();
    assert_eq!(workers, vec![2, 10]);
}

// C#: PerfAnalyzeReportTests.PipeInAGroupNameIsEscapedSoTheTableKeepsItsColumns
#[test]
fn pipe_in_a_group_name_is_escaped_so_the_table_keeps_its_columns() {
    let root = TempRoot::new("perf-analyze-pipe");
    write(
        root.path(),
        "a/perf-1.csv",
        format!("{HEADER}{}", one_nav(1)).as_bytes(),
    );
    write(
        root.path(),
        "a/session.json",
        br#"{"scenario":"S2|x","mode":"Preview","condition":"warm"}"#,
    );

    let result = run(root.path(), None).expect("run");

    let md = fs::read_to_string(&result.summary_md_path).expect("md");
    assert!(md.contains(r"S2\|x/Preview/warm"));
    assert!(!md.contains("| S2|x/"));
}

// ---- golden `run` ----

fn decode_hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn opt_f64(v: &Value) -> Option<f64> {
    if v.is_null() { None } else { v.as_f64() }
}

fn check_groups(name: &str, case: &Value, result: &AnalysisResult) {
    let expected = case["groups"].as_array().expect("groups");
    assert_eq!(result.groups.len(), expected.len(), "{name}: group count");
    for (i, (g, e)) in result.groups.iter().zip(expected).enumerate() {
        let ctx = format!("{name} group {i}");
        let s = &g.summary;
        assert_eq!(s.key.to_string(), golden::str_of(e, "key_string"), "{ctx}: key_string");
        assert_eq!(s.key.scenario, golden::str_of(e, "scenario"), "{ctx}: scenario");
        assert_eq!(s.key.mode, golden::str_of(e, "mode"), "{ctx}: mode");
        assert_eq!(s.key.cond, golden::str_of(e, "cond"), "{ctx}: cond");
        assert_eq!(
            s.preload_workers.map(i64::from),
            e["workers"].as_i64(),
            "{ctx}: workers"
        );
        assert_eq!(i64::from(s.count), golden::i64_of(e, "count"), "{ctx}: count");
        assert_eq!(
            i64::from(s.incomplete),
            golden::i64_of(e, "incomplete"),
            "{ctx}: incomplete"
        );
        assert_eq!(s.dropped_rows, golden::i64_of(e, "dropped_rows"), "{ctx}: dropped_rows");
        assert_eq!(
            s.gc_time_percent,
            opt_f64(&e["gc_time_percent"]),
            "{ctx}: gc_time_percent"
        );
        assert_eq!(
            s.frame_time_p95_ms,
            opt_f64(&e["frame_time_p95_ms"]),
            "{ctx}: frame_time_p95_ms"
        );
        let rules = e["rules"].as_array().expect("rules");
        assert_eq!(g.rules.len(), rules.len(), "{ctx}: rule count");
        for (r, er) in g.rules.iter().zip(rules) {
            let rctx = format!("{ctx} {}", r.rule);
            assert_eq!(r.rule, golden::str_of(er, "rule"), "{rctx}");
            assert_eq!(r.triggered, er["triggered"].as_bool(), "{rctx}: triggered");
            assert_eq!(r.evidence, golden::str_of(er, "evidence"), "{rctx}: evidence");
            assert_eq!(r.note.as_deref(), er["note"].as_str(), "{rctx}: note");
        }
    }
}

// C#: PerfAnalyze.RunAsync (golden perf-analyze `run`, 19 cases)
#[test]
fn golden_run_cases() {
    let g = golden::load("perf-analyze");
    let mut n = 0;
    for case in g.cases.iter().filter(|c| c["kind"] == "run") {
        n += 1;
        let name = golden::str_of(case, "name");
        let root = TempRoot::new("perf-analyze-golden");
        let second = TempRoot::new("perf-analyze-golden-rules");
        if let Some(files) = case["files"].as_object() {
            for (rel, v) in files {
                let bytes: Vec<u8> = if let Some(t) = v.as_str() {
                    t.as_bytes().to_vec()
                } else if let Some(hex) = v["raw_hex"].as_str() {
                    decode_hex(hex)
                } else {
                    let mut b = Vec::new();
                    if v["bom"].as_bool() == Some(true) {
                        b.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
                    }
                    b.extend_from_slice(golden::str_of(v, "content").as_bytes());
                    b
                };
                write(root.path(), rel, &bytes);
            }
        }
        let run_dir = if case["missing_dir"].as_bool() == Some(true) {
            root.path().join("does-not-exist")
        } else {
            root.path().to_path_buf()
        };
        let rules_path = if case["missing_rules"].as_bool() == Some(true) {
            Some(second.path().join("nope.json"))
        } else if let Some(json) = case["rules_json"].as_str() {
            write(second.path(), "rules.json", json.as_bytes());
            Some(second.path().join("rules.json"))
        } else {
            None
        };

        let outcome = run(&run_dir, rules_path.as_deref());

        if let Some(err) = case.get("error") {
            let ty = golden::str_of(err, "type");
            let msg = golden::str_of(err, "message").replace("<RUNDIR>", &root.path().display().to_string());
            let e = outcome.expect_err(&format!("{name}: must fail"));
            match (ty, e) {
                ("InvalidOperationException", PerfError::InvalidOperation(m)) => assert_eq!(m, msg, "{name}"),
                ("DirectoryNotFoundException", PerfError::DirectoryNotFound(m)) => assert_eq!(m, msg, "{name}"),
                ("InvalidDataException", PerfError::InvalidData(m)) => {
                    let prefix = format!("Invalid rules file '{}': ", second.path().join("rules.json").display());
                    assert!(m.starts_with(&prefix), "{name}: {m}");
                }
                ("FileNotFoundException", PerfError::Io(io)) => {
                    assert_eq!(io.kind(), std::io::ErrorKind::NotFound, "{name}");
                }
                (t, e) => panic!("{name}: expected {t}, got {e:?}"),
            }
            continue;
        }

        let result = outcome.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            i64::from(result.csv_file_count),
            golden::i64_of(case, "csv_file_count"),
            "{name}"
        );
        check_groups(name, case, &result);
        assert_eq!(
            result.summary_md_path.file_name().and_then(|s| s.to_str()),
            Some(golden::str_of(case, "summary_md_name")),
            "{name}"
        );
        assert_eq!(
            fs::read_to_string(&result.summary_md_path).expect("md"),
            golden::str_of(case, "summary_md"),
            "{name}: summary.md"
        );
        assert_eq!(
            fs::read_to_string(&result.summary_json_path).expect("json"),
            golden::str_of(case, "summary_json"),
            "{name}: summary.json"
        );
    }
    assert_eq!(n, 19);
}

// ---- mutant killers: dispatcher threshold, InflightJoin decode navs, empty startup ----

const HEADER_1MHZ: &str =
    "# commit=test diag= qpcFrequency=1000000\nutcTicks,qpcTicks,thread,event,nav,pathId,a,b,c,d,text\n";

/// One complete nav 1 (ShowStart at 100 ms, final at 200 ms) with one DispatcherLongOp of `ms` in between.
fn dispatcher_csv(ms: &str) -> String {
    format!(
        "{HEADER_1MHZ}1,100000,1,ShowStart,1,,0,,,,Preview\n1,100100,1,Lookup,1,,,,,,ramHit\n\
         1,150000,1,DispatcherLongOp,,,{ms},,,,Normal;Layout\n1,200000,1,Presented,1,,1,,,,final\n"
    )
}

/// One complete nav 1 whose lookup is `inflight` (kind `InflightJoin`) with a decode of `decode_ms`, final at +100 ms.
fn inflight_csv(decode_ms: &str) -> String {
    format!(
        "{HEADER_1MHZ}1,100000,1,ShowStart,1,,0,,,,Preview\n1,100100,1,Lookup,1,,,,,,inflight\n\
         1,100150,4,Decode,1,,{decode_ms},,,,\n1,200000,1,Presented,1,,1,,,,final\n"
    )
}

fn rule<'a>(g: &'a pr_perf::analyze::GroupResult, name: &str) -> &'a pr_perf::rules::RuleResult {
    g.rules.iter().find(|r| r.rule == name).expect("rule present")
}

// C#: PerfAnalyze.EvaluateAllRules (`DispatcherLongOps.Count(d => d.Ms > dispatcherThreshold)`, strict >)
#[test]
fn dispatcher_long_op_of_exactly_the_threshold_is_not_counted() {
    let at = TempRoot::new("perf-analyze-dispatch-at");
    write(at.path(), "perf-1.csv", dispatcher_csv("16.0").as_bytes());
    let result = run(at.path(), None).expect("run");
    assert_eq!(result.groups.len(), 1);
    let g = &result.groups[0];
    assert_eq!(g.summary.dispatcher_long_ops.len(), 1);
    assert!(g.summary.startup_runs.is_empty(), "no Startup rows -> no startup run");
    let r = rule(g, "R-THREAD");
    assert!(
        r.evidence.starts_with("DispatcherLongOp>16ms count=0;"),
        "{}",
        r.evidence
    );
    assert_eq!(r.triggered, None);

    let above = TempRoot::new("perf-analyze-dispatch-above");
    write(above.path(), "perf-1.csv", dispatcher_csv("16.5").as_bytes());
    let result = run(above.path(), None).expect("run");
    let r = rule(&result.groups[0], "R-THREAD");
    assert!(
        r.evidence.starts_with("DispatcherLongOp>16ms count=1;"),
        "{}",
        r.evidence
    );
    assert_eq!(r.triggered, Some(true));
}

// C#: PerfAnalyze.EvaluateAllRules (decodeNavs include InflightJoin)
#[test]
fn inflight_join_navs_count_as_decode_navs_for_r_dec() {
    let root = TempRoot::new("perf-analyze-inflight-dec");
    write(root.path(), "perf-1.csv", inflight_csv("40.0").as_bytes());
    let result = run(root.path(), None).expect("run");
    let g = &result.groups[0];
    assert_eq!(g.summary.navs.len(), 1);
    assert_eq!(g.summary.navs[0].kind, "InflightJoin");
    let r = rule(g, "R-DEC");
    assert_eq!(
        r.evidence,
        "avg(t_decode/finalVisual)=40.0% over 1 navs (threshold 40%)"
    );
    assert_eq!(r.triggered, Some(true));
}

// C#: PerfAnalyze.AverageDecodeMs (InflightJoin navs feed the R-CONT decode averages)
#[test]
fn inflight_join_navs_feed_the_r_cont_decode_averages() {
    let root = TempRoot::new("perf-analyze-inflight-cont");
    for (workers, decode) in [(1, "10.0"), (4, "20.0")] {
        write(
            root.path(),
            &format!("w{workers}/perf-1.csv"),
            inflight_csv(decode).as_bytes(),
        );
        write(
            root.path(),
            &format!("w{workers}/session.json"),
            format!(r#"{{"scenario":"S2","mode":"Preview","condition":"warm","preloadWorkers":{workers}}}"#).as_bytes(),
        );
    }
    let result = run(root.path(), None).expect("run");
    assert_eq!(result.groups.len(), 2);
    for g in &result.groups {
        let r = rule(g, "R-CONT");
        assert_eq!(
            r.evidence,
            "t_decode(workers=4)=20.0ms vs t_decode(workers=1)=10.0ms, ratio=2.00 (threshold 1.3)"
        );
        assert_eq!(r.triggered, Some(true));
    }
}

/// One complete nav whose ShowStart text (the nav's mode) is `mode`.
fn mode_csv(mode: &str) -> String {
    format!("{HEADER}1,1000,1,ShowStart,1,,0,,,,{mode}\n2,1500,1,Presented,1,,1,,,,final\n")
}

fn key_order(result: &AnalysisResult) -> Vec<(String, String, String, Option<i32>)> {
    result
        .groups
        .iter()
        .map(|g| {
            let k = &g.summary.key;
            (k.scenario.clone(), k.mode.clone(), k.cond.clone(), k.preload_workers)
        })
        .collect()
}

// C#: PerfAnalyze.RunAsync (`.ThenBy(k => k.Mode, StringComparer.Ordinal)`): groups tying on scenario differ only by mode
#[test]
fn groups_tied_on_scenario_are_ordered_by_mode_ordinally() {
    let root = TempRoot::new("perf-analyze-mode-order");
    // First-seen order (sorted file paths): Preview, then Full. Only the mode key can reorder them.
    write(root.path(), "a/perf-1.csv", mode_csv("Preview").as_bytes());
    write(root.path(), "b/perf-1.csv", mode_csv("Full").as_bytes());
    for d in ["a", "b"] {
        write(
            root.path(),
            &format!("{d}/session.json"),
            br#"{"scenario":"S2","condition":"warm","preloadWorkers":2}"#,
        );
    }
    let result = run(root.path(), None).expect("run");
    let modes: Vec<String> = key_order(&result).into_iter().map(|k| k.1).collect();
    assert_eq!(modes, ["Full", "Preview"]);
}

// C#: PerfAnalyze.RunAsync (`.ThenBy(k => k.PreloadWorkers ?? -1)`): unknown workers sort before known ones
#[test]
fn group_with_unknown_workers_sorts_before_a_known_worker_group() {
    let root = TempRoot::new("perf-analyze-unknown-workers");
    // First-seen: unknown workers (a), then workers=0 (b). With `-1` the order stays; any larger sentinel flips it.
    write(root.path(), "a/perf-1.csv", mode_csv("Preview").as_bytes());
    write(root.path(), "b/perf-1.csv", mode_csv("Preview").as_bytes());
    write(
        root.path(),
        "a/session.json",
        br#"{"scenario":"S2","condition":"warm"}"#,
    );
    write(
        root.path(),
        "b/session.json",
        br#"{"scenario":"S2","condition":"warm","preloadWorkers":0}"#,
    );
    let result = run(root.path(), None).expect("run");
    let workers: Vec<Option<i32>> = key_order(&result).into_iter().map(|k| k.3).collect();
    assert_eq!(workers, [None, Some(0)]);

    // Reverse first-seen order: known workers first in file order, unknown must still come first.
    let root2 = TempRoot::new("perf-analyze-unknown-workers-rev");
    write(root2.path(), "a/perf-1.csv", mode_csv("Preview").as_bytes());
    write(root2.path(), "b/perf-1.csv", mode_csv("Preview").as_bytes());
    write(
        root2.path(),
        "a/session.json",
        br#"{"scenario":"S2","condition":"warm","preloadWorkers":0}"#,
    );
    write(
        root2.path(),
        "b/session.json",
        br#"{"scenario":"S2","condition":"warm"}"#,
    );
    let result = run(root2.path(), None).expect("run");
    let workers: Vec<Option<i32>> = key_order(&result).into_iter().map(|k| k.3).collect();
    assert_eq!(workers, [None, Some(0)]);
}

// DIVERGENCE: summary.md/summary.json keep the CRLF line endings the C# StringBuilder.AppendLine writes on Windows
// (lead decision D5), and the CSV reader is UTF-8 only (D9: no system code page fallback). Both are pinned by the
// golden `run` cases, which compare the output files byte for byte.
// DIVERGENCE: an unreadable CSV is reported with Rust's io::Error text inside `InvalidData("Could not analyze ...")`,
// not the .NET exception text (contract open question 2); only the prefix is compared.

// ---- survivors of verifier round 4: wrapping, rounding, empty rules path, R-CONT mode filter, symlinks ----

/// A complete nav `i` (1 ms qpc tick = 1 us): ShowStart, a Lookup with the given text, Presented 10 ms later.
fn lookup_nav(i: i64, lookup_text: &str) -> String {
    let base = 1_000_000 * i;
    format!(
        "{base},{base},1,ShowStart,{i},,0,,,,Preview\n{b1},{b1},1,Lookup,{i},,,,,,{lookup_text}\n{b2},{b2},1,Presented,{i},,1,,,,final\n",
        b1 = base + 100,
        b2 = base + 10_000
    )
}

// C#: PerfAnalyze.RunAsync (catch IOException -> InvalidDataException("Could not analyze '...': ..."))
#[cfg(windows)]
#[test]
fn unreadable_csv_is_wrapped_as_invalid_data_could_not_analyze() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = TempRoot::new("perf-analyze-locked");
    write(root.path(), "perf-1.csv", format!("{HEADER}{}", one_nav(1)).as_bytes());
    // An exclusive handle (share mode 0) makes csv::read fail with a sharing violation (an io::Error).
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.path().join("perf-1.csv"));
    let Ok(_lock) = lock else {
        println!("SKIP unreadable_csv_is_wrapped: cannot open the file exclusively");
        return;
    };
    let err = run(root.path(), None).expect_err("must fail");
    match err {
        PerfError::InvalidData(m) => {
            assert!(m.starts_with("Could not analyze '"), "{m}");
            assert!(m.contains("perf-1.csv"), "{m}");
        }
        other => panic!("expected InvalidData, got {other:?}"),
    }
}

// C#: PerfAnalyze.EvaluateAllRules (`100.0 * (complete - ramHit) / complete`: multiply first)
#[test]
fn non_ram_hit_share_multiplies_before_dividing() {
    let root = TempRoot::new("perf-analyze-share");
    let mut csv = String::from(HEADER_1MHZ);
    for i in 1..=50 {
        csv.push_str(&lookup_nav(i, if i <= 21 { "ramHit" } else { "inflight" }));
    }
    write(root.path(), "perf-1.csv", csv.as_bytes());
    let rules = TempRoot::new("perf-analyze-share-rules");
    write(rules.path(), "rules.json", br#"{"R-PRE":{"nonRamHitShareS2Pct":58}}"#);

    let result = run(root.path(), Some(&rules.path().join("rules.json"))).expect("run");

    let g = &result.groups[0];
    assert_eq!(g.summary.navs.len(), 50);
    let r = rule(g, "R-PRE");
    // 100 * 29 / 50 is exactly 58; 100 * (29 / 50) is 57.99999999999999.
    assert_eq!(r.triggered, Some(true), "{}", r.evidence);
}

// C#: PerfAnalyze.RunAsync (`string.IsNullOrEmpty(rulesPath)` -> default rules)
#[test]
fn empty_rules_path_behaves_like_no_rules_path() {
    let root = TempRoot::new("perf-analyze-empty-rules");
    write(root.path(), "perf-1.csv", format!("{HEADER}{}", one_nav(1)).as_bytes());
    let with_none = run(root.path(), None).expect("run none");
    let with_empty = run(root.path(), Some(Path::new(""))).expect("run empty");
    assert_eq!(with_none.groups.len(), with_empty.groups.len());
    for (a, b) in with_none.groups.iter().zip(&with_empty.groups) {
        let ra: Vec<_> = a.rules.iter().map(|r| (r.triggered, r.evidence.clone())).collect();
        let rb: Vec<_> = b.rules.iter().map(|r| (r.triggered, r.evidence.clone())).collect();
        assert_eq!(ra, rb);
    }
}

/// One complete InflightJoin nav of the given mode (ShowStart text) with a decode of `decode_ms`.
fn inflight_mode_csv(mode: &str, decode_ms: &str) -> String {
    format!(
        "{HEADER_1MHZ}1,100000,1,ShowStart,1,,0,,,,{mode}\n1,100100,1,Lookup,1,,,,,,inflight\n\
         1,100150,4,Decode,1,,{decode_ms},,,,\n1,200000,1,Presented,1,,1,,,,final\n"
    )
}

// C#: PerfAnalyze.EvaluateAllRules (R-CONT siblings: same Scenario AND Mode AND Cond)
#[test]
fn r_cont_does_not_pool_groups_of_different_mode() {
    let root = TempRoot::new("perf-analyze-cont-mode");
    for (dir, mode, workers, decode) in [("a", "Preview", 1, "10.0"), ("b", "Full", 4, "40.0")] {
        write(
            root.path(),
            &format!("{dir}/perf-1.csv"),
            inflight_mode_csv(mode, decode).as_bytes(),
        );
        write(
            root.path(),
            &format!("{dir}/session.json"),
            format!(r#"{{"scenario":"S2","condition":"warm","preloadWorkers":{workers}}}"#).as_bytes(),
        );
    }
    let result = run(root.path(), None).expect("run");
    assert_eq!(result.groups.len(), 2);
    let modes: Vec<&str> = result.groups.iter().map(|g| g.summary.key.mode.as_str()).collect();
    assert_eq!(modes, ["Full", "Preview"]);
    for g in &result.groups {
        // Each group is its own only sibling: equal workers -> nothing to compare.
        assert_eq!(rule(g, "R-CONT").triggered, None, "{}", rule(g, "R-CONT").evidence);
    }
}

#[cfg(windows)]
fn make_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}
#[cfg(windows)]
fn make_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}
#[cfg(unix)]
fn make_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
#[cfg(unix)]
fn make_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Directory junction (`mklink /J`, no privilege needed) on Windows; a directory symlink elsewhere.
#[cfg(windows)]
fn make_junction(target: &Path, link: &Path) -> bool {
    std::process::Command::new("cmd")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
#[cfg(not(windows))]
fn make_junction(target: &Path, link: &Path) -> bool {
    make_dir_symlink(target, link).is_ok()
}

/// Removes the link itself (never the target contents) so a later recursive delete cannot touch the target.
fn remove_link(link: &Path) {
    let _ = fs::remove_dir(link);
}

// Verified with a real .NET 10 / Windows harness: `Directory.EnumerateFiles(run, "perf-*.csv", AllDirectories)`
//  (a) descends into a directory junction and lists `run\jn\perf-a.csv`;
//  (b) file symlink to a perf-*.csv: could not be probed (mklink without privilege: "not enough privilege"); a file
//      symlink is a file entry, so it is collected (the port resolves the link target type);
//  (c) a directory symlink: same as (a) by the same mechanism (could not be created without privilege);
//  (d) a junction pointing at an ancestor is NOT detected: .NET recurses until paths are ~335k chars long and had not
//      terminated after 40 s (75k results). The port deliberately diverges: the cycle is entered once, see below.
#[test]
fn junction_to_directory_is_descended_like_directory_enumerate_files() {
    let root = TempRoot::new("perf-analyze-junction");
    let outside = TempRoot::new("perf-analyze-junction-outside");
    let csv = format!("{HEADER}{}", one_nav(1));
    write(root.path(), "perf-real.csv", csv.as_bytes());
    write(outside.path(), "d/perf-inside.csv", csv.as_bytes());
    write(outside.path(), "d/other.csv", csv.as_bytes());
    let link = root.path().join("jn");
    if !make_junction(&outside.path().join("d"), &link) {
        println!("SKIP junction test: cannot create a directory junction here");
        return;
    }

    let result = run(root.path(), None);
    remove_link(&link);

    // perf-real.csv + jn/perf-inside.csv (other.csv does not match the pattern).
    assert_eq!(result.expect("run").csv_file_count, 2);
}

#[test]
fn symlinked_csv_file_and_symlinked_directory_are_collected() {
    let root = TempRoot::new("perf-analyze-symlink");
    let outside = TempRoot::new("perf-analyze-symlink-outside");
    let csv = format!("{HEADER}{}", one_nav(1));
    write(root.path(), "perf-real.csv", csv.as_bytes());
    write(outside.path(), "target.csv", csv.as_bytes());
    write(outside.path(), "d/perf-inside.csv", csv.as_bytes());

    if let Err(e) = make_file_symlink(&outside.path().join("target.csv"), &root.path().join("perf-link.csv")) {
        println!("SKIP symlink test: cannot create symlinks here ({e})");
        return;
    }
    let dir_link = root.path().join("linkdir");
    if let Err(e) = make_dir_symlink(&outside.path().join("d"), &dir_link) {
        println!("SKIP symlink test: cannot create directory symlinks here ({e})");
        return;
    }

    let result = run(root.path(), None);
    remove_link(&dir_link);

    // perf-real.csv + the file symlink + linkdir/perf-inside.csv.
    assert_eq!(result.expect("run").csv_file_count, 3);
}

// DIVERGENCE: cycle protection. .NET would recurse through the junction until the path is too long (never ends in
// practice); the port enters each directory at most once per descent chain, so the result is just the real file.
#[test]
fn junction_cycle_to_an_ancestor_terminates_without_duplicates() {
    let root = TempRoot::new("perf-analyze-junction-cycle");
    let csv = format!("{HEADER}{}", one_nav(1));
    write(root.path(), "perf-real.csv", csv.as_bytes());
    write(root.path(), "sub/perf-sub.csv", csv.as_bytes());
    let link = root.path().join("sub").join("back");
    if !make_junction(root.path(), &link) {
        println!("SKIP junction cycle test: cannot create a directory junction here");
        return;
    }

    let result = run(root.path(), None);
    remove_link(&link);

    assert_eq!(result.expect("run").csv_file_count, 2);
}
