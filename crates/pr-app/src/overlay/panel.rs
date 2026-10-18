//! Port of `PhotoReview.Shell.Win32/Overlay/PanelElement.cs` (166 lines, baseline d92d6aae, WP-17): the WPF `Border` + `StackPanel`
//! of the overlay. Rounded background, padding, children stacked vertically or horizontally. Pure: no `unsafe`, no `windows`
//! types (AGENTS.md). Contract: `contracts/W8-15d.md`. Stub: every body is `todo!()`.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::element::{ElementBase, ElementNode, OverlayLayoutContext, OverlayThickness};
use crate::services::render::surface::DrawContext;
use crate::viewport::render_geometry::ColorF;

/// `OverlayOrientation` (C# `Vertical = 0`, `Horizontal = 1`), declared in `PanelElement.cs`. Default `Vertical`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayOrientation {
    #[default]
    Vertical = 0,
    Horizontal = 1,
}

/// `PanelElement : OverlayElement`. The children are owned, in insertion order (C# `_children`).
#[derive(Debug)]
pub struct PanelElement {
    base: ElementBase,
    children: Vec<Box<dyn ElementNode>>,
    /// C# `Background`, default `DarkPalette.Overlay` (W8-15b, pinned in `deps-pin`). Alpha only, drawn when `a > 0`.
    pub background: ColorF,
    /// C# `CornerRadius`, default 0. A radius `> 0` uses the rounded fill.
    pub corner_radius: f64,
    /// C# `Padding`, default zero.
    pub padding: OverlayThickness,
    /// C# `Orientation`, default `Vertical`.
    pub orientation: OverlayOrientation,
}

impl PanelElement {
    /// C# `PanelElement(string id)`: empty children, the defaults above. `id` is the C# `Id` (`base.id`).
    pub fn new(id: &str) -> Self {
        todo!()
    }

    /// C# `Children` (`IReadOnlyList<OverlayElement>`), insertion order.
    pub fn children(&self) -> &[Box<dyn ElementNode>] {
        todo!()
    }

    /// C# `Add(child)`: appends and returns `self` for chaining. Rust has no null, so the C# `ArgumentNullException` is a type fact.
    pub fn add(&mut self, child: Box<dyn ElementNode>) -> &mut Self {
        todo!()
    }

    /// C# `Remove(child)`: removes the FIRST child whose `id()` equals `child_id` (ordinal) and returns true; false when none.
    /// C# removes by reference; Rust removes by id (open question 2 in contract.md).
    pub fn remove(&mut self, child_id: &str) -> bool {
        todo!()
    }

    /// Typed read of the child at `index` (insertion order). `None` when the index is out of range or the child is not a `T`.
    /// `CapturePairBadge` and `FolderInfoPanel` use it for their text child (index 0), replacing the C# `Text` property.
    pub fn child<T: ElementNode + 'static>(&self, index: usize) -> Option<&T> {
        todo!()
    }

    /// Typed mutable access to the child at `index`; same rules as [`PanelElement::child`].
    pub fn child_mut<T: ElementNode + 'static>(&mut self, index: usize) -> Option<&mut T> {
        todo!()
    }
}

impl ElementNode for PanelElement {
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

    /// C# `MeasureCore`: inner size = available minus padding (floor 0); vertical sums heights and takes the widest width,
    /// horizontal sums widths and takes the tallest height; each child is measured with the room left on the stacking axis
    /// (vertical: full inner width, the rest of the inner height; horizontal: the rest of the inner width, full inner height).
    /// Result = stacked size plus padding.
    fn measure_core(&mut self, available: SizeD, ctx: &OverlayLayoutContext<'_>) -> SizeD {
        todo!()
    }

    /// C# `ArrangeChildren`: starts at `Bounds` plus the left/top padding; hidden children get a zero-size slot at the cursor
    /// and do not move it; visible ones take their `desired_size` along the stacking axis and the full inner size across it.
    fn arrange_children(&mut self, ctx: &OverlayLayoutContext<'_>) {
        todo!()
    }

    /// C# `RenderCore`: fills the bounds first when `background.a > 0` and both bounds dimensions are `> 0` (rounded when
    /// `corner_radius > 0`), then renders every child in order (hidden children skip themselves in the base `render`).
    fn render_core(&self, dc: &mut dyn DrawContext) {
        todo!()
    }

    /// C# `FindAt`: `None` unless this element hits; then the LAST child (topmost) that hits wins, recursively; otherwise `self`.
    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        todo!()
    }

    /// C# `Find(id)`: `self` when its id matches (ordinal), else the first child whose subtree finds it, in order.
    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        todo!()
    }

    /// C# `ReleaseResources`: forwards to every child (a panel holds no native resource of its own).
    fn release_resources(&mut self) {
        todo!()
    }
}
