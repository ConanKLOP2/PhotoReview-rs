//! Port of `tests/PhotoReview.Imaging.Tests/PreloadSchedulerGapTests.cs` (C# baseline d92d6aae, 105 lines), the two tests
//! that need the scheduler shell: the diagnostic worker environment variable and the pause log. (`InPreloadWindow_Boundaries`
//! is the pure rule of W4-15a, `policy::in_preload_window`.)
//!
//! Red tests (W4-15b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Test notes: the C# reflection read of `_workerCount` becomes [`PreloadScheduler::worker_count`]; the process
//! environment is never mutated: the variable is injected through `PreloadSchedulerDeps::env_lookup` (`Setup::env`).

mod common;

use std::sync::Arc;

use common::preload_fakes::*;
use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_core::catalog::types::CatalogEntry;
use pr_core::diagnostics::diag_options::PRELOAD_WORKERS_VAR;
use pr_imaging::preload_order::PreloadWindow;

/// C# `LowMemoryProbe`: no headroom, a fixed 77 % load with 123 456 789 bytes available.
struct LowMemoryProbe;

impl MemoryProbe for LowMemoryProbe {
    fn has_headroom(&self, _maximum_load: f64, _reserve_bytes: i64) -> bool {
        false
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        Some(MemorySnapshot {
            load_percent: 77,
            available_bytes: 123_456_789,
        })
    }
}

fn no_entries() -> Arc<[CatalogEntry]> {
    Vec::<CatalogEntry>::new().into()
}

#[test]
fn diag_worker_environment_variable_overrides_the_option() {
    // C#: PreloadSchedulerGapTests.DiagWorkerEnvironmentVariable_OverridesTheOption
    let fs = fake_fs();
    let target = FakeTarget::new(&fs);
    let entries = no_entries();

    // `env_lookup` returns "3" for PRELOAD_WORKERS_VAR (the option says 8): the variable wins.
    let overridden = create(
        &target,
        &entries,
        Setup::new(8, PreloadWindow::DEFAULT).env(|name| (name == PRELOAD_WORKERS_VAR).then(|| "3".to_owned())),
    );
    assert_eq!(3, overridden.worker_count());

    // Unset: the configured option applies.
    let configured = create(&target, &entries, Setup::new(8, PreloadWindow::DEFAULT).env(|_| None));
    assert_eq!(8, configured.worker_count());
}

#[test]
fn paused_for_memory_logs_the_snapshot() {
    // C#: PreloadSchedulerGapTests.PausedForMemory_LogsTheSnapshot
    let fs = fake_fs();
    let entries = entries(&fs, 5, "gap");
    let target = FakeTarget::new(&fs);
    let log = CaptureLog::with_enabled(true);
    let scheduler = create(
        &target,
        &entries,
        Setup::new(2, PreloadWindow::DEFAULT)
            .probe(Arc::new(LowMemoryProbe))
            .log(&log),
    );

    assert!(finished(&scheduler.preload_around(0)));

    let paused: Vec<String> = log
        .infos()
        .into_iter()
        .filter(|m| m.starts_with("Preload paused for memory"))
        .collect();
    assert_eq!(1, paused.len(), "exactly one pause line: {paused:?}");
    assert!(paused[0].contains("availableBytes=123456789"), "{}", paused[0]);
    assert!(paused[0].contains("loadPercent=77"), "{}", paused[0]);
}
