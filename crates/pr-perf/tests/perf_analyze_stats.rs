//! Port tests for `pr_perf::stats` (unit W1-17). C# baseline e441d67a: `src/PhotoReview.PerfAnalysis/PerfAnalyzeStats.cs`.
//! Every test names the C# test it ports in a `// C#:` comment. The golden test replays every `perf-stats` case.

use std::panic::{AssertUnwindSafe, catch_unwind};

use indexmap::IndexMap;
use pr_perf::csv::{DiagFlags, PerfCsvFile};
use pr_perf::nav::NavRecord;
use pr_perf::stats::{GroupKey, GroupSummary, RunFileMeta, nearest_rank, phase_getters, slowest_decile_phase_share};
use pr_test_support::{TempRoot, golden};
use serde_json::Value;

// ---------------------------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------------------------

/// C# `Assert.Equal(expected, actual, 6)`: equal after rounding both to 6 decimal places.
fn assert_close6(actual: f64, expected: f64) {
    assert_eq!(
        (actual * 1e6).round(),
        (expected * 1e6).round(),
        "expected {expected}, got {actual}"
    );
}

/// Exact nearest-rank oracle over whole percents: 1-based rank `ceil(P*N/100)`, computed in integers.
fn oracle_rank(percent: i64, count: i64) -> i64 {
    ((percent * count + 99) / 100).max(1)
}

/// Deterministic xorshift64* generator, standing in for the C# `new Random(20260926)` (not reproducible in Rust).
struct XorShift(u64);

impl XorShift {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish integer in `0..bound` (`bound > 0`).
    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

/// A JSON number, or one of the strings `"NaN"`, `"Infinity"`, `"-Infinity"` (golden encoding of non-finite values).
fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => other.parse::<f64>().unwrap_or(f64::NAN),
        },
        other => panic!("not a number: {other}"),
    }
}

/// A golden `value` that may be `null` (`NaN`) or a number. Compares exactly.
fn expect_f64(actual: f64, expected: &Value) -> Result<(), String> {
    if expected.is_null() {
        return if actual.is_nan() {
            Ok(())
        } else {
            Err(format!("expected NaN, got {actual}"))
        };
    }
    let e = num(expected);
    if actual == e || (actual.is_nan() && e.is_nan()) {
        Ok(())
    } else {
        Err(format!("expected {e}, got {actual}"))
    }
}

fn expect_opt_f64(actual: Option<f64>, expected: &Value) -> Result<(), String> {
    match (actual, expected.is_null()) {
        (None, true) => Ok(()),
        (Some(a), false) => expect_f64(a, expected),
        (got, _) => Err(format!("expected {expected}, got {got:?}")),
    }
}

/// `"sorted"` of a `nearest_rank` case: an array of numbers, or `{"range": N}` = `1.0 ..= N.0`.
fn sorted_of(v: &Value) -> Vec<f64> {
    if let Some(range) = v.get("range").and_then(Value::as_u64) {
        return (1..=range).map(|i| i as f64).collect();
    }
    v.as_array()
        .expect("sorted is an array or {range}")
        .iter()
        .map(num)
        .collect()
}

/// One nav of a `decile` / `group_summary` case (C# `NavFromJson`). Every field is optional; `incomplete` defaults to
/// `false` here, unlike `NavRecord::default()`, and `kind` to `"Unknown"`.
fn nav_from_spec(o: &Value) -> NavRecord {
    let f = |key: &str| o.get(key).filter(|v| !v.is_null()).map(num);
    let mut n = NavRecord {
        nav: o.get("nav").and_then(Value::as_i64).unwrap_or(1),
        mode: o.get("mode").and_then(Value::as_str).unwrap_or("").to_owned(),
        t_input_ms: f("t_input"),
        t_pre_ms: f("t_pre"),
        t_stat_ms: f("t_stat"),
        lookup_result: o.get("lookup_result").and_then(Value::as_str).unwrap_or("").to_owned(),
        t_thumb_ms: f("t_thumb"),
        has_thumbnail: o.get("has_thumbnail").and_then(Value::as_bool).unwrap_or(false),
        t_join_ms: f("t_join"),
        t_disk_ms: f("t_disk"),
        t_read_ms: f("t_read"),
        t_decode_ms: f("t_decode"),
        t_verify_ms: f("t_verify"),
        t_assign_ms: f("t_assign"),
        t_render_ms: f("t_render"),
        t_render_frame_ms: f("t_render_frame"),
        first_visual_ms: f("first_visual"),
        final_visual_ms: f("final_visual"),
        incomplete: o.get("incomplete").and_then(Value::as_bool).unwrap_or(false),
        kind: o.get("kind").and_then(Value::as_str).unwrap_or("Unknown").to_owned(),
        ..NavRecord::default()
    };
    if let Some(post) = o.get("post_ms").and_then(Value::as_object) {
        for (k, v) in post {
            n.post_ms.insert(k.clone(), num(v));
        }
    }
    n
}

/// One `seq` navigation list (C# `Navs` with a `seq` object): navs `1..=count`, `final_visual = i * final_step`,
/// optional `first_visual = i * first_step`, `t_render_frame = i * frame_step`, navs `i > incomplete_after` are
/// incomplete, `kind` cycles through `kinds`, and the last `t_decode_last` navs get `t_decode = t_decode_value`.
fn expand_seq(s: &Value) -> Vec<NavRecord> {
    let count = s["count"].as_i64().expect("seq.count");
    let final_step = num(&s["final_step"]);
    let first_step = s.get("first_step").filter(|v| !v.is_null()).map(num);
    let frame_step = s.get("frame_step").filter(|v| !v.is_null()).map(num);
    let incomplete_after = s.get("incomplete_after").and_then(Value::as_i64);
    let kinds: Option<Vec<String>> = s.get("kinds").and_then(Value::as_array).map(|a| {
        a.iter()
            .map(|k| k.as_str().expect("kinds entries are strings").to_owned())
            .collect()
    });
    let decode_last = s.get("t_decode_last").and_then(Value::as_i64).unwrap_or(0);
    let decode_value = s.get("t_decode_value").filter(|v| !v.is_null()).map(num);

    (1..=count)
        .map(|i| {
            let mut n = NavRecord {
                nav: i,
                final_visual_ms: Some(i as f64 * final_step),
                incomplete: incomplete_after.is_some_and(|a| i > a),
                ..NavRecord::default()
            };
            if let Some(f) = first_step {
                n.first_visual_ms = Some(i as f64 * f);
            }
            if let Some(fr) = frame_step {
                n.t_render_frame_ms = Some(i as f64 * fr);
            }
            if let Some(list) = kinds.as_ref().filter(|l| !l.is_empty()) {
                n.kind = list[((i - 1) as usize) % list.len()].clone();
            }
            if i > count - decode_last {
                n.t_decode_ms = decode_value;
            }
            n
        })
        .collect()
}

/// A `navs` node: an array of nav specs or `{"seq": {...}}`.
fn navs_of(v: &Value) -> Vec<NavRecord> {
    if let Some(seq) = v.get("seq") {
        return expand_seq(seq);
    }
    v.as_array()
        .map(|a| a.iter().map(nav_from_spec).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------------
// nearest rank
// ---------------------------------------------------------------------------------------------------------------------

// C#: PerfAnalyzeTests.NearestRankPercentileMatchesHandComputedExpectations
#[test]
fn nearest_rank_percentile_matches_hand_computed_expectations() {
    let ten: &[f64] = &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
    let one: &[f64] = &[5.0];
    let four: &[f64] = &[10.0, 20.0, 30.0, 40.0];
    let rows: [(&[f64], f64, f64); 5] = [
        (ten, 50.0, 5.0),
        (ten, 95.0, 10.0),
        (ten, 100.0, 10.0),
        (one, 50.0, 5.0),
        (four, 50.0, 20.0),
    ];
    for (sorted, p, expected) in rows {
        assert_eq!(nearest_rank(sorted, p), expected, "P{p} of {sorted:?}");
    }
}

// C#: PerfAnalyzeTests.NearestRankOfEmptySequenceIsNaN
#[test]
fn nearest_rank_of_empty_sequence_is_nan() {
    let empty: &[f64] = &[];
    assert!(nearest_rank(empty, 95.0).is_nan());
}

// C#: BenchmarkStatisticsEdgeTests.NearestRank_MatchesIntegerOracleForAllWholePercents
#[test]
fn nearest_rank_matches_integer_oracle_for_all_whole_percents() {
    for n in 1..=400_i64 {
        let sorted: Vec<f64> = (1..=n).map(|i| i as f64).collect();
        for p in 0..=100_i64 {
            let expected = oracle_rank(p, n) as f64;
            let actual = nearest_rank(&sorted, p as f64);
            assert!(
                expected == actual,
                "P{p} of N={n}: expected rank {expected}, got {actual}"
            );
        }
    }
}

// C#: BenchmarkStatisticsEdgeTests.NearestRank_FractionalPercentiles
#[test]
fn nearest_rank_fractional_percentiles() {
    let rows: [(f64, usize, usize); 6] = [
        (99.9, 1000, 999),
        (99.5, 200, 199),
        (0.1, 1000, 1),
        (12.5, 8, 1),
        (37.5, 8, 3),
        (99.99, 10000, 9999),
    ];
    for (percentile, count, expected_rank) in rows {
        let sorted: Vec<f64> = (1..=count).map(|i| i as f64).collect();
        // The decimal oracle, in hundredths of a percent: ceil(p100 * n / 10000).
        let p100 = (percentile * 100.0).round() as i64;
        let oracle = (p100 * count as i64 + 9999) / 10000;
        assert_eq!(
            oracle as usize, expected_rank,
            "table agrees with the decimal oracle for P{percentile}"
        );
        assert_eq!(
            nearest_rank(&sorted, percentile),
            expected_rank as f64,
            "P{percentile} of N={count}"
        );
    }
}

// C#: BenchmarkStatisticsEdgeTests.NearestRank_RandomDataWithTies_Property
#[test]
fn nearest_rank_random_data_with_ties_property() {
    // C# uses new Random(20260926); the sequence cannot be reproduced, so the same structure runs on xorshift.
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for iteration in 0..300 {
        let n = 1 + rng.below(119) as usize;
        let mut data: Vec<f64> = (0..n).map(|_| rng.below(15) as f64).collect(); // many ties
        data.sort_by(f64::total_cmp);
        let mut previous = f64::NEG_INFINITY;
        for p in 0..=100_i64 {
            let actual = nearest_rank(&data, p as f64);
            let expected = data[(oracle_rank(p, n as i64) - 1) as usize];
            assert_eq!(actual, expected, "iteration {iteration}, P{p}, N={n}");
            assert!(
                actual >= data[0] && actual <= data[n - 1],
                "P{p}, N={n}: {actual} outside min/max"
            );
            assert!(actual >= previous, "not monotone at P{p}, N={n}");
            previous = actual;
        }
    }
}

// C#: BenchmarkStatisticsEdgeTests.NearestRank_TwoSamples
#[test]
fn nearest_rank_two_samples() {
    let two: &[f64] = &[1.0, 2.0];
    let rows: [(f64, f64); 5] = [(0.0, 1.0), (50.0, 1.0), (51.0, 2.0), (95.0, 2.0), (100.0, 2.0)];
    for (percentile, expected) in rows {
        assert_eq!(nearest_rank(two, percentile), expected, "P{percentile}");
    }
}

// C#: BenchmarkStatisticsEdgeTests.NearestRank_SingleSample
#[test]
fn nearest_rank_single_sample() {
    for p in [0.0, 1.0, 50.0, 95.0, 100.0] {
        assert_eq!(nearest_rank(&[7.5], p), 7.5, "P{p}");
    }
}

// C#: BenchmarkStatisticsEdgeTests.NearestRank_OutOfRangePercentileIsClamped
#[test]
fn nearest_rank_out_of_range_percentile_is_clamped() {
    let three: &[f64] = &[1.0, 2.0, 3.0];
    let rows: [(f64, f64); 4] = [
        (-5.0, 1.0),
        (150.0, 3.0),
        (f64::INFINITY, 3.0),
        (f64::NEG_INFINITY, 1.0),
    ];
    for (percentile, expected) in rows {
        assert_eq!(nearest_rank(three, percentile), expected, "P{percentile}");
    }
}

// C#: BenchmarkStatisticsEdgeTests.NearestRank_NaNPercentileIsNaN
#[test]
fn nearest_rank_nan_percentile_is_nan() {
    assert!(nearest_rank(&[1.0, 2.0, 3.0], f64::NAN).is_nan());
}

// C#: BenchmarkStatisticsEdgeTests.NearestRank_HugeValuesAreReturnedVerbatim
#[test]
fn nearest_rank_huge_values_are_returned_verbatim() {
    let sorted = [-f64::MAX, 0.0, f64::MAX];
    assert_eq!(nearest_rank(&sorted, 100.0), f64::MAX);
    assert_eq!(nearest_rank(&sorted, 0.0), -f64::MAX);
}

// ---------------------------------------------------------------------------------------------------------------------
// phase shares, group summary
// ---------------------------------------------------------------------------------------------------------------------

// C#: PerfAnalyzeTests.SlowestDecilePhaseShareMatchesHandComputedRatiosForOneNav
#[test]
fn slowest_decile_phase_share_matches_hand_computed_ratios_for_one_nav() {
    // One hand-built nav: final_visual = 100 ms, t_decode = 40 ms (40 %), t_assign = 10 ms (10 %).
    let nav = NavRecord {
        nav: 1,
        final_visual_ms: Some(100.0),
        t_decode_ms: Some(40.0),
        t_assign_ms: Some(10.0),
        incomplete: false,
        ..NavRecord::default()
    };
    let shares = slowest_decile_phase_share(std::slice::from_ref(&nav));
    assert_close6(shares["t_decode"], 0.40);
    assert_close6(shares["t_assign"], 0.10);
    assert!(!shares.contains_key("t_disk"), "never set: excluded, not reported as 0");
}

// C#: PerfAnalyzeTests.SlowestDecilePhaseShareOnlyAveragesTheSlowestTenPercent
#[test]
fn slowest_decile_phase_share_only_averages_the_slowest_ten_percent() {
    // 10 navs, final_visual 10..100 ms, only nav 10 (100 ms) has t_decode = 50 ms.
    let navs: Vec<NavRecord> = (1..=10_i64)
        .map(|i| NavRecord {
            nav: i,
            final_visual_ms: Some(i as f64 * 10.0),
            t_decode_ms: Some(if i == 10 { 50.0 } else { 0.0 }),
            incomplete: false,
            ..NavRecord::default()
        })
        .collect();
    let shares = slowest_decile_phase_share(&navs);
    assert_close6(shares["t_decode"], 0.50);
}

// C#: PerfAnalyzeTests.IncompleteNavsAreExcludedFromPercentilesButStillCounted
#[test]
fn incomplete_navs_are_excluded_from_percentiles_but_still_counted() {
    let navs = vec![
        NavRecord {
            nav: 1,
            final_visual_ms: Some(10.0),
            incomplete: false,
            kind: "RamHit".into(),
            ..NavRecord::default()
        },
        NavRecord {
            nav: 2,
            final_visual_ms: Some(20.0),
            incomplete: false,
            kind: "RamHit".into(),
            ..NavRecord::default()
        },
        // superseded: its huge final_visual must not leak into the percentiles
        NavRecord {
            nav: 3,
            final_visual_ms: Some(9999.0),
            incomplete: true,
            kind: "SourceMiss".into(),
            ..NavRecord::default()
        },
    ];
    let summary = GroupSummary::build(GroupKey::new("S2", "Preview", "warm", None), &navs);
    assert_eq!(summary.count, 3);
    assert_eq!(summary.incomplete, 1);
    assert_eq!(summary.final_max, 20.0);
    assert_eq!(summary.final_p95, 20.0);
}

// C#: PerfAnalyzeHardeningTests.LowSampleWarning_CountsOnlyCompleteNavs
#[test]
fn low_sample_warning_counts_only_complete_navs() {
    let navs: Vec<NavRecord> = (0..25_i64)
        .map(|i| NavRecord {
            nav: i + 1,
            incomplete: i >= 10,
            final_visual_ms: Some(10.0),
            first_visual_ms: Some(5.0),
            ..NavRecord::default()
        })
        .collect();
    let summary = GroupSummary::build(GroupKey::new("S2", "Fast", "warm", Some(2)), &navs);
    assert_eq!(summary.count, 25);
    assert_eq!(summary.incomplete, 15);
    assert!(summary.low_sample_warning());

    let enough: Vec<NavRecord> = (0..20_i64)
        .map(|i| NavRecord {
            nav: i + 1,
            incomplete: false,
            final_visual_ms: Some(10.0),
            first_visual_ms: Some(5.0),
            ..NavRecord::default()
        })
        .collect();
    let summary = GroupSummary::build(GroupKey::new("S2", "Fast", "warm", Some(2)), &enough);
    assert!(!summary.low_sample_warning());
}

// ---------------------------------------------------------------------------------------------------------------------
// RunFileMeta
// ---------------------------------------------------------------------------------------------------------------------

// C#: PlatformErrorHandlingReviewTests.RunFileMeta_Load_NonObjectSessionJson
#[test]
fn run_file_meta_load_non_object_session_json() {
    let root = TempRoot::new("non-object-session");
    std::fs::write(root.join("session.json"), "[1,2,3]").expect("write session.json");
    std::fs::write(root.join("process.json"), "\"text\"").expect("write process.json");
    let file = PerfCsvFile {
        path: root.join("perf-1.csv"),
        diag_flags: DiagFlags::new(),
        qpc_frequency: 1,
        dropped_rows: 0,
        rows: Vec::new(),
    };
    let meta = RunFileMeta::load(&file);
    assert_eq!(meta.scenario, "unknown");
    assert_eq!(meta.gc_time_percent, None);
}

// ---------------------------------------------------------------------------------------------------------------------
// golden `perf-stats` (every case of tests/golden/perf-stats.json)
// ---------------------------------------------------------------------------------------------------------------------

fn check_nearest_rank(case: &Value) -> Result<(), String> {
    let sorted = sorted_of(&case["sorted"]);
    let actual = nearest_rank(&sorted, num(&case["p"]));
    expect_f64(actual, &case["value"])
}

fn check_sweep(case: &Value) -> Result<(), String> {
    let max_n = case["max_n"].as_i64().ok_or("max_n")?;
    let ranks = case["ranks_by_n"].as_array().ok_or("ranks_by_n")?;
    for n in 1..=max_n {
        let sorted: Vec<f64> = (1..=n).map(|i| i as f64).collect();
        let row = ranks[(n - 1) as usize].as_array().ok_or("ranks row")?;
        if row.len() != 101 {
            return Err(format!("n={n}: expected 101 percents, got {}", row.len()));
        }
        for (p, expected) in row.iter().enumerate() {
            let actual = nearest_rank(&sorted, p as f64);
            expect_f64(actual, expected).map_err(|e| format!("n={n} p={p}: {e}"))?;
        }
    }
    Ok(())
}

fn check_decile(case: &Value) -> Result<(), String> {
    let navs = navs_of(&case["navs"]);
    let shares = slowest_decile_phase_share(&navs);
    let expected = case["shares"].as_array().ok_or("shares")?;
    if shares.len() != expected.len() {
        return Err(format!("expected {} shares, got {}", expected.len(), shares.len()));
    }
    for ((name, value), pair) in shares.iter().zip(expected) {
        if pair[0].as_str() != Some(name.as_str()) {
            return Err(format!("share order: expected {}, got {name}", pair[0]));
        }
        expect_f64(*value, &pair[1]).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

fn check_group_summary(case: &Value) -> Result<(), String> {
    let k = &case["key"];
    let workers = match &k["workers"] {
        Value::Null => None,
        v => Some(v.as_i64().ok_or("workers")? as i32),
    };
    let key = GroupKey::new(
        k["scenario"].as_str().ok_or("scenario")?,
        k["mode"].as_str().ok_or("mode")?,
        k["cond"].as_str().ok_or("cond")?,
        workers,
    );
    let navs = navs_of(&case["navs"]);
    let s = GroupSummary::build(key, &navs);

    if s.key.to_string() != golden::str_of(case, "key_string") {
        return Err(format!("key_string: got {}", s.key));
    }
    if i64::from(s.count) != golden::i64_of(case, "count") {
        return Err(format!("count: got {}", s.count));
    }
    if i64::from(s.incomplete) != golden::i64_of(case, "incomplete") {
        return Err(format!("incomplete: got {}", s.incomplete));
    }
    if s.low_sample_warning() != golden::bool_of(case, "low_sample_warning") {
        return Err("low_sample_warning differs".into());
    }
    let floats = [
        ("first_p50", s.first_p50),
        ("first_p95", s.first_p95),
        ("first_max", s.first_max),
        ("final_p50", s.final_p50),
        ("final_p95", s.final_p95),
        ("final_max", s.final_max),
        ("rendered_p50", s.rendered_frame_p50),
        ("rendered_p95", s.rendered_frame_p95),
        ("rendered_max", s.rendered_frame_max),
        ("hit_rate", s.hit_rate),
    ];
    for (field, actual) in floats {
        expect_f64(actual, &case[field]).map_err(|e| format!("{field}: {e}"))?;
    }
    pairs_match_counts(&s.kind_counts, &case["kind_counts"]).map_err(|e| format!("kind_counts: {e}"))?;
    pairs_match_floats(&s.slowest_phase_share, &case["slowest_phase_share"])
        .map_err(|e| format!("slowest_phase_share: {e}"))?;
    Ok(())
}

fn pairs_match_counts(actual: &IndexMap<String, i32>, expected: &Value) -> Result<(), String> {
    let pairs = expected.as_array().ok_or("pairs")?;
    if actual.len() != pairs.len() {
        return Err(format!("expected {} entries, got {}", pairs.len(), actual.len()));
    }
    for ((key, value), pair) in actual.iter().zip(pairs) {
        if pair[0].as_str() != Some(key.as_str()) || pair[1].as_i64() != Some(i64::from(*value)) {
            return Err(format!("expected {pair}, got [{key}, {value}]"));
        }
    }
    Ok(())
}

fn pairs_match_floats(actual: &IndexMap<String, f64>, expected: &Value) -> Result<(), String> {
    let pairs = expected.as_array().ok_or("pairs")?;
    if actual.len() != pairs.len() {
        return Err(format!("expected {} entries, got {}", pairs.len(), actual.len()));
    }
    for ((key, value), pair) in actual.iter().zip(pairs) {
        if pair[0].as_str() != Some(key.as_str()) {
            return Err(format!("expected key {}, got {key}", pair[0]));
        }
        expect_f64(*value, &pair[1]).map_err(|e| format!("{key}: {e}"))?;
    }
    Ok(())
}

fn check_run_meta(case: &Value) -> Result<(), String> {
    let root = TempRoot::new("run-meta");
    let files = case["files"].as_object().ok_or("files")?;
    for (rel, node) in files {
        let (text, bom) = match node {
            Value::String(s) => (s.clone(), false),
            Value::Object(o) => (
                o.get("content").and_then(Value::as_str).ok_or("content")?.to_owned(),
                o.get("bom").and_then(Value::as_bool).unwrap_or(false),
            ),
            other => return Err(format!("unsupported file node {other}")),
        };
        let full = root.join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let mut bytes = Vec::new();
        if bom {
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        bytes.extend_from_slice(text.as_bytes());
        std::fs::write(&full, bytes).map_err(|e| format!("write {}: {e}", full.display()))?;
    }

    let diag_pairs: Vec<(String, String)> = match case["diag"].as_object() {
        Some(o) => o
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
            .collect(),
        None => Vec::new(),
    };
    let diag_flags = DiagFlags::from_pairs(diag_pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let file = PerfCsvFile {
        path: root.join(golden::str_of(case, "csv")),
        diag_flags,
        qpc_frequency: 1,
        dropped_rows: 0,
        rows: Vec::new(),
    };
    let m = RunFileMeta::load(&file);

    if m.scenario != golden::str_of(case, "scenario") {
        return Err(format!("scenario: got {:?}", m.scenario));
    }
    if m.mode != golden::str_of(case, "mode") {
        return Err(format!("mode: got {:?}", m.mode));
    }
    if m.cond != golden::str_of(case, "cond") {
        return Err(format!("cond: got {:?}", m.cond));
    }
    let workers_expected = match &case["preload_workers"] {
        Value::Null => None,
        v => Some(v.as_i64().ok_or("preload_workers")? as i32),
    };
    if m.preload_workers != workers_expected {
        return Err(format!("preload_workers: got {:?}", m.preload_workers));
    }
    expect_opt_f64(m.gc_time_percent, &case["gc_time_percent"]).map_err(|e| format!("gc_time_percent: {e}"))?;
    expect_opt_f64(m.frame_time_p95_ms, &case["frame_time_p95_ms"]).map_err(|e| format!("frame_time_p95_ms: {e}"))?;
    Ok(())
}

fn check_phase_names(case: &Value) -> Result<(), String> {
    let expected: Vec<&str> = case["names"]
        .as_array()
        .ok_or("names")?
        .iter()
        .map(|v| v.as_str().unwrap_or_default())
        .collect();
    let actual: Vec<&str> = phase_getters().iter().map(|(name, _)| *name).collect();
    if actual == expected {
        Ok(())
    } else {
        Err(format!("phase names: got {actual:?}"))
    }
}

/// Replays every `perf-stats` golden case. A panic in one case (for example a `todo!()` in a function of another unit)
/// is recorded as that case's failure, so the report lists every case that disagrees.
// C#: PerfAnalyzeTests (NearestRank/Decile/GroupSummary), BenchmarkStatisticsEdgeTests, PerfAnalyzeHardeningTests
#[test]
fn golden_perf_stats_all_cases() {
    let g = golden::load("perf-stats");
    let mut failures: Vec<String> = Vec::new();
    for case in &g.cases {
        let kind = golden::str_of(case, "kind");
        let outcome = catch_unwind(AssertUnwindSafe(|| match kind {
            "nearest_rank" => check_nearest_rank(case),
            "nearest_rank_sweep" => check_sweep(case),
            "decile" => check_decile(case),
            "group_summary" => check_group_summary(case),
            "run_meta" => check_run_meta(case),
            "phase_names" => check_phase_names(case),
            other => Err(format!("unknown golden kind {other}")),
        }));
        let message = match outcome {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => e,
            Err(panic) => panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned()),
        };
        let shown = if kind == "nearest_rank_sweep" {
            kind.to_owned()
        } else {
            case.to_string()
        };
        failures.push(format!("[{kind}] {message}\n    case: {shown}"));
    }
    assert!(
        failures.is_empty(),
        "{} of {} perf-stats golden cases failed:\n{}",
        failures.len(),
        g.cases.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// W1-17 round-2 additions: mutant killers, JSON depth limit, accepted divergences
// ---------------------------------------------------------------------------------------------------------------------

fn load_side_files(session: Option<&[u8]>, process: Option<&[u8]>) -> RunFileMeta {
    let root = TempRoot::new("side-files");
    if let Some(b) = session {
        std::fs::write(root.join("session.json"), b).expect("write session.json");
    }
    if let Some(b) = process {
        std::fs::write(root.join("process.json"), b).expect("write process.json");
    }
    let file = PerfCsvFile {
        path: root.join("perf-1.csv"),
        diag_flags: DiagFlags::new(),
        qpc_frequency: 1,
        dropped_rows: 0,
        rows: Vec::new(),
    };
    RunFileMeta::load(&file)
}

#[test]
fn gc_percent_divides_first_then_multiplies() {
    // C# `gpd / eld * 100.0`: (1/3)*100 = 33.33333333333333, whereas 1*100/3 would be 33.333333333333336.
    let meta = load_side_files(None, Some(br#"{"gcPauseDeltaMs":1,"elapsedMs":3}"#));
    assert_eq!(meta.gc_time_percent, Some(33.33333333333333));
    assert_ne!(meta.gc_time_percent, Some(1.0_f64 * 100.0 / 3.0));
}

#[test]
fn one_character_folder_alias_is_appended() {
    let meta = load_side_files(Some(br#"{"folderAlias":"F"}"#), None);
    assert_eq!(meta.scenario, "unknown@F");
}

#[test]
fn nan_values_sort_first_like_double_compare() {
    let mk = |v: f64| NavRecord {
        incomplete: false,
        first_visual_ms: Some(v),
        ..NavRecord::default()
    };
    let orders: [[f64; 3]; 6] = [
        [f64::NAN, 1.0, 2.0],
        [f64::NAN, 2.0, 1.0],
        [1.0, f64::NAN, 2.0],
        [2.0, f64::NAN, 1.0],
        [1.0, 2.0, f64::NAN],
        [2.0, 1.0, f64::NAN],
    ];
    for order in orders {
        let navs: Vec<NavRecord> = order.iter().map(|&v| mk(v)).collect();
        let s = GroupSummary::build(GroupKey::new("s", "m", "c", None), &navs);
        // sorted: NaN, 1, 2 -> P50 rank 2 = 1, max = last = 2.
        assert_eq!(s.first_p50, 1.0, "{order:?}");
        assert_eq!(s.first_max, 2.0, "{order:?}");
    }
}

fn nested_arrays(n: usize) -> String {
    format!("{}{}", "[".repeat(n), "]".repeat(n))
}

#[test]
fn json_deeper_than_64_is_ignored_like_json_document_max_depth() {
    // root object + 64 nested arrays = depth 65: C# JsonDocument.Parse throws, the file is ignored.
    let deep = format!(r#"{{"scenario":"A","x":{}}}"#, nested_arrays(64));
    assert_eq!(load_side_files(Some(deep.as_bytes()), None).scenario, "unknown");
    // root object + 63 nested arrays = depth 64: accepted.
    let ok = format!(r#"{{"scenario":"A","x":{}}}"#, nested_arrays(63));
    assert_eq!(load_side_files(Some(ok.as_bytes()), None).scenario, "A");
    // Brackets inside strings do not count.
    let in_str = format!(r#"{{"scenario":"{}\"{}"}}"#, "[".repeat(100), "[".repeat(100));
    assert_eq!(load_side_files(Some(in_str.as_bytes()), None).scenario.len(), 201);
    // process.json uses the same limit.
    let deep_p = format!(r#"{{"gcTimePercent":5,"x":{}}}"#, nested_arrays(64));
    assert_eq!(load_side_files(None, Some(deep_p.as_bytes())).gc_time_percent, None);
}

// DIVERGENCE (README D5): a UTF-16 (BOM FF FE) session.json is decoded by C# `File.ReadAllText`, but the Rust port
// reads UTF-8 only (`csv::decode_bytes`, owned by W1-16), so the file is not parseable and is ignored.
#[test]
fn divergence_utf16_bom_side_file_is_ignored() {
    let mut bytes = vec![0xFF, 0xFE];
    for u in r#"{"scenario":"U"}"#.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    assert_eq!(load_side_files(Some(&bytes), None).scenario, "unknown");
}

// DIVERGENCE (README D5): `"preloadWorkers":-0` is Some(0) in C# (`TryGetInt32`); serde_json reads the token as a
// float, so the Rust port falls through to the env lookup / header fallback and leaves workers unset.
#[test]
fn divergence_negative_zero_preload_workers() {
    let meta = load_side_files(Some(br#"{"preloadWorkers":-0}"#), None);
    assert_eq!(meta.preload_workers, None);
}

// DIVERGENCE (README D5): `1e400` is +Infinity in C# (`TryGetDouble`); serde_json rejects the whole document, so the
// Rust port ignores the file (frameTimeP95Ms in the same file is lost too).
#[test]
fn divergence_overflowing_number_token_rejects_the_file() {
    let meta = load_side_files(None, Some(br#"{"gcTimePercent":1e400,"frameTimeP95Ms":20.5}"#));
    assert_eq!(meta.gc_time_percent, None);
    assert_eq!(meta.frame_time_p95_ms, None);
}
