//! Port of `PhotoReview.Core/Diagnostics/ReviewMetrics.cs` (C# baseline d92d6aae, 183 lines): the thread-safe counters
//! behind the diagnostics window and the benchmark report, plus the immutable snapshot records.
//!
//! All counters are `long` in C#, so they are `AtomicI64` here (README D3) with `SeqCst` ordering. The decode EWMA is
//! an `f64` stored as its bits in an `AtomicI64` and updated with a compare-exchange loop, as in C#. The per-path open
//! table is a `Mutex<HashMap>` keyed by the OrdinalIgnoreCase key of the path (R0003: `ConcurrentDictionary` ->
//! `Mutex<HashMap>` first; never hold the lock across anything else).
//!
//! Serialization: [`ReviewMetricsSnapshot`] serializes with `serde` to the same JSON `System.Text.Json` writes for the
//! C# record (property names, order, `DecoderFallbacks` as an object keyed by the backend name, the computed
//! `DecoderFallbackCount`); the benchmark report (Wave 7) embeds it. DIVERGENCE: `serde_json` does not escape `<`, `>`
//! and non-ASCII characters the way System.Text.Json's default encoder does; readers parse JSON and are unaffected.
//!
//! Unit W2-17. Contract: `contracts/W2-17.md`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::ser::{Serialize, SerializeMap, SerializeStruct, Serializer};

use crate::model::enums::DecoderBackend;
use crate::model::serde_ext::LenientEnum;
use crate::text::{cmp_ignore_case, ignore_case_key};

/// Most distinct paths the per-path open table keeps; past it the least-opened paths are dropped
/// (C# `MaxTrackedSourceOpenPaths`).
pub const MAX_TRACKED_SOURCE_OPEN_PATHS: usize = 4096;

/// Weight of the newest sample in the decode EWMA (C# `DecodeEwmaAlpha`).
pub const DECODE_EWMA_ALPHA: f64 = 0.2;

/// Upper bounds (ms, inclusive) of the present-latency histogram; the last bucket is unbounded.
pub const PRESENT_BUCKET_UPPER_BOUNDS_MS: [i64; 7] = [8, 16, 33, 50, 100, 200, 500];

/// Labels of the 8 histogram buckets, in order.
pub const PRESENT_BUCKET_LABELS: [&str; 8] = ["<=8", "<=16", "<=33", "<=50", "<=100", "<=200", "<=500", ">500"];

/// One entry of the "most opened paths" list. C# `SourceOpenEntry(Path, Count)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOpenEntry {
    pub path: String,
    pub count: i64,
}

/// One histogram bucket. C# `PresentLatencyBucket(Label, Count)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentLatencyBucket {
    pub label: String,
    pub count: i64,
}

/// C# `ReviewMetricsSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewMetricsSnapshot {
    pub cache_hits: i64,
    pub cache_misses: i64,
    pub source_bytes_read: i64,
    pub source_reads: i64,
    pub decode_milliseconds: i64,
    pub presented_images: i64,
    pub present_milliseconds: i64,
    pub preload_hits: i64,
    pub inflight_joins: i64,
    pub disk_cache_hits: i64,
    pub ui_assign_milliseconds: i64,
    /// Backend and count, ordered by the backend's numeric value (the C# dictionary order is not specified).
    pub decoder_fallbacks: Vec<(DecoderBackend, i64)>,
    pub source_open_count: i64,
    /// At most 10 entries: count descending, then path OrdinalIgnoreCase ascending.
    pub top_source_opens: Vec<SourceOpenEntry>,
    pub stat_count: i64,
    pub session_write_count: i64,
    /// AR04 / ADR 0005: presentation-sink updates that had to be marshalled because they arrived off the UI thread.
    pub cross_thread_present_count: i64,
    /// Always 8 buckets, labels from [`PRESENT_BUCKET_LABELS`].
    pub present_histogram: Vec<PresentLatencyBucket>,
}

impl ReviewMetricsSnapshot {
    /// Total decoder fallbacks across all backends (C# `DecoderFallbackCount`, a computed property).
    pub fn decoder_fallback_count(&self) -> i64 {
        self.decoder_fallbacks.iter().map(|(_, n)| *n).sum()
    }
}

/// `DecoderFallbacks`: an object keyed by the backend name, in backend value order.
struct FallbacksJson<'a>(&'a [(DecoderBackend, i64)]);

impl Serialize for FallbacksJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (backend, count) in self.0 {
            map.serialize_entry(backend.name(), count)?;
        }
        map.end()
    }
}

/// `{"Path":..,"Count":..}`.
struct SourceOpenJson<'a>(&'a SourceOpenEntry);

impl Serialize for SourceOpenJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("SourceOpenEntry", 2)?;
        state.serialize_field("Path", &self.0.path)?;
        state.serialize_field("Count", &self.0.count)?;
        state.end()
    }
}

/// `{"Label":..,"Count":..}`.
struct BucketJson<'a>(&'a PresentLatencyBucket);

impl Serialize for BucketJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("PresentLatencyBucket", 2)?;
        state.serialize_field("Label", &self.0.label)?;
        state.serialize_field("Count", &self.0.count)?;
        state.end()
    }
}

impl Serialize for ReviewMetricsSnapshot {
    /// Writes the 19 properties in the C# order and PascalCase names:
    /// `CacheHits, CacheMisses, SourceBytesRead, SourceReads, DecodeMilliseconds, PresentedImages, PresentMilliseconds,
    /// PreloadHits, InflightJoins, DiskCacheHits, UiAssignMilliseconds, DecoderFallbacks (object backend-name -> count),
    /// DecoderFallbackCount, SourceOpenCount, TopSourceOpens [{Path, Count}], StatCount, SessionWriteCount,
    /// CrossThreadPresentCount, PresentHistogram [{Label, Count}]`.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let top: Vec<SourceOpenJson<'_>> = self.top_source_opens.iter().map(SourceOpenJson).collect();
        let histogram: Vec<BucketJson<'_>> = self.present_histogram.iter().map(BucketJson).collect();
        let mut state = serializer.serialize_struct("ReviewMetricsSnapshot", 19)?;
        state.serialize_field("CacheHits", &self.cache_hits)?;
        state.serialize_field("CacheMisses", &self.cache_misses)?;
        state.serialize_field("SourceBytesRead", &self.source_bytes_read)?;
        state.serialize_field("SourceReads", &self.source_reads)?;
        state.serialize_field("DecodeMilliseconds", &self.decode_milliseconds)?;
        state.serialize_field("PresentedImages", &self.presented_images)?;
        state.serialize_field("PresentMilliseconds", &self.present_milliseconds)?;
        state.serialize_field("PreloadHits", &self.preload_hits)?;
        state.serialize_field("InflightJoins", &self.inflight_joins)?;
        state.serialize_field("DiskCacheHits", &self.disk_cache_hits)?;
        state.serialize_field("UiAssignMilliseconds", &self.ui_assign_milliseconds)?;
        state.serialize_field("DecoderFallbacks", &FallbacksJson(&self.decoder_fallbacks))?;
        state.serialize_field("DecoderFallbackCount", &self.decoder_fallback_count())?;
        state.serialize_field("SourceOpenCount", &self.source_open_count)?;
        state.serialize_field("TopSourceOpens", &top)?;
        state.serialize_field("StatCount", &self.stat_count)?;
        state.serialize_field("SessionWriteCount", &self.session_write_count)?;
        state.serialize_field("CrossThreadPresentCount", &self.cross_thread_present_count)?;
        state.serialize_field("PresentHistogram", &histogram)?;
        state.end()
    }
}

/// The counters. C# `ReviewMetrics`.
#[derive(Debug)]
pub struct ReviewMetrics {
    cache_hits: AtomicI64,
    cache_misses: AtomicI64,
    source_bytes_read: AtomicI64,
    source_reads: AtomicI64,
    decode_milliseconds: AtomicI64,
    /// Bits of the EWMA `f64` (0 = no sample yet).
    decode_ewma_bits: AtomicI64,
    presented_images: AtomicI64,
    present_milliseconds: AtomicI64,
    preload_hits: AtomicI64,
    inflight_joins: AtomicI64,
    disk_cache_hits: AtomicI64,
    ui_assign_milliseconds: AtomicI64,
    source_open_count: AtomicI64,
    stat_count: AtomicI64,
    session_write_count: AtomicI64,
    cross_thread_present_count: AtomicI64,
    present_buckets: [AtomicI64; 8],
    decoder_fallbacks: Mutex<HashMap<i32, i64>>,
    /// OrdinalIgnoreCase key (`pr_core::text::ignore_case_key`) -> (first spelling of the path, count).
    source_opens: Mutex<HashMap<String, (String, i64)>>,
}

impl Default for ReviewMetrics {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ReviewMetrics {
    pub fn new() -> Self {
        Self {
            cache_hits: AtomicI64::new(0),
            cache_misses: AtomicI64::new(0),
            source_bytes_read: AtomicI64::new(0),
            source_reads: AtomicI64::new(0),
            decode_milliseconds: AtomicI64::new(0),
            decode_ewma_bits: AtomicI64::new(0),
            presented_images: AtomicI64::new(0),
            present_milliseconds: AtomicI64::new(0),
            preload_hits: AtomicI64::new(0),
            inflight_joins: AtomicI64::new(0),
            disk_cache_hits: AtomicI64::new(0),
            ui_assign_milliseconds: AtomicI64::new(0),
            source_open_count: AtomicI64::new(0),
            stat_count: AtomicI64::new(0),
            session_write_count: AtomicI64::new(0),
            cross_thread_present_count: AtomicI64::new(0),
            present_buckets: std::array::from_fn(|_| AtomicI64::new(0)),
            decoder_fallbacks: Mutex::new(HashMap::new()),
            source_opens: Mutex::new(HashMap::new()),
        }
    }

    pub fn record_preload_hit(&self) {
        self.preload_hits.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_inflight_join(&self) {
        self.inflight_joins.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_disk_cache_hit(&self) {
        self.disk_cache_hits.fetch_add(1, Ordering::SeqCst);
    }

    /// Adds `max(0, milliseconds)`.
    pub fn record_ui_assign(&self, milliseconds: i64) {
        self.ui_assign_milliseconds
            .fetch_add(milliseconds.max(0), Ordering::SeqCst);
    }

    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_decoder_fallback(&self, backend: DecoderBackend) {
        let mut map = lock(&self.decoder_fallbacks);
        let count = map.entry(backend.value()).or_insert(0);
        *count = count.wrapping_add(1);
    }

    /// `RecordSourceRead(bytes, milliseconds, includeInDecodeEwma)`: negative values count as 0; one read; adds to the
    /// byte and decode-millisecond totals; updates the EWMA only when `include_in_decode_ewma` and `milliseconds > 0`.
    pub fn record_source_read(&self, bytes: i64, milliseconds: i64, include_in_decode_ewma: bool) {
        // A negative value (clock adjustment, a caller bug) must not shrink a running total.
        let bytes = bytes.max(0);
        let milliseconds = milliseconds.max(0);
        self.source_reads.fetch_add(1, Ordering::SeqCst);
        self.source_bytes_read.fetch_add(bytes, Ordering::SeqCst);
        self.decode_milliseconds.fetch_add(milliseconds, Ordering::SeqCst);
        if include_in_decode_ewma {
            self.update_decode_ewma(milliseconds);
        }
    }

    fn update_decode_ewma(&self, milliseconds: i64) {
        if milliseconds <= 0 {
            return;
        }
        loop {
            let old_bits = self.decode_ewma_bits.load(Ordering::SeqCst);
            let old = f64::from_bits(old_bits as u64);
            let next = if old <= 0.0 {
                milliseconds as f64
            } else {
                old + DECODE_EWMA_ALPHA * (milliseconds as f64 - old)
            };
            if self
                .decode_ewma_bits
                .compare_exchange(old_bits, next.to_bits() as i64, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return;
            }
        }
    }

    /// Exponentially weighted moving average of source decode wall time in ms, or 0 before the first sample.
    pub fn decode_milliseconds_ewma(&self) -> f64 {
        f64::from_bits(self.decode_ewma_bits.load(Ordering::SeqCst) as u64)
    }

    /// `max(0, milliseconds)`: one presented image, adds the latency, increments its histogram bucket.
    pub fn record_presented(&self, milliseconds: i64) {
        let milliseconds = milliseconds.max(0);
        self.presented_images.fetch_add(1, Ordering::SeqCst);
        self.present_milliseconds.fetch_add(milliseconds, Ordering::SeqCst);
        self.present_buckets[present_bucket_index(milliseconds)].fetch_add(1, Ordering::SeqCst);
    }

    /// Counts one open of a source image file (total and per path, paths compared OrdinalIgnoreCase); trims the table
    /// when it grows past [`MAX_TRACKED_SOURCE_OPEN_PATHS`].
    pub fn record_source_open(&self, path: &str) {
        self.source_open_count.fetch_add(1, Ordering::SeqCst);
        let key = ignore_case_key(path);
        let mut table = lock(&self.source_opens);
        let entry = table.entry(key).or_insert_with(|| (path.to_owned(), 0));
        entry.1 = entry.1.wrapping_add(1);
        if table.len() > MAX_TRACKED_SOURCE_OPEN_PATHS {
            Self::trim_source_opens(&mut table);
        }
    }

    /// Drops the least-opened paths down to 3/4 of the cap, so the sort runs once per `MAX / 4` new paths, not on every
    /// open. Ties are broken by key so the result is deterministic (C#: arbitrary). The total count is unaffected.
    fn trim_source_opens(table: &mut HashMap<String, (String, i64)>) {
        let keep = MAX_TRACKED_SOURCE_OPEN_PATHS * 3 / 4;
        let excess = table.len().saturating_sub(keep);
        if excess == 0 {
            return;
        }
        let mut entries: Vec<(i64, String)> = table.iter().map(|(key, (_, count))| (*count, key.clone())).collect();
        entries.sort();
        for (_, key) in entries.into_iter().take(excess) {
            table.remove(&key);
        }
    }

    /// Distinct paths currently tracked (C# internal `TrackedSourceOpenPaths`, a test/diagnostic seam).
    pub fn tracked_source_open_paths(&self) -> usize {
        lock(&self.source_opens).len()
    }

    pub fn record_stat(&self) {
        self.stat_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_session_write(&self) {
        self.session_write_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_cross_thread_present(&self) {
        self.cross_thread_present_count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn snapshot(&self) -> ReviewMetricsSnapshot {
        let mut fallbacks: Vec<(i32, i64)> = lock(&self.decoder_fallbacks).iter().map(|(k, v)| (*k, *v)).collect();
        fallbacks.sort_by_key(|(value, _)| *value);
        let decoder_fallbacks = fallbacks
            .into_iter()
            .filter_map(|(value, count)| {
                DecoderBackend::MEMBERS
                    .iter()
                    .copied()
                    .find(|member| member.value() == value)
                    .map(|member| (member, count))
            })
            .collect();

        let mut opens: Vec<SourceOpenEntry> = lock(&self.source_opens)
            .values()
            .map(|(path, count)| SourceOpenEntry {
                path: path.clone(),
                count: *count,
            })
            .collect();
        opens.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| cmp_ignore_case(&a.path, &b.path)));
        opens.truncate(10);

        let present_histogram = PRESENT_BUCKET_LABELS
            .iter()
            .zip(self.present_buckets.iter())
            .map(|(label, count)| PresentLatencyBucket {
                label: (*label).to_owned(),
                count: count.load(Ordering::SeqCst),
            })
            .collect();

        ReviewMetricsSnapshot {
            cache_hits: self.cache_hits.load(Ordering::SeqCst),
            cache_misses: self.cache_misses.load(Ordering::SeqCst),
            source_bytes_read: self.source_bytes_read.load(Ordering::SeqCst),
            source_reads: self.source_reads.load(Ordering::SeqCst),
            decode_milliseconds: self.decode_milliseconds.load(Ordering::SeqCst),
            presented_images: self.presented_images.load(Ordering::SeqCst),
            present_milliseconds: self.present_milliseconds.load(Ordering::SeqCst),
            preload_hits: self.preload_hits.load(Ordering::SeqCst),
            inflight_joins: self.inflight_joins.load(Ordering::SeqCst),
            disk_cache_hits: self.disk_cache_hits.load(Ordering::SeqCst),
            ui_assign_milliseconds: self.ui_assign_milliseconds.load(Ordering::SeqCst),
            decoder_fallbacks,
            source_open_count: self.source_open_count.load(Ordering::SeqCst),
            top_source_opens: opens,
            stat_count: self.stat_count.load(Ordering::SeqCst),
            session_write_count: self.session_write_count.load(Ordering::SeqCst),
            cross_thread_present_count: self.cross_thread_present_count.load(Ordering::SeqCst),
            present_histogram,
        }
    }
}

/// Index (0..=7) of the histogram bucket for `milliseconds` (already clamped to >= 0). C# `PresentBucketIndex`.
pub fn present_bucket_index(milliseconds: i64) -> usize {
    PRESENT_BUCKET_UPPER_BOUNDS_MS
        .iter()
        .position(|bound| milliseconds <= *bound)
        .unwrap_or(PRESENT_BUCKET_UPPER_BOUNDS_MS.len())
}
