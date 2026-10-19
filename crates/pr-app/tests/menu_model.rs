//! Red tests (W8-02a, test porter) for `pr_app::menus::{model, builder}` (C# `Menus/ContextMenuModelBuilder.cs`,
//! baseline `d92d6aae`). Ported from `ContextMenuModelBuilderTests.cs`; the two `CalculateClickLevel...` cases live in
//! `menus_zoom_rules.rs` (W8-01d). Tests marked "new" have no C# twin (contract section "C# tests to port").
//!
//! Every test fails through the `todo!()` of the builder stub until the port lands.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pr_app::menus::builder::{build, build_tools_menu};
use pr_app::menus::model::{ContextMenuBuildContext, MenuItemKind, MenuItemModel};
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::context_menu::{
    ContextMenuContext, ContextMenuItemId, ITEMS, apply_hidden, compute, effective_hidden,
};
use pr_l10n::localizer::{Localizer, built_in_english_localizer};
use pr_l10n::tr;
use pr_test_support::golden;
use serde_json::{Value, json};

use ContextMenuItemId as Id;

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

static GLOBAL: Mutex<()> = Mutex::new(());

/// Holds the process-wide lock, pins the ambient localizer to English and restores the previous one on drop.
struct English {
    previous: Arc<Localizer>,
    _lock: MutexGuard<'static, ()>,
}

fn english() -> English {
    let lock = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = Localizer::current();
    Localizer::set_current(built_in_english_localizer());
    English { previous, _lock: lock }
}

impl Drop for English {
    fn drop(&mut self) {
        Localizer::set_current(Arc::clone(&self.previous));
    }
}

fn default_settings() -> AppSettings {
    AppSettings::new(&EnglishSettingsTexts)
}

fn hiding(hidden: &[Id]) -> AppSettings {
    let mut settings = default_settings();
    apply_hidden(&mut settings, hidden.iter().copied());
    settings
}

/// C# test helper `Context(settings, hasImage, canCopyPath, effectiveZoom)`.
fn context(
    settings: &AppSettings,
    has_image: bool,
    can_copy_path: bool,
    effective_zoom: Option<f64>,
) -> ContextMenuBuildContext<'_> {
    ContextMenuBuildContext {
        settings,
        has_image,
        can_undo: true,
        is_fit: effective_zoom.is_none(),
        effective_zoom,
        can_open_in_external_editor: true,
        can_copy_path,
        is_compare_visible: false,
    }
}

fn build_default_context(settings: &AppSettings) -> Vec<MenuItemModel> {
    build(&context(settings, true, true, Some(1.0))).expect("build")
}

/// Ids in order, each separator as "-".
fn ids(items: &[MenuItemModel]) -> Vec<String> {
    items
        .iter()
        .map(|i| {
            if i.kind == MenuItemKind::Separator {
                "-".to_owned()
            } else {
                i.id.clone()
            }
        })
        .collect()
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn find<'a>(items: &'a [MenuItemModel], id: &str) -> &'a MenuItemModel {
    let mut matches = items.iter().filter(|i| i.id == id);
    let first = matches.next().unwrap_or_else(|| panic!("no item {id}"));
    assert!(matches.next().is_none(), "more than one item {id}");
    first
}

fn zoom_children(items: &[MenuItemModel]) -> &[MenuItemModel] {
    &find(items, "ZoomSubmenu").children
}

fn flatten<'a>(items: &'a [MenuItemModel], out: &mut Vec<&'a MenuItemModel>) {
    for item in items {
        out.push(item);
        flatten(&item.children, out);
    }
}

fn assert_no_stray_separators(items: &[MenuItemModel]) {
    if items.is_empty() {
        return;
    }
    assert_ne!(items[0].kind, MenuItemKind::Separator, "leading separator");
    assert_ne!(
        items[items.len() - 1].kind,
        MenuItemKind::Separator,
        "trailing separator"
    );
    for pair in items.windows(2) {
        assert!(
            !(pair[0].kind == MenuItemKind::Separator && pair[1].kind == MenuItemKind::Separator),
            "double separator"
        );
    }
    for submenu in items.iter().filter(|i| i.kind == MenuItemKind::Submenu) {
        assert_no_stray_separators(&submenu.children);
    }
}

fn assert_no_empty_submenu(items: &[MenuItemModel]) {
    for submenu in items.iter().filter(|i| i.kind == MenuItemKind::Submenu) {
        assert!(!submenu.children.is_empty(), "empty submenu {}", submenu.id);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Ported C# tests
// ---------------------------------------------------------------------------------------------------------------------

// C#: ContextMenuModelBuilderTests.Build_DefaultSettings_MatchesDefaultWpfMenu
#[test]
fn build_default_settings_matches_default_wpf_menu() {
    let _en = english();
    let settings = default_settings();
    let items = build_default_context(&settings);
    assert_eq!(
        ids(&items),
        strs(&[
            "Undo",
            "-",
            "Refresh",
            "-",
            "OpenFolder",
            "NextFolder",
            "PreviousFolder",
            "-",
            "ExternalEditor",
            "-",
            "CopyFileName",
            "CopyFullPath",
            "-",
            "Settings"
        ])
    );
}

// C#: ContextMenuModelBuilderTests.Build_NothingHidden_ListsEveryItemInXamlOrderWithZoomSubmenu
#[test]
fn build_nothing_hidden_lists_every_item_in_xaml_order_with_zoom_submenu() {
    let _en = english();
    let settings = hiding(&[]);
    let items = build_default_context(&settings);
    assert_eq!(
        ids(&items),
        strs(&[
            "Undo",
            "MoveToRecycleBin",
            "-",
            "Fit",
            "ZoomToLevel",
            "ZoomSubmenu",
            "Refresh",
            "-",
            "OpenFolder",
            "NextFolder",
            "PreviousFolder",
            "-",
            "ExternalEditor",
            "-",
            "CopyFileName",
            "CopyFullPath",
            "-",
            "Settings"
        ])
    );
    assert_eq!(
        ids(zoom_children(&items)),
        strs(&[
            "ZoomFitWidth",
            "ZoomFitWidth2",
            "ZoomFitHeight",
            "-",
            "Zoom.Preset.50",
            "Zoom.Preset.70",
            "Zoom.Preset.100",
            "Zoom.Preset.150",
            "Zoom.Preset.200",
            "Zoom.Preset.300",
            "Zoom.Preset.400",
            "Zoom.Custom",
            "-",
            "Zoom.AlsoSetClickLevel",
            "Zoom.SetCurrentAsClickLevel"
        ])
    );
}

// C#: ContextMenuModelBuilderTests.Build_NoPhoto_HidesCopyItemsAndTheirSeparator
#[test]
fn build_no_photo_hides_copy_items_and_their_separator() {
    let _en = english();
    let settings = hiding(&[]);
    let items = build(&context(&settings, false, false, Some(1.0))).expect("build");
    let top = ids(&items);
    assert!(!top.contains(&"CopyFileName".to_owned()));
    assert!(!top.contains(&"CopyFullPath".to_owned()));
    let start = top.iter().position(|i| i == "ExternalEditor").expect("ExternalEditor");
    assert_eq!(top[start..].to_vec(), strs(&["ExternalEditor", "-", "Settings"]));
}

// C#: ContextMenuModelBuilderTests.Build_OnlySettingsLeft_IsASingleItemWithoutSeparators
#[test]
fn build_only_settings_left_is_a_single_item_without_separators() {
    let _en = english();
    let all: Vec<Id> = ITEMS
        .iter()
        .filter(|i| !i.locked && i.parent.is_none())
        .map(|i| i.id)
        .collect();
    let settings = hiding(&all);
    let items = build_default_context(&settings);
    assert_eq!(ids(&items), strs(&["Settings"]));
}

// C#: ContextMenuModelBuilderTests.Build_ZoomSubmenuChildrenHidden_CollapsesSubmenuLikeWpf
#[test]
fn build_zoom_submenu_children_hidden_collapses_submenu_like_wpf() {
    let _en = english();
    let settings = hiding(&[
        Id::ZoomFitWidth,
        Id::ZoomFitWidth2,
        Id::ZoomFitHeight,
        Id::ZoomPresets,
        Id::ZoomLevelOptions,
    ]);
    let items = build_default_context(&settings);
    assert!(!ids(&items).contains(&"ZoomSubmenu".to_owned()));
}

// C#: ContextMenuModelBuilderTests.Build_LegacyFlagsConfig_UsesSameHiddenSetAsWpf
#[test]
fn build_legacy_flags_config_uses_same_hidden_set_as_wpf() {
    let _en = english();
    let mut settings = default_settings();
    settings.hidden_context_menu_items = None;
    settings.show_zoom_menu_items = true;
    settings.show_folder_menu_items = false;
    settings.show_recycle_menu_item = true;
    let items = build_default_context(&settings);
    assert_eq!(
        ids(&items),
        strs(&[
            "Undo",
            "MoveToRecycleBin",
            "-",
            "Fit",
            "ZoomToLevel",
            "ZoomSubmenu",
            "Refresh",
            "-",
            "ExternalEditor",
            "-",
            "CopyFileName",
            "CopyFullPath",
            "-",
            "Settings"
        ])
    );
}

// C#: ContextMenuModelBuilderTests.Build_EveryHiddenSubset_MatchesComputeAndNeverLeavesStraySeparators
// 17 unlocked ids, step 3 (about 43k subsets, 2 contexts); must finish in a few seconds.
#[test]
fn build_every_hidden_subset_matches_compute_and_never_leaves_stray_separators() {
    let _en = english();
    let unlocked: Vec<Id> = ITEMS.iter().filter(|i| !i.locked).map(|i| i.id).collect();
    assert_eq!(unlocked.len(), 17);
    let subsets: u32 = 1 << unlocked.len();
    let mut mask = 0u32;
    while mask < subsets {
        let hidden: HashSet<Id> = unlocked
            .iter()
            .enumerate()
            .filter(|(bit, _)| mask & (1 << bit) != 0)
            .map(|(_, id)| *id)
            .collect();
        for has_photo in [true, false] {
            let mut settings = default_settings();
            apply_hidden(&mut settings, hidden.iter().copied());
            let items = build(&context(&settings, has_photo, has_photo, Some(1.0))).expect("build");

            let expected = compute(&effective_hidden(&settings), ContextMenuContext { has_photo }, None);
            let expected_ids: Vec<String> = expected
                .entries
                .iter()
                .map(|e| e.item.map_or_else(|| "-".to_owned(), |i| i.name().to_owned()))
                .collect();
            assert_eq!(expected_ids, ids(&items), "mask {mask} has_photo {has_photo}");
            assert_no_stray_separators(&items);
            assert_no_empty_submenu(&items);
        }
        mask += 3;
    }
}

// C#: ContextMenuModelBuilderTests.Build_TextShortcutAndAutomationName_AreReadFromSettingsAndTr
#[test]
fn build_text_shortcut_and_automation_name_are_read_from_settings_and_tr() {
    let _en = english();
    let mut settings = hiding(&[]);
    settings.click_zoom_percent = 150;
    settings.shortcuts.click_zoom = "D3".to_owned();
    settings.shortcuts.refresh = String::new(); // cleared shortcut -> no accelerator text

    let items = build_default_context(&settings);

    let zoom_to = find(&items, "ZoomToLevel");
    assert_eq!(zoom_to.text, tr::main_menu_zoom_to_level(150));
    assert_eq!(
        zoom_to.automation_name.as_deref(),
        Some(tr::main_menu_zoom_to_level_automation_name(150).as_str())
    );
    assert_eq!(zoom_to.shortcut_text.as_deref(), Some("D3"));
    assert_eq!(find(&items, "Refresh").shortcut_text, None);
    assert_eq!(
        find(&items, "MoveToRecycleBin").shortcut_text.as_deref(),
        Some(settings.shortcuts.send_to_recycle_bin.as_str())
    );
    assert_eq!(
        find(&items, "Fit").shortcut_text.as_deref(),
        Some(settings.shortcuts.toggle_fit.as_str())
    );
    assert_eq!(
        find(&items, "NextFolder").shortcut_text.as_deref(),
        Some(settings.shortcuts.next_folder.as_str())
    );
    assert_eq!(
        find(&items, "PreviousFolder").shortcut_text.as_deref(),
        Some(settings.shortcuts.previous_folder.as_str())
    );
    assert_eq!(find(&items, "Undo").shortcut_text, None);
    assert_eq!(find(&items, "Undo").text, tr::main_menu_undo());
    assert_eq!(
        find(&items, "Settings").automation_name.as_deref(),
        Some(tr::main_menu_settings_automation_name().as_str())
    );
}

// C#: ContextMenuModelBuilderTests.Build_ZoomSubmenu_ChecksTheClickLevelPresetAndReflectsAlsoSetToggle
#[test]
fn build_zoom_submenu_checks_the_click_level_preset_and_reflects_also_set_toggle() {
    let _en = english();
    let mut settings = hiding(&[]);
    settings.click_zoom_percent = 200;
    settings.set_zoom_also_sets_click_level = false;
    settings.shortcuts.fit_width2 = "D4".to_owned();

    let items = build_default_context(&settings);

    let zoom = find(&items, "ZoomSubmenu");
    assert_eq!(zoom.kind, MenuItemKind::Submenu);
    let presets: Vec<&MenuItemModel> = zoom
        .children
        .iter()
        .filter(|c| c.id.starts_with("Zoom.Preset."))
        .collect();
    let checked: Vec<&str> = presets.iter().filter(|p| p.is_checked).map(|p| p.id.as_str()).collect();
    assert_eq!(checked, vec!["Zoom.Preset.200"]);
    assert!(presets.iter().all(|p| p.kind == MenuItemKind::Toggle));
    assert_eq!(
        presets
            .iter()
            .find(|p| p.id == "Zoom.Preset.150")
            .expect("preset 150")
            .text,
        tr::main_menu_click_zoom_level_preset(150)
    );
    assert!(!find(&zoom.children, "Zoom.AlsoSetClickLevel").is_checked);
    assert_eq!(
        find(&zoom.children, "ZoomFitWidth2").shortcut_text.as_deref(),
        Some("D4")
    );
    assert_eq!(
        find(&zoom.children, "Zoom.Custom").text,
        tr::main_menu_click_zoom_level_custom()
    );
}

// C#: ContextMenuModelBuilderTests.Build_CustomClickLevelNotAPreset_ChecksNoPreset
#[test]
fn build_custom_click_level_not_a_preset_checks_no_preset() {
    let _en = english();
    let mut settings = hiding(&[]);
    settings.click_zoom_percent = 123;

    let items = build_default_context(&settings);

    assert!(
        !zoom_children(&items)
            .iter()
            .any(|c| c.is_checked && c.id.starts_with("Zoom.Preset."))
    );
}

// C#: ContextMenuModelBuilderTests.Build_SetCurrentZoom_IsDisabledInFitAndEnabledOtherwise
#[test]
fn build_set_current_zoom_is_disabled_in_fit_and_enabled_otherwise() {
    let _en = english();
    let settings = hiding(&[]);

    let fit = build(&context(&settings, true, true, None)).expect("build");
    let zoomed = build(&context(&settings, true, true, Some(1.5))).expect("build");

    assert!(!find(zoom_children(&fit), "Zoom.SetCurrentAsClickLevel").is_enabled);
    assert!(find(zoom_children(&zoomed), "Zoom.SetCurrentAsClickLevel").is_enabled);
}

// C#: ContextMenuModelBuilderTests.Build_UndoAndExternalEditor_StayEnabledLikeWpf
#[test]
fn build_undo_and_external_editor_stay_enabled_like_wpf() {
    let _en = english();
    let settings = default_settings();
    let ctx = ContextMenuBuildContext {
        settings: &settings,
        has_image: true,
        can_undo: false,
        is_fit: true,
        effective_zoom: None,
        can_open_in_external_editor: false,
        can_copy_path: true,
        is_compare_visible: true,
    };

    let items = build(&ctx).expect("build");

    assert!(find(&items, "Undo").is_enabled);
    assert!(find(&items, "ExternalEditor").is_enabled);
}

// C#: ContextMenuModelBuilderTests.BuildToolsMenu_WithImages_ListsSixCommandsInToolbarOrder
#[test]
fn build_tools_menu_with_images_lists_six_commands_in_toolbar_order() {
    let _en = english();
    let items = build_tools_menu(true);

    assert_eq!(
        ids(&items),
        strs(&[
            "Tools.Recovery",
            "Tools.Diagnostics",
            "Tools.Benchmark",
            "Tools.ClearCache",
            "Tools.RemoveNumberedDuplicates",
            "Tools.RemoveOriginalDuplicates"
        ])
    );
    for item in &items {
        assert_eq!(item.kind, MenuItemKind::Command);
        assert!(item.is_enabled);
        assert!(!item.text.trim().is_empty());
        assert!(item.automation_name.as_deref().is_some_and(|a| !a.trim().is_empty()));
    }
    assert_eq!(items[0].text, tr::main_tools_recovery());
}

// C#: ContextMenuModelBuilderTests.BuildToolsMenu_WithoutImages_DisablesOnlyTheDuplicateRemovals
#[test]
fn build_tools_menu_without_images_disables_only_the_duplicate_removals() {
    let _en = english();
    let items = build_tools_menu(false);

    let disabled: Vec<&str> = items.iter().filter(|i| !i.is_enabled).map(|i| i.id.as_str()).collect();
    assert_eq!(
        disabled,
        vec!["Tools.RemoveNumberedDuplicates", "Tools.RemoveOriginalDuplicates"]
    );
}

// C#: ContextMenuModelBuilderTests.Build_AllIdsAreUniqueAcrossTheWholeTree
#[test]
fn build_all_ids_are_unique_across_the_whole_tree() {
    let _en = english();
    let settings = hiding(&[]);
    let items = build_default_context(&settings);

    let mut all = Vec::new();
    flatten(&items, &mut all);
    let ids: Vec<&str> = all
        .iter()
        .filter(|i| i.kind != MenuItemKind::Separator)
        .map(|i| i.id.as_str())
        .collect();
    let distinct: HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), distinct.len());
}

// ---------------------------------------------------------------------------------------------------------------------
// New tests
// ---------------------------------------------------------------------------------------------------------------------

// new: a NaN effective zoom is "Fit" through zoom_rules, so Zoom.SetCurrentAsClickLevel is disabled
// (builder-level twin of W8-01d's NaN case; NaN cannot come from JSON, so no golden case).
#[test]
fn build_non_finite_zoom_disables_set_current() {
    let _en = english();
    let settings = hiding(&[]);
    let items = build(&context(&settings, true, true, Some(f64::NAN))).expect("build");
    assert!(!find(zoom_children(&items), "Zoom.SetCurrentAsClickLevel").is_enabled);
}

// new: a list of hidden ids written with apply_hidden reads back the same set through effective_hidden. The set is
// then used by the builder, so the round trip is exercised end to end (the build call is what fails on the stub).
#[test]
fn build_test_settings_round_trip_through_apply_hidden() {
    let _en = english();
    let list = [Id::Undo, Id::Fit, Id::ZoomFitHeight, Id::OpenFolder, Id::CopyFullPath];
    let settings = hiding(&list);
    let expected: HashSet<Id> = list.into_iter().collect();
    assert_eq!(effective_hidden(&settings), expected);

    let items = build_default_context(&settings);
    let top = ids(&items);
    for hidden in ["Undo", "Fit", "OpenFolder", "CopyFullPath"] {
        assert!(!top.contains(&hidden.to_owned()), "{hidden} must be hidden");
    }
    assert!(!ids(zoom_children(&items)).contains(&"ZoomFitHeight".to_owned()));
}

// ---------------------------------------------------------------------------------------------------------------------
// Golden `menu-model`
// ---------------------------------------------------------------------------------------------------------------------

fn id_by_name(name: &str) -> Id {
    Id::ALL
        .iter()
        .copied()
        .find(|i| i.name() == name)
        .unwrap_or_else(|| panic!("unknown id {name}"))
}

fn kind_name(kind: MenuItemKind) -> &'static str {
    match kind {
        MenuItemKind::Command => "Command",
        MenuItemKind::Toggle => "Toggle",
        MenuItemKind::Submenu => "Submenu",
        MenuItemKind::Separator => "Separator",
    }
}

fn tree_json(items: &[MenuItemModel]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|i| {
                json!({
                    "id": i.id,
                    "kind": kind_name(i.kind),
                    "text": i.text,
                    "shortcut": i.shortcut_text,
                    "enabled": i.is_enabled,
                    "checked": i.is_checked,
                    "automation": i.automation_name,
                    "children": tree_json(&i.children),
                })
            })
            .collect(),
    )
}

fn render(items: &[MenuItemModel]) -> String {
    items
        .iter()
        .map(|i| {
            if i.kind == MenuItemKind::Separator {
                "|"
            } else {
                i.id.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Settings of one golden `build` case (see `tests/golden-inputs/menu-model.json`, `_comment`).
fn golden_settings(spec: &Value) -> AppSettings {
    let mut settings = default_settings();
    match &spec["hidden"] {
        Value::Array(names) => {
            let hidden: Vec<Id> = names.iter().map(|n| id_by_name(n.as_str().expect("name"))).collect();
            apply_hidden(&mut settings, hidden);
        }
        Value::Null => {
            settings.hidden_context_menu_items = None;
            settings.show_zoom_menu_items = spec["show_zoom"].as_bool().expect("show_zoom");
            settings.show_folder_menu_items = spec["show_folder"].as_bool().expect("show_folder");
            settings.show_recycle_menu_item = spec["show_recycle"].as_bool().expect("show_recycle");
        }
        _ => {} // "default"
    }
    settings.click_zoom_percent = i32::try_from(spec["click_zoom_percent"].as_i64().expect("percent")).expect("i32");
    settings.set_zoom_also_sets_click_level = spec["set_zoom_also_sets_click_level"].as_bool().expect("also");
    if let Some(shortcuts) = spec["shortcuts"].as_object() {
        for (name, value) in shortcuts {
            let slot = settings
                .shortcuts
                .get_mut(name)
                .unwrap_or_else(|| panic!("no shortcut {name}"));
            *slot = value.as_str().expect("shortcut text").to_owned();
        }
    }
    settings
}

fn load_inputs() -> Value {
    let path = golden::repo_root()
        .join("tests")
        .join("golden-inputs")
        .join("menu-model.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("parse inputs")
}

fn assert_case(case: &Value, items: &[MenuItemModel], label: &str) {
    assert_eq!(
        case["render"].as_str().expect("render"),
        render(items),
        "{label}: render"
    );
    assert_eq!(case["tree"], tree_json(items), "{label}: tree");
}

// new: golden `menu-model`, every case (build, mask, submenu_mask, tools) compares render and the full tree with the oracle.
#[test]
fn golden_menu_model_all_cases_match_the_oracle() {
    let _en = english();
    let golden = golden::load("menu-model");
    let inputs = load_inputs();

    // The bit orders in `meta` must equal the table order before the masks are replayed.
    let meta = golden.cases.iter().find(|c| c["kind"] == "meta").expect("meta case");
    let names = |key: &str| -> Vec<Id> {
        meta[key]
            .as_array()
            .expect(key)
            .iter()
            .map(|v| id_by_name(v.as_str().expect("name")))
            .collect()
    };
    let top_hideable = names("top_level_hideable");
    let zoom_children = names("zoom_children");
    let table_top: Vec<Id> = ITEMS
        .iter()
        .filter(|i| i.parent.is_none() && !i.locked)
        .map(|i| i.id)
        .collect();
    let table_zoom: Vec<Id> = ITEMS
        .iter()
        .filter(|i| i.parent == Some(Id::ZoomSubmenu))
        .map(|i| i.id)
        .collect();
    assert_eq!(top_hideable, table_top);
    assert_eq!(zoom_children, table_zoom);

    let mut replayed = 0;
    for case in &golden.cases {
        match case["kind"].as_str().expect("kind") {
            "meta" => {}
            "build" => {
                let name = case["name"].as_str().expect("name");
                let spec = inputs["build"]
                    .as_array()
                    .expect("build")
                    .iter()
                    .find(|c| c["name"] == name)
                    .unwrap_or_else(|| panic!("no input case {name}"));
                let settings = golden_settings(&spec["settings"]);
                let ctx = context(
                    &settings,
                    spec["has_image"].as_bool().expect("has_image"),
                    spec["can_copy_path"].as_bool().expect("can_copy_path"),
                    spec["effective_zoom"].as_f64(),
                );
                let items = build(&ctx).expect("build");
                assert_case(case, &items, name);
                replayed += 1;
            }
            kind @ ("mask" | "submenu_mask") => {
                let mask = case["mask"].as_u64().expect("mask");
                let has_photo = case["has_photo"].as_bool().expect("has_photo");
                let bits = if kind == "mask" { &top_hideable } else { &zoom_children };
                let hidden: Vec<Id> = bits
                    .iter()
                    .enumerate()
                    .filter(|(bit, _)| mask & (1 << bit) != 0)
                    .map(|(_, id)| *id)
                    .collect();
                let settings = hiding(&hidden);
                let items = build(&context(&settings, has_photo, has_photo, Some(1.0))).expect("build");
                assert_case(case, &items, &format!("{kind} {mask} {has_photo}"));
                replayed += 1;
            }
            "tools" => {
                let has_images = case["has_images"].as_bool().expect("has_images");
                let items = build_tools_menu(has_images);
                assert_eq!(case["tree"], tree_json(&items), "tools {has_images}");
                replayed += 1;
            }
            other => panic!("unknown golden kind {other}"),
        }
    }
    let expected = inputs["build"].as_array().expect("build").len()
        + 2 * inputs["masks"].as_array().expect("masks").len()
        + 2 * inputs["submenu_masks"].as_array().expect("submenu_masks").len()
        + inputs["tools"].as_array().expect("tools").len();
    assert_eq!(replayed, expected);
}
