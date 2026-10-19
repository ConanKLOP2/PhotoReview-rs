//! Red tests (W8-15a, test porter) for `pr_app::overlay::element` and the host surface in `pr_app::overlay::contracts`:
//! `ElementBase`, the `ElementNode` template (`measure`, `arrange_in`), the blanket `OpacityTarget` / `OverlayElement`
//! impls, `OverlayThickness`, the alignments, and the `OverlayLayoutContext` / `Easing` value types.
//!
//! Port of `PhotoReview.Shell.Tests/Overlay/OverlayLayoutTests.cs` (two collapse tests, leaf form; baseline `d92d6aae`,
//! the C# repo at `C:/MyProjects/PhotoReview`) plus F tests written from the doc comments of `OverlayElement.cs` (no C# twin,
//! marked "New test"). Contract: `contracts/W8-15a.md`, traps T1 to T5.
//!
//! Every test calls a stub that still ends in `todo!()` (directly or through the template), so the tests fail until the
//! porter fills the bodies of `overlay::element` and `overlay::contracts`. Two value-only tests (`layout_context_debug...`,
//! `easing_discriminants...`) reach real code only at the edge and still need the stub's `Debug`/`todo!()` to pass.
//!
//! `FixedBox` is the test leaf: it implements `ElementNode` only. Its content size is fixed, or its width echoes the
//! `available` width when built with `echo`. `RecordingDraw` records opacity pushes and fills (with the opacity in effect).

use std::cell::Cell;
use std::rc::Rc;

use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::contracts::{Easing, OpacityTarget, OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{
    ElementBase, ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use pr_app::overlay::layer::LayerElement;
use pr_app::services::render::error::RenderError;
use pr_app::services::render::surface::DrawContext;
use pr_app::services::render::text::{TextLayout, TextRenderer, TextStyle, TextTrimming};
use pr_app::services::render::{ColorF, GpuImage, ImageInterpolation};
use pr_imaging::pixels::PixelBuffer;

/// Test leaf: a box with a fixed content size, or (`echo`) a width that follows `available`. Counts its `arrange_children`
/// calls and its `release_resources` calls through shared cells, so a test can read them after the element moved into a tree.
#[derive(Debug)]
struct FixedBox {
    base: ElementBase,
    content: SizeD,
    echo_width: bool,
    arranged: Rc<Cell<u32>>,
    releases: Rc<Cell<u32>>,
}

impl FixedBox {
    fn new(id: &str, width: f64, height: f64) -> Self {
        Self {
            base: ElementBase::new(id, true),
            content: SizeD { width, height },
            echo_width: false,
            arranged: Rc::new(Cell::new(0)),
            releases: Rc::new(Cell::new(0)),
        }
    }

    /// Width follows the `available` width given to `measure_core` (NaN included).
    fn echo(id: &str, height: f64) -> Self {
        let mut leaf = Self::new(id, 0.0, height);
        leaf.echo_width = true;
        leaf
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

    fn measure_core(&mut self, available: SizeD, _context: &OverlayLayoutContext<'_>) -> SizeD {
        let width = if self.echo_width {
            available.width
        } else {
            self.content.width
        };
        SizeD {
            width,
            height: self.content.height,
        }
    }

    fn arrange_children(&mut self, _context: &OverlayLayoutContext<'_>) {
        self.arranged.set(self.arranged.get() + 1);
    }

    fn render_core(&self, dc: &mut dyn DrawContext) {
        dc.fill_rectangle(
            self.base.bounds(),
            ColorF {
                r: 1.0,
                g: 0.0,
                b: 0.0,
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

/// Recording draw context: `events` lists "push", "pop" and "fill" in call order; `fills` keeps each fill's rectangle with
/// the product of the opacity stack at that moment (the opacity in effect).
#[derive(Default)]
struct RecordingDraw {
    stack: Vec<f32>,
    events: Vec<&'static str>,
    fills: Vec<(RectD, f32)>,
}

impl RecordingDraw {
    fn effective(&self) -> f32 {
        self.stack.iter().product()
    }
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

    fn fill_rectangle(&mut self, rect: RectD, _color: ColorF) {
        let effective = self.effective();
        self.fills.push((rect, effective));
        self.events.push("fill");
    }

    fn fill_rounded_rectangle(&mut self, rect: RectD, _radius: f64, _color: ColorF) {
        let effective = self.effective();
        self.fills.push((rect, effective));
        self.events.push("fill");
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
        self.events.push("push");
    }

    fn pop_opacity(&mut self) -> Result<(), RenderError> {
        self.events.push("pop");
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

/// Text renderer that is never asked for a layout by these tests (the leaf does not measure text).
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

// C#: OverlayLayoutTests.HiddenElement_IsCollapsed_NoSpaceNoDrawNoHit (leaf form: the TextElement original is W8-15c/d)
#[test]
fn hidden_element_is_collapsed_no_space_no_draw_no_hit() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    // The C# panel holds a visible child and a hidden one. The layer is the leaf-form parent (the Panel is W8-15d).
    let mut hidden = FixedBox::new("hidden", 80.0, 10.0);
    hidden.base_mut().is_visible = false;
    let mut panel = LayerElement::new("P");
    panel.add(Box::new(FixedBox::new("a", 40.0, 20.0)));
    panel.add(Box::new(hidden));
    let mut dc = RecordingDraw::default();

    panel.arrange(&ctx);
    panel.render(&mut dc);

    // No space: the collapsed child adds nothing to the measured size (contract T5/T6).
    assert_eq!(panel.base().desired_size(), client(40.0, 20.0));
    // No draw: exactly one fill (the visible child), and the opacity pushes and pops are balanced.
    assert_eq!(dc.fills.len(), 1);
    let pushes = dc.events.iter().filter(|e| **e == "push").count();
    let pops = dc.events.iter().filter(|e| **e == "pop").count();
    assert_eq!(pushes, pops);
    // No hit and zero bounds for the collapsed child (contract T4: zero size at the slot origin).
    let hidden_child = &panel.children()[1];
    assert_eq!(hidden_child.base().bounds().width, 0.0);
    assert!(!OverlayElement::hit_test(hidden_child.as_ref(), point(0.0, 0.0)));
}

// C#: OverlayLayoutTests.HiddenPanel_DrawsNothing_AndBalancesNothing (leaf form)
#[test]
fn hidden_element_draws_nothing_and_balances_opacity() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::new("P", 40.0, 20.0);
    leaf.base_mut().is_visible = false;
    leaf.arrange(&ctx);
    let mut dc = RecordingDraw::default();

    leaf.render(&mut dc);

    assert!(dc.events.is_empty(), "a hidden element must not push, fill or pop");
    assert!(dc.stack.is_empty());
    assert_eq!(leaf.base().bounds().width, 0.0);
}

// New test: C# has no direct test of `OverlayThickness` equality; the contract's trap T1 requires C# record equality.
#[test]
fn thickness_equality_is_nan_aware() {
    let with_nan = OverlayThickness::new(f64::NAN, 0.0, 0.0, 0.0);
    assert_eq!(with_nan, OverlayThickness::new(f64::NAN, 0.0, 0.0, 0.0));
    assert_ne!(with_nan, OverlayThickness::new(1.0, 0.0, 0.0, 0.0));
    // `0.0` and `-0.0` are equal under `double.Equals`.
    assert_eq!(
        OverlayThickness::new(0.0, 0.0, 0.0, 0.0),
        OverlayThickness::new(-0.0, 0.0, 0.0, 0.0)
    );
}

// New test: the default value is `Zero` (C# `OverlayThickness.Zero`).
#[test]
fn thickness_zero_is_default() {
    assert_eq!(OverlayThickness::default(), OverlayThickness::ZERO);
    assert_eq!(OverlayThickness::ZERO, OverlayThickness::new(0.0, 0.0, 0.0, 0.0));
}

// New test: the constructors follow the C# shapes (uniform: all sides; symmetric(horizontal, vertical); new(left, top, right, bottom)).
#[test]
fn thickness_constructors_match_csharp_shapes() {
    assert_eq!(
        OverlayThickness::uniform(3.0),
        OverlayThickness::new(3.0, 3.0, 3.0, 3.0)
    );
    assert_eq!(
        OverlayThickness::symmetric(4.0, 2.0),
        OverlayThickness::new(4.0, 2.0, 4.0, 2.0)
    );
    let asymmetric = OverlayThickness::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(asymmetric.left, 1.0);
    assert_eq!(asymmetric.top, 2.0);
    assert_eq!(asymmetric.right, 3.0);
    assert_eq!(asymmetric.bottom, 4.0);
}

// New test: `Horizontal` = left + right, `Vertical` = top + bottom.
#[test]
fn thickness_horizontal_and_vertical_sums() {
    let t = OverlayThickness::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(t.horizontal(), 4.0);
    assert_eq!(t.vertical(), 6.0);
}

// New test: contract semantic 2 and trap T3: NaN becomes 0, values clamp to 0..=1.
#[test]
fn opacity_setter_maps_nan_to_zero_and_clamps() {
    let mut leaf = FixedBox::new("o", 10.0, 10.0);
    OpacityTarget::set_opacity(&mut leaf, f32::NAN);
    assert_eq!(OpacityTarget::opacity(&leaf), 0.0);
    OpacityTarget::set_opacity(&mut leaf, 5.0);
    assert_eq!(OpacityTarget::opacity(&leaf), 1.0);
    OpacityTarget::set_opacity(&mut leaf, -1.0);
    assert_eq!(OpacityTarget::opacity(&leaf), 0.0);
    OpacityTarget::set_opacity(&mut leaf, 0.25);
    assert_eq!(OpacityTarget::opacity(&leaf), 0.25);
}

// New test: trap T3, `Math.Clamp` order: `-0.0` is not below `0.0`, so the sign bit is kept.
#[test]
fn opacity_negative_zero_is_kept() {
    let mut leaf = FixedBox::new("o", 10.0, 10.0);
    OpacityTarget::set_opacity(&mut leaf, -0.0);
    assert_eq!(OpacityTarget::opacity(&leaf).to_bits(), (-0.0f32).to_bits());
}

// New test: trap T2. The echo leaf returns the `available` width, so a NaN width must reach `desired_size` (C# Math.Min/Max
// with NaN gives NaN); a Rust `f64::min` would silently give the finite `max_width`.
#[test]
fn measure_nan_width_propagates_into_desired_size() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::echo("n", 20.0);
    let desired = leaf.measure(client(f64::NAN, 600.0), &ctx);
    assert!(
        desired.width.is_nan(),
        "a NaN available width must give a NaN desired width"
    );
    assert!(leaf.base().desired_size().width.is_nan());
}

// New test: semantic 3. A stretch width is capped at `max_width` (the margin is added back after the cap).
#[test]
fn measure_stretch_width_is_capped_by_max_width() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::echo("m", 20.0);
    leaf.base_mut().max_width = 200.0;
    leaf.base_mut().margin = OverlayThickness::uniform(8.0);
    let desired = leaf.measure(client(500.0, 600.0), &ctx);
    // w0 = min(500 - 16, 200) = 200; content 200; result = min(200, 200) + 16.
    assert_eq!(desired, client(216.0, 20.0 + 16.0));
}

// New test: trap T5. A collapsed element stores and returns a zero desired size.
#[test]
fn measure_collapsed_stores_zero_desired_size() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::new("c", 40.0, 20.0);
    leaf.base_mut().is_visible = false;
    let desired = leaf.measure(client(800.0, 600.0), &ctx);
    assert_eq!(desired, client(0.0, 0.0));
    assert_eq!(leaf.base().desired_size(), client(0.0, 0.0));
}

// New test: trap T4. A collapsed `arrange_in` sets the bounds to (slot.x, slot.y, 0, 0), NOT to `RectD::default()`.
#[test]
fn arrange_in_collapsed_sets_zero_bounds_at_slot_origin() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::new("c", 40.0, 20.0);
    leaf.base_mut().is_visible = false;
    leaf.measure(client(800.0, 600.0), &ctx);
    leaf.arrange_in(rect(30.0, 40.0, 100.0, 50.0), &ctx);
    assert_eq!(leaf.base().bounds(), rect(30.0, 40.0, 0.0, 0.0));
}

// New test: trap T4. A collapsed element does not arrange its children; a visible one does (the contrast keeps the test honest).
#[test]
fn arrange_in_collapsed_does_not_arrange_children() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut hidden = FixedBox::new("h", 40.0, 20.0);
    hidden.base_mut().is_visible = false;
    let hidden_counter = Rc::clone(&hidden.arranged);
    hidden.measure(client(800.0, 600.0), &ctx);
    hidden.arrange_in(rect(0.0, 0.0, 800.0, 600.0), &ctx);
    assert_eq!(hidden_counter.get(), 0);

    let visible = FixedBox::new("v", 40.0, 20.0);
    let visible_counter = Rc::clone(&visible.arranged);
    let mut visible = visible;
    visible.measure(client(800.0, 600.0), &ctx);
    visible.arrange_in(rect(0.0, 0.0, 800.0, 600.0), &ctx);
    assert_eq!(visible_counter.get(), 1);
}

// New test: the alignment matrix (4 horizontal x 4 vertical), one slot (10, 20, 200, 100), content 41 x 21, zero margin.
// Horizontal: Left 10/41, Center 10 + (200 - 41) / 2 = 89.5, Right 169, Stretch 10/200. Vertical: Top 20/21,
// Center 20 + (100 - 21) / 2 = 59.5, Bottom 99, Stretch 20/100.
#[test]
fn arrange_in_alignment_matrix() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let slot = rect(10.0, 20.0, 200.0, 100.0);
    let horizontal = [
        (OverlayHorizontalAlignment::Left, 10.0, 41.0),
        (OverlayHorizontalAlignment::Center, 89.5, 41.0),
        (OverlayHorizontalAlignment::Right, 169.0, 41.0),
        (OverlayHorizontalAlignment::Stretch, 10.0, 200.0),
    ];
    let vertical = [
        (OverlayVerticalAlignment::Top, 20.0, 21.0),
        (OverlayVerticalAlignment::Center, 59.5, 21.0),
        (OverlayVerticalAlignment::Bottom, 99.0, 21.0),
        (OverlayVerticalAlignment::Stretch, 20.0, 100.0),
    ];
    for (h, expected_x, expected_width) in horizontal {
        for (v, expected_y, expected_height) in vertical {
            let mut leaf = FixedBox::new("leaf", 41.0, 21.0);
            leaf.base_mut().horizontal_alignment = h;
            leaf.base_mut().vertical_alignment = v;
            leaf.measure(ctx.client, &ctx);
            leaf.arrange_in(slot, &ctx);
            assert_eq!(
                leaf.base().bounds(),
                rect(expected_x, expected_y, expected_width, expected_height),
                "horizontal {h:?}, vertical {v:?}"
            );
        }
    }
}

// C#: OverlayLayoutTests.HitTest (contract semantic 7, step by step): each of the four conditions alone flips the hit.
#[test]
fn hit_test_requires_visible_hit_enabled_opacity_and_bounds() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::new("h", 40.0, 20.0);
    leaf.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    leaf.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    leaf.measure(client(800.0, 600.0), &ctx);
    leaf.arrange_in(rect(0.0, 0.0, 100.0, 100.0), &ctx);
    assert!(OverlayElement::hit_test(&leaf, point(5.0, 5.0)), "all conditions hold");

    leaf.base_mut().is_visible = false;
    assert!(!OverlayElement::hit_test(&leaf, point(5.0, 5.0)), "collapsed");
    leaf.base_mut().is_visible = true;

    leaf.base_mut().is_hit_test_visible = false;
    assert!(!OverlayElement::hit_test(&leaf, point(5.0, 5.0)), "hit test disabled");
    leaf.base_mut().is_hit_test_visible = true;

    OpacityTarget::set_opacity(&mut leaf, 0.0);
    assert!(!OverlayElement::hit_test(&leaf, point(5.0, 5.0)), "opacity 0");
    OpacityTarget::set_opacity(&mut leaf, 1.0);

    assert!(
        !OverlayElement::hit_test(&leaf, point(50.0, 50.0)),
        "outside the bounds"
    );
}

// C#: OverlayLayoutTests.ZeroOpacity_SkipsDrawing (the part of the template: `render` returns early, no push)
#[test]
fn render_skipped_when_hidden_or_zero_opacity() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::new("z", 40.0, 20.0);
    leaf.measure(client(800.0, 600.0), &ctx);
    leaf.arrange_in(rect(0.0, 0.0, 100.0, 100.0), &ctx);

    OpacityTarget::set_opacity(&mut leaf, 0.0);
    let mut dc = RecordingDraw::default();
    leaf.render(&mut dc);
    assert!(dc.events.is_empty(), "opacity 0 draws nothing and pushes nothing");
}

// New test: semantic 6. A visible element at opacity 0.5 pushes, draws with the opacity in effect, and pops (balanced).
#[test]
fn render_pushes_then_pops_opacity_balanced() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::new("r", 40.0, 20.0);
    leaf.measure(client(800.0, 600.0), &ctx);
    leaf.arrange_in(rect(0.0, 0.0, 100.0, 100.0), &ctx);
    OpacityTarget::set_opacity(&mut leaf, 0.5);
    let mut dc = RecordingDraw::default();

    leaf.render(&mut dc);

    assert_eq!(dc.events, vec!["push", "fill", "pop"]);
    assert_eq!(dc.fills.len(), 1);
    assert_eq!(dc.fills[0].1, 0.5);
    assert!(dc.stack.is_empty(), "the opacity stack returns to depth 0");
}

// New test: semantic 7 for a leaf: `find_at` is `Some(self)` inside the bounds and `None` outside.
#[test]
fn find_at_leaf_returns_self_when_hit() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut leaf = FixedBox::new("leaf", 40.0, 20.0);
    leaf.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    leaf.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    leaf.measure(client(800.0, 600.0), &ctx);
    leaf.arrange_in(rect(0.0, 0.0, 100.0, 100.0), &ctx);

    let hit = leaf.find_at(point(5.0, 5.0)).expect("inside the bounds");
    assert_eq!(hit.base().id(), "leaf");
    assert!(leaf.find_at(point(50.0, 50.0)).is_none());
}

// New test: `arrange` measures in the whole client and places the box at the origin (Stretch fills the client).
#[test]
fn arrange_applies_the_client_as_available_size() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(300.0, 200.0), 1.0, &text);
    let mut leaf = FixedBox::echo("full", 20.0);
    leaf.arrange(&ctx);
    // Echo width: the desired width is the client width; Stretch fills the client in both directions.
    assert_eq!(leaf.base().bounds(), rect(0.0, 0.0, 300.0, 200.0));
}

// New test: `Box<dyn ElementNode>` (a child of a layer) is usable through the host surface `OverlayElement`.
#[test]
fn overlay_element_trait_object_is_usable() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.0, &text);
    let mut boxed: Box<dyn OverlayElement> = Box::new(FixedBox::new("obj", 40.0, 20.0));
    boxed.arrange(&ctx);
    assert_eq!(boxed.id(), "obj");
    assert!(boxed.is_visible());
    assert_eq!(boxed.bounds().width, 800.0);

    let node: Box<dyn ElementNode> = Box::new(FixedBox::new("node", 40.0, 20.0));
    assert_eq!(OverlayElement::id(node.as_ref()), "node");
}

// New test: the layout context is a plain value. Its `Debug` prints the client and dpi and hides the renderer.
#[test]
fn layout_context_debug_hides_the_renderer() {
    let text = NullText;
    let ctx = OverlayLayoutContext::new(client(800.0, 600.0), 1.5, &text);
    let shown = format!("{ctx:?}");
    assert!(shown.contains("<text renderer>"), "{shown}");
    assert!(shown.contains("1.5"), "{shown}");
}

// New test: the `Easing` discriminants match C# (`Linear = 0`, `QuadraticOut = 1`).
#[test]
fn easing_discriminants_match_csharp() {
    assert_eq!(Easing::Linear as i32, 0);
    assert_eq!(Easing::QuadraticOut as i32, 1);
}
