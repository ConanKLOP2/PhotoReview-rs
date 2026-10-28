//! Slice S3 of unit W8-20b (`contracts/W8-20b.md`): the whole-shell viewing tests that `W8-13i`, `W8-13j` and `W8-16g` leave to this unit. Baseline `d92d6aae`:
//! `tests/PhotoReview.Shell.Integration.Tests/Startup/ShellViewingTests.cs` (3 of 14), `Startup/ShellStalePreviewTests.cs` (1), `Viewing/ShellScrollBarEndToEndTests.cs` (5 of 7; the other two are 13i).
//!
//! Native (`#[ignore = "native: needs desktop"]`); the manual one is `#[ignore = "manual: perf report"]`. Remaining unported cases retain explicit pending/blocked ignore reasons.
#![cfg(windows)]
use pr_app::viewing::scrollbar_geometry::{self, ScrollBarBox, ScrollBarMetrics};
use pr_app::viewmodels::viewer_state::calculate_step_zoom;
use pr_core::abstractions::NullLog;
use pr_core::model::enums::DecoderBackend;
use pr_imaging::decode_types::{DecodeRequest, DecodedImageView, ImageInfo};
use pr_imaging::decoder_traits::{DecodeError, ImageDecoder, ImageDecoderFactory};
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::ui_driver;
use pr_shell::composition::registry::DecoderFactoryWrapper;
use pr_shell::startup::graph::{GraphOverrides, production_graph_builder_with};
use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Instant;
static SERIAL: Mutex<()> = Mutex::new(());

#[path = "common/ui_smoke_fixture.rs"]
mod ui_smoke_fixture;

use ui_smoke_fixture::{BOUND, GraphChoice, SmokeConfig, UiProbe, UiSmokeRun, isolated_test, vk};

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
                // C# `CurrentPath(run)` is the PRESENTED path (the catalog moves before the image does), then `IsSettled`. The drawn buffer must also be one not seen before: the presenter
                // may name the new path a moment before the new pixels replace the old ones (the C# test awaited `NextAsync`, which returns after both).
                let suffix = format!("p{index:02}.jpg");
                let seen: Vec<i64> = buffers.iter().copied().collect();
                run.wait_ui_condition("settled navigation", move |p| {
                    presented_path_ends_with(p, &suffix)
                        && is_settled(p)
                        && p.main_window
                            .composer()
                            .and_then(|c| c.last_drawn())
                            .is_some_and(|(drawn, _, _)| !seen.contains(&drawn.id()))
                })
                .unwrap();
                let drawn = run
                    .on_ui(|p| p.main_window.composer().unwrap().last_drawn().unwrap().0.id())
                    .unwrap();
                buffers.insert(drawn);
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
            // The keys are POSTED: the index the shell must end on is computed from the steps (the catalog stops at both ends), not read while keys are still queued.
            let index = final_index(6, 100, |i| i % 7 < 4);
            run.wait_ui_condition("final settled image (seed=12345)", move |p| {
                p.view_model.borrow().current_index() == index
                    && p.view_model.borrow().presenter().is_idle()
                    && presented_path_ends_with(p, &format!("p{index:02}.jpg"))
                    && is_settled(p)
            })
            .unwrap();
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellViewingTests.RapidNavigation_WithRandomDecodeLatency_StillEndsOnTheImageOfTheFinalIndex` (line 150): the "random" latency is a seeded generator (the seed is in the failure message),
/// the delay is a gate the decoder waits on (no `sleep`): every decode blocks until the test releases it, in a seeded random order (completions arrive out of request order), and the rest are
/// released at the end.
#[test]
#[ignore = "native: needs desktop"]
fn rapid_navigation_with_random_decode_latency_still_ends_on_the_image_of_the_final_index() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    isolated_test(
        "rapid_navigation_with_random_decode_latency_still_ends_on_the_image_of_the_final_index",
        || {
            const SEED: u64 = 777;
            let gates = Arc::new(Gates::default());
            gates.open("p00.jpg"); // the first image is not part of the experiment
            let run = gated_run(&gates, 60);
            run.wait_first_image().unwrap();
            run.wait_ui_condition("first image settled", is_settled).unwrap();
            let mut seed = SEED;
            for i in 0..100 {
                run.key(if i % 5 < 4 { vk::RIGHT } else { vk::LEFT }).unwrap();
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                // 0..=2 decodes finish now, whichever is pending (not the oldest first).
                for _ in 0..(seed % 3) {
                    gates.release_pending((seed >> 8) as usize);
                }
            }
            gates.open_all();
            // The keys are POSTED: the index the shell must end on is computed from the steps (the catalog stops at both ends), not read while keys are still queued.
            let index = final_index(60, 100, |i| i % 5 < 4);
            run.wait_ui_condition(&format!("final image settled (seed={SEED})"), move |p| {
                p.view_model.borrow().current_index() == index
                    && p.view_model.borrow().presenter().is_idle()
                    && is_settled(p)
            })
            .unwrap();
            let (path, expected, drawn_is_current) = run
                .on_ui(|p| {
                    let vm = p.view_model.borrow();
                    let expected = vm.catalog().current().map(|e| e.path().to_owned());
                    let current = vm.current_image().and_then(|image| image.as_pixels());
                    let composer = p.main_window.composer().and_then(|c| c.current());
                    let same = matches!((current, composer), (Some(a), Some(b)) if Arc::ptr_eq(a, &b));
                    (
                        vm.presenter().current_presented_path().map(str::to_owned),
                        expected,
                        same,
                    )
                })
                .unwrap();
            assert_eq!(
                path.map(|p| p.to_lowercase()),
                expected.map(|p| p.to_lowercase()),
                "seed={SEED}"
            );
            assert!(drawn_is_current, "seed={SEED}");
            assert!(
                gates.started() > 10,
                "the gated decoder was not used: {}",
                gates.started()
            );
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// C# `ShellStalePreviewTests.OlderPreviewFinishingAfterTheNewOne_NeverReplacesTheDrawnImage` (line 75): every decode waits behind a gate the test opens; the preview of the image the user left
/// finishes after the new one was drawn and must not reach the screen.
#[test]
#[ignore = "native: needs desktop"]
fn older_preview_finishing_after_the_new_one_never_replaces_the_drawn_image() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    isolated_test(
        "older_preview_finishing_after_the_new_one_never_replaces_the_drawn_image",
        || {
            let gates = Arc::new(Gates::default());
            gates.open("p00.jpg"); // the first image is not part of the experiment
            let run = gated_run(&gates, 4);
            run.wait_first_image().unwrap();
            run.wait_ui_condition("first image settled", is_settled).unwrap();

            // Two navigations in a row: p01, then p02. Their decodes are both pending behind closed gates.
            for step in 1..=2 {
                run.key(vk::RIGHT).unwrap();
                run.wait_ui_condition("navigation applied", move |p| {
                    p.view_model.borrow().current_index() == step
                })
                .unwrap();
            }

            gates.open("p02.jpg"); // the NEW image finishes first and is drawn
            run.wait_ui_condition("new image drawn", |p| {
                p.view_model.borrow().presenter().is_idle() && is_settled(p)
            })
            .unwrap();
            let (shown_id, shown_path) = run
                .on_ui(|p| {
                    (
                        p.main_window.composer().unwrap().current().unwrap().id(),
                        p.view_model
                            .borrow()
                            .presenter()
                            .current_presented_path()
                            .map(str::to_owned),
                    )
                })
                .unwrap();
            assert!(shown_path.unwrap().to_lowercase().ends_with("p02.jpg"));

            gates.open("p01.jpg"); // the OLD image finishes late
            gates.wait_completed("p01.jpg");
            run.wait_ui_condition("old preview applied or dropped", |p| {
                p.view_model.borrow().presenter().is_idle() && is_settled(p)
            })
            .unwrap();
            let (current_id, drawn_id, path, index) = run
                .on_ui(|p| {
                    let composer = p.main_window.composer().unwrap();
                    (
                        composer.current().unwrap().id(),
                        composer.last_drawn().unwrap().0.id(),
                        p.view_model
                            .borrow()
                            .presenter()
                            .current_presented_path()
                            .map(str::to_owned),
                        p.view_model.borrow().current_index(),
                    )
                })
                .unwrap();
            assert_eq!(current_id, shown_id);
            assert_eq!(drawn_id, shown_id);
            assert!(path.unwrap().to_lowercase().ends_with("p02.jpg"));
            assert_eq!(index, 2);
            gates.open_all();
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
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
            let expected = (bar.offset + bar.max_offset * 0.25).min(bar.max_offset);
            let tolerance = bar.max_offset * 0.02 + 2.0;
            // The messages are POSTED (the C# test used `SendMessage`): wait until the UI thread applied them.
            run.wait_ui_condition("vertical thumb drag applied", move |p| {
                (p.viewport.vertical_offset() - expected).abs() <= tolerance
            })
            .unwrap();
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
                // POSTED messages (C# `SendMessage`): wait until the UI thread applied the move.
                run.wait_ui_condition("horizontal thumb clamp applied", move |p| {
                    (p.viewport.horizontal_offset() - expected).abs() < 1.0
                })
                .unwrap();
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
            // POSTED messages (C# `SendMessage`): wait until the page step was applied.
            let page = bar.viewport_length.min(bar.max_offset);
            run.wait_ui_condition("track click paged", move |p| {
                (p.viewport.vertical_offset() - page).abs() < 1.0
            })
            .unwrap();
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

/// C# `ShellScrollBarEndToEndTests.ThumbDragLatency_IsMeasuredAtTheHandler_WhichAppliesTheOffsetSynchronously` (line 238): 200 thumb moves alternating between two targets; each sample is the time
/// from posting the `WM_MOUSEMOVE` until the viewport offset shows the target (the C# test used `SendMessage`, which returns when the handler is done; a posted message adds the cross-thread
/// hop, so the numbers are an upper bound of the C# ones). The numbers go to stdout as `key=value`; the test asserts only that every move was applied (the C# `p95 < 50 ms` bound is a
/// hardware statement: the lead compares the printed numbers on an attended machine).
#[test]
#[ignore = "manual: perf report"]
fn thumb_drag_latency_is_measured_at_the_handler_which_applies_the_offset_synchronously() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    isolated_test(
        "thumb_drag_latency_is_measured_at_the_handler_which_applies_the_offset_synchronously",
        || {
            let run = zoomed_run();
            let (bar, dpi, hwnd) = run
                .on_ui(|p| (bar(p, true), p.main_window.window().dpi_scale(), p.hwnd))
                .unwrap();
            assert!(bar.thumb_visible && bar.max_offset > 0.0);
            let x = bar.thumb.x + bar.thumb.width / 2.0;
            let travel = (bar.track.height - bar.thumb.height).max(2.0);
            // The press point and the offset it started from; a sample is the time until the offset of the target position shows.
            let mut base_y = bar.thumb.y + bar.thumb.height / 2.0;
            let mut base_offset = bar.offset;
            assert!(ui_driver::post_left_button(hwnd, px(x, dpi), px(base_y, dpi), true));
            let mut samples = Vec::new();
            let mut re_presses = 0;
            for i in 0..200 {
                let target_y = base_y + (if i % 2 == 0 { 1.0 } else { 0.2 }) * travel / 2.0 - 1.0;
                let expected = (base_offset + (target_y - base_y) / travel * bar.max_offset).clamp(0.0, bar.max_offset);
                let tolerance = bar.max_offset * 0.02 + 2.0;
                let start = Instant::now();
                let posted = ui_driver::post_mouse_move_with_buttons(hwnd, px(x, dpi), px(target_y, dpi), true);
                assert!(posted, "move {i} refused: {}", std::io::Error::last_os_error());
                // A few immediate looks give the fine reading; after that the wait falls back to the rig tick (a frame cuts it short) so the UI thread's message queue is not flooded.
                let mut looks = 0;
                let applied = loop {
                    let applied = run
                        .on_ui(move |p| (p.viewport.vertical_offset() - expected).abs() <= tolerance)
                        .unwrap();
                    if applied || start.elapsed() > RE_PRESS_AFTER {
                        break applied;
                    }
                    looks += 1;
                    if looks > 8 {
                        run.wait_tick(start + BOUND);
                    }
                };
                if !applied {
                    // The drag was lost: press the thumb again where it is now, count it (printed), and measure this sample again. In about half of the runs a posted drag ends by itself after 50 to 80
                    // moves (cause not established, so neither the shell nor the rig was changed for it); the number is printed so the lead sees it.
                    re_presses += 1;
                    assert!(
                        re_presses <= 5,
                        "the drag was lost {re_presses} times: {}",
                        run.diagnostics()
                    );
                    assert!(ui_driver::post_left_button(hwnd, px(x, dpi), px(base_y, dpi), false));
                    let now = run.on_ui(|p| self::bar(p, true)).unwrap();
                    base_y = now.thumb.y + now.thumb.height / 2.0;
                    base_offset = now.offset;
                    assert!(ui_driver::post_left_button(hwnd, px(x, dpi), px(base_y, dpi), true));
                    continue;
                }
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            assert!(ui_driver::post_left_button(hwnd, px(x, dpi), px(base_y, dpi), false));
            samples.sort_by(f64::total_cmp);
            println!("thumb_drag_samples={}", samples.len());
            println!("thumb_drag_re_presses={re_presses}");
            println!("thumb_drag_p50_ms={:.3}", samples[samples.len() / 2]);
            println!("thumb_drag_p95_ms={:.3}", samples[samples.len() * 95 / 100]);
            println!("thumb_drag_max_ms={:.3}", samples[samples.len() - 1]);
            // C#: Assert.True(p95 < 50, ...) in ThumbDragLatency_IsMeasuredAtTheHandler_WhichAppliesTheOffsetSynchronously.
            assert!(
                samples[samples.len() * 95 / 100] < 50.0,
                "p95 {:.2} ms",
                samples[samples.len() * 95 / 100]
            );
            assert_eq!(run.close_and_wait(), Ok(0));
        },
    );
}

/// How long a thumb-drag sample may wait before the drag counts as lost.
const RE_PRESS_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

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
    // C# `ZoomToActualSize` makes a 4000x3000 image bigger than every window. The 640x480 fixture is smaller than the window at 100 %, so zoom in by keyboard steps instead and wait for the
    // zoom the steps must reach (a step adds `zoom_step` and starts from Fit), so both bars exist at every desktop DPI/client size and the layout is settled before the test reads it.
    let (fit, step) = run
        .on_ui(|p| {
            let viewer = p.view_model.borrow().viewer().clone();
            let viewer = viewer.borrow();
            (viewer.fit_zoom(), viewer.zoom_step())
        })
        .unwrap();
    let mut target = fit;
    for _ in 0..12 {
        run.key(vk::ADD).unwrap();
        target = calculate_step_zoom(target, step).unwrap_or(target);
    }
    run.wait_until("zoomed image", move |s| {
        !s.is_fit && (s.zoom_percent - target * 100.0).abs() < 0.01
    })
    .unwrap();
    run.on_ui(|p| p.viewport.update_layout()).unwrap();
    run.wait_ui_condition("both scroll bars visible", |p| {
        bar(p, true).thumb_visible && bar(p, false).thumb_visible
    })
    .unwrap();
    run
}

/// C# `CurrentPath(run).EndsWith(..)`: the path the presenter shows now (not the catalog's current entry, which moves first).
fn presented_path_ends_with(p: &UiProbe, suffix: &str) -> bool {
    p.view_model
        .borrow()
        .presenter()
        .current_presented_path()
        .is_some_and(|path| path.to_lowercase().ends_with(suffix))
}

/// `IsSettled` of the C# viewing tests, read live on the UI thread: nothing pending, the view model's image is the composer's current one and the one that was drawn.
fn is_settled(p: &UiProbe) -> bool {
    let vm = p.view_model.borrow();
    let Some(composer) = p.main_window.composer() else {
        return false;
    };
    let Some(pixels) = vm.current_image().and_then(|image| image.as_pixels()) else {
        return false;
    };
    composer.pending().is_none()
        && composer.current().is_some_and(|current| Arc::ptr_eq(pixels, &current))
        && composer
            .last_drawn()
            .is_some_and(|(drawn, _, _)| Arc::ptr_eq(pixels, &drawn))
}

/// A run over `count` photos (`p00.jpg`...) whose every decode first passes `gates`.
fn gated_run(gates: &Arc<Gates>, count: usize) -> UiSmokeRun {
    let gates = gates.clone();
    UiSmokeRun::start(SmokeConfig {
        photo_count: count,
        name_prefix: "p",
        graph: GraphChoice::Custom(Box::new(move |paths| {
            let wrapper: DecoderFactoryWrapper = Arc::new(move |inner| {
                Arc::new(GatedFactory {
                    inner,
                    gates: gates.clone(),
                })
            });
            production_graph_builder_with(GraphOverrides {
                paths: Some(paths.paths.clone()),
                recycle_bin: Some(paths.bin.clone()),
                log: Some(Arc::new(NullLog)),
                decoder_wrapper: Some(wrapper),
                ..Default::default()
            })
        })),
        ..Default::default()
    })
    .unwrap()
}

#[derive(Default)]
struct GateState {
    open: HashSet<String>,
    open_all: bool,
    next_ticket: u64,
    waiting: Vec<(u64, String)>,
    released: HashSet<u64>,
    started: usize,
    completed: HashSet<String>,
}

/// The C# `Gates`: a decode of a file blocks until its gate is open (`open`), until the test releases that very decode (`release_pending`), or until everything is open (`open_all`). Every wait is
/// bounded by [`BOUND`].
#[derive(Default)]
struct Gates {
    state: Mutex<GateState>,
    changed: Condvar,
}
impl Gates {
    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn open(&self, name: &str) {
        self.lock().open.insert(name.to_lowercase());
        self.changed.notify_all();
    }
    fn open_all(&self) {
        self.lock().open_all = true;
        self.changed.notify_all();
    }
    /// Releases one decode that is waiting right now (`pick` selects which, modulo the number waiting); `false` when none waits.
    fn release_pending(&self, pick: usize) -> bool {
        let mut state = self.lock();
        if state.waiting.is_empty() {
            return false;
        }
        let ticket = state.waiting[pick % state.waiting.len()].0;
        state.released.insert(ticket);
        drop(state);
        self.changed.notify_all();
        true
    }
    fn started(&self) -> usize {
        self.lock().started
    }
    fn wait_completed(&self, name: &str) {
        let deadline = Instant::now() + BOUND;
        let mut state = self.lock();
        while !state.completed.contains(&name.to_lowercase()) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "the decode of {name} did not finish");
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
    fn pass(&self, name: &str) {
        let deadline = Instant::now() + BOUND;
        let mut state = self.lock();
        let ticket = state.next_ticket;
        state.next_ticket += 1;
        state.started += 1;
        state.waiting.push((ticket, name.to_owned()));
        while !(state.open_all || state.open.contains(name) || state.released.remove(&ticket)) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        state.waiting.retain(|(t, _)| *t != ticket);
    }
    fn done(&self, name: &str) {
        self.lock().completed.insert(name.to_owned());
        self.changed.notify_all();
    }
}

struct GatedFactory {
    inner: Arc<dyn ImageDecoderFactory>,
    gates: Arc<Gates>,
}
impl ImageDecoderFactory for GatedFactory {
    fn create(&self, backend: DecoderBackend) -> Arc<dyn ImageDecoder> {
        Arc::new(GatedDecoder {
            inner: self.inner.create(backend),
            gates: self.gates.clone(),
        })
    }
    fn is_registered(&self, backend: DecoderBackend) -> bool {
        self.inner.is_registered(backend)
    }
}
struct GatedDecoder {
    inner: Arc<dyn ImageDecoder>,
    gates: Arc<Gates>,
}
impl ImageDecoder for GatedDecoder {
    fn decode(&self, request: &DecodeRequest) -> Result<Arc<dyn DecodedImageView>, DecodeError> {
        let name = request
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        self.gates.pass(&name);
        let result = self.inner.decode(request);
        self.gates.done(&name);
        result
    }
    fn read_info(&self, path: &str) -> Result<ImageInfo, DecodeError> {
        self.inner.read_info(path)
    }
}

/// The index after `steps` Right (`forward(i)`) or Left keys over `count` photos starting at 0; the catalog stops at both ends.
fn final_index(count: i32, steps: i32, forward: impl Fn(i32) -> bool) -> i32 {
    (0..steps).fold(0, |index, i| {
        if forward(i) {
            (index + 1).min(count - 1)
        } else {
            (index - 1).max(0)
        }
    })
}
