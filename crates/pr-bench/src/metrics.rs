//! Port of `PhotoReview.Benchmarking/BenchmarkMetrics.cs` (C# baseline d92d6aae, 54 lines): subtracting a baseline
//! `ReviewMetricsSnapshot` from a later one (the benchmark engine uses it to drop the warm-up iterations).
//!
//! Unit W7-01b. Contract: `contracts/W7-01b.md`. `BenchmarkStatistics.Percentile` is NOT here: it is already ported
//! (`pr_core::diagnostics::bench_stats::percentile`, W1-15).

use std::collections::HashMap;
use std::fmt;

use pr_core::diagnostics::metrics::{PresentLatencyBucket, ReviewMetricsSnapshot, SourceOpenEntry};
use pr_core::text::{cmp_ignore_case, ignore_case_key};

/// C# `ToDictionary` throws `ArgumentException` when the BASELINE snapshot repeats a key. The text names the repeated
/// key as spelled in the baseline entry that was seen second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchmarkMetricsError {
    /// Two `top_source_opens` baseline entries whose paths are equal under OrdinalIgnoreCase.
    DuplicateSourceOpenPath(String),
    /// Two `present_histogram` baseline buckets with the same label (ordinal, case-sensitive).
    DuplicatePresentBucketLabel(String),
}

impl fmt::Display for BenchmarkMetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSourceOpenPath(key) => write!(f, "duplicate path in baseline TopSourceOpens: {key}"),
            Self::DuplicatePresentBucketLabel(key) => write!(f, "duplicate label in baseline PresentHistogram: {key}"),
        }
    }
}

impl std::error::Error for BenchmarkMetricsError {}

/// C# `BenchmarkMetrics.Since(current, baseline)`: what was recorded after `baseline` was taken.
///
/// * Every scalar counter is `current - baseline` with wrapping `i64` arithmetic (no clamp: a negative delta stays negative).
/// * `decoder_fallbacks`: current order, one entry per current backend whose delta is strictly positive.
/// * `top_source_opens`: each current entry minus the baseline count of the same path (OrdinalIgnoreCase, 0 when the
///   baseline did not list it), entries left with `count <= 0` dropped, then count descending and path
///   OrdinalIgnoreCase ascending (stable).
/// * `present_histogram`: current order, each bucket minus the baseline bucket with the same label (ordinal, 0 when
///   missing); nothing is dropped and counts may be zero or negative.
///
/// Errors only when the baseline has duplicate keys (see [`BenchmarkMetricsError`]); `current` may repeat keys.
pub fn since(
    current: &ReviewMetricsSnapshot,
    baseline: &ReviewMetricsSnapshot,
) -> Result<ReviewMetricsSnapshot, BenchmarkMetricsError> {
    // Baseline lookups first, as C# `ToDictionary` does before anything else: source opens, then histogram labels.
    let mut open_baseline: HashMap<String, i64> = HashMap::with_capacity(baseline.top_source_opens.len());
    for entry in &baseline.top_source_opens {
        if open_baseline
            .insert(ignore_case_key(&entry.path), entry.count)
            .is_some()
        {
            return Err(BenchmarkMetricsError::DuplicateSourceOpenPath(entry.path.clone()));
        }
    }
    let mut label_baseline: HashMap<&str, i64> = HashMap::with_capacity(baseline.present_histogram.len());
    for bucket in &baseline.present_histogram {
        if label_baseline.insert(bucket.label.as_str(), bucket.count).is_some() {
            return Err(BenchmarkMetricsError::DuplicatePresentBucketLabel(bucket.label.clone()));
        }
    }

    // Wrapping, like C# unchecked `long` arithmetic: no clamp, no panic in debug builds.
    let sub = i64::wrapping_sub;

    // Current order; baseline-only backends are ignored; the first baseline entry of a backend is the one used.
    let decoder_fallbacks = current
        .decoder_fallbacks
        .iter()
        .filter_map(|(backend, count)| {
            let base = baseline
                .decoder_fallbacks
                .iter()
                .find(|(b, _)| b == backend)
                .map_or(0, |(_, c)| *c);
            let delta = sub(*count, base);
            (delta > 0).then_some((*backend, delta))
        })
        .collect();

    let mut top_source_opens: Vec<SourceOpenEntry> = current
        .top_source_opens
        .iter()
        .filter_map(|entry| {
            let base = open_baseline.get(&ignore_case_key(&entry.path)).copied().unwrap_or(0);
            let delta = sub(entry.count, base);
            (delta > 0).then(|| SourceOpenEntry {
                path: entry.path.clone(),
                count: delta,
            })
        })
        .collect();
    // Stable sort: count descending, then path OrdinalIgnoreCase ascending; ties keep input order.
    top_source_opens.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| cmp_ignore_case(&a.path, &b.path)));

    // Nothing is dropped here: zero and negative deltas stay.
    let present_histogram = current
        .present_histogram
        .iter()
        .map(|bucket| {
            let base = label_baseline.get(bucket.label.as_str()).copied().unwrap_or(0);
            PresentLatencyBucket {
                label: bucket.label.clone(),
                count: sub(bucket.count, base),
            }
        })
        .collect();

    Ok(ReviewMetricsSnapshot {
        cache_hits: sub(current.cache_hits, baseline.cache_hits),
        cache_misses: sub(current.cache_misses, baseline.cache_misses),
        source_bytes_read: sub(current.source_bytes_read, baseline.source_bytes_read),
        source_reads: sub(current.source_reads, baseline.source_reads),
        decode_milliseconds: sub(current.decode_milliseconds, baseline.decode_milliseconds),
        presented_images: sub(current.presented_images, baseline.presented_images),
        present_milliseconds: sub(current.present_milliseconds, baseline.present_milliseconds),
        preload_hits: sub(current.preload_hits, baseline.preload_hits),
        inflight_joins: sub(current.inflight_joins, baseline.inflight_joins),
        disk_cache_hits: sub(current.disk_cache_hits, baseline.disk_cache_hits),
        ui_assign_milliseconds: sub(current.ui_assign_milliseconds, baseline.ui_assign_milliseconds),
        source_open_count: sub(current.source_open_count, baseline.source_open_count),
        stat_count: sub(current.stat_count, baseline.stat_count),
        session_write_count: sub(current.session_write_count, baseline.session_write_count),
        cross_thread_present_count: sub(current.cross_thread_present_count, baseline.cross_thread_present_count),
        decoder_fallbacks,
        top_source_opens,
        present_histogram,
    })
}
