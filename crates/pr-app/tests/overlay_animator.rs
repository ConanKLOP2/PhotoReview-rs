//! Red tests (W8-15b, test porter) for `pr_app::overlay::animator`: opacity tweens driven by a host-owned frame clock.
//!
//! Port of `PhotoReview.Shell.Tests/Overlay/AnimatorTests.cs` (baseline `d92d6aae`, 22 cases) plus the golden
//! `tests/golden/overlay-animator.json` (27 cases: 25 scripts, 2 ease tables). Every test in this file fails through
//! `todo!()` until the porter fills the bodies of `overlay::animator`.
//!
//! Time convention: the C# fake clock ticks in milliseconds (`ticks_per_second = 1000`); durations are .NET `TimeSpan`
//! tick counts (100 ns), so 1 ms is `10_000` ticks. The Stopwatch half of the C# constructor test is not ported (contract O6).

use std::collections::BTreeMap;

use pr_app::input::frame_clock::FrameTick;
use pr_app::overlay::animator::{ease, Animator, AnimatorEffect, AnimatorError, OpacityStore};
use pr_app::overlay::contracts::Easing;
use pr_test_support::golden::{self, bool_of, i64_of, str_of};
use serde_json::Value;

/// One .NET `TimeSpan` millisecond in ticks (100 ns units).
const TICKS_PER_MS: i64 = 10_000;

/// Fake element table: the test's own `OpacityStore`. A missing key is a removed element (C# `resolve(id)` is `null`).
#[derive(Default)]
struct MapStore {
    values: BTreeMap<String, f32>,
}

impl MapStore {
    fn with(elements: &[(&str, f32)]) -> Self {
        let mut store = Self::default();
        for (id, value) in elements {
            store.insert(id, *value);
        }
        store
    }

    fn insert(&mut self, id: &str, value: f32) {
        self.values.insert(id.to_owned(), value);
    }

    fn remove(&mut self, id: &str) {
        self.values.remove(id);
    }

    fn get(&self, id: &str) -> f32 {
        self.values[id]
    }
}

impl OpacityStore for MapStore {
    fn opacity(&self, element_id: &str) -> Option<f32> {
        self.values.get(element_id).copied()
    }

    fn set_opacity(&mut self, element_id: &str, opacity: f32) {
        if let Some(value) = self.values.get_mut(element_id) {
            *value = opacity;
        }
    }
}

/// The C# fake clock frame: only the timestamp matters to the animator.
fn tick(timestamp: i64) -> FrameTick {
    FrameTick {
        timestamp,
        rendering_time_ms: 0.0,
        timing: None,
    }
}

/// `FadeTo` with the test's tick units, unwrapped (the animator is not disposed in these tests).
fn fade(
    animator: &mut Animator,
    store: &mut MapStore,
    element_id: &str,
    target: f32,
    duration_ticks: i64,
    easing: Easing,
) -> AnimatorEffect {
    animator
        .fade_to(store, element_id, target, duration_ticks, easing)
        .unwrap_or_else(|e| panic!("fade_to({element_id}) failed: {e}"))
}

/// Float comparison with an explicit tolerance (C# `Assert.Equal(expected, actual, precision)`).
fn assert_close(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected} +/- {tolerance}, got {actual}"
    );
}

/// The C# `Create()` helper: a 1000 ticks-per-second animator over two elements at full opacity.
fn create() -> (Animator, MapStore) {
    let animator = Animator::new(1000).expect("1000 ticks per second is valid");
    let store = MapStore::with(&[("Toolbar", 1.0), ("Status", 1.0)]);
    (animator, store)
}

// C#: AnimatorTests.FadeOut_Linear200ms_HitsExpectedOpacityAt0_50_100_200
#[test]
fn fade_out_linear_200ms_hits_expected_opacity_at_0_50_100_200() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        200 * TICKS_PER_MS,
        Easing::Linear,
    );
    assert!(animator.is_animating());

    animator.on_frame(&mut store, &tick(10_000));
    assert_eq!(store.get("Toolbar"), 1.0);
    animator.on_frame(&mut store, &tick(10_050));
    assert_close(store.get("Toolbar"), 0.75, 0.001);
    animator.on_frame(&mut store, &tick(10_100));
    assert_close(store.get("Toolbar"), 0.5, 0.001);
    animator.on_frame(&mut store, &tick(10_200));
    assert_eq!(store.get("Toolbar"), 0.0);
    assert!(!animator.is_animating());
}

// C#: AnimatorTests.FadeIn_150ms_QuadraticOut_FollowsEasing
#[test]
fn fade_in_150ms_quadratic_out_follows_easing() {
    let (mut animator, mut store) = create();
    store.insert("Status", 0.0);

    fade(
        &mut animator,
        &mut store,
        "Status",
        1.0,
        150 * TICKS_PER_MS,
        Easing::QuadraticOut,
    );
    animator.on_frame(&mut store, &tick(0));
    animator.on_frame(&mut store, &tick(75)); // t = 0.5 -> 1 - 0.25 = 0.75
    assert_close(store.get("Status"), 0.75, 0.001);
    animator.on_frame(&mut store, &tick(150));
    assert_eq!(store.get("Status"), 1.0);
}

// C#: AnimatorTests.NewFadeTo_CancelsOldTween_AndStartsFromCurrentOpacity
#[test]
fn new_fade_to_cancels_old_tween_and_starts_from_current_opacity() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        200 * TICKS_PER_MS,
        Easing::Linear,
    );
    animator.on_frame(&mut store, &tick(0));
    animator.on_frame(&mut store, &tick(100));
    assert_close(store.get("Toolbar"), 0.5, 0.001);

    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        1.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );
    assert_eq!(animator.active_count(), 1);
    animator.on_frame(&mut store, &tick(120)); // origin of the new tween
    assert_close(store.get("Toolbar"), 0.5, 0.001);
    animator.on_frame(&mut store, &tick(170)); // halfway
    assert_close(store.get("Toolbar"), 0.75, 0.001);
    animator.on_frame(&mut store, &tick(220));
    assert_eq!(store.get("Toolbar"), 1.0);
    assert!(!animator.is_animating());
    animator.on_frame(&mut store, &tick(400)); // the old tween must not write again
    assert_eq!(store.get("Toolbar"), 1.0);
}

// C#: AnimatorTests.SameTarget_WhileRunning_DoesNotRestartTween
#[test]
fn same_target_while_running_does_not_restart_tween() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        200 * TICKS_PER_MS,
        Easing::Linear,
    );
    animator.on_frame(&mut store, &tick(0));
    animator.on_frame(&mut store, &tick(100));

    // Same target while running: ignored, the duration is not used, no frame is requested.
    let again = fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        200 * TICKS_PER_MS,
        Easing::Linear,
    );
    assert!(!again.request_frame, "same target must not request a frame");
    animator.on_frame(&mut store, &tick(150));

    assert_close(store.get("Toolbar"), 0.25, 0.001);
}

// C#: AnimatorTests.AlreadyAtTarget_DoesNothingAndDoesNotListen
#[test]
fn already_at_target_does_nothing_and_does_not_listen() {
    let (mut animator, mut store) = create();

    let effect = fade(
        &mut animator,
        &mut store,
        "Toolbar",
        1.0,
        200 * TICKS_PER_MS,
        Easing::Linear,
    );

    assert!(!animator.is_animating());
    assert_eq!(effect, AnimatorEffect::default());
}

// C#: AnimatorTests.ZeroDuration_AppliesImmediately
#[test]
fn zero_duration_applies_immediately() {
    let (mut animator, mut store) = create();

    fade(&mut animator, &mut store, "Toolbar", 0.25, 0, Easing::Linear);

    assert_eq!(store.get("Toolbar"), 0.25);
    assert!(!animator.is_animating());
}

// C#: AnimatorTests.ZeroDuration_CancelsRunningTween
#[test]
fn zero_duration_cancels_running_tween() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        200 * TICKS_PER_MS,
        Easing::Linear,
    );
    animator.on_frame(&mut store, &tick(0));
    animator.on_frame(&mut store, &tick(50));

    fade(&mut animator, &mut store, "Toolbar", 1.0, 0, Easing::Linear);
    animator.on_frame(&mut store, &tick(100));

    assert_eq!(store.get("Toolbar"), 1.0);
    assert!(!animator.is_animating());
}

// C#: AnimatorTests.StopsListeningToTheClock_WhenNoTweenIsLeft
#[test]
fn stops_listening_when_no_tween_is_left() {
    let (mut animator, mut store) = create();
    let started = fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );
    assert!(animator.is_animating(), "C# subscribes to the clock: one subscriber");
    assert!(started.request_frame, "C# RequestFrame once");

    animator.on_frame(&mut store, &tick(0));
    animator.on_frame(&mut store, &tick(100));

    assert!(!animator.is_animating(), "C# unsubscribes: zero subscribers");
}

// C#: AnimatorTests.TwoElements_AnimateIndependently
#[test]
fn two_elements_animate_independently() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );
    fade(
        &mut animator,
        &mut store,
        "Status",
        0.5,
        200 * TICKS_PER_MS,
        Easing::Linear,
    );

    animator.on_frame(&mut store, &tick(0));
    animator.on_frame(&mut store, &tick(100));

    assert_eq!(store.get("Toolbar"), 0.0);
    assert_close(store.get("Status"), 0.75, 0.001);
    assert_eq!(animator.active_count(), 1);
}

// C#: AnimatorTests.UnknownElement_IsIgnored
#[test]
fn unknown_element_is_ignored() {
    let (mut animator, mut store) = create();

    let effect = fade(
        &mut animator,
        &mut store,
        "Nope",
        0.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );

    assert!(!animator.is_animating());
    assert_eq!(effect, AnimatorEffect::default());
}

// C#: AnimatorTests.ElementRemovedMidTween_EndsTheTween
#[test]
fn element_removed_mid_tween_ends_the_tween() {
    let mut animator = Animator::new(1000).expect("valid");
    let mut store = MapStore::with(&[("x", 1.0)]);
    fade(&mut animator, &mut store, "x", 0.0, 100 * TICKS_PER_MS, Easing::Linear);
    animator.on_frame(&mut store, &tick(0));

    store.remove("x");
    let effect = animator.on_frame(&mut store, &tick(50));

    assert!(!animator.is_animating());
    assert!(!effect.changed);
}

// C#: AnimatorTests.Changed_FiresOncePerFrameWithAChange
#[test]
fn changed_fires_once_per_frame_with_a_change() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );

    let mut changed = 0;
    for ts in [0, 50, 100] {
        if animator.on_frame(&mut store, &tick(ts)).changed {
            changed += 1;
        }
        if ts == 0 {
            assert_eq!(changed, 0, "opacity does not change at the origin");
        }
    }

    assert_eq!(changed, 2);
}

// C#: AnimatorTests.TargetIsClamped_AndNaNMeansZero
#[test]
fn target_is_clamped_and_nan_means_zero() {
    let (mut animator, mut store) = create();

    fade(&mut animator, &mut store, "Toolbar", 5.0, 0, Easing::Linear);
    assert_eq!(store.get("Toolbar"), 1.0);
    fade(&mut animator, &mut store, "Toolbar", f32::NAN, 0, Easing::Linear);
    assert_eq!(store.get("Toolbar"), 0.0);
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        -1.0,
        10 * TICKS_PER_MS,
        Easing::Linear,
    );
    assert!(!animator.is_animating());
}

// C#: AnimatorTests.TickBeforeStart_ClampsToStartValue_AndRealStopwatchFrequencyIsDefault
// The Stopwatch half (the default constructor reads `Stopwatch.Frequency`) is not ported: the host passes the frequency.
#[test]
fn tick_before_start_clamps_to_start_value() {
    let mut animator = Animator::new(1000).expect("valid");
    let mut store = MapStore::with(&[("x", 1.0)]);
    fade(&mut animator, &mut store, "x", 0.0, 200 * TICKS_PER_MS, Easing::Linear);

    animator.on_frame(&mut store, &tick(1_000_000));
    animator.on_frame(&mut store, &tick(1_000_000 - 5)); // time runs backwards: never above the start value
    assert_eq!(store.get("x"), 1.0);
    animator.on_frame(&mut store, &tick(1_000_000 + 100)); // 100 ms
    assert_close(store.get("x"), 0.5, 0.001);
    animator.on_frame(&mut store, &tick(1_000_000 + 200)); // 200 ms
    assert_eq!(store.get("x"), 0.0);
}

// C#: AnimatorTests.Dispose_StopsListeningAndRejectsNewFades
#[test]
fn dispose_stops_listening_and_rejects_new_fades() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );

    animator.dispose();

    assert!(!animator.is_animating());
    let result = animator.fade_to(&mut store, "Toolbar", 0.0, TICKS_PER_MS, Easing::Linear);
    assert_eq!(result, Err(AnimatorError::Disposed));
}

// C#: AnimatorTests.Ease_Values (theory, five rows)
#[test]
fn ease_values() {
    let rows = [
        (Easing::Linear, 0.25, 0.25),
        (Easing::Linear, 1.0, 1.0),
        (Easing::QuadraticOut, 0.0, 0.0),
        (Easing::QuadraticOut, 0.5, 0.75),
        (Easing::QuadraticOut, 1.0, 1.0),
    ];
    for (easing, t, expected) in rows {
        let actual = ease(easing, t);
        assert!(
            (actual - expected).abs() <= 1e-6,
            "ease({easing:?}, {t}) = {actual}, expected {expected}"
        );
    }
}

// C#: AnimatorTests.FadeEnd_LandsExactlyOnTarget_NotOnAFloatRoundedInterpolation
#[test]
fn fade_end_lands_exactly_on_target_not_on_a_rounded_interpolation() {
    // 0.02 + (0.1 - 0.02) in float rounds one ulp away from 0.1: the last frame must write the target itself.
    let (mut animator, mut store) = create();
    store.insert("Toolbar", 0.02);
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.1,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );

    animator.on_frame(&mut store, &tick(0));
    animator.on_frame(&mut store, &tick(100));

    assert_eq!(store.get("Toolbar"), 0.1_f32);
    assert!(!animator.is_animating());
}

// new: an on_frame call with no tween does nothing: no write, no change, no frame request.
#[test]
fn on_frame_without_tween_changes_nothing() {
    let (mut animator, mut store) = create();

    let effect = animator.on_frame(&mut store, &tick(500));

    assert_eq!(effect, AnimatorEffect::default());
    assert_eq!(store.get("Toolbar"), 1.0);
}

// new: only a call that creates a tween asks for a frame: zero duration and an already-at-target fade do not.
#[test]
fn fade_to_requests_a_frame_only_when_a_tween_is_created() {
    let (mut animator, mut store) = create();

    let zero = fade(&mut animator, &mut store, "Toolbar", 0.5, 0, Easing::Linear);
    let at_target = fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.5,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );
    let tween = fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );

    assert!(!zero.request_frame);
    assert!(!at_target.request_frame);
    assert!(tween.request_frame);
}

// new: extreme timestamps must not panic on the `now - start` subtraction (C# unchecked arithmetic, contract T6).
#[test]
fn wrapping_subtraction_on_extreme_timestamps_does_not_panic() {
    let mut animator = Animator::new(1000).expect("valid");
    let mut store = MapStore::with(&[("x", 1.0)]);
    fade(&mut animator, &mut store, "x", 0.0, 100 * TICKS_PER_MS, Easing::Linear);

    animator.on_frame(&mut store, &tick(i64::MAX)); // sets the start
    animator.on_frame(&mut store, &tick(i64::MIN)); // i64::MIN - i64::MAX overflows: must wrap, not panic

    let value = store.get("x");
    assert!((0.0..=1.0).contains(&value), "value {value} left 0..=1");
}

// new: dispose is idempotent and never writes an opacity.
#[test]
fn dispose_is_idempotent() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );

    animator.dispose();
    animator.dispose();

    assert!(!animator.is_animating());
    assert_eq!(store.get("Toolbar"), 1.0);
}

// new: cancel keeps the current opacity and emits neither a change nor a frame request.
#[test]
fn cancel_keeps_opacity_and_emits_no_change() {
    let (mut animator, mut store) = create();
    fade(
        &mut animator,
        &mut store,
        "Toolbar",
        0.0,
        100 * TICKS_PER_MS,
        Easing::Linear,
    );
    animator.on_frame(&mut store, &tick(0));
    animator.on_frame(&mut store, &tick(50));
    let before = store.get("Toolbar");

    animator.cancel("Toolbar");
    let effect = animator.on_frame(&mut store, &tick(100));

    assert!(!animator.is_animating());
    assert_eq!(store.get("Toolbar"), before);
    assert!(!effect.changed);
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden: tests/golden/overlay-animator.json (`oracle-cs overlay-animator`, C# d92d6aae).
// ---------------------------------------------------------------------------------------------------------------------

/// A golden number: a JSON number, or one of the oracle strings "NaN", "Infinity", "-Infinity", "-0", or decimal text.
fn num(value: &Value) -> f64 {
    match value {
        Value::Number(n) => n.as_f64().unwrap_or_else(|| panic!("not a float: {value}")),
        Value::String(text) => match text.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => other.parse::<f64>().unwrap_or_else(|_| panic!("not a number: {other}")),
        },
        other => panic!("not a number: {other}"),
    }
}

fn easing_of(name: &str) -> Easing {
    match name {
        "Linear" => Easing::Linear,
        "QuadraticOut" => Easing::QuadraticOut,
        other => panic!("unknown easing {other}"),
    }
}

fn run_ease_case(case: &Value) {
    let name = str_of(case, "name");
    let easing = easing_of(str_of(case, "easing"));
    let inputs = case["t"].as_array().unwrap_or_else(|| panic!("{name}: no 't'"));
    let values = case["values"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: no 'values'"));
    assert_eq!(inputs.len(), values.len(), "{name}: t and values differ in length");
    for (t, expected) in inputs.iter().zip(values) {
        let want = num(expected);
        let got = ease(easing, num(t));
        assert_eq!(
            got,
            want,
            "{name}: ease({}, {t}) differs from C#",
            str_of(case, "easing")
        );
    }
}

fn run_script_case(case: &Value) {
    let name = str_of(case, "name");
    let ticks_per_second = i64_of(case, "ticks_per_second");
    let mut animator = Animator::new(ticks_per_second).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut store = MapStore::default();
    for (id, value) in case["elements"].as_object().expect("elements") {
        store.insert(id, num(value) as f32);
    }
    let ops = case["ops"].as_array().expect("ops");
    let results = case["results"].as_array().expect("results");
    assert_eq!(ops.len(), results.len(), "{name}: ops and results differ in length");

    for (index, (op, expected)) in ops.iter().zip(results).enumerate() {
        let step = format!("{name} op #{index} ({})", str_of(op, "op"));
        let mut effect = AnimatorEffect::default();
        let mut threw: Option<&str> = expected.get("threw").and_then(Value::as_str);

        match str_of(op, "op") {
            "fade" => {
                let id = str_of(op, "id");
                let target = num(&op["target"]) as f32;
                let easing = easing_of(str_of(op, "easing"));
                match animator.fade_to(&mut store, id, target, i64_of(op, "duration_ticks"), easing) {
                    Ok(e) => effect = e,
                    Err(error) => {
                        assert_eq!(error, AnimatorError::Disposed, "{step}");
                        assert_eq!(threw, Some("ObjectDisposedException"), "{step}: unexpected error");
                        threw = None; // consumed: the error was the expected one
                    }
                }
            }
            "tick" => effect = animator.on_frame(&mut store, &tick(i64_of(op, "timestamp"))),
            "remove" => store.remove(str_of(op, "id")),
            "restore" => store.insert(str_of(op, "id"), num(&op["opacity"]) as f32),
            "set" => store.set_opacity(str_of(op, "id"), num(&op["opacity"]) as f32),
            "cancel" => animator.cancel(str_of(op, "id")),
            "dispose" => animator.dispose(),
            other => panic!("{name}: unknown op {other}"),
        }
        assert!(
            threw.is_none(),
            "{step}: expected a thrown {threw:?} that did not happen"
        );

        let watched = expected["opacity"].as_object().expect("opacity");
        for (id, want) in watched {
            let got = store.opacity(id).map(f64::from);
            let want = if want.is_null() { None } else { Some(num(want)) };
            assert_eq!(got, want, "{step}: opacity of {id}");
        }
        assert_eq!(
            animator.is_animating(),
            bool_of(expected, "is_animating"),
            "{step}: is_animating"
        );
        assert_eq!(
            animator.active_count() as i64,
            i64_of(expected, "active_count"),
            "{step}: active_count"
        );
        assert_eq!(
            animator.is_animating(),
            bool_of(expected, "listening"),
            "{step}: listening"
        );
        assert_eq!(
            effect.request_frame,
            bool_of(expected, "request_frame"),
            "{step}: request_frame"
        );
        assert_eq!(effect.changed, bool_of(expected, "changed"), "{step}: changed");
    }
}

#[test]
fn golden_overlay_animator() {
    let golden = golden::load("overlay-animator");
    assert_eq!(golden.generator, "oracle-cs overlay-animator");
    assert_eq!(golden.cases.len(), 27, "overlay-animator golden has 27 cases");
    for case in &golden.cases {
        match str_of(case, "kind") {
            "script" => run_script_case(case),
            "ease" => run_ease_case(case),
            other => panic!("unknown kind {other}"),
        }
    }
}
