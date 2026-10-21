//! Port of `PhotoReview.Shell.Win32/Overlay/PanelElement.cs` (166 lines, baseline d92d6aae, WP-17): the WPF `Border` + `StackPanel`
//! of the overlay. Rounded background, padding, children stacked vertically or horizontally. Pure: no `unsafe`, no `windows`
//! types (AGENTS.md). Contract: `contracts/W8-15d.md`.
//!
//! The stack rules live in free helpers over a [`StackStyle`] plus a slice of children (`measure_stack`, `arrange_stack`,
//! `render_stack`, `find_at_children`, `find_in_children`). [`PanelElement`] applies them to its own `Vec` of boxed children;
//! the composites of W8-15e (status, toolbar, zoom indicator) apply them to their typed fields (decision D15e-1).

use crate::input::primitives::{PointD, RectD, SizeD};
use crate::overlay::button::ButtonElement;
use crate::overlay::element::{
    ElementBase, ElementNode, OverlayElement, OverlayLayoutContext, OverlayThickness, net_max,
};
use crate::overlay::palette;
use crate::overlay::text::TextElement;
use crate::services::render::surface::DrawContext;
use crate::viewport::render_geometry::ColorF;

/// `OverlayOrientation` (C# `Vertical = 0`, `Horizontal = 1`), declared in `PanelElement.cs`. Default `Vertical`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayOrientation {
    #[default]
    Vertical = 0,
    Horizontal = 1,
}

/// The box style of a panel (C# `Background`, `CornerRadius`, `Padding`, `Orientation`), copied out so a composite can borrow its
/// children mutably while the helpers read the style.
#[derive(Debug, Clone, Copy)]
pub struct StackStyle {
    pub background: ColorF,
    pub corner_radius: f64,
    pub padding: OverlayThickness,
    pub orientation: OverlayOrientation,
}

/// C# `PanelElement.MeasureCore` over `children` (NaN-propagating max, trap T2): inner size is the available size minus the
/// padding (floor 0); vertical takes the widest child and sums heights, horizontal sums widths and takes the tallest child.
/// Every child is measured, hidden or not (a hidden child answers zero by itself).
pub fn measure_stack(
    style: StackStyle,
    available: SizeD,
    children: &mut [&mut dyn ElementNode],
    ctx: &OverlayLayoutContext<'_>,
) -> SizeD {
    let padding = style.padding;
    let inner_w = net_max(0.0, available.width - padding.horizontal());
    let inner_h = net_max(0.0, available.height - padding.vertical());
    let mut width = 0.0;
    let mut height = 0.0;
    let vertical = style.orientation == OverlayOrientation::Vertical;
    for child in children.iter_mut() {
        let offered = if vertical {
            SizeD {
                width: inner_w,
                height: net_max(0.0, inner_h - height),
            }
        } else {
            SizeD {
                width: net_max(0.0, inner_w - width),
                height: inner_h,
            }
        };
        let size = child.measure(offered, ctx);
        if vertical {
            width = net_max(width, size.width);
            height += size.height;
        } else {
            width += size.width;
            height = net_max(height, size.height);
        }
    }
    SizeD {
        width: width + padding.horizontal(),
        height: height + padding.vertical(),
    }
}

/// C# `PanelElement.ArrangeChildren` over `children`, for a panel whose own `bounds` are already set. A hidden child gets a zero
/// slot at the cursor and does not move it; a visible one takes its measured `desired_size` along the stacking axis (never its
/// previous `bounds`, trap T10) and the full inner size across it.
pub fn arrange_stack(
    style: StackStyle,
    bounds: RectD,
    children: &mut [&mut dyn ElementNode],
    ctx: &OverlayLayoutContext<'_>,
) {
    let padding = style.padding;
    let mut x = bounds.x + padding.left;
    let mut y = bounds.y + padding.top;
    let inner_w = net_max(0.0, bounds.width - padding.horizontal());
    let inner_h = net_max(0.0, bounds.height - padding.vertical());
    let vertical = style.orientation == OverlayOrientation::Vertical;
    for child in children.iter_mut() {
        if !child.base().is_visible {
            child.arrange_in(
                RectD {
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
        if vertical {
            child.arrange_in(
                RectD {
                    x,
                    y,
                    width: inner_w,
                    height: desired.height,
                },
                ctx,
            );
            y += desired.height;
        } else {
            child.arrange_in(
                RectD {
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

/// C# `PanelElement.RenderCore` over `children`: the background first (only when its alpha is `> 0` and the box has area; rounded
/// when `corner_radius > 0`), then every child in order.
pub fn render_stack(style: StackStyle, bounds: RectD, children: &[&dyn ElementNode], dc: &mut dyn DrawContext) {
    if style.background.a > 0.0 && bounds.width > 0.0 && bounds.height > 0.0 {
        if style.corner_radius > 0.0 {
            dc.fill_rounded_rectangle(bounds, style.corner_radius, style.background);
        } else {
            dc.fill_rectangle(bounds, style.background);
        }
    }
    for child in children {
        child.render(dc);
    }
}

/// The LAST child (topmost, trap T1) whose `find_at` hits, searched from the end.
pub fn find_at_children<'a>(children: &[&'a dyn ElementNode], point: PointD) -> Option<&'a dyn ElementNode> {
    children.iter().rev().find_map(|child| child.find_at(point))
}

/// The first child, in order, whose subtree finds `id`.
pub fn find_in_children<'a>(children: &[&'a dyn ElementNode], id: &str) -> Option<&'a dyn ElementNode> {
    children.iter().find_map(|child| child.find(id))
}

/// `new TextElement(id)` for a compile-time id of the overlay. The only failure of the constructor is an empty id, which no caller passes.
pub(crate) fn text_element(id: &str) -> TextElement {
    TextElement::new(id).expect("overlay element ids are non-empty constants")
}

/// `new ButtonElement(id)` for a compile-time id of the overlay (see [`text_element`]).
pub(crate) fn button_element(id: &str) -> ButtonElement {
    ButtonElement::new(id).expect("overlay element ids are non-empty constants")
}

/// `DarkPalette.Get(key)` for a `Dark.*` key that is a compile-time constant of the overlay. The table has every key the overlay
/// uses; an unknown key (a programming error) falls back to the bright text colour instead of panicking.
pub(crate) fn dark_color(key: &str) -> ColorF {
    palette::get(key).unwrap_or_else(palette::text_bright)
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
            background: palette::overlay(),
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
        match self.children.iter().position(|child| child.base().id() == child_id) {
            Some(index) => {
                self.children.remove(index);
                true
            }
            None => false,
        }
    }

    /// The box style as a copy, for the stack helpers.
    pub fn style(&self) -> StackStyle {
        StackStyle {
            background: self.background,
            corner_radius: self.corner_radius,
            padding: self.padding,
            orientation: self.orientation,
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
        let style = self.style();
        let mut children: Vec<&mut dyn ElementNode> = Vec::with_capacity(self.children.len());
        for child in &mut self.children {
            children.push(child.as_mut());
        }
        measure_stack(style, available, &mut children, ctx)
    }

    /// C# `ArrangeChildren`: starts at `Bounds` plus the left/top padding; hidden children get a zero-size slot at the cursor
    /// and do not move it; visible ones take their `desired_size` along the stacking axis and the full inner size across it.
    fn arrange_children(&mut self, ctx: &OverlayLayoutContext<'_>) {
        let style = self.style();
        let bounds = self.base.bounds();
        let mut children: Vec<&mut dyn ElementNode> = Vec::with_capacity(self.children.len());
        for child in &mut self.children {
            children.push(child.as_mut());
        }
        arrange_stack(style, bounds, &mut children, ctx);
    }

    /// C# `RenderCore`: fills the bounds first when `background.a > 0` and both bounds dimensions are `> 0` (rounded when
    /// `corner_radius > 0`), then renders every child in order (hidden children skip themselves in the base `render`).
    fn render_core(&self, dc: &mut dyn DrawContext) {
        let children: Vec<&dyn ElementNode> = self.children.iter().map(|child| &**child).collect();
        render_stack(self.style(), self.base.bounds(), &children, dc);
    }

    /// C# `FindAt`: `None` unless this element hits; then the LAST child (topmost) that hits wins, recursively; otherwise `self`.
    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if !self.hit_test(point) {
            return None;
        }
        let children: Vec<&dyn ElementNode> = self.children.iter().map(|child| &**child).collect();
        find_at_children(&children, point).or(Some(self as &dyn ElementNode))
    }

    /// C# `Find(id)`: `self` when its id matches (ordinal), else the first child whose subtree finds it, in order.
    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.base.id() == id {
            return Some(self);
        }
        let children: Vec<&dyn ElementNode> = self.children.iter().map(|child| &**child).collect();
        find_in_children(&children, id)
    }

    /// C# `ReleaseResources`: forwards to every child (a panel holds no native resource of its own).
    fn release_resources(&mut self) {
        for child in &mut self.children {
            child.release_resources();
        }
    }
}
