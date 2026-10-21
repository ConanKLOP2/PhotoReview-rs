//! `ShellScrollBars` part 4 (W8-16g, contract `contracts/W8-16g.md`): painting and disposal, `ScrollBars.cs` lines 403-488 (`Draw`, `DrawBars`, `DrawBox`,
//! `Dispose`). Baseline `d92d6aae`.
//!
//! Drawn after the image and before the overlay: track background `Dark.ScrollTrack`, rounded thumb (radius 4 DIP; colour by hover / drag state), the
//! corner square when both bars show. While the compare panel is visible it covers everything but the 8 DIP strips right of and below it, so the bars are
//! drawn only inside those two clip strips.

use pr_app::input::primitives::RectD;
use pr_app::services::render::{DrawContext, RenderError};
use pr_app::viewing::scrollbar_geometry::{ScrollAxis, ScrollBarBox, ScrollBarLayout};

use super::state::{ScrollBarColors, ShellScrollBars};
use crate::shell_viewing::compare;

impl ShellScrollBars {
    /// C# `Draw(context)`: nothing when no bar is visible (and `drawn_frames` is not counted). Otherwise `drawn_frames += 1` and `draw_bars`, once
    /// without a clip, or, with the compare panel visible, once per non-empty strip (`right` = `(panel.right, 0, client.w - panel.right, client.h)`, then
    /// `bottom` = `(0, panel.bottom, client.w, client.h - panel.bottom)`) under `push_clip(strip)` / `pop_clip()`; `pop_clip` runs even when a draw fails.
    pub fn draw(&self, context: &mut dyn DrawContext) -> Result<(), RenderError> {
        let layout = self.current_layout();
        if !layout.any_visible() {
            return Ok(());
        }

        self.drawn_frames.set(self.drawn_frames.get() + 1);
        if !(self.is_compare_visible)() {
            self.draw_bars(context, &layout);
            return Ok(());
        }

        let Some(client) = self.client_size() else {
            return Ok(());
        };
        let panel = compare::layout::panel(client);
        let right = RectD {
            x: panel.right(),
            y: 0.0,
            width: (client.width - panel.right()).max(0.0),
            height: client.height,
        };
        let bottom = RectD {
            x: 0.0,
            y: panel.bottom(),
            width: client.width,
            height: (client.height - panel.bottom()).max(0.0),
        };
        let mut result = Ok(());
        for strip in [right, bottom] {
            if strip.width <= 0.0 || strip.height <= 0.0 {
                continue;
            }
            context.push_clip(strip);
            self.draw_bars(context, &layout);
            let popped = context.pop_clip();
            if result.is_ok() {
                result = popped;
            }
        }
        result
    }

    /// C# `DrawBars(context, layout)`: the vertical box, the horizontal box, then the corner (`fill_rectangle(corner, TRACK)`).
    pub(super) fn draw_bars(&self, context: &mut dyn DrawContext, layout: &ScrollBarLayout) {
        let hovered = self.hovered_thumb_axis();
        self.draw_box(context, &layout.vertical, hovered);
        self.draw_box(context, &layout.horizontal, hovered);
        if layout.has_corner {
            context.fill_rectangle(layout.corner, ScrollBarColors::TRACK);
        }
    }

    /// C# `DrawBox(context, box, hovered)`: nothing for a hidden bar; `fill_rectangle(bar, TRACK)`; no thumb -> stop; the thumb is
    /// `fill_rounded_rectangle(thumb, metrics.thumb_radius, colour)` with `THUMB_PRESSED` while dragging THIS axis, else `THUMB_HOVER` when `hovered`
    /// is this axis, else `THUMB`.
    pub(super) fn draw_box(&self, context: &mut dyn DrawContext, bar: &ScrollBarBox, hovered: Option<ScrollAxis>) {
        if !bar.visible {
            return;
        }
        context.fill_rectangle(bar.bar, ScrollBarColors::TRACK);
        if !bar.thumb_visible {
            return;
        }
        let pressed = self.dragging.get() && self.drag_axis.get() == bar.axis;
        let color = if pressed {
            ScrollBarColors::THUMB_PRESSED
        } else if hovered == Some(bar.axis) {
            ScrollBarColors::THUMB_HOVER
        } else {
            ScrollBarColors::THUMB
        };
        context.fill_rounded_rectangle(bar.thumb, self.options.metrics.thumb_radius, color);
    }

    /// C# `Dispose()`: idempotent; clears drag and paging and unhooks the frame (does not release the capture, the window is closing). `Drop` calls it.
    pub fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.dragging.set(false);
        self.paging.set(false);
        self.unhook_frames();
    }
}

impl Drop for ShellScrollBars {
    fn drop(&mut self) {
        self.dispose();
    }
}
