//! Port of `PhotoReview.Shell.Win32/Viewing/PresentationComposer.cs` (354 lines, WP-21, baseline `d92d6aae`). Unit W8-16c (contract `contracts/W8-16c.md`).
//!
//! State and draw order of the "image presentation" part of the main window (replaces `MainImage` + `OutgoingImage` + `ComparePanel` of
//! `MainWindow.xaml`), separate from the window so it runs on an offscreen surface and a fake frame clock:
//! - order, bottom to top: main image, crossfade layer ([`CrossfadeLayer`]), compare panel ([`compare::draw`]); the canvas is cleared by the caller;
//! - same-image upgrade (preview -> full, ADR 0008): the new buffer is uploaded FIRST in stripes at Background priority (`GpuImageCache::prefetch`),
//!   the old image keeps being drawn into the very same destination box until the new bitmap is complete (or [`MAX_DEFER_MS`] passed), so no
//!   frame blocks the UI thread for tens of ms and the layout does not jump (the box is decided by layout, not by the buffer);
//! - filter: `viewing::scaling_policy::choose` (a heavy downscale in motion is drawn `Linear`, then redrawn with the requested filter once still
//!   for `SETTLE_MS`).
//!
//! GPU bitmaps are keyed by `PixelBuffer::id()` (INV-1), never by path. UI thread only; the composer is `!Send`.
//!
//! Rust shape: all state lives in one `Rc<Inner>` behind `Cell`/`RefCell`. The callbacks that outlive a call (the `UploadCompleted` handler, the
//! deadline job given to `schedule_on_ui`, the settle frame handler) hold a `Weak<Inner>`, so a dropped composer is simply ignored. No state borrow
//! is held while `invalidate`, the clock, the cache, the timestamp function or a `CurrentChanged` handler runs (D-W8B4V-9, trap T2).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{RectD, SizeD};
use pr_app::services::render::gpu_image::PrefetchStatus;
use pr_app::services::render::{DrawContext, RenderError};
use pr_app::viewing::image_layer::try_draw;
use pr_app::viewing::scaling_policy::{self, SETTLE_MS};
use pr_app::viewport::render_geometry::{ImageInterpolation, image_interpolation_from_scaling_quality};
use pr_core::model::enums::ScalingQuality;
use pr_imaging::pixels::PixelBuffer;

use super::compare::{self, CompareFrame};
use super::crossfade::CrossfadeLayer;
use crate::host::frame_clock::base::{FrameHandlerId, HostFrameClock, qpc_frequency, qpc_now};

/// C# `PresentationComposer.MaxDeferMs`: how long a same-image upgrade may wait for its prefetch (then it is swapped in and uploaded synchronously).
pub const MAX_DEFER_MS: i64 = 1000;

/// C# `PresentationInputs`: everything one frame needs, computed per frame from the viewer state, the viewport controller and the compare view model.
#[derive(Debug, Clone, Copy)]
pub struct PresentationInputs<'a> {
    /// Client area (DIP).
    pub viewport: SizeD,
    /// Destination box of the main image (DIP): `ImageOrigin` + the real size of the image.
    pub main_destination: RectD,
    /// `ViewerState.EffectiveScalingQuality` (main image).
    pub effective_quality: ScalingQuality,
    /// `ViewerState.ScalingQuality` (the two compare cells use the setting, without the forced HQ of Refresh).
    pub compare_quality: ScalingQuality,
    /// DPI / 96: turns the destination box into device pixels to measure the downscale.
    pub dpi_scale: f64,
    pub compare_visible: bool,
    pub compare: CompareFrame<'a>,
}

/// C# `ComposedFrame`: what one `draw` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposedFrame {
    /// The main image (`current`) was drawn.
    pub main_drawn: bool,
    /// The crossfade layer was drawn (opacity > 0).
    pub crossfade_drawn: bool,
    /// Compare images drawn (0..=2).
    pub compare_images: i32,
    /// The filter used for the main image.
    pub interpolation: ImageInterpolation,
    /// The filter was lowered to `Linear` (moving + heavy downscale); a settle frame follows.
    pub degraded: bool,
}

/// C# `Action<TimeSpan, Action> scheduleOnUi`: runs the closure on the UI thread after the delay (the host owns the timer and the dispatcher,
/// D-W8B4V-4). The closure is not `Send`: the host keeps it in a UI-thread queue and posts a `Send` wake job (precedent: `StripeScheduler`, D-W8B3S-2).
pub type ScheduleOnUi = Rc<dyn Fn(Duration, Box<dyn FnOnce()>)>;

/// C# `Func<long> timestamp`: QPC ticks.
pub type TimestampFn = Rc<dyn Fn() -> i64>;

/// The optional constructor arguments of C#.
#[derive(Clone)]
pub struct ComposerOptions {
    /// `None` = `host::frame_clock::base::qpc_now`.
    pub timestamp: Option<TimestampFn>,
    /// `<= 0` = `qpc_frequency()`.
    pub ticks_per_second: i64,
    /// C# `settleMs`, default `scaling_policy::SETTLE_MS`; a negative value is 0.
    pub settle_ms: i32,
}

impl Default for ComposerOptions {
    fn default() -> Self {
        Self {
            timestamp: None,
            ticks_per_second: 0,
            settle_ms: SETTLE_MS,
        }
    }
}

impl std::fmt::Debug for ComposerOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComposerOptions")
            .field("ticks_per_second", &self.ticks_per_second)
            .field("settle_ms", &self.settle_ms)
            .finish_non_exhaustive()
    }
}

/// Failure of a composer call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerError {
    /// C# `ObjectDisposedException` (`SetIncoming`, `BeginCrossfade` after `dispose`).
    Disposed,
    /// The GPU cache refused a prefetch (device failure).
    Render(RenderError),
}

type CurrentChangedHandler = Rc<dyn Fn()>;

/// The shared state; see the module doc.
struct Inner {
    cache: Rc<dyn PrefetchStatus>,
    clock: Rc<dyn HostFrameClock>,
    invalidate: Rc<dyn Fn()>,
    now: TimestampFn,
    schedule_on_ui: ScheduleOnUi,
    ticks_per_second: i64,
    settle_ticks: i64,
    crossfade: CrossfadeLayer,
    current: RefCell<Option<Arc<PixelBuffer>>>,
    pending: RefCell<Option<Arc<PixelBuffer>>>,
    pending_since: Cell<i64>,
    pending_token: Cell<i64>,
    last_drawn_id: Cell<i64>,
    last_drawn_rect: Cell<RectD>,
    last_drawn_interpolation: Cell<ImageInterpolation>,
    has_last_drawn: Cell<bool>,
    last_motion_ticks: Cell<i64>,
    settle_armed: Cell<bool>,
    settle_subscription: Cell<Option<FrameHandlerId>>,
    upload_subscription: Cell<Option<u64>>,
    current_changed: RefCell<Vec<(u64, CurrentChangedHandler)>>,
    next_handler_id: Cell<u64>,
    promotion_count: Cell<i32>,
    disposed: Cell<bool>,
}

impl Inner {
    fn now(&self) -> i64 {
        (self.now)()
    }

    /// C# `ArmPendingDeadline(token)`.
    fn arm_pending_deadline(self: &Rc<Self>, token: i64) {
        let weak = Rc::downgrade(self);
        let job: Box<dyn FnOnce()> = Box::new(move || {
            let Some(state) = weak.upgrade() else { return };
            if state.disposed.get() || state.pending.borrow().is_none() || token != state.pending_token.get() {
                return;
            }
            if state.try_promote_pending() {
                (state.invalidate)();
            }
        });
        (self.schedule_on_ui)(Duration::from_millis(MAX_DEFER_MS.unsigned_abs()), job);
    }

    /// C# `OnUploadCompleted(pixelBufferId)`.
    fn on_upload_completed(&self, pixel_buffer_id: i64) {
        if self.disposed.get() {
            return;
        }
        let matches = self
            .pending
            .borrow()
            .as_ref()
            .is_some_and(|pending| pending.id() == pixel_buffer_id);
        if matches && self.try_promote_pending() {
            (self.invalidate)();
        }
    }

    /// C# `TryPromotePending()`.
    fn try_promote_pending(&self) -> bool {
        let Some(pending) = self.pending.borrow().clone() else {
            return false;
        };
        if pending.is_disposed() {
            *self.pending.borrow_mut() = None;
            return false;
        }
        let waited = self.now().saturating_sub(self.pending_since.get());
        let limit = MAX_DEFER_MS.saturating_mul(self.ticks_per_second) / 1000;
        if !self.cache.is_fully_uploaded(pending.id()) && waited < limit {
            return false;
        }
        *self.current.borrow_mut() = Some(pending);
        *self.pending.borrow_mut() = None;
        self.promotion_count.set(self.promotion_count.get().saturating_add(1));
        let handlers: Vec<Rc<dyn Fn()>> = self
            .current_changed
            .borrow()
            .iter()
            .map(|(_, handler)| Rc::clone(handler))
            .collect();
        for handler in handlers {
            handler();
        }
        true
    }

    /// C# `LastDrawn`.
    fn last_drawn(&self) -> Option<(Arc<PixelBuffer>, RectD, ImageInterpolation)> {
        if !self.has_last_drawn.get() {
            return None;
        }
        let current = self.current.borrow().clone()?;
        if current.is_disposed() || current.id() != self.last_drawn_id.get() {
            return None;
        }
        Some((current, self.last_drawn_rect.get(), self.last_drawn_interpolation.get()))
    }

    /// C# `SetSettleArmed(armed)`.
    fn set_settle_armed(self: &Rc<Self>, armed: bool) {
        if armed == self.settle_armed.get() || (self.disposed.get() && armed) {
            return;
        }
        self.settle_armed.set(armed);
        if armed {
            let weak = Rc::downgrade(self);
            let id = self.clock.subscribe(Box::new(move |_tick| {
                if let Some(state) = weak.upgrade() {
                    state.on_settle_tick();
                }
            }));
            self.settle_subscription.set(Some(id));
            self.clock.request_frame();
        } else if let Some(id) = self.settle_subscription.take() {
            self.clock.unsubscribe(id);
        }
    }

    /// C# `OnSettleTick`.
    fn on_settle_tick(&self) {
        if self.now().saturating_sub(self.last_motion_ticks.get()) < self.settle_ticks {
            self.clock.request_frame();
            return;
        }
        // Still long enough: one more frame (`draw` chooses the requested filter again and unarms).
        (self.invalidate)();
    }

    /// C# `Dispose()`.
    fn dispose(self: &Rc<Self>) {
        if self.disposed.replace(true) {
            return;
        }
        if let Some(id) = self.upload_subscription.take() {
            self.cache.unsubscribe_upload_completed(id);
        }
        self.set_settle_armed(false);
        self.crossfade.dispose();
        *self.pending.borrow_mut() = None;
        *self.current.borrow_mut() = None;
    }
}

/// C# `PresentationComposer`. Share it as `Rc<PresentationComposer>`; every method takes `&self`.
pub struct PresentationComposer {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for PresentationComposer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationComposer").finish_non_exhaustive()
    }
}

impl PresentationComposer {
    /// C# constructor `(cache, dispatcher, clock, invalidate, timestamp, ticksPerSecond, settleMs, scheduleOnUi)`; the `IUiDispatcher` parameter is
    /// only the default of `scheduleOnUi` in C#, so Rust takes `schedule_on_ui` directly. Subscribes to `UploadCompleted`; creates the
    /// [`CrossfadeLayer`] on the same clock / invalidate / tick rate; `last_motion_ticks = i64::MIN / 2`. `invalidate` is `window.invalidate`.
    pub fn new(
        cache: Rc<dyn PrefetchStatus>,
        clock: Rc<dyn HostFrameClock>,
        invalidate: Rc<dyn Fn()>,
        schedule_on_ui: ScheduleOnUi,
        options: ComposerOptions,
    ) -> Self {
        let now: TimestampFn = options.timestamp.unwrap_or_else(|| Rc::new(qpc_now));
        let ticks_per_second = if options.ticks_per_second > 0 {
            options.ticks_per_second
        } else {
            qpc_frequency()
        };
        let settle_ticks = i64::from(options.settle_ms.max(0)).saturating_mul(ticks_per_second) / 1000;
        let crossfade = CrossfadeLayer::new(Rc::clone(&clock), Rc::clone(&invalidate), ticks_per_second);
        let inner = Rc::new(Inner {
            cache,
            clock,
            invalidate,
            now,
            schedule_on_ui,
            ticks_per_second,
            settle_ticks,
            crossfade,
            current: RefCell::new(None),
            pending: RefCell::new(None),
            pending_since: Cell::new(0),
            pending_token: Cell::new(0),
            last_drawn_id: Cell::new(-1),
            last_drawn_rect: Cell::new(RectD::default()),
            last_drawn_interpolation: Cell::new(ImageInterpolation::NearestNeighbor),
            has_last_drawn: Cell::new(false),
            last_motion_ticks: Cell::new(i64::MIN / 2),
            settle_armed: Cell::new(false),
            settle_subscription: Cell::new(None),
            upload_subscription: Cell::new(None),
            current_changed: RefCell::new(Vec::new()),
            next_handler_id: Cell::new(0),
            promotion_count: Cell::new(0),
            disposed: Cell::new(false),
        });
        let weak = Rc::downgrade(&inner);
        let subscription = inner.cache.subscribe_upload_completed(Rc::new(move |pixel_buffer_id| {
            if let Some(state) = weak.upgrade() {
                state.on_upload_completed(pixel_buffer_id);
            }
        }));
        inner.upload_subscription.set(Some(subscription));
        Self { inner }
    }

    /// C# `Current`: the main image being drawn (the bitmap the layout measures).
    pub fn current(&self) -> Option<Arc<PixelBuffer>> {
        self.inner.current.borrow().clone()
    }

    /// C# `Pending`: the upgrade waiting for its upload (the old image stays visible); `None` when there is none.
    pub fn pending(&self) -> Option<Arc<PixelBuffer>> {
        self.inner.pending.borrow().clone()
    }

    /// C# `Crossfade`.
    pub fn crossfade(&self) -> &CrossfadeLayer {
        &self.inner.crossfade
    }

    /// C# `LastDrawn`: the last frame the user saw (buffer, destination box, filter) so a crossfade freezes exactly that. `None` before any image was
    /// drawn, or when `current` is no longer the buffer that was drawn (its id differs) or is disposed.
    pub fn last_drawn(&self) -> Option<(Arc<PixelBuffer>, RectD, ImageInterpolation)> {
        self.inner.last_drawn()
    }

    /// C# `IsSettleArmed`: waiting for the high-quality frame after a lowered filter.
    pub fn is_settle_armed(&self) -> bool {
        self.inner.settle_armed.get()
    }

    /// C# `PromotionCount`: how many upgrades became the main image (diagnostics, tests).
    pub fn promotion_count(&self) -> i32 {
        self.inner.promotion_count.get()
    }

    /// C# `CurrentChanged +=`: raised when `current` changes because an upgrade was promoted (the window re-measures its layout for the new bitmap).
    /// Handlers run in subscription order on the UI thread.
    pub fn subscribe_current_changed(&self, handler: Rc<dyn Fn()>) -> u64 {
        let id = self.inner.next_handler_id.get().wrapping_add(1);
        self.inner.next_handler_id.set(id);
        self.inner.current_changed.borrow_mut().push((id, handler));
        id
    }

    /// C# `CurrentChanged -=`; an unknown id is a no-op.
    pub fn unsubscribe_current_changed(&self, id: u64) {
        self.inner
            .current_changed
            .borrow_mut()
            .retain(|(known, _)| *known != id);
    }

    /// C# `SetIncoming(image, isFileChange, deferUploadBytes)`: a new image from the presenter. Always drops any pending upgrade first. Replaces
    /// `current` AT ONCE (returns `Ok(true)`) when `image` is `None`, `is_file_change` is set, there is no usable `current`, `image` is disposed, or
    /// its bitmap (`width * height * 4`, in `i64`) is smaller than `defer_upload_bytes`. Otherwise it is a same-image upgrade: `cache.prefetch(image)`;
    /// when the bitmap is already fully uploaded `current` is replaced at once; else `pending = image`, `pending_since = now`, a new deadline is
    /// scheduled ([`MAX_DEFER_MS`]) and `Ok(false)` is returned. `Err(Disposed)` after `dispose`.
    pub fn set_incoming(
        &self,
        image: Option<Arc<PixelBuffer>>,
        is_file_change: bool,
        defer_upload_bytes: i64,
    ) -> Result<bool, ComposerError> {
        let state = &self.inner;
        if state.disposed.get() {
            return Err(ComposerError::Disposed);
        }
        *state.pending.borrow_mut() = None;
        let Some(image) = image else {
            *state.current.borrow_mut() = None;
            return Ok(true);
        };
        let current_usable = state
            .current
            .borrow()
            .as_ref()
            .is_some_and(|current| !current.is_disposed());
        let bytes = i64::from(image.width())
            .saturating_mul(i64::from(image.height()))
            .saturating_mul(4);
        if is_file_change || !current_usable || image.is_disposed() || bytes < defer_upload_bytes {
            *state.current.borrow_mut() = Some(image);
            return Ok(true);
        }
        // Same-image upgrade: keep the old image on screen, upload the new one in stripes.
        state.cache.prefetch(&image).map_err(ComposerError::Render)?;
        if state.cache.is_fully_uploaded(image.id()) {
            *state.current.borrow_mut() = Some(image);
            return Ok(true);
        }
        *state.pending.borrow_mut() = Some(image);
        state.pending_since.set(state.now());
        let token = state.pending_token.get().wrapping_add(1);
        state.pending_token.set(token);
        state.arm_pending_deadline(token);
        Ok(false)
    }

    /// C# `Set(image)`: sets `current` without the upgrade path (initial state, tests); drops any pending upgrade.
    pub fn set(&self, image: Option<Arc<PixelBuffer>>) {
        *self.inner.pending.borrow_mut() = None;
        *self.inner.current.borrow_mut() = image;
    }

    /// C# `BeginCrossfade(duration)`: call BEFORE the new image is set (at `ImageChanging`). Freezes `last_drawn` into the crossfade layer. `Ok(false)`
    /// when nothing was drawn yet (first image after opening a folder) or the layer refuses (zero duration, empty frame). `Err(Disposed)` after `dispose`.
    pub fn begin_crossfade(&self, duration: Duration) -> Result<bool, ComposerError> {
        let state = &self.inner;
        if state.disposed.get() {
            return Err(ComposerError::Disposed);
        }
        let Some((pixels, frame, interpolation)) = state.last_drawn() else {
            return Ok(false);
        };
        state
            .crossfade
            .start(pixels, frame, interpolation, duration)
            .map_err(|_| ComposerError::Disposed)
    }

    /// C# `CancelCrossfade()`: release the old image and stop the fade (close, cache clear, device change).
    pub fn cancel_crossfade(&self) {
        self.inner.crossfade.cancel();
    }

    /// C# `CollectRetainedIds(ids)`: the `PixelBuffer::id()`s the GPU cache must keep so the next frame uploads nothing: `current`, `pending` (both when
    /// not disposed) and the outgoing crossfade image.
    pub fn collect_retained_ids(&self, ids: &mut BTreeSet<i64>) {
        let state = &self.inner;
        if let Some(current) = state.current.borrow().as_ref().filter(|buffer| !buffer.is_disposed()) {
            ids.insert(current.id());
        }
        if let Some(pending) = state.pending.borrow().as_ref().filter(|buffer| !buffer.is_disposed()) {
            ids.insert(pending.id());
        }
        if let Some(outgoing) = state.crossfade.outgoing_buffer_id() {
            ids.insert(outgoing);
        }
    }

    /// C# `TryPromotePending()`: call FIRST in every frame, before layout. Promotes `pending` to `current` when its bitmap is fully uploaded or
    /// [`MAX_DEFER_MS`] elapsed since it was set (`now - pending_since >= MAX_DEFER_MS * ticks_per_second / 1000`); a disposed pending is dropped
    /// (`false`). Counts the promotion and raises `CurrentChanged`. Returns whether it promoted.
    pub fn try_promote_pending(&self) -> bool {
        self.inner.try_promote_pending()
    }

    /// C# `Draw(context, input)`: main image, then the crossfade layer, then the compare panel, in that order; the canvas was cleared by the caller.
    /// The filter is chosen by `scaling_policy::choose` from the requested filter, the bitmap size, the destination box x `dpi_scale` and whether the
    /// box moved within [`settle`](ComposerOptions::settle_ms) ms; the settle clock is armed exactly while the filter is lowered. A GPU / context
    /// error is returned after the clips are popped.
    pub fn draw(
        &self,
        context: &mut dyn DrawContext,
        input: &PresentationInputs<'_>,
    ) -> Result<ComposedFrame, RenderError> {
        let state = &self.inner;
        let mut main_drawn = false;
        let mut interpolation = image_interpolation_from_scaling_quality(input.effective_quality);
        let mut degraded = false;
        let viewport = RectD {
            x: 0.0,
            y: 0.0,
            width: input.viewport.width,
            height: input.viewport.height,
        };

        let current = state.current.borrow().clone();
        if let Some(pixels) = current.filter(|buffer| !buffer.is_disposed()) {
            let destination = input.main_destination;
            let requested = interpolation;
            let now = state.now();
            let moved = pixels.id() == state.last_drawn_id.get()
                && state.has_last_drawn.get()
                && state.last_drawn_rect.get() != destination;
            if moved {
                state.last_motion_ticks.set(now);
            }
            let in_motion = now.saturating_sub(state.last_motion_ticks.get()) < state.settle_ticks;
            let dpi = if input.dpi_scale > 0.0 { input.dpi_scale } else { 1.0 };
            interpolation = scaling_policy::choose(
                requested,
                pixels.width(),
                pixels.height(),
                destination.width * dpi,
                destination.height * dpi,
                in_motion,
            );
            degraded = interpolation != requested;

            context.push_clip(viewport);
            let drawn = try_draw(&*state.cache, context, &pixels, destination, interpolation, 1.0, 1);
            let popped = context.pop_clip();
            main_drawn = drawn?;
            popped?;

            if main_drawn {
                state.last_drawn_id.set(pixels.id());
                state.last_drawn_rect.set(destination);
                state.last_drawn_interpolation.set(interpolation);
                state.has_last_drawn.set(true);
            }
        }

        state.set_settle_armed(degraded);
        let crossfade_drawn = state.crossfade.draw(&*state.cache, context, viewport)?;
        let compare_images = if input.compare_visible {
            compare::draw(
                &*state.cache,
                context,
                input.viewport,
                &input.compare,
                image_interpolation_from_scaling_quality(input.compare_quality),
            )?
        } else {
            0
        };
        Ok(ComposedFrame {
            main_drawn,
            crossfade_drawn,
            compare_images,
            interpolation,
            degraded,
        })
    }

    /// C# `Dispose()`: idempotent; unsubscribes from `UploadCompleted` and the clock, disposes the crossfade layer, drops `pending` and `current`.
    /// `Drop` calls it.
    pub fn dispose(&self) {
        self.inner.dispose();
    }
}

impl Drop for PresentationComposer {
    fn drop(&mut self) {
        self.inner.dispose();
    }
}
