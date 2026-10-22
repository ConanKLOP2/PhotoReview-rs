//! Slice S3 of unit W8-20b (`contracts/W8-20b.md`): the whole-shell viewing tests that `W8-13i`, `W8-13j` and `W8-16g` leave to this unit. Baseline `d92d6aae`:
//! `tests/PhotoReview.Shell.Integration.Tests/Startup/ShellViewingTests.cs` (3 of 14), `Startup/ShellStalePreviewTests.cs` (1), `Viewing/ShellScrollBarEndToEndTests.cs` (5 of 7; the other two are 13i).
//!
//! Native (`#[ignore = "native: needs desktop"]`); the manual one is `#[ignore = "manual: perf report"]`. STUB: the bodies are `todo!()`.
#![cfg(windows)]
#![allow(dead_code, unused_imports)] // stub

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{OpenTarget, SmokeConfig, UiSmokeRun, vk};

/// C# `ShellViewingTests.Navigate_EachStepShowsThatImage_AndTheDrawnBufferIsTheCurrentOne` (line 84).
#[test]
#[ignore = "native: needs desktop"]
fn navigate_each_step_shows_that_image_and_the_drawn_buffer_is_the_current_one() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellViewingTests.RapidNavigation_OneHundredSteps_AlwaysEndsOnTheImageOfTheFinalIndex` (line 114).
#[test]
#[ignore = "native: needs desktop"]
fn rapid_navigation_one_hundred_steps_always_ends_on_the_image_of_the_final_index() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellViewingTests.RapidNavigation_WithRandomDecodeLatency_StillEndsOnTheImageOfTheFinalIndex` (line 150): the "random" latency is a seeded generator (the seed is in the failure message),
/// the delay is a gate the decoder waits on (no `sleep`).
#[test]
#[ignore = "native: needs desktop"]
fn rapid_navigation_with_random_decode_latency_still_ends_on_the_image_of_the_final_index() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellStalePreviewTests.OlderPreviewFinishingAfterTheNewOne_NeverReplacesTheDrawnImage` (line 75): every decode waits behind a gate the test opens; the preview of the image the user left
/// finishes after the new one was drawn and must not reach the screen.
#[test]
#[ignore = "native: needs desktop"]
fn older_preview_finishing_after_the_new_one_never_replaces_the_drawn_image() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellScrollBarEndToEndTests.DraggingTheVerticalThumb_ThroughRealMessages_ScrollsTheViewportProportionally` (line 71).
#[test]
#[ignore = "native: needs desktop"]
fn dragging_the_vertical_thumb_through_real_messages_scrolls_the_viewport_proportionally() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellScrollBarEndToEndTests.DraggingTheHorizontalThumb_PastBothEnds_ClampsAtTheMaximumOffset` (line 104).
#[test]
#[ignore = "native: needs desktop"]
fn dragging_the_horizontal_thumb_past_both_ends_clamps_at_the_maximum_offset() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellScrollBarEndToEndTests.ClickingTheTrack_PagesByOneViewport_AndAPressOnTheBarNeverPansTheImage` (line 124).
#[test]
#[ignore = "native: needs desktop"]
fn clicking_the_track_pages_by_one_viewport_and_a_press_on_the_bar_never_pans_the_image() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellScrollBarEndToEndTests.Zooming_ThroughTheWheel_ResizesTheThumb_AndTheFrameStillPresents` (line 207).
#[test]
#[ignore = "native: needs desktop"]
fn zooming_through_the_wheel_resizes_the_thumb_and_the_frame_still_presents() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellScrollBarEndToEndTests.ThumbDragLatency_IsMeasuredAtTheHandler_WhichAppliesTheOffsetSynchronously` (line 238): prints the numbers, asserts only that the offset is applied
/// synchronously inside the handler (the C# test is a report).
#[test]
#[ignore = "manual: perf report"]
fn thumb_drag_latency_is_measured_at_the_handler_which_applies_the_offset_synchronously() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}
