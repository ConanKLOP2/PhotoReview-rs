//! Red tests (W8-15a, test porter) for `pr_app::overlay::layer::LayerElement`: a one-cell grid whose children overlap.
//!
//! Port of `PhotoReview.Shell.Tests/Overlay/OverlayPartsTests.cs` (baseline `d92d6aae`, the C# repo at
//! `C:/MyProjects/PhotoReview`): the three `Layer_*` tests in leaf form (`FixedBox` instead of `PanelElement`/`TextElement`,
//! same margins, alignments and sizes, so the expected rectangles are the C# ones). The `PanelElement` originals are W8-15d.
//! Plus F tests written from contract semantics 8 and traps T6, T7 (no C# twin of the find and measure rules, marked "New test").
//!
//! Every test calls a stub that still ends in `todo!()` (directly or through the template), so the tests fail until the
//! porter fills the bodies of `overlay::layer`.

use std::cell::Cell;
use std::rc::Rc;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::contracts::{OpacityTarget, OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{
    ElementBase, ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use pr_app::overlay::layer::LayerElement;
use pr_app::services::render::error::RenderError;
use pr_app::services::render::surface::DrawContext;
use pr_app::services::render::text::{TextLayout, TextRenderer, TextStyle, TextTrimming};
use pr_app::services::render::{ColorF, GpuImage, ImageInterpolation};
use pr_imaging::pixels::PixelBuffer;

/// Test leaf: a box with a fixed content size. Counts its `release_resources` calls through a shared cell, so a test can
/// read the count after the leaf moved into a tree.
#[derive(Debug)]
struct FixedBox {
    base: ElementBase,
    content: SizeD,
    releases: Rc<Cell<u32>>,
}

impl FixedBox {
    fn new(id: &str, width: f64, height: f64) -> Self {
        Self {
            base: ElementBase::new(id, true),
            content: SizeD { width, height },
            releases: Rc::new(Cell::new(0)),
        }
    }
}

impl ElementNode for FixedBox {
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

    fn measure_core(&mut self, _available: SizeD, _context: &OverlayLayoutContext<'_>) -> SizeD {
        self.content
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        dc.fill_rectangle(
            self.base.bounds(),
            ColorF {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
        );
    }

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if OverlayElement::hit_test(self, point) {
            Some(self)
        } else {
            None
        }
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.base.id() == id { Some(self) } else { None }
    }

    fn release_resources(&mut self) {
        self.releases.set(self.releases.get() + 1);
    }
}

/// Recording draw context: the opacity stack, and each fill with the opacity in effect (the product of the stack).
#[derive(Default)]
struct RecordingDraw {
    stack: Vec<f32>,
    fills: Vec<f32>,
}

impl DrawContext for RecordingDraw {
    fn clear(&mut self, _color: ColorF) {}

    fn draw_image(
        &mut self,
        _image: &dyn GpuImage,
        _destination: RectD,
        _source_pixels: Option<RectD>,
        _interpolation: ImageInterpolation,
        _opacity: f32,
    ) {
    }

    fn fill_rectangle(&mut self, _rect: RectD, _color: ColorF) {
        self.fills.push(self.stack.iter().product());
    }

    fn fill_rounded_rectangle(&mut self, _rect: RectD, _radius: f64, _color: ColorF) {
        self.fills.push(self.stack.iter().product());
    }

    fn draw_rectangle(&mut self, _rect: RectD, _color: ColorF, _stroke_width: f64) {}

    fn draw_text(&mut self, _layout: &dyn TextLayout, _origin: PointD, _color: ColorF) -> Result<(), RenderError> {
        Ok(())
    }

    fn push_clip(&mut self, _rect: RectD) {}

    fn pop_clip(&mut self) -> Result<(), RenderError> {
        Ok(())
    }

    fn push_opacity(&mut self, opacity: f32) {
        self.stack.push(opacity);
    }

    fn pop_opacity(&mut self) -> Result<(), RenderError> {
        match self.stack.pop() {
            Some(_) => Ok(()),
            None => Err(RenderError::InvalidOperation {
                message: "PopOpacity without PushOpacity.",
            }),
        }
    }

    fn try_read_pixels(&mut self) -> Result<Option<PixelBuffer>, RenderError> {
        Ok(None)
    }
}

/// Text renderer that these tests never ask for a layout.
struct NullText;

struct NullLayout;

impl TextLayout for NullLayout {
    fn size(&self) -> SizeD {
        SizeD {
            width: 0.0,
            height: 0.0,
        }
    }

    fn text(&self) -> &str {
        ""
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl TextRenderer for NullText {
    fn create_layout(
        &self,
        _text: &str,
        _style: &TextStyle,
        _max_width: f64,
        _trimming: TextTrimming,
    ) -> Result<Box<dyn TextLayout>, RenderError> {
        Ok(Box::new(NullLayout))
    }
}

fn client(width: f64, height: f64) -> SizeD {
    SizeD { width, height }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

fn point(x: f64, y: f64) -> PointD {
    PointD { x, y }
}

// C#: OverlayPartsTests.Layer_ChildrenOverlapInTheWholeCell_EachAlignedByItself (leaf form, same margins and alignments)
#[test]
fn layer_children_overlap_in_the_whole_cell_each_aligned_by_itself() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(400.0, 300.0), 1.0, &text);
    let mut top_left = FixedBox::new("A", 100.0, 40.0);
    top_left.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    top_left.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    top_left.base_mut().margin = OverlayThickness::uniform(8.0);
    let mut bottom_right = FixedBox::new("B", 60.0, 20.0);
    bottom_right.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Right;
    bottom_right.base_mut().vertical_alignment = OverlayVerticalAlignment::Bottom;
    bottom_right.base_mut().margin = OverlayThickness::uniform(8.0);
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(top_left)).add(Box::new(bottom_right));

    layer.arrange(&ctx);

    assert_eq!(layer.base().bounds(), rect(0.0, 0.0, 400.0, 300.0));
    let a = layer.children()[0].base().bounds();
    let b = layer.children()[1].base().bounds();
    assert_eq!(a.x, 8.0);
    assert_eq!(a.y, 8.0);
    assert_eq!(b.x + b.width, 400.0 - 8.0);
    assert_eq!(b.y + b.height, 300.0 - 8.0);
    assert!(layer.find("A").is_some());
    assert_eq!(layer.find("B").map(|n| n.base().id()), Some("B"));
}

// C#: OverlayPartsTests.Layer_EmptySpaceIsClickThrough_ChildrenHit_HiddenLayerIsNot (leaf form)
#[test]
fn layer_empty_space_is_click_through_children_hit_hidden_layer_is_not() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(400.0, 300.0), 1.0, &text);
    let mut panel = FixedBox::new("A", 40.0, 20.0);
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(panel));
    layer.arrange(&ctx);

    assert!(layer.find_at(point(300.0, 200.0)).is_none());
    let hit = layer.find_at(point(2.0, 2.0)).expect("the child takes the hit");
    assert_eq!(hit.base().id(), "A");

    layer.base_mut().is_hit_test_visible = false;
    assert!(layer.find_at(point(2.0, 2.0)).is_none());
    layer.base_mut().is_hit_test_visible = true;

    OpacityTarget::set_opacity(&mut layer, 0.0);
    assert!(layer.find_at(point(2.0, 2.0)).is_none());
    OpacityTarget::set_opacity(&mut layer, 1.0);

    layer.base_mut().is_visible = false;
    assert!(layer.find_at(point(2.0, 2.0)).is_none());
}

// C#: OverlayPartsTests.Layer_RenderAppliesItsOpacityToChildren_AndReleasesLayouts (leaf form: the leaf records the opacity it
// was drawn with, and `release_resources` is counted by the leaf)
#[test]
fn layer_render_applies_its_opacity_to_children_and_releases_layouts() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(400.0, 300.0), 1.0, &text);
    let mut child = FixedBox::new("A", 40.0, 20.0);
    child.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    child.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    let releases = Rc::clone(&child.releases);
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(child));
    OpacityTarget::set_opacity(&mut layer, 0.5);
    layer.arrange(&ctx);
    let mut dc = RecordingDraw::default();

    layer.render(&mut dc);

    // The child's fill sees the layer's 0.5 (the child itself is at 1.0).
    assert_eq!(dc.fills, vec![0.5]);
    assert!(dc.stack.is_empty());

    layer.release_resources();
    assert_eq!(releases.get(), 1);
}

// New test: trap T6. The layer measure is the max over ALL children; a collapsed child counts with size 0 (its 90 height
// does not lower or raise the max). Expected (max width 40, max height 50).
#[test]
fn layer_measure_is_max_over_children_including_collapsed() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut hidden = FixedBox::new("B", 10.0, 90.0);
    hidden.base_mut().is_visible = false;
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(FixedBox::new("A", 40.0, 20.0)));
    layer.add(Box::new(hidden));
    layer.add(Box::new(FixedBox::new("C", 30.0, 50.0)));

    let desired = layer.measure(client(800.0, 600.0), &ctx);

    assert_eq!(desired, client(40.0, 50.0));
}

// New test: trap T6. An empty layer measures to zero.
#[test]
fn layer_measure_of_empty_layer_is_zero() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut layer = LayerElement::new("L");
    assert_eq!(layer.measure(client(800.0, 600.0), &ctx), client(0.0, 0.0));
}

// New test: trap T7. An empty cell inside the layer's bounds is click-through: `find_at` never returns the layer itself.
#[test]
fn layer_find_never_returns_the_layer_itself_for_an_empty_cell() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(400.0, 300.0), 1.0, &text);
    let mut child = FixedBox::new("A", 40.0, 20.0);
    child.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    child.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(child));
    layer.arrange(&ctx);

    // (300, 200) is inside the layer's bounds (0, 0, 400, 300) but outside the child.
    assert!(layer.base().bounds().contains(point(300.0, 200.0)));
    assert!(layer.find_at(point(300.0, 200.0)).is_none());
}

// New test: children are hit-tested topmost first (reverse insertion order): the later child wins an overlap.
#[test]
fn layer_find_returns_the_topmost_of_overlapping_children() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(400.0, 300.0), 1.0, &text);
    let mut under = FixedBox::new("under", 40.0, 20.0);
    under.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    under.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    let mut over = FixedBox::new("over", 40.0, 20.0);
    over.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    over.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(under)).add(Box::new(over));
    layer.arrange(&ctx);

    let hit = layer.find_at(point(5.0, 5.0)).expect("both children cover the point");
    assert_eq!(hit.base().id(), "over");
}

// New test: `find(id)` is depth-first: self first, then the children in insertion order (nested layers are searched in place).
#[test]
fn layer_find_by_id_is_depth_first() {
    let mut inner = LayerElement::new("M");
    inner.add(Box::new(FixedBox::new("Z", 10.0, 10.0)));
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(inner));
    layer.add(Box::new(FixedBox::new("F", 10.0, 10.0)));

    assert_eq!(layer.find("L").map(|n| n.base().id()), Some("L"));
    assert_eq!(layer.find("Z").map(|n| n.base().id()), Some("Z"));
    assert_eq!(layer.find("F").map(|n| n.base().id()), Some("F"));
    assert!(layer.find("missing").is_none());
}

// New test: semantic 8. `arrange_children` gives every child the layer's own bounds; a Stretch child fills the cell.
#[test]
fn layer_arrange_gives_every_child_the_layer_bounds() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(400.0, 300.0), 1.0, &text);
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(FixedBox::new("a", 40.0, 20.0)));
    layer.add(Box::new(FixedBox::new("b", 10.0, 10.0)));

    layer.arrange(&ctx);

    assert_eq!(layer.children()[0].base().bounds(), rect(0.0, 0.0, 400.0, 300.0));
    assert_eq!(layer.children()[1].base().bounds(), rect(0.0, 0.0, 400.0, 300.0));
}

// New test: C# `ArgumentException` for an empty id. `expected = "empty"` keeps the stub's `todo!()` panic from passing.
#[test]
#[should_panic(expected = "empty")]
fn layer_new_empty_id_panics() {
    let _ = LayerElement::new("");
}

// New test: children keep insertion order (C# `Children` is the add order).
#[test]
fn layer_add_keeps_insertion_order() {
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(FixedBox::new("first", 1.0, 1.0)));
    layer.add(Box::new(FixedBox::new("second", 1.0, 1.0)));
    layer.add(Box::new(FixedBox::new("third", 1.0, 1.0)));
    let ids: Vec<&str> = layer.children().iter().map(|c| c.base().id()).collect();
    assert_eq!(ids, vec!["first", "second", "third"]);
}

// New test: semantic 8 and trap T10: `release_resources` reaches every child, including the ones inside a nested layer.
#[test]
fn layer_release_resources_reaches_every_child() {
    let direct = FixedBox::new("a", 1.0, 1.0);
    let direct_releases = Rc::clone(&direct.releases);
    let nested_leaf = FixedBox::new("b", 1.0, 1.0);
    let nested_releases = Rc::clone(&nested_leaf.releases);
    let mut nested = LayerElement::new("M");
    nested.add(Box::new(nested_leaf));
    let mut layer = LayerElement::new("L");
    layer.add(Box::new(direct));
    layer.add(Box::new(nested));

    layer.release_resources();

    assert_eq!(direct_releases.get(), 1);
    assert_eq!(nested_releases.get(), 1);
}
