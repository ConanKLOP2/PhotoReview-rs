//! F tests (W8-15d) for `pr_app::overlay::folder_info::FolderInfoPanel` (C# `FolderInfoPanel`, baseline `d92d6aae`).
//!
//! Ports of the folder-info parts of `OverlayMutationGapTests` and `ShellOverlayBehaviorTests` plus `// new:` tests for the contract
//! traps T7, T11 and the localisation rule (note 13).

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use overlay_fakes::FakeTextRenderer;
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::overlay::contracts::{OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment};
use pr_app::overlay::folder_info::{AUTOMATION_NAME_KEY, FolderInfoPanel, MAX_WIDTH, PANEL_ID, TEXT_ID};
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::panel::PanelElement;
use pr_app::services::render::text::{TextStyle, TextTrimming};
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};

fn context(text: &FakeTextRenderer, width: f64, height: f64) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::new(SizeD { width, height }, 1.0, text)
}

fn english() -> Localizer {
    Localizer::create(built_in_english(), &[], &[])
}

/// A second language that overrides only the folder-info automation name.
fn other_language() -> Localizer {
    let json = r#"{ "_meta": { "code": "xx", "name": "Test", "nativeName": "Test", "plural": "none", "authors": ["t"], "formatVersion": 1 },
        "main.folderInfo.automationName": "Other name" }"#;
    let mut warnings = Vec::new();
    let catalog = LanguageCatalog::try_parse(json, "xx.json", &mut warnings).expect("valid test catalog");
    Localizer::create(built_in_english(), &[catalog], &[])
}

fn model(visible: bool, text: &str, font_size: f64) -> OverlayModel {
    OverlayModel {
        folder_info_visible: visible,
        folder_info_text: text.to_owned(),
        font_size,
        ..OverlayModel::default()
    }
}

#[test]
fn constants_and_defaults() {
    // new: contract note 11
    assert_eq!(PANEL_ID, "FolderInfoPanel");
    assert_eq!(TEXT_ID, "FolderInfoText");
    assert_eq!(AUTOMATION_NAME_KEY, "main.folderInfo.automationName");
    assert_eq!(MAX_WIDTH, 640.0);
    let panel = FolderInfoPanel::new(&english());
    let base = panel.base();
    assert_eq!(base.id(), PANEL_ID);
    assert!(base.is_visible);
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Right);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Bottom);
    assert_eq!(base.margin, OverlayThickness::uniform(8.0));
    assert_eq!(base.max_width, 640.0);
    let text = panel.text().expect("text child");
    assert_eq!(text.base().id(), TEXT_ID);
    assert_eq!(text.trimming(), TextTrimming::CharacterEllipsis);
}

#[test]
fn folder_info_is_capped_at_640_dip() {
    // new: twin of OverlayMutationGapTests.FolderInfoPanel_IsCappedAt640Dip_... (cap only, fake text renderer)
    let fake = FakeTextRenderer::new();
    let mut panel = FolderInfoPanel::new(&english());
    // Default font size 12: the fake measures 6 DIP per character.
    panel.apply(&model(true, &"x".repeat(400), 12.0));

    panel.arrange(&context(&fake, 1000.0, 700.0));

    // floor((640 - 12) / 6) x 6 + 12 = 636; the text is cut to the remainder, the panel stays within the cap.
    assert_eq!(panel.bounds().width, 636.0);
    assert_eq!(panel.bounds().x, 1000.0 - 8.0 - 636.0);
    let request = fake.requests().last().cloned().expect("a layout request");
    assert_eq!(request.2, 628.0);
    assert_eq!(request.3, TextTrimming::CharacterEllipsis);
}

#[test]
fn short_text_is_not_capped_and_sits_bottom_right() {
    // new: contract note 11 (anchored right/bottom with margin 8 and padding 6/4)
    let fake = FakeTextRenderer::new();
    let mut panel = FolderInfoPanel::new(&english());
    panel.apply(&model(true, "ABCDEFGHIJ", 10.0));

    panel.arrange(&context(&fake, 800.0, 600.0));

    let b = panel.bounds();
    assert_eq!((b.width, b.height), (62.0, 23.0));
    assert_eq!((b.x, b.y), (800.0 - 8.0 - 62.0, 600.0 - 8.0 - 23.0));
}

#[test]
fn folder_info_apply_sets_visibility_text_font() {
    // C#: ShellOverlayBehaviorTests.Model_DrivesVisibilityTextAndFontOfEveryPanel (folder part)
    let mut panel = FolderInfoPanel::new(&english());
    panel.apply(&model(true, "\u{1F4C1} Trip", 14.0));

    assert!(panel.is_visible());
    let text = panel.text().expect("text");
    assert_eq!(text.text(), "\u{1F4C1} Trip");
    assert_eq!(
        *text.style(),
        TextStyle {
            font_family: "Segoe UI".to_owned(),
            font_size_dip: 14.0,
            bold: false
        }
    );

    panel.apply(&model(false, "\u{1F4C1} Trip", 14.0));
    assert!(!panel.is_visible());
}

#[test]
fn apply_does_not_touch_trimming_or_automation_name() {
    // new: contract notes 12 and 13
    let mut panel = FolderInfoPanel::new(&english());
    let name = panel.automation_name().to_owned();
    panel.apply(&model(true, "x", 20.0));
    assert_eq!(panel.text().expect("text").trimming(), TextTrimming::CharacterEllipsis);
    assert_eq!(panel.automation_name(), name);
}

#[test]
fn automation_name_comes_from_the_catalog_and_follows_a_language_switch() {
    // new: contract note 13 and trap T11
    let texts = built_in_english_localizer();
    let mut panel = FolderInfoPanel::new(&texts);
    assert_eq!(
        panel.automation_name(),
        "Current folder and the neighbouring image folders"
    );

    panel.apply_localization(&other_language());
    assert_eq!(panel.automation_name(), "Other name");

    panel.apply_localization(&texts);
    assert_eq!(
        panel.automation_name(),
        "Current folder and the neighbouring image folders"
    );
}

#[test]
fn hidden_panel_has_no_size_and_no_hit_and_visible_hit_returns_the_inner_panel() {
    // new: contract note 14 and trap T7
    let fake = FakeTextRenderer::new();
    let mut panel = FolderInfoPanel::new(&english());
    panel.apply(&model(false, "ABCDEFGHIJ", 10.0));
    panel.arrange(&context(&fake, 800.0, 600.0));
    assert_eq!(panel.bounds().width, 0.0);
    assert!(panel.find_at(PointD { x: 790.0, y: 590.0 }).is_none());

    panel.apply(&model(true, "ABCDEFGHIJ", 10.0));
    panel.arrange(&context(&fake, 800.0, 600.0));
    let hit = panel.find_at(PointD { x: 790.0, y: 590.0 }).expect("hit");
    assert_eq!(hit.base().id(), PANEL_ID);
    assert!(hit.as_any().downcast_ref::<PanelElement>().is_some());
    assert!(panel.find(TEXT_ID).is_some());
}
