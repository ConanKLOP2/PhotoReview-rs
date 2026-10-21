//! `ShellOverlay` part 3, hit testing and hover (PURE). Port of `ShellOverlay.cs` lines 255 to 302 (`PointerMoved`, `PointerLeft`) and 377 to 378
//! (`HitTest`), baseline `d92d6aae`. Unit W8-15h (contract `contracts/W8-15h.md`). All coordinates are client DIP (the message handler divides pixels
//! by the DPI scale). The element-level rule (visible, hit-test enabled, opacity above 0, inside the box; click-through otherwise) is
//! `pr_app::overlay` (`ElementNode::find_at`); this file maps its answer to a button and drives hover.

use pr_app::coordinators::autohide::ToolbarAutoHidePolicy;
use pr_app::input::primitives::PointD;
use pr_app::overlay::element::ElementNode;

use super::state::{HOT_ZONE_MARGIN, OverlayButton, OverlayState, ShellOverlay};

/// The button under `point` in `state` (the deepest hit element, when it is one of the five buttons).
fn button_at(state: &OverlayState, point: PointD) -> Option<OverlayButton> {
    state
        .host
        .hit_test(point)
        .and_then(|element| OverlayButton::from_element_id(element.base().id()))
}

impl ShellOverlay {
    /// C# `HitTest(point)`: `!disposed && host.hit_test(point).is_some()`. True = an overlay element (button or panel, visible, not faded out) takes the click;
    /// false = it falls through to the image.
    pub fn hit_test(&self, point: PointD) -> bool {
        let state = self.state.borrow();
        !state.disposed && state.host.hit_test(point).is_some()
    }

    /// The button under `point`, if the topmost hit element is one of the five buttons (C# `hit as ButtonElement`; `is_enabled` is NOT checked here).
    pub fn hit_button(&self, point: PointD) -> Option<OverlayButton> {
        button_at(&self.state.borrow(), point)
    }

    /// C# `PointerMoved(point)` (returns nothing; the message handler never swallows `WM_MOUSEMOVE`): ignored after dispose. Order: update hover (the
    /// previous hot button loses `is_hot`, the new one gets it, `request_render` only when the hot button changed); `note_info_activity()`; then
    /// `toolbar_mouse_inside_hot_zone = ToolbarAutoHidePolicy::is_inside_hot_zone(x, y, bar.x, bar.y, bar.width, bar.height, HOT_ZONE_MARGIN)` with `bar` = the
    /// toolbar's box; then `apply_toolbar_visibility()`.
    pub fn pointer_moved(&self, point: PointD) {
        let hot_changed = {
            let mut guard = self.state.borrow_mut();
            let state = &mut *guard;
            if state.disposed {
                return;
            }
            let hot = button_at(state, point);
            if hot == state.hot {
                false
            } else {
                if let Some(old) = state.hot
                    && let Some(button) = state.button_mut(old)
                {
                    button.is_hot = false;
                }
                state.hot = hot;
                if let Some(new) = hot
                    && let Some(button) = state.button_mut(new)
                {
                    button.is_hot = true;
                }
                true
            }
        };
        if hot_changed {
            (self.request_render)();
        }

        // Window_MouseMove of MainWindow: NoteInfoActivity first, then the toolbar's hot zone from the toolbar's box.
        self.note_info_activity();
        {
            let mut state = self.state.borrow_mut();
            let bar = state.toolbar().map(|toolbar| toolbar.base().bounds());
            if let Some(bar) = bar {
                state.toolbar_mouse_inside_hot_zone = ToolbarAutoHidePolicy::is_inside_hot_zone(
                    point.x,
                    point.y,
                    bar.x,
                    bar.y,
                    bar.width,
                    bar.height,
                    HOT_ZONE_MARGIN,
                );
            }
        }
        self.apply_toolbar_visibility();
    }

    /// C# `PointerLeft()`: clears the hot button (and `request_render`) if there is one. The hot-zone flag is NOT touched (WPF had no move to update it).
    pub fn pointer_left(&self) {
        let had_hot = {
            let mut guard = self.state.borrow_mut();
            let state = &mut *guard;
            match state.hot.take() {
                Some(old) => {
                    if let Some(button) = state.button_mut(old) {
                        button.is_hot = false;
                    }
                    true
                }
                None => false,
            }
        };
        if had_hot {
            (self.request_render)();
        }
    }
}
