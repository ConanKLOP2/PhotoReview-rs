//! Red tests (W8-15d, test porter) for `pr_app::overlay::capture_badge::CapturePairBadge`.
//!
//! Port of the badge part of `PhotoReview.Shell.Tests/Overlay/ShellOverlayBehaviorTests.cs` (baseline `d92d6aae`) plus F tests
//! (`// new:`) for the defaults and the traps T6, T7, T8 of `contracts/W8-15d.md`. Fakes are in `overlay_fakes.rs`.
//!
//! Every test builds a `CapturePairBadge` first, whose `new` is still `todo!()`, so the tests fail until the porter fills
//! `overlay::capture_badge` (and `overlay::panel`).

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use overlay_fakes::{FakeTextRenderer, RecordingDrawContext};
use pr_app::input::primitives::{PointD, RectD, SizeD};
use pr_app::overlay::capture_badge::{CapturePairBadge, PANEL_ID, TEXT_ID};
use pr_app::overlay::contracts::{OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment};
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::palette;
use pr_app::overlay::text::TextElement;

fn context(text: &FakeTextRenderer, width: f64, height: f64) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::new(SizeD { width, height }, 1.0, text)
}

fn pair_model() -> OverlayModel {
    OverlayModel {
        capture_pair_visible: true,
        capture_pair_text: "JPEG+RAW".to_owned(),
        capture_pair_tool_tip: "Press F8".to_owned(),
        ..OverlayModel::default()
    }
}

#[test]
fn capture_badge_ids_are_the_c_constants() {
    // new: note 9 (the constants are plain values; the badge is built so the test is red until the stub is filled)
    let badge = CapturePairBadge::new();

    assert_eq!(PANEL_ID, "CapturePairBadge");
    assert_eq!(TEXT_ID, "CapturePairText");
    assert_eq!(ElementNode::base(&badge).id(), PANEL_ID);
}

#[test]
fn capture_badge_defaults_are_center_top_with_the_44_dip_offset() {
    // new: note 9, T8 (margin (8, 44, 8, 8), not uniform 8)
    let badge = CapturePairBadge::new();

    let base = ElementNode::base(&badge);
    assert_eq!(base.id(), PANEL_ID);
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Center);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Top);
    assert_eq!(base.margin, OverlayThickness::new(8.0, 44.0, 8.0, 8.0));
    assert!(base.is_visible, "the badge is visible by default");
    assert_eq!(badge.tool_tip(), "");
    let text = badge.text().expect("child 0 is the text");
    assert_eq!(ElementNode::base(text).id(), TEXT_ID);
}

#[test]
fn capture_badge_default_equals_new() {
    // new: `Default` is `new`
    let badge = CapturePairBadge::default();

    assert_eq!(ElementNode::base(&badge).id(), PANEL_ID);
    assert_eq!(badge.tool_tip(), "");
}

#[test]
fn capture_badge_geometry_matches_padding_corner_and_margin() {
    // new: note 9 (padding (6, 4), margin (8, 44, 8, 8), corner radius 4 gives a rounded fill)
    let fake = FakeTextRenderer::new();
    let mut badge = CapturePairBadge::new();
    badge.apply(&OverlayModel {
        capture_pair_visible: true,
        capture_pair_text: "ABCDEFGHIJ".to_owned(),
        font_size: 10.0,
        ..OverlayModel::default()
    });
    let mut dc = RecordingDrawContext::new();

    badge.arrange(&context(&fake, 800.0, 600.0));
    badge.render(&mut dc);

    // 10 characters x 5 = 50, + 12 padding = 62 wide; 15 + 8 = 23 high; centred at the top, 44 down.
    let bounds = badge.bounds();
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (369.0, 44.0, 62.0, 23.0)
    );
    assert_eq!(dc.fills.len(), 1);
    assert!(dc.fills[0].rounded);
    assert_eq!(dc.fills[0].color, palette::overlay());
}

#[test]
fn capture_badge_apply_sets_visibility_text_font() {
    // C#: ShellOverlayBehaviorTests.Model_DrivesVisibilityTextAndFontOfEveryPanel (capture-pair part)
    let mut badge = CapturePairBadge::new();
    badge.apply(&OverlayModel {
        font_size: 14.0,
        ..pair_model()
    });

    assert!(ElementNode::base(&badge).is_visible);
    let text = badge.text().expect("text child");
    assert_eq!(text.text(), "JPEG+RAW");
    assert_eq!(text.style().font_family, "Segoe UI");
    assert_eq!(text.style().font_size_dip, 14.0);
    assert!(!text.style().bold);

    badge.apply(&OverlayModel {
        capture_pair_visible: false,
        ..OverlayModel::default()
    });
    assert!(!ElementNode::base(&badge).is_visible);
}

#[test]
fn capture_badge_apply_writes_every_field_even_when_hidden() {
    // new: note 10 ("applied regardless of visibility")
    let mut badge = CapturePairBadge::new();

    badge.apply(&OverlayModel {
        capture_pair_visible: false,
        capture_pair_text: "JPEG+RAW".to_owned(),
        capture_pair_tool_tip: "Press F8".to_owned(),
        font_size: 13.0,
        ..OverlayModel::default()
    });

    assert!(!ElementNode::base(&badge).is_visible);
    assert_eq!(badge.text().expect("text child").text(), "JPEG+RAW");
    assert_eq!(badge.text().expect("text child").style().font_size_dip, 13.0);
    assert_eq!(badge.tool_tip(), "Press F8");
}

#[test]
fn capture_badge_tool_tip_is_copied_from_the_model() {
    // C#: ShellOverlayBehaviorTests.Tooltips_HiddenOverlaysHaveNone_ExifAndBadgeComeFromTheModel (badge tooltip copy only), T6
    let mut badge = CapturePairBadge::new();
    assert_eq!(badge.tool_tip(), "");

    badge.apply(&pair_model());
    assert_eq!(badge.tool_tip(), "Press F8");

    // Hidden model with a non-empty tooltip: copied, not cleared.
    badge.apply(&OverlayModel {
        capture_pair_visible: false,
        capture_pair_tool_tip: "Still here".to_owned(),
        ..OverlayModel::default()
    });
    assert_eq!(badge.tool_tip(), "Still here");

    // The model supplies the empty string when there is no pair.
    badge.apply(&OverlayModel::default());
    assert_eq!(badge.tool_tip(), "");
}

#[test]
fn badge_find_returns_the_panel_with_the_badge_id() {
    // new: T7 (find returns the inner panel, not the wrapper)
    let badge = CapturePairBadge::new();

    let found = badge.find(PANEL_ID).expect("the panel");
    assert_eq!(found.id(), PANEL_ID);
    assert!(
        found.as_any().downcast_ref::<CapturePairBadge>().is_none(),
        "the delegation returns the inner PanelElement"
    );
    let text = badge.find(TEXT_ID).expect("the text child");
    assert!(text.as_any().downcast_ref::<TextElement>().is_some());
    assert!(badge.find("Missing").is_none());
}

#[test]
fn badge_find_at_takes_the_hit_on_the_panel_and_none_outside() {
    // new: note 14 (delegation: the panel's find_at rules, text is click-through)
    let fake = FakeTextRenderer::new();
    let mut badge = CapturePairBadge::new();
    badge.apply(&pair_model());
    badge.arrange(&context(&fake, 800.0, 600.0));
    let bounds: RectD = badge.bounds();

    let hit = badge
        .find_at(PointD {
            x: bounds.x + 1.0,
            y: bounds.y + 1.0,
        })
        .expect("the panel takes the hit");
    assert_eq!(hit.id(), PANEL_ID);
    assert!(badge.find_at(PointD { x: 1.0, y: 1.0 }).is_none());
}

#[test]
fn badge_release_resources_drops_the_text_layout() {
    // new: note 14 (delegation of release_resources to the text child)
    let fake = FakeTextRenderer::new();
    let mut badge = CapturePairBadge::new();
    badge.apply(&pair_model());
    badge.arrange(&context(&fake, 800.0, 600.0));
    assert_eq!(fake.live(), 1);

    badge.release_resources();

    assert_eq!(fake.live(), 0);
}
