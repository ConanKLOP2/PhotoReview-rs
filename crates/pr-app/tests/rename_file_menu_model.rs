mod file_action_fakes;
mod presenters_support;
mod support {
    pub mod main_env;
    pub mod present_helpers;
    pub mod presenter_fakes;
    pub mod preview_fakes;
}

use pr_app::menus::builder::build;
use pr_app::menus::model::{ContextMenuBuildContext, MenuItemKind, MenuItemModel};
use pr_app::presenters::settings::SettingsPresenter;
use pr_app::presenters::settings_field_map::{FieldContext, bindings};
use pr_app::presenters::settings_form_state::SettingsFormState;
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::context_menu::{ContextMenuItemId, apply_hidden};
use pr_l10n::localizer::Localizer;
use pr_l10n::localizer::built_in_english_localizer;
use pr_l10n::template::NumberCulture;
use presenters_support::{english, vietnamese};
use std::sync::Mutex;
use support::main_env::MainEnv;
static LOCALIZER_LOCK: Mutex<()> = Mutex::new(());

fn context(settings: &AppSettings, has_image: bool, compare: bool) -> ContextMenuBuildContext<'_> {
    ContextMenuBuildContext {
        settings,
        has_image,
        can_undo: true,
        is_fit: true,
        effective_zoom: None,
        can_open_in_external_editor: true,
        can_copy_path: has_image,
        is_compare_visible: compare,
    }
}
fn entry(items: &[MenuItemModel]) -> &MenuItemModel {
    items
        .iter()
        .find(|i| i.id == "RenameFile")
        .expect("RenameFile menu item")
}

#[test]
fn with_a_photo_the_entry_is_a_command_with_the_f2_text_and_localized_label() {
    // C#: RenameFileMenuModelTests.WithAPhoto_TheEntryIsACommandWithTheF2Text_AndTheLocalizedLabel
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(english());
    let settings = AppSettings::default();
    let items = build(&context(&settings, true, false)).expect("build");
    let item = entry(&items);
    assert_eq!(item.kind, MenuItemKind::Command);
    assert_eq!(item.shortcut_text.as_deref(), Some("F2"));
    assert!(item.is_enabled);
    assert_eq!(item.text, "Rename…");
    assert!(item.children.is_empty());
    assert!(
        item.automation_name
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty())
    );
}

#[test]
fn vietnamese_has_its_own_label() {
    // C#: RenameFileMenuModelTests.Vietnamese_HasItsOwnLabel
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(vietnamese());
    let settings = AppSettings::default();
    assert_eq!(
        entry(&build(&context(&settings, true, false)).expect("build")).text,
        "Đổi tên…"
    );
}

#[test]
fn without_a_photo_the_entry_is_not_there_nor_is_its_separator_left_behind() {
    // C#: RenameFileMenuModelTests.WithoutAPhoto_TheEntryIsNotThere_NorIsItsSeparatorLeftBehind
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(english());
    let settings = AppSettings::default();
    let items = build(&context(&settings, false, false)).expect("build");
    assert!(!items.iter().any(|i| i.id == "RenameFile" || i.id == "Separator.5"));
    let ids: Vec<_> = items.iter().map(|i| i.id.as_str()).collect();
    let external = ids.iter().position(|id| *id == "ExternalEditor").unwrap();
    assert_eq!(&ids[external..], ["ExternalEditor", "Separator.6", "Settings"]);
    assert!(
        !ids.windows(2)
            .any(|pair| pair[0].starts_with("Separator") && pair[1].starts_with("Separator"))
    );
}

#[test]
fn a_cleared_key_shows_no_accelerator_text_but_the_entry_stays() {
    // C#: RenameFileMenuModelTests.AClearedKey_ShowsNoAcceleratorText_ButTheEntryStays
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(english());
    let mut settings = AppSettings::default();
    settings.shortcuts.rename_file.clear();
    let items = build(&context(&settings, true, false)).expect("build");
    assert!(entry(&items).shortcut_text.is_none());
}

#[test]
fn a_rebind_is_shown_in_the_menu() {
    // C#: RenameFileMenuModelTests.ARebind_IsShownInTheMenu
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(english());
    let mut settings = AppSettings::default();
    settings.shortcuts.rename_file = "F6".into();
    let items = build(&context(&settings, true, false)).expect("build");
    assert_eq!(entry(&items).shortcut_text.as_deref(), Some("F6"));
}

#[test]
fn hiding_the_item_removes_only_it() {
    // C#: RenameFileMenuModelTests.HidingTheItem_RemovesOnlyIt
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(english());
    let mut settings = AppSettings::default();
    apply_hidden(&mut settings, [ContextMenuItemId::RenameFile]);
    let items = build(&context(&settings, true, false)).expect("build");
    assert!(!items.iter().any(|i| i.id == "RenameFile"));
    assert!(items.iter().any(|i| i.id == "CopyFileName"));
    assert!(items.iter().any(|i| i.id == "CopyFullPath"));
}

#[test]
fn it_sits_in_the_copy_group_right_before_copy_file_name_by_default() {
    // C#: RenameFileMenuModelTests.ItSitsInTheCopyGroup_RightBeforeCopyFileName_ByDefault
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(english());
    let settings = AppSettings::default();
    let items = build(&context(&settings, true, false)).expect("build");
    let rename = items.iter().position(|i| i.id == "RenameFile").expect("rename");
    assert_eq!(items.get(rename + 1).map(|i| i.id.as_str()), Some("CopyFileName"));
    // The separator ids are renumbered by the builder; assert the kind, not the number.
    assert_eq!(
        &items[rename..rename + 3]
            .iter()
            .map(|i| i.id.as_str())
            .collect::<Vec<_>>(),
        &["RenameFile", "CopyFileName", "CopyFullPath"]
    );
    assert!(rename > 0 && items[rename - 1].kind == MenuItemKind::Separator);
    assert_eq!(items.get(rename + 3).map(|i| i.kind), Some(MenuItemKind::Separator));
    assert_eq!(items.get(rename + 4).map(|i| i.id.as_str()), Some("Settings"));
}

#[test]
fn compare_visible_still_offers_it_and_the_view_model_renames_the_selected_photo() {
    // C#: RenameFileMenuModelTests.CompareVisible_StillOffersIt_TheViewModelRenamesTheSelectedPhoto
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(english());
    let settings = AppSettings::default();
    assert!(entry(&build(&context(&settings, true, true)).expect("build")).is_enabled);
    let mut env = MainEnv::new();
    let (folder, files) = env.images("rename_compare_selected", &["a.jpg", "b.jpg", "c.jpg"]);
    env.open_at(&folder, &files[1]);
    env.vm.toggle_compare();
    env.settle();
    env.vm.select_compare_right();
    env.vm.prompt_rename_current();
    let plan = env.vm.take_rename_prompt().expect("queued plan");
    assert_eq!(plan.source(), files[1]);
    let source = plan.source().to_owned();
    env.vm.finish_rename_prompt(&source, Some("selected"));
    env.settle();
    assert!(std::path::Path::new(&folder).join("selected.jpg").exists());
    assert!(std::path::Path::new(&files[2]).exists());
    assert!(!std::path::Path::new(&files[1]).exists());
}

#[test]
fn the_settings_mouse_page_lists_the_item_as_a_tick_box_with_its_own_label() {
    // C#: RenameFileMenuModelTests.TheSettingsMousePage_ListsTheItemAsATickBox_WithItsOwnLabel
    let _guard = LOCALIZER_LOCK.lock().unwrap();
    Localizer::set_current(english());
    let settings = AppSettings::default();
    let mut state = SettingsFormState::default();
    for row in bindings() {
        (row.load)(&settings, &mut state);
    }
    assert!(state.context_menu_shown.contains(&ContextMenuItemId::RenameFile));
    let texts = built_in_english_localizer();
    let numbers = NumberCulture::invariant();
    let ctx = FieldContext {
        texts: &texts,
        numbers: &numbers,
    };
    assert_eq!(
        SettingsPresenter::context_menu_item_label(ContextMenuItemId::RenameFile, &ctx),
        "Rename…"
    );
}
