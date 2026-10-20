//! Port of `PhotoReview.App.Shared/Input/PointerInputController.cs` (672 lines, baseline `d92d6aae`), W8-05h part 1: click-to-zoom
//! (`ClickZoomAsync`), the ClickZoom shortcut (`ToggleClickZoomAsync`) and the context-menu "Click zoom level"
//! (`SetClickZoomLevelAsync`). Contract: `contracts/W8-05h.md`. The struct is defined in `wheel.rs` (W8-05g); this file adds an
//! `impl` block. Same conventions as `zoom.rs`: a C# `await` is a [`PointerStep`], the zoom is a [`ZoomAction`].

use crate::input::mouse_gestures::{ClickZoomTarget, click_zoom_factor, decide_click_zoom};
use crate::input::pointer::wheel::{PointerContext, PointerInputController};
use crate::input::pointer::zoom::{PointerStep, ZoomAction, viewport_centre};
use crate::input::primitives::PointD;

impl PointerInputController {
    /// C# private `ClickZoomAsync(mouse)`: `target = click_zoom_factor(settings.click_zoom_percent)`. When
    /// `decide_click_zoom(viewer.is_fit(), viewer.zoom(), target) == ClickZoomTarget::Fit`: `commands.apply_fit()` (posts the Fit,
    /// no render yield here) and `Done`. Otherwise `zoom_at_point(mouse, ZoomAction::SetZoom(target))`.
    /// Called by `on_image_release` with the release point (the mouse click always anchors at the cursor) and by
    /// `toggle_click_zoom` with the keyboard anchor.
    pub(super) fn click_zoom(&mut self, ctx: &mut PointerContext<'_>, mouse: PointD) -> PointerStep {
        let target = click_zoom_factor(ctx.settings.click_zoom_percent);
        if decide_click_zoom(ctx.viewer.is_fit(), ctx.viewer.zoom(), target) == ClickZoomTarget::Fit {
            ctx.commands.apply_fit();
            PointerStep::Done
        } else {
            self.zoom_at_point(ctx, mouse, ZoomAction::SetZoom(target))
        }
    }

    /// C# `ToggleClickZoomAsync()` (the ClickZoom shortcut; works even when `click_to_zoom_enabled` is off). In this order:
    /// 1. no images: `Done`, nothing touched (no `cancel_pan`, no `stop_kinetic`);
    /// 2. `cancel_pan`, `stop_kinetic`;
    /// 3. when `!settings.click_zoom_key_toggles_fit` and `decide_click_zoom(is_fit, zoom, click_zoom_factor(percent)) == Fit`
    ///    (already at the click zoom): `Done` with NO zoom and no scroll ("the key means go TO the click zoom, never back to Fit");
    /// 4. else `click_zoom(resolve_keyboard_anchor())`. With `click_zoom_key_toggles_fit` at the click zoom this applies Fit.
    pub fn toggle_click_zoom(&mut self, ctx: &mut PointerContext<'_>) -> PointerStep {
        if !ctx.commands.has_images() {
            return PointerStep::Done;
        }
        self.cancel_pan(ctx.surface);
        self.stop_kinetic(ctx.surface);
        let already_there = decide_click_zoom(
            ctx.viewer.is_fit(),
            ctx.viewer.zoom(),
            click_zoom_factor(ctx.settings.click_zoom_percent),
        ) == ClickZoomTarget::Fit;
        if !ctx.settings.click_zoom_key_toggles_fit && already_there {
            return PointerStep::Done;
        }
        let anchor = self.resolve_keyboard_anchor(ctx);
        self.click_zoom(ctx, anchor)
    }

    /// C# `SetClickZoomLevelAsync(percent)` (context menu): no images gives `Done`; else `cancel_pan`, `stop_kinetic`,
    /// `centre = viewport_centre` (the viewport centre, NOT the keyboard anchor and NOT the cursor), `target =
    /// click_zoom_factor(percent)`, then `zoom_at_point(centre, ZoomAction::SetZoom(target))`. Never toggles Fit, so calling it
    /// twice with the same percent zooms twice.
    pub fn set_click_zoom_level(&mut self, ctx: &mut PointerContext<'_>, percent: i32) -> PointerStep {
        if !ctx.commands.has_images() {
            return PointerStep::Done;
        }
        self.cancel_pan(ctx.surface);
        self.stop_kinetic(ctx.surface);
        let centre = viewport_centre(ctx.surface);
        let target = click_zoom_factor(percent);
        self.zoom_at_point(ctx, centre, ZoomAction::SetZoom(target))
    }
}
