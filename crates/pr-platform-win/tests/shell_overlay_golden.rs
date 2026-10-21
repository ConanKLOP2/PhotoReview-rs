#![cfg(windows)]
//! Native golden twin of W8-15h: the REAL `ShellOverlay` tree (toolbar, status, folder info, zoom indicator, capture-pair badge) with the real DirectWrite
//! text renderer and the Vietnamese catalog (the language of the recording) against `tests/fixtures/golden/overlay-boxes.v1.json` within 2 DIP.
//! Port of `ShellOverlayGoldenTests` (baseline `d92d6aae`, trait Native). Every test needs DirectWrite and the Segoe UI fonts: all are `#[ignore]`d.
//! The fixed strings are the constants of `OverlayGoldenTests` (lines 20 to 25 of the baseline).

#[path = "common/overlay_fakes.rs"]
mod overlay_fakes;
#[path = "common/overlay_layout_fixture.rs"]
mod overlay_layout_fixture;

use std::rc::Rc;
use std::sync::Arc;

use overlay_layout_fixture::{FakeCommands, FakeFrameClock};
use pr_app::input::primitives::{RectD, SizeD};
use pr_app::overlay::model::OverlayModel;
use pr_core::settings::app_settings::AppSettings;
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::keys;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};
use pr_platform_win::render::text::renderer::DWriteTextRenderer;
use pr_platform_win::shell_overlay::overlay::state::{ShellOverlay, ShellOverlayOptions};
use pr_test_support::golden::repo_root;
use serde_json::Value;

const STATUS_LINE: &str = "IMG_0042.JPG  (42/1280)  6000 x 4000  14,2 MB";
const EXIF_LINE: &str = "Canon EOS R5  |  85 mm  f/1.8  1/200 s  ISO 100  |  Taken: 2026-10-01 14:23";
const FOLDER_LINE: &str = "D:\\Photos\\2026\\Trip to Da Lat  -  1280 images";
const ZOOM_LINE: &str = "100 %";
const CAPTURE_PAIR_LINE: &str = "RAW + JPG";

const TOLERANCE: f64 = 2.0;
const PANELS: [&str; 5] = [
    "ToolbarPanel",
    "StatusPanel",
    "FolderInfoPanel",
    "ZoomIndicatorPanel",
    "CapturePairBadge",
];

struct GoldenBox {
    name: String,
    window: SizeD,
    dpi: f64,
    font: f64,
    panel: String,
    rect: RectD,
}

fn golden_boxes() -> Vec<GoldenBox> {
    let path = repo_root()
        .join("tests")
        .join("fixtures")
        .join("golden")
        .join("overlay-boxes.v1.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).expect("valid golden json");
    let number = |item: &Value, key: &str| {
        item[key]
            .as_f64()
            .unwrap_or_else(|| panic!("no number '{key}': {item}"))
    };
    doc["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| GoldenBox {
            name: item["name"].as_str().expect("name").to_owned(),
            window: SizeD {
                width: number(item, "windowWidth"),
                height: number(item, "windowHeight"),
            },
            dpi: number(item, "dpiScale"),
            font: number(item, "overlayFontSize"),
            panel: item["panel"].as_str().expect("panel").to_owned(),
            rect: RectD {
                x: number(item, "x"),
                y: number(item, "y"),
                width: number(item, "width"),
                height: number(item, "height"),
            },
        })
        .collect()
}

/// The English catalog plus the shipped `assets/languages/vi.json` (the language of the recording).
fn vietnamese() -> Arc<Localizer> {
    let path = repo_root().join("assets").join("languages").join("vi.json");
    let text = std::fs::read_to_string(&path).expect("assets/languages/vi.json");
    let mut warnings = Vec::new();
    let vi = LanguageCatalog::try_parse(&text, &path.to_string_lossy(), &mut warnings).expect("valid vi.json");
    Arc::new(Localizer::create(built_in_english(), &[vi], &[]))
}

fn recorder_model(font: f64) -> OverlayModel {
    OverlayModel {
        has_images: true,
        status_panel_visible: true,
        file_info_visible: true,
        status_text: STATUS_LINE.to_owned(),
        font_size: font,
        exif_font_size: font - 1.0,
        exif_visible: true,
        exif_text: EXIF_LINE.to_owned(),
        folder_info_visible: true,
        folder_info_text: FOLDER_LINE.to_owned(),
        zoom_indicator_visible: true,
        zoom_indicator_text: ZOOM_LINE.to_owned(),
        capture_pair_visible: true,
        capture_pair_text: CAPTURE_PAIR_LINE.to_owned(),
        ..OverlayModel::default()
    }
}

fn build(locale: &str, texts: Arc<Localizer>, font: f64) -> Rc<ShellOverlay> {
    let renderer = DWriteTextRenderer::new(Some(locale), "", 512).expect("DirectWrite factory");
    let overlay = ShellOverlay::new(ShellOverlayOptions {
        text: Rc::new(renderer),
        text_renderer_id: 1,
        texts,
        clock: Rc::new(FakeFrameClock::default()),
        settings: Box::new(|| Arc::new(AppSettings::default())),
        commands: Box::new(FakeCommands { calls: Rc::default() }),
        request_render: Box::new(|| {}),
        is_window_active: Box::new(|| true),
        menus: None,
        tooltips: None,
        client_origin_on_screen_dip: None,
        on_command_failed: None,
        on_menu_failed: None,
        timestamp: Some(Box::new(|| 0)),
        ticks_per_second: Some(1000),
        animator_ticks_per_second: Some(1000),
    });
    overlay.update(recorder_model(font));
    overlay
}

fn delta(actual: RectD, expected: RectD) -> f64 {
    (actual.x - expected.x)
        .abs()
        .max((actual.y - expected.y).abs())
        .max((actual.width - expected.width).abs())
        .max((actual.height - expected.height).abs())
}

// C#: ShellOverlayGoldenTests.SixShellPanelsMatchTheWpfGoldenWithinTwoDips
#[test]
#[ignore = "native: needs DirectWrite and Segoe UI"]
fn shell_panels_match_the_wpf_golden_within_two_dips() {
    let boxes = golden_boxes();
    assert_eq!(boxes.len(), 72);
    let vi = vietnamese();
    let mut checked = 0;
    let mut worst: f64 = 0.0;
    let mut failures = Vec::new();
    // One overlay per (window, dpi, font) group, like the C# GroupBy.
    let mut groups: Vec<(SizeD, f64, f64)> = Vec::new();
    for b in &boxes {
        let key = (b.window, b.dpi, b.font);
        if !groups.contains(&key) {
            groups.push(key);
        }
    }
    for (window, dpi, font) in groups {
        let overlay = build("vi-VN", Arc::clone(&vi), font);
        overlay.arrange(window, dpi);
        for b in boxes
            .iter()
            .filter(|b| b.window == window && b.dpi == dpi && b.font == font && PANELS.contains(&b.panel.as_str()))
        {
            checked += 1;
            let actual = overlay.element_bounds(&b.panel).expect("panel exists");
            let d = delta(actual, b.rect);
            worst = worst.max(d);
            if d > TOLERANCE {
                failures.push(format!("{}: golden {:?} got {:?} delta {d:.2}", b.name, b.rect, actual));
            }
        }
    }
    println!("G-OVL (shell): {checked} cases, worst delta {worst:.3} DIP (tolerance {TOLERANCE})");
    assert_eq!(checked, 60);
    assert!(
        failures.is_empty(),
        "worst delta {worst:.2} DIP; {}",
        failures.join("\n")
    );
}

// C#: ShellOverlayGoldenTests.ToolbarBoxMatchesTheGolden_Is210x40_AtEveryWindowSizeAndDpi
#[test]
#[ignore = "native: needs DirectWrite and Segoe UI"]
fn toolbar_box_matches_the_golden_at_every_window_size_and_dpi() {
    let toolbar: Vec<GoldenBox> = golden_boxes()
        .into_iter()
        .filter(|b| b.panel == "ToolbarPanel")
        .collect();
    assert_eq!(toolbar.len(), 12);
    let vi = vietnamese();
    for b in &toolbar {
        let overlay = build("vi-VN", Arc::clone(&vi), b.font);
        overlay.arrange(b.window, b.dpi);
        let actual = overlay.element_bounds("ToolbarPanel").expect("toolbar");
        assert!((actual.x - b.rect.x).abs() <= 0.001, "{}: x", b.name);
        assert!((actual.y - b.rect.y).abs() <= 0.001, "{}: y", b.name);
        assert!(
            (actual.width - b.rect.width).abs() <= TOLERANCE,
            "{}: width {:.2} vs {:.2}",
            b.name,
            actual.width,
            b.rect.width
        );
        assert!(
            (actual.height - b.rect.height).abs() <= TOLERANCE,
            "{}: height {:.2} vs {:.2}",
            b.name,
            actual.height,
            b.rect.height
        );
    }
}

// C#: ShellOverlayGoldenTests.ToolbarKeepsItsBox_WhenOnlyTheDpiChanges
#[test]
#[ignore = "native: needs DirectWrite and Segoe UI"]
fn toolbar_keeps_its_box_when_only_the_dpi_changes() {
    let overlay = build("vi-VN", vietnamese(), 12.0);
    let client = SizeD {
        width: 800.0,
        height: 600.0,
    };
    overlay.arrange(client, 1.0);
    let at_one = overlay.element_bounds("ToolbarPanel").expect("toolbar");
    overlay.arrange(client, 1.5);
    assert_eq!(overlay.element_bounds("ToolbarPanel"), Some(at_one));
}

// C#: ShellOverlayGoldenTests.LanguageSwitch_ChangesToolbarTextTooltipsAndWidth_WithoutRebuilding
#[test]
#[ignore = "native: needs DirectWrite and Segoe UI"]
fn language_switch_changes_toolbar_text_tooltips_and_width_without_rebuilding() {
    let english_texts = built_in_english_localizer();
    let vi = vietnamese();
    let overlay = build("en-US", Arc::clone(&english_texts), 12.0);
    let client = SizeD {
        width: 800.0,
        height: 600.0,
    };

    overlay.apply_localization(&english_texts);
    let english = overlay.with_toolbar(|t| t.fit().content().to_owned());
    overlay.arrange(client, 1.0);
    let english_width = overlay.element_bounds("ToolbarPanel").expect("toolbar").width;
    assert_eq!(english, english_texts.get(keys::MAIN_TOOLBAR_FIT));
    assert_eq!(
        overlay.with_toolbar(|t| t.fit().tool_tip.clone()),
        english_texts.get(keys::MAIN_TOOLBAR_FIT_TOOL_TIP)
    );
    assert_eq!(
        overlay.with_status(|s| s.view_list().content().to_owned()),
        english_texts.get(keys::MAIN_SKIPPED_VIEW_LIST)
    );

    overlay.apply_localization(&vi);
    let vietnamese_fit = overlay.with_toolbar(|t| t.fit().content().to_owned());
    assert_eq!(vietnamese_fit, vi.get(keys::MAIN_TOOLBAR_FIT));
    assert_ne!(english, vietnamese_fit);
    assert_eq!(
        overlay.with_toolbar(|t| t.open_folder().tool_tip.clone()),
        vi.get(keys::MAIN_TOOLBAR_OPEN_FOLDER_TOOL_TIP)
    );
    assert_eq!(
        overlay.with_toolbar(|t| t.tools().automation_name.clone()),
        vi.get(keys::MAIN_TOOLBAR_TOOLS_AUTOMATION_NAME)
    );
    assert_eq!(
        overlay.with_folder_info(|p| p.automation_name().to_owned()),
        vi.get(keys::MAIN_FOLDER_INFO_AUTOMATION_NAME)
    );
    assert_eq!(
        overlay.with_zoom_indicator(|p| p.automation_name().to_owned()),
        vi.get(keys::MAIN_ZOOM_INDICATOR_AUTOMATION_NAME)
    );
    assert_eq!(
        overlay.with_status(|s| s.view_list().automation_name.clone()),
        vi.get(keys::MAIN_SKIPPED_VIEW_LIST_AUTOMATION_NAME)
    );
    overlay.arrange(client, 1.0);
    assert_ne!(
        overlay.element_bounds("ToolbarPanel").expect("toolbar").width,
        english_width
    );
}
