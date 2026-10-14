//! Ports of the W1-15 C# tests (`BenchmarkStatistics`, `DiagOptions`) plus the golden files `percentile` and
//! `diag-options`. Every DiagOptions test reads variables through `DiagOptions::from_lookup` over an in-test map:
//! Rust tests never touch the process environment (`set_var` is `unsafe` in edition 2024).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use pr_core::diagnostics::bench_stats::{PercentileOutOfRange, percentile};
use pr_core::diagnostics::diag_options::{
    DISABLE_DISK_CACHE_VAR, DiagOptions, DiagOptionsCache, FORCE_LOG_VAR, INSTANCE_LABEL_VAR, PRE_READ_VAR,
    PRELOAD_WORKERS_VAR,
};
use pr_test_support::golden::{self, bool_of, str_of};
use serde_json::Value;

// ---- helpers ----------------------------------------------------------------------------------------------------

/// A lookup over a fixed set of variables; every other name is unset.
fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    move |name: &str| map.get(name).cloned()
}

/// `DiagOptions` parsed from a fixed set of variables.
fn options(vars: &[(&str, &str)]) -> DiagOptions {
    DiagOptions::from_lookup(lookup(vars))
}

/// A parse-once read through `cache`, counting how often the loader ran.
fn cached_read(
    cache: &DiagOptionsCache,
    env: &RefCell<HashMap<String, String>>,
    loads: &Cell<u32>,
) -> Arc<DiagOptions> {
    cache.get_or_load(|| {
        loads.set(loads.get() + 1);
        DiagOptions::from_lookup(|name| env.borrow().get(name).cloned())
    })
}

/// A JSON golden number: a JSON number, or one of the strings the oracle writes for non-finite values.
fn golden_f64(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n
            .as_f64()
            .unwrap_or_else(|| panic!("golden number out of f64 range: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => other
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("golden number {other:?}: {e}")),
        },
        other => panic!("golden number expected, got {other}"),
    }
}

/// splitmix64: a tiny seeded PRNG for the oracle test (no dependency, same sequence on every run).
struct Prng(u64);

impl Prng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

// ---- BenchmarkStatistics.Percentile ------------------------------------------------------------------------------

// C#: BenchmarkStatisticsTests.PercentileEmptyCollectionReturnsZero
#[test]
fn percentile_empty_collection_returns_zero() {
    assert_eq!(percentile(&[], 0.5), Ok(0.0));
    // The empty check comes first, so the percentile itself is not looked at.
    assert_eq!(percentile(&[], 7.0), Ok(0.0));
    assert_eq!(percentile(&[], f64::NAN), Ok(0.0));
}

// C#: BenchmarkStatisticsTests.PercentileOutOfRangeThrowsArgumentOutOfRangeException
#[test]
fn percentile_out_of_range_throws_argument_out_of_range() {
    for p in [-0.01, 1.01] {
        assert_eq!(percentile(&[1.0, 2.0], p), Err(PercentileOutOfRange), "p = {p}");
    }
}

// C#: BenchmarkStatisticsTests.PercentileSingleValueReturnsThatValue
#[test]
fn percentile_single_value_returns_that_value() {
    assert_eq!(percentile(&[42.5], 0.95), Ok(42.5));
}

// C#: BenchmarkStatisticsTests.PercentileTwoValuesInterpolatesCorrectly
#[test]
fn percentile_two_values_interpolates_correctly() {
    assert_eq!(percentile(&[20.0, 10.0], 0.5), Ok(15.0));
    assert_eq!(percentile(&[10.0, 20.0], 0.0), Ok(10.0));
    assert_eq!(percentile(&[10.0, 20.0], 1.0), Ok(20.0));
}

// C#: BenchmarkStatisticsTests.PercentileKnownDistributionCalculatesP50AndP95
#[test]
fn percentile_known_distribution_calculates_p50_and_p95() {
    let values: Vec<f64> = (1..=100).map(f64::from).collect();
    assert_eq!(percentile(&values, 0.5), Ok(50.5));
    // C# Assert.Equal(95.05, p95, 2): equal to two decimal places.
    let p95 = percentile(&values, 0.95).expect("0.95 is in range");
    assert!((p95 - 95.05).abs() < 0.005, "p95 = {p95}");
}

// C#: BenchmarkStatisticsEdgeTests.Percentile_MatchesOracle
// Oracle: values are whole tenths (ties included), probabilities whole hundredths; the position
// `(n - 1) * h / 100` is computed in integers, so `lower = pos / 100` and `frac = (pos % 100) / 100` are exact.
#[test]
fn percentile_matches_oracle() {
    const HUNDREDTHS: [i64; 8] = [0, 1, 25, 50, 75, 95, 99, 100];
    let mut rng = Prng(4242);
    for iteration in 0..200 {
        let n = 1 + rng.below(59) as usize; // sizes 1..=59, as the C# `random.Next(1, 60)`
        let tenths: Vec<i64> = (0..n).map(|_| rng.below(1000) as i64).collect();
        let data: Vec<f64> = tenths.iter().map(|t| *t as f64 / 10.0).collect();
        let mut sorted = tenths;
        sorted.sort_unstable();
        let min = sorted[0] as f64 / 10.0;
        let max = sorted[n - 1] as f64 / 10.0;

        for h in HUNDREDTHS {
            let actual = percentile(&data, h as f64 / 100.0).expect("probability in range");

            let position_hundredths = (n as i64 - 1) * h;
            let lower = (position_hundredths / 100) as usize;
            let frac = (position_hundredths % 100) as f64 / 100.0;
            let upper = if position_hundredths % 100 == 0 {
                lower
            } else {
                lower + 1
            };
            let low = sorted[lower] as f64 / 10.0;
            let high = sorted[upper] as f64 / 10.0;
            let expected = low + (high - low) * frac;

            assert!(
                (actual - expected).abs() <= 1e-9,
                "iteration {iteration}, n = {n}, h = {h}: got {actual}, oracle {expected}"
            );
            assert!(
                min <= actual && actual <= max,
                "iteration {iteration}, n = {n}, h = {h}: {actual} outside range"
            );
        }
    }
}

// C#: BenchmarkStatisticsEdgeTests.Percentile_SmallSamples (6 rows)
#[test]
fn percentile_small_samples() {
    let rows: [(&[f64], f64, f64); 6] = [
        (&[], 0.95, 0.0),
        (&[5.0], 0.0, 5.0),
        (&[5.0], 1.0, 5.0),
        (&[2.0, 4.0], 0.99, 3.98),
        (&[4.0, 2.0], 0.5, 3.0),
        (&[3.0, 3.0, 3.0], 0.95, 3.0),
    ];
    for (values, p, expected) in rows {
        let actual = percentile(values, p).expect("probability in range");
        assert!(
            (actual - expected).abs() <= 1e-9,
            "values {values:?}, p {p}: got {actual}, expected {expected}"
        );
    }
}

// Golden `percentile` (oracle-cs percentile): every case. Results are compared by exact bits; a NaN result is
// compared with `is_nan` because the sign of a NaN is not part of the contract.
#[test]
fn percentile_matches_dotnet_golden() {
    let g = golden::load("percentile");
    for case in &g.cases {
        let values: Vec<f64> = case["values"]
            .as_array()
            .unwrap_or_else(|| panic!("case has no values: {case}"))
            .iter()
            .map(golden_f64)
            .collect();
        let p = golden_f64(&case["p"]);
        let result = percentile(&values, p);
        match case["threw"].as_str() {
            Some("ArgumentOutOfRangeException") => assert_eq!(result, Err(PercentileOutOfRange), "{case}"),
            Some(other) => panic!("unexpected exception in golden: {other}"),
            None => {
                let value = result.unwrap_or_else(|e| panic!("{case}: unexpected error {e}"));
                let bits =
                    u64::from_str_radix(str_of(case, "bits"), 16).unwrap_or_else(|e| panic!("{case}: bad bits: {e}"));
                let expected = f64::from_bits(bits);
                if expected.is_nan() {
                    assert!(value.is_nan(), "{case}: got {value}");
                } else {
                    assert_eq!(value.to_bits(), bits, "{case}: got {value}");
                }
            }
        }
    }
}

// ---- DiagOptions: tests/PhotoReview.Core.Tests/Diagnostics/DiagOptionsTests.cs -----------------------------------

// C#: DiagOptionsTests.Defaults_AreAllDisabled
#[test]
fn defaults_are_all_disabled() {
    let none = options(&[]);
    assert!(!none.pre_read);
    assert_eq!(none.preload_workers, None);
    assert!(!none.disable_disk_cache);
    assert!(!none.force_log);
    assert_eq!(none.instance_label, None);
    assert!(!none.any_enabled());
    assert_eq!(none.describe(), "(none)");
    assert_eq!(none, DiagOptions::default());
}

// C#: DiagOptionsTests.BooleanFlags_OnlyOneEnables (4 values x 3 variables)
#[test]
fn boolean_flags_only_one_enables() {
    let cases = [("1", true), ("0", false), ("true", false), ("", false)];
    for (value, expected) in cases {
        for name in [PRE_READ_VAR, DISABLE_DISK_CACHE_VAR, FORCE_LOG_VAR] {
            let opts = options(&[(name, value)]);
            let actual = match name {
                PRE_READ_VAR => opts.pre_read,
                DISABLE_DISK_CACHE_VAR => opts.disable_disk_cache,
                _ => opts.force_log,
            };
            assert_eq!(actual, expected, "{name}={value:?}");
            assert_eq!(opts.any_enabled(), expected, "{name}={value:?}");
        }
    }
}

// C#: DiagOptionsTests.SingleFlag_DoesNotLeakIntoOthers
#[test]
fn single_flag_does_not_leak_into_others() {
    let pre_read = options(&[(PRE_READ_VAR, "1")]);
    assert_eq!(
        (pre_read.pre_read, pre_read.disable_disk_cache, pre_read.force_log),
        (true, false, false)
    );

    let disk_cache = options(&[(DISABLE_DISK_CACHE_VAR, "1")]);
    assert_eq!(
        (disk_cache.pre_read, disk_cache.disable_disk_cache, disk_cache.force_log),
        (false, true, false)
    );

    let force_log = options(&[(FORCE_LOG_VAR, "1")]);
    assert_eq!(
        (force_log.pre_read, force_log.disable_disk_cache, force_log.force_log),
        (false, false, true)
    );
}

// C#: DiagOptionsTests.PreloadWorkers_Range (11 rows)
#[test]
fn preload_workers_range() {
    let rows: [(&str, Option<u32>); 11] = [
        ("0", Some(0)),
        ("1", Some(1)),
        ("8", Some(8)),
        ("16", Some(16)),
        (" 5 ", Some(5)),
        ("17", None),
        ("-1", None),
        ("100", None),
        ("abc", None),
        ("2.5", None),
        ("", None),
    ];
    for (raw, expected) in rows {
        let opts = options(&[(PRELOAD_WORKERS_VAR, raw)]);
        assert_eq!(opts.preload_workers, expected, "raw {raw:?}");
        assert_eq!(opts.any_enabled(), expected.is_some(), "raw {raw:?}");
    }
}

// C#: DiagOptionsTests.PreloadWorkersZero_IsEnabled
#[test]
fn preload_workers_zero_is_enabled() {
    let opts = options(&[(PRELOAD_WORKERS_VAR, "0")]);
    assert!(opts.any_enabled());
    assert_eq!(opts.describe(), "PRELOAD_WORKERS=0");
}

// C#: DiagOptionsTests.InstanceLabel_NullWhenBlank (4 rows)
#[test]
fn instance_label_null_when_blank() {
    let rows: [(&str, Option<&str>); 4] = [
        ("Agent run", Some("Agent run")),
        ("  padded  ", Some("  padded  ")),
        ("", None),
        ("   ", None),
    ];
    for (raw, expected) in rows {
        let opts = options(&[(INSTANCE_LABEL_VAR, raw)]);
        assert_eq!(opts.instance_label.as_deref(), expected, "raw {raw:?}");
        assert_eq!(opts.any_enabled(), expected.is_some(), "raw {raw:?}");
    }
}

// C#: DiagOptionsTests.Describe_ListsActiveFlagsInOrder
#[test]
fn describe_lists_active_flags_in_order() {
    let opts = options(&[
        (PRE_READ_VAR, "1"),
        (PRELOAD_WORKERS_VAR, "2"),
        (DISABLE_DISK_CACHE_VAR, "1"),
        (FORCE_LOG_VAR, "1"),
        (INSTANCE_LABEL_VAR, "lbl"),
    ]);
    assert_eq!(
        opts.describe(),
        "PREREAD=1;PRELOAD_WORKERS=2;DISABLE_DISKCACHE=1;FORCE_LOG=1;INSTANCE_LABEL=lbl"
    );
    assert!(opts.any_enabled());
}

// C#: DiagOptionsTests.Snapshot_IsStableUntilReset
#[test]
fn snapshot_is_stable_until_reset() {
    let env = RefCell::new(HashMap::from([(FORCE_LOG_VAR.to_owned(), "1".to_owned())]));
    let loads = Cell::new(0);
    let cache = DiagOptionsCache::new();

    assert!(cached_read(&cache, &env, &loads).force_log);

    env.borrow_mut().remove(FORCE_LOG_VAR);
    assert!(
        cached_read(&cache, &env, &loads).force_log,
        "the snapshot must not re-read before a reset"
    );
    assert_eq!(loads.get(), 1);

    cache.reset();
    assert!(!cached_read(&cache, &env, &loads).force_log);
    assert_eq!(loads.get(), 2);
}

// ---- DiagOptions: tests/PhotoReview.App.Tests/DiagOptionsTests.cs -----------------------------------------------

// C#: DiagOptionsTests.NoEnvironmentVariablesMeansDisabled
#[test]
fn no_environment_variables_means_disabled() {
    let opts = DiagOptions::from_lookup(|_| None);
    assert!(!opts.pre_read);
    assert_eq!(opts.preload_workers, None);
    assert!(!opts.disable_disk_cache);
    assert!(!opts.force_log);
    assert_eq!(opts.instance_label, None);
    assert!(!opts.any_enabled());
    assert_eq!(opts.describe(), "(none)");
}

// C#: DiagOptionsTests.InstanceLabelParsesNonBlankValues
#[test]
fn instance_label_parses_non_blank_values() {
    assert_eq!(options(&[]).instance_label, None);
    assert!(!options(&[]).any_enabled());

    let set = options(&[(INSTANCE_LABEL_VAR, "AGENT CHECK")]);
    assert_eq!(set.instance_label.as_deref(), Some("AGENT CHECK"));
    assert!(set.any_enabled());
    assert!(set.describe().contains("INSTANCE_LABEL=AGENT CHECK"));

    for blank in ["", "   "] {
        let opts = options(&[(INSTANCE_LABEL_VAR, blank)]);
        assert_eq!(opts.instance_label, None, "raw {blank:?}");
        assert!(!opts.any_enabled(), "raw {blank:?}");
    }
}

// C#: DiagOptionsTests.PreReadParsesOnlyExactly1
#[test]
fn pre_read_parses_only_exactly_1() {
    let on = options(&[(PRE_READ_VAR, "1")]);
    assert!(on.pre_read);
    assert!(on.any_enabled());
    assert!(on.describe().contains("PREREAD=1"));

    for invalid in ["true", "0", "yes", ""] {
        assert!(!options(&[(PRE_READ_VAR, invalid)]).pre_read, "raw {invalid:?}");
    }
}

// C#: DiagOptionsTests.PreloadWorkersParsesInRangeValues (3 rows)
#[test]
fn preload_workers_parses_in_range_values() {
    for (raw, expected) in [("0", 0), ("16", 16), ("4", 4)] {
        let opts = options(&[(PRELOAD_WORKERS_VAR, raw)]);
        assert_eq!(opts.preload_workers, Some(expected), "raw {raw:?}");
        assert!(opts.any_enabled());
        assert!(opts.describe().contains(&format!("PRELOAD_WORKERS={expected}")));
    }
}

// C#: DiagOptionsTests.ReadFromEnvironment_UnderArabicCulture_ParsesWorkers
// Rust has no ambient culture: the invariant rule is checked with the sign the Arabic culture would add to "+4".
#[test]
fn read_from_environment_under_arabic_culture_parses_workers() {
    let opts = options(&[(PRELOAD_WORKERS_VAR, "+4")]);
    assert_eq!(opts.preload_workers, Some(4));
    assert_eq!(opts.describe(), "PRELOAD_WORKERS=4");
}

// C#: DiagOptionsTests.PreloadWorkersRejectsInvalidValues (5 rows)
#[test]
fn preload_workers_rejects_invalid_values() {
    for raw in ["-1", "17", "abc", "", "2.5"] {
        let opts = options(&[(PRELOAD_WORKERS_VAR, raw)]);
        assert_eq!(opts.preload_workers, None, "raw {raw:?}");
        assert!(!opts.any_enabled(), "raw {raw:?}");
    }
}

// C#: DiagOptionsTests.DisableDiskCacheParsesOnlyExactly1
#[test]
fn disable_disk_cache_parses_only_exactly_1() {
    let on = options(&[(DISABLE_DISK_CACHE_VAR, "1")]);
    assert!(on.disable_disk_cache);
    assert!(on.any_enabled());
    assert!(on.describe().contains("DISABLE_DISKCACHE=1"));
    assert!(!options(&[(DISABLE_DISK_CACHE_VAR, "0")]).disable_disk_cache);
}

// C#: DiagOptionsTests.ForceLogParsesOnlyExactly1
#[test]
fn force_log_parses_only_exactly_1() {
    let on = options(&[(FORCE_LOG_VAR, "1")]);
    assert!(on.force_log);
    assert!(on.any_enabled());
    assert!(on.describe().contains("FORCE_LOG=1"));
    assert!(!options(&[(FORCE_LOG_VAR, "0")]).force_log);
}

// C#: DiagOptionsTests.DescribeAndAnyEnabledCombineAllFlags
#[test]
fn describe_and_any_enabled_combine_all_flags() {
    let opts = options(&[
        (PRE_READ_VAR, "1"),
        (PRELOAD_WORKERS_VAR, "2"),
        (DISABLE_DISK_CACHE_VAR, "1"),
        (FORCE_LOG_VAR, "1"),
        (INSTANCE_LABEL_VAR, "AGENT CHECK"),
    ]);
    assert!(opts.any_enabled());
    let description = opts.describe();
    for part in [
        "PREREAD=1",
        "PRELOAD_WORKERS=2",
        "DISABLE_DISKCACHE=1",
        "FORCE_LOG=1",
        "INSTANCE_LABEL=AGENT CHECK",
    ] {
        assert!(description.contains(part), "{description:?} lacks {part:?}");
    }
}

// C#: DiagOptionsTests.ResetForTestsRereadsEnvironment
#[test]
fn reset_for_tests_rereads_environment() {
    let env = RefCell::new(HashMap::new());
    let loads = Cell::new(0);
    let cache = DiagOptionsCache::new();

    assert!(!cached_read(&cache, &env, &loads).pre_read);
    env.borrow_mut().insert(PRE_READ_VAR.to_owned(), "1".to_owned());
    // Production reads the environment once: the stale value stays until a reset.
    assert!(!cached_read(&cache, &env, &loads).pre_read);
    cache.reset();
    assert!(cached_read(&cache, &env, &loads).pre_read);
}

// The process-wide snapshot is shared by every `current()` call until `reset_for_tests()`. Checked by `Arc` identity
// only, so the real process environment is never mutated. Guarded because tests run in parallel and share the static.
static PROCESS_SNAPSHOT_GUARD: Mutex<()> = Mutex::new(());

#[test]
fn process_wide_current_is_shared_until_reset() {
    let _guard = PROCESS_SNAPSHOT_GUARD.lock().unwrap_or_else(PoisonError::into_inner);
    DiagOptions::reset_for_tests();
    let first = DiagOptions::current();
    let second = DiagOptions::current();
    assert!(Arc::ptr_eq(&first, &second));

    DiagOptions::reset_for_tests();
    let third = DiagOptions::current();
    assert!(!Arc::ptr_eq(&first, &third));
}

// Golden `diag-options` (oracle-cs diag-options): every case. The golden is keyed by the C# variable names; they map to
// the Rust names by the prefix change (`PHOTOREVIEW_DIAG_*` -> `PHOTOREVIEW_RS_DIAG_*`, contracts/README.md D4).
#[test]
fn diag_options_matches_dotnet_golden() {
    let g = golden::load("diag-options");
    for case in &g.cases {
        let env = case["env"]
            .as_object()
            .unwrap_or_else(|| panic!("case has no env: {case}"));
        let mut vars: HashMap<String, String> = HashMap::new();
        for (name, value) in env {
            if let Some(text) = value.as_str() {
                vars.insert(
                    name.replacen("PHOTOREVIEW_DIAG_", "PHOTOREVIEW_RS_DIAG_", 1),
                    text.to_owned(),
                );
            }
        }
        let opts = DiagOptions::from_lookup(|name| vars.get(name).cloned());

        assert_eq!(opts.pre_read, bool_of(case, "pre_read"), "{case}");
        assert_eq!(
            opts.preload_workers.map(i64::from),
            case["preload_workers"].as_i64(),
            "{case}"
        );
        assert_eq!(opts.disable_disk_cache, bool_of(case, "disable_disk_cache"), "{case}");
        assert_eq!(opts.force_log, bool_of(case, "force_log"), "{case}");
        assert_eq!(
            opts.instance_label.as_deref(),
            case["instance_label"].as_str(),
            "{case}"
        );
        assert_eq!(opts.any_enabled(), bool_of(case, "any_enabled"), "{case}");
        assert_eq!(opts.describe(), str_of(case, "describe"), "{case}");
    }
}
