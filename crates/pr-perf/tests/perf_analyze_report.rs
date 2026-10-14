//! W1-20: tests for `pr_perf::report` (port of `PerfAnalyzeReport`). The golden `perf-report` pins the full
//! `summary.md` / `summary.json` text produced by the C# code.

use pr_perf::analyze::{AnalysisResult, GroupResult};
use pr_perf::nav::{DispatcherLongOpRow, FolderGenSummary, PreloadItemRow};
use pr_perf::report::{
    NEW_LINE, json_string_literal, render_json, render_markdown, startup_phases, write_json, write_markdown,
};
use pr_perf::rules::RuleResult;
use pr_perf::stats::{GroupKey, GroupSummary};
use pr_test_support::TempRoot;
use pr_test_support::golden;
use serde_json::Value;

fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().expect("finite number"),
        Value::String(s) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => other.parse().expect("number text"),
        },
        other => panic!("not a number: {other}"),
    }
}

fn num_opt(v: &Value) -> Option<f64> {
    if v.is_null() { None } else { Some(num(v)) }
}

fn int_opt(v: &Value) -> Option<i32> {
    if v.is_null() { None } else { Some(int_any(v) as i32) }
}

fn group_from_spec(g: &Value) -> GroupResult {
    let k = &g["key"];
    let key = GroupKey::new(
        k["scenario"].as_str().expect("scenario"),
        k["mode"].as_str().expect("mode"),
        k["cond"].as_str().expect("cond"),
        int_opt(&k["workers"]),
    );
    let mut s = GroupSummary::new(key);
    s.count = g["count"].as_i64().unwrap_or(0) as i32;
    s.incomplete = g["incomplete"].as_i64().unwrap_or(0) as i32;
    if let Some(t) = g["first"].as_object() {
        s.first_p50 = num(&t["p50"]);
        s.first_p95 = num(&t["p95"]);
        s.first_max = num(&t["max"]);
    }
    if let Some(t) = g["final"].as_object() {
        s.final_p50 = num(&t["p50"]);
        s.final_p95 = num(&t["p95"]);
        s.final_max = num(&t["max"]);
    }
    if let Some(t) = g["rendered"].as_object() {
        s.rendered_frame_p50 = num(&t["p50"]);
        s.rendered_frame_p95 = num(&t["p95"]);
        s.rendered_frame_max = num(&t["max"]);
    }
    if !g["hit_rate"].is_null() {
        s.hit_rate = num(&g["hit_rate"]);
    }
    if let Some(kc) = g["kind_counts"].as_array() {
        for p in kc {
            s.kind_counts.insert(
                p[0].as_str().expect("kind").to_owned(),
                p[1].as_i64().expect("n") as i32,
            );
        }
    }
    if let Some(sp) = g["slowest_phase_share"].as_array() {
        for p in sp {
            s.slowest_phase_share
                .insert(p[0].as_str().expect("phase").to_owned(), num(&p[1]));
        }
    }
    s.preload_workers = int_opt(&g["preload_workers"]);
    s.gc_time_percent = num_opt(&g["gc"]);
    s.frame_time_p95_ms = num_opt(&g["frame_p95"]);
    if let Some(items) = g["preload_items"].as_array() {
        for p in items {
            s.preload_items.push(PreloadItemRow {
                slot: p["slot"].as_i64().expect("slot") as i32,
                path_id: p["path_id"].as_str().expect("path_id").to_owned(),
                queue_wait_ms: num(&p["queue_wait"]),
                kind: p["kind"].as_str().expect("kind").to_owned(),
                ms: num(&p["ms"]),
            });
        }
    }
    s.preload_paused_count = g["paused"].as_i64().unwrap_or(0) as i32;
    s.preload_cancel_count = g["cancel"].as_i64().unwrap_or(0) as i32;
    if let Some(ops) = g["dispatcher"].as_array() {
        for p in ops {
            s.dispatcher_long_ops.push(DispatcherLongOpRow {
                ms: num(&p["ms"]),
                priority: p["priority"].as_str().expect("priority").to_owned(),
                name: p["name"].as_str().expect("name").to_owned(),
            });
        }
    }
    s.dispatcher_long_ops_before_start_count = g["dispatcher_before"].as_i64().unwrap_or(0) as i32;
    s.dropped_rows = if g["dropped"].is_null() {
        0
    } else {
        int_any(&g["dropped"])
    };
    if let Some(gens) = g["folder_gens"].as_array() {
        for p in gens {
            s.folder_gens.push(FolderGenSummary {
                generation: int_any(&p["gen"]),
                t1_catalog_ready_ms: num_opt(&p["t1"]),
                t2_ms: num_opt(&p["t2"]),
                t3_ms: num_opt(&p["t3"]),
                t3_phase: p["t3_phase"].as_str().unwrap_or("").to_owned(),
            });
        }
    }
    if let Some(runs) = g["startup_runs"].as_array() {
        for run in runs {
            let mut d = indexmap::IndexMap::new();
            for p in run.as_array().expect("run") {
                d.insert(p[0].as_str().expect("phase").to_owned(), num(&p[1]));
            }
            s.startup_runs.push(d);
        }
    }
    let rules = g["rules"]
        .as_array()
        .map(|ra| {
            ra.iter()
                .map(|p| RuleResult {
                    rule: p["rule"].as_str().expect("rule").to_owned(),
                    triggered: p["triggered"].as_bool(),
                    evidence: p["evidence"].as_str().expect("evidence").to_owned(),
                    note: p["note"].as_str().map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default();
    GroupResult { summary: s, rules }
}

/// serde_json (without `float_roundtrip`) can mis-round a decimal literal by one ulp, which would change the
/// printed digits. The golden inputs are therefore parsed from text where every non-integer-looking number is
/// quoted, and [`num`] parses the string with `f64::from_str` (correctly rounded, like .NET `double.Parse`).
fn load_cases_exact() -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden")
        .join("perf-report.json");
    let text = std::fs::read_to_string(&path).expect("read golden");
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 1024);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '\\' {
                    i += 1;
                    if i < chars.len() {
                        out.push(chars[i]);
                    }
                } else if chars[i] == '"' {
                    break;
                }
                i += 1;
            }
            i += 1;
        } else if c == '-' || c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || matches!(chars[i], '-' | '+' | '.' | 'e' | 'E')) {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if token.contains(['.', 'e', 'E']) || token.len() > 15 {
                out.push('"');
                out.push_str(&token);
                out.push('"');
            } else {
                out.push_str(&token);
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    let doc: Value = serde_json::from_str(&out).expect("parse golden");
    doc["cases"].as_array().expect("cases").clone()
}

fn assert_text_eq(actual: &str, expected: &str, what: &str) {
    if actual == expected {
        return;
    }
    let a: Vec<&str> = actual.split("\r\n").collect();
    let e: Vec<&str> = expected.split("\r\n").collect();
    for i in 0..a.len().max(e.len()) {
        if a.get(i) != e.get(i) {
            panic!(
                "{what}: first difference at line {}\n  actual:   {:?}\n  expected: {:?}",
                i + 1,
                a.get(i),
                e.get(i)
            );
        }
    }
    panic!("{what}: texts differ (line endings or length)");
}

fn result_of(groups: Vec<GroupResult>, csv_file_count: i32) -> AnalysisResult {
    AnalysisResult {
        groups,
        csv_file_count,
        ..Default::default()
    }
}

fn summary(scenario: &str, mode: &str, cond: &str, workers: Option<i32>) -> GroupSummary {
    GroupSummary::new(GroupKey::new(scenario, mode, cond, workers))
}

fn group(s: GroupSummary, rules: Vec<RuleResult>) -> GroupResult {
    GroupResult { summary: s, rules }
}

#[test]
fn golden_perf_report_report_cases_match_csharp_text_byte_for_byte() {
    let cases = load_cases_exact();
    let mut checked = 0;
    for case in cases.iter().filter(|c| c["kind"] == "report") {
        let name = golden::str_of(case, "name");
        let groups = case["groups"]
            .as_array()
            .expect("groups")
            .iter()
            .map(group_from_spec)
            .collect();
        let csv = case["csv_file_count"].as_i64().unwrap_or(1) as i32;
        let result = result_of(groups, csv);
        let md = golden::str_of(case, "markdown");
        let js = golden::str_of(case, "json");
        assert_text_eq(&render_markdown(&result), md, &format!("{name}: markdown"));
        assert_text_eq(&render_json(&result), js, &format!("{name}: json"));

        let root = TempRoot::new("perf-report");
        let md_path = root.join("summary.md");
        let js_path = root.join("summary.json");
        write_markdown(&md_path, &result).expect("write md");
        write_json(&js_path, &result).expect("write json");
        assert_eq!(
            std::fs::read(&md_path).expect("read md"),
            md.as_bytes(),
            "{name}: md bytes"
        );
        assert_eq!(
            std::fs::read(&js_path).expect("read json"),
            js.as_bytes(),
            "{name}: json bytes"
        );
        checked += 1;
    }
    assert_eq!(checked, 5);
}

#[test]
fn golden_perf_report_json_string_cases_match_csharp_serializer() {
    let g = golden::load("perf-report");
    let mut checked = 0;
    for case in g.cases.iter().filter(|c| c["kind"] == "json_string") {
        let text = golden::str_of(case, "text");
        assert_eq!(
            json_string_literal(text),
            golden::str_of(case, "json"),
            "json_string of {text:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 48);
}

// C#: PerfAnalyzeHardeningTests.SummaryJson_FolderStartupAndRulesKeys_AreCamelCase
#[test]
fn summary_json_folder_startup_and_rules_keys_are_camel_case() {
    let mut s = summary("S1", "Fast", "cold", Some(2));
    s.count = 1;
    s.final_p50 = 10.0;
    s.first_p50 = 5.0;
    s.folder_gens.push(FolderGenSummary {
        generation: 3,
        t1_catalog_ready_ms: Some(1.5),
        t2_ms: Some(2.5),
        t3_ms: Some(3.5),
        t3_phase: "x".to_owned(),
    });
    let mut run = indexmap::IndexMap::new();
    run.insert("phaseA".to_owned(), 12.0);
    s.startup_runs.push(run);
    let result = result_of(
        vec![group(
            s,
            vec![RuleResult::new("R-X", Some(true), "evidence", Some("note"))],
        )],
        1,
    );
    let root = TempRoot::new("perf-report-camel");
    let path = root.join("summary.json");
    write_json(&path, &result).expect("write");

    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
    let grp = &doc["groups"][0];
    let folder = &grp["folder"][0];
    assert_eq!(folder["gen"].as_i64(), Some(3));
    assert_eq!(folder["t1CatalogReadyMs"].as_f64(), Some(1.5));
    assert_eq!(folder["t2Ms"].as_f64(), Some(2.5));
    assert_eq!(folder["t3Ms"].as_f64(), Some(3.5));
    assert_eq!(folder["t3Phase"].as_str(), Some("x"));
    let startup = &grp["startup"][0];
    assert_eq!(startup["phase"].as_str(), Some("phaseA"));
    assert_eq!(startup["median"].as_f64(), Some(12.0));
    assert_eq!(startup["count"].as_i64(), Some(1));
    let rule = &grp["rules"][0];
    assert_eq!(rule["rule"].as_str(), Some("R-X"));
    assert_eq!(rule["triggered"].as_bool(), Some(true));
    assert_eq!(rule["evidence"].as_str(), Some("evidence"));
    assert_eq!(rule["note"].as_str(), Some("note"));
    for name in grp.as_object().expect("object").keys() {
        assert!(name.chars().next().is_some_and(char::is_lowercase), "{name}");
    }
}

// C#: (adapted from PerfAnalyzeHardeningTests.DroppedRowsAreSurfacedInMarkdownAndJson)
#[test]
fn dropped_rows_are_surfaced_in_markdown_and_json() {
    let mut s = summary("S1", "Fast", "warm", Some(1));
    s.dropped_rows = 7;
    let result = result_of(vec![group(s, vec![])], 1);
    let md = render_markdown(&result);
    assert!(md.contains(
        "- WARNING: S1/Fast/warm/workers=1: dropped 7 events (listener queue overflow); percentiles and counts for this group are incomplete\r\n"
    ));
    let doc: Value = serde_json::from_str(&render_json(&result)).expect("parse");
    assert_eq!(doc["groups"][0]["droppedRows"].as_i64(), Some(7));
}

// C#: (adapted from PerfAnalyzeHardeningTests.NoDroppedRowsMeansNoWarning)
#[test]
fn no_dropped_rows_means_no_warning() {
    let result = result_of(vec![group(summary("S1", "Fast", "warm", Some(1)), vec![])], 1);
    assert!(!render_markdown(&result).contains("WARNING"));
}

// C#: (adapted from PerfAnalyzeHardeningTests.PipeInAGroupNameIsEscapedSoTheTableKeepsItsColumns)
#[test]
fn pipe_in_a_group_name_is_escaped_so_the_table_keeps_its_columns() {
    let result = result_of(vec![group(summary("S|1", "M\r\nx", "warm", None), vec![])], 1);
    let md = render_markdown(&result);
    assert!(md.contains("| S\\|1/M  x/warm/workers=? | 0 | 0 |"));
}

// C#: (adapted from PerfAnalyzeHardeningTests.PerfAnalyzeWithOnlyIncompleteNavsStillWritesJson)
#[test]
fn nan_percentiles_are_na_in_markdown_and_null_in_json() {
    let result = result_of(vec![group(summary("S1", "Fast", "warm", Some(2)), vec![])], 1);
    let md = render_markdown(&result);
    assert!(md.contains(
        "| S1/Fast/warm/workers=2 | 0 | 0 | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N<20 |\r\n"
    ));
    let doc: Value = serde_json::from_str(&render_json(&result)).expect("parse");
    let first = &doc["groups"][0]["firstVisualMs"];
    assert!(first["p50"].is_null() && first["p95"].is_null() && first["max"].is_null());
    assert!(doc["groups"][0]["hitRate"].is_null());
    assert!(!render_json(&result).contains("NaN"));
}

#[test]
fn render_uses_crlf_and_markdown_ends_with_newline_json_does_not() {
    let result = result_of(vec![], 0);
    let md = render_markdown(&result);
    let js = render_json(&result);
    assert!(md.ends_with(NEW_LINE));
    assert!(!md.replace("\r\n", "").contains('\n'));
    assert!(!js.ends_with('\n'));
    assert_eq!(js, "{\r\n  \"csvFileCount\": 0,\r\n  \"groups\": []\r\n}");
}

#[test]
fn startup_phases_median_even_odd_and_stable_sort_by_median() {
    let mut s = summary("S", "M", "c", None);
    let run = |pairs: &[(&str, f64)]| {
        let mut d = indexmap::IndexMap::new();
        for (k, v) in pairs {
            d.insert((*k).to_owned(), *v);
        }
        d
    };
    s.startup_runs.push(run(&[("b", 10.0), ("a", 30.0), ("c", 10.0)]));
    s.startup_runs.push(run(&[("a", 10.0), ("b", 30.0)]));
    s.startup_runs.push(run(&[("a", 50.0)]));
    let p = startup_phases(&s);
    let names: Vec<&str> = p.iter().map(|x| x.phase.as_str()).collect();
    // b: median (10+30)/2 = 20, a: sorted 10,30,50 -> 30, c: 10 -> c first.
    assert_eq!(names, ["c", "b", "a"]);
    assert_eq!(p[1].median, 20.0);
    assert_eq!((p[1].min, p[1].max, p[1].count), (10.0, 30.0, 2));
    assert_eq!(p[2].median, 30.0);
    assert_eq!(p[2].count, 3);
}

#[test]
fn json_string_literal_escapes_html_sensitive_and_non_ascii_chars() {
    assert_eq!(
        json_string_literal("a<b>&'\"+`"),
        "\"a\\u003Cb\\u003E\\u0026\\u0027\\u0022\\u002B\\u0060\""
    );
    assert_eq!(json_string_literal("\u{1F600}/\\\n"), "\"\\uD83D\\uDE00/\\\\\\n\"");
}

/// An integer that [`load_cases_exact`] may have turned into a string (long digit runs).
fn int_any(v: &Value) -> i64 {
    match v {
        Value::String(s) => s.parse().expect("integer text"),
        other => other.as_i64().expect("integer"),
    }
}
