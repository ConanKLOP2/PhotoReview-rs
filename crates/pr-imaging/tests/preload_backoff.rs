//! Red tests for W4-06b: port of `PhotoReview.Imaging/Preload/PreloadBusyBackoff.cs` and `Preload/PreviewSizeSampler.cs`
//! (C# baseline d92d6aae, PARITY I-11 "Busy back-off: 3 passes, at most 5 retries").
//!
//! Every test fails through `todo!()` in the stub until the W4-06b porter lands the bodies. The C# test each one ports is
//! named in its `// C#:` comment. Golden-driven tests replay `tests/golden-inputs/<name>.json` (the op scripts) against
//! `tests/golden/<name>.json` (the C# results, written by `tools/oracle-cs`).
//!
//! Not ported here: `PreloadDecoderBusyTests` (three tests through `PreloadScheduler`, W4-15).

use std::thread;

use pr_core::catalog::types::CatalogEntry;
use pr_core::time::DateTimeUtc;
use pr_imaging::cache_key::{ImageCacheKey, KeyOptions};
use pr_imaging::decode_types::DecodeBox;
use pr_imaging::preload_backoff::{
    COOLDOWN_PASSES, MAX_RETRIES, MINIMUM_SAMPLES, PreloadBusyBackoff, PreviewSizeSampler,
};
use pr_test_support::{FakeFileSystem, golden};
use serde_json::{Value, json};

/// C# `private const string Path = @"C:\busy\a.cr2"`.
const PATH: &str = r"C:\busy\a.cr2";
/// C# `private static readonly DateTime Written = new(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc)`.
const WRITTEN: &str = "2026-01-01T00:00:00Z";

/// `.NET` ticks (100 ns since 0001-01-01) of an ISO-8601 UTC midnight timestamp such as `2026-01-02T00:00:00Z`.
fn written_ticks(text: &str) -> i64 {
    const TICKS_PER_DAY: i64 = 864_000_000_000;
    /// Days from 0001-01-01 to 1970-01-01.
    const DAYS_0001_TO_EPOCH: i64 = 719_162;
    let (date, time) = text.split_once('T').expect("ISO date-time");
    assert_eq!(time, "00:00:00Z", "golden timestamps are at midnight UTC");
    let parts: Vec<i64> = date.split('-').map(|p| p.parse().expect("numeric date part")).collect();
    let &[year, month, day] = parts.as_slice() else {
        panic!("date must be YYYY-MM-DD: {text}");
    };
    // Howard Hinnant's `days_from_civil`: days since 1970-01-01 of a proleptic Gregorian date.
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_epoch = era * 146_097 + doe - 719_468;
    (days_since_epoch + DAYS_0001_TO_EPOCH) * TICKS_PER_DAY
}

/// C# `ImageCacheKey.Create(new CatalogEntry(path).WithMetadata(length, written), false, box)`. The entry carries its own
/// length and ticks, so the file system is never consulted (the fake is empty).
fn key_at(path: &str, length: i64, written: &str, decode_box: DecodeBox) -> ImageCacheKey {
    let fs = FakeFileSystem::new();
    let written = DateTimeUtc::from_ticks(written_ticks(written)).expect("valid ticks");
    let entry = CatalogEntry::new(path)
        .expect("test path is not blank")
        .with_metadata(length, written, None, None);
    ImageCacheKey::create_for_entry(&fs, &entry, false, decode_box, KeyOptions::default())
        .expect("entry key builds without the file system")
}

/// C# `Key(length)`: `PATH`, written `2026-01-01`, box `100x100`.
fn key(length: i64) -> ImageCacheKey {
    key_at(PATH, length, WRITTEN, DecodeBox::new(100, 100))
}

// C#: PreloadBusyBackoffTests.ShouldSkip_NeverBusyPath_IsFalse
#[test]
fn should_skip_never_busy_path_is_false() {
    assert!(!PreloadBusyBackoff::new().should_skip(PATH, &key(1)));
}

// C#: PreloadBusyBackoffTests.ShouldSkip_AfterBusy_SkipsExactlyTheCooldownPassesThenAllowsARetry
#[test]
fn should_skip_after_busy_skips_exactly_the_cooldown_passes_then_allows_a_retry() {
    let backoff = PreloadBusyBackoff::new();
    backoff.begin_pass();
    backoff.note_busy(PATH, &key(1));

    let mut skipped = Vec::new();
    for _ in 0..COOLDOWN_PASSES + 1 {
        backoff.begin_pass();
        skipped.push(backoff.should_skip(PATH, &key(1)));
    }

    assert_eq!(skipped, [true, true, true, false]);
}

// C#: PreloadBusyBackoffTests.ShouldSkip_AfterTheRetryCap_StaysSkippedWhateverThePass
#[test]
fn should_skip_after_the_retry_cap_stays_skipped_whatever_the_pass() {
    let backoff = PreloadBusyBackoff::new();
    for _ in 0..MAX_RETRIES + 1 {
        backoff.begin_pass();
        backoff.note_busy(PATH, &key(1));
    }

    for _ in 0..50 {
        backoff.begin_pass();
    }

    assert!(backoff.should_skip(PATH, &key(1)));
}

// C#: PreloadBusyBackoffTests.ShouldSkip_FileChangedAfterTheCap_StartsFresh
#[test]
fn should_skip_file_changed_after_the_cap_starts_fresh() {
    let backoff = PreloadBusyBackoff::new();
    for _ in 0..MAX_RETRIES + 1 {
        backoff.note_busy(PATH, &key(1));
    }

    // A different length/mtime: a new file identity.
    assert!(!backoff.should_skip(PATH, &key(2)));
    // And its busy count restarts, so it logs its first busy again.
    assert_eq!(backoff.note_busy(PATH, &key(2)), 1);
}

// C#: PreloadBusyBackoffTests.NoteBusy_ReturnsTheRunningCountPerPath
#[test]
fn note_busy_returns_the_running_count_per_path() {
    let backoff = PreloadBusyBackoff::new();

    let counts = [
        backoff.note_busy(PATH, &key(1)),
        backoff.note_busy(PATH, &key(1)),
        backoff.note_busy(PATH, &key(1)),
    ];

    assert_eq!(counts, [1, 2, 3]);
    assert_eq!(backoff.note_busy(r"C:\busy\b.cr2", &key(1)), 1);
}

// C#: PreloadBusyBackoffTests.RecordSuccess_AfterTheRetryCap_ClearsTheCapSoThePathIsNotSkipped
#[test]
fn record_success_after_the_retry_cap_clears_the_cap_so_the_path_is_not_skipped() {
    let backoff = PreloadBusyBackoff::new();
    for _ in 0..MAX_RETRIES + 1 {
        backoff.begin_pass();
        backoff.note_busy(PATH, &key(1));
    }
    for _ in 0..50 {
        backoff.begin_pass();
    }
    assert!(backoff.should_skip(PATH, &key(1)));

    backoff.record_success(PATH);

    assert!(!backoff.should_skip(PATH, &key(1)));
    // The count restarted.
    assert_eq!(backoff.note_busy(PATH, &key(1)), 1);
}

// C#: PreloadBusyBackoffTests.Clear_DropsTheStateOfEveryPath
#[test]
fn clear_drops_the_state_of_every_path() {
    let backoff = PreloadBusyBackoff::new();
    for _ in 0..MAX_RETRIES + 1 {
        backoff.note_busy(PATH, &key(1));
    }

    backoff.clear();

    assert!(!backoff.should_skip(PATH, &key(1)));
}

const GAP_PATH: &str = r"C:\busy\gap.cr2";

// C#: PreloadBusyBackoffGapTests.Clear_MidCooldown_ForgetsThePathAndRestartsItsBusyCount
#[test]
fn clear_mid_cooldown_forgets_the_path_and_restarts_its_busy_count() {
    let backoff = PreloadBusyBackoff::new();
    backoff.begin_pass();
    backoff.note_busy(GAP_PATH, &key_at(GAP_PATH, 1, WRITTEN, DecodeBox::new(100, 100)));
    backoff.begin_pass();
    // Cooling down.
    assert!(backoff.should_skip(GAP_PATH, &key_at(GAP_PATH, 1, WRITTEN, DecodeBox::new(100, 100))));

    backoff.clear();

    assert!(!backoff.should_skip(GAP_PATH, &key_at(GAP_PATH, 1, WRITTEN, DecodeBox::new(100, 100))));
    assert_eq!(
        backoff.note_busy(GAP_PATH, &key_at(GAP_PATH, 1, WRITTEN, DecodeBox::new(100, 100))),
        1
    );
}

// C#: PreloadBusyBackoffGapTests.BeginPassAndNoteBusy_Concurrent_LoseNoPassAndKeepPerPathCounts
// The C# test reads the private `_pass` field by reflection; `pass_for_tests()` is the Rust seam for that read.
#[test]
fn begin_pass_and_note_busy_concurrent_lose_no_pass_and_keep_per_path_counts() {
    const THREADS: usize = 8;
    const PER_THREAD: i32 = 20_000;
    let backoff = PreloadBusyBackoff::new();

    let counts: Vec<i32> = thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                let backoff = &backoff;
                scope.spawn(move || {
                    let path = format!(r"C:\busy\t{t}.cr2");
                    let key = key_at(&path, 1, WRITTEN, DecodeBox::new(100, 100));
                    let mut last = 0;
                    for _ in 0..PER_THREAD {
                        backoff.begin_pass();
                        last = backoff.note_busy(&path, &key);
                    }
                    last
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker thread")).collect()
    });

    // Each path's running count is exact.
    assert!(counts.iter().all(|&c| c == PER_THREAD), "counts: {counts:?}");
    // No lost increment.
    assert_eq!(backoff.pass_for_tests(), THREADS as i32 * PER_THREAD);
}

// C#: PreloadSmallGapTests.NoteBusy_KeyChange_RestartsTheCount
#[test]
fn note_busy_key_change_restarts_the_count() {
    let small_gap_path = r"C:\busy\small-gap.cr2";
    let backoff = PreloadBusyBackoff::new();
    let key_1 = key_at(small_gap_path, 1, WRITTEN, DecodeBox::new(100, 100));
    let key_2 = key_at(small_gap_path, 2, WRITTEN, DecodeBox::new(100, 100));

    assert_eq!(backoff.note_busy(small_gap_path, &key_1), 1);
    assert_eq!(backoff.note_busy(small_gap_path, &key_1), 2);
    assert_eq!(backoff.note_busy(small_gap_path, &key_2), 1);
    assert_eq!(backoff.note_busy(small_gap_path, &key_2), 2);
}

// C#: PreloadSmallGapTests.Record_NonPositiveBytes_AreIgnored
#[test]
fn record_non_positive_bytes_are_ignored() {
    let sampler = PreviewSizeSampler::new();
    let decode_box = DecodeBox::new(100, 100);
    for _ in 0..MINIMUM_SAMPLES - 1 {
        sampler.record(decode_box, 100);
    }
    sampler.record(decode_box, 0);
    sampler.record(decode_box, -50);
    // Zero and negative samples did not count towards the minimum.
    assert_eq!(sampler.mean_bytes(decode_box), None);

    sampler.record(decode_box, 100);

    assert_eq!(sampler.mean_bytes(decode_box), Some(100.0));
}

// C#: PreloadEstimateCalibrationTests.Sampler_NeedsMinimumSamples_AndRestartsOnBoxChange
#[test]
fn sampler_needs_minimum_samples_and_restarts_on_box_change() {
    let sampler = PreviewSizeSampler::new();
    let decode_box = DecodeBox::new(1000, 1000);
    for _ in 0..MINIMUM_SAMPLES - 1 {
        sampler.record(decode_box, 100);
    }
    assert_eq!(sampler.mean_bytes(decode_box), None);

    sampler.record(decode_box, 900);
    assert_eq!(sampler.mean_bytes(decode_box), Some((7.0 * 100.0 + 900.0) / 8.0));

    let other = DecodeBox::new(500, 500);
    sampler.record(other, 50);
    assert_eq!(sampler.mean_bytes(decode_box), None);
    assert_eq!(sampler.mean_bytes(other), None);
}

/// Loads `tests/golden-inputs/<name>.json` (the op scripts that produced `tests/golden/<name>.json`).
fn golden_inputs(name: &str) -> Vec<Value> {
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

/// Input case with `name` from a golden-inputs script.
fn input_case<'a>(inputs: &'a [Value], name: &str) -> &'a Value {
    inputs
        .iter()
        .find(|case| case["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("no input case named {name}"))
}

/// An `i32` field of an op, or `default` when absent.
fn int_of(op: &Value, field: &str, default: i32) -> i32 {
    op[field]
        .as_i64()
        .map_or(default, |v| i32::try_from(v).expect("box side fits i32"))
}

/// Number of repetitions of an op (`times`, default 1).
fn times_of(op: &Value) -> usize {
    op["times"]
        .as_u64()
        .map_or(1, |t| usize::try_from(t).expect("times fits usize"))
}

/// The cache key an op names: `path`, `length`, optional `written`, optional box (default 100x100).
fn key_of_op(op: &Value) -> ImageCacheKey {
    let path = op["path"].as_str().expect("op has a path");
    let length = op["length"].as_i64().expect("op has a length");
    let written = op["written"].as_str().unwrap_or(WRITTEN);
    let decode_box = DecodeBox::new(int_of(op, "box_width", 100), int_of(op, "box_height", 100));
    key_at(path, length, written, decode_box)
}

/// Runs one backoff op and returns its golden result: `null`, the busy count, or a bool.
fn run_backoff_op(backoff: &PreloadBusyBackoff, op: &Value) -> Value {
    match op["op"].as_str().expect("op name") {
        "begin_pass" => {
            for _ in 0..times_of(op) {
                backoff.begin_pass();
            }
            Value::Null
        }
        "note_busy" => json!(backoff.note_busy(op["path"].as_str().expect("path"), &key_of_op(op))),
        "should_skip" => json!(backoff.should_skip(op["path"].as_str().expect("path"), &key_of_op(op))),
        "record_success" => {
            backoff.record_success(op["path"].as_str().expect("path"));
            Value::Null
        }
        "clear" => {
            backoff.clear();
            Value::Null
        }
        other => panic!("unknown backoff op {other}"),
    }
}

// C#: PreloadBusyBackoffTests + PreloadBusyBackoffGapTests + PreloadSmallGapTests (golden script, 18 cases):
// every op result and the final pass counter must equal the C# oracle values.
#[test]
fn preload_busy_backoff_golden_matches_csharp() {
    let expected_cases = golden::load("preload-backoff-script").cases;
    let inputs = golden_inputs("preload-backoff-script");

    for expected in &expected_cases {
        let name = golden::str_of(expected, "name");
        let case = input_case(&inputs, name);
        let backoff = PreloadBusyBackoff::new();
        let ops = case["ops"].as_array().unwrap_or_else(|| panic!("case {name}: no ops"));

        let actual: Vec<Value> = ops.iter().map(|op| run_backoff_op(&backoff, op)).collect();

        let expected_results = expected["results"]
            .as_array()
            .unwrap_or_else(|| panic!("case {name}: no results"));
        assert_eq!(actual, *expected_results, "case {name}: op results");
        assert_eq!(
            i64::from(backoff.pass_for_tests()),
            golden::i64_of(expected, "pass"),
            "case {name}: final pass counter"
        );
    }
}

/// Runs one sampler op. `record` yields `None` (golden `null`); `mean` yields the mean or `None`.
fn run_sampler_op(sampler: &PreviewSizeSampler, op: &Value) -> Option<f64> {
    let decode_box = DecodeBox::new(int_of(op, "box_width", 0), int_of(op, "box_height", 0));
    match op["op"].as_str().expect("op name") {
        "record" => {
            let bytes = op["bytes"].as_i64().expect("record has bytes");
            for _ in 0..times_of(op) {
                sampler.record(decode_box, bytes);
            }
            None
        }
        "mean" => sampler.mean_bytes(decode_box),
        other => panic!("unknown sampler op {other}"),
    }
}

// C#: PreviewSizeSampler (golden script, 9 cases): every op result must equal the C# oracle value.
#[test]
fn preview_size_sampler_golden_matches_csharp() {
    let expected_cases = golden::load("preview-size-sampler-script").cases;
    let inputs = golden_inputs("preview-size-sampler-script");

    for expected in &expected_cases {
        let name = golden::str_of(expected, "name");
        let case = input_case(&inputs, name);
        let sampler = PreviewSizeSampler::new();
        let ops = case["ops"].as_array().unwrap_or_else(|| panic!("case {name}: no ops"));

        let actual: Vec<Option<f64>> = ops.iter().map(|op| run_sampler_op(&sampler, op)).collect();

        let expected_results: Vec<Option<f64>> = expected["results"]
            .as_array()
            .unwrap_or_else(|| panic!("case {name}: no results"))
            .iter()
            .map(Value::as_f64)
            .collect();
        assert_eq!(actual, expected_results, "case {name}");
    }
}
