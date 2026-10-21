//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.Frame.cs` (169 lines, baseline `d92d6aae`): `RenderFrame`, `RenderNow`, `Present`, `DrawContent`,
//! `OnFramePresented` and the two first-present events. Unit W8-13i (with `scrollbars`). Contract: `contracts/W8-13i.md`.
//!
//! One frame = size check, pacer decision, surface resize (also for a dpi-only change), promote a finished upgrade, run a posted layout, `BeginDraw`,
//! clear to the canvas colour, draw the presentation (13j), the scroll bars (this unit), the overlay (15j, through `MainWindowFeature::draw_overlay`),
//! `EndDraw` + `Present`, statistics, device-loss redraw and the first-present marks.

#![allow(unused_variables, dead_code)] // W8-13i stub: the porter removes this line when the last `todo!()` is gone.

use std::cell::Cell;

use pr_app::services::render::DrawContext;

use super::core::{MainWindow, MainWindowInner};
use crate::host::contracts::{EventHandlerId, WindowEventHandler};
use crate::host::shell_window::create::EventList;

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
        todo!("W8-13i")
    }
}

impl MainWindowInner {
    /// C# `RenderFrame()`: the `ShellWindow::subscribe_render` handler; calls [`present`](Self::present).
    pub(super) fn render_frame(&self) {
        todo!("W8-13i")
    }

    /// C# `RenderNow()`: layout + draw + present synchronously, not waiting for the frame clock (first frame, first image). A drawing error is logged
    /// ("Frame failed"), the frame is dropped; it never propagates.
    pub(crate) fn render_now(&self) {
        todo!("W8-13i")
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
        todo!("W8-13i")
    }

    /// C# `DrawContent(context)`: `draw_presentation(context)` (13j) then `draw_scroll_bars(context)` (scroll bars lie over the image, under the overlay).
    pub(super) fn draw_content(&self, context: &mut dyn DrawContext) {
        todo!("W8-13i")
    }

    /// C# `OnFramePresented()` (lines 130-159): `image_drawn = take(image_drawn_in_frame)`; `first_image = image_drawn && !first_image_drawn` (then
    /// `first_image_drawn = true`); `hub.note_frame_presented(image_drawn)`; the first time ever: `first_present_done = true`, mark `firstPresent`,
    /// `cloak.note_first_present()`, raise `FirstPresented`; if `first_image`: `keep_warm.warm_at_launch()` when `options.activate`, raise `FirstImagePresented`.
    pub(super) fn on_frame_presented(&self) {
        todo!("W8-13i")
    }
}

impl MainWindow {
    /// C# `RenderNow()` (`internal`: the startup code and tests call it).
    pub fn render_now(&self) {
        todo!("W8-13i")
    }

    /// C# `FirstImagePresented`.
    pub fn first_image_presented(&self) -> bool {
        todo!("W8-13i")
    }

    /// C# `FirstPresented += handler`.
    pub fn subscribe_first_presented(&self, handler: WindowEventHandler) -> EventHandlerId {
        todo!("W8-13i")
    }

    /// C# `FirstImagePresentedEvent += handler`.
    pub fn subscribe_first_image_presented(&self, handler: WindowEventHandler) -> EventHandlerId {
        todo!("W8-13i")
    }

    /// Removes a subscription made with either subscribe method; an unknown id is a no-op.
    pub fn unsubscribe_first_present(&self, id: EventHandlerId) {
        todo!("W8-13i")
    }
}
