//! Port of `PhotoReview.Shell.Win32/Viewing/CrossfadeLayer.cs` (165 lines, WP-21 Q-R37, baseline `d92d6aae`). Unit W8-16b (contract `contracts/W8-16b.md`).
//!
//! The OLD image of an image crossfade (WPF `OutgoingImage`): it keeps the frame the user saw (pixel buffer + the destination box last drawn,
//! scroll offset included) while the new image is already fully shown underneath; only this layer's opacity runs 1 -> 0 with
//! `Easing::QuadraticOut` over `ImageTransitionMs`. A new navigation mid-fade CANCELS the running tween and restarts from 1 with the newer outgoing
//! image (never queued). The frame clock is injected, so tests are deterministic and an idle layer holds no clock subscription.
//!
//! The tween maths is `pr_app::overlay::animator::Animator`, which does not subscribe to a clock itself (W8-15b): this layer owns the ONE frame
//! subscription, forwards each tick, and unsubscribes when the animator is idle. UI thread only; the layer is `!Send`.

// Contract stub: the bodies are `todo!()` until the W8-16b port; removed with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::frame_clock::FrameTick as AppFrameTick;
use pr_app::input::primitives::RectD;
use pr_app::services::render::{DrawContext, GpuImageCache, RenderError};
use pr_app::viewport::render_geometry::ImageInterpolation;
use pr_imaging::pixels::PixelBuffer;

use crate::host::frame_clock::base::{FrameTick as HostFrameTick, HostFrameClock};

/// C# `CrossfadeLayer.ElementId`: the id of the layer in the animator.
pub const ELEMENT_ID: &str = "OutgoingImage";

/// Use of a disposed layer (C# `ObjectDisposedException` of `Start`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossfadeError {
    Disposed,
}

/// The host `FrameTick` as the `pr_app` one the animator takes (interim twins, D-W8B3H-1; field for field).
pub fn to_app_tick(tick: &HostFrameTick) -> AppFrameTick {
    todo!("W8-16b")
}

/// C# `CrossfadeLayer`. Share it as `Rc<CrossfadeLayer>`.
pub struct CrossfadeLayer {
    // Fields (all interior-mutable, see the contract "State"): animator, outgoing, frame, interpolation, opacity,
    // frame subscription, start_count, disposed, clock, invalidate.
    _private: (),
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
        todo!("W8-16b")
    }

    /// C# `Opacity` getter (0..=1; 0 or no image = not drawn).
    pub fn opacity(&self) -> f32 {
        todo!("W8-16b")
    }

    /// C# `Opacity` setter (the animator's write path; tests use it).
    pub fn set_opacity(&self, opacity: f32) {
        todo!("W8-16b")
    }

    /// C# `IsActive`: an outgoing image is held (fading).
    pub fn is_active(&self) -> bool {
        todo!("W8-16b")
    }

    /// C# `StartCount`: how many fades were started (diagnostics, tests).
    pub fn start_count(&self) -> i32 {
        todo!("W8-16b")
    }

    /// C# `Outgoing`.
    pub fn outgoing(&self) -> Option<Arc<PixelBuffer>> {
        todo!("W8-16b")
    }

    /// C# `Frame`: the destination box frozen at `start`.
    pub fn frame(&self) -> RectD {
        todo!("W8-16b")
    }

    /// C# `OutgoingBufferId`: `PixelBuffer::id()` of the outgoing image while fading and not disposed (the cache must keep its bitmap).
    pub fn outgoing_buffer_id(&self) -> Option<i64> {
        todo!("W8-16b")
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
        todo!("W8-16b")
    }

    /// C# `Cancel()`: stop the tween (and the clock subscription) and release the outgoing image (opacity 0).
    pub fn cancel(&self) {
        todo!("W8-16b")
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
        todo!("W8-16b")
    }

    /// C# `Dispose()`: idempotent; unsubscribes, disposes the animator, releases the outgoing image. `Drop` calls it.
    pub fn dispose(&self) {
        todo!("W8-16b")
    }
}

impl Drop for CrossfadeLayer {
    fn drop(&mut self) {
        // The port calls `self.dispose()` here (idempotent); the stub has nothing to release.
    }
}
