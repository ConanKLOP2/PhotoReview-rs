//! Viewer operations used by `PhotoReview.App.Shared/Input/PointerInputController.cs` (baseline d92d6aae).
//!
//! A shell sharing its viewer with layout callbacks must borrow only for an individual operation.
//! Holding a RefMut throughout a pointer call prevents synchronous source-size and layout callbacks
//! from reading the same live viewer. Pure callers can continue passing their owned ViewerState.

use crate::viewmodels::viewer_state::{ViewerState, ViewerUpdate};
use pr_core::model::enums::InitialViewMode;

pub trait PointerViewer {
    fn is_fit(&self) -> bool;
    fn zoom(&self) -> f64;
    fn set_zoom(&mut self, value: f64) -> ViewerUpdate;
    fn zoom_to_actual_size(&mut self) -> ViewerUpdate;
    fn zoom_to_fit_width(&mut self) -> ViewerUpdate;
    fn zoom_to_fit_height(&mut self) -> ViewerUpdate;
    fn zoom_in(&mut self) -> ViewerUpdate;
    fn zoom_out(&mut self) -> ViewerUpdate;
    fn wheel_zoom(&mut self, delta: i32) -> ViewerUpdate;
    fn update_viewport(&mut self, width: f64, height: f64, force: bool) -> ViewerUpdate;
    fn apply_initial_view_mode(
        &mut self,
        mode: InitialViewMode,
        width: f64,
        height: f64,
        click_zoom_percent: i32,
        keep_zoom: bool,
    ) -> Option<ViewerUpdate>;
}

macro_rules! forward_update {
    ($name:ident($($arg:ident: $kind:ty),* $(,)?) -> $result:ty) => {
        fn $name(&mut self, $($arg: $kind),*) -> $result {
            ViewerState::$name(self, $($arg),*)
        }
    };
}

impl PointerViewer for ViewerState {
    fn is_fit(&self) -> bool {
        ViewerState::is_fit(self)
    }
    fn zoom(&self) -> f64 {
        ViewerState::zoom(self)
    }
    forward_update!(set_zoom(value: f64) -> ViewerUpdate);
    forward_update!(zoom_to_actual_size() -> ViewerUpdate);
    forward_update!(zoom_to_fit_width() -> ViewerUpdate);
    forward_update!(zoom_to_fit_height() -> ViewerUpdate);
    forward_update!(zoom_in() -> ViewerUpdate);
    forward_update!(zoom_out() -> ViewerUpdate);
    forward_update!(wheel_zoom(delta: i32) -> ViewerUpdate);
    forward_update!(update_viewport(width: f64, height: f64, force: bool) -> ViewerUpdate);
    forward_update!(
        apply_initial_view_mode(
            mode: InitialViewMode,
            width: f64,
            height: f64,
            click_zoom_percent: i32,
            keep_zoom: bool,
        ) -> Option<ViewerUpdate>
    );
}
