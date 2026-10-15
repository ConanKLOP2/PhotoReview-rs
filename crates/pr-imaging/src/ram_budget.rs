//! Port of `PhotoReview.Imaging/Preload/RamBudgetPolicy.cs` (C# baseline d92d6aae): decoded-image memory estimates,
//! cache-share clamps and the whole-folder preload decision, plus the internal `FolderEstimateCache`.
//!
//! Pure integer / `f64` arithmetic. Integer operations that are unchecked in C# (the default context) use `wrapping_*`;
//! the one `checked(...)` block in C# panics here (see [`estimate_folder_preview_bytes`]). The OS query for physical
//! memory is behind [`PhysicalMemorySource`] (the Windows implementation belongs to `pr-platform-win` / W4-07b).

use std::fmt;
use std::sync::Arc;

use pr_core::abstractions::MemoryProbe;
use pr_core::catalog::types::{CatalogEntry, is_raw_path};
use pr_core::settings::perf_options::{MAX_IMAGE_CACHE_RAM_PERCENT, MIN_IMAGE_CACHE_RAM_PERCENT};

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
    source.total_available_memory_bytes()
}

/// C# `ClampToPhysicalMemory`: `min(requested, (long)(physical * 0.5))`; `physical <= 0` returns `requested`.
pub fn clamp_to_physical_memory(requested_bytes: i64, physical_bytes: i64) -> i64 {
    if physical_bytes <= 0 {
        return requested_bytes;
    }
    requested_bytes.min((physical_bytes as f64 * MAX_PHYSICAL_MEMORY_SHARE) as i64)
}

/// C# `MinimumPreviewWindowBytes`: `(long)ImageCount * 3840 * 2160 * 4` (long arithmetic, unchecked).
pub fn minimum_preview_window_bytes(window: PreloadWindow) -> i64 {
    // C# ImageCount is an unchecked int sum (wraps); the product below is long arithmetic, left to right.
    let image_count = window.forward.wrapping_add(window.backward).wrapping_add(1);
    i64::from(image_count)
        .wrapping_mul(i64::from(MINIMUM_BUDGET_BOX_WIDTH))
        .wrapping_mul(i64::from(MINIMUM_BUDGET_BOX_HEIGHT))
        .wrapping_mul(4)
}

/// C# `MinimumCachePercent`: ceiling percent that holds the window, clamped to `[1, 90]`; unknown RAM gives `1`.
pub fn minimum_cache_percent(physical_bytes: i64, window: PreloadWindow) -> i32 {
    if physical_bytes <= 0 {
        return MIN_IMAGE_CACHE_RAM_PERCENT;
    }
    let min_bytes = minimum_preview_window_bytes(window);
    let percent = min_bytes.wrapping_mul(100).wrapping_add(physical_bytes).wrapping_sub(1) / physical_bytes;
    // Clamp on i64 BEFORE narrowing (C# Math.Clamp(long, ..)).
    percent
        .max(i64::from(MIN_IMAGE_CACHE_RAM_PERCENT))
        .min(i64::from(MAX_IMAGE_CACHE_RAM_PERCENT)) as i32
}

/// C# `ClampCachePercent`: `clamp(requested, minimum_cache_percent(..), 90)`.
pub fn clamp_cache_percent(requested_percent: i32, physical_bytes: i64, window: PreloadWindow) -> i32 {
    let minimum = minimum_cache_percent(physical_bytes, window);
    requested_percent.max(minimum).min(MAX_IMAGE_CACHE_RAM_PERCENT)
}

/// C# `BytesForPercent`: `physical / 100 * percent + physical % 100 * percent / 100`; `0` when either is `<= 0`.
pub fn bytes_for_percent(percent: i32, physical_bytes: i64) -> i64 {
    if physical_bytes <= 0 || percent <= 0 {
        return 0;
    }
    let percent = i64::from(percent);
    (physical_bytes / 100)
        .wrapping_mul(percent)
        .wrapping_add((physical_bytes % 100).wrapping_mul(percent) / 100)
}

/// C# `PreviewBytesForPercent`. Errors: `physical_bytes <= 0` => `ArgumentOutOfRange { "physicalBytes" }`.
pub fn preview_bytes_for_percent(
    requested_percent: i32,
    physical_bytes: i64,
    source_bytes_capacity: i64,
    window: PreloadWindow,
) -> Result<i64, ArgumentOutOfRange> {
    if physical_bytes <= 0 {
        return Err(ArgumentOutOfRange {
            param_name: "physicalBytes",
        });
    }
    let percent = clamp_cache_percent(requested_percent, physical_bytes, window);
    let budget = bytes_for_percent(percent, physical_bytes);
    Ok(budget.wrapping_sub(source_bytes_capacity.max(0)).max(1))
}

/// C# `SourceBytesForPercent`; unknown physical RAM returns the request unchanged.
pub fn source_bytes_for_percent(
    requested_bytes: i64,
    requested_percent: i32,
    physical_bytes: i64,
    window: PreloadWindow,
) -> i64 {
    if physical_bytes <= 0 {
        return requested_bytes;
    }
    let percent = clamp_cache_percent(requested_percent, physical_bytes, window);
    let budget = bytes_for_percent(percent, physical_bytes);
    let room = budget.wrapping_sub(minimum_preview_window_bytes(window)).max(0);
    clamp_source_bytes_to_physical_memory(requested_bytes, physical_bytes).min(room)
}

/// C# `ClampSourceBytesToPhysicalMemory`: `min(clamp_to_physical_memory(..), (long)(physical * 0.2))`.
pub fn clamp_source_bytes_to_physical_memory(requested_bytes: i64, physical_bytes: i64) -> i64 {
    if physical_bytes <= 0 {
        return requested_bytes;
    }
    clamp_to_physical_memory(requested_bytes, physical_bytes)
        .min((physical_bytes as f64 * MAX_SOURCE_BYTES_SHARE) as i64)
}

/// C# `ClampPreviewToPhysicalMemory` (R2-A-06).
pub fn clamp_preview_to_physical_memory(requested_bytes: i64, physical_bytes: i64, source_bytes_capacity: i64) -> i64 {
    let clamped = clamp_to_physical_memory(requested_bytes, physical_bytes);
    if physical_bytes <= 0 {
        return clamped;
    }
    let remaining = clamp_to_physical_memory(i64::MAX, physical_bytes)
        .wrapping_sub(source_bytes_capacity.max(0))
        .max(0);
    clamped.min(remaining)
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
    let bounded = target_box.width > 0 && target_box.height > 0;
    let box_bound = if bounded {
        f64::from(target_box.width) * f64::from(target_box.height) * DEFAULT_AVERAGE_BYTES_PER_PIXEL
    } else {
        0.0
    };
    let long_max = i64::MAX as f64;

    if let Some(measured) = measured_mean_preview_bytes.filter(|value| *value > 0.0) {
        let mut calibrated = measured * MEASURED_PREVIEW_MARGIN;
        if bounded && measured <= box_bound {
            calibrated = calibrated.min(box_bound);
        }
        let all = (entries.len() as f64 * calibrated).ceil();
        return if all >= long_max { i64::MAX } else { all as i64 };
    }

    let mut total = 0.0_f64;
    for entry in entries {
        let mut width = entry.width.unwrap_or(0);
        let mut height = entry.height.unwrap_or(0);
        let is_raw = is_raw_path(entry.path());
        let estimate = if width > 0 && height > 0 && (bounded || is_raw) {
            if is_raw {
                // C# `checked(width * 2)`: OverflowException.
                width = width
                    .checked_mul(RAW_PREVIEW_SENSOR_SIZE_FACTOR)
                    .expect("arithmetic operation overflowed");
                height = height
                    .checked_mul(RAW_PREVIEW_SENSOR_SIZE_FACTOR)
                    .expect("arithmetic operation overflowed");
            }
            let scale_w = if target_box.width > 0 {
                f64::from(target_box.width) / f64::from(width)
            } else {
                1.0
            };
            let scale_h = if target_box.height > 0 {
                f64::from(target_box.height) / f64::from(height)
            } else {
                1.0
            };
            let scale = 1.0_f64.min(scale_w.min(scale_h));
            (f64::from(width) * scale).ceil() * (f64::from(height) * scale).ceil() * DEFAULT_AVERAGE_BYTES_PER_PIXEL
        } else if bounded {
            box_bound
        } else if is_raw {
            let length = entry.length.unwrap_or(0);
            if length <= 0 {
                return i64::MAX;
            }
            length as f64 * RAW_COMPRESSED_TO_PREVIEW_FACTOR
        } else {
            entry.length.unwrap_or(0).max(0) as f64 * JPEG_EXPANSION_FACTOR
        };
        total += estimate;
        if total >= long_max {
            return i64::MAX;
        }
    }
    total.ceil() as i64
}

/// C# `ShouldPreloadWholeFolderEstimate`. Errors: `estimated_bytes < 0` => `ArgumentOutOfRange { "estimatedBytes" }`.
pub fn should_preload_whole_folder_estimate(
    estimated_bytes: i64,
    capacity_bytes: i64,
    memory_probe: &dyn MemoryProbe,
    reserve_bytes: i64,
    maximum_load: f64,
) -> Result<bool, ArgumentOutOfRange> {
    if estimated_bytes < 0 {
        return Err(ArgumentOutOfRange {
            param_name: "estimatedBytes",
        });
    }
    Ok(estimated_bytes <= capacity_bytes && memory_probe.has_headroom(maximum_load, reserve_bytes))
}

/// C# internal `FolderEstimateCache`: remembers the last estimate for one catalog snapshot (compared by `Arc` identity,
/// C# `ReferenceEquals`), decode box and measured mean (`Option<f64>` equality: NaN never matches, as in C#).
// No non-test caller until the W4-15 scheduler lands.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct FolderEstimateCache {
    entries: Option<Arc<[CatalogEntry]>>,
    decode_box: DecodeBox,
    measured: Option<f64>,
    estimate: i64,
    compute_count: u32,
}

#[allow(dead_code)] // same justification as the struct
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
        let hit = self.entries.as_ref().is_some_and(|stored| Arc::ptr_eq(stored, entries))
            && self.decode_box == decode_box
            && self.measured == measured_mean_preview_bytes;
        if hit {
            return self.estimate;
        }
        let estimate = estimate_folder_preview_bytes(entries, decode_box, measured_mean_preview_bytes);
        self.entries = Some(Arc::clone(entries));
        self.decode_box = decode_box;
        self.measured = measured_mean_preview_bytes;
        self.estimate = estimate;
        self.compute_count += 1;
        estimate
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
