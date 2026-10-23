//! Port of `PhotoReview.Core/Settings/ContextMenuLayout.cs` tests (baseline 7eaee2a2). Golden: `tests/golden/context-menu.json`.

use std::collections::HashSet;

use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::context_menu::{
    ContextMenuContext, ContextMenuItemId as Id, ContextMenuLayout, DEFAULT_HIDDEN, ITEMS, apply_hidden, compute,
    effective_hidden, item_info, legacy_hidden, parse_hidden, to_stored,
};
use pr_test_support::golden::{self, bool_of, str_of};
use serde_json::Value;

const WITH_PHOTO: ContextMenuContext = ContextMenuContext {
    has_photo: true,
    include_shell_only: false,
};
const NO_PHOTO: ContextMenuContext = ContextMenuContext {
    has_photo: false,
    include_shell_only: false,
};
const SHELL_WITH_PHOTO: ContextMenuContext = ContextMenuContext {
    has_photo: true,
    include_shell_only: true,
};

fn hide(ids: &[Id]) -> HashSet<Id> {
    ids.iter().copied().collect()
}

fn render(layout: &ContextMenuLayout) -> String {
    layout
        .entries
        .iter()
        .map(|e| e.item.map_or("|", |id| id.name()))
        .collect::<Vec<_>>()
        .join(",")
}

fn all_top_level() -> Vec<Id> {
    ITEMS.iter().filter(|i| i.parent.is_none()).map(|i| i.id).collect()
}

fn hideable_top_level() -> Vec<Id> {
    ITEMS
        .iter()
        .filter(|i| i.parent.is_none() && !i.locked)
        .map(|i| i.id)
        .collect()
}

fn id_of(name: &str) -> Id {
    Id::ALL
        .iter()
        .copied()
        .find(|i| i.name() == name)
        .unwrap_or_else(|| panic!("unknown id {name}"))
}

fn ids_of(v: &Value) -> Vec<Id> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|n| id_of(n.as_str().expect("string")))
        .collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|n| n.as_str().expect("string").to_owned())
        .collect()
}

fn names_of(set: &HashSet<Id>) -> Vec<String> {
    to_stored(set.iter().copied())
}

fn kind_cases(kind: &str) -> Vec<Value> {
    let g = golden::load("context-menu");
    assert!(
        g.csharp_commit.starts_with("7eaee2a2"),
        "golden baseline {}",
        g.csharp_commit
    );
    g.cases.into_iter().filter(|c| c["kind"] == kind).collect()
}

fn entries_json(layout: &ContextMenuLayout) -> Value {
    Value::Array(
        layout
            .entries
            .iter()
            .map(|e| {
                Value::Array(vec![
                    e.item.map_or(Value::Null, |i| Value::String(i.name().to_owned())),
                    Value::from(e.group),
                ])
            })
            .collect(),
    )
}

// C#: ContextMenuLayoutTests.Nothing_hidden_shows_every_item_with_a_separator_between_each_group
#[test]
fn nothing_hidden_shows_every_item_with_a_separator_between_each_group() {
    let layout = compute(&hide(&[]), WITH_PHOTO, None);
    assert_eq!(
        "Undo,MoveToRecycleBin,|,Fit,ZoomToLevel,ZoomSubmenu,Refresh,|,OpenFolder,NextFolder,PreviousFolder,|,ExternalEditor,|,CopyFileName,CopyFullPath,|,Settings",
        render(&layout)
    );
}

#[test]
fn rename_file_is_visible_only_in_the_shell_with_a_photo() {
    assert!(item_info(Id::RenameFile).shell_only);
    assert!(!compute(&hide(&[]), WITH_PHOTO, None).is_visible(Id::RenameFile));
    assert!(!compute(&hide(&[]), NO_PHOTO, None).is_visible(Id::RenameFile));
    assert!(compute(&hide(&[]), SHELL_WITH_PHOTO, None).is_visible(Id::RenameFile));
}

// C#: ContextMenuLayoutTests.Item_ids_are_stable_names_and_the_table_has_no_duplicates
#[test]
fn item_ids_are_stable_names_and_the_table_has_no_duplicates() {
    assert_eq!(Id::ALL.len(), ITEMS.len());
    let distinct: HashSet<Id> = ITEMS.iter().map(|i| i.id).collect();
    assert_eq!(ITEMS.len(), distinct.len());
    let names: Vec<&str> = Id::ALL.iter().map(|i| i.name()).collect();
    assert_eq!(
        vec![
            "Undo",
            "MoveToRecycleBin",
            "Fit",
            "ZoomToLevel",
            "ZoomSubmenu",
            "OpenFolder",
            "NextFolder",
            "PreviousFolder",
            "ExternalEditor",
            "CopyFileName",
            "CopyFullPath",
            "Settings",
            "ZoomFitWidth",
            "ZoomFitWidth2",
            "ZoomFitHeight",
            "ZoomPresets",
            "ZoomLevelOptions",
            "Refresh",
            "RenameFile"
        ],
        names
    );
    for id in Id::ALL {
        assert_eq!(id, item_info(id).id);
    }
}

// C#: ContextMenuLayoutTests.Copy_items_need_an_open_photo_on_top_of_the_user_flag
#[test]
fn copy_items_need_an_open_photo_on_top_of_the_user_flag() {
    let no_photo = compute(&hide(&[]), NO_PHOTO, None);
    assert!(!no_photo.is_visible(Id::CopyFileName));
    assert!(!no_photo.is_visible(Id::CopyFullPath));
    assert!(!no_photo.has_separator_before(5));
    assert!(compute(&hide(&[]), WITH_PHOTO, None).is_visible(Id::CopyFileName));
    let hidden_by_user = compute(&hide(&[Id::CopyFileName]), WITH_PHOTO, None);
    assert!(!hidden_by_user.is_visible(Id::CopyFileName));
    assert!(hidden_by_user.is_visible(Id::CopyFullPath));
}

// C#: ContextMenuLayoutTests.An_emptied_group_takes_its_separator_with_it
#[test]
fn an_emptied_group_takes_its_separator_with_it() {
    let layout = compute(
        &hide(&[Id::OpenFolder, Id::NextFolder, Id::PreviousFolder]),
        WITH_PHOTO,
        None,
    );
    assert_eq!(
        "Undo,MoveToRecycleBin,|,Fit,ZoomToLevel,ZoomSubmenu,Refresh,|,ExternalEditor,|,CopyFileName,CopyFullPath,|,Settings",
        render(&layout)
    );
}

// C#: ContextMenuLayoutTests.First_group_never_starts_with_a_separator
#[test]
fn first_group_never_starts_with_a_separator() {
    let layout = compute(&hide(&[Id::Undo, Id::MoveToRecycleBin]), NO_PHOTO, None);
    assert_eq!(
        "Fit,ZoomToLevel,ZoomSubmenu,Refresh,|,OpenFolder,NextFolder,PreviousFolder,|,ExternalEditor,|,Settings",
        render(&layout)
    );
}

// C#: ContextMenuLayoutTests.Hiding_everything_leaves_only_Settings_without_any_separator
#[test]
fn hiding_everything_leaves_only_settings_without_any_separator() {
    let layout = compute(&hide(&all_top_level()), WITH_PHOTO, None);
    assert_eq!("Settings", render(&layout));
}

// C#: ContextMenuLayoutTests.Settings_cannot_be_hidden_even_when_listed
#[test]
fn settings_cannot_be_hidden_even_when_listed() {
    let layout = compute(&hide(&[Id::Settings]), NO_PHOTO, None);
    assert!(layout.is_visible(Id::Settings));
    assert!(parse_hidden(&[Some("Settings".to_owned())]).is_empty());
}

// C#: ContextMenuLayoutTests.Menu_always_has_at_least_one_item_for_every_combination_of_flags
#[test]
fn menu_always_has_at_least_one_item_for_every_combination_of_flags() {
    let hideable = hideable_top_level();
    assert_eq!(13, hideable.len());
    for mask in 0..(1u32 << hideable.len()) {
        let hidden: HashSet<Id> = hideable
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, id)| *id)
            .collect();
        for context in [WITH_PHOTO, NO_PHOTO] {
            let entries = compute(&hidden, context, None).entries;
            assert!(!entries.is_empty());
            assert!(entries[0].item.is_some());
            assert!(entries[entries.len() - 1].item.is_some());
            for i in 1..entries.len() {
                assert!(!(entries[i].item.is_none() && entries[i - 1].item.is_none()));
            }
        }
    }
}

// C#: ContextMenuLayoutTests.Zoom_submenu_collapses_when_the_user_hides_it_or_all_of_its_children
#[test]
fn zoom_submenu_collapses_when_the_user_hides_it_or_all_of_its_children() {
    assert!(!compute(&hide(&[Id::ZoomSubmenu]), WITH_PHOTO, None).is_visible(Id::ZoomSubmenu));
    let all_children = hide(&[
        Id::ZoomFitWidth,
        Id::ZoomFitWidth2,
        Id::ZoomFitHeight,
        Id::ZoomPresets,
        Id::ZoomLevelOptions,
    ]);
    assert!(!compute(&all_children, WITH_PHOTO, None).is_visible(Id::ZoomSubmenu));
    assert!(
        compute(&all_children, WITH_PHOTO, Some(Id::ZoomSubmenu))
            .entries
            .is_empty()
    );
}

// C#: ContextMenuLayoutTests.Zoom_submenu_children_are_toggled_individually_and_separators_follow_the_groups
#[test]
fn zoom_submenu_children_are_toggled_individually_and_separators_follow_the_groups() {
    let sub = Some(Id::ZoomSubmenu);
    let all = compute(&hide(&[]), WITH_PHOTO, sub);
    assert_eq!(
        "ZoomFitWidth,ZoomFitWidth2,ZoomFitHeight,|,ZoomPresets,|,ZoomLevelOptions",
        render(&all)
    );
    let no_fit_width2 = compute(&hide(&[Id::ZoomFitWidth2]), WITH_PHOTO, sub);
    assert_eq!(
        "ZoomFitWidth,ZoomFitHeight,|,ZoomPresets,|,ZoomLevelOptions",
        render(&no_fit_width2)
    );
    let only_options = compute(
        &hide(&[Id::ZoomFitWidth, Id::ZoomFitWidth2, Id::ZoomFitHeight, Id::ZoomPresets]),
        WITH_PHOTO,
        sub,
    );
    assert_eq!("ZoomLevelOptions", render(&only_options));
}

// C#: ContextMenuLayoutTests.Legacy_flags_map_to_the_hidden_set
#[test]
fn legacy_flags_map_to_the_hidden_set() {
    let defaults = legacy_hidden(&AppSettings::default());
    assert_eq!(defaults, DEFAULT_HIDDEN.iter().copied().collect::<HashSet<_>>());

    let all_on = AppSettings {
        show_zoom_menu_items: true,
        show_folder_menu_items: true,
        show_recycle_menu_item: true,
        ..AppSettings::default()
    };
    assert!(legacy_hidden(&all_on).is_empty());

    let folder_off = AppSettings {
        show_zoom_menu_items: true,
        show_folder_menu_items: false,
        show_recycle_menu_item: true,
        ..AppSettings::default()
    };
    assert_eq!(
        legacy_hidden(&folder_off),
        hide(&[Id::OpenFolder, Id::NextFolder, Id::PreviousFolder])
    );
}

// C#: ContextMenuLayoutTests.EffectiveHidden_prefers_the_stored_list_and_treats_empty_as_show_everything
#[test]
fn effective_hidden_prefers_the_stored_list_and_treats_empty_as_show_everything() {
    let with_list = AppSettings {
        show_zoom_menu_items: false,
        hidden_context_menu_items: Some(vec![]),
        ..AppSettings::default()
    };
    assert!(effective_hidden(&with_list).is_empty());
    let legacy_only = AppSettings {
        show_zoom_menu_items: false,
        hidden_context_menu_items: None,
        ..AppSettings::default()
    };
    assert!(effective_hidden(&legacy_only).contains(&Id::Fit));
}

// C#: ContextMenuLayoutTests.ApplyHidden_stores_names_in_menu_order_and_mirrors_the_legacy_flags
#[test]
fn apply_hidden_stores_names_in_menu_order_and_mirrors_the_legacy_flags() {
    let mut settings = AppSettings::default();
    apply_hidden(&mut settings, [Id::CopyFullPath, Id::Fit, Id::Settings, Id::Fit]);
    assert_eq!(
        Some(vec![Some("Fit".to_owned()), Some("CopyFullPath".to_owned())]),
        settings.hidden_context_menu_items
    );
    assert!(settings.show_zoom_menu_items);
    assert!(settings.show_folder_menu_items);
    assert!(settings.show_recycle_menu_item);

    apply_hidden(&mut settings, DEFAULT_HIDDEN);
    assert!(!settings.show_zoom_menu_items);
    assert!(settings.show_folder_menu_items);
    assert!(!settings.show_recycle_menu_item);
}

// ---- golden ----

#[test]
fn golden_items() {
    let cases = kind_cases("items");
    assert_eq!(1, cases.len());
    let c = &cases[0];
    let rows = c["items"].as_array().expect("items");
    assert_eq!(ITEMS.len(), rows.len());
    for (row, info) in rows.iter().zip(ITEMS.iter()) {
        assert_eq!(str_of(row, "id"), info.id.name());
        assert_eq!(row["value"].as_i64().unwrap(), info.id as i64);
        assert_eq!(row["parent"].as_str(), info.parent.map(|p| p.name()));
        assert_eq!(row["group"].as_i64().unwrap(), i64::from(info.group));
        assert_eq!(bool_of(row, "locked"), info.locked);
        assert_eq!(bool_of(row, "needs_photo"), info.needs_photo);
    }
    let names: Vec<String> = Id::ALL.iter().map(|i| i.name().to_owned()).collect();
    assert_eq!(strings(&c["names"]), names);
    let values: Vec<i64> = c["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(values, Id::ALL.iter().map(|i| *i as i64).collect::<Vec<_>>());
    assert_eq!(strings(&c["default_hidden"]), to_stored(DEFAULT_HIDDEN));
}

#[test]
fn golden_parse_hidden() {
    let cases = kind_cases("parse_hidden");
    assert_eq!(10, cases.len());
    for c in cases {
        let names: Vec<Option<String>> = c["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().map(str::to_owned))
            .collect();
        assert_eq!(strings(&c["stored"]), names_of(&parse_hidden(&names)), "{c}");
    }
}

#[test]
fn golden_single_name() {
    let cases = kind_cases("single_name");
    assert_eq!(77, cases.len());
    for c in cases {
        let name = str_of(&c, "name").to_owned();
        assert_eq!(strings(&c["stored"]), names_of(&parse_hidden(&[Some(name)])), "{c}");
    }
}

#[test]
fn golden_legacy() {
    let cases = kind_cases("legacy");
    assert_eq!(8, cases.len());
    for c in cases {
        let s = AppSettings {
            show_zoom_menu_items: bool_of(&c, "zoom"),
            show_folder_menu_items: bool_of(&c, "folder"),
            show_recycle_menu_item: bool_of(&c, "recycle"),
            hidden_context_menu_items: None,
            ..AppSettings::default()
        };
        assert_eq!(strings(&c["hidden"]), names_of(&legacy_hidden(&s)), "{c}");
        assert_eq!(
            strings(&c["effective_with_null_list"]),
            names_of(&effective_hidden(&s)),
            "{c}"
        );
    }
}

#[test]
fn golden_effective() {
    let cases = kind_cases("effective");
    assert_eq!(12, cases.len());
    for c in cases {
        let input = &c["input"];
        let list = input["list"].as_array().map(|a| {
            a.iter()
                .map(|n| n.as_str().map(str::to_owned))
                .collect::<Vec<Option<String>>>()
        });
        let s = AppSettings {
            show_zoom_menu_items: bool_of(input, "zoom"),
            show_folder_menu_items: bool_of(input, "folder"),
            show_recycle_menu_item: bool_of(input, "recycle"),
            hidden_context_menu_items: list,
            ..AppSettings::default()
        };
        assert_eq!(strings(&c["hidden"]), names_of(&effective_hidden(&s)), "{c}");
    }
}

#[test]
fn golden_to_stored() {
    let cases = kind_cases("to_stored");
    assert_eq!(5, cases.len());
    for c in cases {
        assert_eq!(strings(&c["stored"]), to_stored(ids_of(&c["ids"])), "{c}");
    }
}

#[test]
fn golden_apply_hidden() {
    let cases = kind_cases("apply_hidden");
    assert_eq!(11, cases.len());
    for c in cases {
        let mut s = AppSettings::default();
        apply_hidden(&mut s, ids_of(&c["ids"]));
        let stored: Vec<String> = s
            .hidden_context_menu_items
            .clone()
            .expect("always Some")
            .into_iter()
            .map(|n| n.expect("no null"))
            .collect();
        assert_eq!(strings(&c["stored"]), stored, "{c}");
        assert_eq!(bool_of(&c, "show_zoom"), s.show_zoom_menu_items, "{c}");
        assert_eq!(bool_of(&c, "show_folder"), s.show_folder_menu_items, "{c}");
        assert_eq!(bool_of(&c, "show_recycle"), s.show_recycle_menu_item, "{c}");
    }
}

#[test]
fn golden_compute_top_level() {
    let hideable_cases = kind_cases("top_level_hideable");
    assert_eq!(1, hideable_cases.len());
    let hideable = ids_of(&hideable_cases[0]["ids"]);
    assert_eq!(hideable, hideable_top_level());
    let cases = kind_cases("compute");
    assert_eq!(348, cases.len());
    for c in cases {
        let mask = c["mask"].as_u64().unwrap();
        let hidden: HashSet<Id> = hideable
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, id)| *id)
            .collect();
        let ctx = ContextMenuContext {
            has_photo: bool_of(&c, "has_photo"),
            include_shell_only: false,
        };
        let layout = compute(&hidden, ctx, None);
        assert_eq!(str_of(&c, "render"), render(&layout), "{c}");
        assert_eq!(c["entries"], entries_json(&layout), "{c}");
    }
}

#[test]
fn golden_compute_submenu() {
    let child_cases = kind_cases("submenu_children");
    let children = ids_of(&child_cases[0]["ids"]);
    let cases = kind_cases("compute_submenu");
    assert_eq!(32, cases.len());
    for c in cases {
        let mask = c["mask"].as_u64().unwrap();
        let hidden: HashSet<Id> = children
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, id)| *id)
            .collect();
        let layout = compute(&hidden, WITH_PHOTO, Some(Id::ZoomSubmenu));
        assert_eq!(str_of(&c, "render"), render(&layout), "{c}");
        assert_eq!(c["entries"], entries_json(&layout), "{c}");
        let top = compute(&hidden, WITH_PHOTO, None);
        assert_eq!(
            bool_of(&c, "submenu_visible_on_top_level"),
            top.is_visible(Id::ZoomSubmenu),
            "{c}"
        );
    }
}

#[test]
fn golden_compute_misc() {
    let cases = kind_cases("compute_misc");
    assert_eq!(3, cases.len());
    for c in cases {
        let hidden: HashSet<Id> = ids_of(&c["hidden"]).into_iter().collect();
        assert_eq!(
            str_of(&c, "top_photo"),
            render(&compute(&hidden, WITH_PHOTO, None)),
            "{c}"
        );
        assert_eq!(
            str_of(&c, "submenu"),
            render(&compute(&hidden, WITH_PHOTO, Some(Id::ZoomSubmenu))),
            "{c}"
        );
        assert_eq!(
            bool_of(&c, "settings_visible"),
            compute(&hidden, NO_PHOTO, None).is_visible(Id::Settings),
            "{c}"
        );
    }
}

#[test]
fn golden_layout_queries() {
    let cases = kind_cases("layout_queries");
    assert_eq!(1, cases.len());
    let c = &cases[0];
    let layout = compute(&hide(&[]), WITH_PHOTO, None);
    let visible: Vec<String> = Id::ALL
        .iter()
        .filter(|i| layout.is_visible(**i))
        .map(|i| i.name().to_owned())
        .collect();
    assert_eq!(strings(&c["visible"]), visible);
    let separators: Vec<i64> = (0..8)
        .filter(|g| layout.has_separator_before(*g))
        .map(i64::from)
        .collect();
    let expected: Vec<i64> = c["separator_before"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    assert_eq!(expected, separators);
}

// Verifier diff: .NET 10 char.IsWhiteSpace is false for U+001C..U+001F, so Enum.TryParse rejects them.
#[test]
fn c0_controls_1c_to_1f_are_not_whitespace() {
    use pr_core::settings::context_menu::parse_hidden;
    for s in ["\u{1c}Undo", "Undo\u{1f}", "\u{1d}1", "Fit,\u{1e}Undo"] {
        let got = parse_hidden(&[Some(s.to_string())]);
        assert!(got.is_empty(), "{s:?} must not parse");
    }
}

// Verifier diff (repair 2): Enum.TryParse TrimStart()s only; Int32.TryParse allows trailing ASCII ws, then NULs.
#[test]
fn numeric_path_trailing_whitespace_rules() {
    use pr_core::settings::context_menu::parse_hidden;
    let parse = |s: &str| parse_hidden(&[Some(s.to_string())]);
    for s in ["3\u{a0}", "3\u{3000}", "3\u{85}", "-0\0 ", "+17\0\t", "0\0 \0"] {
        assert!(parse(s).is_empty(), "{s:?} must not parse");
    }
    // ASCII space is trailing white space Int32.TryParse allows: these numeric forms do parse.
    for s in ["3 ", "+1 ", "007 ", "-0 "] {
        assert!(!parse(s).is_empty(), "{s:?} must parse");
    }
    for (s, id) in [
        ("0 \0", Id::Undo),
        ("3\t\r\n", Id::ZoomToLevel),
        ("\u{3000}3\0\0", Id::ZoomToLevel),
        ("Fit\u{3000}", Id::Fit),
    ] {
        assert_eq!(parse(s), hide(&[id]), "{s:?}");
    }
}
