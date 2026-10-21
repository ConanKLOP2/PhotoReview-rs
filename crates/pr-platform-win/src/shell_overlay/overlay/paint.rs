//! `ShellOverlay` part 6, drawing and tooltip regions. Port of `ShellOverlay.cs` lines 233 to 245 (`Render`) and 594 to 627 (`SyncTooltips`), baseline
//! `d92d6aae`. Unit W8-15i (contract `contracts/W8-15i.md`). Pure over a `DrawContext` (the recording fake in tests, `D2dDrawContext` in the app).
//!
//! Z order and timing: the overlay is drawn AFTER the image layer, inside the SAME `BeginDraw`/`EndDraw` frame (15j calls `render` from the frame callback
//! of the main window). Roots are drawn in host order: `InfoOverlayHost` first, then the toolbar, so the toolbar is on top.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use pr_app::input::primitives::SizeD;
use pr_app::services::render::DrawContext;

use super::state::ShellOverlay;
use crate::shell_overlay::tooltips::OverlayTooltip;

impl ShellOverlay {
    /// C# `Render(dc, clientDip, dpiScale)` (lines 234 to 245): nothing after dispose. `arrange(client_dip, dpi_scale)`, then `host.render(dc)`, then
    /// `sync_tooltips()` (so the tooltip regions always match the boxes just drawn). The state borrow is released before `sync_tooltips` calls the control.
    pub fn render(&self, dc: &mut dyn DrawContext, client_dip: SizeD, dpi_scale: f64) {
        todo!()
    }

    /// The tooltip regions for the current state (C# `SyncTooltips` lines 601 to 625, without the final call): cleared scratch list, then
    /// - when the toolbar is visible, hit-test enabled and has opacity above 0: every toolbar button with a non-empty `tool_tip`, in Tab order, as
    ///   `OverlayTooltip { id: button id, bounds: button box, text: tool_tip }`;
    /// - when the info layer is hit-test enabled and has opacity above 0: the EXIF line (`StatusPanel` and its EXIF text visible and `model.exif_text` not empty;
    ///   id `ExifInfoText`, the EXIF text box, the model's EXIF text) and the capture badge (visible and non-empty tool tip; id `CapturePairBadge`).
    ///
    /// The order is toolbar buttons, EXIF, badge. A faded-out overlay has no tooltips.
    pub fn collect_tooltips(&self) -> Vec<OverlayTooltip> {
        todo!()
    }

    /// C# `SyncTooltips()`: no tooltip service: nothing. Else `tooltips.sync(collect_tooltips(), last_dpi)`.
    pub(super) fn sync_tooltips(&self) {
        todo!()
    }
}
