//! Lane `st-mainwin-tests` (ST-GAPMAP2 section 6, items 7 and 8): window-level tests of `MainWindow` that the earlier rounds left open.
//!
//! - the 7 end-to-end scroll-bar tests of `ShellScrollBarEndToEndTests` (real window messages through the real handler chain, real viewport controller);
//! - a minimised window (`is_minimized`, `restore`, a frame with a zero client size);
//! - `update_retained` (observed through the GPU cache's retained set), `on_compare_changed`, `on_navigation_changed` (observed through the prefetcher's
//!   direction memory), and `MainWindow::prefetch_host()` (`path_at` range over a real catalog).
//!
//! Every test is `#[ignore = "native: needs desktop"]`: a real UI thread, a hidden shell window (the minimise test shows it minimised, without activating it)
//! and a WARP offscreen surface. No sleeps; every wait is bounded or a direct call. Run with `--include-ignored --test-threads=1`.
//!
//! Deviation from the C# test list: `Zooming_ThroughTheWheel_...` sends Ctrl+wheel to the real input chain, which this fixture does not register (the wheel
//! to zoom path belongs to the shell-input tests). The Rust twin zooms the viewer through its public API and checks the same bar consequences.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/main_window_viewing_fixture.rs"]
mod main_window_viewing_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
mod support {
    #[path = "../../../pr-app/tests/support/main_env.rs"]
    pub mod main_env;
    #[path = "../../../pr-app/tests/support/presenter_fakes.rs"]
    pub mod presenter_fakes;
    #[path = "../../../pr-app/tests/support/preview_fakes.rs"]
    pub mod preview_fakes;
}
#[path = "common/ui_thread.rs"]
mod ui_thread;

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use main_window_fixture::{BuildOptions, Built, build, finish, run, start_ui, take_built};
use main_window_viewing_fixture::{built_with_renderer, solid};
use pr_app::viewing::scrollbar_geometry::offset_for_thumb_start;
use pr_app::viewmodels::compare::CompareProperty;
use pr_app::viewmodels::main::{ChangeSet, MainProperty};
use pr_imaging::decode_types::PlatformImage;
use pr_imaging::pixels::PixelBuffer;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::shell_viewing::prefetcher::{LookupRequest, RetryRequest};
use pr_platform_win::win32::api::user32;
use pr_platform_win::win32::messages::{
    SW_SHOWMINNOACTIVE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

const MK_LBUTTON: usize = 1;
/// The bitmap is bigger than the 640 x 480 DIP window at 100 %, so both bars appear.
const IMAGE_SIDE: i32 = 1500;

fn pack(x: i32, y: i32) -> isize {
    (((y as u32 & 0xFFFF) << 16) | (x as u32 & 0xFFFF)) as isize
}

fn to_pixels(dip_x: f64, dip_y: f64, dpi: f64) -> (i32, i32) {
    ((dip_x * dpi).round() as i32, (dip_y * dpi).round() as i32)
}

fn send(b: &Built, message: u32, wparam: usize, x: i32, y: i32) {
    shell_window_fixture::send(b.shell.hwnd(), message, wparam, pack(x, y));
}

/// A window with its renderer, a `IMAGE_SIDE` square bitmap handled by the composer and the viewer told about the source size; still in Fit mode. The layout
/// is brought up to date synchronously (no posted job to wait for).
fn with_image() -> Rc<Built> {
    let b = built_with_renderer();
    let image = solid(IMAGE_SIDE, 9);
    b.window
        .view_model()
        .borrow()
        .viewer()
        .borrow_mut()
        .set_source_size(IMAGE_SIDE, IMAGE_SIDE, true);
    b.hub.raise(Some(&PlatformImage::Pixels(image)), true);
    b.window.apply_changes(&ChangeSet::default());
    b.window.viewport().update_fit_size();
    b.window.viewport().update_layout();
    b
}

/// C# `ZoomToActualSize` of the end-to-end tests: 100 %, layout done, both bars visible.
fn zoom_to_actual_size(b: &Built) {
    let update = b
        .window
        .view_model()
        .borrow()
        .viewer()
        .borrow_mut()
        .zoom_to_actual_size();
    b.window.apply_viewer_changes(update.changes);
    b.window.viewport().update_layout();
    let layout = b.window.scroll_bars().expect("registered").current_layout();
    assert!(
        layout.vertical.visible && layout.horizontal.visible,
        "both bars after the zoom to 100 %: {layout:?}"
    );
}

// ---- the 7 end-to-end bar tests (ShellScrollBarEndToEndTests) ------------------------------------------------------------------------------------

// C#: DraggingTheVerticalThumb_ThroughRealMessages_ScrollsTheViewportProportionally
#[test]
#[ignore = "native: needs desktop"]
fn dragging_the_vertical_thumb_through_real_messages_scrolls_the_viewport_proportionally() {
    let ui = start_ui();
    run(&ui, || {
        let b = with_image();
        zoom_to_actual_size(&b);
        let bars = b.window.scroll_bars().unwrap();
        let viewport = Rc::clone(b.window.viewport());
        let dpi = b.shell.dpi_scale();
        let bx = bars.current_layout().vertical;
        let (px, py) = to_pixels(bx.thumb.x + 4.0, bx.thumb.y + bx.thumb.height / 2.0, dpi);

        send(&b, WM_LBUTTONDOWN, MK_LBUTTON, px, py);
        assert!(bars.is_dragging(), "a press on the thumb starts a drag");
        assert!(b.shell.has_pointer_capture(), "a thumb drag holds the mouse capture");

        let moved = 10.max((bx.travel() * dpi / 3.0) as i32);
        send(&b, WM_MOUSEMOVE, MK_LBUTTON, px, py + moved);

        let grab = f64::from(py) / dpi - bx.thumb.y;
        let expected = offset_for_thumb_start(&bx, f64::from(py + moved) / dpi - grab);
        assert!(expected > 0.0);
        assert!(
            (viewport.vertical_offset() - expected).abs() < 1e-6,
            "{} vs {expected}",
            viewport.vertical_offset()
        );
        assert!(viewport.horizontal_offset().abs() < 1e-9);
        let thumb_after = bars.current_layout().vertical.thumb;
        assert!(
            (thumb_after.y - (bx.thumb.y + f64::from(moved) / dpi)).abs() < 1e-6,
            "the thumb follows the mouse"
        );

        send(&b, WM_LBUTTONUP, 0, px, py + moved);
        assert!(!bars.is_dragging());
        assert!(!b.shell.has_pointer_capture());
        assert!((viewport.vertical_offset() - expected).abs() < 1e-6);
    });
    finish(ui);
}

// C#: DraggingTheHorizontalThumb_PastBothEnds_ClampsAtTheMaximumOffset
#[test]
#[ignore = "native: needs desktop"]
fn dragging_the_horizontal_thumb_past_both_ends_clamps_at_the_maximum_offset() {
    let ui = start_ui();
    run(&ui, || {
        let b = with_image();
        zoom_to_actual_size(&b);
        let viewport = Rc::clone(b.window.viewport());
        let dpi = b.shell.dpi_scale();
        let bx = b.window.scroll_bars().unwrap().current_layout().horizontal;
        let (px, py) = to_pixels(bx.thumb.x + bx.thumb.width / 2.0, bx.thumb.y + 4.0, dpi);

        send(&b, WM_LBUTTONDOWN, MK_LBUTTON, px, py);
        send(&b, WM_MOUSEMOVE, MK_LBUTTON, px + 20_000, py);
        assert!(
            (viewport.horizontal_offset() - bx.max_offset).abs() < 1e-6,
            "clamped at the end"
        );
        send(&b, WM_MOUSEMOVE, MK_LBUTTON, px - 20_000, py);
        assert!(viewport.horizontal_offset().abs() < 1e-9, "clamped at the start");
        send(&b, WM_LBUTTONUP, 0, px, py);
    });
    finish(ui);
}

// C#: ClickingTheTrack_PagesByOneViewport_AndAPressOnTheBarNeverPansTheImage
#[test]
#[ignore = "native: needs desktop"]
fn clicking_the_track_pages_by_one_viewport_and_a_press_on_the_bar_never_pans_the_image() {
    let ui = start_ui();
    run(&ui, || {
        let b = with_image();
        zoom_to_actual_size(&b);
        let bars = b.window.scroll_bars().unwrap();
        let viewport = Rc::clone(b.window.viewport());
        let dpi = b.shell.dpi_scale();
        let bx = bars.current_layout().vertical;
        let viewport_height = viewport.viewport_height();

        // In the track below the thumb: one page down, released at once (no repeat).
        let (px, py) = to_pixels(bx.track.x + 4.0, bx.thumb.y + bx.thumb.height + 10.0, dpi);
        send(&b, WM_LBUTTONDOWN, MK_LBUTTON, px, py);
        send(&b, WM_LBUTTONUP, 0, px, py);
        let expected = bx.max_offset.min(viewport_height);
        assert!(
            (viewport.vertical_offset() - expected).abs() < 1e-6,
            "{} vs {expected}",
            viewport.vertical_offset()
        );

        // A press on the bar's margin (not the track), over the image box: neither a pan nor a capture.
        let before = (viewport.horizontal_offset(), viewport.vertical_offset());
        let (mx, my) = to_pixels(bx.bar.x + 0.4, bx.bar.y + bx.bar.height / 2.0, dpi);
        send(&b, WM_LBUTTONDOWN, MK_LBUTTON, mx, my);
        assert!(!b.shell.has_pointer_capture());
        send(&b, WM_MOUSEMOVE, MK_LBUTTON, mx - 50, my - 50);
        send(&b, WM_LBUTTONUP, 0, mx - 50, my - 50);
        assert_eq!(before, (viewport.horizontal_offset(), viewport.vertical_offset()));
    });
    finish(ui);
}

// C#: BarsAreAbsent_InFitMode_AndAppearAfterZoom_ReflowingTheViewport
#[test]
#[ignore = "native: needs desktop"]
fn bars_are_absent_in_fit_mode_and_appear_after_zoom_reflowing_the_viewport() {
    let ui = start_ui();
    run(&ui, || {
        let b = with_image();
        let bars = b.window.scroll_bars().unwrap();
        let viewport = Rc::clone(b.window.viewport());
        assert!(b.window.view_model().borrow().viewer().borrow().is_fit());
        assert!(!bars.current_layout().any_visible(), "no bar in Fit mode");
        let client = b.shell.client_size_dip();
        assert!(
            (viewport.viewport_width() - client.width).abs() < 1e-6,
            "no bar takes room in Fit mode"
        );

        zoom_to_actual_size(&b);
        let width = viewport.viewport_width();
        assert!(
            (width - (client.width - 10.0)).abs() < 1e-6,
            "the 10 DIP vertical bar takes room as in WPF: {width}"
        );
        assert!((bars.current_layout().vertical.bar.x - width).abs() < 1e-6);
    });
    finish(ui);
}

// C#: ResizingTheWindow_ReflowsTheBars_AndTheThumbStaysInsideTheTrack
#[test]
#[ignore = "native: needs desktop"]
fn resizing_the_window_reflows_the_bars_and_the_thumb_stays_inside_the_track() {
    let ui = start_ui();
    run(&ui, || {
        let b = with_image();
        zoom_to_actual_size(&b);
        let before = b.shell.client_size_dip();
        user32::set_window_pos(
            b.shell.hwnd(),
            0,
            0,
            0,
            500,
            400,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .unwrap();
        let client = b.shell.client_size_dip();
        assert!(
            client.width != before.width || client.height != before.height,
            "the client size changed"
        );
        b.window.viewport().update_layout();

        let bars = b.window.scroll_bars().unwrap();
        let layout = bars.current_layout();
        assert!(layout.vertical.visible && layout.horizontal.visible);
        assert!((layout.vertical.bar.x - (client.width - 10.0)).abs() < 1e-6);
        assert!((layout.horizontal.bar.y - (client.height - 10.0)).abs() < 1e-6);
        assert!((layout.vertical.bar.height - (client.height - 10.0)).abs() < 1e-6);
        for bx in [layout.vertical, layout.horizontal] {
            assert!(bx.thumb_visible);
            assert!(
                bx.thumb.x >= bx.track.x - 1e-9 && bx.thumb.x + bx.thumb.width <= bx.track.x + bx.track.width + 1e-9
            );
            assert!(
                bx.thumb.y >= bx.track.y - 1e-9 && bx.thumb.y + bx.thumb.height <= bx.track.y + bx.track.height + 1e-9
            );
        }

        // A drag still follows the new geometry.
        let dpi = b.shell.dpi_scale();
        let bx = layout.vertical;
        let (px, py) = to_pixels(bx.thumb.x + 4.0, bx.thumb.y + bx.thumb.height / 2.0, dpi);
        send(&b, WM_LBUTTONDOWN, MK_LBUTTON, px, py);
        send(&b, WM_MOUSEMOVE, MK_LBUTTON, px, py + 30);
        send(&b, WM_LBUTTONUP, 0, px, py + 30);
        let grab = f64::from(py) / dpi - bx.thumb.y;
        let expected = offset_for_thumb_start(&bx, f64::from(py + 30) / dpi - grab);
        assert!((b.window.viewport().vertical_offset() - expected).abs() < 1e-6);
    });
    finish(ui);
}

// C#: Zooming_ThroughTheWheel_ResizesTheThumb_AndTheFrameStillPresents (the zoom comes from the viewer, see the module doc)
#[test]
#[ignore = "native: needs desktop"]
fn zooming_resizes_the_thumb_and_the_frame_still_presents_and_draws_the_bars() {
    let ui = start_ui();
    run(&ui, || {
        let b = with_image();
        zoom_to_actual_size(&b);
        let bars = b.window.scroll_bars().unwrap();
        let before = bars.current_layout().vertical;

        let zoom = b.window.view_model().borrow().viewer().borrow().zoom();
        let update = b
            .window
            .view_model()
            .borrow()
            .viewer()
            .borrow_mut()
            .set_zoom(zoom * 2.0);
        b.window.apply_viewer_changes(update.changes);
        b.window.viewport().update_layout();
        let after = bars.current_layout().vertical;
        assert!(
            after.thumb.height < before.thumb.height,
            "{} -> {}",
            before.thumb.height,
            after.thumb.height
        );
        let viewport = b.window.viewport();
        let expected = after.track_length() * viewport.viewport_height() / viewport.extent_height();
        assert!(
            (after.thumb.height - expected).abs() < 1e-4,
            "{} vs {expected}",
            after.thumb.height
        );

        // A frame with bars presents on the real surface and draws them (read both counters in the same turn).
        let (total, drawn) = (b.window.with_stats(|stats| stats.total()), bars.drawn_frames());
        let mut presented = false;
        for _ in 0..50 {
            b.window.render_now();
            if b.window.with_stats(|stats| stats.total()) > total {
                presented = true;
                break;
            }
        }
        assert!(presented, "a frame with bars must present (the pacer may defer a few)");
        assert!(bars.drawn_frames() > drawn, "the real frame calls draw_scroll_bars");
    });
    finish(ui);
}

// C#: ThumbDragLatency_IsMeasuredAtTheHandler_WhichAppliesTheOffsetSynchronously
#[test]
#[ignore = "native: needs desktop"]
fn thumb_drag_latency_is_measured_at_the_handler_which_applies_the_offset_synchronously() {
    let ui = start_ui();
    run(&ui, || {
        let b = with_image();
        zoom_to_actual_size(&b);
        let dpi = b.shell.dpi_scale();
        let bx = b.window.scroll_bars().unwrap().current_layout().vertical;
        let (px, py) = to_pixels(bx.thumb.x + 4.0, bx.thumb.y + bx.thumb.height / 2.0, dpi);
        let travel_pixels = 2.max((bx.travel() * dpi) as i32 - 2);

        send(&b, WM_LBUTTONDOWN, MK_LBUTTON, px, py);
        let mut samples = Vec::new();
        for i in 0..200 {
            let factor = if i % 2 == 0 { 1.0 } else { 0.2 };
            let target = py + (factor * f64::from(travel_pixels) / 2.0) as i32;
            let start = Instant::now();
            send(&b, WM_MOUSEMOVE, MK_LBUTTON, px, target);
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        send(&b, WM_LBUTTONUP, 0, px, py);
        samples.sort_by(f64::total_cmp);
        let p95 = samples[(samples.len() as f64 * 0.95) as usize];
        assert!(p95 < 50.0, "p95 {p95:.2} ms");
    });
    finish(ui);
}

// ---- minimised -----------------------------------------------------------------------------------------------------------------------------------

// C#: MinimizeThenRestore_KeepsTheViewerStateAndTheWindowRect (the frame half) + IForwardedOpenTarget.IsMinimized / Restore.
// Kills the MISSED mutants `is_minimized -> false`, `is_minimized !=` -> `==`, `restore -> ()`, `restore !=` -> `==` of CL-MAINWIN.
#[test]
#[ignore = "native: needs desktop (shows the window minimised, without activating it)"]
fn a_minimised_window_draws_nothing_and_restore_brings_the_frame_back() {
    let ui = start_ui();
    run(&ui, || {
        let b = with_image();
        assert!(!b.window.is_minimized());
        let zoom_before = b.window.view_model().borrow().viewer().borrow().zoom();

        user32::show_window(b.shell.hwnd(), SW_SHOWMINNOACTIVE);
        assert!(b.window.is_minimized(), "IsIconic is true after the minimise");
        let (width, height) = b.shell.client_size_pixels();
        assert!(
            width <= 0 || height <= 0,
            "a minimised window has no client area: {width}x{height}"
        );

        let total = b.window.with_stats(|stats| stats.total());
        b.window.render_now();
        b.window.render_now();
        assert_eq!(
            b.window.with_stats(|stats| stats.total()),
            total,
            "nothing is presented while minimised"
        );

        b.window.restore();
        assert!(!b.window.is_minimized(), "restore leaves the minimised state");
        let (width, height) = b.shell.client_size_pixels();
        assert!(width > 0 && height > 0, "the client area is back: {width}x{height}");
        let mut presented = false;
        for _ in 0..50 {
            b.window.render_now();
            if b.window.with_stats(|stats| stats.total()) > total {
                presented = true;
                break;
            }
        }
        assert!(presented, "the frame after the restore draws");
        assert_eq!(
            b.window.view_model().borrow().viewer().borrow().zoom(),
            zoom_before,
            "the viewer state is untouched"
        );
    });
    finish(ui);
}

// ---- update_retained / on_compare_changed / on_navigation_changed -------------------------------------------------------------------------------

/// Uploads X (kept), Z (neither retained nor protected) and Y (the last upload) into the window's GPU cache. Y is the cache's most recent image; only X's and
/// Z's fate depend on the retained set.
fn upload_x_z_y(b: &Built, x: &Arc<PixelBuffer>, z: &Arc<PixelBuffer>, y: &Arc<PixelBuffer>) {
    let cache = b.window.image_cache().expect("a cache after attach_renderer");
    for image in [x, z, y] {
        cache.get_or_upload_tiled(image).expect("upload");
    }
}

/// Shrinks the budget so that everything that is not retained or protected is evicted, then reports `(x kept, z kept)`.
fn shrink_and_probe(b: &Built, x: &PixelBuffer, z: &PixelBuffer) -> (bool, bool) {
    let cache = b.window.image_cache().expect("cache");
    cache.inner().set_budget_bytes(1).expect("budget");
    (cache.inner().contains(x.id()), cache.inner().contains(z.id()))
}

// C#: UpdateRetained() after a handled image: ids = composer.CollectRetainedIds (+ planned + compare); cache.Retain(ids). The current image survives a
// shrinking budget, an image nobody retains does not. Kills `update_retained -> ()` and the retained-id plumbing.
#[test]
#[ignore = "native: needs desktop"]
fn handling_an_image_retains_it_so_that_a_shrinking_budget_keeps_it() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        let (x, y, z) = (solid(64, 1), solid(64, 2), solid(64, 3));
        b.hub.raise(Some(&PlatformImage::Pixels(Arc::clone(&x))), true);
        b.window.apply_changes(&ChangeSet::default());
        assert_eq!(b.window.composer().unwrap().current().map(|c| c.id()), Some(x.id()));
        upload_x_z_y(&b, &x, &z, &y);
        assert_eq!(shrink_and_probe(&b, &x, &z), (true, false), "X is retained, Z is not");
    });
    finish(ui);
}

// C#: OnCompareChanged(): unless disposed, UpdateRetained() + Invalidate(). A stale retained set is rebuilt by a compare change.
// Kills `on_compare_changed -> ()` (CL-MAINWIN: no observer).
#[test]
#[ignore = "native: needs desktop"]
fn a_compare_change_rebuilds_the_retained_set() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        let (x, y, z) = (solid(64, 1), solid(64, 2), solid(64, 3));
        b.hub.raise(Some(&PlatformImage::Pixels(Arc::clone(&x))), true);
        b.window.apply_changes(&ChangeSet::default());
        upload_x_z_y(&b, &x, &z, &y);
        // Make the retained set stale on purpose: nothing retained.
        b.window.image_cache().unwrap().inner().retain(&[]).expect("retain");

        let mut changes = ChangeSet::default();
        changes.compare.insert(CompareProperty::IsVisible);
        b.window.apply_changes(&changes);

        assert_eq!(
            shrink_and_probe(&b, &x, &z),
            (true, false),
            "the compare change retained the composer's current image again"
        );
    });
    finish(ui);
}

// Control of the test above: a change set without a compare property does not rebuild the retained set, so the stale (empty) set evicts X.
#[test]
#[ignore = "native: needs desktop"]
fn a_change_set_without_compare_leaves_the_retained_set_alone() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        let (x, y, z) = (solid(64, 1), solid(64, 2), solid(64, 3));
        b.hub.raise(Some(&PlatformImage::Pixels(Arc::clone(&x))), true);
        b.window.apply_changes(&ChangeSet::default());
        upload_x_z_y(&b, &x, &z, &y);
        b.window.image_cache().unwrap().inner().retain(&[]).expect("retain");

        let mut changes = ChangeSet::default();
        changes.main.insert(MainProperty::FolderTitle);
        b.window.apply_changes(&changes);

        assert_eq!(shrink_and_probe(&b, &x, &z), (false, false), "nothing re-retained X");
    });
    finish(ui);
}

// C#: OnNavigationChanged(): prefetcher.Navigated(vm.CurrentIndex, vm.TotalFiles). The view model of the fixture has an empty catalog, so the call clears
// the plan and forgets the last index: the next `navigated` keeps the old direction instead of deriving one from the stale index.
// Kills `on_navigation_changed -> ()`.
#[test]
#[ignore = "native: needs desktop"]
fn a_current_index_change_tells_the_prefetcher_and_forgets_its_last_index() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        let prefetcher = b.window.prefetcher().expect("prefetcher");
        prefetcher.navigated(5, 10);
        prefetcher.navigated(3, 10);
        assert_eq!(prefetcher.direction(), -1);

        let mut changes = ChangeSet::default();
        changes.main.insert(MainProperty::CurrentIndex);
        b.window.apply_changes(&changes);

        prefetcher.navigated(4, 10);
        assert_eq!(
            prefetcher.direction(),
            -1,
            "last_index was reset by the window's navigation step, so 3 -> 4 is not a forward step"
        );
    });
    finish(ui);
}

// Control: without CurrentIndex the prefetcher is not told, its last index (3) stays and 3 -> 4 flips the direction.
#[test]
#[ignore = "native: needs desktop"]
fn a_change_set_without_current_index_does_not_touch_the_prefetcher() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        let prefetcher = b.window.prefetcher().expect("prefetcher");
        prefetcher.navigated(5, 10);
        prefetcher.navigated(3, 10);
        b.window.apply_changes(&ChangeSet::default());
        prefetcher.navigated(4, 10);
        assert_eq!(prefetcher.direction(), 1);
    });
    finish(ui);
}

// ---- PrefetchHost --------------------------------------------------------------------------------------------------------------------------------

// C#: PrefetchHost.PathAt(index): the catalog entry, null outside the catalog. Kills `path_at -> None` and the range-test operators of CL-MAINWIN.
#[test]
#[ignore = "native: needs desktop"]
fn prefetch_host_path_at_reads_the_catalog_and_is_none_outside_it() {
    let ui = start_ui();
    run(&ui, || {
        let b = build(BuildOptions::default());
        std::fs::create_dir_all(&b.photos).unwrap();
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            std::fs::write(b.photos.join(name), b"x").unwrap();
        }
        b.window.open_initial(Some(&b.photos.to_string_lossy()), false);
        for _ in 0..200 {
            if b.window.view_model().borrow().total_files() >= 3 {
                break;
            }
            b.runner.run_all();
            b.window.pump_view_model();
        }
        assert_eq!(
            b.window.view_model().borrow().total_files(),
            3,
            "the catalog holds the three files"
        );

        let host = b.window.prefetch_host();
        for index in 0..3_i32 {
            let expected = b
                .window
                .view_model()
                .borrow()
                .catalog()
                .path_at(index as usize)
                .to_owned();
            assert_eq!(host.path_at(index), Some(expected), "index {index}");
        }
        assert_eq!(host.path_at(-1), None);
        assert_eq!(host.path_at(3), None);
        assert_eq!(host.path_at(i32::MAX), None);
    });
    finish(ui);
}

// The host holds a Weak: once the window is gone every method is a quiet no-op (no panic, no worker thread, no posted job).
// NOT observed here: the worker thread of `start_lookup` and the retry pass of `schedule_retry` on a live window (the effects are only visible through
// `prefetch_calls`, which needs a decoded preview in the preview service: a corpus test).
#[test]
#[ignore = "native: needs desktop"]
fn prefetch_host_does_nothing_after_the_window_is_gone() {
    let ui = start_ui();
    run(&ui, || {
        let b = built_with_renderer();
        let host = b.window.prefetch_host();
        drop(b);
        drop(take_built());
        assert_eq!(host.path_at(0), None);
        host.start_lookup(LookupRequest {
            generation: 1,
            paths: vec!["a.jpg".to_owned()],
        });
        host.schedule_retry(
            Duration::from_millis(1),
            RetryRequest {
                generation: 1,
                index: 0,
                count: 1,
            },
        );
    });
    finish(ui);
}
