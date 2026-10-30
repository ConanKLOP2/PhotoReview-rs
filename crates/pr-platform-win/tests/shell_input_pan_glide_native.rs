//! Native twins for the gap of `X-pointer-click-pan` (cl-misc): a REAL `MainWindow` (hidden shell window, WARP surface, real view model) driven with a mouse
//! DRAG through real messages (`WM_LBUTTONDOWN`, `WM_MOUSEMOVE` with `MK_LBUTTON`, `WM_LBUTTONUP`) whose release STARTS a kinetic glide.
//! `shell_input_e2e.rs` covers click-to-zoom, arrow impulses and the navigation stop; none of them drives a drag release. `SendMessage` does not describe the
//! fake message through `GetMessageTime`, so the window gets a `ShellInputEnvironment` whose `message_time` is a test-owned clock (as the G-INPUT replay does);
//! nothing sleeps. Every test is `#[ignore = "native: needs desktop"]`; nothing is shown or activated.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
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

use std::cell::Cell;
use std::path::PathBuf;

use main_window_fixture::{BuildOptions, Built, WarpHook, build_with_env, built, finish, run, start_ui, wait_until};
use pr_app::input::key_id::KeyId;
use pr_app::input::key_mapping;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::shell_input::environment::ShellInputEnvironment;
use pr_platform_win::shell_input::main_window::MainWindowInputFeature;
use pr_platform_win::win32::messages::{WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE};
use shell_window_fixture::send;
use ui_thread::UiThread;

const MK_LBUTTON: usize = 0x0001;

thread_local! {
    /// The test-owned `GetMessageTime` (ms) of the next pointer message (UI thread).
    static MESSAGE_TIME: Cell<i32> = const { Cell::new(0) };
}

fn write_image(path: &std::path::Path, width: i32, height: i32) {
    use pr_imaging::pixels::{PixelBuffer, PixelLayout};
    use pr_platform_win::wic::cache_image::encode_png;
    let mut pixels = PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).expect("allocate test pixels");
    for pixel in pixels.try_get_span_mut().unwrap().unwrap().chunks_exact_mut(4) {
        pixel.copy_from_slice(&[80, 120, 160, 255]);
    }
    let mut file = std::fs::File::create(path).expect("create image");
    encode_png(&pixels, &mut file).expect("encode PNG");
}

fn settle_step() {
    let b = built();
    b.runner.run_all();
    b.scheduler.run_pending();
    b.window.pump_view_model();
}

fn present_current() {
    let b = built();
    let image = b.window.view_model().borrow().current_image().cloned();
    if let Some(image) = image {
        b.hub.raise(Some(&image), false);
    }
}

fn wait(ui: &UiThread, what: &str, condition: fn(&Built) -> bool) {
    wait_until(ui, what, move || {
        settle_step();
        present_current();
        condition(&built())
    });
}

/// Builds the window (with the test message clock) and opens a folder with one big image, then waits until it is shown.
fn start_with_big_image(ui: &UiThread) -> PathBuf {
    let folder = run(ui, || {
        use pr_imaging::memory_headroom::MemoryInfo;
        use pr_imaging::pixel_memory::PixelBufferImageCodec;
        use pr_platform_win::wic::decoder::WicDirectDecoder;
        use std::sync::Arc;
        use support::main_env::{EnvOptions, MainEnv};
        MESSAGE_TIME.with(|time| time.set(0));
        let env = MainEnv::with_options(EnvOptions {
            decoder: Arc::new(WicDirectDecoder::new(
                Arc::new(PixelBufferImageCodec::INSTANCE),
                None,
                Arc::new(|| MemoryInfo {
                    total_available: 32 * 1024 * 1024 * 1024,
                    load: 0,
                }),
            )),
            initial_view: Some(Box::new(|bridge, settings| {
                bridge.apply_initial_view(built().window.viewport().viewport_size(), settings);
            })),
            ..EnvOptions::default()
        });
        let mut options = BuildOptions::default();
        options.options.extensions.insert(ShellInputEnvironment {
            message_time: Box::new(|| MESSAGE_TIME.with(Cell::get)),
            ..ShellInputEnvironment::default()
        });
        let b = build_with_env(options, env);
        let (surface_width, surface_height) = b.shell.client_size_pixels();
        b.window
            .attach_renderer(Some(&WarpHook::new(surface_width, surface_height)))
            .expect("attach WARP renderer");
        let folder = b.photos.join("shots");
        std::fs::create_dir_all(&folder).expect("create the folder");
        write_image(&folder.join("big.png"), 2400, 1600);
        b.window.open_path(&folder.join("big.png").to_string_lossy());
        folder
    });
    wait(ui, "the first image", |b| {
        b.window.view_model().borrow().has_images()
            && b.window.view_model().borrow().current_image().is_some()
            && b.window.first_image_presented()
            && b.window.viewport().image_actual_width() > 0.0
            && !b.window.viewport().is_layout_posted()
    });
    folder
}

fn input() -> std::rc::Rc<pr_platform_win::shell_input::main_window::MainWindowInput> {
    built()
        .window
        .with_feature::<MainWindowInputFeature, _>(|feature| std::rc::Rc::clone(feature.input()))
        .expect("the input feature is registered")
}

fn key(key: KeyId) {
    send(
        built().window.hwnd(),
        WM_KEYDOWN,
        key_mapping::to_virtual_key(key) as usize,
        1,
    );
}

fn pack(x: i32, y: i32) -> isize {
    ((((y as u16) as usize) << 16) | ((x as u16) as usize)) as isize
}

fn edit_settings(change: impl FnOnce(&mut pr_core::settings::app_settings::AppSettings)) {
    let source = built().window.view_model().borrow().settings_source();
    let edit = source.begin_edit(change);
    let _ = source.commit_edit(edit);
}

fn zoom_percent() -> i32 {
    built()
        .window
        .view_model()
        .borrow()
        .viewer()
        .borrow()
        .display_zoom_percent()
}

/// Zooms to 100 % through a real key and waits until the layout is settled: the 2400x1600 picture is larger than the 640x480 client, so it can pan.
fn zoom_to_actual_size(ui: &UiThread) {
    run(ui, || key(KeyId::D1));
    wait(ui, "100 %", |b| {
        zoom_percent() == 100 && !b.window.viewport().is_layout_posted()
    });
}

fn message(msg: u32, wparam: usize, x: i32, y: i32, time: i32) {
    MESSAGE_TIME.with(|clock| clock.set(time));
    send(built().window.hwnd(), msg, wparam, pack(x, y));
}

/// Press at the client centre, drag left in `steps` moves of `step_px` every `step_ms`, then release `release_after_ms` after the last move.
fn drag_left(steps: i32, step_px: i32, step_ms: i32, release_after_ms: i32) {
    let (cx, cy) = {
        let b = built();
        let size = b.window.window().client_size_dip();
        let dpi = b.window.window().dpi_scale();
        ((size.width * dpi / 2.0) as i32, (size.height * dpi / 2.0) as i32)
    };
    message(WM_LBUTTONDOWN, MK_LBUTTON, cx, cy, 0);
    assert!(
        built().window.window().has_pointer_capture(),
        "a left press on the image captures the pointer"
    );
    for step in 1..=steps {
        message(WM_MOUSEMOVE, MK_LBUTTON, cx - step * step_px, cy, step * step_ms);
    }
    let last = cx - steps * step_px;
    message(WM_LBUTTONUP, 0, last, cy, steps * step_ms + release_after_ms);
    assert!(
        !built().window.window().has_pointer_capture(),
        "the release gives the capture back"
    );
}

fn horizontal_offset() -> f64 {
    built().window.viewport().horizontal_offset()
}

/// Runs `drag`, then waits (bounded) until the layout has applied the pan; returns `(offset before, offset after)`.
fn drag_and_settle(ui: &UiThread, drag: fn()) -> (f64, f64) {
    let before = run(ui, horizontal_offset);
    run(ui, drag);
    wait_until(ui, "the drag has panned the image", move || {
        settle_step();
        (horizontal_offset() - before).abs() > 1.0
    });
    (before, run(ui, horizontal_offset))
}

// C#: ShellInputEndToEndTests (no twin): a mouse drag whose release starts a kinetic glide, through real messages. Dragging left scrolls right.
#[test]
#[ignore = "native: needs desktop"]
fn a_fast_drag_release_starts_a_kinetic_glide_through_real_messages() {
    let ui = start_with_big_image_ui();
    run(&ui, || edit_settings(|s| s.kinetic_pan_enabled = true));
    zoom_to_actual_size(&ui);
    let (before, after_release) = drag_and_settle(&ui, || drag_left(6, 20, 8, 0));
    assert!(
        after_release > before + 1.0,
        "the drag itself pans the image: {before} -> {after_release}"
    );
    assert!(
        run(&ui, || input().input().stop_kinetic()),
        "the release of a fast drag must hook a glide"
    );
    finish(ui);
}

// Same drag, kinetic pan disabled: the release ends the pan and nothing glides.
#[test]
#[ignore = "native: needs desktop"]
fn a_drag_release_without_kinetic_pan_starts_no_glide() {
    let ui = start_with_big_image_ui();
    run(&ui, || edit_settings(|s| s.kinetic_pan_enabled = false));
    zoom_to_actual_size(&ui);
    let (before, after_release) = drag_and_settle(&ui, || drag_left(6, 20, 8, 0));
    assert!(
        after_release > before + 1.0,
        "the drag still pans: {before} -> {after_release}"
    );
    assert!(
        !run(&ui, || input().input().stop_kinetic()),
        "with kinetic pan off the release hooks no glide"
    );
    finish(ui);
}

// A drag that rests before the release has no velocity left: the pointer samples are older than the velocity window, so no glide starts.
#[test]
#[ignore = "native: needs desktop"]
fn a_drag_that_rests_before_the_release_starts_no_glide() {
    let ui = start_with_big_image_ui();
    run(&ui, || edit_settings(|s| s.kinetic_pan_enabled = true));
    zoom_to_actual_size(&ui);
    drag_and_settle(&ui, || drag_left(6, 20, 8, 2_000));
    assert!(
        !run(&ui, || input().input().stop_kinetic()),
        "a release two seconds after the last move has no velocity"
    );
    finish(ui);
}

fn start_with_big_image_ui() -> UiThread {
    let ui = start_ui();
    start_with_big_image(&ui);
    ui
}
