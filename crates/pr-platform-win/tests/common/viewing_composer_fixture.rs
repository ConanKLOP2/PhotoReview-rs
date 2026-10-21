//! Fixture of the `PresentationComposer` tests (W8-16c): a probe cache (`GpuImageCache` + `PrefetchStatus`) with background "stripes" that the test
//! finishes by hand, a recording draw context, and a rig that wires one composer to a fake clock, a manual deadline scheduler and a settable timestamp.
//! UI-thread fakes (`Rc`/`Cell`), no sleeps. Needs `viewing_fakes` (declared as a sibling module by the including test) for `FakeClock`.
#![allow(dead_code)] // each test file uses a different part

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::services::render::gpu_image::PrefetchStatus;
use pr_app::services::render::{DrawContext, GpuImage, GpuImageCache, RenderError, TextLayout};
use pr_app::viewport::render_geometry::{ColorF, ImageInterpolation};
use pr_core::model::enums::ScalingQuality;
use pr_imaging::pixels::{PixelBuffer, PixelLayout};
use pr_platform_win::shell_viewing::compare::CompareFrame;
use pr_platform_win::shell_viewing::composer::{
    ComposedFrame, ComposerOptions, PresentationComposer, PresentationInputs, ScheduleOnUi,
};

use super::viewing_fakes::FakeClock;

/// Number of "stripes" a prefetch needs before it is complete (the C# test cache used 10 jobs per 100 px image).
pub const STRIPES: u32 = 4;

struct ProbeImage(i64);
impl GpuImage for ProbeImage {
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

type UploadHandler = Rc<dyn Fn(i64)>;

/// A GPU cache double: `prefetch` queues the image, [`Probe::finish_next`] runs one stripe (the last one completes the image and raises
/// `UploadCompleted`), [`Probe::device_lost`] forgets everything.
#[derive(Default)]
pub struct Probe {
    pub uploaded: RefCell<BTreeSet<i64>>,
    /// Prefetches waiting for stripes: `(id, stripes left)`, oldest first.
    pub queue: RefCell<VecDeque<(i64, u32)>>,
    pub prefetch_calls: RefCell<Vec<i64>>,
    pub get_calls: RefCell<Vec<i64>>,
    handlers: RefCell<BTreeMap<u64, UploadHandler>>,
    next_handler: Cell<u64>,
    /// `prefetch` completes the image at once and raises `UploadCompleted` from inside the call (a tiny image).
    pub complete_on_prefetch: Cell<bool>,
    pub fail_get: Cell<bool>,
    pub fail_prefetch: Cell<bool>,
}

impl Probe {
    pub fn new() -> Rc<Probe> {
        Rc::new(Probe::default())
    }

    pub fn handler_count(&self) -> usize {
        self.handlers.borrow().len()
    }

    /// Marks `id` as fully uploaded without an event (C# `cache.GetOrUpload(image)` before the upgrade).
    pub fn preload(&self, id: i64) {
        self.uploaded.borrow_mut().insert(id);
    }

    /// Raises `UploadCompleted(id)` on a snapshot of the handlers.
    pub fn raise(&self, id: i64) {
        let handlers: Vec<Rc<dyn Fn(i64)>> = self.handlers.borrow().values().cloned().collect();
        for handler in handlers {
            handler(id);
        }
    }

    /// One background stripe of the oldest queued prefetch; `false` when the queue is empty.
    pub fn finish_next(&self) -> bool {
        let finished = {
            let mut queue = self.queue.borrow_mut();
            let Some((id, left)) = queue.pop_front() else {
                return false;
            };
            if left > 1 {
                queue.push_front((id, left - 1));
                None
            } else {
                Some(id)
            }
        };
        if let Some(id) = finished {
            self.uploaded.borrow_mut().insert(id);
            self.raise(id);
        }
        true
    }

    /// Runs stripes until the queue is empty.
    pub fn finish_all(&self) {
        while self.finish_next() {}
    }

    /// The GPU is lost: every bitmap is gone and the queued stripes belong to dead images.
    pub fn device_lost(&self) {
        self.uploaded.borrow_mut().clear();
        self.queue.borrow_mut().clear();
    }
}

impl GpuImageCache for Probe {
    fn get_or_upload(&self, pixels: &PixelBuffer) -> Result<Rc<dyn GpuImage>, RenderError> {
        if self.fail_get.get() {
            return Err(RenderError::Device {
                hresult: -1,
                operation: "probe get_or_upload",
            });
        }
        if pixels.is_disposed() {
            return Err(RenderError::Disposed { object: "PixelBuffer" });
        }
        self.get_calls.borrow_mut().push(pixels.id());
        self.uploaded.borrow_mut().insert(pixels.id());
        Ok(Rc::new(ProbeImage(pixels.id())))
    }

    fn prefetch(&self, pixels: &Arc<PixelBuffer>) -> Result<(), RenderError> {
        if self.fail_prefetch.get() {
            return Err(RenderError::Device {
                hresult: -2,
                operation: "probe prefetch",
            });
        }
        if pixels.is_disposed() {
            return Ok(());
        }
        let id = pixels.id();
        self.prefetch_calls.borrow_mut().push(id);
        if self.uploaded.borrow().contains(&id) {
            return Ok(());
        }
        if self.complete_on_prefetch.get() {
            self.uploaded.borrow_mut().insert(id);
            self.raise(id);
        } else {
            self.queue.borrow_mut().push_back((id, STRIPES));
        }
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
    fn clear(&self) {
        self.uploaded.borrow_mut().clear();
    }
}

impl PrefetchStatus for Probe {
    fn is_fully_uploaded(&self, pixel_buffer_id: i64) -> bool {
        self.uploaded.borrow().contains(&pixel_buffer_id)
    }
    fn subscribe_upload_completed(&self, handler: Rc<dyn Fn(i64)>) -> u64 {
        let id = self.next_handler.get() + 1;
        self.next_handler.set(id);
        self.handlers.borrow_mut().insert(id, handler);
        id
    }
    fn unsubscribe_upload_completed(&self, id: u64) {
        self.handlers.borrow_mut().remove(&id);
    }
}

/// One recorded draw-context call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Push(RectD),
    Pop,
    Image {
        id: i64,
        destination: RectD,
        interpolation: ImageInterpolation,
        opacity: f32,
    },
    Fill(RectD, ColorF),
}

/// A `DrawContext` that records the calls the composer cares about and counts the clip depth.
#[derive(Default)]
pub struct Rec {
    pub calls: Vec<Call>,
    pub depth: i32,
    pub max_depth: i32,
}

impl Rec {
    pub fn images(&self) -> Vec<(i64, RectD, ImageInterpolation, f32)> {
        self.calls
            .iter()
            .filter_map(|call| match call {
                Call::Image {
                    id,
                    destination,
                    interpolation,
                    opacity,
                } => Some((*id, *destination, *interpolation, *opacity)),
                _ => None,
            })
            .collect()
    }

    pub fn fills(&self) -> Vec<(RectD, ColorF)> {
        self.calls
            .iter()
            .filter_map(|call| match call {
                Call::Fill(rect, color) => Some((*rect, *color)),
                _ => None,
            })
            .collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.calls
            .iter()
            .map(|call| match call {
                Call::Push(_) => "push",
                Call::Pop => "pop",
                Call::Image { .. } => "image",
                Call::Fill(..) => "fill",
            })
            .collect()
    }

    pub fn pushed(&self) -> Vec<RectD> {
        self.calls
            .iter()
            .filter_map(|call| match call {
                Call::Push(rect) => Some(*rect),
                _ => None,
            })
            .collect()
    }
}

impl DrawContext for Rec {
    fn clear(&mut self, _: ColorF) {}
    fn draw_image(
        &mut self,
        image: &dyn GpuImage,
        destination: RectD,
        _: Option<RectD>,
        interpolation: ImageInterpolation,
        opacity: f32,
    ) {
        self.calls.push(Call::Image {
            id: image.pixel_buffer_id(),
            destination,
            interpolation,
            opacity,
        });
    }
    fn fill_rectangle(&mut self, rect: RectD, color: ColorF) {
        self.calls.push(Call::Fill(rect, color));
    }
    fn fill_rounded_rectangle(&mut self, _: RectD, _: f64, _: ColorF) {}
    fn draw_rectangle(&mut self, _: RectD, _: ColorF, _: f64) {}
    fn draw_text(&mut self, _: &dyn TextLayout, _: PointD, _: ColorF) -> Result<(), RenderError> {
        Ok(())
    }
    fn push_clip(&mut self, rect: RectD) {
        self.depth += 1;
        self.max_depth = self.max_depth.max(self.depth);
        self.calls.push(Call::Push(rect));
    }
    fn pop_clip(&mut self) -> Result<(), RenderError> {
        self.depth -= 1;
        self.calls.push(Call::Pop);
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

/// A deadline given to `schedule_on_ui`.
pub struct Scheduled {
    pub delay: Duration,
    pub job: Option<Box<dyn FnOnce()>>,
}

pub fn client() -> SizeD {
    SizeD {
        width: 1000.0,
        height: 700.0,
    }
}

pub fn the_box() -> RectD {
    RectD {
        x: 100.0,
        y: 50.0,
        width: 800.0,
        height: 600.0,
    }
}

pub fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

pub fn buffer(width: i32, height: i32) -> Arc<PixelBuffer> {
    Arc::new(PixelBuffer::allocate(width, height, PixelLayout::Pbgra32).unwrap())
}

pub fn disposed_buffer() -> Arc<PixelBuffer> {
    let mut pixels = PixelBuffer::allocate(8, 8, PixelLayout::Pbgra32).unwrap();
    pixels.dispose();
    Arc::new(pixels)
}

pub fn inputs(destination: RectD) -> PresentationInputs<'static> {
    PresentationInputs {
        viewport: client(),
        main_destination: destination,
        effective_quality: ScalingQuality::HighQuality,
        compare_quality: ScalingQuality::HighQuality,
        dpi_scale: 1.0,
        compare_visible: false,
        compare: CompareFrame::empty(),
    }
}

/// One composer over the probe, a fake clock (1 tick = 1 ms), a manual deadline scheduler and a settable timestamp.
pub struct Rig {
    pub probe: Rc<Probe>,
    pub clock: Rc<FakeClock>,
    pub now: Rc<Cell<i64>>,
    pub invalidations: Rc<Cell<u32>>,
    pub scheduled: Rc<RefCell<Vec<Scheduled>>>,
    pub composer: Rc<PresentationComposer>,
}

impl Rig {
    pub fn new() -> Rig {
        Rig::with(120, 1_000)
    }

    pub fn with(settle_ms: i32, ticks_per_second: i64) -> Rig {
        let probe = Probe::new();
        let clock = Rc::new(FakeClock::default());
        let now = Rc::new(Cell::new(1_000_000_i64));
        let invalidations = Rc::new(Cell::new(0_u32));
        let scheduled: Rc<RefCell<Vec<Scheduled>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&scheduled);
        let schedule: ScheduleOnUi =
            Rc::new(move |delay, job| sink.borrow_mut().push(Scheduled { delay, job: Some(job) }));
        let clock_now = Rc::clone(&now);
        let counter = Rc::clone(&invalidations);
        let composer = PresentationComposer::new(
            Rc::clone(&probe) as Rc<dyn PrefetchStatus>,
            Rc::clone(&clock) as Rc<dyn pr_platform_win::host::frame_clock::base::HostFrameClock>,
            Rc::new(move || counter.set(counter.get() + 1)),
            schedule,
            ComposerOptions {
                timestamp: Some(Rc::new(move || clock_now.get())),
                ticks_per_second,
                settle_ms,
            },
        );
        Rig {
            probe,
            clock,
            now,
            invalidations,
            scheduled,
            composer: Rc::new(composer),
        }
    }

    pub fn advance(&self, ticks: i64) {
        self.now.set(self.now.get() + ticks);
    }

    /// Runs deadline `index` (once; a second call is a no-op).
    pub fn fire(&self, index: usize) {
        let job = self
            .scheduled
            .borrow_mut()
            .get_mut(index)
            .and_then(|slot| slot.job.take());
        if let Some(job) = job {
            job();
        }
    }

    /// Deadlines not fired yet.
    pub fn open_deadlines(&self) -> usize {
        self.scheduled.borrow().iter().filter(|slot| slot.job.is_some()).count()
    }

    pub fn draw(&self, input: &PresentationInputs<'_>) -> (ComposedFrame, Rec) {
        let mut rec = Rec::default();
        let frame = self.composer.draw(&mut rec, input).unwrap();
        (frame, rec)
    }
}
