//! Golden drivers (W8-16a, test porter) for the three `pr_platform_win::shell_viewing` units with no UI dependency:
//! `tests/golden/frame-stats.json` (12 cases), `tests/golden/frame-pacer.json` (13 cases) and `tests/golden/frame-log.json`
//! (10 cases). The `scaling-policy` golden is driven from `pr-app/tests/shell_viewing_scaling.rs` (ScalingPolicy lives there).
//!
//! The goldens hold the expected values; the inputs are in `tests/golden-inputs/<command>.json`, joined to the golden by
//! case `name`. Every case is run; the per-kind counts are asserted, so a short golden fails loudly.
//!
//! Number convention: a JSON number, or the strings "NaN", "Infinity", "-Infinity". Doubles are compared exactly (bit
//! for bit, the oracle writes the shortest round-trip form). Frame times are converted to ticks with the case's
//! `ticks_per_second` and `(ms * freq as f64 / 1000.0) as i64`, the same arithmetic as the oracle's `W816aTicks`.
//!
//! Every test here fails through `todo!()` until the bodies land. A failure message names the case.

use std::cell::RefCell;
use std::collections::HashMap;

use pr_platform_win::shell_viewing::frame_log::{CSHARP_ENV_VARIABLE, ENV_VARIABLE, FrameLog};
use pr_platform_win::shell_viewing::frame_stats::{FrameStats, SlowFrame};
use pr_platform_win::shell_viewing::pacer::{FramePacer, PresentMode};
use pr_test_support::golden::{self, bool_of, i64_of, repo_root, str_of};
use serde_json::Value;

/// `tests/golden-inputs/<command>.json`, keyed by case name.
fn inputs(command: &str) -> HashMap<String, Value> {
    let path = repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{command}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: no cases", path.display()))
        .iter()
        .map(|case| (str_of(case, "name").to_owned(), case.clone()))
        .collect()
}

fn input<'a>(inputs: &'a HashMap<String, Value>, name: &str) -> &'a Value {
    inputs
        .get(name)
        .unwrap_or_else(|| panic!("no golden input named '{name}'"))
}

/// A golden number: a JSON number, or one of the three oracle spellings for non-finite values.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a float: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => other
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("not a golden number {other:?}: {e}")),
        },
        other => panic!("not a number: {other}"),
    }
}

/// `value[key]` as a golden number.
fn num_of(value: &Value, key: &str) -> f64 {
    num(&value[key])
}

/// `value[key]` as an i32 (C# `int`).
fn i32_of(value: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(value, key)).unwrap_or_else(|_| panic!("'{key}' does not fit in i32: {value}"))
}

/// C# `Ms(ms)` of the oracle: `(long)(ms * Stopwatch.Frequency / 1000.0)` with the case's frequency.
fn ticks(ms: f64, ticks_per_second: i64) -> i64 {
    (ms * ticks_per_second as f64 / 1000.0) as i64
}

/// `case[key]` as a JSON array.
fn array_of<'a>(value: &'a Value, key: &str) -> &'a Vec<Value> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("case has no array '{key}': {value}"))
}

/// Exact equality of two golden doubles (a bit-level match, except that NaN matches NaN).
fn exact(actual: f64, expected: f64, what: &str) {
    let same = (actual.is_nan() && expected.is_nan()) || actual.to_bits() == expected.to_bits();
    assert!(same, "{what}: actual {actual} != expected {expected}");
}

// ---------------------------------------------------------------------------------------------------------------------
// frame-stats
// ---------------------------------------------------------------------------------------------------------------------

/// Checks the summary keys every frame-stats case carries (C# `W816aFrameSummary`).
fn check_summary(name: &str, stats: &FrameStats, case: &Value) {
    assert_eq!(stats.count() as i64, i64_of(case, "count"), "{name}: count");
    assert_eq!(stats.total(), i64_of(case, "total"), "{name}: total");
    assert_eq!(
        stats.blocked_presents(),
        i64_of(case, "blocked_presents"),
        "{name}: blocked_presents"
    );
    exact(
        stats.frame_ms(50.0),
        num_of(case, "frame_p50"),
        &format!("{name}: frame_p50"),
    );
    exact(
        stats.frame_ms(95.0),
        num_of(case, "frame_p95"),
        &format!("{name}: frame_p95"),
    );
    exact(
        stats.frame_ms(100.0),
        num_of(case, "frame_p100"),
        &format!("{name}: frame_p100"),
    );
    exact(
        stats.present_ms(50.0),
        num_of(case, "present_p50"),
        &format!("{name}: present_p50"),
    );
    exact(
        stats.max_present_ms(),
        num_of(case, "max_present_ms"),
        &format!("{name}: max_present_ms"),
    );
}

#[test]
fn golden_frame_stats() {
    let golden = golden::load("frame-stats");
    assert_eq!(golden.cases.len(), 12, "frame-stats golden case count");
    let inputs = inputs("frame-stats");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let spec = input(&inputs, name);
        let freq = i64_of(case, "ticks_per_second");
        let kind = str_of(case, "kind");
        assert_eq!(kind, str_of(spec, "kind"), "{name}: kind differs from the input");

        match kind {
            "series" => {
                let n = i32_of(spec, "n");
                let mut stats = FrameStats::new(freq);
                for i in 1..=n {
                    let i = f64::from(i);
                    stats.record(ticks(i, freq), ticks(i / 2.0, freq));
                }
                check_summary(name, &stats, case);
            }
            "empty" => {
                let stats = FrameStats::new(freq);
                check_summary(name, &stats, case);
            }
            "samples" => {
                let mut stats = FrameStats::new(freq);
                for pair in array_of(spec, "samples") {
                    let pair = pair
                        .as_array()
                        .unwrap_or_else(|| panic!("{name}: sample is not a pair"));
                    stats.record(ticks(num(&pair[0]), freq), ticks(num(&pair[1]), freq));
                }
                check_summary(name, &stats, case);
            }
            "ring" => {
                let first_ms = num_of(spec, "first_ms");
                let first_count = i64_of(spec, "first_count");
                let rest_ms = num_of(spec, "rest_ms");
                let total = i64_of(spec, "total");
                let present_ms = num_of(spec, "present_ms");
                let mut stats = FrameStats::new(freq);
                for i in 0..total {
                    let frame_ms = if i < first_count { first_ms } else { rest_ms };
                    stats.record(ticks(frame_ms, freq), ticks(present_ms, freq));
                }
                check_summary(name, &stats, case);
            }
            "reset_keeps_slow" => {
                let mut stats = FrameStats::new(freq);
                for _ in 0..5 {
                    stats.record(ticks(5.0, freq), ticks(5.0, freq));
                }
                stats.record_slow(500.0, 1.0, 480.0, 1.0, "kept");
                stats.reset();
                check_summary(name, &stats, case);
                assert_eq!(
                    stats.slow().len() as i64,
                    i64_of(case, "slow_count"),
                    "{name}: slow_count"
                );
            }
            "is_slow" => {
                let slow = FrameStats::is_slow(num_of(spec, "end_draw_ms"), num_of(spec, "present_ms"));
                assert_eq!(slow, bool_of(case, "slow"), "{name}: is_slow");
            }
            "slow_bounded" => {
                let mut stats = FrameStats::new(freq);
                for entry in array_of(spec, "entries") {
                    record_slow(&mut stats, entry);
                }
                let repeat = &spec["repeat"];
                for _ in 0..i64_of(repeat, "count") {
                    record_slow(&mut stats, &repeat["entry"]);
                }
                assert_eq!(
                    stats.slow().len() as i64,
                    i64_of(case, "slow_count"),
                    "{name}: slow_count"
                );
                let notes: Vec<&str> = stats.slow().iter().map(|slow: &SlowFrame| slow.note.as_str()).collect();
                let expected: Vec<&str> = array_of(case, "slow_notes")
                    .iter()
                    .map(|v| v.as_str().unwrap_or(""))
                    .collect();
                assert_eq!(notes, expected, "{name}: slow_notes");
            }
            other => panic!("{name}: unknown frame-stats kind {other:?}"),
        }
    }
}

fn record_slow(stats: &mut FrameStats, entry: &Value) {
    stats.record_slow(
        num_of(entry, "frame_ms"),
        num_of(entry, "draw_ms"),
        num_of(entry, "end_draw_ms"),
        num_of(entry, "present_ms"),
        str_of(entry, "note"),
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// frame-pacer
// ---------------------------------------------------------------------------------------------------------------------

fn mode_named(name: &str) -> PresentMode {
    match name {
        "Immediate" => PresentMode::Immediate,
        "VSync" => PresentMode::VSync,
        other => panic!("unknown present mode {other:?}"),
    }
}

fn mode_name(mode: PresentMode) -> &'static str {
    match mode {
        PresentMode::Immediate => "Immediate",
        PresentMode::VSync => "VSync",
    }
}

#[test]
fn golden_frame_pacer() {
    let golden = golden::load("frame-pacer");
    assert_eq!(golden.cases.len(), 13, "frame-pacer golden case count");
    let inputs = inputs("frame-pacer");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let spec = input(&inputs, name);
        let kind = str_of(case, "kind");
        assert_eq!(kind, str_of(spec, "kind"), "{name}: kind differs from the input");

        match kind {
            "mode_for" => {
                let mode = FramePacer::new().mode_for(bool_of(spec, "fullscreen"), bool_of(spec, "awaiting"));
                assert_eq!(mode_name(mode), str_of(case, "mode"), "{name}: mode");
            }
            "defer" => {
                let mut pacer = FramePacer::new();
                let flags: Vec<bool> = array_of(spec, "ready")
                    .iter()
                    .map(|ready| {
                        pacer.should_defer(ready.as_bool().unwrap_or_else(|| panic!("{name}: ready is not a bool")))
                    })
                    .collect();
                let expected: Vec<bool> = array_of(case, "deferred")
                    .iter()
                    .map(|v| v.as_bool().unwrap_or(false))
                    .collect();
                assert_eq!(flags, expected, "{name}: deferred");
                assert_eq!(
                    pacer.deferred_total(),
                    i64_of(case, "deferred_total"),
                    "{name}: deferred_total"
                );
            }
            "observe" => {
                let mut pacer = FramePacer::new();
                let mut raised_total = 0_i64;
                let mut raised_flags = Vec::new();
                for step in array_of(spec, "steps") {
                    let mode = mode_named(str_of(step, "mode"));
                    let raised = pacer.observe_present(mode, num_of(step, "present_ms"));
                    raised_total += i64::from(raised);
                    raised_flags.push(raised);
                }
                let expected: Vec<bool> = array_of(case, "raised")
                    .iter()
                    .map(|v| v.as_bool().unwrap_or(false))
                    .collect();
                assert_eq!(raised_flags, expected, "{name}: raised per step");
                assert_eq!(raised_total, i64_of(case, "raised_total"), "{name}: raised_total");
                assert_eq!(pacer.is_degraded(), bool_of(case, "degraded"), "{name}: degraded");
                let after = &spec["after"];
                let mode = pacer.mode_for(bool_of(after, "fullscreen"), bool_of(after, "awaiting"));
                assert_eq!(mode_name(mode), str_of(case, "mode_after"), "{name}: mode_after");
            }
            other => panic!("{name}: unknown frame-pacer kind {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// frame-log
// ---------------------------------------------------------------------------------------------------------------------

/// A unique path under the temp dir, removed on drop (the oracle deletes its file in a `finally`, so does this test).
struct TempLog(std::path::PathBuf);

impl Drop for TempLog {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn golden_frame_log() {
    let golden = golden::load("frame-log");
    assert_eq!(golden.cases.len(), 10, "frame-log golden case count");
    let inputs = inputs("frame-log");

    for case in &golden.cases {
        let name = str_of(case, "name");
        let spec = input(&inputs, name);
        let kind = str_of(case, "kind");
        assert_eq!(kind, str_of(spec, "kind"), "{name}: kind differs from the input");

        match kind {
            "format" => {
                let line = FrameLog::format(
                    num_of(spec, "gap_ms"),
                    num_of(spec, "frame_ms"),
                    num_of(spec, "end_draw_ms"),
                    num_of(spec, "present_ms"),
                );
                assert_eq!(line, str_of(case, "line"), "{name}: line");
            }
            "env" => {
                let value = spec["value"].as_str().map(str::to_owned);
                let asked = RefCell::new(String::new());
                let log = FrameLog::from_environment(
                    |variable| {
                        *asked.borrow_mut() = variable.to_owned();
                        value.clone()
                    },
                    10_000_000,
                );
                assert_eq!(
                    asked.borrow().as_str(),
                    ENV_VARIABLE,
                    "{name}: the Rust code asks its own variable"
                );
                assert_eq!(
                    CSHARP_ENV_VARIABLE,
                    str_of(case, "asked"),
                    "{name}: the golden's C# variable name"
                );
                assert_eq!(log.is_none(), bool_of(case, "is_null"), "{name}: is_null");
            }
            "record" => {
                let freq = i64_of(case, "ticks_per_second");
                let temp = TempLog(std::env::temp_dir().join(format!(
                    "pr-w8-16a-golden-{}-{}.log",
                    std::process::id(),
                    unique_suffix()
                )));
                {
                    let mut log = FrameLog::open(&temp.0, freq).expect("open the golden frame log");
                    for frame in array_of(spec, "frames") {
                        log.record(
                            ticks(num_of(frame, "start_ms"), freq),
                            ticks(num_of(frame, "end_ms"), freq),
                            num_of(frame, "end_draw_ms"),
                            num_of(frame, "present_ms"),
                        );
                    }
                    log.close();
                }
                let text = std::fs::read_to_string(&temp.0).expect("read the golden frame log");
                let expected: Vec<&str> = array_of(case, "lines")
                    .iter()
                    .map(|v| v.as_str().unwrap_or(""))
                    .collect();
                assert_eq!(text.lines().collect::<Vec<_>>(), expected, "{name}: lines");
                let raw: String = expected.iter().map(|line| format!("{line}\r\n")).collect();
                assert_eq!(text, raw, "{name}: raw bytes must be CRLF-terminated lines");
            }
            other => panic!("{name}: unknown frame-log kind {other:?}"),
        }
    }
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0)
}
