//! Port of `PhotoReview.Benchmarking/BenchmarkMetrics.cs` (C# baseline d92d6aae): `BenchmarkMetrics.Since`. Unit W7-01b,
//! contract `contracts/W7-01b.md`. There is no C# test for `Since`: the golden cases come from `tests/golden/benchmark-metrics-since.json`
//! (oracle `tools/oracle-cs/Commands/W7-01b.cs`, which reaches the internal class by reflection), the `// new:` tests are hand-written.
//! Red stage: the stub body is `todo!()`, so every test here fails until the porter implements `since`.

use pr_bench::metrics::{BenchmarkMetricsError, since};
use pr_core::diagnostics::metrics::{
    PRESENT_BUCKET_LABELS, PresentLatencyBucket, ReviewMetricsSnapshot, SourceOpenEntry,
};
use pr_core::model::enums::DecoderBackend;
use pr_core::model::serde_ext::parse_lenient_text;
use pr_core::text::ignore_case_key;
use pr_test_support::golden::{self, i64_of, str_of};
use serde_json::Value;

/// The 15 scalar fields of `ReviewMetricsSnapshot`, in the order of the contract (note 7).
const SCALAR_NAMES: [&str; 15] = [
    "cache_hits",
    "cache_misses",
    "source_bytes_read",
    "source_reads",
    "decode_milliseconds",
    "presented_images",
    "present_milliseconds",
    "preload_hits",
    "inflight_joins",
    "disk_cache_hits",
    "ui_assign_milliseconds",
    "source_open_count",
    "stat_count",
    "session_write_count",
    "cross_thread_present_count",
];

fn scalars(s: &ReviewMetricsSnapshot) -> [i64; 15] {
    [
        s.cache_hits,
        s.cache_misses,
        s.source_bytes_read,
        s.source_reads,
        s.decode_milliseconds,
        s.presented_images,
        s.present_milliseconds,
        s.preload_hits,
        s.inflight_joins,
        s.disk_cache_hits,
        s.ui_assign_milliseconds,
        s.source_open_count,
        s.stat_count,
        s.session_write_count,
        s.cross_thread_present_count,
    ]
}

fn scalars_mut(s: &mut ReviewMetricsSnapshot) -> [&mut i64; 15] {
    [
        &mut s.cache_hits,
        &mut s.cache_misses,
        &mut s.source_bytes_read,
        &mut s.source_reads,
        &mut s.decode_milliseconds,
        &mut s.presented_images,
        &mut s.present_milliseconds,
        &mut s.preload_hits,
        &mut s.inflight_joins,
        &mut s.disk_cache_hits,
        &mut s.ui_assign_milliseconds,
        &mut s.source_open_count,
        &mut s.stat_count,
        &mut s.session_write_count,
        &mut s.cross_thread_present_count,
    ]
}

fn backend_of(name: &str) -> DecoderBackend {
    parse_lenient_text::<DecoderBackend>(Some(name))
        .unwrap_or_else(|| panic!("unknown decoder backend in golden: {name}"))
}

/// Builds a snapshot from a golden case object: every scalar optional (default 0), lists in the order given.
fn snapshot_from_json(v: &Value) -> ReviewMetricsSnapshot {
    let n = |key: &str| v[key].as_i64().unwrap_or(0);
    let list = |key: &str| v[key].as_array().cloned().unwrap_or_default();
    let mut s = ReviewMetricsSnapshot::default();
    for (slot, name) in scalars_mut(&mut s).into_iter().zip(SCALAR_NAMES) {
        *slot = n(name);
    }
    s.decoder_fallbacks = list("decoder_fallbacks")
        .iter()
        .map(|e| (backend_of(str_of(e, "backend")), i64_of(e, "count")))
        .collect();
    s.top_source_opens = list("top_source_opens")
        .iter()
        .map(|e| SourceOpenEntry {
            path: str_of(e, "path").to_owned(),
            count: i64_of(e, "count"),
        })
        .collect();
    s.present_histogram = list("present_histogram")
        .iter()
        .map(|e| PresentLatencyBucket {
            label: str_of(e, "label").to_owned(),
            count: i64_of(e, "count"),
        })
        .collect();
    s
}

/// Compares every field of `got` with the golden `since` object (every scalar, both lists).
fn assert_since_matches(name: &str, got: &ReviewMetricsSnapshot, want: &Value) {
    for (key, value) in SCALAR_NAMES.iter().zip(scalars(got)) {
        assert_eq!(value, i64_of(want, key), "{name}: {key}");
    }
    let want_fallbacks: Vec<(DecoderBackend, i64)> = want["decoder_fallbacks"]
        .as_array()
        .expect("since has decoder_fallbacks")
        .iter()
        .map(|e| (backend_of(str_of(e, "backend")), i64_of(e, "count")))
        .collect();
    assert_eq!(got.decoder_fallbacks, want_fallbacks, "{name}: decoder_fallbacks");
    let want_opens: Vec<SourceOpenEntry> = want["top_source_opens"]
        .as_array()
        .expect("since has top_source_opens")
        .iter()
        .map(|e| SourceOpenEntry {
            path: str_of(e, "path").to_owned(),
            count: i64_of(e, "count"),
        })
        .collect();
    assert_eq!(got.top_source_opens, want_opens, "{name}: top_source_opens");
    let want_histogram: Vec<PresentLatencyBucket> = want["present_histogram"]
        .as_array()
        .expect("since has present_histogram")
        .iter()
        .map(|e| PresentLatencyBucket {
            label: str_of(e, "label").to_owned(),
            count: i64_of(e, "count"),
        })
        .collect();
    assert_eq!(got.present_histogram, want_histogram, "{name}: present_histogram");
}

/// The error the golden case must produce: the first baseline key that repeats, spelled as in the second baseline entry.
/// Source-open paths repeat under OrdinalIgnoreCase, histogram labels repeat ordinally.
fn expected_error(name: &str, baseline: &Value) -> BenchmarkMetricsError {
    let opens = baseline["top_source_opens"].as_array().cloned().unwrap_or_default();
    for (j, entry) in opens.iter().enumerate() {
        let key = ignore_case_key(str_of(entry, "path"));
        if opens[..j]
            .iter()
            .any(|earlier| ignore_case_key(str_of(earlier, "path")) == key)
        {
            return BenchmarkMetricsError::DuplicateSourceOpenPath(str_of(entry, "path").to_owned());
        }
    }
    let buckets = baseline["present_histogram"].as_array().cloned().unwrap_or_default();
    for (j, entry) in buckets.iter().enumerate() {
        let label = str_of(entry, "label");
        if buckets[..j].iter().any(|earlier| str_of(earlier, "label") == label) {
            return BenchmarkMetricsError::DuplicatePresentBucketLabel(label.to_owned());
        }
    }
    panic!("{name}: golden says it throws, but the baseline has no duplicate key");
}

// Golden: every case of tests/golden/benchmark-metrics-since.json (36 cases).
#[test]
fn golden_benchmark_metrics_since() {
    let golden = golden::load("benchmark-metrics-since");
    for case in &golden.cases {
        let name = str_of(case, "name");
        let current = snapshot_from_json(&case["current"]);
        let baseline = snapshot_from_json(&case["baseline"]);
        let result = since(&current, &baseline);
        if case["threw"].is_null() {
            let got = result.unwrap_or_else(|e| panic!("{name}: unexpected error: {e}"));
            assert_since_matches(name, &got, &case["since"]);
        } else {
            let err = result.expect_err(name);
            assert_eq!(err, expected_error(name, &case["baseline"]), "{name}: error variant");
        }
    }
}

// new: equal snapshots subtract to zero scalars, no fallbacks, no opens; the histogram keeps its 8 labels with zero counts.
#[test]
fn since_of_equal_snapshots_is_all_zero_with_empty_lists_except_histogram() {
    let mut snapshot = ReviewMetricsSnapshot::default();
    for slot in scalars_mut(&mut snapshot) {
        *slot = 5;
    }
    snapshot.decoder_fallbacks = vec![(DecoderBackend::Wpf, 3)];
    snapshot.top_source_opens = vec![SourceOpenEntry {
        path: "C:\\a.jpg".to_owned(),
        count: 2,
    }];
    snapshot.present_histogram = PRESENT_BUCKET_LABELS
        .iter()
        .map(|label| PresentLatencyBucket {
            label: (*label).to_owned(),
            count: 1,
        })
        .collect();

    let delta = since(&snapshot, &snapshot).expect("no duplicate keys in this baseline");

    assert_eq!(scalars(&delta), [0; 15]);
    assert!(delta.decoder_fallbacks.is_empty());
    assert!(delta.top_source_opens.is_empty());
    let labels: Vec<&str> = delta.present_histogram.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, PRESENT_BUCKET_LABELS.to_vec());
    assert!(delta.present_histogram.iter().all(|b| b.count == 0));
}

// new: the arguments are borrowed, never changed.
#[test]
fn since_does_not_modify_its_arguments() {
    let mut current = ReviewMetricsSnapshot::default();
    for (i, slot) in scalars_mut(&mut current).into_iter().enumerate() {
        *slot = 100 + i as i64;
    }
    current.decoder_fallbacks = vec![(DecoderBackend::TurboJpeg, 9), (DecoderBackend::Wpf, 4)];
    current.top_source_opens = vec![SourceOpenEntry {
        path: "a.jpg".to_owned(),
        count: 7,
    }];
    let baseline = ReviewMetricsSnapshot {
        cache_hits: 1,
        decoder_fallbacks: vec![(DecoderBackend::Wpf, 1)],
        ..Default::default()
    };
    let current_before = current.clone();
    let baseline_before = baseline.clone();

    let _ = since(&current, &baseline);

    assert_eq!(current, current_before, "current must not change");
    assert_eq!(baseline, baseline_before, "baseline must not change");
}

// new: every one of the 15 scalar fields is subtracted exactly once. Distinct primes per field, current = 2 x baseline,
// so a swapped, forgotten or doubled field shows up as a wrong number.
#[test]
fn since_uses_every_field_once() {
    const PRIMES: [i64; 15] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47];
    let mut baseline = ReviewMetricsSnapshot::default();
    for (slot, prime) in scalars_mut(&mut baseline).into_iter().zip(PRIMES) {
        *slot = prime;
    }
    let mut current = ReviewMetricsSnapshot::default();
    for (slot, prime) in scalars_mut(&mut current).into_iter().zip(PRIMES) {
        *slot = prime * 2;
    }

    let delta = since(&current, &baseline).expect("no duplicate keys");

    assert_eq!(scalars(&delta), PRIMES, "current - baseline = baseline for every field");
}

// new: the error text names the repeated key (the contract's `ToDictionary` message stand-in). Reached through `since`
// so that the test is red until the porter implements it.
#[test]
fn error_display_names_the_duplicate_key() {
    let baseline = ReviewMetricsSnapshot {
        top_source_opens: vec![
            SourceOpenEntry {
                path: "C:\\Photos\\a.jpg".to_owned(),
                count: 1,
            },
            SourceOpenEntry {
                path: "c:\\photos\\A.JPG".to_owned(),
                count: 2,
            },
        ],
        ..Default::default()
    };
    let err = since(&ReviewMetricsSnapshot::default(), &baseline).expect_err("duplicate baseline path must fail");
    let text = err.to_string();
    assert!(text.contains("c:\\photos\\A.JPG"), "message was: {text}");
    let label = BenchmarkMetricsError::DuplicatePresentBucketLabel("<=8".to_owned());
    assert!(label.to_string().contains("<=8"), "message was: {label}");
    let _: &dyn std::error::Error = &label;
}
