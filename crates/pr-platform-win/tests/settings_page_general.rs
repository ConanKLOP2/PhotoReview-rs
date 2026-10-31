#![allow(dead_code)]

#[path = "common/settings_builder_fixture.rs"]
mod settings_builder_fixture;
#[path = "common/settings_page_fixture.rs"]
mod settings_page_fixture;

use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_platform_win::dialogs::settings::{
    builder::inputs::SettingsPageBuilder,
    page_view::SettingsPageView,
    pages::general::{GeneralSettingsPage, SORT_TAGS},
};

#[test]
// C#: GeneralSettingsPage.Key / Declare
fn golden_general_declare_matches_the_csharp_dump() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    assert_eq!(page.key(), "General");
    // C# builder allocates the group box and combo label before the combo itself.
    assert_eq!(page.ids().language, 1003);
    assert_eq!(page.ids().language_group, 1001);
    assert_eq!(settings_builder_fixture::dump(&builder)["counts"]["elements"], 4);
}

#[test]
// C#: GeneralSettingsPage.SortTags
fn golden_sort_tags_match_the_csharp_array() {
    assert_eq!(
        SORT_TAGS,
        [
            "Name",
            "NameAscending",
            "NameDescending",
            "SizeDescending",
            "SizeAscending",
            "Default"
        ]
    );
}

#[test]
// C#: SettingsPagesTests.General_ViewMode_ComboOrderMatchesTheFormIndex
fn view_mode_combo_order_matches_the_form_index() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    host.configure_from(&builder);
    page.attach(host.context());
    for index in 0..6 {
        let state = SettingsFormState {
            view_mode_index: index,
            ..SettingsFormState::default()
        };
        page.load(&state);
        assert_eq!(host.combo_index(page.ids().view_mode), index);
    }
}

#[test]
// C#: SettingsPagesTests.General_SortMode_EveryTagRoundTrips_InComboOrder
fn sort_mode_every_tag_round_trips_in_combo_order() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    host.configure_from(&builder);
    page.attach(host.context());
    for (index, tag) in SORT_TAGS.iter().enumerate() {
        let mut state = SettingsFormState {
            sort_mode_tag: Some((*tag).to_owned()),
            ..SettingsFormState::default()
        };
        page.load(&state);
        assert_eq!(host.combo_index(page.ids().sort_mode), index as i32);
        page.collect(&mut state);
        assert_eq!(state.sort_mode_tag.as_deref(), Some(*tag));
    }
}

#[test]
// C#: SettingsPagesTests.General_SortMode_EveryComboItem_SavesTheModeItsTextNames
fn sort_mode_unknown_or_missing_tag_selects_the_first_item() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    host.configure_from(&builder);
    page.attach(host.context());
    for tag in [None, Some("Bogus"), Some("namedescending")] {
        let state = SettingsFormState {
            sort_mode_tag: tag.map(str::to_owned),
            ..SettingsFormState::default()
        };
        page.load(&state);
        assert_eq!(host.combo_index(page.ids().sort_mode), 0);
    }
}

#[test]
// C#: SettingsPagesTests.General_EditingCombos_WritesThroughToTheSettings
fn collect_sort_mode_out_of_range_gives_none() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    host.configure_from(&builder);
    page.attach(host.context());
    for value in [-1, 99] {
        host.put_combo_index(page.ids().sort_mode, value);
        let mut state = SettingsFormState::default();
        page.collect(&mut state);
        assert_eq!(state.sort_mode_tag, None);
    }
}

#[test]
// C#: SettingsPagesTests.General_InstanceMode_RoundTrips
fn instance_mode_round_trips() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    host.configure_from(&builder);
    page.attach(host.context());
    for index in 0..2 {
        let state = SettingsFormState {
            instance_mode_index: index,
            ..SettingsFormState::default()
        };
        page.load(&state);
        assert_eq!(host.combo_index(page.ids().instance_mode), index);
    }
}

#[test]
// C#: GeneralSettingsPage.Collect reads ViewModeIndex/InstanceModeIndex from the combos (G4/H1)
fn collect_reads_view_and_instance_mode_from_the_combos() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    host.configure_from(&builder);
    page.attach(host.context());
    let loaded = SettingsFormState {
        view_mode_index: 0,
        instance_mode_index: 0,
        ..SettingsFormState::default()
    };
    page.load(&loaded);
    host.put_combo_index(page.ids().view_mode, 1);
    host.put_combo_index(page.ids().instance_mode, 1);
    let mut state = loaded.clone();
    page.collect(&mut state);
    assert_eq!(state.view_mode_index, 1);
    assert_eq!(state.instance_mode_index, 1);
    assert_eq!(host.combo_index(page.ids().view_mode), 1);
    assert_eq!(host.combo_index(page.ids().instance_mode), 1);
}

#[test]
// C#: SettingsPagesTests.General_WithoutALocalizationService_HidesTheLanguageGroup
fn general_without_a_localization_service_hides_the_language_group() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    page.attach(host.context());
    assert!(!host.is_visible(page.ids().language_group));
    assert!(page.languages().is_empty());
    let mut state = SettingsFormState::default();
    page.collect(&mut state);
    assert_eq!(state.selected_language_code, None);
}

#[test]
// C#: SettingsPagesTests.General_OpenLanguagesFolder_AsksExplorerForTheFolder
fn open_languages_folder_asks_explorer_for_the_folder() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    page.attach(host.context());
    assert!(page.on_command(page.ids().open_folder, 0));
    assert!(host.explorer_args().is_empty());
}

#[test]
// C#: GeneralSettingsPage.OnCommand: an enabled update button handles the click.
fn update_check_click_is_handled_while_the_button_is_enabled() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    host.configure_from(&builder);
    page.attach(host.context());
    assert!(page.on_command(page.ids().check_update, 0));
}

#[test]
// C#: SettingsPagesTests.General_ShellIntegration_AddAndRemoveApplyImmediately_AndShowTheStatus
fn attach_calls_the_host_in_the_csharp_order() {
    let mut page = GeneralSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(1000);
    page.declare(&mut builder);
    let host = settings_page_fixture::PageHost::new();
    page.attach(host.context());
    assert_eq!(host.calls().first().map(String::as_str), Some("set_visible 1019 false"));
}

#[test]
// C#: SettingsPagesTests.General_WithoutALocalizationService_HidesTheLanguageGroup
fn a_page_that_was_never_declared_does_not_panic() {
    let mut page = GeneralSettingsPage::new();
    assert_eq!(page.key(), "General");
    assert!(!page.on_command(123456, 0));
}
