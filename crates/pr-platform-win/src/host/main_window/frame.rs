//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.Frame.cs` (169 lines, baseline `d92d6aae`): `RenderFrame`, `RenderNow`, `Present`, `DrawContent`,
//! `OnFramePresented` and the two first-present events. Unit W8-13i (with `scrollbars`). Contract: `contracts/W8-13i.md`.
//!
//! One frame = size check, pacer decision, surface resize (also for a dpi-only change), promote a finished upgrade, run a posted layout, `BeginDraw`,
//! clear to the canvas colour, draw the presentation (13j), the scroll bars (this unit), the overlay (15j, through `MainWindowFeature::draw_overlay`),
//! `EndDraw` + `Present`, statistics, device-loss redraw and the first-present marks.

use std::cell::Cell;
use std::time::Duration;

use pr_app::services::render::{DrawContext, PresentResult};
use pr_core::diagnostics::perf::{STOPWATCH_FREQUENCY, ShellPerfMarks, timestamp};

use super::core::{MainWindow, MainWindowInner, canvas_color};
use crate::host::contracts::{EventHandlerId, ShellWindowApi, ShellWindowEvent, WindowEventHandler};
use crate::host::shell_window::create::EventList;
use crate::shell_viewing::frame_stats::{FrameStats, ticks_to_ms};
use crate::shell_viewing::pacer::PresentMode;

/// The list event the two first-present lists store their handlers under (an `EventList` is keyed by an event; these lists hold one kind each).
const LIST_EVENT: ShellWindowEvent = ShellWindowEvent::Closed;
/// First id of the `FirstImagePresented` list: disjoint from the `FirstPresented` list (ids start at 1), so one unsubscribe method serves both.
const FIRST_IMAGE_IDS_START: EventHandlerId = 1 << 32;

/// The state that C# `ShellMainWindow.Frame.cs` added to the class (`_firstImageDrawn`, `_imageDrawnInFrame`, the two events).
pub struct FrameState {
    /// C# `_firstImageDrawn` (also `FirstImagePresented`).
    pub(super) first_image_drawn: Cell<bool>,
    /// C# `_imageDrawnInFrame`: set by `draw_presentation` when the frame contained the current image; consumed by `on_frame_presented`.
    pub(super) image_drawn_in_frame: Cell<bool>,
    /// C# `FirstPresented` (first frame of any kind presented: the window is revealed).
    pub(super) first_presented: EventList<WindowEventHandler>,
    /// C# `FirstImagePresentedEvent` (first frame that contains an image).
    pub(super) first_image_presented: EventList<WindowEventHandler>,
    /// `present` is running (T2: a hook that calls `render_now` from inside a frame returns at once).
    pub(super) presenting: Cell<bool>,
    /// A drawing step (the scroll bars) failed inside the draw segment: the frame ends like a C# exception would (no overlay, no `on_frame_presented`).
    pub(super) draw_failed: Cell<bool>,
}

/// Resets `presenting` when `present` ends, also on a panic.
struct PresentingGuard<'a>(&'a Cell<bool>);

impl Drop for PresentingGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// The diagnostic text of a slow frame: `mode=Immediate result=Presented img=640x480 fade=false`. `image` is the composer's current image size and
/// `fade` its crossfade state; both are empty when there is no composer or no image (C# prints null as an empty string).
pub fn slow_frame_note(
    mode: PresentMode,
    result: PresentResult,
    image: Option<(i32, i32)>,
    fade: Option<bool>,
) -> String {
    let (width, height) = image.map_or((String::new(), String::new()), |(w, h)| (w.to_string(), h.to_string()));
    let fade = fade.map_or_else(String::new, |active| active.to_string());
    format!("mode={mode:?} result={result:?} img={width}x{height} fade={fade}")
}

/// `Duration` to milliseconds (T3).
pub fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

impl std::fmt::Debug for FrameState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameState")
            .field("first_image_drawn", &self.first_image_drawn.get())
            .finish_non_exhaustive()
    }
}

impl Default for FrameState {
    /// Nothing drawn, no subscribers. The two lists use disjoint id ranges (`EventList::with_first_id`).
    fn default() -> Self {
        Self {
            first_image_drawn: Cell::new(false),
            image_drawn_in_frame: Cell::new(false),
            first_presented: EventList::new(),
            first_image_presented: EventList::with_first_id(FIRST_IMAGE_IDS_START),
            presenting: Cell::new(false),
            draw_failed: Cell::new(false),
        }
    }
}

impl MainWindowInner {
    /// C# `RenderFrame()`: the `ShellWindow::subscribe_render` handler; calls [`present`](Self::present).
    pub(super) fn render_frame(&self) {
        self.present();
    }

    /// C# `RenderNow()`: layout + draw + present synchronously, not waiting for the frame clock (first frame, first image). A drawing error is logged
    /// ("Frame failed"), the frame is dropped; it never propagates.
    pub(crate) fn render_now(&self) {
        self.present();
    }

    /// C# `Present()` (lines 26-121). Steps, in this order (the order is behaviour):
    /// 1. return when disposed, no surface, or the window is destroyed; return when the client size is not positive (minimised);
    /// 2. `awaiting_first_image = !first_image_drawn`; `mode = pacer.mode_for(viewer.is_fullscreen(), awaiting_first_image)`;
    /// 3. unless awaiting the first image: when the surface has a frame-latency wait handle and `pacer.should_defer(surface.wait_for_next_frame(0))`,
    ///    `window.invalidate()` and return (the swap chain has no new frame yet; do not block). A failing wait is logged ("Frame failed") and the frame dropped;
    /// 4. (not holding the surface borrow across foreign calls) `surface.resize(w, h, dpi_scale)` when the pixel size OR the surface `dpi_scale` differs from
    ///    the window's; `composer.try_promote_pending()`; `viewport.update_layout()` when `is_layout_posted()`;
    /// 5. `surface.set_immediate_present(mode == Immediate)`, `begin_draw_context()`, `clear(canvas_color())`, `draw_content`, the features' `draw_overlay`,
    ///    then ALWAYS `end_draw_and_present()` once begun (C# `finally`); an `Err` from begin/resize/end is logged ("Frame failed") and the frame dropped without
    ///    `on_frame_presented` (the unbalanced-stack error of `end_draw_and_present` arrives after the present, as in C#);
    /// 6. `pacer.observe_present(mode, present_ms)`; slow frames (`FrameStats::is_slow(end_draw_ms, present_ms)`) are recorded with the note
    ///    `mode={mode} result={result} img={w}x{h} fade={active}`; `stats.record(frame_ticks, present_ticks)`; `frame_log.record(..)`;
    /// 7. `DeviceRecreated` or `Failed` -> `window.invalidate()`; `Failed` -> return; `Presented`, `DeviceRecreated`, `Occluded` -> `on_frame_presented()`.
    pub(super) fn present(&self) {
        if self.disposed.get() || self.window.is_destroyed() {
            return;
        }
        if self.frame.presenting.replace(true) {
            return; // T2: never nested
        }
        let _guard = PresentingGuard(&self.frame.presenting);
        self.present_frame();
    }

    /// C# `DrawContent(context)`: `draw_presentation(context)` (13j) then `draw_scroll_bars(context)` (scroll bars lie over the image, under the overlay).
    pub(super) fn draw_content(&self, context: &mut dyn DrawContext) {
        self.draw_presentation(context);
        self.draw_scroll_bars(context); // WP-26: the bars lie over the image, under the overlay
    }

    /// C# `OnFramePresented()` (lines 130-159): `image_drawn = take(image_drawn_in_frame)`; `first_image = image_drawn && !first_image_drawn` (then
    /// `first_image_drawn = true`); `hub.note_frame_presented(image_drawn)`; the first time ever: `first_present_done = true`, mark `firstPresent`,
    /// `cloak.note_first_present()`, raise `FirstPresented`; if `first_image`: `keep_warm.warm_at_launch()` when `options.activate`, raise `FirstImagePresented`.
    pub(super) fn on_frame_presented(&self) {
        let image_drawn = self.frame.image_drawn_in_frame.take();
        let mut first_image = false;
        if image_drawn {
            first_image = !self.frame.first_image_drawn.get();
            self.frame.first_image_drawn.set(true);
        }

        self.hub.note_frame_presented(image_drawn);
        if !self.first_present_done.get() {
            self.first_present_done.set(true);
            self.mark(ShellPerfMarks::FIRST_PRESENT);
            self.cloak.note_first_present();
            self.frame.first_presented.raise(LIST_EVENT, &mut |handler| handler());
        }

        if first_image {
            if self.options.activate {
                // The controller is cloned out: the warm-up must not run under the `keep_warm` borrow.
                let controller = self
                    .keep_warm
                    .try_borrow()
                    .ok()
                    .and_then(|setup| setup.as_ref().and_then(|setup| setup.controller.clone()));
                if let Some(controller) = controller {
                    controller.warm_at_launch(); // perf gate 3: the first key press is not the first work the idle GPU sees
                }
            }
            self.frame
                .first_image_presented
                .raise(LIST_EVENT, &mut |handler| handler());
        }
    }
}

impl MainWindowInner {
    /// The body of [`present`](Self::present) once the re-entrancy guard is held. The surface cell is borrowed per step (the draw segment holds it for the
    /// whole segment, D-W8B4M-8); layout, promotion and every callback run with no surface borrow.
    fn present_frame(&self) {
        if self.surface.try_borrow().map_or(true, |surface| surface.is_none()) {
            return;
        }
        let (width, height) = self.window.client_size_pixels();
        if width <= 0 || height <= 0 {
            return; // minimised: nothing to draw; the frame after the restore draws
        }

        // WP-21: Present must not block the UI thread. The first image is always `Immediate`.
        let awaiting_first_image = !self.frame.first_image_drawn.get();
        let fullscreen = self.viewer.borrow().is_fullscreen();
        let mode = self.viewing.pacer.borrow().mode_for(fullscreen, awaiting_first_image);
        if !awaiting_first_image {
            let wait = match self.surface.try_borrow() {
                Ok(surface) => match surface.as_ref() {
                    Some(surface) if surface.frame_latency_wait_handle() != 0 => Some(surface.wait_for_next_frame(0)),
                    _ => None,
                },
                Err(_) => return,
            };
            match wait {
                Some(Ok(ready)) => {
                    if self.viewing.pacer.borrow_mut().should_defer(ready) {
                        self.window.invalidate(); // the swap chain has no new frame yet: next tick, never block
                        return;
                    }
                }
                Some(Err(fault)) => {
                    // C# lets the exception escape; the Rust choice (contract question 1): log and drop the frame.
                    self.log.error("Frame failed", Some(&fault));
                    return;
                }
                None => {}
            }
        }

        let frame_start = timestamp();
        // WP-25: the dpi can change with the same pixel size (a monitor with another dpi, same suggested rect).
        let dpi_scale = self.window.dpi_scale();
        let resize = match self.surface.try_borrow() {
            Ok(surface) => surface.as_ref().is_some_and(|surface| {
                surface.pixel_width() != width || surface.pixel_height() != height || surface.dpi_scale() != dpi_scale
            }),
            Err(_) => return,
        };
        if resize {
            let outcome = match self.surface.try_borrow_mut() {
                Ok(mut surface) => surface.as_mut().map(|surface| surface.resize(width, height, dpi_scale)),
                Err(_) => return,
            };
            if let Some(Err(fault)) = outcome {
                self.log.error("Frame failed", Some(&fault));
                return;
            }
        }

        // WP-21: a finished upgrade becomes the main image BEFORE the layout, so this frame measures the new bitmap.
        let composer = self
            .viewing
            .composer
            .try_borrow()
            .ok()
            .and_then(|composer| composer.clone());
        if let Some(composer) = composer.as_ref() {
            composer.try_promote_pending();
        }
        // A posted layout runs now, before BeginDraw: the scroll bars of this frame come from this frame's layout (D-W8B4M-8).
        if self.viewport.is_layout_posted() {
            self.viewport.update_layout();
        }

        let features = self.feature_snapshot();
        self.frame.draw_failed.set(false);
        let (result, present_ticks, present_start) = {
            let Ok(mut guard) = self.surface.try_borrow_mut() else {
                return;
            };
            let Some(surface) = guard.as_mut() else {
                return;
            };
            surface.set_immediate_present(mode == PresentMode::Immediate);
            match surface.begin_draw_context() {
                Err(fault) => {
                    self.log.error("Frame failed", Some(&fault));
                    return;
                }
                Ok(context) => {
                    context.clear(canvas_color());
                    self.draw_content(context);
                    if !self.frame.draw_failed.get() {
                        for feature in &features {
                            feature.draw_overlay(context); // WP-23: the HUD over the bars, same frame
                        }
                    }
                }
            }
            // Always once a frame was begun (C# `finally`).
            let present_start = timestamp();
            let result = surface.end_draw_and_present();
            (result, timestamp() - present_start, present_start)
        };
        let result = match result {
            Ok(result) => result,
            Err(fault) => {
                self.log.error("Frame failed", Some(&fault));
                return;
            }
        };
        if self.frame.draw_failed.replace(false) {
            return; // a drawing step failed (already logged): the frame is dropped like a C# exception would
        }

        let present_ms = ticks_to_ms(present_ticks, STOPWATCH_FREQUENCY);
        if self.viewing.pacer.borrow_mut().observe_present(mode, present_ms) {
            self.log
                .warn("Present(1) blocked the UI thread; the shell now presents without waiting for vblank (WP-21).");
        }
        let (end_draw_ms, surface_present_ms) = self.surface.try_borrow().map_or((0.0, 0.0), |surface| {
            surface.as_ref().map_or((0.0, 0.0), |surface| {
                (
                    duration_ms(surface.last_end_draw()),
                    duration_ms(surface.last_present()),
                )
            })
        });
        {
            let mut stats = self.viewing.stats.borrow_mut();
            if FrameStats::is_slow(end_draw_ms, surface_present_ms) {
                // Only slow frames build the diagnostic text: the hot path allocates nothing.
                let image = composer
                    .as_ref()
                    .and_then(|composer| composer.current())
                    .map(|image| (image.width(), image.height()));
                let fade = composer.as_ref().map(|composer| composer.crossfade().is_active());
                stats.record_slow(
                    ticks_to_ms(timestamp() - frame_start, STOPWATCH_FREQUENCY),
                    ticks_to_ms(present_start - frame_start, STOPWATCH_FREQUENCY),
                    end_draw_ms,
                    surface_present_ms,
                    &slow_frame_note(mode, result, image, fade),
                );
            }
            let frame_end = timestamp();
            stats.record(frame_end - frame_start, present_ticks);
            if let Ok(mut setup) = self.keep_warm.try_borrow_mut() {
                if let Some(frame_log) = setup.as_mut().and_then(|setup| setup.frame_log.as_mut()) {
                    frame_log.record(frame_start, frame_end, end_draw_ms, surface_present_ms); // WP21-STALL: diagnostics, off by default
                }
            }
        }
        if matches!(result, PresentResult::DeviceRecreated | PresentResult::Failed) {
            // Device lost: the cache cleared itself; the next tick redraws (the image uploads again from its PixelBuffer).
            self.window.invalidate();
            if result == PresentResult::Failed {
                return;
            }
        }

        // `Occluded` is drawn and submitted: it still counts, so the cloak is lifted and the startup marks advance (T7: no retry, no invalidate).
        if matches!(
            result,
            PresentResult::Presented | PresentResult::DeviceRecreated | PresentResult::Occluded
        ) {
            self.on_frame_presented();
        }
    }
}

impl MainWindow {
    /// C# `RenderNow()` (`internal`: the startup code and tests call it).
    pub fn render_now(&self) {
        self.inner.render_now();
    }

    /// C# `FirstImagePresented`.
    pub fn first_image_presented(&self) -> bool {
        self.inner.frame.first_image_drawn.get()
    }

    /// C# `FirstPresented += handler`.
    pub fn subscribe_first_presented(&self, handler: WindowEventHandler) -> EventHandlerId {
        self.inner.frame.first_presented.add(LIST_EVENT, handler)
    }

    /// C# `FirstImagePresentedEvent += handler`.
    pub fn subscribe_first_image_presented(&self, handler: WindowEventHandler) -> EventHandlerId {
        self.inner.frame.first_image_presented.add(LIST_EVENT, handler)
    }

    /// Removes a subscription made with either subscribe method; an unknown id is a no-op.
    pub fn unsubscribe_first_present(&self, id: EventHandlerId) {
        if !self.inner.frame.first_presented.remove(id) {
            self.inner.frame.first_image_presented.remove(id);
        }
    }
}
