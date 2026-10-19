//! Red tests (W8-15d, test porter) for `pr_app::overlay::panel::PanelElement`.
//!
//! Port of the panel part of `PhotoReview.Shell.Tests/Overlay/{OverlayLayoutTests,OverlayHostTests}.cs` (baseline `d92d6aae`)
//! plus F tests (`// new:`) for the traps T1, T2, T4, T5, T12 of `contracts/W8-15d.md`.
//! The fakes are in `overlay_fakes.rs` (`FakeTextRenderer`: width = characters x size / 2, height = size x 1.5, ellipsis cut
//! to whole characters; `RecordingDrawContext`).
//!
//! Every test builds a `PanelElement` first, whose `new` is still `todo!()`, so the tests fail until the porter fills
//! `overlay::panel`.

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use std::cell::RefCell;
use std::rc::Rc;

use overlay_fakes::{FakeTextRenderer, RecordingDrawContext};
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::button::ButtonElement;
use pr_app::overlay::contracts::{OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{
    ElementBase, ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment,
};
use pr_app::overlay::palette;
use pr_app::overlay::panel::{OverlayOrientation, PanelElement};
use pr_app::overlay::text::TextElement;
use pr_app::services::render::DrawContext;
use pr_app::services::render::text::{TextStyle, TextTrimming};

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

/// C# `Text(id, text, size)`.
fn text(id: &str, value: &str, size: f64) -> TextElement {
    let mut element = TextElement::new(id).expect("non-empty id");
    element.set_text(value);
    element.set_style(style(size));
    element
}

/// C# `Badge(id, ...)`: margin 8, padding (6, 4), corner radius 4.
fn badge(id: &str, h: OverlayHorizontalAlignment, v: OverlayVerticalAlignment) -> PanelElement {
    let mut panel = PanelElement::new(id);
    {
        let base = ElementNode::base_mut(&mut panel);
        base.horizontal_alignment = h;
        base.vertical_alignment = v;
        base.margin = OverlayThickness::uniform(8.0);
    }
    panel.padding = OverlayThickness::symmetric(6.0, 4.0);
    panel.corner_radius = 4.0;
    panel
}

fn top_left(id: &str) -> PanelElement {
    let mut panel = PanelElement::new(id);
    let base = ElementNode::base_mut(&mut panel);
    base.horizontal_alignment = OverlayHorizontalAlignment::Left;
    base.vertical_alignment = OverlayVerticalAlignment::Top;
    panel
}

fn assert_rect(actual: RectD, x: f64, y: f64, width: f64, height: f64) {
    assert_eq!((actual.x, actual.y, actual.width, actual.height), (x, y, width, height));
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "expected {expected}, got {actual}");
}

fn text_child(panel: &PanelElement, index: usize) -> &TextElement {
    panel.child::<TextElement>(index).expect("text child")
}

/// Element with a scripted measure: returns `size` (or the offered size when `echo`) and records what it was offered.
#[derive(Debug)]
struct Probe {
    base: ElementBase,
    size: SizeD,
    echo: bool,
    seen: Rc<RefCell<Vec<SizeD>>>,
}

impl Probe {
    fn new(id: &str, size: SizeD, seen: &Rc<RefCell<Vec<SizeD>>>) -> Self {
        Self {
            base: ElementBase::new(id, true),
            size,
            echo: false,
            seen: Rc::clone(seen),
        }
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
        self.seen.borrow_mut().push(available);
        if self.echo { available } else { self.size }
    }

    fn render_core(&self, _dc: &mut dyn DrawContext) {}

    fn find_at(&self, point: PointD) -> Option<&dyn ElementNode> {
        if self.hit_test(point) { Some(self) } else { None }
    }

    fn find(&self, id: &str) -> Option<&dyn ElementNode> {
        if self.base.id() == id { Some(self) } else { None }
    }
}

#[test]
fn panel_defaults_match_the_c_initialisers() {
    // new: note 1 (defaults)
    let panel = PanelElement::new("P");

    assert_eq!(panel.orientation, OverlayOrientation::Vertical);
    assert_eq!(panel.corner_radius, 0.0);
    assert_eq!(panel.padding, OverlayThickness::ZERO);
    assert_eq!(panel.background, palette::overlay());
    assert!(panel.children().is_empty());
    let base = ElementNode::base(&panel);
    assert_eq!(base.id(), "P");
    assert!(base.is_visible);
    assert_eq!(base.opacity(), 1.0);
    assert!(base.max_width.is_infinite());
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Stretch);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Stretch);
}

#[test]
fn panel_sizes_to_text_plus_padding_and_anchors_to_corners() {
    // C#: OverlayLayoutTests.Panel_SizesToTextPlusPadding_AndAnchorsToCorners
    let fake = FakeTextRenderer::new();
    // "ABCDEFGHIJ" = 10 characters x 5 = 50 wide, 15 high.
    let mut bottom_left = badge("A", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Bottom);
    bottom_left.add(Box::new(text("a", "ABCDEFGHIJ", 10.0)));
    let mut top_right = badge("B", OverlayHorizontalAlignment::Right, OverlayVerticalAlignment::Top);
    top_right.add(Box::new(text("b", "ABCDEFGHIJ", 10.0)));
    let mut center = badge("C", OverlayHorizontalAlignment::Center, OverlayVerticalAlignment::Top);
    center.add(Box::new(text("c", "ABCDEFGHIJ", 10.0)));
    ElementNode::base_mut(&mut center).margin = OverlayThickness::new(8.0, 44.0, 8.0, 8.0);

    bottom_left.arrange(&context(&fake, 800.0, 600.0));
    top_right.arrange(&context(&fake, 800.0, 600.0));
    center.arrange(&context(&fake, 800.0, 600.0));

    assert_rect(bottom_left.bounds(), 8.0, 600.0 - 8.0 - 23.0, 62.0, 23.0);
    assert_rect(top_right.bounds(), 800.0 - 8.0 - 62.0, 8.0, 62.0, 23.0);
    assert_rect(center.bounds(), 369.0, 44.0, 62.0, 23.0);
}

#[test]
fn stretch_panel_fills_client_minus_margin() {
    // C#: OverlayLayoutTests.StretchPanel_FillsClientMinusMargin
    let fake = FakeTextRenderer::new();
    let mut compare = PanelElement::new("ComparePanel");
    ElementNode::base_mut(&mut compare).margin = OverlayThickness::uniform(8.0);

    compare.arrange(&context(&fake, 1366.0, 728.0));

    assert_rect(compare.bounds(), 8.0, 8.0, 1350.0, 712.0);
}

#[test]
fn vertical_stack_width_is_widest_child_height_is_sum_with_margins() {
    // C#: OverlayLayoutTests.VerticalStack_WidthIsWidestChild_HeightIsSumWithMargins
    let fake = FakeTextRenderer::new();
    let status = text("StatusText", "IMG_0042.JPG", 12.0); // 12 x 6 = 72 wide, 18 high
    let mut exif = text("ExifInfoText", "Canon EOS R5 85 mm f/1.8", 11.0); // 24 x 5.5 = 132 wide, 16.5 high
    ElementNode::base_mut(&mut exif).margin = OverlayThickness::new(0.0, 2.0, 0.0, 0.0);
    let mut panel = badge(
        "StatusPanel",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Bottom,
    );
    panel.add(Box::new(status)).add(Box::new(exif));

    panel.arrange(&context(&fake, 800.0, 600.0));

    let status = text_child(&panel, 0);
    let exif = text_child(&panel, 1);
    assert_close(panel.bounds().width, 132.0 + 12.0);
    assert_close(panel.bounds().height, 18.0 + 2.0 + 16.5 + 8.0);
    assert_close(status.bounds().x, 8.0 + 6.0);
    assert_close(status.bounds().y, panel.bounds().y + 4.0);
    assert_close(exif.bounds().y, panel.bounds().y + 4.0 + 18.0 + 2.0);
    // A stretch child is as wide as the content area of the panel.
    assert_close(status.bounds().width, 132.0);
}

#[test]
fn max_width_caps_the_panel_and_text_is_trimmed_to_the_remainder() {
    // C#: OverlayLayoutTests.MaxWidth_CapsThePanel_AndTextIsTrimmedToTheRemainder
    let fake = FakeTextRenderer::new();
    let mut folder = text("FolderInfoText", &"x".repeat(400), 10.0); // 400 x 5 = 2000 wide
    folder.set_trimming(TextTrimming::CharacterEllipsis);
    let mut panel = badge(
        "FolderInfoPanel",
        OverlayHorizontalAlignment::Right,
        OverlayVerticalAlignment::Bottom,
    );
    panel.add(Box::new(folder));
    ElementNode::base_mut(&mut panel).max_width = 640.0;

    panel.arrange(&context(&fake, 1920.0, 1080.0));

    // Text cut to 625 (floor on the 5 DIP characters of the fake) + 12 padding; never above MaxWidth.
    assert_close(panel.bounds().width, 637.0);
    assert_close(panel.bounds().x, 1920.0 - 8.0 - 637.0);
    // The text is measured with maxWidth = 640 - 12 (padding).
    let last = fake.requests().pop().expect("a layout request");
    assert_close(last.2, 628.0);
    assert_eq!(last.3, TextTrimming::CharacterEllipsis);
    assert!(text_child(&panel, 0).bounds().width <= 628.0);
}

#[test]
fn panel_narrower_than_client_text_gets_only_available_width() {
    // C#: OverlayLayoutTests.PanelNarrowerThanClient_TextGetsOnlyAvailableWidth
    let fake = FakeTextRenderer::new();
    let mut long = text("t", &"y".repeat(100), 10.0); // 500 wide
    long.set_trimming(TextTrimming::CharacterEllipsis);
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Top);
    panel.add(Box::new(long));

    panel.arrange(&context(&fake, 300.0, 200.0));

    // 300 - 16 (margin) - 12 (padding) = 272.
    let last = fake.requests().pop().expect("a layout request");
    assert_close(last.2, 272.0);
    assert_close(panel.bounds().width, 270.0 + 12.0);
    assert!(panel.bounds().x + panel.bounds().width <= 300.0 - 8.0);
}

#[test]
fn hidden_child_takes_a_zero_slot_and_no_space() {
    // C#: OverlayLayoutTests.HiddenElement_IsCollapsed_NoSpaceNoDrawNoHit (panel part), T9
    let fake = FakeTextRenderer::new();
    let first = text("a", "AAAA", 10.0);
    let mut hidden = text("b", "BBBBBBBB", 10.0);
    ElementNode::base_mut(&mut hidden).is_visible = false;
    let third = text("c", "CC", 10.0);
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Top);
    panel.add(Box::new(first)).add(Box::new(hidden)).add(Box::new(third));
    let mut dc = RecordingDrawContext::new();

    panel.arrange(&context(&fake, 800.0, 600.0));
    panel.render(&mut dc);

    assert_close(panel.bounds().width, 20.0 + 12.0);
    assert_close(panel.bounds().height, 15.0 + 15.0 + 8.0);
    let texts: Vec<&str> = dc.texts.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(texts, ["AAAA", "CC"]);
    let hidden = text_child(&panel, 1);
    assert_eq!(hidden.bounds().width, 0.0);
    assert_eq!(hidden.bounds().height, 0.0);
    assert!(!hidden.hit_test(PointD {
        x: hidden.bounds().x,
        y: hidden.bounds().y
    }));
    // The hidden child sat at the cursor and did not move it: the next visible sibling starts there.
    assert_close(hidden.bounds().y, text_child(&panel, 0).bounds().y + 15.0);
    assert_close(text_child(&panel, 2).bounds().y, hidden.bounds().y);
}

#[test]
fn hidden_panel_fills_nothing_and_draws_no_child() {
    // C#: OverlayLayoutTests.HiddenPanel_DrawsNothing_AndBalancesNothing (panel part)
    let fake = FakeTextRenderer::new();
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Top);
    panel.add(Box::new(text("a", "AAAA", 12.0)));
    ElementNode::base_mut(&mut panel).is_visible = false;
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
    ElementNode::base_mut(&mut panel).set_opacity(0.5);
    let mut dc = RecordingDrawContext::new();

    panel.arrange(&context(&fake, 800.0, 600.0));
    panel.render(&mut dc);

    let fill_at = dc
        .calls
        .iter()
        .position(|c| *c == "fill_rounded_rectangle")
        .expect("rounded fill");
    let text_at = dc.calls.iter().position(|c| *c == "draw_text").expect("text");
    assert!(fill_at < text_at, "the background is drawn before the child");
    assert_eq!(dc.fills.len(), 1);
    assert!(dc.fills[0].rounded);
    assert_eq!(dc.fills[0].color, palette::overlay());
    assert_eq!(dc.fills[0].opacity, 0.5);
    assert_eq!(dc.fills[0].rect, panel.bounds());
    assert_eq!(dc.clip_depth(), 0);
    assert_eq!(dc.texts[0].2, palette::text_bright());
    assert_eq!(dc.texts[0].1, PointD { x: 14.0, y: 12.0 });
}

#[test]
fn render_uses_a_plain_rectangle_when_the_corner_radius_is_zero() {
    // new: note 5 (corner_radius > 0 selects the rounded fill)
    let fake = FakeTextRenderer::new();
    let mut panel = top_left("P");
    panel.add(Box::new(text("a", "AAAA", 10.0)));
    let mut dc = RecordingDrawContext::new();

    panel.arrange(&context(&fake, 800.0, 600.0));
    panel.render(&mut dc);

    assert_eq!(dc.fills.len(), 1);
    assert!(!dc.fills[0].rounded);
    assert!(dc.calls.contains(&"fill_rectangle"));
}

#[test]
fn render_skips_the_fill_for_a_transparent_background_or_empty_bounds() {
    // new: note 5 (alpha > 0 and both bounds dimensions > 0)
    let fake = FakeTextRenderer::new();
    let mut transparent = top_left("T");
    transparent.background.a = 0.0;
    transparent.add(Box::new(text("a", "AAAA", 10.0)));
    let mut empty = top_left("E"); // no children and no padding: zero-size bounds
    let mut dc = RecordingDrawContext::new();

    transparent.arrange(&context(&fake, 800.0, 600.0));
    transparent.render(&mut dc);
    assert!(dc.fills.is_empty());
    assert_eq!(dc.texts.len(), 1, "the child is still drawn");

    let mut dc = RecordingDrawContext::new();
    empty.arrange(&context(&fake, 800.0, 600.0));
    empty.render(&mut dc);
    assert!(dc.fills.is_empty());
}

#[test]
fn vertical_center_places_panel_in_the_middle_of_the_client() {
    // C#: OverlayLayoutTests.VerticalCenter_PlacesPanelInTheMiddleOfTheClient
    let fake = FakeTextRenderer::new();
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Center);
    panel.add(Box::new(text("a", "ABCDEFGHIJ", 10.0)));

    panel.arrange(&context(&fake, 800.0, 600.0));

    // Height 23 (15 text + 8 padding), margin 8: y = 8 + (584 - 23) / 2.
    assert_rect(panel.bounds(), 8.0, 288.5, 62.0, 23.0);
}

#[test]
fn max_width_caps_desired_width_even_when_a_child_cannot_shrink() {
    // C#: OverlayLayoutTests.MaxWidth_CapsTheDesiredWidth_EvenWhenAChildCannotShrink
    let fake = FakeTextRenderer::new();
    // A never-trimmed one-line button: 40 x 5 + 16 + 2 = 218 DIP whatever room it is offered.
    let mut button = ButtonElement::new("B").expect("non-empty id");
    button.set_content(&"x".repeat(40));
    button.set_style(style(10.0));
    let mut panel = top_left("P");
    ElementNode::base_mut(&mut panel).max_width = 100.0;
    panel.add(Box::new(button));

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_close(ElementNode::base(&panel).desired_size().width, 100.0);
    assert_close(panel.bounds().width, 100.0);
}

#[test]
fn horizontal_panel_places_children_side_by_side() {
    // C#: OverlayLayoutTests.HorizontalPanel_PlacesChildrenSideBySide
    let fake = FakeTextRenderer::new();
    let mut panel = top_left("P");
    panel.orientation = OverlayOrientation::Horizontal;
    panel
        .add(Box::new(text("a", "AAAA", 10.0))) // 20 x 15
        .add(Box::new(text("b", "BBBBBB", 10.0))); // 30 x 15

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_rect(text_child(&panel, 0).bounds(), 0.0, 0.0, 20.0, 15.0);
    assert_rect(text_child(&panel, 1).bounds(), 20.0, 0.0, 30.0, 15.0);
    assert_rect(panel.bounds(), 0.0, 0.0, 50.0, 15.0);
}

#[test]
fn vertical_stack_offers_each_child_the_remaining_height() {
    // new: note 3 (measure: full inner width, inner height minus what the previous children took)
    let fake = FakeTextRenderer::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut panel = top_left("P");
    panel.padding = OverlayThickness::symmetric(6.0, 4.0);
    for (id, height) in [("a", 30.0), ("b", 20.0), ("c", 10.0)] {
        panel.add(Box::new(Probe::new(id, SizeD { width: 50.0, height }, &seen)));
    }

    let desired = panel.measure(
        SizeD {
            width: 200.0,
            height: 100.0,
        },
        &context(&fake, 800.0, 600.0),
    );

    let offered: Vec<(f64, f64)> = seen.borrow().iter().map(|s| (s.width, s.height)).collect();
    assert_eq!(offered, [(188.0, 92.0), (188.0, 62.0), (188.0, 42.0)]);
    assert_close(desired.width, 50.0 + 12.0);
    assert_close(desired.height, 60.0 + 8.0);
}

#[test]
fn horizontal_stack_offers_each_child_the_remaining_width() {
    // new: note 3 (measure, horizontal)
    let fake = FakeTextRenderer::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut panel = top_left("P");
    panel.orientation = OverlayOrientation::Horizontal;
    panel.padding = OverlayThickness::symmetric(6.0, 4.0);
    panel.add(Box::new(Probe::new(
        "a",
        SizeD {
            width: 50.0,
            height: 10.0,
        },
        &seen,
    )));
    panel.add(Box::new(Probe::new(
        "b",
        SizeD {
            width: 40.0,
            height: 25.0,
        },
        &seen,
    )));

    let desired = panel.measure(
        SizeD {
            width: 200.0,
            height: 100.0,
        },
        &context(&fake, 800.0, 600.0),
    );

    let offered: Vec<(f64, f64)> = seen.borrow().iter().map(|s| (s.width, s.height)).collect();
    assert_eq!(offered, [(188.0, 92.0), (138.0, 92.0)]);
    assert_close(desired.width, 90.0 + 12.0);
    assert_close(desired.height, 25.0 + 8.0);
}

#[test]
fn nan_available_width_propagates_like_math_max() {
    // new: T2 (C# Math.Max propagates NaN; f64::max would turn the width into the other operand)
    let fake = FakeTextRenderer::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut echo = Probe::new("echo", SizeD::default(), &seen);
    echo.echo = true;
    let mut panel = top_left("P");
    panel.add(Box::new(echo));

    let desired = panel.measure(
        SizeD {
            width: f64::NAN,
            height: 100.0,
        },
        &context(&fake, 800.0, 600.0),
    );

    assert!(desired.width.is_nan(), "got {}", desired.width);
}

#[test]
fn find_at_prefers_the_later_sibling_where_children_overlap() {
    // C#: OverlayLayoutTests.FindAt_PrefersTheLaterSibling_WhereChildrenOverlap, T1
    let fake = FakeTextRenderer::new();
    let mut lower = ButtonElement::new("Lower").expect("non-empty id");
    lower.set_content("AAAA");
    lower.set_style(style(10.0));
    let mut upper = ButtonElement::new("Upper").expect("non-empty id");
    upper.set_content("BBBB");
    upper.set_style(style(10.0));
    // Pulled up 10 DIP: overlaps the lower button at y 15..25.
    ElementNode::base_mut(&mut upper).margin = OverlayThickness::new(0.0, -10.0, 0.0, 0.0);
    let mut panel = top_left("P");
    panel.add(Box::new(lower)).add(Box::new(upper));

    panel.arrange(&context(&fake, 800.0, 600.0));

    let overlap = PointD { x: 2.0, y: 20.0 };
    assert!(panel.child::<ButtonElement>(0).expect("lower").hit_test(overlap));
    assert!(panel.child::<ButtonElement>(1).expect("upper").hit_test(overlap));
    let hit = panel.find_at(overlap).expect("a hit");
    assert_eq!(hit.id(), "Upper"); // the element added later is on top
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
    status.add(Box::new(text("status", "IMG", 10.0)));
    let mut button = ButtonElement::new("Fit").expect("non-empty id");
    button.set_content("Fit");
    button.set_style(style(10.0));
    let mut toolbar = badge(
        "ToolbarPanel",
        OverlayHorizontalAlignment::Left,
        OverlayVerticalAlignment::Top,
    );
    toolbar.add(Box::new(button));

    status.arrange(&context(&fake, 800.0, 600.0));
    toolbar.arrange(&context(&fake, 800.0, 600.0));

    let fit = toolbar.child::<ButtonElement>(0).expect("button").bounds();
    let on_button = PointD {
        x: fit.x + 1.0,
        y: fit.y + 1.0,
    };
    assert_eq!(toolbar.find_at(on_button).expect("button hit").id(), "Fit");
    // Text is click-through: the panel itself takes the hit.
    let on_status = PointD {
        x: status.bounds().x + 1.0,
        y: status.bounds().y + 1.0,
    };
    assert_eq!(status.find_at(on_status).expect("panel hit").id(), "StatusPanel");
    // Over the image (outside the panel): no hit.
    assert!(status.find_at(PointD { x: 400.0, y: 300.0 }).is_none());
    assert!(toolbar.find_at(PointD { x: 400.0, y: 300.0 }).is_none());
}

#[test]
fn find_at_is_none_for_a_hidden_or_transparent_panel() {
    // new: note 6 (the base hit test includes visibility and opacity)
    let fake = FakeTextRenderer::new();
    let mut panel = badge("P", OverlayHorizontalAlignment::Left, OverlayVerticalAlignment::Top);
    panel.add(Box::new(text("a", "AAAA", 10.0)));
    panel.arrange(&context(&fake, 800.0, 600.0));
    let inside = PointD {
        x: panel.bounds().x + 2.0,
        y: panel.bounds().y + 2.0,
    };
    assert!(panel.find_at(inside).is_some());

    ElementNode::base_mut(&mut panel).set_opacity(0.0);
    assert!(panel.find_at(inside).is_none());

    ElementNode::base_mut(&mut panel).set_opacity(1.0);
    ElementNode::base_mut(&mut panel).is_visible = false;
    assert!(panel.find_at(inside).is_none());
}

#[test]
fn panel_find_returns_self_then_the_first_matching_child() {
    // C#: OverlayHostTests.Find_And_Resolve_WalkTheWholeTree (panel part)
    let mut panel = PanelElement::new("FolderInfoPanel");
    panel.add(Box::new(text("FolderInfoText", "x", 10.0)));

    assert_eq!(panel.find("FolderInfoPanel").expect("self").id(), "FolderInfoPanel");
    assert_eq!(panel.find("FolderInfoText").expect("child").id(), "FolderInfoText");
    assert!(panel.find("Missing").is_none());
    assert!(panel.find("folderinfopanel").is_none(), "ordinal, case-sensitive");
}

#[test]
fn find_returns_the_first_child_in_insertion_order_when_ids_repeat() {
    // new: note 7 (first match in insertion order)
    let mut panel = PanelElement::new("P");
    panel
        .add(Box::new(text("dup", "one", 10.0)))
        .add(Box::new(text("dup", "two", 10.0)));

    let found = panel.find("dup").expect("a match");
    let found = found.as_any().downcast_ref::<TextElement>().expect("a text");
    assert_eq!(found.text(), "one");
}

#[test]
fn add_appends_in_order_and_chains() {
    // new: note 2, T12 (`add` returns `&mut Self`)
    let mut panel = PanelElement::new("P");
    panel
        .add(Box::new(text("a", "1", 10.0)))
        .add(Box::new(text("b", "2", 10.0)))
        .add(Box::new(text("c", "3", 10.0)));

    let ids: Vec<&str> = panel.children().iter().map(|c| c.id()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

#[test]
fn remove_takes_the_first_child_with_the_id() {
    // new: T5 (remove by id, not by identity)
    let mut panel = PanelElement::new("P");
    panel
        .add(Box::new(text("dup", "one", 10.0)))
        .add(Box::new(text("other", "x", 10.0)))
        .add(Box::new(text("dup", "two", 10.0)));

    assert!(panel.remove("dup"));
    assert_eq!(panel.children().len(), 2);
    assert_eq!(text_child(&panel, 0).text(), "x");
    assert_eq!(text_child(&panel, 1).text(), "two");

    assert!(!panel.remove("Missing"));
    assert!(!panel.remove("OTHER"), "ordinal, case-sensitive");
    assert_eq!(panel.children().len(), 2);
}

#[test]
fn child_is_typed_by_index_and_none_for_a_wrong_type_or_index() {
    // new: T4 (downcast by type, `None` instead of a panic)
    let mut panel = PanelElement::new("P");
    panel.add(Box::new(text("a", "one", 10.0)));

    assert_eq!(panel.child::<TextElement>(0).map(TextElement::text), Some("one"));
    assert!(panel.child::<ButtonElement>(0).is_none());
    assert!(panel.child::<TextElement>(1).is_none());

    panel.child_mut::<TextElement>(0).expect("text").set_text("two");
    assert_eq!(text_child(&panel, 0).text(), "two");
    assert!(panel.child_mut::<ButtonElement>(0).is_none());
}

#[test]
fn release_resources_forwards_to_every_child() {
    // new: note 8
    let fake = FakeTextRenderer::new();
    let mut panel = top_left("P");
    panel
        .add(Box::new(text("a", "AAAA", 10.0)))
        .add(Box::new(text("b", "BBBB", 10.0)));
    panel.arrange(&context(&fake, 800.0, 600.0));
    assert_eq!(fake.live(), 2);

    panel.release_resources();

    assert_eq!(fake.live(), 0);
}
