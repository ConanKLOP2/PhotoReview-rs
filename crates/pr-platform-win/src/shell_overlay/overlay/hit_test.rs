//! `ShellOverlay` part 3, hit testing and hover (PURE). Port of `ShellOverlay.cs` lines 255 to 302 (`PointerMoved`, `PointerLeft`) and 377 to 378
//! (`HitTest`), baseline `d92d6aae`. Unit W8-15h (contract `contracts/W8-15h.md`). All coordinates are client DIP (the message handler divides pixels
//! by the DPI scale). The element-level rule (visible, hit-test enabled, opacity above 0, inside the box; click-through otherwise) is
//! `pr_app::overlay` (`ElementNode::find_at`); this file maps its answer to a button and drives hover.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use pr_app::input::primitives::PointD;

use super::state::{OverlayButton, ShellOverlay};

impl ShellOverlay {
    /// C# `HitTest(point)`: `!disposed && host.hit_test(point).is_some()`. True = an overlay element (button or panel, visible, not faded out) takes the click;
    /// false = it falls through to the image.
    pub fn hit_test(&self, point: PointD) -> bool {
        todo!()
    }

    /// The button under `point`, if the topmost hit element is one of the five buttons (C# `hit as ButtonElement`; `is_enabled` is NOT checked here).
    pub fn hit_button(&self, point: PointD) -> Option<OverlayButton> {
        todo!()
    }

    /// C# `PointerMoved(point)` (returns nothing; the message handler never swallows `WM_MOUSEMOVE`): ignored after dispose. Order: update hover (the
    /// previous hot button loses `is_hot`, the new one gets it, `request_render` only when the hot button changed); `note_info_activity()`; then
    /// `toolbar_mouse_inside_hot_zone = ToolbarAutoHidePolicy::is_inside_hot_zone(x, y, bar.x, bar.y, bar.width, bar.height, HOT_ZONE_MARGIN)` with `bar` = the
    /// toolbar's box; then `apply_toolbar_visibility()`.
    pub fn pointer_moved(&self, point: PointD) {
        todo!()
    }

    /// C# `PointerLeft()`: clears the hot button (and `request_render`) if there is one. The hot-zone flag is NOT touched (WPF had no move to update it).
    pub fn pointer_left(&self) {
        todo!()
    }
}
