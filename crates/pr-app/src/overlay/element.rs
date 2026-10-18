//! Port of `PhotoReview.Shell.Win32/Overlay/OverlayElement.cs` (WP-17, C-11): the overlay element base class.
//!
//! Baseline `d92d6aae`. Owned by W8-15a. C# `OverlayElement` is an abstract class that implements `IOverlayElement` and
//! `IOpacityTarget`. Rust splits it into three parts:
//! - [`ElementBase`]: the state every element has (id, visibility, opacity, margin, alignments, `MaxWidth`, hit-test flag,
//!   `Bounds`, `DesiredSize`). Plain struct, no trait.
//! - [`ElementNode`]: the template: `measure` and `arrange_in` (the C# non-virtual methods), plus the hooks each concrete
//!   element implements (`measure_core`, `render_core`, `find_at`, `find`, optional `arrange_children`, `release_resources`).
//! - Blanket impls: `OpacityTarget` and `OverlayElement` for every `ElementNode`. No element implements the host surface by hand.
//!
//! Layout is WPF's two steps (Measure, then Arrange) reduced to what Border, StackPanel, TextBlock and Button need. Every
//! number is DIP. Layout does NOT read `OverlayLayoutContext::dpi_scale`.
//!
//! Traps (contract section "Traps"): T1 equality of `OverlayThickness` is NaN-aware (`f64_same`); T2 `Math.Min`/`Math.Max`
//! propagate NaN, so do not use `f64::min`/`max` here; T3 `Opacity` setter maps NaN to 0; T4 a collapsed element's `Bounds`
//! is a zero-size box at the slot origin, not `RectD::default()`, and it does not arrange children.

#![allow(unused_variables, dead_code)] // stub only: removed by the porter when the bodies are filled (contracts/README.md D11)

use std::fmt::Debug;

use crate::input::primitives::{PointD, RectD, SizeD};
use crate::services::render::DrawContext;

pub use super::contracts::{OpacityTarget, OverlayElement, OverlayLayoutContext};

/// Construction failure of an element (lead decision D-W8B1-2: W8-15c takes `Result`, W8-15a proposed a panic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementError {
    /// C# `ArgumentException` for an empty element id.
    EmptyId,
}

impl std::fmt::Display for ElementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyId => f.write_str("overlay element id must not be empty"),
        }
    }
}

impl std::error::Error for ElementError {}

/// C# `OverlayThickness` (`readonly record struct`, margin and padding in DIP). Equality is C# record equality: each field
/// compares with `double.Equals` (NaN equals NaN, `0.0 == -0.0`), so `PartialEq` is manual.
#[derive(Debug, Clone, Copy, Default)]
pub struct OverlayThickness {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl OverlayThickness {
    /// C# `Zero` (the default value).
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    /// C# `OverlayThickness(Left, Top, Right, Bottom)`.
    pub fn new(left: f64, top: f64, right: f64, bottom: f64) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// C# `OverlayThickness(uniform)`: all four sides equal.
    pub fn uniform(value: f64) -> Self {
        Self::new(value, value, value, value)
    }

    /// C# `OverlayThickness(horizontal, vertical)`: left and right get `horizontal`, top and bottom get `vertical`.
    pub fn symmetric(horizontal: f64, vertical: f64) -> Self {
        Self::new(horizontal, vertical, horizontal, vertical)
    }

    /// C# `Horizontal` = `Left + Right`.
    pub fn horizontal(&self) -> f64 {
        todo!()
    }

    /// C# `Vertical` = `Top + Bottom`.
    pub fn vertical(&self) -> f64 {
        todo!()
    }
}

impl PartialEq for OverlayThickness {
    fn eq(&self, other: &Self) -> bool {
        todo!()
    }
}

/// C# `OverlayHorizontalAlignment`. Discriminants match C#. Default `Stretch` (the C# default of `OverlayElement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OverlayHorizontalAlignment {
    Left = 0,
    Center = 1,
    Right = 2,
    #[default]
    Stretch = 3,
}

/// C# `OverlayVerticalAlignment`. Discriminants match C#. Default `Stretch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OverlayVerticalAlignment {
    Top = 0,
    Center = 1,
    Bottom = 2,
    #[default]
    Stretch = 3,
}

/// The state every overlay element carries (C# fields and auto-properties of `OverlayElement`).
///
/// Public fields are the C# `{ get; set; }` properties with no invariant. `opacity` (clamped), `bounds` and `desired_size`
/// (written only by layout) are private.
#[derive(Debug)]
pub struct ElementBase {
    id: String,
    /// C# `IsVisible` (default `true`). `false` = collapsed.
    pub is_visible: bool,
    opacity: f32,
    /// C# `Margin` (outer spacing in DIP).
    pub margin: OverlayThickness,
    /// C# `HorizontalAlignment` (default `Stretch`).
    pub horizontal_alignment: OverlayHorizontalAlignment,
    /// C# `VerticalAlignment` (default `Stretch`).
    pub vertical_alignment: OverlayVerticalAlignment,
    /// C# `MaxWidth` (default `+infinity`), the width cap of the box excluding the margin.
    pub max_width: f64,
    /// C# `IsHitTestVisible`. Set by the element at construction: `true` by default, `false` for `TextElement`.
    pub is_hit_test_visible: bool,
    bounds: RectD,
    desired_size: SizeD,
}

impl ElementBase {
    /// C# constructor `OverlayElement(string id)`: `Id` must not be empty (`ArgumentException.ThrowIfNullOrEmpty`, so
    /// this panics on `""`; a programming error, not a user-data error). Defaults: visible, opacity `1.0`, zero margin,
    /// `Stretch` alignments, `MaxWidth = +infinity`, zero `Bounds` and `DesiredSize`.
    pub fn new(id: &str, is_hit_test_visible: bool) -> Self {
        todo!()
    }

    /// C# `Id`.
    pub fn id(&self) -> &str {
        todo!()
    }

    /// C# `Opacity` getter.
    pub fn opacity(&self) -> f32 {
        todo!()
    }

    /// C# `Opacity` setter: NaN becomes `0.0`, otherwise clamp to `0.0..=1.0` (`Math.Clamp` order, see trap T3).
    pub fn set_opacity(&mut self, value: f32) {
        todo!()
    }

    /// C# `Bounds`: the box after the last arrange, margin excluded.
    pub fn bounds(&self) -> RectD {
        todo!()
    }

    /// C# `DesiredSize`: the size from the last `measure` (margin included); zero after a measure while collapsed.
    pub fn desired_size(&self) -> SizeD {
        todo!()
    }

    /// Written by `ElementNode::arrange_in` only.
    pub(crate) fn set_bounds(&mut self, bounds: RectD) {
        todo!()
    }

    /// Written by `ElementNode::measure` only.
    pub(crate) fn set_desired_size(&mut self, size: SizeD) {
        todo!()
    }
}

/// The template of an overlay element: C# `OverlayElement`'s non-virtual methods and its abstract / virtual hooks.
/// Every concrete element type implements this trait (and nothing else); the host-facing `OverlayElement` comes from the
/// blanket impls below.
///
/// `Debug` is a supertrait so that `Box<dyn ElementNode>` (children of `LayerElement`) is `Debug`.
pub trait ElementNode: Debug {
    /// Downcast support for typed child access (lead decision D-W8B1-2: required by W8-15d `PanelElement::child`).
    fn as_any(&self) -> &dyn std::any::Any;

    /// Mutable twin of [`ElementNode::as_any`].
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// The shared state. Every element returns its own `ElementBase`.
    fn base(&self) -> &ElementBase;

    /// Mutable access to the shared state (used by the template methods and the animator's opacity writes).
    fn base_mut(&mut self) -> &mut ElementBase;

    /// C# `MeasureCore(SizeD available, context)` (abstract). `available` is already reduced by the margin and capped at
    /// `MaxWidth` by `measure`. Returns the content size, margin excluded. `&mut self` because a text element caches its layout.
    fn measure_core(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD;

    /// C# `ArrangeChildren(context)` (virtual). Called by `arrange_in` after `Bounds` is set. Default: no children.
    fn arrange_children(&mut self, context: &OverlayLayoutContext<'_>) {}

    /// C# `RenderCore(IDrawContext)` (abstract). Called by the blanket `OverlayElement::render` between the opacity push
    /// and pop. Children are drawn here (in order), not by the template.
    fn render_core(&self, dc: &mut dyn DrawContext);

    /// C# `FindAt(PointD)` (virtual): the deepest element that takes the hit (its own `hit_test` decides for a leaf), or
    /// `None`. A leaf returns `Some(self)` when `self.hit_test(point)`, else `None`.
    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode>;

    /// C# `Find(string id)` (virtual): `self` when `Id` equals `id` (ordinal), else the first match in the subtree in
    /// order, or `None`.
    fn find(&self, id: &str) -> Option<&dyn ElementNode>;

    /// C# `ReleaseResources()` (virtual): drops cached layouts of this element and its descendants. Default: nothing.
    fn release_resources(&mut self) {}

    /// C# `Measure(SizeD available, context)` (public, non-virtual). Collapsed: stores and returns `(0, 0)`. Otherwise:
    /// `width = max(0, min(available.width - margin.horizontal, max_width))` and
    /// `height = max(0, available.height - margin.vertical)` (NaN propagates, trap T2), `content = measure_core(...)`,
    /// result `(min(content.width, max_width) + margin.horizontal, content.height + margin.vertical)`. Stores it as
    /// `DesiredSize` and returns it.
    fn measure(&mut self, available: SizeD, context: &OverlayLayoutContext<'_>) -> SizeD {
        todo!()
    }

    /// C# `ArrangeIn(RectD slot, context)` (public, non-virtual). `slot` includes the margin. Collapsed: `Bounds` becomes
    /// `(slot.x, slot.y, 0, 0)` and children are NOT arranged (trap T4). Otherwise the inner box is `slot` minus the
    /// margin, the size is `min(inner, max_width)` for `Stretch` horizontal (else `min(desired, inner)`), and the
    /// position follows the alignment (`Right`/`Bottom` anchor to the inner edge, `Center` halves the free space). Then
    /// `Bounds` is set and `arrange_children` runs.
    fn arrange_in(&mut self, slot: RectD, context: &OverlayLayoutContext<'_>) {
        todo!()
    }
}

/// C# `IOpacityTarget` for every element: forwards to `ElementBase`.
impl<T: ElementNode + ?Sized> OpacityTarget for T {
    fn opacity(&self) -> f32 {
        todo!()
    }

    fn set_opacity(&mut self, value: f32) {
        todo!()
    }
}

/// C# `IOverlayElement` for every element: the host surface. `arrange` is C# `Arrange(context)` (measure in the whole
/// client, then `arrange_in` on the client rectangle at the origin). `render` is C# `Render`: return when hidden or
/// opacity `<= 0`; else push the opacity, `render_core`, pop the opacity. `hit_test` is C# `HitTest`.
impl<T: ElementNode + ?Sized> OverlayElement for T {
    fn id(&self) -> &str {
        todo!()
    }

    fn is_visible(&self) -> bool {
        todo!()
    }

    fn bounds(&self) -> RectD {
        todo!()
    }

    fn arrange(&mut self, context: &OverlayLayoutContext<'_>) {
        todo!()
    }

    fn render(&self, dc: &mut dyn DrawContext) {
        todo!()
    }

    fn hit_test(&self, point: PointD) -> bool {
        todo!()
    }
}
