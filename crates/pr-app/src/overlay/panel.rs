//! Port of `PhotoReview.Shell.Win32/Overlay/PanelElement.cs` (166 lines, baseline d92d6aae, WP-17): the WPF `Border` + `StackPanel`
//! of the overlay. Rounded background, padding, children stacked vertically or horizontally. Pure: no `unsafe`, no `windows`
//! types (AGENTS.md). Contract: `contracts/W8-15d.md`. Stub: every body is `todo!()`.

use crate::input::primitives::{PointD, SizeD};
use crate::overlay::element::{
    ElementBase, ElementNode, OverlayElement, OverlayLayoutContext, OverlayThickness, net_max,
};
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
        Self {
            base: ElementBase::new(id, true),
            children: Vec::new(),
            background: crate::overlay::palette::overlay(),
            corner_radius: 0.0,
            padding: OverlayThickness::ZERO,
            orientation: OverlayOrientation::Vertical,
        }
    }

    /// C# `Children` (`IReadOnlyList<OverlayElement>`), insertion order.
    pub fn children(&self) -> &[Box<dyn ElementNode>] {
        &self.children
    }

    /// C# `Add(child)`: appends and returns `self` for chaining. Rust has no null, so the C# `ArgumentNullException` is a type fact.
    pub fn add(&mut self, child: Box<dyn ElementNode>) -> &mut Self {
        self.children.push(child);
        self
    }

    /// C# `Remove(child)`: removes the FIRST child whose `id()` equals `child_id` (ordinal) and returns true; false when none.
    /// C# removes by reference; Rust removes by id (open question 2 in contract.md).
    pub fn remove(&mut self, child_id: &str) -> bool {
        if let Some(index) = self.children.iter().position(|child| child.id() == child_id) {
            self.children.remove(index);
            true
        } else {
            false
        }
    }

    /// Typed read of the child at `index` (insertion order). `None` when the index is out of range or the child is not a `T`.
    /// `CapturePairBadge` and `FolderInfoPanel` use it for their text child (index 0), replacing the C# `Text` property.
    pub fn child<T: ElementNode + 'static>(&self, index: usize) -> Option<&T> {
        self.children.get(index)?.as_any().downcast_ref::<T>()
    }

    /// Typed mutable access to the child at `index`; same rules as [`PanelElement::child`].
    pub fn child_mut<T: ElementNode + 'static>(&mut self, index: usize) -> Option<&mut T> {
        self.children.get_mut(index)?.as_any_mut().downcast_mut::<T>()
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
        let max0 = |v: f64| {
            if v > 0.0 {
                v
            } else if v.is_nan() {
                f64::NAN
            } else {
                0.0
            }
        };
        let inner_w = max0(available.width - self.padding.horizontal());
        let inner_h = max0(available.height - self.padding.vertical());
        let (mut width, mut height) = (0.0, 0.0);
        match self.orientation {
            OverlayOrientation::Vertical => {
                for child in &mut self.children {
                    let size = child.measure(
                        SizeD {
                            width: inner_w,
                            height: max0(inner_h - height),
                        },
                        ctx,
                    );
                    width = net_max(width, size.width);
                    height += size.height;
                }
            }
            OverlayOrientation::Horizontal => {
                for child in &mut self.children {
                    let size = child.measure(
                        SizeD {
                            width: max0(inner_w - width),
                            height: inner_h,
                        },
                        ctx,
                    );
                    width += size.width;
                    height = net_max(height, size.height);
                }
            }
        }
        SizeD {
            width: width + self.padding.horizontal(),
            height: height + self.padding.vertical(),
        }
    }

    /// C# `ArrangeChildren`: starts at `Bounds` plus the left/top padding; hidden children get a zero-size slot at the cursor
    /// and do not move it; visible ones take their `desired_size` along the stacking axis and the full inner size across it.
    fn arrange_children(&mut self, ctx: &OverlayLayoutContext<'_>) {
        let bounds = self.base.bounds();
        let max0 = |v: f64| {
            if v > 0.0 {
                v
            } else if v.is_nan() {
                f64::NAN
            } else {
                0.0
            }
        };
        let (mut x, mut y) = (bounds.x + self.padding.left, bounds.y + self.padding.top);
        let inner_w = max0(bounds.width - self.padding.horizontal());
        let inner_h = max0(bounds.height - self.padding.vertical());
        for child in &mut self.children {
            if !child.is_visible() {
                child.arrange_in(
                    crate::input::primitives::RectD {
                        x,
                        y,
                        width: 0.0,
                        height: 0.0,
                    },
                    ctx,
                );
                continue;
            }
            let desired = child.base().desired_size();
            match self.orientation {
                OverlayOrientation::Vertical => {
                    child.arrange_in(
                        crate::input::primitives::RectD {
                            x,
                            y,
                            width: inner_w,
                            height: desired.height,
                        },
                        ctx,
                    );
                    y += desired.height;
                }
                OverlayOrientation::Horizontal => {
                    child.arrange_in(
                        crate::input::primitives::RectD {
                            x,
                            y,
                            width: desired.width,
                            height: inner_h,
                        },
                        ctx,
                    );
                    x += desired.width;
                }
            }
        }
    }

    /// C# `RenderCore`: fills the bounds first when `background.a > 0` and both bounds dimensions are `> 0` (rounded when
    /// `corner_radius > 0`), then renders every child in order (hidden children skip themselves in the base `render`).
    fn render_core(&self, dc: &mut dyn DrawContext) {
        let bounds = self.base.bounds();
        if self.background.a > 0.0 && bounds.width > 0.0 && bounds.height > 0.0 {
            if self.corner_radius > 0.0 {
                dc.fill_rounded_rectangle(bounds, self.corner_radius, self.background);
            } else {
                dc.fill_rectangle(bounds, self.background);
            }
        }
        for child in &self.children {
            child.render(dc);
        }
    }

    /// C# `FindAt`: `None` unless this element hits; then the LAST child (topmost) that hits wins, recursively; otherwise `self`.
    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if !self.hit_test(point) {
            return None;
        }
        for child in self.children.iter().rev() {
            if let Some(hit) = child.find_at(point) {
                return Some(hit);
            }
        }
        Some(self)
    }

    /// C# `Find(id)`: `self` when its id matches (ordinal), else the first child whose subtree finds it, in order.
    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.id() == id {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find(id) {
                return Some(found);
            }
        }
        None
    }

    /// C# `ReleaseResources`: forwards to every child (a panel holds no native resource of its own).
    fn release_resources(&mut self) {
        for child in &mut self.children {
            child.release_resources();
        }
    }
}
