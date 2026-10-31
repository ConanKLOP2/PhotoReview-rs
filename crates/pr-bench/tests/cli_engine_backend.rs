//! `EngineBackend` (ST-GAPMAP2 section 6, item 1): the engine-backed `BenchmarkBackend` that replaces `UnwiredBackend`.
//!
//! No real codec, no WIC, no real Recycle Bin and no timing claim: the host is a fake over the `pr-imaging` fake decoder, the
//! files are tiny placeholders in a `TempRoot`, and the "recycle bin" is the benchmark's temp-copy deleter. What these tests
//! pin is the WIRING: the catalog is the real one, a profile run goes through the real executor and workload runner, progress
//! is forwarded in order, the logging flag is set for the run and restored, reports and the batch summary are stored in call
//! order, and an unwired host fails every run with the "not wired" text instead of inventing numbers.

#[path = "../../pr-imaging/tests/support/preview_service.rs"]
mod preview_service;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pr_bench::bench_recycle_bin;
use pr_bench::cli::ToolError;
use pr_bench::cli::backend::{BenchmarkBackend, DatasetManifest, PhaseStatus, ProfileCatalog, ProgressLine};
use pr_bench::cli::engine_backend::{
    EngineBackend, EngineHost, ExecutorParts, UNWIRED_HOST_MESSAGE, UnwiredEngineHost,
};
use pr_bench::image_executor::{BenchmarkExecutorOptions, HasHeadroomFn};
use pr_core::abstractions::{Log, NullLog, RecycleBin};
use pr_test_support::TempRoot;
use preview_service::{FakeDecoder, FakePayloadCodec, PHYSICAL_32_GIB, PixelsCodec};

/// A host over the fake decoder. `scratch` keeps the executor's disk cache inside the test's `TempRoot`.
struct FakeHost {
    scratch: std::path::PathBuf,
    logging: Arc<AtomicBool>,
    saw_logging_during_run: Arc<AtomicBool>,
}

impl FakeHost {
    fn new(root: &TempRoot) -> Self {
        Self {
            scratch: root.path().join("scratch"),
            logging: Arc::new(AtomicBool::new(false)),
            saw_logging_during_run: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl EngineHost for FakeHost {
    fn executor_parts(&self) -> Result<ExecutorParts, ToolError> {
        let has_headroom: HasHeadroomFn = Arc::new(|_: f64| true);
        Ok(ExecutorParts {
            platform_codec: Arc::new(PixelsCodec),
            payload_codec: Arc::new(FakePayloadCodec),
            options: BenchmarkExecutorOptions {
                has_headroom: Some(has_headroom),
                decoder: Some(FakeDecoder::new()),
                scratch_parent: Some(self.scratch.clone()),
                physical_memory_bytes: PHYSICAL_32_GIB,
                ..BenchmarkExecutorOptions::default()
            },
        })
    }
    fn recycle_bin(&self) -> Arc<dyn RecycleBin> {
        // The default deleter: it only deletes the benchmark's own temp copies. Never the real bin.
        bench_recycle_bin::create_with(false, || unreachable!("the real bin is never requested in a test"))
    }
    fn log(&self) -> Arc<dyn Log> {
        Arc::new(NullLog)
    }
    fn logging_enabled(&self) -> bool {
        self.logging.load(Ordering::SeqCst)
    }
    fn set_logging_enabled(&self, enabled: bool) {
        if enabled {
            self.saw_logging_during_run.store(true, Ordering::SeqCst);
        }
        self.logging.store(enabled, Ordering::SeqCst);
    }
    fn machine_name(&self) -> Option<String> {
        Some("TEST-MACHINE".to_owned())
    }
}

fn dataset(folder: &str, files: &[String]) -> DatasetManifest {
    DatasetManifest {
        folder: folder.to_owned(),
        files: files.to_vec(),
        unsupported_extensions: vec![("txt".to_owned(), 2)],
        file_limit: 64,
    }
}

fn make_files(root: &TempRoot, names: &[&str]) -> Vec<String> {
    let dir = root.path().join("photos");
    std::fs::create_dir_all(&dir).expect("create the photos folder");
    names
        .iter()
        .map(|name| {
            let path = dir.join(name);
            std::fs::write(&path, b"placeholder").expect("write a placeholder file");
            path.to_string_lossy().into_owned()
        })
        .collect()
}

#[test]
fn catalog_is_the_real_one_30_profiles_27_runnable() {
    let backend = EngineBackend::new(UnwiredEngineHost);
    let all = backend.all();
    let runnable = backend.runnable();
    assert_eq!(all.len(), 30, "27 runnable + 1 correctness-only + 2 not implemented");
    assert_eq!(runnable.len(), 27);
    // runnable = all minus the correctness-only profile and the two not-implemented ones
    assert_eq!(all.iter().filter(|p| p.correctness_only).count(), 1);
    let found = backend
        .find("fast-sequential")
        .expect("fast-sequential is in the catalog");
    assert_eq!(found.id, "fast-sequential");
    assert_eq!(found.workload, "Sequential");
    assert_eq!(found.loading_mode, "Fast");
    assert!(!found.is_file_action);
    assert!(backend.find("no-such-profile").is_none());
}

#[test]
fn file_action_profiles_are_flagged_for_the_actions_mode() {
    let backend = EngineBackend::new(UnwiredEngineHost);
    let actions: Vec<_> = backend.all().into_iter().filter(|p| p.is_file_action).collect();
    assert!(!actions.is_empty(), "the catalog has FileAction profiles");
    assert!(actions.iter().all(|p| p.workload == "FileAction"));
}

#[test]
fn unwired_host_fails_a_run_with_the_not_wired_text_and_stores_nothing() {
    let mut backend = EngineBackend::new(UnwiredEngineHost);
    let profile = backend.find("fast-sequential").expect("profile");
    let mut progress: Vec<ProgressLine> = Vec::new();
    let error = backend
        .run_profile("C:\\photos", &profile, &["C:\\photos\\a.jpg".to_owned()], &mut |p| {
            progress.push(p.clone());
        })
        .expect_err("an unwired host cannot measure anything");
    assert_eq!(error.type_name, "InvalidOperationException");
    assert_eq!(error.message, UNWIRED_HOST_MESSAGE);
    assert!(progress.is_empty());
    assert!(backend.reports().is_empty());
}

#[test]
fn unknown_profile_is_an_invalid_operation_before_the_host_is_asked() {
    let mut backend = EngineBackend::new(UnwiredEngineHost);
    let mut profile = backend.find("fast-sequential").expect("profile");
    profile.id = "no-such-profile".to_owned();
    let error = backend
        .run_profile("C:\\photos", &profile, &[], &mut |_| {})
        .expect_err("unknown profile");
    assert_eq!(error.type_name, "InvalidOperationException");
    assert!(error.message.contains("no-such-profile"), "{}", error.message);
}

#[test]
fn failed_profile_is_recorded_as_fail_with_a_report_and_in_the_summary() {
    let mut backend = EngineBackend::new(UnwiredEngineHost);
    let profile = backend.find("random-navigation").expect("profile");
    let run = backend.record_failed_profile(
        "C:\\photos",
        &profile,
        "Profile failed before producing samples: IOException: boom",
    );
    assert_eq!(run.phase.status, PhaseStatus::Fail);
    assert_eq!(run.run_id.len(), 32);
    assert!(run.run_id.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(backend.reports().len(), 1);

    let report = backend.report_json(&run.run_id).expect("report json");
    assert!(report.contains(&run.run_id));
    assert!(report.contains("random-navigation"));
    assert!(report.contains("boom"));
    assert!(report.contains("C:\\\\photos"), "folder is JSON-escaped: {report}");

    let summary = backend
        .batch_summary_json(&dataset("C:\\photos", &["C:\\photos\\a.jpg".to_owned()]))
        .expect("summary json");
    assert!(summary.contains("a.jpg"));
    assert!(summary.contains("\"txt\""));
    assert!(summary.contains(&run.run_id));
    assert!(summary.contains("boom"));
}

#[test]
fn report_json_for_an_unknown_run_id_is_an_invalid_operation() {
    let backend = EngineBackend::new(UnwiredEngineHost);
    let error = backend
        .report_json("00000000000000000000000000000000")
        .expect_err("no such run");
    assert_eq!(error.type_name, "InvalidOperationException");
}

#[test]
fn reports_and_outcomes_keep_call_order() {
    let mut backend = EngineBackend::new(UnwiredEngineHost);
    let first = backend.find("instant-review").expect("profile");
    let second = backend.find("nearby-only").expect("profile");
    let a = backend.record_failed_profile("C:\\photos", &first, "one");
    let b = backend.record_failed_profile("C:\\photos", &second, "two");
    assert_ne!(a.run_id, b.run_id);
    let ids: Vec<_> = backend.reports().iter().map(|r| r.run_id.clone()).collect();
    assert_eq!(ids, vec![a.run_id, b.run_id]);
}

#[test]
fn a_run_through_the_fake_host_forwards_progress_stores_the_report_and_restores_logging() {
    let root = TempRoot::new("engine-backend-run");
    let files = make_files(&root, &["a.jpg", "b.jpg", "c.jpg"]);
    let folder = root.path().join("photos").to_string_lossy().into_owned();
    let host = FakeHost::new(&root);
    let mut backend = EngineBackend::new(host);
    let profile = backend.find("fast-sequential").expect("profile");

    let mut progress: Vec<ProgressLine> = Vec::new();
    let run = backend
        .run_profile(&folder, &profile, &files, &mut |p| progress.push(p.clone()))
        .expect("the fake host runs the profile");

    assert_eq!(run.run_id.len(), 32);
    assert!(!progress.is_empty(), "progress is forwarded");
    assert!(progress.iter().all(|p| p.profile_id == "fast-sequential"));
    assert!(
        progress.windows(2).all(|w| w[0].completed <= w[1].completed),
        "progress arrives in order"
    );
    assert_eq!(backend.reports().len(), 1);
    assert_eq!(backend.reports()[0].run_id, run.run_id);
    assert_eq!(backend.reports()[0].machine.as_deref(), Some("TEST-MACHINE"));

    let json = backend.report_json(&run.run_id).expect("report json");
    assert!(json.contains("fast-sequential"));
    let summary = backend
        .batch_summary_json(&dataset(&folder, &files))
        .expect("summary json");
    assert!(summary.contains(&run.run_id));
}

#[test]
fn logging_flag_follows_the_profile_during_the_run_and_is_restored_after() {
    let root = TempRoot::new("engine-backend-logging");
    let files = make_files(&root, &["a.jpg", "b.jpg"]);
    let folder = root.path().join("photos").to_string_lossy().into_owned();
    let host = FakeHost::new(&root);
    let logging = Arc::clone(&host.logging);
    let saw = Arc::clone(&host.saw_logging_during_run);
    let mut backend = EngineBackend::new(host);
    let profile = backend.find("logging-on").expect("profile");
    assert!(!logging.load(Ordering::SeqCst), "starts off");

    backend
        .run_profile(&folder, &profile, &files, &mut |_| {})
        .expect("the fake host runs logging-on");

    assert!(saw.load(Ordering::SeqCst), "logging-on turns the flag on for the run");
    assert!(!logging.load(Ordering::SeqCst), "the previous value is restored");
}
