//! `ShellScrollBars` part 4 (W8-16g, contract `contracts/W8-16g.md`): painting and disposal, `ScrollBars.cs` lines 403-488 (`Draw`, `DrawBars`, `DrawBox`,
//! `Dispose`). Baseline `d92d6aae`.
//!
//! Drawn after the image and before the overlay: track background `Dark.ScrollTrack`, rounded thumb (radius 4 DIP; colour by hover / drag state), the
//! corner square when both bars show. While the compare panel is visible it covers everything but the 8 DIP strips right of and below it, so the bars are
//! drawn only inside those two clip strips.

// Contract stub: the bodies are `todo!()` until the W8-16g port; removed with the last `todo!()`.
#![allow(unused_variables, dead_code)]

use pr_app::services::render::{DrawContext, RenderError};
use pr_app::viewing::scrollbar_geometry::{ScrollAxis, ScrollBarBox, ScrollBarLayout};

use super::state::ShellScrollBars;

impl ShellScrollBars {
    /// C# `Draw(context)`: nothing when no bar is visible (and `drawn_frames` is not counted). Otherwise `drawn_frames += 1` and `draw_bars`, once
    /// without a clip, or, with the compare panel visible, once per non-empty strip (`right` = `(panel.right, 0, client.w - panel.right, client.h)`, then
    /// `bottom` = `(0, panel.bottom, client.w, client.h - panel.bottom)`) under `push_clip(strip)` / `pop_clip()`; `pop_clip` runs even when a draw fails.
    pub fn draw(&self, context: &mut dyn DrawContext) -> Result<(), RenderError> {
        todo!("W8-16g")
    }

    /// C# `DrawBars(context, layout)`: the vertical box, the horizontal box, then the corner (`fill_rectangle(corner, TRACK)`).
    pub(super) fn draw_bars(&self, context: &mut dyn DrawContext, layout: &ScrollBarLayout) {
        todo!("W8-16g")
    }

    /// C# `DrawBox(context, box, hovered)`: nothing for a hidden bar; `fill_rectangle(bar, TRACK)`; no thumb -> stop; the thumb is
    /// `fill_rounded_rectangle(thumb, metrics.thumb_radius, colour)` with `THUMB_PRESSED` while dragging THIS axis, else `THUMB_HOVER` when `hovered`
    /// is this axis, else `THUMB`.
    pub(super) fn draw_box(&self, context: &mut dyn DrawContext, bar: &ScrollBarBox, hovered: Option<ScrollAxis>) {
        todo!("W8-16g")
    }

    /// C# `Dispose()`: idempotent; clears drag and paging and unhooks the frame (does not release the capture, the window is closing). `Drop` calls it.
    pub fn dispose(&self) {
        todo!("W8-16g")
    }
}

impl Drop for ShellScrollBars {
    fn drop(&mut self) {
        // The port calls `self.dispose()` here (idempotent); the stub has nothing to release.
    }
}
