//! Port of `PhotoReview.Shell.Win32/Overlay/LayerElement.cs` (WP-23): a one-cell `Grid` without background. Children stack on
//! top of each other, each aligned by itself inside the whole cell.
//!
//! Baseline `d92d6aae`. Owned by W8-15a. Layer opacity and `IsHitTestVisible` apply to every child (the info overlay's
//! auto-hide touches only these two properties of the layer, Q-R34); a child's `Visibility` is never changed.
//!
//! Traps (contract section "Traps"): T6 measure is the max over ALL children (collapsed ones give zero), T7 `find_at`
//! never returns the layer itself (an empty cell is click-through), children are hit-tested topmost first (reverse
//! order), T10 `release_resources` recurses.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::primitives::{PointD, SizeD};
use crate::services::render::DrawContext;

use super::contracts::OverlayLayoutContext;
use super::element::{ElementBase, ElementNode};

/// C# `LayerElement`: a `Grid` cell whose children overlap. Children are kept in insertion order (later = drawn on top).
#[derive(Debug)]
pub struct LayerElement {
    base: ElementBase,
    children: Vec<Box<dyn ElementNode>>,
}

impl LayerElement {
    /// C# `LayerElement(string id)`: hit-test visible (the C# base default). Panics on an empty id (see `ElementBase::new`).
    pub fn new(id: &str) -> Self {
        todo!()
    }

    /// C# `Children` (`IReadOnlyList`), insertion order.
    pub fn children(&self) -> &[Box<dyn ElementNode>] {
        todo!()
    }

    /// C# `Add(child)`: appends and returns `self` for chaining. `child` is never null in Rust (`Box`).
    pub fn add(&mut self, child: Box<dyn ElementNode>) -> &mut Self {
        todo!()
    }
}

impl ElementNode for LayerElement {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn base(&self) -> &ElementBase {
        todo!()
    }

    fn base_mut(&mut self) -> &mut ElementBase {
        todo!()
    }

    /// C# `MeasureCore`: `width = max` of the children's `measure(available, ctx)` widths and `height = max` of their heights.
    /// Each child is measured with the same `available` and is stored as its own `DesiredSize`. Starts at `0` (so no
    /// children gives `(0, 0)`). Uses NaN-propagating max (trap T2).
    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        todo!()
    }

    /// C# `ArrangeChildren`: every child, including collapsed ones, is `arrange_in(self.bounds(), ctx)`.
    fn arrange_children(&mut self, context: &OverlayLayoutContext<'_>) {
        todo!()
    }

    /// C# `RenderCore`: every child in insertion order. The layer's own opacity push is done by the blanket `render`.
    fn render_core(&self, dc: &mut dyn DrawContext) {
        todo!()
    }

    /// C# `FindAt` override: `None` when the layer is hidden, hit-test disabled, opacity `<= 0`, or `bounds` does not contain
    /// `point`. Otherwise the first `Some` from the children in REVERSE order (topmost first). The layer itself is never
    /// returned (trap T7).
    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        todo!()
    }

    /// C# `Find` override: `self` when the id matches, else the first match in children order (depth-first).
    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        todo!()
    }

    /// C# `ReleaseResources` override: forwards to every child (trap T10).
    fn release_resources(&mut self) {
        todo!()
    }
}
