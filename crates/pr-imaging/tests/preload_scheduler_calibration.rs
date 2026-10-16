//! Port of `tests/PhotoReview.Imaging.Tests/PreloadEstimateCalibrationTests.cs` (C# baseline d92d6aae, 156 lines), Q-R17:
//! whole-folder estimate from the decode box, then from measured preview sizes.
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//! `Sampler_NeedsMinimumSamples_AndRestartsOnBoxChange` is W4-06b (`PreviewSizeSampler`), not here.

mod common;

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use common::preload_fakes::*;
use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload::policy::WHOLE_FOLDER_CACHE_FILL_LIMIT;
use pr_imaging::preload::scheduler::PreloadScheduler;
use pr_imaging::preload::target::{KeyError, PreloadTarget};
use pr_imaging::preload_order::{FORWARD_LOOKAHEAD, PreloadWindow};
use pr_test_support::FakeFileSystem;

const IMAGE_COUNT: usize = 200;
const BUDGET: i64 = 100 * 1024 * 1024;
const MIB: i64 = 1024 * 1024;

fn box_1000() -> DecodeBox {
    DecodeBox::new(1000, 1000) // bound: 4 MB per image, 800 MB for the folder
}

/// C# `InMemoryTarget`: no files; keys come from the catalog metadata, sizes are fixed.
struct InMemoryTarget {
    decode_box: DecodeBox,
    preview_bytes: Option<i64>,
    cache_bytes_per_image: i64,
    entries: Arc<[CatalogEntry]>,
    cached: Mutex<HashSet<String>>,
    preloaded: AtomicUsize,
}

impl InMemoryTarget {
    fn new(decode_box: DecodeBox, preview_bytes: Option<i64>, cache_bytes_per_image: i64) -> Arc<Self> {
        let written = DateTimeUtc::from_ticks(638_000_000_000_000_000).expect("a valid date");
        let entries: Arc<[CatalogEntry]> = (0..IMAGE_COUNT)
            .map(|i| {
                CatalogEntry::new(format!("C:\\qr17-fake\\img-{i:03}.jpg"))
                    .expect("valid path")
                    .with_metadata(1000 + i as i64, written, None, None)
            })
            .collect::<Vec<_>>()
            .into();
        Arc::new(Self {
            decode_box,
            preview_bytes,
            cache_bytes_per_image,
            entries,
            cached: Mutex::new(HashSet::new()),
            preloaded: AtomicUsize::new(0),
        })
    }

    fn preloaded_count(&self) -> usize {
        self.preloaded.load(Ordering::SeqCst)
    }

    fn key_of(&self, entry: &CatalogEntry) -> ImageCacheKey {
        ImageCacheKey::create_for_entry(
            &FakeFileSystem::new(),
            entry,
            false,
            self.decode_box,
            KeyOptions::default(),
        )
        .expect("metadata key")
    }
}

impl PreloadTarget for InMemoryTarget {
    fn try_get_cached_preview_for_path(&self, path: &str) -> bool {
        lock_set(&self.cached).contains(&path.to_ascii_uppercase())
    }

    fn try_get_cached_preview(&self, key: &ImageCacheKey) -> bool {
        lock_set(&self.cached).contains(key.path())
    }

    fn preload(&self, path: &str, _cancel: &CancelToken) -> Result<(), DecodeError> {
        self.preloaded.fetch_add(1, Ordering::SeqCst);
        let entry = self
            .entries
            .iter()
            .find(|e| e.path().eq_ignore_ascii_case(path))
            .expect("fixture path");
        let key = self.key_of(entry);
        lock_set(&self.cached).insert(key.path().to_owned());
        Ok(())
    }

    fn get_current_cache_key_for_path(&self, path: &str) -> Result<ImageCacheKey, KeyError> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.path().eq_ignore_ascii_case(path))
            .expect("fixture path");
        Ok(self.key_of(entry))
    }

    fn get_current_cache_key(&self, entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        Ok(self.key_of(entry))
    }

    fn cache_count(&self) -> i32 {
        i32::try_from(lock_set(&self.cached).len()).unwrap_or(i32::MAX)
    }

    fn cache_bytes(&self) -> i64 {
        lock_set(&self.cached).len() as i64 * self.cache_bytes_per_image
    }

    fn cached_preview_bytes(&self, key: &ImageCacheKey) -> Option<i64> {
        if lock_set(&self.cached).contains(key.path()) {
            self.preview_bytes
        } else {
            None
        }
    }
}

fn lock_set(set: &Mutex<HashSet<String>>) -> MutexGuard<'_, HashSet<String>> {
    lock(set)
}

/// C# `LoadProbe(currentLoad)`: headroom is `load < maximumLoad`; no snapshot.
struct ThresholdProbe(f64);

impl MemoryProbe for ThresholdProbe {
    fn has_headroom(&self, maximum_load: f64, _reserve_bytes: i64) -> bool {
        self.0 < maximum_load
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        None
    }
}

fn calibration_scheduler(target: &Arc<InMemoryTarget>, setup: Setup) -> PreloadScheduler {
    create(target, &target.entries, setup)
}

#[test]
fn small_measured_previews_unlock_whole_folder() {
    // C#: PreloadEstimateCalibrationTests.SmallMeasuredPreviews_UnlockWholeFolder
    // Box bound over budget keeps the window; small measured previews then unlock the whole folder (200 x 100 KB x 1.25 = 25 MB).
    let target = InMemoryTarget::new(box_1000(), Some(100_000), 0);
    let scheduler = calibration_scheduler(
        &target,
        Setup::new(4, PreloadWindow::DEFAULT).full_folder_threshold(BUDGET),
    );
    assert!(finished(&scheduler.preload_around(0)));
    assert_eq!(IMAGE_COUNT - 1, target.preloaded_count());
}

#[test]
fn large_measured_previews_keep_window() {
    // C#: PreloadEstimateCalibrationTests.LargeMeasuredPreviews_KeepWindow
    // Measured previews too large for the budget keep the 32-ahead window (200 x 3 MB x 1.25 = 750 MB).
    let target = InMemoryTarget::new(box_1000(), Some(3_000_000), 0);
    let scheduler = calibration_scheduler(
        &target,
        Setup::new(4, PreloadWindow::DEFAULT).full_folder_threshold(BUDGET),
    );
    assert!(finished(&scheduler.preload_around(0)));
    assert_eq!(FORWARD_LOOKAHEAD as usize, target.preloaded_count());
}

#[test]
fn no_measurement_uses_box_bound() {
    // C#: PreloadEstimateCalibrationTests.NoMeasurement_UsesBoxBound
    // No measured sizes: the box bound alone decides (window here).
    let target = InMemoryTarget::new(box_1000(), None, 0);
    let scheduler = calibration_scheduler(
        &target,
        Setup::new(4, PreloadWindow::DEFAULT).full_folder_threshold(BUDGET),
    );
    assert!(finished(&scheduler.preload_around(0)));
    assert_eq!(FORWARD_LOOKAHEAD as usize, target.preloaded_count());
}

#[test]
fn box_bound_within_budget_whole_folder_from_start() {
    // C#: PreloadEstimateCalibrationTests.BoxBoundWithinBudget_WholeFolderFromStart
    // Box bound within budget preloads the whole folder before any measurement (200 x 40 KB = 8 MB).
    let target = InMemoryTarget::new(DecodeBox::new(100, 100), None, 0);
    let scheduler = calibration_scheduler(
        &target,
        Setup::new(4, PreloadWindow::DEFAULT).full_folder_threshold(BUDGET),
    );
    assert!(finished(&scheduler.preload_around(0)));
    assert_eq!(IMAGE_COUNT - 1, target.preloaded_count());
}

#[test]
fn whole_folder_decision_uses_configured_memory_load_limit() {
    // C#: PreloadEstimateCalibrationTests.WholeFolderDecision_UsesConfiguredMemoryLoadLimit
    // The whole-folder decision honours the configured memory-load limit, not a built-in default (0.85 < 0.95 < 0.80 fails).
    let target = InMemoryTarget::new(DecodeBox::new(100, 100), None, 0);
    let scheduler = calibration_scheduler(
        &target,
        Setup::new(4, PreloadWindow::DEFAULT)
            .full_folder_threshold(BUDGET)
            .memory_load_limit(0.95)
            .probe(Arc::new(ThresholdProbe(0.85))),
    );
    assert!(finished(&scheduler.preload_around(0)));
    assert_eq!(IMAGE_COUNT - 1, target.preloaded_count());
}

#[test]
fn whole_folder_stops_at_cache_fill_limit() {
    // C#: PreloadEstimateCalibrationTests.WholeFolder_StopsAtCacheFillLimit
    // The estimate fits (200 x 40 KB box bound = 8 MB), but the cache really holds 1 MB per image: past ~90 images each
    // far decode would only evict a near one. The window is still preloaded.
    let target = InMemoryTarget::new(DecodeBox::new(100, 100), None, MIB);
    let scheduler = calibration_scheduler(
        &target,
        Setup::new(4, PreloadWindow::DEFAULT).full_folder_threshold(BUDGET),
    );
    assert!(finished(&scheduler.preload_around(0)));
    let limit = (BUDGET as f64 * WHOLE_FOLDER_CACHE_FILL_LIMIT / MIB as f64).ceil() as usize;
    let count = target.preloaded_count();
    assert!(
        (FORWARD_LOOKAHEAD as usize..=limit + 4).contains(&count),
        "preloaded {count}, limit {limit} (+ in-flight workers)"
    );
}
