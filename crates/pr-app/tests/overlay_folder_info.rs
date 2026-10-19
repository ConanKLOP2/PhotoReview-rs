//! Red tests (W8-15d, test porter) for `pr_app::overlay::folder_info::FolderInfoPanel`.
//!
//! Port of the folder-info part of `PhotoReview.Shell.Tests/Overlay/{OverlayPartsTests,ShellOverlayBehaviorTests}.cs`
//! (baseline `d92d6aae`) plus F tests (`// new:`) for the defaults, T7 and T11 of `contracts/W8-15d.md`. Fakes are in
//! `overlay_fakes.rs`; the catalog is the built-in English one.
//!
//! Every test builds a `FolderInfoPanel` first, whose `new` is still `todo!()`, so the tests fail until the porter fills
//! `overlay::folder_info` (and `overlay::panel`).

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use overlay_fakes::FakeTextRenderer;
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::overlay::contracts::{OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment};
use pr_app::overlay::folder_info::{AUTOMATION_NAME_KEY, FolderInfoPanel, MAX_WIDTH, PANEL_ID, TEXT_ID};
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::text::TextElement;
use pr_app::services::render::text::TextTrimming;
use pr_l10n::localizer::{built_in_english_localizer, create_pseudo};

fn context(text: &FakeTextRenderer, width: f64, height: f64) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::new(SizeD { width, height }, 1.0, text)
}

#[test]
fn folder_info_constants_are_the_c_values() {
    // new: notes 11 and 13 (the constants are plain values; the panel is built so the test is red until the stub is filled)
    let english = built_in_english_localizer();
    let panel = FolderInfoPanel::new(&english);

    assert_eq!(PANEL_ID, "FolderInfoPanel");
    assert_eq!(TEXT_ID, "FolderInfoText");
    assert_eq!(AUTOMATION_NAME_KEY, "main.folderInfo.automationName");
    assert_eq!(MAX_WIDTH, 640.0);
    assert_eq!(ElementNode::base(&panel).id(), PANEL_ID);
}

#[test]
fn folder_info_defaults_are_right_bottom_capped_with_an_ellipsis_text() {
    // new: note 11
    let english = built_in_english_localizer();
    let panel = FolderInfoPanel::new(&english);

    let base = ElementNode::base(&panel);
    assert_eq!(base.id(), PANEL_ID);
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Right);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Bottom);
    assert_eq!(base.margin, OverlayThickness::uniform(8.0));
    assert_eq!(base.max_width, 640.0);
    assert!(base.is_visible);
    let text = panel.text().expect("child 0 is the text");
    assert_eq!(ElementNode::base(text).id(), TEXT_ID);
    assert_eq!(text.trimming(), TextTrimming::CharacterEllipsis);
}

#[test]
fn folder_info_is_capped_at_640_dip() {
    // C#: OverlayMutationGapTests.FolderInfoPanel_IsCappedAt640Dip_AndTheExifLineSitsTwoDipBelowTheStatusLine (the 640 cap only)
    let english = built_in_english_localizer();
    let fake = FakeTextRenderer::new();
    let mut panel = FolderInfoPanel::new(&english);
    panel.apply(&OverlayModel {
        folder_info_visible: true,
        folder_info_text: "x".repeat(400),
        ..OverlayModel::default()
    });

    panel.arrange(&context(&fake, 1000.0, 700.0));

    // The fake cuts on 6 DIP characters (font size 12): floor((640 - 12) / 6) x 6 + 12 = 636.
    let bounds = panel.bounds();
    assert_eq!(bounds.width, 636.0);
    assert_eq!(bounds.x, 1000.0 - 8.0 - 636.0);
    let last = fake.requests().pop().expect("a layout request");
    assert_eq!(last.2, 628.0, "the text is offered 640 - 12");
    assert_eq!(last.3, TextTrimming::CharacterEllipsis);
}

#[test]
fn folder_info_apply_sets_visibility_text_font() {
    // C#: ShellOverlayBehaviorTests.Model_DrivesVisibilityTextAndFontOfEveryPanel (folder-info part)
    let english = built_in_english_localizer();
    let mut panel = FolderInfoPanel::new(&english);

    panel.apply(&OverlayModel {
        folder_info_visible: true,
        folder_info_text: "\u{1F4C1} Trip".to_owned(),
        font_size: 14.0,
        ..OverlayModel::default()
    });

    assert!(ElementNode::base(&panel).is_visible);
    let text = panel.text().expect("text child");
    assert_eq!(text.text(), "\u{1F4C1} Trip");
    assert_eq!(text.style().font_family, "Segoe UI");
    assert_eq!(text.style().font_size_dip, 14.0);

    panel.apply(&OverlayModel {
        folder_info_visible: false,
        ..OverlayModel::default()
    });
    assert!(!ElementNode::base(&panel).is_visible);
}

#[test]
fn folder_info_apply_does_not_touch_the_exif_or_status_fields() {
    // new: note 12 (only the folder-info fields are read)
    let english = built_in_english_localizer();
    let mut panel = FolderInfoPanel::new(&english);

    panel.apply(&OverlayModel {
        folder_info_visible: true,
        folder_info_text: "Trip".to_owned(),
        status_text: "IMG_1.JPG".to_owned(),
        exif_text: "ISO 100".to_owned(),
        zoom_indicator_text: "150%".to_owned(),
        ..OverlayModel::default()
    });

    assert_eq!(panel.text().expect("text child").text(), "Trip");
}

#[test]
fn folder_info_automation_name_comes_from_the_catalog() {
    // new: notes 11 and 13, T11 (no English literal in the port)
    let english = built_in_english_localizer();
    let panel = FolderInfoPanel::new(&english);

    assert_eq!(
        panel.automation_name(),
        "Current folder and the neighbouring image folders"
    );
    assert_eq!(panel.automation_name(), english.get(AUTOMATION_NAME_KEY));
}

#[test]
fn folder_info_apply_localization_replaces_the_automation_name() {
    // new: note 13 (language switch)
    let english = built_in_english_localizer();
    let pseudo = create_pseudo(&english);
    let mut panel = FolderInfoPanel::new(&english);
    let before = panel.automation_name().to_owned();

    panel.apply_localization(&pseudo);

    assert_ne!(panel.automation_name(), before);
    assert_eq!(panel.automation_name(), pseudo.get(AUTOMATION_NAME_KEY));
}

#[test]
fn folder_info_find_returns_the_panel_with_the_panel_id() {
    // new: T7, note 14 (find returns the inner panel; the text child is found by its id)
    let english = built_in_english_localizer();
    let panel = FolderInfoPanel::new(&english);

    let found = panel.find(PANEL_ID).expect("the panel");
    assert_eq!(found.id(), PANEL_ID);
    assert!(found.as_any().downcast_ref::<FolderInfoPanel>().is_none());
    let text = panel.find(TEXT_ID).expect("the text child");
    assert!(text.as_any().downcast_ref::<TextElement>().is_some());
    assert!(panel.find("Missing").is_none());
}

#[test]
fn folder_info_find_at_takes_the_hit_on_the_panel_and_none_outside() {
    // new: note 14 (delegation)
    let english = built_in_english_localizer();
    let fake = FakeTextRenderer::new();
    let mut panel = FolderInfoPanel::new(&english);
    panel.apply(&OverlayModel {
        folder_info_visible: true,
        folder_info_text: "Trip".to_owned(),
        ..OverlayModel::default()
    });
    panel.arrange(&context(&fake, 800.0, 600.0));
    let bounds = panel.bounds();

    let hit = panel
        .find_at(PointD {
            x: bounds.x + 1.0,
            y: bounds.y + 1.0,
        })
        .expect("the panel takes the hit");
    assert_eq!(hit.id(), PANEL_ID);
    assert!(panel.find_at(PointD { x: 1.0, y: 1.0 }).is_none());
}

#[test]
fn folder_info_hidden_panel_has_a_zero_box() {
    // new: note 14 (delegation: a hidden panel is collapsed)
    let english = built_in_english_localizer();
    let fake = FakeTextRenderer::new();
    let mut panel = FolderInfoPanel::new(&english);
    panel.apply(&OverlayModel {
        folder_info_visible: false,
        folder_info_text: "Trip".to_owned(),
        ..OverlayModel::default()
    });

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_eq!(panel.bounds().width, 0.0);
    assert_eq!(panel.bounds().height, 0.0);
}
