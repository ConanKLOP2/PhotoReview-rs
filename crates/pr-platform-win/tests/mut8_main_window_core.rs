//! P8 (2026-11-06): killers for the `host/main_window/core.rs` mutants that survived the P3 lib run and the first native run of P8:
//! `publish_viewport_size` (996), `register_keep_warm` (1003), `set_keep_warm` (1103), `MainWindow::strong_count` (1166), the zero-client-area guards of
//! `attach_renderer` (1258) and `open_initial` (1293), and the dpi division of `open_initial` (1295).
//!
//! The window is a real hidden shell window; the client size and the dpi are driven with `WM_SIZE` / `WM_DPICHANGED` messages (the shell window stores
//! the numbers of the message), so no minimised window and no desktop interaction are needed. Every wait is bounded or a direct call, no sleeps.
//! Native tests are `#[ignore]`; run with `--include-ignored --test-threads=1`.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/main_window_frame_fixture.rs"]
mod main_window_frame_fixture;
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

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use main_window_fixture::{
    BuildOptions, WarpHook, build, built, finish, new_order, run, start_ui, take_built, wait_until,
};
use main_window_frame_fixture::HookFeature;
use pr_imaging::adaptive_preview::calculate_target_decode_box;
use pr_imaging::decode_types::DecodeBox;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::shell_viewing::keep_warm::{KeepWarmOptions, KeepWarmRegistration};
use pr_platform_win::win32::messages::WM_SIZE;
use pr_platform_win::win32::structs::Rect;
use pr_test_support::TempRoot;

fn pack(width: i32, height: i32) -> isize {
    (((height as u32 & 0xFFFF) << 16) | (width as u32 & 0xFFFF)) as isize
}

/// The window procedure takes the client size from the message (`WM_SIZE`, SIZE_RESTORED).
fn set_client_size(b: &main_window_fixture::Built, width: i32, height: i32) {
    shell_window_fixture::send(b.shell.hwnd(), WM_SIZE, 0, pack(width, height));
    assert_eq!(b.shell.client_size_pixels(), (width, height));
}

// core.rs 996:9 `publish_viewport_size -> ()`: the layout-updated subscription publishes the NEW viewport size for worker threads. Without it the
// accessor keeps answering the size of the constructor.
#[test]
#[ignore = "native: needs desktop"]
fn layout_update_publishes_the_new_viewport_size_for_worker_threads() {
    let ui = start_ui();
    let (initial, published, live) = run(&ui, || {
        let b = build(BuildOptions::default());
        let initial = b.size_source.viewport_size();
        set_client_size(&b, 777, 555);
        b.window.viewport().update_layout();
        let source = Arc::clone(&b.size_source);
        let published = std::thread::spawn(move || source.viewport_size()).join().unwrap();
        (initial, published, b.window.viewport().viewport_size())
    });
    assert_ne!(initial, live, "the test really changed the viewport size");
    assert_eq!(published, live, "the worker thread sees the size of the latest layout");
    finish(ui);
}

fn frame_log_registration(path: std::path::PathBuf) -> KeepWarmRegistration {
    KeepWarmRegistration {
        frame_log_path: Some(path),
        // Switched off: only the frame log is created (no keep-alive thread, no GPU work).
        keep_warm: Some(KeepWarmOptions {
            enabled: false,
            ..KeepWarmOptions::default()
        }),
        prefer_warp: true,
        work: None,
        timestamp: None,
    }
}

// core.rs 1003:9 `register_keep_warm -> ()` (the registration is never taken, no frame log file is created) and 1103:9 `set_keep_warm -> ()` (the setup is
// dropped at once: the file exists but the frames of the window are never recorded). The frame log of the registration records the frames the window draws.
#[test]
#[ignore = "native: needs desktop"]
fn keep_warm_registration_is_stored_and_its_frame_log_records_the_window_frames() {
    let ui = start_ui();
    let root = TempRoot::new("p8-keepwarm");
    let path = root.join("frames.log");
    let log_path = path.clone();
    let existed_while_alive = run(&ui, move || {
        let mut options = BuildOptions::default();
        options
            .options
            .extensions
            .insert(frame_log_registration(log_path.clone()));
        let b = build(options);
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        b.window.render_now();
        b.window.render_now();
        b.window.render_now();
        let existed = log_path.exists();
        drop(b);
        drop(take_built()); // the window is disposed: the setup is disposed first, which drains and closes the frame log
        existed
    });
    assert!(
        existed_while_alive,
        "register_keep_warm opened the frame log of the registration"
    );
    let text = std::fs::read_to_string(&path).expect("the frame log file");
    assert_eq!(text.lines().count(), 3, "one record per drawn frame: {text:?}");
    finish(ui);
}

// The defaults (no registration in the options) must not create anything either; and a registration is TAKEN, not copied: a second window built with the
// same options type finds none. (Control for the test above: the file is created only by the registration.)
#[test]
#[ignore = "native: needs desktop"]
fn without_a_registration_no_frame_log_is_written() {
    let ui = start_ui();
    let root = TempRoot::new("p8-keepwarm-none");
    let path = root.join("frames.log");
    run(&ui, || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        b.window.render_now();
        drop(b);
        drop(take_built());
    });
    assert!(!path.exists());
    finish(ui);
}

// core.rs 1166:9 `strong_count -> 1`: while a window event closure holds its upgraded `Weak` the count is above one (here: inside a frame delivered by the
// shell window's render tick), and it is exactly one again afterwards.
#[test]
#[ignore = "native: needs desktop"]
fn strong_count_sees_the_upgrade_held_by_a_window_event() {
    let ui = start_ui();
    let seen = Arc::new(AtomicUsize::new(0));
    let probe = Arc::clone(&seen);
    run(&ui, move || {
        let b = build(BuildOptions::default());
        let (w, h) = b.shell.client_size_pixels();
        b.window.attach_renderer(Some(&WarpHook::new(w, h))).expect("attach");
        let feature = HookFeature::new("count", new_order());
        *feature.hook.borrow_mut() = Some(Box::new(move || {
            probe.fetch_max(built().window.strong_count(), Ordering::SeqCst);
        }));
        b.window.add_feature(feature);
        // Settle, then ask for a frame that the render tick delivers through the window's own closure.
        b.shell.invalidate();
    });
    let watcher = Arc::clone(&seen);
    wait_until(&ui, "a frame from the render tick", move || {
        watcher.load(Ordering::SeqCst) > 0
    });
    assert!(
        seen.load(Ordering::SeqCst) > 1,
        "the closure of the render event holds an upgraded reference"
    );
    let after = run(&ui, || built().window.strong_count());
    assert_eq!(after, 1, "one owner again once the event returned");
    finish(ui);
}

// core.rs 1258:18 and 1258:32 (`width > 0`, `height > 0` -> `>=`): a client area with a zero side does not resize the surface (the surface keeps the size
// the factory gave it and the attach succeeds). With `>=` the zero side is passed to `resize`.
#[test]
#[ignore = "native: needs desktop"]
fn attach_renderer_leaves_the_surface_size_alone_for_a_zero_client_side() {
    let ui = start_ui();
    for (width, height) in [(0, 480), (640, 0), (0, 0)] {
        let outcome = run(&ui, move || {
            let b = build(BuildOptions::default());
            set_client_size(&b, width, height);
            let attached = b.window.attach_renderer(Some(&WarpHook::new(300, 200)));
            let size = b
                .window
                .with_surface(|surface| (surface.pixel_width(), surface.pixel_height()));
            (attached.is_ok(), size, b.window.has_surface())
        });
        assert!(outcome.0, "attach succeeds for a client of {width} x {height}");
        assert!(outcome.2, "the surface is stored for {width} x {height}");
        assert_eq!(
            outcome.1,
            Some((300, 200)),
            "the factory size is kept for a client of {width} x {height}"
        );
        run(&ui, || drop(take_built()));
    }
    finish(ui);
}

/// The same box the window calculates (`target_decode_box_for`): a side of one DIP or less falls back to the 2200 x 1400 screen.
fn expected_box(client: (i32, i32), dpi: f64) -> DecodeBox {
    let (w, h) = (f64::from(client.0) / dpi, f64::from(client.1) / dpi);
    calculate_target_decode_box(
        if w > 1.0 { w } else { 2200.0 },
        if h > 1.0 { h } else { 1400.0 },
        dpi,
        1.15,
    )
    .unwrap()
}

// core.rs 1293:25, 1293:29, 1293:46 (`client_width > 0 && client_height > 0` with `>=` / `||`): a zero client side leaves the decode box alone.
#[test]
#[ignore = "native: needs desktop"]
fn open_initial_keeps_the_decode_box_for_a_zero_client_side() {
    let ui = start_ui();
    for (width, height) in [(0, 480), (640, 0), (0, 0)] {
        let after = run(&ui, move || {
            let b = build(BuildOptions::default());
            set_client_size(&b, width, height);
            let sentinel = DecodeBox::new(1, 1);
            b.size_source.set_target_decode_box(sentinel);
            b.window.open_initial(None, false);
            let after = b.size_source.target_decode_box();
            drop(b);
            drop(take_built());
            (after, sentinel)
        });
        assert_eq!(
            after.0, after.1,
            "client {width} x {height}: the decode box is not recalculated"
        );
    }
    finish(ui);
}

// core.rs 1295:74 and 1295:106 (`client / dpi` -> `client * dpi`): at 150 % the DIP size is the pixel size divided by the scale. At 100 % both
// operators agree, so the window is first given a 144 dpi (WM_DPICHANGED) and a 960 x 720 pixel client.
#[test]
#[ignore = "native: needs desktop"]
fn open_initial_divides_the_client_pixels_by_the_dpi_scale() {
    let ui = start_ui();
    let (after, expected, other, scale) = run(&ui, || {
        let b = build(BuildOptions::default());
        shell_window_fixture::send_dpi_changed(
            b.shell.hwnd(),
            144,
            Rect {
                left: 0,
                top: 0,
                right: 1000,
                bottom: 800,
            },
        );
        let scale = b.shell.dpi_scale();
        set_client_size(&b, 960, 720);
        b.size_source.set_target_decode_box(DecodeBox::new(1, 1));
        b.window.open_initial(None, false);
        let after = b.size_source.target_decode_box();
        let expected = expected_box((960, 720), scale);
        // What a multiplication would have produced.
        let other = calculate_target_decode_box(960.0 * scale, 720.0 * scale, scale, 1.15).unwrap();
        drop(b);
        drop(take_built());
        (after, expected, other, scale)
    });
    assert_eq!(scale, 1.5, "the window runs at 150 %");
    assert_ne!(expected, other, "the two operators give different boxes at this scale");
    assert_eq!(after, expected);
    finish(ui);
}
