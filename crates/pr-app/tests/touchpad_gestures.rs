//! Red tests (W8-05e, test porter) for `pr_app::input::touchpad_gestures`. Port of
//! `tests/PhotoReview.App.Tests/Input/TouchpadGesturesTests.cs` at baseline `d92d6aae` (feature Q-TOUCHPAD-REFRESH):
//! classifier (6 tests), navigator (11), routing (1) and pan direction (1). Every `#[test]` fails through `todo!()` until the
//! bodies land. Theories are one `#[test]` with a loop over the C# rows, and each test carries a `// C#:` comment.
//!
//! Documented skips (not ported here):
//! - `TouchpadGesturesTests.WheelMessageSource_DecodesTheSignedDeltaAndTheScreenPoint`: W8-05a (`input::wheel_message`).
//! - `PointerInputControllerTests.Touchpad.cs`: W8-05g/h (B2), not in this file.
//!
//! Golden-driven checks of the same rules live in `golden_gestures.rs`.

use pr_app::input::mouse_gestures::WheelOutcomeKind;
use pr_app::input::touchpad_gestures::{
    GESTURE_GAP_MS, PAN_DIP_PER_DELTA_UNIT, TouchpadSwipeNavigator, TouchpadWheelAction, TouchpadWheelClassifier,
    WheelDeviceHint, WheelInput, is_within_gap, pan_offset_delta, route,
};

/// C# `Swipe(navigator, deltaPerMessage, messages, distance, startTime = 1000, stepMs = 8)`: the non-None outcomes.
fn swipe(
    navigator: &mut TouchpadSwipeNavigator,
    delta_per_message: i32,
    messages: i32,
    distance: i32,
    start_time: i32,
    step_ms: i32,
) -> Vec<WheelOutcomeKind> {
    let mut outcomes = Vec::new();
    for i in 0..messages {
        let outcome = navigator.handle(delta_per_message, start_time + i * step_ms, distance);
        if outcome != WheelOutcomeKind::None {
            outcomes.push(outcome);
        }
    }
    outcomes
}

/// `swipe` with the C# defaults (start 1000 ms, 8 ms per message).
fn swipe_default(
    navigator: &mut TouchpadSwipeNavigator,
    delta_per_message: i32,
    messages: i32,
    distance: i32,
) -> Vec<WheelOutcomeKind> {
    swipe(navigator, delta_per_message, messages, distance, 1000, 8)
}

// ---- classifier ----

#[test]
fn classifier_whole_notches_are_a_mouse_wheel() {
    // C#: TouchpadGesturesTests.Classifier_WholeNotches_AreAMouseWheel (theory: 120, -120, 240, -360)
    for delta in [120, -120, 240, -360] {
        let mut classifier = TouchpadWheelClassifier::new();
        assert!(
            !classifier.is_touchpad(delta, 1000, WheelDeviceHint::Unknown),
            "delta {delta} must be a mouse wheel"
        );
    }
}

#[test]
fn classifier_partial_notches_are_a_touchpad() {
    // C#: TouchpadGesturesTests.Classifier_PartialNotches_AreATouchpad (theory: 1, -7, 30, -119, 121)
    for delta in [1, -7, 30, -119, 121] {
        let mut classifier = TouchpadWheelClassifier::new();
        assert!(
            classifier.is_touchpad(delta, 1000, WheelDeviceHint::Unknown),
            "delta {delta} must be a touchpad"
        );
    }
}

#[test]
fn classifier_the_os_touchpad_hint_wins_even_for_a_whole_notch() {
    // C#: TouchpadGesturesTests.Classifier_TheOsTouchpadHint_WinsEvenForAWholeNotch
    let mut classifier = TouchpadWheelClassifier::new();

    assert!(classifier.is_touchpad(120, 1000, WheelDeviceHint::Touchpad));
}

#[test]
fn classifier_an_exact_notch_inside_a_touchpad_gesture_stays_touchpad_until_the_pause_exceeds_the_gap() {
    // C#: TouchpadGesturesTests.Classifier_AnExactNotchInsideATouchpadGesture_StaysTouchpad_UntilThePauseExceedsTheGap
    let gap = GESTURE_GAP_MS;
    let mut classifier = TouchpadWheelClassifier::new();
    assert!(classifier.is_touchpad(-13, 1000, WheelDeviceHint::Unknown));

    assert!(classifier.is_touchpad(-120, 1000 + gap, WheelDeviceHint::Unknown));
    // A pause longer than the gap: the next whole notch is the mouse again.
    assert!(!classifier.is_touchpad(-120, 1000 + 2 * gap + 1, WheelDeviceHint::Unknown));
    // ...and the mouse message ended the gesture: a later notch close behind it is still the mouse.
    assert!(!classifier.is_touchpad(-120, 1000 + 2 * gap + 2, WheelDeviceHint::Unknown));
}

#[test]
fn classifier_reset_ends_the_gesture() {
    // C#: TouchpadGesturesTests.Classifier_Reset_EndsTheGesture
    let mut classifier = TouchpadWheelClassifier::new();
    assert!(classifier.is_touchpad(-13, 1000, WheelDeviceHint::Unknown));

    classifier.reset();

    assert!(!classifier.is_touchpad(-120, 1001, WheelDeviceHint::Unknown));
}

#[test]
fn is_within_gap_uses_wrap_safe_elapsed_time() {
    // C#: TouchpadGesturesTests.IsWithinGap_UsesWrapSafeElapsedTime (theory: 5 rows, the last one is the TickCount wrap)
    let rows = [
        (1000, 1000, true),
        (1000, 1250, true),
        (1000, 1251, false),
        (1000, 999, false),
        (i32::MAX - 10, i32::MIN + 10, true),
    ];
    for (last, now, expected) in rows {
        assert_eq!(is_within_gap(last, now), expected, "last {last}, now {now}");
    }
}

// ---- navigator ----

#[test]
fn navigator_a_light_swipe_changes_exactly_one_image_after_half_the_distance() {
    // C#: TouchpadGesturesTests.Navigator_ALightSwipe_ChangesExactlyOneImage_AfterHalfTheDistance
    let mut navigator = TouchpadSwipeNavigator::new();

    // 9 x -10 = -90: below half of 200 -> nothing.
    assert!(swipe_default(&mut navigator, -10, 9, 200).is_empty());
    // the 10th message reaches -100 = half -> the next image.
    assert_eq!(navigator.handle(-10, 1000 + 9 * 8, 200), WheelOutcomeKind::Next);
}

#[test]
fn navigator_a_long_swipe_changes_one_image_per_full_distance_after_the_first() {
    // C#: TouchpadGesturesTests.Navigator_ALongSwipe_ChangesOneImagePerFullDistanceAfterTheFirst
    let mut navigator = TouchpadSwipeNavigator::new();

    // -720 in total: first image at -100, then -300, -500, -700 -> 4 images (not 7, not a runaway).
    let outcomes = swipe_default(&mut navigator, -10, 72, 200);

    assert_eq!(outcomes, vec![WheelOutcomeKind::Next; 4]);
    assert_eq!(navigator.gesture_total(), -720);
    assert_eq!(navigator.gesture_images(), 4);
}

#[test]
fn navigator_swiping_up_goes_to_the_previous_image() {
    // C#: TouchpadGesturesTests.Navigator_SwipingUp_GoesToThePreviousImage
    let mut navigator = TouchpadSwipeNavigator::new();

    // +300: images at +100 and +300
    let outcomes = swipe_default(&mut navigator, 10, 30, 200);

    assert_eq!(outcomes, vec![WheelOutcomeKind::Previous, WheelOutcomeKind::Previous]);
}

#[test]
fn navigator_a_bigger_distance_setting_needs_a_longer_swipe() {
    // C#: TouchpadGesturesTests.Navigator_ABiggerDistanceSetting_NeedsALongerSwipe
    let mut navigator = TouchpadSwipeNavigator::new();

    // -720: only the first image (at -300); the second needs -900.
    assert_eq!(swipe_default(&mut navigator, -10, 72, 600).len(), 1);
}

#[test]
fn navigator_one_huge_message_changes_one_image_and_queues_nothing() {
    // C#: TouchpadGesturesTests.Navigator_OneHugeMessage_ChangesOneImageAndQueuesNothing
    let mut navigator = TouchpadSwipeNavigator::new();

    assert_eq!(navigator.handle(-5000, 1000, 200), WheelOutcomeKind::Next);
    // The excess was dropped: the next small message of the same swipe does not fire another image.
    assert_eq!(navigator.handle(-10, 1008, 200), WheelOutcomeKind::None);
}

#[test]
fn navigator_a_remainder_below_a_full_step_is_kept_for_the_next_image() {
    // C#: TouchpadGesturesTests.Navigator_ARemainderBelowAFullStep_IsKeptForTheNextImage
    let mut navigator = TouchpadSwipeNavigator::new();

    assert_eq!(navigator.handle(-150, 1000, 200), WheelOutcomeKind::Next); // first step 100, remainder -50
    assert_eq!(navigator.handle(-140, 1008, 200), WheelOutcomeKind::None); // -190 < 200
    assert_eq!(navigator.handle(-10, 1016, 200), WheelOutcomeKind::Next); // -200 reached
}

#[test]
fn navigator_reversing_direction_starts_a_fresh_swipe() {
    // C#: TouchpadGesturesTests.Navigator_ReversingDirection_StartsAFreshSwipe
    let mut navigator = TouchpadSwipeNavigator::new();
    assert_eq!(navigator.handle(-100, 1000, 200), WheelOutcomeKind::Next);
    assert_eq!(navigator.handle(-90, 1008, 200), WheelOutcomeKind::None);

    // Reversed: the -90 partial is dropped and the fresh swipe's first image needs only half again.
    assert_eq!(navigator.handle(90, 1016, 200), WheelOutcomeKind::None);
    assert_eq!(navigator.handle(10, 1024, 200), WheelOutcomeKind::Previous);
}

#[test]
fn navigator_a_pause_longer_than_the_gap_starts_a_fresh_swipe() {
    // C#: TouchpadGesturesTests.Navigator_APauseLongerThanTheGap_StartsAFreshSwipe
    let gap = GESTURE_GAP_MS;
    let mut navigator = TouchpadSwipeNavigator::new();
    assert_eq!(navigator.handle(-100, 1000, 200), WheelOutcomeKind::Next);
    assert_eq!(navigator.handle(-90, 1008, 200), WheelOutcomeKind::None);

    // After the pause the -90 is forgotten, and the first image of the new swipe needs half (100), not a full 200.
    assert_eq!(navigator.handle(-90, 1008 + gap + 1, 200), WheelOutcomeKind::None);
    assert_eq!(navigator.handle(-10, 1008 + gap + 9, 200), WheelOutcomeKind::Next);
}

#[test]
fn navigator_within_the_gap_continues_the_same_swipe() {
    // C#: TouchpadGesturesTests.Navigator_WithinTheGap_ContinuesTheSameSwipe
    let gap = GESTURE_GAP_MS;
    let mut navigator = TouchpadSwipeNavigator::new();
    assert_eq!(navigator.handle(-100, 1000, 200), WheelOutcomeKind::Next);

    // Same swipe (exactly at the gap): the next image needs the full 200, not half.
    assert_eq!(navigator.handle(-150, 1000 + gap, 200), WheelOutcomeKind::None);
    assert_eq!(navigator.handle(-50, 1000 + gap + 8, 200), WheelOutcomeKind::Next);
}

#[test]
fn navigator_reset_forgets_the_partial_swipe() {
    // C#: TouchpadGesturesTests.Navigator_Reset_ForgetsThePartialSwipe
    let mut navigator = TouchpadSwipeNavigator::new();
    assert_eq!(navigator.handle(-90, 1000, 200), WheelOutcomeKind::None);

    navigator.reset();

    assert_eq!(navigator.handle(-90, 1008, 200), WheelOutcomeKind::None); // not -180
    assert_eq!(navigator.handle(-10, 1016, 200), WheelOutcomeKind::Next);
}

#[test]
fn navigator_zero_delta_does_nothing_and_a_non_positive_distance_is_treated_as_one() {
    // C#: TouchpadGesturesTests.Navigator_ZeroDelta_DoesNothing_AndANonPositiveDistanceIsTreatedAsOne
    let mut navigator = TouchpadSwipeNavigator::new();

    assert_eq!(navigator.handle(0, 1000, 200), WheelOutcomeKind::None);
    assert_eq!(navigator.handle(-1, 1008, 0), WheelOutcomeKind::Next);
}

// ---- new: hand-written extra tests (contract "Done when") ----

#[test]
fn navigator_zero_delta_still_refreshes_the_timestamp() {
    // new: trap 9 and Semantics 9(c). A zero-delta message keeps the swipe alive: it refreshes the timestamp even though it
    // changes nothing. Without the refresh the -10 at 1400 would be 400 ms after 1000 (outside the gap) and start fresh.
    let mut navigator = TouchpadSwipeNavigator::new();
    assert_eq!(navigator.handle(-90, 1000, 200), WheelOutcomeKind::None);
    assert_eq!(navigator.handle(0, 1200, 200), WheelOutcomeKind::None);

    // 200 ms after the zero message (inside the gap): the same swipe, sum -100 = half -> Next.
    assert_eq!(navigator.handle(-10, 1400, 200), WheelOutcomeKind::Next);
}

#[test]
fn navigator_round_half_even_first_step_for_distance_5() {
    // new: trap 1. Distance 5: the first step is round_half_even(2.5) = 2 (not 3), so -2 fires and -1 does not.
    let mut navigator = TouchpadSwipeNavigator::new();

    assert_eq!(navigator.handle(-2, 1000, 5), WheelOutcomeKind::Next);

    let mut navigator = TouchpadSwipeNavigator::new();
    assert_eq!(navigator.handle(-1, 1000, 5), WheelOutcomeKind::None);
    assert_eq!(navigator.handle(-1, 1008, 5), WheelOutcomeKind::Next); // -2 reached
}

#[test]
fn navigator_gesture_totals_survive_reset_until_the_next_fresh_swipe() {
    // new: Semantics 10. reset keeps gesture_total and gesture_images; the next fresh swipe clears them.
    let mut navigator = TouchpadSwipeNavigator::new();
    assert_eq!(navigator.handle(-30, 1000, 200), WheelOutcomeKind::None);
    assert_eq!(navigator.gesture_total(), -30);
    assert_eq!(navigator.gesture_images(), 0);

    navigator.reset();
    assert_eq!(navigator.gesture_total(), -30);
    assert_eq!(navigator.gesture_images(), 0);

    // After reset the message is fresh: the totals restart from this message alone.
    assert_eq!(navigator.handle(-10, 1008, 200), WheelOutcomeKind::None);
    assert_eq!(navigator.gesture_total(), -10);
    assert_eq!(navigator.gesture_images(), 0);
}

// ---- routing and pan direction ----

#[test]
fn route_follows_the_rules() {
    // C#: TouchpadGesturesTests.Route_FollowsTheRules (theory, 10 rows: horizontal, ctrl, enabled, isTouchpad, canPan, expected)
    let rows = [
        // ctrl (pinch): vertical -> mouse/zoom path, horizontal -> ignored
        (false, true, true, true, false, TouchpadWheelAction::MouseWheel),
        (true, true, true, true, false, TouchpadWheelAction::Ignore),
        // feature off
        (false, false, false, true, true, TouchpadWheelAction::MouseWheel),
        (true, false, false, true, true, TouchpadWheelAction::Ignore),
        // a notched mouse wheel
        (false, false, true, false, true, TouchpadWheelAction::MouseWheel),
        (true, false, true, false, true, TouchpadWheelAction::Ignore),
        // touchpad on a zoomed, pannable image: both axes pan
        (false, false, true, true, true, TouchpadWheelAction::Pan),
        (true, false, true, true, true, TouchpadWheelAction::Pan),
        // touchpad at Fit (or smaller): vertical navigates, horizontal is ignored
        (false, false, true, true, false, TouchpadWheelAction::Navigate),
        (true, false, true, true, false, TouchpadWheelAction::Ignore),
    ];
    for (horizontal, ctrl, enabled, is_touchpad, can_pan, expected) in rows {
        let input = WheelInput::new(-30, horizontal, ctrl, 0);
        assert_eq!(
            route(input, enabled, is_touchpad, can_pan),
            expected,
            "horizontal {horizontal}, ctrl {ctrl}, enabled {enabled}, touchpad {is_touchpad}, can_pan {can_pan}"
        );
    }
}

#[test]
fn pan_offset_delta_swipe_up_scrolls_towards_the_top_swipe_right_towards_the_right() {
    // C#: TouchpadGesturesTests.PanOffsetDelta_SwipeUpScrollsTowardsTheTop_SwipeRightTowardsTheRight
    let unit = PAN_DIP_PER_DELTA_UNIT;

    assert_eq!(
        pan_offset_delta(WheelInput::new(30, false, false, 0)),
        (0.0, -30.0 * unit)
    );
    assert_eq!(
        pan_offset_delta(WheelInput::new(-30, false, false, 0)),
        (0.0, 30.0 * unit)
    );
    assert_eq!(
        pan_offset_delta(WheelInput::new(30, true, false, 0)),
        (30.0 * unit, 0.0)
    );
    assert_eq!(
        pan_offset_delta(WheelInput::new(-30, true, false, 0)),
        (-30.0 * unit, 0.0)
    );
}

#[test]
fn pan_offset_delta_zero_is_positive_zero() {
    // new: trap 5 (Semantics 12). The vertical negation is done on the integer, so a zero delta gives +0.0, never -0.0.
    let (x, y) = pan_offset_delta(WheelInput::new(0, false, false, 0));
    assert_eq!(x.to_bits(), 0.0f64.to_bits());
    assert_eq!(y.to_bits(), 0.0f64.to_bits());
}
