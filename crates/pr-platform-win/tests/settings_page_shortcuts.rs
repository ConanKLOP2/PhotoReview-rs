use std::collections::{BTreeMap, BTreeSet};

use pr_app::presenters::settings_field_map::shortcut_properties;
use pr_core::settings::shortcuts::ShortcutMappings;
use pr_platform_win::dialogs::settings::pages::shortcuts::{
    ACTION_ROWS, MARKER, NAVIGATION_ROWS, VIEW_ROWS, all_rows, find_problems,
};

// C#: SettingsShortcutsPageTests.Page_HasOneKeyBoxPerFieldMapShortcut_OnlyTheOptionalOnesHaveAClearButton
#[test]
fn page_has_one_key_box_per_field_map_shortcut_only_optional_rows_have_clear_buttons() {
    let rows: Vec<_> = all_rows().collect();
    let properties: BTreeSet<_> = rows.iter().map(|row| row.property).collect();
    let expected: BTreeSet<_> = shortcut_properties().iter().copied().collect();

    assert_eq!(rows.len(), 27);
    assert_eq!(properties, expected);
    assert_eq!(rows.iter().filter(|row| row.clear_tip.is_some()).count(), 14);
    for row in rows {
        assert_eq!(row.clear_tip.is_some(), ShortcutMappings::is_optional(row.property));
    }
}

// C#: SettingsShortcutsPageTests.RowOrder_IsTheDocumentedReadingOrder_OfThe27Commands
#[test]
fn row_order_is_the_documented_reading_order_of_the_27_commands() {
    assert_eq!(NAVIGATION_ROWS.len(), 8);
    assert_eq!(VIEW_ROWS.len(), 13);
    assert_eq!(ACTION_ROWS.len(), 6);
    let rows: Vec<_> = all_rows().map(|row| row.property).collect();
    let expected: Vec<_> = NAVIGATION_ROWS
        .iter()
        .chain(VIEW_ROWS.iter())
        .chain(ACTION_ROWS.iter())
        .map(|row| row.property)
        .collect();
    assert_eq!(rows, expected);
}

// C#: SettingsShortcutsPageTests.Labels_AreTheCommandNames_InEnglishAndVietnamese
#[test]
fn row_tables_have_labels_and_optional_clear_tips() {
    let rows: Vec<_> = all_rows().collect();
    assert_eq!(MARKER, "⚠ ");
    assert!(rows.iter().all(|row| !(row.label)().is_empty()));
    assert!(
        rows.iter()
            .filter_map(|row| row.clear_tip.map(|tip| tip()))
            .all(|tip| !tip.is_empty())
    );
}

// C#: SettingsShortcutsPageTests.FindProblems_FlagsEmptyMandatory_AndInvalidNames_ButNotEmptyOptional
#[test]
fn find_problems_flags_missing_mandatory_and_invalid_names_but_allows_empty_optional() {
    let mut texts = BTreeMap::new();
    for property in shortcut_properties() {
        texts.insert((*property).to_owned(), "A".to_owned());
    }
    let mandatory = shortcut_properties()
        .iter()
        .copied()
        .find(|property| !ShortcutMappings::is_optional(property))
        .expect("test data has mandatory shortcut properties");
    let optional = shortcut_properties()
        .iter()
        .copied()
        .find(|property| ShortcutMappings::is_optional(property))
        .expect("test data has optional shortcut properties");
    texts.insert(mandatory.to_owned(), String::new());
    texts.insert(optional.to_owned(), String::new());
    let problems = find_problems(&texts);
    assert!(problems.contains(mandatory));
    assert!(!problems.contains(optional));

    texts.insert(mandatory.to_owned(), "not a key".to_owned());
    assert!(find_problems(&texts).contains(mandatory));
}

// C#: SettingsShortcutsPageTests.TwoOptionalBoxesWithTheSameKey_AreAConflict_ButTwoEmptyOnesAreNot
#[test]
fn find_problems_marks_duplicate_keys_and_ignores_two_empty_optional_values() {
    let optional: Vec<_> = shortcut_properties()
        .iter()
        .copied()
        .filter(|property| ShortcutMappings::is_optional(property))
        .take(2)
        .collect();
    assert_eq!(optional.len(), 2);
    let mut texts = BTreeMap::new();
    texts.insert(optional[0].to_owned(), String::new());
    texts.insert(optional[1].to_owned(), String::new());
    assert!(find_problems(&texts).is_empty());

    texts.insert(optional[0].to_owned(), "F8".to_owned());
    texts.insert(optional[1].to_owned(), "F8".to_owned());
    assert_eq!(
        find_problems(&texts),
        optional.iter().map(|property| (*property).to_owned()).collect()
    );
}

// C#: SettingsShortcutsPageTests.AnAliasSpelling_CountsAsTheSameKey
#[test]
fn find_problems_counts_alias_spellings_as_the_same_key() {
    let rows: Vec<_> = all_rows().collect();
    let properties: Vec<_> = rows
        .iter()
        .filter(|row| !ShortcutMappings::is_optional(row.property))
        .take(2)
        .map(|row| row.property)
        .collect();
    let mut texts = BTreeMap::new();
    texts.insert(properties[0].to_owned(), "Return".to_owned());
    texts.insert(properties[1].to_owned(), "Enter".to_owned());
    assert_eq!(
        find_problems(&texts),
        properties.iter().map(|property| (*property).to_owned()).collect()
    );
}
