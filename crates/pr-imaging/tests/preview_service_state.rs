//! Port of the state, policy and key tests of `PhotoReview.Imaging.Tests` for `PreviewImageService` (C# baseline
//! d92d6aae; PARITY I-04, I-06, I-07, S-15, INV-2), plus the golden checks against `tests/golden/preview-budget-line.json`
//! (49 cases, `resolve_capacity`) and `tests/golden/preview-current-key.json` (32 cases, the entry key).
//!
//! Port of `PhotoReview.Imaging/Caching/PreviewImageService.cs` (lines 1-63, 85-225, 370-399, 421-431, 664-723, 747,
//! 789-798, 916-923): `contracts/W4-14a.md`.
//!
//! Red tests (W4-14a): they compile against the stub and fail through `todo!()` until the bodies are implemented.
//! Tests that need the decode path (`get_preview_path`, `get_preview`, `get_original_dimensions`) stay red until W4-14b
//! merges. The disk-cache tests of the C# class are W4-14c's; the `PreloadScheduler` tests are the W4-14d step.

mod support {
    pub mod preview_service;
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pr_core::catalog::types::{CatalogEntry, RAW_EXTENSIONS};
use pr_core::diagnostics::metrics::ReviewMetrics;
use pr_core::model::enums::DecoderBackend;
use pr_core::time::DateTimeUtc;
use std::error::Error as _;

use pr_core::text::dotnet_path::PathError;
use pr_imaging::cache::preview_file::PreviewCacheError;
use pr_imaging::cache::source_bytes::{SourceBytesCache, SourceBytesError};
use pr_imaging::cache_key::CacheKeyError;
use pr_imaging::cache_key::{ImageSourceKind, normalize_key_path};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload_order::PreloadWindow;
use pr_imaging::ram_budget::{bytes_for_percent, clamp_cache_percent, minimum_cache_percent};
use pr_imaging::service::SourceChangePhase;
use pr_imaging::service::handle::PreviewHandle;
use pr_imaging::service::{
    DecodeThreadKind, DecodeThreadSpawner, PreviewError, PreviewImageService, PreviewResult, PreviewServiceBuildError,
    PreviewServiceConfig, StdThreadSpawner, resolve_capacity, source_kind_for,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use pr_test_support::{LogLevel, RecordingLog, TempRoot};
use serde_json::Value;
use support::preview_service::{
    CountingFactory, FakeDecoder, GIB, MIB, NeverDecoder, PHYSICAL_32_GIB, TIMEOUT, box_source, config as base_config,
    lock,
};

// ---- helpers --------------------------------------------------------------------------------------------------------

/// Waits for a decode with the hang guard of [`TIMEOUT`].
fn finish(handle: PreviewHandle) -> PreviewResult {
    match handle.wait_timeout(TIMEOUT) {
        Ok(result) => result,
        Err(_) => panic!("the preview did not finish within {TIMEOUT:?}"),
    }
}

fn service_with(config: PreviewServiceConfig) -> PreviewImageService {
    PreviewImageService::new(config).expect("the service builds")
}

/// Writes a small source file named `name` under `root` and returns its path as text.
fn file_in(root: &TempRoot, name: &str) -> String {
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create source directory");
    }
    std::fs::write(&path, [1u8, 2, 3, 4]).expect("write source file");
    path.to_string_lossy().into_owned()
}

fn budget_line_count(log: &RecordingLog) -> usize {
    log.entries()
        .iter()
        .filter(|(level, message)| *level == LogLevel::Info && message.starts_with("Memory budgets:"))
        .count()
}

fn optional_i64(input: &Value, key: &str) -> Option<i64> {
    match &input[key] {
        Value::Null => None,
        value => Some(
            value
                .as_i64()
                .unwrap_or_else(|| panic!("{key} is not an integer: {value}")),
        ),
    }
}

fn window_of(input: &Value) -> Option<PreloadWindow> {
    match &input["window"] {
        Value::Null => None,
        window => Some(PreloadWindow::new(
            i32::try_from(i64_of(window, "forward")).expect("forward fits i32"),
            i32::try_from(i64_of(window, "backward")).expect("backward fits i32"),
        )),
    }
}

fn backend_named(name: &str) -> DecoderBackend {
    match name {
        "Wpf" => DecoderBackend::Wpf,
        "WicDirect" => DecoderBackend::WicDirect,
        "TurboJpeg" => DecoderBackend::TurboJpeg,
        "LibRaw" => DecoderBackend::LibRaw,
        other => panic!("unknown backend {other:?} in the golden"),
    }
}

/// The viewport box of the startup tests: the predicted box of the P-1 launch decode, and a different one.
const PREDICTED: DecodeBox = DecodeBox::new(1280, 768);
const OTHER: DecodeBox = DecodeBox::new(1920, 1152);

// ---- RAM percent (RamCachePercentTests) ------------------------------------------------------------------------------

// C#: RamCachePercentTests.ResolveCapacity_WithPercent_UsesPercentAndLogsShare
#[test]
fn resolve_capacity_with_percent_uses_percent_and_logs_share() {
    // The percent branch ignores the requested bytes (C# passes PerformanceOptions.ImageCacheCapacityBytes here).
    let (capacity, line) = resolve_capacity(16 * GIB, Some(60), PHYSICAL_32_GIB, Some(2 * GIB), None);

    assert_eq!(capacity, bytes_for_percent(60, PHYSICAL_32_GIB) - 2 * GIB);
    assert!(line.starts_with("Memory budgets:"), "{line}");
    assert!(line.contains("cache share 60%"), "{line}");
    assert!(line.contains("source-bytes cache 2048 MiB"), "{line}");
    assert!(!line.contains("clamped"), "{line}");
}

// C#: RamCachePercentTests.ResolveCapacity_OutOfRangePercent_LogsClamp
#[test]
fn resolve_capacity_out_of_range_percent_logs_clamp() {
    let (capacity, line) = resolve_capacity(1, Some(150), PHYSICAL_32_GIB, None, None);

    assert_eq!(capacity, bytes_for_percent(90, PHYSICAL_32_GIB));
    assert!(line.contains("requested 150% clamped to 90%; allowed 4-90%"), "{line}");
    assert!(line.contains("source-bytes cache off"), "{line}");
}

// C#: RamCachePercentTests.ResolveCapacity_LargeWindow_ClampsToThatWindowsFloor (Q-R31)
#[test]
fn resolve_capacity_large_window_clamps_to_that_windows_floor() {
    // 200 ahead + 50 behind + current = 251 UHD previews: the floor for 32 GiB is 25 %.
    let window = PreloadWindow::new(200, 50);
    let floor = minimum_cache_percent(PHYSICAL_32_GIB, window);
    assert_eq!(floor, 25);

    let (capacity, line) = resolve_capacity(1, Some(5), PHYSICAL_32_GIB, None, Some(window));

    assert_eq!(capacity, bytes_for_percent(floor, PHYSICAL_32_GIB));
    assert!(line.contains("requested 5% clamped to 25%; allowed 25-90%"), "{line}");
    // The default window accepts 5 % (its floor is 4 %): the window must reach the clamp itself.
    assert_eq!(clamp_cache_percent(5, PHYSICAL_32_GIB, PreloadWindow::DEFAULT), 5);
}

// C#: RamCachePercentTests.ResolveCapacity_UnknownPhysical_FallsBackToByteBudget
#[test]
fn resolve_capacity_unknown_physical_falls_back_to_byte_budget() {
    let (capacity, line) = resolve_capacity(3 * GIB, Some(50), 0, None, None);

    assert_eq!(capacity, 3 * GIB);
    assert!(line.contains("physical RAM unknown"), "{line}");
    assert!(line.contains("3072 MiB"), "{line}");
}

// C#: RamCachePercentTests.ResolveCapacity_NoPercent_KeepsHalfOfPhysicalClamp
#[test]
fn resolve_capacity_no_percent_keeps_half_of_physical_clamp() {
    let (capacity, line) = resolve_capacity(64 * GIB, None, PHYSICAL_32_GIB, None, None);

    assert_eq!(capacity, 16 * GIB);
    assert!(line.contains("to 50% of physical RAM"), "{line}");
}

// C#: RamCachePercentTests.PreviewImageService_CacheRamPercent_SetsEffectiveCapacity
#[test]
fn preview_image_service_cache_ram_percent_sets_effective_capacity() {
    let root = TempRoot::new("preview-state-percent");
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.capacity_bytes = GIB;
    config.cache_ram_percent = Some(200);

    let service = service_with(config);

    assert_eq!(service.capacity_bytes(), bytes_for_percent(90, PHYSICAL_32_GIB));
}

// ---- budget line (Caching/PreviewImageServiceBudgetLineGapTests) -----------------------------------------------------

// C#: PreviewImageServiceBudgetLineGapTests.ResolveCapacity_PercentLine_HasExactMiBFigures
#[test]
fn resolve_capacity_percent_line_has_exact_mib_figures() {
    let (capacity, line) = resolve_capacity(1, Some(60), PHYSICAL_32_GIB, Some(2 * GIB), None);

    assert_eq!(capacity, 60 * PHYSICAL_32_GIB / 100 - 2 * GIB);
    // The figures are the contract; the surrounding wording may change without breaking this test.
    assert!(line.contains("preview cache 17612 MiB"), "{line}");
    assert!(line.contains("60%"), "{line}");
    assert!(line.contains("32768 MiB physical RAM"), "{line}");
    assert!(line.contains("19660 MiB"), "{line}");
    assert!(line.contains("source-bytes cache 2048 MiB"), "{line}");
}

// C#: PreviewImageServiceBudgetLineGapTests.ResolveCapacity_ClampedLine_HasExactMiBFigures
#[test]
fn resolve_capacity_clamped_line_has_exact_mib_figures() {
    let (_, line) = resolve_capacity(64 * GIB, None, PHYSICAL_32_GIB, None, None);

    // The figures are the contract; the surrounding wording may change without breaking this test.
    assert!(line.contains("preview cache 16384 MiB"), "{line}");
    assert!(line.contains("65536 MiB"), "{line}");
    assert!(line.contains("50%"), "{line}");
    assert!(line.contains("source-bytes cache off"), "{line}");
}

// ---- memory budget clamps (MemoryBudgetClampTests) -------------------------------------------------------------------

// C#: MemoryBudgetClampTests.PreviewService_CapacityBytes_ReportsEffectiveClampedBudget
#[test]
fn preview_service_capacity_bytes_reports_effective_clamped_budget() {
    let root = TempRoot::new("preview-state-clamp");
    let source = Arc::new(SourceBytesCache::new(i64::MAX / 2, PHYSICAL_32_GIB, None).expect("source-bytes cache"));
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.capacity_bytes = i64::MAX / 2;
    config.source_bytes_cache = Some(source.clone());

    let service = service_with(config);

    assert!(service.capacity_bytes() > 0);
    assert!(service.capacity_bytes() + source.capacity_bytes() <= PHYSICAL_32_GIB / 2 + 1);
}

// C#: MemoryBudgetClampTests.PreviewImageService_HugeRequest_LogsClampedEffectiveBudget
#[test]
fn preview_image_service_huge_request_logs_clamped_effective_budget() {
    let root = TempRoot::new("preview-state-huge");
    let log = Arc::new(RecordingLog::new());
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.capacity_bytes = i64::MAX / 2;
    config.log = Some(log.clone());

    let _service = service_with(config);

    let entries = log.entries();
    let lines: Vec<&String> = entries
        .iter()
        .filter(|(level, message)| *level == LogLevel::Info && message.starts_with("Memory budgets:"))
        .map(|(_, message)| message)
        .collect();
    assert_eq!(lines.len(), 1, "exactly one budget line: {entries:?}");
    assert!(lines[0].contains("clamped"), "{}", lines[0]);
    assert!(lines[0].contains("source-bytes cache off"), "{}", lines[0]);
}

// ---- keys (Caching/PreviewImageServiceMutationTests) -----------------------------------------------------------------

// C#: PreviewImageServiceMutationTests.GetCurrentCacheKey_Entry_MatchesPathKey (theory)
#[test]
fn get_current_cache_key_entry_matches_path_key() {
    let cases = [("photo.jpg", false), ("photo.dng", false), ("photo.jpg", true)];
    for (name, original) in cases {
        let root = TempRoot::new("preview-state-entry-key");
        let path = file_in(&root, name);
        let mut config = base_config(root.path(), FakeDecoder::new());
        config.is_original_loading_mode = Arc::new(move || original);
        config.current_backend = Some(Arc::new(|| DecoderBackend::TurboJpeg));
        let service = service_with(config);

        let from_path = service.get_current_cache_key(&path).expect("path key");
        let entry = CatalogEntry::new(path.as_str()).expect("catalog entry");
        let from_entry = service.get_current_cache_key_for_entry(&entry).expect("entry key");

        assert_eq!(from_path, from_entry, "{name} original={original}");
        assert_eq!(from_entry.backend(), DecoderBackend::TurboJpeg, "{name}");
        assert_eq!(from_entry.is_original(), original, "{name}");
        let expected_kind = if name.ends_with(".dng") {
            ImageSourceKind::RAW_PREVIEW
        } else {
            ImageSourceKind::STANDARD
        };
        assert_eq!(from_entry.source_kind(), expected_kind, "{name}");
        assert!(!from_entry.path().is_empty(), "{name}");
    }
}

// C#: PreviewImageServiceMutationTests.GetCurrentCacheKey_DefaultBackend_IsWpf
#[test]
fn get_current_cache_key_default_backend_is_wpf() {
    let root = TempRoot::new("preview-state-default-backend");
    let path = file_in(&root, "a.jpg");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));

    assert_eq!(
        service.get_current_cache_key(&path).expect("key").backend(),
        DecoderBackend::Wpf
    );
}

// C#: PreviewImageServiceMutationTests.MissingPath_LookupsAreAMiss
#[test]
fn missing_path_lookups_are_a_miss() {
    let root = TempRoot::new("preview-state-missing");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));
    let missing = root.join("gone.jpg");

    assert!(
        service
            .try_get_cached_preview_path(&missing.to_string_lossy())
            .is_none()
    );
}

// C#: PreviewImageServiceStartupKeyTests.GetCurrentCacheKey_ExplicitBoxEqualToViewport_EqualsCurrentKey
#[test]
fn get_current_cache_key_explicit_box_equal_to_viewport_equals_current_key() {
    let root = TempRoot::new("preview-state-startup");
    let path = file_in(&root, "launch.jpg");
    let viewport = Arc::new(Mutex::new(PREDICTED));
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.target_decode_box = box_source(viewport);
    let service = service_with(config);

    let current = service.get_current_cache_key(&path).expect("current key");
    let predicted = service
        .get_current_cache_key_for_box(&path, PREDICTED)
        .expect("predicted key");

    assert_eq!(current, predicted);
}

// C#: PreviewImageServiceStartupKeyTests.GetCurrentCacheKey_ExplicitBoxDifferentFromViewport_DiffersFromCurrentKey
#[test]
fn get_current_cache_key_explicit_box_different_from_viewport_differs_from_current_key() {
    let root = TempRoot::new("preview-state-startup-other");
    let path = file_in(&root, "launch.jpg");
    let viewport = Arc::new(Mutex::new(OTHER));
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.target_decode_box = box_source(viewport);
    let service = service_with(config);

    let current = service.get_current_cache_key(&path).expect("current key");
    let predicted = service
        .get_current_cache_key_for_box(&path, PREDICTED)
        .expect("predicted key");

    assert_ne!(current, predicted);
    assert_eq!(predicted.target_box(), PREDICTED);
    assert_eq!(current.target_box(), OTHER);
}

// C#: PreviewImageServiceStartupKeyTests.GetCurrentCacheKey_OriginalMode_ExplicitBoxIsIgnored
#[test]
fn get_current_cache_key_original_mode_explicit_box_is_ignored() {
    let root = TempRoot::new("preview-state-startup-original");
    let path = file_in(&root, "launch.jpg");
    let viewport = Arc::new(Mutex::new(OTHER));
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.is_original_loading_mode = Arc::new(|| true);
    config.target_decode_box = box_source(viewport);
    let service = service_with(config);

    let predicted = service
        .get_current_cache_key_for_box(&path, PREDICTED)
        .expect("predicted key");

    assert_eq!(predicted.target_box(), DecodeBox::UNBOUNDED);
    assert_eq!(predicted, service.get_current_cache_key(&path).expect("current key"));
}

// C#: PreviewImageServiceGapTests2.EvictCachedPath_GivenAnUnnormalizedLowerCasePath_InvalidatesWithTheUpperCasedFullPathAndKeepsOtherEntries
#[test]
fn evict_cached_path_given_an_unnormalized_lower_case_path_invalidates_with_the_upper_cased_full_path() {
    let root = TempRoot::new("preview-state-gap2");
    let evicted = file_in(&root, "sub/a.jpg");
    let kept = file_in(&root, "sub/b.jpg");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));
    finish(service.get_preview_path(&evicted)).expect("decode of a.jpg");
    finish(service.get_preview_path(&kept)).expect("decode of b.jpg");
    assert!(service.try_get_cached_preview_path(&evicted).is_some());
    let seen: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let unnormalized = root
        .join("sub")
        .join("..")
        .join("sub")
        .join("a.jpg")
        .to_string_lossy()
        .into_owned();
    let record = |path: &str| lock(&seen).push(path.to_owned());

    service.evict_cached_path(&unnormalized, Some(&record)).expect("evict");

    let expected = normalize_key_path(&evicted).expect("normalized path");
    assert_eq!(*lock(&seen), vec![expected]);
    assert!(service.try_get_cached_preview_path(&evicted).is_none());
    assert!(service.try_get_cached_preview_path(&kept).is_some());
    assert_eq!(service.cache_count(), 1);
}

// ---- PreviewImageServiceTests (class PreviewImageServiceTests) -------------------------------------------------------

// C#: PreviewImageServiceTests.EvictingPathDropsDecodedBitmap
#[test]
fn evicting_path_drops_decoded_bitmap() {
    let root = TempRoot::new("preview-state-evict");
    let path = file_in(&root, "preview-a.png");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));
    finish(service.get_preview_path(&path)).expect("decode");

    service.evict_cached_path(&path, None).expect("evict");

    assert!(service.try_get_cached_preview_path(&path).is_none() && service.cache_count() == 0);
}

// C#: PreviewImageServiceTests.ClearingPreviewCacheDropsEveryDecodedBitmap
#[test]
fn clearing_preview_cache_drops_every_decoded_bitmap() {
    let root = TempRoot::new("preview-state-clear");
    let path = file_in(&root, "preview-a.png");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));
    finish(service.get_preview_path(&path)).expect("decode");

    service.clear_cache();

    assert!(service.cache_count() == 0 && service.cache_bytes() == 0);
}

// C#: PreviewImageServiceTests.OriginalLoadingModeDecodesAtFullSize
#[test]
fn original_loading_mode_decodes_at_full_size() {
    let root = TempRoot::new("preview-state-original-mode");
    let path = file_in(&root, "preview-a.png");
    let mut original_config = base_config(root.path(), FakeDecoder::new());
    original_config.is_original_loading_mode = Arc::new(|| true);
    let original = service_with(original_config);
    let mut preview_config = base_config(root.path(), FakeDecoder::new());
    preview_config.target_decode_box = Arc::new(|| DecodeBox::new(512, 0));
    let preview = service_with(preview_config);

    let original_key = original.get_current_cache_key(&path).expect("original key");
    let preview_key = preview.get_current_cache_key(&path).expect("preview key");
    assert!(original.is_original_loading_mode());
    assert!(original_key.is_original());
    assert_eq!(original_key.target_width(), 0);
    assert!(!preview_key.is_original());
    assert_eq!(preview_key.target_width(), 512);
}

// C#: PreviewImageServiceTests.ClearCacheDropsSeededOriginalDimensions
#[test]
fn clear_cache_drops_seeded_original_dimensions() {
    let root = TempRoot::new("preview-state-seeded-dims");
    let path = file_in(&root, "preview-a.png");
    let metrics = Arc::new(ReviewMetrics::new());
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.metrics = metrics.clone();
    let service = service_with(config);
    let key = service.get_current_cache_key(&path).expect("key");
    finish(service.get_preview(&path, &key)).expect("decode");

    service.clear_cache();
    let opens_before = metrics.snapshot().source_open_count;
    service.get_original_dimensions(&path, &key).expect("dimensions");

    assert_eq!(metrics.snapshot().source_open_count, opens_before + 1);
}

// ---- decode box (Caching/PreviewDecodeBoxTests) ----------------------------------------------------------------------

// C#: PreviewDecodeBoxTests.ChangedBox_MissesRamCache
#[test]
fn changed_box_misses_ram_cache() {
    let root = TempRoot::new("preview-state-box-change");
    let source = file_in(&root, "source.bin");
    let decoder = FakeDecoder::new();
    let box_value = Arc::new(Mutex::new(DecodeBox::new(2304, 1280)));
    let mut config = base_config(root.path(), decoder.clone());
    config.target_decode_box = box_source(box_value.clone());
    let service = service_with(config);

    finish(service.get_preview_path(&source)).expect("first decode");
    finish(service.get_preview_path(&source)).expect("second request, served from RAM");
    assert_eq!(decoder.requests().len(), 1);

    *lock(&box_value) = DecodeBox::new(2304, 1408);
    assert!(service.try_get_cached_preview_path(&source).is_none());
    finish(service.get_preview_path(&source)).expect("decode at the new box");

    let requests = decoder.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].target_height, 1408);
}

// C#: PreviewDecodeBoxTests.WidthOnlyConstructor_KeepsHeightUnconstrained
#[test]
fn width_only_constructor_keeps_height_unconstrained() {
    let root = TempRoot::new("preview-state-width-only");
    let source = file_in(&root, "source.bin");
    let decoder = FakeDecoder::new();
    let mut config = base_config(root.path(), decoder.clone());
    config.target_decode_box = PreviewServiceConfig::width_only(Arc::new(|| 2208));
    let service = service_with(config);

    let key = service.get_current_cache_key(&source).expect("key");
    finish(service.get_preview(&source, &key)).expect("decode");

    assert_eq!(key.target_box(), DecodeBox::new(2208, 0));
    let requests = decoder.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].decode_box(), DecodeBox::new(2208, 0));
}

// ---- construction (Pixels/Wp06BridgeSplitTests) ----------------------------------------------------------------------

// C#: Wp06BridgeSplitTests.PreviewImageService_NullCodec_Throws
// C#: Wp06BridgeSplitTests.PreviewImageService_NullCodec_WithExplicitDecoder_Throws
// No Rust twin: the platform codec and the payload codec are required (non-`Option`) config fields, so a missing codec
// does not compile.

// C#: Wp06BridgeSplitTests.PreviewImageService_DefaultDecoder_IsWicDirectWithTheCodec
// Rust twin: the WIC default moves to the composition root (W8); without a decoder or factory the build fails.
#[test]
fn new_without_decoder_or_factory_is_an_error() {
    let root = TempRoot::new("preview-state-no-decoder");
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.decoder = None;
    config.decoder_factory = None;

    match PreviewImageService::new(config) {
        Err(PreviewServiceBuildError::NoDecoder) => {}
        other => panic!("expected NoDecoder, got {:?}", other.map(|_| ())),
    }
}

// Rust-only: the factory is asked for its decoder exactly once at construction, and the backend source once as well
// (C#: `_decoder = decoder ?? factory.Create(currentBackend())`).
#[test]
fn factory_create_is_called_once_at_construction_when_no_decoder_is_given() {
    let root = TempRoot::new("preview-state-factory");
    let factory = CountingFactory::new(FakeDecoder::new());
    let backend_calls = Arc::new(AtomicUsize::new(0));
    let backend_counter = backend_calls.clone();
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.decoder = None;
    config.decoder_factory = Some(factory.clone());
    config.current_backend = Some(Arc::new(move || {
        backend_counter.fetch_add(1, Ordering::SeqCst);
        DecoderBackend::Wpf
    }));

    let _service = service_with(config);

    assert_eq!(factory.creates(), 1);
    assert_eq!(backend_calls.load(Ordering::SeqCst), 1);
}

// Rust-only: the constructor logs the "Memory budgets" line exactly once (C# `ResolveCapacity` then `Log.Info`).
#[test]
fn constructor_logs_the_budget_line_once() {
    let root = TempRoot::new("preview-state-budget-log");
    let log = Arc::new(RecordingLog::new());
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.log = Some(log.clone());

    let _service = service_with(config);

    assert_eq!(budget_line_count(&log), 1, "{:?}", log.entries());
}

// ---- RAM cache, epoch and listeners (new in W4-14a) -----------------------------------------------------------------

// Rust-only: the CacheCleared listener runs after the RAM cache and the original dimensions are empty (INV-2, WP-21).
#[test]
fn cache_cleared_listener_runs_after_the_ram_cache_is_empty() {
    let root = TempRoot::new("preview-state-listener");
    let path = file_in(&root, "preview-a.png");
    let service = Arc::new(service_with(base_config(root.path(), FakeDecoder::new())));
    let key = service.get_current_cache_key(&path).expect("key");
    finish(service.get_preview(&path, &key)).expect("decode");
    let observed: Arc<Mutex<Vec<(usize, bool)>>> = Arc::new(Mutex::new(Vec::new()));
    // A weak handle: the service keeps its listeners, so a strong one here would be a cycle.
    let weak = Arc::downgrade(&service);
    let sink = observed.clone();
    let listener_key = key.clone();
    let subscription = service.subscribe_cache_cleared(Arc::new(move || {
        if let Some(service) = weak.upgrade() {
            let dims_known = service.try_get_known_original_dimensions(&listener_key).is_some();
            lock(&sink).push((service.cache_count(), dims_known));
        }
    }));

    service.clear_cache();

    assert_eq!(*lock(&observed), vec![(0, false)]);
    drop(subscription);
}

// Rust-only: dropping a CacheCleared subscription unsubscribes it, and a subscription that outlives the service is harmless.
#[test]
fn cache_cleared_subscription_drop_unsubscribes() {
    let root = TempRoot::new("preview-state-unsubscribe");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let subscription = service.subscribe_cache_cleared(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    service.clear_cache();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(subscription);
    service.clear_cache();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let late = service.subscribe_cache_cleared(Arc::new(|| {}));
    drop(service);
    drop(late);
}

// Rust-only: a path that normalizes to nothing evicts nothing and fails before the cache is touched.
#[test]
fn evict_cached_path_bad_path_evicts_nothing() {
    let root = TempRoot::new("preview-state-bad-evict");
    let path = file_in(&root, "preview-a.png");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));
    finish(service.get_preview_path(&path)).expect("decode");

    let result = service.evict_cached_path("", None);

    assert!(matches!(result, Err(PreviewError::InvalidPath(_))), "{result:?}");
    assert_eq!(service.cache_count(), 1);
    assert!(service.try_get_cached_preview_path(&path).is_some());
}

// Rust-only: a lookup by a blank path is a quiet miss (DIVERGENCE from C#: `new FileInfo("")` throws there, see
// contract Open questions 3).
#[test]
fn try_get_cached_preview_path_bad_path_is_a_miss() {
    let root = TempRoot::new("preview-state-bad-lookup");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));

    assert!(service.try_get_cached_preview_path("").is_none());
}

// Rust-only: the source kind of a path is RAW exactly for the RAW extensions, in any case (C# `SourceKindFor`).
#[test]
fn source_kind_for_every_raw_extension() {
    for extension in RAW_EXTENSIONS {
        let lower = format!("photo{extension}");
        let upper = format!("PHOTO{}", extension.to_uppercase());
        assert_eq!(source_kind_for(&lower), ImageSourceKind::RAW_PREVIEW, "{lower}");
        assert_eq!(source_kind_for(&upper), ImageSourceKind::RAW_PREVIEW, "{upper}");
    }
    for near_miss in ["x.dngx", "x.dng.", "x.tif", "x.heic", "README", "x.webp", "x.jpg"] {
        assert_eq!(source_kind_for(near_miss), ImageSourceKind::STANDARD, "{near_miss}");
    }
}

// Rust-only: a path `Path.GetFullPath` rejects (NUL, only spaces) is a quiet miss too (the same DIVERGENCE as the blank
// path: C# lets the `ArgumentException` escape).
#[test]
fn try_get_cached_preview_path_invalid_characters_are_a_miss() {
    let root = TempRoot::new("preview-state-nul-lookup");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));

    assert!(service.try_get_cached_preview_path("a\0b.jpg").is_none());
    assert!(service.try_get_cached_preview_path("   ").is_none());
}

// C# `ClearSourceBytesCache()`: forwards to the injected source-bytes cache, which is empty afterwards.
#[test]
fn clear_source_bytes_cache_empties_the_injected_source_bytes_cache() {
    let root = TempRoot::new("preview-state-clear-source-bytes");
    let path = file_in(&root, "a.jpg");
    let source = Arc::new(SourceBytesCache::new(64 * 1024 * 1024, PHYSICAL_32_GIB, None).expect("source-bytes cache"));
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.source_bytes_cache = Some(source.clone());
    let service = service_with(config);
    source
        .get_or_read(std::path::Path::new(&path))
        .expect("read the source");
    assert_eq!(source.count(), 1);

    service.clear_source_bytes_cache();

    assert_eq!(source.count(), 0);
    assert_eq!(source.current_size(), 0);
}

// Rust-only: without a source-bytes cache the clear is a no-op (C# `_sourceBytesCache?.Clear()`).
#[test]
fn clear_source_bytes_cache_without_a_cache_is_a_no_op() {
    let root = TempRoot::new("preview-state-clear-no-source-bytes");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));

    service.clear_source_bytes_cache();

    assert_eq!(service.cache_count(), 0);
}

// ---- PreviewError predicates (C# `catch` filters) -------------------------------------------------------------------

fn decode(error: DecodeError) -> PreviewError {
    PreviewError::Decode(Arc::new(error))
}

// C# `catch (OperationCanceledException)`: own cancel, a canceled disk read and a canceled decoder; nothing else.
#[test]
fn preview_error_is_canceled_matches_every_canceled_shape() {
    assert!(PreviewError::Canceled.is_canceled());
    assert!(PreviewError::DiskEntry(Arc::new(PreviewCacheError::Canceled)).is_canceled());
    assert!(decode(DecodeError::Canceled).is_canceled());

    assert!(
        !decode(DecodeError::Format {
            message: "bad".to_owned()
        })
        .is_canceled()
    );
    assert!(!PreviewError::DiskEntry(Arc::new(PreviewCacheError::InvalidData("bad".to_owned()))).is_canceled());
    assert!(!PreviewError::RawFullDecodeKey.is_canceled());
}

// C# `catch (DecoderBusyException)`: only a busy decoder.
#[test]
fn preview_error_is_busy_only_for_a_busy_decoder() {
    assert!(
        decode(DecodeError::Busy {
            message: "full".to_owned()
        })
        .is_busy()
    );

    assert!(!decode(DecodeError::Canceled).is_busy());
    assert!(
        !decode(DecodeError::Format {
            message: "bad".to_owned()
        })
        .is_busy()
    );
    assert!(!PreviewError::Canceled.is_busy());
    assert!(!PreviewError::Panicked("boom".to_owned()).is_busy());
}

// C# `ex is IOException`: every IOException shape is true, the argument and non-IO shapes next to them are false.
#[test]
fn preview_error_is_io_exception_matches_the_csharp_io_exception_family() {
    let io = || Arc::new(std::io::Error::other("disk"));
    assert!(PreviewError::Io(io()).is_io_exception());
    assert!(
        PreviewError::SourceChanged {
            path: "a.jpg".to_owned(),
            phase: SourceChangePhase::Decode,
        }
        .is_io_exception()
    );
    assert!(PreviewError::SourceBytes(SourceBytesError::Io(io())).is_io_exception());
    assert!(
        !PreviewError::SourceBytes(SourceBytesError::Argument { param_name: "path" }).is_io_exception(),
        "ArgumentException is not an IOException"
    );
    assert!(
        PreviewError::Key(CacheKeyError::SourceNoLongerExists {
            full_name: "a.jpg".to_owned(),
        })
        .is_io_exception()
    );
    assert!(PreviewError::Key(CacheKeyError::InvalidPath(PathError::TooLong)).is_io_exception());
    assert!(!PreviewError::Key(CacheKeyError::InvalidPath(PathError::Argument)).is_io_exception());
    assert!(PreviewError::InvalidPath(PathError::TooLong).is_io_exception());
    assert!(!PreviewError::InvalidPath(PathError::Argument).is_io_exception());
    assert!(
        decode(DecodeError::FileNotFound {
            message: "gone".to_owned()
        })
        .is_io_exception()
    );
    assert!(
        decode(DecodeError::MemoryAdmission {
            message: "big".to_owned()
        })
        .is_io_exception()
    );
    assert!(
        !decode(DecodeError::Format {
            message: "bad".to_owned()
        })
        .is_io_exception()
    );
    assert!(!PreviewError::Canceled.is_io_exception());
}

// `std::error::Error::source`: a wrapping variant exposes its inner error, the leaf variants have none.
#[test]
fn preview_error_source_is_the_wrapped_error() {
    let key = PreviewError::Key(CacheKeyError::InvalidPath(PathError::TooLong));
    let inner = key.source().expect("Key wraps the key error");
    assert_eq!(inner.to_string(), PathError::TooLong.to_string());

    let decode_error = decode(DecodeError::Format {
        message: "bad header".to_owned(),
    });
    assert_eq!(
        decode_error.source().map(ToString::to_string).as_deref(),
        Some("bad header")
    );

    let io = PreviewError::Io(Arc::new(std::io::Error::other("disk gone")));
    assert_eq!(io.source().map(ToString::to_string).as_deref(), Some("disk gone"));

    assert!(PreviewError::Canceled.source().is_none());
    assert!(PreviewError::RawFullDecodeKey.source().is_none());
    assert!(PreviewError::Panicked("boom".to_owned()).source().is_none());
}

// ---- golden checks --------------------------------------------------------------------------------------------------

/// `tests/golden/preview-budget-line.json`: `resolve_capacity` for every case (`capacity` and `line`).
#[test]
fn resolve_capacity_matches_every_preview_budget_line_golden_case() {
    let golden = golden::load("preview-budget-line");
    let mut failures = Vec::new();

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = &case["input"];
        let percent = optional_i64(input, "percent").map(|p| i32::try_from(p).expect("percent fits i32"));
        let (capacity, line) = resolve_capacity(
            i64_of(input, "requested"),
            percent,
            i64_of(input, "physical"),
            optional_i64(input, "source"),
            window_of(input),
        );
        let expected_capacity = i64_of(case, "capacity");
        if capacity != expected_capacity {
            failures.push(format!("{name}: capacity {capacity}, expected {expected_capacity}"));
        }
        let expected_line = str_of(case, "line");
        if line != expected_line {
            failures.push(format!("{name}: line {line:?}, expected {expected_line:?}"));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} budget-line cases differ:\n{}",
        failures.len(),
        golden.cases.len(),
        failures.join("\n")
    );
}

/// Expected-versus-actual check of one key field, collected so one run reports every difference.
fn check<T: PartialEq + std::fmt::Debug>(failures: &mut Vec<String>, name: &str, field: &str, got: T, want: T) {
    if got != want {
        failures.push(format!("{name}: {field} = {got:?}, expected {want:?}"));
    }
}

/// `tests/golden/preview-current-key.json`: the entry key (`GetCurrentCacheKey(CatalogEntry)`) and its call counts.
#[test]
fn current_cache_key_for_entry_matches_every_preview_current_key_golden_case() {
    let golden = golden::load("preview-current-key");
    let root = TempRoot::new("preview-state-golden-key");
    let mut failures = Vec::new();

    for case in &golden.cases {
        let name = str_of(case, "name");
        let input = &case["input"];
        let path = str_of(input, "path");

        if case["result"].get("throws").is_some() {
            // C# `new CatalogEntry("")` throws before the service is called: the entry does not exist.
            if CatalogEntry::new(path).is_ok() {
                failures.push(format!("{name}: CatalogEntry::new({path:?}) should fail"));
            }
            continue;
        }

        let mut entry = CatalogEntry::new(path).unwrap_or_else(|e| panic!("{name}: CatalogEntry::new: {e}"));
        entry.length = Some(i64_of(input, "length"));
        entry.last_write_utc = Some(
            DateTimeUtc::from_ticks(i64_of(input, "ticks")).unwrap_or_else(|| panic!("{name}: ticks out of range")),
        );

        let mode_calls = Arc::new(AtomicUsize::new(0));
        let box_calls = Arc::new(AtomicUsize::new(0));
        let backend_calls = Arc::new(AtomicUsize::new(0));
        let original = bool_of(input, "original_mode");
        let width = i32::try_from(i64_of(input, "w")).expect("w fits i32");
        let height = i32::try_from(i64_of(input, "h")).expect("h fits i32");
        let width_only = input.get("width_only").and_then(Value::as_bool).unwrap_or(false);

        let mut config = base_config(root.path(), Arc::new(NeverDecoder));
        let mode = mode_calls.clone();
        config.is_original_loading_mode = Arc::new(move || {
            mode.fetch_add(1, Ordering::SeqCst);
            original
        });
        let box_counter = box_calls.clone();
        config.target_decode_box = if width_only {
            PreviewServiceConfig::width_only(Arc::new(move || {
                box_counter.fetch_add(1, Ordering::SeqCst);
                width
            }))
        } else {
            Arc::new(move || {
                box_counter.fetch_add(1, Ordering::SeqCst);
                DecodeBox::new(width, height)
            })
        };
        if let Some(backend) = input["backend"].as_str() {
            let backend = backend_named(backend);
            let counter = backend_calls.clone();
            config.current_backend = Some(Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                backend
            }));
        }
        let service = service_with(config);
        mode_calls.store(0, Ordering::SeqCst);
        box_calls.store(0, Ordering::SeqCst);
        backend_calls.store(0, Ordering::SeqCst);

        let key = match service.get_current_cache_key_for_entry(&entry) {
            Ok(key) => key,
            Err(error) => {
                failures.push(format!("{name}: key error {error}"));
                continue;
            }
        };

        let expected = &case["result"];
        check(&mut failures, name, "path", key.path(), str_of(expected, "path"));
        check(&mut failures, name, "length", key.length(), i64_of(expected, "length"));
        check(
            &mut failures,
            name,
            "ticks",
            key.last_write_utc_ticks(),
            i64_of(expected, "ticks"),
        );
        check(
            &mut failures,
            name,
            "is_original",
            key.is_original(),
            bool_of(expected, "is_original"),
        );
        check(
            &mut failures,
            name,
            "width",
            i64::from(key.target_width()),
            i64_of(expected, "width"),
        );
        check(
            &mut failures,
            name,
            "height",
            i64::from(key.target_height()),
            i64_of(expected, "height"),
        );
        check(
            &mut failures,
            name,
            "box_unbounded",
            key.target_box().is_unbounded(),
            bool_of(expected, "box_unbounded"),
        );
        check(
            &mut failures,
            name,
            "orientation_applied",
            key.orientation_applied(),
            bool_of(expected, "orientation_applied"),
        );
        check(
            &mut failures,
            name,
            "backend",
            format!("{:?}", key.backend()),
            str_of(expected, "backend").to_owned(),
        );
        check(
            &mut failures,
            name,
            "source_kind",
            i64::from(key.source_kind()),
            i64_of(expected, "source_kind"),
        );
        check(
            &mut failures,
            name,
            "mode_calls",
            i64::try_from(mode_calls.load(Ordering::SeqCst)).expect("count fits i64"),
            i64_of(case, "mode_calls"),
        );
        check(
            &mut failures,
            name,
            "box_calls",
            i64::try_from(box_calls.load(Ordering::SeqCst)).expect("count fits i64"),
            i64_of(case, "box_calls"),
        );
        check(
            &mut failures,
            name,
            "backend_calls",
            i64::try_from(backend_calls.load(Ordering::SeqCst)).expect("count fits i64"),
            i64_of(case, "backend_calls"),
        );
    }

    assert!(
        failures.is_empty(),
        "{} differences in {} key cases:\n{}",
        failures.len(),
        golden.cases.len(),
        failures.join("\n")
    );
}

// ---- repair 2: public behaviour pinned (constants, texts, spawner, RAW full-decode switch) ---------------------------

// C# constructor defaults: `capacityBytes = 16L * 1024 * 1024 * 1024`, `diskCacheCapacityBytes = 4L * 1024 * 1024 * 1024`.
#[test]
fn default_capacities_are_16_gib_ram_and_4_gib_disk() {
    use pr_imaging::service::{
        DEFAULT_CAPACITY_BYTES, DEFAULT_DISK_CACHE_CAPACITY_BYTES, DEFAULT_ORIGINAL_DIMENSIONS_CAPACITY,
        VIEWER_DECODE_SLOTS,
    };
    assert_eq!(DEFAULT_CAPACITY_BYTES, 17_179_869_184);
    assert_eq!(DEFAULT_DISK_CACHE_CAPACITY_BYTES, 4_294_967_296);
    assert_eq!(DEFAULT_ORIGINAL_DIMENSIONS_CAPACITY, 200_000);
    assert_eq!(VIEWER_DECODE_SLOTS, 2);

    let config = PreviewServiceConfig::new(
        Arc::new(ReviewMetrics::new()),
        Arc::new(|| false),
        Arc::new(|| DecodeBox::new(512, 512)),
        Arc::new(support::preview_service::PixelsCodec),
        Arc::new(support::preview_service::FakePayloadCodec),
    );
    assert_eq!(config.capacity_bytes, 17_179_869_184);
    assert_eq!(config.disk_cache_capacity_bytes, 4_294_967_296);
}

// C# `UserFacingError.Localized` keys of the two "source changed" sentences.
#[test]
fn source_change_phase_catalog_keys() {
    assert_eq!(
        SourceChangePhase::Decode.catalog_key(),
        "err.io.sourceChangedDuringDecode"
    );
    assert_eq!(
        SourceChangePhase::ReadingDimensions.catalog_key(),
        "err.io.sourceChangedReadingDimensions"
    );
}

#[test]
fn preview_error_catalog_key_only_for_source_changed() {
    let decode_phase = PreviewError::SourceChanged {
        path: "C:\\x\\a.jpg".to_owned(),
        phase: SourceChangePhase::Decode,
    };
    assert_eq!(
        decode_phase.catalog_key(),
        Some(("err.io.sourceChangedDuringDecode", "C:\\x\\a.jpg"))
    );
    let dims = PreviewError::SourceChanged {
        path: "b.nef".to_owned(),
        phase: SourceChangePhase::ReadingDimensions,
    };
    assert_eq!(
        dims.catalog_key(),
        Some(("err.io.sourceChangedReadingDimensions", "b.nef"))
    );
    assert_eq!(PreviewError::Canceled.catalog_key(), None);
    assert_eq!(PreviewError::RawFullDecodeKey.catalog_key(), None);
    assert_eq!(PreviewError::Panicked("boom".to_owned()).catalog_key(), None);
}

// C# `Exception.Message` texts of the errors this service throws itself; inner errors keep their own text.
#[test]
fn preview_error_display_is_the_csharp_message() {
    assert_eq!(PreviewError::Canceled.to_string(), "The operation was canceled.");
    assert_eq!(
        PreviewError::SourceChanged {
            path: "C:\\p\\a.jpg".to_owned(),
            phase: SourceChangePhase::Decode
        }
        .to_string(),
        "Image source changed during decode: C:\\p\\a.jpg"
    );
    assert_eq!(
        PreviewError::SourceChanged {
            path: "C:\\p\\a.nef".to_owned(),
            phase: SourceChangePhase::ReadingDimensions
        }
        .to_string(),
        "Image source changed while reading dimensions: C:\\p\\a.nef"
    );
    assert_eq!(
        PreviewError::RawFullDecodeKey.to_string(),
        "DecodeAndCache never handles RawFullDecode keys; use DecodeOriginalAsync."
    );
    assert_eq!(
        PreviewError::RawFullDecodeKey.to_string(),
        PreviewError::RAW_FULL_DECODE_KEY_MESSAGE
    );
    assert_eq!(
        PreviewError::Panicked("worker panicked".to_owned()).to_string(),
        "worker panicked"
    );
    assert_eq!(
        PreviewError::Io(Arc::new(std::io::Error::other("disk gone"))).to_string(),
        "disk gone"
    );
    assert_eq!(
        PreviewError::Spawn(Arc::new(std::io::Error::other("no threads"))).to_string(),
        "no threads"
    );
    let inner = DecodeError::InvalidOperation {
        message: "inner text".to_owned(),
    };
    let expected = inner.to_string();
    assert!(!expected.is_empty());
    assert_eq!(decode(inner).to_string(), expected);
    assert_eq!(
        PreviewError::InvalidPath(PathError::TooLong).to_string(),
        PathError::TooLong.to_string()
    );
}

#[test]
fn preview_service_build_error_display_texts() {
    assert_eq!(
        PreviewServiceBuildError::NoDecoder.to_string(),
        "PreviewImageService needs a decoder or a decoder factory (the platform default is wired by the composition root)."
    );
    assert_eq!(
        PreviewServiceBuildError::OriginalDimensionsCapacity(0).to_string(),
        "Capacity must be positive. (Parameter 'originalDimensionsCapacity') Actual value was 0."
    );
    assert_eq!(
        PreviewServiceBuildError::Capacity(-1).to_string(),
        "Capacity must be positive. (Parameter 'capacityBytes') Actual value was -1."
    );
    assert_eq!(
        PreviewServiceBuildError::Spawn(std::io::Error::other("no threads")).to_string(),
        "A preview-service background thread could not be started: no threads"
    );
}

// C# `BoundedLruCache` constructor: `originalDimensionsCapacity <= 0` throws; the Rust build reports the value.
#[test]
fn original_dimensions_capacity_zero_is_a_build_error() {
    let root = TempRoot::new("preview-state-dims-capacity");
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.original_dimensions_capacity = 0;

    match PreviewImageService::new(config) {
        Err(error @ PreviewServiceBuildError::OriginalDimensionsCapacity(0)) => {
            assert!(error.to_string().contains("Actual value was 0."), "{error}");
        }
        other => panic!("expected OriginalDimensionsCapacity(0), got {:?}", other.map(|_| ())),
    }
}

// Contract semantics 11: one named, detached thread per job; the job really runs.
#[test]
fn std_thread_spawner_runs_the_job_on_a_named_thread() {
    let cases = [
        (DecodeThreadKind::Preview, "PhotoReviewRs.PreviewDecode"),
        (DecodeThreadKind::Viewer, "PhotoReviewRs.ViewerDecode"),
        (DecodeThreadKind::Original, "PhotoReviewRs.OriginalDecode"),
        (DecodeThreadKind::PersistWorker, "PhotoReviewRs.PreviewPersist"),
        (DecodeThreadKind::StartupCleanup, "PhotoReviewRs.PreviewCacheCleanup"),
    ];
    for (kind, expected) in cases {
        let (sender, receiver) = std::sync::mpsc::channel();
        StdThreadSpawner
            .spawn(
                kind,
                Box::new(move || {
                    let name = std::thread::current().name().map(str::to_owned);
                    let _ = sender.send(name);
                }),
            )
            .expect("spawn");
        let name = receiver.recv_timeout(TIMEOUT).expect("the job ran");
        assert_eq!(name.as_deref(), Some(expected), "{kind:?}");
    }
}

// C# `IsRawFullDecodeRequest`: `_rawFullDecoder != null && _isRawFullDecodeEnabled() && IsRawPath(path)`.
#[test]
fn is_raw_full_decode_request_needs_decoder_switch_and_raw_extension() {
    let root = TempRoot::new("preview-state-raw-full");
    let build = |raw_decoder: bool, enabled: bool, calls: Arc<AtomicUsize>| {
        let mut config = base_config(root.path(), FakeDecoder::new());
        if raw_decoder {
            config.raw_full_decoder = Some(FakeDecoder::new());
        }
        config.is_raw_full_decode_enabled = Some(Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            enabled
        }));
        service_with(config)
    };

    let calls = Arc::new(AtomicUsize::new(0));
    let service = build(true, true, calls.clone());
    assert!(service.is_raw_full_decode_request("C:\\p\\a.NEF"));
    assert!(service.is_raw_full_decode_request("C:\\p\\a.dng"));
    assert!(!service.is_raw_full_decode_request("C:\\p\\a.jpg"));
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    let calls = Arc::new(AtomicUsize::new(0));
    let service = build(true, false, calls.clone());
    assert!(!service.is_raw_full_decode_request("C:\\p\\a.nef"));
    assert!(!service.is_raw_full_decode_request("C:\\p\\a.jpg"));

    // No RAW full decoder: false, and the enabled source is never read (C# `&&` short-circuit).
    let calls = Arc::new(AtomicUsize::new(0));
    let service = build(false, true, calls.clone());
    assert!(!service.is_raw_full_decode_request("C:\\p\\a.nef"));
    assert!(!service.is_raw_full_decode_request("C:\\p\\a.jpg"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// C# `ActiveViewerDecodes` starts at 0 (no viewer decode runs on a fresh service).
#[test]
fn active_viewer_decodes_is_zero_on_a_fresh_service() {
    let root = TempRoot::new("preview-state-active-viewer");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));
    assert_eq!(service.active_viewer_decodes(), 0);
}

// ---- repair 3: mutation killers (open verdict of the first pass: surviving mutants and the Drop persist-queue case) ----

/// The `disable_disk_cache` value the service resolved, read from its `Debug` output (the field is private).
fn disk_cache_disabled(service: &PreviewImageService) -> bool {
    let text = format!("{service:?}");
    if text.contains("disable_disk_cache: true") {
        true
    } else if text.contains("disable_disk_cache: false") {
        false
    } else {
        panic!("Debug output without the disk switch: {text}")
    }
}

// C# `_disableDiskCache = disableDiskCacheOverride ?? (diskCacheCapacityBytes <= 0 || PHOTOREVIEW_DIAG_DISABLE_DISKCACHE)`
// (D10, F-IMG-4, trap 8: `Some(false)` beats a zero capacity and the diagnostic switch).
#[test]
fn disk_cache_switch_follows_override_then_capacity_then_diagnostic_variable() {
    use pr_core::diagnostics::diag_options::DiagOptions;
    let root = TempRoot::new("preview-state-disk-switch");
    // (override, disk capacity, diagnostic switch, expected "disabled")
    let cases = [
        (None, 4 * GIB, false, false),
        (None, 1, false, false),
        (None, 0, false, true),
        (None, -1, false, true),
        (None, 4 * GIB, true, true),
        (None, 0, true, true),
        (Some(false), 0, true, false),
        (Some(false), 4 * GIB, false, false),
        (Some(true), 4 * GIB, false, true),
    ];
    for (index, (override_value, capacity, diag_disable, expected)) in cases.into_iter().enumerate() {
        let dir = root.join(format!("case-{index}"));
        let mut config = base_config(&dir, FakeDecoder::new());
        config.disable_disk_cache_override = override_value;
        config.disk_cache_capacity_bytes = capacity;
        config.diag = Some(Arc::new(DiagOptions {
            disable_disk_cache: diag_disable,
            ..DiagOptions::default()
        }));
        let service = service_with(config);
        if !expected {
            // The startup cleanup runs while the disk cache is on: let it finish before the directory goes away.
            assert!(service.wait_startup_cleanup(TIMEOUT), "case {index}: startup cleanup");
        }

        assert_eq!(
            disk_cache_disabled(&service),
            expected,
            "case {index}: override {override_value:?}, capacity {capacity}, diag {diag_disable}"
        );
    }
}

// C# `DiskDirectory`: the configured directory, else the app's own cache directory (R0002: `PhotoReviewRs\cache`).
#[test]
fn disk_directory_is_the_configured_one_or_the_app_cache_directory() {
    let root = TempRoot::new("preview-state-disk-dir");
    let configured = root.join("previews");
    let service = service_with(base_config(&configured, FakeDecoder::new()));
    assert_eq!(service.disk_directory(), configured.as_path());

    let mut config = base_config(root.path(), FakeDecoder::new());
    config.disk_cache_directory = None;
    // Disabled: nothing is written to (or cleaned in) the real app cache directory.
    config.disable_disk_cache_override = Some(true);
    let service = service_with(config);
    assert_eq!(
        service.disk_directory(),
        pr_core::app_paths::AppPaths::from_environment()
            .preview_cache_dir
            .as_path()
    );
}

// The `Debug` impls name their type and the effective values (no C# twin; a `Debug` that prints nothing hides them).
#[test]
fn debug_output_names_the_type_and_the_effective_values() {
    let root = TempRoot::new("preview-state-debug");
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.capacity_bytes = 3 * MIB;
    let config_text = format!("{config:?}");
    assert!(config_text.starts_with("PreviewServiceConfig"), "{config_text}");
    assert!(config_text.contains("capacity_bytes: 3145728"), "{config_text}");
    assert!(
        config_text.contains("disk_cache_capacity_bytes: 4294967296"),
        "{config_text}"
    );

    let service = service_with(config);
    let service_text = format!("{service:?}");
    assert!(service_text.starts_with("PreviewImageService"), "{service_text}");
    assert!(service_text.contains("capacity_bytes: 3145728"), "{service_text}");

    let first = service.subscribe_cache_cleared(Arc::new(|| {}));
    let second = service.subscribe_cache_cleared(Arc::new(|| {}));
    let first_text = format!("{first:?}");
    let second_text = format!("{second:?}");
    assert!(first_text.starts_with("CacheClearedSubscription"), "{first_text}");
    assert_ne!(first_text, second_text, "each subscription has its own id");
}

// C# `CacheCleared` is a multicast delegate: handlers run in subscription order; dropping one subscription removes only
// that handler (trap 10: a `Vec`, not a `HashMap`).
#[test]
fn cache_cleared_listeners_run_in_subscription_order_and_unsubscribe_one_by_one() {
    let root = TempRoot::new("preview-state-listener-order");
    let service = service_with(base_config(root.path(), FakeDecoder::new()));
    let order = Arc::new(Mutex::new(Vec::<u32>::new()));
    let mut subscriptions = Vec::new();
    for tag in 1..=4u32 {
        let sink = order.clone();
        subscriptions.push(service.subscribe_cache_cleared(Arc::new(move || lock(&sink).push(tag))));
    }

    service.clear_cache();
    assert_eq!(*lock(&order), vec![1, 2, 3, 4]);

    lock(&order).clear();
    let third = subscriptions.remove(2);
    drop(third);
    service.clear_cache();
    assert_eq!(*lock(&order), vec![1, 2, 4]);
}

// R0003 / semantics 7: the listeners run with no service lock held, so a listener may subscribe, unsubscribe or clear
// again. Run on a helper thread with the hang guard: a lock held across the call would deadlock.
#[test]
fn cache_cleared_listener_may_reenter_the_service() {
    let root = TempRoot::new("preview-state-listener-reentry");
    let service = Arc::new(service_with(base_config(root.path(), FakeDecoder::new())));
    let calls = Arc::new(AtomicUsize::new(0));
    let weak = Arc::downgrade(&service);
    let counter = calls.clone();
    let subscription = service.subscribe_cache_cleared(Arc::new(move || {
        let call = counter.fetch_add(1, Ordering::SeqCst);
        if call == 0
            && let Some(service) = weak.upgrade()
        {
            let extra = service.subscribe_cache_cleared(Arc::new(|| {}));
            drop(extra);
            service.clear_cache();
        }
    }));
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = service.clone();
    std::thread::spawn(move || {
        worker.clear_cache();
        let _ = sender.send(());
    });

    if receiver.recv_timeout(TIMEOUT).is_err() {
        // A deadlocked helper still holds the listener lock: dropping the subscription would block this thread too.
        std::mem::forget(subscription);
        std::mem::forget(service);
        panic!("clear_cache with a re-entrant listener did not finish within {TIMEOUT:?}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    drop(subscription);
}

// C# `EvictCachedPath`: `_sourceBytesCache?.Evict(path)` (a moved/deleted file must not keep its raw bytes in RAM).
#[test]
fn evict_cached_path_also_evicts_the_source_bytes_of_that_path_only() {
    let root = TempRoot::new("preview-state-evict-source-bytes");
    let evicted = file_in(&root, "a.jpg");
    let kept = file_in(&root, "b.jpg");
    let source = Arc::new(SourceBytesCache::new(64 * 1024 * 1024, PHYSICAL_32_GIB, None).expect("source-bytes cache"));
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.source_bytes_cache = Some(source.clone());
    let service = service_with(config);
    source.get_or_read(std::path::Path::new(&evicted)).expect("read a");
    source.get_or_read(std::path::Path::new(&kept)).expect("read b");
    assert_eq!(source.count(), 2);

    service.evict_cached_path(&evicted, None).expect("evict");

    assert_eq!(source.count(), 1);
}

/// Starts the first persist worker (and reports when its job returned), then refuses every later thread.
struct FailSecondSpawner {
    started: Mutex<usize>,
    finished: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl DecodeThreadSpawner for FailSecondSpawner {
    fn spawn(&self, kind: DecodeThreadKind, job: Box<dyn FnOnce() + Send + 'static>) -> std::io::Result<()> {
        let mut started = lock(&self.started);
        *started += 1;
        if *started > 1 {
            return Err(std::io::Error::other("no more threads"));
        }
        let sender = lock(&self.finished).take().expect("one sender");
        StdThreadSpawner.spawn(
            kind,
            Box::new(move || {
                job();
                let _ = sender.send(());
            }),
        )
    }
}

// A background thread that cannot start fails the build with `Spawn`, and the worker that did start is told to finish
// (the persist queue is completed) instead of blocking forever with the service state.
#[test]
fn new_with_a_failing_spawner_is_a_spawn_error_and_the_started_worker_exits() {
    let root = TempRoot::new("preview-state-spawn-fail");
    let (sender, receiver) = std::sync::mpsc::channel();
    let spawner = Arc::new(FailSecondSpawner {
        started: Mutex::new(0),
        finished: Mutex::new(Some(sender)),
    });
    let mut config = base_config(root.path(), FakeDecoder::new());
    config.spawner = Some(spawner);

    match PreviewImageService::new(config) {
        Err(error @ PreviewServiceBuildError::Spawn(_)) => {
            assert!(error.to_string().ends_with("no more threads"), "{error}");
        }
        other => panic!("expected Spawn, got {:?}", other.map(|_| ())),
    }
    receiver
        .recv_timeout(TIMEOUT)
        .expect("the started persist worker exited");
}
