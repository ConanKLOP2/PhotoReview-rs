//! Port tests for `pr_imaging::ram_budget` (C# `PhotoReview.Imaging/Preload/RamBudgetPolicy.cs`, baseline d92d6aae).
//!
//! Ported C# classes (`tests/PhotoReview.Imaging.Tests/`): `RamBudgetPolicyTests`, `RamBudgetPolicyEstimateGapTests`,
//! `RamBudgetPolicyMutationTests`, `RamBudgetPolicyRawLengthTests` (the `FolderEstimateCache` test lives in
//! `src/ram_budget.rs`, because that type is `pub(crate)`), `RamBudgetPolicyRawPreviewTests`, `RamCachePercentTests`
//! (policy-only rows), `RamBudgetPropertyTests`, `Properties/RamBudgetShareAndWindowPropertyTests` (deterministic
//! xorshift RNG instead of `Random`/FsCheck), `MemoryBudgetClampTests` (policy-only halves), plus the golden tests over
//! `tests/golden/ram-budget-{scalar,estimate}.json` with inputs from `tests/golden-inputs/`.
//!
//! Not ported here (they call `PreviewImageService`, `SourceBytesCache`, `PreloadWindow.FromSettings` or the physical
//! RAM query): see the skip list in the PR body (W4-14, W4-10a, W4-05b, Windows probe).

use std::panic::{self, AssertUnwindSafe};

use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_core::catalog::types::CatalogEntry;
use pr_core::settings::perf_options::{
    MAX_IMAGE_CACHE_RAM_PERCENT, MAX_PRELOAD_COUNT, MEMORY_RESERVE_BYTES, MIN_IMAGE_CACHE_RAM_PERCENT,
    MIN_PRELOAD_BACKWARD_COUNT, MIN_PRELOAD_FORWARD_COUNT, PRELOAD_MEMORY_LOAD_LIMIT,
};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::preload_order::PreloadWindow;
use pr_imaging::ram_budget::{
    ArgumentOutOfRange, bytes_for_percent, clamp_cache_percent, clamp_preview_to_physical_memory,
    clamp_source_bytes_to_physical_memory, clamp_to_physical_memory, estimate_folder_preview_bytes,
    minimum_cache_percent, minimum_preview_window_bytes, preview_bytes_for_percent,
    should_preload_whole_folder_estimate, source_bytes_for_percent,
};
use pr_test_support::golden;
use serde_json::{Value, json};

const GIB: i64 = 1024 * 1024 * 1024;
const UHD_PREVIEW_BYTES: i64 = 3840 * 2160 * 4;
const SRAW_PREVIEW_BYTES: i64 = 5184 * 3456 * 4;

/// Memory probe with a fixed answer (C# `FakeMemoryProbe(bool)`): `true` = headroom, `false` = none.
#[derive(Debug)]
struct FixedProbe(bool);

impl MemoryProbe for FixedProbe {
    fn has_headroom(&self, _maximum_load: f64, _reserve_bytes: i64) -> bool {
        self.0
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        None
    }
}

/// Deterministic xorshift64 RNG for the property tests (replaces C# `Random(seed)`; no external crate).
struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in `[lo, hi)`.
    fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(lo < hi, "empty range {lo}..{hi}");
        let span = (i128::from(hi) - i128::from(lo)) as u128;
        let offset = (u128::from(self.next_u64()) % span) as i64;
        lo + offset
    }

    /// Uniform in `[lo, hi)`.
    fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        self.range_i64(i64::from(lo), i64::from(hi)) as i32
    }

    /// Mostly random, with the usual edge values mixed in (C# `PropertyRunner.EdgyLong`).
    fn edgy_long(&mut self) -> i64 {
        match self.next_u64() % 10 {
            0 => 0,
            1 => -1,
            2 => 1,
            3 => i64::MAX,
            4 => i64::MIN,
            5 => i64::MAX / 2,
            _ => self.next_u64() as i64,
        }
    }
}

/// Physical RAM sizes used by the property tests (C# `RandomPhysical` / `Physical`).
fn random_physical(rng: &mut XorShift) -> i64 {
    match rng.range_i64(0, 6) {
        0 => rng.range_i64(1, 1024),
        1 => rng.range_i64(1 << 20, 1 << 34),
        2 => rng.range_i64(1 << 34, 1 << 42),
        3 => i64::MAX - rng.range_i64(0, 5),
        4 => rng.range_i64(1 << 50, i64::MAX),
        _ => 32 * GIB,
    }
}

fn random_window(rng: &mut XorShift) -> PreloadWindow {
    PreloadWindow::new(
        rng.range_i32(MIN_PRELOAD_FORWARD_COUNT, MAX_PRELOAD_COUNT + 1),
        rng.range_i32(MIN_PRELOAD_BACKWARD_COUNT, MAX_PRELOAD_COUNT + 1),
    )
}

/// `new CatalogEntry(path) { Length, Width, Height }` (C# object initialiser).
fn entry(path: &str, length: Option<i64>, width: Option<i32>, height: Option<i32>) -> CatalogEntry {
    let mut entry = CatalogEntry::new(path).expect("test paths are not blank");
    entry.length = length;
    entry.width = width;
    entry.height = height;
    entry
}

/// `new CatalogEntry(path).WithMetadata(length, epoch, width, height)`.
fn with_meta(path: &str, length: i64, width: Option<i32>, height: Option<i32>) -> CatalogEntry {
    entry(path, Some(length), width, height)
}

/// C# `Jpegs(count, length, width, height)`.
fn jpegs(count: usize, length: i64, width: i32, height: i32) -> Vec<CatalogEntry> {
    (0..count)
        .map(|index| with_meta(&format!("photo-{index:03}.jpg"), length, Some(width), Some(height)))
        .collect()
}

/// C# `RawFolder(count, length)` (no dimensions, upper-case extension).
fn raw_folder(count: usize, length: i64) -> Vec<CatalogEntry> {
    (0..count).map(|index| raw(index, length)).collect()
}

fn raw(index: usize, length: i64) -> CatalogEntry {
    with_meta(&format!("folder/IMG_{index:06}.CR2"), length, None, None)
}

fn sraw_entries(count: usize) -> Vec<CatalogEntry> {
    (0..count)
        .map(|index| {
            with_meta(
                &format!("sraw-{index:03}.cr2"),
                10_000_000 + index as i64,
                Some(2592),
                Some(1728),
            )
        })
        .collect()
}

// ---- RamBudgetPolicyTests ------------------------------------------------------------------------------------------

// C#: RamBudgetPolicyTests.WholeFolderRequiresCapacityAndMemoryHeadroom
#[test]
fn whole_folder_requires_capacity_and_memory_headroom() {
    let with_headroom = FixedProbe(true);
    let without_headroom = FixedProbe(false);
    let load = 0.80;

    assert!(
        should_preload_whole_folder_estimate(1_000, 2_000, &with_headroom, MEMORY_RESERVE_BYTES, load).expect("valid")
    );
    assert!(
        should_preload_whole_folder_estimate(1_000, 1_000, &with_headroom, MEMORY_RESERVE_BYTES, load).expect("valid")
    );
    assert!(
        !should_preload_whole_folder_estimate(1_000, 999, &with_headroom, MEMORY_RESERVE_BYTES, load).expect("valid")
    );
    assert!(
        !should_preload_whole_folder_estimate(1_000, 2_000, &without_headroom, MEMORY_RESERVE_BYTES, load)
            .expect("valid")
    );
}

// C#: RamBudgetPolicyTests.RawOriginalEstimateWithoutDimensionsScalesTheCompressedFileLengthByTheDocumentedFactor
#[test]
fn raw_original_estimate_without_dimensions_scales_the_compressed_file_length_by_the_documented_factor() {
    let small = with_meta("small.cr3", 1_000, None, None);
    let large = with_meta("large.cr3", 100_000_000, None, None);

    assert_eq!(
        1_000 * 6,
        estimate_folder_preview_bytes(&[small], DecodeBox::UNBOUNDED, None)
    );
    assert_eq!(
        100_000_000 * 6,
        estimate_folder_preview_bytes(&[large], DecodeBox::UNBOUNDED, None)
    );
}

// C#: RamBudgetPolicyTests.RawFolderEstimateUsesPerFileDimensionsAtTheDecodeBox
#[test]
fn raw_folder_estimate_uses_per_file_dimensions_at_the_decode_box() {
    const IMAGE_COUNT: i64 = 200;
    let entries: Vec<CatalogEntry> = (0..IMAGE_COUNT)
        .map(|index| {
            with_meta(
                &format!("image-{index:03}.arw"),
                20_000_000 + index,
                Some(4000),
                Some(3000),
            )
        })
        .collect();

    let estimate = estimate_folder_preview_bytes(&entries, DecodeBox::new(2048, 1536), None);
    let measured = IMAGE_COUNT * 2048 * 1536 * 4;

    assert!(
        estimate >= (measured as f64 * 0.8) as i64,
        "estimate {estimate} below 80% of {measured}"
    );
    assert!(
        estimate <= (measured as f64 * 1.2) as i64,
        "estimate {estimate} above 120% of {measured}"
    );
}

// C#: RamBudgetPolicyTests.EntriesEstimate_UnboundedBoxWithKnownDimensions_KeepsTheCompressedTimesTenEstimateForNonRawFiles
#[test]
fn entries_estimate_unbounded_box_with_known_dimensions_keeps_the_compressed_times_ten_estimate_for_non_raw_files() {
    let entries = jpegs(200, 500, 4000, 3000);

    let from_entries = estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, None);

    assert_eq!(200 * 500 * 10, from_entries);
}

// C#: RamBudgetPolicyTests.EntriesEstimate_UnboundedBoxWithKnownDimensions_UsesTheMeasuredMeanWhenAvailable
#[test]
fn entries_estimate_unbounded_box_with_known_dimensions_uses_the_measured_mean_when_available() {
    let entries = jpegs(10, 500, 4000, 3000);

    assert_eq!(
        10 * 250_000,
        estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, Some(200_000.0))
    );
}

// C#: RamBudgetPolicyTests.EntriesEstimate_WithMeasuredMean_MatchesTheScalarCalibrationEvenWhenDimensionsAreKnown
#[test]
fn entries_estimate_with_measured_mean_matches_the_scalar_calibration_even_when_dimensions_are_known() {
    let rows: [(f64, i64); 3] = [
        (100_000.0, 200 * 125_000),      // measured + 25 % margin, under the box bound
        (3_600_000.0, 200 * 4_000_000),  // margin would pass the 4 MB bound: capped at it
        (8_000_000.0, 200 * 10_000_000), // 16-bit images measured above the bound: the measurement wins
    ];
    let box_ = DecodeBox::new(1000, 1000);
    let entries = jpegs(200, 1, 4000, 3000);

    for (measured, expected) in rows {
        assert_eq!(
            expected,
            estimate_folder_preview_bytes(&entries, box_, Some(measured)),
            "measured {measured}"
        );
    }
}

// ---- RamBudgetPolicyEstimateGapTests -------------------------------------------------------------------------------

// C#: RamBudgetPolicyEstimateGapTests.EstimateFolderPreviewBytes_UnboundedBoxWithPngEntries_UsesTheJpegFactorTenOfTheEntriesOverload
#[test]
fn estimate_folder_preview_bytes_unbounded_box_with_png_entries_uses_the_jpeg_factor_ten_of_the_entries_overload() {
    // Pinned on purpose: the entries overload does not special-case PNG (x10).
    let entries = [with_meta("a.png", 1_000, None, None)];

    assert_eq!(
        10_000,
        estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, None)
    );
}

// C#: RamBudgetPolicyEstimateGapTests.Estimates_ZeroEntries_AreZeroForEveryBox
#[test]
fn estimates_zero_entries_are_zero_for_every_box() {
    assert_eq!(0, estimate_folder_preview_bytes(&[], DecodeBox::UNBOUNDED, None));
    assert_eq!(0, estimate_folder_preview_bytes(&[], DecodeBox::new(1920, 1080), None));
}

// C#: RamBudgetPolicyEstimateGapTests.EstimateFolderPreviewBytes_RawWithUnknownLengthAfterOtherEntries_DisablesWholeFolderPreload
#[test]
fn estimate_folder_preview_bytes_raw_with_unknown_length_after_other_entries_disables_whole_folder_preload() {
    let entries = [
        with_meta("a.jpg", 1_000, None, None),
        entry("b.cr3", None, None, None), // length unknown
        with_meta("c.jpg", 1_000, None, None),
    ];

    assert_eq!(
        i64::MAX,
        estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, None)
    );
}

// ---- RamBudgetPolicyMutationTests ----------------------------------------------------------------------------------

// C#: RamBudgetPolicyMutationTests.ClampSourceBytes_UnknownRam_ReturnsRequest
#[test]
fn clamp_source_bytes_unknown_ram_returns_request() {
    for physical in [0_i64, -1] {
        assert_eq!(
            123_456_789,
            clamp_source_bytes_to_physical_memory(123_456_789, physical),
            "physical {physical}"
        );
    }
}

// C#: RamBudgetPolicyMutationTests.EstimateEntries_UnboundedBox_UsesCompressedSize
#[test]
fn estimate_entries_unbounded_box_uses_compressed_size() {
    let entries = [
        with_meta("a.jpg", 1000, None, None),
        with_meta("b.png", 500, None, None),
    ];

    for (width, height) in [(256, 0), (0, 256)] {
        assert_eq!(
            15_000,
            estimate_folder_preview_bytes(&entries, DecodeBox::new(width, height), None)
        );
    }
}

// C#: RamBudgetPolicyMutationTests.EstimateEntries_ZeroMeasured_IsIgnored
#[test]
fn estimate_entries_zero_measured_is_ignored() {
    let entries = [
        with_meta("a.jpg", 1000, None, None),
        with_meta("b.jpg", 1000, None, None),
    ];

    assert_eq!(
        2 * 40_000,
        estimate_folder_preview_bytes(&entries, DecodeBox::new(100, 100), Some(0.0))
    );
}

// C#: RamBudgetPolicyMutationTests.EstimateEntries_MeasuredAtBoxBound_IsCapped
#[test]
fn estimate_entries_measured_at_box_bound_is_capped() {
    let entries = [
        entry("a.jpg", None, None, None),
        entry("b.jpg", None, None, None),
        entry("c.jpg", None, None, None),
    ];
    let box_ = DecodeBox::new(100, 100);

    assert_eq!(
        3 * 40_000,
        estimate_folder_preview_bytes(&entries, box_, Some(40_000.0))
    );
    assert_eq!(
        3 * 25_000,
        estimate_folder_preview_bytes(&entries, box_, Some(20_000.0))
    );
    assert_eq!(
        3 * 60_000,
        estimate_folder_preview_bytes(&entries, box_, Some(48_000.0))
    );
}

// C#: RamBudgetPolicyMutationTests.EstimateEntries_PartlyKnownDimensions_UseTheBoxBound
#[test]
fn estimate_entries_partly_known_dimensions_use_the_box_bound() {
    let rows: [(Option<i32>, Option<i32>); 4] = [
        (Some(100), Some(0)),
        (Some(0), Some(100)),
        (Some(100), None),
        (None, Some(100)),
    ];

    for (width, height) in rows {
        let entries = [entry("a.jpg", Some(1000), width, height)];
        assert_eq!(
            40_000,
            estimate_folder_preview_bytes(&entries, DecodeBox::new(100, 100), None),
            "{width:?} x {height:?}"
        );
    }
}

// C#: RamBudgetPolicyMutationTests.EstimateEntries_RawInWidthOnlyBox_ScalesByWidth
#[test]
fn estimate_entries_raw_in_width_only_box_scales_by_width() {
    let entries = [with_meta("a.cr2", 5_000_000, Some(4000), Some(3000))];

    // preview assumed 8000 x 6000; the box width 2000 scales it by 0.25 -> 2000 x 1500 x 4 bytes
    assert_eq!(
        12_000_000,
        estimate_folder_preview_bytes(&entries, DecodeBox::new(2000, 0), None)
    );
    // a height-only box scales by height: 0.5 -> 4000 x 3000 x 4
    assert_eq!(
        48_000_000,
        estimate_folder_preview_bytes(&entries, DecodeBox::new(0, 3000), None)
    );
}

// C#: RamBudgetPolicyMutationTests.EstimateEntries_SumReachingLongRange_IsMaxValue
#[test]
fn estimate_entries_sum_reaching_long_range_is_max_value() {
    let box_ = DecodeBox::new(1 << 30, 1 << 30); // 2^60 pixels x 4 bytes = 2^62 per image
    let entries = [entry("a.jpg", None, None, None), entry("b.jpg", None, None, None)]; // 2^63 in total

    assert_eq!(i64::MAX, estimate_folder_preview_bytes(&entries, box_, None));
}

// ---- RamBudgetPolicyRawLengthTests (the FolderEstimateCache test is in src/ram_budget.rs) ---------------------------

// C#: RamBudgetPolicyRawLengthTests.Unbounded_RawWithoutDimensions_UsesTheCompressedLengthTimesTheDocumentedFactor
#[test]
fn unbounded_raw_without_dimensions_uses_the_compressed_length_times_the_documented_factor() {
    let entries = raw_folder(10, 25_000_000);

    let estimate = estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, None);

    // The literal product (10 files x 25 MB x the documented factor 6) on purpose: a change of the factor must fail this test.
    assert_eq!(1_500_000_000, estimate);
    assert_ne!(i64::MAX, estimate);
}

// C#: RamBudgetPolicyRawLengthTests.Unbounded_RawWithoutDimensionsAndWithoutLength_StaysConservative
#[test]
fn unbounded_raw_without_dimensions_and_without_length_stays_conservative() {
    let entries = [entry("folder/IMG_000001.CR2", None, None, None)];

    assert_eq!(
        i64::MAX,
        estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, None)
    );
}

// C#: RamBudgetPolicyRawLengthTests.Bounded_RawWithoutDimensions_StaysAtTheBoxBound
#[test]
fn bounded_raw_without_dimensions_stays_at_the_box_bound() {
    let entries = raw_folder(10, 25_000_000);

    let estimate = estimate_folder_preview_bytes(&entries, DecodeBox::new(1920, 1080), None);

    assert_eq!(10 * 1920 * 1080 * 4, estimate);
}

// C#: RamBudgetPolicyRawLengthTests.Measured_RawWithoutDimensions_UsesTheCalibratedMean
#[test]
fn measured_raw_without_dimensions_uses_the_calibrated_mean() {
    let entries = raw_folder(10, 25_000_000);

    assert_eq!(
        10 * 1_250_000,
        estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, Some(1_000_000.0))
    );
}

// C#: RamBudgetPolicyRawLengthTests.Estimate_HundredThousandMixedEntries_AllocatesNothingPerEntry
// Ported as a value check only: the C# allocation counter (`GC.GetAllocatedBytesForCurrentThread`) has no Rust twin.
// The structural rule (no per-entry allocation) is kept in the implementation: `is_raw_path` takes `&str`.
#[test]
fn estimate_hundred_thousand_mixed_entries_gives_positive_totals() {
    let entries: Vec<CatalogEntry> = (0..100_000_usize)
        .map(|index| {
            if index % 2 == 0 {
                raw(index, 25_000_000)
            } else {
                with_meta(&format!("folder/IMG_{index:06}.jpg"), 5_000_000, None, None)
            }
        })
        .collect();

    let unbounded = estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, None);
    let bounded = estimate_folder_preview_bytes(&entries, DecodeBox::new(1920, 1080), None);

    assert!(unbounded > 0 && bounded > 0, "unbounded {unbounded}, bounded {bounded}");
}

// C#: RamBudgetPolicyRawLengthTests.Estimate_WithMeasuredMean_DoesNotVisitEntries
// Ported as a value check only (the allocation half has no Rust twin, see the test above).
#[test]
fn estimate_with_measured_mean_does_not_visit_entries() {
    let entries = raw_folder(100_000, 25_000_000);

    let estimate = estimate_folder_preview_bytes(&entries, DecodeBox::UNBOUNDED, Some(1_000_000.0));

    assert_eq!(100_000 * 1_250_000, estimate);
}

// ---- RamBudgetPolicyRawPreviewTests --------------------------------------------------------------------------------

// C#: RamBudgetPolicyRawPreviewTests.OriginalMode_SRawWithPreviewLargerThanSensor_IsNotUnderEstimated
#[test]
fn original_mode_sraw_with_preview_larger_than_sensor_is_not_under_estimated() {
    let estimate = estimate_folder_preview_bytes(&sraw_entries(10), DecodeBox::UNBOUNDED, None);

    assert!(
        estimate >= 10 * SRAW_PREVIEW_BYTES,
        "Estimated {estimate} for a folder that decodes to {}.",
        10 * SRAW_PREVIEW_BYTES
    );
}

// C#: RamBudgetPolicyRawPreviewTests.BoundedBoxLargerThanTheSensor_SRawIsEstimatedAtTheBoxNotTheSensor
#[test]
fn bounded_box_larger_than_the_sensor_sraw_is_estimated_at_the_box_not_the_sensor() {
    let estimate = estimate_folder_preview_bytes(&sraw_entries(10), DecodeBox::new(5184, 3456), None);

    assert!(estimate >= 10 * SRAW_PREVIEW_BYTES, "Estimated {estimate}.");
}

// C#: RamBudgetPolicyRawPreviewTests.BoundedBoxSmallerThanTheSensor_IsStillCappedAtTheBox
#[test]
fn bounded_box_smaller_than_the_sensor_is_still_capped_at_the_box() {
    let entries: Vec<CatalogEntry> = (0..50)
        .map(|index| {
            with_meta(
                &format!("raw-{index:03}.arw"),
                25_000_000 + index,
                Some(6000),
                Some(4000),
            )
        })
        .collect();

    let estimate = estimate_folder_preview_bytes(&entries, DecodeBox::new(1920, 1080), None);

    assert_eq!(50 * 1620 * 1080 * 4, estimate);
}

// C#: RamBudgetPolicyRawPreviewTests.MeasuredMean_StillOverridesTheGeometryEstimateForRaw
#[test]
fn measured_mean_still_overrides_the_geometry_estimate_for_raw() {
    let estimate = estimate_folder_preview_bytes(&sraw_entries(10), DecodeBox::UNBOUNDED, Some(1_000_000.0));

    assert_eq!(10 * 1_250_000, estimate);
}

// ---- RamCachePercentTests (policy-only tests) ----------------------------------------------------------------------

// C#: RamCachePercentTests.MinimumPreviewWindowBytes_Is41UhdPreviewsAt4BytesPerPixel
#[test]
fn minimum_preview_window_bytes_is_41_uhd_previews_at_4_bytes_per_pixel() {
    assert_eq!(41, PreloadWindow::DEFAULT.image_count());
    assert_eq!(
        41 * UHD_PREVIEW_BYTES,
        minimum_preview_window_bytes(PreloadWindow::DEFAULT)
    );
}

// C#: RamCachePercentTests.MinimumPreviewWindowBytes_ForWindow_ScalesWithImageCount
#[test]
fn minimum_preview_window_bytes_for_window_scales_with_image_count() {
    // feat/preload-window-setting: default (32/8 -> 41 previews) is unchanged; a (1,0) window needs only 2.
    assert_eq!(
        2 * UHD_PREVIEW_BYTES,
        minimum_preview_window_bytes(PreloadWindow::new(1, 0))
    );
}

// C#: RamCachePercentTests.MinimumCachePercent_RoundsPreloadWindowUpWithinOneToNinety (theory, 10 rows)
#[test]
fn minimum_cache_percent_rounds_preload_window_up_within_one_to_ninety() {
    let rows: [(i64, i32); 10] = [
        (16 * GIB, 8),
        (32 * GIB, 4),
        (64 * GIB, 2),
        (128 * GIB, 1),      // 0.99 % rounds up to 1
        (1024 * GIB, 1),     // never below 1
        (GIB, 90),           // preload window does not fit: capped at the maximum
        (17_003_520_000, 8), // exactly 8 %: rounding up must not overshoot
        (17_003_519_999, 9), // one byte less: 8.0000001 % rounds up
        (0, 1),              // unknown RAM
        (-5, 1),
    ];

    for (physical, expected) in rows {
        assert_eq!(
            expected,
            minimum_cache_percent(physical, PreloadWindow::DEFAULT),
            "physical {physical}"
        );
    }
}

// C#: RamCachePercentTests.ClampCachePercent_ClampsToDeviceMinimumAndNinety (theory, 5 rows)
#[test]
fn clamp_cache_percent_clamps_to_device_minimum_and_ninety() {
    let rows: [(i32, i64, i32); 5] = [
        (50, 32 * GIB, 50),
        (2, 16 * GIB, 8),   // below the device minimum
        (95, 32 * GIB, 90), // above the maximum
        (90, 32 * GIB, 90),
        (0, 0, 1), // unknown RAM: absolute floor
    ];

    for (requested, physical, expected) in rows {
        assert_eq!(
            expected,
            clamp_cache_percent(requested, physical, PreloadWindow::DEFAULT),
            "requested {requested}"
        );
    }
}

// C#: RamCachePercentTests.BytesForPercent_IsExactFloor (theory, 5 rows)
#[test]
fn bytes_for_percent_is_exact_floor() {
    let rows: [(i32, i64, i64); 5] = [
        (50, 32 * GIB, 16 * GIB),
        (90, 101, 90), // floor of 90.9
        (25, 400, 100),
        (33, 0, 0),
        (0, 32 * GIB, 0),
    ];

    for (percent, physical, expected) in rows {
        assert_eq!(
            expected,
            bytes_for_percent(percent, physical),
            "percent {percent} physical {physical}"
        );
    }
}

// C#: RamCachePercentTests.BytesForPercent_HugePhysical_DoesNotOverflow
#[test]
fn bytes_for_percent_huge_physical_does_not_overflow() {
    assert_eq!(
        i64::MAX / 100 * 90 + i64::MAX % 100 * 90 / 100,
        bytes_for_percent(90, i64::MAX)
    );
}

// C#: RamCachePercentTests.PreviewBytesForPercent_PercentMinusSourceBytes (theory, 5 rows)
#[test]
fn preview_bytes_for_percent_percent_minus_source_bytes() {
    let rows: [(i32, i64, i64, i64); 5] = [
        (50, 32 * GIB, 0, 16 * GIB),
        (50, 32 * GIB, 3 * GIB, 13 * GIB), // source-bytes cache takes its share out of the same budget
        (75, 32 * GIB, 0, 24 * GIB),
        (200, 100 * GIB, 0, 90 * GIB), // clamped to 90 %
        (50, 32 * GIB, 40 * GIB, 1),   // nothing left: never zero (LRU rejects 0)
    ];

    for (percent, physical, source, expected) in rows {
        let actual =
            preview_bytes_for_percent(percent, physical, source, PreloadWindow::DEFAULT).expect("physical > 0");
        assert_eq!(
            expected, actual,
            "percent {percent} physical {physical} source {source}"
        );
    }
}

// C#: RamCachePercentTests.PreviewBytesForPercent_BelowMinimum_StillHoldsPreloadWindow
#[test]
fn preview_bytes_for_percent_below_minimum_still_holds_preload_window() {
    let bytes = preview_bytes_for_percent(1, 16 * GIB, 0, PreloadWindow::DEFAULT).expect("physical > 0");

    assert_eq!(bytes_for_percent(8, 16 * GIB), bytes);
    assert!(bytes >= minimum_preview_window_bytes(PreloadWindow::DEFAULT));
}

// C#: RamCachePercentTests.PreviewBytesForPercent_UnknownPhysical_Throws
#[test]
fn preview_bytes_for_percent_unknown_physical_throws() {
    assert_eq!(
        Err(ArgumentOutOfRange {
            param_name: "physicalBytes"
        }),
        preview_bytes_for_percent(50, 0, 0, PreloadWindow::DEFAULT)
    );
}

// C#: RamCachePercentTests.SourceBytesForPercent_LeavesPreloadWindowToPreviews (theory, 4 rows)
#[test]
fn source_bytes_for_percent_leaves_preload_window_to_previews() {
    let rows: [(i64, i32, i64, i64); 4] = [
        (16 * GIB, 50, 32 * GIB, 6_871_947_673), // the usual 20 % source-bytes cap wins
        (GIB, 50, 32 * GIB, GIB),                // small request unchanged
        (16 * GIB, 4, 32 * GIB, 14_107_934),     // 4 % leaves only (1.28 GiB - preload window) for source bytes
        (16 * GIB, 50, 0, 16 * GIB),             // unknown RAM: unchanged
    ];

    for (requested, percent, physical, expected) in rows {
        let actual = source_bytes_for_percent(requested, percent, physical, PreloadWindow::DEFAULT);
        assert_eq!(
            expected, actual,
            "requested {requested} percent {percent} physical {physical}"
        );
    }
}

// C#: RamCachePercentTests.PreviewPlusSource_NeverExceedsChosenPercent_AndPreviewKeepsPreloadWindow (theory, 3 rows)
#[test]
fn preview_plus_source_never_exceeds_chosen_percent_and_preview_keeps_preload_window() {
    for physical in [16 * GIB, 32 * GIB, 64 * GIB] {
        for percent in 1..=100 {
            let source = source_bytes_for_percent(i64::MAX / 4, percent, physical, PreloadWindow::DEFAULT);
            let preview =
                preview_bytes_for_percent(percent, physical, source, PreloadWindow::DEFAULT).expect("physical > 0");
            let budget = bytes_for_percent(clamp_cache_percent(percent, physical, PreloadWindow::DEFAULT), physical);

            assert!(
                source + preview <= budget,
                "{percent}%: {source} + {preview} > {budget}"
            );
            assert!(
                preview >= minimum_preview_window_bytes(PreloadWindow::DEFAULT),
                "{percent}%: preview {preview}"
            );
        }
    }
}

// C#: RamCachePercentTests.SourceBytesForPercent_LargeWindow_LeavesThatWindowRoom (Q-R31)
#[test]
fn source_bytes_for_percent_large_window_leaves_that_window_room() {
    let window = PreloadWindow::new(200, 50);
    let physical = 32 * GIB;
    let percent = minimum_cache_percent(physical, window); // budget just covers the window
    let budget = bytes_for_percent(percent, physical);
    let expected_room = budget - minimum_preview_window_bytes(window);

    assert_eq!(
        expected_room,
        source_bytes_for_percent(16 * GIB, percent, physical, window)
    );
    assert!(
        source_bytes_for_percent(16 * GIB, percent, physical, PreloadWindow::DEFAULT) > expected_room,
        "with the default window the same percent would have left the source-bytes cache far more room"
    );
}

// ---- MemoryBudgetClampTests (policy-only halves) -------------------------------------------------------------------

// C#: MemoryBudgetClampTests.ClampToPhysicalMemory_LimitsToHalfOfPhysical (theory, 4 rows)
#[test]
fn clamp_to_physical_memory_limits_to_half_of_physical() {
    let rows: [(i64, i64, i64); 4] = [
        (16 * GIB, 32 * GIB, 16 * GIB), // dev box: unchanged
        (16 * GIB, 16 * GIB, 8 * GIB),  // 16 GB machine: halved
        (4 * GIB, 16 * GIB, 4 * GIB),   // already smaller: unchanged
        (16 * GIB, 0, 16 * GIB),        // unknown physical size: unchanged
    ];

    for (requested, physical, expected) in rows {
        assert_eq!(
            expected,
            clamp_to_physical_memory(requested, physical),
            "requested {requested} physical {physical}"
        );
    }
}

// C#: MemoryBudgetClampTests.ClampPreviewToPhysicalMemory_LeavesRoomForSourceBytes (theory, 5 rows)
#[test]
fn clamp_preview_to_physical_memory_leaves_room_for_source_bytes() {
    let rows: [(i64, i64, i64, i64); 5] = [
        (16 * GIB, 16 * GIB, 0, 8 * GIB),       // no source cache: preview keeps the 50% share
        (16 * GIB, 16 * GIB, 3 * GIB, 5 * GIB), // source cache present: the two together stay within 50%
        (2 * GIB, 16 * GIB, 3 * GIB, 2 * GIB),  // already fits
        (16 * GIB, 32 * GIB, 6 * GIB, 10 * GIB),
        (16 * GIB, 0, 6 * GIB, 16 * GIB), // unknown physical size: unchanged
    ];

    for (requested, physical, source, expected) in rows {
        assert_eq!(
            expected,
            clamp_preview_to_physical_memory(requested, physical, source),
            "physical {physical} source {source}"
        );
    }
}

// C#: MemoryBudgetClampTests.ClampSourceBytesToPhysicalMemory_LimitsToTwentyPercent
#[test]
fn clamp_source_bytes_to_physical_memory_limits_to_twenty_percent() {
    assert_eq!(
        (16.0 * GIB as f64 * 0.2) as i64,
        clamp_source_bytes_to_physical_memory(16 * GIB, 16 * GIB)
    );
    assert_eq!(GIB, clamp_source_bytes_to_physical_memory(GIB, 16 * GIB));
}

// C#: MemoryBudgetClampTests.SourceBytesCache_HugeRequest_IsClampedToHalfOfPhysical (policy half only: the
// SourceBytesCache construction half is W4-10a)
#[test]
fn source_bytes_huge_request_is_clamped_to_a_fifth_of_physical() {
    let physical = 16 * GIB;

    let clamped = clamp_source_bytes_to_physical_memory(i64::MAX / 2, physical);

    assert_eq!((physical as f64 * 0.2) as i64, clamped);
    assert!(clamped <= physical / 2);
}

// ---- RamBudgetPropertyTests ----------------------------------------------------------------------------------------

// C#: RamBudgetPropertyTests.BytesForPercent_IsExact
#[test]
fn bytes_for_percent_is_exact() {
    let mut rng = XorShift::new(303);
    for _ in 0..100_000 {
        let physical = random_physical(&mut rng);
        let percent = rng.range_i32(-3, 101); // the documented domain; callers clamp to <= 90 first

        let actual = bytes_for_percent(percent, physical);

        let expected = if percent <= 0 {
            0
        } else {
            // Exact reference: 128-bit arithmetic cannot overflow for percent <= 100.
            (i128::from(physical) * i128::from(percent) / 100) as i64
        };
        assert_eq!(expected, actual, "physical={physical} percent={percent}");
    }
}

// C#: RamBudgetPropertyTests.Clamps_HoldForAnyRam
#[test]
fn clamps_hold_for_any_ram() {
    let mut rng = XorShift::new(304);
    for _ in 0..50_000 {
        let physical = random_physical(&mut rng);
        let requested = rng.range_i32(-50, 250);
        let source = if rng.range_i64(0, 3) == 0 {
            0
        } else {
            rng.range_i64(0, physical)
        };

        let minimum = minimum_cache_percent(physical, PreloadWindow::DEFAULT);
        let clamped = clamp_cache_percent(requested, physical, PreloadWindow::DEFAULT);
        assert!(
            (minimum..=MAX_IMAGE_CACHE_RAM_PERCENT).contains(&clamped),
            "clamped {clamped} outside [{minimum}, {MAX_IMAGE_CACHE_RAM_PERCENT}] for physical={physical} requested={requested}"
        );
        assert!(
            clamp_cache_percent(requested + 1, physical, PreloadWindow::DEFAULT) >= clamped,
            "not monotonic at {requested} on {physical}"
        );

        let preview =
            preview_bytes_for_percent(requested, physical, source, PreloadWindow::DEFAULT).expect("physical > 0");
        assert!(
            preview >= 1,
            "preview budget {preview} for physical={physical} requested={requested} source={source}"
        );
        assert!(
            preview <= bytes_for_percent(clamped, physical) || preview == 1,
            "preview {preview} above share for physical={physical} requested={requested}"
        );

        let source_capacity =
            source_bytes_for_percent(rng.range_i64(0, i64::MAX), requested, physical, PreloadWindow::DEFAULT);
        let source_limit = (physical as f64 * MAX_SOURCE_SHARE) as i64 + 1;
        assert!(
            (0..=source_limit).contains(&source_capacity),
            "source {source_capacity} above {source_limit}"
        );
    }
}

/// `MaxSourceBytesShare` (0.2) as used by the C# property test.
const MAX_SOURCE_SHARE: f64 = 0.2;

// ---- Properties/RamBudgetShareAndWindowPropertyTests ---------------------------------------------------------------

// C#: RamBudgetShareAndWindowPropertyTests.Clamps_NeverExceedTheirShare
#[test]
fn clamps_never_exceed_their_share() {
    let mut rng = XorShift::new(401);
    for _ in 0..20_000 {
        let physical = random_physical(&mut rng);
        let requested = rng.edgy_long();
        let source = rng.edgy_long();
        let half = (physical as f64 * 0.5) as i64;
        let fifth = (physical as f64 * MAX_SOURCE_SHARE) as i64;

        let overall = clamp_to_physical_memory(requested, physical);
        assert!(
            overall <= half && overall <= requested,
            "overall {overall} (phys {physical}, req {requested})"
        );

        let src = clamp_source_bytes_to_physical_memory(requested, physical);
        assert!(
            src <= fifth && src <= overall,
            "source {src} (phys {physical}, req {requested})"
        );

        let preview = clamp_preview_to_physical_memory(requested, physical, source);
        assert!(preview <= overall, "preview above overall clamp");
        assert!(
            preview <= (half - source.max(0)).max(0),
            "preview {preview} + source {source} over half of {physical}"
        );
    }
}

// C#: RamBudgetShareAndWindowPropertyTests.PercentBudgets_FitTheirShare
#[test]
fn percent_budgets_fit_their_share() {
    let mut rng = XorShift::new(402);
    for _ in 0..20_000 {
        let physical = random_physical(&mut rng);
        let window = random_window(&mut rng);
        let percent = rng.range_i32(-20, 200);
        let requested_source = rng.edgy_long();

        let cache_percent = clamp_cache_percent(percent, physical, window);
        let share = bytes_for_percent(cache_percent, physical);

        let source = source_bytes_for_percent(requested_source, percent, physical, window);
        if source > 0 {
            assert!(
                source + minimum_preview_window_bytes(window) <= share,
                "source {source} leaves no room for the window in {share}"
            );
            assert!(source <= (physical as f64 * MAX_SOURCE_SHARE) as i64);
        }

        let preview = preview_bytes_for_percent(percent, physical, source.max(0), window).expect("physical > 0");
        assert!(preview >= 1);
        assert!(preview <= share.max(1), "preview {preview} above share {share}");
        if share > source.max(0) {
            assert!(
                preview + source.max(0) <= share,
                "preview + source exceed the percent share"
            );
        }
    }
}

// C#: RamBudgetShareAndWindowPropertyTests.MinimumCachePercent_IsMonotonic
#[test]
fn minimum_cache_percent_is_monotonic() {
    let mut rng = XorShift::new(403);
    for _ in 0..20_000 {
        let physical = random_physical(&mut rng);
        let window = random_window(&mut rng);
        let bigger = PreloadWindow::new(
            window.forward + rng.range_i32(0, 50),
            window.backward + rng.range_i32(0, 50),
        );

        let at = minimum_cache_percent(physical, window);
        assert!((MIN_IMAGE_CACHE_RAM_PERCENT..=MAX_IMAGE_CACHE_RAM_PERCENT).contains(&at));
        if at < MAX_IMAGE_CACHE_RAM_PERCENT {
            assert!(
                bytes_for_percent(at, physical) >= minimum_preview_window_bytes(window),
                "the floor percent cannot hold the window"
            );
        }
        assert!(
            minimum_cache_percent(physical, bigger) >= at,
            "bigger window lowered the floor"
        );
        assert!(
            minimum_cache_percent(physical + rng.range_i64(0, physical), window) <= at,
            "more RAM raised the floor"
        );
    }
}

// ---- Golden tests (tests/golden/ram-budget-*.json, inputs in tests/golden-inputs/) ---------------------------------

/// Reads the oracle input cases (same order and names as the golden cases).
fn input_cases(name: &str) -> Vec<Value> {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    doc["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing 'cases'", path.display()))
        .clone()
}

fn i32_field(case: &Value, key: &str) -> i32 {
    i32::try_from(golden::i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit i32 in {case}"))
}

fn optional_i64(case: &Value, key: &str) -> Option<i64> {
    case.get(key).and_then(Value::as_i64)
}

fn optional_i32(case: &Value, key: &str) -> Option<i32> {
    optional_i64(case, key).map(|v| i32::try_from(v).unwrap_or_else(|_| panic!("'{key}' does not fit i32 in {case}")))
}

/// `window` object, or `PreloadWindow::DEFAULT` when the case omits it.
fn window_of(case: &Value) -> PreloadWindow {
    match case.get("window") {
        Some(window) if !window.is_null() => {
            PreloadWindow::new(i32_field(window, "forward"), i32_field(window, "backward"))
        }
        _ => PreloadWindow::DEFAULT,
    }
}

/// Runs one scalar case and returns `{name, result}` or `{name, error, param}`, the golden shape.
fn run_scalar(case: &Value) -> Value {
    let name = golden::str_of(case, "name");
    let op = golden::str_of(case, "op");
    let window = window_of(case);
    let ok = |value: Value| json!({ "name": name, "result": value });
    let err = |param: &str| json!({ "name": name, "error": "ArgumentOutOfRangeException", "param": param });
    let physical = || golden::i64_of(case, "physical");
    let requested = || golden::i64_of(case, "requested");
    let percent = || i32_field(case, "percent");

    match op {
        "minimum-window-bytes" => ok(json!(minimum_preview_window_bytes(window))),
        "clamp-to-physical" => ok(json!(clamp_to_physical_memory(requested(), physical()))),
        "clamp-source-bytes" => ok(json!(clamp_source_bytes_to_physical_memory(requested(), physical()))),
        "clamp-preview" => ok(json!(clamp_preview_to_physical_memory(
            requested(),
            physical(),
            golden::i64_of(case, "source")
        ))),
        "minimum-cache-percent" => ok(json!(minimum_cache_percent(physical(), window))),
        "clamp-cache-percent" => ok(json!(clamp_cache_percent(percent(), physical(), window))),
        "bytes-for-percent" => ok(json!(bytes_for_percent(percent(), physical()))),
        "preview-bytes-for-percent" => {
            match preview_bytes_for_percent(percent(), physical(), golden::i64_of(case, "source"), window) {
                Ok(value) => ok(json!(value)),
                Err(error) => err(error.param_name),
            }
        }
        "source-bytes-for-percent" => ok(json!(source_bytes_for_percent(
            requested(),
            percent(),
            physical(),
            window
        ))),
        "should-preload" => {
            let probe = FixedProbe(golden::bool_of(case, "headroom"));
            match should_preload_whole_folder_estimate(
                golden::i64_of(case, "estimated"),
                golden::i64_of(case, "capacity"),
                &probe,
                MEMORY_RESERVE_BYTES,
                PRELOAD_MEMORY_LOAD_LIMIT,
            ) {
                Ok(value) => ok(json!(value)),
                Err(error) => err(error.param_name),
            }
        }
        other => panic!("unknown scalar op '{other}' in case {name}"),
    }
}

/// `entries` of an estimate case as catalog entries (`path`, `length`, `width`, `height`; each may be null).
fn entries_of(case: &Value) -> Vec<CatalogEntry> {
    case["entries"]
        .as_array()
        .expect("estimate case has an entries array")
        .iter()
        .map(|item| {
            entry(
                golden::str_of(item, "path"),
                optional_i64(item, "length"),
                optional_i32(item, "width"),
                optional_i32(item, "height"),
            )
        })
        .collect()
}

/// `measured`: absent or null = None; a number; or the strings "NaN" / "Infinity".
fn measured_of(case: &Value) -> Option<f64> {
    match case.get("measured") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => match text.as_str() {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            other => panic!("unsupported measured value '{other}'"),
        },
        Some(number) => Some(number.as_f64().expect("measured is a number")),
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "non-string panic".to_owned()
    }
}

/// Runs one estimate case; a C# `OverflowException` is the implementation's documented overflow panic.
fn run_estimate(case: &Value) -> Value {
    let name = golden::str_of(case, "name");
    let entries = entries_of(case);
    let target = DecodeBox::new(i32_field(&case["box"], "width"), i32_field(&case["box"], "height"));
    let measured = measured_of(case);

    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        estimate_folder_preview_bytes(&entries, target, measured)
    }));
    match outcome {
        Ok(value) => json!({ "name": name, "result": value }),
        Err(payload) => {
            let text = panic_text(payload.as_ref());
            if text.contains("overflow") {
                json!({ "name": name, "error": "OverflowException" })
            } else {
                // Any other panic (for example `todo!()`) is a failure, never an expected overflow.
                json!({ "name": name, "panic": text })
            }
        }
    }
}

/// Compares every golden case with the output of `run`, and reports all differences at once.
fn assert_golden_matches(golden_name: &str, expected_count: usize, run: fn(&Value) -> Value) {
    let golden_file = golden::load(golden_name);
    let inputs = input_cases(golden_name);
    assert_eq!(
        expected_count,
        golden_file.cases.len(),
        "{golden_name}: golden case count"
    );
    assert_eq!(expected_count, inputs.len(), "{golden_name}: input case count");

    let mut mismatches = Vec::new();
    for expected in &golden_file.cases {
        let name = golden::str_of(expected, "name");
        let input = inputs
            .iter()
            .find(|case| case["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("{golden_name}: no input for case {name}"));
        let actual = run(input);
        if actual != *expected {
            mismatches.push(format!("  {name}\n    expected: {expected}\n    actual:   {actual}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{golden_name}: {} of {} cases differ from the C# oracle:\n{}",
        mismatches.len(),
        golden_file.cases.len(),
        mismatches.join("\n")
    );
}

#[test]
fn golden_ram_budget_scalar_matches_the_csharp_oracle() {
    assert_golden_matches("ram-budget-scalar", 92, run_scalar);
}

#[test]
fn golden_ram_budget_estimate_matches_the_csharp_oracle() {
    assert_golden_matches("ram-budget-estimate", 53, run_estimate);
}
