//! F tests (W8-15d) for `pr_app::overlay::capture_badge::CapturePairBadge` (C# `CapturePairBadge`, baseline `d92d6aae`).
//!
//! Ports of the badge parts of `ShellOverlayBehaviorTests` plus `// new:` tests for the contract traps T6 (tooltip copied, not
//! cleared), T7 (wrapper identity) and T8 (margin and padding constructors).

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use overlay_fakes::{FakeTextRenderer, RecordingDrawContext};
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::overlay::capture_badge::{CapturePairBadge, PANEL_ID, TEXT_ID};
use pr_app::overlay::contracts::{OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment};
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::palette;
use pr_app::overlay::panel::PanelElement;
use pr_app::overlay::text::TextElement;
use pr_app::services::render::text::{TextStyle, TextTrimming};

fn context(text: &FakeTextRenderer) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::new(
        SizeD {
            width: 800.0,
            height: 600.0,
        },
        1.0,
        text,
    )
}

fn model(visible: bool, text: &str, tool_tip: &str, font_size: f64) -> OverlayModel {
    OverlayModel {
        capture_pair_visible: visible,
        capture_pair_text: text.to_owned(),
        capture_pair_tool_tip: tool_tip.to_owned(),
        font_size,
        ..OverlayModel::default()
    }
}

#[test]
fn constants_and_defaults() {
    // new: contract note 9
    assert_eq!(PANEL_ID, "CapturePairBadge");
    assert_eq!(TEXT_ID, "CapturePairText");
    let badge = CapturePairBadge::default();
    assert_eq!(badge.base().id(), PANEL_ID);
    assert!(badge.is_visible());
    assert_eq!(badge.tool_tip(), "");
    let text = badge.text().expect("text child");
    assert_eq!(text.base().id(), TEXT_ID);
    assert_eq!(text.text(), "");
    assert_eq!(text.trimming(), TextTrimming::None);
    let base = badge.base();
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Center);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Top);
    // T8: the 44 DIP top offset is the point of the badge.
    assert_eq!(base.margin, OverlayThickness::new(8.0, 44.0, 8.0, 8.0));
    assert_eq!(base.max_width, f64::INFINITY);
}

#[test]
fn badge_is_centred_at_the_top_44_dip_down_with_padding_6_4() {
    // new: contract note 9 (same numbers as C# `Panel_SizesToTextPlusPadding...` centre case: "ABCDEFGHIJ" at size 10)
    let fake = FakeTextRenderer::new();
    let mut badge = CapturePairBadge::new();
    badge.apply(&model(true, "ABCDEFGHIJ", "", 10.0));

    badge.arrange(&context(&fake));

    let bounds = badge.bounds();
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (369.0, 44.0, 62.0, 23.0)
    );
    let text = badge.text().expect("text").bounds();
    assert_eq!((text.x, text.y), (369.0 + 6.0, 44.0 + 4.0));
}

#[test]
fn badge_renders_a_rounded_overlay_background_then_the_text() {
    // new: contract note 9 and 14 (corner radius 4, delegation of render)
    let fake = FakeTextRenderer::new();
    let mut badge = CapturePairBadge::new();
    badge.apply(&model(true, "AAAA", "", 10.0));
    let mut dc = RecordingDrawContext::new();

    badge.arrange(&context(&fake));
    badge.render(&mut dc);

    assert_eq!(dc.fills.len(), 1);
    assert!(dc.fills[0].rounded);
    assert_eq!(dc.fills[0].color, palette::overlay());
    assert_eq!(dc.texts.len(), 1);
    assert_eq!(dc.texts[0].0, "AAAA");
    assert_eq!(dc.calls.iter().filter(|c| **c == "push_opacity").count(), 2);
}

#[test]
fn capture_badge_apply_sets_visibility_text_font() {
    // C#: ShellOverlayBehaviorTests.Model_DrivesVisibilityTextAndFontOfEveryPanel (badge part)
    let mut badge = CapturePairBadge::new();
    badge.apply(&model(true, "JPEG+RAW", "", 14.0));

    assert!(badge.is_visible());
    let text = badge.text().expect("text");
    assert_eq!(text.text(), "JPEG+RAW");
    assert_eq!(
        *text.style(),
        TextStyle {
            font_family: "Segoe UI".to_owned(),
            font_size_dip: 14.0,
            bold: false
        }
    );

    badge.apply(&model(false, "JPEG+RAW", "", 14.0));
    assert!(!badge.is_visible());
}

#[test]
fn capture_badge_tool_tip_is_copied_from_the_model() {
    // C#: ShellOverlayBehaviorTests.Tooltips_HiddenOverlaysHaveNone_ExifAndBadgeComeFromTheModel (the ToolTip copy), trap T6
    let mut badge = CapturePairBadge::new();
    badge.apply(&model(true, "JPEG+RAW", "Press F8", 12.0));
    assert_eq!(badge.tool_tip(), "Press F8");

    // Hidden but with a non-empty tooltip in the model: the tooltip is still copied (the model is the source of "empty").
    badge.apply(&model(false, "JPEG+RAW", "Press F8 again", 12.0));
    assert!(!badge.is_visible());
    assert_eq!(badge.tool_tip(), "Press F8 again");

    badge.apply(&model(false, "", "", 12.0));
    assert_eq!(badge.tool_tip(), "");
}

#[test]
fn apply_writes_every_field_whatever_the_visibility() {
    // new: contract note 10
    let mut badge = CapturePairBadge::new();
    badge.apply(&model(false, "hidden text", "hidden tip", 9.0));
    assert!(!badge.is_visible());
    assert_eq!(badge.text().expect("text").text(), "hidden text");
    assert_eq!(badge.text().expect("text").style().font_size_dip, 9.0);
    assert_eq!(badge.tool_tip(), "hidden tip");
}

#[test]
fn badge_find_returns_the_panel_with_the_badge_id() {
    // new: trap T7 (the delegation returns the inner panel, not the wrapper)
    let badge = CapturePairBadge::new();

    let found = badge.find(PANEL_ID).expect("badge id");
    assert_eq!(found.base().id(), PANEL_ID);
    assert!(found.as_any().downcast_ref::<PanelElement>().is_some());
    assert!(found.as_any().downcast_ref::<CapturePairBadge>().is_none());
    let text = badge.find(TEXT_ID).expect("text id");
    assert!(text.as_any().downcast_ref::<TextElement>().is_some());
    assert!(badge.find("Missing").is_none());
}

#[test]
fn hidden_badge_draws_nothing_and_takes_no_hit() {
    // new: contract note 14
    let fake = FakeTextRenderer::new();
    let mut badge = CapturePairBadge::new();
    badge.apply(&model(false, "AAAA", "tip", 10.0));
    let mut dc = RecordingDrawContext::new();

    badge.arrange(&context(&fake));
    badge.render(&mut dc);

    assert!(dc.calls.is_empty());
    assert_eq!(badge.bounds().width, 0.0);
    assert!(badge.find_at(PointD { x: 400.0, y: 50.0 }).is_none());
}

#[test]
fn visible_badge_hit_returns_the_inner_panel() {
    // new: trap T7 for `find_at` (the text is click-through, so the panel takes the hit)
    let fake = FakeTextRenderer::new();
    let mut badge = CapturePairBadge::new();
    badge.apply(&model(true, "ABCDEFGHIJ", "", 10.0));
    badge.arrange(&context(&fake));

    let hit = badge.find_at(PointD { x: 380.0, y: 50.0 }).expect("hit");
    assert_eq!(hit.base().id(), PANEL_ID);
    assert!(badge.find_at(PointD { x: 10.0, y: 10.0 }).is_none());
}

#[test]
fn release_resources_drops_the_text_layout() {
    // new: contract note 14 (the delegation reaches the text child)
    let fake = FakeTextRenderer::new();
    let mut badge = CapturePairBadge::new();
    badge.apply(&model(true, "AAAA", "", 10.0));
    badge.arrange(&context(&fake));
    assert_eq!(fake.live(), 1);

    badge.release_resources();

    assert_eq!(fake.live(), 0);
}
