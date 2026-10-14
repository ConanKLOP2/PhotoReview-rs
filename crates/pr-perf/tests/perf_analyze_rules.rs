//! Port of the rules part of `tests/PhotoReview.Integration.Tests/PerfAnalyzeTests.cs`, of
//! `PerfAnalyzeReportTests.cs` (R-UI / R-FOLDER evidence), of `PlatformErrorHandlingReviewTests.cs`
//! (`RulesConfig.Load`) and the golden command `perf-rules` (`tests/golden/perf-rules.json`).
//! Unit W1-18. Contract: contracts/W1-18.md.

use pr_perf::csv::PerfError;
use pr_perf::nav::{FolderGenSummary, NavRecord};
use pr_perf::rules::{
    RuleResult, RulesConfig, evaluate_r_cont, evaluate_r_dec, evaluate_r_disk, evaluate_r_folder, evaluate_r_gc,
    evaluate_r_io, evaluate_r_pre, evaluate_r_thread, evaluate_r_ui, fmt_double, fmt_fixed,
};
use pr_test_support::{TempRoot, golden};
use serde_json::Value;

/// A SourceMiss-like nav with a final visual time, a read time and a decode time (C# `NavWithIo`).
fn io_nav(final_visual: f64, read: f64) -> NavRecord {
    NavRecord {
        nav: 1,
        final_visual_ms: Some(final_visual),
        t_read_ms: Some(read),
        t_decode_ms: Some(1.0),
        incomplete: false,
        ..Default::default()
    }
}

fn decode_nav(final_visual: f64, decode: f64) -> NavRecord {
    NavRecord {
        nav: 1,
        final_visual_ms: Some(final_visual),
        t_decode_ms: Some(decode),
        incomplete: false,
        ..Default::default()
    }
}

// C#: PerfAnalyzeTests.RIoTriggersOnHighIoShareAndIsNotTriggeredBelowThreshold
#[test]
fn r_io_triggers_on_high_io_share_and_is_not_triggered_below_threshold() {
    let cfg = RulesConfig::default(); // default threshold 40%
    let high = [io_nav(100.0, 50.0)]; // 50%
    let low = [io_nav(100.0, 10.0)]; // 10%

    assert_eq!(evaluate_r_io(&cfg, &high).triggered, Some(true));
    assert_eq!(evaluate_r_io(&cfg, &low).triggered, Some(false));
}

// C#: PerfAnalyzeTests.RIoIsNotApplicableWithoutReadTiming
#[test]
fn r_io_is_not_applicable_without_read_timing() {
    let cfg = RulesConfig::default();
    let navs = [NavRecord {
        nav: 1,
        final_visual_ms: Some(100.0),
        t_decode_ms: Some(80.0),
        incomplete: false,
        ..Default::default()
    }];

    let result = evaluate_r_io(&cfg, &navs);

    assert_eq!(result.triggered, None);
}

// C#: PerfAnalyzeTests.RDecTriggersOnHighDecodeShareAndIsNotTriggeredBelowThreshold
#[test]
fn r_dec_triggers_on_high_decode_share_and_is_not_triggered_below_threshold() {
    let cfg = RulesConfig::default();
    let high = [decode_nav(100.0, 60.0)];
    let low = [decode_nav(100.0, 10.0)];

    assert_eq!(evaluate_r_dec(&cfg, &high).triggered, Some(true));
    assert_eq!(evaluate_r_dec(&cfg, &low).triggered, Some(false));
}

// C#: PerfAnalyzeTests.RUiTriggersOnHighUiShareOrHighRamHitP95AndIsNotTriggeredOtherwise
#[test]
fn r_ui_triggers_on_high_ui_share_or_high_ram_hit_p95_and_is_not_triggered_otherwise() {
    let cfg = RulesConfig::default();
    let slow_ram_hit = [NavRecord {
        nav: 1,
        final_visual_ms: Some(100.0),
        t_input_ms: Some(30.0),
        t_assign_ms: Some(20.0),
        t_render_ms: Some(10.0),
        incomplete: false,
        ..Default::default()
    }];
    let fast_ram_hit = [NavRecord {
        nav: 1,
        final_visual_ms: Some(100.0),
        t_input_ms: Some(2.0),
        t_assign_ms: Some(2.0),
        t_render_ms: Some(2.0),
        incomplete: false,
        ..Default::default()
    }];

    assert_eq!(evaluate_r_ui(&cfg, &slow_ram_hit, 100.0, None).triggered, Some(true));
    assert_eq!(evaluate_r_ui(&cfg, &fast_ram_hit, 20.0, None).triggered, Some(false));
}

// C#: PerfAnalyzeTests.RUiTriggersOnHighFrameTimeAlone
#[test]
fn r_ui_triggers_on_high_frame_time_alone() {
    let cfg = RulesConfig::default();
    let result = evaluate_r_ui(&cfg, &[], f64::NAN, Some(60.0));
    assert_eq!(result.triggered, Some(true));
}

// C#: PerfAnalyzeTests.RPreTriggersOnFastRamHitWithLowCoverageAndIsNotTriggeredWithGoodCoverage
#[test]
fn r_pre_triggers_on_fast_ram_hit_with_low_coverage_and_is_not_triggered_with_good_coverage() {
    let cfg = RulesConfig::default();
    // S2 (paced) threshold is 10%.
    assert_eq!(evaluate_r_pre(&cfg, 20.0, 25.0, false).triggered, Some(true));
    assert_eq!(evaluate_r_pre(&cfg, 20.0, 2.0, false).triggered, Some(false));
}

// C#: PerfAnalyzeTests.RContTriggersOnDecodeSlowdownAndIsNotTriggeredWhenStable
#[test]
fn r_cont_triggers_on_decode_slowdown_and_is_not_triggered_when_stable() {
    let cfg = RulesConfig::default();
    assert_eq!(
        evaluate_r_cont(&cfg, Some(40.0), Some(80.0), Some(0), Some(8)).triggered,
        Some(true)
    );
    assert_eq!(
        evaluate_r_cont(&cfg, Some(40.0), Some(42.0), Some(0), Some(8)).triggered,
        Some(false)
    );
}

// C#: PerfAnalyzeTests.RContIsNotApplicableWithoutTwoDistinctWorkerRuns
#[test]
fn r_cont_is_not_applicable_without_two_distinct_worker_runs() {
    let cfg = RulesConfig::default();
    let result = evaluate_r_cont(&cfg, None, None, None, None);
    assert_eq!(result.triggered, None);
}

// C#: PerfAnalyzeTests.RThreadTriggersOnDispatcherLongOpsOrHighInputP95AndIsNotTriggeredOtherwise
#[test]
fn r_thread_triggers_on_dispatcher_long_ops_or_high_input_p95_and_is_not_triggered_otherwise() {
    let cfg = RulesConfig::default();
    assert_eq!(evaluate_r_thread(&cfg, 3, Some(5.0)).triggered, Some(true));
    assert_eq!(evaluate_r_thread(&cfg, 0, Some(40.0)).triggered, Some(true));
    assert_eq!(evaluate_r_thread(&cfg, 0, Some(5.0)).triggered, Some(false));
}

// C#: PerfAnalyzeTests.RGcTriggersAboveThresholdAndIsNotTriggeredBelow
#[test]
fn r_gc_triggers_above_threshold_and_is_not_triggered_below() {
    let cfg = RulesConfig::default(); // default 10%
    assert_eq!(evaluate_r_gc(&cfg, Some(15.0)).triggered, Some(true));
    assert_eq!(evaluate_r_gc(&cfg, Some(2.0)).triggered, Some(false));
    assert_eq!(evaluate_r_gc(&cfg, None).triggered, None);
}

// C#: PerfAnalyzeTests.RDiskTriggersWhenDiskCacheIsNoFasterThanReDecodingAndIsNotTriggeredOtherwise
#[test]
fn r_disk_triggers_when_disk_cache_is_no_faster_than_re_decoding_and_is_not_triggered_otherwise() {
    let cfg = RulesConfig::default();
    let disk_slow = [NavRecord {
        nav: 1,
        t_disk_ms: Some(90.0),
        incomplete: false,
        ..Default::default()
    }];
    let disk_fast = [NavRecord {
        nav: 1,
        t_disk_ms: Some(5.0),
        incomplete: false,
        ..Default::default()
    }];
    // read + decode = 50
    let source_miss = [NavRecord {
        nav: 2,
        t_read_ms: Some(10.0),
        t_decode_ms: Some(40.0),
        incomplete: false,
        ..Default::default()
    }];

    assert_eq!(evaluate_r_disk(&cfg, &disk_slow, &source_miss).triggered, Some(true));
    assert_eq!(evaluate_r_disk(&cfg, &disk_fast, &source_miss).triggered, Some(false));
}

// C#: PerfAnalyzeTests.RFolderTriggersOnSlowFolderOpenAndIsNotTriggeredWhenFast
#[test]
fn r_folder_triggers_on_slow_folder_open_and_is_not_triggered_when_fast() {
    let cfg = RulesConfig::default();
    let slow = [FolderGenSummary {
        generation: 1,
        t2_ms: Some(1500.0),
        ..Default::default()
    }];
    let fast = [FolderGenSummary {
        generation: 1,
        t2_ms: Some(200.0),
        ..Default::default()
    }];

    assert_eq!(evaluate_r_folder(&cfg, &slow).triggered, Some(true));
    assert_eq!(evaluate_r_folder(&cfg, &fast).triggered, Some(false));
}

// C#: PerfAnalyzeTests.RulesConfigOverridesDefaultThresholds
#[test]
fn rules_config_overrides_default_thresholds() {
    let root = TempRoot::new("rules-override");
    let path = root.join("rules.json");
    std::fs::write(&path, r#"{"R-GC":{"gcTimePct":50}}"#).expect("write rules file");
    let cfg = RulesConfig::load(&path).expect("load rules file");

    // 30% would trigger against the default 10% threshold, but not against the overridden 50%.
    assert_eq!(evaluate_r_gc(&cfg, Some(30.0)).triggered, Some(false));
    assert_eq!(evaluate_r_gc(&cfg, Some(60.0)).triggered, Some(true));
}

// C#: PerfAnalyzeReportTests.RUiEvidenceShowsNaForAnAbsentRamHitP95
#[test]
fn r_ui_evidence_shows_na_for_an_absent_ram_hit_p95() {
    let r = evaluate_r_ui(&RulesConfig::default(), &[], f64::NAN, Some(5.0));

    assert!(!r.evidence.contains("NaN"), "evidence: {}", r.evidence);
    assert!(r.evidence.contains("N/A"), "evidence: {}", r.evidence);
}

// C#: PerfAnalyzeReportTests.RFolderEvidenceShowsNaForMissingT1AndT3
#[test]
fn r_folder_evidence_shows_na_for_missing_t1_and_t3() {
    let gens = [FolderGenSummary {
        generation: 3,
        t2_ms: Some(2500.0),
        ..Default::default()
    }];

    let r = evaluate_r_folder(&RulesConfig::default(), &gens);

    assert_eq!(r.triggered, Some(true));
    assert!(r.evidence.contains("T1 catalogReady=N/A"), "evidence: {}", r.evidence);
    assert!(r.evidence.contains("=N/A;"), "evidence: {}", r.evidence);
}

// C#: PlatformErrorHandlingReviewTests.RulesConfig_Load_NullSectionAndMalformed
#[test]
fn rules_config_load_null_section_and_malformed() {
    let root = TempRoot::new("rules-null-section");
    let ok = root.join("ok.json");
    std::fs::write(&ok, r#"{"R-X": null, "R-Y": {"k": 5}}"#).expect("write ok.json");
    let rules = RulesConfig::load(&ok).expect("a null section means all defaults");
    assert_eq!(rules.get("R-X", "k", 7.0), 7.0);
    assert_eq!(rules.get("R-Y", "k", 7.0), 5.0);

    let bad = root.join("bad.json");
    std::fs::write(&bad, "{ not json").expect("write bad.json");
    match RulesConfig::load(&bad) {
        Err(PerfError::InvalidData(msg)) => assert!(msg.contains("bad.json"), "message: {msg}"),
        other => panic!("expected InvalidData, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Golden `perf-rules` (tests/golden/perf-rules.json, generated by tools/oracle-cs Commands/Perf.cs `Rules`).
// Kinds: evaluate (106), get (15), load_error (19), fmt (69). Every case is checked.

/// The .NET double text the golden uses for NaN / Infinity, and any other text parsed as a double.
fn parse_double(text: &str) -> f64 {
    match text {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        other => other
            .parse::<f64>()
            .unwrap_or_else(|e| panic!("not a double {other:?}: {e}")),
    }
}

/// A JSON number, or a string with .NET double text (`"NaN"`).
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("number out of range: {n}")),
        Value::String(s) => parse_double(s),
        other => panic!("not a number: {other}"),
    }
}

fn num_req(case: &Value, key: &str) -> f64 {
    match case.get(key) {
        Some(v) => num(v),
        None => panic!("case has no '{key}': {case}"),
    }
}

fn num_opt(case: &Value, key: &str) -> Option<f64> {
    match case.get(key) {
        None | Some(Value::Null) => None,
        Some(v) => Some(num(v)),
    }
}

fn int_opt(case: &Value, key: &str) -> Option<i32> {
    match case.get(key) {
        None | Some(Value::Null) => None,
        Some(v) => {
            Some(i32::try_from(v.as_i64().unwrap_or_else(|| panic!("'{key}' not an int: {v}"))).expect("int range"))
        }
    }
}

fn int_req(case: &Value, key: &str) -> i32 {
    int_opt(case, key).unwrap_or_else(|| panic!("case has no int '{key}': {case}"))
}

fn bool_req(case: &Value, key: &str) -> bool {
    case.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("case has no bool '{key}': {case}"))
}

fn str_req<'a>(case: &'a Value, key: &str) -> &'a str {
    case.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("case has no string '{key}': {case}"))
}

fn array<'a>(case: &'a Value, key: &str) -> &'a [Value] {
    case.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// A nav spec as the oracle reads it (`NavFromJson`): every field the rules read, the rest left at C# defaults.
fn nav_from_json(spec: &Value) -> NavRecord {
    let opt = |key: &str| num_opt(spec, key);
    NavRecord {
        nav: spec.get("nav").and_then(Value::as_i64).unwrap_or(1),
        t_input_ms: opt("t_input"),
        t_read_ms: opt("t_read"),
        t_decode_ms: opt("t_decode"),
        t_assign_ms: opt("t_assign"),
        t_render_ms: opt("t_render"),
        t_disk_ms: opt("t_disk"),
        final_visual_ms: opt("final_visual"),
        kind: spec.get("kind").and_then(Value::as_str).unwrap_or("Unknown").to_owned(),
        incomplete: spec.get("incomplete").and_then(Value::as_bool).unwrap_or(false),
        ..Default::default()
    }
}

fn navs(case: &Value, key: &str) -> Vec<NavRecord> {
    array(case, key).iter().map(nav_from_json).collect()
}

fn gen_from_json(spec: &Value) -> FolderGenSummary {
    FolderGenSummary {
        generation: spec.get("gen").and_then(Value::as_i64).unwrap_or(1),
        t1_catalog_ready_ms: num_opt(spec, "t1"),
        t2_ms: num_opt(spec, "t2"),
        t3_ms: num_opt(spec, "t3"),
        t3_phase: spec.get("t3_phase").and_then(Value::as_str).unwrap_or("").to_owned(),
    }
}

/// The case's `rules_json` text written to a temp file and loaded (C# `Cfg`); absent or null means the defaults.
fn cfg_for(case: &Value) -> RulesConfig {
    match case.get("rules_json") {
        None | Some(Value::Null) => RulesConfig::default(),
        Some(Value::String(text)) => {
            let root = TempRoot::new("perf-rules-golden");
            let path = root.join("rules.json");
            std::fs::write(&path, text).expect("write rules.json");
            RulesConfig::load(&path).unwrap_or_else(|e| panic!("load {text:?}: {e}"))
        }
        Some(other) => panic!("rules_json is not text: {other}"),
    }
}

fn evaluate(case: &Value, cfg: &RulesConfig) -> RuleResult {
    match str_req(case, "rule") {
        "R-IO" => evaluate_r_io(cfg, &navs(case, "navs")),
        "R-DEC" => evaluate_r_dec(cfg, &navs(case, "navs")),
        "R-UI" => evaluate_r_ui(
            cfg,
            &navs(case, "navs"),
            num_req(case, "ram_hit_final_p95"),
            num_opt(case, "frame_time_p95"),
        ),
        "R-PRE" => evaluate_r_pre(
            cfg,
            num_req(case, "ram_hit_final_p95"),
            num_req(case, "non_ram_hit_share_pct"),
            bool_req(case, "is_burst"),
        ),
        "R-CONT" => evaluate_r_cont(
            cfg,
            num_opt(case, "low_decode"),
            num_opt(case, "high_decode"),
            int_opt(case, "low_workers"),
            int_opt(case, "high_workers"),
        ),
        "R-THREAD" => evaluate_r_thread(
            cfg,
            int_req(case, "dispatcher_long_op_count"),
            num_opt(case, "t_input_p95"),
        ),
        "R-GC" => evaluate_r_gc(cfg, num_opt(case, "gc_time_percent")),
        "R-DISK" => evaluate_r_disk(cfg, &navs(case, "disk_navs"), &navs(case, "source_miss_navs")),
        "R-FOLDER" => {
            let gens: Vec<FolderGenSummary> = array(case, "gens").iter().map(gen_from_json).collect();
            evaluate_r_folder(cfg, &gens)
        }
        other => panic!("unknown rule in golden: {other}"),
    }
}

fn check_evaluate(case: &Value) {
    let cfg = cfg_for(case);
    let got = evaluate(case, &cfg);
    let expected_triggered = match &case["triggered"] {
        Value::Null => None,
        Value::Bool(b) => Some(*b),
        other => panic!("bad triggered {other}: {case}"),
    };
    assert_eq!(got.rule, str_req(case, "result_rule"), "rule in {case}");
    assert_eq!(got.triggered, expected_triggered, "triggered in {case}");
    assert_eq!(got.evidence, str_req(case, "evidence"), "evidence in {case}");
    assert_eq!(got.note.as_deref(), case["note"].as_str(), "note in {case}");
}

fn check_get(case: &Value) {
    let cfg = cfg_for(case);
    let got = cfg.get(str_req(case, "rule"), str_req(case, "key"), num_req(case, "default"));
    let expected = num_req(case, "value");
    assert_eq!(got, expected, "get in {case}");
}

fn check_load_error(case: &Value) {
    let root = TempRoot::new("perf-rules-load-error");
    let path = root.join("rules.json");
    let text = str_req(case, "rules_json");
    std::fs::write(&path, text).expect("write rules.json");
    let result = RulesConfig::load(&path);
    match str_req(case, "error_type") {
        "InvalidDataException" => {
            let prefix = format!("Invalid rules file '{}': ", path.display());
            match result {
                Err(PerfError::InvalidData(msg)) => {
                    assert_eq!(
                        msg.starts_with(&prefix),
                        bool_req(case, "message_has_path_prefix"),
                        "message {msg:?} in {case}"
                    );
                }
                other => panic!("expected InvalidData in {case}, got {other:?}"),
            }
        }
        "none" => assert!(result.is_ok(), "expected Ok in {case}, got {result:?}"),
        other => panic!("unknown error_type {other} in {case}"),
    }
}

fn check_fmt(case: &Value) {
    let v = parse_double(str_req(case, "text"));
    assert_eq!(fmt_fixed(v, 1), str_req(case, "f1"), "F1 in {case}");
    assert_eq!(fmt_fixed(v, 2), str_req(case, "f2"), "F2 in {case}");
    assert_eq!(fmt_double(v), str_req(case, "default"), "default in {case}");
}

// Golden command `perf-rules` (W1-18): every case of all four kinds.
#[test]
fn perf_rules_golden_checks_every_case() {
    let golden = golden::load("perf-rules");
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    for case in &golden.cases {
        let kind = str_req(case, "kind").to_owned();
        match kind.as_str() {
            "evaluate" => check_evaluate(case),
            "get" => check_get(case),
            "load_error" => check_load_error(case),
            "fmt" => check_fmt(case),
            other => panic!("unknown golden kind {other}: {case}"),
        }
        *kinds.entry(kind).or_default() += 1;
    }
    assert_eq!(kinds.values().sum::<usize>(), golden.cases.len());
}
