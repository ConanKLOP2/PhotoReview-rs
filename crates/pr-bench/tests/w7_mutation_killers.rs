//! Focused W7 regression and mutation guards. All I/O stays in TempRoot.

#[path = "../../pr-imaging/tests/support/preview_service.rs"]
mod preview_service;

use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

use pr_bench::cli::tool_path_guard::{ensure_output_directory, ensure_output_file, is_same_or_under, overlaps};
use pr_bench::image_executor::{
    BenchmarkExecutorOptions, BenchmarkImageExecutor, BenchmarkProfileScope, WindowedPreloadTarget,
};
use pr_bench::models::BenchmarkWorkload;
use pr_bench::profiles;
use pr_bench::slow_link_fs::{SharedBandwidthLimiter, TimeSource};
use pr_bench::workload_runner::{
    DotNetRandom, FileActionOp, file_action_op, images_per_sample, select_parallel_indices,
};
use pr_core::io::physical::PhysicalFileSystem;
use pr_core::sync::CancelToken;
use pr_imaging::preload::target::PreloadTarget;
use pr_test_support::{RecordingLog, TempRoot};
use preview_service::{FakeDecoder, FakePayloadCodec, PixelsCodec};

fn executor(root: &TempRoot, files: &[String], log: Arc<RecordingLog>) -> BenchmarkImageExecutor {
    executor_with_profile(root, files, log, profiles::find("no-preload-baseline").unwrap())
}

fn executor_with_profile(
    root: &TempRoot,
    files: &[String],
    log: Arc<RecordingLog>,
    profile: &pr_bench::models::BenchmarkProfile,
) -> BenchmarkImageExecutor {
    BenchmarkImageExecutor::new(
        profile,
        files,
        Arc::new(PixelsCodec),
        Arc::new(FakePayloadCodec),
        BenchmarkExecutorOptions {
            decoder: Some(FakeDecoder::new()),
            log: Some(log),
            scratch_parent: Some(root.join("scratch")),
            has_headroom: Some(Arc::new(|_| true)),
            physical_memory_bytes: 32 * 1024 * 1024 * 1024,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn teardown_scratch_leaf_file_is_preserved_without_error() {
    // C#: BenchmarkImageExecutor.DeleteScratchDirectoryAfterPruneAsync: Directory.Exists guard.
    // Regression: image_executor.rs:753 exists() incorrectly attempts directory removal on a file.
    let root = TempRoot::new("w7-scratch-file");
    let log = Arc::new(RecordingLog::new());
    let executor = executor(&root, &[], log.clone());
    let scratch = executor.disk_cache_directory();
    fs::create_dir_all(scratch.parent().unwrap()).unwrap();
    fs::write(scratch, b"keep leaf file").unwrap();
    executor.dispose();
    assert!(
        executor
            .teardown_background_task()
            .wait_timeout(Duration::from_secs(30))
    );
    assert_eq!(fs::read(scratch).unwrap(), b"keep leaf file");
    assert_eq!(log.entries(), []);
}

#[derive(Default)]
struct FakeTime {
    ticks: AtomicI64,
    sleeps: Mutex<Vec<Duration>>,
}

impl TimeSource for FakeTime {
    fn now_ticks(&self) -> i64 {
        self.ticks.load(Ordering::SeqCst)
    }
    fn sleep(&self, duration: Duration) {
        let mut sleeps = self.sleeps.lock().unwrap();
        assert!(sleeps.len() < 20, "limiter must make progress with a fake clock");
        sleeps.push(duration);
        self.ticks
            .fetch_add(i64::try_from(duration.as_nanos() / 100).unwrap(), Ordering::SeqCst);
    }
}

#[test]
fn limiter_truncates_fractional_milliseconds_and_preserves_leftover_tokens() {
    // NEW: slow_link_fs.rs:152 -= -> += or /=; 155 - -> + or /; 156 / -> % or *;
    // 159 * -> + or /. C#: SharedBandwidthLimiter.ConsumeChunk / Thread.Sleep(TimeSpan).
    let time = Arc::new(FakeTime::default());
    let limiter = SharedBandwidthLimiter::with_time_source(2000, time.clone()).unwrap();
    limiter.consume(3); // 1.5 ms truncates to 1 ms, then the 0.5 ms remainder floors to 1 ms.
    assert_eq!(*time.sleeps.lock().unwrap(), vec![Duration::from_millis(1); 2]);
    limiter.consume(1); // The second sleep refilled four bytes; one remains after consuming three.
    assert_eq!(time.sleeps.lock().unwrap().len(), 2);
    limiter.consume(4);
    assert_eq!(
        *time.sleeps.lock().unwrap(),
        vec![
            Duration::from_millis(1),
            Duration::from_millis(1),
            Duration::from_millis(2)
        ]
    );
}

#[test]
fn workload_counts_and_order_match_workers_and_wrapping_iteration() {
    // NEW: workload_runner.rs:216 constant return; 246 constant vectors / <= -> >.
    // C#: BenchmarkWorkloadRunner.ImagesPerSample / SelectParallelIndices.
    let mut profile = profiles::find("fast-sequential").unwrap().clone();
    profile.workers = 3;
    for workload in [
        BenchmarkWorkload::Sequential,
        BenchmarkWorkload::Random,
        BenchmarkWorkload::Correctness,
    ] {
        for (files, expected) in [(-1, 1), (0, 1), (1, 1), (2, 2), (7, 3)] {
            assert_eq!(images_per_sample(&profile, workload, files), expected);
        }
    }
    for workload in [
        BenchmarkWorkload::FirstFrame,
        BenchmarkWorkload::Preload,
        BenchmarkWorkload::WarmNext,
        BenchmarkWorkload::FileAction,
    ] {
        assert_eq!(images_per_sample(&profile, workload, 7), 1);
    }
    let mut random = DotNetRandom::new(1);
    assert_eq!(
        select_parallel_indices(5, &profile, BenchmarkWorkload::Sequential, 4, &mut random),
        [4, 0, 1]
    );
    assert_eq!(
        select_parallel_indices(5, &profile, BenchmarkWorkload::Sequential, i32::MAX, &mut random),
        [2, -3, -2]
    );
    assert!(select_parallel_indices(0, &profile, BenchmarkWorkload::Sequential, 0, &mut random).is_empty());
    profile.workers = 0;
    assert_eq!(
        select_parallel_indices(5, &profile, BenchmarkWorkload::Sequential, 2, &mut random),
        [2]
    );
}

#[test]
fn interleaved_action_mapping_uses_remainder_and_exact_ids() {
    // NEW: workload_runner.rs:303-307 match-arm deletions; 305 % -> / or +.
    // C#: BenchmarkWorkloadRunner file-action switch; no real file actions are invoked.
    for (id, iteration, expected) in [
        ("action-delete", 0, FileActionOp::Delete),
        ("action-copy", 0, FileActionOp::Copy),
        ("action-interleaved", 0, FileActionOp::Move),
        ("action-interleaved", 1, FileActionOp::Delete),
        ("action-interleaved", 2, FileActionOp::Copy),
        ("action-interleaved", 3, FileActionOp::Move),
        ("action-interleaved", 4, FileActionOp::Delete),
        ("action-interleaved", -1, FileActionOp::Copy),
        ("ACTION-DELETE", 1, FileActionOp::Move),
        ("other", 1, FileActionOp::Move),
    ] {
        assert_eq!(file_action_op(id, iteration), expected);
    }
}

#[test]
fn guard_refuses_equal_inside_and_ancestor_but_accepts_prefix_sibling() {
    // NEW: tool_path_guard.rs:72/83 constant bool; 83 || -> &&; 93/105 body -> Ok(()).
    // C#: ToolPathGuard.IsSameOrUnder / Overlaps / EnsureOutputDirectory / EnsureOutputFile.
    let root = TempRoot::new("w7-guard");
    let photos = root.join("photos");
    fs::create_dir_all(photos.join("child")).unwrap();
    let fs = PhysicalFileSystem;
    let photo = photos.to_str().unwrap();
    let child = photos.join("child");
    let child = child.to_str().unwrap();
    let parent = root.path().to_str().unwrap();
    let sibling = root.join("photos-reports");
    let sibling = sibling.to_str().unwrap();
    assert!(is_same_or_under(&fs, photo, photo).unwrap());
    assert!(is_same_or_under(&fs, child, photo).unwrap());
    assert!(!is_same_or_under(&fs, parent, photo).unwrap());
    assert!(!is_same_or_under(&fs, sibling, photo).unwrap());
    assert!(overlaps(&fs, parent, photo).unwrap());
    assert!(overlaps(&fs, photo, parent).unwrap());
    assert!(!overlaps(&fs, sibling, photo).unwrap());
    for output in [photo, child, parent] {
        assert!(ensure_output_directory(&fs, output, photo).is_err());
    }
    assert!(ensure_output_directory(&fs, sibling, photo).is_ok());
    assert!(ensure_output_file(&fs, photos.join("report.json").to_str().unwrap(), photo).is_err());
    assert!(ensure_output_file(&fs, root.join("report.json").to_str().unwrap(), photo).is_ok());
}

#[test]
fn executor_cached_viewer_decode_is_not_a_preload_hit() {
    // NEW: image_executor.rs:608 && -> ||. C#: BenchmarkImageExecutor.DecodeAsync:
    // a RAM cache hit alone must not increment PreloadHits or consume a nonexistent preload key.
    let root = TempRoot::new("w7-counter");
    let path = root.join("fake.png");
    fs::write(&path, b"fake decoder input").unwrap();
    let path = path.to_str().unwrap().to_owned();
    let executor = executor(&root, std::slice::from_ref(&path), Arc::new(RecordingLog::new()));
    let first = executor.decode(&path, &CancelToken::none());
    let second = executor.decode(&path, &CancelToken::none());
    let metrics = executor.metrics();
    executor.dispose();
    assert!(
        executor
            .teardown_background_task()
            .wait_timeout(Duration::from_secs(30))
    );
    assert!(first.is_ok());
    assert!(second.is_ok());
    assert_eq!(metrics.preload_hits, 0);
}

#[test]
fn executor_window_preserves_both_inclusive_boundaries_and_wrapping_center() {
    // NEW: image_executor.rs:341 > -> ==/< />=, || -> &&; 332 constant bool.
    // C#: WindowedPreloadTarget.OutsideWindow uses unchecked int subtraction and negation.
    let root = TempRoot::new("w7-window");
    let files: Vec<_> = (0..7)
        .map(|i| root.join(format!("{i}.png")).to_str().unwrap().to_owned())
        .collect();
    let executor = executor(&root, &files, Arc::new(RecordingLog::new()));
    let mut profile = profiles::find("fast-balanced").unwrap().clone();
    profile.next_window = 1;
    profile.previous_window = 2;
    let inner: Arc<dyn PreloadTarget> = executor.preview_service_for_tests().clone();
    let target = WindowedPreloadTarget::new(inner, &files, &profile).unwrap();
    target.set_center(3);
    let actual: Vec<_> = files
        .iter()
        .map(|p| target.try_get_cached_preview_for_path(p))
        .collect();
    assert_eq!(actual, [true, false, false, false, false, true, true]);
    assert!(!target.try_get_cached_preview_for_path(root.join("unknown.png").to_str().unwrap()));
    target.set_center(i32::MIN);
    assert!(!target.try_get_cached_preview_for_path(&files[0]));
    executor.dispose();
    assert!(
        executor
            .teardown_background_task()
            .wait_timeout(Duration::from_secs(30))
    );
}

#[test]
fn logging_samples_counter_after_apply_and_keeps_external_same_value_write() {
    // NEW: image_executor.rs:240 != -> ==; apply/restore body deletion.
    // C#: BenchmarkProfileScope.ApplyLogging, EnabledWriteCount includes even same-value writes.
    use std::cell::Cell;
    let mut profile = profiles::find("fast-balanced").unwrap().clone();
    profile.detailed_logging = true;
    let enabled = Cell::new(false);
    let count = Cell::new(0);
    let set = |v| {
        enabled.set(v);
        count.set(count.get() + 1);
    };
    {
        let mut scope =
            BenchmarkProfileScope::apply_logging(&profile, || enabled.get(), set, Some(Box::new(|| count.get())));
        assert!(enabled.get());
        assert_eq!(count.get(), 1);
        scope.dispose();
        scope.dispose();
    }
    assert!(!enabled.get());
    assert_eq!(count.get(), 2);
    {
        let mut scope =
            BenchmarkProfileScope::apply_logging(&profile, || enabled.get(), set, Some(Box::new(|| count.get())));
        set(true);
        scope.dispose();
    }
    assert!(enabled.get());
    assert_eq!(count.get(), 4);
}

#[test]
fn executor_scheduler_options_control_full_folder_escalation() {
    // NEW: image_executor.rs:568 delete field options from PreloadSchedulerDeps (MISSED).
    // C#: BenchmarkTrustTests.Executor_FullFolderControlsWholeFolderWarming.
    let root = TempRoot::new("w7-options");
    let files: Vec<_> = (0..45)
        .map(|i| {
            let path = root.join(format!("{i}.png"));
            fs::write(&path, b"fake pixels").unwrap();
            path.to_str().unwrap().to_owned()
        })
        .collect();
    let mut profile = profiles::find("fast-balanced").unwrap().clone();
    profile.next_window = 100;
    profile.previous_window = 100;
    for full_folder in [false, true] {
        profile.full_folder = full_folder;
        let executor = Arc::new(executor_with_profile(
            &root,
            &files,
            Arc::new(RecordingLog::new()),
            &profile,
        ));
        let (tx, rx) = std::sync::mpsc::channel();
        let background = executor.clone();
        let worker = std::thread::spawn(move || {
            background.warm_preload_around(0);
            background.when_preload_settled();
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(30)).unwrap();
        worker.join().unwrap();
        let cached = executor.is_preview_cached(&files[44]).unwrap();
        executor.dispose();
        assert!(
            executor
                .teardown_background_task()
                .wait_timeout(Duration::from_secs(30))
        );
        assert_eq!(cached, full_folder);
    }
}

#[test]
fn registry_sample_count_uses_case_insensitive_profile_and_unknown_none() {
    // NEW: workload_runner.rs:232 registry_images_per_sample -> None / Some(0/1/-1) (MISSED).
    // C#: BenchmarkModels ImagesPerSampleLookup + BenchmarkWorkloadRunner.ImagesPerSample.
    use pr_bench::workload_runner::registry_images_per_sample;
    let profile = profiles::find("fast-sequential").unwrap();
    assert!(profile.workers > 1);
    assert_eq!(
        registry_images_per_sample("FAST-SEQUENTIAL", BenchmarkWorkload::Sequential),
        Some(profile.workers)
    );
    assert_eq!(
        registry_images_per_sample("fast-sequential", BenchmarkWorkload::FirstFrame),
        Some(1)
    );
    assert_eq!(
        registry_images_per_sample("unknown-profile", BenchmarkWorkload::Sequential),
        None
    );
}

#[test]
fn cache_guard_requires_owned_or_empty_directory_and_checks_protection_first() {
    // NEW: tool_path_guard.rs:125-138 ensure_cache_directory body / && / ! mutants.
    // C#: ToolPathGuard.EnsureCacheDirectory; all protected roots here are TempRoot fixtures.
    use pr_bench::cli::tool_path_guard::{CACHE_MARKER, ensure_cache_directory};
    let root = TempRoot::new("w7-cache-guard");
    let fs = PhysicalFileSystem;
    let cache = root.join("cache");
    let cache_str = cache.to_str().unwrap();
    let protected = [(cache_str, "photo folder")];
    assert!(ensure_cache_directory(&fs, cache_str, &[]).is_ok());
    assert!(ensure_cache_directory(&fs, cache_str, &protected).is_err());
    fs::create_dir(&cache).unwrap();
    assert!(ensure_cache_directory(&fs, cache_str, &[]).is_ok());
    fs::write(cache.join("unowned.txt"), b"user data fixture").unwrap();
    assert!(ensure_cache_directory(&fs, cache_str, &[]).is_err());
    fs::remove_file(cache.join("unowned.txt")).unwrap();
    fs::create_dir(cache.join("unowned-dir")).unwrap();
    assert!(ensure_cache_directory(&fs, cache_str, &[]).is_err());
    fs::write(cache.join(CACHE_MARKER), b"owned").unwrap();
    assert!(ensure_cache_directory(&fs, cache_str, &[]).is_ok());
    assert!(ensure_cache_directory(&fs, cache_str, &protected).is_err());
}
