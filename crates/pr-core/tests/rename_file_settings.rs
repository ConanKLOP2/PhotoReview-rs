use std::collections::HashSet;
use std::sync::Arc;

use pr_core::abstractions::KeyNameValidator;
use pr_core::settings::app_settings::{AppSettings, EnglishSettingsTexts};
use pr_core::settings::context_menu::{
    ContextMenuContext, ContextMenuItemId as Id, compute, item_info, parse_hidden, to_stored,
};
use pr_core::settings::normalizer::{SettingsValidator, disable_conflicting_optional_shortcuts, normalize};
use pr_core::settings::shortcuts::ShortcutMappings;

struct AnyKey;
impl KeyNameValidator for AnyKey {
    fn is_valid_key_name(&self, _: &str) -> bool {
        true
    }
}

#[test]
fn the_menu_item_id_is_appended_at_the_end_so_stored_names_and_values_of_the_older_items_do_not_move() {
    // C#: RenameFileSettingsTests.TheMenuItemId_IsAppendedAtTheEnd_SoStoredNamesAndValuesOfTheOlderItemsDoNotMove
    assert_eq!(Id::ALL.last(), Some(&Id::RenameFile));
    assert_eq!(Id::RenameFile as i32, 18);
    assert_eq!(Id::Refresh as i32, 17);
    assert_eq!(Id::RenameFile.name(), "RenameFile");
}

#[test]
fn the_menu_item_needs_a_photo_is_top_level_sits_beside_copy_and_is_shown_by_default() {
    // C#: RenameFileSettingsTests.TheMenuItem_NeedsAPhoto_IsTopLevel_SitsBesideTheCopyItems_AndIsShownByDefault
    let row = item_info(Id::RenameFile);
    assert!(row.needs_photo);
    assert!(!row.locked);
    assert_eq!(row.parent, None);
    assert_eq!(row.group, item_info(Id::CopyFileName).group);
    assert!(
        !AppSettings::default()
            .hidden_context_menu_items
            .is_some_and(|v| v.iter().flatten().any(|x| x == "RenameFile"))
    );
}

#[test]
fn the_menu_item_with_and_without_a_photo_computes_into_the_layout_accordingly() {
    // C#: RenameFileSettingsTests.TheMenuItem_WithAndWithoutAPhoto_ComputesIntoTheLayoutAccordingly
    let hidden = HashSet::new();
    assert!(
        compute(
            &hidden,
            ContextMenuContext {
                has_photo: true,
                include_shell_only: true
            },
            None
        )
        .is_visible(Id::RenameFile)
    );
    assert!(
        !compute(
            &hidden,
            ContextMenuContext {
                has_photo: false,
                include_shell_only: true
            },
            None
        )
        .is_visible(Id::RenameFile)
    );
    let only_shell_hidden = HashSet::from([Id::RenameFile]);
    assert!(
        !compute(
            &only_shell_hidden,
            ContextMenuContext {
                has_photo: true,
                include_shell_only: true
            },
            None
        )
        .is_visible(Id::RenameFile)
    );
}

#[test]
fn the_wpf_layout_never_counts_the_win32_only_item_nor_leaves_its_separator_behind() {
    // C#: RenameFileSettingsTests.TheWpfLayout_NeverCountsTheWin32OnlyItem_NorLeavesItsSeparatorBehind
    let hidden = HashSet::from([Id::CopyFileName, Id::CopyFullPath]);
    let wpf = compute(
        &hidden,
        ContextMenuContext {
            has_photo: true,
            include_shell_only: false,
        },
        None,
    );
    let shell = compute(
        &hidden,
        ContextMenuContext {
            has_photo: true,
            include_shell_only: true,
        },
        None,
    );
    assert!(!wpf.is_visible(Id::RenameFile));
    assert!(!wpf.has_separator_before(5));
    assert!(shell.is_visible(Id::RenameFile));
    assert!(shell.has_separator_before(5));
}

#[test]
fn the_hidden_name_roundtrips_through_parse_and_store() {
    // C#: RenameFileSettingsTests.TheHiddenNameRoundTrips_ThroughParseAndStore
    let stored = to_stored([Id::RenameFile, Id::Fit]);
    assert_eq!(stored, ["Fit", "RenameFile"]);
    assert!(parse_hidden(&stored.into_iter().map(Some).collect::<Vec<_>>()).contains(&Id::RenameFile));
}

#[test]
fn the_shortcut_defaults_to_f2_is_optional_and_a_cleared_key_stays_cleared() {
    // C#: RenameFileSettingsTests.TheShortcut_DefaultsToF2_IsOptional_AndAClearedKeyStaysCleared
    assert_eq!(AppSettings::default().shortcuts.rename_file, "F2");
    assert_eq!(ShortcutMappings::default().rename_file, "F2");
    assert!(ShortcutMappings::is_optional("RenameFile"));
    let mut settings = AppSettings::default();
    settings.shortcuts.rename_file = "  ".into();
    normalize(&mut settings);
    assert_eq!(settings.shortcuts.rename_file, "");
}

#[test]
fn the_shortcut_is_trimmed_and_null_becomes_cleared() {
    // C#: RenameFileSettingsTests.TheShortcut_IsTrimmed_AndANullBecomesCleared
    let mut settings = AppSettings::default();
    settings.shortcuts.rename_file = " F6 ".into();
    normalize(&mut settings);
    assert_eq!(settings.shortcuts.rename_file, "F6");
    let parsed = AppSettings::from_json(r#"{"Shortcuts":{"RenameFile":null}}"#, &EnglishSettingsTexts)
        .expect("parse")
        .expect("object");
    assert_eq!(parsed.shortcuts.rename_file, "");
}

#[test]
fn an_old_config_without_the_key_gets_f2_and_a_cleared_key_is_kept_through_save_and_load() {
    // C#: RenameFileSettingsTests.AnOldConfigWithoutTheKey_GetsF2_AndAClearedKeyIsKeptThroughSaveAndLoad
    let old = AppSettings::from_json(
        r#"{"ConfigVersion":4,"Shortcuts":{"Refresh":"R","Next":"Right"}}"#,
        &EnglishSettingsTexts,
    )
    .expect("parse")
    .expect("object");
    assert_eq!(old.shortcuts.rename_file, "F2");
    let mut cleared = old;
    cleared.shortcuts.rename_file.clear();
    let saved = cleared.to_json().expect("serialize");
    let loaded = AppSettings::from_json(&saved, &EnglishSettingsTexts)
        .expect("parse")
        .expect("object");
    assert_eq!(loaded.shortcuts.rename_file, "");
}

#[test]
fn a_user_action_on_f2_wins_over_the_new_default_and_the_rename_key_is_silently_disabled() {
    // C#: RenameFileSettingsTests.AUserActionOnF2_WinsOverTheNewDefault_AndTheRenameKeyIsSilentlyDisabled
    let mut settings = AppSettings::default();
    settings.actions[0].shortcut = "F2".into();
    assert!(disable_conflicting_optional_shortcuts(&mut settings).contains(&"RenameFile".to_owned()));
    assert_eq!(settings.shortcuts.rename_file, "");
    assert_eq!(settings.actions[0].shortcut, "F2");
}

#[test]
fn a_key_also_used_by_another_optional_shortcut_is_a_conflict_in_the_validator() {
    // C#: RenameFileSettingsTests.AKeyAlsoUsedByAnotherOptionalShortcut_IsAConflictInTheValidator
    let validator = SettingsValidator::new(Arc::new(AnyKey), Arc::new(EnglishSettingsTexts));
    let mut settings = AppSettings::default();
    settings.shortcuts.rename_file = settings.shortcuts.refresh.clone();
    assert!(validator.validate_shortcuts(&settings).is_some());
    settings.shortcuts.rename_file = "F8".into();
    assert!(validator.validate_shortcuts(&settings).is_none());
}
