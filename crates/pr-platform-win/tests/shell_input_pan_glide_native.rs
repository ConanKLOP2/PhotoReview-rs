//! Native twins for the gap of `X-pointer-click-pan` (cl-misc): a REAL `MainWindow` (hidden shell window, WARP surface, real view model) driven with a mouse
//! DRAG through real messages (`WM_LBUTTONDOWN`, `WM_MOUSEMOVE` with `MK_LBUTTON`, `WM_LBUTTONUP`) whose release STARTS a kinetic glide.
//! `shell_input_e2e.rs` covers click-to-zoom, arrow impulses and the navigation stop; none of them drives a drag release. `SendMessage` does not describe the
//! fake message through `GetMessageTime`, so the window gets a `ShellInputEnvironment` whose `message_time` is a test-owned clock (as the G-INPUT replay does);
//! nothing sleeps. The window also gets a test-owned `FakeHostClock`: a glide frame only happens when the test ticks it, so the position after the release is
//! a function of the fake frame times (lane `st-pointer-pan` adds the glide-after-release, double-click and lost-capture tests). Every test is `#[ignore = "native: needs desktop"]`; nothing is shown or activated.
#![cfg(windows)]

#[path = "../../pr-app/tests/file_action_fakes/mod.rs"]
mod file_action_fakes;
#[path = "common/main_window_fixture.rs"]
mod main_window_fixture;
#[path = "common/shell_window_fixture.rs"]
mod shell_window_fixture;
#[path = "common/viewing_fakes.rs"]
mod viewing_fakes;
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

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use main_window_fixture::{BuildOptions, Built, WarpHook, build_with_env, built, finish, run, start_ui, wait_until};
use pr_app::input::key_id::KeyId;
use pr_app::input::key_mapping;
use pr_core::model::enums::KineticGlideSmoothing;
use pr_platform_win::host::contracts::ShellWindowApi;
use pr_platform_win::host::frame_clock::base::HostFrameClock;
use pr_platform_win::shell_input::environment::ShellInputEnvironment;
use pr_platform_win::shell_input::main_window::MainWindowInputFeature;
use pr_platform_win::win32::messages::{WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE};
use shell_window_fixture::send;
use ui_thread::UiThread;
use viewing_fakes::FakeHostClock;

const MK_LBUTTON: usize = 0x0001;
/// `WM_CAPTURECHANGED` (another window took the capture).
const WM_CAPTURECHANGED: u32 = 0x0215;
/// The test's double-click time (ms) and slop (px): the system values would make the tests machine-dependent.
const DOUBLE_CLICK_MS: i32 = 500;
const DOUBLE_CLICK_SLOP: (i32, i32) = (8, 8);
/// One fake frame is this long (ms).
const FRAME_MS: f64 = 16.0;

thread_local! {
    /// The test-owned `GetMessageTime` (ms) of the next pointer message (UI thread).
    static MESSAGE_TIME: Cell<i32> = const { Cell::new(0) };
    /// The test-owned frame clock of the window (UI thread).
    static FAKE_CLOCK: RefCell<Option<Rc<FakeHostClock>>> = const { RefCell::new(None) };
    /// The rendering time (ms) of the next fake frame.
    static FRAME_TIME: Cell<f64> = const { Cell::new(0.0) };
    /// While true, `settle_step` ticks one fake frame (the window needs frames to come up); the glide tests turn it off to own the frames.
    static AUTO_FRAMES: Cell<bool> = const { Cell::new(true) };
}

fn fake_clock() -> Rc<FakeHostClock> {
    FAKE_CLOCK
        .with(|slot| slot.borrow().clone())
        .expect("the window is built with a fake clock")
}

/// Raises one fake frame, `FRAME_MS` after the previous one.
fn tick_frame() {
    let time = FRAME_TIME.with(|time| {
        time.set(time.get() + FRAME_MS);
        time.get()
    });
    fake_clock().tick(time);
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
    if AUTO_FRAMES.with(Cell::get) {
        tick_frame();
    }
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
        FRAME_TIME.with(|time| time.set(0.0));
        AUTO_FRAMES.with(|auto| auto.set(true));
        let fake = FakeHostClock::new();
        FAKE_CLOCK.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&fake)));
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
        let clock: Rc<dyn HostFrameClock> = fake;
        options.frame_clock = Some(clock);
        options.options.extensions.insert(ShellInputEnvironment {
            message_time: Box::new(|| MESSAGE_TIME.with(Cell::get)),
            double_click_time_ms: Box::new(|| DOUBLE_CLICK_MS),
            double_click_size_pixels: Box::new(|_| DOUBLE_CLICK_SLOP),
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

// ---- glide after the release, with the fake frame clock -----------------------------------------------------------------------------------------

/// Kinetic pan on, the glide timed by the render timestamps only (`Off`: no vblank prediction, so the fake frame times are the whole truth).
fn kinetic_with_render_time() {
    edit_settings(|s| {
        s.kinetic_pan_enabled = true;
        s.kinetic_glide_smoothing = Some(KineticGlideSmoothing::Off);
    });
}

/// Frame subscribers right now (the glide hook is one of them while it runs).
fn frame_subscribers() -> usize {
    fake_clock().subscriber_count()
}

fn own_the_frames() {
    AUTO_FRAMES.with(|auto| auto.set(false));
}

fn centre() -> (i32, i32) {
    let b = built();
    let size = b.window.window().client_size_dip();
    let dpi = b.window.window().dpi_scale();
    ((size.width * dpi / 2.0) as i32, (size.height * dpi / 2.0) as i32)
}

fn has_capture() -> bool {
    built().window.window().has_pointer_capture()
}

/// `n` fake frames, each followed by the layout/pump step (which does not tick: the frames are the test's).
fn frames(n: usize) {
    for _ in 0..n {
        tick_frame();
        settle_step();
    }
}

// New test (no C# twin: the C# end-to-end test stops at "a glide is hooked"): after a fast release the position does NOT move until a frame comes; the first
// frame only anchors the glide clock, each later fake frame advances the image further in the drag direction, and the glide ends by itself (unhooks) with the
// offset still inside the scroll range.
#[test]
#[ignore = "native: needs desktop"]
fn a_released_drag_glides_further_frame_by_frame_and_then_ends_itself() {
    let ui = start_with_big_image_ui();
    run(&ui, kinetic_with_render_time);
    zoom_to_actual_size(&ui);
    run(&ui, own_the_frames);
    let baseline = run(&ui, frame_subscribers);
    let (before_drag, at_release) = drag_and_settle(&ui, || drag_left(6, 20, 8, 0));
    assert!(
        at_release > before_drag + 1.0,
        "the drag pans: {before_drag} -> {at_release}"
    );
    assert_eq!(
        run(&ui, frame_subscribers),
        baseline + 1,
        "the release of a fast drag hooks exactly one frame subscription"
    );
    run(&ui, settle_step);
    assert_eq!(
        run(&ui, horizontal_offset),
        at_release,
        "without a frame the image stays where the release left it"
    );
    run(&ui, || frames(1));
    assert_eq!(
        run(&ui, horizontal_offset),
        at_release,
        "the first frame anchors the glide clock and moves nothing"
    );
    let mut previous = at_release;
    let mut moved_frames = 0;
    let mut count = 0;
    while run(&ui, frame_subscribers) > baseline {
        run(&ui, || frames(1));
        let now = run(&ui, horizontal_offset);
        assert!(now >= previous, "the glide never reverses: {previous} -> {now}");
        if now > previous {
            moved_frames += 1;
        }
        previous = now;
        count += 1;
        assert!(
            count < 2_000,
            "the glide must end by itself in a bounded number of frames"
        );
    }
    assert!(
        moved_frames >= 3,
        "the glide moved on several frames, not just one: {moved_frames}"
    );
    assert!(
        previous > at_release + 1.0,
        "the glide carried the image beyond the release point: {at_release} -> {previous}"
    );
    let max = run(&ui, || {
        let b = built();
        let viewport = b.window.viewport();
        viewport.extent_width() - viewport.viewport_width()
    });
    assert!(
        previous <= max + 0.5,
        "the glide stays inside the scroll range: {previous} > {max}"
    );
    assert!(
        !run(&ui, || input().input().stop_kinetic()),
        "a finished glide is no longer hooked"
    );
    finish(ui);
}

// New test: a new press while the glide runs stops it (the image stays where it is) and the frames after that move nothing.
#[test]
#[ignore = "native: needs desktop"]
fn a_press_during_the_glide_stops_it_and_later_frames_move_nothing() {
    let ui = start_with_big_image_ui();
    run(&ui, kinetic_with_render_time);
    zoom_to_actual_size(&ui);
    run(&ui, own_the_frames);
    let baseline = run(&ui, frame_subscribers);
    drag_and_settle(&ui, || drag_left(6, 20, 8, 0));
    run(&ui, || frames(4));
    let gliding_at = run(&ui, horizontal_offset);
    run(&ui, || {
        let (cx, cy) = centre();
        message(WM_LBUTTONDOWN, MK_LBUTTON, cx, cy, 10_000);
    });
    assert_eq!(run(&ui, frame_subscribers), baseline, "the press unhooks the glide");
    run(&ui, || frames(4));
    assert_eq!(
        run(&ui, horizontal_offset),
        gliding_at,
        "frames after the press move nothing"
    );
    run(&ui, || {
        let (cx, cy) = centre();
        message(WM_LBUTTONUP, 0, cx, cy, 10_050);
    });
    finish(ui);
}

// ---- a drag cancelled by a lost capture -----------------------------------------------------------------------------------------------------------

// C# `PointerInputController.OnLostCapture` through the real `WM_CAPTURECHANGED` (another window took the mouse): the pan ends where it is, the later moves
// and the release do nothing, and no glide starts although the drag was fast.
#[test]
#[ignore = "native: needs desktop"]
fn a_lost_capture_cancels_the_drag_and_no_glide_follows() {
    let ui = start_with_big_image_ui();
    run(&ui, kinetic_with_render_time);
    zoom_to_actual_size(&ui);
    run(&ui, own_the_frames);
    let baseline = run(&ui, frame_subscribers);
    let (cx, cy) = run(&ui, centre);
    run(&ui, move || {
        message(WM_LBUTTONDOWN, MK_LBUTTON, cx, cy, 0);
        for step in 1..=4 {
            message(WM_MOUSEMOVE, MK_LBUTTON, cx - step * 20, cy, step * 8);
        }
        settle_step();
    });
    let panned = run(&ui, horizontal_offset);
    assert!(panned > 0.0, "the drag panned before the capture was stolen: {panned}");
    assert!(run(&ui, has_capture), "the drag holds the capture before it is stolen");
    // Another window took the capture: lParam is that window (a non-null handle other than ours).
    run(&ui, || {
        MESSAGE_TIME.with(|clock| clock.set(40));
        send(built().window.hwnd(), WM_CAPTURECHANGED, 0, 1);
    });
    assert!(!run(&ui, has_capture), "the controller gave its capture state back");
    run(&ui, move || {
        message(WM_MOUSEMOVE, MK_LBUTTON, cx - 200, cy, 48);
        message(WM_LBUTTONUP, 0, cx - 200, cy, 50);
        settle_step();
    });
    assert_eq!(
        run(&ui, horizontal_offset),
        panned,
        "after the lost capture neither a move nor the release pans"
    );
    assert_eq!(
        run(&ui, frame_subscribers),
        baseline,
        "a cancelled drag starts no glide"
    );
    assert!(!run(&ui, || input().input().stop_kinetic()));
    finish(ui);
}

// ---- single and double clicks ---------------------------------------------------------------------------------------------------------------------

/// Click-to-zoom off (a release without a drag does nothing), kinetic pan off, image at 100 %.
fn zoomed_without_click_zoom(ui: &UiThread) {
    run(ui, || {
        edit_settings(|s| {
            s.click_to_zoom_enabled = false;
            s.kinetic_pan_enabled = false;
        })
    });
    zoom_to_actual_size(ui);
    run(ui, own_the_frames);
}

fn click(x: i32, y: i32, down_at: i32, up_at: i32) {
    message(WM_LBUTTONDOWN, MK_LBUTTON, x, y, down_at);
    message(WM_LBUTTONUP, 0, x, y, up_at);
}

/// Lets the posted work of the last messages run (the frames are the test's: a few explicit ones).
fn let_work_run() {
    frames(4);
}

// C# `ClickCounter` + `PointerInputController.OnImagePress` (`IsFitDoubleClick`): two left presses within the double-click time and slop, counted from the
// real message times, are a double click on the zoomed image: it returns to Fit. One click alone changes nothing.
#[test]
#[ignore = "native: needs desktop"]
fn a_double_click_on_the_zoomed_image_returns_to_fit_and_a_single_click_does_not() {
    let ui = start_with_big_image_ui();
    zoomed_without_click_zoom(&ui);
    let (cx, cy) = run(&ui, centre);
    let offset = run(&ui, horizontal_offset);
    run(&ui, move || click(cx, cy, 1_000, 1_020));
    run(&ui, let_work_run);
    assert_eq!(run(&ui, zoom_percent), 100, "one click is not a double click");
    assert_eq!(
        run(&ui, horizontal_offset),
        offset,
        "a click without a drag pans nothing"
    );
    // The second press arrives 100 ms after the first, a pixel away.
    run(&ui, move || message(WM_LBUTTONDOWN, MK_LBUTTON, cx + 1, cy + 1, 1_100));
    run(&ui, let_work_run);
    assert!(
        run(&ui, || built().window.view_model().borrow().viewer().borrow().is_fit()),
        "the double click returns to Fit"
    );
    assert!(!run(&ui, has_capture), "the double click does not start a pan");
    run(&ui, move || message(WM_LBUTTONUP, 0, cx + 1, cy + 1, 1_110));
    finish(ui);
}

// The same two clicks, but the second comes after the double-click time: it is a new single click (a pan starts, nothing is fitted).
#[test]
#[ignore = "native: needs desktop"]
fn a_second_click_after_the_double_click_time_is_not_a_double_click() {
    let ui = start_with_big_image_ui();
    zoomed_without_click_zoom(&ui);
    let (cx, cy) = run(&ui, centre);
    run(&ui, move || click(cx, cy, 1_000, 1_020));
    run(&ui, move || {
        message(WM_LBUTTONDOWN, MK_LBUTTON, cx, cy, 1_000 + DOUBLE_CLICK_MS + 1);
    });
    assert!(
        run(&ui, has_capture),
        "a slow second click is a plain press: the pointer is captured for a pan"
    );
    run(&ui, move || message(WM_LBUTTONUP, 0, cx, cy, 1_600));
    run(&ui, let_work_run);
    assert_eq!(run(&ui, zoom_percent), 100, "no Fit");
    finish(ui);
}

// The second press is within the time but farther than the double-click slop from the first: not a double click either.
#[test]
#[ignore = "native: needs desktop"]
fn a_second_click_beyond_the_double_click_slop_is_not_a_double_click() {
    let ui = start_with_big_image_ui();
    zoomed_without_click_zoom(&ui);
    let (cx, cy) = run(&ui, centre);
    run(&ui, move || click(cx, cy, 1_000, 1_020));
    let far = DOUBLE_CLICK_SLOP.0; // the slop rectangle is centred on the first click: half of it is 4 px
    run(&ui, move || message(WM_LBUTTONDOWN, MK_LBUTTON, cx + far, cy, 1_100));
    assert!(
        run(&ui, has_capture),
        "a second press outside the slop is a plain press"
    );
    run(&ui, move || message(WM_LBUTTONUP, 0, cx + far, cy, 1_110));
    run(&ui, let_work_run);
    assert_eq!(run(&ui, zoom_percent), 100, "no Fit");
    finish(ui);
}
