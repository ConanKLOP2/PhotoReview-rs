//! W2-17: ports of `ReviewMetricsTests` and `ReviewMetricsTrimTests` plus the golden `review-metrics`
//! (C# `PhotoReview.Core.Tests/Diagnostics`, baseline d92d6aae).

use pr_core::diagnostics::metrics::{
    MAX_TRACKED_SOURCE_OPEN_PATHS, PRESENT_BUCKET_LABELS, PRESENT_BUCKET_UPPER_BOUNDS_MS, ReviewMetrics,
    SourceOpenEntry, present_bucket_index,
};
use pr_core::model::enums::DecoderBackend;
use pr_core::model::serde_ext::LenientEnum;
use pr_test_support::golden;
use serde_json::Value;

// C#: ReviewMetricsTests.DecodeMillisecondsEwma_StartsAtFirstSampleThenSmooths
#[test]
fn decode_milliseconds_ewma_starts_at_first_sample_then_smooths() {
    let metrics = ReviewMetrics::new();
    assert_eq!(metrics.decode_milliseconds_ewma(), 0.0);

    metrics.record_source_read(100, 300, true);
    assert_eq!(metrics.decode_milliseconds_ewma(), 300.0);

    metrics.record_source_read(100, 400, true);
    let expected = 300.0 + pr_core::diagnostics::metrics::DECODE_EWMA_ALPHA * 100.0;
    assert!((metrics.decode_milliseconds_ewma() - expected).abs() < 1e-6);

    // A zero-time read (e.g. rounding) is not a decode sample.
    metrics.record_source_read(100, 0, true);
    assert!((metrics.decode_milliseconds_ewma() - expected).abs() < 1e-6);
}

// C#: ReviewMetricsTests.RecordMethodsAccumulateCorrectly
#[test]
fn record_methods_accumulate_correctly() {
    let metrics = ReviewMetrics::new();

    metrics.record_cache_hit();
    metrics.record_cache_hit();
    metrics.record_cache_miss();

    metrics.record_source_read(1024, 15, true);
    metrics.record_source_read(2048, 25, true);

    metrics.record_presented(50);
    metrics.record_preload_hit();
    metrics.record_inflight_join();
    metrics.record_disk_cache_hit();
    metrics.record_ui_assign(10);

    let snapshot = metrics.snapshot();

    assert_eq!(snapshot.cache_hits, 2);
    assert_eq!(snapshot.cache_misses, 1);
    assert_eq!(snapshot.source_bytes_read, 3072);
    assert_eq!(snapshot.source_reads, 2);
    assert_eq!(snapshot.decode_milliseconds, 40);
    assert_eq!(snapshot.presented_images, 1);
    assert_eq!(snapshot.present_milliseconds, 50);
    assert_eq!(snapshot.preload_hits, 1);
    assert_eq!(snapshot.inflight_joins, 1);
    assert_eq!(snapshot.disk_cache_hits, 1);
    assert_eq!(snapshot.ui_assign_milliseconds, 10);
}

// C#: ReviewMetricsTests.FullDecodeSourceReadIsCountedInTotalsButDoesNotAffectPreloadEwma
#[test]
fn full_decode_source_read_is_counted_in_totals_but_does_not_affect_preload_ewma() {
    let metrics = ReviewMetrics::new();

    metrics.record_source_read(24_000_000, 1_500, false);

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.source_reads, 1);
    assert_eq!(snapshot.source_bytes_read, 24_000_000);
    assert_eq!(snapshot.decode_milliseconds, 1_500);
    assert_eq!(metrics.decode_milliseconds_ewma(), 0.0);
}

// C#: ReviewMetricsTests.ConcurrentRecordingIsThreadSafe
#[test]
fn concurrent_recording_is_thread_safe() {
    let metrics = ReviewMetrics::new();
    const THREADS: i64 = 8;
    const PER_THREAD: i64 = 125;
    const ITERATIONS: i64 = THREADS * PER_THREAD;

    std::thread::scope(|scope| {
        for _ in 0..THREADS {
            scope.spawn(|| {
                for _ in 0..PER_THREAD {
                    metrics.record_cache_hit();
                    metrics.record_cache_miss();
                    metrics.record_source_read(100, 2, true);
                    metrics.record_presented(5);
                    metrics.record_preload_hit();
                    metrics.record_inflight_join();
                    metrics.record_disk_cache_hit();
                    metrics.record_ui_assign(1);
                }
            });
        }
    });

    let snapshot = metrics.snapshot();

    assert_eq!(snapshot.cache_hits, ITERATIONS);
    assert_eq!(snapshot.cache_misses, ITERATIONS);
    assert_eq!(snapshot.source_bytes_read, ITERATIONS * 100);
    assert_eq!(snapshot.source_reads, ITERATIONS);
    assert_eq!(snapshot.decode_milliseconds, ITERATIONS * 2);
    assert_eq!(snapshot.presented_images, ITERATIONS);
    assert_eq!(snapshot.present_milliseconds, ITERATIONS * 5);
    assert_eq!(snapshot.preload_hits, ITERATIONS);
    assert_eq!(snapshot.inflight_joins, ITERATIONS);
    assert_eq!(snapshot.disk_cache_hits, ITERATIONS);
    assert_eq!(snapshot.ui_assign_milliseconds, ITERATIONS);
}

// C#: ReviewMetricsTests.SourceOpensAreCountedInTotalAndPerPath
#[test]
fn source_opens_are_counted_in_total_and_per_path() {
    let metrics = ReviewMetrics::new();
    for i in 0..12 {
        for _ in 0..=i {
            metrics.record_source_open(&format!(r"C:\photos\img{i:02}.jpg"));
        }
    }
    metrics.record_source_open(r"C:\PHOTOS\IMG11.JPG"); // same path, different case

    let snapshot = metrics.snapshot();

    assert_eq!(snapshot.source_open_count, 78 + 1);
    assert_eq!(snapshot.top_source_opens.len(), 10);
    assert_eq!(snapshot.top_source_opens[0].path, r"C:\photos\img11.jpg");
    assert_eq!(snapshot.top_source_opens[0].count, 13);
    assert_eq!(snapshot.top_source_opens[1].path, r"C:\photos\img10.jpg");
    assert!(!snapshot.top_source_opens.iter().any(|e| e.path.ends_with("img00.jpg")));
}

// C#: ReviewMetricsTests.SourceOpenTableIsBounded_TopPathsAndTotalSurvive
#[test]
fn source_open_table_is_bounded_top_paths_and_total_survive() {
    let metrics = ReviewMetrics::new();
    for _ in 0..5 {
        metrics.record_source_open(r"C:\photos\hot.jpg");
    }
    for _ in 0..3 {
        metrics.record_source_open(r"C:\photos\warm.jpg");
    }
    let distinct = MAX_TRACKED_SOURCE_OPEN_PATHS * 3;
    for i in 0..distinct {
        metrics.record_source_open(&format!(r"C:\photos\once{i:05}.jpg"));
    }

    let snapshot = metrics.snapshot();

    let tracked = metrics.tracked_source_open_paths();
    assert!(
        (1..=MAX_TRACKED_SOURCE_OPEN_PATHS).contains(&tracked),
        "tracked={tracked}"
    );
    assert_eq!(snapshot.source_open_count, (5 + 3 + distinct) as i64);
    assert_eq!(
        snapshot.top_source_opens[0],
        SourceOpenEntry {
            path: r"C:\photos\hot.jpg".to_owned(),
            count: 5
        }
    );
    assert_eq!(
        snapshot.top_source_opens[1],
        SourceOpenEntry {
            path: r"C:\photos\warm.jpg".to_owned(),
            count: 3
        }
    );
}

// C#: ReviewMetricsTests.StatSessionWriteAndFallbackCountsAccumulate
#[test]
fn stat_session_write_and_fallback_counts_accumulate() {
    let metrics = ReviewMetrics::new();
    metrics.record_stat();
    metrics.record_stat();
    metrics.record_session_write();
    metrics.record_decoder_fallback(DecoderBackend::WicDirect);
    metrics.record_decoder_fallback(DecoderBackend::TurboJpeg);
    metrics.record_decoder_fallback(DecoderBackend::WicDirect);

    let snapshot = metrics.snapshot();

    assert_eq!(snapshot.stat_count, 2);
    assert_eq!(snapshot.session_write_count, 1);
    assert_eq!(snapshot.decoder_fallback_count(), 3);
}

// C#: ReviewMetricsTests.CrossThreadPresentCountAccumulates
#[test]
fn cross_thread_present_count_accumulates() {
    let metrics = ReviewMetrics::new();
    assert_eq!(metrics.snapshot().cross_thread_present_count, 0);

    metrics.record_cross_thread_present();
    metrics.record_cross_thread_present();

    assert_eq!(metrics.snapshot().cross_thread_present_count, 2);
}

// C#: ReviewMetricsTests.PresentLatencyIsBucketed (16 rows of the theory)
#[test]
fn present_latency_is_bucketed() {
    let rows: [(i64, &str); 16] = [
        (0, "<=8"),
        (8, "<=8"),
        (9, "<=16"),
        (16, "<=16"),
        (17, "<=33"),
        (33, "<=33"),
        (34, "<=50"),
        (50, "<=50"),
        (51, "<=100"),
        (100, "<=100"),
        (101, "<=200"),
        (200, "<=200"),
        (201, "<=500"),
        (500, "<=500"),
        (501, ">500"),
        (60_000, ">500"),
    ];
    for (milliseconds, expected_label) in rows {
        let metrics = ReviewMetrics::new();
        metrics.record_presented(milliseconds);

        let histogram = metrics.snapshot().present_histogram;

        assert_eq!(histogram.len(), 8, "{milliseconds} ms");
        let hit: Vec<_> = histogram.iter().filter(|b| b.count == 1).collect();
        assert_eq!(hit.len(), 1, "{milliseconds} ms");
        assert_eq!(hit[0].label, expected_label, "{milliseconds} ms");
        assert_eq!(
            PRESENT_BUCKET_LABELS[present_bucket_index(milliseconds)],
            expected_label
        );
    }
    assert_eq!(PRESENT_BUCKET_UPPER_BOUNDS_MS, [8, 16, 33, 50, 100, 200, 500]);
}

// C#: ReviewMetricsTests.NewFieldsAreSerialized
#[test]
fn new_fields_are_serialized() {
    let metrics = ReviewMetrics::new();
    metrics.record_source_open(r"C:\photos\a.jpg");
    metrics.record_stat();
    metrics.record_session_write();
    metrics.record_presented(20);

    let json = serde_json::to_string(&metrics.snapshot()).expect("snapshot serializes");

    for field in [
        "SourceOpenCount",
        "TopSourceOpens",
        "StatCount",
        "SessionWriteCount",
        "PresentHistogram",
        "DecoderFallbackCount",
        "CrossThreadPresentCount",
    ] {
        assert!(json.contains(&format!("\"{field}\"")), "{field} missing in {json}");
    }
}

// C#: ReviewMetricsTrimTests.SourceOpenTable_TrimsToThreeQuartersOfCap
#[test]
fn source_open_table_trims_to_three_quarters_of_cap() {
    let metrics = ReviewMetrics::new();
    let cap = MAX_TRACKED_SOURCE_OPEN_PATHS;

    for i in 0..cap {
        metrics.record_source_open(&format!(r"C:\photos\p{i:05}.jpg"));
    }
    assert_eq!(metrics.tracked_source_open_paths(), cap);

    metrics.record_source_open(r"C:\photos\one-more.jpg");

    assert_eq!(metrics.tracked_source_open_paths(), cap * 3 / 4);
    assert_eq!(metrics.snapshot().source_open_count, (cap + 1) as i64);
}

// DIVERGENCE (contract note 24): a later spelling of the same file keeps the FIRST spelling and increments the count.
#[test]
fn source_open_keeps_the_first_spelling_of_a_path() {
    let metrics = ReviewMetrics::new();
    metrics.record_source_open(r"C:\Photos\A.jpg");
    metrics.record_source_open(r"c:\photos\a.JPG");

    let top = metrics.snapshot().top_source_opens;

    assert_eq!(top.len(), 1);
    assert_eq!(top[0].path, r"C:\Photos\A.jpg");
    assert_eq!(top[0].count, 2);
}

// C#: ReviewMetrics Interlocked.Increment wraps (README D3): the counters are `long` and never saturate.
#[test]
fn snapshot_counts_wrap_like_a_c_sharp_long() {
    let metrics = ReviewMetrics::new();
    metrics.record_ui_assign(i64::MAX);
    metrics.record_ui_assign(1);

    assert_eq!(metrics.snapshot().ui_assign_milliseconds, i64::MIN);
}

fn run_ops(case: &Value) -> (ReviewMetrics, Vec<String>) {
    let metrics = ReviewMetrics::new();
    let mut ewma_after_reads = Vec::new();
    for op in case["ops"].as_array().expect("ops array") {
        let name = golden::str_of(op, "op");
        let long = |key: &str, fallback: i64| op.get(key).and_then(Value::as_i64).unwrap_or(fallback);
        match name {
            "cacheHit" => (0..long("times", 1)).for_each(|_| metrics.record_cache_hit()),
            "cacheMiss" => (0..long("times", 1)).for_each(|_| metrics.record_cache_miss()),
            "sourceRead" => {
                let include = op.get("ewma").and_then(Value::as_bool).unwrap_or(true);
                metrics.record_source_read(long("bytes", 0), long("ms", 0), include);
                ewma_after_reads.push(format!("{:016X}", metrics.decode_milliseconds_ewma().to_bits()));
            }
            "presented" => metrics.record_presented(long("ms", 0)),
            "preloadHit" => metrics.record_preload_hit(),
            "inflightJoin" => metrics.record_inflight_join(),
            "diskCacheHit" => metrics.record_disk_cache_hit(),
            "uiAssign" => metrics.record_ui_assign(long("ms", 0)),
            "fallback" => {
                let backend_name = golden::str_of(op, "backend");
                let backend = DecoderBackend::MEMBERS
                    .iter()
                    .copied()
                    .find(|member| member.name() == backend_name)
                    .unwrap_or_else(|| panic!("unknown backend {backend_name}"));
                metrics.record_decoder_fallback(backend);
            }
            "sourceOpen" => {
                let path = golden::str_of(op, "path");
                for _ in 0..long("times", 1) {
                    metrics.record_source_open(path);
                }
            }
            "sourceOpenRange" => {
                let prefix = golden::str_of(op, "prefix");
                let suffix = golden::str_of(op, "suffix");
                for i in 0..long("count", 0) {
                    metrics.record_source_open(&format!("{prefix}{i:05}{suffix}"));
                }
            }
            "stat" => metrics.record_stat(),
            "sessionWrite" => metrics.record_session_write(),
            "crossThreadPresent" => metrics.record_cross_thread_present(),
            "snapshotEwma" => {}
            other => panic!("unknown op {other}"),
        }
    }
    (metrics, ewma_after_reads)
}

// Golden: review-metrics (14 cases generated by the C# ReviewMetrics).
#[test]
fn golden_review_metrics_snapshot_ewma_and_tracked_paths_match_csharp() {
    let golden = golden::load("review-metrics");
    assert_eq!(golden.cases.len(), 14);
    for case in &golden.cases {
        let id = golden::str_of(case, "id");
        let (metrics, ewma_after_reads) = run_ops(case);

        let mut actual = serde_json::to_value(metrics.snapshot()).expect("snapshot to value");
        if case.get("stable_top_only").and_then(Value::as_bool) == Some(true) {
            // Ties among once-opened paths after a trim are arbitrary in C#: only entries with Count > 1 are compared.
            if let Some(top) = actual["TopSourceOpens"].as_array_mut() {
                top.retain(|entry| entry["Count"].as_i64().unwrap_or(0) > 1);
            }
        }
        assert_eq!(actual, case["snapshot"], "{id}: snapshot");

        let expected_ewma: Vec<String> = case["ewma_bits_after_each_source_read"]
            .as_array()
            .expect("ewma list")
            .iter()
            .map(|v| v.as_str().expect("ewma bits text").to_owned())
            .collect();
        assert_eq!(ewma_after_reads, expected_ewma, "{id}: ewma after each read");
        assert_eq!(
            format!("{:016X}", metrics.decode_milliseconds_ewma().to_bits()),
            golden::str_of(case, "ewma_bits"),
            "{id}: ewma bits"
        );
        assert_eq!(
            metrics.tracked_source_open_paths() as i64,
            golden::i64_of(case, "tracked_source_open_paths"),
            "{id}: tracked paths"
        );
    }
}
