//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.ScrollBars.cs` (34 lines, baseline `d92d6aae`). Unit W8-13i (with `frame`).
//!
//! The glue between the main window and the self-drawn scroll bars of `ImageScroll` (geometry, mouse and painting are `ShellScrollBars`, W8-16f/16g in
//! `shell_viewing::scrollbars`): construct the bars with the window, the viewport, a "compare is visible" query and a press callback; draw them after the
//! image and before the overlay; dispose them when the window is closed. `RegisterScrollBars` must run BEFORE `RegisterInput` (D-W8B4M-3): the bars'
//! mouse handler must see a press before the image's handler does, because the zoomed image's box covers the bar area.

#![allow(unused_variables, dead_code)] // W8-13i stub: the porter removes this line when the last `todo!()` is gone.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_app::services::render::DrawContext;

use super::core::{MainWindow, MainWindowError, MainWindowInner};
use crate::host::contracts::EventHandlerId;
use crate::shell_viewing::scrollbars::ShellScrollBars;

/// The field `_scrollBars` plus the id of the `Closed` subscription of `OnWindowClosedForScrollBars`.
pub struct ScrollBarState {
    /// The concrete bars of 16f/16g (ported BEFORE this unit; no port trait, D-W8B4X-2).
    pub(super) bars: RefCell<Option<Rc<ShellScrollBars>>>,
    pub(super) closed_subscription: Cell<Option<EventHandlerId>>,
}

impl std::fmt::Debug for ScrollBarState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollBarState")
            .field("registered", &self.bars.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl Default for ScrollBarState {
    /// No bars, no subscription.
    fn default() -> Self {
        todo!("W8-13i")
    }
}

impl MainWindowInner {
    /// C# `RegisterScrollBars()`: `ShellScrollBars::new(&window_api, viewport.clone(), is_compare_visible, Some(on_press), Some(options))` with `options.metrics.thickness = viewport.scroll_bar_thickness()` (D-W8B4X-7, 16f; the constructor appends the bars to
    /// the window's handler chain, so this runs BEFORE `register_input`). `is_compare_visible` reads `vm.compare().is_visible()` (short borrow); `on_press` calls
    /// `MainWindowFeature::on_scroll_bar_press` of every feature (the input feature turns it into `OnWindowPreviewMouseDown`, which stops the kinetic glide: a
    /// press on a bar is still a "window preview mouse down"); subscribes `Closed` (id kept in `closed_subscription`).
    pub(super) fn register_scroll_bars(&self) {
        todo!("W8-13i")
    }

    /// C# `OnWindowClosedForScrollBars`: unsubscribe itself, `dispose` the bars (the bars stay in the field, as in C#).
    pub(super) fn on_window_closed_for_scroll_bars(&self) {
        todo!("W8-13i")
    }

    /// C# `DrawScrollBars(context)`: `bars?.draw(context)`; an `Err(RenderError)` is logged ("Frame failed") and ends the frame like any other draw fault (D-W8B4M-8).
    pub(super) fn draw_scroll_bars(&self, context: &mut dyn DrawContext) {
        todo!("W8-13i")
    }
}

impl MainWindow {
    /// C# `ScrollBars` getter: `Err(FeatureNotRegistered { feature: "Scroll bars" })` ("Scroll bars are not registered.") before `register_scroll_bars`.
    pub fn scroll_bars(&self) -> Result<Rc<ShellScrollBars>, MainWindowError> {
        todo!("W8-13i")
    }
}
