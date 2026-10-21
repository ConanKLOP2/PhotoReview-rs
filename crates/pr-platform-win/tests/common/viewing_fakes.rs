//! Fakes of the viewing tests (W8-16h; 16f and 16g reuse them): a fake shell window (port of the `FakeWindow` of C# `ViewportControllerTests`), a FIFO
//! render poster (the `FakeDispatcher`), a fake host frame clock (the `FakeFrameClock`) and a fake display clock. UI-thread fakes: `Rc`/`Cell`, no sleeps.
#![allow(dead_code)] // shared by several test crates, each of which uses a different part

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Mutex;

use pr_core::abstractions::{DisplayClock, DisplayTiming};
use pr_platform_win::host::contracts::{
    CancelEventArgs, ClosingHandler, EventHandlerId, PointD, ShellCursor, ShellWindowApi, ShellWindowEvent, SizeD,
    WindowEventHandler, WindowMessageHandler,
};
use pr_platform_win::host::error::HostError;
use pr_platform_win::host::frame_clock::base::{FrameHandler, FrameHandlerId, FrameTick, HostFrameClock};
use pr_platform_win::shell_viewing::viewport_controller::RenderPoster;
use pr_platform_win::win32::structs::Hwnd;

use std::collections::BTreeMap;

use pr_app::input::primitives::RectD;
use pr_app::services::render::{DrawContext, GpuImage, GpuImageCache, RenderError, TextLayout};
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation};
use pr_imaging::pixels::PixelBuffer;

/// A handler slot shared between the window and a `raise` in progress.
type SharedHandler = Rc<RefCell<WindowEventHandler>>;

/// C# `FakeWindow`: 1000x600 DIP client at DPI scale 1.0, loaded, no capture.
pub struct FakeWindow {
    pub hwnd: Hwnd,
    pub dpi_scale: Cell<f64>,
    pub client_size: Cell<SizeD>,
    pub loaded: Cell<bool>,
    pub invalidations: Cell<u32>,
    pub releases: Cell<u32>,
    pub captures: Cell<u32>,
    pub cursor: Cell<Option<ShellCursor>>,
    pub has_capture: Cell<bool>,
    pub client_origin_pixel: Cell<PointD>,
    next_id: Cell<u64>,
    subscriptions: RefCell<Vec<(EventHandlerId, ShellWindowEvent, SharedHandler)>>,
}

impl FakeWindow {
    pub fn new() -> Rc<FakeWindow> {
        Rc::new(FakeWindow {
            hwnd: 0x1234,
            dpi_scale: Cell::new(1.0),
            client_size: Cell::new(SizeD {
                width: 1000.0,
                height: 600.0,
            }),
            loaded: Cell::new(true),
            invalidations: Cell::new(0),
            releases: Cell::new(0),
            captures: Cell::new(0),
            cursor: Cell::new(None),
            has_capture: Cell::new(false),
            client_origin_pixel: Cell::new(PointD::default()),
            next_id: Cell::new(0),
            subscriptions: RefCell::new(Vec::new()),
        })
    }

    /// C# `Resize`: changes the client size, then raises `ClientSizeChanged`.
    pub fn resize(&self, width: f64, height: f64) {
        self.client_size.set(SizeD { width, height });
        self.raise(ShellWindowEvent::ClientSizeChanged);
    }

    /// Number of live subscriptions of `event`.
    pub fn subscriber_count(&self, event: ShellWindowEvent) -> usize {
        self.subscriptions
            .borrow()
            .iter()
            .filter(|(_, known, _)| *known == event)
            .count()
    }

    /// Runs the handlers of `event` in subscription order (on a snapshot).
    pub fn raise(&self, event: ShellWindowEvent) {
        let handlers: Vec<SharedHandler> = self
            .subscriptions
            .borrow()
            .iter()
            .filter(|(_, known, _)| *known == event)
            .map(|(_, _, handler)| Rc::clone(handler))
            .collect();
        for handler in handlers {
            (handler.borrow_mut())();
        }
    }
}

impl ShellWindowApi for FakeWindow {
    fn hwnd(&self) -> Hwnd {
        self.hwnd
    }
    fn dpi_scale(&self) -> f64 {
        self.dpi_scale.get()
    }
    fn client_size_dip(&self) -> SizeD {
        self.client_size.get()
    }
    fn is_active(&self) -> bool {
        true
    }
    fn is_loaded(&self) -> bool {
        self.loaded.get()
    }
    fn subscribe(&self, event: ShellWindowEvent, handler: WindowEventHandler) -> EventHandlerId {
        self.next_id.set(self.next_id.get() + 1);
        let id = self.next_id.get();
        self.subscriptions
            .borrow_mut()
            .push((id, event, Rc::new(RefCell::new(handler))));
        id
    }
    fn subscribe_closing(&self, _handler: ClosingHandler) -> EventHandlerId {
        let _ = CancelEventArgs::default();
        self.next_id.set(self.next_id.get() + 1);
        self.next_id.get()
    }
    fn unsubscribe(&self, id: EventHandlerId) {
        self.subscriptions.borrow_mut().retain(|(known, _, _)| *known != id);
    }
    fn add_message_handler(&self, _handler: Rc<dyn WindowMessageHandler>) {}
    fn insert_message_handler_first(&self, _handler: Rc<dyn WindowMessageHandler>) {}
    fn is_destroyed(&self) -> bool {
        false
    }
    fn invalidate(&self) {
        self.invalidations.set(self.invalidations.get() + 1);
    }
    fn set_title(&self, _title: &str) -> Result<(), HostError> {
        Ok(())
    }
    fn set_cursor(&self, cursor: ShellCursor) {
        self.cursor.set(Some(cursor));
    }
    fn capture_pointer(&self) {
        self.captures.set(self.captures.get() + 1);
        self.has_capture.set(true);
    }
    fn release_pointer(&self) {
        self.has_capture.set(false);
        self.releases.set(self.releases.get() + 1);
    }
    fn has_pointer_capture(&self) -> bool {
        self.has_capture.get()
    }
    fn screen_to_client_dip(&self, screen_pixel: PointD) -> PointD {
        let origin = self.client_origin_pixel.get();
        let scale = self.dpi_scale.get();
        PointD {
            x: (screen_pixel.x - origin.x) / scale,
            y: (screen_pixel.y - origin.y) / scale,
        }
    }
    fn close(&self) {}
}

/// C# `FakeDispatcher` for the Render priority only: `post_render` queues, [`FakeRenderPoster::drain`] runs the jobs in FIFO order until the queue is
/// empty (a job may post more).
#[derive(Default)]
pub struct FakeRenderPoster {
    queue: RefCell<VecDeque<Box<dyn FnOnce()>>>,
    posts: Cell<usize>,
}

impl FakeRenderPoster {
    pub fn new() -> Rc<FakeRenderPoster> {
        Rc::new(FakeRenderPoster::default())
    }

    /// Total number of `post_render` calls so far (C# `PostedPriorities.Count`).
    pub fn posts(&self) -> usize {
        self.posts.get()
    }

    /// Jobs queued and not yet run.
    pub fn pending(&self) -> usize {
        self.queue.borrow().len()
    }

    /// Runs the oldest job; `false` when the queue is empty.
    pub fn run_one(&self) -> bool {
        let job = self.queue.borrow_mut().pop_front();
        match job {
            Some(job) => {
                job();
                true
            }
            None => false,
        }
    }

    /// Runs jobs until the queue is empty.
    pub fn drain(&self) {
        while self.run_one() {}
    }
}

impl RenderPoster for FakeRenderPoster {
    fn post_render(&self, job: Box<dyn FnOnce()>) {
        self.posts.set(self.posts.get() + 1);
        self.queue.borrow_mut().push_back(job);
    }
}

struct ClockSlot {
    id: FrameHandlerId,
    /// `None` while the handler runs.
    handler: Option<FrameHandler>,
    removed: bool,
}

/// C# `FakeFrameClock`: `tick(ms)` raises one frame to every subscriber; a handler may unsubscribe (even itself) while it runs.
#[derive(Default)]
pub struct FakeHostClock {
    slots: RefCell<Vec<ClockSlot>>,
    next_id: Cell<FrameHandlerId>,
    requests: Cell<u32>,
    disposed: Cell<bool>,
}

impl FakeHostClock {
    pub fn new() -> Rc<FakeHostClock> {
        Rc::new(FakeHostClock::default())
    }

    /// C# `SubscriberCount`.
    pub fn subscriber_count(&self) -> usize {
        self.slots.borrow().iter().filter(|slot| !slot.removed).count()
    }

    /// C# `FrameRequests`.
    pub fn frame_requests(&self) -> u32 {
        self.requests.get()
    }

    /// C# `Tick(renderingTimeMs)`.
    pub fn tick(&self, rendering_time_ms: f64) {
        let tick = FrameTick {
            timestamp: 0,
            rendering_time_ms,
            timing: None,
        };
        let ids: Vec<FrameHandlerId> = self
            .slots
            .borrow()
            .iter()
            .filter(|slot| !slot.removed)
            .map(|slot| slot.id)
            .collect();
        for id in ids {
            let taken = {
                let mut slots = self.slots.borrow_mut();
                slots
                    .iter_mut()
                    .find(|slot| slot.id == id && !slot.removed)
                    .and_then(|slot| slot.handler.take())
            };
            let Some(mut handler) = taken else { continue };
            handler(&tick);
            let mut slots = self.slots.borrow_mut();
            if let Some(slot) = slots.iter_mut().find(|slot| slot.id == id) {
                slot.handler = Some(handler);
            }
        }
        self.slots.borrow_mut().retain(|slot| !slot.removed);
    }
}

impl HostFrameClock for FakeHostClock {
    fn subscribe(&self, handler: FrameHandler) -> FrameHandlerId {
        self.next_id.set(self.next_id.get() + 1);
        let id = self.next_id.get();
        self.slots.borrow_mut().push(ClockSlot {
            id,
            handler: Some(handler),
            removed: false,
        });
        id
    }
    fn unsubscribe(&self, id: FrameHandlerId) {
        let mut slots = self.slots.borrow_mut();
        if let Some(slot) = slots.iter_mut().find(|slot| slot.id == id) {
            slot.removed = true;
        }
        // A slot whose handler is running is purged by `tick`; the others go now.
        slots.retain(|slot| !(slot.removed && slot.handler.is_some()));
    }
    fn request_frame(&self) {
        self.requests.set(self.requests.get() + 1);
    }
    fn is_armed(&self) -> bool {
        false
    }
    fn tick_count(&self) -> i64 {
        0
    }
    fn wait_handle(&self) -> isize {
        0
    }
    fn deadline_timestamp(&self) -> i64 {
        i64::MAX
    }
    fn ticks_until_deadline(&self) -> i64 {
        i64::MAX
    }
    fn on_signaled(&self) -> Result<bool, HostError> {
        Ok(false)
    }
    fn on_deadline(&self) -> Result<bool, HostError> {
        Ok(false)
    }
    fn dispose(&self) {
        self.disposed.set(true);
        self.slots.borrow_mut().clear();
    }
}

/// C# `FakeDisplayClock`: records the last window, answers `DisplayTiming { last_vblank: 5, refresh_period: 6 }`.
#[derive(Default)]
pub struct FakeDisplayClock {
    last_window: Mutex<Option<isize>>,
}

impl FakeDisplayClock {
    pub fn last_window(&self) -> Option<isize> {
        *self.last_window.lock().unwrap()
    }
}

impl DisplayClock for FakeDisplayClock {
    fn get_timing(&self, window: isize) -> Option<DisplayTiming> {
        *self.last_window.lock().unwrap() = Some(window);
        Some(DisplayTiming {
            last_vblank: 5,
            refresh_period: 6,
        })
    }
}

// ---- W8-16b compare/crossfade fakes (draw context recorder, GPU image, cache, frame clock) ----

#[derive(Default)]
pub struct FakeClock {
    handlers: RefCell<BTreeMap<FrameHandlerId, Rc<RefCell<FrameHandler>>>>,
    next: Cell<FrameHandlerId>,
    pub requests: Cell<usize>,
}

impl FakeClock {
    pub fn tick(&self, timestamp: i64) {
        let handlers: Vec<_> = self.handlers.borrow().values().cloned().collect();
        let tick = FrameTick {
            timestamp,
            rendering_time_ms: 0.0,
            timing: None,
        };
        for handler in handlers {
            handler.borrow_mut()(&tick);
        }
    }
    pub fn subscribers(&self) -> usize {
        self.handlers.borrow().len()
    }
}

impl HostFrameClock for FakeClock {
    fn subscribe(&self, handler: FrameHandler) -> FrameHandlerId {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.handlers.borrow_mut().insert(id, Rc::new(RefCell::new(handler)));
        id
    }
    fn unsubscribe(&self, id: FrameHandlerId) {
        self.handlers.borrow_mut().remove(&id);
    }
    fn request_frame(&self) {
        self.requests.set(self.requests.get() + 1);
    }
    fn is_armed(&self) -> bool {
        self.subscribers() > 0 || self.requests.get() > 0
    }
    fn tick_count(&self) -> i64 {
        0
    }
    fn wait_handle(&self) -> isize {
        0
    }
    fn deadline_timestamp(&self) -> i64 {
        i64::MAX
    }
    fn ticks_until_deadline(&self) -> i64 {
        i64::MAX
    }
    fn on_signaled(&self) -> Result<bool, pr_platform_win::host::error::HostError> {
        Ok(false)
    }
    fn on_deadline(&self) -> Result<bool, pr_platform_win::host::error::HostError> {
        Ok(false)
    }
    fn dispose(&self) {
        self.handlers.borrow_mut().clear();
    }
}

#[derive(Default)]
pub struct RecordingContext {
    pub calls: Vec<String>,
    pub depth: usize,
}

impl DrawContext for RecordingContext {
    fn clear(&mut self, _: ColorF) {
        self.calls.push("clear".into());
    }
    fn draw_image(&mut self, _: &dyn GpuImage, _: RectD, _: Option<RectD>, _: ImageInterpolation, opacity: f32) {
        self.calls.push(format!("image:{opacity}"));
    }
    fn fill_rectangle(&mut self, rect: RectD, _: ColorF) {
        self.calls.push(format!("fill:{rect:?}"));
    }
    fn fill_rounded_rectangle(&mut self, _: RectD, _: f64, _: ColorF) {}
    fn draw_rectangle(&mut self, _: RectD, _: ColorF, _: f64) {}
    fn draw_text(&mut self, _: &dyn TextLayout, _: PointD, _: ColorF) -> Result<(), RenderError> {
        Ok(())
    }
    fn push_clip(&mut self, _: RectD) {
        self.depth += 1;
        self.calls.push("push".into());
    }
    fn pop_clip(&mut self) -> Result<(), RenderError> {
        self.depth -= 1;
        self.calls.push("pop".into());
        Ok(())
    }
    fn push_opacity(&mut self, _: f32) {}
    fn pop_opacity(&mut self) -> Result<(), RenderError> {
        Ok(())
    }
    fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError> {
        Ok(None)
    }
}

struct FakeImage(i64);
impl GpuImage for FakeImage {
    fn pixel_buffer_id(&self) -> i64 {
        self.0
    }
    fn pixel_width(&self) -> i32 {
        1
    }
    fn pixel_height(&self) -> i32 {
        1
    }
    fn gpu_bytes(&self) -> i64 {
        4
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Default)]
pub struct FakeCache {
    pub fail: bool,
}
impl GpuImageCache for FakeCache {
    fn get_or_upload(&self, pixels: &PixelBuffer) -> Result<Rc<dyn GpuImage>, RenderError> {
        if self.fail {
            Err(RenderError::Device {
                hresult: -1,
                operation: "fake",
            })
        } else {
            Ok(Rc::new(FakeImage(pixels.id())))
        }
    }
    fn prefetch(&self, _: &std::sync::Arc<PixelBuffer>) -> Result<(), RenderError> {
        Ok(())
    }
    fn retain(&self, _: &[i64]) -> Result<(), RenderError> {
        Ok(())
    }
    fn budget_bytes(&self) -> i64 {
        0
    }
    fn set_budget_bytes(&self, _: i64) -> Result<(), RenderError> {
        Ok(())
    }
    fn clear(&self) {}
}
