//! Port of `PhotoReview.Integration.Tests` benchmark workload tests (C# baseline d92d6aae): `BenchmarkSelectionGoldenTests`,
//! `BenchmarkStatisticsEdgeTests` (ImagesPerSample rows only), `BenchmarkTrustTests` (RunProfile stamping only),
//! `BenchmarkEngineTimingTests` (RunProfile setup exclusion only), `BenchmarkWorkloadRunnerTests`, plus golden checks
//! over `tests/golden/{bench-random, bench-select-indices, bench-images-per-sample, bench-prepare-edge}.json`
//! (oracle `oracle/W7-03c.cs`).
//!
//! Port of `PhotoReview.Benchmarking/BenchmarkWorkloadRunner.cs` (C# baseline d92d6aae). Unit W7-03c, contract
//! `contracts/W7-03c.md`. Red stage: every stub body is `todo!()`, so each test here fails until the porter implements it.
//!
//! Documented skips (not ported here, C# names kept in the report):
//! - `BenchmarkStatisticsEdgeTests`: `EffectiveImagesPerSample_IsAtLeastOne`, the ranking and phase-result tests (W7-01).
//! - `BenchmarkTrustTests`: the decoder-bench, executor-window, profile-scope and wait-idle tests (W7-01, W7-03b, W7-03d).
//! - `BenchmarkEngineTimingTests`: the `RunPreparedAsync_*` tests (W7-03a).
//! - `BenchmarkWorkloadRunnerTests.WarmNextWorkloadRecordsPreloadHitAfterWarming`: exercises only the executor (W7-03b).
//!
//! File-action tests use the fake decoder of `crates/pr-imaging/tests/support/preview_service.rs` (contract, open
//! question 5) and a recording bin that only ever removes the scratch file it is handed. The real Recycle Bin is never used.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

#[path = "../../pr-imaging/tests/support/preview_service.rs"]
mod preview_service;

use pr_bench::engine::{BenchmarkError, MeasureOutcome, PreparedMeasure, RunPreparedOptions, TimeSource};
use pr_bench::image_executor::BenchmarkProfile as ExecutorProfile;
use pr_bench::image_executor::{BenchmarkExecutorOptions, BenchmarkImageExecutor, HasHeadroomFn};
use pr_bench::models::{BenchmarkProfile, BenchmarkProfileProblem, BenchmarkWorkload};
use pr_bench::profiles;
use pr_bench::workload_runner::{
    DotNetRandom, FileActionOp, WorkloadError, create_seeded_random, file_action_op, images_per_sample,
    prepare_iteration, registry_images_per_sample, run_profile_with, select_parallel_indices,
};
use pr_core::abstractions::{Clock, Log, NullLog, RecycleBin};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use pr_imaging::decoder_traits::ImageDecoder;
use pr_test_support::golden::{self, i64_of, str_of};
use pr_test_support::{FakeClock, TempRoot};
use serde_json::Value;

use preview_service::{FakeDecoder, FakePayloadCodec, PixelsCodec};

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Physical RAM handed to the executor (32 GiB, the same fixed value the W4-14 service tests use).
const PHYSICAL_MEMORY_BYTES: i64 = 32 * 1024 * 1024 * 1024;

/// The registry profile with this exact id, or, for an id the registry does not know, the first registry profile with
/// its id replaced (the golden cases with custom ids only read `workers`, `workload` and the seed).
fn profile_for_id(id: &str) -> BenchmarkProfile {
    match profiles::find(id) {
        Some(found) if found.id == id => found.clone(),
        _ => {
            let mut base = profiles::all()[0].clone();
            base.id = id.to_owned();
            base
        }
    }
}

/// The executor's own profile type. `image_executor` carries a W7-01 stand-in until wiring, which makes it
/// `models::BenchmarkProfile`; this conversion is the only place that knows about the stand-in.
fn executor_profile(profile: &BenchmarkProfile) -> ExecutorProfile {
    ExecutorProfile {
        id: profile.id.clone(),
        loading_mode: profile.loading_mode,
        workers: profile.workers,
        next_window: profile.next_window,
        previous_window: profile.previous_window,
        full_folder: profile.full_folder,
        memory_reserve_bytes: profile.memory_reserve_bytes,
        disk_cache: profile.disk_cache,
        detailed_logging: profile.detailed_logging,
    }
}

fn registry(id: &str) -> BenchmarkProfile {
    profiles::find(id)
        .unwrap_or_else(|| panic!("registry profile '{id}' is missing"))
        .clone()
}

fn workload_of(name: &str) -> BenchmarkWorkload {
    BenchmarkWorkload::from_name(name).unwrap_or_else(|| panic!("unknown workload '{name}'"))
}

/// A monotonic clock the test moves by hand (C# `ManualTimeProvider`). Frequency: nanoseconds.
#[derive(Debug, Default)]
struct ManualTime {
    ticks: AtomicI64,
}

impl ManualTime {
    fn advance_ms(&self, milliseconds: i64) {
        self.ticks.fetch_add(milliseconds * 1_000_000, Ordering::SeqCst);
    }
}

impl TimeSource for ManualTime {
    fn timestamp(&self) -> i64 {
        self.ticks.load(Ordering::SeqCst)
    }

    fn timestamp_frequency(&self) -> i64 {
        1_000_000_000
    }
}

fn run_options<'a>(
    time: &'a dyn TimeSource,
    clock: &'a dyn Clock,
    log: &'a dyn Log,
    cancel: &'a CancelToken,
) -> RunPreparedOptions<'a> {
    RunPreparedOptions {
        progress: None,
        time,
        clock,
        log,
        cancel,
        run_id: None,
        machine: None,
    }
}

/// Test bin (C# `RecordingRecycleBin`): records every path it is asked to send and removes that file, like a real send.
/// Never the user's Recycle Bin. `try_restore` always answers "not restored".
#[derive(Debug, Default)]
struct RecordingBin {
    sent: Mutex<Vec<PathBuf>>,
}

impl RecordingBin {
    fn sent_count(&self) -> usize {
        self.sent.lock().expect("bin lock").len()
    }
}

impl RecycleBin for RecordingBin {
    fn send_to_recycle_bin(&self, path: &Path) -> io::Result<()> {
        self.sent.lock().expect("bin lock").push(path.to_path_buf());
        std::fs::remove_file(path)
    }

    fn try_restore(
        &self,
        _original_path: &Path,
        _expected_size: u64,
        _expected_last_write_utc: DateTimeUtc,
    ) -> io::Result<bool> {
        Ok(false)
    }
}

/// An executor over `files` with the fake decoder, a fixed "headroom available" answer, and a scratch parent inside `root`.
/// Disposed when dropped (the executor has no `Drop` of its own).
struct TestExecutor {
    executor: Arc<BenchmarkImageExecutor>,
}

impl Drop for TestExecutor {
    fn drop(&mut self) {
        self.executor.dispose();
    }
}

fn test_executor(root: &TempRoot, profile: &BenchmarkProfile, files: &[String]) -> TestExecutor {
    let decoder: Arc<dyn ImageDecoder> = FakeDecoder::new();
    let has_headroom: HasHeadroomFn = Arc::new(|_: f64| true);
    let options = BenchmarkExecutorOptions {
        has_headroom: Some(has_headroom),
        decoder: Some(decoder),
        log: None,
        scratch_parent: Some(root.join("scratch")),
        physical_memory_bytes: PHYSICAL_MEMORY_BYTES,
        ..BenchmarkExecutorOptions::default()
    };
    let executor = BenchmarkImageExecutor::new(
        &executor_profile(profile),
        files,
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
        options,
    )
    .expect("the benchmark executor is built over the fake decoder");
    TestExecutor {
        executor: Arc::new(executor),
    }
}

/// `count` small source files in `root` (the fake decoder answers any existing file with a 4 x 3 image).
fn source_files(root: &TempRoot, prefix: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            let path = root.join(format!("{prefix}-{index:03}.png"));
            std::fs::write(&path, b"not really a png").expect("write a source file");
            path.to_string_lossy().into_owned()
        })
        .collect()
}

fn measure_ok() -> PreparedMeasure {
    let step: Box<dyn FnOnce() -> Result<MeasureOutcome, BenchmarkError>> = Box::new(|| {
        Ok(MeasureOutcome {
            correct: true,
            metrics: None,
        })
    });
    step
}

fn prepared(result: Result<PreparedMeasure, BenchmarkError>) -> PreparedMeasure {
    match result {
        Ok(step) => step,
        Err(error) => panic!("prepare_iteration failed: {error:?}"),
    }
}

fn prepare_error(result: Result<PreparedMeasure, BenchmarkError>) -> BenchmarkError {
    match result {
        Ok(_) => panic!("prepare_iteration must refuse this input, it returned a measure step"),
        Err(error) => error,
    }
}

fn outcome(step: PreparedMeasure) -> MeasureOutcome {
    match step() {
        Ok(outcome) => outcome,
        Err(error) => panic!("the measure step failed: {error:?}"),
    }
}

/// The first `count` values of a generator (by value: `DotNetRandom` is not `PartialEq`).
fn first_samples(mut random: DotNetRandom, count: usize) -> Vec<i32> {
    (0..count).map(|_| random.next()).collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// BenchmarkSelectionGoldenTests
// ---------------------------------------------------------------------------------------------------------------------

/// Golden values: `new Random(seed)` with the 17/31 string hash of the profile id (recomputed on .NET Framework).
const GOLDEN_PREVIEW_BALANCED: [i32; 12] = [93, 922, 763, 917, 322, 591, 596, 88, 118, 911, 927, 640];

/// Golden file indices of `random-navigation` (`Workers = 2`, 5 files, iterations 0..5 sharing one random).
const GOLDEN_RANDOM_NAVIGATION: [[i32; 2]; 6] = [[4, 4], [3, 2], [3, 3], [0, 1], [4, 4], [2, 1]];

// C#: BenchmarkSelectionGoldenTests.CreateSeededRandom_YieldsGoldenSequence
#[test]
fn create_seeded_random_yields_golden_sequence() {
    let mut random = create_seeded_random("preview-balanced");

    let sequence: Vec<i32> = (0..12).map(|_| random.next_max(1000)).collect();

    assert_eq!(sequence, GOLDEN_PREVIEW_BALANCED);
}

// C#: BenchmarkSelectionGoldenTests.RandomWorkload_SelectsGoldenFileIndices
#[test]
fn random_workload_selects_golden_file_indices() {
    let profile = BenchmarkProfile {
        workers: 2,
        ..registry("random-navigation")
    };
    let mut random = create_seeded_random(&profile.id);

    let selected: Vec<Vec<i32>> = (0..6)
        .map(|iteration| select_parallel_indices(5, &profile, BenchmarkWorkload::Random, iteration, &mut random))
        .collect();

    let expected: Vec<Vec<i32>> = GOLDEN_RANDOM_NAVIGATION.iter().map(|pair| pair.to_vec()).collect();
    assert_eq!(selected, expected);
}

// ---------------------------------------------------------------------------------------------------------------------
// BenchmarkStatisticsEdgeTests (ImagesPerSample only)
// ---------------------------------------------------------------------------------------------------------------------

/// C# `ProfileWith(workers, workload)`: the first registry profile of that workload, `Workers` replaced.
fn profile_with(workers: i32, workload: BenchmarkWorkload) -> BenchmarkProfile {
    let base = profiles::all()
        .iter()
        .find(|profile| profile.workload == workload)
        .unwrap_or_else(|| panic!("no registry profile has workload {workload:?}"))
        .clone();
    BenchmarkProfile { workers, ..base }
}

// C#: BenchmarkStatisticsEdgeTests.ImagesPerSample_Table (the 10 InlineData rows, one assertion per row)
#[test]
fn images_per_sample_table() {
    let rows: [(BenchmarkWorkload, i32, i32, i32); 10] = [
        (BenchmarkWorkload::FirstFrame, 8, 100, 1),
        (BenchmarkWorkload::Preload, 8, 100, 1),
        (BenchmarkWorkload::WarmNext, 8, 100, 1),
        (BenchmarkWorkload::FileAction, 8, 100, 1),
        (BenchmarkWorkload::Sequential, 8, 100, 8),
        (BenchmarkWorkload::Sequential, 8, 3, 3),
        (BenchmarkWorkload::Random, 12, 1, 1),
        (BenchmarkWorkload::Random, 12, 0, 1),
        (BenchmarkWorkload::Sequential, 0, 10, 1),
        (BenchmarkWorkload::Correctness, 4, i32::MAX, 4),
    ];
    for (workload, workers, files, expected) in rows {
        let profile = profile_with(workers, workload);
        assert_eq!(
            images_per_sample(&profile, workload, files),
            expected,
            "ImagesPerSample({workload:?}, workers {workers}, files {files})"
        );
    }
}

// C#: BenchmarkStatisticsEdgeTests.ImagesPerSample_AgreesWithSelectedIndices
#[test]
fn images_per_sample_agrees_with_selected_indices() {
    let parallel = [
        BenchmarkWorkload::Sequential,
        BenchmarkWorkload::Random,
        BenchmarkWorkload::Correctness,
    ];
    for profile in profiles::all()
        .iter()
        .filter(|profile| parallel.contains(&profile.workload))
    {
        for file_count in [1, 2, 5, 64] {
            let mut random = DotNetRandom::new(1);
            let picked = select_parallel_indices(file_count, profile, profile.workload, 0, &mut random);
            assert_eq!(
                picked.len(),
                usize::try_from(images_per_sample(profile, profile.workload, file_count)).expect("a count"),
                "profile {} with {file_count} files",
                profile.id
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// BenchmarkTrustTests (RunProfile stamping only)
// ---------------------------------------------------------------------------------------------------------------------

// C#: BenchmarkTrustTests.RunProfileAsync_StampsImagesPerSample
#[test]
fn run_profile_with_stamps_images_per_sample() {
    let root = TempRoot::new("bench-trust-stamp");
    let profile = BenchmarkProfile {
        iterations: 1,
        warmup_count: 0,
        ..registry("fast-sequential")
    };
    let time = ManualTime::default();
    let clock = FakeClock::new();
    let log = NullLog;
    let cancel = CancelToken::none();
    let options = run_options(&time, &clock, &log, &cancel);
    let mut prepare =
        |_: BenchmarkWorkload, _: i32, _: &CancelToken| -> Result<PreparedMeasure, BenchmarkError> { Ok(measure_ok()) };

    let report = run_profile_with(&root.path().to_string_lossy(), &profile, &mut prepare, &options, 3)
        .expect("the stub profile runs");

    assert_eq!(report.phases.len(), 1);
    // 8 workers capped by 3 files
    assert_eq!(report.phases[0].images_per_sample, Some(3));
    assert_eq!(images_per_sample(&profile, BenchmarkWorkload::FirstFrame, 3), 1);
}

// ---------------------------------------------------------------------------------------------------------------------
// BenchmarkEngineTimingTests (setup time excluded)
// ---------------------------------------------------------------------------------------------------------------------

// C#: BenchmarkEngineTimingTests.RunProfileAsync_SamplesExcludeSetupTime
#[test]
fn run_profile_with_samples_exclude_setup_time() {
    let root = TempRoot::new("bench-profile-timing");
    let profile = BenchmarkProfile {
        iterations: 2,
        warmup_count: 1,
        ..registry("action-move")
    };
    let time = Arc::new(ManualTime::default());
    let clock = FakeClock::new();
    let log = NullLog;
    let cancel = CancelToken::none();
    let options = run_options(time.as_ref(), &clock, &log, &cancel);
    let prepared_count = Arc::new(AtomicI64::new(0));
    let expected_workload = profile.workload;

    let mut prepare = {
        let time = Arc::clone(&time);
        let prepared_count = Arc::clone(&prepared_count);
        move |workload: BenchmarkWorkload, _: i32, _: &CancelToken| -> Result<PreparedMeasure, BenchmarkError> {
            assert_eq!(
                workload, expected_workload,
                "the prepare step receives the profile's workload"
            );
            prepared_count.fetch_add(1, Ordering::SeqCst);
            // scratch copy / cold-cache eviction: untimed
            time.advance_ms(700);
            let time = Arc::clone(&time);
            let step: Box<dyn FnOnce() -> Result<MeasureOutcome, BenchmarkError>> = Box::new(move || {
                time.advance_ms(20);
                Ok(MeasureOutcome {
                    correct: true,
                    metrics: None,
                })
            });
            Ok(step)
        }
    };

    let report = run_profile_with(&root.path().to_string_lossy(), &profile, &mut prepare, &options, 3)
        .expect("the timing profile runs");

    assert_eq!(report.phases.len(), 1);
    assert_eq!(report.phases[0].samples(), &[20.0, 20.0]);
    // warm-up + 2 timed iterations
    assert_eq!(prepared_count.load(Ordering::SeqCst), 3);
}

// ---------------------------------------------------------------------------------------------------------------------
// BenchmarkWorkloadRunnerTests (file actions use a real executor, the fake decoder and a recording bin)
// ---------------------------------------------------------------------------------------------------------------------

/// Runs the whole file-action iteration for `profile_id` once: prepare, then measure. Returns the outcome.
fn run_file_action_once(
    test: &TestExecutor,
    files: &Arc<[String]>,
    profile: &BenchmarkProfile,
    iteration: i32,
    random: &mut DotNetRandom,
    bin: &Arc<RecordingBin>,
) -> MeasureOutcome {
    let bin_dyn: Arc<dyn RecycleBin> = bin.clone();
    let ct = CancelToken::none();
    let step = prepared(prepare_iteration(
        &test.executor,
        files,
        profile,
        BenchmarkWorkload::FileAction,
        iteration,
        random,
        &bin_dyn,
        &ct,
    ));
    outcome(step)
}

// C#: BenchmarkWorkloadRunnerTests.FileActionProfilePerformsItsOwnOperation (action-move)
#[test]
fn file_action_profile_performs_its_own_operation_action_move() {
    file_action_profile_performs_its_own_operation("action-move");
}

// C#: BenchmarkWorkloadRunnerTests.FileActionProfilePerformsItsOwnOperation (action-delete)
#[test]
fn file_action_profile_performs_its_own_operation_action_delete() {
    file_action_profile_performs_its_own_operation("action-delete");
}

// C#: BenchmarkWorkloadRunnerTests.FileActionProfilePerformsItsOwnOperation (action-copy)
#[test]
fn file_action_profile_performs_its_own_operation_action_copy() {
    file_action_profile_performs_its_own_operation("action-copy");
}

fn file_action_profile_performs_its_own_operation(profile_id: &str) {
    let root = TempRoot::new(&format!("bench-action-{profile_id}"));
    let profile = registry(profile_id);
    let files = source_files(&root, &format!("{profile_id}-source"), 1);
    let test = test_executor(&root, &profile, &files);
    let files: Arc<[String]> = Arc::from(files);
    let bin = Arc::new(RecordingBin::default());
    let mut random = create_seeded_random(&profile.id);

    let result = run_file_action_once(&test, &files, &profile, 0, &mut random, &bin);

    // The action's own correctness check fails if the wrong operation ran, so `correct` is the mapping's regression signal.
    assert!(
        result.correct,
        "{profile_id}: the operation left the source in the wrong state"
    );
    assert!(result.metrics.is_some(), "{profile_id}: metrics are reported");
    // The action profile operates on a scratch copy, never on the catalog file.
    assert!(
        Path::new(&files[0]).exists(),
        "{profile_id}: the catalog file still exists"
    );
    // Only the delete profile goes through the injected bin, exactly once per iteration.
    let expected_bin_calls = usize::from(profile_id == "action-delete");
    assert_eq!(bin.sent_count(), expected_bin_calls, "{profile_id}: bin calls");
}

// C#: BenchmarkWorkloadRunnerTests.PrepareIterationAsync_FileAction_ActionRunsOnlyInMeasureStep
#[test]
fn prepare_iteration_file_action_action_runs_only_in_measure_step() {
    let root = TempRoot::new("bench-action-prepare");
    let profile = registry("action-delete");
    let files = source_files(&root, "prepare-source", 1);
    let test = test_executor(&root, &profile, &files);
    let files: Arc<[String]> = Arc::from(files);
    let bin = Arc::new(RecordingBin::default());
    let bin_dyn: Arc<dyn RecycleBin> = bin.clone();
    let ct = CancelToken::none();

    let step = prepared(prepare_iteration(
        &test.executor,
        &files,
        &profile,
        BenchmarkWorkload::FileAction,
        0,
        &mut create_seeded_random(&profile.id),
        &bin_dyn,
        &ct,
    ));
    // setup only: the timed action has not run
    assert_eq!(bin.sent_count(), 0);

    let result = outcome(step);
    assert!(result.correct);
    assert_eq!(bin.sent_count(), 1);
}

// C#: BenchmarkWorkloadRunnerTests.InterleavedActionProfileCyclesThroughAllThreeOperations
#[test]
fn interleaved_action_profile_cycles_through_all_three_operations() {
    let root = TempRoot::new("bench-action-interleaved");
    let profile = registry("action-interleaved");
    let files = source_files(&root, "interleaved-source", 1);
    let test = test_executor(&root, &profile, &files);
    let files: Arc<[String]> = Arc::from(files);
    let bin = Arc::new(RecordingBin::default());
    let mut random = create_seeded_random(&profile.id);

    for iteration in 0..3 {
        let result = run_file_action_once(&test, &files, &profile, iteration, &mut random, &bin);
        assert!(
            result.correct,
            "iteration {iteration} (op {}) did not match its expected source state",
            iteration % 3
        );
    }

    // Iterations 0..2 map to move, delete, copy: exactly one of them is a delete.
    assert_eq!(bin.sent_count(), 1);
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden-driven tests (tests/golden/*.json)
// ---------------------------------------------------------------------------------------------------------------------

// Golden: bench-random (oracle `bench-random`, 35 cases: 12 seeds, 23 profile-id texts, 24 draws each).
#[test]
fn golden_bench_random_matches_dotnet_random() {
    let golden = golden::load("bench-random");
    assert_eq!(golden.cases.len(), 35);
    for case in &golden.cases {
        let name = str_of(case, "name");
        let draws = case["draws"].as_array().expect("draws");
        let values = case["values"].as_array().expect("values");
        assert_eq!(draws.len(), values.len(), "{name}: one value per draw");

        let mut random = if let Some(seed) = case.get("seed") {
            DotNetRandom::new(seed.as_i64().expect("seed") as i32)
        } else {
            let text = str_of(case, "text");
            let hash = i64_of(case, "hash") as i32;
            // The seed of a text case is its StableHash, so a generator built from the hash must draw the same.
            assert_eq!(
                first_samples(DotNetRandom::new(hash), 12),
                first_samples(create_seeded_random(text), 12),
                "{name}: seed is the hash"
            );
            create_seeded_random(text)
        };

        for (index, (draw, expected)) in draws.iter().zip(values).enumerate() {
            let context = format!("{name}, draw {index}");
            match draw.get("max") {
                // Next()
                None => assert_eq!(random.next(), expected.as_i64().expect(&context) as i32, "{context}"),
                Some(max) => {
                    let max = max.as_i64().expect("max") as i32;
                    if let Some(throws) = expected.get("throws") {
                        assert_eq!(throws.as_str(), Some("ArgumentOutOfRangeException"), "{context}");
                        let refused = catch_unwind(AssertUnwindSafe(|| random.next_max(max)));
                        assert!(refused.is_err(), "{context}: Next({max}) must panic like C#");
                    } else {
                        assert_eq!(
                            random.next_max(max),
                            expected.as_i64().expect(&context) as i32,
                            "{context}"
                        );
                    }
                }
            }
        }
    }
}

// Golden: bench-select-indices (oracle `bench-select-indices`, 141 cases).
#[test]
fn golden_bench_select_indices_matches_csharp() {
    let golden = golden::load("bench-select-indices");
    assert_eq!(golden.cases.len(), 141);
    for case in &golden.cases {
        let name = str_of(case, "name");
        let profile_id = str_of(case, "profile_id");
        let mut profile = profile_for_id(profile_id);
        profile.workers = i64_of(case, "workers") as i32;
        profile.workload = workload_of(str_of(case, "workload"));
        let file_count = i64_of(case, "file_count") as i32;
        let seed_text = case.get("seed_text").and_then(Value::as_str).unwrap_or(profile_id);
        let mut random = create_seeded_random(seed_text);

        let iterations = case["iterations"].as_array().expect("iterations");
        let picked = case["picked"].as_array().expect("picked");
        assert_eq!(iterations.len(), picked.len(), "{name}");
        for (index, iteration) in iterations.iter().enumerate() {
            let iteration = iteration.as_i64().expect("iteration") as i32;
            let expected: Vec<i32> = picked[index]
                .as_array()
                .expect("picked row")
                .iter()
                .map(|value| value.as_i64().expect("index") as i32)
                .collect();
            let actual = select_parallel_indices(file_count, &profile, profile.workload, iteration, &mut random);
            assert_eq!(actual, expected, "{name}: iteration {iteration}");
        }
        assert_eq!(
            images_per_sample(&profile, profile.workload, file_count),
            i64_of(case, "images_per_sample") as i32,
            "{name}: images_per_sample"
        );
    }
}

// Golden: bench-images-per-sample (oracle `bench-images-per-sample`, 490 rows = 7 workloads x 7 workers x 10 file counts).
#[test]
fn golden_bench_images_per_sample_matches_csharp() {
    let golden = golden::load("bench-images-per-sample");
    assert_eq!(golden.cases.len(), 490);
    let base = profiles::all()[0].clone();
    for case in &golden.cases {
        let profile = BenchmarkProfile {
            workers: i64_of(case, "workers") as i32,
            ..base.clone()
        };
        let workload = workload_of(str_of(case, "workload"));
        let file_count = i64_of(case, "file_count") as i32;
        assert_eq!(
            images_per_sample(&profile, workload, file_count),
            i64_of(case, "images_per_sample") as i32,
            "{} workers {} files {}",
            str_of(case, "workload"),
            profile.workers,
            file_count
        );
    }
}

// Golden: bench-prepare-edge (oracle `bench-prepare-edge`, 24 cases: refusals and empty-folder errors).
#[test]
fn golden_bench_prepare_edge_refuses_or_fails_at_prepare() {
    let golden = golden::load("bench-prepare-edge");
    assert_eq!(golden.cases.len(), 24);
    let root = TempRoot::new("bench-prepare-edge");
    // One executor for every case (no files: nothing is decoded or evicted). Its profile is only a construction input.
    let test = test_executor(&root, &registry("fast-sequential"), &[]);
    let files: Arc<[String]> = Arc::from(Vec::<String>::new());
    let bin = Arc::new(RecordingBin::default());
    let bin_dyn: Arc<dyn RecycleBin> = bin.clone();
    let ct = CancelToken::none();
    let mut random = DotNetRandom::new(0);

    for case in &golden.cases {
        let name = str_of(case, "name");
        let profile_id = str_of(case, "profile_id");
        let mut profile = profile_for_id(profile_id);
        profile.workload = workload_of(str_of(case, "profile_workload"));
        let workload = workload_of(str_of(case, "workload"));
        let iteration = i64_of(case, "iteration") as i32;

        let result = prepare_iteration(
            &test.executor,
            &files,
            &profile,
            workload,
            iteration,
            &mut random,
            &bin_dyn,
            &ct,
        );
        let error = prepare_error(result);
        if case.get("problem").is_some() {
            assert_eq!(str_of(case, "problem"), "NotImplemented", "{name}");
            match error {
                BenchmarkError::Profile(refusal) => {
                    assert_eq!(refusal.problem, BenchmarkProfileProblem::NotImplemented, "{name}");
                    assert_eq!(refusal.profile_id, profile_id, "{name}");
                    assert_eq!(refusal.message, str_of(case, "message"), "{name}");
                }
                other => panic!("{name}: expected the NotImplemented refusal, got {other:?}"),
            }
        } else {
            // DIVERGENCE (contract): the empty list fails at prepare in every workload; the stage is not asserted.
            match error {
                BenchmarkError::Operation(inner) => assert!(
                    matches!(
                        inner.downcast_ref::<WorkloadError>(),
                        Some(WorkloadError::InvalidArgument(_))
                    ),
                    "{name}: expected WorkloadError::InvalidArgument"
                ),
                other => panic!("{name}: expected an operation error, got {other:?}"),
            }
        }
    }
    assert_eq!(
        bin.sent_count(),
        0,
        "no file action is ever sent to the bin in the edge cases"
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// New tests (no C# twin)
// ---------------------------------------------------------------------------------------------------------------------

// New: the not-implemented refusal text is byte-exact for both refused profiles (AGENTS.md: CLI, logs and report use it).
#[test]
fn not_implemented_profiles_refuse_with_the_exact_message() {
    let root = TempRoot::new("bench-refusal-text");
    let test = test_executor(&root, &registry("fast-sequential"), &[]);
    let files: Arc<[String]> = Arc::from(Vec::<String>::new());
    let bin: Arc<dyn RecycleBin> = Arc::new(RecordingBin::default());
    let ct = CancelToken::none();
    let expectations = [
        (
            "explorer-reindex",
            "Benchmark profile 'explorer-reindex' does not implement a real Explorer Reindex check yet; a decode-only stand-in would misreport results, so it refuses to run.",
        ),
        (
            "cache-recovery",
            "Benchmark profile 'cache-recovery' does not implement a real Cache Recovery check yet; a decode-only stand-in would misreport results, so it refuses to run.",
        ),
    ];
    for (id, message) in expectations {
        let profile = registry(id);
        let error = prepare_error(prepare_iteration(
            &test.executor,
            &files,
            &profile,
            BenchmarkWorkload::Correctness,
            0,
            &mut DotNetRandom::new(0),
            &bin,
            &ct,
        ));
        match error {
            BenchmarkError::Profile(refusal) => {
                assert_eq!(refusal.problem, BenchmarkProfileProblem::NotImplemented);
                assert_eq!(refusal.profile_id, id);
                assert_eq!(refusal.message, message);
            }
            other => panic!("{id}: expected the refusal, got {other:?}"),
        }
    }
}

// New: every workload rejects an empty file list at prepare with WorkloadError::InvalidArgument (step 6 of the contract).
#[test]
fn empty_file_list_is_invalid_argument_at_prepare_for_every_workload() {
    let root = TempRoot::new("bench-empty-list");
    let profile = registry("fast-sequential");
    let test = test_executor(&root, &profile, &[]);
    let files: Arc<[String]> = Arc::from(Vec::<String>::new());
    let bin = Arc::new(RecordingBin::default());
    let bin_dyn: Arc<dyn RecycleBin> = bin.clone();
    let ct = CancelToken::none();
    for workload in BenchmarkWorkload::ALL {
        let error = prepare_error(prepare_iteration(
            &test.executor,
            &files,
            &profile,
            workload,
            0,
            &mut DotNetRandom::new(0),
            &bin_dyn,
            &ct,
        ));
        match error {
            BenchmarkError::Operation(inner) => assert!(
                matches!(
                    inner.downcast_ref::<WorkloadError>(),
                    Some(WorkloadError::InvalidArgument(_))
                ),
                "{workload:?}: expected InvalidArgument"
            ),
            other => panic!("{workload:?}: expected an operation error, got {other:?}"),
        }
    }
    assert_eq!(bin.sent_count(), 0);
}

// New: a token already canceled before a file action starts gives Canceled and creates no scratch file (checked before ActionScratch::new).
#[test]
fn file_action_with_canceled_token_is_canceled_before_any_file() {
    let root = TempRoot::new("bench-action-canceled");
    let profile = registry("action-move");
    let files = source_files(&root, "canceled-source", 1);
    let test = test_executor(&root, &profile, &files);
    let files: Arc<[String]> = Arc::from(files);
    let bin: Arc<dyn RecycleBin> = Arc::new(RecordingBin::default());
    let ct = CancelToken::new();
    ct.cancel();

    let error = prepare_error(prepare_iteration(
        &test.executor,
        &files,
        &profile,
        BenchmarkWorkload::FileAction,
        0,
        &mut DotNetRandom::new(0),
        &bin,
        &ct,
    ));

    assert!(matches!(error, BenchmarkError::Canceled), "got {error:?}");
}

// New: a token canceled before the measure step of a parallel workload gives Canceled (every decode_async refuses it).
#[test]
fn parallel_step_with_token_canceled_before_the_step_is_canceled() {
    let root = TempRoot::new("bench-step-canceled");
    let profile = registry("fast-sequential");
    let files = source_files(&root, "step-canceled", 2);
    let test = test_executor(&root, &profile, &files);
    let files: Arc<[String]> = Arc::from(files);
    let bin: Arc<dyn RecycleBin> = Arc::new(RecordingBin::default());
    let ct = CancelToken::new();

    let step = prepared(prepare_iteration(
        &test.executor,
        &files,
        &profile,
        BenchmarkWorkload::Sequential,
        0,
        &mut DotNetRandom::new(0),
        &bin,
        &ct,
    ));
    ct.cancel();

    match step() {
        Err(BenchmarkError::Canceled) => {}
        other => panic!("expected Canceled from the measure step, got {other:?}"),
    }
}

// New: next_max(0) returns 0 but still consumes one sample (contract: DotNetRandom semantics).
#[test]
fn next_max_zero_returns_zero_and_consumes_one_sample() {
    let seed = 758_877_054;
    let mut consumed = DotNetRandom::new(seed);
    let mut reference = DotNetRandom::new(seed);

    assert_eq!(consumed.next_max(0), 0);
    reference.next();
    assert_eq!(consumed.next_max(1000), reference.next_max(1000));
}

// New: the registry lookup used by BenchmarkPhaseResult.EffectiveImagesPerSample (case-insensitive id, unknown id = None).
#[test]
fn registry_images_per_sample_uses_the_registry_profile() {
    let profile = registry("fast-balanced");
    assert_eq!(
        registry_images_per_sample("FAST-BALANCED", BenchmarkWorkload::Sequential),
        Some(images_per_sample(&profile, BenchmarkWorkload::Sequential, i32::MAX))
    );
    assert_eq!(
        registry_images_per_sample("no-such-profile", BenchmarkWorkload::Sequential),
        None
    );
}

// New: the action-id to operation mapping (exact, case-sensitive ids; interleaved uses iteration % 3, negative remainders included).
#[test]
fn file_action_op_maps_ids_and_interleaving() {
    let cases: [(&str, i32, FileActionOp); 16] = [
        ("action-move", 0, FileActionOp::Move),
        ("action-move", 7, FileActionOp::Move),
        ("action-delete", 0, FileActionOp::Delete),
        ("action-delete", 5, FileActionOp::Delete),
        ("action-copy", 0, FileActionOp::Copy),
        ("action-copy", 4, FileActionOp::Copy),
        ("action-interleaved", 0, FileActionOp::Move),
        ("action-interleaved", 1, FileActionOp::Delete),
        ("action-interleaved", 2, FileActionOp::Copy),
        ("action-interleaved", 3, FileActionOp::Move),
        ("action-interleaved", -1, FileActionOp::Copy),
        ("action-interleaved", -3, FileActionOp::Move),
        ("action-interleaved", i32::MAX, FileActionOp::Delete),
        ("ACTION-DELETE", 0, FileActionOp::Move),
        ("action-delete ", 0, FileActionOp::Move),
        ("fast-sequential", 1, FileActionOp::Move),
    ];
    for (id, iteration, expected) in cases {
        assert_eq!(file_action_op(id, iteration), expected, "{id:?} iteration {iteration}");
    }
}
