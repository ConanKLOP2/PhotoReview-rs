//! F tests (W8-15d) for `pr_app::overlay::panel::PanelElement` (C# `PanelElement`, baseline `d92d6aae`).
//!
//! Ports of the panel part of `OverlayLayoutTests`, `OverlayHostTests` (see each `// C#:` comment) plus `// new:` tests for the
//! contract traps T1 (last sibling wins), T2 (NaN propagation), T4 (typed child access), T5 (remove by id), T9, T10.
//! The fakes are in `overlay_fakes.rs` (width = characters x size / 2, height = size x 1.5).

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use std::cell::Cell;
use std::rc::Rc;

use overlay_fakes::{FakeTextRenderer, RecordingDrawContext};
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::button::ButtonElement;
use pr_app::overlay::contracts::{OpacityTarget, OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{
    ElementBase, ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use pr_app::overlay::palette;
use pr_app::overlay::panel::{OverlayOrientation, PanelElement};
use pr_app::overlay::text::TextElement;
use pr_app::services::render::text::{TextStyle, TextTrimming};
use pr_app::services::render::{ColorF, DrawContext};

fn style(size: f64) -> TextStyle {
    TextStyle {
        font_family: "Segoe UI".to_owned(),
        font_size_dip: size,
        bold: false,
    }
}

fn context(text: &FakeTextRenderer, width: f64, height: f64) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::new(SizeD { width, height }, 1.0, text)
}

fn text(id: &str, content: &str, size: f64) -> TextElement {
    let mut element = TextElement::new(id).expect("non-empty id");
    element.set_text(content);
    element.set_style(style(size));
    element
}

fn button(id: &str, content: &str, size: f64) -> ButtonElement {
    let mut element = ButtonElement::new(id).expect("non-empty id");
    element.set_content(content);
    element.set_style(style(size));
    element
}

/// The C# `Badge` helper of `OverlayLayoutTests`: margin 8, padding (6, 4), corner radius 4.
fn badge(id: &str, h: OverlayHorizontalAlignment, v: OverlayVerticalAlignment) -> PanelElement {
    let mut panel = PanelElement::new(id);
    panel.base_mut().horizontal_alignment = h;
    panel.base_mut().vertical_alignment = v;
    panel.base_mut().margin = OverlayThickness::uniform(8.0);
    panel.padding = OverlayThickness::symmetric(6.0, 4.0);
    panel.corner_radius = 4.0;
    panel
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

fn assert_rect(actual: RectD, expected: RectD) {
    assert_eq!(
        (actual.x, actual.y, actual.width, actual.height),
        (expected.x, expected.y, expected.width, expected.height)
    );
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

/// Test leaf with a fixed content size, or (`echo`) a content size equal to the offered size.
#[derive(Debug)]
struct Probe {
    base: ElementBase,
    content: SizeD,
    echo: bool,
    releases: Rc<Cell<u32>>,
}

impl Probe {
    fn fixed(id: &str, width: f64, height: f64) -> Self {
        Self {
            base: ElementBase::new(id, true),
            content: SizeD { width, height },
            echo: false,
            releases: Rc::new(Cell::new(0)),
        }
    }

    fn echo(id: &str) -> Self {
        let mut probe = Self::fixed(id, 0.0, 0.0);
        probe.echo = true;
        probe
    }
}

impl ElementNode for Probe {
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
        if self.echo { available } else { self.content }
    }

    fn render_core(&self, _dc: &mut dyn DrawContext) {}

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if self.hit_test(point) { Some(self) } else { None }
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.base.id() == id { Some(self) } else { None }
    }

    fn release_resources(&mut self) {
        self.releases.set(self.releases.get() + 1);
    }
}

#[test]
fn panel_sizes_to_text_plus_padding_and_anchors_to_corners() {
    // C#: OverlayLayoutTests.Panel_SizesToTextPlusPadding_AndAnchorsToCorners
    let fake = FakeTextRenderer::new();
    let mut bottom_left = badge("A", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Bottom);
    bottom_left.add(Box::new(text("a", "ABCDEFGHIJ", 10.0)));
    let mut top_right = badge("B", OverlayHorizontalAlignment::Right, OverlayVerticalAlignment::Top);
    top_right.add(Box::new(text("b", "ABCDEFGHIJ", 10.0)));
    let mut center = badge("C", OverlayHorizontalAlignment::Center, OverlayVerticalAlignment::Top);
    center.add(Box::new(text("c", "ABCDEFGHIJ", 10.0)));
    center.base_mut().margin = OverlayThickness::new(8.0, 44.0, 8.0, 8.0);

    for panel in [&mut bottom_left, &mut top_right, &mut center] {
        panel.arrange(&context(&fake, 800.0, 600.0));
    }

    assert_rect(bottom_left.bounds(), rect(8.0, 600.0 - 8.0 - 23.0, 62.0, 23.0));
    assert_rect(top_right.bounds(), rect(800.0 - 8.0 - 62.0, 8.0, 62.0, 23.0));
    assert_rect(center.bounds(), rect(369.0, 44.0, 62.0, 23.0));
}

#[test]
fn stretch_panel_fills_client_minus_margin() {
    // C#: OverlayLayoutTests.StretchPanel_FillsClientMinusMargin
    let fake = FakeTextRenderer::new();
    let mut compare = PanelElement::new("ComparePanel");
    compare.base_mut().margin = OverlayThickness::uniform(8.0);

    compare.arrange(&context(&fake, 1366.0, 728.0));

    assert_rect(compare.bounds(), rect(8.0, 8.0, 1350.0, 712.0));
}

#[test]
fn vertical_stack_width_is_widest_child_height_is_sum_with_margins() {
    // C#: OverlayLayoutTests.VerticalStack_WidthIsWidestChild_HeightIsSumWithMargins
    let fake = FakeTextRenderer::new();
    let mut panel = badge(
        "StatusPanel",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Bottom,
    );
    panel.add(Box::new(text("StatusText", "IMG_0042.JPG", 12.0)));
    let mut exif = text("ExifInfoText", "Canon EOS R5 85 mm f/1.8", 11.0);
    exif.base_mut().margin = OverlayThickness::new(0.0, 2.0, 0.0, 0.0);
    panel.add(Box::new(exif));

    panel.arrange(&context(&fake, 800.0, 600.0));

    let bounds = panel.bounds();
    assert_close(bounds.width, 132.0 + 12.0);
    assert_close(bounds.height, 18.0 + 2.0 + 16.5 + 8.0);
    let status = panel.child::<TextElement>(0).expect("status").bounds();
    let exif = panel.child::<TextElement>(1).expect("exif").bounds();
    assert_close(status.x, 8.0 + 6.0);
    assert_close(status.y, bounds.y + 4.0);
    assert_close(exif.y, bounds.y + 4.0 + 18.0 + 2.0);
    // The child is Stretch: as wide as the content area of the panel.
    assert_close(status.width, 132.0);
}

#[test]
fn max_width_caps_the_panel_and_text_is_trimmed_to_the_remainder() {
    // C#: OverlayLayoutTests.MaxWidth_CapsThePanel_AndTextIsTrimmedToTheRemainder
    let fake = FakeTextRenderer::new();
    let mut folder = text("FolderInfoText", &"x".repeat(400), 10.0);
    folder.set_trimming(TextTrimming::CharacterEllipsis);
    let mut panel = badge(
        "FolderInfoPanel",
        OverlayHorizontalAlignment::Right,
        OverlayVerticalAlignment::Bottom,
    );
    panel.add(Box::new(folder));
    panel.base_mut().max_width = 640.0;

    panel.arrange(&context(&fake, 1920.0, 1080.0));

    assert_close(panel.bounds().width, 637.0);
    assert_close(panel.bounds().x, 1920.0 - 8.0 - 637.0);
    let request = fake.requests().last().cloned().expect("a layout request");
    assert_close(request.2, 628.0);
    assert_eq!(request.3, TextTrimming::CharacterEllipsis);
    assert!(panel.child::<TextElement>(0).expect("text").bounds().width <= 628.0);
}

#[test]
fn panel_narrower_than_client_text_gets_only_available_width() {
    // C#: OverlayLayoutTests.PanelNarrowerThanClient_TextGetsOnlyAvailableWidth
    let fake = FakeTextRenderer::new();
    let mut label = text("t", &"y".repeat(100), 10.0);
    label.set_trimming(TextTrimming::CharacterEllipsis);
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Top);
    panel.add(Box::new(label));

    panel.arrange(&context(&fake, 300.0, 200.0));

    let request = fake.requests().last().cloned().expect("a layout request");
    assert_close(request.2, 272.0);
    assert_close(panel.bounds().width, 270.0 + 12.0);
    assert!(panel.bounds().right() <= 300.0 - 8.0);
}

#[test]
fn hidden_child_takes_a_zero_slot_and_no_space() {
    // C#: OverlayLayoutTests.HiddenElement_IsCollapsed_NoSpaceNoDrawNoHit (panel part)
    let fake = FakeTextRenderer::new();
    let mut hidden = text("b", "BBBBBBBB", 10.0);
    hidden.base_mut().is_visible = false;
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Top);
    panel.add(Box::new(text("a", "AAAA", 10.0))).add(Box::new(hidden));
    let mut dc = RecordingDrawContext::new();

    panel.arrange(&context(&fake, 800.0, 600.0));
    panel.render(&mut dc);

    assert_close(panel.bounds().width, 20.0 + 12.0);
    assert_close(panel.bounds().height, 15.0 + 8.0);
    assert_eq!(dc.texts.len(), 1);
    assert_eq!(dc.texts[0].0, "AAAA");
    let hidden = panel.child::<TextElement>(1).expect("hidden child");
    assert_eq!(hidden.bounds().width, 0.0);
    assert!(!hidden.hit_test(PointD {
        x: hidden.bounds().x,
        y: hidden.bounds().y
    }));
}

#[test]
fn hidden_panel_fills_nothing_and_draws_no_child() {
    // C#: OverlayLayoutTests.HiddenPanel_DrawsNothing_AndBalancesNothing (panel part)
    let fake = FakeTextRenderer::new();
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Top);
    panel.add(Box::new(text("a", "AAAA", 10.0)));
    panel.base_mut().is_visible = false;
    let mut dc = RecordingDrawContext::new();

    panel.arrange(&context(&fake, 800.0, 600.0));
    panel.render(&mut dc);

    assert!(dc.calls.is_empty());
    assert_eq!(panel.bounds().width, 0.0);
}

#[test]
fn render_fills_rounded_background_then_text() {
    // C#: OverlayLayoutTests.Render_FillsRoundedBackground_ThenText_WithBalancedOpacityAndClip (panel part)
    let fake = FakeTextRenderer::new();
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Top);
    panel.add(Box::new(text("a", "AAAA", 10.0)));
    panel.set_opacity(0.5);
    let mut dc = RecordingDrawContext::new();

    panel.arrange(&context(&fake, 800.0, 600.0));
    panel.render(&mut dc);

    assert_eq!(
        dc.calls,
        [
            "push_opacity",
            "fill_rounded_rectangle",
            "push_opacity",
            "push_clip",
            "draw_text",
            "pop_clip",
            "pop_opacity",
            "pop_opacity"
        ]
    );
    assert!(dc.fills[0].rounded);
    assert_eq!(dc.fills[0].color, palette::overlay());
    assert_eq!(dc.fills[0].opacity, 0.5);
    assert_eq!(dc.clip_depth(), 0);
    assert_eq!(dc.texts[0].2, palette::text_bright());
    assert_eq!((dc.texts[0].1.x, dc.texts[0].1.y), (14.0, 12.0));
}

#[test]
fn vertical_center_places_panel_in_the_middle_of_the_client() {
    // C#: OverlayLayoutTests.VerticalCenter_PlacesPanelInTheMiddleOfTheClient
    let fake = FakeTextRenderer::new();
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Center);
    panel.add(Box::new(text("a", "ABCDEFGHIJ", 10.0)));

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_rect(panel.bounds(), rect(8.0, 288.5, 62.0, 23.0));
}

#[test]
fn max_width_caps_desired_width_even_when_a_child_cannot_shrink() {
    // C#: OverlayLayoutTests.MaxWidth_CapsTheDesiredWidth_EvenWhenAChildCannotShrink
    let fake = FakeTextRenderer::new();
    let mut panel = PanelElement::new("P");
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel.base_mut().max_width = 100.0;
    panel.add(Box::new(button("B", &"x".repeat(40), 10.0)));

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_close(panel.base().desired_size().width, 100.0);
    assert_close(panel.bounds().width, 100.0);
}

#[test]
fn horizontal_panel_places_children_side_by_side() {
    // C#: OverlayLayoutTests.HorizontalPanel_PlacesChildrenSideBySide
    let fake = FakeTextRenderer::new();
    let mut panel = PanelElement::new("P");
    panel.orientation = OverlayOrientation::Horizontal;
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel
        .add(Box::new(text("a", "AAAA", 10.0)))
        .add(Box::new(text("b", "BBBBBB", 10.0)));

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_rect(
        panel.child::<TextElement>(0).expect("a").bounds(),
        rect(0.0, 0.0, 20.0, 15.0),
    );
    assert_rect(
        panel.child::<TextElement>(1).expect("b").bounds(),
        rect(20.0, 0.0, 30.0, 15.0),
    );
    assert_rect(panel.bounds(), rect(0.0, 0.0, 50.0, 15.0));
}

#[test]
fn find_at_prefers_the_later_sibling_where_children_overlap() {
    // C#: OverlayLayoutTests.FindAt_PrefersTheLaterSibling_WhereChildrenOverlap
    let fake = FakeTextRenderer::new();
    let lower = button("Lower", "AAAA", 10.0);
    let mut upper = button("Upper", "BBBB", 10.0);
    upper.base_mut().margin = OverlayThickness::new(0.0, -10.0, 0.0, 0.0);
    let mut panel = PanelElement::new("P");
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel.add(Box::new(lower)).add(Box::new(upper));

    panel.arrange(&context(&fake, 800.0, 600.0));

    let overlap = PointD { x: 2.0, y: 20.0 };
    assert!(panel.child::<ButtonElement>(0).expect("lower").hit_test(overlap));
    assert!(panel.child::<ButtonElement>(1).expect("upper").hit_test(overlap));
    assert_eq!(panel.find_at(overlap).map(|found| found.base().id()), Some("Upper"));
}

#[test]
fn panel_find_returns_self_then_the_first_matching_child() {
    // C#: OverlayHostTests.Find_And_Resolve_WalkTheWholeTree (panel part)
    let mut panel = badge(
        "FolderInfoPanel",
        OverlayHorizontalAlignment::Right,
        OverlayVerticalAlignment::Bottom,
    );
    panel.add(Box::new(text("FolderInfoPanel.Text", "x", 10.0)));

    assert_eq!(
        panel.find("FolderInfoPanel").map(|found| found.base().id()),
        Some("FolderInfoPanel")
    );
    assert_eq!(
        panel.find("FolderInfoPanel.Text").map(|found| found.base().id()),
        Some("FolderInfoPanel.Text")
    );
    assert!(panel.find("Missing").is_none());
    // Ordinal: case matters.
    assert!(panel.find("folderinfopanel").is_none());
}

#[test]
fn panel_find_at_returns_topmost_deepest_element() {
    // C#: OverlayHostTests.HitTest_ReturnsTopmostDeepestElement_AndNullOverImage (panel part)
    let fake = FakeTextRenderer::new();
    let mut status = badge(
        "StatusPanel",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Bottom,
    );
    status.add(Box::new(text("status", "status", 10.0)));
    let mut toolbar = PanelElement::new("ToolbarPanel");
    toolbar.base_mut().margin = OverlayThickness::uniform(8.0);
    toolbar.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    toolbar.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    toolbar.add(Box::new(button("Fit", "Fit", 10.0)));
    status.arrange(&context(&fake, 800.0, 600.0));
    toolbar.arrange(&context(&fake, 800.0, 600.0));

    let fit = toolbar.child::<ButtonElement>(0).expect("fit").bounds();
    let hit = toolbar.find_at(PointD {
        x: fit.x + 1.0,
        y: fit.y + 1.0,
    });
    assert_eq!(hit.map(|found| found.base().id()), Some("Fit"));
    // The text is click-through, so the panel takes the hit.
    let inside = PointD {
        x: status.bounds().x + 1.0,
        y: status.bounds().y + 1.0,
    };
    assert_eq!(
        status.find_at(inside).map(|found| found.base().id()),
        Some("StatusPanel")
    );
    // Over the image below: nothing.
    let image = PointD { x: 400.0, y: 300.0 };
    assert!(status.find_at(image).is_none());
    assert!(toolbar.find_at(image).is_none());
}

#[test]
fn defaults_match_the_csharp_class() {
    // new: contract note 1
    let panel = PanelElement::new("P");
    assert_eq!(panel.background, palette::overlay());
    assert_eq!(panel.corner_radius, 0.0);
    assert_eq!(panel.padding, OverlayThickness::ZERO);
    assert_eq!(panel.orientation, OverlayOrientation::Vertical);
    assert!(panel.children().is_empty());
    assert_eq!(panel.base().max_width, f64::INFINITY);
    assert!(panel.base().is_hit_test_visible);
    assert!(panel.base().is_visible);
    assert_eq!(panel.base().opacity(), 1.0);
    assert_eq!(OverlayOrientation::default(), OverlayOrientation::Vertical);
    assert_eq!(OverlayOrientation::Vertical as i32, 0);
    assert_eq!(OverlayOrientation::Horizontal as i32, 1);
}

#[test]
fn add_appends_in_order_and_chains() {
    // new: contract note 2
    let mut panel = PanelElement::new("P");
    panel
        .add(Box::new(Probe::fixed("one", 1.0, 1.0)))
        .add(Box::new(Probe::fixed("two", 1.0, 1.0)))
        .add(Box::new(Probe::fixed("three", 1.0, 1.0)));
    let ids: Vec<&str> = panel.children().iter().map(|child| child.base().id()).collect();
    assert_eq!(ids, ["one", "two", "three"]);
}

#[test]
fn remove_takes_the_first_child_with_the_id() {
    // new: trap T5
    let mut panel = PanelElement::new("P");
    panel
        .add(Box::new(Probe::fixed("dup", 1.0, 1.0)))
        .add(Box::new(Probe::fixed("other", 2.0, 2.0)))
        .add(Box::new(Probe::fixed("dup", 3.0, 3.0)));

    assert!(panel.remove("dup"));
    let remaining: Vec<(&str, f64)> = panel
        .children()
        .iter()
        .map(|child| (child.base().id(), child.base().desired_size().width))
        .collect();
    assert_eq!(remaining, [("other", 0.0), ("dup", 0.0)]);
    assert!(panel.child::<Probe>(1).expect("second dup").content.width == 3.0);
    assert!(!panel.remove("missing"));
    assert!(!panel.remove("DUP"));
    assert_eq!(panel.children().len(), 2);
    assert!(panel.remove("dup"));
    assert!(panel.remove("other"));
    assert!(!panel.remove("other"));
    assert!(panel.children().is_empty());
}

#[test]
fn child_access_is_typed_and_bounds_checked() {
    // new: trap T4
    let mut panel = PanelElement::new("P");
    panel
        .add(Box::new(text("t", "abc", 10.0)))
        .add(Box::new(Probe::fixed("p", 1.0, 1.0)));

    assert_eq!(panel.child::<TextElement>(0).map(TextElement::text), Some("abc"));
    assert!(panel.child::<Probe>(0).is_none());
    assert!(panel.child::<TextElement>(1).is_none());
    assert!(panel.child::<TextElement>(2).is_none());
    assert!(panel.child_mut::<Probe>(0).is_none());
    assert!(panel.child_mut::<TextElement>(5).is_none());
    panel.child_mut::<TextElement>(0).expect("text").set_text("changed");
    assert_eq!(panel.child::<TextElement>(0).map(TextElement::text), Some("changed"));
}

#[test]
fn nan_available_width_propagates_like_math_max() {
    // new: trap T2 (an echo child returns what it is offered; `f64::max` would turn NaN into 0)
    let fake = FakeTextRenderer::new();
    let ctx = context(&fake, 800.0, 600.0);
    let mut vertical = PanelElement::new("V");
    vertical.add(Box::new(Probe::echo("e")));
    let size = vertical.measure(
        SizeD {
            width: f64::NAN,
            height: 100.0,
        },
        &ctx,
    );
    assert!(size.width.is_nan());
    assert_eq!(size.height, 100.0);

    let mut horizontal = PanelElement::new("H");
    horizontal.orientation = OverlayOrientation::Horizontal;
    horizontal.add(Box::new(Probe::echo("e")));
    let size = horizontal.measure(
        SizeD {
            width: 100.0,
            height: f64::NAN,
        },
        &ctx,
    );
    assert!(size.height.is_nan());

    // The vertical room left for the second child is also NaN-aware: NaN height reaches the echo child.
    let mut stacked = PanelElement::new("S");
    stacked
        .add(Box::new(Probe::fixed("a", 1.0, 1.0)))
        .add(Box::new(Probe::echo("e")));
    let size = stacked.measure(
        SizeD {
            width: 100.0,
            height: f64::NAN,
        },
        &ctx,
    );
    assert!(size.height.is_nan());
}

#[test]
fn measure_gives_each_child_the_room_left_on_the_stacking_axis() {
    // new: contract note 3 (the offered sizes are observed through echo children)
    let fake = FakeTextRenderer::new();
    let ctx = context(&fake, 800.0, 600.0);
    let mut vertical = PanelElement::new("V");
    vertical.padding = OverlayThickness::new(1.0, 2.0, 3.0, 4.0);
    vertical
        .add(Box::new(Probe::fixed("a", 10.0, 30.0)))
        .add(Box::new(Probe::echo("e")));
    let size = vertical.measure(
        SizeD {
            width: 100.0,
            height: 100.0,
        },
        &ctx,
    );
    // inner 96 x 94; after the 30 DIP child the echo child gets 96 x 64.
    assert_eq!((size.width, size.height), (96.0 + 4.0, 30.0 + 64.0 + 6.0));

    let mut horizontal = PanelElement::new("H");
    horizontal.orientation = OverlayOrientation::Horizontal;
    horizontal.padding = OverlayThickness::new(1.0, 2.0, 3.0, 4.0);
    horizontal
        .add(Box::new(Probe::fixed("a", 30.0, 10.0)))
        .add(Box::new(Probe::echo("e")));
    let size = horizontal.measure(
        SizeD {
            width: 100.0,
            height: 100.0,
        },
        &ctx,
    );
    // inner 96 x 94; after the 30 DIP child the echo child gets 66 x 94.
    assert_eq!((size.width, size.height), (30.0 + 66.0 + 4.0, 94.0 + 6.0));
}

#[test]
fn padding_larger_than_available_floors_the_inner_size_at_zero() {
    // new: contract note 3 (max0)
    let fake = FakeTextRenderer::new();
    let ctx = context(&fake, 800.0, 600.0);
    let mut panel = PanelElement::new("P");
    panel.padding = OverlayThickness::uniform(20.0);
    panel.add(Box::new(Probe::echo("e")));
    let size = panel.measure(
        SizeD {
            width: 10.0,
            height: 10.0,
        },
        &ctx,
    );
    assert_eq!((size.width, size.height), (40.0, 40.0));
}

#[test]
fn hidden_child_in_the_middle_does_not_move_the_cursor() {
    // new: trap T9 (the next visible sibling starts where the hidden one would have)
    let fake = FakeTextRenderer::new();
    let mut hidden = Probe::fixed("hidden", 50.0, 50.0);
    hidden.base_mut().is_visible = false;
    let mut panel = PanelElement::new("P");
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel.padding = OverlayThickness::uniform(2.0);
    panel
        .add(Box::new(Probe::fixed("a", 10.0, 10.0)))
        .add(Box::new(hidden))
        .add(Box::new(Probe::fixed("c", 10.0, 10.0)));

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_rect(panel.child::<Probe>(0).expect("a").bounds(), rect(2.0, 2.0, 10.0, 10.0));
    // The hidden child records a zero box at the cursor (after "a"), not RectD::default().
    assert_rect(
        panel.child::<Probe>(1).expect("hidden").bounds(),
        rect(2.0, 12.0, 0.0, 0.0),
    );
    assert_rect(
        panel.child::<Probe>(2).expect("c").bounds(),
        rect(2.0, 12.0, 10.0, 10.0),
    );
    assert_rect(panel.bounds(), rect(0.0, 0.0, 14.0, 24.0));
}

#[test]
fn arrange_uses_the_measured_size_not_the_previous_bounds() {
    // new: trap T10
    let fake = FakeTextRenderer::new();
    let ctx = context(&fake, 800.0, 600.0);
    let mut panel = PanelElement::new("P");
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel
        .add(Box::new(text("a", "AAAA", 10.0)))
        .add(Box::new(text("b", "BBBB", 10.0)));
    panel.arrange(&ctx);
    assert_close(panel.child::<TextElement>(1).expect("b").bounds().y, 15.0);

    panel.child_mut::<TextElement>(0).expect("a").set_style(style(20.0));
    panel.arrange(&ctx);

    assert_close(panel.child::<TextElement>(1).expect("b").bounds().y, 30.0);
}

#[test]
fn render_uses_a_plain_fill_without_corner_radius_and_none_for_empty_or_transparent() {
    // new: contract note 5
    let fake = FakeTextRenderer::new();
    let ctx = context(&fake, 800.0, 600.0);
    let mut panel = PanelElement::new("P");
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel.add(Box::new(Probe::fixed("a", 10.0, 10.0)));
    panel.background = ColorF {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 0.25,
    };
    panel.arrange(&ctx);

    let mut dc = RecordingDrawContext::new();
    panel.render(&mut dc);
    assert_eq!(
        dc.calls,
        [
            "push_opacity",
            "fill_rectangle",
            "push_opacity",
            "pop_opacity",
            "pop_opacity"
        ]
    );
    assert_eq!(dc.fills[0].color, panel.background);
    assert!(!dc.fills[0].rounded);
    assert_rect(dc.fills[0].rect, rect(0.0, 0.0, 10.0, 10.0));

    // Transparent: no fill (a NaN alpha is not `> 0` either).
    for alpha in [0.0, f32::NAN] {
        panel.background.a = alpha;
        let mut dc = RecordingDrawContext::new();
        panel.render(&mut dc);
        assert!(dc.fills.is_empty(), "alpha {alpha}");
    }

    // No area: no fill, in either dimension.
    panel.background.a = 1.0;
    for (width, height) in [(0.0, 10.0), (10.0, 0.0)] {
        let mut flat = PanelElement::new("F");
        flat.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
        flat.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
        flat.add(Box::new(Probe::fixed("z", width, height)));
        flat.arrange(&ctx);
        let mut dc = RecordingDrawContext::new();
        flat.render(&mut dc);
        assert!(dc.fills.is_empty(), "{width} x {height}");
    }
}

#[test]
fn render_draws_the_background_before_the_children_in_order() {
    // new: contract note 5 (children after the fill, in insertion order)
    let fake = FakeTextRenderer::new();
    let mut panel = PanelElement::new("P");
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel
        .add(Box::new(text("a", "one", 10.0)))
        .add(Box::new(text("b", "two", 10.0)));
    let mut dc = RecordingDrawContext::new();

    panel.arrange(&context(&fake, 800.0, 600.0));
    panel.render(&mut dc);

    let order: Vec<&str> = dc.texts.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(order, ["one", "two"]);
    let fill = dc.calls.iter().position(|c| c.starts_with("fill")).expect("fill");
    let first_text = dc.calls.iter().position(|c| *c == "draw_text").expect("text");
    assert!(fill < first_text);
}

#[test]
fn find_at_is_none_when_the_panel_does_not_hit_and_self_when_no_child_does() {
    // new: contract note 6
    let fake = FakeTextRenderer::new();
    let ctx = context(&fake, 800.0, 600.0);
    let mut panel = PanelElement::new("P");
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel.padding = OverlayThickness::uniform(5.0);
    panel.add(Box::new(Probe::fixed("a", 10.0, 10.0)));
    panel.arrange(&ctx);

    // On the padding: no child, the panel itself.
    assert_eq!(
        panel.find_at(PointD { x: 1.0, y: 1.0 }).map(|f| f.base().id()),
        Some("P")
    );
    // On the child.
    assert_eq!(
        panel.find_at(PointD { x: 6.0, y: 6.0 }).map(|f| f.base().id()),
        Some("a")
    );
    // Outside the panel.
    assert!(panel.find_at(PointD { x: 100.0, y: 100.0 }).is_none());
    // Hidden, transparent or click-through panels do not hit.
    panel.base_mut().is_visible = false;
    assert!(panel.find_at(PointD { x: 1.0, y: 1.0 }).is_none());
    panel.base_mut().is_visible = true;
    panel.set_opacity(0.0);
    assert!(panel.find_at(PointD { x: 1.0, y: 1.0 }).is_none());
    panel.set_opacity(1.0);
    panel.base_mut().is_hit_test_visible = false;
    assert!(panel.find_at(PointD { x: 1.0, y: 1.0 }).is_none());
}

#[test]
fn find_returns_the_first_match_in_insertion_order() {
    // new: contract note 7
    let mut inner = PanelElement::new("inner");
    inner.add(Box::new(Probe::fixed("twin", 1.0, 1.0)));
    let mut panel = PanelElement::new("P");
    panel
        .add(Box::new(Probe::fixed("first", 1.0, 1.0)))
        .add(Box::new(inner))
        .add(Box::new(Probe::fixed("twin", 2.0, 2.0)));

    let found = panel.find("twin").expect("twin");
    // The nested one comes first: it is inside the earlier child "inner".
    assert!(found.as_any().downcast_ref::<Probe>().expect("probe").content.width == 1.0);
    assert_eq!(panel.find("inner").map(|f| f.base().id()), Some("inner"));
}

#[test]
fn release_resources_reaches_every_child_including_nested_ones() {
    // new: contract note 8
    let a = Probe::fixed("a", 1.0, 1.0);
    let b = Probe::fixed("b", 1.0, 1.0);
    let (ra, rb) = (Rc::clone(&a.releases), Rc::clone(&b.releases));
    let mut inner = PanelElement::new("inner");
    inner.add(Box::new(b));
    let mut panel = PanelElement::new("P");
    panel.add(Box::new(a)).add(Box::new(inner));

    panel.release_resources();

    assert_eq!((ra.get(), rb.get()), (1, 1));
}

#[test]
fn horizontal_arrange_gives_children_the_inner_height_inside_the_padding() {
    // new: kills `-` -> `+` in arrange_stack (inner height = bounds.height - padding.vertical())
    let fake = FakeTextRenderer::new();
    let mut panel = PanelElement::new("P");
    panel.orientation = OverlayOrientation::Horizontal;
    panel.padding = OverlayThickness::symmetric(6.0, 4.0);
    panel.base_mut().horizontal_alignment = OverlayHorizontalAlignment::Left;
    panel.base_mut().vertical_alignment = OverlayVerticalAlignment::Top;
    panel.add(Box::new(text("a", "AAAA", 10.0)));

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_rect(panel.bounds(), rect(0.0, 0.0, 32.0, 23.0));
    assert_rect(
        panel.child::<TextElement>(0).expect("a").bounds(),
        rect(6.0, 4.0, 20.0, 15.0),
    );
}
