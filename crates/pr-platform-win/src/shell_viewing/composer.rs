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

// Contract stub: the bodies are `todo!()` until the W8-16c port; removed with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use pr_app::input::primitives::{RectD, SizeD};
use pr_app::services::render::gpu_image::PrefetchStatus;
use pr_app::services::render::{DrawContext, RenderError};
use pr_app::viewport::render_geometry::ImageInterpolation;
use pr_core::model::enums::ScalingQuality;
use pr_imaging::pixels::PixelBuffer;

use super::compare::CompareFrame;
use super::crossfade::CrossfadeLayer;
use crate::host::frame_clock::base::HostFrameClock;

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
        todo!("W8-16c")
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

/// C# `PresentationComposer`. Share it as `Rc<PresentationComposer>`; every method takes `&self`.
pub struct PresentationComposer {
    // Fields (interior-mutable, see the contract "State"): cache, clock, invalidate, now, ticks_per_second, settle_ticks, crossfade, current, pending,
    // pending_since, pending_token, last_drawn_{id,rect,interpolation}, has_last_drawn, last_motion_ticks, settle_armed + subscription,
    // upload_completed subscription, current_changed handlers, disposed.
    _private: (),
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
        todo!("W8-16c")
    }

    /// C# `Current`: the main image being drawn (the bitmap the layout measures).
    pub fn current(&self) -> Option<Arc<PixelBuffer>> {
        todo!("W8-16c")
    }

    /// C# `Pending`: the upgrade waiting for its upload (the old image stays visible); `None` when there is none.
    pub fn pending(&self) -> Option<Arc<PixelBuffer>> {
        todo!("W8-16c")
    }

    /// C# `Crossfade`.
    pub fn crossfade(&self) -> &CrossfadeLayer {
        todo!("W8-16c")
    }

    /// C# `LastDrawn`: the last frame the user saw (buffer, destination box, filter) so a crossfade freezes exactly that. `None` before any image was
    /// drawn, or when `current` is no longer the buffer that was drawn (its id differs) or is disposed.
    pub fn last_drawn(&self) -> Option<(Arc<PixelBuffer>, RectD, ImageInterpolation)> {
        todo!("W8-16c")
    }

    /// C# `IsSettleArmed`: waiting for the high-quality frame after a lowered filter.
    pub fn is_settle_armed(&self) -> bool {
        todo!("W8-16c")
    }

    /// C# `PromotionCount`: how many upgrades became the main image (diagnostics, tests).
    pub fn promotion_count(&self) -> i32 {
        todo!("W8-16c")
    }

    /// C# `CurrentChanged +=`: raised when `current` changes because an upgrade was promoted (the window re-measures its layout for the new bitmap).
    /// Handlers run in subscription order on the UI thread.
    pub fn subscribe_current_changed(&self, handler: Rc<dyn Fn()>) -> u64 {
        todo!("W8-16c")
    }

    /// C# `CurrentChanged -=`; an unknown id is a no-op.
    pub fn unsubscribe_current_changed(&self, id: u64) {
        todo!("W8-16c")
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
        todo!("W8-16c")
    }

    /// C# `Set(image)`: sets `current` without the upgrade path (initial state, tests); drops any pending upgrade.
    pub fn set(&self, image: Option<Arc<PixelBuffer>>) {
        todo!("W8-16c")
    }

    /// C# `BeginCrossfade(duration)`: call BEFORE the new image is set (at `ImageChanging`). Freezes `last_drawn` into the crossfade layer. `Ok(false)`
    /// when nothing was drawn yet (first image after opening a folder) or the layer refuses (zero duration, empty frame). `Err(Disposed)` after `dispose`.
    pub fn begin_crossfade(&self, duration: Duration) -> Result<bool, ComposerError> {
        todo!("W8-16c")
    }

    /// C# `CancelCrossfade()`: release the old image and stop the fade (close, cache clear, device change).
    pub fn cancel_crossfade(&self) {
        todo!("W8-16c")
    }

    /// C# `CollectRetainedIds(ids)`: the `PixelBuffer::id()`s the GPU cache must keep so the next frame uploads nothing: `current`, `pending` (both when
    /// not disposed) and the outgoing crossfade image.
    pub fn collect_retained_ids(&self, ids: &mut BTreeSet<i64>) {
        todo!("W8-16c")
    }

    /// C# `TryPromotePending()`: call FIRST in every frame, before layout. Promotes `pending` to `current` when its bitmap is fully uploaded or
    /// [`MAX_DEFER_MS`] elapsed since it was set (`now - pending_since >= MAX_DEFER_MS * ticks_per_second / 1000`); a disposed pending is dropped
    /// (`false`). Counts the promotion and raises `CurrentChanged`. Returns whether it promoted.
    pub fn try_promote_pending(&self) -> bool {
        todo!("W8-16c")
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
        todo!("W8-16c")
    }

    /// C# `Dispose()`: idempotent; unsubscribes from `UploadCompleted` and the clock, disposes the crossfade layer, drops `pending` and `current`.
    /// `Drop` calls it.
    pub fn dispose(&self) {
        todo!("W8-16c")
    }
}

impl Drop for PresentationComposer {
    fn drop(&mut self) {
        // The port calls `self.dispose()` here (idempotent); the stub has nothing to release.
    }
}
