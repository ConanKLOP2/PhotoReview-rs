//! Port of `PhotoReview.Shell.Win32/Overlay/LayerElement.cs` (WP-23): a one-cell `Grid` without background. Children stack on
//! top of each other, each aligned by itself inside the whole cell.
//!
//! Baseline `d92d6aae`. Owned by W8-15a. Layer opacity and `IsHitTestVisible` apply to every child (the info overlay's
//! auto-hide touches only these two properties of the layer, Q-R34); a child's `Visibility` is never changed.
//!
//! Traps (contract section "Traps"): T6 measure is the max over ALL children (collapsed ones give zero), T7 `find_at`
//! never returns the layer itself (an empty cell is click-through), children are hit-tested topmost first (reverse
//! order), T10 `release_resources` recurses.

use crate::input::primitives::{PointD, SizeD};
use crate::services::render::DrawContext;

use super::contracts::{OverlayElement, OverlayLayoutContext};
use super::element::{ElementBase, ElementNode, net_max};

/// C# `LayerElement`: a `Grid` cell whose children overlap. Children are kept in insertion order (later = drawn on top).
#[derive(Debug)]
pub struct LayerElement {
    base: ElementBase,
    children: Vec<Box<dyn ElementNode>>,
}

impl LayerElement {
    /// C# `LayerElement(string id)`: hit-test visible (the C# base default). Panics on an empty id (see `ElementBase::new`).
    pub fn new(id: &str) -> Self {
        Self {
            base: ElementBase::new(id, true),
            children: Vec::new(),
        }
    }

    /// C# `Children` (`IReadOnlyList`), insertion order.
    pub fn children(&self) -> &[Box<dyn ElementNode>] {
        &self.children
    }

    /// C# `Add(child)`: appends and returns `self` for chaining. `child` is never null in Rust (`Box`).
    pub fn add(&mut self, child: Box<dyn ElementNode>) -> &mut Self {
        self.children.push(child);
        self
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
        &self.base
    }

    fn base_mut(&mut self) -> &mut ElementBase {
        &mut self.base
    }

    /// C# `MeasureCore`: `width = max` of the children's `measure(available, ctx)` widths and `height = max` of their heights.
    /// Each child is measured with the same `available` and is stored as its own `DesiredSize`. Starts at `0` (so no
    /// children gives `(0, 0)`). Uses NaN-propagating max (trap T2).
    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        let mut width = 0.0;
        let mut height = 0.0;
        for child in &mut self.children {
            let size = child.measure(available, context);
            width = net_max(width, size.width);
            height = net_max(height, size.height);
        }
        SizeD { width, height }
    }

    /// C# `ArrangeChildren`: every child, including collapsed ones, is `arrange_in(self.bounds(), ctx)`.
    fn arrange_children(&mut self, context: &OverlayLayoutContext<'_>) {
        let bounds = self.base.bounds();
        for child in &mut self.children {
            child.arrange_in(bounds, context);
        }
    }

    /// C# `RenderCore`: every child in insertion order. The layer's own opacity push is done by the blanket `render`.
    fn render_core(&self, dc: &mut dyn DrawContext) {
        for child in &self.children {
            child.render(dc);
        }
    }

    /// C# `FindAt` override: `None` when the layer is hidden, hit-test disabled, opacity `<= 0`, or `bounds` does not contain
    /// `point`. Otherwise the first `Some` from the children in REVERSE order (topmost first). The layer itself is never
    /// returned (trap T7).
    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        let base = &self.base;
        if !base.is_visible || !base.is_hit_test_visible || base.opacity() <= 0.0 || !base.bounds().contains(point) {
            return None;
        }
        self.children.iter().rev().find_map(|child| child.find_at(point))
    }

    /// C# `Find` override: `self` when the id matches, else the first match in children order (depth-first).
    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.base.id() == id {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }

    /// C# `ReleaseResources` override: forwards to every child (trap T10).
    fn release_resources(&mut self) {
        for child in &mut self.children {
            child.release_resources();
        }
    }
}
