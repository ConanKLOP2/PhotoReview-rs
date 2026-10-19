//! Red tests (W8-15e, test porter) for `pr_app::overlay::{status, toolbar, tools_menu, zoom_indicator}`.
//!
//! Port of `PhotoReview.Shell.Tests/Overlay/{OverlayPartsTests,OverlayMutationGapTests}.cs` (baseline `d92d6aae`) plus F tests
//! (`// new:`) for the traps T1 to T14 of `contracts/W8-15e.md`. The fake text renderer and the recording draw context come
//! from `overlay_fakes.rs` (width = characters x size / 2, height = size x 1.5); the catalog is the built-in English one.
//!
//! Every test calls a body that still ends in `todo!()` (`new`, `build`, `execute`), so the tests fail until the porter fills
//! the four modules.
//!
//! The optional fixture test `status_and_zoom_boxes_match_overlay_boxes_fixture` (contract O8) is deferred: it needs the C#
//! `FakeTextRenderer` metric of the golden fixture, which is the W8-15j / W8-20b integration.

#[path = "overlay_fakes.rs"]
mod overlay_fakes;

use overlay_fakes::{FakeTextRenderer, RecordingDrawContext};
use pr_app::input::primitives::{PointD, SizeD};
use pr_app::overlay::button::ButtonElement;
use pr_app::overlay::contracts::{OverlayElement, OverlayLayoutContext};
use pr_app::overlay::element::{ElementNode, OverlayHorizontalAlignment, OverlayThickness, OverlayVerticalAlignment};
use pr_app::overlay::model::OverlayModel;
use pr_app::overlay::palette;
use pr_app::overlay::status::{
    EXIF_TEXT_ID, PANEL_ID as STATUS_PANEL_ID, SKIPPED_ROW_ID, SKIPPED_VIEW_LIST_ID, SKIPPED_WARNING_ID,
    STATUS_TEXT_ID, StatusPanel,
};
use pr_app::overlay::text::TextElement;
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
use pr_l10n::localizer::{Localizer, built_in_english_localizer};

fn context(text: &FakeTextRenderer, width: f64, height: f64) -> OverlayLayoutContext<'_> {
    OverlayLayoutContext::new(SizeD { width, height }, 1.0, text)
}

fn segoe(size: f64) -> TextStyle {
    TextStyle {
        font_family: "Segoe UI".to_owned(),
        font_size_dip: size,
        bold: false,
    }
}

fn english() -> std::sync::Arc<Localizer> {
    built_in_english_localizer()
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-3, "expected {expected}, got {actual}");
}

fn dark(key: &str) -> pr_app::services::render::ColorF {
    palette::get(key).expect("palette key")
}

/// Recording `OverlayCommands` (C# `FakeOverlayCommands`): one string per call.
#[derive(Default)]
struct RecordingCommands {
    calls: Vec<String>,
}

impl OverlayCommands for RecordingCommands {
    fn open_folder(&mut self) {
        self.calls.push("open_folder".to_owned());
    }

    fn show_settings(&mut self) {
        self.calls.push("show_settings".to_owned());
    }

    fn fit(&mut self) {
        self.calls.push("fit".to_owned());
    }

    fn show_skipped_files(&mut self) {
        self.calls.push("show_skipped_files".to_owned());
    }

    fn show_recovery(&mut self) {
        self.calls.push("show_recovery".to_owned());
    }

    fn show_diagnostics(&mut self) {
        self.calls.push("show_diagnostics".to_owned());
    }

    fn show_benchmark(&mut self) {
        self.calls.push("show_benchmark".to_owned());
    }

    fn clear_cache(&mut self) {
        self.calls.push("clear_cache".to_owned());
    }

    fn remove_duplicates(&mut self, numbered: bool) {
        self.calls.push(format!("remove_duplicates({numbered})"));
    }
}

// ---- tools_menu --------------------------------------------------------------------------------------------------------

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
    // new: Semantics 1 (`isEnabled: hasImages` for the two duplicate removals only)
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
    // DIVERGENCE: the C# `ArgumentNullException` for a null `commands` has no Rust form (`&mut dyn OverlayCommands` is never null).
    let mut commands = RecordingCommands::default();

    assert!(!tools_menu::execute(Some("Tools.Nope"), &mut commands, true));
    assert!(!tools_menu::execute(None, &mut commands, true));

    assert!(commands.calls.is_empty());
}

#[test]
fn tools_menu_execute_routes_each_tools_id_to_its_command() {
    // new: Semantics 2
    let expected = [
        (RECOVERY, "show_recovery"),
        (DIAGNOSTICS, "show_diagnostics"),
        (BENCHMARK, "show_benchmark"),
        (CLEAR_CACHE, "clear_cache"),
        (REMOVE_NUMBERED_DUPLICATES, "remove_duplicates(true)"),
        (REMOVE_ORIGINAL_DUPLICATES, "remove_duplicates(false)"),
    ];
    for (id, call) in expected {
        let mut commands = RecordingCommands::default();

        assert!(tools_menu::execute(Some(id), &mut commands, true), "{id} is a known id");

        assert_eq!(commands.calls, [call], "{id}");
    }
}

#[test]
fn tools_menu_execute_duplicates_without_images_is_true_and_calls_nothing() {
    // new: T10 (a completed task in C#: the host must not show "unknown tool")
    for id in [REMOVE_NUMBERED_DUPLICATES, REMOVE_ORIGINAL_DUPLICATES] {
        let mut commands = RecordingCommands::default();

        assert!(tools_menu::execute(Some(id), &mut commands, false), "{id}");

        assert!(commands.calls.is_empty(), "{id}");
    }
}

#[test]
fn tools_menu_execute_ids_are_ordinal() {
    // new: T9 (exact string equality)
    for id in [
        "tools.recovery",
        "Tools.Recovery ",
        " Tools.Recovery",
        "TOOLS.RECOVERY",
        "",
    ] {
        let mut commands = RecordingCommands::default();

        assert!(!tools_menu::execute(Some(id), &mut commands, true), "{id:?}");

        assert!(commands.calls.is_empty(), "{id:?}");
    }
    // Positive control: the exact id works, so the loop above is not trivially passing.
    let mut commands = RecordingCommands::default();
    assert!(tools_menu::execute(Some(RECOVERY), &mut commands, true));
}

// ---- toolbar -----------------------------------------------------------------------------------------------------------

#[test]
fn toolbar_box_and_button_spacing_match_the_xaml_exactly() {
    // C#: OverlayMutationGapTests.Toolbar_BoxAndButtonSpacingMatchTheXamlExactly
    let fake = FakeTextRenderer::new();
    let mut bar = ToolbarPanel::new(&english());

    bar.arrange(&context(&fake, 1000.0, 700.0));

    let bounds = bar.bounds();
    assert_close(bounds.x, 8.0);
    assert_close(bounds.y, 8.0);
    assert_close(bounds.height, 40.0); // 32 (the tools button) + padding 4 + 4
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
    assert_close(bounds.x + bounds.width - 6.0, tools.x + tools.width); // right padding 6
}

#[test]
fn toolbar_defaults_match_create_button() {
    // new: Semantics 1 and 2 (table), T2, T5
    let bar = ToolbarPanel::new(&english());

    let base = ElementNode::base(&bar);
    assert_eq!(base.id(), TOOLBAR_PANEL_ID);
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Left);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Top);
    assert_eq!(base.margin, OverlayThickness::uniform(8.0));
    assert!(base.is_visible);

    let rows = [
        (bar.open_folder(), OPEN_FOLDER_ID, 16.0, (8.0, 2.0), 0.0),
        (bar.settings(), SETTINGS_ID, 12.0, (8.0, 4.0), 5.0),
        (bar.fit(), FIT_ID, 12.0, (8.0, 4.0), 5.0),
        (bar.tools(), TOOLS_ID, 18.0, (0.0, 0.0), 5.0),
    ];
    for (button, id, size, padding, margin_left) in rows {
        assert_eq!(ElementNode::base(button).id(), id);
        assert_eq!(*button.style(), segoe(size), "{id}");
        assert_eq!(
            button.padding,
            OverlayThickness::symmetric(padding.0, padding.1),
            "{id}"
        );
        assert_eq!(
            ElementNode::base(button).margin,
            OverlayThickness::new(margin_left, 0.0, 0.0, 0.0),
            "{id}"
        );
        assert_eq!(button.background, dark("Dark.Input"), "{id}");
        assert_eq!(button.border_color, dark("Dark.BorderToolbar"), "{id}");
        assert_eq!(button.corner_radius, 0.0, "{id}");
    }
    // Only the tools button has a fixed size (T2).
    assert!(bar.open_folder().fixed_width.is_nan() && bar.open_folder().fixed_height.is_nan());
    assert!(bar.settings().fixed_width.is_nan() && bar.fit().fixed_height.is_nan());
    assert_eq!((bar.tools().fixed_width, bar.tools().fixed_height), (34.0, 32.0));
}

#[test]
fn toolbar_glyphs_are_the_c_code_points() {
    // new: T1 (and the buttons carry them, unchanged by the language)
    let bar = ToolbarPanel::new(&english());

    assert_eq!(OPEN_FOLDER_GLYPH.chars().collect::<Vec<_>>(), ['\u{1F4C2}']);
    assert_eq!(SETTINGS_GLYPH.chars().collect::<Vec<_>>(), ['\u{2699}']);
    assert_eq!(TOOLS_GLYPH.chars().collect::<Vec<_>>(), ['\u{22EE}']);
    assert_eq!(bar.open_folder().content(), OPEN_FOLDER_GLYPH);
    assert_eq!(bar.settings().content(), SETTINGS_GLYPH);
    assert_eq!(bar.tools().content(), TOOLS_GLYPH);
}

#[test]
fn toolbar_buttons_are_in_tab_order() {
    // new: Semantics 5 and 6, T13
    let bar = ToolbarPanel::new(&english());

    let ids: Vec<&str> = bar.buttons().iter().map(|b| ElementNode::base(*b).id()).collect();
    assert_eq!(ids, [OPEN_FOLDER_ID, SETTINGS_ID, FIT_ID, TOOLS_ID]);
    // The element tree has the same order: render draws open folder, settings, fit, tools.
    let fake = FakeTextRenderer::new();
    let mut bar = bar;
    bar.arrange(&context(&fake, 1000.0, 700.0));
    let mut dc = RecordingDrawContext::new();
    bar.render(&mut dc);
    let texts: Vec<&str> = dc.texts.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(texts, [OPEN_FOLDER_GLYPH, SETTINGS_GLYPH, "Fit", TOOLS_GLYPH]);
}

#[test]
fn toolbar_move_focus_forward_from_none_starts_at_open_folder() {
    // new: T3
    let mut bar = ToolbarPanel::new(&english());
    assert_eq!(bar.focused_index(), None);
    assert!(!bar.has_keyboard_focus());

    assert!(bar.move_focus(true));

    assert_eq!(bar.focused_index(), Some(0));
    assert!(bar.has_keyboard_focus());
    assert_eq!(
        bar.focused_button().map(|b| ElementNode::base(b).id()),
        Some(OPEN_FOLDER_ID)
    );
}

#[test]
fn toolbar_move_focus_backward_from_none_starts_at_tools() {
    // new: T3
    let mut bar = ToolbarPanel::new(&english());

    assert!(bar.move_focus(false));

    assert_eq!(bar.focused_index(), Some(3));
    assert_eq!(bar.focused_button().map(|b| ElementNode::base(b).id()), Some(TOOLS_ID));
}

#[test]
fn toolbar_move_focus_steps_through_the_buttons() {
    // new: T3 (Tab order)
    let mut bar = ToolbarPanel::new(&english());

    for expected in 0..4 {
        assert!(bar.move_focus(true));
        assert_eq!(bar.focused_index(), Some(expected));
    }
    for expected in (0..3).rev() {
        assert!(bar.move_focus(false));
        assert_eq!(bar.focused_index(), Some(expected));
    }
}

#[test]
fn toolbar_move_focus_does_not_wrap() {
    // new: T3 (past the end: no button, never wraps; signed arithmetic, no usize underflow)
    let mut bar = ToolbarPanel::new(&english());
    bar.move_focus(false); // index 3
    assert!(!bar.move_focus(true));
    assert_eq!(bar.focused_index(), None);
    assert!(!bar.has_keyboard_focus());
    assert!(bar.focused_button().is_none());

    let mut bar = ToolbarPanel::new(&english());
    bar.move_focus(true); // index 0
    assert!(!bar.move_focus(false));
    assert_eq!(bar.focused_index(), None);
    assert!(bar.buttons().iter().all(|b| !b.is_focused));
}

#[test]
fn toolbar_focus_mirrors_is_focused_on_exactly_one_button() {
    // new: T4
    let mut bar = ToolbarPanel::new(&english());
    assert!(bar.buttons().iter().all(|b| !b.is_focused));

    for step in 0..4 {
        bar.move_focus(true);
        let focused: Vec<bool> = bar.buttons().iter().map(|b| b.is_focused).collect();
        let expected: Vec<bool> = (0..4).map(|i| i == step).collect();
        assert_eq!(focused, expected, "after step {step}");
        assert_eq!(bar.focused_index(), Some(step));
    }

    bar.move_focus(false);
    bar.clear_focus();
    assert_eq!(bar.focused_index(), None);
    assert!(bar.buttons().iter().all(|b| !b.is_focused));
}

#[test]
fn toolbar_buttons_mut_reaches_the_owned_buttons() {
    // new: T14 (each typed child is owned once)
    let mut bar = ToolbarPanel::new(&english());

    for button in bar.buttons_mut() {
        button.tool_tip = "changed".to_owned();
    }

    assert!(bar.buttons().iter().all(|b| b.tool_tip == "changed"));
    let found = bar.find(FIT_ID).expect("fit button");
    let found = found.as_any().downcast_ref::<ButtonElement>().expect("a button");
    assert_eq!(found.tool_tip, "changed");
}

#[test]
fn toolbar_apply_localization_sets_fit_text_tooltip_and_automation() {
    // new: Semantics 3
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
fn toolbar_apply_localization_again_replaces_the_text_but_not_the_glyphs() {
    // new: Semantics 3 (language switch)
    let english = english();
    let pseudo = pr_l10n::localizer::create_pseudo(&english);
    let mut bar = ToolbarPanel::new(&english);

    bar.apply_localization(&pseudo);

    assert_eq!(bar.fit().content(), pseudo.get("main.toolbar.fit"));
    assert_ne!(bar.fit().content(), "Fit");
    assert_eq!(bar.settings().tool_tip, pseudo.get("main.toolbar.settings.toolTip"));
    assert_eq!(bar.open_folder().content(), OPEN_FOLDER_GLYPH);
    assert_eq!(bar.settings().content(), SETTINGS_GLYPH);
    assert_eq!(bar.tools().content(), TOOLS_GLYPH);
}

// ---- status panel ------------------------------------------------------------------------------------------------------

#[test]
fn status_defaults_match_the_c_constructor() {
    // new: Semantics 1 and 2, T6, T7
    let panel = StatusPanel::new(&english());

    let base = ElementNode::base(&panel);
    assert_eq!(base.id(), STATUS_PANEL_ID);
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Left);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Bottom);
    assert_eq!(base.margin, OverlayThickness::uniform(8.0));
    assert_eq!(ElementNode::base(panel.status()).id(), STATUS_TEXT_ID);
    assert_eq!(panel.status().trimming(), TextTrimming::CharacterEllipsis);
    assert_eq!(ElementNode::base(panel.exif()).id(), EXIF_TEXT_ID);
    assert_eq!(panel.exif().trimming(), TextTrimming::CharacterEllipsis);
    assert_eq!(
        ElementNode::base(panel.exif()).margin,
        OverlayThickness::new(0.0, 2.0, 0.0, 0.0)
    );
    assert_eq!(panel.exif().color, palette::text_secondary());

    let row = panel.skipped_row();
    assert_eq!(ElementNode::base(row).id(), SKIPPED_ROW_ID);
    assert_eq!(ElementNode::base(row).margin, OverlayThickness::new(0.0, 4.0, 0.0, 0.0));
    let warning = row.warning();
    assert_eq!(ElementNode::base(warning).id(), SKIPPED_WARNING_ID);
    assert_eq!(*warning.style(), segoe(13.0));
    assert_eq!(warning.color, palette::warning());
    assert_eq!(
        ElementNode::base(warning).vertical_alignment,
        OverlayVerticalAlignment::Center
    );
    let view_list = row.view_list();
    assert_eq!(ElementNode::base(view_list).id(), SKIPPED_VIEW_LIST_ID);
    assert_eq!(view_list.padding, OverlayThickness::symmetric(8.0, 2.0));
    assert_eq!(
        ElementNode::base(view_list).margin,
        OverlayThickness::new(8.0, 0.0, 0.0, 0.0)
    );
    assert_eq!(view_list.background, dark("Dark.Input"));
    assert_eq!(view_list.border_color, dark("Dark.BorderToolbar"));
    assert_eq!(view_list.corner_radius, 0.0);
    assert_eq!(
        ElementNode::base(view_list).vertical_alignment,
        OverlayVerticalAlignment::Center
    );
}

#[test]
fn status_apply_localization_sets_the_view_list_texts() {
    // new: Semantics 4, T12
    let panel = StatusPanel::new(&english());

    assert_eq!(panel.view_list().content(), "View list");
    assert_eq!(panel.view_list().automation_name, "View the list of skipped files");
    assert_eq!(panel.skipped_row().view_list().content(), "View list");
}

#[test]
fn status_exif_line_sits_two_dip_below_status_line() {
    // C#: OverlayMutationGapTests.FolderInfoPanel_IsCappedAt640Dip_AndTheExifLineSitsTwoDipBelowTheStatusLine (second half)
    let fake = FakeTextRenderer::new();
    let mut panel = StatusPanel::new(&english());
    panel.apply(&OverlayModel {
        status_panel_visible: true,
        file_info_visible: true,
        status_text: "a.jpg".to_owned(),
        exif_visible: true,
        exif_text: "ISO 100".to_owned(),
        ..OverlayModel::default()
    });

    panel.arrange(&context(&fake, 1000.0, 700.0));

    let status = panel.status().bounds();
    let exif = panel.exif().bounds();
    assert_close(exif.y - (status.y + status.height), 2.0);
}

#[test]
fn status_apply_sets_visibility_text_and_font_of_each_line() {
    // C#: ShellOverlayBehaviorTests.Model_DrivesVisibilityTextAndFontOfEveryPanel (status part)
    let mut panel = StatusPanel::new(&english());

    panel.apply(&OverlayModel {
        status_panel_visible: true,
        file_info_visible: true,
        status_text: "IMG_1.JPG".to_owned(),
        font_size: 14.0,
        exif_font_size: 13.0,
        exif_visible: true,
        exif_text: "ISO 100".to_owned(),
        ..OverlayModel::default()
    });

    assert!(ElementNode::base(&panel).is_visible);
    assert!(ElementNode::base(panel.status()).is_visible);
    assert_eq!(panel.status().text(), "IMG_1.JPG");
    assert_eq!(*panel.status().style(), segoe(14.0));
    assert!(ElementNode::base(panel.exif()).is_visible);
    assert_eq!(panel.exif().text(), "ISO 100");
    assert_eq!(*panel.exif().style(), segoe(13.0));

    panel.apply(&OverlayModel {
        status_panel_visible: false,
        file_info_visible: false,
        exif_visible: false,
        ..OverlayModel::default()
    });
    assert!(!ElementNode::base(&panel).is_visible);
    assert!(!ElementNode::base(panel.status()).is_visible);
    assert!(!ElementNode::base(panel.exif()).is_visible);
}

#[test]
fn status_apply_hides_skipped_row_when_no_skipped_entries() {
    // new: Semantics 3 (C#: StatusPanel_SkippedRowAndWarningFollowTheModel_ViewListButtonClicks, model part)
    let mut panel = StatusPanel::new(&english());

    panel.apply(&OverlayModel {
        status_panel_visible: true,
        has_skipped_entries: true,
        skipped_warning_text: "Skipped 3 files".to_owned(),
        ..OverlayModel::default()
    });
    assert!(ElementNode::base(panel.skipped_row()).is_visible);
    assert_eq!(panel.skipped_row().warning().text(), "Skipped 3 files");

    panel.apply(&OverlayModel {
        status_panel_visible: true,
        has_skipped_entries: false,
        ..OverlayModel::default()
    });
    assert!(!ElementNode::base(panel.skipped_row()).is_visible);
}

#[test]
fn status_apply_does_not_change_warning_or_view_list_style() {
    // new: T7, T11 (apply is narrow)
    let mut panel = StatusPanel::new(&english());
    panel.apply(&OverlayModel {
        font_size: 12.0,
        exif_font_size: 11.0,
        has_skipped_entries: true,
        ..OverlayModel::default()
    });
    let button_style = panel.view_list().style().clone();
    let button_padding = panel.view_list().padding;

    panel.apply(&OverlayModel {
        font_size: 20.0,
        exif_font_size: 19.0,
        has_skipped_entries: true,
        skipped_warning_text: "other".to_owned(),
        ..OverlayModel::default()
    });

    assert_eq!(*panel.skipped_row().warning().style(), segoe(13.0));
    assert_eq!(*panel.view_list().style(), button_style);
    assert_eq!(panel.view_list().padding, button_padding);
    assert_eq!(
        panel.view_list().content(),
        "View list",
        "apply does not touch the localised text"
    );
}

#[test]
fn status_view_list_mut_changes_the_button_that_find_returns() {
    // new: T14
    let mut panel = StatusPanel::new(&english());

    panel.view_list_mut().tool_tip = "changed".to_owned();

    assert_eq!(panel.view_list().tool_tip, "changed");
    let found = panel.find(SKIPPED_VIEW_LIST_ID).expect("view list button");
    let found = found.as_any().downcast_ref::<ButtonElement>().expect("a button");
    assert_eq!(found.tool_tip, "changed");
    assert_eq!(panel.skipped_row().view_list().tool_tip, "changed");
}

#[test]
fn status_element_order_is_status_exif_row() {
    // new: T13 (`find` ids and the draw order)
    let fake = FakeTextRenderer::new();
    let mut panel = StatusPanel::new(&english());
    panel.apply(&OverlayModel {
        status_panel_visible: true,
        file_info_visible: true,
        status_text: "a.jpg".to_owned(),
        exif_visible: true,
        exif_text: "ISO 100".to_owned(),
        has_skipped_entries: true,
        skipped_warning_text: "Skipped".to_owned(),
        ..OverlayModel::default()
    });
    panel.arrange(&context(&fake, 1000.0, 700.0));
    let mut dc = RecordingDrawContext::new();

    panel.render(&mut dc);

    let texts: Vec<&str> = dc.texts.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(texts, ["a.jpg", "ISO 100", "Skipped", "View list"]);
    assert_eq!(panel.find(STATUS_PANEL_ID).expect("self").id(), STATUS_PANEL_ID);
    let exif = panel.find(EXIF_TEXT_ID).expect("exif line");
    assert_eq!(
        exif.as_any().downcast_ref::<TextElement>().map(TextElement::text),
        Some("ISO 100")
    );
    assert_eq!(panel.find(SKIPPED_ROW_ID).expect("row").id(), SKIPPED_ROW_ID);
    assert_eq!(
        panel.find(SKIPPED_VIEW_LIST_ID).expect("button").id(),
        SKIPPED_VIEW_LIST_ID
    );
    assert!(panel.find("Missing").is_none());
}

#[test]
fn status_skipped_row_find_walks_its_children_in_order() {
    // new: T13 (the row's children are `warning, view_list`)
    let panel = StatusPanel::new(&english());
    let row = panel.skipped_row();

    assert_eq!(row.find(SKIPPED_ROW_ID).expect("self").id(), SKIPPED_ROW_ID);
    assert_eq!(row.find(SKIPPED_WARNING_ID).expect("warning").id(), SKIPPED_WARNING_ID);
    assert_eq!(
        row.find(SKIPPED_VIEW_LIST_ID).expect("button").id(),
        SKIPPED_VIEW_LIST_ID
    );
    assert!(row.find(EXIF_TEXT_ID).is_none());
}

#[test]
fn status_find_at_returns_the_view_list_button_over_the_panel() {
    // new: Semantics 5 (the button takes the hit, the text is click-through)
    let fake = FakeTextRenderer::new();
    let mut panel = StatusPanel::new(&english());
    panel.apply(&OverlayModel {
        status_panel_visible: true,
        file_info_visible: true,
        status_text: "a.jpg".to_owned(),
        has_skipped_entries: true,
        skipped_warning_text: "Skipped".to_owned(),
        ..OverlayModel::default()
    });
    panel.arrange(&context(&fake, 1000.0, 700.0));
    let button = panel.view_list().bounds();
    let status = panel.status().bounds();

    let on_button = panel
        .find_at(PointD {
            x: button.x + 1.0,
            y: button.y + 1.0,
        })
        .expect("button hit");
    assert_eq!(on_button.id(), SKIPPED_VIEW_LIST_ID);
    let on_text = panel
        .find_at(PointD {
            x: status.x + 1.0,
            y: status.y + 1.0,
        })
        .expect("panel hit");
    assert_eq!(on_text.id(), STATUS_PANEL_ID);
}

// ---- zoom indicator ----------------------------------------------------------------------------------------------------

#[test]
fn zoom_defaults_are_right_top_with_an_untrimmed_bright_text() {
    // new: Semantics 1, T8
    let panel = ZoomIndicatorPanel::new(&english());

    let base = ElementNode::base(&panel);
    assert_eq!(base.id(), ZOOM_PANEL_ID);
    assert_eq!(base.horizontal_alignment, OverlayHorizontalAlignment::Right);
    assert_eq!(base.vertical_alignment, OverlayVerticalAlignment::Top);
    assert_eq!(base.margin, OverlayThickness::uniform(8.0));
    assert_eq!(ElementNode::base(panel.text()).id(), ZOOM_TEXT_ID);
    assert_eq!(panel.text().color, palette::text_bright());
}

#[test]
fn zoom_apply_sets_visibility_text_and_font_size() {
    // new: Semantics 2 (C#: Model_DrivesVisibilityTextAndFontOfEveryPanel, zoom part)
    let mut panel = ZoomIndicatorPanel::new(&english());

    panel.apply(&OverlayModel {
        zoom_indicator_visible: true,
        zoom_indicator_text: "150%".to_owned(),
        font_size: 15.0,
        ..OverlayModel::default()
    });

    assert!(ElementNode::base(&panel).is_visible);
    assert_eq!(panel.text().text(), "150%");
    assert_eq!(*panel.text().style(), segoe(15.0));

    panel.apply(&OverlayModel {
        zoom_indicator_visible: false,
        ..OverlayModel::default()
    });
    assert!(!ElementNode::base(&panel).is_visible);
}

#[test]
fn zoom_apply_ignores_the_other_model_fields() {
    // new: T11
    let mut panel = ZoomIndicatorPanel::new(&english());

    panel.apply(&OverlayModel {
        zoom_indicator_visible: true,
        zoom_indicator_text: "100%".to_owned(),
        status_text: "IMG".to_owned(),
        exif_text: "ISO".to_owned(),
        folder_info_text: "Trip".to_owned(),
        ..OverlayModel::default()
    });

    assert_eq!(panel.text().text(), "100%");
}

#[test]
fn zoom_text_is_not_trimmed() {
    // new: T8
    let fake = FakeTextRenderer::new();
    let mut panel = ZoomIndicatorPanel::new(&english());
    panel.apply(&OverlayModel {
        zoom_indicator_visible: true,
        zoom_indicator_text: "150%".to_owned(),
        ..OverlayModel::default()
    });

    panel.arrange(&context(&fake, 800.0, 600.0));

    assert_eq!(panel.text().trimming(), TextTrimming::None);
    let last = fake.requests().pop().expect("a layout request");
    assert_eq!(last.3, TextTrimming::None);
}

#[test]
fn zoom_automation_name_comes_from_the_catalog() {
    // new: Semantics 3 (T12: `new` applies the localization)
    let english = english();
    let mut panel = ZoomIndicatorPanel::new(&english);
    assert_eq!(panel.automation_name(), "Current zoom percentage");

    let pseudo = pr_l10n::localizer::create_pseudo(&english);
    panel.apply_localization(&pseudo);
    assert_eq!(panel.automation_name(), pseudo.get("main.zoomIndicator.automationName"));
    assert_ne!(panel.automation_name(), "Current zoom percentage");
}

#[test]
fn zoom_panel_anchors_to_the_top_right_corner() {
    // new: Semantics 1 (margin 8, padding (6, 4))
    let fake = FakeTextRenderer::new();
    let mut panel = ZoomIndicatorPanel::new(&english());
    panel.apply(&OverlayModel {
        zoom_indicator_visible: true,
        zoom_indicator_text: "100%".to_owned(),
        ..OverlayModel::default()
    });

    panel.arrange(&context(&fake, 800.0, 600.0));

    // "100%" = 4 characters x 6 = 24 wide, 18 high (size 12); + padding 12 / 8.
    let bounds = panel.bounds();
    assert_close(bounds.width, 36.0);
    assert_close(bounds.height, 26.0);
    assert_close(bounds.x, 800.0 - 8.0 - 36.0);
    assert_close(bounds.y, 8.0);
}
