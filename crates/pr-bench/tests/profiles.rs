//! Port of `PhotoReview.Integration.Tests/BenchmarkProfileTests.cs` and `BenchmarkRunnableProfileTests` (C# baseline
//! d92d6aae) plus golden checks against `tests/golden/benchmark-profiles.json` (oracle `tools/oracle-cs/Commands/W7-01b.cs`).
//!
//! Port of `PhotoReview.Benchmarking/BenchmarkProfiles.cs` (C# baseline d92d6aae). Unit W7-01b, contract `contracts/W7-01b.md`.
//! Red stage: the stub bodies are `todo!()`, so every test here fails until the porter implements them.
//!
//! Documented skip: `BenchmarkProfileTests.SpeedRankingExcludesCorrectnessOnlyProfile` also calls `BenchmarkRanking.Rank`
//! (needs `BenchmarkPhaseResult`, W7-01a / W7-03). Only its registry half is ported here (`speed_profiles_exclude_correctness_only`).

use std::collections::HashSet;
use std::ptr;

use pr_bench::models::{BenchmarkProfile, BenchmarkWorkload};
use pr_bench::profiles::{all, find, is_not_implemented, runnable};
use pr_core::model::serde_ext::LenientEnum;
use pr_core::text::ignore_case_key;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// The 27 runnable ids in `all()` order: the 30-row table minus `original-correctness`, `cache-recovery` and `explorer-reindex`.
const RUNNABLE_IDS: [&str; 27] = [
    "instant-review",
    "fast-sequential",
    "fast-balanced",
    "fast-aggressive",
    "preview-light",
    "preview-balanced",
    "preview-quality",
    "preview-high-quality",
    "no-preload-baseline",
    "nearby-only",
    "full-folder-warm",
    "large-folder-safe",
    "huge-image-safe",
    "ssd-throughput",
    "hdd-conservative",
    "network-safe",
    "low-memory",
    "ram-maximizer",
    "rapid-key-press",
    "random-navigation",
    "logging-on",
    "logging-off",
    "recommended-auto",
    "action-move",
    "action-delete",
    "action-copy",
    "action-interleaved",
];

fn workload_name(workload: BenchmarkWorkload) -> &'static str {
    match workload {
        BenchmarkWorkload::FirstFrame => "FirstFrame",
        BenchmarkWorkload::Sequential => "Sequential",
        BenchmarkWorkload::Random => "Random",
        BenchmarkWorkload::WarmNext => "WarmNext",
        BenchmarkWorkload::Preload => "Preload",
        BenchmarkWorkload::FileAction => "FileAction",
        BenchmarkWorkload::Correctness => "Correctness",
    }
}

fn workload_from_name(name: &str) -> BenchmarkWorkload {
    BenchmarkWorkload::ALL
        .into_iter()
        .find(|w| workload_name(*w) == name)
        .unwrap_or_else(|| panic!("unknown workload name in golden: {name}"))
}

fn json_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected a JSON array: {value}"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("expected a JSON string: {v}"))
                .to_owned()
        })
        .collect()
}

// C#: BenchmarkProfileTests.ExpandedBenchmarkProfileRegistry
#[test]
fn expanded_benchmark_profile_registry() {
    assert!(
        all().len() >= 25,
        "registry has {} profiles, expected at least 25",
        all().len()
    );
    assert_eq!(all().len(), 30);
}

// C#: BenchmarkProfileTests.OriginalIsCorrectnessOnly
#[test]
fn original_is_correctness_only() {
    let count = all().iter().filter(|p| p.correctness_only).count();
    assert_eq!(count, 1, "exactly one correctness-only profile (original-correctness)");
    let original = find("original-correctness").expect("original-correctness is registered");
    assert!(original.correctness_only);
}

// C#: BenchmarkProfileTests.SpeedRankingExcludesCorrectnessOnlyProfile (registry half only; the ranking half is W7-01a/W7-03).
#[test]
fn speed_profiles_exclude_correctness_only() {
    let speed: Vec<&BenchmarkProfile> = all().iter().filter(|p| !p.correctness_only).collect();
    assert!(!speed.is_empty(), "speed profiles must exist");
    assert!(
        speed.iter().all(|p| p.id != "original-correctness"),
        "original-correctness must not be a speed profile"
    );
}

// C#: BenchmarkRunnableProfileTests.Runnable_ExcludesUnimplementedAndCorrectnessOnlyProfiles
#[test]
fn runnable_excludes_unimplemented_and_correctness_only_profiles() {
    let runnable = runnable();
    assert!(
        !runnable
            .iter()
            .any(|p| p.id == "explorer-reindex" || p.id == "cache-recovery"),
        "runnable must not contain unimplemented profiles"
    );
    assert!(
        !runnable.iter().any(|p| p.correctness_only),
        "runnable must not contain correctness-only profiles"
    );
    assert!(
        runnable.iter().any(|p| p.id == "recommended-auto"),
        "recommended-auto must be runnable"
    );
    let cache_recovery = find("cache-recovery").expect("cache-recovery is registered");
    assert!(
        is_not_implemented(cache_recovery),
        "cache-recovery must be not implemented"
    );
}

// new: the registry is the source of `find`; two profiles that differ only by case would make find ambiguous.
#[test]
fn registry_ids_are_unique_ignoring_case() {
    let keys: HashSet<String> = all().iter().map(|p| ignore_case_key(&p.id)).collect();
    assert_eq!(
        keys.len(),
        all().len(),
        "two profiles share an id under OrdinalIgnoreCase"
    );
}

// new: exact order and membership of the runnable list (note 5 of the contract).
#[test]
fn runnable_is_all_minus_three_in_order() {
    let ids: Vec<&str> = runnable().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids.len(), 27);
    assert_eq!(ids, RUNNABLE_IDS.to_vec());
}

// new: the checks W7-01a's `validate` performs, so a table typo cannot produce a profile that refuses to run.
// `validate` is not called here (not yet merged).
#[test]
fn every_profile_passes_validation_shape() {
    for p in all() {
        assert!(p.workers >= 1, "{}: workers must be >= 1, got {}", p.id, p.workers);
        assert!(
            p.next_window >= 0,
            "{}: next_window must be >= 0, got {}",
            p.id,
            p.next_window
        );
        assert!(
            p.previous_window >= 0,
            "{}: previous_window must be >= 0, got {}",
            p.id,
            p.previous_window
        );
        assert!(
            p.iterations >= 1,
            "{}: iterations must be >= 1, got {}",
            p.id,
            p.iterations
        );
    }
}

fn check_profile_fields(index: usize, want: &Value, got: &BenchmarkProfile) {
    let row = format!("row {index} ({})", got.id);
    assert_eq!(got.id, str_of(want, "id"), "{row}: id");
    assert_eq!(got.name, str_of(want, "name"), "{row}: name");
    assert_eq!(got.description, str_of(want, "description"), "{row}: description");
    assert_eq!(
        LenientEnum::name(got.loading_mode),
        str_of(want, "loading_mode"),
        "{row}: loading_mode"
    );
    assert_eq!(i64::from(got.workers), i64_of(want, "workers"), "{row}: workers");
    assert_eq!(
        i64::from(got.next_window),
        i64_of(want, "next_window"),
        "{row}: next_window"
    );
    assert_eq!(
        i64::from(got.previous_window),
        i64_of(want, "previous_window"),
        "{row}: previous_window"
    );
    assert_eq!(got.full_folder, bool_of(want, "full_folder"), "{row}: full_folder");
    assert_eq!(
        got.memory_reserve_bytes,
        i64_of(want, "memory_reserve_bytes"),
        "{row}: memory_reserve_bytes"
    );
    assert_eq!(got.disk_cache, bool_of(want, "disk_cache"), "{row}: disk_cache");
    assert_eq!(
        got.detailed_logging,
        bool_of(want, "detailed_logging"),
        "{row}: detailed_logging"
    );
    assert_eq!(workload_name(got.workload), str_of(want, "workload"), "{row}: workload");
    assert_eq!(
        i64::from(got.warmup_count),
        i64_of(want, "warmup_count"),
        "{row}: warmup_count"
    );
    assert_eq!(
        i64::from(got.iterations),
        i64_of(want, "iterations"),
        "{row}: iterations"
    );
    assert_eq!(
        got.correctness_only,
        bool_of(want, "correctness_only"),
        "{row}: correctness_only"
    );
}

fn check_registry(case: &Value) {
    let want_profiles = case["profiles"].as_array().expect("registry case has profiles");
    let registry = all();
    assert_eq!(want_profiles.len(), registry.len(), "registry length");
    for (index, (want, got)) in want_profiles.iter().zip(registry).enumerate() {
        check_profile_fields(index, want, got);
    }
    let runnable_ids: Vec<String> = runnable().iter().map(|p| p.id.clone()).collect();
    assert_eq!(runnable_ids, json_strings(&case["runnable_ids"]), "runnable ids");
    let not_implemented_ids: Vec<String> = all()
        .iter()
        .filter(|p| is_not_implemented(p))
        .map(|p| p.id.clone())
        .collect();
    assert_eq!(
        not_implemented_ids,
        json_strings(&case["not_implemented_ids"]),
        "not implemented ids"
    );
}

fn check_find(case: &Value) {
    let id = str_of(case, "id");
    let found = find(id);
    let found_id = found.map(|p| p.id.as_str());
    assert_eq!(found_id, case["found_id"].as_str(), "find({id:?}) found_id");
    let found_index = found.and_then(|p| all().iter().position(|q| ptr::eq(q, p)));
    let want_index = case["found_index"].as_u64().map(|n| n as usize);
    assert_eq!(found_index, want_index, "find({id:?}) found_index");
    let again = find(id);
    let same = match (found, again) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    };
    assert_eq!(
        same,
        bool_of(case, "same_instance_twice"),
        "find({id:?}) same instance twice"
    );
}

fn check_not_implemented(case: &Value) {
    let id = str_of(case, "id");
    let workload = workload_from_name(str_of(case, "workload"));
    // Same argument the oracle builds: `Find(id)`, or `All[0] with { Id = id }` when unknown, then `with { Workload = ... }`.
    let base = match find(id) {
        Some(p) => p.clone(),
        None => BenchmarkProfile {
            id: id.to_owned(),
            ..all()[0].clone()
        },
    };
    let argument = BenchmarkProfile { workload, ..base };
    assert_eq!(
        is_not_implemented(&argument),
        bool_of(case, "value"),
        "{}",
        str_of(case, "name")
    );
}

// Golden: every case of tests/golden/benchmark-profiles.json (registry, find, not_implemented).
#[test]
fn golden_benchmark_profiles() {
    let golden = golden::load("benchmark-profiles");
    for case in &golden.cases {
        match str_of(case, "kind") {
            "registry" => check_registry(case),
            "find" => check_find(case),
            "not_implemented" => check_not_implemented(case),
            other => panic!("{}: unknown kind {other}", str_of(case, "name")),
        }
    }
}
