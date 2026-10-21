//! F tests (W8-15e) for the overlay panels `status`, `toolbar`, `tools_menu` and `zoom_indicator` (baseline `d92d6aae`).
//!
//! Ports of `OverlayPartsTests` (`ToolsMenu_*`), `OverlayMutationGapTests` (`Toolbar_BoxAndButtonSpacing...`, the EXIF half of
//! `FolderInfoPanel_IsCappedAt640Dip...`) plus `// new:` tests for the contract traps T1..T14. The fixture test of the optional
//! contract open question O8 (`overlay-boxes.v1.json`, +/- 2 DIP) is deferred: `tests/fixtures/golden/overlay-boxes.v1.json`
//! is checked by the native overlay run (W8-15j / W8-20b).
//! The fakes are in `overlay_fakes.rs` (width = characters x size / 2, height = size x 1.5).

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use overlay_fakes::{FakeTextRenderer, RecordingDrawContext};
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::overlay::button::ButtonElement;
use pr_app::overlay::contracts::{OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment};
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::palette;
use pr_app::overlay::panel::OverlayOrientation;
use pr_app::overlay::status::{
    EXIF_TEXT_ID, PANEL_ID as STATUS_PANEL_ID, SKIPPED_ROW_ID, SKIPPED_VIEW_LIST_ID, SKIPPED_WARNING_ID,
    STATUS_TEXT_ID, SkippedRow, StatusPanel,
};
use pr_app::overlay::toolbar::{
    FIT_ID, OPEN_FOLDER_GLYPH, OPEN_FOLDER_ID, PANEL_ID as TOOLBAR_PANEL_ID, SETTINGS_GLYPH, SETTINGS_ID, TOOLS_GLYPH,
    TOOLS_ID, ToolbarPanel,
};
use pr_app::overlay::tools_menu::{
    self, BENCHMARK, CLEAR_CACHE, DIAGNOSTICS, OverlayCommands, RECOVERY, REMOVE_NUMBERED_DUPLICATES,
    REMOVE_ORIGINAL_DUPLICATES, TOOLS_IDS,
};
use pr_app::overlay::zoom_indicator::{PANEL_ID as ZOOM_PANEL_ID, TEXT_ID as ZOOM_TEXT_ID, ZoomIndicatorPanel};
use pr_app::services::render::text::{TextStyle, TextTrimming};
use pr_l10n::catalog::LanguageCatalog;
use pr_l10n::localizer::{Localizer, built_in_english, built_in_english_localizer};

fn english() -> std::sync::Arc<Localizer> {
    built_in_english_localizer()
}

/// A second language that overrides the strings of the toolbar, the skipped row and the zoom indicator.
fn other_language() -> Localizer {
    let json = r#"{ "_meta": { "code": "xx", "name": "Test", "nativeName": "Test", "plural": "none", "authors": ["t"], "formatVersion": 1 },
        "main.toolbar.fit": "F2", "main.toolbar.fit.toolTip": "F3", "main.toolbar.fit.automationName": "F4",
        "main.toolbar.openFolder.toolTip": "O1", "main.toolbar.openFolder.automationName": "O2",
        "main.toolbar.settings.toolTip": "S1", "main.toolbar.settings.automationName": "S2",
        "main.toolbar.tools.toolTip": "T1", "main.toolbar.tools.automationName": "T2",
        "main.skipped.viewList": "V1", "main.skipped.viewList.automationName": "V2",
        "main.zoomIndicator.automationName": "Z1" }"#;
    let mut warnings = Vec::new();
    let catalog = LanguageCatalog::try_parse(json, "xx.json", &mut warnings).expect("valid test catalog");
    Localizer::create(built_in_english(), &[catalog], &[])
}

fn context(text: &FakeTextRenderer) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::new(
        SizeD {
            width: 1000.0,
            height: 700.0,
        },
        1.0,
        text,
    )
}

fn style(size: f64) -> TextStyle {
    TextStyle {
        font_family: "Segoe UI".to_owned(),
        font_size_dip: size,
        bold: false,
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-3, "{actual} != {expected}");
}

// ---------------------------------------------------------------- ToolsMenu

#[derive(Default)]
struct FakeCommands {
    calls: Vec<String>,
}

impl OverlayCommands for FakeCommands {
    fn open_folder(&mut self) {
        self.calls.push("open_folder".to_owned());
    }

    fn show_settings(&mut self) {
        self.calls.push("settings".to_owned());
    }

    fn fit(&mut self) {
        self.calls.push("fit".to_owned());
    }

    fn show_skipped_files(&mut self) {
        self.calls.push("skipped".to_owned());
    }

    fn show_recovery(&mut self) {
        self.calls.push("recovery".to_owned());
    }

    fn show_diagnostics(&mut self) {
        self.calls.push("diagnostics".to_owned());
    }

    fn show_benchmark(&mut self) {
        self.calls.push("benchmark".to_owned());
    }

    fn clear_cache(&mut self) {
        self.calls.push("clear_cache".to_owned());
    }

    fn remove_duplicates(&mut self, numbered: bool) {
        self.calls.push(format!("remove_duplicates({numbered})"));
    }
}

#[test]
fn tools_menu_build_gives_six_tools_items_in_xaml_order() {
    // C#: OverlayPartsTests.ToolsMenu_BuildGivesTheSixToolsItemsInXamlOrder
    let items = tools_menu::build(true);
    let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "Tools.Recovery",
            "Tools.Diagnostics",
            "Tools.Benchmark",
            "Tools.ClearCache",
            "Tools.RemoveNumberedDuplicates",
            "Tools.RemoveOriginalDuplicates"
        ]
    );
    assert_eq!(ids, TOOLS_IDS);
    assert!(items.iter().all(|item| item.is_enabled));
}

#[test]
fn tools_menu_build_disables_the_two_duplicate_items_without_images() {
    // new: contract ToolsMenu 1
    let items = tools_menu::build(false);
    let enabled: Vec<(&str, bool)> = items.iter().map(|item| (item.id.as_str(), item.is_enabled)).collect();
    assert_eq!(
        enabled,
        [
            (RECOVERY, true),
            (DIAGNOSTICS, true),
            (BENCHMARK, true),
            (CLEAR_CACHE, true),
            (REMOVE_NUMBERED_DUPLICATES, false),
            (REMOVE_ORIGINAL_DUPLICATES, false),
        ]
    );
}

#[test]
fn tools_menu_execute_ignores_unknown_and_none_ids() {
    // C#: OverlayPartsTests.ToolsMenu_ExecuteIgnoresUnknownAndNullIds
    // DIVERGENCE: the C# `ArgumentNullException` for a null `commands` has no Rust form (`&mut dyn OverlayCommands` cannot be null).
    let mut commands = FakeCommands::default();
    assert!(!tools_menu::execute(Some("Tools.Nope"), &mut commands, true));
    assert!(!tools_menu::execute(None, &mut commands, true));
    assert!(commands.calls.is_empty());
}

#[test]
fn tools_menu_execute_routes_each_tools_id_to_its_command() {
    // new: contract ToolsMenu 2
    let cases: [(&str, &str); 6] = [
        (RECOVERY, "recovery"),
        (DIAGNOSTICS, "diagnostics"),
        (BENCHMARK, "benchmark"),
        (CLEAR_CACHE, "clear_cache"),
        (REMOVE_NUMBERED_DUPLICATES, "remove_duplicates(true)"),
        (REMOVE_ORIGINAL_DUPLICATES, "remove_duplicates(false)"),
    ];
    for (id, expected) in cases {
        let mut commands = FakeCommands::default();
        assert!(tools_menu::execute(Some(id), &mut commands, true), "{id}");
        assert_eq!(commands.calls, [expected], "{id}");
    }
    assert_eq!(RECOVERY, "Tools.Recovery");
    assert_eq!(DIAGNOSTICS, "Tools.Diagnostics");
    assert_eq!(BENCHMARK, "Tools.Benchmark");
    assert_eq!(CLEAR_CACHE, "Tools.ClearCache");
    assert_eq!(REMOVE_NUMBERED_DUPLICATES, "Tools.RemoveNumberedDuplicates");
    assert_eq!(REMOVE_ORIGINAL_DUPLICATES, "Tools.RemoveOriginalDuplicates");
}

#[test]
fn tools_menu_execute_duplicates_without_images_is_true_and_calls_nothing() {
    // new: trap T10
    let mut commands = FakeCommands::default();
    assert!(tools_menu::execute(
        Some(REMOVE_NUMBERED_DUPLICATES),
        &mut commands,
        false
    ));
    assert!(tools_menu::execute(
        Some(REMOVE_ORIGINAL_DUPLICATES),
        &mut commands,
        false
    ));
    assert!(commands.calls.is_empty());
    // The other four do not depend on images.
    for id in [RECOVERY, DIAGNOSTICS, BENCHMARK, CLEAR_CACHE] {
        assert!(tools_menu::execute(Some(id), &mut commands, false));
    }
    assert_eq!(commands.calls, ["recovery", "diagnostics", "benchmark", "clear_cache"]);
}

#[test]
fn tools_menu_execute_ids_are_ordinal() {
    // new: trap T9
    let mut commands = FakeCommands::default();
    for id in [
        "tools.recovery",
        "Tools.Recovery ",
        " Tools.Recovery",
        "TOOLS.RECOVERY",
        "",
        "Recovery",
    ] {
        assert!(!tools_menu::execute(Some(id), &mut commands, true), "{id:?}");
    }
    assert!(commands.calls.is_empty());
}

// ---------------------------------------------------------------- ToolbarPanel

#[test]
fn toolbar_box_and_button_spacing_match_the_xaml_exactly() {
    // C#: OverlayMutationGapTests.Toolbar_BoxAndButtonSpacingMatchTheXamlExactly
    let fake = FakeTextRenderer::new();
    let mut bar = ToolbarPanel::new(&english());

    bar.arrange(&context(&fake));

    let b = bar.bounds();
    assert_close(b.x, 8.0);
    assert_close(b.y, 8.0);
    assert_close(b.height, 40.0); // 32 (the tools button) + padding 4 + 4
    let open = bar.open_folder().bounds();
    let settings = bar.settings().bounds();
    let fit = bar.fit().bounds();
    let tools = bar.tools().bounds();
    assert_close(open.x, 14.0); // margin 8 + padding 6
    assert_close(tools.y, 12.0);
    assert_close(tools.height, 32.0);
    assert_close(tools.width, 34.0);
    assert_close(settings.x - (open.x + open.width), 5.0);
    assert_close(fit.x - (settings.x + settings.width), 5.0);
    assert_close(tools.x - (fit.x + fit.width), 5.0);
    assert_close(b.x + b.width - 6.0, tools.x + tools.width); // right padding 6
}

#[test]
fn toolbar_defaults_ids_and_tab_order() {
    // new: contract Toolbar 1, 2, 5, 6 and trap T5
    let bar = ToolbarPanel::new(&english());
    assert_eq!(bar.base().id(), TOOLBAR_PANEL_ID);
    assert_eq!(TOOLBAR_PANEL_ID, "ToolbarPanel");
    assert_eq!(bar.base().horizontal_alignment, OverlayHorizontalAlignment::Left);
    assert_eq!(bar.base().vertical_alignment, OverlayVerticalAlignment::Top);
    assert_eq!(bar.base().margin, OverlayThickness::uniform(8.0));
    assert!(bar.base().is_visible);

    let ids: Vec<&str> = bar.buttons().iter().map(|b| b.base().id()).collect();
    assert_eq!(ids, [OPEN_FOLDER_ID, SETTINGS_ID, FIT_ID, TOOLS_ID]);
    assert_eq!(ids, ["OpenFolderButton", "SettingsButton", "FitButton", "ToolsButton"]);

    let [open, settings, fit, tools] = bar.buttons();
    assert_eq!(open.base().margin, OverlayThickness::ZERO);
    for button in [settings, fit, tools] {
        assert_eq!(
            button.base().margin,
            OverlayThickness::new(5.0, 0.0, 0.0, 0.0),
            "{}",
            button.base().id()
        );
    }
    assert_eq!(open.padding, OverlayThickness::symmetric(8.0, 2.0));
    assert_eq!(settings.padding, OverlayThickness::symmetric(8.0, 4.0));
    assert_eq!(fit.padding, OverlayThickness::symmetric(8.0, 4.0));
    assert_eq!(tools.padding, OverlayThickness::ZERO);
    assert_eq!(*open.style(), style(16.0));
    assert_eq!(*settings.style(), style(12.0));
    assert_eq!(*fit.style(), style(12.0));
    assert_eq!(*tools.style(), style(18.0));
    assert!(open.fixed_width.is_nan() && open.fixed_height.is_nan());
    assert!(settings.fixed_width.is_nan() && fit.fixed_height.is_nan());
    assert_eq!((tools.fixed_width, tools.fixed_height), (34.0, 32.0));
    for button in bar.buttons() {
        assert_eq!(
            button.background,
            palette::get("Dark.Input").expect("Dark.Input"),
            "{}",
            button.base().id()
        );
        assert_eq!(
            button.border_color,
            palette::get("Dark.BorderToolbar").expect("Dark.BorderToolbar")
        );
        assert_eq!(button.corner_radius, 0.0);
    }
    assert_eq!(open.content(), OPEN_FOLDER_GLYPH);
    assert_eq!(settings.content(), SETTINGS_GLYPH);
    assert_eq!(tools.content(), TOOLS_GLYPH);
    assert_eq!(bar.open_folder().base().id(), OPEN_FOLDER_ID);
    assert_eq!(bar.settings().base().id(), SETTINGS_ID);
    assert_eq!(bar.fit().base().id(), FIT_ID);
    assert_eq!(bar.tools().base().id(), TOOLS_ID);
}

#[test]
fn toolbar_glyphs_are_the_csharp_code_points() {
    // new: trap T1
    assert_eq!(OPEN_FOLDER_GLYPH.chars().collect::<Vec<_>>(), ['\u{1F4C2}']);
    assert_eq!(SETTINGS_GLYPH.chars().collect::<Vec<_>>(), ['\u{2699}']);
    assert_eq!(TOOLS_GLYPH.chars().collect::<Vec<_>>(), ['\u{22EE}']);
    assert_eq!(SETTINGS_GLYPH.as_bytes(), [0xE2, 0x9A, 0x99]);
    assert_eq!(TOOLS_GLYPH.as_bytes(), [0xE2, 0x8B, 0xAE]);
}

#[test]
fn toolbar_horizontal_orientation_places_buttons_in_tab_order() {
    // new: contract Toolbar 1 and 6 (child order)
    let fake = FakeTextRenderer::new();
    let mut bar = ToolbarPanel::new(&english());
    bar.arrange(&context(&fake));
    let xs: Vec<f64> = bar.buttons().iter().map(|b| b.bounds().x).collect();
    assert!(xs.windows(2).all(|pair| pair[0] < pair[1]), "{xs:?}");
    let mut dc = RecordingDrawContext::new();
    bar.render(&mut dc);
    let drawn: Vec<&str> = dc.texts.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(drawn, [OPEN_FOLDER_GLYPH, SETTINGS_GLYPH, "Fit", TOOLS_GLYPH]);
    // The orientation is a property of the style, not of the callers.
    assert_eq!(OverlayOrientation::Horizontal as i32, 1);
}

#[test]
fn toolbar_move_focus_forward_from_none_starts_at_open_folder() {
    // new: trap T3
    let mut bar = ToolbarPanel::new(&english());
    assert_eq!(bar.focused_index(), None);
    assert!(!bar.has_keyboard_focus());
    assert!(bar.focused_button().is_none());

    assert!(bar.move_focus(true));

    assert_eq!(bar.focused_index(), Some(0));
    assert!(bar.has_keyboard_focus());
    assert_eq!(bar.focused_button().map(|b| b.base().id()), Some(OPEN_FOLDER_ID));
    assert!(bar.move_focus(true));
    assert_eq!(bar.focused_index(), Some(1));
    assert!(bar.move_focus(true));
    assert_eq!(bar.focused_button().map(|b| b.base().id()), Some(FIT_ID));
    assert!(bar.move_focus(true));
    assert_eq!(bar.focused_button().map(|b| b.base().id()), Some(TOOLS_ID));
}

#[test]
fn toolbar_move_focus_backward_from_none_starts_at_tools() {
    // new: trap T3
    let mut bar = ToolbarPanel::new(&english());

    assert!(bar.move_focus(false));

    assert_eq!(bar.focused_index(), Some(3));
    assert_eq!(bar.focused_button().map(|b| b.base().id()), Some(TOOLS_ID));
    assert!(bar.move_focus(false));
    assert_eq!(bar.focused_index(), Some(2));
    assert!(bar.move_focus(false));
    assert!(bar.move_focus(false));
    assert_eq!(bar.focused_index(), Some(0));
}

#[test]
fn toolbar_move_focus_does_not_wrap() {
    // new: trap T3
    let mut bar = ToolbarPanel::new(&english());
    bar.move_focus(false); // tools
    assert!(!bar.move_focus(true));
    assert_eq!(bar.focused_index(), None);
    assert!(!bar.has_keyboard_focus());
    assert!(bar.buttons().iter().all(|b| !b.is_focused));

    bar.move_focus(true); // open folder
    assert!(!bar.move_focus(false));
    assert_eq!(bar.focused_index(), None);
    assert!(bar.focused_button().is_none());
    // From "no button" the next press starts again at an end.
    assert!(bar.move_focus(true));
    assert_eq!(bar.focused_index(), Some(0));
}

#[test]
fn toolbar_focus_mirrors_is_focused_on_exactly_one_button() {
    // new: trap T4
    let mut bar = ToolbarPanel::new(&english());
    assert!(bar.buttons().iter().all(|b| !b.is_focused));
    for step in 0..4 {
        bar.move_focus(true);
        let focused: Vec<usize> = bar
            .buttons()
            .iter()
            .enumerate()
            .filter(|(_, b)| b.is_focused)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(focused, [step]);
        assert_eq!(bar.focused_index(), Some(step));
    }
    bar.move_focus(false);
    bar.clear_focus();
    assert_eq!(bar.focused_index(), None);
    assert!(bar.buttons().iter().all(|b| !b.is_focused));
    // buttons_mut writes the same buttons the getters read.
    bar.buttons_mut()[2].is_hot = true;
    assert!(bar.fit().is_hot);
}

#[test]
fn toolbar_apply_localization_sets_fit_text_tooltip_and_automation() {
    // new: contract Toolbar 3 (English catalog)
    let bar = ToolbarPanel::new(&english());
    assert_eq!(bar.fit().content(), "Fit");
    assert_eq!(
        bar.fit().tool_tip,
        "Click Fit, press F, or double-click the image to fit it to the window"
    );
    assert_eq!(
        bar.fit().automation_name,
        "Fit the image to the window (double-click the image)"
    );
    assert_eq!(bar.open_folder().tool_tip, "Open image folder");
    assert_eq!(bar.open_folder().automation_name, "Open image folder");
    assert_eq!(bar.settings().tool_tip, "Settings");
    assert_eq!(bar.settings().automation_name, "Open settings");
    assert_eq!(bar.tools().tool_tip, "Extra tools");
    assert_eq!(bar.tools().automation_name, "Open extra tools");
}

#[test]
fn toolbar_language_switch_replaces_texts_and_keeps_the_glyphs() {
    // new: contract Toolbar 3 (glyph contents never change with language)
    let mut bar = ToolbarPanel::new(&english());
    bar.apply_localization(&other_language());
    assert_eq!(bar.fit().content(), "F2");
    assert_eq!(bar.fit().tool_tip, "F3");
    assert_eq!(bar.fit().automation_name, "F4");
    assert_eq!(
        (
            bar.open_folder().tool_tip.as_str(),
            bar.open_folder().automation_name.as_str()
        ),
        ("O1", "O2")
    );
    assert_eq!(
        (
            bar.settings().tool_tip.as_str(),
            bar.settings().automation_name.as_str()
        ),
        ("S1", "S2")
    );
    assert_eq!(
        (bar.tools().tool_tip.as_str(), bar.tools().automation_name.as_str()),
        ("T1", "T2")
    );
    assert_eq!(bar.open_folder().content(), OPEN_FOLDER_GLYPH);
    assert_eq!(bar.settings().content(), SETTINGS_GLYPH);
    assert_eq!(bar.tools().content(), TOOLS_GLYPH);
}

#[test]
fn toolbar_find_walks_the_buttons_in_order() {
    // new: trap T13 and T14
    let mut bar = ToolbarPanel::new(&english());
    assert_eq!(
        bar.find(TOOLBAR_PANEL_ID).map(|f| f.base().id()),
        Some(TOOLBAR_PANEL_ID)
    );
    for id in [OPEN_FOLDER_ID, SETTINGS_ID, FIT_ID, TOOLS_ID] {
        assert_eq!(bar.find(id).map(|f| f.base().id()), Some(id));
    }
    assert!(bar.find("Missing").is_none());
    bar.buttons_mut()[1].tool_tip = "changed".to_owned();
    let found = bar.find(SETTINGS_ID).expect("settings");
    assert_eq!(
        found.as_any().downcast_ref::<ButtonElement>().expect("button").tool_tip,
        "changed"
    );
}

#[test]
fn toolbar_find_at_returns_the_button_under_the_point_and_the_bar_between_buttons() {
    // new: contract Toolbar 6
    let fake = FakeTextRenderer::new();
    let mut bar = ToolbarPanel::new(&english());
    bar.arrange(&context(&fake));
    let fit = bar.fit().bounds();
    let hit = bar.find_at(PointD {
        x: fit.x + 1.0,
        y: fit.y + 1.0,
    });
    assert_eq!(hit.map(|f| f.base().id()), Some(FIT_ID));
    // The 5 DIP gap between settings and fit belongs to the bar.
    let gap = bar.settings().bounds();
    let in_gap = PointD {
        x: gap.x + gap.width + 2.0,
        y: gap.y + 1.0,
    };
    assert_eq!(bar.find_at(in_gap).map(|f| f.base().id()), Some(TOOLBAR_PANEL_ID));
    assert!(bar.find_at(PointD { x: 900.0, y: 600.0 }).is_none());
}

#[test]
fn toolbar_release_resources_drops_every_button_layout() {
    // new: contract Toolbar 6
    let fake = FakeTextRenderer::new();
    let mut bar = ToolbarPanel::new(&english());
    bar.arrange(&context(&fake));
    assert_eq!(fake.live(), 4);
    bar.release_resources();
    assert_eq!(fake.live(), 0);
}

// ---------------------------------------------------------------- StatusPanel

fn status_model() -> OverlayModel {
    OverlayModel {
        status_panel_visible: true,
        file_info_visible: true,
        status_text: "a.jpg".to_owned(),
        exif_visible: true,
        exif_text: "ISO 100".to_owned(),
        ..OverlayModel::default()
    }
}

#[test]
fn status_exif_line_sits_two_dip_below_status_line() {
    // C#: OverlayMutationGapTests.FolderInfoPanel_IsCappedAt640Dip_AndTheExifLineSitsTwoDipBelowTheStatusLine (second half)
    let fake = FakeTextRenderer::new();
    let mut status = StatusPanel::new(&english());
    status.apply(&status_model());

    status.arrange(&context(&fake));

    let line = status.status().bounds();
    let exif = status.exif().bounds();
    assert_close(exif.y - (line.y + line.height), 2.0);
    assert!(line.height > 0.0 && exif.height > 0.0);
}

#[test]
fn status_defaults_ids_and_style() {
    // new: contract Status 1, 2
    let status = StatusPanel::new(&english());
    assert_eq!(status.base().id(), STATUS_PANEL_ID);
    assert_eq!(STATUS_PANEL_ID, "StatusPanel");
    assert_eq!(STATUS_TEXT_ID, "StatusText");
    assert_eq!(EXIF_TEXT_ID, "ExifInfoText");
    assert_eq!(SKIPPED_ROW_ID, "SkippedRow");
    assert_eq!(SKIPPED_VIEW_LIST_ID, "SkippedViewListButton");
    assert_eq!(SKIPPED_WARNING_ID, "SkippedWarningText");
    let base = status.base();
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Left);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Bottom);
    assert_eq!(base.margin, OverlayThickness::uniform(8.0));
    assert!(base.is_visible);

    assert_eq!(status.status().base().id(), STATUS_TEXT_ID);
    assert_eq!(status.status().trimming(), TextTrimming::CharacterEllipsis);
    assert_eq!(status.status().color, palette::text_bright());
    assert_eq!(status.status().base().margin, OverlayThickness::ZERO);
    assert_eq!(status.exif().base().id(), EXIF_TEXT_ID);
    assert_eq!(status.exif().trimming(), TextTrimming::CharacterEllipsis);
    assert_eq!(status.exif().color, palette::text_secondary());
    assert_eq!(status.exif().base().margin, OverlayThickness::new(0.0, 2.0, 0.0, 0.0));

    let row = status.skipped_row();
    assert_eq!(row.base().id(), SKIPPED_ROW_ID);
    assert_eq!(row.base().margin, OverlayThickness::new(0.0, 4.0, 0.0, 0.0));
    assert_eq!(row.warning().base().id(), SKIPPED_WARNING_ID);
    assert_eq!(*row.warning().style(), style(13.0));
    assert_eq!(row.warning().color, palette::warning());
    assert_eq!(
        row.warning().base().vertical_alignment,
        OverlayVerticalAlignment::Center
    );
    assert_eq!(row.warning().trimming(), TextTrimming::None);
    let list = row.view_list();
    assert_eq!(list.base().id(), SKIPPED_VIEW_LIST_ID);
    assert_eq!(list.padding, OverlayThickness::symmetric(8.0, 2.0));
    assert_eq!(list.base().margin, OverlayThickness::new(8.0, 0.0, 0.0, 0.0));
    assert_eq!(list.background, palette::get("Dark.Input").expect("Dark.Input"));
    assert_eq!(
        list.border_color,
        palette::get("Dark.BorderToolbar").expect("Dark.BorderToolbar")
    );
    assert_eq!(list.corner_radius, 0.0);
    assert_eq!(list.base().vertical_alignment, OverlayVerticalAlignment::Center);
    // The same button through the status panel.
    assert_eq!(status.view_list().base().id(), SKIPPED_VIEW_LIST_ID);
}

#[test]
fn status_apply_maps_each_model_field_to_its_element() {
    // new: contract Status 3
    let mut status = StatusPanel::new(&english());
    status.apply(&OverlayModel {
        status_panel_visible: true,
        file_info_visible: false,
        status_text: "IMG_1.JPG".to_owned(),
        font_size: 14.0,
        exif_font_size: 13.0,
        exif_visible: true,
        exif_text: "ISO 100".to_owned(),
        has_skipped_entries: true,
        skipped_warning_text: "3 skipped".to_owned(),
        ..OverlayModel::default()
    });
    assert!(status.base().is_visible);
    assert!(!status.status().base().is_visible);
    assert_eq!(status.status().text(), "IMG_1.JPG");
    assert_eq!(*status.status().style(), style(14.0));
    assert!(status.exif().base().is_visible);
    assert_eq!(status.exif().text(), "ISO 100");
    assert_eq!(*status.exif().style(), style(13.0));
    assert!(status.skipped_row().base().is_visible);
    assert_eq!(status.skipped_row().warning().text(), "3 skipped");

    status.apply(&OverlayModel {
        status_panel_visible: false,
        file_info_visible: true,
        exif_visible: false,
        ..OverlayModel::default()
    });
    assert!(!status.base().is_visible);
    assert!(status.status().base().is_visible);
    assert!(!status.exif().base().is_visible);
    assert!(!status.skipped_row().base().is_visible);
    assert_eq!(status.skipped_row().warning().text(), "");
}

#[test]
fn status_apply_hides_skipped_row_when_no_skipped_entries() {
    // new: contract Status 3
    let fake = FakeTextRenderer::new();
    let mut status = StatusPanel::new(&english());
    status.apply(&OverlayModel {
        has_skipped_entries: true,
        ..status_model()
    });
    assert!(status.skipped_row().base().is_visible);
    status.apply(&OverlayModel {
        has_skipped_entries: false,
        ..status_model()
    });
    assert!(!status.skipped_row().base().is_visible);

    status.arrange(&context(&fake));
    assert_eq!(status.skipped_row().bounds().height, 0.0);
    let mut dc = RecordingDrawContext::new();
    status.render(&mut dc);
    assert!(dc.texts.iter().all(|t| t.0 != "View list"));
}

#[test]
fn status_apply_does_not_change_warning_or_view_list_style() {
    // new: traps T7 and T11
    let mut status = StatusPanel::new(&english());
    let list_style = status.view_list().style().clone();
    for font_size in [10.0, 20.0] {
        status.apply(&OverlayModel {
            font_size,
            exif_font_size: font_size - 1.0,
            has_skipped_entries: true,
            skipped_warning_text: "warn".to_owned(),
            ..status_model()
        });
        assert_eq!(*status.skipped_row().warning().style(), style(13.0));
        assert_eq!(*status.view_list().style(), list_style);
        assert_eq!(status.skipped_row().warning().color, palette::warning());
        assert_eq!(status.view_list().content(), "View list");
        assert_eq!(status.view_list().automation_name, "View the list of skipped files");
    }
}

#[test]
fn status_localization_sets_view_list_text_and_follows_a_language_switch() {
    // new: contract Status 4 and trap T12
    let mut status = StatusPanel::new(&english());
    assert_eq!(status.view_list().content(), "View list");
    assert_eq!(status.view_list().automation_name, "View the list of skipped files");

    status.apply_localization(&other_language());

    assert_eq!(status.view_list().content(), "V1");
    assert_eq!(status.view_list().automation_name, "V2");
}

#[test]
fn status_view_list_mut_changes_the_button_that_find_returns() {
    // new: trap T14
    let mut status = StatusPanel::new(&english());
    status.view_list_mut().is_hot = true;
    status.view_list_mut().tool_tip = "tip".to_owned();

    let found = status.find(SKIPPED_VIEW_LIST_ID).expect("view list");
    let button = found.as_any().downcast_ref::<ButtonElement>().expect("button");
    assert!(button.is_hot);
    assert_eq!(button.tool_tip, "tip");
    assert!(status.view_list().is_hot);
    assert!(status.skipped_row().view_list().is_hot);
}

#[test]
fn status_element_order_is_status_exif_row() {
    // new: trap T13
    let fake = FakeTextRenderer::new();
    let mut status = StatusPanel::new(&english());
    status.apply(&OverlayModel {
        has_skipped_entries: true,
        skipped_warning_text: "warn".to_owned(),
        ..status_model()
    });
    status.arrange(&context(&fake));

    let line = status.status().bounds();
    let exif = status.exif().bounds();
    let row = status.skipped_row().bounds();
    assert!(line.y < exif.y && exif.y < row.y, "{line:?} {exif:?} {row:?}");
    // The row sits 4 DIP below the EXIF line.
    assert_close(row.y - (exif.y + exif.height), 4.0);
    // Row children: warning first, the button 8 DIP to its right.
    let warning = status.skipped_row().warning().bounds();
    let list = status.view_list().bounds();
    assert_close(list.x - (warning.x + warning.width), 8.0);

    let mut dc = RecordingDrawContext::new();
    status.render(&mut dc);
    let drawn: Vec<&str> = dc.texts.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(drawn, ["a.jpg", "ISO 100", "warn", "View list"]);

    assert_eq!(status.find(EXIF_TEXT_ID).map(|f| f.base().id()), Some(EXIF_TEXT_ID));
    assert_eq!(status.find(SKIPPED_ROW_ID).map(|f| f.base().id()), Some(SKIPPED_ROW_ID));
    assert_eq!(
        status.find(SKIPPED_WARNING_ID).map(|f| f.base().id()),
        Some(SKIPPED_WARNING_ID)
    );
    assert_eq!(
        status.find(SKIPPED_VIEW_LIST_ID).map(|f| f.base().id()),
        Some(SKIPPED_VIEW_LIST_ID)
    );
    let row_node = status.skipped_row();
    assert_eq!(
        row_node.find(SKIPPED_VIEW_LIST_ID).map(|f| f.base().id()),
        Some(SKIPPED_VIEW_LIST_ID)
    );
    assert!(row_node.find(EXIF_TEXT_ID).is_none());
}

#[test]
fn status_skipped_row_is_a_transparent_horizontal_row() {
    // new: contract Status 2 (no background fill of the row, the button hit goes to the button)
    let fake = FakeTextRenderer::new();
    let mut status = StatusPanel::new(&english());
    status.apply(&OverlayModel {
        has_skipped_entries: true,
        skipped_warning_text: "warn".to_owned(),
        ..status_model()
    });
    status.arrange(&context(&fake));
    let mut dc = RecordingDrawContext::new();
    status.render(&mut dc);
    // Fills: the panel (rounded, overlay colour) and the button (plain); the row fills nothing.
    let panel_fills: Vec<(bool, bool)> = dc
        .fills
        .iter()
        .map(|f| (f.rounded, f.color == palette::overlay()))
        .collect();
    assert_eq!(panel_fills, [(true, true), (false, false)]);

    let list = status.view_list().bounds();
    let hit = status.find_at(PointD {
        x: list.x + 1.0,
        y: list.y + 1.0,
    });
    assert_eq!(hit.map(|f| f.base().id()), Some(SKIPPED_VIEW_LIST_ID));
    let row: &SkippedRow = status.skipped_row();
    assert_eq!(row.warning().text(), "warn");
}

#[test]
fn status_release_resources_reaches_nested_children() {
    // new: contract Status 5
    let fake = FakeTextRenderer::new();
    let mut status = StatusPanel::new(&english());
    status.apply(&OverlayModel {
        has_skipped_entries: true,
        skipped_warning_text: "warn".to_owned(),
        ..status_model()
    });
    status.arrange(&context(&fake));
    assert_eq!(fake.live(), 4);
    status.release_resources();
    assert_eq!(fake.live(), 0);
}

// ---------------------------------------------------------------- ZoomIndicatorPanel

#[test]
fn zoom_apply_sets_visibility_text_and_font_size() {
    // new: contract Zoom 2
    let mut zoom = ZoomIndicatorPanel::new(&english());
    assert!(zoom.is_visible());
    zoom.apply(&OverlayModel {
        zoom_indicator_visible: true,
        zoom_indicator_text: "150%".to_owned(),
        font_size: 14.0,
        ..OverlayModel::default()
    });
    assert!(zoom.is_visible());
    assert_eq!(zoom.text().text(), "150%");
    assert_eq!(*zoom.text().style(), style(14.0));

    zoom.apply(&OverlayModel::default());
    assert!(!zoom.is_visible());
    assert_eq!(zoom.text().text(), "");
    assert_eq!(*zoom.text().style(), style(12.0));
}

#[test]
fn zoom_defaults_and_text_is_not_trimmed() {
    // new: trap T8 and contract Zoom 1
    assert_eq!(ZOOM_PANEL_ID, "ZoomIndicatorPanel");
    assert_eq!(ZOOM_TEXT_ID, "ZoomIndicatorText");
    let zoom = ZoomIndicatorPanel::new(&english());
    assert_eq!(zoom.base().id(), ZOOM_PANEL_ID);
    assert_eq!(zoom.base().horizontal_alignment, OverlayHorizontalAlignment::Right);
    assert_eq!(zoom.base().vertical_alignment, OverlayVerticalAlignment::Top);
    assert_eq!(zoom.base().margin, OverlayThickness::uniform(8.0));
    assert_eq!(zoom.text().base().id(), ZOOM_TEXT_ID);
    assert_eq!(zoom.text().trimming(), TextTrimming::None);
    assert_eq!(zoom.text().color, palette::text_bright());
    assert_eq!(*zoom.text().style(), TextStyle::default());
}

#[test]
fn zoom_automation_name_comes_from_the_catalog() {
    // new: contract Zoom 3
    let mut zoom = ZoomIndicatorPanel::new(&english());
    assert_eq!(zoom.automation_name(), "Current zoom percentage");
    zoom.apply_localization(&other_language());
    assert_eq!(zoom.automation_name(), "Z1");
}

#[test]
fn zoom_sits_top_right_and_renders_its_text() {
    // new: contract Zoom 1 (right/top anchoring with margin 8 and padding 6/4)
    let fake = FakeTextRenderer::new();
    let mut zoom = ZoomIndicatorPanel::new(&english());
    zoom.apply(&OverlayModel {
        zoom_indicator_visible: true,
        zoom_indicator_text: "ABCDEFGHIJ".to_owned(),
        font_size: 10.0,
        ..OverlayModel::default()
    });

    zoom.arrange(&context(&fake));

    let b = zoom.bounds();
    assert_eq!((b.x, b.y, b.width, b.height), (1000.0 - 8.0 - 62.0, 8.0, 62.0, 23.0));
    let mut dc = RecordingDrawContext::new();
    zoom.render(&mut dc);
    assert_eq!(dc.fills.len(), 1);
    assert!(dc.fills[0].rounded);
    assert_eq!(dc.texts.len(), 1);
    assert_eq!(dc.texts[0].0, "ABCDEFGHIJ");
    // The text is click-through; the panel takes the hit.
    assert_eq!(
        zoom.find_at(PointD {
            x: b.x + 10.0,
            y: b.y + 10.0
        })
        .map(|f| f.base().id()),
        Some(ZOOM_PANEL_ID)
    );
    assert!(zoom.find(ZOOM_TEXT_ID).is_some());
    zoom.release_resources();
    assert_eq!(fake.live(), 0);
}
