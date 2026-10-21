//! Port of `PhotoReview.Shell.Win32/Viewing/CrossfadeLayer.cs` (165 lines, WP-21 Q-R37, baseline `d92d6aae`). Unit W8-16b (contract `contracts/W8-16b.md`).
//!
//! The OLD image of an image crossfade (WPF `OutgoingImage`): it keeps the frame the user saw (pixel buffer + the destination box last drawn,
//! scroll offset included) while the new image is already fully shown underneath; only this layer's opacity runs 1 -> 0 with
//! `Easing::QuadraticOut` over `ImageTransitionMs`. A new navigation mid-fade CANCELS the running tween and restarts from 1 with the newer outgoing
//! image (never queued). The frame clock is injected, so tests are deterministic and an idle layer holds no clock subscription.
//!
//! The tween maths is `pr_app::overlay::animator::Animator`, which does not subscribe to a clock itself (W8-15b): this layer owns the ONE frame
//! subscription, forwards each tick, and unsubscribes when the animator is idle. UI thread only; the layer is `!Send`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::RectD;
use pr_app::overlay::animator::{Animator, OpacityStore};
use pr_app::overlay::contracts::Easing;
use pr_app::services::render::{DrawContext, GpuImageCache, RenderError};
use pr_app::viewing::image_layer::try_draw;
use pr_app::viewport::render_geometry::ImageInterpolation;
use pr_imaging::pixels::PixelBuffer;

use crate::host::frame_clock::base::{FrameHandlerId, FrameTick, HostFrameClock, qpc_frequency};

/// C# `CrossfadeLayer.ElementId`: the id of the layer in the animator.
pub const ELEMENT_ID: &str = "OutgoingImage";

/// Use of a disposed layer (C# `ObjectDisposedException` of `Start`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossfadeError {
    Disposed,
}

/// C# `CrossfadeLayer`. Share it as `Rc<CrossfadeLayer>`.
pub struct CrossfadeLayer {
    inner: Rc<Inner>,
}

struct Inner {
    clock: Rc<dyn HostFrameClock>,
    invalidate: Rc<dyn Fn()>,
    animator: RefCell<Animator>,
    outgoing: RefCell<Option<Arc<PixelBuffer>>>,
    frame: Cell<RectD>,
    interpolation: Cell<ImageInterpolation>,
    opacity: Cell<f32>,
    start_count: Cell<i32>,
    disposed: Cell<bool>,
    frame_subscription: Cell<Option<FrameHandlerId>>,
}

struct Store<'a>(&'a Inner);

impl OpacityStore for Store<'_> {
    fn opacity(&self, element_id: &str) -> Option<f32> {
        (element_id == ELEMENT_ID && self.0.outgoing.borrow().is_some()).then(|| self.0.opacity.get())
    }
    fn set_opacity(&mut self, element_id: &str, opacity: f32) {
        if element_id == ELEMENT_ID {
            self.0.opacity.set(opacity);
        }
    }
}

impl Inner {
    fn unsubscribe_if_idle(&self) {
        let idle = !self.animator.borrow().is_animating();
        if idle {
            if let Some(id) = self.frame_subscription.take() {
                self.clock.unsubscribe(id);
            }
        }
    }

    fn release(&self) {
        self.outgoing.borrow_mut().take();
        self.opacity.set(0.0);
    }

    fn on_frame(&self, tick: &FrameTick) {
        let effect = self.animator.borrow_mut().on_frame(&mut Store(self), tick);
        self.unsubscribe_if_idle();
        if effect.changed {
            if self.opacity.get() <= 0.0 {
                self.release();
            }
            (self.invalidate)();
        }
    }
}

impl std::fmt::Debug for CrossfadeLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrossfadeLayer").finish_non_exhaustive()
    }
}

impl CrossfadeLayer {
    /// C# constructor `(clock, invalidate, ticksPerSecond = 0)`: `ticks_per_second <= 0` means the QPC frequency
    /// (`host::frame_clock::base::qpc_frequency()`, the unit of `HostFrameTick::timestamp`). `invalidate` is called after every frame in which the
    /// opacity changed (the host redraws).
    pub fn new(clock: Rc<dyn HostFrameClock>, invalidate: Rc<dyn Fn()>, ticks_per_second: i64) -> Self {
        let frequency = if ticks_per_second > 0 {
            ticks_per_second
        } else {
            qpc_frequency()
        };
        let animator = match Animator::new(frequency) {
            Ok(animator) => animator,
            Err(_) => match Animator::new(1_000_000_000) {
                Ok(animator) => animator,
                Err(_) => unreachable!("positive fallback frequency"),
            },
        };
        Self {
            inner: Rc::new(Inner {
                clock,
                invalidate,
                animator: RefCell::new(animator),
                outgoing: RefCell::new(None),
                frame: Cell::new(RectD::default()),
                interpolation: Cell::new(ImageInterpolation::Linear),
                opacity: Cell::new(0.0),
                start_count: Cell::new(0),
                disposed: Cell::new(false),
                frame_subscription: Cell::new(None),
            }),
        }
    }

    /// C# `Opacity` getter (0..=1; 0 or no image = not drawn).
    pub fn opacity(&self) -> f32 {
        self.inner.opacity.get()
    }

    /// C# `Opacity` setter (the animator's write path; tests use it).
    pub fn set_opacity(&self, opacity: f32) {
        self.inner.opacity.set(opacity);
    }

    /// C# `IsActive`: an outgoing image is held (fading).
    pub fn is_active(&self) -> bool {
        self.inner.outgoing.borrow().is_some()
    }

    /// C# `StartCount`: how many fades were started (diagnostics, tests).
    pub fn start_count(&self) -> i32 {
        self.inner.start_count.get()
    }

    /// C# `Outgoing`.
    pub fn outgoing(&self) -> Option<Arc<PixelBuffer>> {
        self.inner.outgoing.borrow().clone()
    }

    /// C# `Frame`: the destination box frozen at `start`.
    pub fn frame(&self) -> RectD {
        self.inner.frame.get()
    }

    /// C# `OutgoingBufferId`: `PixelBuffer::id()` of the outgoing image while fading and not disposed (the cache must keep its bitmap).
    pub fn outgoing_buffer_id(&self) -> Option<i64> {
        self.inner
            .outgoing
            .borrow()
            .as_ref()
            .filter(|pixels| !pixels.is_disposed())
            .map(|pixels| pixels.id())
    }

    /// C# `Start(outgoing, frame, interpolation, duration)`. `Ok(false)` (nothing changes, no subscription) when the buffer is disposed, the duration
    /// is zero or below one 100 ns tick, the frame has `width <= 0` or `height <= 0`, or any frame component is not finite. Otherwise: cancel the
    /// running tween at once, hold `outgoing` / `frame` / `interpolation`, opacity 1, `start_count += 1`, `fade_to(ELEMENT_ID, 0, duration, QuadraticOut)`;
    /// `Ok(true)`. `Err(Disposed)` after `dispose` (checked first).
    pub fn start(
        &self,
        outgoing: Arc<PixelBuffer>,
        frame: RectD,
        interpolation: ImageInterpolation,
        duration: Duration,
    ) -> Result<bool, CrossfadeError> {
        let inner = &self.inner;
        if inner.disposed.get() {
            return Err(CrossfadeError::Disposed);
        }
        let ticks = i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX);
        if outgoing.is_disposed()
            || ticks <= 0
            || !(frame.width > 0.0 && frame.height > 0.0)
            || ![frame.x, frame.y, frame.width, frame.height]
                .into_iter()
                .all(f64::is_finite)
        {
            return Ok(false);
        }
        inner.animator.borrow_mut().cancel(ELEMENT_ID);
        inner.unsubscribe_if_idle();
        *inner.outgoing.borrow_mut() = Some(outgoing);
        inner.frame.set(frame);
        inner.interpolation.set(interpolation);
        inner.opacity.set(1.0);
        inner.start_count.set(inner.start_count.get().wrapping_add(1));
        let effect = inner
            .animator
            .borrow_mut()
            .fade_to(&mut Store(inner), ELEMENT_ID, 0.0, ticks, Easing::QuadraticOut)
            .map_err(|_| CrossfadeError::Disposed)?;
        if effect.request_frame {
            if inner.frame_subscription.get().is_none() {
                let weak: Weak<Inner> = Rc::downgrade(inner);
                let id = inner.clock.subscribe(Box::new(move |tick| {
                    if let Some(inner) = weak.upgrade() {
                        inner.on_frame(tick);
                    }
                }));
                inner.frame_subscription.set(Some(id));
            }
            inner.clock.request_frame();
        }
        Ok(true)
    }

    /// C# `Cancel()`: stop the tween (and the clock subscription) and release the outgoing image (opacity 0).
    pub fn cancel(&self) {
        self.inner.animator.borrow_mut().cancel(ELEMENT_ID);
        self.inner.unsubscribe_if_idle();
        self.inner.release();
    }

    /// C# `Draw(cache, context, viewport)`: nothing held -> `Ok(false)`; the buffer is disposed or (opacity <= 0 and the animator idle) -> release,
    /// `Ok(false)`; opacity <= 0 -> `Ok(false)` (still animating: the next frame decides); otherwise push the viewport clip, `try_draw(.., frame,
    /// interpolation, opacity)`, pop the clip. A draw that returns `false` (the buffer was just disposed) cancels the layer and returns `Ok(false)`.
    /// A GPU / draw-context error is returned after the clip is popped; the layer is left as it was.
    pub fn draw(
        &self,
        cache: &dyn GpuImageCache,
        context: &mut dyn DrawContext,
        viewport: RectD,
    ) -> Result<bool, RenderError> {
        let Some(outgoing) = self.outgoing() else {
            return Ok(false);
        };
        if outgoing.is_disposed() || (self.opacity() <= 0.0 && !self.inner.animator.borrow().is_animating()) {
            self.inner.release();
            return Ok(false);
        }
        if self.opacity() <= 0.0 {
            return Ok(false);
        }
        context.push_clip(viewport);
        let drawn = try_draw(
            cache,
            context,
            &outgoing,
            self.frame(),
            self.inner.interpolation.get(),
            self.opacity(),
            1,
        );
        let popped = context.pop_clip();
        let drawn = drawn?;
        popped?;
        if !drawn {
            self.cancel();
        }
        Ok(drawn)
    }

    /// C# `Dispose()`: idempotent; unsubscribes, disposes the animator, releases the outgoing image. `Drop` calls it.
    pub fn dispose(&self) {
        if self.inner.disposed.replace(true) {
            return;
        }
        if let Some(id) = self.inner.frame_subscription.take() {
            self.inner.clock.unsubscribe(id);
        }
        self.inner.animator.borrow_mut().dispose();
        self.inner.release();
    }
}

impl Drop for CrossfadeLayer {
    fn drop(&mut self) {
        self.dispose();
    }
}
