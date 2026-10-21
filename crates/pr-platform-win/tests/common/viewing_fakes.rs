//! Shared deterministic viewing fakes for W8-16b and later viewing units.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use pr_app::input::primitives::{PointD, RectD};
use pr_app::services::render::{DrawContext, GpuImage, GpuImageCache, RenderError, TextLayout};
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation};
use pr_imaging::pixels::PixelBuffer;
use pr_platform_win::host::frame_clock::base::{FrameHandler, FrameHandlerId, FrameTick, HostFrameClock};

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
