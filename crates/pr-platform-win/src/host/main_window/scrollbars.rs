//! Port of `PhotoReview.Shell.Win32/ShellMainWindow.ScrollBars.cs` (34 lines, baseline `d92d6aae`). Unit W8-13i (with `frame`).
//!
//! The glue between the main window and the self-drawn scroll bars of `ImageScroll` (geometry, mouse and painting are `ShellScrollBars`, W8-16f/16g in
//! `shell_viewing::scrollbars`): construct the bars with the window, the viewport, a "compare is visible" query and a press callback; draw them after the
//! image and before the overlay; dispose them when the window is closed. `RegisterScrollBars` must run BEFORE `RegisterInput` (D-W8B4M-3): the bars'
//! mouse handler must see a press before the image's handler does, because the zoomed image's box covers the bar area.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pr_app::services::render::DrawContext;

use pr_app::viewing::scrollbar_geometry::ScrollBarMetrics;

use super::core::{MainWindow, MainWindowError, MainWindowInner};
use crate::host::contracts::{EventHandlerId, ShellWindowApi, ShellWindowEvent, WindowMessageHandler};
use crate::shell_viewing::scrollbars::{ScrollBarOptions, ShellScrollBars};

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
        Self {
            bars: RefCell::new(None),
            closed_subscription: Cell::new(None),
        }
    }
}

impl MainWindowInner {
    /// C# `RegisterScrollBars()`: `ShellScrollBars::new(&window_api, viewport.clone(), is_compare_visible, Some(on_press), Some(options))` with `options.metrics.thickness = viewport.scroll_bar_thickness()` (D-W8B4X-7, 16f; the constructor appends the bars to
    /// the window's handler chain, so this runs BEFORE `register_input`). `is_compare_visible` reads `vm.compare().is_visible()` (short borrow); `on_press` calls
    /// `MainWindowFeature::on_scroll_bar_press` of every feature (the input feature turns it into `OnWindowPreviewMouseDown`, which stops the kinetic glide: a
    /// press on a bar is still a "window preview mouse down"); subscribes `Closed` (id kept in `closed_subscription`).
    pub(super) fn register_scroll_bars(&self) {
        // Ownership (the C# window owns the bars): every closure given to the bars holds a `Weak` of this window, so the bars never keep it alive.
        let window_api: Rc<dyn ShellWindowApi> = self.window.clone();
        let weak = self.weak_self.clone();
        let is_compare_visible: Rc<dyn Fn() -> bool> = Rc::new(move || {
            weak.upgrade().is_some_and(|inner| {
                // A short borrow; a view model that is borrowed right now is not showing a compare to the bars.
                inner
                    .view_model
                    .try_borrow()
                    .is_ok_and(|view_model| view_model.compare().is_visible())
            })
        });
        let weak = self.weak_self.clone();
        let on_press: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(inner) = weak.upgrade() {
                // C# `_input?.Pointer.OnWindowPreviewMouseDown()`: a press on a bar is still a "window preview mouse down".
                for feature in inner.feature_snapshot() {
                    feature.on_scroll_bar_press();
                }
            }
        });
        let options = ScrollBarOptions {
            metrics: ScrollBarMetrics {
                thickness: self.viewport.scroll_bar_thickness(),
                ..ScrollBarMetrics::default()
            },
            ..ScrollBarOptions::default()
        };
        let bars = ShellScrollBars::new(
            &window_api,
            Rc::clone(&self.viewport),
            is_compare_visible,
            Some(on_press),
            Some(options),
        );
        *self.scroll_bars.bars.borrow_mut() = Some(bars);

        let weak = self.weak_self.clone();
        let id = self.window.subscribe(
            ShellWindowEvent::Closed,
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.on_window_closed_for_scroll_bars();
                }
            }),
        );
        self.scroll_bars.closed_subscription.set(Some(id));
    }

    /// C# `OnWindowClosedForScrollBars`: unsubscribe itself, `dispose` the bars (the bars stay in the field, as in C#).
    pub(super) fn on_window_closed_for_scroll_bars(&self) {
        if let Some(id) = self.scroll_bars.closed_subscription.take() {
            self.window.unsubscribe(id);
        }
        let bars = self.scroll_bars.bars.borrow().clone();
        if let Some(bars) = bars {
            bars.dispose();
            // The window's handler chain holds the bars, the bars hold the viewport and the viewport holds the window: a cycle the C# GC does not mind.
            // The bars stay in the field (as in C#); the chain lets go of them.
            let handler: Rc<dyn WindowMessageHandler> = bars;
            self.window.remove_message_handler(&handler);
        }
    }

    /// C# `DrawScrollBars(context)`: `bars?.draw(context)`; an `Err(RenderError)` is logged ("Frame failed") and ends the frame like any other draw fault (D-W8B4M-8).
    pub(super) fn draw_scroll_bars(&self, context: &mut dyn DrawContext) {
        let bars = self.scroll_bars.bars.borrow().clone();
        if let Some(bars) = bars {
            if let Err(fault) = bars.draw(context) {
                // C#: the exception ends the frame (`Present` logs it and drops the frame).
                self.log.error("Frame failed", Some(&fault));
                self.frame.draw_failed.set(true);
            }
        }
    }
}

impl MainWindow {
    /// C# `ScrollBars` getter: `Err(FeatureNotRegistered { feature: "Scroll bars" })` ("Scroll bars are not registered.") before `register_scroll_bars`.
    pub fn scroll_bars(&self) -> Result<Rc<ShellScrollBars>, MainWindowError> {
        self.inner
            .scroll_bars
            .bars
            .borrow()
            .clone()
            .ok_or(MainWindowError::FeatureNotRegistered { feature: "Scroll bars" })
    }
}
