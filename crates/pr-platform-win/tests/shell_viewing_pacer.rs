//! Red tests (W8-16a, test porter) for `pr_platform_win::shell_viewing::pacer`: the port of
//! `PhotoReview.Shell.Win32/Viewing/FramePacer.cs` (baseline `d92d6aae`).
//!
//! Ported from `tests/PhotoReview.Shell.Tests/Viewing/FramePacerTests.cs` (7 tests, same names in snake_case, each with a
//! `// C#:` comment). The C# `Degraded` event is the `bool` returned by `observe_present` (decision D-16a-4): the event
//! fires on the one call that degrades, so the tests count those `true` returns.
//! Golden drivers over `tests/golden/frame-pacer.json` live in `shell_viewing_golden.rs`.
//!
//! Every test here fails through `todo!()` until the bodies land.

use pr_platform_win::shell_viewing::pacer::{BLOCK_THRESHOLD_MS, FramePacer, PresentMode};

// C#: FramePacerTests.ModeFor_IsImmediateExceptWhenFullscreenAndTheFirstImageIsDrawn (Theory, 4 rows)
#[test]
fn mode_for_is_immediate_except_fullscreen_non_first_image() {
    // (fullscreen, awaitingFirstImage, expectVSync)
    let rows: [(bool, bool, bool); 4] = [
        (false, false, false),
        (false, true, false),
        (true, true, false),
        (true, false, true),
    ];
    for (fullscreen, awaiting_first_image, expect_vsync) in rows {
        let expected = if expect_vsync {
            PresentMode::VSync
        } else {
            PresentMode::Immediate
        };
        assert_eq!(
            FramePacer::new().mode_for(fullscreen, awaiting_first_image),
            expected,
            "ModeFor({fullscreen}, {awaiting_first_image})"
        );
    }
}

// C#: FramePacerTests.ReadySwapChain_IsNeverDeferred
#[test]
fn ready_swap_chain_is_never_deferred() {
    let mut pacer = FramePacer::new();

    assert!(!pacer.should_defer(true));
    assert_eq!(pacer.deferred_total(), 0);
}

// C#: FramePacerTests.BusySwapChain_DefersAtMostThreeFramesInARowThenDrawsAnyway
#[test]
fn busy_swap_chain_defers_at_most_three_in_a_row() {
    let mut pacer = FramePacer::new();

    assert!(pacer.should_defer(false));
    assert!(pacer.should_defer(false));
    assert!(pacer.should_defer(false));
    assert!(!pacer.should_defer(false)); // a covered window never reports ready: draw anyway, no starvation
    assert!(pacer.should_defer(false)); // new cycle
    assert_eq!(pacer.deferred_total(), 4);
}

// C#: FramePacerTests.ReadyFrame_ResetsTheDeferralStreak
#[test]
fn ready_frame_resets_the_deferral_streak() {
    let mut pacer = FramePacer::new();
    assert!(pacer.should_defer(false));
    assert!(pacer.should_defer(false));

    assert!(!pacer.should_defer(true));
    assert!(pacer.should_defer(false));
    assert!(pacer.should_defer(false));
    assert!(pacer.should_defer(false));
    assert!(!pacer.should_defer(false));
}

// C#: FramePacerTests.VSyncThatBlocksTwiceInARow_DegradesToImmediateForTheSession
#[test]
fn vsync_blocking_twice_degrades_once_for_the_session() {
    let mut pacer = FramePacer::new();
    let mut raised = 0;

    raised += usize::from(pacer.observe_present(PresentMode::VSync, BLOCK_THRESHOLD_MS));
    assert!(!pacer.is_degraded());
    raised += usize::from(pacer.observe_present(PresentMode::VSync, 300.0));

    assert!(pacer.is_degraded());
    assert_eq!(raised, 1);
    assert_eq!(pacer.mode_for(true, false), PresentMode::Immediate);
    raised += usize::from(pacer.observe_present(PresentMode::VSync, 500.0));
    assert_eq!(raised, 1); // raised once only
}

// C#: FramePacerTests.AFastFrame_BreaksTheBlockedStreak
#[test]
fn fast_frame_breaks_the_blocked_streak() {
    let mut pacer = FramePacer::new();

    pacer.observe_present(PresentMode::VSync, 300.0);
    pacer.observe_present(PresentMode::VSync, BLOCK_THRESHOLD_MS - 0.1);
    pacer.observe_present(PresentMode::VSync, 300.0);

    assert!(!pacer.is_degraded());
}

// C#: FramePacerTests.ImmediateFrames_NeverCountTowardsDegrading
#[test]
fn immediate_frames_never_count_towards_degrading() {
    let mut pacer = FramePacer::new();

    pacer.observe_present(PresentMode::VSync, 300.0);
    pacer.observe_present(PresentMode::Immediate, 900.0); // measured in Immediate: unrelated to VSync
    pacer.observe_present(PresentMode::VSync, 300.0);

    assert!(!pacer.is_degraded());
}
