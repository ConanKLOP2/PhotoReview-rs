//! Port of `PhotoReview.Imaging/Preload/PreloadBusyBackoff.cs` and `Preload/PreviewSizeSampler.cs`
//! (C# baseline d92d6aae, PARITY I-11 "Busy back-off: 3 passes, at most 5 retries").
//!
//! Two tiny thread-safe helpers of the preload scheduler:
//! - [`PreloadBusyBackoff`]: per-path cooldown (counted in order passes, never wall clock) and retry cap for paths
//!   whose preload ended `Busy`;
//! - [`PreviewSizeSampler`]: running mean of decoded preview bytes at one decode box.
//!
//! No I/O, no clock, no globals. Interior mutability is a `std::sync::Mutex` (C# `lock`); the types are `Send + Sync`.

#![allow(unused_variables, dead_code)] // D11: remove with the last `todo!()`.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::cache_key::ImageCacheKey;
use crate::decode_types::DecodeBox;

/// A path is skipped for the next this-many order passes after a busy outcome (C# `CooldownPasses`).
pub const COOLDOWN_PASSES: i32 = 3;
/// After this many RETRIES (so `MAX_RETRIES + 1` busy outcomes) a path is left alone until its key changes.
pub const MAX_RETRIES: i32 = 5;

/// Per-path busy history; C# `private readonly record struct State(ImageCacheKey Key, int LastPass, int BusyCount)`.
#[derive(Debug, Clone)]
struct State {
    key: ImageCacheKey,
    last_pass: i32,
    busy_count: i32,
}

#[derive(Debug, Default)]
struct BackoffInner {
    /// Keyed by `pr_core::text::ignore_case_key(path)` (C# `StringComparer.OrdinalIgnoreCase`).
    states: HashMap<String, State>,
    /// C# `int _pass`; wraps on overflow like the unchecked C# `++`.
    pass: i32,
}

/// Backoff for paths whose preload ended `Busy` (the RAW decoder's bounded queue refused the background decode).
/// Time is counted in order passes, not wall clock, so it is deterministic. The C# type is `internal`; it is `pub`
/// here so the integration tests (C# `InternalsVisibleTo`) and the scheduler (W4-15) can use it.
#[derive(Debug, Default)]
pub struct PreloadBusyBackoff {
    inner: Mutex<BackoffInner>,
}

impl PreloadBusyBackoff {
    /// New backoff: no history, pass counter 0.
    pub fn new() -> Self {
        todo!("Self::default()")
    }

    /// C# `BeginPass()`: marks the start of an order pass (the order was rebuilt): `pass = pass.wrapping_add(1)`.
    pub fn begin_pass(&self) {
        todo!("lock; pass += 1")
    }

    /// C# `NoteBusy(path, key)`: records a busy outcome and returns how many times this path (as this exact file, i.e.
    /// an equal `key`) has now been busy: `previous + 1` when a state exists with an equal key, else `1`. The stored
    /// state becomes `(key, current pass, count)`.
    pub fn note_busy(&self, path: &str, key: &ImageCacheKey) -> i32 {
        todo!("lock; equal key => busy_count + 1 else 1; store State{{key, pass, count}}")
    }

    /// C# `RecordSuccess(path)`: a preload of the path succeeded; drop its history (the retry cap must not outlive a
    /// good decode). No effect on an unknown path.
    pub fn record_success(&self, path: &str) {
        todo!("lock; remove")
    }

    /// C# `Clear()`: forget every path (folder reload / cache clear). The pass counter is NOT reset.
    pub fn clear(&self) {
        todo!("lock; states.clear()")
    }

    /// C# `ShouldSkip(path, key)`: `false` for an unknown path; when the stored key differs from `key` the state is
    /// REMOVED (the file changed: a fresh start) and `false` is returned; otherwise
    /// `busy_count > MAX_RETRIES || pass - last_pass <= COOLDOWN_PASSES` (wrapping subtraction).
    pub fn should_skip(&self, path: &str, key: &ImageCacheKey) -> bool {
        todo!("lock; lookup; key mismatch removes the entry; else cap or cooldown test")
    }

    /// Test seam for the C# test `BeginPassAndNoteBusy_Concurrent_...`, which reads the private `_pass` field by
    /// reflection. Returns the current pass counter.
    pub fn pass_for_tests(&self) -> i32 {
        todo!("lock; pass")
    }
}

#[derive(Debug, Default)]
struct SamplerInner {
    /// C# `_box` starts as `default` = `DecodeBox(0, 0)`.
    decode_box: DecodeBox,
    total_bytes: i64,
    count: i32,
}

/// Samples needed before the mean replaces the box upper bound (C# `PreviewSizeSampler.MinimumSamples`).
pub const MINIMUM_SAMPLES: i32 = 8;

/// Q-R17: running mean of decoded preview sizes at one decode box, fed by preload as images land in the RAM cache.
/// A sample at a different box restarts the mean. Thread-safe (preload workers record while the scheduler reads).
#[derive(Debug, Default)]
pub struct PreviewSizeSampler {
    inner: Mutex<SamplerInner>,
}

impl PreviewSizeSampler {
    /// New sampler: box `0x0`, no samples.
    pub fn new() -> Self {
        todo!("Self::default()")
    }

    /// C# `Record(box, bytes)`: `bytes <= 0` is ignored. A box different from the stored one replaces it and resets
    /// total and count; then `total += bytes` (wrapping like unchecked C#), `count += 1`.
    pub fn record(&self, decode_box: DecodeBox, bytes: i64) {
        todo!("ignore non-positive; reset on box change; accumulate")
    }

    /// C# `MeanBytes(box)`: `Some(total as f64 / count as f64)` when `box` equals the stored box and
    /// `count >= MINIMUM_SAMPLES`, else `None`.
    pub fn mean_bytes(&self, decode_box: DecodeBox) -> Option<f64> {
        todo!("box equal and count >= MINIMUM_SAMPLES => mean")
    }
}
