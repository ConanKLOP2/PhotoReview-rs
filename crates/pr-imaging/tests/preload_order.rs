//! Port of the C# preload tests `tests/PhotoReview.Imaging.Tests/PreloadOrderServiceTests.cs`,
//! `PreloadOrderPropertyTests.cs` and `PreloadWindowTests.cs` (C# baseline d92d6aae), plus golden checks against
//! `tests/golden/preload-order.json`, `preload-window.json` and `preload-options.json` (inputs in
//! `tests/golden-inputs/`, generated from .NET by `tools/oracle-cs`).
//!
//! Port of `PhotoReview.Imaging/PreloadWindow.cs`, `PreloadOrderService.cs` and `Preload/PreloadOptions.cs`.
//! Red tests (W4-05b): they compile against the stub and fail through `todo!()` until the implementation lands.
//!
//! Documented skips:
//! - `PreloadWindowTests.FromSettings_NullSettings_Throws`: a `&AppSettings` cannot be null (see the comment at the end).
//! - The property test uses a deterministic SplitMix64 stream instead of `new Random(5150)`: the .NET sequence is not
//!   part of parity, the priority invariants are.

use std::collections::HashSet;

use pr_core::settings::app_settings::AppSettings;
use pr_imaging::preload_order::{
    BACKWARD_LOOKAHEAD, FORWARD_LOOKAHEAD, PreloadOptions, PreloadWindow, build_preload_order,
};
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// Seed of the C# property test (`new Random(5150)`), reused as the seed of the Rust stream.
const PROPERTY_SEED: u64 = 5150;
/// C# `GroupOf` returns `int.MaxValue` for an index that is never queued.
const NOT_QUEUED: i32 = i32::MAX;

/// Deterministic SplitMix64 stream with the `Random.Next(lo, hi)` shape used by the C# property test.
struct Rng(u64);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 32) as u32
    }

    /// Uniform in `lo..hi` (like `Random.Next(lo, hi)`, `hi > lo`).
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let span = u32::try_from(hi - lo).expect("positive span");
        lo + i32::try_from(self.next_u32() % span).expect("span fits i32")
    }
}

/// Priority group of `offset` (signed distance in the direction of travel), or `-1` for the centre.
/// Independent classification used as the oracle of the property test.
fn group_of(offset: i32, lead: i32, full_folder: bool, forward: i32, backward: i32) -> i32 {
    if offset == 0 {
        return -1;
    }
    if offset > lead + forward {
        return if full_folder { 3 } else { NOT_QUEUED };
    }
    if offset > lead {
        return 0;
    }
    if offset > 0 {
        return 1;
    }
    if offset >= -backward {
        return 2;
    }
    if full_folder { 4 } else { NOT_QUEUED }
}

fn distinct_count(values: &[i32]) -> usize {
    values.iter().copied().collect::<HashSet<i32>>().len()
}

fn assert_priority_invariants(window: PreloadWindow) {
    let mut rng = Rng(PROPERTY_SEED);
    for iteration in 0..5_000 {
        let count = if rng.range(0, 4) == 0 {
            rng.range(0, 6)
        } else {
            rng.range(1, 300)
        };
        let center = rng.range(-2, count + 3);
        let direction = rng.range(-2, 3);
        let lead = if rng.range(0, 4) == 0 {
            rng.range(-5, 400)
        } else {
            rng.range(0, 30)
        };
        let full_folder = rng.range(0, 2) == 0;
        let context = format!(
            "iteration={iteration} count={count} center={center} dir={direction} lead={lead} full={full_folder} window={}/{}",
            window.forward, window.backward
        );

        let order = build_preload_order(center, count, full_folder, direction, lead, window);

        if !(0..count).contains(&center) {
            assert!(order.is_empty(), "out-of-range centre must queue nothing: {context}");
            continue;
        }

        let dir = if direction < 0 { -1 } else { 1 };
        let effective_lead = lead.clamp(0, count);
        assert!(
            order.iter().all(|&i| (0..count).contains(&i) && i != center),
            "index out of range or the centre itself: {context}"
        );
        assert_eq!(distinct_count(&order), order.len(), "duplicate index: {context}");

        let expected: HashSet<i32> = (0..count)
            .filter(|&i| {
                let group = group_of(
                    dir * (i - center),
                    effective_lead,
                    full_folder,
                    window.forward,
                    window.backward,
                );
                group >= 0 && group != NOT_QUEUED
            })
            .collect();
        let actual: HashSet<i32> = order.iter().copied().collect();
        assert_eq!(actual, expected, "wrong membership: {context}");

        let groups: Vec<i32> = order
            .iter()
            .map(|&i| {
                group_of(
                    dir * (i - center),
                    effective_lead,
                    full_folder,
                    window.forward,
                    window.backward,
                )
            })
            .collect();
        for (pos, pair) in groups.windows(2).enumerate() {
            assert!(
                pair[1] >= pair[0],
                "group order broken at position {}: {context}",
                pos + 1
            );
        }

        // Inside every group the nearest image comes first.
        for (pos, (indices, pair)) in order.windows(2).zip(groups.windows(2)).enumerate() {
            if pair[1] != pair[0] {
                continue;
            }
            let previous = (indices[0] - center).abs();
            let current = (indices[1] - center).abs();
            assert!(
                current > previous,
                "not nearest-first inside group {} at position {}: {context}",
                pair[1],
                pos + 1
            );
        }
    }
}

/// Input of one golden case set, from `tests/golden-inputs/<name>.json`.
fn golden_input_doc(name: &str) -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// `case[key]` as an i32; panics with the case printed when absent or out of range.
fn i32_of(case: &Value, key: &str) -> i32 {
    i32::try_from(i64_of(case, key)).unwrap_or_else(|_| panic!("'{key}' does not fit i32: {case}"))
}

// ---- PreloadOrderServiceTests -------------------------------------------------------------------------------------

// C#: PreloadOrderServiceTests.FullFolderPreloadPrioritizesNextThenPrior
#[test]
fn full_folder_preload_prioritizes_next_then_prior() {
    let order = build_preload_order(40, 100, true, 1, 0, PreloadWindow::DEFAULT);
    assert_eq!(order[0], 41);
    assert_eq!(order[31], 72);
    assert_eq!(order[32], 39);
    assert_eq!(distinct_count(&order), 99);
    assert!(!order.contains(&40));
}

// C#: PreloadOrderServiceTests.NavigatingChangesPreloadPriority
#[test]
fn navigating_changes_preload_priority() {
    let order = build_preload_order(44, 100, false, 1, 0, PreloadWindow::DEFAULT);
    assert_eq!(order[0], 45);
    assert_eq!(order[1], 46);
    assert!(order.contains(&43));
    assert_eq!(order.len(), 40);
    assert_eq!(distinct_count(&order), order.len());
}

// ---- PreloadOrderPropertyTests ------------------------------------------------------------------------------------

// C#: PreloadOrderPropertyTests.Build_SatisfiesPriorityInvariants
#[test]
fn build_satisfies_priority_invariants() {
    assert_priority_invariants(PreloadWindow::DEFAULT);
}

// C#: PreloadOrderPropertyTests.Build_SatisfiesPriorityInvariants_ForConfiguredWindow (InlineData 1, 0)
#[test]
fn build_satisfies_priority_invariants_for_configured_window_1_0() {
    assert_priority_invariants(PreloadWindow::new(1, 0));
}

// C#: PreloadOrderPropertyTests.Build_SatisfiesPriorityInvariants_ForConfiguredWindow (InlineData 5, 2)
#[test]
fn build_satisfies_priority_invariants_for_configured_window_5_2() {
    assert_priority_invariants(PreloadWindow::new(5, 2));
}

// C#: PreloadOrderPropertyTests.Build_SatisfiesPriorityInvariants_ForConfiguredWindow (InlineData 32, 8)
#[test]
fn build_satisfies_priority_invariants_for_configured_window_32_8() {
    assert_priority_invariants(PreloadWindow::new(32, 8));
}

// C#: PreloadOrderPropertyTests.Build_SatisfiesPriorityInvariants_ForConfiguredWindow (InlineData 100, 50)
#[test]
fn build_satisfies_priority_invariants_for_configured_window_100_50() {
    assert_priority_invariants(PreloadWindow::new(100, 50));
}

// C#: PreloadOrderPropertyTests.Build_DirectionAndLead_ExamplePositions
#[test]
fn build_direction_and_lead_example_positions() {
    let forward = FORWARD_LOOKAHEAD as usize;

    let next = build_preload_order(50, 200, false, 1, 0, PreloadWindow::DEFAULT);
    assert_eq!(next[0], 51);
    assert_eq!(next[forward - 1], 82);
    assert_eq!(next[forward], 49);

    let previous = build_preload_order(50, 200, false, -1, 0, PreloadWindow::DEFAULT);
    assert_eq!(previous[0], 49);
    assert_eq!(previous[forward - 1], 18);
    assert_eq!(previous[forward], 51);

    // Starts lead + 1 ahead; the skipped burst images follow the shifted window.
    let burst = build_preload_order(50, 200, false, 1, 3, PreloadWindow::DEFAULT);
    assert_eq!(burst[0], 54);
    assert_eq!(&burst[forward..forward + 3], &[51, 52, 53]);
}

// ---- PreloadWindowTests -------------------------------------------------------------------------------------------

// C#: PreloadWindowTests.Default_MatchesTheHistoricalConstants
#[test]
fn default_matches_the_historical_constants() {
    let window = PreloadWindow::DEFAULT;
    assert_eq!(window.forward, FORWARD_LOOKAHEAD);
    assert_eq!(window.backward, BACKWARD_LOOKAHEAD);
    assert_eq!(window.image_count(), 41);
}

// C#: PreloadWindowTests.ImageCount_IsForwardPlusBackwardPlusCurrent (InlineData 1, 0, 2)
#[test]
fn image_count_is_forward_plus_backward_plus_current_1_0() {
    assert_eq!(PreloadWindow::new(1, 0).image_count(), 2);
}

// C#: PreloadWindowTests.ImageCount_IsForwardPlusBackwardPlusCurrent (InlineData 3, 1, 5)
#[test]
fn image_count_is_forward_plus_backward_plus_current_3_1() {
    assert_eq!(PreloadWindow::new(3, 1).image_count(), 5);
}

// C#: PreloadWindowTests.ImageCount_IsForwardPlusBackwardPlusCurrent (InlineData 32, 8, 41)
#[test]
fn image_count_is_forward_plus_backward_plus_current_32_8() {
    assert_eq!(PreloadWindow::new(32, 8).image_count(), 41);
}

// C#: PreloadWindowTests.ImageCount_IsForwardPlusBackwardPlusCurrent (InlineData 500, 500, 1001)
#[test]
fn image_count_is_forward_plus_backward_plus_current_500_500() {
    assert_eq!(PreloadWindow::new(500, 500).image_count(), 1001);
}

// C#: PreloadWindowTests.FromSettings_ReadsForwardAndBackwardFromAppSettings
#[test]
fn from_settings_reads_forward_and_backward_from_app_settings() {
    let settings = AppSettings {
        preload_forward_count: 64,
        preload_backward_count: 16,
        ..AppSettings::default()
    };
    let window = PreloadWindow::from_settings(&settings);
    assert_eq!(window.forward, 64);
    assert_eq!(window.backward, 16);
}

// C#: PreloadWindowTests.FromSettings_Defaults_MatchPreloadWindowDefault
#[test]
fn from_settings_defaults_match_preload_window_default() {
    let window = PreloadWindow::from_settings(&AppSettings::default());
    assert_eq!(window, PreloadWindow::DEFAULT);
}

// C#: PreloadWindowTests.FromSettings_NullSettings_Throws
// Not ported (not applicable: reference type). A `&AppSettings` cannot be null, so there is no Rust twin.

// ---- Golden-driven tests ------------------------------------------------------------------------------------------

// Golden: tests/golden/preload-order.json (C#: PreloadOrderService.Build, oracle-cs preload-order; 37 cases).
// Inputs: tests/golden-inputs/preload-order.json. The whole `order` array is compared, in sequence.
#[test]
fn golden_preload_order() {
    let golden = golden::load("preload-order");
    let inputs = golden_input_doc("preload-order");
    let input_cases = inputs["cases"].as_array().expect("preload-order inputs: cases");
    assert_eq!(golden.cases.len(), 37);
    assert_eq!(input_cases.len(), golden.cases.len());

    for (case, input) in golden.cases.iter().zip(input_cases) {
        let name = str_of(case, "name");
        assert_eq!(str_of(input, "name"), name);
        let window = PreloadWindow::new(i32_of(input, "forward"), i32_of(input, "backward"));
        let actual = build_preload_order(
            i32_of(input, "center"),
            i32_of(input, "count"),
            bool_of(input, "full_folder"),
            i32_of(input, "direction"),
            i32_of(input, "lead"),
            window,
        );
        let expected: Vec<i32> = case["order"]
            .as_array()
            .expect("golden order array")
            .iter()
            .map(|v| i32::try_from(v.as_i64().expect("order entry is an integer")).expect("order entry fits i32"))
            .collect();
        assert_eq!(actual, expected, "case {name}");
    }
}

// Golden: tests/golden/preload-window.json (C#: PreloadWindow.ImageCount and PreloadWindow.FromSettings; oracle-cs
// preload-window). The first 9 rows are window rows, the last 4 are settings rows (`kind: from_settings`).
#[test]
fn golden_preload_window() {
    let golden = golden::load("preload-window");
    let inputs = golden_input_doc("preload-window");
    let settings_inputs = inputs["settings_cases"]
        .as_array()
        .expect("preload-window inputs: settings_cases");
    assert_eq!(golden.cases.len(), 13);
    let (window_rows, settings_rows) = golden.cases.split_at(9);
    assert_eq!(settings_rows.len(), settings_inputs.len());

    for case in window_rows {
        let name = str_of(case, "name");
        let window = PreloadWindow::new(i32_of(case, "forward"), i32_of(case, "backward"));
        assert_eq!(
            i64::from(window.image_count()),
            i64_of(case, "image_count"),
            "case {name}"
        );
    }

    for (case, input) in settings_rows.iter().zip(settings_inputs) {
        let name = str_of(case, "name");
        assert_eq!(str_of(input, "name"), name);
        assert_eq!(str_of(case, "kind"), "from_settings", "case {name}");

        let mut settings = AppSettings::default();
        if let Some(forward) = input.get("forward_count") {
            settings.preload_forward_count =
                i32::try_from(forward.as_i64().expect("forward_count integer")).expect("forward_count fits i32");
        }
        if let Some(backward) = input.get("backward_count") {
            settings.preload_backward_count =
                i32::try_from(backward.as_i64().expect("backward_count integer")).expect("backward_count fits i32");
        }

        let window = PreloadWindow::from_settings(&settings);
        assert_eq!(i64::from(window.forward), i64_of(case, "forward"), "case {name}");
        assert_eq!(i64::from(window.backward), i64_of(case, "backward"), "case {name}");
        assert_eq!(
            window == PreloadWindow::DEFAULT,
            bool_of(case, "equals_default"),
            "case {name}"
        );
    }
}

// Golden: tests/golden/preload-options.json (C#: PreloadOptions parameterless ctor and `with { Window = ... }`;
// oracle-cs preload-options; 3 cases). Inputs: tests/golden-inputs/preload-options.json.
#[test]
fn golden_preload_options() {
    let golden = golden::load("preload-options");
    let inputs = golden_input_doc("preload-options");
    let input_cases = inputs["cases"].as_array().expect("preload-options inputs: cases");
    assert_eq!(golden.cases.len(), 3);
    assert_eq!(input_cases.len(), golden.cases.len());

    for (case, input) in golden.cases.iter().zip(input_cases) {
        let name = str_of(case, "name");
        assert_eq!(str_of(input, "name"), name);
        let window = if input.get("forward").is_some() {
            PreloadWindow::new(i32_of(input, "forward"), i32_of(input, "backward"))
        } else {
            PreloadWindow::DEFAULT
        };
        let options = PreloadOptions {
            window,
            ..PreloadOptions::default()
        };

        assert_eq!(
            i64::from(options.worker_count),
            i64_of(case, "worker_count"),
            "case {name}"
        );
        assert_eq!(
            options.memory_load_limit,
            case["memory_load_limit"].as_f64().expect("memory_load_limit float"),
            "case {name}"
        );
        assert_eq!(options.reserve_bytes, i64_of(case, "reserve_bytes"), "case {name}");
        assert_eq!(
            options.full_folder_threshold_bytes,
            i64_of(case, "full_folder_threshold_bytes"),
            "case {name}"
        );
        assert_eq!(
            i64::from(options.window.forward),
            i64_of(case, "window_forward"),
            "case {name}"
        );
        assert_eq!(
            i64::from(options.window.backward),
            i64_of(case, "window_backward"),
            "case {name}"
        );
    }
}
