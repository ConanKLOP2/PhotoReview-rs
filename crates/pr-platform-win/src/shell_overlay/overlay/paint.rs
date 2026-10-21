//! `ShellOverlay` part 6, drawing and tooltip regions. Port of `ShellOverlay.cs` lines 233 to 245 (`Render`) and 594 to 627 (`SyncTooltips`), baseline
//! `d92d6aae`. Unit W8-15i (contract `contracts/W8-15i.md`). Pure over a `DrawContext` (the recording fake in tests, `D2dDrawContext` in the app).
//!
//! Z order and timing: the overlay is drawn AFTER the image layer, inside the SAME `BeginDraw`/`EndDraw` frame (15j calls `render` from the frame callback
//! of the main window). Roots are drawn in host order: `InfoOverlayHost` first, then the toolbar, so the toolbar is on top.

use pr_app::input::primitives::SizeD;
use pr_app::overlay::capture_badge::{self, CapturePairBadge};
use pr_app::overlay::element::ElementNode;
use pr_app::overlay::status::{self, StatusPanel};
use pr_app::services::render::DrawContext;

use super::state::ShellOverlay;
use crate::shell_overlay::tooltips::OverlayTooltip;

impl ShellOverlay {
    /// C# `Render(dc, clientDip, dpiScale)` (lines 234 to 245): nothing after dispose. `arrange(client_dip, dpi_scale)`, then `host.render(dc)`, then
    /// `sync_tooltips()` (so the tooltip regions always match the boxes just drawn). The state borrow is released before `sync_tooltips` calls the control.
    pub fn render(&self, dc: &mut dyn DrawContext, client_dip: SizeD, dpi_scale: f64) {
        if self.is_disposed() {
            return;
        }
        self.arrange(client_dip, dpi_scale);
        self.state.borrow().host.render(dc);
        self.sync_tooltips();
    }

    /// The tooltip regions for the current state (C# `SyncTooltips` lines 601 to 625, without the final call): cleared scratch list, then
    /// - when the toolbar is visible, hit-test enabled and has opacity above 0: every toolbar button with a non-empty `tool_tip`, in Tab order, as
    ///   `OverlayTooltip { id: button id, bounds: button box, text: tool_tip }`;
    /// - when the info layer is hit-test enabled and has opacity above 0: the EXIF line (`StatusPanel` and its EXIF text visible and `model.exif_text` not empty;
    ///   id `ExifInfoText`, the EXIF text box, the model's EXIF text) and the capture badge (visible and non-empty tool tip; id `CapturePairBadge`).
    ///
    /// The order is toolbar buttons, EXIF, badge. A faded-out overlay has no tooltips.
    pub fn collect_tooltips(&self) -> Vec<OverlayTooltip> {
        let state = self.state.borrow();
        let mut tools = Vec::new();
        if let Some(toolbar) = state.toolbar() {
            let base = toolbar.base();
            if base.is_visible && base.is_hit_test_visible && base.opacity() > 0.0 {
                for button in toolbar.buttons() {
                    if !button.tool_tip.is_empty() {
                        tools.push(OverlayTooltip {
                            id: button.base().id().to_owned(),
                            bounds: button.base().bounds(),
                            text: button.tool_tip.clone(),
                        });
                    }
                }
            }
        }
        if let Some(layer) = state.info_layer() {
            let base = layer.base();
            if base.is_hit_test_visible && base.opacity() > 0.0 {
                if let Some(panel) = layer.child::<StatusPanel>(0)
                    && panel.base().is_visible
                    && panel.exif().base().is_visible
                    && !state.model.exif_text.is_empty()
                {
                    tools.push(OverlayTooltip {
                        id: status::EXIF_TEXT_ID.to_owned(),
                        bounds: panel.exif().base().bounds(),
                        text: state.model.exif_text.clone(),
                    });
                }
                if let Some(badge) = layer.child::<CapturePairBadge>(3)
                    && badge.base().is_visible
                    && !badge.tool_tip().is_empty()
                {
                    tools.push(OverlayTooltip {
                        id: capture_badge::PANEL_ID.to_owned(),
                        bounds: badge.base().bounds(),
                        text: badge.tool_tip().to_owned(),
                    });
                }
            }
        }
        tools
    }

    /// C# `SyncTooltips()`: no tooltip service: nothing. Else `tooltips.sync(collect_tooltips(), last_dpi)`.
    pub(super) fn sync_tooltips(&self) {
        let Some(tooltips) = &self.tooltips else {
            return;
        };
        let mut scratch = std::mem::take(&mut self.state.borrow_mut().tooltip_scratch);
        scratch.clear();
        scratch.extend(self.collect_tooltips());
        let dpi = self.last_dpi();
        tooltips.sync(&scratch, dpi);
        self.state.borrow_mut().tooltip_scratch = scratch;
    }
}
