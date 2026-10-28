//! Slice S3 of unit W8-20b (`contracts/W8-20b.md`): the whole-shell viewing tests that `W8-13i`, `W8-13j` and `W8-16g` leave to this unit. Baseline `d92d6aae`:
//! `tests/PhotoReview.Shell.Integration.Tests/Startup/ShellViewingTests.cs` (3 of 14), `Startup/ShellStalePreviewTests.cs` (1), `Viewing/ShellScrollBarEndToEndTests.cs` (5 of 7; the other two are 13i).
//!
//! Native (`#[ignore = "native: needs desktop"]`); the manual one is `#[ignore = "manual: perf report"]`. Remaining unported cases retain explicit pending/blocked ignore reasons.
#![cfg(windows)]
use pr_app::viewing::scrollbar_geometry::{self, ScrollBarBox, ScrollBarMetrics};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::ui_driver;
use std::sync::Mutex;
static SERIAL: Mutex<()> = Mutex::new(());

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{SmokeConfig, UiProbe, UiSmokeRun, isolated_test, vk};

/// C# `ShellViewingTests.Navigate_EachStepShowsThatImage_AndTheDrawnBufferIsTheCurrentOne` (line 84).
#[test]
#[ignore = "native: needs desktop"]
fn navigate_each_step_shows_that_image_and_the_drawn_buffer_is_the_current_one() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "navigate_each_step_shows_that_image_and_the_drawn_buffer_is_the_current_one",
        || {
            let run = UiSmokeRun::start(SmokeConfig {
                photo_count: 4,
                name_prefix: "p",
                ..Default::default()
            })
            .unwrap();
            run.wait_first_image().unwrap();
            let mut buffers = std::collections::BTreeSet::new();
            for index in 0..4 {
                if index > 0 {
                    run.key(vk::RIGHT).unwrap();
                }
                let snapshot = run
                    .wait_until("settled navigation", move |s| s.current_index == index && s.settled)
                    .unwrap();
                assert!(
                    snapshot
                        .current_path
                        .as_deref()
                        .unwrap()
                        .ends_with(&format!("p{index:02}.jpg"))
                );
                buffers.insert(snapshot.drawn_buffer_id.unwrap());
            }
            assert_eq!(buffers.len(), 4);
            assert!(run.snapshot().frames > 3);
            assert_eq!(
                run.on_ui(|p| p.main_window.composer().unwrap().crossfade().start_count())
                    .unwrap(),
                0
            );
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellViewingTests.RapidNavigation_OneHundredSteps_AlwaysEndsOnTheImageOfTheFinalIndex` (line 114).
#[test]
#[ignore = "native: needs desktop"]
fn rapid_navigation_one_hundred_steps_always_ends_on_the_image_of_the_final_index() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "rapid_navigation_one_hundred_steps_always_ends_on_the_image_of_the_final_index",
        || {
            let run = UiSmokeRun::start(SmokeConfig {
                photo_count: 6,
                name_prefix: "p",
                ..Default::default()
            })
            .unwrap();
            run.wait_first_image().unwrap();
            let mut seed = 12345u64;
            for i in 0..100 {
                run.key(if i % 7 < 4 { vk::RIGHT } else { vk::LEFT }).unwrap();
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                if seed % 3 == 0 {
                    run.on_ui(|_| ()).unwrap();
                }
            }
            let index = run.on_ui(|p| p.view_model.borrow().current_index()).unwrap();
            assert!((0..6).contains(&index));
            let snapshot = run
                .wait_until("final settled image (seed=12345)", |s| {
                    s.current_index == index && s.settled
                })
                .unwrap();
            assert!(
                snapshot
                    .current_path
                    .as_deref()
                    .unwrap()
                    .ends_with(&format!("p{index:02}.jpg")),
                "{}",
                run.diagnostics()
            );
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellViewingTests.RapidNavigation_WithRandomDecodeLatency_StillEndsOnTheImageOfTheFinalIndex` (line 150): the "random" latency is a seeded generator (the seed is in the failure message),
/// the delay is a gate the decoder waits on (no `sleep`).
#[test]
#[ignore = "native: pending bounded decoder gate fixture (W8-20b S3)"]
fn rapid_navigation_with_random_decode_latency_still_ends_on_the_image_of_the_final_index() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellStalePreviewTests.OlderPreviewFinishingAfterTheNewOne_NeverReplacesTheDrawnImage` (line 75): every decode waits behind a gate the test opens; the preview of the image the user left
/// finishes after the new one was drawn and must not reach the screen.
#[test]
#[ignore = "native: pending stale decoder completion observation (W8-20b S3)"]
fn older_preview_finishing_after_the_new_one_never_replaces_the_drawn_image() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

/// C# `ShellScrollBarEndToEndTests.DraggingTheVerticalThumb_ThroughRealMessages_ScrollsTheViewportProportionally` (line 71).
#[test]
#[ignore = "native: needs desktop"]
fn dragging_the_vertical_thumb_through_real_messages_scrolls_the_viewport_proportionally() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "dragging_the_vertical_thumb_through_real_messages_scrolls_the_viewport_proportionally",
        || {
            let run = zoomed_run();
            let (bar, dpi, hwnd) = run
                .on_ui(|p| (bar(p, true), p.main_window.window().dpi_scale(), p.hwnd))
                .unwrap();
            assert!(bar.thumb_visible && bar.max_offset > 0.0);
            let x = bar.thumb.x + bar.thumb.width / 2.0;
            let y = bar.thumb.y + bar.thumb.height / 2.0;
            let distance = (bar.track.height - bar.thumb.height) * 0.25;
            assert!(ui_driver::post_left_button(hwnd, px(x, dpi), px(y, dpi), true));
            assert!(ui_driver::post_mouse_move_with_buttons(
                hwnd,
                px(x, dpi),
                px(y + distance, dpi),
                true
            ));
            assert!(ui_driver::post_left_button(
                hwnd,
                px(x, dpi),
                px(y + distance, dpi),
                false
            ));
            let offset = run.on_ui(|p| p.viewport.vertical_offset()).unwrap();
            let expected = (bar.offset + bar.max_offset * 0.25).min(bar.max_offset);
            assert!(
                (offset - expected).abs() <= bar.max_offset * 0.02 + 2.0,
                "offset={offset} expected={expected}; {}",
                run.diagnostics()
            );
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellScrollBarEndToEndTests.DraggingTheHorizontalThumb_PastBothEnds_ClampsAtTheMaximumOffset` (line 104).
#[test]
#[ignore = "native: needs desktop"]
fn dragging_the_horizontal_thumb_past_both_ends_clamps_at_the_maximum_offset() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "dragging_the_horizontal_thumb_past_both_ends_clamps_at_the_maximum_offset",
        || {
            let run = zoomed_run();
            let (bar, dpi, hwnd) = run
                .on_ui(|p| (bar(p, false), p.main_window.window().dpi_scale(), p.hwnd))
                .unwrap();
            assert!(bar.thumb_visible && bar.max_offset > 0.0);
            let y = bar.thumb.y + bar.thumb.height / 2.0;
            let x = bar.thumb.x + bar.thumb.width / 2.0;
            assert!(ui_driver::post_left_button(hwnd, px(x, dpi), px(y, dpi), true));
            for (point, expected) in [
                (bar.track.x + bar.track.width + 200.0, bar.max_offset),
                (bar.track.x - 200.0, 0.0),
            ] {
                assert!(ui_driver::post_mouse_move_with_buttons(
                    hwnd,
                    px(point, dpi),
                    px(y, dpi),
                    true
                ));
                let offset = run.on_ui(|p| p.viewport.horizontal_offset()).unwrap();
                assert!((offset - expected).abs() < 1.0, "offset={offset} expected={expected}");
            }
            assert!(ui_driver::post_left_button(
                hwnd,
                px(bar.track.x - 200.0, dpi),
                px(y, dpi),
                false
            ));
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellScrollBarEndToEndTests.ClickingTheTrack_PagesByOneViewport_AndAPressOnTheBarNeverPansTheImage` (line 124).
#[test]
#[ignore = "native: needs desktop"]
fn clicking_the_track_pages_by_one_viewport_and_a_press_on_the_bar_never_pans_the_image() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "clicking_the_track_pages_by_one_viewport_and_a_press_on_the_bar_never_pans_the_image",
        || {
            let run = zoomed_run();
            run.on_ui(|p| p.viewport.scroll_home()).unwrap();
            let (bar, dpi) = run
                .on_ui(|p| (bar(p, true), p.main_window.window().dpi_scale()))
                .unwrap();
            assert!(bar.thumb_visible);
            let x = bar.thumb.x + bar.thumb.width / 2.0;
            let y = (bar.thumb.y + bar.thumb.height + bar.track.y + bar.track.height) / 2.0;
            run.click(px(x, dpi), px(y, dpi)).unwrap();
            let (h, v) = run
                .on_ui(|p| (p.viewport.horizontal_offset(), p.viewport.vertical_offset()))
                .unwrap();
            assert_eq!(h, 0.0);
            assert!(
                (v - bar.viewport_length.min(bar.max_offset)).abs() < 1.0,
                "vertical={v}, bar={bar:?}"
            );
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellScrollBarEndToEndTests.Zooming_ThroughTheWheel_ResizesTheThumb_AndTheFrameStillPresents` (line 207).
#[test]
#[ignore = "native: needs desktop"]
fn zooming_through_the_wheel_resizes_the_thumb_and_the_frame_still_presents() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    isolated_test(
        "zooming_through_the_wheel_resizes_the_thumb_and_the_frame_still_presents",
        || {
            let run = zoomed_run();
            let before = run.on_ui(|p| bar(p, true)).unwrap();
            let frame = run.snapshot().frames;
            run.wheel(120).unwrap();
            run.wait_until("wheel frame", |s| s.frames > frame).unwrap();
            let after = run.on_ui(|p| bar(p, true)).unwrap();
            assert!(
                after.thumb.height < before.thumb.height,
                "before={before:?}, after={after:?}"
            );
            assert!(run.snapshot().first_image_presented);
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellScrollBarEndToEndTests.ThumbDragLatency_IsMeasuredAtTheHandler_WhichAppliesTheOffsetSynchronously` (line 238): prints the numbers, asserts only that the offset is applied
/// synchronously inside the handler (the C# test is a report).
#[test]
#[ignore = "manual: perf report"]
fn thumb_drag_latency_is_measured_at_the_handler_which_applies_the_offset_synchronously() {
    todo!("W8-20b S3: contracts/W8-20b.md")
}

fn px(value: f64, dpi: f64) -> i32 {
    (value * dpi).round() as i32
}
fn bar(p: &UiProbe, vertical: bool) -> ScrollBarBox {
    let window = p.main_window.window();
    let bars = scrollbar_geometry::compute(
        window.client_size_dip(),
        &p.viewport.layout(),
        p.viewport.horizontal_offset(),
        p.viewport.vertical_offset(),
        &ScrollBarMetrics {
            thickness: p.viewport.scroll_bar_thickness(),
            ..Default::default()
        },
    );
    if vertical { bars.vertical } else { bars.horizontal }
}
fn zoomed_run() -> UiSmokeRun {
    let run = UiSmokeRun::start(SmokeConfig {
        fixture: "parity-640x480.jpg",
        ..Default::default()
    })
    .unwrap();
    run.wait_first_image().unwrap();
    // Enlarge relative to the actual Fit value, so both bars exist at every desktop DPI/client size.
    let fit = run.snapshot().zoom_percent;
    for _ in 0..12 {
        run.key(vk::ADD).unwrap();
    }
    run.wait_until("zoomed image", |s| !s.is_fit && s.zoom_percent > fit * 4.0)
        .unwrap();
    run.on_ui(|p| p.viewport.update_layout()).unwrap();
    run
}
