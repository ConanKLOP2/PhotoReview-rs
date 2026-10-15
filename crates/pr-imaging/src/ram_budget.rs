//! Port of `PhotoReview.Imaging/Preload/RamBudgetPolicy.cs` (C# baseline d92d6aae): decoded-image memory estimates,
//! cache-share clamps and the whole-folder preload decision, plus the internal `FolderEstimateCache`.
//!
//! Pure integer / `f64` arithmetic. Integer operations that are unchecked in C# (the default context) use `wrapping_*`;
//! the one `checked(...)` block in C# panics here (see [`estimate_folder_preview_bytes`]). The OS query for physical
//! memory is behind [`PhysicalMemorySource`] (the Windows implementation belongs to `pr-platform-win` / W4-07b).

#![allow(unused_variables, dead_code)] // D11: remove with the last `todo!()`.

use std::fmt;
use std::sync::Arc;

use pr_core::abstractions::MemoryProbe;
use pr_core::catalog::types::CatalogEntry;

use crate::decode_types::DecodeBox;
use crate::preload_order::PreloadWindow;

// ---- constants (public C# consts) ----
/// Compressed bytes to decoded bytes for non-RAW files without dimensions in an unbounded box.
pub const JPEG_EXPANSION_FACTOR: f64 = 10.0;
/// Decoded bytes per pixel used for every geometry estimate.
pub const DEFAULT_AVERAGE_BYTES_PER_PIXEL: f64 = 4.0;
/// Per-axis upper bound of an embedded RAW preview relative to the container's sensor size (Canon sRAW/mRAW).
pub const RAW_PREVIEW_SENSOR_SIZE_FACTOR: i32 = 2;
/// Decoded RAW preview bytes per compressed RAW byte when only the RAW file length is known.
pub const RAW_COMPRESSED_TO_PREVIEW_FACTOR: f64 = 6.0;
/// Largest share of physical RAM a single in-memory cache budget may claim (IMG-11).
pub const MAX_PHYSICAL_MEMORY_SHARE: f64 = 0.5;
/// Largest share of physical RAM the optional source-bytes cache may claim (R2-A-06).
pub const MAX_SOURCE_BYTES_SHARE: f64 = 0.2;
/// Decode box (UHD) the minimum cache percent is sized for.
pub const MINIMUM_BUDGET_BOX_WIDTH: i32 = 3840;
/// See [`MINIMUM_BUDGET_BOX_WIDTH`].
pub const MINIMUM_BUDGET_BOX_HEIGHT: i32 = 2160;
/// Q-R17: margin on the measured mean preview size.
pub const MEASURED_PREVIEW_MARGIN: f64 = 1.25;

/// C# `ArgumentOutOfRangeException` from `ThrowIfNegative` / `ThrowIfNegativeOrZero`; `param_name` is the C# parameter name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgumentOutOfRange {
    pub param_name: &'static str,
}

impl fmt::Display for ArgumentOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "argument out of range: {}", self.param_name)
    }
}

impl std::error::Error for ArgumentOutOfRange {}

/// C# `GC.GetGCMemoryInfo().TotalAvailableMemoryBytes` seam: physical RAM, or the container hard limit; `0` when unknown.
pub trait PhysicalMemorySource: Send + Sync {
    fn total_available_memory_bytes(&self) -> i64;
}

/// C# `GetPhysicalMemoryBytes()`.
pub fn get_physical_memory_bytes(source: &dyn PhysicalMemorySource) -> i64 {
    todo!()
}

/// C# `ClampToPhysicalMemory`: `min(requested, (long)(physical * 0.5))`; `physical <= 0` returns `requested`.
pub fn clamp_to_physical_memory(requested_bytes: i64, physical_bytes: i64) -> i64 {
    todo!()
}

/// C# `MinimumPreviewWindowBytes`: `(long)ImageCount * 3840 * 2160 * 4` (long arithmetic, unchecked).
pub fn minimum_preview_window_bytes(window: PreloadWindow) -> i64 {
    todo!()
}

/// C# `MinimumCachePercent`: ceiling percent that holds the window, clamped to `[1, 90]`; unknown RAM gives `1`.
pub fn minimum_cache_percent(physical_bytes: i64, window: PreloadWindow) -> i32 {
    todo!()
}

/// C# `ClampCachePercent`: `clamp(requested, minimum_cache_percent(..), 90)`.
pub fn clamp_cache_percent(requested_percent: i32, physical_bytes: i64, window: PreloadWindow) -> i32 {
    todo!()
}

/// C# `BytesForPercent`: `physical / 100 * percent + physical % 100 * percent / 100`; `0` when either is `<= 0`.
pub fn bytes_for_percent(percent: i32, physical_bytes: i64) -> i64 {
    todo!()
}

/// C# `PreviewBytesForPercent`. Errors: `physical_bytes <= 0` => `ArgumentOutOfRange { "physicalBytes" }`.
pub fn preview_bytes_for_percent(
    requested_percent: i32,
    physical_bytes: i64,
    source_bytes_capacity: i64,
    window: PreloadWindow,
) -> Result<i64, ArgumentOutOfRange> {
    todo!()
}

/// C# `SourceBytesForPercent`; unknown physical RAM returns the request unchanged.
pub fn source_bytes_for_percent(
    requested_bytes: i64,
    requested_percent: i32,
    physical_bytes: i64,
    window: PreloadWindow,
) -> i64 {
    todo!()
}

/// C# `ClampSourceBytesToPhysicalMemory`: `min(clamp_to_physical_memory(..), (long)(physical * 0.2))`.
pub fn clamp_source_bytes_to_physical_memory(requested_bytes: i64, physical_bytes: i64) -> i64 {
    todo!()
}

/// C# `ClampPreviewToPhysicalMemory` (R2-A-06).
pub fn clamp_preview_to_physical_memory(requested_bytes: i64, physical_bytes: i64, source_bytes_capacity: i64) -> i64 {
    todo!()
}

/// C# `EstimateFolderPreviewBytes`. `i64::MAX` means "do not preload the whole folder".
///
/// # Panics
/// Where C# throws `OverflowException` (`checked(width * 2)` / `checked(height * 2)` for a RAW entry whose stored
/// dimension exceeds `i32::MAX / 2`): panics with `"arithmetic operation overflowed"`.
pub fn estimate_folder_preview_bytes(
    entries: &[CatalogEntry],
    target_box: DecodeBox,
    measured_mean_preview_bytes: Option<f64>,
) -> i64 {
    todo!()
}

/// C# `ShouldPreloadWholeFolderEstimate`. Errors: `estimated_bytes < 0` => `ArgumentOutOfRange { "estimatedBytes" }`.
pub fn should_preload_whole_folder_estimate(
    estimated_bytes: i64,
    capacity_bytes: i64,
    memory_probe: &dyn MemoryProbe,
    reserve_bytes: i64,
    maximum_load: f64,
) -> Result<bool, ArgumentOutOfRange> {
    todo!()
}

/// C# internal `FolderEstimateCache`: remembers the last estimate for one catalog snapshot (compared by `Arc` identity,
/// C# `ReferenceEquals`), decode box and measured mean (`Option<f64>` equality: NaN never matches, as in C#).
#[derive(Debug, Default)]
pub(crate) struct FolderEstimateCache {
    entries: Option<Arc<[CatalogEntry]>>,
    decode_box: DecodeBox,
    measured: Option<f64>,
    estimate: i64,
    compute_count: u32,
}

impl FolderEstimateCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// C# `ComputeCount` (test seam).
    pub(crate) fn compute_count(&self) -> u32 {
        self.compute_count
    }

    /// C# `GetOrCompute`.
    pub(crate) fn get_or_compute(
        &mut self,
        entries: &Arc<[CatalogEntry]>,
        decode_box: DecodeBox,
        measured_mean_preview_bytes: Option<f64>,
    ) -> i64 {
        todo!()
    }
}

#[cfg(test)]
mod stub_tests {
    // The implementation imports these two at module level (clamps to [MIN, MAX]).
    use pr_core::settings::perf_options::{MAX_IMAGE_CACHE_RAM_PERCENT, MIN_IMAGE_CACHE_RAM_PERCENT};

    #[test]
    fn constants_match_csharp() {
        assert_eq!(MAX_IMAGE_CACHE_RAM_PERCENT, 90);
        assert_eq!(MIN_IMAGE_CACHE_RAM_PERCENT, 1);
    }
}

#[cfg(test)]
mod folder_estimate_cache_tests {
    use super::*;

    /// C# `RawFolder(count, length)`: a fresh snapshot each call (distinct `Arc`, so never `ReferenceEquals`).
    fn raw_folder(count: usize, length: i64) -> Arc<[CatalogEntry]> {
        (0..count)
            .map(|index| {
                let mut entry =
                    CatalogEntry::new(format!("folder/IMG_{index:06}.CR2")).expect("test path is not blank");
                entry.length = Some(length);
                entry
            })
            .collect::<Vec<_>>()
            .into()
    }

    // C#: RamBudgetPolicyRawLengthTests.FolderEstimateCache_SameInputs_ComputesOnce_AndRecomputesWhenAnyInputChanges
    #[test]
    fn folder_estimate_cache_same_inputs_compute_once_and_recompute_when_any_input_changes() {
        let mut cache = FolderEstimateCache::new();
        let entries = raw_folder(50, 25_000_000);
        let decode_box = DecodeBox::new(1920, 1080);

        let first = cache.get_or_compute(&entries, decode_box, None);
        let again = cache.get_or_compute(&entries, decode_box, None);
        assert_eq!(first, again);
        assert_eq!(cache.compute_count(), 1);

        cache.get_or_compute(&entries, decode_box, Some(1_000_000.0)); // calibration changed
        cache.get_or_compute(&entries, DecodeBox::UNBOUNDED, Some(1_000_000.0)); // mode (box) changed
        cache.get_or_compute(&raw_folder(50, 25_000_000), DecodeBox::UNBOUNDED, Some(1_000_000.0)); // different snapshot
        assert_eq!(cache.compute_count(), 4);
    }
}
