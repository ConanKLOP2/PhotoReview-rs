//! Port of the pure rules of `PhotoReview.Imaging/Preload/PreloadScheduler.cs` (C# baseline d92d6aae) and of the C# tests
//! that pin them: `PreloadDiagWorkersParseTests.cs` (`ParseDiagWorkers`), `PreloadSchedulerGapTests.cs`
//! (`InPreloadWindow` boundaries) and the policy-level twins of `PreloadSchedulerMutationTests.Resume.cs` and
//! `PreloadSchedulerMutationTests.cs`. Golden check against `tests/golden/preload-policy.json` (213 cases, oracle command
//! `preload-policy`, inputs in `tests/golden-inputs/preload-policy.json`).
//!
//! Port of `PhotoReview.Imaging/Preload/PreloadScheduler.cs` (policy half) and `Preload/IPreloadTarget.cs`.
//! Red tests (W4-15a): `crates/pr-imaging/src/preload/{policy,target}.rs` are `todo!()`, so these tests compile against
//! the stub and FAIL until the porter implements them.
//!
//! Documented skips and substitutions:
//! - `PreloadSchedulerGapTests.DiagWorkerEnvironmentVariable_OverridesTheOption` reads the environment and builds a
//!   scheduler: constructor concern, W4-15b.
//! - The end-to-end `ViewerDecodeActive_*` start counts and `NoViewerDecode_StartsEveryWorker` count starts of a live
//!   scheduler: W4-15b. Only the worker-limit rule itself is pinned here.
//! - The golden `constants` section is checked inside the golden test, not in its own test, so that it cannot pass on
//!   the stub by itself.
//! - JSON has no negative zero: golden case `f0/negative-zero` stores `0`; this test takes the sign from the case name.

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use pr_core::abstractions::{MemoryProbe, MemorySnapshot};
use pr_core::catalog::types::CatalogEntry;
use pr_core::sync::CancelToken;
use pr_core::text::dotnet_path::PathError;
use pr_imaging::cache_key::{CacheKeyError, ImageCacheKey};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::decoder_traits::DecodeError;
use pr_imaging::preload::policy::{self, HeadroomProbeState, PreloadOutcome, ResumeVerdict, Shape};
use pr_imaging::preload::target::{DelegateMemoryProbe, KeyError, PreloadTarget};
use pr_imaging::preload_order::{PreloadOptions, PreloadWindow};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

// ---- helpers -----------------------------------------------------------------------------------------------------

/// Probe with a fixed load: headroom is `load < maximum_load` (C# `LoadProbe` of the resume tests). Records every call.
struct LoadProbe {
    load: f64,
    maxima: Mutex<Vec<(f64, i64)>>,
}

impl LoadProbe {
    fn new(load: f64) -> Self {
        Self {
            load,
            maxima: Mutex::new(Vec::new()),
        }
    }

    /// `(maximum_load, reserve_bytes)` of every `has_headroom` call, in order.
    fn calls(&self) -> Vec<(f64, i64)> {
        self.maxima.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl MemoryProbe for LoadProbe {
    fn has_headroom(&self, maximum_load: f64, reserve_bytes: i64) -> bool {
        self.maxima
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((maximum_load, reserve_bytes));
        self.load < maximum_load
    }

    fn get_snapshot(&self) -> Option<MemorySnapshot> {
        None
    }
}

/// Target that only answers what `evaluate_resume` reads: the cache size (counted) and the viewer decodes.
struct ResumeTarget {
    cache_bytes: i64,
    viewer: i32,
    cache_bytes_reads: AtomicUsize,
}

impl ResumeTarget {
    fn new(cache_bytes: i64, viewer: i32) -> Self {
        Self {
            cache_bytes,
            viewer,
            cache_bytes_reads: AtomicUsize::new(0),
        }
    }

    fn reads(&self) -> usize {
        self.cache_bytes_reads.load(Ordering::SeqCst)
    }
}

impl PreloadTarget for ResumeTarget {
    fn try_get_cached_preview_for_path(&self, _path: &str) -> bool {
        unreachable!("evaluate_resume never looks at the RAM cache")
    }

    fn try_get_cached_preview(&self, _key: &ImageCacheKey) -> bool {
        unreachable!("evaluate_resume never looks at the RAM cache")
    }

    fn preload(&self, _path: &str, _cancel: &CancelToken) -> Result<(), DecodeError> {
        unreachable!("evaluate_resume never decodes")
    }

    fn get_current_cache_key_for_path(&self, _path: &str) -> Result<ImageCacheKey, KeyError> {
        unreachable!("evaluate_resume never builds keys")
    }

    fn get_current_cache_key(&self, _entry: &CatalogEntry) -> Result<ImageCacheKey, KeyError> {
        unreachable!("evaluate_resume never builds keys")
    }

    fn cache_count(&self) -> i32 {
        unreachable!("evaluate_resume reads only the cache size")
    }

    fn cache_bytes(&self) -> i64 {
        self.cache_bytes_reads.fetch_add(1, Ordering::SeqCst);
        self.cache_bytes
    }

    fn active_viewer_decodes(&self) -> i32 {
        self.viewer
    }
}

/// Golden numbers in JSON: integers beyond i64 range are stored as doubles (`9223372036854776000`), so read them as f64
/// and convert with Rust's saturating cast (`2^63` becomes `i64::MAX`, `-2^63` becomes `i64::MIN`).
fn i64_lossy(value: &Value) -> i64 {
    value.as_f64().unwrap_or_else(|| panic!("not a number: {value}")) as i64
}

fn i32_of(case: &Value, key: &str) -> i32 {
    i64_of(case, key) as i32
}

fn usize_of(case: &Value, key: &str) -> usize {
    i64_of(case, key) as usize
}

fn f64_of(case: &Value, key: &str) -> f64 {
    case[key]
        .as_f64()
        .unwrap_or_else(|| panic!("case has no number '{key}': {case}"))
}

/// `Some(snapshot)` when both fields are present, `None` when the golden stores `null` (C# `MemorySnapshot?` null).
fn snapshot_of(case: &Value) -> Option<MemorySnapshot> {
    let load = case["load_percent"].as_f64()?;
    let available = case["available_bytes"].as_f64()?;
    Some(MemorySnapshot {
        load_percent: load as u32,
        available_bytes: available as u64,
    })
}

/// Ticks (100 ns) of a duration, as the golden reports them.
fn ticks_of(duration: Duration) -> i64 {
    (duration.as_nanos() / 100) as i64
}

fn ensure_same<T: PartialEq + std::fmt::Debug>(actual: T, expected: T) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("got {actual:?}, golden says {expected:?}"))
    }
}

fn verdict_name(verdict: ResumeVerdict) -> &'static str {
    match verdict {
        ResumeVerdict::Resume => "Resume",
        ResumeVerdict::NotYet => "NotYet",
        ResumeVerdict::CacheFull => "CacheFull",
    }
}

/// Checks one golden case; `Err` carries the difference (the test reports every difference, not only the first).
fn check_case(section: &str, case: &Value) -> Result<(), String> {
    match section {
        "parse-diag-workers" => {
            let value = case["value"].as_str();
            let expected = case["result"].as_i64().map(|n| n as i32);
            ensure_same(policy::parse_diag_workers(value), expected)
        }
        "in-preload-window" => {
            let shape = Shape {
                direction: i32_of(case, "direction"),
                lead: i32_of(case, "lead"),
            };
            let window = PreloadWindow::new(i32_of(case, "forward"), i32_of(case, "backward"));
            let actual = policy::in_preload_window(i32_of(case, "index"), i32_of(case, "center"), shape, window);
            ensure_same(actual, bool_of(case, "result"))
        }
        "is-still-wanted" => {
            let shape = Shape {
                direction: i32_of(case, "direction"),
                lead: i32_of(case, "lead"),
            };
            let actual = policy::is_still_wanted(i32_of(case, "index"), i32_of(case, "center"), shape);
            ensure_same(actual, bool_of(case, "result"))
        }
        "resume-check-delay" => {
            let actual = ticks_of(policy::resume_check_delay(i32_of(case, "check")));
            ensure_same(actual, i64_of(case, "ticks"))
        }
        "resume-verdict" => {
            let options = PreloadOptions {
                memory_load_limit: f64_of(case, "memory_load_limit"),
                full_folder_threshold_bytes: i64_of(case, "threshold"),
                ..PreloadOptions::default()
            };
            let target = ResumeTarget::new(i64_lossy(&case["cache_bytes"]), i32_of(case, "viewer"));
            let probe = LoadProbe::new(f64_of(case, "load"));
            let verdict = policy::evaluate_resume(&options, &target, &probe);
            ensure_same(verdict_name(verdict), str_of(case, "verdict"))?;
            let expected_calls: Vec<(f64, i64)> = case["probe_calls"]
                .as_array()
                .unwrap_or_else(|| panic!("no probe_calls: {case}"))
                .iter()
                .map(|call| (f64_of(call, "max"), i64_of(call, "reserve")))
                .collect();
            ensure_same(probe.calls(), expected_calls)?;
            ensure_same(target.reads(), usize_of(case, "cache_bytes_reads"))
        }
        "fill-limit" => {
            let actual = policy::fill_limit_reached(i64_lossy(&case["cache_bytes"]), i64_lossy(&case["threshold"]));
            ensure_same(actual, bool_of(case, "result"))
        }
        "f0" => {
            // JSON cannot carry the sign of zero: the case name says which zero the oracle printed.
            let value = if str_of(case, "name") == "negative-zero" {
                -0.0
            } else {
                f64_of(case, "value")
            };
            ensure_same(policy::format_f0_invariant(value), str_of(case, "text").to_owned())
        }
        "policy-log" => {
            let shape = Shape {
                direction: i32_of(case, "direction"),
                lead: i32_of(case, "lead"),
            };
            let text = policy::format_policy_log(
                usize_of(case, "images"),
                DecodeBox::new(i32_of(case, "box_width"), i32_of(case, "box_height")),
                case["measured"].as_f64(),
                i64_of(case, "estimated"),
                i64_of(case, "capacity"),
                bool_of(case, "whole_folder"),
                i32_of(case, "center"),
                shape,
            );
            ensure_same(text, str_of(case, "text").to_owned())
        }
        "paused-log" => {
            let text = policy::format_paused_log(
                usize_of(case, "queued"),
                i32_of(case, "cache_count"),
                i64_lossy(&case["cache_bytes"]),
                snapshot_of(case),
            );
            ensure_same(text, str_of(case, "text").to_owned())
        }
        "progress-log" => {
            let text = policy::format_progress_log(
                usize_of(case, "queued"),
                usize_of(case, "active"),
                i32_of(case, "cache_count"),
                i64_lossy(&case["cache_bytes"]),
                snapshot_of(case),
            );
            ensure_same(text, str_of(case, "text").to_owned())
        }
        "constants" => {
            let values = &case["values"];
            ensure_same(
                policy::WHOLE_FOLDER_CACHE_FILL_LIMIT,
                f64_of(values, "WholeFolderCacheFillLimit"),
            )?;
            ensure_same(
                policy::SLOW_LINK_DECODE_MS_THRESHOLD,
                f64_of(values, "SlowLinkDecodeMsThreshold"),
            )?;
            ensure_same(
                policy::SLOW_LINK_VIEWER_BUSY_WORKER_LIMIT,
                i32_of(values, "SlowLinkViewerBusyWorkerLimit"),
            )?;
            ensure_same(
                ticks_of(policy::RESUME_CHECK_BASE_DELAY),
                i64_of(values, "ResumeCheckBaseDelayTicks"),
            )?;
            ensure_same(
                ticks_of(policy::RESUME_CHECK_MAX_DELAY),
                i64_of(values, "ResumeCheckMaxDelayTicks"),
            )?;
            ensure_same(policy::MAX_RESUME_CHECKS, i32_of(values, "MaxResumeChecks"))?;
            ensure_same(policy::RESUME_LOAD_MARGIN, f64_of(values, "ResumeLoadMargin"))?;
            ensure_same(policy::HEADROOM_RECHECK_MS, i64_of(values, "HeadroomRecheckMs"))?;
            ensure_same(
                ticks_of(policy::DEFAULT_DISPOSE_DRAIN_TIMEOUT),
                i64_of(values, "DisposeDrainTimeoutTicks"),
            )?;
            ensure_same(policy::CONVENIENCE_DEFAULT_WORKER_COUNT, 8)?;
            ensure_same(policy::MAX_DIAG_WORKERS, 16)
        }
        other => Err(format!("unknown golden section '{other}'")),
    }
}

// ---- golden -------------------------------------------------------------------------------------------------------

#[test]
fn preload_policy_golden_matches_csharp() {
    let golden = golden::load("preload-policy");
    assert!(
        golden.csharp_commit.starts_with("d92d6aae"),
        "golden made from {}",
        golden.csharp_commit
    );
    assert_eq!(golden.cases.len(), 213, "golden case count");

    let mut differences = Vec::new();
    for case in &golden.cases {
        let section = str_of(case, "section");
        let name = str_of(case, "name");
        if let Err(message) = check_case(section, case) {
            differences.push(format!("{section}/{name}: {message}"));
        }
    }
    assert!(
        differences.is_empty(),
        "{} of {} golden cases differ from C#:\n{}",
        differences.len(),
        golden.cases.len(),
        differences.join("\n")
    );
}

// ---- ParseDiagWorkers (PreloadDiagWorkersParseTests.cs) -----------------------------------------------------------

// C#: PreloadDiagWorkersParseTests.ParseDiagWorkers_Value_ReturnsExpected (13 rows)
#[test]
fn parse_diag_workers_value_returns_expected() {
    let rows: [(Option<&str>, Option<i32>); 13] = [
        (None, None),
        (Some(""), None),
        (Some("abc"), None),
        (Some("-1"), None),
        (Some("0"), Some(0)),
        (Some("1"), Some(1)),
        (Some("16"), Some(16)),
        (Some("17"), None),
        (Some("8"), Some(8)),
        (Some(" 4 "), Some(4)),
        (Some("+3"), Some(3)),
        (Some("2147483648"), None),
        (Some("1.5"), None),
    ];
    for (value, expected) in rows {
        assert_eq!(policy::parse_diag_workers(value), expected, "value {value:?}");
    }
}

// ---- InPreloadWindow (PreloadSchedulerGapTests.cs) ----------------------------------------------------------------

// C#: PreloadSchedulerGapTests.InPreloadWindow_Boundaries (16 rows; centre 10, forward 3, backward 1)
#[test]
fn in_preload_window_boundaries() {
    // (direction, lead, index, expected)
    let rows: [(i32, i32, i32, bool); 16] = [
        (1, 2, 10, false),
        (1, 2, 11, true),
        (1, 2, 15, true),
        (1, 2, 16, false),
        (1, 2, 9, true),
        (1, 2, 8, false),
        (-1, 2, 9, true),
        (-1, 2, 5, true),
        (-1, 2, 4, false),
        (-1, 2, 11, true),
        (-1, 2, 12, false),
        (0, 0, 11, true),
        (0, 0, 13, true),
        (0, 0, 14, false),
        (0, 0, 9, true),
        (0, 0, 8, false),
    ];
    let window = PreloadWindow::new(3, 1);
    for (direction, lead, index, expected) in rows {
        let shape = Shape { direction, lead };
        assert_eq!(
            policy::in_preload_window(index, 10, shape, window),
            expected,
            "index {index} with direction {direction}, lead {lead}"
        );
    }
}

// ---- resume policy (PreloadSchedulerMutationTests.Resume.cs, policy-level twins) ------------------------------------

// C#: PreloadSchedulerMutationTests.Resume.MemoryPause_PressurePersists_BoundedBackoffWithoutResume (policy twin)
#[test]
fn memory_pause_pressure_persists_bounded_backoff_without_resume() {
    let delays: Vec<f64> = (0..policy::MAX_RESUME_CHECKS)
        .map(|check| policy::resume_check_delay(check).as_secs_f64())
        .collect();
    assert_eq!(delays, vec![2.0, 4.0, 8.0, 16.0, 30.0, 30.0, 30.0, 30.0]);
    assert_eq!(policy::MAX_RESUME_CHECKS, 8);
    // C#: ...the resumed run that pauses again backs off from its next check (4 s, not 2 s).
    assert_eq!(policy::resume_check_delay(1), Duration::from_secs(4));
}

// C#: PreloadSchedulerMutationTests.Resume.MemoryPause_LoadInsideMargin_DoesNotResume (policy twin)
#[test]
fn memory_pause_load_inside_margin_does_not_resume() {
    let options = PreloadOptions::default(); // pause at >= 0.80, resume only below 0.75
    let target = ResumeTarget::new(0, 0);

    // 0.775: inside the hysteresis band, must not resume; the probe is asked with the limit minus the margin.
    let inside = LoadProbe::new(0.80 - policy::RESUME_LOAD_MARGIN / 2.0);
    assert_eq!(
        policy::evaluate_resume(&options, &target, &inside),
        ResumeVerdict::NotYet
    );
    assert!(inside.calls().iter().any(|(max, _)| (max - (0.80 - 0.05)).abs() < 1e-9));

    let below = LoadProbe::new(0.70);
    assert_eq!(
        policy::evaluate_resume(&options, &target, &below),
        ResumeVerdict::Resume
    );
}

// C#: PreloadSchedulerMutationTests.Resume.MemoryPause_CacheAtFillLimit_DoesNotResume (policy twin)
#[test]
fn memory_pause_cache_at_fill_limit_does_not_resume() {
    let options = PreloadOptions {
        full_folder_threshold_bytes: 1_000_000_000,
        ..PreloadOptions::default()
    };
    let target = ResumeTarget::new(950_000_000, 0);
    let probe = LoadProbe::new(0.50);

    assert_eq!(
        policy::evaluate_resume(&options, &target, &probe),
        ResumeVerdict::CacheFull
    );
    assert!(probe.calls().is_empty(), "a full cache must not even probe memory");
}

// C#: PreloadSchedulerMutationTests.Resume.MemoryPause_ViewerDecoding_WaitsForTheNextCheck (policy twin)
#[test]
fn memory_pause_viewer_decoding_waits_for_the_next_check() {
    let options = PreloadOptions::default();
    let probe = LoadProbe::new(0.50);

    let busy = ResumeTarget::new(0, 1);
    assert_eq!(policy::evaluate_resume(&options, &busy, &probe), ResumeVerdict::NotYet);
    assert!(probe.calls().is_empty(), "no probe while a viewer decode runs");

    let idle = ResumeTarget::new(0, 0);
    assert_eq!(policy::evaluate_resume(&options, &idle, &probe), ResumeVerdict::Resume);
}

// ---- worker limit (PreloadSchedulerMutationTests.cs, policy-level twins) ----------------------------------------------

// C#: field initializer `Math.Max(2, Environment.ProcessorCount / 3)` (PreloadScheduler.cs, `_viewerBusyWorkerLimit`)
#[test]
fn viewer_busy_worker_limit_is_max_two_or_cores_over_three() {
    for (cpus, expected) in [(1, 2), (2, 2), (6, 2), (8, 2), (9, 3), (16, 5), (30, 10)] {
        assert_eq!(policy::viewer_busy_worker_limit(cpus), expected, "{cpus} cores");
    }
}

// C#: PreloadSchedulerMutationTests.ViewerDecodeActive_LinkAtThreshold_IsNotSlow (policy twin)
#[test]
fn viewer_decode_active_link_at_threshold_is_not_slow() {
    let cpus = thread::available_parallelism().map_or(1, |n| n.get() as i32);
    let limit = policy::viewer_busy_worker_limit(cpus);

    let at_threshold = policy::worker_limit(16, 1, 200.0, limit);
    assert_eq!(at_threshold, 16_i32.min(limit));
    assert_ne!(at_threshold, 1, "exactly 200 ms is not a slow link");
    assert_eq!(
        policy::worker_limit(16, 1, 201.0, limit),
        1,
        "above 200 ms is a slow link"
    );
}

// Contract semantics 4 (unit rows, no golden section).
#[test]
fn worker_limit_follows_the_viewer_rules() {
    assert_eq!(
        policy::worker_limit(16, 0, 500.0, 5),
        16,
        "no viewer decode: every worker, slow link or not"
    );
    assert_eq!(policy::worker_limit(16, 1, 201.0, 5), 1);
    assert_eq!(policy::worker_limit(16, 1, 200.0, 5), 5);
    assert_eq!(policy::worker_limit(16, 2, 0.0, 2), 2);
    assert_eq!(policy::worker_limit(1, 1, 0.0, 5), 1);
    assert_eq!(policy::worker_limit(16, 1, 0.0, 20), 16);
}

// ---- Rust-only rules (contract semantics 5, 8, 12 and the outcome helper) -------------------------------------------

#[test]
fn forget_unless_failed_keeps_only_failed_paths_queued() {
    assert!(policy::forget_unless_failed(PreloadOutcome::Cached));
    assert!(policy::forget_unless_failed(PreloadOutcome::Superseded));
    assert!(policy::forget_unless_failed(PreloadOutcome::Busy));
    assert!(!policy::forget_unless_failed(PreloadOutcome::Failed));
}

#[test]
fn resume_max_load_subtracts_the_margin_and_never_goes_below_zero() {
    assert!((policy::resume_max_load(0.80) - 0.75).abs() < 1e-12);
    assert!(policy::resume_max_load(0.04).abs() < 1e-12, "clamped to zero");
    assert!(
        policy::resume_max_load(0.05).abs() < 1e-12,
        "equal to the margin gives zero"
    );
    assert!(policy::resume_max_load(f64::NAN).is_nan(), ".NET Math.Max keeps NaN");
}

#[test]
fn headroom_cache_reprobes_only_after_fifty_ms_or_a_queued_decode() {
    let probes = Cell::new(0);
    let probe = |answer: bool| {
        let counter = &probes;
        move || {
            counter.set(counter.get() + 1);
            answer
        }
    };
    let mut state = HeadroomProbeState::default();

    // never probed: the first call probes
    assert!(state.has_headroom_cached(1_000, probe(true)));
    assert_eq!(probes.get(), 1);

    // exactly 50 ms later is not "more than 50": the cached answer stands
    assert!(state.has_headroom_cached(1_050, probe(false)));
    assert_eq!(probes.get(), 1);

    // 51 ms later: probe again
    assert!(!state.has_headroom_cached(1_051, probe(false)));
    assert_eq!(probes.get(), 2);

    // a decode was queued since the last probe: probe again at the same tick
    state.decode_queued_since_check = true;
    assert!(state.has_headroom_cached(1_051, probe(true)));
    assert_eq!(probes.get(), 3);
    assert!(!state.decode_queued_since_check, "a probe clears the queued flag");
}

// ---- log texts without a golden section (texts checked against PreloadScheduler.cs d92d6aae) ------------------------

#[test]
fn log_texts_without_a_golden_section_match_the_csharp_interpolations() {
    let path = r"C:\photos\a.jpg";
    assert_eq!(
        policy::format_resumed_log(5),
        "Preload resumed after memory pause: center=5"
    );
    assert_eq!(
        policy::format_stays_paused_log(123),
        "Preload stays paused: cache at its fill limit (cacheBytes=123)"
    );
    assert_eq!(
        policy::format_skipped_cannot_stat(path),
        format!("Preload skipped, cannot stat: {path}")
    );
    assert_eq!(
        policy::format_deferred_busy(path),
        format!("Preload deferred, decoder busy: {path}")
    );
    assert_eq!(
        policy::format_deferred_busy_again(path),
        format!("Preload deferred again, decoder busy: {path}")
    );
    assert_eq!(policy::format_preload_failed(path), format!("Preload failed: {path}"));
    assert_eq!(
        policy::format_foreign_cancel(path, "The operation was canceled."),
        format!("Preload cancelled by another source: {path} (The operation was canceled.)")
    );
}

// ---- target seam (contract semantics 1 and 14; Rust-only) -----------------------------------------------------------

#[test]
fn key_error_maps_io_path_failures_to_io_and_argument_errors_to_other() {
    let missing = KeyError::from(CacheKeyError::SourceNoLongerExists {
        full_name: r"C:\gone.jpg".to_owned(),
    });
    assert!(matches!(missing, KeyError::Io(_)), "a vanished file is an IOException");
    assert!(
        matches!(
            KeyError::from(CacheKeyError::InvalidPath(PathError::TooLong)),
            KeyError::Io(_)
        ),
        "PathTooLongException is an IOException"
    );
    assert!(
        matches!(
            KeyError::from(CacheKeyError::InvalidPath(PathError::Argument)),
            KeyError::Other(_)
        ),
        "ArgumentException is not caught by the key arms"
    );
}

#[test]
fn delegate_probe_passes_the_maximum_load_and_ignores_the_reserve() {
    let seen: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let probe = DelegateMemoryProbe::new(move |maximum| {
        recorder.lock().unwrap_or_else(PoisonError::into_inner).push(maximum);
        maximum < 0.5
    });

    assert!(probe.has_headroom(0.4, 123));
    assert!(!probe.has_headroom(0.6, 0));
    assert_eq!(*seen.lock().unwrap_or_else(PoisonError::into_inner), vec![0.4, 0.6]);
}

#[test]
fn delegate_probe_snapshot_is_the_constant_fifty_percent_and_sixteen_gib() {
    let probe = DelegateMemoryProbe::new(|_| false);
    let snapshot = probe.get_snapshot().expect("the delegate probe always has a snapshot");
    assert_eq!(snapshot.load_percent, 50);
    assert_eq!(snapshot.available_bytes, 16 * 1024 * 1024 * 1024);
}
