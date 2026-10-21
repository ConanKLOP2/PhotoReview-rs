//! Fixture of the scroll-bar tests (W8-16f; 16g extends it): port of the C# `ScrollBarRig` (`ScrollBarTestSupport.cs`) without `ShellInput` and without the
//! draw recorder (both belong to later units). A fake window that keeps the message-handler chain (the shared `FakeWindow` ignores
//! `add_message_handler`), a real `ViewportController` over a fake render poster and a fake frame clock, a real `ViewerState`, the real
//! `ShellScrollBars`. UI-thread fakes, no sleeps.
#![allow(dead_code)] // shared by the test crates of 16f and 16g

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_app::input::primitives::RectD;
use pr_app::services::render::{DrawContext, GpuImage, RenderError, TextLayout};
use pr_app::viewmodels::viewer_state::ViewerState;
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation};
use pr_imaging::pixels::PixelBuffer;
use pr_platform_win::host::contracts::{
    ClosingHandler, EventHandlerId, PointD, ShellCursor, ShellWindowApi, ShellWindowEvent, SizeD, WindowEventHandler,
    WindowMessage, WindowMessageHandler,
};
use pr_platform_win::host::error::HostError;
use pr_platform_win::shell_viewing::scrollbars::{ScrollBarOptions, ShellScrollBars};
use pr_platform_win::shell_viewing::viewport_controller::{
    ViewportContent, ViewportController, ViewportControllerOptions,
};
use pr_platform_win::win32::structs::{Hwnd, Lparam, Lresult, Wparam};

use super::viewing_fakes::{FakeHostClock, FakeRenderPoster, FakeWindow};

/// A [`FakeWindow`] plus the message-handler chain (C# `FakeShellWindow.AddMessageHandler` / `Dispatch`). Everything else delegates.
pub struct ScrollWindow {
    pub inner: Rc<FakeWindow>,
    handlers: RefCell<Vec<Rc<dyn WindowMessageHandler>>>,
    /// 16g: replaces the window handle the bars see (`Some(0)` = no handle yet, C# `Hwnd == 0`).
    pub hwnd_override: Cell<Option<Hwnd>>,
    /// 16g: runs inside `capture_pointer` after the fake captured (a window whose capture re-dispatches `WM_CAPTURECHANGED`, contract T2).
    pub on_capture: RefCell<Option<Rc<dyn Fn()>>>,
}

impl ScrollWindow {
    pub fn new(inner: Rc<FakeWindow>) -> Rc<ScrollWindow> {
        Rc::new(ScrollWindow {
            inner,
            handlers: RefCell::new(Vec::new()),
            hwnd_override: Cell::new(None),
            on_capture: RefCell::new(None),
        })
    }

    /// 16g: forgets every registered handler (the bars then have no owner but the test).
    pub fn clear_handlers(&self) {
        self.handlers.borrow_mut().clear();
    }

    /// Number of registered message handlers.
    pub fn handler_count(&self) -> usize {
        self.handlers.borrow().len()
    }

    /// C# `Dispatch(message, wParam, lParam)`: runs the chain in registration order on a snapshot; the first `Some` wins.
    pub fn dispatch(&self, msg: u32, wparam: Wparam, lparam: Lparam) -> Option<Lresult> {
        let message = WindowMessage {
            hwnd: self.inner.hwnd,
            msg,
            wparam,
            lparam,
        };
        let snapshot: Vec<Rc<dyn WindowMessageHandler>> = self.handlers.borrow().clone();
        snapshot.iter().find_map(|handler| handler.try_handle(&message))
    }
}

impl ShellWindowApi for ScrollWindow {
    fn hwnd(&self) -> Hwnd {
        self.hwnd_override.get().unwrap_or_else(|| self.inner.hwnd())
    }
    fn dpi_scale(&self) -> f64 {
        self.inner.dpi_scale()
    }
    fn client_size_dip(&self) -> SizeD {
        self.inner.client_size_dip()
    }
    fn is_active(&self) -> bool {
        self.inner.is_active()
    }
    fn is_loaded(&self) -> bool {
        self.inner.is_loaded()
    }
    fn subscribe(&self, event: ShellWindowEvent, handler: WindowEventHandler) -> EventHandlerId {
        self.inner.subscribe(event, handler)
    }
    fn subscribe_closing(&self, handler: ClosingHandler) -> EventHandlerId {
        self.inner.subscribe_closing(handler)
    }
    fn unsubscribe(&self, id: EventHandlerId) {
        self.inner.unsubscribe(id);
    }
    fn add_message_handler(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.handlers.borrow_mut().push(handler);
    }
    fn insert_message_handler_first(&self, handler: Rc<dyn WindowMessageHandler>) {
        self.handlers.borrow_mut().insert(0, handler);
    }
    fn is_destroyed(&self) -> bool {
        self.inner.is_destroyed()
    }
    fn invalidate(&self) {
        self.inner.invalidate();
    }
    fn set_title(&self, title: &str) -> Result<(), HostError> {
        self.inner.set_title(title)
    }
    fn set_cursor(&self, cursor: ShellCursor) {
        self.inner.set_cursor(cursor);
    }
    fn capture_pointer(&self) {
        self.inner.capture_pointer();
        let hook = self.on_capture.borrow().clone();
        if let Some(hook) = hook {
            hook();
        }
    }
    fn release_pointer(&self) {
        self.inner.release_pointer();
    }
    fn has_pointer_capture(&self) -> bool {
        self.inner.has_pointer_capture()
    }
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        self.inner.screen_to_client_dip(screen_pixel)
    }
    fn close(&self) {
        self.inner.close();
    }
}

/// The constructor arguments of the C# `ScrollBarRig` (defaults: client 1000x600, image 3000x2000, DPI 1.0, actual size, with scroll bars).
#[derive(Clone, Copy)]
pub struct RigConfig {
    pub client_width: f64,
    pub client_height: f64,
    pub image_width: i32,
    pub image_height: i32,
    pub dpi_scale: f64,
    pub actual_size: bool,
    pub with_scroll_bars: bool,
}

impl Default for RigConfig {
    fn default() -> Self {
        RigConfig {
            client_width: 1000.0,
            client_height: 600.0,
            image_width: 3000,
            image_height: 2000,
            dpi_scale: 1.0,
            actual_size: true,
            with_scroll_bars: true,
        }
    }
}

/// C# `ScrollBarRig`.
pub struct ScrollBarRig {
    pub window: Rc<ScrollWindow>,
    pub poster: Rc<FakeRenderPoster>,
    pub clock: Rc<FakeHostClock>,
    pub viewer: Rc<RefCell<ViewerState>>,
    pub viewport: Rc<ViewportController>,
    pub bars: Option<Rc<ShellScrollBars>>,
    /// C# `Now` (ticks, 1000 per second).
    pub now: Rc<Cell<i64>>,
    /// C# `CompareVisible`.
    pub compare_visible: Rc<Cell<bool>>,
    /// C# `Presses`.
    pub presses: Rc<Cell<u32>>,
    /// C# `TrackedLeave`.
    pub tracked_leave: Rc<RefCell<Vec<isize>>>,
    bitmap: Rc<Cell<(f64, f64)>>,
}

impl ScrollBarRig {
    pub fn new() -> ScrollBarRig {
        ScrollBarRig::with(RigConfig::default())
    }

    pub fn with(config: RigConfig) -> ScrollBarRig {
        let fake = FakeWindow::new();
        fake.client_size.set(SizeD {
            width: config.client_width,
            height: config.client_height,
        });
        fake.dpi_scale.set(config.dpi_scale);
        let window = ScrollWindow::new(fake);
        let poster = FakeRenderPoster::new();
        let clock = FakeHostClock::new();
        let viewer = Rc::new(RefCell::new(ViewerState::new()));
        {
            let mut state = viewer.borrow_mut();
            state.set_dpi_scale(config.dpi_scale);
            state.set_source_size(config.image_width, config.image_height, true);
        }
        let bitmap = Rc::new(Cell::new((
            f64::from(config.image_width),
            f64::from(config.image_height),
        )));
        let now = Rc::new(Cell::new(0_i64));
        let compare_visible = Rc::new(Cell::new(false));
        let presses = Rc::new(Cell::new(0_u32));
        let tracked_leave = Rc::new(RefCell::new(Vec::new()));

        let content_viewer = Rc::clone(&viewer);
        let content_bitmap = Rc::clone(&bitmap);
        let content: Rc<dyn Fn() -> ViewportContent> = Rc::new(move || {
            let state = content_viewer.borrow();
            let (bitmap_width, bitmap_height) = content_bitmap.get();
            ViewportContent {
                stretch: state.stretch(),
                image_width: state.image_width(),
                image_height: state.image_height(),
                max_image_width: state.max_image_width(),
                max_image_height: state.max_image_height(),
                bitmap_width,
                bitmap_height,
                is_fit: state.is_fit(),
            }
        });
        // C# `UpdateFitSize` reads `Viewport.ViewportSize`; the controller does not exist yet when the closure is built.
        let controller_slot: Rc<RefCell<std::rc::Weak<ViewportController>>> = Rc::default();
        let fit_viewer = Rc::clone(&viewer);
        let fit_slot = Rc::clone(&controller_slot);
        let update_fit_size: Rc<dyn Fn()> = Rc::new(move || {
            let controller = fit_slot.borrow().upgrade();
            if let Some(controller) = controller {
                let (width, height) = controller.viewport_size();
                let update = fit_viewer.borrow_mut().update_viewport(width, height, false);
                if !update.changes.is_empty() {
                    controller.invalidate_layout();
                }
            }
        });
        let timestamp_now = Rc::clone(&now);
        let window_api: Rc<dyn ShellWindowApi> = window.clone();
        let viewport = ViewportController::new(
            Rc::clone(&window_api),
            poster.clone(),
            clock.clone(),
            content,
            update_fit_size,
            ViewportControllerOptions {
                drag_metrics_pixels: Box::new(|| (4, 4)),
                timestamp: Box::new(move || timestamp_now.get()),
                ..ViewportControllerOptions::default()
            },
        );
        *controller_slot.borrow_mut() = Rc::downgrade(&viewport);

        let bars = config.with_scroll_bars.then(|| {
            let compare = Rc::clone(&compare_visible);
            let press_counter = Rc::clone(&presses);
            let leave_log = Rc::clone(&tracked_leave);
            ShellScrollBars::new(
                &window_api,
                Rc::clone(&viewport),
                Rc::new(move || compare.get()),
                Some(Rc::new(move || press_counter.set(press_counter.get() + 1))),
                Some(ScrollBarOptions {
                    repeat_timing: Rc::new(|| (500, 50)),
                    track_mouse_leave: Rc::new(move |hwnd| leave_log.borrow_mut().push(hwnd)),
                    ticks_per_second: 1000,
                    ..ScrollBarOptions::default()
                }),
            )
        });

        let rig = ScrollBarRig {
            window,
            poster,
            clock,
            viewer,
            viewport,
            bars,
            now,
            compare_visible,
            presses,
            tracked_leave,
            bitmap,
        };
        rig.drain();
        if config.actual_size {
            rig.viewer.borrow_mut().zoom_to_actual_size();
        } else {
            rig.viewer
                .borrow_mut()
                .reset_fit(config.client_width, config.client_height);
        }
        rig.viewport.invalidate_layout();
        rig.drain();
        rig
    }

    /// The scroll bars (the rig was built with them).
    pub fn bars(&self) -> &Rc<ShellScrollBars> {
        self.bars.as_ref().expect("rig built with scroll bars")
    }

    /// C# `Drain()`: runs the queued Render-priority jobs.
    pub fn drain(&self) {
        self.poster.drain();
    }

    /// C# `Viewer.SetZoom(zoom)` + the `PropertyChanged -> InvalidateLayout` binding + `Drain()`.
    pub fn set_zoom(&self, zoom: f64) {
        self.viewer.borrow_mut().set_zoom(zoom);
        self.viewport.invalidate_layout();
        self.drain();
    }

    /// C# `Window.Resize(width, height)` + `Drain()`.
    pub fn resize(&self, width: f64, height: f64) {
        self.window.inner.resize(width, height);
        self.drain();
    }

    /// C# `Layout`.
    pub fn layout(&self) -> pr_app::viewing::scrollbar_geometry::ScrollBarLayout {
        self.bars().current_layout()
    }

    /// C# `SetBitmap`: the displayed bitmap is replaced by one of another pixel size.
    pub fn set_bitmap(&self, width: f64, height: f64) {
        self.bitmap.set((width, height));
        self.viewport.invalidate_layout();
        self.drain();
    }
}

// ---- 16g additions (mouse helpers, the fill recorder) ----

/// C# `FillRecorder` (`ScrollBarTestSupport.cs`): `Rects` / `Rounded` / `Clips` / `ClipDepth`; `pop_failures` makes `pop_clip` return an error.
#[derive(Default)]
pub struct FillRecorder {
    pub rects: Vec<(RectD, ColorF)>,
    pub rounded: Vec<(RectD, f64, ColorF)>,
    pub clips: Vec<RectD>,
    pub clip_depth: i32,
    pub pops: u32,
    pub pop_failures: bool,
    /// Every call in order: `"fill"`, `"round"`, `"push"`, `"pop"`.
    pub order: Vec<&'static str>,
}

impl DrawContext for FillRecorder {
    fn clear(&mut self, _: ColorF) {}
    fn draw_image(&mut self, _: &dyn GpuImage, _: RectD, _: Option<RectD>, _: ImageInterpolation, _: f32) {}
    fn fill_rectangle(&mut self, rect: RectD, color: ColorF) {
        self.order.push("fill");
        self.rects.push((rect, color));
    }
    fn fill_rounded_rectangle(&mut self, rect: RectD, radius: f64, color: ColorF) {
        self.order.push("round");
        self.rounded.push((rect, radius, color));
    }
    fn draw_rectangle(&mut self, _: RectD, _: ColorF, _: f64) {}
    fn draw_text(&mut self, _: &dyn TextLayout, _: PointD, _: ColorF) -> Result<(), RenderError> {
        Ok(())
    }
    fn push_clip(&mut self, rect: RectD) {
        self.order.push("push");
        self.clips.push(rect);
        self.clip_depth += 1;
    }
    fn pop_clip(&mut self) -> Result<(), RenderError> {
        self.order.push("pop");
        self.pops += 1;
        self.clip_depth -= 1;
        if self.pop_failures {
            Err(RenderError::InvalidOperation {
                message: "PopClip without PushClip.",
            })
        } else {
            Ok(())
        }
    }
    fn push_opacity(&mut self, _: f32) {}
    fn pop_opacity(&mut self) -> Result<(), RenderError> {
        Ok(())
    }
    fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError> {
        Ok(None)
    }
}

impl ScrollBarRig {
    /// C# `Pack(x, y)`: `(y << 16) | x` as an unsigned 32-bit value (zero-extended, like `(nint)(uint)`); negative values keep their low 16 bits.
    pub fn pack(x: i32, y: i32) -> Lparam {
        ((u32::from(y as u16) << 16) | u32::from(x as u16)) as Lparam
    }

    /// C# `Mouse(message, x, y)`: dispatches with PIXEL client coordinates through the handler chain, then drains.
    pub fn mouse(&self, msg: u32, x: f64, y: f64) -> Option<Lresult> {
        let outcome = self
            .window
            .dispatch(msg, 0x0001, ScrollBarRig::pack(x.round() as i32, y.round() as i32));
        self.drain();
        outcome
    }

    pub fn down(&self, x: f64, y: f64) -> Option<Lresult> {
        self.mouse(pr_platform_win::win32::messages::WM_LBUTTONDOWN, x, y)
    }

    pub fn move_to(&self, x: f64, y: f64) -> Option<Lresult> {
        self.mouse(pr_platform_win::win32::messages::WM_MOUSEMOVE, x, y)
    }

    pub fn up(&self, x: f64, y: f64) -> Option<Lresult> {
        self.mouse(pr_platform_win::win32::messages::WM_LBUTTONUP, x, y)
    }

    /// C# `TickAt(ms)`: the clock `Now` becomes `ms`, one frame is delivered, the queue is drained.
    pub fn tick_at(&self, ms: i64) {
        self.now.set(ms);
        self.clock.tick(16.0);
        self.drain();
    }

    /// C# `Draw()`: draws into a fresh recorder.
    pub fn draw(&self) -> FillRecorder {
        let mut recorder = FillRecorder::default();
        self.bars().draw(&mut recorder).expect("draw");
        recorder
    }
}
