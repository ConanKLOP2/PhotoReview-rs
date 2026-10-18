//! Port of `PhotoReview.App.Shared/Viewport/ViewportState.cs` (WP-16, `internal sealed class` in C#).
//!
//! The state of one viewport: the input last set, the layout computed from it, and the offsets. Pure, one thread (the UI
//! thread of the owner). It keeps two WPF behaviours that `PointerInputController` and `FitViewController` rely on:
//! - **Delayed layout:** `set_input` and `scroll_to` change nothing visible; `update_layout` applies them. The owner calls
//!   `update_layout` whenever `is_layout_pending()` is true.
//! - **Remembered request:** the requested offset is clamped only below 0 when it is written; the effective offset is clamped
//!   to `[0, max]` on each layout. A viewport that shrinks and regrows keeps the request.
//!
//! The layout engine is a seam (decision D-W8-01a-1): `LayoutEngine` is implemented by `viewport::layout::ViewportLayoutEngine`
//! (W8-01b). This unit's tests use a scripted or hand-written engine, so it does not depend on W8-01b.
//! Baseline `d92d6aae`.

use crate::input::primitives::PointD;
use crate::viewport::types::{ViewportInput, ViewportLayout};

/// The two engine functions of C# `ViewportLayoutEngine` that `ViewportState` calls.
pub trait LayoutEngine {
    /// C# `ViewportLayoutEngine.Compute(in ViewportInput)`: a pure function of the input.
    fn compute(&self, input: &ViewportInput) -> ViewportLayout;

    /// C# `ViewportLayoutEngine.ClampOffset(in ViewportLayout, double, double)`: returns `(horizontal, vertical)`.
    fn clamp_offset(&self, layout: &ViewportLayout, horizontal: f64, vertical: f64) -> (f64, f64);
}

/// `ViewportState`. C# fields: `_input`, `_hasInput`, `_inputDirty` (starts true), `_scrollPending`, `_requestedHorizontal`,
/// `_requestedVertical`; plus the public `Layout`, `HorizontalOffset`, `VerticalOffset`.
#[derive(Debug)]
pub struct ViewportState<E: LayoutEngine> {
    engine: E,
    input: ViewportInput,
    has_input: bool,
    input_dirty: bool,
    scroll_pending: bool,
    requested_horizontal: f64,
    requested_vertical: f64,
    layout: ViewportLayout,
    horizontal_offset: f64,
    vertical_offset: f64,
}

impl<E: LayoutEngine> ViewportState<E> {
    /// C# `new ViewportState()`: default input, `is_layout_pending()` true, all offsets 0.
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            input: ViewportInput::default(),
            has_input: false,
            input_dirty: true,
            scroll_pending: false,
            requested_horizontal: 0.0,
            requested_vertical: 0.0,
            layout: ViewportLayout::default(),
            horizontal_offset: 0.0,
            vertical_offset: 0.0,
        }
    }

    /// C# `Input`: the input last accepted by `set_input` (it may not be laid out yet).
    pub fn input(&self) -> ViewportInput {
        self.input
    }

    /// C# `Layout`: the layout of the last `update_layout` that computed one.
    pub fn layout(&self) -> ViewportLayout {
        self.layout
    }

    /// C# `HorizontalOffset`: the effective offset after the last layout (clamped).
    pub fn horizontal_offset(&self) -> f64 {
        self.horizontal_offset
    }

    /// C# `VerticalOffset`.
    pub fn vertical_offset(&self) -> f64 {
        self.vertical_offset
    }

    /// C# `IsLayoutPending` = `_inputDirty || _scrollPending`.
    pub fn is_layout_pending(&self) -> bool {
        self.input_dirty || self.scroll_pending
    }

    /// C# `SetInput(in ViewportInput)`. If `has_input` and the new input equals the stored one (NaN-aware, `0.0 == -0.0`),
    /// nothing changes. Otherwise store it and mark the input dirty. Computes nothing.
    pub fn set_input(&mut self, input: ViewportInput) {
        if self.has_input && input == self.input {
            return;
        }
        self.input = input;
        self.has_input = true;
        self.input_dirty = true;
    }

    /// C# `ScrollTo(horizontal, vertical)`: `ScrollToHorizontalOffset` then `ScrollToVerticalOffset`. Each axis is stored
    /// as `max(0, value)` with +0.0 for any value <= 0 (`-0.0` becomes `+0.0`); a `NaN` axis keeps its old request.
    /// Marks the scroll pending. Applies nothing.
    pub fn scroll_to(&mut self, horizontal: f64, vertical: f64) {
        if !horizontal.is_nan() {
            self.requested_horizontal = non_negative(horizontal);
        }
        if !vertical.is_nan() {
            self.requested_vertical = non_negative(vertical);
        }
        self.scroll_pending = true;
    }

    /// C# `ScrollHome()` = `scroll_to(0.0, 0.0)`.
    pub fn scroll_home(&mut self) {
        self.scroll_to(0.0, 0.0);
    }

    /// C# `UpdateLayout()`. Returns false without any engine call when nothing is pending. Otherwise: if the input is dirty,
    /// `layout = engine.compute(&input)` and the dirty flag clears; then `scroll_pending = false` and the offsets become
    /// `engine.clamp_offset(&layout, requested_horizontal, requested_vertical)`. Returns true iff the (layout, horizontal,
    /// vertical) triple changed. The layout compares with record-struct equality (`double.Equals`); the two offsets compare with
    /// IEEE `!=` (C# tuple inequality), so a NaN offset always counts as changed and `0.0 == -0.0`.
    pub fn update_layout(&mut self) -> bool {
        if !self.is_layout_pending() {
            return false;
        }

        let previous_layout = self.layout;
        let previous_horizontal = self.horizontal_offset;
        let previous_vertical = self.vertical_offset;

        if self.input_dirty {
            self.layout = self.engine.compute(&self.input);
            self.input_dirty = false;
        }
        self.scroll_pending = false;

        let (horizontal, vertical) =
            self.engine
                .clamp_offset(&self.layout, self.requested_horizontal, self.requested_vertical);
        self.horizontal_offset = horizontal;
        self.vertical_offset = vertical;

        self.layout != previous_layout || previous_horizontal != horizontal || previous_vertical != vertical
    }

    /// C# `ImageOrigin`: `(layout.image_rect.x - horizontal_offset, layout.image_rect.y - vertical_offset)`.
    pub fn image_origin(&self) -> PointD {
        PointD {
            x: self.layout.image_rect.x - self.horizontal_offset,
            y: self.layout.image_rect.y - self.vertical_offset,
        }
    }

    /// C# `ToImageElement(PointD viewportPoint)`: `viewport_point - image_origin()`, component-wise.
    pub fn to_image_element(&self, viewport_point: PointD) -> PointD {
        let origin = self.image_origin();
        PointD {
            x: viewport_point.x - origin.x,
            y: viewport_point.y - origin.y,
        }
    }

    /// C# `ImageActualWidth` = `layout.image_rect.width`.
    pub fn image_actual_width(&self) -> f64 {
        self.layout.image_rect.width
    }

    /// C# `ImageActualHeight` = `layout.image_rect.height`.
    pub fn image_actual_height(&self) -> f64 {
        self.layout.image_rect.height
    }
}

/// C# `Math.Max(0.0, value)` for a non-NaN value: `+0.0` for any value <= 0, so `-0.0` becomes `+0.0`.
fn non_negative(value: f64) -> f64 {
    if value > 0.0 { value } else { 0.0 }
}
