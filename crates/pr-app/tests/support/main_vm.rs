//! Shared, UI-thread-only fixtures for the W8-08 MainViewModel tests.

use pr_app::coordinators::presenter::state::ImageChange;
use pr_app::viewmodels::main::{PresentationBridge, ViewerHandle};
use pr_app::viewmodels::viewer_state::ViewerState;

pub fn bridge() -> (PresentationBridge, ViewerHandle) {
    let viewer = ViewerHandle::new(ViewerState::new());
    (PresentationBridge::new(viewer.clone()), viewer)
}

pub fn image_change<'a>(path: Option<&'a str>, width: i32, height: i32, swap: bool) -> ImageChange<'a> {
    ImageChange {
        presented_path: path,
        original_width: width,
        original_height: height,
        is_same_source_swap: swap,
    }
}
