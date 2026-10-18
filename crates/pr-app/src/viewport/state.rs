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

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

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
        todo!()
    }

    /// C# `Input`: the input last accepted by `set_input` (it may not be laid out yet).
    pub fn input(&self) -> ViewportInput {
        todo!()
    }

    /// C# `Layout`: the layout of the last `update_layout` that computed one.
    pub fn layout(&self) -> ViewportLayout {
        todo!()
    }

    /// C# `HorizontalOffset`: the effective offset after the last layout (clamped).
    pub fn horizontal_offset(&self) -> f64 {
        todo!()
    }

    /// C# `VerticalOffset`.
    pub fn vertical_offset(&self) -> f64 {
        todo!()
    }

    /// C# `IsLayoutPending` = `_inputDirty || _scrollPending`.
    pub fn is_layout_pending(&self) -> bool {
        todo!()
    }

    /// C# `SetInput(in ViewportInput)`. If `has_input` and the new input equals the stored one (NaN-aware, `0.0 == -0.0`),
    /// nothing changes. Otherwise store it and mark the input dirty. Computes nothing.
    pub fn set_input(&mut self, input: ViewportInput) {
        todo!()
    }

    /// C# `ScrollTo(horizontal, vertical)`: `ScrollToHorizontalOffset` then `ScrollToVerticalOffset`. Each axis is stored
    /// as `max(0, value)` with +0.0 for any value <= 0 (`-0.0` becomes `+0.0`); a `NaN` axis keeps its old request.
    /// Marks the scroll pending. Applies nothing.
    pub fn scroll_to(&mut self, horizontal: f64, vertical: f64) {
        todo!()
    }

    /// C# `ScrollHome()` = `scroll_to(0.0, 0.0)`.
    pub fn scroll_home(&mut self) {
        todo!()
    }

    /// C# `UpdateLayout()`. Returns false without any engine call when nothing is pending. Otherwise: if the input is dirty,
    /// `layout = engine.compute(&input)` and the dirty flag clears; then `scroll_pending = false` and the offsets become
    /// `engine.clamp_offset(&layout, requested_horizontal, requested_vertical)`. Returns true iff the (layout, horizontal,
    /// vertical) triple changed, compared NaN-aware with `0.0 == -0.0` (C# `previous != (...)` tuple inequality).
    pub fn update_layout(&mut self) -> bool {
        todo!()
    }

    /// C# `ImageOrigin`: `(layout.image_rect.x - horizontal_offset, layout.image_rect.y - vertical_offset)`.
    pub fn image_origin(&self) -> PointD {
        todo!()
    }

    /// C# `ToImageElement(PointD viewportPoint)`: `viewport_point - image_origin()`, component-wise.
    pub fn to_image_element(&self, viewport_point: PointD) -> PointD {
        todo!()
    }

    /// C# `ImageActualWidth` = `layout.image_rect.width`.
    pub fn image_actual_width(&self) -> f64 {
        todo!()
    }

    /// C# `ImageActualHeight` = `layout.image_rect.height`.
    pub fn image_actual_height(&self) -> f64 {
        todo!()
    }
}
