//! Settings > Shortcuts: the texts that reach the action editor.
//! C# `ShortcutsSettingsPage.cs` (7eaee2a2): `SettingsPresenter.ActionsForEditor` names the default actions with the ambient `Tr.ActionDefaultName`.

use std::sync::Arc;

use pr_l10n::{
    catalog::LanguageCatalog,
    localizer::{Localizer, built_in_english_localizer},
};
use pr_platform_win::dialogs::settings::builder::inputs::SettingsPageBuilder;
use pr_platform_win::dialogs::settings::page_view::SettingsPageView;
use pr_platform_win::dialogs::settings::pages::shortcuts::ShortcutsSettingsPage;
use pr_platform_win::win32::dialog_native::BN_CLICKED;

#[path = "common/settings_page_fixture.rs"]
mod settings_page_fixture;
use settings_page_fixture::PageHost;

fn vietnamese_localizer() -> Arc<Localizer> {
    let json = r#"{"_meta":{"code":"vi","name":"Vietnamese","nativeName":"Tieng Viet","plural":"Other"},"action.default.name":"Loai {n}","action.default.backup":"Sao luu"}"#;
    let vi = LanguageCatalog::try_parse(json, "test", &mut Vec::new()).expect("valid catalog");
    Arc::new(Localizer::create(pr_l10n::localizer::built_in_english(), &[vi], &[]))
}

// C#: SettingsPresenterTests.ActionsForEditor_NullGivesDefaults (the defaults use the ambient language, not English).
#[test]
fn edit_actions_with_null_json_opens_the_editor_with_default_names_in_the_ui_language() {
    Localizer::set_current(vietnamese_localizer());
    let mut page = ShortcutsSettingsPage::new();
    let mut builder = SettingsPageBuilder::new(5000);
    page.declare(&mut builder);
    let host = PageHost::new();
    host.configure_from(&builder);
    host.put_actions_json("null");
    page.attach(host.context());

    assert!(page.on_command(page.ids().edit_actions, BN_CLICKED));

    let lists = host.profile_names();
    Localizer::set_current(built_in_english_localizer());
    assert_eq!(lists.len(), 1, "the editor is opened once");
    assert_eq!(lists[0].first().map(String::as_str), Some("Loai 2"));
    assert!(lists[0].iter().any(|name| name == "Sao luu"), "{:?}", lists[0]);
}
